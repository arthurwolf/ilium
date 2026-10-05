use std::cell::RefCell;
use std::collections::VecDeque;

use super::*;
use crate::input::prepare_input;
use crate::neutral::{Part, Role, Turn};
use crate::types::SummaryResponse;

/// A summarizer driven by a script of replies; records every request.
struct ScriptedSummarizer {
    script: RefCell<VecDeque<Result<String, SummarizerError>>>,
    requests: RefCell<Vec<SummaryRequest>>,
    /// Replies used once the script runs out.
    default_reply: Result<String, SummarizerError>,
}

impl ScriptedSummarizer {
    fn new(
        script: Vec<Result<String, SummarizerError>>,
        default_reply: Result<String, SummarizerError>,
    ) -> Self {
        Self {
            script: RefCell::new(script.into()),
            requests: RefCell::new(Vec::new()),
            default_reply,
        }
    }

    fn always(reply: &str) -> Self {
        Self::new(Vec::new(), Ok(reply.to_string()))
    }

    fn request_count(&self) -> usize {
        self.requests.borrow().len()
    }
}

impl Summarizer for ScriptedSummarizer {
    fn summarize(&self, request: &SummaryRequest) -> Result<SummaryResponse, SummarizerError> {
        self.requests.borrow_mut().push(request.clone());
        let reply = self
            .script
            .borrow_mut()
            .pop_front()
            .unwrap_or_else(|| self.default_reply.clone());
        reply.map(|text| SummaryResponse {
            text,
            input_tokens: Some(100),
            output_tokens: Some(10),
        })
    }
}

const GOOD: &str = "Handoff summary: the user wants feature X; Y is done; next run the tests.";

fn turns(count: usize, chars: usize) -> Vec<Turn> {
    (0..count)
        .map(|index| {
            let role = if index % 2 == 0 {
                Role::User
            } else {
                Role::Assistant
            };
            Turn::new(
                role,
                vec![Part::Text(format!("{index}:{}", "w".repeat(chars)))],
            )
        })
        .collect()
}

fn options(context_tokens: u64) -> CompactionOptions {
    CompactionOptions {
        technique: Technique::Codex,
        summarizer_context_tokens: context_tokens,
        summarizer_max_output_tokens: 200,
        ..CompactionOptions::default()
    }
}

struct Outcome {
    result: Result<SummaryResult, CompactionError>,
    logs: Vec<String>,
}

fn run(
    summarizer: &ScriptedSummarizer,
    history: &[Turn],
    options: &CompactionOptions,
    cancel: &AtomicBool,
) -> Outcome {
    let prepared = prepare_input(history, options);
    let context = SummarizeContext {
        technique: resolve_technique(options),
        options,
        prepared: &prepared,
        summarizer,
        cancel,
        facts: None,
    };
    let mut logs = Vec::new();
    let result = {
        let mut emit = |event| {
            if let crate::types::CompactionEvent::Log(line) = event {
                logs.push(line);
            }
        };
        let mut reporter = Reporter::new(&mut emit);
        summarize_history(&context, &mut reporter)
    };
    Outcome { result, logs }
}

#[test]
fn a_history_that_fits_costs_one_call() {
    let summarizer = ScriptedSummarizer::always(GOOD);
    let outcome = run(
        &summarizer,
        &turns(6, 200),
        &options(50_000),
        &AtomicBool::new(false),
    );
    let result = outcome.result.expect("summarizes");
    assert_eq!((result.chunks, result.used_fallback), (1, false));
    assert_eq!(result.text, GOOD);
    assert_eq!(summarizer.request_count(), 1);
    assert_eq!(summarizer.requests.borrow()[0].label, "summary");
    assert_eq!(summarizer.requests.borrow()[0].max_output_tokens, 200);
}

#[test]
fn a_long_history_runs_anchored_chunks_then_one_merge() {
    let summarizer = ScriptedSummarizer::new(
        vec![
            Ok("Handoff part one summary text, long enough to pass.".into()),
            Ok("Handoff part two summary text, long enough to pass.".into()),
        ],
        Ok(GOOD.to_string()),
    );
    // Codex system prompt ~350 tokens; 1_700 context leaves ~256-token chunks.
    let history = turns(12, 600);
    let outcome = run(
        &summarizer,
        &history,
        &options(1_700),
        &AtomicBool::new(false),
    );
    let result = outcome.result.expect("summarizes");
    let requests = summarizer.requests.borrow();
    assert!(result.chunks >= 3, "chunks: {}", result.chunks);
    assert_eq!(requests.len(), result.chunks + 1);
    assert!(requests[0].label.starts_with("chunk 1/"));
    assert!(requests[0].user.contains("part 1 of"));
    assert!(!requests[0].user.contains("<prior-summary>"));
    assert!(requests[1]
        .user
        .contains("<prior-summary>\nHandoff part one summary"));
    assert!(requests[2]
        .user
        .contains("<prior-summary>\nHandoff part two summary"));
    let merge = requests.last().expect("merge call");
    assert_eq!(merge.label, "merge");
    assert!(merge.user.contains("[part 1]") && merge.user.contains("[part 2]"));
    assert_eq!(result.text, GOOD);
}

#[test]
fn a_prior_summary_anchors_the_first_chunk() {
    let mut history = vec![Turn::new(
        Role::EarlierSummary,
        vec![Part::Text("EARLIER STATE".into())],
    )];
    history.extend(turns(4, 100));
    let summarizer = ScriptedSummarizer::always(GOOD);
    run(
        &summarizer,
        &history,
        &options(50_000),
        &AtomicBool::new(false),
    )
    .result
    .expect("summarizes");
    assert!(summarizer.requests.borrow()[0]
        .user
        .contains("<prior-summary>\nEARLIER STATE\n</prior-summary>"));
}

#[test]
fn context_too_long_rechunks_at_half_size() {
    let summarizer = ScriptedSummarizer::new(
        vec![Err(SummarizerError::ContextTooLong("too big".into()))],
        Ok(GOOD.to_string()),
    );
    let history = turns(8, 600);
    let outcome = run(
        &summarizer,
        &history,
        &options(3_000),
        &AtomicBool::new(false),
    );
    let result = outcome.result.expect("summarizes");
    assert!(!result.used_fallback);
    assert!(
        result.chunks >= 2,
        "halving must split the history: {}",
        result.chunks
    );
    assert!(outcome.logs.iter().any(|line| line.contains("too long")));
    let requests = summarizer.requests.borrow();
    assert!(requests[1].user.len() < requests[0].user.len());
}

#[test]
fn repeated_context_too_long_ends_in_the_fallback() {
    let summarizer = ScriptedSummarizer::new(
        Vec::new(),
        Err(SummarizerError::ContextTooLong("always".into())),
    );
    let outcome = run(
        &summarizer,
        &turns(6, 400),
        &options(50_000),
        &AtomicBool::new(false),
    );
    let result = outcome.result.expect("falls back");
    assert!(result.used_fallback);
    assert_eq!(summarizer.request_count(), MAX_HALVINGS + 1);
}

#[test]
fn a_malformed_reply_is_retried_once_with_the_reason() {
    let summarizer = ScriptedSummarizer::new(vec![Ok("   ".into())], Ok(GOOD.to_string()));
    let outcome = run(
        &summarizer,
        &turns(4, 100),
        &options(50_000),
        &AtomicBool::new(false),
    );
    let result = outcome.result.expect("summarizes");
    assert!(!result.used_fallback);
    let requests = summarizer.requests.borrow();
    assert_eq!(requests.len(), 2);
    assert!(!requests[0].user.contains("previous reply was rejected"));
    assert!(requests[1]
        .user
        .contains("Your previous reply was rejected: the reply was empty"));
}

#[test]
fn a_summarizer_that_never_works_yields_the_fallback_with_requests_and_ledger() {
    let mut history = turns(4, 50);
    history.push(Turn::new(
        Role::Assistant,
        vec![Part::ToolCall {
            id: "c".into(),
            name: "Read".into(),
            arguments: serde_json::json!({"file_path": "/src/lib.rs"}),
        }],
    ));
    let summarizer =
        ScriptedSummarizer::new(Vec::new(), Err(SummarizerError::Failed("offline".into())));
    let outcome = run(
        &summarizer,
        &history,
        &options(50_000),
        &AtomicBool::new(false),
    );
    let result = outcome.result.expect("falls back");
    assert!(result.used_fallback);
    assert!(result.text.contains("assembled mechanically"));
    assert!(
        result.text.contains("- 0:"),
        "first user request listed: {}",
        result.text
    );
    assert!(result.text.contains("/src/lib.rs"));
    assert_eq!(summarizer.request_count(), 2, "one retry for a failure");
}

#[test]
fn a_failure_after_some_chunks_keeps_the_partial_summary() {
    let summarizer = ScriptedSummarizer::new(
        vec![Ok(
            "Handoff part one summary text, long enough to pass.".into()
        )],
        Err(SummarizerError::Failed("offline".into())),
    );
    let outcome = run(
        &summarizer,
        &turns(12, 600),
        &options(1_700),
        &AtomicBool::new(false),
    );
    let result = outcome.result.expect("falls back");
    assert!(result.used_fallback);
    assert!(result.text.contains("Handoff part one summary text"));
    assert_eq!(result.chunks, 1);
}

#[test]
fn a_failed_merge_keeps_the_cumulative_last_chunk_summary() {
    let summarizer = ScriptedSummarizer::new(
        vec![
            Ok("Handoff part one summary text, long enough to pass.".into()),
            Ok("Handoff part two summary text, long enough to pass.".into()),
            Ok("Handoff part three summary text, long enough to pass.".into()),
        ],
        Err(SummarizerError::Failed("merge offline".into())),
    );
    let outcome = run(
        &summarizer,
        &turns(10, 600),
        &options(1_700),
        &AtomicBool::new(false),
    );
    let result = outcome.result.expect("summarizes");
    assert!(!result.used_fallback);
    assert!(result.text.starts_with("Handoff part"));
    assert!(
        outcome
            .logs
            .iter()
            .any(|line| line.contains("merge call failed")),
        "logs: {:#?}",
        outcome.logs
    );
}

#[test]
fn cancellation_stops_before_and_between_calls() {
    let summarizer = ScriptedSummarizer::always(GOOD);
    let cancelled = AtomicBool::new(true);
    let outcome = run(&summarizer, &turns(4, 100), &options(50_000), &cancelled);
    assert!(matches!(outcome.result, Err(CompactionError::Cancelled)));
    assert_eq!(summarizer.request_count(), 0);

    let summarizer = ScriptedSummarizer::new(Vec::new(), Err(SummarizerError::Cancelled));
    let outcome = run(
        &summarizer,
        &turns(4, 100),
        &options(50_000),
        &AtomicBool::new(false),
    );
    assert!(matches!(outcome.result, Err(CompactionError::Cancelled)));
}

#[test]
fn technique_validation_applies_to_the_reply() {
    let mut settings = options(50_000);
    settings.technique = Technique::Opencode;
    let summarizer = ScriptedSummarizer::new(
        vec![Ok(
            "## Objective\n- only this heading exists in the reply".into()
        )],
        Ok("## Objective\n- x\n## Work State\n### Active\n- y\n## Next Move\n1. z".into()),
    );
    let outcome = run(
        &summarizer,
        &turns(4, 100),
        &settings,
        &AtomicBool::new(false),
    );
    assert!(!outcome.result.expect("summarizes").used_fallback);
    assert!(summarizer.requests.borrow()[1]
        .user
        .contains("required parts are missing"));
}

//! Shared helpers of the integration tests: fixture copies, a scripted fake
//! summarizer, and an independent reader that validates rewritten transcripts
//! without using the crate's own verification.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use ilium_remote_compaction::{
    compact_session, AgentKind, CompactionError, CompactionEvent, CompactionOptions,
    CompactionOutcome, CompactionRequest, Summarizer, SummarizerError, SummaryRequest,
    SummaryResponse, Technique,
};
use serde_json::Value;

pub const CLAUDE_SESSION_ID: &str = "11111111-1111-4111-8111-111111111111";
pub const CODEX_SESSION_ID: &str = "22222222-2222-4222-8222-222222222222";
pub const SECRET: &str = "sk-ABCDEFGHIJKLMNOPQRSTUV123456";

pub fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// Copies a fixture into `dir` under the file name the agent would use.
pub fn install_fixture(dir: &Path, agent: AgentKind) -> PathBuf {
    let (source, target) = match agent {
        AgentKind::Claude => ("claude_session.jsonl", format!("{CLAUDE_SESSION_ID}.jsonl")),
        AgentKind::Codex => (
            "codex_rollout.jsonl",
            format!("rollout-2026-10-05T10-00-00-{CODEX_SESSION_ID}.jsonl"),
        ),
    };
    let path = dir.join(target);
    std::fs::copy(fixture_path(source), &path).expect("copy fixture");
    path
}

pub fn small_options(technique: Technique) -> CompactionOptions {
    CompactionOptions {
        technique,
        tail_tokens: 150,
        protected_recent_tool_tokens: 100,
        tool_result_chars: 300,
        summarizer_context_tokens: 100_000,
        summarizer_max_output_tokens: 500,
        ..CompactionOptions::default()
    }
}

pub fn request_for(agent: AgentKind, path: &Path, options: CompactionOptions) -> CompactionRequest {
    CompactionRequest {
        agent,
        transcript_path: path.to_path_buf(),
        session_id: match agent {
            AgentKind::Claude => CLAUDE_SESSION_ID,
            AgentKind::Codex => CODEX_SESSION_ID,
        }
        .to_string(),
        project_cwd: PathBuf::from("/repo"),
        options,
        context_window_tokens: Some(200_000),
    }
}

/// A well-formed reply for each technique.
pub fn good_reply(technique: Technique) -> String {
    match technique {
        Technique::ClaudeCode | Technique::BestOfAllWorlds => "<analysis>scratch</analysis>\n<summary>\n1. Primary Request and Intent:\n   FIRST SUMMARY MARKER add a verbose flag\n7. Pending Tasks:\n   - update the README\n8. Current Work:\n   reading README.md\n</summary>".to_string(),
        Technique::Opencode => "## Objective\n- FIRST SUMMARY MARKER add a verbose flag\n## Work State\n### Active\n- README\n## Next Move\n1. update README".to_string(),
        Technique::GeminiCli => "<scratchpad>x</scratchpad><state_snapshot><overall_goal>FIRST SUMMARY MARKER</overall_goal><task_state>README next</task_state></state_snapshot>".to_string(),
        Technique::Codex | Technique::Custom => "FIRST SUMMARY MARKER: the verbose flag is done and the README update is next.".to_string(),
    }
}

type Reply = Result<String, SummarizerError>;

/// Scripted summarizer. `on_call` runs inside every call (to simulate the
/// agent appending to the transcript or the user cancelling).
pub struct Fake<'a> {
    script: RefCell<VecDeque<Reply>>,
    default_reply: Reply,
    pub requests: RefCell<Vec<SummaryRequest>>,
    on_call: Option<Box<dyn Fn() + 'a>>,
}

impl<'a> Fake<'a> {
    pub fn always(reply: &str) -> Self {
        Self::scripted(Vec::new(), Ok(reply.to_string()))
    }

    pub fn failing() -> Self {
        Self::scripted(Vec::new(), Err(SummarizerError::Failed("offline".into())))
    }

    pub fn scripted(script: Vec<Reply>, default_reply: Reply) -> Self {
        Self {
            script: RefCell::new(script.into()),
            default_reply,
            requests: RefCell::new(Vec::new()),
            on_call: None,
        }
    }

    pub fn on_call(mut self, hook: impl Fn() + 'a) -> Self {
        self.on_call = Some(Box::new(hook));
        self
    }

    pub fn calls(&self) -> usize {
        self.requests.borrow().len()
    }
}

impl Summarizer for Fake<'_> {
    fn summarize(&self, request: &SummaryRequest) -> Result<SummaryResponse, SummarizerError> {
        self.requests.borrow_mut().push(request.clone());
        if let Some(hook) = &self.on_call {
            hook();
        }
        let reply = self
            .script
            .borrow_mut()
            .pop_front()
            .unwrap_or_else(|| self.default_reply.clone());
        reply.map(|text| SummaryResponse {
            text,
            input_tokens: None,
            output_tokens: None,
        })
    }
}

pub struct Run {
    pub result: Result<CompactionOutcome, CompactionError>,
    pub events: Vec<CompactionEvent>,
}

pub fn run(request: &CompactionRequest, summarizer: &dyn Summarizer, cancel: &AtomicBool) -> Run {
    let mut events = Vec::new();
    let result = compact_session(request, summarizer, cancel, &mut |event| events.push(event));
    Run { result, events }
}

pub fn run_ok(
    request: &CompactionRequest,
    summarizer: &dyn Summarizer,
) -> (CompactionOutcome, Vec<CompactionEvent>) {
    let finished = run(request, summarizer, &AtomicBool::new(false));
    (
        finished.result.expect("compaction succeeds"),
        finished.events,
    )
}

pub fn parse_lines(path: &Path) -> Vec<Value> {
    let text = std::fs::read_to_string(path).expect("read");
    text.lines()
        .map(|line| {
            serde_json::from_str(line).unwrap_or_else(|error| panic!("bad line {line:?}: {error}"))
        })
        .collect()
}

/// Names of the files in a directory, sorted.
pub fn dir_listing(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .expect("list")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

pub fn uuid_of(value: &Value) -> Option<&str> {
    value.get("uuid").and_then(Value::as_str)
}

/// Independent Claude validation: walks `parentUuid` from the newest record to
/// the last boundary and returns the chain (oldest first).
pub fn claude_active_chain(records: &[Value]) -> Vec<&Value> {
    let boundary_position = records
        .iter()
        .rposition(|record| record["subtype"] == "compact_boundary")
        .expect("a boundary exists");
    let by_uuid: std::collections::HashMap<&str, &Value> = records
        .iter()
        .filter_map(|record| uuid_of(record).map(|uuid| (uuid, record)))
        .collect();
    let mut chain = vec![records.last().expect("records")];
    while chain.last().map(|record| uuid_of(record)) != Some(uuid_of(&records[boundary_position])) {
        let parent = chain.last().expect("chain")["parentUuid"]
            .as_str()
            .expect("parentUuid is set before the boundary");
        chain.push(
            by_uuid
                .get(parent)
                .unwrap_or_else(|| panic!("missing parent {parent}")),
        );
        assert!(chain.len() <= records.len(), "cycle in the parent chain");
    }
    chain.reverse();
    chain
}

/// Every tool_use of the chain has exactly one tool_result and the reverse.
pub fn assert_tool_pairs_complete(chain: &[&Value]) {
    let mut used = Vec::new();
    let mut answered = Vec::new();
    for record in chain {
        for block in record["message"]["content"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default()
        {
            match block["type"].as_str() {
                Some("tool_use") => used.push(block["id"].as_str().expect("id").to_string()),
                Some("tool_result") => {
                    answered.push(block["tool_use_id"].as_str().expect("id").to_string())
                }
                _ => {}
            }
        }
    }
    used.sort();
    answered.sort();
    assert_eq!(used, answered, "every tool_use needs one tool_result");
}

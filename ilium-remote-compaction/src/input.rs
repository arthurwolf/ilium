//! Everything between the neutral conversation and the summarizer: tail
//! selection, redaction, masking of old tool output, rendering to prompt text
//! and chunking at turn boundaries.

use std::collections::{HashMap, HashSet};

use serde_json::{json, Value};

use crate::ledger::{truncate_chars, Ledger};
use crate::neutral::{Part, Role, Turn};
use crate::redact::{redact_text, redact_value};
use crate::tokens::estimate_tokens;
use crate::types::CompactionOptions;

/// Longest rendering of one tool call's arguments inside the prompt.
const MAX_ARGUMENT_CHARS: usize = 1_200;
/// Newest user requests the fallback summary lists.
const FALLBACK_REQUEST_COUNT: usize = 12;
const FALLBACK_REQUEST_CHARS: usize = 600;

// ------------------------------------------------------------ tail selection

/// Index at which the verbatim tail starts: `turns[split..]` is kept as is and
/// `turns[..split]` is summarized.
///
/// Rules: the tail fits `tail_tokens` but never more than half of the whole
/// conversation (so something is always summarized); it starts at a user turn
/// when one is available; it never separates a tool call from its result; it
/// never contains an earlier summary (that is an anchor, not history).
pub(crate) fn select_tail(turns: &[Turn], tail_tokens: u64) -> usize {
    let total: u64 = turns.iter().map(Turn::estimated_tokens).sum();
    let budget = tail_tokens.min(total / 2);
    let last_summary = turns
        .iter()
        .rposition(|turn| turn.role == Role::EarlierSummary);
    let floor = last_summary.map_or(0, |index| index + 1);

    let mut split = turns.len();
    let mut used = 0;
    while split > floor {
        let cost = turns[split - 1].estimated_tokens();
        if used + cost > budget {
            break;
        }
        used += cost;
        split -= 1;
    }
    if let Some(offset) = turns[split..]
        .iter()
        .position(|turn| turn.role == Role::User)
    {
        split += offset;
    }
    pair_safe_split(turns, split).max(floor)
}

/// Moves `split` earlier until no tool result in the tail answers a call that
/// is in the summarized part.
fn pair_safe_split(turns: &[Turn], mut split: usize) -> usize {
    loop {
        let defined_in_tail: HashSet<&str> = turns[split..]
            .iter()
            .flat_map(|turn| turn.parts.iter())
            .filter_map(|part| match part {
                Part::ToolCall { id, .. } => Some(id.as_str()),
                _ => None,
            })
            .collect();
        let mut earliest = split;
        for part in turns[split..].iter().flat_map(|turn| turn.parts.iter()) {
            let Part::ToolResult { call_id, .. } = part else {
                continue;
            };
            if defined_in_tail.contains(call_id.as_str()) {
                continue;
            }
            if let Some(index) = turns[..split].iter().rposition(|turn| {
                turn.parts.iter().any(
                    |candidate| matches!(candidate, Part::ToolCall { id, .. } if id == call_id),
                )
            }) {
                earliest = earliest.min(index);
            }
        }
        if earliest == split {
            return split;
        }
        split = earliest;
    }
}

// ----------------------------------------------------------------- redaction

/// Redacts secrets in every part of the turns; returns the replacement count.
pub(crate) fn redact_turns(turns: &mut [Turn]) -> usize {
    let mut count = 0;
    for turn in turns {
        for part in &mut turn.parts {
            match part {
                Part::Text(text) | Part::Omitted(text) => count += redact_in_place(text),
                Part::ToolResult { text, .. } => count += redact_in_place(text),
                Part::ToolCall { arguments, .. } => count += redact_value(arguments),
            }
        }
    }
    count
}

fn redact_in_place(text: &mut String) -> usize {
    let (redacted, count) = redact_text(text);
    if count > 0 {
        *text = redacted;
    }
    count
}

// ------------------------------------------------------------------- masking

/// Keeps the first and last halves of `text` within `max_chars` and notes how
/// many characters were left out, using the given marker template.
fn shorten_middle(text: &str, max_chars: usize, marker_template: &str) -> String {
    let total = text.chars().count();
    if total <= max_chars {
        return text.to_string();
    }
    let head_chars = max_chars / 2;
    let tail_chars = max_chars - head_chars;
    let head: String = text.chars().take(head_chars).collect();
    let tail: String = text.chars().skip(total - tail_chars).collect();
    let marker = ilium_prompts::render_value(
        marker_template,
        &json!({ "omitted": (total - head_chars - tail_chars).to_string() }),
    );
    format!("{head}{}{tail}", marker.trim_end())
}

/// Replaces the text of tool results older than the newest
/// `protected_tokens` of tool output with a head-and-tail stub. Returns the
/// estimated tokens saved.
pub(crate) fn mask_tool_results(
    turns: &mut [Turn],
    protected_tokens: u64,
    stub_chars: usize,
) -> u64 {
    let mut protected_used = 0;
    let mut budget_exhausted = false;
    let mut saved = 0;
    for turn in turns.iter_mut().rev() {
        for part in turn.parts.iter_mut().rev() {
            let Part::ToolResult { text, .. } = part else {
                continue;
            };
            let cost = estimate_tokens(text);
            if !budget_exhausted && protected_used + cost <= protected_tokens {
                protected_used += cost;
                continue;
            }
            budget_exhausted = true;
            if text.chars().count() <= stub_chars {
                continue;
            }
            let stub = shorten_middle(text, stub_chars, "compaction/tool-result-omitted");
            saved += cost.saturating_sub(estimate_tokens(&stub));
            *text = stub;
        }
    }
    saved
}

// ----------------------------------------------------------------- rendering

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RenderedTurn {
    pub(crate) text: String,
    pub(crate) tokens: u64,
}

fn render_arguments(arguments: &Value) -> String {
    let text = match arguments {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    };
    truncate_chars(&text, MAX_ARGUMENT_CHARS)
}

fn render_turn(number: usize, turn: &Turn) -> String {
    let label = match turn.role {
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::ToolResults => "tool results",
        Role::EarlierSummary => "earlier summary",
    };
    let mut lines = vec![format!("[turn {number} | {label}]")];
    for part in &turn.parts {
        match part {
            Part::Text(text) | Part::Omitted(text) => lines.push(text.clone()),
            Part::ToolCall {
                id,
                name,
                arguments,
            } => lines.push(format!(
                "tool call {id}: {name} {}",
                render_arguments(arguments)
            )),
            Part::ToolResult {
                call_id,
                text,
                is_error,
            } => {
                let failed = if *is_error { " (error)" } else { "" };
                lines.push(format!("tool result for {call_id}{failed}:\n{text}"));
            }
        }
    }
    lines.join("\n")
}

/// Renders the conversation turns; earlier summaries are skipped because they
/// travel as the prior summary.
pub(crate) fn render_turns(turns: &[Turn]) -> Vec<RenderedTurn> {
    turns
        .iter()
        .filter(|turn| turn.role != Role::EarlierSummary)
        .enumerate()
        .map(|(index, turn)| {
            let text = render_turn(index + 1, turn);
            let tokens = estimate_tokens(&text);
            RenderedTurn { text, tokens }
        })
        .collect()
}

// ------------------------------------------------------------------ chunking

/// End (exclusive) of the chunk starting at `start`: as many whole turns as
/// fit `budget_tokens`, at least one. A single oversized turn is truncated
/// when the chunk text is built.
pub(crate) fn chunk_end(rendered: &[RenderedTurn], start: usize, budget_tokens: u64) -> usize {
    let mut end = start;
    let mut used = 0;
    while end < rendered.len() {
        let cost = rendered[end].tokens.min(budget_tokens);
        if end > start && used + cost > budget_tokens {
            break;
        }
        used += cost;
        end += 1;
    }
    end
}

/// Ordered chunk ranges covering every turn.
pub(crate) fn plan_chunks(
    rendered: &[RenderedTurn],
    budget_tokens: u64,
) -> Vec<std::ops::Range<usize>> {
    let mut ranges = Vec::new();
    let mut start = 0;
    while start < rendered.len() {
        let end = chunk_end(rendered, start, budget_tokens);
        ranges.push(start..end);
        start = end;
    }
    ranges
}

/// Joins the turns of one chunk, truncating any turn larger than the budget.
pub(crate) fn chunk_text(
    rendered: &[RenderedTurn],
    range: std::ops::Range<usize>,
    budget_tokens: u64,
) -> String {
    let max_chars = (budget_tokens.max(1) as usize).saturating_mul(4);
    rendered[range]
        .iter()
        .map(|turn| shorten_middle(&turn.text, max_chars, "compaction/turn-truncated"))
        .collect::<Vec<_>>()
        .join("\n\n")
}

// ------------------------------------------------------------ prepared input

/// The summarizer-ready view of the part of the conversation being summarized.
pub(crate) struct PreparedInput {
    pub(crate) prior_summary: Option<String>,
    pub(crate) rendered: Vec<RenderedTurn>,
    pub(crate) ledger: Ledger,
    pub(crate) user_requests: Vec<String>,
    pub(crate) redactions: usize,
    pub(crate) masked_savings: u64,
}

/// Redacts, extracts the ledger from the unmasked text, masks old tool
/// output and renders. `turns` is the summarized part of the conversation.
pub(crate) fn prepare_input(turns: &[Turn], options: &CompactionOptions) -> PreparedInput {
    let mut prior_pieces: Vec<String> = turns
        .iter()
        .filter(|turn| turn.role == Role::EarlierSummary)
        .map(Turn::text)
        .filter(|text| !text.trim().is_empty())
        .collect();
    let mut conversation: Vec<Turn> = turns
        .iter()
        .filter(|turn| turn.role != Role::EarlierSummary)
        .cloned()
        .collect();

    let mut redactions = 0;
    if options.redact_secrets {
        redactions += redact_turns(&mut conversation);
        for piece in &mut prior_pieces {
            redactions += redact_in_place(piece);
        }
    }
    let ledger = Ledger::from_turns(&conversation);
    let user_requests = conversation
        .iter()
        .filter(|turn| turn.role == Role::User)
        .map(|turn| truncate_chars(turn.text().trim(), FALLBACK_REQUEST_CHARS))
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>();
    let skipped = user_requests.len().saturating_sub(FALLBACK_REQUEST_COUNT);
    let user_requests = user_requests.into_iter().skip(skipped).collect();

    let masked_savings = mask_tool_results(
        &mut conversation,
        options.protected_recent_tool_tokens,
        options.tool_result_chars,
    );
    let prior_summary = (!prior_pieces.is_empty()).then(|| prior_pieces.join("\n\n"));
    PreparedInput {
        prior_summary,
        rendered: render_turns(&conversation),
        ledger,
        user_requests,
        redactions,
        masked_savings,
    }
}

/// Maps each tool call id of `turns` to the position of the turn that holds
/// its result, for writers that must keep pairs adjacent.
pub(crate) fn result_lookup(turns: &[Turn]) -> HashMap<&str, &Part> {
    turns
        .iter()
        .flat_map(|turn| turn.parts.iter())
        .filter_map(|part| match part {
            Part::ToolResult { call_id, .. } => Some((call_id.as_str(), part)),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(text: &str) -> Turn {
        Turn::new(Role::User, vec![Part::Text(text.to_string())])
    }

    fn assistant(parts: Vec<Part>) -> Turn {
        Turn::new(Role::Assistant, parts)
    }

    fn call(id: &str) -> Part {
        Part::ToolCall {
            id: id.into(),
            name: "Bash".into(),
            arguments: json!({"command": "ls"}),
        }
    }

    fn results(entries: &[(&str, &str)]) -> Turn {
        Turn::new(
            Role::ToolResults,
            entries
                .iter()
                .map(|(id, text)| Part::ToolResult {
                    call_id: (*id).into(),
                    text: (*text).into(),
                    is_error: false,
                })
                .collect(),
        )
    }

    fn filler(chars: usize) -> String {
        "x".repeat(chars)
    }

    #[test]
    fn tail_is_capped_at_half_and_starts_at_a_user_turn() {
        let turns = vec![
            user(&filler(400)),
            assistant(vec![Part::Text(filler(400))]),
            user(&filler(400)),
            assistant(vec![Part::Text(filler(400))]),
        ];
        // Every turn is 100 tokens; a huge tail budget is capped to half (200).
        assert_eq!(select_tail(&turns, 10_000), 2);
        // A tiny budget still yields a valid (possibly empty) tail.
        assert_eq!(select_tail(&turns, 0), 4);
    }

    #[test]
    fn tail_never_splits_a_tool_pair() {
        let turns = vec![
            user(&filler(4000)),
            assistant(vec![Part::Text("run".into()), call("a")]),
            results(&[("a", &filler(400))]),
            assistant(vec![Part::Text(filler(40))]),
        ];
        // Budget fits only the last turn and the results; the split must move
        // back to the assistant turn holding the call.
        let split = select_tail(&turns, 150);
        assert_eq!(split, 1, "the call turn must travel with its result");
        let split_without_pair = select_tail(&turns[..2], 150);
        assert!(split_without_pair <= 2);
    }

    #[test]
    fn earlier_summary_is_never_part_of_the_tail() {
        let turns = vec![
            user("old"),
            Turn::new(Role::EarlierSummary, vec![Part::Text("summary".into())]),
            user("new"),
            assistant(vec![Part::Text("reply".into())]),
        ];
        let split = select_tail(&turns, 10_000);
        assert!(
            split >= 2,
            "split {split} would put the summary in the tail"
        );
    }

    #[test]
    fn masking_protects_the_newest_output_and_stubs_older_results() {
        let big = filler(8_000);
        let mut turns = vec![
            results(&[("a", &big)]),
            results(&[("b", &big)]),
            results(&[("c", "short")]),
        ];
        let saved = mask_tool_results(&mut turns, 2_100, 400);
        let text = |index: usize| match &turns[index].parts[0] {
            Part::ToolResult { text, .. } => text.clone(),
            _ => String::new(),
        };
        assert_eq!(text(2), "short");
        assert_eq!(text(1).len(), 8_000, "newest big result is protected");
        let masked = text(0);
        assert!(
            masked.len() < 600,
            "older result is stubbed: {}",
            masked.len()
        );
        assert!(masked.contains("7600 characters omitted"));
        assert!(saved > 1_800);
    }

    #[test]
    fn short_results_are_never_stubbed() {
        let mut turns = vec![results(&[("a", "tiny")]), results(&[("b", &filler(9_000))])];
        let saved = mask_tool_results(&mut turns, 0, 100);
        assert!((2_150..2_250).contains(&saved), "saved {saved}");
        match &turns[0].parts[0] {
            Part::ToolResult { text, .. } => assert_eq!(text, "tiny"),
            _ => unreachable!(),
        }
    }

    #[test]
    fn chunking_respects_turn_boundaries_and_covers_everything() {
        let rendered: Vec<RenderedTurn> = (0..7)
            .map(|index| RenderedTurn {
                text: format!("turn {index}"),
                tokens: 100,
            })
            .collect();
        let ranges = plan_chunks(&rendered, 250);
        assert_eq!(ranges, vec![0..2, 2..4, 4..6, 6..7]);
        let covered: usize = ranges.iter().map(|range| range.len()).sum();
        assert_eq!(covered, 7);
        // One huge turn gets a chunk of its own and is truncated.
        let huge = vec![RenderedTurn {
            text: filler(10_000),
            tokens: 2_500,
        }];
        assert_eq!(plan_chunks(&huge, 100), vec![0..1]);
        let text = chunk_text(&huge, 0..1, 100);
        assert!(text.len() < 700 && text.contains("omitted to fit the summarizer window"));
    }

    #[test]
    fn prepare_input_redacts_before_the_ledger_and_keeps_the_anchor() {
        let turns = vec![
            Turn::new(
                Role::EarlierSummary,
                vec![Part::Text("earlier work sk-abcdefghijklmnop1234".into())],
            ),
            user("please use token=abcdef123456 on https://example.com"),
            assistant(vec![Part::Text("ok".into()), call("a")]),
        ];
        let prepared = prepare_input(&turns, &CompactionOptions::default());
        assert_eq!(prepared.redactions, 2);
        assert_eq!(
            prepared.prior_summary.as_deref(),
            Some("earlier work [REDACTED]")
        );
        assert_eq!(prepared.rendered.len(), 2);
        assert!(prepared.rendered[0].text.contains("token=[REDACTED]"));
        assert_eq!(prepared.ledger.urls, ["https://example.com"]);
        assert_eq!(prepared.ledger.commands, ["ls"]);
        assert_eq!(prepared.user_requests.len(), 1);

        let options = CompactionOptions {
            redact_secrets: false,
            ..CompactionOptions::default()
        };
        let plain = prepare_input(&turns, &options);
        assert_eq!(plain.redactions, 0);
        assert!(plain.rendered[0].text.contains("token=abcdef123456"));
    }
}

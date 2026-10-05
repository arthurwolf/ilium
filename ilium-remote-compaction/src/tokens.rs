//! Token accounting: a cheap byte-based estimator for planning, and exact
//! usage read back from the transcript for the "before" figure and the
//! monitor.

use std::path::Path;

use serde_json::Value;

use crate::transcript_io::read_tail_values;
use crate::AgentKind;

/// How much of the end of a transcript the probes inspect.
const PROBE_WINDOW_BYTES: u64 = 8 * 1024 * 1024;

/// Bytes divided by four, rounded up: the heuristic Codex itself uses.
pub fn estimate_tokens(text: &str) -> u64 {
    (text.len() as u64).div_ceil(4)
}

/// Latest context usage stated by the transcript, with the context window
/// when the transcript states it.
///
/// Claude: the newest assistant usage (input + cache creation + cache read +
/// output); scanning stops at a compaction boundary because records re-emitted
/// after it carry no usage and older usage is stale. Codex: the newest
/// `token_count` event's `last_token_usage.total_tokens`.
pub fn latest_context_usage(
    agent: AgentKind,
    transcript_path: &Path,
) -> Option<(u64, Option<u64>)> {
    let values = read_tail_values(transcript_path, PROBE_WINDOW_BYTES).ok()?;
    match agent {
        AgentKind::Claude => claude_usage(&values),
        AgentKind::Codex => codex_usage(&values),
    }
}

fn claude_usage(values: &[Value]) -> Option<(u64, Option<u64>)> {
    for entry in values.iter().rev() {
        if entry.get("type").and_then(Value::as_str) == Some("system")
            && entry.get("subtype").and_then(Value::as_str) == Some("compact_boundary")
        {
            return None;
        }
        if entry.get("type").and_then(Value::as_str) != Some("assistant")
            || entry.get("isSidechain").and_then(Value::as_bool) == Some(true)
        {
            continue;
        }
        let Some(usage) = entry.pointer("/message/usage") else {
            continue;
        };
        let field = |name: &str| usage.get(name).and_then(Value::as_u64).unwrap_or(0);
        let total = field("input_tokens")
            + field("cache_creation_input_tokens")
            + field("cache_read_input_tokens")
            + field("output_tokens");
        if total > 0 {
            return Some((total, None));
        }
    }
    None
}

fn codex_usage(values: &[Value]) -> Option<(u64, Option<u64>)> {
    for entry in values.iter().rev() {
        let Some(payload) = entry.get("payload") else {
            continue;
        };
        if entry.get("type").and_then(Value::as_str) != Some("event_msg")
            || payload.get("type").and_then(Value::as_str) != Some("token_count")
        {
            continue;
        }
        let Some(info) = payload.get("info").filter(|info| info.is_object()) else {
            continue;
        };
        let total = info.pointer("/last_token_usage/total_tokens")?.as_u64()?;
        let window = info.get("model_context_window").and_then(Value::as_u64);
        return Some((total, window));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn estimator_is_bytes_over_four_rounded_up() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("abcde"), 2);
    }

    #[test]
    fn claude_usage_sums_all_four_fields_and_stops_at_a_boundary() {
        let usage = json!({"type":"assistant","message":{"usage":{
            "input_tokens":10,"cache_creation_input_tokens":20,
            "cache_read_input_tokens":30,"output_tokens":5}}});
        assert_eq!(claude_usage(std::slice::from_ref(&usage)), Some((65, None)));
        let boundary = json!({"type":"system","subtype":"compact_boundary"});
        let no_usage = json!({"type":"assistant","message":{"content":[]}});
        assert_eq!(claude_usage(&[usage, boundary, no_usage]), None);
    }

    #[test]
    fn codex_usage_reads_the_last_total_and_window() {
        let event = |total: u64| {
            json!({"type":"event_msg","payload":{"type":"token_count","info":{
                "last_token_usage":{"total_tokens":total},"model_context_window":258400}}})
        };
        assert_eq!(codex_usage(&[event(1), event(7)]), Some((7, Some(258_400))));
        let empty = json!({"type":"event_msg","payload":{"type":"token_count","info":null}});
        assert_eq!(codex_usage(&[event(3), empty]), Some((3, Some(258_400))));
    }
}

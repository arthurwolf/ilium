//! Cheap probe: is the transcript tail a clean pause point?

use std::collections::HashSet;
use std::path::Path;

use serde_json::Value;

use crate::transcript_io::read_tail_values;
use crate::AgentKind;

/// How much of the end of a transcript the probe inspects.
const PROBE_WINDOW_BYTES: u64 = 4 * 1024 * 1024;

/// True when the newest conversation record shows the agent finished its turn
/// and no tool call is waiting for a result. A transcript that cannot be read
/// is not at a pause point.
pub fn transcript_is_at_pause_point(agent: AgentKind, transcript_path: &Path) -> bool {
    let Ok(values) = read_tail_values(transcript_path, PROBE_WINDOW_BYTES) else {
        return false;
    };
    match agent {
        AgentKind::Claude => claude_at_pause_point(&values),
        AgentKind::Codex => codex_at_pause_point(&values),
    }
}

fn kind(value: &Value) -> &str {
    value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default()
}

fn content_blocks(value: &Value) -> &[Value] {
    value
        .pointer("/message/content")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
}

fn claude_at_pause_point(values: &[Value]) -> bool {
    let conversation: Vec<&Value> = values
        .iter()
        .filter(|value| {
            matches!(kind(value), "user" | "assistant")
                && value.get("isSidechain").and_then(Value::as_bool) != Some(true)
        })
        .collect();
    let Some(newest) = conversation.last() else {
        return false;
    };
    if kind(newest) != "assistant"
        || newest
            .pointer("/message/stop_reason")
            .and_then(Value::as_str)
            != Some("end_turn")
    {
        return false;
    }
    // Calls and results since the last human prompt.
    let mut called = HashSet::new();
    let mut answered = HashSet::new();
    for value in conversation.iter().rev() {
        let blocks = content_blocks(value);
        let has_result = blocks
            .iter()
            .any(|block| block.get("type").and_then(Value::as_str) == Some("tool_result"));
        if kind(value) == "user" && !has_result {
            break;
        }
        for block in blocks {
            match block.get("type").and_then(Value::as_str) {
                Some("tool_use") => called.extend(block.get("id").and_then(Value::as_str)),
                Some("tool_result") => {
                    answered.extend(block.get("tool_use_id").and_then(Value::as_str));
                }
                _ => {}
            }
        }
    }
    called.is_subset(&answered)
}

fn codex_at_pause_point(values: &[Value]) -> bool {
    let mut lifecycle_end = None;
    let mut called = HashSet::new();
    let mut answered = HashSet::new();
    for value in values.iter().rev() {
        let payload = value.get("payload").unwrap_or(&Value::Null);
        let payload_kind = payload
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        match (kind(value), payload_kind) {
            ("event_msg", "task_started" | "turn_started") => {
                if lifecycle_end.is_none() {
                    return false;
                }
                break;
            }
            ("event_msg", "task_complete" | "turn_complete" | "turn_aborted") => {
                lifecycle_end.get_or_insert(true);
            }
            ("response_item", "function_call" | "custom_tool_call" | "local_shell_call") => {
                let id = payload
                    .get("call_id")
                    .or_else(|| payload.get("id"))
                    .and_then(Value::as_str);
                called.extend(id);
            }
            (
                "response_item",
                "function_call_output" | "custom_tool_call_output" | "local_shell_call_output",
            ) => answered.extend(payload.get("call_id").and_then(Value::as_str)),
            _ => {}
        }
    }
    lifecycle_end == Some(true) && called.is_subset(&answered)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn assistant(stop: &str, blocks: Value) -> Value {
        json!({"type":"assistant","message":{"stop_reason":stop,"content":blocks}})
    }

    fn user_text(text: &str) -> Value {
        json!({"type":"user","message":{"content":text}})
    }

    #[test]
    fn claude_pause_requires_an_ended_turn_with_no_open_tool_use() {
        let ended = [
            user_text("hi"),
            assistant("end_turn", json!([{"type":"text","text":"done"}])),
        ];
        assert!(claude_at_pause_point(&ended));

        let mid_turn = [
            user_text("hi"),
            assistant("tool_use", json!([{"type":"tool_use","id":"a"}])),
        ];
        assert!(!claude_at_pause_point(&mid_turn));

        let new_prompt_pending = [
            assistant("end_turn", json!([{"type":"text","text":"done"}])),
            user_text("another"),
        ];
        assert!(!claude_at_pause_point(&new_prompt_pending));

        let unresolved = [
            user_text("hi"),
            assistant("tool_use", json!([{"type":"tool_use","id":"a"}])),
            assistant("end_turn", json!([{"type":"text","text":"done"}])),
        ];
        assert!(!claude_at_pause_point(&unresolved));

        let resolved = [
            user_text("hi"),
            assistant("tool_use", json!([{"type":"tool_use","id":"a"}])),
            json!({"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"a"}]}}),
            assistant("end_turn", json!([{"type":"text","text":"done"}])),
        ];
        assert!(claude_at_pause_point(&resolved));
        assert!(!claude_at_pause_point(&[]));
    }

    fn event(kind: &str) -> Value {
        json!({"type":"event_msg","payload":{"type":kind}})
    }

    fn item(kind: &str, id: &str) -> Value {
        json!({"type":"response_item","payload":{"type":kind,"call_id":id}})
    }

    #[test]
    fn codex_pause_requires_a_finished_turn_with_no_open_call() {
        let finished = [
            event("task_started"),
            item("function_call", "a"),
            item("function_call_output", "a"),
            event("task_complete"),
        ];
        assert!(codex_at_pause_point(&finished));
        let running = [event("task_started"), item("function_call", "a")];
        assert!(!codex_at_pause_point(&running));
        let aborted_open = [
            event("task_started"),
            item("function_call", "a"),
            event("turn_aborted"),
        ];
        assert!(!codex_at_pause_point(&aborted_open));
        let aborted = [event("task_started"), event("turn_aborted")];
        assert!(codex_at_pause_point(&aborted));
        let second_turn_running = [
            event("task_started"),
            event("task_complete"),
            event("task_started"),
        ];
        assert!(!codex_at_pause_point(&second_turn_running));
        assert!(!codex_at_pause_point(&[]));
    }
}

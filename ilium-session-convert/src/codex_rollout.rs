//! Reads a Codex rollout (`rollout-*-<id>.jsonl`) into the handful of
//! conversation items worth carrying to Claude Code, counting everything else
//! as dropped.
//!
//! Two rollout generations are handled. Older files use
//! `function_call shell_command` and `custom_tool_call apply_patch`; current
//! ones use `custom_tool_call exec` (a JavaScript snippet) plus structured
//! `custom_tool_call_output` arrays. Both carry `response_item` payloads, which
//! are the single source of truth: `event_msg` records duplicate them (the
//! user message in particular) and are never converted.

use serde_json::Value;
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};
use std::path::Path;

use crate::error::ConvertError;

/// How many lines are read between progress and cancellation checks.
const PROGRESS_INTERVAL_LINES: usize = 500;

/// User-role text blocks that are harness context, not something the person
/// typed. Matched against the block text after leading whitespace.
const HARNESS_BLOCK_PREFIXES: [&str; 6] = [
    "# AGENTS.md instructions",
    "<environment_context>",
    "<user_instructions>",
    "<permissions instructions>",
    "<permissions_instructions>",
    "<codex_internal_context",
];

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct RolloutMeta {
    pub(crate) session_id: Option<String>,
    pub(crate) cwd: Option<String>,
    pub(crate) git_branch: Option<String>,
}

/// How a tool call carried its arguments in the rollout.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ToolArguments {
    /// `function_call.arguments`: a JSON document in a string.
    JsonText(String),
    /// `custom_tool_call.input`: free-form text (a patch, a script).
    FreeText(String),
    /// Already structured (synthesised from `local_shell_call`).
    Structured(Value),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum SourceItem {
    UserText {
        text: String,
        timestamp: Option<String>,
    },
    AssistantText {
        text: String,
        timestamp: Option<String>,
    },
    ToolCall {
        call_id: String,
        name: String,
        arguments: ToolArguments,
        timestamp: Option<String>,
    },
    ToolOutput {
        call_id: String,
        text: String,
        is_error: bool,
    },
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ParsedRollout {
    pub(crate) meta: RolloutMeta,
    pub(crate) items: Vec<SourceItem>,
    /// Lines that held JSON (everything except blank/unparseable lines).
    pub(crate) json_lines: usize,
    pub(crate) malformed_lines: usize,
    /// Lines parsed but intentionally not converted, by coarse kind.
    pub(crate) dropped_by_kind: BTreeMap<String, usize>,
}

impl ParsedRollout {
    pub(crate) fn dropped_total(&self) -> usize {
        self.dropped_by_kind.values().sum::<usize>() + self.malformed_lines
    }

    fn drop_kind(&mut self, kind: impl Into<String>) {
        *self.dropped_by_kind.entry(kind.into()).or_insert(0) += 1;
    }
}

/// Parses the rollout at `path`. `on_progress` receives the fraction of bytes
/// read every few hundred lines and returns `false` to cancel.
pub(crate) fn parse_rollout(
    path: &Path,
    on_progress: &mut dyn FnMut(f32) -> bool,
) -> Result<ParsedRollout, ConvertError> {
    let file = std::fs::File::open(path).map_err(|error| ConvertError::SourceUnreadable {
        path: path.to_path_buf(),
        error,
    })?;
    let total_bytes = file.metadata().map(|metadata| metadata.len()).unwrap_or(0);
    let mut bytes_read = 0u64;
    let mut reader = BufReader::new(file);
    let mut parsed = ParsedRollout::default();
    let mut buffer = Vec::new();
    let mut line_count = 0usize;
    loop {
        buffer.clear();
        let read = reader.read_until(b'\n', &mut buffer).map_err(|error| {
            ConvertError::SourceUnreadable {
                path: path.to_path_buf(),
                error,
            }
        })?;
        if read == 0 {
            break;
        }
        line_count += 1;
        bytes_read += read as u64;
        if line_count.is_multiple_of(PROGRESS_INTERVAL_LINES) {
            let fraction = if total_bytes == 0 {
                0.0
            } else {
                (bytes_read as f64 / total_bytes as f64) as f32
            };
            if !on_progress(fraction) {
                return Err(ConvertError::Cancelled);
            }
        }
        let text = String::from_utf8_lossy(&buffer);
        let trimmed = text.trim();
        if trimmed.is_empty() {
            continue;
        }
        match serde_json::from_str::<Value>(trimmed) {
            Ok(entry) => {
                parsed.json_lines += 1;
                absorb_entry(&mut parsed, &entry);
            }
            Err(_) => parsed.malformed_lines += 1,
        }
    }
    Ok(parsed)
}

fn absorb_entry(parsed: &mut ParsedRollout, entry: &Value) {
    let kind = entry.get("type").and_then(Value::as_str).unwrap_or("");
    let payload = entry.get("payload").unwrap_or(&Value::Null);
    match kind {
        "session_meta" => absorb_session_meta(parsed, payload),
        "response_item" => absorb_response_item(parsed, entry, payload),
        // `event_msg`, `turn_context`, `compacted`, `world_state`,
        // `token_usage_record`, ... carry no conversation of their own.
        other => parsed.drop_kind(other),
    }
}

fn absorb_session_meta(parsed: &mut ParsedRollout, payload: &Value) {
    // Only the first `session_meta` owns the rollout's identity.
    if parsed.meta.session_id.is_some() {
        parsed.drop_kind("session_meta");
        return;
    }
    let text = |value: Option<&Value>| value.and_then(Value::as_str).map(str::to_string);
    parsed.meta.session_id = text(payload.get("id"));
    parsed.meta.cwd = text(payload.get("cwd"));
    parsed.meta.git_branch = text(payload.pointer("/git/branch")).filter(|b| !b.is_empty());
}

fn absorb_response_item(parsed: &mut ParsedRollout, entry: &Value, payload: &Value) {
    let timestamp = entry
        .get("timestamp")
        .and_then(Value::as_str)
        .map(str::to_string);
    let payload_type = payload.get("type").and_then(Value::as_str).unwrap_or("");
    match payload_type {
        "message" => absorb_message(parsed, payload, timestamp),
        "function_call" | "custom_tool_call" => absorb_tool_call(parsed, payload, timestamp),
        "local_shell_call" => absorb_local_shell_call(parsed, payload, timestamp),
        "function_call_output" | "custom_tool_call_output" | "local_shell_call_output" => {
            absorb_tool_output(parsed, payload)
        }
        // reasoning (encrypted), agent_message, ghost_snapshot, web_search_call, ...
        other => parsed.drop_kind(format!("response_item:{other}")),
    }
}

fn absorb_message(parsed: &mut ParsedRollout, payload: &Value, timestamp: Option<String>) {
    let role = payload.get("role").and_then(Value::as_str).unwrap_or("");
    let is_user = role == "user";
    if !is_user && role != "assistant" {
        // developer/system messages are harness instructions.
        parsed.drop_kind(format!("message:{role}"));
        return;
    }
    let blocks = payload
        .get("content")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let mut kept = Vec::new();
    for block in blocks {
        let block_type = block.get("type").and_then(Value::as_str).unwrap_or("");
        let is_text = matches!(block_type, "input_text" | "output_text" | "text");
        let Some(text) = block
            .get("text")
            .and_then(Value::as_str)
            .filter(|_| is_text)
        else {
            // encrypted_content, input_image, ...
            continue;
        };
        if is_user && is_harness_block(text) {
            continue;
        }
        if text.trim().is_empty() {
            continue;
        }
        kept.push(text.to_string());
    }
    if kept.is_empty() {
        parsed.drop_kind(if is_user {
            "message:user (harness context)"
        } else {
            "message:assistant (no text)"
        });
        return;
    }
    let text = kept.join("\n\n");
    parsed.items.push(if is_user {
        SourceItem::UserText { text, timestamp }
    } else {
        SourceItem::AssistantText { text, timestamp }
    });
}

fn is_harness_block(text: &str) -> bool {
    let start = text.trim_start();
    HARNESS_BLOCK_PREFIXES
        .iter()
        .any(|prefix| start.starts_with(prefix))
}

fn absorb_tool_call(parsed: &mut ParsedRollout, payload: &Value, timestamp: Option<String>) {
    let (Some(call_id), Some(name)) = (
        payload.get("call_id").and_then(Value::as_str),
        payload.get("name").and_then(Value::as_str),
    ) else {
        parsed.drop_kind("tool_call (missing call_id or name)");
        return;
    };
    let arguments = match (
        payload.get("arguments").and_then(Value::as_str),
        payload.get("input").and_then(Value::as_str),
    ) {
        (Some(json_text), _) => ToolArguments::JsonText(json_text.to_string()),
        (None, Some(free_text)) => ToolArguments::FreeText(free_text.to_string()),
        (None, None) => ToolArguments::JsonText(String::new()),
    };
    parsed.items.push(SourceItem::ToolCall {
        call_id: call_id.to_string(),
        name: name.to_string(),
        arguments,
        timestamp,
    });
}

/// `local_shell_call` (an older built-in shell tool) keeps its argv under
/// `action.command`; it is presented as an ordinary `shell` call.
fn absorb_local_shell_call(parsed: &mut ParsedRollout, payload: &Value, timestamp: Option<String>) {
    let call_id = payload
        .get("call_id")
        .or_else(|| payload.get("id"))
        .and_then(Value::as_str);
    let command = payload.pointer("/action/command");
    let (Some(call_id), Some(command)) = (call_id, command) else {
        parsed.drop_kind("local_shell_call (missing call_id or command)");
        return;
    };
    let mut arguments = serde_json::Map::new();
    arguments.insert("command".to_string(), command.clone());
    if let Some(workdir) = payload.pointer("/action/working_directory") {
        arguments.insert("workdir".to_string(), workdir.clone());
    }
    parsed.items.push(SourceItem::ToolCall {
        call_id: call_id.to_string(),
        name: "shell".to_string(),
        arguments: ToolArguments::Structured(Value::Object(arguments)),
        timestamp,
    });
}

fn absorb_tool_output(parsed: &mut ParsedRollout, payload: &Value) {
    let Some(call_id) = payload.get("call_id").and_then(Value::as_str) else {
        parsed.drop_kind("tool_output (missing call_id)");
        return;
    };
    let output = payload.get("output").unwrap_or(&Value::Null);
    let is_error = output
        .get("success")
        .and_then(Value::as_bool)
        .is_some_and(|success| !success);
    parsed.items.push(SourceItem::ToolOutput {
        call_id: call_id.to_string(),
        text: flatten_output(output),
        is_error,
    });
}

/// Turns the many `output` shapes into plain text: a string, an array of
/// `{type, text}` blocks, or a `{content|output: string}` object.
fn flatten_output(output: &Value) -> String {
    match output {
        Value::Null => String::new(),
        Value::String(text) => text.clone(),
        Value::Array(blocks) => {
            let mut joined = String::new();
            for block in blocks {
                let piece = match block {
                    Value::String(text) => Some(text.as_str()),
                    other => other.get("text").and_then(Value::as_str),
                };
                let Some(piece) = piece else {
                    continue;
                };
                if !joined.is_empty() && !joined.ends_with('\n') {
                    joined.push('\n');
                }
                joined.push_str(piece);
            }
            joined
        }
        Value::Object(map) => ["content", "output"]
            .iter()
            .find_map(|key| map.get(*key).and_then(Value::as_str))
            .map(str::to_string)
            .unwrap_or_else(|| output.to_string()),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn parse_lines(lines: &[Value]) -> ParsedRollout {
        let mut parsed = ParsedRollout::default();
        for line in lines {
            absorb_entry(&mut parsed, line);
        }
        parsed
    }

    #[test]
    fn harness_blocks_are_dropped_per_block_not_per_message() {
        let parsed = parse_lines(&[json!({
            "type": "response_item",
            "payload": {"type": "message", "role": "user", "content": [
                {"type": "input_text", "text": "# AGENTS.md instructions for /x\n..."},
                {"type": "input_text", "text": "<environment_context>\n</environment_context>"},
                {"type": "input_text", "text": "real prompt"}
            ]}
        })]);
        assert_eq!(
            parsed.items,
            vec![SourceItem::UserText {
                text: "real prompt".to_string(),
                timestamp: None
            }]
        );
    }

    #[test]
    fn message_with_only_harness_blocks_is_dropped() {
        let parsed = parse_lines(&[json!({
            "type": "response_item",
            "payload": {"type": "message", "role": "user", "content": [
                {"type": "input_text", "text": "<codex_internal_context source=\"goal\">x"}
            ]}
        })]);
        assert!(parsed.items.is_empty());
        assert_eq!(parsed.dropped_total(), 1);
    }

    #[test]
    fn output_arrays_are_joined_with_line_breaks() {
        let output = json!([
            {"type": "input_text", "text": "Script completed\n"},
            {"type": "input_text", "text": "{\"exit_code\":0}"}
        ]);
        assert_eq!(
            flatten_output(&output),
            "Script completed\n{\"exit_code\":0}"
        );
    }

    #[test]
    fn local_shell_call_becomes_a_shell_call() {
        let parsed = parse_lines(&[json!({
            "type": "response_item",
            "payload": {"type": "local_shell_call", "call_id": "c1",
                "action": {"type": "exec", "command": ["ls", "-la"]}}
        })]);
        assert!(matches!(
            parsed.items.as_slice(),
            [SourceItem::ToolCall { name, .. }] if name == "shell"
        ));
    }
}

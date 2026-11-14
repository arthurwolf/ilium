//! Plain-text entries of an agent transcript: what the person asked, what the agent answered
//! and what tools returned. Claude Code and Codex record different JSONL shapes; this reads
//! both into one ordered list. It keeps no windows and does no clipping, because the evidence
//! digest picks and bounds what it needs from the whole list.

use serde_json::Value;

/// Which transcript format a session file uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentKind {
    Claude,
    Codex,
    /// An agent without a readable transcript format.
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub kind: EntryKind,
    pub content: String,
}

/// The entries of `lines` in file order. Lines that are not JSON are skipped.
pub fn extract_entries<'a>(class: AgentKind, lines: impl Iterator<Item = &'a str>) -> Vec<Entry> {
    let records = lines.filter_map(|line| serde_json::from_str::<Value>(line).ok());
    match class {
        AgentKind::Claude => claude_entries(records),
        AgentKind::Codex => codex_entries(records),
        AgentKind::Other => Vec::new(),
    }
}

fn claude_entries(records: impl Iterator<Item = Value>) -> Vec<Entry> {
    let mut entries = Vec::new();
    for record in records {
        if record.get("isSidechain").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        let Some(message) = record.get("message") else {
            continue;
        };
        match record.get("type").and_then(Value::as_str) {
            Some("user") => match message.get("content") {
                Some(Value::String(_)) => {
                    if let Some(content) = genuine_request_text(AgentKind::Claude, &record) {
                        entries.push(Entry {
                            kind: EntryKind::User,
                            content,
                        });
                    }
                }
                Some(Value::Array(items)) => {
                    for item in items {
                        if item.get("type").and_then(Value::as_str) != Some("tool_result") {
                            continue;
                        }
                        if let Some(content) = textual_value(item.get("content")) {
                            entries.push(Entry {
                                kind: EntryKind::Tool,
                                content,
                            });
                        }
                    }
                }
                Some(_) | None => {}
            },
            Some("assistant") => {
                if let Some(content) = assistant_content(message.get("content")) {
                    entries.push(Entry {
                        kind: EntryKind::Assistant,
                        content,
                    });
                }
            }
            Some(_) | None => {}
        }
    }
    entries
}

fn assistant_content(content: Option<&Value>) -> Option<String> {
    match content? {
        Value::String(text) => non_empty(text),
        Value::Array(items) => {
            let text = items
                .iter()
                .filter(|item| item.get("type").and_then(Value::as_str) == Some("text"))
                .filter_map(|item| item.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n");
            non_empty(&text)
        }
        _ => None,
    }
}

/// Codex records user turns twice: as `event_msg` events and as `response_item` messages. The
/// response items are the complete stream, so they win when any exist.
fn codex_entries(records: impl Iterator<Item = Value>) -> Vec<Entry> {
    // Each record is placed by its position so the two user streams merge back in order.
    let mut others: Vec<(usize, Entry)> = Vec::new();
    let mut event_users: Vec<(usize, Entry)> = Vec::new();
    let mut response_users: Vec<(usize, Entry)> = Vec::new();
    for (position, record) in records.enumerate() {
        let Some(payload) = record.get("payload") else {
            continue;
        };
        match record.get("type").and_then(Value::as_str) {
            Some("event_msg") => match payload.get("type").and_then(Value::as_str) {
                Some("user_message") => {
                    if let Some(message) = genuine_request_text(AgentKind::Codex, &record) {
                        event_users.push((position, user_entry(&message)));
                    }
                }
                Some("agent_message") => {
                    if let Some(message) = payload.get("message").and_then(Value::as_str) {
                        others.push((
                            position,
                            Entry {
                                kind: EntryKind::Assistant,
                                content: message.to_string(),
                            },
                        ));
                    }
                }
                Some(_) | None => {}
            },
            Some("response_item") => {
                if payload.get("type").and_then(Value::as_str) == Some("message")
                    && payload.get("role").and_then(Value::as_str) == Some("user")
                {
                    if let Some(message) = genuine_request_text(AgentKind::Codex, &record) {
                        response_users.push((position, user_entry(&message)));
                    }
                    continue;
                }
                if matches!(
                    payload.get("type").and_then(Value::as_str),
                    Some("custom_tool_call_output" | "function_call_output")
                ) {
                    if let Some(output) = textual_value(payload.get("output")) {
                        others.push((
                            position,
                            Entry {
                                kind: EntryKind::Tool,
                                content: output,
                            },
                        ));
                    }
                }
            }
            Some(_) | None => {}
        }
    }
    let users = if response_users.is_empty() {
        event_users
    } else {
        response_users
    };
    let mut merged: Vec<(usize, Entry)> = others.into_iter().chain(users).collect();
    merged.sort_by_key(|(position, _)| *position);
    merged.into_iter().map(|(_, entry)| entry).collect()
}

fn user_entry(message: &str) -> Entry {
    Entry {
        kind: EntryKind::User,
        content: message.trim().to_string(),
    }
}

/// Extracts text from the string, array and block shapes used by Claude and Codex tool results.
/// Image blocks have no textual field and are omitted.
pub fn textual_value(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(text) => non_empty(text),
        Value::Array(items) => {
            let text = items
                .iter()
                .filter_map(|item| textual_value(Some(item)))
                .collect::<Vec<_>>()
                .join("\n");
            non_empty(&text)
        }
        Value::Bool(value) => Some(value.to_string()),
        Value::Number(value) => Some(value.to_string()),
        Value::Object(object) => object
            .get("text")
            .and_then(Value::as_str)
            .and_then(non_empty)
            .or_else(|| textual_value(object.get("content"))),
        Value::Null => None,
    }
}

fn non_empty(text: &str) -> Option<String> {
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

/// Text the person typed as a task, or `None` for harness records, injected context,
/// sidechains, compaction summaries and slash-command bookkeeping.
pub fn genuine_request_text(class: AgentKind, entry: &Value) -> Option<String> {
    match class {
        AgentKind::Claude => {
            if entry.get("type")?.as_str()? != "user"
                || entry.get("isSidechain").and_then(Value::as_bool) == Some(true)
                || entry.get("isMeta").and_then(Value::as_bool) == Some(true)
                || entry.get("isCompactSummary").and_then(Value::as_bool) == Some(true)
                || entry.get("promptSource").and_then(Value::as_str) == Some("system")
            {
                return None;
            }
            let origin = entry
                .get("origin")
                .and_then(|value| value.get("kind"))
                .and_then(Value::as_str);
            if !matches!(origin, None | Some("human")) {
                return None;
            }
            let text = entry.get("message")?.get("content")?.as_str()?;
            (!text.trim().is_empty()
                && !is_claude_injected_message(text)
                && !is_non_task_control_text(text))
            .then(|| text.to_owned())
        }
        AgentKind::Codex => {
            let payload = entry.get("payload")?;
            let text = match entry.get("type")?.as_str()? {
                "event_msg" if payload.get("type")?.as_str()? == "user_message" => {
                    payload.get("message")?.as_str()?.to_owned()
                }
                "response_item"
                    if payload.get("type")?.as_str()? == "message"
                        && payload.get("role")?.as_str()? == "user" =>
                {
                    codex_user_content(payload.get("content")?)?
                }
                _ => return None,
            };
            (!text.trim().is_empty()
                && !is_codex_injected_message(&text)
                && !is_non_task_control_text(&text))
            .then_some(text)
        }
        AgentKind::Other => None,
    }
}

fn codex_user_content(content: &Value) -> Option<String> {
    match content {
        Value::String(text) => Some(text.to_owned()),
        Value::Array(blocks) => {
            let text = blocks
                .iter()
                .filter(|block| {
                    matches!(
                        block.get("type").and_then(Value::as_str),
                        Some("input_text" | "text")
                    )
                })
                .filter_map(|block| block.get("text").and_then(Value::as_str))
                .filter(|text| !is_codex_injected_message(text))
                .collect::<Vec<_>>()
                .join("\n");
            (!text.trim().is_empty()).then_some(text)
        }
        _ => None,
    }
}

fn is_codex_injected_message(message: &str) -> bool {
    let text = message.trim_start();
    text.starts_with("# AGENTS.md instructions for ")
        || text.starts_with("<environment_context>")
        || text.starts_with("<codex_internal_context ")
        || text.starts_with("<task-notification>")
        || text.starts_with("Ilium progress monitor ")
}

fn is_claude_injected_message(message: &str) -> bool {
    let text = message.trim_start();
    is_codex_injected_message(message)
        || text.starts_with("<local-command-")
        || text.starts_with("<system-reminder>")
        || text.starts_with(
            "Caveat: The messages below were generated by the user while running local commands",
        )
}

/// Slash bookkeeping is not a task. A `/goal` with an objective is an authored task, however.
fn is_non_task_control_text(message: &str) -> bool {
    let text = message.trim();
    if let Some(rest) = text.strip_prefix("<command-name>") {
        let Some((command, tail)) = rest.split_once("</command-name>") else {
            return true;
        };
        if command != "/goal" {
            return true;
        }
        let Some(arguments) = tail
            .split("<command-args>")
            .nth(1)
            .and_then(|value| value.split_once("</command-args>"))
            .map(|(value, _)| value.trim())
        else {
            return true;
        };
        return is_goal_bookkeeping(arguments);
    }

    let mut words = text.split_whitespace();
    let Some(command) = words.next() else {
        return true;
    };
    if !command.starts_with('/') {
        return false;
    }
    if command == "/goal" {
        return is_goal_bookkeeping(words.next().unwrap_or_default());
    }
    true
}

fn is_goal_bookkeeping(first_argument: &str) -> bool {
    first_argument.is_empty()
        || matches!(
            first_argument,
            "clear" | "pause" | "resume" | "status" | "stop"
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_extracts_user_assistant_and_tool_entries_in_order() {
        let lines = [
            r#"{"type":"user","message":{"content":"fix the parser"}}"#,
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"on it"},{"type":"tool_use","name":"x"}]}}"#,
            r#"{"type":"user","message":{"content":[{"type":"tool_result","content":"ok"}]}}"#,
            r#"{"type":"user","isSidechain":true,"message":{"content":"sidechain"}}"#,
            "not json",
        ];
        let entries = extract_entries(AgentKind::Claude, lines.into_iter());
        let kinds: Vec<EntryKind> = entries.iter().map(|entry| entry.kind).collect();
        assert_eq!(
            kinds,
            vec![EntryKind::User, EntryKind::Assistant, EntryKind::Tool]
        );
        assert_eq!(entries[1].content, "on it");
    }

    #[test]
    fn codex_prefers_response_item_user_turns_and_skips_injected_context() {
        let lines = [
            r#"{"type":"event_msg","payload":{"type":"user_message","message":"event only"}}"#,
            r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"<environment_context>x</environment_context>"}]}}"#,
            r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"real task"}]}}"#,
            r#"{"type":"event_msg","payload":{"type":"agent_message","message":"done"}}"#,
            r#"{"type":"response_item","payload":{"type":"function_call_output","output":"result"}}"#,
        ];
        let entries = extract_entries(AgentKind::Codex, lines.into_iter());
        assert_eq!(
            entries,
            vec![
                Entry {
                    kind: EntryKind::User,
                    content: "real task".into()
                },
                Entry {
                    kind: EntryKind::Assistant,
                    content: "done".into()
                },
                Entry {
                    kind: EntryKind::Tool,
                    content: "result".into()
                },
            ]
        );
    }

    #[test]
    fn harness_text_and_bookkeeping_are_not_requests() {
        for text in [
            "Ilium progress monitor 1 reports completion",
            "# AGENTS.md instructions for /tmp/project",
            "/clear",
            "<command-name>/goal</command-name><command-args>clear</command-args>",
        ] {
            let record = serde_json::json!({"type":"user","message":{"content":text}});
            assert_eq!(genuine_request_text(AgentKind::Claude, &record), None);
        }
        let goal = serde_json::json!({"type":"user","message":{"content":"<command-name>/goal</command-name><command-args>ship it</command-args>"}});
        assert!(genuine_request_text(AgentKind::Claude, &goal).is_some());
    }

    #[test]
    fn textual_value_handles_blocks_and_scalars() {
        let value = serde_json::json!([{"text": "a"}, "b", 3, {"content": "c"}]);
        assert_eq!(textual_value(Some(&value)).as_deref(), Some("a\nb\n3\nc"));
        assert_eq!(textual_value(Some(&serde_json::json!("  "))), None);
    }
}

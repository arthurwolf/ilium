//! Extracts a bounded, chronological LLM context from supported coding-agent
//! transcripts. The provider-specific JSONL shapes stay contained here;
//! `session_naming` only sees typed user, assistant, and tool-output entries.
//!
//! Each entry kind keeps its own recent window. A tool-heavy turn therefore
//! cannot evict every user request or assistant answer before the prompt is
//! rendered. Actual character clipping belongs to `crate::naming`, the shared
//! LLM-boundary module, so every dynamic session-title field follows one rule.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader};
use std::path::Path;

use ilium_core::AgentClass;
use serde::Serialize;
use serde_json::Value;

/// The most recent entries retained independently for each semantic kind.
/// Kept generous -- a retitle/restructure decision benefits from seeing
/// enough of the actual back-and-forth to tell what the session is really
/// about, not just its most recent couple of turns.
pub const RECENT_ENTRY_COUNT_PER_KIND: usize = 15;

/// One transcript item with an explicit role so the title model never has to
/// infer whether a JSONL fragment came from the user, the agent, or a tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptEntryKind {
    User,
    Assistant,
    Tool,
}

impl TranscriptEntryKind {
    pub const fn prompt_label(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::Tool => "tool",
        }
    }
}

/// One plain-text transcript entry in original chronological order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptEntry {
    pub kind: TranscriptEntryKind,
    pub content: String,
}

#[derive(Debug)]
struct SequencedEntry {
    sequence: u64,
    entry: TranscriptEntry,
}

/// Three bounded queues preserve recent context from every role while the
/// transcript is streamed. Only the final merge allocates the combined list.
#[derive(Debug, Default)]
struct RecentEntries {
    next_sequence: u64,
    user: VecDeque<SequencedEntry>,
    assistant: VecDeque<SequencedEntry>,
    tool: VecDeque<SequencedEntry>,
}

impl RecentEntries {
    fn push(&mut self, kind: TranscriptEntryKind, content: impl Into<String>) {
        let content = content.into();
        let content = content.trim();
        if content.is_empty() {
            return;
        }

        let queue = match kind {
            TranscriptEntryKind::User => &mut self.user,
            TranscriptEntryKind::Assistant => &mut self.assistant,
            TranscriptEntryKind::Tool => &mut self.tool,
        };
        if queue.len() == RECENT_ENTRY_COUNT_PER_KIND {
            queue.pop_front();
        }
        queue.push_back(SequencedEntry {
            sequence: self.next_sequence,
            entry: TranscriptEntry {
                kind,
                content: content.to_string(),
            },
        });
        self.next_sequence = self.next_sequence.wrapping_add(1);
    }

    fn finish(self) -> Vec<TranscriptEntry> {
        let mut entries: Vec<SequencedEntry> = self
            .user
            .into_iter()
            .chain(self.assistant)
            .chain(self.tool)
            .collect();
        entries.sort_unstable_by_key(|entry| entry.sequence);
        entries.into_iter().map(|entry| entry.entry).collect()
    }
}

/// Reads one project-verified transcript and returns recent user messages,
/// assistant answers, and textual tool outputs in chronological order.
/// Unknown agents have no verified transcript format and therefore return an
/// empty list without touching the filesystem.
pub fn recent_transcript_entries(
    class: &AgentClass,
    transcript_path: &Path,
) -> anyhow::Result<Vec<TranscriptEntry>> {
    if matches!(class, AgentClass::Other(_)) {
        return Ok(Vec::new());
    }
    if matches!(class, AgentClass::Antigravity) {
        return antigravity_recent_entries(transcript_path);
    }

    let file = std::fs::File::open(transcript_path)?;
    // A malformed UTF-8/JSON line must not hide every later valid transcript
    // entry. `read_line` consumes the bad bytes before reporting the error.
    #[allow(clippy::lines_filter_map_ok)]
    let lines = BufReader::new(file).lines().filter_map(Result::ok);
    Ok(extract_transcript_entries(class, lines))
}

/// Compatibility-shaped projection used by project restructuring, which only
/// asks for user-authored intent. Session retitling consumes the full typed
/// stream through `recent_transcript_entries`.
pub fn recent_user_prompts(
    class: &AgentClass,
    transcript_path: &Path,
) -> anyhow::Result<Vec<String>> {
    Ok(recent_transcript_entries(class, transcript_path)?
        .into_iter()
        .filter(|entry| entry.kind == TranscriptEntryKind::User)
        .map(|entry| entry.content)
        .collect())
}

fn extract_transcript_entries(
    class: &AgentClass,
    lines: impl Iterator<Item = String>,
) -> Vec<TranscriptEntry> {
    match class {
        AgentClass::Claude => claude_entries(lines),
        AgentClass::Codex => codex_entries(lines),
        AgentClass::Antigravity | AgentClass::Other(_) => Vec::new(),
    }
}

/// Claude stores genuine user prompts as string content, assistant replies as
/// `text` blocks, and tool results as array-valued user content. Thinking and
/// tool-use request blocks are deliberately excluded: they are neither an
/// answer shown to the user nor tool output.
fn claude_entries(lines: impl Iterator<Item = String>) -> Vec<TranscriptEntry> {
    let mut recent = RecentEntries::default();
    for entry in lines.filter_map(|line| serde_json::from_str::<Value>(&line).ok()) {
        if entry
            .get("isSidechain")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            continue;
        }
        let Some(message) = entry.get("message") else {
            continue;
        };
        match entry.get("type").and_then(Value::as_str) {
            Some("user") => match message.get("content") {
                Some(Value::String(content)) => {
                    recent.push(TranscriptEntryKind::User, content.clone());
                }
                Some(Value::Array(items)) => {
                    for item in items {
                        if item.get("type").and_then(Value::as_str) != Some("tool_result") {
                            continue;
                        }
                        if let Some(content) = textual_value(item.get("content")) {
                            recent.push(TranscriptEntryKind::Tool, content);
                        }
                    }
                }
                Some(_) | None => {}
            },
            Some("assistant") => {
                if let Some(content) = assistant_content(message.get("content")) {
                    recent.push(TranscriptEntryKind::Assistant, content);
                }
            }
            Some(_) | None => {}
        }
    }
    recent.finish()
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

/// Codex emits clean user and assistant text as `event_msg` records. Tool
/// results live in `response_item` records; injected user/developer messages,
/// reasoning, and tool-call requests are excluded.
fn codex_entries(lines: impl Iterator<Item = String>) -> Vec<TranscriptEntry> {
    let mut recent = RecentEntries::default();
    for entry in lines.filter_map(|line| serde_json::from_str::<Value>(&line).ok()) {
        match entry.get("type").and_then(Value::as_str) {
            Some("event_msg") => {
                let Some(payload) = entry.get("payload") else {
                    continue;
                };
                match payload.get("type").and_then(Value::as_str) {
                    Some("user_message") => {
                        if let Some(message) = payload.get("message").and_then(Value::as_str) {
                            recent.push(TranscriptEntryKind::User, message.to_string());
                        }
                    }
                    Some("thread_goal_updated") => {
                        if let Some(objective) = payload
                            .get("goal")
                            .and_then(|goal| goal.get("objective"))
                            .and_then(Value::as_str)
                        {
                            recent.push(TranscriptEntryKind::User, objective.to_string());
                        }
                    }
                    Some("agent_message") => {
                        if let Some(message) = payload.get("message").and_then(Value::as_str) {
                            recent.push(TranscriptEntryKind::Assistant, message.to_string());
                        }
                    }
                    Some(_) | None => {}
                }
            }
            Some("response_item") => {
                let Some(payload) = entry.get("payload") else {
                    continue;
                };
                if !matches!(
                    payload.get("type").and_then(Value::as_str),
                    Some("custom_tool_call_output" | "function_call_output")
                ) {
                    continue;
                }
                if let Some(output) = textual_value(payload.get("output")) {
                    recent.push(TranscriptEntryKind::Tool, output);
                }
            }
            Some(_) | None => {}
        }
    }
    recent.finish()
}

/// Extracts text from the string/array/block shapes used by Claude and Codex
/// tool results. Image/base64 blocks have no textual field and are omitted.
fn textual_value(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(text) => non_empty(text),
        Value::Array(items) => {
            let text = items
                .iter()
                .filter_map(|item| {
                    item.get("text")
                        .and_then(Value::as_str)
                        .and_then(non_empty)
                        .or_else(|| textual_value(item.get("content")))
                })
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

/// Antigravity's conversation database is binary. Its adjacent history JSONL
/// currently exposes user-entered display text only; terminal-screen context
/// still supplies the latest visible agent answer to the retitle prompt.
fn antigravity_recent_entries(transcript_path: &Path) -> anyhow::Result<Vec<TranscriptEntry>> {
    let Some(session_id) = transcript_path.file_stem().and_then(|value| value.to_str()) else {
        return Ok(Vec::new());
    };
    let Some(antigravity_dir) = transcript_path.parent().and_then(Path::parent) else {
        return Ok(Vec::new());
    };
    // `history.jsonl` is an inferred sibling, not a path the caller already
    // verified (unlike `transcript_path` itself). A session whose history
    // file has not been written yet is "no context available", the same as
    // the two soft exits above -- not a hard failure worth aborting the
    // retitle attempt over.
    let file = match std::fs::File::open(antigravity_dir.join("history.jsonl")) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    #[allow(clippy::lines_filter_map_ok)]
    let lines = BufReader::new(file).lines().filter_map(Result::ok);
    let mut recent = RecentEntries::default();
    for entry in lines.filter_map(|line| serde_json::from_str::<Value>(&line).ok()) {
        if entry.get("conversationId").and_then(Value::as_str) != Some(session_id) {
            continue;
        }
        if let Some(display) = entry.get("display").and_then(Value::as_str) {
            recent.push(TranscriptEntryKind::User, display.to_string());
        }
    }
    Ok(recent.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines_of(text: &str) -> impl Iterator<Item = String> + '_ {
        text.lines().map(str::to_string)
    }

    #[test]
    fn claude_extracts_user_assistant_and_tool_entries_in_order() {
        let contents = [
            r#"{"type":"user","isSidechain":false,"message":{"content":"fix login"}}"#,
            r#"{"type":"assistant","message":{"content":[{"type":"thinking","thinking":"private"},{"type":"text","text":"I found the auth handler."},{"type":"tool_use","name":"Read"}]}}"#,
            r#"{"type":"user","message":{"content":[{"type":"tool_result","content":"handler source"}]}}"#,
            r#"{"type":"user","isSidechain":true,"message":{"content":"subagent prompt"}}"#,
        ]
        .join("\n");

        assert_eq!(
            claude_entries(lines_of(&contents)),
            vec![
                TranscriptEntry {
                    kind: TranscriptEntryKind::User,
                    content: "fix login".to_string(),
                },
                TranscriptEntry {
                    kind: TranscriptEntryKind::Assistant,
                    content: "I found the auth handler.".to_string(),
                },
                TranscriptEntry {
                    kind: TranscriptEntryKind::Tool,
                    content: "handler source".to_string(),
                },
            ]
        );
    }

    #[test]
    fn codex_extracts_clean_messages_and_textual_tool_outputs() {
        let contents = [
            r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"<environment_context>ignore</environment_context>"}]}}"#,
            r#"{"type":"event_msg","payload":{"type":"user_message","message":"write tests"}}"#,
            r#"{"type":"event_msg","payload":{"type":"agent_message","message":"The failing path is isolated."}}"#,
            r#"{"type":"response_item","payload":{"type":"custom_tool_call_output","output":[{"type":"input_text","text":"test output"},{"type":"input_text","text":"second block"}]}}"#,
        ]
        .join("\n");

        assert_eq!(
            codex_entries(lines_of(&contents)),
            vec![
                TranscriptEntry {
                    kind: TranscriptEntryKind::User,
                    content: "write tests".to_string(),
                },
                TranscriptEntry {
                    kind: TranscriptEntryKind::Assistant,
                    content: "The failing path is isolated.".to_string(),
                },
                TranscriptEntry {
                    kind: TranscriptEntryKind::Tool,
                    content: "test output\nsecond block".to_string(),
                },
            ]
        );
    }

    #[test]
    fn each_entry_kind_keeps_its_own_recent_window() {
        let mut lines = Vec::new();
        for index in 0..(RECENT_ENTRY_COUNT_PER_KIND + 2) {
            lines.push(format!(
                r#"{{"type":"event_msg","payload":{{"type":"user_message","message":"user {index}"}}}}"#
            ));
            lines.push(format!(
                r#"{{"type":"event_msg","payload":{{"type":"agent_message","message":"assistant {index}"}}}}"#
            ));
            lines.push(format!(
                r#"{{"type":"response_item","payload":{{"type":"function_call_output","output":"tool {index}"}}}}"#
            ));
        }

        let entries = codex_entries(lines.into_iter());
        assert_eq!(entries.len(), RECENT_ENTRY_COUNT_PER_KIND * 3);
        assert!(!entries.iter().any(|entry| entry.content.ends_with(" 0")));
        assert!(!entries.iter().any(|entry| entry.content.ends_with(" 1")));
        assert!(entries.iter().any(|entry| entry.content == "user 2"));
        assert!(entries.iter().any(|entry| entry.content == "assistant 2"));
        assert!(entries.iter().any(|entry| entry.content == "tool 2"));
    }

    #[test]
    fn malformed_lines_are_skipped_without_hiding_later_entries() {
        let entries = claude_entries(lines_of(
            "not-json\n{\"type\":\"user\",\"message\":{\"content\":\"still read\"}}",
        ));
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].content, "still read");
    }

    #[test]
    fn textual_value_falls_back_to_content_when_text_is_blank() {
        let item = serde_json::json!([{"text": "   ", "content": "real text"}]);
        assert_eq!(textual_value(Some(&item)), Some("real text".to_string()));
    }

    #[test]
    fn other_agent_class_never_touches_the_filesystem() {
        let missing = Path::new("/nonexistent/ilium-transcript-context/session.jsonl");
        assert!(
            recent_transcript_entries(&AgentClass::Other("custom".to_string()), missing)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn antigravity_reads_matching_user_history() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("antigravity-cli");
        let conversations = root.join("conversations");
        std::fs::create_dir_all(&conversations).unwrap();
        let session_id = "3a333333-3333-4333-8333-333333333333";
        let conversation_path = conversations.join(format!("{session_id}.db"));
        std::fs::write(&conversation_path, "fixture").unwrap();
        std::fs::write(
            root.join("history.jsonl"),
            [
                serde_json::json!({"conversationId": "other", "display": "ignore"}),
                serde_json::json!({"conversationId": session_id, "display": "first task"}),
                serde_json::json!({"conversationId": session_id, "display": "second task"}),
            ]
            .into_iter()
            .map(|entry| entry.to_string())
            .collect::<Vec<_>>()
            .join("\n"),
        )
        .unwrap();

        assert_eq!(
            recent_transcript_entries(&AgentClass::Antigravity, &conversation_path).unwrap(),
            vec![
                TranscriptEntry {
                    kind: TranscriptEntryKind::User,
                    content: "first task".to_string(),
                },
                TranscriptEntry {
                    kind: TranscriptEntryKind::User,
                    content: "second task".to_string(),
                },
            ]
        );
    }

    #[test]
    fn antigravity_missing_history_file_returns_empty_instead_of_erroring() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("antigravity-cli");
        let conversations = root.join("conversations");
        std::fs::create_dir_all(&conversations).unwrap();
        let session_id = "4b444444-4444-4444-8444-444444444444";
        let conversation_path = conversations.join(format!("{session_id}.db"));
        std::fs::write(&conversation_path, "fixture").unwrap();
        // No history.jsonl written at all -- the session simply has no
        // recorded history yet, distinct from a real I/O failure.

        assert_eq!(
            recent_transcript_entries(&AgentClass::Antigravity, &conversation_path).unwrap(),
            Vec::new()
        );
    }
}

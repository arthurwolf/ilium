//! Builds, validates and atomically writes a Claude Code transcript from
//! converted Codex conversation items.
//!
//! The line shape is the one verified to resume in Claude Code 2.1.285: one
//! JSON object per line with `uuid`/`parentUuid` chaining, one content block
//! per assistant line, and every `tool_use` answered by a `tool_result` in the
//! very next line so the replayed history is accepted by the Anthropic API.

use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::{json, Map, Value};

use crate::codex_rollout::{SourceItem, ToolArguments};
use crate::error::ConvertError;

/// Claude Code version stamped on written lines (the one verified to resume).
pub(crate) const CLAUDE_CODE_VERSION: &str = "2.1.285";
/// Any valid model string works for replay; this is the one used when verifying.
const PLACEHOLDER_MODEL: &str = "claude-haiku-4-5-20251001";
/// Tool results longer than this are cut so one command dump cannot dominate
/// the replayed context.
pub(crate) const MAX_TOOL_RESULT_CHARS: usize = 30_000;
const MISSING_RESULT_TEXT: &str = ilium_prompts::conversion::MISSING_RESULT;
const EMPTY_RESULT_TEXT: &str = ilium_prompts::conversion::EMPTY_RESULT;
const SYNTHETIC_FIRST_PROMPT: &str = ilium_prompts::conversion::SYNTHETIC_FIRST_PROMPT;
const MAX_TITLE_CHARS: usize = 80;

const SHELL_TOOL_NAMES: [&str; 6] = [
    "shell",
    "shell_command",
    "exec_command",
    "local_shell",
    "container.exec",
    "unified_exec",
];

pub(crate) struct BuildInput<'a> {
    pub(crate) items: &'a [SourceItem],
    pub(crate) session_id: &'a str,
    pub(crate) cwd: &'a str,
    pub(crate) git_branch: Option<&'a str>,
    /// Used for any line whose source item has no usable timestamp.
    pub(crate) fallback_timestamp: DateTime<Utc>,
}

#[derive(Debug, Default)]
pub(crate) struct BuiltTranscript {
    /// Serialized JSON lines, without trailing newlines.
    pub(crate) lines: Vec<String>,
    /// Source items that became part of the transcript.
    pub(crate) converted_items: usize,
    /// Source items skipped here: orphan or duplicate outputs.
    pub(crate) dropped_items: usize,
    pub(crate) tool_calls: usize,
    pub(crate) synthesized_results: usize,
    pub(crate) truncated_results: usize,
    pub(crate) synthesized_first_prompt: bool,
}

struct LineBuilder<'a> {
    session_id: &'a str,
    cwd: &'a str,
    git_branch: Option<&'a str>,
    previous_uuid: Option<String>,
    last_timestamp: String,
    lines: Vec<String>,
}

impl LineBuilder<'_> {
    fn push_message(&mut self, role: &str, timestamp: Option<&str>, message: Value) {
        let uuid = uuid::Uuid::new_v4().to_string();
        if let Some(timestamp) = timestamp {
            self.last_timestamp = timestamp.to_string();
        }
        let mut line = Map::new();
        line.insert("type".into(), json!(role));
        line.insert("uuid".into(), json!(uuid));
        line.insert("parentUuid".into(), json!(self.previous_uuid));
        line.insert("sessionId".into(), json!(self.session_id));
        line.insert("timestamp".into(), json!(self.last_timestamp));
        line.insert("cwd".into(), json!(self.cwd));
        line.insert("version".into(), json!(CLAUDE_CODE_VERSION));
        line.insert("isSidechain".into(), json!(false));
        line.insert("userType".into(), json!("external"));
        line.insert("entrypoint".into(), json!("cli"));
        if let Some(branch) = self.git_branch {
            line.insert("gitBranch".into(), json!(branch));
        }
        line.insert("message".into(), message);
        self.previous_uuid = Some(uuid);
        self.lines.push(Value::Object(line).to_string());
    }

    fn push_user_text(&mut self, timestamp: Option<&str>, text: &str) {
        self.push_message("user", timestamp, json!({"role": "user", "content": text}));
    }

    fn push_assistant_blocks(&mut self, timestamp: Option<&str>, block: Value, stop_reason: &str) {
        let message_id = format!("msg_{}", uuid::Uuid::new_v4().simple());
        self.push_message(
            "assistant",
            timestamp,
            json!({
                "id": message_id,
                "type": "message",
                "role": "assistant",
                "model": PLACEHOLDER_MODEL,
                "content": [block],
                "stop_reason": stop_reason,
                "stop_sequence": null,
            }),
        );
    }

    fn push_tool_result(
        &mut self,
        timestamp: Option<&str>,
        tool_use_id: &str,
        text: &str,
        is_error: bool,
    ) {
        self.push_message(
            "user",
            timestamp,
            json!({"role": "user", "content": [{
                "type": "tool_result",
                "tool_use_id": tool_use_id,
                "content": text,
                "is_error": is_error,
            }]}),
        );
    }
}

/// Normalises a Codex RFC 3339 timestamp to Claude's `YYYY-MM-DDTHH:MM:SS.mmmZ`.
pub(crate) fn normalize_timestamp(raw: Option<&str>) -> Option<String> {
    let parsed = DateTime::parse_from_rfc3339(raw?).ok()?;
    Some(
        parsed
            .with_timezone(&Utc)
            .to_rfc3339_opts(SecondsFormat::Millis, true),
    )
}

/// Builds the transcript lines. Pure apart from UUID generation.
pub(crate) fn build_transcript(input: &BuildInput<'_>) -> BuiltTranscript {
    let mut built = BuiltTranscript::default();

    // Outputs are matched to calls by id wherever they sit, because parallel
    // calls can answer out of order. The first output for an id wins.
    let call_ids: HashSet<&str> = input
        .items
        .iter()
        .filter_map(|item| match item {
            SourceItem::ToolCall { call_id, .. } => Some(call_id.as_str()),
            _ => None,
        })
        .collect();
    let mut outputs: HashMap<&str, (&str, bool)> = HashMap::new();
    for item in input.items {
        let SourceItem::ToolOutput {
            call_id,
            text,
            is_error,
        } = item
        else {
            continue;
        };
        if call_ids.contains(call_id.as_str()) && !outputs.contains_key(call_id.as_str()) {
            outputs.insert(call_id.as_str(), (text.as_str(), *is_error));
        } else {
            // Orphan output (no call) or a repeated answer to one call.
            built.dropped_items += 1;
        }
    }

    let mut builder = LineBuilder {
        session_id: input.session_id,
        cwd: input.cwd,
        git_branch: input.git_branch,
        previous_uuid: None,
        // Seeded from the first source timestamp so a placeholder first line
        // never post-dates the lines after it.
        last_timestamp: first_source_timestamp(input.items).unwrap_or_else(|| {
            input
                .fallback_timestamp
                .to_rfc3339_opts(SecondsFormat::Millis, true)
        }),
        lines: Vec::new(),
    };

    let first_is_user = input
        .items
        .iter()
        .find(|item| !matches!(item, SourceItem::ToolOutput { .. }))
        .is_some_and(|item| matches!(item, SourceItem::UserText { .. }));
    if !first_is_user && !input.items.is_empty() {
        builder.push_user_text(None, SYNTHETIC_FIRST_PROMPT);
        built.synthesized_first_prompt = true;
    }

    let mut answered: HashSet<&str> = HashSet::new();
    for item in input.items {
        match item {
            SourceItem::UserText { text, timestamp } => {
                let timestamp = normalize_timestamp(timestamp.as_deref());
                builder.push_user_text(timestamp.as_deref(), text);
                built.converted_items += 1;
            }
            SourceItem::AssistantText { text, timestamp } => {
                let timestamp = normalize_timestamp(timestamp.as_deref());
                builder.push_assistant_blocks(
                    timestamp.as_deref(),
                    json!({"type": "text", "text": text}),
                    "end_turn",
                );
                built.converted_items += 1;
            }
            SourceItem::ToolCall {
                call_id,
                name,
                arguments,
                timestamp,
            } => {
                let timestamp = normalize_timestamp(timestamp.as_deref());
                let (tool_name, tool_input) = claude_tool_use(name, arguments);
                let tool_use_id = format!("toolu_{}", uuid::Uuid::new_v4().simple());
                builder.push_assistant_blocks(
                    timestamp.as_deref(),
                    json!({
                        "type": "tool_use",
                        "id": tool_use_id,
                        "name": tool_name,
                        "input": tool_input,
                    }),
                    "tool_use",
                );
                built.tool_calls += 1;
                built.converted_items += 1;

                // Only the first call with a given id receives the output.
                let output = answered
                    .insert(call_id.as_str())
                    .then(|| outputs.get(call_id.as_str()))
                    .flatten();
                match output {
                    Some((text, is_error)) => {
                        let (text, truncated) = bounded_result_text(text);
                        built.truncated_results += usize::from(truncated);
                        builder.push_tool_result(
                            timestamp.as_deref(),
                            &tool_use_id,
                            &text,
                            *is_error,
                        );
                        built.converted_items += 1;
                    }
                    None => {
                        builder.push_tool_result(
                            timestamp.as_deref(),
                            &tool_use_id,
                            MISSING_RESULT_TEXT,
                            false,
                        );
                        built.synthesized_results += 1;
                    }
                }
            }
            // Consumed when their call was emitted; orphans were counted above.
            SourceItem::ToolOutput { .. } => {}
        }
    }

    // `ai-title` is how Claude Code labels a session in its resume picker; it
    // sits outside the uuid chain, exactly as in real transcripts.
    if let Some(title) = first_prompt_title(input.items) {
        builder.lines.push(
            json!({"type": "ai-title", "aiTitle": title, "sessionId": input.session_id})
                .to_string(),
        );
    }
    built.lines = builder.lines;
    built
}

fn first_source_timestamp(items: &[SourceItem]) -> Option<String> {
    items.iter().find_map(|item| match item {
        SourceItem::UserText { timestamp, .. }
        | SourceItem::AssistantText { timestamp, .. }
        | SourceItem::ToolCall { timestamp, .. } => normalize_timestamp(timestamp.as_deref()),
        SourceItem::ToolOutput { .. } => None,
    })
}

fn first_prompt_title(items: &[SourceItem]) -> Option<String> {
    let text = items.iter().find_map(|item| match item {
        SourceItem::UserText { text, .. } => Some(text.as_str()),
        _ => None,
    })?;
    let line = text.lines().map(str::trim).find(|line| !line.is_empty())?;
    Some(line.chars().take(MAX_TITLE_CHARS).collect())
}

fn bounded_result_text(text: &str) -> (String, bool) {
    if text.trim().is_empty() {
        return (EMPTY_RESULT_TEXT.to_string(), false);
    }
    let total = text.chars().count();
    if total <= MAX_TOOL_RESULT_CHARS {
        return (text.to_string(), false);
    }
    let mut kept: String = text.chars().take(MAX_TOOL_RESULT_CHARS).collect();
    kept.push_str(&ilium_prompts::render_value(
        "conversion/truncated-result",
        &serde_json::json!({"v0": (total - MAX_TOOL_RESULT_CHARS).to_string()}),
    ));
    (kept, true)
}

/// Maps a Codex tool call to a Claude `tool_use` name and input object.
pub(crate) fn claude_tool_use(name: &str, arguments: &ToolArguments) -> (String, Value) {
    if SHELL_TOOL_NAMES.contains(&name) {
        if let Some(input) = shell_input(arguments) {
            return ("Bash".to_string(), input);
        }
    }
    if name == "apply_patch" {
        if let Some(patch) = patch_text(arguments) {
            return ("apply_patch".to_string(), json!({"patch": patch}));
        }
    }
    (sanitize_tool_name(name), generic_input(arguments))
}

/// Anthropic tool names must match `^[a-zA-Z0-9_-]{1,64}$`.
pub(crate) fn sanitize_tool_name(name: &str) -> String {
    let sanitized: String = name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '_' || character == '-' {
                character
            } else {
                '_'
            }
        })
        .take(64)
        .collect();
    if sanitized.is_empty() {
        "tool".to_string()
    } else {
        sanitized
    }
}

fn parsed_arguments(arguments: &ToolArguments) -> Option<Value> {
    match arguments {
        ToolArguments::JsonText(text) => serde_json::from_str(text).ok(),
        ToolArguments::Structured(value) => Some(value.clone()),
        ToolArguments::FreeText(_) => None,
    }
}

fn shell_input(arguments: &ToolArguments) -> Option<Value> {
    let parsed = parsed_arguments(arguments)?;
    let command = ["command", "cmd"]
        .iter()
        .find_map(|key| parsed.get(*key))
        .and_then(command_text)?;
    let description = match parsed.get("workdir").and_then(Value::as_str) {
        Some(workdir) if !workdir.is_empty() => ilium_prompts::render_value(
            "conversion/bash-description-workdir",
            &serde_json::json!({"v0": (workdir).to_string()}),
        ),
        _ => ilium_prompts::conversion::BASH_DESCRIPTION.to_string(),
    };
    Some(json!({"command": command, "description": description}))
}

/// A shell command given as text or as an argv array.
fn command_text(command: &Value) -> Option<String> {
    match command {
        Value::String(text) => Some(text.clone()),
        Value::Array(parts) => {
            let argv: Vec<&str> = parts.iter().filter_map(Value::as_str).collect();
            if argv.is_empty() {
                return None;
            }
            // `["bash", "-lc", "script"]` is just `script`.
            if argv.len() == 3 && matches!(argv[1], "-c" | "-lc") {
                return Some(argv[2].to_string());
            }
            Some(
                argv.iter()
                    .map(|part| shell_quote(part))
                    .collect::<Vec<_>>()
                    .join(" "),
            )
        }
        _ => None,
    }
}

fn shell_quote(part: &str) -> String {
    let is_plain = !part.is_empty()
        && part
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_@%+=:,./-".contains(c));
    if is_plain {
        part.to_string()
    } else {
        format!("'{}'", part.replace('\'', "'\\''"))
    }
}

fn patch_text(arguments: &ToolArguments) -> Option<String> {
    match arguments {
        ToolArguments::FreeText(text) => Some(text.clone()),
        other => {
            let parsed = parsed_arguments(other)?;
            ["input", "patch"]
                .iter()
                .find_map(|key| parsed.get(*key).and_then(Value::as_str))
                .map(str::to_string)
        }
    }
}

fn generic_input(arguments: &ToolArguments) -> Value {
    match arguments {
        ToolArguments::FreeText(text) => json!({"input": text}),
        ToolArguments::JsonText(text) if text.trim().is_empty() => json!({}),
        ToolArguments::JsonText(text) => match serde_json::from_str::<Value>(text) {
            Ok(object @ Value::Object(_)) => object,
            _ => json!({"input": text}),
        },
        ToolArguments::Structured(object @ Value::Object(_)) => object.clone(),
        ToolArguments::Structured(other) => json!({"input": other}),
    }
}

/// What [`validate_transcript`] learned about a transcript.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct TranscriptSummary {
    pub(crate) message_lines: usize,
    pub(crate) tool_uses: usize,
}

/// Checks the invariants Claude Code and the Anthropic API depend on: a
/// first user message, an unbroken `parentUuid` chain, one session id, and a
/// `tool_result` immediately after every `tool_use` with the same id.
pub(crate) fn validate_transcript(
    content: &str,
    session_id: &str,
) -> Result<TranscriptSummary, String> {
    let mut summary = TranscriptSummary::default();
    let mut previous_uuid: Option<String> = None;
    let mut pending_tool_use: Option<String> = None;
    for (index, raw) in content.lines().enumerate() {
        let number = index + 1;
        let line: Value =
            serde_json::from_str(raw).map_err(|error| format!("line {number}: {error}"))?;
        let kind = line.get("type").and_then(Value::as_str).unwrap_or("");
        if kind != "user" && kind != "assistant" {
            continue;
        }
        if line.get("sessionId").and_then(Value::as_str) != Some(session_id) {
            return Err(format!("line {number}: wrong sessionId"));
        }
        let uuid = line
            .get("uuid")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("line {number}: missing uuid"))?;
        let parent = line.get("parentUuid").and_then(Value::as_str);
        if parent != previous_uuid.as_deref() {
            return Err(format!("line {number}: parentUuid does not chain"));
        }
        if summary.message_lines == 0 && kind != "user" {
            return Err("first message is not a user message".to_string());
        }
        let blocks = line
            .pointer("/message/content")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default();
        if let Some(expected) = pending_tool_use.take() {
            let answers = blocks.iter().any(|block| {
                block.get("type").and_then(Value::as_str) == Some("tool_result")
                    && block.get("tool_use_id").and_then(Value::as_str) == Some(expected.as_str())
            });
            if kind != "user" || !answers {
                return Err(format!(
                    "line {number}: tool_use {expected} is not answered"
                ));
            }
        }
        for block in blocks {
            if block.get("type").and_then(Value::as_str) == Some("tool_use") {
                let id = block
                    .get("id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| format!("line {number}: tool_use without id"))?;
                pending_tool_use = Some(id.to_string());
                summary.tool_uses += 1;
            }
        }
        previous_uuid = Some(uuid.to_string());
        summary.message_lines += 1;
    }
    if let Some(unanswered) = pending_tool_use {
        return Err(format!("the final tool_use {unanswered} is not answered"));
    }
    if summary.message_lines == 0 {
        return Err("no message lines".to_string());
    }
    Ok(summary)
}

/// Writes `lines` to `<directory>/<session_id>.jsonl` via a temp file in the
/// same directory and a rename, so a half-written transcript never appears
/// under its final name. `should_cancel` is consulted right before the
/// rename; on cancellation or any error the temp file is removed.
pub(crate) fn write_atomically(
    directory: &Path,
    session_id: &str,
    lines: &[String],
    should_cancel: &dyn Fn() -> bool,
) -> Result<PathBuf, ConvertError> {
    let serialized_bytes = serialized_bytes(lines);
    if serialized_bytes > crate::MAX_TRANSCRIPT_BYTES {
        return Err(ConvertError::FileTooLarge {
            path: directory.join(format!("{session_id}.jsonl")),
            bytes: serialized_bytes,
            maximum: crate::MAX_TRANSCRIPT_BYTES,
        });
    }
    let final_path = directory.join(format!("{session_id}.jsonl"));
    let temporary_path = directory.join(format!(".{session_id}.jsonl.tmp"));
    let write_error = |path: &Path, error: std::io::Error| ConvertError::TargetWrite {
        path: path.to_path_buf(),
        error,
    };

    let directory_existed = directory.is_dir();
    ilium_platform::secure_fs::create_private_directory(directory)
        .map_err(|error| write_error(directory, error))?;

    let outcome =
        write_temporary(&temporary_path, lines).map_err(|e| write_error(&temporary_path, e));
    let outcome = outcome.and_then(|()| {
        if should_cancel() {
            return Err(ConvertError::Cancelled);
        }
        std::fs::rename(&temporary_path, &final_path).map_err(|e| write_error(&final_path, e))
    });
    if outcome.is_err() {
        // Best effort: the temp file is ours and never matches `*.jsonl`.
        let _ = std::fs::remove_file(&temporary_path);
        if !directory_existed {
            // Non-recursive: only succeeds if the directory is still empty.
            let _ = std::fs::remove_dir(directory);
        }
    }
    outcome.map(|()| final_path)
}

fn serialized_bytes(lines: &[String]) -> u64 {
    lines
        .iter()
        .try_fold(0u64, |total, line| {
            total.checked_add(line.len() as u64)?.checked_add(1)
        })
        .unwrap_or(u64::MAX)
}

fn write_temporary(path: &Path, lines: &[String]) -> std::io::Result<()> {
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    ilium_platform::secure_fs::restrict_open_file_to_owner(&file)?;
    let mut writer = std::io::BufWriter::new(file);
    for line in lines {
        writer.write_all(line.as_bytes())?;
        writer.write_all(b"\n")?;
    }
    writer.flush()?;
    writer.get_ref().sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_names_are_sanitized_to_the_api_alphabet() {
        assert_eq!(sanitize_tool_name("mcp__a.b/c d"), "mcp__a_b_c_d");
        assert_eq!(sanitize_tool_name(""), "tool");
        assert_eq!(sanitize_tool_name(&"x".repeat(100)).len(), 64);
    }

    #[test]
    fn serialized_size_includes_newlines_and_detects_overflow() {
        assert_eq!(serialized_bytes(&["abc".into(), "x".into()]), 6);
        assert_eq!(serialized_bytes(&[String::new()]), 1);
    }

    #[test]
    fn argv_commands_are_unwrapped_or_quoted() {
        assert_eq!(
            command_text(&json!(["bash", "-lc", "ls -la"])),
            Some("ls -la".to_string())
        );
        assert_eq!(
            command_text(&json!(["echo", "hello world", "it's"])),
            Some("echo 'hello world' 'it'\\''s'".to_string())
        );
    }

    #[test]
    fn timestamps_are_normalised_to_millisecond_z() {
        assert_eq!(
            normalize_timestamp(Some("2026-09-24T12:27:45.931123Z")).as_deref(),
            Some("2026-09-24T12:27:45.931Z")
        );
        assert_eq!(normalize_timestamp(Some("garbage")), None);
    }

    #[test]
    fn long_results_are_truncated_with_a_marker() {
        let text = "x".repeat(MAX_TOOL_RESULT_CHARS + 10);
        let (bounded, truncated) = bounded_result_text(&text);
        assert!(truncated);
        assert!(bounded.contains("10 more characters"));
    }
}

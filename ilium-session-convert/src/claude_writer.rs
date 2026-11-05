//! Builds, validates and atomically writes a Claude Code transcript from
//! converted Codex conversation items.
//!
//! The line shape is the one verified to resume in Claude Code 2.1.285: one
//! JSON object per line with `uuid`/`parentUuid` chaining, one content block
//! per assistant line, and every `tool_use` answered by a `tool_result` in the
//! very next line so the replayed history is accepted by the Anthropic API.
//!
//! Nothing here holds the transcript in memory. This module owns the shape of
//! one line ([`ClaudeLineEmitter`]) and the Codex -> Claude tool mapping;
//! [`transcript_passes`] turns a stream of source items into lines in two
//! passes; [`file_writer`] streams those lines into the final file through a
//! temporary one; [`validation`] re-reads the written file line by line.

mod file_writer;
mod transcript_passes;
mod validation;

use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::{json, Map, Value};

use crate::codex_rollout::{BoundedToolOutput, ToolArguments};
use crate::error::ConvertError;
use crate::MAX_TOOL_RESULT_CHARS;

pub(crate) use file_writer::TranscriptFileWriter;
pub(crate) use transcript_passes::{
    ConversationFacts, RolloutOutputSource, TranscriptAssembler, TranscriptTarget,
};
pub(crate) use validation::validate_transcript;

/// Claude Code version stamped on written lines (the one verified to resume).
pub(crate) const CLAUDE_CODE_VERSION: &str = "2.1.285";
/// Any valid model string works for replay; this is the one used when verifying.
const PLACEHOLDER_MODEL: &str = "claude-haiku-4-5-20251001";
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

/// Where the generated identifiers (line uuids, message ids, tool-use ids)
/// come from. Production draws random v4 UUIDs; tests draw a fixed sequence
/// so two conversions of the same source can be compared byte for byte.
pub(crate) trait IdentifierSource {
    fn next_uuid(&mut self) -> uuid::Uuid;
}

/// The production [`IdentifierSource`].
pub(crate) struct RandomIdentifiers;

impl IdentifierSource for RandomIdentifiers {
    fn next_uuid(&mut self) -> uuid::Uuid {
        uuid::Uuid::new_v4()
    }
}

/// Receives finished transcript lines (without trailing newline) one at a
/// time, so the emitter never has to retain them.
pub(crate) trait LineSink {
    fn write_line(&mut self, line: &str) -> Result<(), ConvertError>;
}

impl LineSink for Vec<String> {
    fn write_line(&mut self, line: &str) -> Result<(), ConvertError> {
        self.push(line.to_string());
        Ok(())
    }
}

/// Emits Claude Code lines in order, maintaining the `parentUuid` chain and
/// the "last known timestamp" that lines without a source timestamp inherit.
pub(crate) struct ClaudeLineEmitter<'a> {
    session_id: &'a str,
    cwd: &'a str,
    git_branch: Option<&'a str>,
    previous_uuid: Option<String>,
    last_timestamp: String,
    identifiers: &'a mut dyn IdentifierSource,
    sink: &'a mut dyn LineSink,
}

impl<'a> ClaudeLineEmitter<'a> {
    /// `initial_timestamp` is what the first line carries when its source
    /// item has none (the first source timestamp, so a placeholder line never
    /// post-dates the lines after it).
    pub(crate) fn new(
        target: &TranscriptTarget<'a>,
        initial_timestamp: String,
        identifiers: &'a mut dyn IdentifierSource,
        sink: &'a mut dyn LineSink,
    ) -> Self {
        Self {
            session_id: target.session_id,
            cwd: target.cwd,
            git_branch: target.git_branch,
            previous_uuid: None,
            last_timestamp: initial_timestamp,
            identifiers,
            sink,
        }
    }

    fn next_uuid(&mut self) -> uuid::Uuid {
        self.identifiers.next_uuid()
    }

    fn push_message(
        &mut self,
        role: &str,
        timestamp: Option<&str>,
        message: Value,
    ) -> Result<(), ConvertError> {
        let uuid = self.next_uuid().to_string();
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
        self.sink.write_line(&Value::Object(line).to_string())
    }

    pub(crate) fn push_user_text(
        &mut self,
        timestamp: Option<&str>,
        text: &str,
    ) -> Result<(), ConvertError> {
        self.push_message("user", timestamp, json!({"role": "user", "content": text}))
    }

    pub(crate) fn push_assistant_blocks(
        &mut self,
        timestamp: Option<&str>,
        block: Value,
        stop_reason: &str,
    ) -> Result<(), ConvertError> {
        let message_id = format!("msg_{}", self.next_uuid().simple());
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
        )
    }

    pub(crate) fn push_tool_result(
        &mut self,
        timestamp: Option<&str>,
        tool_use_id: &str,
        text: &str,
        is_error: bool,
    ) -> Result<(), ConvertError> {
        self.push_message(
            "user",
            timestamp,
            json!({"role": "user", "content": [{
                "type": "tool_result",
                "tool_use_id": tool_use_id,
                "content": text,
                "is_error": is_error,
            }]}),
        )
    }

    /// `ai-title` is how Claude Code labels a session in its resume picker; it
    /// sits outside the uuid chain, exactly as in real transcripts.
    pub(crate) fn push_title(&mut self, title: &str) -> Result<(), ConvertError> {
        self.sink.write_line(
            &json!({"type": "ai-title", "aiTitle": title, "sessionId": self.session_id})
                .to_string(),
        )
    }

    /// A fresh `toolu_` id for the next `tool_use` block.
    pub(crate) fn next_tool_use_id(&mut self) -> String {
        format!("toolu_{}", self.next_uuid().simple())
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

/// The session title Claude Code shows for a prompt: its first non-empty
/// line, cut to [`MAX_TITLE_CHARS`].
fn title_from_prompt(text: &str) -> Option<String> {
    let line = text.lines().map(str::trim).find(|line| !line.is_empty())?;
    Some(line.chars().take(MAX_TITLE_CHARS).collect())
}

/// The result text to replay. The parser already cut the output at
/// [`MAX_TOOL_RESULT_CHARS`]; this adds the marker that says how much of the
/// complete output is missing.
fn bounded_result_text(output: &BoundedToolOutput) -> (String, bool) {
    if output.is_blank() {
        return (EMPTY_RESULT_TEXT.to_string(), false);
    }
    if !output.truncated {
        return (output.text.clone(), false);
    }
    let mut kept = output.text.clone();
    kept.push_str(&ilium_prompts::render_value(
        "conversion/truncated-result",
        &serde_json::json!({"v0": (output.original_chars - MAX_TOOL_RESULT_CHARS).to_string()}),
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

#[cfg(test)]
pub(crate) mod test_support {
    use super::IdentifierSource;

    /// Hands out `00000000-0000-0000-0000-000000000001`, `...0002`, ... so a
    /// conversion is reproducible.
    #[derive(Debug, Default)]
    pub(crate) struct SequentialIdentifiers {
        issued: u128,
    }

    impl IdentifierSource for SequentialIdentifiers {
        fn next_uuid(&mut self) -> uuid::Uuid {
            self.issued += 1;
            uuid::Uuid::from_u128(self.issued)
        }
    }
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
        let output = crate::codex_rollout::flatten_output(&Value::String(
            "x".repeat(MAX_TOOL_RESULT_CHARS + 10),
        ));
        let (bounded, truncated) = bounded_result_text(&output);
        assert!(truncated);
        assert!(bounded.starts_with(&"x".repeat(MAX_TOOL_RESULT_CHARS)));
        assert!(bounded.contains("10 more characters"));
    }

    #[test]
    fn titles_take_the_first_non_empty_line_cut_to_the_limit() {
        assert_eq!(
            title_from_prompt("\n   \n  hello world \nsecond").as_deref(),
            Some("hello world")
        );
        assert_eq!(title_from_prompt("  \n\t"), None);
        let long = "é".repeat(MAX_TITLE_CHARS + 5);
        assert_eq!(
            title_from_prompt(&long).map(|title| title.chars().count()),
            Some(MAX_TITLE_CHARS)
        );
    }

    #[test]
    fn emitted_lines_chain_uuids_and_inherit_the_last_timestamp() {
        let mut identifiers = test_support::SequentialIdentifiers::default();
        let mut lines: Vec<String> = Vec::new();
        let target = TranscriptTarget {
            session_id: "s",
            cwd: "/work",
            git_branch: Some("main"),
            fallback_timestamp: Utc::now(),
        };
        let mut emitter = ClaudeLineEmitter::new(
            &target,
            "2026-01-01T00:00:00.000Z".to_string(),
            &mut identifiers,
            &mut lines,
        );
        emitter.push_user_text(None, "hi").unwrap();
        emitter
            .push_assistant_blocks(
                Some("2026-01-01T00:00:05.000Z"),
                json!({"type": "text", "text": "yo"}),
                "end_turn",
            )
            .unwrap();
        emitter.push_user_text(None, "again").unwrap();
        emitter.push_title("hi").unwrap();
        let parsed: Vec<Value> = lines
            .iter()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(parsed[0]["parentUuid"], Value::Null);
        assert_eq!(parsed[0]["uuid"], "00000000-0000-0000-0000-000000000001");
        assert_eq!(parsed[0]["timestamp"], "2026-01-01T00:00:00.000Z");
        assert_eq!(parsed[0]["gitBranch"], "main");
        assert_eq!(parsed[1]["parentUuid"], parsed[0]["uuid"]);
        assert_eq!(
            parsed[1]["message"]["id"],
            "msg_00000000000000000000000000000002"
        );
        assert_eq!(parsed[1]["uuid"], "00000000-0000-0000-0000-000000000003");
        assert_eq!(parsed[1]["timestamp"], "2026-01-01T00:00:05.000Z");
        assert_eq!(parsed[2]["timestamp"], "2026-01-01T00:00:05.000Z");
        assert_eq!(parsed[3]["type"], "ai-title");
        assert_eq!(parsed[3]["sessionId"], "s");
        assert_eq!(parsed[3].get("uuid"), None);
    }
}

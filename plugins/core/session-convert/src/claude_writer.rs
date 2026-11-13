//! Mapping of Codex items to Claude transcript content: timestamps, titles, tool names, shell commands and
//! result text. Moved from `ilium-session-convert`; the line writer, verification and file handling are host work.

use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::{json, Value};

use crate::rollout::{BoundedToolOutput, ToolArguments};
use crate::MAX_TOOL_RESULT_CHARS;

const EMPTY_RESULT_TEXT: &str = crate::prompts::EMPTY_RESULT;
const MAX_TITLE_CHARS: usize = 80;

const SHELL_TOOL_NAMES: [&str; 6] = [
    "shell",
    "shell_command",
    "exec_command",
    "local_shell",
    "container.exec",
    "unified_exec",
];

/// Normalises a Codex RFC 3339 timestamp to Claude's `YYYY-MM-DDTHH:MM:SS.mmmZ`.
pub fn normalize_timestamp(raw: Option<&str>) -> Option<String> {
    let parsed = DateTime::parse_from_rfc3339(raw?).ok()?;
    Some(
        parsed
            .with_timezone(&Utc)
            .to_rfc3339_opts(SecondsFormat::Millis, true),
    )
}

/// The session title Claude Code shows for a prompt: its first non-empty
/// line, cut to [`MAX_TITLE_CHARS`].
pub fn title_from_prompt(text: &str) -> Option<String> {
    let line = text.lines().map(str::trim).find(|line| !line.is_empty())?;
    Some(line.chars().take(MAX_TITLE_CHARS).collect())
}

/// The result text to replay. The parser already cut the output at
/// [`MAX_TOOL_RESULT_CHARS`]; this adds the marker that says how much of the
/// complete output is missing.
pub fn bounded_result_text(output: &BoundedToolOutput) -> (String, bool) {
    if output.is_blank() {
        return (EMPTY_RESULT_TEXT.to_string(), false);
    }
    if !output.truncated {
        return (output.text.clone(), false);
    }
    let mut kept = output.text.clone();
    kept.push_str(&crate::prompts::truncated_result(
        output.original_chars - MAX_TOOL_RESULT_CHARS,
    ));
    (kept, true)
}

/// Maps a Codex tool call to a Claude `tool_use` name and input object.
pub fn claude_tool_use(name: &str, arguments: &ToolArguments) -> (String, Value) {
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
pub fn sanitize_tool_name(name: &str) -> String {
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
        Some(workdir) if !workdir.is_empty() => crate::prompts::bash_description_workdir(workdir),
        _ => crate::prompts::BASH_DESCRIPTION.to_string(),
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
mod tests {
    use super::*;

    /// REQ: PORT:ilium-session-convert/src/claude_writer.rs::tool_names_are_sanitized_to_the_api_alphabet
    #[test]
    fn tool_names_are_sanitized_to_the_api_alphabet() {
        assert_eq!(sanitize_tool_name("mcp__a.b/c d"), "mcp__a_b_c_d");
        assert_eq!(sanitize_tool_name(""), "tool");
        assert_eq!(sanitize_tool_name(&"x".repeat(100)).len(), 64);
    }

    /// REQ: PORT:ilium-session-convert/src/claude_writer.rs::argv_commands_are_unwrapped_or_quoted
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

    /// REQ: PORT:ilium-session-convert/src/claude_writer.rs::timestamps_are_normalised_to_millisecond_z
    #[test]
    fn timestamps_are_normalised_to_millisecond_z() {
        assert_eq!(
            normalize_timestamp(Some("2026-09-24T12:27:45.931123Z")).as_deref(),
            Some("2026-09-24T12:27:45.931Z")
        );
        assert_eq!(normalize_timestamp(Some("garbage")), None);
    }

    /// REQ: PORT:ilium-session-convert/src/claude_writer.rs::long_results_are_truncated_with_a_marker
    #[test]
    fn long_results_are_truncated_with_a_marker() {
        let output =
            crate::rollout::flatten_output(&Value::String("x".repeat(MAX_TOOL_RESULT_CHARS + 10)));
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
}

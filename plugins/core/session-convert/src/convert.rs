//! Codex -> Claude Code: the pure conversion of a rollout text into a verified Claude Code transcript.
//!
//! Moved from `ilium-session-convert/src/codex_to_claude.rs`. The kernel located the rollout, scanned it twice,
//! streamed the lines into a temporary file and re-read that file to verify it. The plugin gets the rollout text
//! from the host (`fs.read`), builds the lines in memory, verifies them with [`crate::verify`] before they leave
//! the job, and hands them back; writing the file is the host's job (`propose-agent-config`).

use std::path::Path;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::ConvertError;
use crate::rollout::parse_rollout_text_checked;
use crate::transcript::{
    assemble, conversation_item_count, IdentifierSource, TitleLine, TranscriptTarget,
};
use crate::verify::validate_transcript;

/// The job reports progress and checks for a cancel request once per this many rollout lines.
pub const PROGRESS_EVERY_LINES: usize = 50;

/// The input of the `codex-to-claude` job: the rollout's path inside the `agent-sessions:codex` scope.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ConvertInput {
    pub rollout: String,
}

/// The output of the `codex-to-claude` job (manual page, "Codex to Claude Code as a job").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConvertOutput {
    /// The converted transcript, one JSON object per line, no trailing newline.
    pub claude_jsonl: String,
    /// Lines of the transcript.
    pub kept: usize,
    /// Rollout lines left out: reasoning, token events, unreadable lines and tool outputs without a call.
    pub dropped: usize,
    /// The id of the new Claude Code session, the `sessionId` of every line.
    pub session_id: String,
}

/// Converts the text of a Codex rollout.
///
/// `before_line(index, total)` runs before each rollout line (see [`parse_rollout_text_checked`]); an error it
/// returns, such as [`ConvertError::Cancelled`], stops the conversion. `source` only names the rollout in error
/// messages.
pub fn convert_rollout(
    rollout_text: &str,
    source: &Path,
    identifiers: &mut dyn IdentifierSource,
    fallback_timestamp: DateTime<Utc>,
    before_line: &mut dyn FnMut(usize, usize) -> Result<(), ConvertError>,
) -> Result<ConvertOutput, ConvertError> {
    let parsed = parse_rollout_text_checked(rollout_text, before_line)?;
    if conversation_item_count(&parsed.items) == 0 {
        return Err(ConvertError::EmptyConversation {
            path: source.to_path_buf(),
        });
    }
    let session_id = identifiers.next_identifier().hyphenated();
    let cwd = parsed.meta.cwd.clone().unwrap_or_default();
    let target = TranscriptTarget {
        session_id: &session_id,
        cwd: &cwd,
        git_branch: parsed.meta.git_branch.as_deref(),
        fallback_timestamp,
    };
    let assembled = assemble(&parsed.items, &target, identifiers, TitleLine::Omit);
    let claude_jsonl = assembled.lines.join("\n");
    validate_transcript(&claude_jsonl, &session_id).map_err(ConvertError::TargetVerification)?;
    Ok(ConvertOutput {
        kept: assembled.lines.len(),
        dropped: parsed.dropped_total() + assembled.stats.unpaired_outputs,
        claude_jsonl,
        session_id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript::SequentialIdentifiers;
    use serde_json::{json, Value};

    fn user_line(text: &str) -> String {
        json!({"type": "response_item", "payload": {"type": "message", "role": "user", "content": [{"type": "input_text", "text": text}]}})
            .to_string()
    }

    fn assistant_line(text: &str) -> String {
        json!({"type": "response_item", "payload": {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": text}]}})
            .to_string()
    }

    fn call_line() -> String {
        json!({"type": "response_item", "payload": {"type": "function_call", "name": "shell", "arguments": "{\"command\":[\"ls\"]}", "call_id": "c1"}})
            .to_string()
    }

    fn result_line() -> String {
        json!({"type": "response_item", "payload": {"type": "function_call_output", "call_id": "c1", "output": "{\"output\":\"a.txt\\n\"}"}})
            .to_string()
    }

    fn reasoning_line() -> String {
        json!({"type": "response_item", "payload": {"type": "reasoning", "summary": [{"type": "summary_text", "text": "SECRET-THOUGHT"}]}})
            .to_string()
    }

    fn token_line() -> String {
        json!({"type": "event_msg", "payload": {"type": "token_count", "info": null}}).to_string()
    }

    fn standard_text() -> String {
        [
            user_line("hello there"),
            assistant_line("hi, how can I help"),
            call_line(),
            result_line(),
            reasoning_line(),
            token_line(),
        ]
        .join("\n")
    }

    fn fallback() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2030-01-01T00:00:00Z")
            .expect("literal timestamp")
            .with_timezone(&Utc)
    }

    fn convert(text: &str) -> Result<ConvertOutput, ConvertError> {
        convert_rollout(
            text,
            Path::new("rollout.jsonl"),
            &mut SequentialIdentifiers(0),
            fallback(),
            &mut |_, _| Ok(()),
        )
    }

    #[test]
    fn user_prompts_assistant_text_and_tool_calls_are_kept_and_reasoning_is_dropped() {
        let output = convert(&standard_text()).expect("converts");
        assert!(output.claude_jsonl.contains("hello there"));
        assert!(output.claude_jsonl.contains("hi, how can I help"));
        assert!(!output.claude_jsonl.contains("SECRET-THOUGHT"));
        assert!(!output.claude_jsonl.contains("token_count"));
        assert_eq!(output.kept, 4);
        assert_eq!(output.dropped, 2);
        assert_eq!(output.claude_jsonl.split('\n').count(), output.kept);
        for line in output.claude_jsonl.split('\n') {
            let value: Value = serde_json::from_str(line).expect("each line is JSON");
            assert_eq!(value["sessionId"], output.session_id.as_str());
        }
    }

    #[test]
    fn a_call_and_its_result_are_two_separate_lines_in_order() {
        let output = convert(&standard_text()).expect("converts");
        let lines: Vec<&str> = output.claude_jsonl.split('\n').collect();
        let call: Vec<usize> = (0..lines.len())
            .filter(|index| lines[*index].contains("\"ls\""))
            .collect();
        let result: Vec<usize> = (0..lines.len())
            .filter(|index| lines[*index].contains("a.txt"))
            .collect();
        assert_eq!(call, vec![2]);
        assert_eq!(result, vec![3]);
        let first = output.claude_jsonl.find("hello there").expect("prompt");
        let second = output
            .claude_jsonl
            .find("hi, how can I help")
            .expect("answer");
        assert!(first < second);
    }

    #[test]
    fn an_unreadable_line_is_dropped_and_counted() {
        let text = format!("{}\nthis is not json", standard_text());
        let output = convert(&text).expect("converts");
        assert_eq!((output.kept, output.dropped), (4, 3));
    }

    #[test]
    fn a_rollout_without_a_conversation_fails_with_its_source_named() {
        let error = convert(&reasoning_line()).expect_err("nothing to convert");
        assert!(matches!(error, ConvertError::EmptyConversation { .. }));
        assert!(error.to_string().contains("rollout.jsonl"));
        let error = convert("").expect_err("empty");
        assert!(matches!(error, ConvertError::EmptyConversation { .. }));
    }

    #[test]
    fn the_callback_sees_every_line_once_with_the_line_count() {
        let text = standard_text();
        let mut seen = Vec::new();
        convert_rollout(
            &text,
            Path::new("rollout.jsonl"),
            &mut SequentialIdentifiers(0),
            fallback(),
            &mut |index, total| {
                seen.push((index, total));
                Ok(())
            },
        )
        .expect("converts");
        assert_eq!(
            seen,
            (0..6)
                .map(|index| (index, 6))
                .collect::<Vec<(usize, usize)>>()
        );
    }

    #[test]
    fn the_callback_error_stops_the_conversion() {
        let text = standard_text();
        let error = convert_rollout(
            &text,
            Path::new("rollout.jsonl"),
            &mut SequentialIdentifiers(0),
            fallback(),
            &mut |index, _| {
                if index == 3 {
                    Err(ConvertError::Cancelled)
                } else {
                    Ok(())
                }
            },
        )
        .expect_err("cancelled");
        assert!(matches!(error, ConvertError::Cancelled));
    }

    #[test]
    fn the_rollout_directory_and_branch_reach_every_line() {
        let meta = json!({"type": "session_meta", "payload": {"id": "s", "cwd": "/work/project", "git": {"branch": "main"}}})
            .to_string();
        let text = format!("{meta}\n{}", user_line("hi"));
        let output = convert(&text).expect("converts");
        let line: Value = serde_json::from_str(&output.claude_jsonl).expect("one line");
        assert_eq!(line["cwd"], "/work/project");
        assert_eq!(line["gitBranch"], "main");
    }

    #[test]
    fn the_input_of_the_job_ignores_extra_fields() {
        let input: ConvertInput =
            serde_json::from_str(r#"{"rollout":"2030/a.jsonl","unused":true}"#).expect("parses");
        assert_eq!(input.rollout, "2030/a.jsonl");
        assert!(serde_json::from_str::<ConvertInput>("{").is_err());
        assert!(serde_json::from_str::<ConvertInput>("{}").is_err());
    }
}

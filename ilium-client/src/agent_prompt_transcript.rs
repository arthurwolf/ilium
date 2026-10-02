//! Exact text evidence from a verified agent transcript suffix.
//!
//! Naming summaries trim text and fold records. Recovery must retain the
//! provider's raw string and must observe a record appended after Enter.

use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::Path;

use anyhow::Result;
use chrono::{DateTime, Utc};
use ilium_core::AgentClass;
use serde_json::Value;

/// Reads only complete JSONL records after the byte length captured before
/// input delivery. A truncated/replaced file yields no result. The caller
/// verifies path, session and project through `TranscriptLocator` on each read.
pub fn exact_user_prompt_after(
    class: &AgentClass,
    path: &Path,
    baseline_length: u64,
    submitted_after: DateTime<Utc>,
) -> Result<Option<String>> {
    let mut file = File::open(path)?;
    if file.metadata()?.len() < baseline_length {
        return Ok(None);
    }
    file.seek(SeekFrom::Start(baseline_length))?;
    let mut lines = BufReader::new(file);
    let mut line = String::new();
    let mut latest_user = None;
    loop {
        line.clear();
        if lines.read_line(&mut line)? == 0 {
            break;
        }
        // The provider may still be writing this record. Never parse a
        // partial JSONL row as evidence for a completed user message.
        if !line.ends_with('\n') {
            break;
        }
        let Ok(entry) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let Some(recorded_at) = entry
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        else {
            continue;
        };
        if recorded_at < submitted_after {
            continue;
        }
        match class {
            AgentClass::Claude => {
                if entry.get("type").and_then(Value::as_str) == Some("user")
                    && entry.get("isSidechain").and_then(Value::as_bool) != Some(true)
                {
                    if let Some(text) = entry
                        .get("message")
                        .and_then(|message| message.get("content"))
                        .and_then(Value::as_str)
                    {
                        if !text.is_empty() {
                            latest_user = Some(text.to_owned());
                        }
                    }
                }
            }
            AgentClass::Codex => match entry.get("type").and_then(Value::as_str) {
                Some("event_msg") => {
                    let payload = entry.get("payload");
                    if payload
                        .and_then(|payload| payload.get("type"))
                        .and_then(Value::as_str)
                        == Some("user_message")
                    {
                        if let Some(text) = payload
                            .and_then(|payload| payload.get("message"))
                            .and_then(Value::as_str)
                        {
                            if !text.is_empty()
                                && !crate::transcript_context::is_codex_context_envelope(text)
                            {
                                latest_user = Some(text.to_owned());
                            }
                        }
                    }
                }
                Some("response_item") => {
                    let payload = entry.get("payload");
                    if payload
                        .and_then(|payload| payload.get("type"))
                        .and_then(Value::as_str)
                        == Some("message")
                        && payload
                            .and_then(|payload| payload.get("role"))
                            .and_then(Value::as_str)
                            == Some("user")
                    {
                        let content = payload.and_then(|payload| payload.get("content"));
                        if let Some(text) = content.and_then(codex_single_text) {
                            if !text.is_empty()
                                && !crate::transcript_context::is_codex_context_envelope(text)
                            {
                                latest_user = Some(text.to_owned());
                            }
                        }
                    }
                }
                Some(_) | None => {}
            },
            AgentClass::Antigravity | AgentClass::Other(_) => {}
        }
    }
    Ok(latest_user)
}

fn codex_single_text(content: &Value) -> Option<&str> {
    if let Some(text) = content.as_str() {
        return Some(text);
    }
    let blocks = content.as_array()?;
    if blocks.len() != 1 {
        return None;
    }
    let block = &blocks[0];
    if !matches!(
        block.get("type").and_then(Value::as_str),
        Some("input_text" | "text")
    ) {
        return None;
    }
    block.get("text").and_then(Value::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn cutoff() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-10-02T20:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn repeated_identical_codex_prompt_is_new_evidence_and_keeps_trailing_text() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.jsonl");
        let old = serde_json::json!({"timestamp":"2026-10-02T20:00:01Z","type":"event_msg", "payload":{"type":"user_message","message":"same  "}});
        std::fs::write(&path, format!("{old}\n")).unwrap();
        let baseline = std::fs::metadata(&path).unwrap().len();
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        writeln!(file, "{old}").unwrap();
        assert_eq!(
            exact_user_prompt_after(&AgentClass::Codex, &path, baseline, cutoff()).unwrap(),
            Some("same  ".to_string())
        );
    }

    #[test]
    fn claude_user_string_preserves_multiline_and_ignores_tool_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.jsonl");
        let user = serde_json::json!({"timestamp":"2026-10-02T20:00:01Z","type":"user","message":{"content":"line 1\nline 2  "}});
        let tool = serde_json::json!({"timestamp":"2026-10-02T20:00:02Z","type":"user","message":{"content":[{"type":"tool_result","content":"wrong"}]}});
        std::fs::write(&path, format!("{user}\n{tool}\n")).unwrap();
        assert_eq!(
            exact_user_prompt_after(&AgentClass::Claude, &path, 0, cutoff()).unwrap(),
            Some("line 1\nline 2  ".to_string())
        );
    }

    #[test]
    fn incomplete_suffix_is_not_a_submission() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.jsonl");
        std::fs::write(&path, b"{\"type\":\"event_msg\",\"payload\":{\"type\":\"user_message\",\"message\":\"partial\"}}").unwrap();
        assert_eq!(
            exact_user_prompt_after(&AgentClass::Codex, &path, 0, cutoff()).unwrap(),
            None
        );
    }

    #[test]
    fn newer_event_user_supersedes_older_response_user() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.jsonl");
        let older = serde_json::json!({"timestamp":"2026-10-02T20:00:01Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"older"}]}});
        let newer = serde_json::json!({"timestamp":"2026-10-02T20:00:02Z","type":"event_msg","payload":{"type":"user_message","message":"newer  "}});
        std::fs::write(&path, format!("{older}\n{newer}\n")).unwrap();
        assert_eq!(
            exact_user_prompt_after(&AgentClass::Codex, &path, 0, cutoff()).unwrap(),
            Some("newer  ".to_string())
        );
    }

    #[test]
    fn delayed_prior_turn_after_offset_is_rejected_by_timestamp() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.jsonl");
        let old = serde_json::json!({"timestamp":"2026-10-02T19:59:59Z","type":"event_msg","payload":{"type":"user_message","message":"stale"}});
        std::fs::write(&path, format!("{old}\n")).unwrap();
        assert_eq!(
            exact_user_prompt_after(&AgentClass::Codex, &path, 0, cutoff()).unwrap(),
            None
        );
    }
}

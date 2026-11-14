//! Checks a written Claude Code transcript line by line against the invariants Claude Code and the Anthropic API
//! depend on. Moved from `ilium-session-convert/src/claude_writer/validation.rs`; the plugin verifies the text it
//! read back from the host instead of streaming a file.

use serde_json::Value;

/// The longest line a transcript may hold; the same bound the Codex reader applies.
pub const MAX_LINE_BYTES: usize = 256 * 1024 * 1024;

/// What [`validate_transcript`] learned about a transcript.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct TranscriptSummary {
    pub message_lines: usize,
    pub tool_uses: usize,
}

/// Checks a first user message, an unbroken `parentUuid` chain, one session id, and a `tool_result` immediately
/// after every `tool_use` with the same id. The error names the offending line.
pub fn validate_transcript(text: &str, session_id: &str) -> Result<TranscriptSummary, String> {
    let mut summary = TranscriptSummary::default();
    let mut previous_uuid: Option<String> = None;
    let mut pending_tool_use: Option<String> = None;
    for (index, raw) in text.split('\n').enumerate() {
        let number = index + 1;
        let raw = raw.strip_suffix('\r').unwrap_or(raw);
        // Blank lines (a trailing newline, a stray empty line) carry nothing.
        if raw.is_empty() {
            continue;
        }
        if raw.len() > MAX_LINE_BYTES {
            return Err(format!(
                "line {number}: {} bytes is longer than the {MAX_LINE_BYTES} byte line limit",
                raw.len()
            ));
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    const SESSION: &str = "s-1";

    fn user(uuid: &str, parent: Option<&str>, content: Value) -> String {
        serde_json::json!({
            "type": "user", "uuid": uuid, "parentUuid": parent, "sessionId": SESSION,
            "message": {"role": "user", "content": content},
        })
        .to_string()
    }

    fn assistant(uuid: &str, parent: Option<&str>, block: Value) -> String {
        serde_json::json!({
            "type": "assistant", "uuid": uuid, "parentUuid": parent, "sessionId": SESSION,
            "message": {"role": "assistant", "content": [block]},
        })
        .to_string()
    }

    fn validate(lines: &[String]) -> Result<TranscriptSummary, String> {
        let mut text = lines.join("\n");
        text.push('\n');
        validate_transcript(&text, SESSION)
    }

    #[test]
    fn a_well_formed_transcript_is_summarised() {
        let lines = [
            user("u1", None, Value::String("hi".into())),
            assistant(
                "a1",
                Some("u1"),
                serde_json::json!({"type": "tool_use", "id": "t1", "name": "Bash", "input": {}}),
            ),
            user(
                "u2",
                Some("a1"),
                serde_json::json!([{"type": "tool_result", "tool_use_id": "t1", "content": "ok"}]),
            ),
            serde_json::json!({"type": "ai-title", "aiTitle": "hi", "sessionId": SESSION})
                .to_string(),
        ];
        assert_eq!(
            validate(&lines),
            Ok(TranscriptSummary {
                message_lines: 3,
                tool_uses: 1
            })
        );
        // A CRLF transcript validates the same way.
        let crlf = lines.join("\r\n") + "\r\n";
        assert!(validate_transcript(&crlf, SESSION).is_ok());
    }

    #[test]
    fn every_broken_invariant_names_its_line() {
        let first_user = user("u1", None, Value::String("hi".into()));
        let cases: Vec<(Vec<String>, &str)> = vec![
            (
                vec![assistant(
                    "a1",
                    None,
                    serde_json::json!({"type": "text", "text": "x"}),
                )],
                "first message is not a user message",
            ),
            (
                vec![
                    first_user.clone(),
                    user("u2", Some("nope"), Value::String("x".into())),
                ],
                "line 2: parentUuid does not chain",
            ),
            (
                vec![first_user.clone(), "{not json".to_string()],
                "line 2: ",
            ),
            (
                vec![
                    first_user.clone(),
                    assistant(
                        "a1",
                        Some("u1"),
                        serde_json::json!({"type": "tool_use", "id": "t1", "name": "x", "input": {}}),
                    ),
                    assistant(
                        "a2",
                        Some("a1"),
                        serde_json::json!({"type": "text", "text": "x"}),
                    ),
                ],
                "line 3: tool_use t1 is not answered",
            ),
            (
                vec![
                    first_user.clone(),
                    assistant(
                        "a1",
                        Some("u1"),
                        serde_json::json!({"type": "tool_use", "id": "t1", "name": "x", "input": {}}),
                    ),
                ],
                "the final tool_use t1 is not answered",
            ),
            (
                vec![first_user.clone().replace(SESSION, "other")],
                "line 1: wrong sessionId",
            ),
            (vec![], "no message lines"),
        ];
        for (lines, expected) in cases {
            let error = validate(&lines).unwrap_err();
            assert!(error.starts_with(expected), "{error:?} vs {expected:?}");
        }
    }

    #[test]
    fn a_corrupted_line_in_the_middle_is_reported_with_its_number() {
        let mut lines = vec![user("u1", None, Value::String("hi".into()))];
        for index in 2..=5 {
            lines.push(assistant(
                &format!("a{index}"),
                Some(if index == 2 { "u1" } else { "" }),
                serde_json::json!({"type": "text", "text": "x"}),
            ));
        }
        // Fix the chain by hand, then corrupt line 4 only.
        let mut previous = "u1".to_string();
        for (index, line) in lines.iter_mut().enumerate().skip(1) {
            let uuid = format!("a{}", index + 1);
            *line = assistant(
                &uuid,
                Some(&previous),
                serde_json::json!({"type": "text", "text": "x"}),
            );
            previous = uuid;
        }
        lines[3] = lines[3][..lines[3].len() / 2].to_string();
        let error = validate(&lines).unwrap_err();
        assert!(error.starts_with("line 4: "), "{error}");
    }
}

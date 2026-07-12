//! Reads a Claude Code/Codex session transcript (JSONL) and extracts the
//! handful of most recent prompts the user actually typed, compacted down
//! to a bounded size -- the raw material `session_naming` sends to the free
//! LLM to infer a short session title.
//!
//! Deliberately separate from `session_naming` (which owns the LLM prompt
//! template and response parsing) and from `agent_detect` (which only
//! knows transcript *paths*, never their content): this module's one job is
//! turning transcript bytes into a short list of plain-text prompt strings.

use std::path::Path;

use illium_core::AgentClass;
use serde_json::Value;

/// How many of the user's most recent prompts feed the title inference --
/// enough for the LLM to see a session's arc without the prompt growing
/// unbounded on a long-running pane.
pub const RECENT_PROMPT_COUNT: usize = 5;

/// Per-prompt line cap passed to `compact_prompt`. A long pasted prompt
/// (a stack trace, a file dump) would otherwise dominate the request; the
/// head and tail of a long prompt are almost always more informative than
/// its middle.
pub const PROMPT_MAX_LINES: usize = 100;

/// Reads `transcript_path` and returns up to `RECENT_PROMPT_COUNT` of the
/// most recent prompts the user actually typed, oldest first, each
/// compacted to at most `PROMPT_MAX_LINES` lines. Returns an empty `Vec`
/// (not an error) for an `AgentClass::Other` transcript, since illium has
/// no known parser for an arbitrary agent CLI's transcript format.
pub fn recent_user_prompts(
    class: &AgentClass,
    transcript_path: &Path,
) -> anyhow::Result<Vec<String>> {
    let contents = std::fs::read_to_string(transcript_path)?;
    let prompts = extract_user_prompts(class, &contents);
    let recent_start = prompts.len().saturating_sub(RECENT_PROMPT_COUNT);
    Ok(prompts[recent_start..]
        .iter()
        .map(|prompt| compact_prompt(prompt, PROMPT_MAX_LINES))
        .collect())
}

/// Pure extraction over already-read transcript text, in transcript order
/// (oldest first) -- split out from `recent_user_prompts` so parsing logic
/// is testable without touching a filesystem.
fn extract_user_prompts(class: &AgentClass, contents: &str) -> Vec<String> {
    match class {
        AgentClass::Claude => claude_user_prompts(contents),
        AgentClass::Codex => codex_user_prompts(contents),
        AgentClass::Other(_) => Vec::new(),
    }
}

/// Claude Code writes one JSON object per line. A genuine user-typed
/// message has `"type":"user"`, is not part of a sub-agent sidechain, and
/// carries its text directly as a JSON string in `message.content` --
/// tool-result turns are also `"type":"user"` but their `content` is
/// always a JSON array, never a plain string, which is what tells the two
/// apart here.
fn claude_user_prompts(contents: &str) -> Vec<String> {
    contents
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|entry| entry.get("type").and_then(Value::as_str) == Some("user"))
        .filter(|entry| {
            !entry
                .get("isSidechain")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        })
        .filter_map(|entry| {
            entry
                .get("message")?
                .get("content")?
                .as_str()
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(str::to_string)
        })
        .collect()
}

/// Codex's rollout files interleave several record kinds; the clean,
/// human-typed text of one turn is the `event_msg` records whose payload
/// type is `user_message` -- unlike the `response_item` records (also
/// `role: "user"`), which additionally carry Codex's own injected
/// `AGENTS.md`/environment-context boilerplate ahead of the real prompt.
fn codex_user_prompts(contents: &str) -> Vec<String> {
    contents
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|entry| entry.get("type").and_then(Value::as_str) == Some("event_msg"))
        .filter_map(|entry| {
            let payload = entry.get("payload")?;
            (payload.get("type").and_then(Value::as_str) == Some("user_message"))
                .then(|| payload.get("message")?.as_str())
                .flatten()
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(str::to_string)
        })
        .collect()
}

/// Compacts `text` to at most `max_lines` lines by keeping its first and
/// last halves and collapsing the middle into a single marker line noting
/// how many lines were dropped -- the beginning of a prompt states the
/// task, the end usually carries the most recent refinement, and a long
/// pasted block (logs, file contents) in between is the least useful part
/// to send to the title-inference LLM.
fn compact_prompt(text: &str, max_lines: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() <= max_lines || max_lines == 0 {
        return text.to_string();
    }

    let head_count = max_lines / 2;
    let tail_count = max_lines - head_count - 1;
    let omitted = lines.len() - head_count - tail_count;

    let mut compacted: Vec<String> = lines[..head_count]
        .iter()
        .map(|&line| line.to_string())
        .collect();
    compacted.push(format!("… [{omitted} lines omitted] …"));
    compacted.extend(
        lines[lines.len() - tail_count..]
            .iter()
            .map(|&line| line.to_string()),
    );
    compacted.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_prompts_keep_typed_user_text_and_skip_tool_results_and_sidechains() {
        let contents = [
            r#"{"type":"user","isSidechain":false,"message":{"role":"user","content":"first prompt"}}"#,
            r#"{"type":"user","isSidechain":false,"message":{"role":"user","content":[{"type":"tool_result","content":"ls output"}]}}"#,
            r#"{"type":"user","isSidechain":true,"message":{"role":"user","content":"sub-agent internal prompt"}}"#,
            r#"{"type":"assistant","message":{"role":"assistant","content":"a reply"}}"#,
            r#"{"type":"user","message":{"role":"user","content":"second prompt, no isSidechain field"}}"#,
        ]
        .join("\n");

        assert_eq!(
            claude_user_prompts(&contents),
            vec![
                "first prompt".to_string(),
                "second prompt, no isSidechain field".to_string(),
            ]
        );
    }

    #[test]
    fn codex_prompts_keep_user_message_events_and_skip_response_items() {
        let contents = [
            r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"<environment_context>...</environment_context>"}]}}"#,
            r#"{"type":"event_msg","payload":{"type":"token_count","info":null}}"#,
            r#"{"type":"event_msg","payload":{"type":"user_message","message":"write a fibonacci function","images":null}}"#,
        ]
        .join("\n");

        assert_eq!(
            codex_user_prompts(&contents),
            vec!["write a fibonacci function".to_string()]
        );
    }

    #[test]
    fn recent_user_prompts_keeps_only_the_most_recent_and_ignores_malformed_lines() {
        let dir = std::env::temp_dir()
            .join("illium-transcript-prompts-tests")
            .join(format!("{:?}", std::thread::current().id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("session.jsonl");

        let mut lines = vec!["not json at all".to_string()];
        for index in 0..(RECENT_PROMPT_COUNT + 2) {
            lines.push(format!(
                r#"{{"type":"user","message":{{"role":"user","content":"prompt {index}"}}}}"#
            ));
        }
        std::fs::write(&path, lines.join("\n")).unwrap();

        let prompts = recent_user_prompts(&AgentClass::Claude, &path).unwrap();
        assert_eq!(prompts.len(), RECENT_PROMPT_COUNT);
        assert_eq!(prompts.first().unwrap(), "prompt 2");
        assert_eq!(
            prompts.last().unwrap(),
            &format!("prompt {}", RECENT_PROMPT_COUNT + 1)
        );
    }

    #[test]
    fn other_agent_class_yields_no_prompts() {
        assert!(
            extract_user_prompts(&AgentClass::Other("opencode".to_string()), "irrelevant")
                .is_empty()
        );
    }

    #[test]
    fn compact_prompt_leaves_short_text_untouched() {
        let text = "line one\nline two\nline three";
        assert_eq!(compact_prompt(text, 100), text);
    }

    #[test]
    fn compact_prompt_keeps_head_and_tail_and_notes_the_omitted_count() {
        let lines: Vec<String> = (0..250).map(|index| format!("line {index}")).collect();
        let text = lines.join("\n");

        let compacted = compact_prompt(&text, 100);
        let compacted_lines: Vec<&str> = compacted.lines().collect();

        assert_eq!(compacted_lines.len(), 100);
        assert_eq!(compacted_lines[0], "line 0");
        assert_eq!(compacted_lines[49], "line 49");
        assert_eq!(compacted_lines[50], "… [151 lines omitted] …");
        assert_eq!(compacted_lines[51], "line 201");
        assert_eq!(compacted_lines[99], "line 249");
    }
}

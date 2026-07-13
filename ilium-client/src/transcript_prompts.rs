//! Reads a Claude Code/Codex session transcript (JSONL) and extracts the
//! handful of most recent prompts the user actually typed, compacted down
//! to a bounded size -- the raw material `session_naming` sends to the free
//! LLM to infer a short session title.
//!
//! Deliberately separate from `session_naming` (which owns the LLM prompt
//! template and response parsing) and from `agent_detect` (which only
//! knows transcript *paths*, never their content): this module's one job is
//! turning transcript bytes into a short list of plain-text prompt strings.

use std::collections::VecDeque;
use std::path::Path;

use ilium_core::AgentClass;
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
/// (not an error) for an `AgentClass::Other` transcript, since ilium has
/// no known parser for an arbitrary agent CLI's transcript format.
pub fn recent_user_prompts(
    class: &AgentClass,
    transcript_path: &Path,
) -> anyhow::Result<Vec<String>> {
    // `AgentClass::Other` has no known transcript format at all -- its
    // `transcript_path` (when callers have one to pass) isn't guaranteed to
    // exist or be readable, and this function's contract is to hand back
    // "no prompts" for it, not an I/O error, so check before touching the
    // filesystem rather than after.
    if matches!(class, AgentClass::Other(_)) {
        return Ok(Vec::new());
    }
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
/// is testable without touching a filesystem. Already bounded to at most
/// `RECENT_PROMPT_COUNT` entries by the underlying parser (see
/// `take_last`): a long-running session's transcript can carry many
/// thousands of user turns, and materializing every one of them just to
/// keep the last handful would make this allocation scale with the whole
/// session's lifetime instead of with the bounded window the caller
/// actually needs.
fn extract_user_prompts(class: &AgentClass, contents: &str) -> Vec<String> {
    match class {
        AgentClass::Claude => claude_user_prompts(contents),
        AgentClass::Codex => codex_user_prompts(contents),
        AgentClass::Other(_) => Vec::new(),
    }
}

/// Keeps only the last `cap` items yielded by `texts`, in original order,
/// without ever holding more than `cap` of them at once -- a bounded
/// `VecDeque` ring buffer rather than collecting the full iterator and
/// truncating afterwards, so a transcript with an unbounded number of
/// matching lines never forces an unbounded intermediate allocation.
fn take_last(texts: impl Iterator<Item = String>, cap: usize) -> Vec<String> {
    if cap == 0 {
        return Vec::new();
    }
    let mut recent: VecDeque<String> = VecDeque::with_capacity(cap);
    for text in texts {
        if recent.len() == cap {
            recent.pop_front();
        }
        recent.push_back(text);
    }
    recent.into_iter().collect()
}

/// Claude Code writes one JSON object per line. A genuine user-typed
/// message has `"type":"user"`, is not part of a sub-agent sidechain, and
/// carries its text directly as a JSON string in `message.content` --
/// tool-result turns are also `"type":"user"` but their `content` is
/// always a JSON array, never a plain string, which is what tells the two
/// apart here.
fn claude_user_prompts(contents: &str) -> Vec<String> {
    take_last(
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
            }),
        RECENT_PROMPT_COUNT,
    )
}

/// Codex's rollout files interleave several record kinds; the clean,
/// human-typed text of one turn is either an `event_msg` record whose
/// payload type is `user_message`, or -- when Codex tracks that turn's
/// text as a persistent goal instead -- an `event_msg` record whose
/// payload type is `thread_goal_updated`, carrying the same text as
/// `payload.goal.objective`. A given session emits one or the other for
/// its first turn (observed on real transcripts; the "goal" form leaves
/// zero `user_message` records), never both, so both are extracted here
/// rather than treating `user_message` as the only source. Unlike the
/// `response_item` records (also `role: "user"`), neither of these carries
/// Codex's own injected `AGENTS.md`/environment-context boilerplate ahead
/// of the real prompt.
fn codex_user_prompts(contents: &str) -> Vec<String> {
    take_last(
        contents
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .filter(|entry| entry.get("type").and_then(Value::as_str) == Some("event_msg"))
            .filter_map(|entry| {
                let payload = entry.get("payload")?;
                match payload.get("type").and_then(Value::as_str) {
                    Some("user_message") => payload.get("message")?.as_str(),
                    Some("thread_goal_updated") => payload.get("goal")?.get("objective")?.as_str(),
                    _ => None,
                }
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(str::to_string)
            }),
        RECENT_PROMPT_COUNT,
    )
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
    fn codex_prompts_fall_back_to_thread_goal_updated_when_no_user_message_events_exist() {
        // Real Codex rollouts sometimes record the first turn's text only
        // as a tracked goal, with zero `user_message` events at all -- the
        // exact shape that used to make `infer_pane_title` fail every time
        // for a session recorded this way.
        let contents = [
            r#"{"type":"event_msg","payload":{"type":"thread_goal_updated","goal":{"objective":"add a hourly proxy-sync task","status":"active"}}}"#,
            r#"{"type":"event_msg","payload":{"type":"task_started","turn_id":"abc"}}"#,
        ]
        .join("\n");

        assert_eq!(
            codex_user_prompts(&contents),
            vec!["add a hourly proxy-sync task".to_string()]
        );
    }

    #[test]
    fn recent_user_prompts_keeps_only_the_most_recent_and_ignores_malformed_lines() {
        let dir = std::env::temp_dir()
            .join("ilium-transcript-prompts-tests")
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
    fn recent_user_prompts_returns_empty_for_other_agent_class_without_touching_the_filesystem() {
        // A path that does not exist -- if `recent_user_prompts` tried to
        // read it before checking the agent class, this would surface as an
        // `Err` instead of the documented empty `Vec`.
        let missing_path = Path::new("/nonexistent/ilium-transcript-prompts-test/session.jsonl");

        let prompts =
            recent_user_prompts(&AgentClass::Other("opencode".to_string()), missing_path).unwrap();

        assert!(prompts.is_empty());
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

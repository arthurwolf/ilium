//! Reads the official Codex thread name out of the text of `~/.codex/session_index.jsonl`.
//!
//! Codex does not record its thread name inside the per-session rollout files. It appends
//! `{"id","thread_name","updated_at"}` lines to this index, one per naming or rename event, so
//! the last line for an id is the current name. The host reads the file (import `fs`, scope
//! `home`) and passes its text here; this module holds only the rule.

/// Upper bound on the index size the caller may read into memory. The live index is well under
/// 1 MiB; a larger file is treated as unreadable instead of buffered.
pub const MAX_INDEX_BYTES: u64 = 32 * 1024 * 1024;

/// The path of the index below the user's home directory.
pub const INDEX_RELATIVE_PATH: &str = ".codex/session_index.jsonl";

/// The current official thread name for `session_id`, or `None` when Codex has not published
/// one yet (it names a thread some time after the first turn) or the index holds none.
pub fn latest_thread_name(index: &str, session_id: &str) -> Option<String> {
    index
        .lines()
        .rev()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|entry| entry.get("id").and_then(|id| id.as_str()) == Some(session_id))
        .find_map(|entry| {
            let name = entry.get("thread_name")?.as_str()?.trim();
            (!name.is_empty()).then(|| name.to_string())
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SESSION: &str = "01a03ef2-4024-72c0-9ce7-203b39cd3a2a";

    #[test]
    fn the_last_line_for_a_session_is_its_current_name() {
        let index = format!(
            "{{\"id\":\"{SESSION}\",\"thread_name\":\"no actually looking\",\"updated_at\":\"a\"}}\n\
             {{\"id\":\"other\",\"thread_name\":\"Other thread\",\"updated_at\":\"b\"}}\n\
             {{\"id\":\"{SESSION}\",\"thread_name\":\"Fix spike lengths on bubbles\",\"updated_at\":\"c\"}}\n"
        );
        assert_eq!(
            latest_thread_name(&index, SESSION).as_deref(),
            Some("Fix spike lengths on bubbles")
        );
        assert_eq!(
            latest_thread_name(&index, "other").as_deref(),
            Some("Other thread")
        );
    }

    #[test]
    fn unknown_sessions_blank_names_and_malformed_lines_yield_none() {
        let index = format!(
            "not json\n{{\"id\":\"{SESSION}\",\"thread_name\":\"  \",\"updated_at\":\"a\"}}\n"
        );
        assert_eq!(latest_thread_name(&index, SESSION), None);
        assert_eq!(latest_thread_name(&index, "missing"), None);
    }

    #[test]
    fn a_blank_latest_line_falls_back_to_the_previous_name() {
        let index = format!(
            "{{\"id\":\"{SESSION}\",\"thread_name\":\"Earlier name\",\"updated_at\":\"a\"}}\n\
             {{\"id\":\"{SESSION}\",\"thread_name\":\"\",\"updated_at\":\"b\"}}\n"
        );
        assert_eq!(
            latest_thread_name(&index, SESSION).as_deref(),
            Some("Earlier name")
        );
    }

    #[test]
    fn an_empty_index_has_no_name() {
        assert_eq!(latest_thread_name("", SESSION), None);
    }
}

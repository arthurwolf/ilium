//! Titles of plain shell panes: the command a user ran becomes the pane title.
//!
//! The kernel's input tracker owns the PTY bytes. It rebuilds the line from the editing keys,
//! reports only a line it could rebuild exactly, and only while the shell (not a running
//! program) owns the terminal; it hands that line to plugins as `user-line-submitted`
//! (primitives-terminal-reads.md). This module owns the rule: what a line is worth as a title.
//! The rename itself goes through `rename-node` with source `inferred`, which the kernel applies
//! only to a pane that is a confirmed plain shell with an automatic title, so a title typed by
//! hand is never replaced.

use ilium_plugin_sdk::types::{Intent, NodeId, RenameNodeSpec, Submission, TitleSource};

/// A shell title is at most this many characters, then an ellipsis.
pub const MAX_TITLE_CHARS: usize = 80;

/// The title a submitted shell line gives its pane: whitespace compacted to single spaces and
/// cut to [`MAX_TITLE_CHARS`] characters (not bytes) with a trailing `…`. `None` for a line that
/// is empty after compaction.
pub fn title_for_line(line: &str) -> Option<String> {
    let compact = line.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.is_empty() {
        return None;
    }
    let mut title = String::new();
    for (count, character) in compact.chars().enumerate() {
        if count >= MAX_TITLE_CHARS {
            title.push('…');
            break;
        }
        title.push(character);
    }
    Some(title)
}

/// The rename a submitted line asks for, `None` when the pane runs an agent (an agent prompt is
/// not a shell command, the model names those panes) or the line gives no title.
pub fn rename_for_line(pane: NodeId, line: &str, has_agent: bool) -> Option<Submission> {
    if has_agent {
        return None;
    }
    let title = title_for_line(line)?;
    Some(Submission::new(Intent::RenameNode(RenameNodeSpec {
        node: pane,
        title,
        source: TitleSource::Inferred,
        expected_revision: None,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// REQ: PORT:ilium-server/src/shell_title.rs::normalizes_whitespace_and_truncates_by_characters
    #[test]
    fn normalizes_whitespace_and_truncates_by_characters() {
        assert_eq!(
            title_for_line("  echo   café  ").as_deref(),
            Some("echo café")
        );
        let long_command = "é".repeat(MAX_TITLE_CHARS + 1);
        assert_eq!(
            title_for_line(&long_command),
            Some(format!("{}…", "é".repeat(MAX_TITLE_CHARS)))
        );
        let exact = "é".repeat(MAX_TITLE_CHARS);
        assert_eq!(title_for_line(&exact), Some(exact));
    }

    #[test]
    fn a_blank_line_gives_no_title() {
        assert_eq!(title_for_line(""), None);
        assert_eq!(title_for_line(" \t "), None);
    }

    #[test]
    fn a_multiline_paste_is_one_compact_title() {
        assert_eq!(
            title_for_line("first line\nsecond line").as_deref(),
            Some("first line second line")
        );
    }

    /// REQ: PORT:ilium-server/tests/shell_command_titles.rs::completed_shell_commands_update_the_title_for_other_clients_until_user_rename
    #[test]
    fn a_shell_line_renames_its_pane_as_an_inferred_title() {
        let submission = rename_for_line(NodeId(7), "git status", false).expect("a rename");
        let Intent::RenameNode(spec) = submission.intent else {
            panic!("a rename is submitted");
        };
        assert_eq!(spec.node, NodeId(7));
        assert_eq!(spec.title, "git status");
        assert_eq!(spec.source, TitleSource::Inferred);
        assert_eq!(spec.expected_revision, None);
    }

    #[test]
    fn an_agent_pane_prompt_is_not_a_shell_title() {
        assert!(rename_for_line(NodeId(7), "fix the login bug", true).is_none());
    }
}

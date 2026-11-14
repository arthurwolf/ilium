//! "Create an agent from a line": the rule for which clicked editor line counts and the prompt
//! the new agent pane starts with (core-plugin-editor.md, F-023, F-114). Pure; `lib.rs` turns the
//! result into a `create-pane` submission.

use crate::template;

/// The prompt template; `{{v0}}` is the line, `{{v1}}` the file, `{{v2}}` the one-based line.
const GOAL_FROM_LINE: &str = include_str!("../templates/goal-from-line.hbs");

/// True when the line, ignoring leading white space, starts with `TODO`. A `TODO` later in the
/// line (after code or a comment marker) does not count.
pub fn is_todo_line(text: &str) -> bool {
    text.trim_start().starts_with("TODO")
}

/// The default prompt for the agent: the line text verbatim, the file it came from and the
/// one-based line number.
pub fn default_prompt(line_text: &str, file: &str, zero_based_line: u32) -> String {
    let line_number = (zero_based_line + 1).to_string();
    template::render(
        GOAL_FROM_LINE,
        &[("v0", line_text), ("v1", file), ("v2", &line_number)],
    )
}

/// The key that keeps a double click on one line from creating two agents.
pub fn dedup_key(buffer: u64, line: u32) -> String {
    format!("editor-todo:{buffer}:{line}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// REQ: F-023, F-114
    #[test]
    fn only_a_line_that_starts_with_todo_counts() {
        assert!(is_todo_line("TODO write docs"));
        assert!(is_todo_line("    TODO indented"));
        assert!(!is_todo_line("let x = 1; // TODO later"));
        assert!(!is_todo_line(""));
    }

    /// REQ: F-023, F-114
    #[test]
    fn the_prompt_names_the_file_and_the_one_based_line() {
        let prompt = default_prompt("TODO a", "src/x.rs", 1);
        assert!(prompt.starts_with("/goal please do the following task: \"TODO a\""));
        assert!(prompt.contains("src/x.rs at line 2"));
    }

    /// REQ: F-023
    #[test]
    fn line_text_is_not_interpreted() {
        let prompt = default_prompt("TODO {{v1}} <b>", "f.rs", 0);
        assert!(prompt.contains("TODO {{v1}} <b>"));
    }
}

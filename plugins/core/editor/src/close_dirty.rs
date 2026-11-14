//! The question shown when an editor with unsaved changes is closed.

use crate::template;

const CLOSE_DIRTY_EDITOR: &str = include_str!("../templates/close-dirty-editor.hbs");

/// The confirmation text for the buffer called `name`.
pub fn question(name: &str) -> String {
    template::render(CLOSE_DIRTY_EDITOR, &[("name", name)])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// REQ: F-019
    #[test]
    fn the_question_names_the_buffer() {
        assert_eq!(
            question("a.rs"),
            "\"a.rs\" has unsaved changes. Close anyway?"
        );
    }
}

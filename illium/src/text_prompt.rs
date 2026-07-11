//! Pure editing logic for a single-line text input, shared by every modal
//! that collects a name or command line (`Mode::Rename`, `Mode::CommandPrompt`
//! in `app.rs`). Kept free of `ratatui`/`crossterm` types beyond `KeyCode` so
//! it can be unit-tested without a terminal, and so rendering (`modal.rs`)
//! stays a pure function of this state.

use crossterm::event::KeyCode;

/// A text buffer plus a cursor position (in `char`s, not bytes -- UTF-8
/// input must never split a multi-byte character).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextPromptState {
    pub buf: String,
    pub cursor: usize,
}

impl TextPromptState {
    /// Starts with the cursor at the end of `initial`, matching the natural
    /// place to keep typing when renaming something that already has a name.
    pub fn new(initial: impl Into<String>) -> Self {
        let buf = initial.into();
        let cursor = buf.chars().count();
        Self { buf, cursor }
    }

    /// Byte offset of `self.cursor` within `self.buf`, for slicing.
    fn byte_index(&self) -> usize {
        self.buf
            .char_indices()
            .nth(self.cursor)
            .map_or(self.buf.len(), |(index, _)| index)
    }

    fn insert_char(&mut self, c: char) {
        let index = self.byte_index();
        self.buf.insert(index, c);
        self.cursor += 1;
    }

    fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        self.cursor -= 1;
        let index = self.byte_index();
        self.buf.remove(index);
    }

    fn delete_forward(&mut self) {
        if self.cursor >= self.buf.chars().count() {
            return;
        }
        let index = self.byte_index();
        self.buf.remove(index);
    }

    fn move_left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    fn move_right(&mut self) {
        self.cursor = (self.cursor + 1).min(self.buf.chars().count());
    }

    fn move_home(&mut self) {
        self.cursor = 0;
    }

    fn move_end(&mut self) {
        self.cursor = self.buf.chars().count();
    }
}

/// What a key event did to a prompt in progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptOutcome {
    /// The buffer changed (or the cursor moved); stay in the prompt.
    Continue,
    /// `Enter` -- the caller should act on `state.buf` and close the prompt.
    Commit,
    /// `Esc` -- the caller should discard `state.buf` and close the prompt.
    Cancel,
}

/// Applies one key press to `state`. Shared by every text-prompt modal so
/// cursor movement/editing behaves identically everywhere it's used.
pub fn handle_key(state: &mut TextPromptState, code: KeyCode) -> PromptOutcome {
    match code {
        KeyCode::Enter => return PromptOutcome::Commit,
        KeyCode::Esc => return PromptOutcome::Cancel,
        KeyCode::Backspace => state.backspace(),
        KeyCode::Delete => state.delete_forward(),
        KeyCode::Left => state.move_left(),
        KeyCode::Right => state.move_right(),
        KeyCode::Home => state.move_home(),
        KeyCode::End => state.move_end(),
        KeyCode::Char(c) => state.insert_char(c),
        _ => {}
    }
    PromptOutcome::Continue
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_places_cursor_at_end() {
        let state = TextPromptState::new("hello");
        assert_eq!(state.buf, "hello");
        assert_eq!(state.cursor, 5);
    }

    #[test]
    fn typing_inserts_at_cursor() {
        let mut state = TextPromptState::new("");
        handle_key(&mut state, KeyCode::Char('a'));
        handle_key(&mut state, KeyCode::Char('b'));
        assert_eq!(state.buf, "ab");
        assert_eq!(state.cursor, 2);
    }

    #[test]
    fn backspace_removes_before_cursor() {
        let mut state = TextPromptState::new("ab");
        handle_key(&mut state, KeyCode::Backspace);
        assert_eq!(state.buf, "a");
        assert_eq!(state.cursor, 1);
    }

    #[test]
    fn backspace_at_start_is_a_no_op() {
        let mut state = TextPromptState::new("");
        handle_key(&mut state, KeyCode::Backspace);
        assert_eq!(state.buf, "");
        assert_eq!(state.cursor, 0);
    }

    #[test]
    fn left_then_insert_splices_mid_buffer() {
        let mut state = TextPromptState::new("ac");
        handle_key(&mut state, KeyCode::Left);
        handle_key(&mut state, KeyCode::Char('b'));
        assert_eq!(state.buf, "abc");
        assert_eq!(state.cursor, 2);
    }

    #[test]
    fn delete_forward_removes_after_cursor() {
        let mut state = TextPromptState::new("abc");
        state.cursor = 1;
        handle_key(&mut state, KeyCode::Delete);
        assert_eq!(state.buf, "ac");
        assert_eq!(state.cursor, 1);
    }

    #[test]
    fn home_and_end_jump_to_bounds() {
        let mut state = TextPromptState::new("abc");
        handle_key(&mut state, KeyCode::Home);
        assert_eq!(state.cursor, 0);
        handle_key(&mut state, KeyCode::End);
        assert_eq!(state.cursor, 3);
    }

    #[test]
    fn multibyte_characters_are_never_split() {
        let mut state = TextPromptState::new("café");
        handle_key(&mut state, KeyCode::Backspace);
        assert_eq!(state.buf, "caf");
    }

    #[test]
    fn enter_commits_and_esc_cancels() {
        let mut state = TextPromptState::new("x");
        assert_eq!(
            handle_key(&mut state, KeyCode::Enter),
            PromptOutcome::Commit
        );
        assert_eq!(handle_key(&mut state, KeyCode::Esc), PromptOutcome::Cancel);
    }
}

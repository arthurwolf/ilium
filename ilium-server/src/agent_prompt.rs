//! Proposal: exact PTY input reconstruction for an agent composer.
//!
//! This is separate from the 4,096-character shell-title tracker. It retains
//! ordinary authored text without a convenience length cap, but never calls
//! terminal-owned edits or an incomplete escape/UTF-8 sequence exact.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnavailableReason {
    HistoryOrCompletion,
    UnsupportedControl,
    InvalidUtf8,
    IncompleteSequence,
    UnattributedInput,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubmittedPrompt {
    pub exact_text: Option<String>,
    pub unavailable_reason: Option<UnavailableReason>,
}

#[derive(Debug, Default)]
pub struct AgentPromptTracker {
    text: String,
    cursor: usize,
    unavailable_reason: Option<UnavailableReason>,
    utf8_pending: Vec<u8>,
    escape_pending: Vec<u8>,
    in_bracketed_paste: bool,
    skip_next_terminator: Option<u8>,
}

impl AgentPromptTracker {
    /// Keep an unattributed or automated edit from being reported as the next
    /// user-authored prompt. Consume its bytes so its Enter closes the opaque
    /// interval, allowing a later fresh submission to be reconstructed.
    pub fn observe_unattributed_written(&mut self, bytes: &[u8]) -> Option<SubmittedPrompt> {
        self.mark_unavailable(UnavailableReason::UnattributedInput);
        self.observe_written(bytes)
    }

    /// Consume bytes only after the PTY write succeeded. The latest Enter in
    /// a batch wins. A returned string is exact for the input Ilium wrote;
    /// it is not proof the provider consumed or accepted that input.
    pub fn observe_written(&mut self, bytes: &[u8]) -> Option<SubmittedPrompt> {
        let mut latest = None;
        for &byte in bytes {
            if let Some(opposite) = self.skip_next_terminator.take() {
                if byte == opposite {
                    continue;
                }
            }
            // An editor control is not the continuation byte of a character.
            // Do not join a UTF-8 prefix from a previous IPC request across it.
            if !self.utf8_pending.is_empty() && !(0x80..=0xbf).contains(&byte) {
                self.mark_unavailable(UnavailableReason::InvalidUtf8);
            }
            if !self.escape_pending.is_empty() {
                self.escape_pending.push(byte);
                if let Some(sequence) = known_escape(&self.escape_pending) {
                    self.escape_pending.clear();
                    self.apply_escape(sequence);
                    continue;
                }
                if is_escape_prefix(&self.escape_pending) {
                    continue;
                }
                self.mark_unavailable(UnavailableReason::UnsupportedControl);
                self.escape_pending.clear();
                // An unknown sequence must not swallow a following Enter.
                if byte != b'\r' && byte != b'\n' {
                    continue;
                }
            }
            if byte == b'\x1b' {
                if !self.utf8_pending.is_empty() {
                    self.mark_unavailable(UnavailableReason::InvalidUtf8);
                    self.utf8_pending.clear();
                }
                self.escape_pending.push(byte);
                continue;
            }
            if self.in_bracketed_paste {
                self.insert_utf8_byte(byte);
                continue;
            }
            match byte {
                b'\r' | b'\n' => {
                    if !self.utf8_pending.is_empty() {
                        self.mark_unavailable(UnavailableReason::IncompleteSequence);
                    }
                    latest = Some(self.commit());
                    self.skip_next_terminator = Some(if byte == b'\r' { b'\n' } else { b'\r' });
                }
                // These controls have provider-owned editing semantics. In
                // particular, Ctrl-U may clear only to the cursor, whose
                // location is unknowable after history or completion.
                b'\x03' | b'\x15' | b'\x04' | b'\x0b' | b'\x17' => {
                    self.mark_unavailable(UnavailableReason::UnsupportedControl)
                }
                b'\x01' => self.move_home(),  // Ctrl-A.
                b'\x05' => self.move_end(),   // Ctrl-E.
                b'\x02' => self.move_left(),  // Ctrl-B.
                b'\x06' => self.move_right(), // Ctrl-F.
                b'\x7f' | b'\x08' => self.backspace(),
                b'\t' => self.mark_unavailable(UnavailableReason::HistoryOrCompletion),
                0..=31 => self.mark_unavailable(UnavailableReason::UnsupportedControl),
                _ => self.insert_utf8_byte(byte),
            }
        }
        latest
    }

    pub fn reset(&mut self) {
        self.text.clear();
        self.cursor = 0;
        self.unavailable_reason = None;
        self.utf8_pending.clear();
        self.escape_pending.clear();
        self.in_bracketed_paste = false;
        self.skip_next_terminator = None;
    }

    fn commit(&mut self) -> SubmittedPrompt {
        let unavailable_reason = self.unavailable_reason;
        let exact_text = unavailable_reason
            .is_none()
            .then(|| std::mem::take(&mut self.text));
        self.reset();
        SubmittedPrompt {
            exact_text,
            unavailable_reason,
        }
    }

    fn mark_unavailable(&mut self, reason: UnavailableReason) {
        if self.unavailable_reason.is_none() {
            self.unavailable_reason = Some(reason);
        }
        self.text.clear();
        self.cursor = 0;
        self.utf8_pending.clear();
    }

    fn insert_utf8_byte(&mut self, byte: u8) {
        if self.unavailable_reason.is_some() {
            return;
        }
        if self.utf8_pending.is_empty() && byte.is_ascii() {
            self.text.insert(self.cursor, char::from(byte));
            self.cursor += 1;
            return;
        }
        self.utf8_pending.push(byte);
        match std::str::from_utf8(&self.utf8_pending) {
            Ok(character) => {
                let character = character.to_owned();
                self.text.insert_str(self.cursor, &character);
                self.cursor += character.len();
                self.utf8_pending.clear();
            }
            Err(error) if error.error_len().is_none() && self.utf8_pending.len() < 4 => {}
            Err(_) => self.mark_unavailable(UnavailableReason::InvalidUtf8),
        }
    }

    fn apply_escape(&mut self, sequence: Escape) {
        if self.in_bracketed_paste {
            if sequence == Escape::PasteEnd {
                self.in_bracketed_paste = false;
            } else {
                self.mark_unavailable(UnavailableReason::UnsupportedControl);
            }
            return;
        }
        match sequence {
            Escape::PasteStart => self.in_bracketed_paste = true,
            Escape::Left => self.move_left(),
            Escape::Right => self.move_right(),
            Escape::Home => self.move_home(),
            Escape::End => self.move_end(),
            Escape::Delete => self.delete_at_cursor(),
            Escape::History | Escape::PasteEnd => {
                self.mark_unavailable(UnavailableReason::HistoryOrCompletion);
            }
        }
    }

    fn move_left(&mut self) {
        if !self.cursor_edit_is_unambiguous() {
            return;
        }
        self.cursor = self.text[..self.cursor]
            .char_indices()
            .next_back()
            .map_or(0, |(index, _)| index);
    }

    fn move_right(&mut self) {
        if !self.cursor_edit_is_unambiguous() {
            return;
        }
        if self.cursor < self.text.len() {
            self.cursor += self.text[self.cursor..]
                .chars()
                .next()
                .map_or(0, char::len_utf8);
        }
    }

    fn move_home(&mut self) {
        if self.cursor_edit_is_unambiguous() {
            self.cursor = 0;
        }
    }

    fn move_end(&mut self) {
        if self.cursor_edit_is_unambiguous() {
            self.cursor = self.text.len();
        }
    }

    fn cursor_edit_is_unambiguous(&mut self) -> bool {
        // Providers may navigate grapheme clusters and multiline visual rows
        // rather than Unicode scalars and the entire string.
        if !self.text.is_ascii() || self.text.contains('\n') {
            self.mark_unavailable(UnavailableReason::UnsupportedControl);
            return false;
        }
        true
    }

    fn backspace(&mut self) {
        if self.cursor > 0 {
            // The check may clear the uncertain composer. Returning before
            // removal also keeps a mixed Unicode/ASCII suffix from panicking.
            if !self.cursor_edit_is_unambiguous() {
                return;
            }
            if !self.text[..self.cursor]
                .chars()
                .next_back()
                .is_some_and(|character| character.is_ascii())
            {
                self.mark_unavailable(UnavailableReason::UnsupportedControl);
                return;
            }
            self.move_left();
            self.text.remove(self.cursor);
        }
    }

    fn delete_at_cursor(&mut self) {
        if self.cursor < self.text.len() {
            if !self.text[self.cursor..]
                .chars()
                .next()
                .is_some_and(|character| character.is_ascii())
            {
                self.mark_unavailable(UnavailableReason::UnsupportedControl);
                return;
            }
            self.text.remove(self.cursor);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Escape {
    PasteStart,
    PasteEnd,
    Left,
    Right,
    Home,
    End,
    Delete,
    History,
}

const ESCAPES: &[(&[u8], Escape)] = &[
    (b"\x1b[200~", Escape::PasteStart),
    (b"\x1b[201~", Escape::PasteEnd),
    (b"\x1b[D", Escape::Left),
    (b"\x1b[C", Escape::Right),
    (b"\x1b[H", Escape::Home),
    (b"\x1b[F", Escape::End),
    (b"\x1b[1~", Escape::Home),
    (b"\x1b[4~", Escape::End),
    (b"\x1b[3~", Escape::Delete),
    (b"\x1b[A", Escape::History),
    (b"\x1b[B", Escape::History),
];

fn known_escape(bytes: &[u8]) -> Option<Escape> {
    ESCAPES
        .iter()
        .find_map(|(sequence, escape)| (*sequence == bytes).then_some(*escape))
}

fn is_escape_prefix(bytes: &[u8]) -> bool {
    ESCAPES
        .iter()
        .any(|(sequence, _)| sequence.starts_with(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_unicode_multiline_paste_and_trailing_spaces_survive_split_chunks() {
        let mut tracker = AgentPromptTracker::default();
        let prompt = format!("{}\ncafé  ", "authored text ".repeat(800));
        let bytes = format!("\x1b[200~{prompt}\x1b[201~").into_bytes();
        for chunk in bytes.chunks(3) {
            assert!(tracker.observe_written(chunk).is_none());
        }
        assert_eq!(
            tracker
                .observe_written(b"\r")
                .unwrap()
                .exact_text
                .as_deref(),
            Some(prompt.as_str())
        );
    }

    #[test]
    fn split_utf8_character_and_crlf_are_one_submission() {
        let mut tracker = AgentPromptTracker::default();
        assert!(tracker.observe_written(&[0xC3]).is_none());
        assert_eq!(
            tracker
                .observe_written(&[0xA9, b'\r'])
                .unwrap()
                .exact_text
                .as_deref(),
            Some("é")
        );
        assert!(tracker.observe_written(b"\n").is_none());
    }

    #[test]
    fn terminal_owned_history_remains_unavailable_after_ctrl_u() {
        let mut tracker = AgentPromptTracker::default();
        tracker.observe_written(b"old\x1b[A");
        assert_eq!(
            tracker.observe_written(b"\r").unwrap().unavailable_reason,
            Some(UnavailableReason::HistoryOrCompletion)
        );
        tracker.observe_written(b"old\t\x15new  ");
        assert!(tracker.observe_written(b"\r").unwrap().exact_text.is_none());
        assert_eq!(
            tracker
                .observe_written(b"next  \r")
                .unwrap()
                .exact_text
                .as_deref(),
            Some("next  ")
        );
    }

    #[test]
    fn control_between_utf8_chunks_fails_closed() {
        let mut tracker = AgentPromptTracker::default();
        tracker.observe_written(&[0xc3]);
        tracker.observe_written(&[0x01, 0xa9]);
        let submission = tracker.observe_written(b"\r").unwrap();
        assert!(submission.exact_text.is_none());
        assert_eq!(
            submission.unavailable_reason,
            Some(UnavailableReason::InvalidUtf8)
        );
    }

    #[test]
    fn deleting_non_ascii_grapheme_is_not_called_exact() {
        let mut tracker = AgentPromptTracker::default();
        tracker.observe_written("café".as_bytes());
        let submission = tracker.observe_written(b"\x7f\r").unwrap();
        assert!(submission.exact_text.is_none());
    }

    #[test]
    fn backspace_after_mixed_unicode_ascii_fails_closed_without_panicking() {
        let mut tracker = AgentPromptTracker::default();
        tracker.observe_written("éx".as_bytes());
        let submission = tracker.observe_written(b"\x7f\r").unwrap();
        assert!(submission.exact_text.is_none());
        assert_eq!(
            submission.unavailable_reason,
            Some(UnavailableReason::UnsupportedControl)
        );
    }

    #[test]
    fn moving_across_unicode_or_multiline_text_is_unavailable() {
        for text in ["e\u{301}", "a\nb"] {
            let mut tracker = AgentPromptTracker::default();
            tracker.observe_written(format!("\x1b[200~{text}\x1b[201~").as_bytes());
            tracker.observe_written(b"\x1b[D");
            assert!(tracker.observe_written(b"\r").unwrap().exact_text.is_none());
        }
    }

    #[test]
    fn automated_edit_invalidates_current_prompt_only() {
        let mut tracker = AgentPromptTracker::default();
        tracker.observe_written(b"human ");
        tracker.observe_unattributed_written(b"automatic");
        assert_eq!(
            tracker.observe_written(b"\r").unwrap().unavailable_reason,
            Some(UnavailableReason::UnattributedInput)
        );
        assert_eq!(
            tracker
                .observe_written(b"next\r")
                .unwrap()
                .exact_text
                .as_deref(),
            Some("next")
        );
    }
}

//! Best-effort reconstruction of a command line typed into an interactive
//! shell. This module is deliberately pure: PTY ownership, foreground-job
//! checks, and tree updates remain in the server's input handler.

const MAX_TRACKED_COMMAND_CHARS: usize = 4096;
const MAX_TITLE_CHARS: usize = 80;

/// Stateful command-line observer for one shell PTY. It understands the
/// common line-editing bytes ilium currently forwards and fails closed for
/// unknown/history-editing sequences rather than inventing a misleading title.
#[derive(Debug, Default)]
pub struct ShellCommandTracker {
    current_line: Vec<char>,
    cursor: usize,
    opaque_reason: Option<OpaqueInputReason>,
    is_bracketed_paste: bool,
    was_truncated: bool,
}

/// The first terminal-owned editing action that made exact reconstruction
/// impossible. Keeping the specific cause lets session diagnostics distinguish
/// a history-recalled `/clear` from corrupt bytes or an unsupported key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpaqueInputReason {
    Completion,
    HistoryNavigation,
    UnsupportedEscapeSequence,
    UnsupportedControlByte,
    InvalidUtf8,
}

impl OpaqueInputReason {
    /// Plain-language evidence stored in the agent event log.
    pub const fn explanation(self) -> &'static str {
        match self {
            Self::Completion => {
                "shell completion may have changed the submitted line outside ilium's input stream"
            }
            Self::HistoryNavigation => {
                "shell history navigation replaced the line outside ilium's input stream"
            }
            Self::UnsupportedEscapeSequence => {
                "an unsupported terminal escape sequence changed or inspected the line"
            }
            Self::UnsupportedControlByte => {
                "an unsupported control byte made the terminal-owned line state unknowable"
            }
            Self::InvalidUtf8 => "invalid UTF-8 bytes prevented exact input reconstruction",
        }
    }
}

/// Best available reconstruction of one submitted interactive line. `text`
/// is absent when shell history/completion or an unknown control sequence
/// made the terminal-owned editor state unknowable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackedSubmission {
    pub text: Option<String>,
    pub was_truncated: bool,
    pub opaque_reason: Option<OpaqueInputReason>,
}

impl TrackedSubmission {
    /// Returns text only when lifecycle decisions can safely treat it as the
    /// complete submitted line. Bounded/truncated or terminal-owned edits are
    /// diagnostic evidence, never command-classification input.
    pub fn exact_text(&self) -> Option<&str> {
        if self.was_truncated || self.opaque_reason.is_some() {
            return None;
        }
        self.text.as_deref()
    }
}

impl ShellCommandTracker {
    /// Incorporates input that was successfully written to the shell. The
    /// returned title is the latest completed, non-empty command in `bytes`.
    pub fn observe(&mut self, bytes: &[u8]) -> Option<String> {
        self.observe_submission(bytes)
            .and_then(|submission| submission.text)
            .map(|text| normalize_title(&text))
            .filter(|title| !title.is_empty())
    }

    /// Incorporates successfully-written input and returns the latest
    /// semantic submission, preserving full tracked text for debug history.
    pub fn observe_submission(&mut self, bytes: &[u8]) -> Option<TrackedSubmission> {
        let mut latest_submission = None;
        let mut index = 0;

        while index < bytes.len() {
            if self.is_bracketed_paste {
                if bytes[index..].starts_with(b"\x1b[201~") {
                    self.is_bracketed_paste = false;
                    index += 6;
                    continue;
                }
                if let Some((character, consumed)) = decode_utf8_character(&bytes[index..]) {
                    self.insert_character(character);
                    index += consumed;
                } else {
                    self.discard_pending_line(OpaqueInputReason::InvalidUtf8);
                    index += 1;
                }
                continue;
            }
            match bytes[index] {
                b'\r' | b'\n' => {
                    latest_submission = Some(self.commit_current_line());
                    index += 1;
                }
                b'\x7f' | b'\x08' => {
                    self.backspace();
                    index += 1;
                }
                b'\x01' => {
                    self.cursor = 0;
                    index += 1;
                }
                b'\x02' => {
                    self.cursor = self.cursor.saturating_sub(1);
                    index += 1;
                }
                b'\x03' => {
                    self.reset_pending_line();
                    index += 1;
                }
                b'\x04' => {
                    self.delete_at_cursor();
                    index += 1;
                }
                b'\x05' => {
                    self.cursor = self.current_line.len();
                    index += 1;
                }
                b'\x06' => {
                    self.cursor = (self.cursor + 1).min(self.current_line.len());
                    index += 1;
                }
                b'\x0b' => {
                    self.delete_to_end();
                    index += 1;
                }
                b'\x15' => {
                    self.delete_to_start();
                    index += 1;
                }
                b'\x17' => {
                    self.delete_previous_word();
                    index += 1;
                }
                b'\t' => {
                    // Completion may replace or extend the submitted line;
                    // retaining the typed prefix as exact would hide a missed
                    // slash-command transition such as `/cle<Tab>`.
                    self.discard_pending_line(OpaqueInputReason::Completion);
                    index += 1;
                }
                b'\x1b' if bytes[index..].starts_with(b"\x1b[200~") => {
                    self.is_bracketed_paste = true;
                    index += 6;
                }
                b'\x1b' => self.consume_escape(bytes, &mut index),
                byte if byte >= b' ' => {
                    if let Some((character, consumed)) = decode_utf8_character(&bytes[index..]) {
                        self.insert_character(character);
                        index += consumed;
                    } else {
                        self.discard_pending_line(OpaqueInputReason::InvalidUtf8);
                        index += 1;
                    }
                }
                _ => {
                    self.discard_pending_line(OpaqueInputReason::UnsupportedControlByte);
                    index += 1;
                }
            }
        }

        latest_submission
    }

    /// Discards an in-progress command when the shell no longer owns the
    /// foreground PTY or agent detection changes the input semantics.
    pub fn reset_pending_line(&mut self) {
        self.current_line.clear();
        self.cursor = 0;
        self.opaque_reason = None;
        self.is_bracketed_paste = false;
        self.was_truncated = false;
    }

    fn discard_pending_line(&mut self, reason: OpaqueInputReason) {
        self.current_line.clear();
        self.cursor = 0;
        if self.opaque_reason.is_none() {
            self.opaque_reason = Some(reason);
        }
    }

    fn consume_escape(&mut self, bytes: &[u8], index: &mut usize) {
        let remaining = &bytes[*index..];
        let consumed = if remaining.starts_with(b"\x1b[D") {
            self.cursor = self.cursor.saturating_sub(1);
            3
        } else if remaining.starts_with(b"\x1b[C") {
            self.cursor = (self.cursor + 1).min(self.current_line.len());
            3
        } else if remaining.starts_with(b"\x1b[H") || remaining.starts_with(b"\x1b[1~") {
            self.cursor = 0;
            if remaining.starts_with(b"\x1b[1~") {
                4
            } else {
                3
            }
        } else if remaining.starts_with(b"\x1b[F") || remaining.starts_with(b"\x1b[4~") {
            self.cursor = self.current_line.len();
            if remaining.starts_with(b"\x1b[4~") {
                4
            } else {
                3
            }
        } else if remaining.starts_with(b"\x1b[3~") {
            self.delete_at_cursor();
            4
        } else if remaining.starts_with(b"\x1b[A") || remaining.starts_with(b"\x1b[B") {
            // Shell history content is intentionally opaque to raw input.
            self.discard_pending_line(OpaqueInputReason::HistoryNavigation);
            3
        } else {
            self.discard_pending_line(OpaqueInputReason::UnsupportedEscapeSequence);
            escape_sequence_len(remaining)
        };
        *index += consumed;
    }

    fn insert_character(&mut self, character: char) {
        if self.opaque_reason.is_some() {
            return;
        }
        if self.current_line.len() >= MAX_TRACKED_COMMAND_CHARS {
            self.was_truncated = true;
            return;
        }
        self.current_line.insert(self.cursor, character);
        self.cursor += 1;
    }

    fn backspace(&mut self) {
        if self.opaque_reason.is_none() && self.cursor > 0 {
            self.cursor -= 1;
            self.current_line.remove(self.cursor);
        }
    }

    fn delete_at_cursor(&mut self) {
        if self.opaque_reason.is_none() && self.cursor < self.current_line.len() {
            self.current_line.remove(self.cursor);
        }
    }

    fn delete_to_end(&mut self) {
        if self.opaque_reason.is_none() {
            self.current_line.truncate(self.cursor);
        }
    }

    fn delete_to_start(&mut self) {
        if self.opaque_reason.is_none() {
            self.current_line.drain(..self.cursor);
            self.cursor = 0;
        }
    }

    fn delete_previous_word(&mut self) {
        if self.opaque_reason.is_some() {
            return;
        }
        while self.cursor > 0 && self.current_line[self.cursor - 1].is_whitespace() {
            self.backspace();
        }
        while self.cursor > 0 && !self.current_line[self.cursor - 1].is_whitespace() {
            self.backspace();
        }
    }

    fn commit_current_line(&mut self) -> TrackedSubmission {
        let submission = TrackedSubmission {
            text: self
                .opaque_reason
                .is_none()
                .then(|| self.current_line.iter().collect::<String>()),
            was_truncated: self.was_truncated,
            opaque_reason: self.opaque_reason,
        };
        self.reset_pending_line();
        submission
    }
}

fn decode_utf8_character(bytes: &[u8]) -> Option<(char, usize)> {
    for length in 1..=bytes.len().min(4) {
        let Ok(text) = std::str::from_utf8(&bytes[..length]) else {
            continue;
        };
        if text.chars().count() == 1 {
            return text.chars().next().map(|character| (character, length));
        }
    }
    None
}

fn escape_sequence_len(bytes: &[u8]) -> usize {
    if bytes.len() < 2 || bytes[1] != b'[' {
        return bytes.len().min(2);
    }
    bytes
        .iter()
        .enumerate()
        .skip(2)
        .find_map(|(index, byte)| (b'@'..=b'~').contains(byte).then_some(index + 1))
        .unwrap_or(bytes.len())
}

fn normalize_title(command: &str) -> String {
    let compact = command.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = compact.chars();
    let mut title = String::new();
    for (count, ch) in (&mut chars).enumerate() {
        if count >= MAX_TITLE_CHARS {
            title.push('…');
            break;
        }
        title.push(ch);
    }
    title
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commits_the_latest_complete_command_across_input_frames() {
        let mut tracker = ShellCommandTracker::default();
        assert_eq!(tracker.observe(b"git "), None);
        assert_eq!(tracker.observe(b"status\r"), Some("git status".to_string()));
        assert_eq!(
            tracker.observe(b"cargo test\r"),
            Some("cargo test".to_string())
        );
    }

    #[test]
    fn applies_common_line_edits_before_committing() {
        let mut tracker = ShellCommandTracker::default();
        assert_eq!(
            tracker.observe(b"git staus\x1b[D\x1b[Dt\r"),
            Some("git status".to_string())
        );
    }

    #[test]
    fn cancellation_and_history_navigation_never_publish_a_guess() {
        let mut tracker = ShellCommandTracker::default();
        assert_eq!(tracker.observe(b"secret\x03\r"), None);
        assert_eq!(tracker.observe(b"\x1b[A\r"), None);
        assert_eq!(
            tracker.observe(b"echo safe\r"),
            Some("echo safe".to_string())
        );
    }

    #[test]
    fn normalizes_whitespace_and_truncates_by_characters() {
        let mut tracker = ShellCommandTracker::default();
        assert_eq!(
            tracker.observe("  echo   café  \r".as_bytes()),
            Some("echo café".to_string())
        );

        let long_command = format!("{}\r", "é".repeat(MAX_TITLE_CHARS + 1));
        let title = tracker
            .observe(long_command.as_bytes())
            .expect("command title");
        assert_eq!(title, format!("{}…", "é".repeat(MAX_TITLE_CHARS)));
    }

    #[test]
    fn bracketed_multiline_paste_is_one_complete_debug_submission() {
        let mut tracker = ShellCommandTracker::default();
        let submission = tracker
            .observe_submission(b"\x1b[200~first line\nsecond line\x1b[201~\r")
            .expect("trailing enter submits the pasted text");

        assert_eq!(submission.text.as_deref(), Some("first line\nsecond line"));
        assert!(!submission.was_truncated);
        assert_eq!(submission.opaque_reason, None);
    }

    #[test]
    fn debug_submission_marks_content_truncated_at_the_bounded_limit() {
        let mut tracker = ShellCommandTracker::default();
        let mut input = "x".repeat(MAX_TRACKED_COMMAND_CHARS + 17).into_bytes();
        input.push(b'\r');

        let submission = tracker
            .observe_submission(&input)
            .expect("enter commits the bounded submission");

        assert_eq!(
            submission.text.as_deref().map(str::len),
            Some(MAX_TRACKED_COMMAND_CHARS)
        );
        assert!(submission.was_truncated);
        assert_eq!(submission.opaque_reason, None);
        assert_eq!(submission.exact_text(), None);
    }

    #[test]
    fn debug_submission_preserves_unknown_state_instead_of_inventing_text() {
        let mut tracker = ShellCommandTracker::default();
        let submission = tracker
            .observe_submission(b"typed prefix\x1b[A\r")
            .expect("enter still represents a submission boundary");

        assert_eq!(submission.text, None);
        assert!(!submission.was_truncated);
        assert_eq!(
            submission.opaque_reason,
            Some(OpaqueInputReason::HistoryNavigation)
        );
    }

    #[test]
    fn completion_explains_why_a_slash_command_cannot_be_classified() {
        let mut tracker = ShellCommandTracker::default();
        let submission = tracker
            .observe_submission(b"/cle\t\r")
            .expect("enter still represents a submission boundary");

        assert_eq!(submission.text, None);
        assert_eq!(
            submission.opaque_reason,
            Some(OpaqueInputReason::Completion)
        );
        assert_eq!(submission.exact_text(), None);
        assert!(submission
            .opaque_reason
            .is_some_and(|reason| reason.explanation().contains("completion")));
    }
}

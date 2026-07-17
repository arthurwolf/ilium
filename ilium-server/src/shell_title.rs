//! Best-effort reconstruction of a command line typed into an interactive
//! shell. This module is deliberately pure: PTY ownership, foreground-job
//! checks, and tree updates remain in the server's input handler.

const MAX_TRACKED_COMMAND_CHARS: usize = 4096;
const MAX_TITLE_CHARS: usize = 80;

/// Stateful command-line observer for one shell PTY. It understands the
/// common line-editing bytes ilium currently forwards and fails closed for
/// unknown/history-editing sequences rather than inventing a misleading title.
#[derive(Debug)]
pub struct ShellCommandTracker {
    current_line: Vec<char>,
    cursor: usize,
    is_current_line_known: bool,
}

impl Default for ShellCommandTracker {
    fn default() -> Self {
        Self {
            current_line: Vec::new(),
            cursor: 0,
            is_current_line_known: true,
        }
    }
}

impl ShellCommandTracker {
    /// Incorporates input that was successfully written to the shell. The
    /// returned title is the latest completed, non-empty command in `bytes`.
    pub fn observe(&mut self, bytes: &[u8]) -> Option<String> {
        let mut latest_title = None;
        let mut index = 0;

        while index < bytes.len() {
            match bytes[index] {
                b'\r' | b'\n' => {
                    if let Some(title) = self.commit_current_line() {
                        latest_title = Some(title);
                    }
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
                    // Completion changes the shell-owned line in ways raw
                    // input cannot observe. Keep the directly typed prefix.
                    index += 1;
                }
                b'\x1b' => self.consume_escape(bytes, &mut index),
                byte if byte >= b' ' => {
                    if let Some((character, consumed)) = decode_utf8_character(&bytes[index..]) {
                        self.insert_character(character);
                        index += consumed;
                    } else {
                        self.discard_pending_line();
                        index += 1;
                    }
                }
                _ => {
                    self.discard_pending_line();
                    index += 1;
                }
            }
        }

        latest_title
    }

    /// Discards an in-progress command when the shell no longer owns the
    /// foreground PTY or agent detection changes the input semantics.
    pub fn reset_pending_line(&mut self) {
        self.current_line.clear();
        self.cursor = 0;
        self.is_current_line_known = true;
    }

    fn discard_pending_line(&mut self) {
        self.current_line.clear();
        self.cursor = 0;
        self.is_current_line_known = false;
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
            self.discard_pending_line();
            3
        } else {
            self.discard_pending_line();
            escape_sequence_len(remaining)
        };
        *index += consumed;
    }

    fn insert_character(&mut self, character: char) {
        if !self.is_current_line_known || self.current_line.len() >= MAX_TRACKED_COMMAND_CHARS {
            return;
        }
        self.current_line.insert(self.cursor, character);
        self.cursor += 1;
    }

    fn backspace(&mut self) {
        if self.is_current_line_known && self.cursor > 0 {
            self.cursor -= 1;
            self.current_line.remove(self.cursor);
        }
    }

    fn delete_at_cursor(&mut self) {
        if self.is_current_line_known && self.cursor < self.current_line.len() {
            self.current_line.remove(self.cursor);
        }
    }

    fn delete_to_end(&mut self) {
        if self.is_current_line_known {
            self.current_line.truncate(self.cursor);
        }
    }

    fn delete_to_start(&mut self) {
        if self.is_current_line_known {
            self.current_line.drain(..self.cursor);
            self.cursor = 0;
        }
    }

    fn delete_previous_word(&mut self) {
        if !self.is_current_line_known {
            return;
        }
        while self.cursor > 0 && self.current_line[self.cursor - 1].is_whitespace() {
            self.backspace();
        }
        while self.cursor > 0 && !self.current_line[self.cursor - 1].is_whitespace() {
            self.backspace();
        }
    }

    fn commit_current_line(&mut self) -> Option<String> {
        let title = self
            .is_current_line_known
            .then(|| normalize_title(self.current_line.iter().collect::<String>().as_str()))
            .filter(|title| !title.is_empty());
        self.reset_pending_line();
        title
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
}

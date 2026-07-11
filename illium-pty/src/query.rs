//! Answers the small set of terminal capability-query escape sequences that
//! well-behaved terminal apps send and then block waiting for a reply.
//!
//! `vt100::Parser` is a pure output *parser* -- it has no notion of writing
//! anything back to the child, so without this, any query the child sends
//! (e.g. crossterm's own `supports_keyboard_enhancement`, which every
//! ratatui-style TUI calls at startup) goes unanswered. The child then sits
//! blocked on its own read-with-timeout (crossterm's is 2 seconds) before
//! giving up and continuing without the feature -- exactly the "prints its
//! banner, then pauses for a couple of seconds before the real UI appears"
//! symptom this fixes. A real terminal emulator answers these immediately,
//! which is why the same command run directly in e.g. Ghostty or the VS
//! Code terminal doesn't show the pause.

use std::io::Write;
use std::sync::{Arc, Mutex};

/// Callback target installed on a [`vt100::Parser`] to answer capability
/// queries by writing replies back down the pty's write half.
pub(crate) struct TerminalQueryResponder {
    // Same channel `PtySession::write` writes down; a reply here lands on
    // the child's stdin exactly like a keystroke would.
    pub(crate) reply_writer: Arc<Mutex<Box<dyn Write + Send>>>,
}

impl TerminalQueryResponder {
    fn reply(&mut self, bytes: &[u8]) {
        // Best-effort: a query reply that fails to send is no worse than
        // the unanswered query this responder exists to fix, and a
        // poisoned lock here would mean the pane is already being torn
        // down.
        if let Ok(mut writer) = self.reply_writer.lock() {
            let _ = writer.write_all(bytes);
            let _ = writer.flush();
        }
    }
}

impl vt100::Callbacks for TerminalQueryResponder {
    fn unhandled_csi(
        &mut self,
        screen: &mut vt100::Screen,
        i1: Option<u8>,
        _i2: Option<u8>,
        params: &[&[u16]],
        c: char,
    ) {
        match (i1, c) {
            // Kitty keyboard-protocol support query (`CSI ? u`). illium
            // never forwards the kitty protocol into the child pty (the
            // key-encoding layer always emits legacy ANSI sequences), so
            // the truthful answer is "no progressive-enhancement flags
            // active".
            (Some(b'?'), 'u') => self.reply(b"\x1b[?0u"),
            // Primary Device Attributes (`CSI c` / `CSI 0 c`). Answer as a
            // basic VT100-with-AVO terminal, matching what tmux/screen
            // report for a nested session.
            (None, 'c') => self.reply(b"\x1b[?1;2c"),
            // Device Status Report (`CSI 5n`): "terminal OK".
            (None, 'n') if params.first().and_then(|p| p.first()) == Some(&5) => {
                self.reply(b"\x1b[0n");
            }
            // Cursor Position Report (`CSI 6n`), answered with the real
            // cursor position so apps that poll it for cursor-relative
            // rendering (e.g. bracketed prompts) get a correct reply
            // instead of hanging.
            (None, 'n') if params.first().and_then(|p| p.first()) == Some(&6) => {
                let (row, col) = screen.cursor_position();
                self.reply(format!("\x1b[{};{}R", row + 1, col + 1).as_bytes());
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// In-memory `Write` sink standing in for the pty writer, so these
    /// tests exercise `TerminalQueryResponder` directly through
    /// `vt100::Parser::process` without spawning a real pty or child
    /// process.
    #[derive(Clone, Default)]
    struct RecordingSink(Arc<Mutex<Vec<u8>>>);

    impl Write for RecordingSink {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn parser_with_recording_responder() -> (vt100::Parser<TerminalQueryResponder>, RecordingSink) {
        let sink = RecordingSink::default();
        let reply_writer: Arc<Mutex<Box<dyn Write + Send>>> =
            Arc::new(Mutex::new(Box::new(sink.clone())));
        let parser =
            vt100::Parser::new_with_callbacks(24, 80, 0, TerminalQueryResponder { reply_writer });
        (parser, sink)
    }

    #[test]
    fn primary_device_attributes_query_gets_a_reply() {
        let (mut parser, sink) = parser_with_recording_responder();
        parser.process(b"\x1b[c");
        assert_eq!(sink.0.lock().unwrap().as_slice(), b"\x1b[?1;2c");
    }

    #[test]
    fn kitty_keyboard_enhancement_query_reports_no_flags() {
        // This is the exact query crossterm's `supports_keyboard_enhancement`
        // sends (see `terminal/sys/unix.rs` in the crossterm crate); without
        // a reply the child blocks on it for a 2-second timeout, which is
        // the pause this fix eliminates.
        let (mut parser, sink) = parser_with_recording_responder();
        parser.process(b"\x1b[?u\x1b[c");
        assert_eq!(sink.0.lock().unwrap().as_slice(), b"\x1b[?0u\x1b[?1;2c");
    }

    #[test]
    fn cursor_position_report_reflects_the_real_cursor() {
        let (mut parser, sink) = parser_with_recording_responder();
        // Move the cursor to row 3, col 5 (1-indexed CUP), then ask for it.
        parser.process(b"\x1b[3;5H\x1b[6n");
        assert_eq!(sink.0.lock().unwrap().as_slice(), b"\x1b[3;5R");
    }
}

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

/// Upper bound on how many capability-query replies [`TerminalQueryResponder`]
/// will write per PTY read chunk (see [`TerminalQueryResponder::reset_reply_budget`]).
///
/// `unhandled_csi` runs synchronously inside `vt100::Parser::process`, which
/// `PtySession`'s reader thread calls while holding the parser's write lock.
/// Without a cap, a child that floods its own stdout with query escape
/// sequences (e.g. repeated `CSI 6n`) without draining its stdin between
/// them -- buggy, or attacker-controlled output the agent is merely relaying
/// -- could accumulate enough synchronous `write_all` calls in one chunk to
/// fill the kernel's pty input buffer and block. That block would happen
/// while the parser's write lock is held, stalling every other consumer of
/// it (screen reads, resize, teardown) along with the reader thread itself.
/// Sixty-four is far above what any legitimate startup probe sends (one or
/// two queries), so this never engages in normal use.
const MAX_REPLIES_PER_READ_CHUNK: u32 = 64;

/// Callback target installed on a [`vt100::Parser`] to answer capability
/// queries by writing replies back down the pty's write half.
pub(crate) struct TerminalQueryResponder {
    // Same channel `PtySession::write` writes down; a reply here lands on
    // the child's stdin exactly like a keystroke would.
    reply_writer: Arc<Mutex<Box<dyn Write + Send>>>,
    replies_sent_in_current_chunk: u32,
}

impl TerminalQueryResponder {
    pub(crate) fn new(reply_writer: Arc<Mutex<Box<dyn Write + Send>>>) -> Self {
        Self {
            reply_writer,
            replies_sent_in_current_chunk: 0,
        }
    }

    /// Resets the per-chunk reply budget. The owning `PtySession` calls this
    /// immediately before each `Parser::process` call so the budget tracks
    /// "replies within this one read chunk" rather than "replies over the
    /// pane's whole lifetime" -- a burst is capped, but the responder still
    /// answers normally on the next chunk.
    pub(crate) fn reset_reply_budget(&mut self) {
        self.replies_sent_in_current_chunk = 0;
    }

    fn reply(&mut self, bytes: &[u8]) {
        if self.replies_sent_in_current_chunk >= MAX_REPLIES_PER_READ_CHUNK {
            return;
        }
        self.replies_sent_in_current_chunk += 1;
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
            // Kitty keyboard-protocol support query (`CSI ? u`). ilium never
            // forwards the kitty protocol into the child pty (the
            // key-encoding layer always emits legacy ANSI sequences), so
            // this is deliberately left unanswered rather than answered with
            // an empty flag set: per the kitty spec, crossterm's
            // `supports_keyboard_enhancement` (see
            // `query_keyboard_enhancement_flags_raw` in crossterm's
            // `terminal/sys/unix.rs`) treats *any* `CSI ? <flags> u` reply --
            // including `?0u` -- as "protocol supported", regardless of
            // which flags are set. Answering here would make a
            // crossterm/ratatui child believe kitty-encoded input is
            // available when it never will be. The child's query still
            // gets resolved promptly and truthfully: it also sends `CSI c`
            // (Primary Device Attributes, handled below) as part of the
            // same detection probe, and receiving that reply alone without
            // a keyboard-enhancement reply is what crossterm treats as "not
            // supported" -- so leaving this arm unhandled doesn't reintroduce
            // the startup pause this module exists to fix.
            //
            // Primary Device Attributes (`CSI c` / `CSI 0 c`). Answer as a
            // basic VT100-with-AVO terminal, matching what tmux/screen
            // report for a nested session. Per the DEC spec DA1 only takes
            // parameter 0 (or none); a nonzero parameter is not a valid
            // attributes request, and answering it anyway would push
            // unsolicited reply bytes onto the child's stdin, so those are
            // ignored.
            (None, 'c')
                if params
                    .first()
                    .and_then(|p| p.first())
                    .is_none_or(|v| *v == 0) =>
            {
                self.reply(b"\x1b[?1;2c");
            }
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
                self.reply(
                    format!("\x1b[{};{}R", row.saturating_add(1), col.saturating_add(1)).as_bytes(),
                );
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
            vt100::Parser::new_with_callbacks(24, 80, 0, TerminalQueryResponder::new(reply_writer));
        (parser, sink)
    }

    #[test]
    fn primary_device_attributes_query_gets_a_reply() {
        let (mut parser, sink) = parser_with_recording_responder();
        parser.process(b"\x1b[c");
        assert_eq!(sink.0.lock().unwrap().as_slice(), b"\x1b[?1;2c");
    }

    #[test]
    fn kitty_keyboard_enhancement_query_is_left_unanswered() {
        // This is the exact query pair crossterm's
        // `supports_keyboard_enhancement` sends (see
        // `query_keyboard_enhancement_flags_raw` in crossterm's
        // `terminal/sys/unix.rs`): `CSI ?u` followed by `CSI c`. Crossterm
        // treats *any* reply to the first query -- even an empty flag set --
        // as "kitty protocol supported", so it must go unanswered here.
        // Answering only the Primary Device Attributes query still resolves
        // crossterm's probe promptly (no 2-second timeout) while reporting
        // "not supported", which is the truthful answer for this pty.
        let (mut parser, sink) = parser_with_recording_responder();
        parser.process(b"\x1b[?u\x1b[c");
        assert_eq!(sink.0.lock().unwrap().as_slice(), b"\x1b[?1;2c");
    }

    #[test]
    fn cursor_position_report_reflects_the_real_cursor() {
        let (mut parser, sink) = parser_with_recording_responder();
        // Move the cursor to row 3, col 5 (1-indexed CUP), then ask for it.
        parser.process(b"\x1b[3;5H\x1b[6n");
        assert_eq!(sink.0.lock().unwrap().as_slice(), b"\x1b[3;5R");
    }

    #[test]
    fn device_attributes_with_nonzero_parameter_is_left_unanswered() {
        // DA1 only takes parameter 0 (or none). `CSI 5 c` is not a valid
        // attributes request, so answering it would push unsolicited reply
        // bytes onto the child's stdin; the explicit `CSI 0 c` form must
        // still get the normal reply.
        let (mut parser, sink) = parser_with_recording_responder();
        parser.process(b"\x1b[5c");
        assert!(sink.0.lock().unwrap().is_empty());

        parser.process(b"\x1b[0c");
        assert_eq!(sink.0.lock().unwrap().as_slice(), b"\x1b[?1;2c");
    }

    #[test]
    fn private_marker_variants_are_left_unanswered() {
        // `vte` (vt100's own CSI parser) routes the `?` private-marker byte
        // into the `i1` callback parameter, distinct from `None`. This test
        // pins that down: `CSI ? c` (a private-marker PDA variant) and
        // `CSI ? 6 n` (DECXCPR, the extended cursor-position report) must
        // NOT fall into the plain `(None, 'c')` / `(None, 'n')` arms above,
        // because DECXCPR expects a differently-shaped reply
        // (`CSI ? row ; col ; page R`) than the plain CPR this responder
        // sends. Answering with the wrong shape would be worse than not
        // answering at all, so both must produce no reply.
        let (mut parser, sink) = parser_with_recording_responder();
        parser.process(b"\x1b[?c\x1b[?6n");
        assert!(sink.0.lock().unwrap().is_empty());
    }

    #[test]
    fn reply_flood_within_one_chunk_is_capped() {
        // A child that emits far more query escape sequences than any
        // legitimate startup probe would, all within a single `process()`
        // call, must not get a synchronous reply for every single one --
        // see `MAX_REPLIES_PER_READ_CHUNK`'s doc comment for why an
        // unbounded reply count here is a liveness hazard, not just noise.
        let (mut parser, sink) = parser_with_recording_responder();
        let flood = b"\x1b[c".repeat(MAX_REPLIES_PER_READ_CHUNK as usize + 50);
        parser.process(&flood);
        let replies_written = sink.0.lock().unwrap().len() / b"\x1b[?1;2c".len();
        assert_eq!(replies_written, MAX_REPLIES_PER_READ_CHUNK as usize);
    }

    #[test]
    fn reply_budget_resets_for_the_next_chunk() {
        // Mirrors what `PtySession`'s reader thread does: reset the budget
        // immediately before each `process()` call. A capped burst on one
        // chunk must not permanently silence the responder.
        let (mut parser, sink) = parser_with_recording_responder();
        let flood = b"\x1b[c".repeat(MAX_REPLIES_PER_READ_CHUNK as usize + 10);
        parser.process(&flood);
        let bytes_after_flood = sink.0.lock().unwrap().len();

        parser.callbacks_mut().reset_reply_budget();
        parser.process(b"\x1b[c");

        let total_bytes = sink.0.lock().unwrap().len();
        assert_eq!(total_bytes - bytes_after_flood, b"\x1b[?1;2c".len());
    }
}

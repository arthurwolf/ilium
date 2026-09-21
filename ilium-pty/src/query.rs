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
//!
//! This module performs no I/O of its own. [`TerminalQueryResponder`] only
//! *composes* reply bytes into a buffer while `vt100::Parser::process` runs;
//! the owning `PtySession` reader thread drains that buffer with
//! [`TerminalQueryResponder::take_pending_replies`] once it has released the
//! parser lock, and it is that thread -- not the parser callback -- that
//! writes the bytes down the pty. Keeping the write outside the callback is
//! what guarantees a child whose stdin buffer is full can never stall the
//! parser lock (and with it every screen read, resize, and teardown) behind
//! a blocking `write_all`.

/// Upper bound on how many capability-query replies [`TerminalQueryResponder`]
/// will queue per PTY read chunk (see
/// [`TerminalQueryResponder::take_pending_replies`]).
///
/// `unhandled_csi` runs synchronously inside `vt100::Parser::process`, so
/// every queued reply is memory held while that one chunk is parsed. Without
/// a cap, a child that floods its own stdout with query escape sequences
/// (e.g. repeated `CSI 6n`) -- buggy, or attacker-controlled output the agent
/// is merely relaying -- would have every one of them answered: a single
/// 64 KiB read chunk of nothing but `CSI 6n` is over twenty thousand queries,
/// so the queue (and the unsolicited input pushed at the child afterwards)
/// would grow with the flood instead of staying bounded. Sixty-four is far
/// above what any legitimate startup probe sends (one or two queries), so
/// this never engages in normal use.
const MAX_REPLIES_PER_READ_CHUNK: u32 = 64;

/// Callback target installed on a [`vt100::Parser`] to answer capability
/// queries. It never writes to the pty itself -- it accumulates the reply
/// bytes for the chunk currently being parsed, and the reader thread that
/// drove `process()` sends them on afterwards.
#[derive(Default)]
pub(crate) struct TerminalQueryResponder {
    // Reply bytes composed during the current `Parser::process` call, in the
    // order the queries arrived. Drained (and thereby reset) by
    // `take_pending_replies` once per chunk.
    pending_replies: Vec<u8>,
    replies_queued_in_current_chunk: u32,
}

impl TerminalQueryResponder {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Hands the caller every reply queued while parsing the chunk that just
    /// finished, and resets the per-chunk reply budget along with it.
    ///
    /// The owning `PtySession` calls this immediately after each
    /// `Parser::process` call and writes the returned bytes to the pty's
    /// write half, so a reply lands on the child's stdin exactly like a
    /// keystroke would. Draining *is* the budget reset, which is why the
    /// budget cannot drift out of step with the chunk boundary: a burst is
    /// capped, but the responder still answers normally on the next chunk.
    pub(crate) fn take_pending_replies(&mut self) -> Vec<u8> {
        self.replies_queued_in_current_chunk = 0;
        std::mem::take(&mut self.pending_replies)
    }

    fn queue_reply(&mut self, bytes: &[u8]) {
        if self.replies_queued_in_current_chunk >= MAX_REPLIES_PER_READ_CHUNK {
            return;
        }
        self.replies_queued_in_current_chunk += 1;
        self.pending_replies.extend_from_slice(bytes);
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
                self.queue_reply(b"\x1b[?1;2c");
            }
            // Device Status Report (`CSI 5n`): "terminal OK".
            (None, 'n') if params.first().and_then(|p| p.first()) == Some(&5) => {
                self.queue_reply(b"\x1b[0n");
            }
            // Cursor Position Report (`CSI 6n`), answered with the real
            // cursor position so apps that poll it for cursor-relative
            // rendering (e.g. bracketed prompts) get a correct reply
            // instead of hanging.
            (None, 'n') if params.first().and_then(|p| p.first()) == Some(&6) => {
                let (rows, cols) = screen.size();
                let (row, col) = screen.cursor_position();
                // CPR is 1-indexed and must name a cell that exists. vt100
                // parks the cursor one column *past* the last one while a
                // wrap is pending -- the ordinary state after any line
                // filled to the right edge -- which would otherwise report
                // column `cols + 1` and put the querying app's idea of the
                // cursor off the screen. Real terminals report the last
                // column in that state, so clamp to the screen. `max(1)`
                // keeps a degenerate zero-sized screen from producing the
                // invalid row/column 0.
                let reported_row = row.saturating_add(1).min(rows.max(1));
                let reported_col = col.saturating_add(1).min(cols.max(1));
                self.queue_reply(format!("\x1b[{reported_row};{reported_col}R").as_bytes());
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a parser wired to a real [`TerminalQueryResponder`], so these
    /// tests exercise the responder through `vt100::Parser::process` without
    /// spawning a real pty or child process.
    fn parser_with_responder() -> vt100::Parser<TerminalQueryResponder> {
        vt100::Parser::new_with_callbacks(24, 80, 0, TerminalQueryResponder::new())
    }

    /// Feeds one read chunk through a fresh parser and returns exactly the
    /// bytes `PtySession`'s reader thread would write back to the child.
    fn replies_to(chunk: &[u8]) -> Vec<u8> {
        let mut parser = parser_with_responder();
        parser.process(chunk);
        parser.callbacks_mut().take_pending_replies()
    }

    #[test]
    fn primary_device_attributes_query_gets_a_reply() {
        assert_eq!(replies_to(b"\x1b[c").as_slice(), b"\x1b[?1;2c");
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
        assert_eq!(replies_to(b"\x1b[?u\x1b[c").as_slice(), b"\x1b[?1;2c");
    }

    #[test]
    fn cursor_position_report_reflects_the_real_cursor() {
        // Move the cursor to row 3, col 5 (1-indexed CUP), then ask for it.
        assert_eq!(replies_to(b"\x1b[3;5H\x1b[6n").as_slice(), b"\x1b[3;5R");
    }

    #[test]
    fn cursor_position_report_clamps_a_pending_wrap_to_the_last_column() {
        // Writing at the last column leaves vt100's cursor at column `cols`
        // (0-indexed), one past the last cell, with the wrap deferred until
        // the next printable character. The report must still name a real
        // column -- `80` on this 80-column screen, not `81`.
        assert_eq!(replies_to(b"\x1b[1;80Hx\x1b[6n").as_slice(), b"\x1b[1;80R");
    }

    #[test]
    fn device_attributes_with_nonzero_parameter_is_left_unanswered() {
        // DA1 only takes parameter 0 (or none). `CSI 5 c` is not a valid
        // attributes request, so answering it would push unsolicited reply
        // bytes onto the child's stdin; the explicit `CSI 0 c` form must
        // still get the normal reply.
        assert!(replies_to(b"\x1b[5c").is_empty());
        assert_eq!(replies_to(b"\x1b[0c").as_slice(), b"\x1b[?1;2c");
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
        assert!(replies_to(b"\x1b[?c\x1b[?6n").is_empty());
    }

    #[test]
    fn reply_flood_within_one_chunk_is_capped() {
        // A child that emits far more query escape sequences than any
        // legitimate startup probe would, all within a single `process()`
        // call, must not get a reply queued for every single one -- see
        // `MAX_REPLIES_PER_READ_CHUNK`'s doc comment for why an unbounded
        // reply count here is a resource hazard, not just noise.
        let flood = b"\x1b[c".repeat(MAX_REPLIES_PER_READ_CHUNK as usize + 50);
        let replies = replies_to(&flood);
        assert_eq!(
            replies.len(),
            MAX_REPLIES_PER_READ_CHUNK as usize * b"\x1b[?1;2c".len()
        );
    }

    #[test]
    fn reply_budget_resets_for_the_next_chunk() {
        // Mirrors what `PtySession`'s reader thread does: drain the queued
        // replies after each `process()` call. A capped burst on one chunk
        // must not permanently silence the responder.
        let mut parser = parser_with_responder();
        let flood = b"\x1b[c".repeat(MAX_REPLIES_PER_READ_CHUNK as usize + 10);
        parser.process(&flood);
        assert_eq!(
            parser.callbacks_mut().take_pending_replies().len(),
            MAX_REPLIES_PER_READ_CHUNK as usize * b"\x1b[?1;2c".len()
        );

        parser.process(b"\x1b[c");
        assert_eq!(
            parser.callbacks_mut().take_pending_replies().as_slice(),
            b"\x1b[?1;2c"
        );
    }
}

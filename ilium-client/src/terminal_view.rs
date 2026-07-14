//! Local, read-only render cache for one PTY-backed pane's screen.
//!
//! ilium-client never owns a real PTY (ilium-server does -- see the
//! crate's module docs). What it owns instead is a `vt100::Parser` fed
//! purely from `ServerEvent::ScreenUpdate` byte chunks, so `tui_term`
//! still has a real `vt100::Screen` to render from without this crate
//! needing a second, IPC-specific screen representation (see
//! `ilium_ipc::ServerEvent::ScreenUpdate`'s own doc comment for why the
//! wire carries raw bytes rather than a pre-diffed cell format).
//!
//! Scrollback lives entirely here, client-side, on the same `vt100::Parser`
//! -- not on `ilium-server`'s copy (see `ilium-pty::PtySession::spawn`,
//! which deliberately keeps its own parser's scrollback at zero: the server
//! only ever needs the *current* screen, for agent-activity detection and
//! mouse-protocol negotiation, never a scrolled-back one). `vt100::Screen`
//! already implements a scrollback ring buffer and transparently reflows
//! `cell`/`rows`/`contents` through it via `set_scrollback`, so this crate
//! doesn't need a second, hand-rolled history buffer -- it only needs to
//! turn scrollback on and expose navigation over it. `tui_term::widget::
//! PseudoTerminal` already renders a scrolled-back `Screen` correctly
//! (cursor hidden/adjusted as appropriate), so scrolling here is exactly
//! `Screen::set_scrollback` plus a cached total (see `scrollback_total`).

use std::sync::Arc;

/// Starting geometry for a freshly created pane, before the first real
/// `ResizePane` request (sent once the client knows the pane's actual
/// on-screen content box) corrects it.
pub const DEFAULT_ROWS: u16 = 24;
pub const DEFAULT_COLS: u16 = 80;

/// How many rows of scrolled-off history `vt100` retains per pane. Applies
/// per terminal pane, client-side only -- see the module docs for why the
/// server's own parser keeps none at all.
const SCROLLBACK_LINES: usize = 10_000;
/// The detached server retains this same upper bound per pane. Keeping the
/// raw stream locally lets search cover retained history that is older than
/// the render parser's 10,000-line viewport without giving the client a PTY.
const MAX_SEARCHABLE_HISTORY_BYTES: usize = 32 * 1024 * 1024;

pub struct TerminalView {
    parser: vt100::Parser,
    // `vt100::Screen` exposes the current scroll *offset*
    // (`Screen::scrollback`) but no direct "how many rows have
    // accumulated" accessor -- only `Screen::set_scrollback`'s internal
    // clamp knows that. `refresh_scrollback_total` reads it via that
    // clamp (set to `usize::MAX`, read back, restore) and caches the
    // result here so rendering (`scrollback_total`) stays a cheap `&self`
    // read instead of needing a `&mut Screen` on every frame.
    scrollback_total: usize,
    /// Newest server output chunk represented by `parser`. This turns an
    /// attach-time replay plus the connection's already-queued live updates
    /// into one exactly-once stream.
    last_output_sequence: u64,
    /// Raw retained output used by the workspace search. This deliberately
    /// mirrors server replay bytes rather than attempting to reverse a
    /// rendered `vt100::Screen`, which only keeps a bounded visual window.
    history_bytes: Arc<Vec<u8>>,
    /// A search result can temporarily rebuild the visual parser at an old
    /// point in history. Fresh output is retained but does not silently move
    /// that evidence out from under the user; `scroll_to_bottom` restores the
    /// live parser from `history_bytes`.
    is_history_anchor: bool,
}

impl TerminalView {
    /// Starts a fresh, blank screen at `rows`x`cols` -- the caller should
    /// send `ClientRequest::ResizePane` promptly after creating a pane so
    /// the server-side PTY matches, and this view's own `resize` keeps the
    /// local parser matching whatever the client's own layout computed.
    pub fn new(rows: u16, cols: u16) -> Self {
        Self {
            parser: vt100::Parser::new(rows, cols, SCROLLBACK_LINES),
            scrollback_total: 0,
            last_output_sequence: 0,
            history_bytes: Arc::new(Vec::new()),
            is_history_anchor: false,
        }
    }

    /// Feeds one chunk of raw PTY output bytes (from `ServerEvent::ScreenUpdate`)
    /// into the local parser. If the view is currently scrolled back,
    /// `vt100` itself keeps the scrolled-to content stable (it advances the
    /// scroll offset in step as rows are pushed into scrollback) -- this
    /// only needs to refresh the cached total, not the position.
    pub fn feed(&mut self, bytes: &[u8]) {
        self.append_history(bytes);
        self.parser.process(bytes);
        self.refresh_scrollback_total();
    }

    /// Replaces this freshly-attached view with the server-owned retained
    /// output history. The server supplies a sequence watermark so any live
    /// event queued before the replay is applied can be rejected later.
    pub fn apply_replay(&mut self, bytes: &[u8], through_sequence: u64, _is_complete: bool) {
        let (rows, cols) = self.parser.screen().size();
        self.parser = vt100::Parser::new(rows, cols, SCROLLBACK_LINES);
        self.scrollback_total = 0;
        Arc::make_mut(&mut self.history_bytes).clear();
        self.append_history(bytes);
        self.parser.process(bytes);
        self.refresh_scrollback_total();
        self.last_output_sequence = through_sequence;
        self.is_history_anchor = false;
    }

    /// Applies a live output chunk only when it was not already included in
    /// the attach replay. Output sequence numbers are pane-local and
    /// monotonic for one server lifetime.
    pub fn apply_live_output(&mut self, sequence: u64, bytes: &[u8]) {
        if sequence <= self.last_output_sequence {
            return;
        }
        self.append_history(bytes);
        if !self.is_history_anchor {
            self.parser.process(bytes);
            self.refresh_scrollback_total();
        }
        self.last_output_sequence = sequence;
    }

    /// Resizes the local screen to match a `ResizePane` request just sent
    /// to the server, so the client's own rendering never waits on a round
    /// trip before reflowing. A resize can change how many rows the
    /// scrollback reflows into, so the cached total (and the current
    /// position, which `refresh_scrollback_total` re-clamps) are refreshed
    /// here too.
    pub fn resize(&mut self, rows: u16, cols: u16) {
        self.parser.screen_mut().set_size(rows, cols);
        self.refresh_scrollback_total();
    }

    /// Runs `f` with the current `vt100::Screen`, for rendering via
    /// `tui_term::widget::PseudoTerminal::new(screen)`.
    pub fn with_screen<R>(&self, f: impl FnOnce(&vt100::Screen) -> R) -> R {
        f(self.parser.screen())
    }

    /// Scrolls further back into history by `lines` rows (clamped to the
    /// oldest row available).
    pub fn scroll_up(&mut self, lines: u16) {
        let screen = self.parser.screen_mut();
        let current = screen.scrollback();
        screen.set_scrollback(current.saturating_add(usize::from(lines)));
    }

    /// Scrolls back toward the live tail by `lines` rows (clamped at the
    /// live view, i.e. offset `0`).
    pub fn scroll_down(&mut self, lines: u16) {
        let screen = self.parser.screen_mut();
        let current = screen.scrollback();
        screen.set_scrollback(current.saturating_sub(usize::from(lines)));
    }

    /// Jumps back to the live tail -- called whenever the pane sends fresh
    /// input, matching how an ordinary terminal emulator drops you back to
    /// the prompt the moment you start typing again.
    pub fn scroll_to_bottom(&mut self) {
        if self.is_history_anchor {
            self.rebuild_live_parser();
            self.is_history_anchor = false;
        }
        self.parser.screen_mut().set_scrollback(0);
    }

    /// `true` once the view has scrolled away from the live tail.
    pub fn is_scrolled_back(&self) -> bool {
        self.parser.screen().scrollback() > 0
    }

    /// Current offset into scrollback: `0` at the live tail, increasing
    /// toward the oldest retained row.
    pub fn scrollback_position(&self) -> usize {
        self.parser.screen().scrollback()
    }

    /// Total rows currently retained in scrollback (bounded by
    /// `SCROLLBACK_LINES`), for sizing the scrollbar.
    pub fn scrollback_total(&self) -> usize {
        self.scrollback_total
    }

    /// Returns all output retained for workspace search, including bytes the
    /// visible parser has already rotated out of its render scrollback.
    pub fn searchable_history(&self) -> &[u8] {
        self.history_bytes.as_slice()
    }

    /// Produces an O(1) immutable view of retained output for the search
    /// worker. Later PTY output uses `Arc::make_mut`, so a worker can scan a
    /// stable history snapshot without blocking the interactive event loop.
    pub fn searchable_history_snapshot(&self) -> Arc<Vec<u8>> {
        Arc::clone(&self.history_bytes)
    }

    /// Rebuilds the visible terminal around one raw-output offset found by
    /// workspace search. The matching bytes become the newest visible output,
    /// so the result opens at its actual place rather than merely focusing the
    /// correct pane. The ordinary live view returns on the next input/wheel
    /// journey to the bottom.
    pub fn jump_to_history_byte(&mut self, end_byte: usize) {
        let (rows, cols) = self.parser.screen().size();
        let end_byte = end_byte.min(self.history_bytes.len());
        self.parser = vt100::Parser::new(rows, cols, SCROLLBACK_LINES);
        self.parser.process(&self.history_bytes[..end_byte]);
        self.refresh_scrollback_total();
        self.is_history_anchor = true;
    }

    #[cfg(test)]
    pub fn last_output_sequence(&self) -> u64 {
        self.last_output_sequence
    }

    /// `true` once the pane's foreground app has negotiated an xterm mouse
    /// protocol -- when it has, wheel events belong to that app (it's
    /// asking to receive them), not to this view's own scrollback
    /// navigation. Most agent CLIs (and a plain shell prompt) never
    /// negotiate one, which is exactly the case this feature targets.
    pub fn wants_mouse_protocol(&self) -> bool {
        self.with_screen(|screen| screen.mouse_protocol_mode() != vt100::MouseProtocolMode::None)
    }

    /// Re-derives `scrollback_total` (see the field doc comment) and
    /// re-clamps the current scroll position against it in the same pass,
    /// since `set_scrollback` is the only operation that performs that
    /// clamp.
    fn refresh_scrollback_total(&mut self) {
        let screen = self.parser.screen_mut();
        let current = screen.scrollback();
        screen.set_scrollback(usize::MAX);
        self.scrollback_total = screen.scrollback();
        screen.set_scrollback(current);
    }

    fn append_history(&mut self, bytes: &[u8]) {
        let history_bytes = Arc::make_mut(&mut self.history_bytes);
        history_bytes.extend_from_slice(bytes);
        let excess = history_bytes
            .len()
            .saturating_sub(MAX_SEARCHABLE_HISTORY_BYTES);
        if excess > 0 {
            history_bytes.drain(..excess);
        }
    }

    fn rebuild_live_parser(&mut self) {
        let (rows, cols) = self.parser.screen().size();
        self.parser = vt100::Parser::new(rows, cols, SCROLLBACK_LINES);
        self.parser.process(&self.history_bytes);
        self.refresh_scrollback_total();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feed_updates_the_rendered_screen_text() {
        let mut view = TerminalView::new(4, 20);
        view.feed(b"hello");
        let text = view.with_screen(|screen| screen.contents());
        assert!(text.contains("hello"));
    }

    #[test]
    fn resize_changes_the_screen_dimensions() {
        let mut view = TerminalView::new(4, 20);
        view.resize(10, 40);
        let (rows, cols) = view.with_screen(|screen| screen.size());
        assert_eq!((rows, cols), (10, 40));
    }

    #[test]
    fn resize_truncating_wide_character_keeps_render_parser_usable() {
        let mut view = TerminalView::new(2, 216);
        view.feed("\u{1b}[215G界".as_bytes());

        view.resize(2, 215);
        view.feed(b"\x1b[0Jrender-cache-alive");

        assert_eq!(view.with_screen(|screen| screen.size()), (2, 215));
        assert!(view
            .with_screen(|screen| screen.contents())
            .contains("render-cache-alive"));
    }

    /// Pushes enough `\n`-terminated lines to overflow a small screen so
    /// several rows scroll off into `vt100`'s scrollback buffer.
    fn feed_lines(view: &mut TerminalView, count: usize) {
        for line in 0..count {
            view.feed(format!("line {line}\r\n").as_bytes());
        }
    }

    #[test]
    fn scrolled_off_rows_accumulate_in_scrollback() {
        let mut view = TerminalView::new(4, 20);
        assert_eq!(view.scrollback_total(), 0);
        feed_lines(&mut view, 10);
        assert!(view.scrollback_total() > 0);
    }

    #[test]
    fn top_anchored_scroll_region_accumulates_agent_history() {
        let mut view = TerminalView::new(4, 20);
        // Codex reserves lower rows for its input/status UI and scrolls the
        // transcript through a DECSTBM region anchored at the screen top.
        // Those rows still leave the physical viewport and must remain
        // available to ilium's wheel scrollback.
        view.feed(b"\x1b[1;3rfirst\r\nsecond\r\nthird\r\nfourth\r\n");

        let total = view.scrollback_total();
        assert!(total > 0);
        view.scroll_up(total as u16);
        assert!(view
            .with_screen(|screen| screen.contents())
            .contains("first"));
    }

    #[test]
    fn lower_scroll_region_does_not_pollute_terminal_history() {
        let mut view = TerminalView::new(4, 20);
        // A region below row zero is an application-owned widget. Rotating
        // it must not create fake terminal history entries.
        view.feed(b"\x1b[2;4rfirst\r\nsecond\r\nthird\r\nfourth\r\n");

        assert_eq!(view.scrollback_total(), 0);
    }

    #[test]
    fn scroll_up_and_down_move_the_position_and_clamp_at_both_ends() {
        let mut view = TerminalView::new(4, 20);
        feed_lines(&mut view, 10);
        let total = view.scrollback_total();

        view.scroll_up(3);
        assert_eq!(view.scrollback_position(), 3);
        assert!(view.is_scrolled_back());

        // Scrolling past the oldest row clamps at the total instead of
        // going further.
        view.scroll_up(total as u16 + 5);
        assert_eq!(view.scrollback_position(), total);

        view.scroll_down(total as u16 + 5);
        assert_eq!(view.scrollback_position(), 0);
        assert!(!view.is_scrolled_back());
    }

    #[test]
    fn scroll_to_bottom_resets_to_the_live_tail() {
        let mut view = TerminalView::new(4, 20);
        feed_lines(&mut view, 10);
        view.scroll_up(5);
        assert!(view.is_scrolled_back());

        view.scroll_to_bottom();
        assert_eq!(view.scrollback_position(), 0);
        assert!(!view.is_scrolled_back());
    }

    #[test]
    fn a_fresh_view_reports_no_negotiated_mouse_protocol() {
        let view = TerminalView::new(4, 20);
        assert!(!view.wants_mouse_protocol());
    }

    #[test]
    fn replay_restores_scrollback_and_deduplicates_queued_live_output() {
        let mut original = TerminalView::new(4, 20);
        feed_lines(&mut original, 12);

        let replay = (0..12)
            .map(|line| format!("line {line}\r\n"))
            .collect::<String>();
        let mut reattached = TerminalView::new(4, 20);
        reattached.apply_replay(replay.as_bytes(), 12, true);
        let total_after_replay = reattached.scrollback_total();
        assert!(total_after_replay > 0);

        reattached.apply_live_output(12, b"duplicate\r\n");
        assert_eq!(reattached.scrollback_total(), total_after_replay);
        assert_eq!(reattached.last_output_sequence(), 12);

        reattached.apply_live_output(13, b"new live output\r\n");
        assert_eq!(reattached.last_output_sequence(), 13);
        assert!(reattached
            .with_screen(|screen| screen.contents())
            .contains("new live output"));
    }

    #[test]
    fn search_jump_rebuilds_the_terminal_around_an_old_history_match() {
        let mut view = TerminalView::new(3, 30);
        view.apply_replay(b"first\r\nneedle lives here\r\nlast\r\n", 4, true);
        let end = view
            .searchable_history()
            .windows(b"needle".len())
            .position(|window| window == b"needle")
            .expect("needle in retained history")
            + b"needle".len();

        view.jump_to_history_byte(end);

        assert!(view
            .with_screen(|screen| screen.contents())
            .contains("needle"));
    }
}

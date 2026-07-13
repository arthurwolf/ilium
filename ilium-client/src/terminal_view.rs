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

/// Starting geometry for a freshly created pane, before the first real
/// `ResizePane` request (sent once the client knows the pane's actual
/// on-screen content box) corrects it.
pub const DEFAULT_ROWS: u16 = 24;
pub const DEFAULT_COLS: u16 = 80;

/// How many rows of scrolled-off history `vt100` retains per pane. Applies
/// per terminal pane, client-side only -- see the module docs for why the
/// server's own parser keeps none at all.
const SCROLLBACK_LINES: usize = 10_000;

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
        }
    }

    /// Feeds one chunk of raw PTY output bytes (from `ServerEvent::ScreenUpdate`)
    /// into the local parser. If the view is currently scrolled back,
    /// `vt100` itself keeps the scrolled-to content stable (it advances the
    /// scroll offset in step as rows are pushed into scrollback) -- this
    /// only needs to refresh the cached total, not the position.
    pub fn feed(&mut self, bytes: &[u8]) {
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
        self.feed(bytes);
        self.last_output_sequence = through_sequence;
    }

    /// Applies a live output chunk only when it was not already included in
    /// the attach replay. Output sequence numbers are pane-local and
    /// monotonic for one server lifetime.
    pub fn apply_live_output(&mut self, sequence: u64, bytes: &[u8]) {
        if sequence <= self.last_output_sequence {
            return;
        }
        self.feed(bytes);
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
}

//! Local, read-only render cache for one PTY-backed pane's screen.
//!
//! illium-client never owns a real PTY (illium-server does -- see the
//! crate's module docs). What it owns instead is a `vt100::Parser` fed
//! purely from `ServerEvent::ScreenUpdate` byte chunks, so `tui_term`
//! still has a real `vt100::Screen` to render from without this crate
//! needing a second, IPC-specific screen representation (see
//! `illium_ipc::ServerEvent::ScreenUpdate`'s own doc comment for why the
//! wire carries raw bytes rather than a pre-diffed cell format).

/// Starting geometry for a freshly created pane, before the first real
/// `ResizePane` request (sent once the client knows the pane's actual
/// on-screen content box) corrects it.
pub const DEFAULT_ROWS: u16 = 24;
pub const DEFAULT_COLS: u16 = 80;

pub struct TerminalView {
    parser: vt100::Parser,
}

impl TerminalView {
    /// Starts a fresh, blank screen at `rows`x`cols` -- the caller should
    /// send `ClientRequest::ResizePane` promptly after creating a pane so
    /// the server-side PTY matches, and this view's own `resize` keeps the
    /// local parser matching whatever the client's own layout computed.
    pub fn new(rows: u16, cols: u16) -> Self {
        // No scrollback kept locally: illium-server is the only side that
        // ever needs a durable transcript of a pane's output, and a fresh
        // attach starts from an empty screen until the server's own
        // `ScreenUpdate` stream repaints it.
        Self {
            parser: vt100::Parser::new(rows, cols, 0),
        }
    }

    /// Feeds one chunk of raw PTY output bytes (from `ServerEvent::ScreenUpdate`)
    /// into the local parser.
    pub fn feed(&mut self, bytes: &[u8]) {
        self.parser.process(bytes);
    }

    /// Resizes the local screen to match a `ResizePane` request just sent
    /// to the server, so the client's own rendering never waits on a round
    /// trip before reflowing.
    pub fn resize(&mut self, rows: u16, cols: u16) {
        self.parser.screen_mut().set_size(rows, cols);
    }

    /// Runs `f` with the current `vt100::Screen`, for rendering via
    /// `tui_term::widget::PseudoTerminal::new(screen)`.
    pub fn with_screen<R>(&self, f: impl FnOnce(&vt100::Screen) -> R) -> R {
        f(self.parser.screen())
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
}

/// A parser for terminal output which produces an in-memory representation of
/// the terminal contents.
pub struct Parser<CB: crate::callbacks::Callbacks = ()> {
    // vte without its std feature uses a fixed OSC buffer. Oversized terminal
    // metadata is truncated until its terminator, never accumulated unboundedly.
    // Keep the fixed buffer on the heap: moving parsers through pane startup
    // must not multiply 64 KiB stack frames in debug builds.
    parser: Box<vte::Parser<65536>>,
    screen: crate::perform::WrappedScreen<CB>,
}

impl Parser {
    /// Creates a new terminal parser of the given size and with the given
    /// amount of scrollback.
    #[must_use]
    pub fn new(rows: u16, cols: u16, scrollback_len: usize) -> Self {
        Self {
            parser: Box::new(vte::Parser::new_with_size()),
            screen: crate::perform::WrappedScreen::new(rows, cols, scrollback_len),
        }
    }
}

impl<CB: crate::callbacks::Callbacks> Parser<CB> {
    /// Creates a new terminal parser of the given size and with the given
    /// amount of scrollback. Terminal events will be reported via method
    /// calls on the provided [`Callbacks`](crate::callbacks::Callbacks)
    /// implementation.
    pub fn new_with_callbacks(rows: u16, cols: u16, scrollback_len: usize, callbacks: CB) -> Self {
        Self {
            parser: Box::new(vte::Parser::new_with_size()),
            screen: crate::perform::WrappedScreen::new_with_callbacks(
                rows,
                cols,
                scrollback_len,
                callbacks,
            ),
        }
    }

    /// Processes the contents of the given byte string, and updates the
    /// in-memory terminal state.
    pub fn process(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.screen, bytes);
    }

    /// Returns retained parser and terminal storage bytes, including the fixed
    /// OSC buffer and inline callback state. Allocator metadata and allocations
    /// owned internally by user callbacks are excluded.
    #[must_use]
    pub fn allocation_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            .saturating_add(std::mem::size_of_val(self.parser.as_ref()))
            .saturating_add(self.screen.screen.allocation_bytes())
            .saturating_sub(std::mem::size_of::<crate::Screen>())
    }

    /// Returns a reference to a [`Screen`](crate::Screen) object containing
    /// the terminal state.
    #[must_use]
    pub fn screen(&self) -> &crate::Screen {
        &self.screen.screen
    }

    /// Returns a mutable reference to a [`Screen`](crate::Screen) object
    /// containing the terminal state.
    #[must_use]
    pub fn screen_mut(&mut self) -> &mut crate::Screen {
        &mut self.screen.screen
    }

    /// Returns a reference to the [`Callbacks`](crate::callbacks::Callbacks)
    /// state object passed into the constructor.
    pub fn callbacks(&self) -> &CB {
        &self.screen.callbacks
    }

    /// Returns a mutable reference to the
    /// [`Callbacks`](crate::callbacks::Callbacks) state object passed into
    /// the constructor.
    pub fn callbacks_mut(&mut self) -> &mut CB {
        &mut self.screen.callbacks
    }
}

impl Default for Parser {
    /// Returns a parser with dimensions 80x24 and no scrollback.
    fn default() -> Self {
        Self::new(24, 80, 0)
    }
}

impl std::io::Write for Parser {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.process(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn boxed_control_buffer_is_charged_without_inflating_parser_stack_size() {
        let parser = super::Parser::new(1, 1, 0);
        assert!(std::mem::size_of_val(&parser) < 4096);
        assert!(parser.allocation_bytes() >= 65536 + parser.screen().allocation_bytes());
    }

    #[test]
    fn parser_metadata_capacity_does_not_inflate_owner_stack_frames() {
        assert!(std::mem::size_of::<super::Parser>() < 4096);
    }
    use super::Parser;

    #[test]
    fn retention_changes_preserve_live_alternate_and_partial_sequences() {
        let mut parser = Parser::new(3, 20, 100);
        for line in 0..20 {
            parser.process(format!("history {line}\r\n").as_bytes());
        }
        let primary = parser.screen().contents();
        let primary_cursor = parser.screen().cursor_position();
        parser.process(b"\x1b[?1h\x1b[?1049halt");
        let alternate = parser.screen().contents();
        let alternate_cursor = parser.screen().cursor_position();
        parser.screen_mut().set_scrollback_limit(2);
        assert!(parser.screen().alternate_screen());
        assert!(parser.screen().application_cursor());
        assert_eq!(parser.screen().contents(), alternate);
        assert_eq!(parser.screen().cursor_position(), alternate_cursor);
        parser.process(b"\x1b[?1049l");
        assert_eq!(parser.screen().contents(), primary);
        assert_eq!(parser.screen().cursor_position(), primary_cursor);
        parser.screen_mut().set_scrollback(usize::MAX);
        assert_eq!(parser.screen().scrollback(), 2);
        parser.screen_mut().set_scrollback(0);

        // Retention must never replace the parser, even in the middle of a CSI
        // or multibyte character arriving across output-frame boundaries.
        parser.process(b"\x1b[2;");
        parser.screen_mut().trim_scrollback(0);
        parser.process(b"3H\xe2\x82");
        parser.screen_mut().set_scrollback_limit(4);
        parser.process(b"\xac");
        assert_eq!(parser.screen().cell(1, 2).unwrap().contents(), "€");
        assert_eq!(parser.screen().scrollback_limit(), 4);
        for _ in 0..20 {
            parser.process(b"\r\nnext");
        }
        parser.screen_mut().set_scrollback(usize::MAX);
        assert_eq!(parser.screen().scrollback(), 4);
    }

    #[test]
    fn visible_history_retention_preserves_window_for_every_offset() {
        for offset in [0, 1, 2, 3, 4, 10, usize::MAX] {
            let mut parser = Parser::new(3, 12, 100);
            for line in 0..30 {
                parser.process(format!("\x1b[31mrow {line}\r\n").as_bytes());
            }
            parser.screen_mut().set_scrollback(offset);
            let visible = parser.screen().contents_formatted();
            let cursor = parser.screen().cursor_position();
            let before = parser.screen().allocation_bytes();
            parser.screen_mut().retain_visible_scrollback();
            assert_eq!(parser.screen().contents_formatted(), visible);
            assert_eq!(parser.screen().cursor_position(), cursor);
            assert!(parser.screen().allocation_bytes() < before);
            assert!(parser.screen().scrollback() <= 3);
            assert_eq!(parser.screen().scrollback_limit(), 100);
        }
    }

    #[test]
    fn resizing_before_first_alternate_entry_keeps_pending_storage_dormant() {
        let mut parser = Parser::new(100, 100, 0);
        let original = parser.screen().allocation_bytes();
        for (rows, columns) in [(3, 10), (2, 50), (1, 1)] {
            parser.screen_mut().set_size(rows, columns);
            let current = parser.screen().allocation_bytes();
            let pending = parser.screen().pending_screen_allocation_bytes();
            assert!(pending > 0);
            assert!(current < original);
            let fresh = Parser::new(rows, columns, 0);
            assert_eq!(current, fresh.screen().allocation_bytes());
            assert_eq!(pending, fresh.screen().pending_screen_allocation_bytes());
        }
        let before = parser.screen().allocation_bytes();
        let pending = parser.screen().pending_screen_allocation_bytes();
        parser.process(b"\x1b[?1049h");
        assert_eq!(parser.screen().allocation_bytes(), before + pending);
        assert_eq!(parser.screen().pending_screen_allocation_bytes(), 0);
        parser.process(b"x");
        assert_eq!(parser.screen().contents(), "x");
    }

    #[test]
    fn alternate_screen_pending_allocation_is_exact_and_charged_once() {
        let mut parser = Parser::new(3, 10, 0);
        let initial = parser.screen().allocation_bytes();
        let pending = parser.screen().pending_screen_allocation_bytes();
        assert!(pending > 0);
        parser.process(b"\x1b[?1049");
        assert_eq!(parser.screen().pending_screen_allocation_bytes(), pending);
        parser.process(b"h");
        assert_eq!(parser.screen().allocation_bytes(), initial + pending);
        assert_eq!(parser.screen().pending_screen_allocation_bytes(), 0);
        parser.process(b"\x1b[?1049l\x1b[?1049h");
        assert_eq!(parser.screen().allocation_bytes(), initial + pending);
        assert_eq!(parser.screen().pending_screen_allocation_bytes(), 0);
    }

    #[test]
    fn bounded_insert_preserves_wide_glyphs_and_clears_split_glyphs() {
        let mut parser = Parser::new(2, 10, 0);
        parser.process("a界b\x1b[1;2H\x1b[@".as_bytes());
        assert_eq!(parser.screen().cell(0, 2).unwrap().contents(), "界");
        assert!(parser.screen().cell(0, 3).unwrap().is_wide_continuation());
        parser.process(b"\x1b[1;4H\x1b[@");
        assert!(!parser.screen().cell(0, 2).unwrap().is_wide());
        assert!(!parser.screen().cell(0, 4).unwrap().is_wide_continuation());
    }

    #[test]
    fn maximum_csi_counts_are_clamped_without_overallocating_live_rows() {
        for command in [b'@', b'L', b'M', b'S', b'T'] {
            let mut huge = Parser::new(3, 10, 2);
            let mut bounded = Parser::new(3, 10, 2);
            huge.process(b"one\r\ntwo\r\nthree\x1b[H");
            bounded.process(b"one\r\ntwo\r\nthree\x1b[H");
            let count = if command == b'@' { 10 } else { 3 };
            huge.process(format!("\x1b[65535{}", char::from(command)).as_bytes());
            bounded.process(format!("\x1b[{count}{}", char::from(command)).as_bytes());
            assert_eq!(huge.screen().contents(), bounded.screen().contents());
            assert_eq!(huge.allocation_bytes(), bounded.allocation_bytes());
            huge.process(b"\x1b[Husable");
            assert!(huge.screen().contents().starts_with("usable"));
        }
    }

    #[test]
    fn allocation_accounting_releases_resize_high_water_marks() {
        let mut parser = Parser::new(200, 10, 10);
        parser.process(b"\x1b[?1049hother\x1b[?1049l");
        parser.screen_mut().set_size(3, 500);
        parser.screen_mut().trim_scrollback(0);
        let mut fresh = Parser::new(3, 500, 10);
        fresh.process(b"\x1b[?1049hother\x1b[?1049l");
        fresh.screen_mut().trim_scrollback(0);
        assert_eq!(parser.allocation_bytes(), fresh.allocation_bytes());
        parser.screen_mut().set_size(3, 10);
        parser.screen_mut().trim_scrollback(0);
        assert!(parser.allocation_bytes() < fresh.allocation_bytes());
        let before_osc = parser.allocation_bytes();
        parser.process(b"\x1b]2;");
        for _ in 0..256 {
            parser.process(&[b'x'; 4096]);
        }
        assert_eq!(parser.allocation_bytes(), before_osc);
    }

    #[test]
    fn oversized_unterminated_osc_is_bounded_and_recovers_on_terminator() {
        #[derive(Default)]
        struct Titles {
            length: usize,
        }
        impl crate::Callbacks for Titles {
            fn set_window_title(&mut self, _: &mut crate::Screen, title: &[u8]) {
                self.length = title.len();
            }
        }
        let mut parser = Parser::new_with_callbacks(3, 20, 10, Titles::default());
        parser.process(b"before\x1b]2;");
        let block = [b'x'; 4096];
        for _ in 0..256 {
            parser.process(&block);
        }
        parser.screen_mut().trim_scrollback(0);
        parser.process(b"\x1b\\after");
        assert!(parser.callbacks().length <= 65536);
        assert!(parser.callbacks().length > 0);
        assert_eq!(parser.screen().contents(), "beforeafter");
    }

    #[test]
    fn erase_display_mode_three_purges_scrollback_but_keeps_visible_rows() {
        let mut parser = Parser::new(3, 20, 100);
        for line in 0..8 {
            parser.process(format!("line {line}\r\n").as_bytes());
        }

        parser.screen_mut().set_scrollback(usize::MAX);
        assert!(parser.screen().scrollback() > 0);
        parser.screen_mut().set_scrollback(0);
        let visible_before_purge = parser.screen().contents();

        parser.process(b"\x1b[3J");

        parser.screen_mut().set_scrollback(usize::MAX);
        assert_eq!(parser.screen().scrollback(), 0);
        assert_eq!(parser.screen().contents(), visible_before_purge);
    }
}

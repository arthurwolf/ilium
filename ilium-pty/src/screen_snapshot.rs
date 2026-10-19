//! Serializes a live `vt100` screen into bytes that rebuild it in a fresh parser of the same size.
//!
//! `Screen::state_formatted` covers visible cells, attributes and input modes. It does not cover
//! the alternate-screen flag, the DECSTBM scroll region, origin mode (DECOM) or the cursor saved by
//! DECSC, so those are appended here. Order matters: DECSTBM and origin-mode changes move the
//! cursor, so the saved state is set first and the live cursor is restored last.

/// Escape bytes that rebuild `screen` in a fresh parser with the same size.
///
/// Known limit: with origin mode on and the cursor pending a wrap at the right edge, the
/// pending wrap is not restored; the cursor position itself is.
pub(crate) fn snapshot_bytes(screen: &vt100::Screen) -> Vec<u8> {
    let mut bytes = Vec::new();
    if screen.alternate_screen() {
        bytes.extend_from_slice(b"\x1b[?1049h");
    }
    bytes.extend(screen.state_formatted());

    let (rows, cols) = screen.size();
    let (top, bottom) = screen.scroll_region();
    if (top, bottom) != (0, rows.saturating_sub(1)) {
        bytes.extend(format!("\x1b[{};{}r", top + 1, bottom + 1).into_bytes());
    }

    let (saved_row, saved_col) = screen.saved_cursor_position();
    let saved_origin = screen.saved_origin_mode();
    let saved_relative_row = if saved_origin {
        saved_row.saturating_sub(top)
    } else {
        saved_row
    };
    bytes.extend_from_slice(origin_mode_sequence(saved_origin));
    bytes.extend(format!("\x1b[{};{}H", saved_relative_row + 1, saved_col + 1).into_bytes());
    bytes.extend_from_slice(b"\x1b7");

    let (cursor_row, cursor_col) = screen.cursor_position();
    let origin = screen.origin_mode();
    bytes.extend_from_slice(origin_mode_sequence(origin));
    if origin {
        // The library's cursor writer uses absolute rows, which are wrong under origin mode.
        let relative_row = cursor_row.saturating_sub(top);
        let relative_col = cursor_col.min(cols.saturating_sub(1));
        bytes.extend(format!("\x1b[{};{}H", relative_row + 1, relative_col + 1).into_bytes());
    } else {
        bytes.extend(screen.cursor_state_formatted());
    }
    bytes
}

fn origin_mode_sequence(on: bool) -> &'static [u8] {
    if on {
        b"\x1b[?6h"
    } else {
        b"\x1b[?6l"
    }
}

#[cfg(test)]
mod tests {
    use super::snapshot_bytes;

    /// The observable state a rebuilt screen must match exactly.
    fn observable(parser: &vt100::Parser) -> String {
        let screen = parser.screen();
        let (row, col) = screen.cursor_position();
        let (rows, cols) = screen.size();
        let wrapped: String = (0..rows)
            .map(|row_index| if screen.row_wrapped(row_index) { 'w' } else { '.' })
            .collect();
        format!(
            "contents={:?}\ncursor=({row},{col}) hide={} alt={} keypad={} appcursor={} paste={} mouse={:?}/{:?}\nsize={rows}x{cols} wrapped={wrapped} scroll_region={:?} origin={} saved_cursor={:?} saved_origin={}",
            screen.contents_formatted(),
            screen.hide_cursor(),
            screen.alternate_screen(),
            screen.application_keypad(),
            screen.application_cursor(),
            screen.bracketed_paste(),
            screen.mouse_protocol_mode(),
            screen.mouse_protocol_encoding(),
            screen.scroll_region(),
            screen.origin_mode(),
            screen.saved_cursor_position(),
            screen.saved_origin_mode(),
        )
    }

    /// Feeds `stream`, rebuilds from its snapshot, then feeds `after` to both and compares.
    fn assert_round_trip(name: &str, stream: &[u8], after: &[u8], rows: u16, cols: u16) {
        let mut live = vt100::Parser::new(rows, cols, 0);
        live.process(stream);
        let mut rebuilt = vt100::Parser::new(rows, cols, 0);
        rebuilt.process(&snapshot_bytes(live.screen()));
        assert_eq!(observable(&live), observable(&rebuilt), "[{name}] at snapshot");
        live.process(after);
        rebuilt.process(after);
        assert_eq!(observable(&live), observable(&rebuilt), "[{name}] after continuation");
    }

    #[test]
    fn plain_text_colours_and_attributes_round_trip() {
        assert_round_trip(
            "plain_sgr",
            b"\x1b[31mred\x1b[0m \x1b[1;4mbold-under\x1b[0m \x1b[7minv\x1b[0m\r\nnext \x1b[38;5;208morange\x1b[0m \x1b[38;2;10;20;30mtrue\x1b[0m",
            b"\r\n\x1b[3mitalic\x1b[0m end",
            10,
            40,
        );
    }

    #[test]
    fn wrapped_lines_round_trip() {
        assert_round_trip(
            "wrapped_long_line",
            b"0123456789abcdefghijklmnopqrstuvwxyz0123456789",
            b"!more",
            6,
            10,
        );
    }

    #[test]
    fn wide_characters_at_the_edge_round_trip() {
        assert_round_trip(
            "wide_and_emoji_edge",
            "漢字テスト 🙂 中文中文中文中文中文".as_bytes(),
            "\r\nlast 字".as_bytes(),
            4,
            12,
        );
    }

    #[test]
    fn active_alternate_screen_round_trips() {
        assert_round_trip(
            "alternate_screen_active",
            b"main-screen-text\x1b[?1049h\x1b[2J\x1b[Halt-screen-app",
            b"\x1b[3;1Hmore",
            8,
            30,
        );
    }

    #[test]
    fn left_alternate_screen_round_trips() {
        assert_round_trip(
            "alternate_screen_left",
            b"main\x1b[?1049h\x1b[Halt\x1b[?1049l\r\nback",
            b"!",
            8,
            30,
        );
    }

    #[test]
    fn input_modes_round_trip() {
        assert_round_trip(
            "input_modes",
            b"\x1b[?1h\x1b=\x1b[?2004h\x1b[?1002h\x1b[?1006h\x1b[?25l",
            b"\x1b[?25h\x1b[?1000l",
            6,
            30,
        );
    }

    #[test]
    fn scroll_region_round_trips() {
        assert_round_trip(
            "scroll_region",
            b"\x1b[2;5r\x1b[5;1Hline-a\r\nline-b\r\nline-c\r\nline-d\r\nline-e\r\nline-f",
            b"\r\nNEXT",
            8,
            20,
        );
    }

    #[test]
    fn saved_cursor_round_trips() {
        assert_round_trip(
            "saved_cursor",
            b"\x1b[3;7Hsaved\x1b7\x1b[1;1Hmoved",
            b"\x1b8X",
            6,
            20,
        );
    }

    #[test]
    fn tab_stops_round_trip() {
        assert_round_trip("tab_stops", b"a\tb\x1bH\x1b[2gc\tdone", b"\tZ", 4, 40);
    }

    #[test]
    fn origin_mode_round_trips() {
        assert_round_trip(
            "origin_mode",
            b"\x1b[3;6r\x1b[?6h\x1b[1;1Hinside",
            b"\x1b[2;1Hnext",
            10,
            20,
        );
    }

    #[test]
    fn pending_wrap_at_the_right_edge_round_trips() {
        assert_round_trip(
            "cursor_past_end_pending_wrap",
            b"12345678901234567890",
            b"x",
            3,
            20,
        );
    }
}

//! Visible-screen text rebuilt from retained terminal output bytes, without a
//! live pty. Automation outside the server (for example `ilium recap`) uses
//! this to read what a pane showed from the bytes the server replays on attach.

/// Screen geometry used when no live pane size is known. Replayed output that
/// depends on a different width wraps approximately; the text stays bounded.
pub const REPLAY_SCREEN_ROWS: u16 = 60;
pub const REPLAY_SCREEN_COLUMNS: u16 = 200;

/// Parses `bytes` into a screen of the given size and returns its visible text,
/// one line per row, trailing blank rows removed.
///
/// The parser has no scrollback and no query responder: replayed capability
/// queries are ignored instead of answered, so nothing is written back to any
/// process.
pub fn screen_text_from_replay(bytes: &[u8], rows: u16, columns: u16) -> String {
    let mut parser = vt100::Parser::new(rows, columns, 0);
    parser.process(bytes);
    let contents = parser.screen().contents();
    let mut lines: Vec<&str> = contents.lines().map(str::trim_end).collect();
    while lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replayed_text_keeps_visible_lines_and_drops_trailing_blank_rows() {
        let text = screen_text_from_replay(b"first line\r\nsecond line\r\n", 10, 40);
        assert_eq!(text, "first line\nsecond line");
    }

    #[test]
    fn cursor_movement_overwrites_earlier_cells() {
        let text = screen_text_from_replay(b"abcdef\rXY", 4, 20);
        assert_eq!(text, "XYcdef");
    }

    #[test]
    fn empty_replay_yields_empty_text() {
        assert_eq!(screen_text_from_replay(b"", 4, 20), "");
    }
}

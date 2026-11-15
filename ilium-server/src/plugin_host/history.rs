//! Scrollback text of a terminal pane for plugins (`PaneRead::history`, foundation contract
//! section 17.4).
//!
//! The pane owner hands out its live screen read-only, and the terminal emulator needs a write
//! to show a scrolled-back view. The retained output journal is raw PTY bytes, so the lines are
//! rebuilt on a private emulator: the retained bytes are fed to a fresh parser of the pane's
//! size, and the rows that scrolled off its top are read back oldest first. The live parser is
//! never touched.

/// Most scrollback lines one call rebuilds. A plugin asks for at most this many whatever it
/// passes to `read-screen`.
pub(crate) const MAX_HISTORY_LINES: usize = 100_000;

/// The last `wanted` lines that scrolled off the top of a pane of `rows` by `columns` cells,
/// oldest first, from the retained output `bytes`. The visible screen is not included.
pub(crate) fn scrollback_lines(
    bytes: &[u8],
    rows: u16,
    columns: u16,
    wanted: usize,
) -> Vec<String> {
    let wanted = wanted.min(MAX_HISTORY_LINES);
    if wanted == 0 || bytes.is_empty() {
        return Vec::new();
    }
    let rows = rows.max(1);
    let columns = columns.max(1);
    let mut parser = vt100::Parser::new(rows, columns, MAX_HISTORY_LINES);
    parser.process(bytes);
    let screen = parser.screen_mut();
    // The offset clamps to the rows that really scrolled off, which tells how many there are.
    screen.set_scrollback(usize::MAX);
    let available = screen.scrollback();
    let count = available.min(wanted);
    let mut lines = Vec::with_capacity(count);
    // At offset `offset` the top row shows the scrollback line `available - offset`, so walking
    // the offset down from `count` reads the newest `count` lines oldest first.
    for offset in (1..=count).rev() {
        screen.set_scrollback(offset);
        lines.push(screen.rows(0, columns).next().unwrap_or_default());
    }
    screen.set_scrollback(0);
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn numbered(count: usize) -> Vec<u8> {
        (1..=count)
            .map(|number| format!("line {number}\r\n"))
            .collect::<String>()
            .into_bytes()
    }

    #[test]
    fn rows_that_scrolled_off_the_top_come_back_oldest_first() {
        // Ten lines on a four-row screen: the last three lines and the cursor row stay visible.
        let lines = scrollback_lines(&numbered(10), 4, 20, 100);
        assert_eq!(lines.first().map(String::as_str), Some("line 1"));
        assert_eq!(lines.last().map(String::as_str), Some("line 7"));
        assert_eq!(lines.len(), 7);
    }

    #[test]
    fn only_the_newest_requested_lines_are_returned() {
        let lines = scrollback_lines(&numbered(10), 4, 20, 3);
        assert_eq!(lines, vec!["line 5", "line 6", "line 7"]);
    }

    #[test]
    fn a_screenful_of_output_has_no_scrollback() {
        assert!(scrollback_lines(&numbered(2), 4, 20, 100).is_empty());
    }

    #[test]
    fn nothing_requested_or_retained_is_empty() {
        assert!(scrollback_lines(&numbered(10), 4, 20, 0).is_empty());
        assert!(scrollback_lines(&[], 4, 20, 10).is_empty());
    }

    #[test]
    fn escape_sequences_never_reach_the_text() {
        let mut bytes = b"\x1b[31mred line\x1b[0m\r\n".to_vec();
        bytes.extend(numbered(6));
        let lines = scrollback_lines(&bytes, 4, 20, 100);
        assert_eq!(lines.first().map(String::as_str), Some("red line"));
    }

    #[test]
    fn a_degenerate_size_is_widened_instead_of_failing() {
        assert!(scrollback_lines(&numbered(5), 0, 0, 10).len() <= 10);
    }
}

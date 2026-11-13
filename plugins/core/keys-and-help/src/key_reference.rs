//! Pure text rules of the key reference (port of the width helpers of `ilium-client/src/help.rs`).
//!
//! The panel rows are cut to the cell width of the view. A cut text ends with `…`, and the width is
//! measured in terminal cells, so a wide character is never split and never overruns the column.

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// The ellipsis that marks an omission, one cell wide.
pub const ELLIPSIS: &str = "…";

/// Cuts `text` to at most `max_width` cells. A text that fits is returned unchanged; a longer one
/// keeps as many whole characters as fit before the ellipsis; a zero width gives an empty string.
pub fn truncate_to_width(text: &str, max_width: usize) -> String {
    if UnicodeWidthStr::width(text) <= max_width {
        return text.to_owned();
    }
    if max_width == 0 {
        return String::new();
    }
    let content_width = max_width.saturating_sub(UnicodeWidthStr::width(ELLIPSIS));
    let mut result = String::new();
    let mut width = 0;
    for character in text.chars() {
        let character_width = UnicodeWidthChar::width(character).unwrap_or(0);
        if width + character_width > content_width {
            break;
        }
        result.push(character);
        width += character_width;
    }
    result.push_str(ELLIPSIS);
    result
}

/// The line of one key row: the chord padded to `chord_width` cells, two blanks, then the summary
/// cut to the rest of `total_width`. A chord wider than its column is cut like a summary.
pub fn row_line(chord: &str, summary: &str, chord_width: usize, total_width: usize) -> String {
    let chord = truncate_to_width(chord, chord_width);
    let padding = chord_width.saturating_sub(UnicodeWidthStr::width(chord.as_str()));
    let summary_width = total_width.saturating_sub(chord_width + 2);
    format!(
        "{chord}{}  {}",
        " ".repeat(padding),
        truncate_to_width(summary, summary_width)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// REQ: PORT:ilium-client/src/help.rs::help_summary_truncation_respects_terminal_cells_and_marks_omissions
    #[test]
    fn help_summary_truncation_respects_terminal_cells_and_marks_omissions() {
        assert_eq!(truncate_to_width("move workspace pane", 6), "move …");
        assert_eq!(truncate_to_width("界面設定", 5), "界面…");
        assert_eq!(truncate_to_width("ok", 2), "ok");
        assert_eq!(truncate_to_width("long", 0), "");
    }

    #[test]
    fn a_row_never_exceeds_its_width_and_aligns_the_summaries() {
        for total in [8, 20, 40] {
            let line = row_line("Ctrl+B c", "Create a new terminal pane", 10, total);
            assert!(
                UnicodeWidthStr::width(line.as_str()) <= total.max(12),
                "{line}"
            );
        }
        let line = row_line("a", "x", 4, 20);
        assert_eq!(line, "a     x");
    }
}

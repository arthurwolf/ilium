//! Cell widths (Ilium local change).
//!
//! The parser must count cells exactly as the program that produced the byte
//! stream did, or a row that the producer filled to the last column overflows
//! here, wraps, and every later line feed lands one row low. Two producers
//! matter to Ilium:
//!
//! * terminal programs, which follow `unicode-width` (Unicode 16), including
//!   the two-cell emoji presentation that U+FE0F (VS16) selects;
//! * Windows ConPTY, whose console host re-emits every pane with its own width
//!   table. A census under a real ConPTY on the GitHub `windows-2025` runner
//!   found it agrees with `unicode-width` on every glyph Ilium draws except
//!   U+2637, which Unicode 16 reclassified as wide and the console host still
//!   draws as one cell.

use unicode_width::{UnicodeWidthChar as _, UnicodeWidthStr as _};

/// Which width table the producer of the byte stream uses.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum WidthPolicy {
    /// `unicode-width` (Unicode 16): terminal programs and Unix terminals.
    Unicode,
    /// The Windows console host behind ConPTY: `unicode-width`, except that
    /// the symbols Unicode 16 reclassified from neutral to wide stay narrow.
    ///
    /// Only U+2637 was measured; the rest of that reclassification (the
    /// trigrams, monograms, digrams and hexagrams) is assumed to behave the
    /// same because it came from the same Unicode change. A later console
    /// host that adopts Unicode 16 widths would disagree again.
    WindowsConsole,
}

impl Default for WidthPolicy {
    /// ConPTY output on Windows, `unicode-width` everywhere else.
    fn default() -> Self {
        if cfg!(windows) {
            Self::WindowsConsole
        } else {
            Self::Unicode
        }
    }
}

impl WidthPolicy {
    /// The cell width of `c`, or `None` for a control character.
    pub(crate) fn char_width(self, c: char) -> Option<usize> {
        if self == Self::WindowsConsole && is_unicode_16_wide_reclassification(c) {
            return Some(1);
        }
        c.width()
    }
}

/// Symbols whose East Asian Width changed from neutral to wide in Unicode 16.
fn is_unicode_16_wide_reclassification(c: char) -> bool {
    matches!(u32::from(c), 0x2630..=0x2637 | 0x268A..=0x268F | 0x4DC0..=0x4DFF)
}

/// Whether VS16 after the one-cell `base` selects a two-cell emoji, as both
/// `unicode-width` and the Windows console host draw it.
pub(crate) fn widens_with_emoji_presentation(base: char) -> bool {
    if base.width() != Some(1) {
        return false;
    }
    let mut text = String::with_capacity(8);
    text.push(base);
    text.push('\u{FE0F}');
    text.as_str().width() == 2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_console_keeps_the_unicode_16_trigrams_narrow() {
        assert_eq!(WidthPolicy::Unicode.char_width('\u{2637}'), Some(2));
        assert_eq!(WidthPolicy::WindowsConsole.char_width('\u{2637}'), Some(1));
        assert_eq!(WidthPolicy::WindowsConsole.char_width('\u{4DC0}'), Some(1));
        // Everything else follows unicode-width under both policies.
        for c in ['a', '\u{4E2D}', '\u{1F600}', '\u{25CF}', '\u{2800}'] {
            assert_eq!(WidthPolicy::WindowsConsole.char_width(c), c.width());
        }
    }

    #[test]
    fn vs16_widens_text_presentation_emoji_only() {
        for base in ['\u{1F6E0}', '\u{267B}', '\u{23F8}', '\u{26D3}', '\u{1F5A5}'] {
            assert!(widens_with_emoji_presentation(base), "{base:?}");
        }
        assert!(!widens_with_emoji_presentation('a'));
        assert!(!widens_with_emoji_presentation('\u{1F600}'));
    }

    fn parsed(policy: WidthPolicy, bytes: &str, cols: u16) -> crate::Parser {
        let mut parser = crate::Parser::new(3, cols, 0);
        parser.screen_mut().set_width_policy(policy);
        parser.process(bytes.as_bytes());
        parser
    }

    #[test]
    fn a_console_host_row_filled_to_the_edge_does_not_wrap() {
        // The console host re-emits a full row with U+2637 as one cell, then
        // CR LF. Counting it as two cells used to wrap the row and push every
        // later line one row down.
        let row = format!("\u{2637}{}", "x".repeat(9));
        let bytes = format!("{row}\r\nnext");
        let windows = parsed(WidthPolicy::WindowsConsole, &bytes, 10);
        assert_eq!(windows.screen().contents(), format!("{row}\nnext"));
        assert!(!windows.screen().row_wrapped(0));
        let unicode = parsed(WidthPolicy::Unicode, &bytes, 10);
        assert!(unicode.screen().row_wrapped(0));
    }

    #[test]
    fn vs16_emoji_takes_two_cells_like_the_program_that_printed_it() {
        let parser = parsed(WidthPolicy::Unicode, "\u{1F6E0}\u{FE0F}X", 10);
        let screen = parser.screen();
        let emoji = screen.cell(0, 0).unwrap();
        assert!(emoji.is_wide());
        assert_eq!(emoji.contents(), "\u{1F6E0}\u{FE0F}");
        assert!(screen.cell(0, 1).unwrap().is_wide_continuation());
        assert_eq!(screen.cell(0, 2).unwrap().contents(), "X");
        assert_eq!(screen.cursor_position(), (0, 3));
    }

    #[test]
    fn vs16_overwrites_a_following_wide_cell_cleanly() {
        // Cursor back over an existing wide character, then an emoji.
        let parser = parsed(WidthPolicy::Unicode, "a\u{4E2D}\x1b[1G\u{267B}\u{FE0F}", 10);
        let screen = parser.screen();
        assert!(screen.cell(0, 0).unwrap().is_wide());
        assert!(screen.cell(0, 1).unwrap().is_wide_continuation());
        assert!(!screen.cell(0, 2).unwrap().is_wide_continuation());
        assert_eq!(screen.cell(0, 2).unwrap().contents(), "");
    }

    #[test]
    fn vs16_on_the_last_column_or_after_plain_text_stays_one_cell() {
        let edge = parsed(WidthPolicy::Unicode, "123456789\u{267B}\u{FE0F}", 10);
        assert!(!edge.screen().cell(0, 9).unwrap().is_wide());
        assert_eq!(
            edge.screen().cell(0, 9).unwrap().contents(),
            "\u{267B}\u{FE0F}"
        );
        let plain = parsed(WidthPolicy::Unicode, "a\u{FE0F}b", 10);
        assert_eq!(plain.screen().cell(0, 1).unwrap().contents(), "b");
    }
}

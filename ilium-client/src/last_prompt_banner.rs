//! The optional banner reserved below a detected agent pane's toolbar (or
//! at its content top if the toolbar is hidden), showing the most recent
//! exactly-reconstructed line the user typed or pasted and submitted -- see
//! `ilium_core::Tree::set_last_prompt`.
//!
//! The enabled slot is reserved before interaction. Text wraps and is
//! middle-truncated within that fixed budget, preserving terminal dimensions.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::theme::{self, ColorScheme};

/// Splits `text` into logical lines (`\n`, `\r\n`, or a lone `\r` -- see
/// the module-level rationale carried over from the previous
/// line-splitting logic: exactly-reconstructed terminal input can carry any
/// of the three depending on where it was typed or pasted from), then
/// word-wraps each logical line to `width` columns.
///
/// Wrapping breaks at whitespace, collapsing whitespace runs -- this is a
/// preview banner, not an exact-whitespace transcript, so that's an
/// acceptable trade for keeping the wrap logic simple and matching how a
/// terminal itself reflows text. A single word wider than `width` is
/// hard-broken at grapheme boundaries so no returned row's display width
/// can ever exceed `width`. An empty logical line still yields one empty
/// display row, so an intentional blank line inside a multi-line prompt
/// survives wrapping instead of vanishing.
pub fn wrap_lines(text: &str, width: u16) -> Vec<String> {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    normalized
        .lines()
        .flat_map(|line| wrap_one_line(line, width))
        .collect()
}

fn wrap_one_line(line: &str, width: u16) -> Vec<String> {
    let width = usize::from(width.max(1));
    if line.is_empty() {
        return vec![String::new()];
    }
    let mut rows = Vec::new();
    let mut current = String::new();
    let mut current_width = 0usize;
    for word in line.split_whitespace() {
        let word_width = UnicodeWidthStr::width(word);
        if word_width > width {
            if !current.is_empty() {
                rows.push(std::mem::take(&mut current));
            }
            let (piece, piece_width) = hard_break_word(word, width, &mut rows);
            current = piece;
            current_width = piece_width;
            continue;
        }
        let gap = usize::from(!current.is_empty());
        if !current.is_empty() && current_width + gap + word_width > width {
            rows.push(std::mem::take(&mut current));
            current_width = 0;
        }
        if !current.is_empty() {
            current.push(' ');
            current_width += 1;
        }
        current.push_str(word);
        current_width += word_width;
    }
    if !current.is_empty() || rows.is_empty() {
        rows.push(current);
    }
    rows
}

/// Breaks one word too wide to fit on any row at grapheme boundaries,
/// pushing every full row into `rows` and returning the final (possibly
/// partial) row's text and display width so the caller can keep filling it
/// with the next word.
fn hard_break_word(word: &str, width: usize, rows: &mut Vec<String>) -> (String, usize) {
    let mut piece = String::new();
    let mut piece_width = 0usize;
    for grapheme in word.graphemes(true) {
        let grapheme_width = UnicodeWidthStr::width(grapheme).max(1);
        if piece_width + grapheme_width > width && !piece.is_empty() {
            rows.push(std::mem::take(&mut piece));
            piece_width = 0;
        }
        piece.push_str(grapheme);
        piece_width += grapheme_width;
    }
    (piece, piece_width)
}

/// Splits already-wrapped display `rows` down to at most `max_lines`. At or
/// under budget, every row is kept as-is. Over budget, keeps the first
/// `max_lines.div_ceil(2)` and last `max_lines / 2` rows -- dropping the
/// middle -- and marks the two rows adjacent to that gap (`true` in the
/// returned tuple) so the caller can style them as the seam, since
/// inserting a separate marker row would grow the display past `max_lines`.
pub fn truncate_middle_rows(rows: Vec<String>, max_lines: u16) -> Vec<(String, bool)> {
    let max_lines = usize::from(max_lines.max(1));
    if rows.len() <= max_lines {
        return rows.into_iter().map(|row| (row, false)).collect();
    }
    let head = max_lines.div_ceil(2);
    let tail = max_lines - head;
    let mut result = Vec::with_capacity(max_lines);
    result.extend(
        rows[..head]
            .iter()
            .enumerate()
            .map(|(index, row)| (row.clone(), index == head - 1)),
    );
    result.extend(
        rows[rows.len() - tail..]
            .iter()
            .enumerate()
            .map(|(index, row)| (row.clone(), index == 0)),
    );
    result
}

/// Wraps `text` to `width` columns, then middle-truncates to at most
/// `max_lines` rows. Rendering uses the smaller of this budget and the
/// physical slot; wrapping never changes the fixed reservation.
pub fn layout(text: &str, width: u16, max_lines: u16) -> Vec<(String, bool)> {
    truncate_middle_rows(wrap_lines(text, width), max_lines)
}

/// Fixed row budget, independent of prompt presence or wrapping.
pub fn reserved_height(max_lines: u16) -> u16 {
    max_lines.max(1)
}

/// Renders the last-prompt banner into `area`: a background fill spanning
/// the whole reserved region (see [`theme::last_prompt_style`]), with
/// `prompt` -- when present -- wrapped and middle-truncated via [`layout`]
/// using `area`'s own width and `max_lines`. `None` (no exact submission
/// recorded yet for this pane) renders only the background fill.
pub fn render(
    frame: &mut Frame,
    area: Rect,
    prompt: Option<&str>,
    max_lines: u16,
    scheme: ColorScheme,
) {
    if area.height == 0 {
        return;
    }
    let lines: Vec<Line> = prompt
        .map(|prompt| {
            layout(prompt, area.width, max_lines.min(area.height))
                .into_iter()
                .map(|(line, is_seam)| {
                    if is_seam {
                        Line::from(Span::styled(line, Style::new().add_modifier(Modifier::DIM)))
                    } else {
                        Line::from(line)
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    let paragraph = Paragraph::new(lines).style(theme::last_prompt_style(scheme));
    frame.render_widget(paragraph, area);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_prompt_slot_retains_the_end_of_the_prompt() {
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(40, 2)).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                render(
                    frame,
                    area,
                    Some("FIRST\nSECOND\nTHIRD\nLAST"),
                    4,
                    ColorScheme::Dark,
                );
            })
            .unwrap();
        let text = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(text.contains("FIRST"));
        assert!(text.contains("LAST"));
        assert!(!text.contains("SECOND"));
    }

    fn owned(lines: Vec<(&str, bool)>) -> Vec<(String, bool)> {
        lines
            .into_iter()
            .map(|(line, is_seam)| (line.to_string(), is_seam))
            .collect()
    }

    #[test]
    fn text_at_or_under_budget_keeps_every_line_untouched() {
        assert_eq!(
            layout("only one line", 80, 4),
            owned(vec![("only one line", false)])
        );
        assert_eq!(
            layout("one\ntwo\nthree\nfour", 80, 4),
            owned(vec![
                ("one", false),
                ("two", false),
                ("three", false),
                ("four", false)
            ])
        );
        assert_eq!(layout("", 80, 4), Vec::<(String, bool)>::new());
    }

    #[test]
    fn over_budget_keeps_first_two_and_last_two_at_the_default_of_four() {
        let text = "one\ntwo\nthree\nfour\nfive\nsix\nseven";
        assert_eq!(
            layout(text, 80, 4),
            owned(vec![
                ("one", false),
                ("two", true),
                ("six", true),
                ("seven", false),
            ])
        );
    }

    #[test]
    fn odd_budgets_give_the_extra_kept_line_to_the_head() {
        let text = "one\ntwo\nthree\nfour\nfive\nsix\nseven";
        assert_eq!(
            layout(text, 80, 5),
            owned(vec![
                ("one", false),
                ("two", false),
                ("three", true),
                ("six", true),
                ("seven", false),
            ])
        );
    }

    #[test]
    fn a_budget_of_one_keeps_only_the_first_line_marked_as_the_seam() {
        let text = "one\ntwo\nthree";
        assert_eq!(layout(text, 80, 1), owned(vec![("one", true)]));
    }

    #[test]
    fn lone_carriage_returns_split_lines_same_as_line_feeds() {
        let text = "first line of a longer prompt\rsecond line\rthird line\rfourth line\rfifth line\rsixth line";
        assert_eq!(
            layout(text, 80, 4),
            owned(vec![
                ("first line of a longer prompt", false),
                ("second line", true),
                ("fifth line", true),
                ("sixth line", false),
            ])
        );
    }

    #[test]
    fn a_zero_budget_is_treated_as_one() {
        let text = "one\ntwo\nthree";
        assert_eq!(layout(text, 80, 0), owned(vec![("one", true)]));
    }

    #[test]
    fn the_prompt_slot_reserves_the_full_configured_budget() {
        assert_eq!(reserved_height(4), 4);
    }

    #[test]
    fn a_zero_prompt_budget_still_reserves_one_row() {
        assert_eq!(reserved_height(0), 1);
    }

    #[test]
    fn a_prompt_exactly_at_the_wrap_boundary_stays_on_one_row() {
        // "12345 12345" is 11 columns wide, exactly `width`.
        assert_eq!(wrap_lines("12345 12345", 11), vec!["12345 12345"]);
    }

    #[test]
    fn a_long_single_line_word_wraps_instead_of_being_clipped() {
        let text = "the quick brown fox jumps over the lazy dog";
        assert_eq!(
            wrap_lines(text, 10),
            vec!["the quick", "brown fox", "jumps over", "the lazy", "dog"],
        );
        // Wrapping does not change the configured reservation.
        assert_eq!(reserved_height(20), 20);
    }

    #[test]
    fn a_word_wider_than_the_column_hard_breaks_at_grapheme_boundaries() {
        let text = "seeAVeryLongUnbrokenIdentifierThatCannotWrapAtASpace";
        let wrapped = wrap_lines(text, 10);
        assert!(
            wrapped
                .iter()
                .all(|row| UnicodeWidthStr::width(row.as_str()) <= 10),
            "every row must fit the column width: {wrapped:?}"
        );
        assert_eq!(wrapped.join(""), text, "no characters may be dropped");
    }

    #[test]
    fn blank_lines_inside_a_multiline_prompt_survive_wrapping_as_empty_rows() {
        assert_eq!(
            wrap_lines("first\n\nthird", 80),
            vec!["first".to_string(), String::new(), "third".to_string()]
        );
    }

    #[test]
    fn wrapping_that_still_exceeds_max_lines_is_middle_truncated_over_wrapped_rows() {
        let text = "aa bb cc dd ee ff gg hh ii jj";
        // Every word is 2 columns wide; fitting a second one onto the same
        // row would need 5 columns (`2 + 1 gap + 2`), which is over the
        // width-4 budget, so wrapping at width 4 yields ten rows, one word
        // per row.
        let wrapped = wrap_lines(text, 4);
        assert_eq!(wrapped.len(), 10);
        let shown = layout(text, 4, 4);
        assert_eq!(
            shown,
            owned(vec![
                ("aa", false),
                ("bb", true),
                ("ii", true),
                ("jj", false),
            ])
        );
    }

    #[test]
    fn a_narrow_width_of_zero_does_not_panic() {
        let _ = wrap_lines("some text", 0);
        let _ = reserved_height(4);
    }
}

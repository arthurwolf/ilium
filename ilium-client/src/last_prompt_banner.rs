//! The optional banner reserved below a detected agent pane's toolbar (or
//! at its content top if the toolbar is hidden), showing the most recent
//! exactly-reconstructed line the user typed or pasted and submitted -- see
//! `ilium_core::Tree::set_last_prompt`. A fixed number of rows
//! (`UiSettings::last_prompt_max_lines`) regardless of the current prompt's
//! actual line count -- see `PaneViewport::with_last_prompt_reserved`'s doc
//! comment for why the reservation can't track content size.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::theme::{self, ColorScheme};

/// Splits `text` into display lines truncated to at most `max_lines` rows.
/// At or under budget, every real line is kept as-is. Over budget, keeps
/// the first `max_lines.div_ceil(2)` and last `max_lines / 2` lines --
/// dropping the middle -- and marks the two lines adjacent to that gap
/// (`true` in the returned tuple) so the caller can style them as the seam,
/// since inserting a separate marker row would grow the display past
/// `max_lines`.
///
/// Line boundaries are `\n`, `\r\n`, or a lone `\r` -- exactly-reconstructed
/// terminal input can carry any of the three depending on where it was typed
/// or pasted from, and `str::lines()` alone only recognizes the first two,
/// leaving a lone `\r` as an invisible control character that collapses the
/// whole prompt onto one row instead of splitting it.
pub fn truncate_middle(text: &str, max_lines: u16) -> Vec<(String, bool)> {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<&str> = normalized.lines().collect();
    let max_lines = usize::from(max_lines.max(1));
    if lines.len() <= max_lines {
        return lines
            .into_iter()
            .map(|line| (line.to_string(), false))
            .collect();
    }
    let head = max_lines.div_ceil(2);
    let tail = max_lines - head;
    let mut result = Vec::with_capacity(max_lines);
    result.extend(
        lines[..head]
            .iter()
            .enumerate()
            .map(|(index, line)| (line.to_string(), index == head - 1)),
    );
    result.extend(
        lines[lines.len() - tail..]
            .iter()
            .enumerate()
            .map(|(index, line)| (line.to_string(), index == 0)),
    );
    result
}

/// Renders the last-prompt banner into `area`: a background fill spanning
/// the whole reserved region (see [`theme::last_prompt_style`]), with
/// `prompt` -- when present -- middle-truncated to `area.height` rows via
/// [`truncate_middle`]. `None` (no exact submission recorded yet for this
/// pane) renders only the background fill.
pub fn render(frame: &mut Frame, area: Rect, prompt: Option<&str>, scheme: ColorScheme) {
    if area.height == 0 {
        return;
    }
    let lines: Vec<Line> = prompt
        .map(|prompt| {
            truncate_middle(prompt, area.height)
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

    fn owned(lines: Vec<(&str, bool)>) -> Vec<(String, bool)> {
        lines
            .into_iter()
            .map(|(line, is_seam)| (line.to_string(), is_seam))
            .collect()
    }

    #[test]
    fn text_at_or_under_budget_keeps_every_line_untouched() {
        assert_eq!(
            truncate_middle("only one line", 4),
            owned(vec![("only one line", false)])
        );
        assert_eq!(
            truncate_middle("one\ntwo\nthree\nfour", 4),
            owned(vec![
                ("one", false),
                ("two", false),
                ("three", false),
                ("four", false)
            ])
        );
        assert_eq!(truncate_middle("", 4), Vec::<(String, bool)>::new());
    }

    #[test]
    fn over_budget_keeps_first_two_and_last_two_at_the_default_of_four() {
        let text = "one\ntwo\nthree\nfour\nfive\nsix\nseven";
        assert_eq!(
            truncate_middle(text, 4),
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
            truncate_middle(text, 5),
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
        assert_eq!(truncate_middle(text, 1), owned(vec![("one", true)]));
    }

    #[test]
    fn lone_carriage_returns_split_lines_same_as_line_feeds() {
        let text = "first line of a longer prompt\rsecond line\rthird line\rfourth line\rfifth line\rsixth line";
        assert_eq!(
            truncate_middle(text, 4),
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
        assert_eq!(truncate_middle(text, 0), owned(vec![("one", true)]));
    }
}

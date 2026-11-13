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

use crate::last_prompt_toolbar::{PromptToolbarButton, ToolbarVisibility};
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

/// Columns left of the text: one blank, the quote gutter, one blank.
const TEXT_INSET_LEFT: u16 = 3;
const TEXT_INSET_RIGHT: u16 = 2;
/// Left-aligned quarter block drawn as the blockquote gutter.
const QUOTE_GUTTER: &str = "\u{258e}";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BannerTab {
    Prompt,
    Goal,
}

/// Tab geometry is shared by rendering and mouse hit testing. Compact labels
/// keep both tabs available when the banner is narrow.
pub fn tab_rects(area: Rect, toolbar: bool) -> Option<(Rect, Rect)> {
    if area.height == 0 {
        return None;
    }
    let reserved =
        if area.height == 1 && toolbar && crate::last_prompt_toolbar::has_toolbar(area.width) {
            crate::last_prompt_toolbar::TOOLBAR_WIDTH
        } else {
            0
        };
    let available = area.width.saturating_sub(reserved);
    if available < 3 {
        return None;
    }
    let (prompt_width, goal_width) = if available >= 12 { (6, 4) } else { (1, 1) };
    Some((
        Rect::new(area.x, area.y, prompt_width, 1),
        Rect::new(area.x + prompt_width + 1, area.y, goal_width, 1),
    ))
}

/// Columns the text wraps to inside `area_width`. Areas too narrow for the
/// insets use the whole width so tiny panes still show their prompt.
fn text_width(area_width: u16) -> u16 {
    let inset = TEXT_INSET_LEFT + TEXT_INSET_RIGHT;
    if area_width > inset + 8 {
        area_width - inset
    } else {
        area_width
    }
}

/// Interaction state for the banner's toolbar and optional tabs.
pub struct BannerControls {
    pub toolbar: Option<ToolbarVisibility>,
    pub hovered: Option<PromptToolbarButton>,
    pub selected_tab: Option<BannerTab>,
}

/// Renders the last-prompt banner into `area`: a background fill spanning
/// the whole reserved region (see [`theme::last_prompt_style`]), with
/// `prompt` -- when present -- wrapped and middle-truncated via [`layout`]
/// inside breathing room on both sides, behind an accent-coloured quote
/// gutter on every row. `None` (no exact submission recorded yet for this
/// pane) renders only the background fill.
pub fn render(
    frame: &mut Frame,
    area: Rect,
    prompt: Option<&str>,
    max_lines: u16,
    scheme: ColorScheme,
    controls: BannerControls,
) {
    let BannerControls {
        toolbar,
        hovered,
        selected_tab,
    } = controls;
    if area.height == 0 {
        return;
    }
    frame.render_widget(
        Paragraph::new("").style(theme::last_prompt_style(scheme)),
        area,
    );
    let Some(prompt) = prompt else {
        return;
    };
    let framed = text_width(area.width) != area.width;
    let (inset_left, inset_right) = if framed {
        (TEXT_INSET_LEFT, TEXT_INSET_RIGHT)
    } else {
        (0, 0)
    };
    let mut text_area = Rect::new(
        area.x + inset_left,
        area.y,
        area.width - inset_left - inset_right,
        area.height,
    );
    if toolbar.is_some() && crate::last_prompt_toolbar::has_toolbar(area.width) {
        text_area.width = text_area
            .width
            .saturating_sub(crate::last_prompt_toolbar::TOOLBAR_WIDTH.saturating_sub(inset_right));
    }
    if let Some(selected) = selected_tab {
        if let Some((prompt_tab, goal_tab)) = tab_rects(area, toolbar.is_some()) {
            for (tab, rect, full, short) in [
                (BannerTab::Prompt, prompt_tab, "Prompt", "P"),
                (BannerTab::Goal, goal_tab, "Goal", "G"),
            ] {
                let mut style = theme::last_prompt_style(scheme);
                if tab == selected {
                    style = style.add_modifier(Modifier::BOLD | Modifier::REVERSED);
                }
                frame.render_widget(
                    Paragraph::new(if rect.width == 1 { short } else { full }).style(style),
                    rect,
                );
            }
            if area.height > 1 {
                text_area.y += 1;
                text_area.height = text_area.height.saturating_sub(1);
            } else {
                let start = goal_tab.x.saturating_add(goal_tab.width).saturating_add(1);
                let end = text_area.x.saturating_add(text_area.width);
                text_area.x = start.min(end);
                text_area.width = end.saturating_sub(text_area.x);
            }
        }
    }
    let rows = layout(prompt, text_area.width, max_lines.min(text_area.height));
    let gutter_style = Style::new()
        .fg(theme::accent_bg())
        .bg(theme::muted_accent_bg(scheme));
    for (index, (_, is_seam)) in rows.iter().enumerate() {
        if framed && (selected_tab.is_none() || area.height > 1) {
            let gutter = if *is_seam {
                gutter_style.add_modifier(Modifier::DIM)
            } else {
                gutter_style
            };
            frame.render_widget(
                Paragraph::new(Span::styled(QUOTE_GUTTER, gutter)),
                Rect::new(area.x + 1, text_area.y + index as u16, 1, 1),
            );
        }
    }
    let lines: Vec<Line> = rows
        .into_iter()
        .map(|(line, is_seam)| {
            if is_seam {
                Line::from(Span::styled(line, Style::new().add_modifier(Modifier::DIM)))
            } else {
                Line::from(line)
            }
        })
        .collect();
    frame.render_widget(
        Paragraph::new(lines).style(theme::last_prompt_style(scheme)),
        text_area,
    );
    if let Some(visibility) = toolbar {
        render_toolbar(frame, area, visibility, hovered, scheme);
    }
}

/// Draws the banner's mini toolbar in its bottom-right corner.
fn render_toolbar(
    frame: &mut Frame,
    area: Rect,
    visibility: ToolbarVisibility,
    hovered: Option<PromptToolbarButton>,
    scheme: ColorScheme,
) {
    for (button, rect) in crate::last_prompt_toolbar::visible_buttons(area, visibility) {
        let mut style = theme::last_prompt_style(scheme);
        if hovered == Some(button) {
            style = style.add_modifier(Modifier::BOLD | Modifier::REVERSED);
        }
        frame.render_widget(Paragraph::new(Span::styled(button.glyph(), style)), rect);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn goal_tabs_fit_narrow_rows_and_never_overlap_the_toolbar() {
        let area = Rect::new(2, 3, 30, 1);
        let (prompt, goal) = tab_rects(area, true).unwrap();
        assert!(prompt.x + prompt.width < goal.x);
        assert!(
            goal.x + goal.width <= area.x + area.width - crate::last_prompt_toolbar::TOOLBAR_WIDTH
        );
        let compact = Rect::new(0, 0, 5, 1);
        let (prompt, goal) = tab_rects(compact, false).unwrap();
        assert_eq!((prompt.width, goal.width), (1, 1));
        assert!(tab_rects(Rect::new(0, 0, 2, 1), false).is_none());
    }

    #[test]
    fn goal_tab_renders_with_the_prompt_controls_in_one_and_two_rows() {
        for height in [1, 2] {
            let mut terminal =
                ratatui::Terminal::new(ratatui::backend::TestBackend::new(50, height)).unwrap();
            terminal
                .draw(|frame| {
                    let area = frame.area();
                    render(
                        frame,
                        area,
                        Some("Paused: objective"),
                        height,
                        ColorScheme::Dark,
                        BannerControls {
                            toolbar: Some(ToolbarVisibility {
                                older: false,
                                newer: false,
                            }),
                            hovered: None,
                            selected_tab: Some(BannerTab::Goal),
                        },
                    );
                })
                .unwrap();
            let buffer = terminal.backend().buffer();
            let top: String = (0..50).map(|x| buffer[(x, 0)].symbol()).collect();
            assert!(top.contains("Prompt") && top.contains("Goal"));
            let bottom: String = (0..50).map(|x| buffer[(x, height - 1)].symbol()).collect();
            assert!(bottom.contains(PromptToolbarButton::Close.glyph()));
            assert!(bottom.contains("Paused"));
        }
    }

    #[test]
    fn wide_banner_draws_a_quote_gutter_and_indents_the_text() {
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(40, 3)).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                render(
                    frame,
                    area,
                    Some("hello world"),
                    3,
                    ColorScheme::Dark,
                    BannerControls {
                        toolbar: None,
                        hovered: None,
                        selected_tab: None,
                    },
                );
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        let row: String = (0..40).map(|x| buffer[(x, 0)].symbol()).collect();
        let gutter_x = row.chars().position(|c| c == '\u{258e}');
        let text_x = row.find("hello").map(|byte| row[..byte].chars().count());
        let (gutter_x, text_x) = (gutter_x.expect("gutter"), text_x.expect("text"));
        assert!(gutter_x < text_x);
        assert!(text_x >= usize::from(TEXT_INSET_LEFT));
    }

    #[test]
    fn toolbar_draws_its_buttons_in_the_bottom_right_corner() {
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(60, 3)).unwrap();
        let visibility = ToolbarVisibility {
            older: false,
            newer: false,
        };
        terminal
            .draw(|frame| {
                let area = frame.area();
                render(
                    frame,
                    area,
                    Some("hello"),
                    3,
                    ColorScheme::Dark,
                    BannerControls {
                        toolbar: Some(visibility),
                        hovered: None,
                        selected_tab: None,
                    },
                );
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        let bottom: String = (0..60).map(|x| buffer[(x, 2)].symbol()).collect();
        assert!(bottom.contains(PromptToolbarButton::Copy.glyph()));
        assert!(bottom.contains(PromptToolbarButton::Close.glyph()));
        assert!(!bottom.contains(PromptToolbarButton::Older.glyph()));
        let top: String = (0..60).map(|x| buffer[(x, 0)].symbol()).collect();
        assert!(!top.contains(PromptToolbarButton::Close.glyph()));
    }

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
                    BannerControls {
                        toolbar: None,
                        hovered: None,
                        selected_tab: None,
                    },
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

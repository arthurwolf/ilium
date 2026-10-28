//! Rounded frame rules that separate the stacked sections of a pane (agent
//! toolbar, last prompt, progress footer, tree toolbar) from the content
//! around them.
//!
//! A rule spans the full outer width of its panel. Its first and last cells
//! are `├` and `┤`, which is exactly what the panel's rounded `Fuzzy`-merged
//! border produces where a horizontal line meets the vertical edge, so the
//! rule reads as part of the same frame. Unicode has no rounded tee, so the
//! tee glyphs are the correct joint for a rounded border.

use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

use crate::theme;

const LEFT_JOINT: &str = "├";
const RIGHT_JOINT: &str = "┤";
const LINE: &str = "─";
/// Cells between the left joint and the title: one line cell, then a space.
const TITLE_INSET: u16 = 2;

/// Draws one rule across `area` (its first row). A `title` is inset after the
/// left joint and dropped, rather than clipped mid-word, when it cannot fit
/// with a line cell and a space on both sides.
pub fn render_rule(frame: &mut Frame, area: Rect, title: Option<&str>, focused: bool) {
    if area.width < 2 || area.height == 0 {
        return;
    }
    let style = theme::border_style(focused);
    let buffer = frame.buffer_mut();
    let last = area.right() - 1;
    for x in area.x..=last {
        let symbol = if x == area.x {
            LEFT_JOINT
        } else if x == last {
            RIGHT_JOINT
        } else {
            LINE
        };
        buffer[(x, area.y)].set_symbol(symbol).set_style(style);
    }
    let Some(title) = title.filter(|title| !title.is_empty()) else {
        return;
    };
    let label = format!(" {title} ");
    let label_width = UnicodeWidthStr::width(label.as_str()) as u16;
    // Joint + inset line cells + label + at least one line cell + joint.
    if area.width < TITLE_INSET + label_width + 2 {
        return;
    }
    buffer.set_string(
        area.x + TITLE_INSET,
        area.y,
        &label,
        style.add_modifier(Modifier::BOLD),
    );
}

#[cfg(test)]
mod tests {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    use super::*;

    fn row(width: u16, title: Option<&str>) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, 1)).unwrap();
        terminal
            .draw(|frame| render_rule(frame, frame.area(), title, true))
            .unwrap();
        let buffer = terminal.backend().buffer();
        (0..width).map(|x| buffer[(x, 0)].symbol()).collect()
    }

    #[test]
    fn joins_the_side_borders_with_tees() {
        assert_eq!(row(8, None), "├──────┤");
    }

    #[test]
    fn titles_are_inset_and_dropped_when_they_do_not_fit() {
        assert_eq!(row(14, Some("prompt")), "├─ prompt ───┤");
        assert_eq!(row(8, Some("prompt")), "├──────┤");
    }

    #[test]
    fn degenerate_widths_draw_nothing_unsafe() {
        assert_eq!(row(2, Some("x")), "├┤");
        let mut terminal = Terminal::new(TestBackend::new(1, 1)).unwrap();
        terminal
            .draw(|frame| render_rule(frame, frame.area(), None, false))
            .unwrap();
    }
}

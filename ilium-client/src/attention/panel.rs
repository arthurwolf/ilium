//! Drawing and hit testing for the attention queue's three surfaces: the
//! bottom-of-the-left-panel button, the right-hand panel, and the arrival
//! block shown over the focused pane when a new item appears.
//!
//! Everything here is a pure function of plain values (rows, counts, rects),
//! so geometry is shared by drawing and by pointer hit testing and is tested
//! without an `App`. `crate::app_attention` builds the rows from live state.
//!
//! Glyphs: the codebase's icon table (`icon_settings`) has no Tabler set and
//! its defaults are emoji, so the kinds use fixed emoji of the same family.

use ratatui::layout::{Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::item::AttentionKind;
use crate::theme;

/// Rows of one queue entry: header, two question lines, project and title.
pub const ENTRY_LINES: u16 = 4;
/// One blank row between entries.
pub const ENTRY_HEIGHT: u16 = ENTRY_LINES + 1;
/// The arrival block is not drawn on a pane narrower than this.
pub const MINIMUM_ARRIVAL_WIDTH: u16 = 24;
/// The arrival block never takes more than this share of the pane width.
const ARRIVAL_WIDTH_FRACTION: u16 = 2;
/// Glyph of the button and of the panel title.
pub const ATTENTION_GLYPH: &str = "\u{1F514}";
/// Label of the left-panel button.
pub const BUTTON_LABEL: &str = "Next: needs you";

/// One queue entry as the panel shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelRow {
    pub kind: AttentionKind,
    pub reason: String,
    pub question: Option<String>,
    pub project: String,
    pub title: String,
    pub age_ms: u64,
    pub is_looked: bool,
}

pub const fn kind_glyph(kind: AttentionKind) -> &'static str {
    match kind {
        AttentionKind::Question => "\u{2753}",
        AttentionKind::Stuck => "\u{26D4}",
        AttentionKind::WaitingOn => "\u{23F3}",
        AttentionKind::Changed => "\u{2728}",
    }
}

/// Compact age: `45s`, `12m`, `3h`, `2d`.
pub fn format_age(age_ms: u64) -> String {
    let seconds = age_ms / 1000;
    match seconds {
        0..=59 => format!("{seconds}s"),
        60..=3599 => format!("{}m", seconds / 60),
        3600..=86_399 => format!("{}h", seconds / 3600),
        _ => format!("{}d", seconds / 86_400),
    }
}

/// Clips `text` to `width` terminal cells, ending in `…` when it was cut.
pub fn clip(text: &str, width: usize) -> String {
    if text.width() <= width {
        return text.to_string();
    }
    if width == 0 {
        return String::new();
    }
    let mut clipped = String::new();
    let mut used = 0;
    for character in text.chars() {
        let advance = character.width().unwrap_or(0);
        if used + advance + 1 > width {
            break;
        }
        clipped.push(character);
        used += advance;
    }
    clipped.push('\u{2026}');
    clipped
}

/// Greedy word wrap into at most `maximum_lines` lines of `width` cells; the
/// last line ends in `…` when text remains.
pub fn wrap(text: &str, width: usize, maximum_lines: usize) -> Vec<String> {
    if width == 0 || maximum_lines == 0 {
        return Vec::new();
    }
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut is_truncated = false;
    for word in text.split_whitespace() {
        // A word longer than a whole line is hard-clipped.
        let word = clip(word, width);
        let separator = usize::from(!current.is_empty());
        if current.width() + separator + word.width() <= width {
            if separator == 1 {
                current.push(' ');
            }
            current.push_str(&word);
            continue;
        }
        lines.push(std::mem::take(&mut current));
        if lines.len() == maximum_lines {
            is_truncated = true;
            break;
        }
        current = word;
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if is_truncated {
        if let Some(mut last) = lines.pop() {
            while last.width() + 1 > width {
                last.pop();
            }
            last.push('\u{2026}');
            lines.push(last);
        }
    }
    lines
}

// ---------------------------------------------------------------- button

/// Text of the left-panel button for a given width: the glyph, the label and a
/// right-aligned count badge, degrading to the glyph and badge when narrow.
pub fn button_text(count: usize, width: u16) -> String {
    let width = usize::from(width);
    let badge = if count == 0 {
        String::new()
    } else {
        format!("{count}")
    };
    let full = format!("{ATTENTION_GLYPH} {BUTTON_LABEL}");
    let compact = format!("{ATTENTION_GLYPH} {badge}").trim_end().to_string();
    let label = if full.width() + badge.width() + 2 <= width {
        full
    } else {
        compact.clone()
    };
    if label == compact {
        return clip(&compact, width);
    }
    let padding = width.saturating_sub(label.width() + badge.width() + 1);
    format!("{label}{}{badge} ", " ".repeat(padding))
}

/// Draws the button. An empty queue draws it dim so the row keeps its place
/// without looking pressable.
pub fn render_button(frame: &mut Frame, area: Rect, count: usize) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let style = if count == 0 {
        Style::new().add_modifier(Modifier::DIM)
    } else {
        theme::selected_style().add_modifier(Modifier::BOLD)
    };
    let text = button_text(count, area.width);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(text, style))).style(style),
        area,
    );
}

// ----------------------------------------------------------------- panel

/// Geometry shared by drawing and hit testing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PanelLayout {
    pub frame: Rect,
    pub content: Rect,
    /// Whole entries that fit in `content`.
    pub capacity: usize,
}

pub fn layout(area: Rect) -> PanelLayout {
    let content = theme::block(false).inner(area);
    let capacity = usize::from((content.height + 1) / ENTRY_HEIGHT);
    PanelLayout {
        frame: area,
        content,
        capacity,
    }
}

/// First visible entry so that `selected` stays on screen with as little
/// scrolling as possible.
pub fn first_visible(selected: usize, total: usize, capacity: usize) -> usize {
    if capacity == 0 || total <= capacity {
        return 0;
    }
    let last_start = total - capacity;
    selected.saturating_sub(capacity - 1).min(last_start)
}

/// Screen rectangle of the entry in visible slot `slot`.
pub fn entry_rect(layout: PanelLayout, slot: usize) -> Rect {
    let offset = u16::try_from(slot)
        .unwrap_or(u16::MAX)
        .saturating_mul(ENTRY_HEIGHT);
    Rect::new(
        layout.content.x,
        layout.content.y.saturating_add(offset),
        layout.content.width,
        ENTRY_LINES.min(layout.content.height.saturating_sub(offset)),
    )
}

/// Index (into the full ordered list) of the entry under `position`.
pub fn entry_at(
    layout: PanelLayout,
    position: Position,
    first: usize,
    total: usize,
) -> Option<usize> {
    (0..layout.capacity.min(total.saturating_sub(first)))
        .find(|slot| entry_rect(layout, *slot).contains(position))
        .map(|slot| first + slot)
}

/// Draws the panel. `selected` is the highlighted entry index.
pub fn render_panel(
    frame: &mut Frame,
    area: Rect,
    rows: &[PanelRow],
    selected: usize,
    is_focused: bool,
) {
    let layout = layout(area);
    let title = format!("{ATTENTION_GLYPH} Attention queue \u{b7} {}", rows.len());
    frame.render_widget(
        theme::block(is_focused).title(theme::chrome_title(&title)),
        area,
    );
    if layout.content.width == 0 || layout.content.height == 0 {
        return;
    }
    if rows.is_empty() {
        let lines = vec![
            Line::from(Span::styled(
                "Nothing needs you right now.",
                Style::new().add_modifier(Modifier::DIM),
            )),
            Line::from(Span::styled(
                "Agents that ask a question, get stuck or finish show up here.",
                Style::new().add_modifier(Modifier::DIM),
            )),
        ];
        frame.render_widget(Paragraph::new(lines), layout.content);
        return;
    }
    let first = first_visible(selected, rows.len(), layout.capacity);
    for (slot, row) in rows.iter().skip(first).take(layout.capacity).enumerate() {
        let rect = entry_rect(layout, slot);
        let is_selected = first + slot == selected;
        frame.render_widget(
            Paragraph::new(entry_lines(row, rect.width, is_selected)),
            rect,
        );
    }
}

fn entry_lines(row: &PanelRow, width: u16, is_selected: bool) -> Vec<Line<'static>> {
    let width = usize::from(width);
    let base = if is_selected {
        theme::selected_style()
    } else {
        Style::new()
    };
    let dim = base.add_modifier(Modifier::DIM);
    let age = format_age(row.age_ms);
    let head = format!("{} {}", kind_glyph(row.kind), row.kind.label());
    let reason_budget = width.saturating_sub(head.width() + age.width() + 5);
    let reason = clip(&row.reason, reason_budget);
    let used = head.width() + 3 + reason.width();
    let gap = width.saturating_sub(used + age.width());
    let head_style = if row.is_looked {
        base.add_modifier(Modifier::DIM)
    } else {
        base.add_modifier(Modifier::BOLD)
    };
    let mut lines = vec![Line::from(vec![
        Span::styled(head, head_style),
        Span::styled(format!(" \u{b7} {reason}"), base),
        Span::styled(" ".repeat(gap), base),
        Span::styled(age, dim),
    ])];
    let question_width = width.saturating_sub(3);
    let mut question_lines = match row.question.as_deref() {
        Some(question) => wrap(question, question_width, 2),
        None => Vec::new(),
    };
    question_lines.resize(2, String::new());
    for text in question_lines {
        lines.push(Line::from(Span::styled(format!("   {text}"), base)));
    }
    let place = if row.project.is_empty() {
        row.title.clone()
    } else {
        format!("{} \u{b7} {}", row.project, row.title)
    };
    lines.push(Line::from(Span::styled(
        format!("   {}", clip(&place, question_width)),
        dim,
    )));
    lines
}

// --------------------------------------------------------------- arrival

/// Where the arrival block goes on a pane whose terminal content is
/// `content_area`: the right end of its top row. `None` when the pane is too
/// narrow to hold a readable block.
pub fn arrival_area(content_area: Rect, text_width: usize) -> Option<Rect> {
    if content_area.height == 0 || content_area.width < MINIMUM_ARRIVAL_WIDTH {
        return None;
    }
    let widest = (content_area.width / ARRIVAL_WIDTH_FRACTION).max(MINIMUM_ARRIVAL_WIDTH);
    let width = u16::try_from(text_width + 2)
        .unwrap_or(u16::MAX)
        .clamp(MINIMUM_ARRIVAL_WIDTH, widest);
    Some(Rect::new(
        content_area.right() - width,
        content_area.y,
        width,
        1,
    ))
}

/// Text of the arrival block: kind, agent title and reason.
pub fn arrival_text(kind: AttentionKind, title: &str, reason: &str) -> String {
    format!(
        "{} {}: {} \u{2013} {}",
        kind_glyph(kind),
        kind.label(),
        title,
        reason
    )
}

/// Draws the arrival block over `content_area`'s top row.
pub fn render_arrival(
    frame: &mut Frame,
    content_area: Rect,
    kind: AttentionKind,
    title: &str,
    reason: &str,
) {
    let text = arrival_text(kind, title, reason);
    let Some(area) = arrival_area(content_area, text.width()) else {
        return;
    };
    let shown = format!(
        " {} ",
        clip(&text, usize::from(area.width).saturating_sub(2))
    );
    let style = theme::selected_style().add_modifier(Modifier::BOLD);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(shown, style))).style(style),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn row(kind: AttentionKind, question: Option<&str>) -> PanelRow {
        PanelRow {
            kind,
            reason: "Waiting for your answer or approval".into(),
            question: question.map(str::to_string),
            project: "ilium".into(),
            title: "build agent".into(),
            age_ms: 125_000,
            is_looked: false,
        }
    }

    fn draw(width: u16, height: u16, draw: impl FnOnce(&mut Frame)) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| draw(frame)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn ages_use_the_largest_whole_unit() {
        assert_eq!(format_age(0), "0s");
        assert_eq!(format_age(59_999), "59s");
        assert_eq!(format_age(125_000), "2m");
        assert_eq!(format_age(7_300_000), "2h");
        assert_eq!(format_age(200_000_000), "2d");
    }

    #[test]
    fn clip_respects_cell_width_and_marks_the_cut() {
        assert_eq!(clip("short", 10), "short");
        assert_eq!(clip("abcdefghij", 5), "abcd\u{2026}");
        assert_eq!(clip("anything", 0), "");
        assert!(clip("\u{1F514}\u{1F514}\u{1F514}", 4).width() <= 4);
    }

    #[test]
    fn wrap_limits_lines_and_ellipsizes_the_overflow() {
        assert_eq!(wrap("one two three", 20, 2), vec!["one two three"]);
        let lines = wrap("alpha beta gamma delta epsilon zeta", 12, 2);
        assert_eq!(lines.len(), 2);
        assert!(lines.iter().all(|line| line.width() <= 12));
        assert!(lines[1].ends_with('\u{2026}'));
        assert!(wrap("text", 0, 2).is_empty());
    }

    #[test]
    fn button_text_fits_every_width_and_shows_the_badge() {
        for width in 1..60 {
            assert!(
                button_text(3, width).width() <= usize::from(width),
                "width {width}"
            );
            assert!(
                button_text(0, width).width() <= usize::from(width),
                "width {width}"
            );
        }
        let wide = button_text(3, 30);
        assert!(wide.contains(BUTTON_LABEL));
        assert!(wide.trim_end().ends_with('3'));
        assert!(!button_text(0, 30).trim_end().ends_with('0'));
        assert!(!button_text(5, 8).contains("Next"));
    }

    #[test]
    fn empty_button_is_dim_and_a_full_one_is_not() {
        let area = Rect::new(0, 0, 30, 1);
        let mut terminal = Terminal::new(TestBackend::new(30, 1)).unwrap();
        terminal
            .draw(|frame| render_button(frame, area, 0))
            .unwrap();
        assert!(terminal.backend().buffer()[(4, 0)]
            .modifier
            .contains(Modifier::DIM));
        terminal
            .draw(|frame| render_button(frame, area, 2))
            .unwrap();
        assert!(!terminal.backend().buffer()[(4, 0)]
            .modifier
            .contains(Modifier::DIM));
    }

    #[test]
    fn panel_layout_fits_whole_entries_and_hit_tests_each_one() {
        let area = Rect::new(10, 0, 50, 17);
        let panel = layout(area);
        assert_eq!(panel.content, theme::block(false).inner(area));
        assert_eq!(
            panel.capacity,
            usize::from((panel.content.height + 1) / ENTRY_HEIGHT)
        );
        assert!(panel.capacity >= 2);
        for slot in 0..panel.capacity {
            let rect = entry_rect(panel, slot);
            assert!(rect.bottom() <= panel.content.bottom());
            let inside = Position::new(rect.x + 1, rect.y + 2);
            assert_eq!(entry_at(panel, inside, 3, 10), Some(3 + slot));
        }
        // The spacer row between entries and the border belong to no entry.
        let spacer = Position::new(panel.content.x, entry_rect(panel, 0).bottom());
        assert_eq!(entry_at(panel, spacer, 0, 10), None);
        assert_eq!(entry_at(panel, Position::new(area.x, area.y), 0, 10), None);
        // Fewer entries than slots: the empty slots are not clickable.
        let below = Position::new(panel.content.x, entry_rect(panel, 1).y);
        assert_eq!(entry_at(panel, below, 0, 1), None);
    }

    #[test]
    fn scrolling_keeps_the_selection_visible() {
        assert_eq!(first_visible(0, 3, 5), 0);
        assert_eq!(first_visible(4, 10, 3), 2);
        assert_eq!(first_visible(9, 10, 3), 7);
        assert_eq!(first_visible(0, 10, 3), 0);
        assert_eq!(first_visible(5, 10, 0), 0);
    }

    #[test]
    fn panel_shows_kind_reason_question_project_and_age() {
        let rows = vec![row(
            AttentionKind::Question,
            Some("Delete the build folder?"),
        )];
        let text = draw(60, 12, |frame| {
            render_panel(frame, Rect::new(0, 0, 60, 12), &rows, 0, true)
        });
        for needle in [
            "Question",
            "Waiting for your answer",
            "Delete the build folder?",
            "ilium",
            "build agent",
            "2m",
        ] {
            assert!(text.contains(needle), "missing {needle:?} in\n{text}");
        }
    }

    #[test]
    fn empty_panel_explains_itself() {
        let text = draw(60, 8, |frame| {
            render_panel(frame, Rect::new(0, 0, 60, 8), &[], 0, false)
        });
        assert!(text.contains("Nothing needs you right now."));
    }

    #[test]
    fn arrival_block_sits_at_the_right_of_the_top_row_and_skips_narrow_panes() {
        let content = Rect::new(2, 3, 80, 20);
        let area = arrival_area(content, 40).unwrap();
        assert_eq!(area.y, content.y);
        assert_eq!(area.height, 1);
        assert_eq!(area.right(), content.right());
        assert!(area.width <= content.width / 2);
        assert!(arrival_area(Rect::new(0, 0, MINIMUM_ARRIVAL_WIDTH - 1, 5), 10).is_none());
        assert!(arrival_area(Rect::new(0, 0, 80, 0), 10).is_none());
        let text = draw(80, 4, |frame| {
            render_arrival(
                frame,
                Rect::new(0, 0, 80, 4),
                AttentionKind::Stuck,
                "deploy",
                "Its goal is blocked",
            );
        });
        assert!(text.contains("Stuck: deploy"), "{text}");
    }
}

//! Responsive document and hit geometry for the automatic Triggers tab.
//!
//! This module is deliberately separate from the general settings renderer:
//! an event owns a variable number of action chips, and those chips wrap at
//! terminal-cell boundaries. Rendering, scrolling, keyboard visibility, and
//! mouse hit-testing all consume the same generated document so narrow-window
//! behavior cannot drift between presentation and interaction.

use ratatui::buffer::CellDiffOption;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState};
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

use crate::app::App;
use crate::icon_settings::{IconSettings, IconTarget};
use crate::theme;
use crate::trigger_settings::{TriggerAction, TriggerEvent};

const DOCUMENT_INSET: u16 = 2;
const EVENT_SPACING: u16 = 1;
const EVENT_FRAME_WIDTH: u16 = 2;

/// One action chip's exact document-space terminal-cell rectangle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TriggerChipHit {
    pub event: TriggerEvent,
    pub action: Option<TriggerAction>,
    pub row: u16,
    pub start_column: u16,
    pub end_column: u16,
}

/// Generated once per render/hit query from the same width and settings.
pub struct TriggerDocument {
    pub lines: Vec<Line<'static>>,
    pub chips: Vec<TriggerChipHit>,
    pub event_rows: Vec<(TriggerEvent, u16, u16)>,
}

/// Renders the aerated event router and its live action chips.
pub fn render(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    selected_event: usize,
    selected_action: usize,
    scroll: u16,
) {
    // A distinct near-black canvas forces a complete repaint when entering
    // this icon-dense tab. Without it, terminal diffing can retain characters
    // underneath VS16 emoji continuation cells from the previous tab.
    frame.render_widget(
        Block::default().style(Style::new().bg(Color::Rgb(3, 3, 3))),
        area,
    );
    let document = build_document(
        app,
        area.width.saturating_sub(1),
        selected_event,
        selected_action,
    );
    let total_lines = document.lines.len();
    frame.render_widget(Paragraph::new(document.lines).scroll((scroll, 0)), area);
    if total_lines > usize::from(area.height) && area.width >= 2 && area.height > 0 {
        let mut state = ScrollbarState::new(total_lines)
            .position(usize::from(scroll))
            .viewport_content_length(usize::from(area.height));
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None)
                .track_symbol(Some("│"))
                .style(theme::border_style(false)),
            Rect::new(area.right(), area.y, 1, area.height),
            &mut state,
        );
    }
    force_repaint_safe_diff(frame.buffer_mut(), area);
}

/// Largest valid vertical document offset for the current terminal width.
pub fn max_scroll(
    app: &App,
    width: u16,
    height: u16,
    selected_event: usize,
    selected_action: usize,
) -> u16 {
    let total = build_document(
        app,
        width.saturating_sub(1),
        selected_event,
        selected_action,
    )
    .lines
    .len() as u16;
    total.saturating_sub(height)
}

/// Keeps the selected event's heading and chips visible after keyboard moves.
pub fn scroll_for_selection(
    app: &App,
    area: Rect,
    selected_event: usize,
    selected_action: usize,
    current_scroll: u16,
) -> u16 {
    let document = build_document(
        app,
        area.width.saturating_sub(1),
        selected_event,
        selected_action,
    );
    let Some((_, start, end)) = document.event_rows.get(selected_event).copied() else {
        return current_scroll;
    };
    if start < current_scroll {
        return start;
    }
    let visible_bottom = current_scroll.saturating_add(area.height.saturating_sub(1));
    if end > visible_bottom {
        // When the event's own block (heading through its last chip row) is
        // taller than the viewport, bottom-anchoring alone would scroll the
        // heading off the top -- clamp to `start` so the heading stays
        // visible even if the block's tail does not fully fit.
        return end.saturating_add(1).saturating_sub(area.height).min(start);
    }
    current_scroll
}

/// Resolves one visible click to the event and chip generated at that cell.
pub fn hit_test(
    app: &App,
    area: Rect,
    scroll: u16,
    position: Position,
    selected_event: usize,
    selected_action: usize,
) -> Option<(TriggerEvent, Option<Option<TriggerAction>>)> {
    if !area.contains(position) {
        return None;
    }
    let document = build_document(
        app,
        area.width.saturating_sub(1),
        selected_event,
        selected_action,
    );
    let row = position.y.saturating_sub(area.y).saturating_add(scroll);
    let column = position.x.saturating_sub(area.x);
    if let Some(chip) = document
        .chips
        .iter()
        .find(|chip| chip.row == row && column >= chip.start_column && column < chip.end_column)
    {
        return Some((chip.event, Some(chip.action)));
    }
    document
        .event_rows
        .iter()
        .find(|(_, start, end)| row >= *start && row <= *end)
        .map(|(event, _, _)| (*event, None))
}

/// Builds the complete styled document plus shared interaction geometry.
pub fn build_document(
    app: &App,
    width: u16,
    selected_event: usize,
    selected_action: usize,
) -> TriggerDocument {
    let usable_width = width.max(1);
    let mut lines = vec![Line::from(Span::styled(
        "AUTOMATION ROUTER",
        Style::new()
            .fg(theme::accent_bg())
            .add_modifier(Modifier::BOLD),
    ))];
    push_wrapped_text(
        &mut lines,
        "Events decide when ilium may spend an LLM call.",
        usable_width,
        0,
        Style::new().add_modifier(Modifier::BOLD),
    );
    push_wrapped_text(
        &mut lines,
        "Choose None or combine retitling with one restructure scope. Changes apply immediately.",
        usable_width,
        0,
        Style::new().add_modifier(Modifier::DIM),
    );
    lines.push(Line::from(""));
    let mut chips = Vec::new();
    let mut event_rows = Vec::new();

    for (event_index, event) in TriggerEvent::ALL.into_iter().enumerate() {
        let start_row = lines.len() as u16;
        let has_frame = usable_width >= EVENT_FRAME_WIDTH + 1;
        let inner_width = if has_frame {
            usable_width - EVENT_FRAME_WIDTH
        } else {
            usable_width
        };
        let mut event_lines = Vec::new();
        let mut event_chips = Vec::new();
        let selected = event_index == selected_event;
        let heading_style = if selected {
            Style::new()
                .fg(theme::accent_bg())
                .add_modifier(Modifier::BOLD)
        } else {
            Style::new().add_modifier(Modifier::BOLD)
        };
        let heading = Line::from(vec![
            Span::raw(format!("{}  ", event_glyph(event, &app.ui_settings.icons))),
            Span::styled(event.label(), heading_style),
            Span::raw("   "),
            Span::styled(
                event.scope_label(),
                Style::new().fg(theme::border_style(false).fg.unwrap_or_default()),
            ),
        ]);
        if heading.width() <= usize::from(inner_width) {
            event_lines.push(heading);
        } else {
            push_wrapped_text(
                &mut event_lines,
                &format!(
                    "{}  {}",
                    event_glyph(event, &app.ui_settings.icons),
                    event.label()
                ),
                inner_width,
                0,
                heading_style,
            );
            push_wrapped_text(
                &mut event_lines,
                event.scope_label(),
                inner_width,
                1,
                Style::new().fg(theme::border_style(false).fg.unwrap_or_default()),
            );
        }
        push_wrapped_text(
            &mut event_lines,
            event.description(),
            inner_width,
            1,
            Style::new().add_modifier(Modifier::DIM),
        );

        let choices = std::iter::once(None)
            .chain(event.available_actions().iter().copied().map(Some))
            .collect::<Vec<_>>();
        let mut current_spans = vec![Span::raw(" ".repeat(usize::from(DOCUMENT_INSET)))];
        let mut current_column = DOCUMENT_INSET;
        let mut current_has_vs16_action = false;
        for (choice_index, action) in choices.into_iter().enumerate() {
            let label = chip_label(action, &app.ui_settings.icons);
            let chip_width =
                u16::try_from(UnicodeWidthStr::width(label.as_str())).unwrap_or(u16::MAX);
            let is_vs16_action = action.is_some() && label.contains('\u{fe0f}');
            let gap = if current_column == DOCUMENT_INSET {
                0
            } else {
                1
            };
            if current_column > DOCUMENT_INSET
                && (current_column
                    .saturating_add(gap)
                    .saturating_add(chip_width)
                    > inner_width
                    || (current_has_vs16_action && is_vs16_action))
            {
                event_lines.push(Line::from(current_spans));
                current_spans = vec![Span::raw(" ".repeat(usize::from(DOCUMENT_INSET)))];
                current_column = DOCUMENT_INSET;
                current_has_vs16_action = false;
            }
            if current_column > DOCUMENT_INSET {
                current_spans.push(Span::raw(" "));
                current_column += 1;
            }
            let is_enabled = action
                .map(|action| app.trigger_settings.is_enabled(event, action))
                .unwrap_or_else(|| app.trigger_settings.actions_for(event).is_empty());
            let is_cursor = selected && choice_index == selected_action;
            let style = if is_cursor {
                theme::selected_style().add_modifier(Modifier::BOLD)
            } else if is_enabled {
                Style::new()
                    .fg(theme::accent_bg())
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::new().add_modifier(Modifier::DIM)
            };
            let row = event_lines.len() as u16;
            event_chips.push(TriggerChipHit {
                event,
                action,
                row,
                start_column: current_column,
                end_column: current_column.saturating_add(chip_width),
            });
            current_spans.push(Span::styled(label, style));
            current_column = current_column.saturating_add(chip_width);
            current_has_vs16_action |= is_vs16_action;
        }
        event_lines.push(Line::from(current_spans));

        if has_frame {
            let border = theme::border_style(selected);
            let horizontal = "─".repeat(usize::from(usable_width - EVENT_FRAME_WIDTH));
            lines.push(Line::from(vec![
                Span::styled("╭", border),
                Span::styled(horizontal.clone(), border),
                Span::styled("╮", border),
            ]));
            for (row, content) in event_lines.into_iter().enumerate() {
                let content_width = u16::try_from(content.width()).unwrap_or(u16::MAX);
                let padding = inner_width.saturating_sub(content_width);
                lines.push(Line::from(
                    std::iter::once(Span::styled("│", border))
                        .chain(content.spans)
                        .chain(std::iter::once(Span::raw(" ".repeat(usize::from(padding)))))
                        .chain(std::iter::once(Span::styled("│", border)))
                        .collect::<Vec<_>>(),
                ));
                for mut chip in event_chips
                    .iter()
                    .filter(|chip| usize::from(chip.row) == row)
                    .cloned()
                {
                    chip.row = start_row + 1 + chip.row;
                    chip.start_column = chip.start_column.saturating_add(1);
                    chip.end_column = chip.end_column.saturating_add(1);
                    chips.push(chip);
                }
            }
            lines.push(Line::from(vec![
                Span::styled("╰", border),
                Span::styled(horizontal, border),
                Span::styled("╯", border),
            ]));
        } else {
            for mut chip in event_chips {
                chip.row = start_row + chip.row;
                chips.push(chip);
            }
            lines.extend(event_lines);
        }
        let end_row = lines.len().saturating_sub(1) as u16;
        event_rows.push((event, start_row, end_row));
        lines.extend((0..EVENT_SPACING).map(|_| Line::from("")));
    }

    TriggerDocument {
        lines,
        chips,
        event_rows,
    }
}

/// Appends cell-width-aware wrapped lines while preserving a visual inset.
fn push_wrapped_text(
    lines: &mut Vec<Line<'static>>,
    text: &str,
    width: u16,
    inset: u16,
    style: Style,
) {
    let inset = inset.min(width.saturating_sub(1));
    let mut current = " ".repeat(usize::from(inset));
    let mut current_width = inset;
    for word in text.split_whitespace() {
        let word_width = u16::try_from(UnicodeWidthStr::width(word)).unwrap_or(u16::MAX);
        let gap = u16::from(current_width > inset);
        if current_width > inset
            && current_width.saturating_add(gap.saturating_add(word_width)) > width
        {
            lines.push(Line::from(Span::styled(current, style)));
            current = " ".repeat(usize::from(inset));
            current_width = inset;
        }
        if current_width > inset {
            current.push(' ');
            current_width = current_width.saturating_add(1);
        }
        current.push_str(word);
        current_width = current_width.saturating_add(word_width);
    }
    lines.push(Line::from(Span::styled(current, style)));
}

/// Makes this bounded icon-dense surface deterministic across VS16 policies.
///
/// Ratatui and tmux may disagree about whether a VS16 sequence advances one or
/// two cells. Emitting the complete settings region whenever a redraw already
/// happens prevents a changed chip style from reusing stale continuation
/// cells. This does not schedule extra frames and is confined to Settings.
fn force_repaint_safe_diff(buffer: &mut ratatui::buffer::Buffer, area: Rect) {
    for row in area.y..area.bottom() {
        for column in area.x..area.right() {
            buffer[(column, row)].set_diff_option(CellDiffOption::AlwaysUpdate);
        }
    }
}

fn event_glyph(event: TriggerEvent, icons: &IconSettings) -> &str {
    let target = match event {
        TriggerEvent::StartupComplete => IconTarget::ToolbarSettings,
        TriggerEvent::AgentSessionReady => IconTarget::OtherAgent,
        TriggerEvent::AgentPromptSubmitted => IconTarget::Terminal,
        TriggerEvent::AgentStartedWorking => IconTarget::Working,
        TriggerEvent::AgentWaitingBackground => IconTarget::WaitingBackground,
        TriggerEvent::AgentApprovalRequired => IconTarget::WaitingApproval,
        TriggerEvent::AgentFinishedWork => IconTarget::Done,
        TriggerEvent::TerminalActivityCheckpoint => IconTarget::Terminal,
    };
    icons.glyph(target)
}

fn chip_label(action: Option<TriggerAction>, icons: &IconSettings) -> String {
    match action {
        None => "[ None ]".to_owned(),
        Some(TriggerAction::RetitleElement) => format!(
            "[ {} {} ]",
            icons.glyph(IconTarget::RowRetitle),
            TriggerAction::RetitleElement.compact_label()
        ),
        Some(TriggerAction::RestructureProject) => format!(
            "[ {} {} ]",
            icons.glyph(IconTarget::RowProjectRestructure),
            TriggerAction::RestructureProject.compact_label()
        ),
        Some(TriggerAction::RestructureAllProjects) => format!(
            "[ {} {} ]",
            icons.glyph(IconTarget::ToolbarRestructure),
            TriggerAction::RestructureAllProjects.compact_label()
        ),
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn overflow_indicator_does_not_replace_trigger_document_text() {
        use ratatui::{backend::TestBackend, Terminal};

        let directory = tempfile::tempdir().unwrap();
        let app = App::new(
            "trigger-scrollbar".to_owned(),
            directory.path().to_path_buf(),
        );
        let area = Rect::new(1, 1, 40, 4);
        let document = build_document(&app, area.width - 1, 0, 0);
        assert!(document.lines.len() > usize::from(area.height));
        let mut reference = Terminal::new(TestBackend::new(42, 6)).unwrap();
        reference
            .draw(|frame| frame.render_widget(Paragraph::new(document.lines.clone()), area))
            .unwrap();
        let mut candidate = Terminal::new(TestBackend::new(42, 6)).unwrap();
        candidate
            .draw(|frame| render(frame, area, &app, 0, 0, 0))
            .unwrap();
        for y in area.y..area.bottom() {
            for x in area.x..area.right() {
                assert_eq!(
                    candidate.backend().buffer()[(x, y)].symbol(),
                    reference.backend().buffer()[(x, y)].symbol(),
                    "document cell {x},{y}"
                );
            }
        }
        assert!((area.y..area.bottom())
            .any(|y| candidate.backend().buffer()[(area.right(), y)].symbol() != " "));
    }

    #[test]
    fn narrow_document_wraps_chips_without_overlapping_hit_rectangles() {
        let app = App::new("test".to_owned(), std::env::temp_dir());
        let document = build_document(&app, 28, 6, 2);
        assert!(document.lines.len() > build_document(&app, 100, 6, 2).lines.len());
        for row in 0..document.lines.len() as u16 {
            let row_hits = document
                .chips
                .iter()
                .filter(|hit| hit.row == row)
                .collect::<Vec<_>>();
            for pair in row_hits.windows(2) {
                assert!(pair[0].end_column <= pair[1].start_column);
            }
        }
    }

    #[test]
    fn trigger_events_use_rounded_sections_at_wide_and_compact_widths() {
        let app = App::new("trigger-framing".to_owned(), std::env::temp_dir());
        for width in [100, 28] {
            let document = build_document(&app, width, 0, 0);
            assert_eq!(document.event_rows.len(), TriggerEvent::ALL.len());
            assert!(document
                .lines
                .iter()
                .all(|line| line.width() <= usize::from(width)));
            for (event, start, end) in &document.event_rows {
                let top = document.lines[usize::from(*start)].to_string();
                let bottom = document.lines[usize::from(*end)].to_string();
                assert!(top.starts_with('╭'), "missing {event:?} top frame: {top:?}");
                assert!(top.ends_with('╮'), "missing {event:?} top frame: {top:?}");
                assert!(
                    bottom.starts_with('╰'),
                    "missing {event:?} bottom frame: {bottom:?}"
                );
                assert!(
                    bottom.ends_with('╯'),
                    "missing {event:?} bottom frame: {bottom:?}"
                );
            }
        }
    }

    #[test]
    fn narrow_document_wraps_every_description_without_clipping_words() {
        let app = App::new("test".to_owned(), std::env::temp_dir());
        let width = 28;
        let document = build_document(&app, width, 0, 0);

        assert!(document
            .lines
            .iter()
            .all(|line| line.width() <= usize::from(width)));
        let rendered = document
            .lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .map(|span| span.content.as_ref())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(rendered.contains("Changes apply immediately."));
        assert!(rendered.contains("terminal"));
        assert!(rendered.contains("replay are loaded."));
    }

    #[test]
    fn vs16_action_icons_never_share_a_physical_row() {
        let app = App::new("test".to_owned(), std::env::temp_dir());
        let document = build_document(&app, 100, 0, 0);

        for row in 0..document.lines.len() as u16 {
            let vs16_actions = document
                .chips
                .iter()
                .filter(|chip| chip.row == row && chip.action.is_some())
                .filter(|chip| chip_label(chip.action, &app.ui_settings.icons).contains('\u{fe0f}'))
                .count();
            assert!(vs16_actions <= 1);
        }
    }

    #[test]
    fn trigger_surface_marks_every_cell_for_safe_repaint() {
        let area = Rect::new(0, 0, 12, 1);
        let mut buffer = ratatui::buffer::Buffer::empty(area);
        buffer.set_string(0, 0, "♻️ action", Style::new());

        force_repaint_safe_diff(&mut buffer, area);

        assert_eq!(buffer[(0, 0)].symbol(), "♻️");
        assert!(buffer
            .content
            .iter()
            .all(|cell| cell.diff_option == CellDiffOption::AlwaysUpdate));
    }

    #[test]
    fn hit_test_returns_the_exact_wrapped_chip() {
        let app = App::new("test".to_owned(), std::env::temp_dir());
        for width in [32, 29] {
            let area = Rect::new(10, 4, width, 12);
            let document = build_document(&app, area.width - 1, 0, 0);
            let chip = document
                .chips
                .iter()
                .find(|chip| {
                    chip.event == TriggerEvent::AgentFinishedWork
                        && chip.action == Some(TriggerAction::RestructureProject)
                })
                .copied()
                .unwrap();
            let scroll = chip.row.saturating_sub(3);
            let hit = hit_test(
                &app,
                area,
                scroll,
                Position::new(area.x + chip.start_column, area.y + chip.row - scroll),
                0,
                0,
            );
            assert_eq!(
                hit,
                Some((
                    TriggerEvent::AgentFinishedWork,
                    Some(Some(TriggerAction::RestructureProject))
                )),
                "framed chip click at width {width}"
            );
        }
    }
}

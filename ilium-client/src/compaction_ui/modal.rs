//! The Apply confirmation: a modal over the whole Settings screen that shows
//! the resolved file, the key, old -> new value, the diff text, the plan's
//! shadowing warnings and the "new sessions only" sentence. Built on the
//! shared dialog pieces of [`crate::modal`] (frame, action buttons, hit
//! testing); only the text body is specific.

use ratatui::layout::{Constraint, Direction, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState};
use ratatui::Frame;

use super::text::wrap_text;
use super::{dim, warning};
use crate::agent_config_writer::AgentConfigTarget;
use crate::compaction_app::{agent_label, PendingApply};
use crate::compaction_report::group_thousands;
use crate::modal::{
    centered_fixed_rect, dialog_action_layout, inset_rect, render_dialog_actions, DialogAction,
    DialogActionLayout, DialogActions,
};
use crate::theme;

const MODAL_WIDTH: u16 = 100;
const MODAL_HEIGHT: u16 = 30;

/// Geometry of the confirmation, shared by drawing, scrolling and clicks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ModalLayout {
    pub popup: Rect,
    pub text_area: Rect,
    pub scrollbar_area: Rect,
    pub actions: DialogActionLayout,
    pub hint_row: Rect,
}

pub(crate) fn modal_layout(screen: Rect) -> ModalLayout {
    let popup = centered_fixed_rect(
        MODAL_WIDTH.min(screen.width.saturating_sub(2)),
        MODAL_HEIGHT.min(screen.height.saturating_sub(2)),
        screen,
    );
    let inner = inset_rect(popup, 1);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(inner);
    let scrollbar_width = u16::from(rows[0].width >= 2);
    let text_area = Rect {
        width: rows[0].width.saturating_sub(scrollbar_width),
        ..rows[0]
    };
    ModalLayout {
        popup,
        text_area,
        scrollbar_area: Rect::new(
            text_area.right(),
            text_area.y,
            scrollbar_width,
            text_area.height,
        ),
        actions: dialog_action_layout(rows[2]),
        hint_row: rows[3],
    }
}

/// The confirmation text at `width` columns.
pub(crate) fn modal_lines(pending: &PendingApply, width: u16) -> Vec<Line<'static>> {
    let plan = &pending.plan;
    let width = usize::from(width).saturating_sub(1).max(20);
    let agent = agent_label(pending.agent);
    let mut lines: Vec<Line<'static>> = Vec::new();
    const LABEL_WIDTH: usize = 8;
    let field = |lines: &mut Vec<Line<'static>>, label: &str, value: &str, style: Style| {
        let rows = wrap_text(value, width.saturating_sub(LABEL_WIDTH));
        for (index, row) in rows.into_iter().enumerate() {
            let head = if index == 0 {
                format!("{label:<LABEL_WIDTH$}")
            } else {
                " ".repeat(LABEL_WIDTH)
            };
            lines.push(Line::from(vec![
                Span::styled(head, dim()),
                Span::styled(row, style),
            ]));
        }
    };
    field(
        &mut lines,
        "Agent",
        agent,
        Style::new().add_modifier(Modifier::BOLD),
    );
    field(
        &mut lines,
        "File",
        &plan.path.display().to_string(),
        Style::new(),
    );
    field(&mut lines, "Key", plan.target.key(), Style::new());
    field(
        &mut lines,
        "Change",
        &format!(
            "{} -> {}",
            plan.old_value
                .map_or_else(|| "not set".to_owned(), group_thousands),
            group_thousands(plan.new_value)
        ),
        Style::new().add_modifier(Modifier::BOLD),
    );
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled("Diff", dim())));
    for row in plan.diff.lines() {
        let style = if row.starts_with('+') {
            Style::new().fg(Color::Green)
        } else if row.starts_with('-') {
            Style::new().fg(Color::Red)
        } else {
            dim()
        };
        for piece in wrap_text(row, width.saturating_sub(2)) {
            let piece = if row.starts_with(['+', '-']) && !piece.starts_with(['+', '-']) {
                format!("  {piece}")
            } else {
                piece
            };
            lines.push(Line::from(Span::styled(format!("  {piece}"), style)));
        }
    }
    if let Some(support) = &pending.extrapolated {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Extrapolated value",
            warning().add_modifier(Modifier::BOLD),
        )));
        for row in wrap_text(
            &format!(
                "This value is EXTRAPOLATED ({support}). The simulation extends beyond your observed compactions, so the saving is a model result, not a measurement."
            ),
            width,
        ) {
            lines.push(Line::from(Span::styled(row, warning())));
        }
    }
    if !plan.warnings.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Warnings",
            warning().add_modifier(Modifier::BOLD),
        )));
        for warning_item in &plan.warnings {
            for (index, row) in wrap_text(&warning_item.message(), width.saturating_sub(2))
                .into_iter()
                .enumerate()
            {
                lines.push(Line::from(Span::styled(
                    format!("{}{row}", if index == 0 { "- " } else { "  " }),
                    warning(),
                )));
            }
        }
    }
    lines.push(Line::from(""));
    for row in wrap_text(
        &format!(
            "This only affects NEW {agent} sessions: running sessions keep the limit they already loaded. Ilium does not restart anything. A revert record is kept, so this tab can restore the previous value."
        ),
        width,
    ) {
        lines.push(Line::from(Span::raw(row)));
    }
    lines
}

fn text_height(pending: &PendingApply, screen: Rect) -> (usize, u16) {
    let layout = modal_layout(screen);
    let lines = modal_lines(pending, layout.text_area.width).len();
    (lines, layout.text_area.height)
}

/// Largest scroll of the confirmation text.
pub(crate) fn modal_max_scroll(pending: &PendingApply, screen: Rect) -> u16 {
    let (lines, height) = text_height(pending, screen);
    lines
        .saturating_sub(usize::from(height))
        .min(usize::from(u16::MAX)) as u16
}

/// Lines a page key scrolls.
pub(crate) fn modal_page_height(screen: Rect) -> u16 {
    modal_layout(screen)
        .text_area
        .height
        .saturating_sub(1)
        .max(1)
}

/// The button under `position`.
pub(crate) fn modal_action_at(screen: Rect, position: Position) -> Option<DialogAction> {
    modal_layout(screen).actions.action_at(position)
}

/// Draws the confirmation when one is pending; draws nothing otherwise.
pub(crate) fn render(frame: &mut Frame, screen: Rect, app: &crate::app::App) {
    let Some(pending) = app.optimization.pending_apply.as_ref() else {
        return;
    };
    let layout = modal_layout(screen);
    frame.render_widget(Clear, layout.popup);
    let title = format!(
        "Apply to {}?",
        match pending.plan.target {
            AgentConfigTarget::ClaudeAutoCompactWindow => "Claude Code",
            AgentConfigTarget::CodexAutoCompactTokenLimit => "Codex",
        }
    );
    let block = theme::block(true)
        .title(theme::chrome_title(&title))
        .border_style(Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD));
    frame.render_widget(block, layout.popup);
    let lines = modal_lines(pending, layout.text_area.width);
    let total_lines = lines.len();
    let scroll = pending.scroll.min(modal_max_scroll(pending, screen));
    frame.render_widget(Paragraph::new(lines).scroll((scroll, 0)), layout.text_area);
    if total_lines > usize::from(layout.text_area.height) && !layout.scrollbar_area.is_empty() {
        let mut state = ScrollbarState::new(total_lines)
            .position(usize::from(scroll))
            .viewport_content_length(usize::from(layout.text_area.height));
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None)
                .track_symbol(Some("│"))
                .style(theme::border_style(false)),
            layout.scrollbar_area,
            &mut state,
        );
    }
    render_dialog_actions(
        frame,
        layout.actions,
        DialogActions::form("Cancel", "Apply"),
    );
    frame.render_widget(
        Paragraph::new("Enter or Y applies · Esc or N cancels · ↑/↓ scroll")
            .style(dim())
            .alignment(ratatui::layout::Alignment::Center),
        layout.hint_row,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirmation_scrollbar_has_its_own_column_without_covering_actions() {
        for (width, height) in [(120, 40), (80, 24), (40, 12), (12, 6), (1, 1)] {
            let screen = Rect::new(0, 0, width, height);
            let layout = modal_layout(screen);
            assert_eq!(layout.text_area.right(), layout.scrollbar_area.x);
            assert_eq!(layout.text_area.y, layout.scrollbar_area.y);
            assert_eq!(layout.text_area.height, layout.scrollbar_area.height);
            assert!(layout.scrollbar_area.right() <= screen.right());
            assert!(layout.scrollbar_area.bottom() <= screen.bottom());
            if !layout.text_area.is_empty() {
                assert!(layout.text_area.bottom() <= layout.hint_row.y);
            }
        }
    }
}

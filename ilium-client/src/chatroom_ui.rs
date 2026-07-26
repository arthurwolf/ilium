//! Chatroom right-panel rendering.
//!
//! The room is a project-local Markdown log rather than a server-owned pane,
//! so this module renders its cached records and live project participants
//! without introducing a second IPC state model.

use ilium_core::NodeId;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap};
use ratatui::Frame;

use crate::app::{App, FocusTarget};
use crate::chatroom::ChatMessage;
use crate::theme;

/// Shared geometry for chatroom rendering and pointer hit testing.
///
/// Keeping the message viewport and scrollbar track in one contract prevents
/// mouse navigation from drifting away from the wrapped rows drawn on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChatroomLayout {
    pub messages_area: Rect,
    pub message_content_area: Rect,
    pub message_text_area: Rect,
    pub scrollbar_area: Rect,
    composer_area: Rect,
    participants_area: Rect,
}

/// Wrapped-line scroll limits for one room at its current rendered size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChatroomScrollMetrics {
    pub total_lines: usize,
    pub visible_lines: usize,
    pub maximum_top: usize,
    pub top: usize,
}

impl ChatroomScrollMetrics {
    /// True when some history is outside the current message viewport.
    pub const fn is_overflowing(self) -> bool {
        self.maximum_top > 0
    }
}

/// Computes the responsive room regions and reserves one inner column for
/// the scrollbar, so message text never renders underneath its thumb.
pub fn layout(area: Rect) -> ChatroomLayout {
    let (messages_area, composer_area, participants_area) = if area.width < 70 {
        // A hovered/focused sidebar can temporarily make the right panel
        // narrow. Stack the companion list instead of compressing each
        // message into single-word columns.
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(6),
                Constraint::Length(5),
                Constraint::Length(3),
            ])
            .split(area);
        (rows[0], rows[2], rows[1])
    } else {
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(72), Constraint::Percentage(28)])
            .split(area);
        let left = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(4), Constraint::Length(3)])
            .split(columns[0]);
        (left[0], left[1], columns[1])
    };
    let message_content_area = theme::block(false).inner(messages_area);
    let scrollbar_width = u16::from(message_content_area.width > 1);
    let message_text_area = Rect::new(
        message_content_area.x,
        message_content_area.y,
        message_content_area.width.saturating_sub(scrollbar_width),
        message_content_area.height,
    );
    let scrollbar_area = Rect::new(
        message_text_area.right(),
        message_content_area.y,
        scrollbar_width,
        message_content_area.height,
    );
    ChatroomLayout {
        messages_area,
        message_content_area,
        message_text_area,
        scrollbar_area,
        composer_area,
        participants_area,
    }
}

/// Measures the exact wrapped message rows used by rendering and resolves the
/// stored distance from the live tail into a top-line paragraph offset.
pub fn scroll_metrics(
    area: Rect,
    messages: &[ChatMessage],
    scroll_from_newest: usize,
) -> ChatroomScrollMetrics {
    let room_layout = layout(area);
    let paragraph = Paragraph::new(message_lines(messages)).wrap(Wrap { trim: false });
    let total_lines = paragraph.line_count(room_layout.message_text_area.width);
    metrics_from_line_count(
        total_lines,
        room_layout.message_text_area.height,
        scroll_from_newest,
    )
}

/// Maps one pointer row on the visible scrollbar track to a paragraph top
/// offset. Rows outside the track clamp to its nearest end during a drag.
pub fn scrollbar_top_for_row(scrollbar_area: Rect, maximum_top: usize, pointer_row: u16) -> usize {
    if maximum_top == 0 || scrollbar_area.height <= 1 {
        return 0;
    }
    let last_row = scrollbar_area.bottom().saturating_sub(1);
    let relative_row =
        usize::from(pointer_row.clamp(scrollbar_area.y, last_row) - scrollbar_area.y);
    let denominator = usize::from(scrollbar_area.height - 1);
    relative_row
        .saturating_mul(maximum_top)
        .saturating_add(denominator / 2)
        / denominator
}

/// Draws a project's file-backed conversation, active agent list, and user
/// composer. Messages stay text-first so the corresponding `CHATROOM.md`
/// remains useful outside ilium too.
pub fn render(frame: &mut Frame, area: Rect, app: &App, project_id: NodeId) {
    let room_layout = layout(area);
    let focused = matches!(app.focus, FocusTarget::Pane);
    let room = app.chatrooms.get(&project_id);
    let message_lines = room
        .map(|state| message_lines(&state.messages))
        .unwrap_or_else(|| {
            vec![Line::from(Span::styled(
                "Loading chatroom…",
                Style::new().add_modifier(Modifier::DIM),
            ))]
        });
    let title = app
        .tree
        .get(project_id)
        .map(|node| format!("💬 Chatroom · {}", node.name))
        .unwrap_or_else(|| "💬 Chatroom".to_string());
    let message_block = theme::block(focused).title(theme::chrome_title(&title));
    frame.render_widget(message_block, room_layout.messages_area);
    let message_paragraph = Paragraph::new(message_lines).wrap(Wrap { trim: false });
    let metrics = metrics_from_line_count(
        message_paragraph.line_count(room_layout.message_text_area.width),
        room_layout.message_text_area.height,
        room.map(|state| state.scroll_from_newest)
            .unwrap_or_default(),
    );
    frame.render_widget(
        message_paragraph.scroll((u16::try_from(metrics.top).unwrap_or(u16::MAX), 0)),
        room_layout.message_text_area,
    );
    draw_scrollbar(frame, room_layout, metrics, focused);

    let draft = room.map(|state| state.draft.as_str()).unwrap_or_default();
    frame.render_widget(
        Paragraph::new(vec![Line::from(vec![
            Span::styled("<user> ", Style::new().add_modifier(Modifier::BOLD)),
            Span::raw(draft),
        ])])
        .block(theme::block(focused).title(theme::chrome_title("Write message · Enter sends"))),
        room_layout.composer_area,
    );

    let participants = app.chatroom_participants(project_id);
    let participant_lines = if participants.is_empty() {
        vec![Line::from(Span::styled(
            "No live agent panes in this project.",
            Style::new().add_modifier(Modifier::DIM),
        ))]
    } else {
        participants
            .iter()
            .map(|participant| Line::from(format!("● {participant}")))
            .collect()
    };
    frame.render_widget(
        Paragraph::new(participant_lines)
            .wrap(Wrap { trim: true })
            .block(theme::block(false).title(theme::chrome_title("Agents in this project"))),
        room_layout.participants_area,
    );
}

/// Produces the styled logical records that both rendering and wrapped-line
/// measurement consume.
fn message_lines(messages: &[ChatMessage]) -> Vec<Line<'static>> {
    if messages.is_empty() {
        return vec![Line::from(Span::styled(
            "No messages yet. Type below to post as user.",
            Style::new().add_modifier(Modifier::DIM),
        ))];
    }
    messages
        .iter()
        .flat_map(|message| {
            [
                Line::from(vec![
                    Span::styled(
                        format!("{} ", message.timestamp),
                        Style::new().add_modifier(Modifier::DIM),
                    ),
                    Span::styled(
                        format!("<{}>", message.author),
                        Style::new().add_modifier(Modifier::BOLD),
                    ),
                ]),
                Line::from(format!("  {}", message.content)),
                Line::default(),
            ]
        })
        .collect()
}

/// Resolves a tail-relative scroll value into the exact top row while keeping
/// every arithmetic operation bounded for tiny terminal areas.
fn metrics_from_line_count(
    total_lines: usize,
    visible_height: u16,
    scroll_from_newest: usize,
) -> ChatroomScrollMetrics {
    let visible_lines = usize::from(visible_height);
    let maximum_top = total_lines.saturating_sub(visible_lines);
    let bounded_from_newest = scroll_from_newest.min(maximum_top);
    ChatroomScrollMetrics {
        total_lines,
        visible_lines,
        maximum_top,
        top: maximum_top.saturating_sub(bounded_from_newest),
    }
}

/// Draws a high-contrast inner scrollbar only when history exceeds the
/// viewport. The state models rows rather than message records because one
/// record can wrap to several terminal lines.
fn draw_scrollbar(
    frame: &mut Frame,
    room_layout: ChatroomLayout,
    metrics: ChatroomScrollMetrics,
    focused: bool,
) {
    if !metrics.is_overflowing() || room_layout.scrollbar_area.is_empty() {
        return;
    }
    let mut scrollbar_state = ScrollbarState::new(metrics.maximum_top.saturating_add(1))
        .position(metrics.top)
        .viewport_content_length(metrics.visible_lines);
    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .begin_symbol(None)
        .end_symbol(None)
        .track_symbol(Some("│"))
        .thumb_symbol("█")
        .style(theme::border_style(focused));
    frame.render_stateful_widget(scrollbar, room_layout.scrollbar_area, &mut scrollbar_state);
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    use super::*;
    use crate::app::{App, ChatroomViewState, RightPanelTarget};

    fn messages(count: usize) -> Vec<ChatMessage> {
        (0..count)
            .map(|index| ChatMessage {
                timestamp: format!("2026-07-26 12:{index:02}:00 +02:00"),
                author: "agent:codex".to_string(),
                content: format!("unique chatroom message {index:02}"),
            })
            .collect()
    }

    #[test]
    fn overflowing_room_draws_a_scrollbar_and_starts_at_the_live_tail() {
        let mut app = App::new("test".to_string(), PathBuf::from("/tmp"));
        let project_id = app
            .tree
            .add_project(PathBuf::from("/tmp/chatroom-scroll-render"))
            .unwrap();
        app.chatrooms.insert(
            project_id,
            ChatroomViewState {
                messages: messages(20),
                ..ChatroomViewState::default()
            },
        );
        app.right_panel_target = RightPanelTarget::Chatroom { project_id };
        app.set_screen_area(Rect::new(0, 0, 100, 18));
        let area = app.layout.pane_area;
        let room_layout = layout(area);
        let mut terminal = Terminal::new(TestBackend::new(100, 18)).unwrap();

        terminal
            .draw(|frame| render(frame, area, &app, project_id))
            .unwrap();

        let buffer = terminal.backend().buffer();
        assert!(
            (room_layout.scrollbar_area.y..room_layout.scrollbar_area.bottom())
                .any(|row| buffer[(room_layout.scrollbar_area.x, row)].symbol() == "█")
        );
        let rendered = buffer
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("unique chatroom message 19"));
        assert!(!rendered.contains("unique chatroom message 00"));
    }

    #[test]
    fn tail_relative_offset_resolves_to_oldest_and_newest_viewports() {
        let area = Rect::new(0, 0, 90, 16);
        let messages = messages(20);

        let newest = scroll_metrics(area, &messages, 0);
        let oldest = scroll_metrics(area, &messages, usize::MAX);

        assert!(newest.is_overflowing());
        assert_eq!(newest.top, newest.maximum_top);
        assert_eq!(oldest.top, 0);
    }

    #[test]
    fn scrollbar_track_rows_map_to_bounded_history_positions() {
        let area = Rect::new(10, 5, 1, 11);

        assert_eq!(scrollbar_top_for_row(area, 80, 0), 0);
        assert_eq!(scrollbar_top_for_row(area, 80, 5), 0);
        assert_eq!(scrollbar_top_for_row(area, 80, 10), 40);
        assert_eq!(scrollbar_top_for_row(area, 80, 15), 80);
        assert_eq!(scrollbar_top_for_row(area, 80, 99), 80);
    }
}

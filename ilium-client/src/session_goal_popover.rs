//! Local, non-modal goal preview. Goal content is never logged or persisted.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ilium_core::NodeId;
use ratatui::{
    layout::{Position, Rect},
    style::{Modifier, Style},
    text::Line,
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

use crate::{
    agent_toolbar::{self, AgentToolbarAction, ToolbarContext},
    app::{App, Mode, RightPanelTarget},
};

pub(crate) struct GoalPopover {
    pane: NodeId,
    pinned: bool,
    focused: bool,
    scroll: usize,
}

impl App {
    fn goal_ui_active(&self) -> bool {
        self.modal_stack.is_empty()
            && matches!(self.mode, Mode::Normal)
            && !matches!(
                self.right_panel_target,
                RightPanelTarget::Chatroom { .. }
                    | RightPanelTarget::Resource { .. }
                    | RightPanelTarget::Attention
            )
    }

    fn goal_flag_rect(&self, pane: NodeId) -> Option<Rect> {
        if !self.ui_settings.goal_toolbar_icon_enabled
            || self.current_goal_phase(pane).is_none()
            || !self.shows_agent_toolbar(pane)
        {
            return None;
        }
        let area = self.pane_viewport(pane)?.toolbar_area?;
        let context = ToolbarContext {
            provider: self.agent_toolbar_provider(pane),
            icons: &self.ui_settings.icons,
            effort: self
                .agent_toolbar_effort
                .get(&pane)
                .copied()
                .unwrap_or_default(),
            show_labels: self.ui_settings.show_toolbar_labels,
            selection_enabled: self.ui_settings.terminal_text_selection_enabled,
            show_prompt_reopen: self.last_prompt_reopen_available(pane),
            show_goal: true,
        };
        agent_toolbar::button_rect_for(area, context, AgentToolbarAction::Goal)
    }

    fn goal_popover_geometry(&self, pane: NodeId) -> Option<Rect> {
        let flag = self.goal_flag_rect(pane)?;
        let bounds = self.layout.pane_area;
        let width = bounds.width.min(56);
        let height = bounds.height.min(18);
        if width < 4 || height < 3 {
            return None;
        }
        let x = flag
            .right()
            .saturating_sub(width)
            .max(bounds.x)
            .min(bounds.right().saturating_sub(width));
        let below = flag.bottom();
        let y = if below.saturating_add(height) <= bounds.bottom() {
            below
        } else {
            flag.y.saturating_sub(height).max(bounds.y)
        };
        Some(Rect::new(x, y, width, height))
    }

    pub(crate) fn toggle_goal_popover(&mut self, pane: NodeId) {
        if self
            .goal_popover
            .as_ref()
            .is_some_and(|p| p.pane == pane && p.pinned)
        {
            self.goal_popover = None;
        } else if self.goal_flag_rect(pane).is_some() {
            self.goal_popover = Some(GoalPopover {
                pane,
                pinned: true,
                focused: true,
                scroll: 0,
            });
        }
    }

    pub(crate) fn handle_goal_popover_mouse(
        &mut self,
        mouse: MouseEvent,
        position: Position,
    ) -> bool {
        if !self.goal_ui_active() {
            return false;
        }
        if let Some(pane) = self.goal_popover.as_ref().map(|p| p.pane) {
            let Some(rect) = self.goal_popover_geometry(pane) else {
                self.goal_popover = None;
                return false;
            };
            if rect.contains(position) {
                if let Some(popover) = self.goal_popover.as_mut() {
                    match mouse.kind {
                        MouseEventKind::ScrollUp => {
                            popover.scroll = popover.scroll.saturating_sub(3)
                        }
                        MouseEventKind::ScrollDown => {
                            popover.scroll = popover.scroll.saturating_add(3)
                        }
                        MouseEventKind::Down(MouseButton::Left) => {
                            popover.pinned = true;
                            popover.focused = true;
                        }
                        _ => {}
                    }
                }
                return true;
            }
        }
        let pane = self.pane_viewport_at(position).map(|view| view.pane_id);
        if let Some(pane) = pane.filter(|pane| {
            self.goal_flag_rect(*pane)
                .is_some_and(|rect| rect.contains(position))
        }) {
            match mouse.kind {
                MouseEventKind::Down(MouseButton::Left) => {
                    self.toggle_goal_popover(pane);
                    return true;
                }
                MouseEventKind::Moved if self.goal_popover.as_ref().is_none_or(|p| !p.pinned) => {
                    self.goal_popover = Some(GoalPopover {
                        pane,
                        pinned: false,
                        focused: false,
                        scroll: 0,
                    });
                }
                _ => {}
            }
            return false;
        }
        if self.goal_popover.as_ref().is_some_and(|p| !p.pinned) {
            self.goal_popover = None;
        }
        if matches!(mouse.kind, MouseEventKind::Down(_)) {
            if let Some(popover) = self.goal_popover.as_mut() {
                popover.focused = false;
            }
        }
        false
    }

    pub(crate) fn handle_goal_popover_key(&mut self, key: KeyEvent) -> bool {
        if !self.goal_ui_active() {
            return false;
        }
        if self
            .goal_popover
            .as_ref()
            .is_some_and(|p| self.goal_popover_geometry(p.pane).is_none())
        {
            self.goal_popover = None;
            return false;
        }
        let Some(popover) = self.goal_popover.as_mut().filter(|p| p.pinned && p.focused) else {
            return false;
        };
        if !key.modifiers.is_empty() && key.modifiers != KeyModifiers::SHIFT {
            return false;
        }
        match key.code {
            KeyCode::Esc => self.goal_popover = None,
            KeyCode::Up => popover.scroll = popover.scroll.saturating_sub(1),
            KeyCode::Down => popover.scroll = popover.scroll.saturating_add(1),
            KeyCode::PageUp => popover.scroll = popover.scroll.saturating_sub(10),
            KeyCode::PageDown => popover.scroll = popover.scroll.saturating_add(10),
            KeyCode::Home => popover.scroll = 0,
            KeyCode::End => popover.scroll = usize::MAX,
            _ => {
                popover.focused = false;
                return false;
            }
        }
        true
    }

    pub(crate) fn draw_goal_popover(&mut self, frame: &mut Frame) {
        let Some(mut popover) = self.goal_popover.take() else {
            return;
        };
        let Some(area) = self.goal_popover_geometry(popover.pane) else {
            return;
        };
        let (header, body) = self
            .goal_observation(popover.pane)
            .map(crate::session_goal_display::goal_detail_parts)
            .unwrap_or_else(|| {
                (
                    self.current_goal_phase(popover.pane)
                        .map_or(
                            "Status unavailable",
                            crate::session_goal_display::phase_label,
                        )
                        .to_owned(),
                    Some("Goal text unavailable"),
                )
            });
        let visible = usize::from(area.height.saturating_sub(2));
        let (scroll, rows) = crate::session_goal_display::wrap_goal_detail(
            &header,
            body,
            area.width.saturating_sub(2),
            popover.scroll,
            visible,
        );
        popover.scroll = scroll;
        let lines: Vec<Line<'_>> = rows.iter().map(|row| Line::raw(row.as_str())).collect();
        frame.render_widget(Clear, area);
        let title = if popover.pinned {
            " Goal · Esc closes "
        } else {
            " Goal "
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(Style::default().add_modifier(Modifier::BOLD));
        frame.render_widget(Paragraph::new(lines).block(block), area);
        self.goal_popover = Some(popover);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{FocusTarget, PaneRuntime, RightPanelTarget};
    use crate::terminal_view::TerminalView;
    use crossterm::event::KeyModifiers;
    use ilium_core::{AgentActivity, AgentClass, GoalState, PaneContentKind, PaneStatus, ROOT_ID};

    #[test]
    fn flag_preview_pinning_and_keyboard_focus_follow_current_goal() {
        let mut app = App::new("test-session".to_owned(), std::env::temp_dir());
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        let pane = app
            .tree
            .add_pane(group, "agent", PaneContentKind::Terminal)
            .unwrap();
        app.tree
            .set_pane_status(
                pane,
                PaneStatus::from_activity(
                    AgentClass::Claude,
                    AgentActivity::Idle,
                    Some(GoalState::Paused),
                ),
            )
            .unwrap();
        app.panes.insert(
            pane,
            PaneRuntime::Terminal(Box::new(TerminalView::new(24, 80))),
        );
        app.right_panel_target = RightPanelTarget::Pane { pane_id: pane };
        app.focus = FocusTarget::Pane;
        app.set_screen_area(Rect::new(0, 0, 140, 44));
        let flag = app.goal_flag_rect(pane).unwrap();
        let position = Position::new(flag.x, flag.y);
        let event = |kind| MouseEvent {
            kind,
            column: position.x,
            row: position.y,
            modifiers: KeyModifiers::NONE,
        };
        app.handle_goal_popover_mouse(event(MouseEventKind::Moved), position);
        assert!(app.goal_popover.as_ref().is_some_and(|p| !p.pinned));
        assert!(
            app.handle_goal_popover_mouse(event(MouseEventKind::Down(MouseButton::Left)), position)
        );
        assert!(app
            .goal_popover
            .as_ref()
            .is_some_and(|p| p.pinned && p.focused));
        assert!(app.handle_goal_popover_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)));
        assert!(!app.handle_goal_popover_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE)));
        app.ui_settings.goal_toolbar_icon_enabled = false;
        assert!(!app.handle_goal_popover_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)));
        assert!(app.goal_popover.is_none());
    }
}

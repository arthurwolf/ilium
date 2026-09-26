//! App-side behaviour of the costs-and-stats popover: opening it from the
//! second header icon, pinning it, routing the pointer while it is open, and
//! keeping its data fresh.
//!
//! Hovering the icon shows a preview; clicking it pins the popover (and shows
//! its close control); clicking the icon again, or the close control, closes
//! it. The popover is deliberately not a modal `Mode`: a pinned popover is a
//! live dashboard the user keeps open while the agent works, so keyboard input
//! must keep reaching the agent. Only pointer events over the popover itself
//! are claimed.

use std::time::{Duration, Instant};

use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ilium_core::NodeId;
use ratatui::layout::Position;

use crate::app::{App, Mode, RightPanelTarget};
use crate::session_stats_store::StatsRequest;
use crate::session_stats_ui::{geometry, StatsGeometry, StatsHit, StatsPopover};
use crate::theme;

/// The clock and load spinner redraw at this cadence while a popover is open.
const CLOCK_REDRAW_INTERVAL: Duration = Duration::from_secs(1);
/// Rows one wheel notch scrolls.
const WHEEL_ROWS: i32 = 3;

impl App {
    /// Whether popovers may currently be shown or interacted with. Any modal
    /// dialog covers them, and the chatroom replaces the pane surface.
    fn stats_ui_active(&self) -> bool {
        self.modal_stack.is_empty()
            && matches!(self.mode, Mode::Normal)
            && !matches!(self.right_panel_target, RightPanelTarget::Chatroom { .. })
    }

    /// The pane whose second header icon sits under `position`, when that
    /// pane is a detected agent (the only kind with statistics to show).
    pub fn stats_icon_pane_at(&self, position: Position) -> Option<NodeId> {
        let viewport = self.pane_viewport_at(position)?;
        let is_icon = theme::chrome_stats_cell(viewport.outer_area) == position;
        (is_icon && self.is_detected_agent_pane(viewport.pane_id)).then_some(viewport.pane_id)
    }

    /// Geometry of the open popover, `None` when none is open or its pane is
    /// off screen.
    pub fn stats_popover_geometry(&self) -> Option<StatsGeometry> {
        let popover = self.stats_popover.as_ref()?;
        let viewport = self.pane_viewport(popover.pane_id)?;
        geometry(
            self.layout.pane_area,
            theme::chrome_stats_cell(viewport.outer_area),
        )
    }

    pub fn close_stats_popover(&mut self) {
        self.stats_popover = None;
    }

    /// Claims pointer events aimed at the popover or its header icon. Returns
    /// `true` when the event was consumed and must not reach the tree, the
    /// pane, or the agent's terminal.
    pub fn handle_stats_popover_mouse(&mut self, mouse: MouseEvent, position: Position) -> bool {
        if !self.stats_ui_active() {
            return false;
        }
        let is_left_press = matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left));

        if let Some(layout) = self.stats_popover_geometry() {
            if let Some(hit) = layout.hit(position) {
                self.route_popover_hit(mouse.kind, hit);
                return true;
            }
            if let Some(popover) = self.stats_popover.as_mut() {
                popover.hovered = None;
            }
        }

        if let Some(pane_id) = self.stats_icon_pane_at(position) {
            if is_left_press {
                self.toggle_pinned_stats_popover(pane_id);
                return true;
            }
            if matches!(mouse.kind, MouseEventKind::Moved)
                && self
                    .stats_popover
                    .as_ref()
                    .is_none_or(|popover| !popover.pinned)
            {
                self.open_stats_popover(pane_id, false);
            }
            // Movement over the icon still reaches the ordinary hover
            // handling so no other hover state is left stale.
            return false;
        }

        // A hover preview lives only while the pointer is over the icon or
        // the popover; a pinned one stays until it is closed.
        if self
            .stats_popover
            .as_ref()
            .is_some_and(|popover| !popover.pinned)
        {
            self.stats_popover = None;
        }
        false
    }

    fn route_popover_hit(&mut self, kind: MouseEventKind, hit: StatsHit) {
        let Some(popover) = self.stats_popover.as_mut() else {
            return;
        };
        match kind {
            MouseEventKind::Moved => popover.hovered = Some(hit),
            MouseEventKind::ScrollUp => popover.scroll_by(-WHEEL_ROWS),
            MouseEventKind::ScrollDown => popover.scroll_by(WHEEL_ROWS),
            MouseEventKind::Down(MouseButton::Left) => match hit {
                StatsHit::Close => self.stats_popover = None,
                StatsHit::Tab(tab) => {
                    popover.pinned = true;
                    popover.select_tab(tab);
                }
                // Clicking a hover preview keeps it, as a pinned window.
                StatsHit::Body => popover.pinned = true,
            },
            _ => {}
        }
    }

    fn open_stats_popover(&mut self, pane_id: NodeId, pinned: bool) {
        match self.stats_popover.as_mut() {
            Some(popover) if popover.pane_id == pane_id => popover.pinned |= pinned,
            _ => {
                self.stats_popover = Some(StatsPopover::new(pane_id, pinned, Instant::now()));
            }
        }
    }

    /// The icon's click: pin a fresh or previewed popover, close a pinned one.
    fn toggle_pinned_stats_popover(&mut self, pane_id: NodeId) {
        let is_pinned_here = self
            .stats_popover
            .as_ref()
            .is_some_and(|popover| popover.pane_id == pane_id && popover.pinned);
        if is_pinned_here {
            self.stats_popover = None;
        } else {
            self.open_stats_popover(pane_id, true);
        }
    }

    /// Whether the pane's detected agent keeps a readable JSONL transcript.
    pub fn stats_agent_is_supported(&self, pane_id: NodeId) -> bool {
        use ilium_core::{AgentClass, NodeKind, PaneStatus};
        self.tree.get(pane_id).is_some_and(|node| {
            let NodeKind::Pane { status, .. } = &node.kind else {
                return false;
            };
            match status {
                PaneStatus::Agent(class, _) | PaneStatus::AgentWithGoal(class, _, _) => {
                    matches!(class, AgentClass::Claude | AgentClass::Codex)
                }
                _ => false,
            }
        })
    }

    /// Everything the worker needs to read this pane's transcript, `None`
    /// until the agent's session id is known.
    fn session_stats_request(&self, pane_id: NodeId) -> Option<StatsRequest> {
        let (class, session_id, project_path) = self.last_prompt_transcript_context(pane_id)?;
        if !self.stats_agent_is_supported(pane_id) {
            return None;
        }
        let home = directories::BaseDirs::new()?.home_dir().to_path_buf();
        Some(StatsRequest {
            class,
            session_id,
            project_path,
            home,
        })
    }

    /// Periodic maintenance, called from every tick: applies finished worker
    /// results, keeps an open popover's data fresh, and reports whether a
    /// redraw is needed (new data, the live clock, or the load spinner).
    pub(crate) fn tick_session_stats(&mut self, now: Instant) -> bool {
        let mut changed = self.session_stats.drain_events();
        let Some(pane_id) = self.stats_popover.as_ref().map(|popover| popover.pane_id) else {
            let tree = &self.tree;
            self.session_stats
                .retain_panes(|pane_id| tree.get(pane_id).is_some());
            return changed;
        };
        if !self.is_detected_agent_pane(pane_id) || self.pane_viewport(pane_id).is_none() {
            self.stats_popover = None;
            return true;
        }
        if !self.stats_ui_active() {
            return changed;
        }
        if let Some(request) = self.session_stats_request(pane_id) {
            self.session_stats.request_refresh(pane_id, request, now);
        }
        let is_loading = self.session_stats.entry(pane_id).is_some_and(|entry| {
            matches!(
                entry.state,
                crate::session_stats_store::LoadState::Loading { .. }
            )
        });
        if let Some(popover) = self.stats_popover.as_mut() {
            if is_loading || now.duration_since(popover.last_clock_redraw) >= CLOCK_REDRAW_INTERVAL
            {
                popover.last_clock_redraw = now;
                changed = true;
            }
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::KeyModifiers;
    use ilium_core::{AgentActivity, AgentClass, PaneContentKind, PaneStatus, ROOT_ID};
    use ratatui::layout::Rect;

    use super::*;
    use crate::app::{FocusTarget, PaneRuntime};
    use crate::session_stats_ui::StatsTab;
    use crate::terminal_view::TerminalView;

    fn agent_app() -> (App, NodeId) {
        let mut app = App::new("test-session".to_string(), std::env::temp_dir());
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        let pane_id = app
            .tree
            .add_pane(group, "agent", PaneContentKind::Terminal)
            .unwrap();
        app.tree
            .set_pane_status(
                pane_id,
                PaneStatus::Agent(AgentClass::Claude, AgentActivity::Idle),
            )
            .unwrap();
        app.panes.insert(
            pane_id,
            PaneRuntime::Terminal(Box::new(TerminalView::new(24, 80))),
        );
        app.right_panel_target = RightPanelTarget::Pane { pane_id };
        app.focus = FocusTarget::Pane;
        app.set_screen_area(Rect::new(0, 0, 140, 44));
        (app, pane_id)
    }

    fn event(kind: MouseEventKind, position: Position) -> MouseEvent {
        MouseEvent {
            kind,
            column: position.x,
            row: position.y,
            modifiers: KeyModifiers::NONE,
        }
    }

    fn send(app: &mut App, kind: MouseEventKind, position: Position) -> bool {
        app.handle_stats_popover_mouse(event(kind, position), position)
    }

    #[test]
    fn hovering_the_second_icon_previews_and_leaving_it_closes() {
        let (mut app, pane_id) = agent_app();
        let viewport = app.pane_viewport(pane_id).unwrap();
        let icon = theme::chrome_stats_cell(viewport.outer_area);

        send(&mut app, MouseEventKind::Moved, icon);
        let popover = app.stats_popover.as_ref().expect("hover opens a preview");
        assert_eq!(popover.pane_id, pane_id);
        assert!(!popover.pinned);

        send(
            &mut app,
            MouseEventKind::Moved,
            Position::new(icon.x + 40, 42),
        );
        assert!(app.stats_popover.is_none(), "leaving closes a preview");
    }

    #[test]
    fn a_preview_survives_the_pointer_moving_onto_the_popover() {
        let (mut app, pane_id) = agent_app();
        let viewport = app.pane_viewport(pane_id).unwrap();
        let icon = theme::chrome_stats_cell(viewport.outer_area);
        send(&mut app, MouseEventKind::Moved, icon);
        let layout = app.stats_popover_geometry().unwrap();

        let inside = Position::new(layout.body.x + 3, layout.body.y + 3);
        assert!(send(&mut app, MouseEventKind::Moved, inside));
        assert!(app.stats_popover.is_some());
    }

    #[test]
    fn clicking_the_icon_pins_and_clicking_again_or_close_unpins() {
        let (mut app, pane_id) = agent_app();
        let icon = theme::chrome_stats_cell(app.pane_viewport(pane_id).unwrap().outer_area);

        assert!(send(
            &mut app,
            MouseEventKind::Down(MouseButton::Left),
            icon
        ));
        assert!(app.stats_popover.as_ref().unwrap().pinned);

        // A pinned popover stays when the pointer wanders off.
        send(
            &mut app,
            MouseEventKind::Moved,
            Position::new(icon.x + 60, icon.y + 35),
        );
        assert!(app.stats_popover.is_some());

        let layout = app.stats_popover_geometry().unwrap();
        let close = Position::new(layout.close.x + 1, layout.close.y);
        assert!(send(
            &mut app,
            MouseEventKind::Down(MouseButton::Left),
            close
        ));
        assert!(app.stats_popover.is_none(), "the close control closes it");

        send(&mut app, MouseEventKind::Down(MouseButton::Left), icon);
        assert!(app.stats_popover.is_some());
        send(&mut app, MouseEventKind::Down(MouseButton::Left), icon);
        assert!(app.stats_popover.is_none(), "the icon toggles it off");
    }

    #[test]
    fn clicking_a_preview_pins_it_and_tabs_switch() {
        let (mut app, pane_id) = agent_app();
        let icon = theme::chrome_stats_cell(app.pane_viewport(pane_id).unwrap().outer_area);
        send(&mut app, MouseEventKind::Moved, icon);
        let layout = app.stats_popover_geometry().unwrap();
        let tokens = layout
            .tabs
            .iter()
            .find(|(tab, _)| *tab == StatsTab::Tokens)
            .unwrap()
            .1;

        assert!(send(
            &mut app,
            MouseEventKind::Down(MouseButton::Left),
            Position::new(tokens.x + 2, tokens.y)
        ));
        let popover = app.stats_popover.as_ref().unwrap();
        assert!(popover.pinned);
        assert_eq!(popover.tab, StatsTab::Tokens);
    }

    #[test]
    fn events_over_the_popover_never_reach_the_pane() {
        let (mut app, pane_id) = agent_app();
        let icon = theme::chrome_stats_cell(app.pane_viewport(pane_id).unwrap().outer_area);
        send(&mut app, MouseEventKind::Down(MouseButton::Left), icon);
        let layout = app.stats_popover_geometry().unwrap();
        let inside = Position::new(layout.body.x + 4, layout.body.y + 4);
        for kind in [
            MouseEventKind::Down(MouseButton::Left),
            MouseEventKind::Down(MouseButton::Right),
            MouseEventKind::Up(MouseButton::Left),
            MouseEventKind::Drag(MouseButton::Left),
            MouseEventKind::ScrollDown,
        ] {
            assert!(send(&mut app, kind, inside), "{kind:?} must be claimed");
        }
        // Outside the popover, ordinary routing is untouched.
        assert!(!send(
            &mut app,
            MouseEventKind::Down(MouseButton::Left),
            Position::new(layout.area.x + 1, layout.area.bottom() + 1)
        ));
    }

    #[test]
    fn plain_shells_have_no_stats_icon() {
        let mut app = App::new("test-session".to_string(), std::env::temp_dir());
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        let pane_id = app
            .tree
            .add_pane(group, "shell", PaneContentKind::Terminal)
            .unwrap();
        app.panes.insert(
            pane_id,
            PaneRuntime::Terminal(Box::new(TerminalView::new(24, 80))),
        );
        app.right_panel_target = RightPanelTarget::Pane { pane_id };
        app.set_screen_area(Rect::new(0, 0, 140, 44));
        let icon = theme::chrome_stats_cell(app.pane_viewport(pane_id).unwrap().outer_area);
        assert_eq!(app.stats_icon_pane_at(icon), None);
        assert!(!send(
            &mut app,
            MouseEventKind::Down(MouseButton::Left),
            icon
        ));
    }

    #[test]
    fn full_click_sequence_through_the_top_level_dispatcher_pins() {
        let (mut app, pane_id) = agent_app();
        let icon = theme::chrome_stats_cell(app.pane_viewport(pane_id).unwrap().outer_area);
        for kind in [
            MouseEventKind::Moved,
            MouseEventKind::Moved,
            MouseEventKind::Down(MouseButton::Left),
            MouseEventKind::Up(MouseButton::Left),
        ] {
            let position = if matches!(kind, MouseEventKind::Moved) && app.stats_popover.is_some() {
                Position::new(icon.x + 60, 42)
            } else {
                icon
            };
            crate::mouse::handle_mouse_event(&mut app, event(kind, position));
            let _ = app.tick_session_stats(Instant::now());
        }
        assert!(
            app.stats_popover
                .as_ref()
                .is_some_and(|popover| popover.pinned),
            "the dispatcher sequence must leave a pinned popover"
        );
    }

    #[test]
    fn agents_without_a_readable_transcript_are_unsupported() {
        let (mut app, pane_id) = agent_app();
        assert!(app.stats_agent_is_supported(pane_id));
        app.tree
            .set_pane_status(
                pane_id,
                PaneStatus::Agent(AgentClass::Antigravity, AgentActivity::Idle),
            )
            .unwrap();
        assert!(!app.stats_agent_is_supported(pane_id));
        app.agent_session_ids
            .insert(pane_id, "33333333-3333-4333-8333-333333333333".to_string());
        assert!(app.session_stats_request(pane_id).is_none());
    }

    #[test]
    fn modal_dialogs_suspend_the_popover_interaction() {
        let (mut app, pane_id) = agent_app();
        let icon = theme::chrome_stats_cell(app.pane_viewport(pane_id).unwrap().outer_area);
        send(&mut app, MouseEventKind::Down(MouseButton::Left), icon);
        app.mode = Mode::Help;
        assert!(!send(
            &mut app,
            MouseEventKind::Down(MouseButton::Left),
            icon
        ));
        assert!(
            app.stats_popover.is_some(),
            "state is kept for when the dialog closes"
        );
    }

    #[test]
    fn the_tick_closes_a_popover_whose_pane_stopped_being_an_agent() {
        let (mut app, pane_id) = agent_app();
        let icon = theme::chrome_stats_cell(app.pane_viewport(pane_id).unwrap().outer_area);
        send(&mut app, MouseEventKind::Down(MouseButton::Left), icon);
        app.tree
            .set_pane_status(pane_id, PaneStatus::PlainShell)
            .unwrap();
        assert!(app.tick_session_stats(Instant::now()));
        assert!(app.stats_popover.is_none());
    }
}

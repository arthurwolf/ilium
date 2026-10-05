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
    /// pane has a current or historical agent identity.
    pub fn stats_icon_pane_at(&self, position: Position) -> Option<NodeId> {
        let viewport = self.pane_viewport_at(position)?;
        let is_icon = theme::chrome_stats_cell(viewport.outer_area) == position;
        (is_icon && self.is_known_agent_pane(viewport.pane_id)).then_some(viewport.pane_id)
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
            if let Some(mut hit) = layout.hit(position) {
                if hit == StatsHit::Body {
                    let scale = self
                        .stats_popover
                        .as_ref()
                        .and_then(|popover| popover.scale_at(layout.body, position));
                    if let Some(scale) = scale {
                        hit = StatsHit::Scale(scale);
                    }
                }
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
                StatsHit::Scale(scale) => {
                    popover.pinned = true;
                    popover.scale = scale;
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
        use ilium_core::{AgentClass, NodeKind};
        self.tree.get(pane_id).is_some_and(|node| {
            let NodeKind::Pane { status, .. } = &node.kind else {
                return false;
            };
            status
                .known_agent_state()
                .is_some_and(|agent| matches!(agent.class, AgentClass::Claude | AgentClass::Codex))
        })
    }

    /// Everything the worker needs to read this pane's transcript, `None`
    /// until the agent's session id is known.
    pub(crate) fn session_stats_request(&self, pane_id: NodeId) -> Option<StatsRequest> {
        let (class, session_id, project_path) = self.known_agent_history_context(pane_id)?;
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

    /// Filters snapshots immediately, including between an IPC identity
    /// transition and the next maintenance tick.
    pub(crate) fn current_stats_entry(
        &self,
        pane_id: NodeId,
    ) -> Option<&crate::session_stats_store::StatsEntry> {
        let request = self.session_stats_request(pane_id)?;
        self.session_stats.matching_entry(pane_id, &request)
    }

    /// Periodic maintenance, called from every tick: applies finished worker
    /// results, keeps an open popover's data fresh, and reports whether a
    /// redraw is needed (new data, the live clock, or the load spinner).
    pub(crate) fn tick_session_stats(&mut self, now: Instant) -> bool {
        let contexts = self
            .tree
            .panes()
            .filter_map(|node| {
                self.session_stats_request(node.id)
                    .map(|request| (node.id, request))
            })
            .collect();
        let mut changed = self.session_stats.reconcile_contexts(&contexts);
        changed |= self.session_stats.drain_events();
        let Some(pane_id) = self.stats_popover.as_ref().map(|popover| popover.pane_id) else {
            return changed;
        };
        if !self.is_known_agent_pane(pane_id) || self.pane_viewport(pane_id).is_none() {
            self.stats_popover = None;
            return true;
        }
        if !self.stats_ui_active() {
            return changed;
        }
        if let Some(request) = self.session_stats_request(pane_id) {
            self.session_stats
                .request_refresh_interactive(pane_id, request, now);
        }
        let is_loading = self.current_stats_entry(pane_id).is_some_and(|entry| {
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
                PaneStatus::from_activity(AgentClass::Claude, AgentActivity::Idle, None),
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
                PaneStatus::from_activity(AgentClass::Antigravity, AgentActivity::Idle, None),
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

#[cfg(test)]
mod retained_identity_tests {
    use super::*;
    use ilium_core::{
        AgentActivity, AgentAvailability, AgentClass, AgentExitOutcome, AgentProcessKey,
        AgentRecovery, PaneContentKind, PaneStatus, ROOT_ID,
    };
    use std::sync::Arc;
    fn ready_app() -> (App, NodeId, Arc<crate::session_stats::SessionStats>) {
        let mut app = App::new("synthetic-stats-recovery".into(), std::env::temp_dir());
        let group = app.tree.add_group(ROOT_ID, "synthetic").unwrap();
        let pane_id = app
            .tree
            .add_pane(group, "synthetic agent", PaneContentKind::Terminal)
            .unwrap();
        app.tree
            .set_pane_status(
                pane_id,
                PaneStatus::from_activity(AgentClass::Claude, AgentActivity::Idle, None),
            )
            .unwrap();
        app.agent_session_ids
            .insert(pane_id, "original-session".into());
        let request = app.session_stats_request(pane_id).unwrap();
        let snapshot = Arc::new(crate::session_stats::SessionStats {
            prompt_count: 991,
            ..Default::default()
        });
        app.session_stats
            .insert_ready_for_request_for_test(pane_id, request, snapshot.clone());
        (app, pane_id, snapshot)
    }
    #[test]
    fn unresolved_replacement_hides_old_stats_before_tick_and_drops_cache_without_popover() {
        let (mut app, pane_id, _) = ready_app();
        app.tree
            .set_pane_status(
                pane_id,
                PaneStatus::from_activity(AgentClass::Codex, AgentActivity::Idle, None),
            )
            .unwrap();
        app.agent_session_ids.remove(&pane_id);
        assert!(app.current_stats_entry(pane_id).is_none());
        assert!(app.tick_session_stats(Instant::now()));
        assert!(app.session_stats.entry(pane_id).is_none());
    }
    #[test]
    fn crash_keeps_original_stats_after_live_session_map_is_cleared() {
        let (mut app, pane_id, snapshot) = ready_app();
        let ilium_core::NodeKind::Pane { status, .. } = &app.tree.get(pane_id).unwrap().kind else {
            panic!("fixture pane expected")
        };
        let last_known_state = status.agent_state().unwrap().clone();
        let recovery = AgentRecovery {
            last_known_state,
            process: AgentProcessKey {
                class: AgentClass::Claude,
                process_id: 99,
                started_at_unix_seconds: 1,
            },
            availability: AgentAvailability::Exited(AgentExitOutcome::Unknown),
            signal_name: None,
            session_id: Some("original-session".into()),
            last_prompt: None,
            previous_exact_prompt: None,
            latest_prompt_unavailable: false,
        };
        app.tree
            .set_pane_status(pane_id, PaneStatus::AgentUnavailable(Box::new(recovery)))
            .unwrap();
        app.agent_session_ids.remove(&pane_id);
        assert!(app.known_agent_history_context(pane_id).is_some());
        assert!(Arc::ptr_eq(
            &snapshot,
            app.current_stats_entry(pane_id)
                .unwrap()
                .stats
                .as_ref()
                .unwrap()
        ));
        assert!(!app.tick_session_stats(Instant::now()));
        assert!(Arc::ptr_eq(
            &snapshot,
            app.current_stats_entry(pane_id)
                .unwrap()
                .stats
                .as_ref()
                .unwrap()
        ));
    }
    #[test]
    fn different_verified_session_hides_old_snapshot_immediately() {
        let (mut app, pane_id, _) = ready_app();
        app.agent_session_ids
            .insert(pane_id, "replacement-session".into());
        assert!(app.current_stats_entry(pane_id).is_none());
        app.tick_session_stats(Instant::now());
        assert!(app.session_stats.entry(pane_id).is_none());
    }

    #[test]
    fn stopped_agent_stats_dot_opens_pinned_retained_metrics_in_actual_renderer() {
        use crate::app::{FocusTarget, PaneRuntime};
        use crate::terminal_view::TerminalView;
        use ratatui::{backend::TestBackend, layout::Rect, Terminal};

        let (mut app, pane_id, _) = ready_app();
        let request = app.session_stats_request(pane_id).unwrap();
        app.session_stats.insert_ready_for_request_for_test(
            pane_id,
            request,
            Arc::new(crate::session_stats::SessionStats {
                provider: Some("synthetic-retained-stats".into()),
                prompt_count: 991,
                ..Default::default()
            }),
        );
        app.tree
            .set_pane_status(
                pane_id,
                PaneStatus::AgentUnavailable(Box::new(AgentRecovery {
                    last_known_state: ilium_core::AgentState::from_activity(
                        AgentClass::Claude,
                        AgentActivity::Idle,
                        None,
                    ),
                    process: AgentProcessKey {
                        class: AgentClass::Claude,
                        process_id: 99,
                        started_at_unix_seconds: 1,
                    },
                    availability: AgentAvailability::Exited(AgentExitOutcome::Unknown),
                    signal_name: None,
                    session_id: Some("original-session".into()),
                    last_prompt: None,
                    previous_exact_prompt: None,
                    latest_prompt_unavailable: false,
                })),
            )
            .unwrap();
        app.agent_session_ids.remove(&pane_id);
        app.panes.insert(
            pane_id,
            PaneRuntime::Terminal(Box::new(TerminalView::new(40, 100))),
        );
        app.right_panel_target = RightPanelTarget::Pane { pane_id };
        app.focus = FocusTarget::Pane;
        app.onboarding = None;
        app.set_screen_area(Rect::new(0, 0, 140, 44));
        let mut terminal = Terminal::new(TestBackend::new(140, 44)).unwrap();
        terminal
            .draw(|frame| crate::ui::draw(frame, &mut app))
            .unwrap();
        let icon = theme::chrome_stats_cell(app.pane_viewport(pane_id).unwrap().outer_area);
        assert_eq!(terminal.backend().buffer()[(icon.x, icon.y)].symbol(), "●");
        let mouse = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: icon.x,
            row: icon.y,
            modifiers: crossterm::event::KeyModifiers::NONE,
        };
        assert!(app.handle_stats_popover_mouse(mouse, icon));
        assert!(app.stats_popover.as_ref().unwrap().pinned);
        terminal
            .draw(|frame| crate::ui::draw(frame, &mut app))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let rendered: String = (0..44)
            .flat_map(|row| (0..140).map(move |column| buffer[(column, row)].symbol()))
            .collect();
        assert!(rendered.contains("synthetic-retained-stats"));
        assert!(rendered.contains("991"));
        assert!(!rendered.contains("Waiting for the agent's session"));

        // A replacement with unresolved identity must hide the old metrics
        // even before the next maintenance tick reconciles the store.
        app.tree
            .set_pane_status(
                pane_id,
                PaneStatus::from_activity(AgentClass::Codex, AgentActivity::Idle, None),
            )
            .unwrap();
        terminal
            .draw(|frame| crate::ui::draw(frame, &mut app))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let rendered: String = (0..44)
            .flat_map(|row| (0..140).map(move |column| buffer[(column, row)].symbol()))
            .collect();
        assert!(!rendered.contains("synthetic-retained-stats"));
        assert!(!rendered.contains("991"));
        assert!(rendered.contains("Waiting for the agent's session"));
    }
}

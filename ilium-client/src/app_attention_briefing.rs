//! `App` side of the briefing box: which panes show one, how many rows it
//! reserves, when it starts and ends, and its mouse handling.
//!
//! This is a child module of `app` (declared there with `#[path]`). The pure
//! pieces (rows, drawing, hit rectangles, the entry lifecycle) live in
//! `crate::attention::briefing`.

use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ilium_core::{AgentTurn, NodeId, NodeKind, Tree};
use ratatui::layout::{Position, Rect};

use super::App;
use crate::attention::briefing::{self, BriefingButton, View};

fn unix_millis_now() -> u64 {
    crate::scheduled_input::unix_millis_now()
}

/// The turn of the agent in `pane_id`, when a known agent runs there.
fn agent_turn(tree: &Tree, pane_id: NodeId) -> Option<AgentTurn> {
    let node = tree.get(pane_id)?;
    let NodeKind::Pane { status, .. } = &node.kind else {
        return None;
    };
    status.known_agent_state().map(|agent| agent.turn)
}

impl App {
    /// Whether briefing boxes may exist at all: the attention queue is on and
    /// the briefing setting is on.
    pub fn briefing_enabled(&self) -> bool {
        self.attention_settings.briefing_enabled()
    }

    /// What the pane's briefing box shows, or `None` when it shows nothing.
    /// The live queue item wins over the snapshot taken on arrival, so a late
    /// LLM reply is visible at once.
    pub fn briefing_view(&self, pane_id: NodeId) -> Option<View<'_>> {
        if !self.briefing_enabled() {
            return None;
        }
        let entry = self.attention.briefings.entry(pane_id)?;
        let item = self.attention.queue.item(pane_id).unwrap_or(&entry.item);
        let waiting_on = item
            .waiting_on
            .and_then(|target| self.tree.get(target))
            .map(|node| node.name.as_str());
        Some(View {
            item,
            waiting_on,
            age_ms: unix_millis_now().saturating_sub(item.created_ms),
            collapsed: entry.collapsed,
        })
    }

    /// Rows the pane reserves for its briefing, given the pane `width` and the
    /// rows it can spare. Zero when there is no briefing or no room.
    pub fn briefing_rows(&self, pane_id: NodeId, width: u16, spare_rows: u16) -> u16 {
        self.briefing_view(pane_id)
            .map_or(0, |view| briefing::reserved_rows(&view, width, spare_rows))
    }

    /// The button of the pane's briefing the pointer rests on.
    pub fn hovered_briefing_button(&self, pane_id: NodeId) -> Option<BriefingButton> {
        self.attention
            .briefings
            .hovered
            .filter(|(hovered, _)| *hovered == pane_id)
            .map(|(_, button)| button)
    }

    /// Starts the briefing of `pane_id` from its queue item. Used when the
    /// user arrives through the "Next: needs you" press.
    pub(super) fn note_briefing_visit(&mut self, pane_id: NodeId) {
        if !self.briefing_enabled() {
            return;
        }
        let Some(item) = self.attention.queue.item(pane_id).cloned() else {
            return;
        };
        let turn = agent_turn(&self.tree, pane_id);
        self.attention
            .briefings
            .activate(&item, unix_millis_now(), turn);
    }

    /// Notices an arrival that did not go through the press: the overview
    /// dialog marks the target looked-at and focuses it, so a focused pane
    /// looked at since the last reconcile was reached through the queue. Must
    /// run before the queue is reconciled, while the item still exists.
    pub(super) fn detect_briefing_jump(&mut self) {
        let Some(last_reconcile) = self.attention.last_reconcile_ms else {
            return;
        };
        let Some(pane_id) = self.focused_pane_id() else {
            return;
        };
        let looked = self.attention.queue.last_looked_ms(pane_id).unwrap_or(0);
        if looked > last_reconcile {
            self.note_briefing_visit(pane_id);
        }
    }

    /// Follows the queue and drops briefings that no longer apply. Returns
    /// true when anything changed.
    pub(super) fn refresh_briefings(&mut self, now_ms: u64) -> bool {
        if !self.briefing_enabled() {
            let had_any = !self.attention.briefings.is_empty();
            self.attention.briefings.clear();
            return had_any;
        }
        let expiry_ms = u64::from(self.attention_settings.expiry_seconds) * 1_000;
        let queue = &self.attention.queue;
        let tree = &self.tree;
        let mut changed = self
            .attention
            .briefings
            .retain_current(now_ms, expiry_ms, |pane_id| {
                tree.get(pane_id)?;
                Some((queue.item(pane_id), agent_turn(tree, pane_id)))
            });
        for pane_id in self.attention.briefings.pane_ids() {
            if let Some(live) = self.attention.queue.item(pane_id) {
                changed |= self.attention.briefings.follow(pane_id, live);
            }
        }
        changed
    }

    /// Mouse handling for the pane's briefing box. Returns true when the
    /// event was a click on a button and is consumed.
    pub(super) fn handle_briefing_mouse(
        &mut self,
        pane_id: NodeId,
        area: Option<Rect>,
        mouse: MouseEvent,
        position: Position,
    ) -> bool {
        let button = area
            .filter(|area| area.contains(position) && self.briefing_view(pane_id).is_some())
            .and_then(|area| briefing::button_at(area, position));
        let hovered = button.map(|button| (pane_id, button));
        let is_this_pane = self
            .attention
            .briefings
            .hovered
            .is_none_or(|(hovered_id, _)| hovered_id == pane_id);
        if hovered.is_some() || is_this_pane {
            self.attention.briefings.hovered = hovered;
        }
        let Some(button) = button else {
            return false;
        };
        if !matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
            return false;
        }
        match button {
            BriefingButton::Collapse => self.attention.briefings.toggle_collapsed(pane_id),
            BriefingButton::Dismiss => self.attention.briefings.dismiss(pane_id),
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ilium_core::{AgentActivity, AgentClass, PaneContentKind, PaneStatus, ROOT_ID};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    use super::*;
    use crate::attention::{AttentionKind, AttentionMode, AttentionSettings};

    fn app() -> App {
        let mut app = App::new("test".to_string(), std::env::temp_dir());
        app.set_screen_area(Rect::new(0, 0, 120, 40));
        app.apply_attention_settings(AttentionSettings::default());
        app
    }

    fn add_agent(app: &mut App, name: &str, activity: AgentActivity) -> NodeId {
        let group = app
            .tree
            .children_of(ilium_core::ROOT_ID)
            .ok()
            .and_then(|children| children.first().copied())
            .filter(|id| app.tree.get(*id).is_some_and(|node| node.is_group()))
            .unwrap_or_else(|| app.tree.add_group(ROOT_ID, "work").unwrap());
        let pane_id = app
            .tree
            .add_pane(group, name, PaneContentKind::Terminal)
            .unwrap();
        set_activity(app, pane_id, activity);
        pane_id
    }

    fn set_activity(app: &mut App, pane_id: NodeId, activity: AgentActivity) {
        app.tree
            .set_pane_status(
                pane_id,
                PaneStatus::from_activity(AgentClass::Claude, activity, None),
            )
            .unwrap();
    }

    fn with_settings(app: &mut App, mode: AttentionMode, show_briefing: bool) {
        app.apply_attention_settings(AttentionSettings {
            mode,
            show_briefing,
            ..AttentionSettings::default()
        });
    }

    /// A moment later on the wall clock, so "looked at since the last
    /// reconcile" is strictly later than that reconcile.
    fn later() {
        std::thread::sleep(Duration::from_millis(5));
    }

    fn viewport_of(app: &App, pane_id: NodeId) -> crate::split_layout::PaneViewport {
        app.pane_viewports()
            .into_iter()
            .find(|viewport| viewport.pane_id == pane_id)
            .expect("the pane is displayed")
    }

    fn click(app: &mut App, position: Position) {
        crate::mouse::handle_mouse_event(
            app,
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: position.x,
                row: position.y,
                modifiers: KeyModifiers::NONE,
            },
        );
    }

    #[test]
    fn the_press_opens_a_briefing_with_the_items_own_facts() {
        let mut app = app();
        let asking = add_agent(&mut app, "asks", AgentActivity::WaitingApproval);
        let other = add_agent(&mut app, "also", AgentActivity::WaitingApproval);
        app.tick_attention();
        assert!(
            app.briefing_view(asking).is_none(),
            "nothing before arriving"
        );

        app.action_attention_press();

        let target = app.active_pane_id().expect("the press focused a pane");
        let view = app
            .briefing_view(target)
            .expect("a briefing for the target");
        assert_eq!(view.item.kind, AttentionKind::Question);
        assert!(!view.collapsed);
        assert!(
            view.item.briefing.why_here.is_some(),
            "an instant item carries the automatic wording"
        );
        assert!(!view.item.briefing.from_llm);
        let unvisited = if target == asking { other } else { asking };
        assert!(app.briefing_view(unvisited).is_none());
    }

    #[test]
    fn instant_only_shows_the_automatic_briefing_and_off_shows_nothing() {
        let mut app = app();
        let asking = add_agent(&mut app, "asks", AgentActivity::WaitingApproval);
        with_settings(&mut app, AttentionMode::InstantOnly, true);
        app.tick_attention();
        app.action_attention_press();
        assert!(app.briefing_view(asking).is_some());

        with_settings(&mut app, AttentionMode::Off, true);
        assert!(app.briefing_view(asking).is_none());
        assert!(app.attention.briefings.is_empty(), "Off empties the state");
    }

    #[test]
    fn the_setting_off_means_no_box_and_no_reserved_rows() {
        let mut app = app();
        let asking = add_agent(&mut app, "asks", AgentActivity::WaitingApproval);
        app.tick_attention();
        app.action_attention_press();
        let with_box = viewport_of(&app, asking);
        assert!(with_box.briefing_area.is_some());

        with_settings(&mut app, AttentionMode::Shared, false);
        app.tick_attention();
        let without = viewport_of(&app, asking);
        assert_eq!(without.briefing_area, None);
        assert_eq!(without.briefing_bottom_rule, None);
        assert!(
            without.content_area.height > with_box.content_area.height,
            "the terminal gets the rows back"
        );

        // Turned on again, the next arrival shows it; the old state did not
        // linger.
        with_settings(&mut app, AttentionMode::Shared, true);
        assert!(app.briefing_view(asking).is_none());
    }

    #[test]
    fn the_box_sits_above_the_terminal_and_the_terminal_shrinks_by_its_rows() {
        let mut app = app();
        let asking = add_agent(&mut app, "asks", AgentActivity::WaitingApproval);
        app.tick_attention();
        app.focus_pane(asking);
        let before = viewport_of(&app, asking);
        assert_eq!(before.briefing_area, None);

        app.note_briefing_visit(asking);
        let after = viewport_of(&app, asking);
        let area = after.briefing_area.expect("reserved");
        assert!(area.height >= 1);
        assert!(area.bottom() <= after.content_area.y, "no overlap");
        assert!(after.content_area.height < before.content_area.height);
        assert_eq!(area.x, before.content_area.x);
    }

    #[test]
    fn the_overview_dialog_jump_also_opens_the_briefing() {
        let mut app = app();
        let asking = add_agent(&mut app, "asks", AgentActivity::WaitingApproval);
        app.tick_attention();
        later();
        app.action_open_attention_overview();
        app.handle_attention_overview_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.active_pane_id(), Some(asking));
        assert!(
            app.briefing_view(asking).is_none(),
            "found at the next reconcile, not by the dialog code"
        );

        later();
        app.reconcile_attention(unix_millis_now());

        assert!(app.briefing_view(asking).is_some());
    }

    #[test]
    fn staying_on_a_pane_does_not_open_a_briefing_by_itself() {
        let mut app = app();
        let asking = add_agent(&mut app, "asks", AgentActivity::WaitingApproval);
        app.focus_pane(asking);
        later();
        app.reconcile_attention(unix_millis_now());
        later();
        app.reconcile_attention(unix_millis_now());
        assert!(app.attention.queue.item(asking).is_some());
        assert!(app.briefing_view(asking).is_none());
    }

    #[test]
    fn a_finished_agents_briefing_outlives_its_queue_item_until_the_agent_moves_on() {
        let mut app = app();
        let finished = add_agent(&mut app, "done", AgentActivity::Done);
        app.tick_attention();
        assert_eq!(
            app.attention.queue.item(finished).map(|item| item.kind),
            Some(AttentionKind::Changed)
        );
        later();
        app.action_attention_press();
        assert!(app.briefing_view(finished).is_some());

        // The user has read it: the evidence behind the item is gone and a
        // full reconcile removes the item. The briefing keeps its snapshot.
        set_activity(&mut app, finished, AgentActivity::Idle);
        later();
        app.reconcile_attention(unix_millis_now());
        assert!(app.attention.queue.item(finished).is_none());
        let view = app.briefing_view(finished).expect("snapshot kept");
        assert_eq!(view.item.kind, AttentionKind::Changed);

        // The agent starts something new: the briefing ends.
        set_activity(&mut app, finished, AgentActivity::Working);
        later();
        app.reconcile_attention(unix_millis_now());
        assert!(app.briefing_view(finished).is_none());
    }

    #[test]
    fn closing_the_pane_ends_its_briefing() {
        let mut app = app();
        let asking = add_agent(&mut app, "asks", AgentActivity::WaitingApproval);
        app.tick_attention();
        app.action_attention_press();
        assert!(app.briefing_view(asking).is_some());
        app.tree.remove_node(asking).unwrap();
        later();
        app.reconcile_attention(unix_millis_now());
        assert!(app.attention.briefings.is_empty());
    }

    #[test]
    fn clicking_the_buttons_folds_and_dismisses_the_box() {
        let mut app = app();
        let asking = add_agent(&mut app, "asks", AgentActivity::WaitingApproval);
        app.tick_attention();
        app.action_attention_press();
        let area = viewport_of(&app, asking).briefing_area.expect("reserved");
        let [(_, collapse), (_, dismiss)] = briefing::buttons(area);
        assert!(area.height > 1);

        click(&mut app, Position::new(collapse.x, collapse.y));
        assert!(app.briefing_view(asking).unwrap().collapsed);
        let folded = viewport_of(&app, asking)
            .briefing_area
            .expect("still shown");
        assert_eq!(folded.height, 1, "a folded box is one line");

        click(&mut app, Position::new(collapse.x, collapse.y));
        assert!(!app.briefing_view(asking).unwrap().collapsed);

        click(&mut app, Position::new(dismiss.x, dismiss.y));
        assert!(app.briefing_view(asking).is_none());
        assert_eq!(viewport_of(&app, asking).briefing_area, None);

        // A second visit to the same item does not bring it back.
        app.note_briefing_visit(asking);
        assert!(app.briefing_view(asking).is_none());
    }

    #[test]
    fn a_click_on_the_body_text_is_not_a_button() {
        let mut app = app();
        let asking = add_agent(&mut app, "asks", AgentActivity::WaitingApproval);
        app.tick_attention();
        app.action_attention_press();
        let area = viewport_of(&app, asking).briefing_area.expect("reserved");
        click(&mut app, Position::new(area.x + 6, area.y + 1));
        let view = app.briefing_view(asking).expect("still shown");
        assert!(!view.collapsed);
    }

    fn screen_text(app: &mut App) -> String {
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        terminal.draw(|frame| crate::ui::draw(frame, app)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..40)
            .map(|y| {
                (0..120)
                    .map(|x| buffer[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn the_box_is_drawn_for_a_terminal_pane_and_not_when_the_setting_is_off() {
        let mut app = app();
        let asking = add_agent(&mut app, "asks", AgentActivity::WaitingApproval);
        app.panes.insert(
            asking,
            super::super::PaneRuntime::Terminal(Box::new(crate::terminal_view::TerminalView::new(
                24, 80,
            ))),
        );
        app.tick_attention();
        app.action_attention_press();

        let text = screen_text(&mut app);
        assert!(text.contains("why you're here"), "{text}");
        assert!(text.contains("Question"), "{text}");
        assert!(text.contains("automatic"), "{text}");
        assert!(text.contains("\u{2715}"), "{text}");

        with_settings(&mut app, AttentionMode::Shared, false);
        let text = screen_text(&mut app);
        assert!(!text.contains("why you're here"), "{text}");
    }

    #[test]
    fn a_tiny_terminal_degrades_without_panicking() {
        let mut app = app();
        let asking = add_agent(&mut app, "asks", AgentActivity::WaitingApproval);
        app.panes.insert(
            asking,
            super::super::PaneRuntime::Terminal(Box::new(crate::terminal_view::TerminalView::new(
                24, 80,
            ))),
        );
        app.tick_attention();
        app.action_attention_press();
        for (width, height) in [(60u16, 6u16), (40, 8), (30, 5), (80, 3), (120, 12)] {
            app.set_screen_area(Rect::new(0, 0, width, height));
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|frame| crate::ui::draw(frame, &mut app))
                .unwrap();
            let height_of = |app: &App| {
                app.pane_viewports()
                    .into_iter()
                    .find(|viewport| viewport.pane_id == asking)
                    .map(|viewport| (viewport.content_area.height, viewport.briefing_area))
            };
            let (with_box, area) = height_of(&app).expect("the pane is displayed");
            with_settings(&mut app, AttentionMode::Shared, false);
            let (without_box, none) = height_of(&app).expect("the pane is displayed");
            assert_eq!(none, None);
            assert!(
                with_box <= without_box,
                "{width}x{height}: the box never adds rows"
            );
            if area.is_some() {
                assert!(
                    with_box >= briefing::MINIMUM_TERMINAL_ROWS.min(without_box),
                    "{width}x{height}: the terminal keeps its rows"
                );
            }
            // Turning it back on restores nothing by itself; re-arm for the
            // next size.
            with_settings(&mut app, AttentionMode::Shared, true);
            app.note_briefing_visit(asking);
        }
    }
}

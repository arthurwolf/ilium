//! `App` side of the "Needs you" overview dialog: building the cards from the
//! live queue, opening and closing, and applying the actions the pure dialog
//! (`crate::attention::dialog`) returns.
//!
//! This is a child module of `app` (declared there with `#[path]`) so it can
//! reach `App::focus_pane`. The dialog is `Mode::AttentionOverview`; its UI
//! state is `App::attention.dialog`.

use crossterm::event::{KeyEvent, MouseEvent};
use ilium_core::{NodeId, NodeKind};
use ratatui::layout::{Position, Rect};

use super::{is_press, App, Mode};
use crate::attention::dialog::{self, CardData, DialogAction, DialogState};

fn unix_millis_now() -> u64 {
    crate::scheduled_input::unix_millis_now()
}

impl App {
    /// The cards of the open (or about to open) dialog, best first. Built
    /// from the in-memory queue every time, so ages and contents are live.
    pub fn attention_overview_cards(&self) -> Vec<CardData> {
        let now_ms = unix_millis_now();
        let queue = &self.attention.queue;
        queue
            .ordered_for_display(now_ms)
            .into_iter()
            .map(|item| CardData {
                pane_id: item.pane_id,
                kind: item.kind,
                title: self
                    .tree
                    .get(item.pane_id)
                    .map_or_else(|| item.title.clone(), |node| node.name.clone()),
                project: item
                    .project_id
                    .and_then(|id| self.tree.get(id))
                    .map(|node| node.name.clone())
                    .unwrap_or_default(),
                agent_class: self.attention_agent_class_label(item.pane_id),
                reason: item.reason.clone(),
                question: item.question.clone(),
                waiting_on: item
                    .waiting_on
                    .and_then(|id| self.tree.get(id))
                    .map(|node| node.name.clone()),
                priority: item.priority,
                source: item.source,
                age_ms: now_ms.saturating_sub(item.created_ms),
                is_looked: queue.last_looked_ms(item.pane_id).unwrap_or(0) >= item.created_ms,
            })
            .collect()
    }

    fn attention_agent_class_label(&self, pane_id: NodeId) -> String {
        match self.tree.get(pane_id).map(|node| &node.kind) {
            Some(NodeKind::Pane { status, .. }) => status
                .known_agent_state()
                .map(|state| state.class.label().to_string())
                .unwrap_or_default(),
            _ => String::new(),
        }
    }

    /// Opens the overview (the `[+]` button and `Action::AttentionOverview`).
    ///
    /// Only from the normal view: another dialog keeps its own state, and
    /// stacking this one over it would lose that state on close.
    pub fn action_open_attention_overview(&mut self) {
        if !self.attention_enabled() {
            self.status_message = Some("The attention queue is off".to_string());
            return;
        }
        // A leader chord (`Ctrl+B O`) runs its action while the leader mode
        // is still pending, so those modes count as "no dialog open".
        if !matches!(
            self.mode,
            Mode::Normal | Mode::LeaderPending | Mode::NavigationLeaderPending
        ) {
            return;
        }
        let cards = self.attention_overview_cards();
        let mut state = DialogState::opened_at_second(unix_millis_now() / 1000);
        // Start on the card the "Next" button would jump to, when it is queued.
        let first_unread = cards.iter().position(|card| !card.is_looked).unwrap_or(0);
        state.select(first_unread, &cards);
        let layout = dialog::compute(self.layout.screen_area, &cards);
        state.scroll = layout.ensure_visible(first_unread, 0);
        self.attention.dialog = Some(state);
        self.mode = Mode::AttentionOverview;
    }

    pub fn close_attention_overview(&mut self) {
        self.attention.dialog = None;
        if matches!(self.mode, Mode::AttentionOverview) {
            self.mode = Mode::Normal;
        }
    }

    /// Keyboard contract of `Mode::AttentionOverview`.
    pub fn handle_attention_overview_key(&mut self, key: KeyEvent) {
        if !is_press(&key) {
            return;
        }
        let cards = self.attention_overview_cards();
        let screen = self.layout.screen_area;
        let Some(state) = self.attention.dialog.as_mut() else {
            self.close_attention_overview();
            return;
        };
        let action = state.handle_key(key.code, key.modifiers, screen, &cards);
        self.apply_attention_overview_action(action);
    }

    /// Pointer contract of `Mode::AttentionOverview`: every pointer event is
    /// consumed so nothing reaches the panes or the tree underneath.
    pub fn handle_attention_overview_mouse(&mut self, mouse: MouseEvent) {
        let cards = self.attention_overview_cards();
        let screen = self.layout.screen_area;
        let Some(state) = self.attention.dialog.as_mut() else {
            self.close_attention_overview();
            return;
        };
        let position = Position::new(mouse.column, mouse.row);
        let action = state.handle_mouse(mouse.kind, position, screen, &cards);
        self.apply_attention_overview_action(action);
    }

    fn apply_attention_overview_action(&mut self, action: DialogAction) {
        match action {
            DialogAction::None => {}
            DialogAction::Close => self.close_attention_overview(),
            DialogAction::Open(pane_id) => {
                self.close_attention_overview();
                self.jump_to_attention_card(pane_id);
            }
        }
    }

    /// Same effect as a click on a row of the right-hand panel
    /// (`App::open_attention_pane` in `app_attention.rs`, which is private to
    /// that file): focus the agent, record that it was looked at, and drop
    /// the arrival block.
    fn jump_to_attention_card(&mut self, pane_id: NodeId) {
        let was_centered = self.is_tree_row_centered(pane_id);
        self.focus_pane(pane_id);
        self.center_and_blink_tree_row(pane_id, was_centered);
        self.attention.queue.mark_looked(pane_id, unix_millis_now());
        self.attention.arrival = None;
    }

    /// Redraw trigger while the dialog is open: ages are shown to the second,
    /// so a frame is due whenever the wall-clock second changes.
    pub fn tick_attention_dialog(&mut self) -> bool {
        let second = unix_millis_now() / 1000;
        let Some(state) = self.attention.dialog.as_mut() else {
            return false;
        };
        if state.last_redraw_second == second {
            return false;
        }
        state.last_redraw_second = second;
        true
    }

    /// The `[+]` beside the "Next: needs you" button, when it is drawn.
    pub fn attention_plus_rect(&self) -> Option<Rect> {
        let button = self.attention_button_rect()?;
        dialog::split_button(button).1
    }

    /// True while the pointer rests on the `[+]`.
    pub fn is_attention_plus_hovered(&self) -> bool {
        let Some(position) = self.pointer_position else {
            return false;
        };
        self.attention_plus_rect()
            .is_some_and(|rect| rect.contains(position))
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{
        KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    };
    use ilium_core::{AgentActivity, AgentClass, NodeId, PaneContentKind, PaneStatus, ROOT_ID};
    use ratatui::backend::TestBackend;
    use ratatui::layout::Rect;
    use ratatui::Terminal;

    use super::super::{App, Mode};
    use crate::attention::AttentionSettings;

    fn app() -> App {
        let mut app = App::new("test".to_string(), std::env::temp_dir());
        app.set_screen_area(Rect::new(0, 0, 120, 40));
        app.apply_attention_settings(AttentionSettings::default());
        app
    }

    fn add_agent(app: &mut App, name: &str, activity: AgentActivity) -> NodeId {
        let group = app
            .tree
            .children_of(ROOT_ID)
            .ok()
            .and_then(|children| children.first().copied())
            .filter(|id| app.tree.get(*id).is_some_and(|node| node.is_group()))
            .unwrap_or_else(|| app.tree.add_group(ROOT_ID, "work").unwrap());
        let pane_id = app
            .tree
            .add_pane(group, name, PaneContentKind::Terminal)
            .unwrap();
        app.tree
            .set_pane_status(
                pane_id,
                PaneStatus::from_activity(AgentClass::Claude, activity, None),
            )
            .unwrap();
        pane_id
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn mouse(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }
    }

    fn app_with_two_waiting_agents() -> (App, NodeId, NodeId) {
        let mut app = app();
        let first = add_agent(&mut app, "builder", AgentActivity::WaitingApproval);
        let second = add_agent(&mut app, "reviewer", AgentActivity::WaitingApproval);
        app.tick_attention();
        assert_eq!(app.attention_count(), 2, "both agents are queued");
        (app, first, second)
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
    fn opening_sets_the_mode_and_builds_cards_from_the_queue() {
        let (mut app, first, second) = app_with_two_waiting_agents();
        app.action_open_attention_overview();
        assert!(matches!(app.mode, Mode::AttentionOverview));
        assert!(app.attention.dialog.is_some());
        let cards = app.attention_overview_cards();
        let ids: Vec<NodeId> = cards.iter().map(|card| card.pane_id).collect();
        assert!(ids.contains(&first) && ids.contains(&second));
        assert!(cards.iter().all(|card| card.agent_class == "Claude"));
        assert!(cards.iter().all(|card| !card.title.is_empty()));
    }

    #[test]
    fn opening_does_nothing_while_the_feature_is_off_or_another_dialog_is_open() {
        let (mut app, _, _) = app_with_two_waiting_agents();
        app.mode = Mode::Help;
        app.action_open_attention_overview();
        assert!(matches!(app.mode, Mode::Help));
        assert!(app.attention.dialog.is_none());
    }

    #[test]
    fn escape_and_q_close_and_leave_normal_mode() {
        let (mut app, _, _) = app_with_two_waiting_agents();
        for code in [KeyCode::Esc, KeyCode::Char('q')] {
            app.action_open_attention_overview();
            app.handle_attention_overview_key(key(code));
            assert!(matches!(app.mode, Mode::Normal));
            assert!(app.attention.dialog.is_none());
        }
    }

    #[test]
    fn enter_jumps_to_the_selected_agent_marks_it_looked_and_closes() {
        let (mut app, _, _) = app_with_two_waiting_agents();
        app.action_open_attention_overview();
        app.handle_attention_overview_key(key(KeyCode::Down));
        let cards = app.attention_overview_cards();
        let selected = {
            let state = app.attention.dialog.as_ref().unwrap();
            cards[state.selected_index(&cards)].pane_id
        };
        app.handle_attention_overview_key(key(KeyCode::Enter));
        assert!(matches!(app.mode, Mode::Normal));
        assert_eq!(app.active_pane_id(), Some(selected));
        let item = app.attention.queue.item(selected).unwrap();
        assert!(
            app.attention.queue.last_looked_ms(selected).unwrap_or(0) >= item.created_ms,
            "the jump records that the user looked"
        );
    }

    #[test]
    fn clicking_a_card_jumps_to_that_agent() {
        let (mut app, _, _) = app_with_two_waiting_agents();
        app.action_open_attention_overview();
        let cards = app.attention_overview_cards();
        let layout = crate::attention::dialog::compute(app.layout.screen_area, &cards);
        let visible = layout.visible(0);
        let target = visible
            .iter()
            .find(|v| v.slot.target == crate::attention::dialog::SlotTarget::Card { index: 1 })
            .expect("second card visible");
        crate::mouse::handle_mouse_event(
            &mut app,
            mouse(
                MouseEventKind::Down(MouseButton::Left),
                target.rect.x + 3,
                target.rect.y + 2,
            ),
        );
        assert!(matches!(app.mode, Mode::Normal));
        assert_eq!(app.active_pane_id(), Some(cards[1].pane_id));
    }

    #[test]
    fn clicking_outside_the_dialog_closes_it_without_jumping() {
        let (mut app, _, _) = app_with_two_waiting_agents();
        app.action_open_attention_overview();
        let before = app.active_pane_id();
        crate::mouse::handle_mouse_event(
            &mut app,
            mouse(MouseEventKind::Down(MouseButton::Left), 0, 0),
        );
        assert!(matches!(app.mode, Mode::Normal));
        assert_eq!(app.active_pane_id(), before);
    }

    #[test]
    fn the_plus_button_opens_the_dialog_but_the_rest_of_the_button_still_jumps() {
        let (mut app, _, _) = app_with_two_waiting_agents();
        app.set_screen_area(Rect::new(0, 0, 120, 40));
        let button = app.attention_button_rect().expect("button is drawn");
        let plus = app.attention_plus_rect().expect("plus is drawn");
        assert_eq!(plus.right(), button.right());
        assert!(!app.is_attention_plus_hovered());

        crate::mouse::handle_mouse_event(
            &mut app,
            mouse(
                MouseEventKind::Down(MouseButton::Left),
                button.x + 1,
                button.y,
            ),
        );
        assert!(
            matches!(app.mode, Mode::Normal),
            "main part jumps, no dialog"
        );

        crate::mouse::handle_mouse_event(
            &mut app,
            mouse(MouseEventKind::Down(MouseButton::Left), plus.x + 1, plus.y),
        );
        assert!(matches!(app.mode, Mode::AttentionOverview));
    }

    #[test]
    fn hovering_the_plus_is_derived_from_the_pointer() {
        let (mut app, _, _) = app_with_two_waiting_agents();
        let plus = app.attention_plus_rect().unwrap();
        app.set_pointer_position(Some(ratatui::layout::Position::new(plus.x, plus.y)));
        assert!(app.is_attention_plus_hovered());
        app.set_pointer_position(Some(ratatui::layout::Position::new(0, 0)));
        assert!(!app.is_attention_plus_hovered());
    }

    #[test]
    fn the_leader_chord_opens_the_overview_while_the_leader_is_pending() {
        use crossterm::event::Event;
        let (mut app, _, _) = app_with_two_waiting_agents();
        let leader = Event::Key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL));
        let letter = Event::Key(KeyEvent::new(KeyCode::Char('O'), KeyModifiers::SHIFT));
        crate::keys::handle_event(&mut app, leader);
        assert!(matches!(app.mode, Mode::LeaderPending));
        crate::keys::handle_event(&mut app, letter);
        assert!(
            matches!(app.mode, Mode::AttentionOverview),
            "Ctrl+B O must open the overview"
        );
        assert!(app.attention.dialog.is_some());
    }

    #[test]
    fn open_dialog_draws_over_the_screen_and_shows_the_agents() {
        let (mut app, _, _) = app_with_two_waiting_agents();
        app.action_open_attention_overview();
        let text = screen_text(&mut app);
        assert!(text.contains("Needs you"), "{text}");
        assert!(
            text.contains("builder") && text.contains("reviewer"),
            "{text}"
        );
        assert!(text.contains("Esc close"), "{text}");
    }

    #[test]
    fn an_emptied_queue_turns_the_open_dialog_into_the_empty_state() {
        let (mut app, first, second) = app_with_two_waiting_agents();
        app.action_open_attention_overview();
        for pane_id in [first, second] {
            app.tree
                .set_pane_status(
                    pane_id,
                    PaneStatus::from_activity(AgentClass::Claude, AgentActivity::Working, None),
                )
                .unwrap();
        }
        app.attention.last_reconcile_ms = None;
        app.tick_attention();
        assert!(app.attention_overview_cards().is_empty());
        assert!(
            matches!(app.mode, Mode::AttentionOverview),
            "focus is not stolen"
        );
        let text = screen_text(&mut app);
        assert!(text.contains("All clear"), "{text}");
        // Enter on the empty state is harmless.
        app.handle_attention_overview_key(key(KeyCode::Enter));
        assert!(matches!(app.mode, Mode::AttentionOverview));
    }

    #[test]
    fn the_age_redraw_fires_once_per_second_only_while_open() {
        let (mut app, _, _) = app_with_two_waiting_agents();
        assert!(!app.tick_attention_dialog(), "closed dialog never redraws");
        app.action_open_attention_overview();
        assert!(!app.tick_attention_dialog(), "same second as opening");
        app.attention.dialog.as_mut().unwrap().last_redraw_second = 0;
        assert!(app.tick_attention_dialog());
        assert!(!app.tick_attention_dialog(), "already redrawn this second");
    }
}

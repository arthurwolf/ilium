//! Worktree draft choices; repository actions remain owned by the original form.
use crate::app::{App, Mode};
use crate::value_control::{ControlKind, ControlSpec, ValueControl};
use crate::value_dialog::ChoiceOption;
use crate::value_dialog_host::ValueDialogHost;
use crate::worktree_dialog::{
    WorktreeClosePolicy, WorktreeDialogFocus, WorktreeDialogMode, WorktreeDialogState,
    WorktreeDialogStatus,
};
use ilium_core::{AgentProvider, BuiltinAgentProvider};
use ratatui::layout::Rect;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkspaceChoice {
    Provider,
    Where,
    ClosePolicy,
}
impl WorkspaceChoice {
    pub const ALL: [Self; 3] = [Self::Provider, Self::Where, Self::ClosePolicy];
    pub fn focus(self) -> WorktreeDialogFocus {
        match self {
            Self::Provider => WorktreeDialogFocus::Provider,
            Self::Where => WorktreeDialogFocus::Where,
            Self::ClosePolicy => WorktreeDialogFocus::ClosePolicy,
        }
    }
    pub fn from_focus(focus: WorktreeDialogFocus) -> Option<Self> {
        Self::ALL.into_iter().find(|field| field.focus() == focus)
    }
    pub fn available(self, parent: &WorktreeDialogState) -> bool {
        !matches!(parent.status, WorktreeDialogStatus::Creating(_))
            && (self != Self::ClosePolicy || parent.advanced)
    }
    fn entries(self) -> Vec<(String, &'static str)> {
        match self {
            Self::Provider => BuiltinAgentProvider::ALL
                .into_iter()
                .map(|value| (format!("{value:?}"), value.label()))
                .collect(),
            Self::Where => vec![
                ("New".into(), "New worktree"),
                ("Existing".into(), "Existing worktree"),
            ],
            Self::ClosePolicy => vec![
                ("Keep".into(), "Keep worktree"),
                ("OfferRemovalWhenSafe".into(), "Offer safe removal"),
            ],
        }
    }
    pub fn selected(self, parent: &WorktreeDialogState) -> String {
        match self {
            Self::Provider => format!("{:?}", parent.provider),
            Self::Where => format!("{:?}", parent.mode),
            Self::ClosePolicy => format!("{:?}", parent.close_policy),
        }
    }
    pub fn title(self) -> &'static str {
        match self {
            Self::Provider => "Agent provider",
            Self::Where => "Worktree location",
            Self::ClosePolicy => "Worktree close policy",
        }
    }
    pub fn options(self) -> Vec<ChoiceOption> {
        self.entries()
            .into_iter()
            .map(|(id, label)| ChoiceOption {
                id,
                label: label.into(),
                disabled_reason: None,
            })
            .collect()
    }
    pub fn control(self, screen: Rect, parent: &WorktreeDialogState) -> ValueControl {
        let layout = crate::worktree_dialog::dialog_layout(screen, parent);
        let (row, label) = match self {
            Self::Provider => (layout.provider_row, "Agent"),
            Self::Where => (layout.where_row, "Where"),
            Self::ClosePolicy => (layout.close_policy_row, "On close"),
        };
        let selected = self.selected(parent);
        let value = self
            .entries()
            .into_iter()
            .find(|(id, _)| *id == selected)
            .map_or("Unavailable", |(_, label)| label);
        let enabled = self.available(parent);
        ValueControl::new(
            row,
            ControlSpec {
                kind: ControlKind::Choice,
                label,
                value,
                label_width: 10,
                previous_enabled: enabled,
                next_enabled: enabled,
                open_enabled: enabled,
            },
        )
    }
    pub fn apply(self, parent: &mut WorktreeDialogState, id: &str) -> Result<(), String> {
        if !self.available(parent) {
            return Err("This worktree control is no longer available".into());
        }
        match self {
            Self::Provider => {
                parent.provider = BuiltinAgentProvider::ALL
                    .into_iter()
                    .find(|value| format!("{value:?}") == id)
                    .ok_or("This agent provider is unavailable")?
            }
            Self::Where => parent.set_mode(match id {
                "New" => WorktreeDialogMode::New,
                "Existing" => WorktreeDialogMode::Existing,
                _ => return Err("This worktree location is unavailable".into()),
            }),
            Self::ClosePolicy => {
                parent.close_policy = match id {
                    "Keep" => WorktreeClosePolicy::Keep,
                    "OfferRemovalWhenSafe" => WorktreeClosePolicy::OfferRemovalWhenSafe,
                    _ => return Err("This close policy is unavailable".into()),
                }
            }
        }
        Ok(())
    }
    pub fn step(self, parent: &mut WorktreeDialogState, direction: i32) -> Result<(), String> {
        let entries = self.entries();
        let index = entries
            .iter()
            .position(|(id, _)| *id == self.selected(parent))
            .ok_or("The current choice is unavailable")?;
        let next = (index as i32 + direction.signum()).rem_euclid(entries.len() as i32) as usize;
        self.apply(parent, &entries[next].0)
    }
}

impl App {
    pub(crate) fn begin_workspace_choice_dialog(
        &mut self,
        parent: Box<WorktreeDialogState>,
        field: WorkspaceChoice,
    ) {
        match ValueDialogHost::workspace_choice(&parent, field) {
            Ok(host) => self.push_modal_over(
                Mode::CreateAgentWorkspace(parent),
                Mode::ValueDialog(Box::new(host)),
            ),
            Err(error) => {
                self.status_message = Some(error);
                self.mode = Mode::CreateAgentWorkspace(parent);
            }
        }
    }
    // Only this instance's immediate parent may receive its correlated repository reply.
    pub(crate) fn workspace_dialog_ref(&self) -> Option<&WorktreeDialogState> {
        match &self.mode {
            Mode::CreateAgentWorkspace(parent) => Some(parent),
            Mode::ValueDialog(host) => match self.modal_stack.last() {
                Some(Mode::CreateAgentWorkspace(parent))
                    if host.matches_workspace_parent(parent) =>
                {
                    Some(parent)
                }
                _ => None,
            },
            _ => None,
        }
    }
    pub(crate) fn workspace_dialog_mut(&mut self) -> Option<&mut WorktreeDialogState> {
        match &mut self.mode {
            Mode::CreateAgentWorkspace(parent) => Some(parent),
            Mode::ValueDialog(host) => match self.modal_stack.last_mut() {
                Some(Mode::CreateAgentWorkspace(parent))
                    if host.matches_workspace_parent(parent) =>
                {
                    Some(parent)
                }
                _ => None,
            },
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value_dialog::{DialogOutcome, ValueDialogState};
    use ilium_core::{NodeId, ROOT_ID};

    fn parent() -> WorktreeDialogState {
        let mut state = WorktreeDialogState::new(
            NodeId(1),
            NodeId(2),
            BuiltinAgentProvider::Claude,
            WorktreeDialogMode::New,
        );
        state.advanced = true;
        state.branch_is_auto = false;
        state.path_is_auto = false;
        state.branch = crate::text_prompt::TextPromptState::new("authored/λ");
        state.path = crate::text_prompt::TextPromptState::new("/tmp/authored worktree");
        state.set_prompt_text("Authored\nmultiline request");
        state
    }
    fn facts() -> ilium_ipc::RepoFacts {
        ilium_ipc::RepoFacts {
            repo_common_dir: "/tmp/synthetic-repo/.git".into(),
            checkout_root: "/tmp/synthetic-repo".into(),
            project_subpath: Default::default(),
            current_branch: Some("topic".into()),
            default_base_ref: "main".into(),
            default_base_commit: "abcdef123456".into(),
            local_branches: vec!["main".into()],
            worktrees: vec![],
            source_dirty_count: 0,
            main_dirty_count: 0,
            has_gitmodules: false,
            git_version: ilium_ipc::WorkspaceGitVersion {
                major: 2,
                minor: 45,
                patch: 0,
            },
        }
    }
    #[test]
    fn catalogs_preserve_authored_fields_and_reject_reopened_drafts() {
        for (field, count) in WorkspaceChoice::ALL.into_iter().zip([3, 2, 2]) {
            let mut state = parent();
            let prompt = state.prompt_text();
            let host = ValueDialogHost::workspace_choice(&state, field).unwrap();
            let ValueDialogState::Choice(choice) = &host.dialog else {
                panic!("catalog");
            };
            assert_eq!(choice.options().len(), count);
            let id = choice.options().last().unwrap().id.clone();
            host.apply_workspace_choice(&mut state, &id).unwrap();
            assert_eq!(state.prompt_text(), prompt);
            assert_eq!(state.branch.buf, "authored/λ");
            assert_eq!(state.path.buf, "/tmp/authored worktree");
            assert!(!state.branch_is_auto && !state.path_is_auto);
            assert!(host.apply_workspace_choice(&mut parent(), &id).is_err());
            assert!(host.apply_workspace_choice(&mut state, "invented").is_err());
        }
    }
    #[test]
    fn choice_buttons_paint_and_hit_on_responsive_rows() {
        use crate::value_control::{ControlAction, ControlStyles, PointerButton};
        use ratatui::{backend::TestBackend, layout::Position, Terminal};
        let state = parent();
        for width in [24, 80, 140] {
            let screen = Rect::new(0, 0, width, 32);
            for field in WorkspaceChoice::ALL {
                let control = field.control(screen, &state);
                let geometry = control.geometry();
                let mut terminal = Terminal::new(TestBackend::new(width, 32)).unwrap();
                terminal
                    .draw(|frame| control.render(frame, ControlStyles::default()))
                    .unwrap();
                for (rect, glyph, action) in [
                    (geometry.previous, "←", ControlAction::PreviousChoice),
                    (geometry.open, "+", ControlAction::OpenChoices),
                    (geometry.next, "→", ControlAction::NextChoice),
                ] {
                    assert_eq!(
                        terminal.backend().buffer()[(rect.x, rect.y)].symbol(),
                        glyph
                    );
                    assert_eq!(
                        control.hit(Position::new(rect.x, rect.y), PointerButton::Left),
                        Some(action)
                    );
                }
                assert_eq!(
                    control.hit(
                        Position::new(geometry.label.x, geometry.label.y),
                        PointerButton::Left
                    ),
                    None
                );
            }
        }
    }
    #[test]
    fn actual_mouse_and_keyboard_open_catalog_without_submitting() {
        use crossterm::event::{
            Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
        };
        let mut app = App::new(
            "worktree-input".into(),
            "/tmp/synthetic-selector-input".into(),
        );
        app.set_screen_area(Rect::new(0, 0, 100, 32));
        let state = parent();
        let provider = state.provider;
        let geometry = WorkspaceChoice::Provider
            .control(app.layout.screen_area, &state)
            .geometry();
        app.mode = Mode::CreateAgentWorkspace(Box::new(state));
        let click = |app: &mut App, button, rect: Rect| {
            crate::mouse::handle_mouse_event(
                app,
                MouseEvent {
                    kind: MouseEventKind::Down(button),
                    column: rect.x,
                    row: rect.y,
                    modifiers: KeyModifiers::NONE,
                },
            )
        };
        click(&mut app, MouseButton::Left, geometry.value);
        assert_eq!(
            app.workspace_dialog_ref().unwrap().provider,
            provider.stepped(1)
        );
        click(&mut app, MouseButton::Right, geometry.value);
        assert_eq!(app.workspace_dialog_ref().unwrap().provider, provider);
        click(&mut app, MouseButton::Left, geometry.open);
        assert!(matches!(app.mode, Mode::ValueDialog(_)));
        crate::keys::handle_event(
            &mut app,
            Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        );
        assert!(matches!(app.mode, Mode::CreateAgentWorkspace(_)));
        crate::keys::handle_event(
            &mut app,
            Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        );
        assert!(matches!(app.mode, Mode::ValueDialog(_)));
        crate::keys::handle_event(
            &mut app,
            Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        );
        assert_eq!(
            app.workspace_dialog_ref().unwrap().prompt_text(),
            "Authored\nmultiline request"
        );
        assert!(app.take_outbound_requests().is_empty());
    }

    #[tokio::test]
    async fn loading_parent_retains_correlated_reply_and_allocations_under_catalog() {
        let (_execution, preparation) =
            crate::ipc_preparation::IpcPreparation::standalone().unwrap();
        let mut app = App::new(
            "worktree-catalog".into(),
            "/tmp/synthetic-selector-fixture".into(),
        );
        app.open_create_agent_workspace_dialog(BuiltinAgentProvider::Claude, ROOT_ID, false);
        let (request_id, project) = app
            .take_outbound_requests()
            .into_iter()
            .find_map(|request| match request {
                ilium_ipc::ClientRequest::QueryRepoFacts {
                    request_id,
                    project,
                } => Some((request_id, project)),
                _ => None,
            })
            .unwrap();
        let Mode::CreateAgentWorkspace(state) = std::mem::replace(&mut app.mode, Mode::Normal)
        else {
            panic!("parent");
        };
        let expected = state.facts_derivation_bytes(&facts()).unwrap();
        app.begin_workspace_choice_dialog(state, WorkspaceChoice::Provider);
        assert_eq!(
            app.repo_facts_derivation_bytes(request_id, project, &Ok(facts())),
            Some(expected)
        );
        app.receive_repo_facts(request_id + 1, project, Err("stale".into()));
        assert!(matches!(
            app.workspace_dialog_ref().unwrap().status,
            WorktreeDialogStatus::Loading
        ));
        let reservation = preparation.reserve_decoder().await.unwrap();
        let decoded = preparation
            .run_reserved(reservation, move |_| {
                let event = ilium_ipc::ServerEvent::RepoFactsReported {
                    request_id,
                    project,
                    result: Ok(facts()),
                };
                let bytes = event.retained_bytes();
                Ok((event, bytes))
            })
            .await
            .unwrap();
        let (event, retention) = preparation
            .retain_decoded(decoded)
            .await
            .unwrap()
            .into_parts();
        let derived = retention
            .as_ref()
            .unwrap()
            .try_reserve_derived(expected)
            .unwrap();
        // The admitted charge includes the storage guard overhead as well as
        // the requested payload. The same complete charge must survive the dialog.
        let admitted_derived_bytes = derived.declared_bytes();
        assert!(admitted_derived_bytes >= expected);
        app.processing_event_retention = retention;
        app.processing_derivation_retention = Some(derived);
        crate::render_cache::apply(&mut app, event);
        app.processing_event_retention = None;
        app.processing_derivation_retention = None;
        assert!(matches!(app.mode, Mode::ValueDialog(_)));
        let state = app.workspace_dialog_ref().unwrap();
        assert!(matches!(state.status, WorktreeDialogStatus::Ready));
        assert!(state.facts.is_some());
        assert!(state.facts_retention.is_some());
        assert_eq!(
            state
                .derivation_retention
                .as_ref()
                .unwrap()
                .declared_bytes(),
            admitted_derived_bytes
        );
        assert!(preparation.decoded_storage_bytes() > 0);
        let Mode::ValueDialog(host) = std::mem::replace(&mut app.mode, Mode::Normal) else {
            panic!("child");
        };
        app.finish_value_dialog(host, DialogOutcome::Cancel);
        assert!(matches!(app.mode, Mode::CreateAgentWorkspace(_)));
        app.mode = Mode::Normal;
        assert_eq!(preparation.decoded_storage_bytes(), 0);
    }
}

//! Typed parent-draft targets for the shared value dialog.
use crate::prompt_queue::PromptQueueDialogState;
use crate::value_dialog::{ChoiceDialogState, ChoiceOption, ValueDialogState};
use ilium_core::{AgentProvider, NodeId, PromptQueueDelivery};

pub enum ValueTarget {
    EditorLineDisplay {
        pane_id: NodeId,
        path: Option<std::path::PathBuf>,
        original: crate::config::LineDisplay,
    },
    OnboardingVoice {
        control: crate::value_voice::VoiceValue,
        identity: std::sync::Arc<()>,
        revision: u64,
        original: crate::config::VoiceSettings,
        directory: std::path::PathBuf,
    },
    OnboardingInference {
        field: crate::value_inference::OnboardingInference,
        identity: std::sync::Arc<()>,
        revision: u64,
        directory: std::path::PathBuf,
    },
    AgentEffort {
        pane_id: NodeId,
        original: crate::agent_toolbar::EffortLevel,
    },
    PruneBranch {
        project: NodeId,
        target: ilium_ipc::WorkspacePruneTarget,
        identity: std::sync::Arc<()>,
    },
    IconColumns {
        identity: std::sync::Arc<()>,
        target: crate::icon_settings::IconTarget,
    },
    KeyboardPrefix {
        field: crate::value_keyboard::KeyboardPrefix,
        destination: crate::value_keyboard::KeyboardDestination,
        directory: std::path::PathBuf,
    },
    WorkspaceChoice {
        field: crate::value_worktree::WorkspaceChoice,
        identity: std::sync::Arc<()>,
        project_id: NodeId,
        parent_group: NodeId,
    },
    AgentFromLineProvider {
        source: crate::agent_from_line::EditorSourceLine,
        parent_group: NodeId,
    },
    BoardStorage {
        parent_group: NodeId,
    },
    TriggerScope {
        draft_id: String,
    },
    SoundStudio {
        control: crate::onboarding::studio::SoundControl,
        identity: std::sync::Arc<()>,
        revision: u64,
        directory: std::path::PathBuf,
    },
    QueueDelivery {
        pane_id: NodeId,
    },
    Animation(crate::value_animation::AnimationControlTarget),
    Plugin(crate::value_plugin::PluginTarget),
    Cost {
        target: crate::value_cost::CostValueTarget,
        directory: std::path::PathBuf,
    },
    DetectionNumber {
        field: crate::value_settings::SettingsNumber,
        original: ilium_ipc::AgentDetectionSettings,
        pending: Option<crate::value_detection::DetectionNumberPending>,
    },
    SettingsNumber {
        field: crate::value_settings::SettingsNumber,
        directory: std::path::PathBuf,
        inference_revision: Option<u64>,
        autosave: Option<InferenceNumberAutosave>,
    },
    SettingsChoice {
        field: crate::value_settings_choice::SettingsChoice,
        directory: std::path::PathBuf,
        session_previous: Option<crate::config::SessionSettings>,
        inference_revision: Option<u64>,
    },
}

#[derive(Default)]
pub struct InferenceNumberAutosave {
    pub deadline: Option<std::time::Instant>,
    pub operation: Option<std::sync::Arc<()>>,
}

pub struct ValueDialogHost {
    pub target: ValueTarget,
    pub dialog: ValueDialogState,
    pending_save: Option<std::sync::Arc<()>>,
}

impl ValueDialogHost {
    pub(crate) fn number_host(
        target: ValueTarget,
        dialog: crate::value_dialog::NumberDialogState,
    ) -> Self {
        Self {
            target,
            dialog: ValueDialogState::Number(dialog),
            pending_save: None,
        }
    }

    pub(crate) fn choice_host(target: ValueTarget, dialog: ChoiceDialogState) -> Self {
        Self {
            target,
            dialog: ValueDialogState::Choice(dialog),
            pending_save: None,
        }
    }

    pub(crate) fn prune_branch(
        state: &crate::worktree_manager::WorktreeManagerState,
    ) -> Result<Self, String> {
        let (target, current) = state
            .confirmation_branch()
            .ok_or("No removal confirmation is open")?;
        use ilium_ipc::WorkspacePruneBranchPolicy as Policy;
        Ok(Self {
            target: ValueTarget::PruneBranch {
                project: state.project,
                target: target.clone(),
                identity: state.confirmation_identity.clone(),
            },
            dialog: ValueDialogState::Choice(ChoiceDialogState::new(
                "Branch disposition",
                vec![
                    ChoiceOption {
                        id: "keep".into(),
                        label: "Keep branch".into(),
                        disabled_reason: None,
                    },
                    ChoiceOption {
                        id: "delete-if-safe".into(),
                        label: "Delete merged branch safely (-d)".into(),
                        disabled_reason: None,
                    },
                ],
                Some(
                    if current == Policy::Keep {
                        "keep"
                    } else {
                        "delete-if-safe"
                    }
                    .into(),
                ),
            )?),
            pending_save: None,
        })
    }

    pub(crate) fn apply_prune_branch_choice(
        &self,
        state: &mut crate::worktree_manager::WorktreeManagerState,
        id: &str,
    ) -> Result<(), String> {
        let ValueTarget::PruneBranch {
            project,
            target,
            identity,
        } = &self.target
        else {
            return Err("This is not a branch choice".into());
        };
        if state.project != *project
            || !std::sync::Arc::ptr_eq(identity, &state.confirmation_identity)
            || state
                .confirmation_branch()
                .is_none_or(|(current, _)| current != target)
        {
            return Err("The removal confirmation changed; reopen branch choices".into());
        }
        use ilium_ipc::WorkspacePruneBranchPolicy as Policy;
        let policy = match id {
            "keep" => Policy::Keep,
            "delete-if-safe" => Policy::DeleteIfSafe,
            _ => return Err("This branch policy is unavailable".into()),
        };
        state.set_confirmation_branch(policy)
    }

    pub fn icon_columns(picker: &crate::app::IconPickerState) -> Result<Self, String> {
        Ok(Self {
            target: ValueTarget::IconColumns {
                identity: picker.identity.clone(),
                target: picker.target,
            },
            dialog: ValueDialogState::Choice(ChoiceDialogState::new(
                "Icon catalog layout",
                crate::app::IconPickerColumnMode::ALL
                    .into_iter()
                    .map(|mode| ChoiceOption {
                        id: format!("{mode:?}"),
                        label: mode.label().into(),
                        disabled_reason: None,
                    })
                    .collect(),
                Some(format!("{:?}", picker.column_mode)),
            )?),
            pending_save: None,
        })
    }
    pub(crate) fn matches_icon_picker(&self, picker: &crate::app::IconPickerState) -> bool {
        matches!(&self.target,ValueTarget::IconColumns { identity, target } if *target == picker.target && std::sync::Arc::ptr_eq(identity,&picker.identity))
    }
    pub fn apply_icon_column_choice(
        &self,
        state: &mut crate::app::SettingsState,
        id: &str,
        screen: ratatui::layout::Rect,
    ) -> Result<(), String> {
        let picker = state
            .icon_picker
            .as_mut()
            .filter(|picker| self.matches_icon_picker(picker))
            .ok_or("The icon catalog changed; reopen its layout choices")?;
        picker.column_mode = crate::app::IconPickerColumnMode::ALL
            .into_iter()
            .find(|mode| format!("{mode:?}") == id)
            .ok_or("This icon layout is unavailable")?;
        picker.scroll_row = crate::settings_ui::icon_picker_scroll_for_entry(screen, picker);
        Ok(())
    }

    pub fn keyboard_prefix(
        field: crate::value_keyboard::KeyboardPrefix,
        destination: crate::value_keyboard::KeyboardDestination,
        directory: std::path::PathBuf,
        settings: crate::config::KeyboardSettings,
    ) -> Result<Self, String> {
        Ok(Self {
            target: ValueTarget::KeyboardPrefix {
                field,
                destination,
                directory,
            },
            dialog: ValueDialogState::Choice(ChoiceDialogState::new(
                field.title(),
                field.options(),
                Some(field.current(settings).letter().to_string()),
            )?),
            pending_save: None,
        })
    }

    pub fn workspace_choice(
        parent: &crate::worktree_dialog::WorktreeDialogState,
        field: crate::value_worktree::WorkspaceChoice,
    ) -> Result<Self, String> {
        if !field.available(parent) {
            return Err("This worktree control is unavailable".into());
        }
        Ok(Self {
            target: ValueTarget::WorkspaceChoice {
                field,
                identity: parent.identity.clone(),
                project_id: parent.project_id,
                parent_group: parent.parent_group,
            },
            dialog: ValueDialogState::Choice(ChoiceDialogState::new(
                field.title(),
                field.options(),
                Some(field.selected(parent)),
            )?),
            pending_save: None,
        })
    }
    pub(crate) fn matches_workspace_parent(
        &self,
        parent: &crate::worktree_dialog::WorktreeDialogState,
    ) -> bool {
        matches!(&self.target, ValueTarget::WorkspaceChoice { identity, project_id, parent_group, .. } if std::sync::Arc::ptr_eq(identity, &parent.identity) && *project_id == parent.project_id && *parent_group == parent.parent_group)
    }
    pub fn apply_workspace_choice(
        &self,
        parent: &mut crate::worktree_dialog::WorktreeDialogState,
        id: &str,
    ) -> Result<(), String> {
        if !self.matches_workspace_parent(parent) {
            return Err("The worktree draft changed; reopen its catalog".into());
        }
        let ValueTarget::WorkspaceChoice { field, .. } = self.target else {
            return Err("This catalog belongs to a different form".into());
        };
        field.apply(parent, id)
    }

    pub fn agent_from_line_provider(
        parent: &crate::agent_from_line::CreateAgentFromLineState,
    ) -> Result<Self, String> {
        let options = crate::agent_from_line::AgentLaunchType::ALL
            .into_iter()
            .map(|value| ChoiceOption {
                id: format!("{value:?}"),
                label: value.label().into(),
                disabled_reason: None,
            })
            .collect();
        Ok(Self {
            target: ValueTarget::AgentFromLineProvider {
                source: parent.source.clone(),
                parent_group: parent.parent_group,
            },
            dialog: ValueDialogState::Choice(ChoiceDialogState::new(
                "Agent provider",
                options,
                Some(format!("{:?}", parent.agent_type)),
            )?),
            pending_save: None,
        })
    }

    pub fn apply_agent_from_line_provider(
        &self,
        parent: &mut crate::agent_from_line::CreateAgentFromLineState,
        id: &str,
    ) -> Result<(), String> {
        let ValueTarget::AgentFromLineProvider {
            source,
            parent_group,
        } = &self.target
        else {
            return Err("This catalog belongs to a different form".into());
        };
        if parent.parent_group != *parent_group || parent.source != *source {
            return Err(
                "The source line or destination changed; reopen its provider catalog".into(),
            );
        }
        parent.agent_type = crate::agent_from_line::AgentLaunchType::ALL
            .into_iter()
            .find(|value| format!("{value:?}") == id)
            .ok_or("This agent provider is no longer available")?;
        Ok(())
    }

    pub fn board_storage(parent: &crate::app::CreateBoardState) -> Result<Self, String> {
        let options = crate::app::BoardStorageKind::ALL
            .into_iter()
            .map(|value| ChoiceOption {
                id: format!("{value:?}"),
                label: value.label().into(),
                disabled_reason: None,
            })
            .collect();
        Ok(Self {
            target: ValueTarget::BoardStorage {
                parent_group: parent.parent_group,
            },
            dialog: ValueDialogState::Choice(ChoiceDialogState::new(
                "Board storage",
                options,
                Some(format!("{:?}", parent.storage_kind)),
            )?),
            pending_save: None,
        })
    }

    pub fn apply_board_storage_choice(
        &self,
        parent: &mut crate::app::CreateBoardState,
        id: &str,
    ) -> Result<(), String> {
        let ValueTarget::BoardStorage { parent_group } = &self.target else {
            return Err("This catalog belongs to a different form".into());
        };
        if parent.parent_group != *parent_group {
            return Err("The board destination changed; reopen its storage catalog".into());
        }
        parent.storage_kind = crate::app::BoardStorageKind::ALL
            .into_iter()
            .find(|value| format!("{value:?}") == id)
            .ok_or("This storage option is no longer available")?;
        Ok(())
    }

    pub fn trigger_scope(
        parent: &crate::text_trigger_dialog::TextTriggerDialogState,
    ) -> Result<Self, String> {
        let options = ilium_ipc::TextTriggerTarget::ALL
            .into_iter()
            .map(|value| ChoiceOption {
                id: format!("{value:?}"),
                label: value.label().into(),
                disabled_reason: None,
            })
            .collect();
        Ok(Self {
            target: ValueTarget::TriggerScope {
                draft_id: parent.identity().into(),
            },
            dialog: ValueDialogState::Choice(ChoiceDialogState::new(
                "Text trigger scope",
                options,
                Some(format!("{:?}", parent.target)),
            )?),
            pending_save: None,
        })
    }

    pub fn apply_trigger_scope_choice(
        &self,
        parent: &mut crate::text_trigger_dialog::TextTriggerDialogState,
        id: &str,
    ) -> Result<(), String> {
        let ValueTarget::TriggerScope { draft_id } = &self.target else {
            return Err("This catalog belongs to a different form".into());
        };
        if parent.identity() != draft_id {
            return Err("The text trigger changed; reopen its scope catalog".into());
        }
        parent.target = ilium_ipc::TextTriggerTarget::ALL
            .into_iter()
            .find(|value| format!("{value:?}") == id)
            .ok_or("This trigger scope is no longer available")?;
        Ok(())
    }

    pub fn studio_number(
        control: crate::onboarding::studio::SoundControl,
        studio: &crate::onboarding::studio::SoundStudio,
        revision: u64,
        directory: std::path::PathBuf,
    ) -> Self {
        Self {
            target: ValueTarget::SoundStudio {
                control,
                identity: studio.identity.clone(),
                revision,
                directory,
            },
            dialog: ValueDialogState::Number(crate::value_dialog::NumberDialogState::new(
                format!("{} ({})", control.label(), control.number_unit()),
                control.number_text(&studio.draft.design),
            )),
            pending_save: None,
        }
    }

    pub fn settings_choice(
        field: crate::value_settings_choice::SettingsChoice,
        app: &crate::app::App,
        directory: std::path::PathBuf,
    ) -> Result<Self, String> {
        if !directory.is_absolute() {
            return Err("The configuration directory is unavailable".into());
        }
        Ok(Self {
            target: ValueTarget::SettingsChoice {
                field,
                directory,
                session_previous: (field
                    == crate::value_settings_choice::SettingsChoice::SessionRecovery)
                    .then_some(app.session_settings),
                inference_revision: matches!(
                    field,
                    crate::value_settings_choice::SettingsChoice::InferenceProvider
                        | crate::value_settings_choice::SettingsChoice::KiloModel
                        | crate::value_settings_choice::SettingsChoice::OllamaModel
                        | crate::value_settings_choice::SettingsChoice::OpenAiModel
                )
                .then_some(app.onboarding_revision),
            },
            dialog: field.dialog(app)?,
            pending_save: None,
        })
    }

    pub fn settings_number(
        field: crate::value_settings::SettingsNumber,
        app: &crate::app::App,
        directory: std::path::PathBuf,
    ) -> Result<Self, String> {
        if !directory.is_absolute() {
            return Err("The configuration directory is unavailable".into());
        }
        let (_, initial) = field.snapshot(app);
        Ok(Self {
            target: ValueTarget::SettingsNumber {
                field,
                directory,
                autosave: (field == crate::value_settings::SettingsNumber::InferenceTokenBudget)
                    .then(InferenceNumberAutosave::default),
                inference_revision: (field
                    == crate::value_settings::SettingsNumber::InferenceTokenBudget)
                    .then_some(app.onboarding_revision),
            },
            dialog: ValueDialogState::Number(crate::value_dialog::NumberDialogState::new(
                field.title(),
                initial,
            )),
            pending_save: None,
        })
    }

    pub fn cost(
        settings: &crate::cost_settings::CostSettings,
        row: crate::cost_settings::CostRow,
        directory: std::path::PathBuf,
    ) -> Result<Self, String> {
        if !directory.is_absolute() {
            return Err("The cost configuration directory is unavailable".into());
        }
        let target = crate::value_cost::CostValueTarget::new(settings, row)?;
        let dialog = target.dialog(settings)?;
        Ok(Self {
            target: ValueTarget::Cost { target, directory },
            dialog,
            pending_save: None,
        })
    }

    pub fn animation(
        target: crate::value_animation::AnimationControlTarget,
    ) -> Result<Self, String> {
        let dialog = target.dialog()?;
        Ok(Self {
            target: ValueTarget::Animation(target),
            dialog,
            pending_save: None,
        })
    }

    pub(crate) fn inference_budget_draft(&self) -> Option<&str> {
        if !matches!(
            self.target,
            ValueTarget::SettingsNumber {
                autosave: Some(_),
                ..
            }
        ) {
            return None;
        }
        match &self.dialog {
            ValueDialogState::Number(number) => Some(&number.draft.buf),
            _ => None,
        }
    }

    pub fn is_saving(&self) -> bool {
        self.pending_save.is_some()
    }

    pub fn begin_save(&mut self, token: std::sync::Arc<()>) {
        self.pending_save = Some(token);
        self.reject("Saving; wait for the disk receipt (Esc closes this dialog)".into());
    }

    /// A superseded receipt cannot close or overwrite a replacement dialog.
    pub fn finish_save(&mut self, token: &std::sync::Arc<()>, result: Result<(), String>) -> bool {
        if !self
            .pending_save
            .as_ref()
            .is_some_and(|pending| std::sync::Arc::ptr_eq(pending, token))
        {
            return false;
        }
        self.pending_save = None;
        if let Err(error) = result {
            self.reject(error);
        }
        true
    }

    pub fn queue_delivery(parent: &PromptQueueDialogState) -> Result<Self, String> {
        let selected = match parent.delivery_choice {
            PromptQueueDelivery::Once => "once",
            PromptQueueDelivery::Times { .. } => "times",
            PromptQueueDelivery::Forever => "forever",
        };
        let options = [
            ("once", "Once"),
            ("times", "Run X times"),
            ("forever", "Enqueue forever (DANGER)"),
        ]
        .into_iter()
        .map(|(id, label)| ChoiceOption {
            id: id.into(),
            label: label.into(),
            disabled_reason: None,
        })
        .collect();
        Ok(Self {
            target: ValueTarget::QueueDelivery {
                pane_id: parent.pane_id,
            },
            dialog: ValueDialogState::Choice(ChoiceDialogState::new(
                "Prompt delivery",
                options,
                Some(selected.into()),
            )?),
            pending_save: None,
        })
    }
    pub fn apply_queue_choice(
        &self,
        parent: &mut PromptQueueDialogState,
        id: &str,
    ) -> Result<(), String> {
        let ValueTarget::QueueDelivery { pane_id } = &self.target else {
            return Err("This dialog belongs to a different form".into());
        };
        if parent.pane_id != *pane_id {
            return Err("The queued prompt destination changed; reopen its delivery list".into());
        }
        let choice = match id {
            "once" => PromptQueueDelivery::Once,
            "times" => PromptQueueDelivery::Times { remaining_runs: 2 },
            "forever" => PromptQueueDelivery::Forever,
            _ => return Err("This delivery option is no longer available".into()),
        };
        parent.delivery_choice = choice;
        Ok(())
    }
    pub fn reject(&mut self, error: String) {
        match &mut self.dialog {
            ValueDialogState::Choice(choice) => choice.notice = Some(error),
            ValueDialogState::Number(number) => number.reject(error),
        }
    }
}

impl crate::app::App {
    pub(crate) fn begin_agent_from_line_provider_dialog(
        &mut self,
        parent: Box<crate::agent_from_line::CreateAgentFromLineState>,
    ) {
        match ValueDialogHost::agent_from_line_provider(&parent) {
            Ok(host) => self.push_modal_over(
                crate::app::Mode::CreateAgentFromLine(parent),
                crate::app::Mode::ValueDialog(Box::new(host)),
            ),
            Err(error) => {
                self.status_message = Some(error);
                self.mode = crate::app::Mode::CreateAgentFromLine(parent);
            }
        }
    }

    pub(crate) fn begin_board_storage_dialog(&mut self, parent: crate::app::CreateBoardState) {
        match ValueDialogHost::board_storage(&parent) {
            Ok(host) => self.push_modal_over(
                crate::app::Mode::CreateBoard(parent),
                crate::app::Mode::ValueDialog(Box::new(host)),
            ),
            Err(error) => {
                self.status_message = Some(error);
                self.mode = crate::app::Mode::CreateBoard(parent);
            }
        }
    }
    pub(crate) fn begin_trigger_scope_dialog(
        &mut self,
        parent: Box<crate::text_trigger_dialog::TextTriggerDialogState>,
    ) {
        match ValueDialogHost::trigger_scope(&parent) {
            Ok(host) => self.push_modal_over(
                crate::app::Mode::TextTriggerDialog(parent),
                crate::app::Mode::ValueDialog(Box::new(host)),
            ),
            Err(error) => {
                self.status_message = Some(error);
                self.mode = crate::app::Mode::TextTriggerDialog(parent);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text_prompt::TextPromptState;
    #[test]
    fn agent_from_line_catalog_preserves_authored_prompt_source_and_destination() {
        use crate::agent_from_line::{
            AgentLaunchType, CreateAgentFocus, CreateAgentFromLineState, EditorSourceLine,
        };
        let source = EditorSourceLine {
            pane_id: NodeId(8),
            path: "/tmp/authored λ.rs".into(),
            line_number: 17,
            text: "original source".into(),
        };
        let mut parent = CreateAgentFromLineState::new(source.clone(), NodeId(9));
        parent.prompt =
            ratatui_textarea::TextArea::from(["authored first line", "authored second line"]);
        parent.focus = CreateAgentFocus::AgentType;
        let before = parent.prompt_text();
        let host = ValueDialogHost::agent_from_line_provider(&parent).unwrap();
        let ValueDialogState::Choice(choice) = &host.dialog else {
            panic!("catalog");
        };
        assert_eq!(choice.options().len(), AgentLaunchType::ALL.len());
        assert_eq!(
            choice.selected_id.as_deref(),
            Some(format!("{:?}", parent.agent_type).as_str())
        );
        host.apply_agent_from_line_provider(&mut parent, "Codex")
            .unwrap();
        assert_eq!(parent.agent_type, AgentLaunchType::Codex);
        assert_eq!(parent.prompt_text(), before);
        assert_eq!(parent.source, source);
        assert_eq!(parent.parent_group, NodeId(9));
        assert_eq!(parent.focus, CreateAgentFocus::AgentType);
        assert!(host
            .apply_agent_from_line_provider(&mut parent, "invented")
            .is_err());
        parent.source.line_number += 1;
        assert!(host
            .apply_agent_from_line_provider(&mut parent, "Claude")
            .is_err());
        assert_eq!(parent.agent_type, AgentLaunchType::Codex);
    }

    #[test]
    fn board_and_trigger_chrome_paints_the_exact_picker_and_arrow_targets() {
        use crate::value_control::{ControlAction, ControlStyles, PointerButton};
        use ratatui::{
            backend::TestBackend,
            layout::{Position, Rect},
            Terminal,
        };
        let trigger = crate::text_trigger_dialog::TextTriggerDialogState::new(None);
        for (width, height) in [(32, 20), (80, 30), (140, 40)] {
            let area = Rect::new(0, 0, width, height);
            for control in [
                crate::modal::create_board_storage_control(
                    area,
                    crate::app::BoardStorageKind::Folder.label(),
                ),
                crate::text_trigger_dialog::target_control(area, &trigger),
            ] {
                let geometry = control.geometry();
                let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
                terminal
                    .draw(|frame| control.render(frame, ControlStyles::default()))
                    .unwrap();
                for (rect, glyph) in [
                    (geometry.previous, "←"),
                    (geometry.open, "+"),
                    (geometry.next, "→"),
                ] {
                    assert_eq!(
                        terminal.backend().buffer()[(rect.x, rect.y)].symbol(),
                        glyph
                    );
                }
                assert_eq!(
                    control.hit(
                        Position::new(geometry.value.x, geometry.value.y),
                        PointerButton::Left
                    ),
                    Some(ControlAction::NextChoice)
                );
                assert_eq!(
                    control.hit(
                        Position::new(geometry.value.x, geometry.value.y),
                        PointerButton::Right
                    ),
                    Some(ControlAction::PreviousChoice)
                );
                assert_eq!(
                    control.hit(
                        Position::new(geometry.open.x, geometry.open.y),
                        PointerButton::Left
                    ),
                    Some(ControlAction::OpenChoices)
                );
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
    fn board_catalog_keeps_authored_fields_and_rejects_changed_destination() {
        let mut parent = crate::app::CreateBoardState {
            parent_group: NodeId(12),
            name: TextPromptState::new("Authored λ board"),
            path: TextPromptState::new("/tmp/authored plan.md"),
            storage_kind: crate::app::BoardStorageKind::Folder,
            editing_path: true,
        };
        parent.name.cursor = 3;
        parent.path.cursor = 7;
        let original = parent.clone();
        let host = ValueDialogHost::board_storage(&parent).unwrap();
        let ValueDialogState::Choice(choice) = &host.dialog else {
            panic!("catalog");
        };
        assert_eq!(choice.options().len(), 2);
        assert_eq!(choice.selected_id.as_deref(), Some("Folder"));
        host.apply_board_storage_choice(&mut parent, "MarkdownFile")
            .unwrap();
        assert_eq!(
            parent.storage_kind,
            crate::app::BoardStorageKind::MarkdownFile
        );
        assert_eq!(parent.name, original.name);
        assert_eq!(parent.path, original.path);
        assert_eq!(parent.editing_path, original.editing_path);
        assert!(host
            .apply_board_storage_choice(&mut parent, "invented")
            .is_err());
        parent.parent_group = NodeId(13);
        assert!(host
            .apply_board_storage_choice(&mut parent, "Folder")
            .is_err());
        assert_eq!(
            parent.storage_kind,
            crate::app::BoardStorageKind::MarkdownFile
        );
    }

    #[test]
    fn trigger_scope_catalog_preserves_authored_rule_and_fences_reopened_editor() {
        let mut parent = crate::text_trigger_dialog::TextTriggerDialogState::new(None);
        parent.regexp = TextPromptState::new("authored.*λ");
        parent.message = TextPromptState::new("authored message");
        parent.regexp.cursor = 3;
        parent.enabled = false;
        parent.focus = crate::text_trigger_dialog::TextTriggerFocus::Target;
        let before = parent.candidate();
        let host = ValueDialogHost::trigger_scope(&parent).unwrap();
        let ValueDialogState::Choice(choice) = &host.dialog else {
            panic!("catalog");
        };
        assert_eq!(
            choice.options().len(),
            ilium_ipc::TextTriggerTarget::ALL.len()
        );
        host.apply_trigger_scope_choice(&mut parent, "Agents")
            .unwrap();
        let mut expected = before.clone();
        expected.target = ilium_ipc::TextTriggerTarget::Agents;
        assert_eq!(parent.candidate(), expected);
        assert_eq!(parent.regexp.cursor, 3);
        assert_eq!(
            parent.focus,
            crate::text_trigger_dialog::TextTriggerFocus::Target
        );
        let mut replacement =
            crate::text_trigger_dialog::TextTriggerDialogState::new(Some((0, &before)));
        assert!(host
            .apply_trigger_scope_choice(&mut replacement, "Terminals")
            .is_err());
        assert_eq!(replacement.candidate(), before);
    }

    #[test]
    fn full_delivery_list_keeps_current_and_updates_only_parent_choice() {
        let mut parent = PromptQueueDialogState::new(NodeId(7));
        parent.text = TextPromptState::new("authored prompt");
        parent.times = TextPromptState::new("17");
        parent.focus = crate::prompt_queue::PromptQueueFocus::Delivery;
        let host = ValueDialogHost::queue_delivery(&parent).unwrap();
        let ValueDialogState::Choice(choice) = &host.dialog else {
            panic!("choice dialog");
        };
        assert_eq!(
            choice
                .options()
                .iter()
                .map(|option| option.id.as_str())
                .collect::<Vec<_>>(),
            vec!["once", "times", "forever"]
        );
        assert_eq!(choice.selected_id.as_deref(), Some("once"));
        host.apply_queue_choice(&mut parent, "times").unwrap();
        assert_eq!(parent.times.buf, "17");
        assert_eq!(parent.text.buf, "authored prompt");
        assert_eq!(
            parent.focus,
            crate::prompt_queue::PromptQueueFocus::Delivery
        );
        assert_eq!(
            parent.validated_request().unwrap(),
            (
                "authored prompt".into(),
                PromptQueueDelivery::Times { remaining_runs: 17 }
            )
        );
    }
    #[test]
    fn changed_parent_and_unknown_choice_are_rejected_without_mutation() {
        let original = PromptQueueDialogState::new(NodeId(7));
        let host = ValueDialogHost::queue_delivery(&original).unwrap();
        let mut changed = PromptQueueDialogState::new(NodeId(8));
        assert!(host.apply_queue_choice(&mut changed, "forever").is_err());
        assert_eq!(changed.delivery_choice, PromptQueueDelivery::Once);
        let mut parent = PromptQueueDialogState::new(NodeId(7));
        assert!(host.apply_queue_choice(&mut parent, "unknown").is_err());
        assert_eq!(parent.delivery_choice, PromptQueueDelivery::Once);
    }
    #[test]
    fn only_matching_receipt_releases_pending_exact_draft_for_retry() {
        let target = crate::value_animation::AnimationControlTarget::new(
            "/project".into(),
            "rain".into(),
            crate::value_animation::AnimationControlScope::Scene,
            ilium_ambient::Control::slider("speed", "Speed", 10, (0, 100, 10), "%", ""),
        )
        .unwrap();
        let mut host = ValueDialogHost::animation(target).unwrap();
        let ValueDialogState::Number(number) = &mut host.dialog else {
            panic!("number");
        };
        number.draft = TextPromptState::new("017");
        let token = std::sync::Arc::new(());
        host.begin_save(std::sync::Arc::clone(&token));
        assert!(host.is_saving());
        assert!(!host.finish_save(&std::sync::Arc::new(()), Err("foreign".into())));
        assert!(host.is_saving());
        assert!(host.finish_save(&token, Err("disk full".into())));
        assert!(!host.is_saving());
        let ValueDialogState::Number(number) = &host.dialog else {
            panic!("number");
        };
        assert_eq!(number.draft.buf, "017");
        assert_eq!(number.error.as_deref(), Some("disk full"));
        let retry = std::sync::Arc::new(());
        host.begin_save(std::sync::Arc::clone(&retry));
        assert!(!host.finish_save(&token, Ok(())));
        assert!(host.finish_save(&retry, Ok(())));
    }
}

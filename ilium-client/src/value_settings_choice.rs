//! Closed settings catalogs with stable enum identities and fresh commit validation.
use crate::app::{
    AgentMonitoringRow, App, AppearanceRow, EditorRow, Mode, SettingsTab, TerminalRow,
};
use crate::config::{
    AgentIdentifierMode, LineDisplay, MotionLevel, NewPaneDirectory, SidebarDensity, TreeOrder,
};
use crate::remote_compaction_settings::TechniqueTarget;
use crate::theme::ColorScheme;
use crate::value_dialog::{ChoiceDialogState, ChoiceOption, DialogOutcome, ValueDialogState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsChoice {
    TerminalDirectory,
    EditorLineDisplay,
    EditorMarkdown,
    TreeOrder,
    AgentIdentifier,
    ColorScheme,
    Motion,
    SidebarDensity,
    AttentionIndicator,
    LeftPanelSizingMode,
    AgentMonitoringMode,
    VoiceModel,
    VoiceName,
    VoiceReasoning,
    VoiceInputMode,
    VoiceVadEagerness,
    VoiceInputDevice,
    VoiceOutputDevice,
    SoundSource,
    SoundFile,
    TitleStyle,
    ResetTimeDisplay,
    InferenceProvider,
    KiloModel,
    OllamaModel,
    OpenAiModel,
    AnthropicModel,
    ProgressFillStyle,
    GitDefaultWhere,
    GitDefaultBase,
    GitBranchLine,
    GitClosePolicy,
    SessionRecovery,
    SmartCopyModifier,
    RemoteTechnique(TechniqueTarget),
}

fn sound_file_id(path: &std::path::Path) -> String {
    // Debug escapes preserve non-Unicode native path identities; selection
    // resolves against original PathBufs and never decodes display strings.
    format!("file:{:?}", path.as_os_str())
}

fn sound_file_catalog(app: &App) -> (Vec<ChoiceOption>, String) {
    let mut options = vec![ChoiceOption {
        id: "none".into(),
        label: "No file selected".into(),
        disabled_reason: None,
    }];
    for sound in &app.sound_discovery.sounds {
        let id = sound_file_id(&sound.path);
        if !options.iter().any(|option| option.id == id) {
            options.push(ChoiceOption {
                id,
                label: format!("{} — {}", sound.collection, sound.display_name),
                disabled_reason: None,
            });
        }
    }
    let selected = app
        .sound_settings
        .file
        .as_deref()
        .map_or_else(|| "none".into(), sound_file_id);
    if let Some(path) = app
        .sound_settings
        .file
        .as_deref()
        .filter(|_| !options.iter().any(|option| option.id == selected))
    {
        options.push(ChoiceOption {
            id: selected.clone(),
            label: format!("{} (outside discovered catalog)", path.display()),
            disabled_reason: Some(
                "This saved file is outside the currently discovered catalog".into(),
            ),
        });
    }
    (options, selected)
}

fn device_catalog(names: &[String], current: Option<&str>) -> (Vec<ChoiceOption>, String) {
    let mut options = vec![ChoiceOption {
        id: "default".into(),
        label: "System default".into(),
        disabled_reason: None,
    }];
    for name in names {
        let id = format!("device:{name}");
        if !options.iter().any(|option| option.id == id) {
            options.push(ChoiceOption {
                id,
                label: name.clone(),
                disabled_reason: None,
            });
        }
    }
    let selected = current.map_or_else(|| "default".into(), |name| format!("device:{name}"));
    if let Some(name) = current.filter(|_| !options.iter().any(|option| option.id == selected)) {
        options.push(ChoiceOption {
            id: selected.clone(),
            label: name.into(),
            disabled_reason: Some("This saved device is not currently available".into()),
        });
    }
    (options, selected)
}

fn catalog<T: Copy + std::fmt::Debug + PartialEq>(
    values: &[T],
    current: T,
    label: impl Fn(T) -> String,
) -> (Vec<ChoiceOption>, String) {
    let selected = format!("{current:?}");
    let mut options: Vec<_> = values
        .iter()
        .map(|value| ChoiceOption {
            id: format!("{value:?}"),
            label: label(*value),
            disabled_reason: None,
        })
        .collect();
    if !values.contains(&current) {
        // Cost sorting is a managed current TreeOrder outside its cyclic registry.
        options.push(ChoiceOption {
            id: selected.clone(),
            label: label(current),
            disabled_reason: Some(
                "Managed by another setting; choose a listed option to change this value".into(),
            ),
        });
    }
    (options, selected)
}

impl SettingsChoice {
    pub const ALL: [Self; 37] = [
        Self::TerminalDirectory,
        Self::EditorLineDisplay,
        Self::EditorMarkdown,
        Self::TreeOrder,
        Self::AgentIdentifier,
        Self::ColorScheme,
        Self::Motion,
        Self::SidebarDensity,
        Self::AttentionIndicator,
        Self::LeftPanelSizingMode,
        Self::AgentMonitoringMode,
        Self::VoiceModel,
        Self::VoiceName,
        Self::VoiceReasoning,
        Self::VoiceInputMode,
        Self::VoiceVadEagerness,
        Self::VoiceInputDevice,
        Self::VoiceOutputDevice,
        Self::SoundSource,
        Self::SoundFile,
        Self::TitleStyle,
        Self::ResetTimeDisplay,
        Self::InferenceProvider,
        Self::KiloModel,
        Self::OllamaModel,
        Self::OpenAiModel,
        Self::AnthropicModel,
        Self::ProgressFillStyle,
        Self::GitDefaultWhere,
        Self::GitDefaultBase,
        Self::GitBranchLine,
        Self::GitClosePolicy,
        Self::SessionRecovery,
        Self::SmartCopyModifier,
        Self::RemoteTechnique(TechniqueTarget::Claude),
        Self::RemoteTechnique(TechniqueTarget::Codex),
        Self::RemoteTechnique(TechniqueTarget::Other),
    ];

    pub fn at(app: &App, tab: SettingsTab, row: usize) -> Option<Self> {
        match tab {
            SettingsTab::Git => match crate::app::GitRow::ALL.get(row) {
                Some(crate::app::GitRow::DefaultWhere) => Some(Self::GitDefaultWhere),
                Some(crate::app::GitRow::DefaultBase) => Some(Self::GitDefaultBase),
                Some(crate::app::GitRow::BranchLine) => Some(Self::GitBranchLine),
                Some(crate::app::GitRow::DefaultClosePolicy) => Some(Self::GitClosePolicy),
                _ => None,
            },
            SettingsTab::RemoteCompaction => {
                match crate::remote_compaction_settings::RemoteCompactionRow::rows(
                    &app.remote_compaction_settings,
                )
                .get(row)
                {
                    Some(crate::remote_compaction_settings::RemoteCompactionRow::Technique(
                        target,
                    )) => Some(Self::RemoteTechnique(*target)),
                    _ => None,
                }
            }
            SettingsTab::Session
                if crate::app::SessionRow::ALL.get(row)
                    == Some(&crate::app::SessionRow::RecoveryPolicy) =>
            {
                Some(Self::SessionRecovery)
            }
            SettingsTab::Terminal
                if TerminalRow::ALL.get(row) == Some(&TerminalRow::SmartCopyLightKey) =>
            {
                Some(Self::SmartCopyModifier)
            }
            SettingsTab::Inference => {
                match crate::settings_ui::inference_rows(&app.inference_settings).get(row) {
                    Some(crate::app::InferenceRow::Provider) => Some(Self::InferenceProvider),
                    Some(crate::app::InferenceRow::KiloGatewayModel) => Some(Self::KiloModel),
                    Some(crate::app::InferenceRow::Field(
                        crate::app::InferenceSettingField::OllamaModel,
                    )) => Some(Self::OllamaModel),
                    Some(crate::app::InferenceRow::Field(
                        crate::app::InferenceSettingField::OpenAiModel,
                    )) => Some(Self::OpenAiModel),
                    Some(crate::app::InferenceRow::Field(
                        crate::app::InferenceSettingField::AnthropicModel,
                    )) => Some(Self::AnthropicModel),
                    _ => None,
                }
            }
            SettingsTab::ResetPlanning if row == 2 => Some(Self::ResetTimeDisplay),
            SettingsTab::Sound => match crate::app::SoundRow::ALL.get(row) {
                Some(crate::app::SoundRow::Source) => Some(Self::SoundSource),
                Some(crate::app::SoundRow::File) => Some(Self::SoundFile),
                _ => None,
            },
            SettingsTab::VoiceControl => match crate::voice_settings::VoiceRow::ALL.get(row) {
                Some(crate::voice_settings::VoiceRow::Model) => Some(Self::VoiceModel),
                Some(crate::voice_settings::VoiceRow::Voice) => Some(Self::VoiceName),
                Some(crate::voice_settings::VoiceRow::ReasoningEffort) => {
                    Some(Self::VoiceReasoning)
                }
                Some(crate::voice_settings::VoiceRow::InputMode) => Some(Self::VoiceInputMode),
                Some(crate::voice_settings::VoiceRow::VadEagerness) => {
                    Some(Self::VoiceVadEagerness)
                }
                Some(crate::voice_settings::VoiceRow::InputDevice) => Some(Self::VoiceInputDevice),
                Some(crate::voice_settings::VoiceRow::OutputDevice) => {
                    Some(Self::VoiceOutputDevice)
                }
                _ => None,
            },
            SettingsTab::Terminal
                if TerminalRow::ALL.get(row) == Some(&TerminalRow::NewPaneDirectory) =>
            {
                Some(Self::TerminalDirectory)
            }
            SettingsTab::Titles if row == 0 => Some(Self::TitleStyle),
            SettingsTab::Editor => match EditorRow::ALL.get(row) {
                Some(EditorRow::LineDisplay) => Some(Self::EditorLineDisplay),
                Some(EditorRow::MarkdownDefault) => Some(Self::EditorMarkdown),
                _ => None,
            },
            SettingsTab::Appearance => {
                match AppearanceRow::visible(app.ui_settings.left_panel_sizing.mode).get(row) {
                    Some(AppearanceRow::LeftPanelSizingMode) => Some(Self::LeftPanelSizingMode),
                    Some(AppearanceRow::ProgressFillStyle) => Some(Self::ProgressFillStyle),
                    Some(AppearanceRow::TreeOrder) => Some(Self::TreeOrder),
                    Some(AppearanceRow::AgentIdentifierMode) => Some(Self::AgentIdentifier),
                    Some(AppearanceRow::ColorScheme) => Some(Self::ColorScheme),
                    Some(AppearanceRow::MotionLevel) => Some(Self::Motion),
                    Some(AppearanceRow::SidebarDensity) => Some(Self::SidebarDensity),
                    _ => None,
                }
            }
            SettingsTab::AgentMonitoring => {
                match crate::settings_ui::agent_monitoring_rows(app).get(row) {
                    Some(AgentMonitoringRow::Mode) => Some(Self::AgentMonitoringMode),
                    Some(AgentMonitoringRow::AttentionRunningIndicator) => {
                        Some(Self::AttentionIndicator)
                    }
                    Some(AgentMonitoringRow::ProgressFillStyle) => Some(Self::ProgressFillStyle),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::GitDefaultWhere => "Default where",
            Self::GitDefaultBase => "Default base",
            Self::GitBranchLine => "Branch line",
            Self::GitClosePolicy => "Close policy",
            Self::SessionRecovery => "Recovery policy",
            Self::SmartCopyModifier => "Smart Copy modifier",
            Self::ProgressFillStyle => "Progress fill style",
            Self::InferenceProvider => "Provider",
            Self::KiloModel => "Kilo model",
            Self::OllamaModel => "Ollama model",
            Self::OpenAiModel => "OpenAI model",
            Self::AnthropicModel => "Anthropic model",
            Self::ResetTimeDisplay => "Time display",
            Self::SoundSource => "Sound source",
            Self::SoundFile => "Sound file",
            Self::TitleStyle => "Title style",
            Self::VoiceModel => "Model",
            Self::VoiceName => "Voice",
            Self::VoiceReasoning => "Reasoning effort",
            Self::VoiceInputMode => "Input mode",
            Self::VoiceVadEagerness => "VAD eagerness",
            Self::VoiceInputDevice => "Input device",
            Self::VoiceOutputDevice => "Output device",
            Self::TerminalDirectory => "New pane directory",
            Self::EditorLineDisplay => "Long lines",
            Self::EditorMarkdown => "Markdown default",
            Self::TreeOrder => "Tree order",
            Self::AgentIdentifier => "Agent identifier",
            Self::ColorScheme => "Color scheme",
            Self::Motion => "Motion level",
            Self::SidebarDensity => "Sidebar density",
            Self::AttentionIndicator => "Attention running indicator",
            Self::LeftPanelSizingMode => "Left panel sizing",
            Self::AgentMonitoringMode => "Agent Monitoring mode",
            Self::RemoteTechnique(TechniqueTarget::Claude) => "Claude technique",
            Self::RemoteTechnique(TechniqueTarget::Codex) => "Codex technique",
            Self::RemoteTechnique(TechniqueTarget::Other) => "Other agents technique",
        }
    }

    pub fn options(self, app: &App) -> (Vec<ChoiceOption>, String) {
        match self {
            Self::GitDefaultWhere => catalog(
                &crate::config::GitDefaultWhere::ALL,
                app.git_settings.default_where,
                |value| value.label().into(),
            ),
            Self::GitDefaultBase => catalog(
                &crate::config::GitDefaultBase::ALL,
                app.git_settings.default_base,
                |value| value.label().into(),
            ),
            Self::GitBranchLine => catalog(
                &crate::config::GitBranchLine::ALL,
                app.git_settings.branch_line,
                |value| value.label().into(),
            ),
            Self::GitClosePolicy => catalog(
                &crate::config::GitClosePolicy::ALL,
                app.git_settings.default_close_policy,
                |value| value.label().into(),
            ),
            Self::SessionRecovery => catalog(
                &crate::config::SessionRecoveryPolicy::ALL,
                app.session_settings.recovery_policy,
                |value| value.label().into(),
            ),
            Self::SmartCopyModifier => catalog(
                &crate::config::SmartCopyLightKey::ALL,
                app.terminal_settings.smart_copy_light_key,
                |value| value.label().into(),
            ),
            Self::ProgressFillStyle => {
                let names = crate::icon_settings::TASK_PROGRESS_STYLE_NAMES;
                let mut options: Vec<_> = names
                    .iter()
                    .map(|name| ChoiceOption {
                        id: (*name).into(),
                        label: (*name).into(),
                        disabled_reason: None,
                    })
                    .collect();
                let selected = if let Some(index) = crate::icon_settings::task_progress_preset_index(
                    &app.ui_settings.icons.task_progress_frames,
                ) {
                    names[index].to_owned()
                } else {
                    options.push(ChoiceOption { id: "custom".into(), label: "Custom (current authored frames)".into(), disabled_reason: Some("The current authored frame list is retained; select a preset only to replace it".into()) });
                    "custom".into()
                };
                (options, selected)
            }
            Self::OllamaModel | Self::OpenAiModel | Self::AnthropicModel => {
                let (models, current) = match self {
                    Self::OllamaModel => (&app.ollama_models, &app.inference_settings.ollama.model),
                    Self::AnthropicModel => (
                        &app.anthropic_models,
                        &app.inference_settings.anthropic.model,
                    ),
                    _ => (&app.openai_models, &app.inference_settings.openai.model),
                };
                let mut options = Vec::new();
                let mut ids = std::collections::HashSet::new();
                for model in std::iter::once(current)
                    .chain(models.iter())
                    .filter(|model| !model.trim().is_empty())
                {
                    if ids.insert(model.as_str()) {
                        options.push(ChoiceOption {
                            id: model.clone(),
                            label: model.clone(),
                            disabled_reason: None,
                        });
                    }
                }
                if options.is_empty() {
                    options.push(ChoiceOption { id: "unavailable".into(), label: "No model configured".into(), disabled_reason: Some("Press E on the model row to enter a name, or refresh the model catalog".into()) });
                }
                let selected = if current.trim().is_empty() {
                    options[0].id.clone()
                } else {
                    current.clone()
                };
                (options, selected)
            }
            Self::InferenceProvider => catalog(
                &ilium_inference::InferenceProviderKind::ALL,
                app.inference_settings.selected_provider,
                |provider| provider.label().into(),
            ),
            Self::KiloModel => (
                crate::value_inference::OnboardingInference::KiloModel.options(app),
                app.inference_settings.kilo_gateway.model.clone(),
            ),
            Self::ResetTimeDisplay => catalog(
                &[
                    crate::reset_planning::ResetTimeStyle::Exact,
                    crate::reset_planning::ResetTimeStyle::Human,
                ],
                app.reset_planning_settings.time_style,
                |value| match value {
                    crate::reset_planning::ResetTimeStyle::Exact => "Exact · d h m s".into(),
                    crate::reset_planning::ResetTimeStyle::Human => "Human · rounded".into(),
                },
            ),
            Self::SoundSource => catalog(
                &ilium_sound::SoundSourceKind::ALL,
                app.sound_settings.source,
                |value| value.label().into(),
            ),
            Self::SoundFile => sound_file_catalog(app),
            Self::TitleStyle => catalog(
                &[
                    ilium_inference::TitleStyle::Labeling,
                    ilium_inference::TitleStyle::Summarization,
                ],
                app.inference_settings.title_style,
                |value| match value {
                    ilium_inference::TitleStyle::Labeling => "Labeling".into(),
                    ilium_inference::TitleStyle::Summarization => "Summarization".into(),
                },
            ),
            Self::VoiceModel => catalog(
                &ilium_voice::VoiceModel::ALL,
                app.voice_settings.model,
                |value| value.label().into(),
            ),
            Self::VoiceName => catalog(
                &ilium_voice::VoiceName::ALL,
                app.voice_settings.voice,
                |value| value.label().into(),
            ),
            Self::VoiceReasoning => catalog(
                &ilium_voice::ReasoningEffort::ALL,
                app.voice_settings.reasoning_effort,
                |value| value.label().into(),
            ),
            Self::VoiceInputMode => catalog(
                &ilium_voice::VoiceInputMode::ALL,
                app.voice_settings.input_mode,
                |value| value.label().into(),
            ),
            Self::VoiceVadEagerness => catalog(
                &ilium_voice::VadEagerness::ALL,
                app.voice_settings.vad_eagerness,
                |value| value.label().into(),
            ),
            Self::VoiceInputDevice => device_catalog(
                &app.voice_input_devices,
                app.voice_settings.input_device_name.as_deref(),
            ),
            Self::VoiceOutputDevice => device_catalog(
                &app.voice_output_devices,
                app.voice_settings.output_device_name.as_deref(),
            ),
            Self::TerminalDirectory => catalog(
                &NewPaneDirectory::ALL,
                app.terminal_settings.new_pane_directory,
                |value| value.label().into(),
            ),
            Self::EditorLineDisplay => catalog(
                &[LineDisplay::Clip, LineDisplay::Wrap],
                app.editor_settings.line_display,
                |value| value.label().into(),
            ),
            Self::EditorMarkdown => catalog(
                &[false, true],
                app.editor_settings.markdown_rendered_by_default,
                |value| if value { "Rendered" } else { "Source" }.into(),
            ),
            Self::TreeOrder => catalog(&TreeOrder::ALL, app.ui_settings.tree_order, |value| {
                value.label().into()
            }),
            Self::AgentIdentifier => catalog(
                &AgentIdentifierMode::ALL,
                app.ui_settings.agent_identifiers.mode,
                |value| value.label().into(),
            ),
            Self::ColorScheme => catalog(
                &[ColorScheme::Dark, ColorScheme::Light],
                app.ui_settings.color_scheme,
                |value| {
                    match value {
                        ColorScheme::Dark => "Dark",
                        ColorScheme::Light => "Light",
                    }
                    .into()
                },
            ),
            Self::Motion => catalog(&MotionLevel::ALL, app.ui_settings.motion_level, |value| {
                value.label().into()
            }),
            Self::SidebarDensity => catalog(
                &SidebarDensity::ALL,
                app.ui_settings.sidebar_density,
                |value| value.label().into(),
            ),
            Self::AttentionIndicator => catalog(
                &crate::agent_monitoring::AttentionRunningIndicator::ALL,
                app.ui_settings.attention_running_indicator,
                |value| value.label().into(),
            ),
            Self::LeftPanelSizingMode => catalog(
                &crate::config::LeftPanelSizingMode::ALL,
                app.ui_settings.left_panel_sizing.mode,
                |value| value.label().into(),
            ),
            Self::AgentMonitoringMode => catalog(
                &crate::agent_monitoring::AgentMonitoringMode::ALL,
                app.ui_settings.agent_monitoring_mode,
                |value| value.label().into(),
            ),
            Self::RemoteTechnique(target) => (
                ilium_remote_compaction::Technique::ALL
                    .into_iter()
                    .map(|value| ChoiceOption {
                        id: value.id().into(),
                        label: value.label().into(),
                        disabled_reason: None,
                    })
                    .collect(),
                app.remote_compaction_settings.technique(target).id().into(),
            ),
        }
    }

    pub fn dialog(self, app: &App) -> Result<ValueDialogState, String> {
        let (options, selected) = self.options(app);
        Ok(ValueDialogState::Choice(ChoiceDialogState::new(
            self.title(),
            options,
            Some(selected),
        )?))
    }
}

impl App {
    fn save_session_choice(
        &mut self,
        id: &str,
        directory: std::path::PathBuf,
        token: Option<std::sync::Arc<()>>,
        baseline: crate::config::SessionSettings,
    ) -> Result<(), String> {
        use crate::filesystem::configuration::ConfigurationChange;
        use crate::filesystem::configurations::ConfigurationIntent;
        let mut previous = self.session_settings;
        previous.recovery_policy = baseline.recovery_policy;
        let mut desired = self.session_settings;
        desired.recovery_policy = crate::config::SessionRecoveryPolicy::ALL
            .into_iter()
            .find(|value| format!("{value:?}") == id)
            .ok_or("This recovery policy is no longer available")?;
        let intent = token.map_or(ConfigurationIntent::Session { desired }, |token| {
            ConfigurationIntent::SessionValueDialog { desired, token }
        });
        self.enqueue_configuration(
            directory,
            ConfigurationChange::Session { previous, desired },
            intent,
        )?;
        self.apply_session_settings(desired);
        Ok(())
    }

    /// Session writes merge against their original snapshot. A durable receipt
    /// may include a concurrent backup-setting edit and must reconcile it before
    /// closing the matching choice child. Observed readback is never success.
    pub(crate) fn collect_session_choice_configuration(
        &mut self,
        desired: crate::config::SessionSettings,
        token: &std::sync::Arc<()>,
        completion: crate::filesystem::ordered::WriteCompletion<
            crate::filesystem::configuration::ConfigurationWrite,
        >,
    ) {
        use crate::filesystem::configuration::ConfigurationSaved;
        use crate::filesystem::ordered::WriteCompletion;
        use ilium_execution::JobOutcome;
        match completion {
            WriteCompletion::Outcome { outcome, .. } => {
                let _retained = outcome.map(|outcome| {
                    let result = match outcome {
                        JobOutcome::Finished(Ok(ConfigurationSaved::Session(saved))) => {
                            if self.session_settings == desired {
                                self.apply_session_settings(saved);
                                Ok(())
                            } else {
                                Err("Session settings changed; reopen this dialog".into())
                            }
                        }
                        JobOutcome::Finished(Err(failure)) => {
                            if self.session_settings == desired {
                                if let Some(observed) = failure.observed_session {
                                    self.apply_session_settings(observed);
                                }
                            }
                            Err(failure.message)
                        }
                        JobOutcome::Finished(Ok(_)) => {
                            Err("Unexpected session receipt; publication is unconfirmed".into())
                        }
                        JobOutcome::NotStarted { .. } | JobOutcome::Panicked => Err(
                            "Session settings worker did not complete; publication is unconfirmed"
                                .into(),
                        ),
                    };
                    self.finish_value_dialog_save(token, result);
                });
            }
            WriteCompletion::Rejected { rejection, .. } => self.finish_value_dialog_save(
                token,
                Err(format!(
                    "Session settings remain unsaved: {:?}",
                    rejection.reason
                )),
            ),
            WriteCompletion::Lost { .. } => self.finish_value_dialog_save(
                token,
                Err("Session settings receipt lost; publication is unconfirmed".into()),
            ),
        }
    }

    pub(crate) fn begin_settings_choice_dialog(&mut self, field: SettingsChoice) {
        let result = self
            .config_dir
            .clone()
            .ok_or_else(|| "The configuration directory is unavailable".to_owned())
            .and_then(|directory| {
                crate::value_dialog_host::ValueDialogHost::settings_choice(field, self, directory)
            });
        match result {
            Ok(host) => self.push_modal(Mode::ValueDialog(Box::new(host))),
            Err(error) => self.status_message = Some(error),
        }
    }

    pub(crate) fn commit_settings_choice_dialog(
        &mut self,
        host: &mut crate::value_dialog_host::ValueDialogHost,
        outcome: &DialogOutcome,
    ) -> Result<(), String> {
        let crate::value_dialog_host::ValueTarget::SettingsChoice {
            field,
            directory,
            inference_revision,
            session_previous,
        } = &host.target
        else {
            return Err("This is not a settings choice dialog".into());
        };
        if inference_revision.is_some_and(|revision| revision != self.onboarding_revision) {
            return Err("Inference settings changed; reopen this dialog".into());
        }
        if host.is_saving() {
            return Err("The previous choice is still being saved".into());
        }
        if self.config_dir.as_ref() != Some(directory) {
            return Err("The configuration destination changed; reopen this dialog".into());
        }
        let Some(Mode::Settings(parent)) = self.modal_stack.last() else {
            return Err("The settings parent changed; reopen this dialog".into());
        };
        if SettingsChoice::at(self, parent.tab, parent.selected_row) != Some(*field) {
            return Err("The selected setting changed; reopen this dialog".into());
        }
        let DialogOutcome::Choose(id) = outcome else {
            return Err("Choose an option for this setting".into());
        };
        let token = std::sync::Arc::new(());
        if let Some(previous) = session_previous {
            self.save_session_choice(id, directory.clone(), Some(token.clone()), *previous)?;
        } else {
            self.save_settings_choice(*field, id, directory.clone(), Some(token.clone()))?;
        }
        if let crate::value_dialog_host::ValueTarget::SettingsChoice {
            inference_revision: Some(revision),
            ..
        } = &mut host.target
        {
            *revision = self.onboarding_revision;
        }
        host.begin_save(token);
        Ok(())
    }

    pub(crate) fn step_settings_choice(&mut self, field: SettingsChoice, direction: i32) {
        if direction == 0 {
            return;
        }
        let result = (|| {
            let (options, selected) = field.options(self);
            let enabled: Vec<_> = options
                .iter()
                .filter(|option| option.disabled_reason.is_none())
                .collect();
            if enabled.is_empty() {
                return Err("No options are available".into());
            }
            let current = enabled.iter().position(|option| option.id == selected);
            let next = current.map_or(if direction < 0 { enabled.len() - 1 } else { 0 }, |index| {
                (index as i64 + i64::from(direction.signum())).rem_euclid(enabled.len() as i64)
                    as usize
            });
            let directory = self
                .config_dir
                .clone()
                .ok_or_else(|| "The configuration directory is unavailable".to_owned())?;
            self.save_settings_choice(field, &enabled[next].id, directory, None)
        })();
        if let Err(error) = result {
            self.status_message = Some(error);
        }
    }

    pub(crate) fn save_settings_choice(
        &mut self,
        field: SettingsChoice,
        id: &str,
        directory: std::path::PathBuf,
        token: Option<std::sync::Arc<()>>,
    ) -> Result<(), String> {
        use crate::filesystem::configuration::ConfigurationChange;
        use crate::filesystem::configurations::ConfigurationIntent;
        if field == SettingsChoice::SessionRecovery {
            return self.save_session_choice(id, directory, token, self.session_settings);
        }
        let (options, _) = field.options(self);
        let option = options
            .iter()
            .find(|option| option.id == id)
            .ok_or("This option is no longer available")?;
        if let Some(reason) = &option.disabled_reason {
            return Err(reason.clone());
        }
        if matches!(
            field,
            SettingsChoice::InferenceProvider
                | SettingsChoice::TitleStyle
                | SettingsChoice::KiloModel
                | SettingsChoice::OllamaModel
                | SettingsChoice::OpenAiModel
                | SettingsChoice::AnthropicModel
        ) {
            let mut desired = self.inference_settings.clone();
            match field {
                SettingsChoice::InferenceProvider => {
                    desired.selected_provider = ilium_inference::InferenceProviderKind::ALL
                        .into_iter()
                        .find(|provider| format!("{provider:?}") == id)
                        .ok_or("Provider is unavailable")?
                }
                SettingsChoice::TitleStyle => {
                    desired.title_style = [
                        ilium_inference::TitleStyle::Labeling,
                        ilium_inference::TitleStyle::Summarization,
                    ]
                    .into_iter()
                    .find(|value| format!("{value:?}") == id)
                    .ok_or("Title style is unavailable")?;
                }
                SettingsChoice::KiloModel => desired.kilo_gateway.model = id.into(),
                SettingsChoice::OllamaModel => desired.ollama.model = id.into(),
                SettingsChoice::OpenAiModel => desired.openai.model = id.into(),
                SettingsChoice::AnthropicModel => desired.anthropic.model = id.into(),
                _ => return Err("This is not an inference choice".into()),
            }
            self.enqueue_inference_value(directory, &desired, token)?;
            self.apply_inference_settings_locally(desired);
            return Ok(());
        }
        macro_rules! select {
            ($values:expr, $destination:expr) => {{
                $destination = $values
                    .iter()
                    .copied()
                    .find(|value| format!("{value:?}") == id)
                    .ok_or("This option is no longer available")?;
            }};
        }
        let mut git = self.git_settings.clone();
        let mut terminal = self.terminal_settings;
        let mut editor = self.editor_settings;
        let mut ui = self.ui_settings.clone();
        let mut voice = self.voice_settings.clone();
        let mut sound = self.sound_settings.clone();
        let mut resets = self.reset_planning_settings.clone();
        let mut remote_compaction = self.remote_compaction_settings.clone();
        match field {
            SettingsChoice::GitDefaultWhere => {
                select!(crate::config::GitDefaultWhere::ALL, git.default_where)
            }
            SettingsChoice::GitDefaultBase => {
                select!(crate::config::GitDefaultBase::ALL, git.default_base)
            }
            SettingsChoice::GitBranchLine => {
                select!(crate::config::GitBranchLine::ALL, git.branch_line)
            }
            SettingsChoice::GitClosePolicy => {
                select!(crate::config::GitClosePolicy::ALL, git.default_close_policy)
            }
            SettingsChoice::SessionRecovery => {
                return Err("Session choice was not dispatched".into());
            }
            SettingsChoice::SmartCopyModifier => select!(
                crate::config::SmartCopyLightKey::ALL,
                terminal.smart_copy_light_key
            ),
            SettingsChoice::InferenceProvider
            | SettingsChoice::TitleStyle
            | SettingsChoice::KiloModel
            | SettingsChoice::OllamaModel
            | SettingsChoice::OpenAiModel
            | SettingsChoice::AnthropicModel => {
                return Err("Inference choice was not dispatched".into());
            }
            SettingsChoice::ResetTimeDisplay => {
                select!(
                    [
                        crate::reset_planning::ResetTimeStyle::Exact,
                        crate::reset_planning::ResetTimeStyle::Human
                    ],
                    resets.time_style
                );
            }
            SettingsChoice::SoundSource => {
                select!(ilium_sound::SoundSourceKind::ALL, sound.source);
                if sound.source == ilium_sound::SoundSourceKind::SoundFile && sound.file.is_none() {
                    sound.file = self
                        .sound_discovery
                        .sounds
                        .first()
                        .map(|entry| entry.path.clone());
                }
            }
            SettingsChoice::SoundFile => {
                sound.file = if id == "none" {
                    None
                } else {
                    Some(
                        self.sound_discovery
                            .sounds
                            .iter()
                            .find(|entry| sound_file_id(&entry.path) == id)
                            .ok_or("This sound file is no longer available")?
                            .path
                            .clone(),
                    )
                };
                sound.source = ilium_sound::SoundSourceKind::SoundFile;
            }
            SettingsChoice::VoiceModel => {
                select!(ilium_voice::VoiceModel::ALL, voice.model);
            }
            SettingsChoice::VoiceName => {
                select!(ilium_voice::VoiceName::ALL, voice.voice);
            }
            SettingsChoice::VoiceReasoning => {
                select!(ilium_voice::ReasoningEffort::ALL, voice.reasoning_effort);
            }
            SettingsChoice::VoiceInputMode => {
                select!(ilium_voice::VoiceInputMode::ALL, voice.input_mode);
            }
            SettingsChoice::VoiceVadEagerness => {
                select!(ilium_voice::VadEagerness::ALL, voice.vad_eagerness);
            }
            SettingsChoice::VoiceInputDevice | SettingsChoice::VoiceOutputDevice => {
                let device = if id == "default" {
                    None
                } else {
                    Some(
                        id.strip_prefix("device:")
                            .ok_or("This device option is no longer available")?
                            .to_owned(),
                    )
                };
                match field {
                    SettingsChoice::VoiceInputDevice => voice.input_device_name = device,
                    _ => voice.output_device_name = device,
                }
            }
            SettingsChoice::TerminalDirectory => {
                select!(NewPaneDirectory::ALL, terminal.new_pane_directory);
            }
            SettingsChoice::EditorLineDisplay => {
                select!([LineDisplay::Clip, LineDisplay::Wrap], editor.line_display);
            }
            SettingsChoice::EditorMarkdown => {
                select!([false, true], editor.markdown_rendered_by_default);
            }
            SettingsChoice::TreeOrder => {
                select!(TreeOrder::ALL, ui.tree_order);
            }
            SettingsChoice::AgentIdentifier => {
                select!(AgentIdentifierMode::ALL, ui.agent_identifiers.mode);
            }
            SettingsChoice::ColorScheme => {
                select!([ColorScheme::Dark, ColorScheme::Light], ui.color_scheme);
            }
            SettingsChoice::Motion => {
                select!(MotionLevel::ALL, ui.motion_level);
            }
            SettingsChoice::SidebarDensity => {
                select!(SidebarDensity::ALL, ui.sidebar_density);
            }
            SettingsChoice::ProgressFillStyle => {
                let index = crate::icon_settings::TASK_PROGRESS_STYLE_NAMES
                    .iter()
                    .position(|name| *name == id)
                    .ok_or("This frame preset is no longer available")?;
                ui.icons.task_progress_frames =
                    crate::icon_settings::task_progress_preset_frames(index);
            }
            SettingsChoice::AttentionIndicator => {
                select!(
                    crate::agent_monitoring::AttentionRunningIndicator::ALL,
                    ui.attention_running_indicator
                );
            }
            SettingsChoice::LeftPanelSizingMode => {
                select!(
                    crate::config::LeftPanelSizingMode::ALL,
                    ui.left_panel_sizing.mode
                );
            }
            SettingsChoice::AgentMonitoringMode => {
                select!(
                    crate::agent_monitoring::AgentMonitoringMode::ALL,
                    ui.agent_monitoring_mode
                );
            }
            SettingsChoice::RemoteTechnique(target) => {
                let value = ilium_remote_compaction::Technique::from_id(id)
                    .ok_or("This technique is no longer available")?;
                *remote_compaction.technique_mut(target) = value;
            }
        }
        let intent = token.map_or(
            ConfigurationIntent::Plain {
                label: "settings choice",
                success: None,
            },
            |token| ConfigurationIntent::ValueDialog { token },
        );
        match field {
            SettingsChoice::GitDefaultWhere
            | SettingsChoice::GitDefaultBase
            | SettingsChoice::GitBranchLine
            | SettingsChoice::GitClosePolicy => {
                git.validate()?;
                self.enqueue_configuration(
                    directory,
                    ConfigurationChange::Git(git.clone()),
                    intent,
                )?;
                self.apply_git_settings(git);
            }
            SettingsChoice::ResetTimeDisplay => {
                self.enqueue_configuration(
                    directory,
                    ConfigurationChange::ResetPlanning(resets.clone()),
                    intent,
                )?;
                self.apply_reset_planning_settings(resets);
            }
            SettingsChoice::SoundSource | SettingsChoice::SoundFile => {
                self.enqueue_configuration(
                    directory,
                    ConfigurationChange::Sound(sound.clone()),
                    intent,
                )?;
                self.apply_sound_settings(sound.clone());
                // The current server receives the same live update as the
                // original settings handler; other servers use their watcher.
                self.queue_request(ilium_ipc::ClientRequest::UpdateSoundSettings {
                    settings: sound,
                });
            }
            SettingsChoice::VoiceModel
            | SettingsChoice::VoiceName
            | SettingsChoice::VoiceReasoning
            | SettingsChoice::VoiceInputMode
            | SettingsChoice::VoiceVadEagerness
            | SettingsChoice::VoiceInputDevice
            | SettingsChoice::VoiceOutputDevice => {
                self.enqueue_configuration(
                    directory,
                    ConfigurationChange::Voice(voice.clone()),
                    intent,
                )?;
                self.apply_voice_runtime_settings(voice);
            }
            SettingsChoice::RemoteTechnique(_) => {
                self.enqueue_configuration(
                    directory,
                    ConfigurationChange::RemoteCompaction(remote_compaction.clone()),
                    intent,
                )?;
                self.apply_remote_compaction_settings(remote_compaction);
            }
            SettingsChoice::TerminalDirectory | SettingsChoice::SmartCopyModifier => {
                self.enqueue_configuration(
                    directory,
                    ConfigurationChange::Terminal(terminal),
                    intent,
                )?;
                self.apply_terminal_settings(terminal);
            }
            SettingsChoice::EditorLineDisplay | SettingsChoice::EditorMarkdown => {
                self.enqueue_configuration(directory, ConfigurationChange::Editor(editor), intent)?;
                self.apply_editor_settings(editor);
            }
            _ => {
                self.enqueue_configuration(
                    directory,
                    ConfigurationChange::Ui(Box::new(ui.clone())),
                    intent,
                )?;
                self.apply_ui_settings(ui);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::SettingsState;

    #[test]
    fn sound_catalog_retains_authored_file_and_selection_updates_disk_and_server() {
        let directory = tempfile::tempdir().unwrap();
        let mut app = App::new("sound-catalog".into(), directory.path().into());
        app.config_dir = Some(directory.path().into());
        let file = directory.path().join("Unicode chime λ.wav");
        let entry = ilium_sound::SystemSound {
            path: file.clone(),
            display_name: "Test chime".into(),
            collection: "Synthetic test catalog".into(),
        };
        app.sound_discovery.sounds = vec![entry.clone(), entry];
        let retained = directory.path().join("authored-outside-catalog.wav");
        app.sound_settings.file = Some(retained.clone());
        let design = app.sound_settings.design.clone();
        let events = app.sound_settings.events;
        let (options, selected) = SettingsChoice::SoundFile.options(&app);
        assert_eq!(options.len(), 3);
        assert_eq!(selected, sound_file_id(&retained));
        assert!(options.last().unwrap().disabled_reason.is_some());
        let attempts = app.configuration_admission.attempts;
        assert!(app
            .save_settings_choice(
                SettingsChoice::SoundFile,
                &selected,
                directory.path().into(),
                None
            )
            .is_err());
        assert_eq!(app.sound_settings.file, Some(retained));
        assert_eq!(app.configuration_admission.attempts, attempts);
        let id = sound_file_id(&file);
        app.save_settings_choice(
            SettingsChoice::SoundFile,
            &id,
            directory.path().into(),
            None,
        )
        .unwrap();
        assert_eq!(app.sound_settings.file, Some(file.clone()));
        assert_eq!(
            app.sound_settings.source,
            ilium_sound::SoundSourceKind::SoundFile
        );
        assert_eq!(app.sound_settings.design, design);
        assert_eq!(app.sound_settings.events, events);
        assert!(app.take_outbound_requests().into_iter().any(|request| matches!(request, ilium_ipc::ClientRequest::UpdateSoundSettings { settings } if settings.file == Some(file.clone()) && settings.design == design && settings.events == events)));
        app.settle_filesystem_for_test();
        let saved = crate::config::load(directory.path()).unwrap().sound;
        assert_eq!(saved.file, Some(file));
        assert_eq!(saved.design, design);
        assert_eq!(saved.events, events);
        app.sound_discovery.sounds.clear();
        let before = app.sound_settings.clone();
        assert!(app
            .save_settings_choice(
                SettingsChoice::SoundFile,
                &id,
                directory.path().into(),
                None
            )
            .is_err());
        assert_eq!(app.sound_settings, before);
    }

    #[test]
    fn device_list_retains_unavailable_current_and_rejects_disappeared_selection() {
        let directory = tempfile::tempdir().unwrap();
        let mut app = App::new("device-catalog".into(), directory.path().into());
        app.voice_input_devices = vec![
            "Test microphone".into(),
            "Test microphone".into(),
            "USB mic".into(),
        ];
        app.voice_settings.input_device_name = Some("Unplugged mic".into());
        let (options, selected) = SettingsChoice::VoiceInputDevice.options(&app);
        assert_eq!(options.len(), 4);
        assert_eq!(selected, "device:Unplugged mic");
        assert!(options.last().unwrap().disabled_reason.is_some());
        assert_eq!(
            options
                .iter()
                .filter(|option| option.id == "device:Test microphone")
                .count(),
            1
        );
        app.voice_input_devices.clear();
        let original = app.voice_settings.clone();
        let attempts = app.configuration_admission.attempts;
        assert!(app
            .save_settings_choice(
                SettingsChoice::VoiceInputDevice,
                "device:Test microphone",
                directory.path().into(),
                None
            )
            .is_err());
        assert_eq!(app.voice_settings, original);
        assert_eq!(app.configuration_admission.attempts, attempts);
        assert!(app.take_voice_runtime_request().is_none());
    }

    #[test]
    fn settings_choice_value_left_advances_right_reverses_and_plus_opens_catalog() {
        use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
        use ratatui::layout::Rect;
        for width in [140, 80, 60, 40] {
            let directory = tempfile::tempdir().unwrap();
            let mut app = App::new(format!("choice-pointer-{width}"), directory.path().into());
            app.config_dir = Some(directory.path().into());
            app.set_screen_area(Rect::new(0, 0, width, 60));
            app.terminal_settings.new_pane_directory = NewPaneDirectory::LastUsed;
            app.mode = Mode::Settings(SettingsState {
                tab: SettingsTab::Terminal,
                selected_row: 1,
                ..SettingsState::default()
            });
            for (button, expected) in [
                (MouseButton::Left, NewPaneDirectory::ProjectRoot),
                (MouseButton::Right, NewPaneDirectory::LastUsed),
            ] {
                let Mode::Settings(state) = &app.mode else {
                    panic!("settings");
                };
                let layout = crate::settings_ui::compute_layout_for_mode(
                    app.layout.screen_area,
                    &app,
                    state,
                );
                let (_, control) = crate::settings_ui::settings_choice_control(
                    layout.content_area,
                    &app,
                    state,
                    1,
                )
                .unwrap();
                let value = control.geometry().value;
                crate::mouse::handle_mouse_event(
                    &mut app,
                    MouseEvent {
                        kind: MouseEventKind::Down(button),
                        column: value.x,
                        row: value.y,
                        modifiers: KeyModifiers::NONE,
                    },
                );
                assert_eq!(
                    app.terminal_settings.new_pane_directory, expected,
                    "{width}"
                );
            }
            app.settle_filesystem_for_test();
            assert_eq!(
                crate::config::load(directory.path())
                    .unwrap()
                    .terminal
                    .new_pane_directory,
                NewPaneDirectory::LastUsed,
                "{width}"
            );
            let Mode::Settings(state) = &app.mode else {
                panic!("settings");
            };
            let layout =
                crate::settings_ui::compute_layout_for_mode(app.layout.screen_area, &app, state);
            let (_, control) =
                crate::settings_ui::settings_choice_control(layout.content_area, &app, state, 1)
                    .unwrap();
            let open = control.geometry().open;
            crate::mouse::handle_mouse_event(
                &mut app,
                MouseEvent {
                    kind: MouseEventKind::Down(MouseButton::Left),
                    column: open.x,
                    row: open.y,
                    modifiers: KeyModifiers::NONE,
                },
            );
            let Mode::ValueDialog(host) = &app.mode else {
                panic!("full catalog at {width}");
            };
            let ValueDialogState::Choice(choice) = &host.dialog else {
                panic!("choice at {width}");
            };
            assert_eq!(
                choice.options().len(),
                NewPaneDirectory::ALL.len(),
                "{width}"
            );
            assert_eq!(choice.selected_id.as_deref(), Some("LastUsed"), "{width}");
            let project_root_index = choice
                .options()
                .iter()
                .position(|option| option.id == "ProjectRoot")
                .expect("the full catalog exposes the ProjectRoot option");
            let layout = crate::value_dialog::dialog_layout(app.layout.screen_area);
            let selection = ratatui::layout::Position::new(
                layout.document.x + 1,
                layout.document.y + project_root_index as u16,
            );
            crate::mouse::handle_mouse_event(
                &mut app,
                MouseEvent {
                    kind: MouseEventKind::Down(MouseButton::Left),
                    column: selection.x,
                    row: selection.y,
                    modifiers: KeyModifiers::NONE,
                },
            );
            app.settle_filesystem_for_test();
            assert!(matches!(app.mode, Mode::Settings(_)), "{width}");
            assert_eq!(
                crate::config::load(directory.path())
                    .unwrap()
                    .terminal
                    .new_pane_directory,
                NewPaneDirectory::ProjectRoot,
                "{width}"
            );
        }
    }

    #[test]
    fn display_mode_selectors_share_pointer_direction_and_full_dialogs() {
        use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
        use ratatui::layout::Rect;

        for (tab, field, expected_left, expected_right, expected_options) in [
            (
                SettingsTab::Appearance,
                SettingsChoice::LeftPanelSizingMode,
                "FocusDependent",
                "Fixed",
                crate::config::LeftPanelSizingMode::ALL.len(),
            ),
            (
                SettingsTab::AgentMonitoring,
                SettingsChoice::AgentMonitoringMode,
                "Attention",
                "Normal",
                crate::agent_monitoring::AgentMonitoringMode::ALL.len(),
            ),
        ] {
            for width in [140, 80] {
                let directory = tempfile::tempdir().unwrap();
                let mut app = App::new(
                    format!("display-mode-pointer-{width}"),
                    directory.path().into(),
                );
                app.config_dir = Some(directory.path().into());
                if field == SettingsChoice::LeftPanelSizingMode {
                    app.ui_settings.left_panel_sizing.mode =
                        crate::config::LeftPanelSizingMode::Fixed;
                }
                app.set_screen_area(Rect::new(0, 0, width, 80));
                app.mode = Mode::Settings(SettingsState {
                    tab,
                    selected_row: 0,
                    ..SettingsState::default()
                });
                for (button, expected) in [
                    (MouseButton::Left, expected_left),
                    (MouseButton::Right, expected_right),
                ] {
                    let Mode::Settings(state) = &app.mode else {
                        panic!("settings mode");
                    };
                    let layout = crate::settings_ui::compute_layout_for_mode(
                        app.layout.screen_area,
                        &app,
                        state,
                    );
                    let (_, control) = crate::settings_ui::settings_choice_control(
                        layout.content_area,
                        &app,
                        state,
                        0,
                    )
                    .expect("display mode uses shared selector");
                    let value = control.geometry().value;
                    crate::mouse::handle_mouse_event(
                        &mut app,
                        MouseEvent {
                            kind: MouseEventKind::Down(button),
                            column: value.x,
                            row: value.y,
                            modifiers: KeyModifiers::NONE,
                        },
                    );
                    let (options, selected) = field.options(&app);
                    assert_eq!(selected, expected, "{tab:?} {width}");
                    assert_eq!(options.len(), expected_options, "{tab:?} {width}");
                }

                let Mode::Settings(state) = &app.mode else {
                    panic!("settings mode");
                };
                let layout = crate::settings_ui::compute_layout_for_mode(
                    app.layout.screen_area,
                    &app,
                    state,
                );
                let (_, control) = crate::settings_ui::settings_choice_control(
                    layout.content_area,
                    &app,
                    state,
                    0,
                )
                .expect("display mode uses shared selector");
                let open = control.geometry().open;
                crate::mouse::handle_mouse_event(
                    &mut app,
                    MouseEvent {
                        kind: MouseEventKind::Down(MouseButton::Left),
                        column: open.x,
                        row: open.y,
                        modifiers: KeyModifiers::NONE,
                    },
                );
                let Mode::ValueDialog(host) = &app.mode else {
                    panic!("+ opens the full catalog");
                };
                let ValueDialogState::Choice(dialog) = &host.dialog else {
                    panic!("choice dialog");
                };
                assert_eq!(dialog.options().len(), expected_options, "{tab:?} {width}");
            }
        }
    }

    #[test]
    fn discovered_model_catalogs_are_complete_deduplicated_and_keep_authored_current() {
        let mut app = App::new("synthetic-full-model-catalogs".into(), std::env::temp_dir());
        for field in [
            SettingsChoice::OllamaModel,
            SettingsChoice::OpenAiModel,
            SettingsChoice::AnthropicModel,
        ] {
            let models: Vec<String> = (0..400)
                .map(|index| format!("synthetic/model-α-{index}"))
                .collect();
            if field == SettingsChoice::OllamaModel {
                app.ollama_models = models;
                app.ollama_models.push("synthetic/model-α-399".into());
                app.inference_settings.ollama.model = "authored/not-discovered".into();
            } else if field == SettingsChoice::OpenAiModel {
                app.openai_models = models;
                app.openai_models.push("synthetic/model-α-399".into());
                app.inference_settings.openai.model = "authored/not-discovered".into();
            } else {
                app.anthropic_models = models;
                app.anthropic_models.push("synthetic/model-α-399".into());
                app.inference_settings.anthropic.model = "authored/not-discovered".into();
            }
            let (options, selected) = field.options(&app);
            assert_eq!(options.len(), 401);
            assert_eq!(selected, "authored/not-discovered");
            assert!(options
                .iter()
                .any(|option| option.id == "synthetic/model-α-399"));
            let ValueDialogState::Choice(dialog) = field.dialog(&app).unwrap() else {
                panic!("model catalog");
            };
            assert_eq!(dialog.options().len(), 401);
            assert_eq!(
                dialog.selected_id.as_deref(),
                Some("authored/not-discovered")
            );
        }
    }

    #[test]
    fn empty_model_catalog_explains_manual_entry_without_saving_placeholder() {
        let directory = tempfile::tempdir().unwrap();
        let mut app = App::new("synthetic-empty-models".into(), directory.path().into());
        app.config_dir = Some(directory.path().into());
        for field in [SettingsChoice::OllamaModel, SettingsChoice::OpenAiModel] {
            let (options, selected) = field.options(&app);
            assert_eq!(options.len(), 1);
            assert_eq!(selected, "unavailable");
            assert!(options[0]
                .disabled_reason
                .as_deref()
                .unwrap()
                .contains("Press E"));
            let attempts = app.configuration_admission.attempts;
            assert!(app
                .save_settings_choice(field, &selected, directory.path().into(), None)
                .is_err());
            assert_eq!(app.configuration_admission.attempts, attempts);
        }
    }

    #[test]
    fn progress_style_catalog_retains_authored_frames_until_explicit_preset_and_persists() {
        let directory = tempfile::tempdir().unwrap();
        let mut app = App::new(
            "synthetic-progress-style-catalog".into(),
            directory.path().into(),
        );
        app.config_dir = Some(directory.path().into());
        let custom = vec!["α".into(), "β".into(), "γ".into()];
        app.ui_settings.icons.task_progress_frames = custom.clone();
        let (options, selected) = SettingsChoice::ProgressFillStyle.options(&app);
        assert_eq!(
            options.len(),
            crate::icon_settings::TASK_PROGRESS_STYLE_NAMES.len() + 1
        );
        assert_eq!(selected, "custom");
        assert!(options
            .iter()
            .find(|option| option.id == "custom")
            .unwrap()
            .disabled_reason
            .is_some());
        let _dialog = SettingsChoice::ProgressFillStyle.dialog(&app).unwrap();
        assert_eq!(app.ui_settings.icons.task_progress_frames, custom);
        let attempts = app.configuration_admission.attempts;
        assert!(app
            .save_settings_choice(
                SettingsChoice::ProgressFillStyle,
                "custom",
                directory.path().into(),
                None
            )
            .is_err());
        assert_eq!(app.configuration_admission.attempts, attempts);
        for (index, name) in crate::icon_settings::TASK_PROGRESS_STYLE_NAMES
            .iter()
            .enumerate()
        {
            app.save_settings_choice(
                SettingsChoice::ProgressFillStyle,
                name,
                directory.path().into(),
                None,
            )
            .unwrap();
            app.settle_filesystem_for_test();
            let expected = crate::icon_settings::task_progress_preset_frames(index);
            assert_eq!(app.ui_settings.icons.task_progress_frames, expected);
            assert_eq!(
                crate::config::load(directory.path())
                    .unwrap()
                    .ui
                    .icons
                    .task_progress_frames,
                expected
            );
        }
    }

    #[test]
    fn every_settings_choice_retains_its_full_catalog_and_current_identity() {
        let mut app = App::new("choice-catalog".into(), std::env::temp_dir());
        let config_directory = std::env::temp_dir().join("ilium-choice-catalog-test");
        app.config_dir = Some(config_directory);
        app.inference_settings.ollama.model = "synthetic-authored-ollama".into();
        app.inference_settings.openai.model = "synthetic-authored-openai".into();
        let expected = [
            3,
            2,
            2,
            6,
            4,
            2,
            3,
            3,
            6,
            crate::config::LeftPanelSizingMode::ALL.len(),
            crate::agent_monitoring::AgentMonitoringMode::ALL.len(),
            2,
            10,
            3,
            2,
            4,
            1,
            1,
            5,
            1,
            2,
            2,
            ilium_inference::InferenceProviderKind::ALL.len(),
            crate::value_inference::OnboardingInference::KiloModel
                .options(&app)
                .len(),
            1,
            1,
            1,
            crate::icon_settings::TASK_PROGRESS_STYLE_NAMES.len(),
            crate::config::GitDefaultWhere::ALL.len(),
            crate::config::GitDefaultBase::ALL.len(),
            crate::config::GitBranchLine::ALL.len(),
            crate::config::GitClosePolicy::ALL.len(),
            crate::config::SessionRecoveryPolicy::ALL.len(),
            crate::config::SmartCopyLightKey::ALL.len(),
            ilium_remote_compaction::Technique::ALL.len(),
            ilium_remote_compaction::Technique::ALL.len(),
            ilium_remote_compaction::Technique::ALL.len(),
        ];
        assert_eq!(expected.len(), SettingsChoice::ALL.len());
        for (field, expected) in SettingsChoice::ALL.into_iter().zip(expected) {
            app.inference_settings.selected_provider = match field {
                SettingsChoice::KiloModel => ilium_inference::InferenceProviderKind::KiloGateway,
                SettingsChoice::OllamaModel => ilium_inference::InferenceProviderKind::Ollama,
                SettingsChoice::OpenAiModel => ilium_inference::InferenceProviderKind::OpenAi,
                SettingsChoice::AnthropicModel => ilium_inference::InferenceProviderKind::Anthropic,
                _ => app.inference_settings.selected_provider,
            };
            let (options, selected) = field.options(&app);
            assert_eq!(options.len(), expected, "{field:?}");
            assert!(options
                .iter()
                .all(|option| { option.disabled_reason.is_none() || option.id == selected }));
            assert_eq!(
                options
                    .iter()
                    .filter(|option| option.id == selected)
                    .count(),
                1
            );
            let ids: std::collections::HashSet<_> =
                options.iter().map(|option| &option.id).collect();
            assert_eq!(ids.len(), expected);
            let ValueDialogState::Choice(dialog) = field.dialog(&app).unwrap() else {
                panic!("choice");
            };
            assert_eq!(dialog.options().len(), expected);
            assert_eq!(dialog.selected_id, Some(selected));

            let location = crate::app::SettingsTab::ALL
                .into_iter()
                .find_map(|tab| {
                    (0..crate::settings_ui::settings_number_row_count(&app, tab))
                        .find(|row| SettingsChoice::at(&app, tab, *row) == Some(field))
                        .map(|row| (tab, row))
                })
                .unwrap_or_else(|| panic!("{field:?} must have a settings row"));
            app.mode = Mode::Settings(SettingsState {
                tab: location.0,
                selected_row: location.1,
                ..SettingsState::default()
            });
            app.begin_settings_choice_dialog(field);
            let Mode::ValueDialog(host) = std::mem::replace(&mut app.mode, Mode::Normal) else {
                panic!("{field:?} plus control must open a dialog");
            };
            assert!(
                matches!(
                    &host.target,
                    crate::value_dialog_host::ValueTarget::SettingsChoice {
                        field: actual,
                        ..
                    } if *actual == field
                ),
                "{field:?} dialog must retain its target identity"
            );
            let ValueDialogState::Choice(opened) = &host.dialog else {
                panic!("{field:?} must open its complete choice catalog");
            };
            assert_eq!(opened.options().len(), expected, "{field:?} dialog catalog");
            app.finish_value_dialog(host, DialogOutcome::Cancel);
        }
    }

    #[test]
    fn title_style_choice_uses_full_catalog_and_persists_without_changing_provider() {
        let directory = tempfile::tempdir().unwrap();
        let mut app = App::new("title-style-choice".into(), directory.path().into());
        app.config_dir = Some(directory.path().into());
        let provider = app.inference_settings.selected_provider;
        let (options, selected) = SettingsChoice::TitleStyle.options(&app);
        assert_eq!(options.len(), 2);
        assert_eq!(selected, "Labeling");

        app.save_settings_choice(
            SettingsChoice::TitleStyle,
            "Summarization",
            directory.path().into(),
            None,
        )
        .unwrap();
        app.settle_filesystem_for_test();

        let loaded = crate::config::load(directory.path()).unwrap();
        assert_eq!(
            loaded.inference.title_style,
            ilium_inference::TitleStyle::Summarization
        );
        assert_eq!(loaded.inference.selected_provider, provider);
    }

    #[test]
    fn display_modes_use_full_catalogs_and_persist_through_ui_settings() {
        let directory = tempfile::tempdir().unwrap();
        let mut app = App::new("display-mode-choices".into(), directory.path().into());
        app.config_dir = Some(directory.path().into());

        assert_eq!(
            SettingsChoice::at(&app, SettingsTab::Appearance, 0),
            Some(SettingsChoice::LeftPanelSizingMode)
        );
        assert_eq!(
            SettingsChoice::at(&app, SettingsTab::AgentMonitoring, 0),
            Some(SettingsChoice::AgentMonitoringMode)
        );
        for (field, expected_labels) in [
            (
                SettingsChoice::LeftPanelSizingMode,
                vec!["Fixed", "Focus-dependent", "Width-dependent"],
            ),
            (
                SettingsChoice::AgentMonitoringMode,
                vec!["Normal", "Attention"],
            ),
        ] {
            let (options, selected) = field.options(&app);
            assert_eq!(
                options
                    .iter()
                    .map(|option| option.label.as_str())
                    .collect::<Vec<_>>(),
                expected_labels
            );
            assert!(options.iter().any(|option| option.id == selected));
            let ValueDialogState::Choice(dialog) = field.dialog(&app).unwrap() else {
                panic!("full selector dialog");
            };
            assert_eq!(dialog.options().len(), expected_labels.len());
        }

        let panel_mode_id = SettingsChoice::LeftPanelSizingMode.options(&app).0[1]
            .id
            .clone();
        app.save_settings_choice(
            SettingsChoice::LeftPanelSizingMode,
            &panel_mode_id,
            directory.path().into(),
            None,
        )
        .unwrap();
        app.settle_filesystem_for_test();
        assert_eq!(
            app.ui_settings.left_panel_sizing.mode,
            crate::config::LeftPanelSizingMode::FocusDependent
        );

        let attention_id = SettingsChoice::AgentMonitoringMode.options(&app).0[1]
            .id
            .clone();
        app.save_settings_choice(
            SettingsChoice::AgentMonitoringMode,
            &attention_id,
            directory.path().into(),
            None,
        )
        .unwrap();
        app.settle_filesystem_for_test();
        let saved = crate::config::load(directory.path()).unwrap().ui;
        assert_eq!(
            saved.left_panel_sizing.mode,
            crate::config::LeftPanelSizingMode::FocusDependent
        );
        assert_eq!(
            saved.agent_monitoring_mode,
            crate::agent_monitoring::AgentMonitoringMode::Attention
        );
    }

    #[test]
    fn reset_time_catalog_persists_display_without_changing_monitors() {
        let directory = tempfile::tempdir().unwrap();
        let mut app = App::new("reset-choice".into(), directory.path().into());
        app.config_dir = Some(directory.path().into());
        app.reset_planning_settings.monitor_claude = false;
        app.reset_planning_settings.monitor_codex = true;
        let checked = std::time::SystemTime::now();
        app.reset_monitor_state.codex.last_checked = Some(checked);
        app.save_settings_choice(
            SettingsChoice::ResetTimeDisplay,
            "Human",
            directory.path().into(),
            None,
        )
        .unwrap();
        app.settle_filesystem_for_test();
        let saved = crate::config::load(directory.path())
            .unwrap()
            .reset_planning;
        assert_eq!(
            saved.time_style,
            crate::reset_planning::ResetTimeStyle::Human
        );
        assert!(!saved.monitor_claude);
        assert!(saved.monitor_codex);
        assert_eq!(app.reset_monitor_state.codex.last_checked, Some(checked));
        app.step_settings_choice(SettingsChoice::ResetTimeDisplay, -1);
        assert_eq!(
            app.reset_planning_settings.time_style,
            crate::reset_planning::ResetTimeStyle::Exact
        );
        app.settle_filesystem_for_test();
    }

    #[test]
    fn managed_tree_sort_is_visible_but_cannot_bypass_its_owner() {
        let mut app = App::new("choice-catalog".into(), std::env::temp_dir());
        app.ui_settings.tree_order = TreeOrder::CostDescending;
        let (options, selected) = SettingsChoice::TreeOrder.options(&app);
        assert_eq!(options.len(), TreeOrder::ALL.len() + 1);
        assert_eq!(selected, "CostDescending");
        assert!(options.last().unwrap().disabled_reason.is_some());
        let attempts = app.configuration_admission.attempts;
        assert!(app
            .save_settings_choice(
                SettingsChoice::TreeOrder,
                &selected,
                std::env::temp_dir(),
                None
            )
            .is_err());
        assert!(app
            .save_settings_choice(
                SettingsChoice::TreeOrder,
                "invented",
                std::env::temp_dir(),
                None
            )
            .is_err());
        assert_eq!(app.ui_settings.tree_order, TreeOrder::CostDescending);
        assert_eq!(app.configuration_admission.attempts, attempts);
    }

    #[test]
    fn settings_choice_failure_keeps_dialog_and_retry_preserves_other_fields() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        std::fs::write(&path, "[terminal\n").unwrap();
        let mut app = App::new("choice-retry".into(), directory.path().into());
        app.config_dir = Some(directory.path().into());
        app.terminal_settings.scrollback_budget_mib = 17;
        app.mode = Mode::Settings(SettingsState {
            tab: SettingsTab::Terminal,
            selected_row: 1,
            ..SettingsState::default()
        });
        app.begin_settings_choice_dialog(SettingsChoice::TerminalDirectory);
        let Mode::ValueDialog(host) = std::mem::replace(&mut app.mode, Mode::Normal) else {
            panic!("choice modal");
        };
        app.finish_value_dialog(host, DialogOutcome::Choose("FocusedTerminal".into()));
        app.settle_filesystem_for_test();
        let Mode::ValueDialog(host) = &app.mode else {
            panic!("failed save retains dialog");
        };
        assert!(!host.is_saving());
        let ValueDialogState::Choice(choice) = &host.dialog else {
            panic!("choice");
        };
        assert!(choice.notice.is_some());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "[terminal\n");
        std::fs::write(&path, "[terminal]\n").unwrap();
        let Mode::ValueDialog(host) = std::mem::replace(&mut app.mode, Mode::Normal) else {
            panic!("retry");
        };
        app.finish_value_dialog(host, DialogOutcome::Choose("FocusedTerminal".into()));
        app.settle_filesystem_for_test();
        assert!(matches!(app.mode, Mode::Settings(_)));
        let config = crate::config::load(directory.path()).unwrap();
        assert_eq!(
            config.terminal.new_pane_directory,
            NewPaneDirectory::FocusedTerminal
        );
        assert_eq!(config.terminal.scrollback_budget_mib, 17);
    }
    #[test]
    fn git_session_and_modifier_catalogs_keep_authored_fields_and_persist_every_option() {
        let directory = tempfile::tempdir().unwrap();
        let mut app = App::new(
            "synthetic-remaining-settings".into(),
            directory.path().into(),
        );
        app.config_dir = Some(directory.path().into());
        app.git_settings.branch_prefix = "authored/".into();
        app.git_settings.worktree_location_template = "{repo_parent}/λ/{branch_slug}".into();
        app.git_settings.setup_command = "printf authored".into();
        let authored = app.git_settings.clone();
        app.terminal_settings.scrollback_budget_mib = 37;
        app.terminal_settings.smart_copy_light = true;
        crate::config::save_terminal_settings(directory.path(), &app.terminal_settings).unwrap();
        for field in [
            SettingsChoice::GitDefaultWhere,
            SettingsChoice::GitDefaultBase,
            SettingsChoice::GitBranchLine,
            SettingsChoice::GitClosePolicy,
            SettingsChoice::SessionRecovery,
            SettingsChoice::SmartCopyModifier,
        ] {
            let (options, selected) = field.options(&app);
            assert!(options.len() >= 2);
            assert!(options.iter().any(|option| option.id == selected));
            assert!(options
                .iter()
                .all(|option| option.disabled_reason.is_none()));
            for option in options {
                app.save_settings_choice(field, &option.id, directory.path().into(), None)
                    .unwrap();
                app.settle_filesystem_for_test();
                let disk = crate::config::load(directory.path()).unwrap();
                assert_eq!(disk.git.branch_prefix, authored.branch_prefix);
                assert_eq!(
                    disk.git.worktree_location_template,
                    authored.worktree_location_template
                );
                assert_eq!(disk.git.setup_command, authored.setup_command);
                assert_eq!(disk.terminal.scrollback_budget_mib, 37);
                assert!(disk.terminal.smart_copy_light);
                let expected = match field {
                    SettingsChoice::GitDefaultWhere => format!("{:?}", disk.git.default_where),
                    SettingsChoice::GitDefaultBase => format!("{:?}", disk.git.default_base),
                    SettingsChoice::GitBranchLine => format!("{:?}", disk.git.branch_line),
                    SettingsChoice::GitClosePolicy => {
                        format!("{:?}", disk.git.default_close_policy)
                    }
                    SettingsChoice::SessionRecovery => {
                        format!("{:?}", disk.session.recovery_policy)
                    }
                    SettingsChoice::SmartCopyModifier => {
                        format!("{:?}", disk.terminal.smart_copy_light_key)
                    }
                    _ => unreachable!(),
                };
                assert_eq!(expected, option.id);
            }
        }
    }

    #[test]
    fn session_catalog_durable_merge_keeps_concurrent_backup_setting_and_closes_own_child() {
        let directory = tempfile::tempdir().unwrap();
        let mut app = App::new("synthetic-session-merge".into(), directory.path().into());
        app.config_dir = Some(directory.path().into());
        let previous = app.session_settings;
        let mut concurrent = previous;
        concurrent.backups_enabled = !previous.backups_enabled;
        crate::config::save_session_settings(directory.path(), &previous, &concurrent).unwrap();
        app.mode = Mode::Settings(SettingsState {
            tab: SettingsTab::Session,
            selected_row: 0,
            ..SettingsState::default()
        });
        app.begin_settings_choice_dialog(SettingsChoice::SessionRecovery);
        let Mode::ValueDialog(host) = std::mem::replace(&mut app.mode, Mode::Normal) else {
            panic!("choice child");
        };
        app.finish_value_dialog(host, DialogOutcome::Choose("StartFresh".into()));
        assert!(matches!(&app.mode, Mode::ValueDialog(host) if host.is_saving()));
        assert_eq!(
            app.session_settings.backups_enabled,
            previous.backups_enabled
        );
        app.settle_filesystem_for_test();
        assert!(matches!(app.mode, Mode::Settings(_)));
        assert_eq!(
            app.session_settings.recovery_policy,
            crate::config::SessionRecoveryPolicy::StartFresh
        );
        assert_eq!(
            app.session_settings.backups_enabled,
            concurrent.backups_enabled
        );
        assert_eq!(
            crate::config::load(directory.path()).unwrap().session,
            app.session_settings
        );
        assert!(app
            .status_message
            .as_deref()
            .unwrap()
            .contains("server next starts"));
    }

    #[test]
    fn session_catalog_failed_write_retains_selection_then_retries_without_silent_success() {
        let directory = tempfile::tempdir().unwrap();
        let config_path = directory.path().join("config.toml");
        std::fs::create_dir(&config_path).unwrap();
        let mut app = App::new("synthetic-session-failure".into(), directory.path().into());
        app.config_dir = Some(directory.path().into());
        app.mode = Mode::Settings(SettingsState {
            tab: SettingsTab::Session,
            selected_row: 0,
            ..SettingsState::default()
        });
        app.begin_settings_choice_dialog(SettingsChoice::SessionRecovery);
        let Mode::ValueDialog(host) = std::mem::replace(&mut app.mode, Mode::Normal) else {
            panic!("choice child");
        };
        app.finish_value_dialog(host, DialogOutcome::Choose("AskBeforeRestore".into()));
        app.settle_filesystem_for_test();
        let Mode::ValueDialog(host) = &app.mode else {
            panic!("failed child retained");
        };
        assert!(!host.is_saving());
        let ValueDialogState::Choice(dialog) = &host.dialog else {
            panic!("choice");
        };
        assert!(dialog.notice.is_some());
        assert_eq!(dialog.selected_id.as_deref(), Some("RestoreAutomatically"));
        std::fs::remove_dir(&config_path).unwrap();
        let Mode::ValueDialog(host) = std::mem::replace(&mut app.mode, Mode::Normal) else {
            panic!("choice child");
        };
        app.finish_value_dialog(host, DialogOutcome::Choose("AskBeforeRestore".into()));
        app.settle_filesystem_for_test();
        assert!(matches!(app.mode, Mode::Settings(_)));
        assert_eq!(
            crate::config::load(directory.path())
                .unwrap()
                .session
                .recovery_policy,
            crate::config::SessionRecoveryPolicy::AskBeforeRestore
        );
    }

    #[test]
    fn git_validation_and_closed_writer_do_not_apply_unsaved_choice() {
        let directory = tempfile::tempdir().unwrap();
        let mut app = App::new(
            "synthetic-unadmitted-choices".into(),
            directory.path().into(),
        );
        app.config_dir = Some(directory.path().into());
        app.git_settings.branch_prefix = "invalid\0".into();
        let original = app.git_settings.clone();
        let attempts = app.configuration_admission.attempts;
        assert!(app
            .save_settings_choice(
                SettingsChoice::GitDefaultWhere,
                "NewWorktree",
                directory.path().into(),
                None
            )
            .is_err());
        assert_eq!(app.git_settings, original);
        assert_eq!(app.configuration_admission.attempts, attempts);
        app.configuration_files = None;
        let original = app.session_settings;
        assert!(app
            .save_settings_choice(
                SettingsChoice::SessionRecovery,
                "StartFresh",
                directory.path().into(),
                None
            )
            .is_err());
        assert_eq!(app.session_settings, original);
        let original = app.terminal_settings;
        assert!(app
            .save_settings_choice(
                SettingsChoice::SmartCopyModifier,
                "Alt",
                directory.path().into(),
                None
            )
            .is_err());
        assert_eq!(app.terminal_settings, original);
    }
    #[test]
    fn lost_session_receipt_keeps_matching_child_and_obsolete_token_cannot_finish_reopen() {
        use crate::filesystem::ordered::{WriteCompletion, WriteId};
        let directory = tempfile::tempdir().unwrap();
        let mut app = App::new(
            "synthetic-lost-session-receipt".into(),
            directory.path().into(),
        );
        app.config_dir = Some(directory.path().into());
        app.mode = Mode::Settings(SettingsState {
            tab: SettingsTab::Session,
            selected_row: 0,
            ..SettingsState::default()
        });
        app.begin_settings_choice_dialog(SettingsChoice::SessionRecovery);
        let old = std::sync::Arc::new(());
        if let Mode::ValueDialog(host) = &mut app.mode {
            host.begin_save(old.clone());
        }
        app.collect_session_choice_configuration(
            app.session_settings,
            &old,
            WriteCompletion::Lost { id: WriteId(73) },
        );
        let Mode::ValueDialog(host) = &app.mode else {
            panic!("lost child retained");
        };
        assert!(!host.is_saving());
        let ValueDialogState::Choice(dialog) = &host.dialog else {
            panic!("choice");
        };
        assert!(dialog.notice.as_deref().unwrap().contains("unconfirmed"));
        app.pop_modal();
        app.begin_settings_choice_dialog(SettingsChoice::SessionRecovery);
        let new = std::sync::Arc::new(());
        if let Mode::ValueDialog(host) = &mut app.mode {
            host.begin_save(new.clone());
        }
        app.collect_session_choice_configuration(
            app.session_settings,
            &old,
            WriteCompletion::Lost { id: WriteId(73) },
        );
        assert!(matches!(&app.mode, Mode::ValueDialog(host) if host.is_saving()));
        app.collect_session_choice_configuration(
            app.session_settings,
            &new,
            WriteCompletion::Lost { id: WriteId(74) },
        );
        assert!(matches!(&app.mode, Mode::ValueDialog(host) if !host.is_saving()));
    }
}

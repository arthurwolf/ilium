//! Numeric settings binding. Rendering never owns persistence or runtime effects.
use crate::app::{
    AgentMonitoringRow, App, AppearanceRow, EditorRow, KanbanBoardRow, Mode, SettingsTab,
    TerminalRow,
};
use crate::value_config::{self, BoardNumber, PanelNumber, ScalarNumber};
use crate::value_number::{NumberSpec, NumberValue};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsNumber {
    Remote(crate::remote_compaction_settings::RemoteCompactionRow),
    Panel(PanelNumber),
    Ui(UiNumber),
    TerminalScrollback,
    TerminalEngineMemory,
    EditorAutosaveDelay,
    Board(BoardNumber),
    ApiPort,
    VoiceVolume,
    NotificationCoalesce,
    InferenceTokenBudget,
    WorkingPollSeconds,
    IdlePollSeconds,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiNumber {
    AutoFreezeAfter,
    LastPromptLines,
    ProgressLines,
    CompletedProgressHideAfter,
}
impl UiNumber {
    fn scalar(self) -> ScalarNumber {
        match self {
            Self::AutoFreezeAfter => ScalarNumber::AutoFreezeAfter,
            Self::LastPromptLines => ScalarNumber::LastPromptLines,
            Self::ProgressLines => ScalarNumber::ProgressLines,
            Self::CompletedProgressHideAfter => ScalarNumber::CompletedProgressHideAfter,
        }
    }
}

impl SettingsNumber {
    pub fn at(app: &App, tab: SettingsTab, row: usize) -> Option<Self> {
        match tab {
            SettingsTab::RemoteCompaction => crate::remote_compaction_settings_ui::rows(app)
                .get(row)
                .copied()
                .filter(|row| row.is_debounced())
                .map(Self::Remote),
            SettingsTab::Inference
                if crate::settings_ui::inference_rows(&app.inference_settings).get(row)
                    == Some(&crate::app::InferenceRow::Field(
                        crate::app::InferenceSettingField::RestructurePromptTokenLimit,
                    )) =>
            {
                Some(Self::InferenceTokenBudget)
            }
            SettingsTab::VoiceControl
                if crate::voice_settings::VoiceRow::ALL.get(row)
                    == Some(&crate::voice_settings::VoiceRow::OutputVolume) =>
            {
                Some(Self::VoiceVolume)
            }
            SettingsTab::Sound
                if crate::app::SoundRow::ALL.get(row)
                    == Some(&crate::app::SoundRow::NotifyCoalesce) =>
            {
                Some(Self::NotificationCoalesce)
            }
            SettingsTab::Appearance => {
                match AppearanceRow::visible(app.ui_settings.left_panel_sizing.mode).get(row) {
                    Some(AppearanceRow::FixedPanelWidth) => {
                        Some(Self::Panel(PanelNumber::FixedWidth))
                    }
                    Some(AppearanceRow::UnfocusedPanelWidth) => {
                        Some(Self::Panel(PanelNumber::UnfocusedWidth))
                    }
                    Some(AppearanceRow::FocusedPanelWidth) => {
                        Some(Self::Panel(PanelNumber::FocusedWidth))
                    }
                    Some(AppearanceRow::MinimumTerminalWidth) => {
                        Some(Self::Panel(PanelNumber::MinimumTerminalWidth))
                    }
                    Some(AppearanceRow::LastPromptMaxLines) => {
                        Some(Self::Ui(UiNumber::LastPromptLines))
                    }
                    Some(AppearanceRow::ProgressMonitorMaxLines) => {
                        Some(Self::Ui(UiNumber::ProgressLines))
                    }
                    Some(AppearanceRow::AutoFreezeAfter) => {
                        Some(Self::Ui(UiNumber::AutoFreezeAfter))
                    }
                    _ => None,
                }
            }
            SettingsTab::AgentMonitoring => {
                match crate::settings_ui::agent_monitoring_rows(app).get(row) {
                    Some(AgentMonitoringRow::WorkingPollSeconds)
                        if app.agent_detection_settings.is_some() =>
                    {
                        Some(Self::WorkingPollSeconds)
                    }
                    Some(AgentMonitoringRow::IdlePollSeconds)
                        if app.agent_detection_settings.is_some() =>
                    {
                        Some(Self::IdlePollSeconds)
                    }
                    Some(AgentMonitoringRow::ProgressMonitorMaxLines) => {
                        Some(Self::Ui(UiNumber::ProgressLines))
                    }
                    Some(AgentMonitoringRow::CompletedProgressHideAfter) => {
                        Some(Self::Ui(UiNumber::CompletedProgressHideAfter))
                    }
                    _ => None,
                }
            }
            SettingsTab::Terminal
                if TerminalRow::ALL.get(row) == Some(&TerminalRow::ScrollbackBudget) =>
            {
                Some(Self::TerminalScrollback)
            }
            SettingsTab::Terminal
                if TerminalRow::ALL.get(row) == Some(&TerminalRow::EngineMemoryBudget) =>
            {
                Some(Self::TerminalEngineMemory)
            }
            SettingsTab::Editor if EditorRow::ALL.get(row) == Some(&EditorRow::AutosaveDelay) => {
                Some(Self::EditorAutosaveDelay)
            }
            SettingsTab::KanbanBoard => match KanbanBoardRow::ALL.get(row) {
                Some(KanbanBoardRow::CardPreviewLines) => {
                    Some(Self::Board(BoardNumber::CardPreviewLines))
                }
                Some(KanbanBoardRow::MinimumColumnWidth) => {
                    Some(Self::Board(BoardNumber::MinimumColumnWidth))
                }
                None => None,
            },
            SettingsTab::Api if row == 0 => Some(Self::ApiPort),
            _ => None,
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Remote(row) => row.label(),
            Self::WorkingPollSeconds => "Working poll interval (s; 0 = 500 ms minimum)",
            Self::IdlePollSeconds => "Idle poll interval (s; 0 = 500 ms minimum)",
            Self::InferenceTokenBudget => "Restructure prompt tokens",
            Self::VoiceVolume => "Output volume (%)",
            Self::NotificationCoalesce => "Task notification coalescing (s)",
            Self::Panel(PanelNumber::FixedWidth) => "Fixed panel width",
            Self::Panel(PanelNumber::UnfocusedWidth) => "Unfocused panel width",
            Self::Panel(PanelNumber::FocusedWidth) => "Focused panel width",
            Self::Panel(PanelNumber::MinimumTerminalWidth) => "Minimum terminal width",
            Self::Ui(UiNumber::LastPromptLines) => "Last prompt lines",
            Self::Ui(UiNumber::AutoFreezeAfter) => "Auto-freeze delay (s)",
            Self::Ui(UiNumber::ProgressLines) => "Progress footer lines",
            Self::Ui(UiNumber::CompletedProgressHideAfter) => {
                "Hide completed progress after (s; 0 = never)"
            }
            Self::TerminalScrollback => "Scrollback budget (MiB)",
            Self::TerminalEngineMemory => "Parser pool budget (MiB; 0 = Off)",
            Self::EditorAutosaveDelay => "Autosave delay (ms)",
            Self::Board(BoardNumber::CardPreviewLines) => "Card preview lines",
            Self::Board(BoardNumber::MinimumColumnWidth) => "Minimum column width",
            Self::ApiPort => "HTTP API port",
        }
    }

    pub fn snapshot(self, app: &App) -> (NumberSpec, String) {
        match self {
            Self::Remote(row) => {
                use crate::remote_compaction_settings::*;
                let settings = &app.remote_compaction_settings;
                let (minimum, maximum, value) = match row {
                    RemoteCompactionRow::Threshold => (
                        THRESHOLD_PERCENT_RANGE.0 as i128,
                        THRESHOLD_PERCENT_RANGE.1 as i128,
                        settings.threshold_percent as i128,
                    ),
                    RemoteCompactionRow::PauseTimeout => (
                        PAUSE_TIMEOUT_RANGE.0 as i128,
                        PAUSE_TIMEOUT_RANGE.1 as i128,
                        settings.pause_timeout_seconds as i128,
                    ),
                    RemoteCompactionRow::Cooldown => (
                        COOLDOWN_MINUTES_RANGE.0 as i128,
                        COOLDOWN_MINUTES_RANGE.1 as i128,
                        settings.cooldown_minutes as i128,
                    ),
                    RemoteCompactionRow::TailTokens => (
                        TAIL_TOKENS_RANGE.0 as i128,
                        TAIL_TOKENS_RANGE.1 as i128,
                        settings.tail_tokens as i128,
                    ),
                    RemoteCompactionRow::ProtectedToolTokens => (
                        PROTECTED_TOOL_TOKENS_RANGE.0 as i128,
                        PROTECTED_TOOL_TOKENS_RANGE.1 as i128,
                        settings.protected_recent_tool_tokens as i128,
                    ),
                    RemoteCompactionRow::ToolResultChars => (
                        TOOL_RESULT_CHARS_RANGE.0 as i128,
                        TOOL_RESULT_CHARS_RANGE.1 as i128,
                        settings.tool_result_chars as i128,
                    ),
                    RemoteCompactionRow::SummarizerContextTokens => (
                        SUMMARIZER_CONTEXT_RANGE.0 as i128,
                        SUMMARIZER_CONTEXT_RANGE.1 as i128,
                        settings.summarizer_context_tokens as i128,
                    ),
                    RemoteCompactionRow::KeepBackups => (
                        KEEP_BACKUPS_RANGE.0 as i128,
                        KEEP_BACKUPS_RANGE.1 as i128,
                        settings.keep_backups as i128,
                    ),
                    _ => unreachable!("Only numeric remote rows are bound"),
                };
                (NumberSpec::Integer { minimum, maximum }, value.to_string())
            }

            Self::WorkingPollSeconds => (
                ScalarNumber::WorkingPollSeconds.spec(),
                app.agent_detection_settings
                    .as_ref()
                    .map_or(0, |settings| settings.working_poll_seconds)
                    .to_string(),
            ),
            Self::IdlePollSeconds => (
                ScalarNumber::IdlePollSeconds.spec(),
                app.agent_detection_settings
                    .as_ref()
                    .map_or(0, |settings| settings.idle_poll_seconds)
                    .to_string(),
            ),
            Self::InferenceTokenBudget => (
                ScalarNumber::InferenceTokenBudget.spec(),
                app.inference_settings
                    .restructure_prompt_token_limit
                    .to_string(),
            ),
            Self::VoiceVolume => (
                ScalarNumber::VoiceVolume.spec(),
                app.voice_settings.output_volume_percent.to_string(),
            ),
            Self::NotificationCoalesce => (
                ScalarNumber::NotificationCoalesce.spec(),
                app.notification_settings.task_coalesce_seconds.to_string(),
            ),
            Self::Panel(field) => (
                field.spec(&app.ui_settings.left_panel_sizing),
                field.value(&app.ui_settings.left_panel_sizing).to_string(),
            ),
            Self::Ui(field) => (
                field.scalar().spec(),
                match field {
                    UiNumber::AutoFreezeAfter => {
                        app.ui_settings.auto_freeze_after_seconds.to_string()
                    }
                    UiNumber::LastPromptLines => app.ui_settings.last_prompt_max_lines.to_string(),
                    UiNumber::ProgressLines => app.ui_settings.progress_max_lines.to_string(),
                    UiNumber::CompletedProgressHideAfter => app
                        .ui_settings
                        .completed_progress_hide_after_seconds
                        .to_string(),
                },
            ),
            Self::TerminalScrollback => (
                value_config::terminal_scrollback_spec(),
                app.terminal_settings.scrollback_budget_mib.to_string(),
            ),
            Self::TerminalEngineMemory => (
                value_config::terminal_engine_memory_spec(),
                app.terminal_settings.engine_memory_budget_mib.to_string(),
            ),
            Self::EditorAutosaveDelay => (
                value_config::editor_autosave_spec(),
                app.editor_settings.autosave_delay_ms.to_string(),
            ),
            Self::Board(field) => (
                field.spec(),
                field.value(&app.kanban_board_settings).to_string(),
            ),
            Self::ApiPort => (
                ScalarNumber::ApiPort.spec(),
                app.api_settings.port.to_string(),
            ),
        }
    }

    pub fn stepped(self, app: &App, direction: i32) -> Result<String, String> {
        let (spec, text) = self.snapshot(app);
        if self == Self::Ui(UiNumber::AutoFreezeAfter) {
            let current = app.ui_settings.auto_freeze_after_seconds;
            let next = if direction < 0 {
                current.saturating_sub(900).max(1)
            } else {
                current.saturating_add(900).min(i64::MAX as u64)
            };
            return Ok(next.to_string());
        }
        if let Self::Remote(row) = self {
            let mut updated = app.remote_compaction_settings.clone();
            updated.adjust(row, direction);
            use crate::remote_compaction_settings::RemoteCompactionRow;
            return Ok(match row {
                RemoteCompactionRow::Threshold => updated.threshold_percent.to_string(),
                RemoteCompactionRow::PauseTimeout => updated.pause_timeout_seconds.to_string(),
                RemoteCompactionRow::Cooldown => updated.cooldown_minutes.to_string(),
                RemoteCompactionRow::TailTokens => updated.tail_tokens.to_string(),
                RemoteCompactionRow::ProtectedToolTokens => {
                    updated.protected_recent_tool_tokens.to_string()
                }
                RemoteCompactionRow::ToolResultChars => updated.tool_result_chars.to_string(),
                RemoteCompactionRow::SummarizerContextTokens => {
                    updated.summarizer_context_tokens.to_string()
                }
                RemoteCompactionRow::KeepBackups => updated.keep_backups.to_string(),
                _ => return Err("Not a numeric remote setting".into()),
            });
        }
        if self == Self::EditorAutosaveDelay {
            spec.parse(&text)?;
            return Ok(app
                .editor_settings
                .stepped_autosave_delay_ms(direction)
                .to_string());
        }
        if matches!(&self, Self::TerminalEngineMemory) {
            let mut settings = crate::config::TerminalSettings::default();
            value_config::set_terminal_engine_memory(&mut settings, &text)?;
            return Ok(settings
                .stepped_engine_memory_budget_mib(direction)
                .to_string());
        }
        let step = match self {
            Self::VoiceVolume => 5,
            Self::NotificationCoalesce => {
                i128::from(ilium_sound::NotificationSettings::TASK_COALESCE_STEP_SECONDS)
            }
            Self::TerminalScrollback => 4,
            Self::Ui(UiNumber::CompletedProgressHideAfter) => 30,
            Self::Ui(UiNumber::AutoFreezeAfter) => 15 * 60,
            _ => 1,
        };
        match spec.stepped(spec.parse(&text)?, NumberValue::Integer(step), direction)? {
            NumberValue::Integer(value) => Ok(value.to_string()),
            NumberValue::Decimal(_) => Err("This setting requires a whole number".into()),
        }
    }
}

impl App {
    pub(crate) fn begin_settings_number_dialog(&mut self, field: SettingsNumber) {
        if field.is_detection() {
            self.begin_detection_number_dialog(field);
            return;
        }
        let result = self
            .config_dir
            .clone()
            .ok_or_else(|| "The configuration directory is unavailable".to_owned())
            .and_then(|directory| {
                crate::value_dialog_host::ValueDialogHost::settings_number(field, self, directory)
            });
        match result {
            Ok(host) => self.push_modal(Mode::ValueDialog(Box::new(host))),
            Err(error) => self.status_message = Some(error),
        }
    }

    pub(crate) fn commit_settings_number_dialog(
        &mut self,
        host: &mut crate::value_dialog_host::ValueDialogHost,
        outcome: &crate::value_dialog::DialogOutcome,
    ) -> Result<(), String> {
        use crate::value_dialog_host::ValueTarget;
        if matches!(host.target, ValueTarget::DetectionNumber { .. }) {
            return self.commit_detection_number_dialog(host, outcome);
        }
        let ValueTarget::SettingsNumber {
            field,
            directory,
            inference_revision,
            autosave,
        } = &host.target
        else {
            return Err("This is not a numeric settings dialog".into());
        };
        if inference_revision.is_some_and(|revision| revision != self.onboarding_revision) {
            return Err("Inference settings changed; reopen this dialog".into());
        }
        if host.is_saving() {
            return Err("The previous value is still being saved".into());
        }
        if self.config_dir.as_ref() != Some(directory) {
            return Err("The configuration destination changed; reopen this dialog".into());
        }
        let Some(Mode::Settings(parent)) = self.modal_stack.last() else {
            return Err("The settings parent changed; reopen this dialog".into());
        };
        if SettingsNumber::at(self, parent.tab, parent.selected_row) != Some(*field) {
            return Err("The selected setting changed; reopen this dialog".into());
        }
        let crate::value_dialog::DialogOutcome::CommitNumber(text) = outcome else {
            return Err("Enter a number for this setting".into());
        };
        let token = std::sync::Arc::new(());
        let operation = autosave.as_ref().and_then(|autosave| autosave.operation.as_ref())
            .filter(|_| text.trim().parse::<u32>() == Ok(self.inference_settings.restructure_prompt_token_limit))
            .filter(|operation| matches!(&self.inference_save_state,
                crate::filesystem::configurations::InferenceSaveState::Pending { operation: current, .. }
                if std::sync::Arc::ptr_eq(current, operation)))
            .cloned();
        if let Some(operation) = operation {
            self.configuration_files
                .as_mut()
                .ok_or("Configuration writer unavailable")?
                .attach_inference_value_dialog(&operation, token.clone())?;
        } else {
            self.save_settings_number(*field, text, directory.clone(), Some(token.clone()))?;
        }
        if let ValueTarget::SettingsNumber {
            autosave: Some(autosave),
            ..
        } = &mut host.target
        {
            autosave.deadline = None;
            if let crate::filesystem::configurations::InferenceSaveState::Pending {
                operation,
                ..
            } = &self.inference_save_state
            {
                autosave.operation = Some(operation.clone());
            }
        }
        if let crate::value_dialog_host::ValueTarget::SettingsNumber {
            inference_revision: Some(revision),
            ..
        } = &mut host.target
        {
            *revision = self.onboarding_revision;
        }
        host.begin_save(token);
        Ok(())
    }

    pub(crate) fn queue_inference_number_autosave(
        &mut self,
        host: &mut crate::value_dialog_host::ValueDialogHost,
        previous: Option<&str>,
        now: std::time::Instant,
    ) {
        let Some(draft) = host.inference_budget_draft() else {
            return;
        };
        if previous == Some(draft) || host.is_saving() {
            return;
        }
        let valid = ScalarNumber::InferenceTokenBudget.parse(draft).is_ok();
        let crate::value_dialog_host::ValueTarget::SettingsNumber {
            autosave: Some(autosave),
            ..
        } = &mut host.target
        else {
            return;
        };
        if let Some(operation) = autosave.operation.take() {
            if matches!(&self.inference_save_state,
                crate::filesystem::configurations::InferenceSaveState::Pending { operation: current, .. }
                if std::sync::Arc::ptr_eq(current, &operation))
            {
                self.inference_save_state =
                    crate::filesystem::configurations::InferenceSaveState::Unsaved;
                self.inference_settings_save_error = None;
            }
        }
        autosave.deadline = valid.then_some(now + std::time::Duration::from_millis(600));
    }

    pub(crate) fn finish_inference_number_autosave(
        &mut self,
        operation: &std::sync::Arc<()>,
        result: &Result<(), String>,
    ) {
        let Mode::ValueDialog(host) = &mut self.mode else {
            return;
        };
        let owns = matches!(&host.target, crate::value_dialog_host::ValueTarget::SettingsNumber { autosave: Some(autosave), .. }
            if autosave.operation.as_ref().is_some_and(|current| std::sync::Arc::ptr_eq(current, operation)));
        if owns {
            if let crate::value_dialog::ValueDialogState::Number(number) = &mut host.dialog {
                number.error = result.as_ref().err().cloned();
            }
        }
    }

    pub(crate) fn tick_inference_number_autosave(&mut self, now: std::time::Instant) -> bool {
        let due = matches!(&self.mode, Mode::ValueDialog(host)
            if matches!(&host.target, crate::value_dialog_host::ValueTarget::SettingsNumber { autosave: Some(autosave), .. }
                if autosave.deadline.is_some_and(|deadline| now >= deadline)) && !host.is_saving());
        if !due {
            return false;
        }
        let Mode::ValueDialog(mut host) = std::mem::replace(&mut self.mode, Mode::Normal) else {
            return false;
        };
        let result = (|| {
            let crate::value_dialog_host::ValueTarget::SettingsNumber {
                field,
                directory,
                inference_revision,
                autosave: Some(autosave),
            } = &mut host.target
            else {
                return Err("Number dialog changed".to_owned());
            };
            autosave.deadline = None;
            if *inference_revision != Some(self.onboarding_revision)
                || self.config_dir.as_ref() != Some(directory)
            {
                return Err("Inference settings changed; reopen this dialog".into());
            }
            let Some(Mode::Settings(parent)) = self.modal_stack.last() else {
                return Err("Settings parent changed".into());
            };
            if SettingsNumber::at(self, parent.tab, parent.selected_row) != Some(*field) {
                return Err("Selected setting changed".into());
            }
            let crate::value_dialog::ValueDialogState::Number(number) = &host.dialog else {
                return Err("Number dialog changed".into());
            };
            self.save_settings_number(*field, &number.draft.buf, directory.clone(), None)?;
            *inference_revision = Some(self.onboarding_revision);
            if let crate::filesystem::configurations::InferenceSaveState::Pending {
                operation,
                ..
            } = &self.inference_save_state
            {
                autosave.operation = Some(operation.clone());
            }
            Ok(())
        })();
        if let Err(error) = result {
            host.reject(error);
        }
        self.mode = Mode::ValueDialog(host);
        true
    }

    pub(crate) fn step_settings_number(&mut self, field: SettingsNumber, direction: i32) {
        if let SettingsNumber::Remote(row) = field {
            // Preserve remote settings' existing 600 ms numeric save debounce.
            self.settings_adjust_remote_compaction_row(row, direction);
            return;
        }
        let result = (|| {
            let text = field.stepped(self, direction)?;
            let directory = self
                .config_dir
                .clone()
                .ok_or_else(|| "The configuration directory is unavailable".to_owned())?;
            self.save_settings_number(field, &text, directory, None)
        })();
        if let Err(error) = result {
            self.status_message = Some(error);
        }
    }

    pub(crate) fn save_settings_number(
        &mut self,
        field: SettingsNumber,
        text: &str,
        directory: std::path::PathBuf,
        token: Option<std::sync::Arc<()>>,
    ) -> Result<(), String> {
        use crate::filesystem::configuration::ConfigurationChange;
        use crate::filesystem::configurations::ConfigurationIntent;
        if field == SettingsNumber::InferenceTokenBudget {
            let limit = u32::try_from(ScalarNumber::InferenceTokenBudget.parse(text)?)
                .map_err(|_| "Token budget exceeds storage range")?;
            let mut desired = self.inference_settings.clone();
            desired.restructure_prompt_token_limit = limit;
            self.enqueue_inference_value(directory, &desired, token)?;
            self.apply_inference_settings_locally(desired);
            return Ok(());
        }
        if field.is_detection() {
            let original = self
                .agent_detection_settings
                .as_ref()
                .ok_or("Detection settings are loading")?;
            if self.agent_detection_settings_pending || self.detection_number_pending.is_some() {
                return Err("A detection settings write is already pending".into());
            }
            let desired = field.detection_desired(original, text)?;
            if !self.queue_request(ilium_ipc::ClientRequest::UpdateAgentDetectionSettings {
                request_id: None,
                settings: desired,
            }) {
                return Err("Server settings request was not admitted".into());
            }
            self.agent_detection_settings_pending = true;
            self.agent_detection_settings_error = None;
            return Ok(());
        }
        // Construct and validate from current settings, preserving concurrent fields.
        let change = match field {
            SettingsNumber::Remote(row) => {
                use crate::remote_compaction_settings::RemoteCompactionRow;
                let NumberValue::Integer(value) = field.snapshot(self).0.parse(text)? else {
                    return Err("Enter a whole number".into());
                };
                let mut settings = self.remote_compaction_settings.clone();
                match row {
                    RemoteCompactionRow::Threshold => settings.threshold_percent = value as _,
                    RemoteCompactionRow::PauseTimeout => {
                        settings.pause_timeout_seconds = value as _
                    }
                    RemoteCompactionRow::Cooldown => settings.cooldown_minutes = value as _,
                    RemoteCompactionRow::TailTokens => settings.tail_tokens = value as _,
                    RemoteCompactionRow::ProtectedToolTokens => {
                        settings.protected_recent_tool_tokens = value as _
                    }
                    RemoteCompactionRow::ToolResultChars => settings.tool_result_chars = value as _,
                    RemoteCompactionRow::SummarizerContextTokens => {
                        settings.summarizer_context_tokens = value as _
                    }
                    RemoteCompactionRow::KeepBackups => settings.keep_backups = value as _,
                    _ => return Err("Not a numeric remote setting".into()),
                }
                ConfigurationChange::RemoteCompaction(settings)
            }

            SettingsNumber::WorkingPollSeconds | SettingsNumber::IdlePollSeconds => {
                return Err("Server number was not dispatched".into());
            }
            SettingsNumber::InferenceTokenBudget => {
                return Err("Inference number was not dispatched".into());
            }
            SettingsNumber::VoiceVolume => {
                let mut settings = self.voice_settings.clone();
                settings.output_volume_percent =
                    u8::try_from(ScalarNumber::VoiceVolume.parse(text)?)
                        .map_err(|_| "Volume exceeds storage range")?;
                ConfigurationChange::Voice(settings)
            }
            SettingsNumber::NotificationCoalesce => {
                let mut settings = self.notification_settings;
                settings.task_coalesce_seconds =
                    u32::try_from(ScalarNumber::NotificationCoalesce.parse(text)?)
                        .map_err(|_| "Duration exceeds storage range")?;
                ConfigurationChange::Notifications(settings)
            }
            SettingsNumber::Panel(field) => {
                let mut settings = self.ui_settings.clone();
                field.set(&mut settings.left_panel_sizing, text)?;
                ConfigurationChange::Ui(Box::new(settings))
            }
            SettingsNumber::Ui(field) => {
                let mut settings = self.ui_settings.clone();
                let value = field.scalar().parse(text)?;
                match field {
                    UiNumber::AutoFreezeAfter => settings.auto_freeze_after_seconds = value,
                    UiNumber::LastPromptLines => {
                        settings.last_prompt_max_lines =
                            u8::try_from(value).map_err(|_| "Line count exceeds storage range")?
                    }
                    UiNumber::ProgressLines => {
                        settings.progress_max_lines =
                            u8::try_from(value).map_err(|_| "Line count exceeds storage range")?
                    }
                    UiNumber::CompletedProgressHideAfter => {
                        settings.completed_progress_hide_after_seconds =
                            u32::try_from(value).map_err(|_| "Duration exceeds storage range")?
                    }
                }
                ConfigurationChange::Ui(Box::new(settings))
            }
            SettingsNumber::TerminalScrollback => {
                let mut settings = self.terminal_settings;
                value_config::set_terminal_scrollback(&mut settings, text)?;
                ConfigurationChange::Terminal(settings)
            }
            SettingsNumber::TerminalEngineMemory => {
                let mut settings = self.terminal_settings;
                value_config::set_terminal_engine_memory(&mut settings, text)?;
                ConfigurationChange::Terminal(settings)
            }
            SettingsNumber::EditorAutosaveDelay => {
                let mut settings = self.editor_settings;
                value_config::set_editor_autosave_delay(&mut settings, text)?;
                ConfigurationChange::Editor(settings)
            }
            SettingsNumber::Board(field) => {
                let mut settings = self.kanban_board_settings;
                field.set(&mut settings, text)?;
                ConfigurationChange::Kanban(settings)
            }
            SettingsNumber::ApiPort => {
                let mut settings = self.api_settings;
                settings.port = u16::try_from(ScalarNumber::ApiPort.parse(text)?)
                    .map_err(|_| "API port exceeds its storage range".to_owned())?;
                ConfigurationChange::Api(settings)
            }
        };
        let intent = match token {
            Some(token) => ConfigurationIntent::ValueDialog { token },
            None => ConfigurationIntent::Plain {
                label: "numeric settings",
                success: (field == SettingsNumber::ApiPort)
                    .then_some("HTTP API port saved; restart the server to apply it"),
            },
        };
        // Admission precedes local runtime changes. The same immutable values
        // are supplied to the writer and applied to existing consumers.
        match change {
            ConfigurationChange::RemoteCompaction(settings) => {
                self.enqueue_configuration(
                    directory,
                    ConfigurationChange::RemoteCompaction(settings.clone()),
                    intent,
                )?;
                self.apply_remote_compaction_settings(settings);
            }

            ConfigurationChange::Voice(settings) => {
                self.enqueue_configuration(
                    directory,
                    ConfigurationChange::Voice(settings.clone()),
                    intent,
                )?;
                self.apply_voice_runtime_settings(settings);
            }
            ConfigurationChange::Notifications(settings) => {
                self.enqueue_configuration(
                    directory,
                    ConfigurationChange::Notifications(settings),
                    intent,
                )?;
                self.apply_notification_settings(settings);
            }
            ConfigurationChange::Ui(settings) => {
                self.enqueue_configuration(
                    directory,
                    ConfigurationChange::Ui(settings.clone()),
                    intent,
                )?;
                // Numeric fields do not change monitor/debug toggles; use the
                // same layout and runtime application path as other UI settings.
                self.apply_ui_settings(*settings);
                if matches!(field, SettingsNumber::Ui(_)) {
                    // Banner/footer reservations can change while the outer
                    // layout stays equal; set_layout alone cannot resize them.
                    self.resize_displayed_panes(ilium_ipc::PaneResizeCause::UserInterfaceSettings);
                }
            }
            ConfigurationChange::Terminal(settings) => {
                self.enqueue_configuration(
                    directory,
                    ConfigurationChange::Terminal(settings),
                    intent,
                )?;
                self.apply_terminal_settings(settings);
            }
            ConfigurationChange::Editor(settings) => {
                self.enqueue_configuration(
                    directory,
                    ConfigurationChange::Editor(settings),
                    intent,
                )?;
                self.apply_editor_settings(settings);
            }
            ConfigurationChange::Kanban(settings) => {
                self.enqueue_configuration(
                    directory,
                    ConfigurationChange::Kanban(settings),
                    intent,
                )?;
                self.kanban_board_settings = settings;
            }
            ConfigurationChange::Api(settings) => {
                self.enqueue_configuration(directory, ConfigurationChange::Api(settings), intent)?;
                self.api_settings = settings;
            }
            _ => return Err("Unsupported numeric settings destination".into()),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::SettingsState;
    use crate::value_dialog::{DialogOutcome, ValueDialogState};

    #[test]
    fn every_numeric_settings_binding_renders_centered_value_and_live_pointer_targets() {
        use crate::value_control::{
            cell_width, ControlAction, PointerButton, NUMBER_DECREMENT_GLYPH,
            NUMBER_INCREMENT_GLYPH,
        };
        use ratatui::layout::Rect;
        use ratatui::{backend::TestBackend, Terminal};

        let directory = tempfile::tempdir().unwrap();
        let mut app = App::new(
            "all-numeric-settings-controls".into(),
            directory.path().into(),
        );
        app.config_dir = Some(directory.path().to_owned());
        app.agent_detection_settings = Some(ilium_ipc::AgentDetectionSettings {
            working_poll_seconds: 15,
            idle_poll_seconds: 45,
            custom_signatures: Vec::new(),
        });
        let screen = Rect::new(0, 0, 140, 180);
        app.set_screen_area(screen);

        let mut binding_count = 0;
        for tab in SettingsTab::ALL {
            let state = SettingsState {
                tab,
                ..SettingsState::default()
            };
            let mut layout = crate::settings_ui::compute_layout_for_mode(screen, &app, &state);
            let instructions = crate::instruction_settings::panel_height(tab, layout.content_area);
            layout.content_area.y += instructions;
            layout.content_area.height = layout.content_area.height.saturating_sub(instructions);
            let mut terminal =
                Terminal::new(TestBackend::new(screen.width, screen.height)).unwrap();
            terminal
                .draw(|frame| crate::settings_ui::render(frame, frame.area(), &app, &state))
                .unwrap();

            for row in 0..crate::settings_ui::settings_number_row_count(&app, tab) {
                let Some(field) = SettingsNumber::at(&app, tab, row) else {
                    continue;
                };
                binding_count += 1;

                let (actual_field, control) = crate::settings_ui::settings_number_control(
                    layout.content_area,
                    &app,
                    &state,
                    row,
                )
                .unwrap_or_else(|| panic!("{tab:?}/{row} must render its numeric setting"));
                assert_eq!(actual_field, field, "{tab:?}/{row}");

                let geometry = control.geometry();
                assert_eq!(
                    geometry.value.x - geometry.value_slot.x,
                    (geometry.value_slot.width - geometry.value.width) / 2,
                    "{tab:?}/{row} value must be centered"
                );
                for (rectangle, glyph, action) in [
                    (
                        geometry.previous,
                        NUMBER_DECREMENT_GLYPH,
                        ControlAction::Decrement,
                    ),
                    (
                        geometry.next,
                        NUMBER_INCREMENT_GLYPH,
                        ControlAction::Increment,
                    ),
                    (geometry.open, "*", ControlAction::EditNumber),
                ] {
                    assert_eq!(rectangle.width, cell_width(glyph) as u16, "{tab:?}/{row}");
                    assert_eq!(
                        terminal.backend().buffer()[(rectangle.x, rectangle.y)].symbol(),
                        glyph,
                        "{tab:?}/{row} visible button"
                    );
                    let hit = control.hit(
                        ratatui::layout::Position::new(rectangle.x, rectangle.y),
                        PointerButton::Left,
                    );
                    if action == ControlAction::EditNumber {
                        assert_eq!(hit, Some(action), "{tab:?}/{row} direct-entry target");
                    } else {
                        assert!(
                            hit.is_none() || hit == Some(action),
                            "{tab:?}/{row} button maps to {action:?} or is disabled at its bound"
                        );
                    }
                }

                app.mode = Mode::Settings(SettingsState {
                    tab,
                    selected_row: row,
                    ..SettingsState::default()
                });
                app.begin_settings_number_dialog(field);
                let Mode::ValueDialog(host) = std::mem::replace(&mut app.mode, Mode::Normal) else {
                    panic!("{tab:?}/{row} must open its numeric dialog");
                };
                assert!(
                    matches!(&host.dialog, ValueDialogState::Number(_)),
                    "{tab:?}/{row} must open a number editor"
                );
                assert!(
                    matches!(
                        &host.target,
                        crate::value_dialog_host::ValueTarget::SettingsNumber { field: actual, .. }
                            | crate::value_dialog_host::ValueTarget::DetectionNumber { field: actual, .. }
                            if *actual == field
                    ),
                    "{tab:?}/{row} dialog must target {field:?}"
                );
                app.finish_value_dialog(host, DialogOutcome::Cancel);
            }
        }

        assert_eq!(binding_count, 26, "all numeric settings rows must be bound");
    }

    #[test]
    fn auto_freeze_delay_supports_exact_seconds_and_original_arrow_steps() {
        let directory = tempfile::tempdir().unwrap();
        let mut app = App::new("freeze-number".into(), directory.path().into());
        let field = SettingsNumber::Ui(UiNumber::AutoFreezeAfter);
        let (spec, _) = field.snapshot(&app);
        assert!(spec.parse("1").is_ok());
        assert!(spec.parse("901").is_ok());
        assert!(spec.parse("0").is_err());
        assert!(spec.parse("1.5").is_err());
        assert!(spec.parse(&(i128::from(i64::MAX) + 1).to_string()).is_err());
        app.ui_settings.auto_freeze_after_seconds = 901;
        assert_eq!(field.stepped(&app, -1).unwrap(), "1");
        assert_eq!(field.stepped(&app, 1).unwrap(), "1801");
        let rows = AppearanceRow::visible(app.ui_settings.left_panel_sizing.mode);
        let row = rows
            .iter()
            .position(|row| *row == AppearanceRow::AutoFreezeAfter)
            .unwrap();
        assert_eq!(
            SettingsNumber::at(&app, SettingsTab::Appearance, row),
            Some(field)
        );
        let original = app.ui_settings.clone();
        app.save_settings_number(field, "137", directory.path().into(), None)
            .unwrap();
        assert_eq!(app.ui_settings.auto_freeze_after_seconds, 137);
        app.settle_filesystem_for_test();
        let saved = crate::config::load(directory.path()).unwrap().ui;
        assert_eq!(saved.auto_freeze_after_seconds, 137);
        assert_eq!(saved.auto_freeze_enabled, original.auto_freeze_enabled);
        assert_eq!(saved.last_prompt_max_lines, original.last_prompt_max_lines);
        let attempts = app.configuration_admission.attempts;
        assert!(app
            .save_settings_number(field, "0", directory.path().into(), None)
            .is_err());
        assert_eq!(app.configuration_admission.attempts, attempts);
        assert_eq!(app.ui_settings.auto_freeze_after_seconds, 137);
    }

    #[test]
    fn remote_numbers_validate_bounds_and_preserve_original_ladders() {
        use crate::remote_compaction_settings::RemoteCompactionRow as Row;
        let directory = tempfile::tempdir().unwrap();
        let mut app = App::new("remote-number".into(), directory.path().into());
        for row in [
            Row::Threshold,
            Row::PauseTimeout,
            Row::Cooldown,
            Row::TailTokens,
            Row::ProtectedToolTokens,
            Row::ToolResultChars,
            Row::SummarizerContextTokens,
            Row::KeepBackups,
        ] {
            let field = SettingsNumber::Remote(row);
            let (spec, current) = field.snapshot(&app);
            let NumberSpec::Integer { minimum, maximum } = spec else {
                panic!("Remote settings require whole numbers");
            };
            assert!(spec.parse(&minimum.to_string()).is_ok());
            assert!(spec.parse(&maximum.to_string()).is_ok());
            assert!(spec.parse(&(minimum - 1).to_string()).is_err());
            assert!(spec.parse(&(maximum + 1).to_string()).is_err());
            assert!(spec.parse("1.5").is_err());
            assert!(spec.parse(&current).is_ok());
            for direction in [-1, 1] {
                let actual = field.stepped(&app, direction).unwrap();
                let original = app.remote_compaction_settings.clone();
                app.remote_compaction_settings.adjust(row, direction);
                let expected = field.snapshot(&app).1;
                app.remote_compaction_settings = original;
                assert_eq!(actual, expected, "{row:?}, direction {direction}");
            }
            let original = app.remote_compaction_settings.clone();
            app.save_settings_number(field, &minimum.to_string(), directory.path().into(), None)
                .unwrap();
            assert_eq!(field.snapshot(&app).1, minimum.to_string());
            app.settle_filesystem_for_test();
            let saved = crate::config::load(directory.path())
                .unwrap()
                .remote_compaction;
            assert_eq!(saved.enabled, original.enabled);
            app.remote_compaction_settings = saved;
            assert_eq!(
                field.snapshot(&app).1,
                minimum.to_string(),
                "{row:?} reload"
            );
            let attempts = app.configuration_admission.attempts;
            assert!(app
                .save_settings_number(
                    field,
                    &(maximum + 1).to_string(),
                    directory.path().into(),
                    None
                )
                .is_err());
            assert_eq!(app.configuration_admission.attempts, attempts);
            assert_eq!(field.snapshot(&app).1, minimum.to_string());
        }
    }

    #[test]
    fn exact_voice_volume_reconciles_once_and_notification_limits_do_not_wrap() {
        let directory = tempfile::tempdir().unwrap();
        let mut app = App::new("voice-number".into(), directory.path().into());
        app.config_dir = Some(directory.path().into());
        app.voice_settings.enabled = true;
        let original = app.voice_settings.clone();
        app.save_settings_number(
            SettingsNumber::VoiceVolume,
            "73",
            directory.path().into(),
            None,
        )
        .unwrap();
        assert_eq!(app.voice_settings.output_volume_percent, 73);
        assert!(matches!(
            app.take_voice_runtime_request(),
            Some(crate::app::VoiceRuntimeRequest::Reconfigure)
        ));
        assert!(app.take_voice_runtime_request().is_none());
        app.settle_filesystem_for_test();
        let saved = crate::config::load(directory.path()).unwrap().voice;
        assert_eq!(saved.output_volume_percent, 73);
        assert_eq!(saved.model, original.model);
        assert_eq!(saved.voice, original.voice);
        let attempts = app.configuration_admission.attempts;
        assert!(app
            .save_settings_number(
                SettingsNumber::VoiceVolume,
                "101",
                directory.path().into(),
                None
            )
            .is_err());
        assert_eq!(app.configuration_admission.attempts, attempts);
        assert_eq!(app.voice_settings.output_volume_percent, 73);
        assert!(app.take_voice_runtime_request().is_none());
        app.save_settings_number(
            SettingsNumber::NotificationCoalesce,
            "17",
            directory.path().into(),
            None,
        )
        .unwrap();
        assert_eq!(app.notification_settings.task_coalesce_seconds, 17);
        assert_eq!(
            SettingsNumber::NotificationCoalesce
                .stepped(&app, -1)
                .unwrap(),
            "7"
        );
        app.notification_settings.task_coalesce_seconds =
            ilium_sound::NotificationSettings::MAX_TASK_COALESCE_SECONDS;
        assert_eq!(
            SettingsNumber::NotificationCoalesce
                .stepped(&app, 1)
                .unwrap(),
            ilium_sound::NotificationSettings::MAX_TASK_COALESCE_SECONDS.to_string()
        );
        app.settle_filesystem_for_test();
        assert_eq!(
            crate::config::load(directory.path())
                .unwrap()
                .notifications
                .task_coalesce_seconds,
            17
        );
    }

    #[test]
    fn direct_settings_number_preserves_exact_draft_after_failure_and_retries() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        std::fs::write(&path, "[editor\n").unwrap();
        let mut app = App::new("numeric-settings-test".into(), directory.path().into());
        app.config_dir = Some(directory.path().into());
        app.mode = Mode::Settings(SettingsState {
            tab: SettingsTab::Editor,
            selected_row: 4,
            ..SettingsState::default()
        });
        app.begin_settings_number_dialog(SettingsNumber::EditorAutosaveDelay);
        let Mode::ValueDialog(mut host) = std::mem::replace(&mut app.mode, Mode::Normal) else {
            panic!("numeric editor");
        };
        let ValueDialogState::Number(number) = &mut host.dialog else {
            panic!("number");
        };
        number.draft = crate::text_prompt::TextPromptState::new("01101");
        app.finish_value_dialog(host, DialogOutcome::CommitNumber("01101".into()));
        app.settle_filesystem_for_test();
        let Mode::ValueDialog(host) = &app.mode else {
            panic!("failure keeps dialog");
        };
        assert!(!host.is_saving());
        let ValueDialogState::Number(number) = &host.dialog else {
            panic!("number");
        };
        assert_eq!(number.draft.buf, "01101");
        assert!(number.error.is_some());
        assert_eq!(app.editor_settings.autosave_delay_ms, 1101);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "[editor\n");
        std::fs::write(&path, "[editor]\n[api]\nport = 8873\n").unwrap();
        let Mode::ValueDialog(host) = std::mem::replace(&mut app.mode, Mode::Normal) else {
            panic!("retry");
        };
        app.finish_value_dialog(host, DialogOutcome::CommitNumber("01101".into()));
        app.settle_filesystem_for_test();
        assert!(matches!(app.mode, Mode::Settings(_)));
        let config = crate::config::load(directory.path()).unwrap();
        assert_eq!(config.editor.autosave_delay_ms, 1101);
        assert_eq!(config.api.port, 8873);
    }

    #[test]
    fn numeric_settings_validation_and_changed_parent_cannot_mutate_or_enqueue() {
        let mut app = App::new("numeric-settings-test".into(), std::env::temp_dir());
        app.config_dir = Some(std::env::temp_dir());
        app.mode = Mode::Settings(SettingsState {
            tab: SettingsTab::Terminal,
            ..SettingsState::default()
        });
        app.begin_settings_number_dialog(SettingsNumber::TerminalScrollback);
        let Mode::ValueDialog(mut host) = std::mem::replace(&mut app.mode, Mode::Normal) else {
            panic!("dialog");
        };
        let original = app.terminal_settings;
        let attempts = app.configuration_admission.attempts;
        for invalid in ["", "513", "17.5", "-1", "NaN"] {
            assert!(app
                .commit_settings_number_dialog(
                    &mut host,
                    &DialogOutcome::CommitNumber(invalid.into())
                )
                .is_err());
            assert_eq!(app.terminal_settings, original);
            assert_eq!(app.configuration_admission.attempts, attempts);
        }
        let Some(Mode::Settings(parent)) = app.modal_stack.last_mut() else {
            panic!("parent");
        };
        parent.tab = SettingsTab::Editor;
        assert!(app
            .commit_settings_number_dialog(&mut host, &DialogOutcome::CommitNumber("17".into()))
            .is_err());
        assert_eq!(app.terminal_settings, original);
        assert_eq!(app.configuration_admission.attempts, attempts);
    }
}

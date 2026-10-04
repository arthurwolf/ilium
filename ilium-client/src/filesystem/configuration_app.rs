#[derive(Default)]
pub(crate) struct ConfigurationAdmission {
    pub attempts: u64,
    pub accepted: u64,
    pub rejection: Option<String>,
}

use super::configuration::{ConfigurationChange, ConfigurationSaved};
use super::configurations::{ConfigurationIntent, InferenceSaveState};
use super::ordered::WriteCompletion;
use crate::app::{App, InferenceSettingField, Mode};
use ilium_execution::JobOutcome;
use std::path::PathBuf;
use std::sync::Arc;

impl App {
    pub(crate) fn enqueue_configuration(
        &mut self,
        directory: PathBuf,
        change: ConfigurationChange,
        intent: ConfigurationIntent,
    ) -> Result<(), String> {
        let result = self
            .configuration_files
            .as_mut()
            .ok_or_else(|| "Configuration writer unavailable; change is unsaved".to_owned())
            .and_then(|files| files.enqueue(directory, change, intent));
        self.configuration_admission.attempts =
            self.configuration_admission.attempts.saturating_add(1);
        match &result {
            Ok(()) => {
                self.configuration_admission.accepted =
                    self.configuration_admission.accepted.saturating_add(1)
            }
            Err(error) => self.configuration_admission.rejection = Some(error.clone()),
        }
        self.status_message = Some(match &result {
            Ok(()) => "Saving settings…".into(),
            Err(error) => error.clone(),
        });
        result
    }
    pub(crate) fn persist_configuration(
        &mut self,
        change: ConfigurationChange,
        label: &'static str,
    ) {
        if let Some(directory) = self.config_dir.clone() {
            let _ = self.enqueue_configuration(
                directory,
                change,
                ConfigurationIntent::Plain {
                    label,
                    success: None,
                },
            );
        } else {
            self.configuration_admission.attempts =
                self.configuration_admission.attempts.saturating_add(1);
            let error = "Configuration directory unavailable; settings remain unsaved".to_owned();
            self.configuration_admission.rejection = Some(error.clone());
            self.status_message = Some(error);
        }
    }
    pub(crate) fn persist_inference_configuration(&mut self) {
        let operation = Arc::new(());
        self.inference_save_state = InferenceSaveState::Pending {
            operation: Arc::clone(&operation),
            budget_dialog: None,
        };
        self.inference_settings_save_error = None;
        let prepared = self
            .config_dir
            .clone()
            .ok_or_else(|| {
                "Configuration directory unavailable; settings remain unsaved".to_owned()
            })
            .and_then(|directory| {
                super::configuration::inference_snapshot(&self.inference_settings)
                    .map(|change| (directory, change))
            });
        let result = match prepared {
            Ok((directory, change)) => self.enqueue_configuration(
                directory,
                change,
                ConfigurationIntent::Inference {
                    operation: Arc::clone(&operation),
                },
            ),
            Err(error) => {
                self.configuration_admission.attempts =
                    self.configuration_admission.attempts.saturating_add(1);
                self.configuration_admission.rejection = Some(error.clone());
                Err(error)
            }
        };
        if let Err(error) = result {
            self.finish_inference_save(&operation, Err(error));
        }
    }

    pub(crate) fn collect_inference_configuration(
        &mut self,
        operation: &Arc<()>,
        completion: WriteCompletion<super::configuration::ConfigurationWrite>,
    ) {
        self.collect_inference_value_configuration(operation, None, completion);
    }

    fn collect_inference_value_configuration(
        &mut self,
        operation: &Arc<()>,
        token: Option<&Arc<()>>,
        completion: WriteCompletion<super::configuration::ConfigurationWrite>,
    ) {
        match completion {
            WriteCompletion::Outcome { outcome, .. } => {
                let _retained = outcome.map(|outcome| {
                    let result = match outcome {
                        JobOutcome::Finished(Ok(ConfigurationSaved::Plain)) => Ok(()),
                        JobOutcome::Finished(Err(failure)) => Err(failure.message),
                        JobOutcome::Finished(Ok(_)) => {
                            Err(
                                "Unexpected inference configuration receipt; save unconfirmed".into(),
                            )
                        }
                        JobOutcome::NotStarted { .. } | JobOutcome::Panicked => {
                            Err(
                                "Configuration worker did not complete the write; publication is unconfirmed".into(),
                            )
                        }
                    };
                    self.finish_inference_value_save(operation, token, result);
                });
            }
            WriteCompletion::Rejected { rejection, .. } => {
                self.finish_inference_value_save(
                    operation,
                    token,
                    Err(format!("Settings remain unsaved: {:?}", rejection.reason)),
                );
            }
            WriteCompletion::Lost { .. } => {
                self.finish_inference_value_save(
                    operation,
                    token,
                    Err("Configuration receipt lost; publication is unconfirmed".into()),
                );
            }
        }
    }

    fn finish_inference_value_save(
        &mut self,
        operation: &Arc<()>,
        token: Option<&Arc<()>>,
        result: Result<(), String>,
    ) {
        // Operation and dialog token jointly own settlement. An obsolete write
        // must not dismiss even a host that still retains its older save token.
        if !matches!(&self.inference_save_state,
            InferenceSaveState::Pending { operation: current, .. }
            if Arc::ptr_eq(current, operation))
        {
            return;
        }
        self.finish_inference_number_autosave(operation, &result);
        self.finish_inference_save(operation, result.clone());
        if let Some(token) = token {
            self.finish_value_dialog_save(token, result);
        }
    }

    pub(crate) fn finish_inference_save(
        &mut self,
        operation: &Arc<()>,
        result: Result<(), String>,
    ) {
        let budget_dialog = match &self.inference_save_state {
            InferenceSaveState::Pending {
                operation: current,
                budget_dialog,
            } if Arc::ptr_eq(current, operation) => budget_dialog.clone(),
            _ => return,
        };
        self.inference_save_state = InferenceSaveState::Unsaved;
        if let Err(error) = result {
            self.status_message = Some(format!("Could not save Inference settings: {error}"));
            self.inference_settings_save_error = Some(error);
            return;
        }
        self.inference_save_state = InferenceSaveState::Durable;
        self.inference_settings_save_error = None;
        self.status_message = Some("Inference settings saved to disk".into());
        let Some((identity, limit)) = budget_dialog else {
            return;
        };
        let owns_dialog = self
            .restructure_budget_identity
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, &identity));
        let owns_value = matches!(
            &self.mode,
            Mode::InferenceSettingPrompt(
                InferenceSettingField::RestructurePromptTokenLimit,
                state
            ) if state.buf.trim().parse::<u32>() == Ok(limit)
        );
        if !owns_dialog || !owns_value {
            return;
        }
        self.restructure_budget_identity = None;
        self.restructure_budget_autosave_deadline = None;
        self.pop_modal();
    }

    fn collect_value_dialog_configuration(
        &mut self,
        token: &Arc<()>,
        completion: WriteCompletion<super::configuration::ConfigurationWrite>,
    ) {
        match completion {
            WriteCompletion::Outcome { outcome, .. } => {
                let _retained = outcome.map(|outcome| {
                    let result = match outcome {
                        JobOutcome::Finished(Ok(ConfigurationSaved::Plain)) => Ok(()),
                        JobOutcome::Finished(Err(failure)) => Err(failure.message),
                        JobOutcome::Finished(Ok(_)) => Err("Unexpected value-dialog receipt; publication is unconfirmed".into()),
                        JobOutcome::NotStarted { .. } | JobOutcome::Panicked => Err("Configuration worker did not complete the write; publication is unconfirmed".into()),
                    };
                    self.finish_value_dialog_save(token, result);
                });
            }
            WriteCompletion::Rejected { rejection, .. } => self.finish_value_dialog_save(
                token,
                Err(format!("Settings remain unsaved: {:?}", rejection.reason)),
            ),
            WriteCompletion::Lost { .. } => self.finish_value_dialog_save(
                token,
                Err("Configuration receipt lost; publication is unconfirmed".into()),
            ),
        }
    }

    pub(crate) fn collect_configuration_files(&mut self) -> bool {
        let Some(mut files) = self.configuration_files.take() else {
            return false;
        };
        let mut changed = false;
        if let Some((path, result)) = files.poll_project() {
            // A selection changed after admission may still own an obsolete
            // receipt. Only the selected binding may install the read.
            if self
                .animation_write_path()
                .as_ref()
                .is_ok_and(|current| *current == path)
                || self.animation_project_binding() == path
            {
                self.install_animation_project_settings(path, result);
                changed = true;
            }
        }

        while self.animation_frame.can_queue_configuration() {
            let Some((intent, completion)) = files.poll() else {
                break;
            };
            changed = true;
            if let Some(ConfigurationIntent::SessionValueDialog { desired, token }) = &intent {
                self.collect_session_choice_configuration(*desired, token, completion);
                continue;
            }
            if let Some(ConfigurationIntent::ValueDialog { token }) = &intent {
                self.collect_value_dialog_configuration(token, completion);
                continue;
            }
            if let Some(ConfigurationIntent::InferenceValueDialog { operation, token }) = &intent {
                self.collect_inference_value_configuration(operation, Some(token), completion);
                continue;
            }
            if let Some(ConfigurationIntent::Inference { operation }) = &intent {
                self.collect_inference_configuration(operation, completion);
                continue;
            }
            let mut result = None;
            match completion {
                WriteCompletion::Outcome { outcome, .. } => {
                    let _retained=outcome.map(|outcome|{result=Some(match outcome {
                    JobOutcome::Finished(result)=>result,
                    JobOutcome::NotStarted { .. }|JobOutcome::Panicked=>Err(super::configuration::ConfigurationFailure{message:"Configuration worker did not complete the write; publication is unconfirmed".into(),observed_session:None,observed_agent_setup:None}),
                });});
                }
                WriteCompletion::Rejected { rejection, .. } => {
                    let message = format!("Settings remain unsaved: {:?}", rejection.reason);
                    if let Some(ConfigurationIntent::Animation {
                        picker: Some(save), ..
                    }) = &intent
                    {
                        self.finish_animation_picker_save(save, Err(message.clone()));
                    }
                    if matches!(intent, Some(ConfigurationIntent::Onboarding { .. })) {
                        self.onboarding_dismiss_pending = false;
                    }
                    if let Some(ConfigurationIntent::Animation {
                        value_dialog: Some(token),
                        ..
                    }) = &intent
                    {
                        self.finish_value_dialog_save(token, Err(message.clone()));
                    }
                    self.status_message = Some(message);
                }
                WriteCompletion::Lost { .. } => {
                    let message =
                        "Configuration receipt lost; publication is unconfirmed".to_owned();
                    if let Some(ConfigurationIntent::Animation {
                        picker: Some(save), ..
                    }) = &intent
                    {
                        self.finish_animation_picker_save(save, Err(message.clone()));
                    }
                    if matches!(intent, Some(ConfigurationIntent::Onboarding { .. })) {
                        self.onboarding_dismiss_pending = false;
                    }
                    if let Some(ConfigurationIntent::Animation {
                        value_dialog: Some(token),
                        ..
                    }) = &intent
                    {
                        self.finish_value_dialog_save(token, Err(message.clone()));
                    }
                    self.status_message = Some(message);
                }
            }
            let Some(result) = result else {
                continue;
            };
            match result {
                Ok(saved) => {
                    if let Some(ConfigurationIntent::Plain { label, .. }) = &intent {
                        if *label == "Git settings" {
                            self.git_settings_error = None;
                        }
                    }
                    match saved {
                        ConfigurationSaved::Session(saved) => {
                            if matches!(&intent,Some(ConfigurationIntent::Session{desired}) if self.session_settings==*desired)
                            {
                                self.session_settings = saved;
                            }
                        }
                        ConfigurationSaved::AgentSetup(saved) => {
                            if matches!(&intent,Some(ConfigurationIntent::AgentSetup{desired,..}) if &self.agent_setup_settings==desired)
                            {
                                self.agent_setup_settings = saved;
                            }
                            if let Some(ConfigurationIntent::AgentSetup {
                                dialog: Some(dialog),
                                ..
                            }) = &intent
                            {
                                use super::configurations::AgentSetupDialog;
                                let matching = match (dialog, &self.mode) {
                                    (
                                        AgentSetupDialog::Path { feature, value },
                                        Mode::AgentSetupPathPrompt(current, state),
                                    ) => feature == current && *value == state.buf,
                                    (
                                        AgentSetupDialog::Suppression(expected),
                                        Mode::AgentSetupPrompt(current),
                                    ) => {
                                        std::sync::Arc::ptr_eq(
                                            &expected.identity,
                                            &current.identity,
                                        ) && expected == current.as_ref()
                                    }
                                    _ => false,
                                };
                                if matching {
                                    self.pop_modal();
                                }
                            }
                            self.refresh_agent_setup_statuses();
                            self.reconcile_agent_setup_prompts();
                        }
                        ConfigurationSaved::TextTriggers(settings) => {
                            self.queue_request(ilium_ipc::ClientRequest::UpdateTextTriggers {
                                settings,
                            });
                            if let Some(ConfigurationIntent::TextTriggers {
                                dialog: Some(expected),
                            }) = &intent
                            {
                                if matches!(&self.mode,Mode::TextTriggerDialog(state) if state.candidate()==*expected)
                                {
                                    self.pop_modal();
                                }
                            }
                        }
                        ConfigurationSaved::Animation(settings) => {
                            if let Some(ConfigurationIntent::Animation {
                                path,
                                picker,
                                value_dialog,
                                ..
                            }) = &intent
                            {
                                // Validate the accepted execution identity before native
                                // acknowledgement advances the desired frame revision.
                                let value_result = value_dialog
                                    .as_ref()
                                    .map(|_| self.validate_plugin_value_receipt());
                                self.acknowledge_animation_settings(path, *settings);
                                if let Some(save) = picker {
                                    self.finish_animation_picker_save(save, Ok(()));
                                }
                                if let (Some(token), Some(result)) = (value_dialog, value_result) {
                                    self.finish_value_dialog_save(token, result);
                                }
                            }
                        }
                        ConfigurationSaved::Separators(_) => {}
                        ConfigurationSaved::Plain => {}
                    }
                    if let Some(ConfigurationIntent::Onboarding { revision, dismiss }) = &intent {
                        if *revision == self.onboarding_revision && *dismiss {
                            self.onboarding = None;
                            self.onboarding_dismiss_pending = false;
                        }
                    }
                    let animation_pending =
                        matches!(&intent, Some(ConfigurationIntent::Animation { .. }))
                            && self.status_message.as_ref().is_some_and(|message| {
                                message.starts_with("Animation settings saved; execution pending:")
                            });
                    if animation_pending {
                        continue;
                    }
                    self.status_message = Some(match intent {
                        Some(ConfigurationIntent::Plain { label, success }) => success
                            .map(str::to_owned)
                            .unwrap_or_else(|| format!("{label} saved to disk")),
                        Some(ConfigurationIntent::TextTriggers { .. }) => {
                            "Text Triggers saved to disk".into()
                        }
                        _ => "Settings saved to disk".into(),
                    });
                }
                Err(failure) => {
                    let failure_label = match &intent {
                        Some(ConfigurationIntent::Plain { label, .. }) => *label,
                        Some(ConfigurationIntent::Animation { .. }) => "animation settings",
                        _ => "settings",
                    };
                    let animation_failure =
                        matches!(&intent, Some(ConfigurationIntent::Animation { .. }));
                    if let Some(ConfigurationIntent::Animation { path, desired, .. }) = &intent {
                        if self.animation_project_binding() == *path
                            && self.animation_settings == **desired
                        {
                            self.failed_animation_settings = Some((**desired).clone());
                            if let Some(committed) = &self.committed_animation_settings {
                                self.animation_settings = committed.clone();
                            }
                        }
                    }

                    if let Some(ConfigurationIntent::Animation {
                        picker: Some(save), ..
                    }) = &intent
                    {
                        self.finish_animation_picker_save(save, Err(failure.message.clone()));
                    }
                    if let Some(ConfigurationIntent::Animation {
                        value_dialog: Some(token),
                        ..
                    }) = &intent
                    {
                        self.finish_value_dialog_save(token, Err(failure.message.clone()));
                    }

                    if let Some(ConfigurationIntent::Plain { label, .. }) = &intent {
                        if *label == "Git settings" {
                            self.git_settings_error = Some(failure.message.clone());
                        }
                    }
                    // Readback may reflect publication before a directory flush
                    // error. Never overwrite a subsequent edit or blindly roll
                    // back an already changed file.
                    match intent {
                        Some(ConfigurationIntent::Session { desired })
                            if self.session_settings == desired =>
                        {
                            if let Some(observed) = failure.observed_session {
                                self.session_settings = observed;
                            }
                        }
                        Some(ConfigurationIntent::AgentSetup { desired, .. })
                            if self.agent_setup_settings == desired =>
                        {
                            if let Some(observed) = failure.observed_agent_setup {
                                self.agent_setup_settings = observed;
                            }
                        }
                        Some(ConfigurationIntent::TextTriggers { .. }) => {
                            if let Mode::TextTriggerDialog(state) = &mut self.mode {
                                state.save_error = Some(failure.message.clone());
                            }
                        }
                        Some(ConfigurationIntent::Onboarding { .. }) => {
                            self.onboarding_dismiss_pending = false
                        }
                        _ => {}
                    }
                    let retained_candidate = if animation_failure {
                        self.failed_animation_settings
                            .as_ref()
                            .map(|settings| {
                                format!("; unsaved {} candidate retained", settings.kind.label())
                            })
                            .unwrap_or_default()
                    } else {
                        String::new()
                    };
                    self.status_message = Some(format!(
                        "Could not save {}: {}{}",
                        failure_label, failure.message, retained_candidate
                    ));
                }
            }
        }
        self.configuration_files = Some(files);
        changed
    }
}

#[cfg(test)]
mod inference_value_receipt_tests {
    use super::*;
    use crate::value_dialog::{DialogOutcome, ValueDialogState};
    use crate::value_settings::SettingsNumber;

    fn budget_app(directory: &std::path::Path) -> App {
        let mut app = App::new("synthetic-budget-receipt".into(), directory.into());
        app.config_dir = Some(directory.into());
        let row = crate::settings_ui::inference_rows(&app.inference_settings)
            .iter()
            .position(|row| {
                *row == crate::app::InferenceRow::Field(
                    InferenceSettingField::RestructurePromptTokenLimit,
                )
            })
            .unwrap();
        app.mode = Mode::Settings(crate::app::SettingsState {
            tab: crate::app::SettingsTab::Inference,
            selected_row: row,
            ..crate::app::SettingsState::default()
        });
        app.begin_settings_number_dialog(SettingsNumber::InferenceTokenBudget);
        app
    }

    fn paste_budget(app: &mut App, text: &str) -> std::time::Instant {
        let Mode::ValueDialog(host) = &mut app.mode else {
            panic!("number");
        };
        let ValueDialogState::Number(number) = &mut host.dialog else {
            panic!("number");
        };
        number.draft.buf.clear();
        crate::keys::handle_event(app, crossterm::event::Event::Paste(text.into()));
        let Mode::ValueDialog(host) = &app.mode else {
            panic!("number");
        };
        let crate::value_dialog_host::ValueTarget::SettingsNumber {
            autosave: Some(autosave),
            ..
        } = &host.target
        else {
            panic!("debounce");
        };
        autosave.deadline.unwrap()
    }

    #[test]
    fn typed_budget_debounces_draft_and_enter_upgrades_same_pending_write() {
        let directory = tempfile::tempdir().unwrap();
        let mut app = budget_app(directory.path());
        let deadline = paste_budget(&mut app, "123456");
        let attempts = app.configuration_admission.attempts;
        assert!(!app.tick_inference_number_autosave(deadline - std::time::Duration::from_millis(1)));
        assert_eq!(app.configuration_admission.attempts, attempts);
        assert!(app.tick_inference_number_autosave(deadline));
        assert!(matches!(&app.mode, Mode::ValueDialog(host) if !host.is_saving()));
        assert_eq!(app.configuration_admission.attempts, attempts + 1);
        crate::keys::handle_event(
            &mut app,
            crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Enter,
                crossterm::event::KeyModifiers::NONE,
            )),
        );
        assert!(matches!(&app.mode, Mode::ValueDialog(host) if host.is_saving()));
        assert_eq!(app.configuration_admission.attempts, attempts + 1);
        app.settle_filesystem_for_test();
        assert!(matches!(&app.mode, Mode::Settings(_)));
        assert_eq!(
            crate::config::load(directory.path())
                .unwrap()
                .inference
                .restructure_prompt_token_limit,
            123456
        );
    }

    #[test]
    fn typed_budget_autosave_keeps_dialog_open_and_escape_cancels_unsent_draft() {
        let directory = tempfile::tempdir().unwrap();
        let mut app = budget_app(directory.path());
        let deadline = paste_budget(&mut app, "123456");
        assert!(app.tick_inference_number_autosave(deadline));
        app.settle_filesystem_for_test();
        assert!(matches!(&app.mode, Mode::ValueDialog(host) if !host.is_saving()));
        assert!(matches!(
            app.inference_save_state,
            InferenceSaveState::Durable
        ));
        let next = paste_budget(&mut app, "234567");
        let attempts = app.configuration_admission.attempts;
        crate::keys::handle_event(
            &mut app,
            crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Esc,
                crossterm::event::KeyModifiers::NONE,
            )),
        );
        assert!(!app.tick_inference_number_autosave(next));
        assert_eq!(app.configuration_admission.attempts, attempts);
        assert_eq!(
            crate::config::load(directory.path())
                .unwrap()
                .inference
                .restructure_prompt_token_limit,
            123456
        );
    }

    #[test]
    fn latest_typed_budget_lost_receipt_keeps_exact_draft_and_retry_notice() {
        let directory = tempfile::tempdir().unwrap();
        let mut app = budget_app(directory.path());
        let token = Arc::new(());
        let operation = Arc::new(());
        let Mode::ValueDialog(host) = &mut app.mode else {
            panic!("number");
        };
        let ValueDialogState::Number(number) = &mut host.dialog else {
            panic!("number");
        };
        number.draft.buf = "123456".into();
        host.begin_save(token.clone());
        app.inference_save_state = InferenceSaveState::Pending {
            operation: operation.clone(),
            budget_dialog: None,
        };
        app.collect_inference_value_configuration(
            &operation,
            Some(&token),
            WriteCompletion::Lost {
                id: super::super::ordered::WriteId(1),
            },
        );
        let Mode::ValueDialog(host) = &app.mode else {
            panic!("lost receipt must retain child");
        };
        assert!(!host.is_saving());
        let ValueDialogState::Number(number) = &host.dialog else {
            panic!("number");
        };
        assert_eq!(number.draft.buf, "123456");
        assert!(number.error.as_deref().unwrap().contains("receipt lost"));
        assert!(matches!(
            app.inference_save_state,
            InferenceSaveState::Unsaved
        ));
    }

    #[test]
    fn superseded_budget_operation_cannot_settle_its_still_visible_save_token() {
        let directory = tempfile::tempdir().unwrap();
        let mut app = budget_app(directory.path());
        let Mode::ValueDialog(host) = &mut app.mode else {
            panic!("exact budget");
        };
        let token = Arc::new(());
        host.begin_save(token.clone());
        let obsolete = Arc::new(());
        let latest = Arc::new(());
        app.inference_save_state = InferenceSaveState::Pending {
            operation: latest.clone(),
            budget_dialog: None,
        };
        app.status_message = Some("newer save status".into());
        app.inference_settings_save_error = Some("newer save error".into());
        for result in [Ok(()), Err("obsolete write error".into())] {
            app.finish_inference_value_save(&obsolete, Some(&token), result);
            assert!(matches!(&app.mode, Mode::ValueDialog(host) if host.is_saving()));
            assert!(
                matches!(&app.inference_save_state, InferenceSaveState::Pending { operation, .. } if Arc::ptr_eq(operation, &latest))
            );
            assert_eq!(app.status_message.as_deref(), Some("newer save status"));
            assert_eq!(
                app.inference_settings_save_error.as_deref(),
                Some("newer save error")
            );
        }
    }

    #[test]
    fn typed_budget_waits_for_one_own_write_retains_failure_and_retries() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(
            directory.path().join("config.toml"),
            "invalid synthetic configuration = [",
        )
        .unwrap();
        let mut app = budget_app(directory.path());
        let Mode::ValueDialog(mut host) = std::mem::replace(&mut app.mode, Mode::Normal) else {
            panic!("exact budget");
        };
        if let ValueDialogState::Number(number) = &mut host.dialog {
            number.draft.buf = "123456".into();
        }
        let outcome = DialogOutcome::CommitNumber("123456".into());
        let attempts = app.configuration_admission.attempts;
        app.commit_settings_number_dialog(&mut host, &outcome)
            .unwrap();
        assert!(host.is_saving());
        assert!(app
            .commit_settings_number_dialog(&mut host, &outcome)
            .is_err());
        assert_eq!(app.configuration_admission.attempts, attempts + 1);
        app.mode = Mode::ValueDialog(host);
        app.settle_filesystem_for_test();
        let Mode::ValueDialog(host) = &app.mode else {
            panic!("failure retains child");
        };
        assert!(!host.is_saving());
        let ValueDialogState::Number(number) = &host.dialog else {
            panic!("number");
        };
        assert!(number.error.is_some());
        assert!(matches!(
            app.inference_save_state,
            InferenceSaveState::Unsaved
        ));
        std::fs::write(directory.path().join("config.toml"), "").unwrap();
        let Mode::ValueDialog(host) = std::mem::replace(&mut app.mode, Mode::Normal) else {
            panic!("retry");
        };
        app.finish_value_dialog(host, outcome);
        assert!(matches!(&app.mode, Mode::ValueDialog(host) if host.is_saving()));
        app.settle_filesystem_for_test();
        assert!(
            matches!(&app.mode, Mode::Settings(state) if state.tab == crate::app::SettingsTab::Inference)
        );
        assert!(matches!(
            app.inference_save_state,
            InferenceSaveState::Durable
        ));
        assert_eq!(
            crate::config::load(directory.path())
                .unwrap()
                .inference
                .restructure_prompt_token_limit,
            123456
        );
    }
}

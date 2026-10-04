//! Numeric detection settings retain server ownership and durable reply identity.
use crate::app::{App, Mode, SettingsTab};
use crate::value_dialog::{DialogOutcome, NumberDialogState};
use crate::value_dialog_host::{ValueDialogHost, ValueTarget};
use crate::value_settings::SettingsNumber;
use ilium_ipc::{AgentDetectionSettings, AgentDetectionSettingsError, ClientRequest};
use std::sync::Arc;

#[derive(Clone)]
pub struct DetectionNumberPending {
    pub request_id: u64,
    pub desired: AgentDetectionSettings,
    pub original: AgentDetectionSettings,
    pub token: Arc<()>,
}

impl SettingsNumber {
    pub(crate) fn is_detection(self) -> bool {
        matches!(self, Self::WorkingPollSeconds | Self::IdlePollSeconds)
    }
    pub(crate) fn detection_desired(
        self,
        original: &AgentDetectionSettings,
        text: &str,
    ) -> Result<AgentDetectionSettings, String> {
        let mut desired = original.clone();
        match self {
            Self::WorkingPollSeconds => {
                desired.working_poll_seconds =
                    crate::value_config::ScalarNumber::WorkingPollSeconds.parse(text)?
            }
            Self::IdlePollSeconds => {
                desired.idle_poll_seconds =
                    crate::value_config::ScalarNumber::IdlePollSeconds.parse(text)?
            }
            _ => return Err("This is not a detection interval".into()),
        }
        Ok(desired)
    }
}

impl App {
    pub(crate) fn begin_detection_number_dialog(&mut self, field: SettingsNumber) {
        let result = (|| {
            let original = self
                .agent_detection_settings
                .clone()
                .ok_or("Detection settings are still loading")?;
            let (_, text) = field.snapshot(self);
            Ok(ValueDialogHost::number_host(
                ValueTarget::DetectionNumber {
                    field,
                    original,
                    pending: None,
                },
                NumberDialogState::new(field.title(), text),
            ))
        })();
        match result {
            Ok(host) => self.push_modal(Mode::ValueDialog(Box::new(host))),
            Err(error) => self.status_message = Some(error),
        }
    }

    pub(crate) fn commit_detection_number_dialog(
        &mut self,
        host: &mut ValueDialogHost,
        outcome: &DialogOutcome,
    ) -> Result<(), String> {
        if host.is_saving() {
            return Err("The previous server write is still pending".into());
        }
        let ValueTarget::DetectionNumber {
            field,
            original,
            pending,
        } = &mut host.target
        else {
            return Err("This is not a server number dialog".into());
        };
        let Some(Mode::Settings(parent)) = self.modal_stack.last() else {
            return Err("Settings parent changed".into());
        };
        if parent.tab != SettingsTab::AgentMonitoring
            || SettingsNumber::at(self, parent.tab, parent.selected_row) != Some(*field)
            || self.agent_detection_settings.as_ref() != Some(original)
        {
            return Err("Detection settings changed; reopen this dialog".into());
        }
        if self.agent_detection_settings_pending || self.detection_number_pending.is_some() {
            return Err("Another detection settings write is pending".into());
        }
        let DialogOutcome::CommitNumber(text) = outcome else {
            return Err("Enter a poll interval".into());
        };
        let desired = field.detection_desired(original, text)?;
        let request_id = self.next_workspace_request_id();
        if !self.queue_request(ClientRequest::UpdateAgentDetectionSettings {
            request_id: Some(request_id),
            settings: desired.clone(),
        }) {
            return Err("Server settings request was not admitted; retry".into());
        }
        let token = Arc::new(());
        *pending = Some(DetectionNumberPending {
            request_id,
            desired,
            original: original.clone(),
            token: token.clone(),
        });
        self.detection_number_pending = pending.clone();
        self.agent_detection_settings_pending = true;
        self.agent_detection_settings_error = None;
        self.status_message = Some("Saving server detection settings…".into());
        host.begin_save(token);
        Ok(())
    }

    pub(crate) fn receive_detection_number_reply(
        &mut self,
        request_id: u64,
        result: Result<AgentDetectionSettings, AgentDetectionSettingsError>,
    ) {
        let Some(operation) = self.detection_number_pending.as_ref() else {
            return;
        };
        if operation.request_id != request_id {
            return;
        }
        let token = operation.token.clone();
        let current = self.agent_detection_settings.clone();
        let owns_authoritative_snapshot = current.as_ref().is_some_and(|settings| {
            settings == &operation.original || settings == &operation.desired
        });
        self.detection_number_pending = None;
        // Receipt ownership survives Esc or a covered child. Only the server
        // result changes authoritative settings; dialog ownership is separate.
        // A newer broadcast may supersede this accepted write before its direct
        // receipt arrives. Never roll that authoritative snapshot backward.
        if result.is_err() || owns_authoritative_snapshot {
            self.apply_agent_detection_settings_result(result.clone());
        } else {
            self.agent_detection_settings_pending = false;
        }
        let visible_owner = matches!(&self.mode, Mode::ValueDialog(host)
            if matches!(&host.target, ValueTarget::DetectionNumber { pending: Some(pending), .. }
                if pending.request_id == request_id) && host.is_saving());
        if !visible_owner {
            for mode in &mut self.modal_stack {
                if let Mode::ValueDialog(host) = mode {
                    host.finish_save(
                        &token,
                        Err(
                            "Server write finished while this dialog was covered; reopen it".into(),
                        ),
                    );
                }
            }
            self.status_message = Some(match result {
                Ok(_) => "Detection settings saved to disk".into(),
                Err(error) => error.message,
            });
            return;
        }
        let Mode::ValueDialog(host) = &self.mode else {
            return;
        };
        let ValueTarget::DetectionNumber {
            field,
            original,
            pending: Some(pending),
        } = &host.target
        else {
            return;
        };
        if pending.request_id != request_id || !host.is_saving() {
            return;
        }
        let owns_parent = matches!(self.modal_stack.last(), Some(Mode::Settings(parent)) if parent.tab == SettingsTab::AgentMonitoring && SettingsNumber::at(self, parent.tab, parent.selected_row) == Some(*field));
        let token = pending.token.clone();
        let owns_snapshot = current
            .as_ref()
            .is_some_and(|current| current == original || current == &pending.desired);
        let settled = if !owns_parent || !owns_snapshot {
            Err("Detection settings changed; reopen this dialog".into())
        } else {
            match result {
                Ok(settings) if settings == pending.desired => {
                    self.status_message = Some("Detection settings saved to disk".into());
                    Ok(())
                }
                Ok(_) => Err("Server returned a different interval; reopen this dialog".into()),
                Err(error) => Err(error.message),
            }
        };
        self.agent_detection_settings_pending = false;
        self.finish_value_dialog_save(&token, settled);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::SettingsState;
    use crate::value_dialog::ValueDialogState;
    fn fixture(field: SettingsNumber) -> App {
        let mut app = App::new("synthetic-server-number".into(), std::env::temp_dir());
        app.config_dir = None; // Server settings must not require client disk I/O.
        app.agent_detection_settings = Some(AgentDetectionSettings {
            working_poll_seconds: 10,
            idle_poll_seconds: 45,
            custom_signatures: vec![ilium_ipc::CustomAgentSignature {
                name_substring: "synthetic-agent".into(),
                class: ilium_core::AgentClass::Other("synthetic-agent".into()),
            }],
        });
        let row = (0..crate::settings_ui::agent_monitoring_rows(&app).len())
            .find(|row| SettingsNumber::at(&app, SettingsTab::AgentMonitoring, *row) == Some(field))
            .unwrap();
        app.mode = Mode::Settings(SettingsState {
            tab: SettingsTab::AgentMonitoring,
            selected_row: row,
            ..SettingsState::default()
        });
        app.begin_settings_number_dialog(field);
        app
    }
    fn submit(app: &mut App, text: &str) -> (u64, AgentDetectionSettings) {
        let Mode::ValueDialog(host) = std::mem::replace(&mut app.mode, Mode::Normal) else {
            panic!("number child");
        };
        app.finish_value_dialog(host, DialogOutcome::CommitNumber(text.into()));
        let request = app.take_outbound_requests().pop().unwrap();
        let ClientRequest::UpdateAgentDetectionSettings {
            request_id: Some(id),
            settings,
        } = request
        else {
            panic!("correlated server update");
        };
        (id, settings)
    }
    #[test]
    fn working_and_idle_exact_saves_preserve_signatures_wait_for_own_reply_and_ignore_broadcast() {
        for field in [
            SettingsNumber::WorkingPollSeconds,
            SettingsNumber::IdlePollSeconds,
        ] {
            let mut app = fixture(field);
            let original = app.agent_detection_settings.clone().unwrap();
            let (id, desired) = submit(&mut app, "0");
            assert_eq!(desired.custom_signatures, original.custom_signatures);
            assert_eq!(app.agent_detection_settings.as_ref(), Some(&original));
            assert!(matches!(&app.mode, Mode::ValueDialog(host) if host.is_saving()));
            app.receive_detection_number_reply(id + 1, Ok(desired.clone()));
            assert!(matches!(&app.mode, Mode::ValueDialog(host) if host.is_saving()));
            crate::render_cache::apply(
                &mut app,
                ilium_ipc::ServerEvent::AgentDetectionSettingsChanged {
                    request_id: None,
                    result: Ok(desired.clone()),
                },
            );
            assert!(matches!(&app.mode, Mode::ValueDialog(host) if host.is_saving()));
            assert!(app.agent_detection_settings_pending);
            app.settings_update_agent_detection(original.clone());
            assert!(app.take_outbound_requests().is_empty());
            app.receive_detection_number_reply(id, Ok(desired.clone()));
            assert!(matches!(&app.mode, Mode::Settings(_)));
            assert_eq!(app.agent_detection_settings, Some(desired));
        }
    }
    #[test]
    fn server_rejection_retains_draft_retry_uses_new_id_and_old_reply_cannot_finish_it() {
        let mut app = fixture(SettingsNumber::WorkingPollSeconds);
        if let Mode::ValueDialog(host) = &mut app.mode {
            if let ValueDialogState::Number(number) = &mut host.dialog {
                number.draft.buf = "123".into();
            }
        }
        let (old, _) = submit(&mut app, "123");
        app.receive_detection_number_reply(
            old,
            Err(AgentDetectionSettingsError {
                message: "synthetic disk failure".into(),
            }),
        );
        let Mode::ValueDialog(host) = &app.mode else {
            panic!("failure retains child");
        };
        assert!(!host.is_saving());
        let ValueDialogState::Number(number) = &host.dialog else {
            panic!("number");
        };
        assert_eq!(number.draft.buf, "123");
        assert!(number
            .error
            .as_deref()
            .unwrap()
            .contains("synthetic disk failure"));
        let (new, desired) = submit(&mut app, "123");
        assert_ne!(old, new);
        app.receive_detection_number_reply(old, Ok(desired.clone()));
        assert!(matches!(&app.mode, Mode::ValueDialog(host) if host.is_saving()));
        app.receive_detection_number_reply(new, Ok(desired));
        assert!(matches!(app.mode, Mode::Settings(_)));
    }
    #[test]
    fn invalid_stale_or_unadmitted_server_number_does_not_mark_pending() {
        let mut app = fixture(SettingsNumber::IdlePollSeconds);
        let Mode::ValueDialog(mut host) = std::mem::replace(&mut app.mode, Mode::Normal) else {
            panic!("number");
        };
        assert!(app
            .commit_detection_number_dialog(&mut host, &DialogOutcome::CommitNumber("-1".into()))
            .is_err());
        assert!(!app.agent_detection_settings_pending);
        assert!(app.take_outbound_requests().is_empty());
        app.outbound_admission = None;
        assert!(app
            .commit_detection_number_dialog(&mut host, &DialogOutcome::CommitNumber("123".into()))
            .is_err());
        assert!(!host.is_saving());
        assert!(!app.agent_detection_settings_pending);
        app.agent_detection_settings
            .as_mut()
            .unwrap()
            .working_poll_seconds = 99;
        assert!(app
            .commit_detection_number_dialog(&mut host, &DialogOutcome::CommitNumber("123".into()))
            .is_err());
        assert!(app.take_outbound_requests().is_empty());
    }
    #[test]
    fn closed_child_rejection_clears_pending_and_cannot_settle_reopened_child() {
        let mut app = fixture(SettingsNumber::WorkingPollSeconds);
        let original = app.agent_detection_settings.clone();
        let (id, _) = submit(&mut app, "123");
        app.pop_modal();
        app.begin_settings_number_dialog(SettingsNumber::WorkingPollSeconds);
        app.receive_detection_number_reply(
            id,
            Err(AgentDetectionSettingsError {
                message: "synthetic rejected after Esc".into(),
            }),
        );
        assert_eq!(app.agent_detection_settings, original);
        assert!(!app.agent_detection_settings_pending);
        assert!(app.detection_number_pending.is_none());
        assert!(matches!(&app.mode, Mode::ValueDialog(host) if !host.is_saving()));
        let (new, desired) = submit(&mut app, "124");
        app.receive_detection_number_reply(id, Ok(desired.clone()));
        assert!(matches!(&app.mode, Mode::ValueDialog(host) if host.is_saving()));
        app.receive_detection_number_reply(new, Ok(desired));
        assert!(matches!(app.mode, Mode::Settings(_)));
    }
    #[test]
    fn covered_child_receipt_updates_authoritative_state_without_closing_cover() {
        let mut app = fixture(SettingsNumber::IdlePollSeconds);
        let (id, desired) = submit(&mut app, "125");
        app.push_modal(Mode::Normal);
        app.receive_detection_number_reply(id, Ok(desired.clone()));
        assert!(matches!(app.mode, Mode::Normal));
        assert_eq!(app.agent_detection_settings, Some(desired));
        assert!(!app.agent_detection_settings_pending);
        app.pop_modal();
        assert!(matches!(&app.mode, Mode::ValueDialog(host) if !host.is_saving()));
    }
    #[test]
    fn newer_authoritative_broadcast_is_not_rolled_back_by_older_receipt() {
        let mut app = fixture(SettingsNumber::WorkingPollSeconds);
        let (id, desired) = submit(&mut app, "126");
        let mut newer = desired.clone();
        newer.idle_poll_seconds = 98;
        app.apply_agent_detection_settings_result(Ok(newer.clone()));
        app.receive_detection_number_reply(id, Ok(desired));
        assert_eq!(app.agent_detection_settings, Some(newer));
        assert!(!app.agent_detection_settings_pending);
        assert!(app.detection_number_pending.is_none());
        assert!(matches!(&app.mode, Mode::ValueDialog(host) if !host.is_saving()));
    }
}

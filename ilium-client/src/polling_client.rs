//! Client-side polling projection correlation; no provider or repository access.
use crate::app::App;

impl App {
    pub(crate) fn request_polling_snapshot(&mut self) -> bool {
        if self.polling_snapshot_request.is_some() {
            return false;
        }
        let request_id = self.next_workspace_request_id();
        if !self.queue_request(ilium_ipc::ClientRequest::Polling {
            request_id,
            operation: ilium_ipc::polling::PollingOperation::Snapshot,
        }) {
            return false;
        }
        self.polling_snapshot_request = Some(request_id);
        true
    }

    /// The open Polling Detection page follows the leader's projection without
    /// the user reopening Settings. One request is in flight at most; the
    /// request itself changes nothing on screen, only its reply does.
    pub(crate) fn tick_polling_refresh(&mut self, now: std::time::Instant) {
        const INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);
        let showing = matches!(
            &self.mode,
            crate::app::Mode::Settings(state)
                if state.tab == crate::app::SettingsTab::PollingDetection
        );
        if !showing {
            self.polling_snapshot_requested_at = None;
            return;
        }
        if self
            .polling_snapshot_requested_at
            .is_some_and(|at| now.duration_since(at) < INTERVAL)
        {
            return;
        }
        if self.request_polling_snapshot() {
            self.polling_snapshot_requested_at = Some(now);
        }
    }

    pub(crate) fn accepts_polling_snapshot(
        &self,
        request_id: u64,
        snapshot: &ilium_ipc::polling::PollingSnapshot,
    ) -> bool {
        self.polling_snapshot_request == Some(request_id)
            && snapshot.settings.validate().is_ok()
            && self
                .polling_snapshot
                .as_ref()
                .is_none_or(|old| snapshot.revision >= old.revision)
    }

    pub(crate) fn receive_polling_reply(
        &mut self,
        request_id: u64,
        reply: ilium_ipc::polling::PollingReply,
    ) {
        if self.polling_snapshot_request != Some(request_id) {
            return;
        }
        match reply {
            ilium_ipc::polling::PollingReply::Snapshot(snapshot) => {
                if self.accepts_polling_snapshot(request_id, &snapshot) {
                    if self
                        .polling_save
                        .accepts_snapshot(&snapshot.settings, &self.polling_settings)
                    {
                        self.polling_settings = snapshot.settings.clone();
                        self.polling_settings_retention =
                            self.processing_derivation_retention.clone();
                    }
                    self.polling_snapshot = Some(*snapshot);
                }
            }
            ilium_ipc::polling::PollingReply::Rejected(reason) => {
                use ilium_ipc::polling::PollingRejection;
                let message = match reason {
                    PollingRejection::InvalidRequest => {
                        "Polling request is invalid; reopen settings to retry."
                    }
                    PollingRejection::Unauthorized => {
                        "Polling access was refused; reopen settings after coordination recovers."
                    }
                    PollingRejection::Unavailable => {
                        "Polling history is unavailable; reopen settings to retry."
                    }
                    PollingRejection::StaleGeneration => {
                        "Polling agent activity changed; reopen settings to refresh."
                    }
                    PollingRejection::StaleFinding => {
                        "Polling finding changed; reopen settings to refresh."
                    }
                    PollingRejection::StaleCursor => {
                        "Polling history changed; reopen settings to refresh."
                    }
                    PollingRejection::StaleEstimate => {
                        "Polling estimate expired; request a fresh estimate."
                    }
                    PollingRejection::BudgetReached => {
                        "Polling monthly budget reached; review the spending limit."
                    }
                    PollingRejection::KeyMissing => {
                        "Polling needs the saved OpenAI key; manage it in Inference settings."
                    }
                };
                self.status_message = Some(message.into());
                self.status_message_retention = self.processing_derivation_retention.clone();
            }
            _ => {}
        }
        self.polling_snapshot_request = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_core::polling_detection::{DetectorStatus, PollingSettings};
    use ilium_ipc::{
        polling::{PollingReply, PollingSnapshot},
        ClientRequest, ServerEvent,
    };

    fn snapshot(revision: u64) -> PollingSnapshot {
        PollingSnapshot {
            revision,
            status: DetectorStatus::WaitingForActivity,
            settings: PollingSettings {
                event_interval: 7,
                ..Default::default()
            },
            key_configured: true,
            findings: Vec::new(),
            day: None,
            month: None,
            all_time: None,
            coalesced: 4,
            dropped_capacity: 2,
        }
    }

    // Missing entry-point request leaves the visible tab on client defaults.
    #[test]
    fn polling_opening_settings_requests_the_authoritative_projection() {
        let mut app = App::new("synthetic-polling-client".into(), std::env::temp_dir());
        app.action_open_settings();
        let requests = app.take_outbound_requests();
        assert_eq!(
            requests
                .iter()
                .filter(|request| matches!(
                    request,
                    ClientRequest::Polling {
                        operation: ilium_ipc::polling::PollingOperation::Snapshot,
                        ..
                    }
                ))
                .count(),
            1
        );
        assert!(app.polling_snapshot_request.is_some());
    }

    // A rejected request must release admission and explain unavailable data.
    #[test]
    fn polling_rejected_snapshot_preserves_history_and_explains_the_failure() {
        let mut app = App::new("synthetic-polling-client".into(), std::env::temp_dir());
        app.polling_snapshot = Some(snapshot(5));
        app.polling_snapshot_request = Some(42);
        crate::render_cache::apply(
            &mut app,
            ServerEvent::Polling {
                request_id: 42,
                reply: PollingReply::Rejected(ilium_ipc::polling::PollingRejection::Unavailable),
            },
        );
        assert_eq!(app.polling_snapshot.as_ref().unwrap().revision, 5);
        assert_eq!(app.polling_snapshot_request, None);
        assert!(app
            .status_message
            .as_ref()
            .is_some_and(|message| message.to_lowercase().contains("polling")));
        assert!(app.request_polling_snapshot());
    }

    // The credential action must edit the existing key, never change providers.
    #[test]
    fn polling_key_management_preserves_the_selected_inference_provider() {
        use crate::app::{InferenceSettingField, Mode};
        let mut app = App::new("synthetic-polling-key".into(), std::env::temp_dir());
        app.inference_settings.selected_provider = ilium_inference::InferenceProviderKind::Ollama;
        app.settings_adjust_polling_row(crate::polling_settings::PollingRow::ManageKey, 1);
        assert!(matches!(
            app.mode,
            Mode::InferenceSettingPrompt(InferenceSettingField::OpenAiApiKey, _)
        ));
        assert_eq!(
            app.inference_settings.selected_provider,
            ilium_inference::InferenceProviderKind::Ollama
        );
    }

    // A readable zero-attempt ledger is different from unavailable or unpriced history.
    #[test]
    fn polling_empty_accounting_does_not_present_a_fabricated_zero_dollar_bill() {
        let mut app = App::new("synthetic-polling-accounting".into(), std::env::temp_dir());
        let mut projection = snapshot(1);
        projection.all_time = Some(ilium_ipc::polling::PollingAccounting {
            attempts: 0,
            known_cost_nanodollars: 0,
            reserved_nanodollars: 0,
            unknown_attempts: 0,
            input_tokens: 0,
        });
        app.polling_snapshot = Some(projection);
        let rendered = crate::polling_settings::view(&app, 0, 160)
            .into_iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(rendered.contains("No requests yet"));
        assert!(rendered.contains("Today: unavailable"));
        assert!(!rendered.contains("All time: $0"));
    }

    #[test]
    fn polling_snapshot_request_is_coalesced_until_the_correlated_reply() {
        let mut app = App::new("synthetic-polling-client".into(), std::env::temp_dir());
        assert!(app.request_polling_snapshot());
        assert!(!app.request_polling_snapshot());
        let requests = app.take_outbound_requests();
        let [ClientRequest::Polling {
            request_id,
            operation: ilium_ipc::polling::PollingOperation::Snapshot,
        }] = requests.as_slice()
        else {
            panic!("polling snapshot must use one bounded outbound request");
        };
        crate::render_cache::apply(
            &mut app,
            ServerEvent::Polling {
                request_id: *request_id,
                reply: PollingReply::Snapshot(Box::new(snapshot(3))),
            },
        );
        assert!(
            app.request_polling_snapshot(),
            "reply must release the coalescing slot"
        );
    }

    #[test]
    fn polling_correlated_snapshot_reaches_settings_without_fabricating_accounting() {
        let mut app = App::new("synthetic-polling-client".into(), std::env::temp_dir());
        app.polling_snapshot_request = Some(42);
        crate::render_cache::apply(
            &mut app,
            ServerEvent::Polling {
                request_id: 42,
                reply: PollingReply::Snapshot(Box::new(snapshot(3))),
            },
        );
        let received = app
            .polling_snapshot
            .as_ref()
            .expect("server projection must reach the settings view");
        assert_eq!(received.revision, 3);
        assert_eq!(received.coalesced, 4);
        assert!(received.day.is_none());
        assert_eq!(app.polling_settings.event_interval, 7);
        assert_eq!(app.polling_snapshot_request, None);
    }

    #[test]
    fn polling_unmatched_and_stale_snapshots_cannot_replace_current_projection() {
        let mut app = App::new("synthetic-polling-client".into(), std::env::temp_dir());
        app.polling_snapshot = Some(snapshot(5));
        app.polling_snapshot_request = Some(42);
        for (request_id, revision) in [(41, 9), (42, 4)] {
            crate::render_cache::apply(
                &mut app,
                ServerEvent::Polling {
                    request_id,
                    reply: PollingReply::Snapshot(Box::new(snapshot(revision))),
                },
            );
            assert_eq!(app.polling_snapshot.as_ref().unwrap().revision, 5);
        }
    }
    #[test]
    fn polling_numeric_changes_wait_for_autosave_instead_of_writing_each_step() {
        let mut app = App::new("synthetic-polling-debounce".into(), std::env::temp_dir());
        let before = app.configuration_admission.attempts;
        app.settings_adjust_polling_row(crate::polling_settings::PollingRow::Interval, 1);
        app.settings_adjust_polling_row(crate::polling_settings::PollingRow::Interval, 1);
        assert_eq!(app.polling_settings.event_interval, 7);
        assert_eq!(
            app.configuration_admission.attempts, before,
            "numeric steps must coalesce during the 600 ms quiet period"
        );
    }

    #[test]
    fn polling_late_snapshot_updates_accounting_without_overwriting_an_unsaved_draft() {
        let mut app = App::new("synthetic-polling-draft".into(), std::env::temp_dir());
        app.polling_snapshot_request = Some(42);
        app.settings_adjust_polling_row(crate::polling_settings::PollingRow::Interval, 1);
        assert_eq!(app.polling_settings.event_interval, 6);
        crate::render_cache::apply(
            &mut app,
            ServerEvent::Polling {
                request_id: 42,
                reply: PollingReply::Snapshot(Box::new(snapshot(3))),
            },
        );
        assert_eq!(
            app.polling_settings.event_interval, 6,
            "a snapshot requested before editing must preserve the local numeric draft"
        );
        assert_eq!(app.polling_snapshot.as_ref().unwrap().revision, 3);
        assert_eq!(app.polling_snapshot.as_ref().unwrap().coalesced, 4);
    }

    #[test]
    fn polling_status_projection_explains_missing_keys_and_unavailable_history() {
        let mut app = App::new("synthetic-polling-status".into(), std::env::temp_dir());
        for (status, expected) in [
            (DetectorStatus::NoApiKey, "No API key"),
            (DetectorStatus::BudgetReached, "Budget reached"),
            (DetectorStatus::HistoryUnavailable, "History unavailable"),
            (DetectorStatus::WaitingForActivity, "Waiting for activity"),
        ] {
            let mut projection = snapshot(1);
            projection.status = status;
            app.polling_snapshot = Some(projection);
            let rendered = crate::polling_settings::view(&app, 0, 160)
                .into_iter()
                .map(|line| line.to_string())
                .collect::<Vec<_>>()
                .join("\n");
            assert!(
                rendered.contains(expected),
                "status must tell the user what prevents polling detection: {rendered}"
            );
        }
    }
}

//! Polling settings drafts and ordered durability acknowledgements.
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::app::{App, InferenceSettingField};
use crate::filesystem::configuration::{
    ConfigurationChange, ConfigurationSaved, ConfigurationWrite,
};
use crate::filesystem::configurations::ConfigurationIntent;
use crate::filesystem::ordered::WriteCompletion;
use crate::polling_settings::PollingRow;
use ilium_core::polling_detection::PollingSettings;
use ilium_execution::JobOutcome;

#[derive(Default, PartialEq, Eq)]
pub(crate) enum SaveStatus {
    #[default]
    Ready,
    Waiting,
    Saving,
    Saved,
    Invalid,
    Failed,
}

#[derive(Default)]
pub(crate) struct PollingSave {
    draft: Option<Arc<()>>,
    pub deadline: Option<Instant>,
    status: SaveStatus,
}

impl PollingSave {
    pub fn label(&self) -> &'static str {
        match self.status {
            SaveStatus::Ready => "Settings unchanged",
            SaveStatus::Waiting => "Unsaved changes · saving shortly",
            SaveStatus::Saving => "Saving settings…",
            SaveStatus::Saved => "Settings saved",
            SaveStatus::Invalid => "Invalid settings · change not saved",
            SaveStatus::Failed => "Save failed · draft retained; change a value to retry",
        }
    }

    /// A successful local receipt is not proof that the server reloaded it.
    /// Keep the draft until the server acknowledges the same durable values.
    pub fn accepts_snapshot(
        &mut self,
        incoming: &PollingSettings,
        desired: &PollingSettings,
    ) -> bool {
        if self.draft.is_none() {
            return true;
        }
        if self.status == SaveStatus::Saved && incoming == desired {
            self.draft = None;
            return true;
        }
        false
    }
}

impl App {
    pub fn settings_adjust_polling_row(&mut self, row: PollingRow, direction: i32) {
        if row == PollingRow::ManageKey {
            self.settings_open_inference_field(InferenceSettingField::OpenAiApiKey);
            return;
        }
        let mut settings = self.polling_settings.clone();
        row.adjust(&mut settings, direction);
        if settings.validate().is_err() {
            self.polling_save.status = SaveStatus::Invalid;
            return;
        }
        if settings == self.polling_settings {
            return;
        }
        self.polling_settings = settings;
        self.polling_save.draft = Some(Arc::new(()));
        self.polling_save.status = SaveStatus::Waiting;
        self.polling_save.deadline = Some(Instant::now() + Duration::from_millis(600));
        if matches!(
            row,
            PollingRow::Enabled
                | PollingRow::Filter
                | PollingRow::Scope
                | PollingRow::UnlimitedBudget
        ) {
            self.flush_polling_save();
        }
    }

    pub(crate) fn tick_polling_autosave(&mut self, now: Instant) -> bool {
        if self
            .polling_save
            .deadline
            .is_none_or(|deadline| now < deadline)
        {
            return false;
        }
        self.flush_polling_save()
    }

    pub(crate) fn flush_polling_save(&mut self) -> bool {
        if self.polling_save.deadline.take().is_none() {
            return false;
        }
        let Some(operation) = self.polling_save.draft.clone() else {
            return false;
        };
        self.polling_save.status = SaveStatus::Saving;
        let result = match self.config_dir.clone() {
            Some(directory) => self.enqueue_configuration(
                directory,
                ConfigurationChange::PollingDetection(self.polling_settings.clone()),
                ConfigurationIntent::Polling {
                    operation: operation.clone(),
                },
            ),
            None => {
                Err("Configuration directory unavailable; polling draft remains unsaved".into())
            }
        };
        match result {
            Ok(()) => true,
            Err(error) => {
                self.finish_polling_save(&operation, Err(error));
                false
            }
        }
    }

    fn finish_polling_save(&mut self, operation: &Arc<()>, result: Result<(), String>) {
        if !self
            .polling_save
            .draft
            .as_ref()
            .is_some_and(|draft| Arc::ptr_eq(draft, operation))
        {
            return;
        }
        self.polling_save.status = if result.is_ok() {
            SaveStatus::Saved
        } else {
            SaveStatus::Failed
        };
        self.status_message = Some(match result {
            Ok(()) => "Polling settings saved to disk".into(),
            Err(error) => format!("Polling settings remain unsaved: {error}"),
        });
    }

    pub(crate) fn collect_polling_configuration(
        &mut self,
        operation: &Arc<()>,
        completion: WriteCompletion<ConfigurationWrite>,
    ) {
        let result = match completion {
            WriteCompletion::Outcome { outcome, .. } => {
                let mut result =
                    Err("Polling write did not complete; publication is unconfirmed".into());
                let _retained = outcome.map(|outcome| {
                    result = match outcome {
                        JobOutcome::Finished(Ok(ConfigurationSaved::Plain)) => Ok(()),
                        JobOutcome::Finished(Err(failure)) => Err(failure.message),
                        _ => {
                            Err("Polling write did not complete; publication is unconfirmed".into())
                        }
                    };
                });
                result
            }
            WriteCompletion::Rejected { rejection, .. } => {
                Err(format!("Polling save refused: {:?}", rejection.reason))
            }
            WriteCompletion::Lost { .. } => {
                Err("Polling write receipt lost; publication is unconfirmed".into())
            }
        };
        self.finish_polling_save(operation, result);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn polling_autosave_coalesces_steps_and_acknowledges_actual_disk_publication() {
        let directory = tempfile::tempdir().unwrap();
        let mut app = App::new(
            "synthetic-polling-write".into(),
            directory.path().to_owned(),
        );
        app.config_dir = Some(directory.path().to_owned());
        app.settings_adjust_polling_row(PollingRow::Interval, 1);
        app.settings_adjust_polling_row(PollingRow::Interval, 1);
        let deadline = app.polling_save.deadline.unwrap();
        assert!(!app.tick_polling_autosave(deadline - Duration::from_millis(1)));
        assert!(!directory.path().join("config.toml").exists());
        assert!(app.tick_polling_autosave(deadline));
        assert_eq!(app.configuration_admission.attempts, 1);
        assert!(app.polling_save.status == SaveStatus::Saving);
        app.settle_filesystem_for_test();
        assert_eq!(
            crate::config::load(directory.path())
                .unwrap()
                .polling_detection
                .event_interval,
            7
        );
        assert!(app.polling_save.status == SaveStatus::Saved);
        assert!(!app.tick_polling_autosave(deadline + Duration::from_secs(1)));
    }

    #[test]
    fn polling_old_write_receipt_cannot_mark_a_newer_draft_saved() {
        let directory = tempfile::tempdir().unwrap();
        let mut app = App::new(
            "synthetic-polling-receipt".into(),
            directory.path().to_owned(),
        );
        app.config_dir = Some(directory.path().to_owned());
        app.settings_adjust_polling_row(PollingRow::Interval, 1);
        assert!(app.flush_polling_save());
        app.settings_adjust_polling_row(PollingRow::Interval, 1);
        app.settle_filesystem_for_test();
        assert!(app.polling_save.status == SaveStatus::Waiting);
        assert_eq!(app.polling_settings.event_interval, 7);
        assert_eq!(
            crate::config::load(directory.path())
                .unwrap()
                .polling_detection
                .event_interval,
            6
        );
        assert!(app.flush_polling_save());
        app.settle_filesystem_for_test();
        assert!(app.polling_save.status == SaveStatus::Saved);
        assert_eq!(
            crate::config::load(directory.path())
                .unwrap()
                .polling_detection
                .event_interval,
            7
        );
        let settings = app.polling_settings.clone();
        assert!(app.polling_save.accepts_snapshot(&settings, &settings));
        assert!(app.polling_save.draft.is_none());
    }

    #[test]
    fn polling_failed_save_keeps_the_draft_and_does_not_report_saved() {
        let directory = tempfile::tempdir().unwrap();
        let obstruction = directory.path().join("not-a-directory");
        std::fs::write(&obstruction, "synthetic obstruction").unwrap();
        let mut app = App::new(
            "synthetic-polling-failure".into(),
            directory.path().to_owned(),
        );
        app.config_dir = Some(obstruction);
        app.settings_adjust_polling_row(PollingRow::Enabled, 1);
        app.settle_filesystem_for_test();
        assert!(app.polling_settings.enabled);
        assert!(app.polling_save.status == SaveStatus::Failed);
        assert!(app.polling_save.draft.is_some());
        assert!(app.polling_save.label().contains("Save failed"));
        assert!(!app
            .polling_save
            .accepts_snapshot(&PollingSettings::default(), &app.polling_settings));
    }
}

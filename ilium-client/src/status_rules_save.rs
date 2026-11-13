//! Debounced persistence of the global custom status rules.
//!
//! Every validated edit replaces `App::custom_status_rules` at once (tree and
//! previews follow immediately) and only arms a deadline here. The write
//! itself goes through the ordered configuration writer
//! (`ConfigurationChange::StatusRules`) when the deadline passes, as soon as
//! the rules editor is no longer open, and on quit (`lib.rs` flushes before
//! draining the filesystem). The write always carries the rules in memory at
//! that moment, so a burst of edits costs one write and a retry can never
//! persist an older set. A failed write keeps the in-memory edits, shows the
//! failure in the editor header and is retried until a write succeeds.

use std::time::{Duration, Instant};

use crate::app::App;
use crate::filesystem::configuration::ConfigurationChange;
use crate::filesystem::configurations::ConfigurationIntent;

/// Quiet time after the last edit before the rules are written.
pub const STATUS_RULES_SAVE_DEBOUNCE: Duration = Duration::from_millis(300);

/// Wait before writing again after a failed write.
pub const STATUS_RULES_SAVE_RETRY: Duration = Duration::from_secs(5);

/// Label of the configuration intent; the writer's completion handler uses it
/// to route success and failure back here.
pub(crate) const STATUS_RULES_SAVE_LABEL: &str = "custom status rules";

/// Shown in the editor header when there is no configuration folder: edits
/// apply, but nothing can write them to disk.
pub(crate) const STATUS_RULES_SESSION_ONLY: &str =
    "No configuration folder: these rules apply until Ilium quits";

impl App {
    /// Arm (or push back) the debounced write after an in-memory edit.
    pub(crate) fn schedule_custom_status_rules_save(&mut self) {
        self.custom_status_rules_save_deadline = Some(Instant::now() + STATUS_RULES_SAVE_DEBOUNCE);
    }

    /// Write the rules once the deadline passed, or at once when the editor
    /// was closed while a write was pending. Returns whether a write was
    /// queued.
    pub(crate) fn tick_custom_status_rules_save(&mut self, now: Instant) -> bool {
        let Some(deadline) = self.custom_status_rules_save_deadline else {
            return false;
        };
        // A retry after a failure waits for its own deadline even with the
        // editor closed, so a broken file is not hammered on every tick.
        let leaving_editor = self.custom_status_rules_editor.is_none()
            && self.custom_status_rules_write_error.is_none();
        if now < deadline && !leaving_editor {
            return false;
        }
        self.flush_custom_status_rules_save()
    }

    /// Queue the pending write now, whatever its deadline (leaving the
    /// editor, quitting). Returns whether a write was queued.
    pub(crate) fn flush_custom_status_rules_save(&mut self) -> bool {
        if self.custom_status_rules_save_deadline.take().is_none() {
            return false;
        }
        let Some(directory) = self.config_dir.clone() else {
            // Without a configuration directory there is nowhere to write;
            // the edit stays in memory, as `settings_save_custom_status_rules`
            // already reported.
            return false;
        };
        let queued = self.enqueue_configuration(
            directory,
            ConfigurationChange::StatusRules(Box::new(self.custom_status_rules.clone())),
            ConfigurationIntent::Plain {
                label: STATUS_RULES_SAVE_LABEL,
                success: None,
            },
        );
        match queued {
            Ok(()) => true,
            Err(error) => {
                self.custom_status_rules_write_failed(&error);
                false
            }
        }
    }

    /// A queued write (or its admission) failed: keep the in-memory rules,
    /// say so in the editor header and try again later.
    pub(crate) fn custom_status_rules_write_failed(&mut self, message: &str) {
        let report = format!("Could not write config.toml, retrying: {message}");
        if let Some(editor) = self.custom_status_rules_editor.as_mut() {
            editor.save_error = Some(report.clone());
        }
        self.custom_status_rules_write_error = Some(report);
        // A newer edit's own (earlier) deadline wins; it writes the same
        // current rules.
        let retry_at = Instant::now() + STATUS_RULES_SAVE_RETRY;
        self.custom_status_rules_save_deadline = Some(
            self.custom_status_rules_save_deadline
                .map_or(retry_at, |pending| pending.min(retry_at)),
        );
    }

    /// A queued write reached the disk: clear a previous failure report and
    /// the loader's repair notes, which no longer describe the file.
    pub(crate) fn custom_status_rules_write_succeeded(&mut self) {
        if let Some(report) = self.custom_status_rules_write_error.take() {
            if let Some(editor) = self.custom_status_rules_editor.as_mut() {
                if editor.save_error.as_deref() == Some(report.as_str()) {
                    editor.save_error = None;
                }
            }
        }
        self.custom_status_rules_load_issues.clear();
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use ilium_core::status_rule_expression::default_rules;

    use crate::app::App;

    fn app_with_config_dir() -> (tempfile::TempDir, App) {
        let directory = tempfile::tempdir().unwrap();
        let mut app = App::new("status-rules-save".into(), directory.path().to_owned());
        app.config_dir = Some(directory.path().to_owned());
        (directory, app)
    }

    fn saved_names(directory: &tempfile::TempDir) -> Option<Vec<String>> {
        let text = std::fs::read_to_string(directory.path().join("config.toml")).ok()?;
        text.contains("[status_rules]").then(|| {
            crate::config::load_status_rules(directory.path())
                .unwrap()
                .rules
                .rules
                .into_iter()
                .map(|rule| rule.name)
                .collect()
        })
    }

    fn renamed(name: &str) -> ilium_core::status_rule_expression::StatusRules {
        let mut rules = default_rules();
        rules.rules[0].name = name.to_owned();
        rules
    }

    fn open_editor(app: &mut App) {
        app.custom_status_rules_editor = Some(crate::status_rules_editor::EditorState::new(
            &app.custom_status_rules,
        ));
    }

    #[test]
    fn edits_apply_at_once_and_a_burst_is_written_once_after_the_debounce() {
        let (directory, mut app) = app_with_config_dir();
        open_editor(&mut app);
        app.settings_save_custom_status_rules(renamed("First"))
            .unwrap();
        app.settings_save_custom_status_rules(renamed("Second"))
            .unwrap();
        assert_eq!(app.custom_status_rules.rules[0].name, "Second");
        let deadline = app
            .custom_status_rules_save_deadline
            .expect("a write is pending");
        app.settle_filesystem_for_test();
        assert_eq!(
            saved_names(&directory),
            None,
            "nothing written before the debounce"
        );
        assert!(!app.tick_custom_status_rules_save(deadline - Duration::from_millis(1)));
        assert!(app.tick_custom_status_rules_save(deadline));
        assert!(app.custom_status_rules_save_deadline.is_none());
        assert!(!app.tick_custom_status_rules_save(deadline + Duration::from_secs(1)));
        app.settle_filesystem_for_test();
        assert_eq!(saved_names(&directory).unwrap()[0], "Second");
    }

    #[test]
    fn closing_the_editor_or_quitting_writes_without_waiting() {
        let (directory, mut app) = app_with_config_dir();
        open_editor(&mut app);
        app.settings_save_custom_status_rules(renamed("Closed"))
            .unwrap();
        app.custom_status_rules_editor = None;
        assert!(app.tick_custom_status_rules_save(Instant::now()));
        app.settle_filesystem_for_test();
        assert_eq!(saved_names(&directory).unwrap()[0], "Closed");

        open_editor(&mut app);
        app.settings_save_custom_status_rules(renamed("Quit"))
            .unwrap();
        assert!(app.flush_custom_status_rules_save());
        assert!(
            !app.flush_custom_status_rules_save(),
            "nothing left to write"
        );
        app.settle_filesystem_for_test();
        assert_eq!(saved_names(&directory).unwrap()[0], "Quit");
    }

    #[test]
    fn a_failed_write_keeps_the_edit_reports_it_and_retries_until_it_succeeds() {
        let (directory, mut app) = app_with_config_dir();
        app.custom_status_rules_load_issues = vec![
            ilium_core::status_rule_expression::ValidationIssue::warning(
                ilium_core::status_rule_expression::IssueKind::InvalidIdentifier,
                None,
                "repaired",
            ),
        ];
        // A directory where the file belongs makes every read and write fail.
        let config = directory.path().join("config.toml");
        std::fs::create_dir(&config).unwrap();
        open_editor(&mut app);
        app.settings_save_custom_status_rules(renamed("Kept"))
            .unwrap();
        assert!(app.flush_custom_status_rules_save());
        app.settle_filesystem_for_test();
        assert_eq!(app.custom_status_rules.rules[0].name, "Kept");
        let report = app
            .custom_status_rules_write_error
            .clone()
            .expect("the failure is recorded");
        assert!(
            report.starts_with("Could not write config.toml, retrying"),
            "{report}"
        );
        assert_eq!(
            app.custom_status_rules_editor
                .as_ref()
                .unwrap()
                .save_error
                .as_deref(),
            Some(report.as_str())
        );
        assert_eq!(
            app.custom_status_rules_load_issues.len(),
            1,
            "file not rewritten yet"
        );
        let retry = app
            .custom_status_rules_save_deadline
            .expect("a retry is armed");
        assert!(retry > Instant::now() + Duration::from_secs(1));
        // Closing the editor does not turn the retry into a busy loop.
        app.custom_status_rules_editor = None;
        assert!(!app.tick_custom_status_rules_save(Instant::now()));

        std::fs::remove_dir(&config).unwrap();
        open_editor(&mut app);
        app.custom_status_rules_editor.as_mut().unwrap().save_error = Some(report);
        assert!(app.tick_custom_status_rules_save(retry));
        app.settle_filesystem_for_test();
        assert_eq!(saved_names(&directory).unwrap()[0], "Kept");
        assert!(app.custom_status_rules_write_error.is_none());
        assert!(app
            .custom_status_rules_editor
            .as_ref()
            .unwrap()
            .save_error
            .is_none());
        assert!(app.custom_status_rules_load_issues.is_empty());
    }

    #[test]
    fn a_rejected_edit_schedules_nothing() {
        let (_directory, mut app) = app_with_config_dir();
        let mut broken = default_rules();
        broken.rules[1].id = broken.rules[0].id.clone();
        assert!(app.settings_save_custom_status_rules(broken).is_err());
        assert!(app.custom_status_rules_save_deadline.is_none());
        assert_eq!(app.custom_status_rules, default_rules());
    }
}

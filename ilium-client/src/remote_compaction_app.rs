//! App-side handling of the Remote compaction settings: applying a row
//! interaction, the debounced save of numeric rows and the privacy-banner
//! dismissal. Pure value logic lives in [`crate::remote_compaction_settings`].

use std::time::{Duration, Instant};

use crate::app::App;
use crate::filesystem::configuration::ConfigurationChange;
use crate::instruction_settings::InstructionField;
use crate::remote_compaction_settings::{RemoteCompactionRow, RemoteCompactionSettings};

/// Numeric rows save this long after the last change (repo auto-save rule).
pub const REMOTE_COMPACTION_SAVE_DEBOUNCE: Duration = Duration::from_millis(600);

impl App {
    /// Installs the settings loaded from `config.toml` at startup.
    pub fn apply_remote_compaction_settings(&mut self, settings: RemoteCompactionSettings) {
        self.remote_compaction_settings = settings;
        self.remote_compaction_save_deadline = None;
    }

    /// Applies one interaction on a tab row (`direction` is `-1`, `0` for
    /// activate, or `1`). Toggles, selects and the banner close button save
    /// at once; numeric steppers save after the debounce. The prompt row opens
    /// the multi-line editor instead.
    pub fn settings_adjust_remote_compaction_row(
        &mut self,
        row: RemoteCompactionRow,
        direction: i32,
    ) {
        if row == RemoteCompactionRow::CustomPrompt {
            if direction == 0 {
                self.settings_open_instruction(InstructionField::CompactionPrompt);
            }
            return;
        }
        let mut settings = self.remote_compaction_settings.clone();
        if !settings.adjust(row, direction) {
            return;
        }
        self.remote_compaction_settings = settings;
        if row.is_debounced() {
            self.remote_compaction_save_deadline =
                Some(Instant::now() + REMOTE_COMPACTION_SAVE_DEBOUNCE);
        } else {
            self.persist_remote_compaction_settings();
        }
    }

    /// Clears the custom prompt (`Delete` on that row).
    pub fn settings_clear_remote_compaction_prompt(&mut self) {
        self.settings_commit_remote_compaction_prompt(String::new());
    }

    /// Stores the prompt written in the multi-line editor.
    pub(crate) fn settings_commit_remote_compaction_prompt(&mut self, value: String) {
        if self.remote_compaction_settings.set_custom_prompt(value) {
            self.persist_remote_compaction_settings();
        }
    }

    /// Closes the privacy banner for good, wherever it was drawn. Persists at
    /// once, so a restart never brings it back.
    pub fn dismiss_remote_compaction_privacy_banner(&mut self) {
        if self.remote_compaction_settings.dismiss_privacy_banner() {
            self.persist_remote_compaction_settings();
        }
    }

    /// Queues the whole table for saving and cancels any pending debounce,
    /// since the snapshot already holds every change.
    pub(crate) fn persist_remote_compaction_settings(&mut self) {
        self.remote_compaction_save_deadline = None;
        self.persist_configuration(
            ConfigurationChange::RemoteCompaction(self.remote_compaction_settings.clone()),
            "remote compaction settings",
        );
    }

    /// Fires the debounced numeric save once its deadline passes. Returns
    /// whether a save was queued.
    pub(crate) fn tick_remote_compaction_save(&mut self, now: Instant) -> bool {
        let Some(deadline) = self.remote_compaction_save_deadline else {
            return false;
        };
        if now < deadline {
            return false;
        }
        self.persist_remote_compaction_settings();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_remote_compaction::Technique;

    fn app_with_config_dir() -> (tempfile::TempDir, App) {
        let directory = tempfile::tempdir().unwrap();
        let mut app = App::new("remote-compaction-app".into(), directory.path().to_owned());
        app.config_dir = Some(directory.path().to_owned());
        (directory, app)
    }

    fn saved(directory: &tempfile::TempDir) -> RemoteCompactionSettings {
        crate::config::load(directory.path())
            .unwrap()
            .remote_compaction
    }

    #[test]
    fn toggles_and_selects_persist_immediately() {
        let (directory, mut app) = app_with_config_dir();
        app.settings_adjust_remote_compaction_row(RemoteCompactionRow::Enabled, 0);
        app.settings_adjust_remote_compaction_row(
            RemoteCompactionRow::Technique(
                crate::remote_compaction_settings::TechniqueTarget::Codex,
            ),
            1,
        );
        assert!(app.remote_compaction_save_deadline.is_none());
        app.settle_filesystem_for_test();
        let loaded = saved(&directory);
        assert!(loaded.enabled);
        assert_ne!(loaded.codex_technique, Technique::Codex);
        assert_eq!(loaded, app.remote_compaction_settings);
    }

    #[test]
    fn numeric_rows_wait_for_the_debounce_then_save_once() {
        let (directory, mut app) = app_with_config_dir();
        app.settings_adjust_remote_compaction_row(RemoteCompactionRow::Threshold, 1);
        app.settings_adjust_remote_compaction_row(RemoteCompactionRow::Threshold, 1);
        assert_eq!(app.remote_compaction_settings.threshold_percent, 67);
        let deadline = app
            .remote_compaction_save_deadline
            .expect("a save is pending");
        app.settle_filesystem_for_test();
        assert_eq!(saved(&directory).threshold_percent, 65, "nothing saved yet");
        assert!(!app.tick_remote_compaction_save(deadline - Duration::from_millis(1)));
        assert!(app.tick_remote_compaction_save(deadline));
        assert!(app.remote_compaction_save_deadline.is_none());
        assert!(!app.tick_remote_compaction_save(deadline + Duration::from_secs(1)));
        app.settle_filesystem_for_test();
        assert_eq!(saved(&directory).threshold_percent, 67);
    }

    #[test]
    fn an_immediate_save_carries_and_cancels_a_pending_numeric_change() {
        let (directory, mut app) = app_with_config_dir();
        app.settings_adjust_remote_compaction_row(RemoteCompactionRow::KeepBackups, 1);
        assert!(app.remote_compaction_save_deadline.is_some());
        app.settings_adjust_remote_compaction_row(RemoteCompactionRow::Automatic, 0);
        assert!(app.remote_compaction_save_deadline.is_none());
        app.settle_filesystem_for_test();
        let loaded = saved(&directory);
        assert_eq!(loaded.keep_backups, 4);
        assert!(loaded.automatic);
    }

    #[test]
    fn dismissing_the_banner_persists_forever_and_only_once() {
        let (directory, mut app) = app_with_config_dir();
        assert!(!saved(&directory).privacy_banner_dismissed);
        app.dismiss_remote_compaction_privacy_banner();
        app.settle_filesystem_for_test();
        assert!(saved(&directory).privacy_banner_dismissed);
        let attempts = app.configuration_admission.attempts;
        app.dismiss_remote_compaction_privacy_banner();
        assert_eq!(
            app.configuration_admission.attempts, attempts,
            "no second save"
        );
    }

    #[test]
    fn the_custom_prompt_edits_persist_and_clear() {
        let (directory, mut app) = app_with_config_dir();
        app.settings_commit_remote_compaction_prompt("Keep API names.".into());
        app.settle_filesystem_for_test();
        assert_eq!(saved(&directory).custom_prompt, "Keep API names.");
        app.settings_clear_remote_compaction_prompt();
        app.settle_filesystem_for_test();
        assert_eq!(saved(&directory).custom_prompt, "");
    }

    #[test]
    fn the_configuration_command_is_byte_accounted_even_at_the_largest_prompt() {
        use crate::remote_compaction_settings::CUSTOM_PROMPT_MAX_CHARS;

        let mut settings = RemoteCompactionSettings::default();
        let mut prompt = String::with_capacity(2 * 1024 * 1024);
        prompt.push_str(&"\u{1}".repeat(CUSTOM_PROMPT_MAX_CHARS));
        settings.set_custom_prompt(prompt);
        assert!(settings.custom_prompt.len() < CUSTOM_PROMPT_MAX_CHARS * 6);
        let normalized = ConfigurationChange::RemoteCompaction(settings.clone())
            .normalize()
            .expect("the bounded prompt fits the configuration transport");
        let ConfigurationChange::RemoteCompaction(value) = &normalized else {
            panic!("wrong command");
        };
        assert_eq!(value.custom_prompt, settings.custom_prompt);
        assert!(
            value.custom_prompt.capacity() < 1024 * 1024,
            "spare capacity is dropped"
        );
        let bytes = normalized.checked_bytes().expect("within the byte limit");
        assert!(bytes >= value.custom_prompt.len());
        assert!(bytes <= 1024 * 1024 + std::mem::size_of::<ConfigurationChange>());

        let small = ConfigurationChange::RemoteCompaction(RemoteCompactionSettings::default());
        assert!(small.checked_bytes().unwrap() < bytes);
    }

    fn settings_app() -> (tempfile::TempDir, App) {
        let (directory, mut app) = app_with_config_dir();
        app.set_screen_area(ratatui::layout::Rect::new(0, 0, 130, 60));
        app.mode = crate::app::Mode::Settings(crate::app::SettingsState {
            tab: crate::app::SettingsTab::RemoteCompaction,
            ..crate::app::SettingsState::default()
        });
        (directory, app)
    }

    fn press(app: &mut App, code: crossterm::event::KeyCode) {
        crate::keys::handle_event(
            app,
            crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                code,
                crossterm::event::KeyModifiers::NONE,
            )),
        );
    }

    fn selected_row(app: &App) -> usize {
        match &app.mode {
            crate::app::Mode::Settings(state) => state.selected_row,
            _ => panic!("settings should stay open"),
        }
    }

    #[test]
    fn keyboard_closes_the_banner_for_good_and_ignores_stray_arrows() {
        use crossterm::event::KeyCode;
        for dismiss in [
            KeyCode::Char('x'),
            KeyCode::Delete,
            KeyCode::Enter,
            KeyCode::Char(' '),
        ] {
            let (directory, mut app) = settings_app();
            assert_eq!(
                crate::remote_compaction_settings_ui::rows(&app).first(),
                Some(&RemoteCompactionRow::PrivacyBanner)
            );
            press(&mut app, KeyCode::Right);
            assert!(
                !app.remote_compaction_settings.privacy_banner_dismissed,
                "arrows never close it"
            );
            press(&mut app, dismiss);
            assert!(
                app.remote_compaction_settings.privacy_banner_dismissed,
                "{dismiss:?}"
            );
            app.settle_filesystem_for_test();
            assert!(
                saved(&directory).privacy_banner_dismissed,
                "{dismiss:?} persisted"
            );
            assert_eq!(
                crate::remote_compaction_settings_ui::rows(&app).first(),
                Some(&RemoteCompactionRow::Enabled)
            );
            assert_eq!(selected_row(&app), 0);
        }
    }

    #[test]
    fn keyboard_walks_the_rows_toggles_and_cycles_each_agent_technique() {
        use crate::remote_compaction_settings::TechniqueTarget;
        use crossterm::event::KeyCode;
        let (directory, mut app) = settings_app();
        press(&mut app, KeyCode::Char('x'));
        let rows = crate::remote_compaction_settings_ui::rows(&app);
        let index_of = |row: RemoteCompactionRow| rows.iter().position(|r| *r == row).unwrap();

        press(&mut app, KeyCode::Char(' '));
        assert!(app.remote_compaction_settings.enabled);
        press(&mut app, KeyCode::Char(' '));
        assert!(!app.remote_compaction_settings.enabled);

        for target in TechniqueTarget::ALL {
            let row = RemoteCompactionRow::Technique(target);
            while selected_row(&app) < index_of(row) {
                press(&mut app, KeyCode::Down);
            }
            let start = app.remote_compaction_settings.technique(target);
            press(&mut app, KeyCode::Right);
            let next = app.remote_compaction_settings.technique(target);
            assert_ne!(next, start, "{target:?}");
            press(&mut app, KeyCode::Left);
            assert_eq!(app.remote_compaction_settings.technique(target), start);
            press(&mut app, KeyCode::Enter);
            let crate::app::Mode::ValueDialog(host) = &app.mode else {
                panic!("Enter opens the technique catalogue");
            };
            let crate::value_dialog::ValueDialogState::Choice(choice) = &host.dialog else {
                panic!("technique choice dialog");
            };
            assert_eq!(choice.selected_id.as_deref(), Some(start.id()));
            let current = choice
                .options()
                .iter()
                .position(|option| option.id == start.id())
                .unwrap();
            let wanted = choice
                .options()
                .iter()
                .position(|option| option.id == next.id())
                .unwrap();
            for _ in 0..current.abs_diff(wanted) {
                press(
                    &mut app,
                    if wanted > current {
                        KeyCode::Down
                    } else {
                        KeyCode::Up
                    },
                );
            }
            press(&mut app, KeyCode::Enter);
            app.settle_filesystem_for_test();
            assert!(matches!(app.mode, crate::app::Mode::Settings(_)));
            assert_eq!(app.remote_compaction_settings.technique(target), next);
            assert_eq!(saved(&directory).technique(target), next);
        }

        let threshold = index_of(RemoteCompactionRow::Threshold);
        while selected_row(&app) > threshold {
            press(&mut app, KeyCode::Up);
        }
        press(&mut app, KeyCode::Char('+'));
        press(&mut app, KeyCode::Char('+'));
        press(&mut app, KeyCode::Char('-'));
        assert_eq!(app.remote_compaction_settings.threshold_percent, 66);
        assert!(app.remote_compaction_save_deadline.is_some());
        app.settle_filesystem_for_test();
        app.tick_remote_compaction_save(Instant::now() + Duration::from_secs(1));
        app.settle_filesystem_for_test();
        let loaded = saved(&directory);
        assert_eq!(loaded.threshold_percent, 66);
        assert_eq!(loaded, app.remote_compaction_settings);

        // x on a row that is not the banner changes nothing.
        let before = app.remote_compaction_settings.clone();
        press(&mut app, KeyCode::Char('x'));
        assert_eq!(app.remote_compaction_settings, before);
    }

    #[test]
    fn keyboard_enter_on_the_prompt_row_opens_the_editor_and_delete_clears_the_text() {
        use crossterm::event::KeyCode;
        let (_directory, mut app) = settings_app();
        app.remote_compaction_settings.custom_prompt = "Keep API names.".into();
        let rows = crate::remote_compaction_settings_ui::rows(&app);
        let prompt = rows
            .iter()
            .position(|row| *row == RemoteCompactionRow::CustomPrompt)
            .unwrap();
        for _ in 0..prompt {
            press(&mut app, KeyCode::Down);
        }
        press(&mut app, KeyCode::Delete);
        assert_eq!(app.remote_compaction_settings.custom_prompt, "");
        app.remote_compaction_settings.custom_prompt = "Again".into();
        press(&mut app, KeyCode::Enter);
        assert!(matches!(app.mode, crate::app::Mode::VoicePromptEditor(_)));
    }

    fn click(app: &mut App, column: u16, row: u16) {
        crate::mouse::handle_mouse_event(
            app,
            crossterm::event::MouseEvent {
                kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
                column,
                row,
                modifiers: crossterm::event::KeyModifiers::empty(),
            },
        );
    }

    fn content_area(app: &App) -> ratatui::layout::Rect {
        let crate::app::Mode::Settings(state) = &app.mode else {
            panic!("settings fixture");
        };
        crate::settings_ui::compute_layout_for_mode(app.layout.screen_area, app, state).content_area
    }

    #[test]
    fn mouse_click_on_the_x_closes_the_banner_for_good_and_other_clicks_do_not() {
        let (directory, mut app) = settings_app();
        let content = content_area(&app);
        let view = crate::remote_compaction_settings_ui::view(&app, 0, content.width);
        let banner = view.rows[0];
        assert_eq!(banner.row, RemoteCompactionRow::PrivacyBanner);
        // A click inside the box but off the button only selects the row.
        click(&mut app, content.x + 6, content.y + banner.first_line + 1);
        assert!(!app.remote_compaction_settings.privacy_banner_dismissed);
        assert!(matches!(&app.mode, crate::app::Mode::Settings(state)
            if state.tab == crate::app::SettingsTab::RemoteCompaction && state.selected_row == 0));
        click(
            &mut app,
            content.x + banner.control_x + 1,
            content.y + banner.control_line,
        );
        assert!(app.remote_compaction_settings.privacy_banner_dismissed);
        app.settle_filesystem_for_test();
        assert!(saved(&directory).privacy_banner_dismissed);
        let after = crate::remote_compaction_settings_ui::view(&app, 0, content.width);
        assert_eq!(after.rows[0].row, RemoteCompactionRow::Enabled);
    }

    #[test]
    fn mouse_toggles_steps_and_cycles_technique_rows_from_the_control_halves() {
        use crate::remote_compaction_settings::TechniqueTarget;
        let (directory, mut app) = settings_app();
        app.remote_compaction_settings.privacy_banner_dismissed = true;
        let content = content_area(&app);
        let view = crate::remote_compaction_settings_ui::view(&app, 0, content.width);
        let span = |row: RemoteCompactionRow| *view.rows.iter().find(|s| s.row == row).unwrap();

        let enabled = span(RemoteCompactionRow::Enabled);
        click(&mut app, content.x + 8, content.y + enabled.first_line);
        assert!(app.remote_compaction_settings.enabled);

        let control_geometry = |app: &App, row: RemoteCompactionRow| {
            let crate::app::Mode::Settings(state) = &app.mode else {
                panic!("settings fixture");
            };
            let index = crate::remote_compaction_settings_ui::rows(app)
                .iter()
                .position(|candidate| *candidate == row)
                .unwrap();
            crate::settings_ui::settings_number_control(content_area(app), app, state, index)
                .map(|(_, control)| control.geometry())
                .or_else(|| {
                    crate::settings_ui::settings_choice_control(
                        content_area(app),
                        app,
                        state,
                        index,
                    )
                    .map(|(_, control)| control.geometry())
                })
                .unwrap()
        };
        let threshold = control_geometry(&app, RemoteCompactionRow::Threshold);
        click(&mut app, threshold.next.x, threshold.next.y);
        assert_eq!(app.remote_compaction_settings.threshold_percent, 66);
        for _ in 0..2 {
            let threshold = control_geometry(&app, RemoteCompactionRow::Threshold);
            click(&mut app, threshold.previous.x, threshold.previous.y);
        }
        assert_eq!(app.remote_compaction_settings.threshold_percent, 64);
        // Clicking the description line under a stepper never changes it.
        let threshold = span(RemoteCompactionRow::Threshold);
        click(
            &mut app,
            content.x + threshold.control_x + 4,
            content.y + threshold.control_line + 1,
        );
        assert_eq!(app.remote_compaction_settings.threshold_percent, 64);
        assert!(matches!(app.mode, crate::app::Mode::Settings(_)));

        let claude = control_geometry(
            &app,
            RemoteCompactionRow::Technique(TechniqueTarget::Claude),
        );
        click(&mut app, claude.value.x, claude.value.y);
        assert_eq!(
            app.remote_compaction_settings.claude_technique,
            Technique::Codex
        );
        let prompt = span(RemoteCompactionRow::CustomPrompt);
        click(&mut app, content.x + 8, content.y + prompt.first_line);
        assert!(matches!(app.mode, crate::app::Mode::VoicePromptEditor(_)));

        app.tick_remote_compaction_save(Instant::now() + Duration::from_secs(1));
        app.settle_filesystem_for_test();
        let loaded = saved(&directory);
        assert!(loaded.enabled);
        assert_eq!(loaded.threshold_percent, 64);
        assert_eq!(loaded.claude_technique, Technique::Codex);
    }

    #[test]
    fn numeric_star_accepts_direct_entry_and_persists_remote_compaction_value() {
        use crossterm::event::KeyCode;

        let (directory, mut app) = settings_app();
        app.remote_compaction_settings.privacy_banner_dismissed = true;
        let row = RemoteCompactionRow::Threshold;
        let index = crate::remote_compaction_settings_ui::rows(&app)
            .iter()
            .position(|candidate| *candidate == row)
            .expect("threshold row exists");
        let crate::app::Mode::Settings(state) = &app.mode else {
            panic!("settings fixture");
        };
        let (_, control) =
            crate::settings_ui::settings_number_control(content_area(&app), &app, state, index)
                .expect("threshold uses the shared numeric control");
        let open = control.geometry().open;
        click(&mut app, open.x, open.y);

        let crate::app::Mode::ValueDialog(host) = &app.mode else {
            panic!("star opens direct numeric entry");
        };
        let draft_length = match &host.dialog {
            crate::value_dialog::ValueDialogState::Number(number) => {
                number.draft.buf.chars().count()
            }
            crate::value_dialog::ValueDialogState::Choice(_) => {
                panic!("threshold uses a number dialog")
            }
        };
        for _ in 0..draft_length {
            press(&mut app, KeyCode::Backspace);
        }
        for character in "67".chars() {
            press(&mut app, KeyCode::Char(character));
        }
        press(&mut app, KeyCode::Enter);
        app.settle_filesystem_for_test();

        assert_eq!(app.remote_compaction_settings.threshold_percent, 67);
        assert_eq!(saved(&directory).threshold_percent, 67);
        assert!(matches!(app.mode, crate::app::Mode::Settings(_)));
    }

    #[test]
    fn technique_plus_opens_full_catalog_and_pointer_selection_persists() {
        use crate::remote_compaction_settings::TechniqueTarget;

        let (directory, mut app) = settings_app();
        app.remote_compaction_settings.privacy_banner_dismissed = true;
        let target = TechniqueTarget::Claude;
        let selected = *Technique::ALL
            .last()
            .expect("technique catalog is nonempty");
        let wanted = Technique::ALL[0];
        assert_ne!(selected, wanted);
        *app.remote_compaction_settings.technique_mut(target) = selected;

        let row = RemoteCompactionRow::Technique(target);
        let index = crate::remote_compaction_settings_ui::rows(&app)
            .iter()
            .position(|candidate| *candidate == row)
            .expect("Claude technique row exists");
        let crate::app::Mode::Settings(state) = &app.mode else {
            panic!("settings fixture");
        };
        let (_, control) =
            crate::settings_ui::settings_choice_control(content_area(&app), &app, state, index)
                .expect("technique uses the shared choice control");
        let open = control.geometry().open;
        click(&mut app, open.x, open.y);

        let crate::app::Mode::ValueDialog(host) = &app.mode else {
            panic!("plus opens the complete technique catalog");
        };
        assert!(matches!(
            &host.target,
            crate::value_dialog_host::ValueTarget::SettingsChoice {
                field: crate::value_settings_choice::SettingsChoice::RemoteTechnique(found),
                ..
            } if *found == target
        ));
        let crate::value_dialog::ValueDialogState::Choice(dialog) = &host.dialog else {
            panic!("techniques use a choice dialog");
        };
        assert_eq!(dialog.options().len(), Technique::ALL.len());
        let wanted_index = dialog
            .options()
            .iter()
            .position(|option| option.id == wanted.id())
            .expect("full catalog contains the selected technique");
        let document = crate::value_dialog::dialog_layout(app.layout.screen_area).document;
        click(
            &mut app,
            document.x.saturating_add(2),
            document.y.saturating_add(wanted_index as u16),
        );
        app.settle_filesystem_for_test();

        assert_eq!(app.remote_compaction_settings.technique(target), wanted);
        assert_eq!(saved(&directory).technique(target), wanted);
    }

    #[test]
    fn the_prompt_row_opens_the_editor_on_the_stored_text() {
        let (_directory, mut app) = app_with_config_dir();
        app.remote_compaction_settings.custom_prompt = "Line one\nLine two".into();
        app.settings_adjust_remote_compaction_row(RemoteCompactionRow::CustomPrompt, 0);
        let crate::app::Mode::VoicePromptEditor(editor) = &app.mode else {
            panic!("the prompt row opens the multi-line editor");
        };
        assert_eq!(editor.instruction_field, InstructionField::CompactionPrompt);
        assert_eq!(editor.text(), "Line one\nLine two");
        app.settings_commit_instruction(InstructionField::CompactionPrompt, "Changed".into());
        assert_eq!(app.remote_compaction_settings.custom_prompt, "Changed");
    }
}

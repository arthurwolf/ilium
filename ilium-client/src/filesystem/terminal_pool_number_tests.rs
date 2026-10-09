use crate::app::{App, Mode, SettingsState, SettingsTab, TerminalRow};
use crate::config::{self, NewPaneDirectory, SmartCopyLightKey, TerminalSettings};
use crate::value_dialog::ValueDialogState;
use crate::value_settings::SettingsNumber;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
//
fn number_app() -> (tempfile::TempDir, App) {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("config.toml"),
        "[numeric_fixture]\nkeep = 'untouched'\n",
    )
    .unwrap();
    let settings = TerminalSettings {
        scrollback_budget_mib: 16,
        engine_memory_budget_mib: 4096,
        new_pane_directory: NewPaneDirectory::LastUsed,
        smart_copy_light: false,
        smart_copy_light_key: SmartCopyLightKey::Alt,
    };
    config::save_terminal_settings(directory.path(), &settings).unwrap();
    let mut app = App::new("terminal-pool-number-test".into(), directory.path().into());
    app.config_dir = Some(directory.path().into());
    app.apply_terminal_settings(settings);
    app.settle_filesystem_for_test();
    app.configuration_admission.rejection = None;
    assert_eq!(config::load(directory.path()).unwrap().terminal, settings);
    assert_eq!(app.configuration_files.as_ref().unwrap().pending(), 0);
    (directory, app)
}
//
fn open_number(app: &mut App) {
    let selected_row = TerminalRow::ALL
        .iter()
        .position(|row| *row == TerminalRow::EngineMemoryBudget)
        .unwrap();
    app.mode = Mode::Settings(SettingsState {
        tab: SettingsTab::Terminal,
        selected_row,
        ..SettingsState::default()
    });
    app.begin_settings_number_dialog(SettingsNumber::TerminalEngineMemory);
}
//
fn submit_number(app: &mut App, text: &str) {
    let Mode::ValueDialog(host) = &mut app.mode else {
        panic!("expected number dialog")
    };
    let ValueDialogState::Number(number) = &mut host.dialog else {
        panic!("expected numeric draft")
    };
    number.draft.buf.clear();
    crate::keys::handle_event(app, Event::Paste(text.into()));
    crate::keys::handle_event(
        app,
        Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
    );
}
//
#[test]
fn terminal_pool_number_dialog_collects_before_reporting_completion_and_reloads_exact_values() {
    let (directory, mut app) = number_app();
    for budget_mib in [257, 0, 256, 16384] {
        open_number(&mut app);
        let concurrent = TerminalSettings {
            scrollback_budget_mib: app.terminal_settings.scrollback_budget_mib + 4,
            new_pane_directory: NewPaneDirectory::FocusedTerminal,
            smart_copy_light: !app.terminal_settings.smart_copy_light,
            smart_copy_light_key: SmartCopyLightKey::Shift,
            ..app.terminal_settings
        };
        app.apply_terminal_settings(concurrent);
        let before = (
            app.configuration_admission.attempts,
            app.configuration_admission.accepted,
        );
        submit_number(&mut app, &budget_mib.to_string());
        let expected = TerminalSettings {
            engine_memory_budget_mib: budget_mib,
            ..concurrent
        };
        assert_eq!(app.configuration_admission.attempts, before.0 + 1);
        assert_eq!(app.configuration_admission.accepted, before.1 + 1);
        assert!(app.configuration_admission.rejection.is_none());
        assert_eq!(app.status_message.as_deref(), Some("Saving settings…"));
        assert!(matches!(&app.mode, Mode::ValueDialog(host) if host.is_saving()));
        assert!(app.configuration_files.as_ref().unwrap().pending() > 0);
        assert_eq!(app.terminal_settings, expected);
        app.settle_filesystem_for_test();
        assert!(matches!(&app.mode, Mode::Settings(state) if state.tab == SettingsTab::Terminal));
        assert_eq!(app.configuration_files.as_ref().unwrap().pending(), 0);
        assert_eq!(config::load(directory.path()).unwrap().terminal, expected);
        let document: toml::Value =
            toml::from_str(&std::fs::read_to_string(directory.path().join("config.toml")).unwrap())
                .unwrap();
        assert_eq!(
            document["numeric_fixture"]["keep"].as_str(),
            Some("untouched")
        );
    }
}
//
#[test]
fn terminal_pool_number_dialog_rejects_invalid_drafts_without_runtime_or_disk_changes() {
    for text in ["1", "255", "16385", "-1", "1.5", "4294967296"] {
        let (directory, mut app) = number_app();
        let before_settings = app.terminal_settings;
        let before_bytes = std::fs::read(directory.path().join("config.toml")).unwrap();
        let before = (
            app.configuration_admission.attempts,
            app.configuration_admission.accepted,
        );
        open_number(&mut app);
        submit_number(&mut app, text);
        assert!(
            matches!(&app.mode, Mode::ValueDialog(host) if !host.is_saving()),
            "{text}"
        );
        assert_eq!(app.configuration_admission.attempts, before.0, "{text}");
        assert_eq!(app.configuration_admission.accepted, before.1, "{text}");
        assert_eq!(app.terminal_settings, before_settings, "{text}");
        app.settle_filesystem_for_test();
        assert_eq!(app.configuration_files.as_ref().unwrap().pending(), 0);
        assert_eq!(
            std::fs::read(directory.path().join("config.toml")).unwrap(),
            before_bytes,
            "{text}"
        );
        assert_eq!(
            config::load(directory.path()).unwrap().terminal,
            before_settings
        );
    }
}
//
#[test]
fn terminal_pool_number_dialog_queue_refusal_is_unsaved_and_does_not_apply_locally() {
    let (directory, mut app) = number_app();
    let before_settings = app.terminal_settings;
    let before_bytes = std::fs::read(directory.path().join("config.toml")).unwrap();
    let before = (
        app.configuration_admission.attempts,
        app.configuration_admission.accepted,
    );
    app.configuration_files.as_mut().unwrap().close_admission();
    open_number(&mut app);
    submit_number(&mut app, "257");
    assert_eq!(app.configuration_admission.attempts, before.0 + 1);
    assert_eq!(app.configuration_admission.accepted, before.1);
    let rejection = app
        .configuration_admission
        .rejection
        .as_deref()
        .expect("queue rejection");
    assert!(!rejection.is_empty());
    assert_eq!(app.status_message.as_deref(), Some(rejection));
    assert!(matches!(&app.mode, Mode::ValueDialog(host) if !host.is_saving()));
    assert_eq!(app.terminal_settings, before_settings);
    app.settle_filesystem_for_test();
    assert_eq!(app.configuration_files.as_ref().unwrap().pending(), 0);
    assert_eq!(
        std::fs::read(directory.path().join("config.toml")).unwrap(),
        before_bytes
    );
    assert_eq!(
        config::load(directory.path()).unwrap().terminal,
        before_settings
    );
}

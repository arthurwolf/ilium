use super::*;
use serde_json::json;
fn parser_pool_control_app() -> (
    tempfile::TempDir,
    App,
    crate::config::TerminalSettings,
    Vec<u8>,
) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("config.toml");
    std::fs::write(&path, "[future_settings]\nkeep = 'untouched'\n").unwrap();
    let settings = crate::config::TerminalSettings {
        engine_memory_budget_mib: 4096,
        scrollback_budget_mib: 16, // Preserve the independent scrollback budget.
        new_pane_directory: crate::config::NewPaneDirectory::FocusedTerminal,
        smart_copy_light: false,
        smart_copy_light_key: crate::config::SmartCopyLightKey::Alt,
    };
    crate::config::save_terminal_settings(directory.path(), &settings).unwrap();
    let mut app = App::new("parser-pool-control".into(), directory.path().into());
    app.config_dir = Some(directory.path().into());
    app.apply_terminal_settings(settings);
    app.settle_filesystem_for_test();
    assert!(app.configuration_files.is_some());
    assert_eq!(app.configuration_files.as_ref().unwrap().pending(), 0);
    assert_eq!(
        crate::config::load(directory.path()).unwrap().terminal,
        settings
    );
    let original = std::fs::read(&path).unwrap();
    (directory, app, settings, original)
}
fn parser_pool_set_command(value: Value) -> SettingsCommand {
    SettingsCommand {
        action: SettingsAction::Set,
        path: Some("terminal.engine_memory_budget_mib".into()),
        value: Some(value),
        direction: None,
    }
}
#[test]
fn parser_pool_execute_reports_pending_then_reloads_every_valid_boundary_and_custom_value() {
    let (directory, mut app, original, _) = parser_pool_control_app();
    for budget in [0, 256, 257, 16384] {
        let attempts = app.configuration_admission.attempts;
        let accepted = app.configuration_admission.accepted;
        let receipt = execute(&mut app, parser_pool_set_command(json!(budget))).unwrap();
        assert_eq!(app.configuration_admission.attempts, attempts + 1);
        assert_eq!(app.configuration_admission.accepted, accepted + 1);
        assert!(app.configuration_admission.rejection.is_none());
        let expected_receipt = ExecutionReceipt::local_write_pending(
            "Settings",
            json!({"path": "terminal.engine_memory_budget_mib", "accepted_writes": 1}),
            accepted + 1,
        );
        assert_eq!(receipt.status, expected_receipt.status);
        assert_eq!(receipt.data, expected_receipt.data);
        assert_ne!(
            receipt.status,
            ExecutionReceipt::immediate("immediate").status
        );
        assert!(!receipt.terminate_session_after_delivery);
        assert_eq!(app.status_message.as_deref(), Some("Saving settings…"));
        assert!(app.configuration_files.as_ref().unwrap().pending() > 0);
        let expected = crate::config::TerminalSettings {
            engine_memory_budget_mib: budget,
            ..original
        };
        assert_eq!(app.terminal_settings, expected);
        let current = execute(
            &mut app,
            SettingsCommand {
                action: SettingsAction::Get,
                path: None,
                value: None,
                direction: None,
            },
        )
        .unwrap();
        assert_eq!(current.status, "ok");
        assert_eq!(
            current.data["terminal"]["engine_memory_budget_mib"],
            json!(budget)
        );
        assert_eq!(
            current.data["terminal"]["parser_pool_enabled"],
            json!(budget != 0)
        );
        app.settle_filesystem_for_test();
        assert_eq!(app.configuration_files.as_ref().unwrap().pending(), 0);
        assert_eq!(
            crate::config::load(directory.path()).unwrap().terminal,
            expected
        );
        let document: toml::Value =
            toml::from_str(&std::fs::read_to_string(directory.path().join("config.toml")).unwrap())
                .unwrap();
        assert_eq!(
            document["future_settings"]["keep"].as_str(),
            Some("untouched")
        );
        assert_eq!(app.configuration_admission.accepted, accepted + 1);
    }
}
#[test]
fn parser_pool_execute_rejects_invalid_values_without_reporting_a_save() {
    let (directory, mut app, original, original_bytes) = parser_pool_control_app();
    for value in [
        json!(-1),
        json!(1),
        json!(255),
        json!(16385),
        json!(u32::MAX),
        json!(u64::from(u32::MAX) + 1),
        json!(1.5),
    ] {
        let attempts = app.configuration_admission.attempts;
        let accepted = app.configuration_admission.accepted;
        app.status_message = Some("preserved status".into());
        assert!(execute(&mut app, parser_pool_set_command(value)).is_err());
        assert_eq!(app.terminal_settings, original);
        assert_eq!(app.configuration_admission.attempts, attempts);
        assert_eq!(app.configuration_admission.accepted, accepted);
        assert_eq!(app.status_message.as_deref(), Some("preserved status"));
        app.settle_filesystem_for_test();
        assert_eq!(
            std::fs::read(directory.path().join("config.toml")).unwrap(),
            original_bytes
        );
        assert_eq!(
            crate::config::load(directory.path()).unwrap().terminal,
            original
        );
    }
}
#[test]
fn parser_pool_execute_reports_missing_or_closed_writer_as_unsaved() {
    for close_writer in [false, true] {
        let (directory, mut app, original, original_bytes) = parser_pool_control_app();
        if close_writer {
            app.configuration_files.as_mut().unwrap().close_admission();
        } else {
            app.configuration_files = None;
        }
        let attempts = app.configuration_admission.attempts;
        let accepted = app.configuration_admission.accepted;
        let error = execute(&mut app, parser_pool_set_command(json!(257))).unwrap_err();
        assert!(error.contains("Settings change has unaccepted writes:"));
        assert_eq!(app.configuration_admission.attempts, attempts + 1); // The scoped setter attempted persistence once.
        assert_eq!(app.configuration_admission.accepted, accepted);
        let rejection = app.configuration_admission.rejection.as_deref().unwrap();
        assert!(error.contains(rejection));
        assert_eq!(app.status_message.as_deref(), Some(rejection));
        assert_ne!(app.status_message.as_deref(), Some("Saving settings…"));
        assert_eq!(
            app.terminal_settings,
            crate::config::TerminalSettings {
                engine_memory_budget_mib: 257,
                ..original
            }
        );
        app.settle_filesystem_for_test(); // Drain the closed owner without retrying the refused command.
        assert_eq!(
            std::fs::read(directory.path().join("config.toml")).unwrap(),
            original_bytes
        );
        assert_eq!(
            crate::config::load(directory.path()).unwrap().terminal,
            original
        );
    }
}

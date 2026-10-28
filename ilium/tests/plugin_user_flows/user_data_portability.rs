//! Plugin data export, erase and import for the user (USR-245 to USR-248, USR-262).

use crate::support::*;

fn report_text(output: &CliOutput) -> String {
    serde_json::to_string(&output.stdout_jsonl).unwrap_or_default()
}

/// REQ: USR-245
#[test]
fn usr_245_export_data_writes_one_file_per_data_kind_into_one_folder() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.data", "1.0.0", &["tree.read"]));
    rig.set_plugin_setting("com.example.data", "theme", "dark");
    rig.write_storage_bucket("com.example.data");
    rig.plugin_logs("com.example.data", "info", "a log line");
    rig.inject_trap("com.example.data");
    rig.start_server();
    let _ = rig.call_needing("com.example.data", "server", "tree.read");
    let folder = rig.scratch_dir().join("export");
    let output = rig.cli(&["export-data", folder.to_str().expect("utf-8 path")]);
    assert_eq!(result_exit_code(&output), 0, "{:?}", error_codes(&output));
    let files: Vec<_> = std::fs::read_dir(&folder)
        .expect("export folder")
        .flatten()
        .collect();
    assert_eq!(
        files.len(),
        6,
        "settings, storage, grants, logs, audit, crash"
    );
    assert!(files.iter().all(|file| file.path().is_file()));
}

/// REQ: USR-246
#[test]
fn usr_246_erase_data_removes_one_plugin_and_keeps_the_others() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.gone", "1.0.0", &["tree.read"]));
    install_ok(&mut rig, &spec("com.example.kept", "1.0.0", &["tree.read"]));
    rig.set_plugin_setting("com.example.gone", "theme", "dark");
    rig.set_plugin_setting("com.example.kept", "theme", "light");
    rig.plugin_logs("com.example.gone", "info", "gone line");
    rig.attach_client(vec![DialogAnswer::ConfirmYes]);
    let output = rig.cli(&["erase-data", "com.example.gone"]);
    assert_eq!(result_exit_code(&output), 0, "{:?}", error_codes(&output));
    assert!(!rig.confirmations_shown().is_empty());
    assert_eq!(rig.plugin_setting("com.example.gone", "theme"), None);
    assert!(rig.grant_entries("com.example.gone").is_empty());
    assert!(rig.log_lines("com.example.gone").is_empty());
    assert_eq!(
        rig.plugin_setting("com.example.kept", "theme").as_deref(),
        Some("light")
    );
    let report = report_text(&output);
    assert!(report.contains("com.example.gone") && report.contains("com.example.kept"));
}

/// REQ: USR-247
#[test]
fn usr_247_export_and_erase_succeed_with_networking_blocked() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.net", "1.0.0", &["tree.read"]));
    rig.set_plugin_setting("com.example.net", "theme", "dark");
    rig.set_request_logging(true);
    rig.deny_network();
    let folder = rig.scratch_dir().join("offline-export");
    let export = rig.cli(&["export-data", folder.to_str().expect("utf-8 path")]);
    assert_eq!(result_exit_code(&export), 0, "{:?}", error_codes(&export));
    assert_eq!(rig.fetch_calls(), 0);
    rig.attach_client(vec![DialogAnswer::ConfirmYes]);
    let erase = rig.cli(&["erase-data", "com.example.net"]);
    assert_eq!(result_exit_code(&erase), 0, "{:?}", error_codes(&erase));
    assert!(rig.network_requests().is_empty());
}

/// REQ: USR-248
#[test]
fn usr_248_erase_keeps_policy_required_audit_records_and_names_them() {
    let mut rig = UserRig::new();
    rig.write_system_policy("version = 1\n[audit]\nkeep_days = 30\n");
    install_ok(&mut rig, &spec("com.example.aud", "1.0.0", &[]));
    rig.attach_client(vec![DialogAnswer::ConfirmYes]);
    let output = rig.cli(&["erase-data", "com.example.aud"]);
    assert_eq!(result_exit_code(&output), 0, "{:?}", error_codes(&output));
    let live = rig.audit_dir().join("audit.jsonl");
    assert!(std::fs::read_to_string(&live)
        .unwrap_or_default()
        .contains("com.example.aud"));
    assert!(report_text(&output).contains("30"));
    rig.advance_ms(31 * 86_400_000);
    rig.start_server();
    assert!(!std::fs::read_to_string(&live)
        .unwrap_or_default()
        .contains("com.example.aud"));
}

/// REQ: USR-262
#[test]
fn usr_262_import_data_restores_settings_after_reinstall_when_confirmed() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.roam", "1.0.0", &["tree.read"]));
    rig.set_plugin_setting("com.example.roam", "theme", "dark");
    let folder = rig.scratch_dir().join("roam-export");
    let folder_text = folder.to_str().expect("utf-8 path").to_string();
    assert_eq!(
        result_exit_code(&rig.cli(&["export-data", folder_text.as_str()])),
        0
    );
    let _ = rig.cli(&["uninstall", "com.example.roam", "--yes"]);
    install_ok(&mut rig, &spec("com.example.roam", "1.0.0", &["tree.read"]));
    rig.attach_client(vec![DialogAnswer::ConfirmYes]);
    let output = rig.cli(&["import-data", folder_text.as_str()]);
    assert_eq!(result_exit_code(&output), 0, "{:?}", error_codes(&output));
    assert!(!rig.confirmations_shown().is_empty());
    assert_eq!(
        rig.plugin_setting("com.example.roam", "theme").as_deref(),
        Some("dark")
    );
}

/// REQ: USR-262
#[test]
fn usr_262_import_data_keeps_existing_data_without_replace() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.keep", "1.0.0", &["tree.read"]));
    rig.set_plugin_setting("com.example.keep", "theme", "dark");
    let folder = rig.scratch_dir().join("keep-export");
    let folder_text = folder.to_str().expect("utf-8 path").to_string();
    assert_eq!(
        result_exit_code(&rig.cli(&["export-data", folder_text.as_str()])),
        0
    );
    rig.set_plugin_setting("com.example.keep", "theme", "light");
    rig.attach_client(vec![DialogAnswer::ConfirmYes]);
    let output = rig.cli(&["import-data", folder_text.as_str()]);
    assert_eq!(result_exit_code(&output), 0, "{:?}", error_codes(&output));
    assert_eq!(
        rig.plugin_setting("com.example.keep", "theme").as_deref(),
        Some("light")
    );
}

/// REQ: USR-262
#[test]
fn usr_262_import_data_writes_nothing_when_the_confirmation_is_declined() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.shy", "1.0.0", &["tree.read"]));
    rig.set_plugin_setting("com.example.shy", "theme", "dark");
    let folder = rig.scratch_dir().join("shy-export");
    let folder_text = folder.to_str().expect("utf-8 path").to_string();
    assert_eq!(
        result_exit_code(&rig.cli(&["export-data", folder_text.as_str()])),
        0
    );
    let _ = rig.cli(&["uninstall", "com.example.shy", "--yes"]);
    install_ok(&mut rig, &spec("com.example.shy", "1.0.0", &["tree.read"]));
    rig.attach_client(vec![DialogAnswer::ConfirmNo]);
    let _ = rig.cli(&["import-data", folder_text.as_str()]);
    assert_eq!(rig.plugin_setting("com.example.shy", "theme"), None);
}

/// REQ: USR-245
#[test]
fn usr_245_export_data_file_contents_carry_the_plugin_id_setting_and_log_line() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.content", "1.0.0", &["tree.read"]));
    rig.set_plugin_setting("com.example.content", "theme", "dark");
    rig.plugin_logs("com.example.content", "info", "a distinct log line");
    let folder = rig.scratch_dir().join("content-export");
    let output = rig.cli(&["export-data", folder.to_str().expect("utf-8 path")]);
    assert_eq!(result_exit_code(&output), 0, "{:?}", error_codes(&output));
    let mut combined = String::new();
    for file in std::fs::read_dir(&folder).expect("export folder").flatten() {
        combined.push_str(&std::fs::read_to_string(file.path()).unwrap_or_default());
    }
    assert!(combined.contains("com.example.content"));
    assert!(combined.contains("dark"));
    assert!(combined.contains("a distinct log line"));
}

/// REQ: USR-246
#[test]
fn usr_246_erase_data_declined_at_the_confirmation_keeps_the_plugin_data() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.stay", "1.0.0", &["tree.read"]));
    rig.set_plugin_setting("com.example.stay", "theme", "dark");
    rig.plugin_logs("com.example.stay", "info", "stay line");
    rig.attach_client(vec![DialogAnswer::ConfirmNo]);
    let _ = rig.cli(&["erase-data", "com.example.stay"]);
    assert!(!rig.confirmations_shown().is_empty());
    assert_eq!(
        rig.plugin_setting("com.example.stay", "theme").as_deref(),
        Some("dark")
    );
    assert!(!rig.log_lines("com.example.stay").is_empty());
}

/// REQ: USR-247
#[test]
fn usr_247_export_with_networking_blocked_still_writes_files() {
    let mut rig = UserRig::new();
    install_ok(&mut rig, &spec("com.example.offline", "1.0.0", &["tree.read"]));
    rig.set_plugin_setting("com.example.offline", "theme", "dark");
    rig.deny_network();
    let folder = rig.scratch_dir().join("offline-files-export");
    let export = rig.cli(&["export-data", folder.to_str().expect("utf-8 path")]);
    assert_eq!(result_exit_code(&export), 0, "{:?}", error_codes(&export));
    let written = std::fs::read_dir(&folder)
        .expect("export folder")
        .flatten()
        .count();
    assert!(written > 0, "export wrote no files while networking was blocked");
    assert!(rig.network_requests().is_empty());
}

/// REQ: USR-248
#[test]
fn usr_248_kept_audit_records_survive_until_the_retention_period_ends() {
    let mut rig = UserRig::new();
    rig.write_system_policy("version = 1\n[audit]\nkeep_days = 30\n");
    install_ok(&mut rig, &spec("com.example.keepaud", "1.0.0", &[]));
    rig.attach_client(vec![DialogAnswer::ConfirmYes]);
    let _ = rig.cli(&["erase-data", "com.example.keepaud"]);
    let live = rig.audit_dir().join("audit.jsonl");
    rig.advance_ms(29 * 86_400_000);
    rig.start_server();
    assert!(std::fs::read_to_string(&live)
        .unwrap_or_default()
        .contains("com.example.keepaud"));
}

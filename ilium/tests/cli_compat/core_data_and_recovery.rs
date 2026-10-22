//! Core data versions and core plugin restart (port-compat lane: COR-093, COR-094, COR-095, COR-109).
//! Drives the real `ilium` binary in an isolated sandbox and checks the files it leaves behind.

use crate::common::*;

/// REQ: COR-093
#[test]
fn cor_093_newer_snapshot_and_config_are_not_overwritten() {
    let sandbox = Sandbox::new();
    let sessions = sandbox.project.join(".ilium/sessions");
    std::fs::create_dir_all(&sessions).expect("sessions dir");
    let snapshot_path = sessions.join("default.json");
    let snapshot_text = r#"{"version": 99}"#;
    std::fs::write(&snapshot_path, snapshot_text).expect("snapshot");
    let config_dir = sandbox.path().join("config/ilium");
    std::fs::create_dir_all(&config_dir).expect("config dir");
    let config_path = config_dir.join("config.toml");
    let config_text = "config_version = 99\n";
    std::fs::write(&config_path, config_text).expect("config");

    let run = sandbox.ilium(&["ls"]);

    assert_eq!(
        std::fs::read_to_string(&snapshot_path).expect("snapshot readable"),
        snapshot_text,
        "a snapshot written by a newer build must stay byte for byte"
    );
    assert_eq!(
        std::fs::read_to_string(&config_path).expect("config readable"),
        config_text,
        "a config written by a newer build must stay byte for byte"
    );
    let output = format!("{}{}", run.stdout, run.stderr);
    assert!(
        output.contains("99"),
        "the notice must name the stored version 99: {output}"
    );
}

/// REQ: COR-094
#[test]
fn cor_094_unreadable_snapshot_version_is_moved_aside_as_corrupt() {
    let sandbox = Sandbox::new();
    let sessions = sandbox.project.join(".ilium/sessions");
    std::fs::create_dir_all(&sessions).expect("sessions dir");
    let snapshot_path = sessions.join("default.json");
    std::fs::write(&snapshot_path, r#"{"version": "not-a-number"}"#).expect("snapshot");

    let run = sandbox.ilium(&["ls"]);

    assert_eq!(run.code, 0, "the session must start clean: {}", run.stderr);
    assert!(
        !snapshot_path.exists(),
        "a snapshot with an unreadable version must be moved aside, not read"
    );
    let moved_aside_names: Vec<String> = std::fs::read_dir(&sessions)
        .expect("sessions dir readable")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name != "default.json" && name.contains("default"))
        .collect();
    assert!(
        !moved_aside_names.is_empty(),
        "the corrupt snapshot must leave a moved-aside copy in {sessions:?}"
    );
}

/// REQ: COR-095
#[test]
fn cor_095_section_from_a_newer_schema_is_kept_unmodified_and_reported_once() {
    let sandbox = Sandbox::new();
    let sessions = sandbox.project.join(".ilium/sessions");
    std::fs::create_dir_all(&sessions).expect("sessions dir");
    let snapshot_path = sessions.join("default.json");
    let section_text = r#"{"schema_version": 99, "data": {"monitors": ["keep-me"]}}"#;
    let snapshot_text = format!(
        r#"{{"version": 4, "plugin_sections": {{"ilium.core.progress": {section_text}}}}}"#
    );
    std::fs::write(&snapshot_path, &snapshot_text).expect("snapshot");

    let run = sandbox.ilium(&["ls"]);

    assert_eq!(run.code, 0, "{}", run.stderr);
    let stored: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(&snapshot_path).expect("snapshot readable"),
    )
    .expect("snapshot must remain valid JSON");
    let expected: serde_json::Value =
        serde_json::from_str(section_text).expect("expected section parses");
    assert_eq!(
        stored["plugin_sections"]["ilium.core.progress"], expected,
        "the newer-schema section must be kept unmodified"
    );
    let output = format!("{}{}", run.stdout, run.stderr);
    assert_eq!(
        output.matches("ilium.core.progress").count(),
        1,
        "the unknown section must be reported exactly once at boot: {output}"
    );
}

/// REQ: COR-109
/// Covers only the `ilium plugin restart <id>` entry point; the reset of the failure window needs
/// a degraded core plugin, which the test surface cannot produce yet.
#[test]
fn cor_109_plugin_restart_command_restarts_a_core_plugin_and_names_it() {
    let sandbox = Sandbox::new();

    let run = sandbox.ilium(&["plugin", "restart", "ilium.core.diagnostics"]);

    assert_eq!(run.code, 0, "restart must succeed: {}", run.stderr);
    assert!(
        run.stdout.contains("ilium.core.diagnostics"),
        "the restart answer must name the plugin: {}",
        run.stdout
    );
}

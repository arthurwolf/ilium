//! Shared helpers of the data-migration lane. Everything uses the skeleton or the items named in
//! API_GAPS.md (the migration rig).

#![allow(dead_code)]

use std::collections::BTreeMap;

use ilium_plugin_api::manifest::{validate_json, CheckMode, Diagnostic, ValidationContext};
use ilium_plugin_api::{ErrorCode, Platform, PluginError, Tier};
use ilium_plugin_host::cli::CliOutput;
pub use ilium_plugin_host::testing::migration::{
    ActivateReport, Bucket, MigServer, MigrateCall, MigrateScope, MigrationRig, PluginSpec, Step,
};
use serde_json::Value;

/// The plugin id most tests use.
pub const NOTES: &str = "com.example.notes";

/// One mebibyte.
pub const MIB: u64 = 1024 * 1024;

/// A fresh rig with empty data, state and config directories and plenty of free space.
pub fn rig() -> MigrationRig {
    MigrationRig::new()
}

/// Version 1.0.0 of the notes plugin: no `data_version` key, no migrator.
pub fn notes_v1() -> PluginSpec {
    PluginSpec::new(NOTES, "1.0.0")
}

/// Version 2.0.0 of the notes plugin with `data_version` 2 and the given migrator steps.
pub fn notes_v2(steps: Vec<Step>) -> PluginSpec {
    PluginSpec::new(NOTES, "2.0.0")
        .data_version(2)
        .migrator(steps)
}

/// Version 3.0.0 of the notes plugin with `data_version` 3 and the given migrator steps.
pub fn notes_v3(steps: Vec<Step>) -> PluginSpec {
    PluginSpec::new(NOTES, "3.0.0")
        .data_version(3)
        .migrator(steps)
}

/// UTF-8 bytes of a text.
pub fn bytes(text: &str) -> Vec<u8> {
    text.as_bytes().to_vec()
}

/// A plugin error with the given code and message.
pub fn plugin_error(code: ErrorCode, message: &str) -> PluginError {
    PluginError {
        code,
        message: message.to_owned(),
        retry_after_ms: None,
    }
}

/// Every `code` of the `error` lines of CLI output, in order.
pub fn error_codes(output: &CliOutput) -> Vec<String> {
    output
        .stdout_jsonl
        .iter()
        .filter(|line| line["type"] == "error")
        .filter_map(|line| line["code"].as_str().map(str::to_owned))
        .collect()
}

/// Installs a spec and asserts exit code 0.
pub fn install_ok(rig: &MigrationRig, spec: &PluginSpec) {
    let output = rig.install(spec);
    assert_eq!(
        output.exit_code, 0,
        "install failed: {:?}",
        output.stdout_jsonl
    );
}

/// Updates with a spec and asserts exit code 0.
pub fn update_ok(rig: &MigrationRig, spec: &PluginSpec, flags: &[&str]) {
    let output = rig.update(spec, flags);
    assert_eq!(
        output.exit_code, 0,
        "update failed: {:?}",
        output.stdout_jsonl
    );
}

/// A baseline manifest as JSON text with an extra raw `data_version` member when given.
pub fn manifest_text(data_version_json: Option<&str>) -> String {
    let mut text = String::from(
        "{\"api\":\"ilium:plugin@0.1.0\",\"id\":\"com.example.notes\",\"name\":\"Notes\",\
         \"version\":\"1.0.0\",\"publisher\":\"Example Labs\",\
         \"server\":{\"entry\":\"server.wasm\",\"world\":\"ilium:server/base@0.1.0\",\"exports\":[]}",
    );
    if let Some(raw) = data_version_json {
        text.push_str(&format!(",\"data_version\":{raw}"));
    }
    text.push('}');
    text
}

/// Validation context for a source manifest (`ilium plugin check`, tier T3).
pub fn check_context() -> ValidationContext {
    ValidationContext {
        tier: Tier::T3,
        mode: CheckMode::Check,
        platform: Platform::Linux,
        installed: Vec::new(),
        core_plugins: Vec::new(),
        unstable_features: Vec::new(),
    }
}

/// The diagnostics of a manifest text.
pub fn diagnostics_of(text: &str) -> Vec<Diagnostic> {
    validate_json(text, &check_context())
}

/// The codes of the diagnostics, in order.
pub fn codes_of(found: &[Diagnostic]) -> Vec<String> {
    found
        .iter()
        .map(|diagnostic| diagnostic.code.as_str().to_owned())
        .collect()
}

/// Whether a manifest with this raw `data_version` member is refused with `manifest.data-version`.
pub fn is_refused_for_data_version(raw: &str) -> bool {
    let found = diagnostics_of(&manifest_text(Some(raw)));
    codes_of(&found)
        .iter()
        .any(|code| code == "manifest.data-version")
}

/// The registry entry (`installed.json`) of a plugin, or `Value::Null`.
pub fn registry_entry(rig: &MigrationRig, id: &str) -> Value {
    rig.registry_json()["plugins"]
        .as_array()
        .and_then(|plugins| plugins.iter().find(|entry| entry["id"] == id))
        .cloned()
        .unwrap_or(Value::Null)
}

/// Every key of a bucket with its value, as the plugin sees it.
pub fn dump(server: &MigServer, bucket: Bucket) -> BTreeMap<String, Vec<u8>> {
    server.storage(NOTES).dump(bucket)
}

/// The migrate calls of a plugin whose scope is the global bucket and the data directory.
pub fn global_calls(rig: &MigrationRig, id: &str) -> Vec<MigrateCall> {
    rig.migrate_calls(id)
        .into_iter()
        .filter(|call| matches!(call.scope, MigrateScope::Global))
        .collect()
}

/// The migrate calls of a plugin whose scope is one project session.
pub fn session_calls(rig: &MigrationRig, id: &str) -> Vec<MigrateCall> {
    rig.migrate_calls(id)
        .into_iter()
        .filter(|call| matches!(call.scope, MigrateScope::Session { .. }))
        .collect()
}

/// A backup directory name has the form `<version>-<YYYYMMDDTHHmmssZ>`.
pub fn is_backup_name(name: &str, version: &str) -> bool {
    let Some(stamp) = name.strip_prefix(&format!("{version}-")) else {
        return false;
    };
    let bytes = stamp.as_bytes();
    bytes.len() == 16
        && bytes[8] == b'T'
        && bytes[15] == b'Z'
        && bytes[..8].iter().all(u8::is_ascii_digit)
        && bytes[9..15].iter().all(u8::is_ascii_digit)
}

/// Brings up server `name` on a project and session, activates the notes plugin at version 1 with
/// the given global entries, and returns the server. The rig must already have version 1 installed.
pub fn running_v1(rig: &MigrationRig, name: &str, project: &str, session: &str) -> MigServer {
    let server = rig.server(name, project, session);
    let report = server.activate(NOTES);
    assert_eq!(
        report.active_version, "1.0.0",
        "v1 did not activate: {report:?}"
    );
    server
}

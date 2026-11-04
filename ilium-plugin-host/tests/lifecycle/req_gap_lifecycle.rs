//! Requirement tests for the lifecycle lane's install, update and uninstall crash-safety rules:
//! LIF-000 (staging location), LIF-021 (grants before registry), LIF-022 (trial retries),
//! LIF-023 (rollback refused during an update) and LIF-024 (uninstall during an update).
//!
//! These drive the `ilium_plugin_host::testing::ops` rig, which owns the plugin root, the
//! registry (`installed.json`), the grants file and the CLI. They fail until the host behaves.

use std::path::PathBuf;

use ilium_plugin_host::cli::CliOutput;
use ilium_plugin_host::testing::ops::{Fault, OpsRig, PackageSpec, RegistryStep};

const NOTES: &str = "com.example.notes";
const CANDIDATE: &str = "2.0.0";
const INSTALLED: &str = "1.0.0";

/// Writes a package file for `spec` and runs `install` non-interactively.
fn install_spec(rig: &OpsRig, spec: &PackageSpec) -> CliOutput {
    let path = rig.write_package(spec);
    let path_text = path.to_string_lossy().into_owned();
    rig.cli(&["install", &path_text, "--yes", "--allow-unsigned"])
}

/// Writes a package file for `spec` and runs `update` non-interactively.
fn update_spec(rig: &OpsRig, spec: &PackageSpec) -> CliOutput {
    let path = rig.write_package(spec);
    let path_text = path.to_string_lossy().into_owned();
    rig.cli(&["update", &path_text, "--yes", "--allow-unsigned"])
}

/// The registry entry of a plugin, found whether the registry keys plugins by id or lists them.
fn registry_entry(rig: &OpsRig, plugin_id: &str) -> serde_json::Value {
    let registry = rig.registry_json();
    match &registry["plugins"] {
        serde_json::Value::Object(map) => map.get(plugin_id).cloned().unwrap_or(serde_json::Value::Null),
        serde_json::Value::Array(rows) => rows
            .iter()
            .find(|row| row["id"] == plugin_id)
            .cloned()
            .unwrap_or(serde_json::Value::Null),
        _ => serde_json::Value::Null,
    }
}

/// The `update` object of a plugin's registry entry, or null when no update is pending.
fn update_record(rig: &OpsRig, plugin_id: &str) -> serde_json::Value {
    registry_entry(rig, plugin_id)["update"].clone()
}

/// The `update.state` value of a plugin, when an update is pending.
fn update_state(rig: &OpsRig, plugin_id: &str) -> Option<String> {
    update_record(rig, plugin_id)["state"].as_str().map(str::to_owned)
}

/// The installed version of a plugin, read from its registry entry.
fn installed_version(rig: &OpsRig, plugin_id: &str) -> Option<String> {
    let entry = registry_entry(rig, plugin_id);
    entry["version"]
        .as_str()
        .or_else(|| entry["installed_version"].as_str())
        .map(str::to_owned)
}

/// True when the registry lists the plugin.
fn registry_has(rig: &OpsRig, plugin_id: &str) -> bool {
    !registry_entry(rig, plugin_id).is_null()
}

/// True when the grants file has an entry for the plugin.
fn grants_has(rig: &OpsRig, plugin_id: &str) -> bool {
    !rig.grants_json()[plugin_id].is_null()
}

/// Whether the plugin's stdout or stderr text mentions the given error code.
fn output_mentions(output: &CliOutput, code: &str) -> bool {
    output.stdout_raw.contains(code) || output.stderr.contains(code)
}

/// Installs the first version of the notes plugin as a lazy (tier 3) plugin, asserting success.
fn install_lazy_notes(rig: &OpsRig) {
    let output = install_spec(rig, &PackageSpec::new(NOTES, INSTALLED).tier(3));
    assert_eq!(output.exit_code, 0, "install must succeed: {}", output.stderr);
}

/// Stages an update of the notes plugin to the candidate version as a lazy plugin.
fn stage_lazy_update(rig: &OpsRig) {
    let output = update_spec(rig, &PackageSpec::new(NOTES, CANDIDATE).tier(3));
    assert_eq!(output.exit_code, 0, "update must stage: {}", output.stderr);
}

/// REQ: LIF-000
#[test]
fn req_lif_000_staging_same_volume_a() {
    let rig = OpsRig::new();
    let output = install_spec(&rig, &PackageSpec::new(NOTES, INSTALLED));
    assert_eq!(output.exit_code, 0, "install must succeed: {}", output.stderr);
    let plugin_root = rig.plugins_root();
    let store_dir = plugin_root.join(rig.store_path_for(NOTES));
    assert!(
        store_dir.starts_with(&plugin_root),
        "the committed package must sit under the plugin root, so staging can rename into it"
    );
    assert!(store_dir.exists(), "the package must be committed to store/");
    let staging = plugin_root.join("staging");
    let leftovers: Vec<PathBuf> = std::fs::read_dir(&staging)
        .map(|entries| entries.filter_map(|entry| entry.ok().map(|e| e.path())).collect())
        .unwrap_or_default();
    assert!(
        leftovers.is_empty(),
        "staging/ must hold no leftover install directory after the single rename: {leftovers:?}"
    );
}

/// REQ: LIF-000
#[test]
fn req_lif_000_staging_not_in_os_temp_dir_b() {
    let rig = OpsRig::new();
    let output = install_spec(&rig, &PackageSpec::new(NOTES, INSTALLED));
    assert_eq!(output.exit_code, 0, "install must succeed: {}", output.stderr);
    let temp_text = std::env::temp_dir().to_string_lossy().into_owned();
    let names_temp_staging = output.stdout_raw.contains("staging") && output.stdout_raw.contains(&temp_text);
    assert!(
        !names_temp_staging,
        "install must not unpack into the OS temporary directory ({temp_text})"
    );
    let staging_in_temp = std::env::temp_dir().join("staging");
    assert!(
        !staging_in_temp.join(NOTES).exists(),
        "no staging copy of the plugin may appear under the OS temporary directory"
    );
}

/// REQ: LIF-021
#[test]
fn req_lif_021_grants_before_registry_a() {
    let rig = OpsRig::new();
    rig.inject(Fault::RegistryWrite(RegistryStep::DirSynced));
    let _ = install_spec(&rig, &PackageSpec::new(NOTES, INSTALLED).grant("notes.read"));
    assert!(
        grants_has(&rig, NOTES),
        "a crash after the registry write must leave the reviewed grants entry in grants.json"
    );
    assert!(
        registry_has(&rig, NOTES),
        "a crash after the registry write must leave the plugin in installed.json"
    );
}

/// REQ: LIF-021
#[test]
fn req_lif_021_crash_before_registry_rename_removes_grants_entry_b() {
    let rig = OpsRig::new();
    rig.inject(Fault::RegistryWrite(RegistryStep::TempSynced));
    let _ = install_spec(&rig, &PackageSpec::new(NOTES, INSTALLED).grant("notes.read"));
    let _restarted = rig.start_server("default");
    assert!(
        !registry_has(&rig, NOTES),
        "an interruption before the registry rename must leave the plugin absent from installed.json"
    );
    assert!(
        !grants_has(&rig, NOTES),
        "start must delete the grants entry whose id has no registry entry"
    );
}

/// REQ: LIF-022
#[test]
fn req_lif_022_trial_restart_increments_attempts_a() {
    let rig = OpsRig::new();
    install_lazy_notes(&rig);
    stage_lazy_update(&rig);
    let server = rig.start_server("default");
    server.kill_sigkill();
    let _restarted = rig.start_server("default");
    assert_eq!(
        update_state(&rig, NOTES).as_deref(),
        Some("trial"),
        "the candidate must be in trial after the first start"
    );
    assert_eq!(
        update_record(&rig, NOTES)["attempts"].as_u64(),
        Some(1),
        "a restart in trial must increment update.attempts to 1"
    );
}

/// REQ: LIF-022
#[test]
fn req_lif_022_third_interruption_rolls_back_and_marks_skip_b() {
    let rig = OpsRig::new();
    install_lazy_notes(&rig);
    stage_lazy_update(&rig);
    for _ in 0..3 {
        let server = rig.start_server("default");
        server.kill_sigkill();
    }
    let _restarted = rig.start_server("default");
    assert!(
        update_record(&rig, NOTES).is_null(),
        "after the third interruption the update must be rolled back and cleared"
    );
    assert_eq!(
        installed_version(&rig, NOTES).as_deref(),
        Some(INSTALLED),
        "the old version must be active after the rollback"
    );
    assert!(
        rig.registry_json().to_string().contains("update.rolled-back"),
        "the rollback must be logged as update.rolled-back"
    );
}

/// REQ: LIF-023
#[test]
fn req_lif_023_rollback_refused_while_update_pending_a() {
    let rig = OpsRig::new();
    install_lazy_notes(&rig);
    stage_lazy_update(&rig);
    let before = rig.registry_bytes();
    let output = rig.cli(&["rollback", NOTES]);
    assert_eq!(output.exit_code, 4, "rollback during an update must exit 4");
    assert!(
        output_mentions(&output, "install.update-busy"),
        "rollback during an update must report install.update-busy: {}",
        output.stderr
    );
    assert_eq!(
        rig.registry_bytes(),
        before,
        "a refused rollback must leave the registry unchanged"
    );
}

/// REQ: LIF-023
#[test]
fn req_lif_023_rollback_succeeds_after_update_cancel_b() {
    let rig = OpsRig::new();
    install_lazy_notes(&rig);
    stage_lazy_update(&rig);
    let cancel = rig.cli(&["update-cancel", NOTES]);
    assert_eq!(cancel.exit_code, 0, "update-cancel must succeed: {}", cancel.stderr);
    let output = rig.cli(&["rollback", NOTES]);
    assert_eq!(
        output.exit_code, 0,
        "rollback must succeed once the update is cancelled: {}",
        output.stderr
    );
    assert_eq!(
        installed_version(&rig, NOTES).as_deref(),
        Some(INSTALLED),
        "rollback after cancel must leave the old version installed"
    );
}

/// REQ: LIF-024
#[test]
fn req_lif_024_uninstall_refused_while_migrating_a() {
    let rig = OpsRig::new();
    let install = install_spec(&rig, &PackageSpec::new(NOTES, INSTALLED).migrator_both());
    assert_eq!(install.exit_code, 0, "install must succeed: {}", install.stderr);
    let update = update_spec(&rig, &PackageSpec::new(NOTES, CANDIDATE).migrator_both().wait_ms(500));
    assert_eq!(update.exit_code, 0, "update must start: {}", update.stderr);
    assert_eq!(
        update_state(&rig, NOTES).as_deref(),
        Some("migrating"),
        "the slow migrator must hold the update in migrating"
    );
    let output = rig.cli(&["uninstall", NOTES, "--yes"]);
    assert_eq!(output.exit_code, 4, "uninstall during migration must exit 4");
    assert!(
        output_mentions(&output, "install.update-busy"),
        "uninstall during migration must report install.update-busy"
    );
    assert!(
        registry_has(&rig, NOTES),
        "a refused uninstall must leave the plugin installed until the migration finishes"
    );
}

/// REQ: LIF-024
#[test]
fn req_lif_024_uninstall_in_trial_removes_candidate_and_logs_cancel_b() {
    let rig = OpsRig::new();
    install_lazy_notes(&rig);
    stage_lazy_update(&rig);
    assert_eq!(
        update_state(&rig, NOTES).as_deref(),
        Some("trial"),
        "the lazy candidate must be in trial before the uninstall"
    );
    let candidate_dir = rig.plugins_root().join(rig.store_path_for(NOTES)).join(CANDIDATE);
    let output = rig.cli(&["uninstall", NOTES, "--yes"]);
    assert_eq!(output.exit_code, 0, "uninstall in trial must succeed: {}", output.stderr);
    assert!(
        !candidate_dir.exists(),
        "uninstall in trial must delete the candidate directory store/<id>/{CANDIDATE}"
    );
    assert!(
        output_mentions(&output, "update.cancelled"),
        "uninstall in trial must log update.cancelled"
    );
    assert!(
        !rig.plugins_root().join(rig.store_path_for(NOTES)).exists(),
        "uninstall must remove store/<id>/"
    );
}

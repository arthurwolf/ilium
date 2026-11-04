//! Gap-review tests for the config-migration lane: lifecycle operations on installed plugins
//! (LIF-095..099). Each test drives the in-process migration rig in
//! `ilium_plugin_host::testing::migration`. The rig and the CLI paths these tests call are not
//! complete yet, so the tests are expected to fail at runtime until the host provides them.

use std::collections::BTreeMap;

use ilium_plugin_host::cli::CliOutput;
use ilium_plugin_host::testing::migration::{MigrationRig, PluginSpec};
use serde_json::Value;

const NOTES: &str = "com.example.notes";
const BRIDGE: &str = "com.example.bridge";
const DEPENDANT: &str = "com.example.dependant";

fn rig() -> MigrationRig {
    MigrationRig::new()
}

fn notes(version: &str) -> PluginSpec {
    PluginSpec::new(NOTES, version)
}

fn error_codes(output: &CliOutput) -> Vec<String> {
    output
        .stdout_jsonl
        .iter()
        .filter(|line| line["type"] == "error")
        .filter_map(|line| line["code"].as_str().map(str::to_owned))
        .collect()
}

fn position_of(codes: &[String], needle: &str) -> Option<usize> {
    codes.iter().position(|code| code.contains(needle))
}

fn install_ok(rig: &MigrationRig, spec: &PluginSpec) {
    let output = rig.install(spec);
    assert_eq!(
        output.exit_code, 0,
        "install failed: {:?}",
        output.stdout_jsonl
    );
}

fn skip_list(rig: &MigrationRig, id: &str) -> Vec<String> {
    let registry: Value = rig.registry_json();
    registry["plugins"][id]["skip"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

fn data_hashes(rig: &MigrationRig, id: &str) -> BTreeMap<String, Vec<u8>> {
    rig.data_dir_files(id)
}

/// REQ: LIF-095
#[test]
fn req_lif_095_reinstall_staging_a() {
    let rig = rig();
    install_ok(&rig, &notes("1.0.0"));
    let first = rig.server("main", "/project-a", "s1");
    let second = rig.server("main", "/project-b", "s2");
    first.activate(NOTES);
    second.activate(NOTES);

    let grants_before = rig.info_json(NOTES)["grants"].clone();
    let previous_before = rig.info_json(NOTES)["previous_version"].clone();
    let data_before = data_hashes(&rig, NOTES);

    let output = rig.cli(&["plugin", "reinstall", NOTES]);
    assert_eq!(
        output.exit_code, 0,
        "reinstall failed: {:?}",
        output.stdout_jsonl
    );

    let codes = rig.log_codes();
    let deactivate_at = position_of(&codes, "deactivate(upgrade)")
        .expect("reinstall must send deactivate(upgrade) to the live instances");
    let activate_at = position_of(&codes, "restored")
        .expect("reinstall must reactivate the instances with restored");
    assert!(
        deactivate_at < activate_at,
        "deactivate(upgrade) must precede the reactivation: {codes:?}"
    );
    assert_eq!(rig.info_json(NOTES)["grants"], grants_before);
    assert_eq!(rig.info_json(NOTES)["previous_version"], previous_before);
    assert_eq!(data_hashes(&rig, NOTES), data_before);
}

/// REQ: LIF-095
#[test]
fn req_lif_095_reinstall_failure_window_unchanged_b() {
    let rig = rig();
    install_ok(&rig, &notes("1.0.0"));
    let server = rig.server("main", "/project-a", "s1");
    server.activate(NOTES);

    let window_before = rig.failures_in_window(NOTES);
    let output = rig.cli(&["plugin", "reinstall", NOTES]);
    assert_eq!(
        output.exit_code, 0,
        "reinstall failed: {:?}",
        output.stdout_jsonl
    );
    assert_eq!(
        rig.failures_in_window(NOTES),
        window_before,
        "reinstall must not change the failure window"
    );
}

/// REQ: LIF-096
#[test]
fn req_lif_096_downgrade_skip_mark_a() {
    let rig = rig();
    install_ok(&rig, &notes("1.1.0"));
    let downgrade = rig.cli_install_file(&notes("1.0.0"), &["--allow-downgrade"]);
    assert_eq!(
        downgrade.exit_code, 0,
        "downgrade failed: {:?}",
        downgrade.stdout_jsonl
    );
    assert!(
        skip_list(&rig, NOTES).contains(&"1.1.0".to_owned()),
        "the replaced newer version must be listed in skip"
    );

    let check = rig.cli(&["plugin", "check-updates", "--auto"]);
    assert_eq!(check.exit_code, 0, "{:?}", check.stdout_jsonl);
    assert_eq!(
        rig.info_json(NOTES)["active_version"],
        Value::String("1.0.0".to_owned()),
        "auto_update = install must not reinstall a skipped version"
    );

    let explicit = rig.cli_install_file(&notes("1.1.0"), &[]);
    assert_eq!(
        explicit.exit_code, 0,
        "an explicit install of the skipped version must succeed: {:?}",
        explicit.stdout_jsonl
    );
    assert!(!skip_list(&rig, NOTES).contains(&"1.1.0".to_owned()));
}

/// REQ: LIF-096
#[test]
fn req_lif_096_downgrade_skip_mark_b() {
    let rig = rig();
    install_ok(&rig, &notes("1.1.0"));
    let downgrade = rig.cli_install_file(&notes("1.0.0"), &["--allow-downgrade"]);
    assert_eq!(downgrade.exit_code, 0, "{:?}", downgrade.stdout_jsonl);

    let update_all = rig.cli(&["plugin", "update", "--all"]);
    assert_eq!(update_all.exit_code, 0, "{:?}", update_all.stdout_jsonl);
    assert_eq!(
        rig.info_json(NOTES)["active_version"],
        Value::String("1.0.0".to_owned()),
        "update --all must skip the marked version"
    );

    let unskip = rig.cli(&["plugin", "install", NOTES, "1.1.0", "--unskip"]);
    assert_eq!(unskip.exit_code, 0, "{:?}", unskip.stdout_jsonl);
    assert!(
        !skip_list(&rig, NOTES).contains(&"1.1.0".to_owned()),
        "--unskip must clear the skip mark"
    );
}

/// REQ: LIF-097
#[test]
fn req_lif_097_dependant_refuses_downgrade_a() {
    let rig = rig();
    install_ok(&rig, &PluginSpec::new(BRIDGE, "2.0.0"));
    install_ok(&rig, &PluginSpec::new(DEPENDANT, "1.0.0"));

    let rollback = rig.rollback(BRIDGE, &[]);
    assert_ne!(rollback.exit_code, 0, "rollback must be refused");
    assert!(
        error_codes(&rollback).contains(&"dependency.in-use".to_owned()),
        "expected dependency.in-use: {:?}",
        rollback.stdout_jsonl
    );

    let downgrade = rig.cli_install_file(
        &PluginSpec::new(BRIDGE, "1.0.0"),
        &["--allow-downgrade"],
    );
    assert_ne!(downgrade.exit_code, 0);
    assert!(error_codes(&downgrade).contains(&"dependency.in-use".to_owned()));
    assert_eq!(
        rig.info_json(BRIDGE)["active_version"],
        Value::String("2.0.0".to_owned()),
        "the refused downgrade leaves the bridge plugin at 2.0.0"
    );
}

/// REQ: LIF-097
#[test]
fn req_lif_097_dependant_disabled_when_forced_b() {
    let rig = rig();
    install_ok(&rig, &PluginSpec::new(BRIDGE, "2.0.0"));
    install_ok(&rig, &PluginSpec::new(DEPENDANT, "1.0.0"));

    let forced = rig.rollback(BRIDGE, &["--force"]);
    assert_eq!(forced.exit_code, 0, "{:?}", forced.stdout_jsonl);
    assert_eq!(
        rig.info_json(BRIDGE)["active_version"],
        Value::String("1.0.0".to_owned())
    );
    let dependant = rig.info_json(DEPENDANT);
    assert_eq!(
        dependant["state"],
        Value::String("disabled".to_owned()),
        "the dependant must be disabled"
    );
    assert_eq!(
        dependant["disabled_reason"],
        Value::String("requires com.example.bridge".to_owned()),
        "the reason must read `requires B`"
    );
}

/// REQ: LIF-098
#[test]
fn req_lif_098_update_of_disabled_plugin_stages_a() {
    let rig = rig();
    install_ok(&rig, &notes("1.0.0"));
    let disable = rig.cli(&["plugin", "disable", NOTES]);
    assert_eq!(disable.exit_code, 0, "{:?}", disable.stdout_jsonl);

    let update = rig.update(&notes("1.1.0"), &[]);
    assert_eq!(update.exit_code, 0, "{:?}", update.stdout_jsonl);
    let info = rig.info_json(NOTES);
    assert_eq!(
        info["update"]["state"],
        Value::String("staged".to_owned()),
        "an update of a disabled plugin stops at staged"
    );
    assert_eq!(info["active_version"], Value::String("1.0.0".to_owned()));

    let enable = rig.cli(&["plugin", "enable", NOTES]);
    assert_eq!(enable.exit_code, 0, "{:?}", enable.stdout_jsonl);
    assert_eq!(
        rig.info_json(NOTES)["active_version"],
        Value::String("1.1.0".to_owned()),
        "enable must run the swap, migration and trial"
    );
}

/// REQ: LIF-098
#[test]
fn req_lif_098_update_does_not_enable_disabled_plugin_b() {
    let rig = rig();
    install_ok(&rig, &notes("1.0.0"));
    let disable = rig.cli(&["plugin", "disable", NOTES]);
    assert_eq!(disable.exit_code, 0, "{:?}", disable.stdout_jsonl);

    let update = rig.update(&notes("1.1.0"), &[]);
    assert_eq!(update.exit_code, 0, "{:?}", update.stdout_jsonl);
    let info = rig.info_json(NOTES);
    assert_eq!(
        info["state"],
        Value::String("installed-disabled-by-user".to_owned()),
        "the update must not enable the plugin by itself"
    );
    assert!(
        !rig.log_codes().iter().any(|code| code.contains("activate")),
        "no activate call may happen during a staged update"
    );
}

/// REQ: LIF-099
#[test]
fn req_lif_099_restore_backup_copies_and_renames_a() {
    let rig = rig();
    install_ok(&rig, &notes("1.0.0"));
    rig.write_data_file(NOTES, "notes.txt", b"first draft");
    let server = rig.server("main", "/project-a", "s1");
    server.activate(NOTES);
    let backup_name = rig
        .backups(NOTES)
        .first()
        .cloned()
        .expect("an upgrade must leave a backup");
    let backup_files = rig.backup_data_files(NOTES, &backup_name);

    let output = rig.rollback(NOTES, &["--restore-backup"]);
    assert_eq!(output.exit_code, 0, "{:?}", output.stdout_jsonl);
    assert_eq!(
        data_hashes(&rig, NOTES),
        backup_files,
        "the restored data must match the backup byte for byte"
    );
    let verified = output
        .stdout_jsonl
        .iter()
        .any(|line| line["files"].is_u64() && line["bytes"].is_u64());
    assert!(verified, "the copy must report verified file and byte counts");
}

/// REQ: LIF-099
#[test]
fn req_lif_099_restore_backup_disk_full_keeps_data_b() {
    let rig = rig();
    install_ok(&rig, &notes("1.0.0"));
    rig.write_data_file(NOTES, "notes.txt", b"current data");
    let server = rig.server("main", "/project-a", "s1");
    server.activate(NOTES);

    let before = data_hashes(&rig, NOTES);
    let registry_before = rig.registry_json();
    rig.fill_disk();

    let output = rig.rollback(NOTES, &["--restore-backup"]);
    rig.free_disk();

    assert_ne!(output.exit_code, 0, "a full disk must refuse the restore");
    let codes = error_codes(&output);
    assert!(
        codes.contains(&"install.no-space".to_owned()) || codes.contains(&"install.io".to_owned()),
        "expected install.no-space or install.io: {codes:?}"
    );
    assert_eq!(data_hashes(&rig, NOTES), before, "current data must stay intact");
    assert!(
        data_hashes(&rig, NOTES)
            .keys()
            .all(|name| !name.contains(".tmp") && !name.contains("restore")),
        "no temporary restore directory may remain"
    );
    assert_eq!(rig.registry_json(), registry_before, "the registry must not change");
    assert!(
        !rig.log_codes().iter().any(|code| code.contains("deactivate")),
        "no deactivate may happen before the copy succeeded"
    );
}

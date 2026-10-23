//! Trial, uninstall retention and rollback safety (LIF-957, LIF-958, LIF-959, LIF-976, LIF-977).
//!
//! The tests drive the `ilium_plugin_host::testing::ops` rig: the virtual clock, the CLI for
//! `doctor`, `uninstall` and `rollback`, and the server handle for deactivate calls and
//! kill-at-stage faults.

use std::time::Duration;

use crate::common::*;

/// A whole number of days on the virtual clock.
fn days(count: u64) -> Duration {
    Duration::from_secs(count * 24 * 60 * 60)
}

/// A lazy plugin: its candidate activates only when the named command runs, so a trial has no
/// trigger to end it.
fn lazy_pkg(id: &str, version: &str) -> PackageSpec {
    pkg(id, version).activation_trigger("on-command:acme.run")
}

/// REQ: LIF-957
#[test]
fn lif_957_trial_older_than_thirty_days_is_a_doctor_warning_with_update_trial_old() {
    let rig = rig();
    install_ok(&rig, &lazy_pkg(NOTES, "1.0.0"));
    update(&rig, &lazy_pkg(NOTES, "2.0.0"));
    rig.advance(days(31));
    let output = rig.cli(&["doctor"]);
    assert!(
        warning_codes(&output).iter().any(|code| code == "update.trial-old"),
        "a trial older than 30 days must raise the update.trial-old warning"
    );
}

/// REQ: LIF-957
#[test]
fn lif_957_no_automatic_removal_of_candidate_or_previous_version_during_a_trial() {
    let rig = rig();
    install_ok(&rig, &lazy_pkg(NOTES, "1.0.0"));
    update(&rig, &lazy_pkg(NOTES, "2.0.0"));
    rig.advance(days(29));
    assert!(
        !warning_codes(&rig.cli(&["doctor"]))
            .iter()
            .any(|code| code == "update.trial-old"),
        "no trial warning before 30 days"
    );
    rig.advance(days(2));
    let versions = store_versions(&rig, NOTES);
    assert!(
        versions.iter().any(|version| version == "1.0.0")
            && versions.iter().any(|version| version == "2.0.0"),
        "both the previous and the candidate version stay in store/<id>/, found {versions:?}"
    );
}

/// REQ: LIF-958
#[test]
fn lif_958_kept_data_of_an_uninstalled_plugin_survives_automatic_time_and_restart() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.write_setting(NOTES, "density", "compact");
    rig.write_storage(NOTES, "global", "draft", b"kept");
    let uninstall = rig.cli(&["uninstall", NOTES]);
    assert_eq!(uninstall.exit_code, 0, "uninstall without --purge must succeed");
    rig.advance(days(730));
    let _server = rig.start_server("default");
    assert!(rig.data_dir(NOTES).exists(), "data/<id>/ must stay after two years");
    assert!(
        rig.storage_db_path(NOTES).exists(),
        "storage/<id>/ must stay after two years"
    );
    assert_eq!(rig.setting(NOTES, "density").as_deref(), Some("compact"));
    let doctor = rig.cli(&["doctor"]);
    assert!(
        doctor
            .stdout_jsonl
            .iter()
            .any(|line| line.to_string().contains("orphan") && line.to_string().contains(NOTES)),
        "ilium plugin doctor must list the kept data of {NOTES} as orphaned with its size"
    );
}

/// REQ: LIF-958
#[test]
fn lif_958_only_purge_removes_the_kept_data_of_an_uninstalled_plugin() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.write_storage(NOTES, "global", "draft", b"kept");
    let uninstall = rig.cli(&["uninstall", NOTES, "--purge"]);
    assert_eq!(uninstall.exit_code, 0, "uninstall --purge must succeed");
    rig.advance(days(730));
    let _server = rig.start_server("default");
    assert!(
        !rig.data_dir(NOTES).exists(),
        "uninstall --purge is the one path that removes data/<id>/"
    );
}

/// REQ: LIF-959
#[test]
fn lif_959_rollback_refuses_package_corrupt_and_deactivates_nothing_on_a_flipped_byte() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("default");
    update(&rig, &pkg(NOTES, "2.0.0"));
    server.apply_pending();
    let previous_dir = rig.plugins_root().join("store").join(NOTES).join("1.0.0");
    let (relative_path, contents) = snapshot_tree(&previous_dir)
        .into_iter()
        .find(|(path, _)| !path.ends_with("manifest.sha256"))
        .expect("the stored previous version has at least one file");
    assert!(!contents.is_empty(), "the flipped file must have a byte to flip");
    let mut flipped = contents.clone();
    flipped[0] ^= 0x01;
    rig.corrupt_file(&previous_dir.join(relative_path), &flipped);
    let deactivations_before = server.deactivations(NOTES).len();
    let output = rig.cli(&["rollback", NOTES]);
    assert!(
        error_codes(&output).iter().any(|code| code == "package.corrupt"),
        "a stored file that fails manifest.sha256 must refuse rollback with package.corrupt"
    );
    assert_eq!(output.exit_code, 1, "package.corrupt exits 1");
    assert_eq!(
        registry_entry(&rig, NOTES)["active_version"],
        "2.0.0",
        "active_version must stay unchanged"
    );
    assert_eq!(
        server.deactivations(NOTES).len(),
        deactivations_before,
        "no instance may be deactivated by a refused rollback"
    );
}

/// REQ: LIF-959
#[test]
fn lif_959_rollback_with_an_intact_previous_version_proceeds() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("default");
    update(&rig, &pkg(NOTES, "2.0.0"));
    server.apply_pending();
    let output = rig.cli(&["rollback", NOTES]);
    assert!(
        error_codes(&output).is_empty(),
        "an intact previous version must not be refused by the manifest check"
    );
    assert_eq!(registry_entry(&rig, NOTES)["active_version"], "1.0.0");
}

/// REQ: LIF-976
#[test]
fn lif_976_failed_rollback_on_a_full_disk_leaves_registry_and_active_version_unchanged() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("default");
    update(&rig, &pkg(NOTES, "2.0.0"));
    server.apply_pending();
    let registry_before = rig.registry_bytes();
    rig.fill_disk();
    let output = rig.cli(&["rollback", NOTES]);
    assert!(
        error_codes(&output).iter().any(|code| code == "install.no-space"),
        "a full disk must report install.no-space"
    );
    assert_eq!(
        rig.registry_bytes(),
        registry_before,
        "a rollback that fails before its first deactivate(upgrade) leaves the registry unchanged"
    );
    assert_eq!(registry_entry(&rig, NOTES)["active_version"], "2.0.0");
}

/// REQ: LIF-976
#[test]
fn lif_976_rollback_writes_no_file_other_than_the_registry() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server("default");
    update(&rig, &pkg(NOTES, "2.0.0"));
    server.apply_pending();
    let before = snapshot_tree(&rig.plugins_root());
    let output = rig.cli(&["rollback", NOTES]);
    assert_eq!(output.exit_code, 0, "a plain rollback must succeed");
    let after = snapshot_tree(&rig.plugins_root());
    let changed: Vec<String> = before
        .iter()
        .filter(|entry| !after.contains(*entry))
        .chain(after.iter().filter(|entry| !before.contains(*entry)))
        .map(|(path, _)| path.clone())
        .collect();
    assert!(
        changed.iter().all(|path| path.ends_with("installed.json")),
        "only the registry may change, but these changed: {changed:?}"
    );
}

/// REQ: LIF-977
#[test]
fn lif_977_rollback_killed_mid_deactivate_leaves_exactly_one_active_version_after_restart() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server_manual("s1");
    update(&rig, &pkg(NOTES, "2.0.0"));
    server.apply_pending();
    server.pause_at(UpdateStage::Deactivated);
    rig.cli(&["rollback", NOTES]);
    server.apply_pending();
    server.crash();
    let fresh = rig.start_server("s1");
    rig.advance(secs(10));
    let version = fresh.active_version(NOTES);
    assert!(
        version.as_deref() == Some("1.0.0") || version.as_deref() == Some("2.0.0"),
        "exactly one of the two versions must be active after the restart, found {version:?}"
    );
}

/// REQ: LIF-977
#[test]
fn lif_977_the_interrupted_rollback_leaves_no_update_state_after_restart() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let server = rig.start_server_manual("s1");
    update(&rig, &pkg(NOTES, "2.0.0"));
    server.apply_pending();
    server.pause_at(UpdateStage::Deactivated);
    rig.cli(&["rollback", NOTES]);
    server.apply_pending();
    server.crash();
    let _fresh = rig.start_server("s1");
    rig.advance(secs(10));
    assert!(
        registry_entry(&rig, NOTES)["update"].is_null(),
        "the rollback is finished or undone as an update in trial, so no update state remains"
    );
}

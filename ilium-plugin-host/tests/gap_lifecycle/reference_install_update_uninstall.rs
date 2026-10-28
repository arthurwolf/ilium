//! Gap tests for the install, update and uninstall reference: update cancel, superseding, review
//! expiry, error catalogue codes, dev leases, uninstall deadlines, read-only roots and install
//! limits (LIF-707, 711, 713, 716, 717, 725, 726).
//!
//! Assumptions: the store is `<plugins root>/store/<id>/<version>/`; `registry_json()["plugins"][id]`
//! holds the `update` field; `deactivate_hangs` makes an instance never return from deactivate;
//! `make_plugins_root_read_only` chmods the root 0500; `lease_period` of a dev session is 10 s.

use std::path::Path;

use crate::support::*;

/// The version directory names under the store of one plugin id.
fn store_versions(rig: &OpsRig, id: &str) -> Vec<String> {
    let dir = rig.plugins_root().join("store").join(id);
    match std::fs::read_dir(&dir) {
        Ok(entries) => entries
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// The `update` field of one plugin in the registry.
fn registry_update_field(rig: &OpsRig, id: &str) -> serde_json::Value {
    rig.registry_json()["plugins"][id]["update"].clone()
}

/// REQ: LIF-707
#[test]
fn lif_707_update_cancel_deletes_candidate_and_exits_zero_when_nothing_is_staged() {
    assert_manual_says(
        "reference-install-update-uninstall",
        "`ilium plugin update-cancel <id>` (and the Decline action of the review dialog) MUST delete",
    );
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let nothing = rig.cli(&["plugin", "update-cancel", NOTES]);
    assert_eq!(nothing.exit_code, 0, "no staged update exits 0");
    assert_eq!(nothing.stdout_jsonl[0]["changed"], false);
    update(&rig, &pkg(NOTES, "2.0.0"));
    let cancelled = rig.cli(&["plugin", "update-cancel", NOTES]);
    assert_eq!(cancelled.exit_code, 0, "cancelling a staged update exits 0");
    assert!(
        !store_versions(&rig, NOTES).contains(&"2.0.0".to_string()),
        "the candidate directory is deleted"
    );
    assert!(
        registry_update_field(&rig, NOTES).is_null(),
        "update is set to null"
    );
}

/// REQ: LIF-708
#[test]
fn lif_708_update_to_c_replaces_staged_b_and_is_refused_while_migrating() {
    assert_manual_says(
        "reference-install-update-uninstall",
        "An update to version C while B is `staged` or `update-staged` MUST replace B",
    );
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    update(&rig, &pkg(NOTES, "2.0.0"));
    let replaced = update(&rig, &pkg(NOTES, "3.0.0"));
    assert_eq!(replaced.exit_code, 0, "update to C replaces staged B");
    assert!(
        !store_versions(&rig, NOTES).contains(&"2.0.0".to_string()),
        "B is deleted"
    );
    assert!(
        store_versions(&rig, NOTES).contains(&"3.0.0".to_string()),
        "C is written"
    );
    let superseded = rig
        .log_codes()
        .iter()
        .filter(|code| code.as_str() == "update.superseded")
        .count();
    assert_eq!(
        superseded, 1,
        "one update.superseded log naming both versions"
    );
    let server = rig.start_server("default");
    server.pause_at(UpdateStage::Migrating);
    let busy = update(&rig, &pkg(NOTES, "4.0.0"));
    assert_eq!(
        busy.exit_code, 4,
        "an update during migration returns exit 4"
    );
    assert!(error_codes(&busy).contains(&"install.update-busy".to_string()));
}

/// REQ: LIF-711
#[test]
fn lif_711_update_staged_candidate_expires_after_30_days_and_is_logged() {
    assert_manual_says(
        "reference-install-update-uninstall",
        "A package in `update-staged` MUST remain staged for at most 30 days",
    );
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    update(&rig, &pkg(NOTES, "2.0.0").grant("net.http"));
    assert_eq!(list_row(&rig, NOTES)["state"], "update-staged");
    rig.jump_wall_clock(31 * 86_400);
    let server = rig.start_server("default");
    assert!(!store_versions(&rig, NOTES).contains(&"2.0.0".to_string()));
    assert_eq!(log_count(&server, "update.review-expired"), 1);
}

/// REQ: LIF-713
#[test]
fn lif_713_four_codes_are_returned_for_their_situations_with_their_exit_codes() {
    assert_manual_says(
        "reference-install-update-uninstall",
        "The error codes `install.no-previous`, `rollback.no-backup`, `migration.data-newer` and `migration.interrupted` MUST each appear",
    );
    let first_install = rig();
    install_ok(&first_install, &pkg(NOTES, "1.0.0"));
    let no_previous = first_install.cli(&["rollback", NOTES]);
    assert_eq!(no_previous.exit_code, 5, "install.no-previous exits 5");
    assert!(error_codes(&no_previous).contains(&"install.no-previous".to_string()));

    let no_backup = rig();
    install_ok(&no_backup, &pkg(NOTES, "1.0.0"));
    update(&no_backup, &pkg(NOTES, "2.0.0"));
    no_backup.delete_backups(NOTES);
    let gone = no_backup.cli(&["rollback", NOTES, "--restore-backup"]);
    assert_eq!(gone.exit_code, 1, "rollback.no-backup exits 1");
    assert!(error_codes(&gone).contains(&"rollback.no-backup".to_string()));

    let newer_data = rig();
    install_ok(&newer_data, &pkg(NOTES, "1.0.0").data_version(3));
    let older = install(&newer_data, &pkg(NOTES, "2.0.0").data_version(2));
    assert_eq!(older.exit_code, 1, "migration.data-newer exits 1");
    assert!(error_codes(&older).contains(&"migration.data-newer".to_string()));

    let interrupted = rig();
    install_ok(&interrupted, &pkg(NOTES, "1.0.0"));
    interrupted.plant_interrupted_migrations(NOTES, 2);
    let twice = interrupted.cli(&["plugin", "start", NOTES]);
    assert_eq!(twice.exit_code, 1, "migration.interrupted exits 1");
    assert!(error_codes(&twice).contains(&"migration.interrupted".to_string()));
}

/// REQ: LIF-716
#[test]
fn lif_716_uninstall_is_refused_while_a_dev_lease_holds_the_id_and_proceeds_after_expiry() {
    assert_manual_says(
        "reference-install-update-uninstall",
        "`ilium plugin uninstall <id>` while an `ilium plugin dev` lease (LIF-229) shadows the id",
    );
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let dev = rig.dev_start(&pkg(NOTES, "1.0.1"), &[]);
    let refused = rig.cli(&["plugin", "uninstall", NOTES]);
    assert_eq!(refused.exit_code, 4, "uninstall with a dev lease exits 4");
    assert!(error_codes(&refused).contains(&"install.dev-attached".to_string()));
    assert!(
        refused.stderr.contains(&dev.attached_session()),
        "the refusal names the dev session"
    );
    dev.kill_without_cleanup();
    rig.advance(secs(30));
    let removed = rig.cli(&["plugin", "uninstall", NOTES]);
    assert_eq!(
        removed.exit_code, 0,
        "uninstall proceeds after the lease expired"
    );
}

/// REQ: LIF-717
#[test]
fn lif_717_uninstall_drops_an_instance_that_misses_the_deadline_and_removes_within_10_s() {
    assert_manual_says(
        "reference-install-update-uninstall",
        "During uninstall the host MUST wait at most the deactivate deadline per instance",
    );
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0").deactivate_hangs());
    let servers: Vec<ServerHandle> = (0..4)
        .map(|index| rig.start_server(&format!("session-{index}")))
        .collect();
    let started = rig.virtual_elapsed();
    let output = rig.cli(&["plugin", "uninstall", NOTES]);
    assert_eq!(output.exit_code, 0, "uninstall completes");
    let elapsed = rig.virtual_elapsed() - started;
    assert!(
        elapsed < secs(10),
        "uninstall of 4 instances takes under 10 s, took {elapsed:?}"
    );
    assert_eq!(log_count(&servers[0], "deactivate.deadline"), 1);
    assert!(
        !rig.plugins_root().join("store").join(NOTES).exists(),
        "files are deleted only after every instance is gone"
    );
}

/// REQ: LIF-725
#[test]
fn lif_725_install_on_read_only_root_fails_at_step_1_and_writes_nothing() {
    assert_manual_says(
        "reference-install-update-uninstall",
        "An install, update or uninstall on a read-only plugin root MUST fail at step 1",
    );
    let rig = rig();
    rig.make_plugins_root_read_only();
    let before = snapshot_names(rig.plugins_root().as_path());
    let output = install(&rig, &pkg(NOTES, "1.0.0"));
    assert_eq!(output.exit_code, 1, "install.read-only exits 1");
    assert!(error_codes(&output).contains(&"install.read-only".to_string()));
    assert_eq!(
        snapshot_names(rig.plugins_root().as_path()),
        before,
        "nothing is written"
    );
}

/// REQ: LIF-725
#[test]
fn lif_725_read_only_registry_at_start_runs_plugins_and_refuses_lifecycle_writes() {
    assert_manual_says(
        "reference-install-update-uninstall",
        "A host that finds the registry read-only at start MUST run the plugins in it",
    );
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.make_plugins_root_read_only();
    let server = rig.start_server("default");
    assert_eq!(server.active_version(NOTES).as_deref(), Some("1.0.0"));
    let refused = update(&rig, &pkg(NOTES, "2.0.0"));
    assert_eq!(refused.exit_code, 1, "lifecycle writes are refused");
}

/// Names of the direct entries of a directory, sorted.
fn snapshot_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok())
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

/// REQ: LIF-726
#[test]
fn lif_726_install_refused_past_256_plugins_and_past_2_gib_per_plugin() {
    assert_manual_says(
        "reference-install-update-uninstall",
        "The host MUST refuse an install that would make the number of installed plugins exceed 256",
    );
    let rig = rig();
    for index in 0..256 {
        install_ok(&rig, &pkg(&format!("com.example.p{index:03}"), "1.0.0"));
    }
    let too_many = install(&rig, &pkg("com.example.p256", "1.0.0"));
    assert!(error_codes(&too_many).contains(&"install.too-many".to_string()));

    let large = crate::support::rig();
    install_ok(&large, &pkg(NOTES, "1.0.0").padding(1_500 * 1024 * 1024));
    let second = install(&large, &pkg(NOTES, "1.1.0").padding(1_500 * 1024 * 1024));
    assert!(
        error_codes(&second).contains(&"install.store-too-large".to_string()),
        "a second 1.5 GiB version of one plugin exceeds the 2 GiB store cap"
    );
}

/// REQ: LIF-707
#[test]
fn lif_707_update_cancel_is_refused_with_update_in_progress_during_trial() {
    assert_manual_says(
        "reference-install-update-uninstall",
        "It MUST refuse with `update.in-progress` (exit 1) once `update.state` is `migrating` or `trial`.",
    );
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    update(&rig, &pkg(NOTES, "2.0.0"));
    let server = rig.start_server("default");
    server.pause_at(UpdateStage::Trial);
    let refused = rig.cli(&["plugin", "update-cancel", NOTES]);
    assert_eq!(refused.exit_code, 1, "update-cancel during trial exits 1");
    assert!(error_codes(&refused).contains(&"update.in-progress".to_string()));
    assert!(
        store_versions(&rig, NOTES).contains(&"2.0.0".to_string()),
        "the candidate is kept while the trial runs"
    );
}

/// REQ: LIF-708
#[test]
fn lif_708_update_to_c_replaces_update_staged_b_and_keeps_the_grant_request_of_c() {
    assert_manual_says(
        "reference-install-update-uninstall",
        "An update to version C while B is `staged` or `update-staged` MUST replace B",
    );
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    update(&rig, &pkg(NOTES, "2.0.0").grant("net.http"));
    assert_eq!(list_row(&rig, NOTES)["state"], "update-staged");
    let replaced = update(&rig, &pkg(NOTES, "3.0.0"));
    assert_eq!(
        replaced.exit_code, 0,
        "update to C replaces update-staged B"
    );
    assert!(!store_versions(&rig, NOTES).contains(&"2.0.0".to_string()));
    assert!(store_versions(&rig, NOTES).contains(&"3.0.0".to_string()));
    let server = rig.start_server("default");
    server.pause_at(UpdateStage::Trial);
    let busy = update(&rig, &pkg(NOTES, "4.0.0"));
    assert_eq!(busy.exit_code, 4, "an update during trial returns exit 4");
    assert!(error_codes(&busy).contains(&"install.update-busy".to_string()));
}

/// REQ: LIF-711
#[test]
fn lif_711_update_staged_candidate_is_still_staged_at_29_days_with_no_expiry_log() {
    assert_manual_says(
        "reference-install-update-uninstall",
        "A package in `update-staged` MUST remain staged for at most 30 days",
    );
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    update(&rig, &pkg(NOTES, "2.0.0").grant("net.http"));
    rig.jump_wall_clock(29 * 86_400);
    let server = rig.start_server("default");
    assert!(
        store_versions(&rig, NOTES).contains(&"2.0.0".to_string()),
        "the candidate is kept before the 30 day limit"
    );
    assert_eq!(list_row(&rig, NOTES)["state"], "update-staged");
    assert_eq!(log_count(&server, "update.review-expired"), 0);
}

/// REQ: LIF-713
#[test]
fn lif_713_each_situation_returns_only_its_own_error_code() {
    assert_manual_says(
        "reference-install-update-uninstall",
        "and no other code MUST be returned for these situations",
    );
    let first_install = rig();
    install_ok(&first_install, &pkg(NOTES, "1.0.0"));
    let no_previous = first_install.cli(&["rollback", NOTES]);
    assert_eq!(
        error_codes(&no_previous),
        vec!["install.no-previous".to_string()],
        "rollback without a previous version returns only install.no-previous"
    );

    let no_backup = rig();
    install_ok(&no_backup, &pkg(NOTES, "1.0.0"));
    update(&no_backup, &pkg(NOTES, "2.0.0"));
    no_backup.delete_backups(NOTES);
    let gone = no_backup.cli(&["rollback", NOTES, "--restore-backup"]);
    assert_eq!(
        error_codes(&gone),
        vec!["rollback.no-backup".to_string()],
        "a missing backup returns only rollback.no-backup"
    );
}

/// REQ: LIF-716
#[test]
fn lif_716_refused_uninstall_leaves_the_store_and_the_installed_row_unchanged() {
    assert_manual_says(
        "reference-install-update-uninstall",
        "`ilium plugin uninstall <id>` while an `ilium plugin dev` lease (LIF-229) shadows the id",
    );
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let _dev = rig.dev_start(&pkg(NOTES, "1.0.1"), &[]);
    let refused = rig.cli(&["plugin", "uninstall", NOTES]);
    assert_eq!(refused.exit_code, 4);
    assert!(
        rig.plugins_root().join("store").join(NOTES).exists(),
        "the store is not touched by a refused uninstall"
    );
    assert_eq!(list_row(&rig, NOTES)["state"], "installed-enabled");
}

/// REQ: LIF-717
#[test]
fn lif_717_every_instance_is_gone_after_uninstall_completes() {
    assert_manual_says(
        "reference-install-update-uninstall",
        "MUST NOT delete files until every instance of the id is gone",
    );
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0").deactivate_hangs());
    let servers: Vec<ServerHandle> = (0..4)
        .map(|index| rig.start_server(&format!("session-{index}")))
        .collect();
    let output = rig.cli(&["plugin", "uninstall", NOTES]);
    assert_eq!(output.exit_code, 0, "uninstall completes");
    assert!(
        servers
            .iter()
            .all(|server| server.active_version(NOTES).is_none()),
        "no instance of the id is still active"
    );
    assert!(
        list_rows(&rig).iter().all(|row| row["id"] != NOTES),
        "the id is removed from the plugin list"
    );
}

/// REQ: LIF-726
#[test]
fn lif_726_refused_257th_install_leaves_exactly_256_plugins_installed() {
    assert_manual_says(
        "reference-install-update-uninstall",
        "The host MUST refuse an install that would make the number of installed plugins exceed 256",
    );
    let rig = rig();
    for index in 0..256 {
        install_ok(&rig, &pkg(&format!("com.example.p{index:03}"), "1.0.0"));
    }
    let too_many = install(&rig, &pkg("com.example.p256", "1.0.0"));
    assert_eq!(too_many.exit_code, 1, "install.too-many exits 1");
    assert_eq!(
        list_rows(&rig).len(),
        256,
        "the refused plugin is not installed"
    );
    assert!(!rig
        .plugins_root()
        .join("store")
        .join("com.example.p256")
        .exists());
}

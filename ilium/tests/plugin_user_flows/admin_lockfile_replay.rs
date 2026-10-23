//! Administrator lockfile replay (USR-242, USR-244): `ilium plugin sync --locked`.

use crate::support::*;

/// REQ: USR-242
#[test]
fn usr_242_sync_locked_removes_plugins_that_are_not_in_the_lockfile() {
    let mut rig = UserRig::new();
    rig.open_project("lock-replay");
    install_ok(&mut rig, &spec("com.example.keep", "1.0.0", &[]));
    assert_eq!(result_exit_code(&rig.cli(&["lock"])), 0);
    install_ok(&mut rig, &spec("com.example.extra", "1.0.0", &[]));
    let output = rig.cli(&["sync", "--locked"]);
    assert_eq!(result_exit_code(&output), 0, "{:?}", error_codes(&output));
    let ids: Vec<String> = rig.list().into_iter().map(|row| row.id).collect();
    assert_eq!(ids, vec!["com.example.keep".to_string()]);
}

/// REQ: USR-242
#[test]
fn usr_242_sync_locked_installs_a_missing_entry_at_the_pinned_version() {
    let mut rig = UserRig::new();
    rig.open_project("lock-missing");
    install_ok(&mut rig, &spec("com.example.keep", "1.0.0", &[]));
    assert_eq!(result_exit_code(&rig.cli(&["lock"])), 0);
    assert_eq!(
        result_exit_code(&rig.cli(&["uninstall", "com.example.keep", "--yes"])),
        0
    );
    let output = rig.cli(&["sync", "--locked"]);
    assert_eq!(result_exit_code(&output), 0, "{:?}", error_codes(&output));
    assert_eq!(list_row(&mut rig, "com.example.keep").version, "1.0.0");
}

/// REQ: USR-242
#[test]
fn usr_242_a_corrupt_hash_fails_with_lock_mismatch_and_changes_nothing() {
    let mut rig = UserRig::new();
    let project = rig.open_project("lock-corrupt");
    install_ok(&mut rig, &spec("com.example.keep", "1.0.0", &[]));
    assert_eq!(result_exit_code(&rig.cli(&["lock"])), 0);
    install_ok(&mut rig, &spec("com.example.extra", "1.0.0", &[]));
    let lock_path = project.join("ilium-plugins.lock");
    let text = std::fs::read_to_string(&lock_path).expect("lock file");
    let mut lock: serde_json::Value = serde_json::from_str(&text).expect("lock json");
    lock["plugins"][0]["sha256"] = serde_json::Value::String("0".repeat(64));
    std::fs::write(&lock_path, lock.to_string()).expect("write corrupt lock");
    let before = tree_snapshot(&rig.data_dir());
    let output = rig.cli(&["sync", "--locked"]);
    assert!(error_codes(&output).contains(&"lock.mismatch".to_string()));
    assert_eq!(tree_snapshot(&rig.data_dir()), before);
    let ids: Vec<String> = rig.list().into_iter().map(|row| row.id).collect();
    assert!(ids.contains(&"com.example.extra".to_string()));
}

/// REQ: USR-244
#[test]
fn usr_244_a_replayed_grant_the_policy_forbids_is_left_off_and_reported() {
    let mut rig = UserRig::new();
    rig.open_project("lock-policy");
    install_ok(
        &mut rig,
        &spec("com.example.clip", "1.0.0", &["clipboard.read"]),
    );
    assert_eq!(result_exit_code(&rig.cli(&["lock"])), 0);
    rig.write_system_policy("version = 1\n[grants]\ndeny = [\"clipboard.read\"]\n");
    assert_eq!(
        result_exit_code(&rig.cli(&["uninstall", "com.example.clip", "--yes"])),
        0
    );
    let output = rig.cli(&["sync", "--locked"]);
    let entries = rig.grant_entries("com.example.clip");
    assert!(entries
        .iter()
        .all(|entry| !(entry.grant == "clipboard.read" && entry.state == "granted")));
    assert!(entries.iter().all(|entry| entry.grant == "clipboard.read"));
    let shown = format!("{:?} {}", output.stdout_jsonl, rig.notices().join(" "));
    assert!(shown.contains("Blocked by your administrator"), "{shown}");
}

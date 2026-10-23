//! Core-plugin gap tests for `reference-persistence-and-restart`: the session lock for two
//! servers on one project, serialised atomic writes, and stale lock takeover.

use std::thread;

use ilium_plugin_host::testing::ops::OpsRig;

const NOTES: &str = "com.example.notes";
const SETTING_KEY: &str = "theme";

/// REQ: COR-100
#[test]
fn cor_100_a_second_server_on_the_same_session_is_refused_and_reports_the_owner() {
    let rig = OpsRig::new();
    let first = rig.start_server("s1");
    let snapshot_before = rig.snapshot_bytes("s1");
    let refusal = rig
        .start_server_refusal("s1")
        .expect("the second server must be refused");
    assert_eq!(
        refusal.owner_pid,
        first.owner_pid(),
        "the refusal must name the first server"
    );
    assert_eq!(
        rig.snapshot_bytes("s1"),
        snapshot_before,
        "the refused server must write nothing"
    );
}

/// REQ: COR-101
#[test]
fn cor_101_concurrent_writes_from_two_sessions_leave_a_parseable_file_with_one_complete_value() {
    let rig = OpsRig::new();
    let _first = rig.start_server("s1");
    let _second = rig.start_server("s2");
    thread::scope(|scope| {
        scope.spawn(|| rig.write_setting(NOTES, SETTING_KEY, "dark"));
        scope.spawn(|| rig.write_setting(NOTES, SETTING_KEY, "light"));
    });
    let text = rig.config_text();
    let parsed: toml::Table = text.parse().expect("the config file must always parse");
    let value = parsed
        .get(SETTING_KEY)
        .and_then(|value| value.as_str())
        .expect("the key must hold one complete value");
    assert!(
        value == "dark" || value == "light",
        "unexpected value {value:?}"
    );
}

/// REQ: COR-102
#[test]
fn cor_102_a_stale_lock_from_a_killed_server_is_taken_over_and_the_takeover_is_logged() {
    let rig = OpsRig::new();
    let first = rig.start_server("s1");
    first.kill_sigkill();
    let _second = rig.start_server("s1");
    assert!(
        rig.log_codes()
            .iter()
            .any(|code| code == "lock.stale-takeover"),
        "the stale takeover must be logged"
    );
}

/// REQ: COR-102
#[test]
fn cor_102_a_lock_held_by_a_live_process_refuses_the_takeover() {
    let rig = OpsRig::new();
    let _holder = rig.hold_lock();
    let refusal = rig
        .start_server_refusal("s1")
        .expect("a live lock holder must block the takeover");
    assert!(
        refusal.reason.contains("lock"),
        "reason must mention the lock: {refusal:?}"
    );
}

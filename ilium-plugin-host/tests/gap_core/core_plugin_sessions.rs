//! Core-plugin gap tests for `core-plugin-sessions`: while the sessions plugin is down the kernel
//! keeps writing the snapshot, and a restarted plugin never overwrites a newer kernel snapshot.

use std::time::Duration;

use ilium_plugin_host::testing::ops::OpsRig;

const SESSIONS: &str = "ilium.core.sessions";

/// REQ: COR-214
#[test]
fn cor_214_the_kernel_keeps_writing_the_snapshot_every_thirty_seconds_while_sessions_is_down() {
    let rig = OpsRig::new();
    let server = rig.start_server("s1");
    server.disable(SESSIONS);
    let first = server.snapshot_time("s1");
    rig.advance(Duration::from_secs(31));
    let second = server.snapshot_time("s1");
    assert!(
        second > first,
        "the snapshot must be rewritten within 30 seconds"
    );
}

/// REQ: COR-214
#[test]
fn cor_214_a_restarted_sessions_plugin_does_not_overwrite_a_newer_kernel_snapshot() {
    let rig = OpsRig::new();
    let server = rig.start_server("s1");
    server.disable(SESSIONS);
    rig.advance(Duration::from_secs(40));
    let newer_snapshot = rig.snapshot_bytes("s1");
    server.enable(SESSIONS);
    assert_eq!(
        rig.snapshot_bytes("s1"),
        newer_snapshot,
        "the restarted plugin must not replace the newer snapshot with older state"
    );
}

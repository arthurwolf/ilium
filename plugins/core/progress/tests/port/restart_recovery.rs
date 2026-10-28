//! Restart recovery (original ilium-server/tests/progress_restart_recovery.rs).

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::DeactivationReason;

use crate::common::*;
use crate::support::*;

/// REQ: PORT:ilium-server/tests/progress_restart_recovery.rs::monitor_survives_server_restart_and_keeps_observing
#[test]
fn port_059_a_monitor_survives_a_restart_and_keeps_observing() {
    let (mut host, pane) = host_with_pane();
    register(&mut host, pane, PROBE, &running(JOB, 10, "Working"), &[]);
    let state = host
        .deactivate(DeactivationReason::Shutdown)
        .expect("state");
    host.restart_with(state);
    expect_probe(&mut host, PROBE, &running(JOB, 20, "Still working"));
    host.settle();
    tick(&mut host, 1);
    let runs_after_restore = probe_runs(&mut host);
    feed(
        &mut host,
        PROBE,
        &[running(JOB, 30, "Later"), running(JOB, 40, "Latest")],
    );
    tick(&mut host, 1);
    tick(&mut host, 1);
    assert!(
        probe_runs(&mut host) > runs_after_restore,
        "the restored monitor keeps polling"
    );
    let record = status(&mut host, pane);
    assert_eq!(record["monitor_count"], 1);
    assert_eq!(record["monitors"][0]["monitor_health"]["state"], "healthy");
    assert_eq!(record["monitors"][0]["report"]["percent"], 40.0);
}

/// All probe outputs for a command, used with [`feed`].
pub fn feed(host: &mut FakeHost, command: &str, outputs: &[String]) {
    for output in outputs {
        expect_probe(host, command, output);
    }
}

/// REQ: PORT:ilium-server/tests/progress_restart_recovery.rs::monitor_survives_server_restart_and_keeps_observing
#[test]
fn port_059_the_restored_monitor_keeps_the_job_identity_of_its_last_report() {
    let (mut host, pane) = host_with_pane();
    register(&mut host, pane, PROBE, &running(JOB, 10, "Working"), &[]);
    let state = host
        .deactivate(DeactivationReason::Shutdown)
        .expect("state");
    host.restart_with(state);
    host.settle();
    let record = status(&mut host, pane);
    assert_eq!(record["monitors"][0]["report"]["job_id"], JOB);
    assert_eq!(record["monitor_count"], 1);
}

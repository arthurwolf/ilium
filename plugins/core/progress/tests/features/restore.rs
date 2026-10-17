//! Restore after a restart (progress-B12, progress-B13, F-061).

use crate::common::*;

const PROBE: &str = "/opt/probe.sh";
const JOB: &str = "export-412";

/// REQ: progress-B12, F-061
#[test]
fn progress_b12_deactivation_returns_state_that_restart_accepts() {
    let (mut host, pane) = host_with_pane();
    register(&mut host, pane, PROBE, &running(JOB, 10, "Working"), &[]);
    let state = host.deactivate(ilium_plugin_sdk::types::DeactivationReason::Shutdown);
    assert!(state.is_some());
}

/// REQ: progress-B12, F-061
#[test]
fn progress_b12_a_monitor_is_restored_only_after_a_preflight_probe_with_the_same_job_id() {
    let (mut host, pane) = host_with_pane();
    register(&mut host, pane, PROBE, &running(JOB, 10, "Working"), &[]);
    let state = host
        .deactivate(ilium_plugin_sdk::types::DeactivationReason::Shutdown)
        .expect("state");
    host.restart_with(state);
    assert_eq!(
        status(&mut host, pane)["monitor_count"],
        0,
        "nothing restored before the preflight probe"
    );
    expect_probe(&mut host, PROBE, &running(JOB, 20, "Still working"));
    host.settle();
    tick(&mut host, 1);
    assert_eq!(status(&mut host, pane)["monitor_count"], 1);
}

/// REQ: progress-B12
#[test]
fn progress_b12_the_restored_monitor_gets_a_fresh_monitor_id() {
    let (mut host, pane) = host_with_pane();
    let old = register(&mut host, pane, PROBE, &running(JOB, 10, "Working"), &[]);
    let state = host
        .deactivate(ilium_plugin_sdk::types::DeactivationReason::Shutdown)
        .expect("state");
    host.restart_with(state);
    expect_probe(&mut host, PROBE, &running(JOB, 20, "Still working"));
    host.settle();
    tick(&mut host, 1);
    let record = status(&mut host, pane);
    assert_ne!(record["monitors"][0]["monitor_id"].as_u64().unwrap(), old);
}

/// REQ: progress-B12
#[test]
fn progress_b12_a_probe_that_now_reports_another_job_id_is_not_restored() {
    let (mut host, pane) = host_with_pane();
    register(&mut host, pane, PROBE, &running(JOB, 10, "Working"), &[]);
    let state = host
        .deactivate(ilium_plugin_sdk::types::DeactivationReason::Shutdown)
        .expect("state");
    host.restart_with(state);
    expect_probe(&mut host, PROBE, &running("another-job", 1, "Other"));
    host.settle();
    tick(&mut host, 1);
    assert_eq!(status(&mut host, pane)["monitor_count"], 0);
}

/// REQ: progress-B12
#[test]
fn progress_b12_a_probe_that_fails_its_preflight_is_not_restored() {
    let (mut host, pane) = host_with_pane();
    register(&mut host, pane, PROBE, &running(JOB, 10, "Working"), &[]);
    let state = host
        .deactivate(ilium_plugin_sdk::types::DeactivationReason::Shutdown)
        .expect("state");
    host.restart_with(state);
    expect_exit(&mut host, PROBE, failing_exit(1));
    host.settle();
    tick(&mut host, 1);
    assert_eq!(status(&mut host, pane)["monitor_count"], 0);
}

/// REQ: progress-B13, F-061
#[test]
fn progress_b13_only_a_queued_outcome_is_sent_again_after_a_restart() {
    let (mut host, pane) = host_with_pane();
    register(&mut host, pane, PROBE, &running(JOB, 10, "Working"), &[]);
    expect_probe(&mut host, PROBE, &done(JOB, "Finished"));
    tick(&mut host, 1);
    let before = typed_texts(&host, pane).len();
    let state = host
        .deactivate(ilium_plugin_sdk::types::DeactivationReason::Shutdown)
        .expect("state");
    host.restart_with(state);
    host.process().allow_unexpected();
    tick(&mut host, 30);
    // progress-B13: only `queued` is retried after a restart; this outcome was already typed.
    assert_eq!(
        before, 1,
        "the settled outcome was typed exactly once before the restart"
    );
    assert_eq!(
        typed_texts(&host, pane).len(),
        before,
        "a typed outcome is never sent again after a restart"
    );
}

/// REQ: F-061
#[test]
fn f_061_the_upgrade_and_crash_restart_reasons_also_produce_restorable_state() {
    for reason in [
        ilium_plugin_sdk::types::DeactivationReason::Upgrade,
        ilium_plugin_sdk::types::DeactivationReason::CrashRestart,
    ] {
        let (mut host, pane) = host_with_pane();
        register(&mut host, pane, PROBE, &running(JOB, 10, "Working"), &[]);
        let state = host.deactivate(reason);
        if let Some(state) = state {
            host.restart_with(state);
        }
        assert!(host.failures().is_empty());
    }
}

/// REQ: F-061
#[test]
fn f_061_restoring_with_no_monitors_registered_leaves_the_pane_empty() {
    let (mut host, pane) = host_with_pane();
    let state = host.deactivate(ilium_plugin_sdk::types::DeactivationReason::Shutdown);
    if let Some(state) = state {
        host.restart_with(state);
    }
    assert_eq!(status(&mut host, pane)["monitor_count"], 0);
}

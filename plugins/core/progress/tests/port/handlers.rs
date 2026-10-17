//! Server request handling for the monitor (original ilium-server/src/ipc/handlers.rs).

use ilium_plugin_sdk::testing::TreeNode;
use ilium_plugin_sdk::types::DeactivationReason;
use ilium_plugin_sdk::types::NodeClass;
use serde_json::json;

use crate::common::*;
use crate::support::*;

/// REQ: PORT:ilium-server/src/ipc/handlers.rs::clear_pane_progress_monitor_stops_the_loop_and_clears_reported_progress
#[test]
fn port_060_clearing_a_monitor_stops_its_probe_loop_and_clears_its_progress() {
    let (mut host, pane) = host_with_pane();
    let id = register(&mut host, pane, PROBE, &running(JOB, 10, "W"), &[]);
    let run = progress(&mut host, pane, &["clear", "--monitor-id", &id.to_string()]);
    assert_eq!(run.exit, Some(0));
    let runs = probe_runs(&mut host);
    host.process().allow_unexpected();
    tick(&mut host, 30);
    assert_eq!(probe_runs(&mut host), runs);
    assert_eq!(status(&mut host, pane)["monitor_count"], 0);
}

/// REQ: PORT:ilium-server/src/ipc/handlers.rs::disabling_progress_monitor_setting_rejects_new_requests_and_stops_running_ones
#[test]
fn port_059_disabling_the_setting_fails_running_monitors_and_rejects_new_requests_disabled() {
    let (mut host, pane) = host_with_pane();
    let id = register(&mut host, pane, PROBE, &running(JOB, 10, "W"), &[]);
    set_kernel_setting(&mut host, "ui.progress_monitor_enabled", json!(false)).expect("accepted");
    host.settle();
    let record = status(&mut host, pane);
    assert_eq!(health_of(&record, id), "failed");
    let text = record.to_string();
    assert!(
        text.contains(
            "progress monitoring was disabled in Ilium settings; the task outcome is unknown"
        ),
        "{text}"
    );
    let run = progress(&mut host, pane, &["set", "--command", "/opt/other.sh"]);
    assert_eq!(code(&run), "disabled");
}

/// REQ: PORT:ilium-server/src/ipc/handlers.rs::held_progress_wait_takes_the_outcome_instead_of_the_composer
#[test]
fn port_060_a_held_wait_takes_the_outcome_and_a_dead_waiter_leaves_it_to_the_composer() {
    let (mut host, pane) = host_with_pane();
    expect_probe(&mut host, PROBE, &running(JOB, 10, "W"));
    let open = start_in_pane(
        &mut host,
        pane,
        &["progress", "set", "--command", PROBE, "--wait"],
    );
    settle_with(&mut host, &done(JOB, "Finished"));
    assert_eq!(
        record_of(&open.read().stdout, "progress_wait")["composer_notice"],
        "suppressed"
    );
    assert!(typed_texts(&host, pane).is_empty());
    let (mut other, other_pane) = host_with_pane();
    expect_probe(&mut other, PROBE, &running(JOB, 10, "W"));
    let waiting = start_in_pane(
        &mut other,
        other_pane,
        &["progress", "set", "--command", PROBE, "--wait"],
    );
    waiting.cancel(&mut other);
    settle_with(&mut other, &done(JOB, "Finished"));
    assert_eq!(typed_texts(&other, other_pane).len(), 1);
}

/// REQ: PORT:ilium-server/src/ipc/handlers.rs::progress_set_acknowledges_only_after_snapshot_contains_the_monitor
#[test]
fn port_059_an_acknowledged_set_is_already_in_the_state_a_restart_restores() {
    let (mut host, pane) = host_with_pane();
    register(&mut host, pane, PROBE, &running(JOB, 10, "W"), &[]);
    let state = host
        .deactivate(DeactivationReason::CrashRestart)
        .expect("state");
    host.restart_with(state);
    expect_probe(&mut host, PROBE, &running(JOB, 20, "W"));
    host.settle();
    tick(&mut host, 1);
    assert_eq!(status(&mut host, pane)["monitor_count"], 1);
}

/// REQ: PORT:ilium-server/src/ipc/handlers.rs::repeated_progress_set_request_replays_one_committed_result_without_rerunning_probe
#[test]
fn port_059_the_same_probe_set_twice_returns_the_one_existing_monitor() {
    let (mut host, pane) = host_with_pane();
    let first = register(&mut host, pane, PROBE, &running(JOB, 10, "W"), &[]);
    let second = register(&mut host, pane, PROBE, &running(JOB, 10, "W"), &[]);
    assert_eq!(first, second);
    assert_eq!(status(&mut host, pane)["monitor_count"], 1);
}

/// REQ: PORT:ilium-server/src/ipc/handlers.rs::set_pane_progress_monitor_rejects_an_empty_command_and_a_non_terminal_pane
#[test]
fn port_060_set_rejects_a_blank_command_and_a_non_terminal_pane_invalid_request() {
    let (mut host, pane) = host_with_pane();
    for blank in ["", "   "] {
        let run = progress(&mut host, pane, &["set", "--command", blank]);
        assert_eq!(code(&run), "invalid-request", "{blank:?}");
    }
    let editor = host.tree().add(TreeNode {
        id: 0,
        parent: None,
        name: "notes".to_string(),
        class: NodeClass::Pane,
        content_kind: Some("ilium.editor".to_string()),
        agent: None,
    });
    let run = progress(&mut host, editor, &["set", "--command", PROBE]);
    assert_eq!(code(&run), "invalid-request");
}

/// REQ: PORT:ilium-server/src/ipc/handlers.rs::set_pane_progress_monitor_runs_the_command_and_broadcasts_reported_progress
#[test]
fn port_060_set_runs_the_command_and_reports_its_progress() {
    let (mut host, pane) = host_with_pane();
    let id = register(
        &mut host,
        pane,
        PROBE,
        &running("render-job", 10, "frame 10/100"),
        &[],
    );
    let calls = host.process().calls();
    assert_eq!(calls[0].program, "sh");
    assert_eq!(calls[0].arguments, ["-c", PROBE]);
    expect_probe(&mut host, PROBE, "{\"job_id\":\"render-job\",\"status\":\"running\",\"percent\":42.5,\"message\":\"frame 10/100\",\"details\":\"\",\"error\":null}");
    tick(&mut host, 1);
    let record = status(&mut host, pane);
    assert_eq!(record["monitors"][0]["monitor_id"], id);
    assert_eq!(record["monitors"][0]["report"]["percent"], 42.5);
    assert_eq!(record["monitors"][0]["report"]["message"], "frame 10/100");
}

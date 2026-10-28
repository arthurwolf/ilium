//! Redelivery and the supervision of a monitor (original ilium-server/src/progress_watchdog.rs).

use ilium_plugin_sdk::testing::{FakeHost, TreeNode};
use ilium_plugin_sdk::types::{AgentView, NodeClass, Outcome};

use crate::common::*;
use crate::support::*;

fn settled(host: &mut ilium_plugin_sdk::testing::FakeHost) -> u64 {
    let pane = add_agent(host, "codex", "codex");
    register(host, pane, PROBE, &running(JOB, 10, "W"), &[]);
    settle_with(host, &done(JOB, "Finished"));
    pane
}

/// REQ: PORT:ilium-server/src/progress_watchdog.rs::delivered_outcome_is_never_redelivered
#[test]
fn port_060_a_delivered_outcome_is_never_typed_again() {
    let mut host = activated_server();
    let pane = settled(&mut host);
    for request in host.pending_requests() {
        host.complete(request, Outcome::Ok);
    }
    host.settle();
    host.process().allow_unexpected();
    tick(&mut host, 200);
    assert_eq!(typed_texts(&host, pane).len(), 1);
}

/// REQ: PORT:ilium-server/src/progress_watchdog.rs::failed_evidence_is_sticky_failed_health_with_the_reason
#[test]
fn port_059_failed_health_is_sticky_and_carries_the_reason() {
    let (mut host, pane) = host_with_pane();
    let id = register(&mut host, pane, PROBE, &running(JOB, 10, "W"), &[]);
    for _ in 0..3 {
        expect_exit(&mut host, PROBE, failing_exit(1));
        tick(&mut host, 1);
    }
    host.process().allow_unexpected();
    tick(&mut host, 120);
    let record = status(&mut host, pane);
    assert_eq!(health_of(&record, id), "failed");
    assert!(!record["monitors"][0]["monitor_health"]["last_error"]
        .as_str()
        .unwrap_or_default()
        .is_empty());
}

/// REQ: PORT:ilium-server/src/progress_watchdog.rs::outcome_with_live_delivery_task_is_left_alone
#[test]
fn port_060_an_outcome_whose_delivery_is_still_in_flight_is_not_typed_twice() {
    let mut host = activated_server();
    let pane = settled(&mut host);
    assert_eq!(
        host.pending_requests().len(),
        1,
        "the delivery request is still pending"
    );
    host.process().allow_unexpected();
    pass(&mut host, 19_000);
    assert_eq!(typed_texts(&host, pane).len(), 1);
}

/// REQ: PORT:ilium-server/src/progress_watchdog.rs::outcome_without_supported_composer_is_not_redelivered
#[test]
fn port_060_an_outcome_is_typed_into_the_pane_that_registered_it_and_no_other() {
    let (mut host, pane) = host_with_pane();
    let other = add_shell(&mut host, "other");
    register(&mut host, pane, PROBE, &running(JOB, 10, "W"), &[]);
    settle_with(&mut host, &done(JOB, "Finished"));
    host.process().allow_unexpected();
    tick(&mut host, 100);
    assert!(typed_texts(&host, other).is_empty());
}

/// REQ: PORT:ilium-server/src/progress_watchdog.rs::reconcile_fails_a_monitor_whose_observation_task_vanished
#[test]
fn port_059_a_monitor_whose_observation_stopped_reports_failed_health_with_its_reason() {
    let (mut host, pane) = host_with_pane();
    let id = register(&mut host, pane, PROBE, &running(JOB, 10, "W"), &[]);
    set_kernel_setting(
        &mut host,
        "ui.progress_monitor_enabled",
        serde_json::json!(false),
    )
    .expect("accepted");
    let record = status(&mut host, pane);
    assert_eq!(health_of(&record, id), "failed");
    assert!(
        record.to_string().contains("the task outcome is unknown"),
        "{record}"
    );
}

/// REQ: PORT:ilium-server/src/progress_watchdog.rs::running_monitor_with_live_observation_task_is_left_alone
#[test]
fn port_059_a_running_monitor_with_a_live_probe_loop_stays_healthy() {
    let (mut host, pane) = host_with_pane();
    let id = register(&mut host, pane, PROBE, &running(JOB, 10, "W"), &[]);
    for percent in [20, 30, 40] {
        expect_probe(&mut host, PROBE, &running(JOB, percent, "W"));
        tick(&mut host, 21);
    }
    assert_eq!(health_of(&status(&mut host, pane), id), "healthy");
}

/// REQ: PORT:ilium-server/src/progress_watchdog.rs::running_monitor_without_observation_task_is_stopped
#[test]
fn port_059_a_monitor_that_is_no_longer_observed_makes_no_further_probe_runs() {
    let (mut host, pane) = host_with_pane();
    register(&mut host, pane, PROBE, &running(JOB, 10, "W"), &[]);
    set_kernel_setting(
        &mut host,
        "ui.progress_monitor_enabled",
        serde_json::json!(false),
    )
    .expect("accepted");
    let runs = probe_runs(&mut host);
    host.process().allow_unexpected();
    tick(&mut host, 60);
    assert_eq!(probe_runs(&mut host), runs);
}

/// REQ: PORT:ilium-server/src/progress_watchdog.rs::settled_outcome_not_yet_in_composer_is_redelivered_once
#[test]
fn port_060_an_outcome_not_yet_in_the_composer_is_typed_again_once_after_twenty_seconds() {
    let mut host = activated_server();
    let pane = settled(&mut host);
    host.process().allow_unexpected();
    pass(&mut host, 19_999);
    assert_eq!(typed_texts(&host, pane).len(), 1);
    pass(&mut host, 1);
    assert_eq!(typed_texts(&host, pane).len(), 2);
}

/// REQ: PORT:ilium-server/src/progress_watchdog.rs::uncertain_and_attempted_outcomes_are_redelivered_as_possible_duplicates
#[test]
fn port_060_an_outcome_with_an_uncertain_delivery_is_typed_again_at_the_next_reconcile() {
    let mut host = activated_server();
    let pane = settled(&mut host);
    for request in host.pending_requests() {
        host.complete(request, Outcome::Cancelled);
    }
    host.settle();
    host.process().allow_unexpected();
    pass(&mut host, 20_000);
    assert_eq!(typed_texts(&host, pane).len(), 2);
}

/// An agent pane in the fake tree; returns its node id.
pub fn add_agent(host: &mut FakeHost, name: &str, signature: &str) -> u64 {
    host.tree().add(TreeNode {
        id: 0,
        parent: None,
        name: name.to_string(),
        class: NodeClass::Pane,
        content_kind: Some("ilium.terminal".to_string()),
        agent: Some(AgentView {
            signature: signature.to_string(),
            activity: ilium_plugin_sdk::types::AgentActivity::Idle,
            objective: None,
            goal: None,
        }),
    })
}

/// REQ: PORT:ilium-server/src/progress_watchdog.rs::delivered_outcome_is_never_redelivered
#[test]
fn port_060_a_delivered_outcome_is_not_typed_again_after_ten_minutes() {
    let mut host = activated_server();
    let pane = settled(&mut host);
    for request in host.pending_requests() {
        host.complete(request, Outcome::Ok);
    }
    host.settle();
    host.process().allow_unexpected();
    pass(&mut host, 600_000);
    assert_eq!(typed_texts(&host, pane).len(), 1);
}

/// REQ: PORT:ilium-server/src/progress_watchdog.rs::failed_evidence_is_sticky_failed_health_with_the_reason
#[test]
fn port_059_failed_health_stays_failed_after_ten_minutes_without_a_probe() {
    let (mut host, pane) = host_with_pane();
    let id = register(&mut host, pane, PROBE, &running(JOB, 10, "W"), &[]);
    for _ in 0..3 {
        expect_exit(&mut host, PROBE, failing_exit(1));
        tick(&mut host, 1);
    }
    host.process().allow_unexpected();
    tick(&mut host, 600);
    assert_eq!(health_of(&status(&mut host, pane), id), "failed");
}

/// REQ: PORT:ilium-server/src/progress_watchdog.rs::outcome_with_live_delivery_task_is_left_alone
#[test]
fn port_060_a_queued_outcome_is_not_typed_twice_during_a_long_wait() {
    let mut host = activated_server();
    let pane = settled(&mut host);
    host.process().allow_unexpected();
    pass(&mut host, 60_000);
    assert_eq!(typed_texts(&host, pane).len(), 1);
}

/// REQ: PORT:ilium-server/src/progress_watchdog.rs::outcome_without_supported_composer_is_not_redelivered
#[test]
fn port_060_a_finished_monitor_stays_in_the_status_while_it_is_not_retyped() {
    let (mut host, pane) = host_with_pane();
    let id = register(&mut host, pane, PROBE, &running(JOB, 10, "W"), &[]);
    settle_with(&mut host, &done(JOB, "Finished"));
    host.process().allow_unexpected();
    tick(&mut host, 100);
    let record = status(&mut host, pane);
    assert_eq!(record["monitors"][0]["monitor_id"], id);
    assert_eq!(record["monitors"][0]["report"]["status"], "done");
}

/// REQ: PORT:ilium-server/src/progress_watchdog.rs::reconcile_fails_a_monitor_whose_observation_task_vanished
#[test]
fn port_059_disabling_the_monitor_keeps_its_last_report_readable() {
    let (mut host, pane) = host_with_pane();
    register(&mut host, pane, PROBE, &running(JOB, 10, "W"), &[]);
    set_kernel_setting(
        &mut host,
        "ui.progress_monitor_enabled",
        serde_json::json!(false),
    )
    .expect("accepted");
    let record = status(&mut host, pane);
    assert_eq!(record["monitor_count"], 1);
    assert_eq!(record["monitors"][0]["report"]["status"], "running");
}

/// REQ: PORT:ilium-server/src/progress_watchdog.rs::running_monitor_with_live_observation_task_is_left_alone
#[test]
fn port_059_a_live_probe_loop_keeps_running_probes_on_schedule() {
    let (mut host, pane) = host_with_pane();
    register(&mut host, pane, PROBE, &running(JOB, 10, "W"), &[]);
    let before = probe_runs(&mut host);
    expect_probe(&mut host, PROBE, &running(JOB, 20, "W"));
    tick(&mut host, 21);
    assert!(probe_runs(&mut host) > before);
}

/// REQ: PORT:ilium-server/src/progress_watchdog.rs::running_monitor_without_observation_task_is_stopped
#[test]
fn port_059_a_disabled_monitor_is_not_removed_from_the_status() {
    let (mut host, pane) = host_with_pane();
    register(&mut host, pane, PROBE, &running(JOB, 10, "W"), &[]);
    set_kernel_setting(
        &mut host,
        "ui.progress_monitor_enabled",
        serde_json::json!(false),
    )
    .expect("accepted");
    assert_eq!(status(&mut host, pane)["monitor_count"], 1);
}

/// REQ: PORT:ilium-server/src/progress_watchdog.rs::settled_outcome_not_yet_in_composer_is_redelivered_once
#[test]
fn port_060_a_redelivered_outcome_is_not_typed_a_third_time() {
    let mut host = activated_server();
    let pane = settled(&mut host);
    host.process().allow_unexpected();
    pass(&mut host, 20_000);
    assert_eq!(typed_texts(&host, pane).len(), 2);
    pass(&mut host, 60_000);
    assert_eq!(typed_texts(&host, pane).len(), 2);
}

/// REQ: PORT:ilium-server/src/progress_watchdog.rs::uncertain_and_attempted_outcomes_are_redelivered_as_possible_duplicates
#[test]
fn port_060_a_possible_duplicate_is_typed_with_the_same_text_as_the_first_delivery() {
    let mut host = activated_server();
    let pane = settled(&mut host);
    for request in host.pending_requests() {
        host.complete(request, Outcome::Cancelled);
    }
    host.settle();
    host.process().allow_unexpected();
    pass(&mut host, 20_000);
    let texts = typed_texts(&host, pane);
    assert_eq!(texts.len(), 2);
    assert_eq!(texts[0], texts[1]);
}

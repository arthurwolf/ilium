//! Ports of the task-outcome tests of ilium-server/src/notifications.rs and of
//! ilium-server/src/ipc/handlers.rs: which monitor reports raise which alert, when an outcome is
//! redundant, when the agent is mid-turn, the merge window, and the once-only claim.
//!
//! Monitor reports reach the plugin as `ilium.core.progress/changed` topic events (API_GAPS.md).

use ilium_plugin_sdk::types::{AgentActivity, Submission};
use serde_json::json;

use crate::common::*;

fn alerts(submissions: &[Submission]) -> (usize, usize) {
    (notices(submissions).len(), sounds(submissions).len())
}

/// REQ: PORT:ilium-server/src/notifications.rs::task_outcome_kind_follows_the_monitor_report
#[test]
fn tasks_port_a_done_report_is_a_success_alert() {
    let mut host = server_host();
    set_setting(&mut host, "notifications.task_succeeded", json!(true));
    set_setting(&mut host, "sound.events.task_succeeded", json!(true));
    let pane = add_pane(&mut host, "build-shell", None);
    assert_eq!(
        alerts(&task_outcome(&mut host, pane, "job-1", "done")),
        (1, 1)
    );
}

/// REQ: PORT:ilium-server/src/notifications.rs::task_outcome_kind_follows_the_monitor_report
#[test]
fn tasks_port_an_error_report_is_a_failure_alert() {
    let mut host = server_host();
    let pane = add_pane(&mut host, "build-shell", None);
    let found = notices(&task_outcome(&mut host, pane, "job-1", "error"));
    assert_eq!(found.len(), 1);
    assert!(found[0].title.contains("failed"), "{}", found[0].title);
}

/// REQ: PORT:ilium-server/src/notifications.rs::task_outcome_kind_follows_the_monitor_report
#[test]
fn tasks_port_a_running_report_raises_nothing() {
    let mut host = server_host();
    set_setting(&mut host, "notifications.task_succeeded", json!(true));
    let pane = add_pane(&mut host, "build-shell", None);
    assert_eq!(
        alerts(&task_outcome(&mut host, pane, "job-1", "running")),
        (0, 0)
    );
}

/// REQ: PORT:ilium-server/src/notifications.rs::task_outcomes_map_to_their_own_events
#[test]
fn tasks_port_a_lost_monitor_is_governed_by_the_failure_switches() {
    let mut host = server_host();
    let pane = add_pane(&mut host, "build-shell", None);
    assert_eq!(
        alerts(&task_outcome(&mut host, pane, "job-1", "lost")),
        (1, 1)
    );
}

/// REQ: PORT:ilium-server/src/notifications.rs::task_outcomes_map_to_their_own_events
#[test]
fn tasks_port_the_failure_switch_silences_error_and_lost_but_not_done() {
    let mut host = server_host();
    set_setting(&mut host, "notifications.task_failed", json!(false));
    set_setting(&mut host, "sound.events.task_failed", json!(false));
    set_setting(&mut host, "notifications.task_succeeded", json!(true));
    let pane = add_pane(&mut host, "build-shell", None);
    assert_eq!(
        alerts(&task_outcome(&mut host, pane, "job-1", "error")),
        (0, 0)
    );
    host.advance_clock(31_000);
    assert_eq!(
        alerts(&task_outcome(&mut host, pane, "job-2", "lost")),
        (0, 0)
    );
    host.advance_clock(31_000);
    assert_eq!(
        notices(&task_outcome(&mut host, pane, "job-3", "done")).len(),
        1
    );
}

/// REQ: PORT:ilium-server/src/notifications.rs::task_outcome_is_redundant_only_for_an_idle_or_finished_agent
#[test]
fn tasks_port_redundant_only_for_an_idle_or_finished_agent() {
    for (activity, expected) in [
        (Some(AgentActivity::Idle), (0, 0)),
        (Some(AgentActivity::Done), (0, 0)),
        (Some(AgentActivity::Working), (1, 1)),
        (None, (1, 1)),
    ] {
        let mut host = server_host();
        let pane = add_pane(&mut host, "claude-api", activity);
        assert_eq!(
            alerts(&task_outcome(&mut host, pane, "job-1", "error")),
            expected,
            "{activity:?}"
        );
    }
}

/// REQ: PORT:ilium-server/src/notifications.rs::task_outcome_is_redundant_only_for_an_idle_or_finished_agent
#[test]
fn tasks_port_switching_the_suppression_off_keeps_the_idle_agent_alert() {
    let mut host = server_host();
    set_setting(
        &mut host,
        "notifications.suppress_redundant_task_outcomes",
        json!(false),
    );
    let pane = add_pane(&mut host, "claude-api", Some(AgentActivity::Idle));
    assert_eq!(
        alerts(&task_outcome(&mut host, pane, "job-1", "error")),
        (1, 1)
    );
}

/// REQ: PORT:ilium-server/src/notifications.rs::agent_mid_turn_is_true_only_while_the_agent_is_active
#[test]
fn tasks_port_mid_turn_only_while_the_agent_is_active() {
    for (activity, mid_turn) in [
        (Some(AgentActivity::Working), true),
        (Some(AgentActivity::AwaitingApproval), true),
        (Some(AgentActivity::Idle), false),
        (None, false),
    ] {
        let mut host = server_host();
        set_setting(
            &mut host,
            "notifications.suppress_redundant_task_outcomes",
            json!(false),
        );
        set_setting(&mut host, "notifications.task_succeeded", json!(true));
        let pane = add_pane(&mut host, "pane", activity);
        let found = notices(&task_outcome(&mut host, pane, "job-1", "done"));
        assert_eq!(found.len(), 1, "{activity:?}");
        let marked = found[0].title.contains("(agent still working)");
        assert_eq!(marked, mid_turn, "{activity:?}: {}", found[0].title);
    }
}

/// REQ: PORT:ilium-server/src/notifications.rs::coalescer_merges_same_kind_outcomes_inside_the_window
#[test]
fn tasks_port_the_coalescer_merges_same_kind_outcomes_inside_the_window() {
    let mut host = server_host();
    let pane = add_pane(&mut host, "build-shell", None);
    let other = add_pane(&mut host, "other-shell", None);
    assert_eq!(
        notices(&task_outcome(&mut host, pane, "job-1", "error")).len(),
        1
    );
    host.advance_clock(10_000);
    assert!(
        notices(&task_outcome(&mut host, pane, "job-2", "error")).is_empty(),
        "same kind, inside 30 s"
    );
    // A success is never swallowed by an earlier failure, nor is another pane's outcome.
    set_setting(&mut host, "notifications.task_succeeded", json!(true));
    assert_eq!(
        notices(&task_outcome(&mut host, pane, "job-3", "done")).len(),
        1
    );
    assert_eq!(
        notices(&task_outcome(&mut host, other, "job-4", "error")).len(),
        1
    );
    host.advance_clock(21_000);
    assert_eq!(
        notices(&task_outcome(&mut host, pane, "job-5", "error")).len(),
        1,
        "31 s after the first"
    );
}

/// REQ: PORT:ilium-server/src/notifications.rs::coalescer_with_a_zero_window_admits_everything
#[test]
fn tasks_port_a_zero_window_admits_everything() {
    let mut host = server_host();
    set_setting(&mut host, "notifications.task_coalesce_seconds", json!(0));
    let pane = add_pane(&mut host, "build-shell", None);
    assert_eq!(
        notices(&task_outcome(&mut host, pane, "job-1", "error")).len(),
        1
    );
    assert_eq!(
        notices(&task_outcome(&mut host, pane, "job-2", "error")).len(),
        1
    );
}

/// REQ: PORT:ilium-server/src/ipc/handlers.rs::terminal_outcome_notification_is_once_and_plain_shell_keeps_result
#[test]
fn tasks_port_one_terminal_outcome_notifies_once_and_a_plain_shell_keeps_it() {
    let mut host = server_host();
    set_setting(&mut host, "notifications.task_succeeded", json!(true));
    let pane = add_pane(&mut host, "build-shell", None);
    assert_eq!(
        alerts(&task_outcome(&mut host, pane, "job-1", "running")),
        (0, 0)
    );
    let first = task_outcome(&mut host, pane, "job-1", "done");
    assert_eq!(notices(&first).len(), 1);
    // The same monitor reporting its terminal state again does not notify again, even with the merge
    // window off.
    set_setting(&mut host, "notifications.task_coalesce_seconds", json!(0));
    let again = task_outcome(&mut host, pane, "job-1", "done");
    assert_eq!(alerts(&again), (0, 0));
    // The plain shell is never treated as redundant: its outcome was delivered above.
}

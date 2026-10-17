//! F-049 rule 1, skip redundant outcomes: task alerts are skipped while the agent in that pane is
//! idle, finished-unread or parked; panes with no agent are never skipped
//! (src/docs/notifications.md "How task alerts are merged or skipped").

use ilium_plugin_sdk::types::AgentActivity;
use serde_json::json;

use crate::common::*;

fn failed_outcome_on(activity: Option<AgentActivity>) -> (Vec<String>, usize) {
    let mut host = server_host();
    let node = add_pane(&mut host, "claude-api", activity);
    let submissions = task_outcome(&mut host, node, "job-3", "error");
    (sounds(&submissions), notices(&submissions).len())
}

/// REQ: F-049
#[test]
fn redundant_049_a_task_alert_on_an_idle_agent_is_skipped() {
    let (played, shown) = failed_outcome_on(Some(AgentActivity::Idle));
    assert!(played.is_empty());
    assert_eq!(shown, 0);
}

/// REQ: F-049
#[test]
fn redundant_049_a_task_alert_on_a_finished_unread_agent_is_skipped() {
    let (played, shown) = failed_outcome_on(Some(AgentActivity::Done));
    assert!(played.is_empty());
    assert_eq!(shown, 0);
}

/// REQ: F-049
#[test]
fn redundant_049_a_task_alert_on_a_working_agent_is_kept() {
    let (played, shown) = failed_outcome_on(Some(AgentActivity::Working));
    assert_eq!(played.len(), 1);
    assert_eq!(shown, 1);
}

/// REQ: F-049
#[test]
fn redundant_049_a_task_alert_on_an_agent_awaiting_approval_is_kept() {
    let (played, shown) = failed_outcome_on(Some(AgentActivity::AwaitingApproval));
    assert_eq!(played.len(), 1);
    assert_eq!(shown, 1);
}

/// REQ: F-049
#[test]
fn redundant_049_a_pane_with_no_agent_is_never_skipped() {
    let (played, shown) = failed_outcome_on(None);
    assert_eq!(played.len(), 1);
    assert_eq!(shown, 1);
}

/// REQ: F-049
#[test]
fn redundant_049_the_rule_applies_to_a_lost_monitor_too() {
    let mut host = server_host();
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Idle));
    let submissions = task_outcome(&mut host, node, "job-3", "lost");
    assert!(sounds(&submissions).is_empty());
    assert!(notices(&submissions).is_empty());
}

/// REQ: F-049
#[test]
fn redundant_049_the_rule_applies_to_a_succeeded_task_too() {
    let mut host = server_host();
    set_setting(&mut host, "sound.events.task_succeeded", json!(true));
    set_setting(&mut host, "notifications.task_succeeded", json!(true));
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Idle));
    let submissions = task_outcome(&mut host, node, "job-3", "done");
    assert!(sounds(&submissions).is_empty());
    assert!(notices(&submissions).is_empty());
}

/// REQ: F-049
#[test]
fn redundant_049_turning_the_rule_off_keeps_the_alert_on_an_idle_agent() {
    let mut host = server_host();
    set_setting(
        &mut host,
        "notifications.suppress_redundant_task_outcomes",
        json!(false),
    );
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Idle));
    let submissions = task_outcome(&mut host, node, "job-3", "error");
    assert_eq!(sounds(&submissions).len(), 1);
    assert_eq!(notices(&submissions).len(), 1);
}

/// REQ: F-049
#[test]
fn redundant_049_turning_the_rule_off_keeps_the_alert_on_a_finished_agent() {
    let mut host = server_host();
    set_setting(
        &mut host,
        "notifications.suppress_redundant_task_outcomes",
        json!(false),
    );
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Done));
    assert_eq!(
        notices(&task_outcome(&mut host, node, "job-3", "error")).len(),
        1
    );
}

/// REQ: F-049
#[test]
fn redundant_049_turning_the_rule_back_on_skips_again() {
    let mut host = server_host();
    set_setting(
        &mut host,
        "notifications.suppress_redundant_task_outcomes",
        json!(false),
    );
    set_setting(
        &mut host,
        "notifications.suppress_redundant_task_outcomes",
        json!(true),
    );
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Idle));
    assert!(notices(&task_outcome(&mut host, node, "job-3", "error")).is_empty());
}

/// REQ: F-049
#[test]
fn redundant_049_a_skipped_outcome_does_not_start_a_merge_window() {
    let mut host = server_host();
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Idle));
    assert!(notices(&task_outcome(&mut host, node, "job-3", "error")).is_empty());
    host.tree().set_activity(node.0, AgentActivity::Working);
    host.advance_clock(1_000);
    assert_eq!(
        notices(&task_outcome(&mut host, node, "job-4", "error")).len(),
        1
    );
}

/// REQ: F-049
#[test]
fn redundant_049_the_agent_state_is_read_when_the_outcome_arrives() {
    let mut host = server_host();
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Working));
    assert_eq!(
        notices(&task_outcome(&mut host, node, "job-3", "error")).len(),
        1
    );
    host.tree().set_activity(node.0, AgentActivity::Idle);
    host.advance_clock(31_000);
    assert!(notices(&task_outcome(&mut host, node, "job-4", "error")).is_empty());
}

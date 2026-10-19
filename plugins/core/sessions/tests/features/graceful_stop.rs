//! F-076: stop gracefully before the final snapshot (page sample "Stop gracefully before a
//! snapshot"). Trigger: `deactivate(shutdown)` of the server side (see ../API_GAPS.md).

use ilium_plugin_sdk::types::{DeactivationReason, NodeId};

use crate::common::*;

/// REQ: F-076
#[test]
fn graceful_076_grace_period_constant_is_five_thousand_milliseconds() {
    assert_eq!(GRACE_MS, 5_000);
    assert!(page_text().contains("`grace-ms` of 5000"));
}

/// REQ: F-076
#[test]
fn graceful_076_shell_pane_gets_terminate_with_five_second_grace_at_shutdown() {
    let mut host = activated_server();
    let pane = add_shell(&mut host, "shell");
    host.deactivate(DeactivationReason::Shutdown);
    let stops = terminations(&host.submitted());
    assert_eq!(stops.len(), 1);
    assert_eq!(stops[0].0.pane, NodeId(pane));
    assert_eq!(stops[0].0.grace_ms, 5_000);
}

/// REQ: F-076
#[test]
fn graceful_076_the_grace_is_not_one_millisecond_short() {
    let mut host = activated_server();
    add_shell(&mut host, "shell");
    host.deactivate(DeactivationReason::Shutdown);
    let stops = terminations(&host.submitted());
    assert_eq!(stops.len(), 1);
    assert_eq!(stops[0].0.grace_ms, 5_000);
}

/// REQ: F-076
#[test]
fn graceful_076_terminate_submission_has_no_dedup_key() {
    let mut host = activated_server();
    add_shell(&mut host, "shell");
    host.deactivate(DeactivationReason::Shutdown);
    assert_eq!(terminations(&host.submitted())[0].1, None);
}

/// REQ: F-076
#[test]
fn graceful_076_every_pane_the_policy_does_not_relaunch_is_stopped() {
    let mut host = activated_server();
    let first = add_shell(&mut host, "one");
    let second = add_shell(&mut host, "two");
    host.deactivate(DeactivationReason::Shutdown);
    let stopped: Vec<NodeId> = terminations(&host.submitted())
        .into_iter()
        .map(|(spec, _)| spec.pane)
        .collect();
    assert_eq!(stopped, [NodeId(first), NodeId(second)]);
}

/// REQ: F-076
#[test]
fn graceful_076_nothing_is_terminated_while_the_server_runs() {
    let mut host = activated_server();
    add_shell(&mut host, "shell");
    host.deliver(vec![attached(1), detached(1)]);
    assert!(terminations(&host.submitted()).is_empty());
}

/// REQ: F-076
#[test]
fn graceful_076_no_panes_means_no_terminate_at_shutdown() {
    let mut host = activated_server();
    host.deactivate(DeactivationReason::Shutdown);
    assert!(terminations(&host.submitted()).is_empty());
}

/// REQ: F-076
#[test]
fn graceful_076_a_disable_does_not_terminate_panes() {
    let mut host = activated_server();
    add_shell(&mut host, "shell");
    host.deactivate(DeactivationReason::Disabled);
    assert!(terminations(&host.submitted()).is_empty());
}

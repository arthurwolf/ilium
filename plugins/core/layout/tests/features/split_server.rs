//! Server side of "Split four ways": create the pane first, then join with `create-split`.

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{ErrorCode, Outcome, SplitDirection};

use crate::common::*;

/// Runs one split on the server side with a focused terminal pane; returns the host, the focused
/// id and the id the harness reported for the created pane.
fn run_split(channel: &str) -> (FakeHost, u64, Option<u64>) {
    let mut host = activated_server();
    let focused = host.tree().add(pane_node("api", "ilium.terminal", None));
    host.deliver_one(message_from_client(channel, id_bytes(focused)));
    host.settle();
    let created = host.outcomes().into_iter().find_map(|outcome| match outcome {
        Outcome::OkNode(node) => Some(node.0),
        _ => None,
    });
    (host, focused, created)
}

fn check_split(channel: &str, focused_first: bool, direction: SplitDirection) {
    let (host, focused, created) = run_split(channel);
    let created = created.expect("create-pane completed with ok-node");
    let splits = create_splits(&host.submitted());
    assert_eq!(splits.len(), 1);
    let expected = if focused_first { vec![focused, created] } else { vec![created, focused] };
    let actual: Vec<u64> = splits[0].panes.iter().map(|node| node.0).collect();
    assert_eq!(actual, expected);
    assert_eq!(splits[0].direction, direction);
}

/// REQ: F-003
#[test]
fn f_003_split_right_joins_focused_then_new_pane_vertically() {
    check_split("split-right", true, SplitDirection::Vertical);
}

/// REQ: F-003
#[test]
fn f_003_split_left_joins_new_then_focused_pane_vertically() {
    check_split("split-left", false, SplitDirection::Vertical);
}

/// REQ: F-003
#[test]
fn f_003_split_down_joins_focused_then_new_pane_horizontally() {
    check_split("split-down", true, SplitDirection::Horizontal);
}

/// REQ: F-003
#[test]
fn f_003_split_up_joins_new_then_focused_pane_horizontally() {
    check_split("split-up", false, SplitDirection::Horizontal);
}

/// REQ: F-003
#[test]
fn f_003_first_submission_is_a_terminal_create_pane_and_no_split_yet() {
    let mut host = activated_server();
    let focused = host.tree().add(pane_node("api", "ilium.terminal", None));
    let submissions = host.deliver_one(message_from_client("split-right", id_bytes(focused)));
    let panes = create_panes(&submissions);
    assert_eq!(panes.len(), 1);
    assert_eq!(panes[0].content_kind, "ilium.terminal");
    assert!(create_splits(&submissions).is_empty());
}

/// REQ: F-003
#[test]
fn f_003_exactly_two_panes_are_joined_by_one_split() {
    for channel in SPLIT_CHANNELS {
        let (host, _, _) = run_split(channel);
        let splits = create_splits(&host.submitted());
        assert_eq!(splits.len(), 1, "{channel}");
        assert_eq!(splits[0].panes.len(), 2, "{channel}");
    }
}

/// REQ: F-003
#[test]
fn f_003_missing_create_pane_grant_refuses_and_no_split_follows_not_permitted() {
    let mut host = activated_server();
    let focused = host.tree().add(pane_node("api", "ilium.terminal", None));
    host.revoke("panes.lifecycle");
    host.deliver_one(message_from_client("split-right", id_bytes(focused)));
    host.settle();
    let refused = host.outcomes().into_iter().any(|outcome| match outcome {
        Outcome::Refused(error) => error.code == ErrorCode::NotPermitted,
        _ => false,
    });
    assert!(refused);
    assert!(create_splits(&host.submitted()).is_empty());
}

/// REQ: F-003
#[test]
fn f_003_missing_restructure_grant_refuses_the_split_not_permitted() {
    let mut host = activated_server();
    let focused = host.tree().add(pane_node("api", "ilium.terminal", None));
    host.revoke("tree.restructure");
    host.deliver_one(message_from_client("split-right", id_bytes(focused)));
    host.settle();
    let refused = host.outcomes().into_iter().any(|outcome| match outcome {
        Outcome::Refused(error) => error.code == ErrorCode::NotPermitted,
        _ => false,
    });
    assert!(refused);
    assert!(host.failures().is_empty());
    assert!(!host.is_disabled());
}

/// REQ: F-003
#[test]
fn f_003_a_successful_split_leaves_the_plugin_enabled_without_failures() {
    let (host, _, _) = run_split("split-right");
    assert!(host.failures().is_empty());
    assert!(!host.is_disabled());
}

/// REQ: F-003
#[test]
fn f_003_two_splits_in_a_row_create_two_distinct_panes() {
    let mut host = activated_server();
    let focused = host.tree().add(pane_node("api", "ilium.terminal", None));
    host.deliver_one(message_from_client("split-right", id_bytes(focused)));
    host.settle();
    host.deliver_one(message_from_client("split-down", id_bytes(focused)));
    host.settle();
    let splits = create_splits(&host.submitted());
    assert_eq!(splits.len(), 2);
    let new_first: u64 = splits[0].panes[1].0;
    let new_second: u64 = splits[1].panes[1].0;
    assert_ne!(new_first, new_second);
}

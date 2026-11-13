//! F-004 and F-006: the node commands the tree context menu invokes
//! (`ilium.core.tree/{move-up,move-down,set-bookmark,set-locked-closed}`) and the indent keys.

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{
    CommandContext, CommandInvokedEvent, CommandOrigin, Event, Intent, MoveNodeSpec, NodeClass,
    NodeId, Placement, SetBookmarkedSpec, SetNodeLockedSpec, Submission,
};

use crate::common::{add_node, server_host};

fn invoke(host: &mut FakeHost, command: &str, arguments: serde_json::Value) -> Vec<Submission> {
    host.deliver_one(Event::CommandInvoked(CommandInvokedEvent {
        command: command.to_string(),
        arguments_json: arguments.to_string(),
        origin: CommandOrigin::Menu,
        context: CommandContext::default(),
    }))
}

fn group_with_two_panes(host: &mut FakeHost) -> (u64, u64, u64) {
    let group = add_node(host, None, "work", NodeClass::Container);
    let first = add_node(host, Some(group), "first", NodeClass::Pane);
    let second = add_node(host, Some(group), "second", NodeClass::Pane);
    (group, first, second)
}

/// REQ: F-004
#[test]
fn feature_004_move_down_swaps_a_pane_with_the_next_sibling() {
    let mut host = server_host();
    let (_, first, second) = group_with_two_panes(&mut host);
    let submissions = invoke(
        &mut host,
        "ilium.core.tree/move-down",
        serde_json::json!({ "node": first }),
    );
    assert_eq!(submissions.len(), 1);
    assert_eq!(
        submissions[0].intent,
        Intent::MoveNode(MoveNodeSpec {
            node: NodeId(first),
            target: NodeId(second),
            placement: Placement::After
        })
    );
}

/// REQ: F-004
#[test]
fn feature_004_move_up_swaps_a_pane_with_the_previous_sibling() {
    let mut host = server_host();
    let (_, first, second) = group_with_two_panes(&mut host);
    let submissions = invoke(
        &mut host,
        "ilium.core.tree/move-up",
        serde_json::json!({ "node": second }),
    );
    assert_eq!(
        submissions
            .iter()
            .map(|s| s.intent.clone())
            .collect::<Vec<_>>(),
        vec![Intent::MoveNode(MoveNodeSpec {
            node: NodeId(second),
            target: NodeId(first),
            placement: Placement::Before
        })]
    );
}

/// REQ: F-004
#[test]
fn feature_004_a_move_at_the_outer_boundary_submits_nothing() {
    let mut host = server_host();
    let (_, first, second) = group_with_two_panes(&mut host);
    assert!(invoke(&mut host, "move-up", serde_json::json!({ "node": first })).is_empty());
    assert!(invoke(
        &mut host,
        "move-down",
        serde_json::json!({ "node": second })
    )
    .is_empty());
}

/// REQ: F-004
#[test]
fn feature_004_a_command_without_a_node_or_with_an_unknown_name_submits_nothing() {
    let mut host = server_host();
    group_with_two_panes(&mut host);
    assert!(invoke(&mut host, "move-up", serde_json::json!({})).is_empty());
    assert!(invoke(&mut host, "frobnicate", serde_json::json!({ "node": 1 })).is_empty());
    assert!(invoke(&mut host, "set-bookmark", serde_json::json!({ "node": 1 })).is_empty());
}

/// REQ: F-006
#[test]
fn feature_006_set_bookmark_writes_the_metadata_byte_and_mirrors_the_intent() {
    let mut host = server_host();
    let (_, first, _) = group_with_two_panes(&mut host);
    let on = invoke(
        &mut host,
        "ilium.core.tree/set-bookmark",
        serde_json::json!({ "node": first, "on": true }),
    );
    assert_eq!(host.tree().metadata(first), Some(vec![1]));
    assert_eq!(
        on[0].intent,
        Intent::SetBookmarked(SetBookmarkedSpec {
            node: NodeId(first),
            bookmarked: true
        })
    );
    let off = invoke(
        &mut host,
        "set-bookmark",
        serde_json::json!({ "node": first, "on": false }),
    );
    let metadata = host.tree().metadata(first);
    assert!(metadata.is_none() || metadata == Some(vec![]));
    assert_eq!(
        off[0].intent,
        Intent::SetBookmarked(SetBookmarkedSpec {
            node: NodeId(first),
            bookmarked: false
        })
    );
}

/// REQ: F-006
#[test]
fn feature_006_set_locked_closed_submits_the_lock_intent() {
    let mut host = server_host();
    let (_, first, _) = group_with_two_panes(&mut host);
    let submissions = invoke(
        &mut host,
        "ilium.core.tree/set-locked-closed",
        serde_json::json!({ "node": first, "on": true }),
    );
    assert_eq!(
        submissions[0].intent,
        Intent::SetNodeLocked(SetNodeLockedSpec {
            node: NodeId(first),
            locked: true
        })
    );
}

/// REQ: F-004
#[test]
fn feature_004_set_bookmark_for_a_missing_node_submits_nothing() {
    let mut host = server_host();
    let submissions = invoke(
        &mut host,
        "set-bookmark",
        serde_json::json!({ "node": 424_242u64, "on": true }),
    );
    assert!(submissions.is_empty());
}

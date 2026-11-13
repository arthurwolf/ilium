//! F-006 automatic removal of empty groups (page sample "Remove empty groups", user docs "Bookmarks, locks and automatic clean-up").
//!
//! The tests use `FakeTree::set_locked` and `FakeTree::set_metadata`, which the skeleton lacks (see API_GAPS.md).

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{Event, Intent, NodeClass, NodeId};

use crate::common::{add_node, closed_nodes, server_host};

/// A group with one pane under it, after the tree was observed.
fn group_with_pane(host: &mut FakeHost) -> (u64, u64) {
    let group = add_node(host, None, "Deploy", NodeClass::Container);
    let pane = add_node(host, Some(group), "api", NodeClass::Pane);
    host.settle();
    (group, pane)
}

/// REQ: F-006
#[test]
fn feature_006_closing_the_last_child_closes_the_empty_group() {
    let mut host = server_host();
    let (group, pane) = group_with_pane(&mut host);
    host.tree().remove(pane);
    host.settle();
    assert_eq!(closed_nodes(&host.submitted()), vec![NodeId(group)]);
}

/// REQ: F-006
#[test]
fn feature_006_a_group_with_a_remaining_child_stays_open() {
    let mut host = server_host();
    let (group, pane) = group_with_pane(&mut host);
    add_node(&mut host, Some(group), "web", NodeClass::Pane);
    host.settle();
    host.tree().remove(pane);
    host.settle();
    assert!(closed_nodes(&host.submitted()).is_empty());
    assert!(
        host.tree().get(group).is_some(),
        "the group is still open in the tree"
    );
    // The only submissions are the `order` decorations of the new nodes.
    assert!(
        host.submitted()
            .iter()
            .all(|submission| matches!(submission.intent, Intent::SetDecoration(_))),
        "nothing but decorations submitted: {:?}",
        host.submitted()
    );
}

/// REQ: F-006
#[test]
fn feature_006_a_locked_group_is_not_closed_when_it_empties() {
    let mut host = server_host();
    let (group, pane) = group_with_pane(&mut host);
    host.tree().set_locked(group, true);
    host.settle();
    host.tree().remove(pane);
    host.settle();
    assert!(closed_nodes(&host.submitted()).is_empty());
}

/// REQ: F-006
#[test]
fn feature_006_a_bookmarked_group_is_not_closed_when_it_empties() {
    let mut host = server_host();
    let (group, pane) = group_with_pane(&mut host);
    host.tree().set_metadata(group, vec![1]);
    host.settle();
    host.tree().remove(pane);
    host.settle();
    assert!(closed_nodes(&host.submitted()).is_empty());
}

/// REQ: F-006
#[test]
fn feature_006_metadata_other_than_the_bookmark_byte_does_not_protect_the_group() {
    let mut host = server_host();
    let (group, pane) = group_with_pane(&mut host);
    host.tree().set_metadata(group, vec![0]);
    host.settle();
    host.tree().remove(pane);
    host.settle();
    assert_eq!(closed_nodes(&host.submitted()), vec![NodeId(group)]);
}

/// REQ: F-006
#[test]
fn feature_006_empty_metadata_means_not_bookmarked() {
    let mut host = server_host();
    let (group, pane) = group_with_pane(&mut host);
    host.tree().set_metadata(group, vec![]);
    host.settle();
    host.tree().remove(pane);
    host.settle();
    assert_eq!(closed_nodes(&host.submitted()), vec![NodeId(group)]);
}

/// REQ: F-006
#[test]
fn feature_006_unlocking_before_the_last_child_goes_allows_the_close() {
    let mut host = server_host();
    let (group, pane) = group_with_pane(&mut host);
    host.tree().set_locked(group, true);
    host.settle();
    host.tree().set_locked(group, false);
    host.settle();
    host.tree().remove(pane);
    host.settle();
    assert_eq!(closed_nodes(&host.submitted()), vec![NodeId(group)]);
}

/// REQ: F-006
#[test]
fn feature_006_the_cascade_closes_ancestors_that_become_empty() {
    let mut host = server_host();
    let outer = add_node(&mut host, None, "Outer", NodeClass::Container);
    let inner = add_node(&mut host, Some(outer), "Inner", NodeClass::Container);
    let pane = add_node(&mut host, Some(inner), "api", NodeClass::Pane);
    host.settle();
    host.tree().remove(pane);
    host.settle();
    assert_eq!(closed_nodes(&host.submitted()), vec![NodeId(inner)]);
    // The kernel executes close-node and reports the removal of the inner group.
    host.tree().remove(inner);
    host.settle();
    assert_eq!(
        closed_nodes(&host.submitted()),
        vec![NodeId(inner), NodeId(outer)]
    );
}

/// REQ: F-006
#[test]
fn feature_006_the_cascade_stops_at_a_locked_ancestor() {
    let mut host = server_host();
    let outer = add_node(&mut host, None, "Outer", NodeClass::Container);
    let inner = add_node(&mut host, Some(outer), "Inner", NodeClass::Container);
    let pane = add_node(&mut host, Some(inner), "api", NodeClass::Pane);
    host.tree().set_locked(outer, true);
    host.settle();
    host.tree().remove(pane);
    host.tree().remove(inner);
    host.settle();
    assert_eq!(closed_nodes(&host.submitted()), vec![NodeId(inner)]);
}

/// REQ: F-006
#[test]
fn feature_006_a_folder_that_loses_its_last_child_is_closed_too() {
    let mut host = server_host();
    let folder = add_node(&mut host, None, "docs", NodeClass::Folder);
    let pane = add_node(&mut host, Some(folder), "notes", NodeClass::Pane);
    host.settle();
    host.tree().remove(pane);
    host.settle();
    assert_eq!(closed_nodes(&host.submitted()), vec![NodeId(folder)]);
}

/// REQ: F-006
#[test]
fn feature_006_a_removal_of_a_node_never_seen_closes_nothing() {
    let mut host = server_host();
    let direct = host.deliver_one(Event::NodeRemoved(NodeId(9_999)));
    host.settle();
    assert!(
        direct.is_empty(),
        "no submission for an unknown node: {direct:?}"
    );
    assert!(
        host.submitted().is_empty(),
        "nothing submitted at all: {:?}",
        host.submitted()
    );
}

/// REQ: F-006
#[test]
fn feature_006_a_second_removal_event_for_the_same_node_closes_only_once() {
    let mut host = server_host();
    let (group, pane) = group_with_pane(&mut host);
    host.tree().remove(pane);
    host.settle();
    host.deliver_one(Event::NodeRemoved(NodeId(pane)));
    host.settle();
    assert_eq!(closed_nodes(&host.submitted()), vec![NodeId(group)]);
}

/// REQ: F-006
#[test]
fn feature_006_a_group_whose_parent_is_gone_from_the_tree_is_not_closed() {
    let mut host = server_host();
    let (group, pane) = group_with_pane(&mut host);
    host.tree().remove(group);
    host.settle();
    host.deliver_one(Event::NodeRemoved(NodeId(pane)));
    host.settle();
    assert!(closed_nodes(&host.submitted()).is_empty());
}

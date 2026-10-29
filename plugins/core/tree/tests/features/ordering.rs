//! F-005 tree ordering: the server side submits `set-decoration` with a `sort-key` (page sample "Order the tree").

use ilium_plugin_sdk::types::{Event, Intent, NodeClass, NodeId};
use proptest::prelude::*;

use crate::common::{add_node, expected_sort_key, order_keys, server_host};

/// REQ: F-005
#[test]
fn feature_005_tree_changed_submits_a_sort_key_for_the_new_node() {
    let mut host = server_host();
    let node = add_node(&mut host, None, "Deploy", NodeClass::Container);
    host.settle();
    let keys = order_keys(&host.submitted());
    assert_eq!(
        keys,
        vec![(NodeId(node), Some(expected_sort_key("Deploy")))]
    );
}

/// REQ: F-005
#[test]
fn feature_005_the_decoration_uses_slot_order_and_is_not_persisted() {
    let mut host = server_host();
    let node = add_node(&mut host, None, "docs", NodeClass::Folder);
    host.settle();
    let submitted = host.submitted();
    let spec = submitted
        .iter()
        .find_map(|submission| match &submission.intent {
            Intent::SetDecoration(spec) if spec.node == NodeId(node) => Some(spec.clone()),
            _ => None,
        })
        .expect("a set-decoration for the node");
    assert_eq!(spec.slot, "order");
    assert!(!spec.persist, "sort keys are recomputed, never persisted");
    assert!(
        spec.content.iter().all(|span| span.text.is_empty()),
        "the order decoration draws nothing"
    );
}

/// REQ: F-005
#[test]
fn feature_005_a_key_is_the_first_four_bytes_of_the_lowercase_title() {
    let mut host = server_host();
    let node = add_node(&mut host, None, "Zeta", NodeClass::Pane);
    host.settle();
    // 'z' 'e' 't' 'a' = 0x7a 0x65 0x74 0x61.
    let expected = ((0x7ai64 * 256 + 0x65) * 256 + 0x74) * 256 + 0x61;
    assert_eq!(
        order_keys(&host.submitted()),
        vec![(NodeId(node), Some(expected))]
    );
}

/// REQ: F-005
#[test]
fn feature_005_case_does_not_change_the_key() {
    let mut host = server_host();
    let upper = add_node(&mut host, None, "BUILD", NodeClass::Pane);
    let lower = add_node(&mut host, None, "build", NodeClass::Pane);
    host.settle();
    let keys = order_keys(&host.submitted());
    let key_of = |id: u64| {
        keys.iter()
            .find(|(node, _)| *node == NodeId(id))
            .and_then(|(_, key)| *key)
    };
    assert_eq!(key_of(upper), key_of(lower));
    assert_eq!(key_of(upper), Some(expected_sort_key("build")));
}

/// REQ: F-005
#[test]
fn feature_005_titles_longer_than_four_bytes_share_the_key_of_their_prefix() {
    let mut host = server_host();
    let first = add_node(&mut host, None, "abcdX", NodeClass::Pane);
    let second = add_node(&mut host, None, "abcdY", NodeClass::Pane);
    host.settle();
    let keys = order_keys(&host.submitted());
    let key_of = |id: u64| {
        keys.iter()
            .find(|(node, _)| *node == NodeId(id))
            .and_then(|(_, key)| *key)
    };
    assert_eq!(key_of(first), key_of(second));
}

/// REQ: F-005
#[test]
fn feature_005_short_titles_use_only_the_bytes_they_have() {
    let mut host = server_host();
    let node = add_node(&mut host, None, "a", NodeClass::Pane);
    host.settle();
    assert_eq!(
        order_keys(&host.submitted()),
        vec![(NodeId(node), Some(0x61))]
    );
}

/// REQ: F-005
#[test]
fn feature_005_alphabetical_titles_get_ascending_keys() {
    let mut host = server_host();
    let alpha = add_node(&mut host, None, "alpha", NodeClass::Pane);
    let beta = add_node(&mut host, None, "beta", NodeClass::Pane);
    let gamma = add_node(&mut host, None, "gamma", NodeClass::Pane);
    host.settle();
    let keys = order_keys(&host.submitted());
    let key_of = |id: u64| {
        keys.iter()
            .find(|(node, _)| *node == NodeId(id))
            .and_then(|(_, key)| *key)
            .expect("key")
    };
    assert!(key_of(alpha) < key_of(beta));
    assert!(key_of(beta) < key_of(gamma));
}

/// REQ: F-005
#[test]
fn feature_005_an_unchanged_key_is_not_submitted_again() {
    let mut host = server_host();
    let node = add_node(&mut host, None, "Deploy", NodeClass::Container);
    host.settle();
    let before = order_keys(&host.submitted()).len();
    host.deliver_one(Event::TreeChanged(vec![NodeId(node)]));
    host.settle();
    assert_eq!(order_keys(&host.submitted()).len(), before);
}

/// REQ: F-005
#[test]
fn feature_005_a_rename_that_keeps_the_first_four_bytes_submits_nothing_new() {
    let mut host = server_host();
    let node = add_node(&mut host, None, "build-api", NodeClass::Pane);
    host.settle();
    let before = order_keys(&host.submitted()).len();
    host.tree().rename(node, "BUILD-web");
    host.settle();
    assert_eq!(order_keys(&host.submitted()).len(), before);
}

/// REQ: F-005
#[test]
fn feature_005_a_rename_that_changes_the_key_submits_the_new_key() {
    let mut host = server_host();
    let node = add_node(&mut host, None, "api", NodeClass::Pane);
    host.settle();
    host.tree().rename(node, "zzz");
    host.settle();
    let keys = order_keys(&host.submitted());
    assert_eq!(
        keys.last(),
        Some(&(NodeId(node), Some(expected_sort_key("zzz"))))
    );
    assert_eq!(keys.len(), 2);
}

/// REQ: F-005
#[test]
fn feature_005_only_changed_nodes_in_a_batch_are_submitted() {
    let mut host = server_host();
    let kept = add_node(&mut host, None, "keep", NodeClass::Pane);
    let moved = add_node(&mut host, None, "old", NodeClass::Pane);
    host.settle();
    host.tree().rename(moved, "new");
    host.deliver_one(Event::TreeChanged(vec![NodeId(kept), NodeId(moved)]));
    host.settle();
    let keys = order_keys(&host.submitted());
    assert_eq!(
        keys.iter()
            .filter(|(node, _)| *node == NodeId(kept))
            .count(),
        1
    );
    assert_eq!(
        keys.iter()
            .filter(|(node, _)| *node == NodeId(moved))
            .count(),
        2
    );
}

/// REQ: F-005
#[test]
fn feature_005_a_node_that_is_gone_from_the_tree_is_skipped() {
    let mut host = server_host();
    let node = add_node(&mut host, None, "ghost", NodeClass::Pane);
    host.settle();
    host.tree().remove(node);
    host.settle();
    let before = host.submitted().len();
    host.deliver_one(Event::TreeChanged(vec![NodeId(node)]));
    host.settle();
    assert_eq!(host.submitted().len(), before);
}

/// REQ: F-005
#[test]
fn feature_005_events_other_than_tree_changed_submit_no_order_keys() {
    let mut host = server_host();
    let node = add_node(&mut host, None, "x", NodeClass::Pane);
    host.settle();
    let before = order_keys(&host.submitted()).len();
    host.deliver(vec![
        Event::ThemeChanged,
        Event::KeymapChanged,
        Event::PaneFocused(Some(NodeId(node))),
    ]);
    host.settle();
    assert_eq!(order_keys(&host.submitted()).len(), before);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    // REQ: F-005
    #[test]
    fn feature_005_property_key_matches_the_rule_for_any_title(title in "[A-Za-z0-9 _.-]{1,12}") {
        let mut host = server_host();
        let node = add_node(&mut host, None, &title, NodeClass::Pane);
        host.settle();
        prop_assert_eq!(
            order_keys(&host.submitted()),
            vec![(NodeId(node), Some(expected_sort_key(&title)))]
        );
    }

    // REQ: F-005
    #[test]
    fn feature_005_property_key_order_follows_prefix_order(left in "[a-z]{4}", right in "[a-z]{4}") {
        let mut host = server_host();
        let first = add_node(&mut host, None, &left, NodeClass::Pane);
        let second = add_node(&mut host, None, &right, NodeClass::Pane);
        host.settle();
        let keys = order_keys(&host.submitted());
        let key_of = |id: u64| keys.iter().find(|(node, _)| *node == NodeId(id)).and_then(|(_, key)| *key).expect("key");
        prop_assert_eq!(left.cmp(&right), key_of(first).cmp(&key_of(second)));
    }
}

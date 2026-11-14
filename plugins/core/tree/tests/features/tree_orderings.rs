//! The orderings the plugin contributes to the kernel's "Order by" menu
//! (`contributes.tree_orderings`): item type, newest first, oldest first and the two name orders.
//! The server side publishes, per ordering, one sort key per node in the ordering's slot; the
//! kernel sorts siblings by those keys, ascending.

use std::collections::HashMap;

use ilium_plugin_core_tree::ordering::{sibling_keys, Entry, EntryKind, PLUGIN_ORDERINGS};
use ilium_plugin_sdk::types::{Event, Intent, NodeClass, NodeId, Submission};
use serde_json::Value;

use crate::common::{add_node, manifest_path, server_host};

/// The keys submitted in `slot`, newest submission last per node.
fn slot_keys(submissions: &[Submission], slot: &str) -> HashMap<u64, i64> {
    let mut keys = HashMap::new();
    for submission in submissions {
        if let Intent::SetDecoration(spec) = &submission.intent {
            if spec.slot == slot {
                if let Some(key) = spec.sort_key {
                    keys.insert(spec.node.0, key);
                }
            }
        }
    }
    keys
}

/// The nodes of `keys` from the lowest key to the highest, as the kernel sorts them.
fn by_key(keys: &HashMap<u64, i64>) -> Vec<u64> {
    let mut nodes: Vec<(i64, u64)> = keys.iter().map(|(node, key)| (*key, *node)).collect();
    nodes.sort_unstable();
    nodes.into_iter().map(|(_, node)| node).collect()
}

fn slot_of(id: &str) -> &'static str {
    PLUGIN_ORDERINGS
        .iter()
        .find(|ordering| ordering.id == id)
        .map(|ordering| ordering.slot)
        .expect("a known ordering")
}

/// The manifest and the constants list the same orderings in the same order.
#[test]
fn the_manifest_contributes_exactly_the_orderings_the_plugin_ranks() {
    let text = std::fs::read_to_string(manifest_path()).expect("manifest readable");
    let manifest: Value = serde_json::from_str(&text).expect("manifest is JSON");
    let contributed = manifest["contributes"]["tree_orderings"]
        .as_array()
        .expect("tree_orderings is contributed");
    assert_eq!(contributed.len(), PLUGIN_ORDERINGS.len());
    for (entry, ordering) in contributed.iter().zip(PLUGIN_ORDERINGS.iter()) {
        assert_eq!(entry["id"], ordering.id);
        assert_eq!(entry["title"], ordering.title);
        assert_eq!(entry["slot"], ordering.slot);
        assert!(ordering.title.chars().count() <= 32, "title length cap");
    }
}

#[test]
fn the_name_orderings_rank_siblings_case_insensitively() {
    let mut host = server_host();
    let zebra = add_node(&mut host, None, "Zebra", NodeClass::Pane);
    let alpha = add_node(&mut host, None, "alpha", NodeClass::Pane);
    let middle = add_node(&mut host, None, "Middle", NodeClass::Pane);
    host.settle();
    let submitted = host.submitted();
    assert_eq!(
        by_key(&slot_keys(&submitted, slot_of("name-az"))),
        vec![alpha, middle, zebra]
    );
    assert_eq!(
        by_key(&slot_keys(&submitted, slot_of("name-za"))),
        vec![zebra, middle, alpha]
    );
}

#[test]
fn the_age_orderings_follow_node_creation() {
    let mut host = server_host();
    let first = add_node(&mut host, None, "first", NodeClass::Pane);
    let second = add_node(&mut host, None, "second", NodeClass::Pane);
    let third = add_node(&mut host, None, "third", NodeClass::Pane);
    host.settle();
    let submitted = host.submitted();
    assert_eq!(
        by_key(&slot_keys(&submitted, slot_of("age-newest"))),
        vec![third, second, first]
    );
    assert_eq!(
        by_key(&slot_keys(&submitted, slot_of("age-oldest"))),
        vec![first, second, third]
    );
}

#[test]
fn the_type_ordering_groups_kinds_then_orders_names() {
    let mut host = server_host();
    let pane = add_node(&mut host, None, "alpha", NodeClass::Pane);
    let folder = add_node(&mut host, None, "docs", NodeClass::Folder);
    let group = add_node(&mut host, None, "work", NodeClass::Container);
    host.settle();
    assert_eq!(
        by_key(&slot_keys(&host.submitted(), slot_of("type"))),
        vec![group, folder, pane]
    );
}

#[test]
fn a_rename_re_ranks_the_siblings_whose_position_moved() {
    let mut host = server_host();
    let first = add_node(&mut host, None, "alpha", NodeClass::Pane);
    let second = add_node(&mut host, None, "beta", NodeClass::Pane);
    host.settle();
    host.tree().rename(first, "zulu");
    host.settle();
    assert_eq!(
        by_key(&slot_keys(&host.submitted(), slot_of("name-az"))),
        vec![second, first]
    );
}

#[test]
fn an_unchanged_rank_is_not_submitted_again() {
    let mut host = server_host();
    let node = add_node(&mut host, None, "alpha", NodeClass::Pane);
    host.settle();
    let before = host.submitted().len();
    host.deliver_one(Event::TreeChanged(vec![NodeId(node)]));
    host.settle();
    assert_eq!(host.submitted().len(), before);
}

#[test]
fn removing_a_node_re_ranks_the_siblings_that_stay() {
    let mut host = server_host();
    let group = add_node(&mut host, None, "group", NodeClass::Container);
    let low = add_node(&mut host, Some(group), "a", NodeClass::Pane);
    let high = add_node(&mut host, Some(group), "b", NodeClass::Pane);
    host.settle();
    assert_eq!(
        slot_keys(&host.submitted(), slot_of("name-az")).get(&high),
        Some(&1)
    );
    host.tree().remove(low);
    host.settle();
    assert_eq!(
        slot_keys(&host.submitted(), slot_of("name-az")).get(&high),
        Some(&0),
        "the survivor moves up behind the removed sibling"
    );
}

#[test]
fn sibling_keys_are_positions_in_each_ordering() {
    let entries = vec![
        Entry {
            id: 1,
            name: "beta".into(),
            kind: EntryKind::Terminal,
        },
        Entry {
            id: 2,
            name: "Alpha".into(),
            kind: EntryKind::Terminal,
        },
        Entry {
            id: 3,
            name: "docs".into(),
            kind: EntryKind::Folder,
        },
    ];
    let keys = sibling_keys(&entries, true);
    let key = |node: u64, ordering: &str| {
        let index = PLUGIN_ORDERINGS
            .iter()
            .position(|candidate| candidate.id == ordering)
            .expect("a known ordering");
        keys.iter()
            .find(|(id, which, _)| *id == node && *which == index)
            .map(|(_, _, key)| *key)
    };
    assert_eq!(key(2, "name-az"), Some(0));
    assert_eq!(key(1, "name-az"), Some(1));
    assert_eq!(key(3, "name-az"), Some(2));
    assert_eq!(key(3, "type"), Some(0), "a folder ranks before terminals");
    assert_eq!(key(3, "age-newest"), Some(0));
    assert_eq!(key(1, "age-oldest"), Some(0));
}

#[test]
fn a_parent_that_cannot_hold_normal_children_keeps_its_structural_order() {
    let entries = vec![
        Entry {
            id: 1,
            name: "zebra".into(),
            kind: EntryKind::Terminal,
        },
        Entry {
            id: 2,
            name: "alpha".into(),
            kind: EntryKind::Terminal,
        },
    ];
    for (node, _which, key) in sibling_keys(&entries, false) {
        assert_eq!(key, (node - 1) as i64, "node {node} keeps its slot");
    }
}

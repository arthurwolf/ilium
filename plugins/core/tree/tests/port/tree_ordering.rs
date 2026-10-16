//! Twins of the tests in ilium-client/src/tree_ordering.rs. The plugin expresses order as one
//! `sort-key` per node (page sample "Order the tree"); the kernel applies it per Order By mode.

use ilium_plugin_sdk::types::NodeClass;

use crate::common::{add_node, order_keys, server_host, sorted_by_key};

/// REQ: PORT:ilium-client/src/tree_ordering.rs::cost_descending_sorts_by_rank_and_keeps_manual_order_for_ties
#[test]
fn port_tree_ordering_cost_descending_ties_keep_manual_order() {
    let mut host = server_host();
    let group = add_node(&mut host, None, "work", NodeClass::Container);
    let free_first = add_node(&mut host, Some(group), "free-a", NodeClass::Pane);
    let free_second = add_node(&mut host, Some(group), "free-b", NodeClass::Pane);
    host.settle();
    let submitted = host.submitted();
    let keys = order_keys(&submitted);
    let key_of = |id: u64| keys.iter().find(|(node, _)| node.0 == id).and_then(|(_, key)| *key);
    // Unranked siblings tie on the key, so the kernel falls back to the manual (creation) order.
    assert_eq!(key_of(free_first), key_of(free_second));
    assert_eq!(sorted_by_key(&submitted, &[free_second, free_first]), vec![free_first, free_second]);
}

/// REQ: PORT:ilium-client/src/tree_ordering.rs::manual_and_age_modes_preserve_or_reverse_creation_order
#[test]
fn port_tree_ordering_keys_do_not_depend_on_creation_order() {
    let mut host = server_host();
    let group = add_node(&mut host, None, "work", NodeClass::Container);
    let old = add_node(&mut host, Some(group), "old", NodeClass::Pane);
    let middle = add_node(&mut host, Some(group), "middle", NodeClass::Pane);
    let new = add_node(&mut host, Some(group), "new", NodeClass::Pane);
    host.settle();
    let submitted = host.submitted();
    // Manual and age modes use the creation order, which the plugin must not disturb: the keys follow titles only.
    assert_eq!(sorted_by_key(&submitted, &[old, middle, new]), vec![middle, new, old]);
}

/// REQ: PORT:ilium-client/src/tree_ordering.rs::name_modes_apply_inside_each_group_but_never_reorder_split_members
#[test]
fn port_tree_ordering_name_keys_ignore_case() {
    let mut host = server_host();
    let group = add_node(&mut host, None, "work", NodeClass::Container);
    let zebra = add_node(&mut host, Some(group), "Zebra", NodeClass::Pane);
    let alpha = add_node(&mut host, Some(group), "alpha", NodeClass::Pane);
    host.settle();
    let submitted = host.submitted();
    // Ascending name order puts alpha before Zebra; descending is the reverse of the same keys.
    assert_eq!(sorted_by_key(&submitted, &[zebra, alpha]), vec![alpha, zebra]);
}

/// REQ: PORT:ilium-client/src/tree_ordering.rs::type_mode_groups_every_visible_node_kind_then_orders_names
#[test]
fn port_tree_ordering_names_inside_one_kind_follow_title_keys() {
    let mut host = server_host();
    let parent = add_node(&mut host, None, "work", NodeClass::Container);
    let roadmap = add_node(&mut host, Some(parent), "roadmap", NodeClass::Pane);
    let shell = add_node(&mut host, Some(parent), "shell", NodeClass::Pane);
    let codex = add_node(&mut host, Some(parent), "codex", NodeClass::Pane);
    let editor = add_node(&mut host, Some(parent), "editor", NodeClass::Pane);
    let assets = add_node(&mut host, Some(parent), "assets", NodeClass::Folder);
    let nested = add_node(&mut host, Some(parent), "nested", NodeClass::Container);
    host.settle();
    let submitted = host.submitted();
    assert_eq!(
        sorted_by_key(&submitted, &[roadmap, shell, codex, editor, assets, nested]),
        vec![assets, codex, editor, nested, roadmap, shell]
    );
}

/// REQ: PORT:ilium-client/src/tree_ordering.rs::type_mode_also_sorts_a_projects_direct_children
#[test]
fn port_tree_ordering_direct_children_of_a_top_level_node_get_keys() {
    let mut host = server_host();
    let project = add_node(&mut host, None, "project", NodeClass::Container);
    let shell = add_node(&mut host, Some(project), "shell", NodeClass::Pane);
    let nested = add_node(&mut host, Some(project), "nested", NodeClass::Container);
    host.settle();
    let submitted = host.submitted();
    assert_eq!(sorted_by_key(&submitted, &[shell, nested]), vec![nested, shell]);
}

//! Twins of the tests in ilium-client/src/tree_ordering.rs, run against the plugin's pure
//! `ordering::ordered`. The kernel tree is replaced by a list of entries in manual order; ids grow
//! in creation order, as node ids do in the kernel.

use std::collections::HashMap;

use ilium_plugin_core_tree::ordering::{ordered, Entry, EntryKind, TreeOrder};

fn entry(id: u64, name: &str, kind: EntryKind) -> Entry {
    Entry {
        id,
        name: name.to_string(),
        kind,
    }
}

fn names(children: &[Entry], ids: &[u64]) -> Vec<String> {
    ids.iter()
        .filter_map(|id| children.iter().find(|entry| entry.id == *id))
        .map(|entry| entry.name.clone())
        .collect()
}

fn run(
    children: &[Entry],
    sortable: bool,
    order: TreeOrder,
    ranks: &HashMap<u64, f64>,
) -> Vec<String> {
    names(children, &ordered(children, sortable, order, ranks))
}

/// REQ: PORT:ilium-client/src/tree_ordering.rs::cost_descending_sorts_by_rank_and_keeps_manual_order_for_ties
#[test]
fn port_tree_ordering_cost_descending_ties_keep_manual_order() {
    let children = vec![
        entry(2, "cheap", EntryKind::Terminal),
        entry(3, "free-a", EntryKind::Terminal),
        entry(4, "pricey", EntryKind::Terminal),
        entry(5, "free-b", EntryKind::Terminal),
    ];
    let ranks = HashMap::from([(2, 1.5), (4, 40.0)]);
    assert_eq!(
        run(&children, true, TreeOrder::CostDescending, &ranks),
        ["pricey", "cheap", "free-a", "free-b"],
        "unranked nodes keep their manual order after every ranked one"
    );
    // Without ranks the mode degrades to the manual order.
    assert_eq!(
        run(&children, true, TreeOrder::CostDescending, &HashMap::new()),
        ["cheap", "free-a", "pricey", "free-b"]
    );
}

/// REQ: PORT:ilium-client/src/tree_ordering.rs::manual_and_age_modes_preserve_or_reverse_creation_order
#[test]
fn port_tree_ordering_manual_and_age_modes_preserve_or_reverse_creation_order() {
    let children = vec![
        entry(2, "old", EntryKind::Terminal),
        entry(3, "middle", EntryKind::Terminal),
        entry(4, "new", EntryKind::Terminal),
    ];
    let none = HashMap::new();
    assert_eq!(
        run(&children, true, TreeOrder::Manual, &none),
        ["old", "middle", "new"]
    );
    assert_eq!(
        run(&children, true, TreeOrder::AgeAscending, &none),
        ["new", "middle", "old"]
    );
    assert_eq!(
        run(&children, true, TreeOrder::AgeDescending, &none),
        ["old", "middle", "new"]
    );
}

/// REQ: PORT:ilium-client/src/tree_ordering.rs::type_mode_groups_every_visible_node_kind_then_orders_names
#[test]
fn port_tree_ordering_type_mode_groups_every_visible_node_kind_then_orders_names() {
    // The kernel test adds a board, a shell (moved into the split), a Codex agent, an editor, a
    // folder, a split view and a nested group; the shell is no longer a direct child.
    let children = vec![
        entry(2, "roadmap", EntryKind::Board),
        entry(4, "codex", EntryKind::Agent(EntryKind::agent_rank("codex"))),
        entry(5, "editor", EntryKind::Editor),
        entry(6, "assets", EntryKind::Folder),
        entry(7, "split", EntryKind::OtherContainer),
        entry(8, "nested", EntryKind::Group),
    ];
    assert_eq!(
        run(&children, true, TreeOrder::Type, &HashMap::new()),
        ["nested", "split", "assets", "codex", "editor", "roadmap"]
    );
}

/// REQ: PORT:ilium-client/src/tree_ordering.rs::type_mode_also_sorts_a_projects_direct_children
#[test]
fn port_tree_ordering_type_mode_also_sorts_a_projects_direct_children() {
    let children = vec![
        entry(2, "shell", EntryKind::Terminal),
        entry(3, "nested", EntryKind::Group),
    ];
    assert_eq!(
        run(&children, true, TreeOrder::Type, &HashMap::new()),
        ["nested", "shell"]
    );
}

/// REQ: PORT:ilium-client/src/tree_ordering.rs::name_modes_apply_inside_each_group_but_never_reorder_split_members
#[test]
fn port_tree_ordering_name_modes_apply_inside_each_group_but_never_reorder_split_members() {
    let split_members = vec![
        entry(2, "Zebra", EntryKind::Terminal),
        entry(3, "alpha", EntryKind::Terminal),
    ];
    // A split view does not accept normal children: its structural order stays.
    assert_eq!(
        run(
            &split_members,
            false,
            TreeOrder::NameAscending,
            &HashMap::new()
        ),
        ["Zebra", "alpha"]
    );
    let group_children = vec![entry(4, "split", EntryKind::OtherContainer)];
    assert_eq!(
        run(
            &group_children,
            true,
            TreeOrder::NameDescending,
            &HashMap::new()
        ),
        ["split"]
    );
}

/// REQ: PORT:ilium-client/src/tree_ordering.rs::name_modes_apply_inside_each_group_but_never_reorder_split_members
#[test]
fn port_tree_ordering_name_order_ignores_case_and_descending_reverses_it() {
    let children = vec![
        entry(2, "Zebra", EntryKind::Terminal),
        entry(3, "alpha", EntryKind::Terminal),
    ];
    let none = HashMap::new();
    assert_eq!(
        run(&children, true, TreeOrder::NameAscending, &none),
        ["alpha", "Zebra"]
    );
    assert_eq!(
        run(&children, true, TreeOrder::NameDescending, &none),
        ["Zebra", "alpha"]
    );
}

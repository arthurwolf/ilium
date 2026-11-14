//! The kernel's mechanism for presenting the left-panel tree in an order other than the manual one.
//!
//! The detached server owns one durable manual child vector per container. This module derives a
//! client-local presentation order from that vector without mutating it, by sorting siblings
//! with a rank map. It knows no ordering policy: the item-type, age and name orders are
//! `contributes.tree_orderings` of the tree plugin (`ilium.core.tree`), which publishes one sort
//! key per node; the cost order is the Agent Cost tab's rank map. Only normal groups are sorted:
//! split-view child order also determines right-panel placement and must remain structural.

use std::borrow::Cow;
use std::collections::HashMap;

use ilium_core::{Node, NodeId, Tree};

use crate::config::TreeOrder;

/// Returns `parent`'s children in the requested presentation order.
/// Missing/non-container parents fail soft to an empty list, matching the
/// tree renderer's existing recursive-walk contract.
#[cfg(test)]
pub fn ordered_children(tree: &Tree, parent: NodeId, tree_order: TreeOrder) -> Cow<'_, [NodeId]> {
    ordered_children_ranked(tree, parent, tree_order, &HashMap::new())
}

/// [`ordered_children`] with the per-node ranks the non-manual modes sort by: the spend for
/// `CostDescending` (most expensive first, a node without one ranks as free) and the plugin's
/// sort keys for `PluginKey` (ascending, a node without one sinks below every keyed one). Ties
/// keep the manual order because the sort is stable.
pub fn ordered_children_ranked<'a>(
    tree: &'a Tree,
    parent: NodeId,
    tree_order: TreeOrder,
    ranks: &HashMap<NodeId, f64>,
) -> Cow<'a, [NodeId]> {
    let Ok(children) = tree.children_of(parent) else {
        return Cow::Borrowed(&[]);
    };

    // Split views retain their explicit layout order in every mode. Projects
    // accept the same normal-child kinds a group does (only split views are
    // pane-only and structural), so they must be sortable too -- checking
    // `is_group` here would silently leave every project's direct children
    // stuck in manual order no matter which mode the user picks.
    if !tree.get(parent).is_some_and(Node::accepts_normal_children)
        || tree_order == TreeOrder::Manual
    {
        return Cow::Borrowed(children);
    }

    let mut ordered = children.to_vec();
    match tree_order {
        TreeOrder::Manual => {}
        TreeOrder::CostDescending => {
            let rank = |id: NodeId| ranks.get(&id).copied().unwrap_or(0.0);
            ordered.sort_by(|left, right| rank(*right).total_cmp(&rank(*left)));
        }
        TreeOrder::PluginKey => {
            let rank = |id: NodeId| ranks.get(&id).copied().unwrap_or(f64::INFINITY);
            ordered.sort_by(|left, right| rank(*left).total_cmp(&rank(*right)));
        }
    }
    Cow::Owned(ordered)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_core::{PaneContentKind, SplitOrientation, ROOT_ID};

    fn names(tree: &Tree, ids: &[NodeId]) -> Vec<String> {
        ids.iter()
            .filter_map(|id| tree.get(*id).map(|node| node.name.clone()))
            .collect()
    }

    #[test]
    fn plugin_key_sorts_by_ascending_key_and_keeps_unkeyed_nodes_last() {
        let mut tree = Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        let later = tree
            .add_pane(group, "later", PaneContentKind::Terminal)
            .unwrap();
        tree.add_pane(group, "unkeyed", PaneContentKind::Terminal)
            .unwrap();
        let first = tree
            .add_pane(group, "first", PaneContentKind::Terminal)
            .unwrap();
        let ranks = HashMap::from([(later, 7.0), (first, -2.0)]);
        let ordered = ordered_children_ranked(&tree, group, TreeOrder::PluginKey, &ranks);
        assert_eq!(names(&tree, &ordered), ["first", "later", "unkeyed"]);
        // Without keys the mode is the manual order.
        assert_eq!(
            names(&tree, &ordered_children(&tree, group, TreeOrder::PluginKey)),
            ["later", "unkeyed", "first"]
        );
    }

    #[test]
    fn cost_descending_sorts_by_rank_and_keeps_manual_order_for_ties() {
        let mut tree = Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        let cheap = tree
            .add_pane(group, "cheap", PaneContentKind::Terminal)
            .unwrap();
        tree.add_pane(group, "free-a", PaneContentKind::Terminal)
            .unwrap();
        let pricey = tree
            .add_pane(group, "pricey", PaneContentKind::Terminal)
            .unwrap();
        tree.add_pane(group, "free-b", PaneContentKind::Terminal)
            .unwrap();
        let ranks = HashMap::from([(cheap, 1.5), (pricey, 40.0)]);

        let ordered = ordered_children_ranked(&tree, group, TreeOrder::CostDescending, &ranks);
        assert_eq!(
            names(&tree, &ordered),
            ["pricey", "cheap", "free-a", "free-b"],
            "unranked nodes keep their manual order after every ranked one"
        );
        // Without ranks the mode degrades to the manual order.
        assert_eq!(
            names(
                &tree,
                &ordered_children(&tree, group, TreeOrder::CostDescending)
            ),
            ["cheap", "free-a", "pricey", "free-b"]
        );
    }

    #[test]
    fn manual_order_is_the_stored_order() {
        let mut tree = Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        tree.add_pane(group, "old", PaneContentKind::Terminal)
            .unwrap();
        tree.add_pane(group, "middle", PaneContentKind::Terminal)
            .unwrap();
        tree.add_pane(group, "new", PaneContentKind::Terminal)
            .unwrap();

        assert_eq!(
            names(&tree, &ordered_children(&tree, group, TreeOrder::Manual)),
            ["old", "middle", "new"]
        );
    }

    #[test]
    fn a_project_sorts_its_direct_children_too() {
        let mut tree = Tree::new();
        let project = tree
            .add_project(std::path::PathBuf::from("/tmp/project"))
            .unwrap();
        let shell = tree
            .add_pane(project, "shell", PaneContentKind::Terminal)
            .unwrap();
        let nested = tree.add_group(project, "nested").unwrap();
        let ranks = HashMap::from([(nested, 0.0), (shell, 1.0)]);

        assert_eq!(
            names(
                &tree,
                &ordered_children_ranked(&tree, project, TreeOrder::PluginKey, &ranks)
            ),
            ["nested", "shell"]
        );
    }

    #[test]
    fn ranks_apply_inside_each_group_but_never_reorder_split_members() {
        let mut tree = Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        let zebra = tree
            .add_pane(group, "Zebra", PaneContentKind::Terminal)
            .unwrap();
        let alpha = tree
            .add_pane(group, "alpha", PaneContentKind::Terminal)
            .unwrap();
        let split = tree
            .create_split_view(group, "split", SplitOrientation::Vertical, &[zebra, alpha])
            .unwrap();
        let ranks = HashMap::from([(alpha, 0.0), (zebra, 1.0)]);

        assert_eq!(
            names(
                &tree,
                &ordered_children_ranked(&tree, split, TreeOrder::PluginKey, &ranks)
            ),
            ["Zebra", "alpha"]
        );
        assert_eq!(
            names(
                &tree,
                &ordered_children_ranked(&tree, group, TreeOrder::PluginKey, &ranks)
            ),
            ["split"]
        );
    }
}

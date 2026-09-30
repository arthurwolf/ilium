//! Pure ordering policy for the left-panel tree.
//!
//! The detached server owns one durable manual child vector per container.
//! This module derives a client-local presentation order from that vector
//! without mutating it. Only normal groups are sorted: split-view child order
//! also determines right-panel placement and must remain structural.

use std::borrow::Cow;
use std::cmp::Ordering;
use std::collections::HashMap;

#[cfg(test)]
use ilium_core::AgentClass;
use ilium_core::{Node, NodeId, NodeKind, PaneContentKind, PaneStatus, Tree};

use crate::config::TreeOrder;

/// Returns `parent`'s children in the requested presentation order.
/// Missing/non-container parents fail soft to an empty list, matching the
/// tree renderer's existing recursive-walk contract.
pub fn ordered_children(tree: &Tree, parent: NodeId, tree_order: TreeOrder) -> Cow<'_, [NodeId]> {
    ordered_children_ranked(tree, parent, tree_order, &HashMap::new())
}

/// [`ordered_children`] with the per-node spend that `CostDescending` sorts
/// by. A node missing from `cost_ranks` ranks as free, so it sinks below
/// every agent that has spent something; ties keep the manual order.
pub fn ordered_children_ranked<'a>(
    tree: &'a Tree,
    parent: NodeId,
    tree_order: TreeOrder,
    cost_ranks: &HashMap<NodeId, f64>,
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
    ordered.sort_by(|left_id, right_id| {
        compare_nodes(tree, *left_id, *right_id, tree_order, cost_ranks)
    });
    Cow::Owned(ordered)
}

/// Total comparator for two valid siblings. A missing node sorts last rather
/// than panicking if a malformed snapshot ever references a stale child id.
fn compare_nodes(
    tree: &Tree,
    left_id: NodeId,
    right_id: NodeId,
    tree_order: TreeOrder,
    cost_ranks: &HashMap<NodeId, f64>,
) -> Ordering {
    let (left, right) = match (tree.get(left_id), tree.get(right_id)) {
        (Some(left), Some(right)) => (left, right),
        // Both stale: equal rather than an order-dependent Greater/Less, so
        // the comparator stays antisymmetric even with multiple bad ids.
        (None, None) => return Ordering::Equal,
        (None, Some(_)) => return Ordering::Greater,
        (Some(_), None) => return Ordering::Less,
    };

    match tree_order {
        TreeOrder::Manual => Ordering::Equal,
        TreeOrder::Type => type_rank(left)
            .cmp(&type_rank(right))
            .then_with(|| compare_names(left, right)),
        // Node ids are allocated monotonically and never reused, so the
        // largest id is the youngest node and therefore has the least age.
        TreeOrder::AgeAscending => right.id.cmp(&left.id),
        TreeOrder::AgeDescending => left.id.cmp(&right.id),
        TreeOrder::NameAscending => compare_names(left, right),
        TreeOrder::NameDescending => compare_names(right, left),
        TreeOrder::CostDescending => {
            let rank = |id: NodeId| cost_ranks.get(&id).copied().unwrap_or(0.0);
            // `sort_by` is stable, so equal spend keeps the manual order.
            rank(right_id).total_cmp(&rank(left_id))
        }
    }
}

/// Case-insensitive display-name ordering with an id tie-breaker so names
/// differing only by case still render deterministically.
fn compare_names(left: &Node, right: &Node) -> Ordering {
    left.name
        .to_lowercase()
        .cmp(&right.name.to_lowercase())
        .then_with(|| left.name.cmp(&right.name))
        .then_with(|| left.id.cmp(&right.id))
}

/// Groups related rows by the kind of icon/concept the user sees. Agent
/// terminals have their own ranks because Claude, Codex, and plain shells
/// are visibly distinct things in the sidebar.
fn type_rank(node: &Node) -> u8 {
    match &node.kind {
        NodeKind::Container(container) if container.is_group() => 0,
        NodeKind::Container(_) => 1,
        NodeKind::Folder { .. } => 2,
        NodeKind::Pane {
            content: PaneContentKind::Terminal,
            status: PaneStatus::Agent(agent),
            ..
        } => agent.class.type_sort_rank(),
        NodeKind::Pane {
            content: PaneContentKind::Terminal,
            ..
        } => 7,
        NodeKind::Pane {
            content: PaneContentKind::Editor,
            ..
        } => 8,
        NodeKind::Pane {
            content: PaneContentKind::Board,
            ..
        } => 9,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_core::{AgentActivity, BoardStorage, SplitOrientation, ROOT_ID};
    use std::path::PathBuf;

    fn names(tree: &Tree, ids: &[NodeId]) -> Vec<String> {
        ids.iter()
            .filter_map(|id| tree.get(*id).map(|node| node.name.clone()))
            .collect()
    }

    #[test]
    fn cost_descending_sorts_by_rank_and_keeps_manual_order_for_ties() {
        let mut tree = Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        let cheap = tree
            .add_pane(group, "cheap", PaneContentKind::Terminal)
            .unwrap();
        let free_first = tree
            .add_pane(group, "free-a", PaneContentKind::Terminal)
            .unwrap();
        let pricey = tree
            .add_pane(group, "pricey", PaneContentKind::Terminal)
            .unwrap();
        let free_second = tree
            .add_pane(group, "free-b", PaneContentKind::Terminal)
            .unwrap();
        let ranks = HashMap::from([(cheap, 1.5), (pricey, 40.0)]);

        let ordered = ordered_children_ranked(&tree, group, TreeOrder::CostDescending, &ranks);
        assert_eq!(
            names(&tree, &ordered),
            ["pricey", "cheap", "free-a", "free-b"],
            "unranked nodes keep their manual order after every ranked one"
        );
        let _ = (free_first, free_second);
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
    fn manual_and_age_modes_preserve_or_reverse_creation_order() {
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
        assert_eq!(
            names(
                &tree,
                &ordered_children(&tree, group, TreeOrder::AgeAscending)
            ),
            ["new", "middle", "old"]
        );
        assert_eq!(
            names(
                &tree,
                &ordered_children(&tree, group, TreeOrder::AgeDescending)
            ),
            ["old", "middle", "new"]
        );
    }

    #[test]
    fn type_mode_groups_every_visible_node_kind_then_orders_names() {
        let mut tree = Tree::new();
        let parent = tree.add_group(ROOT_ID, "work").unwrap();
        tree.add_board(
            parent,
            "roadmap".to_string(),
            BoardStorage::MarkdownFile {
                path: PathBuf::from("roadmap.md"),
            },
        )
        .unwrap();
        let shell = tree
            .add_pane(parent, "shell", PaneContentKind::Terminal)
            .unwrap();
        let codex = tree
            .add_pane(parent, "codex", PaneContentKind::Terminal)
            .unwrap();
        tree.set_pane_status(
            codex,
            PaneStatus::from_activity(AgentClass::Codex, AgentActivity::Idle, None),
        )
        .unwrap();
        tree.add_pane(parent, "editor", PaneContentKind::Editor)
            .unwrap();
        tree.add_folder(parent, PathBuf::from("assets")).unwrap();
        tree.create_split_view(parent, "split", SplitOrientation::Vertical, &[shell])
            .unwrap();
        tree.add_group(parent, "nested").unwrap();

        assert_eq!(
            names(&tree, &ordered_children(&tree, parent, TreeOrder::Type)),
            ["nested", "split", "assets", "codex", "editor", "roadmap"]
        );
    }

    #[test]
    fn type_mode_also_sorts_a_projects_direct_children() {
        let mut tree = Tree::new();
        let project = tree.add_project(PathBuf::from("/tmp/project")).unwrap();
        tree.add_pane(project, "shell", PaneContentKind::Terminal)
            .unwrap();
        tree.add_group(project, "nested").unwrap();

        assert_eq!(
            names(&tree, &ordered_children(&tree, project, TreeOrder::Type)),
            ["nested", "shell"]
        );
    }

    #[test]
    fn name_modes_apply_inside_each_group_but_never_reorder_split_members() {
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

        assert_eq!(
            names(
                &tree,
                &ordered_children(&tree, split, TreeOrder::NameAscending)
            ),
            ["Zebra", "alpha"]
        );
        assert_eq!(
            names(
                &tree,
                &ordered_children(&tree, group, TreeOrder::NameDescending)
            ),
            ["split"]
        );
    }
}

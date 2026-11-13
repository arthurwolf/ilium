//! Projection of active terminal panes onto the tree without changing it.

use std::collections::HashSet;

use ilium_core::{NodeId, NodeKind, PaneContentKind, Tree};

/// Keeps active terminals and their ancestors, then preserves non-terminal
/// pane rows directly within those visible containers. Such auxiliary panes do
/// not themselves make an otherwise inactive project or group visible.
pub(crate) fn visible_nodes(tree: &Tree, active_terminals: &HashSet<NodeId>) -> HashSet<NodeId> {
    let mut visible = HashSet::new();
    for &pane_id in active_terminals {
        if !matches!(
            tree.get(pane_id).map(|node| &node.kind),
            Some(NodeKind::Pane {
                content: PaneContentKind::Terminal,
                ..
            })
        ) {
            continue;
        }
        let mut current = Some(pane_id);
        while let Some(node_id) = current {
            if !visible.insert(node_id) {
                break;
            }
            current = tree.parent_of(node_id);
        }
    }
    for node_id in tree.all_ids() {
        let Some(node) = tree.get(node_id) else {
            continue;
        };
        if matches!(
            &node.kind,
            NodeKind::Pane {
                content: PaneContentKind::Editor | PaneContentKind::Board,
                ..
            }
        ) && node.parent.is_some_and(|parent| visible.contains(&parent))
        {
            visible.insert(node_id);
        }
    }
    visible
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn active_descendants_keep_ancestors_and_visible_container_auxiliaries() {
        let mut tree = Tree::new();
        let active_project = tree.add_project(PathBuf::from("/active")).unwrap();
        let inactive_project = tree.add_project(PathBuf::from("/inactive")).unwrap();
        let active_group = tree.add_group(active_project, "active").unwrap();
        let inactive_group = tree.add_group(active_project, "inactive").unwrap();
        let active = tree
            .add_pane(active_group, "active terminal", PaneContentKind::Terminal)
            .unwrap();
        let inactive = tree
            .add_pane(
                inactive_group,
                "inactive terminal",
                PaneContentKind::Terminal,
            )
            .unwrap();
        let editor = tree
            .add_pane(active_project, "editor", PaneContentKind::Editor)
            .unwrap();
        let hidden_editor = tree
            .add_pane(inactive_group, "hidden editor", PaneContentKind::Editor)
            .unwrap();
        let other_editor = tree
            .add_pane(inactive_project, "other editor", PaneContentKind::Editor)
            .unwrap();
        let visible = visible_nodes(&tree, &HashSet::from([active, editor, NodeId(u64::MAX)]));
        for id in [active_project, active_group, active, editor] {
            assert!(visible.contains(&id));
        }
        for id in [
            inactive_project,
            inactive_group,
            inactive,
            hidden_editor,
            other_editor,
        ] {
            assert!(!visible.contains(&id));
        }
    }

    #[test]
    fn no_active_terminals_produces_no_visible_projects() {
        let mut tree = Tree::new();
        let project = tree.add_project(PathBuf::from("/project")).unwrap();
        let editor = tree
            .add_pane(project, "editor", PaneContentKind::Editor)
            .unwrap();
        assert!(visible_nodes(&tree, &HashSet::from([editor])).is_empty());
    }
}

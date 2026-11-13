//! Structural move rules of the tree: one visible step up or down, indent and outdent.
//!
//! Ports of `Tree::move_node_one_step` (ilium-core), `compute_indent_target` and
//! `compute_outdent_target` (ilium-client keys). The rules read the tree only through the
//! [`TreeFacts`] trait, so they are pure: the plugin implements the trait over the `tree` host
//! import and the tests over a small in-memory tree. A node whose parent is `None` is a top-level
//! group, the child of the hidden root of the kernel tree.

use ilium_plugin_sdk::types::Placement;

/// Direction of a one-step move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Towards the top of the tree.
    Up,
    /// Towards the bottom of the tree.
    Down,
}

/// The most panes a split view holds.
pub const MAXIMUM_SPLIT_VIEW_PANES: usize = 4;

/// What the move rules need to know about the tree.
pub trait TreeFacts {
    /// The parent of a node; `None` for a top-level group.
    fn parent(&self, node: u64) -> Option<u64>;
    /// The children of a node in their stored order; `None` lists the top-level groups.
    fn children(&self, parent: Option<u64>) -> Vec<u64>;
    /// Whether the node is a pane.
    fn is_pane(&self, node: u64) -> bool;
    /// Whether the node is a group or a project, which accept ordinary children.
    fn accepts_normal_children(&self, node: u64) -> bool;
    /// Whether the node is a normal group (not a project).
    fn is_group(&self, node: u64) -> bool;
    /// Whether the node is a split view.
    fn is_split_view(&self, node: u64) -> bool;
    /// Whether the node is a project.
    fn is_project(&self, node: u64) -> bool;
    /// Whether the node is a worktree container or sits inside one.
    fn in_worktree(&self, node: u64) -> bool;
}

/// Where a move lands: next to a node, or inside a container.
pub type Target = (u64, Placement);

/// The nearest project at or above a node.
fn project_of(facts: &impl TreeFacts, node: u64) -> Option<u64> {
    let mut current = Some(node);
    let mut guard = 0;
    while let Some(id) = current {
        if facts.is_project(id) {
            return Some(id);
        }
        guard += 1;
        if guard > 4096 {
            return None;
        }
        current = facts.parent(id);
    }
    None
}

/// The nearest sibling group before or after `group`, walking out of nested groups only when no
/// peer group exists at the current level. Candidates stay inside the project of `group`.
fn adjacent_group(facts: &impl TreeFacts, group: u64, direction: Direction) -> Option<u64> {
    let project = project_of(facts, group);
    let is_candidate = |candidate: u64| {
        facts.accepts_normal_children(candidate) && project_of(facts, candidate) == project
    };
    let mut current = group;
    for _ in 0..4096 {
        let parent = facts.parent(current);
        let siblings = facts.children(parent);
        let position = siblings.iter().position(|sibling| *sibling == current)?;
        let found = match direction {
            Direction::Up => siblings[..position]
                .iter()
                .rev()
                .copied()
                .find(|candidate| is_candidate(*candidate)),
            Direction::Down => siblings[position + 1..]
                .iter()
                .copied()
                .find(|candidate| is_candidate(*candidate)),
        };
        if found.is_some() {
            return found;
        }
        parent?;
        current = parent?;
    }
    None
}

/// Where a node lands when it moves one visible step. Within a container it swaps with its
/// neighbour. At a nested container boundary a pane leaves into the parent group right before or
/// after that container; at a top-level group boundary it moves into the nearest adjacent group,
/// appended on an upward move and prepended on a downward one. A group only reorders among its
/// siblings. `None` means the node is at an outer boundary.
pub fn one_step_target(facts: &impl TreeFacts, node: u64, direction: Direction) -> Option<Target> {
    let parent = facts.parent(node);
    let siblings = facts.children(parent);
    let position = siblings.iter().position(|sibling| *sibling == node)?;
    let neighbour = match direction {
        Direction::Up => position.checked_sub(1),
        Direction::Down => (position + 1 < siblings.len()).then_some(position + 1),
    };
    if let Some(index) = neighbour {
        let placement = match direction {
            Direction::Up => Placement::Before,
            Direction::Down => Placement::After,
        };
        return Some((siblings[index], placement));
    }
    if !facts.is_pane(node) {
        return None;
    }
    let container = parent?;
    if let Some(parent_group) = facts.parent(container) {
        if facts.accepts_normal_children(parent_group) {
            let placement = match direction {
                Direction::Up => Placement::Before,
                Direction::Down => Placement::After,
            };
            return Some((container, placement));
        }
    }
    let adjacent = adjacent_group(facts, container, direction)?;
    let placement = match direction {
        Direction::Up => Placement::InsideLast,
        Direction::Down => Placement::InsideFirst,
    };
    Some((adjacent, placement))
}

/// The indent target of a node: the nearest preceding sibling that is a group, or a split view
/// with room when the node is a pane, appended at the end of its children. `None` when nothing
/// precedes the node or the node belongs to a worktree.
pub fn indent_target(facts: &impl TreeFacts, node: u64) -> Option<Target> {
    if facts.in_worktree(node) {
        return None;
    }
    let siblings = facts.children(facts.parent(node));
    let own_index = siblings.iter().position(|sibling| *sibling == node)?;
    let moving_is_pane = facts.is_pane(node);
    let container = siblings[..own_index]
        .iter()
        .rev()
        .copied()
        .find(|candidate| {
            if facts.is_group(*candidate) {
                true
            } else if facts.is_split_view(*candidate) {
                moving_is_pane && facts.children(Some(*candidate)).len() < MAXIMUM_SPLIT_VIEW_PANES
            } else {
                false
            }
        })?;
    Some((container, Placement::InsideLast))
}

/// The outdent target of a node: right after its current group, in the parent of that group.
/// `None` when the node is at the top level, or when a pane would be left directly under the
/// hidden root, because panes always need an enclosing group.
pub fn outdent_target(facts: &impl TreeFacts, node: u64) -> Option<Target> {
    if facts.in_worktree(node) {
        return None;
    }
    let current_group = facts.parent(node)?;
    let grandparent = facts.parent(current_group);
    if grandparent.is_none() && facts.is_pane(node) {
        return None;
    }
    Some((current_group, Placement::After))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    #[derive(Clone, Copy, PartialEq)]
    enum Kind {
        Group,
        Project,
        Split,
        Pane,
    }

    #[derive(Default)]
    struct Fake {
        nodes: BTreeMap<u64, (Option<u64>, Kind)>,
    }

    impl Fake {
        fn add(&mut self, parent: Option<u64>, kind: Kind) -> u64 {
            let id = self.nodes.len() as u64 + 1;
            self.nodes.insert(id, (parent, kind));
            id
        }
    }

    impl TreeFacts for Fake {
        fn parent(&self, node: u64) -> Option<u64> {
            self.nodes.get(&node).and_then(|entry| entry.0)
        }
        fn children(&self, parent: Option<u64>) -> Vec<u64> {
            self.nodes
                .iter()
                .filter(|(_, entry)| entry.0 == parent)
                .map(|(id, _)| *id)
                .collect()
        }
        fn is_pane(&self, node: u64) -> bool {
            self.nodes
                .get(&node)
                .is_some_and(|entry| entry.1 == Kind::Pane)
        }
        fn accepts_normal_children(&self, node: u64) -> bool {
            self.nodes
                .get(&node)
                .is_some_and(|entry| matches!(entry.1, Kind::Group | Kind::Project))
        }
        fn is_group(&self, node: u64) -> bool {
            self.nodes
                .get(&node)
                .is_some_and(|entry| entry.1 == Kind::Group)
        }
        fn is_split_view(&self, node: u64) -> bool {
            self.nodes
                .get(&node)
                .is_some_and(|entry| entry.1 == Kind::Split)
        }
        fn is_project(&self, node: u64) -> bool {
            self.nodes
                .get(&node)
                .is_some_and(|entry| entry.1 == Kind::Project)
        }
        fn in_worktree(&self, _node: u64) -> bool {
            false
        }
    }

    #[test]
    fn a_pane_swaps_with_its_neighbours_inside_a_group() {
        let mut tree = Fake::default();
        let group = tree.add(None, Kind::Group);
        let first = tree.add(Some(group), Kind::Pane);
        let second = tree.add(Some(group), Kind::Pane);
        assert_eq!(
            one_step_target(&tree, second, Direction::Up),
            Some((first, Placement::Before))
        );
        assert_eq!(
            one_step_target(&tree, first, Direction::Down),
            Some((second, Placement::After))
        );
    }

    #[test]
    fn a_pane_at_the_edge_of_a_nested_group_leaves_it() {
        let mut tree = Fake::default();
        let top = tree.add(None, Kind::Group);
        let inner = tree.add(Some(top), Kind::Group);
        let pane = tree.add(Some(inner), Kind::Pane);
        assert_eq!(
            one_step_target(&tree, pane, Direction::Up),
            Some((inner, Placement::Before))
        );
        assert_eq!(
            one_step_target(&tree, pane, Direction::Down),
            Some((inner, Placement::After))
        );
    }

    #[test]
    fn a_pane_at_a_top_level_boundary_enters_the_adjacent_group() {
        let mut tree = Fake::default();
        let first = tree.add(None, Kind::Group);
        let second = tree.add(None, Kind::Group);
        let pane = tree.add(Some(first), Kind::Pane);
        assert_eq!(
            one_step_target(&tree, pane, Direction::Down),
            Some((second, Placement::InsideFirst))
        );
        let last = tree.add(Some(second), Kind::Pane);
        assert_eq!(
            one_step_target(&tree, last, Direction::Up),
            Some((first, Placement::InsideLast))
        );
    }

    #[test]
    fn a_group_never_leaves_its_siblings_and_a_lone_pane_stays_at_the_outer_edge() {
        let mut tree = Fake::default();
        let only = tree.add(None, Kind::Group);
        let pane = tree.add(Some(only), Kind::Pane);
        assert_eq!(one_step_target(&tree, only, Direction::Up), None);
        assert_eq!(one_step_target(&tree, only, Direction::Down), None);
        assert_eq!(one_step_target(&tree, pane, Direction::Up), None);
        assert_eq!(one_step_target(&tree, pane, Direction::Down), None);
    }

    #[test]
    fn a_pane_never_crosses_a_project_boundary() {
        let mut tree = Fake::default();
        let project_a = tree.add(None, Kind::Project);
        let project_b = tree.add(None, Kind::Project);
        let group_a = tree.add(Some(project_a), Kind::Group);
        tree.add(Some(project_b), Kind::Group);
        let pane = tree.add(Some(group_a), Kind::Pane);
        // The pane exits into project A only; the neighbouring project is not a candidate.
        assert_eq!(
            one_step_target(&tree, pane, Direction::Down),
            Some((group_a, Placement::After))
        );
    }

    #[test]
    fn indent_skips_panes_and_full_split_views() {
        let mut tree = Fake::default();
        let top = tree.add(None, Kind::Group);
        let split = tree.add(Some(top), Kind::Split);
        for _ in 0..MAXIMUM_SPLIT_VIEW_PANES {
            tree.add(Some(split), Kind::Pane);
        }
        let pane = tree.add(Some(top), Kind::Pane);
        assert_eq!(indent_target(&tree, pane), None);
    }
}

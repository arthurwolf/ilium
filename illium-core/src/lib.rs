//! Pure domain model for illium: a tree of Groups and Panes, no I/O.
//!
//! This crate has zero dependency on tokio, portable-pty, ratatui, or any
//! other adapter. Everything here must stay unit-testable with plain
//! `#[test]` functions.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct NodeId(pub u64);

pub const ROOT_ID: NodeId = NodeId(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PaneContentKind {
    Terminal,
    Editor,
}

/// Direction used by tree-row move controls. The domain owns the boundary
/// rule because moving a pane across groups is a structural mutation, not a
/// rendering concern.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TreeMoveDirection {
    Up,
    Down,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentClass {
    Claude,
    Codex,
    Other(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentActivity {
    Working,
    WaitingApproval,
    /// A working turn just finished and nobody has looked at this pane
    /// since -- distinct from `Idle` so the UI can flag it for attention
    /// until the user focuses the pane, at which point it reverts to
    /// `Idle`.
    Done,
    Idle,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PaneStatus {
    /// Terminal pane, no agent CLI detected in it.
    PlainShell,
    /// Terminal pane running a detected agent CLI.
    Agent(AgentClass, AgentActivity),
    /// Editor pane; `true` means it has unsaved changes.
    Editor { dirty: bool },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum NodeKind {
    Group {
        children: Vec<NodeId>,
        expanded: bool,
    },
    Pane {
        content: PaneContentKind,
        status: PaneStatus,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    pub parent: Option<NodeId>,
    pub name: String,
    pub kind: NodeKind,
}

impl Node {
    pub fn is_group(&self) -> bool {
        matches!(self.kind, NodeKind::Group { .. })
    }

    pub fn is_pane(&self) -> bool {
        matches!(self.kind, NodeKind::Pane { .. })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TreeError {
    #[error("node {0:?} not found")]
    NodeNotFound(NodeId),
    #[error("node {0:?} is not a group")]
    NotAGroup(NodeId),
    #[error("node {0:?} is not a pane")]
    NotAPane(NodeId),
    #[error("cannot remove the root group")]
    CannotRemoveRoot,
    #[error("cannot move the root group")]
    CannotMoveRoot,
    #[error("cannot move node {0:?} into its own descendant {1:?}")]
    CannotMoveIntoDescendant(NodeId, NodeId),
    #[error("panes cannot be direct children of the session root; put them in a group")]
    PanesRequireGroup,
}

/// One entry in the flattened, top-level-first listing returned by
/// [`Tree::list_groups`]: a group's id and its nesting depth (`0` for
/// the session root itself -- illium's "top level" -- `1` for a
/// top-level group, `2` for a group nested one level inside that, etc).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GroupListing {
    pub id: NodeId,
    pub depth: usize,
}

/// Session tree: one root Group (id 0) containing an arbitrary-depth mix
/// of Groups and Panes.
///
/// Derives `Serialize`/`Deserialize` so `illium-server` can hand a full
/// snapshot to `illium-ipc` for the `TreeSnapshot` event and so the JSON
/// crash-recovery snapshot (README M5) can persist it; deriving serde here
/// is not an I/O concern, it's just data shape, so it doesn't violate this
/// crate's "no I/O" rule.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tree {
    nodes: HashMap<NodeId, Node>,
    next_id: u64,
}

impl Default for Tree {
    fn default() -> Self {
        Self::new()
    }
}

impl Tree {
    pub fn new() -> Self {
        let mut nodes = HashMap::new();
        nodes.insert(
            ROOT_ID,
            Node {
                id: ROOT_ID,
                parent: None,
                name: "session".to_string(),
                kind: NodeKind::Group {
                    children: Vec::new(),
                    expanded: true,
                },
            },
        );
        Tree { nodes, next_id: 1 }
    }

    fn alloc_id(&mut self) -> NodeId {
        let id = NodeId(self.next_id);
        self.next_id += 1;
        id
    }

    pub fn get(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(&id)
    }

    fn get_mut(&mut self, id: NodeId) -> Result<&mut Node, TreeError> {
        self.nodes.get_mut(&id).ok_or(TreeError::NodeNotFound(id))
    }

    pub fn children_of(&self, id: NodeId) -> Result<&[NodeId], TreeError> {
        match &self.get(id).ok_or(TreeError::NodeNotFound(id))?.kind {
            NodeKind::Group { children, .. } => Ok(children.as_slice()),
            NodeKind::Pane { .. } => Err(TreeError::NotAGroup(id)),
        }
    }

    pub fn parent_of(&self, id: NodeId) -> Option<NodeId> {
        self.get(id).and_then(|n| n.parent)
    }

    /// All pane nodes in the tree, in no particular order.
    pub fn panes(&self) -> impl Iterator<Item = &Node> {
        self.nodes.values().filter(|n| n.is_pane())
    }

    /// All node ids, in no particular order (used for persistence/debugging).
    pub fn all_ids(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.nodes.keys().copied()
    }

    fn push_child(&mut self, parent: NodeId, child: NodeId) -> Result<(), TreeError> {
        match &mut self.get_mut(parent)?.kind {
            NodeKind::Group { children, .. } => {
                children.push(child);
                Ok(())
            }
            NodeKind::Pane { .. } => Err(TreeError::NotAGroup(parent)),
        }
    }

    fn remove_child(&mut self, parent: NodeId, child: NodeId) -> Result<(), TreeError> {
        match &mut self.get_mut(parent)?.kind {
            NodeKind::Group { children, .. } => {
                children.retain(|c| *c != child);
                Ok(())
            }
            NodeKind::Pane { .. } => Err(TreeError::NotAGroup(parent)),
        }
    }

    pub fn add_group(
        &mut self,
        parent: NodeId,
        name: impl Into<String>,
    ) -> Result<NodeId, TreeError> {
        if self.get(parent).is_none() {
            return Err(TreeError::NodeNotFound(parent));
        }
        let id = self.alloc_id();
        self.nodes.insert(
            id,
            Node {
                id,
                parent: Some(parent),
                name: name.into(),
                kind: NodeKind::Group {
                    children: Vec::new(),
                    expanded: true,
                },
            },
        );
        self.push_child(parent, id)?;
        Ok(id)
    }

    pub fn add_pane(
        &mut self,
        parent: NodeId,
        name: impl Into<String>,
        content: PaneContentKind,
    ) -> Result<NodeId, TreeError> {
        if self.get(parent).is_none() {
            return Err(TreeError::NodeNotFound(parent));
        }
        if parent == ROOT_ID {
            return Err(TreeError::PanesRequireGroup);
        }
        let id = self.alloc_id();
        let status = match content {
            PaneContentKind::Terminal => PaneStatus::PlainShell,
            PaneContentKind::Editor => PaneStatus::Editor { dirty: false },
        };
        self.nodes.insert(
            id,
            Node {
                id,
                parent: Some(parent),
                name: name.into(),
                kind: NodeKind::Pane { content, status },
            },
        );
        self.push_child(parent, id)?;
        Ok(id)
    }

    /// Removes a node. If it's a Group, removes its whole subtree.
    pub fn remove_node(&mut self, id: NodeId) -> Result<(), TreeError> {
        if id == ROOT_ID {
            return Err(TreeError::CannotRemoveRoot);
        }
        let node = self.get(id).ok_or(TreeError::NodeNotFound(id))?;
        let parent = node.parent;

        // Collect the whole subtree first (BFS), then remove it in one pass.
        let mut to_remove = vec![id];
        let mut frontier = vec![id];
        while let Some(current) = frontier.pop() {
            if let Some(Node {
                kind: NodeKind::Group { children, .. },
                ..
            }) = self.nodes.get(&current)
            {
                for child in children.clone() {
                    to_remove.push(child);
                    frontier.push(child);
                }
            }
        }
        for node_id in &to_remove {
            self.nodes.remove(node_id);
        }
        if let Some(parent) = parent {
            self.remove_child(parent, id)?;
        }
        Ok(())
    }

    pub fn rename_node(&mut self, id: NodeId, name: impl Into<String>) -> Result<(), TreeError> {
        self.get_mut(id)?.name = name.into();
        Ok(())
    }

    pub fn toggle_expanded(&mut self, id: NodeId) -> Result<(), TreeError> {
        match &mut self.get_mut(id)?.kind {
            NodeKind::Group { expanded, .. } => {
                *expanded = !*expanded;
                Ok(())
            }
            NodeKind::Pane { .. } => Err(TreeError::NotAGroup(id)),
        }
    }

    pub fn set_pane_status(&mut self, id: NodeId, status: PaneStatus) -> Result<(), TreeError> {
        match &mut self.get_mut(id)?.kind {
            NodeKind::Pane { status: s, .. } => {
                *s = status;
                Ok(())
            }
            NodeKind::Group { .. } => Err(TreeError::NotAPane(id)),
        }
    }

    /// True if `ancestor` is `node` itself or a transitive parent of `node`.
    fn is_ancestor_of(&self, ancestor: NodeId, node: NodeId) -> bool {
        let mut current = Some(node);
        while let Some(c) = current {
            if c == ancestor {
                return true;
            }
            current = self.parent_of(c);
        }
        false
    }

    /// Moves `id` to become a child of `new_parent`, inserted at `index`
    /// within the new parent's children (clamped to the list length; `None`
    /// appends at the end).
    pub fn move_node(
        &mut self,
        id: NodeId,
        new_parent: NodeId,
        index: Option<usize>,
    ) -> Result<(), TreeError> {
        if id == ROOT_ID {
            return Err(TreeError::CannotMoveRoot);
        }
        if self.get(new_parent).is_none() {
            return Err(TreeError::NodeNotFound(new_parent));
        }
        if !matches!(self.get(new_parent).unwrap().kind, NodeKind::Group { .. }) {
            return Err(TreeError::NotAGroup(new_parent));
        }
        if new_parent == ROOT_ID && self.get(id).ok_or(TreeError::NodeNotFound(id))?.is_pane() {
            return Err(TreeError::PanesRequireGroup);
        }
        // A node can't be moved into itself or one of its own descendants.
        if self.is_ancestor_of(id, new_parent) {
            return Err(TreeError::CannotMoveIntoDescendant(id, new_parent));
        }
        let old_parent = self.get(id).ok_or(TreeError::NodeNotFound(id))?.parent;
        if let Some(old_parent) = old_parent {
            self.remove_child(old_parent, id)?;
        }
        match &mut self.get_mut(new_parent)?.kind {
            NodeKind::Group { children, .. } => {
                let index = index.unwrap_or(children.len()).min(children.len());
                children.insert(index, id);
            }
            NodeKind::Pane { .. } => unreachable!("checked above"),
        }
        self.get_mut(id)?.parent = Some(new_parent);
        Ok(())
    }

    /// Returns the first top-level group under the session root, creating
    /// one named `name` if none exists yet. Callers that need somewhere to
    /// put a new pane (boot, or a UI fallback with no more specific target)
    /// use this instead of ever handing `add_pane` the root id directly.
    pub fn ensure_default_group(&mut self, name: impl Into<String>) -> NodeId {
        let existing = self.children_of(ROOT_ID).ok().and_then(|children| {
            children.iter().copied().find(|&child| {
                matches!(
                    self.get(child).map(|n| &n.kind),
                    Some(NodeKind::Group { .. })
                )
            })
        });
        match existing {
            Some(id) => id,
            None => self
                .add_group(ROOT_ID, name)
                .expect("root exists and accepts group children"),
        }
    }

    /// Every group in the tree (Panes excluded), pre-order, in the exact
    /// order they render in the tree panel, prefixed with a `ROOT_ID` entry
    /// standing for "the top level" itself. Used by the "create group"
    /// destination picker, so a user choosing where to nest a new group sees
    /// the same structure and ordering the tree panel already shows them.
    pub fn list_groups(&self) -> Vec<GroupListing> {
        let mut destinations = vec![GroupListing {
            id: ROOT_ID,
            depth: 0,
        }];
        self.collect_group_listings(ROOT_ID, 1, &mut destinations);
        destinations
    }

    fn collect_group_listings(&self, parent: NodeId, depth: usize, out: &mut Vec<GroupListing>) {
        let Ok(children) = self.children_of(parent) else {
            return;
        };
        for &child in children {
            if matches!(
                self.get(child).map(|node| &node.kind),
                Some(NodeKind::Group { .. })
            ) {
                out.push(GroupListing { id: child, depth });
                self.collect_group_listings(child, depth + 1, out);
            }
        }
    }

    /// Shifts `id` by `delta` positions among its siblings (negative = up
    /// toward index 0, positive = down). Out-of-range shifts clamp instead
    /// of erroring.
    pub fn reorder_sibling(&mut self, id: NodeId, delta: i32) -> Result<(), TreeError> {
        let parent = self.get(id).ok_or(TreeError::NodeNotFound(id))?.parent;
        let Some(parent) = parent else {
            return Ok(()); // root has no siblings
        };
        match &mut self.get_mut(parent)?.kind {
            NodeKind::Group { children, .. } => {
                let Some(pos) = children.iter().position(|c| *c == id) else {
                    return Ok(());
                };
                let new_pos = (pos as i32 + delta).clamp(0, children.len() as i32 - 1) as usize;
                children.remove(pos);
                children.insert(new_pos, id);
                Ok(())
            }
            NodeKind::Pane { .. } => unreachable!("parent of a node is always a group"),
        }
    }

    /// Moves a node one visible step in its tree ordering. Within a group it
    /// reorders siblings. At a pane's group boundary it transfers the pane
    /// into the nearest preceding/following group, appending on an upward
    /// move and inserting at index zero on a downward move. This preserves
    /// the invariant that panes are always children of a Group.
    ///
    /// Groups themselves only reorder among their current siblings: moving a
    /// boundary group into another group would change the hierarchy rather
    /// than its visible ordering and would be surprising for a tiny arrow.
    /// Returns `true` when a move occurred and `false` at an outer boundary.
    pub fn move_node_one_step(
        &mut self,
        id: NodeId,
        direction: TreeMoveDirection,
    ) -> Result<bool, TreeError> {
        let node = self.get(id).ok_or(TreeError::NodeNotFound(id))?;
        let parent = node.parent.ok_or(TreeError::CannotMoveRoot)?;
        let is_pane = node.is_pane();
        let siblings = self.children_of(parent)?;
        let position = siblings
            .iter()
            .position(|sibling| *sibling == id)
            .ok_or(TreeError::NodeNotFound(id))?;

        let sibling_target = match direction {
            TreeMoveDirection::Up => position.checked_sub(1),
            TreeMoveDirection::Down => (position + 1 < siblings.len()).then_some(position + 1),
        };
        if let Some(target) = sibling_target {
            let delta = if target < position { -1 } else { 1 };
            self.reorder_sibling(id, delta)?;
            return Ok(true);
        }
        if !is_pane {
            return Ok(false);
        }

        let Some(adjacent_group) = self.adjacent_group(parent, direction) else {
            return Ok(false);
        };
        let index = match direction {
            TreeMoveDirection::Up => None,
            TreeMoveDirection::Down => Some(0),
        };
        self.move_node(id, adjacent_group, index)?;
        Ok(true)
    }

    /// Finds the nearest sibling group before/after `group`, walking out of
    /// nested groups only when no peer group exists at the current level.
    fn adjacent_group(&self, group: NodeId, direction: TreeMoveDirection) -> Option<NodeId> {
        let mut current_group = group;
        loop {
            let parent = self.parent_of(current_group)?;
            let siblings = self.children_of(parent).ok()?;
            let position = siblings
                .iter()
                .position(|sibling| *sibling == current_group)?;
            let candidates: Box<dyn Iterator<Item = &NodeId>> = match direction {
                TreeMoveDirection::Up => Box::new(siblings[..position].iter().rev()),
                TreeMoveDirection::Down => Box::new(siblings[position + 1..].iter()),
            };
            if let Some(group_id) = candidates.copied().find(|candidate| {
                matches!(
                    self.get(*candidate).map(|node| &node.kind),
                    Some(NodeKind::Group { .. })
                )
            }) {
                return Some(group_id);
            }
            if parent == ROOT_ID {
                return None;
            }
            current_group = parent;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_tree_has_root_group() {
        let tree = Tree::new();
        let root = tree.get(ROOT_ID).unwrap();
        assert!(root.is_group());
        assert_eq!(tree.children_of(ROOT_ID).unwrap().len(), 0);
    }

    #[test]
    fn list_groups_is_top_level_first_then_pre_order_nesting() {
        let mut tree = Tree::new();
        let backend = tree.add_group(ROOT_ID, "backend").unwrap();
        let api = tree.add_group(backend, "api").unwrap();
        let frontend = tree.add_group(ROOT_ID, "frontend").unwrap();
        // A pane must never appear in the listing -- only groups do.
        tree.add_pane(api, "shell", PaneContentKind::Terminal)
            .unwrap();

        let listing = tree.list_groups();
        assert_eq!(
            listing,
            vec![
                GroupListing {
                    id: ROOT_ID,
                    depth: 0
                },
                GroupListing {
                    id: backend,
                    depth: 1
                },
                GroupListing { id: api, depth: 2 },
                GroupListing {
                    id: frontend,
                    depth: 1
                },
            ]
        );
    }

    #[test]
    fn list_groups_on_empty_tree_is_just_the_top_level() {
        let tree = Tree::new();
        assert_eq!(
            tree.list_groups(),
            vec![GroupListing {
                id: ROOT_ID,
                depth: 0
            }]
        );
    }

    #[test]
    fn add_pane_appears_under_parent() {
        let mut tree = Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        let pane = tree
            .add_pane(group, "shell", PaneContentKind::Terminal)
            .unwrap();
        assert_eq!(tree.children_of(group).unwrap(), &[pane]);
        assert_eq!(tree.parent_of(pane), Some(group));
    }

    #[test]
    fn add_group_and_nest_pane() {
        let mut tree = Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        let pane = tree
            .add_pane(group, "shell", PaneContentKind::Terminal)
            .unwrap();
        assert_eq!(tree.children_of(group).unwrap(), &[pane]);
        assert_eq!(tree.children_of(ROOT_ID).unwrap(), &[group]);
    }

    #[test]
    fn remove_group_removes_descendants() {
        let mut tree = Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        let pane = tree
            .add_pane(group, "shell", PaneContentKind::Terminal)
            .unwrap();
        tree.remove_node(group).unwrap();
        assert!(tree.get(group).is_none());
        assert!(tree.get(pane).is_none());
        assert_eq!(tree.children_of(ROOT_ID).unwrap().len(), 0);
    }

    #[test]
    fn cannot_remove_root() {
        let mut tree = Tree::new();
        assert!(matches!(
            tree.remove_node(ROOT_ID),
            Err(TreeError::CannotRemoveRoot)
        ));
    }

    #[test]
    fn move_node_between_groups() {
        let mut tree = Tree::new();
        let a = tree.add_group(ROOT_ID, "a").unwrap();
        let b = tree.add_group(ROOT_ID, "b").unwrap();
        let pane = tree
            .add_pane(a, "shell", PaneContentKind::Terminal)
            .unwrap();
        tree.move_node(pane, b, None).unwrap();
        assert_eq!(tree.children_of(a).unwrap().len(), 0);
        assert_eq!(tree.children_of(b).unwrap(), &[pane]);
        assert_eq!(tree.parent_of(pane), Some(b));
    }

    #[test]
    fn cannot_move_group_into_its_own_descendant() {
        let mut tree = Tree::new();
        let a = tree.add_group(ROOT_ID, "a").unwrap();
        let b = tree.add_group(a, "b").unwrap();
        let err = tree.move_node(a, b, None).unwrap_err();
        assert!(matches!(err, TreeError::CannotMoveIntoDescendant(_, _)));
    }

    #[test]
    fn reorder_sibling_moves_within_parent() {
        let mut tree = Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        let a = tree
            .add_pane(group, "a", PaneContentKind::Terminal)
            .unwrap();
        let b = tree
            .add_pane(group, "b", PaneContentKind::Terminal)
            .unwrap();
        let c = tree
            .add_pane(group, "c", PaneContentKind::Terminal)
            .unwrap();
        tree.reorder_sibling(a, 1).unwrap();
        assert_eq!(tree.children_of(group).unwrap(), &[b, a, c]);
        tree.reorder_sibling(c, -5).unwrap();
        assert_eq!(tree.children_of(group).unwrap(), &[c, b, a]);
    }

    #[test]
    fn pane_arrow_moves_across_adjacent_groups_at_boundaries() {
        let mut tree = Tree::new();
        let first = tree.add_group(ROOT_ID, "first").unwrap();
        let second = tree.add_group(ROOT_ID, "second").unwrap();
        let first_pane = tree
            .add_pane(first, "first-pane", PaneContentKind::Terminal)
            .unwrap();
        let second_pane = tree
            .add_pane(second, "second-pane", PaneContentKind::Terminal)
            .unwrap();

        assert!(tree
            .move_node_one_step(first_pane, TreeMoveDirection::Down)
            .unwrap());
        assert_eq!(tree.children_of(first).unwrap(), &[]);
        assert_eq!(
            tree.children_of(second).unwrap(),
            &[first_pane, second_pane]
        );

        assert!(tree
            .move_node_one_step(first_pane, TreeMoveDirection::Up)
            .unwrap());
        assert_eq!(tree.children_of(first).unwrap(), &[first_pane]);
        assert_eq!(tree.children_of(second).unwrap(), &[second_pane]);
    }

    #[test]
    fn group_arrow_stops_at_its_sibling_boundary() {
        let mut tree = Tree::new();
        let first = tree.add_group(ROOT_ID, "first").unwrap();
        let second = tree.add_group(ROOT_ID, "second").unwrap();

        assert!(tree
            .move_node_one_step(first, TreeMoveDirection::Down)
            .unwrap());
        assert_eq!(tree.children_of(ROOT_ID).unwrap(), &[second, first]);
        assert!(!tree
            .move_node_one_step(first, TreeMoveDirection::Down)
            .unwrap());
    }

    #[test]
    fn add_pane_directly_under_root_is_rejected() {
        let mut tree = Tree::new();
        let err = tree
            .add_pane(ROOT_ID, "shell", PaneContentKind::Terminal)
            .unwrap_err();
        assert!(matches!(err, TreeError::PanesRequireGroup));
    }

    #[test]
    fn move_pane_directly_under_root_is_rejected() {
        let mut tree = Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        let pane = tree
            .add_pane(group, "shell", PaneContentKind::Terminal)
            .unwrap();
        let err = tree.move_node(pane, ROOT_ID, None).unwrap_err();
        assert!(matches!(err, TreeError::PanesRequireGroup));
        assert_eq!(tree.parent_of(pane), Some(group));
    }

    #[test]
    fn ensure_default_group_creates_once_then_reuses() {
        let mut tree = Tree::new();
        let first = tree.ensure_default_group("default");
        assert_eq!(tree.children_of(ROOT_ID).unwrap(), &[first]);
        let second = tree.ensure_default_group("default");
        assert_eq!(first, second);
        assert_eq!(tree.children_of(ROOT_ID).unwrap().len(), 1);
    }

    #[test]
    fn ensure_default_group_reuses_any_existing_top_level_group() {
        let mut tree = Tree::new();
        let work = tree.add_group(ROOT_ID, "work").unwrap();
        let default = tree.ensure_default_group("default");
        assert_eq!(work, default);
    }

    #[test]
    fn rename_and_toggle_expanded() {
        let mut tree = Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        tree.rename_node(group, "renamed").unwrap();
        assert_eq!(tree.get(group).unwrap().name, "renamed");
        tree.toggle_expanded(group).unwrap();
        match tree.get(group).unwrap().kind {
            NodeKind::Group { expanded, .. } => assert!(!expanded),
            _ => panic!("expected group"),
        }
    }
}

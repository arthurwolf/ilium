//! Pure domain model for ilium: a tree of containers, panes, and folder
//! references, no I/O. Containers are normal groups or bounded split views.
//!
//! This crate has zero dependency on tokio, portable-pty, ratatui, or any
//! other adapter. Everything here must stay unit-testable with plain
//! `#[test]` functions.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct NodeId(pub u64);

pub const ROOT_ID: NodeId = NodeId(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PaneContentKind {
    Terminal,
    Editor,
    Board,
}

/// The user-owned document backing a kanban board.  The server persists this
/// descriptor with the tree, while the client owns the local file I/O needed
/// to render and mutate the board.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BoardStorage {
    /// Each immediate child directory is one column; Markdown files inside
    /// it are cards.
    Folder { path: PathBuf },
    /// Headings are columns and direct bullet-list items are cards.
    MarkdownFile { path: PathBuf },
}

impl BoardStorage {
    pub fn path(&self) -> &std::path::Path {
        match self {
            Self::Folder { path } | Self::MarkdownFile { path } => path,
        }
    }
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
    /// The agent has dispatched one or more background subagents/tasks and
    /// is waiting on them to finish -- distinct from `Working` (a live
    /// foreground turn actively streaming output) so the UI can show a
    /// calmer, slower animation instead of the fast "actively thinking"
    /// spinner. Parallels `WaitingApproval` (blocked on the user) as
    /// "blocked on background work" rather than "blocked on you".
    WaitingBackground,
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
    /// A client-local kanban board backed by a user-selected path.
    Board,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PaneTitleSource {
    Automatic,
    UserSpecified,
}

impl PaneTitleSource {
    pub const fn is_user_specified(self) -> bool {
        matches!(self, Self::UserSpecified)
    }
}

/// One durable, server-owned input scheduled for a terminal pane. The tree
/// carries the absolute deadline so every attached client renders the same
/// countdown and a detached server can still execute it after the UI exits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduledPaneInput {
    pub execute_at_unix_millis: u64,
    pub text: String,
    pub send_enter: bool,
}

impl ScheduledPaneInput {
    /// At least text or Enter must be present; otherwise the schedule would
    /// wake the server only to perform an invisible no-op.
    pub fn has_input(&self) -> bool {
        !self.text.is_empty() || self.send_enter
    }
}

pub const MAXIMUM_SPLIT_VIEW_PANES: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SplitOrientation {
    Vertical,
    Horizontal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContainerKind {
    Group,
    SplitView { orientation: SplitOrientation },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContainerNode {
    pub kind: ContainerKind,
    pub children: Vec<NodeId>,
    pub expanded: bool,
}

impl ContainerNode {
    pub fn group() -> Self {
        Self {
            kind: ContainerKind::Group,
            children: Vec::new(),
            expanded: true,
        }
    }

    pub fn split_view(orientation: SplitOrientation) -> Self {
        Self {
            kind: ContainerKind::SplitView { orientation },
            children: Vec::new(),
            expanded: true,
        }
    }

    pub const fn is_group(&self) -> bool {
        matches!(self.kind, ContainerKind::Group)
    }

    pub const fn is_split_view(&self) -> bool {
        matches!(self.kind, ContainerKind::SplitView { .. })
    }

    pub const fn split_orientation(&self) -> Option<SplitOrientation> {
        match self.kind {
            ContainerKind::Group => None,
            ContainerKind::SplitView { orientation } => Some(orientation),
        }
    }

    /// Validates adding a pane to this container. `is_existing_child`
    /// distinguishes a same-container reorder from a new member, because a
    /// full split must still allow its existing panes to be reordered.
    fn validate_pane_child(&self, is_existing_child: bool) -> Result<(), TreeError> {
        if self.is_split_view()
            && !is_existing_child
            && self.children.len() >= MAXIMUM_SPLIT_VIEW_PANES
        {
            return Err(TreeError::SplitViewCapacityReached);
        }
        Ok(())
    }

    /// Enforces the child policy owned by this container abstraction. Normal
    /// groups accept every domain node kind; split views accept panes only
    /// and delegate their capacity rule to [`Self::validate_pane_child`].
    fn validate_child(&self, child: &Node, is_existing_child: bool) -> Result<(), TreeError> {
        if !self.is_split_view() {
            return Ok(());
        }
        if !child.is_pane() {
            return Err(TreeError::SplitViewOnlyAcceptsPanes);
        }
        self.validate_pane_child(is_existing_child)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum NodeKind {
    Container(ContainerNode),
    Pane {
        content: PaneContentKind,
        status: PaneStatus,
        title_source: PaneTitleSource,
        /// Present exactly for [`PaneContentKind::Board`]. Kept in the pure
        /// tree rather than a server runtime because boards have no PTY or
        /// background process to own.
        board_storage: Option<BoardStorage>,
        /// At most one pending action per terminal pane. `serde(default)`
        /// preserves crash-recovery compatibility with snapshots written
        /// before scheduled input existed.
        #[serde(default)]
        scheduled_input: Option<ScheduledPaneInput>,
    },
    /// A persisted filesystem root. Its descendants are read locally by the
    /// client and intentionally never become server-owned domain nodes.
    Folder {
        path: PathBuf,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    pub parent: Option<NodeId>,
    pub name: String,
    /// A short (2-3 word) alternative to `name`, shown in place of it when
    /// the tree panel is too narrow to display the full name comfortably --
    /// see `ilium-client`'s `tree_ui` render path. Only ever populated by an
    /// LLM naming inference (`session_naming`/`terminal_naming`); a plain
    /// user-typed rename or the raw shell-command-echo titler have no
    /// distinct short form, so they leave this `None` and every width
    /// renders `name`.
    pub short_name: Option<String>,
    pub kind: NodeKind,
}

impl Node {
    pub fn is_group(&self) -> bool {
        matches!(&self.kind, NodeKind::Container(container) if container.is_group())
    }

    pub fn is_container(&self) -> bool {
        matches!(self.kind, NodeKind::Container(_))
    }

    pub fn is_split_view(&self) -> bool {
        matches!(&self.kind, NodeKind::Container(container) if container.is_split_view())
    }

    pub fn is_pane(&self) -> bool {
        matches!(self.kind, NodeKind::Pane { .. })
    }

    pub fn is_folder(&self) -> bool {
        matches!(self.kind, NodeKind::Folder { .. })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TreeError {
    #[error("node {0:?} not found")]
    NodeNotFound(NodeId),
    #[error("node {0:?} is not a group")]
    NotAGroup(NodeId),
    #[error("node {0:?} is not a container")]
    NotAContainer(NodeId),
    #[error("node {0:?} is not a pane")]
    NotAPane(NodeId),
    #[error("pane {0:?} is not a terminal")]
    NotATerminal(NodeId),
    #[error("scheduled input must contain text, Enter, or both")]
    EmptyScheduledInput,
    #[error("cannot remove the root group")]
    CannotRemoveRoot,
    #[error("cannot move the root group")]
    CannotMoveRoot,
    #[error("cannot move node {0:?} into its own descendant {1:?}")]
    CannotMoveIntoDescendant(NodeId, NodeId),
    #[error("panes cannot be direct children of the session root; put them in a group")]
    PanesRequireGroup,
    #[error("split views must be direct children of a normal group")]
    SplitViewRequiresGroup,
    #[error("split views can contain panes only")]
    SplitViewOnlyAcceptsPanes,
    #[error("split views can contain at most {MAXIMUM_SPLIT_VIEW_PANES} panes")]
    SplitViewCapacityReached,
    #[error("pane {0:?} is already inside a split view")]
    PaneAlreadyInSplitView(NodeId),
    #[error("pane {0:?} was selected more than once")]
    DuplicateSplitViewPane(NodeId),
}

/// One entry in the flattened, top-level-first listing returned by
/// [`Tree::list_groups`]: a group's id and its nesting depth (`0` for
/// the session root itself -- ilium's "top level" -- `1` for a
/// top-level group, `2` for a group nested one level inside that, etc).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GroupListing {
    pub id: NodeId,
    pub depth: usize,
}

/// Session tree: one root normal group (id 0) containing an arbitrary-depth
/// mix of normal groups, split-view containers, panes, and folder roots.
///
/// Derives `Serialize`/`Deserialize` so `ilium-server` can hand a full
/// snapshot to `ilium-ipc` for the `TreeSnapshot` event and so the JSON
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
                short_name: None,
                kind: NodeKind::Container(ContainerNode::group()),
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
            NodeKind::Container(container) => Ok(container.children.as_slice()),
            NodeKind::Pane { .. } | NodeKind::Folder { .. } => Err(TreeError::NotAContainer(id)),
        }
    }

    pub fn split_orientation(&self, id: NodeId) -> Option<SplitOrientation> {
        let NodeKind::Container(container) = &self.get(id)?.kind else {
            return None;
        };
        container.split_orientation()
    }

    pub fn parent_of(&self, id: NodeId) -> Option<NodeId> {
        self.get(id).and_then(|n| n.parent)
    }

    /// All pane nodes in the tree, in no particular order.
    pub fn panes(&self) -> impl Iterator<Item = &Node> {
        self.nodes.values().filter(|n| n.is_pane())
    }

    pub fn pane_ids_in_tree_order(&self) -> Vec<NodeId> {
        fn collect(tree: &Tree, parent: NodeId, pane_ids: &mut Vec<NodeId>) {
            let Ok(children) = tree.children_of(parent) else {
                return;
            };
            for child in children {
                match tree.get(*child) {
                    Some(node) if node.is_pane() => pane_ids.push(*child),
                    Some(node) if node.is_container() => collect(tree, *child, pane_ids),
                    _ => {}
                }
            }
        }

        let mut pane_ids = Vec::new();
        collect(self, ROOT_ID, &mut pane_ids);
        pane_ids
    }

    /// All node ids, in no particular order (used for persistence/debugging).
    pub fn all_ids(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.nodes.keys().copied()
    }

    fn push_child(&mut self, parent: NodeId, child: NodeId) -> Result<(), TreeError> {
        match &mut self.get_mut(parent)?.kind {
            NodeKind::Container(container) => {
                container.children.push(child);
                Ok(())
            }
            NodeKind::Pane { .. } | NodeKind::Folder { .. } => {
                Err(TreeError::NotAContainer(parent))
            }
        }
    }

    fn remove_child(&mut self, parent: NodeId, child: NodeId) -> Result<(), TreeError> {
        match &mut self.get_mut(parent)?.kind {
            NodeKind::Container(container) => {
                container.children.retain(|candidate| *candidate != child);
                Ok(())
            }
            NodeKind::Pane { .. } | NodeKind::Folder { .. } => {
                Err(TreeError::NotAContainer(parent))
            }
        }
    }

    pub fn add_group(
        &mut self,
        parent: NodeId,
        name: impl Into<String>,
    ) -> Result<NodeId, TreeError> {
        let parent_node = self.get(parent).ok_or(TreeError::NodeNotFound(parent))?;
        if !parent_node.is_group() {
            return Err(TreeError::NotAGroup(parent));
        }
        let id = self.alloc_id();
        self.nodes.insert(
            id,
            Node {
                id,
                parent: Some(parent),
                name: name.into(),
                short_name: None,
                kind: NodeKind::Container(ContainerNode::group()),
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
        if parent == ROOT_ID {
            return Err(TreeError::PanesRequireGroup);
        }
        self.validate_new_pane_parent(parent)?;
        let id = self.alloc_id();
        let status = match content {
            PaneContentKind::Terminal => PaneStatus::PlainShell,
            PaneContentKind::Editor => PaneStatus::Editor { dirty: false },
            PaneContentKind::Board => PaneStatus::Board,
        };
        self.nodes.insert(
            id,
            Node {
                id,
                parent: Some(parent),
                name: name.into(),
                short_name: None,
                kind: NodeKind::Pane {
                    content,
                    status,
                    title_source: PaneTitleSource::Automatic,
                    board_storage: None,
                    scheduled_input: None,
                },
            },
        );
        self.push_child(parent, id)?;
        Ok(id)
    }

    fn validate_new_pane_parent(&self, parent: NodeId) -> Result<(), TreeError> {
        if parent == ROOT_ID {
            return Err(TreeError::PanesRequireGroup);
        }
        let parent_node = self.get(parent).ok_or(TreeError::NodeNotFound(parent))?;
        let NodeKind::Container(container) = &parent_node.kind else {
            return Err(TreeError::NotAContainer(parent));
        };
        container.validate_pane_child(false)
    }

    /// Adds a board pane whose user-owned storage descriptor is persisted in
    /// the tree. Board I/O stays outside this pure domain crate.
    pub fn add_board(
        &mut self,
        parent: NodeId,
        name: String,
        storage: BoardStorage,
    ) -> Result<NodeId, TreeError> {
        self.validate_new_pane_parent(parent)?;
        let id = self.alloc_id();
        self.nodes.insert(
            id,
            Node {
                id,
                parent: Some(parent),
                name,
                short_name: None,
                kind: NodeKind::Pane {
                    content: PaneContentKind::Board,
                    status: PaneStatus::Board,
                    title_source: PaneTitleSource::Automatic,
                    board_storage: Some(storage),
                    scheduled_input: None,
                },
            },
        );
        self.push_child(parent, id)?;
        Ok(id)
    }

    /// Adds a filesystem root beneath a group. Only this stable reference is
    /// persisted; files under it remain local filesystem data.
    pub fn add_folder(&mut self, parent: NodeId, path: PathBuf) -> Result<NodeId, TreeError> {
        if !self
            .get(parent)
            .ok_or(TreeError::NodeNotFound(parent))?
            .is_group()
        {
            return Err(TreeError::NotAGroup(parent));
        }
        let name = path
            .file_name()
            .filter(|name| !name.is_empty())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        let id = self.alloc_id();
        self.nodes.insert(
            id,
            Node {
                id,
                parent: Some(parent),
                name,
                short_name: None,
                kind: NodeKind::Folder { path },
            },
        );
        self.push_child(parent, id)?;
        Ok(id)
    }

    pub fn create_split_view(
        &mut self,
        parent_group: NodeId,
        name: impl Into<String>,
        orientation: SplitOrientation,
        pane_ids: &[NodeId],
    ) -> Result<NodeId, TreeError> {
        if pane_ids.len() > MAXIMUM_SPLIT_VIEW_PANES {
            return Err(TreeError::SplitViewCapacityReached);
        }
        if !self
            .get(parent_group)
            .ok_or(TreeError::NodeNotFound(parent_group))?
            .is_group()
        {
            return Err(TreeError::SplitViewRequiresGroup);
        }

        let mut unique_panes = std::collections::HashSet::new();
        for pane_id in pane_ids {
            let pane = self
                .get(*pane_id)
                .ok_or(TreeError::NodeNotFound(*pane_id))?;
            if !pane.is_pane() {
                return Err(TreeError::NotAPane(*pane_id));
            }
            if !unique_panes.insert(*pane_id) {
                return Err(TreeError::DuplicateSplitViewPane(*pane_id));
            }
            if pane
                .parent
                .and_then(|parent| self.get(parent))
                .is_some_and(Node::is_split_view)
            {
                return Err(TreeError::PaneAlreadyInSplitView(*pane_id));
            }
        }

        let mut updated_tree = self.clone();
        let split_view_id = updated_tree.alloc_id();
        updated_tree.nodes.insert(
            split_view_id,
            Node {
                id: split_view_id,
                parent: Some(parent_group),
                name: name.into(),
                short_name: None,
                kind: NodeKind::Container(ContainerNode::split_view(orientation)),
            },
        );
        updated_tree.push_child(parent_group, split_view_id)?;
        for pane_id in pane_ids {
            updated_tree.move_node(*pane_id, split_view_id, None)?;
        }
        *self = updated_tree;
        Ok(split_view_id)
    }

    /// Removes a node. If it is a container, removes its whole subtree.
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
                kind: NodeKind::Container(ContainerNode { children, .. }),
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

    /// Unconditionally renames a group or pane; for a pane this also
    /// permanently marks it `UserSpecified`, so no automatic titler
    /// overwrites it again. `short_name` is the short-form alternative
    /// shown when the tree panel is narrow (`None` when the new name has
    /// no distinct short form, e.g. a user-typed rename).
    pub fn rename_node(
        &mut self,
        id: NodeId,
        name: impl Into<String>,
        short_name: Option<String>,
    ) -> Result<(), TreeError> {
        let node = self.get_mut(id)?;
        node.name = name.into();
        node.short_name = short_name;
        if let NodeKind::Pane { title_source, .. } = &mut node.kind {
            *title_source = PaneTitleSource::UserSpecified;
        }
        Ok(())
    }

    /// Proposes a title (and its short-form alternative) for an automatic
    /// title source. No-ops once the pane has been genuinely user-renamed
    /// (`title_source == UserSpecified`), and returns `false` without
    /// mutating the node when neither `title` nor `short_title` actually
    /// changed.
    pub fn set_automatic_pane_title(
        &mut self,
        id: NodeId,
        title: impl Into<String>,
        short_title: Option<String>,
    ) -> Result<bool, TreeError> {
        let node = self.get_mut(id)?;
        let NodeKind::Pane { title_source, .. } = &node.kind else {
            return Err(TreeError::NotAPane(id));
        };
        if title_source.is_user_specified() {
            return Ok(false);
        }
        let title = title.into();
        if node.name == title && node.short_name == short_title {
            return Ok(false);
        }
        node.name = title;
        node.short_name = short_title;
        Ok(true)
    }

    pub fn toggle_expanded(&mut self, id: NodeId) -> Result<(), TreeError> {
        match &mut self.get_mut(id)?.kind {
            NodeKind::Container(container) => {
                container.expanded = !container.expanded;
                Ok(())
            }
            NodeKind::Pane { .. } | NodeKind::Folder { .. } => Err(TreeError::NotAContainer(id)),
        }
    }

    pub fn set_pane_status(&mut self, id: NodeId, status: PaneStatus) -> Result<(), TreeError> {
        match &mut self.get_mut(id)?.kind {
            NodeKind::Pane { status: s, .. } => {
                *s = status;
                Ok(())
            }
            NodeKind::Container(_) | NodeKind::Folder { .. } => Err(TreeError::NotAPane(id)),
        }
    }

    /// Replaces the pending input for one terminal pane. Replacement is
    /// intentional: the context-menu action can be used again to move the
    /// deadline or change the payload without stacking surprising actions.
    pub fn schedule_pane_input(
        &mut self,
        id: NodeId,
        scheduled_input: ScheduledPaneInput,
    ) -> Result<(), TreeError> {
        if !scheduled_input.has_input() {
            return Err(TreeError::EmptyScheduledInput);
        }
        let node = self.get_mut(id)?;
        let NodeKind::Pane {
            content,
            scheduled_input: pending_input,
            ..
        } = &mut node.kind
        else {
            return Err(TreeError::NotAPane(id));
        };
        if *content != PaneContentKind::Terminal {
            return Err(TreeError::NotATerminal(id));
        }
        *pending_input = Some(scheduled_input);
        Ok(())
    }

    /// Clears only the schedule the caller actually observed. This compare-
    /// and-clear contract prevents an executor finishing an old action from
    /// deleting a replacement submitted concurrently for the same pane.
    pub fn clear_scheduled_pane_input_if_matches(
        &mut self,
        id: NodeId,
        expected: &ScheduledPaneInput,
    ) -> Result<bool, TreeError> {
        let node = self.get_mut(id)?;
        let NodeKind::Pane {
            scheduled_input, ..
        } = &mut node.kind
        else {
            return Err(TreeError::NotAPane(id));
        };
        if scheduled_input.as_ref() != Some(expected) {
            return Ok(false);
        }
        *scheduled_input = None;
        Ok(true)
    }

    /// Every pending action in no particular order. Scheduling policy belongs
    /// to `ilium-server`; the pure tree only exposes its durable domain state.
    pub fn scheduled_pane_inputs(
        &self,
    ) -> impl Iterator<Item = (NodeId, &ScheduledPaneInput)> {
        self.nodes.iter().filter_map(|(id, node)| {
            let NodeKind::Pane {
                scheduled_input: Some(scheduled_input),
                ..
            } = &node.kind
            else {
                return None;
            };
            Some((*id, scheduled_input))
        })
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

    /// Moves `id` to become a child of `new_parent`. `index` is read
    /// against `new_parent`'s children *as they exist at call time*
    /// (including `id` itself, if it is already one of them): `id` ends up
    /// immediately before whatever element currently sits at `index`, or
    /// appended at the end when `index` is `None` or `>=` the children
    /// count. This makes same-parent reordering (e.g. a tree-row drag
    /// dropped onto a sibling) and cross-parent moves use one consistent
    /// contract -- callers never need to special-case "moving within the
    /// same group".
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
        let moving_node = self.get(id).ok_or(TreeError::NodeNotFound(id))?;
        let destination = self
            .get(new_parent)
            .ok_or(TreeError::NodeNotFound(new_parent))?;
        let NodeKind::Container(destination_container) = &destination.kind else {
            return Err(TreeError::NotAContainer(new_parent));
        };
        if new_parent == ROOT_ID && moving_node.is_pane() {
            return Err(TreeError::PanesRequireGroup);
        }
        if moving_node.is_split_view() && !destination.is_group() {
            return Err(TreeError::SplitViewRequiresGroup);
        }
        destination_container
            .validate_child(moving_node, moving_node.parent == Some(new_parent))?;
        // A node can't be moved into itself or one of its own descendants.
        if self.is_ancestor_of(id, new_parent) {
            return Err(TreeError::CannotMoveIntoDescendant(id, new_parent));
        }
        let old_parent = self.get(id).ok_or(TreeError::NodeNotFound(id))?.parent;

        // `index` was computed by the caller against the children list that
        // still contains `id` (when reordering within the same parent).
        // Removing `id` first shifts every following sibling back by one,
        // so a requested index that was past `id`'s old position must shift
        // down by one too -- otherwise `id` lands one slot too far forward
        // (e.g. dropping a pane "onto" its immediate successor would
        // instead push it past that successor).
        let index = match (old_parent, index) {
            (Some(old_parent), Some(index)) if old_parent == new_parent => {
                let siblings = self.children_of(old_parent)?;
                match siblings.iter().position(|sibling| *sibling == id) {
                    Some(old_position) if old_position < index => Some(index - 1),
                    _ => Some(index),
                }
            }
            (_, index) => index,
        };

        if let Some(old_parent) = old_parent {
            self.remove_child(old_parent, id)?;
        }
        match &mut self.get_mut(new_parent)?.kind {
            NodeKind::Container(container) => {
                let index = index
                    .unwrap_or(container.children.len())
                    .min(container.children.len());
                container.children.insert(index, id);
            }
            NodeKind::Pane { .. } | NodeKind::Folder { .. } => unreachable!("checked above"),
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
                    Some(NodeKind::Container(container)) if container.is_group()
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
                Some(NodeKind::Container(container)) if container.is_group()
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
            NodeKind::Container(container) => {
                let Some(pos) = container.children.iter().position(|c| *c == id) else {
                    return Ok(());
                };
                let new_pos =
                    (pos as i32 + delta).clamp(0, container.children.len() as i32 - 1) as usize;
                container.children.remove(pos);
                container.children.insert(new_pos, id);
                Ok(())
            }
            NodeKind::Pane { .. } | NodeKind::Folder { .. } => {
                unreachable!("parent of a node is always a group")
            }
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
                    Some(NodeKind::Container(container)) if container.is_group()
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
    fn add_folder_persists_only_its_filesystem_root() {
        let mut tree = Tree::new();
        let group = tree.add_group(ROOT_ID, "workspace").unwrap();
        let folder = tree
            .add_folder(group, PathBuf::from("/tmp/project"))
            .unwrap();

        assert_eq!(tree.parent_of(folder), Some(group));
        assert_eq!(tree.get(folder).unwrap().name, "project");
        assert!(
            matches!(tree.get(folder).unwrap().kind, NodeKind::Folder { ref path } if path == &PathBuf::from("/tmp/project"))
        );
        assert_eq!(tree.children_of(group).unwrap(), &[folder]);
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
    fn move_node_within_same_parent_lands_immediately_before_target_index() {
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

        // Dragging `a` (index 0) onto `c` (index 2, in the list that still
        // contains `a`) must land `a` immediately before `c`: [b, a, c].
        // The naive pre-fix behavior inserted at the raw index 2 *after*
        // removing `a`, landing it at the end instead: [b, c, a].
        tree.move_node(a, group, Some(2)).unwrap();
        assert_eq!(tree.children_of(group).unwrap(), &[b, a, c]);

        // Dragging `c` backward onto `b` (index 0 of [b, a, c]) must land
        // `c` immediately before `b`: [c, b, a]. This direction never
        // shifts, so it is a regression guard against overcorrecting.
        tree.move_node(c, group, Some(0)).unwrap();
        assert_eq!(tree.children_of(group).unwrap(), &[c, b, a]);
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
    fn add_pane_under_a_pane_is_rejected_without_orphaning() {
        let mut tree = Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        let pane = tree
            .add_pane(group, "shell", PaneContentKind::Terminal)
            .unwrap();
        let ids_before = tree.all_ids().count();

        let err = tree
            .add_pane(pane, "nested", PaneContentKind::Terminal)
            .unwrap_err();
        assert!(matches!(err, TreeError::NotAContainer(id) if id == pane));
        // The rejected child must never have been inserted into the node
        // map -- a failed add must not leave an orphaned, unreachable node.
        assert_eq!(tree.all_ids().count(), ids_before);
    }

    #[test]
    fn add_board_persists_its_storage_descriptor() {
        let mut tree = Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        let storage = BoardStorage::MarkdownFile {
            path: PathBuf::from("/tmp/work.md"),
        };
        let board = tree
            .add_board(group, "Work".to_string(), storage.clone())
            .unwrap();
        assert!(matches!(
            tree.get(board).map(|node| &node.kind),
            Some(NodeKind::Pane {
                content: PaneContentKind::Board,
                status: PaneStatus::Board,
                board_storage: Some(saved_storage),
                ..
            }) if saved_storage == &storage
        ));
    }

    #[test]
    fn add_group_under_a_pane_is_rejected_without_orphaning() {
        let mut tree = Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        let pane = tree
            .add_pane(group, "shell", PaneContentKind::Terminal)
            .unwrap();
        let ids_before = tree.all_ids().count();

        let err = tree.add_group(pane, "nested").unwrap_err();
        assert!(matches!(err, TreeError::NotAGroup(id) if id == pane));
        assert_eq!(tree.all_ids().count(), ids_before);
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
        tree.rename_node(group, "renamed", None).unwrap();
        assert_eq!(tree.get(group).unwrap().name, "renamed");
        tree.toggle_expanded(group).unwrap();
        match &tree.get(group).unwrap().kind {
            NodeKind::Container(container) => assert!(!container.expanded),
            _ => panic!("expected group"),
        }
    }

    #[test]
    fn create_split_view_moves_selected_panes_in_order() {
        let mut tree = Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        let first = tree
            .add_pane(group, "first", PaneContentKind::Terminal)
            .unwrap();
        let second = tree
            .add_pane(group, "second", PaneContentKind::Editor)
            .unwrap();

        let split = tree
            .create_split_view(
                group,
                "Vertical split",
                SplitOrientation::Vertical,
                &[second, first],
            )
            .unwrap();

        assert_eq!(tree.children_of(split).unwrap(), &[second, first]);
        assert_eq!(tree.parent_of(first), Some(split));
        assert_eq!(tree.parent_of(second), Some(split));
        assert_eq!(
            tree.split_orientation(split),
            Some(SplitOrientation::Vertical)
        );
    }

    #[test]
    fn create_split_view_is_atomic_when_a_selected_pane_is_invalid() {
        let mut tree = Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        let pane = tree
            .add_pane(group, "first", PaneContentKind::Terminal)
            .unwrap();
        let before = tree.clone();

        let error = tree
            .create_split_view(
                group,
                "Vertical split",
                SplitOrientation::Vertical,
                &[pane, NodeId(999)],
            )
            .unwrap_err();

        assert!(matches!(error, TreeError::NodeNotFound(NodeId(999))));
        assert_eq!(tree, before);
    }

    #[test]
    fn split_view_rejects_a_fifth_pane_and_non_pane_children() {
        let mut tree = Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        let panes = (0..MAXIMUM_SPLIT_VIEW_PANES)
            .map(|index| {
                tree.add_pane(group, format!("pane {index}"), PaneContentKind::Terminal)
                    .unwrap()
            })
            .collect::<Vec<_>>();
        let split = tree
            .create_split_view(
                group,
                "Horizontal split",
                SplitOrientation::Horizontal,
                &panes,
            )
            .unwrap();
        let fifth = tree
            .add_pane(group, "fifth", PaneContentKind::Terminal)
            .unwrap();
        let nested_group = tree.add_group(group, "nested").unwrap();

        assert!(matches!(
            tree.move_node(fifth, split, None),
            Err(TreeError::SplitViewCapacityReached)
        ));
        assert!(matches!(
            tree.move_node(nested_group, split, None),
            Err(TreeError::SplitViewOnlyAcceptsPanes)
        ));
    }

    #[test]
    fn new_panes_can_be_created_directly_inside_a_split_until_capacity() {
        let mut tree = Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        let split = tree
            .create_split_view(group, "Vertical split", SplitOrientation::Vertical, &[])
            .unwrap();

        for index in 0..MAXIMUM_SPLIT_VIEW_PANES {
            tree.add_pane(split, format!("pane {index}"), PaneContentKind::Terminal)
                .unwrap();
        }

        assert!(matches!(
            tree.add_pane(split, "overflow", PaneContentKind::Editor),
            Err(TreeError::SplitViewCapacityReached)
        ));
    }

    #[test]
    fn create_split_view_rejects_panes_already_in_a_split() {
        let mut tree = Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        let pane = tree
            .add_pane(group, "pane", PaneContentKind::Terminal)
            .unwrap();
        tree.create_split_view(group, "First split", SplitOrientation::Vertical, &[pane])
            .unwrap();

        assert!(matches!(
            tree.create_split_view(
                group,
                "Second split",
                SplitOrientation::Horizontal,
                &[pane],
            ),
            Err(TreeError::PaneAlreadyInSplitView(id)) if id == pane
        ));
    }

    #[test]
    fn split_views_accept_every_supported_member_count() {
        for pane_count in 0..=MAXIMUM_SPLIT_VIEW_PANES {
            let mut tree = Tree::new();
            let group = tree.add_group(ROOT_ID, "work").unwrap();
            let panes = (0..pane_count)
                .map(|index| {
                    tree.add_pane(group, format!("pane {index}"), PaneContentKind::Terminal)
                        .unwrap()
                })
                .collect::<Vec<_>>();

            let split = tree
                .create_split_view(group, "split", SplitOrientation::Vertical, &panes)
                .unwrap();

            assert_eq!(tree.children_of(split).unwrap().len(), pane_count);
        }
    }

    #[test]
    fn removing_a_split_removes_its_member_panes_as_one_subtree() {
        let mut tree = Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        let first = tree
            .add_pane(group, "first", PaneContentKind::Terminal)
            .unwrap();
        let second = tree
            .add_pane(group, "second", PaneContentKind::Editor)
            .unwrap();
        let split = tree
            .create_split_view(
                group,
                "split",
                SplitOrientation::Horizontal,
                &[first, second],
            )
            .unwrap();

        tree.remove_node(split).unwrap();

        assert!(tree.get(split).is_none());
        assert!(tree.get(first).is_none());
        assert!(tree.get(second).is_none());
        assert!(tree.children_of(group).unwrap().is_empty());
    }

    #[test]
    fn scheduled_input_is_terminal_only_and_requires_a_real_payload() {
        let mut tree = Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        let terminal = tree
            .add_pane(group, "shell", PaneContentKind::Terminal)
            .unwrap();
        let editor = tree
            .add_pane(group, "notes", PaneContentKind::Editor)
            .unwrap();
        let empty = ScheduledPaneInput {
            execute_at_unix_millis: 1000,
            text: String::new(),
            send_enter: false,
        };

        assert!(matches!(
            tree.schedule_pane_input(terminal, empty),
            Err(TreeError::EmptyScheduledInput)
        ));
        assert!(matches!(
            tree.schedule_pane_input(
                editor,
                ScheduledPaneInput {
                    execute_at_unix_millis: 1000,
                    text: "ignored".to_string(),
                    send_enter: false,
                }
            ),
            Err(TreeError::NotATerminal(id)) if id == editor
        ));
    }

    #[test]
    fn replacing_and_compare_clearing_scheduled_input_is_race_safe() {
        let mut tree = Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        let terminal = tree
            .add_pane(group, "shell", PaneContentKind::Terminal)
            .unwrap();
        let original = ScheduledPaneInput {
            execute_at_unix_millis: 1000,
            text: "first".to_string(),
            send_enter: true,
        };
        let replacement = ScheduledPaneInput {
            execute_at_unix_millis: 2000,
            text: "second".to_string(),
            send_enter: false,
        };

        tree.schedule_pane_input(terminal, original.clone())
            .unwrap();
        tree.schedule_pane_input(terminal, replacement.clone())
            .unwrap();

        assert!(!tree
            .clear_scheduled_pane_input_if_matches(terminal, &original)
            .unwrap());
        assert_eq!(
            tree.scheduled_pane_inputs().collect::<Vec<_>>(),
            vec![(terminal, &replacement)]
        );
        assert!(tree
            .clear_scheduled_pane_input_if_matches(terminal, &replacement)
            .unwrap());
        assert_eq!(tree.scheduled_pane_inputs().count(), 0);
    }
}

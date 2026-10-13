//! Mutations an outside program may request, as one serializable enum.
//!
//! `ilium ctl` builds an [`Operation`] from command-line flags and the HTTP API
//! parses one from a JSON body; both then call [`Operation::requests`] to get
//! the exact IPC requests and [`Operation::outcome`] to decide, from the tree
//! before and after, whether the server applied it. Keeping translation and
//! confirmation here means the two surfaces cannot disagree about what
//! "rename" or "move" means, and both are unit-testable without a server.

use std::path::PathBuf;

use ilium_core::AgentProvider as _;
use ilium_core::{
    BuiltinAgentProvider, ContainerKind, NodeId, NodeKind, PromptQueueDelivery, RestructurePlan,
    Tree, TreeMoveDirection, ROOT_ID,
};
use ilium_ipc::{
    ClientRequest, NewPaneKind, NewPaneWorkingDirectory, PromptSubmissionSource,
    WorkspaceDisposition,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OperationError {
    #[error("node {0} does not exist")]
    NoSuchNode(u64),
    #[error("node {id} is a {actual}; this operation needs {wanted}")]
    WrongKind {
        id: u64,
        actual: &'static str,
        wanted: &'static str,
    },
    #[error("{0}")]
    Invalid(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorktreeAction {
    /// Close the pane and keep its worktree and branch.
    #[default]
    Keep,
    /// Also remove the pane's worktree if it is clean and merged.
    Remove,
    /// Also delete the creation branch when safe.
    RemoveAndBranch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepDirection {
    Up,
    Down,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentProvider {
    Claude,
    Codex,
    Antigravity,
}

impl AgentProvider {
    pub fn parse(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "claude" => Some(Self::Claude),
            "codex" => Some(Self::Codex),
            "antigravity" | "agy" => Some(Self::Antigravity),
            _ => None,
        }
    }

    pub const fn builtin(self) -> BuiltinAgentProvider {
        match self {
            Self::Claude => BuiltinAgentProvider::Claude,
            Self::Codex => BuiltinAgentProvider::Codex,
            Self::Antigravity => BuiltinAgentProvider::Antigravity,
        }
    }
}

/// What a new pane runs.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PaneLaunch {
    /// The user's default shell.
    #[default]
    Shell,
    /// One shell command line (`$SHELL -c`). Closes itself when the command
    /// ends unless the operation sets `keep_open`.
    Command { command: String },
    /// The built-in editor on a file.
    Editor { path: PathBuf },
    /// A built-in agent CLI, optionally given a first prompt.
    Agent {
        provider: AgentProvider,
        #[serde(default)]
        prompt: Option<String>,
    },
}

/// One queued prompt's delivery policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum QueueDelivery {
    #[default]
    Once,
    Times {
        runs: u32,
    },
    Forever,
}

const fn default_true() -> bool {
    true
}

/// A requested change. Node ids are the numbers `ilium ctl tree` prints.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    /// Set a group, project or pane title. A rename fixes the title against
    /// later automatic renaming.
    Rename { node: u64, title: String },
    /// Close a pane and end its process.
    Close {
        pane: u64,
        #[serde(default)]
        worktree: WorktreeAction,
    },
    /// Swap a node with its previous or next sibling.
    MoveStep { node: u64, direction: StepDirection },
    /// Move a node under another container at `index` (default: the end).
    Reparent {
        node: u64,
        parent: u64,
        #[serde(default)]
        index: Option<usize>,
    },
    /// Add a directory-backed top-level project.
    NewProject { path: PathBuf },
    /// Add a group under a project or group.
    NewGroup { parent: u64, name: String },
    /// Add a file-browser folder root under a container.
    NewFolder { parent: u64, path: PathBuf },
    /// Change the directory new entries of a project start in.
    ChangeProjectFolder { project: u64, path: PathBuf },
    /// Start a pane. `parent` is a project or group id; omitted means the
    /// session's launch project.
    NewPane {
        #[serde(default)]
        parent: Option<u64>,
        #[serde(default)]
        launch: PaneLaunch,
        #[serde(default)]
        cwd: Option<PathBuf>,
        #[serde(default)]
        keep_open: bool,
    },
    /// Type text into a pane and press Enter (or just paste it).
    SendText {
        pane: u64,
        text: String,
        #[serde(default = "default_true")]
        enter: bool,
    },
    /// Write named keys (`enter`, `esc`, `ctrl-c`, `up`, `text:abc`) to a pane.
    SendKeys { pane: u64, keys: Vec<String> },
    /// Type text into a pane after a delay.
    Schedule {
        pane: u64,
        delay_seconds: u64,
        text: String,
        #[serde(default = "default_true")]
        enter: bool,
    },
    /// Queue a prompt to send when the pane's agent next finishes.
    Enqueue {
        pane: u64,
        text: String,
        #[serde(default)]
        delivery: QueueDelivery,
    },
    /// Drop every queued prompt of a pane.
    ClearQueue { pane: u64 },
    /// Set or clear a node's sidebar bookmark.
    Bookmark { node: u64, on: bool },
    /// Expand or collapse a container or folder in the sidebar.
    Expand { node: u64, expanded: bool },
    /// Lock a container or folder closed (or unlock it).
    LockClosed { node: u64, locked: bool },
    /// Stop the process in a pane without closing the pane.
    Terminate { pane: u64 },
    /// Replace the tree (or one project's contents) with a restructure plan.
    /// The plan must mention every existing pane and folder exactly once.
    ApplyRestructure {
        plan: RestructurePlan,
        #[serde(default)]
        project: Option<u64>,
    },
    /// Undo the most recent session-wide restructure.
    RevertRestructure,
    /// Undo the latest restructure of one project.
    RevertProjectRestructure { project: u64 },
    /// End the session: every pane process stops.
    KillSession,
}

/// Names for the keys `SendKeys` accepts, for error messages and the guide.
pub const NAMED_KEYS: &[&str] = &[
    "enter",
    "esc",
    "tab",
    "backspace",
    "space",
    "up",
    "down",
    "right",
    "left",
    "home",
    "end",
    "pageup",
    "pagedown",
    "delete",
    "ctrl-<letter>",
    "text:<literal>",
];

/// Bytes a terminal expects for one named key.
pub fn encode_key(name: &str) -> Result<Vec<u8>, OperationError> {
    if let Some(literal) = name.strip_prefix("text:") {
        return Ok(literal.as_bytes().to_vec());
    }
    let lower = name.to_ascii_lowercase();
    if let Some(letter) = lower.strip_prefix("ctrl-") {
        let mut characters = letter.chars();
        if let (Some(character), None) = (characters.next(), characters.next()) {
            if character.is_ascii_lowercase() {
                return Ok(vec![character as u8 - b'a' + 1]);
            }
        }
        return Err(OperationError::Invalid(format!(
            "{name:?} is not a control key; use ctrl-a through ctrl-z"
        )));
    }
    let bytes: &[u8] = match lower.as_str() {
        "enter" | "return" => b"\r",
        "esc" | "escape" => b"\x1b",
        "tab" => b"\t",
        "backspace" => b"\x7f",
        "space" => b" ",
        "up" => b"\x1b[A",
        "down" => b"\x1b[B",
        "right" => b"\x1b[C",
        "left" => b"\x1b[D",
        "home" => b"\x1b[H",
        "end" => b"\x1b[F",
        "pageup" => b"\x1b[5~",
        "pagedown" => b"\x1b[6~",
        "delete" => b"\x1b[3~",
        _ => {
            return Err(OperationError::Invalid(format!(
                "unknown key {name:?}; accepted: {}",
                NAMED_KEYS.join(", ")
            )))
        }
    };
    Ok(bytes.to_vec())
}

fn kind_name(tree: &Tree, id: NodeId) -> Result<&'static str, OperationError> {
    let node = tree.get(id).ok_or(OperationError::NoSuchNode(id.0))?;
    Ok(crate::view::node_kind_name(node))
}

fn require_pane(tree: &Tree, id: u64) -> Result<NodeId, OperationError> {
    let id = NodeId(id);
    match kind_name(tree, id)? {
        "pane" => Ok(id),
        actual => Err(OperationError::WrongKind {
            id: id.0,
            actual,
            wanted: "a pane",
        }),
    }
}

fn require_container(tree: &Tree, id: u64) -> Result<NodeId, OperationError> {
    let id = NodeId(id);
    match kind_name(tree, id)? {
        "project" | "group" => Ok(id),
        actual => Err(OperationError::WrongKind {
            id: id.0,
            actual,
            wanted: "a project or group",
        }),
    }
}

fn require_project(tree: &Tree, id: u64) -> Result<NodeId, OperationError> {
    let id = NodeId(id);
    match kind_name(tree, id)? {
        "project" => Ok(id),
        actual => Err(OperationError::WrongKind {
            id: id.0,
            actual,
            wanted: "a project",
        }),
    }
}

fn require_existing(tree: &Tree, id: u64) -> Result<NodeId, OperationError> {
    let id = NodeId(id);
    kind_name(tree, id)?;
    if id == ROOT_ID {
        return Err(OperationError::Invalid(
            "the session root cannot be changed".to_owned(),
        ));
    }
    Ok(id)
}

fn require_absolute(path: &std::path::Path) -> Result<(), OperationError> {
    if path.is_absolute() {
        return Ok(());
    }
    Err(OperationError::Invalid(format!(
        "path {} must be absolute",
        path.display()
    )))
}

impl Operation {
    /// Stable lowercase operation name, the `op` value.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Rename { .. } => "rename",
            Self::Close { .. } => "close",
            Self::MoveStep { .. } => "move_step",
            Self::Reparent { .. } => "reparent",
            Self::NewProject { .. } => "new_project",
            Self::NewGroup { .. } => "new_group",
            Self::NewFolder { .. } => "new_folder",
            Self::ChangeProjectFolder { .. } => "change_project_folder",
            Self::NewPane { .. } => "new_pane",
            Self::SendText { .. } => "send_text",
            Self::SendKeys { .. } => "send_keys",
            Self::Schedule { .. } => "schedule",
            Self::Enqueue { .. } => "enqueue",
            Self::ClearQueue { .. } => "clear_queue",
            Self::Bookmark { .. } => "bookmark",
            Self::Expand { .. } => "expand",
            Self::LockClosed { .. } => "lock_closed",
            Self::Terminate { .. } => "terminate",
            Self::ApplyRestructure { .. } => "apply_restructure",
            Self::RevertRestructure => "revert_restructure",
            Self::RevertProjectRestructure { .. } => "revert_project_restructure",
            Self::KillSession => "kill_session",
        }
    }

    /// The IPC requests that carry this operation out, validated against the
    /// current tree. `workspace_request_id` identifies a worktree close.
    pub fn requests(
        &self,
        tree: &Tree,
        workspace_request_id: u64,
    ) -> Result<Vec<ClientRequest>, OperationError> {
        let request = match self {
            Self::Rename { node, title } => {
                let node_id = require_existing(tree, *node)?;
                if title.trim().is_empty() {
                    return Err(OperationError::Invalid("title must not be empty".into()));
                }
                ClientRequest::RenameNode {
                    node_id,
                    title: title.clone(),
                    short_title: None,
                    inferred_icon: None,
                }
            }
            Self::Close { pane, worktree } => {
                let pane_id = require_pane(tree, *pane)?;
                match worktree {
                    WorktreeAction::Keep => ClientRequest::ClosePane { pane_id },
                    other => ClientRequest::ClosePaneWithWorkspaceDisposition {
                        request_id: workspace_request_id,
                        pane_id,
                        disposition: match other {
                            WorktreeAction::Remove => WorkspaceDisposition::RemoveWorktree,
                            _ => WorkspaceDisposition::RemoveWorktreeAndBranch,
                        },
                    },
                }
            }
            Self::MoveStep { node, direction } => ClientRequest::MoveNode {
                node_id: require_existing(tree, *node)?,
                direction: match direction {
                    StepDirection::Up => TreeMoveDirection::Up,
                    StepDirection::Down => TreeMoveDirection::Down,
                },
            },
            Self::Reparent {
                node,
                parent,
                index,
            } => {
                let node_id = require_existing(tree, *node)?;
                let new_parent = require_container(tree, *parent)?;
                ClientRequest::ReparentNode {
                    node_id,
                    new_parent,
                    index: *index,
                }
            }
            Self::NewProject { path } => {
                require_absolute(path)?;
                ClientRequest::NewProject { path: path.clone() }
            }
            Self::NewGroup { parent, name } => {
                if name.trim().is_empty() {
                    return Err(OperationError::Invalid("name must not be empty".into()));
                }
                ClientRequest::NewGroup {
                    parent_group: require_container(tree, *parent)?,
                    name: name.clone(),
                }
            }
            Self::NewFolder { parent, path } => {
                require_absolute(path)?;
                ClientRequest::NewFolder {
                    parent_group: require_container(tree, *parent)?,
                    path: path.clone(),
                }
            }
            Self::ChangeProjectFolder { project, path } => {
                require_absolute(path)?;
                ClientRequest::ChangeProjectFolder {
                    project_id: require_project(tree, *project)?,
                    path: path.clone(),
                }
            }
            Self::NewPane {
                parent,
                launch,
                cwd,
                keep_open,
            } => {
                let parent_group = match parent {
                    Some(parent) => require_container(tree, *parent)?,
                    None => ROOT_ID,
                };
                if let Some(cwd) = cwd {
                    require_absolute(cwd)?;
                }
                let kind = match launch {
                    PaneLaunch::Shell => NewPaneKind::PlainShell,
                    PaneLaunch::Command { command } if *keep_open => {
                        NewPaneKind::Command(command.clone())
                    }
                    PaneLaunch::Command { command } => {
                        NewPaneKind::CommandClosingOnExit(command.clone())
                    }
                    PaneLaunch::Editor { path } => {
                        require_absolute(path)?;
                        NewPaneKind::Editor(path.clone())
                    }
                    PaneLaunch::Agent { provider, prompt } => {
                        let command_line = provider.builtin().command_line().to_owned();
                        match prompt.as_deref().filter(|text| !text.trim().is_empty()) {
                            Some(initial_input) => NewPaneKind::CommandWithInitialInput {
                                command_line,
                                initial_input: initial_input.to_owned(),
                            },
                            None => NewPaneKind::Command(command_line),
                        }
                    }
                };
                ClientRequest::NewPane {
                    parent_group,
                    kind,
                    working_directory: cwd
                        .clone()
                        .map_or(NewPaneWorkingDirectory::ProjectRoot, |cwd| {
                            NewPaneWorkingDirectory::Directory(cwd)
                        }),
                }
            }
            Self::SendText { pane, text, enter } => {
                let pane_id = require_pane(tree, *pane)?;
                if *enter {
                    ClientRequest::SubmitTerminalText {
                        pane_id,
                        text: text.clone(),
                        source: PromptSubmissionSource::Keyboard,
                    }
                } else {
                    ClientRequest::PasteTerminalText {
                        pane_id,
                        text: text.clone(),
                    }
                }
            }
            Self::SendKeys { pane, keys } => {
                let pane_id = require_pane(tree, *pane)?;
                if keys.is_empty() {
                    return Err(OperationError::Invalid("keys must not be empty".into()));
                }
                let mut bytes = Vec::new();
                for key in keys {
                    bytes.extend(encode_key(key)?);
                }
                ClientRequest::UserKeyInput {
                    pane_id,
                    bytes,
                    submission: None,
                    prompt_epoch: None,
                }
            }
            Self::Schedule {
                pane,
                delay_seconds,
                text,
                enter,
            } => ClientRequest::SchedulePaneInput {
                pane_id: require_pane(tree, *pane)?,
                delay_seconds: *delay_seconds,
                text: text.clone(),
                send_enter: *enter,
            },
            Self::Enqueue {
                pane,
                text,
                delivery,
            } => {
                let delivery = match delivery {
                    QueueDelivery::Once => PromptQueueDelivery::Once,
                    QueueDelivery::Times { runs } => PromptQueueDelivery::Times {
                        remaining_runs: *runs,
                    },
                    QueueDelivery::Forever => PromptQueueDelivery::Forever,
                };
                if !delivery.is_valid() {
                    return Err(OperationError::Invalid("runs must be at least 1".into()));
                }
                ClientRequest::EnqueuePrompt {
                    pane_id: require_pane(tree, *pane)?,
                    text: text.clone(),
                    delivery,
                }
            }
            Self::ClearQueue { pane } => ClientRequest::ClearPromptQueue {
                pane_id: require_pane(tree, *pane)?,
            },
            Self::Bookmark { node, on } => ClientRequest::SetNodeBookmarked {
                node_id: require_existing(tree, *node)?,
                is_bookmarked: *on,
            },
            Self::Expand { node, expanded } => ClientRequest::SetNodeExpanded {
                node_id: require_existing(tree, *node)?,
                expanded: *expanded,
            },
            Self::LockClosed { node, locked } => ClientRequest::SetNodeLockedClosed {
                node_id: require_existing(tree, *node)?,
                locked_closed: *locked,
            },
            Self::Terminate { pane } => ClientRequest::TerminatePaneProcess {
                pane_id: require_pane(tree, *pane)?,
            },
            Self::ApplyRestructure { plan, project } => match project {
                None => ClientRequest::ApplyRestructurePlan {
                    plan: plan.clone(),
                    title_observations: Vec::new(),
                },
                Some(project) => ClientRequest::ApplyProjectRestructurePlan {
                    project_id: require_project(tree, *project)?,
                    plan: plan.clone(),
                    inference_activity_revisions: Vec::new(),
                    title_observations: Vec::new(),
                },
            },
            Self::RevertRestructure => ClientRequest::RevertLastRestructure,
            Self::RevertProjectRestructure { project } => ClientRequest::RevertProjectRestructure {
                project_id: require_project(tree, *project)?,
            },
            Self::KillSession => ClientRequest::KillSession,
        };
        Ok(vec![request])
    }

    /// True when the server shows no tree change for this operation, so the
    /// caller confirms by absence of an error instead of by comparing trees.
    pub const fn is_input_only(&self) -> bool {
        matches!(
            self,
            Self::SendText { .. }
                | Self::SendKeys { .. }
                | Self::Schedule { .. }
                | Self::Enqueue { .. }
                | Self::ClearQueue { .. }
                | Self::Terminate { .. }
                | Self::KillSession
        )
    }

    /// Decides from the tree before and after whether the operation took
    /// effect. `Some(details)` is the confirmed result; `None` means not yet.
    pub fn outcome(&self, before: &Tree, after: &Tree) -> Option<Value> {
        let created = |kind: Option<&str>| -> Option<Value> {
            let known: std::collections::HashSet<NodeId> = before.all_ids().collect();
            let mut added: Vec<NodeId> = after.all_ids().filter(|id| !known.contains(id)).collect();
            added.sort();
            let wanted = added.into_iter().find(|id| {
                after.get(*id).is_some_and(|node| {
                    kind.is_none_or(|kind| crate::view::node_kind_name(node) == kind)
                })
            })?;
            Some(json!({"created": wanted.0}))
        };
        match self {
            Self::Rename { node, title } => (after.get(NodeId(*node))?.name == *title)
                .then(|| json!({"node": node, "title": title})),
            Self::Close { pane, .. } => after
                .get(NodeId(*pane))
                .is_none()
                .then(|| json!({"closed": pane})),
            Self::MoveStep { node, .. } => {
                let position = |tree: &Tree| {
                    let parent = tree.parent_of(NodeId(*node))?;
                    let siblings = tree.children_of(parent).ok()?;
                    Some((parent, siblings.iter().position(|id| id.0 == *node)?))
                };
                let (previous, current) = (position(before), position(after));
                (current.is_some() && current != previous)
                    .then(|| json!({"node": node, "parent": current.map(|c| c.0 .0), "index": current.map(|c| c.1)}))
            }
            Self::Reparent {
                node,
                parent,
                index,
            } => {
                let current_parent = after.parent_of(NodeId(*node))?;
                let siblings = after.children_of(current_parent).ok()?;
                let position = siblings.iter().position(|id| id.0 == *node)?;
                let wants_index =
                    index.is_none_or(|index| index.min(siblings.len() - 1) == position);
                (current_parent.0 == *parent && wants_index)
                    .then(|| json!({"node": node, "parent": parent, "index": position}))
            }
            Self::NewProject { .. } => created(Some("project")),
            Self::NewGroup { .. } => created(Some("group")),
            Self::NewFolder { .. } => created(Some("folder")),
            Self::NewPane { .. } => created(Some("pane")),
            Self::ChangeProjectFolder { project, path } => {
                let changed = match &after.get(NodeId(*project))?.kind {
                    NodeKind::Container(container) => matches!(
                        &container.kind,
                        ContainerKind::Project { path: current } if current == path
                    ),
                    _ => false,
                };
                changed.then(|| json!({"project": project, "path": path}))
            }
            Self::Bookmark { node, on } => (after.get(NodeId(*node))?.is_bookmarked == *on)
                .then(|| json!({"node": node, "bookmarked": on})),
            Self::Expand { node, expanded } => {
                let current = match &after.get(NodeId(*node))?.kind {
                    NodeKind::Container(container) => container.expanded,
                    NodeKind::Folder { expanded, .. } => *expanded,
                    NodeKind::Pane { .. } => return None,
                };
                (current == *expanded).then(|| json!({"node": node, "expanded": expanded}))
            }
            Self::LockClosed { node, locked } => {
                let current = match &after.get(NodeId(*node))?.kind {
                    NodeKind::Container(container) => container.locked_closed,
                    NodeKind::Folder { locked_closed, .. } => *locked_closed,
                    NodeKind::Pane { .. } => return None,
                };
                (current == *locked).then(|| json!({"node": node, "locked_closed": locked}))
            }
            Self::ApplyRestructure { .. } => (before != after).then(|| json!({"applied": true})),
            Self::RevertRestructure | Self::RevertProjectRestructure { .. } => {
                (before != after).then(|| json!({"reverted": true}))
            }
            Self::SendText { .. }
            | Self::SendKeys { .. }
            | Self::Schedule { .. }
            | Self::Enqueue { .. }
            | Self::ClearQueue { .. }
            | Self::Terminate { .. }
            | Self::KillSession => None,
        }
    }
}

/// One example of every operation, for `ilium ctl ops` and the HTTP guide.
/// `RestructurePlan` is shown with an empty plan; real plans come from files.
pub fn examples() -> Vec<Operation> {
    vec![
        Operation::Rename {
            node: 12,
            title: "Login fix".into(),
        },
        Operation::Close {
            pane: 12,
            worktree: WorktreeAction::Keep,
        },
        Operation::MoveStep {
            node: 12,
            direction: StepDirection::Up,
        },
        Operation::Reparent {
            node: 12,
            parent: 5,
            index: Some(0),
        },
        Operation::NewProject {
            path: PathBuf::from("/abs/path/to/repo"),
        },
        Operation::NewGroup {
            parent: 5,
            name: "Experiments".into(),
        },
        Operation::NewFolder {
            parent: 5,
            path: PathBuf::from("/abs/path/to/folder"),
        },
        Operation::ChangeProjectFolder {
            project: 5,
            path: PathBuf::from("/abs/path"),
        },
        Operation::NewPane {
            parent: Some(5),
            launch: PaneLaunch::Agent {
                provider: AgentProvider::Claude,
                prompt: Some("Fix the failing test".into()),
            },
            cwd: Some(PathBuf::from("/abs/path/subdir")),
            keep_open: false,
        },
        Operation::SendText {
            pane: 12,
            text: "run the tests".into(),
            enter: true,
        },
        Operation::SendKeys {
            pane: 12,
            keys: vec!["ctrl-c".into(), "enter".into()],
        },
        Operation::Schedule {
            pane: 12,
            delay_seconds: 600,
            text: "status?".into(),
            enter: true,
        },
        Operation::Enqueue {
            pane: 12,
            text: "next step".into(),
            delivery: QueueDelivery::Once,
        },
        Operation::ClearQueue { pane: 12 },
        Operation::Bookmark { node: 12, on: true },
        Operation::Expand {
            node: 5,
            expanded: false,
        },
        Operation::LockClosed {
            node: 5,
            locked: true,
        },
        Operation::Terminate { pane: 12 },
        Operation::ApplyRestructure {
            plan: RestructurePlan {
                children: Vec::new(),
            },
            project: Some(5),
        },
        Operation::RevertRestructure,
        Operation::RevertProjectRestructure { project: 5 },
        Operation::KillSession,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_core::PaneContentKind;

    fn sample() -> (Tree, NodeId, NodeId, NodeId) {
        let mut tree = Tree::new();
        let project = tree.add_project(PathBuf::from("/tmp/alpha")).unwrap();
        let group = tree.add_group(project, "work").unwrap();
        let pane = tree
            .add_pane(group, "shell".to_owned(), PaneContentKind::Terminal)
            .unwrap();
        (tree, project, group, pane)
    }

    #[test]
    fn every_example_round_trips_and_names_are_unique() {
        let mut names = std::collections::HashSet::new();
        for example in examples() {
            let text = serde_json::to_string(&example).unwrap();
            assert_eq!(serde_json::from_str::<Operation>(&text).unwrap(), example);
            assert!(names.insert(example.name()), "{}", example.name());
        }
    }

    #[test]
    fn json_round_trip_uses_the_op_tag() {
        let operation: Operation =
            serde_json::from_str(r#"{"op":"rename","node":4,"title":"x"}"#).unwrap();
        assert_eq!(
            operation,
            Operation::Rename {
                node: 4,
                title: "x".into()
            }
        );
        let text = serde_json::to_string(&operation).unwrap();
        assert!(text.contains(r#""op":"rename""#));
        assert!(serde_json::from_str::<Operation>(
            r#"{"op":"rename","node":4,"title":"x","extra":1}"#
        )
        .is_err());
    }

    #[test]
    fn defaults_fill_optional_fields() {
        let operation: Operation =
            serde_json::from_str(r#"{"op":"send_text","pane":3,"text":"hi"}"#).unwrap();
        assert!(matches!(operation, Operation::SendText { enter: true, .. }));
        let operation: Operation = serde_json::from_str(r#"{"op":"new_pane"}"#).unwrap();
        assert!(matches!(
            operation,
            Operation::NewPane {
                parent: None,
                launch: PaneLaunch::Shell,
                cwd: None,
                keep_open: false
            }
        ));
    }

    #[test]
    fn rename_validates_the_node_and_title() {
        let (tree, _, _, pane) = sample();
        let ok = Operation::Rename {
            node: pane.0,
            title: "new".into(),
        };
        assert!(matches!(
            ok.requests(&tree, 1).unwrap()[0],
            ClientRequest::RenameNode { .. }
        ));
        let missing = Operation::Rename {
            node: 999,
            title: "new".into(),
        };
        assert_eq!(
            missing.requests(&tree, 1),
            Err(OperationError::NoSuchNode(999))
        );
        let empty = Operation::Rename {
            node: pane.0,
            title: " ".into(),
        };
        assert!(matches!(
            empty.requests(&tree, 1),
            Err(OperationError::Invalid(_))
        ));
        let root = Operation::Rename {
            node: 0,
            title: "x".into(),
        };
        assert!(root.requests(&tree, 1).is_err());
    }

    #[test]
    fn pane_operations_reject_groups() {
        let (tree, _, group, _) = sample();
        let close = Operation::Close {
            pane: group.0,
            worktree: WorktreeAction::Keep,
        };
        assert!(matches!(
            close.requests(&tree, 1),
            Err(OperationError::WrongKind { .. })
        ));
    }

    #[test]
    fn close_with_worktree_action_uses_the_disposition_request() {
        let (tree, _, _, pane) = sample();
        let request = Operation::Close {
            pane: pane.0,
            worktree: WorktreeAction::RemoveAndBranch,
        }
        .requests(&tree, 7)
        .unwrap()
        .remove(0);
        assert!(matches!(
            request,
            ClientRequest::ClosePaneWithWorkspaceDisposition {
                request_id: 7,
                disposition: WorkspaceDisposition::RemoveWorktreeAndBranch,
                ..
            }
        ));
    }

    #[test]
    fn new_pane_maps_launch_and_directory() {
        let (tree, project, ..) = sample();
        let operation = Operation::NewPane {
            parent: Some(project.0),
            launch: PaneLaunch::Agent {
                provider: AgentProvider::Codex,
                prompt: Some("go".into()),
            },
            cwd: Some(PathBuf::from("/tmp/alpha/sub")),
            keep_open: false,
        };
        let request = operation.requests(&tree, 1).unwrap().remove(0);
        let ClientRequest::NewPane {
            parent_group,
            kind,
            working_directory,
        } = request
        else {
            panic!("expected NewPane");
        };
        assert_eq!(parent_group, project);
        assert!(
            matches!(kind, NewPaneKind::CommandWithInitialInput { ref command_line, .. } if command_line == "codex")
        );
        assert_eq!(
            working_directory,
            NewPaneWorkingDirectory::Directory(PathBuf::from("/tmp/alpha/sub"))
        );
        let relative = Operation::NewPane {
            parent: None,
            launch: PaneLaunch::Shell,
            cwd: Some(PathBuf::from("relative")),
            keep_open: false,
        };
        assert!(relative.requests(&tree, 1).is_err());
    }

    #[test]
    fn command_panes_close_on_exit_unless_kept_open() {
        let (tree, ..) = sample();
        let make = |keep_open| Operation::NewPane {
            parent: None,
            launch: PaneLaunch::Command {
                command: "ls".into(),
            },
            cwd: None,
            keep_open,
        };
        let closing = make(false).requests(&tree, 1).unwrap().remove(0);
        let kept = make(true).requests(&tree, 1).unwrap().remove(0);
        assert!(matches!(
            closing,
            ClientRequest::NewPane {
                kind: NewPaneKind::CommandClosingOnExit(_),
                ..
            }
        ));
        assert!(matches!(
            kept,
            ClientRequest::NewPane {
                kind: NewPaneKind::Command(_),
                ..
            }
        ));
    }

    #[test]
    fn key_names_encode_to_terminal_bytes() {
        assert_eq!(encode_key("enter").unwrap(), b"\r");
        assert_eq!(encode_key("ctrl-c").unwrap(), vec![3]);
        assert_eq!(encode_key("Up").unwrap(), b"\x1b[A");
        assert_eq!(encode_key("text:hi there").unwrap(), b"hi there");
        assert!(encode_key("ctrl-1").is_err());
        assert!(encode_key("nonsense").is_err());
    }

    #[test]
    fn outcome_detects_rename_close_and_creation() {
        let (before, project, group, pane) = sample();
        let mut after = before.clone();
        after
            .rename_node(pane, "renamed".to_owned(), None, None)
            .ok();
        let rename = Operation::Rename {
            node: pane.0,
            title: "renamed".into(),
        };
        if after.get(pane).unwrap().name == "renamed" {
            assert!(rename.outcome(&before, &after).is_some());
        }
        assert!(rename.outcome(&before, &before).is_none());

        let mut closed = before.clone();
        closed.remove_node(pane).unwrap();
        let close = Operation::Close {
            pane: pane.0,
            worktree: WorktreeAction::Keep,
        };
        assert!(close.outcome(&before, &closed).is_some());
        assert!(close.outcome(&before, &before).is_none());

        let mut grown = before.clone();
        let new_pane = grown
            .add_pane(group, "n".to_owned(), PaneContentKind::Terminal)
            .unwrap();
        let operation = Operation::NewPane {
            parent: Some(project.0),
            launch: PaneLaunch::Shell,
            cwd: None,
            keep_open: false,
        };
        assert_eq!(
            operation.outcome(&before, &grown),
            Some(json!({"created": new_pane.0}))
        );
    }

    #[test]
    fn move_step_outcome_needs_a_position_change() {
        let (mut before, _, group, first) = sample();
        let second = before
            .add_pane(group, "two".to_owned(), PaneContentKind::Terminal)
            .unwrap();
        let mut after = before.clone();
        after
            .move_node_one_step(second, TreeMoveDirection::Up)
            .unwrap();
        let step = Operation::MoveStep {
            node: second.0,
            direction: StepDirection::Up,
        };
        assert!(step.outcome(&before, &after).is_some());
        assert!(step.outcome(&before, &before).is_none());
        let _ = first;
    }
}

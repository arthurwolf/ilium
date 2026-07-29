//! Pure domain model for ilium: a tree of containers, panes, and folder
//! references, no I/O. Containers are normal groups or bounded split views.
//!
//! This crate has zero dependency on tokio, portable-pty, ratatui, or any
//! other adapter. Everything here must stay unit-testable with plain
//! `#[test]` functions.

use std::collections::{HashMap, HashSet};
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
    /// Google's Antigravity CLI. Its executable is `agy`; `antimatter` is
    /// also accepted as a process-name alias for the terminology used in the
    /// original feature request.
    Antigravity,
    Other(String),
}

impl AgentClass {
    /// Returns the built-in provider represented by this class. Custom
    /// signatures deliberately remain `Other`: they have no verified launch,
    /// resume, or local-transcript contract until a provider is added.
    pub const fn provider(&self) -> Option<BuiltinAgentProvider> {
        match self {
            Self::Claude => Some(BuiltinAgentProvider::Claude),
            Self::Codex => Some(BuiltinAgentProvider::Codex),
            Self::Antigravity => Some(BuiltinAgentProvider::Antigravity),
            Self::Other(_) => None,
        }
    }

    /// Stable user-facing label shared by presentation and naming layers.
    pub fn label(&self) -> &str {
        match self {
            Self::Claude => AgentProvider::label(BuiltinAgentProvider::Claude),
            Self::Codex => AgentProvider::label(BuiltinAgentProvider::Codex),
            Self::Antigravity => AgentProvider::label(BuiltinAgentProvider::Antigravity),
            Self::Other(name) => name,
        }
    }

    /// Stable type ordering for tree presentation. Unknown providers stay
    /// after the built-ins, while terminal/editor/board ordering remains a
    /// client concern.
    pub fn type_sort_rank(&self) -> u8 {
        self.provider()
            .map(BuiltinAgentProvider::type_sort_rank)
            .unwrap_or(6)
    }
}

/// The complete built-in provider registry. New built-in agent support is one
/// additional variant plus this trait implementation; all cross-layer
/// behavior (launch, identity, resume syntax, labels, and ordering) then
/// flows from the same definition rather than from parallel Claude/Codex
/// branches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum BuiltinAgentProvider {
    Claude,
    Codex,
    Antigravity,
}

/// Provider-owned reason that a submitted command invalidates the current
/// externally persisted session identity. This is returned alongside the
/// decision so diagnostics cannot drift from the rule that changed state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionIdentityTransitionRule {
    SharedNewSessionCommand,
    ClaudeOrCodexClearStartsFreshSession,
}

impl SessionIdentityTransitionRule {
    /// Plain-language evidence suitable for the persisted agent event log.
    pub const fn explanation(self) -> &'static str {
        match self {
            Self::SharedNewSessionCommand => {
                "the first command word was /resume, /branch, or /new, which replaces the persisted session for every built-in provider"
            }
            Self::ClaudeOrCodexClearStartsFreshSession => {
                "the verified provider was Claude or Codex, whose exact /clear command starts a fresh ilium session identity"
            }
        }
    }
}

/// Provider-specific behavior that is pure and safe to share across every
/// layer. Filesystem transcript formats remain in `ilium-agent-session`, and
/// process polling remains in `ilium-server`; this trait owns only the stable
/// contract each layer needs to identify or invoke an installed CLI.
pub trait AgentProvider {
    /// The wire/domain class emitted after this provider is identified.
    fn class(self) -> AgentClass;
    /// Human-readable, stable product name.
    fn label(self) -> &'static str;
    /// Literal command used for a fresh interactive pane.
    fn command_line(self) -> &'static str;
    /// Lowercase executable-name substrings that identify this provider.
    fn process_name_substrings(self) -> &'static [&'static str];
    /// Builds the provider's exact resume command around a shell-quoted id.
    fn resume_command(self, quoted_session_id: &str) -> String;
    /// Parses one current process command line into a provider session id.
    fn session_id_from_arguments(self, arguments: &[String]) -> Option<String>;
    /// Returns why one submitted interactive command replaces the provider's
    /// externally persisted session inside the current process.
    fn session_identity_transition_rule(
        self,
        submitted_line: &str,
    ) -> Option<SessionIdentityTransitionRule>;

    /// Convenience predicate derived from the diagnostic-bearing rule.
    fn invalidates_session_identity(self, submitted_line: &str) -> bool
    where
        Self: Sized,
    {
        self.session_identity_transition_rule(submitted_line)
            .is_some()
    }
}

impl BuiltinAgentProvider {
    /// Registry order drives create-agent selector order and is deliberately
    /// stable for keyboard wrapping and deterministic tree type ordering.
    pub const ALL: [Self; 3] = [Self::Claude, Self::Codex, Self::Antigravity];

    /// Selects an adjacent provider in the registry, wrapping in either
    /// direction for keyboard and mouse controls.
    pub fn stepped(self, direction: i32) -> Self {
        let index = Self::ALL
            .iter()
            .position(|candidate| *candidate == self)
            .unwrap_or(0) as i32;
        Self::ALL[(index + direction.signum()).rem_euclid(Self::ALL.len() as i32) as usize]
    }

    /// Parses a persisted command only when it is exactly one of ilium's own
    /// known resume forms. Arbitrary user shell commands remain opaque.
    pub fn resume_binding(command: &str) -> Option<(Self, String)> {
        Self::ALL.into_iter().find_map(|provider| {
            let session_id = provider.session_id_from_resume_command(command)?;
            Some((provider, session_id))
        })
    }

    /// Converts a configuration class name into the matching built-in
    /// provider. `antimatter` remains a compatibility alias for the requested
    /// name; the actual Google product and installed CLI are Antigravity/agy.
    pub fn from_config_name(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "claude" => Some(Self::Claude),
            "codex" => Some(Self::Codex),
            "antigravity" | "antimatter" => Some(Self::Antigravity),
            _ => None,
        }
    }

    /// Resolves the exact fresh-launch command from a persisted pane origin.
    pub fn from_command_line(command_line: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|provider| provider.command_line() == command_line)
    }

    pub const fn type_sort_rank(self) -> u8 {
        match self {
            Self::Claude => 3,
            Self::Codex => 4,
            Self::Antigravity => 5,
        }
    }

    fn session_id_from_resume_command(self, command: &str) -> Option<String> {
        let raw_session_id = match self {
            Self::Claude => command.strip_prefix("claude --resume ")?,
            Self::Codex => command.strip_prefix("codex resume ")?,
            Self::Antigravity => command.strip_prefix("agy --conversation ")?,
        };
        unquoted_uuid(raw_session_id)
    }
}

impl AgentProvider for BuiltinAgentProvider {
    fn class(self) -> AgentClass {
        match self {
            Self::Claude => AgentClass::Claude,
            Self::Codex => AgentClass::Codex,
            Self::Antigravity => AgentClass::Antigravity,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Claude => "Claude",
            Self::Codex => "Codex",
            Self::Antigravity => "Antigravity",
        }
    }

    fn command_line(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Antigravity => "agy",
        }
    }

    fn process_name_substrings(self) -> &'static [&'static str] {
        match self {
            Self::Claude => &["claude"],
            Self::Codex => &["codex"],
            Self::Antigravity => &["agy", "antigravity", "antimatter"],
        }
    }

    fn resume_command(self, quoted_session_id: &str) -> String {
        match self {
            Self::Claude => format!("claude --resume {quoted_session_id}"),
            Self::Codex => format!("codex resume {quoted_session_id}"),
            Self::Antigravity => format!("agy --conversation {quoted_session_id}"),
        }
    }

    fn session_id_from_arguments(self, arguments: &[String]) -> Option<String> {
        match self {
            Self::Claude => claude_session_id_from_arguments(arguments),
            Self::Codex => codex_session_id_from_arguments(arguments),
            Self::Antigravity => antigravity_session_id_from_arguments(arguments),
        }
    }

    fn session_identity_transition_rule(
        self,
        submitted_line: &str,
    ) -> Option<SessionIdentityTransitionRule> {
        let first_word = submitted_line.split_whitespace().next();
        if matches!(first_word, Some("/resume" | "/branch" | "/new")) {
            return Some(SessionIdentityTransitionRule::SharedNewSessionCommand);
        }

        // Claude and Codex both present `/clear` as a fresh conversation.
        // Treat that user-visible boundary as a fresh ilium session even when
        // the provider process temporarily retains its old transcript handle.
        (matches!(self, Self::Claude | Self::Codex) && submitted_line.trim() == "/clear")
            .then_some(SessionIdentityTransitionRule::ClaudeOrCodexClearStartsFreshSession)
    }
}

fn claude_session_id_from_arguments(arguments: &[String]) -> Option<String> {
    const SESSION_FLAGS: &[&str] = &["--resume", "-r", "--session-id"];
    let option_arguments_end = arguments
        .iter()
        .position(|argument| argument == "--")
        .unwrap_or(arguments.len());
    let option_arguments = &arguments[..option_arguments_end];
    if option_arguments
        .iter()
        .any(|argument| argument == "--fork-session")
    {
        return None;
    }
    // Track the first candidate found and whether we've seen a conflicting one.
    // Same UUID multiple times is OK; different UUIDs cancel each other out.
    let mut found_candidate: Option<String> = None;
    let mut seen_conflict = false;
    for (index, argument) in option_arguments.iter().enumerate() {
        if let Some((flag, value)) = argument.split_once('=') {
            if SESSION_FLAGS.contains(&flag) && looks_like_uuid(value) {
                if let Some(ref candidate) = found_candidate {
                    if candidate != value {
                        seen_conflict = true;
                    }
                } else {
                    found_candidate = Some(value.to_string());
                }
            }
        } else if SESSION_FLAGS.contains(&argument.as_str()) {
            if let Some(value) = option_arguments
                .get(index + 1)
                .filter(|value| looks_like_uuid(value))
            {
                if let Some(ref candidate) = found_candidate {
                    if candidate != value {
                        seen_conflict = true;
                    }
                } else {
                    found_candidate = Some(value.to_string());
                }
            }
        }
    }
    (!seen_conflict).then_some(found_candidate).flatten()
}

fn codex_session_id_from_arguments(arguments: &[String]) -> Option<String> {
    let executable_index = arguments.iter().position(|argument| {
        std::path::Path::new(argument)
            .file_name()
            .and_then(|name| name.to_str())
            == Some("codex")
    })?;
    arguments
        .get(executable_index + 1)
        .filter(|argument| *argument == "resume")?;
    arguments
        .get(executable_index + 2)
        .filter(|candidate| looks_like_uuid(candidate))
        .cloned()
}

fn antigravity_session_id_from_arguments(arguments: &[String]) -> Option<String> {
    let executable_index = arguments.iter().position(|argument| {
        matches!(
            std::path::Path::new(argument)
                .file_name()
                .and_then(|name| name.to_str()),
            Some("agy") | Some("antigravity") | Some("antimatter")
        )
    })?;
    let arguments = &arguments[executable_index + 1..];
    for (index, argument) in arguments.iter().enumerate() {
        if let Some(value) = argument.strip_prefix("--conversation=") {
            if looks_like_uuid(value) {
                return Some(value.to_string());
            }
        }
        if argument == "--conversation" {
            return arguments
                .get(index + 1)
                .filter(|candidate| looks_like_uuid(candidate))
                .cloned();
        }
    }
    None
}

fn unquoted_uuid(value: &str) -> Option<String> {
    let value = value
        .strip_prefix('\'')
        .and_then(|quoted| quoted.strip_suffix('\''))
        .unwrap_or(value);
    (!value.chars().any(char::is_whitespace) && looks_like_uuid(value)).then(|| value.to_string())
}

fn looks_like_uuid(value: &str) -> bool {
    const HYPHEN_INDICES: [usize; 4] = [8, 13, 18, 23];
    value.len() == 36
        && value.chars().enumerate().all(|(index, character)| {
            if HYPHEN_INDICES.contains(&index) {
                character == '-'
            } else {
                character.is_ascii_hexdigit()
            }
        })
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
    /// A working turn just finished and the user has not opened that pane or
    /// submitted newer terminal input. Distinct from `Idle` so every client
    /// can present it as unread completed work until it is acknowledged.
    Done,
    Idle,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PaneStatus {
    /// Terminal pane, no agent CLI detected in it.
    PlainShell,
    /// Terminal pane running a detected agent CLI.
    Agent(AgentClass, AgentActivity),
    /// Terminal pane running a detected agent CLI with a visible persistent
    /// task goal. Kept distinct so goal state travels through the existing
    /// snapshot/status-change contract without parallel client-local state.
    AgentWithGoal(AgentClass, AgentActivity),
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

/// Identifies who last established a node's current place and label in the
/// workspace tree. For groups and split views this is also their creation
/// source; panes and folders already exist before an AI restructure, so the
/// value instead records who last arranged or retitled that existing item.
///
/// This stays on the domain node rather than in the client prompt code so it
/// survives snapshots, undo, and every attached client seeing the same tree.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum StructureSource {
    #[default]
    Manual,
    LlmRestructure,
}

impl StructureSource {
    pub const fn prompt_label(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::LlmRestructure => "LLM restructure",
        }
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

/// How often one queued prompt is delivered after successive agent-finished
/// transitions. The default is deliberately one delivery: repeated delivery
/// is explicit because an unattended agent can otherwise loop indefinitely.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PromptQueueDelivery {
    Once,
    Times { remaining_runs: u32 },
    Forever,
}

impl PromptQueueDelivery {
    pub fn is_valid(&self) -> bool {
        !matches!(self, Self::Times { remaining_runs: 0 })
    }
}

/// One durable FIFO prompt waiting for an agent-finished transition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueuedPrompt {
    pub text: String,
    pub delivery: PromptQueueDelivery,
}

impl QueuedPrompt {
    pub fn is_valid(&self) -> bool {
        !self.text.trim().is_empty() && self.delivery.is_valid()
    }
}

pub const MAXIMUM_SPLIT_VIEW_PANES: usize = 4;

/// Hard cap on how many prompts one pane may hold in `prompt_queue`. Recurring
/// entries (`Times`/`Forever`) are rotated to the tail on delivery rather than
/// pinned at the head (see [`Tree::acknowledge_queued_prompt`]), so under
/// normal operation the queue is bounded by what a user actually queued. This
/// cap exists only to stop a misbehaving client or automation loop from
/// growing the persisted/broadcast snapshot without bound.
pub const MAXIMUM_PROMPT_QUEUE_LEN: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SplitOrientation {
    Vertical,
    Horizontal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContainerKind {
    /// A persisted, top-level workspace boundary. Projects own the working
    /// directory inherited by every entry created beneath them.
    Project {
        path: PathBuf,
    },
    Group,
    SplitView {
        orientation: SplitOrientation,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContainerNode {
    pub kind: ContainerKind,
    pub children: Vec<NodeId>,
    pub expanded: bool,
}

impl ContainerNode {
    pub fn project(path: PathBuf) -> Self {
        Self {
            kind: ContainerKind::Project { path },
            children: Vec::new(),
            expanded: true,
        }
    }

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

    pub const fn is_project(&self) -> bool {
        matches!(self.kind, ContainerKind::Project { .. })
    }

    pub const fn accepts_normal_children(&self) -> bool {
        matches!(
            self.kind,
            ContainerKind::Project { .. } | ContainerKind::Group
        )
    }

    pub const fn is_split_view(&self) -> bool {
        matches!(self.kind, ContainerKind::SplitView { .. })
    }

    pub const fn split_orientation(&self) -> Option<SplitOrientation> {
        match self.kind {
            ContainerKind::Project { .. } | ContainerKind::Group => None,
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
        /// FIFO prompts sent only after a detected agent-finished transition.
        /// `serde(default)` keeps existing recovery snapshots compatible.
        #[serde(default)]
        prompt_queue: Vec<QueuedPrompt>,
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
    /// Optional LLM-suggested visual marker for this title. It is durable but
    /// presentation-only: the client displays it only when its Appearance
    /// preference enables inferred title icons.
    #[serde(default)]
    pub inferred_icon: Option<String>,
    /// Kept separately from pane title provenance because a manual move can
    /// change the meaning of a structure even when no pane title changed.
    /// Missing fields in older JSON crash snapshots are manual by default.
    #[serde(default)]
    pub structure_source: StructureSource,
    pub kind: NodeKind,
}

impl Node {
    pub fn is_group(&self) -> bool {
        matches!(&self.kind, NodeKind::Container(container) if container.is_group())
    }

    pub fn is_project(&self) -> bool {
        matches!(&self.kind, NodeKind::Container(container) if container.is_project())
    }

    /// Whether this node can directly own ordinary project entries.
    pub fn accepts_normal_children(&self) -> bool {
        matches!(&self.kind, NodeKind::Container(container) if container.accepts_normal_children())
    }

    pub fn project_path(&self) -> Option<&std::path::Path> {
        match &self.kind {
            NodeKind::Container(ContainerNode {
                kind: ContainerKind::Project { path },
                ..
            }) => Some(path),
            _ => None,
        }
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
    #[error("node {0:?} is not a project")]
    NotAProject(NodeId),
    #[error("node {0:?} is not a container")]
    NotAContainer(NodeId),
    #[error("node {0:?} is not a split view")]
    NotASplitView(NodeId),
    #[error("node {0:?} is not a pane")]
    NotAPane(NodeId),
    #[error("pane {0:?} is not a terminal")]
    NotATerminal(NodeId),
    #[error("scheduled input must contain text, Enter, or both")]
    EmptyScheduledInput,
    #[error("queued prompt must contain text and a positive delivery count")]
    EmptyQueuedPrompt,
    #[error("pane {0:?} prompt queue already holds the maximum of {1} entries")]
    PromptQueueFull(NodeId, usize),
    #[error("cannot remove the root group")]
    CannotRemoveRoot,
    #[error("cannot move the root group")]
    CannotMoveRoot,
    #[error("cannot move node {0:?} into its own descendant {1:?}")]
    CannotMoveIntoDescendant(NodeId, NodeId),
    #[error("panes cannot be direct children of the session root; put them in a group")]
    PanesRequireGroup,
    #[error("only projects can be direct children of the session root")]
    RootRequiresProject,
    #[error("projects must remain direct children of the session root")]
    ProjectMustRemainTopLevel,
    #[error("cannot move node {node:?} from project {from_project:?} into project {to_project:?}")]
    CrossProjectMove {
        node: NodeId,
        from_project: NodeId,
        to_project: NodeId,
    },
    #[error("node {0:?} is not inside a project")]
    NodeOutsideProject(NodeId),
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
    #[error("split view {0:?} was referenced more than once in a restructure plan")]
    RestructureDuplicateSplitView(NodeId),
    #[error("restructure plan referenced split view {split_view:?} outside project {project:?}")]
    RestructureSplitViewOutsideProject { split_view: NodeId, project: NodeId },
    #[error(
        "restructure plan changed the protected split-view set (expected {expected:?}, actual {actual:?})"
    )]
    RestructureSplitViewSetMismatch {
        expected: Vec<NodeId>,
        actual: Vec<NodeId>,
    },
    #[error(
        "restructure plan changed protected split view {split_view:?} children (expected {expected:?}, actual {actual:?})"
    )]
    RestructureSplitViewChildrenChanged {
        split_view: NodeId,
        expected: Vec<NodeId>,
        actual: Vec<NodeId>,
    },
    #[error("board storage is already open at {}", .0.display())]
    BoardStorageAlreadyOpen(PathBuf),
    #[error("node {0:?} is not a folder")]
    NotAFolder(NodeId),
    #[error("node {0:?} was referenced more than once in a restructure plan")]
    RestructureDuplicateLeaf(NodeId),
    #[error("restructure plan referenced node {node:?} outside project {project:?}")]
    RestructureLeafOutsideProject { node: NodeId, project: NodeId },
    #[error(
        "restructure plan referenced {actual} existing pane/folder(s), expected exactly {expected}"
    )]
    RestructureLeafSetMismatch { expected: usize, actual: usize },
    /// Raised only by [`Tree::validate`]: the tree's `nodes`/`next_id`
    /// shape violates an invariant every other method on this type assumes
    /// holds (e.g. every mutation method that reads a node's parent
    /// container). `Tree` derives `Deserialize` so a crash-recovery
    /// snapshot can be loaded wholesale (see `ilium-server`'s
    /// `persistence` module); deserializing arbitrary JSON bypasses every
    /// invariant `add_pane`/`move_node`/etc. normally enforce, so a
    /// hand-edited or otherwise corrupted snapshot file must fail here,
    /// loudly, rather than let a later operation panic or silently
    /// corrupt state.
    #[error("tree structure is invalid: {0}")]
    InvalidStructure(String),
}

/// One node in a proposed new tree shape, as produced by a restructure
/// inference call and consumed by [`Tree::apply_restructure`]. This
/// describes a full replacement of everything under [`ROOT_ID`], not a diff:
/// `Pane`/`Folder` always reference an existing node by id (their identity
/// and any backing resource -- a PTY, an open editor buffer -- must survive
/// unchanged). Normal `Group` nodes are freshly authored. Split views are
/// different: they are user-created presentation layouts, so a plan can only
/// reference an existing split by id and must preserve its exact pane order.
/// The panes nested beneath that reference still carry AI-authored titles.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RestructureNode {
    Pane {
        id: NodeId,
        title: String,
        short_title: Option<String>,
        icon: Option<String>,
    },
    Folder {
        id: NodeId,
        title: String,
        short_title: Option<String>,
        icon: Option<String>,
    },
    Group {
        title: String,
        short_title: Option<String>,
        icon: Option<String>,
        children: Vec<RestructureNode>,
    },
    ExistingSplitView {
        id: NodeId,
        /// Enforced to equal the split view's current pane ids and order by
        /// [`Tree::apply_restructure`]. Only the nested panes' title fields
        /// are mutable; the split's identity, orientation, name, icon, and
        /// membership are user-owned and survive unchanged.
        children: Vec<RestructureNode>,
    },
}

/// A complete proposed replacement for everything under [`ROOT_ID`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RestructurePlan {
    pub children: Vec<RestructureNode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RestructureParentKind {
    Project,
    Group,
    SplitView,
}

#[derive(Default)]
struct RestructureReferences {
    leaves: HashSet<NodeId>,
    split_views: HashSet<NodeId>,
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
                inferred_icon: None,
                structure_source: StructureSource::Manual,
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

    /// Returns the enclosing project for `id`, including `id` itself when it
    /// is a project. Every non-root runtime entry must have exactly one.
    pub fn project_ancestor(&self, id: NodeId) -> Option<NodeId> {
        let mut current = Some(id);
        while let Some(node_id) = current {
            let node = self.get(node_id)?;
            if node.is_project() {
                return Some(node_id);
            }
            current = node.parent;
        }
        None
    }

    pub fn project_path_for(&self, id: NodeId) -> Option<&std::path::Path> {
        self.project_ancestor(id)
            .and_then(|project_id| self.get(project_id))
            .and_then(Node::project_path)
    }

    pub fn project_ids(&self) -> Vec<NodeId> {
        self.children_of(ROOT_ID)
            .unwrap_or_default()
            .iter()
            .copied()
            .filter(|id| self.get(*id).is_some_and(Node::is_project))
            .collect()
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

    /// Finds the ordinary group that owns `node_id`. Split views are a
    /// presentation container rather than a folder boundary, so a pane inside
    /// one belongs to the nearest enclosing `Group`; projects and the session
    /// root are skipped unless the root itself is the legacy group owner.
    pub fn containing_group(&self, node_id: NodeId) -> Option<NodeId> {
        let mut current = Some(node_id);
        while let Some(candidate) = current {
            let node = self.get(candidate)?;
            if node.is_group() {
                return Some(candidate);
            }
            current = node.parent;
        }
        None
    }

    /// Returns the focusable panes owned by one ordinary group in its visible
    /// child order. Direct pane children and direct split-view members are one
    /// group-level sequence; nested groups deliberately remain separate
    /// folders, reached by group-jump navigation rather than by cycling.
    pub fn navigable_panes_in_group(&self, group_id: NodeId) -> Vec<NodeId> {
        if !self.get(group_id).is_some_and(Node::is_group) {
            return Vec::new();
        }
        let Ok(children) = self.children_of(group_id) else {
            return Vec::new();
        };
        let mut pane_ids = Vec::new();
        for child in children {
            match self.get(*child) {
                Some(node) if node.is_pane() => pane_ids.push(*child),
                Some(node) if node.is_split_view() => {
                    if let Ok(split_children) = self.children_of(*child) {
                        pane_ids.extend(
                            split_children
                                .iter()
                                .copied()
                                .filter(|pane_id| self.get(*pane_id).is_some_and(Node::is_pane)),
                        );
                    }
                }
                _ => {}
            }
        }
        pane_ids
    }

    /// Lists ordinary groups in the same depth-first order shown by the tree
    /// panel. Project containers are transparent ownership boundaries here:
    /// Jump is about folders of panes, not switching a project heading.
    pub fn group_ids_in_tree_order(&self) -> Vec<NodeId> {
        fn collect(tree: &Tree, parent: NodeId, groups: &mut Vec<NodeId>) {
            let Ok(children) = tree.children_of(parent) else {
                return;
            };
            for child in children {
                let Some(node) = tree.get(*child) else {
                    continue;
                };
                if node.is_group() {
                    groups.push(*child);
                }
                if node.is_container() {
                    collect(tree, *child, groups);
                }
            }
        }

        let mut groups = Vec::new();
        if self.get(ROOT_ID).is_some_and(Node::is_group)
            && !self.navigable_panes_in_group(ROOT_ID).is_empty()
        {
            groups.push(ROOT_ID);
        }
        collect(self, ROOT_ID, &mut groups);
        groups
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

    /// Adds a top-level project. Only projects may be direct root children.
    pub fn add_project(&mut self, path: PathBuf) -> Result<NodeId, TreeError> {
        let name = project_display_name(&path);
        let id = self.alloc_id();
        self.push_child(ROOT_ID, id)?;
        self.nodes.insert(
            id,
            Node {
                id,
                parent: Some(ROOT_ID),
                name,
                short_name: None,
                inferred_icon: None,
                structure_source: StructureSource::Manual,
                kind: NodeKind::Container(ContainerNode::project(path)),
            },
        );
        Ok(id)
    }

    /// Ensures a project exists for `path`, and upgrades older snapshots
    /// whose root directly contained ordinary groups/folders into that one
    /// launch project without changing the identity of their descendants.
    pub fn ensure_launch_project(&mut self, path: PathBuf) -> Result<NodeId, TreeError> {
        if let Some(existing) = self.project_ids().into_iter().find(|project_id| {
            self.get(*project_id)
                .and_then(Node::project_path)
                .is_some_and(|existing_path| existing_path == path)
        }) {
            return Ok(existing);
        }

        // A persisted project tree is already in the modern shape. Its
        // launch project may legitimately have been pointed at a different
        // directory since the server originally started, so never add a new
        // wrapper merely because `session_cwd` no longer matches a project
        // path. Wrapping is only the legacy-root migration below.
        if let Some(existing_project) = self.project_ids().into_iter().next() {
            return Ok(existing_project);
        }

        let legacy_children = self.children_of(ROOT_ID)?.to_vec();
        let project_id = self.add_project(path)?;
        for child_id in legacy_children {
            if child_id == project_id {
                continue;
            }
            self.move_node(child_id, project_id, None)?;
        }
        Ok(project_id)
    }

    pub fn change_project_folder(
        &mut self,
        project_id: NodeId,
        path: PathBuf,
    ) -> Result<(), TreeError> {
        let node = self.get_mut(project_id)?;
        let NodeKind::Container(ContainerNode {
            kind: ContainerKind::Project { path: project_path },
            ..
        }) = &mut node.kind
        else {
            return Err(TreeError::NotAProject(project_id));
        };
        *project_path = path;
        Ok(())
    }

    pub fn add_group(
        &mut self,
        parent: NodeId,
        name: impl Into<String>,
    ) -> Result<NodeId, TreeError> {
        let parent_node = self.get(parent).ok_or(TreeError::NodeNotFound(parent))?;
        if !matches!(&parent_node.kind, NodeKind::Container(container) if container.accepts_normal_children())
        {
            return Err(TreeError::NotAGroup(parent));
        }
        let id = self.alloc_id();
        self.push_child(parent, id)?;
        self.nodes.insert(
            id,
            Node {
                id,
                parent: Some(parent),
                name: name.into(),
                short_name: None,
                inferred_icon: None,
                structure_source: StructureSource::Manual,
                kind: NodeKind::Container(ContainerNode::group()),
            },
        );
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
        self.push_child(parent, id)?;
        self.nodes.insert(
            id,
            Node {
                id,
                parent: Some(parent),
                name: name.into(),
                short_name: None,
                inferred_icon: None,
                structure_source: StructureSource::Manual,
                kind: NodeKind::Pane {
                    content,
                    status,
                    title_source: PaneTitleSource::Automatic,
                    board_storage: None,
                    scheduled_input: None,
                    prompt_queue: Vec::new(),
                },
            },
        );
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
        if self.panes().any(|node| {
            matches!(
                &node.kind,
                NodeKind::Pane {
                    board_storage: Some(existing_storage),
                    ..
                } if existing_storage.path() == storage.path()
            )
        }) {
            return Err(TreeError::BoardStorageAlreadyOpen(
                storage.path().to_path_buf(),
            ));
        }
        let id = self.alloc_id();
        self.push_child(parent, id)?;
        self.nodes.insert(
            id,
            Node {
                id,
                parent: Some(parent),
                name,
                short_name: None,
                inferred_icon: None,
                structure_source: StructureSource::Manual,
                kind: NodeKind::Pane {
                    content: PaneContentKind::Board,
                    status: PaneStatus::Board,
                    title_source: PaneTitleSource::Automatic,
                    board_storage: Some(storage),
                    scheduled_input: None,
                    prompt_queue: Vec::new(),
                },
            },
        );
        Ok(id)
    }

    /// Adds a filesystem root beneath a group. Only this stable reference is
    /// persisted; files under it remain local filesystem data.
    pub fn add_folder(&mut self, parent: NodeId, path: PathBuf) -> Result<NodeId, TreeError> {
        if parent == ROOT_ID {
            return Err(TreeError::RootRequiresProject);
        }
        if !matches!(self.get(parent).map(|node| &node.kind), Some(NodeKind::Container(container)) if container.accepts_normal_children())
        {
            return Err(TreeError::NotAGroup(parent));
        }
        let name = path
            .file_name()
            .filter(|name| !name.is_empty())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        let id = self.alloc_id();
        self.push_child(parent, id)?;
        self.nodes.insert(
            id,
            Node {
                id,
                parent: Some(parent),
                name,
                short_name: None,
                inferred_icon: None,
                structure_source: StructureSource::Manual,
                kind: NodeKind::Folder { path },
            },
        );
        Ok(id)
    }

    pub fn create_split_view(
        &mut self,
        parent_group: NodeId,
        name: impl Into<String>,
        orientation: SplitOrientation,
        pane_ids: &[NodeId],
    ) -> Result<NodeId, TreeError> {
        if parent_group == ROOT_ID {
            return Err(TreeError::RootRequiresProject);
        }
        if pane_ids.len() > MAXIMUM_SPLIT_VIEW_PANES {
            return Err(TreeError::SplitViewCapacityReached);
        }
        if !matches!(self.get(parent_group).map(|node| &node.kind), Some(NodeKind::Container(container)) if container.accepts_normal_children())
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
                inferred_icon: None,
                structure_source: StructureSource::Manual,
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

    /// Replaces everything under [`ROOT_ID`] with `plan`'s shape. Every
    /// existing pane and folder must appear exactly once as a leaf somewhere
    /// in the plan -- no id missing, none invented, none duplicated -- which
    /// is validated in full against the current tree before anything is
    /// mutated; any single violation rejects the whole plan and leaves the
    /// tree untouched. On success, normal groups are rebuilt, while every
    /// pane, folder, and user-owned split view keeps its `NodeId`. A split
    /// view also keeps its orientation, title metadata, and exact ordered
    /// membership; only the nested panes' titles may change.
    pub fn apply_restructure(&mut self, plan: RestructurePlan) -> Result<(), TreeError> {
        let projects = self.project_ids();
        if projects.is_empty() {
            return self.apply_legacy_restructure(plan);
        }
        if projects.len() != 1 {
            return Err(TreeError::RootRequiresProject);
        }
        self.apply_project_restructure(projects[0], plan)
    }

    fn apply_legacy_restructure(&mut self, plan: RestructurePlan) -> Result<(), TreeError> {
        let mut referenced = RestructureReferences::default();
        self.validate_restructure_children(
            &plan.children,
            RestructureParentKind::Group,
            &mut referenced,
        )?;
        let existing_leaf_count = self
            .nodes
            .values()
            .filter(|node| node.is_pane() || node.is_folder())
            .count();
        if referenced.leaves.len() != existing_leaf_count {
            return Err(TreeError::RestructureLeafSetMismatch {
                expected: existing_leaf_count,
                actual: referenced.leaves.len(),
            });
        }
        self.validate_restructure_split_view_set(
            self.nodes
                .values()
                .filter(|node| node.is_split_view())
                .map(|node| node.id),
            &referenced.split_views,
        )?;

        let mut updated = self.clone();
        if let NodeKind::Container(container) = &mut updated.get_mut(ROOT_ID)?.kind {
            container.children.clear();
        }
        let old_container_ids: Vec<NodeId> = updated
            .nodes
            .values()
            .filter(|node| node.is_group() && node.id != ROOT_ID)
            .map(|node| node.id)
            .collect();
        for id in old_container_ids {
            updated.nodes.remove(&id);
        }
        Self::rebuild_restructure_children(&mut updated, ROOT_ID, &plan.children)?;
        *self = updated;
        Ok(())
    }

    /// Replaces only `project_id`'s descendants with an LLM-authored plan.
    /// The project node, its directory, and every other project's structure
    /// survive unchanged; referenced leaves must be exactly this project's
    /// current panes/folder roots. Existing split views are protected
    /// user-owned layout nodes: every one must appear exactly once with the
    /// same ordered children, and no new split id can be introduced.
    pub fn apply_project_restructure(
        &mut self,
        project_id: NodeId,
        plan: RestructurePlan,
    ) -> Result<(), TreeError> {
        if !self.get(project_id).is_some_and(Node::is_project) {
            return Err(TreeError::NotAProject(project_id));
        }

        let mut referenced = RestructureReferences::default();
        self.validate_restructure_children(
            &plan.children,
            RestructureParentKind::Project,
            &mut referenced,
        )?;
        for leaf_id in &referenced.leaves {
            if self.project_ancestor(*leaf_id) != Some(project_id) {
                return Err(TreeError::RestructureLeafOutsideProject {
                    node: *leaf_id,
                    project: project_id,
                });
            }
        }
        for split_view_id in &referenced.split_views {
            if self.project_ancestor(*split_view_id) != Some(project_id) {
                return Err(TreeError::RestructureSplitViewOutsideProject {
                    split_view: *split_view_id,
                    project: project_id,
                });
            }
        }

        let existing_leaf_count = self
            .nodes
            .values()
            .filter(|node| {
                (node.is_pane() || node.is_folder())
                    && self.project_ancestor(node.id) == Some(project_id)
            })
            .count();
        if referenced.leaves.len() != existing_leaf_count {
            return Err(TreeError::RestructureLeafSetMismatch {
                expected: existing_leaf_count,
                actual: referenced.leaves.len(),
            });
        }
        self.validate_restructure_split_view_set(
            self.nodes
                .values()
                .filter(|node| {
                    node.is_split_view() && self.project_ancestor(node.id) == Some(project_id)
                })
                .map(|node| node.id),
            &referenced.split_views,
        )?;

        let mut updated = self.clone();
        if let NodeKind::Container(container) = &mut updated.get_mut(project_id)?.kind {
            container.children.clear();
        }
        let old_container_ids: Vec<NodeId> = updated
            .nodes
            .values()
            .filter(|node| {
                node.id != project_id
                    && node.is_group()
                    && updated.is_ancestor_of(project_id, node.id)
            })
            .map(|node| node.id)
            .collect();
        for id in old_container_ids {
            updated.nodes.remove(&id);
        }

        Self::rebuild_restructure_children(&mut updated, project_id, &plan.children)?;
        *self = updated;
        Ok(())
    }

    /// Restores only one project subtree from a tree captured immediately
    /// before that project's restructure. Other projects and later edits to
    /// them remain untouched.
    pub fn restore_project_from(
        &mut self,
        project_id: NodeId,
        previous: &Tree,
    ) -> Result<(), TreeError> {
        if !self.get(project_id).is_some_and(Node::is_project)
            || !previous.get(project_id).is_some_and(Node::is_project)
        {
            return Err(TreeError::NotAProject(project_id));
        }

        let current_descendants: Vec<NodeId> = self
            .nodes
            .keys()
            .copied()
            .filter(|node_id| *node_id != project_id && self.is_ancestor_of(project_id, *node_id))
            .collect();
        for node_id in current_descendants {
            self.nodes.remove(&node_id);
        }

        let previous_descendants: Vec<Node> = previous
            .nodes
            .values()
            .filter(|node| node.id != project_id && previous.is_ancestor_of(project_id, node.id))
            .cloned()
            .collect();
        for node in previous_descendants {
            self.nodes.insert(node.id, node);
        }

        let previous_children = previous.children_of(project_id)?.to_vec();
        let NodeKind::Container(container) = &mut self.get_mut(project_id)?.kind else {
            return Err(TreeError::NotAProject(project_id));
        };
        container.children = previous_children;
        Ok(())
    }

    /// Validates a plan subtree against the *current* (pre-restructure) tree:
    /// every referenced id exists and is the kind it claims to be, no id is
    /// referenced twice anywhere in the whole plan, and every protected split
    /// has its exact current pane ids in the exact current order. Whether the
    /// plan covers every existing leaf and split is checked once by the caller
    /// after this returns, since that requires each full referenced set.
    fn validate_restructure_children(
        &self,
        nodes: &[RestructureNode],
        parent_kind: RestructureParentKind,
        referenced: &mut RestructureReferences,
    ) -> Result<(), TreeError> {
        for node in nodes {
            match node {
                RestructureNode::Pane { id, .. } => {
                    if !self.get(*id).ok_or(TreeError::NodeNotFound(*id))?.is_pane() {
                        return Err(TreeError::NotAPane(*id));
                    }
                    if !referenced.leaves.insert(*id) {
                        return Err(TreeError::RestructureDuplicateLeaf(*id));
                    }
                }
                RestructureNode::Folder { id, .. } => {
                    if parent_kind == RestructureParentKind::SplitView {
                        return Err(TreeError::SplitViewOnlyAcceptsPanes);
                    }
                    if !self
                        .get(*id)
                        .ok_or(TreeError::NodeNotFound(*id))?
                        .is_folder()
                    {
                        return Err(TreeError::NotAFolder(*id));
                    }
                    if !referenced.leaves.insert(*id) {
                        return Err(TreeError::RestructureDuplicateLeaf(*id));
                    }
                }
                RestructureNode::Group { children, .. } => {
                    if parent_kind == RestructureParentKind::SplitView {
                        return Err(TreeError::SplitViewOnlyAcceptsPanes);
                    }
                    self.validate_restructure_children(
                        children,
                        RestructureParentKind::Group,
                        referenced,
                    )?;
                }
                RestructureNode::ExistingSplitView { id, children } => {
                    if parent_kind == RestructureParentKind::SplitView {
                        return Err(TreeError::SplitViewOnlyAcceptsPanes);
                    }
                    let existing = self.get(*id).ok_or(TreeError::NodeNotFound(*id))?;
                    if !existing.is_split_view() {
                        return Err(TreeError::NotASplitView(*id));
                    }
                    if !referenced.split_views.insert(*id) {
                        return Err(TreeError::RestructureDuplicateSplitView(*id));
                    }
                    let actual_children = children
                        .iter()
                        .map(|child| match child {
                            RestructureNode::Pane { id, .. } => Ok(*id),
                            RestructureNode::Folder { .. }
                            | RestructureNode::Group { .. }
                            | RestructureNode::ExistingSplitView { .. } => {
                                Err(TreeError::SplitViewOnlyAcceptsPanes)
                            }
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    let expected_children = self.children_of(*id)?.to_vec();
                    if actual_children != expected_children {
                        return Err(TreeError::RestructureSplitViewChildrenChanged {
                            split_view: *id,
                            expected: expected_children,
                            actual: actual_children,
                        });
                    }
                    self.validate_restructure_children(
                        children,
                        RestructureParentKind::SplitView,
                        referenced,
                    )?;
                }
            }
        }
        Ok(())
    }

    fn validate_restructure_split_view_set(
        &self,
        expected: impl IntoIterator<Item = NodeId>,
        actual: &HashSet<NodeId>,
    ) -> Result<(), TreeError> {
        let mut expected: Vec<NodeId> = expected.into_iter().collect();
        let mut actual: Vec<NodeId> = actual.iter().copied().collect();
        expected.sort_unstable();
        actual.sort_unstable();
        if expected != actual {
            return Err(TreeError::RestructureSplitViewSetMismatch { expected, actual });
        }
        Ok(())
    }

    /// Builds `nodes` as the children of `parent` on an already-validated
    /// plan: reattaches referenced panes/folders in place (updating parent
    /// and title), allocates fresh ids for normal groups, and reattaches every
    /// protected split using its existing id and unchanged presentation data.
    fn rebuild_restructure_children(
        tree: &mut Tree,
        parent: NodeId,
        nodes: &[RestructureNode],
    ) -> Result<(), TreeError> {
        for node in nodes {
            let child_id = match node {
                RestructureNode::Pane {
                    id,
                    title,
                    short_title,
                    icon,
                } => {
                    let existing = tree.get_mut(*id)?;
                    existing.parent = Some(parent);
                    existing.name = title.clone();
                    existing.short_name = short_title.clone();
                    existing.inferred_icon = icon.clone();
                    existing.structure_source = StructureSource::LlmRestructure;
                    // A restructure-authored title is curated, not a
                    // per-turn automatic guess: freeze it the same way a
                    // plain user rename does, so the background per-pane
                    // titler never overwrites it on the pane's next turn.
                    if let NodeKind::Pane { title_source, .. } = &mut existing.kind {
                        *title_source = PaneTitleSource::UserSpecified;
                    }
                    *id
                }
                RestructureNode::Folder {
                    id,
                    title,
                    short_title,
                    icon,
                } => {
                    let existing = tree.get_mut(*id)?;
                    existing.parent = Some(parent);
                    existing.name = title.clone();
                    existing.short_name = short_title.clone();
                    existing.inferred_icon = icon.clone();
                    existing.structure_source = StructureSource::LlmRestructure;
                    *id
                }
                RestructureNode::Group {
                    title,
                    short_title,
                    icon,
                    children,
                } => {
                    let group_id = tree.alloc_id();
                    tree.nodes.insert(
                        group_id,
                        Node {
                            id: group_id,
                            parent: Some(parent),
                            name: title.clone(),
                            short_name: short_title.clone(),
                            inferred_icon: icon.clone(),
                            structure_source: StructureSource::LlmRestructure,
                            kind: NodeKind::Container(ContainerNode::group()),
                        },
                    );
                    Self::rebuild_restructure_children(tree, group_id, children)?;
                    group_id
                }
                RestructureNode::ExistingSplitView { id, children } => {
                    let existing = tree.get_mut(*id)?;
                    let NodeKind::Container(container) = &mut existing.kind else {
                        return Err(TreeError::NotASplitView(*id));
                    };
                    if !container.is_split_view() {
                        return Err(TreeError::NotASplitView(*id));
                    }
                    existing.parent = Some(parent);
                    container.children.clear();
                    Self::rebuild_restructure_children(tree, *id, children)?;
                    *id
                }
            };
            tree.push_child(parent, child_id)?;
        }
        Ok(())
    }

    /// Removes a node. If it is a container, removes its whole subtree.
    pub fn remove_node(&mut self, id: NodeId) -> Result<(), TreeError> {
        if id == ROOT_ID {
            return Err(TreeError::CannotRemoveRoot);
        }
        let node = self.get(id).ok_or(TreeError::NodeNotFound(id))?;
        let parent = node.parent;

        // Collect the whole subtree first (DFS), then remove it in one pass.
        let mut to_remove = vec![id];
        let mut frontier = vec![id];
        while let Some(current) = frontier.pop() {
            if let Some(Node {
                kind: NodeKind::Container(ContainerNode { children, .. }),
                ..
            }) = self.nodes.get(&current)
            {
                for &child in children {
                    to_remove.push(child);
                    frontier.push(child);
                }
            }
        }
        if let Some(parent) = parent {
            self.remove_child(parent, id)?;
        }
        for node_id in &to_remove {
            self.nodes.remove(node_id);
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
        inferred_icon: Option<String>,
    ) -> Result<(), TreeError> {
        let node = self.get_mut(id)?;
        node.name = name.into();
        node.short_name = short_name;
        node.inferred_icon = inferred_icon;
        node.structure_source = StructureSource::Manual;
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
        inferred_icon: Option<String>,
    ) -> Result<bool, TreeError> {
        let node = self.get_mut(id)?;
        let NodeKind::Pane { title_source, .. } = &node.kind else {
            return Err(TreeError::NotAPane(id));
        };
        if title_source.is_user_specified() {
            return Ok(false);
        }
        let title = title.into();
        if node.name == title
            && node.short_name == short_title
            && node.inferred_icon == inferred_icon
        {
            return Ok(false);
        }
        node.name = title;
        node.short_name = short_title;
        node.inferred_icon = inferred_icon;
        Ok(true)
    }

    /// Resets a terminal whose agent conversation started over. The old title
    /// and tree grouping both described the cleared conversation, so this
    /// discards every title field and moves the pane directly under its
    /// owning project. The transition is applied to a clone first so a
    /// structural rejection cannot leave either half committed.
    pub fn reset_terminal_pane_for_fresh_conversation(
        &mut self,
        id: NodeId,
        title: impl Into<String>,
    ) -> Result<bool, TreeError> {
        let mut updated_tree = self.clone();
        let node = updated_tree.get(id).ok_or(TreeError::NodeNotFound(id))?;
        let NodeKind::Pane { content, .. } = &node.kind else {
            return Err(TreeError::NotAPane(id));
        };
        if *content != PaneContentKind::Terminal {
            return Err(TreeError::NotATerminal(id));
        }

        let project_id = updated_tree
            .project_ancestor(id)
            .ok_or(TreeError::NodeOutsideProject(id))?;
        let node = updated_tree.get_mut(id)?;
        let NodeKind::Pane { title_source, .. } = &mut node.kind else {
            unreachable!("the pane kind was validated before the project move");
        };

        let title = title.into();
        let changed = node.name != title
            || node.short_name.is_some()
            || node.inferred_icon.is_some()
            || title_source.is_user_specified();
        node.name = title;
        node.short_name = None;
        node.inferred_icon = None;
        *title_source = PaneTitleSource::Automatic;

        let placement_changed = node.parent != Some(project_id);
        if placement_changed {
            updated_tree.move_node(id, project_id, None)?;
        }

        *self = updated_tree;
        Ok(changed || placement_changed)
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

    /// Acknowledges that the user has seen a completed agent turn. Only
    /// `Done` changes: concurrent detection may already have observed
    /// renewed work, and a stale acknowledgement must never overwrite that
    /// newer activity with `Idle`.
    pub fn acknowledge_agent_completion(
        &mut self,
        id: NodeId,
    ) -> Result<Option<PaneStatus>, TreeError> {
        let NodeKind::Pane { status, .. } = &mut self.get_mut(id)?.kind else {
            return Err(TreeError::NotAPane(id));
        };
        let acknowledged_status = match status {
            PaneStatus::Agent(class, AgentActivity::Done) => {
                Some(PaneStatus::Agent(class.clone(), AgentActivity::Idle))
            }
            PaneStatus::AgentWithGoal(class, AgentActivity::Done) => Some(
                PaneStatus::AgentWithGoal(class.clone(), AgentActivity::Idle),
            ),
            _ => None,
        };
        if let Some(acknowledged_status) = &acknowledged_status {
            *status = acknowledged_status.clone();
        }
        Ok(acknowledged_status)
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
    pub fn scheduled_pane_inputs(&self) -> impl Iterator<Item = (NodeId, &ScheduledPaneInput)> {
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

    /// Adds one prompt to a terminal pane's durable FIFO queue.
    pub fn enqueue_prompt(&mut self, id: NodeId, prompt: QueuedPrompt) -> Result<(), TreeError> {
        if !prompt.is_valid() {
            return Err(TreeError::EmptyQueuedPrompt);
        }
        let node = self.get_mut(id)?;
        let NodeKind::Pane {
            content,
            prompt_queue,
            ..
        } = &mut node.kind
        else {
            return Err(TreeError::NotAPane(id));
        };
        if *content != PaneContentKind::Terminal {
            return Err(TreeError::NotATerminal(id));
        }
        if prompt_queue.len() >= MAXIMUM_PROMPT_QUEUE_LEN {
            return Err(TreeError::PromptQueueFull(id, MAXIMUM_PROMPT_QUEUE_LEN));
        }
        prompt_queue.push(prompt);
        Ok(())
    }

    /// Removes every pending prompt for one terminal pane.
    pub fn clear_prompt_queue(&mut self, id: NodeId) -> Result<(), TreeError> {
        let node = self.get_mut(id)?;
        let NodeKind::Pane {
            content,
            prompt_queue,
            ..
        } = &mut node.kind
        else {
            return Err(TreeError::NotAPane(id));
        };
        if *content != PaneContentKind::Terminal {
            return Err(TreeError::NotATerminal(id));
        }
        prompt_queue.clear();
        Ok(())
    }

    /// Returns the FIFO head without consuming it. The server writes it to
    /// the PTY before acknowledging delivery, so a failed write leaves it
    /// queued for the next genuine completion.
    pub fn next_queued_prompt(&self, id: NodeId) -> Result<Option<&QueuedPrompt>, TreeError> {
        let node = self.get(id).ok_or(TreeError::NodeNotFound(id))?;
        let NodeKind::Pane {
            content,
            prompt_queue,
            ..
        } = &node.kind
        else {
            return Err(TreeError::NotAPane(id));
        };
        if *content != PaneContentKind::Terminal {
            return Err(TreeError::NotATerminal(id));
        }
        Ok(prompt_queue.first())
    }

    /// Acknowledges delivery only when the current FIFO head is still the
    /// entry the server read. This compare-and-advance prevents a concurrent
    /// clear or enqueue mutation from consuming the wrong prompt.
    ///
    /// A one-shot entry (`Once`, or `Times` on its last run) is removed
    /// outright. A recurring entry that still has deliveries left (`Times`
    /// with runs remaining, or `Forever`) is rotated to the *tail* of the
    /// queue instead of being left pinned at the head: this keeps the FIFO a
    /// true cycle, so a recurring prompt can never permanently block the
    /// entries queued behind it (functional starvation) or accumulate
    /// unreachable entries ahead of it (unbounded growth) -- every entry is
    /// either eventually consumed or eventually delivered again.
    pub fn acknowledge_queued_prompt(
        &mut self,
        id: NodeId,
        expected: &QueuedPrompt,
    ) -> Result<bool, TreeError> {
        let node = self.get_mut(id)?;
        let NodeKind::Pane { prompt_queue, .. } = &mut node.kind else {
            return Err(TreeError::NotAPane(id));
        };
        let Some(head) = prompt_queue.first() else {
            return Ok(false);
        };
        if head != expected {
            return Ok(false);
        }
        // Removed before deciding delivery so a rotation and a permanent
        // removal share the same single mutation site.
        let mut delivered = prompt_queue.remove(0);
        match &mut delivered.delivery {
            PromptQueueDelivery::Once => {}
            PromptQueueDelivery::Times { remaining_runs } if *remaining_runs > 1 => {
                *remaining_runs -= 1;
                prompt_queue.push(delivered);
            }
            PromptQueueDelivery::Times { .. } => {}
            PromptQueueDelivery::Forever => {
                prompt_queue.push(delivered);
            }
        }
        Ok(true)
    }

    /// Number of independently queued messages, including recurring entries.
    pub fn prompt_queue_len(&self, id: NodeId) -> Option<usize> {
        match &self.get(id)?.kind {
            NodeKind::Pane { prompt_queue, .. } => Some(prompt_queue.len()),
            NodeKind::Container(_) | NodeKind::Folder { .. } => None,
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
        if moving_node.is_project() && new_parent != ROOT_ID {
            return Err(TreeError::ProjectMustRemainTopLevel);
        }
        if new_parent == ROOT_ID && moving_node.is_pane() {
            return Err(TreeError::PanesRequireGroup);
        }
        if new_parent == ROOT_ID && !moving_node.is_project() {
            return Err(TreeError::RootRequiresProject);
        }
        if moving_node.is_split_view()
            && !matches!(&destination.kind, NodeKind::Container(container) if container.accepts_normal_children())
        {
            return Err(TreeError::SplitViewRequiresGroup);
        }
        if let (Some(from_project), Some(to_project)) =
            (self.project_ancestor(id), self.project_ancestor(new_parent))
        {
            if from_project != to_project {
                return Err(TreeError::CrossProjectMove {
                    node: id,
                    from_project,
                    to_project,
                });
            }
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
        let moving_node = self.get_mut(id)?;
        moving_node.parent = Some(new_parent);
        moving_node.structure_source = StructureSource::Manual;
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

    /// Returns a project's first ordinary group, creating `name` directly
    /// inside that project when needed. Terminal creation uses this default
    /// grouping while the project remains the only top-level owner.
    pub fn ensure_project_default_group(
        &mut self,
        project_id: NodeId,
        name: impl Into<String>,
    ) -> Result<NodeId, TreeError> {
        if !self.get(project_id).is_some_and(Node::is_project) {
            return Err(TreeError::NotAProject(project_id));
        }
        if let Some(group_id) = self
            .children_of(project_id)?
            .iter()
            .copied()
            .find(|child| self.get(*child).is_some_and(Node::is_group))
        {
            return Ok(group_id);
        }
        self.add_group(project_id, name)
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
            if self.get(child).is_some_and(Node::accepts_normal_children) {
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
                let pos_i64 = pos as i64;
                let delta_i64 = delta as i64;
                let len_i64 = container.children.len() as i64;
                let new_pos = (pos_i64 + delta_i64).clamp(0, len_i64 - 1) as usize;
                container.children.remove(pos);
                container.children.insert(new_pos, id);
                Ok(())
            }
            NodeKind::Pane { .. } | NodeKind::Folder { .. } => {
                unreachable!("parent of a node is always a group")
            }
        }
    }

    /// Moves a node one visible step in its tree ordering. Within a container
    /// it reorders siblings. At a nested container boundary, a pane exits into
    /// the parent group immediately before/after that container. At a
    /// top-level group boundary, it transfers into the nearest adjacent group,
    /// appending on an upward move and prepending on a downward move. This
    /// preserves the invariant that panes are never children of the root.
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

        if let Some((parent_group, index)) = self.pane_boundary_exit_target(parent, direction) {
            self.move_node(id, parent_group, Some(index))?;
            return Ok(true);
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

    /// Returns where a boundary pane lands when its current container is
    /// nested inside a normal group. Up places it immediately before the
    /// container; down places it immediately after. Top-level groups return
    /// `None` because a pane may never become a direct child of the root.
    fn pane_boundary_exit_target(
        &self,
        container: NodeId,
        direction: TreeMoveDirection,
    ) -> Option<(NodeId, usize)> {
        let parent_group = self.parent_of(container)?;
        if parent_group == ROOT_ID || !self.get(parent_group)?.accepts_normal_children() {
            return None;
        }
        let container_position = self
            .children_of(parent_group)
            .ok()?
            .iter()
            .position(|sibling| *sibling == container)?;
        let index = match direction {
            TreeMoveDirection::Up => container_position,
            TreeMoveDirection::Down => container_position + 1,
        };
        Some((parent_group, index))
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
            let mut found = None;
            match direction {
                TreeMoveDirection::Up => {
                    for candidate in siblings[..position].iter().rev() {
                        if self
                            .get(*candidate)
                            .is_some_and(Node::accepts_normal_children)
                        {
                            found = Some(*candidate);
                            break;
                        }
                    }
                }
                TreeMoveDirection::Down => {
                    for candidate in siblings[position + 1..].iter() {
                        if self
                            .get(*candidate)
                            .is_some_and(Node::accepts_normal_children)
                        {
                            found = Some(*candidate);
                            break;
                        }
                    }
                }
            }
            if let Some(group_id) = found {
                return Some(group_id);
            }
            if parent == ROOT_ID {
                return None;
            }
            current_group = parent;
        }
    }

    /// Validates the structural invariants every other method on this type
    /// assumes: the root is a parent-less group, every non-root node's
    /// `parent` names a container that lists it back exactly once, every
    /// node is reachable from the root (so parent-walking methods like
    /// [`Self::project_ancestor`] and [`Self::is_ancestor_of`] cannot loop
    /// forever on a cycle disconnected from the root), and `next_id` is
    /// past every id already in use (so the next [`Self::alloc_id`] cannot
    /// collide with, and silently overwrite, an existing node).
    ///
    /// This crate's own mutation methods (`add_pane`, `move_node`, ...)
    /// always keep these invariants true, so a `Tree` built purely through
    /// this crate's public API never needs this call. It exists for the
    /// one path that bypasses that API entirely: `Tree` derives
    /// `Deserialize` so `ilium-server` can load a crash-recovery snapshot
    /// wholesale, and JSON deserialized straight into private fields
    /// carries none of those invariants on its own. Callers on that path
    /// must call this immediately after deserializing and reject the
    /// snapshot on error rather than trust it.
    pub fn validate(&self) -> Result<(), TreeError> {
        let root = self
            .nodes
            .get(&ROOT_ID)
            .ok_or(TreeError::NodeNotFound(ROOT_ID))?;
        if root.parent.is_some() {
            return Err(TreeError::InvalidStructure(format!(
                "root {ROOT_ID:?} must have no parent"
            )));
        }
        if !root.is_group() {
            return Err(TreeError::InvalidStructure(format!(
                "root {ROOT_ID:?} must be a group"
            )));
        }

        // Breadth-first walk from the root through every container's
        // children. Discovering every node this way, rather than trusting
        // `self.nodes` directly, is what catches both a cycle (a node
        // whose ancestry never reaches the root) and an orphan (a node
        // present in the map but never listed as anyone's child): either
        // one is invisible to a check that only looks at individual nodes
        // in isolation.
        let mut visited = std::collections::HashSet::new();
        visited.insert(ROOT_ID);
        let mut frontier = vec![ROOT_ID];
        while let Some(current_id) = frontier.pop() {
            let current = self
                .nodes
                .get(&current_id)
                .ok_or(TreeError::NodeNotFound(current_id))?;
            let NodeKind::Container(container) = &current.kind else {
                continue;
            };
            let mut children_of_current = std::collections::HashSet::new();
            for &child_id in &container.children {
                if child_id == current_id || !children_of_current.insert(child_id) {
                    return Err(TreeError::InvalidStructure(format!(
                        "node {current_id:?} lists child {child_id:?} more than once"
                    )));
                }
                let child = self
                    .nodes
                    .get(&child_id)
                    .ok_or(TreeError::NodeNotFound(child_id))?;
                if child.parent != Some(current_id) {
                    return Err(TreeError::InvalidStructure(format!(
                        "node {child_id:?}'s parent does not match its listing under {current_id:?}"
                    )));
                }
                if !visited.insert(child_id) {
                    return Err(TreeError::InvalidStructure(format!(
                        "node {child_id:?} is listed as a child of more than one parent"
                    )));
                }
                frontier.push(child_id);
            }
        }
        if visited.len() != self.nodes.len() {
            return Err(TreeError::InvalidStructure(
                "tree contains node(s) unreachable from the root".to_string(),
            ));
        }

        let max_existing_id = self.nodes.keys().map(|id| id.0).max().unwrap_or(0);
        if self.next_id <= max_existing_id {
            return Err(TreeError::InvalidStructure(format!(
                "next_id {} would collide with existing node id {max_existing_id}",
                self.next_id
            )));
        }

        Ok(())
    }
}

fn project_display_name(path: &std::path::Path) -> String {
    path.file_name()
        .filter(|name| !name.is_empty())
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
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
    fn projects_are_top_level_and_keep_entries_with_their_own_directory() {
        let mut tree = Tree::new();
        let first_project = tree.add_project(PathBuf::from("/tmp/first")).unwrap();
        let second_project = tree.add_project(PathBuf::from("/tmp/second")).unwrap();
        let first_group = tree.add_group(first_project, "work").unwrap();
        let pane = tree
            .add_pane(first_group, "shell", PaneContentKind::Terminal)
            .unwrap();

        assert_eq!(
            tree.project_path_for(pane),
            Some(std::path::Path::new("/tmp/first"))
        );
        assert!(matches!(
            tree.move_node(pane, second_project, None),
            Err(TreeError::CrossProjectMove { .. })
        ));
        assert!(matches!(
            tree.move_node(first_group, ROOT_ID, None),
            Err(TreeError::RootRequiresProject)
        ));
        tree.change_project_folder(first_project, PathBuf::from("/tmp/renamed"))
            .unwrap();
        assert_eq!(
            tree.project_path_for(pane),
            Some(std::path::Path::new("/tmp/renamed"))
        );
    }

    #[test]
    fn ensuring_the_launch_project_preserves_a_changed_project_folder() {
        let mut tree = Tree::new();
        let project = tree.add_project(PathBuf::from("/tmp/launch")).unwrap();
        tree.change_project_folder(project, PathBuf::from("/tmp/relocated"))
            .unwrap();

        assert_eq!(
            tree.ensure_launch_project(PathBuf::from("/tmp/launch"))
                .unwrap(),
            project
        );
        assert_eq!(tree.project_ids(), vec![project]);
        assert_eq!(
            tree.project_path_for(project),
            Some(std::path::Path::new("/tmp/relocated"))
        );
    }

    #[test]
    fn project_default_group_is_created_once_inside_its_project() {
        let mut tree = Tree::new();
        let project = tree.add_project(PathBuf::from("/tmp/project")).unwrap();
        let group = tree
            .ensure_project_default_group(project, "default")
            .unwrap();

        assert_eq!(tree.parent_of(group), Some(project));
        assert_eq!(
            tree.ensure_project_default_group(project, "ignored")
                .unwrap(),
            group
        );
    }

    #[test]
    fn project_restructure_changes_only_its_own_subtree_and_can_be_restored() {
        let mut tree = Tree::new();
        let first_project = tree.add_project(PathBuf::from("/tmp/first")).unwrap();
        let second_project = tree.add_project(PathBuf::from("/tmp/second")).unwrap();
        let first_group = tree.add_group(first_project, "first").unwrap();
        let second_group = tree.add_group(second_project, "second").unwrap();
        let first_pane = tree
            .add_pane(first_group, "one", PaneContentKind::Terminal)
            .unwrap();
        let second_pane = tree
            .add_pane(second_group, "two", PaneContentKind::Terminal)
            .unwrap();
        let before = tree.clone();

        tree.apply_project_restructure(
            first_project,
            RestructurePlan {
                children: vec![RestructureNode::Group {
                    title: "Reorganized".to_string(),
                    short_title: None,
                    icon: None,
                    children: vec![RestructureNode::Pane {
                        id: first_pane,
                        title: "renamed one".to_string(),
                        short_title: None,
                        icon: None,
                    }],
                }],
            },
        )
        .unwrap();

        assert_eq!(tree.project_ancestor(second_pane), Some(second_project));
        assert_eq!(tree.get(second_pane).unwrap().name, "two");
        tree.restore_project_from(first_project, &before).unwrap();
        assert_eq!(tree.get(first_pane).unwrap().name, "one");
        assert_eq!(tree.get(second_pane).unwrap().name, "two");
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
    fn pane_arrow_exits_a_nested_group_at_its_boundaries() {
        let mut tree = Tree::new();
        let outer = tree.add_group(ROOT_ID, "outer").unwrap();
        let before = tree
            .add_pane(outer, "before", PaneContentKind::Terminal)
            .unwrap();
        let nested = tree.add_group(outer, "nested").unwrap();
        let first = tree
            .add_pane(nested, "first", PaneContentKind::Terminal)
            .unwrap();
        let last = tree
            .add_pane(nested, "last", PaneContentKind::Terminal)
            .unwrap();
        let after = tree
            .add_pane(outer, "after", PaneContentKind::Terminal)
            .unwrap();

        assert!(tree
            .move_node_one_step(first, TreeMoveDirection::Up)
            .unwrap());
        assert_eq!(tree.parent_of(first), Some(outer));
        assert_eq!(
            tree.children_of(outer).unwrap(),
            &[before, first, nested, after]
        );
        assert_eq!(tree.children_of(nested).unwrap(), &[last]);

        assert!(tree
            .move_node_one_step(last, TreeMoveDirection::Down)
            .unwrap());
        assert_eq!(tree.parent_of(last), Some(outer));
        assert_eq!(
            tree.children_of(outer).unwrap(),
            &[before, first, nested, last, after]
        );
        assert_eq!(tree.children_of(nested).unwrap(), &[]);
    }

    #[test]
    fn pane_arrow_exits_a_split_view_at_its_boundaries() {
        let mut tree = Tree::new();
        let group = tree.add_group(ROOT_ID, "group").unwrap();
        let before = tree
            .add_pane(group, "before", PaneContentKind::Terminal)
            .unwrap();
        let first = tree
            .add_pane(group, "first", PaneContentKind::Terminal)
            .unwrap();
        let last = tree
            .add_pane(group, "last", PaneContentKind::Terminal)
            .unwrap();
        let split = tree
            .create_split_view(group, "split", SplitOrientation::Vertical, &[first, last])
            .unwrap();
        let after = tree
            .add_pane(group, "after", PaneContentKind::Terminal)
            .unwrap();

        assert!(tree
            .move_node_one_step(first, TreeMoveDirection::Up)
            .unwrap());
        assert_eq!(tree.parent_of(first), Some(group));
        assert_eq!(
            tree.children_of(group).unwrap(),
            &[before, first, split, after]
        );
        assert_eq!(tree.children_of(split).unwrap(), &[last]);

        assert!(tree
            .move_node_one_step(last, TreeMoveDirection::Down)
            .unwrap());
        assert_eq!(tree.parent_of(last), Some(group));
        assert_eq!(
            tree.children_of(group).unwrap(),
            &[before, first, split, last, after]
        );
        assert_eq!(tree.children_of(split).unwrap(), &[]);
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
    fn add_board_rejects_a_second_pane_for_the_same_storage_path() {
        let mut tree = Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        let path = PathBuf::from("/tmp/shared-board.md");
        tree.add_board(
            group,
            "First".to_string(),
            BoardStorage::MarkdownFile { path: path.clone() },
        )
        .unwrap();

        let error = tree
            .add_board(
                group,
                "Second".to_string(),
                BoardStorage::MarkdownFile { path: path.clone() },
            )
            .unwrap_err();

        assert!(matches!(
            error,
            TreeError::BoardStorageAlreadyOpen(existing_path) if existing_path == path
        ));
        assert_eq!(tree.panes().count(), 1);
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
        tree.rename_node(group, "renamed", None, None).unwrap();
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
    fn queued_prompts_are_fifo_and_advance_only_after_acknowledged_delivery() {
        let mut tree = Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        let terminal = tree
            .add_pane(group, "agent", PaneContentKind::Terminal)
            .unwrap();
        let once = QueuedPrompt {
            text: "first".to_string(),
            delivery: PromptQueueDelivery::Once,
        };
        let repeat = QueuedPrompt {
            text: "second".to_string(),
            delivery: PromptQueueDelivery::Times { remaining_runs: 2 },
        };
        tree.enqueue_prompt(terminal, once.clone()).unwrap();
        tree.enqueue_prompt(terminal, repeat.clone()).unwrap();
        assert_eq!(tree.prompt_queue_len(terminal), Some(2));
        assert_eq!(tree.next_queued_prompt(terminal).unwrap(), Some(&once));
        assert!(!tree.acknowledge_queued_prompt(terminal, &repeat).unwrap());
        assert!(tree.acknowledge_queued_prompt(terminal, &once).unwrap());
        assert_eq!(tree.next_queued_prompt(terminal).unwrap(), Some(&repeat));
        assert!(tree.acknowledge_queued_prompt(terminal, &repeat).unwrap());
        assert_eq!(tree.prompt_queue_len(terminal), Some(1));
        assert!(matches!(
            tree.next_queued_prompt(terminal).unwrap(),
            Some(QueuedPrompt {
                delivery: PromptQueueDelivery::Times { remaining_runs: 1 },
                ..
            })
        ));
        let final_repeat = tree.next_queued_prompt(terminal).unwrap().cloned().unwrap();
        assert!(tree
            .acknowledge_queued_prompt(terminal, &final_repeat)
            .unwrap());
        assert_eq!(tree.prompt_queue_len(terminal), Some(0));
    }

    /// Regression test for a leak/starvation bug: a `Forever` entry ahead of
    /// other queued prompts used to stay pinned at index 0 forever, so
    /// everything queued behind it was permanently unreachable and the queue
    /// grew without bound as more prompts were enqueued. Acknowledging a
    /// recurring entry must rotate it to the tail instead, so the queue stays
    /// a bounded cycle and every one-shot entry behind it still drains.
    #[test]
    fn recurring_prompts_rotate_to_the_tail_instead_of_starving_the_queue() {
        let mut tree = Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        let terminal = tree
            .add_pane(group, "agent", PaneContentKind::Terminal)
            .unwrap();
        let forever = QueuedPrompt {
            text: "reminder".to_string(),
            delivery: PromptQueueDelivery::Forever,
        };
        let once_a = QueuedPrompt {
            text: "one-shot-a".to_string(),
            delivery: PromptQueueDelivery::Once,
        };
        let once_b = QueuedPrompt {
            text: "one-shot-b".to_string(),
            delivery: PromptQueueDelivery::Once,
        };
        tree.enqueue_prompt(terminal, forever.clone()).unwrap();
        tree.enqueue_prompt(terminal, once_a.clone()).unwrap();
        tree.enqueue_prompt(terminal, once_b.clone()).unwrap();
        assert_eq!(tree.prompt_queue_len(terminal), Some(3));

        // First completion delivers the Forever prompt; it rotates to the
        // tail rather than staying at the head, so the queue length is
        // unchanged and the one-shots are now reachable.
        assert_eq!(tree.next_queued_prompt(terminal).unwrap(), Some(&forever));
        assert!(tree.acknowledge_queued_prompt(terminal, &forever).unwrap());
        assert_eq!(tree.prompt_queue_len(terminal), Some(3));
        assert_eq!(tree.next_queued_prompt(terminal).unwrap(), Some(&once_a));

        // Both one-shots drain permanently.
        assert!(tree.acknowledge_queued_prompt(terminal, &once_a).unwrap());
        assert_eq!(tree.next_queued_prompt(terminal).unwrap(), Some(&once_b));
        assert!(tree.acknowledge_queued_prompt(terminal, &once_b).unwrap());
        assert_eq!(tree.prompt_queue_len(terminal), Some(1));

        // Only the Forever entry remains, and it keeps cycling without ever
        // growing the queue on repeated completions.
        assert_eq!(tree.next_queued_prompt(terminal).unwrap(), Some(&forever));
        for _ in 0..5 {
            assert!(tree.acknowledge_queued_prompt(terminal, &forever).unwrap());
            assert_eq!(tree.prompt_queue_len(terminal), Some(1));
        }
    }

    #[test]
    fn enqueue_prompt_rejects_growth_past_the_bounded_maximum() {
        let mut tree = Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        let terminal = tree
            .add_pane(group, "agent", PaneContentKind::Terminal)
            .unwrap();
        for index in 0..MAXIMUM_PROMPT_QUEUE_LEN {
            tree.enqueue_prompt(
                terminal,
                QueuedPrompt {
                    text: format!("prompt-{index}"),
                    delivery: PromptQueueDelivery::Once,
                },
            )
            .unwrap();
        }
        assert_eq!(
            tree.prompt_queue_len(terminal),
            Some(MAXIMUM_PROMPT_QUEUE_LEN)
        );
        let overflow = QueuedPrompt {
            text: "one-too-many".to_string(),
            delivery: PromptQueueDelivery::Once,
        };
        assert!(matches!(
            tree.enqueue_prompt(terminal, overflow),
            Err(TreeError::PromptQueueFull(id, MAXIMUM_PROMPT_QUEUE_LEN)) if id == terminal
        ));
        assert_eq!(
            tree.prompt_queue_len(terminal),
            Some(MAXIMUM_PROMPT_QUEUE_LEN)
        );
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

    #[test]
    fn provider_registry_owns_antigravity_launch_resume_and_argument_syntax() {
        let session_id = "a1111111-1111-4111-8111-111111111111";
        let provider = BuiltinAgentProvider::Antigravity;

        assert_eq!(provider.class(), AgentClass::Antigravity);
        assert_eq!(provider.command_line(), "agy");
        assert_eq!(
            provider.resume_command(session_id),
            format!("agy --conversation {session_id}")
        );
        assert!(provider.process_name_substrings().contains(&"antimatter"));
        assert_eq!(
            provider.session_id_from_arguments(&[
                "/home/developer/.local/bin/agy".to_string(),
                "--conversation".to_string(),
                session_id.to_string(),
            ]),
            Some(session_id.to_string())
        );
        assert_eq!(
            BuiltinAgentProvider::resume_binding(&format!("agy --conversation {session_id}")),
            Some((provider, session_id.to_string()))
        );
    }

    #[test]
    fn provider_registry_owns_interactive_session_transition_syntax() {
        for provider in BuiltinAgentProvider::ALL {
            for command in ["/resume", "/resume another", "/branch release", "/new"] {
                assert!(provider.invalidates_session_identity(command));
                assert_eq!(
                    provider.session_identity_transition_rule(command),
                    Some(SessionIdentityTransitionRule::SharedNewSessionCommand)
                );
            }
            for input in ["please run /resume", "/fork investigate", "resume"] {
                assert!(!provider.invalidates_session_identity(input));
            }
        }

        for provider in [BuiltinAgentProvider::Claude, BuiltinAgentProvider::Codex] {
            assert!(provider.invalidates_session_identity("  /clear  "));
            assert_eq!(
                provider.session_identity_transition_rule("  /clear  "),
                Some(SessionIdentityTransitionRule::ClaudeOrCodexClearStartsFreshSession)
            );
        }
        assert!(!BuiltinAgentProvider::Codex.invalidates_session_identity("/clear later"));
        assert!(!BuiltinAgentProvider::Antigravity.invalidates_session_identity("/clear"));
    }

    #[test]
    fn fresh_agent_session_reset_discards_old_title_and_grouping() {
        let mut tree = Tree::new();
        let project = tree
            .ensure_launch_project(PathBuf::from("/tmp/project"))
            .unwrap();
        let direct_pane = tree
            .add_pane(project, "Keep me first", PaneContentKind::Terminal)
            .unwrap();
        let group = tree.add_group(project, "Old work").unwrap();
        let pane_id = tree
            .add_pane(group, "Old inferred title", PaneContentKind::Terminal)
            .unwrap();
        let split_peer = tree
            .add_pane(group, "Split peer", PaneContentKind::Terminal)
            .unwrap();
        let split = tree
            .create_split_view(
                group,
                "Old split",
                SplitOrientation::Vertical,
                &[pane_id, split_peer],
            )
            .unwrap();
        tree.rename_node(
            pane_id,
            "User title for old work",
            Some("Old work".to_string()),
            Some("📜".to_string()),
        )
        .unwrap();

        assert!(tree
            .reset_terminal_pane_for_fresh_conversation(pane_id, "<new>")
            .unwrap());
        let pane = tree.get(pane_id).unwrap();
        assert_eq!(pane.name, "<new>");
        assert_eq!(pane.short_name, None);
        assert_eq!(pane.inferred_icon, None);
        assert_eq!(pane.parent, Some(project));
        assert_eq!(
            tree.children_of(project).unwrap(),
            &[direct_pane, group, pane_id]
        );
        assert_eq!(tree.children_of(split).unwrap(), &[split_peer]);
        let NodeKind::Pane { title_source, .. } = &pane.kind else {
            panic!("expected terminal pane");
        };
        assert_eq!(*title_source, PaneTitleSource::Automatic);
        assert!(!tree
            .reset_terminal_pane_for_fresh_conversation(pane_id, "<new>")
            .unwrap());
    }

    #[test]
    fn fresh_agent_session_reset_is_atomic_without_an_owning_project() {
        let mut tree = Tree::new();
        let legacy_group = tree.add_group(ROOT_ID, "Legacy work").unwrap();
        let pane_id = tree
            .add_pane(
                legacy_group,
                "Title that must survive rejection",
                PaneContentKind::Terminal,
            )
            .unwrap();
        let before = tree.clone();

        assert!(matches!(
            tree.reset_terminal_pane_for_fresh_conversation(pane_id, "<new>"),
            Err(TreeError::NodeOutsideProject(changed_id)) if changed_id == pane_id
        ));
        assert_eq!(tree, before);
    }

    #[test]
    fn completion_acknowledgement_only_changes_done_agents_and_preserves_goal_ownership() {
        let mut tree = Tree::new();
        let project = tree
            .ensure_launch_project(PathBuf::from("/tmp/project"))
            .unwrap();
        let ordinary_pane = tree
            .add_pane(project, "ordinary", PaneContentKind::Terminal)
            .unwrap();
        let goal_pane = tree
            .add_pane(project, "goal", PaneContentKind::Terminal)
            .unwrap();
        let working_pane = tree
            .add_pane(project, "working", PaneContentKind::Terminal)
            .unwrap();
        tree.set_pane_status(
            ordinary_pane,
            PaneStatus::Agent(AgentClass::Codex, AgentActivity::Done),
        )
        .unwrap();
        tree.set_pane_status(
            goal_pane,
            PaneStatus::AgentWithGoal(AgentClass::Claude, AgentActivity::Done),
        )
        .unwrap();
        tree.set_pane_status(
            working_pane,
            PaneStatus::Agent(AgentClass::Codex, AgentActivity::Working),
        )
        .unwrap();

        assert_eq!(
            tree.acknowledge_agent_completion(ordinary_pane).unwrap(),
            Some(PaneStatus::Agent(AgentClass::Codex, AgentActivity::Idle))
        );
        assert_eq!(
            tree.acknowledge_agent_completion(goal_pane).unwrap(),
            Some(PaneStatus::AgentWithGoal(
                AgentClass::Claude,
                AgentActivity::Idle
            ))
        );
        assert_eq!(
            tree.acknowledge_agent_completion(working_pane).unwrap(),
            None,
            "a stale input acknowledgement must not overwrite renewed work"
        );
        assert!(matches!(
            &tree.get(goal_pane).unwrap().kind,
            NodeKind::Pane {
                status: PaneStatus::AgentWithGoal(AgentClass::Claude, AgentActivity::Idle),
                ..
            }
        ));
    }

    #[test]
    fn apply_restructure_regroups_and_retitles_in_one_shot() {
        let mut tree = Tree::new();
        let flat_group = tree.add_group(ROOT_ID, "misc").unwrap();
        let pane_a = tree
            .add_pane(flat_group, "shell-a", PaneContentKind::Terminal)
            .unwrap();
        let pane_b = tree
            .add_pane(flat_group, "shell-b", PaneContentKind::Terminal)
            .unwrap();

        let plan = RestructurePlan {
            children: vec![RestructureNode::Group {
                title: "Auth refactor".to_string(),
                short_title: Some("Auth".to_string()),
                icon: None,
                children: vec![
                    RestructureNode::Pane {
                        id: pane_a,
                        title: "Backend agent".to_string(),
                        short_title: None,
                        icon: None,
                    },
                    RestructureNode::Pane {
                        id: pane_b,
                        title: "Frontend shell".to_string(),
                        short_title: None,
                        icon: None,
                    },
                ],
            }],
        };
        tree.apply_restructure(plan).unwrap();

        assert!(tree.get(flat_group).is_none(), "old group id is discarded");
        let new_group = tree.children_of(ROOT_ID).unwrap()[0];
        assert_eq!(tree.get(new_group).unwrap().name, "Auth refactor");
        assert_eq!(
            tree.get(new_group).unwrap().structure_source,
            StructureSource::LlmRestructure
        );
        assert_eq!(tree.children_of(new_group).unwrap(), &[pane_a, pane_b]);
        assert_eq!(tree.get(pane_a).unwrap().name, "Backend agent");
        assert_eq!(tree.parent_of(pane_a), Some(new_group));
        assert_eq!(
            tree.get(pane_a).unwrap().structure_source,
            StructureSource::LlmRestructure
        );
        let NodeKind::Pane { title_source, .. } = &tree.get(pane_a).unwrap().kind else {
            panic!("expected a pane");
        };
        assert_eq!(*title_source, PaneTitleSource::UserSpecified);

        tree.rename_node(pane_a, "User refined backend task", None, None)
            .unwrap();
        assert_eq!(
            tree.get(pane_a).unwrap().structure_source,
            StructureSource::Manual
        );
    }

    #[test]
    fn apply_restructure_preserves_split_views_and_folders() {
        let mut tree = Tree::new();
        let project = tree
            .add_project(PathBuf::from("/tmp/protected-split"))
            .unwrap();
        let group = tree.add_group(project, "work").unwrap();
        let pane_a = tree
            .add_pane(group, "a", PaneContentKind::Terminal)
            .unwrap();
        let pane_b = tree
            .add_pane(group, "b", PaneContentKind::Terminal)
            .unwrap();
        let folder = tree
            .add_folder(group, PathBuf::from("/tmp/project"))
            .unwrap();
        let split_view = tree
            .create_split_view(
                group,
                "User Split",
                SplitOrientation::Horizontal,
                &[pane_a, pane_b],
            )
            .unwrap();
        tree.rename_node(
            split_view,
            "Pinned User Split",
            Some("Pinned Split".to_string()),
            Some("🪟".to_string()),
        )
        .unwrap();
        tree.toggle_expanded(split_view).unwrap();

        let plan = RestructurePlan {
            children: vec![RestructureNode::Group {
                title: "AI regrouped work".to_string(),
                short_title: None,
                icon: None,
                children: vec![
                    RestructureNode::ExistingSplitView {
                        id: split_view,
                        children: vec![
                            RestructureNode::Pane {
                                id: pane_a,
                                title: "a2".to_string(),
                                short_title: None,
                                icon: None,
                            },
                            RestructureNode::Pane {
                                id: pane_b,
                                title: "b2".to_string(),
                                short_title: None,
                                icon: None,
                            },
                        ],
                    },
                    RestructureNode::Folder {
                        id: folder,
                        title: "Project root".to_string(),
                        short_title: None,
                        icon: None,
                    },
                ],
            }],
        };
        tree.apply_project_restructure(project, plan).unwrap();

        assert!(tree.get(group).is_none());
        let new_group = tree.children_of(project).unwrap()[0];
        assert_eq!(tree.children_of(new_group).unwrap(), &[split_view, folder]);
        let preserved_split = tree.get(split_view).unwrap();
        assert_eq!(preserved_split.name, "Pinned User Split");
        assert_eq!(preserved_split.short_name.as_deref(), Some("Pinned Split"));
        assert_eq!(preserved_split.inferred_icon.as_deref(), Some("🪟"));
        assert_eq!(preserved_split.structure_source, StructureSource::Manual);
        let NodeKind::Container(preserved_container) = &preserved_split.kind else {
            panic!("protected split should remain a container");
        };
        assert!(!preserved_container.expanded);
        assert_eq!(
            tree.split_orientation(split_view),
            Some(SplitOrientation::Horizontal)
        );
        assert_eq!(tree.children_of(split_view).unwrap(), &[pane_a, pane_b]);
        assert_eq!(tree.get(pane_a).unwrap().name, "a2");
        assert_eq!(tree.get(pane_b).unwrap().name, "b2");
        assert_eq!(tree.get(folder).unwrap().name, "Project root");
    }

    #[test]
    fn apply_restructure_rejects_dissolving_an_existing_split_view() {
        let mut tree = Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        let pane_a = tree
            .add_pane(group, "a", PaneContentKind::Terminal)
            .unwrap();
        let pane_b = tree
            .add_pane(group, "b", PaneContentKind::Terminal)
            .unwrap();
        let split_view = tree
            .create_split_view(
                group,
                "User Split",
                SplitOrientation::Vertical,
                &[pane_a, pane_b],
            )
            .unwrap();
        let before = tree.clone();

        let plan = RestructurePlan {
            children: vec![
                RestructureNode::Pane {
                    id: pane_a,
                    title: "a2".to_string(),
                    short_title: None,
                    icon: None,
                },
                RestructureNode::Pane {
                    id: pane_b,
                    title: "b2".to_string(),
                    short_title: None,
                    icon: None,
                },
            ],
        };
        let error = tree.apply_restructure(plan).unwrap_err();

        assert!(matches!(
            error,
            TreeError::RestructureSplitViewSetMismatch {
                expected,
                actual
            } if expected == vec![split_view] && actual.is_empty()
        ));
        assert_eq!(tree, before);
    }

    #[test]
    fn apply_restructure_rejects_reordering_or_replacing_split_members() {
        let mut tree = Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        let pane_a = tree
            .add_pane(group, "a", PaneContentKind::Terminal)
            .unwrap();
        let pane_b = tree
            .add_pane(group, "b", PaneContentKind::Terminal)
            .unwrap();
        let split_view = tree
            .create_split_view(
                group,
                "User Split",
                SplitOrientation::Vertical,
                &[pane_a, pane_b],
            )
            .unwrap();
        let before = tree.clone();

        let plan = RestructurePlan {
            children: vec![RestructureNode::ExistingSplitView {
                id: split_view,
                children: vec![
                    RestructureNode::Pane {
                        id: pane_b,
                        title: "b2".to_string(),
                        short_title: None,
                        icon: None,
                    },
                    RestructureNode::Pane {
                        id: pane_a,
                        title: "a2".to_string(),
                        short_title: None,
                        icon: None,
                    },
                ],
            }],
        };
        let error = tree.apply_restructure(plan).unwrap_err();

        assert!(matches!(
            error,
            TreeError::RestructureSplitViewChildrenChanged {
                split_view: changed_split,
                expected,
                actual
            } if changed_split == split_view
                && expected == vec![pane_a, pane_b]
                && actual == vec![pane_b, pane_a]
        ));
        assert_eq!(tree, before);
    }

    #[test]
    fn apply_restructure_rejects_omitting_an_empty_split_view() {
        let mut tree = Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        let pane = tree
            .add_pane(group, "shell", PaneContentKind::Terminal)
            .unwrap();
        let split_view = tree
            .create_split_view(group, "Empty Split", SplitOrientation::Vertical, &[])
            .unwrap();
        let before = tree.clone();

        let plan = RestructurePlan {
            children: vec![RestructureNode::Pane {
                id: pane,
                title: "renamed shell".to_string(),
                short_title: None,
                icon: None,
            }],
        };
        let error = tree.apply_restructure(plan).unwrap_err();

        assert!(matches!(
            error,
            TreeError::RestructureSplitViewSetMismatch {
                expected,
                actual
            } if expected == vec![split_view] && actual.is_empty()
        ));
        assert_eq!(tree, before);
    }

    #[test]
    fn apply_restructure_rejects_an_invented_split_view() {
        let mut tree = Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        let pane = tree
            .add_pane(group, "shell", PaneContentKind::Terminal)
            .unwrap();
        let before = tree.clone();

        let plan = RestructurePlan {
            children: vec![RestructureNode::ExistingSplitView {
                id: NodeId(9999),
                children: vec![RestructureNode::Pane {
                    id: pane,
                    title: "renamed shell".to_string(),
                    short_title: None,
                    icon: None,
                }],
            }],
        };
        let error = tree.apply_restructure(plan).unwrap_err();

        assert!(matches!(error, TreeError::NodeNotFound(NodeId(9999))));
        assert_eq!(tree, before);
    }

    #[test]
    fn apply_restructure_rejects_a_duplicated_split_reference() {
        let mut tree = Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        let pane = tree
            .add_pane(group, "shell", PaneContentKind::Terminal)
            .unwrap();
        let split_view = tree
            .create_split_view(group, "User Split", SplitOrientation::Vertical, &[pane])
            .unwrap();
        let before = tree.clone();
        let split_plan = || RestructureNode::ExistingSplitView {
            id: split_view,
            children: vec![RestructureNode::Pane {
                id: pane,
                title: "renamed shell".to_string(),
                short_title: None,
                icon: None,
            }],
        };

        let error = tree
            .apply_restructure(RestructurePlan {
                children: vec![split_plan(), split_plan()],
            })
            .unwrap_err();

        assert!(matches!(
            error,
            TreeError::RestructureDuplicateSplitView(id) if id == split_view
        ));
        assert_eq!(tree, before);
    }

    #[test]
    fn project_restructure_rejects_a_split_from_another_project() {
        let mut tree = Tree::new();
        let project_a = tree.add_project(PathBuf::from("/tmp/project-a")).unwrap();
        let group_a = tree.add_group(project_a, "a").unwrap();
        let pane_a = tree
            .add_pane(group_a, "a pane", PaneContentKind::Terminal)
            .unwrap();
        let project_b = tree.add_project(PathBuf::from("/tmp/project-b")).unwrap();
        let group_b = tree.add_group(project_b, "b").unwrap();
        let split_b = tree
            .create_split_view(group_b, "B Split", SplitOrientation::Horizontal, &[])
            .unwrap();
        let before = tree.clone();

        let error = tree
            .apply_project_restructure(
                project_a,
                RestructurePlan {
                    children: vec![
                        RestructureNode::ExistingSplitView {
                            id: split_b,
                            children: Vec::new(),
                        },
                        RestructureNode::Pane {
                            id: pane_a,
                            title: "renamed a pane".to_string(),
                            short_title: None,
                            icon: None,
                        },
                    ],
                },
            )
            .unwrap_err();

        assert!(matches!(
            error,
            TreeError::RestructureSplitViewOutsideProject {
                split_view,
                project
            } if split_view == split_b && project == project_a
        ));
        assert_eq!(tree, before);
    }

    #[test]
    fn apply_restructure_rejects_a_missing_leaf_and_leaves_tree_untouched() {
        let mut tree = Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        let pane_a = tree
            .add_pane(group, "a", PaneContentKind::Terminal)
            .unwrap();
        let _pane_b = tree
            .add_pane(group, "b", PaneContentKind::Terminal)
            .unwrap();
        let before = tree.clone();

        let plan = RestructurePlan {
            children: vec![RestructureNode::Pane {
                id: pane_a,
                title: "only a".to_string(),
                short_title: None,
                icon: None,
            }],
        };
        let err = tree.apply_restructure(plan).unwrap_err();

        assert!(matches!(
            err,
            TreeError::RestructureLeafSetMismatch {
                expected: 2,
                actual: 1
            }
        ));
        assert_eq!(tree, before);
    }

    #[test]
    fn apply_restructure_rejects_a_duplicated_leaf() {
        let mut tree = Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        let pane_a = tree
            .add_pane(group, "a", PaneContentKind::Terminal)
            .unwrap();
        let before = tree.clone();

        let plan = RestructurePlan {
            children: vec![
                RestructureNode::Pane {
                    id: pane_a,
                    title: "a1".to_string(),
                    short_title: None,
                    icon: None,
                },
                RestructureNode::Pane {
                    id: pane_a,
                    title: "a2".to_string(),
                    short_title: None,
                    icon: None,
                },
            ],
        };
        let err = tree.apply_restructure(plan).unwrap_err();

        assert!(matches!(err, TreeError::RestructureDuplicateLeaf(id) if id == pane_a));
        assert_eq!(tree, before);
    }

    #[test]
    fn apply_restructure_rejects_an_invented_id() {
        let mut tree = Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        let _pane_a = tree
            .add_pane(group, "a", PaneContentKind::Terminal)
            .unwrap();
        let before = tree.clone();

        let plan = RestructurePlan {
            children: vec![RestructureNode::Pane {
                id: NodeId(9999),
                title: "ghost".to_string(),
                short_title: None,
                icon: None,
            }],
        };
        let err = tree.apply_restructure(plan).unwrap_err();

        assert!(matches!(err, TreeError::NodeNotFound(NodeId(9999))));
        assert_eq!(tree, before);
    }

    #[test]
    fn apply_restructure_rejects_a_group_id_referenced_as_a_pane() {
        let mut tree = Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        let before = tree.clone();

        let plan = RestructurePlan {
            children: vec![RestructureNode::Pane {
                id: group,
                title: "not a pane".to_string(),
                short_title: None,
                icon: None,
            }],
        };
        let err = tree.apply_restructure(plan).unwrap_err();

        assert!(matches!(err, TreeError::NotAPane(id) if id == group));
        assert_eq!(tree, before);
    }

    #[test]
    fn apply_restructure_rejects_adding_a_pane_to_an_existing_split_view() {
        let mut tree = Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        let panes: Vec<NodeId> = (0..5)
            .map(|index| {
                tree.add_pane(group, format!("p{index}"), PaneContentKind::Terminal)
                    .unwrap()
            })
            .collect();
        let split_view = tree
            .create_split_view(group, "User Split", SplitOrientation::Vertical, &panes[..4])
            .unwrap();
        let before = tree.clone();

        let plan = RestructurePlan {
            children: vec![RestructureNode::ExistingSplitView {
                id: split_view,
                children: panes
                    .into_iter()
                    .map(|id| RestructureNode::Pane {
                        id,
                        title: "p".to_string(),
                        short_title: None,
                        icon: None,
                    })
                    .collect(),
            }],
        };
        let err = tree.apply_restructure(plan).unwrap_err();

        assert!(matches!(
            err,
            TreeError::RestructureSplitViewChildrenChanged {
                split_view: changed_split,
                ..
            } if changed_split == split_view
        ));
        assert_eq!(tree, before);
    }

    #[test]
    fn apply_restructure_rejects_a_non_pane_child_of_a_split_view() {
        let mut tree = Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        let pane_a = tree
            .add_pane(group, "a", PaneContentKind::Terminal)
            .unwrap();
        let split_view = tree
            .create_split_view(group, "User Split", SplitOrientation::Vertical, &[pane_a])
            .unwrap();
        let before = tree.clone();

        let plan = RestructurePlan {
            children: vec![RestructureNode::ExistingSplitView {
                id: split_view,
                children: vec![RestructureNode::Group {
                    title: "nested".to_string(),
                    short_title: None,
                    icon: None,
                    children: vec![RestructureNode::Pane {
                        id: pane_a,
                        title: "a".to_string(),
                        short_title: None,
                        icon: None,
                    }],
                }],
            }],
        };
        let err = tree.apply_restructure(plan).unwrap_err();

        assert!(matches!(err, TreeError::SplitViewOnlyAcceptsPanes));
        assert_eq!(tree, before);
    }

    #[test]
    fn group_navigation_keeps_nested_groups_separate_and_includes_split_members() {
        let mut tree = Tree::new();
        let project = tree.add_project(PathBuf::from("/tmp/navigation")).unwrap();
        let group = tree.add_group(project, "work").unwrap();
        let direct = tree
            .add_pane(group, "direct", PaneContentKind::Terminal)
            .unwrap();
        let first_split_member = tree
            .add_pane(group, "first split", PaneContentKind::Terminal)
            .unwrap();
        let second_split_member = tree
            .add_pane(group, "second split", PaneContentKind::Editor)
            .unwrap();
        let split = tree
            .create_split_view(
                group,
                "split",
                SplitOrientation::Vertical,
                &[first_split_member, second_split_member],
            )
            .unwrap();
        let nested_group = tree.add_group(group, "nested").unwrap();
        let nested_pane = tree
            .add_pane(nested_group, "nested pane", PaneContentKind::Board)
            .unwrap();

        assert_eq!(tree.containing_group(second_split_member), Some(group));
        assert_eq!(tree.containing_group(nested_pane), Some(nested_group));
        assert_eq!(
            tree.navigable_panes_in_group(group),
            vec![direct, first_split_member, second_split_member]
        );
        assert_eq!(tree.group_ids_in_tree_order(), vec![group, nested_group]);
        assert!(tree.get(split).unwrap().is_split_view());
    }

    #[test]
    fn validate_accepts_every_tree_built_through_the_public_api() {
        let mut tree = Tree::new();
        let project = tree.add_project(PathBuf::from("/tmp/validate")).unwrap();
        let group = tree.add_group(project, "work").unwrap();
        let first = tree
            .add_pane(group, "first", PaneContentKind::Terminal)
            .unwrap();
        let second = tree
            .add_pane(group, "second", PaneContentKind::Editor)
            .unwrap();
        tree.create_split_view(group, "split", SplitOrientation::Vertical, &[first, second])
            .unwrap();
        tree.add_folder(group, PathBuf::from("/tmp/validate/src"))
            .unwrap();

        assert!(tree.validate().is_ok());
    }

    #[test]
    fn validate_rejects_a_root_that_is_not_a_group() {
        let mut tree = Tree::new();
        tree.nodes.get_mut(&ROOT_ID).unwrap().kind = NodeKind::Folder {
            path: PathBuf::from("/tmp/not-a-group"),
        };

        assert!(matches!(
            tree.validate(),
            Err(TreeError::InvalidStructure(_))
        ));
    }

    #[test]
    fn validate_rejects_a_node_whose_parent_is_not_a_container() {
        let mut tree = Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        let pane = tree
            .add_pane(group, "shell", PaneContentKind::Terminal)
            .unwrap();
        // Corrupt as a hand-edited/crash-truncated snapshot would: insert a
        // node whose `parent` points at the pane instead of at a
        // container. `Pane` has no children list, so nothing can ever list
        // this node back -- it is simultaneously an orphan (unreachable
        // from the root) and, if some other id-holding code path (e.g. a
        // stale `NodeId` a client still holds) ever named it directly,
        // exactly the shape that panics `reorder_sibling`'s
        // `unreachable!("parent of a node is always a group")`. Every safe
        // mutation method keeps this from happening; only a `Deserialize`
        // straight into private fields can produce it.
        tree.nodes.insert(
            NodeId(9999),
            Node {
                id: NodeId(9999),
                parent: Some(pane),
                name: "orphaned under a pane".to_string(),
                short_name: None,
                inferred_icon: None,
                structure_source: StructureSource::Manual,
                kind: NodeKind::Container(ContainerNode::group()),
            },
        );

        assert!(matches!(
            tree.validate(),
            Err(TreeError::InvalidStructure(_))
        ));
    }

    #[test]
    fn validate_rejects_a_parent_child_cycle_disconnected_from_the_root() {
        let mut tree = Tree::new();
        let a = tree.add_group(ROOT_ID, "a").unwrap();
        let b = tree.add_group(ROOT_ID, "b").unwrap();
        // Detach both from the root's own child list and wire them into a
        // two-node cycle instead: `a`'s parent is `b` and `b`'s parent is
        // `a`, with matching (also cyclic) child listings. No sequence of
        // `move_node`/`add_group` calls can produce this; only bypassing
        // the API via `Deserialize` can.
        if let NodeKind::Container(root) = &mut tree.nodes.get_mut(&ROOT_ID).unwrap().kind {
            root.children.clear();
        }
        tree.nodes.get_mut(&a).unwrap().parent = Some(b);
        tree.nodes.get_mut(&b).unwrap().parent = Some(a);
        if let NodeKind::Container(container) = &mut tree.nodes.get_mut(&a).unwrap().kind {
            container.children = vec![b];
        }
        if let NodeKind::Container(container) = &mut tree.nodes.get_mut(&b).unwrap().kind {
            container.children = vec![a];
        }

        assert!(matches!(
            tree.validate(),
            Err(TreeError::InvalidStructure(_))
        ));
    }

    #[test]
    fn validate_rejects_a_next_id_that_would_collide_with_an_existing_node() {
        let mut tree = Tree::new();
        tree.add_group(ROOT_ID, "work").unwrap();
        tree.next_id = 1;

        assert!(matches!(
            tree.validate(),
            Err(TreeError::InvalidStructure(_))
        ));
    }
}

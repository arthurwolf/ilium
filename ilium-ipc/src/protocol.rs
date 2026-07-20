//! Wire types shared by `ilium-client` and `ilium-server`. Nothing in
//! this module does I/O -- it's message shapes only, framed onto a stream
//! by [`crate::framing`]. Tree-structural types (`NodeId`,
//! `TreeMoveDirection`, `PaneStatus`) are re-exported from `ilium-core`
//! rather than redefined here, so the client and server always agree on
//! what a "move" or a "pane status" means; this crate never invents a
//! second copy of a domain concept ilium-core already owns.

use std::path::PathBuf;

use ilium_agent_debug::{AgentDebugEntry, AgentDebugEventDraft, PaneResizeCause};
use ilium_core::{
    BoardStorage, NodeId, PaneStatus, PaneTitleSource, PromptQueueDelivery, RestructurePlan,
    SplitOrientation, Tree, TreeMoveDirection,
};
use ilium_sound::{SoundSettings, SoundSourceKind};
use serde::{Deserialize, Serialize};

/// What kind of pane to create for a [`ClientRequest::NewPane`]. Kept
/// separate from `ilium_core::PaneContentKind`/`PaneStatus` because those
/// describe a pane's *current* content/state in the tree, while this
/// describes what the *server* should spawn -- e.g. `Command` carries the
/// shell command line to launch, which has no equivalent in the domain
/// model (the tree only ever records that a pane is a `Terminal`, not what
/// was run in it).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NewPaneKind {
    /// Spawn the user's default shell.
    PlainShell,
    /// Spawn a specific command line (e.g. `claude`, `codex`) instead of
    /// the default shell.
    Command(String),
    /// Open a file in the built-in editor pane.
    Editor(PathBuf),
    /// Spawn `command_line`, write `initial_input` into its PTY, then submit
    /// it with Enter. Appended so existing bincode variant indexes remain
    /// stable for clients and detached servers built from an earlier source.
    CommandWithInitialInput {
        command_line: String,
        initial_input: String,
    },
}

/// Server-side starting-directory strategy for a newly spawned terminal.
/// The client chooses the policy from its persisted settings, while the
/// server resolves live process cwd information it alone owns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NewPaneWorkingDirectory {
    ProjectRoot,
    FocusedTerminal,
    LastUsed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MouseEventKind {
    Down(MouseButton),
    Up(MouseButton),
    Drag(MouseButton),
    Moved,
    ScrollUp,
    ScrollDown,
    ScrollLeft,
    ScrollRight,
}

/// Modifier keys held during a mouse event. A plain struct of `bool`s
/// rather than a bitflags type -- three fields, always all three
/// meaningful, no need for an extra dependency just to name them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct MouseModifiers {
    pub shift: bool,
    pub alt: bool,
    pub control: bool,
}

/// The semantic origin of an Enter submission. Raw terminal bytes are not
/// enough to recover this reliably: bracketed paste may contain newlines,
/// while scheduled and queued prompts bypass the interactive key handler.
/// The server echoes this value only after the complete PTY write succeeds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PromptSubmissionSource {
    Keyboard,
    VoiceControl,
    ScheduledInput,
    QueuedPrompt,
    InitialAgentPrompt,
}

/// Requests sent from `ilium-client` to `ilium-server`. Everything here
/// is either a tree-structural command (delegated straight to
/// `ilium_core::Tree` methods server-side) or a pane-IO command (raw
/// bytes/coordinates the server forwards to the right PTY via
/// `ilium-pty`). Terminal-capability encoding (e.g. which mouse protocol
/// a pane's foreground app negotiated) stays entirely client-side: the
/// client already has to track that per-pane to render correctly, and the
/// server has no business knowing about it, so `KeyInput`/`MouseInput`
/// carry data the server can forward opaquely.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClientRequest {
    /// Attach this client connection to the named session.
    Attach { session: String },
    /// Create a new pane as a child of `parent_group`.
    NewPane {
        parent_group: NodeId,
        kind: NewPaneKind,
        working_directory: NewPaneWorkingDirectory,
    },
    /// Close a pane (and terminate its PTY/process).
    ClosePane { pane_id: NodeId },
    /// Move a tree node one step in `direction` (mirrors
    /// `Tree::move_node_one_step`).
    MoveNode {
        node_id: NodeId,
        direction: TreeMoveDirection,
    },
    /// Rename a group or pane. `short_title` is the short-form alternative
    /// `ilium-client`'s tree panel shows in place of `title` when the panel
    /// is too narrow for the full title -- `None` when the source of this
    /// rename has no distinct short form (e.g. a user typed it directly).
    RenameNode {
        node_id: NodeId,
        title: String,
        short_title: Option<String>,
        inferred_icon: Option<String>,
    },
    /// Notify the server that a pane's terminal viewport was resized (the
    /// client resizes its own rendering unconditionally; this tells the
    /// server to resize the underlying PTY to match).
    ResizePane {
        pane_id: NodeId,
        rows: u16,
        cols: u16,
        cause: PaneResizeCause,
    },
    /// Raw bytes already encoded for the terminal (arrow keys, control
    /// sequences, literal text) to write into a pane's PTY.
    KeyInput {
        pane_id: NodeId,
        bytes: Vec<u8>,
        submission: Option<PromptSubmissionSource>,
    },
    /// A mouse event to forward into a pane's PTY, already encoded per
    /// that pane's negotiated mouse protocol.
    MouseInput {
        pane_id: NodeId,
        kind: MouseEventKind,
        column: u16,
        row: u16,
        modifiers: MouseModifiers,
    },
    /// Detach this client connection without ending the session.
    Detach,
    /// End the session: kill every pane's process and tear down the tree.
    KillSession,
    /// Create a new group as a child of `parent_group` (`ilium_core::ROOT_ID`
    /// for a top-level group -- unlike `NewPane`, the domain tree itself
    /// allows a group directly under the session root, so this needs no
    /// "no group to target" fallback the way `NewPane`'s `parent_group`
    /// does server-side). Appended as the last variant (rather than next to
    /// `NewPane`, its closest conceptual neighbor) so it doesn't shift the
    /// bincode variant index of every request declared after it -- see
    /// `ilium-ipc`'s `a_bad_shape_frame_errors_instead_of_misparsing_as_the_wrong_variant`
    /// test, which pins specific cross-enum shape collisions.
    NewGroup { parent_group: NodeId, name: String },
    /// Move `node_id` to become a child of `new_parent`, inserted at
    /// `index` within the new parent's children (`None` appends at the
    /// end). Mirrors `Tree::move_node` directly -- unlike `MoveNode`
    /// (`Tree::move_node_one_step`, a one-step sibling swap), this carries
    /// an arbitrary destination and index, for drag-and-drop and
    /// indent-into/out-of-group moves. Appended last for the same reason
    /// `NewGroup` is: keeps every earlier variant's bincode variant index
    /// stable.
    ReparentNode {
        node_id: NodeId,
        new_parent: NodeId,
        index: Option<usize>,
    },
    /// Proposes an automatic title that has no agent-session dependency
    /// (e.g. the plain-shell typed-command titler). Agent transcript titles
    /// use `SetSessionPaneTitle` so the server can verify their expected ID.
    /// Mirrors `Tree::set_automatic_pane_title`: the server applies it only
    /// while the pane hasn't been genuinely user-renamed, unlike `RenameNode`
    /// (which always wins and permanently marks the pane user-specified).
    /// `short_title` mirrors `RenameNode`'s field -- `None` from a source
    /// with no distinct short form (e.g. the plain-shell typed-command
    /// titler, which just echoes the command line).
    /// Appended last for the same reason `NewGroup`/`ReparentNode` are:
    /// keeps every earlier variant's bincode variant index stable.
    SetAutomaticPaneTitle {
        pane_id: NodeId,
        title: String,
        short_title: Option<String>,
        inferred_icon: Option<String>,
    },
    /// The client's active pane view changed: `pane_id` just became (or
    /// stopped being) the pane the user is currently looking at --
    /// entering/leaving pane focus, or switching directly from one pane to
    /// another. Drives `ilium-server`'s adaptive detection schedule: a
    /// focused pane polls on the loop's fastest tick regardless of its
    /// classified status, and the transition itself forces an immediate
    /// (debounced) recheck -- see `ilium-server::detection::force_check`.
    /// Appended last for the same reason `NewGroup`/`ReparentNode`/
    /// `SetAutomaticPaneTitle` are: keeps every earlier variant's bincode
    /// variant index stable.
    SetPaneFocus { pane_id: NodeId, focused: bool },
    /// End the server process without clearing its crash-recovery snapshot.
    /// The next CLI attach can therefore launch a newly-built server and
    /// restore the same project session. Appended last to preserve every
    /// existing bincode discriminant.
    RestartServer,
    /// Persist a filesystem root in the tree. Directory reads remain a
    /// client-local concern; the server owns only this structural reference.
    NewFolder { parent_group: NodeId, path: PathBuf },
    /// Adds one canonical directory-backed top-level project.
    NewProject { path: PathBuf },
    /// Changes the working directory inherited by entries subsequently
    /// created under this project. Existing processes keep their cwd.
    ChangeProjectFolder { project_id: NodeId, path: PathBuf },
    /// Create a persisted, file-backed kanban board under `parent_group`.
    NewBoard {
        parent_group: NodeId,
        name: String,
        storage: BoardStorage,
    },
    /// Atomically creates a split-view container and reparents the selected
    /// panes into it in the supplied order. An empty `pane_ids` list creates
    /// an empty split view.
    CreateSplitView {
        parent_group: NodeId,
        name: String,
        orientation: SplitOrientation,
        pane_ids: Vec<NodeId>,
    },
    /// Replaces the detached server's live sound configuration immediately.
    /// The client has already persisted the same value to `config.toml` so a
    /// future server process starts with it as well.
    UpdateSoundSettings { settings: SoundSettings },
    /// Plays the current source once without changing any event checkbox.
    /// Used by the Sound settings tab's Preview row.
    PreviewSound {
        source: SoundSourceKind,
        file: Option<PathBuf>,
    },
    /// Replaces any pending input for `pane_id` and starts a detached-server
    /// countdown. The server converts this relative delay to one absolute
    /// deadline before broadcasting the authoritative tree snapshot.
    SchedulePaneInput {
        pane_id: NodeId,
        delay_seconds: u64,
        text: String,
        send_enter: bool,
    },
    /// Appends one prompt to the terminal's durable FIFO. Delivery is
    /// triggered only by the server detecting a future agent-finished state.
    EnqueuePrompt {
        pane_id: NodeId,
        text: String,
        delivery: PromptQueueDelivery,
    },
    /// Removes all pending prompts for a terminal pane.
    ClearPromptQueue { pane_id: NodeId },
    /// Applies an LLM title only if `pane_id` still owns the exact session
    /// the client summarized. The server-side compare-and-set closes the
    /// race where `/resume` invalidates a session while a worker result is
    /// crossing IPC. `title_source` controls whether the still-current result
    /// remains automatic or records an explicit user-requested retitle.
    SetSessionPaneTitle {
        pane_id: NodeId,
        expected_session_id: String,
        /// Monotonic per-pane generation that changes whenever ilium sees
        /// the agent return to a fresh conversation. Prevents a worker that
        /// summarized the pre-clear screen from restoring its stale title.
        expected_title_generation: u64,
        title: String,
        short_title: Option<String>,
        inferred_icon: Option<String>,
        title_source: PaneTitleSource,
    },
    /// Replaces everything under the session root with `plan`'s shape --
    /// mirrors `Tree::apply_restructure` directly. The server keeps the
    /// pre-restructure tree in a one-slot undo buffer before applying, so a
    /// single `RevertLastRestructure` can undo it. Appended last to keep
    /// every earlier variant's bincode discriminant stable.
    ApplyRestructurePlan(RestructurePlan),
    /// Restores the tree exactly as it was immediately before the most
    /// recent successfully-applied `ApplyRestructurePlan` on this server, if
    /// any is still held in its one-slot undo buffer. A no-op (surfaced as
    /// `ServerEvent::Error`) if the buffer is empty or has already been
    /// consumed by an earlier revert.
    RevertLastRestructure,
    /// Replaces only one top-level project's descendants. The plan must
    /// mention exactly that project's existing panes/folder roots.
    ApplyProjectRestructurePlan {
        project_id: NodeId,
        plan: RestructurePlan,
    },
    /// Restores only `project_id` from its own latest restructure undo point.
    RevertProjectRestructure { project_id: NodeId },
    /// Resolves an attach-time crash-recovery prompt for this session.
    ResolveSessionRecovery { restore: bool },
    /// Applies the Debug tab's file-logging toggle to the already-running
    /// detached server. The client persists the same value before sending it,
    /// so future server/client processes start with the identical policy.
    UpdateDebugLogging { enabled: bool },
    /// Applies the User Interface tab's agent-debug toggle to the detached
    /// server. Disabling capture retains existing history but stops new
    /// entries until recording is enabled again.
    UpdateAgentDebugMenu { enabled: bool },
    /// Fetches one pane's retained history on demand. The optional sequence
    /// supports a cheap delta when reopening an already-cached log.
    GetPaneDebugLog {
        pane_id: NodeId,
        after_sequence: Option<u64>,
    },
    /// Records a client-owned observation such as an LLM title request or
    /// result. The server rejects stale agent-session/title generations,
    /// then stamps time, sequence, source, and current context itself.
    RecordAgentDebugEvent {
        pane_id: NodeId,
        expected_session_id: Option<String>,
        expected_title_generation: u64,
        event: AgentDebugEventDraft,
    },
}

impl ClientRequest {
    /// Stable, payload-free request name for process diagnostics. Keeping this
    /// exhaustive means every future IPC feature must make an explicit logging
    /// decision instead of silently disappearing from the major-action trail.
    pub const fn diagnostic_name(&self) -> &'static str {
        match self {
            Self::Attach { .. } => "attach",
            Self::NewPane { .. } => "new_pane",
            Self::ClosePane { .. } => "close_pane",
            Self::MoveNode { .. } => "move_node",
            Self::RenameNode { .. } => "rename_node",
            Self::ResizePane { .. } => "resize_pane",
            Self::KeyInput { .. } => "key_input",
            Self::MouseInput { .. } => "mouse_input",
            Self::Detach => "detach",
            Self::KillSession => "kill_session",
            Self::NewGroup { .. } => "new_group",
            Self::ReparentNode { .. } => "reparent_node",
            Self::SetAutomaticPaneTitle { .. } => "set_automatic_pane_title",
            Self::SetPaneFocus { .. } => "set_pane_focus",
            Self::RestartServer => "restart_server",
            Self::NewFolder { .. } => "new_folder",
            Self::NewProject { .. } => "new_project",
            Self::ChangeProjectFolder { .. } => "change_project_folder",
            Self::NewBoard { .. } => "new_board",
            Self::CreateSplitView { .. } => "create_split_view",
            Self::UpdateSoundSettings { .. } => "update_sound_settings",
            Self::PreviewSound { .. } => "preview_sound",
            Self::SchedulePaneInput { .. } => "schedule_pane_input",
            Self::EnqueuePrompt { .. } => "enqueue_prompt",
            Self::ClearPromptQueue { .. } => "clear_prompt_queue",
            Self::SetSessionPaneTitle { .. } => "set_session_pane_title",
            Self::ApplyRestructurePlan(_) => "apply_restructure_plan",
            Self::RevertLastRestructure => "revert_last_restructure",
            Self::ApplyProjectRestructurePlan { .. } => "apply_project_restructure_plan",
            Self::RevertProjectRestructure { .. } => "revert_project_restructure",
            Self::ResolveSessionRecovery { .. } => "resolve_session_recovery",
            Self::UpdateDebugLogging { .. } => "update_debug_logging",
            Self::UpdateAgentDebugMenu { .. } => "update_agent_debug_menu",
            Self::GetPaneDebugLog { .. } => "get_pane_debug_log",
            Self::RecordAgentDebugEvent { .. } => "record_agent_debug_event",
        }
    }

    /// High-frequency presentation/input traffic is available at `debug`
    /// level; structural, lifecycle, scheduling, and settings requests form
    /// the normal `info`-level major-action trail.
    pub const fn is_high_frequency_diagnostic(&self) -> bool {
        matches!(
            self,
            Self::ResizePane { .. }
                | Self::KeyInput {
                    submission: None,
                    ..
                }
                | Self::MouseInput { .. }
                | Self::SetPaneFocus { .. }
        )
    }
}

/// Events pushed from `ilium-server` to `ilium-client`, asynchronously
/// -- not a request/response pair, since the server pushes tree changes
/// and terminal output as they happen rather than waiting to be polled.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ServerEvent {
    /// The full current tree. Sent on attach and after any structural
    /// change (pane/group create, close, move, rename) rather than a
    /// diff: the tree is small (session/group/pane metadata only, no
    /// terminal content) and a full snapshot means the client can never
    /// drift from the server by missing one incremental update -- the
    /// simplicity is worth more here than the bytes saved by diffing.
    TreeSnapshot(Tree),
    /// One or more consecutive raw PTY output chunks for `pane_id`, in the
    /// order they were produced. `sequence` is the newest source chunk
    /// included in `bytes`, allowing the server to combine an already-ready
    /// burst without changing replay de-duplication. Sent as raw bytes rather than a `vt100::Screen`
    /// cell-diff: `vt100::Screen` doesn't implement `Serialize`, and the
    /// client already needs its own `vt100::Parser` per pane to drive
    /// `tui-term` rendering, so feeding it the same byte stream the
    /// server's parser sees keeps both sides' screens derived from one
    /// source of truth instead of the server pre-computing a diff format
    /// the client would just re-derive cells from anyway. Trades some
    /// wire bytes (a full escape sequence vs. a minimal cell delta) for
    /// not needing a second, IPC-specific screen-diff representation.
    ScreenUpdate {
        pane_id: NodeId,
        sequence: u64,
        bytes: Vec<u8>,
    },
    /// A pane's detected status changed (agent identity/activity, or
    /// editor dirty state).
    PaneStatusChanged { pane_id: NodeId, status: PaneStatus },
    /// Something went wrong that the client should surface to the user
    /// (e.g. a pane failed to spawn). Not used for routine
    /// request-rejected cases that have a more specific event already.
    Error { message: String },
    /// An agent pane's session/thread ID was just discovered (see
    /// `ilium-server`'s `session_id` module) -- sent whenever the active
    /// agent session changes. Kept separate from
    /// `PaneStatusChanged` rather than added as a field on `PaneStatus`'s
    /// `Agent` variant: a session ID doesn't affect how a pane is
    /// displayed, and `PaneStatusChanged` already broadcasts far more
    /// often (every detection tick a pane's activity changes) than a
    /// session ID -- which changes much less often -- needs to travel.
    PaneSessionIdResolved {
        pane_id: NodeId,
        session_id: String,
        /// Exact detected agent process that owns this verified transcript.
        /// `None` is retained only as a defensive representation for an
        /// inconsistent restored runtime; normal discovery always supplies it.
        process_id: Option<u32>,
        title_generation: u64,
    },
    /// The previously resolved ID no longer belongs to the detected agent
    /// process (the agent exited or its class changed). Clients must discard
    /// every title-inference cache keyed by that stale identity.
    PaneSessionIdCleared {
        pane_id: NodeId,
        title_generation: u64,
    },
    /// Replays the server-owned path for an editor pane when a client
    /// attaches to an already-restored session. Editor buffers stay local to
    /// each client, but the path is required to create that local buffer.
    PaneEditorPathResolved {
        pane_id: NodeId,
        path: Option<PathBuf>,
    },
    /// A bounded replay of all terminal output retained by the server when a
    /// client attaches. It is sent after `TreeSnapshot` and before live
    /// output is allowed to win the connection writer's priority, so a
    /// reattached client can reconstruct its local `vt100` scrollback.
    ///
    /// `through_sequence` identifies the newest chunk included in `bytes`.
    /// Live `ScreenUpdate`s at or below that sequence are duplicates queued
    /// while the attach was in flight and must be ignored by the client.
    /// Appended last to preserve every existing bincode discriminant.
    TerminalReplay {
        pane_id: NodeId,
        through_sequence: u64,
        bytes: Vec<u8>,
        is_complete: bool,
    },
    /// An agent's visible conversation was cleared. Clients discard local
    /// title-worker state; a separate `PaneSessionIdCleared` accompanies
    /// Claude/Codex `/clear`, and the following tree snapshot shows `<new>`.
    /// Appended last to preserve every existing bincode discriminant.
    PaneSessionTitleCleared {
        pane_id: NodeId,
        title_generation: u64,
    },
    /// A stored session snapshot awaits an explicit recovery decision.
    SessionRecoveryAvailable { pane_count: usize },
    /// The complete attach-time state stream has been delivered: tree,
    /// terminal replay, agent session identities, and editor paths. This is
    /// intentionally later than the first `TreeSnapshot`, so startup actions
    /// cannot race partially reconstructed client state.
    InitialStateSyncComplete,
    /// A semantically identified prompt or command was accepted by the PTY.
    /// Emitted only after the full write succeeds; consumers must never infer
    /// submissions by scanning arbitrary key or paste bytes for carriage
    /// returns.
    PanePromptSubmitted {
        pane_id: NodeId,
        source: PromptSubmissionSource,
    },
    /// The detached server accepted a live Debug logging update. Every
    /// attached client applies this to its own process writer and rendered
    /// setting without echoing another request back to the server. Appended
    /// last to preserve every existing bincode discriminant.
    DebugLoggingChanged { enabled: bool },
    /// The server accepted a live agent-debug recording/menu update.
    AgentDebugMenuChanged { enabled: bool },
    /// One on-demand retained history response. `through_sequence` is the
    /// newest server sequence known even when the requested delta is empty.
    PaneDebugLogSnapshot {
        pane_id: NodeId,
        through_sequence: u64,
        retained_from_sequence: u64,
        dropped_entry_count: u64,
        entries: Vec<AgentDebugEntry>,
    },
    /// One accepted live append. Unchanged detection polls never reach IPC;
    /// clients still merge by authoritative `sequence` so replay/live races
    /// cannot duplicate an event.
    PaneDebugEntryAppended {
        pane_id: NodeId,
        entry: AgentDebugEntry,
    },
}

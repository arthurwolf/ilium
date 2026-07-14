//! Wire types shared by `ilium-client` and `ilium-server`. Nothing in
//! this module does I/O -- it's message shapes only, framed onto a stream
//! by [`crate::framing`]. Tree-structural types (`NodeId`,
//! `TreeMoveDirection`, `PaneStatus`) are re-exported from `ilium-core`
//! rather than redefined here, so the client and server always agree on
//! what a "move" or a "pane status" means; this crate never invents a
//! second copy of a domain concept ilium-core already owns.

use std::path::PathBuf;

use ilium_core::{BoardStorage, NodeId, PaneStatus, SplitOrientation, Tree, TreeMoveDirection};
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
    },
    /// Notify the server that a pane's terminal viewport was resized (the
    /// client resizes its own rendering unconditionally; this tells the
    /// server to resize the underlying PTY to match).
    ResizePane {
        pane_id: NodeId,
        rows: u16,
        cols: u16,
    },
    /// Raw bytes already encoded for the terminal (arrow keys, control
    /// sequences, literal text) to write into a pane's PTY.
    KeyInput { pane_id: NodeId, bytes: Vec<u8> },
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
    /// Proposes an automatically-inferred title for `pane_id` (e.g.
    /// `ilium-client`'s background LLM session-title inference). Mirrors
    /// `Tree::set_automatic_pane_title`: the server applies it only while
    /// the pane hasn't been genuinely user-renamed, unlike `RenameNode`
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
    /// A chunk of raw PTY output bytes for `pane_id`, in the order they
    /// were produced. Sent as raw bytes rather than a `vt100::Screen`
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
    /// Appended last so existing bincode variant indexes remain stable.
    PaneSessionIdResolved { pane_id: NodeId, session_id: String },
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
}

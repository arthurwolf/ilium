//! Wire types shared by `illium-client` and `illium-server`. Nothing in
//! this module does I/O -- it's message shapes only, framed onto a stream
//! by [`crate::framing`]. Tree-structural types (`NodeId`,
//! `TreeMoveDirection`, `PaneStatus`) are re-exported from `illium-core`
//! rather than redefined here, so the client and server always agree on
//! what a "move" or a "pane status" means; this crate never invents a
//! second copy of a domain concept illium-core already owns.

use std::path::PathBuf;

use illium_core::{NodeId, PaneStatus, Tree, TreeMoveDirection};
use serde::{Deserialize, Serialize};

/// What kind of pane to create for a [`ClientRequest::NewPane`]. Kept
/// separate from `illium_core::PaneContentKind`/`PaneStatus` because those
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

/// Requests sent from `illium-client` to `illium-server`. Everything here
/// is either a tree-structural command (delegated straight to
/// `illium_core::Tree` methods server-side) or a pane-IO command (raw
/// bytes/coordinates the server forwards to the right PTY via
/// `illium-pty`). Terminal-capability encoding (e.g. which mouse protocol
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
    /// Rename a group or pane.
    RenameNode { node_id: NodeId, title: String },
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
}

/// Events pushed from `illium-server` to `illium-client`, asynchronously
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
    ScreenUpdate { pane_id: NodeId, bytes: Vec<u8> },
    /// A pane's detected status changed (agent identity/activity, or
    /// editor dirty state).
    PaneStatusChanged { pane_id: NodeId, status: PaneStatus },
    /// Something went wrong that the client should surface to the user
    /// (e.g. a pane failed to spawn). Not used for routine
    /// request-rejected cases that have a more specific event already.
    Error { message: String },
}

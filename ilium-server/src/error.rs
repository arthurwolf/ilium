//! Typed errors for `ilium-server`.
//!
//! Two very different failure classes live in this crate: startup failures
//! (bad config, can't bind the session socket) that should abort the
//! process before it does anything, and per-request/per-pane failures that
//! must never do that -- a single pane's detection hiccup or a malformed
//! client request must not take the rest of the session down (see
//! `CLAUDE.md`'s top-level-error-boundary rule). [`ServerError`] covers
//! both; callers on the "must keep running" side (the detection loop, the
//! per-connection request handler) are expected to log an `Err` and
//! continue rather than propagate it further, not to treat every variant
//! as fatal.

use std::path::PathBuf;

use ilium_core::{NodeId, TreeError};

#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    /// The platform reported no resolvable home directory, so
    /// `directories::ProjectDirs` could not compute config/data paths.
    /// Fatal at startup -- there is nowhere to put the session socket.
    #[error("could not determine the platform config/data directories for ilium")]
    NoProjectDirs,

    /// The resolved session socket path could not be bound. Fatal at
    /// startup; the most common cause is a stale socket file left behind
    /// by a crashed previous server for the same session, which the
    /// caller is expected to have already attempted to remove.
    #[error("failed to bind session socket at {path}: {source}")]
    SocketBind {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// The resolved session socket path (whether under
    /// `$XDG_RUNTIME_DIR/ilium` or, lacking that, `<data_dir>`) is at or
    /// over the ~108-byte `sockaddr_un.sun_path` limit the OS enforces for
    /// Unix domain socket paths. The project-session resolver catches this
    /// before `bind()` is ever attempted, so the user gets a specific error
    /// instead of a raw, opaque `std::io::Error`.
    #[error(
        "session socket path {path:?} is {byte_length} bytes, at or over the \
         {max_byte_length}-byte limit this OS allows for a Unix domain socket path; shorten \
         XDG_RUNTIME_DIR (or, if that is unset, XDG_DATA_HOME) or use a shorter session name"
    )]
    SocketPathTooLong {
        path: PathBuf,
        byte_length: usize,
        max_byte_length: usize,
    },

    /// Reading or parsing `~/.config/ilium/config.toml` failed. Not fatal
    /// on its own -- callers fall back to defaults and log a warning --
    /// kept as a typed variant so that fallback decision is explicit
    /// rather than an unwrapped `Result` at the call site.
    #[error("failed to load config from {path}: {source}")]
    ConfigLoad {
        path: PathBuf,
        #[source]
        source: ConfigLoadError,
    },

    /// A tree-structural request (`NewPane`, `ClosePane`, `MoveNode`,
    /// `RenameNode`) referenced a node the tree rejected -- most commonly
    /// a stale `NodeId` a client is still holding after the node was
    /// already removed. Reported back to the requesting client as an
    /// `ilium_ipc::ServerEvent::Error`, never propagated up to crash the
    /// connection or the server.
    #[error("tree operation failed: {0}")]
    Tree(#[from] TreeError),

    /// Spawning, resizing, writing to, or killing a pane's pty failed.
    /// Reported back to the requesting client; the pane is torn down
    /// (removed from the tree and the pane registry) rather than left in
    /// an inconsistent half-alive state.
    #[error("pty operation on pane {pane_id:?} failed: {source}")]
    Pty {
        pane_id: NodeId,
        #[source]
        source: ilium_pty::PtyError,
    },

    /// A request referenced a `NodeId` with no matching entry in the pane
    /// registry (e.g. `KeyInput` targeting a group, or a pane id from a
    /// different session).
    #[error("no pane found for node {0:?}")]
    PaneNotFound(NodeId),

    /// The IPC wire layer failed to (de)serialize or frame a message.
    #[error("ipc framing error: {0}")]
    Ipc(#[from] ilium_ipc::IpcError),

    /// Reading/writing the crash-recovery snapshot file failed. Never
    /// fatal -- the snapshot is a best-effort convenience, not the source
    /// of truth -- but typed so the persistence module can log a specific
    /// reason instead of swallowing an opaque `std::io::Error`.
    #[error("snapshot {operation} at {path} failed: {source}")]
    Snapshot {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: SnapshotError,
    },

    /// Importing the pre-client/server project-local workspace file failed.
    /// The file is retained so the user can recover it rather than losing a
    /// session merely because migration could not complete automatically.
    #[error("failed to migrate legacy workspace file {path}: {message}")]
    LegacyWorkspace { path: PathBuf, message: String },

    /// Another live server already owns this session socket. Treating this
    /// as a startup failure is essential: unlinking a live socket would
    /// strand its daemon and make a second, empty server appear instead.
    #[error("an ilium server is already listening on session socket {0}")]
    SessionAlreadyRunning(PathBuf),

    /// A filesystem operation outside the more specific variants above
    /// (creating the data directory, removing a stale socket file) failed.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Why `~/.config/ilium/config.toml` could not be loaded -- kept separate
/// from `ServerError::ConfigLoad`'s `path` field so the two concerns (which
/// file, why it failed) stay independently testable.
#[derive(Debug, thiserror::Error)]
pub enum ConfigLoadError {
    #[error("failed to read config file: {0}")]
    Read(#[from] std::io::Error),
    #[error("failed to parse config file as TOML: {0}")]
    Parse(#[from] toml::de::Error),
    /// A `[[detection.custom_signatures]]` entry parsed as valid TOML but
    /// failed semantic validation (empty `process_name`, or an
    /// `agent_class` other than `"claude"`/`"codex"`/`"other"`). Kept
    /// distinct from `Parse` -- the file *is* well-formed TOML, just not a
    /// signature this crate knows how to build.
    #[error("invalid detection.custom_signatures entry: {0}")]
    InvalidCustomSignature(String),
}

/// Why a crash-recovery snapshot read/write failed.
#[derive(Debug, thiserror::Error)]
pub enum SnapshotError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    /// Covers both directions -- `serde_json::Error` is the same type for
    /// a failed serialize and a failed parse, and the `operation` field on
    /// `ServerError::Snapshot` (e.g. `"serialize"` vs. `"parse"`) already
    /// says which one happened.
    #[error("(de)serializing snapshot JSON failed: {0}")]
    Json(#[from] serde_json::Error),
}

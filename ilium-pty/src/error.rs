//! Typed errors for the `ilium-pty` adapter. `portable_pty`'s own trait
//! methods return `anyhow::Error` (it has no typed error enum of its own),
//! so this wraps each failure point in a variant that names *where* in the
//! spawn/write/resize lifecycle it happened, instead of leaking an opaque
//! `anyhow::Error` straight out of this crate's public API.

/// Everything that can go wrong talking to a spawned pty and its child
/// process, from opening the pty through to writing input into it.
#[derive(Debug, thiserror::Error)]
pub enum PtyError {
    /// The OS failed to allocate a pty pair (`openpty`-equivalent).
    #[error("failed to open pty: {0}")]
    Open(#[source] anyhow::Error),

    /// The pty was opened but the requested command failed to spawn on its
    /// slave side (bad program name, permission denied, etc).
    #[error("failed to spawn command in pty: {0}")]
    Spawn(#[source] anyhow::Error),

    /// Setting up the pty master's io halves failed after a successful
    /// spawn: duplicating or cloning its read half (including a master that
    /// exposes no raw fd to duplicate) or taking its writer.
    #[error("failed to set up pty io: {0}")]
    Io(#[source] anyhow::Error),

    /// Resizing the pty's underlying OS handle failed (the `vt100` screen
    /// resize that accompanies it cannot fail, so this only ever reports
    /// the OS-level half).
    #[error("failed to resize pty: {0}")]
    Resize(#[source] anyhow::Error),

    /// Writing bytes to the pty's write half (child stdin) failed -- e.g.
    /// the child already exited and closed its end. Deliberately `#[source]`
    /// rather than `#[from]`: a blanket `From<std::io::Error>` would let any
    /// `?` anywhere in this crate silently relabel an unrelated OS failure
    /// as a write failure, which is exactly the "which lifecycle step failed"
    /// distinction this enum exists to preserve (see `Kill` below). Call
    /// sites name the variant explicitly instead.
    #[error("failed to write to pty: {0}")]
    Write(#[source] std::io::Error),

    /// Terminating the spawned child process failed. Kept distinct from
    /// `Write` (both ultimately wrap `std::io::Error`) so callers -- e.g.
    /// `ilium-server` closing a pane -- can tell "the pty stopped
    /// accepting input" apart from "we couldn't kill the process" without
    /// matching on the error message.
    #[error("failed to kill pty child process: {0}")]
    Kill(#[source] std::io::Error),
}

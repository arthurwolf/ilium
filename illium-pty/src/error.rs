//! Typed errors for the `illium-pty` adapter. `portable_pty`'s own trait
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

    /// Cloning the pty master's reader or taking its writer failed after a
    /// successful spawn.
    #[error("failed to set up pty io: {0}")]
    Io(#[source] anyhow::Error),

    /// Resizing the pty's underlying OS handle failed (the `vt100` screen
    /// resize that accompanies it cannot fail, so this only ever reports
    /// the OS-level half).
    #[error("failed to resize pty: {0}")]
    Resize(#[source] anyhow::Error),

    /// Writing bytes to the pty's write half (child stdin) failed -- e.g.
    /// the child already exited and closed its end.
    #[error("failed to write to pty: {0}")]
    Write(#[from] std::io::Error),

    /// Terminating the spawned child process failed. Kept distinct from
    /// `Write` (both ultimately wrap `std::io::Error`) so callers -- e.g.
    /// `illium-server` closing a pane -- can tell "the pty stopped
    /// accepting input" apart from "we couldn't kill the process" without
    /// matching on the error message.
    #[error("failed to kill pty child process: {0}")]
    Kill(#[source] std::io::Error),
}

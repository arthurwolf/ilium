//! Adapter around `portable-pty` + `vt100`: spawn a command behind a pty,
//! then write input to it, resize it, read its parsed screen state, and
//! subscribe to its raw output bytes (live, replayed, or recovered after a
//! gap) until it exits or is killed. That is the entire contract this crate
//! exposes -- see [`PtySession`] for the individual operations.
//!
//! This is the only crate that touches `portable_pty::*` directly, and the
//! only one that owns a `vt100` parser fed by a real pty: everything the
//! rest of the system knows about a pane's screen either comes from
//! [`PtySession::with_screen`]/[`ScreenSnapshot`] or is re-derived by
//! replaying this crate's output bytes through a second, pty-less parser on
//! the far side of IPC (`ilium-client`'s `terminal_view`), never by reaching
//! around this boundary to the pty itself. This crate never knows about a
//! pane tree or agent detection -- those concerns live in
//! `ilium-core`/`ilium-detect`/`ilium-server`, layered on top of what this
//! crate exposes.
//!
//! Terminal capability-query answering (Kitty keyboard-protocol / DA /
//! cursor-position queries) and mouse-event encoding to the xterm wire
//! protocols live here too, in the private `query` and `mouse` modules
//! respectively -- both are pty/terminal-protocol concerns, not tree or
//! detection concerns, so they belong behind the same boundary as the pty
//! itself.

mod error;
mod mouse;
mod query;
mod session;

pub use error::PtyError;
pub use session::{
    PtyCommand, PtyOutputChunk, PtyOutputRecovery, PtyOutputReplay, PtySession, ScreenSnapshot,
};

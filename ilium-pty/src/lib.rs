//! Adapter around `portable-pty` + `vt100`: spawn a command behind a pty,
//! then write input to it, resize it, read its parsed screen state, and
//! subscribe to its raw output bytes (live, replayed, or recovered after a
//! gap) until it exits or is killed. That is the entire contract this crate
//! exposes -- see [`PtySession`] for the individual operations.
//!
//! This crate owns the `vt100` parser fed by a real pty. Native descriptor,
//! transport, and cancellation decisions live in `ilium-platform::pty_io`;
//! everything the rest of the system knows about a pane's screen comes from
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

mod delivery;
mod error;
mod mouse;
mod owner;
mod owner_queue;
mod query;
mod screen_reader;
mod session;

pub use delivery::{
    Delivery, DeliveryError, DeliveryFailure, DeliveryObserver, DeliveryReceipt, OperationKind,
    ShutdownReason,
};
pub use error::PtyError;
pub use owner::{OwnerLimits, OwnerStatus, PtyInput, QueueLoad, ShutdownReport};
pub use session::{
    PtyChildExit, PtyCommand, PtyExitCause, PtyOutputChunk, PtyOutputRecovery, PtyOutputReplay,
    PtySession, ScreenSnapshot,
};

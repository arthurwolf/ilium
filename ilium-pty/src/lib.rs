//! Adapter around `portable-pty` + `vt100`: spawn a command behind a pty,
//! get a handle to write input, resize, and read screen state. That is the
//! entire contract this crate exposes.
//!
//! This is the only crate that touches `portable_pty::*` and `vt100::*`
//! directly (besides the `vt100::Screen` type callers borrow through
//! [`PtySession::with_screen`], which they already depend on `vt100` to
//! name). It never knows about a pane tree or agent detection -- those
//! concerns live in `ilium-core`/`ilium-detect`/`ilium-server`, layered
//! on top of what this crate exposes.
//!
//! Terminal capability-query answering (Kitty keyboard-protocol / DA /
//! cursor-position queries) and mouse-event encoding to the xterm wire
//! protocols live here too, in [`query`] and [`mouse`] respectively --
//! both are pty/terminal-protocol concerns, not tree or detection
//! concerns, so they belong behind the same boundary as the pty itself.

mod error;
mod mouse;
mod query;
mod session;

pub use error::PtyError;
pub use session::{PtyCommand, PtyOutputChunk, PtyOutputReplay, PtySession};

//! The local client/server channel for one ilium session.
//!
//! `ilium-ipc` owns *what* is exchanged and how it is framed, and says so:
//! it is deliberately generic over any async byte stream. This crate owns
//! *where* those bytes travel, which is the part that differs per platform:
//! a Unix domain socket on Unix, a named pipe on Windows.
//!
//! Both sides name a session the same way -- by the socket path the CLI
//! resolves and passes to the server -- so [`SessionEndpoint`] takes that one
//! identity and knows how to bind, connect, probe, and clean up, whichever
//! mechanism actually carries the bytes.
//!
//! The differences worth knowing about, because they shape the API:
//!
//! - A Unix socket is a filesystem entry that outlives the process that bound
//!   it. A crashed server therefore leaves one behind, so binding must first
//!   decide whether an existing entry belongs to a live server or is debris.
//!   A named pipe has no filesystem presence and disappears with its process,
//!   so there is nothing to clean up and [`SessionEndpoint::remove_stale`] is
//!   a no-op there.
//! - A Unix listener accepts many connections. A named pipe has no listening
//!   socket at all: each *instance* serves one client, and the next instance
//!   must already exist before that client is handed off, or a connecting
//!   client meets `ERROR_PIPE_BUSY`. [`SessionListener`] hides that.

mod endpoint;
mod stream;

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

pub use endpoint::{
    is_transient_accept_error, Liveness, SessionEndpoint, SessionListener, TransportError,
};
pub use stream::{SessionReadHalf, SessionStream, SessionWriteHalf};

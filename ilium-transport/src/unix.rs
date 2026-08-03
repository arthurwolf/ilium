//! Unix domain socket implementation of the session transport.
//!
//! The endpoint identity is the socket path itself, so everything here is the
//! obvious thing: bind creates the file, connect opens it, and the file is
//! what a crashed server leaves behind.

use std::io;
use std::path::Path;

use tokio::net::{UnixListener, UnixStream};

use crate::endpoint::Liveness;

pub(crate) type Stream = UnixStream;
pub(crate) type Listener = UnixListener;

pub(crate) fn display(identity: &Path) -> String {
    identity.display().to_string()
}

pub(crate) async fn bind(identity: &Path) -> io::Result<Listener> {
    UnixListener::bind(identity)
}

pub(crate) async fn connect(identity: &Path) -> io::Result<Stream> {
    UnixStream::connect(identity).await
}

pub(crate) async fn accept(listener: &mut Listener) -> io::Result<Stream> {
    listener.accept().await.map(|(stream, _peer)| stream)
}

/// Probes synchronously, with the blocking standard-library socket: callers
/// run this before a runtime exists.
pub(crate) fn probe_liveness(identity: &Path) -> Liveness {
    if !identity.exists() {
        return Liveness::Absent;
    }
    match std::os::unix::net::UnixStream::connect(identity) {
        Ok(_) => Liveness::Live,
        Err(error) if connect_error_proves_dead_listener(&error) => Liveness::StaleListener,
        // Anything else says nothing about liveness -- descriptor exhaustion
        // and permission errors are about *this* process, not the server --
        // so the safe answer is "live": never delete another process's socket
        // on inconclusive evidence.
        Err(_) => Liveness::Live,
    }
}

/// True only for a connect failure that proves the filesystem entry is a dead
/// listener rather than a live one this process merely failed to reach.
///
/// `ConnectionRefused` is the kernel's answer when nothing is `accept()`ing on
/// the socket. `NotFound` covers a path removed by a concurrent racer between
/// the existence check above and this connect. Everything else -- `EMFILE`
/// and `ENFILE` (descriptor exhaustion, not exotic in a process that spawns
/// many PTYs), `EACCES`, a transient `EINTR` -- proves nothing, and deleting a
/// healthy server's socket would let a caller start a second server for the
/// same project.
fn connect_error_proves_dead_listener(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::ConnectionRefused | io::ErrorKind::NotFound
    )
}

pub(crate) fn remove_stale(identity: &Path) -> io::Result<()> {
    match std::fs::remove_file(identity) {
        Ok(()) => Ok(()),
        // Already gone is the outcome asked for.
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

pub(crate) fn peer_process_id(stream: &Stream) -> Option<u32> {
    stream
        .peer_cred()
        .ok()
        .and_then(|credentials| credentials.pid())
        .and_then(|pid| u32::try_from(pid).ok())
}

pub(crate) fn is_transient_accept_error(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::ConnectionAborted
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::Interrupted
    )
}

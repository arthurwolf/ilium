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
    use std::os::unix::fs::FileTypeExt;

    let metadata = match std::fs::symlink_metadata(identity) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Liveness::Absent,
        // Any other stat failure -- EACCES on a parent component, ELOOP --
        // says nothing about whether a server holds this session, and
        // `Absent` would send the caller off to start a second one.
        Err(_) => return Liveness::Unreachable,
    };
    // An entry that is not a socket at all cannot have a listener behind it,
    // whatever `connect` would say about it. Deciding this by file type rather
    // than by error code matters because the kernels disagree: connecting to a
    // regular file is `ECONNREFUSED` on Linux but `ENOTSOCK` on macOS, and only
    // the former looks like a dead listener.
    if !metadata.file_type().is_socket() {
        return Liveness::StaleListener;
    }
    match std::os::unix::net::UnixStream::connect(identity) {
        Ok(_) => Liveness::Live,
        Err(error) if connect_error_proves_dead_listener(&error) => Liveness::StaleListener,
        // Anything else is about *this* process rather than the server, so the
        // endpoint is neither confirmed alive nor safe to delete.
        Err(_) => Liveness::Unreachable,
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

/// The peer's process id, or `None` when the kernel cannot name it.
///
/// `SO_PEERCRED` answers with the peer's id *translated into this process's PID
/// namespace*, and reports `0` when there is no such translation -- a server
/// bound inside a container whose runtime directory is shared with a client on
/// the host is exactly that case. Zero is a sentinel meaning "not nameable
/// here", never a process: `kill(0, ...)` signals the caller's own process
/// group and `kill(0, 0)` reports it as alive forever, so a caller handed
/// `Some(0)` would either signal itself or wait for an exit that never comes.
/// Reporting it as unavailable is the honest answer, and the one callers
/// already handle.
pub(crate) fn peer_process_id(stream: &Stream) -> Option<u32> {
    stream
        .peer_cred()
        .ok()
        .and_then(|credentials| credentials.pid())
        // `try_from` drops a negative id, which is no more a single process
        // than zero is; the filter drops the untranslatable-peer sentinel.
        .and_then(|process_id| u32::try_from(process_id).ok())
        .filter(|process_id| *process_id != 0)
}

pub(crate) fn is_transient_accept_error(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::ConnectionAborted
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::Interrupted
            // Should not surface from an async accept, but it describes "no
            // connection ready yet" rather than a broken listener, so
            // treating it as fatal would be wrong if it ever did.
            | io::ErrorKind::WouldBlock
    )
}

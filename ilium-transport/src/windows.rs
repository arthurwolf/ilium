//! Named-pipe implementation of the session transport.
//!
//! Windows has no filesystem-visible local socket, so the endpoint identity --
//! the socket path the CLI resolved -- is hashed into a stable pipe name that
//! both sides derive independently from the same argument. Nothing is created
//! on disk, which is also why there is no stale debris to clear.
//!
//! The awkward part is accepting. A named pipe has no listening socket: each
//! *instance* serves exactly one client. A server creates an instance, waits
//! for a client on it, and must have the next instance already created before
//! handing that one off -- otherwise a client connecting in the gap is told
//! `ERROR_PIPE_BUSY`. [`accept`] therefore creates the replacement before it
//! returns the connection, never after.

use std::io;
use std::path::Path;
use std::pin::Pin;
use std::task::{Context, Poll};

use sha2::{Digest, Sha256};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::windows::named_pipe::{
    ClientOptions, NamedPipeClient, NamedPipeServer, ServerOptions,
};

use crate::endpoint::Liveness;

/// From `winerror.h`. Returned by a connect attempt when every instance of an
/// existing pipe is already serving a client -- which proves a server is
/// running, unlike "not found".
const ERROR_PIPE_BUSY: i32 = 231;

/// One connected pipe, remembering which side of it this process holds.
///
/// The side matters: asking "who is my peer" is a different call depending on
/// whether this end is the client or the server, unlike a Unix socket's
/// symmetric peer credentials.
#[derive(Debug)]
pub(crate) enum Stream {
    Client(NamedPipeClient),
    Server(NamedPipeServer),
}

impl AsyncRead for Stream {
    fn poll_read(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Self::Client(pipe) => Pin::new(pipe).poll_read(context, buffer),
            Self::Server(pipe) => Pin::new(pipe).poll_read(context, buffer),
        }
    }
}

impl AsyncWrite for Stream {
    fn poll_write(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.get_mut() {
            Self::Client(pipe) => Pin::new(pipe).poll_write(context, buffer),
            Self::Server(pipe) => Pin::new(pipe).poll_write(context, buffer),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Self::Client(pipe) => Pin::new(pipe).poll_flush(context),
            Self::Server(pipe) => Pin::new(pipe).poll_flush(context),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Self::Client(pipe) => Pin::new(pipe).poll_shutdown(context),
            Self::Server(pipe) => Pin::new(pipe).poll_shutdown(context),
        }
    }
}

/// Holds the *next* instance, already created and waiting for a client.
#[derive(Debug)]
pub(crate) struct Listener {
    pipe_name: String,
    next_instance: NamedPipeServer,
}

/// Derives the pipe name both sides use from one session identity.
///
/// A digest rather than the path itself: pipe names may not contain a
/// backslash, live in a single flat namespace where collisions between
/// different projects would be silent, and are capped in length. Hashing the
/// canonical path solves all three at once, and both processes are given the
/// same path by the CLI so both derive the same name.
fn pipe_name(identity: &Path) -> String {
    let digest = Sha256::digest(identity.as_os_str().as_encoded_bytes());
    let mut suffix = String::with_capacity(32);
    for byte in &digest[..16] {
        suffix.push_str(&format!("{byte:02x}"));
    }
    format!(r"\\.\pipe\ilium-{suffix}")
}

pub(crate) fn display(identity: &Path) -> String {
    format!("{} ({})", identity.display(), pipe_name(identity))
}

pub(crate) async fn bind(identity: &Path) -> io::Result<Listener> {
    let pipe_name = pipe_name(identity);
    // `first_pipe_instance` makes this fail if another server already owns the
    // name, which is the exclusivity a Unix bind gets from the filesystem: two
    // servers can never both own one session.
    let next_instance = ServerOptions::new()
        .first_pipe_instance(true)
        .create(&pipe_name)?;
    Ok(Listener {
        pipe_name,
        next_instance,
    })
}

pub(crate) async fn accept(listener: &mut Listener) -> io::Result<Stream> {
    listener.next_instance.connect().await?;
    // Create the replacement *before* yielding the connected instance, so no
    // client can arrive in a window where the pipe exists but has no free
    // instance and be rejected with ERROR_PIPE_BUSY.
    let replacement = ServerOptions::new().create(&listener.pipe_name)?;
    let connected = std::mem::replace(&mut listener.next_instance, replacement);
    Ok(Stream::Server(connected))
}

pub(crate) async fn connect(identity: &Path) -> io::Result<Stream> {
    let pipe_name = pipe_name(identity);
    // `accept` closes the busy window for an attentive server, but a client
    // can still arrive between one accept completing and the next instance
    // existing. That is a transient condition with a documented remedy: wait
    // briefly and retry, rather than failing an otherwise healthy connect.
    const BUSY_RETRY_LIMIT: usize = 20;
    const BUSY_RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(25);

    for _ in 0..BUSY_RETRY_LIMIT {
        match ClientOptions::new().open(&pipe_name) {
            Ok(pipe) => return Ok(Stream::Client(pipe)),
            Err(error) if error.raw_os_error() == Some(ERROR_PIPE_BUSY) => {
                tokio::time::sleep(BUSY_RETRY_DELAY).await;
            }
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::WouldBlock,
        "every named pipe instance stayed busy",
    ))
}

pub(crate) fn probe_liveness(identity: &Path) -> Liveness {
    let pipe_name = pipe_name(identity);
    match ClientOptions::new().open(&pipe_name) {
        // Connecting proves a server is there. The connection is dropped
        // immediately; the server accepts it and sees an instant EOF, exactly
        // as the Unix probe behaves.
        Ok(_) => Liveness::Live,
        // Every instance busy also proves a server is there.
        Err(error) if error.raw_os_error() == Some(ERROR_PIPE_BUSY) => Liveness::Live,
        Err(error) if error.kind() == io::ErrorKind::NotFound => Liveness::Absent,
        // Neither confirmed alive nor safe to act on. A named pipe has nothing
        // to delete either way, so this only affects whether callers treat the
        // session as running.
        Err(_) => Liveness::Unreachable,
    }
}

/// A named pipe vanishes with the process that created it, so a crashed server
/// leaves nothing behind to clean up.
pub(crate) fn remove_stale(identity: &Path) -> io::Result<()> {
    let _ = identity;
    Ok(())
}

pub(crate) fn peer_process_id(stream: &Stream) -> Option<u32> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::Pipes::{
        GetNamedPipeClientProcessId, GetNamedPipeServerProcessId,
    };

    let mut process_id: u32 = 0;
    let queried = match stream {
        // This end is the client, so the peer is the server.
        Stream::Client(pipe) => {
            let handle = pipe.as_raw_handle();
            // SAFETY: `handle` is a live pipe handle owned by `pipe` for the
            // duration of this call, and `process_id` is a live local the
            // callee only writes.
            unsafe { GetNamedPipeServerProcessId(handle as _, &mut process_id) }
        }
        // This end is the server, so the peer is the client.
        Stream::Server(pipe) => {
            let handle = pipe.as_raw_handle();
            // SAFETY: as above.
            unsafe { GetNamedPipeClientProcessId(handle as _, &mut process_id) }
        }
    };
    (queried != 0).then_some(process_id)
}

pub(crate) fn is_transient_accept_error(error: &io::Error) -> bool {
    // A client that connects and disconnects before the server finishes
    // accepting surfaces as a broken pipe rather than a reset connection, so
    // the Unix set alone would misclassify it as fatal.
    matches!(
        error.kind(),
        io::ErrorKind::ConnectionAborted
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::Interrupted
            | io::ErrorKind::WouldBlock
            | io::ErrorKind::BrokenPipe
    )
}

//! Naming one session's channel, and binding, connecting, or probing it.

use std::io;
use std::path::{Path, PathBuf};

use crate::stream::SessionStream;

#[cfg(unix)]
use crate::unix as imp;
#[cfg(windows)]
use crate::windows as imp;

/// What a liveness probe found at an endpoint.
///
/// The distinction between [`StaleListener`](Liveness::StaleListener) and
/// [`Unreachable`](Liveness::Unreachable) is the one that matters: only the
/// former may be deleted. Removing a healthy server's endpoint because this
/// process happened to be out of descriptors would let a second server start
/// for the same project.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Liveness {
    /// Something accepted a connection: a server owns this session.
    Live,
    /// Nothing is listening and nothing was left behind.
    Absent,
    /// Nothing is listening, and what remains is provably debris a bind must
    /// clear first. Only reachable on platforms whose endpoints have
    /// filesystem presence.
    StaleListener,
    /// Something is there but this process could not reach it, for reasons
    /// that say nothing about whether a server is alive -- descriptor
    /// exhaustion, a permissions problem, an interrupted call. Not live, and
    /// never safe to delete.
    Unreachable,
}

#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("failed to bind the session endpoint {endpoint}: {source}")]
    Bind {
        endpoint: String,
        #[source]
        source: io::Error,
    },
    #[error("failed to connect to the session endpoint {endpoint}: {source}")]
    Connect {
        endpoint: String,
        #[source]
        source: io::Error,
    },
}

/// Where one session's server listens and its clients connect.
///
/// Constructed from the socket path the CLI resolves. On Unix that path *is*
/// the endpoint. On Windows it names no file: it is hashed into a stable pipe
/// name, so both sides derive the same channel from the same argument without
/// the CLI needing to know which mechanism is in use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionEndpoint {
    identity: PathBuf,
}

impl SessionEndpoint {
    /// Names an endpoint from the session's resolved socket path.
    pub fn from_path(path: impl Into<PathBuf>) -> Self {
        Self {
            identity: path.into(),
        }
    }

    /// The identity as the CLI passes it between processes.
    pub fn as_path(&self) -> &Path {
        &self.identity
    }

    /// A short human-readable form for diagnostics.
    pub fn display(&self) -> String {
        imp::display(&self.identity)
    }

    /// Starts listening, after clearing any debris a crashed server left.
    ///
    /// Clearing first is what makes a session recoverable without manual
    /// intervention: a Unix socket file outlives the process that bound it, so
    /// a server that died without unbinding would otherwise block every future
    /// bind with `EADDRINUSE`. Debris is only removed once a probe has proven
    /// nothing is actually listening, so this can never displace a live server.
    pub async fn bind(&self) -> Result<SessionListener, TransportError> {
        if self.probe_liveness() == Liveness::StaleListener {
            let _ = self.remove_stale();
        }
        imp::bind(&self.identity)
            .await
            .map(|inner| SessionListener { inner })
            .map_err(|source| TransportError::Bind {
                endpoint: self.display(),
                source,
            })
    }

    /// Connects to the session server.
    pub async fn connect(&self) -> Result<SessionStream, TransportError> {
        imp::connect(&self.identity)
            .await
            .map(|inner| SessionStream { inner })
            .map_err(|source| TransportError::Connect {
                endpoint: self.display(),
                source,
            })
    }

    /// Whether a server currently owns this session.
    ///
    /// Synchronous on purpose: the CLI calls it before it has a runtime, and
    /// on the path that decides whether to spawn a server at all.
    pub fn probe_liveness(&self) -> Liveness {
        imp::probe_liveness(&self.identity)
    }

    /// Removes whatever a dead server left behind. A no-op where endpoints
    /// have no filesystem presence.
    pub fn remove_stale(&self) -> io::Result<()> {
        imp::remove_stale(&self.identity)
    }
}

/// Accepts client connections for one session.
#[derive(Debug)]
pub struct SessionListener {
    inner: imp::Listener,
}

impl SessionListener {
    /// Waits for the next client.
    pub async fn accept(&mut self) -> io::Result<SessionStream> {
        imp::accept(&mut self.inner)
            .await
            .map(|inner| SessionStream { inner })
    }
}

/// Whether an accept failure is a per-connection blip worth retrying, rather
/// than proof the listener itself is unusable.
///
/// A peer that aborts mid-handshake, or an interrupted syscall, says nothing
/// about the listener. Descriptor exhaustion or a destroyed listener does, and
/// retrying those would busy-spin forever.
pub fn is_transient_accept_error(error: &io::Error) -> bool {
    imp::is_transient_accept_error(error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// Endpoint identities live under a temp dir on Unix (where they become
    /// real socket files) and are merely hashed on Windows, so a temp path is
    /// a valid identity on both.
    fn test_endpoint(name: &str) -> (tempfile::TempDir, SessionEndpoint) {
        let directory = tempfile::tempdir().expect("temp dir");
        let endpoint = SessionEndpoint::from_path(directory.path().join(name));
        (directory, endpoint)
    }

    #[tokio::test]
    async fn an_unbound_endpoint_is_not_live() {
        let (_directory, endpoint) = test_endpoint("absent.sock");

        assert_eq!(endpoint.probe_liveness(), Liveness::Absent);
    }

    #[tokio::test]
    async fn a_bound_endpoint_is_live() {
        let (_directory, endpoint) = test_endpoint("live.sock");
        // Held so the endpoint stays bound for the probe. Note the probe
        // itself connects and drops, which a server would observe as one
        // connection that immediately ends -- so this deliberately does not
        // also accept, which would consume that probe rather than a client.
        let _listener = endpoint.bind().await.expect("bind");

        assert_eq!(endpoint.probe_liveness(), Liveness::Live);
    }

    #[tokio::test]
    async fn a_bound_endpoint_round_trips_bytes() {
        let (_directory, endpoint) = test_endpoint("roundtrip.sock");
        let mut listener = endpoint.bind().await.expect("bind");

        let server = tokio::spawn(async move {
            let mut stream = listener.accept().await.expect("accept");
            let mut received = [0u8; 5];
            stream.read_exact(&mut received).await.expect("read");
            stream.write_all(b"world").await.expect("write");
            received
        });

        let mut client = endpoint.connect().await.expect("connect");
        client.write_all(b"hello").await.expect("write");
        let mut response = [0u8; 5];
        client.read_exact(&mut response).await.expect("read");

        assert_eq!(&response, b"world");
        assert_eq!(&server.await.expect("server task"), b"hello");
    }

    #[tokio::test]
    async fn a_second_client_is_accepted_after_the_first_disconnects() {
        // The named-pipe implementation must create its next instance before
        // handing off an accepted one; without that, this second connect
        // fails with ERROR_PIPE_BUSY.
        let (_directory, endpoint) = test_endpoint("sequential.sock");
        let mut listener = endpoint.bind().await.expect("bind");

        for expected in [b"one__", b"two__"] {
            let endpoint_for_client = endpoint.clone();
            let client = tokio::spawn(async move {
                let mut stream = endpoint_for_client.connect().await.expect("connect");
                stream.write_all(expected).await.expect("write");
            });
            let mut accepted = listener.accept().await.expect("accept");
            let mut received = [0u8; 5];
            accepted.read_exact(&mut received).await.expect("read");
            assert_eq!(&received, expected);
            client.await.expect("client task");
        }
    }

    #[tokio::test]
    async fn two_clients_can_be_connected_at_the_same_time() {
        let (_directory, endpoint) = test_endpoint("concurrent.sock");
        let mut listener = endpoint.bind().await.expect("bind");

        let first_endpoint = endpoint.clone();
        let second_endpoint = endpoint.clone();
        let first_client = tokio::spawn(async move {
            let mut stream = first_endpoint.connect().await.expect("connect first");
            stream.write_all(b"first").await.expect("write");
            stream
        });
        let mut first_accepted = listener.accept().await.expect("accept first");

        // The second client connects while the first is still open, which is
        // the case a single-instance pipe would reject.
        let second_client = tokio::spawn(async move {
            let mut stream = second_endpoint.connect().await.expect("connect second");
            stream.write_all(b"secnd").await.expect("write");
            stream
        });
        let mut second_accepted = listener.accept().await.expect("accept second");

        let mut first_received = [0u8; 5];
        first_accepted
            .read_exact(&mut first_received)
            .await
            .expect("read first");
        let mut second_received = [0u8; 5];
        second_accepted
            .read_exact(&mut second_received)
            .await
            .expect("read second");

        assert_eq!(&first_received, b"first");
        assert_eq!(&second_received, b"secnd");
        drop(first_client.await.expect("first client task"));
        drop(second_client.await.expect("second client task"));
    }

    #[tokio::test]
    async fn a_connected_peer_reports_a_process_id_where_supported() {
        let (_directory, endpoint) = test_endpoint("peer.sock");
        let mut listener = endpoint.bind().await.expect("bind");
        let endpoint_for_client = endpoint.clone();
        let client = tokio::spawn(async move { endpoint_for_client.connect().await });
        let accepted = listener.accept().await.expect("accept");
        let connected = client.await.expect("client task").expect("connect");

        // Both ends live in this test process, so whatever a platform reports
        // must be this process rather than an arbitrary value.
        if let Some(peer) = connected.peer_process_id() {
            assert_eq!(peer, std::process::id());
        }
        drop(accepted);
    }

    /// Binding after a crash must succeed rather than fail with the address
    /// still in use. Only Unix has debris to clear.
    #[cfg(unix)]
    #[tokio::test]
    async fn binding_replaces_a_stale_listener_left_by_a_dead_server() {
        let (_directory, endpoint) = test_endpoint("stale.sock");
        let listener = endpoint.bind().await.expect("first bind");
        // Closing the listener without unlinking is exactly what a crashed
        // server leaves behind: the socket file remains, nothing is listening.
        drop(listener);

        assert_eq!(endpoint.probe_liveness(), Liveness::StaleListener);
        endpoint.bind().await.expect("rebind over stale debris");
    }

    /// A path that is not a socket at all is debris, and deciding that by file
    /// type rather than by connect error is what makes the answer the same on
    /// every kernel: connecting to a regular file is `ECONNREFUSED` on Linux
    /// but `ENOTSOCK` on macOS, and only the first looks like a dead listener.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_regular_file_at_the_endpoint_is_debris_rather_than_a_live_session() {
        let (_directory, endpoint) = test_endpoint("not-a-socket.sock");
        std::fs::write(endpoint.as_path(), b"not a socket").expect("write a regular file");

        assert_eq!(endpoint.probe_liveness(), Liveness::StaleListener);

        endpoint.remove_stale().expect("clear debris");
        assert!(!endpoint.as_path().exists());
    }
}

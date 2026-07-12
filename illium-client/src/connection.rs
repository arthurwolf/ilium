//! Owns the client's Unix domain socket connection to `illium-server` for
//! one session: sends the initial `Attach`, then drives a reader task
//! (decodes `ServerEvent` frames into a channel `crate::run`'s event loop
//! selects on) and a writer task (encodes queued `ClientRequest`s) each
//! with a tracked `JoinHandle` -- both are aborted together on `Drop`, so
//! a session detach or connection failure can never leave either task
//! running past the connection's own lifetime (see `CLAUDE.md`'s
//! async-task-ownership rule).

use std::path::Path;

use illium_ipc::{ClientRequest, IpcError, ServerEvent};
use tokio::net::UnixStream;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

#[derive(Debug, thiserror::Error)]
pub enum ConnectionError {
    #[error("failed to connect to illium-server socket {path}: {source}")]
    Connect {
        path: std::path::PathBuf,
        source: std::io::Error,
    },
    #[error("failed to send the initial Attach request: {0}")]
    InitialAttach(IpcError),
}

/// One live connection to a session's `illium-server`. `events` yields
/// every decoded `ServerEvent` in arrival order; `requests` is cloned into
/// anything that needs to send a `ClientRequest` (the event-dispatch
/// layer, naming workers via `crate::app::App::take_outbound_requests`).
pub struct Connection {
    pub events: mpsc::UnboundedReceiver<ServerEvent>,
    pub requests: mpsc::UnboundedSender<ClientRequest>,
    reader_task: JoinHandle<()>,
    writer_task: JoinHandle<()>,
}

impl Connection {
    /// Connects to `socket_path` and immediately queues `ClientRequest::Attach`
    /// for `session` -- the very first frame the server will read on this
    /// connection (see `illium_server::ipc::handlers::handle_attach`).
    pub async fn connect(socket_path: &Path, session: String) -> Result<Self, ConnectionError> {
        let stream =
            UnixStream::connect(socket_path)
                .await
                .map_err(|source| ConnectionError::Connect {
                    path: socket_path.to_path_buf(),
                    source,
                })?;
        let (read_half, write_half) = stream.into_split();

        let (event_tx, event_rx) = mpsc::unbounded_channel::<ServerEvent>();
        let (request_tx, request_rx) = mpsc::unbounded_channel::<ClientRequest>();

        request_tx
            .send(ClientRequest::Attach { session })
            // The receiver can only be closed if `reader_task` already
            // panicked before this line, which can't happen -- it hasn't
            // been spawned yet. Treated as an invariant, not a real error
            // path, but surfaced via the typed error rather than `.unwrap()`
            // per `CLAUDE.md`'s no-panics rule.
            .map_err(|error| {
                ConnectionError::InitialAttach(IpcError::Io(std::io::Error::other(
                    error.to_string(),
                )))
            })?;

        let reader_task = tokio::spawn(read_loop(read_half, event_tx));
        let writer_task = tokio::spawn(write_loop(write_half, request_rx));

        Ok(Self {
            events: event_rx,
            requests: request_tx,
            reader_task,
            writer_task,
        })
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.reader_task.abort();
        self.writer_task.abort();
    }
}

/// Decodes `ServerEvent` frames off `read_half` until the connection ends
/// (a clean `UnexpectedEof` between frames) or a real protocol error
/// occurs, forwarding each to `event_tx`.
async fn read_loop(
    mut read_half: tokio::net::unix::OwnedReadHalf,
    event_tx: mpsc::UnboundedSender<ServerEvent>,
) {
    loop {
        let event: ServerEvent = match illium_ipc::read_frame(&mut read_half).await {
            Ok(event) => event,
            Err(IpcError::Io(io_error)) if io_error.kind() == std::io::ErrorKind::UnexpectedEof => {
                tracing::info!("server closed the connection");
                break;
            }
            Err(error) => {
                tracing::warn!("connection read failed, closing: {error}");
                break;
            }
        };
        if event_tx.send(event).is_err() {
            // The event loop that owns `crate::run` has shut down; nothing
            // left to forward to.
            break;
        }
    }
}

/// Encodes and sends every `ClientRequest` received on `request_rx`, until
/// the sender side is dropped (session shutdown) or a write fails (server
/// gone).
async fn write_loop(
    mut write_half: tokio::net::unix::OwnedWriteHalf,
    mut request_rx: mpsc::UnboundedReceiver<ClientRequest>,
) {
    while let Some(request) = request_rx.recv().await {
        if let Err(error) = illium_ipc::write_frame(&mut write_half, &request).await {
            tracing::warn!("connection write failed, closing: {error}");
            break;
        }
    }
}

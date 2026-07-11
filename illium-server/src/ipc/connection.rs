//! One attached client connection: reads `ClientRequest` frames, dispatches
//! them via [`crate::ipc::handlers`], and writes back both this
//! connection's own direct replies and every broadcast `ServerEvent` the
//! session produces while attached.
//!
//! Structured as two concurrently-polled loops (reader, writer) inside a
//! *single* spawned task, not two separately-spawned tasks -- `tokio::join!`
//! polls both as plain futures without an extra `tokio::spawn` each, so
//! the one `JoinHandle` the caller tracks (`ServerState::track_connection_task`)
//! cancels both at once. Splitting into two independently-spawned tasks
//! would mean two handles to track and cancel together for what is really
//! one logical connection.

use std::sync::Arc;

use illium_ipc::{read_frame, write_frame, ClientRequest, ServerEvent};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::mpsc;

use crate::ipc::handlers;
use crate::state::ServerState;

/// Drives one accepted connection to completion: concurrently reads
/// requests (dispatching each one) and writes replies/broadcasts, until
/// either side signals the connection is done (client disconnected, a
/// `Detach`/`KillSession` request was handled, or the write side's stream
/// closed).
pub async fn handle<S>(state: Arc<ServerState>, stream: S)
where
    S: AsyncRead + AsyncWrite + Send + 'static,
{
    let (read_half, write_half) = tokio::io::split(stream);
    let (direct_tx, direct_rx) = mpsc::unbounded_channel::<ServerEvent>();
    let broadcast_rx = state.events.subscribe();

    let reader = read_requests(Arc::clone(&state), read_half, direct_tx);
    let writer = write_replies(write_half, broadcast_rx, direct_rx);
    tokio::join!(reader, writer);
}

/// Reads and dispatches `ClientRequest` frames until the stream ends, a
/// frame fails to decode, or a request signals the connection should
/// close. A per-request handling failure is reported back to this
/// connection alone (via `direct_tx`, inside `handlers::handle_request`)
/// and never stops the loop -- one malformed or rejected request must not
/// end the whole connection, only the request that caused it.
async fn read_requests<R>(
    state: Arc<ServerState>,
    mut read_half: R,
    direct_tx: mpsc::UnboundedSender<ServerEvent>,
) where
    R: AsyncRead + Unpin,
{
    loop {
        let request: ClientRequest = match read_frame(&mut read_half).await {
            Ok(request) => request,
            Err(illium_ipc::IpcError::Io(io_error))
                if io_error.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                // The peer closed the connection between frames -- the
                // normal way a client disconnects, not an error.
                break;
            }
            Err(error) => {
                tracing::warn!("connection closed after a frame read/decode error: {error}");
                break;
            }
        };

        if handlers::handle_request(&state, request, &direct_tx).await {
            break;
        }
    }
}

/// Forwards both this connection's direct replies and every session-wide
/// broadcast event to the client, until the underlying stream errors (the
/// client is gone) or both source channels are closed.
async fn write_replies<W>(
    mut write_half: W,
    mut broadcast_rx: tokio::sync::broadcast::Receiver<ServerEvent>,
    mut direct_rx: mpsc::UnboundedReceiver<ServerEvent>,
) where
    W: AsyncWrite + Unpin,
{
    // Once the reader loop ends it drops `direct_tx`, permanently closing
    // `direct_rx` -- `mpsc::Receiver::recv` on a closed channel resolves
    // to `None` *immediately* on every poll, so once that happens this
    // flag stops `select!` from polling that branch at all. Without it,
    // `select!` would busy-loop re-resolving the already-closed branch
    // instead of actually waiting on `broadcast_rx`.
    let mut direct_channel_closed = false;

    loop {
        let event = tokio::select! {
            broadcast_result = broadcast_rx.recv() => match broadcast_result {
                Ok(event) => event,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                    tracing::warn!("connection lagged behind the session broadcast, skipped {skipped} event(s)");
                    continue;
                }
                // The session's broadcast sender only drops with
                // `ServerState` itself, i.e. the whole server is gone --
                // nothing more this connection can do.
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            },
            direct_event = direct_rx.recv(), if !direct_channel_closed => match direct_event {
                Some(event) => event,
                // The reader loop ended (see `read_requests`); no more
                // direct replies are coming, but broadcasts may still be
                // worth forwarding until the stream itself errors below,
                // so keep looping (with this branch now disabled) rather
                // than returning immediately.
                None => {
                    direct_channel_closed = true;
                    continue;
                }
            },
        };

        if let Err(error) = write_frame(&mut write_half, &event).await {
            tracing::warn!("connection write failed, closing: {error}");
            break;
        }
    }
}

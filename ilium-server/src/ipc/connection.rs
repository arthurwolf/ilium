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

use ilium_ipc::{read_frame, write_frame, ClientRequest, ServerEvent};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::mpsc;

use crate::ipc::handlers;
use crate::state::ServerState;

/// Bound on this connection's own direct-reply queue (`Attach` snapshots,
/// per-request errors, resize/key/mouse acks). A slow or stalled client
/// still gets backpressured -- `handlers::send_direct` awaits capacity --
/// rather than letting the queue grow without bound while `write_replies`
/// is stuck on a blocked socket write.
const DIRECT_CHANNEL_CAPACITY: usize = 64;

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
    let (direct_tx, direct_rx) = mpsc::channel::<ServerEvent>(DIRECT_CHANNEL_CAPACITY);
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
    direct_tx: mpsc::Sender<ServerEvent>,
) where
    R: AsyncRead + Unpin,
{
    loop {
        let request: ClientRequest = match read_frame(&mut read_half).await {
            Ok(request) => request,
            Err(ilium_ipc::IpcError::Io(io_error))
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
/// client is gone), the broadcast sender is gone (the whole server is
/// gone), or the reader loop ends (see `read_requests`) -- at which point
/// any broadcast already queued for this connection is drained and sent
/// before returning.
async fn write_replies<W>(
    mut write_half: W,
    mut broadcast_rx: tokio::sync::broadcast::Receiver<ServerEvent>,
    mut direct_rx: mpsc::Receiver<ServerEvent>,
) where
    W: AsyncWrite + Unpin,
{
    loop {
        let event = tokio::select! {
            // Direct attach replies establish the tree and terminal replay
            // before any queued live output is allowed onto this connection.
            // `biased` makes that ordering deterministic when both channels
            // are ready in the same poll.
            biased;
            direct_event = direct_rx.recv() => match direct_event {
                Some(event) => event,
                // The reader loop ended (Detach/KillSession/EOF/decode
                // error): no more requests will ever be dispatched on this
                // connection, so no more direct replies are coming either.
                // Blocking on future `broadcast_rx.recv()`s from here would
                // park this task -- and leak its write-half fd and
                // broadcast subscription -- for as long as the session
                // stays otherwise idle, since nothing would ever wake this
                // select to notice the reader is gone (see
                // `ServerState::track_connection_task`/
                // `abort_all_connection_tasks`, which only reap tracked
                // handles lazily on the next accept or at full server
                // shutdown). Any event already queued for this connection
                // (e.g. `KillSession`'s final `TreeSnapshot`, sent before
                // its handler returns and the reader loop exits) is still
                // worth flushing, so drain what's already pending and then
                // stop, rather than waiting indefinitely for more.
                None => {
                    drain_pending_broadcasts(&mut broadcast_rx, &mut write_half).await;
                    break;
                }
            },
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
        };

        if let Err(error) = write_frame(&mut write_half, &event).await {
            tracing::warn!("connection write failed, closing: {error}");
            break;
        }
    }
}

/// Flushes every broadcast event already sitting in `broadcast_rx`'s buffer
/// (non-blocking) to `write_half`, then returns -- used only once the
/// reader loop has ended and no more direct replies or requests are coming,
/// so there is no reason left to keep waiting on *future* broadcasts (see
/// `write_replies`).
async fn drain_pending_broadcasts<W>(
    broadcast_rx: &mut tokio::sync::broadcast::Receiver<ServerEvent>,
    write_half: &mut W,
) where
    W: AsyncWrite + Unpin,
{
    loop {
        let event = match broadcast_rx.try_recv() {
            Ok(event) => event,
            Err(tokio::sync::broadcast::error::TryRecvError::Lagged(skipped)) => {
                tracing::warn!(
                    "connection lagged while draining final broadcasts, skipped {skipped} event(s)"
                );
                continue;
            }
            // Empty: nothing left queued. Closed: the whole server is gone.
            // Either way, nothing more to flush.
            Err(_) => return,
        };
        if let Err(error) = write_frame(write_half, &event).await {
            tracing::warn!("connection write failed while draining final broadcasts: {error}");
            return;
        }
    }
}

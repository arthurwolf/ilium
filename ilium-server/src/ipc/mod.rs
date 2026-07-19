//! IPC layer: accepts connections on the session's UDS and drives each one
//! (see [`connection`]), dispatching requests through [`handlers`].

mod connection;
// `pub(crate)`, not private: `crate::run`'s crash-recovery restore path
// (in `lib.rs`) calls `handlers::spawn_and_register_pane` directly, the
// same function `handle_new_pane` uses for a live client's `NewPane`
// request -- see that function's doc comment for why the two share it.
pub(crate) mod handlers;

use std::sync::Arc;

use tokio::net::UnixListener;

use crate::state::ServerState;

/// Accepts connections on `listener` forever, spawning one tracked task
/// per connection (see `ServerState::track_connection_task`). A transient
/// per-attempt accept failure (an aborted/reset handshake, an interrupted
/// syscall) is retried in place. Anything else means the listener itself
/// is unrecoverable -- rare enough on a UDS that surfacing it up to
/// `run`'s top-level error boundary is the right call, matching how a
/// listener dying is a session-level event, not a per-connection one.
pub async fn accept_loop(state: Arc<ServerState>, listener: UnixListener) {
    loop {
        let (stream, _peer_addr) = match listener.accept().await {
            Ok(accepted) => accepted,
            // A peer that aborts/resets its connection mid-handshake (or an
            // interrupted syscall) is a per-connection blip, not a problem
            // with the listener itself -- retry accepting. Anything else
            // (e.g. EMFILE/ENFILE from fd exhaustion, or the listener socket
            // going away) is unrecoverable: looping on it would busy-spin
            // forever instead of honoring this function's documented
            // contract of returning so `run`'s top-level select! can shut
            // the session down.
            Err(error) if is_transient_accept_error(&error) => {
                tracing::warn!("transient error accepting a connection, retrying: {error}");
                continue;
            }
            Err(error) => {
                tracing::error!(
                    "failed to accept a connection, listener is no longer usable: {error}"
                );
                return;
            }
        };

        let connection_state = Arc::clone(&state);
        tracing::info!(session_name = %state.session_name, "client connection accepted");
        let handle = tokio::spawn(async move {
            connection::handle(connection_state, stream).await;
        });
        state.track_connection_task(handle);
    }
}

/// Whether an `accept()` failure is a one-off blip scoped to the connection
/// attempt that failed (safe to retry) rather than a sign the listener
/// itself is broken (which should propagate up, see `accept_loop`).
fn is_transient_accept_error(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::ConnectionAborted
            | std::io::ErrorKind::ConnectionReset
            | std::io::ErrorKind::Interrupted
            | std::io::ErrorKind::WouldBlock
    )
}

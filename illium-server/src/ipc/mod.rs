//! IPC layer: accepts connections on the session's UDS and drives each one
//! (see [`connection`]), dispatching requests through [`handlers`].

mod connection;
mod handlers;

use std::sync::Arc;

use tokio::net::UnixListener;

use crate::state::ServerState;

/// Accepts connections on `listener` forever, spawning one tracked task
/// per connection (see `ServerState::track_connection_task`). Returns only
/// if accepting itself fails unrecoverably -- a single failed *accept*
/// (as opposed to a failure within an already-accepted connection, which
/// stays scoped to that connection's own task) is rare enough on a UDS
/// that surfacing it up to `run`'s top-level error boundary is the right
/// call, matching how a listener dying is a session-level event, not a
/// per-connection one.
pub async fn accept_loop(state: Arc<ServerState>, listener: UnixListener) {
    loop {
        let (stream, _peer_addr) = match listener.accept().await {
            Ok(accepted) => accepted,
            Err(error) => {
                tracing::error!("failed to accept a connection: {error}");
                continue;
            }
        };

        let connection_state = Arc::clone(&state);
        let handle = tokio::spawn(async move {
            connection::handle(connection_state, stream).await;
        });
        state.track_connection_task(handle);
    }
}

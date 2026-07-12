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

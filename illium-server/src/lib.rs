//! `illium-server`: owns all PTYs and the session tree for one session,
//! runs the adaptive agent-detection loop, and speaks the `illium-ipc`
//! protocol over that session's Unix domain socket. See README
//! "Architecture" and `CLAUDE.md`'s layering rules -- this crate is the
//! one place those pieces are wired together; each piece's actual logic
//! lives in its own module (or, for the tree/pty/detection primitives
//! themselves, in `illium-core`/`illium-pty`/`illium-detect`).
//!
//! One process serves exactly one session (see `CLAUDE.md`: one UDS
//! socket per session, never multiplexed) -- there is no multi-session
//! registry here, unlike a hypothetical single "server for the whole
//! machine." [`ServerOptions`]/[`run`] take that session's resolved paths
//! directly rather than resolving them internally, so tests can point a
//! server at a tempdir without touching `~/.config`/`~/.local/share` (see
//! `crate::paths` for how `main` resolves the real paths).

pub mod config;
mod detection;
pub mod error;
mod ipc;
mod mouse;
mod pane;
pub mod paths;
mod persistence;
mod state;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tokio::net::UnixListener;

use crate::config::DetectionConfig;
use crate::error::ServerError;
use crate::state::ServerState;

/// Everything [`run`] needs to serve one session. Constructed by `main`
/// from real, platform-resolved paths (see `paths::resolve`), or directly
/// by tests with tempdir paths -- either way `run` itself never touches
/// `directories::ProjectDirs`.
pub struct ServerOptions {
    pub session_name: String,
    pub socket_path: PathBuf,
    pub snapshot_path: PathBuf,
    pub detection_config: DetectionConfig,
}

/// How long `run` waits after a `KillSession` shutdown signal before
/// aborting still-open connection tasks and returning. Long enough for
/// each attached connection's writer loop to wake on the just-broadcast
/// final `TreeSnapshot` and flush it (a single `write_frame` call over a
/// local UDS), short enough that a deliberate shutdown still completes
/// promptly.
const SHUTDOWN_GRACE_PERIOD: Duration = Duration::from_millis(200);

/// Runs the server until a `KillSession` request is handled or the UDS
/// listener itself fails unrecoverably. Binds `options.socket_path`
/// first, removing a stale socket file left behind by a crashed previous
/// server for the same session -- a fresh `run` for the same session name
/// is always a legitimate restart, never a "two servers, one session"
/// conflict (that model doesn't exist here; see the module docs).
pub async fn run(options: ServerOptions) -> Result<(), ServerError> {
    if options.socket_path.exists() {
        std::fs::remove_file(&options.socket_path)?;
    }
    let listener =
        UnixListener::bind(&options.socket_path).map_err(|source| ServerError::SocketBind {
            path: options.socket_path.clone(),
            source,
        })?;

    let state = Arc::new(ServerState::new(
        options.session_name,
        options.snapshot_path,
        options.detection_config,
    ));

    // See `crate::persistence`'s module docs for the "loading a snapshot
    // does not yet respawn its panes" gap this logs about.
    match persistence::load_snapshot(&state.snapshot_path).await {
        Ok(Some(snapshot)) => tracing::info!(
            "found a crash-recovery snapshot with {} pane(s) for session {:?}; \
             starting with a fresh tree -- automatic pane restoration from a \
             found snapshot is not yet implemented",
            snapshot.panes.len(),
            state.session_name
        ),
        Ok(None) => {}
        Err(error) => tracing::warn!("failed to load crash-recovery snapshot: {error}"),
    }

    let detection_task = detection::spawn(Arc::clone(&state));

    tokio::select! {
        () = ipc::accept_loop(Arc::clone(&state), listener) => {}
        () = state.shutdown.notified() => {
            tracing::info!("session {:?} received a shutdown signal", state.session_name);
        }
    }

    detection_task.abort();
    // Gives already-broadcast events (notably `KillSession`'s final
    // `TreeSnapshot`) a chance to actually reach attached clients before
    // their connection tasks are cancelled -- see this constant's doc
    // comment.
    tokio::time::sleep(SHUTDOWN_GRACE_PERIOD).await;
    state.abort_all_connection_tasks();

    let _ = std::fs::remove_file(&options.socket_path);
    Ok(())
}

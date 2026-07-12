//! Top-level typed errors for the illium CLI wrapper.

use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum CliError {
    #[error("could not resolve illium's data directory (no valid home directory found)")]
    NoProjectDirs,
    #[error("--cwd {0:?} is not a valid directory")]
    InvalidCwd(PathBuf),
    #[error(
        "could not locate the illium-server binary next to {0:?} or on PATH -- \
         run `cargo build --workspace` so both binaries land in the same directory"
    )]
    ServerBinaryNotFound(PathBuf),
    #[error("failed to spawn illium-server for session {session:?}: {source}")]
    SpawnServer {
        session: String,
        source: std::io::Error,
    },
    #[error("session {0:?}'s server did not become ready within {1:?}")]
    ServerStartTimeout(String, Duration),
    #[error("session {0:?} is not running")]
    SessionNotRunning(String),
    #[error("failed to read the session socket directory {0:?}: {1}")]
    ReadSocketDir(PathBuf, std::io::Error),
    #[error("the server reported an error: {0}")]
    ServerReportedError(String),
    #[error(transparent)]
    Connection(#[from] illium_client::connection::ConnectionError),
    #[error(transparent)]
    Client(#[from] illium_client::error::ClientError),
}

//! Top-level typed errors for illium-client.

use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("could not resolve illium's data directory (no valid home directory found)")]
    NoProjectDirs,
    #[error("failed to enter raw/alternate-screen terminal mode: {0}")]
    TerminalSetup(#[source] std::io::Error),
    #[error(transparent)]
    Connection(#[from] crate::connection::ConnectionError),
    #[error("session directory {0:?} is not a valid directory")]
    InvalidSessionCwd(PathBuf),
}

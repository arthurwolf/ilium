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
    /// Reading or parsing `~/.config/illium/config.toml`'s client-side
    /// tables (`[keybindings]`, `[theme]`) failed. Not fatal on its own --
    /// `crate::run` logs it and falls back to defaults -- kept as a typed
    /// variant so that fallback decision is explicit rather than an
    /// unwrapped `Result` at the call site, matching
    /// `illium-server`'s own `ServerError::ConfigLoad`.
    #[error("failed to load config from {path}: {source}")]
    ConfigLoad {
        path: PathBuf,
        #[source]
        source: crate::config::ConfigLoadError,
    },
}

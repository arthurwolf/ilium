//! Top-level typed errors for ilium-client.

use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("could not resolve ilium's data directory (no valid home directory found)")]
    NoProjectDirs,
    #[error("failed to enter raw/alternate-screen terminal mode: {0}")]
    TerminalSetup(#[source] std::io::Error),
    #[error(transparent)]
    Connection(#[from] crate::connection::ConnectionError),
    #[error("session directory {0:?} is not a valid directory")]
    InvalidSessionCwd(PathBuf),
    #[error("failed to initialize session file logging: {0}")]
    Logging(#[from] ilium_logging::LoggingError),
    /// Reading or parsing `~/.config/ilium/config.toml`'s client-side
    /// tables (`[keybindings]`, `[theme]`) failed. Not fatal on its own --
    /// `crate::run` logs it and falls back to defaults -- kept as a typed
    /// variant so that fallback decision is explicit rather than an
    /// unwrapped `Result` at the call site, matching
    /// `ilium-server`'s own `ServerError::ConfigLoad`.
    #[error("failed to load config from {path}: {source}")]
    ConfigLoad {
        path: PathBuf,
        #[source]
        source: crate::config::ConfigLoadError,
    },
    /// Persisting a settings-screen change (`crate::app::Mode::Settings`)
    /// back to `config.toml`'s `[ui]` table failed. Not fatal -- the caller
    /// (`App`) reports it via `status_message` and keeps the change live in
    /// memory rather than losing it, matching `ConfigLoad`'s own
    /// "bad config file is a warning, not a crash" policy.
    #[error("failed to save config to {path}: {source}")]
    ConfigSave {
        path: PathBuf,
        // Boxed so this variant doesn't dominate `ClientError`'s overall
        // size (`clippy::result_large_err`) -- `ConfigSaveError` embeds a
        // `toml::de::Error`/`toml::ser::Error`, both sizable.
        #[source]
        source: Box<crate::config::ConfigSaveError>,
    },
}

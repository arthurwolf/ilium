//! Top-level typed errors for ilium-client.

use std::path::PathBuf;

/// Final execution custody must survive an earlier terminal/service failure.
#[derive(Debug)]
pub struct ExecutionShutdownFailure {
    shutdown: std::io::Error,
    previous: Option<ClientError>,
}
impl ExecutionShutdownFailure {
    pub fn shutdown(&self) -> &std::io::Error {
        &self.shutdown
    }
    pub fn previous(&self) -> Option<&ClientError> {
        self.previous.as_ref()
    }
}
impl std::fmt::Display for ExecutionShutdownFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.shutdown.fmt(formatter)?;
        if let Some(previous) = &self.previous {
            write!(formatter, "; earlier client failure: {previous}")?;
        }
        Ok(())
    }
}
impl std::error::Error for ExecutionShutdownFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.shutdown)
    }
}
pub(crate) fn preserve_execution_shutdown_error(
    shutdown: std::io::Error,
    previous: Option<ClientError>,
) -> ClientError {
    ClientError::TerminalSetup(std::io::Error::other(ExecutionShutdownFailure {
        shutdown,
        previous,
    }))
}

pub use crate::terminal_input_owner::{
    InputEvent, InputFailure, InputRetirement, InputRetirementDeadline, InputShutdownReport,
    InputStartError,
};

#[derive(Debug, thiserror::Error)]
pub enum InputRetirementError {
    #[error(transparent)]
    Failed(InputFailure),
    #[error(transparent)]
    Deadline(InputRetirementDeadline),
}

/// Both input outcomes and the earlier client error survive cleanup. A native
/// failure can own a refused Event; a retirement failure can own the rest of
/// the FIFO or a still-live native ticket. Neither may lose to Result::and.
#[derive(Debug)]
pub struct InputRunError {
    pub failure: Option<InputFailure>,
    pub retirement: Option<InputRetirementError>,
    pub other: Option<ClientError>,
}

impl std::fmt::Display for InputRunError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("terminal input did not complete")?;
        if let Some(failure) = &self.failure {
            write!(formatter, "; {failure}")?;
        }
        if let Some(retirement) = &self.retirement {
            write!(formatter, "; {retirement}")?;
        }
        if let Some(other) = &self.other {
            write!(formatter, "; client cleanup: {other}")?;
        }
        Ok(())
    }
}

impl std::error::Error for InputRunError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        if let Some(failure) = &self.failure {
            Some(failure)
        } else if let Some(retirement) = &self.retirement {
            Some(retirement)
        } else {
            self.other.as_ref().map(|error| error as _)
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("could not resolve ilium's data directory (no valid home directory found)")]
    NoProjectDirs,
    #[error("failed to initialize client process resources: {0}")]
    ProcessResources(#[source] std::io::Error),
    #[error("failed to acquire terminal input ownership: {0}")]
    InputStartup(#[from] InputStartError),
    #[error("{0}")]
    Input(#[source] Box<InputRunError>),
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
    #[error("failed to load Kilo Gateway paid proxies from MongoDB: {0}")]
    ProxyDatabase(#[from] crate::proxy_database::ProxyDatabaseError),
}

#[cfg(test)]
mod size_probe {
    #[test]
    fn client_error_stays_well_under_the_large_err_threshold() {
        let size = std::mem::size_of::<super::ClientError>();
        eprintln!("ClientError size {size}");
        assert!(size <= 104, "ClientError grew to {size} bytes");
    }
}

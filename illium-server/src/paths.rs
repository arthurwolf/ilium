//! Resolves the real, platform-correct config/data paths this server uses
//! when run as an actual daemon (as opposed to a test, which constructs
//! [`crate::ServerOptions`] directly with tempdir paths and never calls
//! into this module -- see `CLAUDE.md`'s "Config & data locations", which
//! this module exists solely to satisfy: never hardcode `~`, always go
//! through `directories::ProjectDirs`.

use std::path::PathBuf;

use directories::ProjectDirs;

use crate::error::ServerError;

/// The three real filesystem locations one running session needs:
/// - `config_dir`: shared across all sessions, holds `config.toml`.
/// - `socket_path`: this session's own UDS, `<data_dir>/<session>.sock`
///   (one socket per session -- see `CLAUDE.md`, never multiplexed).
/// - `snapshot_path`: this session's own crash-recovery JSON snapshot,
///   `<data_dir>/<session>.snapshot.json`.
pub struct SessionPaths {
    pub config_dir: PathBuf,
    pub socket_path: PathBuf,
    pub snapshot_path: PathBuf,
}

/// Resolves [`SessionPaths`] for `session_name` and ensures the data
/// directory exists (sockets and snapshots both live there, so it must be
/// created before either is used). Does not create `config_dir` -- an
/// absent config directory means "no config.toml", which
/// [`crate::config::load`] already treats as "use defaults", not an error.
pub fn resolve(session_name: &str) -> Result<SessionPaths, ServerError> {
    let project_dirs = ProjectDirs::from("", "", "illium").ok_or(ServerError::NoProjectDirs)?;
    let data_dir = project_dirs.data_dir();
    std::fs::create_dir_all(data_dir)?;

    Ok(SessionPaths {
        config_dir: project_dirs.config_dir().to_path_buf(),
        socket_path: data_dir.join(format!("{session_name}.sock")),
        snapshot_path: data_dir.join(format!("{session_name}.snapshot.json")),
    })
}

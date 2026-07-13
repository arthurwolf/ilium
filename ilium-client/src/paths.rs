//! Resolves client-local configuration paths. The CLI supplies the exact
//! project session socket to `RunOptions`, so this crate never derives a
//! potentially wrong machine-wide session identity on its own.

use std::path::PathBuf;

use directories::ProjectDirs;

use crate::error::ClientError;

/// This client's config directory, `~/.config/ilium/` on Linux -- the
/// same `directories::ProjectDirs` app-id `ilium-server` resolves
/// independently for its own `config.toml` (see the workspace
/// `CLAUDE.md`'s "Config & data locations"). Holds `config.toml`'s
/// client-side tables (`[keybindings]`, `[theme]`) -- see
/// `crate::config::load`.
pub fn config_dir() -> Result<PathBuf, ClientError> {
    let project_dirs = ProjectDirs::from("", "", "ilium").ok_or(ClientError::NoProjectDirs)?;
    Ok(project_dirs.config_dir().to_path_buf())
}

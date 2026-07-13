//! Server-wide configuration path resolution.
//!
//! Project session paths deliberately do not live here: the CLI resolves and
//! passes their exact socket/snapshot paths to the detached server, ensuring
//! the server cannot silently attach a different project to a global default.

use std::path::PathBuf;

use directories::ProjectDirs;

use crate::error::ServerError;

/// Resolves the user-wide configuration directory. Theme, keybinding, and
/// detection preferences are intentionally global; project session state is
/// not.
pub fn config_dir() -> Result<PathBuf, ServerError> {
    let project_dirs = ProjectDirs::from("", "", "ilium").ok_or(ServerError::NoProjectDirs)?;
    Ok(project_dirs.config_dir().to_path_buf())
}

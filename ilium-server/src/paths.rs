//! Server-wide configuration path resolution.
//!
//! Project session paths deliberately do not live here: the CLI resolves and
//! passes their exact socket/snapshot paths to the detached server, ensuring
//! the server cannot silently attach a different project to a global default.

use std::path::PathBuf;

use crate::error::ServerError;

pub use ilium_platform::paths::CONFIG_DIR_ENV;

/// Resolves the user-wide configuration directory. Theme, keybinding, and
/// detection preferences are intentionally global; project session state is
/// not.
///
/// [`CONFIG_DIR_ENV`] wins when set, so a caller can be explicit rather than
/// depending on which environment variable a given platform happens to read.
pub fn config_dir() -> Result<PathBuf, ServerError> {
    ilium_platform::paths::config_dir().ok_or(ServerError::NoProjectDirs)
}

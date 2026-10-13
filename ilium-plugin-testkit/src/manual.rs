//! Locates the plugin manual pages for documentation-lint tests (test-plan.md section 1.3).

use std::path::PathBuf;

/// Environment variable that overrides the manual pages folder.
pub const MANUAL_DIR_ENV: &str = "ILIUM_PLUGIN_MANUAL_DIR";

/// The folder with the manual pages: the environment override, else `docs/plugins-manual/pages`
/// of the repository that contains this crate.
pub fn pages_dir() -> PathBuf {
    if let Some(overridden) = std::env::var_os(MANUAL_DIR_ENV) {
        return PathBuf::from(overridden);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("docs").join("plugins-manual").join("pages")
}

//! The lock file: `{"format": 1, "plugins": [{"id", "version", "sha256"}]}`, sorted by id
//! (reference-dependency-resolution.md; PKG-803, PKG-828).

use crate::error::PluginError;
use crate::registry::Registry;

/// A parsed lock file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockFile;

impl LockFile {
    /// The locked version of `id`.
    pub fn version_of(&self, _id: &str) -> Option<String> {
        unimplemented!("api::lock::LockFile::version_of")
    }
}

/// Parses and validates a lock text; a bad lock is `lock.invalid` (PKG-828).
pub fn parse(_text: &str) -> Result<LockFile, PluginError> {
    unimplemented!("api::lock::parse")
}

/// Builds a lock for `requests` such as `demo.plugin@^1.0.0`; a yanked version is never chosen (PKG-803).
pub fn regenerate(_registry: &Registry, _requests: &[&str]) -> Result<LockFile, PluginError> {
    unimplemented!("api::lock::regenerate")
}

//! Deterministic plugin archives (reference-package-format.md; PKG-825).

use std::path::Path;

use crate::error::PluginError;

/// One entry of a read archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveEntry;

impl ArchiveEntry {
    /// The path inside the archive.
    pub fn name(&self) -> &str {
        unimplemented!("api::package::ArchiveEntry::name")
    }

    /// The ZIP timestamp as `YYYY-MM-DD HH:MM:SS`; always `1980-01-01 00:00:00`.
    pub fn modified(&self) -> String {
        unimplemented!("api::package::ArchiveEntry::modified")
    }

    /// The Unix mode; always `0o644`.
    pub fn mode(&self) -> u32 {
        unimplemented!("api::package::ArchiveEntry::mode")
    }

    /// The length of the extra field; always 0.
    pub fn extra_field_len(&self) -> usize {
        unimplemented!("api::package::ArchiveEntry::extra_field_len")
    }
}

/// Packs the source tree at `root` into archive bytes that depend only on the tree and `signed_at`.
pub fn pack_directory(_root: &Path, _signed_at: &str) -> Result<Vec<u8>, PluginError> {
    unimplemented!("api::package::pack_directory")
}

/// Reads the entries of archive bytes.
pub fn read_archive(_bytes: &[u8]) -> Result<Vec<ArchiveEntry>, PluginError> {
    unimplemented!("api::package::read_archive")
}

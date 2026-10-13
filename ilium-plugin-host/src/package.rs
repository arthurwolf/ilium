//! The `.ilp` package archive: a strict ZIP subset, read and written by hand
//! (reference-packaging.md PKG-100..182).

use ilium_plugin_api::manifest::{Diagnostic, FileEntry, Manifest};

/// One file in a package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveEntry {
    /// Path inside the archive (reference-packaging.md PKG-110).
    pub path: String,
    /// File bytes.
    pub bytes: Vec<u8>,
}

/// The signature document `signature.json` (reference-packaging.md PKG-200).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureDoc {
    /// Key id of the signer.
    pub key_id: String,
    /// Publisher id of the signer.
    pub publisher_id: String,
    /// Signing time, RFC 3339 UTC.
    pub signed_at: String,
    /// Base64 Ed25519 signature over the canonical manifest and file table.
    pub signature: String,
    /// The signed file table.
    pub files: Vec<FileEntry>,
}

/// A parsed, structurally valid package (reference-packaging.md).
#[derive(Debug, Clone)]
pub struct PackageArchive {
    /// The manifest.
    pub manifest: Manifest,
    /// Every other file.
    pub entries: Vec<ArchiveEntry>,
    /// The signature, if the package is signed.
    pub signature: Option<SignatureDoc>,
}

/// Reads package bytes; a structural fault is a `package.*` diagnostic (reference-packaging.md).
pub fn read_package(bytes: &[u8]) -> Result<PackageArchive, Vec<Diagnostic>> {
    unimplemented!("host::package::read_package")
}

/// Writes files as a package in the accepted ZIP subset (reference-packaging.md).
pub fn write_package(files: &[(String, Vec<u8>)]) -> Result<Vec<u8>, Diagnostic> {
    unimplemented!("host::package::write_package")
}

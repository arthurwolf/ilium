//! Fake file system (reference-test-harness.md "Fake file system"). In memory only.

use ilium_plugin_sdk_core::types::FsPath;

/// Conversion into an `FsPath`: an `FsPath` itself or the shorthand `"scope:path"` string.
pub trait IntoFsPath {
    /// Converts to an `FsPath`.
    fn into_fs_path(self) -> FsPath;
}

impl IntoFsPath for FsPath {
    fn into_fs_path(self) -> FsPath {
        self
    }
}

impl IntoFsPath for &str {
    fn into_fs_path(self) -> FsPath {
        unimplemented!("DEV-458")
    }
}

impl IntoFsPath for String {
    fn into_fs_path(self) -> FsPath {
        unimplemented!("DEV-458")
    }
}

/// Handle on the in-memory file system of a host.
#[derive(Debug)]
pub struct FakeFs {
    _private: (),
}

impl FakeFs {
    /// Writes a file for test setup; a write to a watched path delivers `fs-changed` (DEV-456).
    pub fn write(&mut self, _path: impl IntoFsPath, _bytes: &[u8]) {
        unimplemented!("DEV-456")
    }

    /// Reads a file for an assertion.
    pub fn read(&self, _path: impl IntoFsPath) -> Option<Vec<u8>> {
        unimplemented!("DEV-456")
    }
}

/// Parses the shorthand `"scope:path"` form.
pub fn parse_shorthand(_text: &str) -> Result<FsPath, crate::errors::HarnessError> {
    unimplemented!("DEV-458")
}

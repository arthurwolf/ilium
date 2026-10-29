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

impl IntoFsPath for &String {
    fn into_fs_path(self) -> FsPath {
        self.as_str().into_fs_path()
    }
}

/// One recorded `fs::tail` call (lane hook, see manual_gaps_fakes.md; DEV-456).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TailCall {
    /// The `max-bytes` argument the plugin passed.
    pub max_bytes: usize,
    /// The cursor the fake returned in the chunk.
    pub returned_cursor: u64,
}

/// One recorded `fs::lock` call (lane hook, see manual_gaps_fakes.md; DEV-456).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockCall {
    /// The locked path in `"scope:path"` shorthand.
    pub path: String,
    /// Whether the plugin asked for an exclusive lock.
    pub exclusive: bool,
}

/// One recorded `fs::write` or append call (lane hook, see manual_gaps_fakes.md; DEV-456).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteCall {
    /// The written path in `"scope:path"` shorthand.
    pub path: String,
    /// Whether the call appended instead of replacing.
    pub append: bool,
}

/// A lock a test holds on a path until `release` (lane hook, see manual_gaps_fakes.md; DEV-456).
#[derive(Debug)]
pub struct HeldLock {
    _private: (),
}

impl HeldLock {
    /// Releases the lock so the plugin's next `lock` succeeds.
    pub fn release(self) {
        unimplemented!("DEV-456")
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

    /// Writes a file for test setup without delivering `fs-changed` (lane hook, see manual_gaps_fakes.md; DEV-456).
    pub fn seed(&mut self, _path: impl IntoFsPath, _bytes: &[u8]) {
        unimplemented!("DEV-456")
    }

    /// Seeds a file of `size` bytes without materialising its content (lane hook, see manual_gaps_fakes.md).
    pub fn seed_sparse(&mut self, _path: impl IntoFsPath, _size: u64) {
        unimplemented!("DEV-456")
    }

    /// Creates a directory and its parents (lane hook, see manual_gaps_fakes.md; DEV-456).
    pub fn mkdir(&mut self, _path: impl IntoFsPath) {
        unimplemented!("DEV-456")
    }

    /// Removes a file or an empty directory (lane hook, see manual_gaps_fakes.md; DEV-456).
    pub fn remove(&mut self, _path: impl IntoFsPath) {
        unimplemented!("DEV-456")
    }

    /// Creates a symbolic link at `link` pointing to `target` (lane hook, see manual_gaps_fakes.md; api-fs.md).
    pub fn symlink(&mut self, _link: impl IntoFsPath, _target: impl IntoFsPath) {
        unimplemented!("DEV-456")
    }

    /// Whether the path is a symbolic link (lane hook, see manual_gaps_fakes.md; api-fs.md).
    pub fn is_symlink(&self, _path: impl IntoFsPath) -> bool {
        unimplemented!("DEV-456")
    }

    /// Replaces a file by rename, as an external writer would (lane hook, see manual_gaps_fakes.md; api-fs-atomic-and-tail.md).
    pub fn replace_atomically(&mut self, _path: impl IntoFsPath, _bytes: &[u8]) {
        unimplemented!("DEV-456")
    }

    /// Makes an external write of `bytes` land between the plugin's next read and its write (lane hook, see manual_gaps_fakes.md).
    pub fn race_next_write(&mut self, _path: impl IntoFsPath, _bytes: &[u8]) {
        unimplemented!("DEV-456")
    }

    /// Unix permission bits of a file, `None` when it does not exist (lane hook, see manual_gaps_fakes.md).
    pub fn mode(&self, _path: impl IntoFsPath) -> Option<u32> {
        unimplemented!("DEV-456")
    }

    /// Sets the Unix permission bits of a file (lane hook, see manual_gaps_fakes.md).
    pub fn set_mode(&mut self, _path: impl IntoFsPath, _mode: u32) {
        unimplemented!("DEV-456")
    }

    /// Modification instant of a file in fake milliseconds, `None` when it does not exist (lane hook, see manual_gaps_fakes.md).
    pub fn modified(&self, _path: impl IntoFsPath) -> Option<u64> {
        unimplemented!("DEV-456")
    }

    /// The paths that exist in one scope, in `"scope:path"` shorthand (lane hook, see manual_gaps_fakes.md).
    pub fn paths(&self, _scope: &str) -> Vec<String> {
        unimplemented!("DEV-456")
    }

    /// Every path that exists, in every scope (lane hook, see manual_gaps_fakes.md).
    pub fn all_paths(&self) -> Vec<FsPath> {
        unimplemented!("DEV-456")
    }

    /// Holds a lock on a path, as another program would, until `release` (lane hook, see manual_gaps_fakes.md; api-fs.md "lock").
    pub fn hold_lock(&mut self, _path: impl IntoFsPath, _exclusive: bool) -> HeldLock {
        unimplemented!("DEV-456")
    }

    /// Paths the plugin currently holds a lock on (lane hook, see manual_gaps_fakes.md; api-fs.md "lock").
    pub fn held_locks(&self) -> Vec<String> {
        unimplemented!("DEV-456")
    }

    /// Every `lock` call the plugin made, in order (lane hook, see manual_gaps_fakes.md).
    pub fn lock_log(&self) -> Vec<LockCall> {
        unimplemented!("DEV-456")
    }

    /// Every `write` call the plugin made, in order (lane hook, see manual_gaps_fakes.md).
    pub fn write_log(&self) -> Vec<WriteCall> {
        unimplemented!("DEV-456")
    }

    /// Every `tail` call the plugin made, in order (lane hook, see manual_gaps_fakes.md; api-fs-atomic-and-tail.md).
    pub fn tail_log(&self) -> Vec<TailCall> {
        unimplemented!("DEV-456")
    }
}

/// Parses the shorthand `"scope:path"` form.
pub fn parse_shorthand(_text: &str) -> Result<FsPath, crate::errors::HarnessError> {
    unimplemented!("DEV-458")
}

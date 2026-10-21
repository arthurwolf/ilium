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

#[cfg(test)]
mod spec_tests {
    use super::*;

    /// REQ: [DEV-458] The shorthand form "scope:path" names a scope and a path relative to that scope's root, with '/' separators.
    #[test]
    fn shorthand_splits_scope_and_relative_path() {
        let path = parse_shorthand("project:docs/notes.md").expect("valid shorthand");
        assert_eq!(path.scope, "project");
        assert_eq!(path.path, "docs/notes.md");
    }

    /// REQ: [DEV-458] A path that contains `..` MUST fail with `harness.fs-path-invalid`.
    #[test]
    fn parent_segment_is_rejected_with_fs_path_invalid() {
        let error = parse_shorthand("project:../secrets.txt").expect_err("parent segment must fail");
        assert_eq!(error.code, crate::errors::codes::FS_PATH_INVALID);
    }

    /// REQ: [DEV-458] A path that is absolute MUST fail with `harness.fs-path-invalid`.
    #[test]
    fn absolute_path_is_rejected_with_fs_path_invalid() {
        let error = parse_shorthand("project:/etc/hosts").expect_err("absolute path must fail");
        assert_eq!(error.code, crate::errors::codes::FS_PATH_INVALID);
    }

    /// REQ: [DEV-458] Shorthand is "scope:path" (reference-test-harness.md, Fake file system); text with no scope separator, or empty text, is not a valid path.
    #[test]
    fn text_without_scope_separator_or_empty_text_is_rejected() {
        assert!(parse_shorthand("notes.md").is_err());
        assert!(parse_shorthand("").is_err());
    }

    /// REQ: [DEV-456] host.fs().write(path, bytes) followed by host.fs().read(path) returns the same bytes, with the shorthand accepted as the path.
    #[test]
    fn write_then_read_round_trips_bytes_through_shorthand() {
        let mut fs = FakeFs { _private: () };
        fs.write("project:notes.md", b"hello");
        assert_eq!(fs.read("project:notes.md"), Some(b"hello".to_vec()));
    }

    /// REQ: [DEV-456] An FsPath value and its shorthand string address the same file.
    #[test]
    fn fs_path_value_and_shorthand_address_same_file() {
        let mut fs = FakeFs { _private: () };
        fs.write(
            FsPath {
                scope: "project".to_string(),
                path: "notes.md".to_string(),
            },
            b"x",
        );
        assert_eq!(fs.read("project:notes.md"), Some(b"x".to_vec()));
    }

    /// REQ: [DEV-456] read returns None for a file that was never written (fakes/fs.rs read doc: "Option" result, absent file).
    #[test]
    fn read_of_missing_file_returns_none() {
        let fs = FakeFs { _private: () };
        assert_eq!(fs.read("project:missing.md"), None);
    }

    /// REQ: [DEV-456] remove deletes a file, so a later read returns None (fakes/fs.rs remove doc: "Removes a file or an empty directory").
    #[test]
    fn remove_deletes_file_so_read_returns_none() {
        let mut fs = FakeFs { _private: () };
        fs.write("project:old.md", b"x");
        fs.remove("project:old.md");
        assert_eq!(fs.read("project:old.md"), None);
    }

    /// REQ: api-fs.md entry-kind `symlink` (fakes/fs.rs symlink and is_symlink docs): symlink creates a link at `link` to `target`, is_symlink reports it, and a regular file is not a symlink.
    #[test]
    fn symlink_is_reported_and_regular_file_is_not() {
        let mut fs = FakeFs { _private: () };
        fs.write("project:real.md", b"x");
        fs.symlink("project:link.md", "project:real.md");
        assert!(fs.is_symlink("project:link.md"));
        assert!(!fs.is_symlink("project:real.md"));
    }

    /// REQ: fakes/fs.rs paths doc: lists the paths in one scope in "scope:path" shorthand, and leaves out other scopes.
    #[test]
    fn paths_lists_only_the_requested_scope_in_shorthand_form() {
        let mut fs = FakeFs { _private: () };
        fs.write("project:a.md", b"a");
        fs.write("data:b.bin", b"b");
        assert_eq!(fs.paths("project"), vec!["project:a.md".to_string()]);
    }
}

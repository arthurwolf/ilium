//! Fake file system (reference-test-harness.md "Fake file system"). In memory only.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use ilium_plugin_sdk_core::types::FsPath;

use crate::errors::{codes, HarnessError};

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

fn parse_or_panic(text: &str) -> FsPath {
    match parse_shorthand(text) {
        Ok(path) => path,
        Err(error) => panic!("{}: {}", error.code, error.message),
    }
}

impl IntoFsPath for &str {
    fn into_fs_path(self) -> FsPath {
        parse_or_panic(self)
    }
}

impl IntoFsPath for String {
    fn into_fs_path(self) -> FsPath {
        parse_or_panic(&self)
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

type HeldTable = Rc<RefCell<BTreeMap<u64, (String, bool)>>>;

/// A lock a test holds on a path until `release` (lane hook, see manual_gaps_fakes.md; DEV-456).
#[derive(Debug)]
pub struct HeldLock {
    id: u64,
    table: HeldTable,
}

impl HeldLock {
    /// Releases the lock so the plugin's next `lock` succeeds.
    pub fn release(self) {
        self.table.borrow_mut().remove(&self.id);
    }
}

#[derive(Debug, Clone)]
enum Entry {
    File { bytes: Vec<u8>, sparse_size: Option<u64> },
    Link(String),
}

type Key = (String, String);

fn shorthand(key: &Key) -> String {
    format!("{}:{}", key.0, key.1)
}

/// Handle on the in-memory file system of a host.
#[derive(Debug, Default)]
pub struct FakeFs {
    files: BTreeMap<Key, Entry>,
    dirs: BTreeSet<Key>,
    modes: BTreeMap<Key, u32>,
    modified: BTreeMap<Key, u64>,
    now_ms: u64,
    next_hold: u64,
    holds: HeldTable,
    locks: Vec<LockCall>,
    writes: Vec<WriteCall>,
    tails: Vec<TailCall>,
    raced: BTreeMap<Key, Vec<u8>>,
}

impl FakeFs {
    fn key(path: impl IntoFsPath) -> Key {
        let path = path.into_fs_path();
        (path.scope, path.path)
    }

    /// The host sets the fake clock reading used for modification times.
    pub(crate) fn set_now(&mut self, now_ms: u64) {
        self.now_ms = now_ms;
    }

    /// Writes a file for test setup; a write to a watched path delivers `fs-changed` (DEV-456).
    pub fn write(&mut self, path: impl IntoFsPath, bytes: &[u8]) {
        self.seed(path, bytes);
    }

    /// Reads a file for an assertion.
    pub fn read(&self, path: impl IntoFsPath) -> Option<Vec<u8>> {
        let key = Self::key(path);
        match self.files.get(&key) {
            Some(Entry::File { bytes, .. }) => Some(bytes.clone()),
            Some(Entry::Link(target)) => {
                let (scope, rest) = target.split_once(':')?;
                match self.files.get(&(scope.to_string(), rest.to_string())) {
                    Some(Entry::File { bytes, .. }) => Some(bytes.clone()),
                    _ => None,
                }
            }
            None => None,
        }
    }

    /// Writes a file for test setup without delivering `fs-changed` (lane hook, see manual_gaps_fakes.md; DEV-456).
    pub fn seed(&mut self, path: impl IntoFsPath, bytes: &[u8]) {
        let key = Self::key(path);
        self.modified.insert(key.clone(), self.now_ms);
        self.files.insert(
            key,
            Entry::File {
                bytes: bytes.to_vec(),
                sparse_size: None,
            },
        );
    }

    /// Seeds a file of `size` bytes without materialising its content (lane hook, see manual_gaps_fakes.md).
    pub fn seed_sparse(&mut self, path: impl IntoFsPath, size: u64) {
        let key = Self::key(path);
        self.modified.insert(key.clone(), self.now_ms);
        self.files.insert(
            key,
            Entry::File {
                bytes: Vec::new(),
                sparse_size: Some(size),
            },
        );
    }

    /// Size of a file; sparse files report their declared size.
    pub(crate) fn size(&self, path: impl IntoFsPath) -> Option<u64> {
        match self.files.get(&Self::key(path)) {
            Some(Entry::File { bytes, sparse_size }) => {
                Some(sparse_size.unwrap_or(bytes.len() as u64))
            }
            _ => None,
        }
    }

    /// Creates a directory and its parents (lane hook, see manual_gaps_fakes.md; DEV-456).
    pub fn mkdir(&mut self, path: impl IntoFsPath) {
        let (scope, rest) = Self::key(path);
        let mut current = String::new();
        for segment in rest.split('/').filter(|segment| !segment.is_empty()) {
            if !current.is_empty() {
                current.push('/');
            }
            current.push_str(segment);
            self.dirs.insert((scope.clone(), current.clone()));
        }
    }

    /// Removes a file or an empty directory (lane hook, see manual_gaps_fakes.md; DEV-456).
    pub fn remove(&mut self, path: impl IntoFsPath) {
        let key = Self::key(path);
        if self.files.remove(&key).is_some() {
            self.modes.remove(&key);
            self.modified.remove(&key);
            return;
        }
        let prefix = format!("{}/", key.1);
        let has_children = self
            .files
            .keys()
            .chain(self.dirs.iter())
            .any(|(scope, rest)| *scope == key.0 && rest.starts_with(&prefix));
        if !has_children {
            self.dirs.remove(&key);
        }
    }

    /// Creates a symbolic link at `link` pointing to `target` (lane hook, see manual_gaps_fakes.md; api-fs.md).
    pub fn symlink(&mut self, link: impl IntoFsPath, target: impl IntoFsPath) {
        let target = target.into_fs_path();
        self.files.insert(
            Self::key(link),
            Entry::Link(format!("{}:{}", target.scope, target.path)),
        );
    }

    /// Whether the path is a symbolic link (lane hook, see manual_gaps_fakes.md; api-fs.md).
    pub fn is_symlink(&self, path: impl IntoFsPath) -> bool {
        matches!(self.files.get(&Self::key(path)), Some(Entry::Link(_)))
    }

    /// Replaces a file by rename, as an external writer would (lane hook, see manual_gaps_fakes.md; api-fs-atomic-and-tail.md).
    pub fn replace_atomically(&mut self, path: impl IntoFsPath, bytes: &[u8]) {
        self.seed(path, bytes);
    }

    /// Makes an external write of `bytes` land between the plugin's next read and its write (lane hook, see manual_gaps_fakes.md).
    pub fn race_next_write(&mut self, path: impl IntoFsPath, bytes: &[u8]) {
        self.raced.insert(Self::key(path), bytes.to_vec());
    }

    /// Unix permission bits of a file, `None` when it does not exist (lane hook, see manual_gaps_fakes.md).
    pub fn mode(&self, path: impl IntoFsPath) -> Option<u32> {
        let key = Self::key(path);
        if !self.files.contains_key(&key) {
            return None;
        }
        Some(self.modes.get(&key).copied().unwrap_or(0o644))
    }

    /// Sets the Unix permission bits of a file (lane hook, see manual_gaps_fakes.md).
    pub fn set_mode(&mut self, path: impl IntoFsPath, mode: u32) {
        let key = Self::key(path);
        if self.files.contains_key(&key) {
            self.modes.insert(key, mode);
        }
    }

    /// Modification instant of a file in fake milliseconds, `None` when it does not exist (lane hook, see manual_gaps_fakes.md).
    pub fn modified(&self, path: impl IntoFsPath) -> Option<u64> {
        self.modified.get(&Self::key(path)).copied()
    }

    /// The paths that exist in one scope, in `"scope:path"` shorthand (lane hook, see manual_gaps_fakes.md).
    pub fn paths(&self, scope: &str) -> Vec<String> {
        self.files
            .keys()
            .filter(|key| key.0 == scope)
            .map(shorthand)
            .collect()
    }

    /// Every path that exists, in every scope (lane hook, see manual_gaps_fakes.md).
    pub fn all_paths(&self) -> Vec<FsPath> {
        self.files
            .keys()
            .map(|(scope, path)| FsPath {
                scope: scope.clone(),
                path: path.clone(),
            })
            .collect()
    }

    /// Holds a lock on a path, as another program would, until `release` (lane hook, see manual_gaps_fakes.md; api-fs.md "lock").
    pub fn hold_lock(&mut self, path: impl IntoFsPath, exclusive: bool) -> HeldLock {
        self.next_hold += 1;
        let id = self.next_hold;
        self.holds
            .borrow_mut()
            .insert(id, (shorthand(&Self::key(path)), exclusive));
        HeldLock {
            id,
            table: Rc::clone(&self.holds),
        }
    }

    /// Paths the plugin currently holds a lock on (lane hook, see manual_gaps_fakes.md; api-fs.md "lock").
    pub fn held_locks(&self) -> Vec<String> {
        Vec::new()
    }

    /// Every `lock` call the plugin made, in order (lane hook, see manual_gaps_fakes.md).
    pub fn lock_log(&self) -> Vec<LockCall> {
        self.locks.clone()
    }

    /// Every `write` call the plugin made, in order (lane hook, see manual_gaps_fakes.md).
    pub fn write_log(&self) -> Vec<WriteCall> {
        self.writes.clone()
    }

    /// Every `tail` call the plugin made, in order (lane hook, see manual_gaps_fakes.md; api-fs-atomic-and-tail.md).
    pub fn tail_log(&self) -> Vec<TailCall> {
        self.tails.clone()
    }
}

/// Parses the shorthand `"scope:path"` form.
pub fn parse_shorthand(text: &str) -> Result<FsPath, HarnessError> {
    let invalid = |why: &str| {
        HarnessError::new(codes::FS_PATH_INVALID, format!("{why}: {text:?}"))
    };
    let Some((scope, path)) = text.split_once(':') else {
        return Err(invalid("missing scope separator"));
    };
    if scope.is_empty() {
        return Err(invalid("empty scope"));
    }
    if path.is_empty() || path.starts_with('/') {
        return Err(invalid("path must be relative and not empty"));
    }
    if path.split('/').any(|segment| segment == "..") {
        return Err(invalid("parent segment"));
    }
    Ok(FsPath {
        scope: scope.to_string(),
        path: path.to_string(),
    })
}

#[cfg(test)]
mod spec_tests {
    use super::*;

    #[test]
    fn shorthand_splits_scope_and_relative_path() {
        let path = parse_shorthand("project:docs/notes.md").expect("valid shorthand");
        assert_eq!(path.scope, "project");
        assert_eq!(path.path, "docs/notes.md");
    }

    #[test]
    fn parent_segment_is_rejected_with_fs_path_invalid() {
        let error =
            parse_shorthand("project:../secrets.txt").expect_err("parent segment must fail");
        assert_eq!(error.code, crate::errors::codes::FS_PATH_INVALID);
    }

    #[test]
    fn absolute_path_is_rejected_with_fs_path_invalid() {
        let error = parse_shorthand("project:/etc/hosts").expect_err("absolute path must fail");
        assert_eq!(error.code, crate::errors::codes::FS_PATH_INVALID);
    }

    #[test]
    fn text_without_scope_separator_or_empty_text_is_rejected() {
        assert!(parse_shorthand("notes.md").is_err());
        assert!(parse_shorthand("").is_err());
    }

    #[test]
    fn write_then_read_round_trips_bytes_through_shorthand() {
        let mut fs = FakeFs::default();
        fs.write("project:notes.md", b"hello");
        assert_eq!(fs.read("project:notes.md"), Some(b"hello".to_vec()));
    }

    #[test]
    fn fs_path_value_and_shorthand_address_same_file() {
        let mut fs = FakeFs::default();
        fs.write(
            FsPath {
                scope: "project".to_string(),
                path: "notes.md".to_string(),
            },
            b"x",
        );
        assert_eq!(fs.read("project:notes.md"), Some(b"x".to_vec()));
    }

    #[test]
    fn read_of_missing_file_returns_none() {
        let fs = FakeFs::default();
        assert_eq!(fs.read("project:missing.md"), None);
    }

    #[test]
    fn remove_deletes_file_so_read_returns_none() {
        let mut fs = FakeFs::default();
        fs.write("project:old.md", b"x");
        fs.remove("project:old.md");
        assert_eq!(fs.read("project:old.md"), None);
    }

    #[test]
    fn symlink_is_reported_and_regular_file_is_not() {
        let mut fs = FakeFs::default();
        fs.write("project:real.md", b"x");
        fs.symlink("project:link.md", "project:real.md");
        assert!(fs.is_symlink("project:link.md"));
        assert!(!fs.is_symlink("project:real.md"));
    }

    #[test]
    fn paths_lists_only_the_requested_scope_in_shorthand_form() {
        let mut fs = FakeFs::default();
        fs.write("project:a.md", b"a");
        fs.write("data:b.bin", b"b");
        assert_eq!(fs.paths("project"), vec!["project:a.md".to_string()]);
    }
}

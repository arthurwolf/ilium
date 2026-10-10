//! Handle-relative animation storage. Unix atomic replacement swaps a directory
//! entry for a fresh inode; it does not preserve metadata or promise inode CAS.
use crate::secure_fs::NoFollowDirectory;
use std::{
    ffi::OsStr,
    fs::File,
    io::{self, Write},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
};
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn unsupported() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "animation storage operation is unsupported on this platform",
    )
}
pub fn validate_leaf(name: &str) -> io::Result<()> {
    if name.is_empty()
        || name.len() > 255
        || matches!(name, "." | "..")
        || name
            .chars()
            .any(|character| character.is_control() || matches!(character, '/' | '\\' | ':'))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid storage leaf",
        ));
    }
    let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    if matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && matches!(stem.as_bytes()[3], b'1'..=b'9'))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "reserved storage leaf",
        ));
    }
    Ok(())
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileIdentity {
    pub device: u64,
    pub inode: u64,
}
#[cfg(unix)]
fn identity(file: &File) -> io::Result<FileIdentity> {
    use std::os::unix::fs::MetadataExt;

    let metadata = file.metadata()?;
    Ok(FileIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}
#[cfg(windows)]
fn identity(file: &File) -> io::Result<FileIdentity> {
    let (device, inode) = crate::secure_fs::file_generation(file)?;
    Ok(FileIdentity { device, inode })
}
#[cfg(not(any(unix, windows)))]
fn identity(_file: &File) -> io::Result<FileIdentity> {
    Err(unsupported())
}
pub struct PinnedFile {
    file: File,
    identity: FileIdentity,
    #[cfg(windows)]
    read_file: Mutex<File>,
}
#[cfg(all(test, any(unix, windows)))]
mod pinned_file_tests {
    use super::*;
    use std::io::{Read, Seek, SeekFrom};

    #[test]
    fn positional_reads_preserve_the_shared_file_offset() {
        let fixture = tempfile::tempdir().unwrap();
        let path = fixture.path().join("selected.bin");
        std::fs::write(&path, b"abcdef").unwrap();
        let mut sequential = File::open(path).unwrap();
        sequential.seek(SeekFrom::Start(1)).unwrap();
        let pinned = PinnedFile::from_host(sequential.try_clone().unwrap()).unwrap();

        let mut positional = [0; 2];
        assert_eq!(pinned.read_at(&mut positional, 3).unwrap(), 2);
        assert_eq!(&positional, b"de");
        assert_eq!(pinned.identity(), identity(&pinned.file).unwrap());

        let mut next = [0; 1];
        sequential.read_exact(&mut next).unwrap();
        assert_eq!(&next, b"b");
    }

    #[cfg(windows)]
    #[test]
    fn positional_reads_work_from_handle_relative_windows_open() {
        let fixture = tempfile::tempdir().unwrap();
        let path = fixture.path().join("selected.bin");
        std::fs::write(&path, b"abcdef").unwrap();
        let directory = NoFollowDirectory::open_root(fixture.path()).unwrap();
        let file =
            PinnedFile::from_host(directory.open_regular(OsStr::new("selected.bin")).unwrap())
                .unwrap();

        let mut bytes = [0; 2];
        assert_eq!(file.read_at(&mut bytes, 3).unwrap(), 2);
        assert_eq!(&bytes, b"de");
    }
}
impl PinnedFile {
    pub fn from_host(file: File) -> io::Result<Self> {
        if !file.metadata()?.is_file() {
            return Err(io::Error::other("selected handle is not a regular file"));
        }
        let identity = identity(&file)?;
        #[cfg(windows)]
        let read_file = Mutex::new(crate::nofollow_windows::reopen_regular_for_read(&file)?);
        Ok(Self {
            file,
            identity,
            #[cfg(windows)]
            read_file,
        })
    }
    pub fn identity(&self) -> FileIdentity {
        self.identity
    }
    pub fn len(&self) -> io::Result<u64> {
        if identity(&self.file)? != self.identity {
            return Err(io::Error::other("selected file identity changed"));
        }
        Ok(self.file.metadata()?.len())
    }
    pub fn is_empty(&self) -> io::Result<bool> {
        self.len().map(|length| length == 0)
    }
    pub fn modified(&self) -> io::Result<std::time::SystemTime> {
        if identity(&self.file)? != self.identity {
            return Err(io::Error::other("selected file changed before age read"));
        }
        self.file.metadata()?.modified()
    }
    /// Native cache use is recorded on the completed index inode. This is
    /// advisory eviction order only: it never changes index content/authority.
    pub fn mark_used(&self) -> io::Result<()> {
        if identity(&self.file)? != self.identity {
            return Err(io::Error::other("selected file changed before use mark"));
        }
        self.file
            .set_times(std::fs::FileTimes::new().set_modified(std::time::SystemTime::now()))?;
        if identity(&self.file)? != self.identity {
            return Err(io::Error::other("selected file changed after use mark"));
        }
        self.file.sync_all()
    }

    /// Reads from a specific offset without changing the selected handle's file offset.
    pub fn read_at(&self, out: &mut [u8], offset: u64) -> io::Result<usize> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::FileExt;
            if identity(&self.file)? != self.identity {
                return Err(io::Error::other("selected file changed"));
            }
            self.file.read_at(out, offset)
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::FileExt;
            if identity(&self.file)? != self.identity {
                return Err(io::Error::other("selected file changed"));
            }
            let read_file = self
                .read_file
                .lock()
                .map_err(|_| io::Error::other("selected read handle lock poisoned"))?;
            if identity(&read_file)? != self.identity {
                return Err(io::Error::other("selected read handle identity changed"));
            }
            let bytes_read = read_file.seek_read(out, offset)?;
            if identity(&self.file)? != self.identity || identity(&read_file)? != self.identity {
                return Err(io::Error::other(
                    "selected file changed during positional read",
                ));
            }
            Ok(bytes_read)
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = (out, offset);
            Err(unsupported())
        }
    }
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos", windows)))]
mod pinned_directory_supported_platform_tests {
    use super::*;

    #[test]
    fn pinned_saved_catalog_lists_handle_relative_entries_and_progress() {
        let fixture = tempfile::tempdir().unwrap();
        std::fs::write(fixture.path().join("r.0.0.mca"), b"region").unwrap();
        std::fs::create_dir(fixture.path().join("nested")).unwrap();
        let root = Arc::new(NoFollowDirectory::open_root(fixture.path()).unwrap());
        let pinned = PinnedDirectory::from_host(root).unwrap();
        let mut updates = Vec::new();

        let entries = pinned
            .list_saved_catalog_with_progress(16, &mut |completed, total| {
                updates.push((completed, total));
            })
            .unwrap();

        assert_eq!(entries.len(), 2);
        assert_eq!(
            entries
                .iter()
                .map(|entry| (entry.name.as_str(), entry.bytes, entry.is_directory))
                .collect::<Vec<_>>(),
            [("nested", 0, true), ("r.0.0.mca", 6, false)]
        );
        assert_eq!(updates.first(), Some(&(0, None)));
        assert_eq!(updates.last(), Some(&(2, Some(2))));
        assert!(pinned.child("nested", false).is_ok());
        assert_eq!(
            pinned.ancestor_identities(128).unwrap()[0],
            pinned.identity()
        );
    }

    #[test]
    fn pinned_atomic_publication_is_create_new_and_replace_safe() {
        let fixture = tempfile::tempdir().unwrap();
        let root = Arc::new(
            PinnedDirectory::from_host(Arc::new(
                NoFollowDirectory::open_root(fixture.path()).unwrap(),
            ))
            .unwrap(),
        );

        let mut first = root
            .begin_atomic("history.json", WriteMode::CreateNew)
            .unwrap();
        first.write(b"first", 64).unwrap();
        first.prepare_durable().unwrap();
        assert_eq!(first.publish_entry().unwrap(), first.identity);
        first.durable_ack().unwrap();
        assert_eq!(
            std::fs::read(fixture.path().join("history.json")).unwrap(),
            b"first"
        );

        let mut duplicate = root
            .begin_atomic("history.json", WriteMode::CreateNew)
            .unwrap();
        duplicate.write(b"duplicate", 64).unwrap();
        duplicate.prepare_durable().unwrap();
        assert!(duplicate.publish_entry().is_err());
        assert!(!duplicate.was_published());
        drop(duplicate);
        assert_eq!(root.list(8).unwrap().len(), 1);

        let mut replacement = root
            .begin_atomic("history.json", WriteMode::ReplaceEntry)
            .unwrap();
        replacement.write(b"replacement", 64).unwrap();
        replacement.prepare_durable().unwrap();
        assert_eq!(replacement.publish_entry().unwrap(), replacement.identity);
        replacement.durable_ack().unwrap();
        assert_eq!(
            std::fs::read(fixture.path().join("history.json")).unwrap(),
            b"replacement"
        );
    }
}

pub struct PinnedDirectory {
    root: Arc<NoFollowDirectory>,
    identity: FileIdentity,
    mutations: Mutex<()>,
}
impl PinnedDirectory {
    pub fn from_host(root: Arc<NoFollowDirectory>) -> io::Result<Self> {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            let identity = identity(&root.try_clone_file()?)?;
            Ok(Self {
                root,
                identity,
                mutations: Mutex::new(()),
            })
        }
        #[cfg(windows)]
        {
            let (device, inode) = root.generation()?;
            Ok(Self {
                root,
                identity: FileIdentity { device, inode },
                mutations: Mutex::new(()),
            })
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        {
            let _ = root;
            Err(unsupported())
        }
    }
    pub fn identity(&self) -> FileIdentity {
        self.identity
    }
    /// Shares this already pinned directory descriptor with another trusted
    /// native reader. A pathname derived from the directory is not authority.
    pub fn original_root(&self) -> Arc<NoFollowDirectory> {
        Arc::clone(&self.root)
    }
    /// Native-only physical ancestry for proving a protected history root is
    /// outside a selected source tree. Walks fixed `..` entries from the retained
    /// descriptor, never reconstructs authority from an old path label.
    pub fn ancestor_identities(&self, maximum: usize) -> io::Result<Vec<FileIdentity>> {
        if maximum == 0 || maximum > 128 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "ancestor bound",
            ));
        }
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            use std::os::fd::{AsRawFd, FromRawFd};
            let mut current = self.root.try_clone_file()?;
            let mut ancestors = Vec::with_capacity(maximum);
            let parent_name = c"..";
            loop {
                let current_identity = identity(&current)?;
                if ancestors.contains(&current_identity) {
                    return Err(io::Error::other("directory ancestry cycle"));
                }
                ancestors.push(current_identity);
                // SAFETY: current is an owned live directory descriptor and the
                // fixed NUL-terminated parent name cannot contain guest input.
                let descriptor = unsafe {
                    libc::openat(
                        current.as_raw_fd(),
                        parent_name.as_ptr(),
                        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
                    )
                };
                if descriptor < 0 {
                    return Err(io::Error::last_os_error());
                }
                // SAFETY: successful openat transfers this fresh descriptor.
                let parent = unsafe { File::from_raw_fd(descriptor) };
                if identity(&parent)? == current_identity {
                    return Ok(ancestors);
                }
                if ancestors.len() == maximum {
                    return Err(io::Error::other("directory ancestry exceeds bound"));
                }
                current = parent;
            }
        }
        #[cfg(windows)]
        {
            let mut current = Arc::clone(&self.root);
            let mut ancestors = Vec::with_capacity(maximum);
            loop {
                let (device, inode) = current.generation()?;
                let current_identity = FileIdentity { device, inode };
                if ancestors.contains(&current_identity) {
                    return Err(io::Error::other("directory ancestry cycle"));
                }
                ancestors.push(current_identity);
                let parent = current.open_parent_directory()?;
                if parent.generation()? == (device, inode) {
                    return Ok(ancestors);
                }
                if ancestors.len() == maximum {
                    return Err(io::Error::other("directory ancestry exceeds bound"));
                }
                current = Arc::new(parent);
            }
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        {
            Err(unsupported())
        }
    }
    pub fn open_file(&self, leaf: &str) -> io::Result<PinnedFile> {
        validate_leaf(leaf)?;
        PinnedFile::from_host(self.root.open_regular(OsStr::new(leaf))?)
    }
    pub fn child(&self, leaf: &str, create: bool) -> io::Result<Self> {
        validate_leaf(leaf)?;
        if create {
            self.root.create_directory_if_missing(OsStr::new(leaf))?;
        }
        Self::from_host(Arc::new(self.root.open_directory(OsStr::new(leaf))?))
    }
    pub fn list(&self, maximum: usize) -> io::Result<Vec<DirectoryEntry>> {
        if maximum == 0 || maximum > 1024 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "directory entry limit",
            ));
        }
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            unix_list(self, maximum)
        }
        #[cfg(windows)]
        {
            windows_list(self, maximum)
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        {
            Err(unsupported())
        }
    }
    /// Existing saved-scene catalog ceiling, independently reserved by its
    /// complete scene account before a worker reads this descriptor. Ordinary
    /// selected-resource callers retain the 1,024-entry `list` ceiling.
    pub fn list_saved_catalog(&self, maximum: usize) -> io::Result<Vec<DirectoryEntry>> {
        self.list_saved_catalog_with_progress(maximum, &mut |_, _| {})
    }
    /// Lists a saved-world catalog while reporting how many directory entries
    /// have been inspected. Totals stay unknown until the directory reaches EOF.
    pub fn list_saved_catalog_with_progress(
        &self,
        maximum: usize,
        progress: &mut dyn FnMut(usize, Option<usize>),
    ) -> io::Result<Vec<DirectoryEntry>> {
        if maximum == 0 || maximum > 16_384 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "saved catalog directory entry limit",
            ));
        }
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            unix_list_with_progress(self, maximum, progress)
        }
        #[cfg(windows)]
        {
            windows_list_with_progress(self, maximum, progress)
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        {
            let _ = progress;
            Err(unsupported())
        }
    }
    /// Cooperative whole-namespace mutation lease, nonblocking. A fresh open
    /// description ensures clones of the same root do not bypass flock conflicts.
    pub fn try_exclusive_lease(&self) -> io::Result<DirectoryMutationLease> {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            use std::os::fd::{AsRawFd, FromRawFd};
            let root = self.root.try_clone_file()?;
            let dot = std::ffi::CString::new(".").map_err(io::Error::other)?;
            // SAFETY: pinned directory and static dot create independent OFD.
            let descriptor = unsafe {
                libc::openat(
                    root.as_raw_fd(),
                    dot.as_ptr(),
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
                )
            };
            if descriptor < 0 {
                return Err(io::Error::last_os_error());
            }
            let file = unsafe { File::from_raw_fd(descriptor) };
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(DirectoryMutationLease { _file: file })
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            Err(unsupported())
        }
    }
    /// Readers retain a shared inode lease so root-budget eviction cannot
    /// remove chunks while a playback window can still fault them in.
    pub fn try_shared_lease(&self) -> io::Result<DirectoryMutationLease> {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            use std::os::fd::{AsRawFd, FromRawFd};
            let root = self.root.try_clone_file()?;
            let dot = std::ffi::CString::new(".").map_err(io::Error::other)?;
            let descriptor = unsafe {
                libc::openat(
                    root.as_raw_fd(),
                    dot.as_ptr(),
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
                )
            };
            if descriptor < 0 {
                return Err(io::Error::last_os_error());
            }
            let file = unsafe { File::from_raw_fd(descriptor) };
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_SH | libc::LOCK_NB) } != 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(DirectoryMutationLease { _file: file })
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            Err(unsupported())
        }
    }
    /// Remove only the exact pinned regular file under the caller's directory
    /// mutation lease. A substituted inode or symlink is refused.
    pub fn remove_pinned_file(&self, leaf: &str, file: &PinnedFile) -> io::Result<()> {
        validate_leaf(leaf)?;
        if file.identity() != identity(&file.file)? {
            return Err(io::Error::other("clip removal handle changed"));
        }
        self.root.remove_regular(OsStr::new(leaf), &file.file)
    }
    /// Drop an empty pinned clip directory only when its inode is still the
    /// selected one. The caller holds the parent namespace mutation lease.
    pub fn remove_empty_child(&self, leaf: &str, expected: FileIdentity) -> io::Result<()> {
        validate_leaf(leaf)?;
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            use std::os::fd::AsRawFd;
            let root = self.root.try_clone_file()?;
            let leaf = std::ffi::CString::new(leaf.as_bytes()).map_err(io::Error::other)?;
            let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
            if unsafe {
                libc::fstatat(
                    root.as_raw_fd(),
                    leaf.as_ptr(),
                    stat.as_mut_ptr(),
                    libc::AT_SYMLINK_NOFOLLOW,
                )
            } != 0
            {
                return Err(io::Error::last_os_error());
            }
            let stat = unsafe { stat.assume_init() };
            if stat.st_mode & libc::S_IFMT != libc::S_IFDIR
                || u64::try_from(stat.st_dev).ok() != Some(expected.device)
                || stat.st_ino != expected.inode
            {
                return Err(io::Error::other("clip directory changed before removal"));
            }
            if unsafe { libc::unlinkat(root.as_raw_fd(), leaf.as_ptr(), libc::AT_REMOVEDIR) } != 0 {
                return Err(io::Error::last_os_error());
            }
            self.sync()
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            let _ = expected;
            Err(unsupported())
        }
    }
    pub fn sync(&self) -> io::Result<()> {
        self.root.sync_all()
    }
    pub fn begin_atomic(self: &Arc<Self>, leaf: &str, mode: WriteMode) -> io::Result<AtomicFile> {
        validate_leaf(leaf)?;
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        {
            static NEXT: AtomicU64 = AtomicU64::new(1);
            let sequence = loop {
                let current = NEXT.load(Ordering::Acquire);
                let next = current
                    .checked_add(1)
                    .ok_or_else(|| io::Error::other("storage temporary identity exhausted"))?;
                if NEXT
                    .compare_exchange_weak(current, next, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok()
                {
                    break current;
                }
            };
            let mut random = [0u8; 16];
            getrandom::fill(&mut random).map_err(|error| io::Error::other(error.to_string()))?;
            let suffix = random
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            let temporary = format!(".ilium-stage-{sequence}-{suffix}");
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            let file = self.root.create_regular(OsStr::new(&temporary))?;
            #[cfg(windows)]
            let file = self
                .root
                .create_regular_for_rename(OsStr::new(&temporary))?;
            let identity = identity(&file)?;
            Ok(AtomicFile {
                root: Arc::clone(self),
                file,
                temporary,
                leaf: leaf.to_owned(),
                mode,
                identity,
                published: false,
                aborted: false,
                bytes: 0,
            })
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        {
            let _ = mode;
            Err(unsupported())
        }
    }
}
/// Closing the retained file releases the actual OS flock lease.
pub struct DirectoryMutationLease {
    _file: File,
}
#[derive(Debug, Clone, Copy)]
pub enum WriteMode {
    CreateNew,
    ReplaceEntry,
}
#[derive(Debug)]
pub struct DirectoryEntry {
    pub name: String,
    pub bytes: u64,
    pub is_directory: bool,
}
pub struct AtomicFile {
    root: Arc<PinnedDirectory>,
    file: File,
    temporary: String,
    leaf: String,
    mode: WriteMode,
    identity: FileIdentity,
    published: bool,
    aborted: bool,
    bytes: usize,
}
impl AtomicFile {
    /// Native-only hook view of the original staging inode. The file remains
    /// unpublished; callers must validate the complete bounded readback before
    /// publication. No script receives this file or its temporary name.
    pub fn try_clone_staging_file(&self) -> io::Result<File> {
        if self.published || self.aborted || identity(&self.file)? != self.identity {
            return Err(io::Error::other("staging file no longer unpublished"));
        }
        self.file.try_clone()
    }
    pub fn open_staging_readonly(&self) -> io::Result<File> {
        if self.published || self.aborted {
            return Err(io::Error::other("staging file no longer available"));
        }
        let file = self.root.root.open_regular(OsStr::new(&self.temporary))?;
        if identity(&file)? != self.identity {
            return Err(io::Error::other("staging readback inode changed"));
        }
        Ok(file)
    }
    /// True only after the actual native rename syscall succeeded, even when
    /// subsequent identity/durability confirmation returned an error.
    pub fn was_published(&self) -> bool {
        self.published
    }
    /// Remove only the original unpublished staging inode, reporting cleanup
    /// failures to callers that must retain an explicit failed-write receipt.
    pub fn abort_unpublished(&mut self) -> io::Result<()> {
        if self.published {
            return Err(io::Error::other("published atomic file cannot be aborted"));
        }
        if !self.aborted {
            self.root
                .root
                .remove_regular(OsStr::new(&self.temporary), &self.file)?;
            self.aborted = true;
        }
        Ok(())
    }
    pub fn write(&mut self, bytes: &[u8], maximum: usize) -> io::Result<()> {
        if self.published
            || self.aborted
            || self
                .bytes
                .checked_add(bytes.len())
                .is_none_or(|total| total > maximum)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "atomic write bound",
            ));
        }
        self.file.write_all(bytes)?;
        self.bytes += bytes.len();
        Ok(())
    }
    pub fn prepare_durable(&self) -> io::Result<()> {
        self.file.sync_all()
    }
    /// Bounded native issue point. Bulk write/fsync happen outside broker lock.
    /// ReplaceEntry replaces the currently named regular entry, never follows it.
    /// External directory mutators are not serialized by our process mutex: there
    /// is no atomic compare-inode rename guarantee. Caller must accept that policy.
    pub fn publish_entry(&mut self) -> io::Result<FileIdentity> {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            use std::os::fd::AsRawFd;
            let _guard = self
                .root
                .mutations
                .lock()
                .map_err(|_| io::Error::other("directory mutation guard poisoned"))?;
            if self.published
                || self.aborted
                || self.root.open_file(&self.temporary)?.identity() != self.identity
            {
                return Err(io::Error::other("atomic source identity changed"));
            }
            let root = self.root.root.try_clone_file()?;
            let source =
                std::ffi::CString::new(self.temporary.as_bytes()).map_err(io::Error::other)?;
            let target = std::ffi::CString::new(self.leaf.as_bytes()).map_err(io::Error::other)?;
            if matches!(self.mode, WriteMode::ReplaceEntry) {
                match self.root.open_file(&self.leaf) {
                    Ok(_) => {}
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error),
                }
            }
            // SAFETY: descriptors are pinned directories, CString leaves live through call.
            #[cfg(target_os = "linux")]
            let result = unsafe {
                let flags = if matches!(self.mode, WriteMode::CreateNew) {
                    libc::RENAME_NOREPLACE
                } else {
                    0
                };
                libc::renameat2(
                    root.as_raw_fd(),
                    source.as_ptr(),
                    root.as_raw_fd(),
                    target.as_ptr(),
                    flags,
                )
            };
            #[cfg(target_os = "macos")]
            let result = unsafe {
                let flags = if matches!(self.mode, WriteMode::CreateNew) {
                    libc::RENAME_EXCL
                } else {
                    0
                };
                libc::renameatx_np(
                    root.as_raw_fd(),
                    source.as_ptr(),
                    root.as_raw_fd(),
                    target.as_ptr(),
                    flags,
                )
            };
            if result != 0 {
                return Err(io::Error::last_os_error());
            }
            self.published = true;
            if self.root.open_file(&self.leaf)?.identity() != self.identity {
                return Err(io::Error::other(
                    "published entry identity changed; effect may have occurred",
                ));
            }
            Ok(self.identity)
        }
        #[cfg(windows)]
        {
            let _guard = self
                .root
                .mutations
                .lock()
                .map_err(|_| io::Error::other("directory mutation guard poisoned"))?;
            if self.published
                || self.aborted
                || self.root.open_file(&self.temporary)?.identity() != self.identity
            {
                return Err(io::Error::other("atomic source identity changed"));
            }
            if matches!(self.mode, WriteMode::ReplaceEntry) {
                match self.root.open_file(&self.leaf) {
                    Ok(_) => {}
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error),
                }
            }
            self.root.root.rename_regular(
                &self.file,
                OsStr::new(&self.leaf),
                matches!(self.mode, WriteMode::ReplaceEntry),
            )?;
            self.published = true;
            if self.root.open_file(&self.leaf)?.identity() != self.identity {
                return Err(io::Error::other(
                    "published entry identity changed; effect may have occurred",
                ));
            }
            Ok(self.identity)
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        {
            Err(unsupported())
        }
    }
    pub fn durable_ack(&self) -> io::Result<()> {
        if !self.published {
            return Err(io::Error::other("write not published"));
        }
        self.root.sync()
    }
}
impl Drop for AtomicFile {
    fn drop(&mut self) {
        if !self.published && !self.aborted {
            let _ = self
                .root
                .root
                .remove_regular(OsStr::new(&self.temporary), &self.file);
        }
    }
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn unix_list(directory: &PinnedDirectory, maximum: usize) -> io::Result<Vec<DirectoryEntry>> {
    unix_list_with_progress(directory, maximum, &mut |_, _| {})
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn unix_list_with_progress(
    directory: &PinnedDirectory,
    maximum: usize,
    progress: &mut dyn FnMut(usize, Option<usize>),
) -> io::Result<Vec<DirectoryEntry>> {
    use std::{
        ffi::{CStr, CString},
        os::fd::AsRawFd,
    };
    let root = directory.root.try_clone_file()?;
    use std::os::unix::fs::MetadataExt;
    if root.metadata()?.blksize() > 65536 {
        return Err(io::Error::other(
            "native directory buffer exceeds declared bound",
        ));
    }
    let dot = CString::new(".").map_err(io::Error::other)?;
    // SAFETY: pinned dirfd and static single-component dot open an independent cursor.
    let descriptor = unsafe {
        libc::openat(
            root.as_raw_fd(),
            dot.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
        )
    };
    if descriptor < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful descriptor ownership transfers to DIR on success.
    let stream = unsafe { libc::fdopendir(descriptor) };
    if stream.is_null() {
        unsafe { libc::close(descriptor) };
        return Err(io::Error::last_os_error());
    }
    struct Stream(*mut libc::DIR);
    impl Drop for Stream {
        fn drop(&mut self) {
            unsafe { libc::closedir(self.0) };
        }
    }
    let stream = Stream(stream);
    let mut entries = Vec::new();
    let mut scanned = 0;
    progress(0, None);
    loop {
        // SAFETY: only this owner uses DIR; errno reset distinguishes EOF/error.
        #[cfg(target_os = "linux")]
        unsafe {
            *libc::__errno_location() = 0
        };
        #[cfg(target_os = "macos")]
        unsafe {
            *libc::__error() = 0
        };
        let entry = unsafe { libc::readdir(stream.0) };
        if entry.is_null() {
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(0) {
                return Err(error);
            }
            break;
        }
        let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) }
            .to_str()
            .map_err(io::Error::other)?;
        if matches!(name, "." | "..") {
            continue;
        }
        scanned += 1;
        if scanned > maximum {
            return Err(io::Error::other("directory scan exceeds entry bound"));
        }
        validate_leaf(name)?;
        let name_c = CString::new(name).map_err(io::Error::other)?;
        let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
        let result = unsafe {
            libc::fstatat(
                root.as_raw_fd(),
                name_c.as_ptr(),
                stat.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        };
        if result != 0 {
            return Err(io::Error::last_os_error());
        }
        let stat = unsafe { stat.assume_init() };
        let kind = stat.st_mode & libc::S_IFMT;
        if kind != libc::S_IFREG && kind != libc::S_IFDIR {
            return Err(io::Error::other(
                "directory contains unsupported link/special entry",
            ));
        }
        entries.push(DirectoryEntry {
            name: name.to_owned(),
            bytes: stat.st_size.max(0) as u64,
            is_directory: kind == libc::S_IFDIR,
        });
        if scanned % 64 == 0 {
            progress(scanned, None);
        }
    }
    progress(scanned, Some(scanned));
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(entries)
}

#[cfg(windows)]
fn windows_list(directory: &PinnedDirectory, maximum: usize) -> io::Result<Vec<DirectoryEntry>> {
    windows_list_with_progress(directory, maximum, &mut |_, _| {})
}

#[cfg(windows)]
fn windows_list_with_progress(
    directory: &PinnedDirectory,
    maximum: usize,
    progress: &mut dyn FnMut(usize, Option<usize>),
) -> io::Result<Vec<DirectoryEntry>> {
    use std::{os::windows::io::AsRawHandle, slice};
    use windows_sys::Win32::{
        Foundation::{GetLastError, ERROR_NO_MORE_FILES},
        Storage::FileSystem::{
            FileIdBothDirectoryInfo, FileIdBothDirectoryRestartInfo, GetFileInformationByHandleEx,
            FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, FILE_ID_BOTH_DIR_INFO,
        },
    };

    const BUFFER_BYTES: usize = 64 * 1024;
    let root = directory.root.try_clone_file()?;
    let mut buffer = vec![0_u64; BUFFER_BYTES / std::mem::size_of::<u64>()];
    let mut entries = Vec::new();
    let mut scanned = 0;
    let mut restart = true;
    progress(0, None);
    loop {
        let information_class = if restart {
            FileIdBothDirectoryRestartInfo
        } else {
            FileIdBothDirectoryInfo
        };
        // SAFETY: `buffer` is writable and eight-byte aligned, the handle is a
        // pinned directory with list access, and the information class selects
        // the matching variable-length FILE_ID_BOTH_DIR_INFO records.
        let succeeded = unsafe {
            GetFileInformationByHandleEx(
                root.as_raw_handle() as _,
                information_class,
                buffer.as_mut_ptr().cast(),
                BUFFER_BYTES as u32,
            )
        };
        if succeeded == 0 {
            let error = unsafe { GetLastError() };
            if error == ERROR_NO_MORE_FILES {
                break;
            }
            return Err(io::Error::from_raw_os_error(error as i32));
        }
        restart = false;
        let mut offset = 0_usize;
        loop {
            let header_end = offset
                .checked_add(std::mem::size_of::<FILE_ID_BOTH_DIR_INFO>())
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "directory record overflow")
                })?;
            if header_end > BUFFER_BYTES {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "directory record exceeds buffer",
                ));
            }
            // SAFETY: the bounds check above leaves room for the fixed record;
            // read_unaligned accepts the API's byte-offset record alignment.
            let information = unsafe {
                buffer
                    .as_ptr()
                    .cast::<u8>()
                    .add(offset)
                    .cast::<FILE_ID_BOTH_DIR_INFO>()
                    .read_unaligned()
            };
            let name_bytes = information.FileNameLength as usize;
            let name_offset = std::mem::offset_of!(FILE_ID_BOTH_DIR_INFO, FileName);
            let name_end = offset
                .checked_add(name_offset)
                .and_then(|start| start.checked_add(name_bytes))
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "directory name overflow")
                })?;
            if name_bytes % 2 != 0 || name_end > BUFFER_BYTES {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid directory entry name length",
                ));
            }
            // SAFETY: the byte range is within the returned buffer and the
            // byte length was checked to contain whole UTF-16 code units.
            let name_units = unsafe {
                slice::from_raw_parts(
                    buffer
                        .as_ptr()
                        .cast::<u8>()
                        .add(offset + name_offset)
                        .cast::<u16>(),
                    name_bytes / 2,
                )
            };
            let name = String::from_utf16(name_units).map_err(io::Error::other)?;
            if !matches!(name.as_str(), "." | "..") {
                scanned += 1;
                if scanned > maximum {
                    return Err(io::Error::other("directory scan exceeds entry bound"));
                }
                validate_leaf(&name)?;
                if information.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                    return Err(io::Error::other(
                        "directory contains unsupported link/special entry",
                    ));
                }
                if information.FileAttributes & FILE_ATTRIBUTE_DIRECTORY == 0
                    && information.EndOfFile < 0
                {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "directory entry has a negative file size",
                    ));
                }
                entries.push(DirectoryEntry {
                    name,
                    bytes: information.EndOfFile.max(0) as u64,
                    is_directory: information.FileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0,
                });
                if scanned % 64 == 0 {
                    progress(scanned, None);
                }
            }
            let next = information.NextEntryOffset as usize;
            if next == 0 {
                break;
            }
            if next % 8 != 0
                || offset
                    .checked_add(next)
                    .is_none_or(|next| next >= BUFFER_BYTES)
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid next directory record offset",
                ));
            }
            offset += next;
        }
    }
    progress(scanned, Some(scanned));
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(entries)
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
mod tests {
    use super::*;
    #[test]
    fn pinned_directory_survives_path_replacement_and_rejects_links_and_fifo() {
        let fixture = tempfile::tempdir().unwrap();
        let original = fixture.path().join("selected");
        std::fs::create_dir(&original).unwrap();
        std::fs::write(original.join("original"), b"one").unwrap();
        let pinned = Arc::new(
            PinnedDirectory::from_host(Arc::new(NoFollowDirectory::open_root(&original).unwrap()))
                .unwrap(),
        );
        std::fs::rename(&original, fixture.path().join("old")).unwrap();
        std::fs::create_dir(&original).unwrap();
        std::fs::write(original.join("decoy"), b"two").unwrap();
        assert!(pinned.open_file("original").is_ok());
        assert!(pinned.open_file("decoy").is_err());
        std::os::unix::fs::symlink("original", fixture.path().join("old/link")).unwrap();
        assert!(pinned.open_file("link").is_err());
        let fifo =
            std::ffi::CString::new(fixture.path().join("old/fifo").to_str().unwrap()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        assert!(pinned.open_file("fifo").is_err());
        assert!(pinned.list(8).is_err());
    }
    #[test]
    fn atomic_create_refuses_existing_and_replace_truthfully_changes_inode() {
        let fixture = tempfile::tempdir().unwrap();
        let root = Arc::new(
            PinnedDirectory::from_host(Arc::new(
                NoFollowDirectory::open_root(fixture.path()).unwrap(),
            ))
            .unwrap(),
        );
        let mut initial = root.begin_atomic("value", WriteMode::CreateNew).unwrap();
        initial.write(b"first", 1024).unwrap();
        initial.prepare_durable().unwrap();
        let old = initial.publish_entry().unwrap();
        initial.durable_ack().unwrap();
        let mut duplicate = root.begin_atomic("value", WriteMode::CreateNew).unwrap();
        duplicate.write(b"bad", 1024).unwrap();
        assert!(duplicate.publish_entry().is_err());
        drop(duplicate);
        let mut replacement = root.begin_atomic("value", WriteMode::ReplaceEntry).unwrap();
        replacement.write(b"next", 1024).unwrap();
        replacement.prepare_durable().unwrap();
        let new = replacement.publish_entry().unwrap();
        replacement.durable_ack().unwrap();
        assert_ne!(old, new);
        let mut bytes = [0u8; 4];
        assert_eq!(
            root.open_file("value")
                .unwrap()
                .read_at(&mut bytes, 0)
                .unwrap(),
            4
        );
        assert_eq!(&bytes, b"next");
        assert_eq!(root.list(8).unwrap().len(), 1);
    }
    #[test]
    fn traversal_and_scanned_count_are_bounded() {
        let fixture = tempfile::tempdir().unwrap();
        let root = PinnedDirectory::from_host(Arc::new(
            NoFollowDirectory::open_root(fixture.path()).unwrap(),
        ))
        .unwrap();
        for name in ["../x", "a/b", "a\\b", "CON.txt", ".", ""] {
            assert!(root.open_file(name).is_err());
        }
        for name in ["a", "b", "c"] {
            std::fs::write(fixture.path().join(name), b"x").unwrap();
        }
        assert!(root.list(2).is_err());
        assert_eq!(root.list(3).unwrap().len(), 3);
        assert_eq!(root.list(3).unwrap().len(), 3);
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn saved_catalog_reports_directory_entries_while_scanning() {
        let fixture = tempfile::tempdir().unwrap();
        let root = PinnedDirectory::from_host(Arc::new(
            NoFollowDirectory::open_root(fixture.path()).unwrap(),
        ))
        .unwrap();
        for entry in 0..130 {
            std::fs::write(fixture.path().join(format!("entry-{entry}")), b"x").unwrap();
        }
        let mut updates = Vec::new();

        let entries = root
            .list_saved_catalog_with_progress(256, &mut |completed, total| {
                updates.push((completed, total));
            })
            .unwrap();

        assert_eq!(entries.len(), 130);
        assert_eq!(
            updates,
            [(0, None), (64, None), (128, None), (130, Some(130))]
        );
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn pinned_directory_identity_accepts_directory_handles() {
        use std::os::unix::fs::MetadataExt;

        let fixture = tempfile::tempdir().unwrap();
        let expected = std::fs::metadata(fixture.path()).unwrap();
        let root = PinnedDirectory::from_host(Arc::new(
            NoFollowDirectory::open_root(fixture.path()).unwrap(),
        ))
        .unwrap();

        assert_eq!(
            root.identity(),
            FileIdentity {
                device: expected.dev(),
                inode: expected.ino(),
            }
        );
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn remove_empty_child_requires_matching_device_and_inode() {
        let fixture = tempfile::tempdir().unwrap();
        let root = PinnedDirectory::from_host(Arc::new(
            NoFollowDirectory::open_root(fixture.path()).unwrap(),
        ))
        .unwrap();
        std::fs::create_dir(fixture.path().join("child")).unwrap();
        let child = root.child("child", false).unwrap();
        let identity = child.identity();

        let mismatched_device = FileIdentity {
            device: identity.device.wrapping_add(1),
            inode: identity.inode,
        };
        assert!(root.remove_empty_child("child", mismatched_device).is_err());
        assert!(fixture.path().join("child").is_dir());

        root.remove_empty_child("child", identity).unwrap();
        assert!(!fixture.path().join("child").exists());
    }
    #[test]
    fn namespace_mutation_lease_serializes_independent_root_views() {
        let fixture = tempfile::tempdir().unwrap();
        let handle = Arc::new(NoFollowDirectory::open_root(fixture.path()).unwrap());
        let root = PinnedDirectory::from_host(handle.clone()).unwrap();
        let other = PinnedDirectory::from_host(handle).unwrap();
        let lease = root.try_exclusive_lease().unwrap();
        assert_eq!(
            other.try_exclusive_lease().err().unwrap().kind(),
            io::ErrorKind::WouldBlock
        );
        drop(lease);
        assert!(other.try_exclusive_lease().is_ok());
    }
    #[test]
    fn pinned_ancestry_tracks_physical_parent_after_old_label_replacement() {
        let fixture = tempfile::tempdir().unwrap();
        let original = fixture.path().join("original");
        std::fs::create_dir(&original).unwrap();
        let root =
            PinnedDirectory::from_host(Arc::new(NoFollowDirectory::open_root(&original).unwrap()))
                .unwrap();
        let parent = PinnedDirectory::from_host(Arc::new(
            NoFollowDirectory::open_root(fixture.path()).unwrap(),
        ))
        .unwrap();
        std::fs::rename(&original, fixture.path().join("retained")).unwrap();
        std::fs::create_dir(&original).unwrap();
        let replacement =
            PinnedDirectory::from_host(Arc::new(NoFollowDirectory::open_root(&original).unwrap()))
                .unwrap();
        let ancestry = root.ancestor_identities(128).unwrap();
        assert_eq!(ancestry[0], root.identity());
        assert!(ancestry.contains(&parent.identity()));
        assert!(!ancestry.contains(&replacement.identity()));
        assert!(root.ancestor_identities(0).is_err());
        assert!(root.ancestor_identities(129).is_err());
        assert!(root.ancestor_identities(1).is_err());
    }
    #[test]
    fn pinned_atomic_abort_removes_original_stage_and_refuses_later_publication() {
        let fixture = tempfile::tempdir().unwrap();
        let root = Arc::new(
            PinnedDirectory::from_host(Arc::new(
                NoFollowDirectory::open_root(fixture.path()).unwrap(),
            ))
            .unwrap(),
        );
        let mut stage = root
            .begin_atomic("history", WriteMode::ReplaceEntry)
            .unwrap();
        stage.write(b"unfinished", 128).unwrap();
        stage.abort_unpublished().unwrap();
        stage.abort_unpublished().unwrap();
        assert!(!stage.was_published());
        assert!(stage.write(b"extra", 128).is_err());
        assert!(stage.publish_entry().is_err());
        assert!(stage.try_clone_staging_file().is_err());
        assert!(stage.open_staging_readonly().is_err());
        assert!(root.list(8).unwrap().is_empty());
    }
}

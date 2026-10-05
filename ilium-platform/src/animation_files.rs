//! Handle-relative animation storage. Linux atomic replacement swaps a directory
//! entry for a fresh 0600 inode; it does not preserve metadata or promise inode CAS.
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
#[cfg(not(target_os = "linux"))]
fn unsupported() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "animation atomic storage is qualified only on Linux",
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
#[cfg(target_os = "linux")]
fn identity(file: &File) -> io::Result<FileIdentity> {
    use std::os::unix::fs::MetadataExt;
    let metadata = file.metadata()?;
    Ok(FileIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}
#[cfg(not(target_os = "linux"))]
fn identity(_file: &File) -> io::Result<FileIdentity> {
    Err(unsupported())
}
pub struct PinnedFile {
    file: File,
    identity: FileIdentity,
}
impl PinnedFile {
    pub fn from_host(file: File) -> io::Result<Self> {
        if !file.metadata()?.is_file() {
            return Err(io::Error::other("selected handle is not a regular file"));
        }
        let identity = identity(&file)?;
        Ok(Self { file, identity })
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

    /// Positional reads preserve the shared selected handle's file offset.
    pub fn read_at(&self, out: &mut [u8], offset: u64) -> io::Result<usize> {
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::fs::FileExt;
            if identity(&self.file)? != self.identity {
                return Err(io::Error::other("selected file changed"));
            }
            self.file.read_at(out, offset)
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (out, offset);
            Err(unsupported())
        }
    }
}
pub struct PinnedDirectory {
    root: Arc<NoFollowDirectory>,
    identity: FileIdentity,
    mutations: Mutex<()>,
}
impl PinnedDirectory {
    pub fn from_host(root: Arc<NoFollowDirectory>) -> io::Result<Self> {
        #[cfg(target_os = "linux")]
        {
            let identity = identity(&root.try_clone_file()?)?;
            Ok(Self {
                root,
                identity,
                mutations: Mutex::new(()),
            })
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = root;
            Err(unsupported())
        }
    }
    pub fn identity(&self) -> FileIdentity {
        self.identity
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
        #[cfg(target_os = "linux")]
        {
            linux_list(self, maximum)
        }
        #[cfg(not(target_os = "linux"))]
        {
            Err(unsupported())
        }
    }
    /// Cooperative whole-namespace mutation lease, nonblocking. A fresh open
    /// description ensures clones of the same root do not bypass flock conflicts.
    pub fn try_exclusive_lease(&self) -> io::Result<DirectoryMutationLease> {
        #[cfg(target_os = "linux")]
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
        #[cfg(not(target_os = "linux"))]
        {
            Err(unsupported())
        }
    }
    /// Readers retain a shared inode lease so root-budget eviction cannot
    /// remove chunks while a playback window can still fault them in.
    pub fn try_shared_lease(&self) -> io::Result<DirectoryMutationLease> {
        #[cfg(target_os = "linux")]
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
        #[cfg(not(target_os = "linux"))]
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
        #[cfg(target_os = "linux")]
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
                || stat.st_dev != expected.device
                || stat.st_ino != expected.inode
            {
                return Err(io::Error::other("clip directory changed before removal"));
            }
            if unsafe { libc::unlinkat(root.as_raw_fd(), leaf.as_ptr(), libc::AT_REMOVEDIR) } != 0 {
                return Err(io::Error::last_os_error());
            }
            self.sync()
        }
        #[cfg(not(target_os = "linux"))]
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
        #[cfg(target_os = "linux")]
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
            let file = self.root.create_regular(OsStr::new(&temporary))?;
            let identity = identity(&file)?;
            Ok(AtomicFile {
                root: Arc::clone(self),
                file,
                temporary,
                leaf: leaf.to_owned(),
                mode,
                identity,
                published: false,
                bytes: 0,
            })
        }
        #[cfg(not(target_os = "linux"))]
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
    bytes: usize,
}
impl AtomicFile {
    pub fn write(&mut self, bytes: &[u8], maximum: usize) -> io::Result<()> {
        if self.published
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
        #[cfg(target_os = "linux")]
        {
            use std::os::fd::AsRawFd;
            let _guard = self
                .root
                .mutations
                .lock()
                .map_err(|_| io::Error::other("directory mutation guard poisoned"))?;
            if self.published || self.root.open_file(&self.temporary)?.identity() != self.identity {
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
            let flags = if matches!(self.mode, WriteMode::CreateNew) {
                libc::RENAME_NOREPLACE
            } else {
                0
            };
            // SAFETY: descriptors are pinned directories, CString leaves live through call.
            let result = unsafe {
                libc::renameat2(
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
        #[cfg(not(target_os = "linux"))]
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
        if !self.published {
            let _ = self
                .root
                .root
                .remove_regular(OsStr::new(&self.temporary), &self.file);
        }
    }
}
#[cfg(target_os = "linux")]
fn linux_list(directory: &PinnedDirectory, maximum: usize) -> io::Result<Vec<DirectoryEntry>> {
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
    loop {
        // SAFETY: only this owner uses DIR; errno reset distinguishes EOF/error.
        unsafe { *libc::__errno_location() = 0 };
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
    }
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(entries)
}

#[cfg(all(test, target_os = "linux"))]
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
}

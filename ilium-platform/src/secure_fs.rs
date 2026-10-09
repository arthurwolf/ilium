//! Directories and files that only their owner may read.
//!
//! ilium writes three kinds of user-private data: debug logs (which contain
//! terminal contents), session snapshots (which contain the workspace tree and
//! pane titles), and lock files under a shared temporary directory. All three
//! want the same guarantee, so the guarantee lives here once.
//!
//! On Unix that guarantee is explicit: mode `0o700` on directories, `0o600` on
//! files, `O_NOFOLLOW` so a pre-planted symlink in a world-writable directory
//! cannot redirect a write, `O_CLOEXEC` so a descriptor never leaks into a
//! spawned agent CLI, and an `lstat`-based check ahead of every `chmod` so a
//! symlink planted at the path itself (rather than encountered while opening
//! a file through it) is refused instead of silently chmod'd through.
//!
//! On Windows it is inherited: the paths involved live under the user's own
//! profile (see [`crate::runtime_dir`]), whose ACL already denies other
//! non-administrative users. Windows has no `chmod` equivalent that maps onto
//! those mode bits, and rewriting a DACL per file would be both slower and
//! easier to get wrong than relying on the profile's inherited permissions.

use std::fs::OpenOptions;
use std::io;
use std::path::Path;

/// Opens an existing regular file for reading, preserving ordinary symlink
/// resolution. Validate the opened handle rather than a raced path metadata
/// check. On Unix, a FIFO swapped into the path cannot block the open.
pub fn open_regular_file(path: &Path) -> io::Result<std::fs::File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC);
    }
    let file = options.open(path)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::other("read source is not a regular file"));
    }
    Ok(file)
}

/// Identifies the generation of an already-open file handle. Callers must
/// open the path first and derive this identity from that same handle so a
/// replacement between path lookup and identity capture cannot be mistaken
/// for the original file.
#[cfg(unix)]
pub fn file_generation(file: &std::fs::File) -> io::Result<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;

    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(io::Error::other("file generation requires a regular file"));
    }
    Ok((metadata.dev(), metadata.ino()))
}

/// Windows counterpart using the open handle's volume and stable file ID.
#[cfg(windows)]
pub fn file_generation(file: &std::fs::File) -> io::Result<(u64, u64)> {
    crate::nofollow_windows::file_generation(file)
}

#[cfg(not(any(unix, windows)))]
pub fn file_generation(_file: &std::fs::File) -> io::Result<(u64, u64)> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "file generation verification is unavailable",
    ))
}

/// Flushes the directory entry after publishing a file by rename. The file
/// itself must already have been flushed. An unsupported directory flush is
/// an error, never a successful durability acknowledgement.
pub fn sync_parent_directory(path: &Path) -> io::Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_CLOEXEC)
            .open(parent)?
            .sync_all()
    }
    #[cfg(not(unix))]
    {
        let _ = parent;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "directory flush requires platform durable replacement",
        ))
    }
}

/// Metadata acknowledgement level for directory creation/removal. Keep this
/// distinct from flushed file contents: unsupported directory fsync must not
/// be described as a successful flush, or disable native directory operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectorySyncOutcome {
    Synced,
    Unsupported,
}

pub fn sync_parent_directory_if_supported(path: &Path) -> io::Result<DirectorySyncOutcome> {
    match sync_parent_directory(path) {
        Ok(()) => Ok(DirectorySyncOutcome::Synced),
        Err(error) if error.kind() == io::ErrorKind::Unsupported => {
            Ok(DirectorySyncOutcome::Unsupported)
        }
        Err(error) => Err(error),
    }
}

/// Publishes an already flushed temporary file and waits for the platform's
/// durable replacement operation. Both paths must be on the same filesystem.
/// Also supports renaming a directory to a new path; the domain writer must
/// hold its mutation lock and validate destination conflicts before calling.
pub fn replace_file_durably(source: &Path, destination: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::{
            MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
        };
        let wide = |path: &Path| -> io::Result<Vec<u16>> {
            let mut value: Vec<u16> = path.as_os_str().encode_wide().collect();
            if value.contains(&0) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "path contains NUL",
                ));
            }
            value.push(0);
            Ok(value)
        };
        let source = wide(source)?;
        let destination = wide(destination)?;
        // SAFETY: both buffers are NUL-terminated and live through the call.
        if unsafe {
            MoveFileExW(
                source.as_ptr(),
                destination.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        std::fs::rename(source, destination)?;
        sync_parent_directory(destination)
    }
}

/// Creates `path` and any missing parents, restricted to the current user.
///
/// Every directory this creates -- the leaf *and* each intermediate parent --
/// is owner-only from the moment `mkdir` returns, never for a window
/// afterwards. Re-running this on an existing directory re-applies the
/// restriction to the leaf, which matters because the directory may have been
/// created by an older build (or by a user's own `mkdir`) with looser
/// permissions.
pub fn create_private_directory(path: &Path) -> io::Result<()> {
    create_directory_tree_privately(path)?;
    restrict_directory_to_owner(path)
}

/// Creates `path` and any missing parents, passing the owner-only mode to
/// `mkdir` itself rather than chmod'ing afterwards.
///
/// Two things go wrong when the tree is created with a plain
/// `std::fs::create_dir_all` and only the leaf is chmod'd afterwards. The leaf
/// exists at the process umask's default mode (typically `0o755`) for the
/// whole window between `mkdir` and that `chmod`, so anyone can read it -- or,
/// in a world-writable parent, plant files inside it -- for as long as the
/// window lasts. And the intermediate parents are never narrowed at all: a
/// project's `<project>/.ilium` on the way to `.ilium/sessions`, or the
/// per-user root on the way to its `logs` subdirectory, stayed world-listable
/// forever. Handing the mode to `mkdir` fixes both: the mode applies to every
/// component this call creates, and it applies atomically at creation.
///
/// `umask` can only clear bits, so the result is never wider than `0o700`.
#[cfg(unix)]
fn create_directory_tree_privately(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;

    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)
}

/// Windows has no mode argument to hand `mkdir`: directories created here
/// live under the user's own profile and inherit its ACL, which is what makes
/// them private in the first place. See the module comment.
#[cfg(not(unix))]
fn create_directory_tree_privately(path: &Path) -> io::Result<()> {
    std::fs::create_dir_all(path)
}

/// Restricts an existing directory to the current user.
#[cfg(unix)]
pub fn restrict_directory_to_owner(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    refuse_pre_existing_symlink(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
}

/// Windows directories under the user profile are already owner-private; see
/// the module comment for why no explicit ACL edit happens here.
#[cfg(not(unix))]
pub fn restrict_directory_to_owner(path: &Path) -> io::Result<()> {
    let _ = path;
    Ok(())
}

/// Restricts an existing file to the current user.
///
/// Callers use this after a create-then-rename sequence, where the final path
/// did not exist at open time and so could not be opened privately.
#[cfg(unix)]
pub fn restrict_file_to_owner(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    refuse_pre_existing_symlink(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
}

/// Refuses to act on a symlink already sitting at `path`.
///
/// `std::fs::set_permissions` follows symlinks, and so does the recursive
/// directory creation's "does this already exist" check (it falls
/// back to `path.is_dir()`, which resolves through a symlink and reports
/// success without creating anything). Together those two facts mean a
/// symlink pre-planted at a deterministic path in a world-writable directory
/// -- exactly what [`crate::runtime_dir::short_shared_directory`] resolves
/// under `/tmp` -- would make `create_private_directory` silently treat the
/// symlink's target as already private, then chmod that target instead of
/// failing. This lstat-based check sees the symlink itself and refuses
/// before `set_permissions` can be tricked into following it.
#[cfg(unix)]
fn refuse_pre_existing_symlink(path: &Path) -> io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(io::Error::other(format!(
            "refusing to follow pre-existing symlink at {}",
            path.display()
        ))),
        // A regular file or directory at the path is what the caller expects
        // to restrict.
        Ok(_) => Ok(()),

        // Nothing at the path is fine too: the caller may be about to create
        // it, and the follow-up operation produces the natural error if not.
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),

        // Any other lstat failure (EACCES, ELOOP, EIO, ...) means the symlink
        // question could not be answered at all -- propagate rather than
        // report "safe" for a path this guard never actually inspected.
        Err(error) => Err(error),
    }
}

/// See [`restrict_directory_to_owner`] for why Windows relies on inheritance.
#[cfg(not(unix))]
pub fn restrict_file_to_owner(path: &Path) -> io::Result<()> {
    let _ = path;
    Ok(())
}

/// Restricts an already-open file to the current user through its handle.
///
/// Operating on the descriptor (`fchmod` underneath) instead of the path
/// closes the race that [`restrict_file_to_owner`] cannot: nothing swapped in
/// at the path after the open can redirect this call, so callers that hold
/// the handle should always prefer this variant.
#[cfg(unix)]
pub fn restrict_open_file_to_owner(file: &std::fs::File) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    file.set_permissions(std::fs::Permissions::from_mode(0o600))
}

/// See [`restrict_directory_to_owner`] for why Windows relies on inheritance.
#[cfg(not(unix))]
pub fn restrict_open_file_to_owner(file: &std::fs::File) -> io::Result<()> {
    let _ = file;
    Ok(())
}

/// Restricts an existing file to the current user, with the owner's execute
/// bit set.
///
/// For a copied executable (a test fixture binary, an installed helper), not
/// a data file: use [`restrict_file_to_owner`] for anything that should not
/// run.
#[cfg(unix)]
pub fn restrict_executable_file_to_owner(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    refuse_pre_existing_symlink(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
}

/// Windows derives executability from the file extension rather than a
/// permission bit; see the module comment for why no explicit ACL edit
/// happens here either.
#[cfg(not(unix))]
pub fn restrict_executable_file_to_owner(path: &Path) -> io::Result<()> {
    let _ = path;
    Ok(())
}

/// Open options that produce an owner-only file, with the access mode left to
/// the caller: appending for a log, read-write for a lock, create-new for a
/// snapshot's temporary file.
///
/// Returning the builder rather than an opened file keeps every caller's
/// intent visible at its own call site while the privacy decision stays here.
#[cfg(unix)]
pub fn private_open_options() -> OpenOptions {
    use std::os::unix::fs::OpenOptionsExt;

    let mut options = OpenOptions::new();
    // `O_NOFOLLOW` refuses a symlink at the final path component, which is the
    // attack a world-writable parent directory invites. `O_CLOEXEC` keeps the
    // descriptor out of spawned agent CLIs.
    options
        .mode(0o600)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
    options
}

/// Windows has no `O_NOFOLLOW`/`O_CLOEXEC` equivalent to set here: handles are
/// non-inheritable by default, and reparse-point traversal is governed by the
/// directory's own ACL rather than a per-open flag.
#[cfg(not(unix))]
pub fn private_open_options() -> OpenOptions {
    OpenOptions::new()
}

/// A directory handle used to walk one child at a time without following
/// symbolic links. Worktree include copying uses this instead of resolving a
/// candidate path and opening it later, which would leave a parent-symlink race.
///
/// Unix uses `openat` with `O_NOFOLLOW`; Windows uses handle-relative
/// `NtCreateFile` and refuses every reparse point (see
/// `nofollow_windows`).
#[cfg(unix)]
pub struct NoFollowDirectory {
    file: std::fs::File,
}

#[cfg(windows)]
pub use crate::nofollow_windows::NoFollowDirectory;

#[cfg(not(any(unix, windows)))]
pub struct NoFollowDirectory;

/// Linux-only directory capability for crossing ancestors that grant search
/// permission without granting directory read/list permission.
///
/// This type is intentionally narrower than [`NoFollowDirectory`]. Its `O_PATH`
/// descriptor may only be used to continue a nofollow walk or to open the final
/// selected readable resource. It cannot be listed, synchronized, mutated, or
/// cloned into a selected-directory capability.
#[cfg(target_os = "linux")]
pub struct NoFollowTraversalDirectory {
    file: std::fs::File,
}

/// Whether this platform can safely create and verify worktree ownership
/// markers and copy included files without following substituted paths.
pub const fn supports_nofollow_directories() -> bool {
    cfg!(any(unix, windows))
}

/// Returns the device and inode of an exact canonical directory after opening
/// it without following a substituted final symlink.
#[cfg(unix)]
pub fn directory_generation(path: &Path) -> io::Result<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;

    if crate::paths::canonicalize(path)? != path {
        return Err(io::Error::other("directory path is not canonical"));
    }
    let directory = NoFollowDirectory::open_root(path)?;
    let metadata = directory.file.metadata()?;
    Ok((metadata.dev(), metadata.ino()))
}

/// Windows counterpart of the Unix proof: the volume serial and file ID of the
/// exact canonical directory, read from a handle opened without following a
/// substituted final link or junction.
#[cfg(windows)]
pub fn directory_generation(path: &Path) -> io::Result<(u64, u64)> {
    if crate::paths::canonicalize(path)? != path {
        return Err(io::Error::other("directory path is not canonical"));
    }
    NoFollowDirectory::open_root(path)?.generation()
}

#[cfg(not(any(unix, windows)))]
pub fn directory_generation(_path: &Path) -> io::Result<(u64, u64)> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "directory generation verification is unavailable",
    ))
}

/// An optional generation fence for ordinary pane spawning. Worktree
/// deletion itself still requires the strict `directory_generation` proof.
#[cfg(unix)]
pub fn spawn_directory_generation(path: &Path) -> io::Result<Option<(u64, u64)>> {
    directory_generation(path).map(Some)
}

/// Best effort on Windows: a launch directory that is not spelled in canonical
/// form (a pane may start anywhere the user can `cd`, not only in a worktree)
/// simply has no fence, exactly as every Windows launch did before the fence
/// existed. A canonical directory that is swapped between the two
/// observations still changes the answer and refuses the spawn.
#[cfg(windows)]
pub fn spawn_directory_generation(path: &Path) -> io::Result<Option<(u64, u64)>> {
    Ok(directory_generation(path).ok())
}

#[cfg(not(any(unix, windows)))]
pub fn spawn_directory_generation(_path: &Path) -> io::Result<Option<(u64, u64)>> {
    Ok(None)
}

#[cfg(target_os = "linux")]
impl NoFollowTraversalDirectory {
    /// Starts a traversal without requiring read/list access to the directory.
    ///
    /// The pre/post identity check gives this public root constructor the same
    /// changed-during-open refusal as `NoFollowDirectory::open_root`.
    pub fn open_root(path: &Path) -> io::Result<Self> {
        use std::os::fd::FromRawFd;
        use std::os::unix::fs::MetadataExt;

        let before = std::fs::symlink_metadata(path)?;
        if !before.file_type().is_dir() || before.file_type().is_symlink() {
            return Err(io::Error::other("traversal root must be a real directory"));
        }

        let path = unix_c_string(path.as_os_str())?;
        // `O_PATH` obtains pathname authority without requiring directory read
        // permission. Later relative lookups still require ordinary search
        // permission from the kernel.
        //
        // SAFETY: `path` is a valid NUL-terminated byte string. On success this
        // transfers ownership of the returned descriptor to `File`.
        let descriptor = unsafe {
            libc::open(
                path.as_ptr(),
                libc::O_PATH | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if descriptor < 0 {
            return Err(io::Error::last_os_error());
        }

        // SAFETY: successful `open` returned a fresh descriptor owned here.
        let file = unsafe { std::fs::File::from_raw_fd(descriptor) };
        let after = file.metadata()?;
        if !after.file_type().is_dir() || before.dev() != after.dev() || before.ino() != after.ino()
        {
            return Err(io::Error::other("traversal root changed while opening"));
        }

        Ok(Self { file })
    }

    /// Returns the physical identity of this already pinned traversal
    /// directory without upgrading it to a readable directory capability.
    ///
    /// `fstat` through an `O_PATH` descriptor needs neither directory listing
    /// permission nor pathname reopening. The descriptor remains suitable only
    /// for the restricted traversal operations exposed by this type.
    pub fn identity(&self) -> io::Result<(u64, u64)> {
        use std::os::unix::fs::MetadataExt;

        let metadata = self.file.metadata()?;
        if !metadata.file_type().is_dir() {
            return Err(io::Error::other("traversal handle is not a directory"));
        }

        Ok((metadata.dev(), metadata.ino()))
    }

    /// Continues the handle-relative walk through one real directory component.
    pub fn open_directory(&self, name: &std::ffi::OsStr) -> io::Result<Self> {
        use std::os::fd::{AsRawFd, FromRawFd};

        let name = unix_child_name(name)?;
        // `O_DIRECTORY` plus the post-open file-type check deliberately prevents
        // the `O_PATH | O_NOFOLLOW` symlink-descriptor special case from turning
        // a symlink into traversal authority.
        //
        // SAFETY: `self.file` is an owned live directory descriptor and `name`
        // is exactly one NUL-terminated path component.
        let descriptor = unsafe {
            libc::openat(
                self.file.as_raw_fd(),
                name.as_ptr(),
                libc::O_PATH | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if descriptor < 0 {
            return Err(io::Error::last_os_error());
        }

        // SAFETY: successful `openat` returned a fresh descriptor owned here.
        let file = unsafe { std::fs::File::from_raw_fd(descriptor) };
        if !file.metadata()?.file_type().is_dir() {
            return Err(io::Error::other("traversal child must be a real directory"));
        }

        Ok(Self { file })
    }

    /// Opens the selected directory as the ordinary readable directory
    /// capability required by listing, sync, leases, and `PinnedDirectory`.
    pub fn open_readable_directory(&self, name: &std::ffi::OsStr) -> io::Result<NoFollowDirectory> {
        use std::os::fd::{AsRawFd, FromRawFd};

        let name = unix_child_name(name)?;
        // SAFETY: the traversal descriptor is a pinned directory authority and
        // `name` is one validated child component.
        let descriptor = unsafe {
            libc::openat(
                self.file.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if descriptor < 0 {
            return Err(io::Error::last_os_error());
        }

        // SAFETY: successful `openat` returned a fresh descriptor owned here.
        let file = unsafe { std::fs::File::from_raw_fd(descriptor) };
        if !file.metadata()?.file_type().is_dir() {
            return Err(io::Error::other("selected child is not a directory"));
        }

        Ok(NoFollowDirectory { file })
    }

    /// Opens the selected file directly from a traversal-only parent.
    pub fn open_regular(&self, name: &std::ffi::OsStr) -> io::Result<std::fs::File> {
        use std::os::fd::{AsRawFd, FromRawFd};

        let name = unix_child_name(name)?;
        // Keep the ordinary selected-file contract: read-only, nofollow, and
        // nonblocking so a substituted FIFO cannot block the picker.
        //
        // SAFETY: the parent descriptor is live and `name` is one validated,
        // NUL-terminated path component.
        let descriptor = unsafe {
            libc::openat(
                self.file.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
            )
        };
        if descriptor < 0 {
            return Err(io::Error::last_os_error());
        }

        // SAFETY: successful `openat` returned a fresh descriptor owned here.
        let file = unsafe { std::fs::File::from_raw_fd(descriptor) };
        if !file.metadata()?.file_type().is_file() {
            return Err(io::Error::other("selected child is not a regular file"));
        }

        Ok(file)
    }
}

#[cfg(unix)]
impl NoFollowDirectory {
    /// Clone this already admitted directory handle for a read-only capability
    /// adapter. Both views retain the same root even if its pathname changes.
    pub fn try_clone_file(&self) -> io::Result<std::fs::File> {
        self.file.try_clone()
    }

    pub fn sync_all(&self) -> io::Result<()> {
        self.file.sync_all()
    }
    pub fn open_root(path: &Path) -> io::Result<Self> {
        use std::os::fd::FromRawFd;
        use std::os::unix::fs::MetadataExt;

        let before = std::fs::symlink_metadata(path)?;
        if !before.file_type().is_dir() || before.file_type().is_symlink() {
            return Err(io::Error::other("root must be a real directory"));
        }
        let path = unix_c_string(path.as_os_str())?;
        // SAFETY: `path` is a valid NUL-terminated byte string. On success,
        // this call transfers ownership of the returned descriptor to File.
        let descriptor = unsafe {
            libc::open(
                path.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if descriptor < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `open` returned a new descriptor owned by this process.
        let file = unsafe { std::fs::File::from_raw_fd(descriptor) };
        let after = file.metadata()?;
        if !after.file_type().is_dir() || before.dev() != after.dev() || before.ino() != after.ino()
        {
            return Err(io::Error::other("root changed while opening"));
        }
        Ok(Self { file })
    }

    pub fn open_directory(&self, name: &std::ffi::OsStr) -> io::Result<Self> {
        use std::os::fd::{AsRawFd, FromRawFd};

        let name = unix_child_name(name)?;
        // SAFETY: `self.file` is an open directory descriptor, and `name` is a
        // single NUL-terminated path component. File owns a successful result.
        let descriptor = unsafe {
            libc::openat(
                self.file.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if descriptor < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `openat` returned a new descriptor owned by this process.
        Ok(Self {
            file: unsafe { std::fs::File::from_raw_fd(descriptor) },
        })
    }

    /// Create a child directory, returning whether this call created it.
    /// Callers should record a newly created path before opening the child,
    /// since opening can itself fail after mkdir succeeds.
    pub fn create_directory_if_missing(&self, name: &std::ffi::OsStr) -> io::Result<bool> {
        use std::os::fd::AsRawFd;

        let name_c = unix_child_name(name)?;
        // SAFETY: the parent handle is an open directory and `name_c` is one
        // valid path component. The mode is owner-only from creation.
        let result = unsafe { libc::mkdirat(self.file.as_raw_fd(), name_c.as_ptr(), 0o700) };
        let created = if result == 0 {
            true
        } else {
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::AlreadyExists {
                return Err(error);
            }
            false
        };
        Ok(created)
    }

    pub fn open_regular(&self, name: &std::ffi::OsStr) -> io::Result<std::fs::File> {
        use std::os::fd::{AsRawFd, FromRawFd};

        let name = unix_child_name(name)?;
        // O_NONBLOCK makes a raced-in FIFO fail the subsequent regular-file
        // check instead of hanging the server worker inside openat.
        // SAFETY: `self.file` is an open directory and `name` is one valid
        // path component. File owns a successful descriptor.
        let descriptor = unsafe {
            libc::openat(
                self.file.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
            )
        };
        if descriptor < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `openat` returned a new descriptor owned by this process.
        let file = unsafe { std::fs::File::from_raw_fd(descriptor) };
        if !file.metadata()?.file_type().is_file() {
            return Err(io::Error::other("child is not a regular file"));
        }
        Ok(file)
    }

    pub fn create_regular(&self, name: &std::ffi::OsStr) -> io::Result<std::fs::File> {
        use std::os::fd::{AsRawFd, FromRawFd};

        let name = unix_child_name(name)?;
        // SAFETY: `self.file` is an open directory and `name` is one valid
        // path component. O_EXCL refuses any existing leaf, including links.
        let descriptor = unsafe {
            libc::openat(
                self.file.as_raw_fd(),
                name.as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o600,
            )
        };
        if descriptor < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `openat` returned a new descriptor owned by this process.
        Ok(unsafe { std::fs::File::from_raw_fd(descriptor) })
    }

    /// Remove a regular child only when it still names the supplied open file.
    /// This keeps the parent traversal pinned and refuses a substituted leaf
    /// observed at the identity check. POSIX has no atomic compare-and-unlink;
    /// callers must still serialize mutations of this directory.
    pub fn remove_regular(
        &self,
        name: &std::ffi::OsStr,
        expected: &std::fs::File,
    ) -> io::Result<()> {
        use std::os::fd::AsRawFd;
        use std::os::unix::fs::MetadataExt;

        let name = unix_child_name(name)?;
        let expected = expected.metadata()?;
        if !expected.file_type().is_file() {
            return Err(io::Error::other("expected handle is not a regular file"));
        }
        let mut current = std::mem::MaybeUninit::<libc::stat>::uninit();
        // SAFETY: `current` points to writable storage for one stat result;
        // the directory descriptor and child name are valid for this call.
        let status = unsafe {
            libc::fstatat(
                self.file.as_raw_fd(),
                name.as_ptr(),
                current.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        };
        if status < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: successful fstatat initialized the stat value.
        let current = unsafe { current.assume_init() };
        if current.st_mode & libc::S_IFMT != libc::S_IFREG
            || !metadata_id_matches(current.st_dev, expected.dev())
            || !metadata_id_matches(current.st_ino, expected.ino())
        {
            return Err(io::Error::other("regular child changed before removal"));
        }
        // SAFETY: the directory descriptor and child name are valid. A
        // concurrent mutation after fstatat is outside this method's guard.
        if unsafe { libc::unlinkat(self.file.as_raw_fd(), name.as_ptr(), 0) } < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

#[cfg(unix)]
fn metadata_id_matches<T>(current: T, expected: u64) -> bool
where
    T: TryFrom<u64> + PartialEq,
{
    T::try_from(expected).is_ok_and(|identity| current == identity)
}

#[cfg(unix)]
fn unix_c_string(value: &std::ffi::OsStr) -> io::Result<std::ffi::CString> {
    use std::os::unix::ffi::OsStrExt;

    std::ffi::CString::new(value.as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path contains NUL"))
}

#[cfg(unix)]
fn unix_child_name(name: &std::ffi::OsStr) -> io::Result<std::ffi::CString> {
    use std::os::unix::ffi::OsStrExt;

    let bytes = name.as_bytes();
    if bytes.is_empty() || bytes == b"." || bytes == b".." || bytes.contains(&b'/') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid child path component",
        ));
    }
    unix_c_string(name)
}

#[cfg(not(any(unix, windows)))]
impl NoFollowDirectory {
    pub fn try_clone_file(&self) -> io::Result<std::fs::File> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "safe directory-handle cloning is unavailable on this platform",
        ))
    }

    pub fn sync_all(&self) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "directory durability is unavailable",
        ))
    }
    pub fn open_root(_path: &Path) -> io::Result<Self> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "safe worktree include copying is unavailable on this platform",
        ))
    }

    pub fn open_directory(&self, _name: &std::ffi::OsStr) -> io::Result<Self> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "safe worktree include copying is unavailable on this platform",
        ))
    }

    pub fn create_directory_if_missing(&self, _name: &std::ffi::OsStr) -> io::Result<bool> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "safe worktree include copying is unavailable on this platform",
        ))
    }

    pub fn open_regular(&self, _name: &std::ffi::OsStr) -> io::Result<std::fs::File> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "safe worktree include copying is unavailable on this platform",
        ))
    }

    pub fn create_regular(&self, _name: &std::ffi::OsStr) -> io::Result<std::fs::File> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "safe worktree include copying is unavailable on this platform",
        ))
    }

    pub fn remove_regular(
        &self,
        _name: &std::ffi::OsStr,
        _expected: &std::fs::File,
    ) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "safe worktree include copying is unavailable on this platform",
        ))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn durable_replacement_preserves_flushed_contents_on_readback() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let destination = directory.path().join("saved");
        let source = directory.path().join("pending");
        std::fs::write(&destination, b"previous").expect("previous file");
        std::fs::write(&source, b"replacement").expect("pending file");
        std::fs::File::open(&source)
            .expect("pending handle")
            .sync_all()
            .expect("flush contents");
        super::replace_file_durably(&source, &destination).expect("durable replacement");
        assert_eq!(
            std::fs::read(&destination).expect("saved readback"),
            b"replacement"
        );
        assert!(!source.exists());
    }

    #[cfg(unix)]
    #[test]
    fn regular_read_follows_symlinks_but_rejects_fifo_without_a_writer() {
        use std::os::unix::ffi::OsStrExt;
        let directory = tempfile::tempdir().expect("temporary directory");
        let source = directory.path().join("image");
        let link = directory.path().join("linked-image");
        std::fs::write(&source, b"image bytes").expect("image");
        std::os::unix::fs::symlink(&source, &link).expect("symlink");
        assert!(super::open_regular_file(&link).is_ok());
        let fifo = directory.path().join("fifo");
        let name = std::ffi::CString::new(fifo.as_os_str().as_bytes()).expect("FIFO path");
        // SAFETY: NUL-terminated task-owned path, no existing object there.
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        assert!(super::open_regular_file(&fifo).is_err());
        assert!(super::open_regular_file(directory.path()).is_err());
    }
    use super::*;

    #[cfg(unix)]
    #[test]
    fn nofollow_directory_stays_on_open_parent_after_path_is_replaced() {
        use std::io::Write;
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().expect("temp dir");
        let root = temp.path().join("root");
        let outside = temp.path().join("outside");
        let moved = temp.path().join("moved");
        std::fs::create_dir(&root).expect("root");
        std::fs::create_dir(root.join("child")).expect("child");
        std::fs::create_dir(&outside).expect("outside");

        let root_handle = NoFollowDirectory::open_root(&root).expect("root handle");
        let child_handle = root_handle
            .open_directory("child".as_ref())
            .expect("child handle");
        std::fs::rename(root.join("child"), &moved).expect("rename child");
        symlink(&outside, root.join("child")).expect("replace with symlink");

        assert!(root_handle.open_directory("child".as_ref()).is_err());
        child_handle
            .create_regular("safe".as_ref())
            .expect("create in held directory")
            .write_all(b"safe")
            .expect("write");
        assert_eq!(std::fs::read(moved.join("safe")).expect("safe"), b"safe");
        assert!(!outside.join("safe").exists());
    }

    #[cfg(unix)]
    #[test]
    fn nofollow_directory_rejects_symlink_leaf_and_special_file() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().expect("temp dir");
        let root = NoFollowDirectory::open_root(temp.path()).expect("root handle");
        std::fs::write(temp.path().join("real"), "real").expect("real");
        symlink("real", temp.path().join("linked")).expect("link");
        assert!(root.open_regular("linked".as_ref()).is_err());
        assert!(root.create_regular("linked".as_ref()).is_err());
        let fifo_path = unix_c_string(temp.path().join("pipe").as_os_str()).expect("fifo path");
        // SAFETY: `fifo_path` is a valid NUL-terminated path owned by this test.
        assert_eq!(unsafe { libc::mkfifo(fifo_path.as_ptr(), 0o600) }, 0);
        assert!(root.open_regular("pipe".as_ref()).is_err());
        assert!(root.open_regular(".".as_ref()).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn nofollow_remove_rejects_replaced_leaf() {
        let temp = tempfile::tempdir().expect("temp dir");
        let root = NoFollowDirectory::open_root(temp.path()).expect("root handle");
        let marker = temp.path().join("marker");
        let moved = temp.path().join("moved");
        std::fs::write(&marker, "owned").expect("marker");
        let owned = root.open_regular("marker".as_ref()).expect("owned handle");
        std::fs::rename(&marker, &moved).expect("rename");
        std::fs::write(&marker, "other").expect("replacement");
        assert!(root.remove_regular("marker".as_ref(), &owned).is_err());
        assert_eq!(
            std::fs::read(&marker).expect("replacement survives"),
            b"other"
        );
        root.remove_regular(
            "marker".as_ref(),
            &root
                .open_regular("marker".as_ref())
                .expect("replacement handle"),
        )
        .expect("remove exact replacement");
        assert!(!marker.exists());
    }

    #[test]
    fn create_private_directory_is_idempotent_and_restricts_an_existing_directory() {
        let root = tempfile::tempdir().expect("temp dir");
        let nested = root.path().join("outer").join("inner");

        create_private_directory(&nested).expect("first creation");
        // A second call must succeed on the already-created path, because the
        // session resolver runs it on every attach, not only the first.
        create_private_directory(&nested).expect("second creation");

        assert!(nested.is_dir());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&nested)
                .expect("metadata")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o700);
        }
    }

    /// Regression test: only the leaf used to be restricted, so the
    /// intermediate directories `create_dir_all` made on the way to it -- a
    /// project's `.ilium` on the way to `.ilium/sessions`, the per-user root
    /// on the way to its `logs` -- kept whatever world-listable mode the
    /// umask gave them, exposing session and log *names* to every other user
    /// on the machine even though the files inside were `0o600`.
    #[cfg(unix)]
    #[test]
    fn create_private_directory_restricts_the_parents_it_creates_too() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().expect("temp dir");
        let outer = root.path().join("outer");
        let nested = outer.join("inner");

        create_private_directory(&nested).expect("creation");

        let outer_mode = std::fs::metadata(&outer)
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(
            outer_mode, 0o700,
            "an intermediate directory created on the way to the leaf must be owner-only too"
        );
    }

    #[test]
    fn private_open_options_creates_an_owner_only_file() {
        let root = tempfile::tempdir().expect("temp dir");
        let path = root.path().join("private.log");

        let file = private_open_options()
            .create(true)
            .append(true)
            .open(&path)
            .expect("open private file");
        drop(file);

        assert!(path.is_file());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path)
                .expect("metadata")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600);
        }
    }

    #[test]
    fn restrict_file_to_owner_tightens_a_file_created_by_a_rename() {
        let root = tempfile::tempdir().expect("temp dir");
        let path = root.path().join("renamed.json");
        std::fs::write(&path, b"{}").expect("write file");

        restrict_file_to_owner(&path).expect("restrict");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path)
                .expect("metadata")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600);
        }
    }

    /// Reproduces the attack the module doc calls out: a symlink pre-planted
    /// at ilium's deterministic path in a world-writable directory, pointing
    /// at a directory the attacker controls. `create_dir_all` alone would
    /// silently treat the symlink's target as "already exists" and
    /// `set_permissions` would chmod that target -- this asserts both that
    /// the call is refused and that the decoy's own permissions are left
    /// untouched, since a refusal that still mutated the target would not
    /// actually close the hole.
    #[cfg(unix)]
    #[test]
    fn create_private_directory_refuses_a_pre_planted_symlink() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().expect("temp dir");
        let decoy = root.path().join("attacker-owned");
        std::fs::create_dir(&decoy).expect("create decoy");
        std::fs::set_permissions(&decoy, std::fs::Permissions::from_mode(0o755))
            .expect("set decoy permissions");
        let victim_path = root.path().join("ilium-socket-dir");
        std::os::unix::fs::symlink(&decoy, &victim_path).expect("plant symlink");

        let result = create_private_directory(&victim_path);

        assert!(result.is_err(), "a pre-planted symlink must be refused");
        let decoy_mode = std::fs::metadata(&decoy)
            .expect("decoy metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(
            decoy_mode, 0o755,
            "the decoy directory must not be chmod'd through the symlink"
        );
    }
}

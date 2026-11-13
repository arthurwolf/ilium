//! Linux inode custody for a future sandboxed Codex goal read.
//!
//! This does not itself read or qualify a goal. It pins all three SQLite WAL
//! files without updating access times and detects directory entry changes.

use std::ffi::CString;
use std::fs::File;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, RawFd};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

const NAMES: [&str; 3] = ["goals_1.sqlite", "goals_1.sqlite-wal", "goals_1.sqlite-shm"];

/// Keeps the database, WAL and shared-memory inodes open until the caller has
/// finished its read and checked `unchanged`. Missing prerequisites fail closed.
pub struct PinnedCodexGoalFiles {
    directory: File,
    watch: File,
    files: [File; 3],
    invalidated: std::sync::atomic::AtomicBool,
}

fn file_from_descriptor(descriptor: RawFd) -> io::Result<File> {
    if descriptor < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: a successful open or inotify_init returned this owned descriptor.
    Ok(unsafe { File::from_raw_fd(descriptor) })
}

fn identity(file: &File) -> io::Result<libc::stat> {
    // SAFETY: zero is a valid initial value for the output buffer of fstat.
    let mut stat: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: the descriptor is owned and stat points to writable storage.
    if unsafe { libc::fstat(file.as_raw_fd(), &mut stat) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(stat)
}

impl PinnedCodexGoalFiles {
    pub fn open(provider_home: &Path) -> io::Result<Self> {
        if !provider_home.is_absolute() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "provider home is not absolute",
            ));
        }
        let path = CString::new(provider_home.as_os_str().as_bytes()).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidInput, "provider home contains NUL")
        })?;
        // SAFETY: path is a terminated C string; flags do not create or change files.
        let directory = file_from_descriptor(unsafe {
            libc::open(
                path.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        })?;
        // SAFETY: inotify_init1 returns a new owned descriptor on success.
        let watch = file_from_descriptor(unsafe {
            libc::inotify_init1(libc::IN_CLOEXEC | libc::IN_NONBLOCK)
        })?;
        let watch_path =
            CString::new(format!("/proc/self/fd/{}", directory.as_raw_fd())).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidInput, "invalid directory descriptor")
            })?;
        let mask = libc::IN_CREATE
            | libc::IN_DELETE
            | libc::IN_MOVED_FROM
            | libc::IN_MOVED_TO
            | libc::IN_DELETE_SELF
            | libc::IN_MOVE_SELF;
        // SAFETY: watch_path is a terminated C string and the watch is owned.
        if unsafe { libc::inotify_add_watch(watch.as_raw_fd(), watch_path.as_ptr(), mask) } < 0 {
            return Err(io::Error::last_os_error());
        }
        let open_file = |name: &str| -> io::Result<File> {
            let name = CString::new(name).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidInput, "invalid database filename")
            })?;
            // O_NOATIME is required: ordinary read-only open can update user data.
            // SAFETY: directory and name remain valid and no creation flag is used.
            let file = file_from_descriptor(unsafe {
                libc::openat(
                    directory.as_raw_fd(),
                    name.as_ptr(),
                    libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NOATIME | libc::O_CLOEXEC,
                )
            })?;
            let stat = identity(&file)?;
            // SAFETY: geteuid has no preconditions.
            if stat.st_mode & libc::S_IFMT != libc::S_IFREG
                || stat.st_uid != unsafe { libc::geteuid() }
            {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "provider file is not an owned regular file",
                ));
            }
            Ok(file)
        };
        let files = [
            open_file(NAMES[0])?,
            open_file(NAMES[1])?,
            open_file(NAMES[2])?,
        ];
        let mut pinned = Self {
            directory,
            watch,
            files,
            invalidated: std::sync::atomic::AtomicBool::new(false),
        };
        pinned.unchanged()?;
        Ok(pinned)
    }

    /// Descriptors are borrowed; a launcher must explicitly transfer only
    /// these and ensure no writable descriptor survives into the payload.
    pub fn descriptors(&self) -> [RawFd; 3] {
        self.files.each_ref().map(AsRawFd::as_raw_fd)
    }

    /// Detect replacement, including rename-away-and-back, before accepting a
    /// result. Queue overflow, directory movement or any entry change fails.
    pub fn unchanged(&mut self) -> io::Result<()> {
        use std::sync::atomic::Ordering;
        if self.invalidated.load(Ordering::Acquire) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "provider directory was invalidated",
            ));
        }
        let result = self.check_unchanged();
        if result.is_err() {
            self.invalidated.store(true, Ordering::Release);
        }
        result
    }

    fn check_unchanged(&self) -> io::Result<()> {
        let mut events = [0u8; 4096];
        // SAFETY: the buffer is writable, watch is nonblocking and owned.
        let count = unsafe {
            libc::read(
                self.watch.as_raw_fd(),
                events.as_mut_ptr().cast(),
                events.len(),
            )
        };
        if count >= 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "provider directory changed or watch closed",
            ));
        }
        if count < 0 {
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::WouldBlock {
                return Err(error);
            }
        }
        for (name, file) in NAMES.iter().zip(&self.files) {
            let name = CString::new(*name).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidInput, "invalid database filename")
            })?;
            // SAFETY: zero is a valid initial value for the fstatat output buffer.
            let mut current: libc::stat = unsafe { std::mem::zeroed() };
            // SAFETY: the directory descriptor, name and output buffer are valid.
            if unsafe {
                libc::fstatat(
                    self.directory.as_raw_fd(),
                    name.as_ptr(),
                    &mut current,
                    libc::AT_SYMLINK_NOFOLLOW,
                )
            } != 0
            {
                return Err(io::Error::last_os_error());
            }
            let pinned = identity(file)?;
            if current.st_dev != pinned.st_dev
                || current.st_ino != pinned.st_ino
                || current.st_mode & libc::S_IFMT != libc::S_IFREG
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "provider file identity changed",
                ));
            }
        }
        // Catch directory changes that occurred while the names were checked.
        // SAFETY: the buffer is writable, watch is nonblocking and owned.
        let count = unsafe {
            libc::read(
                self.watch.as_raw_fd(),
                events.as_mut_ptr().cast(),
                events.len(),
            )
        };
        if count >= 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "provider directory changed or watch closed",
            ));
        }
        if count < 0 {
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::WouldBlock {
                return Err(error);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_database_refuses_missing_files_and_symlinks() {
        use std::os::unix::fs::symlink;
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join(NAMES[0]), b"synthetic").unwrap();
        assert!(PinnedCodexGoalFiles::open(directory.path()).is_err());
        std::fs::write(directory.path().join(NAMES[2]), b"synthetic").unwrap();
        symlink(
            directory.path().join(NAMES[0]),
            directory.path().join(NAMES[1]),
        )
        .unwrap();
        assert!(PinnedCodexGoalFiles::open(directory.path()).is_err());
    }

    #[test]
    fn pinned_database_rejects_rename_away_and_back() {
        let directory = tempfile::tempdir().unwrap();
        for name in NAMES {
            std::fs::write(directory.path().join(name), b"synthetic").unwrap();
        }
        let mut files = PinnedCodexGoalFiles::open(directory.path()).unwrap();
        assert!(files.unchanged().is_ok());
        let database = directory.path().join(NAMES[0]);
        let moved = directory.path().join("temporary");
        std::fs::rename(&database, &moved).unwrap();
        std::fs::rename(&moved, &database).unwrap();
        assert!(files.unchanged().is_err());
        assert!(files.unchanged().is_err());
    }
}

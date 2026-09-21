//! One cross-process exclusive lock, held for a guard's lifetime.
//!
//! The CLI serializes competing first-attach processes with it: the socket
//! recheck, the log-path publication, and the detached server spawn have to
//! run as one step, or two clients racing to attach to the same project start
//! rival servers writing different logs.
//!
//! `std::fs::File::lock` provides this on every supported platform (`flock` on
//! Unix, `LockFileEx` on Windows), so no dependency and no `#[cfg]` is needed
//! -- the lock file itself is created through [`crate::secure_fs`] so it keeps
//! the same owner-only guarantee as everything else ilium writes.

use std::fs::File;
use std::io;
use std::path::Path;

use crate::secure_fs;

/// An exclusive lock over one path, released when this guard drops.
///
/// Dropping unlocks explicitly and then closes the file; closing alone would
/// also release the lock, but an explicit unlock keeps the release ordered
/// relative to the guard's own scope rather than to the file's last handle.
#[derive(Debug)]
pub struct ExclusiveFileLock {
    file: File,
}

impl ExclusiveFileLock {
    /// Blocks until this process owns the lock at `path`, creating the lock
    /// file if it does not exist.
    ///
    /// A lock file is never truncated: its contents are irrelevant, and
    /// truncating one would momentarily disturb a concurrent holder's view of
    /// a file it is entitled to assume is stable.
    pub fn acquire(path: &Path) -> io::Result<Self> {
        // A bare relative path (`start.lock`) yields an empty parent, which is
        // the current directory and already exists: creating "" would fail on
        // the follow-up chmod rather than do anything useful.
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty());
        if let Some(parent) = parent {
            secure_fs::create_private_directory(parent)?;
        }
        let file = secure_fs::private_open_options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        // An older build may have left the lock file world-readable; tighten
        // it now rather than trusting whatever mode it was created with. This
        // goes through the open handle, not the path: a path-based chmod would
        // reopen the name and so reintroduce exactly the swap-in race the
        // `O_NOFOLLOW` open above just refused.
        secure_fs::restrict_open_file_to_owner(&file)?;
        file.lock()?;
        Ok(Self { file })
    }
}

impl Drop for ExclusiveFileLock {
    fn drop(&mut self) {
        // A failed unlock is not actionable: the handle closes immediately
        // afterwards, which releases the lock regardless, and turning this
        // into a panic would abort a caller whose real work already
        // succeeded.
        let _ = self.file.unlock();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acquire_creates_the_lock_file_and_its_parent_directory() {
        let root = tempfile::tempdir().expect("temp dir");
        let path = root.path().join("state").join("start.lock");

        let lock = ExclusiveFileLock::acquire(&path).expect("acquire");

        assert!(path.is_file());
        drop(lock);
    }

    #[test]
    fn a_released_lock_can_be_acquired_again() {
        let root = tempfile::tempdir().expect("temp dir");
        let path = root.path().join("start.lock");

        let first = ExclusiveFileLock::acquire(&path).expect("first acquire");
        drop(first);
        let second = ExclusiveFileLock::acquire(&path).expect("second acquire");
        drop(second);
    }

    #[test]
    fn a_held_lock_blocks_another_process_until_it_is_released() {
        // Threads share one file table, and `flock` ownership is per open
        // file description rather than per thread, so a second *process* is
        // the only honest way to observe exclusion. The child re-opens the
        // same path and reports how long it waited.
        let root = tempfile::tempdir().expect("temp dir");
        let path = root.path().join("contended.lock");
        let held = ExclusiveFileLock::acquire(&path).expect("parent acquires");

        let mut child = std::process::Command::new(std::env::current_exe().expect("test binary"))
            .args([
                "--exact",
                "--nocapture",
                "--ignored",
                "file_lock::tests::lock_child_helper",
            ])
            .env("ILIUM_LOCK_TEST_PATH", &path)
            .spawn()
            .expect("spawn child");

        // Give the child long enough to reach its blocking acquire, then
        // confirm it is still waiting before releasing. Without this check,
        // a broken (non-exclusive) lock would let the child finish almost
        // instantly, but `child.wait()` below would not be called until
        // after this thread's own sleep anyway -- so only polling before the
        // release actually distinguishes "blocked, then released" from
        // "never blocked at all".
        std::thread::sleep(std::time::Duration::from_millis(300));
        assert!(
            child.try_wait().expect("poll child").is_none(),
            "child acquired the lock while the parent still held it"
        );
        drop(held);

        let status = child.wait().expect("child exits");
        assert!(status.success(), "child failed to acquire after release");
    }

    /// Child half of `a_held_lock_blocks_another_process_until_it_is_released`.
    /// `#[ignore]` keeps it out of ordinary runs; the parent invokes it by
    /// name with `--ignored`.
    #[test]
    #[ignore = "helper process invoked by the contention test"]
    fn lock_child_helper() {
        // `var_os`, not `var`: a temporary directory under a non-UTF-8
        // `TMPDIR` is a perfectly ordinary path this helper must still handle.
        let path = std::env::var_os("ILIUM_LOCK_TEST_PATH").expect("lock path from parent");
        let lock = ExclusiveFileLock::acquire(Path::new(&path)).expect("child acquires");
        drop(lock);
    }
}

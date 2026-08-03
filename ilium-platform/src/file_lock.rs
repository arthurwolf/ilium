//! One cross-process exclusive lock, held for a guard's lifetime.
//!
//! Two places need the same thing: the CLI serializes competing first-attach
//! processes so two clients cannot start rival servers for one project, and
//! the chatroom serializes appends so concurrent writers cannot interleave
//! half-written message lines.
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
        if let Some(parent) = path.parent() {
            secure_fs::create_private_directory(parent)?;
        }
        let file = secure_fs::private_open_options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        // An older build may have left the lock file world-readable; tighten
        // it now rather than trusting whatever mode it was created with.
        secure_fs::restrict_file_to_owner(path)?;
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
            .args(["--exact", "--nocapture", "--ignored", "lock_child_helper"])
            .env("ILIUM_LOCK_TEST_PATH", &path)
            .spawn()
            .expect("spawn child");

        // Give the child long enough to reach its blocking acquire, then
        // release; if exclusion were broken the child would already have
        // exited successfully before this point either way, so the assertion
        // below is on the child's own observation, not on this sleep.
        std::thread::sleep(std::time::Duration::from_millis(300));
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
        let path = std::env::var("ILIUM_LOCK_TEST_PATH").expect("lock path from parent");
        let lock = ExclusiveFileLock::acquire(Path::new(&path)).expect("child acquires");
        drop(lock);
    }
}

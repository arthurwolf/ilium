//! Elects one attached TUI process to execute automatic trigger actions.
//!
//! Every client attached to a session receives the same semantic server events.
//! A kernel file lock beside that session's Unix socket turns those broadcasts
//! into at-most-once LLM work without moving provider credentials or client
//! configuration into `ilium-server`. The lock is released automatically when
//! its owning client exits; another already-attached client claims it on the
//! next trigger occurrence.

use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

/// Process-local handle for the session's automatic-action execution lease.
pub struct TriggerExecutionLease {
    file: Option<File>,
    is_owner: bool,
}

impl TriggerExecutionLease {
    /// Opens the stable lease file derived from the already-resolved socket.
    ///
    /// Failure is non-fatal to the TUI, but automatic actions remain disabled
    /// for this client because running them without exclusion could duplicate
    /// paid inference and conflicting structure mutations.
    pub fn open(socket_path: &Path) -> Self {
        let path = lease_path(socket_path);
        let file = match OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
        {
            Ok(file) => Some(file),
            Err(error) => {
                tracing::warn!(
                    path = %path.display(),
                    %error,
                    "automatic trigger actions disabled because the session lease could not open"
                );
                None
            }
        };
        Self {
            file,
            is_owner: false,
        }
    }

    /// Returns whether this process owns, or just acquired, the session lease.
    pub fn claim(&mut self) -> bool {
        if self.is_owner {
            return true;
        }
        let Some(file) = &self.file else {
            return false;
        };
        match file.try_lock() {
            Ok(()) => {
                self.is_owner = true;
                true
            }
            Err(std::fs::TryLockError::WouldBlock) => false,
            Err(std::fs::TryLockError::Error(error)) => {
                tracing::warn!(
                    %error,
                    "automatic trigger actions disabled because the session lease could not lock"
                );
                false
            }
        }
    }
}

/// Keeps the lease identity one-to-one with the canonical session socket.
fn lease_path(socket_path: &Path) -> PathBuf {
    socket_path.with_extension("automatic-actions.lock")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_one_client_executes_and_another_takes_over_after_drop() {
        let directory = tempfile::tempdir().unwrap();
        let socket_path = directory.path().join("project-default.sock");
        let mut first = TriggerExecutionLease::open(&socket_path);
        let mut second = TriggerExecutionLease::open(&socket_path);

        assert!(first.claim());
        assert!(!second.claim());

        drop(first);
        assert!(second.claim());
    }
}

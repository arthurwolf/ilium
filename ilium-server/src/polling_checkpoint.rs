//! Durable per-agent spacing checkpoints for the elected polling detector.
//!
//! Only the wall-clock time of the last admitted check is stored, keyed by the
//! non-secret agent identity. After a detector restart this keeps the minimum
//! spacing intact, so a restart can never produce a burst of paid checks. No
//! transcript text, credential or provider response is written here.
use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const FILE_NAME: &str = "polling-checkpoints.json";
const MAXIMUM_ENTRIES: usize = 4096;
const MAXIMUM_BYTES: u64 = 1024 * 1024;
const RETENTION_SECONDS: u64 = 24 * 60 * 60;

#[derive(Debug, Default, Serialize, Deserialize)]
struct Stored {
    version: u32,
    last_check_unix_seconds: BTreeMap<String, u64>,
}

/// In-memory copy of the checkpoint file; persistence is explicit and runs on
/// an admitted I/O worker supplied by the caller.
#[derive(Debug, Default, Clone)]
pub struct Checkpoints {
    last_check_unix_seconds: BTreeMap<String, u64>,
}

impl Checkpoints {
    pub fn path(directory: &Path) -> PathBuf {
        directory.join(FILE_NAME)
    }

    /// A missing file is an empty checkpoint set; a damaged one fails closed to
    /// empty as well, because a checkpoint only ever delays a check.
    pub fn load(directory: &Path, now_unix_seconds: u64) -> Self {
        let path = Self::path(directory);
        let Ok(file) = ilium_platform::secure_fs::open_private_regular_file(
            &path,
            ilium_platform::secure_fs::PrivateFileMode::Read,
        ) else {
            return Self::default();
        };
        let mut bytes = Vec::new();
        if io::Read::read_to_end(&mut io::Read::take(&file, MAXIMUM_BYTES + 1), &mut bytes).is_err()
            || bytes.len() as u64 > MAXIMUM_BYTES
        {
            return Self::default();
        }
        let Ok(stored) = serde_json::from_slice::<Stored>(&bytes) else {
            return Self::default();
        };
        let mut checkpoints = Self {
            last_check_unix_seconds: stored.last_check_unix_seconds,
        };
        checkpoints.prune(now_unix_seconds);
        checkpoints
    }

    pub fn last_check(&self, agent: &str) -> Option<u64> {
        self.last_check_unix_seconds.get(agent).copied()
    }

    pub fn record(&mut self, agent: &str, now_unix_seconds: u64) {
        if agent.is_empty() || agent.len() > 1024 {
            return;
        }
        self.last_check_unix_seconds
            .insert(agent.to_owned(), now_unix_seconds);
        self.prune(now_unix_seconds);
    }

    pub fn forget(&mut self, agent: &str) {
        self.last_check_unix_seconds.remove(agent);
    }

    pub fn len(&self) -> usize {
        self.last_check_unix_seconds.len()
    }

    pub fn is_empty(&self) -> bool {
        self.last_check_unix_seconds.is_empty()
    }

    fn prune(&mut self, now_unix_seconds: u64) {
        self.last_check_unix_seconds
            .retain(|_, seen| now_unix_seconds.saturating_sub(*seen) <= RETENTION_SECONDS);
        while self.last_check_unix_seconds.len() > MAXIMUM_ENTRIES {
            let oldest = self
                .last_check_unix_seconds
                .iter()
                .min_by_key(|(_, seen)| **seen)
                .map(|(agent, _)| agent.clone());
            match oldest {
                Some(agent) => self.last_check_unix_seconds.remove(&agent),
                None => break,
            };
        }
    }

    /// Atomic owner-only replacement; a crash leaves the previous file intact.
    pub fn save(&self, directory: &Path) -> io::Result<()> {
        let stored = Stored {
            version: 1,
            last_check_unix_seconds: self.last_check_unix_seconds.clone(),
        };
        let bytes = serde_json::to_vec(&stored).map_err(io::Error::other)?;
        let destination = Self::path(directory);
        let temporary = directory.join(format!("{FILE_NAME}.{}.tmp", std::process::id()));
        {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&temporary)?;
            ilium_platform::secure_fs::restrict_open_file_to_owner(&file)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
        }
        ilium_platform::secure_fs::replace_file_durably(&temporary, &destination)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checkpoints_survive_a_restart_and_expire() {
        let directory = tempfile::tempdir().unwrap();
        let mut checkpoints = Checkpoints::default();
        checkpoints.record("session|server|7|1", 1_000);
        checkpoints.record("session|server|8|1", 90_000);
        checkpoints.save(directory.path()).unwrap();
        let recovered = Checkpoints::load(directory.path(), 90_100);
        assert_eq!(recovered.last_check("session|server|8|1"), Some(90_000));
        assert_eq!(
            recovered.last_check("session|server|7|1"),
            None,
            "entries older than one day are pruned on recovery"
        );
    }

    #[test]
    fn a_damaged_file_recovers_as_empty_without_failing() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(Checkpoints::path(directory.path()), b"not json").unwrap();
        assert!(Checkpoints::load(directory.path(), 1).is_empty());
    }

    #[test]
    fn the_checkpoint_set_is_bounded() {
        let mut checkpoints = Checkpoints::default();
        for index in 0..(MAXIMUM_ENTRIES + 10) {
            checkpoints.record(&format!("agent-{index}"), 10 + index as u64);
        }
        assert_eq!(checkpoints.len(), MAXIMUM_ENTRIES);
        assert_eq!(checkpoints.last_check("agent-0"), None);
    }
}

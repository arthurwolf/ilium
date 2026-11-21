//! Durable external-task ordering ledger. Call on an admitted I/O worker.

use std::path::{Path, PathBuf};

use ilium_core::external_tasks::{
    ExternalTaskKey, ExternalTaskObservation, ExternalTaskUpdate, ExternalTasks,
};
use ilium_platform::resource_fs::ResourceLedgerFile;

const MAX_LEDGER_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum ExternalTaskStoreError {
    #[error("external task ledger is busy")]
    Busy,
    #[error("external task ledger I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("external task ledger JSON is invalid: {0}")]
    Json(#[from] serde_json::Error),
    #[error("external task ledger contains invalid or duplicate records: {0:?}")]
    InvalidLedger(ExternalTaskUpdate),
    #[error("external task ledger exceeds its byte limit")]
    Capacity,
    #[error("external task commit outcome unknown: {0}")]
    OutcomeUnknown(std::io::Error),
}

/// A project/session-specific ledger. Its path is explicitly supplied by the composition root.
pub struct ExternalTaskStore {
    path: PathBuf,
}

impl ExternalTaskStore {
    pub fn new(path: impl Into<PathBuf>) -> Result<Self, ExternalTaskStoreError> {
        let path = path.into();
        if !path.is_absolute() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "ledger path must be absolute",
            )
            .into());
        }
        Ok(Self { path })
    }

    fn lock(&self) -> Result<ResourceLedgerFile, ExternalTaskStoreError> {
        ResourceLedgerFile::try_lock(Path::new(&self.path))?.ok_or(ExternalTaskStoreError::Busy)
    }

    fn read(file: &ResourceLedgerFile) -> Result<ExternalTasks, ExternalTaskStoreError> {
        let Some(bytes) = file.read_reconciled(MAX_LEDGER_BYTES)? else {
            return Ok(ExternalTasks::default());
        };
        let entries: Vec<(ExternalTaskKey, ExternalTaskObservation)> =
            serde_json::from_slice(&bytes)?;
        ExternalTasks::restore(entries).map_err(ExternalTaskStoreError::InvalidLedger)
    }

    pub fn snapshot(&self) -> Result<ExternalTasks, ExternalTaskStoreError> {
        Self::read(&self.lock()?)
    }

    /// Compare and persist while holding the cross-process ledger lock.
    pub fn update(
        &self,
        key: ExternalTaskKey,
        observation: ExternalTaskObservation,
    ) -> Result<ExternalTaskUpdate, ExternalTaskStoreError> {
        let file = self.lock()?;
        let mut tasks = Self::read(&file)?;
        let outcome = tasks.update(key, observation);
        if outcome == ExternalTaskUpdate::Applied {
            let entries: Vec<_> = tasks.entries().collect();
            let bytes = serde_json::to_vec(&entries)?;
            if bytes.len() as u64 > MAX_LEDGER_BYTES {
                return Err(ExternalTaskStoreError::Capacity);
            }
            file.replace(&bytes)
                .map_err(ExternalTaskStoreError::OutcomeUnknown)?;
        }
        Ok(outcome)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_core::external_tasks::ExternalTaskView;

    #[test]
    fn tombstone_survives_reopen_and_rejects_stale_update() {
        let root = std::env::var_os("ILIUM_RESOURCE_TEST_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/data/ilium-resource-queue-tests"));
        std::fs::create_dir_all(&root).unwrap();
        let temp = tempfile::tempdir_in(root).unwrap();
        let path = temp.path().join("external-tasks.json");
        let key = ExternalTaskKey {
            producer: "test-producer".into(),
            job: "test-job".into(),
        };
        let view = ExternalTaskObservation {
            sequence: 1,
            value: Some(ExternalTaskView {
                title: "Test".into(),
                status: "running".into(),
            }),
        };
        let store = ExternalTaskStore::new(&path).unwrap();
        assert_eq!(
            store.update(key.clone(), view.clone()).unwrap(),
            ExternalTaskUpdate::Applied
        );
        assert_eq!(
            store
                .update(
                    key.clone(),
                    ExternalTaskObservation {
                        sequence: 2,
                        value: None
                    }
                )
                .unwrap(),
            ExternalTaskUpdate::Applied
        );
        let reopened = ExternalTaskStore::new(&path).unwrap();
        assert_eq!(
            reopened.update(key, view).unwrap(),
            ExternalTaskUpdate::Stale
        );
        assert_eq!(reopened.snapshot().unwrap().live().count(), 0);
    }

    #[test]
    fn corrupt_ledger_is_not_replaced_by_an_update() {
        let root = std::env::var_os("ILIUM_RESOURCE_TEST_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/data/ilium-resource-queue-tests"));
        std::fs::create_dir_all(&root).unwrap();
        let temp = tempfile::tempdir_in(root).unwrap();
        let path = temp.path().join("external-tasks.json");
        std::fs::write(&path, b"invalid ledger").unwrap();
        let store = ExternalTaskStore::new(&path).unwrap();
        let key = ExternalTaskKey {
            producer: "test-producer".into(),
            job: "test-job".into(),
        };
        assert!(store
            .update(
                key,
                ExternalTaskObservation {
                    sequence: 1,
                    value: None
                }
            )
            .is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"invalid ledger");
    }
}

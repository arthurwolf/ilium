//! Original immutable sound settings, charged before Arc publication.
use crate::execution::ExecutionClient;
use ilium_execution::{RejectReason, StorageAdmission};
use ilium_sound::SoundSettings;
use std::ops::Deref;
use std::path::PathBuf;
use std::sync::Arc;

pub(crate) struct SharedSoundSettings {
    value: SoundSettings,
    // Payload before lease: the last queued/native reader retires both.
    _storage: Arc<StorageAdmission>,
}
impl Deref for SharedSoundSettings {
    type Target = SoundSettings;
    fn deref(&self) -> &Self::Target {
        &self.value
    }
}
impl std::fmt::Debug for SharedSoundSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SharedSoundSettings")
            .finish_non_exhaustive()
    }
}
impl SharedSoundSettings {
    pub(crate) fn try_new(
        execution: &ExecutionClient,
        value: SoundSettings,
    ) -> Result<Arc<Self>, (RejectReason, SoundSettings)> {
        let bytes = std::mem::size_of::<Self>()
            .checked_add(std::mem::size_of::<StorageAdmission>())
            .and_then(|n| n.checked_add(4 * std::mem::size_of::<usize>()))
            .and_then(|n| n.checked_add(value.file.as_ref().map_or(0, PathBuf::capacity)));
        let Some(bytes) = bytes else {
            return Err((RejectReason::InvalidCost, value));
        };
        match execution.try_reserve_storage(bytes) {
            Ok(storage) => Ok(Arc::new(Self {
                value,
                _storage: storage,
            })),
            Err(reason) => Err((reason, value)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replacement_keeps_the_original_path_charged_until_the_last_snapshot_drops() {
        let execution = crate::execution::ServerExecution::start().unwrap();
        let quota = execution.quota_group();
        let baseline = quota.snapshot().worker_bytes;
        let mut path = PathBuf::with_capacity(8192);
        path.push("original-sound.wav");
        let original_pointer = path.as_os_str().as_encoded_bytes().as_ptr();
        let original_capacity = path.capacity();
        let original = SharedSoundSettings::try_new(
            &execution.client,
            SoundSettings {
                file: Some(path),
                ..SoundSettings::default()
            },
        )
        .unwrap();
        let charged = quota.snapshot().worker_bytes;
        assert!(charged >= baseline + original_capacity);
        let snapshot = Arc::clone(&original);
        assert_eq!(quota.snapshot().worker_bytes, charged);
        assert_eq!(
            snapshot
                .file
                .as_ref()
                .unwrap()
                .as_os_str()
                .as_encoded_bytes()
                .as_ptr(),
            original_pointer
        );
        let replacement =
            SharedSoundSettings::try_new(&execution.client, SoundSettings::default()).unwrap();
        drop(original);
        assert!(quota.snapshot().worker_bytes > charged);
        drop(replacement);
        assert_eq!(quota.snapshot().worker_bytes, charged);
        drop(snapshot);
        assert_eq!(quota.snapshot().worker_bytes, baseline);
    }

    #[test]
    fn closed_bank_returns_the_original_settings_allocation() {
        let execution = crate::execution::ServerExecution::start().unwrap();
        let mut path = PathBuf::with_capacity(4096);
        path.push("refused-original.wav");
        let pointer = path.as_os_str().as_encoded_bytes().as_ptr();
        let capacity = path.capacity();
        execution.request_shutdown();
        let (reason, original) = SharedSoundSettings::try_new(
            &execution.client,
            SoundSettings {
                file: Some(path),
                ..SoundSettings::default()
            },
        )
        .unwrap_err();
        assert_eq!(reason, RejectReason::Closed);
        let path = original.file.unwrap();
        assert_eq!(path.capacity(), capacity);
        assert_eq!(path.as_os_str().as_encoded_bytes().as_ptr(), pointer);
    }
}

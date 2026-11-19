//! Fake storage (reference-test-harness.md "Fake storage and settings").

use std::collections::{BTreeMap, VecDeque};

use ilium_plugin_sdk_core::generated::host::{Bucket, Entry};
use ilium_plugin_sdk_core::types::{ErrorCode, PluginError};

/// Largest single value a bucket accepts (1 MiB, DEV-465).
const MAX_VALUE_BYTES: usize = 1024 * 1024;
/// Largest total size of one bucket (16 MiB, DEV-465).
const MAX_BUCKET_BYTES: usize = 16 * 1024 * 1024;

/// Read handle on the fake storage of a host.
#[derive(Debug, Default, Clone)]
pub struct FakeStorage {
    values: BTreeMap<String, BTreeMap<String, Vec<u8>>>,
    shared_order: VecDeque<String>,
    write_error: Option<PluginError>,
    read_error: Option<PluginError>,
}

impl FakeStorage {
    /// An empty store.
    pub fn new() -> FakeStorage {
        FakeStorage::default()
    }

    /// Injects or clears a storage failure for later guest writes and removals.
    pub fn set_write_error(&mut self, error: Option<PluginError>) {
        self.write_error = error;
    }

    /// Injects or clears a storage failure for later guest reads.
    pub fn set_read_error(&mut self, error: Option<PluginError>) {
        self.read_error = error;
    }

    /// Every key and value of one bucket.
    pub fn dump(&self, bucket: Bucket) -> BTreeMap<String, Vec<u8>> {
        self.values
            .get(bucket.as_str())
            .cloned()
            .unwrap_or_default()
    }

    /// One value.
    pub fn get(&self, bucket: Bucket, key: &str) -> Option<Vec<u8>> {
        self.values
            .get(bucket.as_str())
            .and_then(|map| map.get(key))
            .cloned()
    }

    pub(crate) fn try_get(&self, bucket: Bucket, key: &str) -> Result<Option<Vec<u8>>, PluginError> {
        if key.is_empty() || key.len() > 256 {
            return Err(PluginError::new(ErrorCode::InvalidArgument, "invalid-key"));
        }
        if let Some(error) = &self.read_error {
            return Err(error.clone());
        }
        Ok(self.get(bucket, key))
    }

    pub(crate) fn try_keys(&self, bucket: Bucket, prefix: &str) -> Result<Vec<String>, PluginError> {
        if let Some(error) = &self.read_error {
            return Err(error.clone());
        }
        Ok(self.dump(bucket).into_keys().filter(|key| key.starts_with(prefix)).collect())
    }

    /// Seeds one value before the plugin runs. Invalid or over-quota seeds are ignored.
    pub fn put(&mut self, bucket: Bucket, key: &str, value: &[u8]) {
        let _ = self.try_put(bucket, key, value);
    }

    /// Applies a guest write and reports the same validation and quota failures as the host.
    pub(crate) fn try_put(
        &mut self,
        bucket: Bucket,
        key: &str,
        value: &[u8],
    ) -> Result<(), PluginError> {
        if let Some(error) = &self.write_error {
            return Err(error.clone());
        }
        if key.is_empty() || key.len() > 256 || value.len() > MAX_VALUE_BYTES {
            return Err(PluginError::new(
                ErrorCode::InvalidArgument,
                "invalid storage key or value",
            ));
        }
        let map = self.values.entry(bucket.as_str().to_owned()).or_default();
        let others: usize = map
            .iter()
            .filter(|(existing, _)| existing.as_str() != key)
            .map(|(existing, stored)| existing.len() + stored.len())
            .sum();
        if bucket != Bucket::Shared && others + key.len() + value.len() > MAX_BUCKET_BYTES {
            return Err(PluginError::new(
                ErrorCode::QuotaExceeded,
                "bucket-quota-exceeded",
            ));
        }
        map.insert(key.to_string(), value.to_vec());
        if bucket == Bucket::Shared {
            self.shared_order.retain(|existing| existing != key);
            self.shared_order.push_back(key.to_owned());
            let mut size = others + key.len() + value.len();
            while size > MAX_BUCKET_BYTES {
                if let Some(oldest) = self.shared_order.pop_front() {
                    if let Some(removed) = map.remove(&oldest) {
                        size -= oldest.len() + removed.len();
                    }
                } else {
                    break;
                }
            }
        }
        Ok(())
    }

    /// Removes one value as the plugin asked; a missing key is not an error.
    pub(crate) fn remove(&mut self, bucket: Bucket, key: &str) -> Result<(), PluginError> {
        if key.is_empty() || key.len() > 256 {
            return Err(PluginError::new(ErrorCode::InvalidArgument, "invalid-batch"));
        }
        if let Some(error) = &self.write_error {
            return Err(error.clone());
        }
        if bucket == Bucket::Shared {
            self.shared_order.retain(|existing| existing != key);
        }
        if let Some(map) = self.values.get_mut(bucket.as_str()) {
            map.remove(key);
        }
        Ok(())
    }

    /// Validates and applies the whole batch before publishing any of its changes.
    pub(crate) fn write_batch(&mut self, bucket: Bucket, entries: Vec<Entry>) -> Result<(), PluginError> {
        if let Some(error) = &self.write_error {
            return Err(error.clone());
        }
        let bytes = entries.iter().try_fold(0usize, |total, entry| {
            total.checked_add(entry.key.len())?.checked_add(entry.value.as_ref().map_or(0, Vec::len))
        });
        if entries.len() > 256 || bytes.is_none_or(|size| size > 4 * 1024 * 1024)
            || entries.iter().any(|entry| entry.key.is_empty() || entry.key.len() > 256
                || entry.value.as_ref().is_some_and(|value| value.len() > MAX_VALUE_BYTES))
        {
            return Err(PluginError::new(ErrorCode::InvalidArgument, "invalid-batch"));
        }
        let mut values = self.dump(bucket);
        let mut order = self.shared_order.clone();
        for entry in entries {
            if bucket == Bucket::Shared {
                order.retain(|key| key != &entry.key);
            }
            if let Some(value) = entry.value {
                if bucket == Bucket::Shared {
                    order.push_back(entry.key.clone());
                }
                values.insert(entry.key, value);
            } else {
                values.remove(&entry.key);
            }
        }
        let mut size: usize = values.iter().map(|(key, value)| key.len() + value.len()).sum();
        if bucket != Bucket::Shared && size > MAX_BUCKET_BYTES {
            return Err(PluginError::new(ErrorCode::QuotaExceeded, "bucket-quota-exceeded"));
        }
        while bucket == Bucket::Shared && size > MAX_BUCKET_BYTES {
            let Some(oldest) = order.pop_front() else { break };
            if let Some(value) = values.remove(&oldest) {
                size -= oldest.len() + value.len();
            }
        }
        self.values.insert(bucket.as_str().to_owned(), values);
        if bucket == Bucket::Shared {
            self.shared_order = order;
        }
        Ok(())
    }

    /// Whole content as `{bucket: {key: bytes}}`, for snapshots; empty buckets are left out.
    pub(crate) fn dump_all(&self) -> BTreeMap<String, BTreeMap<String, Vec<u8>>> {
        self.values
            .iter()
            .filter(|(_, map)| !map.is_empty())
            .map(|(name, map)| (name.clone(), map.clone()))
            .collect()
    }

    pub(crate) fn snapshot(&self) -> BTreeMap<String, BTreeMap<String, Vec<u8>>> {
        self.values.clone()
    }
}

#[cfg(test)]
mod spec_tests {
    use super::*;
    use crate::FakeHost;

    #[test]
    fn invalid_batch_does_not_apply_an_earlier_removal() {
        let mut storage = FakeStorage::new();
        storage.put(Bucket::Session, "saved", b"original");
        let error = storage.write_batch(Bucket::Session, vec![
            Entry { key: "saved".into(), value: None },
            Entry { key: String::new(), value: Some(vec![1]) },
        ]).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert_eq!(storage.get(Bucket::Session, "saved"), Some(b"original".to_vec()));
    }

    #[test]
    fn injected_write_error_preserves_existing_data_and_can_be_cleared() {
        let mut storage = FakeStorage::new();
        storage.try_put(Bucket::Project, "key", b"old").unwrap();
        storage.set_write_error(Some(PluginError::new(ErrorCode::Internal, "disk failure")));
        assert_eq!(
            storage.try_put(Bucket::Project, "key", b"new").unwrap_err().code,
            ErrorCode::Internal
        );
        assert_eq!(storage.get(Bucket::Project, "key"), Some(b"old".to_vec()));
        storage.set_write_error(None);
        storage.try_put(Bucket::Project, "key", b"new").unwrap();
        assert_eq!(storage.get(Bucket::Project, "key"), Some(b"new".to_vec()));
    }

    #[test]
    fn injected_write_error_also_preserves_a_removed_value() {
        let mut storage = FakeStorage::new();
        storage.put(Bucket::Project, "key", b"old");
        storage.set_write_error(Some(PluginError::new(ErrorCode::Internal, "disk failure")));
        assert_eq!(storage.remove(Bucket::Project, "key").unwrap_err().code, ErrorCode::Internal);
        assert_eq!(storage.get(Bucket::Project, "key"), Some(b"old".to_vec()));
        storage.set_write_error(None);
        storage.remove(Bucket::Project, "key").unwrap();
        assert_eq!(storage.get(Bucket::Project, "key"), None);
    }

    #[test]
    fn private_quota_counts_keys_and_refused_replacement_preserves_data() {
        let mut storage = FakeStorage::new();
        let value = vec![1; MAX_VALUE_BYTES];
        for index in 0..15 {
            storage
                .try_put(Bucket::Project, &format!("key{index}"), &value)
                .unwrap();
        }
        storage.try_put(Bucket::Project, "last", &[7]).unwrap();
        let error = storage.try_put(Bucket::Project, "last", &value).unwrap_err();
        assert_eq!(error.code, ErrorCode::QuotaExceeded);
        assert_eq!(storage.get(Bucket::Project, "last"), Some(vec![7]));
        assert_eq!(
            storage.try_put(Bucket::Project, "", &[]).unwrap_err().code,
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            storage
                .try_put(Bucket::Project, &"k".repeat(257), &[])
                .unwrap_err()
                .code,
            ErrorCode::InvalidArgument
        );
    }

    #[test]
    fn shared_quota_evicts_oldest_write_and_refresh_updates_order() {
        let mut storage = FakeStorage::new();
        let value = vec![1; MAX_VALUE_BYTES];
        for index in 0..15 {
            storage
                .try_put(Bucket::Shared, &format!("key{index}"), &value)
                .unwrap();
        }
        storage.try_put(Bucket::Shared, "key0", &value).unwrap();
        storage.try_put(Bucket::Shared, "new", &value).unwrap();
        assert!(storage.get(Bucket::Shared, "key0").is_some());
        assert_eq!(storage.get(Bucket::Shared, "key1"), None);
        assert!(storage.get(Bucket::Shared, "new").is_some());
    }

    /// REQ: DEV-465
    #[test]
    fn dev_465_put_is_visible_to_get_at_once() {
        let mut host = FakeHost::new();
        host.storage_mut()
            .put(Bucket::Global, "last_suggestion", b"hello");
        assert_eq!(
            host.storage().get(Bucket::Global, "last_suggestion"),
            Some(b"hello".to_vec())
        );
    }

    /// REQ: DEV-465
    #[test]
    fn dev_465_second_put_replaces_the_first_value() {
        let mut host = FakeHost::new();
        host.storage_mut().put(Bucket::Project, "k", b"first");
        host.storage_mut().put(Bucket::Project, "k", b"second");
        assert_eq!(
            host.storage().get(Bucket::Project, "k"),
            Some(b"second".to_vec())
        );
    }

    /// REQ: DEV-465
    #[test]
    fn dev_465_dump_returns_every_key_and_value_of_one_bucket() {
        let mut host = FakeHost::new();
        host.storage_mut().put(Bucket::Session, "a", &[1, 2]);
        host.storage_mut().put(Bucket::Session, "b", &[3]);
        host.storage_mut().put(Bucket::Global, "other", &[9]);
        let dumped: BTreeMap<String, Vec<u8>> = host.storage().dump(Bucket::Session);
        let mut expected: BTreeMap<String, Vec<u8>> = BTreeMap::new();
        expected.insert("a".to_string(), vec![1, 2]);
        expected.insert("b".to_string(), vec![3]);
        assert_eq!(dumped, expected);
    }

    /// REQ: DEV-465
    #[test]
    fn dev_465_buckets_are_isolated_from_each_other() {
        let mut host = FakeHost::new();
        host.storage_mut().put(Bucket::Global, "shared_name", b"g");
        assert_eq!(host.storage().get(Bucket::Project, "shared_name"), None);
        assert_eq!(host.storage().get(Bucket::Shared, "shared_name"), None);
    }

    /// REQ: DEV-465
    #[test]
    fn dev_465_missing_key_reads_as_none() {
        let host = FakeHost::new();
        assert_eq!(host.storage().get(Bucket::Global, "never_put"), None);
        assert!(host.storage().dump(Bucket::Global).is_empty());
    }

    /// REQ: DEV-465
    #[test]
    fn dev_465_empty_value_round_trips_as_present() {
        let mut host = FakeHost::new();
        host.storage_mut().put(Bucket::Shared, "empty", &[]);
        assert_eq!(
            host.storage().get(Bucket::Shared, "empty"),
            Some(Vec::new())
        );
        assert_eq!(host.storage().get(Bucket::Shared, "absent"), None);
    }

    /// REQ: DEV-465
    #[test]
    fn dev_465_seeded_value_is_the_one_dumped_from_the_host() {
        let mut host = FakeHost::new();
        host.storage_mut().put(Bucket::Project, "kept", &[1]);
        assert_eq!(host.storage().dump(Bucket::Project)["kept"], vec![1]);
    }

    /// suite in ilium-plugin-testkit/tests/sdk_fakehost; this unit covers the fake's own contract)
    /// REQ: DEV-465
    #[test]
    fn dev_465_value_stays_readable_after_a_second_put_on_another_key() {
        let mut host = FakeHost::new();
        host.storage_mut().put(Bucket::Global, "first", &[1]);
        host.storage_mut().put(Bucket::Global, "second", &[2]);
        assert_eq!(host.storage().get(Bucket::Global, "first"), Some(vec![1]));
        assert_eq!(host.storage().get(Bucket::Global, "second"), Some(vec![2]));
    }
}

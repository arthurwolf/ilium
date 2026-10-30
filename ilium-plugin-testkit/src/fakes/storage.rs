//! Fake storage (reference-test-harness.md "Fake storage and settings").

use std::collections::BTreeMap;

use ilium_plugin_sdk_core::generated::host::Bucket;

/// Largest single value a bucket accepts (1 MiB, DEV-465).
const MAX_VALUE_BYTES: usize = 1024 * 1024;
/// Largest total size of one bucket (16 MiB, DEV-465).
const MAX_BUCKET_BYTES: usize = 16 * 1024 * 1024;

/// Read handle on the fake storage of a host.
#[derive(Debug, Default, Clone)]
pub struct FakeStorage {
    buckets: BTreeMap<&'static str, BTreeMap<String, Vec<u8>>>,
}

fn bucket_name(bucket: Bucket) -> &'static str {
    match bucket {
        Bucket::Global => "global",
        Bucket::Project => "project",
        Bucket::Session => "session",
        Bucket::Shared => "shared",
    }
}

impl FakeStorage {
    /// An empty store.
    pub fn new() -> FakeStorage {
        FakeStorage::default()
    }

    /// Every key and value of one bucket.
    pub fn dump(&self, bucket: Bucket) -> BTreeMap<String, Vec<u8>> {
        self.buckets
            .get(bucket_name(bucket))
            .cloned()
            .unwrap_or_default()
    }

    /// One value.
    pub fn get(&self, bucket: Bucket, key: &str) -> Option<Vec<u8>> {
        self.buckets
            .get(bucket_name(bucket))
            .and_then(|map| map.get(key))
            .cloned()
    }

    /// Seeds one value before the plugin runs, as the real store would hold it (DEV-465, via
    /// `FakeHost::storage_mut`). A value above the quota is ignored, as the real store refuses it.
    pub fn put(&mut self, bucket: Bucket, key: &str, value: &[u8]) {
        if value.len() > MAX_VALUE_BYTES {
            return;
        }
        let map = self.buckets.entry(bucket_name(bucket)).or_default();
        let others: usize = map
            .iter()
            .filter(|(existing, _)| existing.as_str() != key)
            .map(|(_, stored)| stored.len())
            .sum();
        if others + value.len() > MAX_BUCKET_BYTES {
            return;
        }
        map.insert(key.to_string(), value.to_vec());
    }

    /// Removes one value as the plugin asked; a missing key is not an error.
    pub(crate) fn remove(&mut self, bucket: Bucket, key: &str) {
        if let Some(map) = self.buckets.get_mut(bucket_name(bucket)) {
            map.remove(key);
        }
    }

    /// Whole content as `{bucket: {key: bytes}}`, for snapshots; empty buckets are left out.
    pub(crate) fn dump_all(&self) -> BTreeMap<String, BTreeMap<String, Vec<u8>>> {
        self.buckets
            .iter()
            .filter(|(_, map)| !map.is_empty())
            .map(|(name, map)| (name.to_string(), map.clone()))
            .collect()
    }
}

#[cfg(test)]
mod spec_tests {
    use super::*;
    use crate::FakeHost;

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

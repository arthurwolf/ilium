//! Fake storage (reference-test-harness.md "Fake storage and settings").

use std::collections::BTreeMap;

use ilium_plugin_sdk_core::generated::host::Bucket;

/// Read handle on the fake storage of a host.
#[derive(Debug)]
pub struct FakeStorage {
    _private: (),
}

impl FakeStorage {
    /// Every key and value of one bucket.
    pub fn dump(&self, _bucket: Bucket) -> BTreeMap<String, Vec<u8>> {
        unimplemented!("DEV-465")
    }

    /// One value.
    pub fn get(&self, _bucket: Bucket, _key: &str) -> Option<Vec<u8>> {
        unimplemented!("DEV-465")
    }

    /// Seeds one value before the plugin runs, as the real store would hold it (DEV-465, via
    /// `FakeHost::storage_mut`).
    pub fn put(&mut self, _bucket: Bucket, _key: &str, _value: &[u8]) {
        unimplemented!("DEV-465")
    }
}

#[cfg(test)]
mod spec_tests {
    use super::*;
    use crate::FakeHost;

    /// REQ: DEV-465 (put is visible to get at once, inside the same call batch)
    #[test]
    fn dev_465_put_is_visible_to_get_at_once() {
        let mut host = FakeHost::new();
        host.storage_mut().put(Bucket::Global, "last_suggestion", b"hello");
        assert_eq!(
            host.storage().get(Bucket::Global, "last_suggestion"),
            Some(b"hello".to_vec())
        );
    }

    /// REQ: DEV-465 (put overwrites: the store holds the latest value per key)
    #[test]
    fn dev_465_second_put_replaces_the_first_value() {
        let mut host = FakeHost::new();
        host.storage_mut().put(Bucket::Project, "k", b"first");
        host.storage_mut().put(Bucket::Project, "k", b"second");
        assert_eq!(host.storage().get(Bucket::Project, "k"), Some(b"second".to_vec()));
    }

    /// REQ: DEV-465 (dump returns every key and value of one bucket, and only that bucket)
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

    /// REQ: DEV-465 (buckets are separate stores: a key in one bucket is absent from another)
    #[test]
    fn dev_465_buckets_are_isolated_from_each_other() {
        let mut host = FakeHost::new();
        host.storage_mut().put(Bucket::Global, "shared_name", b"g");
        assert_eq!(host.storage().get(Bucket::Project, "shared_name"), None);
        assert_eq!(host.storage().get(Bucket::Shared, "shared_name"), None);
    }

    /// REQ: DEV-465 (empty input: a key that was never put reads as absent, not as an empty value)
    #[test]
    fn dev_465_missing_key_reads_as_none() {
        let host = FakeHost::new();
        assert_eq!(host.storage().get(Bucket::Global, "never_put"), None);
        assert!(host.storage().dump(Bucket::Global).is_empty());
    }

    /// REQ: DEV-465 (edge: an empty value is a stored value, distinct from a missing key)
    #[test]
    fn dev_465_empty_value_round_trips_as_present() {
        let mut host = FakeHost::new();
        host.storage_mut().put(Bucket::Shared, "empty", &[]);
        assert_eq!(host.storage().get(Bucket::Shared, "empty"), Some(Vec::new()));
        assert_eq!(host.storage().get(Bucket::Shared, "absent"), None);
    }

    /// REQ: DEV-465 (values seeded through storage_mut are the values the plugin later reads)
    #[test]
    fn dev_465_seeded_value_is_the_one_dumped_from_the_host() {
        let mut host = FakeHost::new();
        host.storage_mut().put(Bucket::Project, "kept", &[1]);
        assert_eq!(host.storage().dump(Bucket::Project)["kept"], vec![1]);
    }

    /// REQ: DEV-465 (survival across deactivate then activate is asserted by the integration
    /// suite in ilium-plugin-testkit/tests/sdk_fakehost; this unit covers the fake's own contract)
    #[test]
    fn dev_465_value_stays_readable_after_a_second_put_on_another_key() {
        let mut host = FakeHost::new();
        host.storage_mut().put(Bucket::Global, "first", &[1]);
        host.storage_mut().put(Bucket::Global, "second", &[2]);
        assert_eq!(host.storage().get(Bucket::Global, "first"), Some(vec![1]));
        assert_eq!(host.storage().get(Bucket::Global, "second"), Some(vec![2]));
    }
}

//! Fake storage (reference-test-harness.md "Fake storage and settings").

use std::collections::BTreeMap;

use ilium_plugin_sdk_core::generated::host::Bucket;

/// Read handle on the fake storage of a host. Host-import quota enforcement is separate.
#[derive(Debug, Default)]
pub struct FakeStorage {
    values: BTreeMap<String, BTreeMap<String, Vec<u8>>>,
}

impl FakeStorage {
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
            .and_then(|values| values.get(key))
            .cloned()
    }

    /// Seeds one value before the plugin runs (DEV-465, via `FakeHost::storage_mut`).
    pub fn put(&mut self, bucket: Bucket, key: &str, value: &[u8]) {
        self.values
            .entry(bucket.as_str().to_owned())
            .or_default()
            .insert(key.to_owned(), value.to_vec());
    }

    pub(crate) fn snapshot(&self) -> BTreeMap<String, BTreeMap<String, Vec<u8>>> {
        self.values.clone()
    }
}

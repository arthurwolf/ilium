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
}

//! Host implementation of `ilium:host/storage` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostStorage` trait; hand-owned afterwards.

use ilium_plugin_api::generated::host::storage::{Bucket, Entry, HostStorage};
use ilium_plugin_api::generated::types::errors::PluginError;

/// Host side of the `storage` interface.
#[derive(Debug)]
pub struct StorageImport;

impl HostStorage for StorageImport {
    fn get(&mut self, bucket: Bucket, key: String) -> Option<Vec<u8>> {
        unimplemented!("host::imports::storage::get")
    }

    fn put(&mut self, bucket: Bucket, key: String, value: Vec<u8>) -> Result<(), PluginError> {
        unimplemented!("host::imports::storage::put")
    }

    fn remove(&mut self, bucket: Bucket, key: String) {
        unimplemented!("host::imports::storage::remove")
    }

    fn keys(&mut self, bucket: Bucket, prefix: String) -> Vec<String> {
        unimplemented!("host::imports::storage::keys")
    }

    fn write_batch(&mut self, bucket: Bucket, entries: Vec<Entry>) -> Result<(), PluginError> {
        unimplemented!("host::imports::storage::write_batch")
    }

    fn lease(&mut self, key: String, ttl_millis: u32) -> bool {
        unimplemented!("host::imports::storage::lease")
    }
}

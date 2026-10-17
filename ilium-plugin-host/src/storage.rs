//! Per-plugin key-value storage with quotas, journal and data-version migration
//! (reference-storage.md).

use ilium_plugin_api::ids::PluginId;
use ilium_plugin_api::PluginError;

/// One value is at most 1 MiB (reference-storage.md).
pub const VALUE_MAX_BYTES: usize = 1024 * 1024;

/// One bucket is at most 16 MiB (reference-storage.md).
pub const BUCKET_MAX_BYTES: usize = 16 * 1024 * 1024;

/// The storage of one plugin.
#[derive(Debug)]
pub struct PluginStorage {
    plugin: PluginId,
}

impl PluginStorage {
    /// Opens the storage of a plugin under the data directory.
    pub fn open(root: &std::path::Path, plugin: PluginId) -> Result<PluginStorage, PluginError> {
        unimplemented!("host::storage::PluginStorage::open")
    }

    /// Reads a value.
    pub fn get(&self, bucket: &str, key: &str) -> Result<Option<Vec<u8>>, PluginError> {
        unimplemented!("host::storage::PluginStorage::get")
    }

    /// Writes a value; quota violations are `quota-exceeded`.
    pub fn put(&mut self, bucket: &str, key: &str, value: Vec<u8>) -> Result<(), PluginError> {
        unimplemented!("host::storage::PluginStorage::put")
    }

    /// Runs the registered migrations up to the data version in the manifest.
    pub fn migrate(&mut self, target_version: u32) -> Result<(), PluginError> {
        unimplemented!("host::storage::PluginStorage::migrate")
    }
}

/// Sets the data version a plugin's storage was last written with (reference-storage.md,
/// reference-install-and-update.md PKG-288).
pub fn write_bucket_stamp(
    storage_root: &std::path::Path,
    plugin: &str,
    data_version: u64,
) -> std::io::Result<()> {
    unimplemented!("host::storage::write_bucket_stamp")
}

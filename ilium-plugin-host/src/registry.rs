//! On-disk registries: installed packages, grants and enable flags (reference-packaging.md,
//! reference-security.md).

use std::path::{Path, PathBuf};

use ilium_plugin_api::grants::Grant;
use ilium_plugin_api::ids::PluginId;

use crate::error::HostError;

/// The package store directory (reference-packaging.md).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageStore {
    /// Root directory of the store.
    pub root: PathBuf,
}

impl PackageStore {
    /// The directory of one installed version.
    pub fn version_dir(&self, plugin: &PluginId, version: &str) -> PathBuf {
        unimplemented!("host::registry::PackageStore::version_dir")
    }
}

/// One line of `installed.json` (reference-packaging.md).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct InstalledRecord {
    /// The plugin id.
    pub id: String,
    /// The installed version.
    pub version: String,
    /// Where the package came from.
    pub source: String,
    /// The release channel.
    pub channel: String,
    /// Whether the user pinned this version.
    pub pinned: bool,
}

/// The registry of installed plugins, `installed.json` (reference-packaging.md).
#[derive(Debug)]
pub struct InstalledRegistry {
    path: PathBuf,
}

impl InstalledRegistry {
    /// Opens or creates the registry file.
    pub fn open(path: &Path) -> Result<InstalledRegistry, HostError> {
        unimplemented!("host::registry::InstalledRegistry::open")
    }

    /// Every installed record.
    pub fn list(&self) -> Result<Vec<InstalledRecord>, HostError> {
        unimplemented!("host::registry::InstalledRegistry::list")
    }

    /// Inserts or replaces one record, atomically.
    pub fn put(&mut self, record: InstalledRecord) -> Result<(), HostError> {
        unimplemented!("host::registry::InstalledRegistry::put")
    }

    /// Removes a record.
    pub fn remove(&mut self, plugin: &PluginId) -> Result<(), HostError> {
        unimplemented!("host::registry::InstalledRegistry::remove")
    }
}

/// The grant store, `$XDG_STATE_HOME/ilium/grants.json`: mode 0600, single writer, atomic replace
/// (reference-security.md).
#[derive(Debug)]
pub struct GrantStore {
    path: PathBuf,
}

impl GrantStore {
    /// Opens the store, taking the single-writer lock.
    pub fn open(path: &Path) -> Result<GrantStore, HostError> {
        unimplemented!("host::registry::GrantStore::open")
    }

    /// The grants a user approved for a plugin.
    pub fn granted(&self, plugin: &PluginId) -> Result<Vec<Grant>, HostError> {
        unimplemented!("host::registry::GrantStore::granted")
    }

    /// Replaces the approved grants of a plugin, atomically.
    pub fn set_granted(&mut self, plugin: &PluginId, grants: &[Grant]) -> Result<(), HostError> {
        unimplemented!("host::registry::GrantStore::set_granted")
    }
}

/// Enable flags, `config.toml [plugins.<id>]` (reference-packaging.md).
#[derive(Debug)]
pub struct EnableFlags {
    path: PathBuf,
}

impl EnableFlags {
    /// Opens the flags of a config file.
    pub fn open(path: &Path) -> Result<EnableFlags, HostError> {
        unimplemented!("host::registry::EnableFlags::open")
    }

    /// Whether the user enabled the plugin; a plugin without a flag is enabled.
    pub fn is_enabled(&self, plugin: &PluginId) -> bool {
        unimplemented!("host::registry::EnableFlags::is_enabled")
    }

    /// Sets the flag.
    pub fn set_enabled(&mut self, plugin: &PluginId, enabled: bool) -> Result<(), HostError> {
        unimplemented!("host::registry::EnableFlags::set_enabled")
    }
}

/// The package download folder under the cache directory (reference-install-and-update.md PKG-267).
pub fn downloads_dir(cache: &Path) -> PathBuf {
    unimplemented!("host::registry::downloads_dir")
}

/// The offline cache folder of one registry URL under the cache directory (PKG-267).
pub fn registry_cache_dir(cache: &Path, url: &str) -> PathBuf {
    unimplemented!("host::registry::registry_cache_dir")
}

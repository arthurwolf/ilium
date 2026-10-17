//! Data-migration rig: one in-process host with a fake clock, fake disk and fake plugin runtime
//! that drives `migrator.migrate`, the data-version stamps, backups and rollback (test plan lane
//! data-migration, LIF-100..121, WIT migrator.migrate). Test support only.

use std::collections::BTreeMap;
use std::path::PathBuf;

use ilium_plugin_api::PluginError;

use crate::cli::CliOutput;

pub use ilium_plugin_api::generated::host::Bucket;

/// One scripted step of a plugin's `migrate` export (LIF-105, 108, 110, 116).
#[derive(Debug, Clone)]
pub enum Step {
    /// Puts a value.
    Put(Bucket, String, Vec<u8>),
    /// Removes a value.
    Remove(Bucket, String),
    /// Writes a data file.
    WriteFile(String, Vec<u8>),
    /// Returns an error.
    Fail(PluginError),
    /// Traps.
    Trap,
    /// Never returns.
    Hang,
    /// Kills the host process.
    KillHost,
    /// Kills the host process on the first run only.
    KillHostFirstRun,
}

/// The scope of one `migrate` call (LIF-104).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MigrateScope {
    /// The global bucket and the data directory.
    Global,
    /// One session bucket.
    Session {
        /// The project path.
        project: String,
        /// The session name.
        session: String,
    },
}

/// One observed `migrate` call (LIF-104, 105, 107, 112).
#[derive(Debug, Clone)]
pub struct MigrateCall {
    /// The data version migrated from.
    pub from: u32,
    /// The data version migrated to.
    pub to: u32,
    /// The scope.
    pub scope: MigrateScope,
    /// Virtual time the call took.
    pub elapsed_ms: u64,
    /// The backup made before the call.
    pub backup_name: Option<String>,
    /// Whether the migrate lock was held during the call.
    pub lock_held: bool,
    /// Whether the export returned `Ok`.
    pub returned_ok: bool,
    /// The error the export returned.
    pub returned_error: Option<PluginError>,
}

/// What an activation or upgrade did (LIF-103, 108, 110, 112).
#[derive(Debug, Clone, PartialEq)]
pub struct ActivateReport {
    /// The version active afterwards.
    pub active_version: String,
    /// Whether the update was rolled back.
    pub rolled_back: bool,
    /// Whether the host was killed during the run.
    pub killed: bool,
    /// The restore bytes delivered to `activate`.
    pub restored: Option<Vec<u8>>,
    /// The deactivation reason delivered to the old version.
    pub deactivation_reason: Option<String>,
    /// Virtual time spent waiting for the migrate lock.
    pub lock_wait_ms: u64,
}

/// A plugin spec with a data version and a scripted migrator (LIF-100, 103, 105).
#[derive(Debug, Clone)]
pub struct PluginSpec {
    _private: (),
}

impl PluginSpec {
    /// A plugin with the given id and version.
    pub fn new(id: &str, version: &str) -> PluginSpec {
        unimplemented!("host::testing::migration::PluginSpec::new")
    }

    /// The manifest `data_version`.
    pub fn data_version(self, version: u32) -> PluginSpec {
        unimplemented!("host::testing::migration::PluginSpec::data_version")
    }

    /// The manifest `data_version` as raw JSON.
    pub fn data_version_json(self, json: &str) -> PluginSpec {
        unimplemented!("host::testing::migration::PluginSpec::data_version_json")
    }

    /// The scripted `migrate` export.
    pub fn migrator(self, steps: Vec<Step>) -> PluginSpec {
        unimplemented!("host::testing::migration::PluginSpec::migrator")
    }

    /// The manifest migrate deadline.
    pub fn migrate_deadline_ms(self, ms: u64) -> PluginSpec {
        unimplemented!("host::testing::migration::PluginSpec::migrate_deadline_ms")
    }

    /// The restore blob `deactivate` returns.
    pub fn restore_bytes(self, bytes: Vec<u8>) -> PluginSpec {
        unimplemented!("host::testing::migration::PluginSpec::restore_bytes")
    }
}

/// The storage of one plugin as the plugin sees it (LIF-115, 117, 119, 120).
#[derive(Debug)]
pub struct StorageHandle {
    _private: (),
}

impl StorageHandle {
    /// `storage.put`.
    pub fn put(&self, bucket: Bucket, key: &str, value: &[u8]) -> Result<(), PluginError> {
        unimplemented!("host::testing::migration::StorageHandle::put")
    }

    /// `storage.get`.
    pub fn get(&self, bucket: Bucket, key: &str) -> Option<Vec<u8>> {
        unimplemented!("host::testing::migration::StorageHandle::get")
    }

    /// `storage.remove`.
    pub fn remove(&self, bucket: Bucket, key: &str) -> Result<(), PluginError> {
        unimplemented!("host::testing::migration::StorageHandle::remove")
    }

    /// `storage.keys` with a prefix.
    pub fn keys(&self, bucket: Bucket, prefix: &str) -> Vec<String> {
        unimplemented!("host::testing::migration::StorageHandle::keys")
    }

    /// `storage.write-batch`; `None` removes a key.
    pub fn write_batch(&self, bucket: Bucket, entries: &[(String, Option<Vec<u8>>)]) -> Result<(), PluginError> {
        unimplemented!("host::testing::migration::StorageHandle::write_batch")
    }

    /// Whether the storage offers compare-and-set.
    pub fn has_compare_and_set(&self) -> bool {
        unimplemented!("host::testing::migration::StorageHandle::has_compare_and_set")
    }

    /// Every key of a bucket with its value.
    pub fn dump(&self, bucket: Bucket) -> BTreeMap<String, Vec<u8>> {
        unimplemented!("host::testing::migration::StorageHandle::dump")
    }
}

/// One in-process server of the migration rig (LIF-101..121).
#[derive(Debug)]
pub struct MigServer {
    _private: (),
}

impl MigServer {
    /// Activates a plugin.
    pub fn activate(&self, id: &str) -> ActivateReport {
        unimplemented!("host::testing::migration::MigServer::activate")
    }

    /// Applies the pending update of a plugin.
    pub fn upgrade(&self, id: &str) -> ActivateReport {
        unimplemented!("host::testing::migration::MigServer::upgrade")
    }

    /// The storage of a plugin.
    pub fn storage(&self, id: &str) -> StorageHandle {
        unimplemented!("host::testing::migration::MigServer::storage")
    }

    /// The data-version stamp of a bucket.
    pub fn stamp(&self, id: &str, bucket: Bucket) -> Option<u32> {
        unimplemented!("host::testing::migration::MigServer::stamp")
    }

    /// Calls `migrate` directly.
    pub fn call_migrate(&self, id: &str, from: u32, to: u32) -> Result<(), PluginError> {
        unimplemented!("host::testing::migration::MigServer::call_migrate")
    }

    /// Kills the server.
    pub fn kill(&self) {
        unimplemented!("host::testing::migration::MigServer::kill")
    }
}

/// The migration rig (LIF-100..121).
#[derive(Debug)]
pub struct MigrationRig {
    _private: (),
}

impl MigrationRig {
    /// A fresh rig.
    pub fn new() -> MigrationRig {
        unimplemented!("host::testing::migration::MigrationRig::new")
    }

    /// Starts a server for a project session.
    pub fn server(&self, name: &str, project: &str, session: &str) -> MigServer {
        unimplemented!("host::testing::migration::MigrationRig::server")
    }

    /// Restarts a server.
    pub fn restart_server(&self, server: &MigServer) -> MigServer {
        unimplemented!("host::testing::migration::MigrationRig::restart_server")
    }

    /// Installs a plugin.
    pub fn install(&self, spec: &PluginSpec) -> CliOutput {
        unimplemented!("host::testing::migration::MigrationRig::install")
    }

    /// Updates a plugin.
    pub fn update(&self, spec: &PluginSpec, flags: &[&str]) -> CliOutput {
        unimplemented!("host::testing::migration::MigrationRig::update")
    }

    /// Rolls a plugin back.
    pub fn rollback(&self, id: &str, flags: &[&str]) -> CliOutput {
        unimplemented!("host::testing::migration::MigrationRig::rollback")
    }

    /// Runs a CLI command.
    pub fn cli(&self, args: &[&str]) -> CliOutput {
        unimplemented!("host::testing::migration::MigrationRig::cli")
    }

    /// Installs a package file with flags.
    pub fn cli_install_file(&self, spec: &PluginSpec, flags: &[&str]) -> CliOutput {
        unimplemented!("host::testing::migration::MigrationRig::cli_install_file")
    }

    /// `installed.json` parsed.
    pub fn registry_json(&self) -> serde_json::Value {
        unimplemented!("host::testing::migration::MigrationRig::registry_json")
    }

    /// The data-version stamp of a bucket.
    pub fn stamp(&self, id: &str, bucket: Bucket) -> Option<u32> {
        unimplemented!("host::testing::migration::MigrationRig::stamp")
    }

    /// The data-version stamp of the data directory.
    pub fn data_dir_stamp(&self, id: &str) -> Option<u32> {
        unimplemented!("host::testing::migration::MigrationRig::data_dir_stamp")
    }

    /// Where the stamp is stored.
    pub fn stamp_location(&self, id: &str) -> PathBuf {
        unimplemented!("host::testing::migration::MigrationRig::stamp_location")
    }

    /// The data directory of a plugin.
    pub fn plugin_data_dir(&self, id: &str) -> PathBuf {
        unimplemented!("host::testing::migration::MigrationRig::plugin_data_dir")
    }

    /// The files of the data directory, by relative path.
    pub fn data_dir_files(&self, id: &str) -> BTreeMap<String, Vec<u8>> {
        unimplemented!("host::testing::migration::MigrationRig::data_dir_files")
    }

    /// Writes a data file.
    pub fn write_data_file(&self, id: &str, name: &str, bytes: &[u8]) {
        unimplemented!("host::testing::migration::MigrationRig::write_data_file")
    }

    /// The backup names of a plugin.
    pub fn backups(&self, id: &str) -> Vec<String> {
        unimplemented!("host::testing::migration::MigrationRig::backups")
    }

    /// A bucket of a backup.
    pub fn backup_dump(&self, id: &str, backup: &str, bucket: Bucket) -> BTreeMap<String, Vec<u8>> {
        unimplemented!("host::testing::migration::MigrationRig::backup_dump")
    }

    /// The data files of a backup.
    pub fn backup_data_files(&self, id: &str, backup: &str) -> BTreeMap<String, Vec<u8>> {
        unimplemented!("host::testing::migration::MigrationRig::backup_data_files")
    }

    /// The `migrate` calls of a plugin.
    pub fn migrate_calls(&self, id: &str) -> Vec<MigrateCall> {
        unimplemented!("host::testing::migration::MigrationRig::migrate_calls")
    }

    /// The log codes so far.
    pub fn log_codes(&self) -> Vec<String> {
        unimplemented!("host::testing::migration::MigrationRig::log_codes")
    }

    /// The last log line with a code, as its JSON record (`level`, `code`, `message`).
    pub fn log_line(&self, code: &str) -> Option<serde_json::Value> {
        unimplemented!("host::testing::migration::MigrationRig::log_line")
    }

    /// Failures in the window of a plugin.
    pub fn failures_in_window(&self, id: &str) -> u32 {
        unimplemented!("host::testing::migration::MigrationRig::failures_in_window")
    }

    /// `plugin info` as JSON.
    pub fn info_json(&self, id: &str) -> serde_json::Value {
        unimplemented!("host::testing::migration::MigrationRig::info_json")
    }

    /// The migrate lock path.
    pub fn lock_path(&self, id: &str) -> PathBuf {
        unimplemented!("host::testing::migration::MigrationRig::lock_path")
    }

    /// Writes the config file.
    pub fn config(&self, toml: &str) {
        unimplemented!("host::testing::migration::MigrationRig::config")
    }

    /// Sets the wall clock.
    pub fn set_clock_utc(&self, rfc3339: &str) {
        unimplemented!("host::testing::migration::MigrationRig::set_clock_utc")
    }

    /// Moves the clock.
    pub fn advance_clock(&self, ms: u64) {
        unimplemented!("host::testing::migration::MigrationRig::advance_clock")
    }

    /// Sets the free disk space.
    pub fn set_free_bytes(&self, bytes: u64) {
        unimplemented!("host::testing::migration::MigrationRig::set_free_bytes")
    }

    /// Fills the disk.
    pub fn fill_disk(&self) {
        unimplemented!("host::testing::migration::MigrationRig::fill_disk")
    }

    /// Frees the disk.
    pub fn free_disk(&self) {
        unimplemented!("host::testing::migration::MigrationRig::free_disk")
    }

    /// Sets the quota of a bucket.
    pub fn set_plugin_quota(&self, id: &str, bucket: Bucket, bytes: u64) {
        unimplemented!("host::testing::migration::MigrationRig::set_plugin_quota")
    }

    /// Simulates a power loss.
    pub fn power_loss(&self) {
        unimplemented!("host::testing::migration::MigrationRig::power_loss")
    }

    /// Reads a value after a power loss.
    pub fn read_after_power_loss(&self, id: &str, bucket: Bucket, key: &str) -> Option<Vec<u8>> {
        unimplemented!("host::testing::migration::MigrationRig::read_after_power_loss")
    }

    /// Bytes not yet flushed.
    pub fn unflushed_bytes(&self, id: &str) -> u64 {
        unimplemented!("host::testing::migration::MigrationRig::unflushed_bytes")
    }

    /// Crashes during the next write.
    pub fn crash_during_next_write(&self) {
        unimplemented!("host::testing::migration::MigrationRig::crash_during_next_write")
    }
}

impl Default for MigrationRig {
    fn default() -> MigrationRig {
        MigrationRig::new()
    }
}

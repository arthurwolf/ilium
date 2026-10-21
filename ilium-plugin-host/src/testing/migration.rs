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
    pub fn new(_id: &str, _version: &str) -> PluginSpec {
        unimplemented!("host::testing::migration::PluginSpec::new")
    }

    /// The manifest `data_version`.
    pub fn data_version(self, _version: u32) -> PluginSpec {
        unimplemented!("host::testing::migration::PluginSpec::data_version")
    }

    /// The manifest `data_version` as raw JSON.
    pub fn data_version_json(self, _json: &str) -> PluginSpec {
        unimplemented!("host::testing::migration::PluginSpec::data_version_json")
    }

    /// The scripted `migrate` export.
    pub fn migrator(self, _steps: Vec<Step>) -> PluginSpec {
        unimplemented!("host::testing::migration::PluginSpec::migrator")
    }

    /// The manifest migrate deadline.
    pub fn migrate_deadline_ms(self, _ms: u64) -> PluginSpec {
        unimplemented!("host::testing::migration::PluginSpec::migrate_deadline_ms")
    }

    /// The restore blob `deactivate` returns.
    pub fn restore_bytes(self, _bytes: Vec<u8>) -> PluginSpec {
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
    pub fn put(&self, _bucket: Bucket, _key: &str, _value: &[u8]) -> Result<(), PluginError> {
        unimplemented!("host::testing::migration::StorageHandle::put")
    }

    /// `storage.get`.
    pub fn get(&self, _bucket: Bucket, _key: &str) -> Option<Vec<u8>> {
        unimplemented!("host::testing::migration::StorageHandle::get")
    }

    /// `storage.remove`.
    pub fn remove(&self, _bucket: Bucket, _key: &str) -> Result<(), PluginError> {
        unimplemented!("host::testing::migration::StorageHandle::remove")
    }

    /// `storage.keys` with a prefix.
    pub fn keys(&self, _bucket: Bucket, _prefix: &str) -> Vec<String> {
        unimplemented!("host::testing::migration::StorageHandle::keys")
    }

    /// `storage.write-batch`; `None` removes a key.
    pub fn write_batch(
        &self,
        _bucket: Bucket,
        _entries: &[(String, Option<Vec<u8>>)],
    ) -> Result<(), PluginError> {
        unimplemented!("host::testing::migration::StorageHandle::write_batch")
    }

    /// Whether the storage offers compare-and-set.
    pub fn has_compare_and_set(&self) -> bool {
        unimplemented!("host::testing::migration::StorageHandle::has_compare_and_set")
    }

    /// Every key of a bucket with its value.
    pub fn dump(&self, _bucket: Bucket) -> BTreeMap<String, Vec<u8>> {
        unimplemented!("host::testing::migration::StorageHandle::dump")
    }
}

/// One in-process server of the migration rig (LIF-101..121).
#[derive(Debug)]
pub struct MigServer {
    _private: (),
}

impl MigServer {
    /// Whether the server still runs (COR-090: it boots with defaults after a parse error).
    pub fn is_running(&self) -> bool {
        unimplemented!("host::testing::migration::MigServer::is_running")
    }

    /// Writes one setting through the host's settings write path (COR-090).
    pub fn write_setting(&self, _table: &str, _key: &str, _value: &str) {
        unimplemented!("host::testing::migration::MigServer::write_setting")
    }

    /// Activates a plugin.
    pub fn activate(&self, _id: &str) -> ActivateReport {
        unimplemented!("host::testing::migration::MigServer::activate")
    }

    /// Applies the pending update of a plugin.
    pub fn upgrade(&self, _id: &str) -> ActivateReport {
        unimplemented!("host::testing::migration::MigServer::upgrade")
    }

    /// The storage of a plugin.
    pub fn storage(&self, _id: &str) -> StorageHandle {
        unimplemented!("host::testing::migration::MigServer::storage")
    }

    /// The data-version stamp of a bucket.
    pub fn stamp(&self, _id: &str, _bucket: Bucket) -> Option<u32> {
        unimplemented!("host::testing::migration::MigServer::stamp")
    }

    /// Calls `migrate` directly.
    pub fn call_migrate(&self, _id: &str, _from: u32, _to: u32) -> Result<(), PluginError> {
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
    pub fn server(&self, _name: &str, _project: &str, _session: &str) -> MigServer {
        unimplemented!("host::testing::migration::MigrationRig::server")
    }

    /// Restarts a server.
    pub fn restart_server(&self, _server: &MigServer) -> MigServer {
        unimplemented!("host::testing::migration::MigrationRig::restart_server")
    }

    /// Installs a plugin.
    pub fn install(&self, _spec: &PluginSpec) -> CliOutput {
        unimplemented!("host::testing::migration::MigrationRig::install")
    }

    /// Updates a plugin.
    pub fn update(&self, _spec: &PluginSpec, _flags: &[&str]) -> CliOutput {
        unimplemented!("host::testing::migration::MigrationRig::update")
    }

    /// Rolls a plugin back.
    pub fn rollback(&self, _id: &str, _flags: &[&str]) -> CliOutput {
        unimplemented!("host::testing::migration::MigrationRig::rollback")
    }

    /// Runs a CLI command.
    pub fn cli(&self, _args: &[&str]) -> CliOutput {
        unimplemented!("host::testing::migration::MigrationRig::cli")
    }

    /// Installs a package file with flags.
    pub fn cli_install_file(&self, _spec: &PluginSpec, _flags: &[&str]) -> CliOutput {
        unimplemented!("host::testing::migration::MigrationRig::cli_install_file")
    }

    /// `installed.json` parsed.
    pub fn registry_json(&self) -> serde_json::Value {
        unimplemented!("host::testing::migration::MigrationRig::registry_json")
    }

    /// The data-version stamp of a bucket.
    pub fn stamp(&self, _id: &str, _bucket: Bucket) -> Option<u32> {
        unimplemented!("host::testing::migration::MigrationRig::stamp")
    }

    /// The data-version stamp of the data directory.
    pub fn data_dir_stamp(&self, _id: &str) -> Option<u32> {
        unimplemented!("host::testing::migration::MigrationRig::data_dir_stamp")
    }

    /// Where the stamp is stored.
    pub fn stamp_location(&self, _id: &str) -> PathBuf {
        unimplemented!("host::testing::migration::MigrationRig::stamp_location")
    }

    /// The data directory of a plugin.
    pub fn plugin_data_dir(&self, _id: &str) -> PathBuf {
        unimplemented!("host::testing::migration::MigrationRig::plugin_data_dir")
    }

    /// The files of the data directory, by relative path.
    pub fn data_dir_files(&self, _id: &str) -> BTreeMap<String, Vec<u8>> {
        unimplemented!("host::testing::migration::MigrationRig::data_dir_files")
    }

    /// Writes a data file.
    pub fn write_data_file(&self, _id: &str, _name: &str, _bytes: &[u8]) {
        unimplemented!("host::testing::migration::MigrationRig::write_data_file")
    }

    /// The backup names of a plugin.
    pub fn backups(&self, _id: &str) -> Vec<String> {
        unimplemented!("host::testing::migration::MigrationRig::backups")
    }

    /// A bucket of a backup.
    pub fn backup_dump(
        &self,
        _id: &str,
        _backup: &str,
        _bucket: Bucket,
    ) -> BTreeMap<String, Vec<u8>> {
        unimplemented!("host::testing::migration::MigrationRig::backup_dump")
    }

    /// The data files of a backup.
    pub fn backup_data_files(&self, _id: &str, _backup: &str) -> BTreeMap<String, Vec<u8>> {
        unimplemented!("host::testing::migration::MigrationRig::backup_data_files")
    }

    /// The `migrate` calls of a plugin.
    pub fn migrate_calls(&self, _id: &str) -> Vec<MigrateCall> {
        unimplemented!("host::testing::migration::MigrationRig::migrate_calls")
    }

    /// The log codes so far.
    pub fn log_codes(&self) -> Vec<String> {
        unimplemented!("host::testing::migration::MigrationRig::log_codes")
    }

    /// The last log line with a code, as its JSON record (`level`, `code`, `message`).
    pub fn log_line(&self, _code: &str) -> Option<serde_json::Value> {
        unimplemented!("host::testing::migration::MigrationRig::log_line")
    }

    /// Failures in the window of a plugin.
    pub fn failures_in_window(&self, _id: &str) -> u32 {
        unimplemented!("host::testing::migration::MigrationRig::failures_in_window")
    }

    /// `plugin info` as JSON.
    pub fn info_json(&self, _id: &str) -> serde_json::Value {
        unimplemented!("host::testing::migration::MigrationRig::info_json")
    }

    /// The migrate lock path.
    pub fn lock_path(&self, _id: &str) -> PathBuf {
        unimplemented!("host::testing::migration::MigrationRig::lock_path")
    }

    /// The current bytes of the config file as text (COR-090).
    pub fn config_text(&self) -> String {
        unimplemented!("host::testing::migration::MigrationRig::config_text")
    }

    /// Writes the config file.
    pub fn config(&self, _toml: &str) {
        unimplemented!("host::testing::migration::MigrationRig::config")
    }

    /// Sets the wall clock.
    pub fn set_clock_utc(&self, _rfc3339: &str) {
        unimplemented!("host::testing::migration::MigrationRig::set_clock_utc")
    }

    /// Moves the clock.
    pub fn advance_clock(&self, _ms: u64) {
        unimplemented!("host::testing::migration::MigrationRig::advance_clock")
    }

    /// Sets the free disk space.
    pub fn set_free_bytes(&self, _bytes: u64) {
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
    pub fn set_plugin_quota(&self, _id: &str, _bucket: Bucket, _bytes: u64) {
        unimplemented!("host::testing::migration::MigrationRig::set_plugin_quota")
    }

    /// Simulates a power loss.
    pub fn power_loss(&self) {
        unimplemented!("host::testing::migration::MigrationRig::power_loss")
    }

    /// Reads a value after a power loss.
    pub fn read_after_power_loss(&self, _id: &str, _bucket: Bucket, _key: &str) -> Option<Vec<u8>> {
        unimplemented!("host::testing::migration::MigrationRig::read_after_power_loss")
    }

    /// Bytes not yet flushed.
    pub fn unflushed_bytes(&self, _id: &str) -> u64 {
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

#[cfg(test)]
mod tests {
    use ilium_plugin_api::{ErrorCode, PluginError};

    use super::{Bucket, MigServer, MigrateScope, MigrationRig, PluginSpec, Step};
    use crate::cli::CliOutput;

    const PLUGIN_ID: &str = "dev.ilium.migration-fixture";
    const OLD_VERSION: &str = "1.0.0";
    const NEW_VERSION: &str = "2.0.0";
    const FOUR_MIB: usize = 4 * 1024 * 1024;
    const LARGE_QUOTA: u64 = 64 * 1024 * 1024;

    fn plugin_error(code: ErrorCode, message: &str) -> PluginError {
        PluginError {
            code,
            message: message.to_string(),
            retry_after_ms: None,
        }
    }

    fn assert_succeeded(output: &CliOutput) {
        assert_eq!(output.exit_code, 0, "command failed: {}", output.stderr);
    }

    fn assert_refused_with(output: &CliOutput, code: &str) {
        assert_ne!(output.exit_code, 0, "command was accepted");
        let mentioned = output.stderr.contains(code) || output.stdout_raw.contains(code);
        assert!(mentioned, "refusal does not name {code}: {}", output.stderr);
    }

    fn install_v1(rig: &MigrationRig, data_version: u32) {
        let spec = PluginSpec::new(PLUGIN_ID, OLD_VERSION).data_version(data_version);
        assert_succeeded(&rig.install(&spec));
    }

    fn update_to_v2(rig: &MigrationRig, data_version: u32, steps: Vec<Step>) -> CliOutput {
        let spec = PluginSpec::new(PLUGIN_ID, NEW_VERSION)
            .data_version(data_version)
            .migrator(steps);
        rig.update(&spec, &[])
    }

    fn count_calls(rig: &MigrationRig, scope: &MigrateScope, from: u32, to: u32) -> usize {
        rig.migrate_calls(PLUGIN_ID)
            .iter()
            .filter(|call| call.from == from && call.to == to && call.scope == *scope)
            .count()
    }

    fn session_scope(project: &str, session: &str) -> MigrateScope {
        MigrateScope::Session {
            project: project.to_string(),
            session: session.to_string(),
        }
    }

    fn only_backup_name(rig: &MigrationRig) -> String {
        let backups = rig.backups(PLUGIN_ID);
        assert_eq!(backups.len(), 1, "expected exactly one backup, found {backups:?}");
        backups[0].clone()
    }

    fn is_stale_data_version(error: Option<PluginError>) -> bool {
        matches!(
            error,
            Some(PluginError { code: ErrorCode::Conflict, message, .. })
                if message == "stale-data-version"
        )
    }

    /// Two sessions of one project. `stale` stays on version 1 while `fresh` migrates the
    /// project and global stamps to version 3.
    fn stale_and_fresh_servers(rig: &MigrationRig) -> (MigServer, MigServer) {
        let stale = rig.server("stale", "/projects/alpha", "s1");
        let fresh = rig.server("fresh", "/projects/alpha", "s2");
        install_v1(rig, 1);
        stale.activate(PLUGIN_ID);
        fresh.activate(PLUGIN_ID);
        assert_succeeded(&update_to_v2(rig, 3, vec![]));
        fresh.upgrade(PLUGIN_ID);
        (stale, fresh)
    }

    /// REQ: LIF-100
    #[test]
    fn manifest_data_version_zero_is_refused() {
        let rig = MigrationRig::new();
        let output = rig.install(&PluginSpec::new(PLUGIN_ID, OLD_VERSION).data_version(0));
        assert_refused_with(&output, "manifest.data-version");
    }

    /// REQ: LIF-100
    #[test]
    fn manifest_data_version_string_is_refused() {
        let rig = MigrationRig::new();
        let output = rig.install(
            &PluginSpec::new(PLUGIN_ID, OLD_VERSION).data_version_json("\"2\""),
        );
        assert_refused_with(&output, "manifest.data-version");
    }

    /// REQ: LIF-100
    #[test]
    fn manifest_data_version_upper_bound_is_accepted() {
        let rig = MigrationRig::new();
        let output = rig.install(
            &PluginSpec::new(PLUGIN_ID, OLD_VERSION).data_version(u32::MAX),
        );
        assert_succeeded(&output);
    }

    /// REQ: LIF-100
    #[test]
    fn absent_data_version_stamps_bucket_as_one() {
        let rig = MigrationRig::new();
        assert_succeeded(&rig.install(&PluginSpec::new(PLUGIN_ID, OLD_VERSION)));
        let server = rig.server("main", "/projects/alpha", "default");
        server.activate(PLUGIN_ID);
        assert!(server.storage(PLUGIN_ID).put(Bucket::Global, "key", b"value").is_ok());
        assert_eq!(rig.stamp(PLUGIN_ID, Bucket::Global), Some(1));
    }

    /// REQ: LIF-101
    #[test]
    fn stamp_is_readable_after_successful_migration() {
        let rig = MigrationRig::new();
        let server = rig.server("main", "/projects/alpha", "default");
        install_v1(&rig, 1);
        server.activate(PLUGIN_ID);
        assert!(server.storage(PLUGIN_ID).put(Bucket::Global, "key", b"value").is_ok());
        assert_succeeded(&update_to_v2(&rig, 3, vec![]));
        server.upgrade(PLUGIN_ID);
        assert_eq!(rig.stamp(PLUGIN_ID, Bucket::Global), Some(3));
        assert_eq!(rig.data_dir_stamp(PLUGIN_ID), Some(3));
    }

    /// REQ: LIF-102
    #[test]
    fn unstamped_bucket_is_stamped_at_first_write_without_migration() {
        let rig = MigrationRig::new();
        let server = rig.server("main", "/projects/alpha", "default");
        assert_succeeded(&rig.install(&PluginSpec::new(PLUGIN_ID, OLD_VERSION).data_version(4)));
        server.activate(PLUGIN_ID);
        assert!(server.storage(PLUGIN_ID).put(Bucket::Global, "key", b"value").is_ok());
        assert_eq!(rig.stamp(PLUGIN_ID, Bucket::Global), Some(4));
        assert!(rig.migrate_calls(PLUGIN_ID).is_empty());
    }

    /// REQ: LIF-103
    #[test]
    fn restore_bytes_dropped_when_data_version_changes() {
        let rig = MigrationRig::new();
        let server = rig.server("main", "/projects/alpha", "default");
        let spec = PluginSpec::new(PLUGIN_ID, OLD_VERSION)
            .data_version(1)
            .restore_bytes(b"blob".to_vec());
        assert_succeeded(&rig.install(&spec));
        server.activate(PLUGIN_ID);
        assert_succeeded(&update_to_v2(&rig, 2, vec![]));
        let report = server.upgrade(PLUGIN_ID);
        assert_eq!(report.restored, None);
        assert!(rig.log_line("restore.dropped-data-version").is_some());
    }

    /// REQ: LIF-103
    #[test]
    fn restore_bytes_kept_when_data_version_is_unchanged() {
        let rig = MigrationRig::new();
        let server = rig.server("main", "/projects/alpha", "default");
        let spec = PluginSpec::new(PLUGIN_ID, OLD_VERSION)
            .data_version(1)
            .restore_bytes(b"blob".to_vec());
        assert_succeeded(&rig.install(&spec));
        server.activate(PLUGIN_ID);
        assert_succeeded(&update_to_v2(&rig, 1, vec![]));
        let report = server.upgrade(PLUGIN_ID);
        assert_eq!(report.restored, Some(b"blob".to_vec()));
    }

    /// REQ: LIF-104
    #[test]
    fn migrate_runs_once_per_bucket_scope_per_update() {
        let rig = MigrationRig::new();
        let first = rig.server("a", "/projects/alpha", "s1");
        let second = rig.server("b", "/projects/beta", "s2");
        install_v1(&rig, 1);
        first.activate(PLUGIN_ID);
        second.activate(PLUGIN_ID);
        assert!(first.storage(PLUGIN_ID).put(Bucket::Session, "key", b"a").is_ok());
        assert!(second.storage(PLUGIN_ID).put(Bucket::Session, "key", b"b").is_ok());
        assert_succeeded(&update_to_v2(&rig, 3, vec![]));
        first.upgrade(PLUGIN_ID);
        second.upgrade(PLUGIN_ID);
        first.activate(PLUGIN_ID);
        assert_eq!(count_calls(&rig, &MigrateScope::Global, 1, 3), 1);
        assert_eq!(count_calls(&rig, &session_scope("/projects/alpha", "s1"), 1, 3), 1);
        assert_eq!(count_calls(&rig, &session_scope("/projects/beta", "s2"), 1, 3), 1);
    }

    /// REQ: LIF-104
    #[test]
    fn migrate_skipped_when_data_version_is_unchanged() {
        let rig = MigrationRig::new();
        let server = rig.server("main", "/projects/alpha", "default");
        install_v1(&rig, 2);
        server.activate(PLUGIN_ID);
        let steps = vec![Step::Put(Bucket::Global, "key".to_string(), b"new".to_vec())];
        assert_succeeded(&update_to_v2(&rig, 2, steps));
        server.upgrade(PLUGIN_ID);
        assert!(rig.migrate_calls(PLUGIN_ID).is_empty());
    }

    /// REQ: LIF-105
    #[test]
    fn migrate_error_does_not_count_toward_failure_window() {
        let rig = MigrationRig::new();
        let server = rig.server("main", "/projects/alpha", "default");
        install_v1(&rig, 1);
        server.activate(PLUGIN_ID);
        let steps = vec![Step::Fail(plugin_error(ErrorCode::Internal, "boom"))];
        assert_succeeded(&update_to_v2(&rig, 3, steps));
        let report = server.upgrade(PLUGIN_ID);
        assert!(report.rolled_back);
        assert_eq!(rig.failures_in_window(PLUGIN_ID), 0);
        assert!(rig.migrate_calls(PLUGIN_ID).iter().any(|call| !call.returned_ok));
    }

    /// REQ: LIF-105
    #[test]
    fn migrate_trap_does_not_count_toward_failure_window() {
        let rig = MigrationRig::new();
        let server = rig.server("main", "/projects/alpha", "default");
        install_v1(&rig, 1);
        server.activate(PLUGIN_ID);
        assert_succeeded(&update_to_v2(&rig, 3, vec![Step::Trap]));
        let report = server.upgrade(PLUGIN_ID);
        assert!(report.rolled_back);
        assert_eq!(rig.failures_in_window(PLUGIN_ID), 0);
    }

    /// REQ: LIF-105
    #[test]
    fn migrate_deadline_miss_does_not_count_toward_failure_window() {
        let rig = MigrationRig::new();
        let server = rig.server("main", "/projects/alpha", "default");
        install_v1(&rig, 1);
        server.activate(PLUGIN_ID);
        let spec = PluginSpec::new(PLUGIN_ID, NEW_VERSION)
            .data_version(3)
            .migrator(vec![Step::Hang])
            .migrate_deadline_ms(50);
        assert_succeeded(&rig.update(&spec, &[]));
        let report = server.upgrade(PLUGIN_ID);
        assert!(report.rolled_back);
        assert_eq!(rig.failures_in_window(PLUGIN_ID), 0);
    }

    /// REQ: LIF-106
    #[test]
    fn migrate_called_twice_with_same_arguments_leaves_same_data() {
        let rig = MigrationRig::new();
        let server = rig.server("main", "/projects/alpha", "default");
        install_v1(&rig, 1);
        server.activate(PLUGIN_ID);
        assert!(server.storage(PLUGIN_ID).put(Bucket::Global, "key", b"old").is_ok());
        let steps = vec![Step::Put(Bucket::Global, "key".to_string(), b"new".to_vec())];
        assert_succeeded(&update_to_v2(&rig, 3, steps));
        assert!(server.call_migrate(PLUGIN_ID, 1, 3).is_ok());
        let after_first = server.storage(PLUGIN_ID).dump(Bucket::Global);
        assert!(server.call_migrate(PLUGIN_ID, 1, 3).is_ok());
        let after_second = server.storage(PLUGIN_ID).dump(Bucket::Global);
        assert_eq!(after_first, after_second);
        assert_eq!(after_second.get("key"), Some(&b"new".to_vec()));
    }

    /// REQ: LIF-107
    #[test]
    fn backup_taken_before_migrate_uses_timestamped_name() {
        let rig = MigrationRig::new();
        let server = rig.server("main", "/projects/alpha", "default");
        install_v1(&rig, 1);
        server.activate(PLUGIN_ID);
        assert!(server.storage(PLUGIN_ID).put(Bucket::Global, "key", b"old").is_ok());
        assert_succeeded(&update_to_v2(&rig, 3, vec![]));
        server.upgrade(PLUGIN_ID);
        let name = only_backup_name(&rig);
        let timestamp = name.rsplit('-').next().unwrap_or_default();
        assert_eq!(timestamp.len(), 16, "timestamp in {name}");
        assert!(timestamp.ends_with('Z'));
        assert_eq!(timestamp.as_bytes()[8], b'T');
        assert!(timestamp[..8].bytes().all(|byte| byte.is_ascii_digit()));
        let global = rig.backup_dump(PLUGIN_ID, &name, Bucket::Global);
        assert_eq!(global.get("key"), Some(&b"old".to_vec()));
    }

    /// REQ: LIF-107
    #[test]
    fn zero_backup_cap_disables_backups() {
        let rig = MigrationRig::new();
        rig.config("[plugins]\nmigration_backup_mib = 0\n");
        let server = rig.server("main", "/projects/alpha", "default");
        install_v1(&rig, 1);
        server.activate(PLUGIN_ID);
        assert_succeeded(&update_to_v2(&rig, 3, vec![]));
        server.upgrade(PLUGIN_ID);
        assert!(rig.backups(PLUGIN_ID).is_empty());
    }

    /// REQ: LIF-107
    #[test]
    fn update_refused_with_no_space_before_touching_data() {
        let rig = MigrationRig::new();
        let server = rig.server("main", "/projects/alpha", "default");
        install_v1(&rig, 1);
        server.activate(PLUGIN_ID);
        assert!(server.storage(PLUGIN_ID).put(Bucket::Global, "key", b"old").is_ok());
        rig.set_free_bytes(0);
        let output = update_to_v2(&rig, 3, vec![]);
        assert_ne!(output.exit_code, 0);
        assert!(rig.log_codes().iter().any(|code| code == "install.no-space"));
        assert_eq!(rig.stamp(PLUGIN_ID, Bucket::Global), Some(1));
    }

    /// REQ: LIF-108
    #[test]
    fn failed_migration_restores_backup_keeps_old_version_and_stamp() {
        let rig = MigrationRig::new();
        let server = rig.server("main", "/projects/alpha", "default");
        install_v1(&rig, 1);
        server.activate(PLUGIN_ID);
        assert!(server.storage(PLUGIN_ID).put(Bucket::Global, "a", b"old").is_ok());
        assert!(server.storage(PLUGIN_ID).put(Bucket::Global, "b", b"old").is_ok());
        let steps = vec![
            Step::Put(Bucket::Global, "a".to_string(), b"half".to_vec()),
            Step::Fail(plugin_error(ErrorCode::Internal, "half-written")),
        ];
        assert_succeeded(&update_to_v2(&rig, 3, steps));
        let report = server.upgrade(PLUGIN_ID);
        assert!(report.rolled_back);
        assert_eq!(report.active_version, OLD_VERSION);
        let name = only_backup_name(&rig);
        assert_eq!(
            server.storage(PLUGIN_ID).dump(Bucket::Global),
            rig.backup_dump(PLUGIN_ID, &name, Bucket::Global)
        );
        assert_eq!(rig.stamp(PLUGIN_ID, Bucket::Global), Some(1));
        let rendered = rig
            .log_line("migration.failed")
            .map(|record| record.to_string())
            .unwrap_or_default();
        assert!(rendered.contains("half-written"), "log record: {rendered}");
    }

    /// REQ: LIF-109
    #[test]
    fn only_newest_backup_kept_after_two_successful_updates() {
        let rig = MigrationRig::new();
        let server = rig.server("main", "/projects/alpha", "default");
        install_v1(&rig, 1);
        server.activate(PLUGIN_ID);
        rig.set_clock_utc("2026-10-21T10:00:00Z");
        assert_succeeded(&update_to_v2(&rig, 2, vec![]));
        server.upgrade(PLUGIN_ID);
        rig.set_clock_utc("2026-10-21T11:00:00Z");
        assert_succeeded(&update_to_v2(&rig, 3, vec![]));
        server.upgrade(PLUGIN_ID);
        let name = only_backup_name(&rig);
        assert!(name.ends_with("20261021T110000Z"), "kept backup {name}");
    }

    /// REQ: LIF-110
    #[test]
    fn interrupted_migration_is_rerun_from_backup_on_next_start() {
        let rig = MigrationRig::new();
        let server = rig.server("main", "/projects/alpha", "default");
        install_v1(&rig, 1);
        server.activate(PLUGIN_ID);
        assert!(server.storage(PLUGIN_ID).put(Bucket::Global, "key", b"old").is_ok());
        let steps = vec![Step::KillHostFirstRun];
        assert_succeeded(&update_to_v2(&rig, 3, steps));
        let report = server.upgrade(PLUGIN_ID);
        assert!(report.killed);
        let restarted = rig.restart_server(&server);
        assert!(restarted.is_running());
        assert_eq!(rig.stamp(PLUGIN_ID, Bucket::Global), Some(3));
    }

    /// REQ: LIF-110
    #[test]
    fn second_interrupted_migration_rolls_back_and_marks_skip() {
        let rig = MigrationRig::new();
        let server = rig.server("main", "/projects/alpha", "default");
        install_v1(&rig, 1);
        server.activate(PLUGIN_ID);
        assert_succeeded(&update_to_v2(&rig, 3, vec![Step::KillHost]));
        let report = server.upgrade(PLUGIN_ID);
        assert!(report.killed);
        let second = rig.restart_server(&server);
        let third = rig.restart_server(&second);
        assert!(third.is_running());
        assert!(rig.log_line("migration.interrupted").is_some());
        assert!(rig.registry_json().to_string().contains("\"skip\""));
        assert_eq!(rig.stamp(PLUGIN_ID, Bucket::Global), Some(1));
    }

    /// REQ: LIF-111
    #[test]
    fn rollback_below_stamp_refused_without_flags() {
        let rig = MigrationRig::new();
        let server = rig.server("main", "/projects/alpha", "default");
        install_v1(&rig, 1);
        server.activate(PLUGIN_ID);
        assert_succeeded(&update_to_v2(&rig, 3, vec![]));
        server.upgrade(PLUGIN_ID);
        let output = rig.rollback(PLUGIN_ID, &[]);
        assert_refused_with(&output, "migration.data-newer");
        assert_eq!(rig.stamp(PLUGIN_ID, Bucket::Global), Some(3));
    }

    /// REQ: LIF-111
    #[test]
    fn rollback_with_restore_backup_restores_previous_contents() {
        let rig = MigrationRig::new();
        let server = rig.server("main", "/projects/alpha", "default");
        install_v1(&rig, 1);
        server.activate(PLUGIN_ID);
        assert!(server.storage(PLUGIN_ID).put(Bucket::Global, "key", b"old").is_ok());
        assert_succeeded(&update_to_v2(&rig, 3, vec![]));
        server.upgrade(PLUGIN_ID);
        assert_succeeded(&rig.rollback(PLUGIN_ID, &["--restore-backup"]));
        assert_eq!(rig.stamp(PLUGIN_ID, Bucket::Global), Some(1));
        assert_eq!(server.storage(PLUGIN_ID).get(Bucket::Global, "key"), Some(b"old".to_vec()));
    }

    /// REQ: LIF-111
    #[test]
    fn rollback_with_discard_data_deletes_buckets_and_data_dir() {
        let rig = MigrationRig::new();
        let server = rig.server("main", "/projects/alpha", "default");
        install_v1(&rig, 1);
        server.activate(PLUGIN_ID);
        rig.write_data_file(PLUGIN_ID, "data.bin", b"x");
        assert_succeeded(&update_to_v2(&rig, 3, vec![]));
        server.upgrade(PLUGIN_ID);
        assert_succeeded(&rig.rollback(PLUGIN_ID, &["--discard-data"]));
        assert_eq!(rig.stamp(PLUGIN_ID, Bucket::Global), None);
        assert!(rig.data_dir_files(PLUGIN_ID).is_empty());
    }

    /// REQ: LIF-112
    #[test]
    fn global_migration_runs_once_across_two_servers() {
        let rig = MigrationRig::new();
        let first = rig.server("a", "/projects/alpha", "s1");
        let second = rig.server("b", "/projects/beta", "s2");
        install_v1(&rig, 1);
        first.activate(PLUGIN_ID);
        second.activate(PLUGIN_ID);
        assert_succeeded(&update_to_v2(&rig, 3, vec![]));
        first.upgrade(PLUGIN_ID);
        second.upgrade(PLUGIN_ID);
        assert_eq!(count_calls(&rig, &MigrateScope::Global, 1, 3), 1);
        assert_eq!(rig.stamp(PLUGIN_ID, Bucket::Global), Some(3));
    }

    /// REQ: LIF-113
    #[test]
    fn writes_from_stale_instance_conflict_on_global_and_project_buckets() {
        let rig = MigrationRig::new();
        let (stale, _fresh) = stale_and_fresh_servers(&rig);
        let storage = stale.storage(PLUGIN_ID);
        assert!(is_stale_data_version(storage.put(Bucket::Global, "key", b"x").err()));
        assert!(is_stale_data_version(storage.put(Bucket::Project, "key", b"x").err()));
    }

    /// REQ: LIF-114
    #[test]
    fn each_project_and_session_migrates_once_in_its_own_session() {
        let rig = MigrationRig::new();
        let alpha = rig.server("a", "/projects/alpha", "s1");
        let beta = rig.server("b", "/projects/beta", "s2");
        install_v1(&rig, 1);
        alpha.activate(PLUGIN_ID);
        beta.activate(PLUGIN_ID);
        assert!(alpha.storage(PLUGIN_ID).put(Bucket::Session, "key", b"a").is_ok());
        assert!(beta.storage(PLUGIN_ID).put(Bucket::Session, "key", b"b").is_ok());
        assert_succeeded(&update_to_v2(&rig, 3, vec![]));
        alpha.upgrade(PLUGIN_ID);
        beta.upgrade(PLUGIN_ID);
        assert_eq!(count_calls(&rig, &session_scope("/projects/alpha", "s1"), 1, 3), 1);
        assert_eq!(count_calls(&rig, &session_scope("/projects/beta", "s2"), 1, 3), 1);
    }

    /// REQ: LIF-115
    #[test]
    fn put_is_flushed_before_return_and_survives_power_loss() {
        let rig = MigrationRig::new();
        let server = rig.server("main", "/projects/alpha", "default");
        install_v1(&rig, 1);
        server.activate(PLUGIN_ID);
        assert!(server.storage(PLUGIN_ID).put(Bucket::Global, "key", b"durable").is_ok());
        assert_eq!(rig.unflushed_bytes(PLUGIN_ID), 0);
        rig.power_loss();
        assert_eq!(
            rig.read_after_power_loss(PLUGIN_ID, Bucket::Global, "key"),
            Some(b"durable".to_vec())
        );
    }

    /// REQ: LIF-116
    #[test]
    fn crash_between_data_file_writes_leaves_version_marker_old() {
        let rig = MigrationRig::new();
        let server = rig.server("main", "/projects/alpha", "default");
        install_v1(&rig, 1);
        server.activate(PLUGIN_ID);
        rig.write_data_file(PLUGIN_ID, "set.bin", b"old-set");
        rig.write_data_file(PLUGIN_ID, "set.version", b"1");
        let steps = vec![
            Step::WriteFile("set.bin".to_string(), b"new-set".to_vec()),
            Step::KillHost,
            Step::WriteFile("set.version".to_string(), b"3".to_vec()),
        ];
        assert_succeeded(&update_to_v2(&rig, 3, steps));
        let report = server.upgrade(PLUGIN_ID);
        assert!(report.killed);
        assert_eq!(
            rig.data_dir_files(PLUGIN_ID).get("set.version"),
            Some(&b"1".to_vec())
        );
    }

    /// REQ: LIF-117
    #[test]
    fn refused_batch_keeps_every_previous_value() {
        let rig = MigrationRig::new();
        let server = rig.server("main", "/projects/alpha", "default");
        install_v1(&rig, 1);
        server.activate(PLUGIN_ID);
        rig.set_plugin_quota(PLUGIN_ID, Bucket::Global, 64);
        let storage = server.storage(PLUGIN_ID);
        assert!(storage.put(Bucket::Global, "a", b"x").is_ok());
        let entries = vec![
            ("b".to_string(), Some(vec![0u8; 4])),
            ("c".to_string(), Some(vec![0u8; 100])),
        ];
        let error = storage.write_batch(Bucket::Global, &entries).err();
        assert!(error.is_some_and(|error| error.code == ErrorCode::QuotaExceeded));
        assert_eq!(storage.get(Bucket::Global, "a"), Some(b"x".to_vec()));
        assert_eq!(storage.get(Bucket::Global, "b"), None);
        assert_eq!(storage.get(Bucket::Global, "c"), None);
    }

    /// REQ: LIF-117
    #[test]
    fn batch_of_256_entries_is_accepted() {
        let rig = MigrationRig::new();
        let server = rig.server("main", "/projects/alpha", "default");
        install_v1(&rig, 1);
        server.activate(PLUGIN_ID);
        rig.set_plugin_quota(PLUGIN_ID, Bucket::Global, LARGE_QUOTA);
        let storage = server.storage(PLUGIN_ID);
        let entries: Vec<(String, Option<Vec<u8>>)> = (0..256)
            .map(|index| (format!("key-{index}"), Some(vec![1u8])))
            .collect();
        assert!(storage.write_batch(Bucket::Global, &entries).is_ok());
        assert_eq!(storage.keys(Bucket::Global, "key-").len(), 256);
    }

    /// REQ: LIF-117
    #[test]
    fn batch_of_257_entries_is_invalid_argument() {
        let rig = MigrationRig::new();
        let server = rig.server("main", "/projects/alpha", "default");
        install_v1(&rig, 1);
        server.activate(PLUGIN_ID);
        rig.set_plugin_quota(PLUGIN_ID, Bucket::Global, LARGE_QUOTA);
        let storage = server.storage(PLUGIN_ID);
        let entries: Vec<(String, Option<Vec<u8>>)> = (0..257)
            .map(|index| (format!("key-{index}"), Some(vec![1u8])))
            .collect();
        let error = storage.write_batch(Bucket::Global, &entries).err();
        assert!(error.is_some_and(|error| error.code == ErrorCode::InvalidArgument));
        assert!(storage.keys(Bucket::Global, "key-").is_empty());
    }

    /// REQ: LIF-117
    #[test]
    fn batch_over_four_mib_of_key_and_value_bytes_is_invalid_argument() {
        let rig = MigrationRig::new();
        let server = rig.server("main", "/projects/alpha", "default");
        install_v1(&rig, 1);
        server.activate(PLUGIN_ID);
        rig.set_plugin_quota(PLUGIN_ID, Bucket::Global, LARGE_QUOTA);
        let storage = server.storage(PLUGIN_ID);
        // Key "big" (3 bytes) plus a value of exactly 4 MiB is one byte over the 4 MiB total.
        let entries = vec![("big".to_string(), Some(vec![0u8; FOUR_MIB]))];
        let error = storage.write_batch(Bucket::Global, &entries).err();
        assert!(error.is_some_and(|error| error.code == ErrorCode::InvalidArgument));
        assert_eq!(storage.get(Bucket::Global, "big"), None);
    }

    /// REQ: LIF-118
    #[test]
    fn crash_during_batch_leaves_previous_value() {
        let rig = MigrationRig::new();
        let server = rig.server("main", "/projects/alpha", "default");
        install_v1(&rig, 1);
        server.activate(PLUGIN_ID);
        let storage = server.storage(PLUGIN_ID);
        assert!(storage.put(Bucket::Global, "key", b"old").is_ok());
        rig.crash_during_next_write();
        let entries = vec![("key".to_string(), Some(b"new".to_vec()))];
        let _outcome = storage.write_batch(Bucket::Global, &entries);
        rig.power_loss();
        assert_eq!(
            rig.read_after_power_loss(PLUGIN_ID, Bucket::Global, "key"),
            Some(b"old".to_vec())
        );
    }

    /// REQ: LIF-118
    #[test]
    fn remove_is_flushed_before_return() {
        let rig = MigrationRig::new();
        let server = rig.server("main", "/projects/alpha", "default");
        install_v1(&rig, 1);
        server.activate(PLUGIN_ID);
        let storage = server.storage(PLUGIN_ID);
        assert!(storage.put(Bucket::Global, "key", b"value").is_ok());
        assert!(storage.remove(Bucket::Global, "key").is_ok());
        assert_eq!(rig.unflushed_bytes(PLUGIN_ID), 0);
        rig.power_loss();
        assert_eq!(rig.read_after_power_loss(PLUGIN_ID, Bucket::Global, "key"), None);
    }

    /// REQ: LIF-119
    #[test]
    fn full_disk_put_returns_io_no_space_not_quota_exceeded() {
        let rig = MigrationRig::new();
        let server = rig.server("main", "/projects/alpha", "default");
        install_v1(&rig, 1);
        server.activate(PLUGIN_ID);
        let storage = server.storage(PLUGIN_ID);
        rig.fill_disk();
        let error = storage.put(Bucket::Global, "key", b"value").err();
        assert!(error.is_some_and(|error| error.code == ErrorCode::Io && error.message == "no-space"));
        rig.free_disk();
        assert!(storage.put(Bucket::Global, "key", b"value").is_ok());
    }

    /// REQ: LIF-119
    #[test]
    fn plugin_quota_breach_still_reports_quota_exceeded() {
        let rig = MigrationRig::new();
        let server = rig.server("main", "/projects/alpha", "default");
        install_v1(&rig, 1);
        server.activate(PLUGIN_ID);
        rig.set_plugin_quota(PLUGIN_ID, Bucket::Global, 2);
        let error = server.storage(PLUGIN_ID).put(Bucket::Global, "key", &[0u8; 10]).err();
        assert!(error.is_some_and(|error| error.code == ErrorCode::QuotaExceeded));
    }

    /// REQ: LIF-120
    #[test]
    fn last_writer_wins_across_two_servers() {
        let rig = MigrationRig::new();
        let first = rig.server("a", "/projects/alpha", "s1");
        let second = rig.server("b", "/projects/beta", "s2");
        install_v1(&rig, 1);
        first.activate(PLUGIN_ID);
        second.activate(PLUGIN_ID);
        assert!(first.storage(PLUGIN_ID).put(Bucket::Global, "key", b"from-first").is_ok());
        assert!(second.storage(PLUGIN_ID).put(Bucket::Global, "key", b"from-second").is_ok());
        assert_eq!(
            first.storage(PLUGIN_ID).get(Bucket::Global, "key"),
            Some(b"from-second".to_vec())
        );
    }

    /// REQ: LIF-121
    #[test]
    fn stale_instance_put_remove_and_batch_all_conflict() {
        let rig = MigrationRig::new();
        let (stale, _fresh) = stale_and_fresh_servers(&rig);
        let storage = stale.storage(PLUGIN_ID);
        assert!(is_stale_data_version(storage.put(Bucket::Global, "key", b"x").err()));
        assert!(is_stale_data_version(storage.remove(Bucket::Global, "key").err()));
        let entries = vec![("key".to_string(), Some(b"x".to_vec()))];
        assert!(is_stale_data_version(storage.write_batch(Bucket::Global, &entries).err()));
    }

    /// REQ: COR-090
    #[test]
    fn unparseable_config_is_left_unchanged_and_host_still_boots() {
        let rig = MigrationRig::new();
        let broken = "[plugins\nmigration_backup_mib = = 3\n";
        rig.config(broken);
        let server = rig.server("main", "/projects/alpha", "default");
        assert!(server.is_running());
        assert_eq!(rig.config_text(), broken);
        assert!(rig.log_codes().iter().any(|code| code == "config.parse-error"));
        let warning = rig
            .log_line("config.parse-error")
            .map(|record| record.to_string())
            .unwrap_or_default();
        assert!(warning.contains("config.toml"), "warning: {warning}");
        server.write_setting("plugins", "migration_backup_mib", "0");
        assert_eq!(rig.config_text(), broken);
    }
}

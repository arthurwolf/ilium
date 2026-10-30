//! Data-migration rig: one in-process host with a fake clock, fake disk and fake plugin runtime
//! that drives `migrator.migrate`, the data-version stamps, backups and rollback (test plan lane
//! data-migration, LIF-100..121, WIT migrator.migrate). Test support only.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ilium_plugin_api::ids::parse_plugin_id;
use ilium_plugin_api::{PluginError, Tier};

use crate::audit::{AuditActor, AuditContext};
use crate::cli::local_install::factory;
use crate::cli::{self, CliEnv, CliOutput};
use crate::install::{Consent, ConsentDecision, ConsentRequest};
use crate::package::write_package;
use crate::testing::wat::{files_inventory, plugin_component};
use crate::registry_client::{FetchError, Fetcher};
use crate::storage::{PluginStorage, StorageContext, BUCKET_MAX_BYTES};

pub use ilium_plugin_api::generated::host::Bucket;


/// Reports a production hook the rig needs but the production host does not expose yet. The
/// test that reaches it fails at this line with the missing hook named, instead of passing on a
/// value the rig made up.
fn blocked(hook: &str) -> ! {
    panic!("rig blocked: the production host does not expose {hook} yet")
}

/// Shared-cache size the rig opens storage with (the host default for T3 plugins).
const SHARED_BUCKET_BYTES: usize = 64 * 1024 * 1024;

/// Distinguishes the scratch roots of rigs created by one process.
static RIG_SEQUENCE: AtomicU64 = AtomicU64::new(0);

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
    id: String,
    version: String,
    data_version: DataVersionField,
    migrator: Option<Vec<Step>>,
    migrate_deadline_ms: Option<u64>,
    restore_bytes: Option<Vec<u8>>,
}

/// How the manifest states `data_version`: left out, a number, or raw JSON (LIF-100).
#[derive(Debug, Clone)]
enum DataVersionField {
    Absent,
    Number(u32),
    Json(String),
}

impl PluginSpec {
    /// A plugin with the given id and version.
    pub fn new(id: &str, version: &str) -> PluginSpec {
        PluginSpec {
            id: id.to_owned(),
            version: version.to_owned(),
            data_version: DataVersionField::Absent,
            migrator: None,
            migrate_deadline_ms: None,
            restore_bytes: None,
        }
    }

    /// The manifest `data_version`.
    pub fn data_version(mut self, version: u32) -> PluginSpec {
        self.data_version = DataVersionField::Number(version);
        self
    }

    /// The manifest `data_version` as raw JSON.
    pub fn data_version_json(mut self, json: &str) -> PluginSpec {
        self.data_version = DataVersionField::Json(json.to_owned());
        self
    }

    /// The scripted `migrate` export.
    pub fn migrator(mut self, steps: Vec<Step>) -> PluginSpec {
        self.migrator = Some(steps);
        self
    }

    /// The manifest migrate deadline.
    pub fn migrate_deadline_ms(mut self, ms: u64) -> PluginSpec {
        self.migrate_deadline_ms = Some(ms);
        self
    }

    /// The restore blob `deactivate` returns.
    pub fn restore_bytes(mut self, bytes: Vec<u8>) -> PluginSpec {
        self.restore_bytes = Some(bytes);
        self
    }
}

/// The storage of one plugin as the plugin sees it (LIF-115, 117, 119, 120).
#[derive(Debug)]
pub struct StorageHandle {
    storage: Rc<RefCell<PluginStorage>>,
}

/// The wire name of a bucket, as the storage layer keys it.
fn bucket_name(bucket: Bucket) -> &'static str {
    match bucket {
        Bucket::Global => "global",
        Bucket::Project => "project",
        Bucket::Session => "session",
        Bucket::Shared => "shared",
    }
}

impl StorageHandle {
    /// `storage.put`.
    pub fn put(&self, bucket: Bucket, key: &str, value: &[u8]) -> Result<(), PluginError> {
        self.storage
            .borrow_mut()
            .put(bucket_name(bucket), key, value.to_vec())
    }

    /// `storage.get`.
    pub fn get(&self, bucket: Bucket, key: &str) -> Option<Vec<u8>> {
        self.storage
            .borrow()
            .get(bucket_name(bucket), key)
            .ok()
            .flatten()
    }

    /// `storage.remove`.
    pub fn remove(&self, bucket: Bucket, key: &str) -> Result<(), PluginError> {
        self.storage.borrow_mut().remove(bucket_name(bucket), key)
    }

    /// `storage.keys` with a prefix.
    pub fn keys(&self, bucket: Bucket, prefix: &str) -> Vec<String> {
        self.storage
            .borrow()
            .keys(bucket_name(bucket), prefix)
            .unwrap_or_default()
    }

    /// `storage.write-batch`; `None` removes a key.
    pub fn write_batch(
        &self,
        bucket: Bucket,
        entries: &[(String, Option<Vec<u8>>)],
    ) -> Result<(), PluginError> {
        self.storage
            .borrow_mut()
            .write_batch(bucket_name(bucket), entries)
    }

    /// Whether the storage offers compare-and-set.
    pub fn has_compare_and_set(&self) -> bool {
        // The storage interface offers put, get, remove, keys and write-batch only; no
        // compare-and-set operation exists in ilium-plugin-api or the production storage.
        false
    }

    /// Every key of a bucket with its value.
    pub fn dump(&self, bucket: Bucket) -> BTreeMap<String, Vec<u8>> {
        let storage = self.storage.borrow();
        let name = bucket_name(bucket);
        let entries: BTreeMap<String, Vec<u8>> = storage
            .keys(name, "")
            .unwrap_or_default()
            .into_iter()
            .filter_map(|key| {
                let value = storage.get(name, &key).ok().flatten()?;
                Some((key, value))
            })
            .collect();
        entries
    }
}

/// One in-process server of the migration rig (LIF-101..121).
#[derive(Debug)]
pub struct MigServer {
    storage_root: PathBuf,
    project: String,
    session: String,
    running: AtomicBool,
    /// The data version this server's storage checks against. Stays at 1 until `activate`
    /// records the running version.
    data_version: AtomicU32,
}

impl MigServer {
    /// Whether the server still runs (COR-090: it boots with defaults after a parse error).
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Writes one setting through the host's settings write path (COR-090).
    pub fn write_setting(&self, _table: &str, _key: &str, _value: &str) {
        blocked("the settings table of the in-process server")
    }

    /// Activates a plugin.
    pub fn activate(&self, _id: &str) -> ActivateReport {
        blocked("the in-process activation of a plugin version")
    }

    /// Applies the pending update of a plugin.
    pub fn upgrade(&self, _id: &str) -> ActivateReport {
        blocked("the in-process upgrade of a plugin version")
    }

    /// The storage of a plugin.
    pub fn storage(&self, id: &str) -> StorageHandle {
        let plugin = match parse_plugin_id(id) {
            Ok(plugin) => plugin,
            Err(_) => panic!("test plugin id is not valid: {id}"),
        };
        let context = StorageContext {
            project: Some(PathBuf::from(self.project.as_str())),
            session: Some(self.session.clone()),
            data_version: self.data_version.load(Ordering::SeqCst),
        };
        let storage = PluginStorage::open_with_context(
            &self.storage_root,
            plugin,
            context,
            Tier::T3,
            BUCKET_MAX_BYTES,
            SHARED_BUCKET_BYTES,
        )
        .expect("storage opens for a test plugin");
        StorageHandle {
            storage: Rc::new(RefCell::new(storage)),
        }
    }

    /// The data-version stamp of a bucket.
    pub fn stamp(&self, _id: &str, _bucket: Bucket) -> Option<u32> {
        blocked("the data-version stamp of a bucket")
    }

    /// Calls `migrate` directly.
    pub fn call_migrate(&self, _id: &str, _from: u32, _to: u32) -> Result<(), PluginError> {
        blocked("a direct call of the migrate export")
    }

    /// Kills the server.
    pub fn kill(&self) {
        self.running.store(false, Ordering::SeqCst);
    }
}

/// Consent that approves exactly the grants a package requests, as the default review does.
struct ApproveRequested;

impl Consent for ApproveRequested {
    fn decide(&self, request: &ConsentRequest) -> ConsentDecision {
        ConsentDecision::Approve(request.requested.clone())
    }
}

/// The package bytes of a spec without a scripted migrator: a manifest with the declared data
/// version and an empty server component.
fn package_bytes(spec: &PluginSpec) -> Vec<u8> {
    if spec.migrator.is_some() {
        blocked("a scripted migrate export in the plugin guest");
    }
    if spec.migrate_deadline_ms.is_some() || spec.restore_bytes.is_some() {
        blocked("the manifest migrate deadline and the restore blob of deactivate");
    }
    let mut manifest = serde_json::json!({
        "api": "ilium:plugin@0.1.0",
        "id": spec.id,
        "name": spec.id,
        "version": spec.version,
        "publisher": "Example Developer",
        "replaces": [],
        "server": {"entry": "server.wasm", "world": "ilium:server/base@0.1.0", "exports": []},
    });
    match &spec.data_version {
        DataVersionField::Absent => {}
        DataVersionField::Number(version) => manifest["data_version"] = serde_json::json!(version),
        DataVersionField::Json(text) => {
            manifest["data_version"] = serde_json::from_str(text)
                .unwrap_or_else(|error| panic!("data_version JSON {text}: {error}"));
        }
    }
    // A data version above 1 requires the `migrator` export in the manifest and the component
    // (PKG-045); a plugin with nothing to migrate exports a migrator that returns ok.
    let needs_migrator = match &spec.data_version {
        DataVersionField::Absent => false,
        DataVersionField::Number(version) => *version > 1,
        DataVersionField::Json(text) => text.trim().parse::<u64>().is_ok_and(|version| version > 1),
    };
    let optional: &[&str] = if needs_migrator { &["migrator"] } else { &[] };
    manifest["server"]["exports"] = serde_json::json!(optional);
    let mut files = vec![("server.wasm".to_owned(), plugin_component(optional))];
    manifest["files"] = files_inventory(&files);
    let manifest_text = serde_json::to_string_pretty(&manifest).expect("manifest serializes");
    files.push(("manifest.json".to_owned(), manifest_text.into_bytes()));
    write_package(&files).unwrap_or_else(|diagnostic| panic!("migration package failed: {diagnostic:?}"))
}

fn list_files(directory: &std::path::Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            list_files(&path, found);
        } else {
            found.push(path);
        }
    }
}

/// The migration rig (LIF-100..121).
#[derive(Debug)]
pub struct MigrationRig {
    root: PathBuf,
    clock: Cell<SystemTime>,
}

/// Fetches nothing: the migration rig never reaches a registry (the CLI must work offline).
struct OfflineFetcher;

impl Fetcher for OfflineFetcher {
    fn get(&self, _url: &str) -> Result<Vec<u8>, FetchError> {
        Err(FetchError::Unreachable)
    }
}

/// Parses `YYYY-MM-DDTHH:MM:SSZ` into whole seconds since the Unix epoch.
fn utc_seconds(text: &str) -> Option<u64> {
    let bytes = text.as_bytes();
    let separators_match = bytes.len() == 20
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[10] == b'T'
        && bytes[13] == b':'
        && bytes[16] == b':'
        && bytes[19] == b'Z';
    if !separators_match {
        return None;
    }
    let year: i64 = text.get(0..4)?.parse().ok()?;
    let month: i64 = text.get(5..7)?.parse().ok()?;
    let day: i64 = text.get(8..10)?.parse().ok()?;
    let hour: u64 = text.get(11..13)?.parse().ok()?;
    let minute: u64 = text.get(14..16)?.parse().ok()?;
    let second: u64 = text.get(17..19)?.parse().ok()?;
    let days = days_from_civil(year, month, day);
    let day_seconds = u64::try_from(days).ok()?.checked_mul(86_400)?;
    day_seconds.checked_add(hour * 3_600 + minute * 60 + second)
}

/// Days since 1970-01-01 of a proleptic Gregorian date (Hinnant's civil-from-days inverse).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let shifted_month = if month > 2 { month - 3 } else { month + 9 };
    let day_of_year = (153 * shifted_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

impl MigrationRig {
    /// A fresh rig.
    pub fn new() -> MigrationRig {
        let sequence = RIG_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "ilium-migration-{}-{sequence}",
            std::process::id()
        ));
        for directory in [
            root.join("storage"),
            root.join("config"),
            root.join("state"),
            root.join("data"),
        ] {
            if let Err(failure) = fs::create_dir_all(&directory) {
                panic!("creating {}: {failure}", directory.display());
            }
        }
        let rig = MigrationRig {
            root,
            clock: Cell::new(UNIX_EPOCH),
        };
        rig.set_clock_utc("2026-10-23T00:00:00Z");
        rig
    }

    fn storage_root(&self) -> PathBuf {
        self.root.join("storage")
    }

    fn config_path(&self) -> PathBuf {
        self.root.join("config").join("config.toml")
    }

    /// The environment every CLI command of the rig runs in.
    fn cli_env(&self) -> CliEnv {
        CliEnv::new(
            self.root.join("data"),
            self.root.join("state"),
            self.root.join("config"),
            self.root.clone(),
            self.clock.get(),
            true,
            Arc::new(OfflineFetcher),
        )
    }

    /// Starts a server for a project session.
    pub fn server(&self, _name: &str, project: &str, session: &str) -> MigServer {
        MigServer {
            storage_root: self.storage_root(),
            project: project.to_owned(),
            session: session.to_owned(),
            running: AtomicBool::new(true),
            data_version: AtomicU32::new(1),
        }
    }

    /// Restarts a server.
    pub fn restart_server(&self, server: &MigServer) -> MigServer {
        server.kill();
        MigServer {
            storage_root: server.storage_root.clone(),
            project: server.project.clone(),
            session: server.session.clone(),
            running: AtomicBool::new(true),
            data_version: AtomicU32::new(server.data_version.load(Ordering::SeqCst)),
        }
    }

    /// The plugin root of the production layout: `plugins/` under the data directory.
    fn plugin_root(&self) -> PathBuf {
        self.root.join("data").join("plugins")
    }

    /// Writes a package file and runs `ilium plugin install` on it with the given flags.
    fn install_package_file(&self, file_name: &str, bytes: &[u8], flags: &[&str]) -> CliOutput {
        let folder = self.root.join("packages");
        fs::create_dir_all(&folder)
            .unwrap_or_else(|failure| panic!("creating {}: {failure}", folder.display()));
        let path = folder.join(file_name);
        fs::write(&path, bytes)
            .unwrap_or_else(|failure| panic!("writing {}: {failure}", path.display()));
        let consent: Arc<dyn Consent + Send + Sync> = Arc::new(ApproveRequested);
        let audit_context = AuditContext {
            session: "migration-rig".to_owned(),
            actor: AuditActor {
                kind: "cli".to_owned(),
                pane: None,
                plugin: None,
            },
        };
        let mut env = self.cli_env();
        env.cwd = self.root.clone();
        let backend = match factory(&env, consent, audit_context) {
            Ok(backend) => backend,
            Err(diagnostic) => {
                return CliOutput {
                    exit_code: 1,
                    stdout_jsonl: Vec::new(),
                    stderr: diagnostic.message.clone(),
                    stdout_raw: String::new(),
                    stdout_text: String::new(),
                };
            }
        };
        let env = env.with_installer(backend);
        let mut owned = vec!["install".to_owned(), path.display().to_string()];
        owned.extend(flags.iter().map(|flag| (*flag).to_owned()));
        cli::run(&owned, &env)
    }

    /// Installs a plugin.
    pub fn install(&self, spec: &PluginSpec) -> CliOutput {
        let bytes = package_bytes(spec);
        self.install_package_file(&format!("{}-{}.iliumpkg", spec.id, spec.version), &bytes, &[])
    }

    /// Updates a plugin.
    pub fn update(&self, _spec: &PluginSpec, _flags: &[&str]) -> CliOutput {
        blocked("the ilium plugin update command")
    }

    /// Rolls a plugin back.
    pub fn rollback(&self, _id: &str, _flags: &[&str]) -> CliOutput {
        blocked("the ilium plugin rollback command")
    }

    /// Runs a CLI command.
    pub fn cli(&self, args: &[&str]) -> CliOutput {
        let owned: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
        cli::run(&owned, &self.cli_env())
    }

    /// Installs a package file with flags.
    pub fn cli_install_file(&self, spec: &PluginSpec, flags: &[&str]) -> CliOutput {
        let bytes = package_bytes(spec);
        self.install_package_file(&format!("{}-{}.iliumpkg", spec.id, spec.version), &bytes, flags)
    }

    /// `installed.json` parsed.
    pub fn registry_json(&self) -> serde_json::Value {
        // A registry that was never written holds no plugins; the rig reports it as null.
        fs::read_to_string(self.plugin_root().join("installed.json"))
            .ok()
            .map(|text| {
                serde_json::from_str(&text)
                    .unwrap_or_else(|error| panic!("installed.json is not JSON: {error}"))
            })
            .unwrap_or(serde_json::Value::Null)
    }

    /// The data-version stamp of a bucket.
    pub fn stamp(&self, _id: &str, _bucket: Bucket) -> Option<u32> {
        blocked("the data-version stamp of a bucket")
    }

    /// The data-version stamp of the data directory.
    pub fn data_dir_stamp(&self, _id: &str) -> Option<u32> {
        blocked("the data-version stamp of the data directory")
    }

    /// Where the stamp is stored.
    pub fn stamp_location(&self, _id: &str) -> PathBuf {
        blocked("the stamp file layout")
    }

    /// The data directory of a plugin.
    pub fn plugin_data_dir(&self, id: &str) -> PathBuf {
        self.plugin_root().join("data").join(id)
    }

    /// The files of the data directory, by relative path.
    pub fn data_dir_files(&self, id: &str) -> BTreeMap<String, Vec<u8>> {
        let directory = self.plugin_data_dir(id);
        let mut found = Vec::new();
        list_files(&directory, &mut found);
        found
            .into_iter()
            .map(|path| {
                let relative = path
                    .strip_prefix(&directory)
                    .unwrap_or(&path)
                    .display()
                    .to_string();
                let bytes = fs::read(&path)
                    .unwrap_or_else(|failure| panic!("reading {}: {failure}", path.display()));
                (relative, bytes)
            })
            .collect()
    }

    /// Writes a data file.
    pub fn write_data_file(&self, id: &str, name: &str, bytes: &[u8]) {
        let path = self.plugin_data_dir(id).join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .unwrap_or_else(|failure| panic!("creating {}: {failure}", parent.display()));
        }
        fs::write(&path, bytes)
            .unwrap_or_else(|failure| panic!("writing {}: {failure}", path.display()));
    }

    /// The backup names of a plugin.
    pub fn backups(&self, id: &str) -> Vec<String> {
        let Ok(entries) = fs::read_dir(self.plugin_root().join("backups").join(id)) else {
            return Vec::new();
        };
        let mut names: Vec<String> = entries
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    /// A bucket of a backup.
    pub fn backup_dump(
        &self,
        _id: &str,
        _backup: &str,
        _bucket: Bucket,
    ) -> BTreeMap<String, Vec<u8>> {
        blocked("the backup bucket readback")
    }

    /// The data files of a backup.
    pub fn backup_data_files(&self, _id: &str, _backup: &str) -> BTreeMap<String, Vec<u8>> {
        blocked("the backup data file layout")
    }

    /// The `migrate` calls of a plugin.
    pub fn migrate_calls(&self, _id: &str) -> Vec<MigrateCall> {
        blocked("the record of migrate calls of the fake runtime")
    }

    /// The log codes so far.
    pub fn log_codes(&self) -> Vec<String> {
        blocked("the migration log code record")
    }

    /// The last log line with a code, as its JSON record (`level`, `code`, `message`).
    pub fn log_line(&self, _code: &str) -> Option<serde_json::Value> {
        blocked("the migration log line record")
    }

    /// Failures in the window of a plugin.
    pub fn failures_in_window(&self, _id: &str) -> u32 {
        blocked("the failure window counter")
    }

    /// `plugin info` as JSON.
    pub fn info_json(&self, _id: &str) -> serde_json::Value {
        blocked("the plugin info report")
    }

    /// The migrate lock path.
    pub fn lock_path(&self, _id: &str) -> PathBuf {
        blocked("the migrate lock file layout")
    }

    /// The current bytes of the config file as text (COR-090).
    pub fn config_text(&self) -> String {
        fs::read_to_string(self.config_path()).unwrap_or_default()
    }

    /// Writes the config file.
    pub fn config(&self, toml: &str) {
        if let Err(failure) = fs::write(self.config_path(), toml) {
            panic!("writing the test config file: {failure}");
        }
    }

    /// Sets the wall clock.
    pub fn set_clock_utc(&self, rfc3339: &str) {
        let Some(seconds) = utc_seconds(rfc3339) else {
            panic!("clock must be YYYY-MM-DDTHH:MM:SSZ, got {rfc3339}");
        };
        self.clock.set(UNIX_EPOCH + Duration::from_secs(seconds));
    }

    /// Moves the clock.
    pub fn advance_clock(&self, ms: u64) {
        self.clock.set(self.clock.get() + Duration::from_millis(ms));
    }

    /// Sets the free disk space.
    pub fn set_free_bytes(&self, _bytes: u64) {
        blocked("a fake disk with a settable free space")
    }

    /// Fills the disk.
    pub fn fill_disk(&self) {
        blocked("a fake disk with a settable free space")
    }

    /// Frees the disk.
    pub fn free_disk(&self) {
        blocked("a fake disk with a settable free space")
    }

    /// Sets the quota of a bucket.
    pub fn set_plugin_quota(&self, _id: &str, _bucket: Bucket, _bytes: u64) {
        blocked("a per-plugin bucket quota setter")
    }

    /// Simulates a power loss.
    pub fn power_loss(&self) {
        blocked("a fake disk that loses unflushed writes")
    }

    /// Reads a value after a power loss.
    pub fn read_after_power_loss(&self, _id: &str, _bucket: Bucket, _key: &str) -> Option<Vec<u8>> {
        blocked("a fake disk that loses unflushed writes")
    }

    /// Bytes not yet flushed.
    pub fn unflushed_bytes(&self, _id: &str) -> u64 {
        blocked("a fake disk that loses unflushed writes")
    }

    /// Crashes during the next write.
    pub fn crash_during_next_write(&self) {
        blocked("a crash injection hook in the storage write")
    }
}

impl Default for MigrationRig {
    fn default() -> MigrationRig {
        MigrationRig::new()
    }
}

impl Drop for MigrationRig {
    fn drop(&mut self) {
        // Scratch root of this rig only; servers hold copies of paths, never the rig.
        let _ = fs::remove_dir_all(&self.root);
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
        assert_eq!(
            backups.len(),
            1,
            "expected exactly one backup, found {backups:?}"
        );
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
        let output =
            rig.install(&PluginSpec::new(PLUGIN_ID, OLD_VERSION).data_version_json("\"2\""));
        assert_refused_with(&output, "manifest.data-version");
    }

    /// REQ: LIF-100
    #[test]
    fn manifest_data_version_upper_bound_is_accepted() {
        let rig = MigrationRig::new();
        let output = rig.install(&PluginSpec::new(PLUGIN_ID, OLD_VERSION).data_version(u32::MAX));
        assert_succeeded(&output);
    }

    /// REQ: LIF-100
    #[test]
    fn absent_data_version_stamps_bucket_as_one() {
        let rig = MigrationRig::new();
        assert_succeeded(&rig.install(&PluginSpec::new(PLUGIN_ID, OLD_VERSION)));
        let server = rig.server("main", "/projects/alpha", "default");
        server.activate(PLUGIN_ID);
        assert!(server
            .storage(PLUGIN_ID)
            .put(Bucket::Global, "key", b"value")
            .is_ok());
        assert_eq!(rig.stamp(PLUGIN_ID, Bucket::Global), Some(1));
    }

    /// REQ: LIF-101
    #[test]
    fn stamp_is_readable_after_successful_migration() {
        let rig = MigrationRig::new();
        let server = rig.server("main", "/projects/alpha", "default");
        install_v1(&rig, 1);
        server.activate(PLUGIN_ID);
        assert!(server
            .storage(PLUGIN_ID)
            .put(Bucket::Global, "key", b"value")
            .is_ok());
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
        assert!(server
            .storage(PLUGIN_ID)
            .put(Bucket::Global, "key", b"value")
            .is_ok());
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
        assert!(first
            .storage(PLUGIN_ID)
            .put(Bucket::Session, "key", b"a")
            .is_ok());
        assert!(second
            .storage(PLUGIN_ID)
            .put(Bucket::Session, "key", b"b")
            .is_ok());
        assert_succeeded(&update_to_v2(&rig, 3, vec![]));
        first.upgrade(PLUGIN_ID);
        second.upgrade(PLUGIN_ID);
        first.activate(PLUGIN_ID);
        assert_eq!(count_calls(&rig, &MigrateScope::Global, 1, 3), 1);
        assert_eq!(
            count_calls(&rig, &session_scope("/projects/alpha", "s1"), 1, 3),
            1
        );
        assert_eq!(
            count_calls(&rig, &session_scope("/projects/beta", "s2"), 1, 3),
            1
        );
    }

    /// REQ: LIF-104
    #[test]
    fn migrate_skipped_when_data_version_is_unchanged() {
        let rig = MigrationRig::new();
        let server = rig.server("main", "/projects/alpha", "default");
        install_v1(&rig, 2);
        server.activate(PLUGIN_ID);
        let steps = vec![Step::Put(
            Bucket::Global,
            "key".to_string(),
            b"new".to_vec(),
        )];
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
        assert!(rig
            .migrate_calls(PLUGIN_ID)
            .iter()
            .any(|call| !call.returned_ok));
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
        assert!(server
            .storage(PLUGIN_ID)
            .put(Bucket::Global, "key", b"old")
            .is_ok());
        let steps = vec![Step::Put(
            Bucket::Global,
            "key".to_string(),
            b"new".to_vec(),
        )];
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
        assert!(server
            .storage(PLUGIN_ID)
            .put(Bucket::Global, "key", b"old")
            .is_ok());
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
        assert!(server
            .storage(PLUGIN_ID)
            .put(Bucket::Global, "key", b"old")
            .is_ok());
        rig.set_free_bytes(0);
        let output = update_to_v2(&rig, 3, vec![]);
        assert_ne!(output.exit_code, 0);
        assert!(rig
            .log_codes()
            .iter()
            .any(|code| code == "install.no-space"));
        assert_eq!(rig.stamp(PLUGIN_ID, Bucket::Global), Some(1));
    }

    /// REQ: LIF-108
    #[test]
    fn failed_migration_restores_backup_keeps_old_version_and_stamp() {
        let rig = MigrationRig::new();
        let server = rig.server("main", "/projects/alpha", "default");
        install_v1(&rig, 1);
        server.activate(PLUGIN_ID);
        assert!(server
            .storage(PLUGIN_ID)
            .put(Bucket::Global, "a", b"old")
            .is_ok());
        assert!(server
            .storage(PLUGIN_ID)
            .put(Bucket::Global, "b", b"old")
            .is_ok());
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
        assert!(server
            .storage(PLUGIN_ID)
            .put(Bucket::Global, "key", b"old")
            .is_ok());
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
        assert!(server
            .storage(PLUGIN_ID)
            .put(Bucket::Global, "key", b"old")
            .is_ok());
        assert_succeeded(&update_to_v2(&rig, 3, vec![]));
        server.upgrade(PLUGIN_ID);
        assert_succeeded(&rig.rollback(PLUGIN_ID, &["--restore-backup"]));
        assert_eq!(rig.stamp(PLUGIN_ID, Bucket::Global), Some(1));
        assert_eq!(
            server.storage(PLUGIN_ID).get(Bucket::Global, "key"),
            Some(b"old".to_vec())
        );
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
        assert!(is_stale_data_version(
            storage.put(Bucket::Global, "key", b"x").err()
        ));
        assert!(is_stale_data_version(
            storage.put(Bucket::Project, "key", b"x").err()
        ));
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
        assert!(alpha
            .storage(PLUGIN_ID)
            .put(Bucket::Session, "key", b"a")
            .is_ok());
        assert!(beta
            .storage(PLUGIN_ID)
            .put(Bucket::Session, "key", b"b")
            .is_ok());
        assert_succeeded(&update_to_v2(&rig, 3, vec![]));
        alpha.upgrade(PLUGIN_ID);
        beta.upgrade(PLUGIN_ID);
        assert_eq!(
            count_calls(&rig, &session_scope("/projects/alpha", "s1"), 1, 3),
            1
        );
        assert_eq!(
            count_calls(&rig, &session_scope("/projects/beta", "s2"), 1, 3),
            1
        );
    }

    /// REQ: LIF-115
    #[test]
    fn put_is_flushed_before_return_and_survives_power_loss() {
        let rig = MigrationRig::new();
        let server = rig.server("main", "/projects/alpha", "default");
        install_v1(&rig, 1);
        server.activate(PLUGIN_ID);
        assert!(server
            .storage(PLUGIN_ID)
            .put(Bucket::Global, "key", b"durable")
            .is_ok());
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
        assert_eq!(
            rig.read_after_power_loss(PLUGIN_ID, Bucket::Global, "key"),
            None
        );
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
        assert!(
            error.is_some_and(|error| error.code == ErrorCode::Io && error.message == "no-space")
        );
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
        let error = server
            .storage(PLUGIN_ID)
            .put(Bucket::Global, "key", &[0u8; 10])
            .err();
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
        assert!(first
            .storage(PLUGIN_ID)
            .put(Bucket::Global, "key", b"from-first")
            .is_ok());
        assert!(second
            .storage(PLUGIN_ID)
            .put(Bucket::Global, "key", b"from-second")
            .is_ok());
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
        assert!(is_stale_data_version(
            storage.put(Bucket::Global, "key", b"x").err()
        ));
        assert!(is_stale_data_version(
            storage.remove(Bucket::Global, "key").err()
        ));
        let entries = vec![("key".to_string(), Some(b"x".to_vec()))];
        assert!(is_stale_data_version(
            storage.write_batch(Bucket::Global, &entries).err()
        ));
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
        assert!(rig
            .log_codes()
            .iter()
            .any(|code| code == "config.parse-error"));
        let warning = rig
            .log_line("config.parse-error")
            .map(|record| record.to_string())
            .unwrap_or_default();
        assert!(warning.contains("config.toml"), "warning: {warning}");
        server.write_setting("plugins", "migration_backup_mib", "0");
        assert_eq!(rig.config_text(), broken);
    }
}

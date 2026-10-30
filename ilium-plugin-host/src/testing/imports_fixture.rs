//! Imports fixture: one plugin instance with a manifest, grants, a project directory, a manual
//! clock and limits. Clock, log, storage and the basic primitive use real adapters;
//! unrelated imports remain explicit pending production implementations. Test support only.

use super::fake_kernel::FakeKernel;
use crate::imports::{
    basic::BasicImport,
    log::{LogClock, LogDiagnostics, LogFailure, LogImport, LogTime, PluginLogger},
    storage::{StorageAuthorization, StorageImport, StorageImportContext, StorageInstanceState},
};
use crate::imports::{
    clock::ClockImport, codec::CodecImport, fs::FsImport, http::HttpImport, jobs::JobsImport,
    journal::JournalImport, panes::PanesImport, process::ProcessImport, settings::SettingsImport,
    timers::TimersImport, tree::TreeImport,
};
use crate::storage::StorageContext;
use crate::testing::harness::{HarnessHandle, HostHarness, NullGuest};
use crate::telemetry::{LogConfig, LogLevel, PluginLogInstanceBudget, Refusal};
use ilium_plugin_api::{grants::Grant, ids::parse_plugin_id, ProcessKind};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use std::time::UNIX_EPOCH;

use ilium_plugin_api::generated::host::{
    Bucket, ChildHandle, FieldError, HostClock, HostCodec, HostFs, HostHttp, HostJobs, HostJournal,
    HostLog, HostPanes, HostProcess, HostSettings, HostStorage, HostTimers, HostTree,
};
use ilium_plugin_api::generated::types::{
    ErrorCode, Event, FsPath, MeterSegment, NodeId, Outcome, PluginError, RequestId, RichSpan,
    RowState, StreamId, WatchId,
};
use ilium_plugin_api::side::Side;
use ilium_plugin_api::Tier;

use crate::telemetry::LogLine;

/// Pump rounds that `pump` and `advance` ask the production harness to run.
const PUMP_ROUNDS: u32 = 64;

/// Longest settings validate answer that still saves, in virtual time (SET-022).
const SETTINGS_VALIDATE_BUDGET: Duration = Duration::from_millis(100);

/// Largest settings patch a save accepts, in bytes (SET-024).
const SETTINGS_PATCH_LIMIT_BYTES: usize = 256 * 1024;

/// Longest text value a settings patch may carry, in characters (SET-024).
const SETTINGS_TEXT_LIMIT_CHARS: usize = 4096;

/// Refuses a settings patch that is too large or holds a text value over 4096 characters (SET-024).
fn check_patch_limits(patch_json: &str) -> Result<(), String> {
    if patch_json.len() > SETTINGS_PATCH_LIMIT_BYTES {
        return Err(format!(
            "settings patch of {} bytes exceeds 256 KiB",
            patch_json.len()
        ));
    }
    let patch: serde_json::Value =
        serde_json::from_str(patch_json).map_err(|error| error.to_string())?;
    let longest_text = longest_text_chars(&patch);
    if longest_text > SETTINGS_TEXT_LIMIT_CHARS {
        return Err(format!(
            "settings text of {longest_text} characters exceeds 4096"
        ));
    }
    Ok(())
}

/// The character count of the longest string anywhere inside a JSON value.
fn longest_text_chars(value: &serde_json::Value) -> usize {
    match value {
        serde_json::Value::String(text) => text.chars().count(),
        serde_json::Value::Array(items) => items.iter().map(longest_text_chars).max().unwrap_or(0),
        serde_json::Value::Object(map) => map.values().map(longest_text_chars).max().unwrap_or(0),
        _ => 0,
    }
}

/// The stored settings object with the patch's keys written over it; the validate hook sees this view.
fn merge_settings_patch(stored_json: &str, patch_json: &str) -> Result<String, String> {
    let mut merged: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(stored_json).unwrap_or_default();
    let patch: serde_json::Map<String, serde_json::Value> = serde_json::from_str(patch_json)
        .map_err(|error| format!("settings patch is not a JSON object: {error}"))?;
    merged.extend(patch);
    serde_json::to_string(&merged).map_err(|error| error.to_string())
}

/// How the user changed a settings row (SET-020..024, 031, 050, 051).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserInput {
    /// A toggle.
    Toggle,
    /// A choice.
    Choice,
    /// A dialog confirmation.
    DialogConfirm,
    /// A number step.
    NumberStep,
    /// Typed text.
    Typed,
    /// A text area.
    TextArea,
}

/// What a user save did (SET-020..024).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UserSave {
    /// Saved.
    Saved,
    /// Still pending.
    Pending,
    /// Refused with a message.
    Refused(String),
}

/// The settings-validate export of the fixture plugin: patch JSON, merged JSON.
pub type SettingsValidate = Box<dyn FnMut(&str, &str) -> Result<(), Vec<FieldError>>>;

/// The job-runner export of the fixture plugin: kind, input.
pub type JobRunner = Box<dyn FnMut(&str, &[u8]) -> Result<Vec<u8>, PluginError>>;

#[derive(Clone)]
struct FixtureClock(Arc<Mutex<ClockState>>);
struct ClockState {
    wall: u64,
    monotonic: u64,
}
impl HostClock for FixtureClock {
    fn now_unix_millis(&mut self) -> u64 {
        self.0.lock().unwrap().wall
    }
    fn monotonic_millis(&mut self) -> u64 {
        self.0.lock().unwrap().monotonic
    }
}
impl LogClock for FixtureClock {
    fn now(&self) -> Result<LogTime, Refusal> {
        let clock = self.0.lock().map_err(|_| Refusal {
            code: "log.clock-poisoned".into(),
            message: "fixture clock poisoned".into(),
        })?;
        // Gregorian civil date from epoch days; UTC is the fixture's explicit
        // local timezone. Wall corrections never change its duration clock.
        let seconds = clock.wall / 1000;
        let days = (seconds / 86400) as i64 + 719468;
        let era = days / 146097;
        let day_of_era = days - era * 146097;
        let year_of_era =
            (day_of_era - day_of_era / 1460 + day_of_era / 36524 - day_of_era / 146096) / 365;
        let mut year = year_of_era + era * 400;
        let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
        let month_phase = (5 * day_of_year + 2) / 153;
        let day = day_of_year - (153 * month_phase + 2) / 5 + 1;
        let month = month_phase + if month_phase < 10 { 3 } else { -9 };
        year += i64::from(month <= 2);
        if year > 9999 {
            return Err(Refusal {
                code: "log.clock-range".into(),
                message: "wall clock exceeds log timestamp range".into(),
            });
        }
        Ok(LogTime {
            local_timestamp: format!(
                "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02}",
                seconds / 3600 % 24,
                seconds / 60 % 60,
                seconds % 60
            ),
            monotonic_millis: clock.monotonic,
        })
    }
}
#[derive(Default)]
struct FixtureDiagnostics(Mutex<Option<ErrorCode>>);
impl LogDiagnostics for FixtureDiagnostics {
    fn failed(&self, _failure: LogFailure) {
        *self.0.lock().unwrap() = Some(ErrorCode::Io);
    }
}
struct FixtureStorage {
    import: StorageImport,
    clock: FixtureClock,
    errors: Arc<FixtureDiagnostics>,
}
impl HostStorage for FixtureStorage {
    fn get(&mut self, bucket: Bucket, key: String) -> Option<Vec<u8>> {
        self.import.get(bucket, key)
    }
    fn put(&mut self, bucket: Bucket, key: String, value: Vec<u8>) -> Result<(), PluginError> {
        self.import.put(bucket, key, value)
    }
    fn remove(&mut self, bucket: Bucket, key: String) -> Result<(), PluginError> {
        self.import.remove(bucket, key)
    }
    fn keys(&mut self, bucket: Bucket, prefix: String) -> Vec<String> {
        self.import.keys(bucket, prefix)
    }
    fn write_batch(
        &mut self,
        bucket: Bucket,
        entries: Vec<ilium_plugin_api::generated::host::Entry>,
    ) -> Result<(), PluginError> {
        self.import.write_batch(bucket, entries)
    }
    fn lease(&mut self, key: String, ttl_millis: u32) -> bool {
        let wall = self.clock.0.lock().unwrap().wall;
        match self
            .import
            .lease_at(&key, ttl_millis, UNIX_EPOCH + Duration::from_millis(wall))
        {
            Ok(acquired) => acquired,
            Err(error) => {
                *self.errors.0.lock().unwrap() = Some(error.code);
                false
            }
        }
    }
}

/// Builds an [`ImportsFixture`].
pub struct ImportsFixtureBuilder {
    tier: Tier,
    side: Side,
    grants: Vec<String>,
    manifest: String,
    project: Option<PathBuf>,
    limits: BTreeMap<String, u64>,
    additional: Vec<String>,
    validate: Option<SettingsValidate>,
    validate_latency: Duration,
    runner: Option<JobRunner>,
    websocket: bool,
}

impl std::fmt::Debug for ImportsFixtureBuilder {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ImportsFixtureBuilder")
    }
}

impl ImportsFixtureBuilder {
    /// Fallible setup for tests that deliberately exercise invalid configuration.
    /// This validates fixture identities and limits; it does not claim package
    /// admission or instantiate a WebAssembly guest.
    pub fn try_build(self) -> Result<ImportsFixture, String> {
        let manifest: serde_json::Value =
            serde_json::from_str(&self.manifest).map_err(|error| error.to_string())?;
        if !manifest.is_object() {
            return Err("fixture manifest must be an object".into());
        }
        let plugin = match manifest.get("id") {
            Some(value) => value.as_str().ok_or("plugin id must be a string")?,
            None => "test.imports",
        }
        .to_owned();
        let plugin_id = parse_plugin_id(&plugin).map_err(|_| "invalid plugin identity")?;
        let grants = self
            .grants
            .iter()
            .map(|grant| Grant::parse(grant).map_err(|error| format!("invalid grant: {error:?}")))
            .collect::<Result<Vec<_>, _>>()?;
        let mut additional = Vec::new();
        let mut identities = std::collections::BTreeSet::from([plugin.clone()]);
        for text in self.additional {
            let value: serde_json::Value =
                serde_json::from_str(&text).map_err(|error| error.to_string())?;
            let id = value["id"]
                .as_str()
                .ok_or("additional fixture plugin requires id")?;
            parse_plugin_id(id).map_err(|_| "invalid additional plugin identity")?;
            if !identities.insert(id.to_owned()) {
                return Err("duplicate fixture plugin identity".into());
            }
            additional.push(value);
        }
        let requested = ilium_plugin_api::limits::Limits(self.limits.clone());
        let resolved = ilium_plugin_api::limits::clamp(&requested, self.tier)
            .map_err(|error| error.message)?;
        let bytes_for = |name: &str| -> Result<usize, String> {
            let key = name
                .parse::<ilium_plugin_api::limits::LimitKey>()
                .map_err(|_| "unknown limit")?;
            usize::try_from(resolved[&key])
                .ok()
                .and_then(|value| value.checked_mul(1024 * 1024))
                .ok_or_else(|| "limit exceeds address space".into())
        };
        let root = tempfile::Builder::new()
            .prefix("ilium-imports-")
            .tempdir()
            .map_err(|error| error.to_string())?;
        let project = match self.project {
            Some(project) => std::fs::canonicalize(project).map_err(|error| error.to_string())?,
            None => {
                let project = root.path().join("project");
                std::fs::create_dir(&project).map_err(|error| error.to_string())?;
                project
            }
        };
        if !project.is_dir() {
            return Err("fixture project root must be a directory".into());
        }
        let data_version = match manifest.get("data_version") {
            None => 1,
            Some(value) => value
                .as_u64()
                .and_then(|value| u32::try_from(value).ok())
                .filter(|value| *value > 0)
                .ok_or("invalid fixture data version")?,
        };
        let scope = StorageContext {
            project: Some(project),
            session: Some("imports-fixture".into()),
            data_version,
        };
        let errors = Arc::new(FixtureDiagnostics::default());
        let storage_authorization = Arc::new(Mutex::new(StorageAuthorization::new(
            plugin_id.clone(),
            self.side,
            StorageInstanceState::Active,
        )));
        let storage_errors = errors.clone();
        let storage = StorageImport::open(
            &root.path().join("storage"),
            StorageImportContext {
                plugin: plugin_id,
                side: self.side,
                scope,
                tier: self.tier,
                bucket_limit_bytes: bytes_for("storage_bucket_mib")?,
                shared_limit_bytes: bytes_for("shared_cache_mib")?,
            },
            storage_authorization.clone(),
            move |_, error| {
                *storage_errors.0.lock().unwrap() = Some(error.code);
            },
        )
        .map_err(|error| error.message)?;
        let clock = FixtureClock(Arc::new(Mutex::new(ClockState {
            wall: 1_893_456_000_000,
            monotonic: 0,
        })));
        let logger = Arc::new(Mutex::new(
            PluginLogger::open(
                &root.path().join("logs"),
                &plugin,
                self.tier,
                LogConfig {
                    threshold: LogLevel::Trace,
                    ..LogConfig::for_tier(self.tier)
                },
                1024,
                &[],
            )
            .map_err(|error| error.code)?,
        ));
        let budget = Arc::new(Mutex::new(
            PluginLogInstanceBudget::new(1, self.side).map_err(|error| error.code)?,
        ));
        let log_import = LogImport::new(
            logger.clone(),
            budget.clone(),
            Arc::new(clock.clone()),
            errors.clone(),
        )
        .map_err(|error| error.code)?;
        let kernel = Arc::new(FakeKernel::new(if self.side == Side::Server {
            ProcessKind::Server
        } else {
            ProcessKind::Client
        }));
        kernel.state().current_plugin = plugin.clone();
        if let Some(kinds) = manifest["contributes"]["pane_kinds"].as_array() {
            for kind in kinds {
                if kind["primitive"] == "basic" {
                    let id = kind["id"].as_str().ok_or("basic pane kind requires id")?;
                    if id
                        .strip_prefix(&format!("{plugin}."))
                        .is_none_or(|local| local.is_empty())
                    {
                        return Err("fixture pane kind must belong to its plugin".into());
                    }
                    kernel.register_pane_kind(
                        &plugin,
                        id,
                        ilium_plugin_api::generated::types::tree::PrimitiveKind::Basic,
                    );
                }
            }
        }
        let grant_refs: Vec<&str> = self.grants.iter().map(String::as_str).collect();
        let harness = HostHarness::new()
            .tier(self.tier)
            .side(self.side)
            .grants(&grant_refs)
            .kernel(kernel.clone())
            .with_native_guest(Box::new(NullGuest))
            .build()
            .map_err(|error| format!("imports fixture harness did not build: {error}"))?;
        Ok(ImportsFixture {
            log_import,
            logger,
            budget,
            clock: clock.clone(),
            storage_import: FixtureStorage {
                import: storage,
                clock,
                errors: errors.clone(),
            },
            storage_authorization,
            grants: Arc::new(Mutex::new(grants)),
            kernel,
            plugin,
            manifest,
            errors,
            stopped: false,
            next_node: 1,
            _additional: additional,
            validate: self.validate,
            validate_latency: self.validate_latency,
            harness,
            codec_import: CodecImport,
            fs_import: FsImport,
            http_import: HttpImport,
            jobs_import: JobsImport,
            journal_import: JournalImport,
            panes_import: PanesImport,
            process_import: ProcessImport,
            settings_import: SettingsImport,
            timers_import: TimersImport,
            tree_import: TreeImport,
            save_trace: Vec::new(),
            validate_trap_pending: false,
            _runner: self.runner,
            _websocket: self.websocket,
            _limits: self.limits,
            root,
        })
    }
    /// The tier.
    pub fn tier(mut self, _tier: Tier) -> ImportsFixtureBuilder {
        self.tier = _tier;
        self
    }

    /// The side.
    pub fn side(mut self, _side: Side) -> ImportsFixtureBuilder {
        self.side = _side;
        self
    }

    /// The grants.
    pub fn grants(mut self, _grants: &[&str]) -> ImportsFixtureBuilder {
        self.grants = _grants.iter().map(|grant| (*grant).into()).collect();
        self
    }

    /// The manifest; installs `test.imports` unless the JSON sets another id.
    pub fn manifest_json(mut self, _json: &str) -> ImportsFixtureBuilder {
        self.manifest = _json.into();
        self
    }

    /// The project root backing the `project` fs scope.
    pub fn project_root(mut self, _root: &Path) -> ImportsFixtureBuilder {
        self.project = Some(_root.into());
        self
    }

    /// A named limit (`fs_read_all_mib`, `fs_write_mib`, `journal_entries`, `storage_bucket_mib`, `node_metadata_kib`).
    pub fn limit(mut self, _name: &str, _value: u64) -> ImportsFixtureBuilder {
        self.limits.insert(_name.into(), _value);
        self
    }

    /// Installs a second plugin.
    pub fn also_install(mut self, _manifest_json: &str) -> ImportsFixtureBuilder {
        self.additional.push(_manifest_json.into());
        self
    }

    /// The fake settings-validate export.
    pub fn settings_validate(mut self, _validate: SettingsValidate) -> ImportsFixtureBuilder {
        self.validate = Some(_validate);
        self
    }

    /// The virtual latency of the fake settings-validate export.
    pub fn settings_validate_latency(mut self, _latency: Duration) -> ImportsFixtureBuilder {
        self.validate_latency = _latency;
        self
    }

    /// The fake job-runner export.
    pub fn job_runner(mut self, _runner: JobRunner) -> ImportsFixtureBuilder {
        self.runner = Some(_runner);
        self
    }

    /// Whether the websocket feature is on.
    pub fn websocket_feature(mut self, _on: bool) -> ImportsFixtureBuilder {
        self.websocket = _on;
        self
    }

    /// Builds the fixture.
    pub fn build(self) -> ImportsFixture {
        self.try_build()
            .unwrap_or_else(|error| panic!("invalid imports fixture setup: {error}"))
    }
}

/// Reports a production hook the rig needs but the production host does not expose yet. The
/// test that reaches it fails at this line with the missing hook named, instead of passing on a
/// value the rig made up.
fn blocked(hook: &str) -> ! {
    panic!("rig blocked: the production host does not expose {hook} yet")
}

/// The restricted import set of a job instance (WIT:ilium:host/jobs.*).
pub struct JobInstance {
    jobs_import: JobsImport,
    settings_import: SettingsImport,
    clock_import: ClockImport,
    log_import: LogImport,
}

impl std::fmt::Debug for JobInstance {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("JobInstance")
    }
}

impl JobInstance {
    /// The `jobs` import of the job instance.
    pub fn jobs(&mut self) -> &mut dyn HostJobs {
        &mut self.jobs_import
    }

    /// The `settings` import of the job instance.
    pub fn settings(&mut self) -> &mut dyn HostSettings {
        &mut self.settings_import
    }

    /// The `clock` import of the job instance.
    pub fn clock(&mut self) -> &mut dyn HostClock {
        &mut self.clock_import
    }

    /// The `log` import of the job instance.
    pub fn log(&mut self) -> &mut dyn HostLog {
        &mut self.log_import
    }

    /// Cancels the job from the host.
    pub fn cancel_from_host(&mut self) {
        blocked("a cancel hook on the job instance")
    }

    /// The progress values reported, clamped to 0..=100 with NaN dropped.
    pub fn reported_progress(&self) -> Vec<f32> {
        blocked("a progress recorder on the job instance")
    }
}

/// One plugin instance with every host import bound to it (test plan lane imports-core).
pub struct ImportsFixture {
    // Last: adapters drop before the owned directory is removed.
    harness: HarnessHandle,
    codec_import: CodecImport,
    fs_import: FsImport,
    http_import: HttpImport,
    jobs_import: JobsImport,
    journal_import: JournalImport,
    panes_import: PanesImport,
    process_import: ProcessImport,
    settings_import: SettingsImport,
    timers_import: TimersImport,
    tree_import: TreeImport,
    /// Steps of the last `user_save`, in the order they ran.
    save_trace: Vec<String>,
    /// Set by `make_next_validate_trap`; consumed by the next settings-validate call.
    validate_trap_pending: bool,
    log_import: LogImport,
    logger: Arc<Mutex<PluginLogger>>,
    budget: Arc<Mutex<PluginLogInstanceBudget>>,
    clock: FixtureClock,
    storage_import: FixtureStorage,
    storage_authorization: Arc<Mutex<StorageAuthorization>>,
    grants: Arc<Mutex<Vec<Grant>>>,
    kernel: Arc<FakeKernel>,
    plugin: String,
    manifest: serde_json::Value,
    errors: Arc<FixtureDiagnostics>,
    stopped: bool,
    next_node: u64,
    _additional: Vec<serde_json::Value>,
    validate: Option<SettingsValidate>,
    validate_latency: Duration,
    _runner: Option<JobRunner>,
    _websocket: bool,
    _limits: BTreeMap<String, u64>,
    root: tempfile::TempDir,
}

impl std::fmt::Debug for ImportsFixture {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ImportsFixture")
    }
}

impl Drop for ImportsFixture {
    fn drop(&mut self) {
        // Owned adapter handles may outlive the fixture; dropping their runtime
        // owner revokes authority before its private persistence is removed.
        if let Ok(mut grants) = self.grants.lock() {
            grants.clear();
        }
        if let Ok(mut authorization) = self.storage_authorization.lock() {
            authorization.set_state(StorageInstanceState::Inactive);
        }
    }
}

impl ImportsFixture {
    /// The real primitive adapter, sharing current grants and authoritative
    /// registered kinds with this fixture's kernel.
    pub fn basic(&self) -> BasicImport<'static> {
        let log = Mutex::new(
            LogImport::new(
                self.logger.clone(),
                self.budget.clone(),
                Arc::new(self.clock.clone()),
                self.errors.clone(),
            )
            .expect("fixture logger setup is valid"),
        );
        let errors = self.errors.clone();
        BasicImport::new(
            self.kernel.clone(),
            self.plugin.clone(),
            self.grants.clone(),
            move |message| {
                *errors.0.lock().unwrap() = Some(ErrorCode::NotPermitted);
                log.lock().unwrap().log(
                    ilium_plugin_api::generated::host::log::Level::Warn,
                    message.into(),
                );
            },
        )
    }
    /// Explicit current-grant replacement, observed by already borrowed adapters.
    pub fn replace_grants(&mut self, grants: &[&str]) -> Result<(), String> {
        if self.stopped {
            return Err("fixture instance is stopped".into());
        }
        let replacement = grants
            .iter()
            .map(|grant| Grant::parse(grant).map_err(|error| format!("invalid grant: {error:?}")))
            .collect::<Result<Vec<_>, _>>()?;
        *self.grants.lock().unwrap() = replacement;
        Ok(())
    }
    /// Add an actual pane of a kind already registered by the fixture manifest.
    pub fn add_basic_pane(&mut self, kind: &str, title: &str, columns: u16, rows: u16) -> NodeId {
        self.insert_node(
            None,
            title,
            ilium_plugin_api::generated::types::tree::NodeClass::Pane,
            Some(kind.into()),
            columns,
            rows,
        )
    }
    pub fn kernel(&self) -> Arc<FakeKernel> {
        self.kernel.clone()
    }
    pub fn manifest(&self) -> &serde_json::Value {
        &self.manifest
    }
    fn insert_node(
        &mut self,
        parent: Option<NodeId>,
        title: &str,
        class: ilium_plugin_api::generated::types::tree::NodeClass,
        kind: Option<String>,
        columns: u16,
        rows: u16,
    ) -> NodeId {
        let id = NodeId(self.next_node);
        self.next_node = self
            .next_node
            .checked_add(1)
            .expect("fixture node identity exhausted");
        let mut state = self.kernel.state();
        assert!(
            parent.is_none_or(|id| state.nodes.iter().any(|node| node.id == id)),
            "fixture parent must exist"
        );
        state.nodes.push(ilium_plugin_api::kernel::TreeNode {
            id,
            parent,
            class,
            content_kind: kind,
            title: title.into(),
            cwd: None,
            agent: None,
            locked: false,
            revision: 1,
        });
        state.sizes.insert(id, (columns, rows));
        state.epoch = state.epoch.saturating_add(1);
        id
    }
    fn persisted_logs(&self) -> Vec<LogLine> {
        let mut records = Vec::new();
        for suffix in [".4", ".3", ".2", ".1", ""] {
            let path = self
                .root
                .path()
                .join("logs")
                .join(format!("{}.log{suffix}", self.plugin));
            let text = match std::fs::read_to_string(path) {
                Ok(text) => text,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => panic!("fixture log read failed: {error}"),
            };
            for line in text.lines() {
                let Some(level) = line.get(20..25) else {
                    panic!("malformed persisted fixture log")
                };
                let Some(rest) = line.get(33..) else {
                    panic!("malformed persisted fixture log")
                };
                let Some((plugin, message)) = rest.split_once(' ') else {
                    panic!("malformed persisted fixture log")
                };
                records.push(LogLine {
                    plugin: plugin.into(),
                    level: level.trim().into(),
                    message: message.into(),
                });
            }
        }
        records
    }
    /// Starts a builder.
    pub fn builder() -> ImportsFixtureBuilder {
        ImportsFixtureBuilder {
            tier: Tier::T2,
            side: Side::Server,
            grants: Vec::new(),
            manifest: "{}".into(),
            project: None,
            limits: BTreeMap::new(),
            additional: Vec::new(),
            validate: None,
            validate_latency: Duration::ZERO,
            runner: None,
            websocket: false,
        }
    }

    /// The `log` import.
    pub fn log(&mut self) -> &mut dyn HostLog {
        &mut self.log_import
    }

    /// The `clock` import.
    pub fn clock(&mut self) -> &mut dyn HostClock {
        &mut self.clock
    }

    /// The `codec` import.
    pub fn codec(&mut self) -> &mut dyn HostCodec {
        &mut self.codec_import
    }

    /// The `fs` import.
    pub fn fs(&mut self) -> &mut dyn HostFs {
        &mut self.fs_import
    }

    /// The `http` import.
    pub fn http(&mut self) -> &mut dyn HostHttp {
        &mut self.http_import
    }

    /// The `jobs` import.
    pub fn jobs(&mut self) -> &mut dyn HostJobs {
        &mut self.jobs_import
    }

    /// The `journal` import.
    pub fn journal(&mut self) -> &mut dyn HostJournal {
        &mut self.journal_import
    }

    /// The `panes` import.
    pub fn panes(&mut self) -> &mut dyn HostPanes {
        &mut self.panes_import
    }

    /// The `process` import.
    pub fn process(&mut self) -> &mut dyn HostProcess {
        &mut self.process_import
    }

    /// The `settings` import.
    pub fn settings(&mut self) -> &mut dyn HostSettings {
        &mut self.settings_import
    }

    /// The `storage` import.
    pub fn storage(&mut self) -> &mut dyn HostStorage {
        &mut self.storage_import
    }

    /// The `timers` import.
    pub fn timers(&mut self) -> &mut dyn HostTimers {
        &mut self.timers_import
    }

    /// The `tree` import.
    pub fn tree(&mut self) -> &mut dyn HostTree {
        &mut self.tree_import
    }

    /// Starts a job instance of a kind.
    pub fn job_instance(&mut self, _kind: &str, _input: &[u8]) -> JobInstance {
        JobInstance {
            jobs_import: JobsImport,
            settings_import: SettingsImport,
            clock_import: ClockImport,
            log_import: LogImport::new(
                self.logger.clone(),
                self.budget.clone(),
                Arc::new(self.clock.clone()),
                self.errors.clone(),
            )
            .expect("fixture logger setup is valid"),
        }
    }

    /// Moves the manual clock and runs due work.
    pub fn advance(&mut self, _by: Duration) {
        {
            let mut clock = self.clock.0.lock().unwrap();
            clock.wall = clock
                .wall
                .saturating_add(crate::imports::clock::duration_millis(_by));
            clock.monotonic = clock
                .monotonic
                .saturating_add(crate::imports::clock::duration_millis(_by));
        }
        self.kernel.state().clock = Duration::from_millis(self.clock.0.lock().unwrap().monotonic);
        self.harness.advance_clock(_by);
        self.pump();
    }

    /// Runs pending work without moving the clock.
    pub fn pump(&mut self) {
        self.log_import.tick();
        self.harness.pump(PUMP_ROUNDS);
    }

    /// Sets the wall clock.
    pub fn set_wall_clock_unix_millis(&mut self, _millis: u64) {
        self.clock.0.lock().unwrap().wall = _millis;
    }

    /// Keeps async requests pending.
    pub fn hold_async_completions(&mut self, on: bool) {
        // The kernel hook holds only the next request; releasing clears that hold.
        if on {
            self.harness.kernel().hold_next_request();
        } else {
            let _ = self.harness.kernel().take_hold_next_request();
        }
    }

    /// Waits for the outcome of a request.
    pub fn wait_outcome(&mut self, request: RequestId, timeout: Duration) -> Option<Outcome> {
        self.harness.advance_clock(timeout);
        self.harness.pump(PUMP_ROUNDS);
        self.harness
            .take_result(request)
            .ok()
            .map(|result| result.outcome)
    }

    /// Every log line so far.
    pub fn logs(&self) -> Vec<LogLine> {
        self.persisted_logs()
    }

    /// Every event delivered to the fixture plugin.
    pub fn events(&self) -> Vec<Event> {
        self.kernel.state().events.iter().cloned().collect()
    }

    /// Every event delivered to a plugin.
    pub fn events_of(&self, _plugin: &str) -> Vec<Event> {
        if _plugin == self.plugin {
            self.events()
        } else {
            Vec::new()
        }
    }

    /// The settings target last opened.
    pub fn opened_settings_target(&self) -> Option<String> {
        blocked("the settings UI open-target record")
    }

    /// The values of a choice row.
    pub fn choice_values(&self, _key: &str) -> Vec<String> {
        blocked("the settings UI choice model")
    }

    /// The item ids of a choice row.
    pub fn choice_item_ids(&self, _key: &str) -> Vec<String> {
        blocked("the settings UI choice model")
    }

    /// Whether a choice row shows a value as unavailable.
    pub fn choice_shows_unavailable(&self, _key: &str, _value: &str) -> bool {
        blocked("the settings UI choice model")
    }

    /// The stored value of a setting.
    pub fn settings_value(&self, key: &str) -> serde_json::Value {
        self.harness
            .stored_setting(key)
            .unwrap_or(serde_json::Value::Null)
    }

    /// Whether the plugin instance is stopped.
    pub fn plugin_is_stopped(&self) -> bool {
        self.stopped
    }

    /// The note of a settings row.
    pub fn row_note(&self, _key: &str) -> Option<String> {
        blocked("the settings UI row model")
    }

    /// Whether a settings row currently shows the `Saving…` marker.
    pub fn row_saving(&self, _key: &str) -> bool {
        blocked("the settings UI row model")
    }

    /// The text of a live info row.
    pub fn info_text(&self, _key: &str) -> Option<String> {
        blocked("the settings UI info model")
    }

    /// The spans of a rich info row.
    pub fn info_spans(&self, _key: &str) -> Vec<RichSpan> {
        blocked("the settings UI info model")
    }

    /// The state of a settings row.
    pub fn row_state(&self, _key: &str) -> RowState {
        blocked("the settings UI row model")
    }

    /// The state of one item of a list row.
    pub fn item_state(&self, _key: &str, _index: u32) -> Option<RowState> {
        blocked("the settings UI row model")
    }

    /// The segments of a meter row.
    pub fn meter_segments(&self, _id: &str) -> Vec<MeterSegment> {
        blocked("the settings UI meter model")
    }

    /// The warning codes raised while loading settings.
    pub fn load_warning_codes(&self) -> Vec<String> {
        blocked("the load warning record")
    }

    /// Plugin sections drawn in a settings tab.
    pub fn drawn_plugin_sections(&self, _tab: &str) -> usize {
        blocked("the plugin section draw record")
    }

    /// Plugin tabs drawn.
    pub fn drawn_plugin_tabs(&self) -> usize {
        blocked("the plugin tab draw record")
    }

    /// The steps of the last save.
    pub fn save_trace(&self) -> Vec<String> {
        self.save_trace.clone()
    }

    /// Replaces the stored settings JSON.
    pub fn restore_stored_json(&mut self, json: &str) {
        self.settings_import
            .set_json(json.to_owned())
            .unwrap_or_else(|error| panic!("stored settings did not restore: {}", error.message));
    }

    /// The user edits a row.
    pub fn user_edit(&mut self, key: &str, value_json: &str, input: UserInput) -> UserSave {
        match input {
            UserInput::Toggle | UserInput::Choice | UserInput::DialogConfirm => {
                let value: serde_json::Value = match serde_json::from_str(value_json) {
                    Ok(value) => value,
                    Err(error) => return UserSave::Refused(error.to_string()),
                };
                let mut patch = serde_json::Map::new();
                patch.insert(key.to_owned(), value);
                self.user_save(&serde_json::Value::Object(patch).to_string())
            }
            // Debounced inputs save after the 600 ms pause; no production debounce hook exists yet.
            UserInput::NumberStep | UserInput::Typed | UserInput::TextArea => UserSave::Pending,
        }
    }

    /// The user saves a patch: size and text limits, then the validate export, then the write.
    pub fn user_save(&mut self, patch_json: &str) -> UserSave {
        self.save_trace.clear();
        self.save_trace.push("limits".to_owned());
        if let Err(reason) = check_patch_limits(patch_json) {
            return UserSave::Refused(reason);
        }
        let stored_json = self.settings_import.get_json();
        let merged_json = match merge_settings_patch(&stored_json, patch_json) {
            Ok(merged_json) => merged_json,
            Err(reason) => return UserSave::Refused(reason),
        };
        {
            let latency = self.validate_latency;
            if latency > SETTINGS_VALIDATE_BUDGET {
                return UserSave::Refused(format!(
                    "settings-validate answered after {} ms",
                    latency.as_millis()
                ));
            }
        }
        if let Some(validate) = self.validate.as_mut() {
            self.save_trace.push("settings-validate".to_owned());
            if std::mem::take(&mut self.validate_trap_pending) {
                self.save_trace.push("settings-validate trapped".to_owned());
                return UserSave::Refused("settings-validate trapped".to_owned());
            }
            if let Err(errors) = validate(patch_json, &merged_json) {
                let reason = errors.first().map_or_else(
                    || "settings-validate refused the patch".to_owned(),
                    |error| error.message.clone(),
                );
                return UserSave::Refused(reason);
            }
        }
        match self.settings_import.set_json(patch_json.to_owned()) {
            Ok(()) => {
                self.save_trace.push("settings-changed".to_owned());
                UserSave::Saved
            }
            Err(error) => UserSave::Refused(error.message),
        }
    }

    /// The next job traps.
    pub fn make_next_job_trap(&mut self) {
        blocked("a fault injection hook in the job runner")
    }

    /// The next job never returns.
    pub fn make_next_job_run_forever(&mut self) {
        blocked("a fault injection hook in the job runner")
    }

    /// The next job exceeds its memory limit.
    pub fn make_next_job_exceed_memory(&mut self) {
        blocked("a fault injection hook in the job runner")
    }

    /// The next settings-validate call traps.
    pub fn make_next_validate_trap(&mut self) {
        self.validate_trap_pending = true;
    }

    /// Stops the plugin instance.
    pub fn stop_plugin(&mut self) {
        let mut grants = self.grants.lock().unwrap();
        let mut authorization = self.storage_authorization.lock().unwrap();
        grants.clear();
        authorization.set_state(StorageInstanceState::Inactive);
        self.stopped = true;
        self.harness.deactivate_plugin();
    }

    /// Starts a background request.
    pub fn start_background_request(&mut self) -> RequestId {
        self.jobs_import
            .start("background".to_owned(), Vec::new())
            .unwrap_or_else(|error| panic!("background request refused: {}", error.message))
    }

    /// Completes a background request. The harness hook completes the oldest pending one.
    pub fn complete_background_request(&mut self, _request: RequestId) {
        self.harness.complete_oldest_request();
    }

    /// The user chooses paths in the picker.
    pub fn picker_choose(&mut self, _paths: Vec<FsPath>) {
        blocked("the file picker of the settings UI")
    }

    /// The user cancels the picker.
    pub fn picker_cancel(&mut self) {
        blocked("the file picker of the settings UI")
    }

    /// Waits for an event of a watch.
    pub fn wait_fs_event(&mut self, _watch: WatchId, _timeout: Duration) -> bool {
        blocked("a watch event queue on the fs import")
    }

    /// Waits for chunks of a stream.
    pub fn wait_stream_chunks(
        &mut self,
        _stream: StreamId,
        _count: usize,
        _timeout: Duration,
    ) -> Vec<Vec<u8>> {
        blocked("a chunk queue on the stream import")
    }

    /// Waits for output of a child.
    pub fn wait_child_output(
        &mut self,
        _child: ChildHandle,
        _expected: &[u8],
        _timeout: Duration,
    ) -> bool {
        blocked("the child output queue of the process import")
    }

    /// Waits for a child to exit.
    pub fn wait_child_exit(&mut self, _child: ChildHandle, _timeout: Duration) -> bool {
        blocked("the child exit record of the process import")
    }

    /// Adds a terminal pane.
    pub fn add_terminal_pane(&mut self, _title: &str, _columns: u16, _rows: u16) -> NodeId {
        self.insert_node(
            None,
            _title,
            ilium_plugin_api::generated::types::tree::NodeClass::Pane,
            Some("ilium.terminal".into()),
            _columns,
            _rows,
        )
    }

    /// Adds a pane restored from a snapshot.
    pub fn add_restored_pane(&mut self, _title: &str, _columns: u16, _rows: u16) -> NodeId {
        let pane = self.add_terminal_pane(_title, _columns, _rows);
        self.kernel.set_pane_restored(pane, true);
        pane
    }

    /// Adds a group.
    pub fn add_group(&mut self, _title: &str) -> NodeId {
        self.insert_node(
            None,
            _title,
            ilium_plugin_api::generated::types::tree::NodeClass::Folder,
            None,
            0,
            0,
        )
    }

    /// Adds a group under a parent.
    pub fn add_group_under(&mut self, _parent: NodeId, _title: &str) -> NodeId {
        self.insert_node(
            Some(_parent),
            _title,
            ilium_plugin_api::generated::types::tree::NodeClass::Folder,
            None,
            0,
            0,
        )
    }

    /// Adds a terminal pane under a parent.
    pub fn add_terminal_pane_under(
        &mut self,
        _parent: NodeId,
        _title: &str,
        _columns: u16,
        _rows: u16,
    ) -> NodeId {
        self.insert_node(
            Some(_parent),
            _title,
            ilium_plugin_api::generated::types::tree::NodeClass::Pane,
            Some("ilium.terminal".into()),
            _columns,
            _rows,
        )
    }

    /// Exits a pane.
    pub fn exit_pane(&mut self, _pane: NodeId) {
        self.kernel.remove_node(_pane);
    }

    /// Focuses a pane.
    pub fn focus_pane(&mut self, _pane: NodeId) {
        self.kernel.state().focus = Some(_pane);
    }

    /// Records a host journal entry (`pane-created`, `pane-exited`, `input-sent`).
    pub fn record_host_journal_entry(&mut self, pane: NodeId, kind: &str) {
        self.harness
            .kernel()
            .state()
            .journal
            .insert((pane, kind.to_owned()));
    }

    /// Takes a storage lease as a second caller.
    pub fn other_caller_lease(&mut self, _key: &str, _ttl_millis: u32) -> bool {
        let context = self.storage_import.import.context();
        let caller = crate::storage::PluginStorage::open_with_context(
            &self.root.path().join("storage"),
            context.plugin.clone(),
            context.scope.clone(),
            context.tier,
            context.bucket_limit_bytes,
            context.shared_limit_bytes,
        )
        .expect("fixture storage context was admitted");
        match caller.lease_at(
            _key,
            _ttl_millis,
            UNIX_EPOCH + Duration::from_millis(self.clock.0.lock().unwrap().wall),
        ) {
            Ok(acquired) => acquired,
            Err(error) => {
                *self.errors.0.lock().unwrap() = Some(error.code);
                false
            }
        }
    }

    /// Makes the on-disk data version of a bucket newer.
    pub fn make_bucket_data_version_newer(&mut self, _bucket: Bucket) {
        let context = self.storage_import.import.context();
        let mut newer = context.scope.clone();
        newer.data_version = newer
            .data_version
            .checked_add(1)
            .expect("fixture data version exhausted");
        let engine = crate::storage::PluginStorage::open_with_context(
            &self.root.path().join("storage"),
            context.plugin.clone(),
            newer.clone(),
            context.tier,
            context.bucket_limit_bytes,
            context.shared_limit_bytes,
        )
        .expect("fixture storage context was admitted");
        engine
            .stamp_bucket_after_migration(_bucket.as_str(), newer.data_version)
            .expect("fixture newer bucket stamp write");
    }

    /// The code of the last refused infallible import call.
    pub fn last_import_error_code(&self) -> Option<ErrorCode> {
        *self.errors.0.lock().unwrap()
    }

    /// Sets the agent signature of a node.
    pub fn set_agent(&mut self, _node: NodeId, _signature: &str) {
        if let Some(node) = self
            .kernel
            .state()
            .nodes
            .iter_mut()
            .find(|node| node.id == _node)
        {
            node.agent = Some(ilium_plugin_api::generated::types::tree::AgentView {
                signature: _signature.into(),
                activity: ilium_plugin_api::generated::types::tree::AgentActivity::None,
                objective: None,
                goal: None,
            });
        }
    }

    /// Locks a node.
    pub fn lock_node(&mut self, _node: NodeId) {
        if let Some(node) = self
            .kernel
            .state()
            .nodes
            .iter_mut()
            .find(|node| node.id == _node)
        {
            node.locked = true;
            node.revision = node.revision.saturating_add(1);
        }
    }

    /// Renames a node.
    pub fn rename_node(&mut self, _node: NodeId, _title: &str) {
        if let Some(node) = self
            .kernel
            .state()
            .nodes
            .iter_mut()
            .find(|node| node.id == _node)
        {
            node.title = _title.into();
            node.revision = node.revision.saturating_add(1);
        }
    }

    /// Reads node metadata as a second installed plugin.
    pub fn other_plugin_metadata(&mut self, _node: NodeId) -> Option<Vec<u8>> {
        blocked("a second installed plugin's node metadata")
    }
}

#[cfg(test)]
mod tests {
    use super::{ImportsFixture, ImportsFixtureBuilder, UserInput, UserSave};
    use ilium_plugin_api::generated::types::ErrorCode;
    use ilium_plugin_api::side::Side;
    use ilium_plugin_api::Tier;
    use std::path::Path;
    use std::time::Duration;

    /// A value of `count` ASCII characters.
    fn repeated_text(count: usize) -> String {
        "a".repeat(count)
    }

    /// A fixture with the default manifest and no user-supplied exports.
    fn default_fixture() -> ImportsFixture {
        ImportsFixture::builder().manifest_json("{}").build()
    }

    /// Writes one info row and returns the error code, if the call was refused.
    fn info_refusal_code(fixture: &mut ImportsFixture, key: &str, text: &str) -> Option<ErrorCode> {
        fixture
            .settings()
            .set_info(key.to_string(), text.to_string())
            .err()
            .map(|error| error.code)
    }

    #[test]
    fn builder_debug_text_names_the_builder() {
        let builder = ImportsFixture::builder();
        assert_eq!(format!("{builder:?}"), "ImportsFixtureBuilder");
    }

    #[test]
    fn fixture_debug_text_names_the_fixture() {
        let fixture = default_fixture();
        assert_eq!(format!("{fixture:?}"), "ImportsFixture");
    }

    #[test]
    fn job_instance_debug_text_names_the_job_instance() {
        let mut fixture = default_fixture();
        let job = fixture.job_instance("test.kind", &[]);
        assert_eq!(format!("{job:?}"), "JobInstance");
    }

    #[test]
    fn user_input_variants_are_distinct_and_comparable() {
        assert_eq!(UserInput::Toggle, UserInput::Toggle);
        assert_ne!(UserInput::Toggle, UserInput::Choice);
        assert_ne!(UserInput::Typed, UserInput::TextArea);
    }

    #[test]
    fn user_save_refused_compares_by_message() {
        assert_eq!(
            UserSave::Refused("bad".to_string()),
            UserSave::Refused("bad".to_string())
        );
        assert_ne!(
            UserSave::Refused("bad".to_string()),
            UserSave::Refused("worse".to_string())
        );
        assert_ne!(UserSave::Refused(String::new()), UserSave::Saved);
        assert_ne!(UserSave::Pending, UserSave::Saved);
    }

    #[test]
    fn builder_accepts_every_documented_setter_in_one_chain() {
        let fixture = ImportsFixture::builder()
            .tier(Tier::T1)
            .side(Side::Server)
            .grants(&["fs.read:project", "http:example.invalid"])
            .manifest_json("{}")
            .project_root(Path::new("."))
            .limit("fs_write_mib", 1)
            .websocket_feature(false)
            .build();
        assert!(!fixture.plugin_is_stopped());
    }

    #[test]
    fn builder_accepts_empty_grant_list() {
        let fixture = ImportsFixture::builder()
            .grants(&[])
            .manifest_json("{}")
            .build();
        assert!(!fixture.plugin_is_stopped());
    }

    #[test]
    fn builder_limit_accepts_zero_and_maximum_values() {
        let fixture = ImportsFixture::builder()
            .limit("journal_entries", 0)
            .limit("node_metadata_kib", u64::MAX)
            .manifest_json("{}")
            .build();
        assert!(!fixture.plugin_is_stopped());
    }

    #[test]
    fn builder_settings_validate_latency_accepts_zero() {
        let fixture = ImportsFixture::builder()
            .manifest_json("{}")
            .settings_validate_latency(Duration::ZERO)
            .build();
        assert!(!fixture.plugin_is_stopped());
    }

    /// REQ: SET-022
    #[test]
    fn settings_validate_answer_at_100_ms_still_saves() {
        let mut fixture = ImportsFixture::builder()
            .manifest_json("{}")
            .settings_validate(Box::new(|_patch, _merged| Ok(())))
            .settings_validate_latency(Duration::from_millis(100))
            .build();
        assert_eq!(fixture.user_save("{\"a\":1}"), UserSave::Saved);
    }

    /// REQ: SET-022
    #[test]
    fn settings_validate_answer_at_150_ms_refuses_the_save() {
        let mut fixture = ImportsFixture::builder()
            .manifest_json("{}")
            .settings_validate(Box::new(|_patch, _merged| Ok(())))
            .settings_validate_latency(Duration::from_millis(150))
            .build();
        let old_value = fixture.settings_value("a");
        assert!(matches!(
            fixture.user_save("{\"a\":2}"),
            UserSave::Refused(_)
        ));
        assert_eq!(fixture.settings_value("a"), old_value);
    }

    #[test]
    fn job_runner_returning_empty_output_is_accepted() {
        let fixture = ImportsFixture::builder()
            .manifest_json("{}")
            .job_runner(Box::new(|_kind, _input| Ok(Vec::new())))
            .build();
        assert!(!fixture.plugin_is_stopped());
    }

    #[test]
    fn fresh_fixture_has_no_logs_and_no_events() {
        let fixture = default_fixture();
        assert!(fixture.logs().is_empty());
        assert!(fixture.events().is_empty());
    }

    #[test]
    fn advance_by_zero_and_pump_leave_a_fresh_fixture_quiet() {
        let mut fixture = default_fixture();
        fixture.advance(Duration::ZERO);
        fixture.pump();
        assert!(fixture.events().is_empty());
    }

    #[test]
    fn stop_plugin_marks_the_instance_stopped() {
        let mut fixture = default_fixture();
        assert!(!fixture.plugin_is_stopped());
        fixture.stop_plugin();
        assert!(fixture.plugin_is_stopped());
    }

    #[test]
    fn unknown_choice_row_has_no_values_or_items() {
        let fixture = default_fixture();
        assert!(fixture.choice_values("no-such-row").is_empty());
        assert!(fixture.choice_item_ids("no-such-row").is_empty());
    }

    #[test]
    fn fresh_job_instance_reports_no_progress() {
        let fixture_job = {
            let mut fixture = default_fixture();
            fixture.job_instance("test.kind", &[])
        };
        assert!(fixture_job.reported_progress().is_empty());
    }

    #[test]
    fn restore_stored_json_then_read_back_value() {
        let mut fixture = default_fixture();
        fixture.restore_stored_json("{\"a\":1}");
        assert_eq!(fixture.settings_value("a"), serde_json::json!(1));
    }

    /// REQ: SET-023
    #[test]
    fn toggle_choice_and_dialog_confirm_save_at_once() {
        let mut fixture = default_fixture();
        assert_eq!(
            fixture.user_edit("flag", "true", UserInput::Toggle),
            UserSave::Saved
        );
        assert_eq!(
            fixture.user_edit("mode", "\"fast\"", UserInput::Choice),
            UserSave::Saved
        );
        assert_eq!(
            fixture.user_edit("confirm", "true", UserInput::DialogConfirm),
            UserSave::Saved
        );
    }

    /// REQ: SET-023
    #[test]
    fn typed_number_and_text_area_are_pending_during_debounce() {
        let mut fixture = default_fixture();
        assert_eq!(
            fixture.user_edit("count", "5", UserInput::NumberStep),
            UserSave::Pending
        );
        assert_eq!(
            fixture.user_edit("name", "\"abc\"", UserInput::Typed),
            UserSave::Pending
        );
        assert_eq!(
            fixture.user_edit("notes", "\"abc\"", UserInput::TextArea),
            UserSave::Pending
        );
    }

    /// REQ: SET-020
    #[test]
    fn number_typed_outside_bounds_is_refused_and_keeps_stored_value() {
        let mut fixture = default_fixture();
        fixture.restore_stored_json("{\"limit\":3}");
        let outcome = fixture.user_edit("limit", "99", UserInput::Typed);
        assert!(matches!(outcome, UserSave::Refused(_)));
        assert_eq!(fixture.settings_value("limit"), serde_json::json!(3));
    }

    /// REQ: SET-021
    #[test]
    fn save_runs_settings_validate_before_settings_changed() {
        let mut fixture = ImportsFixture::builder()
            .manifest_json("{}")
            .settings_validate(Box::new(|_patch, _merged| Ok(())))
            .build();
        assert_eq!(fixture.user_save("{\"a\":1}"), UserSave::Saved);
        let trace = fixture.save_trace();
        let validate_at = trace.iter().position(|step| step.contains("validate"));
        let changed_at = trace.iter().position(|step| step.contains("changed"));
        assert!(validate_at.is_some());
        assert!(changed_at.is_some());
        assert!(validate_at < changed_at);
    }

    /// REQ: SET-031
    #[test]
    fn set_json_save_is_written_at_once() {
        let mut fixture = default_fixture();
        assert_eq!(fixture.user_save("{\"a\":1}"), UserSave::Saved);
        assert_eq!(fixture.settings_value("a"), serde_json::json!(1));
    }

    /// REQ: SET-024
    #[test]
    fn save_with_more_than_256_kib_is_refused() {
        let mut fixture = default_fixture();
        let oversized_patch = format!("{{\"big\":\"{}\"}}", repeated_text(300 * 1024));
        assert!(matches!(
            fixture.user_save(&oversized_patch),
            UserSave::Refused(_)
        ));
    }

    /// REQ: SET-024
    #[test]
    fn text_value_of_4096_characters_is_accepted() {
        let mut fixture = default_fixture();
        let patch = format!("{{\"text\":\"{}\"}}", repeated_text(4096));
        assert_eq!(fixture.user_save(&patch), UserSave::Saved);
    }

    /// REQ: SET-024
    #[test]
    fn text_value_of_4097_characters_is_refused() {
        let mut fixture = default_fixture();
        let patch = format!("{{\"text\":\"{}\"}}", repeated_text(4097));
        assert!(matches!(fixture.user_save(&patch), UserSave::Refused(_)));
    }

    /// REQ: SET-050
    #[test]
    fn eleventh_settings_call_in_one_second_returns_rate_limited() {
        let mut fixture = default_fixture();
        for index in 0..10 {
            assert_eq!(
                info_refusal_code(&mut fixture, &format!("row-{index}"), "x"),
                None
            );
        }
        assert_eq!(
            info_refusal_code(&mut fixture, "row-10", "x"),
            Some(ErrorCode::RateLimited)
        );
        assert_eq!(
            fixture.last_import_error_code(),
            Some(ErrorCode::RateLimited)
        );
    }

    /// REQ: SET-051
    #[test]
    fn info_text_of_120_characters_is_accepted() {
        let mut fixture = default_fixture();
        assert_eq!(
            info_refusal_code(&mut fixture, "info", &repeated_text(120)),
            None
        );
    }

    /// REQ: SET-051
    #[test]
    fn info_text_of_121_characters_returns_invalid_argument() {
        let mut fixture = default_fixture();
        assert_eq!(
            info_refusal_code(&mut fixture, "info", &repeated_text(121)),
            Some(ErrorCode::InvalidArgument)
        );
    }

    /// REQ: SET-051
    #[test]
    fn info_text_empty_string_is_accepted() {
        let mut fixture = default_fixture();
        assert_eq!(info_refusal_code(&mut fixture, "info", ""), None);
    }

    #[test]
    fn last_import_error_code_is_none_before_any_refused_call() {
        let fixture = default_fixture();
        assert_eq!(fixture.last_import_error_code(), None);
    }

    #[test]
    fn unknown_setting_key_reads_as_absent_and_unknown_row_has_no_note() {
        let fixture = default_fixture();
        assert!(fixture.row_note("no-such-row").is_none());
        assert!(fixture.info_text("no-such-row").is_none());
    }
}

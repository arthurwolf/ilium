//! Imports fixture: one plugin instance with a manifest, grants, a project directory, a manual
//! clock and limits, exposing every host import bound to that instance (test plan lane
//! imports-core, SET-*, WIT:ilium:host/*). Test support only.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ilium_plugin_api::generated::host::plugins::InstanceState;
use ilium_plugin_api::generated::host::{
    Bucket, ChildHandle, FieldError, HostClock, HostCodec, HostFs, HostHttp, HostJobs, HostJournal,
    HostLog, HostPanes, HostProcess, HostSettings, HostStorage, HostTimers, HostTree,
};
use ilium_plugin_api::generated::types::intents::{
    CreateGroupSpec, CreatePaneSpec, SetNodeLockedSpec,
};
use ilium_plugin_api::generated::types::tree::Placement;
use ilium_plugin_api::generated::types::{
    ErrorCode, Event, FsPath, Intent, MeterSegment, NodeId, Outcome, PluginError, RequestId,
    RichSpan, RowState, StreamId, WatchId,
};
use ilium_plugin_api::ids::PluginId;
use ilium_plugin_api::kernel::{KernelFacade, Origin};
use ilium_plugin_api::side::Side;
use ilium_plugin_api::Tier;

use crate::imports::clock::ClockImport;
use crate::imports::codec::CodecImport;
use crate::imports::fs::FsImport;
use crate::imports::http::HttpImport;
use crate::imports::jobs::JobsImport;
use crate::imports::journal::JournalImport;
use crate::imports::log::LogImport;
use crate::imports::panes::PanesImport;
use crate::imports::process::ProcessImport;
use crate::imports::settings::SettingsImport;
use crate::imports::storage::{
    StorageAuthorization, StorageImport, StorageImportContext, StorageInstanceState,
};
use crate::imports::timers::TimersImport;
use crate::imports::tree::TreeImport;
use crate::storage::StorageContext;
use crate::telemetry::LogLine;
use crate::testing::harness::{HarnessHandle, HostHarness, NullGuest};

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

/// Pump rounds that `pump` and `advance` ask the production harness to run.
const PUMP_ROUNDS: u32 = 64;

/// Longest settings validate answer that still saves, in virtual time (SET-022).
const SETTINGS_VALIDATE_BUDGET: Duration = Duration::from_millis(100);

/// Largest settings patch a save accepts, in bytes (SET-024).
const SETTINGS_PATCH_LIMIT_BYTES: usize = 256 * 1024;

/// Longest text value a settings patch may carry, in characters (SET-024).
const SETTINGS_TEXT_LIMIT_CHARS: usize = 4096;

/// Bucket quota of the fixture's storage engine, in bytes.
/// The default bucket quota of api-storage.md (16 MiB), the ceiling for tiers 2 and 3.
const FIXTURE_BUCKET_LIMIT_BYTES: usize = 16 * 1024 * 1024;

/// Shared quota of the fixture's storage engine, in bytes.
const FIXTURE_SHARED_LIMIT_BYTES: usize = 128 * 1024 * 1024;

/// Numbers the storage roots of fixtures built in this process.
static STORAGE_ROOT_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// A fresh directory for one fixture's storage engine, unique within this process.
fn fixture_storage_root() -> PathBuf {
    let sequence = STORAGE_ROOT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "ilium-imports-fixture-{}-{sequence}",
        std::process::id()
    ))
}

/// The production storage import of the fixture plugin, opened on its own root.
/// Storage is always granted (api-storage.md), so the fixture always holds one.
fn open_fixture_storage(root: &Path, tier: Tier, side: Side) -> StorageImport {
    let plugin = PluginId("test.imports".to_owned());
    let context = StorageImportContext {
        plugin: plugin.clone(),
        side,
        scope: StorageContext {
            project: None,
            session: None,
            data_version: 1,
        },
        tier,
        bucket_limit_bytes: FIXTURE_BUCKET_LIMIT_BYTES,
        shared_limit_bytes: FIXTURE_SHARED_LIMIT_BYTES,
    };
    let authorization = Arc::new(Mutex::new(StorageAuthorization::new(
        plugin,
        side,
        StorageInstanceState::Active,
    )));
    StorageImport::open(root, context, authorization, |_, _| {})
        .unwrap_or_else(|error| panic!("imports fixture storage did not open: {}", error.message))
}

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

/// Creates a node through the fake kernel's intent path and returns its id.
/// A refused intent is a fixture misuse, so it panics with the node kind named.
fn create_fixture_node(harness: &HarnessHandle, intent: Intent, kind_text: &str) -> NodeId {
    match harness.kernel().apply_intent(&Origin::Kernel, intent) {
        Outcome::OkNode(node) => node,
        _ => panic!("imports fixture could not create {kind_text}"),
    }
}

/// The create-pane intent of a terminal pane, under an optional parent.
fn terminal_pane_spec(parent: Option<NodeId>, title: &str) -> CreatePaneSpec {
    CreatePaneSpec {
        parent,
        title: title.to_owned(),
        content_kind: "terminal".to_owned(),
        cwd: None,
        command: None,
        initial_input: None,
    }
}

/// The create-group intent of an empty group, under an optional parent.
fn empty_group_spec(parent: Option<NodeId>, title: &str) -> CreateGroupSpec {
    CreateGroupSpec {
        parent,
        title: title.to_owned(),
        placement: Placement::InsideLast,
        children: Vec::new(),
    }
}

/// Builder inputs that have no production sink yet; kept so the setters stay total.
struct FixtureConfig {
    manifest_json: Option<String>,
    project_root: Option<PathBuf>,
    limits: Vec<(String, u64)>,
    also_install: Vec<String>,
    settings_validate: Option<SettingsValidate>,
    settings_validate_latency: Option<Duration>,
    job_runner: Option<JobRunner>,
    websocket_feature: bool,
}

/// Builds an [`ImportsFixture`].
#[derive(Default)]
pub struct ImportsFixtureBuilder {
    tier: Option<Tier>,
    side: Option<Side>,
    grants: Vec<String>,
    manifest_json: Option<String>,
    project_root: Option<PathBuf>,
    limits: Vec<(String, u64)>,
    also_install: Vec<String>,
    settings_validate: Option<SettingsValidate>,
    settings_validate_latency: Option<Duration>,
    job_runner: Option<JobRunner>,
    websocket_feature: bool,
}

impl std::fmt::Debug for ImportsFixtureBuilder {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ImportsFixtureBuilder")
    }
}

impl ImportsFixtureBuilder {
    /// The tier.
    pub fn tier(mut self, tier: Tier) -> ImportsFixtureBuilder {
        self.tier = Some(tier);
        self
    }

    /// The side.
    pub fn side(mut self, side: Side) -> ImportsFixtureBuilder {
        self.side = Some(side);
        self
    }

    /// The grants.
    pub fn grants(mut self, grants: &[&str]) -> ImportsFixtureBuilder {
        self.grants = grants.iter().map(|grant| (*grant).to_owned()).collect();
        self
    }

    /// The manifest; installs `test.imports` unless the JSON sets another id.
    pub fn manifest_json(mut self, json: &str) -> ImportsFixtureBuilder {
        self.manifest_json = Some(json.to_owned());
        self
    }

    /// The project root backing the `project` fs scope.
    pub fn project_root(mut self, root: &Path) -> ImportsFixtureBuilder {
        self.project_root = Some(root.to_path_buf());
        self
    }

    /// A named limit (`fs_read_all_mib`, `fs_write_mib`, `journal_entries`, `storage_bucket_mib`, `node_metadata_kib`).
    pub fn limit(mut self, name: &str, value: u64) -> ImportsFixtureBuilder {
        self.limits.push((name.to_owned(), value));
        self
    }

    /// Installs a second plugin.
    pub fn also_install(mut self, manifest_json: &str) -> ImportsFixtureBuilder {
        self.also_install.push(manifest_json.to_owned());
        self
    }

    /// The fake settings-validate export.
    pub fn settings_validate(mut self, validate: SettingsValidate) -> ImportsFixtureBuilder {
        self.settings_validate = Some(validate);
        self
    }

    /// The virtual latency of the fake settings-validate export.
    pub fn settings_validate_latency(mut self, latency: Duration) -> ImportsFixtureBuilder {
        self.settings_validate_latency = Some(latency);
        self
    }

    /// The fake job-runner export.
    pub fn job_runner(mut self, runner: JobRunner) -> ImportsFixtureBuilder {
        self.job_runner = Some(runner);
        self
    }

    /// Whether the websocket feature is on.
    pub fn websocket_feature(mut self, on: bool) -> ImportsFixtureBuilder {
        self.websocket_feature = on;
        self
    }

    /// Builds the fixture on the production host harness, with an in-process guest that
    /// exports nothing, so every import call reaches the production import implementations.
    pub fn build(self) -> ImportsFixture {
        let grant_refs: Vec<&str> = self.grants.iter().map(String::as_str).collect();
        let tier = self.tier.unwrap_or(Tier::T3);
        let side = self.side.unwrap_or(Side::Server);
        let storage_import = open_fixture_storage(&fixture_storage_root(), tier, side);
        let harness = HostHarness::new()
            .tier(tier)
            .side(side)
            .grants(&grant_refs)
            .with_native_guest(Box::new(NullGuest))
            .build()
            .unwrap_or_else(|error| panic!("imports fixture harness did not build: {error}"));
        ImportsFixture {
            harness,
            config: FixtureConfig {
                manifest_json: self.manifest_json,
                project_root: self.project_root,
                limits: self.limits,
                also_install: self.also_install,
                settings_validate: self.settings_validate,
                settings_validate_latency: self.settings_validate_latency,
                job_runner: self.job_runner,
                websocket_feature: self.websocket_feature,
            },
            log_import: LogImport,
            clock_import: ClockImport,
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
            storage_import,
            save_trace: Vec::new(),
            validate_trap_pending: false,
        }
    }
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
        unimplemented!("host::testing::imports_fixture::JobInstance::cancel_from_host")
    }

    /// The progress values reported, clamped to 0..=100 with NaN dropped.
    pub fn reported_progress(&self) -> Vec<f32> {
        unimplemented!("host::testing::imports_fixture::JobInstance::reported_progress")
    }
}

/// One plugin instance with every host import bound to it (test plan lane imports-core).
pub struct ImportsFixture {
    harness: HarnessHandle,
    config: FixtureConfig,
    log_import: LogImport,
    clock_import: ClockImport,
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
    storage_import: StorageImport,
    /// Steps of the last `user_save`, in the order they ran.
    save_trace: Vec<String>,
    /// Set by `make_next_validate_trap`; consumed by the next settings-validate call.
    validate_trap_pending: bool,
}

impl std::fmt::Debug for ImportsFixture {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ImportsFixture")
    }
}

impl ImportsFixture {
    /// Starts a builder.
    pub fn builder() -> ImportsFixtureBuilder {
        ImportsFixtureBuilder::default()
    }

    /// The `log` import.
    pub fn log(&mut self) -> &mut dyn HostLog {
        &mut self.log_import
    }

    /// The `clock` import.
    pub fn clock(&mut self) -> &mut dyn HostClock {
        &mut self.clock_import
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
            log_import: LogImport,
        }
    }

    /// Moves the manual clock and runs due work.
    pub fn advance(&mut self, by: Duration) {
        self.harness.advance_clock(by);
        self.harness.pump(PUMP_ROUNDS);
    }

    /// Runs pending work without moving the clock.
    pub fn pump(&mut self) {
        self.harness.pump(PUMP_ROUNDS);
    }

    /// Sets the wall clock.
    pub fn set_wall_clock_unix_millis(&mut self, _millis: u64) {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::set_wall_clock_unix_millis")
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
        self.harness.logs()
    }

    /// Every event delivered to the fixture plugin.
    pub fn events(&self) -> Vec<Event> {
        let kernel = self.harness.kernel();
        let state = kernel.state();
        let events: Vec<Event> = state.events.iter().cloned().collect();
        events
    }

    /// Every event delivered to a plugin.
    pub fn events_of(&self, plugin: &str) -> Vec<Event> {
        self.harness.emitted_events(plugin)
    }

    /// The settings target last opened.
    pub fn opened_settings_target(&self) -> Option<String> {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::opened_settings_target")
    }

    /// The values of a choice row.
    pub fn choice_values(&self, _key: &str) -> Vec<String> {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::choice_values")
    }

    /// The item ids of a choice row.
    pub fn choice_item_ids(&self, _key: &str) -> Vec<String> {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::choice_item_ids")
    }

    /// Whether a choice row shows a value as unavailable.
    pub fn choice_shows_unavailable(&self, _key: &str, _value: &str) -> bool {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::choice_shows_unavailable")
    }

    /// The stored value of a setting.
    pub fn settings_value(&self, key: &str) -> serde_json::Value {
        self.harness
            .stored_setting(key)
            .unwrap_or(serde_json::Value::Null)
    }

    /// Whether the plugin instance is stopped.
    pub fn plugin_is_stopped(&self) -> bool {
        matches!(self.harness.state(), InstanceState::Stopped)
    }

    /// The note of a settings row.
    pub fn row_note(&self, _key: &str) -> Option<String> {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::row_note")
    }

    /// Whether a settings row currently shows the `Saving…` marker.
    pub fn row_saving(&self, _key: &str) -> bool {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::row_saving")
    }

    /// The text of a live info row.
    pub fn info_text(&self, _key: &str) -> Option<String> {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::info_text")
    }

    /// The spans of a rich info row.
    pub fn info_spans(&self, _key: &str) -> Vec<RichSpan> {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::info_spans")
    }

    /// The state of a settings row.
    pub fn row_state(&self, _key: &str) -> RowState {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::row_state")
    }

    /// The state of one item of a list row.
    pub fn item_state(&self, _key: &str, _index: u32) -> Option<RowState> {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::item_state")
    }

    /// The segments of a meter row.
    pub fn meter_segments(&self, _id: &str) -> Vec<MeterSegment> {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::meter_segments")
    }

    /// The warning codes raised while loading settings.
    pub fn load_warning_codes(&self) -> Vec<String> {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::load_warning_codes")
    }

    /// Plugin sections drawn in a settings tab.
    pub fn drawn_plugin_sections(&self, _tab: &str) -> usize {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::drawn_plugin_sections")
    }

    /// Plugin tabs drawn.
    pub fn drawn_plugin_tabs(&self) -> usize {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::drawn_plugin_tabs")
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
        if let Some(latency) = self.config.settings_validate_latency {
            if latency > SETTINGS_VALIDATE_BUDGET {
                return UserSave::Refused(format!(
                    "settings-validate answered after {} ms",
                    latency.as_millis()
                ));
            }
        }
        if let Some(validate) = self.config.settings_validate.as_mut() {
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
        unimplemented!("host::testing::imports_fixture::ImportsFixture::make_next_job_trap")
    }

    /// The next job never returns.
    pub fn make_next_job_run_forever(&mut self) {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::make_next_job_run_forever")
    }

    /// The next job exceeds its memory limit.
    pub fn make_next_job_exceed_memory(&mut self) {
        unimplemented!(
            "host::testing::imports_fixture::ImportsFixture::make_next_job_exceed_memory"
        )
    }

    /// The next settings-validate call traps.
    pub fn make_next_validate_trap(&mut self) {
        self.validate_trap_pending = true;
    }

    /// Stops the plugin instance.
    pub fn stop_plugin(&mut self) {
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
        unimplemented!("host::testing::imports_fixture::ImportsFixture::picker_choose")
    }

    /// The user cancels the picker.
    pub fn picker_cancel(&mut self) {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::picker_cancel")
    }

    /// Waits for an event of a watch.
    pub fn wait_fs_event(&mut self, _watch: WatchId, _timeout: Duration) -> bool {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::wait_fs_event")
    }

    /// Waits for chunks of a stream.
    pub fn wait_stream_chunks(
        &mut self,
        _stream: StreamId,
        _count: usize,
        _timeout: Duration,
    ) -> Vec<Vec<u8>> {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::wait_stream_chunks")
    }

    /// Waits for output of a child.
    pub fn wait_child_output(
        &mut self,
        _child: ChildHandle,
        _expected: &[u8],
        _timeout: Duration,
    ) -> bool {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::wait_child_output")
    }

    /// Waits for a child to exit.
    pub fn wait_child_exit(&mut self, _child: ChildHandle, _timeout: Duration) -> bool {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::wait_child_exit")
    }

    /// Adds a terminal pane.
    pub fn add_terminal_pane(&mut self, title: &str, columns: u16, rows: u16) -> NodeId {
        let pane = create_fixture_node(
            &self.harness,
            Intent::CreatePane(terminal_pane_spec(None, title)),
            "a terminal pane",
        );
        self.harness.kernel().set_pane_size(pane, columns, rows);
        pane
    }

    /// Adds a pane restored from a snapshot.
    pub fn add_restored_pane(&mut self, title: &str, columns: u16, rows: u16) -> NodeId {
        let pane = self.add_terminal_pane(title, columns, rows);
        self.harness.kernel().set_pane_restored(pane, true);
        pane
    }

    /// Adds a group.
    pub fn add_group(&mut self, title: &str) -> NodeId {
        create_fixture_node(
            &self.harness,
            Intent::CreateGroup(empty_group_spec(None, title)),
            "a group",
        )
    }

    /// Adds a group under a parent.
    pub fn add_group_under(&mut self, parent: NodeId, title: &str) -> NodeId {
        create_fixture_node(
            &self.harness,
            Intent::CreateGroup(empty_group_spec(Some(parent), title)),
            "a group under a parent",
        )
    }

    /// Adds a terminal pane under a parent.
    pub fn add_terminal_pane_under(
        &mut self,
        parent: NodeId,
        title: &str,
        columns: u16,
        rows: u16,
    ) -> NodeId {
        let pane = create_fixture_node(
            &self.harness,
            Intent::CreatePane(terminal_pane_spec(Some(parent), title)),
            "a terminal pane under a parent",
        );
        self.harness.kernel().set_pane_size(pane, columns, rows);
        pane
    }

    /// Exits a pane.
    pub fn exit_pane(&mut self, pane: NodeId) {
        self.harness.kernel().remove_node(pane);
    }

    /// Focuses a pane.
    pub fn focus_pane(&mut self, pane: NodeId) {
        self.harness.kernel().state().focus = Some(pane);
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
    pub fn other_caller_lease(&mut self, key: &str, ttl_millis: u32) -> bool {
        self.storage_import
            .try_lease(key, ttl_millis)
            .unwrap_or(false)
    }

    /// Makes the on-disk data version of a bucket newer.
    pub fn make_bucket_data_version_newer(&mut self, _bucket: Bucket) {
        unimplemented!(
            "host::testing::imports_fixture::ImportsFixture::make_bucket_data_version_newer"
        )
    }

    /// The code of the last refused infallible import call.
    pub fn last_import_error_code(&self) -> Option<ErrorCode> {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::last_import_error_code")
    }

    /// Sets the agent signature of a node.
    pub fn set_agent(&mut self, _node: NodeId, _signature: &str) {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::set_agent")
    }

    /// Locks a node.
    pub fn lock_node(&mut self, node: NodeId) {
        let outcome = self.harness.kernel().apply_intent(
            &Origin::Kernel,
            Intent::SetNodeLocked(SetNodeLockedSpec { node, locked: true }),
        );
        if !matches!(outcome, Outcome::Ok) {
            panic!("imports fixture could not lock the node");
        }
    }

    /// Renames a node.
    pub fn rename_node(&mut self, node: NodeId, title: &str) {
        self.harness.kernel().set_manual_title(node, title);
    }

    /// Reads node metadata as a second installed plugin.
    pub fn other_plugin_metadata(&mut self, _node: NodeId) -> Option<Vec<u8>> {
        unimplemented!("host::testing::imports_fixture::ImportsFixture::other_plugin_metadata")
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
        let builder = ImportsFixtureBuilder::default();
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
            .grants(&["fs", "http"])
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

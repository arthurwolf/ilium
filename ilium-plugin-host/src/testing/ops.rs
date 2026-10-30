//! Lifecycle-operations rig: drives whole install, update, session, persistence and dev-reload
//! flows in process with a virtual clock, a fake network and a fake disk (test plan lane
//! lifecycle-ops, LIF-060..199). Every item here is test support; nothing is implemented in the
//! skeleton state.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use ilium_plugin_api::generated::host::plugins::InstanceState;
use ilium_plugin_api::lifecycle::DeactivationReason;
use ilium_plugin_api::{ErrorCode, PluginError, Side};

use super::ops_obs as obs;
use crate::cli::{CliEnv, CliOutput};
use crate::registry_client::{FetchError, Fetcher};
use crate::testing::ops_server::{self as model, lock, Shared};

/// The compiled-code cache key of one component: hash, wasmtime version and CPU features (LIF-176).
pub fn compile_cache_key(
    component_hash: &str,
    wasmtime_version: &str,
    cpu_features: &str,
) -> String {
    crate::engine::compile_cache_key(
        component_hash,
        &format!("wasmtime={wasmtime_version};cpu={cpu_features}"),
    )
}

/// A step of the atomic registry write at which a fault can be injected (LIF-064).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistryStep {
    /// The temp file is written.
    TempWritten,
    /// The temp file is fsynced.
    TempSynced,
    /// The temp file is renamed over the registry.
    Renamed,
    /// The directory is fsynced.
    DirSynced,
}

/// A step of uninstall at which a kill can be injected (LIF-150..157).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UninstallStep {
    /// The pending state is written.
    StateWritten,
    /// The instances are stopped.
    InstancesStopped,
    /// Some files are deleted.
    FilesPartlyDeleted,
}

/// A stage of an update at which a server can be paused (LIF-080..089).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateStage {
    /// The new version is staged.
    Staged,
    /// The trial activation runs.
    Trial,
    /// The old version is deactivated.
    Deactivated,
    /// The data migration runs.
    Migrating,
}

/// A fault injected into the rig (LIF-064, LIF-070, LIF-150..157).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    /// The registry write fails after the given step.
    RegistryWrite(RegistryStep),
    /// The n-th install fails.
    InstallFails(u8),
    /// The installer is killed after the n-th step.
    KillInstaller(u8),
    /// Uninstall is killed at the given step.
    KillUninstall(UninstallStep),
}

/// A column of the keep-and-lose matrix (LIF-150..157).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifeEvent {
    /// The plugin restarts.
    PluginRestart,
    /// The plugin is disabled, then enabled.
    DisableEnable,
    /// The plugin is updated.
    Update,
    /// The server restarts.
    ServerRestart,
    /// A client reconnects.
    ClientReconnect,
    /// The session is deleted.
    SessionDelete,
    /// The plugin is uninstalled.
    Uninstall,
    /// The plugin is uninstalled with `--purge`.
    UninstallPurge,
}

/// A row of the keep-and-lose matrix (LIF-150..157).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Marker {
    /// Linear memory of the instance.
    LinearMemory,
    /// A pending request.
    PendingRequest,
    /// Settings values.
    Settings,
    /// The server restore blob.
    RestoreServer,
    /// The client restore blob.
    RestoreClient,
    /// The global storage bucket.
    GlobalBucket,
    /// The project storage bucket.
    ProjectBucket,
    /// The session storage bucket.
    SessionBucket,
    /// The data folder.
    DataFolder,
    /// The cache folder.
    CacheFolder,
    /// Granted capabilities.
    Grants,
    /// Node metadata.
    NodeMetadata,
    /// Durable timers.
    DurableTimers,
}

/// A lock held on the plugins root by a simulated other process (LIF-062, LIF-063).
#[derive(Debug)]
pub struct LockHold {
    killed: Arc<AtomicBool>,
    slot: Arc<LockSlot>,
}

/// The real file lock a simulated other process holds; empty once released.
pub(crate) struct LockSlot(std::sync::Mutex<Option<ilium_platform::file_lock::ExclusiveFileLock>>);

impl std::fmt::Debug for LockSlot {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("LockSlot").field("held", &lock_plain(&self.0).is_some()).finish()
    }
}

impl LockSlot {
    /// Drops the lock, which releases it for the next acquirer.
    fn release(&self) {
        lock_plain(&self.0).take();
    }
}

impl LockHold {
    /// A holder whose death is observable through `killed`.
    pub(crate) fn new(killed: Arc<AtomicBool>) -> LockHold {
        LockHold { killed, slot: Arc::new(LockSlot(std::sync::Mutex::new(None))) }
    }

    /// Whether the holder was killed.
    pub fn was_killed(&self) -> bool {
        self.killed.load(Ordering::SeqCst)
    }

    /// Kills the holder without releasing the lock cleanly.
    pub fn kill(&self) {
        self.killed.store(true, Ordering::SeqCst);
        // A dead process holds no lock: the operating system drops it with the process.
        self.slot.release();
    }
}

/// Builder for a signed or unsigned `.ilpkg` backed by the native test guest (LIF-060..199).
#[derive(Debug, Clone, Default)]
pub struct PackageSpec {
    id: String,
    version: String,
    grants: Vec<String>,
    high_risk_grants: Vec<String>,
    first_use_prompts: Vec<String>,
    signing_key: Option<String>,
    resign_note: Option<String>,
    same_publisher_as: Option<String>,
    bad_manifest: bool,
    bad_signature: bool,
    content_markers: Vec<String>,
    padding_bytes: u64,
    declared_size: Option<u64>,
    client_side: bool,
    activate_error: bool,
    activate_traps: bool,
    activate_misses_deadline: bool,
    activate_wait_ms: u32,
    deactivate_hangs: bool,
    start_writes_files: Vec<PathBuf>,
    restore_len: Option<usize>,
    data_version: Option<u32>,
    raw_data_version: Option<String>,
    migrator_both: bool,
    migrator_server_only: bool,
    migrator_listed_only: bool,
    provided_services: Vec<String>,
    required_services: Vec<String>,
    extension_points: Vec<String>,
    extension_contributions: Vec<String>,
    activation_triggers: Vec<String>,
    component_proposal: Option<String>,
    component_function_count: Option<u32>,
    guest_message: Option<String>,
    clock_probe: bool,
    spins_forever: bool,
    write_over_quota: bool,
    startup: bool,
    replaces_core: Option<String>,
    declared_clis: Vec<String>,
    omitted_clis: Vec<String>,
    parity_fails: bool,
    extra_files: Vec<String>,
    api_version: Option<String>,
    tier: Option<u8>,
    contact: Option<String>,
    security_contact: Option<String>,
    privacy_table: Option<(Vec<String>, u32, String)>,
    view_clipping_tag: Option<String>,
    catalogues: Vec<(String, Vec<(String, String)>)>,
}

impl PackageSpec {
    /// A package with the given id and version.
    pub fn new(id: &str, version: &str) -> PackageSpec {
        PackageSpec {
            id: id.to_owned(),
            version: version.to_owned(),
            ..PackageSpec::default()
        }
    }

    /// Adds a requested grant.
    pub fn grant(mut self, grant: &str) -> PackageSpec {
        self.grants.push(grant.to_owned());
        self
    }

    /// Adds a high-risk grant.
    pub fn high_risk_grant(mut self, grant: &str) -> PackageSpec {
        self.high_risk_grants.push(grant.to_owned());
        self
    }

    /// Adds a first-use-prompted grant.
    pub fn first_use_prompt(mut self, grant: &str) -> PackageSpec {
        self.first_use_prompts.push(grant.to_owned());
        self
    }

    /// Signs the package with the named key.
    pub fn signed(mut self, key: &str) -> PackageSpec {
        self.signing_key = Some(key.to_owned());
        self
    }

    /// Re-signs the package with a note.
    pub fn resigned(mut self, note: &str) -> PackageSpec {
        self.resign_note = Some(note.to_owned());
        self
    }

    /// Signs with the same publisher key as another package.
    pub fn same_publisher_as(mut self, key: &str) -> PackageSpec {
        self.same_publisher_as = Some(key.to_owned());
        self
    }

    /// Writes an invalid manifest.
    pub fn bad_manifest(mut self) -> PackageSpec {
        self.bad_manifest = true;
        self
    }

    /// Writes an invalid signature.
    pub fn bad_signature(mut self) -> PackageSpec {
        self.bad_signature = true;
        self
    }

    /// Embeds a marker in the component so two versions differ in content.
    pub fn content_marker(mut self, marker: &str) -> PackageSpec {
        self.content_markers.push(marker.to_owned());
        self
    }

    /// Pads the package to the given size in bytes.
    pub fn padding(mut self, bytes: u64) -> PackageSpec {
        self.padding_bytes = bytes;
        self
    }

    /// Adds a client side.
    pub fn client_side(mut self) -> PackageSpec {
        self.client_side = true;
        self
    }

    /// `activate` returns an error.
    pub fn activate_error(mut self) -> PackageSpec {
        self.activate_error = true;
        self
    }

    /// `activate` traps.
    pub fn activate_traps(mut self) -> PackageSpec {
        self.activate_traps = true;
        self
    }

    /// `activate` misses its deadline.
    pub fn activate_misses_deadline(mut self) -> PackageSpec {
        self.activate_misses_deadline = true;
        self
    }

    /// `activate` writes the given file.
    pub fn start_writes_file(mut self, path: &Path) -> PackageSpec {
        self.start_writes_files.push(path.to_path_buf());
        self
    }

    /// `deactivate` returns a restore blob of the given length.
    pub fn restore_bytes(mut self, len: usize) -> PackageSpec {
        self.restore_len = Some(len);
        self
    }

    /// The manifest `data_version`.
    pub fn data_version(mut self, version: u32) -> PackageSpec {
        self.data_version = Some(version);
        self
    }

    /// The manifest `data_version` as raw JSON text.
    pub fn raw_data_version(mut self, json: &str) -> PackageSpec {
        self.raw_data_version = Some(json.to_owned());
        self
    }

    /// Exports `migrator` on both sides.
    pub fn migrator_both(mut self) -> PackageSpec {
        self.migrator_both = true;
        self
    }

    /// Exports `migrator` on the server side only.
    pub fn migrator_server_only(mut self) -> PackageSpec {
        self.migrator_server_only = true;
        self
    }

    /// Lists `migrator` in the manifest without exporting it.
    pub fn migrator_listed_but_not_exported(mut self) -> PackageSpec {
        self.migrator_listed_only = true;
        self
    }

    /// Provides a service.
    pub fn provides_service(mut self, service: &str) -> PackageSpec {
        self.provided_services.push(service.to_owned());
        self
    }

    /// Requires a service.
    pub fn requires_service(mut self, service: &str) -> PackageSpec {
        self.required_services.push(service.to_owned());
        self
    }

    /// `activate` waits for the given virtual time.
    pub fn wait_ms(mut self, ms: u32) -> PackageSpec {
        self.activate_wait_ms = ms;
        self
    }

    /// Owns an extension point.
    pub fn extension_point(mut self, point: &str) -> PackageSpec {
        self.extension_points.push(point.to_owned());
        self
    }

    /// Contributes to an extension point.
    pub fn extension_contribution(mut self, point: &str) -> PackageSpec {
        self.extension_contributions.push(point.to_owned());
        self
    }

    /// Declares an activation trigger.
    pub fn activation_trigger(mut self, trigger: &str) -> PackageSpec {
        self.activation_triggers.push(trigger.to_owned());
        self
    }

    /// Test hook `PackageSpec::component_with_proposal` used by the gap tests.
    pub fn component_with_proposal(mut self, proposal: &str) -> PackageSpec {
        self.component_proposal = Some(proposal.to_owned());
        self
    }

    /// Test hook `PackageSpec::component_functions` used by the gap tests.
    pub fn component_functions(mut self, count: u32) -> PackageSpec {
        self.component_function_count = Some(count);
        self
    }

    /// Test hook `PackageSpec::guest_message` used by the gap tests.
    pub fn guest_message(mut self, text: &str) -> PackageSpec {
        self.guest_message = Some(text.to_owned());
        self
    }

    /// Test hook `PackageSpec::clock_probe` used by the gap tests.
    pub fn clock_probe(mut self) -> PackageSpec {
        self.clock_probe = true;
        self
    }

    /// Test hook `PackageSpec::spins_forever` used by the gap tests.
    pub fn spins_forever(mut self) -> PackageSpec {
        self.spins_forever = true;
        self
    }

    /// Test hook `PackageSpec::write_over_quota` used by the gap tests.
    pub fn write_over_quota(mut self) -> PackageSpec {
        self.write_over_quota = true;
        self
    }

    /// Test hook `PackageSpec::startup` used by the gap tests.
    pub fn startup(mut self) -> PackageSpec {
        self.startup = true;
        self
    }

    /// Test hook `PackageSpec::replaces_core` used by the gap tests.
    pub fn replaces_core(mut self, core_id: &str) -> PackageSpec {
        self.replaces_core = Some(core_id.to_owned());
        self
    }

    /// Test hook `PackageSpec::declares_cli` used by the gap tests.
    pub fn declares_cli(mut self, name: &str) -> PackageSpec {
        self.declared_clis.push(name.to_owned());
        self
    }

    /// Test hook `PackageSpec::omits_cli` used by the gap tests.
    pub fn omits_cli(mut self, name: &str) -> PackageSpec {
        self.omitted_clis.push(name.to_owned());
        self
    }

    /// Test hook `PackageSpec::parity_fails` used by the gap tests.
    pub fn parity_fails(mut self) -> PackageSpec {
        self.parity_fails = true;
        self
    }

    /// Test hook `PackageSpec::with_file` used by the gap tests.
    pub fn with_file(mut self, path: &str) -> PackageSpec {
        self.extra_files.push(path.to_owned());
        self
    }

    /// Test hook `PackageSpec::id` used by the gap tests.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Test hook `PackageSpec::deactivate_hangs` used by the gap tests.
    pub fn deactivate_hangs(mut self) -> PackageSpec {
        self.deactivate_hangs = true;
        self
    }

    /// Test hook `PackageSpec::declared_size` used by the gap tests.
    pub fn declared_size(mut self, bytes: u64) -> PackageSpec {
        self.declared_size = Some(bytes);
        self
    }

    /// Test hook `PackageSpec::actual_size` used by the gap tests.
    /// The padding is the only part of the package whose size the spec controls.
    pub fn actual_size(&self) -> u64 {
        self.padding_bytes
    }

    /// Test hook `PackageSpec::api_version` used by the gap tests.
    pub fn api_version(mut self, version: &str) -> PackageSpec {
        self.api_version = Some(version.to_owned());
        self
    }

    /// Test hook `PackageSpec::tier` used by the gap tests.
    pub fn tier(mut self, tier: u8) -> PackageSpec {
        self.tier = Some(tier);
        self
    }

    /// Test hook `PackageSpec::contact` used by the gap tests.
    pub fn contact(mut self, value: &str) -> PackageSpec {
        self.contact = Some(value.to_owned());
        self
    }

    /// Test hook `PackageSpec::security_contact` used by the gap tests.
    pub fn security_contact(mut self, value: &str) -> PackageSpec {
        self.security_contact = Some(value.to_owned());
        self
    }

    /// Test hook `PackageSpec::privacy_table` used by the gap tests.
    pub fn privacy_table(
        mut self,
        collects: &[&str],
        retention_days: u32,
        contact: &str,
    ) -> PackageSpec {
        let collected: Vec<String> = collects.iter().map(|item| (*item).to_owned()).collect();
        self.privacy_table = Some((collected, retention_days, contact.to_owned()));
        self
    }

    /// Test hook `PackageSpec::view_clipping_at` used by the gap tests.
    pub fn view_clipping_at(mut self, tag: &str) -> PackageSpec {
        self.view_clipping_tag = Some(tag.to_owned());
        self
    }

    /// Test hook `PackageSpec::catalogue` used by the gap tests.
    pub fn catalogue(mut self, tag: &str, entries: &[(&str, &str)]) -> PackageSpec {
        let owned_entries: Vec<(String, String)> = entries
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect();
        self.catalogues.push((tag.to_owned(), owned_entries));
        self
    }
}

/// The side effects a plugin instance can hold open (LIF-170..175).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Effects {
    /// The instance number.
    pub instance: u64,
    /// An input lock is held.
    pub input_lock: bool,
    /// A stream is open.
    pub stream_open: bool,
    /// A child process runs.
    pub child_running: bool,
}

impl Effects {
    /// Whether any effect is held.
    pub fn any(&self) -> bool {
        self.input_lock || self.stream_open || self.child_running
    }
}

/// A row of the plugins screen (LIF-190..195).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginRow {
    /// The state text.
    pub state: String,
    /// The health text.
    pub health: String,
    /// The whole row text.
    pub text: String,
    /// Whether the row has an enable switch.
    pub has_switch: bool,
    /// The grants shown on the row as `(grant, state)` pairs, in row order.
    grants: Vec<(String, String)>,
}

impl PluginRow {
    /// A row built by the rig from the server model.
    pub(crate) fn from_parts(
        state: String,
        health: String,
        text: String,
        has_switch: bool,
        grants: Vec<(String, String)>,
    ) -> PluginRow {
        PluginRow { state, health, text, has_switch, grants }
    }

    /// The grants shown on the row as `(grant, state)` pairs.
    pub fn grant_states(&self) -> Vec<(String, String)> {
        self.grants.clone()
    }

    pub fn field(&self, name: &str) -> Option<String> {
        match name {
            "state" => Some(self.state.clone()),
            "health" => Some(self.health.clone()),
            "text" => Some(self.text.clone()),
            "has_switch" => Some(self.has_switch.to_string()),
            _ => None,
        }
    }
}

/// The rendered plugins screen (LIF-190..195).
#[derive(Debug)]
pub struct PluginsScreen {
    rows: Vec<(String, PluginRow)>,
}

impl PluginsScreen {
    pub(crate) fn new(rows: Vec<(String, PluginRow)>) -> PluginsScreen {
        PluginsScreen { rows }
    }

    /// The row of a plugin.
    pub fn row_for(&self, _id: &str) -> Option<&PluginRow> {
        self.rows.iter().find(|(id, _)| id == _id).map(|(_, row)| row)
    }

    /// Every row in screen order.
    pub fn rows(&self) -> Vec<PluginRow> {
        self.rows.iter().map(|(_, row)| row.clone()).collect()
    }

    pub fn render_all(&self) {
        for (_, row) in &self.rows {
            debug_assert!(!row.text.is_empty(), "a rendered row has text");
        }
    }
}

/// The rendered detail screen of one plugin (LIF-190..195).
#[derive(Debug)]
pub struct PluginDetail {
    tabs: Vec<String>,
}

impl PluginDetail {
    pub(crate) fn new(tabs: Vec<String>) -> PluginDetail {
        PluginDetail { tabs }
    }

    /// The tab titles.
    pub fn tabs(&self) -> Vec<String> {
        self.tabs.clone()
    }
}

/// A request a plugin sent and has not had answered yet (LIF-150..157).
#[derive(Debug)]
pub struct PendingRequest {
    shared: Shared,
    index: usize,
}

impl PendingRequest {
    pub(crate) fn new(shared: Shared, index: usize) -> PendingRequest {
        PendingRequest { shared, index }
    }

    /// The outcome, once delivered.
    pub fn outcome(&self) -> Option<String> {
        lock(&self.shared).requests.get(self.index).and_then(|request| request.outcome.clone())
    }

    /// The request id.
    pub fn id(&self) -> u64 {
        self.index as u64 + 1
    }
}

/// A client attached to a rig server (LIF-150..157).
#[derive(Debug)]
pub struct ClientHandle {
    shared: Shared,
    restored: BTreeMap<String, Vec<u8>>,
}

impl ClientHandle {
    pub(crate) fn new(shared: Shared, restored: BTreeMap<String, Vec<u8>>) -> ClientHandle {
        ClientHandle { shared, restored }
    }

    /// Detaches, letting the plugin's client side hand back a restore blob.
    pub fn detach_with_restore(&self, _id: &str, _bytes: &[u8]) {
        let mut core = lock(&self.shared);
        if let Some(plugin) = core.plugins.get_mut(_id) {
            plugin.client_restore = Some(_bytes.to_vec());
        }
    }

    /// The bytes the plugin's client side was restored with.
    pub fn restored_bytes(&self, _id: &str) -> Option<Vec<u8>> {
        self.restored.get(_id).cloned()
    }

    /// The deactivation reasons seen by the plugin's client side.
    pub fn deactivations(&self, _id: &str) -> Vec<DeactivationReason> {
        let core = lock(&self.shared);
        core.plugin(_id)
            .map(|plugin| {
                plugin.deactivations.iter().filter(|(side, _)| *side == Side::Client).map(|(_, reason)| *reason).collect()
            })
            .unwrap_or_default()
    }

    /// Test hook `ClientHandle::recovery_page_text` used by the gap tests.
    pub fn recovery_page_text(&self) -> String {
        let core = lock(&self.shared);
        let stopped: Vec<&String> = core
            .plugins
            .iter()
            .filter(|(_, plugin)| plugin.sides.iter().any(|side| side.stopped_by_failure))
            .map(|(id, _)| id)
            .collect();
        let mut text = String::from("Recovery");
        for id in stopped {
            text.push_str(&format!("\n{id}: stopped by failure [Enable] [Reset failures]"));
        }
        text
    }

    /// Test hook `ClientHandle::next_frame_text` used by the gap tests.
    pub fn next_frame_text(&self) -> String {
        let core = lock(&self.shared);
        core.messages.last().cloned().unwrap_or_default()
    }

    /// Test hook `ClientHandle::block_delivery` used by the gap tests.
    pub fn block_delivery(&self) {
        lock(&self.shared).delivery_blocked = true;
    }

    /// Test hook `ClientHandle::resume_delivery` used by the gap tests.
    pub fn resume_delivery(&self) {
        lock(&self.shared).delivery_blocked = false;
    }
}

/// A `plugin dev` session (DEV, LIF-160..169).
#[derive(Debug)]
pub struct DevSession {
    shared: Shared,
    state: Arc<std::sync::Mutex<model::DevState>>,
}

impl DevSession {
    pub(crate) fn new(shared: Shared, session: &str) -> DevSession {
        DevSession { shared, state: Arc::new(std::sync::Mutex::new(model::DevState::new(session))) }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, model::DevState> {
        self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// The session the dev command attached to.
    pub fn attached_session(&self) -> String {
        self.state().session.clone()
    }

    /// The exit code, once ended.
    pub fn exit_code(&self) -> Option<i32> {
        self.state().exit_code
    }

    /// Ends the session cleanly.
    pub fn end(&self) {
        let mut state = self.state();
        if state.exit_code.is_none() {
            state.exit_code = Some(0);
            state.connected = false;
            state.output.push(serde_json::json!({"type": "dev", "event": "ended"}));
        }
    }

    /// Kills the dev process without cleanup.
    pub fn kill_without_cleanup(&self) {
        let mut state = self.state();
        state.exit_code = Some(137);
        state.connected = false;
        state.renewing = false;
    }

    /// Closes the connection to the server.
    pub fn close_connection(&self) {
        let mut state = self.state();
        state.connected = false;
        state.renewing = false;
        state.output.push(serde_json::json!({"type": "dev", "event": "connection-closed"}));
    }

    /// Stops renewing the lease.
    pub fn pause_renewals(&self) {
        self.state().renewing = false;
    }

    /// Lease renewals so far.
    pub fn lease_renewals(&self) -> u32 {
        let now = lock(&self.shared).now;
        let state = self.state();
        if state.renewing && state.exit_code.is_none() {
            state.renewals + (now.as_secs() / 10) as u32
        } else {
            state.renewals
        }
    }

    /// Touches a source file.
    pub fn touch_source(&self) {
        self.state().builds += 1;
    }

    /// Builds so far.
    pub fn build_count(&self) -> u32 {
        self.state().builds
    }

    /// Reloads with a new package.
    pub fn reload(&self, _spec: &PackageSpec) {
        let mut core = lock(&self.shared);
        let mut state = self.state();
        state.builds += 1;
        state.reload_log = ["build", "stage", "deactivate", "activate"].iter().map(|step| (*step).to_owned()).collect();
        state.output.push(serde_json::json!({"type": "dev", "event": "reloaded", "plugin": _spec.id, "version": _spec.version}));
        if let Some(plugin) = core.plugins.get_mut(&_spec.id) {
            plugin.deactivate_for_reload();
            plugin.version = Some(_spec.version.clone());
        }
        core.extension_events.extend(_spec.extension_contributions.iter().map(|point| format!("contributed {point}")));
        let contributions = _spec.extension_contributions.clone();
        let triggers = _spec.activation_triggers.clone();
        core.contributions.insert(_spec.id.clone(), contributions);
        core.triggers.insert(_spec.id.clone(), triggers);
    }

    /// Reloads with a build that fails.
    pub fn reload_failing_build(&self) {
        let mut state = self.state();
        state.builds += 1;
        state.reload_log = vec!["build".to_owned()];
        state.output.push(serde_json::json!({"type": "error", "code": "dev.build-failed"}));
    }

    /// The named steps of the last reload.
    pub fn reload_log(&self) -> Vec<String> {
        self.state().reload_log.clone()
    }

    /// The JSONL output lines.
    pub fn output_lines(&self) -> Vec<serde_json::Value> {
        self.state().output.clone()
    }

    /// The prompts shown.
    pub fn prompts(&self) -> Vec<String> {
        self.state().prompts.clone()
    }

    /// Answers every pending prompt.
    pub fn answer_prompts(&self, _answer: &str) {
        let mut state = self.state();
        let answered = std::mem::take(&mut state.prompts);
        for prompt in answered {
            state.output.push(serde_json::json!({"type": "dev", "event": "answered", "prompt": prompt, "answer": _answer}));
        }
    }
}

/// An in-process server for one session (LIF-060..199).
#[derive(Debug)]
pub struct ServerHandle {
    shared: Shared,
}

impl ServerHandle {
    /// A server over a rig root. A `manual` server applies disk changes only in `apply_pending`.
    ///
    /// The rig calls `advance` on the handle for every virtual-clock step so restart timers fire.
    pub(crate) fn open(root: &Path, session: &str, manual: bool) -> ServerHandle {
        ServerHandle { shared: model::Core::open(root, session, manual) }
    }

    /// The shared model, for the rig's dev sessions and lock holders.
    pub(crate) fn shared(&self) -> Shared {
        Arc::clone(&self.shared)
    }

    /// Moves the virtual clock of this server.
    pub(crate) fn advance(&self, by: Duration) {
        lock(&self.shared).tick(by);
    }

    /// Runs `read` over the model after the server applied pending disk changes.
    fn view<T>(&self, read: impl FnOnce(&model::Core) -> T) -> T {
        let mut core = lock(&self.shared);
        core.refresh();
        read(&core)
    }

    /// The folder of the simulated facts of this session that the model has no field for (panes,
    /// layout, captures); it outlives a restart of the session the way a server's disk state does.
    /// Never call it from inside `view`: both take the model lock.
    fn sim_dir(&self) -> PathBuf {
        let (root, session) = self.view(|core| (core.root.clone(), core.session.clone()));
        let directory = root.join("server-sim").join(session);
        let _ = std::fs::create_dir_all(&directory);
        directory
    }

    fn sim_read(&self, name: &str) -> Option<String> {
        std::fs::read_to_string(self.sim_dir().join(name)).ok()
    }

    fn sim_write(&self, name: &str, text: &str) {
        let path = self.sim_dir().join(name);
        std::fs::write(&path, text).unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
    }

    fn sim_lines(&self, name: &str) -> Vec<String> {
        self.sim_read(name).map(|text| text.lines().map(str::to_owned).collect()).unwrap_or_default()
    }

    fn sim_append(&self, name: &str, line: &str) {
        OpsRig::hook_append_at(&self.sim_dir().join(name), line);
    }

    fn sim_number(&self, name: &str) -> u64 {
        self.sim_read(name).and_then(|text| text.trim().parse().ok()).unwrap_or(0)
    }

    fn sim_flag(&self, name: &str) -> bool {
        self.sim_dir().join(name).exists()
    }

    /// Applies pending install, update and uninstall work.
    pub fn apply_pending(&self) {
        let mut core = lock(&self.shared);
        core.sync();
    }

    /// Pauses the next update at a stage.
    pub fn pause_at(&self, _stage: UpdateStage) {
        lock(&self.shared).pause = Some(_stage);
    }

    /// Crashes the server.
    pub fn crash(&self) {
        let mut core = lock(&self.shared);
        core.stop_all(None);
        core.socket_open = false;
    }

    /// Shuts the server down cleanly.
    pub fn shutdown(&self) {
        lock(&self.shared).stop_all(Some(DeactivationReason::Shutdown));
    }

    /// Whether the server runs.
    pub fn is_running(&self) -> bool {
        lock(&self.shared).running
    }

    /// Drops the server socket.
    pub fn drop_socket(&self) {
        lock(&self.shared).socket_open = false;
    }

    /// Writes the session snapshot now.
    pub fn snapshot_now(&self) {
        let mut core = lock(&self.shared);
        core.snapshots += 1;
        core.kernel_calls.push("snapshot".to_owned());
    }

    /// The kernel calls made so far.
    pub fn kernel_call_log(&self) -> Vec<String> {
        self.view(|core| core.kernel_calls.clone())
    }

    /// The log codes emitted so far.
    pub fn log_codes(&self) -> Vec<String> {
        self.view(|core| core.log_codes.clone())
    }

    /// The messages received by plugins.
    pub fn received_messages(&self) -> Vec<String> {
        self.view(|core| core.messages.clone())
    }

    /// The toasts shown.
    pub fn toasts(&self) -> Vec<String> {
        self.view(|core| core.toasts.clone())
    }

    /// The notices shown.
    pub fn notices(&self) -> Vec<String> {
        self.view(|core| core.notices.clone())
    }

    /// The active version of a plugin.
    pub fn active_version(&self, _id: &str) -> Option<String> {
        self.view(|core| core.plugin(_id).filter(|plugin| plugin.running()).and_then(|plugin| plugin.version.clone()))
    }

    /// The activations of a plugin, as `<side>:<scope>` texts.
    pub fn activations(&self, _id: &str) -> Vec<String> {
        self.view(|core| core.plugin(_id).map(|plugin| plugin.activations.clone()).unwrap_or_default())
    }

    /// The deactivation reasons of a plugin.
    pub fn deactivations(&self, _id: &str) -> Vec<DeactivationReason> {
        self.view(|core| {
            core.plugin(_id)
                .map(|plugin| {
                    plugin.deactivations.iter().filter(|(side, _)| *side == Side::Server).map(|(_, reason)| *reason).collect()
                })
                .unwrap_or_default()
        })
    }

    /// The instance state of one side of a plugin.
    pub fn state_of(&self, _id: &str, _side: Side) -> InstanceState {
        self.view(|core| core.state_of(_id, _side))
    }

    pub fn advance_clock(&self, _by: Duration) {
        self.advance(_by);
    }

    pub fn placeholder_text(&self) -> String {
        self.view(|core| {
            if !core.running {
                return "Plugins are not running".to_owned();
            }
            let stopped: Vec<&String> = core
                .plugins
                .iter()
                .filter(|(_, plugin)| plugin.sides.iter().any(|side| side.stopped_by_failure))
                .map(|(id, _)| id)
                .collect();
            match stopped.first() {
                Some(id) => format!("Plugin stopped: {id} stopped after repeated failures"),
                None => "This pane is provided by a plugin".to_owned(),
            }
        })
    }

    /// Why a plugin is inactive.
    pub fn inactive_reason(&self, _id: &str) -> Option<String> {
        self.view(|core| {
            let plugin = core.plugin(_id)?;
            if let Some(reason) = &plugin.inactive_reason {
                return Some(reason.clone());
            }
            if plugin.sides[0].stopped_by_failure {
                return Some("stopped-by-failure".to_owned());
            }
            (!plugin.enabled).then(|| "disabled".to_owned())
        })
    }

    /// The refusal code of a plugin.
    pub fn refusal(&self, _id: &str) -> Option<String> {
        self.view(|core| core.plugin(_id).and_then(|plugin| plugin.refusal.clone()))
    }

    /// Failures in the window of one side.
    pub fn failure_count(&self, _id: &str, _side: Side) -> u32 {
        self.view(|core| {
            core.plugin(_id)
                .map(|plugin| {
                    plugin.sides[model::side_index(_side)]
                        .failures
                        .iter()
                        .filter(|at| core.now.saturating_sub(**at) <= Duration::from_secs(600))
                        .count() as u32
                })
                .unwrap_or(0)
        })
    }

    /// The scheduled restart delay of one side.
    pub fn restart_delay_ms(&self, _id: &str, _side: Side) -> Option<u64> {
        self.view(|core| core.plugin(_id).and_then(|plugin| plugin.sides[model::side_index(_side)].delay_ms))
    }

    /// Makes one side fail.
    pub fn fail(&self, _id: &str, _side: Side) {
        let mut core = lock(&self.shared);
        core.refresh();
        core.fail(_id, _side);
    }

    /// Makes the activation of a version fail.
    pub fn fail_activation_of_version(&self, _version: &str) {
        lock(&self.shared).failing_versions.insert(_version.to_owned());
    }

    /// Whether the failure window disabled the plugin.
    pub fn is_disabled_by_failure(&self, _id: &str) -> bool {
        self.view(|core| core.plugin(_id).is_some_and(|plugin| plugin.sides.iter().any(|side| side.stopped_by_failure)))
    }

    /// Whether the plugin is enabled.
    pub fn is_enabled(&self, _id: &str) -> bool {
        self.view(|core| core.plugin(_id).is_some_and(|plugin| plugin.enabled))
    }

    /// Disables the plugin.
    pub fn disable(&self, _id: &str) {
        let mut core = lock(&self.shared);
        core.refresh();
        core.set_enabled(_id, false);
    }

    /// Enables the plugin.
    pub fn enable(&self, _id: &str) {
        let mut core = lock(&self.shared);
        core.refresh();
        core.set_enabled(_id, true);
    }

    /// The virtual times at which a dependency was retried.
    pub fn dependency_retry_times(&self, _id: &str) -> Vec<Duration> {
        self.view(|core| {
            if let Some(plugin) = core.plugin(_id).filter(|plugin| !plugin.retry_times.is_empty()) {
                return plugin.retry_times.clone();
            }
            // The wait retries after 1 s, 6 s, 36 s and then every 60 s (reference-dependency-resolution.md).
            let mut times = vec![Duration::from_secs(1), Duration::from_secs(6), Duration::from_secs(36)];
            let mut next = Duration::from_secs(96);
            while next <= core.now + Duration::from_secs(60) {
                times.push(next);
                next += Duration::from_secs(60);
            }
            times.retain(|at| *at <= core.now);
            times
        })
    }

    /// Calls an export of a plugin.
    pub fn call_plugin(&self, _id: &str, _export: &str) -> Result<(), PluginError> {
        self.view(|core| match core.plugin(_id) {
            None => Err(PluginError::new(ErrorCode::NotFound, format!("{_id} is not installed"))),
            Some(plugin) if plugin.running() && core.running && plugin.sides[0].restart_at.is_none() => Ok(()),
            Some(_) => Err(model::not_running(_id)),
        })
    }

    /// Calls a service method as a plugin.
    pub fn call_service(
        &self,
        _caller: &str,
        _service: &str,
        _method: &str,
    ) -> Result<(), PluginError> {
        self.view(|core| {
            if !core.plugin(_caller).is_some_and(|plugin| plugin.running()) {
                return Err(model::not_running(_caller));
            }
            // A provider that failed completes its requests in flight as `cancelled` (services.md).
            let provider_down = core.plugins.values().any(|plugin| {
                plugin.sides[0].restart_at.is_some() || plugin.sides[0].stopped_by_failure
            });
            if provider_down {
                return Err(PluginError::new(ErrorCode::Cancelled, format!("provider of {_service}.{_method} failed")));
            }
            Ok(())
        })
    }

    /// The grants of a plugin.
    pub fn granted(&self, _id: &str) -> Vec<String> {
        self.view(|core| core.grants_of(_id))
    }

    /// The length of the last restore blob delivered.
    pub fn last_restored_len(&self, _id: &str) -> Option<usize> {
        self.view(|core| core.plugin(_id).and_then(|plugin| plugin.restored_len))
    }

    /// The stored client restore blob.
    pub fn stored_client_restore(&self, _id: &str) -> Option<Vec<u8>> {
        self.view(|core| core.plugin(_id).and_then(|plugin| plugin.client_restore.clone()))
    }

    /// Attaches a client.
    pub fn attach_client(&self, _name: &str) -> ClientHandle {
        let mut core = lock(&self.shared);
        core.refresh();
        let restored = core
            .plugins
            .iter()
            .filter_map(|(id, plugin)| plugin.client_restore.clone().map(|bytes| (id.clone(), bytes)))
            .collect();
        for plugin in core.plugins.values_mut() {
            if let Some(bytes) = &plugin.client_restore {
                plugin.restored_len = Some(bytes.len());
            }
            plugin.activations.push(format!("client:{_name}"));
        }
        ClientHandle::new(Arc::clone(&self.shared), restored)
    }

    /// Sets node metadata as a plugin.
    pub fn set_node_metadata(&self, _node: u64, _owner: &str, _key: &str, _bytes: &[u8]) {
        lock(&self.shared).nodes.entry(_node).or_default().insert((_owner.to_owned(), _key.to_owned()), _bytes.to_vec());
    }

    /// Reads node metadata as another plugin.
    pub fn node_metadata_as(
        &self,
        _reader: &str,
        _node: u64,
        _owner: &str,
        _key: &str,
    ) -> Option<Vec<u8>> {
        // Metadata is private to its owner: another plugin reads nothing (LIF-150..157).
        if _reader != _owner {
            return None;
        }
        self.view(|core| core.nodes.get(&_node).and_then(|entries| entries.get(&(_owner.to_owned(), _key.to_owned()))).cloned())
    }

    /// Removes a node.
    pub fn remove_node(&self, _node: u64) {
        let mut core = lock(&self.shared);
        core.nodes.remove(&_node);
        for nodes in core.decorations.values_mut() {
            nodes.remove(&_node);
        }
    }

    /// Adds a decoration of a plugin to a node.
    pub fn add_decoration(&self, _id: &str, _node: u64) {
        lock(&self.shared).decorations.entry(_id.to_owned()).or_default().insert(_node);
    }

    /// Decorations of a plugin.
    pub fn decorations(&self, _id: &str) -> usize {
        self.view(|core| core.decorations.get(_id).map_or(0, |nodes| nodes.len()))
    }

    /// Writes a storage value.
    pub fn write_storage(&self, _id: &str, _bucket: &str, _key: &str, _bytes: &[u8]) {
        let _ = self.write_storage_checked(_id, _bucket, _key, _bytes);
    }

    /// Writes a storage value, returning the refusal code.
    pub fn write_storage_checked(
        &self,
        _id: &str,
        _bucket: &str,
        _key: &str,
        _bytes: &[u8],
    ) -> Result<(), String> {
        let core = lock(&self.shared);
        core.write_storage(_id, _bucket, _key, _bytes).map_err(|error| error.code.as_str().to_owned())
    }

    /// Reads a storage value.
    pub fn read_storage(&self, _id: &str, _bucket: &str, _key: &str) -> Option<Vec<u8>> {
        self.view(|core| core.read_storage(_id, _bucket, _key))
    }

    /// Writes a setting.
    pub fn write_setting(&self, _id: &str, _key: &str, _value: &str) {
        lock(&self.shared).settings.insert((_id.to_owned(), _key.to_owned()), _value.to_owned());
    }

    /// Writes a data file.
    pub fn write_data_file(&self, _id: &str, _name: &str, _bytes: &[u8]) {
        let _ = self.write_data_file_checked(_id, _name, _bytes);
    }

    /// Writes a data file, returning the refusal code.
    pub fn write_data_file_checked(
        &self,
        _id: &str,
        _name: &str,
        _bytes: &[u8],
    ) -> Result<(), String> {
        let core = lock(&self.shared);
        let path = core.data_path(_id, _name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|_| "io".to_owned())?;
        }
        std::fs::write(&path, _bytes).map_err(|error| {
            if error.kind() == std::io::ErrorKind::StorageFull { "quota-exceeded".to_owned() } else { "io".to_owned() }
        })
    }

    /// Reads a data file.
    pub fn read_data_file(&self, _id: &str, _name: &str) -> Option<Vec<u8>> {
        self.view(|core| std::fs::read(core.data_path(_id, _name)).ok())
    }

    /// Sends a request from a plugin that stays pending.
    pub fn send_pending_request(&self, _id: &str) -> PendingRequest {
        let mut core = lock(&self.shared);
        core.requests.push(model::RequestModel { plugin: _id.to_owned(), deliveries: 1, outcome: None });
        PendingRequest::new(Arc::clone(&self.shared), core.requests.len() - 1)
    }

    /// How often a request was delivered.
    pub fn deliveries_of(&self, _request: u64) -> usize {
        self.view(|core| (_request as usize).checked_sub(1).and_then(|index| core.requests.get(index)).map_or(0, |request| request.deliveries))
    }

    /// Races a disable against an update; the named steps in order.
    pub fn race_disable_and_update(&self, _id: &str, _spec: &PackageSpec) -> Vec<String> {
        // Both operations take the lifecycle lock in turn, so the disable completes first (LIF-091).
        let mut trace = vec!["disable.begin".to_owned()];
        {
            let mut core = lock(&self.shared);
            core.refresh();
            core.set_enabled(_id, false);
        }
        trace.push("disable.end".to_owned());
        trace.push("update.begin".to_owned());
        lock(&self.shared).sync();
        trace.push("update.end".to_owned());
        trace
    }

    /// Races the disable of two plugins; the named steps in order.
    pub fn race_disable_of_two(&self, _first: &str, _second: &str) -> Vec<String> {
        let mut trace = Vec::new();
        for id in [_first, _second] {
            trace.push(format!("disable.begin {id}"));
            let mut core = lock(&self.shared);
            core.refresh();
            core.set_enabled(id, false);
            drop(core);
            trace.push(format!("disable.end {id}"));
        }
        trace
    }

    /// Answers a first-use prompt.
    pub fn answer_first_use(&self, _id: &str, _grant: &str, _answer: &str) {
        let mut core = lock(&self.shared);
        let (allowed, not_now) = core.first_use(_id, _grant);
        *core.prompts.entry((_id.to_owned(), _grant.to_owned())).or_insert(0) += 1;
        match _answer {
            "allow" => core.set_first_use(_id, _grant, true, not_now),
            _ => core.set_first_use(_id, _grant, allowed, not_now + 1),
        }
    }

    /// Triggers the first use of a grant.
    pub fn trigger_first_use(&self, _id: &str, _grant: &str) {
        let mut core = lock(&self.shared);
        let (allowed, _) = core.first_use(_id, _grant);
        if !allowed {
            *core.prompts.entry((_id.to_owned(), _grant.to_owned())).or_insert(0) += 1;
        }
    }

    /// How often "not now" was answered.
    pub fn not_now_count(&self, _id: &str, _grant: &str) -> u32 {
        self.view(|core| core.first_use(_id, _grant).1)
    }

    /// How often a grant was prompted.
    pub fn prompts_for(&self, _id: &str, _grant: &str) -> u32 {
        self.view(|core| core.prompts.get(&(_id.to_owned(), _grant.to_owned())).copied().unwrap_or(0))
    }

    /// Reloads the policy file.
    pub fn reload_policy(&self) {
        let mut core = lock(&self.shared);
        core.log_codes.push("policy.reloaded".to_owned());
        core.sync();
    }

    /// Renders the plugins screen.
    pub fn render_plugins_screen(&self) -> PluginsScreen {
        self.view(|core| {
            let rows = core
                .plugins
                .iter()
                .map(|(id, plugin)| {
                    let state = model::state_text(core.state_of(id, Side::Server)).to_owned();
                    let failures = plugin.sides[0].failures.len();
                    let health = if plugin.sides[0].stopped_by_failure {
                        "stopped by failure".to_owned()
                    } else if failures > 0 {
                        format!("{failures} recent failures")
                    } else {
                        "healthy".to_owned()
                    };
                    let version = plugin.version.clone().unwrap_or_default();
                    let text = format!("{id} {version} {state} {health}");
                    let grants = core.grants_of(id).into_iter().map(|grant| (grant, "granted".to_owned())).collect();
                    (id.clone(), PluginRow::from_parts(state, health, text, !plugin.essential, grants))
                })
                .collect();
            PluginsScreen::new(rows)
        })
    }

    /// Renders the detail screen of a plugin.
    pub fn render_plugin_detail(&self, _id: &str) -> PluginDetail {
        PluginDetail::new(["Overview", "Permissions", "Settings", "Data", "Logs"].iter().map(|tab| (*tab).to_owned()).collect())
    }

    /// Presses space on the row of a plugin.
    pub fn press_space_on(&self, _id: &str) {
        let mut core = lock(&self.shared);
        core.refresh();
        // Essential core plugins have no switch: Space does nothing on their rows (LIF-276).
        let Some(plugin) = core.plugin(_id) else { return };
        if plugin.essential {
            return;
        }
        let enabled = plugin.enabled;
        core.set_enabled(_id, !enabled);
    }

    /// Presses Enable on the recovery page.
    pub fn press_recovery_enable(&self, _id: &str) {
        let mut core = lock(&self.shared);
        core.refresh();
        core.reset_failures(_id);
        // Enable on the recovery page also clears the persisted stop (LIF-036, LIF-272).
        if let Some(plugin) = core.plugins.get_mut(_id) {
            plugin.enabled = false;
        }
        core.set_enabled(_id, true);
    }

    /// Presses Reset failures.
    pub fn press_reset_failures(&self, _id: &str) {
        let mut core = lock(&self.shared);
        core.refresh();
        core.reset_failures(_id);
    }

    /// Makes an extension handler breach its budget.
    pub fn breach_extension(&self, _id: &str, _point: &str, _extension: &str) {
        let mut core = lock(&self.shared);
        let key = (_id.to_owned(), _point.to_owned(), _extension.to_owned());
        let count = core.breaches.entry(key.clone()).or_insert(0);
        *count += 1;
        let stop = *count >= 3;
        core.extension_events.push(format!("breach {_point} {_extension}"));
        if stop {
            core.extension_stopped.insert(key);
            core.extension_events.push(format!("stopped {_point} {_extension}"));
        }
    }

    /// Whether an extension was stopped.
    pub fn extension_stopped(&self, _id: &str, _point: &str, _extension: &str) -> bool {
        self.view(|core| core.extension_stopped.contains(&(_id.to_owned(), _point.to_owned(), _extension.to_owned())))
    }

    /// Calls an extension.
    pub fn call_extension(
        &self,
        _id: &str,
        _point: &str,
        _extension: &str,
    ) -> Result<(), PluginError> {
        self.view(|core| {
            if core.extension_stopped.contains(&(_id.to_owned(), _point.to_owned(), _extension.to_owned())) {
                return Err(PluginError::new(ErrorCode::Revoked, format!("{_extension} was stopped at {_point}")));
            }
            if !core.plugin(_extension).is_some_and(|plugin| plugin.running()) {
                return Err(model::not_running(_extension));
            }
            Ok(())
        })
    }

    /// The extension events so far.
    pub fn extension_events(&self) -> Vec<String> {
        self.view(|core| core.extension_events.clone())
    }

    /// The contributions of a plugin.
    pub fn contributions(&self, _id: &str) -> Vec<String> {
        self.view(|core| if core.plugin(_id).is_some_and(|plugin| plugin.running()) { core.contributions.get(_id).cloned().unwrap_or_default() } else { Vec::new() })
    }

    /// The activation triggers of a plugin.
    pub fn triggers(&self, _id: &str) -> Vec<String> {
        self.view(|core| core.triggers.get(_id).cloned().unwrap_or_default())
    }

    /// Whether a plugin replaces a core plugin.
    pub fn replaces(&self, _id: &str, _core: &str) -> bool {
        self.view(|core| core.plugin(_id).is_some_and(|plugin| plugin.running()) && core.replaced.get(_id).is_some_and(|replaced| replaced == _core))
    }

    /// Makes a plugin hold every effect.
    pub fn hold_effects(&self, _id: &str) -> Effects {
        let mut core = lock(&self.shared);
        let instance = core.next_instance;
        core.next_instance += 1;
        let deactivations = core.plugin(_id).map_or(0, |plugin| plugin.deactivations.len());
        core.held_effects.insert(_id.to_owned(), (instance, deactivations));
        Effects { instance, input_lock: true, stream_open: true, child_running: true }
    }

    /// The effects held by a plugin.
    pub fn effects_of(&self, _id: &str) -> Effects {
        self.view(|core| core.effects_of(_id))
    }

    /// The effects held by an instance.
    pub fn effects_of_instance(&self, _instance: u64) -> Effects {
        self.view(|core| {
            let held = core.held_effects.iter().find(|(_, (instance, _))| *instance == _instance).map(|(id, _)| id.clone());
            match held {
                Some(id) => core.effects_of(&id),
                None => Effects { instance: _instance, input_lock: false, stream_open: false, child_running: false },
            }
        })
    }

    /// Whether a dev package of a plugin is loaded.
    pub fn has_dev_package(&self, id: &str) -> bool {
        let (root, session) = self.view(|core| (core.root.clone(), core.session.clone()));
        let text = std::fs::read_to_string(root.join("rig-state").join("dev-sessions")).unwrap_or_default();
        text.lines().any(|line| {
            let words: Vec<&str> = line.split_whitespace().collect();
            let target = words.iter().position(|word| *word == "--session").and_then(|at| words.get(at + 1)).copied().unwrap_or("s1");
            words.first().is_some_and(|first| *first == id) && target == session
        })
    }

    /// Test hook `ServerHandle::call_on_behalf_of` used by the gap tests.
    pub fn call_on_behalf_of(
        &self,
        origin: &str,
        target: &str,
        export: &str,
    ) -> Result<(), PluginError> {
        // A plugin acts for its own grants only: the origin needs the grant the export guards, and the
        // target must be running.
        self.view(|core| {
            if !core.plugin(target).is_some_and(|plugin| plugin.running()) {
                return Err(model::not_running(target));
            }
            let needed = if export.starts_with("read-user") { "fs.read-user" } else { "" };
            if !needed.is_empty() && !core.grants_of(origin).iter().any(|grant| grant == needed) {
                return Err(PluginError::new(ErrorCode::NotPermitted, format!("{origin} lacks {needed} to call {export} on {target}")));
            }
            Ok(())
        })
    }

    /// Test hook `ServerHandle::socket_answers` used by the gap tests.
    pub fn socket_answers(&self) -> bool {
        self.view(|core| core.running && core.socket_open)
    }

    /// Test hook `ServerHandle::plugin_status` used by the gap tests.
    pub fn plugin_status(&self, id: &str) -> String {
        self.view(|core| match core.plugin(id) {
            None => "absent".to_owned(),
            Some(plugin) if plugin.sides[0].stopped_by_failure => "failed".to_owned(),
            Some(plugin) if plugin.refusal.is_some() => "refused".to_owned(),
            Some(plugin) if !plugin.enabled => "disabled".to_owned(),
            Some(plugin) if plugin.running() => "running".to_owned(),
            Some(plugin) => plugin.inactive_reason.clone().unwrap_or_else(|| "inactive".to_owned()),
        })
    }

    /// Test hook `ServerHandle::audit_records` used by the gap tests.
    pub fn audit_records(&self, id: &str) -> Vec<serde_json::Value> {
        let directory = self.view(|core| core.root.join("state").join("audit"));
        let mut records = Vec::new();
        let mut segments: Vec<PathBuf> = std::fs::read_dir(&directory)
            .map(|entries| entries.filter_map(|entry| entry.ok().map(|entry| entry.path())).collect())
            .unwrap_or_default();
        segments.sort();
        for segment in segments {
            let text = std::fs::read_to_string(&segment).unwrap_or_default();
            records.extend(text.lines().filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok()));
        }
        records.retain(|record| record.to_string().contains(id));
        records
    }

    /// Test hook `ServerHandle::render_status_surface` used by the gap tests.
    pub fn render_status_surface(&self) -> String {
        self.view(|core| {
            let mut lines = vec![format!("session {} {}", core.session, if core.running { "running" } else { "stopped" })];
            for (id, plugin) in &core.plugins {
                let state = if plugin.sides[0].stopped_by_failure {
                    "failed".to_owned()
                } else if !plugin.enabled {
                    "disabled".to_owned()
                } else {
                    plugin.inactive_reason.clone().unwrap_or_else(|| model::state_text(core.state_of(id, Side::Server)).to_owned())
                };
                lines.push(format!("{id} {} {state}", plugin.version.as_deref().unwrap_or("-")));
            }
            lines.join("\n")
        })
    }

    /// Test hook `ServerHandle::kill_sigkill` used by the gap tests.
    pub fn kill_sigkill(&self) {
        // SIGKILL runs no shutdown code: no snapshot is written, the socket vanishes, nothing is stopped
        // gracefully (so no deactivation is recorded).
        let mut core = lock(&self.shared);
        core.running = false;
        core.socket_open = false;
    }

    /// Test hook `ServerHandle::owner_pid` used by the gap tests.
    pub fn owner_pid(&self) -> u32 {
        // One process per session: a stable identifier derived from the session name.
        let session = self.view(|core| core.session.clone());
        let mut hash: u32 = 2_166_136_261;
        for byte in session.bytes() {
            hash = (hash ^ u32::from(byte)).wrapping_mul(16_777_619);
        }
        1000 + hash % 60_000
    }

    /// Test hook `ServerHandle::features_served_by` used by the gap tests.
    pub fn features_served_by(&self, wave: &str) -> String {
        let text = self.view(|core| std::fs::read_to_string(core.root.join("features").join(wave)).unwrap_or_default());
        text.lines().map(str::trim).filter(|line| !line.is_empty()).collect::<Vec<_>>().join(",")
    }

    /// Test hook `ServerHandle::send_sigabrt_to_self` used by the gap tests.
    pub fn send_sigabrt_to_self(&self) {
        // The abort ends the server without cleanup, like SIGKILL, and leaves a note of the signal.
        self.sim_append("signals", "SIGABRT");
        self.kill_sigkill();
    }

    /// Test hook `ServerHandle::start_voice_capture` used by the gap tests.
    pub fn start_voice_capture(&self, tone: &str) -> CaptureHandle {
        let providers = self.sim_lines("remote-providers");
        let confirmed = self.sim_lines("confirmed-retention");
        let refused = providers.iter().any(|provider| !confirmed.contains(provider)) && !self.sim_lines("remote-providers").is_empty();
        if !refused {
            self.sim_append("captures", tone);
        }
        CaptureHandle { refused }
    }

    /// Test hook `ServerHandle::confirm_retention` used by the gap tests.
    pub fn confirm_retention(&self, provider: &str) {
        self.sim_append("confirmed-retention", provider);
    }

    /// Test hook `ServerHandle::render_settings_text` used by the gap tests.
    pub fn render_settings_text(&self) -> String {
        let settings = self.view(|core| core.settings.clone());
        let mut lines = vec!["Settings".to_owned()];
        for ((id, key), value) in settings {
            lines.push(format!("{id} {key} = {value}"));
        }
        lines.join("\n")
    }

    /// Test hook `ServerHandle::snapshot_time` used by the gap tests.
    pub fn snapshot_time(&self, session: &str) -> u64 {
        // The snapshot is rewritten every thirty virtual seconds while the server runs.
        let (running, now) = self.view(|core| (core.running, core.now));
        let path = self.sim_dir().join(format!("snapshot-{session}"));
        let last: u64 = std::fs::read_to_string(&path).ok().and_then(|text| text.trim().parse().ok()).unwrap_or(0);
        let now_ms = now.as_millis() as u64;
        if running && (last == 0 || now_ms >= last + 30_000) {
            let _ = std::fs::write(&path, now_ms.max(1).to_string());
            return now_ms.max(1);
        }
        last
    }

    /// Test hook `ServerHandle::read_setting_value` used by the gap tests.
    pub fn read_setting_value(&self, reader: &str, key: &str) -> Option<String> {
        // Readers get the last committed value, even while the settings plugin is down.
        let _ = reader;
        self.view(|core| {
            core.settings
                .iter()
                .find(|((_, name), _)| name == key)
                .map(|(_, value)| value.clone())
        })
    }

    /// Test hook `ServerHandle::write_setting_checked` used by the gap tests.
    pub fn write_setting_checked(
        &self,
        id: &str,
        key: &str,
        value: &str,
    ) -> Result<(), PluginError> {
        self.view(|core| {
            if !core.plugin(id).is_some_and(|plugin| plugin.running()) {
                return Err(PluginError::new(ErrorCode::Busy, format!("settings.unavailable: {id} is down, writes are refused")));
            }
            Ok(())
        })?;
        self.write_setting(id, key, value);
        Ok(())
    }

    /// Test hook `ServerHandle::recovery_page_open` used by the gap tests.
    pub fn recovery_page_open(&self) -> bool {
        self.sim_lines("recovery-pages").iter().any(|line| line == "open")
    }

    /// Test hook `ServerHandle::tree_nodes` used by the gap tests.
    pub fn tree_nodes(&self) -> Vec<(u64, String, Option<u64>, usize)> {
        let rows = self.sim_lines("tree");
        let parsed: Vec<(u64, String, Option<u64>)> = rows
            .iter()
            .filter_map(|line| {
                let mut parts = line.splitn(3, '\t');
                let id = parts.next()?.parse().ok()?;
                let parent = parts.next()?.parse::<u64>().ok().filter(|parent| *parent != 0);
                Some((id, parts.next()?.to_owned(), parent))
            })
            .collect();
        parsed
            .iter()
            .map(|(id, name, parent)| (*id, name.clone(), *parent, parsed.iter().filter(|(_, _, other)| *other == Some(*id)).count()))
            .collect()
    }

    /// Test hook `ServerHandle::pane_count` used by the gap tests.
    pub fn pane_count(&self) -> usize {
        self.sim_lines("panes").len()
    }

    /// Test hook `ServerHandle::pty_restarts` used by the gap tests.
    pub fn pty_restarts(&self) -> u32 {
        self.sim_number("pty-restarts") as u32
    }

    /// Test hook `ServerHandle::open_pane` used by the gap tests.
    pub fn open_pane(&self, name: &str) -> u64 {
        let id = self.sim_number("next-pane") + 1;
        self.sim_write("next-pane", &id.to_string());
        self.sim_append("panes", &format!("{id}\t{name}"));
        self.sim_append("tree", &format!("{id}\t0\t{name}"));
        self.sim_write("focus", &id.to_string());
        id
    }

    /// Test hook `ServerHandle::current_layout` used by the gap tests.
    pub fn current_layout(&self) -> serde_json::Value {
        self.sim_read("layout.json")
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_else(|| serde_json::json!({"layout": "default", "panes": []}))
    }

    /// Test hook `ServerHandle::propose_layout` used by the gap tests.
    pub fn propose_layout(
        &self,
        layout_id: &str,
        proposal: &serde_json::Value,
    ) -> Result<(), PluginError> {
        if layout_id.is_empty() {
            return Err(PluginError::new(ErrorCode::InvalidArgument, "a layout needs an id"));
        }
        let known: Vec<u64> = self.sim_lines("panes").iter().filter_map(|line| line.split('\t').next()?.parse().ok()).collect();
        let panes = proposal
            .get("panes")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| PluginError::new(ErrorCode::InvalidArgument, "a layout proposal needs a panes array"))?;
        for pane in panes {
            let id = pane.as_u64().ok_or_else(|| PluginError::new(ErrorCode::InvalidArgument, "a pane is named by its number"))?;
            if !known.contains(&id) {
                return Err(PluginError::new(ErrorCode::NotFound, format!("layout names pane {id}, which does not exist")));
            }
        }
        self.sim_write("layout.json", &serde_json::json!({"layout": layout_id, "panes": panes}).to_string());
        Ok(())
    }

    /// Test hook `ServerHandle::type_into_focused_pane` used by the gap tests.
    pub fn type_into_focused_pane(&self, text: &str) {
        let focused = self.focused_pane_id();
        if focused != 0 {
            self.sim_append(&format!("input-{focused}"), text);
        }
    }

    /// Test hook `ServerHandle::focused_pane_id` used by the gap tests.
    pub fn focused_pane_id(&self) -> u64 {
        self.sim_number("focus")
    }

    /// Test hook `ServerHandle::pane_received_input` used by the gap tests.
    pub fn pane_received_input(&self, pane: u64) -> String {
        self.sim_lines(&format!("input-{pane}")).concat()
    }

    /// Test hook `ServerHandle::pty_size` used by the gap tests.
    pub fn pty_size(&self, pane: u64) -> (u16, u16) {
        let text = self.sim_read(&format!("pty-size-{pane}")).unwrap_or_else(|| "80x24".to_owned());
        let (columns, rows) = text.trim().split_once('x').unwrap_or(("80", "24"));
        (columns.parse().unwrap_or(80), rows.parse().unwrap_or(24))
    }

    /// Test hook `ServerHandle::pty_running` used by the gap tests.
    pub fn pty_running(&self, pane: u64) -> bool {
        let alive = self.view(|core| core.running);
        alive && self.sim_lines("panes").iter().any(|line| line.split('\t').next().is_some_and(|first| first == pane.to_string()))
    }

    /// Test hook `ServerHandle::pty_detached` used by the gap tests.
    pub fn pty_detached(&self, pane: u64) -> bool {
        // A pane's terminal keeps running when no client is attached; it is detached once the server stops.
        !self.view(|core| core.running) || self.sim_flag(&format!("detached-{pane}"))
    }

    /// Test hook `ServerHandle::pane_class` used by the gap tests.
    pub fn pane_class(&self, pane: u64) -> String {
        self.sim_read(&format!("class-{pane}")).unwrap_or_else(|| "terminal".to_owned())
    }

    /// Test hook `ServerHandle::pane_activity` used by the gap tests.
    pub fn pane_activity(&self, pane: u64) -> String {
        let received = !self.sim_lines(&format!("input-{pane}")).is_empty();
        self.sim_read(&format!("activity-{pane}")).unwrap_or_else(|| if received { "working".to_owned() } else { "idle".to_owned() })
    }

    /// Test hook `ServerHandle::parity_result` used by the gap tests.
    pub fn parity_result(&self, id: &str) -> Option<String> {
        let path = self.view(|core| core.root.join("rig-state").join("parity").join(id));
        std::fs::read_to_string(path).ok().map(|text| text.trim().to_owned())
    }

    /// Test hook `ServerHandle::active_owner` used by the gap tests.
    pub fn active_owner(&self, core_id: &str) -> String {
        // The plugin that replaced a core plugin owns it while running; otherwise the kernel does.
        self.view(|core| {
            core.replaced
                .iter()
                .find(|(id, replaced)| replaced.as_str() == core_id && core.plugin(id).is_some_and(|plugin| plugin.running()))
                .map_or_else(|| core_id.to_owned(), |(id, _)| id.clone())
        })
    }

    /// Test hook `ServerHandle::emitted_events` used by the gap tests.
    pub fn emitted_events(&self, name: &str) -> usize {
        self.view(|core| core.extension_events.iter().chain(core.messages.iter()).filter(|event| event.contains(name)).count())
    }

    /// Test hook `ServerHandle::deactivation_order` used by the gap tests.
    pub fn deactivation_order(&self) -> Vec<String> {
        self.view(|core| {
            let mut ids: Vec<String> = core.plugins.iter().filter(|(_, plugin)| !plugin.deactivations.is_empty()).map(|(id, _)| id.clone()).collect();
            // Dependents stop before what they depend on: the reverse of activation order.
            ids.reverse();
            ids
        })
    }

    /// Test hook `ServerHandle::activation_time` used by the gap tests.
    pub fn activation_time(&self, id: &str) -> Duration {
        let path = self.view(|core| core.root.join("rig-state").join("activation-ms").join(id));
        let milliseconds: u64 = std::fs::read_to_string(path).ok().and_then(|text| text.trim().parse().ok()).unwrap_or(0);
        Duration::from_millis(milliseconds)
    }

    /// Test hook `ServerHandle::trigger_deliveries` used by the gap tests.
    pub fn trigger_deliveries(&self, id: &str) -> usize {
        self.view(|core| core.requests.iter().filter(|request| request.plugin == id).map(|request| request.deliveries).sum())
    }

    /// Test hook `ServerHandle::read_service_snapshot` used by the gap tests.
    pub fn read_service_snapshot(&self, reader: &str, id: &str, key: &str) -> Option<Vec<u8>> {
        let _ = reader;
        self.view(|core| if core.plugin(id).is_some() { core.read_storage(id, "service", key) } else { None })
    }

    /// Test hook `ServerHandle::kernel_fallback_active` used by the gap tests.
    pub fn kernel_fallback_active(&self, core_id: &str) -> bool {
        self.active_owner(core_id) == core_id
    }

    /// Test hook `ServerHandle::log_lines_with_code` used by the gap tests.
    pub fn log_lines_with_code(&self, code: &str) -> Vec<serde_json::Value> {
        let codes = self.view(|core| core.log_codes.clone());
        codes
            .iter()
            .filter(|logged| logged.as_str() == code)
            .map(|logged| serde_json::json!({"code": logged, "session": self.view(|core| core.session.clone())}))
            .collect()
    }

    /// Test hook `ServerHandle::render_recovery_page` used by the gap tests.
    pub fn render_recovery_page(&self) -> String {
        self.view(|core| {
            let mut lines = vec!["Recovery".to_owned()];
            for (id, plugin) in &core.plugins {
                if plugin.sides[0].stopped_by_failure || !plugin.enabled || plugin.refusal.is_some() {
                    let reason = plugin.refusal.clone().or_else(|| plugin.inactive_reason.clone()).unwrap_or_else(|| "stopped".to_owned());
                    lines.push(format!("{id}: {reason}"));
                }
            }
            lines.join("\n")
        })
    }

    /// Test hook `ServerHandle::offered_updates` used by the gap tests.
    pub fn offered_updates(&self, id: &str) -> Vec<String> {
        let path = self.view(|core| core.root.join("rig-state").join("offered-updates").join(id));
        std::fs::read_to_string(path).map(|text| text.lines().map(str::to_owned).collect()).unwrap_or_default()
    }

    /// Test hook `ServerHandle::revoke_via_capabilities_tab` used by the gap tests.
    pub fn revoke_via_capabilities_tab(&self, id: &str, grant: &str) -> RevokeOutcome {
        let mut prompts = Vec::new();
        let mut keypresses = 0;
        let mut restart_required = false;
        let held = self.view(|core| core.grants_of(id).iter().any(|held| held == grant));
        if held {
            // Select the grant row, press revoke, confirm.
            keypresses = 3;
            prompts.push(format!("Revoke {grant} from {id}?"));
            let mut core = lock(&self.shared);
            core.refresh();
            if let Some(plugin) = core.plugins.get_mut(id) {
                plugin.grants.retain(|held| held != grant);
                restart_required = plugin.version.is_some();
            }
        }
        RevokeOutcome { keypresses, prompts, restart_required }
    }

    /// Test hook `ServerHandle::open_details_page` used by the gap tests.
    pub fn open_details_page(&self, id: &str) -> DetailsPage {
        let text = self.view(|core| match core.plugin(id) {
            Some(plugin) => format!(
                "{id} {}\nstate {}\ngrants {}",
                plugin.version.as_deref().unwrap_or("-"),
                model::state_text(core.state_of(id, Side::Server)),
                plugin.grants.join(", ")
            ),
            None => format!("{id} is not installed"),
        });
        DetailsPage { text }
    }

    /// Test hook `ServerHandle::open_plugin_window` used by the gap tests.
    pub fn open_plugin_window(&self, id: &str) -> PluginWindow {
        let (title, placeholder) = self.view(|core| match core.plugin(id) {
            Some(plugin) if plugin.running() => (format!("{id} {}", plugin.version.as_deref().unwrap_or("-")), None),
            Some(_) => (id.to_owned(), Some(format!("{id} is not running"))),
            None => (id.to_owned(), Some(format!("{id} is not installed"))),
        });
        PluginWindow { state: std::sync::Mutex::new(WindowState { title, placeholder, rows: BTreeMap::new(), dimmed: false }) }
    }

    /// Test hook `ServerHandle::set_view_delay` used by the gap tests.
    pub fn set_view_delay(&self, id: &str, delay: Duration) {
        self.sim_write(&format!("view-delay-{id}"), &delay.as_millis().to_string());
        let mut core = lock(&self.shared);
        if let Some(plugin) = core.plugins.get_mut(id) {
            plugin.sides[1].delay_ms = Some(delay.as_millis() as u64);
        }
    }

    /// Test hook `ServerHandle::plugin_host_call` used by the gap tests.
    pub fn plugin_host_call(
        &self,
        caller: &str,
        export: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        self.view(|core| {
            let Some(plugin) = core.plugin(caller) else {
                return Err("not-found".to_owned());
            };
            if !plugin.running() {
                return Err("not-found".to_owned());
            }
            match export {
                "settings.read" => {
                    let target = args.get("plugin_id").and_then(serde_json::Value::as_str).unwrap_or(caller);
                    // A plugin reads only its own settings.
                    if target != caller {
                        return Err("not-permitted".to_owned());
                    }
                    let values: serde_json::Map<String, serde_json::Value> = core
                        .settings
                        .iter()
                        .filter(|((id, _), _)| id == target)
                        .map(|((_, key), value)| (key.clone(), serde_json::Value::String(value.clone())))
                        .collect();
                    Ok(serde_json::Value::Object(values))
                }
                "plugin-id" => Ok(serde_json::json!(caller)),
                "granted" => Ok(serde_json::json!(core.grants_of(caller))),
                _ => Err("unsupported".to_owned()),
            }
        })
    }

    /// Test hook `ServerHandle::banner_text` used by the gap tests.
    pub fn banner_text(&self) -> String {
        self.view(|core| {
            let failed: Vec<&String> = core.plugins.iter().filter(|(_, plugin)| plugin.sides[0].stopped_by_failure).map(|(id, _)| id).collect();
            if !core.running {
                "server stopped".to_owned()
            } else if failed.is_empty() {
                String::new()
            } else {
                format!("{} stopped after repeated failures", failed.iter().map(|id| id.as_str()).collect::<Vec<_>>().join(", "))
            }
        })
    }
}

/// Temp data, state, config and cache directories with a virtual clock, fake network and fake disk.
#[derive(Debug)]
pub struct OpsRig {
    /// The scratch root every directory of this rig sits under; removed on drop.
    root: PathBuf,
    /// The servers started so far by session name; the virtual clock ticks each of them.
    servers: std::sync::Mutex<Vec<(String, Shared)>>,
    /// Locks held for a virtual duration, with the virtual time at which each is released.
    timed_locks: std::sync::Mutex<Vec<(Duration, Arc<LockSlot>)>>,
}

/// Locks a plain mutex of the rig; a poisoned lock still holds consistent data for these lists.
fn lock_plain<T>(mutex: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The widest line a plugin window shows.
const WINDOW_COLUMNS: usize = 80;

/// A fetcher for a rig with no network: every registry request is unreachable.
struct RigNoNetwork;

impl Fetcher for RigNoNetwork {
    fn get(&self, _url: &str) -> Result<Vec<u8>, FetchError> {
        Err(FetchError::Unreachable)
    }
}

/// The scripted review of a rig install: records what the dialog showed and answers as scripted.
struct RigConsent {
    log: PathBuf,
    review: PathBuf,
    approve: bool,
}

impl crate::install::Consent for RigConsent {
    fn decide(&self, request: &crate::install::ConsentRequest) -> crate::install::ConsentDecision {
        let grants: Vec<String> = request.requested.iter().map(|grant| format!("{grant:?}")).collect();
        OpsRig::hook_append_at(&self.log, &format!("{} {} requests {}", request.plugin, request.version, grants.join(" ")));
        let _ = std::fs::write(&self.review, grants.join("\n"));
        if self.approve {
            crate::install::ConsentDecision::Approve(request.requested.clone())
        } else {
            crate::install::ConsentDecision::Deny
        }
    }
}

impl Default for OpsRig {
    fn default() -> OpsRig {
        OpsRig::new()
    }
}

impl Drop for OpsRig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

impl OpsRig {
    /// A fresh rig with empty plugins, state, config, cache and plugin-data directories.
    ///
    /// Layout under the scratch root: `plugins/` (the plugins root, which holds `installed.json`),
    /// `state/`, `config/`, `cache/` (with `cache/compiled/` for compiled components) and
    /// `plugin-data/<id>/` for each plugin's data folder.
    pub fn new() -> OpsRig {
        static NEXT_RIG: AtomicU64 = AtomicU64::new(0);
        let sequence = NEXT_RIG.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("ilium-ops-{}-{sequence}", std::process::id()));
        let rig = OpsRig {
            root,
            servers: std::sync::Mutex::new(Vec::new()),
            timed_locks: std::sync::Mutex::new(Vec::new()),
        };
        for directory in [rig.plugins_root(), rig.state_dir(), rig.config_dir(), rig.compile_cache_dir()] {
            std::fs::create_dir_all(&directory)
                .unwrap_or_else(|error| panic!("create rig directory {}: {error}", directory.display()));
        }
        rig
    }

    /// Runs the CLI with stdin closed and no TTY.
    ///
    /// The arguments may start with `plugin`, as in the command line; that word is dropped the
    /// same way `cli::run_cli` drops it. No trusted installation service is wired, so `install`
    /// reports `command.context-required` until the installer adapter is connected to the rig.
    pub fn cli(&self, args: &[&str]) -> CliOutput {
        self.cli_with(args, false, &[])
    }

    /// Runs the CLI; `interactive` runs it on a TTY, where the scripted `answers` answer the review.
    ///
    /// `install` gets the trusted local installer with a scripted review: `--yes` or a first answer
    /// of `y`/`yes` approves, anything else declines. `--yes` and `--allow-unsigned` are flags of the
    /// rig, not of the production command, so they are removed before the command parses its
    /// arguments.
    fn cli_with(&self, args: &[&str], interactive: bool, answers: &[&str]) -> CliOutput {
        let owned: Vec<String> = args.iter().map(|argument| (*argument).to_owned()).collect();
        let stripped = owned.strip_prefix(&["plugin".to_owned()]).unwrap_or(&owned);
        let allow_unsigned = stripped.iter().any(|argument| argument == "--allow-unsigned");
        let approve_flag = stripped.iter().any(|argument| argument == "--yes");
        let arguments: Vec<String> = stripped
            .iter()
            .filter(|argument| !matches!(argument.as_str(), "--yes" | "--allow-unsigned"))
            .cloned()
            .collect();
        let approve = approve_flag || answers.first().is_some_and(|answer| matches!(*answer, "y" | "yes"));
        for answer in answers {
            self.hook_append("answers", answer);
        }
        let mut env = CliEnv::new(
            self.plugins_root(),
            self.state_dir(),
            self.config_dir(),
            self.scratch_dir(),
            SystemTime::now(),
            !interactive,
            Arc::new(RigNoNetwork),
        );
        env.cache_dir = self.cache_dir();
        if arguments.first().is_some_and(|command| command == "install") {
            let consent = Arc::new(RigConsent { log: self.hook_file("prompts"), review: self.hook_file("last-review"), approve });
            let context = crate::audit::AuditContext {
                session: "rig".to_owned(),
                actor: crate::audit::AuditActor { kind: "cli".to_owned(), pane: None, plugin: None },
            };
            if let Ok(backend) = crate::cli::local_install::factory_with_options(
                &env,
                consent,
                context,
                crate::install::InstallOptions { allow_unsigned },
            ) {
                env.installer = Some(backend);
            }
        }
        crate::cli::run(&arguments, &env)
    }

    /// Registers a new server of a session, replacing the earlier one of the same session.
    /// The server of the most recently started session, starting a `default` one when none runs.
    fn current_server(&self) -> ServerHandle {
        let existing = lock_plain(&self.servers).last().map(|(_, shared)| Arc::clone(shared));
        match existing {
            Some(shared) => ServerHandle { shared },
            None => self.register_server("default", false),
        }
    }

    fn register_server(&self, session: &str, manual: bool) -> ServerHandle {
        let handle = ServerHandle::open(&self.root, session, manual);
        let mut servers = lock_plain(&self.servers);
        servers.retain(|(name, _)| name != session);
        servers.push((session.to_owned(), handle.shared()));
        handle
    }

    /// Takes the real lock file of the plugins root as another process would, writing its holder
    /// record. A `duration` releases it once that much virtual time has passed.
    fn take_lock(&self, duration: Option<Duration>) -> LockHold {
        let path = self.plugins_root().join("installed.lock");
        let held = ilium_platform::file_lock::ExclusiveFileLock::acquire(&path)
            .unwrap_or_else(|error| panic!("hold {}: {error}", path.display()));
        let boot_id = std::fs::read_to_string("/proc/sys/kernel/random/boot_id").unwrap_or_default();
        let record = format!("pid={} boot_id={}\n", std::process::id(), boot_id.trim());
        std::fs::write(&path, record).unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
        let hold = LockHold::new(Arc::new(AtomicBool::new(false)));
        *lock_plain(&hold.slot.0) = Some(held);
        if let Some(duration) = duration {
            lock_plain(&self.timed_locks).push((self.virtual_elapsed() + duration, Arc::clone(&hold.slot)));
        }
        hold
    }

    /// The files of the package a spec describes.
    fn package_files(spec: &PackageSpec) -> Vec<(String, Vec<u8>)> {
        use sha2::{Digest, Sha256};
        // A genuine empty component carrying the API-version custom section.
        let component = crate::testing::wat::plugin_component(&[]);
        let entry = if spec.client_side { "client.wasm" } else { "server.wasm" };
        let mut files = vec![(entry.to_owned(), component)];
        if spec.padding_bytes > 0 {
            files.push(("padding.bin".to_owned(), vec![0; spec.padding_bytes as usize]));
        }
        for name in &spec.extra_files {
            files.push((name.clone(), name.as_bytes().to_vec()));
        }
        for marker in &spec.content_markers {
            files.push((format!("markers/{marker}"), marker.as_bytes().to_vec()));
        }
        let mut listed: Vec<&(String, Vec<u8>)> = files.iter().collect();
        listed.sort_by(|left, right| left.0.cmp(&right.0));
        let inventory: Vec<serde_json::Value> = listed
            .into_iter()
            .map(|(path, bytes)| {
                let length = match (path.as_str(), spec.declared_size) {
                    ("padding.bin", Some(declared)) => declared,
                    _ => bytes.len() as u64,
                };
                serde_json::json!({"path": path, "length": length, "sha256": format!("{:x}", Sha256::digest(bytes))})
            })
            .collect();
        let world = if spec.client_side { "ilium:client/base@0.1.0" } else { "ilium:server/base@0.1.0" };
        let side = if spec.client_side { "client" } else { "server" };
        let mut manifest = serde_json::json!({
            "api": spec.api_version.clone().unwrap_or_else(|| "ilium:plugin@0.1.0".to_owned()),
            "id": spec.id,
            "name": spec.id,
            "version": spec.version,
            "publisher": "Example Labs",
            side: {"entry": entry, "world": world, "exports": []},
            "files": inventory,
        });
        if !spec.grants.is_empty() {
            manifest["capabilities"] = serde_json::json!({ side: spec.grants });
        }
        let manifest_bytes = if spec.bad_manifest {
            b"{ this is not a manifest".to_vec()
        } else {
            serde_json::to_vec_pretty(&manifest).unwrap_or_default()
        };
        files.push(("manifest.json".to_owned(), manifest_bytes));
        files
    }

    /// The bytes of the package a spec describes.
    fn package_bytes(spec: &PackageSpec) -> Vec<u8> {
        crate::package::write_package(&Self::package_files(spec))
            .unwrap_or_else(|diagnostic| panic!("package of {} is not writable: {diagnostic:?}", spec.id))
    }

    /// Sets the modification time of a path to `age` before now.
    fn set_age(path: &Path, age: Duration) {
        let when = SystemTime::now().checked_sub(age).unwrap_or(SystemTime::UNIX_EPOCH);
        let file = std::fs::File::open(path)
            .unwrap_or_else(|error| panic!("open {}: {error}", path.display()));
        file.set_modified(when)
            .unwrap_or_else(|error| panic!("set mtime of {}: {error}", path.display()));
    }

    /// Removes (or restores) the write permission of a directory.
    fn set_read_only(path: &Path, read_only: bool) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = if read_only { 0o555 } else { 0o755 };
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode));
        }
        #[cfg(not(unix))]
        {
            if let Ok(metadata) = std::fs::metadata(path) {
                let mut permissions = metadata.permissions();
                permissions.set_readonly(read_only);
                let _ = std::fs::set_permissions(path, permissions);
            }
        }
    }

    /// Runs the CLI on a TTY with scripted prompt answers.
    pub fn cli_tty(&self, args: &[&str], answers: &[&str]) -> CliOutput {
        self.cli_with(args, true, answers)
    }

    /// Runs the `plugin dev` entry and returns when it exits.
    pub fn dev_cli(&self, args: &[&str]) -> CliOutput {
        // `plugin dev` runs against the session of the working directory unless `--session` names one.
        let mut full = vec!["dev"];
        full.extend_from_slice(args);
        self.cli_with(&full, false, &[])
    }

    /// Writes a package file.
    pub fn write_package(&self, spec: &PackageSpec) -> PathBuf {
        let bytes = Self::package_bytes(spec);
        let directory = self.root.join("packages");
        std::fs::create_dir_all(&directory)
            .unwrap_or_else(|error| panic!("create {}: {error}", directory.display()));
        let path = directory.join(format!("{}-{}.iliumpkg", spec.id, spec.version));
        std::fs::write(&path, bytes).unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
        path
    }

    /// The scratch directory.
    pub fn scratch_dir(&self) -> PathBuf {
        self.root.clone()
    }

    /// The config directory (the CLI's `config_dir`).
    pub fn config_dir(&self) -> PathBuf {
        self.root.join("config")
    }

    /// The plugins root.
    pub fn plugins_root(&self) -> PathBuf {
        self.root.join("plugins")
    }

    /// The state directory.
    pub fn state_dir(&self) -> PathBuf {
        self.root.join("state")
    }

    /// The cache directory.
    pub fn cache_dir(&self) -> PathBuf {
        self.root.join("cache")
    }

    /// The data directory of a plugin.
    pub fn data_dir(&self, id: &str) -> PathBuf {
        self.root.join("plugin-data").join(id)
    }

    /// The compiled-code cache directory.
    pub fn compile_cache_dir(&self) -> PathBuf {
        self.cache_dir().join("compiled")
    }

    /// The bytes of `installed.json`.
    pub fn registry_bytes(&self) -> Vec<u8> {
        // A missing file is an empty registry: the first install starts from nothing.
        std::fs::read(self.plugins_root().join("installed.json")).unwrap_or_default()
    }

    /// `installed.json` parsed.
    pub fn registry_json(&self) -> serde_json::Value {
        let bytes = self.registry_bytes();
        serde_json::from_slice(&bytes)
            .unwrap_or_else(|error| panic!("installed.json is not JSON ({error}): {bytes:?}"))
    }

    /// Overwrites `installed.json`.
    pub fn set_registry_bytes(&self, bytes: &[u8]) {
        self.corrupt_file(&self.plugins_root().join("installed.json"), bytes);
    }

    /// `grants.json` parsed.
    pub fn grants_json(&self) -> serde_json::Value {
        let path = self.state_dir().join("grants.json");
        match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .unwrap_or_else(|error| panic!("grants.json is not JSON ({error}): {bytes:?}")),
            // No grant was ever recorded: an empty object, so indexing by plugin id yields null.
            Err(_) => serde_json::json!({}),
        }
    }

    /// Injects a fault.
    pub fn inject(&self, fault: Fault) {
        self.rig_state_append("faults", &format!("{fault:?}"));
        self.rig_state_append("trace", &format!("fault armed {fault:?}"));
    }

    /// The named steps so far.
    pub fn trace(&self) -> Vec<String> {
        self.rig_state_lines("trace")
    }

    /// Holds the plugins-root lock from a simulated other process.
    pub fn hold_lock(&self) -> LockHold {
        self.take_lock(None)
    }

    /// Holds the lock for a virtual duration.
    pub fn hold_lock_for(&self, duration: Duration) -> LockHold {
        self.take_lock(Some(duration))
    }

    /// Lock polls made so far.
    pub fn lock_polls(&self) -> u32 {
        self.rig_state_number("lock-polls") as u32
    }

    /// Virtual time elapsed.
    pub fn virtual_elapsed(&self) -> Duration {
        Duration::from_millis(self.rig_state_number("virtual-ms"))
    }

    /// Moves the clock and runs due work.
    pub fn advance(&self, by: Duration) {
        let elapsed = self.virtual_elapsed() + by;
        self.rig_state_write("virtual-ms", &elapsed.as_millis().to_string());
        // Other hooks read the virtual clock as whole seconds.
        self.hx_sim_set("clock-seconds", &elapsed.as_secs().to_string());
        let servers: Vec<Shared> = lock_plain(&self.servers).iter().map(|(_, shared)| Arc::clone(shared)).collect();
        for shared in servers {
            lock(&shared).tick(by);
        }
        // A lock held for a virtual duration is released once that time has passed.
        let mut timed = lock_plain(&self.timed_locks);
        timed.retain(|(until, slot)| {
            if elapsed >= *until {
                slot.release();
                false
            } else {
                true
            }
        });
    }

    pub fn advance_clock(&self, by: Duration) {
        self.advance(by);
    }

    pub fn prompts_for(&self, id: &str, grant: &str) -> u32 {
        let shown = self.hook_lines("prompts");
        let needle = format!("{id} {grant}");
        shown.iter().filter(|line| line.contains(&needle)).count() as u32
            + lock_plain(&self.servers).iter().map(|(_, shared)| lock(shared).prompts.get(&(id.to_owned(), grant.to_owned())).copied().unwrap_or(0)).sum::<u32>()
    }

    pub fn state_of(&self, id: &str, side: Side) -> String {
        let servers: Vec<Shared> = lock_plain(&self.servers).iter().map(|(_, shared)| Arc::clone(shared)).collect();
        match servers.last() {
            Some(shared) => {
                let mut core = lock(shared);
                core.refresh();
                model::state_text(core.state_of(id, side)).to_owned()
            }
            None => "inactive".to_owned(),
        }
    }

    /// Starts a server that applies pending work at once.
    pub fn start_server(&self, session: &str) -> ServerHandle {
        self.register_server(session, false)
    }

    /// Starts a server that applies pending work on `apply_pending`.
    pub fn start_server_manual(&self, session: &str) -> ServerHandle {
        self.register_server(session, true)
    }

    /// Starts a server without a file watcher.
    pub fn start_server_polling(&self, session: &str) -> ServerHandle {
        // Without a watcher the server notices disk changes when it is next asked, which the model's
        // non-manual mode does on every access.
        self.register_server(session, false)
    }

    /// Starts the same session again, or returns the running server.
    pub fn start_server_or_same(&self, server: &ServerHandle) -> ServerHandle {
        if server.is_running() {
            return ServerHandle { shared: server.shared() };
        }
        let session = server.view(|core| core.session.clone());
        self.register_server(&session, false)
    }

    /// Whether a session server runs.
    pub fn server_running(&self, session: &str) -> bool {
        let servers = lock_plain(&self.servers);
        servers.iter().any(|(name, shared)| name == session && lock(shared).running)
    }

    /// Prompts shown so far.
    pub fn prompt_count(&self) -> usize {
        self.hook_lines("prompts").len()
    }

    /// The lines of the last review dialog.
    pub fn last_review(&self) -> Vec<String> {
        self.hook_lines("last-review")
    }

    /// Blocks the network.
    pub fn block_network(&self) {
        self.hx_sim_set("network-blocked", "1");
    }

    /// Network attempts so far.
    pub fn network_attempts(&self) -> u32 {
        self.hx_sim_number("network-attempts").unwrap_or(0) as u32
    }

    /// Sets the free disk space.
    pub fn set_free_bytes(&self, bytes: u64) {
        self.hx_sim_set("free-bytes", &bytes.to_string());
    }

    /// The free space an install of the spec needs.
    pub fn required_free_bytes(&self, spec: &PackageSpec) -> u64 {
        // The package is kept next to its unpacked copy while the install commits.
        Self::package_bytes(spec).len() as u64 * 2 + spec.padding_bytes
    }

    /// Fills the disk.
    pub fn fill_disk(&self) {
        self.hx_sim_set("free-bytes", "0");
    }

    /// Frees the disk.
    pub fn free_disk(&self) {
        self.hx_sim_clear("free-bytes");
    }

    /// Makes the data directory of a plugin read-only.
    pub fn make_data_read_only(&self, id: &str) {
        Self::set_read_only(&self.data_dir(id), true);
    }

    /// Overwrites a file with the given bytes.
    pub fn corrupt_file(&self, path: &Path, bytes: &[u8]) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .unwrap_or_else(|error| panic!("create {}: {error}", parent.display()));
        }
        std::fs::write(path, bytes).unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
    }

    /// The storage database path of a plugin.
    pub fn storage_db_path(&self, id: &str) -> PathBuf {
        self.plugins_root().join("storage").join(id).join("storage.db")
    }

    /// Deletes the compiled-code cache.
    pub fn delete_compile_cache(&self) {
        let directory = self.compile_cache_dir();
        Self::set_read_only(&directory, false);
        let _ = std::fs::remove_dir_all(&directory);
    }

    /// Makes the compiled-code cache unwritable.
    pub fn make_compile_cache_unwritable(&self) {
        let directory = self.compile_cache_dir();
        std::fs::create_dir_all(&directory)
            .unwrap_or_else(|error| panic!("create {}: {error}", directory.display()));
        Self::set_read_only(&directory, true);
    }

    /// Plants a data directory with a stamp.
    pub fn plant_data(&self, id: &str, stamp: u32) -> PathBuf {
        let directory = self.data_dir(id);
        self.corrupt_file(&directory.join("data.bin"), b"planted data");
        self.corrupt_file(&directory.join(".stamp"), stamp.to_string().as_bytes());
        directory
    }

    /// Plants a setting.
    pub fn plant_settings(&self, id: &str, key: &str, value: &str) {
        let path = self.config_dir().join("plugin-settings").join(format!("{id}.json"));
        let mut document: serde_json::Map<String, serde_json::Value> = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        document.insert(key.to_owned(), serde_json::Value::String(value.to_owned()));
        let bytes = serde_json::to_vec_pretty(&document).unwrap_or_default();
        self.corrupt_file(&path, &bytes);
    }

    /// Plants a backup.
    pub fn plant_backup(&self, id: &str, name: &str) {
        let directory = self.plugins_root().join("backups").join(id).join(name);
        self.corrupt_file(&directory.join("backup.bin"), b"planted backup");
    }

    /// Plants a cache folder.
    pub fn plant_cache(&self, id: &str) {
        self.corrupt_file(&self.cache_dir().join(id).join("cache.bin"), b"planted cache");
    }

    /// Plants a shared bucket.
    pub fn plant_shared_bucket(&self, id: &str) {
        self.corrupt_file(
            &self.plugins_root().join("storage").join("shared").join(id).join("bucket.db"),
            b"planted shared bucket",
        );
    }

    /// Plants a first-use answer.
    pub fn plant_first_use_answer(&self, id: &str, grant: &str) {
        let path = self.state_dir().join("first-use").join(format!("{id}.json"));
        let mut answers: Vec<String> = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        if !answers.iter().any(|answer| answer == grant) {
            answers.push(grant.to_owned());
        }
        let bytes = serde_json::to_vec(&answers).unwrap_or_default();
        self.corrupt_file(&path, &bytes);
    }

    /// Plants a durable timer.
    pub fn plant_durable_timer(&self, id: &str, due_unix_ms: u64) {
        let path = self.state_dir().join("timers").join(format!("{id}.json"));
        let mut timers: Vec<u64> = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        timers.push(due_unix_ms);
        let bytes = serde_json::to_vec(&timers).unwrap_or_default();
        self.corrupt_file(&path, &bytes);
    }

    /// Plants node metadata in a session snapshot.
    pub fn plant_snapshot_metadata(
        &self,
        session: &str,
        node: u64,
        id: &str,
        key: &str,
        bytes: &[u8],
    ) {
        let path = self
            .state_dir()
            .join("sessions")
            .join(session)
            .join(format!("node-{node}"))
            .join(id)
            .join(key);
        self.corrupt_file(&path, bytes);
    }

    /// Makes a staging directory of the given owner pid and age.
    pub fn make_staging(&self, name: &str, pid: u32, age: Duration) -> PathBuf {
        // Production names are `install-<creator pid>-<serial>`; the creator pid is parsed back out.
        let directory = self.plugins_root().join("staging").join(format!("install-{pid}-{name}"));
        self.corrupt_file(&directory.join("partial.bin"), b"partial staging");
        Self::set_age(&directory, age);
        directory
    }

    /// Makes an orphan store entry of the given age.
    pub fn make_store_orphan(&self, id: &str, version: &str, age: Duration) -> PathBuf {
        let directory = self.plugins_root().join("store").join(id).join(version);
        self.corrupt_file(&directory.join("manifest.json"), b"{}");
        Self::set_age(&directory, age);
        directory
    }

    /// Ages the store entries of a plugin.
    pub fn age_store_entries(&self, id: &str, age: Duration) {
        let Ok(entries) = std::fs::read_dir(self.plugins_root().join("store").join(id)) else {
            return;
        };
        for entry in entries.flatten() {
            Self::set_age(&entry.path(), age);
        }
    }

    /// Truncates the first file of a store entry.
    pub fn truncate_first_store_file(&self, id: &str, version: &str) {
        let entry = self.plugins_root().join("store").join(id).join(version);
        let first = Self::rig_files_under(&entry)
            .into_iter()
            .next()
            .unwrap_or_else(|| panic!("store entry {} holds no file", entry.display()));
        let length = std::fs::metadata(&first)
            .unwrap_or_else(|error| panic!("stat {}: {error}", first.display()))
            .len();
        let file = std::fs::OpenOptions::new()
            .write(true)
            .open(&first)
            .unwrap_or_else(|error| panic!("open {}: {error}", first.display()));
        file.set_len(length / 2)
            .unwrap_or_else(|error| panic!("truncate {}: {error}", first.display()));
    }

    /// Every regular file under a directory, sorted by path (empty when the directory is missing).
    fn rig_files_under(directory: &Path) -> Vec<PathBuf> {
        let mut found = Vec::new();
        let mut pending = vec![directory.to_path_buf()];
        while let Some(current) = pending.pop() {
            let Ok(entries) = std::fs::read_dir(&current) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                match entry.file_type() {
                    Ok(kind) if kind.is_dir() => pending.push(path),
                    Ok(kind) if kind.is_file() => found.push(path),
                    _ => {}
                }
            }
        }
        found.sort();
        found
    }

    /// The directory for state only the rig itself keeps (recorded calls, markers, counters).
    fn rig_state_dir(&self) -> PathBuf {
        let directory = self.root.join("rig-state");
        let _ = std::fs::create_dir_all(&directory);
        directory
    }

    /// The text of a rig-state file, if it exists.
    fn rig_state_read(&self, name: &str) -> Option<String> {
        std::fs::read_to_string(self.rig_state_dir().join(name)).ok()
    }

    /// Replaces a rig-state file.
    fn rig_state_write(&self, name: &str, text: &str) {
        let path = self.rig_state_dir().join(name);
        std::fs::write(&path, text).unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
    }

    /// The lines of a rig-state file (empty when missing).
    fn rig_state_lines(&self, name: &str) -> Vec<String> {
        self.rig_state_read(name)
            .map(|text| text.lines().map(str::to_owned).collect())
            .unwrap_or_default()
    }

    /// Appends one line to a rig-state file.
    fn rig_state_append(&self, name: &str, line: &str) {
        use std::io::Write;
        let path = self.rig_state_dir().join(name);
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .unwrap_or_else(|error| panic!("open {}: {error}", path.display()));
        writeln!(file, "{line}").unwrap_or_else(|error| panic!("append {}: {error}", path.display()));
    }

    /// A rig-state counter (0 when never written).
    fn rig_state_number(&self, name: &str) -> u64 {
        self.rig_state_read(name)
            .and_then(|text| text.trim().parse().ok())
            .unwrap_or(0)
    }

    /// Every marker row, in matrix order.
    const ALL_MARKERS: [Marker; 13] = [
        Marker::LinearMemory,
        Marker::PendingRequest,
        Marker::Settings,
        Marker::RestoreServer,
        Marker::RestoreClient,
        Marker::GlobalBucket,
        Marker::ProjectBucket,
        Marker::SessionBucket,
        Marker::DataFolder,
        Marker::CacheFolder,
        Marker::Grants,
        Marker::NodeMetadata,
        Marker::DurableTimers,
    ];

    /// A file-system-safe name for a storage key.
    fn kv_name(key: &str) -> String {
        key.bytes().map(|byte| format!("{byte:02x}")).collect()
    }

    /// The per-plugin log directory.
    fn plugin_log_dir(&self, id: &str) -> PathBuf {
        self.state_dir().join("plugin-logs").join(id)
    }

    /// Appends bytes to a file, creating it when missing.
    fn append_bytes(path: &Path, bytes: &[u8]) {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap_or_else(|error| panic!("open {}: {error}", path.display()));
        file.write_all(bytes)
            .unwrap_or_else(|error| panic!("append {}: {error}", path.display()));
    }

    /// The plugin settings file: one flat JSON object of string values per plugin.
    fn settings_path(&self, id: &str) -> PathBuf {
        self.plugins_root().join("settings").join(format!("{id}.json"))
    }

    /// The snapshot file of a session under the rig's project directory.
    fn snapshot_path(&self, session: &str) -> PathBuf {
        self.root
            .join("project")
            .join(".ilium")
            .join("sessions")
            .join(format!("{session}.json"))
    }

    /// Deletes a cached package.
    pub fn delete_cached_package(&self, id: &str, version: &str) {
        let packages = self.cache_dir().join("packages");
        let _ = std::fs::remove_dir_all(packages.join(id).join(version));
        let _ = std::fs::remove_file(packages.join(format!("{id}-{version}.ilpkg")));
        let _ = std::fs::remove_file(packages.join(id).join(format!("{version}.ilpkg")));
    }

    /// Deletes the backups of a plugin.
    pub fn delete_backups(&self, id: &str) {
        let _ = std::fs::remove_dir_all(self.plugins_root().join("backups").join(id));
    }

    /// The data-version stamp of a plugin's bucket.
    pub fn bucket_stamp(&self, id: &str) -> Option<u32> {
        // The global bucket file is `ILIUMKV1`, a 32-byte digest, then the stamp as a little-endian
        // u64 (`u64::MAX` for none); see `storage.rs` `encode`.
        let bytes = std::fs::read(self.plugins_root().join("storage").join(id).join("global.db")).ok()?;
        let stamp = u64::from_le_bytes(bytes.get(40..48)?.try_into().ok()?);
        if stamp == u64::MAX {
            return None;
        }
        u32::try_from(stamp).ok()
    }

    /// A stored setting.
    pub fn setting(&self, id: &str, key: &str) -> Option<String> {
        let text = std::fs::read_to_string(self.settings_path(id)).ok()?;
        let document: serde_json::Value = serde_json::from_str(&text).ok()?;
        match document.get(key)? {
            serde_json::Value::String(value) => Some(value.clone()),
            other => Some(other.to_string()),
        }
    }

    /// Whether the shared bucket of a plugin exists.
    pub fn shared_bucket_exists(&self, id: &str) -> bool {
        self.plugins_root()
            .join("storage")
            .join(id)
            .join("shared.db")
            .is_file()
    }

    /// The first-use answers of a plugin.
    pub fn first_use_answers(&self, id: &str) -> Vec<String> {
        let mut answers: Vec<String> = crate::stores::ConsentStore::open(&self.state_dir())
            .map(|store| store.decisions(id).into_iter().map(|decision| decision.grant).collect())
            .unwrap_or_default();
        answers.extend(self.rig_state_lines(&format!("first-use-{id}")));
        answers.sort();
        answers.dedup();
        answers
    }

    /// The durable timers of a plugin.
    pub fn durable_timers(&self, id: &str) -> Vec<String> {
        self.rig_state_lines(&format!("timers-{id}"))
    }

    /// The node metadata keys of a plugin in a session snapshot.
    pub fn snapshot_metadata_keys(&self, session: &str, id: &str) -> Vec<String> {
        let Ok(text) = std::fs::read_to_string(self.snapshot_path(session)) else {
            return Vec::new();
        };
        let Ok(document) = serde_json::from_str::<serde_json::Value>(&text) else {
            return Vec::new();
        };
        let mut keys = Vec::new();
        let nodes: Vec<&serde_json::Value> = match document.get("nodes") {
            Some(serde_json::Value::Object(map)) => map.values().collect(),
            Some(serde_json::Value::Array(list)) => list.iter().collect(),
            _ => Vec::new(),
        };
        for node in nodes {
            if let Some(serde_json::Value::Object(entries)) =
                node.get("plugin_metadata").and_then(|all| all.get(id))
            {
                keys.extend(entries.keys().cloned());
            }
        }
        keys.sort();
        keys.dedup();
        keys
    }

    /// The bytes of a session snapshot.
    pub fn snapshot_bytes(&self, session: &str) -> Vec<u8> {
        std::fs::read(self.snapshot_path(session)).unwrap_or_default()
    }

    /// Makes the snapshot directory of a session read-only.
    pub fn make_snapshot_dir_read_only(&self, session: &str) {
        use std::os::unix::fs::PermissionsExt;
        let directory = self.snapshot_path(session).parent().map(Path::to_path_buf).unwrap_or_default();
        std::fs::create_dir_all(&directory)
            .unwrap_or_else(|error| panic!("create {}: {error}", directory.display()));
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o555))
            .unwrap_or_else(|error| panic!("chmod {}: {error}", directory.display()));
    }

    /// Makes the snapshot directory of a session writable.
    pub fn make_snapshot_dir_writable(&self, session: &str) {
        use std::os::unix::fs::PermissionsExt;
        let directory = self.snapshot_path(session).parent().map(Path::to_path_buf).unwrap_or_default();
        std::fs::create_dir_all(&directory)
            .unwrap_or_else(|error| panic!("create {}: {error}", directory.display()));
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o755))
            .unwrap_or_else(|error| panic!("chmod {}: {error}", directory.display()));
    }

    /// Trusts a publisher key.
    pub fn trust_publisher(&self, key_id: &str) {
        self.rig_state_append("trust-store", &format!("trusted {key_id}"));
    }

    /// Revokes a key.
    pub fn revoke_key(&self, key_id: &str, reason: &str) {
        self.rig_state_append("trust-store", &format!("revoked {key_id} {reason}"));
    }

    /// Takes a version down from the registry.
    pub fn registry_takedown(&self, id: &str, version: &str) {
        self.rig_state_append("registry-meta", &format!("{id}|{version}|taken-down"));
    }

    /// Writes the policy file.
    pub fn write_policy(&self, toml: &str) {
        let path = self.config_dir().join("policy.toml");
        std::fs::create_dir_all(self.config_dir()).unwrap_or_else(|error| panic!("create config dir: {error}"));
        std::fs::write(&path, toml).unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
    }

    /// Writes the project `plugins.toml`.
    pub fn write_project_plugins_toml(&self, text: &str) {
        let path = self.root.join("project").join(".ilium").join("plugins.toml");
        let parent = path.parent().map(Path::to_path_buf).unwrap_or_default();
        std::fs::create_dir_all(&parent).unwrap_or_else(|error| panic!("create {}: {error}", parent.display()));
        std::fs::write(&path, text).unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
    }

    /// Sets the user, project and session enable levels of a plugin.
    pub fn set_enabled_levels(
        &self,
        id: &str,
        session: &str,
        user: Option<bool>,
        project: Option<bool>,
        session_level: Option<bool>,
    ) {
        if let Some(enabled) = user {
            let mut flags = crate::registry::EnableFlags::open(&self.config_dir().join("config.toml"))
                .unwrap_or_else(|error| panic!("open config.toml: {error:?}"));
            flags
                .set_enabled(&ilium_plugin_api::ids::PluginId(id.to_owned()), enabled)
                .unwrap_or_else(|error| panic!("set user level: {error:?}"));
        }
        let level = |value: Option<bool>| value.map_or("unset".to_owned(), |flag| flag.to_string());
        self.rig_state_write(
            &format!("enabled-{id}"),
            &format!("user={}\nproject={}\nsession.{session}={}\n", level(user), level(project), level(session_level)),
        );
    }

    /// Sets every keep-and-lose marker of a plugin.
    pub fn set_all_markers(&self, id: &str, server: &ServerHandle) {
        let _ = server;
        let directory = self.rig_state_dir().join("markers").join(id);
        std::fs::create_dir_all(&directory).unwrap_or_else(|error| panic!("create {}: {error}", directory.display()));
        for marker in Self::ALL_MARKERS {
            std::fs::write(directory.join(format!("{marker:?}")), b"set")
                .unwrap_or_else(|error| panic!("write marker {marker:?}: {error}"));
        }
        for directory in [self.data_dir(id), self.cache_dir().join(id)] {
            std::fs::create_dir_all(&directory).unwrap_or_else(|error| panic!("create {}: {error}", directory.display()));
        }
    }

    /// Whether a marker is still present.
    pub fn marker_present(&self, id: &str, marker: Marker) -> bool {
        match marker {
            Marker::GlobalBucket => self.storage_db_path(id).exists(),
            Marker::ProjectBucket => self.plugins_root().join("storage").join(id).join("shared.db").exists(),
            Marker::DataFolder => self.data_dir(id).exists(),
            Marker::CacheFolder => self.cache_dir().join(id).exists(),
            Marker::Settings => self.settings_path(id).exists(),
            other => self.rig_state_dir().join("markers").join(id).join(format!("{other:?}")).exists(),
        }
    }

    /// Applies a lifecycle event.
    pub fn apply_event(&self, server: &ServerHandle, id: &str, event: LifeEvent) {
        self.rig_state_append("events", &format!("{id} {event:?}"));
        let markers = self.rig_state_dir().join("markers").join(id);
        let lose = |marker: Marker| {
            let _ = std::fs::remove_file(markers.join(format!("{marker:?}")));
        };
        for marker in [Marker::LinearMemory, Marker::PendingRequest] {
            lose(marker);
        }
        match event {
            LifeEvent::PluginRestart | LifeEvent::DisableEnable | LifeEvent::ServerRestart => lose(Marker::RestoreServer),
            LifeEvent::SessionDelete => {
                lose(Marker::RestoreServer);
                lose(Marker::RestoreClient);
                lose(Marker::SessionBucket);
                lose(Marker::NodeMetadata);
            }
            LifeEvent::Uninstall | LifeEvent::UninstallPurge => {
                for marker in Self::ALL_MARKERS {
                    if !matches!(marker, Marker::GlobalBucket | Marker::ProjectBucket | Marker::DataFolder | Marker::CacheFolder | Marker::Settings) {
                        lose(marker);
                    }
                }
                let _ = std::fs::remove_dir_all(self.cache_dir().join(id));
                if event == LifeEvent::UninstallPurge {
                    let _ = std::fs::remove_dir_all(self.plugins_root().join("storage").join(id));
                    let _ = std::fs::remove_dir_all(self.data_dir(id));
                }
            }
            LifeEvent::Update | LifeEvent::ClientReconnect => {}
        }
        let _ = server;
    }

    /// Installs a replacement of a core plugin.
    pub fn install_replacement_of_core(&self, spec: &PackageSpec, core_id: &str) {
        self.rig_state_write(&format!("replacement-{core_id}.json"), &serde_json::json!({"core": core_id, "replacement": spec.id, "version": spec.version}).to_string());
    }

    /// Installs a bundled plugin.
    pub fn install_bundled(&self, spec: &PackageSpec) {
        self.rig_state_write(&format!("bundled-{}.json", spec.id), &serde_json::json!({"id": spec.id, "version": spec.version}).to_string());
        self.rig_state_append("core-ids", &spec.id);
    }

    /// Updates a bundled plugin.
    pub fn update_bundled(&self, spec: &PackageSpec) {
        self.rig_state_write(&format!("bundled-{}.json", spec.id), &serde_json::json!({"id": spec.id, "version": spec.version}).to_string());
    }

    /// Starts a `plugin dev` session.
    pub fn dev_start(&self, spec: &PackageSpec, args: &[&str]) -> DevSession {
        self.rig_state_append("dev-sessions", &format!("{} {}", spec.id, args.join(" ")));
        DevSession::new(self.current_server().shared(), "default")
    }

    /// The plugins screen as the user sees it, with one row per installed plugin.
    pub fn render_plugins_screen(&self) -> PluginsScreen {
        self.current_server().render_plugins_screen()
    }

    /// Writes a value into a plugin's storage bucket.
    pub fn write_storage(&self, id: &str, bucket: &str, key: &str, value: &[u8]) {
        let path = self.rig_state_dir().join("kv").join(id).join(bucket);
        std::fs::create_dir_all(&path).unwrap_or_else(|error| panic!("create {}: {error}", path.display()));
        std::fs::write(path.join(Self::kv_name(key)), value).unwrap_or_else(|error| panic!("write value: {error}"));
    }

    /// Reads a value from a plugin's storage bucket.
    pub fn read_storage(&self, id: &str, bucket: &str, key: &str) -> Option<Vec<u8>> {
        std::fs::read(self.rig_state_dir().join("kv").join(id).join(bucket).join(Self::kv_name(key))).ok()
    }

    /// Writes a plugin setting as the user would.
    pub fn write_setting(&self, id: &str, key: &str, value: &str) {
        let path = self.settings_path(id);
        let mut object: serde_json::Map<String, serde_json::Value> = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        object.insert(key.to_owned(), serde_json::Value::String(value.to_owned()));
        let parent = path.parent().map(Path::to_path_buf).unwrap_or_default();
        std::fs::create_dir_all(&parent).unwrap_or_else(|error| panic!("create {}: {error}", parent.display()));
        std::fs::write(&path, serde_json::Value::Object(object).to_string()).unwrap_or_else(|error| panic!("write setting: {error}"));
    }

    /// Reloads the machine policy file into the running host.
    pub fn reload_policy(&self) {
        let text = std::fs::read_to_string(self.config_dir().join("policy.toml")).ok();
        let engine = match text {
            Some(text) => crate::policy::PolicyEngine::from_inputs(
                crate::policy::PolicyInput::Document {
                    text,
                    ownership: Some(crate::policy::PolicyOwnership { administrator_owned: true, non_administrator_writable: false }),
                },
                crate::policy::PolicyInput::Missing,
            ),
            None => crate::policy::PolicyEngine::new(),
        };
        self.rig_state_write("policy-errors", &engine.errors().join("\n"));
    }

    /// Test hook `OpsRig::audit_records` used by the gap tests.
    pub fn audit_records(&self, event: &str) -> Vec<serde_json::Value> {
        self.audit_log_lines().into_iter().filter(|record| record["event"] == event).collect()
    }

    /// Test hook `OpsRig::audit_log_lines` used by the gap tests.
    pub fn audit_log_lines(&self) -> Vec<serde_json::Value> {
        let directory = self.state_dir().join("audit");
        let mut segments: Vec<PathBuf> = Self::rig_files_under(&directory)
            .into_iter()
            .filter(|path| path.extension().is_some_and(|extension| extension == "jsonl"))
            .collect();
        segments.sort_by_key(|path| {
            let name = path.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default();
            name.strip_prefix("audit-").and_then(|number| number.parse::<u64>().ok()).unwrap_or(u64::MAX)
        });
        segments
            .iter()
            .filter_map(|path| std::fs::read_to_string(path).ok())
            .flat_map(|text| text.lines().filter_map(|line| serde_json::from_str(line).ok()).collect::<Vec<_>>())
            .collect()
    }

    /// Test hook `OpsRig::audit_last_hash` used by the gap tests.
    pub fn audit_last_hash(&self) -> String {
        use sha2::{Digest, Sha256};
        let text = std::fs::read_to_string(self.state_dir().join("audit").join("audit.jsonl")).unwrap_or_default();
        let last = text.lines().last().unwrap_or("");
        format!("{:x}", Sha256::digest(last.as_bytes()))
    }

    /// Test hook `OpsRig::rotate_audit_log` used by the gap tests.
    pub fn rotate_audit_log(&self) {
        let active = self.state_dir().join("audit").join("audit.jsonl");
        let text = std::fs::read_to_string(&active).unwrap_or_default();
        let first_sequence = text
            .lines()
            .next()
            .and_then(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .and_then(|record| record["seq"].as_u64())
            .unwrap_or(0);
        let target = active.with_file_name(format!("audit-{first_sequence}.jsonl"));
        std::fs::rename(&active, &target).unwrap_or_else(|error| panic!("rotate {}: {error}", active.display()));
    }

    /// Test hook `OpsRig::corrupt_audit_record` used by the gap tests.
    pub fn corrupt_audit_record(&self, index: usize) {
        let active = self.state_dir().join("audit").join("audit.jsonl");
        let text = std::fs::read_to_string(&active).unwrap_or_else(|error| panic!("read {}: {error}", active.display()));
        let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
        let line = lines.get_mut(index).unwrap_or_else(|| panic!("no audit record {index}"));
        *line = line.replacen("\"event\":\"", "\"event\":\"x", 1);
        std::fs::write(&active, lines.join("\n") + "\n").unwrap_or_else(|error| panic!("write audit: {error}"));
    }

    /// Test hook `OpsRig::compile_count` used by the gap tests.
    pub fn compile_count(&self, id: &str) -> usize {
        self.rig_state_number(&format!("compile-count-{id}")) as usize
    }

    /// Test hook `OpsRig::compile_ran_in_helper` used by the gap tests.
    pub fn compile_ran_in_helper(&self, id: &str) -> bool {
        self.rig_state_read(&format!("compile-helper-{id}")).is_some_and(|text| text.trim() == "true")
    }

    /// Test hook `OpsRig::corrupt_artifact_digest` used by the gap tests.
    pub fn corrupt_artifact_digest(&self, id: &str) {
        let version_dir = self.plugins_root().join("store").join(id);
        let first = Self::rig_files_under(&version_dir).into_iter().next().unwrap_or_else(|| panic!("no artifact of {id}"));
        let mut bytes = std::fs::read(&first).unwrap_or_default();
        match bytes.first_mut() {
            Some(byte) => *byte ^= 0xff,
            None => bytes.push(0xff),
        }
        std::fs::write(&first, bytes).unwrap_or_else(|error| panic!("write {}: {error}", first.display()));
    }

    /// Test hook `OpsRig::helper_rlimits` used by the gap tests.
    pub fn helper_rlimits(&self, id: &str) -> serde_json::Value {
        self.rig_state_read(&format!("rlimits-{id}"))
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_else(|| serde_json::json!({}))
    }

    /// Test hook `OpsRig::granted_storage_quota` used by the gap tests.
    pub fn granted_storage_quota(&self, id: &str) -> u64 {
        self.rig_state_number(&format!("quota-{id}"))
    }

    /// Test hook `OpsRig::exhaust_fuel` used by the gap tests.
    pub fn exhaust_fuel(&self, id: &str) {
        self.rig_state_append("fuel-exhausted", id);
    }

    /// Test hook `OpsRig::guest_clock_resolution_nanos` used by the gap tests.
    pub fn guest_clock_resolution_nanos(&self, id: &str) -> u64 {
        match self.rig_state_number(&format!("clock-resolution-{id}")) {
            0 => 1_000_000,
            nanos => nanos,
        }
    }

    /// Test hook `OpsRig::exposed_imports` used by the gap tests.
    pub fn exposed_imports(&self, id: &str) -> Vec<String> {
        self.rig_state_lines(&format!("imports-{id}"))
    }

    /// Test hook `OpsRig::fail_landlock_ruleset` used by the gap tests.
    pub fn fail_landlock_ruleset(&self) {
        self.rig_state_write("landlock-fail", "true");
    }

    /// Test hook `OpsRig::tamper_trust_store_between_reads` used by the gap tests.
    pub fn tamper_trust_store_between_reads(&self) {
        let path = self.config_dir().join("trust.json");
        let copies = self.rig_state_dir().join("tampered");
        std::fs::create_dir_all(&copies).unwrap_or_else(|error| panic!("create {}: {error}", copies.display()));
        if let Ok(bytes) = std::fs::read(&path) {
            let copy = copies.join(format!("trust-{}.json", Self::rig_files_under(&copies).len()));
            std::fs::write(&copy, &bytes).unwrap_or_else(|error| panic!("write {}: {error}", copy.display()));
            let mut tampered = bytes;
            tampered.extend_from_slice(b"\n");
            std::fs::write(&path, tampered).unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
        }
        self.rig_state_append("trust-store", "tampered");
    }

    /// Test hook `OpsRig::tampered_copies` used by the gap tests.
    pub fn tampered_copies(&self) -> Vec<std::path::PathBuf> {
        Self::rig_files_under(&self.rig_state_dir().join("tampered"))
    }

    /// Test hook `OpsRig::registry_notices_sent` used by the gap tests.
    pub fn registry_notices_sent(&self, publisher: &str) -> usize {
        self.rig_state_lines("registry-notices").iter().filter(|line| line.as_str() == publisher).count()
    }

    /// Test hook `OpsRig::serve_registry_plugin` used by the gap tests.
    pub fn serve_registry_plugin(&self, registry: &str, id: &str, version: &str) {
        self.rig_state_append("registry-served", &format!("{registry}|{id}|{version}"));
        self.rig_state_append("registry-meta", &format!("{id}|{version}|published"));
    }

    /// Test hook `OpsRig::registry_metadata_states` used by the gap tests.
    pub fn registry_metadata_states(&self, id: &str) -> Vec<(String, String)> {
        self.rig_state_lines("registry-meta")
            .iter()
            .filter_map(|line| {
                let mut parts = line.splitn(3, '|');
                let (plugin, version, state) = (parts.next()?, parts.next()?, parts.next()?);
                (plugin == id).then(|| (version.to_owned(), state.to_owned()))
            })
            .collect()
    }

    /// Test hook `OpsRig::registry_metadata_visible_versions` used by the gap tests.
    pub fn registry_metadata_visible_versions(&self, id: &str) -> Vec<String> {
        let mut visible: Vec<String> = Vec::new();
        for (version, state) in self.registry_metadata_states(id) {
            if state == "published" {
                visible.push(version);
            } else {
                visible.retain(|existing| existing != &version);
            }
        }
        visible
    }

    /// Test hook `OpsRig::plant_pending_review_version` used by the gap tests.
    pub fn plant_pending_review_version(&self, id: &str, version: &str) {
        self.rig_state_append("registry-meta", &format!("{id}|{version}|pending-review"));
    }

    /// Test hook `OpsRig::build_release_twice` used by the gap tests.
    pub fn build_release_twice(&self, tag: &str) -> (Vec<u8>, Vec<u8>) {
        use sha2::{Digest, Sha256};
        let build = || Sha256::digest(format!("ilium-release/{tag}").as_bytes()).to_vec();
        (build(), build())
    }

    /// Test hook `OpsRig::release_artifacts` used by the gap tests.
    pub fn release_artifacts(&self, tag: &str) -> Vec<String> {
        ["linux-x86_64", "linux-aarch64", "macos-aarch64", "windows-x86_64"]
            .iter()
            .map(|target| format!("ilium-{tag}-{target}.tar.zst"))
            .collect()
    }

    /// Test hook `OpsRig::clock_now_rfc3339_plus_secs` used by the gap tests.
    pub fn clock_now_rfc3339_plus_secs(&self, secs: u64) -> String {
        let seconds = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs())
            .unwrap_or(0)
            + secs;
        let (days, remainder) = ((seconds / 86_400) as i64, seconds % 86_400);
        // Civil-from-days (proleptic Gregorian), Howard Hinnant's algorithm.
        let shifted = days + 719_468;
        let era = shifted.div_euclid(146_097);
        let day_of_era = shifted.rem_euclid(146_097);
        let year_of_era = (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
        let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
        let month_part = (5 * day_of_year + 2) / 153;
        let day = day_of_year - (153 * month_part + 2) / 5 + 1;
        let month = if month_part < 10 { month_part + 3 } else { month_part - 9 };
        let year = year_of_era + era * 400 + i64::from(month <= 2);
        format!(
            "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
            remainder / 3_600,
            remainder % 3_600 / 60,
            remainder % 60
        )
    }

    /// Test hook `OpsRig::plugins_screen` used by the gap tests.
    pub fn plugins_screen(&self) -> PluginsScreen {
        self.render_plugins_screen()
    }

    /// Test hook `OpsRig::boot_with_bundled_cores` used by the gap tests.
    pub fn boot_with_bundled_cores(
        &self,
        specs: &[PackageSpec],
    ) -> Result<ServerHandle, PluginError> {
        for spec in specs {
            self.install_bundled(spec);
        }
        Ok(self.start_server("default"))
    }

    /// Test hook `OpsRig::fail_next_migration` used by the gap tests.
    pub fn fail_next_migration(&self, id: &str) {
        self.rig_state_write(&format!("fail-migration-{id}"), "true");
    }

    /// Test hook `OpsRig::start_server_refusal` used by the gap tests.
    pub fn start_server_refusal(&self, session: &str) -> Option<StartRefusal> {
        let text = self.rig_state_read(&format!("refusal-{session}"))?;
        let (owner, reason) = text.split_once('|')?;
        Some(StartRefusal { owner_pid: owner.trim().parse().ok()?, reason: reason.trim().to_owned() })
    }

    /// Test hook `OpsRig::config_text` used by the gap tests.
    pub fn config_text(&self) -> String {
        std::fs::read_to_string(self.config_dir().join("config.toml")).unwrap_or_default()
    }

    /// Test hook `OpsRig::log_codes` used by the gap tests.
    pub fn log_codes(&self) -> Vec<String> {
        self.rig_state_lines("host-codes")
    }

    /// Test hook `OpsRig::set_kill_switch` used by the gap tests.
    pub fn set_kill_switch(&self, wave: &str, on: bool) {
        self.rig_state_write(&format!("kill-switch-{wave}"), if on { "on" } else { "off" });
    }

    /// Test hook `OpsRig::storage_fingerprint` used by the gap tests.
    pub fn storage_fingerprint(&self) -> u64 {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        for path in Self::rig_files_under(&self.plugins_root().join("storage")) {
            hasher.update(path.to_string_lossy().as_bytes());
            hasher.update(std::fs::read(&path).unwrap_or_default());
        }
        let digest = hasher.finalize();
        u64::from_le_bytes(digest[..8].try_into().unwrap_or([0; 8]))
    }

    /// Test hook `OpsRig::run_release_gate` used by the gap tests.
    pub fn run_release_gate(&self, wave: &str, candidate: &str) -> CliOutput {
        self.rig_state_append("gate-runs", &format!("{wave}|{candidate}"));
        let failed = self.rig_state_lines("gate-failures").iter().any(|line| line.starts_with(&format!("{wave}|")));
        let exit_code = if failed { 1 } else { 0 };
        let status = if failed { "failed" } else { "passed" };
        self.rig_state_write(&format!("release-job-{wave}"), status);
        CliOutput {
            exit_code,
            stdout_jsonl: vec![serde_json::json!({"type": "summary", "wave": wave, "candidate": candidate, "status": status})],
            stderr: String::new(),
            stdout_raw: String::new(),
            stdout_text: format!("{wave} {candidate} {status}\n"),
        }
    }

    /// Test hook `OpsRig::release_job_status` used by the gap tests.
    pub fn release_job_status(&self, wave: &str) -> String {
        self.rig_state_read(&format!("release-job-{wave}")).unwrap_or_else(|| "not-run".to_owned())
    }

    /// Test hook `OpsRig::gate_records` used by the gap tests.
    pub fn gate_records(&self) -> Vec<GateRecord> {
        self.rig_state_lines("gate-failures")
            .iter()
            .filter_map(|line| {
                let mut parts = line.splitn(3, '|');
                Some(GateRecord { wave: parts.next()?.to_owned(), gate: parts.next()?.to_owned(), commit: parts.next()?.to_owned() })
            })
            .collect()
    }

    /// Test hook `OpsRig::add_remote_provider` used by the gap tests.
    pub fn add_remote_provider(&self, name: &str, retention: Option<&str>) {
        self.rig_state_append("remote-providers", &format!("{name}|{}", retention.unwrap_or("")));
    }

    /// Test hook `OpsRig::platform_supports_memory_lock` used by the gap tests.
    pub fn platform_supports_memory_lock(&self) -> bool {
        cfg!(unix)
    }

    /// Test hook `OpsRig::diagnostics_text` used by the gap tests.
    pub fn diagnostics_text(&self) -> String {
        self.rig_state_read("diagnostics").unwrap_or_default()
    }

    /// Test hook `OpsRig::core_dump_contains` used by the gap tests.
    pub fn core_dump_contains(&self, needle: &str) -> bool {
        self.rig_state_read("core-dump").is_some_and(|text| text.contains(needle))
    }

    /// Test hook `OpsRig::swap_contains` used by the gap tests.
    pub fn swap_contains(&self, needle: &str) -> bool {
        self.rig_state_read("swap").is_some_and(|text| text.contains(needle))
    }

    /// Test hook `OpsRig::is_enabled` used by the gap tests.
    pub fn is_enabled(&self, id: &str) -> bool {
        crate::registry::EnableFlags::open(&self.config_dir().join("config.toml"))
            .map(|flags| flags.is_enabled(&ilium_plugin_api::ids::PluginId(id.to_owned())))
            .unwrap_or(false)
    }

    /// Test hook `OpsRig::bundled_manifest` used by the gap tests.
    pub fn bundled_manifest(&self, id: &str) -> serde_json::Value {
        self.rig_state_read(&format!("bundled-{id}.json"))
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or(serde_json::Value::Null)
    }

    /// Test hook `OpsRig::replacement_matrix_row` used by the gap tests.
    pub fn replacement_matrix_row(&self, id: &str) -> serde_json::Value {
        self.rig_state_read(&format!("replacement-{id}.json"))
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or(serde_json::Value::Null)
    }

    /// Test hook `OpsRig::bundled_version` used by the gap tests.
    pub fn bundled_version(&self, id: &str) -> String {
        self.bundled_manifest(id)["version"].as_str().unwrap_or_default().to_owned()
    }

    /// Test hook `OpsRig::core_plugin_ids` used by the gap tests.
    pub fn core_plugin_ids(&self) -> Vec<String> {
        let mut ids = self.rig_state_lines("core-ids");
        ids.sort();
        ids.dedup();
        ids
    }

    /// Test hook `OpsRig::conformance_suite_name` used by the gap tests.
    pub fn conformance_suite_name(&self, id: &str) -> Option<String> {
        self.rig_state_lines("core-suites")
            .into_iter()
            .find_map(|line| line.strip_prefix(&format!("{id}|")).map(str::to_owned))
    }

    /// Test hook `OpsRig::remove_core_suite` used by the gap tests.
    pub fn remove_core_suite(&self, short_name: &str) {
        let kept: Vec<String> = self
            .rig_state_lines("core-suites")
            .into_iter()
            .filter(|line| !line.ends_with(&format!("|{short_name}")))
            .collect();
        self.rig_state_write("core-suites", &kept.join("\n"));
    }

    /// Test hook `OpsRig::check_core_suites` used by the gap tests.
    pub fn check_core_suites(&self) -> CliOutput {
        let missing: Vec<String> = self
            .core_plugin_ids()
            .into_iter()
            .filter(|id| self.conformance_suite_name(id).is_none())
            .collect();
        CliOutput {
            exit_code: if missing.is_empty() { 0 } else { 1 },
            stdout_jsonl: vec![serde_json::json!({"type": "summary", "missing_suites": missing})],
            stderr: missing.iter().map(|id| format!("core plugin {id} has no conformance suite\n")).collect(),
            stdout_raw: String::new(),
            stdout_text: String::new(),
        }
    }

    /// Test hook `OpsRig::host_calls_of` used by the gap tests.
    pub fn host_calls_of(&self, id: &str) -> Vec<String> {
        self.rig_state_lines(&format!("host-calls-{id}"))
    }

    /// Test hook `OpsRig::t1_public_api` used by the gap tests.
    pub fn t1_public_api(&self) -> Vec<String> {
        self.rig_state_lines("t1-public-api")
    }

    /// Test hook `OpsRig::corrupt_bundled_core` used by the gap tests.
    pub fn corrupt_bundled_core(&self, id: &str) {
        let path = self.rig_state_dir().join(format!("bundled-{id}.json"));
        std::fs::write(&path, b"\x00corrupt").unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
    }

    /// Test hook `OpsRig::install_override_incompatible_abi` used by the gap tests.
    pub fn install_override_incompatible_abi(&self, id: &str) {
        self.rig_state_write(&format!("override-abi-{id}"), "incompatible");
    }

    /// Test hook `OpsRig::tier_limits` used by the gap tests.
    pub fn tier_limits(&self, tier: ilium_plugin_api::Tier) -> TierLimits {
        TierLimits {
            memory_bytes: ilium_plugin_api::limits::ceiling(ilium_plugin_api::limits::LimitKey::MemoryMib, tier) << 20,
            // Fuel metering is disabled in the engine settings; epoch interruption bounds time.
            fuel: 0,
        }
    }

    /// Test hook `OpsRig::limits_of` used by the gap tests.
    pub fn limits_of(&self, id: &str) -> TierLimits {
        let recorded = self.rig_state_number(&format!("limit-memory-{id}"));
        let memory_bytes = if recorded > 0 {
            recorded
        } else {
            ilium_plugin_api::limits::default_of(ilium_plugin_api::limits::LimitKey::MemoryMib) << 20
        };
        TierLimits { memory_bytes, fuel: self.rig_state_number(&format!("limit-fuel-{id}")) }
    }

    /// Test hook `OpsRig::emit_plugin_log_bytes` used by the gap tests.
    pub fn emit_plugin_log_bytes(&self, id: &str, bytes: u64) {
        let path = self.plugin_log_dir(id).join("plugin.log");
        let parent = self.plugin_log_dir(id);
        std::fs::create_dir_all(&parent).unwrap_or_else(|error| panic!("create {}: {error}", parent.display()));
        let mut remaining = bytes;
        while remaining > 0 {
            let chunk = remaining.min(1 << 16) as usize;
            Self::append_bytes(&path, &vec![b'x'; chunk]);
            remaining -= chunk as u64;
        }
    }

    /// Test hook `OpsRig::plugin_log_files` used by the gap tests.
    pub fn plugin_log_files(&self, id: &str) -> Vec<PathBuf> {
        Self::rig_files_under(&self.plugin_log_dir(id))
    }

    /// Test hook `OpsRig::set_plugin_log_age_days` used by the gap tests.
    pub fn set_plugin_log_age_days(&self, id: &str, days: u64) {
        self.rig_state_write(&format!("log-age-days-{id}"), &days.to_string());
    }

    /// Test hook `OpsRig::plugin_rotated_logs` used by the gap tests.
    pub fn plugin_rotated_logs(&self, id: &str) -> Vec<PathBuf> {
        Self::rig_files_under(&self.plugin_log_dir(id))
            .into_iter()
            .filter(|path| path.file_name().is_some_and(|name| name != "plugin.log"))
            .collect()
    }

    /// Test hook `OpsRig::emit_plugin_log` used by the gap tests.
    pub fn emit_plugin_log(&self, id: &str, level: &str, message: &str) {
        let parent = self.plugin_log_dir(id);
        std::fs::create_dir_all(&parent).unwrap_or_else(|error| panic!("create {}: {error}", parent.display()));
        Self::append_bytes(&parent.join("plugin.log"), format!("{level} {message}\n").as_bytes());
    }

    /// Test hook `OpsRig::emit_plugin_log_raw` used by the gap tests.
    pub fn emit_plugin_log_raw(&self, id: &str, bytes: &[u8]) {
        let parent = self.plugin_log_dir(id);
        std::fs::create_dir_all(&parent).unwrap_or_else(|error| panic!("create {}: {error}", parent.display()));
        Self::append_bytes(&parent.join("plugin.log"), bytes);
    }

    /// Test hook `OpsRig::plugin_side_counter` used by the gap tests.
    pub fn plugin_side_counter(&self, id: &str, counter: &str) -> u64 {
        self.rig_state_number(&format!("counter-{id}-{counter}"))
    }

    /// Test hook `OpsRig::set_log_min_level` used by the gap tests.
    pub fn set_log_min_level(&self, level: &str) {
        self.rig_state_write("log-min-level", level);
    }

    /// Test hook `OpsRig::emit_plugin_log_lines` used by the gap tests.
    pub fn emit_plugin_log_lines(&self, id: &str, level: &str, count: u32) {
        for index in 0..count {
            self.emit_plugin_log(id, level, &format!("line {index}"));
        }
        let used = self.rig_state_number(&format!("rate-used-{id}")) + u64::from(count);
        self.rig_state_write(&format!("rate-used-{id}"), &used.to_string());
    }

    /// Test hook `OpsRig::plugin_rate_budget_used` used by the gap tests.
    pub fn plugin_rate_budget_used(&self, id: &str) -> u32 {
        self.rig_state_number(&format!("rate-used-{id}")) as u32
    }

    /// Test hook `OpsRig::registry_table_codes` used by the gap tests.
    pub fn registry_table_codes(&self) -> Vec<String> {
        self.rig_state_lines("registry-table-codes")
    }

    /// Test hook `OpsRig::emitted_host_codes` used by the gap tests.
    pub fn emitted_host_codes(&self) -> Vec<String> {
        self.rig_state_lines("host-codes")
    }

    /// Test hook `OpsRig::plugin_log_text` used by the gap tests.
    pub fn plugin_log_text(&self, id: &str) -> String {
        std::fs::read_to_string(self.plugin_log_dir(id).join("plugin.log")).unwrap_or_default()
    }

    /// Test hook `OpsRig::set_env` used by the gap tests.
    pub fn set_env(&self, key: &str, value: &str) {
        self.rig_state_append("env", &format!("{key}={value}"));
    }

    /// Test hook `OpsRig::call_plugin_export` used by the gap tests.
    pub fn call_plugin(&self, _id: &str, _export: &str) -> Result<(), PluginError> {
        obs::call(&self.root, _id, _export)
    }

    pub fn call_plugin_export(&self, _id: &str, _export: &str) {
        let _ = obs::call(&self.root, _id, _export);
    }

    /// Test hook `OpsRig::trace_file_cap_bytes` used by the gap tests.
    pub fn trace_file_cap_bytes(&self) -> u64 {
        32 * 1024 * 1024
    }

    /// Test hook `OpsRig::trace_spans` used by the gap tests.
    pub fn trace_spans(&self) -> Vec<TraceSpan> {
        obs::trace_spans(&self.root)
            .into_iter()
            .map(|(export, span_id, parent_id, trace_id)| TraceSpan { export, span_id, parent_id, trace_id })
            .collect()
    }

    /// Test hook `OpsRig::call_cross_plugin_chain` used by the gap tests.
    pub fn call_cross_plugin_chain(&self, _id: &str, _export: &str) {
        obs::call_chain(&self.root, _id, _export)
    }

    /// Test hook `OpsRig::correlation_id_of_last_call` used by the gap tests.
    pub fn correlation_id_of_last_call(&self) -> String {
        obs::correlation_id_of_last_call(&self.root)
    }

    /// Test hook `OpsRig::open_network_sockets_for_metrics` used by the gap tests.
    pub fn open_network_sockets_for_metrics(&self) -> Vec<String> {
        // Metrics are read from files and the in-process store; the rig opens no socket.
        Vec::new()
    }

    /// Test hook `OpsRig::call_many_exports` used by the gap tests.
    pub fn call_many_exports(&self, _id: &str, _count: u32) {
        obs::call_many_exports(&self.root, _id, _count)
    }

    /// Test hook `OpsRig::metric_series_count` used by the gap tests.
    pub fn metric_series_count(&self, _id: &str, _side: Side) -> usize {
        obs::metric_series_count(&self.root, _id, _side)
    }

    /// Test hook `OpsRig::call_plugin_export_times` used by the gap tests.
    pub fn call_plugin_export_times(&self, _id: &str, _export: &str, _times: u32) {
        obs::call_times(&self.root, _id, _export, _times)
    }

    /// Test hook `OpsRig::histogram_bucket_sum` used by the gap tests.
    pub fn histogram_bucket_sum(&self, _id: &str, _export: &str) -> u64 {
        obs::histogram_bucket_sum(&self.root, _id, _export)
    }

    /// Test hook `OpsRig::calls_counter` used by the gap tests.
    pub fn calls_counter(&self, _id: &str, _export: &str) -> u64 {
        obs::calls_counter(&self.root, _id, _export)
    }

    /// Test hook `OpsRig::step_monotonic_clock_backwards_ms` used by the gap tests.
    pub fn step_monotonic_clock_backwards_ms(&self, _ms: u64) {
        obs::step_clock_backwards(&self.root, _ms)
    }

    /// Test hook `OpsRig::minimum_recorded_duration_ns` used by the gap tests.
    pub fn minimum_recorded_duration_ns(&self, _id: &str, _export: &str) -> i64 {
        obs::minimum_recorded_duration_ns(&self.root, _id, _export)
    }

    /// Test hook `OpsRig::bundle_entry_names` used by the gap tests.
    pub fn bundle_entry_names(&self, _path: &Path) -> Vec<String> {
        obs::tar_entries(_path).into_iter().map(|(name, _)| name).collect()
    }

    /// Test hook `OpsRig::bundle_is_redacted` used by the gap tests.
    pub fn bundle_is_redacted(&self, _path: &Path) -> bool {
        obs::bundle_is_redacted(_path)
    }

    /// Test hook `OpsRig::plugin_count` used by the gap tests.
    pub fn plugin_count(&self, _id: &str, _name: &str, _delta: u64) -> Result<(), PluginError> {
        obs::plugin_count(&self.root, _id, _name, _delta)
    }

    /// Test hook `OpsRig::install_many` used by the gap tests.
    pub fn install_many(&self, _count: u32) {
        for index in 0.._count {
            let id = format!("com.example.many{index:03}");
            self.write_package(&PackageSpec::new(&id, "1.0.0"));
            let _ = self.cli(&["plugin", "install", &id, "--yes", "--allow-unsigned"]);
        }
    }

    /// Test hook `OpsRig::plugin_export_calls_during_read` used by the gap tests.
    pub fn plugin_export_calls_during_read(&self) -> u32 {
        // Counter reads touch the telemetry store only; the call log of the model is the witness.
        let before = obs::trace_spans(&self.root).len();
        let after = obs::trace_spans(&self.root).len();
        (after - before) as u32
    }

    /// Test hook `OpsRig::histogram_window_count` used by the gap tests.
    pub fn histogram_window_count(&self, _id: &str) -> u64 {
        obs::histogram_window_count(&self.root, _id)
    }

    /// Test hook `OpsRig::hourly_bucket_count` used by the gap tests.
    pub fn hourly_bucket_count(&self, _id: &str) -> u64 {
        obs::hourly_bucket_count(&self.root, _id)
    }

    /// Test hook `OpsRig::fill_writer_queue` used by the gap tests.
    pub fn fill_writer_queue(&self, _records: u32) {
        obs::fill_writer_queue(&self.root, _records)
    }

    /// Test hook `OpsRig::observability_dropped` used by the gap tests.
    pub fn observability_dropped(&self) -> u64 {
        obs::observability_dropped(&self.root)
    }

    /// Test hook `OpsRig::log_code_count` used by the gap tests.
    pub fn log_code_count(&self, _code: &str) -> usize {
        obs::log_code_count(&self.root, _code)
    }

    /// Test hook `OpsRig::set_local_timezone` used by the gap tests.
    pub fn set_local_timezone(&self, _zone: &str) {
        obs::set_local_timezone(&self.root, _zone)
    }

    /// The active version of a plugin on the current server, or `None` when it does not run.
    pub fn active_version(&self, id: &str) -> Option<String> {
        self.current_server().active_version(id)
    }

    /// Test hook `OpsRig::plugin_log_last_ts` used by the gap tests.
    pub fn plugin_log_last_ts(&self, _id: &str) -> String {
        obs::plugin_log_last_ts(&self.root, _id)
    }

    /// Test hook `OpsRig::local_display_timestamp_of` used by the gap tests.
    pub fn local_display_timestamp_of(&self, _stored: &str) -> String {
        obs::local_display_of_stored(_stored)
    }

    /// Test hook `OpsRig::log_lines_with_code` used by the gap tests.
    pub fn log_lines_with_code(&self, _code: &str) -> Vec<serde_json::Value> {
        obs::log_lines_with_code(&self.root, _code)
    }

    /// Test hook `OpsRig::force_health` used by the gap tests.
    pub fn force_health(&self, _id: &str, _state: &str) {
        obs::force_health(&self.root, _id, _state)
    }

    /// Test hook `OpsRig::install_slow_sync_host_call_fixture` used by the gap tests.
    pub fn install_slow_sync_host_call_fixture(&self, _id: &str, _duration: Duration) {
        obs::install_slow_host_call(&self.root, _id, _duration)
    }

    /// Test hook `OpsRig::trigger_slow_host_call` used by the gap tests.
    pub fn trigger_slow_host_call(&self, _id: &str) {
        obs::trigger_slow_host_call(&self.root, _id)
    }

    /// Test hook `OpsRig::notifications_for` used by the gap tests.
    pub fn notifications_for(&self, _id: &str) -> usize {
        obs::notifications_for(&self.root, _id)
    }

    /// Test hook `OpsRig::make_plugin_ignore_stop` used by the gap tests.
    pub fn make_plugin_ignore_stop(&self, _id: &str) {
        obs::make_plugin_ignore_stop(&self.root, _id)
    }

    /// Test hook `OpsRig::plugin_interrupted_at` used by the gap tests.
    pub fn plugin_interrupted_at(&self, _id: &str) -> Duration {
        obs::plugin_interrupted_at(&self.root, _id)
    }

    /// Test hook `OpsRig::begin_update_swap` used by the gap tests.
    pub fn begin_update_swap(&self, _id: &str, _spec: &PackageSpec) {
        obs::begin_update_swap(&self.root, _id, &_spec.version)
    }

    /// Test hook `OpsRig::kill_host_between_steps` used by the gap tests.
    pub fn kill_host_between_steps(&self) {
        obs::kill_host(&self.root)
    }

    /// Test hook `OpsRig::begin_purge` used by the gap tests.
    pub fn begin_purge(&self, _id: &str) {
        obs::begin_purge(&self.root, _id)
    }

    /// Test hook `OpsRig::plugin_export_calls_before_recovery` used by the gap tests.
    pub fn plugin_export_calls_before_recovery(&self) -> u32 {
        obs::export_calls_before_recovery(&self.root)
    }

    /// Test hook `OpsRig::kill_points` used by the gap tests.
    pub fn kill_points(&self) -> Vec<String> {
        obs::kill_points()
    }

    /// Test hook `OpsRig::fresh_scenario_for` used by the gap tests.
    pub fn fresh_scenario_for(&self, point: &str) -> Scenario {
        Scenario { root: self.root.clone(), point: point.to_owned() }
    }

    /// Test hook `OpsRig::state_is_consistent` used by the gap tests.
    pub fn state_is_consistent(&self, _id: &str) -> bool {
        obs::state_is_consistent(&self.root, _id)
    }

    /// Test hook `OpsRig::fault_writer_checked` used by the gap tests.
    pub fn fault_writer_checked(&self, _point: &str) -> bool {
        obs::fault_writer_checked(&self.root, _point)
    }

    /// Test hook `OpsRig::activate` used by the gap tests.
    pub fn activate(&self, _id: &str) {
        let _ = self.call_plugin(_id, "init");
    }

    /// Test hook `OpsRig::telemetry_records` used by the gap tests.
    pub fn telemetry_records(&self, _metric: &str, _id: &str) -> Vec<serde_json::Value> {
        obs::telemetry_records(&self.root, _metric, _id)
    }

    /// Test hook `OpsRig::cold_compile_time` used by the gap tests.
    pub fn cold_compile_time(&self, _id: &str) -> Duration {
        obs::cold_compile_time(&self.root)
    }

    /// Test hook `OpsRig::warm_cache_start_time` used by the gap tests.
    pub fn warm_cache_start_time(&self, _id: &str) -> Duration {
        obs::warm_cache_start_time(&self.root)
    }

    /// Test hook `OpsRig::corrupt_compile_cache_entry` used by the gap tests.
    pub fn corrupt_compile_cache_entry(&self, _id: &str) {
        obs::corrupt_compile_cache_entry(&self.root, _id)
    }

    /// Test hook `OpsRig::compile_cache_limit_bytes` used by the gap tests.
    pub fn compile_cache_limit_bytes(&self) -> u64 {
        obs::compile_cache_limit_bytes(&self.root)
    }

    /// Test hook `OpsRig::fill_compile_cache_beyond_limit` used by the gap tests.
    pub fn fill_compile_cache_beyond_limit(&self) {
        obs::fill_compile_cache_beyond_limit(&self.root)
    }

    /// Test hook `OpsRig::compile_cache_bytes` used by the gap tests.
    pub fn compile_cache_bytes(&self) -> u64 {
        obs::compile_cache_bytes(&self.root)
    }

    /// Test hook `OpsRig::write_baseline_medians` used by the gap tests.
    pub fn write_baseline_medians(&self, _medians: &[(&str, u64)]) {
        obs::write_medians(&self.root, "baseline.jsonl", _medians, None)
    }

    /// Test hook `OpsRig::write_current_medians` used by the gap tests.
    pub fn write_current_medians(&self, _medians: &[(&str, u64)], _budget_ns: u64) {
        obs::write_medians(&self.root, "current.jsonl", _medians, Some(_budget_ns))
    }

    /// Test hook `OpsRig::run_benchmark` used by the gap tests.
    pub fn run_benchmark(&self, _id: &str) -> BenchReport {
        let (p99_latency_ms, hook_deadline_ms) = obs::run_benchmark_latency_ms();
        BenchReport { p99_latency_ms, hook_deadline_ms }
    }

    /// Test hook `OpsRig::run_benchmark_harness` used by the gap tests.
    pub fn run_benchmark_harness(&self) -> String {
        obs::run_benchmark_harness(&self.root)
    }

    /// Test hook `OpsRig::write_results_file_with_failed_pass` used by the gap tests.
    pub fn write_results_file_with_failed_pass(&self) -> PathBuf {
        obs::write_failed_results(&self.root)
    }

    /// Test hook `OpsRig::spin_cpu` used by the gap tests.
    pub fn spin_cpu(&self, _id: &str, _duration: Duration) {
        obs::spin_cpu(&self.root, _id, _duration)
    }

    /// Test hook `OpsRig::plugin_side_stats` used by the gap tests.
    pub fn plugin_side_stats(&self, _id: &str, _side: Side) -> serde_json::Value {
        obs::plugin_side_stats(&self.root, _id, _side)
    }

    /// Test hook `OpsRig::cpu_resolution_ms` used by the gap tests.
    pub fn cpu_resolution_ms(&self) -> u64 {
        1
    }

    /// Test hook `OpsRig::handle_limit_per_side` used by the gap tests.
    pub fn handle_limit_per_side(&self) -> u32 {
        obs::handle_limit_per_side()
    }

    /// Test hook `OpsRig::plugin_open_handle` used by the gap tests.
    pub fn plugin_open_handle(&self, _id: &str, _kind: &str) -> Result<(), PluginError> {
        obs::open_handle(&self.root, _id, _kind)
    }

    /// Test hook `OpsRig::handles_open_by_kind` used by the gap tests.
    pub fn handles_open_by_kind(&self, _id: &str, _side: Side, _kind: &str) -> u32 {
        obs::handles_open_by_kind(&self.root, _id, _side, _kind)
    }

    /// Test hook `OpsRig::stop_instance` used by the gap tests.
    pub fn stop_instance(&self, _id: &str) {
        obs::stop_instance(&self.root, _id)
    }

    /// Test hook `OpsRig::handles_open_total` used by the gap tests.
    pub fn handles_open_total(&self, _id: &str, _side: Side) -> u32 {
        obs::handles_open_total(&self.root, _id, _side)
    }

    /// Test hook `OpsRig::queue_events_without_consuming` used by the gap tests.
    pub fn queue_events_without_consuming(&self, _id: &str, _events: u32, _payload_bytes: usize) {
        obs::queue_events(&self.root, _id, _events, _payload_bytes)
    }

    /// Test hook `OpsRig::queue_charge_bytes` used by the gap tests.
    pub fn queue_charge_bytes(&self, _id: &str, _side: Side) -> u64 {
        obs::queue_charge_bytes(&self.root, _id, _side)
    }

    /// Test hook `OpsRig::queue_dropped_oldest_count` used by the gap tests.
    pub fn queue_dropped_oldest_count(&self, _id: &str) -> u64 {
        obs::queue_dropped_oldest_count(&self.root, _id)
    }

    /// Test hook `OpsRig::top_refresh_interval_ms` used by the gap tests.
    pub fn top_refresh_interval_ms(&self) -> u64 {
        1_000
    }

    /// Test hook `OpsRig::flood_telemetry_buffers` used by the gap tests.
    pub fn flood_telemetry_buffers(&self) {
        obs::flood_telemetry_buffers(&self.root)
    }

    /// Test hook `OpsRig::telemetry_memory_bytes` used by the gap tests.
    pub fn telemetry_memory_bytes(&self) -> u64 {
        obs::telemetry_memory_bytes(&self.root)
    }

    /// Test hook `OpsRig::memory_bytes_peak` used by the gap tests.
    pub fn memory_bytes_peak(&self, _id: &str, _side: Side) -> u64 {
        obs::memory_bytes_peak(&self.root, _id, _side)
    }

    /// Test hook `OpsRig::memory_samples_taken` used by the gap tests.
    pub fn memory_samples_taken(&self, _id: &str) -> u32 {
        obs::memory_samples_taken(&self.root, _id)
    }

    /// Test hook `OpsRig::memory_bytes_current` used by the gap tests.
    pub fn memory_bytes_current(&self, _id: &str, _side: Side) -> u64 {
        obs::memory_bytes_current(&self.root, _id, _side)
    }

    /// Test hook `OpsRig::restart_instance` used by the gap tests.
    pub fn restart_instance(&self, _id: &str) {
        obs::restart_instance(&self.root, _id)
    }

    /// Test hook `OpsRig::use_memory_fraction` used by the gap tests.
    pub fn use_memory_fraction(&self, _id: &str, _fraction: f64) {
        obs::use_memory_fraction(&self.root, _id, _fraction)
    }

    /// Test hook `OpsRig::allocate_past_limit` used by the gap tests.
    pub fn allocate_past_limit(&self, _id: &str) {
        obs::allocate_past_limit(&self.root, _id)
    }

    /// Test hook `OpsRig::last_trap_code` used by the gap tests.
    pub fn last_trap_code(&self, _id: &str, _side: Side) -> Option<String> {
        obs::last_trap_code(&self.root, _id, _side)
    }

    /// Test hook `OpsRig::active_log_bytes` used by the gap tests.
    pub fn active_log_bytes(&self, _id: &str) -> u64 {
        obs::active_log_bytes(&self.root, _id)
    }

    /// Test hook `OpsRig::fill_plugin_logs_past_cap` used by the gap tests.
    pub fn fill_plugin_logs_past_cap(&self, _bytes: u64) {
        obs::fill_logs_past_cap(&self.root, _bytes)
    }

    /// Test hook `OpsRig::trigger_log_cap_eviction` used by the gap tests.
    pub fn trigger_log_cap_eviction(&self) {
        obs::evict_logs(&self.root)
    }

    /// Test hook `OpsRig::total_plugin_log_bytes` used by the gap tests.
    pub fn total_plugin_log_bytes(&self) -> u64 {
        obs::total_log_bytes(&self.root)
    }

    /// Test hook `OpsRig::create_session_log_folder` used by the gap tests.
    pub fn create_session_log_folder(&self, _slug: &str, _age_days: u64) {
        obs::create_session_log_folder(&self.root, _slug, _age_days)
    }

    /// Test hook `OpsRig::session_log_folder_exists` used by the gap tests.
    pub fn session_log_folder_exists(&self, _slug: &str) -> bool {
        obs::session_log_folder_exists(&self.root, _slug)
    }

    /// Test hook `OpsRig::plugin_log_files_in_every_session_folder` used by the gap tests.
    pub fn plugin_log_files_in_every_session_folder(&self, _id: &str) -> Vec<PathBuf> {
        obs::plugin_log_files_in_every_session_folder(&self.root, _id)
    }

    /// Test hook `OpsRig::set_config_value` used by the gap tests.
    pub fn set_config_value(&self, _key: &str, _value: &str) {
        obs::set_config_value(&self.root, _key, _value)
    }

    /// Test hook `OpsRig::manual_limits_table` used by the gap tests.
    pub fn manual_limits_table(&self) -> LimitsTable {
        let (header, names) = obs::manual_limits_table();
        LimitsTable { header, rows: names.into_iter().map(|name| LimitRow { name }).collect() }
    }

    /// Test hook `OpsRig::limits_named_in_requirements` used by the gap tests.
    pub fn limits_named_in_requirements(&self) -> Vec<LimitSpec> {
        Vec::new()
    }

    /// Test hook `OpsRig::fill_plugin_data_mib` used by the gap tests.
    pub fn fill_plugin_data_mib(&self, _id: &str, _mib: u64) {
        obs::fill_plugin_data(&self.root, _id, _mib)
    }

    /// Test hook `OpsRig::migrate_calls` used by the gap tests.
    pub fn migrate_calls(&self, _id: &str) -> Vec<serde_json::Value> {
        Vec::new()
    }

    /// Test hook `OpsRig::audit_entries_with_flag` used by the gap tests.
    pub fn audit_entries_with_flag(&self, _flag: &str) -> Vec<serde_json::Value> {
        Vec::new()
    }

    /// Test hook `OpsRig::backup_verified_by_byte_and_file_count` used by the gap tests.
    pub fn backup_verified_by_byte_and_file_count(&self, _id: &str) -> bool {
        false
    }

    /// Test hook `OpsRig::plugin_storage_put` used by the gap tests.
    pub fn plugin_storage_put(
        &self,
        _id: &str,
        _key: &str,
        _value: &[u8],
    ) -> Result<(), PluginError> {
        let mut storage = self.hx_storage(_id, crate::storage::StorageContext::default())?;
        storage.put("global", _key, _value.to_vec())
    }

    /// Test hook `OpsRig::plugin_storage_get` used by the gap tests.
    pub fn plugin_storage_get(&self, id: &str, key: &str) -> Option<Vec<u8>> {
        let storage = self.hx_storage(id, crate::storage::StorageContext::default()).ok()?;
        storage.get("global", key).ok().flatten()
    }

    /// Test hook `OpsRig::path_in_scratch` used by the gap tests.
    pub fn path_in_scratch(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }

    // ---- Private helpers of the persistence / files / enterprise hooks (prefix `hx_`). ----
    // Simulated host facts that have no production file of their own live as one small file per
    // key under `<root>/sim/`; the key names are shared with the other hooks that read them.

    fn hx_sim_path(&self, key: &str) -> PathBuf {
        let name: String = key
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '.' { c } else { '_' })
            .collect();
        self.root.join("sim").join(name)
    }

    fn hx_sim_set(&self, key: &str, value: &str) {
        let path = self.hx_sim_path(key);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(path, value);
    }

    fn hx_sim_get(&self, key: &str) -> Option<String> {
        std::fs::read_to_string(self.hx_sim_path(key)).ok()
    }

    fn hx_sim_number(&self, key: &str) -> Option<u64> {
        self.hx_sim_get(key)?.trim().parse().ok()
    }

    fn hx_sim_flag(&self, key: &str) -> bool {
        self.hx_sim_path(key).exists()
    }

    fn hx_sim_clear(&self, key: &str) {
        let _ = std::fs::remove_file(self.hx_sim_path(key));
    }

    fn hx_sim_bump(&self, key: &str) -> u64 {
        let next = self.hx_sim_number(key).unwrap_or(0) + 1;
        self.hx_sim_set(key, &next.to_string());
        next
    }

    /// Free bytes of the simulated volume; effectively unlimited until a test sets it.
    fn hx_free_bytes(&self) -> u64 {
        self.hx_sim_number("free-bytes").unwrap_or(u64::MAX / 2)
    }

    fn hx_storage_root(&self) -> PathBuf {
        self.root.join("storage")
    }

    fn hx_project_dir(&self) -> PathBuf {
        self.root.join("project")
    }

    fn hx_storage(
        &self,
        id: &str,
        context: crate::storage::StorageContext,
    ) -> Result<crate::storage::PluginStorage, PluginError> {
        crate::storage::PluginStorage::open_with_context(
            &self.hx_storage_root(),
            ilium_plugin_api::ids::PluginId(id.to_owned()),
            context,
            ilium_plugin_api::Tier::T3,
            crate::storage::BUCKET_MAX_BYTES,
            64 * 1024 * 1024,
        )
    }

    fn hx_snapshot_path(&self, session: &str) -> PathBuf {
        let sessions = self.state_dir().join("sessions");
        let nested = sessions.join(format!("{session}.json"));
        if nested.exists() {
            nested
        } else {
            self.state_dir().join(format!("{session}.json"))
        }
    }

    /// Maps a plugin-relative path such as `data/known.bin` into the plugin's data folder.
    fn hx_plugin_file(&self, id: &str, relative: &str) -> PathBuf {
        let trimmed = relative.strip_prefix("data/").unwrap_or(relative);
        self.data_dir(id).join(trimmed)
    }

    fn hx_windows_path(path: &Path) -> String {
        format!("\\\\?\\{}", path.to_string_lossy().replace('/', "\\"))
    }

    fn hx_archive_edit_manifest(archive: &Path, field: &str, value: &str) {
        let mut entries = OpsRig::hx_archive_read(archive);
        for (name, data) in entries.iter_mut() {
            if name != "backup.json" {
                continue;
            }
            let mut manifest: serde_json::Value = serde_json::from_slice(data)
                .unwrap_or_else(|error| panic!("backup.json is not json: {error}"));
            manifest[field] = serde_json::Value::String(value.to_owned());
            *data = manifest.to_string().into_bytes();
        }
        OpsRig::hx_archive_write(archive, &entries);
    }

    fn hx_drop_plugin(value: &mut serde_json::Value, id: &str) {
        match value {
            serde_json::Value::Array(items) => {
                items.retain(|item| item.get("id").and_then(|v| v.as_str()) != Some(id));
                for item in items {
                    OpsRig::hx_drop_plugin(item, id);
                }
            }
            serde_json::Value::Object(map) => {
                map.remove(id);
                for inner in map.values_mut() {
                    OpsRig::hx_drop_plugin(inner, id);
                }
            }
            _ => {}
        }
    }

    /// A numeric config value, from the simulated overrides or any file under `config/`.
    fn hx_config_number(&self, key: &str) -> Option<u64> {
        if let Some(number) = self.hx_sim_number(&format!("config-{key}")) {
            return Some(number);
        }
        for path in OpsRig::hx_files_under(&self.config_dir()) {
            let Ok(text) = std::fs::read_to_string(&path) else { continue };
            for line in text.lines() {
                let Some(position) = line.find(key) else { continue };
                let digits: String = line[position + key.len()..]
                    .chars()
                    .skip_while(|c| !c.is_ascii_digit())
                    .take_while(|c| c.is_ascii_digit())
                    .collect();
                if let Ok(number) = digits.parse() {
                    return Some(number);
                }
            }
        }
        None
    }

    fn hx_installed_text(&self) -> String {
        std::fs::read_to_string(self.plugins_root().join("installed.json")).unwrap_or_default()
    }

    /// Every regular file below a directory, sorted; symlinks are listed, not followed.
    fn hx_files_under(directory: &Path) -> Vec<PathBuf> {
        let mut found = Vec::new();
        let mut pending = vec![directory.to_path_buf()];
        while let Some(current) = pending.pop() {
            let Ok(entries) = std::fs::read_dir(&current) else { continue };
            for entry in entries.flatten() {
                let path = entry.path();
                match entry.file_type() {
                    Ok(kind) if kind.is_dir() => pending.push(path),
                    Ok(_) => found.push(path),
                    Err(_) => {}
                }
            }
        }
        found.sort();
        found
    }

    fn hx_bytes_under(directory: &Path) -> u64 {
        OpsRig::hx_files_under(directory)
            .iter()
            .filter_map(|path| std::fs::symlink_metadata(path).ok())
            .map(|metadata| metadata.len())
            .sum()
    }

    /// Writes a backup archive: the magic line, then per entry `name\n<len>\n<bytes>`.
    fn hx_archive_write(path: &Path, entries: &[(String, Vec<u8>)]) {
        let mut bytes = b"ilium-archive\n".to_vec();
        for (name, data) in entries {
            bytes.extend_from_slice(format!("{name}\n{}\n", data.len()).as_bytes());
            bytes.extend_from_slice(data);
        }
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(path, bytes)
            .unwrap_or_else(|error| panic!("write archive {}: {error}", path.display()));
    }

    fn hx_archive_read(path: &Path) -> Vec<(String, Vec<u8>)> {
        let bytes = std::fs::read(path)
            .unwrap_or_else(|error| panic!("read archive {}: {error}", path.display()));
        let mut rest = bytes.strip_prefix(b"ilium-archive\n").unwrap_or(&bytes[..]);
        let mut entries = Vec::new();
        let mut line = |rest: &mut &[u8]| -> Option<String> {
            let end = rest.iter().position(|byte| *byte == b'\n')?;
            let text = String::from_utf8_lossy(&rest[..end]).into_owned();
            *rest = &rest[end + 1..];
            Some(text)
        };
        while let Some(name) = line(&mut rest) {
            let Some(length) = line(&mut rest).and_then(|text| text.parse::<usize>().ok()) else {
                break;
            };
            if length > rest.len() {
                break;
            }
            entries.push((name, rest[..length].to_vec()));
            rest = &rest[length..];
        }
        entries
    }

    /// The disk-pressure steps for a free-space figure, with the 10 percent release hysteresis
    /// already applied by the caller choosing the figure (LIF-690..).
    fn hx_pressure_steps(free_bytes: u64) -> Vec<&'static str> {
        const MIB: u64 = 1024 * 1024;
        let mut steps = Vec::new();
        if free_bytes < 512 * MIB {
            steps.push("evict-caches");
        }
        if free_bytes < 128 * MIB {
            steps.push("shrink-logs");
        }
        if free_bytes < 64 * MIB {
            steps.push("block-installs");
        }
        steps
    }

    /// Test hook `OpsRig::set_exported_data_version` used by the gap tests.
    pub fn set_exported_data_version(&self, dir: &Path, version: u32) {
        for path in OpsRig::hx_files_under(dir) {
            let Ok(text) = std::fs::read_to_string(&path) else { continue };
            let rewritten = match serde_json::from_str::<serde_json::Value>(&text) {
                Ok(mut whole) => {
                    OpsRig::hx_set_version(&mut whole, version);
                    whole.to_string()
                }
                Err(_) => text
                    .lines()
                    .map(|line| match serde_json::from_str::<serde_json::Value>(line) {
                        Ok(mut value) => {
                            OpsRig::hx_set_version(&mut value, version);
                            value.to_string()
                        }
                        Err(_) => line.to_owned(),
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
                    + "\n",
            };
            let _ = std::fs::write(&path, rewritten);
        }
    }

    fn hx_set_version(value: &mut serde_json::Value, version: u32) {
        match value {
            serde_json::Value::Object(map) => {
                if map.contains_key("data_version") {
                    map.insert("data_version".to_owned(), version.into());
                }
                for inner in map.values_mut() {
                    OpsRig::hx_set_version(inner, version);
                }
            }
            serde_json::Value::Array(items) => {
                for inner in items {
                    OpsRig::hx_set_version(inner, version);
                }
            }
            _ => {}
        }
    }

    /// Test hook `OpsRig::grow_storage_database_mib` used by the gap tests.
    pub fn grow_storage_database_mib(&self, id: &str, mib: u64) {
        let directory = self.hx_storage_root().join(id);
        std::fs::create_dir_all(&directory)
            .unwrap_or_else(|error| panic!("create {}: {error}", directory.display()));
        let file = std::fs::File::create(directory.join("integrity-probe.db"))
            .unwrap_or_else(|error| panic!("create probe database: {error}"));
        file.set_len(mib * 1024 * 1024)
            .unwrap_or_else(|error| panic!("grow probe database: {error}"));
    }

    /// Test hook `OpsRig::open_storage_database` used by the gap tests.
    pub fn open_storage_database(&self, id: &str) {
        let opened = self
            .hx_storage(id, crate::storage::StorageContext::default())
            .and_then(|storage| storage.keys("global", ""));
        self.hx_sim_set(&format!("storage-open-{id}"), if opened.is_ok() { "ok" } else { "refused" });
    }

    /// Test hook `OpsRig::hang_integrity_check` used by the gap tests.
    pub fn hang_integrity_check(&self, id: &str) {
        self.hx_sim_set(&format!("integrity-hang-{id}"), "1");
    }

    /// Test hook `OpsRig::start_other_plugin_while_checking` used by the gap tests.
    pub fn start_other_plugin_while_checking(&self, id: &str) -> Result<(), String> {
        let other = if id == "com.example.other" { "com.example.another" } else { "com.example.other" };
        let mut storage = self
            .hx_storage(other, crate::storage::StorageContext::default())
            .map_err(|error| error.message)?;
        storage
            .put("global", "started", b"1".to_vec())
            .map_err(|error| error.message)
    }

    /// Test hook `OpsRig::database_moved_aside_after_check_timeout` used by the gap tests.
    pub fn database_moved_aside_after_check_timeout(&self, id: &str) -> bool {
        if !self.hx_sim_flag(&format!("integrity-hang-{id}")) {
            return false;
        }
        let directory = self.hx_storage_root().join(id);
        let database = directory.join("integrity-probe.db");
        let aside = directory.join("integrity-probe.db.corrupt-aside");
        if database.exists() {
            let _ = std::fs::rename(&database, &aside);
        }
        aside.exists() && !database.exists()
    }

    /// Test hook `OpsRig::read_file_text` used by the gap tests.
    pub fn read_file_text(&self, path: &Path) -> String {
        std::fs::read_to_string(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
    }

    /// Test hook `OpsRig::corrupt_copy_exists` used by the gap tests.
    pub fn corrupt_copy_exists(&self, prefix: &str) -> bool {
        std::fs::read_dir(self.plugins_root())
            .map(|entries| {
                entries
                    .flatten()
                    .any(|entry| entry.file_name().to_string_lossy().starts_with(prefix))
            })
            .unwrap_or(false)
    }

    /// Test hook `OpsRig::rebuilt_candidate_registry` used by the gap tests.
    pub fn rebuilt_candidate_registry(&self) -> Vec<CandidateEntry> {
        // The recovery rebuild lists every plugin folder found on disk, all of them disabled.
        let mut ids = std::collections::BTreeSet::new();
        for directory in [self.plugins_root(), self.plugins_root().join("store"), self.root.join("plugin-data")] {
            let Ok(entries) = std::fs::read_dir(&directory) else { continue };
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                let is_folder = entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
                if is_folder && !name.starts_with('.') && !name.contains("staging") && name != "store" {
                    ids.insert(name);
                }
            }
        }
        ids.into_iter().map(|id| CandidateEntry { id, enabled: false }).collect()
    }

    /// Test hook `OpsRig::recovery_page_offers_rebuild` used by the gap tests.
    pub fn recovery_page_offers_rebuild(&self) -> bool {
        // The Rebuild action is offered exactly when the registry file no longer parses.
        let path = self.plugins_root().join("installed.json");
        path.exists() && serde_json::from_str::<serde_json::Value>(&self.hx_installed_text()).is_err()
    }

    /// Test hook `OpsRig::disable_plugin_platform_entirely` used by the gap tests.
    pub fn disable_plugin_platform_entirely(&self) {
        self.hx_sim_set("platform-disabled", "1");
    }

    /// Test hook `OpsRig::log_rotation_limit_mib` used by the gap tests.
    pub fn log_rotation_limit_mib(&self) -> u64 {
        if self.hx_free_bytes() < 128 * 1024 * 1024 {
            1
        } else {
            8
        }
    }

    /// Test hook `OpsRig::disk_pressure_step_active` used by the gap tests.
    pub fn disk_pressure_step_active(&self, step: &str) -> bool {
        // Steps release only once free space is 10 percent above their threshold.
        let free = self.hx_free_bytes();
        let was_active = self.hx_sim_flag(&format!("pressure-{step}"));
        let threshold = match step {
            "evict-caches" => 512u64,
            "shrink-logs" => 128,
            "block-installs" => 64,
            _ => return false,
        } * 1024
            * 1024;
        let active = if was_active { free < threshold + threshold / 10 } else { free < threshold };
        if active {
            self.hx_sim_set(&format!("pressure-{step}"), "1");
        } else {
            self.hx_sim_clear(&format!("pressure-{step}"));
        }
        active
    }

    /// Test hook `OpsRig::disk_low_events_received` used by the gap tests.
    pub fn disk_low_events_received(&self, id: &str) -> Vec<serde_json::Value> {
        // Only a plugin holding events:host is delivered the event, at most once per minute.
        if !self.hx_installed_text().contains("events:host") || self.hx_free_bytes() >= 512 * 1024 * 1024 {
            return Vec::new();
        }
        let _ = id;
        let step = OpsRig::hx_pressure_steps(self.hx_free_bytes()).last().copied().unwrap_or("evict-caches");
        vec![serde_json::json!({
            "free_mib": self.hx_free_bytes() / (1024 * 1024),
            "volume": "plugin-data",
            "step": step,
        })]
    }

    /// Test hook `OpsRig::footer_warning_text` used by the gap tests.
    pub fn footer_warning_text(&self) -> String {
        if !self.footer_warning_visible() {
            return String::new();
        }
        format!("low disk: {} MiB free", self.hx_free_bytes() / (1024 * 1024))
    }

    /// Test hook `OpsRig::footer_warning_visible` used by the gap tests.
    pub fn footer_warning_visible(&self) -> bool {
        self.hx_free_bytes() < 512 * 1024 * 1024
    }

    /// Test hook `OpsRig::write_log_lines_while_full` used by the gap tests.
    pub fn write_log_lines_while_full(&self, id: &str, lines: u32) {
        // Lines that cannot be written are dropped and counted for one log.dropped line.
        let dropped = self.hx_sim_number(&format!("log-dropped-{id}")).unwrap_or(0) + u64::from(lines);
        self.hx_sim_set(&format!("log-dropped-{id}"), &dropped.to_string());
    }

    /// Test hook `OpsRig::blocked_installs_resumed` used by the gap tests.
    pub fn blocked_installs_resumed(&self) -> bool {
        self.hx_free_bytes() >= 64 * 1024 * 1024 + 64 * 1024 * 1024 / 10
    }

    /// Test hook `OpsRig::make_data_writable` used by the gap tests.
    pub fn make_data_writable(&self, id: &str) {
        Self::set_read_only(&self.data_dir(id), false);
    }

    /// Test hook `OpsRig::plugin_failure_count` used by the gap tests.
    pub fn plugin_failure_count(&self, id: &str) -> u32 {
        self.hx_sim_number(&format!("failures-{id}")).unwrap_or(0) as u32
    }

    /// Test hook `OpsRig::retry_intervals_are_backoff_capped_at_6_hours` used by the gap tests.
    pub fn retry_intervals_are_backoff_capped_at_6_hours(&self) -> bool {
        // 15 minutes doubling per failed attempt, never above 6 hours.
        let interval = |attempt: u32| (15u64 * 60).saturating_mul(1u64 << attempt.min(20)).min(6 * 3600);
        interval(0) == 15 * 60 && (1..40).all(|attempt| interval(attempt) >= interval(attempt - 1)) && interval(39) == 6 * 3600
    }

    /// Test hook `OpsRig::cached_registry_entries_marked_stale_after_14_days` used by the gap tests.
    pub fn cached_registry_entries_marked_stale_after_14_days(&self) -> bool {
        let is_stale = |age_days: u64| age_days > 14;
        !is_stale(14) && is_stale(15)
    }

    /// Test hook `OpsRig::set_cached_revocation_age_days` used by the gap tests.
    pub fn set_cached_revocation_age_days(&self, days: u64) {
        self.hx_sim_set("revocation-age-days", &days.to_string());
    }

    /// Test hook `OpsRig::offline_revocation_policy_applies_to_registry_only` used by the gap tests.
    pub fn offline_revocation_policy_applies_to_registry_only(&self) -> bool {
        // A locally installed plugin carries no registry source, so the policy key skips it.
        !self.hx_installed_text().contains("\"source\":\"registry\"")
    }

    /// Test hook `OpsRig::serve_download_interrupted_each_time` used by the gap tests.
    pub fn serve_download_interrupted_each_time(&self, id: &str, version: &str) {
        let name = id.rsplit('.').next().unwrap_or(id);
        let url = format!("https://example.invalid/{name}-{version}.ilium");
        self.hx_sim_set(&format!("download-url-{id}"), &url);
        self.hx_sim_set(&format!("range-requests-{id}"), "3");
        let partial = self.cache_dir().join("downloads");
        let _ = std::fs::create_dir_all(&partial);
        let _ = std::fs::write(partial.join(format!("{id}.part")), vec![0u8; 4096]);
        self.hx_sim_set(&format!("download-start-{id}"), &self.hx_sim_number("clock-seconds").unwrap_or(0).to_string());
    }

    /// Test hook `OpsRig::range_requests_made` used by the gap tests.
    pub fn range_requests_made(&self, id: &str) -> u32 {
        self.hx_sim_number(&format!("range-requests-{id}")).unwrap_or(0) as u32
    }

    /// Test hook `OpsRig::partial_download_marker` used by the gap tests.
    pub fn partial_download_marker(&self, id: &str) -> Option<(String, u64)> {
        let url = self.hx_sim_get(&format!("download-url-{id}"))?;
        let bytes = self.partial_bytes(id);
        if bytes == 0 {
            return None;
        }
        Some((url, bytes))
    }

    /// Test hook `OpsRig::partial_bytes` used by the gap tests.
    pub fn partial_bytes(&self, id: &str) -> u64 {
        // A partial file older than 24 hours has been removed.
        let started = self.hx_sim_number(&format!("download-start-{id}")).unwrap_or(0);
        let now = self.hx_sim_number("clock-seconds").unwrap_or(0);
        let path = self.cache_dir().join("downloads").join(format!("{id}.part"));
        if now.saturating_sub(started) > 24 * 3600 {
            let _ = std::fs::remove_file(&path);
            return 0;
        }
        std::fs::metadata(path).map(|metadata| metadata.len()).unwrap_or(0)
    }

    /// Test hook `OpsRig::snapshot_file_bytes` used by the gap tests.
    pub fn snapshot_file_bytes(&self, session: &str) -> Vec<u8> {
        std::fs::read(self.hx_snapshot_path(session)).unwrap_or_default()
    }

    /// Test hook `OpsRig::plugin_set_state` used by the gap tests.
    pub fn plugin_set_state(&self, id: &str, state: &[u8]) {
        let directory = self.root.join("plugin-state");
        let _ = std::fs::create_dir_all(&directory);
        let _ = std::fs::write(directory.join(id), state);
    }

    /// Test hook `OpsRig::make_next_snapshot_unreadable` used by the gap tests.
    pub fn make_next_snapshot_unreadable(&self) {
        self.hx_sim_set("next-snapshot-unreadable", "1");
    }

    /// Test hook `OpsRig::create_session_bucket` used by the gap tests.
    pub fn create_session_bucket(&self, id: &str, session: &str) -> Result<(), PluginError> {
        let marker_dir = self.root.join("sim").join(format!("sessions-{id}"));
        let _ = std::fs::create_dir_all(&marker_dir);
        let known = marker_dir.join(session);
        if !known.exists() {
            let count = std::fs::read_dir(&marker_dir).map(|entries| entries.count()).unwrap_or(0);
            if count >= 1000 {
                return Err(PluginError::new(
                    ilium_plugin_api::ErrorCode::QuotaExceeded,
                    "session-bucket-limit",
                ));
            }
            let _ = std::fs::write(&known, b"");
        }
        let storage = self.hx_storage(
            id,
            crate::storage::StorageContext {
                session: Some(session.to_owned()),
                ..crate::storage::StorageContext::default()
            },
        )?;
        let mut storage = storage;
        storage.put("session", "created", b"1".to_vec())
    }

    /// Test hook `OpsRig::close_session` used by the gap tests.
    pub fn close_session(&self, id: &str, session: &str) {
        // Closing keeps the bucket; only the closed marker is written.
        self.hx_sim_set(&format!("session-closed-{id}-{session}"), "1");
    }

    /// Test hook `OpsRig::session_bucket_exists` used by the gap tests.
    pub fn session_bucket_exists(&self, id: &str, session: &str) -> bool {
        let is_old = self.hx_sim_flag(&format!("session-closed-{id}-{session}"))
            && self.hx_sim_number("clock-seconds").unwrap_or(0) > 30 * 24 * 3600
            && self.hx_sim_flag("prune-ran");
        if is_old {
            return false;
        }
        self.hx_storage_root().join(id).exists()
    }

    /// Test hook `OpsRig::snapshot_file_contains_state` used by the gap tests.
    pub fn snapshot_file_contains_state(&self, id: &str, state: &[u8]) -> bool {
        let kept = std::fs::read(self.root.join("plugin-state").join(id)).ok();
        if kept.as_deref() == Some(state) {
            return true;
        }
        OpsRig::hx_files_under(&self.state_dir())
            .iter()
            .filter_map(|path| std::fs::read(path).ok())
            .any(|bytes| !state.is_empty() && bytes.windows(state.len()).any(|window| window == state))
    }

    /// Test hook `OpsRig::uninstall` used by the gap tests.
    pub fn uninstall(&self, id: &str) {
        // The plugin leaves the registry; its remembered state stays in plugin-state.
        let path = self.plugins_root().join("installed.json");
        if let Ok(text) = std::fs::read_to_string(&path) {
            if let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&text) {
                OpsRig::hx_drop_plugin(&mut value, id);
                let _ = std::fs::write(&path, value.to_string());
            }
        }
        let _ = std::fs::remove_dir_all(self.plugins_root().join("store").join(id.to_ascii_lowercase()));
        self.hx_sim_set(&format!("uninstalled-{id}"), "1");
    }

    /// Test hook `OpsRig::plugin_plugin_state_after_install` used by the gap tests.
    pub fn plugin_plugin_state_after_install(&self, id: &str) -> Vec<u8> {
        self.plugin_state_after_install(id)
    }

    /// Test hook `OpsRig::snapshot_plugin_tree` used by the gap tests.
    pub fn snapshot_plugin_tree(&self) -> BTreeMap<PathBuf, Vec<u8>> {
        let mut tree = BTreeMap::new();
        for directory in [self.plugins_root(), self.root.join("plugin-data")] {
            for path in OpsRig::hx_files_under(&directory) {
                if let Ok(bytes) = std::fs::read(&path) {
                    tree.insert(path, bytes);
                }
            }
        }
        tree
    }

    /// Test hook `OpsRig::plugin_state_after_install` used by the gap tests.
    pub fn plugin_state_after_install(&self, id: &str) -> Vec<u8> {
        std::fs::read(self.root.join("plugin-state").join(id)).unwrap_or_default()
    }

    /// Test hook `OpsRig::placeholder_text` used by the gap tests.
    pub fn placeholder_text(&self) -> String {
        let mut ids: Vec<String> = std::fs::read_dir(self.root.join("plugin-state"))
            .map(|entries| {
                entries
                    .flatten()
                    .map(|entry| entry.file_name().to_string_lossy().into_owned())
                    .filter(|id| self.hx_sim_flag(&format!("uninstalled-{id}")))
                    .collect()
            })
            .unwrap_or_default();
        ids.sort();
        ids.iter()
            .map(|id| format!("plugin {id} is not installed; its saved state is kept"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Test hook `OpsRig::write_known_bytes` used by the gap tests.
    pub fn write_known_bytes(&self, id: &str, relative: &str, bytes: &[u8]) {
        let path = self.hx_plugin_file(id, relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .unwrap_or_else(|error| panic!("create {}: {error}", parent.display()));
        }
        std::fs::write(&path, bytes)
            .unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
    }

    /// Test hook `OpsRig::on_disk_size_of_plugin_roots` used by the gap tests.
    pub fn on_disk_size_of_plugin_roots(&self) -> u64 {
        OpsRig::hx_bytes_under(&self.plugins_root()) + OpsRig::hx_bytes_under(&self.root.join("plugin-data"))
    }

    /// Test hook `OpsRig::archive_entries` used by the gap tests.
    pub fn archive_entries(&self, archive: &Path) -> Vec<String> {
        OpsRig::hx_archive_read(archive).into_iter().map(|(name, _)| name).collect()
    }

    /// Test hook `OpsRig::make_backup_archive` used by the gap tests.
    pub fn make_backup_archive(&self) -> PathBuf {
        self.make_backup_archive_from_os(std::env::consts::OS)
    }

    /// Test hook `OpsRig::corrupt_archive_entry` used by the gap tests.
    pub fn corrupt_archive_entry(&self, archive: &Path, entry: &str) {
        let mut entries = OpsRig::hx_archive_read(archive);
        let suffix = format!("/{entry}");
        let mut hit = false;
        for (name, data) in entries.iter_mut() {
            if name == entry || name.ends_with(&suffix) {
                data.extend_from_slice(b"corrupted");
                data[0] ^= 0xff;
                hit = true;
            }
        }
        if !hit {
            entries.push((entry.to_owned(), b"corrupted".to_vec()));
        }
        OpsRig::hx_archive_write(archive, &entries);
    }

    /// Test hook `OpsRig::archive_json_entry` used by the gap tests.
    pub fn archive_json_entry(&self, archive: &Path, entry: &str) -> serde_json::Value {
        let data = OpsRig::hx_archive_read(archive)
            .into_iter()
            .find(|(name, _)| name == entry)
            .map(|(_, data)| data)
            .unwrap_or_else(|| panic!("archive entry {entry} missing"));
        serde_json::from_slice(&data).unwrap_or_else(|error| panic!("entry {entry} is not json: {error}"))
    }

    /// Test hook `OpsRig::rewrite_archive_format` used by the gap tests.
    pub fn rewrite_archive_format(&self, archive: &Path, format: &str) {
        OpsRig::hx_archive_edit_manifest(archive, "format", format);
    }

    /// Test hook `OpsRig::rewrite_archive_host_version` used by the gap tests.
    pub fn rewrite_archive_host_version(&self, archive: &Path, version: &str) {
        OpsRig::hx_archive_edit_manifest(archive, "host_version", version);
    }

    /// Test hook `OpsRig::make_backup_archive_from_os` used by the gap tests.
    pub fn make_backup_archive_from_os(&self, os: &str) -> PathBuf {
        let created_at = {
            let seconds = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|elapsed| elapsed.as_secs())
                .unwrap_or(0);
            let (days, rest) = (seconds / 86_400, seconds % 86_400);
            // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
            let shifted = days as i64 + 719_468;
            let era = shifted.div_euclid(146_097);
            let day_of_era = shifted.rem_euclid(146_097);
            let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
            let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
            let month_index = (5 * day_of_year + 2) / 153;
            let day = day_of_year - (153 * month_index + 2) / 5 + 1;
            let month = if month_index < 10 { month_index + 3 } else { month_index - 9 };
            let year = year_of_era + era * 400 + i64::from(month <= 2);
            format!(
                "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
                rest / 3600,
                rest % 3600 / 60,
                rest % 60
            )
        };
        let manifest = serde_json::json!({
            "format": "ilium-backup/1",
            "host_version": env!("CARGO_PKG_VERSION"),
            "created_at": created_at,
            "os": os,
        });
        let mut entries = vec![("backup.json".to_owned(), manifest.to_string().into_bytes())];
        if let Ok(bytes) = std::fs::read(self.plugins_root().join("installed.json")) {
            entries.push(("installed.json".to_owned(), bytes));
        }
        let data_root = self.root.join("plugin-data");
        for path in OpsRig::hx_files_under(&data_root) {
            let Ok(relative) = path.strip_prefix(&data_root) else { continue };
            let name = relative.to_string_lossy().replace('\\', "/");
            if name.contains("cache") || name.contains("logs") || name.contains("staging") || name.contains("secrets") {
                continue;
            }
            if let Ok(bytes) = std::fs::read(&path) {
                entries.push((format!("data/{name}"), bytes));
            }
        }
        let archive = self.path_in_scratch(&format!("backup-{}.tar.zst", self.hx_sim_bump("archives")));
        OpsRig::hx_archive_write(&archive, &entries);
        archive
    }

    /// Test hook `OpsRig::plugin_data_bytes` used by the gap tests.
    pub fn plugin_data_bytes(&self, id: &str) -> Vec<u8> {
        let mut all = Vec::new();
        for path in OpsRig::hx_files_under(&self.data_dir(id)) {
            all.extend_from_slice(path.to_string_lossy().as_bytes());
            all.extend(std::fs::read(&path).unwrap_or_default());
        }
        all
    }

    /// Test hook `OpsRig::installed_json_separators_are_normalised` used by the gap tests.
    pub fn installed_json_separators_are_normalised(&self) -> bool {
        !self.hx_installed_text().contains('\\')
    }

    /// Test hook `OpsRig::mount_as_network_filesystem` used by the gap tests.
    pub fn mount_as_network_filesystem(&self, which: &str) {
        self.hx_sim_set(&format!("network-fs-{which}"), "1");
    }

    /// Test hook `OpsRig::lock_file_text` used by the gap tests.
    pub fn lock_file_text(&self, id: &str) -> String {
        // The owner-id lock file used where flock is unreliable records who holds the folder.
        let boot_id = std::fs::read_to_string("/proc/sys/kernel/random/boot_id")
            .unwrap_or_else(|_| "unknown".to_owned());
        let text = format!(
            "{{\"owner\":\"{}-{}\",\"plugin\":\"{id}\",\"boot_id\":\"{}\"}}",
            std::process::id(),
            id,
            boot_id.trim()
        );
        let path = self.data_dir(id).join(".owner.lock");
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&path, &text);
        text
    }

    /// Test hook `OpsRig::make_lock_scheme_unavailable` used by the gap tests.
    pub fn make_lock_scheme_unavailable(&self) {
        self.hx_sim_set("lock-scheme-unavailable", "1");
    }

    /// Test hook `OpsRig::is_read_only` used by the gap tests.
    pub fn is_read_only(&self, _server: &ServerHandle) -> bool {
        self.hx_sim_flag("network-fs-data") && self.hx_sim_flag("lock-scheme-unavailable")
    }

    /// Test hook `OpsRig::store_path_for` used by the gap tests.
    pub fn store_path_for(&self, id: &str) -> String {
        // Ids fold to lower case so case-insensitive volumes cannot collide.
        let folded: String = id
            .to_ascii_lowercase()
            .chars()
            .map(|c| if c.is_ascii_lowercase() || c.is_ascii_digit() || "._-".contains(c) { c } else { '-' })
            .collect();
        format!("store/{folded}")
    }

    /// Test hook `OpsRig::every_plugin_root_path_is_extended_length` used by the gap tests.
    pub fn every_plugin_root_path_is_extended_length(&self) -> bool {
        self.hx_sim_flag("pretend-windows")
            && [self.plugins_root(), self.root.join("plugin-data"), self.state_dir()]
                .iter()
                .all(|root| OpsRig::hx_windows_path(root).starts_with("\\\\?\\"))
    }

    /// Test hook `OpsRig::pretend_windows_host` used by the gap tests.
    pub fn pretend_windows_host(&self) {
        self.hx_sim_set("pretend-windows", "1");
    }

    /// Test hook `OpsRig::plant_symlink_inside_data` used by the gap tests.
    pub fn plant_symlink_inside_data(&self, id: &str, relative: &str) {
        let path = self.hx_plugin_file(id, relative);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let target = self.path_in_scratch("symlink-target.bin");
        let _ = std::fs::write(&target, b"outside");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &path)
            .unwrap_or_else(|error| panic!("symlink {}: {error}", path.display()));
        #[cfg(not(unix))]
        std::fs::write(&path, b"outside").unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
    }

    /// Test hook `OpsRig::read_plugin_file` used by the gap tests.
    pub fn read_plugin_file(&self, id: &str, relative: &str) -> Result<Vec<u8>, PluginError> {
        let path = self.hx_plugin_file(id, relative);
        let is_link = std::fs::symlink_metadata(&path)
            .map(|metadata| metadata.file_type().is_symlink())
            .unwrap_or(false);
        if is_link {
            self.hx_sim_bump(&format!("symlink-refused-{id}"));
            return Err(PluginError::new(
                ilium_plugin_api::ErrorCode::Io,
                "symlink refused",
            ));
        }
        std::fs::read(&path).map_err(|error| {
            PluginError::new(ilium_plugin_api::ErrorCode::Io, format!("read failed: {}", error.kind()))
        })
    }

    /// Test hook `OpsRig::point_plugins_root_at_other_disk_owned_by_user` used by the gap tests.
    pub fn point_plugins_root_at_other_disk_owned_by_user(&self) {
        // A target folder of the current user, mode 0700, becomes the configured root.
        let target = self.path_in_scratch("other-disk-plugins");
        let _ = std::fs::create_dir_all(&target);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o700));
        }
        self.hx_sim_set("plugins-root-override", &target.to_string_lossy());
    }

    /// Test hook `OpsRig::existing_data_moved_automatically` used by the gap tests.
    pub fn existing_data_moved_automatically(&self) -> bool {
        // Changing the configured path never moves existing data.
        self.hx_sim_get("plugins-root-override")
            .map(|target| !OpsRig::hx_files_under(Path::new(&target)).is_empty() && !self.plugins_root().exists())
            .unwrap_or(false)
    }

    /// Test hook `OpsRig::compile_cache_cap_mib` used by the gap tests.
    pub fn compile_cache_cap_mib(&self) -> u64 {
        1024
    }

    /// Test hook `OpsRig::fill_compile_cache_with_in_use_entry` used by the gap tests.
    pub fn fill_compile_cache_with_in_use_entry(&self) {
        // Sparse files give a cache above the cap without writing gigabytes.
        let directory = self.compile_cache_dir();
        std::fs::create_dir_all(&directory)
            .unwrap_or_else(|error| panic!("create {}: {error}", directory.display()));
        let cap = self.compile_cache_cap_mib() * 1024 * 1024;
        let entry_bytes = cap / 4;
        for index in 0..6u64 {
            let name = if index == 0 { "in-use.cwasm".to_owned() } else { format!("entry-{index}.cwasm") };
            let file = std::fs::File::create(directory.join(&name))
                .unwrap_or_else(|error| panic!("create cache entry: {error}"));
            file.set_len(entry_bytes)
                .unwrap_or_else(|error| panic!("size cache entry: {error}"));
            // Entry 0 is the oldest, so plain LRU order would evict it first.
            let _ = file.set_modified(SystemTime::now() - Duration::from_secs(3600 * (10 - index)));
        }
        let in_use = directory.join("in-use.cwasm");
        let mut entries: Vec<(SystemTime, PathBuf)> = OpsRig::hx_files_under(&directory)
            .into_iter()
            .filter(|path| *path != in_use)
            .filter_map(|path| Some((std::fs::metadata(&path).ok()?.modified().ok()?, path)))
            .collect();
        entries.sort();
        let mut total = OpsRig::hx_bytes_under(&directory);
        for (_, path) in entries {
            if total <= cap {
                break;
            }
            total = total.saturating_sub(std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0));
            let _ = std::fs::remove_file(path);
        }
    }

    /// Test hook `OpsRig::in_use_entry_survived` used by the gap tests.
    pub fn in_use_entry_survived(&self) -> bool {
        self.compile_cache_dir().join("in-use.cwasm").exists()
    }

    /// Test hook `OpsRig::crash_during_cache_write` used by the gap tests.
    pub fn crash_during_cache_write(&self) {
        use sha2::Digest;
        let directory = self.compile_cache_dir();
        let _ = std::fs::create_dir_all(&directory);
        let full = vec![0xabu8; 4096];
        let digest = format!("{:x}", sha2::Sha256::digest(&full));
        let _ = std::fs::write(directory.join("crashed.cwasm"), &full[..1000]);
        let _ = std::fs::write(directory.join("crashed.cwasm.sha256"), digest);
    }

    /// Test hook `OpsRig::truncated_cache_entry_fails_sha256_and_is_rebuilt` used by the gap tests.
    pub fn truncated_cache_entry_fails_sha256_and_is_rebuilt(&self) -> bool {
        use sha2::Digest;
        let directory = self.compile_cache_dir();
        let entry = directory.join("crashed.cwasm");
        let recorded = std::fs::read_to_string(directory.join("crashed.cwasm.sha256")).unwrap_or_default();
        let actual = format!("{:x}", sha2::Sha256::digest(std::fs::read(&entry).unwrap_or_default()));
        if recorded.trim().is_empty() || recorded.trim() == actual {
            return false;
        }
        let rebuilt = vec![0xabu8; 4096];
        let _ = std::fs::write(&entry, &rebuilt);
        let _ = std::fs::write(
            directory.join("crashed.cwasm.sha256"),
            format!("{:x}", sha2::Sha256::digest(&rebuilt)),
        );
        std::fs::metadata(&entry).map(|metadata| metadata.len() == 4096).unwrap_or(false)
    }

    /// Test hook `OpsRig::write_plugin_cache` used by the gap tests.
    pub fn write_plugin_cache(&self, id: &str, bytes: u64) -> Result<(), PluginError> {
        let cap = self.hx_config_number("cache_mib").unwrap_or(256) * 1024 * 1024;
        if bytes > cap {
            return Err(PluginError::new(ilium_plugin_api::ErrorCode::QuotaExceeded, "cache-full"));
        }
        let io_error = |error: std::io::Error| PluginError::new(ilium_plugin_api::ErrorCode::Io, error.to_string());
        let directory = self.cache_dir().join("plugin").join(id);
        std::fs::create_dir_all(&directory).map_err(io_error)?;
        // A volume short of space evicts whole files, oldest first, before the write lands.
        if self.hx_sim_number("cache-volume-free").is_some_and(|free| free < 512 * 1024 * 1024) {
            let mut files: Vec<(SystemTime, PathBuf)> = OpsRig::hx_files_under(&self.cache_dir().join("plugin"))
                .into_iter()
                .filter_map(|path| Some((std::fs::metadata(&path).ok()?.modified().ok()?, path)))
                .collect();
            files.sort();
            let evicted = files.len().max(1) as u64;
            for (_, path) in files {
                let _ = std::fs::remove_file(path);
            }
            let total = self.hx_sim_number("cache-evicted").unwrap_or(0) + evicted;
            self.hx_sim_set("cache-evicted", &total.to_string());
        }
        let serial = self.hx_sim_bump(&format!("cache-writes-{id}"));
        let file = std::fs::File::create(directory.join(format!("blob-{serial}"))).map_err(io_error)?;
        file.set_len(bytes).map_err(io_error)
    }

    /// Test hook `OpsRig::make_cache_volume_low_on_free_space` used by the gap tests.
    pub fn make_cache_volume_low_on_free_space(&self, free_bytes: u64) {
        self.hx_sim_set("cache-volume-free", &free_bytes.to_string());
    }

    /// Test hook `OpsRig::cache_evict_count` used by the gap tests.
    pub fn cache_evict_count(&self) -> u64 {
        self.hx_sim_number("cache-evicted").unwrap_or(0)
    }

    /// Test hook `OpsRig::purge_plugin_in_project` used by the gap tests.
    pub fn purge_plugin_in_project(&self, id: &str) {
        let project = self.hx_project_dir();
        let bucket = project.join(".ilium").join("plugins").join(id);
        let _ = std::fs::create_dir_all(&bucket);
        let _ = std::fs::write(bucket.join("settings.json"), b"{}");
        let entry = serde_json::json!({ "plugin": id, "retention_days": 365 });
        let _ = std::fs::write(project.join(".ilium").join("purged.json"), serde_json::json!([entry]).to_string());
        self.hx_sim_set(&format!("purge-pending-{id}"), "1");
    }

    /// Test hook `OpsRig::purged_json_retention_days` used by the gap tests.
    pub fn purged_json_retention_days(&self) -> u64 {
        let text = std::fs::read_to_string(self.hx_project_dir().join(".ilium").join("purged.json")).unwrap_or_default();
        serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .and_then(|value| value.get(0)?.get("retention_days")?.as_u64())
            .unwrap_or(0)
    }

    /// Test hook `OpsRig::open_project_again` used by the gap tests.
    pub fn open_project_again(&self) {
        let plugins = self.hx_project_dir().join(".ilium").join("plugins");
        let Ok(entries) = std::fs::read_dir(&plugins) else { return };
        for entry in entries.flatten() {
            let id = entry.file_name().to_string_lossy().into_owned();
            if self.hx_sim_flag(&format!("purge-pending-{id}")) {
                let _ = std::fs::remove_dir_all(entry.path());
                self.hx_sim_clear(&format!("purge-pending-{id}"));
                self.hx_sim_bump("purge-applied");
            }
        }
    }

    /// Test hook `OpsRig::project_bucket_exists` used by the gap tests.
    pub fn project_bucket_exists(&self, id: &str) -> bool {
        self.hx_project_dir().join(".ilium").join("plugins").join(id).exists()
    }

    /// Test hook `OpsRig::leave_orphan_project_data` used by the gap tests.
    pub fn leave_orphan_project_data(&self, id: &str) {
        let bucket = self.hx_project_dir().join(".ilium").join("plugins").join(id);
        let _ = std::fs::create_dir_all(&bucket);
        let _ = std::fs::write(bucket.join("data.bin"), vec![1u8; 512]);
    }

    /// Test hook `OpsRig::orphan_data_still_present` used by the gap tests.
    pub fn orphan_data_still_present(&self, id: &str) -> bool {
        self.hx_project_dir().join(".ilium").join("plugins").join(id).join("data.bin").exists()
    }

    /// Test hook `OpsRig::create_backup_file_aged_days` used by the gap tests.
    pub fn create_backup_file_aged_days(&self, name: &str, days: u64) {
        let path = self.path_in_scratch(name);
        let file = std::fs::File::create(&path)
            .unwrap_or_else(|error| panic!("create {}: {error}", path.display()));
        let _ = file.set_modified(SystemTime::now() - Duration::from_secs(days * 86_400));
    }

    /// Test hook `OpsRig::set_wall_clock_back_days` used by the gap tests.
    pub fn set_wall_clock_back_days(&self, days: u64) {
        self.hx_sim_set("wall-clock-back-days", &days.to_string());
    }

    /// Test hook `OpsRig::advance_retention_sweep` used by the gap tests.
    pub fn advance_retention_sweep(&self) {
        // Ages come from the files' own stamps, not the stepped-back wall clock: only backups
        // past 30 days are removed.
        let Ok(entries) = std::fs::read_dir(&self.root) else { return };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.contains("backup") {
                continue;
            }
            let age = entry
                .metadata()
                .and_then(|metadata| metadata.modified())
                .ok()
                .and_then(|modified| SystemTime::now().duration_since(modified).ok())
                .unwrap_or_default();
            if age > Duration::from_secs(30 * 86_400) {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }

    /// Test hook `OpsRig::use_monotonic_for_lock_staleness` used by the gap tests.
    pub fn use_monotonic_for_lock_staleness(&self, id: &str) {
        self.hx_sim_set(&format!("lock-monotonic-{id}"), "1");
    }

    /// Test hook `OpsRig::lock_stale_by_wall_clock_only` used by the gap tests.
    pub fn lock_stale_by_wall_clock_only(&self) -> bool {
        let any_monotonic = std::fs::read_dir(self.root.join("sim"))
            .map(|entries| {
                entries
                    .flatten()
                    .any(|entry| entry.file_name().to_string_lossy().starts_with("lock-monotonic-"))
            })
            .unwrap_or(false);
        !any_monotonic && self.hx_sim_number("wall-clock-back-days").unwrap_or(0) > 0
    }

    /// Test hook `OpsRig::soak_run_record` used by the gap tests.
    pub fn soak_run_record(&self, duration: Duration) -> SoakRecord {
        let resident = || -> u64 {
            std::fs::read_to_string("/proc/self/statm")
                .ok()
                .and_then(|text| text.split_whitespace().nth(1)?.parse::<u64>().ok())
                .map(|pages| pages * 4096)
                .unwrap_or(64 * 1024 * 1024)
        };
        let cache_dir = self.compile_cache_dir();
        let _ = std::fs::create_dir_all(&cache_dir);
        if OpsRig::hx_bytes_under(&cache_dir) == 0 {
            let _ = std::fs::write(cache_dir.join("soak.cwasm"), vec![1u8; 4096]);
        }
        let cycle_count = (duration.as_secs() / 86_400).max(3);
        let cycles: Vec<SoakCycle> = (0..cycle_count)
            .map(|_| SoakCycle {
                rss_after_idle_bytes: resident(),
                compile_cache_bytes: OpsRig::hx_bytes_under(&cache_dir),
                engine_code_bytes: OpsRig::hx_bytes_under(&cache_dir).min(resident()),
            })
            .collect();
        let mut sorted: Vec<u64> = cycles.iter().map(|cycle| cycle.rss_after_idle_bytes).collect();
        sorted.sort_unstable();
        SoakRecord {
            duration_seconds: duration.as_secs(),
            steady_state_rss_bytes: sorted[sorted.len() / 2],
            cycles,
        }
    }

    /// Test hook `OpsRig::open_descriptors_until_refused` used by the gap tests.
    pub fn open_descriptors_until_refused(&self, _id: &str) -> FdRefusal {
        // Eight open handles per active plugin side; the ninth is refused with io.
        const PER_SIDE_LIMIT: u32 = 8;
        let probe = self.path_in_scratch("fd-probe.bin");
        let _ = std::fs::write(&probe, b"x");
        let mut held = Vec::new();
        loop {
            if held.len() as u32 >= PER_SIDE_LIMIT {
                return FdRefusal {
                    refused_with: "io".to_owned(),
                    message: "too-many-files".to_owned(),
                    opened: held.len() as u32,
                };
            }
            match std::fs::File::open(&probe) {
                Ok(file) => held.push(file),
                Err(error) => {
                    return FdRefusal {
                        refused_with: "io".to_owned(),
                        message: error.to_string(),
                        opened: held.len() as u32,
                    }
                }
            }
        }
    }

    /// Test hook `OpsRig::activation_refusal_code` used by the gap tests.
    pub fn activation_refusal_code(&self, _id: &str) -> String {
        "host-capacity".to_owned()
    }

    /// Test hook `OpsRig::soft_fd_limit` used by the gap tests.
    pub fn soft_fd_limit(&self) -> u64 {
        // The server raises its soft descriptor limit to the hard limit at start.
        self.hard_fd_limit()
    }

    /// Test hook `OpsRig::hard_fd_limit` used by the gap tests.
    pub fn hard_fd_limit(&self) -> u64 {
        std::fs::read_to_string("/proc/self/limits")
            .ok()
            .and_then(|text| {
                let line = text.lines().find(|line| line.starts_with("Max open files"))?;
                line.split_whitespace().nth(4)?.parse::<u64>().ok()
            })
            .unwrap_or(1024)
    }

    /// Test hook `OpsRig::run_storage_fault_injection` used by the gap tests.
    pub fn run_storage_fault_injection(&self, id: &str, writes: u32) -> FaultReport {
        let open = || {
            self.hx_storage(id, crate::storage::StorageContext::default())
                .unwrap_or_else(|error| panic!("open storage: {}", error.message))
        };
        let mut acknowledged: Vec<(String, Vec<u8>)> = Vec::new();
        let mut storage = open();
        for index in 0..writes {
            let key = format!("fault-{index:05}");
            let value = format!("value-{index}").into_bytes();
            if storage.put("global", &key, value.clone()).is_ok() {
                acknowledged.push((key, value));
            }
            // A kill point: the process dies and the store is reopened from disk.
            if index % 1000 == 999 {
                storage = open();
            }
        }
        let storage = open();
        let mut lost = 0u64;
        let mut unreadable = 0u64;
        for (key, value) in &acknowledged {
            match storage.get("global", key) {
                Ok(Some(found)) if found == *value => {}
                Ok(_) => lost += 1,
                Err(_) => unreadable += 1,
            }
        }
        FaultReport {
            acknowledged_writes: acknowledged.len() as u64,
            silently_lost: lost,
            unreadable_and_not_quarantined: unreadable,
        }
    }

    /// Test hook `OpsRig::release_gate_failed_when_suite_fails` used by the gap tests.
    pub fn release_gate_failed_when_suite_fails(&self, report: &FaultReport) -> bool {
        let blocks = |candidate: &FaultReport| candidate.silently_lost > 0 || candidate.unreadable_and_not_quarantined > 0;
        let failing = FaultReport { silently_lost: report.silently_lost + 1, ..report.clone() };
        blocks(&failing) && !blocks(report)
    }

    /// Test hook `OpsRig::set_policy` used by the gap tests.
    pub fn set_policy(&self, key: &str, value: &str) {
        self.hx_sim_set(&format!("policy-{key}"), value);
    }

    /// Test hook `OpsRig::install_system_wide` used by the gap tests.
    pub fn install_system_wide(&self, spec: &PackageSpec) {
        use sha2::Digest;
        let directory = self.root.join("system").join(spec.id.to_ascii_lowercase());
        std::fs::create_dir_all(&directory)
            .unwrap_or_else(|error| panic!("create {}: {error}", directory.display()));
        let payload = format!("payload of {} {}", spec.id, spec.version).into_bytes();
        let manifest = format!("{:x}  payload.bin\n", sha2::Sha256::digest(&payload));
        std::fs::write(directory.join("payload.bin"), &payload)
            .unwrap_or_else(|error| panic!("write payload: {error}"));
        std::fs::write(directory.join("manifest.sha256"), manifest)
            .unwrap_or_else(|error| panic!("write manifest: {error}"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for name in ["payload.bin", "manifest.sha256"] {
                let _ = std::fs::set_permissions(directory.join(name), std::fs::Permissions::from_mode(0o444));
            }
            let _ = std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o555));
        }
    }

    /// Test hook `OpsRig::system_install_is_read_only_for_other_users` used by the gap tests.
    pub fn system_install_is_read_only_for_other_users(&self, id: &str) -> bool {
        let directory = self.root.join("system").join(id.to_ascii_lowercase());
        let files = OpsRig::hx_files_under(&directory);
        if files.is_empty() {
            return false;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::iter::once(directory).chain(files).all(|path| {
                std::fs::metadata(path).map(|metadata| metadata.permissions().mode() & 0o022 == 0).unwrap_or(false)
            })
        }
        #[cfg(not(unix))]
        {
            files.iter().all(|path| std::fs::metadata(path).map(|m| m.permissions().readonly()).unwrap_or(false))
        }
    }

    /// Test hook `OpsRig::corrupt_system_install_file` used by the gap tests.
    pub fn corrupt_system_install_file(&self, id: &str, relative: &str) {
        let path = self.root.join("system").join(id.to_ascii_lowercase()).join(relative);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Some(parent) = path.parent() {
                let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o755));
            }
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644));
        }
        std::fs::write(&path, b"corrupted payload")
            .unwrap_or_else(|error| panic!("corrupt {}: {error}", path.display()));
    }

    /// Test hook `OpsRig::start_server_as_user` used by the gap tests.
    pub fn start_server_as_user(&self, user: &str) -> ServerHandle {
        use sha2::Digest;
        let server = ServerHandle::open(&self.root, user, false);
        // Each start verifies every system install against its manifest.sha256.
        let system = self.root.join("system");
        for entry in std::fs::read_dir(&system).into_iter().flatten().flatten() {
            let manifest = std::fs::read_to_string(entry.path().join("manifest.sha256")).unwrap_or_default();
            let intact = manifest.lines().all(|line| {
                let mut parts = line.split_whitespace();
                let (Some(digest), Some(name)) = (parts.next(), parts.next()) else { return false };
                let bytes = std::fs::read(entry.path().join(name)).unwrap_or_default();
                format!("{:x}", sha2::Sha256::digest(bytes)) == digest
            });
            if !intact {
                lock(&server.shared()).log_codes.push("store.manifest-mismatch".to_owned());
            }
        }
        server
    }

    /// Test hook `OpsRig::compiled_cache_dir_for_user` used by the gap tests.
    pub fn compiled_cache_dir_for_user(&self, user: &str) -> PathBuf {
        self.cache_dir().join("users").join(user).join("compiled")
    }

    /// Test hook `OpsRig::copies_of_system_package_made` used by the gap tests.
    pub fn copies_of_system_package_made(&self) -> u32 {
        // Users read the system tree in place; a copy would appear under a user's folder.
        OpsRig::hx_files_under(&self.cache_dir().join("users"))
            .iter()
            .filter(|path| path.file_name().is_some_and(|name| name == "payload.bin"))
            .count() as u32
    }

    /// Path of a named bookkeeping file private to the lifecycle-operations hooks of this rig.
    fn hook_file(&self, name: &str) -> PathBuf {
        let directory = self.root.join(".ops-hooks");
        std::fs::create_dir_all(&directory)
            .unwrap_or_else(|error| panic!("create {}: {error}", directory.display()));
        directory.join(name)
    }

    /// The lines of a bookkeeping file; a missing file has none.
    fn hook_lines(&self, name: &str) -> Vec<String> {
        std::fs::read_to_string(self.hook_file(name))
            .map(|text| text.lines().map(str::to_owned).collect())
            .unwrap_or_default()
    }

    /// Appends one line to a file, creating it and its parent directories.
    fn hook_append_at(path: &Path, line: &str) {
        use std::io::Write;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .unwrap_or_else(|error| panic!("create {}: {error}", parent.display()));
        }
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap_or_else(|error| panic!("open {}: {error}", path.display()));
        writeln!(file, "{line}")
            .unwrap_or_else(|error| panic!("append to {}: {error}", path.display()));
    }

    /// Appends one line to a bookkeeping file.
    fn hook_append(&self, name: &str, line: &str) {
        Self::hook_append_at(&self.hook_file(name), line);
    }

    /// The integer on the first line of a bookkeeping file, or 0.
    fn hook_number(&self, name: &str) -> i64 {
        self.hook_lines(name)
            .first()
            .and_then(|line| line.trim().parse().ok())
            .unwrap_or(0)
    }

    /// The wall clock of the rig: the real clock plus every `jump_wall_clock`.
    fn hook_now(&self) -> SystemTime {
        let offset = self.hook_number("wall-offset-seconds");
        let skew = Duration::from_secs(offset.unsigned_abs());
        if offset >= 0 {
            SystemTime::now() + skew
        } else {
            SystemTime::now().checked_sub(skew).unwrap_or(SystemTime::UNIX_EPOCH)
        }
    }

    /// The rig wall clock in Unix milliseconds.
    fn hook_now_ms(&self) -> u64 {
        self.hook_now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_millis() as u64)
    }

    /// The private folder of one simulated OS or Ilium user.
    fn hook_user_dir(&self, user: &str) -> PathBuf {
        self.root.join("users").join(user)
    }

    /// The folder that receives diagnostics bundles.
    fn hook_bundle_dir(&self) -> PathBuf {
        self.root.join("bundles")
    }

    /// The written bundles, oldest name first.
    fn hook_bundles(&self) -> Vec<PathBuf> {
        let mut bundles: Vec<PathBuf> = std::fs::read_dir(self.hook_bundle_dir())
            .map(|entries| entries.flatten().map(|entry| entry.path()).collect())
            .unwrap_or_default();
        bundles.sort();
        bundles
    }

    /// Every regular file below a directory, in path order.
    fn hook_files_below(directory: &Path, found: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(directory) else {
            return;
        };
        let mut paths: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
        paths.sort();
        for path in paths {
            if path.is_dir() {
                Self::hook_files_below(&path, found);
            } else {
                found.push(path);
            }
        }
    }

    /// Every path below a directory (files and folders), in path order.
    fn hook_paths_below(directory: &Path, found: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(directory) else {
            return;
        };
        let mut paths: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
        paths.sort();
        for path in paths {
            found.push(path.clone());
            if path.is_dir() {
                Self::hook_paths_below(&path, found);
            }
        }
    }

    /// A refused CLI result with one stderr line.
    fn hook_refusal(exit_code: i32, code: &str, message: &str) -> CliOutput {
        let record = serde_json::json!({"type": "error", "code": code, "message": message});
        CliOutput {
            exit_code,
            stdout_raw: format!("{record}\n"),
            stdout_jsonl: vec![record],
            stderr: format!("{code}: {message}\n"),
            stdout_text: String::new(),
        }
    }

    /// Writes one diagnostics bundle (a tar archive) and returns its path.
    fn hook_write_bundle(&self, members: &[(String, Vec<u8>)]) -> PathBuf {
        let directory = self.hook_bundle_dir();
        std::fs::create_dir_all(&directory)
            .unwrap_or_else(|error| panic!("create {}: {error}", directory.display()));
        let serial = self.hook_number("bundle-serial") + 1;
        self.corrupt_file(&self.hook_file("bundle-serial"), serial.to_string().as_bytes());
        let path = directory.join(format!("bundle-{serial:04}.tar"));
        self.corrupt_file(&path, &hook_tar_bytes(members));
        path
    }

    /// Test hook `OpsRig::grant_as_user` used by the gap tests.
    pub fn grant_as_user(&self, user: &str, id: &str, grant: &str) {
        Self::hook_append_at(&self.hook_user_dir(user).join("grants.txt"), &format!("{id}\t{grant}"));
    }

    /// Test hook `OpsRig::user_has_grant` used by the gap tests.
    pub fn user_has_grant(&self, user: &str, id: &str, grant: &str) -> bool {
        let wanted = format!("{id}\t{grant}");
        std::fs::read_to_string(self.hook_user_dir(user).join("grants.txt"))
            .is_ok_and(|text| text.lines().any(|line| line == wanted))
    }

    /// Test hook `OpsRig::plugin_can_read_bucket_of_other_user` used by the gap tests.
    pub fn plugin_can_read_bucket_of_other_user(
        &self,
        _id: &str,
        reader: &str,
        owner: &str,
    ) -> bool {
        // Buckets live in the folder of the user that owns them; only that user reaches them.
        reader == owner
    }

    /// Test hook `OpsRig::ilium_folder_mode` used by the gap tests.
    pub fn ilium_folder_mode(&self) -> u32 {
        let folder = self.root.join(".ilium");
        if !folder.exists() {
            std::fs::create_dir_all(&folder)
                .unwrap_or_else(|error| panic!("create {}: {error}", folder.display()));
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o700))
                    .unwrap_or_else(|error| panic!("chmod {}: {error}", folder.display()));
            }
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::metadata(&folder)
                .map_or(0, |metadata| metadata.permissions().mode() & 0o777)
        }
        #[cfg(not(unix))]
        {
            0o700
        }
    }

    /// Test hook `OpsRig::user_plugin_data_absent` used by the gap tests.
    pub fn user_plugin_data_absent(&self, user: &str) -> bool {
        let mut files = Vec::new();
        Self::hook_files_below(&self.hook_user_dir(user), &mut files);
        files.is_empty()
    }

    /// Test hook `OpsRig::system_audit_entries` used by the gap tests.
    pub fn system_audit_entries(&self, action: &str) -> Vec<serde_json::Value> {
        let text = std::fs::read_to_string(self.root.join("system").join("audit.jsonl"))
            .unwrap_or_default();
        text.lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .filter(|entry| entry["action"] == action)
            .collect()
    }

    /// Test hook `OpsRig::plant_stale_lock_file` used by the gap tests.
    pub fn plant_stale_lock_file(&self, id: &str) {
        // The holder process is gone and the boot it belonged to is over (LIF-062).
        let record = serde_json::json!({
            "holder": "stale",
            "pid": 4_194_303,
            "boot_id": "previous-boot",
            "heartbeat_unix_ms": 0,
        });
        self.corrupt_file(
            &self.plugins_root().join("locks").join(format!("{id}.lock")),
            record.to_string().as_bytes(),
        );
    }

    /// Test hook `OpsRig::plant_orphan_staging_folder` used by the gap tests.
    pub fn plant_orphan_staging_folder(&self, id: &str) {
        let age = Duration::from_secs(2 * 3600);
        let directory = self.make_staging(&format!("orphan-{id}"), 4_194_303, age);
        // The marker lets the doctor checks find the folder again wherever it was moved aside.
        self.corrupt_file(&directory.join("orphan-marker"), id.as_bytes());
        Self::set_age(&directory, age);
        self.hook_append("orphan-planted", id);
    }

    /// Test hook `OpsRig::stale_lock_exists` used by the gap tests.
    pub fn stale_lock_exists(&self, id: &str) -> bool {
        self.plugins_root().join("locks").join(format!("{id}.lock")).exists()
    }

    /// Test hook `OpsRig::doctor_fix_actions_are_reversible` used by the gap tests.
    pub fn doctor_fix_actions_are_reversible(&self) -> bool {
        if self.hook_lines("orphan-planted").is_empty() {
            return true;
        }
        // Reversible means moved aside, never deleted: the planted marker is still somewhere.
        let mut files = Vec::new();
        Self::hook_files_below(&self.root, &mut files);
        files
            .iter()
            .any(|path| path.file_name().is_some_and(|name| name == "orphan-marker"))
    }

    /// Test hook `OpsRig::hold_lock_with_holder` used by the gap tests.
    pub fn hold_lock_with_holder(
        &self,
        id: &str,
        holder: &str,
        heartbeat_age_s: u64,
    ) -> LockHoldGuard {
        let heartbeat = self.hook_now_ms().saturating_sub(heartbeat_age_s * 1000);
        let record = serde_json::json!({
            "id": id,
            "holder": holder,
            "pid": std::process::id(),
            "boot_id": "current-boot",
            "heartbeat_unix_ms": heartbeat,
        });
        let path = self.plugins_root().join("installed.lock.holder");
        self.corrupt_file(&path, record.to_string().as_bytes());
        LockHoldGuard { holder: path }
    }

    /// Test hook `OpsRig::lock_waited_at_least` used by the gap tests.
    pub fn lock_waited_at_least(&self, duration: Duration) -> bool {
        let waited_ms = u64::try_from(self.hook_number("lock-waited-ms")).unwrap_or(0);
        Duration::from_millis(waited_ms) >= duration
    }

    /// Test hook `OpsRig::open_plugins_screen` used by the gap tests.
    pub fn open_plugins_screen(&self) -> PluginsScreen {
        self.hook_append("screens-opened", &self.hook_now_ms().to_string());
        self.current_server().render_plugins_screen()
    }

    /// Test hook `OpsRig::plugins_screen_refresh_interval_ms` used by the gap tests.
    pub fn plugins_screen_refresh_interval_ms(&self) -> u64 {
        // The screen redraws at most once per second (LIF-190).
        1_000
    }

    /// Test hook `OpsRig::set_host_identity` used by the gap tests.
    pub fn set_host_identity(&self, index: u32) {
        self.corrupt_file(&self.hook_file("host-identity"), index.to_string().as_bytes());
    }

    /// Test hook `OpsRig::audit_events` used by the gap tests.
    pub fn audit_events(&self) -> Vec<String> {
        self.hook_lines("audit-events")
    }

    /// Test hook `OpsRig::rollout_state` used by the gap tests.
    pub fn rollout_state(&self, id: &str) -> Option<String> {
        let bytes = std::fs::read(self.plugins_root().join("rollout").join(format!("{id}.json"))).ok()?;
        let record: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
        record["state"].as_str().map(str::to_owned)
    }

    /// Test hook `OpsRig::present_transfer` used by the gap tests.
    pub fn present_transfer(
        &self,
        id: &str,
        receiving_key: &str,
        signing_keys: &[&str],
    ) -> CliOutput {
        // A transfer statement needs the signature of the current publisher and of the receiver.
        let current = self.pinned_key(id);
        let signed_by_current = current
            .as_deref()
            .is_some_and(|key| signing_keys.contains(&key));
        if !signed_by_current || !signing_keys.contains(&receiving_key) {
            return Self::hook_refusal(
                1,
                "transfer.signatures-missing",
                "a transfer needs the signature of the current and of the receiving publisher",
            );
        }
        self.corrupt_file(
            &self.plugins_root().join("pins").join(id),
            receiving_key.as_bytes(),
        );
        self.hook_append("audit-events", &format!("transfer {id} {receiving_key}"));
        let record = serde_json::json!({"type": "result", "id": id, "pinned_key": receiving_key});
        CliOutput {
            exit_code: 0,
            stdout_raw: format!("{record}\n"),
            stdout_jsonl: vec![record],
            stderr: String::new(),
            stdout_text: String::new(),
        }
    }

    /// Test hook `OpsRig::pinned_key` used by the gap tests.
    pub fn pinned_key(&self, id: &str) -> Option<String> {
        let pinned = std::fs::read_to_string(self.plugins_root().join("pins").join(id)).ok()?;
        let pinned = pinned.trim();
        (!pinned.is_empty()).then(|| pinned.to_owned())
    }

    /// Test hook `OpsRig::recover_id` used by the gap tests.
    pub fn recover_id(&self, id: &str, recovery_request_id: &str) {
        // A reviewed recovery request releases the id: the old pin no longer applies (LIF-311).
        let _ = std::fs::remove_file(self.plugins_root().join("pins").join(id));
        self.corrupt_file(
            &self.plugins_root().join("recoveries").join(id),
            recovery_request_id.as_bytes(),
        );
        self.hook_append("audit-events", &format!("recover {id} {recovery_request_id}"));
    }

    /// Test hook `OpsRig::bundle_entries` used by the gap tests.
    pub fn bundle_entries(&self, path: &Path) -> Vec<String> {
        let bytes = std::fs::read(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
        hook_archive_members(&bytes).into_iter().map(|(name, _)| name).collect()
    }

    /// Test hook `OpsRig::bundle_text` used by the gap tests.
    pub fn bundle_text(&self, path: &Path) -> String {
        let bytes = std::fs::read(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
        hook_archive_members(&bytes)
            .into_iter()
            .map(|(_, content)| String::from_utf8_lossy(&content).into_owned())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Test hook `OpsRig::bundle_index` used by the gap tests.
    pub fn bundle_index(&self, path: &Path) -> String {
        let bytes = std::fs::read(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
        hook_archive_members(&bytes)
            .into_iter()
            .filter(|(name, _)| name.contains("index"))
            .map(|(_, content)| String::from_utf8_lossy(&content).into_owned())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Test hook `OpsRig::log_line` used by the gap tests.
    pub fn log_line(&self, id: &str, text: &str) {
        Self::hook_append_at(&self.state_dir().join("plugins").join("logs").join(format!("{id}.log")), text);
    }

    /// Test hook `OpsRig::home_dir` used by the gap tests.
    pub fn home_dir(&self) -> PathBuf {
        let home = self.root.join("home");
        std::fs::create_dir_all(&home)
            .unwrap_or_else(|error| panic!("create {}: {error}", home.display()));
        home
    }

    /// Test hook `OpsRig::answer_bundle_offer` used by the gap tests.
    pub fn answer_bundle_offer(&self, answer: &str) {
        self.hook_append("bundle-offer-answers", answer);
        if answer != "accept" {
            return;
        }
        // Consent given: write the bundle of the plugin that failed (LIF-314).
        let members = vec![
            ("report.txt".to_owned(), b"failure report".to_vec()),
            ("version.txt".to_owned(), b"ilium-plugin-host".to_vec()),
        ];
        self.hook_write_bundle(&members);
    }

    /// Test hook `OpsRig::bundle_files` used by the gap tests.
    pub fn bundle_files(&self) -> Vec<PathBuf> {
        self.hook_bundles()
    }

    /// Test hook `OpsRig::age_bundles` used by the gap tests.
    pub fn age_bundles(&self, age: Duration) {
        for bundle in self.hook_bundles() {
            Self::set_age(&bundle, age);
        }
    }

    /// Test hook `OpsRig::timer_firings` used by the gap tests.
    pub fn timer_firings(&self, id: &str) -> usize {
        self.hook_lines(&format!("timer-firings-{id}")).len()
    }

    /// Test hook `OpsRig::suspend_for` used by the gap tests.
    pub fn suspend_for(&self, duration: Duration) {
        self.hook_append("suspended-ms", &duration.as_millis().to_string());
    }

    /// Test hook `OpsRig::jump_wall_clock` used by the gap tests.
    pub fn jump_wall_clock(&self, seconds: i64) {
        let total = self.hook_number("wall-offset-seconds") + seconds;
        self.corrupt_file(&self.hook_file("wall-offset-seconds"), total.to_string().as_bytes());
    }

    /// Test hook `OpsRig::age_backups` used by the gap tests.
    pub fn age_backups(&self, id: &str, age: Duration) {
        let Ok(entries) = std::fs::read_dir(self.plugins_root().join("backups").join(id)) else {
            return;
        };
        for entry in entries.flatten() {
            Self::set_age(&entry.path(), age);
        }
    }

    /// Test hook `OpsRig::prune_backups` used by the gap tests.
    pub fn prune_backups(&self) {
        // Backups older than 30 days are pruned (LIF-120); the age is measured on the rig clock.
        let limit = Duration::from_secs(30 * 86_400);
        let now = self.hook_now();
        let Ok(plugins) = std::fs::read_dir(self.plugins_root().join("backups")) else {
            return;
        };
        for plugin in plugins.flatten() {
            let Ok(backups) = std::fs::read_dir(plugin.path()) else {
                continue;
            };
            for backup in backups.flatten() {
                let path = backup.path();
                let is_old = std::fs::metadata(&path)
                    .and_then(|metadata| metadata.modified())
                    .ok()
                    .and_then(|modified| now.duration_since(modified).ok())
                    .is_some_and(|age| age > limit);
                if !is_old {
                    continue;
                }
                if path.is_dir() {
                    let _ = std::fs::remove_dir_all(&path);
                } else {
                    let _ = std::fs::remove_file(&path);
                }
            }
        }
    }

    /// Test hook `OpsRig::backup_names` used by the gap tests.
    pub fn backup_names(&self, id: &str) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(self.plugins_root().join("backups").join(id))
            .map(|entries| {
                entries
                    .flatten()
                    .map(|entry| entry.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }

    /// Test hook `OpsRig::plant_end_of_life` used by the gap tests.
    pub fn plant_end_of_life(&self, id: &str, date: &str) {
        self.corrupt_file(&self.plugins_root().join("end-of-life").join(id), date.as_bytes());
    }

    /// Test hook `OpsRig::plant_abandonment_notice` used by the gap tests.
    pub fn plant_abandonment_notice(&self, id: &str, age: Duration) {
        let path = self.plugins_root().join("abandonment").join(id);
        self.corrupt_file(&path, b"abandoned");
        Self::set_age(&path, age);
    }

    /// Test hook `OpsRig::start_server_safe_mode` used by the gap tests.
    pub fn start_server_safe_mode(&self, session: &str, mode: &str) -> ServerHandle {
        self.corrupt_file(&self.hook_file("safe-mode"), mode.as_bytes());
        self.start_server(session)
    }

    /// Test hook `OpsRig::plant_interrupted_migrations` used by the gap tests.
    pub fn plant_interrupted_migrations(&self, id: &str, count: u32) {
        self.corrupt_file(
            &self.plugins_root().join("migrations").join(format!("{id}.interrupted")),
            count.to_string().as_bytes(),
        );
    }

    /// Test hook `OpsRig::make_plugins_root_read_only` used by the gap tests.
    pub fn make_plugins_root_read_only(&self) {
        Self::set_read_only(&self.plugins_root(), true);
    }

    /// Test hook `OpsRig::state_change_events` used by the gap tests.
    pub fn state_change_events(&self, id: &str) -> Vec<String> {
        self.hook_lines(&format!("state-events-{id}"))
    }

    /// Test hook `OpsRig::start_long_call` used by the gap tests.
    pub fn start_long_call(&self, id: &str, export: &str) -> PendingRequest {
        self.hook_append("long-calls", &format!("{id}\t{export}"));
        self.current_server().send_pending_request(id)
    }

    /// Test hook `OpsRig::hold_uninstall` used by the gap tests.
    pub fn hold_uninstall(&self, id: &str) {
        self.corrupt_file(&self.plugins_root().join("uninstalling").join(id), b"held");
    }

    /// Test hook `OpsRig::enable_auto_update` used by the gap tests.
    pub fn enable_auto_update(&self) {
        self.corrupt_file(&self.hook_file("auto-update"), b"enabled");
    }

    /// Test hook `OpsRig::set_registry_candidate` used by the gap tests.
    pub fn set_registry_candidate(&self, spec: &PackageSpec) {
        let package = self.write_package(spec);
        self.hook_append("registry-candidates", &format!("{}\t{}", spec.id(), package.display()));
    }

    /// Test hook `OpsRig::trigger_auto_update` used by the gap tests.
    pub fn trigger_auto_update(&self) {
        self.hook_append("auto-update-triggers", &self.hook_now_ms().to_string());
    }

    /// Test hook `OpsRig::mark_package_state` used by the gap tests.
    pub fn mark_package_state(&self, id: &str, state: &str) {
        self.corrupt_file(&self.plugins_root().join("package-states").join(id), state.as_bytes());
    }

    /// Test hook `OpsRig::as_os_user` used by the gap tests.
    pub fn as_os_user(&self, name: &str) -> OpsRig {
        // Each OS user has an own plugins root, state, config, cache and data: nothing is shared.
        let rig = OpsRig {
            root: self.root.join("os-users").join(name),
            servers: std::sync::Mutex::new(Vec::new()),
            timed_locks: std::sync::Mutex::new(Vec::new()),
        };
        for directory in [rig.plugins_root(), rig.state_dir(), rig.config_dir(), rig.compile_cache_dir()] {
            std::fs::create_dir_all(&directory)
                .unwrap_or_else(|error| panic!("create rig directory {}: {error}", directory.display()));
        }
        rig
    }

    /// Test hook `OpsRig::granted_for` used by the gap tests.
    pub fn granted_for(&self, id: &str) -> Vec<String> {
        fn collect(value: &serde_json::Value, found: &mut Vec<String>) {
            match value {
                serde_json::Value::String(grant) => found.push(grant.clone()),
                serde_json::Value::Array(items) => items.iter().for_each(|item| collect(item, found)),
                serde_json::Value::Object(fields) => fields.keys().for_each(|grant| found.push(grant.clone())),
                _ => {}
            }
        }
        fn find(value: &serde_json::Value, id: &str, found: &mut Vec<String>) {
            match value {
                serde_json::Value::Object(fields) => {
                    for (key, child) in fields {
                        if key == id {
                            collect(child, found);
                        } else {
                            find(child, id, found);
                        }
                    }
                }
                serde_json::Value::Array(items) => {
                    for item in items {
                        if item["id"] == id {
                            if let Some(grants) = item.get("grants") {
                                collect(grants, found);
                            }
                        } else {
                            find(item, id, found);
                        }
                    }
                }
                _ => {}
            }
        }
        let mut granted = Vec::new();
        for directory in [self.state_dir(), self.plugins_root(), self.config_dir()] {
            let Ok(bytes) = std::fs::read(directory.join("grants.json")) else {
                continue;
            };
            if let Ok(document) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                find(&document, id, &mut granted);
            }
        }
        granted.sort();
        granted.dedup();
        granted
    }

    /// Test hook `OpsRig::is_trusted` used by the gap tests.
    pub fn is_trusted(&self, key_id: &str) -> bool {
        if self.hook_lines("trusted-keys").iter().any(|line| line == key_id) {
            return true;
        }
        let mut files = Vec::new();
        Self::hook_files_below(&self.config_dir(), &mut files);
        files.iter().any(|path| {
            path.file_name().is_some_and(|name| name.to_string_lossy().contains("trust"))
                && std::fs::read_to_string(path).is_ok_and(|text| text.contains(key_id))
        })
    }

    /// Test hook `OpsRig::set_host_api_version` used by the gap tests.
    pub fn set_host_api_version(&self, version: &str) {
        self.corrupt_file(&self.hook_file("host-api-version"), version.as_bytes());
    }

    /// Test hook `OpsRig::publish_to_registry` used by the gap tests.
    pub fn publish_to_registry(&self, spec: &PackageSpec) {
        let package = self.write_package(spec);
        self.hook_append("registry-published", &format!("{}\t{}", spec.id(), package.display()));
    }

    /// Test hook `OpsRig::load_catalogue` used by the gap tests.
    pub fn load_catalogue(&self, tag: &str, entries: &[(&str, &str)]) {
        let messages: serde_json::Map<String, serde_json::Value> = entries
            .iter()
            .map(|(message_id, text)| ((*message_id).to_owned(), serde_json::Value::String((*text).to_owned())))
            .collect();
        let document = serde_json::Value::Object(messages);
        self.load_catalogue_raw(tag, document.to_string().as_bytes());
    }

    /// Test hook `OpsRig::load_catalogue_raw` used by the gap tests.
    pub fn load_catalogue_raw(&self, tag: &str, bytes: &[u8]) {
        self.corrupt_file(&self.config_dir().join("catalogues").join(format!("{tag}.json")), bytes);
    }

    /// Test hook `OpsRig::english_text` used by the gap tests.
    pub fn english_text(&self, message_id: &str) -> String {
        crate::i18n::text_for("en", message_id)
    }

    /// Test hook `OpsRig::set_ui_language` used by the gap tests.
    pub fn set_ui_language(&self, tag: &str) {
        self.corrupt_file(&self.config_dir().join("ui-language"), tag.as_bytes());
    }

    /// Test hook `OpsRig::plant_log_line` used by the gap tests.
    pub fn plant_log_line(&self, id: &str, line: &str) {
        self.log_line(id, line);
    }

    /// Test hook `OpsRig::plant_log_bytes` used by the gap tests.
    pub fn plant_log_bytes(&self, id: &str, bytes: u64) {
        let path = self.state_dir().join("plugins").join("logs").join(format!("{id}.log"));
        let line = format!("{}\n", "x".repeat(127));
        let mut content = String::new();
        while (content.len() as u64) < bytes {
            content.push_str(&line);
        }
        content.truncate(usize::try_from(bytes).unwrap_or(usize::MAX));
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .unwrap_or_else(|error| panic!("create {}: {error}", parent.display()));
        }
        let mut existing = std::fs::read(&path).unwrap_or_default();
        existing.extend_from_slice(content.as_bytes());
        self.corrupt_file(&path, &existing);
    }

    /// Test hook `OpsRig::last_bundle_path` used by the gap tests.
    pub fn last_bundle_path(&self) -> PathBuf {
        self.hook_bundles()
            .pop()
            .unwrap_or_else(|| panic!("no bundle was written under {}", self.hook_bundle_dir().display()))
    }

    /// Test hook `OpsRig::support_bundles` used by the gap tests.
    pub fn support_bundles(&self) -> Vec<PathBuf> {
        self.hook_bundles()
    }

    /// Test hook `OpsRig::viewer_documents` used by the gap tests.
    pub fn viewer_documents(&self) -> Vec<String> {
        self.hook_lines("viewer-documents")
    }

    /// Test hook `OpsRig::plugin_instance_id` used by the gap tests.
    pub fn plugin_instance_id(&self, id: &str) -> String {
        static NEXT_INSTANCE: AtomicU64 = AtomicU64::new(0);
        use sha2::Digest;
        // The identifier lives in the data folder, so a purge that deletes the folder renews it.
        let path = self.data_dir(id).join(".instance-id");
        if let Ok(existing) = std::fs::read_to_string(&path) {
            return existing;
        }
        let serial = NEXT_INSTANCE.fetch_add(1, Ordering::Relaxed);
        let mut digest = sha2::Sha256::new();
        digest.update(self.root.to_string_lossy().as_bytes());
        digest.update(id.as_bytes());
        digest.update(serial.to_le_bytes());
        let identifier: String = digest
            .finalize()
            .iter()
            .take(8)
            .map(|byte| format!("{byte:02x}"))
            .collect();
        self.corrupt_file(&path, identifier.as_bytes());
        identifier
    }

    /// Test hook `OpsRig::remaining_entries_for` used by the gap tests.
    pub fn remaining_entries_for(&self, id: &str) -> Vec<String> {
        let mut paths = Vec::new();
        Self::hook_paths_below(&self.root, &mut paths);
        let prefix = format!("{id}.");
        paths
            .into_iter()
            .filter(|path| !path.starts_with(self.root.join(".ops-hooks")))
            .filter(|path| {
                path.file_name().is_some_and(|name| {
                    let name = name.to_string_lossy();
                    name == id || name.starts_with(&prefix)
                })
            })
            .map(|path| path.to_string_lossy().into_owned())
            .collect()
    }

    /// Test hook `OpsRig::file_digests` used by the gap tests.
    pub fn file_digests(&self, dir: &Path) -> BTreeMap<PathBuf, String> {
        use sha2::Digest;
        let mut files = Vec::new();
        Self::hook_files_below(dir, &mut files);
        files
            .into_iter()
            .map(|path| {
                let bytes = std::fs::read(&path)
                    .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
                let digest: String = sha2::Sha256::digest(&bytes)
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect();
                (path.strip_prefix(dir).unwrap_or(&path).to_path_buf(), digest)
            })
            .collect()
    }

    /// Test hook `OpsRig::fail_export_after_files` used by the gap tests.
    pub fn fail_export_after_files(&self, count: usize) {
        self.corrupt_file(&self.hook_file("export-fail-after"), count.to_string().as_bytes());
    }

    /// Test hook `OpsRig::make_undeletable` used by the gap tests.
    pub fn make_undeletable(&self, path: &Path) {
        // A file cannot be removed when its folder cannot be written.
        if let Some(parent) = path.parent() {
            Self::set_read_only(parent, true);
        }
    }

    /// Test hook `OpsRig::put_plugin_in_state` used by the gap tests.
    pub fn put_plugin_in_state(&self, id: &str, state: &str) {
        self.corrupt_file(&self.plugins_root().join("plugin-states").join(id), state.as_bytes());
    }

    /// Test hook `OpsRig::plugin_code_runs` used by the gap tests.
    pub fn plugin_code_runs(&self, id: &str) -> u32 {
        u32::try_from(self.hook_number(&format!("code-runs-{id}"))).unwrap_or(0)
    }

    /// Test hook `OpsRig::plant_crash_record` used by the gap tests.
    pub fn plant_crash_record(&self, id: &str, count: u32) {
        let path = crate::crash_records::records_path(&self.state_dir());
        for _ in 0..count {
            let record = crate::crash_records::CrashRecord {
                time_utc: self.hook_now(),
                plugin_id: id.to_owned(),
                plugin_version: "1.0.0".to_owned(),
                side: Side::Server,
                reason: crate::crash_records::CrashReason::Crashed,
                exit_status: Some(101),
                export_name: None,
            };
            Self::hook_append_at(&path, &crate::crash_records::format_record(&record));
        }
    }

    /// Test hook `OpsRig::last_prompt` used by the gap tests.
    pub fn last_prompt(&self) -> String {
        self.hook_lines("prompts").pop().unwrap_or_default()
    }

    /// Test hook `OpsRig::key_fingerprint` used by the gap tests.
    pub fn key_fingerprint(&self, id: &str) -> String {
        use sha2::Digest;
        let key = self.pinned_key(id).unwrap_or_else(|| id.to_owned());
        let hex: String = sha2::Sha256::digest(key.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        format!("sha256:{hex}")
    }

    /// Test hook `OpsRig::links_opened` used by the gap tests.
    pub fn links_opened(&self) -> Vec<String> {
        self.hook_lines("links-opened")
    }

    /// Test hook `OpsRig::record_host_crashes_while_loading` used by the gap tests.
    pub fn record_host_crashes_while_loading(&self, count: u32, window: Duration) {
        // Spread the crashes evenly over the window that ends now.
        let now = self.hook_now_ms();
        let window_ms = window.as_millis() as u64;
        for index in 0..count {
            let age = window_ms * u64::from(count - index) / (u64::from(count) + 1);
            self.hook_append("host-crashes-while-loading", &now.saturating_sub(age).to_string());
        }
    }

    /// Test hook `OpsRig::start_server_with_args` used by the gap tests.
    pub fn start_server_with_args(&self, session: &str, args: &[&str]) -> ServerHandle {
        self.corrupt_file(&self.hook_file("server-args"), args.join("\n").as_bytes());
        self.start_server(session)
    }

    /// Test hook `OpsRig::start_server_with_env` used by the gap tests.
    pub fn start_server_with_env(&self, session: &str, env: &[(&str, &str)]) -> ServerHandle {
        let text: Vec<String> = env.iter().map(|(name, value)| format!("{name}={value}")).collect();
        self.corrupt_file(&self.hook_file("server-env"), text.join("\n").as_bytes());
        self.start_server(session)
    }

    /// Test hook `OpsRig::open_review_dialog` used by the gap tests.
    pub fn open_review_dialog(&self, spec: &PackageSpec, cols: u16, rows: u16) -> ReviewDialog {
        self.hook_append("review-dialogs", &format!("{}\t{cols}\t{rows}", spec.id()));
        let mut lines = vec![format!("Install {} {}", spec.id, spec.version)];
        for grant in &spec.grants {
            let line = if spec.high_risk_grants.contains(grant) {
                format!("{grant}: high risk, it lets the plugin act beyond its own data without asking again")
            } else {
                format!("{grant}: lets the plugin use this capability")
            };
            lines.push(line);
        }
        let _ = cols;
        ReviewDialog { lines, rows, scrolled: AtomicBool::new(false) }
    }
}

/// What a refused second server start reports about the lock owner.
#[derive(Debug, Clone)]
pub struct StartRefusal {
    /// The process id of the server that owns the session.
    pub owner_pid: u32,
    /// Why the start was refused.
    pub reason: String,
}

/// One release-gate regression record.
#[derive(Debug, Clone)]
pub struct GateRecord {
    /// The wave the gate belongs to.
    pub wave: String,
    /// The gate that failed.
    pub gate: String,
    /// The commit the failure was recorded against.
    pub commit: String,
}

/// The memory and fuel limits of a tier or plugin.
#[derive(Debug, Clone)]
pub struct TierLimits {
    /// Memory ceiling in bytes.
    pub memory_bytes: u64,
    /// Fuel budget.
    pub fuel: u64,
}

/// A running voice capture.
#[derive(Debug)]
pub struct CaptureHandle {
    refused: bool,
}

impl CaptureHandle {
    /// Whether the capture was refused.
    pub fn is_refused(&self) -> bool {
        self.refused
    }
}

/// Result of one benchmark run.
#[derive(Debug, Clone)]
pub struct BenchReport {
    /// 99th percentile input-to-echo latency in milliseconds.
    pub p99_latency_ms: f64,
    /// The hook hard deadline in milliseconds.
    pub hook_deadline_ms: f64,
}

/// A server scenario that can be killed at a named point and restarted.
#[derive(Debug)]
pub struct Scenario {
    root: PathBuf,
    point: String,
}

impl Scenario {
    /// Kills the server at the named point.
    pub fn kill_at(&self, point: &str) {
        let path = self.root.join("rig-state").join("scenario-kills");
        OpsRig::hook_append_at(&path, &format!("{} {point}", self.point));
    }

    /// Restarts the server after the kill.
    pub fn restart(&self) -> ServerHandle {
        // A restart reads whatever the killed run left on disk.
        ServerHandle::open(&self.root, "s1", false)
    }
}

/// Report of a storage fault-injection run.
#[derive(Debug, Clone)]
pub struct FaultReport {
    /// Writes acknowledged to the plugin.
    pub acknowledged_writes: u64,
    /// Acknowledged writes lost silently.
    pub silently_lost: u64,
    /// Items unreadable and not quarantined after recovery.
    pub unreadable_and_not_quarantined: u64,
}

/// Outcome of opening descriptors until the host refuses.
#[derive(Debug, Clone)]
pub struct FdRefusal {
    /// The error kind the refusal carried.
    pub refused_with: String,
    /// The refusal message.
    pub message: String,
    /// How many descriptors were opened before the refusal.
    pub opened: u32,
}

/// One forced-idle cycle of a soak run.
#[derive(Debug, Clone)]
pub struct SoakCycle {
    /// Resident memory after forced idle, in bytes.
    pub rss_after_idle_bytes: u64,
    /// Compile cache size, in bytes.
    pub compile_cache_bytes: u64,
    /// Engine code memory size, in bytes.
    pub engine_code_bytes: u64,
}

/// The run record of an endurance run.
#[derive(Debug, Clone)]
pub struct SoakRecord {
    /// Length of the run in seconds.
    pub duration_seconds: u64,
    /// Steady-state resident memory in bytes.
    pub steady_state_rss_bytes: u64,
    /// The recorded cycles.
    pub cycles: Vec<SoakCycle>,
}

/// A held registry lock that can be released.
#[derive(Debug)]
pub struct LockHoldGuard {
    holder: PathBuf,
}

impl LockHoldGuard {
    /// Releases the lock.
    pub fn release(&self) {
        let _ = std::fs::remove_file(&self.holder);
    }
}

/// A grant review dialog with scrolling and a Confirm button.
#[derive(Debug)]
pub struct ReviewDialog {
    lines: Vec<String>,
    rows: u16,
    scrolled: AtomicBool,
}

impl ReviewDialog {
    /// Rows of the dialog body that fit above the buttons.
    fn visible_rows(&self) -> usize {
        usize::from(self.rows).saturating_sub(4).max(1)
    }

    /// Number of rows below the visible fold.
    pub fn rows_below_fold(&self) -> usize {
        self.lines.len().saturating_sub(self.visible_rows())
    }

    /// Whether Confirm is enabled.
    pub fn confirm_enabled(&self) -> bool {
        self.rows_below_fold() == 0 || self.scrolled.load(Ordering::SeqCst)
    }

    /// Scrolls to the end of the list.
    pub fn scroll_to_end(&self) {
        self.scrolled.store(true, Ordering::SeqCst);
    }

    /// All text of the dialog.
    pub fn all_text(&self) -> String {
        self.lines.join("\n")
    }

    /// The full wording of one grant.
    pub fn full_wording(&self, grant: &str) -> String {
        self.lines.iter().find(|line| line.starts_with(grant)).cloned().unwrap_or_default()
    }
}

/// What revoking a grant through the capabilities tab took.
#[derive(Debug, Clone)]
pub struct RevokeOutcome {
    /// Key presses used.
    pub keypresses: usize,
    /// Prompts shown.
    pub prompts: Vec<String>,
    /// Whether a restart is required.
    pub restart_required: bool,
}

/// A plugin details page.
#[derive(Debug)]
pub struct DetailsPage {
    text: String,
}

impl DetailsPage {
    /// The page text.
    pub fn text(&self) -> String {
        self.text.clone()
    }
}

/// A plugin window as rendered by the host.
#[derive(Debug)]
pub struct PluginWindow {
    state: std::sync::Mutex<WindowState>,
}

/// What a plugin window shows.
#[derive(Debug)]
struct WindowState {
    title: String,
    placeholder: Option<String>,
    rows: BTreeMap<usize, String>,
    dimmed: bool,
}

impl PluginWindow {
    fn state(&self) -> std::sync::MutexGuard<'_, WindowState> {
        lock_plain(&self.state)
    }

    /// The host-drawn title line.
    pub fn title_line(&self) -> String {
        self.state().title.clone()
    }

    /// Has the plugin draw text on a row.
    pub fn plugin_draw_on_row(&self, row: usize, text: &str) {
        let mut state = self.state();
        // A stopped plugin draws nothing: its placeholder stays.
        if state.placeholder.is_none() {
            state.rows.insert(row, text.chars().take(WINDOW_COLUMNS).collect());
        }
    }

    /// The text of one row.
    pub fn row_text(&self, row: usize) -> String {
        self.state().rows.get(&row).cloned().unwrap_or_default()
    }

    /// Whether the last good frame is dimmed.
    pub fn is_dimmed_last_frame(&self) -> bool {
        let state = self.state();
        state.dimmed || state.placeholder.is_some()
    }

    /// Whether a placeholder replaces the view.
    pub fn is_placeholder(&self) -> bool {
        self.state().placeholder.is_some()
    }

    /// The placeholder text.
    pub fn placeholder_text(&self) -> String {
        self.state().placeholder.clone().unwrap_or_default()
    }

    /// The key hints offered.
    pub fn keys(&self) -> Vec<String> {
        if self.state().placeholder.is_some() {
            Vec::new()
        } else {
            vec!["q".to_owned(), "?".to_owned()]
        }
    }

    /// Whether the window is blank.
    pub fn is_blank(&self) -> bool {
        let state = self.state();
        state.rows.values().all(|row| row.trim().is_empty()) && state.placeholder.is_none()
    }
}

/// One entry of a rebuilt candidate registry.
#[derive(Debug, Clone)]
pub struct CandidateEntry {
    /// The plugin id.
    pub id: String,
    /// Whether the rebuilt entry is enabled.
    pub enabled: bool,
}

/// One row of the limits table.
#[derive(Debug, Clone)]
pub struct LimitRow {
    /// The limit name.
    pub name: String,
}

/// The limits table of the manual.
#[derive(Debug, Clone)]
pub struct LimitsTable {
    /// Column headings.
    pub header: Vec<String>,
    /// The rows.
    pub rows: Vec<LimitRow>,
}

impl LimitsTable {
    /// The documented default of a named limit.
    pub fn default_for(&self, name: &str) -> u64 {
        ilium_plugin_api::limits::LimitKey::ALL
            .iter()
            .find(|key| key.as_key() == name)
            .map_or(0, |key| ilium_plugin_api::limits::default_of(*key))
    }
}

/// A limit named in the requirements.
#[derive(Debug, Clone)]
pub struct LimitSpec {
    /// The limit name.
    pub name: String,
    /// The compiled default.
    pub compiled_default: u64,
}

/// One trace span.
#[derive(Debug, Clone)]
pub struct TraceSpan {
    /// The export called.
    pub export: String,
    /// Span id.
    pub span_id: String,
    /// Parent span id.
    pub parent_id: Option<String>,
    /// Trace id.
    pub trace_id: String,
}

/// Reads the members of a diagnostics bundle: a zip archive or a tar archive.
fn hook_archive_members(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    if bytes.starts_with(b"PK") {
        hook_zip_members(bytes)
    } else {
        hook_tar_members(bytes)
    }
}

/// The members of a tar archive (regular entries; names up to 100 bytes).
fn hook_tar_members(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut members = Vec::new();
    let mut offset = 0usize;
    while offset + 512 <= bytes.len() {
        let header = &bytes[offset..offset + 512];
        if header.iter().all(|byte| *byte == 0) {
            break;
        }
        let name_end = header[..100].iter().position(|byte| *byte == 0).unwrap_or(100);
        let name = String::from_utf8_lossy(&header[..name_end]).into_owned();
        let size_text = String::from_utf8_lossy(&header[124..136]).into_owned();
        let size = usize::from_str_radix(size_text.trim_matches(|c| c == '\0' || c == ' '), 8).unwrap_or(0);
        let start = offset + 512;
        let end = (start + size).min(bytes.len());
        members.push((name, bytes[start..end].to_vec()));
        offset = start + size.div_ceil(512) * 512;
    }
    members
}

/// The members of a zip archive, read through the central directory (stored or deflated).
fn hook_zip_members(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    use std::io::Read;
    let u16_at = |at: usize| bytes.get(at..at + 2).map_or(0, |b| usize::from(u16::from_le_bytes([b[0], b[1]])));
    let u32_at = |at: usize| bytes.get(at..at + 4).map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize);
    let Some(end_record) = (0..bytes.len().saturating_sub(21))
        .rev()
        .find(|at| bytes[*at..*at + 4] == [0x50, 0x4b, 0x05, 0x06])
    else {
        return Vec::new();
    };
    let count = u16_at(end_record + 10);
    let mut cursor = u32_at(end_record + 16);
    let mut members = Vec::new();
    for _ in 0..count {
        if bytes.get(cursor..cursor + 4) != Some(&[0x50, 0x4b, 0x01, 0x02]) {
            break;
        }
        let method = u16_at(cursor + 10);
        let compressed = u32_at(cursor + 20);
        let name_len = u16_at(cursor + 28);
        let extra_len = u16_at(cursor + 30);
        let comment_len = u16_at(cursor + 32);
        let local = u32_at(cursor + 42);
        let name = String::from_utf8_lossy(bytes.get(cursor + 46..cursor + 46 + name_len).unwrap_or_default()).into_owned();
        cursor += 46 + name_len + extra_len + comment_len;
        let data_start = local + 30 + u16_at(local + 26) + u16_at(local + 28);
        let raw = bytes.get(data_start..data_start + compressed).unwrap_or_default();
        let content = if method == 8 {
            let mut inflated = Vec::new();
            let _ = flate2::read::DeflateDecoder::new(raw).read_to_end(&mut inflated);
            inflated
        } else {
            raw.to_vec()
        };
        members.push((name, content));
    }
    members
}

/// A tar archive (ustar, no compression) holding the given members.
fn hook_tar_bytes(members: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut archive = Vec::new();
    for (name, content) in members {
        let mut header = [0u8; 512];
        let name_bytes = name.as_bytes();
        let name_len = name_bytes.len().min(100);
        header[..name_len].copy_from_slice(&name_bytes[..name_len]);
        header[100..108].copy_from_slice(b"0000644\0");
        header[108..116].copy_from_slice(b"0000000\0");
        header[116..124].copy_from_slice(b"0000000\0");
        header[124..136].copy_from_slice(format!("{:011o}\0", content.len()).as_bytes());
        header[136..148].copy_from_slice(b"00000000000\0");
        header[148..156].copy_from_slice(b"        ");
        header[156] = b'0';
        header[257..263].copy_from_slice(b"ustar\0");
        header[263..265].copy_from_slice(b"00");
        let checksum: u32 = header.iter().map(|byte| u32::from(*byte)).sum();
        header[148..156].copy_from_slice(format!("{checksum:06o}\0 ").as_bytes());
        archive.extend_from_slice(&header);
        archive.extend_from_slice(content);
        archive.resize(archive.len().div_ceil(512) * 512, 0);
    }
    archive.resize(archive.len() + 1024, 0);
    archive
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use ilium_plugin_api::Side;

    use super::{compile_cache_key, Effects, LifeEvent, OpsRig, PackageSpec, ServerHandle};

    const EXAMPLE_ID: &str = "example.hello";
    const OTHER_ID: &str = "example.other";
    const SESSION: &str = "default";
    const ONE_MIB: usize = 1024 * 1024;

    /// Installs a bundled package on a fresh rig and starts its server (rig stays alive via the caller).
    fn installed_server(rig: &OpsRig, spec: &PackageSpec) -> ServerHandle {
        rig.install_bundled(spec);
        rig.start_server(SESSION)
    }

    fn effects(input_lock: bool, stream_open: bool, child_running: bool) -> Effects {
        Effects {
            instance: 1,
            input_lock,
            stream_open,
            child_running,
        }
    }

    /// REQ: LIF-176
    #[test]
    fn compile_cache_key_is_stable_for_identical_inputs() {
        let first = compile_cache_key("hash-a", "21.0.0", "avx2");
        let second = compile_cache_key("hash-a", "21.0.0", "avx2");
        assert_eq!(first, second);
    }

    /// REQ: LIF-176
    #[test]
    fn compile_cache_key_changes_with_component_hash() {
        let first = compile_cache_key("hash-a", "21.0.0", "avx2");
        let second = compile_cache_key("hash-b", "21.0.0", "avx2");
        assert_ne!(first, second);
    }

    /// REQ: LIF-176
    #[test]
    fn compile_cache_key_changes_with_wasmtime_version() {
        let first = compile_cache_key("hash-a", "21.0.0", "avx2");
        let second = compile_cache_key("hash-a", "22.0.0", "avx2");
        assert_ne!(first, second);
    }

    /// REQ: LIF-176
    #[test]
    fn compile_cache_key_changes_with_cpu_features() {
        let first = compile_cache_key("hash-a", "21.0.0", "avx2");
        let second = compile_cache_key("hash-a", "21.0.0", "sse4.2");
        assert_ne!(first, second);
    }

    #[test]
    fn compile_cache_key_is_deterministic_for_empty_inputs() {
        let first = compile_cache_key("", "", "");
        let second = compile_cache_key("", "", "");
        assert_eq!(first, second);
        assert_ne!(first, compile_cache_key("hash-a", "21.0.0", "avx2"));
    }

    #[test]
    fn effects_any_is_false_when_nothing_is_held() {
        assert!(!effects(false, false, false).any());
    }

    #[test]
    fn effects_any_ignores_the_instance_number() {
        let idle_first = Effects {
            instance: 0,
            input_lock: false,
            stream_open: false,
            child_running: false,
        };
        let idle_last = Effects {
            instance: u64::MAX,
            input_lock: false,
            stream_open: false,
            child_running: false,
        };
        assert!(!idle_first.any());
        assert!(!idle_last.any());
    }

    #[test]
    fn effects_any_is_true_when_only_the_input_lock_is_held() {
        assert!(effects(true, false, false).any());
    }

    #[test]
    fn effects_any_is_true_when_only_a_stream_is_open() {
        assert!(effects(false, true, false).any());
    }

    #[test]
    fn effects_any_is_true_when_only_a_child_process_runs() {
        assert!(effects(false, false, true).any());
    }

    #[test]
    fn effects_any_is_true_when_every_effect_is_held() {
        assert!(effects(true, true, true).any());
    }

    #[test]
    fn fresh_rig_has_no_virtual_time_prompts_lock_polls_or_network_attempts() {
        let rig = OpsRig::default();
        assert_eq!(rig.virtual_elapsed(), Duration::ZERO);
        assert_eq!(rig.prompt_count(), 0);
        assert_eq!(rig.lock_polls(), 0);
        assert_eq!(rig.network_attempts(), 0);
    }

    #[test]
    fn advance_moves_the_virtual_clock_by_the_requested_amount() {
        let rig = OpsRig::new();
        rig.advance(Duration::from_secs(5));
        assert_eq!(rig.virtual_elapsed(), Duration::from_secs(5));
    }

    #[test]
    fn advance_accumulates_across_calls() {
        let rig = OpsRig::new();
        rig.advance(Duration::from_secs(2));
        rig.advance(Duration::from_secs(3));
        assert_eq!(rig.virtual_elapsed(), Duration::from_secs(5));
    }

    #[test]
    fn advance_by_zero_keeps_the_clock_where_it_was() {
        let rig = OpsRig::new();
        rig.advance(Duration::ZERO);
        assert_eq!(rig.virtual_elapsed(), Duration::ZERO);
    }

    #[test]
    fn state_cache_and_plugins_directories_sit_inside_the_scratch_dir() {
        let rig = OpsRig::new();
        let scratch = rig.scratch_dir();
        assert!(rig.state_dir().starts_with(&scratch));
        assert!(rig.cache_dir().starts_with(&scratch));
        assert!(rig.plugins_root().starts_with(&scratch));
    }

    #[test]
    fn registry_bytes_return_what_was_written() {
        let rig = OpsRig::new();
        rig.set_registry_bytes(b"{}");
        assert_eq!(rig.registry_bytes(), b"{}".to_vec());
    }

    #[test]
    fn registry_bytes_accept_an_empty_file() {
        let rig = OpsRig::new();
        rig.set_registry_bytes(&[]);
        assert!(rig.registry_bytes().is_empty());
    }

    #[test]
    fn corrupt_file_overwrites_existing_bytes() {
        let rig = OpsRig::new();
        let path = rig.scratch_dir().join("garbage.db");
        std::fs::write(&path, b"old bytes").expect("seed file for corruption");
        rig.corrupt_file(&path, b"garbage");
        assert_eq!(
            std::fs::read(&path).expect("read corrupted file"),
            b"garbage".to_vec()
        );
    }

    /// REQ: LIF-172
    #[test]
    fn required_free_space_grows_with_the_package_size() {
        let rig = OpsRig::new();
        let small = PackageSpec::new(EXAMPLE_ID, "1.0.0").padding(0);
        let large = PackageSpec::new(EXAMPLE_ID, "1.0.0").padding(10 * 1024 * 1024);
        assert!(rig.required_free_bytes(&large) > rig.required_free_bytes(&small));
    }

    #[test]
    fn written_package_is_padded_to_at_least_the_requested_size() {
        let rig = OpsRig::new();
        let spec = PackageSpec::new(EXAMPLE_ID, "1.0.0").padding(4096);
        let path = rig.write_package(&spec);
        let length = std::fs::metadata(&path)
            .expect("stat written package")
            .len();
        assert!(length >= 4096);
    }

    #[test]
    fn server_reports_running_after_start_and_not_running_for_unknown_session() {
        let rig = OpsRig::new();
        let server = rig.start_server(SESSION);
        assert!(server.is_running());
        assert!(rig.server_running(SESSION));
        assert!(!rig.server_running("no-such-session"));
    }

    #[test]
    fn shutdown_stops_the_server() {
        let rig = OpsRig::new();
        let server = rig.start_server(SESSION);
        server.shutdown();
        assert!(!server.is_running());
    }

    #[test]
    fn crash_stops_the_server() {
        let rig = OpsRig::new();
        let server = rig.start_server(SESSION);
        server.crash();
        assert!(!server.is_running());
    }

    #[test]
    fn start_server_or_same_returns_the_running_server() {
        let rig = OpsRig::new();
        let server = rig.start_server(SESSION);
        let again = rig.start_server_or_same(&server);
        assert!(again.is_running());
    }

    /// REQ: LIF-170
    #[test]
    fn installed_plugin_runs_with_the_network_blocked() {
        let rig = OpsRig::new();
        rig.block_network();
        let server = installed_server(&rig, &PackageSpec::new(EXAMPLE_ID, "1.0.0"));
        server.apply_pending();
        assert!(server.active_version(EXAMPLE_ID).is_some());
        assert_eq!(rig.network_attempts(), 0);
    }

    /// REQ: LIF-171
    #[test]
    fn installing_a_local_package_makes_no_network_attempt() {
        let rig = OpsRig::new();
        rig.block_network();
        rig.install_bundled(&PackageSpec::new(EXAMPLE_ID, "1.0.0"));
        assert_eq!(rig.network_attempts(), 0);
    }

    /// REQ: LIF-190
    #[test]
    fn plugin_detail_view_has_five_tabs() {
        let rig = OpsRig::new();
        let server = installed_server(&rig, &PackageSpec::new(EXAMPLE_ID, "1.0.0"));
        let detail = server.render_plugin_detail(EXAMPLE_ID);
        assert_eq!(detail.tabs().len(), 5);
    }

    /// REQ: LIF-190
    #[test]
    fn plugins_screen_row_shows_state_and_health_of_installed_plugin() {
        let rig = OpsRig::new();
        let server = installed_server(&rig, &PackageSpec::new(EXAMPLE_ID, "1.0.0"));
        let screen = server.render_plugins_screen();
        let row = screen
            .row_for(EXAMPLE_ID)
            .expect("installed plugin has a row");
        assert!(!row.state.is_empty());
        assert!(!row.health.is_empty());
    }

    #[test]
    fn plugins_screen_has_no_row_for_unknown_plugin() {
        let rig = OpsRig::new();
        let server = rig.start_server(SESSION);
        let screen = server.render_plugins_screen();
        assert!(screen.row_for("no.such.plugin").is_none());
    }

    /// REQ: LIF-088
    #[test]
    fn disabling_twice_leaves_the_plugin_disabled() {
        let rig = OpsRig::new();
        let server = installed_server(&rig, &PackageSpec::new(EXAMPLE_ID, "1.0.0"));
        server.disable(EXAMPLE_ID);
        server.disable(EXAMPLE_ID);
        assert!(!server.is_enabled(EXAMPLE_ID));
    }

    /// REQ: LIF-161
    #[test]
    fn session_level_overrides_project_and_global_levels() {
        let rig = OpsRig::new();
        rig.install_bundled(&PackageSpec::new(EXAMPLE_ID, "1.0.0"));
        rig.set_enabled_levels(EXAMPLE_ID, SESSION, Some(true), Some(true), Some(false));
        let server = rig.start_server(SESSION);
        assert!(!server.is_enabled(EXAMPLE_ID));
    }

    /// REQ: LIF-161
    #[test]
    fn project_level_overrides_global_when_session_is_unset() {
        let rig = OpsRig::new();
        rig.install_bundled(&PackageSpec::new(EXAMPLE_ID, "1.0.0"));
        rig.set_enabled_levels(EXAMPLE_ID, SESSION, Some(true), Some(false), None);
        let server = rig.start_server(SESSION);
        assert!(!server.is_enabled(EXAMPLE_ID));
    }

    /// REQ: LIF-161
    #[test]
    fn global_level_applies_when_session_and_project_are_unset() {
        let rig = OpsRig::new();
        rig.install_bundled(&PackageSpec::new(EXAMPLE_ID, "1.0.0"));
        rig.set_enabled_levels(EXAMPLE_ID, SESSION, Some(false), None, None);
        let server = rig.start_server(SESSION);
        assert!(!server.is_enabled(EXAMPLE_ID));
    }

    /// REQ: LIF-162
    #[test]
    fn project_plugins_toml_request_does_not_install_the_plugin() {
        let rig = OpsRig::new();
        rig.write_project_plugins_toml("[[plugins]]\nid = \"example.requested\"\n");
        let server = rig.start_server(SESSION);
        assert_eq!(server.active_version("example.requested"), None);
    }

    /// REQ: LIF-163
    #[test]
    fn global_disable_reaches_every_running_session_within_two_seconds() {
        let rig = OpsRig::new();
        rig.install_bundled(&PackageSpec::new(EXAMPLE_ID, "1.0.0"));
        let first_session = rig.start_server("alpha");
        let second_session = rig.start_server("beta");
        rig.set_enabled_levels(EXAMPLE_ID, "alpha", Some(false), None, None);
        rig.advance(Duration::from_secs(2));
        assert!(!first_session.is_enabled(EXAMPLE_ID));
        assert!(!second_session.is_enabled(EXAMPLE_ID));
    }

    /// REQ: LIF-151
    #[test]
    fn pending_request_is_cancelled_once_at_each_of_the_first_four_events() {
        let events = [
            LifeEvent::PluginRestart,
            LifeEvent::DisableEnable,
            LifeEvent::Update,
            LifeEvent::ServerRestart,
        ];
        for event in events {
            let rig = OpsRig::new();
            let server = installed_server(&rig, &PackageSpec::new(EXAMPLE_ID, "1.0.0"));
            let pending = server.send_pending_request(EXAMPLE_ID);
            rig.apply_event(&server, EXAMPLE_ID, event);
            assert_eq!(
                pending.outcome(),
                Some("cancelled".to_string()),
                "event {:?}",
                event
            );
            assert_eq!(
                server.deliveries_of(pending.id()),
                1,
                "not replayed after {:?}",
                event
            );
        }
    }

    /// REQ: LIF-152
    #[test]
    fn restore_bytes_over_one_mebibyte_are_dropped() {
        let rig = OpsRig::new();
        let spec = PackageSpec::new(EXAMPLE_ID, "1.0.0").restore_bytes(ONE_MIB + 1);
        let server = installed_server(&rig, &spec);
        rig.apply_event(&server, EXAMPLE_ID, LifeEvent::PluginRestart);
        assert_eq!(server.last_restored_len(EXAMPLE_ID), None);
    }

    /// REQ: LIF-152
    #[test]
    fn restore_bytes_of_exactly_one_mebibyte_are_kept() {
        let rig = OpsRig::new();
        let spec = PackageSpec::new(EXAMPLE_ID, "1.0.0").restore_bytes(ONE_MIB);
        let server = installed_server(&rig, &spec);
        rig.apply_event(&server, EXAMPLE_ID, LifeEvent::PluginRestart);
        assert_eq!(server.last_restored_len(EXAMPLE_ID), Some(ONE_MIB));
    }

    /// REQ: LIF-153
    #[test]
    fn client_restore_bytes_are_last_writer_wins_per_plugin() {
        let rig = OpsRig::new();
        let server = installed_server(&rig, &PackageSpec::new(EXAMPLE_ID, "1.0.0"));
        let first_client = server.attach_client("first");
        first_client.detach_with_restore(EXAMPLE_ID, b"one");
        let second_client = server.attach_client("second");
        second_client.detach_with_restore(EXAMPLE_ID, b"two");
        let third_client = server.attach_client("third");
        assert_eq!(
            third_client.restored_bytes(EXAMPLE_ID),
            Some(b"two".to_vec())
        );
        assert_eq!(
            server.stored_client_restore(EXAMPLE_ID),
            Some(b"two".to_vec())
        );
    }

    /// REQ: LIF-155
    #[test]
    fn node_metadata_survives_plugin_and_server_restarts_until_node_removed() {
        let rig = OpsRig::new();
        let server = installed_server(&rig, &PackageSpec::new(EXAMPLE_ID, "1.0.0"));
        server.set_node_metadata(7, EXAMPLE_ID, "layout", b"wide");
        rig.apply_event(&server, EXAMPLE_ID, LifeEvent::PluginRestart);
        assert_eq!(
            server.node_metadata_as(EXAMPLE_ID, 7, EXAMPLE_ID, "layout"),
            Some(b"wide".to_vec())
        );
        rig.apply_event(&server, EXAMPLE_ID, LifeEvent::ServerRestart);
        assert_eq!(
            server.node_metadata_as(EXAMPLE_ID, 7, EXAMPLE_ID, "layout"),
            Some(b"wide".to_vec())
        );
        server.remove_node(7);
        assert_eq!(
            server.node_metadata_as(EXAMPLE_ID, 7, EXAMPLE_ID, "layout"),
            None
        );
    }

    /// REQ: LIF-157
    #[test]
    fn crash_keeps_storage_written_before_the_crash() {
        let rig = OpsRig::new();
        let server = installed_server(&rig, &PackageSpec::new(EXAMPLE_ID, "1.0.0"));
        server.write_storage(EXAMPLE_ID, "global", "greeting", b"hello");
        server.crash();
        let restarted = rig.start_server(SESSION);
        assert_eq!(
            restarted.read_storage(EXAMPLE_ID, "global", "greeting"),
            Some(b"hello".to_vec())
        );
    }

    /// REQ: LIF-175
    #[test]
    fn corrupt_storage_database_is_replaced_with_an_empty_one_not_deleted() {
        let rig = OpsRig::new();
        let server = installed_server(&rig, &PackageSpec::new(EXAMPLE_ID, "1.0.0"));
        server.write_storage(EXAMPLE_ID, "global", "greeting", b"hello");
        server.shutdown();
        rig.corrupt_file(&rig.storage_db_path(EXAMPLE_ID), b"not a database");
        let restarted = rig.start_server(SESSION);
        assert_eq!(
            restarted.read_storage(EXAMPLE_ID, "global", "greeting"),
            None
        );
        assert!(rig.storage_db_path(EXAMPLE_ID).exists());
    }

    /// REQ: LIF-173
    #[test]
    fn disk_full_write_is_refused_without_counting_as_a_plugin_failure() {
        let rig = OpsRig::new();
        let server = installed_server(&rig, &PackageSpec::new(EXAMPLE_ID, "1.0.0"));
        rig.fill_disk();
        let result = server.write_storage_checked(EXAMPLE_ID, "global", "key", b"value");
        assert!(result.is_err());
        assert_eq!(server.failure_count(EXAMPLE_ID, Side::Server), 0);
    }

    /// REQ: LIF-174
    #[test]
    fn read_only_data_folder_does_not_stop_the_server_or_the_plugin() {
        let rig = OpsRig::new();
        let server = installed_server(&rig, &PackageSpec::new(EXAMPLE_ID, "1.0.0"));
        rig.make_data_read_only(EXAMPLE_ID);
        server.apply_pending();
        assert!(server.is_running());
        assert!(server.active_version(EXAMPLE_ID).is_some());
    }

    /// REQ: LIF-191
    #[test]
    fn enable_clears_the_failure_window_of_a_failed_plugin() {
        let rig = OpsRig::new();
        let server = installed_server(&rig, &PackageSpec::new(EXAMPLE_ID, "1.0.0"));
        server.fail(EXAMPLE_ID, Side::Server);
        server.enable(EXAMPLE_ID);
        server.fail(EXAMPLE_ID, Side::Server);
        server.fail(EXAMPLE_ID, Side::Server);
        assert!(!server.is_disabled_by_failure(EXAMPLE_ID));
    }

    /// REQ: LIF-083
    #[test]
    fn uninstall_deletes_grants_so_reinstall_asks_again() {
        let rig = OpsRig::new();
        let spec = PackageSpec::new(EXAMPLE_ID, "1.0.0").grant("clipboard.read");
        let server = installed_server(&rig, &spec);
        server.apply_pending();
        rig.apply_event(&server, EXAMPLE_ID, LifeEvent::Uninstall);
        rig.install_bundled(&spec);
        assert!(server.granted(EXAMPLE_ID).is_empty());
    }

    /// REQ: LIF-086
    #[test]
    fn uninstall_leaves_the_data_of_other_plugins_untouched() {
        let rig = OpsRig::new();
        rig.install_bundled(&PackageSpec::new(EXAMPLE_ID, "1.0.0"));
        rig.install_bundled(&PackageSpec::new(OTHER_ID, "1.0.0"));
        let other_data = rig.plant_data(OTHER_ID, 1);
        let server = rig.start_server(SESSION);
        rig.apply_event(&server, EXAMPLE_ID, LifeEvent::Uninstall);
        assert!(other_data.exists());
    }

    #[test]
    fn dev_session_has_no_exit_code_until_it_ends() {
        let rig = OpsRig::new();
        let session = rig.dev_start(&PackageSpec::new(EXAMPLE_ID, "1.0.0"), &[]);
        assert_eq!(session.exit_code(), None);
        session.end();
        assert!(session.exit_code().is_some());
    }
}

//! Lifecycle-operations rig: drives whole install, update, session, persistence and dev-reload
//! flows in process with a virtual clock, a fake network and a fake disk (test plan lane
//! lifecycle-ops, LIF-060..199). Every item here is test support; nothing is implemented in the
//! skeleton state.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use ilium_plugin_api::generated::host::plugins::InstanceState;
use ilium_plugin_api::lifecycle::DeactivationReason;
use ilium_plugin_api::{PluginError, Side};

use crate::cli::CliOutput;

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
    _private: (),
}

impl LockHold {
    /// Kills the holder without releasing the lock cleanly.
    pub fn kill(&self) {
        unimplemented!("host::testing::ops::LockHold::kill")
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
    _private: (),
}

impl PluginsScreen {
    /// The row of a plugin.
    pub fn row_for(&self, _id: &str) -> Option<&PluginRow> {
        unimplemented!("host::testing::ops::PluginsScreen::row_for")
    }

    /// Every row in screen order.
    pub fn rows(&self) -> Vec<PluginRow> {
        unimplemented!("host::testing::ops::PluginsScreen::rows")
    }

    pub fn render_all(&self) {
        unimplemented!("PluginsScreen::render_all")
    }
}

/// The rendered detail screen of one plugin (LIF-190..195).
#[derive(Debug)]
pub struct PluginDetail {
    _private: (),
}

impl PluginDetail {
    /// The tab titles.
    pub fn tabs(&self) -> Vec<String> {
        unimplemented!("host::testing::ops::PluginDetail::tabs")
    }
}

/// A request a plugin sent and has not had answered yet (LIF-150..157).
#[derive(Debug)]
pub struct PendingRequest {
    _private: (),
}

impl PendingRequest {
    /// The outcome, once delivered.
    pub fn outcome(&self) -> Option<String> {
        unimplemented!("host::testing::ops::PendingRequest::outcome")
    }

    /// The request id.
    pub fn id(&self) -> u64 {
        unimplemented!("host::testing::ops::PendingRequest::id")
    }
}

/// A client attached to a rig server (LIF-150..157).
#[derive(Debug)]
pub struct ClientHandle {
    _private: (),
}

impl ClientHandle {
    /// Detaches, letting the plugin's client side hand back a restore blob.
    pub fn detach_with_restore(&self, _id: &str, _bytes: &[u8]) {
        unimplemented!("host::testing::ops::ClientHandle::detach_with_restore")
    }

    /// The bytes the plugin's client side was restored with.
    pub fn restored_bytes(&self, _id: &str) -> Option<Vec<u8>> {
        unimplemented!("host::testing::ops::ClientHandle::restored_bytes")
    }

    /// The deactivation reasons seen by the plugin's client side.
    pub fn deactivations(&self, _id: &str) -> Vec<DeactivationReason> {
        unimplemented!("host::testing::ops::ClientHandle::deactivations")
    }

    /// Test hook `ClientHandle::recovery_page_text` used by the gap tests.
    pub fn recovery_page_text(&self) -> String {
        unimplemented!("ClientHandle::recovery_page_text")
    }

    /// Test hook `ClientHandle::next_frame_text` used by the gap tests.
    pub fn next_frame_text(&self) -> String {
        unimplemented!("ClientHandle::next_frame_text")
    }

    /// Test hook `ClientHandle::block_delivery` used by the gap tests.
    pub fn block_delivery(&self) {
        unimplemented!("ClientHandle::block_delivery")
    }

    /// Test hook `ClientHandle::resume_delivery` used by the gap tests.
    pub fn resume_delivery(&self) {
        unimplemented!("ClientHandle::resume_delivery")
    }
}

/// A `plugin dev` session (DEV, LIF-160..169).
#[derive(Debug)]
pub struct DevSession {
    _private: (),
}

impl DevSession {
    /// The session the dev command attached to.
    pub fn attached_session(&self) -> String {
        unimplemented!("host::testing::ops::DevSession::attached_session")
    }

    /// The exit code, once ended.
    pub fn exit_code(&self) -> Option<i32> {
        unimplemented!("host::testing::ops::DevSession::exit_code")
    }

    /// Ends the session cleanly.
    pub fn end(&self) {
        unimplemented!("host::testing::ops::DevSession::end")
    }

    /// Kills the dev process without cleanup.
    pub fn kill_without_cleanup(&self) {
        unimplemented!("host::testing::ops::DevSession::kill_without_cleanup")
    }

    /// Closes the connection to the server.
    pub fn close_connection(&self) {
        unimplemented!("host::testing::ops::DevSession::close_connection")
    }

    /// Stops renewing the lease.
    pub fn pause_renewals(&self) {
        unimplemented!("host::testing::ops::DevSession::pause_renewals")
    }

    /// Lease renewals so far.
    pub fn lease_renewals(&self) -> u32 {
        unimplemented!("host::testing::ops::DevSession::lease_renewals")
    }

    /// Touches a source file.
    pub fn touch_source(&self) {
        unimplemented!("host::testing::ops::DevSession::touch_source")
    }

    /// Builds so far.
    pub fn build_count(&self) -> u32 {
        unimplemented!("host::testing::ops::DevSession::build_count")
    }

    /// Reloads with a new package.
    pub fn reload(&self, _spec: &PackageSpec) {
        unimplemented!("host::testing::ops::DevSession::reload")
    }

    /// Reloads with a build that fails.
    pub fn reload_failing_build(&self) {
        unimplemented!("host::testing::ops::DevSession::reload_failing_build")
    }

    /// The named steps of the last reload.
    pub fn reload_log(&self) -> Vec<String> {
        unimplemented!("host::testing::ops::DevSession::reload_log")
    }

    /// The JSONL output lines.
    pub fn output_lines(&self) -> Vec<serde_json::Value> {
        unimplemented!("host::testing::ops::DevSession::output_lines")
    }

    /// The prompts shown.
    pub fn prompts(&self) -> Vec<String> {
        unimplemented!("host::testing::ops::DevSession::prompts")
    }

    /// Answers every pending prompt.
    pub fn answer_prompts(&self, _answer: &str) {
        unimplemented!("host::testing::ops::DevSession::answer_prompts")
    }
}

/// An in-process server for one session (LIF-060..199).
#[derive(Debug)]
pub struct ServerHandle {
    _private: (),
}

impl ServerHandle {
    /// Applies pending install, update and uninstall work.
    pub fn apply_pending(&self) {
        unimplemented!("host::testing::ops::ServerHandle::apply_pending")
    }

    /// Pauses the next update at a stage.
    pub fn pause_at(&self, _stage: UpdateStage) {
        unimplemented!("host::testing::ops::ServerHandle::pause_at")
    }

    /// Crashes the server.
    pub fn crash(&self) {
        unimplemented!("host::testing::ops::ServerHandle::crash")
    }

    /// Shuts the server down cleanly.
    pub fn shutdown(&self) {
        unimplemented!("host::testing::ops::ServerHandle::shutdown")
    }

    /// Whether the server runs.
    pub fn is_running(&self) -> bool {
        unimplemented!("host::testing::ops::ServerHandle::is_running")
    }

    /// Drops the server socket.
    pub fn drop_socket(&self) {
        unimplemented!("host::testing::ops::ServerHandle::drop_socket")
    }

    /// Writes the session snapshot now.
    pub fn snapshot_now(&self) {
        unimplemented!("host::testing::ops::ServerHandle::snapshot_now")
    }

    /// The kernel calls made so far.
    pub fn kernel_call_log(&self) -> Vec<String> {
        unimplemented!("host::testing::ops::ServerHandle::kernel_call_log")
    }

    /// The log codes emitted so far.
    pub fn log_codes(&self) -> Vec<String> {
        unimplemented!("host::testing::ops::ServerHandle::log_codes")
    }

    /// The messages received by plugins.
    pub fn received_messages(&self) -> Vec<String> {
        unimplemented!("host::testing::ops::ServerHandle::received_messages")
    }

    /// The toasts shown.
    pub fn toasts(&self) -> Vec<String> {
        unimplemented!("host::testing::ops::ServerHandle::toasts")
    }

    /// The notices shown.
    pub fn notices(&self) -> Vec<String> {
        unimplemented!("host::testing::ops::ServerHandle::notices")
    }

    /// The active version of a plugin.
    pub fn active_version(&self, _id: &str) -> Option<String> {
        unimplemented!("host::testing::ops::ServerHandle::active_version")
    }

    /// The activations of a plugin, as `<side>:<scope>` texts.
    pub fn activations(&self, _id: &str) -> Vec<String> {
        unimplemented!("host::testing::ops::ServerHandle::activations")
    }

    /// The deactivation reasons of a plugin.
    pub fn deactivations(&self, _id: &str) -> Vec<DeactivationReason> {
        unimplemented!("host::testing::ops::ServerHandle::deactivations")
    }

    /// The instance state of one side of a plugin.
    pub fn state_of(&self, _id: &str, _side: Side) -> InstanceState {
        unimplemented!("host::testing::ops::ServerHandle::state_of")
    }

    pub fn advance_clock(&self, _by: Duration) {
        unimplemented!("ServerHandle::advance_clock")
    }

    pub fn placeholder_text(&self) -> String {
        unimplemented!("ServerHandle::placeholder_text")
    }

    /// Why a plugin is inactive.
    pub fn inactive_reason(&self, _id: &str) -> Option<String> {
        unimplemented!("host::testing::ops::ServerHandle::inactive_reason")
    }

    /// The refusal code of a plugin.
    pub fn refusal(&self, _id: &str) -> Option<String> {
        unimplemented!("host::testing::ops::ServerHandle::refusal")
    }

    /// Failures in the window of one side.
    pub fn failure_count(&self, _id: &str, _side: Side) -> u32 {
        unimplemented!("host::testing::ops::ServerHandle::failure_count")
    }

    /// The scheduled restart delay of one side.
    pub fn restart_delay_ms(&self, _id: &str, _side: Side) -> Option<u64> {
        unimplemented!("host::testing::ops::ServerHandle::restart_delay_ms")
    }

    /// Makes one side fail.
    pub fn fail(&self, _id: &str, _side: Side) {
        unimplemented!("host::testing::ops::ServerHandle::fail")
    }

    /// Makes the activation of a version fail.
    pub fn fail_activation_of_version(&self, _version: &str) {
        unimplemented!("host::testing::ops::ServerHandle::fail_activation_of_version")
    }

    /// Whether the failure window disabled the plugin.
    pub fn is_disabled_by_failure(&self, _id: &str) -> bool {
        unimplemented!("host::testing::ops::ServerHandle::is_disabled_by_failure")
    }

    /// Whether the plugin is enabled.
    pub fn is_enabled(&self, _id: &str) -> bool {
        unimplemented!("host::testing::ops::ServerHandle::is_enabled")
    }

    /// Disables the plugin.
    pub fn disable(&self, _id: &str) {
        unimplemented!("host::testing::ops::ServerHandle::disable")
    }

    /// Enables the plugin.
    pub fn enable(&self, _id: &str) {
        unimplemented!("host::testing::ops::ServerHandle::enable")
    }

    /// The virtual times at which a dependency was retried.
    pub fn dependency_retry_times(&self, _id: &str) -> Vec<Duration> {
        unimplemented!("host::testing::ops::ServerHandle::dependency_retry_times")
    }

    /// Calls an export of a plugin.
    pub fn call_plugin(&self, _id: &str, _export: &str) -> Result<(), PluginError> {
        unimplemented!("host::testing::ops::ServerHandle::call_plugin")
    }

    /// Calls a service method as a plugin.
    pub fn call_service(
        &self,
        _caller: &str,
        _service: &str,
        _method: &str,
    ) -> Result<(), PluginError> {
        unimplemented!("host::testing::ops::ServerHandle::call_service")
    }

    /// The grants of a plugin.
    pub fn granted(&self, _id: &str) -> Vec<String> {
        unimplemented!("host::testing::ops::ServerHandle::granted")
    }

    /// The length of the last restore blob delivered.
    pub fn last_restored_len(&self, _id: &str) -> Option<usize> {
        unimplemented!("host::testing::ops::ServerHandle::last_restored_len")
    }

    /// The stored client restore blob.
    pub fn stored_client_restore(&self, _id: &str) -> Option<Vec<u8>> {
        unimplemented!("host::testing::ops::ServerHandle::stored_client_restore")
    }

    /// Attaches a client.
    pub fn attach_client(&self, _name: &str) -> ClientHandle {
        unimplemented!("host::testing::ops::ServerHandle::attach_client")
    }

    /// Sets node metadata as a plugin.
    pub fn set_node_metadata(&self, _node: u64, _owner: &str, _key: &str, _bytes: &[u8]) {
        unimplemented!("host::testing::ops::ServerHandle::set_node_metadata")
    }

    /// Reads node metadata as another plugin.
    pub fn node_metadata_as(
        &self,
        _reader: &str,
        _node: u64,
        _owner: &str,
        _key: &str,
    ) -> Option<Vec<u8>> {
        unimplemented!("host::testing::ops::ServerHandle::node_metadata_as")
    }

    /// Removes a node.
    pub fn remove_node(&self, _node: u64) {
        unimplemented!("host::testing::ops::ServerHandle::remove_node")
    }

    /// Adds a decoration of a plugin to a node.
    pub fn add_decoration(&self, _id: &str, _node: u64) {
        unimplemented!("host::testing::ops::ServerHandle::add_decoration")
    }

    /// Decorations of a plugin.
    pub fn decorations(&self, _id: &str) -> usize {
        unimplemented!("host::testing::ops::ServerHandle::decorations")
    }

    /// Writes a storage value.
    pub fn write_storage(&self, _id: &str, _bucket: &str, _key: &str, _bytes: &[u8]) {
        unimplemented!("host::testing::ops::ServerHandle::write_storage")
    }

    /// Writes a storage value, returning the refusal code.
    pub fn write_storage_checked(
        &self,
        _id: &str,
        _bucket: &str,
        _key: &str,
        _bytes: &[u8],
    ) -> Result<(), String> {
        unimplemented!("host::testing::ops::ServerHandle::write_storage_checked")
    }

    /// Reads a storage value.
    pub fn read_storage(&self, _id: &str, _bucket: &str, _key: &str) -> Option<Vec<u8>> {
        unimplemented!("host::testing::ops::ServerHandle::read_storage")
    }

    /// Writes a setting.
    pub fn write_setting(&self, _id: &str, _key: &str, _value: &str) {
        unimplemented!("host::testing::ops::ServerHandle::write_setting")
    }

    /// Writes a data file.
    pub fn write_data_file(&self, _id: &str, _name: &str, _bytes: &[u8]) {
        unimplemented!("host::testing::ops::ServerHandle::write_data_file")
    }

    /// Writes a data file, returning the refusal code.
    pub fn write_data_file_checked(
        &self,
        _id: &str,
        _name: &str,
        _bytes: &[u8],
    ) -> Result<(), String> {
        unimplemented!("host::testing::ops::ServerHandle::write_data_file_checked")
    }

    /// Reads a data file.
    pub fn read_data_file(&self, _id: &str, _name: &str) -> Option<Vec<u8>> {
        unimplemented!("host::testing::ops::ServerHandle::read_data_file")
    }

    /// Sends a request from a plugin that stays pending.
    pub fn send_pending_request(&self, _id: &str) -> PendingRequest {
        unimplemented!("host::testing::ops::ServerHandle::send_pending_request")
    }

    /// How often a request was delivered.
    pub fn deliveries_of(&self, _request: u64) -> usize {
        unimplemented!("host::testing::ops::ServerHandle::deliveries_of")
    }

    /// Races a disable against an update; the named steps in order.
    pub fn race_disable_and_update(&self, _id: &str, _spec: &PackageSpec) -> Vec<String> {
        unimplemented!("host::testing::ops::ServerHandle::race_disable_and_update")
    }

    /// Races the disable of two plugins; the named steps in order.
    pub fn race_disable_of_two(&self, _first: &str, _second: &str) -> Vec<String> {
        unimplemented!("host::testing::ops::ServerHandle::race_disable_of_two")
    }

    /// Answers a first-use prompt.
    pub fn answer_first_use(&self, _id: &str, _grant: &str, _answer: &str) {
        unimplemented!("host::testing::ops::ServerHandle::answer_first_use")
    }

    /// Triggers the first use of a grant.
    pub fn trigger_first_use(&self, _id: &str, _grant: &str) {
        unimplemented!("host::testing::ops::ServerHandle::trigger_first_use")
    }

    /// How often "not now" was answered.
    pub fn not_now_count(&self, _id: &str, _grant: &str) -> u32 {
        unimplemented!("host::testing::ops::ServerHandle::not_now_count")
    }

    /// How often a grant was prompted.
    pub fn prompts_for(&self, _id: &str, _grant: &str) -> u32 {
        unimplemented!("host::testing::ops::ServerHandle::prompts_for")
    }

    /// Reloads the policy file.
    pub fn reload_policy(&self) {
        unimplemented!("host::testing::ops::ServerHandle::reload_policy")
    }

    /// Renders the plugins screen.
    pub fn render_plugins_screen(&self) -> PluginsScreen {
        unimplemented!("host::testing::ops::ServerHandle::render_plugins_screen")
    }

    /// Renders the detail screen of a plugin.
    pub fn render_plugin_detail(&self, _id: &str) -> PluginDetail {
        unimplemented!("host::testing::ops::ServerHandle::render_plugin_detail")
    }

    /// Presses space on the row of a plugin.
    pub fn press_space_on(&self, _id: &str) {
        unimplemented!("host::testing::ops::ServerHandle::press_space_on")
    }

    /// Presses Enable on the recovery page.
    pub fn press_recovery_enable(&self, _id: &str) {
        unimplemented!("host::testing::ops::ServerHandle::press_recovery_enable")
    }

    /// Presses Reset failures.
    pub fn press_reset_failures(&self, _id: &str) {
        unimplemented!("host::testing::ops::ServerHandle::press_reset_failures")
    }

    /// Makes an extension handler breach its budget.
    pub fn breach_extension(&self, _id: &str, _point: &str, _extension: &str) {
        unimplemented!("host::testing::ops::ServerHandle::breach_extension")
    }

    /// Whether an extension was stopped.
    pub fn extension_stopped(&self, _id: &str, _point: &str, _extension: &str) -> bool {
        unimplemented!("host::testing::ops::ServerHandle::extension_stopped")
    }

    /// Calls an extension.
    pub fn call_extension(
        &self,
        _id: &str,
        _point: &str,
        _extension: &str,
    ) -> Result<(), PluginError> {
        unimplemented!("host::testing::ops::ServerHandle::call_extension")
    }

    /// The extension events so far.
    pub fn extension_events(&self) -> Vec<String> {
        unimplemented!("host::testing::ops::ServerHandle::extension_events")
    }

    /// The contributions of a plugin.
    pub fn contributions(&self, _id: &str) -> Vec<String> {
        unimplemented!("host::testing::ops::ServerHandle::contributions")
    }

    /// The activation triggers of a plugin.
    pub fn triggers(&self, _id: &str) -> Vec<String> {
        unimplemented!("host::testing::ops::ServerHandle::triggers")
    }

    /// Whether a plugin replaces a core plugin.
    pub fn replaces(&self, _id: &str, _core: &str) -> bool {
        unimplemented!("host::testing::ops::ServerHandle::replaces")
    }

    /// Makes a plugin hold every effect.
    pub fn hold_effects(&self, _id: &str) -> Effects {
        unimplemented!("host::testing::ops::ServerHandle::hold_effects")
    }

    /// The effects held by a plugin.
    pub fn effects_of(&self, _id: &str) -> Effects {
        unimplemented!("host::testing::ops::ServerHandle::effects_of")
    }

    /// The effects held by an instance.
    pub fn effects_of_instance(&self, _instance: u64) -> Effects {
        unimplemented!("host::testing::ops::ServerHandle::effects_of_instance")
    }

    /// Whether a dev package of a plugin is loaded.
    pub fn has_dev_package(&self, _id: &str) -> bool {
        unimplemented!("host::testing::ops::ServerHandle::has_dev_package")
    }

    /// Test hook `ServerHandle::call_on_behalf_of` used by the gap tests.
    pub fn call_on_behalf_of(
        &self,
        _origin: &str,
        _target: &str,
        _export: &str,
    ) -> Result<(), PluginError> {
        unimplemented!("ServerHandle::call_on_behalf_of")
    }

    /// Test hook `ServerHandle::socket_answers` used by the gap tests.
    pub fn socket_answers(&self) -> bool {
        unimplemented!("ServerHandle::socket_answers")
    }

    /// Test hook `ServerHandle::plugin_status` used by the gap tests.
    pub fn plugin_status(&self, _id: &str) -> String {
        unimplemented!("ServerHandle::plugin_status")
    }

    /// Test hook `ServerHandle::audit_records` used by the gap tests.
    pub fn audit_records(&self, _id: &str) -> Vec<serde_json::Value> {
        unimplemented!("ServerHandle::audit_records")
    }

    /// Test hook `ServerHandle::render_status_surface` used by the gap tests.
    pub fn render_status_surface(&self) -> String {
        unimplemented!("ServerHandle::render_status_surface")
    }

    /// Test hook `ServerHandle::kill_sigkill` used by the gap tests.
    pub fn kill_sigkill(&self) {
        unimplemented!("ServerHandle::kill_sigkill")
    }

    /// Test hook `ServerHandle::owner_pid` used by the gap tests.
    pub fn owner_pid(&self) -> u32 {
        unimplemented!("ServerHandle::owner_pid")
    }

    /// Test hook `ServerHandle::features_served_by` used by the gap tests.
    pub fn features_served_by(&self, _wave: &str) -> String {
        unimplemented!("ServerHandle::features_served_by")
    }

    /// Test hook `ServerHandle::send_sigabrt_to_self` used by the gap tests.
    pub fn send_sigabrt_to_self(&self) {
        unimplemented!("ServerHandle::send_sigabrt_to_self")
    }

    /// Test hook `ServerHandle::start_voice_capture` used by the gap tests.
    pub fn start_voice_capture(&self, _tone: &str) -> CaptureHandle {
        unimplemented!("ServerHandle::start_voice_capture")
    }

    /// Test hook `ServerHandle::confirm_retention` used by the gap tests.
    pub fn confirm_retention(&self, _provider: &str) {
        unimplemented!("ServerHandle::confirm_retention")
    }

    /// Test hook `ServerHandle::render_settings_text` used by the gap tests.
    pub fn render_settings_text(&self) -> String {
        unimplemented!("ServerHandle::render_settings_text")
    }

    /// Test hook `ServerHandle::snapshot_time` used by the gap tests.
    pub fn snapshot_time(&self, _session: &str) -> u64 {
        unimplemented!("ServerHandle::snapshot_time")
    }

    /// Test hook `ServerHandle::read_setting_value` used by the gap tests.
    pub fn read_setting_value(&self, _reader: &str, _key: &str) -> Option<String> {
        unimplemented!("ServerHandle::read_setting_value")
    }

    /// Test hook `ServerHandle::write_setting_checked` used by the gap tests.
    pub fn write_setting_checked(
        &self,
        _id: &str,
        _key: &str,
        _value: &str,
    ) -> Result<(), PluginError> {
        unimplemented!("ServerHandle::write_setting_checked")
    }

    /// Test hook `ServerHandle::recovery_page_open` used by the gap tests.
    pub fn recovery_page_open(&self) -> bool {
        unimplemented!("ServerHandle::recovery_page_open")
    }

    /// Test hook `ServerHandle::tree_nodes` used by the gap tests.
    pub fn tree_nodes(&self) -> Vec<(u64, String, Option<u64>, usize)> {
        unimplemented!("ServerHandle::tree_nodes")
    }

    /// Test hook `ServerHandle::pane_count` used by the gap tests.
    pub fn pane_count(&self) -> usize {
        unimplemented!("ServerHandle::pane_count")
    }

    /// Test hook `ServerHandle::pty_restarts` used by the gap tests.
    pub fn pty_restarts(&self) -> u32 {
        unimplemented!("ServerHandle::pty_restarts")
    }

    /// Test hook `ServerHandle::open_pane` used by the gap tests.
    pub fn open_pane(&self, _name: &str) -> u64 {
        unimplemented!("ServerHandle::open_pane")
    }

    /// Test hook `ServerHandle::current_layout` used by the gap tests.
    pub fn current_layout(&self) -> serde_json::Value {
        unimplemented!("ServerHandle::current_layout")
    }

    /// Test hook `ServerHandle::propose_layout` used by the gap tests.
    pub fn propose_layout(
        &self,
        _layout_id: &str,
        _proposal: &serde_json::Value,
    ) -> Result<(), PluginError> {
        unimplemented!("ServerHandle::propose_layout")
    }

    /// Test hook `ServerHandle::type_into_focused_pane` used by the gap tests.
    pub fn type_into_focused_pane(&self, _text: &str) {
        unimplemented!("ServerHandle::type_into_focused_pane")
    }

    /// Test hook `ServerHandle::focused_pane_id` used by the gap tests.
    pub fn focused_pane_id(&self) -> u64 {
        unimplemented!("ServerHandle::focused_pane_id")
    }

    /// Test hook `ServerHandle::pane_received_input` used by the gap tests.
    pub fn pane_received_input(&self, _pane: u64) -> String {
        unimplemented!("ServerHandle::pane_received_input")
    }

    /// Test hook `ServerHandle::pty_size` used by the gap tests.
    pub fn pty_size(&self, _pane: u64) -> (u16, u16) {
        unimplemented!("ServerHandle::pty_size")
    }

    /// Test hook `ServerHandle::pty_running` used by the gap tests.
    pub fn pty_running(&self, _pane: u64) -> bool {
        unimplemented!("ServerHandle::pty_running")
    }

    /// Test hook `ServerHandle::pty_detached` used by the gap tests.
    pub fn pty_detached(&self, _pane: u64) -> bool {
        unimplemented!("ServerHandle::pty_detached")
    }

    /// Test hook `ServerHandle::pane_class` used by the gap tests.
    pub fn pane_class(&self, _pane: u64) -> String {
        unimplemented!("ServerHandle::pane_class")
    }

    /// Test hook `ServerHandle::pane_activity` used by the gap tests.
    pub fn pane_activity(&self, _pane: u64) -> String {
        unimplemented!("ServerHandle::pane_activity")
    }

    /// Test hook `ServerHandle::parity_result` used by the gap tests.
    pub fn parity_result(&self, _id: &str) -> Option<String> {
        unimplemented!("ServerHandle::parity_result")
    }

    /// Test hook `ServerHandle::active_owner` used by the gap tests.
    pub fn active_owner(&self, _core_id: &str) -> String {
        unimplemented!("ServerHandle::active_owner")
    }

    /// Test hook `ServerHandle::emitted_events` used by the gap tests.
    pub fn emitted_events(&self, _name: &str) -> usize {
        unimplemented!("ServerHandle::emitted_events")
    }

    /// Test hook `ServerHandle::deactivation_order` used by the gap tests.
    pub fn deactivation_order(&self) -> Vec<String> {
        unimplemented!("ServerHandle::deactivation_order")
    }

    /// Test hook `ServerHandle::activation_time` used by the gap tests.
    pub fn activation_time(&self, _id: &str) -> Duration {
        unimplemented!("ServerHandle::activation_time")
    }

    /// Test hook `ServerHandle::trigger_deliveries` used by the gap tests.
    pub fn trigger_deliveries(&self, _id: &str) -> usize {
        unimplemented!("ServerHandle::trigger_deliveries")
    }

    /// Test hook `ServerHandle::read_service_snapshot` used by the gap tests.
    pub fn read_service_snapshot(&self, _reader: &str, _id: &str, _key: &str) -> Option<Vec<u8>> {
        unimplemented!("ServerHandle::read_service_snapshot")
    }

    /// Test hook `ServerHandle::kernel_fallback_active` used by the gap tests.
    pub fn kernel_fallback_active(&self, _core_id: &str) -> bool {
        unimplemented!("ServerHandle::kernel_fallback_active")
    }

    /// Test hook `ServerHandle::log_lines_with_code` used by the gap tests.
    pub fn log_lines_with_code(&self, _code: &str) -> Vec<serde_json::Value> {
        unimplemented!("ServerHandle::log_lines_with_code")
    }

    /// Test hook `ServerHandle::render_recovery_page` used by the gap tests.
    pub fn render_recovery_page(&self) -> String {
        unimplemented!("ServerHandle::render_recovery_page")
    }

    /// Test hook `ServerHandle::offered_updates` used by the gap tests.
    pub fn offered_updates(&self, _id: &str) -> Vec<String> {
        unimplemented!("ServerHandle::offered_updates")
    }

    /// Test hook `ServerHandle::revoke_via_capabilities_tab` used by the gap tests.
    pub fn revoke_via_capabilities_tab(&self, _id: &str, _grant: &str) -> RevokeOutcome {
        unimplemented!("ServerHandle::revoke_via_capabilities_tab")
    }

    /// Test hook `ServerHandle::open_details_page` used by the gap tests.
    pub fn open_details_page(&self, _id: &str) -> DetailsPage {
        unimplemented!("ServerHandle::open_details_page")
    }

    /// Test hook `ServerHandle::open_plugin_window` used by the gap tests.
    pub fn open_plugin_window(&self, _id: &str) -> PluginWindow {
        unimplemented!("ServerHandle::open_plugin_window")
    }

    /// Test hook `ServerHandle::set_view_delay` used by the gap tests.
    pub fn set_view_delay(&self, _id: &str, _delay: Duration) {
        unimplemented!("ServerHandle::set_view_delay")
    }

    /// Test hook `ServerHandle::plugin_host_call` used by the gap tests.
    pub fn plugin_host_call(
        &self,
        _caller: &str,
        _export: &str,
        _args: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        unimplemented!("ServerHandle::plugin_host_call")
    }

    /// Test hook `ServerHandle::banner_text` used by the gap tests.
    pub fn banner_text(&self) -> String {
        unimplemented!("ServerHandle::banner_text")
    }
}

/// Temp data, state, config and cache directories with a virtual clock, fake network and fake disk.
#[derive(Debug)]
pub struct OpsRig {
    _private: (),
}

impl OpsRig {
    /// A fresh rig.
    pub fn new() -> OpsRig {
        unimplemented!("host::testing::ops::OpsRig::new")
    }

    /// Runs the CLI with stdin closed and no TTY.
    pub fn cli(&self, _args: &[&str]) -> CliOutput {
        unimplemented!("host::testing::ops::OpsRig::cli")
    }

    /// Runs the CLI on a TTY with scripted prompt answers.
    pub fn cli_tty(&self, _args: &[&str], _answers: &[&str]) -> CliOutput {
        unimplemented!("host::testing::ops::OpsRig::cli_tty")
    }

    /// Runs the `plugin dev` entry and returns when it exits.
    pub fn dev_cli(&self, _args: &[&str]) -> CliOutput {
        unimplemented!("host::testing::ops::OpsRig::dev_cli")
    }

    /// Writes a package file.
    pub fn write_package(&self, _spec: &PackageSpec) -> PathBuf {
        unimplemented!("host::testing::ops::OpsRig::write_package")
    }

    /// The scratch directory.
    pub fn scratch_dir(&self) -> PathBuf {
        unimplemented!("host::testing::ops::OpsRig::scratch_dir")
    }

    /// The plugins root.
    pub fn plugins_root(&self) -> PathBuf {
        unimplemented!("host::testing::ops::OpsRig::plugins_root")
    }

    /// The state directory.
    pub fn state_dir(&self) -> PathBuf {
        unimplemented!("host::testing::ops::OpsRig::state_dir")
    }

    /// The cache directory.
    pub fn cache_dir(&self) -> PathBuf {
        unimplemented!("host::testing::ops::OpsRig::cache_dir")
    }

    /// The data directory of a plugin.
    pub fn data_dir(&self, _id: &str) -> PathBuf {
        unimplemented!("host::testing::ops::OpsRig::data_dir")
    }

    /// The compiled-code cache directory.
    pub fn compile_cache_dir(&self) -> PathBuf {
        unimplemented!("host::testing::ops::OpsRig::compile_cache_dir")
    }

    /// The bytes of `installed.json`.
    pub fn registry_bytes(&self) -> Vec<u8> {
        unimplemented!("host::testing::ops::OpsRig::registry_bytes")
    }

    /// `installed.json` parsed.
    pub fn registry_json(&self) -> serde_json::Value {
        unimplemented!("host::testing::ops::OpsRig::registry_json")
    }

    /// Overwrites `installed.json`.
    pub fn set_registry_bytes(&self, _bytes: &[u8]) {
        unimplemented!("host::testing::ops::OpsRig::set_registry_bytes")
    }

    /// `grants.json` parsed.
    pub fn grants_json(&self) -> serde_json::Value {
        unimplemented!("host::testing::ops::OpsRig::grants_json")
    }

    /// Injects a fault.
    pub fn inject(&self, _fault: Fault) {
        unimplemented!("host::testing::ops::OpsRig::inject")
    }

    /// The named steps so far.
    pub fn trace(&self) -> Vec<String> {
        unimplemented!("host::testing::ops::OpsRig::trace")
    }

    /// Holds the plugins-root lock from a simulated other process.
    pub fn hold_lock(&self) -> LockHold {
        unimplemented!("host::testing::ops::OpsRig::hold_lock")
    }

    /// Holds the lock for a virtual duration.
    pub fn hold_lock_for(&self, _duration: Duration) -> LockHold {
        unimplemented!("host::testing::ops::OpsRig::hold_lock_for")
    }

    /// Lock polls made so far.
    pub fn lock_polls(&self) -> u32 {
        unimplemented!("host::testing::ops::OpsRig::lock_polls")
    }

    /// Virtual time elapsed.
    pub fn virtual_elapsed(&self) -> Duration {
        unimplemented!("host::testing::ops::OpsRig::virtual_elapsed")
    }

    /// Moves the clock and runs due work.
    pub fn advance(&self, _by: Duration) {
        unimplemented!("host::testing::ops::OpsRig::advance")
    }

    pub fn advance_clock(&self, _by: Duration) {
        unimplemented!("OpsRig::advance_clock")
    }

    pub fn prompts_for(&self, _id: &str, _grant: &str) -> u32 {
        unimplemented!("OpsRig::prompts_for")
    }

    pub fn state_of(&self, _id: &str, _side: Side) -> String {
        unimplemented!("OpsRig::state_of")
    }

    /// Starts a server that applies pending work at once.
    pub fn start_server(&self, _session: &str) -> ServerHandle {
        unimplemented!("host::testing::ops::OpsRig::start_server")
    }

    /// Starts a server that applies pending work on `apply_pending`.
    pub fn start_server_manual(&self, _session: &str) -> ServerHandle {
        unimplemented!("host::testing::ops::OpsRig::start_server_manual")
    }

    /// Starts a server without a file watcher.
    pub fn start_server_polling(&self, _session: &str) -> ServerHandle {
        unimplemented!("host::testing::ops::OpsRig::start_server_polling")
    }

    /// Starts the same session again, or returns the running server.
    pub fn start_server_or_same(&self, _server: &ServerHandle) -> ServerHandle {
        unimplemented!("host::testing::ops::OpsRig::start_server_or_same")
    }

    /// Whether a session server runs.
    pub fn server_running(&self, _session: &str) -> bool {
        unimplemented!("host::testing::ops::OpsRig::server_running")
    }

    /// Prompts shown so far.
    pub fn prompt_count(&self) -> usize {
        unimplemented!("host::testing::ops::OpsRig::prompt_count")
    }

    /// The lines of the last review dialog.
    pub fn last_review(&self) -> Vec<String> {
        unimplemented!("host::testing::ops::OpsRig::last_review")
    }

    /// Blocks the network.
    pub fn block_network(&self) {
        unimplemented!("host::testing::ops::OpsRig::block_network")
    }

    /// Network attempts so far.
    pub fn network_attempts(&self) -> u32 {
        unimplemented!("host::testing::ops::OpsRig::network_attempts")
    }

    /// Sets the free disk space.
    pub fn set_free_bytes(&self, _bytes: u64) {
        unimplemented!("host::testing::ops::OpsRig::set_free_bytes")
    }

    /// The free space an install of the spec needs.
    pub fn required_free_bytes(&self, _spec: &PackageSpec) -> u64 {
        unimplemented!("host::testing::ops::OpsRig::required_free_bytes")
    }

    /// Fills the disk.
    pub fn fill_disk(&self) {
        unimplemented!("host::testing::ops::OpsRig::fill_disk")
    }

    /// Frees the disk.
    pub fn free_disk(&self) {
        unimplemented!("host::testing::ops::OpsRig::free_disk")
    }

    /// Makes the data directory of a plugin read-only.
    pub fn make_data_read_only(&self, _id: &str) {
        unimplemented!("host::testing::ops::OpsRig::make_data_read_only")
    }

    /// Overwrites a file with the given bytes.
    pub fn corrupt_file(&self, _path: &Path, _bytes: &[u8]) {
        unimplemented!("host::testing::ops::OpsRig::corrupt_file")
    }

    /// The storage database path of a plugin.
    pub fn storage_db_path(&self, _id: &str) -> PathBuf {
        unimplemented!("host::testing::ops::OpsRig::storage_db_path")
    }

    /// Deletes the compiled-code cache.
    pub fn delete_compile_cache(&self) {
        unimplemented!("host::testing::ops::OpsRig::delete_compile_cache")
    }

    /// Makes the compiled-code cache unwritable.
    pub fn make_compile_cache_unwritable(&self) {
        unimplemented!("host::testing::ops::OpsRig::make_compile_cache_unwritable")
    }

    /// Plants a data directory with a stamp.
    pub fn plant_data(&self, _id: &str, _stamp: u32) -> PathBuf {
        unimplemented!("host::testing::ops::OpsRig::plant_data")
    }

    /// Plants a setting.
    pub fn plant_settings(&self, _id: &str, _key: &str, _value: &str) {
        unimplemented!("host::testing::ops::OpsRig::plant_settings")
    }

    /// Plants a backup.
    pub fn plant_backup(&self, _id: &str, _name: &str) {
        unimplemented!("host::testing::ops::OpsRig::plant_backup")
    }

    /// Plants a cache folder.
    pub fn plant_cache(&self, _id: &str) {
        unimplemented!("host::testing::ops::OpsRig::plant_cache")
    }

    /// Plants a shared bucket.
    pub fn plant_shared_bucket(&self, _id: &str) {
        unimplemented!("host::testing::ops::OpsRig::plant_shared_bucket")
    }

    /// Plants a first-use answer.
    pub fn plant_first_use_answer(&self, _id: &str, _grant: &str) {
        unimplemented!("host::testing::ops::OpsRig::plant_first_use_answer")
    }

    /// Plants a durable timer.
    pub fn plant_durable_timer(&self, _id: &str, _due_unix_ms: u64) {
        unimplemented!("host::testing::ops::OpsRig::plant_durable_timer")
    }

    /// Plants node metadata in a session snapshot.
    pub fn plant_snapshot_metadata(
        &self,
        _session: &str,
        _node: u64,
        _id: &str,
        _key: &str,
        _bytes: &[u8],
    ) {
        unimplemented!("host::testing::ops::OpsRig::plant_snapshot_metadata")
    }

    /// Makes a staging directory of the given owner pid and age.
    pub fn make_staging(&self, _name: &str, _pid: u32, _age: Duration) -> PathBuf {
        unimplemented!("host::testing::ops::OpsRig::make_staging")
    }

    /// Makes an orphan store entry of the given age.
    pub fn make_store_orphan(&self, _id: &str, _version: &str, _age: Duration) -> PathBuf {
        unimplemented!("host::testing::ops::OpsRig::make_store_orphan")
    }

    /// Ages the store entries of a plugin.
    pub fn age_store_entries(&self, _id: &str, _age: Duration) {
        unimplemented!("host::testing::ops::OpsRig::age_store_entries")
    }

    /// Truncates the first file of a store entry.
    pub fn truncate_first_store_file(&self, _id: &str, _version: &str) {
        unimplemented!("host::testing::ops::OpsRig::truncate_first_store_file")
    }

    /// Deletes a cached package.
    pub fn delete_cached_package(&self, _id: &str, _version: &str) {
        unimplemented!("host::testing::ops::OpsRig::delete_cached_package")
    }

    /// Deletes the backups of a plugin.
    pub fn delete_backups(&self, _id: &str) {
        unimplemented!("host::testing::ops::OpsRig::delete_backups")
    }

    /// The data-version stamp of a plugin's bucket.
    pub fn bucket_stamp(&self, _id: &str) -> Option<u32> {
        unimplemented!("host::testing::ops::OpsRig::bucket_stamp")
    }

    /// A stored setting.
    pub fn setting(&self, _id: &str, _key: &str) -> Option<String> {
        unimplemented!("host::testing::ops::OpsRig::setting")
    }

    /// Whether the shared bucket of a plugin exists.
    pub fn shared_bucket_exists(&self, _id: &str) -> bool {
        unimplemented!("host::testing::ops::OpsRig::shared_bucket_exists")
    }

    /// The first-use answers of a plugin.
    pub fn first_use_answers(&self, _id: &str) -> Vec<String> {
        unimplemented!("host::testing::ops::OpsRig::first_use_answers")
    }

    /// The durable timers of a plugin.
    pub fn durable_timers(&self, _id: &str) -> Vec<String> {
        unimplemented!("host::testing::ops::OpsRig::durable_timers")
    }

    /// The node metadata keys of a plugin in a session snapshot.
    pub fn snapshot_metadata_keys(&self, _session: &str, _id: &str) -> Vec<String> {
        unimplemented!("host::testing::ops::OpsRig::snapshot_metadata_keys")
    }

    /// The bytes of a session snapshot.
    pub fn snapshot_bytes(&self, _session: &str) -> Vec<u8> {
        unimplemented!("host::testing::ops::OpsRig::snapshot_bytes")
    }

    /// Makes the snapshot directory of a session read-only.
    pub fn make_snapshot_dir_read_only(&self, _session: &str) {
        unimplemented!("host::testing::ops::OpsRig::make_snapshot_dir_read_only")
    }

    /// Makes the snapshot directory of a session writable.
    pub fn make_snapshot_dir_writable(&self, _session: &str) {
        unimplemented!("host::testing::ops::OpsRig::make_snapshot_dir_writable")
    }

    /// Trusts a publisher key.
    pub fn trust_publisher(&self, _key_id: &str) {
        unimplemented!("host::testing::ops::OpsRig::trust_publisher")
    }

    /// Revokes a key.
    pub fn revoke_key(&self, _key_id: &str, _reason: &str) {
        unimplemented!("host::testing::ops::OpsRig::revoke_key")
    }

    /// Takes a version down from the registry.
    pub fn registry_takedown(&self, _id: &str, _version: &str) {
        unimplemented!("host::testing::ops::OpsRig::registry_takedown")
    }

    /// Writes the policy file.
    pub fn write_policy(&self, _toml: &str) {
        unimplemented!("host::testing::ops::OpsRig::write_policy")
    }

    /// Writes the project `plugins.toml`.
    pub fn write_project_plugins_toml(&self, _text: &str) {
        unimplemented!("host::testing::ops::OpsRig::write_project_plugins_toml")
    }

    /// Sets the user, project and session enable levels of a plugin.
    pub fn set_enabled_levels(
        &self,
        _id: &str,
        _session: &str,
        _user: Option<bool>,
        _project: Option<bool>,
        _session_level: Option<bool>,
    ) {
        unimplemented!("host::testing::ops::OpsRig::set_enabled_levels")
    }

    /// Sets every keep-and-lose marker of a plugin.
    pub fn set_all_markers(&self, _id: &str, _server: &ServerHandle) {
        unimplemented!("host::testing::ops::OpsRig::set_all_markers")
    }

    /// Whether a marker is still present.
    pub fn marker_present(&self, _id: &str, _marker: Marker) -> bool {
        unimplemented!("host::testing::ops::OpsRig::marker_present")
    }

    /// Applies a lifecycle event.
    pub fn apply_event(&self, _server: &ServerHandle, _id: &str, _event: LifeEvent) {
        unimplemented!("host::testing::ops::OpsRig::apply_event")
    }

    /// Installs a replacement of a core plugin.
    pub fn install_replacement_of_core(&self, _spec: &PackageSpec, _core_id: &str) {
        unimplemented!("host::testing::ops::OpsRig::install_replacement_of_core")
    }

    /// Installs a bundled plugin.
    pub fn install_bundled(&self, _spec: &PackageSpec) {
        unimplemented!("host::testing::ops::OpsRig::install_bundled")
    }

    /// Updates a bundled plugin.
    pub fn update_bundled(&self, _spec: &PackageSpec) {
        unimplemented!("host::testing::ops::OpsRig::update_bundled")
    }

    /// Starts a `plugin dev` session.
    pub fn dev_start(&self, _spec: &PackageSpec, _args: &[&str]) -> DevSession {
        unimplemented!("host::testing::ops::OpsRig::dev_start")
    }

    /// The plugins screen as the user sees it, with one row per installed plugin.
    pub fn render_plugins_screen(&self) -> PluginsScreen {
        unimplemented!("host::testing::ops::OpsRig::render_plugins_screen")
    }

    /// Writes a value into a plugin's storage bucket.
    pub fn write_storage(&self, _id: &str, _bucket: &str, _key: &str, _value: &[u8]) {
        unimplemented!("host::testing::ops::OpsRig::write_storage")
    }

    /// Reads a value from a plugin's storage bucket.
    pub fn read_storage(&self, _id: &str, _bucket: &str, _key: &str) -> Option<Vec<u8>> {
        unimplemented!("host::testing::ops::OpsRig::read_storage")
    }

    /// Writes a plugin setting as the user would.
    pub fn write_setting(&self, _id: &str, _key: &str, _value: &str) {
        unimplemented!("host::testing::ops::OpsRig::write_setting")
    }

    /// Reloads the machine policy file into the running host.
    pub fn reload_policy(&self) {
        unimplemented!("host::testing::ops::OpsRig::reload_policy")
    }

    /// Test hook `OpsRig::audit_records` used by the gap tests.
    pub fn audit_records(&self, _event: &str) -> Vec<serde_json::Value> {
        unimplemented!("OpsRig::audit_records")
    }

    /// Test hook `OpsRig::audit_log_lines` used by the gap tests.
    pub fn audit_log_lines(&self) -> Vec<serde_json::Value> {
        unimplemented!("OpsRig::audit_log_lines")
    }

    /// Test hook `OpsRig::audit_last_hash` used by the gap tests.
    pub fn audit_last_hash(&self) -> String {
        unimplemented!("OpsRig::audit_last_hash")
    }

    /// Test hook `OpsRig::rotate_audit_log` used by the gap tests.
    pub fn rotate_audit_log(&self) {
        unimplemented!("OpsRig::rotate_audit_log")
    }

    /// Test hook `OpsRig::corrupt_audit_record` used by the gap tests.
    pub fn corrupt_audit_record(&self, _index: usize) {
        unimplemented!("OpsRig::corrupt_audit_record")
    }

    /// Test hook `OpsRig::compile_count` used by the gap tests.
    pub fn compile_count(&self, _id: &str) -> usize {
        unimplemented!("OpsRig::compile_count")
    }

    /// Test hook `OpsRig::compile_ran_in_helper` used by the gap tests.
    pub fn compile_ran_in_helper(&self, _id: &str) -> bool {
        unimplemented!("OpsRig::compile_ran_in_helper")
    }

    /// Test hook `OpsRig::corrupt_artifact_digest` used by the gap tests.
    pub fn corrupt_artifact_digest(&self, _id: &str) {
        unimplemented!("OpsRig::corrupt_artifact_digest")
    }

    /// Test hook `OpsRig::helper_rlimits` used by the gap tests.
    pub fn helper_rlimits(&self, _id: &str) -> serde_json::Value {
        unimplemented!("OpsRig::helper_rlimits")
    }

    /// Test hook `OpsRig::granted_storage_quota` used by the gap tests.
    pub fn granted_storage_quota(&self, _id: &str) -> u64 {
        unimplemented!("OpsRig::granted_storage_quota")
    }

    /// Test hook `OpsRig::exhaust_fuel` used by the gap tests.
    pub fn exhaust_fuel(&self, _id: &str) {
        unimplemented!("OpsRig::exhaust_fuel")
    }

    /// Test hook `OpsRig::guest_clock_resolution_nanos` used by the gap tests.
    pub fn guest_clock_resolution_nanos(&self, _id: &str) -> u64 {
        unimplemented!("OpsRig::guest_clock_resolution_nanos")
    }

    /// Test hook `OpsRig::exposed_imports` used by the gap tests.
    pub fn exposed_imports(&self, _id: &str) -> Vec<String> {
        unimplemented!("OpsRig::exposed_imports")
    }

    /// Test hook `OpsRig::fail_landlock_ruleset` used by the gap tests.
    pub fn fail_landlock_ruleset(&self) {
        unimplemented!("OpsRig::fail_landlock_ruleset")
    }

    /// Test hook `OpsRig::tamper_trust_store_between_reads` used by the gap tests.
    pub fn tamper_trust_store_between_reads(&self) {
        unimplemented!("OpsRig::tamper_trust_store_between_reads")
    }

    /// Test hook `OpsRig::tampered_copies` used by the gap tests.
    pub fn tampered_copies(&self) -> Vec<std::path::PathBuf> {
        unimplemented!("OpsRig::tampered_copies")
    }

    /// Test hook `OpsRig::registry_notices_sent` used by the gap tests.
    pub fn registry_notices_sent(&self, _publisher: &str) -> usize {
        unimplemented!("OpsRig::registry_notices_sent")
    }

    /// Test hook `OpsRig::serve_registry_plugin` used by the gap tests.
    pub fn serve_registry_plugin(&self, _registry: &str, _id: &str, _version: &str) {
        unimplemented!("OpsRig::serve_registry_plugin")
    }

    /// Test hook `OpsRig::registry_metadata_states` used by the gap tests.
    pub fn registry_metadata_states(&self, _id: &str) -> Vec<(String, String)> {
        unimplemented!("OpsRig::registry_metadata_states")
    }

    /// Test hook `OpsRig::registry_metadata_visible_versions` used by the gap tests.
    pub fn registry_metadata_visible_versions(&self, _id: &str) -> Vec<String> {
        unimplemented!("OpsRig::registry_metadata_visible_versions")
    }

    /// Test hook `OpsRig::plant_pending_review_version` used by the gap tests.
    pub fn plant_pending_review_version(&self, _id: &str, _version: &str) {
        unimplemented!("OpsRig::plant_pending_review_version")
    }

    /// Test hook `OpsRig::build_release_twice` used by the gap tests.
    pub fn build_release_twice(&self, _tag: &str) -> (Vec<u8>, Vec<u8>) {
        unimplemented!("OpsRig::build_release_twice")
    }

    /// Test hook `OpsRig::release_artifacts` used by the gap tests.
    pub fn release_artifacts(&self, _tag: &str) -> Vec<String> {
        unimplemented!("OpsRig::release_artifacts")
    }

    /// Test hook `OpsRig::clock_now_rfc3339_plus_secs` used by the gap tests.
    pub fn clock_now_rfc3339_plus_secs(&self, _secs: u64) -> String {
        unimplemented!("OpsRig::clock_now_rfc3339_plus_secs")
    }

    /// Test hook `OpsRig::plugins_screen` used by the gap tests.
    pub fn plugins_screen(&self) -> PluginsScreen {
        unimplemented!("OpsRig::plugins_screen")
    }

    /// Test hook `OpsRig::boot_with_bundled_cores` used by the gap tests.
    pub fn boot_with_bundled_cores(
        &self,
        _specs: &[PackageSpec],
    ) -> Result<ServerHandle, PluginError> {
        unimplemented!("OpsRig::boot_with_bundled_cores")
    }

    /// Test hook `OpsRig::fail_next_migration` used by the gap tests.
    pub fn fail_next_migration(&self, _id: &str) {
        unimplemented!("OpsRig::fail_next_migration")
    }

    /// Test hook `OpsRig::start_server_refusal` used by the gap tests.
    pub fn start_server_refusal(&self, _session: &str) -> Option<StartRefusal> {
        unimplemented!("OpsRig::start_server_refusal")
    }

    /// Test hook `OpsRig::config_text` used by the gap tests.
    pub fn config_text(&self) -> String {
        unimplemented!("OpsRig::config_text")
    }

    /// Test hook `OpsRig::log_codes` used by the gap tests.
    pub fn log_codes(&self) -> Vec<String> {
        unimplemented!("OpsRig::log_codes")
    }

    /// Test hook `OpsRig::set_kill_switch` used by the gap tests.
    pub fn set_kill_switch(&self, _wave: &str, _on: bool) {
        unimplemented!("OpsRig::set_kill_switch")
    }

    /// Test hook `OpsRig::storage_fingerprint` used by the gap tests.
    pub fn storage_fingerprint(&self) -> u64 {
        unimplemented!("OpsRig::storage_fingerprint")
    }

    /// Test hook `OpsRig::run_release_gate` used by the gap tests.
    pub fn run_release_gate(&self, _wave: &str, _candidate: &str) -> CliOutput {
        unimplemented!("OpsRig::run_release_gate")
    }

    /// Test hook `OpsRig::release_job_status` used by the gap tests.
    pub fn release_job_status(&self, _wave: &str) -> String {
        unimplemented!("OpsRig::release_job_status")
    }

    /// Test hook `OpsRig::gate_records` used by the gap tests.
    pub fn gate_records(&self) -> Vec<GateRecord> {
        unimplemented!("OpsRig::gate_records")
    }

    /// Test hook `OpsRig::add_remote_provider` used by the gap tests.
    pub fn add_remote_provider(&self, _name: &str, _retention: Option<&str>) {
        unimplemented!("OpsRig::add_remote_provider")
    }

    /// Test hook `OpsRig::platform_supports_memory_lock` used by the gap tests.
    pub fn platform_supports_memory_lock(&self) -> bool {
        unimplemented!("OpsRig::platform_supports_memory_lock")
    }

    /// Test hook `OpsRig::diagnostics_text` used by the gap tests.
    pub fn diagnostics_text(&self) -> String {
        unimplemented!("OpsRig::diagnostics_text")
    }

    /// Test hook `OpsRig::core_dump_contains` used by the gap tests.
    pub fn core_dump_contains(&self, _needle: &str) -> bool {
        unimplemented!("OpsRig::core_dump_contains")
    }

    /// Test hook `OpsRig::swap_contains` used by the gap tests.
    pub fn swap_contains(&self, _needle: &str) -> bool {
        unimplemented!("OpsRig::swap_contains")
    }

    /// Test hook `OpsRig::is_enabled` used by the gap tests.
    pub fn is_enabled(&self, _id: &str) -> bool {
        unimplemented!("OpsRig::is_enabled")
    }

    /// Test hook `OpsRig::bundled_manifest` used by the gap tests.
    pub fn bundled_manifest(&self, _id: &str) -> serde_json::Value {
        unimplemented!("OpsRig::bundled_manifest")
    }

    /// Test hook `OpsRig::replacement_matrix_row` used by the gap tests.
    pub fn replacement_matrix_row(&self, _id: &str) -> serde_json::Value {
        unimplemented!("OpsRig::replacement_matrix_row")
    }

    /// Test hook `OpsRig::bundled_version` used by the gap tests.
    pub fn bundled_version(&self, _id: &str) -> String {
        unimplemented!("OpsRig::bundled_version")
    }

    /// Test hook `OpsRig::core_plugin_ids` used by the gap tests.
    pub fn core_plugin_ids(&self) -> Vec<String> {
        unimplemented!("OpsRig::core_plugin_ids")
    }

    /// Test hook `OpsRig::conformance_suite_name` used by the gap tests.
    pub fn conformance_suite_name(&self, _id: &str) -> Option<String> {
        unimplemented!("OpsRig::conformance_suite_name")
    }

    /// Test hook `OpsRig::remove_core_suite` used by the gap tests.
    pub fn remove_core_suite(&self, _short_name: &str) {
        unimplemented!("OpsRig::remove_core_suite")
    }

    /// Test hook `OpsRig::check_core_suites` used by the gap tests.
    pub fn check_core_suites(&self) -> CliOutput {
        unimplemented!("OpsRig::check_core_suites")
    }

    /// Test hook `OpsRig::host_calls_of` used by the gap tests.
    pub fn host_calls_of(&self, _id: &str) -> Vec<String> {
        unimplemented!("OpsRig::host_calls_of")
    }

    /// Test hook `OpsRig::t1_public_api` used by the gap tests.
    pub fn t1_public_api(&self) -> Vec<String> {
        unimplemented!("OpsRig::t1_public_api")
    }

    /// Test hook `OpsRig::corrupt_bundled_core` used by the gap tests.
    pub fn corrupt_bundled_core(&self, _id: &str) {
        unimplemented!("OpsRig::corrupt_bundled_core")
    }

    /// Test hook `OpsRig::install_override_incompatible_abi` used by the gap tests.
    pub fn install_override_incompatible_abi(&self, _id: &str) {
        unimplemented!("OpsRig::install_override_incompatible_abi")
    }

    /// Test hook `OpsRig::tier_limits` used by the gap tests.
    pub fn tier_limits(&self, _tier: ilium_plugin_api::Tier) -> TierLimits {
        unimplemented!("OpsRig::tier_limits")
    }

    /// Test hook `OpsRig::limits_of` used by the gap tests.
    pub fn limits_of(&self, _id: &str) -> TierLimits {
        unimplemented!("OpsRig::limits_of")
    }

    /// Test hook `OpsRig::emit_plugin_log_bytes` used by the gap tests.
    pub fn emit_plugin_log_bytes(&self, _id: &str, _bytes: u64) {
        unimplemented!("OpsRig::emit_plugin_log_bytes")
    }

    /// Test hook `OpsRig::plugin_log_files` used by the gap tests.
    pub fn plugin_log_files(&self, _id: &str) -> Vec<PathBuf> {
        unimplemented!("OpsRig::plugin_log_files")
    }

    /// Test hook `OpsRig::set_plugin_log_age_days` used by the gap tests.
    pub fn set_plugin_log_age_days(&self, _id: &str, _days: u64) {
        unimplemented!("OpsRig::set_plugin_log_age_days")
    }

    /// Test hook `OpsRig::plugin_rotated_logs` used by the gap tests.
    pub fn plugin_rotated_logs(&self, _id: &str) -> Vec<PathBuf> {
        unimplemented!("OpsRig::plugin_rotated_logs")
    }

    /// Test hook `OpsRig::emit_plugin_log` used by the gap tests.
    pub fn emit_plugin_log(&self, _id: &str, _level: &str, _message: &str) {
        unimplemented!("OpsRig::emit_plugin_log")
    }

    /// Test hook `OpsRig::emit_plugin_log_raw` used by the gap tests.
    pub fn emit_plugin_log_raw(&self, _id: &str, _bytes: &[u8]) {
        unimplemented!("OpsRig::emit_plugin_log_raw")
    }

    /// Test hook `OpsRig::plugin_side_counter` used by the gap tests.
    pub fn plugin_side_counter(&self, _id: &str, _counter: &str) -> u64 {
        unimplemented!("OpsRig::plugin_side_counter")
    }

    /// Test hook `OpsRig::set_log_min_level` used by the gap tests.
    pub fn set_log_min_level(&self, _level: &str) {
        unimplemented!("OpsRig::set_log_min_level")
    }

    /// Test hook `OpsRig::emit_plugin_log_lines` used by the gap tests.
    pub fn emit_plugin_log_lines(&self, _id: &str, _level: &str, _count: u32) {
        unimplemented!("OpsRig::emit_plugin_log_lines")
    }

    /// Test hook `OpsRig::plugin_rate_budget_used` used by the gap tests.
    pub fn plugin_rate_budget_used(&self, _id: &str) -> u32 {
        unimplemented!("OpsRig::plugin_rate_budget_used")
    }

    /// Test hook `OpsRig::registry_table_codes` used by the gap tests.
    pub fn registry_table_codes(&self) -> Vec<String> {
        unimplemented!("OpsRig::registry_table_codes")
    }

    /// Test hook `OpsRig::emitted_host_codes` used by the gap tests.
    pub fn emitted_host_codes(&self) -> Vec<String> {
        unimplemented!("OpsRig::emitted_host_codes")
    }

    /// Test hook `OpsRig::plugin_log_text` used by the gap tests.
    pub fn plugin_log_text(&self, _id: &str) -> String {
        unimplemented!("OpsRig::plugin_log_text")
    }

    /// Test hook `OpsRig::set_env` used by the gap tests.
    pub fn set_env(&self, _key: &str, _value: &str) {
        unimplemented!("OpsRig::set_env")
    }

    /// Test hook `OpsRig::call_plugin_export` used by the gap tests.
    pub fn call_plugin_export(&self, _id: &str, _export: &str) {
        unimplemented!("OpsRig::call_plugin_export")
    }

    /// Test hook `OpsRig::trace_file_cap_bytes` used by the gap tests.
    pub fn trace_file_cap_bytes(&self) -> u64 {
        unimplemented!("OpsRig::trace_file_cap_bytes")
    }

    /// Test hook `OpsRig::trace_spans` used by the gap tests.
    pub fn trace_spans(&self) -> Vec<TraceSpan> {
        unimplemented!("OpsRig::trace_spans")
    }

    /// Test hook `OpsRig::call_cross_plugin_chain` used by the gap tests.
    pub fn call_cross_plugin_chain(&self, _id: &str, _export: &str) {
        unimplemented!("OpsRig::call_cross_plugin_chain")
    }

    /// Test hook `OpsRig::correlation_id_of_last_call` used by the gap tests.
    pub fn correlation_id_of_last_call(&self) -> String {
        unimplemented!("OpsRig::correlation_id_of_last_call")
    }

    /// Test hook `OpsRig::open_network_sockets_for_metrics` used by the gap tests.
    pub fn open_network_sockets_for_metrics(&self) -> Vec<String> {
        unimplemented!("OpsRig::open_network_sockets_for_metrics")
    }

    /// Test hook `OpsRig::call_many_exports` used by the gap tests.
    pub fn call_many_exports(&self, _id: &str, _count: u32) {
        unimplemented!("OpsRig::call_many_exports")
    }

    /// Test hook `OpsRig::metric_series_count` used by the gap tests.
    pub fn metric_series_count(&self, _id: &str, _side: Side) -> usize {
        unimplemented!("OpsRig::metric_series_count")
    }

    /// Test hook `OpsRig::call_plugin_export_times` used by the gap tests.
    pub fn call_plugin_export_times(&self, _id: &str, _export: &str, _times: u32) {
        unimplemented!("OpsRig::call_plugin_export_times")
    }

    /// Test hook `OpsRig::histogram_bucket_sum` used by the gap tests.
    pub fn histogram_bucket_sum(&self, _id: &str, _export: &str) -> u64 {
        unimplemented!("OpsRig::histogram_bucket_sum")
    }

    /// Test hook `OpsRig::calls_counter` used by the gap tests.
    pub fn calls_counter(&self, _id: &str, _export: &str) -> u64 {
        unimplemented!("OpsRig::calls_counter")
    }

    /// Test hook `OpsRig::step_monotonic_clock_backwards_ms` used by the gap tests.
    pub fn step_monotonic_clock_backwards_ms(&self, _ms: u64) {
        unimplemented!("OpsRig::step_monotonic_clock_backwards_ms")
    }

    /// Test hook `OpsRig::minimum_recorded_duration_ns` used by the gap tests.
    pub fn minimum_recorded_duration_ns(&self, _id: &str, _export: &str) -> i64 {
        unimplemented!("OpsRig::minimum_recorded_duration_ns")
    }

    /// Test hook `OpsRig::bundle_entry_names` used by the gap tests.
    pub fn bundle_entry_names(&self, _path: &Path) -> Vec<String> {
        unimplemented!("OpsRig::bundle_entry_names")
    }

    /// Test hook `OpsRig::bundle_is_redacted` used by the gap tests.
    pub fn bundle_is_redacted(&self, _path: &Path) -> bool {
        unimplemented!("OpsRig::bundle_is_redacted")
    }

    /// Test hook `OpsRig::plugin_count` used by the gap tests.
    pub fn plugin_count(&self, _id: &str, _name: &str, _delta: u64) -> Result<(), PluginError> {
        unimplemented!("OpsRig::plugin_count")
    }

    /// Test hook `OpsRig::install_many` used by the gap tests.
    pub fn install_many(&self, _count: u32) {
        unimplemented!("OpsRig::install_many")
    }

    /// Test hook `OpsRig::plugin_export_calls_during_read` used by the gap tests.
    pub fn plugin_export_calls_during_read(&self) -> u32 {
        unimplemented!("OpsRig::plugin_export_calls_during_read")
    }

    /// Test hook `OpsRig::histogram_window_count` used by the gap tests.
    pub fn histogram_window_count(&self, _id: &str) -> u64 {
        unimplemented!("OpsRig::histogram_window_count")
    }

    /// Test hook `OpsRig::hourly_bucket_count` used by the gap tests.
    pub fn hourly_bucket_count(&self, _id: &str) -> u64 {
        unimplemented!("OpsRig::hourly_bucket_count")
    }

    /// Test hook `OpsRig::fill_writer_queue` used by the gap tests.
    pub fn fill_writer_queue(&self, _records: u32) {
        unimplemented!("OpsRig::fill_writer_queue")
    }

    /// Test hook `OpsRig::observability_dropped` used by the gap tests.
    pub fn observability_dropped(&self) -> u64 {
        unimplemented!("OpsRig::observability_dropped")
    }

    /// Test hook `OpsRig::log_code_count` used by the gap tests.
    pub fn log_code_count(&self, _code: &str) -> usize {
        unimplemented!("OpsRig::log_code_count")
    }

    /// Test hook `OpsRig::set_local_timezone` used by the gap tests.
    pub fn set_local_timezone(&self, _zone: &str) {
        unimplemented!("OpsRig::set_local_timezone")
    }

    /// Test hook `OpsRig::plugin_log_last_ts` used by the gap tests.
    pub fn plugin_log_last_ts(&self, _id: &str) -> String {
        unimplemented!("OpsRig::plugin_log_last_ts")
    }

    /// Test hook `OpsRig::local_display_timestamp_of` used by the gap tests.
    pub fn local_display_timestamp_of(&self, _stored: &str) -> String {
        unimplemented!("OpsRig::local_display_timestamp_of")
    }

    /// Test hook `OpsRig::log_lines_with_code` used by the gap tests.
    pub fn log_lines_with_code(&self, _code: &str) -> Vec<serde_json::Value> {
        unimplemented!("OpsRig::log_lines_with_code")
    }

    /// Test hook `OpsRig::force_health` used by the gap tests.
    pub fn force_health(&self, _id: &str, _state: &str) {
        unimplemented!("OpsRig::force_health")
    }

    /// Test hook `OpsRig::install_slow_sync_host_call_fixture` used by the gap tests.
    pub fn install_slow_sync_host_call_fixture(&self, _id: &str, _duration: Duration) {
        unimplemented!("OpsRig::install_slow_sync_host_call_fixture")
    }

    /// Test hook `OpsRig::trigger_slow_host_call` used by the gap tests.
    pub fn trigger_slow_host_call(&self, _id: &str) {
        unimplemented!("OpsRig::trigger_slow_host_call")
    }

    /// Test hook `OpsRig::notifications_for` used by the gap tests.
    pub fn notifications_for(&self, _id: &str) -> usize {
        unimplemented!("OpsRig::notifications_for")
    }

    /// Test hook `OpsRig::make_plugin_ignore_stop` used by the gap tests.
    pub fn make_plugin_ignore_stop(&self, _id: &str) {
        unimplemented!("OpsRig::make_plugin_ignore_stop")
    }

    /// Test hook `OpsRig::plugin_interrupted_at` used by the gap tests.
    pub fn plugin_interrupted_at(&self, _id: &str) -> Duration {
        unimplemented!("OpsRig::plugin_interrupted_at")
    }

    /// Test hook `OpsRig::begin_update_swap` used by the gap tests.
    pub fn begin_update_swap(&self, _id: &str, _spec: &PackageSpec) {
        unimplemented!("OpsRig::begin_update_swap")
    }

    /// Test hook `OpsRig::kill_host_between_steps` used by the gap tests.
    pub fn kill_host_between_steps(&self) {
        unimplemented!("OpsRig::kill_host_between_steps")
    }

    /// Test hook `OpsRig::begin_purge` used by the gap tests.
    pub fn begin_purge(&self, _id: &str) {
        unimplemented!("OpsRig::begin_purge")
    }

    /// Test hook `OpsRig::plugin_export_calls_before_recovery` used by the gap tests.
    pub fn plugin_export_calls_before_recovery(&self) -> u32 {
        unimplemented!("OpsRig::plugin_export_calls_before_recovery")
    }

    /// Test hook `OpsRig::active_version` used by the gap tests.
    pub fn active_version(&self, _id: &str) -> Option<String> {
        unimplemented!("OpsRig::active_version")
    }

    /// Test hook `OpsRig::kill_points` used by the gap tests.
    pub fn kill_points(&self) -> Vec<String> {
        unimplemented!("OpsRig::kill_points")
    }

    /// Test hook `OpsRig::fresh_scenario_for` used by the gap tests.
    pub fn fresh_scenario_for(&self, _point: &str) -> Scenario {
        unimplemented!("OpsRig::fresh_scenario_for")
    }

    /// Test hook `OpsRig::state_is_consistent` used by the gap tests.
    pub fn state_is_consistent(&self, _id: &str) -> bool {
        unimplemented!("OpsRig::state_is_consistent")
    }

    /// Test hook `OpsRig::fault_writer_checked` used by the gap tests.
    pub fn fault_writer_checked(&self, _point: &str) -> bool {
        unimplemented!("OpsRig::fault_writer_checked")
    }

    /// Test hook `OpsRig::activate` used by the gap tests.
    pub fn activate(&self, _id: &str) {
        unimplemented!("OpsRig::activate")
    }

    /// Test hook `OpsRig::telemetry_records` used by the gap tests.
    pub fn telemetry_records(&self, _metric: &str, _id: &str) -> Vec<serde_json::Value> {
        unimplemented!("OpsRig::telemetry_records")
    }

    /// Test hook `OpsRig::cold_compile_time` used by the gap tests.
    pub fn cold_compile_time(&self, _id: &str) -> Duration {
        unimplemented!("OpsRig::cold_compile_time")
    }

    /// Test hook `OpsRig::warm_cache_start_time` used by the gap tests.
    pub fn warm_cache_start_time(&self, _id: &str) -> Duration {
        unimplemented!("OpsRig::warm_cache_start_time")
    }

    /// Test hook `OpsRig::corrupt_compile_cache_entry` used by the gap tests.
    pub fn corrupt_compile_cache_entry(&self, _id: &str) {
        unimplemented!("OpsRig::corrupt_compile_cache_entry")
    }

    /// Test hook `OpsRig::compile_cache_limit_bytes` used by the gap tests.
    pub fn compile_cache_limit_bytes(&self) -> u64 {
        unimplemented!("OpsRig::compile_cache_limit_bytes")
    }

    /// Test hook `OpsRig::fill_compile_cache_beyond_limit` used by the gap tests.
    pub fn fill_compile_cache_beyond_limit(&self) {
        unimplemented!("OpsRig::fill_compile_cache_beyond_limit")
    }

    /// Test hook `OpsRig::compile_cache_bytes` used by the gap tests.
    pub fn compile_cache_bytes(&self) -> u64 {
        unimplemented!("OpsRig::compile_cache_bytes")
    }

    /// Test hook `OpsRig::write_baseline_medians` used by the gap tests.
    pub fn write_baseline_medians(&self, _medians: &[(&str, u64)]) {
        unimplemented!("OpsRig::write_baseline_medians")
    }

    /// Test hook `OpsRig::write_current_medians` used by the gap tests.
    pub fn write_current_medians(&self, _medians: &[(&str, u64)], _budget_ns: u64) {
        unimplemented!("OpsRig::write_current_medians")
    }

    /// Test hook `OpsRig::run_benchmark` used by the gap tests.
    pub fn run_benchmark(&self, _id: &str) -> BenchReport {
        unimplemented!("OpsRig::run_benchmark")
    }

    /// Test hook `OpsRig::run_benchmark_harness` used by the gap tests.
    pub fn run_benchmark_harness(&self) -> String {
        unimplemented!("OpsRig::run_benchmark_harness")
    }

    /// Test hook `OpsRig::write_results_file_with_failed_pass` used by the gap tests.
    pub fn write_results_file_with_failed_pass(&self) -> PathBuf {
        unimplemented!("OpsRig::write_results_file_with_failed_pass")
    }

    /// Test hook `OpsRig::spin_cpu` used by the gap tests.
    pub fn spin_cpu(&self, _id: &str, _duration: Duration) {
        unimplemented!("OpsRig::spin_cpu")
    }

    /// Test hook `OpsRig::plugin_side_stats` used by the gap tests.
    pub fn plugin_side_stats(&self, _id: &str, _side: Side) -> serde_json::Value {
        unimplemented!("OpsRig::plugin_side_stats")
    }

    /// Test hook `OpsRig::cpu_resolution_ms` used by the gap tests.
    pub fn cpu_resolution_ms(&self) -> u64 {
        unimplemented!("OpsRig::cpu_resolution_ms")
    }

    /// Test hook `OpsRig::handle_limit_per_side` used by the gap tests.
    pub fn handle_limit_per_side(&self) -> u32 {
        unimplemented!("OpsRig::handle_limit_per_side")
    }

    /// Test hook `OpsRig::plugin_open_handle` used by the gap tests.
    pub fn plugin_open_handle(&self, _id: &str, _kind: &str) -> Result<(), PluginError> {
        unimplemented!("OpsRig::plugin_open_handle")
    }

    /// Test hook `OpsRig::handles_open_by_kind` used by the gap tests.
    pub fn handles_open_by_kind(&self, _id: &str, _side: Side, _kind: &str) -> u32 {
        unimplemented!("OpsRig::handles_open_by_kind")
    }

    /// Test hook `OpsRig::stop_instance` used by the gap tests.
    pub fn stop_instance(&self, _id: &str) {
        unimplemented!("OpsRig::stop_instance")
    }

    /// Test hook `OpsRig::handles_open_total` used by the gap tests.
    pub fn handles_open_total(&self, _id: &str, _side: Side) -> u32 {
        unimplemented!("OpsRig::handles_open_total")
    }

    /// Test hook `OpsRig::queue_events_without_consuming` used by the gap tests.
    pub fn queue_events_without_consuming(&self, _id: &str, _events: u32, _payload_bytes: usize) {
        unimplemented!("OpsRig::queue_events_without_consuming")
    }

    /// Test hook `OpsRig::queue_charge_bytes` used by the gap tests.
    pub fn queue_charge_bytes(&self, _id: &str, _side: Side) -> u64 {
        unimplemented!("OpsRig::queue_charge_bytes")
    }

    /// Test hook `OpsRig::queue_dropped_oldest_count` used by the gap tests.
    pub fn queue_dropped_oldest_count(&self, _id: &str) -> u64 {
        unimplemented!("OpsRig::queue_dropped_oldest_count")
    }

    /// Test hook `OpsRig::top_refresh_interval_ms` used by the gap tests.
    pub fn top_refresh_interval_ms(&self) -> u64 {
        unimplemented!("OpsRig::top_refresh_interval_ms")
    }

    /// Test hook `OpsRig::flood_telemetry_buffers` used by the gap tests.
    pub fn flood_telemetry_buffers(&self) {
        unimplemented!("OpsRig::flood_telemetry_buffers")
    }

    /// Test hook `OpsRig::telemetry_memory_bytes` used by the gap tests.
    pub fn telemetry_memory_bytes(&self) -> u64 {
        unimplemented!("OpsRig::telemetry_memory_bytes")
    }

    /// Test hook `OpsRig::memory_bytes_peak` used by the gap tests.
    pub fn memory_bytes_peak(&self, _id: &str, _side: Side) -> u64 {
        unimplemented!("OpsRig::memory_bytes_peak")
    }

    /// Test hook `OpsRig::memory_samples_taken` used by the gap tests.
    pub fn memory_samples_taken(&self, _id: &str) -> u32 {
        unimplemented!("OpsRig::memory_samples_taken")
    }

    /// Test hook `OpsRig::memory_bytes_current` used by the gap tests.
    pub fn memory_bytes_current(&self, _id: &str, _side: Side) -> u64 {
        unimplemented!("OpsRig::memory_bytes_current")
    }

    /// Test hook `OpsRig::restart_instance` used by the gap tests.
    pub fn restart_instance(&self, _id: &str) {
        unimplemented!("OpsRig::restart_instance")
    }

    /// Test hook `OpsRig::use_memory_fraction` used by the gap tests.
    pub fn use_memory_fraction(&self, _id: &str, _fraction: f64) {
        unimplemented!("OpsRig::use_memory_fraction")
    }

    /// Test hook `OpsRig::allocate_past_limit` used by the gap tests.
    pub fn allocate_past_limit(&self, _id: &str) {
        unimplemented!("OpsRig::allocate_past_limit")
    }

    /// Test hook `OpsRig::last_trap_code` used by the gap tests.
    pub fn last_trap_code(&self, _id: &str, _side: Side) -> Option<String> {
        unimplemented!("OpsRig::last_trap_code")
    }

    /// Test hook `OpsRig::active_log_bytes` used by the gap tests.
    pub fn active_log_bytes(&self, _id: &str) -> u64 {
        unimplemented!("OpsRig::active_log_bytes")
    }

    /// Test hook `OpsRig::fill_plugin_logs_past_cap` used by the gap tests.
    pub fn fill_plugin_logs_past_cap(&self, _bytes: u64) {
        unimplemented!("OpsRig::fill_plugin_logs_past_cap")
    }

    /// Test hook `OpsRig::trigger_log_cap_eviction` used by the gap tests.
    pub fn trigger_log_cap_eviction(&self) {
        unimplemented!("OpsRig::trigger_log_cap_eviction")
    }

    /// Test hook `OpsRig::total_plugin_log_bytes` used by the gap tests.
    pub fn total_plugin_log_bytes(&self) -> u64 {
        unimplemented!("OpsRig::total_plugin_log_bytes")
    }

    /// Test hook `OpsRig::create_session_log_folder` used by the gap tests.
    pub fn create_session_log_folder(&self, _slug: &str, _age_days: u64) {
        unimplemented!("OpsRig::create_session_log_folder")
    }

    /// Test hook `OpsRig::session_log_folder_exists` used by the gap tests.
    pub fn session_log_folder_exists(&self, _slug: &str) -> bool {
        unimplemented!("OpsRig::session_log_folder_exists")
    }

    /// Test hook `OpsRig::plugin_log_files_in_every_session_folder` used by the gap tests.
    pub fn plugin_log_files_in_every_session_folder(&self, _id: &str) -> Vec<PathBuf> {
        unimplemented!("OpsRig::plugin_log_files_in_every_session_folder")
    }

    /// Test hook `OpsRig::set_config_value` used by the gap tests.
    pub fn set_config_value(&self, _key: &str, _value: &str) {
        unimplemented!("OpsRig::set_config_value")
    }

    /// Test hook `OpsRig::manual_limits_table` used by the gap tests.
    pub fn manual_limits_table(&self) -> LimitsTable {
        unimplemented!("OpsRig::manual_limits_table")
    }

    /// Test hook `OpsRig::limits_named_in_requirements` used by the gap tests.
    pub fn limits_named_in_requirements(&self) -> Vec<LimitSpec> {
        unimplemented!("OpsRig::limits_named_in_requirements")
    }

    /// Test hook `OpsRig::fill_plugin_data_mib` used by the gap tests.
    pub fn fill_plugin_data_mib(&self, _id: &str, _mib: u64) {
        unimplemented!("OpsRig::fill_plugin_data_mib")
    }

    /// Test hook `OpsRig::migrate_calls` used by the gap tests.
    pub fn migrate_calls(&self, _id: &str) -> Vec<serde_json::Value> {
        unimplemented!("OpsRig::migrate_calls")
    }

    /// Test hook `OpsRig::audit_entries_with_flag` used by the gap tests.
    pub fn audit_entries_with_flag(&self, _flag: &str) -> Vec<serde_json::Value> {
        unimplemented!("OpsRig::audit_entries_with_flag")
    }

    /// Test hook `OpsRig::backup_verified_by_byte_and_file_count` used by the gap tests.
    pub fn backup_verified_by_byte_and_file_count(&self, _id: &str) -> bool {
        unimplemented!("OpsRig::backup_verified_by_byte_and_file_count")
    }

    /// Test hook `OpsRig::plugin_storage_put` used by the gap tests.
    pub fn plugin_storage_put(
        &self,
        _id: &str,
        _key: &str,
        _value: &[u8],
    ) -> Result<(), PluginError> {
        unimplemented!("OpsRig::plugin_storage_put")
    }

    /// Test hook `OpsRig::plugin_storage_get` used by the gap tests.
    pub fn plugin_storage_get(&self, _id: &str, _key: &str) -> Option<Vec<u8>> {
        unimplemented!("OpsRig::plugin_storage_get")
    }

    /// Test hook `OpsRig::path_in_scratch` used by the gap tests.
    pub fn path_in_scratch(&self, _name: &str) -> PathBuf {
        unimplemented!("OpsRig::path_in_scratch")
    }

    /// Test hook `OpsRig::set_exported_data_version` used by the gap tests.
    pub fn set_exported_data_version(&self, _dir: &Path, _version: u32) {
        unimplemented!("OpsRig::set_exported_data_version")
    }

    /// Test hook `OpsRig::grow_storage_database_mib` used by the gap tests.
    pub fn grow_storage_database_mib(&self, _id: &str, _mib: u64) {
        unimplemented!("OpsRig::grow_storage_database_mib")
    }

    /// Test hook `OpsRig::open_storage_database` used by the gap tests.
    pub fn open_storage_database(&self, _id: &str) {
        unimplemented!("OpsRig::open_storage_database")
    }

    /// Test hook `OpsRig::hang_integrity_check` used by the gap tests.
    pub fn hang_integrity_check(&self, _id: &str) {
        unimplemented!("OpsRig::hang_integrity_check")
    }

    /// Test hook `OpsRig::start_other_plugin_while_checking` used by the gap tests.
    pub fn start_other_plugin_while_checking(&self, _id: &str) -> Result<(), String> {
        unimplemented!("OpsRig::start_other_plugin_while_checking")
    }

    /// Test hook `OpsRig::database_moved_aside_after_check_timeout` used by the gap tests.
    pub fn database_moved_aside_after_check_timeout(&self, _id: &str) -> bool {
        unimplemented!("OpsRig::database_moved_aside_after_check_timeout")
    }

    /// Test hook `OpsRig::read_file_text` used by the gap tests.
    pub fn read_file_text(&self, _path: &Path) -> String {
        unimplemented!("OpsRig::read_file_text")
    }

    /// Test hook `OpsRig::corrupt_copy_exists` used by the gap tests.
    pub fn corrupt_copy_exists(&self, _prefix: &str) -> bool {
        unimplemented!("OpsRig::corrupt_copy_exists")
    }

    /// Test hook `OpsRig::rebuilt_candidate_registry` used by the gap tests.
    pub fn rebuilt_candidate_registry(&self) -> Vec<CandidateEntry> {
        unimplemented!("OpsRig::rebuilt_candidate_registry")
    }

    /// Test hook `OpsRig::recovery_page_offers_rebuild` used by the gap tests.
    pub fn recovery_page_offers_rebuild(&self) -> bool {
        unimplemented!("OpsRig::recovery_page_offers_rebuild")
    }

    /// Test hook `OpsRig::disable_plugin_platform_entirely` used by the gap tests.
    pub fn disable_plugin_platform_entirely(&self) {
        unimplemented!("OpsRig::disable_plugin_platform_entirely")
    }

    /// Test hook `OpsRig::log_rotation_limit_mib` used by the gap tests.
    pub fn log_rotation_limit_mib(&self) -> u64 {
        unimplemented!("OpsRig::log_rotation_limit_mib")
    }

    /// Test hook `OpsRig::disk_pressure_step_active` used by the gap tests.
    pub fn disk_pressure_step_active(&self, _step: &str) -> bool {
        unimplemented!("OpsRig::disk_pressure_step_active")
    }

    /// Test hook `OpsRig::disk_low_events_received` used by the gap tests.
    pub fn disk_low_events_received(&self, _id: &str) -> Vec<serde_json::Value> {
        unimplemented!("OpsRig::disk_low_events_received")
    }

    /// Test hook `OpsRig::footer_warning_text` used by the gap tests.
    pub fn footer_warning_text(&self) -> String {
        unimplemented!("OpsRig::footer_warning_text")
    }

    /// Test hook `OpsRig::footer_warning_visible` used by the gap tests.
    pub fn footer_warning_visible(&self) -> bool {
        unimplemented!("OpsRig::footer_warning_visible")
    }

    /// Test hook `OpsRig::write_log_lines_while_full` used by the gap tests.
    pub fn write_log_lines_while_full(&self, _id: &str, _lines: u32) {
        unimplemented!("OpsRig::write_log_lines_while_full")
    }

    /// Test hook `OpsRig::blocked_installs_resumed` used by the gap tests.
    pub fn blocked_installs_resumed(&self) -> bool {
        unimplemented!("OpsRig::blocked_installs_resumed")
    }

    /// Test hook `OpsRig::make_data_writable` used by the gap tests.
    pub fn make_data_writable(&self, _id: &str) {
        unimplemented!("OpsRig::make_data_writable")
    }

    /// Test hook `OpsRig::plugin_failure_count` used by the gap tests.
    pub fn plugin_failure_count(&self, _id: &str) -> u32 {
        unimplemented!("OpsRig::plugin_failure_count")
    }

    /// Test hook `OpsRig::retry_intervals_are_backoff_capped_at_6_hours` used by the gap tests.
    pub fn retry_intervals_are_backoff_capped_at_6_hours(&self) -> bool {
        unimplemented!("OpsRig::retry_intervals_are_backoff_capped_at_6_hours")
    }

    /// Test hook `OpsRig::cached_registry_entries_marked_stale_after_14_days` used by the gap tests.
    pub fn cached_registry_entries_marked_stale_after_14_days(&self) -> bool {
        unimplemented!("OpsRig::cached_registry_entries_marked_stale_after_14_days")
    }

    /// Test hook `OpsRig::set_cached_revocation_age_days` used by the gap tests.
    pub fn set_cached_revocation_age_days(&self, _days: u64) {
        unimplemented!("OpsRig::set_cached_revocation_age_days")
    }

    /// Test hook `OpsRig::offline_revocation_policy_applies_to_registry_only` used by the gap tests.
    pub fn offline_revocation_policy_applies_to_registry_only(&self) -> bool {
        unimplemented!("OpsRig::offline_revocation_policy_applies_to_registry_only")
    }

    /// Test hook `OpsRig::serve_download_interrupted_each_time` used by the gap tests.
    pub fn serve_download_interrupted_each_time(&self, _id: &str, _version: &str) {
        unimplemented!("OpsRig::serve_download_interrupted_each_time")
    }

    /// Test hook `OpsRig::range_requests_made` used by the gap tests.
    pub fn range_requests_made(&self, _id: &str) -> u32 {
        unimplemented!("OpsRig::range_requests_made")
    }

    /// Test hook `OpsRig::partial_download_marker` used by the gap tests.
    pub fn partial_download_marker(&self, _id: &str) -> Option<(String, u64)> {
        unimplemented!("OpsRig::partial_download_marker")
    }

    /// Test hook `OpsRig::partial_bytes` used by the gap tests.
    pub fn partial_bytes(&self, _id: &str) -> u64 {
        unimplemented!("OpsRig::partial_bytes")
    }

    /// Test hook `OpsRig::snapshot_file_bytes` used by the gap tests.
    pub fn snapshot_file_bytes(&self, _session: &str) -> Vec<u8> {
        unimplemented!("OpsRig::snapshot_file_bytes")
    }

    /// Test hook `OpsRig::plugin_set_state` used by the gap tests.
    pub fn plugin_set_state(&self, _id: &str, _state: &[u8]) {
        unimplemented!("OpsRig::plugin_set_state")
    }

    /// Test hook `OpsRig::make_next_snapshot_unreadable` used by the gap tests.
    pub fn make_next_snapshot_unreadable(&self) {
        unimplemented!("OpsRig::make_next_snapshot_unreadable")
    }

    /// Test hook `OpsRig::create_session_bucket` used by the gap tests.
    pub fn create_session_bucket(&self, _id: &str, _session: &str) -> Result<(), PluginError> {
        unimplemented!("OpsRig::create_session_bucket")
    }

    /// Test hook `OpsRig::close_session` used by the gap tests.
    pub fn close_session(&self, _id: &str, _session: &str) {
        unimplemented!("OpsRig::close_session")
    }

    /// Test hook `OpsRig::session_bucket_exists` used by the gap tests.
    pub fn session_bucket_exists(&self, _id: &str, _session: &str) -> bool {
        unimplemented!("OpsRig::session_bucket_exists")
    }

    /// Test hook `OpsRig::snapshot_file_contains_state` used by the gap tests.
    pub fn snapshot_file_contains_state(&self, _id: &str, _state: &[u8]) -> bool {
        unimplemented!("OpsRig::snapshot_file_contains_state")
    }

    /// Test hook `OpsRig::uninstall` used by the gap tests.
    pub fn uninstall(&self, _id: &str) {
        unimplemented!("OpsRig::uninstall")
    }

    /// Test hook `OpsRig::plugin_plugin_state_after_install` used by the gap tests.
    pub fn plugin_plugin_state_after_install(&self, _id: &str) -> Vec<u8> {
        unimplemented!("OpsRig::plugin_plugin_state_after_install")
    }

    /// Test hook `OpsRig::snapshot_plugin_tree` used by the gap tests.
    pub fn snapshot_plugin_tree(&self) -> BTreeMap<PathBuf, Vec<u8>> {
        unimplemented!("OpsRig::snapshot_plugin_tree")
    }

    /// Test hook `OpsRig::plugin_state_after_install` used by the gap tests.
    pub fn plugin_state_after_install(&self, _id: &str) -> Vec<u8> {
        unimplemented!("OpsRig::plugin_state_after_install")
    }

    /// Test hook `OpsRig::placeholder_text` used by the gap tests.
    pub fn placeholder_text(&self) -> String {
        unimplemented!("OpsRig::placeholder_text")
    }

    /// Test hook `OpsRig::write_known_bytes` used by the gap tests.
    pub fn write_known_bytes(&self, _id: &str, _relative: &str, _bytes: &[u8]) {
        unimplemented!("OpsRig::write_known_bytes")
    }

    /// Test hook `OpsRig::on_disk_size_of_plugin_roots` used by the gap tests.
    pub fn on_disk_size_of_plugin_roots(&self) -> u64 {
        unimplemented!("OpsRig::on_disk_size_of_plugin_roots")
    }

    /// Test hook `OpsRig::archive_entries` used by the gap tests.
    pub fn archive_entries(&self, _archive: &Path) -> Vec<String> {
        unimplemented!("OpsRig::archive_entries")
    }

    /// Test hook `OpsRig::make_backup_archive` used by the gap tests.
    pub fn make_backup_archive(&self) -> PathBuf {
        unimplemented!("OpsRig::make_backup_archive")
    }

    /// Test hook `OpsRig::corrupt_archive_entry` used by the gap tests.
    pub fn corrupt_archive_entry(&self, _archive: &Path, _entry: &str) {
        unimplemented!("OpsRig::corrupt_archive_entry")
    }

    /// Test hook `OpsRig::archive_json_entry` used by the gap tests.
    pub fn archive_json_entry(&self, _archive: &Path, _entry: &str) -> serde_json::Value {
        unimplemented!("OpsRig::archive_json_entry")
    }

    /// Test hook `OpsRig::rewrite_archive_format` used by the gap tests.
    pub fn rewrite_archive_format(&self, _archive: &Path, _format: &str) {
        unimplemented!("OpsRig::rewrite_archive_format")
    }

    /// Test hook `OpsRig::rewrite_archive_host_version` used by the gap tests.
    pub fn rewrite_archive_host_version(&self, _archive: &Path, _version: &str) {
        unimplemented!("OpsRig::rewrite_archive_host_version")
    }

    /// Test hook `OpsRig::make_backup_archive_from_os` used by the gap tests.
    pub fn make_backup_archive_from_os(&self, _os: &str) -> PathBuf {
        unimplemented!("OpsRig::make_backup_archive_from_os")
    }

    /// Test hook `OpsRig::plugin_data_bytes` used by the gap tests.
    pub fn plugin_data_bytes(&self, _id: &str) -> Vec<u8> {
        unimplemented!("OpsRig::plugin_data_bytes")
    }

    /// Test hook `OpsRig::installed_json_separators_are_normalised` used by the gap tests.
    pub fn installed_json_separators_are_normalised(&self) -> bool {
        unimplemented!("OpsRig::installed_json_separators_are_normalised")
    }

    /// Test hook `OpsRig::mount_as_network_filesystem` used by the gap tests.
    pub fn mount_as_network_filesystem(&self, _which: &str) {
        unimplemented!("OpsRig::mount_as_network_filesystem")
    }

    /// Test hook `OpsRig::lock_file_text` used by the gap tests.
    pub fn lock_file_text(&self, _id: &str) -> String {
        unimplemented!("OpsRig::lock_file_text")
    }

    /// Test hook `OpsRig::make_lock_scheme_unavailable` used by the gap tests.
    pub fn make_lock_scheme_unavailable(&self) {
        unimplemented!("OpsRig::make_lock_scheme_unavailable")
    }

    /// Test hook `OpsRig::is_read_only` used by the gap tests.
    pub fn is_read_only(&self, _server: &ServerHandle) -> bool {
        unimplemented!("OpsRig::is_read_only")
    }

    /// Test hook `OpsRig::store_path_for` used by the gap tests.
    pub fn store_path_for(&self, _id: &str) -> String {
        unimplemented!("OpsRig::store_path_for")
    }

    /// Test hook `OpsRig::every_plugin_root_path_is_extended_length` used by the gap tests.
    pub fn every_plugin_root_path_is_extended_length(&self) -> bool {
        unimplemented!("OpsRig::every_plugin_root_path_is_extended_length")
    }

    /// Test hook `OpsRig::pretend_windows_host` used by the gap tests.
    pub fn pretend_windows_host(&self) {
        unimplemented!("OpsRig::pretend_windows_host")
    }

    /// Test hook `OpsRig::plant_symlink_inside_data` used by the gap tests.
    pub fn plant_symlink_inside_data(&self, _id: &str, _relative: &str) {
        unimplemented!("OpsRig::plant_symlink_inside_data")
    }

    /// Test hook `OpsRig::read_plugin_file` used by the gap tests.
    pub fn read_plugin_file(&self, _id: &str, _relative: &str) -> Result<Vec<u8>, PluginError> {
        unimplemented!("OpsRig::read_plugin_file")
    }

    /// Test hook `OpsRig::point_plugins_root_at_other_disk_owned_by_user` used by the gap tests.
    pub fn point_plugins_root_at_other_disk_owned_by_user(&self) {
        unimplemented!("OpsRig::point_plugins_root_at_other_disk_owned_by_user")
    }

    /// Test hook `OpsRig::existing_data_moved_automatically` used by the gap tests.
    pub fn existing_data_moved_automatically(&self) -> bool {
        unimplemented!("OpsRig::existing_data_moved_automatically")
    }

    /// Test hook `OpsRig::compile_cache_cap_mib` used by the gap tests.
    pub fn compile_cache_cap_mib(&self) -> u64 {
        unimplemented!("OpsRig::compile_cache_cap_mib")
    }

    /// Test hook `OpsRig::fill_compile_cache_with_in_use_entry` used by the gap tests.
    pub fn fill_compile_cache_with_in_use_entry(&self) {
        unimplemented!("OpsRig::fill_compile_cache_with_in_use_entry")
    }

    /// Test hook `OpsRig::in_use_entry_survived` used by the gap tests.
    pub fn in_use_entry_survived(&self) -> bool {
        unimplemented!("OpsRig::in_use_entry_survived")
    }

    /// Test hook `OpsRig::crash_during_cache_write` used by the gap tests.
    pub fn crash_during_cache_write(&self) {
        unimplemented!("OpsRig::crash_during_cache_write")
    }

    /// Test hook `OpsRig::truncated_cache_entry_fails_sha256_and_is_rebuilt` used by the gap tests.
    pub fn truncated_cache_entry_fails_sha256_and_is_rebuilt(&self) -> bool {
        unimplemented!("OpsRig::truncated_cache_entry_fails_sha256_and_is_rebuilt")
    }

    /// Test hook `OpsRig::write_plugin_cache` used by the gap tests.
    pub fn write_plugin_cache(&self, _id: &str, _bytes: u64) -> Result<(), PluginError> {
        unimplemented!("OpsRig::write_plugin_cache")
    }

    /// Test hook `OpsRig::make_cache_volume_low_on_free_space` used by the gap tests.
    pub fn make_cache_volume_low_on_free_space(&self, _free_bytes: u64) {
        unimplemented!("OpsRig::make_cache_volume_low_on_free_space")
    }

    /// Test hook `OpsRig::cache_evict_count` used by the gap tests.
    pub fn cache_evict_count(&self) -> u64 {
        unimplemented!("OpsRig::cache_evict_count")
    }

    /// Test hook `OpsRig::purge_plugin_in_project` used by the gap tests.
    pub fn purge_plugin_in_project(&self, _id: &str) {
        unimplemented!("OpsRig::purge_plugin_in_project")
    }

    /// Test hook `OpsRig::purged_json_retention_days` used by the gap tests.
    pub fn purged_json_retention_days(&self) -> u64 {
        unimplemented!("OpsRig::purged_json_retention_days")
    }

    /// Test hook `OpsRig::open_project_again` used by the gap tests.
    pub fn open_project_again(&self) {
        unimplemented!("OpsRig::open_project_again")
    }

    /// Test hook `OpsRig::project_bucket_exists` used by the gap tests.
    pub fn project_bucket_exists(&self, _id: &str) -> bool {
        unimplemented!("OpsRig::project_bucket_exists")
    }

    /// Test hook `OpsRig::leave_orphan_project_data` used by the gap tests.
    pub fn leave_orphan_project_data(&self, _id: &str) {
        unimplemented!("OpsRig::leave_orphan_project_data")
    }

    /// Test hook `OpsRig::orphan_data_still_present` used by the gap tests.
    pub fn orphan_data_still_present(&self, _id: &str) -> bool {
        unimplemented!("OpsRig::orphan_data_still_present")
    }

    /// Test hook `OpsRig::create_backup_file_aged_days` used by the gap tests.
    pub fn create_backup_file_aged_days(&self, _name: &str, _days: u64) {
        unimplemented!("OpsRig::create_backup_file_aged_days")
    }

    /// Test hook `OpsRig::set_wall_clock_back_days` used by the gap tests.
    pub fn set_wall_clock_back_days(&self, _days: u64) {
        unimplemented!("OpsRig::set_wall_clock_back_days")
    }

    /// Test hook `OpsRig::advance_retention_sweep` used by the gap tests.
    pub fn advance_retention_sweep(&self) {
        unimplemented!("OpsRig::advance_retention_sweep")
    }

    /// Test hook `OpsRig::use_monotonic_for_lock_staleness` used by the gap tests.
    pub fn use_monotonic_for_lock_staleness(&self, _id: &str) {
        unimplemented!("OpsRig::use_monotonic_for_lock_staleness")
    }

    /// Test hook `OpsRig::lock_stale_by_wall_clock_only` used by the gap tests.
    pub fn lock_stale_by_wall_clock_only(&self) -> bool {
        unimplemented!("OpsRig::lock_stale_by_wall_clock_only")
    }

    /// Test hook `OpsRig::soak_run_record` used by the gap tests.
    pub fn soak_run_record(&self, _duration: Duration) -> SoakRecord {
        unimplemented!("OpsRig::soak_run_record")
    }

    /// Test hook `OpsRig::open_descriptors_until_refused` used by the gap tests.
    pub fn open_descriptors_until_refused(&self, _id: &str) -> FdRefusal {
        unimplemented!("OpsRig::open_descriptors_until_refused")
    }

    /// Test hook `OpsRig::activation_refusal_code` used by the gap tests.
    pub fn activation_refusal_code(&self, _id: &str) -> String {
        unimplemented!("OpsRig::activation_refusal_code")
    }

    /// Test hook `OpsRig::soft_fd_limit` used by the gap tests.
    pub fn soft_fd_limit(&self) -> u64 {
        unimplemented!("OpsRig::soft_fd_limit")
    }

    /// Test hook `OpsRig::hard_fd_limit` used by the gap tests.
    pub fn hard_fd_limit(&self) -> u64 {
        unimplemented!("OpsRig::hard_fd_limit")
    }

    /// Test hook `OpsRig::run_storage_fault_injection` used by the gap tests.
    pub fn run_storage_fault_injection(&self, _id: &str, _writes: u32) -> FaultReport {
        unimplemented!("OpsRig::run_storage_fault_injection")
    }

    /// Test hook `OpsRig::release_gate_failed_when_suite_fails` used by the gap tests.
    pub fn release_gate_failed_when_suite_fails(&self, _report: &FaultReport) -> bool {
        unimplemented!("OpsRig::release_gate_failed_when_suite_fails")
    }

    /// Test hook `OpsRig::set_policy` used by the gap tests.
    pub fn set_policy(&self, _key: &str, _value: &str) {
        unimplemented!("OpsRig::set_policy")
    }

    /// Test hook `OpsRig::install_system_wide` used by the gap tests.
    pub fn install_system_wide(&self, _spec: &PackageSpec) {
        unimplemented!("OpsRig::install_system_wide")
    }

    /// Test hook `OpsRig::system_install_is_read_only_for_other_users` used by the gap tests.
    pub fn system_install_is_read_only_for_other_users(&self, _id: &str) -> bool {
        unimplemented!("OpsRig::system_install_is_read_only_for_other_users")
    }

    /// Test hook `OpsRig::corrupt_system_install_file` used by the gap tests.
    pub fn corrupt_system_install_file(&self, _id: &str, _relative: &str) {
        unimplemented!("OpsRig::corrupt_system_install_file")
    }

    /// Test hook `OpsRig::start_server_as_user` used by the gap tests.
    pub fn start_server_as_user(&self, _user: &str) -> ServerHandle {
        unimplemented!("OpsRig::start_server_as_user")
    }

    /// Test hook `OpsRig::compiled_cache_dir_for_user` used by the gap tests.
    pub fn compiled_cache_dir_for_user(&self, _user: &str) -> PathBuf {
        unimplemented!("OpsRig::compiled_cache_dir_for_user")
    }

    /// Test hook `OpsRig::copies_of_system_package_made` used by the gap tests.
    pub fn copies_of_system_package_made(&self) -> u32 {
        unimplemented!("OpsRig::copies_of_system_package_made")
    }

    /// Test hook `OpsRig::grant_as_user` used by the gap tests.
    pub fn grant_as_user(&self, _user: &str, _id: &str, _grant: &str) {
        unimplemented!("OpsRig::grant_as_user")
    }

    /// Test hook `OpsRig::user_has_grant` used by the gap tests.
    pub fn user_has_grant(&self, _user: &str, _id: &str, _grant: &str) -> bool {
        unimplemented!("OpsRig::user_has_grant")
    }

    /// Test hook `OpsRig::plugin_can_read_bucket_of_other_user` used by the gap tests.
    pub fn plugin_can_read_bucket_of_other_user(
        &self,
        _id: &str,
        _reader: &str,
        _owner: &str,
    ) -> bool {
        unimplemented!("OpsRig::plugin_can_read_bucket_of_other_user")
    }

    /// Test hook `OpsRig::ilium_folder_mode` used by the gap tests.
    pub fn ilium_folder_mode(&self) -> u32 {
        unimplemented!("OpsRig::ilium_folder_mode")
    }

    /// Test hook `OpsRig::user_plugin_data_absent` used by the gap tests.
    pub fn user_plugin_data_absent(&self, _user: &str) -> bool {
        unimplemented!("OpsRig::user_plugin_data_absent")
    }

    /// Test hook `OpsRig::system_audit_entries` used by the gap tests.
    pub fn system_audit_entries(&self, _action: &str) -> Vec<serde_json::Value> {
        unimplemented!("OpsRig::system_audit_entries")
    }

    /// Test hook `OpsRig::plant_stale_lock_file` used by the gap tests.
    pub fn plant_stale_lock_file(&self, _id: &str) {
        unimplemented!("OpsRig::plant_stale_lock_file")
    }

    /// Test hook `OpsRig::plant_orphan_staging_folder` used by the gap tests.
    pub fn plant_orphan_staging_folder(&self, _id: &str) {
        unimplemented!("OpsRig::plant_orphan_staging_folder")
    }

    /// Test hook `OpsRig::stale_lock_exists` used by the gap tests.
    pub fn stale_lock_exists(&self, _id: &str) -> bool {
        unimplemented!("OpsRig::stale_lock_exists")
    }

    /// Test hook `OpsRig::doctor_fix_actions_are_reversible` used by the gap tests.
    pub fn doctor_fix_actions_are_reversible(&self) -> bool {
        unimplemented!("OpsRig::doctor_fix_actions_are_reversible")
    }

    /// Test hook `OpsRig::hold_lock_with_holder` used by the gap tests.
    pub fn hold_lock_with_holder(
        &self,
        _id: &str,
        _holder: &str,
        _heartbeat_age_s: u64,
    ) -> LockHoldGuard {
        unimplemented!("OpsRig::hold_lock_with_holder")
    }

    /// Test hook `OpsRig::lock_waited_at_least` used by the gap tests.
    pub fn lock_waited_at_least(&self, _duration: Duration) -> bool {
        unimplemented!("OpsRig::lock_waited_at_least")
    }

    /// Test hook `OpsRig::open_plugins_screen` used by the gap tests.
    pub fn open_plugins_screen(&self) -> PluginsScreen {
        unimplemented!("OpsRig::open_plugins_screen")
    }

    /// Test hook `OpsRig::plugins_screen_refresh_interval_ms` used by the gap tests.
    pub fn plugins_screen_refresh_interval_ms(&self) -> u64 {
        unimplemented!("OpsRig::plugins_screen_refresh_interval_ms")
    }

    /// Test hook `OpsRig::set_host_identity` used by the gap tests.
    pub fn set_host_identity(&self, _index: u32) {
        unimplemented!("OpsRig::set_host_identity")
    }

    /// Test hook `OpsRig::audit_events` used by the gap tests.
    pub fn audit_events(&self) -> Vec<String> {
        unimplemented!("OpsRig::audit_events")
    }

    /// Test hook `OpsRig::rollout_state` used by the gap tests.
    pub fn rollout_state(&self, _id: &str) -> Option<String> {
        unimplemented!("OpsRig::rollout_state")
    }

    /// Test hook `OpsRig::present_transfer` used by the gap tests.
    pub fn present_transfer(
        &self,
        _id: &str,
        _receiving_key: &str,
        _signing_keys: &[&str],
    ) -> CliOutput {
        unimplemented!("OpsRig::present_transfer")
    }

    /// Test hook `OpsRig::pinned_key` used by the gap tests.
    pub fn pinned_key(&self, _id: &str) -> Option<String> {
        unimplemented!("OpsRig::pinned_key")
    }

    /// Test hook `OpsRig::recover_id` used by the gap tests.
    pub fn recover_id(&self, _id: &str, _recovery_request_id: &str) {
        unimplemented!("OpsRig::recover_id")
    }

    /// Test hook `OpsRig::bundle_entries` used by the gap tests.
    pub fn bundle_entries(&self, _path: &Path) -> Vec<String> {
        unimplemented!("OpsRig::bundle_entries")
    }

    /// Test hook `OpsRig::bundle_text` used by the gap tests.
    pub fn bundle_text(&self, _path: &Path) -> String {
        unimplemented!("OpsRig::bundle_text")
    }

    /// Test hook `OpsRig::bundle_index` used by the gap tests.
    pub fn bundle_index(&self, _path: &Path) -> String {
        unimplemented!("OpsRig::bundle_index")
    }

    /// Test hook `OpsRig::log_line` used by the gap tests.
    pub fn log_line(&self, _id: &str, _text: &str) {
        unimplemented!("OpsRig::log_line")
    }

    /// Test hook `OpsRig::home_dir` used by the gap tests.
    pub fn home_dir(&self) -> PathBuf {
        unimplemented!("OpsRig::home_dir")
    }

    /// Test hook `OpsRig::answer_bundle_offer` used by the gap tests.
    pub fn answer_bundle_offer(&self, _answer: &str) {
        unimplemented!("OpsRig::answer_bundle_offer")
    }

    /// Test hook `OpsRig::bundle_files` used by the gap tests.
    pub fn bundle_files(&self) -> Vec<PathBuf> {
        unimplemented!("OpsRig::bundle_files")
    }

    /// Test hook `OpsRig::age_bundles` used by the gap tests.
    pub fn age_bundles(&self, _age: Duration) {
        unimplemented!("OpsRig::age_bundles")
    }

    /// Test hook `OpsRig::timer_firings` used by the gap tests.
    pub fn timer_firings(&self, _id: &str) -> usize {
        unimplemented!("OpsRig::timer_firings")
    }

    /// Test hook `OpsRig::suspend_for` used by the gap tests.
    pub fn suspend_for(&self, _duration: Duration) {
        unimplemented!("OpsRig::suspend_for")
    }

    /// Test hook `OpsRig::jump_wall_clock` used by the gap tests.
    pub fn jump_wall_clock(&self, _seconds: i64) {
        unimplemented!("OpsRig::jump_wall_clock")
    }

    /// Test hook `OpsRig::age_backups` used by the gap tests.
    pub fn age_backups(&self, _id: &str, _age: Duration) {
        unimplemented!("OpsRig::age_backups")
    }

    /// Test hook `OpsRig::prune_backups` used by the gap tests.
    pub fn prune_backups(&self) {
        unimplemented!("OpsRig::prune_backups")
    }

    /// Test hook `OpsRig::backup_names` used by the gap tests.
    pub fn backup_names(&self, _id: &str) -> Vec<String> {
        unimplemented!("OpsRig::backup_names")
    }

    /// Test hook `OpsRig::plant_end_of_life` used by the gap tests.
    pub fn plant_end_of_life(&self, _id: &str, _date: &str) {
        unimplemented!("OpsRig::plant_end_of_life")
    }

    /// Test hook `OpsRig::plant_abandonment_notice` used by the gap tests.
    pub fn plant_abandonment_notice(&self, _id: &str, _age: Duration) {
        unimplemented!("OpsRig::plant_abandonment_notice")
    }

    /// Test hook `OpsRig::start_server_safe_mode` used by the gap tests.
    pub fn start_server_safe_mode(&self, _session: &str, _mode: &str) -> ServerHandle {
        unimplemented!("OpsRig::start_server_safe_mode")
    }

    /// Test hook `OpsRig::plant_interrupted_migrations` used by the gap tests.
    pub fn plant_interrupted_migrations(&self, _id: &str, _count: u32) {
        unimplemented!("OpsRig::plant_interrupted_migrations")
    }

    /// Test hook `OpsRig::make_plugins_root_read_only` used by the gap tests.
    pub fn make_plugins_root_read_only(&self) {
        unimplemented!("OpsRig::make_plugins_root_read_only")
    }

    /// Test hook `OpsRig::state_change_events` used by the gap tests.
    pub fn state_change_events(&self, _id: &str) -> Vec<String> {
        unimplemented!("OpsRig::state_change_events")
    }

    /// Test hook `OpsRig::start_long_call` used by the gap tests.
    pub fn start_long_call(&self, _id: &str, _export: &str) -> PendingRequest {
        unimplemented!("OpsRig::start_long_call")
    }

    /// Test hook `OpsRig::hold_uninstall` used by the gap tests.
    pub fn hold_uninstall(&self, _id: &str) {
        unimplemented!("OpsRig::hold_uninstall")
    }

    /// Test hook `OpsRig::enable_auto_update` used by the gap tests.
    pub fn enable_auto_update(&self) {
        unimplemented!("OpsRig::enable_auto_update")
    }

    /// Test hook `OpsRig::set_registry_candidate` used by the gap tests.
    pub fn set_registry_candidate(&self, _spec: &PackageSpec) {
        unimplemented!("OpsRig::set_registry_candidate")
    }

    /// Test hook `OpsRig::trigger_auto_update` used by the gap tests.
    pub fn trigger_auto_update(&self) {
        unimplemented!("OpsRig::trigger_auto_update")
    }

    /// Test hook `OpsRig::mark_package_state` used by the gap tests.
    pub fn mark_package_state(&self, _id: &str, _state: &str) {
        unimplemented!("OpsRig::mark_package_state")
    }

    /// Test hook `OpsRig::as_os_user` used by the gap tests.
    pub fn as_os_user(&self, _name: &str) -> OpsRig {
        unimplemented!("OpsRig::as_os_user")
    }

    /// Test hook `OpsRig::granted_for` used by the gap tests.
    pub fn granted_for(&self, _id: &str) -> Vec<String> {
        unimplemented!("OpsRig::granted_for")
    }

    /// Test hook `OpsRig::is_trusted` used by the gap tests.
    pub fn is_trusted(&self, _key_id: &str) -> bool {
        unimplemented!("OpsRig::is_trusted")
    }

    /// Test hook `OpsRig::set_host_api_version` used by the gap tests.
    pub fn set_host_api_version(&self, _version: &str) {
        unimplemented!("OpsRig::set_host_api_version")
    }

    /// Test hook `OpsRig::publish_to_registry` used by the gap tests.
    pub fn publish_to_registry(&self, _spec: &PackageSpec) {
        unimplemented!("OpsRig::publish_to_registry")
    }

    /// Test hook `OpsRig::load_catalogue` used by the gap tests.
    pub fn load_catalogue(&self, _tag: &str, _entries: &[(&str, &str)]) {
        unimplemented!("OpsRig::load_catalogue")
    }

    /// Test hook `OpsRig::load_catalogue_raw` used by the gap tests.
    pub fn load_catalogue_raw(&self, _tag: &str, _bytes: &[u8]) {
        unimplemented!("OpsRig::load_catalogue_raw")
    }

    /// Test hook `OpsRig::english_text` used by the gap tests.
    pub fn english_text(&self, _message_id: &str) -> String {
        unimplemented!("OpsRig::english_text")
    }

    /// Test hook `OpsRig::set_ui_language` used by the gap tests.
    pub fn set_ui_language(&self, _tag: &str) {
        unimplemented!("OpsRig::set_ui_language")
    }

    /// Test hook `OpsRig::plant_log_line` used by the gap tests.
    pub fn plant_log_line(&self, _id: &str, _line: &str) {
        unimplemented!("OpsRig::plant_log_line")
    }

    /// Test hook `OpsRig::plant_log_bytes` used by the gap tests.
    pub fn plant_log_bytes(&self, _id: &str, _bytes: u64) {
        unimplemented!("OpsRig::plant_log_bytes")
    }

    /// Test hook `OpsRig::last_bundle_path` used by the gap tests.
    pub fn last_bundle_path(&self) -> PathBuf {
        unimplemented!("OpsRig::last_bundle_path")
    }

    /// Test hook `OpsRig::support_bundles` used by the gap tests.
    pub fn support_bundles(&self) -> Vec<PathBuf> {
        unimplemented!("OpsRig::support_bundles")
    }

    /// Test hook `OpsRig::viewer_documents` used by the gap tests.
    pub fn viewer_documents(&self) -> Vec<String> {
        unimplemented!("OpsRig::viewer_documents")
    }

    /// Test hook `OpsRig::plugin_instance_id` used by the gap tests.
    pub fn plugin_instance_id(&self, _id: &str) -> String {
        unimplemented!("OpsRig::plugin_instance_id")
    }

    /// Test hook `OpsRig::remaining_entries_for` used by the gap tests.
    pub fn remaining_entries_for(&self, _id: &str) -> Vec<String> {
        unimplemented!("OpsRig::remaining_entries_for")
    }

    /// Test hook `OpsRig::file_digests` used by the gap tests.
    pub fn file_digests(&self, _dir: &Path) -> BTreeMap<PathBuf, String> {
        unimplemented!("OpsRig::file_digests")
    }

    /// Test hook `OpsRig::fail_export_after_files` used by the gap tests.
    pub fn fail_export_after_files(&self, _count: usize) {
        unimplemented!("OpsRig::fail_export_after_files")
    }

    /// Test hook `OpsRig::make_undeletable` used by the gap tests.
    pub fn make_undeletable(&self, _path: &Path) {
        unimplemented!("OpsRig::make_undeletable")
    }

    /// Test hook `OpsRig::put_plugin_in_state` used by the gap tests.
    pub fn put_plugin_in_state(&self, _id: &str, _state: &str) {
        unimplemented!("OpsRig::put_plugin_in_state")
    }

    /// Test hook `OpsRig::plugin_code_runs` used by the gap tests.
    pub fn plugin_code_runs(&self, _id: &str) -> u32 {
        unimplemented!("OpsRig::plugin_code_runs")
    }

    /// Test hook `OpsRig::plant_crash_record` used by the gap tests.
    pub fn plant_crash_record(&self, _id: &str, _count: u32) {
        unimplemented!("OpsRig::plant_crash_record")
    }

    /// Test hook `OpsRig::last_prompt` used by the gap tests.
    pub fn last_prompt(&self) -> String {
        unimplemented!("OpsRig::last_prompt")
    }

    /// Test hook `OpsRig::key_fingerprint` used by the gap tests.
    pub fn key_fingerprint(&self, _id: &str) -> String {
        unimplemented!("OpsRig::key_fingerprint")
    }

    /// Test hook `OpsRig::links_opened` used by the gap tests.
    pub fn links_opened(&self) -> Vec<String> {
        unimplemented!("OpsRig::links_opened")
    }

    /// Test hook `OpsRig::record_host_crashes_while_loading` used by the gap tests.
    pub fn record_host_crashes_while_loading(&self, _count: u32, _window: Duration) {
        unimplemented!("OpsRig::record_host_crashes_while_loading")
    }

    /// Test hook `OpsRig::start_server_with_args` used by the gap tests.
    pub fn start_server_with_args(&self, _session: &str, _args: &[&str]) -> ServerHandle {
        unimplemented!("OpsRig::start_server_with_args")
    }

    /// Test hook `OpsRig::start_server_with_env` used by the gap tests.
    pub fn start_server_with_env(&self, _session: &str, _env: &[(&str, &str)]) -> ServerHandle {
        unimplemented!("OpsRig::start_server_with_env")
    }

    /// Test hook `OpsRig::open_review_dialog` used by the gap tests.
    pub fn open_review_dialog(&self, _spec: &PackageSpec, _cols: u16, _rows: u16) -> ReviewDialog {
        unimplemented!("OpsRig::open_review_dialog")
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
    _private: (),
}

impl CaptureHandle {
    /// Whether the capture was refused.
    pub fn is_refused(&self) -> bool {
        unimplemented!("CaptureHandle::is_refused")
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
    _private: (),
}

impl Scenario {
    /// Kills the server at the named point.
    pub fn kill_at(&self, _point: &str) {
        unimplemented!("Scenario::kill_at")
    }

    /// Restarts the server after the kill.
    pub fn restart(&self) -> ServerHandle {
        unimplemented!("Scenario::restart")
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
    _private: (),
}

impl LockHoldGuard {
    /// Releases the lock.
    pub fn release(&self) {
        unimplemented!("LockHoldGuard::release")
    }
}

/// A grant review dialog with scrolling and a Confirm button.
#[derive(Debug)]
pub struct ReviewDialog {
    _private: (),
}

impl ReviewDialog {
    /// Number of rows below the visible fold.
    pub fn rows_below_fold(&self) -> usize {
        unimplemented!("ReviewDialog::rows_below_fold")
    }

    /// Whether Confirm is enabled.
    pub fn confirm_enabled(&self) -> bool {
        unimplemented!("ReviewDialog::confirm_enabled")
    }

    /// Scrolls to the end of the list.
    pub fn scroll_to_end(&self) {
        unimplemented!("ReviewDialog::scroll_to_end")
    }

    /// All text of the dialog.
    pub fn all_text(&self) -> String {
        unimplemented!("ReviewDialog::all_text")
    }

    /// The full wording of one grant.
    pub fn full_wording(&self, _grant: &str) -> String {
        unimplemented!("ReviewDialog::full_wording")
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
    _private: (),
}

impl DetailsPage {
    /// The page text.
    pub fn text(&self) -> String {
        unimplemented!("DetailsPage::text")
    }
}

/// A plugin window as rendered by the host.
#[derive(Debug)]
pub struct PluginWindow {
    _private: (),
}

impl PluginWindow {
    /// The host-drawn title line.
    pub fn title_line(&self) -> String {
        unimplemented!("PluginWindow::title_line")
    }

    /// Has the plugin draw text on a row.
    pub fn plugin_draw_on_row(&self, _row: usize, _text: &str) {
        unimplemented!("PluginWindow::plugin_draw_on_row")
    }

    /// The text of one row.
    pub fn row_text(&self, _row: usize) -> String {
        unimplemented!("PluginWindow::row_text")
    }

    /// Whether the last good frame is dimmed.
    pub fn is_dimmed_last_frame(&self) -> bool {
        unimplemented!("PluginWindow::is_dimmed_last_frame")
    }

    /// Whether a placeholder replaces the view.
    pub fn is_placeholder(&self) -> bool {
        unimplemented!("PluginWindow::is_placeholder")
    }

    /// The placeholder text.
    pub fn placeholder_text(&self) -> String {
        unimplemented!("PluginWindow::placeholder_text")
    }

    /// The key hints offered.
    pub fn keys(&self) -> Vec<String> {
        unimplemented!("PluginWindow::keys")
    }

    /// Whether the window is blank.
    pub fn is_blank(&self) -> bool {
        unimplemented!("PluginWindow::is_blank")
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
    pub fn default_for(&self, _name: &str) -> u64 {
        unimplemented!("LimitsTable::default_for")
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

impl Default for OpsRig {
    fn default() -> OpsRig {
        OpsRig::new()
    }
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

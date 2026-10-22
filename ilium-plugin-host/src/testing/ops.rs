//! Lifecycle-operations rig: drives whole install, update, session, persistence and dev-reload
//! flows in process with a virtual clock, a fake network and a fake disk (test plan lane
//! lifecycle-ops, LIF-060..199). Every item here is test support; nothing is implemented in the
//! skeleton state.

use std::path::{Path, PathBuf};
use std::time::Duration;

use ilium_plugin_api::generated::host::plugins::InstanceState;
use ilium_plugin_api::lifecycle::DeactivationReason;
use ilium_plugin_api::{PluginError, Side};

use crate::cli::CliOutput;

/// The compiled-code cache key of one component: hash, wasmtime version and CPU features (LIF-176).
pub fn compile_cache_key(
    _component_hash: &str,
    _wasmtime_version: &str,
    _cpu_features: &str,
) -> String {
    unimplemented!("host::testing::ops::compile_cache_key")
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
#[derive(Debug, Clone)]
pub struct PackageSpec {
    _private: (),
}

impl PackageSpec {
    /// A package with the given id and version.
    pub fn new(_id: &str, _version: &str) -> PackageSpec {
        unimplemented!("host::testing::ops::PackageSpec::new")
    }

    /// Adds a requested grant.
    pub fn grant(self, _grant: &str) -> PackageSpec {
        unimplemented!("host::testing::ops::PackageSpec::grant")
    }

    /// Adds a high-risk grant.
    pub fn high_risk_grant(self, _grant: &str) -> PackageSpec {
        unimplemented!("host::testing::ops::PackageSpec::high_risk_grant")
    }

    /// Adds a first-use-prompted grant.
    pub fn first_use_prompt(self, _grant: &str) -> PackageSpec {
        unimplemented!("host::testing::ops::PackageSpec::first_use_prompt")
    }

    /// Signs the package with the named key.
    pub fn signed(self, _key: &str) -> PackageSpec {
        unimplemented!("host::testing::ops::PackageSpec::signed")
    }

    /// Re-signs the package with a note.
    pub fn resigned(self, _note: &str) -> PackageSpec {
        unimplemented!("host::testing::ops::PackageSpec::resigned")
    }

    /// Signs with the same publisher key as another package.
    pub fn same_publisher_as(self, _key: &str) -> PackageSpec {
        unimplemented!("host::testing::ops::PackageSpec::same_publisher_as")
    }

    /// Writes an invalid manifest.
    pub fn bad_manifest(self) -> PackageSpec {
        unimplemented!("host::testing::ops::PackageSpec::bad_manifest")
    }

    /// Writes an invalid signature.
    pub fn bad_signature(self) -> PackageSpec {
        unimplemented!("host::testing::ops::PackageSpec::bad_signature")
    }

    /// Embeds a marker in the component so two versions differ in content.
    pub fn content_marker(self, _marker: &str) -> PackageSpec {
        unimplemented!("host::testing::ops::PackageSpec::content_marker")
    }

    /// Pads the package to the given size in bytes.
    pub fn padding(self, _bytes: u64) -> PackageSpec {
        unimplemented!("host::testing::ops::PackageSpec::padding")
    }

    /// Adds a client side.
    pub fn client_side(self) -> PackageSpec {
        unimplemented!("host::testing::ops::PackageSpec::client_side")
    }

    /// `activate` returns an error.
    pub fn activate_error(self) -> PackageSpec {
        unimplemented!("host::testing::ops::PackageSpec::activate_error")
    }

    /// `activate` traps.
    pub fn activate_traps(self) -> PackageSpec {
        unimplemented!("host::testing::ops::PackageSpec::activate_traps")
    }

    /// `activate` misses its deadline.
    pub fn activate_misses_deadline(self) -> PackageSpec {
        unimplemented!("host::testing::ops::PackageSpec::activate_misses_deadline")
    }

    /// `activate` writes the given file.
    pub fn start_writes_file(self, _path: &Path) -> PackageSpec {
        unimplemented!("host::testing::ops::PackageSpec::start_writes_file")
    }

    /// `deactivate` returns a restore blob of the given length.
    pub fn restore_bytes(self, _len: usize) -> PackageSpec {
        unimplemented!("host::testing::ops::PackageSpec::restore_bytes")
    }

    /// The manifest `data_version`.
    pub fn data_version(self, _version: u32) -> PackageSpec {
        unimplemented!("host::testing::ops::PackageSpec::data_version")
    }

    /// The manifest `data_version` as raw JSON text.
    pub fn raw_data_version(self, _json: &str) -> PackageSpec {
        unimplemented!("host::testing::ops::PackageSpec::raw_data_version")
    }

    /// Exports `migrator` on both sides.
    pub fn migrator_both(self) -> PackageSpec {
        unimplemented!("host::testing::ops::PackageSpec::migrator_both")
    }

    /// Exports `migrator` on the server side only.
    pub fn migrator_server_only(self) -> PackageSpec {
        unimplemented!("host::testing::ops::PackageSpec::migrator_server_only")
    }

    /// Lists `migrator` in the manifest without exporting it.
    pub fn migrator_listed_but_not_exported(self) -> PackageSpec {
        unimplemented!("host::testing::ops::PackageSpec::migrator_listed_but_not_exported")
    }

    /// Provides a service.
    pub fn provides_service(self, _service: &str) -> PackageSpec {
        unimplemented!("host::testing::ops::PackageSpec::provides_service")
    }

    /// Requires a service.
    pub fn requires_service(self, _service: &str) -> PackageSpec {
        unimplemented!("host::testing::ops::PackageSpec::requires_service")
    }

    /// `activate` waits for the given virtual time.
    pub fn wait_ms(self, _ms: u32) -> PackageSpec {
        unimplemented!("host::testing::ops::PackageSpec::wait_ms")
    }

    /// Owns an extension point.
    pub fn extension_point(self, _point: &str) -> PackageSpec {
        unimplemented!("host::testing::ops::PackageSpec::extension_point")
    }

    /// Contributes to an extension point.
    pub fn extension_contribution(self, _point: &str) -> PackageSpec {
        unimplemented!("host::testing::ops::PackageSpec::extension_contribution")
    }

    /// Declares an activation trigger.
    pub fn activation_trigger(self, _trigger: &str) -> PackageSpec {
        unimplemented!("host::testing::ops::PackageSpec::activation_trigger")
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
        unimplemented!("host::testing::ops::Effects::any")
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
        assert_eq!(restarted.read_storage(EXAMPLE_ID, "global", "greeting"), None);
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

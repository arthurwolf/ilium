//! Lifecycle host: a multi-plugin, multi-client host with a manual monotonic clock around a real
//! `LifecycleManager` (test plan lane lifecycle, LIF-001..059, LIF-122..145, LIF-180..186).
//! Test support only.

use ilium_plugin_api::generated::host::plugins::{
    HealthState, HostPlugins, InstanceState, PackageState, SideState,
};
use ilium_plugin_api::generated::types::{Outcome, RequestId};
use ilium_plugin_api::lifecycle::DeactivationReason;
use ilium_plugin_api::limits::LimitKey;
use ilium_plugin_api::{Diagnostic, PluginError, Side, Tier};

use crate::cli::CliOutput;
use crate::lifecycle::{LifecycleManager, StartMode, StartReport};

/// The safe mode of a start is the host's own (LIF-180, 181).
pub use crate::lifecycle::SafeMode;

/// A failure injected into one side instance (LIF-033, 035).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureKind {
    /// A trap.
    Trap,
    /// `activate` returns an error.
    ActivateError,
    /// `activate` misses its deadline.
    ActivateDeadline,
    /// An export misses its deadline.
    ExportDeadline,
    /// `deactivate` misses its deadline.
    DeactivateDeadline(DeactivationReason),
    /// The event queue overflows.
    EventQueueOverflow,
    /// The memory limit is hit.
    MemoryLimit,
    /// A required service provider fails to activate.
    ServiceProviderActivateFails,
    /// A job traps.
    JobTrap,
    /// An extension handler times out.
    ExtensionHandlerTimeout,
    /// The helper crashes while this plugin runs.
    HelperCrashRunning,
    /// The helper crashes while another plugin runs.
    HelperCrashOther,
}

/// Where an effective limit comes from (LIF-143).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitSource {
    /// The tier default.
    Default,
    /// The manifest.
    Manifest,
    /// The user setting.
    User,
    /// The tier ceiling.
    Ceiling,
    /// A fixed host value.
    Fixed,
}

/// One activation of one side in one scope (LIF-032, 045..047).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivationRecord {
    /// The plugin id.
    pub plugin: String,
    /// The side.
    pub side: Side,
    /// The scope.
    pub scope: String,
    /// Clock reading at begin.
    pub begin_ms: u64,
    /// Clock reading at end.
    pub end_ms: u64,
    /// Sequence number at begin.
    pub begin_seq: u64,
    /// Sequence number at end.
    pub end_seq: u64,
}

/// One `plugin-state-changed` record (LIF-016).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateChange {
    /// The plugin id.
    pub plugin: String,
    /// The side.
    pub side: Side,
    /// The scope.
    pub scope: String,
    /// The previous state.
    pub from: InstanceState,
    /// The new state.
    pub to: InstanceState,
    /// The reason text.
    pub reason: String,
    /// Wall clock of the change.
    pub at_unix_ms: u64,
}

/// One `service-availability-changed` record (LIF-053).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvailabilityChange {
    /// The service interface.
    pub interface: String,
    /// Whether a provider is available.
    pub available: bool,
    /// The provider plugin id.
    pub provider: String,
}

/// One captured log line (LIF-038, 056).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LifeLogLine {
    /// The plugin id.
    pub plugin: String,
    /// The side.
    pub side: Side,
    /// The scope.
    pub scope: String,
    /// The code.
    pub code: String,
    /// The message.
    pub message: String,
}

/// What a timed call did (LIF-125, 126, 129).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallResult {
    /// Whether the call trapped.
    pub trapped: bool,
    /// Virtual time the call took.
    pub elapsed_ms: u64,
}

/// What a hook chain did (LIF-056).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookResult {
    /// Handlers that passed.
    pub passes: u32,
}

/// A disable that has begun but not finished (LIF-020).
#[derive(Debug)]
pub struct PendingTransition {
    _private: (),
}

impl PendingTransition {
    /// Whether the transition finished.
    pub fn finished(&self) -> bool {
        unimplemented!("host::testing::life::PendingTransition::finished")
    }
}

/// Declares one test plugin without building a package (LIF-045..047, 055).
#[derive(Debug, Clone)]
pub struct PluginSpec {
    _private: (),
}

impl PluginSpec {
    /// A plugin with the given id.
    pub fn new(_id: &str) -> PluginSpec {
        unimplemented!("host::testing::life::PluginSpec::new")
    }

    /// The tier.
    pub fn tier(self, _tier: Tier) -> PluginSpec {
        unimplemented!("host::testing::life::PluginSpec::tier")
    }

    /// The sides.
    pub fn sides(self, _sides: &[Side]) -> PluginSpec {
        unimplemented!("host::testing::life::PluginSpec::sides")
    }

    /// Activates at startup.
    pub fn startup(self) -> PluginSpec {
        unimplemented!("host::testing::life::PluginSpec::startup")
    }

    /// Requires a plugin.
    pub fn requires_plugin(self, _id: &str) -> PluginSpec {
        unimplemented!("host::testing::life::PluginSpec::requires_plugin")
    }

    /// Requires an optional plugin.
    pub fn requires_optional_plugin(self, _id: &str) -> PluginSpec {
        unimplemented!("host::testing::life::PluginSpec::requires_optional_plugin")
    }

    /// Requires a host release.
    pub fn requires_host(self, _range: &str) -> PluginSpec {
        unimplemented!("host::testing::life::PluginSpec::requires_host")
    }

    /// Requires a service.
    pub fn requires_service(self, _service: &str) -> PluginSpec {
        unimplemented!("host::testing::life::PluginSpec::requires_service")
    }

    /// Provides a service.
    pub fn provides_service(self, _service: &str) -> PluginSpec {
        unimplemented!("host::testing::life::PluginSpec::provides_service")
    }

    /// Owns an extension point.
    pub fn owns_point(self, _point: &str) -> PluginSpec {
        unimplemented!("host::testing::life::PluginSpec::owns_point")
    }

    /// Contributes to an extension point.
    pub fn contributes_to(self, _point: &str) -> PluginSpec {
        unimplemented!("host::testing::life::PluginSpec::contributes_to")
    }

    /// The memory limit.
    pub fn memory_mib(self, _mib: u64) -> PluginSpec {
        unimplemented!("host::testing::life::PluginSpec::memory_mib")
    }

    /// `activate` takes this long.
    pub fn activate_delay_ms(self, _ms: u64) -> PluginSpec {
        unimplemented!("host::testing::life::PluginSpec::activate_delay_ms")
    }

    /// `deactivate` takes this long.
    pub fn deactivate_delay_ms(self, _ms: u64) -> PluginSpec {
        unimplemented!("host::testing::life::PluginSpec::deactivate_delay_ms")
    }

    /// Requested grants.
    pub fn grants(self, _grants: &[&str]) -> PluginSpec {
        unimplemented!("host::testing::life::PluginSpec::grants")
    }

    /// Replaces a core plugin.
    pub fn replaces(self, _core: &str) -> PluginSpec {
        unimplemented!("host::testing::life::PluginSpec::replaces")
    }
}

/// The lifecycle host (LIF-001..059, 122..145, 180..186).
#[derive(Debug)]
pub struct LifeHost {
    _private: (),
}

impl LifeHost {
    /// A fresh host.
    pub fn new() -> LifeHost {
        unimplemented!("host::testing::life::LifeHost::new")
    }

    /// Sets the session memory pool.
    pub fn with_pool_mib(self, _mib: u64) -> LifeHost {
        unimplemented!("host::testing::life::LifeHost::with_pool_mib")
    }

    /// Installs a plugin.
    pub fn install(&mut self, _spec: PluginSpec) {
        unimplemented!("host::testing::life::LifeHost::install")
    }

    /// Installs a plugin, expecting the install to be refused with a code.
    pub fn install_refused(&mut self, _spec: PluginSpec, _code: &str) {
        unimplemented!("host::testing::life::LifeHost::install_refused")
    }

    /// Installs a plugin that policy blocks.
    pub fn install_policy_blocked(&mut self, _spec: PluginSpec) {
        unimplemented!("host::testing::life::LifeHost::install_policy_blocked")
    }

    /// Tries to install a plugin.
    pub fn try_install(&mut self, _spec: PluginSpec) -> Result<(), Diagnostic> {
        unimplemented!("host::testing::life::LifeHost::try_install")
    }

    /// Tries to install a plugin with one manifest key overridden.
    pub fn try_install_manifest_key(
        &mut self,
        _plugin: &str,
        _key: &str,
        _json: &str,
    ) -> Result<(), Diagnostic> {
        unimplemented!("host::testing::life::LifeHost::try_install_manifest_key")
    }

    /// Starts the host.
    pub fn start(&mut self, _mode: StartMode) -> StartReport {
        unimplemented!("host::testing::life::LifeHost::start")
    }

    /// Starts the host in a safe mode chosen by flag or environment.
    pub fn start_safe(&mut self, _mode: SafeMode, _flag: Option<&str>, _env: Option<&str>) {
        unimplemented!("host::testing::life::LifeHost::start_safe")
    }

    /// The active safe mode.
    pub fn safe_mode(&self) -> SafeMode {
        unimplemented!("host::testing::life::LifeHost::safe_mode")
    }

    /// Records one crash during start at the current time.
    pub fn crash_during_start(&mut self) {
        unimplemented!("host::testing::life::LifeHost::crash_during_start")
    }

    /// Starts through the crash-loop guard.
    pub fn start_after_crashes(&mut self) {
        unimplemented!("host::testing::life::LifeHost::start_after_crashes")
    }

    /// Shuts the host down.
    pub fn shutdown(&mut self) {
        unimplemented!("host::testing::life::LifeHost::shutdown")
    }

    /// Begins a shutdown without finishing it.
    pub fn begin_shutdown(&mut self) {
        unimplemented!("host::testing::life::LifeHost::begin_shutdown")
    }

    /// Restarts the server.
    pub fn restart_server(&mut self) {
        unimplemented!("host::testing::life::LifeHost::restart_server")
    }

    /// Crashes the server.
    pub fn crash_server(&mut self) {
        unimplemented!("host::testing::life::LifeHost::crash_server")
    }

    /// Attaches a client; returns its scope.
    pub fn attach_client(&mut self, _name: &str) -> String {
        unimplemented!("host::testing::life::LifeHost::attach_client")
    }

    /// Detaches a client.
    pub fn detach_client(&mut self, _name: &str) {
        unimplemented!("host::testing::life::LifeHost::detach_client")
    }

    /// Kills a client.
    pub fn kill_client(&mut self, _name: &str) {
        unimplemented!("host::testing::life::LifeHost::kill_client")
    }

    /// Connects a client that speaks an API version.
    pub fn connect_client_with_api(
        &mut self,
        _name: &str,
        _api: &str,
    ) -> Result<String, PluginError> {
        unimplemented!("host::testing::life::LifeHost::connect_client_with_api")
    }

    /// Moves the manual clock.
    pub fn advance_ms(&mut self, _ms: u64) {
        unimplemented!("host::testing::life::LifeHost::advance_ms")
    }

    /// The wall clock.
    pub fn unix_ms(&self) -> u64 {
        unimplemented!("host::testing::life::LifeHost::unix_ms")
    }

    /// Injects a failure into one side instance.
    pub fn fail(&mut self, _plugin: &str, _side: Side, _scope: &str, _kind: FailureKind) {
        unimplemented!("host::testing::life::LifeHost::fail")
    }

    /// The state of one side instance.
    pub fn side(&self, _plugin: &str, _side: Side, _scope: &str) -> SideState {
        unimplemented!("host::testing::life::LifeHost::side")
    }

    /// The package state of a plugin.
    pub fn package_state(&self, _plugin: &str) -> PackageState {
        unimplemented!("host::testing::life::LifeHost::package_state")
    }

    /// Forces the package state of a plugin.
    pub fn force_package_state(&mut self, _plugin: &str, _state: PackageState) {
        unimplemented!("host::testing::life::LifeHost::force_package_state")
    }

    /// Every activation so far.
    pub fn activations(&self) -> Vec<ActivationRecord> {
        unimplemented!("host::testing::life::LifeHost::activations")
    }

    /// The deactivation reasons of a plugin's server side.
    pub fn deactivations(&self, _plugin: &str) -> Vec<DeactivationReason> {
        unimplemented!("host::testing::life::LifeHost::deactivations")
    }

    /// The deactivation reasons of a plugin on one client.
    pub fn client_deactivations(&self, _plugin: &str, _client: &str) -> Vec<DeactivationReason> {
        unimplemented!("host::testing::life::LifeHost::client_deactivations")
    }

    /// The deactivation reasons of a plugin across every side and scope.
    pub fn all_deactivations(&self, _plugin: &str) -> Vec<DeactivationReason> {
        unimplemented!("host::testing::life::LifeHost::all_deactivations")
    }

    /// Plugin ids in deactivation order.
    pub fn deactivation_order(&self) -> Vec<String> {
        unimplemented!("host::testing::life::LifeHost::deactivation_order")
    }

    /// Every state change so far.
    pub fn state_changes(&self) -> Vec<StateChange> {
        unimplemented!("host::testing::life::LifeHost::state_changes")
    }

    /// The availability changes a consumer saw.
    pub fn service_events(&self, _consumer: &str) -> Vec<AvailabilityChange> {
        unimplemented!("host::testing::life::LifeHost::service_events")
    }

    /// Calls a service as a consumer.
    pub fn call_service(&mut self, _consumer: &str, _interface: &str) -> Result<(), PluginError> {
        unimplemented!("host::testing::life::LifeHost::call_service")
    }

    /// `installed.json` parsed.
    pub fn installed_json(&self) -> serde_json::Value {
        unimplemented!("host::testing::life::LifeHost::installed_json")
    }

    /// Runs a CLI command.
    pub fn cli(&mut self, _args: &[&str]) -> CliOutput {
        unimplemented!("host::testing::life::LifeHost::cli")
    }

    /// The `plugins` import as the host sees it.
    pub fn plugins_api(&mut self) -> &mut dyn HostPlugins {
        unimplemented!("host::testing::life::LifeHost::plugins_api")
    }

    /// The lifecycle manager.
    pub fn manager(&mut self) -> &mut LifecycleManager {
        unimplemented!("host::testing::life::LifeHost::manager")
    }

    /// Resets the failure window of a plugin.
    pub fn reset_failures(&mut self, _plugin: &str) {
        unimplemented!("host::testing::life::LifeHost::reset_failures")
    }

    /// Updates a plugin.
    pub fn update(&mut self, _plugin: &str, _version: &str) {
        unimplemented!("host::testing::life::LifeHost::update")
    }

    /// Updates a plugin to a version whose trial fails.
    pub fn update_candidate_fails(&mut self, _plugin: &str, _version: &str) {
        unimplemented!("host::testing::life::LifeHost::update_candidate_fails")
    }

    /// Updates the server side only.
    pub fn update_server_only(&mut self, _plugin: &str, _version: &str) {
        unimplemented!("host::testing::life::LifeHost::update_server_only")
    }

    /// The version of a plugin's client side in a scope.
    pub fn client_version(&self, _plugin: &str, _scope: &str) -> String {
        unimplemented!("host::testing::life::LifeHost::client_version")
    }

    /// Sends a change notice to a plugin.
    pub fn send_change_notice(&mut self, _plugin: &str) {
        unimplemented!("host::testing::life::LifeHost::send_change_notice")
    }

    /// Sends a message between the sides of a plugin.
    pub fn message_between_sides(
        &mut self,
        _plugin: &str,
        _scope: &str,
    ) -> Result<(), PluginError> {
        unimplemented!("host::testing::life::LifeHost::message_between_sides")
    }

    /// Revokes a grant.
    pub fn revoke(&mut self, _plugin: &str, _grant: &str) {
        unimplemented!("host::testing::life::LifeHost::revoke")
    }

    /// Uninstalls a plugin.
    pub fn uninstall(&mut self, _plugin: &str) {
        unimplemented!("host::testing::life::LifeHost::uninstall")
    }

    /// Races a disable against an uninstall.
    pub fn race_disable_and_uninstall(&mut self, _plugin: &str) {
        unimplemented!("host::testing::life::LifeHost::race_disable_and_uninstall")
    }

    /// Begins a disable without finishing it.
    pub fn begin_disable(&mut self, _plugin: &str) -> PendingTransition {
        unimplemented!("host::testing::life::LifeHost::begin_disable")
    }

    /// The grants delivered to the last activation.
    pub fn granted_of_last_activation(&self, _plugin: &str) -> Vec<String> {
        unimplemented!("host::testing::life::LifeHost::granted_of_last_activation")
    }

    /// The active version of one side.
    pub fn active_version(&self, _plugin: &str, _side: Side) -> String {
        unimplemented!("host::testing::life::LifeHost::active_version")
    }

    /// Every captured log line.
    pub fn log(&self) -> Vec<LifeLogLine> {
        unimplemented!("host::testing::life::LifeHost::log")
    }

    /// Queued events of one side.
    pub fn queued_events(&self, _plugin: &str, _side: Side) -> usize {
        unimplemented!("host::testing::life::LifeHost::queued_events")
    }

    /// Dropped events of one side.
    pub fn dropped_events(&self, _plugin: &str, _side: Side) -> usize {
        unimplemented!("host::testing::life::LifeHost::dropped_events")
    }

    /// Delivers events to one side instance.
    pub fn deliver_event(&mut self, _plugin: &str, _side: Side, _scope: &str, _count: u32) {
        unimplemented!("host::testing::life::LifeHost::deliver_event")
    }

    /// Export calls made on one side.
    pub fn export_calls(&self, _plugin: &str, _side: Side) -> usize {
        unimplemented!("host::testing::life::LifeHost::export_calls")
    }

    /// Export calls made after the instance was dropped.
    pub fn export_calls_after_drop(&self, _plugin: &str) -> usize {
        unimplemented!("host::testing::life::LifeHost::export_calls_after_drop")
    }

    /// The essential-failure banner lines.
    pub fn banner(&self) -> Vec<String> {
        unimplemented!("host::testing::life::LifeHost::banner")
    }

    /// Records near misses of a plugin.
    pub fn near_miss(&mut self, _plugin: &str, _count: u32) {
        unimplemented!("host::testing::life::LifeHost::near_miss")
    }

    /// Fills the event queue of a plugin to a fraction.
    pub fn fill_event_queue(&mut self, _plugin: &str, _fraction_permille: u32) {
        unimplemented!("host::testing::life::LifeHost::fill_event_queue")
    }

    /// Rate-limits a plugin.
    pub fn rate_limited(&mut self, _plugin: &str) {
        unimplemented!("host::testing::life::LifeHost::rate_limited")
    }

    /// Makes a plugin exceed its quota.
    pub fn quota_exceeded(&mut self, _plugin: &str) {
        unimplemented!("host::testing::life::LifeHost::quota_exceeded")
    }

    /// The health of a plugin.
    pub fn health(&self, _plugin: &str) -> HealthState {
        unimplemented!("host::testing::life::LifeHost::health")
    }

    /// Runs a timed call.
    pub fn run_call(
        &mut self,
        _plugin: &str,
        _deadline_ms: u64,
        _work_ms: u64,
        _sync_host_ms: u64,
        _async_wait_ms: u64,
    ) -> CallResult {
        unimplemented!("host::testing::life::LifeHost::run_call")
    }

    /// Leaves requests of a plugin pending.
    pub fn pending_requests(&mut self, _plugin: &str, _count: u32) {
        unimplemented!("host::testing::life::LifeHost::pending_requests")
    }

    /// The outcomes delivered to a plugin.
    pub fn request_outcomes(&self, _plugin: &str) -> Vec<Outcome> {
        unimplemented!("host::testing::life::LifeHost::request_outcomes")
    }

    /// Sends a request that is never answered.
    pub fn request_never_answered(&mut self, _plugin: &str, _timeout_ms: u64) -> RequestId {
        unimplemented!("host::testing::life::LifeHost::request_never_answered")
    }

    /// Hangs the helper; returns the kill delay.
    pub fn helper_hang(&mut self, _missed_heartbeats: u32, _past_deadline_ms: u64) -> Option<u64> {
        unimplemented!("host::testing::life::LifeHost::helper_hang")
    }

    /// Crashes the helper while a plugin runs.
    pub fn helper_crash(&mut self, _running: &str, _hosted: &[&str]) {
        unimplemented!("host::testing::life::LifeHost::helper_crash")
    }

    /// Helper restarts so far.
    pub fn helper_restarts(&self) -> u32 {
        unimplemented!("host::testing::life::LifeHost::helper_restarts")
    }

    /// Metric buckets of one side.
    pub fn metric_buckets(&self, _plugin: &str, _side: Side) -> usize {
        unimplemented!("host::testing::life::LifeHost::metric_buckets")
    }

    /// The diagnostics report.
    pub fn diagnostics_report(&self) -> String {
        unimplemented!("host::testing::life::LifeHost::diagnostics_report")
    }

    /// Sets a user limit.
    pub fn set_user_limit(&mut self, _plugin: Option<&str>, _key: LimitKey, _value: u64) {
        unimplemented!("host::testing::life::LifeHost::set_user_limit")
    }

    /// The effective limit of a plugin and its source.
    pub fn effective_limit(&self, _plugin: &str, _key: LimitKey) -> (u64, LimitSource) {
        unimplemented!("host::testing::life::LifeHost::effective_limit")
    }

    /// The refusal of a drained CPU bucket.
    pub fn bucket_refusal(&mut self, _plugin: &str, _balance_ms: i64) -> PluginError {
        unimplemented!("host::testing::life::LifeHost::bucket_refusal")
    }

    /// Drains the CPU bucket of a plugin.
    pub fn drain_bucket(&mut self, _plugin: &str) {
        unimplemented!("host::testing::life::LifeHost::drain_bucket")
    }

    /// The CPU bucket balance of a plugin.
    pub fn bucket_balance_ms(&self, _plugin: &str) -> i64 {
        unimplemented!("host::testing::life::LifeHost::bucket_balance_ms")
    }

    /// Submits a payload of the given size.
    pub fn submit_payload(&mut self, _plugin: &str, _bytes: u64) {
        unimplemented!("host::testing::life::LifeHost::submit_payload")
    }

    /// Session pool memory in use.
    pub fn session_pool_used_mib(&self) -> u64 {
        unimplemented!("host::testing::life::LifeHost::session_pool_used_mib")
    }

    /// The cgroup memory maximum.
    pub fn cgroup_memory_max_mib(&self) -> u64 {
        unimplemented!("host::testing::life::LifeHost::cgroup_memory_max_mib")
    }

    /// The footer text.
    pub fn footer(&self) -> String {
        unimplemented!("host::testing::life::LifeHost::footer")
    }

    /// Plugin ids that run.
    pub fn running_plugins(&self) -> Vec<String> {
        unimplemented!("host::testing::life::LifeHost::running_plugins")
    }

    /// Triggers the activation of a lazy plugin.
    pub fn trigger_activation(&mut self, _plugin: &str) {
        unimplemented!("host::testing::life::LifeHost::trigger_activation")
    }

    /// Capacity retries of a plugin so far.
    pub fn capacity_retries(&self, _plugin: &str) -> u32 {
        unimplemented!("host::testing::life::LifeHost::capacity_retries")
    }

    /// Presses Enable on the recovery page.
    pub fn recovery_page_enable(&mut self, _plugin: &str) {
        unimplemented!("host::testing::life::LifeHost::recovery_page_enable")
    }

    /// Plugin ids in activation begin order.
    pub fn start_order(&self) -> Vec<String> {
        unimplemented!("host::testing::life::LifeHost::start_order")
    }

    /// Peak concurrent activations of a side.
    pub fn peak_concurrent_activations(&self, _side: Side) -> usize {
        unimplemented!("host::testing::life::LifeHost::peak_concurrent_activations")
    }

    /// Peak concurrent deactivations.
    pub fn peak_concurrent_deactivations(&self) -> usize {
        unimplemented!("host::testing::life::LifeHost::peak_concurrent_deactivations")
    }

    /// Runs a hook chain.
    pub fn call_hook(&mut self, _owner: &str, _point: &str) -> HookResult {
        unimplemented!("host::testing::life::LifeHost::call_hook")
    }

    /// Clock reading of a client's first frame.
    pub fn first_frame_ms(&self, _client: &str) -> u64 {
        unimplemented!("host::testing::life::LifeHost::first_frame_ms")
    }

    /// The marks drawn in a client's first frame.
    pub fn first_frame_marks(&self, _client: &str) -> Vec<String> {
        unimplemented!("host::testing::life::LifeHost::first_frame_marks")
    }

    /// Whether the essential plugins are active on a client.
    pub fn essential_active(&self, _client: &str) -> bool {
        unimplemented!("host::testing::life::LifeHost::essential_active")
    }

    /// Every file under the data and config directories.
    pub fn profile_snapshot(&self) -> Vec<(String, Vec<u8>)> {
        unimplemented!("host::testing::life::LifeHost::profile_snapshot")
    }
}

impl Default for LifeHost {
    fn default() -> LifeHost {
        LifeHost::new()
    }
}

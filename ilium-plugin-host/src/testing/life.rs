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

#[cfg(test)]
mod tests {
    use super::{FailureKind, LifeHost, LimitSource, PluginSpec, SafeMode};
    use crate::lifecycle::StartMode;
    use ilium_plugin_api::generated::host::plugins::{HealthState, InstanceState, PackageState};
    use ilium_plugin_api::lifecycle::DeactivationReason;
    use ilium_plugin_api::limits::LimitKey;
    use ilium_plugin_api::{Side, Tier};

    /// A third-party server-side plugin that starts at startup.
    fn server_plugin(plugin_id: &str) -> PluginSpec {
        PluginSpec::new(plugin_id)
            .tier(Tier::T3)
            .sides(&[Side::Server])
            .startup()
    }

    /// A third-party client-side plugin that starts at startup.
    fn client_plugin(plugin_id: &str) -> PluginSpec {
        PluginSpec::new(plugin_id)
            .tier(Tier::T3)
            .sides(&[Side::Client])
            .startup()
    }

    /// A third-party plugin with a server and a client side that starts at startup.
    fn server_and_client_plugin(plugin_id: &str) -> PluginSpec {
        PluginSpec::new(plugin_id)
            .tier(Tier::T3)
            .sides(&[Side::Server, Side::Client])
            .startup()
    }

    #[test]
    fn fresh_host_has_no_activations() {
        let host = LifeHost::new();
        assert!(host.activations().is_empty());
    }

    #[test]
    fn fresh_host_has_no_state_changes_log_or_running_plugins() {
        let host = LifeHost::new();
        assert!(host.state_changes().is_empty());
        assert!(host.log().is_empty());
        assert!(host.running_plugins().is_empty());
        assert!(host.start_order().is_empty());
        assert!(host.banner().is_empty());
    }

    #[test]
    fn default_host_is_equivalent_to_new() {
        let host = LifeHost::default();
        assert!(host.running_plugins().is_empty());
        assert_eq!(host.safe_mode(), SafeMode::Off);
    }

    #[test]
    fn peak_concurrency_is_zero_on_fresh_host() {
        let host = LifeHost::new();
        assert_eq!(host.peak_concurrent_activations(Side::Server), 0);
        assert_eq!(host.peak_concurrent_deactivations(), 0);
    }

    #[test]
    fn pool_without_sessions_reports_no_memory_in_use() {
        let host = LifeHost::new().with_pool_mib(64);
        assert_eq!(host.session_pool_used_mib(), 0);
    }

    #[test]
    fn try_install_accepts_a_valid_spec() {
        let mut host = LifeHost::new();
        assert!(host.try_install(server_plugin("core.alpha")).is_ok());
    }

    #[test]
    fn try_install_rejects_an_empty_plugin_id() {
        let mut host = LifeHost::new();
        assert!(host.try_install(PluginSpec::new("")).is_err());
    }

    #[test]
    fn try_install_manifest_key_rejects_an_empty_key() {
        let mut host = LifeHost::new();
        host.install(server_plugin("core.alpha"));
        let result = host.try_install_manifest_key("core.alpha", "", "\"1.0.0\"");
        assert!(result.is_err());
    }

    /// REQ: LIF-001
    #[test]
    fn installed_plugin_has_exactly_one_package_state() {
        let mut host = LifeHost::new();
        host.install(server_plugin("core.alpha"));
        assert_eq!(host.package_state("core.alpha"), PackageState::InstalledEnabled);
    }

    #[test]
    fn forced_package_state_is_reported_back() {
        let mut host = LifeHost::new();
        host.install(server_plugin("core.alpha"));
        host.force_package_state("core.alpha", PackageState::DisabledByPolicy);
        assert_eq!(host.package_state("core.alpha"), PackageState::DisabledByPolicy);
    }

    #[test]
    fn start_no_plugins_starts_nothing() {
        let mut host = LifeHost::new();
        host.install(server_plugin("core.alpha"));
        let report = host.start(StartMode::NoPlugins);
        assert!(report.started.is_empty());
        assert!(report.failed.is_empty());
        assert!(host.running_plugins().is_empty());
    }

    #[test]
    fn normal_start_reports_the_startup_plugin_as_started() {
        let mut host = LifeHost::new();
        host.install(server_plugin("core.alpha"));
        let report = host.start(StartMode::Normal);
        assert_eq!(report.started, vec!["core.alpha".to_string()]);
        assert!(report.failed.is_empty());
    }

    /// REQ: LIF-004
    #[test]
    fn safe_mode_is_not_written_to_installed_json() {
        let mut host = LifeHost::new();
        host.install(server_plugin("core.alpha"));
        host.start_safe(SafeMode::ThirdParty, None, None);
        assert_eq!(host.safe_mode(), SafeMode::ThirdParty);
        let installed = host.installed_json().to_string();
        assert!(!installed.contains("disabled-by-policy"));
    }

    #[test]
    fn activation_records_have_ordered_begin_and_end_readings() {
        let mut host = LifeHost::new();
        host.install(server_plugin("core.alpha"));
        host.start(StartMode::Normal);
        let records = host.activations();
        assert!(!records.is_empty());
        for record in &records {
            assert!(record.begin_ms <= record.end_ms);
            assert!(record.begin_seq < record.end_seq);
        }
    }

    /// REQ: LIF-014
    #[test]
    fn shutdown_sends_exactly_one_deactivate_per_live_server_instance() {
        let mut host = LifeHost::new();
        host.install(server_plugin("core.alpha"));
        host.start(StartMode::Normal);
        host.shutdown();
        assert_eq!(host.all_deactivations("core.alpha").len(), 1);
    }

    /// REQ: LIF-015
    #[test]
    fn clean_client_disconnect_sends_only_a_shutdown_deactivate() {
        let mut host = LifeHost::new();
        host.install(client_plugin("ui.gamma"));
        host.start(StartMode::Normal);
        let scope = host.attach_client("c1");
        assert!(!scope.is_empty());
        host.detach_client("c1");
        assert_eq!(
            host.client_deactivations("ui.gamma", "c1"),
            vec![DeactivationReason::Shutdown]
        );
        assert!(host.deactivations("ui.gamma").is_empty());
    }

    /// REQ: LIF-016
    #[test]
    fn state_changes_of_one_side_chain_from_each_previous_state() {
        let mut host = LifeHost::new();
        host.install(server_plugin("core.alpha"));
        host.start(StartMode::Normal);
        host.shutdown();
        let changes: Vec<_> = host
            .state_changes()
            .into_iter()
            .filter(|change| change.plugin == "core.alpha" && change.side == Side::Server)
            .collect();
        assert!(!changes.is_empty());
        for pair in changes.windows(2) {
            assert_eq!(pair[0].to, pair[1].from);
        }
    }

    /// REQ: LIF-016
    #[test]
    fn state_change_wall_clock_never_goes_backwards() {
        let mut host = LifeHost::new();
        host.install(server_plugin("core.alpha"));
        host.start(StartMode::Normal);
        host.shutdown();
        let times: Vec<u64> = host.state_changes().iter().map(|c| c.at_unix_ms).collect();
        for pair in times.windows(2) {
            assert!(pair[0] <= pair[1]);
        }
    }

    /// REQ: LIF-053
    #[test]
    fn consumer_sees_availability_loss_when_provider_is_uninstalled() {
        let mut host = LifeHost::new();
        host.install(
            PluginSpec::new("svc.provider")
                .tier(Tier::T3)
                .sides(&[Side::Server])
                .provides_service("svc.clock")
                .startup(),
        );
        host.install(
            PluginSpec::new("ui.consumer")
                .tier(Tier::T3)
                .sides(&[Side::Client])
                .requires_service("svc.clock")
                .startup(),
        );
        host.start(StartMode::Normal);
        host.uninstall("svc.provider");
        let events = host.service_events("ui.consumer");
        assert!(events.iter().any(|event| {
            event.interface == "svc.clock" && !event.available && event.provider == "svc.provider"
        }));
    }

    #[test]
    fn calling_an_unprovided_service_is_an_error() {
        let mut host = LifeHost::new();
        host.install(client_plugin("ui.consumer"));
        host.start(StartMode::Normal);
        assert!(host.call_service("ui.consumer", "svc.missing").is_err());
    }

    #[test]
    fn advancing_the_manual_clock_by_zero_leaves_the_wall_clock_unchanged() {
        let mut host = LifeHost::new();
        let before = host.unix_ms();
        host.advance_ms(0);
        assert_eq!(host.unix_ms(), before);
    }

    #[test]
    fn advancing_the_manual_clock_moves_the_wall_clock_by_the_given_amount() {
        let mut host = LifeHost::new();
        let before = host.unix_ms();
        host.advance_ms(1_500);
        assert_eq!(host.unix_ms(), before + 1_500);
    }

    /// REQ: LIF-002
    #[test]
    fn three_client_traps_stop_only_the_client_side() {
        let mut host = LifeHost::new();
        host.install(server_and_client_plugin("core.alpha"));
        host.start(StartMode::Normal);
        let scope = host.attach_client("c1");
        for _ in 0..3 {
            host.fail("core.alpha", Side::Client, &scope, FailureKind::Trap);
        }
        assert_eq!(
            host.side("core.alpha", Side::Client, &scope).state,
            InstanceState::StoppedByFailure
        );
        assert_eq!(
            host.side("core.alpha", Side::Server, "server").state,
            InstanceState::Active
        );
    }

    /// REQ: LIF-025
    #[test]
    fn two_traps_on_one_client_count_twice_in_its_window() {
        let mut host = LifeHost::new();
        host.install(server_and_client_plugin("core.alpha"));
        host.start(StartMode::Normal);
        let scope = host.attach_client("c1");
        host.fail("core.alpha", Side::Client, &scope, FailureKind::Trap);
        host.fail("core.alpha", Side::Client, &scope, FailureKind::Trap);
        assert_eq!(
            host.side("core.alpha", Side::Client, &scope).failures_in_window,
            2
        );
    }

    /// REQ: LIF-008
    #[test]
    fn server_failure_stop_is_written_to_installed_json() {
        let mut host = LifeHost::new();
        host.install(server_plugin("core.alpha"));
        host.start(StartMode::Normal);
        for _ in 0..3 {
            host.fail("core.alpha", Side::Server, "server", FailureKind::Trap);
        }
        assert!(host.installed_json().to_string().contains("failure_stopped"));
    }

    /// REQ: LIF-010
    #[test]
    fn failing_first_activation_of_a_candidate_keeps_the_previous_version() {
        let mut host = LifeHost::new();
        host.install(server_plugin("core.alpha"));
        host.start(StartMode::Normal);
        host.update_candidate_fails("core.alpha", "2.0.0");
        assert_ne!(host.active_version("core.alpha", Side::Server), "2.0.0");
        assert_ne!(host.package_state("core.alpha"), PackageState::UpdateStaged);
    }

    /// REQ: LIF-020
    #[test]
    fn a_begun_disable_is_not_finished_until_completed() {
        let mut host = LifeHost::new();
        host.install(server_plugin("core.alpha"));
        host.start(StartMode::Normal);
        let pending = host.begin_disable("core.alpha");
        assert!(!pending.finished());
    }

    /// REQ: LIF-020
    #[test]
    fn disable_racing_uninstall_settles_on_one_package_state() {
        let mut host = LifeHost::new();
        host.install(server_plugin("core.alpha"));
        host.start(StartMode::Normal);
        host.race_disable_and_uninstall("core.alpha");
        assert!(matches!(
            host.package_state("core.alpha"),
            PackageState::NotInstalled | PackageState::InstalledDisabledByUser
        ));
    }

    /// REQ: LIF-133
    #[test]
    fn helper_crash_counts_one_helper_restart() {
        let mut host = LifeHost::new();
        host.install(server_plugin("core.alpha"));
        host.start(StartMode::Normal);
        host.helper_crash("core.alpha", &[]);
        assert_eq!(host.helper_restarts(), 1);
    }

    #[test]
    fn fresh_host_has_no_helper_restarts() {
        let host = LifeHost::new();
        assert_eq!(host.helper_restarts(), 0);
    }

    /// REQ: LIF-143
    #[test]
    fn effective_limit_comes_from_the_tier_default_until_a_user_sets_one() {
        let mut host = LifeHost::new();
        host.install(server_plugin("core.alpha"));
        let (_, source) = host.effective_limit("core.alpha", LimitKey::MemoryMib);
        assert_eq!(source, LimitSource::Default);
        host.set_user_limit(Some("core.alpha"), LimitKey::MemoryMib, 32);
        assert_eq!(
            host.effective_limit("core.alpha", LimitKey::MemoryMib),
            (32, LimitSource::User)
        );
    }

    #[test]
    fn zero_work_call_is_not_trapped_and_takes_no_time() {
        let mut host = LifeHost::new();
        host.install(server_plugin("core.alpha"));
        host.start(StartMode::Normal);
        let result = host.run_call("core.alpha", 100, 0, 0, 0);
        assert!(!result.trapped);
        assert_eq!(result.elapsed_ms, 0);
    }

    #[test]
    fn hook_chain_with_no_handlers_passes_zero_times() {
        let mut host = LifeHost::new();
        let result = host.call_hook("core.alpha", "ilium.core.none/point");
        assert_eq!(result.passes, 0);
    }

    #[test]
    fn requests_of_an_unknown_plugin_have_no_outcomes() {
        let host = LifeHost::new();
        assert!(host.request_outcomes("core.alpha").is_empty());
    }

    #[test]
    fn delivering_zero_events_leaves_the_queue_and_drop_count_empty() {
        let mut host = LifeHost::new();
        host.install(server_plugin("core.alpha"));
        host.start(StartMode::Normal);
        host.deliver_event("core.alpha", Side::Server, "server", 0);
        assert_eq!(host.queued_events("core.alpha", Side::Server), 0);
        assert_eq!(host.dropped_events("core.alpha", Side::Server), 0);
    }

    #[test]
    fn drained_cpu_bucket_is_not_positive() {
        let mut host = LifeHost::new();
        host.install(server_plugin("core.alpha"));
        host.start(StartMode::Normal);
        host.drain_bucket("core.alpha");
        assert!(host.bucket_balance_ms("core.alpha") <= 0);
    }

    #[test]
    fn health_of_a_plugin_that_was_never_installed_is_not_healthy() {
        let host = LifeHost::new();
        assert_ne!(host.health("core.alpha"), HealthState::Healthy);
    }
}

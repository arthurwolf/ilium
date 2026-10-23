//! A multi-plugin boot harness: a kernel process (server, client, CLI) booting plugins through
//! the real lifecycle manager on a fake kernel (reference-host-architecture.md HST-00x..HST-045,
//! reference-kernel-and-boot.md COR-02x).

use std::time::Duration;

use ilium_plugin_api::error::Code;
use ilium_plugin_api::{PluginError, ProcessKind, Side, Tier};

use crate::crash_records::CrashRecord;
use crate::lifecycle::{ShutdownReport, StartMode, StartReport};
use crate::telemetry::LogLine;

pub use ilium_plugin_api::generated::types::ClientId;

/// A plugin the harness installs before boot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginSpec {
    id: String,
    tier: Tier,
    core: bool,
    sides: Vec<Side>,
    activation: Vec<String>,
    client_activation: Vec<String>,
    requires: Vec<String>,
    limits: Vec<(String, u64)>,
    default_grants: Vec<String>,
    stored_grants: Vec<String>,
    contributions: Vec<String>,
    subscriptions: Vec<String>,
    deactivate_takes_ms: u64,
    records_tree_on_activate: bool,
}

impl PluginSpec {
    /// A plugin of `id` at `tier`.
    pub fn new(_id: &str, _tier: Tier) -> PluginSpec {
        unimplemented!("host::testing::boot::PluginSpec::new")
    }

    /// Marks the plugin as a core plugin.
    pub fn core(self) -> PluginSpec {
        unimplemented!("host::testing::boot::PluginSpec::core")
    }

    /// The sides the plugin provides.
    pub fn sides(self, _sides: &[Side]) -> PluginSpec {
        unimplemented!("host::testing::boot::PluginSpec::sides")
    }

    /// The server-side activation events.
    pub fn activation(self, _events: &[&str]) -> PluginSpec {
        unimplemented!("host::testing::boot::PluginSpec::activation")
    }

    /// The client-side activation events.
    pub fn client_activation(self, _events: &[&str]) -> PluginSpec {
        unimplemented!("host::testing::boot::PluginSpec::client_activation")
    }

    /// The plugins this one requires.
    pub fn requires(self, _ids: &[&str]) -> PluginSpec {
        unimplemented!("host::testing::boot::PluginSpec::requires")
    }

    /// A manifest limit.
    pub fn limit(self, _key: &str, _value: u64) -> PluginSpec {
        unimplemented!("host::testing::boot::PluginSpec::limit")
    }

    /// The grants the manifest declares.
    pub fn default_grants(self, _grants: &[&str]) -> PluginSpec {
        unimplemented!("host::testing::boot::PluginSpec::default_grants")
    }

    /// The grants already in the grant store at boot.
    pub fn stored_grants(self, _grants: &[&str]) -> PluginSpec {
        unimplemented!("host::testing::boot::PluginSpec::stored_grants")
    }

    /// A contribution to an extension point.
    pub fn contributes(self, _point: &str) -> PluginSpec {
        unimplemented!("host::testing::boot::PluginSpec::contributes")
    }

    /// An event subscription.
    pub fn subscribes(self, _event: &str) -> PluginSpec {
        unimplemented!("host::testing::boot::PluginSpec::subscribes")
    }

    /// How long the plugin's deactivate export takes.
    pub fn deactivate_takes_ms(self, _ms: u64) -> PluginSpec {
        unimplemented!("host::testing::boot::PluginSpec::deactivate_takes_ms")
    }

    /// Makes the plugin record the tree size it sees on activate.
    pub fn records_tree_on_activate(self) -> PluginSpec {
        unimplemented!("host::testing::boot::PluginSpec::records_tree_on_activate")
    }
}

/// Builds a [`BootRun`] for one kernel process kind.
#[derive(Debug)]
pub struct BootHarness;

impl BootHarness {
    /// A harness for a kernel of `kind`.
    pub fn new(_kind: ProcessKind) -> BootHarness {
        unimplemented!("host::testing::boot::BootHarness::new")
    }

    /// The start mode.
    pub fn mode(self, _mode: StartMode) -> BootHarness {
        unimplemented!("host::testing::boot::BootHarness::mode")
    }

    /// Installs a plugin.
    pub fn plugin(self, _plugin: PluginSpec) -> BootHarness {
        unimplemented!("host::testing::boot::BootHarness::plugin")
    }

    /// A restored tree of `panes` panes present before activation.
    pub fn restored_tree(self, _panes: usize) -> BootHarness {
        unimplemented!("host::testing::boot::BootHarness::restored_tree")
    }

    /// Makes engine creation fail (HST-044).
    pub fn fail_engine_creation(self) -> BootHarness {
        unimplemented!("host::testing::boot::BootHarness::fail_engine_creation")
    }

    /// Corrupts the grant store before boot.
    pub fn corrupt_grant_store(self) -> BootHarness {
        unimplemented!("host::testing::boot::BootHarness::corrupt_grant_store")
    }

    /// Tampers with an installed package's files (HST-045).
    pub fn tamper_package(self, _id: &str) -> BootHarness {
        unimplemented!("host::testing::boot::BootHarness::tamper_package")
    }

    /// Installs every core plugin of the manual's activation table (COR-021).
    pub fn core_activation_table(self) -> BootHarness {
        unimplemented!("host::testing::boot::BootHarness::core_activation_table")
    }

    /// Installs one core plugin of the activation table.
    pub fn plugin_from_table(self, _id: &str) -> BootHarness {
        unimplemented!("host::testing::boot::BootHarness::plugin_from_table")
    }

    /// Pre-fills a plugin's store with one key.
    pub fn store_data(self, _id: &str, _key: &str, _value: &str) -> BootHarness {
        unimplemented!("host::testing::boot::BootHarness::store_data")
    }

    /// Builds the run.
    pub fn build(self) -> BootRun {
        unimplemented!("host::testing::boot::BootHarness::build")
    }
}

/// Live, queued and peak job counts of a plugin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct JobStats {
    /// Jobs running now.
    pub live: usize,
    /// Jobs waiting.
    pub queued: usize,
    /// The highest live count seen.
    pub peak: usize,
}

/// A CLI run through the harness.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliRun {
    /// The process that hosted the plugin.
    pub host_process: ProcessKind,
    /// The plugins activated for the run.
    pub activated: Vec<String>,
}

/// One side of one plugin as the harness reports it (the state word of the manual's tables).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SideState {
    /// `active`, `stopped-by-failure`, ...
    pub state: String,
    /// Failures in the current window.
    pub failures_in_window: u32,
}

/// A contribution as the registry holds it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Contribution {
    /// The contributing plugin.
    pub plugin: String,
    /// The extension point.
    pub point: String,
    /// Whether the contribution belongs to a plugin that is no longer active.
    pub stale: bool,
}

/// A row of the kernel's tree pane.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TreeRow {
    /// The row text.
    pub text: String,
    /// Whether the row was drawn from name and status only (no plugin decorator ran).
    pub uses_only_name_and_status: bool,
}

/// One composed client frame.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ComposedFrame {
    /// Time the composition took.
    pub elapsed: Duration,
    /// The scene frame used.
    pub scene_frame_id: u64,
    /// Whether the previous scene frame was reused because the new one was late.
    pub reused_previous_scene_frame: bool,
}

/// A booted kernel process with its plugins.
#[derive(Debug)]
pub struct BootRun;

impl BootRun {
    // ---- Lifecycle ------------------------------------------------------------------------

    /// Starts the plugins.
    pub fn start(&mut self) -> StartReport {
        unimplemented!("host::testing::boot::BootRun::start")
    }

    /// Shuts the plugins down.
    pub fn shutdown(&mut self) -> ShutdownReport {
        unimplemented!("host::testing::boot::BootRun::shutdown")
    }

    /// Starts the server up to, not including, accepting clients.
    pub fn begin_start_paused_before_accept(&mut self) {
        unimplemented!("host::testing::boot::BootRun::begin_start_paused_before_accept")
    }

    /// Holds the shutdown right before plugin deactivation.
    pub fn shutdown_hold_before_deactivate(&mut self) {
        unimplemented!("host::testing::boot::BootRun::shutdown_hold_before_deactivate")
    }

    /// Time the plugin part of the shutdown took.
    pub fn shutdown_plugin_time(&self) -> Duration {
        unimplemented!("host::testing::boot::BootRun::shutdown_plugin_time")
    }

    /// Restarts the server process and returns the new run.
    pub fn restart_server(self) -> BootRun {
        unimplemented!("host::testing::boot::BootRun::restart_server")
    }

    /// Restarts one plugin.
    pub fn restart_plugin(&mut self, _id: &str) {
        unimplemented!("host::testing::boot::BootRun::restart_plugin")
    }

    /// Advances the harness clock.
    pub fn advance(&mut self, _by: Duration) {
        unimplemented!("host::testing::boot::BootRun::advance")
    }

    /// Pumps the host for up to `budget` and returns the time it took.
    pub fn timed_pump(&mut self, _budget: Duration) -> Duration {
        unimplemented!("host::testing::boot::BootRun::timed_pump")
    }

    /// The trace of boot steps, in order.
    pub fn trace(&self) -> Vec<String> {
        unimplemented!("host::testing::boot::BootRun::trace")
    }

    /// The log lines emitted so far.
    pub fn logs(&self) -> Vec<LogLine> {
        unimplemented!("host::testing::boot::BootRun::logs")
    }

    /// The plugins activated, in order.
    pub fn activation_order(&self) -> Vec<String> {
        unimplemented!("host::testing::boot::BootRun::activation_order")
    }

    /// The plugins deactivated, in order.
    pub fn deactivation_order(&self) -> Vec<String> {
        unimplemented!("host::testing::boot::BootRun::deactivation_order")
    }

    /// How often `deactivate` was called on a plugin.
    pub fn deactivate_calls(&self, _id: &str) -> usize {
        unimplemented!("host::testing::boot::BootRun::deactivate_calls")
    }

    /// Whether the host started at all.
    pub fn host_started(&self) -> bool {
        unimplemented!("host::testing::boot::BootRun::host_started")
    }

    /// Whether the kernel process is still alive.
    pub fn kernel_alive(&self) -> bool {
        unimplemented!("host::testing::boot::BootRun::kernel_alive")
    }

    /// Whether the server process is still alive.
    pub fn server_alive(&self) -> bool {
        unimplemented!("host::testing::boot::BootRun::server_alive")
    }

    /// Whether the session snapshot was written.
    pub fn snapshot_written(&self) -> bool {
        unimplemented!("host::testing::boot::BootRun::snapshot_written")
    }

    /// Whether the session lock was taken.
    pub fn session_lock_taken(&self) -> bool {
        unimplemented!("host::testing::boot::BootRun::session_lock_taken")
    }

    /// Whether the server spawned another server process.
    pub fn servers_started(&self) -> usize {
        unimplemented!("host::testing::boot::BootRun::servers_started")
    }

    /// How often the server restarted.
    pub fn server_restarts(&self) -> u32 {
        unimplemented!("host::testing::boot::BootRun::server_restarts")
    }

    // ---- Plugin state ---------------------------------------------------------------------

    /// The state of one side of a plugin.
    pub fn side_state(&self, _id: &str, _side: Side) -> SideState {
        unimplemented!("host::testing::boot::BootRun::side_state")
    }

    /// The state of a plugin's client side in one client.
    pub fn client_side_state(&self, _client: ClientId, _id: &str) -> SideState {
        unimplemented!("host::testing::boot::BootRun::client_side_state")
    }

    /// How often a plugin was instantiated.
    pub fn instantiations(&self, _id: &str) -> u32 {
        unimplemented!("host::testing::boot::BootRun::instantiations")
    }

    /// How often a plugin was instantiated in the parent process.
    pub fn parent_instantiations(&self, _id: &str) -> u32 {
        unimplemented!("host::testing::boot::BootRun::parent_instantiations")
    }

    /// How often a plugin was instantiated in the helper process.
    pub fn helper_instantiations(&self, _id: &str) -> u32 {
        unimplemented!("host::testing::boot::BootRun::helper_instantiations")
    }

    /// Why a plugin is disabled, if it is.
    pub fn disabled_reason(&self, _id: &str) -> Option<String> {
        unimplemented!("host::testing::boot::BootRun::disabled_reason")
    }

    /// Disables a plugin.
    pub fn disable(&mut self, _id: &str) {
        unimplemented!("host::testing::boot::BootRun::disable")
    }

    /// Enables a plugin.
    pub fn enable(&mut self, _id: &str) {
        unimplemented!("host::testing::boot::BootRun::enable")
    }

    /// Disables every plugin.
    pub fn disable_all_plugins(&mut self) {
        unimplemented!("host::testing::boot::BootRun::disable_all_plugins")
    }

    /// Whether the off-effect row of a disabled core plugin holds.
    pub fn off_effect_holds(&self, _name: &str) -> bool {
        unimplemented!("host::testing::boot::BootRun::off_effect_holds")
    }

    /// The grants a plugin holds.
    pub fn granted(&self, _id: &str) -> Vec<String> {
        unimplemented!("host::testing::boot::BootRun::granted")
    }

    /// Revokes one grant of a plugin.
    pub fn revoke_grant(&mut self, _id: &str, _grant: &str) {
        unimplemented!("host::testing::boot::BootRun::revoke_grant")
    }

    /// The effective value of a plugin's limit.
    pub fn effective_limit(&self, _id: &str, _key: &str) -> u64 {
        unimplemented!("host::testing::boot::BootRun::effective_limit")
    }

    /// Changes a plugin's limit at runtime.
    pub fn change_limit(&mut self, _key: &str, _value: u64) {
        unimplemented!("host::testing::boot::BootRun::change_limit")
    }

    /// The events a plugin received.
    pub fn plugin_events(&self, _id: &str) -> Vec<String> {
        unimplemented!("host::testing::boot::BootRun::plugin_events")
    }

    /// The outcomes a plugin has not yet received.
    pub fn pending_outcomes(&self, _id: &str) -> Vec<String> {
        unimplemented!("host::testing::boot::BootRun::pending_outcomes")
    }

    /// Leaves `count` requests of a plugin pending.
    pub fn leave_requests_pending(&mut self, _id: &str, _count: usize) {
        unimplemented!("host::testing::boot::BootRun::leave_requests_pending")
    }

    /// Submits an intent from a plugin and returns the outcome word.
    pub fn submit_from(&mut self, _id: &str) -> String {
        unimplemented!("host::testing::boot::BootRun::submit_from")
    }

    /// Makes one side of a plugin trap.
    pub fn trap_in(&mut self, _id: &str, _side: Side) {
        unimplemented!("host::testing::boot::BootRun::trap_in")
    }

    /// Makes a plugin's client side trap in one client.
    pub fn trap_in_client(&mut self, _client: ClientId, _id: &str) {
        unimplemented!("host::testing::boot::BootRun::trap_in_client")
    }

    /// Makes one side of a plugin fail until the host disables it.
    pub fn fail_until_disabled(&mut self, _id: &str, _side: Side) {
        unimplemented!("host::testing::boot::BootRun::fail_until_disabled")
    }

    /// Makes a plugin's client side fail in one client until disabled there.
    pub fn fail_until_disabled_in_client(&mut self, _client: ClientId, _id: &str) {
        unimplemented!("host::testing::boot::BootRun::fail_until_disabled_in_client")
    }

    /// Fires an activation trigger.
    pub fn fire_trigger(&mut self, _trigger: &str) {
        unimplemented!("host::testing::boot::BootRun::fire_trigger")
    }

    /// Whether a kernel command still works.
    pub fn command_works(&mut self, _command: &str) -> bool {
        unimplemented!("host::testing::boot::BootRun::command_works")
    }

    /// A plugin tries to unregister a kernel command.
    pub fn plugin_tries_to_unregister_command(
        &mut self,
        _id: &str,
        _command: &str,
    ) -> Result<(), PluginError> {
        unimplemented!("host::testing::boot::BootRun::plugin_tries_to_unregister_command")
    }

    /// The queue depth of one side of a plugin.
    pub fn queue_depth(&self, _id: &str, _side: Side) -> usize {
        unimplemented!("host::testing::boot::BootRun::queue_depth")
    }

    /// The quota bucket level of one side of a plugin.
    pub fn quota_bucket(&self, _id: &str, _side: Side) -> i64 {
        unimplemented!("host::testing::boot::BootRun::quota_bucket")
    }

    /// The tree size a plugin saw on activate.
    pub fn tree_seen_on_activate(&self, _id: &str) -> usize {
        unimplemented!("host::testing::boot::BootRun::tree_seen_on_activate")
    }

    /// The tree-changed batches a plugin received.
    pub fn tree_changed_batches(&self, _id: &str) -> usize {
        unimplemented!("host::testing::boot::BootRun::tree_changed_batches")
    }

    /// The nodes across the tree-changed batches a plugin received.
    pub fn tree_changed_nodes(&self, _id: &str) -> usize {
        unimplemented!("host::testing::boot::BootRun::tree_changed_nodes")
    }

    /// The restore-step events a plugin received.
    pub fn restore_step_events(&self, _id: &str) -> usize {
        unimplemented!("host::testing::boot::BootRun::restore_step_events")
    }

    /// Delivers `nodes` nodes of tree traffic to a plugin.
    pub fn deliver_tree_traffic(&mut self, _id: &str, _nodes: usize) {
        unimplemented!("host::testing::boot::BootRun::deliver_tree_traffic")
    }

    /// Plugins subscribed to the journal.
    pub fn journal_plugin_subscribers(&self) -> usize {
        unimplemented!("host::testing::boot::BootRun::journal_plugin_subscribers")
    }

    /// The install-time file-hash check codes of a plugin (HST-045).
    pub fn verify_install_hashes(&mut self, _id: &str) -> Vec<String> {
        unimplemented!("host::testing::boot::BootRun::verify_install_hashes")
    }

    // ---- Processes, hosts and engines -----------------------------------------------------

    /// The process hosting one side of a plugin.
    pub fn host_process(&self, _id: &str, _side: Side) -> ProcessKind {
        unimplemented!("host::testing::boot::BootRun::host_process")
    }

    /// The process hosting one side of a plugin for one client.
    pub fn host_process_of_client(&self, _id: &str, _side: Side, _client: ClientId) -> ProcessKind {
        unimplemented!("host::testing::boot::BootRun::host_process_of_client")
    }

    /// How many hosts run one side of a plugin.
    pub fn hosts_of(&self, _id: &str, _side: Side) -> usize {
        unimplemented!("host::testing::boot::BootRun::hosts_of")
    }

    /// How many hosts the server runs.
    pub fn server_host_count(&self) -> usize {
        unimplemented!("host::testing::boot::BootRun::server_host_count")
    }

    /// How many helper processes run.
    pub fn helper_count(&self) -> usize {
        unimplemented!("host::testing::boot::BootRun::helper_count")
    }

    /// Kills the helper process.
    pub fn kill_helper(&mut self) {
        unimplemented!("host::testing::boot::BootRun::kill_helper")
    }

    /// The crash records the host wrote during this run (USR-227).
    pub fn crash_records(&self) -> Vec<CrashRecord> {
        unimplemented!("host::testing::boot::BootRun::crash_records")
    }

    /// Hangs the helper process.
    pub fn hang_helper(&mut self) {
        unimplemented!("host::testing::boot::BootRun::hang_helper")
    }

    /// How many engines exist.
    pub fn engine_count(&self) -> u32 {
        unimplemented!("host::testing::boot::BootRun::engine_count")
    }

    /// The id of the process's engine.
    pub fn engine_id(&self) -> u64 {
        unimplemented!("host::testing::boot::BootRun::engine_id")
    }

    /// The engine a store belongs to.
    pub fn engine_of_store(&self, _store: u64) -> u64 {
        unimplemented!("host::testing::boot::BootRun::engine_of_store")
    }

    /// The store ids of one side of a plugin.
    pub fn store_ids(&self, _id: &str, _side: Side) -> Vec<u64> {
        unimplemented!("host::testing::boot::BootRun::store_ids")
    }

    /// The store ids of a plugin's job instances.
    pub fn job_store_ids(&self, _id: &str) -> Vec<u64> {
        unimplemented!("host::testing::boot::BootRun::job_store_ids")
    }

    /// Starts `count` jobs of a plugin.
    pub fn start_jobs(&mut self, _id: &str, _count: usize) {
        unimplemented!("host::testing::boot::BootRun::start_jobs")
    }

    /// Completes one job of a plugin.
    pub fn complete_one_job(&mut self, _id: &str) {
        unimplemented!("host::testing::boot::BootRun::complete_one_job")
    }

    /// Live, queued and peak job counts of a plugin.
    pub fn job_stats(&self, _id: &str) -> JobStats {
        unimplemented!("host::testing::boot::BootRun::job_stats")
    }

    /// The memory base of job `index` of a plugin.
    pub fn job_memory_base(&self, _id: &str, _index: usize) -> u64 {
        unimplemented!("host::testing::boot::BootRun::job_memory_base")
    }

    /// The memory base of a plugin's main instance.
    pub fn main_memory_base(&self, _id: &str) -> u64 {
        unimplemented!("host::testing::boot::BootRun::main_memory_base")
    }

    /// The worker bank job `index` of a plugin runs on.
    pub fn job_worker_bank(&self, _id: &str, _index: usize) -> String {
        unimplemented!("host::testing::boot::BootRun::job_worker_bank")
    }

    /// The thread a plugin's scene frames render on.
    pub fn scene_frame_thread(&self, _id: &str) -> u64 {
        unimplemented!("host::testing::boot::BootRun::scene_frame_thread")
    }

    /// Starts a plugin's scene instance.
    pub fn start_scene_instance(&mut self, _id: &str) {
        unimplemented!("host::testing::boot::BootRun::start_scene_instance")
    }

    /// Delays every scene frame by `delay`.
    pub fn set_scene_frame_delay(&mut self, _delay: Duration) {
        unimplemented!("host::testing::boot::BootRun::set_scene_frame_delay")
    }

    /// The client's UI thread.
    pub fn client_ui_thread(&self) -> u64 {
        unimplemented!("host::testing::boot::BootRun::client_ui_thread")
    }

    /// The client's executor threads.
    pub fn client_executor_threads(&self) -> Vec<u64> {
        unimplemented!("host::testing::boot::BootRun::client_executor_threads")
    }

    /// Composes one client frame.
    pub fn compose_client_frame(&mut self, _client: ClientId) -> ComposedFrame {
        unimplemented!("host::testing::boot::BootRun::compose_client_frame")
    }

    /// Runs a CLI command of a plugin.
    pub fn run_cli(&mut self, _id: &str, _command: &str) -> CliRun {
        unimplemented!("host::testing::boot::BootRun::run_cli")
    }

    /// Ends the CLI process.
    pub fn run_cli_exit(&mut self) {
        unimplemented!("host::testing::boot::BootRun::run_cli_exit")
    }

    /// A CLI process calls `tree.import` on a plugin.
    pub fn cli_call_tree_import(&mut self, _id: &str) -> Result<(), PluginError> {
        unimplemented!("host::testing::boot::BootRun::cli_call_tree_import")
    }

    // ---- Clients --------------------------------------------------------------------------

    /// Attaches a client.
    pub fn attach_client(&mut self) -> Result<ClientId, Code> {
        unimplemented!("host::testing::boot::BootRun::attach_client")
    }

    /// Connects a client, retrying for `budget` with `interval` between tries.
    pub fn client_connect_with_retry(
        &mut self,
        _budget: Duration,
        _interval: Duration,
    ) -> Result<ClientId, Code> {
        unimplemented!("host::testing::boot::BootRun::client_connect_with_retry")
    }

    /// A client exits.
    pub fn client_exit(&mut self, _client: ClientId) {
        unimplemented!("host::testing::boot::BootRun::client_exit")
    }

    /// Shuts a client down and returns how long it took.
    pub fn client_shutdown(&mut self, _client: ClientId) -> Duration {
        unimplemented!("host::testing::boot::BootRun::client_shutdown")
    }

    /// The events the client-side plugins of a client saw.
    pub fn events_seen_by_client_plugins(&self, _client: ClientId) -> Vec<String> {
        unimplemented!("host::testing::boot::BootRun::events_seen_by_client_plugins")
    }

    /// The notices shown to a client.
    pub fn notices_for_client(&self, _client: ClientId) -> Vec<String> {
        unimplemented!("host::testing::boot::BootRun::notices_for_client")
    }

    /// Delivers a pending outcome to a client; true when one was delivered.
    pub fn deliver_outcome_to_client(&mut self, _client: ClientId) -> bool {
        unimplemented!("host::testing::boot::BootRun::deliver_outcome_to_client")
    }

    /// How often the recovery page opened in a client.
    pub fn recovery_page_opens(&self, _client: ClientId) -> u32 {
        unimplemented!("host::testing::boot::BootRun::recovery_page_opens")
    }

    /// Whether the recovery page renders without plugins.
    pub fn recovery_page_renders(&self) -> bool {
        unimplemented!("host::testing::boot::BootRun::recovery_page_renders")
    }

    /// The contributions a client's replica holds.
    pub fn replica_contributions(&self, _client: ClientId) -> Vec<Contribution> {
        unimplemented!("host::testing::boot::BootRun::replica_contributions")
    }

    /// The IPC message kind a client's replica was built from.
    pub fn replica_source(&self, _client: ClientId) -> String {
        unimplemented!("host::testing::boot::BootRun::replica_source")
    }

    // ---- Contribution registry -----------------------------------------------------------

    /// The contributions of the registry.
    pub fn contributions(&self) -> Vec<Contribution> {
        unimplemented!("host::testing::boot::BootRun::contributions")
    }

    /// The process owning the contribution registry.
    pub fn contribution_registry_owner(&self) -> ProcessKind {
        unimplemented!("host::testing::boot::BootRun::contribution_registry_owner")
    }

    /// A client-side plugin registers an extension point.
    pub fn client_registers_point(&mut self, _point: &str) {
        unimplemented!("host::testing::boot::BootRun::client_registers_point")
    }

    /// A client-side plugin registers a service.
    pub fn client_registers_service(&mut self, _service: &str) {
        unimplemented!("host::testing::boot::BootRun::client_registers_service")
    }

    /// A server-side plugin registers an extension point.
    pub fn server_registers_point(&mut self, _point: &str) {
        unimplemented!("host::testing::boot::BootRun::server_registers_point")
    }

    /// Whether the server registry holds a point.
    pub fn server_sees_point(&self, _point: &str) -> bool {
        unimplemented!("host::testing::boot::BootRun::server_sees_point")
    }

    /// Whether the server registry holds a service.
    pub fn server_sees_service(&self, _service: &str) -> bool {
        unimplemented!("host::testing::boot::BootRun::server_sees_service")
    }

    // ---- Kernel probes --------------------------------------------------------------------

    /// Opens a pane on the fake kernel.
    pub fn kernel_new_pane(&mut self) -> u64 {
        unimplemented!("host::testing::boot::BootRun::kernel_new_pane")
    }

    /// Types into a kernel pane.
    pub fn kernel_type(&mut self, _pane: u64, _text: &str) {
        unimplemented!("host::testing::boot::BootRun::kernel_type")
    }

    /// The screen of a kernel pane.
    pub fn kernel_screen(&self, _pane: u64) -> String {
        unimplemented!("host::testing::boot::BootRun::kernel_screen")
    }

    /// The rows of the kernel's tree pane.
    pub fn kernel_tree_rows(&self) -> Vec<TreeRow> {
        unimplemented!("host::testing::boot::BootRun::kernel_tree_rows")
    }

    /// Kernel locks held right now.
    pub fn kernel_locks_held(&self) -> u32 {
        unimplemented!("host::testing::boot::BootRun::kernel_locks_held")
    }

    /// The most kernel locks held during any guest call.
    pub fn max_kernel_locks_held_during_guest_calls(&self) -> u32 {
        unimplemented!("host::testing::boot::BootRun::max_kernel_locks_held_during_guest_calls")
    }

    /// Changes a kernel setting.
    pub fn change_kernel_setting(&mut self, _key: &str, _value: &str) {
        unimplemented!("host::testing::boot::BootRun::change_kernel_setting")
    }

    /// Changes a plugin setting.
    pub fn change_plugin_setting(&mut self, _id: &str, _key: &str, _value: &str) {
        unimplemented!("host::testing::boot::BootRun::change_plugin_setting")
    }

    /// Changes a key binding.
    pub fn change_key_binding(&mut self, _key: &str, _action: &str) {
        unimplemented!("host::testing::boot::BootRun::change_key_binding")
    }

    /// Changes the HTTP API port.
    pub fn change_http_port(&mut self, _port: u16) {
        unimplemented!("host::testing::boot::BootRun::change_http_port")
    }

    /// The port the HTTP API listens on.
    pub fn live_http_port(&self) -> u16 {
        unimplemented!("host::testing::boot::BootRun::live_http_port")
    }

    /// Whether the kernel, not a plugin, owns the HTTP listener.
    pub fn http_listener_owner_is_kernel(&self) -> bool {
        unimplemented!("host::testing::boot::BootRun::http_listener_owner_is_kernel")
    }

    /// The status of an HTTP API request.
    pub fn http_status(&mut self, _method: &str, _path: &str) -> u16 {
        unimplemented!("host::testing::boot::BootRun::http_status")
    }

    /// A value in a plugin's store.
    pub fn stored_data(&self, _id: &str, _key: &str) -> Option<String> {
        unimplemented!("host::testing::boot::BootRun::stored_data")
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use ilium_plugin_api::{ProcessKind, Side, Tier};

    use super::{BootHarness, BootRun, ClientId, JobStats, PluginSpec};
    use crate::lifecycle::StartMode;

    /// A T1 plugin that activates at boot through the `startup` trigger.
    fn startup_plugin(id: &str) -> PluginSpec {
        PluginSpec::new(id, Tier::T1).activation(&["startup"])
    }

    /// A server-kernel harness with the given plugins, built but not started.
    fn server_run(plugins: Vec<PluginSpec>) -> BootRun {
        let mut harness = BootHarness::new(ProcessKind::Server);
        for plugin in plugins {
            harness = harness.plugin(plugin);
        }
        harness.build()
    }

    /// Attaches a client and fails the test with the refusal code when the server refuses it.
    fn attached_client(run: &mut BootRun) -> ClientId {
        match run.attach_client() {
            Ok(client) => client,
            Err(code) => panic!("client was refused: {}", code.as_str()),
        }
    }

    #[test]
    fn empty_harness_starts_with_no_plugins() {
        let mut run = server_run(Vec::new());
        let report = run.start();
        assert!(report.started.is_empty());
        assert!(report.failed.is_empty());
        assert!(run.activation_order().is_empty());
    }

    #[test]
    fn plugin_spec_with_no_sides_equals_plain_spec() {
        let plain = PluginSpec::new("ilium.core.tree", Tier::T1);
        let with_empty_sides = PluginSpec::new("ilium.core.tree", Tier::T1).sides(&[]);
        assert_eq!(plain, with_empty_sides);
    }

    /// REQ: COR-021
    #[test]
    fn only_startup_plugins_activate_at_boot() {
        let mut run = server_run(vec![
            startup_plugin("ilium.core.tree"),
            PluginSpec::new("ilium.core.search", Tier::T1).activation(&["command:search.open"]),
        ]);
        let report = run.start();
        assert_eq!(report.started, vec!["ilium.core.tree".to_string()]);
        assert_eq!(run.activation_order(), vec!["ilium.core.tree".to_string()]);
        run.fire_trigger("command:search.open");
        assert!(run
            .activation_order()
            .contains(&"ilium.core.search".to_string()));
    }

    /// REQ: HST-027
    #[test]
    fn non_startup_plugin_is_not_instantiated_at_boot() {
        let mut run = server_run(vec![
            PluginSpec::new("ilium.core.search", Tier::T1).activation(&["command:search.open"]),
            startup_plugin("ilium.core.tree"),
        ]);
        run.start();
        assert_eq!(run.instantiations("ilium.core.search"), 0);
        assert_eq!(run.instantiations("ilium.core.tree"), 1);
    }

    /// REQ: COR-020
    #[test]
    fn restored_tree_is_visible_at_first_activation() {
        let mut run = BootHarness::new(ProcessKind::Server)
            .restored_tree(3)
            .plugin(startup_plugin("ilium.core.tree").records_tree_on_activate())
            .build();
        run.start();
        assert_eq!(run.tree_seen_on_activate("ilium.core.tree"), 3);
    }

    /// REQ: HST-026
    #[test]
    fn restored_tree_delivers_one_batch_and_no_restore_replay() {
        let mut run = BootHarness::new(ProcessKind::Server)
            .restored_tree(3)
            .plugin(startup_plugin("ilium.core.tree").subscribes("tree-changed"))
            .build();
        run.start();
        assert_eq!(run.tree_changed_batches("ilium.core.tree"), 1);
        assert_eq!(run.restore_step_events("ilium.core.tree"), 0);
    }

    /// REQ: HST-028
    #[test]
    fn service_provider_activates_before_its_consumer() {
        let mut run = server_run(vec![
            startup_plugin("consumer.plugin").requires(&["provider.plugin"]),
            startup_plugin("provider.plugin"),
        ]);
        run.start();
        assert_eq!(
            run.activation_order(),
            vec!["provider.plugin".to_string(), "consumer.plugin".to_string()]
        );
    }

    /// REQ: HST-028
    #[test]
    fn service_cycle_leaves_cycle_plugins_disabled() {
        let mut run = server_run(vec![
            startup_plugin("cycle.a").requires(&["cycle.b"]),
            startup_plugin("cycle.b").requires(&["cycle.a"]),
        ]);
        let report = run.start();
        assert!(report.started.is_empty());
        assert_eq!(report.failed.len(), 2);
        assert!(report
            .failed
            .iter()
            .all(|(_, code)| code.as_str() == "manifest.service-cycle"));
        assert!(run.disabled_reason("cycle.a").is_some());
    }

    /// REQ: HST-030
    #[test]
    fn shutdown_deactivates_in_reverse_activation_order() {
        let mut run = server_run(vec![
            startup_plugin("consumer.plugin").requires(&["provider.plugin"]),
            startup_plugin("provider.plugin"),
        ]);
        run.start();
        let report = run.shutdown();
        let mut expected_order = run.activation_order();
        expected_order.reverse();
        assert_eq!(run.deactivation_order(), expected_order);
        assert!(report.forced.is_empty());
    }

    /// REQ: HST-030
    #[test]
    fn instant_deactivate_is_not_forced_and_called_once() {
        let mut run = server_run(vec![
            startup_plugin("ilium.core.tree").deactivate_takes_ms(0)
        ]);
        run.start();
        let report = run.shutdown();
        assert!(report.forced.is_empty());
        assert_eq!(run.deactivate_calls("ilium.core.tree"), 1);
    }

    /// REQ: HST-030
    #[test]
    fn slow_deactivate_is_forced_past_the_five_second_budget() {
        let mut run = server_run(vec![
            startup_plugin("slow.plugin").deactivate_takes_ms(10_000)
        ]);
        run.start();
        let report = run.shutdown();
        assert!(report.forced.contains(&"slow.plugin".to_string()));
        assert!(run.shutdown_plugin_time() <= Duration::from_secs(5));
    }

    /// REQ: HST-029
    #[test]
    fn submissions_are_refused_as_cancelled_before_first_deactivate() {
        let mut run = server_run(vec![startup_plugin("ilium.core.tree")]);
        run.start();
        run.shutdown_hold_before_deactivate();
        assert_eq!(run.submit_from("ilium.core.tree"), "cancelled");
        assert_eq!(run.deactivate_calls("ilium.core.tree"), 0);
    }

    /// REQ: HST-031
    #[test]
    fn client_connecting_before_server_ready_is_refused_as_starting() {
        let mut run = server_run(vec![startup_plugin("ilium.core.tree")]);
        run.begin_start_paused_before_accept();
        let code = match run.attach_client() {
            Ok(_) => panic!("client was accepted before the server host finished step 6"),
            Err(code) => code,
        };
        assert_eq!(code.as_str(), "server.starting");
    }

    /// REQ: HST-031
    #[test]
    fn client_connecting_after_start_is_accepted() {
        let mut run = server_run(vec![startup_plugin("ilium.core.tree")]);
        run.start();
        assert!(run.attach_client().is_ok());
    }

    /// REQ: HST-032
    #[test]
    fn client_shutdown_is_bounded_at_two_seconds() {
        let mut run = server_run(vec![startup_plugin("slow.plugin")
            .sides(&[Side::Server, Side::Client])
            .client_activation(&["startup"])
            .deactivate_takes_ms(10_000)]);
        run.start();
        let client = attached_client(&mut run);
        let took = run.client_shutdown(client);
        assert!(took <= Duration::from_secs(2));
    }

    /// REQ: HST-010
    #[test]
    fn client_exit_leaves_server_and_its_plugin_running() {
        let mut run = server_run(vec![startup_plugin("ilium.core.tree")]);
        run.start();
        let client = attached_client(&mut run);
        run.client_exit(client);
        assert!(run.server_alive());
        assert_eq!(
            run.side_state("ilium.core.tree", Side::Server).state,
            "active"
        );
    }

    /// REQ: HST-007
    #[test]
    fn each_side_is_hosted_by_exactly_one_process() {
        let mut run = server_run(vec![startup_plugin("ilium.core.tree")
            .sides(&[Side::Server, Side::Client])
            .client_activation(&["startup"])]);
        run.start();
        let client = attached_client(&mut run);
        assert_eq!(run.hosts_of("ilium.core.tree", Side::Server), 1);
        assert_eq!(
            run.host_process("ilium.core.tree", Side::Server),
            ProcessKind::Server
        );
        assert_eq!(
            run.host_process_of_client("ilium.core.tree", Side::Client, client),
            ProcessKind::Client
        );
    }

    /// REQ: HST-011
    #[test]
    fn session_has_exactly_one_server_host() {
        let mut run = server_run(vec![startup_plugin("ilium.core.tree")]);
        run.start();
        assert_eq!(run.server_host_count(), 1);
    }

    /// REQ: HST-008
    #[test]
    fn t1_plugin_runs_in_process_without_helper() {
        let mut run = server_run(vec![startup_plugin("ilium.core.tree")]);
        run.start();
        assert_eq!(
            run.host_process("ilium.core.tree", Side::Server),
            ProcessKind::Server
        );
        assert_eq!(run.helper_count(), 0);
    }

    /// REQ: HST-009
    #[test]
    fn t2_plugin_runs_in_helper_never_in_parent_engine() {
        let mut run = BootHarness::new(ProcessKind::Server)
            .plugin(PluginSpec::new("vendor.sandboxed", Tier::T2).activation(&["startup"]))
            .build();
        run.start();
        assert_eq!(
            run.host_process("vendor.sandboxed", Side::Server),
            ProcessKind::Helper
        );
        assert_eq!(run.parent_instantiations("vendor.sandboxed"), 0);
        assert_eq!(run.helper_count(), 1);
    }

    /// REQ: HST-021
    #[test]
    fn one_engine_serves_every_store_of_the_process() {
        let mut run = server_run(vec![
            startup_plugin("ilium.core.tree").limit("concurrent_jobs", 4)
        ]);
        run.start();
        run.start_jobs("ilium.core.tree", 2);
        assert_eq!(run.engine_count(), 1);
        let store_ids = run.store_ids("ilium.core.tree", Side::Server);
        assert!(!store_ids.is_empty());
        for store_id in store_ids {
            assert_eq!(run.engine_of_store(store_id), run.engine_id());
        }
    }

    /// REQ: HST-022
    #[test]
    fn each_job_instance_has_its_own_store() {
        let mut run = server_run(vec![
            startup_plugin("ilium.core.tree").limit("concurrent_jobs", 8)
        ]);
        run.start();
        run.start_jobs("ilium.core.tree", 3);
        let mut job_store_ids = run.job_store_ids("ilium.core.tree");
        assert_eq!(job_store_ids.len(), 3);
        job_store_ids.sort_unstable();
        job_store_ids.dedup();
        assert_eq!(job_store_ids.len(), 3);
    }

    /// REQ: HST-017
    #[test]
    fn job_memory_is_separate_from_main_instance_memory() {
        let mut run = server_run(vec![
            startup_plugin("ilium.core.tree").limit("concurrent_jobs", 2)
        ]);
        run.start();
        run.start_jobs("ilium.core.tree", 1);
        assert_ne!(
            run.job_memory_base("ilium.core.tree", 0),
            run.main_memory_base("ilium.core.tree")
        );
    }

    /// REQ: HST-018
    #[test]
    fn jobs_beyond_concurrent_limit_wait_in_queue() {
        let mut run = server_run(vec![
            startup_plugin("ilium.core.search").limit("concurrent_jobs", 2)
        ]);
        run.start();
        run.start_jobs("ilium.core.search", 5);
        assert_eq!(
            run.job_stats("ilium.core.search"),
            JobStats {
                live: 2,
                queued: 3,
                peak: 2
            }
        );
    }

    /// REQ: HST-018
    #[test]
    fn jobs_exactly_at_concurrent_limit_do_not_queue() {
        let mut run = server_run(vec![
            startup_plugin("ilium.core.search").limit("concurrent_jobs", 2)
        ]);
        run.start();
        run.start_jobs("ilium.core.search", 2);
        assert_eq!(
            run.job_stats("ilium.core.search"),
            JobStats {
                live: 2,
                queued: 0,
                peak: 2
            }
        );
    }

    /// REQ: HST-018
    #[test]
    fn zero_started_jobs_leave_stats_at_default() {
        let mut run = server_run(vec![
            startup_plugin("ilium.core.search").limit("concurrent_jobs", 2)
        ]);
        run.start();
        run.start_jobs("ilium.core.search", 0);
        assert_eq!(run.job_stats("ilium.core.search"), JobStats::default());
    }

    /// REQ: HST-016
    #[test]
    fn late_scene_frame_is_replaced_by_previous_complete_frame() {
        let mut run = BootHarness::new(ProcessKind::Client)
            .plugin(
                startup_plugin("ilium.core.ambient")
                    .sides(&[Side::Client, Side::Scene])
                    .client_activation(&["startup"]),
            )
            .build();
        run.start();
        let client = attached_client(&mut run);
        run.start_scene_instance("ilium.core.ambient");
        let first = run.compose_client_frame(client);
        run.set_scene_frame_delay(Duration::from_millis(500));
        let second = run.compose_client_frame(client);
        assert!(second.reused_previous_scene_frame);
        assert_eq!(second.scene_frame_id, first.scene_frame_id);
    }

    /// REQ: HST-015
    #[test]
    fn scene_frames_render_off_client_ui_and_executor_threads() {
        let mut run = BootHarness::new(ProcessKind::Client)
            .plugin(
                startup_plugin("ilium.core.ambient")
                    .sides(&[Side::Client, Side::Scene])
                    .client_activation(&["startup"]),
            )
            .build();
        run.start();
        let _client = attached_client(&mut run);
        run.start_scene_instance("ilium.core.ambient");
        let scene_thread = run.scene_frame_thread("ilium.core.ambient");
        assert_ne!(scene_thread, run.client_ui_thread());
        assert!(!run.client_executor_threads().contains(&scene_thread));
    }

    /// REQ: HST-013
    #[test]
    fn client_failure_leaves_other_clients_side_active() {
        let mut run = server_run(vec![startup_plugin("ilium.core.tree")
            .sides(&[Side::Server, Side::Client])
            .client_activation(&["startup"])]);
        run.start();
        let first = attached_client(&mut run);
        let second = attached_client(&mut run);
        run.trap_in_client(first, "ilium.core.tree");
        assert!(
            run.client_side_state(first, "ilium.core.tree")
                .failures_in_window
                >= 1
        );
        let other = run.client_side_state(second, "ilium.core.tree");
        assert_eq!(other.state, "active");
        assert_eq!(other.failures_in_window, 0);
    }

    /// REQ: HST-035
    #[test]
    fn guest_trap_ends_only_the_trapped_plugin_instance() {
        let mut run = server_run(vec![
            startup_plugin("ilium.core.tree"),
            startup_plugin("ilium.core.search"),
        ]);
        run.start();
        run.trap_in("ilium.core.tree", Side::Server);
        assert_ne!(
            run.side_state("ilium.core.tree", Side::Server).state,
            "active"
        );
        assert_eq!(
            run.side_state("ilium.core.search", Side::Server).state,
            "active"
        );
    }

    /// REQ: HST-038
    #[test]
    fn no_kernel_lock_is_held_during_guest_calls() {
        let mut run = server_run(vec![startup_plugin("ilium.core.tree")]);
        run.start();
        assert_eq!(run.kernel_locks_held(), 0);
        assert_eq!(run.max_kernel_locks_held_during_guest_calls(), 0);
    }

    /// REQ: HST-039
    #[test]
    fn killed_helper_leaves_parent_running() {
        let mut run = BootHarness::new(ProcessKind::Server)
            .plugin(PluginSpec::new("vendor.sandboxed", Tier::T2).activation(&["startup"]))
            .build();
        run.start();
        run.kill_helper();
        assert!(run.kernel_alive());
        assert!(run.server_alive());
        assert!(run.host_started());
    }

    /// REQ: HST-039
    #[test]
    fn hung_helper_does_not_hang_parent() {
        let mut run = BootHarness::new(ProcessKind::Server)
            .plugin(PluginSpec::new("vendor.sandboxed", Tier::T2).activation(&["startup"]))
            .build();
        run.start();
        run.hang_helper();
        let took = run.timed_pump(Duration::from_millis(100));
        assert!(took < Duration::from_secs(1));
        assert!(run.kernel_alive());
    }

    /// REQ: HST-025
    #[test]
    fn client_point_is_not_visible_to_server_plugins() {
        let mut run = server_run(vec![startup_plugin("ilium.core.tree")]);
        run.start();
        run.client_registers_point("ilium.point.client");
        assert!(!run.server_sees_point("ilium.point.client"));
        run.server_registers_point("ilium.point.server");
        assert!(run.server_sees_point("ilium.point.server"));
    }

    /// REQ: HST-019
    #[test]
    fn cli_host_activates_only_the_named_plugin() {
        let mut run = BootHarness::new(ProcessKind::Cli)
            .plugin(PluginSpec::new("ilium.core.tree", Tier::T1).core())
            .plugin(PluginSpec::new("ilium.core.search", Tier::T1))
            .build();
        let cli = run.run_cli("ilium.core.search", "search");
        assert_eq!(cli.activated, vec!["ilium.core.search".to_string()]);
        assert_eq!(cli.host_process, ProcessKind::Cli);
    }

    /// REQ: HST-020
    #[test]
    fn cli_session_import_is_refused_without_starting_server() {
        let mut run = BootHarness::new(ProcessKind::Cli)
            .plugin(PluginSpec::new("ilium.core.tree", Tier::T1))
            .build();
        run.run_cli("ilium.core.tree", "tree");
        assert!(run.cli_call_tree_import("ilium.core.tree").is_err());
        assert_eq!(run.servers_started(), 0);
    }

    /// REQ: HST-033
    #[test]
    fn safe_plugins_start_skips_t2_packages() {
        let mut run = BootHarness::new(ProcessKind::Server)
            .mode(StartMode::SafePlugins)
            .plugin(startup_plugin("ilium.core.tree").core())
            .plugin(PluginSpec::new("vendor.sandboxed", Tier::T2).activation(&["startup"]))
            .build();
        let report = run.start();
        assert_eq!(report.started, vec!["ilium.core.tree".to_string()]);
        assert!(!report.started.contains(&"vendor.sandboxed".to_string()));
        assert_eq!(run.helper_count(), 0);
    }

    /// REQ: HST-033
    #[test]
    fn no_plugins_mode_starts_no_plugin_host() {
        let mut run = BootHarness::new(ProcessKind::Server)
            .mode(StartMode::NoPlugins)
            .plugin(startup_plugin("ilium.core.tree"))
            .build();
        run.start();
        assert!(!run.host_started());
        assert!(run.kernel_alive());
        assert!(run.activation_order().is_empty());
    }

    /// REQ: HST-034
    #[test]
    fn no_plugins_mode_boots_to_usable_recovery_page() {
        let mut run = BootHarness::new(ProcessKind::Server)
            .mode(StartMode::NoPlugins)
            .plugin(startup_plugin("ilium.core.tree"))
            .build();
        run.start();
        assert!(run.recovery_page_renders());
    }

    /// REQ: HST-037
    #[test]
    fn engine_failure_continues_without_plugin_host() {
        let mut run = BootHarness::new(ProcessKind::Server)
            .plugin(startup_plugin("ilium.core.tree"))
            .fail_engine_creation()
            .build();
        run.start();
        assert!(!run.host_started());
        assert!(run.kernel_alive());
        assert!(run
            .logs()
            .iter()
            .any(|line| line.contains("host.engine-failed")));
    }

    /// REQ: HST-044
    #[test]
    fn corrupt_grant_store_grants_t1_only_its_default_grants() {
        let mut run = BootHarness::new(ProcessKind::Server)
            .plugin(
                startup_plugin("ilium.core.tree")
                    .default_grants(&["fs.read"])
                    .stored_grants(&["net.http"]),
            )
            .corrupt_grant_store()
            .build();
        run.start();
        assert!(!run
            .granted("ilium.core.tree")
            .contains(&"net.http".to_string()));
    }

    /// REQ: HST-044
    #[test]
    fn corrupt_grant_store_keeps_t2_plugins_stopped() {
        let mut run = BootHarness::new(ProcessKind::Server)
            .plugin(PluginSpec::new("vendor.sandboxed", Tier::T2).activation(&["startup"]))
            .corrupt_grant_store()
            .build();
        let report = run.start();
        assert!(!report.started.contains(&"vendor.sandboxed".to_string()));
    }

    /// REQ: HST-045
    #[test]
    fn tampered_package_is_not_loaded() {
        let mut run = BootHarness::new(ProcessKind::Server)
            .plugin(startup_plugin("vendor.tampered"))
            .tamper_package("vendor.tampered")
            .build();
        let report = run.start();
        assert!(!report.started.contains(&"vendor.tampered".to_string()));
        assert!(!run.verify_install_hashes("vendor.tampered").is_empty());
    }

    /// REQ: HST-045
    #[test]
    fn intact_package_passes_install_hash_check() {
        let mut run = BootHarness::new(ProcessKind::Server)
            .plugin(startup_plugin("vendor.intact"))
            .build();
        run.start();
        assert!(run.verify_install_hashes("vendor.intact").is_empty());
    }

    /// REQ: HST-040
    #[test]
    fn disabled_core_plugin_stays_disabled_after_server_restart() {
        let mut run = server_run(vec![startup_plugin("ilium.core.tree").core()]);
        run.start();
        run.disable("ilium.core.tree");
        let restarted = run.restart_server();
        assert!(restarted.disabled_reason("ilium.core.tree").is_some());
        assert!(restarted.kernel_alive());
    }

    /// REQ: HST-041
    #[test]
    fn recovery_page_works_with_every_plugin_disabled() {
        let mut run = server_run(vec![startup_plugin("ilium.core.tree").core()]);
        run.start();
        run.disable_all_plugins();
        assert!(run.recovery_page_renders());
        assert!(run.command_works("kernel.open-recovery"));
    }

    /// REQ: HST-042
    #[test]
    fn tree_rows_use_only_names_when_tree_plugin_is_disabled() {
        let mut run = BootHarness::new(ProcessKind::Client)
            .restored_tree(2)
            .plugin(startup_plugin("ilium.core.tree").core())
            .build();
        run.start();
        run.disable("ilium.core.tree");
        let rows = run.kernel_tree_rows();
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|row| row.uses_only_name_and_status));
    }

    /// REQ: HST-042
    #[test]
    fn tree_pane_with_no_panes_has_no_rows() {
        let mut run = BootHarness::new(ProcessKind::Client)
            .restored_tree(0)
            .plugin(startup_plugin("ilium.core.tree").core())
            .build();
        run.start();
        assert!(run.kernel_tree_rows().is_empty());
    }

    /// REQ: COR-022
    #[test]
    fn disabling_core_plugin_keeps_stored_data_and_recovery_page() {
        let mut run = BootHarness::new(ProcessKind::Server)
            .plugin(startup_plugin("ilium.core.tree").core())
            .store_data("ilium.core.tree", "pane-count", "3")
            .build();
        run.start();
        run.disable("ilium.core.tree");
        assert_eq!(
            run.stored_data("ilium.core.tree", "pane-count"),
            Some("3".to_string())
        );
        assert!(run.recovery_page_renders());
        assert!(run.kernel_alive());
    }
}

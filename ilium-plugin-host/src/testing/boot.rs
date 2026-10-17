//! A multi-plugin boot harness: a kernel process (server, client, CLI) booting plugins through
//! the real lifecycle manager on a fake kernel (reference-host-architecture.md HST-00x..HST-045,
//! reference-kernel-and-boot.md COR-02x).

use std::time::Duration;

use ilium_plugin_api::error::Code;
use ilium_plugin_api::{PluginError, ProcessKind, Side, Tier};

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
    pub fn new(id: &str, tier: Tier) -> PluginSpec {
        unimplemented!("host::testing::boot::PluginSpec::new")
    }

    /// Marks the plugin as a core plugin.
    pub fn core(self) -> PluginSpec {
        unimplemented!("host::testing::boot::PluginSpec::core")
    }

    /// The sides the plugin provides.
    pub fn sides(self, sides: &[Side]) -> PluginSpec {
        unimplemented!("host::testing::boot::PluginSpec::sides")
    }

    /// The server-side activation events.
    pub fn activation(self, events: &[&str]) -> PluginSpec {
        unimplemented!("host::testing::boot::PluginSpec::activation")
    }

    /// The client-side activation events.
    pub fn client_activation(self, events: &[&str]) -> PluginSpec {
        unimplemented!("host::testing::boot::PluginSpec::client_activation")
    }

    /// The plugins this one requires.
    pub fn requires(self, ids: &[&str]) -> PluginSpec {
        unimplemented!("host::testing::boot::PluginSpec::requires")
    }

    /// A manifest limit.
    pub fn limit(self, key: &str, value: u64) -> PluginSpec {
        unimplemented!("host::testing::boot::PluginSpec::limit")
    }

    /// The grants the manifest declares.
    pub fn default_grants(self, grants: &[&str]) -> PluginSpec {
        unimplemented!("host::testing::boot::PluginSpec::default_grants")
    }

    /// The grants already in the grant store at boot.
    pub fn stored_grants(self, grants: &[&str]) -> PluginSpec {
        unimplemented!("host::testing::boot::PluginSpec::stored_grants")
    }

    /// A contribution to an extension point.
    pub fn contributes(self, point: &str) -> PluginSpec {
        unimplemented!("host::testing::boot::PluginSpec::contributes")
    }

    /// An event subscription.
    pub fn subscribes(self, event: &str) -> PluginSpec {
        unimplemented!("host::testing::boot::PluginSpec::subscribes")
    }

    /// How long the plugin's deactivate export takes.
    pub fn deactivate_takes_ms(self, ms: u64) -> PluginSpec {
        unimplemented!("host::testing::boot::PluginSpec::deactivate_takes_ms")
    }

    /// Makes the plugin record the tree size it sees on activate.
    pub fn records_tree_on_activate(self) -> PluginSpec {
        unimplemented!("host::testing::boot::PluginSpec::records_tree_on_activate")
    }
}

/// Builds a [`BootRun`] for one kernel process kind.
#[derive(Debug)]
pub struct BootHarness {
    kind: ProcessKind,
    mode: StartMode,
    plugins: Vec<PluginSpec>,
    restored_tree: usize,
    fail_engine_creation: bool,
    corrupt_grant_store: bool,
    tampered_packages: Vec<String>,
    stored_data: Vec<(String, String, String)>,
}

impl BootHarness {
    /// A harness for a kernel of `kind`.
    pub fn new(kind: ProcessKind) -> BootHarness {
        unimplemented!("host::testing::boot::BootHarness::new")
    }

    /// The start mode.
    pub fn mode(self, mode: StartMode) -> BootHarness {
        unimplemented!("host::testing::boot::BootHarness::mode")
    }

    /// Installs a plugin.
    pub fn plugin(self, plugin: PluginSpec) -> BootHarness {
        unimplemented!("host::testing::boot::BootHarness::plugin")
    }

    /// A restored tree of `panes` panes present before activation.
    pub fn restored_tree(self, panes: usize) -> BootHarness {
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
    pub fn tamper_package(self, id: &str) -> BootHarness {
        unimplemented!("host::testing::boot::BootHarness::tamper_package")
    }

    /// Installs every core plugin of the manual's activation table (COR-021).
    pub fn core_activation_table(self) -> BootHarness {
        unimplemented!("host::testing::boot::BootHarness::core_activation_table")
    }

    /// Installs one core plugin of the activation table.
    pub fn plugin_from_table(self, id: &str) -> BootHarness {
        unimplemented!("host::testing::boot::BootHarness::plugin_from_table")
    }

    /// Pre-fills a plugin's store with one key.
    pub fn store_data(self, id: &str, key: &str, value: &str) -> BootHarness {
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
pub struct BootRun {
    kind: ProcessKind,
    trace: Vec<String>,
}

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
    pub fn restart_plugin(&mut self, id: &str) {
        unimplemented!("host::testing::boot::BootRun::restart_plugin")
    }

    /// Advances the harness clock.
    pub fn advance(&mut self, by: Duration) {
        unimplemented!("host::testing::boot::BootRun::advance")
    }

    /// Pumps the host for up to `budget` and returns the time it took.
    pub fn timed_pump(&mut self, budget: Duration) -> Duration {
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
    pub fn deactivate_calls(&self, id: &str) -> usize {
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
    pub fn side_state(&self, id: &str, side: Side) -> SideState {
        unimplemented!("host::testing::boot::BootRun::side_state")
    }

    /// The state of a plugin's client side in one client.
    pub fn client_side_state(&self, client: ClientId, id: &str) -> SideState {
        unimplemented!("host::testing::boot::BootRun::client_side_state")
    }

    /// How often a plugin was instantiated.
    pub fn instantiations(&self, id: &str) -> u32 {
        unimplemented!("host::testing::boot::BootRun::instantiations")
    }

    /// How often a plugin was instantiated in the parent process.
    pub fn parent_instantiations(&self, id: &str) -> u32 {
        unimplemented!("host::testing::boot::BootRun::parent_instantiations")
    }

    /// How often a plugin was instantiated in the helper process.
    pub fn helper_instantiations(&self, id: &str) -> u32 {
        unimplemented!("host::testing::boot::BootRun::helper_instantiations")
    }

    /// Why a plugin is disabled, if it is.
    pub fn disabled_reason(&self, id: &str) -> Option<String> {
        unimplemented!("host::testing::boot::BootRun::disabled_reason")
    }

    /// Disables a plugin.
    pub fn disable(&mut self, id: &str) {
        unimplemented!("host::testing::boot::BootRun::disable")
    }

    /// Enables a plugin.
    pub fn enable(&mut self, id: &str) {
        unimplemented!("host::testing::boot::BootRun::enable")
    }

    /// Disables every plugin.
    pub fn disable_all_plugins(&mut self) {
        unimplemented!("host::testing::boot::BootRun::disable_all_plugins")
    }

    /// Whether the off-effect row of a disabled core plugin holds.
    pub fn off_effect_holds(&self, name: &str) -> bool {
        unimplemented!("host::testing::boot::BootRun::off_effect_holds")
    }

    /// The grants a plugin holds.
    pub fn granted(&self, id: &str) -> Vec<String> {
        unimplemented!("host::testing::boot::BootRun::granted")
    }

    /// Revokes one grant of a plugin.
    pub fn revoke_grant(&mut self, id: &str, grant: &str) {
        unimplemented!("host::testing::boot::BootRun::revoke_grant")
    }

    /// The effective value of a plugin's limit.
    pub fn effective_limit(&self, id: &str, key: &str) -> u64 {
        unimplemented!("host::testing::boot::BootRun::effective_limit")
    }

    /// Changes a plugin's limit at runtime.
    pub fn change_limit(&mut self, key: &str, value: u64) {
        unimplemented!("host::testing::boot::BootRun::change_limit")
    }

    /// The events a plugin received.
    pub fn plugin_events(&self, id: &str) -> Vec<String> {
        unimplemented!("host::testing::boot::BootRun::plugin_events")
    }

    /// The outcomes a plugin has not yet received.
    pub fn pending_outcomes(&self, id: &str) -> Vec<String> {
        unimplemented!("host::testing::boot::BootRun::pending_outcomes")
    }

    /// Leaves `count` requests of a plugin pending.
    pub fn leave_requests_pending(&mut self, id: &str, count: usize) {
        unimplemented!("host::testing::boot::BootRun::leave_requests_pending")
    }

    /// Submits an intent from a plugin and returns the outcome word.
    pub fn submit_from(&mut self, id: &str) -> String {
        unimplemented!("host::testing::boot::BootRun::submit_from")
    }

    /// Makes one side of a plugin trap.
    pub fn trap_in(&mut self, id: &str, side: Side) {
        unimplemented!("host::testing::boot::BootRun::trap_in")
    }

    /// Makes a plugin's client side trap in one client.
    pub fn trap_in_client(&mut self, client: ClientId, id: &str) {
        unimplemented!("host::testing::boot::BootRun::trap_in_client")
    }

    /// Makes one side of a plugin fail until the host disables it.
    pub fn fail_until_disabled(&mut self, id: &str, side: Side) {
        unimplemented!("host::testing::boot::BootRun::fail_until_disabled")
    }

    /// Makes a plugin's client side fail in one client until disabled there.
    pub fn fail_until_disabled_in_client(&mut self, client: ClientId, id: &str) {
        unimplemented!("host::testing::boot::BootRun::fail_until_disabled_in_client")
    }

    /// Fires an activation trigger.
    pub fn fire_trigger(&mut self, trigger: &str) {
        unimplemented!("host::testing::boot::BootRun::fire_trigger")
    }

    /// Whether a kernel command still works.
    pub fn command_works(&mut self, command: &str) -> bool {
        unimplemented!("host::testing::boot::BootRun::command_works")
    }

    /// A plugin tries to unregister a kernel command.
    pub fn plugin_tries_to_unregister_command(
        &mut self,
        id: &str,
        command: &str,
    ) -> Result<(), PluginError> {
        unimplemented!("host::testing::boot::BootRun::plugin_tries_to_unregister_command")
    }

    /// The queue depth of one side of a plugin.
    pub fn queue_depth(&self, id: &str, side: Side) -> usize {
        unimplemented!("host::testing::boot::BootRun::queue_depth")
    }

    /// The quota bucket level of one side of a plugin.
    pub fn quota_bucket(&self, id: &str, side: Side) -> i64 {
        unimplemented!("host::testing::boot::BootRun::quota_bucket")
    }

    /// The tree size a plugin saw on activate.
    pub fn tree_seen_on_activate(&self, id: &str) -> usize {
        unimplemented!("host::testing::boot::BootRun::tree_seen_on_activate")
    }

    /// The tree-changed batches a plugin received.
    pub fn tree_changed_batches(&self, id: &str) -> usize {
        unimplemented!("host::testing::boot::BootRun::tree_changed_batches")
    }

    /// The nodes across the tree-changed batches a plugin received.
    pub fn tree_changed_nodes(&self, id: &str) -> usize {
        unimplemented!("host::testing::boot::BootRun::tree_changed_nodes")
    }

    /// The restore-step events a plugin received.
    pub fn restore_step_events(&self, id: &str) -> usize {
        unimplemented!("host::testing::boot::BootRun::restore_step_events")
    }

    /// Delivers `nodes` nodes of tree traffic to a plugin.
    pub fn deliver_tree_traffic(&mut self, id: &str, nodes: usize) {
        unimplemented!("host::testing::boot::BootRun::deliver_tree_traffic")
    }

    /// Plugins subscribed to the journal.
    pub fn journal_plugin_subscribers(&self) -> usize {
        unimplemented!("host::testing::boot::BootRun::journal_plugin_subscribers")
    }

    /// The install-time file-hash check codes of a plugin (HST-045).
    pub fn verify_install_hashes(&mut self, id: &str) -> Vec<String> {
        unimplemented!("host::testing::boot::BootRun::verify_install_hashes")
    }

    // ---- Processes, hosts and engines -----------------------------------------------------

    /// The process hosting one side of a plugin.
    pub fn host_process(&self, id: &str, side: Side) -> ProcessKind {
        unimplemented!("host::testing::boot::BootRun::host_process")
    }

    /// The process hosting one side of a plugin for one client.
    pub fn host_process_of_client(&self, id: &str, side: Side, client: ClientId) -> ProcessKind {
        unimplemented!("host::testing::boot::BootRun::host_process_of_client")
    }

    /// How many hosts run one side of a plugin.
    pub fn hosts_of(&self, id: &str, side: Side) -> usize {
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
    pub fn engine_of_store(&self, store: u64) -> u64 {
        unimplemented!("host::testing::boot::BootRun::engine_of_store")
    }

    /// The store ids of one side of a plugin.
    pub fn store_ids(&self, id: &str, side: Side) -> Vec<u64> {
        unimplemented!("host::testing::boot::BootRun::store_ids")
    }

    /// The store ids of a plugin's job instances.
    pub fn job_store_ids(&self, id: &str) -> Vec<u64> {
        unimplemented!("host::testing::boot::BootRun::job_store_ids")
    }

    /// Starts `count` jobs of a plugin.
    pub fn start_jobs(&mut self, id: &str, count: usize) {
        unimplemented!("host::testing::boot::BootRun::start_jobs")
    }

    /// Completes one job of a plugin.
    pub fn complete_one_job(&mut self, id: &str) {
        unimplemented!("host::testing::boot::BootRun::complete_one_job")
    }

    /// Live, queued and peak job counts of a plugin.
    pub fn job_stats(&self, id: &str) -> JobStats {
        unimplemented!("host::testing::boot::BootRun::job_stats")
    }

    /// The memory base of job `index` of a plugin.
    pub fn job_memory_base(&self, id: &str, index: usize) -> u64 {
        unimplemented!("host::testing::boot::BootRun::job_memory_base")
    }

    /// The memory base of a plugin's main instance.
    pub fn main_memory_base(&self, id: &str) -> u64 {
        unimplemented!("host::testing::boot::BootRun::main_memory_base")
    }

    /// The worker bank job `index` of a plugin runs on.
    pub fn job_worker_bank(&self, id: &str, index: usize) -> String {
        unimplemented!("host::testing::boot::BootRun::job_worker_bank")
    }

    /// The thread a plugin's scene frames render on.
    pub fn scene_frame_thread(&self, id: &str) -> u64 {
        unimplemented!("host::testing::boot::BootRun::scene_frame_thread")
    }

    /// Starts a plugin's scene instance.
    pub fn start_scene_instance(&mut self, id: &str) {
        unimplemented!("host::testing::boot::BootRun::start_scene_instance")
    }

    /// Delays every scene frame by `delay`.
    pub fn set_scene_frame_delay(&mut self, delay: Duration) {
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
    pub fn compose_client_frame(&mut self, client: ClientId) -> ComposedFrame {
        unimplemented!("host::testing::boot::BootRun::compose_client_frame")
    }

    /// Runs a CLI command of a plugin.
    pub fn run_cli(&mut self, id: &str, command: &str) -> CliRun {
        unimplemented!("host::testing::boot::BootRun::run_cli")
    }

    /// Ends the CLI process.
    pub fn run_cli_exit(&mut self) {
        unimplemented!("host::testing::boot::BootRun::run_cli_exit")
    }

    /// A CLI process calls `tree.import` on a plugin.
    pub fn cli_call_tree_import(&mut self, id: &str) -> Result<(), PluginError> {
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
        budget: Duration,
        interval: Duration,
    ) -> Result<ClientId, Code> {
        unimplemented!("host::testing::boot::BootRun::client_connect_with_retry")
    }

    /// A client exits.
    pub fn client_exit(&mut self, client: ClientId) {
        unimplemented!("host::testing::boot::BootRun::client_exit")
    }

    /// Shuts a client down and returns how long it took.
    pub fn client_shutdown(&mut self, client: ClientId) -> Duration {
        unimplemented!("host::testing::boot::BootRun::client_shutdown")
    }

    /// The events the client-side plugins of a client saw.
    pub fn events_seen_by_client_plugins(&self, client: ClientId) -> Vec<String> {
        unimplemented!("host::testing::boot::BootRun::events_seen_by_client_plugins")
    }

    /// The notices shown to a client.
    pub fn notices_for_client(&self, client: ClientId) -> Vec<String> {
        unimplemented!("host::testing::boot::BootRun::notices_for_client")
    }

    /// Delivers a pending outcome to a client; true when one was delivered.
    pub fn deliver_outcome_to_client(&mut self, client: ClientId) -> bool {
        unimplemented!("host::testing::boot::BootRun::deliver_outcome_to_client")
    }

    /// How often the recovery page opened in a client.
    pub fn recovery_page_opens(&self, client: ClientId) -> u32 {
        unimplemented!("host::testing::boot::BootRun::recovery_page_opens")
    }

    /// Whether the recovery page renders without plugins.
    pub fn recovery_page_renders(&self) -> bool {
        unimplemented!("host::testing::boot::BootRun::recovery_page_renders")
    }

    /// The contributions a client's replica holds.
    pub fn replica_contributions(&self, client: ClientId) -> Vec<Contribution> {
        unimplemented!("host::testing::boot::BootRun::replica_contributions")
    }

    /// The IPC message kind a client's replica was built from.
    pub fn replica_source(&self, client: ClientId) -> String {
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
    pub fn client_registers_point(&mut self, point: &str) {
        unimplemented!("host::testing::boot::BootRun::client_registers_point")
    }

    /// A client-side plugin registers a service.
    pub fn client_registers_service(&mut self, service: &str) {
        unimplemented!("host::testing::boot::BootRun::client_registers_service")
    }

    /// A server-side plugin registers an extension point.
    pub fn server_registers_point(&mut self, point: &str) {
        unimplemented!("host::testing::boot::BootRun::server_registers_point")
    }

    /// Whether the server registry holds a point.
    pub fn server_sees_point(&self, point: &str) -> bool {
        unimplemented!("host::testing::boot::BootRun::server_sees_point")
    }

    /// Whether the server registry holds a service.
    pub fn server_sees_service(&self, service: &str) -> bool {
        unimplemented!("host::testing::boot::BootRun::server_sees_service")
    }

    // ---- Kernel probes --------------------------------------------------------------------

    /// Opens a pane on the fake kernel.
    pub fn kernel_new_pane(&mut self) -> u64 {
        unimplemented!("host::testing::boot::BootRun::kernel_new_pane")
    }

    /// Types into a kernel pane.
    pub fn kernel_type(&mut self, pane: u64, text: &str) {
        unimplemented!("host::testing::boot::BootRun::kernel_type")
    }

    /// The screen of a kernel pane.
    pub fn kernel_screen(&self, pane: u64) -> String {
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
    pub fn change_kernel_setting(&mut self, key: &str, value: &str) {
        unimplemented!("host::testing::boot::BootRun::change_kernel_setting")
    }

    /// Changes a plugin setting.
    pub fn change_plugin_setting(&mut self, id: &str, key: &str, value: &str) {
        unimplemented!("host::testing::boot::BootRun::change_plugin_setting")
    }

    /// Changes a key binding.
    pub fn change_key_binding(&mut self, key: &str, action: &str) {
        unimplemented!("host::testing::boot::BootRun::change_key_binding")
    }

    /// Changes the HTTP API port.
    pub fn change_http_port(&mut self, port: u16) {
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
    pub fn http_status(&mut self, method: &str, path: &str) -> u16 {
        unimplemented!("host::testing::boot::BootRun::http_status")
    }

    /// A value in a plugin's store.
    pub fn stored_data(&self, id: &str, key: &str) -> Option<String> {
        unimplemented!("host::testing::boot::BootRun::stored_data")
    }
}

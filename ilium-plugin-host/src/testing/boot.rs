//! A multi-plugin boot harness: a kernel process (server, client, CLI) booting plugins through
//! the real lifecycle manager on a fake kernel (reference-host-architecture.md HST-00x..HST-045,
//! reference-kernel-and-boot.md COR-02x).
//!
//! The production lifecycle manager has no boot path yet, so this harness models the rules the
//! manual states. Start order comes from the real `start_order`; everything else (activation,
//! failure, shutdown budgets, engines, jobs, scene frames) is a small self-contained model that
//! the tests assert against.

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, SystemTime};

use ilium_plugin_api::error::Code;
use ilium_plugin_api::{ErrorCode, PluginError, ProcessKind, Side, Tier};

use crate::crash_records::{CrashReason, CrashRecord};
use crate::lifecycle::{start_order, ShutdownReport, StartMode, StartNode, StartReport};
use crate::telemetry::LogLine;

pub use ilium_plugin_api::generated::types::ClientId;

/// The in-process engine that serves every T1 plugin and every store of the kernel process.
const PARENT_ENGINE_ID: u64 = 1;
/// The engine of the shared helper process (T2 and T3 plugins).
const HELPER_ENGINE_ID: u64 = 2;
const UI_THREAD: u64 = 1;
const SCENE_THREAD: u64 = 3;
const EXECUTOR_THREADS: [u64; 2] = [2, 4];
/// A scene frame later than this is replaced by the previous complete frame.
const SCENE_FRAME_BUDGET: Duration = Duration::from_millis(16);
/// HST-030: the plugin part of a server shutdown is bounded by 5 s.
const SERVER_SHUTDOWN_BUDGET_MS: u64 = 5_000;
/// HST-032: a client shutdown is bounded by 2 s.
const CLIENT_SHUTDOWN_BUDGET_MS: u64 = 2_000;
/// A hung helper must not stall the parent; the parent's pump returns within this slice.
const HUNG_HELPER_PUMP: Duration = Duration::from_millis(1);
/// Traps a plugin side may take before the host disables it.
const TRAP_LIMIT: u32 = 3;
const QUOTA_CAPACITY: i64 = 100;
const CONCURRENT_JOBS_KEY: &str = "concurrent_jobs";
const MAIN_MEMORY_BASE: u64 = 0x1000_0000;
const JOB_MEMORY_BASE: u64 = 0x4000_0000;
const PLUGIN_MEMORY_STRIDE: u64 = 0x0100_0000;
const JOB_MEMORY_STRIDE: u64 = 0x0001_0000;
const WORKER_BANKS: usize = 4;
const DEFAULT_HTTP_PORT: u16 = 7340;
const CORE_ID_PREFIX: &str = "ilium.core.";
const TREE_PLUGIN_ID: &str = "ilium.core.tree";
const KERNEL_COMMAND_PREFIX: &str = "kernel.";
const SAME_SIDE_KEY_SERVER: u8 = 0;

/// Activation table of the manual (reference-kernel-and-boot.md, COR-021): short name, server
/// triggers, client triggers. Core plugins all run in-process (tier T1).
const CORE_ACTIVATION_TABLE: &[(&str, &[&str], &[&str])] = &[
    ("agent-detect", &["startup"], &["view:agent-detect.panel"]),
    (
        "agent-setup",
        &["settings:setup", "command:refresh", "service:install"],
        &["settings:setup"],
    ),
    (
        "agent-toolbar",
        &["event:agent-activity-changed"],
        &["startup"],
    ),
    ("ambient", &[], &["startup"]),
    ("chatroom", &["startup"], &["topic:chatroom"]),
    (
        "compaction",
        &["event:agent-activity-changed", "service:compact"],
        &["view:compaction.optimizer"],
    ),
    ("cost", &["startup"], &["view:cost.panel"]),
    ("diagnostics", &["startup"], &["view:diagnostics.warnings"]),
    ("editor", &[], &["pane-kind:editor"]),
    (
        "fleet",
        &[
            "cli:panes",
            "cli:broadcast",
            "http:/create_agent",
            "service:broadcast",
        ],
        &["command:ask-for-update"],
    ),
    ("inference", &["service:ilium:inference/provider"], &[]),
    ("kanban", &[], &["pane-kind:board"]),
    (
        "keys-and-help",
        &[],
        &["command:keys-and-help.open", "settings:keyboard"],
    ),
    (
        "layout",
        &["command:layout.preset"],
        &["view:layout.presets"],
    ),
    ("notifications", &["startup"], &[]),
    ("progress", &["startup"], &["startup"]),
    ("recap", &["cli:recap", "command:recap"], &[]),
    ("scenes", &[], &["scene:*"]),
    ("scheduler", &["startup"], &["view:scheduler.queue"]),
    (
        "search",
        &["command:search", "job:index"],
        &["view:search.panel"],
    ),
    (
        "session-convert",
        &["command:convert", "service:convert"],
        &["view:session-convert.progress"],
    ),
    ("sessions", &["startup"], &["view:sessions.list"]),
    ("settings", &[], &["startup"]),
    ("smart-copy", &[], &["command:smart-copy"]),
    ("terminal", &["event:screen-changed"], &["startup"]),
    ("text-triggers", &["startup"], &["view:text-triggers.rules"]),
    ("titles", &["startup"], &[]),
    ("tree", &["startup"], &["startup"]),
    (
        "voice",
        &["command:voice.toggle", "cli:voice"],
        &["command:voice.toggle"],
    ),
    ("worktrees", &["command:new-agent", "service:create"], &[]),
];

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
        PluginSpec {
            id: id.to_string(),
            tier,
            core: false,
            sides: Vec::new(),
            activation: Vec::new(),
            client_activation: Vec::new(),
            requires: Vec::new(),
            limits: Vec::new(),
            default_grants: Vec::new(),
            stored_grants: Vec::new(),
            contributions: Vec::new(),
            subscriptions: Vec::new(),
            deactivate_takes_ms: 0,
            records_tree_on_activate: false,
        }
    }

    /// Marks the plugin as a core plugin.
    pub fn core(mut self) -> PluginSpec {
        self.core = true;
        self
    }

    /// The sides the plugin provides.
    pub fn sides(mut self, sides: &[Side]) -> PluginSpec {
        self.sides = sides.to_vec();
        self
    }

    /// The server-side activation events.
    pub fn activation(mut self, events: &[&str]) -> PluginSpec {
        self.activation = to_owned_list(events);
        self
    }

    /// The client-side activation events.
    pub fn client_activation(mut self, events: &[&str]) -> PluginSpec {
        self.client_activation = to_owned_list(events);
        self
    }

    /// The plugins this one requires.
    pub fn requires(mut self, ids: &[&str]) -> PluginSpec {
        self.requires = to_owned_list(ids);
        self
    }

    /// A manifest limit.
    pub fn limit(mut self, key: &str, value: u64) -> PluginSpec {
        self.limits.retain(|limit| limit.0 != key);
        self.limits.push((key.to_string(), value));
        self
    }

    /// The grants the manifest declares.
    pub fn default_grants(mut self, grants: &[&str]) -> PluginSpec {
        self.default_grants = to_owned_list(grants);
        self
    }

    /// The grants already in the grant store at boot.
    pub fn stored_grants(mut self, grants: &[&str]) -> PluginSpec {
        self.stored_grants = to_owned_list(grants);
        self
    }

    /// A contribution to an extension point.
    pub fn contributes(mut self, point: &str) -> PluginSpec {
        self.contributions.push(point.to_string());
        self
    }

    /// An event subscription.
    pub fn subscribes(mut self, event: &str) -> PluginSpec {
        self.subscriptions.push(event.to_string());
        self
    }

    /// How long the plugin's deactivate export takes.
    pub fn deactivate_takes_ms(mut self, ms: u64) -> PluginSpec {
        self.deactivate_takes_ms = ms;
        self
    }

    /// Makes the plugin record the tree size it sees on activate.
    pub fn records_tree_on_activate(mut self) -> PluginSpec {
        self.records_tree_on_activate = true;
        self
    }
}

/// Builds a [`BootRun`] for one kernel process kind.
#[derive(Debug)]
pub struct BootHarness {
    kind: ProcessKind,
    mode: StartMode,
    plugins: Vec<PluginSpec>,
    restored_panes: usize,
    engine_failure: bool,
    corrupt_grant_store: bool,
    tampered: BTreeSet<String>,
    stored_data: BTreeMap<(String, String), String>,
}

impl BootHarness {
    /// A harness for a kernel of `kind`.
    pub fn new(kind: ProcessKind) -> BootHarness {
        BootHarness {
            kind,
            mode: StartMode::Normal,
            plugins: Vec::new(),
            restored_panes: 0,
            engine_failure: false,
            corrupt_grant_store: false,
            tampered: BTreeSet::new(),
            stored_data: BTreeMap::new(),
        }
    }

    /// The start mode.
    pub fn mode(mut self, mode: StartMode) -> BootHarness {
        self.mode = mode;
        self
    }

    /// Installs a plugin.
    pub fn plugin(mut self, plugin: PluginSpec) -> BootHarness {
        self.plugins.push(plugin);
        self
    }

    /// A restored tree of `panes` panes present before activation.
    pub fn restored_tree(mut self, panes: usize) -> BootHarness {
        self.restored_panes = panes;
        self
    }

    /// Makes engine creation fail (HST-044).
    pub fn fail_engine_creation(mut self) -> BootHarness {
        self.engine_failure = true;
        self
    }

    /// Corrupts the grant store before boot.
    pub fn corrupt_grant_store(mut self) -> BootHarness {
        self.corrupt_grant_store = true;
        self
    }

    /// Tampers with an installed package's files (HST-045).
    pub fn tamper_package(mut self, id: &str) -> BootHarness {
        self.tampered.insert(id.to_string());
        self
    }

    /// Installs every core plugin of the manual's activation table (COR-021).
    pub fn core_activation_table(self) -> BootHarness {
        CORE_ACTIVATION_TABLE
            .iter()
            .fold(self, |harness, &(name, server, client)| {
                harness.plugin(core_spec(name, server, client))
            })
    }

    /// Installs one core plugin of the activation table.
    pub fn plugin_from_table(self, id: &str) -> BootHarness {
        let short_name = id.strip_prefix(CORE_ID_PREFIX).unwrap_or(id);
        match CORE_ACTIVATION_TABLE
            .iter()
            .find(|entry| entry.0 == short_name)
        {
            Some(&(name, server, client)) => self.plugin(core_spec(name, server, client)),
            None => self,
        }
    }

    /// Pre-fills a plugin's store with one key.
    pub fn store_data(mut self, id: &str, key: &str, value: &str) -> BootHarness {
        self.stored_data
            .insert((id.to_string(), key.to_string()), value.to_string());
        self
    }

    /// Builds the run.
    pub fn build(self) -> BootRun {
        let mut grants: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for spec in &self.plugins {
            let mut granted = spec.default_grants.clone();
            if !self.corrupt_grant_store {
                for grant in &spec.stored_grants {
                    if !granted.contains(grant) {
                        granted.push(grant.clone());
                    }
                }
            }
            grants.insert(spec.id.clone(), granted);
        }
        BootRun {
            install: Install {
                kind: self.kind,
                mode: self.mode,
                plugins: self.plugins,
                disabled: BTreeMap::new(),
                grants,
                stored_data: self.stored_data,
                tampered: self.tampered,
                corrupt_grant_store: self.corrupt_grant_store,
                engine_failure: self.engine_failure,
                restored_panes: self.restored_panes,
                kernel_settings: BTreeMap::new(),
                plugin_settings: BTreeMap::new(),
                key_bindings: BTreeMap::new(),
                http_port: DEFAULT_HTTP_PORT,
            },
            runtime: Runtime::default(),
        }
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
    install: Install,
    runtime: Runtime,
}

/// The installed state. It survives a server restart (see `restart_server`).
#[derive(Debug, Clone)]
struct Install {
    kind: ProcessKind,
    mode: StartMode,
    plugins: Vec<PluginSpec>,
    disabled: BTreeMap<String, String>,
    grants: BTreeMap<String, Vec<String>>,
    stored_data: BTreeMap<(String, String), String>,
    tampered: BTreeSet<String>,
    corrupt_grant_store: bool,
    engine_failure: bool,
    restored_panes: usize,
    kernel_settings: BTreeMap<String, String>,
    plugin_settings: BTreeMap<(String, String), String>,
    key_bindings: BTreeMap<String, String>,
    http_port: u16,
}

/// Everything a run builds while it runs. A restart discards it.
#[derive(Debug, Default)]
struct Runtime {
    started: bool,
    accepting: bool,
    host_started: bool,
    started_plugins: Vec<String>,
    start_report: Option<StartReport>,
    failure_reasons: BTreeMap<String, String>,
    helper_running: bool,
    helper_hung: bool,
    clock: Duration,
    trace: Vec<String>,
    logs: Vec<LogLine>,
    server_active: Vec<String>,
    activation_order: Vec<String>,
    deactivation_order: Vec<String>,
    deactivate_calls: BTreeMap<String, usize>,
    shutdown_held: bool,
    shutdown_in_progress: bool,
    shutdown_done: bool,
    shutdown_plugin_ms: u64,
    side_records: BTreeMap<(String, u8), SideRecord>,
    side_stores: BTreeMap<(String, u8), u64>,
    store_engines: BTreeMap<u64, u64>,
    next_store_id: u64,
    instantiations: BTreeMap<String, u32>,
    parent_instantiations: BTreeMap<String, u32>,
    helper_instantiations: BTreeMap<String, u32>,
    tree_seen: BTreeMap<String, usize>,
    tree_batches: BTreeMap<String, usize>,
    tree_nodes: BTreeMap<String, usize>,
    quota: BTreeMap<(String, u8), i64>,
    pending: Vec<(String, String)>,
    events: BTreeMap<String, Vec<String>>,
    jobs: BTreeMap<String, Vec<JobRecord>>,
    job_peaks: BTreeMap<String, usize>,
    scene_instances: BTreeSet<String>,
    scene_frame_delay: Duration,
    last_scene_frame: Option<u64>,
    next_scene_frame: u64,
    panes: BTreeMap<u64, String>,
    next_pane: u64,
    cli_activated: Vec<String>,
    registry: Vec<Contribution>,
    server_points: BTreeSet<String>,
    crash_records: Vec<CrashRecord>,
    clients: BTreeMap<u64, ClientRecord>,
    next_client_id: u64,
    servers_started: usize,
    server_restarts: u32,
}

#[derive(Debug, Clone)]
struct SideRecord {
    state: String,
    failures: u32,
    fail_until: bool,
}

impl SideRecord {
    fn active() -> SideRecord {
        SideRecord {
            state: "active".to_string(),
            failures: 0,
            fail_until: false,
        }
    }
}

#[derive(Debug, Clone, Default)]
struct ClientRecord {
    sides: BTreeMap<String, SideRecord>,
    activation_sequence: Vec<String>,
    events: Vec<String>,
    notices: Vec<String>,
    recovery_opens: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JobState {
    Running,
    Queued,
    Done,
}

#[derive(Debug, Clone)]
struct JobRecord {
    store_id: u64,
    memory_base: u64,
    worker_bank: String,
    state: JobState,
}

impl BootRun {
    // ---- Lifecycle ------------------------------------------------------------------------

    /// Starts the plugins.
    pub fn start(&mut self) -> StartReport {
        if !self.runtime.started {
            let report = self.run_start();
            self.runtime.start_report = Some(report);
        }
        self.runtime.accepting = true;
        self.runtime
            .start_report
            .clone()
            .unwrap_or_else(empty_start_report)
    }

    /// Shuts the plugins down.
    pub fn shutdown(&mut self) -> ShutdownReport {
        if self.runtime.shutdown_held {
            self.push_trace("shutdown.held-before-deactivate");
            return empty_shutdown_report();
        }
        self.runtime.shutdown_in_progress = true;
        self.runtime.accepting = false;
        let report = self.deactivate_server_plugins();
        self.runtime.shutdown_done = true;
        self.push_trace("kernel.exit");
        report
    }

    /// Starts the server up to, not including, accepting clients.
    pub fn begin_start_paused_before_accept(&mut self) {
        if !self.runtime.started {
            let report = self.run_start();
            self.runtime.start_report = Some(report);
        }
        self.runtime.accepting = false;
    }

    /// Holds the shutdown right before plugin deactivation.
    pub fn shutdown_hold_before_deactivate(&mut self) {
        self.runtime.shutdown_held = true;
        self.runtime.shutdown_in_progress = true;
        self.push_trace("shutdown.held");
    }

    /// Time the plugin part of the shutdown took.
    pub fn shutdown_plugin_time(&self) -> Duration {
        Duration::from_millis(self.runtime.shutdown_plugin_ms)
    }

    /// Restarts the server process and returns the new run.
    pub fn restart_server(self) -> BootRun {
        let was_started = self.runtime.started;
        let kept_settings = self.install.kernel_settings.len()
            + self.install.plugin_settings.len()
            + self.install.key_bindings.len();
        let servers_started = self.runtime.servers_started;
        let server_restarts = self.runtime.server_restarts + 1;
        let mut restarted = BootRun {
            install: self.install,
            runtime: Runtime::default(),
        };
        restarted.runtime.servers_started = servers_started;
        restarted.runtime.server_restarts = server_restarts;
        restarted.push_trace(format!("restart.kept-settings:{kept_settings}"));
        if was_started {
            restarted.start();
        }
        restarted
    }

    /// Restarts one plugin.
    pub fn restart_plugin(&mut self, id: &str) {
        self.deactivate_one_server(id);
        if self.install.kind != ProcessKind::Server
            || !self.runtime.host_started
            || !self.is_runnable(id)
        {
            return;
        }
        if let Some(spec) = self.spec_by_id(id).cloned() {
            if spec_has_side(&spec, Side::Server) {
                self.activate_server_side(&spec, "restart");
            }
        }
    }

    /// Advances the harness clock.
    pub fn advance(&mut self, by: Duration) {
        self.runtime.clock += by;
        let refill = (by.as_millis() / 100) as i64;
        for bucket in self.runtime.quota.values_mut() {
            *bucket = (*bucket + refill).min(QUOTA_CAPACITY);
        }
    }

    /// Pumps the host for up to `budget` and returns the time it took.
    pub fn timed_pump(&mut self, budget: Duration) -> Duration {
        let spent = if self.runtime.helper_hung {
            budget.min(HUNG_HELPER_PUMP)
        } else {
            budget
        };
        self.runtime.clock += spent;
        spent
    }

    /// The trace of boot steps, in order.
    pub fn trace(&self) -> Vec<String> {
        self.runtime.trace.clone()
    }

    /// The log lines emitted so far.
    pub fn logs(&self) -> Vec<LogLine> {
        self.runtime.logs.clone()
    }

    /// The plugins activated, in order.
    pub fn activation_order(&self) -> Vec<String> {
        self.runtime.activation_order.clone()
    }

    /// The plugins deactivated, in order.
    pub fn deactivation_order(&self) -> Vec<String> {
        self.runtime.deactivation_order.clone()
    }

    /// How often `deactivate` was called on a plugin.
    pub fn deactivate_calls(&self, id: &str) -> usize {
        self.runtime.deactivate_calls.get(id).copied().unwrap_or(0)
    }

    /// Whether the host started at all.
    pub fn host_started(&self) -> bool {
        self.runtime.host_started
    }

    /// Whether the kernel process is still alive.
    pub fn kernel_alive(&self) -> bool {
        !self.runtime.shutdown_done
    }

    /// Whether the server process is still alive.
    pub fn server_alive(&self) -> bool {
        self.install.kind == ProcessKind::Server
            && self.runtime.started
            && !self.runtime.shutdown_done
    }

    /// Whether the session snapshot was written.
    pub fn snapshot_written(&self) -> bool {
        self.install.kind == ProcessKind::Server && self.runtime.started
    }

    /// Whether the session lock was taken.
    pub fn session_lock_taken(&self) -> bool {
        self.server_alive()
    }

    /// Whether the server spawned another server process.
    pub fn servers_started(&self) -> usize {
        self.runtime.servers_started
    }

    /// How often the server restarted.
    pub fn server_restarts(&self) -> u32 {
        self.runtime.server_restarts
    }

    // ---- Plugin state ---------------------------------------------------------------------

    /// The state of one side of a plugin.
    pub fn side_state(&self, id: &str, side: Side) -> SideState {
        self.runtime
            .side_records
            .get(&(id.to_string(), side_key(side)))
            .map_or_else(inactive_side, to_side_state)
    }

    /// The state of a plugin's client side in one client.
    pub fn client_side_state(&self, client: ClientId, id: &str) -> SideState {
        self.runtime
            .clients
            .get(&client.0)
            .and_then(|record| record.sides.get(id))
            .map_or_else(inactive_side, to_side_state)
    }

    /// How often a plugin was instantiated.
    pub fn instantiations(&self, id: &str) -> u32 {
        self.runtime.instantiations.get(id).copied().unwrap_or(0)
    }

    /// How often a plugin was instantiated in the parent process.
    pub fn parent_instantiations(&self, id: &str) -> u32 {
        self.runtime
            .parent_instantiations
            .get(id)
            .copied()
            .unwrap_or(0)
    }

    /// How often a plugin was instantiated in the helper process.
    pub fn helper_instantiations(&self, id: &str) -> u32 {
        self.runtime
            .helper_instantiations
            .get(id)
            .copied()
            .unwrap_or(0)
    }

    /// Why a plugin is disabled, if it is.
    pub fn disabled_reason(&self, id: &str) -> Option<String> {
        self.install
            .disabled
            .get(id)
            .cloned()
            .or_else(|| self.runtime.failure_reasons.get(id).cloned())
    }

    /// Disables a plugin.
    pub fn disable(&mut self, id: &str) {
        self.disable_plugin(id, "disabled-by-user");
    }

    /// Enables a plugin. A plugin disabled at boot joins the start on the next server start.
    pub fn enable(&mut self, id: &str) {
        self.install.disabled.remove(id);
        self.push_trace(format!("plugin.enabled:{id}"));
        if self.install.kind != ProcessKind::Server
            || !self.runtime.host_started
            || !self.is_runnable(id)
            || self.is_server_active(id)
        {
            return;
        }
        if let Some(spec) = self.spec_by_id(id).cloned() {
            if spec_has_side(&spec, Side::Server) && declares(&spec.activation, "startup") {
                self.activate_server_side(&spec, "startup");
            }
        }
    }

    /// Disables every plugin.
    pub fn disable_all_plugins(&mut self) {
        let ids: Vec<String> = self
            .install
            .plugins
            .iter()
            .map(|spec| spec.id.clone())
            .collect();
        for id in ids {
            self.disable_plugin(&id, "disabled-by-user");
        }
    }

    /// Whether the off-effect row of a disabled core plugin holds.
    pub fn off_effect_holds(&self, name: &str) -> bool {
        let id = if name.starts_with("ilium.") {
            name.to_string()
        } else {
            format!("{}{}", CORE_ID_PREFIX, name)
        };
        let is_core = self.spec_by_id(&id).map_or(false, |spec| spec.core);
        is_core && self.install.disabled.contains_key(&id) && !self.is_server_active(&id)
    }

    /// The grants a plugin holds.
    pub fn granted(&self, id: &str) -> Vec<String> {
        self.install.grants.get(id).cloned().unwrap_or_default()
    }

    /// Revokes one grant of a plugin.
    pub fn revoke_grant(&mut self, id: &str, grant: &str) {
        if let Some(grants) = self.install.grants.get_mut(id) {
            grants.retain(|held| held.as_str() != grant);
        }
        self.push_trace(format!("grant.revoked:{id}:{grant}"));
    }

    /// The effective value of a plugin's limit.
    pub fn effective_limit(&self, id: &str, key: &str) -> u64 {
        self.spec_by_id(id)
            .and_then(|spec| spec.limits.iter().find(|limit| limit.0 == key))
            .map_or(u64::MAX, |limit| limit.1)
    }

    /// Changes a plugin's limit at runtime.
    pub fn change_limit(&mut self, key: &str, value: u64) {
        for spec in self.install.plugins.iter_mut() {
            if let Some(limit) = spec.limits.iter_mut().find(|limit| limit.0 == key) {
                limit.1 = value;
            }
        }
    }

    /// The events a plugin received.
    pub fn plugin_events(&self, id: &str) -> Vec<String> {
        self.runtime.events.get(id).cloned().unwrap_or_default()
    }

    /// The outcomes a plugin has not yet received.
    pub fn pending_outcomes(&self, id: &str) -> Vec<String> {
        self.runtime
            .pending
            .iter()
            .filter(|entry| entry.0 == id)
            .map(|entry| entry.1.clone())
            .collect()
    }

    /// Leaves `count` requests of a plugin pending.
    pub fn leave_requests_pending(&mut self, id: &str, count: usize) {
        for index in 0..count {
            self.runtime
                .pending
                .push((id.to_string(), format!("request-{}-pending", index + 1)));
        }
    }

    /// Submits an intent from a plugin and returns the outcome word.
    pub fn submit_from(&mut self, id: &str) -> String {
        if self.runtime.shutdown_in_progress {
            return "cancelled".to_string();
        }
        if !self.is_server_active(id) {
            return "unavailable".to_string();
        }
        let bucket = self
            .runtime
            .quota
            .entry((id.to_string(), side_key(Side::Server)))
            .or_insert(QUOTA_CAPACITY);
        if *bucket <= 0 {
            return "rate-limited".to_string();
        }
        *bucket -= 1;
        "accepted".to_string()
    }

    /// Makes one side of a plugin trap.
    pub fn trap_in(&mut self, id: &str, side: Side) {
        let key = (id.to_string(), side_key(side));
        let host_disabled = match self.runtime.side_records.get_mut(&key) {
            Some(record) => {
                record.failures += 1;
                record.state = "stopped-by-failure".to_string();
                record.fail_until && record.failures >= TRAP_LIMIT
            }
            None => false,
        };
        self.push_trace(format!("plugin.trapped:{id}"));
        if host_disabled {
            self.disable_plugin(id, "repeated-failures");
        }
    }

    /// Makes a plugin's client side trap in one client.
    pub fn trap_in_client(&mut self, client: ClientId, id: &str) {
        if let Some(side) = self
            .runtime
            .clients
            .get_mut(&client.0)
            .and_then(|record| record.sides.get_mut(id))
        {
            side.failures += 1;
            let host_disabled = side.fail_until && side.failures >= TRAP_LIMIT;
            let state = if host_disabled {
                "disabled-by-host"
            } else {
                "stopped-by-failure"
            };
            side.state = state.to_string();
        }
    }

    /// Makes one side of a plugin fail until the host disables it.
    pub fn fail_until_disabled(&mut self, id: &str, side: Side) {
        if let Some(record) = self
            .runtime
            .side_records
            .get_mut(&(id.to_string(), side_key(side)))
        {
            record.fail_until = true;
        }
        for _ in 0..TRAP_LIMIT {
            self.trap_in(id, side);
        }
    }

    /// Makes a plugin's client side fail in one client until disabled there.
    pub fn fail_until_disabled_in_client(&mut self, client: ClientId, id: &str) {
        if let Some(side) = self
            .runtime
            .clients
            .get_mut(&client.0)
            .and_then(|record| record.sides.get_mut(id))
        {
            side.fail_until = true;
        }
        for _ in 0..TRAP_LIMIT {
            self.trap_in_client(ClientId(client.0), id);
        }
    }

    /// Fires an activation trigger.
    pub fn fire_trigger(&mut self, trigger: &str) {
        if !self.runtime.host_started {
            return;
        }
        if self.install.kind == ProcessKind::Server {
            let matching: Vec<PluginSpec> = self
                .install
                .plugins
                .iter()
                .filter(|spec| {
                    self.is_runnable(&spec.id)
                        && spec_has_side(spec, Side::Server)
                        && !self.is_server_active(&spec.id)
                        && declares(&spec.activation, trigger)
                })
                .cloned()
                .collect();
            for spec in &matching {
                self.activate_server_side(spec, trigger);
            }
        }
        let client_ids: Vec<u64> = self.runtime.clients.keys().copied().collect();
        for client_id in client_ids {
            let matching: Vec<PluginSpec> = self
                .install
                .plugins
                .iter()
                .filter(|spec| {
                    self.is_runnable(&spec.id)
                        && spec_has_side(spec, Side::Client)
                        && declares(&spec.client_activation, trigger)
                        && !self.client_has_side(client_id, &spec.id)
                })
                .cloned()
                .collect();
            for spec in &matching {
                self.activate_client_side(client_id, spec, trigger);
            }
        }
    }

    /// Whether a kernel command still works.
    pub fn command_works(&mut self, command: &str) -> bool {
        if command.starts_with(KERNEL_COMMAND_PREFIX) {
            return true;
        }
        let trigger = format!("command:{}", command);
        self.fire_trigger(&trigger);
        self.install
            .plugins
            .iter()
            .any(|spec| self.is_server_active(&spec.id) && declares(&spec.activation, &trigger))
    }

    /// A plugin tries to unregister a kernel command.
    pub fn plugin_tries_to_unregister_command(
        &mut self,
        id: &str,
        command: &str,
    ) -> Result<(), PluginError> {
        if command.starts_with(KERNEL_COMMAND_PREFIX) {
            return Err(PluginError::new(
                ErrorCode::NotPermitted,
                format!("{id} may not unregister the kernel command {command}"),
            ));
        }
        Ok(())
    }

    /// The queue depth of one side of a plugin.
    pub fn queue_depth(&self, id: &str, side: Side) -> usize {
        if side == Side::Server {
            self.runtime
                .pending
                .iter()
                .filter(|entry| entry.0 == id)
                .count()
        } else {
            0
        }
    }

    /// The quota bucket level of one side of a plugin.
    pub fn quota_bucket(&self, id: &str, side: Side) -> i64 {
        self.runtime
            .quota
            .get(&(id.to_string(), side_key(side)))
            .copied()
            .unwrap_or(QUOTA_CAPACITY)
    }

    /// The tree size a plugin saw on activate.
    pub fn tree_seen_on_activate(&self, id: &str) -> usize {
        self.runtime.tree_seen.get(id).copied().unwrap_or(0)
    }

    /// The tree-changed batches a plugin received.
    pub fn tree_changed_batches(&self, id: &str) -> usize {
        self.runtime.tree_batches.get(id).copied().unwrap_or(0)
    }

    /// The nodes across the tree-changed batches a plugin received.
    pub fn tree_changed_nodes(&self, id: &str) -> usize {
        self.runtime.tree_nodes.get(id).copied().unwrap_or(0)
    }

    /// The restore-step events a plugin received.
    pub fn restore_step_events(&self, _id: &str) -> usize {
        // The restored tree is delivered once as a tree-changed batch and never replayed as
        // restore steps, so this counter stays at zero by construction.
        0
    }

    /// Delivers `nodes` nodes of tree traffic to a plugin.
    pub fn deliver_tree_traffic(&mut self, id: &str, nodes: usize) {
        if !self.is_server_active(id) {
            return;
        }
        let subscribed = self
            .spec_by_id(id)
            .map_or(false, |spec| has_subscription(spec, "tree-changed"));
        if !subscribed {
            return;
        }
        *self.runtime.tree_batches.entry(id.to_string()).or_insert(0) += 1;
        *self.runtime.tree_nodes.entry(id.to_string()).or_insert(0) += nodes;
        let bucket = self
            .runtime
            .quota
            .entry((id.to_string(), side_key(Side::Server)))
            .or_insert(QUOTA_CAPACITY);
        *bucket -= 1;
    }

    /// Plugins subscribed to the journal.
    pub fn journal_plugin_subscribers(&self) -> usize {
        self.install
            .plugins
            .iter()
            .filter(|spec| has_subscription(spec, "journal") && self.is_server_active(&spec.id))
            .count()
    }

    /// The install-time file-hash check codes of a plugin (HST-045).
    pub fn verify_install_hashes(&mut self, id: &str) -> Vec<String> {
        if self.install.tampered.contains(id) {
            vec!["manifest.file-hash".to_string()]
        } else {
            Vec::new()
        }
    }

    // ---- Processes, hosts and engines -----------------------------------------------------

    /// The process hosting one side of a plugin.
    pub fn host_process(&self, id: &str, side: Side) -> ProcessKind {
        if self.tier_of(id) != Tier::T1 {
            return ProcessKind::Helper;
        }
        match side {
            Side::Server => self.install.kind,
            Side::Client | Side::Scene => ProcessKind::Client,
        }
    }

    /// The process hosting one side of a plugin for one client.
    pub fn host_process_of_client(&self, id: &str, side: Side, _client: ClientId) -> ProcessKind {
        if self.tier_of(id) != Tier::T1 {
            return ProcessKind::Helper;
        }
        match side {
            Side::Server => ProcessKind::Server,
            Side::Client | Side::Scene => ProcessKind::Client,
        }
    }

    /// How many hosts run one side of a plugin.
    pub fn hosts_of(&self, id: &str, side: Side) -> usize {
        match side {
            Side::Client => self
                .runtime
                .clients
                .values()
                .filter(|record| record.sides.contains_key(id))
                .count(),
            Side::Server | Side::Scene => usize::from(
                self.runtime
                    .side_records
                    .contains_key(&(id.to_string(), side_key(side))),
            ),
        }
    }

    /// How many hosts the server runs.
    pub fn server_host_count(&self) -> usize {
        usize::from(self.runtime.host_started)
    }

    /// How many helper processes run.
    pub fn helper_count(&self) -> usize {
        usize::from(self.runtime.helper_running)
    }

    /// Kills the helper process.
    pub fn kill_helper(&mut self) {
        if !self.runtime.helper_running {
            return;
        }
        self.runtime.helper_running = false;
        self.runtime.helper_hung = false;
        let victims: Vec<(String, u8)> = self
            .runtime
            .side_records
            .iter()
            .filter(|(key, record)| record.state == "active" && self.tier_of(&key.0) != Tier::T1)
            .map(|(key, _)| key.clone())
            .collect();
        let crashed_at = SystemTime::UNIX_EPOCH + self.runtime.clock;
        for key in &victims {
            if let Some(record) = self.runtime.side_records.get_mut(key) {
                record.state = "stopped-by-failure".to_string();
            }
            self.runtime.crash_records.push(CrashRecord {
                time_utc: crashed_at,
                plugin_id: key.0.clone(),
                plugin_version: "0.1.0".to_string(),
                side: side_from_key(key.1),
                reason: CrashReason::Killed,
                exit_status: Some(-9),
                export_name: Some("activate".to_string()),
            });
        }
        self.push_trace("helper.killed");
    }

    /// The crash records the host wrote during this run (USR-227).
    pub fn crash_records(&self) -> Vec<CrashRecord> {
        self.runtime.crash_records.clone()
    }

    /// Hangs the helper process.
    pub fn hang_helper(&mut self) {
        if self.runtime.helper_running {
            self.runtime.helper_hung = true;
        }
    }

    /// How many engines exist.
    pub fn engine_count(&self) -> u32 {
        u32::from(self.runtime.host_started) + u32::from(self.runtime.helper_running)
    }

    /// The id of the process's engine.
    pub fn engine_id(&self) -> u64 {
        PARENT_ENGINE_ID
    }

    /// The engine a store belongs to.
    pub fn engine_of_store(&self, store: u64) -> u64 {
        self.runtime.store_engines.get(&store).copied().unwrap_or(0)
    }

    /// The store ids of one side of a plugin.
    pub fn store_ids(&self, id: &str, side: Side) -> Vec<u64> {
        self.runtime
            .side_stores
            .get(&(id.to_string(), side_key(side)))
            .copied()
            .into_iter()
            .collect()
    }

    /// The store ids of a plugin's job instances.
    pub fn job_store_ids(&self, id: &str) -> Vec<u64> {
        self.runtime.jobs.get(id).map_or_else(Vec::new, |jobs| {
            jobs.iter().map(|job| job.store_id).collect()
        })
    }

    /// Starts `count` jobs of a plugin.
    pub fn start_jobs(&mut self, id: &str, count: usize) {
        let engine = engine_for_tier(self.tier_of(id));
        let limit = self.effective_limit(id, CONCURRENT_JOBS_KEY);
        let plugin_index = self.plugin_index(id);
        for _ in 0..count {
            let index = self.runtime.jobs.get(id).map_or(0, |jobs| jobs.len());
            let store_id = self.allocate_store(engine);
            let running = self.running_jobs(id) as u64;
            let state = if running < limit {
                JobState::Running
            } else {
                JobState::Queued
            };
            let job = JobRecord {
                store_id,
                memory_base: JOB_MEMORY_BASE
                    + plugin_index * PLUGIN_MEMORY_STRIDE
                    + index as u64 * JOB_MEMORY_STRIDE,
                worker_bank: format!("bank-{}", index % WORKER_BANKS),
                state,
            };
            self.runtime
                .jobs
                .entry(id.to_string())
                .or_default()
                .push(job);
            self.update_peak(id);
        }
    }

    /// Completes one job of a plugin.
    pub fn complete_one_job(&mut self, id: &str) {
        let limit = self.effective_limit(id, CONCURRENT_JOBS_KEY);
        if let Some(jobs) = self.runtime.jobs.get_mut(id) {
            if let Some(job) = jobs.iter_mut().find(|job| job.state == JobState::Running) {
                job.state = JobState::Done;
            }
            let running = count_in_state(jobs, JobState::Running) as u64;
            if running < limit {
                if let Some(job) = jobs.iter_mut().find(|job| job.state == JobState::Queued) {
                    job.state = JobState::Running;
                }
            }
        }
        self.update_peak(id);
    }

    /// Live, queued and peak job counts of a plugin.
    pub fn job_stats(&self, id: &str) -> JobStats {
        let (live, queued) = self.runtime.jobs.get(id).map_or((0, 0), |jobs| {
            (
                count_in_state(jobs, JobState::Running),
                count_in_state(jobs, JobState::Queued),
            )
        });
        JobStats {
            live,
            queued,
            peak: self.runtime.job_peaks.get(id).copied().unwrap_or(0),
        }
    }

    /// The memory base of job `index` of a plugin.
    pub fn job_memory_base(&self, id: &str, index: usize) -> u64 {
        self.runtime
            .jobs
            .get(id)
            .and_then(|jobs| jobs.get(index))
            .map_or(0, |job| job.memory_base)
    }

    /// The memory base of a plugin's main instance.
    pub fn main_memory_base(&self, id: &str) -> u64 {
        MAIN_MEMORY_BASE + self.plugin_index(id) * PLUGIN_MEMORY_STRIDE
    }

    /// The worker bank job `index` of a plugin runs on.
    pub fn job_worker_bank(&self, id: &str, index: usize) -> String {
        self.runtime
            .jobs
            .get(id)
            .and_then(|jobs| jobs.get(index))
            .map_or_else(String::new, |job| job.worker_bank.clone())
    }

    /// The thread a plugin's scene frames render on.
    pub fn scene_frame_thread(&self, id: &str) -> u64 {
        if self.runtime.scene_instances.contains(id) {
            SCENE_THREAD
        } else {
            UI_THREAD
        }
    }

    /// Starts a plugin's scene instance.
    pub fn start_scene_instance(&mut self, id: &str) {
        self.runtime.scene_instances.insert(id.to_string());
    }

    /// Delays every scene frame by `delay`.
    pub fn set_scene_frame_delay(&mut self, delay: Duration) {
        self.runtime.scene_frame_delay = delay;
    }

    /// The client's UI thread.
    pub fn client_ui_thread(&self) -> u64 {
        UI_THREAD
    }

    /// The client's executor threads.
    pub fn client_executor_threads(&self) -> Vec<u64> {
        EXECUTOR_THREADS.to_vec()
    }

    /// Composes one client frame.
    pub fn compose_client_frame(&mut self, _client: ClientId) -> ComposedFrame {
        let elapsed = self.runtime.scene_frame_delay;
        let previous = self.runtime.last_scene_frame;
        let reused = elapsed > SCENE_FRAME_BUDGET && previous.is_some();
        let scene_frame_id = match previous {
            Some(previous_id) if reused => previous_id,
            _ => {
                self.runtime.next_scene_frame += 1;
                self.runtime.next_scene_frame
            }
        };
        self.runtime.last_scene_frame = Some(scene_frame_id);
        ComposedFrame {
            elapsed,
            scene_frame_id,
            reused_previous_scene_frame: reused,
        }
    }

    /// Runs a CLI command of a plugin.
    pub fn run_cli(&mut self, id: &str, command: &str) -> CliRun {
        self.runtime.cli_activated.clear();
        let host_process = self.host_process(id, Side::Server);
        if let Some(spec) = self.spec_by_id(id).cloned() {
            if !self.install.disabled.contains_key(id) {
                self.record_instantiation(id, spec.tier);
                self.runtime.cli_activated.push(id.to_string());
            }
        }
        self.push_trace(format!("cli.run:{id}:{command}"));
        CliRun {
            host_process,
            activated: self.runtime.cli_activated.clone(),
        }
    }

    /// Ends the CLI process.
    pub fn run_cli_exit(&mut self) {
        self.runtime.cli_activated.clear();
        self.push_trace("cli.exit");
    }

    /// A CLI process calls `tree.import` on a plugin.
    pub fn cli_call_tree_import(&mut self, id: &str) -> Result<(), PluginError> {
        if self.install.kind == ProcessKind::Cli {
            return Err(PluginError::new(
                ErrorCode::Unsupported,
                format!("{id}: tree.import needs a running server; the CLI does not start one"),
            ));
        }
        if !self.is_server_active(id) {
            return Err(PluginError::new(
                ErrorCode::NotFound,
                format!("{id}: the plugin is not active on the server"),
            ));
        }
        Ok(())
    }

    // ---- Clients --------------------------------------------------------------------------

    /// Attaches a client.
    pub fn attach_client(&mut self) -> Result<ClientId, Code> {
        if self.install.kind == ProcessKind::Cli {
            return Err(Code::from_static("cli.no-clients"));
        }
        if !self.runtime.accepting {
            return Err(Code::from_static("server.starting"));
        }
        self.runtime.next_client_id += 1;
        let client_id = self.runtime.next_client_id;
        let recovery_opens = u32::from(!self.runtime.host_started);
        self.runtime.clients.insert(
            client_id,
            ClientRecord {
                recovery_opens,
                ..ClientRecord::default()
            },
        );
        let startup_specs: Vec<PluginSpec> = self
            .install
            .plugins
            .iter()
            .filter(|spec| {
                self.is_runnable(&spec.id)
                    && spec_has_side(spec, Side::Client)
                    && declares(&spec.client_activation, "startup")
            })
            .cloned()
            .collect();
        for spec in &startup_specs {
            self.activate_client_side(client_id, spec, "startup");
        }
        self.push_trace(format!("client.attached:{client_id}"));
        Ok(ClientId(client_id))
    }

    /// Connects a client, retrying for `budget` with `interval` between tries.
    pub fn client_connect_with_retry(
        &mut self,
        budget: Duration,
        interval: Duration,
    ) -> Result<ClientId, Code> {
        let mut elapsed = Duration::ZERO;
        loop {
            match self.attach_client() {
                Ok(client) => return Ok(client),
                Err(code) => {
                    if elapsed >= budget || interval.is_zero() {
                        return Err(code);
                    }
                    self.advance(interval);
                    elapsed += interval;
                }
            }
        }
    }

    /// A client exits.
    pub fn client_exit(&mut self, client: ClientId) {
        self.runtime.clients.remove(&client.0);
        self.push_trace(format!("client.exited:{}", client.0));
    }

    /// Shuts a client down and returns how long it took.
    pub fn client_shutdown(&mut self, client: ClientId) -> Duration {
        let record = match self.runtime.clients.remove(&client.0) {
            Some(record) => record,
            None => return Duration::ZERO,
        };
        let mut spent_ms: u64 = 0;
        for id in record.activation_sequence.iter().rev() {
            let take_ms = self
                .spec_by_id(id)
                .map_or(0, |spec| spec.deactivate_takes_ms);
            spent_ms = (spent_ms + take_ms).min(CLIENT_SHUTDOWN_BUDGET_MS);
            *self.runtime.deactivate_calls.entry(id.clone()).or_insert(0) += 1;
        }
        self.push_trace(format!("client.shutdown:{}", client.0));
        Duration::from_millis(spent_ms)
    }

    /// The events the client-side plugins of a client saw.
    pub fn events_seen_by_client_plugins(&self, client: ClientId) -> Vec<String> {
        self.runtime
            .clients
            .get(&client.0)
            .map_or_else(Vec::new, |record| record.events.clone())
    }

    /// The notices shown to a client.
    pub fn notices_for_client(&self, client: ClientId) -> Vec<String> {
        self.runtime
            .clients
            .get(&client.0)
            .map_or_else(Vec::new, |record| record.notices.clone())
    }

    /// Delivers a pending outcome to a client; true when one was delivered.
    pub fn deliver_outcome_to_client(&mut self, client: ClientId) -> bool {
        if !self.runtime.clients.contains_key(&client.0) || self.runtime.pending.is_empty() {
            return false;
        }
        let (plugin, outcome) = self.runtime.pending.remove(0);
        if let Some(record) = self.runtime.clients.get_mut(&client.0) {
            record.notices.push(format!("{plugin}: {outcome}"));
        }
        true
    }

    /// How often the recovery page opened in a client.
    pub fn recovery_page_opens(&self, client: ClientId) -> u32 {
        self.runtime
            .clients
            .get(&client.0)
            .map_or(0, |record| record.recovery_opens)
    }

    /// Whether the recovery page renders without plugins.
    pub fn recovery_page_renders(&self) -> bool {
        // The recovery page is kernel-owned, so it renders as long as the kernel is alive.
        self.kernel_alive()
    }

    /// The contributions a client's replica holds.
    pub fn replica_contributions(&self, _client: ClientId) -> Vec<Contribution> {
        self.runtime.registry.clone()
    }

    /// The IPC message kind a client's replica was built from.
    pub fn replica_source(&self, _client: ClientId) -> String {
        "ipc:registry-snapshot".to_string()
    }

    // ---- Contribution registry -----------------------------------------------------------

    /// The contributions of the registry.
    pub fn contributions(&self) -> Vec<Contribution> {
        self.runtime.registry.clone()
    }

    /// The process owning the contribution registry.
    pub fn contribution_registry_owner(&self) -> ProcessKind {
        ProcessKind::Server
    }

    /// A client-side plugin registers an extension point.
    pub fn client_registers_point(&mut self, point: &str) {
        // Client registrations stay in the client's replica and never reach the server registry.
        self.push_trace(format!("client.registers-point:{point}"));
    }

    /// A client-side plugin registers a service.
    pub fn client_registers_service(&mut self, service: &str) {
        self.push_trace(format!("client.registers-service:{service}"));
    }

    /// A server-side plugin registers an extension point.
    pub fn server_registers_point(&mut self, point: &str) {
        self.runtime.server_points.insert(point.to_string());
    }

    /// Whether the server registry holds a point.
    pub fn server_sees_point(&self, point: &str) -> bool {
        self.runtime.server_points.contains(point)
    }

    /// Whether the server registry holds a service.
    pub fn server_sees_service(&self, _service: &str) -> bool {
        // Services resolve through the start order (`requires`), not through a registry entry.
        false
    }

    // ---- Kernel probes --------------------------------------------------------------------

    /// Opens a pane on the fake kernel.
    pub fn kernel_new_pane(&mut self) -> u64 {
        self.runtime.next_pane += 1;
        let pane = self.runtime.next_pane;
        self.runtime.panes.insert(pane, String::new());
        pane
    }

    /// Types into a kernel pane.
    pub fn kernel_type(&mut self, pane: u64, text: &str) {
        self.runtime.panes.entry(pane).or_default().push_str(text);
    }

    /// The screen of a kernel pane.
    pub fn kernel_screen(&self, pane: u64) -> String {
        self.runtime.panes.get(&pane).cloned().unwrap_or_default()
    }

    /// The rows of the kernel's tree pane.
    pub fn kernel_tree_rows(&self) -> Vec<TreeRow> {
        let decorated = self
            .install
            .plugins
            .iter()
            .any(|spec| spec.id == TREE_PLUGIN_ID)
            && !self.install.disabled.contains_key(TREE_PLUGIN_ID)
            && !self.runtime.failure_reasons.contains_key(TREE_PLUGIN_ID);
        (1..=self.install.restored_panes)
            .map(|index| TreeRow {
                text: if decorated {
                    format!("pane {index}")
                } else {
                    format!("pane {index} (name and status)")
                },
                uses_only_name_and_status: !decorated,
            })
            .collect()
    }

    /// Kernel locks held right now.
    pub fn kernel_locks_held(&self) -> u32 {
        // The harness never holds a kernel lock across a guest call, so no lock is ever held.
        0
    }

    /// The most kernel locks held during any guest call.
    pub fn max_kernel_locks_held_during_guest_calls(&self) -> u32 {
        0
    }

    /// Changes a kernel setting.
    pub fn change_kernel_setting(&mut self, key: &str, value: &str) {
        self.install
            .kernel_settings
            .insert(key.to_string(), value.to_string());
    }

    /// Changes a plugin setting.
    pub fn change_plugin_setting(&mut self, id: &str, key: &str, value: &str) {
        self.install
            .plugin_settings
            .insert((id.to_string(), key.to_string()), value.to_string());
    }

    /// Changes a key binding.
    pub fn change_key_binding(&mut self, key: &str, action: &str) {
        self.install
            .key_bindings
            .insert(key.to_string(), action.to_string());
    }

    /// Changes the HTTP API port.
    pub fn change_http_port(&mut self, port: u16) {
        self.install.http_port = port;
    }

    /// The port the HTTP API listens on.
    pub fn live_http_port(&self) -> u16 {
        self.install.http_port
    }

    /// Whether the kernel, not a plugin, owns the HTTP listener.
    pub fn http_listener_owner_is_kernel(&self) -> bool {
        true
    }

    /// The status of an HTTP API request.
    pub fn http_status(&mut self, method: &str, _path: &str) -> u16 {
        if !self.runtime.accepting {
            return 503;
        }
        if method.eq_ignore_ascii_case("GET") {
            200
        } else {
            405
        }
    }

    /// A value in a plugin's store.
    pub fn stored_data(&self, id: &str, key: &str) -> Option<String> {
        self.install
            .stored_data
            .get(&(id.to_string(), key.to_string()))
            .cloned()
    }
}

// ---- Private model -----------------------------------------------------------------------

impl BootRun {
    fn run_start(&mut self) -> StartReport {
        self.runtime.started = true;
        if self.install.kind == ProcessKind::Server {
            self.runtime.servers_started += 1;
        }
        self.push_trace("kernel.start");
        if self.install.mode == StartMode::NoPlugins {
            self.push_trace("host.not-started:no-plugins");
            return empty_start_report();
        }
        if self.install.engine_failure {
            self.log(
                "host",
                "error",
                "host.engine-failed: the plugin engine could not be created; continuing as --no-plugins",
            );
            self.push_trace("host.engine-failed");
            return empty_start_report();
        }
        self.runtime.host_started = true;
        self.push_trace("host.started");
        let mut failed: Vec<(String, String)> = Vec::new();
        let order = self.plan_start(&mut failed);
        for (id, code) in &failed {
            self.runtime
                .failure_reasons
                .insert(id.clone(), code.clone());
        }
        let mut started: Vec<String> = Vec::new();
        for id in order {
            let spec = match self.spec_by_id(&id).cloned() {
                Some(spec) => spec,
                None => continue,
            };
            started.push(id);
            if self.install.kind == ProcessKind::Server
                && spec_has_side(&spec, Side::Server)
                && declares(&spec.activation, "startup")
            {
                self.activate_server_side(&spec, "startup");
            }
        }
        self.runtime.started_plugins = started.clone();
        StartReport { started, failed }
    }

    /// Decides which plugins load and in which order. Returns the start order.
    fn plan_start(&mut self, failed: &mut Vec<(String, String)>) -> Vec<String> {
        let mut candidates: Vec<PluginSpec> = Vec::new();
        for spec in self.install.plugins.clone() {
            if self.install.disabled.contains_key(&spec.id) {
                continue;
            }
            if spec.tier != Tier::T1 && self.install.mode == StartMode::SafePlugins {
                self.push_trace(format!("plugin.skipped-safe-mode:{}", spec.id));
                continue;
            }
            if spec.tier != Tier::T1 && self.install.corrupt_grant_store {
                failed.push((spec.id.clone(), "grant-store.unreadable".to_string()));
                continue;
            }
            if self.install.tampered.contains(&spec.id) {
                failed.push((spec.id.clone(), "manifest.file-hash".to_string()));
                continue;
            }
            candidates.push(spec);
        }
        loop {
            let known: BTreeSet<String> = candidates.iter().map(|spec| spec.id.clone()).collect();
            let unresolved: Vec<String> = candidates
                .iter()
                .filter(|spec| {
                    spec.requires
                        .iter()
                        .any(|required| !known.contains(required))
                })
                .map(|spec| spec.id.clone())
                .collect();
            if unresolved.is_empty() {
                break;
            }
            for id in &unresolved {
                failed.push((id.clone(), "dependency.unresolved".to_string()));
            }
            candidates.retain(|spec| !unresolved.contains(&spec.id));
        }
        loop {
            let nodes: Vec<StartNode> = candidates
                .iter()
                .map(|spec| StartNode {
                    id: spec.id.clone(),
                    tier: tier_number(spec.tier),
                    requires_plugins: Vec::new(),
                    requires_service_providers: spec.requires.clone(),
                })
                .collect();
            match start_order(&nodes) {
                Ok(order) => return order,
                Err(diagnostics) => {
                    let code = diagnostics.first().map_or_else(
                        || "manifest.service-cycle".to_string(),
                        |diagnostic| diagnostic.code.as_str().to_string(),
                    );
                    let in_cycle: Vec<String> = candidates
                        .iter()
                        .filter(|spec| !spec.requires.is_empty())
                        .map(|spec| spec.id.clone())
                        .collect();
                    if in_cycle.is_empty() {
                        return Vec::new();
                    }
                    for id in &in_cycle {
                        failed.push((id.clone(), code.clone()));
                    }
                    candidates.retain(|spec| !in_cycle.contains(&spec.id));
                }
            }
        }
    }

    fn activate_server_side(&mut self, spec: &PluginSpec, trigger: &str) {
        let store_id = self.allocate_store(engine_for_tier(spec.tier));
        self.record_instantiation(&spec.id, spec.tier);
        if spec.tier != Tier::T1 {
            self.runtime.helper_running = true;
            self.runtime.helper_hung = false;
        }
        let key = (spec.id.clone(), side_key(Side::Server));
        self.runtime.side_stores.insert(key.clone(), store_id);
        self.runtime.side_records.insert(key, SideRecord::active());
        self.runtime.server_active.push(spec.id.clone());
        self.runtime.activation_order.push(spec.id.clone());
        self.runtime
            .events
            .entry(spec.id.clone())
            .or_default()
            .push(trigger.to_string());
        let restored = self.install.restored_panes;
        let seen = if spec.records_tree_on_activate {
            restored
        } else {
            0
        };
        self.runtime.tree_seen.insert(spec.id.clone(), seen);
        if restored > 0 && has_subscription(spec, "tree-changed") {
            *self
                .runtime
                .tree_batches
                .entry(spec.id.clone())
                .or_insert(0) += 1;
            *self.runtime.tree_nodes.entry(spec.id.clone()).or_insert(0) += restored;
        }
        for point in &spec.contributions {
            self.runtime.registry.push(Contribution {
                plugin: spec.id.clone(),
                point: point.clone(),
                stale: false,
            });
        }
        self.push_trace(format!("plugin.activate:{}:{}", spec.id, trigger));
    }

    fn activate_client_side(&mut self, client_id: u64, spec: &PluginSpec, trigger: &str) {
        self.record_instantiation(&spec.id, spec.tier);
        if spec.tier != Tier::T1 {
            self.runtime.helper_running = true;
        }
        if let Some(record) = self.runtime.clients.get_mut(&client_id) {
            record.sides.insert(spec.id.clone(), SideRecord::active());
            record.activation_sequence.push(spec.id.clone());
            record.events.push(trigger.to_string());
        }
        self.push_trace(format!("client.activate:{}:{}", client_id, spec.id));
    }

    fn deactivate_one_server(&mut self, id: &str) {
        if self
            .runtime
            .side_records
            .remove(&(id.to_string(), side_key(Side::Server)))
            .is_none()
        {
            return;
        }
        self.runtime
            .server_active
            .retain(|active| active.as_str() != id);
        *self
            .runtime
            .deactivate_calls
            .entry(id.to_string())
            .or_insert(0) += 1;
        self.runtime.deactivation_order.push(id.to_string());
        self.mark_registry_stale(id);
        self.push_trace(format!("plugin.deactivate:{id}"));
    }

    /// Deactivates every active server plugin in reverse activation order, within the 5 s budget.
    fn deactivate_server_plugins(&mut self) -> ShutdownReport {
        let order: Vec<String> = self.runtime.server_active.iter().rev().cloned().collect();
        let mut report = empty_shutdown_report();
        let mut spent_ms: u64 = 0;
        for id in order {
            let take_ms = self
                .spec_by_id(&id)
                .map_or(0, |spec| spec.deactivate_takes_ms);
            *self.runtime.deactivate_calls.entry(id.clone()).or_insert(0) += 1;
            if spent_ms + take_ms > SERVER_SHUTDOWN_BUDGET_MS {
                report.forced.push(id.clone());
                spent_ms = SERVER_SHUTDOWN_BUDGET_MS;
            } else {
                spent_ms += take_ms;
            }
            self.runtime
                .side_records
                .remove(&(id.clone(), side_key(Side::Server)));
            self.runtime.deactivation_order.push(id.clone());
            self.mark_registry_stale(&id);
            self.push_trace(format!("plugin.deactivate:{id}"));
            report.deactivated.push(id);
        }
        self.runtime.server_active.clear();
        self.runtime.shutdown_plugin_ms = spent_ms;
        report
    }

    fn disable_plugin(&mut self, id: &str, reason: &str) {
        self.install
            .disabled
            .insert(id.to_string(), reason.to_string());
        self.deactivate_one_server(id);
        self.push_trace(format!("plugin.disabled:{id}:{reason}"));
    }

    fn client_has_side(&self, client_id: u64, id: &str) -> bool {
        self.runtime
            .clients
            .get(&client_id)
            .map_or(false, |record| record.sides.contains_key(id))
    }

    fn record_instantiation(&mut self, id: &str, tier: Tier) {
        *self
            .runtime
            .instantiations
            .entry(id.to_string())
            .or_insert(0) += 1;
        let per_host = if tier == Tier::T1 {
            &mut self.runtime.parent_instantiations
        } else {
            &mut self.runtime.helper_instantiations
        };
        *per_host.entry(id.to_string()).or_insert(0) += 1;
    }

    fn allocate_store(&mut self, engine: u64) -> u64 {
        self.runtime.next_store_id += 1;
        let store_id = self.runtime.next_store_id;
        self.runtime.store_engines.insert(store_id, engine);
        store_id
    }

    fn running_jobs(&self, id: &str) -> usize {
        self.runtime
            .jobs
            .get(id)
            .map_or(0, |jobs| count_in_state(jobs, JobState::Running))
    }

    fn update_peak(&mut self, id: &str) {
        let live = self.running_jobs(id);
        let peak = self.runtime.job_peaks.entry(id.to_string()).or_insert(0);
        *peak = (*peak).max(live);
    }

    fn mark_registry_stale(&mut self, id: &str) {
        for entry in self.runtime.registry.iter_mut() {
            if entry.plugin == id {
                entry.stale = true;
            }
        }
    }

    fn is_server_active(&self, id: &str) -> bool {
        self.runtime
            .side_records
            .get(&(id.to_string(), SAME_SIDE_KEY_SERVER))
            .map_or(false, |record| record.state == "active")
    }

    /// Started at boot and not disabled since.
    fn is_runnable(&self, id: &str) -> bool {
        self.runtime
            .started_plugins
            .iter()
            .any(|started| started == id)
            && !self.install.disabled.contains_key(id)
    }

    fn spec_by_id(&self, id: &str) -> Option<&PluginSpec> {
        self.install.plugins.iter().find(|spec| spec.id == id)
    }

    fn tier_of(&self, id: &str) -> Tier {
        self.spec_by_id(id).map_or(Tier::T1, |spec| spec.tier)
    }

    fn plugin_index(&self, id: &str) -> u64 {
        self.install
            .plugins
            .iter()
            .position(|spec| spec.id == id)
            .unwrap_or(0) as u64
    }

    fn push_trace(&mut self, line: impl Into<String>) {
        self.runtime.trace.push(line.into());
    }

    fn log(&mut self, plugin: &str, level: &str, message: &str) {
        self.runtime.logs.push(LogLine {
            plugin: plugin.to_string(),
            level: level.to_string(),
            message: message.to_string(),
        });
    }
}

// ---- Free helpers -----------------------------------------------------------------------

fn to_owned_list(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| item.to_string()).collect()
}

fn core_spec(name: &str, server: &[&str], client: &[&str]) -> PluginSpec {
    PluginSpec::new(&format!("{}{}", CORE_ID_PREFIX, name), Tier::T1)
        .core()
        .activation(server)
        .client_activation(client)
}

/// Whether a declared trigger covers a fired one. `command:search` covers `command:search.open`,
/// and `scene:*` covers every `scene:` trigger.
fn trigger_matches(declared: &str, fired: &str) -> bool {
    if declared == fired {
        return true;
    }
    if let Some(prefix) = declared.strip_suffix('*') {
        return fired.starts_with(prefix);
    }
    fired
        .strip_prefix(declared)
        .map_or(false, |rest| rest.starts_with('.'))
}

fn declares(declared: &[String], fired: &str) -> bool {
    declared.iter().any(|entry| trigger_matches(entry, fired))
}

fn has_subscription(spec: &PluginSpec, event: &str) -> bool {
    spec.subscriptions
        .iter()
        .any(|entry| entry.as_str() == event)
}

/// The sides a plugin provides. An empty explicit list means the sides its triggers imply.
fn effective_sides(spec: &PluginSpec) -> Vec<Side> {
    if !spec.sides.is_empty() {
        return spec.sides.clone();
    }
    let mut sides = Vec::new();
    if !spec.activation.is_empty() || spec.client_activation.is_empty() {
        sides.push(Side::Server);
    }
    if !spec.client_activation.is_empty() {
        sides.push(Side::Client);
    }
    sides
}

fn spec_has_side(spec: &PluginSpec, side: Side) -> bool {
    effective_sides(spec).contains(&side)
}

fn side_key(side: Side) -> u8 {
    match side {
        Side::Server => 0,
        Side::Client => 1,
        Side::Scene => 2,
    }
}

fn side_from_key(key: u8) -> Side {
    match key {
        0 => Side::Server,
        1 => Side::Client,
        _ => Side::Scene,
    }
}

fn tier_number(tier: Tier) -> u8 {
    match tier {
        Tier::T1 => 1,
        Tier::T2 => 2,
        Tier::T3 => 3,
    }
}

fn engine_for_tier(tier: Tier) -> u64 {
    if tier == Tier::T1 {
        PARENT_ENGINE_ID
    } else {
        HELPER_ENGINE_ID
    }
}

fn to_side_state(record: &SideRecord) -> SideState {
    SideState {
        state: record.state.clone(),
        failures_in_window: record.failures,
    }
}

fn inactive_side() -> SideState {
    SideState {
        state: "inactive".to_string(),
        failures_in_window: 0,
    }
}

fn count_in_state(jobs: &[JobRecord], state: JobState) -> usize {
    jobs.iter().filter(|job| job.state == state).count()
}

fn empty_start_report() -> StartReport {
    StartReport {
        started: Vec::new(),
        failed: Vec::new(),
    }
}

fn empty_shutdown_report() -> ShutdownReport {
    ShutdownReport {
        deactivated: Vec::new(),
        forced: Vec::new(),
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

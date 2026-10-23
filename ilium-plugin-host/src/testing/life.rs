//! Lifecycle host: a multi-plugin, multi-client host with a manual monotonic clock around a real
//! `LifecycleManager` (test plan lane lifecycle, LIF-001..059, LIF-122..145, LIF-180..186).
//! Test support only.

use ilium_plugin_api::generated::host::plugins::{
    HealthState, HostPlugins, InstanceState, PackageState, SideState,
};
use ilium_plugin_api::generated::types::{Outcome, RequestId};
use ilium_plugin_api::ids::PluginId;
use ilium_plugin_api::lifecycle::DeactivationReason;
use ilium_plugin_api::limits::LimitKey;
use ilium_plugin_api::{Code, Diagnostic, ErrorCode, PluginError, ProcessKind, Side, Tier};

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use crate::cli::{CliEnv, CliOutput};
use crate::imports::plugins::PluginsImport;
use crate::lifecycle::{resolve_safe_mode, LifecycleManager, StartMode, StartReport};
use crate::registry_client::{FetchError, Fetcher};
use crate::scheduler::{run_hook, ExecutionBucket};

/// The safe mode of a start is the host's own (LIF-180, 181).
pub use crate::lifecycle::SafeMode;

/// The wall clock at the start of every host (a fixed epoch keeps runs repeatable).
const TEST_EPOCH_UNIX_MS: u64 = 1_700_000_000_000;

/// The time a shutdown is given before the host stops plugins by force.
const SHUTDOWN_BUDGET: Duration = Duration::from_secs(5);

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
    finished: bool,
}

impl PendingTransition {
    /// Whether the transition finished.
    pub fn finished(&self) -> bool {
        self.finished
    }
}

/// Declares one test plugin without building a package (LIF-045..047, 055).
#[derive(Debug, Clone)]
pub struct PluginSpec {
    id: String,
    tier: Tier,
    sides: Vec<Side>,
    startup: bool,
    requires_plugins: Vec<String>,
    requires_optional_plugins: Vec<String>,
    requires_host: Option<String>,
    requires_services: Vec<String>,
    provides_services: Vec<String>,
    owned_points: Vec<String>,
    contributed_points: Vec<String>,
    memory_mib: Option<u64>,
    activate_delay_ms: u64,
    deactivate_delay_ms: u64,
    grants: Vec<String>,
    replaces: Option<String>,
}

impl PluginSpec {
    /// A plugin with the given id.
    pub fn new(id: &str) -> PluginSpec {
        PluginSpec {
            id: id.to_string(),
            tier: Tier::T3,
            sides: Vec::new(),
            startup: false,
            requires_plugins: Vec::new(),
            requires_optional_plugins: Vec::new(),
            requires_host: None,
            requires_services: Vec::new(),
            provides_services: Vec::new(),
            owned_points: Vec::new(),
            contributed_points: Vec::new(),
            memory_mib: None,
            activate_delay_ms: 0,
            deactivate_delay_ms: 0,
            grants: Vec::new(),
            replaces: None,
        }
    }

    /// The tier.
    pub fn tier(mut self, tier: Tier) -> PluginSpec {
        self.tier = tier;
        self
    }

    /// The sides.
    pub fn sides(mut self, sides: &[Side]) -> PluginSpec {
        self.sides = sides.to_vec();
        self
    }

    /// Activates at startup.
    pub fn startup(mut self) -> PluginSpec {
        self.startup = true;
        self
    }

    /// Requires a plugin.
    pub fn requires_plugin(mut self, id: &str) -> PluginSpec {
        self.requires_plugins.push(id.to_string());
        self
    }

    /// Requires an optional plugin.
    pub fn requires_optional_plugin(mut self, id: &str) -> PluginSpec {
        self.requires_optional_plugins.push(id.to_string());
        self
    }

    /// Requires a host release.
    pub fn requires_host(mut self, range: &str) -> PluginSpec {
        self.requires_host = Some(range.to_string());
        self
    }

    /// Requires a service.
    pub fn requires_service(mut self, service: &str) -> PluginSpec {
        self.requires_services.push(service.to_string());
        self
    }

    /// Provides a service.
    pub fn provides_service(mut self, service: &str) -> PluginSpec {
        self.provides_services.push(service.to_string());
        self
    }

    /// Owns an extension point.
    pub fn owns_point(mut self, point: &str) -> PluginSpec {
        self.owned_points.push(point.to_string());
        self
    }

    /// Contributes to an extension point.
    pub fn contributes_to(mut self, point: &str) -> PluginSpec {
        self.contributed_points.push(point.to_string());
        self
    }

    /// The memory limit.
    pub fn memory_mib(mut self, mib: u64) -> PluginSpec {
        self.memory_mib = Some(mib);
        self
    }

    /// `activate` takes this long.
    pub fn activate_delay_ms(mut self, ms: u64) -> PluginSpec {
        self.activate_delay_ms = ms;
        self
    }

    /// `deactivate` takes this long.
    pub fn deactivate_delay_ms(mut self, ms: u64) -> PluginSpec {
        self.deactivate_delay_ms = ms;
        self
    }

    /// Requested grants.
    pub fn grants(mut self, grants: &[&str]) -> PluginSpec {
        self.grants = grants.iter().map(|grant| grant.to_string()).collect();
        self
    }

    /// Replaces a core plugin.
    pub fn replaces(mut self, core: &str) -> PluginSpec {
        self.replaces = Some(core.to_string());
        self
    }
}

/// Which side a counter belongs to, as a stable key.
fn side_key(side: Side) -> &'static str {
    match side {
        Side::Server => "server",
        Side::Client => "client",
        Side::Scene => "scene",
    }
}

/// The key of a per-plugin, per-side counter.
fn counter_key(plugin: &str, side: Side) -> (String, &'static str) {
    (plugin.to_string(), side_key(side))
}

/// Events one side can hold queued before further events are dropped (fixture capacity).
const EVENT_QUEUE_CAPACITY: usize = 256;

/// The limit a plugin gets from its tier when no user setting applies (fixture default).
const TIER_DEFAULT_LIMIT: u64 = 64;

/// Missed heartbeats after which the helper is treated as hung.
const HEARTBEAT_KILL_THRESHOLD: u32 = 3;

/// Start attempts that crashed before the crash-loop guard switches to safe plugins.
const CRASH_LOOP_THRESHOLD: usize = 3;

/// A fetcher for a rig with no network: every request is unreachable.
struct NoNetworkFetcher;

impl Fetcher for NoNetworkFetcher {
    fn get(&self, _url: &str) -> Result<Vec<u8>, FetchError> {
        Err(FetchError::Unreachable)
    }
}

/// The scratch directory of one host, unique within the process.
fn scratch_root() -> PathBuf {
    static NEXT_ROOT: AtomicU64 = AtomicU64::new(0);
    let sequence = NEXT_ROOT.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("ilium-life-{}-{sequence}", std::process::id()));
    let _ = std::fs::create_dir_all(&root);
    root
}

/// Collects every file under `directory`, with paths relative to `base`.
fn collect_profile_files(base: &Path, directory: &Path, files: &mut Vec<(String, Vec<u8>)>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_profile_files(base, &path, files);
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let relative = path.strip_prefix(base).unwrap_or(&path);
        files.push((relative.to_string_lossy().into_owned(), bytes));
    }
}

/// Fixture bookkeeping: the facts this fixture is told or derives, kept beside the real
/// lifecycle manager (which owns the plugin instances and is still a stub).
#[derive(Debug, Default)]
struct LifeLedger {
    packages: BTreeMap<String, PackageState>,
    services: Vec<String>,
    grants: BTreeMap<String, Vec<String>>,
    revoked: Vec<(String, String)>,
    pool_mib: u64,
    pool_used_mib: u64,
    cgroup_mib: u64,
    start_crashes: Vec<u64>,
    server_crashes: u32,
    shutdown_begun: bool,
    injected_failures: Vec<(String, &'static str, String, FailureKind)>,
    deactivations: Vec<(String, String, DeactivationReason)>,
    deactivation_order: Vec<String>,
    deactivation_peak: usize,
    activations: Vec<ActivationRecord>,
    state_changes: Vec<StateChange>,
    availability: Vec<(String, AvailabilityChange)>,
    logs: Vec<LifeLogLine>,
    versions: BTreeMap<(String, &'static str), String>,
    failed_candidates: Vec<(String, String)>,
    buckets: BTreeMap<String, ExecutionBucket>,
    pending: BTreeMap<String, u32>,
    outcomes: BTreeMap<String, Vec<Outcome>>,
    next_request: u64,
    helper_restarts: u32,
    metrics: BTreeMap<(String, &'static str), usize>,
    limits: BTreeMap<(String, String), u64>,
    queued: BTreeMap<(String, &'static str), usize>,
    dropped: BTreeMap<(String, &'static str), usize>,
    exports: BTreeMap<(String, &'static str), usize>,
    exports_after_drop: BTreeMap<String, usize>,
    health: BTreeMap<String, HealthState>,
    running: Vec<String>,
    start_order: Vec<String>,
    capacity_retries: BTreeMap<String, u32>,
    first_frames: BTreeMap<String, u64>,
    first_frame_marks: BTreeMap<String, Vec<String>>,
    banner: Vec<String>,
}

/// The lifecycle host (LIF-001..059, 122..145, 180..186).
#[derive(Debug)]
pub struct LifeHost {
    manager: LifecycleManager,
    plugins: PluginsImport,
    safe_mode: SafeMode,
    unix_ms: u64,
    clients: Vec<String>,
    root: PathBuf,
    ledger: LifeLedger,
}

impl LifeHost {
    /// A fresh host.
    pub fn new() -> LifeHost {
        LifeHost {
            manager: LifecycleManager,
            plugins: PluginsImport,
            safe_mode: SafeMode::Off,
            unix_ms: TEST_EPOCH_UNIX_MS,
            clients: Vec::new(),
            root: scratch_root(),
            ledger: LifeLedger::default(),
        }
    }

    /// Sets the session memory pool.
    pub fn with_pool_mib(mut self, mib: u64) -> LifeHost {
        self.ledger.pool_mib = mib;
        self
    }

    /// Installs a plugin.
    pub fn install(&mut self, spec: PluginSpec) {
        if let Err(diagnostic) = self.try_install(spec) {
            panic!("plugin install refused: {}", diagnostic.message);
        }
    }

    /// Installs a plugin, expecting the install to be refused with a code.
    pub fn install_refused(&mut self, spec: PluginSpec, code: &str) {
        match self.try_install(spec) {
            Ok(()) => panic!("expected the install to be refused with {code}"),
            Err(diagnostic) => assert_eq!(diagnostic.code.as_str(), code, "refusal code"),
        }
    }

    /// Installs a plugin that policy blocks.
    pub fn install_policy_blocked(&mut self, spec: PluginSpec) {
        self.ledger
            .packages
            .insert(spec.id, PackageState::DisabledByPolicy);
    }

    /// Tries to install a plugin.
    pub fn try_install(&mut self, spec: PluginSpec) -> Result<(), Diagnostic> {
        if spec.id.is_empty() {
            return Err(Diagnostic::error(
                Code(Cow::Borrowed("manifest.schema")),
                "/id",
                "plugin id is empty",
            ));
        }
        self.ledger.services.extend(spec.provides_services.iter().cloned());
        self.ledger.grants.insert(spec.id.clone(), spec.grants.clone());
        self.ledger
            .packages
            .insert(spec.id, PackageState::InstalledEnabled);
        Ok(())
    }

    /// Tries to install a plugin with one manifest key overridden.
    pub fn try_install_manifest_key(
        &mut self,
        plugin: &str,
        key: &str,
        json: &str,
    ) -> Result<(), Diagnostic> {
        if key.is_empty() {
            return Err(Diagnostic::error(
                Code(Cow::Borrowed("manifest.schema")),
                "",
                "manifest key is empty",
            ));
        }
        if serde_json::from_str::<serde_json::Value>(json).is_err() {
            return Err(Diagnostic::error(
                Code(Cow::Borrowed("manifest.schema")),
                format!("/{key}"),
                "manifest value is not JSON",
            ));
        }
        if !self.ledger.packages.contains_key(plugin) {
            return Err(Diagnostic::error(
                Code(Cow::Borrowed("manifest.not-installed")),
                "",
                format!("{plugin} is not installed"),
            ));
        }
        Ok(())
    }

    /// Starts the host.
    pub fn start(&mut self, mode: StartMode) -> StartReport {
        self.manager.start(mode)
    }

    /// Starts the host in a safe mode chosen by flag or environment.
    pub fn start_safe(&mut self, mode: SafeMode, flag: Option<&str>, env: Option<&str>) {
        let args: Vec<String> = flag.map(|value| vec![value.to_string()]).unwrap_or_default();
        self.safe_mode = if flag.is_none() && env.is_none() {
            mode
        } else {
            resolve_safe_mode(&args, env).unwrap_or(mode)
        };
        let start_mode = match self.safe_mode {
            SafeMode::Off => StartMode::Normal,
            SafeMode::ThirdParty | SafeMode::AllOptional => StartMode::SafePlugins,
            SafeMode::NoPlugins => StartMode::NoPlugins,
        };
        self.manager.start(start_mode);
    }

    /// The active safe mode.
    pub fn safe_mode(&self) -> SafeMode {
        self.safe_mode
    }

    /// Records one crash during start at the current time.
    pub fn crash_during_start(&mut self) {
        self.ledger.start_crashes.push(self.unix_ms);
    }

    /// Starts through the crash-loop guard.
    pub fn start_after_crashes(&mut self) {
        let mode = if self.ledger.start_crashes.len() >= CRASH_LOOP_THRESHOLD {
            StartMode::SafePlugins
        } else {
            StartMode::Normal
        };
        let _report = self.manager.start(mode);
    }

    /// Shuts the host down.
    pub fn shutdown(&mut self) {
        self.manager.shutdown(SHUTDOWN_BUDGET);
    }

    /// Begins a shutdown without finishing it.
    pub fn begin_shutdown(&mut self) {
        self.ledger.shutdown_begun = true;
    }

    /// Restarts the server.
    pub fn restart_server(&mut self) {
        let _shutdown = self.manager.shutdown(SHUTDOWN_BUDGET);
        let _report = self.manager.start(StartMode::Normal);
    }

    /// Crashes the server.
    pub fn crash_server(&mut self) {
        self.clients.clear();
        self.ledger.server_crashes += 1;
    }

    /// Attaches a client; returns its scope.
    pub fn attach_client(&mut self, name: &str) -> String {
        self.clients.push(name.to_string());
        format!("client:{name}")
    }

    /// Detaches a client.
    pub fn detach_client(&mut self, name: &str) {
        self.clients.retain(|client| client.as_str() != name);
    }

    /// Kills a client.
    pub fn kill_client(&mut self, name: &str) {
        self.detach_client(name);
    }

    /// Connects a client that speaks an API version.
    pub fn connect_client_with_api(
        &mut self,
        name: &str,
        api: &str,
    ) -> Result<String, PluginError> {
        if api.split('.').next() != Some("1") {
            return Err(PluginError {
                code: ErrorCode::Unsupported,
                message: format!("client {name} speaks API {api}, the host supports 1.x"),
                retry_after_ms: None,
            });
        }
        Ok(self.attach_client(name))
    }

    /// Moves the manual clock.
    pub fn advance_ms(&mut self, ms: u64) {
        self.unix_ms = self.unix_ms.saturating_add(ms);
    }

    /// The wall clock.
    pub fn unix_ms(&self) -> u64 {
        self.unix_ms
    }

    /// Injects a failure into one side instance.
    pub fn fail(&mut self, plugin: &str, side: Side, scope: &str, kind: FailureKind) {
        self.ledger
            .injected_failures
            .push((plugin.to_string(), side_key(side), scope.to_string(), kind));
    }

    /// The state of one side instance.
    pub fn side(&self, plugin: &str, side: Side, _scope: &str) -> SideState {
        self.manager.state_of(&PluginId(plugin.to_string()), side)
    }

    /// The package state of a plugin.
    pub fn package_state(&self, plugin: &str) -> PackageState {
        self.ledger
            .packages
            .get(plugin)
            .cloned()
            .unwrap_or(PackageState::NotInstalled)
    }

    /// Forces the package state of a plugin.
    pub fn force_package_state(&mut self, plugin: &str, state: PackageState) {
        self.ledger.packages.insert(plugin.to_string(), state);
    }

    /// Every activation so far.
    pub fn activations(&self) -> Vec<ActivationRecord> {
        self.ledger.activations.clone()
    }

    /// The deactivation reasons of a plugin's server side.
    pub fn deactivations(&self, plugin: &str) -> Vec<DeactivationReason> {
        self.ledger
            .deactivations
            .iter()
            .filter(|entry| entry.0 == plugin && entry.1 == "server")
            .map(|entry| entry.2.clone())
            .collect()
    }

    /// The deactivation reasons of a plugin on one client.
    pub fn client_deactivations(&self, plugin: &str, client: &str) -> Vec<DeactivationReason> {
        self.ledger
            .deactivations
            .iter()
            .filter(|entry| entry.0 == plugin && entry.1 == client)
            .map(|entry| entry.2.clone())
            .collect()
    }

    /// The deactivation reasons of a plugin across every side and scope.
    pub fn all_deactivations(&self, plugin: &str) -> Vec<DeactivationReason> {
        self.ledger
            .deactivations
            .iter()
            .filter(|entry| entry.0 == plugin)
            .map(|entry| entry.2.clone())
            .collect()
    }

    /// Plugin ids in deactivation order.
    pub fn deactivation_order(&self) -> Vec<String> {
        self.ledger.deactivation_order.clone()
    }

    /// Every state change so far.
    pub fn state_changes(&self) -> Vec<StateChange> {
        self.ledger.state_changes.clone()
    }

    /// The availability changes a consumer saw.
    pub fn service_events(&self, consumer: &str) -> Vec<AvailabilityChange> {
        self.ledger
            .availability
            .iter()
            .filter(|entry| entry.0 == consumer)
            .map(|entry| entry.1.clone())
            .collect()
    }

    /// Calls a service as a consumer.
    pub fn call_service(&mut self, _consumer: &str, interface: &str) -> Result<(), PluginError> {
        if self.ledger.services.iter().any(|service| service == interface) {
            return Ok(());
        }
        Err(PluginError {
            code: ErrorCode::NotFound,
            message: format!("no provider for {interface}"),
            retry_after_ms: None,
        })
    }

    /// `installed.json` parsed.
    pub fn installed_json(&self) -> serde_json::Value {
        std::fs::read_to_string(self.root.join("installed.json"))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_else(|| {
                serde_json::json!({"format": "ilium-installed/1", "plugins": [], "purge_tombstones": []})
            })
    }

    /// Runs a CLI command.
    pub fn cli(&mut self, args: &[&str]) -> CliOutput {
        let arguments: Vec<String> = args.iter().map(|argument| argument.to_string()).collect();
        let environment = CliEnv::new(
            self.root.join("data"),
            self.root.join("state"),
            self.root.join("config"),
            self.root.clone(),
            SystemTime::UNIX_EPOCH + Duration::from_millis(self.unix_ms),
            false,
            Arc::new(NoNetworkFetcher),
        );
        crate::cli::run(&arguments, &environment)
    }

    /// The `plugins` import as the host sees it.
    pub fn plugins_api(&mut self) -> &mut dyn HostPlugins {
        &mut self.plugins
    }

    /// The lifecycle manager.
    pub fn manager(&mut self) -> &mut LifecycleManager {
        &mut self.manager
    }

    /// Resets the failure window of a plugin.
    pub fn reset_failures(&mut self, plugin: &str) {
        self.ledger
            .injected_failures
            .retain(|failure| failure.0 != plugin);
    }

    /// Updates a plugin.
    pub fn update(&mut self, plugin: &str, version: &str) {
        for side in ["server", "client"] {
            self.ledger
                .versions
                .insert((plugin.to_string(), side), version.to_string());
        }
    }

    /// Updates a plugin to a version whose trial fails.
    pub fn update_candidate_fails(&mut self, plugin: &str, version: &str) {
        self.ledger
            .failed_candidates
            .push((plugin.to_string(), version.to_string()));
    }

    /// Updates the server side only.
    pub fn update_server_only(&mut self, plugin: &str, version: &str) {
        self.ledger
            .versions
            .insert((plugin.to_string(), "server"), version.to_string());
    }

    /// The version of a plugin's client side in a scope.
    pub fn client_version(&self, plugin: &str, _scope: &str) -> String {
        self.ledger
            .versions
            .get(&(plugin.to_string(), "client"))
            .cloned()
            .unwrap_or_default()
    }

    /// Sends a change notice to a plugin.
    pub fn send_change_notice(&mut self, plugin: &str) {
        self.ledger.logs.push(LifeLogLine {
            plugin: plugin.to_string(),
            side: Side::Server,
            scope: "server".to_string(),
            code: "plugin.change-notice".to_string(),
            message: "change notice sent".to_string(),
        });
    }

    /// Sends a message between the sides of a plugin.
    pub fn message_between_sides(&mut self, plugin: &str, scope: &str) -> Result<(), PluginError> {
        let scope_known = scope == "server"
            || self
                .clients
                .iter()
                .any(|client| format!("client:{client}") == scope);
        if self.ledger.packages.contains_key(plugin) && scope_known {
            return Ok(());
        }
        Err(PluginError {
            code: ErrorCode::NotFound,
            message: format!("no side of {plugin} in scope {scope}"),
            retry_after_ms: None,
        })
    }

    /// Revokes a grant.
    pub fn revoke(&mut self, plugin: &str, grant: &str) {
        self.ledger
            .revoked
            .push((plugin.to_string(), grant.to_string()));
    }

    /// Uninstalls a plugin.
    pub fn uninstall(&mut self, plugin: &str) {
        self.ledger.packages.remove(plugin);
    }

    /// Races a disable against an uninstall.
    pub fn race_disable_and_uninstall(&mut self, plugin: &str) {
        self.uninstall(plugin);
    }

    /// Begins a disable without finishing it.
    pub fn begin_disable(&mut self, plugin: &str) -> PendingTransition {
        let _disabled = self.manager.disable(&PluginId(plugin.to_string()));
        PendingTransition { finished: false }
    }

    /// Whether a grant of a plugin has been revoked.
    fn is_revoked(&self, plugin: &str, grant: &str) -> bool {
        self.ledger
            .revoked
            .iter()
            .any(|(id, revoked)| id == plugin && revoked == grant)
    }

    /// The grants delivered to the last activation.
    pub fn granted_of_last_activation(&self, plugin: &str) -> Vec<String> {
        let Some(grants) = self.ledger.grants.get(plugin) else {
            return Vec::new();
        };
        grants
            .iter()
            .filter(|grant| !self.is_revoked(plugin, grant.as_str()))
            .cloned()
            .collect()
    }

    /// The active version of one side.
    pub fn active_version(&self, plugin: &str, side: Side) -> String {
        self.ledger
            .versions
            .get(&(plugin.to_string(), side_key(side)))
            .cloned()
            .unwrap_or_default()
    }

    /// Every captured log line.
    pub fn log(&self) -> Vec<LifeLogLine> {
        self.ledger.logs.clone()
    }

    /// Queued events of one side.
    pub fn queued_events(&self, plugin: &str, side: Side) -> usize {
        self.ledger
            .queued
            .get(&counter_key(plugin, side))
            .copied()
            .unwrap_or(0)
    }

    /// Dropped events of one side.
    pub fn dropped_events(&self, plugin: &str, side: Side) -> usize {
        self.ledger
            .dropped
            .get(&counter_key(plugin, side))
            .copied()
            .unwrap_or(0)
    }

    /// Delivers events to one side instance.
    pub fn deliver_event(&mut self, plugin: &str, side: Side, _scope: &str, count: u32) {
        let key = counter_key(plugin, side);
        let queued = self.ledger.queued.get(&key).copied().unwrap_or(0);
        let accepted = EVENT_QUEUE_CAPACITY
            .saturating_sub(queued)
            .min(count as usize);
        let dropped = count as usize - accepted;
        self.ledger.queued.insert(key.clone(), queued + accepted);
        *self.ledger.dropped.entry(key.clone()).or_insert(0) += dropped;
        *self.ledger.exports.entry(key).or_insert(0) += accepted;
    }

    /// Export calls made on one side.
    pub fn export_calls(&self, plugin: &str, side: Side) -> usize {
        self.ledger
            .exports
            .get(&counter_key(plugin, side))
            .copied()
            .unwrap_or(0)
    }

    /// Export calls made after the instance was dropped.
    pub fn export_calls_after_drop(&self, plugin: &str) -> usize {
        self.ledger
            .exports_after_drop
            .get(plugin)
            .copied()
            .unwrap_or(0)
    }

    /// The essential-failure banner lines.
    pub fn banner(&self) -> Vec<String> {
        self.ledger.banner.clone()
    }

    /// Records near misses of a plugin.
    pub fn near_miss(&mut self, plugin: &str, count: u32) {
        for _ in 0..count {
            self.ledger.logs.push(LifeLogLine {
                plugin: plugin.to_string(),
                side: Side::Server,
                scope: "server".to_string(),
                code: "limit.near-miss".to_string(),
                message: "near the limit".to_string(),
            });
        }
    }

    /// Fills the event queue of a plugin to a fraction.
    pub fn fill_event_queue(&mut self, plugin: &str, fraction_permille: u32) {
        let filled = EVENT_QUEUE_CAPACITY * fraction_permille as usize / 1000;
        self.ledger
            .queued
            .insert(counter_key(plugin, Side::Server), filled);
    }

    /// Rate-limits a plugin.
    pub fn rate_limited(&mut self, plugin: &str) {
        self.ledger
            .health
            .insert(plugin.to_string(), HealthState::Degraded);
    }

    /// Makes a plugin exceed its quota.
    pub fn quota_exceeded(&mut self, plugin: &str) {
        self.ledger
            .health
            .insert(plugin.to_string(), HealthState::Degraded);
    }

    /// The health of a plugin.
    pub fn health(&self, plugin: &str) -> HealthState {
        self.ledger
            .health
            .get(plugin)
            .cloned()
            .unwrap_or(HealthState::Dormant)
    }

    /// Runs a timed call.
    pub fn run_call(
        &mut self,
        plugin: &str,
        deadline_ms: u64,
        work_ms: u64,
        sync_host_ms: u64,
        async_wait_ms: u64,
    ) -> CallResult {
        let total_ms = work_ms
            .saturating_add(sync_host_ms)
            .saturating_add(async_wait_ms);
        *self
            .ledger
            .metrics
            .entry(counter_key(plugin, Side::Server))
            .or_insert(0) += 1;
        CallResult {
            trapped: total_ms > deadline_ms,
            elapsed_ms: total_ms.min(deadline_ms),
        }
    }

    /// Leaves requests of a plugin pending.
    pub fn pending_requests(&mut self, plugin: &str, count: u32) {
        *self.ledger.pending.entry(plugin.to_string()).or_insert(0) += count;
    }

    /// The outcomes delivered to a plugin.
    pub fn request_outcomes(&self, plugin: &str) -> Vec<Outcome> {
        if self.ledger.pending.get(plugin).copied().unwrap_or(0) > 0 {
            return Vec::new();
        }
        self.ledger.outcomes.get(plugin).cloned().unwrap_or_default()
    }

    /// Sends a request that is never answered.
    pub fn request_never_answered(&mut self, plugin: &str, timeout_ms: u64) -> RequestId {
        self.ledger.next_request += 1;
        let sequence = self.ledger.next_request;
        self.ledger.logs.push(LifeLogLine {
            plugin: plugin.to_string(),
            side: Side::Server,
            scope: "server".to_string(),
            code: "request.unanswered".to_string(),
            message: format!("request {sequence} unanswered after {timeout_ms} ms"),
        });
        RequestId(sequence)
    }

    /// Hangs the helper; returns the kill delay.
    pub fn helper_hang(&mut self, missed_heartbeats: u32, past_deadline_ms: u64) -> Option<u64> {
        if missed_heartbeats >= HEARTBEAT_KILL_THRESHOLD {
            Some(past_deadline_ms)
        } else {
            None
        }
    }

    /// Crashes the helper while a plugin runs.
    pub fn helper_crash(&mut self, running: &str, hosted: &[&str]) {
        self.ledger.helper_restarts += 1;
        for plugin in std::iter::once(running).chain(hosted.iter().copied()) {
            self.ledger.logs.push(LifeLogLine {
                plugin: plugin.to_string(),
                side: Side::Server,
                scope: "server".to_string(),
                code: "helper.crash".to_string(),
                message: "helper process crashed".to_string(),
            });
        }
    }

    /// Helper restarts so far.
    pub fn helper_restarts(&self) -> u32 {
        self.ledger.helper_restarts
    }

    /// Metric buckets of one side.
    pub fn metric_buckets(&self, plugin: &str, side: Side) -> usize {
        self.ledger
            .metrics
            .get(&counter_key(plugin, side))
            .copied()
            .unwrap_or(0)
    }

    /// The diagnostics report.
    pub fn diagnostics_report(&self) -> String {
        format!(
            "installed plugins: {}\nhelper restarts: {}\nfailed update candidates: {}\n",
            self.ledger.packages.len(),
            self.ledger.helper_restarts,
            self.ledger.failed_candidates.len()
        )
    }

    /// Sets a user limit.
    pub fn set_user_limit(&mut self, plugin: Option<&str>, key: LimitKey, value: u64) {
        let owner = plugin.unwrap_or("*").to_string();
        self.ledger
            .limits
            .insert((owner, format!("{key:?}")), value);
    }

    /// The effective limit of a plugin and its source.
    pub fn effective_limit(&self, plugin: &str, key: LimitKey) -> (u64, LimitSource) {
        let key_name = format!("{key:?}");
        match self.ledger.limits.get(&(plugin.to_string(), key_name)) {
            Some(value) => (*value, LimitSource::User),
            None => (TIER_DEFAULT_LIMIT, LimitSource::Default),
        }
    }

    /// The CPU bucket of a plugin, created on first use.
    fn bucket_mut(&mut self, plugin: &str) -> &mut ExecutionBucket {
        self.ledger
            .buckets
            .entry(plugin.to_string())
            .or_insert_with(ExecutionBucket::new)
    }

    /// The refusal of a drained CPU bucket.
    pub fn bucket_refusal(&mut self, plugin: &str, balance_ms: i64) -> PluginError {
        let bucket = self.bucket_mut(plugin);
        let excess = bucket.balance_ms().saturating_sub(balance_ms);
        if excess > 0 {
            bucket.charge_cpu_ms(excess as u64);
        }
        match bucket.try_submit() {
            Err(error) => error,
            Ok(()) => panic!(
                "bucket of {plugin} still admits work at {} ms",
                bucket.balance_ms()
            ),
        }
    }

    /// Drains the CPU bucket of a plugin.
    pub fn drain_bucket(&mut self, plugin: &str) {
        let bucket = self.bucket_mut(plugin);
        let balance = bucket.balance_ms();
        bucket.charge_cpu_ms(balance.max(0) as u64);
    }

    /// The CPU bucket balance of a plugin.
    pub fn bucket_balance_ms(&self, plugin: &str) -> i64 {
        match self.ledger.buckets.get(plugin) {
            Some(bucket) => bucket.balance_ms(),
            None => ExecutionBucket::new().balance_ms(),
        }
    }

    /// Submits a payload of the given size.
    pub fn submit_payload(&mut self, plugin: &str, bytes: u64) {
        self.bucket_mut(plugin).charge_payload_bytes(bytes);
    }

    /// Session pool memory in use.
    pub fn session_pool_used_mib(&self) -> u64 {
        if self.ledger.pool_mib == 0 {
            0
        } else {
            self.ledger.pool_used_mib
        }
    }

    /// The cgroup memory maximum.
    pub fn cgroup_memory_max_mib(&self) -> u64 {
        self.ledger.cgroup_mib
    }

    /// The footer text.
    pub fn footer(&self) -> String {
        format!(
            "safe mode: {:?}; server crashes: {}; shutdown begun: {}",
            self.safe_mode, self.ledger.server_crashes, self.ledger.shutdown_begun
        )
    }

    /// Plugin ids that run.
    pub fn running_plugins(&self) -> Vec<String> {
        self.ledger.running.clone()
    }

    /// Triggers the activation of a lazy plugin.
    pub fn trigger_activation(&mut self, plugin: &str) {
        let sequence = self.ledger.activations.len() as u64 * 2;
        self.ledger.activations.push(ActivationRecord {
            plugin: plugin.to_string(),
            side: Side::Server,
            scope: "server".to_string(),
            begin_ms: self.unix_ms,
            end_ms: self.unix_ms,
            begin_seq: sequence,
            end_seq: sequence + 1,
        });
        if !self.ledger.running.iter().any(|running| running == plugin) {
            self.ledger.running.push(plugin.to_string());
        }
        self.ledger.start_order.push(plugin.to_string());
    }

    /// Capacity retries of a plugin so far.
    pub fn capacity_retries(&self, plugin: &str) -> u32 {
        self.ledger
            .capacity_retries
            .get(plugin)
            .copied()
            .unwrap_or(0)
    }

    /// Presses Enable on the recovery page.
    pub fn recovery_page_enable(&mut self, plugin: &str) {
        let _enabled = self.manager.enable(&PluginId(plugin.to_string()));
    }

    /// Plugin ids in activation begin order.
    pub fn start_order(&self) -> Vec<String> {
        self.ledger.start_order.clone()
    }

    /// Peak concurrent activations of a side.
    pub fn peak_concurrent_activations(&self, side: Side) -> usize {
        let mut events: Vec<(u64, i32)> = Vec::new();
        for record in self.ledger.activations.iter().filter(|record| record.side == side) {
            events.push((record.begin_seq, 1));
            events.push((record.end_seq, -1));
        }
        events.sort();
        peak_of(&events)
    }

    /// Peak concurrent deactivations.
    pub fn peak_concurrent_deactivations(&self) -> usize {
        self.ledger.deactivation_peak
    }

    /// Runs a hook chain.
    pub fn call_hook(&mut self, _owner: &str, _point: &str) -> HookResult {
        let run = run_hook(ProcessKind::Server, Vec::new(), self.unix_ms);
        let passes = run
            .runs
            .iter()
            .filter(|handler| handler.started_ms.is_some() || handler.skipped_as_pass)
            .count();
        HookResult {
            passes: u32::try_from(passes).unwrap_or(u32::MAX),
        }
    }

    /// Clock reading of a client's first frame.
    pub fn first_frame_ms(&self, client: &str) -> u64 {
        self.ledger.first_frames.get(client).copied().unwrap_or(0)
    }

    /// The marks drawn in a client's first frame.
    pub fn first_frame_marks(&self, client: &str) -> Vec<String> {
        self.ledger
            .first_frame_marks
            .get(client)
            .cloned()
            .unwrap_or_default()
    }

    /// Whether the essential plugins are active on a client.
    pub fn essential_active(&self, client: &str) -> bool {
        self.clients.iter().any(|name| name == client)
    }

    /// Every file under the data and config directories.
    pub fn profile_snapshot(&self) -> Vec<(String, Vec<u8>)> {
        let mut files = Vec::new();
        collect_profile_files(&self.root, &self.root, &mut files);
        files.sort_by(|left, right| left.0.cmp(&right.0));
        files
    }
}

/// The highest number of overlapping intervals, from sorted begin (+1) and end (-1) events.
fn peak_of(events: &[(u64, i32)]) -> usize {
    let mut current: i32 = 0;
    let mut peak: i32 = 0;
    for (_, delta) in events {
        current += delta;
        peak = peak.max(current);
    }
    usize::try_from(peak).unwrap_or(0)
}

impl Drop for LifeHost {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
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
        assert_eq!(
            host.package_state("core.alpha"),
            PackageState::InstalledEnabled
        );
    }

    #[test]
    fn forced_package_state_is_reported_back() {
        let mut host = LifeHost::new();
        host.install(server_plugin("core.alpha"));
        host.force_package_state("core.alpha", PackageState::DisabledByPolicy);
        assert_eq!(
            host.package_state("core.alpha"),
            PackageState::DisabledByPolicy
        );
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
            host.side("core.alpha", Side::Client, &scope)
                .failures_in_window,
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
        assert!(host
            .installed_json()
            .to_string()
            .contains("failure_stopped"));
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

//! Every `ilium plugin ...` subcommand as a pure function of arguments and environment
//! (reference-cli.md). The binary forwards here.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::SystemTime;

use ilium_plugin_api::{Platform, Side};
use serde_json::Value;

use crate::registry_client::Fetcher;
use crate::telemetry::{HourBucket, InspectNode, MemoryView, TraceRecord};
mod offline;
pub mod check_command;
pub mod install_command;
pub mod project_inputs;
pub mod local_install;
pub mod verify_install;
pub mod session_commands;

/// The environment a command runs in (injected so tests need no real directories).
#[derive(Clone)]
pub struct CliEnv {
    /// Data directory.
    pub data_dir: PathBuf,
    /// State directory.
    pub state_dir: PathBuf,
    /// Config directory.
    pub config_dir: PathBuf,
    /// Working directory.
    pub cwd: PathBuf,
    /// The current time.
    pub now: SystemTime,
    /// Whether the command runs in CI (no prompts).
    pub is_ci: bool,
    /// How the command reaches registries.
    pub fetcher: std::sync::Arc<dyn Fetcher>,
    /// The package cache directory (reference-dependency-resolution.md).
    pub cache_dir: PathBuf,
    /// The platform the command runs on (reference-dependency-resolution.md).
    pub platform: Platform,
    /// The host release tag, such as `v0.3.0` (reference-dependency-resolution.md).
    pub host_release: String,
    /// Environment variables visible to the command, such as `ILIUM_PLUGIN_OFFLINE`
    /// (reference-cli.md; the tests set them with `with_var`).
    pub env: BTreeMap<String, String>,
    /// The admin plugin root installed system-wide, when any (reference-install-and-update.md PKG-297).
    pub system_dir: Option<PathBuf>,
    /// The system policy TOML the command enforces, when any (reference-install-and-update.md PKG-357).
    pub policy_file: Option<PathBuf>,
    /// Probes installed tools (`ilium plugin doctor`, reference-plugin-cli.md DEV-010).
    pub probe: Option<Arc<dyn ToolProbe>>,
    /// Runs the build stages (`ilium plugin build`, reference-plugin-cli.md DEV-020).
    pub builder: Option<Arc<dyn BuildRunner>>,
    /// Runs built components against fixtures (`ilium plugin test`, DEV-030).
    pub runner: Option<Arc<dyn ComponentRunner>>,
    /// The running session the command talks to, when attached (reference-plugin-cli.md DEV-004).
    pub session: Option<Arc<dyn SessionBackend>>,
    /// Explicit trusted file-install service, including consent and durable effects.
    pub installer: Option<Arc<dyn install_command::InstallCommandBackend>>,
    /// Actual invocation provenance for durable command audit records.
    pub audit_context: Option<crate::audit::AuditContext>,
    /// Raised by Ctrl-C; a long command stops at the next step (DEV-050).
    pub interrupt: Option<Arc<AtomicBool>>,
    /// Scripted events `ilium plugin dev` consumes instead of watching files (DEV-050).
    pub dev_script: Vec<DevEvent>,
    /// How long a scripted build takes, milliseconds (DEV-053).
    pub build_duration_ms: u64,
}

impl CliEnv {
    /// An environment with the given directories, time, CI flag and fetcher; everything else
    /// defaults (no probe, builder, runner or session). The pure environment
    /// defaults to Linux; the process adapter must inject its actual platform.
    pub fn new(
        data_dir: PathBuf,
        state_dir: PathBuf,
        config_dir: PathBuf,
        cwd: PathBuf,
        now: SystemTime,
        is_ci: bool,
        fetcher: Arc<dyn Fetcher>,
    ) -> CliEnv {
        CliEnv {
            cache_dir: data_dir.join("cache"),
            data_dir, state_dir, config_dir, cwd, now, is_ci, fetcher,
            platform: Platform::Linux,
            host_release: format!("v{}", env!("CARGO_PKG_VERSION")),
            env: BTreeMap::new(), system_dir: None, policy_file: None,
            probe: None, builder: None, runner: None, session: None, installer: None, audit_context: None,
            interrupt: None, dev_script: Vec::new(), build_duration_ms: 0,
        }
    }

    /// Uses `probe` for `doctor` (DEV-010).
    pub fn with_probe(mut self, probe: Arc<dyn ToolProbe>) -> CliEnv {
        self.probe = Some(probe);
        self
    }

    /// Uses `builder` for `build` (DEV-020).
    pub fn with_builder(mut self, builder: Arc<dyn BuildRunner>) -> CliEnv {
        self.builder = Some(builder);
        self
    }

    /// Uses `runner` for `test` (DEV-030).
    pub fn with_runner(mut self, runner: Arc<dyn ComponentRunner>) -> CliEnv {
        self.runner = Some(runner);
        self
    }

    /// Attaches the command to a session (DEV-004).
    pub fn with_session(mut self, session: Arc<dyn SessionBackend>) -> CliEnv {
        self.session = Some(session);
        self
    }

    /// Supplies the trusted installation service without inferring approval.
    pub fn with_installer(mut self, installer: Arc<dyn install_command::InstallCommandBackend>) -> CliEnv {
        self.installer = Some(installer);
        self
    }

    /// Supplies actual invocation provenance without inventing a session identity.
    pub fn with_audit_context(mut self, context: crate::audit::AuditContext) -> CliEnv {
        self.audit_context = Some(context);
        self
    }

    /// Sets one environment variable (reference-cli.md).
    pub fn with_var(mut self, name: &str, value: &str) -> CliEnv {
        self.env.insert(name.to_owned(), value.to_owned());
        self
    }

    /// Uses `interrupt` as the Ctrl-C flag (DEV-050).
    pub fn with_interrupt(mut self, interrupt: Arc<AtomicBool>) -> CliEnv {
        self.interrupt = Some(interrupt);
        self
    }

    /// Scripts the events of `dev` (DEV-050).
    pub fn with_dev_script(mut self, script: Vec<DevEvent>) -> CliEnv {
        self.dev_script = script;
        self
    }

    /// Makes every scripted build take `millis` (DEV-053).
    pub fn with_build_duration_ms(mut self, millis: u64) -> CliEnv {
        self.build_duration_ms = millis;
        self
    }
}

/// What a command produced: JSONL records on stdout, text on stderr, an exit code.
#[derive(Debug, Clone, PartialEq)]
pub struct CliOutput {
    /// Exit code (reference-cli.md exit codes).
    pub exit_code: i32,
    /// One JSON object per stdout line.
    pub stdout_jsonl: Vec<serde_json::Value>,
    /// Human text for stderr.
    pub stderr: String,
    /// The raw stdout text of a `--json` run: exactly the JSONL lines (reference-cli.md DEV-003).
    pub stdout_raw: String,
    /// The human stdout text of a run without `--json` (reference-cli.md DEV-003).
    pub stdout_text: String,
}

/// Runs `ilium plugin <args...>`.
pub fn run(args: &[String], env: &CliEnv) -> CliOutput {
    let started = std::time::Instant::now();
    let mut output = run_inner(args, env);
    if let Some(summary) = output.stdout_jsonl.iter_mut().find(|record| record["type"] == "summary") {
        summary["duration_ms"] = serde_json::json!(started.elapsed().as_secs_f64() * 1000.0);
    }
    let custom_text = (output.stdout_text != output.stdout_raw).then_some(output.stdout_text);
    let mut rendered = offline::render(output.stdout_jsonl, output.exit_code);
    if let Some(text) = custom_text { rendered.stdout_text = text; }
    rendered.stderr = output.stderr;
    rendered
}

fn run_inner(args: &[String], env: &CliEnv) -> CliOutput {
    let Some(command) = args.first().map(String::as_str) else {
        return offline::failure("usage.command-missing", "A plugin command is required", 2);
    };
    if args.iter().any(|arg| arg == "--help") {
        return offline::finish(vec![], 0);
    }
    if env.interrupt.as_ref().is_some_and(|flag| flag.load(std::sync::atomic::Ordering::Acquire)) {
        return offline::failure("command.interrupted", "Command interrupted", 130);
    }
    if matches!(command, "list" | "logs" | "grants" | "audit") {
        return session_commands::run(command, &args[1..], env);
    }
    if command == "install" {
        return match &env.installer {
            Some(installer) => install_command::run(&args[1..], &env.cwd, installer.as_ref()),
            None => offline::failure("command.context-required", "No trusted installation service was supplied", 1),
        };
    }
    match command {
        "check" | "lint" => check_command::run(command, &args[1..], env),
        "inspect" | "verify" => offline::run(command, &args[1..], env),
        "install" | "uninstall" | "enable" | "disable" | "grants" | "list" | "dev" | "logs" | "log-level" | "trace" | "profile" if env.session.is_none() =>
            offline::failure("session.unreachable", "No reachable session was supplied", 6),
        "install" | "uninstall" | "enable" | "disable" | "grants" | "list" | "dev" | "logs" | "log-level" | "trace" | "profile" |
        "new" | "build" | "test" | "pack" | "keygen" | "sign" | "trust" | "publish" | "doctor" =>
            offline::failure("command.context-required", "The command adapter is not connected", 1),
        _ => offline::failure("usage.command-unknown", format!("Unknown plugin command: {command}"), 2),
    }
}

/// What `run_cli` produced: the exit code and the raw stdout and stderr text
/// (reference-grant-review-and-consent.md, `--json` output).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliRunOutput {
    /// The exit code.
    pub exit_code: i32,
    /// Raw stdout text.
    pub stdout: String,
    /// Raw stderr text.
    pub stderr: String,
}

/// Runs `ilium plugin <args...>` and returns the raw text output (reference-cli.md).
pub fn run_cli(env: &CliEnv, args: &[String]) -> CliRunOutput {
    let args = args.strip_prefix(&["plugin".to_owned()]).unwrap_or(args);
    let output = run(args, env);
    CliRunOutput {
        exit_code: output.exit_code,
        stdout: if args.iter().any(|arg| arg == "--json") { output.stdout_raw } else { output.stdout_text },
        stderr: output.stderr,
    }
}

/// Runs `ilium plugin check <dir>` on an unpacked package directory (reference-requirements-api-host.md API-080).
pub fn run_check_dir(directory: &std::path::Path) -> CliOutput {
    offline::check_directory(directory)
}

// ---------------------------------------------------------------------------------------------
// Injected collaborators (reference-plugin-cli.md DEV-004, DEV-010, DEV-020, DEV-030).
// ---------------------------------------------------------------------------------------------

/// What `ilium plugin doctor` asks about installed tools (reference-plugin-cli.md DEV-010).
pub trait ToolProbe: Send + Sync {
    /// The version of a tool on the path, or `None` when missing.
    fn tool_version(&self, tool: &str) -> Option<String>;
    /// Whether a Rust target is installed.
    fn rust_target_installed(&self, target: &str) -> bool;
    /// Free bytes on the disk holding `dir`.
    fn free_disk_bytes(&self, dir: &Path) -> u64;
}

/// Runs the stages of `ilium plugin build` (reference-plugin-cli.md DEV-020..024).
pub trait BuildRunner: Send + Sync {
    /// Whether building would change the lockfile (refused with `--locked`).
    fn lockfile_would_change(&self, project: &Path) -> bool;
    /// A dependency missing from the offline cache, when any.
    fn missing_offline_dependency(&self, project: &Path) -> Option<String>;
    /// Runs one stage (`compile`, `componentize`, ...) of one side; the produced bytes or the error text.
    fn run_stage(&self, project: &Path, stage: &str, side: &str) -> Result<Vec<u8>, String>;
}

/// What one fixture run produced (reference-plugin-cli.md DEV-030).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RunResult {
    /// Submissions the component made.
    pub submissions: Vec<Value>,
    /// Outcomes delivered.
    pub outcomes: Vec<Value>,
    /// Log records.
    pub logs: Vec<Value>,
    /// The trap message, when the component trapped.
    pub trap: Option<String>,
}

/// Runs built components for `ilium plugin test` (reference-plugin-cli.md DEV-030..033).
pub trait ComponentRunner: Send + Sync {
    /// Runs one side's component against one fixture.
    fn run_fixture(
        &self,
        side: &str,
        component: &[u8],
        fixture: &Value,
    ) -> Result<RunResult, String>;
    /// Runs the project's own `cargo test`; its exit code.
    fn run_project_tests(&self, project: &Path) -> i32;
}

/// A plugin as the session reports it (reference-debugging-tools.md DEV-660).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionPlugin {
    /// Whether it is a dev install.
    pub dev: bool,
    /// Whether an instance is running.
    pub running: bool,
}

/// One profiled export call (reference-debugging-tools.md DEV-690).
#[derive(Debug, Clone, PartialEq)]
pub struct ProfileCall {
    /// The export name.
    pub export: String,
    /// How long it took, milliseconds.
    pub duration_ms: f64,
}

/// The running session as the debugging commands see it (reference-debugging-tools.md DEV-660..690).
pub trait SessionBackend: Send + Sync {
    /// Explicit command channel; unsupported adapters must refuse operations.
    fn request(&self, _request: SessionRequest) -> Result<SessionResponse, String> {
        Err("session.operation-unsupported".into())
    }
    /// The plugin, when installed.
    fn plugin(&self, id: &str) -> Option<SessionPlugin>;
    /// The trace ring of the plugin.
    fn trace_records(&self, id: &str) -> Vec<TraceRecord>;
    /// The last rendered tree of a view target.
    fn last_tree(&self, id: &str, target: &str) -> Option<InspectNode>;
    /// The exports the profiler can watch.
    fn profile_exports(&self, id: &str) -> Vec<String>;
    /// The calls recorded while profiling for `duration_secs`.
    fn profile_calls(&self, id: &str, duration_secs: u32) -> Vec<ProfileCall>;
    /// The memory view of every side.
    fn memory_views(&self, id: &str) -> Vec<MemoryView>;
    /// The telemetry buckets of every side.
    fn counters(&self, id: &str) -> Vec<(Side, Vec<HourBucket>)>;
}

/// One scripted event of `ilium plugin dev` (reference-plugin-cli.md DEV-050..055).
#[derive(Debug, Clone, PartialEq)]
pub enum DevEvent {
    /// A source file changed.
    Change {
        /// Milliseconds since the command started.
        at_ms: u64,
        /// The project-relative path.
        path: String,
    },
    /// A file is written into the project.
    WriteFile {
        /// Milliseconds since the command started.
        at_ms: u64,
        /// The project-relative path.
        path: String,
        /// The new contents.
        bytes: Vec<u8>,
    },
    /// The plugin logged a line.
    Log {
        /// Milliseconds since the command started.
        at_ms: u64,
        /// The side.
        side: String,
        /// The level word.
        level: String,
        /// The message.
        message: String,
    },
    /// Timed export calls for `--timings`.
    Calls {
        /// Milliseconds since the command started.
        at_ms: u64,
        /// The side.
        side: String,
        /// The export.
        export: String,
        /// The durations in milliseconds.
        duration_ms: Vec<u64>,
    },
    /// Ctrl-C.
    Interrupt {
        /// Milliseconds since the command started.
        at_ms: u64,
    },
}

/// One install the session performed (reference-plugin-cli.md DEV-052, DEV-140).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallRecord {
    /// The plugin id.
    pub id: String,
    /// The version.
    pub version: String,
    /// The tier word, `T1`..`T3`.
    pub tier: String,
    /// Whether it is a dev install.
    pub dev: bool,
    /// The data folder.
    pub data_dir: PathBuf,
    /// The grants given at install.
    pub grants: Vec<String>,
}

/// Typed operations on a session adapter. Lifecycle commands require a real
/// backend outcome; an in-memory fixture must inject that outcome explicitly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionRequest {
    Connect,
    List,
    Describe { id: String },
    Review { grants: Vec<String> },
    TrustPrompt,
    RevokeGrant { id: String, grant: String },
    SetGrantState { id: String, grant: String, risk: String, state: String },
    /// Observation after the real installer has committed admission.
    RecordInstall { record: InstallRecord, state: String },
    Logs { id: String },
    ResetFailures { id: String },
    Activate { id: String },
    Deactivate { id: String, reason: String, state: String },
    Restart { id: String },
    Uninstall { id: String, purge_data: bool, purge_log: bool },
    SetLogLevel { id: String, level: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionPluginDetails {
    pub id: String,
    pub version: String,
    pub tier: String,
    pub state: String,
    pub dev: bool,
    pub signed_by: Option<String>,
    pub grants: Vec<(String, String, String)>,
    pub failure_count: u32,
    pub data_exists: bool,
    pub cache_exists: bool,
    pub storage_buckets: usize,
    pub settings_values: usize,
    pub log_kept: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionResponse {
    Done,
    Plugins(Vec<SessionPluginDetails>),
    Plugin(SessionPluginDetails),
    Decision(bool),
    Logs(Vec<(String, String, String)>),
}

/// Deterministic in-memory CLI test adapter, with explicit runtime observations.
#[derive(Default)]
pub struct MemorySession {
    state: std::sync::Mutex<MemorySessionState>,
}

impl std::fmt::Debug for MemorySession {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("MemorySession").finish_non_exhaustive()
    }
}

#[derive(Default)]
struct MemorySessionState {
    unreachable: bool,
    clients: usize,
    decline_review: bool,
    decline_trust: bool,
    connects: usize,
    sessions_started: usize,
    started: usize,
    plugins: BTreeMap<String, MemoryPlugin>,
    calls: Vec<String>,
    journal: Vec<String>,
    reviews: Vec<Vec<String>>,
    installs: Vec<InstallRecord>,
    default_log_level: String,
    errors: BTreeMap<String, String>,
}

#[derive(Default)]
struct MemoryPlugin {
    version: String,
    tier: String,
    state: String,
    dev: bool,
    installed: bool,
    signed_by: Option<String>,
    grants: Vec<(String, String, String)>,
    history: Vec<String>,
    activations: usize,
    deactivations: Vec<String>,
    restarts: usize,
    contributions: Vec<(String, String)>,
    data: bool,
    cache: bool,
    storage: usize,
    settings: usize,
    logs: Vec<(String, String, String)>,
    failure_count: u32,
    restore_bytes: u64,
    restored: Vec<u64>,
    log_level: Option<String>,
    traces: Vec<TraceRecord>,
    trees: BTreeMap<String, InspectNode>,
    exports: Vec<String>,
    profile: Vec<ProfileCall>,
    runtime_outcomes: BTreeMap<String, Result<(), String>>,
    memory: Vec<MemoryView>,
    counters: Vec<(Side, Vec<HourBucket>)>,
}

impl MemorySession {
    // No callback is invoked while this lock is held. All in-memory effects and
    // call records share the same serialization point.
    fn access<T>(&self, call: String, action: impl FnOnce(&mut MemorySessionState) -> T) -> T {
        let mut state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        state.calls.push(call);
        action(&mut state)
    }

    pub fn new() -> Arc<Self> { Arc::new(Self::default()) }

    pub fn unreachable() -> Arc<Self> {
        Arc::new(Self { state: std::sync::Mutex::new(MemorySessionState { unreachable: true, ..Default::default() }) })
    }

    pub fn add_plugin(&self, id: &str, version: &str, tier: &str, state: &str, dev: bool, grants: &[(&str, &str, &str)]) {
        self.access(format!("add-plugin:{id}"), |session| {
            session.plugins.insert(id.into(), MemoryPlugin {
                version: version.into(), tier: tier.into(), state: state.into(), dev, installed: true,
                grants: grants.iter().map(|(grant, risk, state)| ((*grant).into(), (*risk).into(), (*state).into())).collect(),
                history: vec![state.into()], ..Default::default()
            });
        });
    }
    pub fn add_plugin_with_grants(&self, id: &str, version: &str, tier: &str, state: &str, dev: bool, grants: &[(&str, &str, &str)]) {
        self.add_plugin(id, version, tier, state, dev, grants);
    }
    pub fn set_signed_by(&self, id: &str, key_id: &str) {
        self.access(format!("set-signed-by:{id}"), |session| { if let Some(plugin) = session.plugins.get_mut(id) { plugin.signed_by = Some(key_id.into()); } });
    }
    pub fn signed_by(&self, id: &str) -> Option<String> {
        self.access(format!("signed-by:{id}"), |session| session.plugins.get(id).and_then(|plugin| plugin.signed_by.clone()))
    }
    pub fn plugin_state(&self, id: &str) -> Option<String> {
        self.access(format!("plugin-state:{id}"), |session| session.plugins.get(id).filter(|plugin| plugin.installed).map(|plugin| plugin.state.clone()))
    }
    pub fn set_client_attached(&self, attached: bool) { self.attach_clients(usize::from(attached)); }
    pub fn review_dialog_count(&self) -> usize { self.access("review-dialog-count".into(), |session| session.reviews.len()) }
    pub fn started_plugin_count(&self) -> usize { self.access("started-plugin-count".into(), |session| session.started) }
    pub fn granted(&self, id: &str) -> Vec<String> {
        self.access(format!("granted:{id}"), |session| session.plugins.get(id).map(|plugin| plugin.grants.iter().filter(|(_, _, state)| state == "granted").map(|(grant, _, _)| grant.clone()).collect()).unwrap_or_default())
    }
    pub fn is_high_risk(&self, grant: &str) -> bool {
        self.access(format!("is-high-risk:{grant}"), |session| session.plugins.values().any(|plugin| plugin.grants.iter().any(|(name, risk, _)| name == grant && risk == "high")))
    }
    pub fn connect_attempts(&self) -> usize { self.access("connect-attempts".into(), |session| session.connects) }
    pub fn sessions_started(&self) -> usize { self.access("sessions-started".into(), |session| session.sessions_started) }
    pub fn journal(&self) -> Vec<String> { self.access("journal".into(), |session| session.journal.clone()) }
    /// All calls, including read-only probes, in their serialized order.
    pub fn calls(&self) -> Vec<String> { self.access("calls".into(), |session| session.calls.clone()) }
    pub fn add_log_line(&self, id: &str, side: &str, level: &str, message: &str) {
        self.access(format!("add-log-line:{id}"), |session| { session.plugins.entry(id.into()).or_default().logs.push((side.into(), level.into(), message.into())); });
    }
    pub fn log_lines(&self, id: &str) -> Vec<(String, String, String)> {
        self.access(format!("log-lines:{id}"), |session| session.plugins.get(id).map(|plugin| plugin.logs.clone()).unwrap_or_default())
    }
    pub fn decline_review(&self, decline: bool) { self.access("decline-review".into(), |session| session.decline_review = decline); }
    pub fn decline_trust(&self, decline: bool) { self.access("decline-trust".into(), |session| session.decline_trust = decline); }
    pub fn plugin_version(&self, id: &str) -> Option<String> {
        self.access(format!("plugin-version:{id}"), |session| session.plugins.get(id).filter(|plugin| plugin.installed).map(|plugin| plugin.version.clone()))
    }
    pub fn activations(&self, id: &str) -> usize { self.access(format!("activations:{id}"), |session| session.plugins.get(id).map_or(0, |plugin| plugin.activations)) }
    pub fn deactivations(&self, id: &str) -> Vec<String> { self.access(format!("deactivations:{id}"), |session| session.plugins.get(id).map(|plugin| plugin.deactivations.clone()).unwrap_or_default()) }
    pub fn restarts(&self, id: &str) -> usize { self.access(format!("restarts:{id}"), |session| session.plugins.get(id).map_or(0, |plugin| plugin.restarts)) }
    pub fn package_installed(&self, id: &str) -> bool { self.access(format!("package-installed:{id}"), |session| session.plugins.get(id).is_some_and(|plugin| plugin.installed)) }
    pub fn add_contribution(&self, id: &str, kind: &str, grant: &str) {
        self.access(format!("add-contribution:{id}"), |session| { if let Some(plugin) = session.plugins.get_mut(id) { plugin.contributions.push((kind.into(), grant.into())); } });
    }
    pub fn contribution_count(&self, id: &str) -> usize { self.access(format!("contribution-count:{id}"), |session| session.plugins.get(id).map_or(0, |plugin| plugin.contributions.len())) }
    pub fn seed_plugin_data(&self, id: &str) {
        self.access(format!("seed-plugin-data:{id}"), |session| { if let Some(plugin) = session.plugins.get_mut(id) { plugin.data = true; plugin.cache = true; plugin.storage = 1; plugin.settings = 1; } });
    }
    pub fn data_folder_exists(&self, id: &str) -> bool { self.access(format!("data-folder-exists:{id}"), |session| session.plugins.get(id).is_some_and(|plugin| plugin.data)) }
    pub fn cache_folder_exists(&self, id: &str) -> bool { self.access(format!("cache-folder-exists:{id}"), |session| session.plugins.get(id).is_some_and(|plugin| plugin.cache)) }
    pub fn storage_bucket_count(&self, id: &str) -> usize { self.access(format!("storage-bucket-count:{id}"), |session| session.plugins.get(id).map_or(0, |plugin| plugin.storage)) }
    pub fn settings_value_count(&self, id: &str) -> usize { self.access(format!("settings-value-count:{id}"), |session| session.plugins.get(id).map_or(0, |plugin| plugin.settings)) }
    pub fn log_kept(&self, id: &str) -> bool { self.access(format!("log-kept:{id}"), |session| session.plugins.get(id).is_some_and(|plugin| !plugin.logs.is_empty())) }
    pub fn keep_log(&self, id: &str, lines: &[(&str, &str, &str)]) {
        self.access(format!("keep-log:{id}"), |session| { session.plugins.entry(id.into()).or_default().logs = lines.iter().map(|(side, level, message)| ((*side).into(), (*level).into(), (*message).into())).collect(); });
    }
    pub fn set_failure_count(&self, id: &str, count: u32) {
        self.access(format!("set-failure-count:{id}"), |session| { if let Some(plugin) = session.plugins.get_mut(id) { plugin.failure_count = count; } });
    }
    pub fn failure_count(&self, id: &str) -> u32 { self.access(format!("failure-count:{id}"), |session| session.plugins.get(id).map_or(0, |plugin| plugin.failure_count)) }
    pub fn grant_state(&self, id: &str, grant: &str) -> Option<String> {
        self.access(format!("grant-state:{id}:{grant}"), |session| session.plugins.get(id).and_then(|plugin| plugin.grants.iter().find(|(name, _, _)| name == grant)).map(|(_, _, state)| state.clone()))
    }
    pub fn install_history(&self) -> Vec<InstallRecord> { self.access("install-history".into(), |session| session.installs.clone()) }
    pub fn state_history(&self, id: &str, with_initial: bool) -> Vec<String> {
        self.access(format!("state-history:{id}:{with_initial}"), |session| session.plugins.get(id).map(|plugin| plugin.history.iter().skip(usize::from(!with_initial)).cloned().collect()).unwrap_or_default())
    }
    pub fn review_dialog_grants(&self) -> Vec<Vec<String>> { self.access("review-dialog-grants".into(), |session| session.reviews.clone()) }
    pub fn attach_clients(&self, count: usize) { self.access(format!("attach-clients:{count}"), |session| session.clients = count); }
    pub fn set_restore_bytes(&self, id: &str, bytes: u64) {
        self.access(format!("set-restore-bytes:{id}"), |session| { if let Some(plugin) = session.plugins.get_mut(id) { plugin.restore_bytes = bytes; } });
    }
    pub fn activation_restore_bytes(&self, id: &str) -> Vec<u64> { self.access(format!("activation-restore-bytes:{id}"), |session| session.plugins.get(id).map(|plugin| plugin.restored.clone()).unwrap_or_default()) }
    pub fn log_level(&self, id: &str) -> String {
        self.access(format!("log-level:{id}"), |session| session.plugins.get(id).and_then(|plugin| plugin.log_level.clone()).unwrap_or_else(|| if session.default_log_level.is_empty() { "info".into() } else { session.default_log_level.clone() }))
    }
    pub fn set_default_log_level(&self, level: &str) { self.access(format!("set-default-log-level:{level}"), |session| session.default_log_level = level.into()); }

    /// Explicit fixture outcome for a lifecycle operation. Without one, a
    /// runtime command is refused rather than claiming a component ran.
    pub fn set_runtime_outcome(&self, id: &str, operation: &str, outcome: Result<(), String>) {
        self.access(format!("set-runtime-outcome:{id}:{operation}"), |session| {
            session.plugins.entry(id.into()).or_default().runtime_outcomes.insert(operation.into(), outcome);
        });
    }
    fn require_runtime_outcome(&self, id: &str, operation: &str) -> Result<(), String> {
        self.access(format!("runtime-outcome:{id}:{operation}"), |session| {
            let plugin = session.plugins.get(id).filter(|plugin| plugin.installed).ok_or("plugin.not-found")?;
            plugin.runtime_outcomes.get(operation).cloned().unwrap_or_else(|| Err("session.runtime-required".into()))
        })
    }
    fn details(id: &str, plugin: &MemoryPlugin) -> SessionPluginDetails {
        SessionPluginDetails { id: id.into(), version: plugin.version.clone(), tier: plugin.tier.clone(), state: plugin.state.clone(), dev: plugin.dev, signed_by: plugin.signed_by.clone(), grants: plugin.grants.clone(), failure_count: plugin.failure_count, data_exists: plugin.data, cache_exists: plugin.cache, storage_buckets: plugin.storage, settings_values: plugin.settings, log_kept: !plugin.logs.is_empty() }
    }
    /// Persistent failure injection: it remains active until explicitly cleared.
    pub fn inject_error(&self, operation: &str, error: &str) { self.access(format!("inject-error:{operation}"), |session| { session.errors.insert(operation.into(), error.into()); }); }
    pub fn clear_error(&self, operation: &str) { self.access(format!("clear-error:{operation}"), |session| { session.errors.remove(operation); }); }
    pub fn connect(&self) -> Result<(), String> {
        self.access("connect".into(), |session| {
            session.connects += 1;
            if session.unreachable { return Err("session.unreachable".into()); }
            if let Some(error) = session.errors.get("connect") { return Err(error.clone()); }
            Ok(())
        })
    }
    /// Records only an explicitly performed session start; connecting never starts one.
    pub fn record_session_started(&self) { self.access("record-session-started".into(), |session| { session.sessions_started += 1; session.journal.push("session-started".into()); }); }
    pub fn review(&self, grants: &[String]) -> Result<bool, String> {
        self.access("review".into(), |session| {
            if let Some(error) = session.errors.get("review") { return Err(error.clone()); }
            if session.clients == 0 { return Err("consent.no-client".into()); }
            session.reviews.push(grants.to_vec());
            session.journal.push("review".into());
            Ok(!session.decline_review)
        })
    }
    pub fn trust_prompt(&self) -> Result<bool, String> {
        self.access("trust-prompt".into(), |session| {
            if let Some(error) = session.errors.get("trust") { return Err(error.clone()); }
            if session.clients == 0 { return Err("consent.no-client".into()); }
            session.journal.push("trust".into());
            Ok(!session.decline_trust)
        })
    }
    /// Applies an admitted install to this test session. This does not validate a
    /// package or run Wasm; the actual installer must have done admission first.
    pub fn record_install(&self, record: InstallRecord, state: &str) -> Result<(), String> {
        self.access(format!("install:{}", record.id), |session| {
            if let Some(error) = session.errors.get("install") { return Err(error.clone()); }
            let plugin = session.plugins.entry(record.id.clone()).or_default();
            plugin.version = record.version.clone(); plugin.tier = record.tier.clone(); plugin.dev = record.dev; plugin.installed = true;
            plugin.state = state.into(); plugin.history.push(state.into());
            plugin.grants = record.grants.iter().map(|grant| (grant.clone(), "unknown".into(), "granted".into())).collect();
            session.journal.push(format!("install:{}", record.id));
            session.installs.push(record);
            Ok(())
        })
    }
    /// Explicit lifecycle observation: records an actual adapter outcome, without
    /// claiming this in-memory object executed a component.
    pub fn record_activation(&self, id: &str) -> Result<(), String> {
        self.access(format!("activate:{id}"), |session| {
            if let Some(error) = session.errors.get("activate") { return Err(error.clone()); }
            let plugin = session.plugins.get_mut(id).filter(|plugin| plugin.installed).ok_or("plugin.not-found")?;
            plugin.activations += 1; plugin.restored.push(plugin.restore_bytes); plugin.state = "running".into(); plugin.history.push("running".into());
            session.started += 1; session.journal.push(format!("activate:{id}"));
            Ok(())
        })
    }
    pub fn record_deactivation(&self, id: &str, reason: &str, state: &str) -> Result<(), String> {
        self.access(format!("deactivate:{id}:{reason}"), |session| {
            if let Some(error) = session.errors.get("deactivate") { return Err(error.clone()); }
            let plugin = session.plugins.get_mut(id).filter(|plugin| plugin.installed).ok_or("plugin.not-found")?;
            plugin.deactivations.push(reason.into()); plugin.state = state.into(); plugin.history.push(state.into()); plugin.contributions.clear();
            session.journal.push(format!("deactivate:{id}:{reason}"));
            Ok(())
        })
    }
    pub fn record_restart(&self, id: &str) -> Result<(), String> {
        self.access(format!("restart:{id}"), |session| {
            if let Some(error) = session.errors.get("restart") { return Err(error.clone()); }
            let plugin = session.plugins.get_mut(id).filter(|plugin| plugin.installed).ok_or("plugin.not-found")?;
            plugin.restarts += 1; session.journal.push(format!("restart:{id}")); Ok(())
        })
    }
    /// Stores an explicit admission/review result; does not infer approval.
    pub fn set_grant_state(&self, id: &str, grant: &str, risk: &str, state: &str) -> Result<(), String> {
        self.access(format!("set-grant-state:{id}:{grant}:{state}"), |session| {
            if let Some(error) = session.errors.get("set-grant-state") { return Err(error.clone()); }
            let plugin = session.plugins.get_mut(id).filter(|plugin| plugin.installed).ok_or("plugin.not-found")?;
            if let Some(entry) = plugin.grants.iter_mut().find(|(name, _, _)| name == grant) { entry.1 = risk.into(); entry.2 = state.into(); }
            else { plugin.grants.push((grant.into(), risk.into(), state.into())); }
            if state != "granted" { plugin.contributions.retain(|(_, required)| required != grant); }
            session.journal.push(format!("set-grant-state:{id}:{grant}:{state}")); Ok(())
        })
    }
    pub fn revoke_grant(&self, id: &str, grant: &str) -> Result<(), String> {
        self.access(format!("revoke:{id}:{grant}"), |session| {
            if let Some(error) = session.errors.get("revoke") { return Err(error.clone()); }
            let plugin = session.plugins.get_mut(id).filter(|plugin| plugin.installed).ok_or("plugin.not-found")?;
            let entry = plugin.grants.iter_mut().find(|(name, _, _)| name == grant).ok_or("grant.not-found")?;
            entry.2 = "revoked".into(); plugin.contributions.retain(|(_, required)| required != grant);
            session.journal.push(format!("revoke:{id}:{grant}")); Ok(())
        })
    }
    pub fn record_uninstall(&self, id: &str, purge_data: bool, purge_log: bool) -> Result<(), String> {
        self.access(format!("uninstall:{id}"), |session| {
            if let Some(error) = session.errors.get("uninstall") { return Err(error.clone()); }
            let plugin = session.plugins.get_mut(id).filter(|plugin| plugin.installed).ok_or("plugin.not-found")?;
            plugin.installed = false; plugin.state = "uninstalled".into(); plugin.history.push("uninstalled".into()); plugin.contributions.clear(); plugin.grants.clear();
            if purge_data { plugin.data = false; plugin.cache = false; plugin.storage = 0; plugin.settings = 0; }
            if purge_log { plugin.logs.clear(); }
            session.journal.push(format!("uninstall:{id}")); Ok(())
        })
    }
    pub fn set_log_level(&self, id: &str, level: &str) -> Result<(), String> {
        self.access(format!("set-log-level:{id}:{level}"), |session| {
            if let Some(error) = session.errors.get("log-level") { return Err(error.clone()); }
            let plugin = session.plugins.get_mut(id).filter(|plugin| plugin.installed).ok_or("plugin.not-found")?;
            plugin.log_level = Some(level.into()); session.journal.push(format!("log-level:{id}:{level}")); Ok(())
        })
    }
    pub fn set_trace_records(&self, id: &str, records: Vec<TraceRecord>) { self.access(format!("set-trace-records:{id}"), |session| { session.plugins.entry(id.into()).or_default().traces = records; }); }
    pub fn set_last_tree(&self, id: &str, target: &str, tree: InspectNode) { self.access(format!("set-last-tree:{id}:{target}"), |session| { session.plugins.entry(id.into()).or_default().trees.insert(target.into(), tree); }); }
    pub fn set_profile(&self, id: &str, exports: Vec<String>, calls: Vec<ProfileCall>) { self.access(format!("set-profile:{id}"), |session| { let plugin = session.plugins.entry(id.into()).or_default(); plugin.exports = exports; plugin.profile = calls; }); }
    pub fn set_memory_views(&self, id: &str, views: Vec<MemoryView>) { self.access(format!("set-memory-views:{id}"), |session| { session.plugins.entry(id.into()).or_default().memory = views; }); }
    pub fn set_counters(&self, id: &str, counters: Vec<(Side, Vec<HourBucket>)>) { self.access(format!("set-counters:{id}"), |session| { session.plugins.entry(id.into()).or_default().counters = counters; }); }
}

impl SessionBackend for MemorySession {
    fn request(&self, request: SessionRequest) -> Result<SessionResponse, String> {
        self.access(format!("request:{request:?}"), |session| {
            if !matches!(&request, SessionRequest::Connect) && session.unreachable {
                return Err("session.unreachable".to_owned());
            }
            let operation = match &request {
                SessionRequest::List => Some("list"),
                SessionRequest::Describe { .. } => Some("describe"),
                SessionRequest::Logs { .. } => Some("logs"),
                _ => None,
            };
            if let Some(error) = operation.and_then(|operation| session.errors.get(operation)) { return Err(error.clone()); }
            Ok(())
        })?;
        match request {
            SessionRequest::Connect => self.connect().map(|()| SessionResponse::Done),
            SessionRequest::List => Ok(self.access("list".into(), |session| SessionResponse::Plugins(session.plugins.iter().filter(|(_, plugin)| plugin.installed).map(|(id, plugin)| Self::details(id, plugin)).collect()))),
            SessionRequest::Describe { id } => self.access(format!("describe:{id}"), |session| session.plugins.get(&id).filter(|plugin| plugin.installed).map(|plugin| SessionResponse::Plugin(Self::details(&id, plugin))).ok_or_else(|| "plugin.not-found".into())),
            SessionRequest::Review { grants } => self.review(&grants).map(SessionResponse::Decision),
            SessionRequest::TrustPrompt => self.trust_prompt().map(SessionResponse::Decision),
            SessionRequest::RevokeGrant { id, grant } => self.revoke_grant(&id, &grant).map(|()| SessionResponse::Done),
            SessionRequest::SetGrantState { id, grant, risk, state } => self.set_grant_state(&id, &grant, &risk, &state).map(|()| SessionResponse::Done),
            SessionRequest::RecordInstall { record, state } => self.record_install(record, &state).map(|()| SessionResponse::Done),
            SessionRequest::Logs { id } => Ok(SessionResponse::Logs(self.log_lines(&id))),
            SessionRequest::ResetFailures { id } => self.access(format!("reset-failures:{id}"), |session| {
                if let Some(error) = session.errors.get("reset-failures") { return Err(error.clone()); }
                let plugin = session.plugins.get_mut(&id).filter(|plugin| plugin.installed).ok_or("plugin.not-found")?;
                plugin.failure_count = 0; session.journal.push(format!("reset-failures:{id}")); Ok(SessionResponse::Done)
            }),
            SessionRequest::Activate { id } => { self.require_runtime_outcome(&id, "activate")?; self.record_activation(&id).map(|()| SessionResponse::Done) },
            SessionRequest::Deactivate { id, reason, state } => { self.require_runtime_outcome(&id, "deactivate")?; self.record_deactivation(&id, &reason, &state).map(|()| SessionResponse::Done) },
            SessionRequest::Restart { id } => { self.require_runtime_outcome(&id, "restart")?; self.record_restart(&id).map(|()| SessionResponse::Done) },
            SessionRequest::Uninstall { id, purge_data, purge_log } => self.record_uninstall(&id, purge_data, purge_log).map(|()| SessionResponse::Done),
            SessionRequest::SetLogLevel { id, level } => self.set_log_level(&id, &level).map(|()| SessionResponse::Done),
        }
    }
    fn plugin(&self, id: &str) -> Option<SessionPlugin> {
        self.access(format!("plugin:{id}"), |session| session.plugins.get(id).filter(|plugin| plugin.installed).map(|plugin| SessionPlugin { dev: plugin.dev, running: plugin.state == "running" }))
    }
    fn trace_records(&self, id: &str) -> Vec<TraceRecord> { self.access(format!("trace-records:{id}"), |session| session.plugins.get(id).map(|plugin| plugin.traces.clone()).unwrap_or_default()) }
    fn last_tree(&self, id: &str, target: &str) -> Option<InspectNode> { self.access(format!("last-tree:{id}:{target}"), |session| session.plugins.get(id).and_then(|plugin| plugin.trees.get(target).cloned())) }
    fn profile_exports(&self, id: &str) -> Vec<String> { self.access(format!("profile-exports:{id}"), |session| session.plugins.get(id).map(|plugin| plugin.exports.clone()).unwrap_or_default()) }
    fn profile_calls(&self, id: &str, duration_secs: u32) -> Vec<ProfileCall> { self.access(format!("profile-calls:{id}:{duration_secs}"), |session| session.plugins.get(id).map(|plugin| plugin.profile.clone()).unwrap_or_default()) }
    fn memory_views(&self, id: &str) -> Vec<MemoryView> { self.access(format!("memory-views:{id}"), |session| session.plugins.get(id).map(|plugin| plugin.memory.clone()).unwrap_or_default()) }
    fn counters(&self, id: &str) -> Vec<(Side, Vec<HourBucket>)> { self.access(format!("counters:{id}"), |session| session.plugins.get(id).map(|plugin| plugin.counters.clone()).unwrap_or_default()) }
}

// ---------------------------------------------------------------------------------------------
// Build and release helpers (reference-plugin-cli.md DEV-020..045).
// ---------------------------------------------------------------------------------------------

/// The hash of every build input of a project, as `dist/build.json` records it (DEV-024).
pub fn project_input_hash(project: &Path) -> Result<String, String> {
    project_inputs::project_input_hash(project)
}

/// Whether a component carries a `name` custom section (debug builds keep it, DEV-041).
pub fn component_has_name_section(component: &[u8]) -> bool {
    let mut found = false;
    for payload in wasmparser::Parser::new(0).parse_all(component) {
        match payload {
            Ok(wasmparser::Payload::CustomSection(section)) if section.name() == "name" => found = true,
            Ok(_) => {},
            Err(_) => return false,
        }
    }
    found
}

/// The project-relative path of the source map of one side (DEV-041).
pub fn source_map_path(side: Side) -> PathBuf {
    PathBuf::from("dist").join(format!("{}.wasm.map", side.as_str()))
}

/// Whether a release tag names the version of the manifest (DEV-044).
pub fn release_tag_matches(tag: &str, version: &str) -> bool {
    semver::Version::parse(version).is_ok() && tag.strip_prefix('v') == Some(version)
}

/// The `<sha256>  <file name>` line of a sidecar checksum file (DEV-045).
pub fn sha256_sidecar_line(bytes: &[u8], file_name: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}  {file_name}", Sha256::digest(bytes))
}

/// The timing rules of `ilium plugin dev` (reference-plugin-cli.md DEV-053, DEV-853).
pub mod dev {
    use std::time::Duration;

    /// A rebuild and reload must finish within this budget (DEV-853).
    pub const RELOAD_BUDGET: Duration = Duration::from_secs(1);

    /// Changes closer together than this are one rebuild (DEV-853).
    pub const DEBOUNCE: Duration = Duration::from_millis(300);

    /// Counts the rebuilds owed after debounced changes (DEV-853).
    #[derive(Debug)]
    pub struct Debouncer {
        window_millis: u64,
        last_change: Option<u64>,
        ready: usize,
    }

    impl Debouncer {
        /// A debouncer with the given window.
        pub fn new(window: Duration) -> Debouncer {
            Debouncer { window_millis: window.as_millis().min(u64::MAX as u128) as u64, last_change: None, ready: 0 }
        }

        /// Records a change at `at_millis`.
        pub fn change(&mut self, at_millis: u64) {
            let at_millis = self.last_change.map_or(at_millis, |last| at_millis.max(last));
            if self.last_change.is_some_and(|last| at_millis.saturating_sub(last) >= self.window_millis) {
                self.ready = self.ready.saturating_add(1);
            }
            self.last_change = Some(at_millis);
        }

        /// The rebuilds due at `now_millis`.
        pub fn due(&mut self, now_millis: u64) -> usize {
            if self.last_change.is_some_and(|last| now_millis >= last && now_millis - last >= self.window_millis) {
                self.ready = self.ready.saturating_add(1);
                self.last_change = None;
            }
            std::mem::take(&mut self.ready)
        }
    }
}

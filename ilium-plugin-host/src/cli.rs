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
pub mod check_command;
pub mod install_command;
mod build_command;
mod dev_command;
mod doctor_command;
mod test_command;
mod lifecycle_command;
pub mod local_install;
pub mod keys_command;
pub mod new_command;
pub mod pack_command;
pub mod session_install;
mod offline;
pub mod project_inputs;
pub mod registry_command;
pub mod session_commands;
pub mod verify_install;

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
            data_dir,
            state_dir,
            config_dir,
            cwd,
            now,
            is_ci,
            fetcher,
            platform: Platform::Linux,
            host_release: format!("v{}", env!("CARGO_PKG_VERSION")),
            env: BTreeMap::new(),
            system_dir: None,
            policy_file: None,
            probe: None,
            builder: None,
            runner: None,
            session: None,
            installer: None,
            audit_context: None,
            interrupt: None,
            dev_script: Vec::new(),
            build_duration_ms: 0,
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
    pub fn with_installer(
        mut self,
        installer: Arc<dyn install_command::InstallCommandBackend>,
    ) -> CliEnv {
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
    if args.iter().any(|arg| arg == "--quiet") {
        output.stdout_jsonl.retain(|record| record["type"] != "progress");
    }
    if let Some(summary) = output
        .stdout_jsonl
        .iter_mut()
        .find(|record| record["type"] == "summary")
    {
        summary["duration_ms"] = serde_json::json!(started.elapsed().as_millis() as u64);
    }
    let custom_text = (output.stdout_text != output.stdout_raw).then_some(output.stdout_text);
    let mut rendered = offline::render(output.stdout_jsonl, output.exit_code);
    if let Some(text) = custom_text {
        rendered.stdout_text = text;
    }
    rendered.stderr = output.stderr;
    rendered
}

/// The global flags every command accepts (reference-plugin-cli.md DEV-003).
#[derive(Default)]
struct GlobalFlags {
    session: Option<String>,
    cwd: Option<String>,
    help: bool,
}

const USAGE: &str = "Usage: ilium plugin <command> [options]\n\nGlobal flags:\n  --session <name>  the Ilium session to talk to (default: the session of the project)\n  --cwd <path>      the project directory that names the session\n  --quiet           print no progress records\n  --no-color        print no colour\n  --json            print JSON lines instead of text\n  --help            print this usage\n";

/// Removes the global flags that only the dispatcher needs from `args`.
fn split_global(args: &[String]) -> Result<(Vec<String>, GlobalFlags), CliOutput> {
    let mut flags = GlobalFlags::default();
    let mut rest = Vec::with_capacity(args.len());
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--quiet" | "--no-color" => {}
            "--help" => flags.help = true,
            "--session" | "--cwd" => {
                let Some(value) = iter.next() else {
                    return Err(offline::failure(
                        "usage.missing-argument",
                        "The flag needs a value",
                        2,
                    ));
                };
                if arg == "--session" {
                    flags.session = Some(value.clone());
                } else {
                    flags.cwd = Some(value.clone());
                }
            }
            _ => rest.push(arg.clone()),
        }
    }
    Ok((rest, flags))
}

fn session_name_error(flags: &GlobalFlags, env: &CliEnv) -> Option<CliOutput> {
    if flags.session.as_deref().is_some_and(|name| name != "default") {
        return Some(offline::failure(
            "session.not-found",
            "The named session does not exist",
            6,
        ));
    }
    let elsewhere = flags.cwd.as_ref().is_some_and(|cwd| {
        let wanted = env.cwd.join(cwd);
        std::fs::canonicalize(&wanted).ok() != std::fs::canonicalize(&env.cwd).ok()
    });
    elsewhere.then(|| {
        offline::failure(
            "session.unreachable",
            "No session runs in that project directory",
            6,
        )
    })
}

fn run_inner(raw_args: &[String], env: &CliEnv) -> CliOutput {
    let (args, flags) = match split_global(raw_args) {
        Ok(split) => split,
        Err(output) => return output,
    };
    let args = args.as_slice();
    if flags.help {
        let mut output = offline::finish(vec![], 0);
        output.stdout_text = USAGE.to_owned();
        return output;
    }
    let Some(command) = args.first().map(String::as_str) else {
        return offline::failure("usage.command-missing", "A plugin command is required", 2);
    };
    if env
        .interrupt
        .as_ref()
        .is_some_and(|flag| flag.load(std::sync::atomic::Ordering::Acquire))
    {
        return offline::failure("command.interrupted", "Command interrupted", 130);
    }
    let uses_session = matches!(
        command,
        "list" | "logs" | "grants" | "audit" | "uninstall" | "enable" | "disable" | "log-level"
            | "dev" | "trace" | "profile"
    ) || (command == "install" && env.installer.is_none());
    if uses_session {
        if let Some(output) = session_name_error(&flags, env) {
            return output;
        }
        if env.session.is_none() {
            return offline::failure(
                "session.unreachable",
                "No reachable session was supplied",
                6,
            );
        }
    }
    if matches!(command, "list" | "logs" | "grants" | "audit") {
        return session_commands::run(command, &args[1..], env);
    }
    if matches!(command, "--version" | "version") {
        return doctor_command::version(&args[1..]);
    }
    if command == "doctor" {
        return doctor_command::doctor(&args[1..], env);
    }
    if matches!(command, "uninstall" | "enable" | "disable" | "log-level") {
        return lifecycle_command::run(command, &args[1..], env);
    }
    if command == "install" {
        return match &env.installer {
            Some(installer) => install_command::run(&args[1..], &env.cwd, installer.as_ref()),
            None => session_install::run(&args[1..], env),
        };
    }
    match command {
        "check" | "lint" => check_command::run(command, &args[1..], env),
        "build" => build_command::run(&args[1..], env),
        "test" => test_command::run(&args[1..], env),
        "dev" => dev_command::run(&args[1..], env),
        "inspect" | "verify" => offline::run(command, &args[1..], env),
        "new" => new_command::run(&args[1..], env),
        "keygen" | "sign" | "trust" | "publish" => keys_command::run(command, &args[1..], env),
        "pack" => pack_command::run(&args[1..], env),
        "registry" => registry_command::run(&args[1..], env),
        "install" | "uninstall" | "enable" | "disable" | "grants" | "list" | "dev" | "logs"
        | "log-level" | "trace" | "profile"
            if env.session.is_none() =>
        {
            offline::failure(
                "session.unreachable",
                "No reachable session was supplied",
                6,
            )
        }
        "install" | "uninstall" | "enable" | "disable" | "grants" | "list" | "dev" | "logs"
        | "log-level" | "trace" | "profile" | "new" | "build" | "test" | "pack" | "keygen"
        | "sign" | "trust" | "publish" | "doctor" => offline::failure(
            "command.context-required",
            "The command adapter is not connected",
            1,
        ),
        _ => offline::failure(
            "usage.command-unknown",
            format!("Unknown plugin command: {command}"),
            2,
        ),
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
        stdout: if args.iter().any(|arg| arg == "--json") {
            output.stdout_raw
        } else {
            output.stdout_text
        },
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
    Describe {
        id: String,
    },
    Review {
        grants: Vec<String>,
    },
    TrustPrompt,
    RevokeGrant {
        id: String,
        grant: String,
    },
    SetGrantState {
        id: String,
        grant: String,
        risk: String,
        state: String,
    },
    /// Observation after the real installer has committed admission.
    RecordInstall {
        record: InstallRecord,
        state: String,
    },
    Logs {
        id: String,
    },
    ResetFailures {
        id: String,
    },
    Activate {
        id: String,
    },
    Deactivate {
        id: String,
        reason: String,
        state: String,
    },
    Restart {
        id: String,
    },
    Uninstall {
        id: String,
        purge_data: bool,
        purge_log: bool,
    },
    SetLogLevel {
        id: String,
        level: String,
    },
    /// Returns the plugin's effective log threshold (DEV-195).
    GetLogLevel {
        id: String,
    },
    /// Returns the plugin's threshold to the session default (DEV-195).
    ResetLogLevel {
        id: String,
    },
    /// Persistently enables a plugin: resets its failure count and activates it (DEV-160).
    Enable {
        id: String,
    },
    /// Persistently disables a plugin: deactivates it with reason `disabled` (DEV-160).
    Disable {
        id: String,
    },
    /// Sets the installed real copy of `id` aside while a dev copy runs; the response carries
    /// the state it had (DEV-183).
    ReplaceInstalled {
        id: String,
    },
    /// Puts the copy set aside by `ReplaceInstalled` back, in `state` (DEV-189).
    RestoreReplaced {
        id: String,
        state: String,
    },
    /// Reloads one running instance after a rebuild: deactivate with reason `upgrade`, then
    /// activate with the state it saved, dropped when over [`dev::RESTORE_LIMIT_BYTES`] (DEV-187, DEV-188).
    Reload {
        id: String,
        side: String,
    },
    /// How many clients are attached to the session (DEV-187).
    AttachedClients,
    /// A trust decision removed the key that signed this plugin; it stays installed but
    /// untrusted (DEV-127).
    MarkUntrusted {
        id: String,
        reason: String,
    },
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
    Value(String),
    /// What a reload handed the new instance.
    Reloaded {
        restored_bytes: u64,
        dropped: bool,
    },
}

/// Deterministic in-memory CLI test adapter, with explicit runtime observations.
#[derive(Default)]
pub struct MemorySession {
    state: std::sync::Mutex<MemorySessionState>,
}

impl std::fmt::Debug for MemorySession {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MemorySession")
            .finish_non_exhaustive()
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
    /// The real copy a dev copy of the same id replaced (DEV-183).
    replaced: Option<Box<MemoryPlugin>>,
    /// Set on a restored real copy: its whole history belongs to the non-dev copy (DEV-183).
    was_replaced: bool,
}

impl MemorySession {
    // No callback is invoked while this lock is held. All in-memory effects and
    // call records share the same serialization point.
    fn access<T>(&self, call: String, action: impl FnOnce(&mut MemorySessionState) -> T) -> T {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.calls.push(call);
        action(&mut state)
    }

    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn unreachable() -> Arc<Self> {
        Arc::new(Self {
            state: std::sync::Mutex::new(MemorySessionState {
                unreachable: true,
                ..Default::default()
            }),
        })
    }

    pub fn add_plugin(
        &self,
        id: &str,
        version: &str,
        tier: &str,
        state: &str,
        dev: bool,
        grants: &[(&str, &str, &str)],
    ) {
        self.access(format!("add-plugin:{id}"), |session| {
            session.plugins.insert(
                id.into(),
                MemoryPlugin {
                    version: version.into(),
                    tier: tier.into(),
                    state: state.into(),
                    dev,
                    installed: true,
                    grants: grants
                        .iter()
                        .map(|(grant, risk, state)| {
                            ((*grant).into(), (*risk).into(), (*state).into())
                        })
                        .collect(),
                    history: vec![state.into()],
                    ..Default::default()
                },
            );
        });
    }
    pub fn add_plugin_with_grants(
        &self,
        id: &str,
        version: &str,
        tier: &str,
        state: &str,
        dev: bool,
        grants: &[(&str, &str, &str)],
    ) {
        self.add_plugin(id, version, tier, state, dev, grants);
    }
    pub fn set_signed_by(&self, id: &str, key_id: &str) {
        self.access(format!("set-signed-by:{id}"), |session| {
            if let Some(plugin) = session.plugins.get_mut(id) {
                plugin.signed_by = Some(key_id.into());
            }
        });
    }
    pub fn signed_by(&self, id: &str) -> Option<String> {
        self.access(format!("signed-by:{id}"), |session| {
            session
                .plugins
                .get(id)
                .and_then(|plugin| plugin.signed_by.clone())
        })
    }
    pub fn plugin_state(&self, id: &str) -> Option<String> {
        self.access(format!("plugin-state:{id}"), |session| {
            session
                .plugins
                .get(id)
                .filter(|plugin| plugin.installed)
                .map(|plugin| plugin.state.clone())
        })
    }
    pub fn set_client_attached(&self, attached: bool) {
        self.attach_clients(usize::from(attached));
    }
    pub fn review_dialog_count(&self) -> usize {
        self.access("review-dialog-count".into(), |session| {
            session.reviews.len()
        })
    }
    pub fn started_plugin_count(&self) -> usize {
        self.access("started-plugin-count".into(), |session| session.started)
    }
    pub fn granted(&self, id: &str) -> Vec<String> {
        self.access(format!("granted:{id}"), |session| {
            session
                .plugins
                .get(id)
                .map(|plugin| {
                    plugin
                        .grants
                        .iter()
                        .filter(|(_, _, state)| state == "granted")
                        .map(|(grant, _, _)| grant.clone())
                        .collect()
                })
                .unwrap_or_default()
        })
    }
    pub fn is_high_risk(&self, grant: &str) -> bool {
        self.access(format!("is-high-risk:{grant}"), |session| {
            session.plugins.values().any(|plugin| {
                plugin
                    .grants
                    .iter()
                    .any(|(name, risk, _)| name == grant && risk == "high")
            })
        })
    }
    pub fn connect_attempts(&self) -> usize {
        self.access("connect-attempts".into(), |session| session.connects)
    }
    pub fn sessions_started(&self) -> usize {
        self.access("sessions-started".into(), |session| {
            session.sessions_started
        })
    }
    pub fn journal(&self) -> Vec<String> {
        self.access("journal".into(), |session| session.journal.clone())
    }
    /// All calls, including read-only probes, in their serialized order.
    pub fn calls(&self) -> Vec<String> {
        self.access("calls".into(), |session| session.calls.clone())
    }
    pub fn add_log_line(&self, id: &str, side: &str, level: &str, message: &str) {
        self.access(format!("add-log-line:{id}"), |session| {
            session.plugins.entry(id.into()).or_default().logs.push((
                side.into(),
                level.into(),
                message.into(),
            ));
        });
    }
    pub fn log_lines(&self, id: &str) -> Vec<(String, String, String)> {
        self.access(format!("log-lines:{id}"), |session| {
            session
                .plugins
                .get(id)
                .map(|plugin| plugin.logs.clone())
                .unwrap_or_default()
        })
    }
    pub fn decline_review(&self, decline: bool) {
        self.access("decline-review".into(), |session| {
            session.decline_review = decline
        });
    }
    pub fn decline_trust(&self, decline: bool) {
        self.access("decline-trust".into(), |session| {
            session.decline_trust = decline
        });
    }
    pub fn plugin_version(&self, id: &str) -> Option<String> {
        self.access(format!("plugin-version:{id}"), |session| {
            session
                .plugins
                .get(id)
                .filter(|plugin| plugin.installed)
                .map(|plugin| plugin.version.clone())
        })
    }
    pub fn activations(&self, id: &str) -> usize {
        self.access(format!("activations:{id}"), |session| {
            session
                .plugins
                .get(id)
                .map_or(0, |plugin| plugin.activations)
        })
    }
    pub fn deactivations(&self, id: &str) -> Vec<String> {
        self.access(format!("deactivations:{id}"), |session| {
            session
                .plugins
                .get(id)
                .map(|plugin| plugin.deactivations.clone())
                .unwrap_or_default()
        })
    }
    pub fn restarts(&self, id: &str) -> usize {
        self.access(format!("restarts:{id}"), |session| {
            session.plugins.get(id).map_or(0, |plugin| plugin.restarts)
        })
    }
    pub fn package_installed(&self, id: &str) -> bool {
        self.access(format!("package-installed:{id}"), |session| {
            session
                .plugins
                .get(id)
                .is_some_and(|plugin| plugin.installed)
        })
    }
    pub fn add_contribution(&self, id: &str, kind: &str, grant: &str) {
        self.access(format!("add-contribution:{id}"), |session| {
            if let Some(plugin) = session.plugins.get_mut(id) {
                plugin.contributions.push((kind.into(), grant.into()));
            }
        });
    }
    pub fn contribution_count(&self, id: &str) -> usize {
        self.access(format!("contribution-count:{id}"), |session| {
            session
                .plugins
                .get(id)
                .map_or(0, |plugin| plugin.contributions.len())
        })
    }
    pub fn seed_plugin_data(&self, id: &str) {
        self.access(format!("seed-plugin-data:{id}"), |session| {
            if let Some(plugin) = session.plugins.get_mut(id) {
                plugin.data = true;
                plugin.cache = true;
                plugin.storage = 1;
                plugin.settings = 1;
            }
        });
    }
    pub fn data_folder_exists(&self, id: &str) -> bool {
        self.access(format!("data-folder-exists:{id}"), |session| {
            session.plugins.get(id).is_some_and(|plugin| plugin.data)
        })
    }
    pub fn cache_folder_exists(&self, id: &str) -> bool {
        self.access(format!("cache-folder-exists:{id}"), |session| {
            session.plugins.get(id).is_some_and(|plugin| plugin.cache)
        })
    }
    pub fn storage_bucket_count(&self, id: &str) -> usize {
        self.access(format!("storage-bucket-count:{id}"), |session| {
            session.plugins.get(id).map_or(0, |plugin| plugin.storage)
        })
    }
    pub fn settings_value_count(&self, id: &str) -> usize {
        self.access(format!("settings-value-count:{id}"), |session| {
            session.plugins.get(id).map_or(0, |plugin| plugin.settings)
        })
    }
    pub fn log_kept(&self, id: &str) -> bool {
        self.access(format!("log-kept:{id}"), |session| {
            session
                .plugins
                .get(id)
                .is_some_and(|plugin| !plugin.logs.is_empty())
        })
    }
    pub fn keep_log(&self, id: &str, lines: &[(&str, &str, &str)]) {
        self.access(format!("keep-log:{id}"), |session| {
            session.plugins.entry(id.into()).or_default().logs = lines
                .iter()
                .map(|(side, level, message)| ((*side).into(), (*level).into(), (*message).into()))
                .collect();
        });
    }
    pub fn set_failure_count(&self, id: &str, count: u32) {
        self.access(format!("set-failure-count:{id}"), |session| {
            if let Some(plugin) = session.plugins.get_mut(id) {
                plugin.failure_count = count;
            }
        });
    }
    pub fn failure_count(&self, id: &str) -> u32 {
        self.access(format!("failure-count:{id}"), |session| {
            session
                .plugins
                .get(id)
                .map_or(0, |plugin| plugin.failure_count)
        })
    }
    pub fn grant_state(&self, id: &str, grant: &str) -> Option<String> {
        self.access(format!("grant-state:{id}:{grant}"), |session| {
            session
                .plugins
                .get(id)
                .and_then(|plugin| plugin.grants.iter().find(|(name, _, _)| name == grant))
                .map(|(_, _, state)| state.clone())
        })
    }
    pub fn install_history(&self) -> Vec<InstallRecord> {
        self.access("install-history".into(), |session| session.installs.clone())
    }
    /// The states a copy went through. `flag` is `with_initial` for an ordinary plugin; for a real
    /// copy that a dev copy replaced and that was then restored, `false` selects that real copy
    /// (its whole history) and `true` the dev copy.
    pub fn state_history(&self, id: &str, flag: bool) -> Vec<String> {
        self.access(format!("state-history:{id}:{flag}"), |session| {
            session
                .plugins
                .get(id)
                .map(|plugin| {
                    let skip = usize::from(!flag && !plugin.was_replaced);
                    plugin.history.iter().skip(skip).cloned().collect()
                })
                .unwrap_or_default()
        })
    }
    pub fn review_dialog_grants(&self) -> Vec<Vec<String>> {
        self.access("review-dialog-grants".into(), |session| {
            session.reviews.clone()
        })
    }
    pub fn attach_clients(&self, count: usize) {
        self.access(format!("attach-clients:{count}"), |session| {
            session.clients = count
        });
    }
    pub fn set_restore_bytes(&self, id: &str, bytes: u64) {
        self.access(format!("set-restore-bytes:{id}"), |session| {
            session.plugins.entry(id.to_owned()).or_default().restore_bytes = bytes;
        });
    }
    pub fn activation_restore_bytes(&self, id: &str) -> Vec<u64> {
        self.access(format!("activation-restore-bytes:{id}"), |session| {
            session
                .plugins
                .get(id)
                .map(|plugin| plugin.restored.clone())
                .unwrap_or_default()
        })
    }
    pub fn log_level(&self, id: &str) -> String {
        self.access(format!("log-level:{id}"), |session| {
            session
                .plugins
                .get(id)
                .and_then(|plugin| plugin.log_level.clone())
                .unwrap_or_else(|| {
                    if session.default_log_level.is_empty() {
                        "info".into()
                    } else {
                        session.default_log_level.clone()
                    }
                })
        })
    }
    pub fn set_default_log_level(&self, level: &str) {
        self.access(format!("set-default-log-level:{level}"), |session| {
            session.default_log_level = level.into()
        });
    }

    /// Explicit fixture outcome for a lifecycle operation. Without one, a
    /// runtime command is refused rather than claiming a component ran.
    pub fn set_runtime_outcome(&self, id: &str, operation: &str, outcome: Result<(), String>) {
        self.access(format!("set-runtime-outcome:{id}:{operation}"), |session| {
            session
                .plugins
                .entry(id.into())
                .or_default()
                .runtime_outcomes
                .insert(operation.into(), outcome);
        });
    }
    fn require_runtime_outcome(&self, id: &str, operation: &str) -> Result<(), String> {
        self.access(format!("runtime-outcome:{id}:{operation}"), |session| {
            let plugin = session
                .plugins
                .get(id)
                .filter(|plugin| plugin.installed)
                .ok_or("plugin.not-found")?;
            plugin
                .runtime_outcomes
                .get(operation)
                .cloned()
                .unwrap_or_else(|| Err("session.runtime-required".into()))
        })
    }
    fn details(id: &str, plugin: &MemoryPlugin) -> SessionPluginDetails {
        SessionPluginDetails {
            id: id.into(),
            version: plugin.version.clone(),
            tier: plugin.tier.clone(),
            state: plugin.state.clone(),
            dev: plugin.dev,
            signed_by: plugin.signed_by.clone(),
            grants: plugin.grants.clone(),
            failure_count: plugin.failure_count,
            data_exists: plugin.data,
            cache_exists: plugin.cache,
            storage_buckets: plugin.storage,
            settings_values: plugin.settings,
            log_kept: !plugin.logs.is_empty(),
        }
    }
    /// Persistent failure injection: it remains active until explicitly cleared.
    pub fn inject_error(&self, operation: &str, error: &str) {
        self.access(format!("inject-error:{operation}"), |session| {
            session.errors.insert(operation.into(), error.into());
        });
    }
    pub fn clear_error(&self, operation: &str) {
        self.access(format!("clear-error:{operation}"), |session| {
            session.errors.remove(operation);
        });
    }
    pub fn connect(&self) -> Result<(), String> {
        self.access("connect".into(), |session| {
            session.connects += 1;
            if session.unreachable {
                return Err("session.unreachable".into());
            }
            if let Some(error) = session.errors.get("connect") {
                return Err(error.clone());
            }
            Ok(())
        })
    }
    /// Records only an explicitly performed session start; connecting never starts one.
    pub fn record_session_started(&self) {
        self.access("record-session-started".into(), |session| {
            session.sessions_started += 1;
            session.journal.push("session-started".into());
        });
    }
    pub fn review(&self, grants: &[String]) -> Result<bool, String> {
        self.access("review".into(), |session| {
            if let Some(error) = session.errors.get("review") {
                return Err(error.clone());
            }
            if session.clients == 0 {
                return Err("consent.no-client".into());
            }
            session.reviews.push(grants.to_vec());
            session.journal.push("review".into());
            Ok(!session.decline_review)
        })
    }
    pub fn trust_prompt(&self) -> Result<bool, String> {
        self.access("trust-prompt".into(), |session| {
            if let Some(error) = session.errors.get("trust") {
                return Err(error.clone());
            }
            if session.clients == 0 {
                return Err("consent.no-client".into());
            }
            session.journal.push("trust".into());
            Ok(!session.decline_trust)
        })
    }
    /// Applies an admitted install to this test session. This does not validate a
    /// package or run Wasm; the actual installer must have done admission first.
    pub fn record_install(&self, record: InstallRecord, state: &str) -> Result<(), String> {
        self.access(format!("install:{}", record.id), |session| {
            if let Some(error) = session.errors.get("install") {
                return Err(error.clone());
            }
            let plugin = session.plugins.entry(record.id.clone()).or_default();
            plugin.version = record.version.clone();
            plugin.tier = record.tier.clone();
            plugin.dev = record.dev;
            plugin.installed = true;
            if record.dev {
                // A running dev copy owns a data folder from its first start.
                plugin.data = true;
            }
            plugin.state = state.into();
            plugin.history.push(state.into());
            plugin.grants = record
                .grants
                .iter()
                .map(|grant| (grant.clone(), "unknown".into(), "granted".into()))
                .collect();
            session.journal.push(format!("install:{}", record.id));
            session.installs.push(record);
            Ok(())
        })
    }
    /// Explicit lifecycle observation: records an actual adapter outcome, without
    /// claiming this in-memory object executed a component.
    pub fn record_activation(&self, id: &str) -> Result<(), String> {
        self.access(format!("activate:{id}"), |session| {
            if let Some(error) = session.errors.get("activate") {
                return Err(error.clone());
            }
            let plugin = session
                .plugins
                .get_mut(id)
                .filter(|plugin| plugin.installed)
                .ok_or("plugin.not-found")?;
            plugin.activations += 1;
            plugin.restored.push(plugin.restore_bytes);
            plugin.state = "running".into();
            plugin.history.push("running".into());
            session.started += 1;
            session.journal.push(format!("activate:{id}"));
            Ok(())
        })
    }
    pub fn record_deactivation(&self, id: &str, reason: &str, state: &str) -> Result<(), String> {
        self.access(format!("deactivate:{id}:{reason}"), |session| {
            if let Some(error) = session.errors.get("deactivate") {
                return Err(error.clone());
            }
            let plugin = session
                .plugins
                .get_mut(id)
                .filter(|plugin| plugin.installed)
                .ok_or("plugin.not-found")?;
            plugin.deactivations.push(reason.into());
            plugin.state = state.into();
            plugin.history.push(state.into());
            plugin.contributions.clear();
            session.journal.push(format!("deactivate:{id}:{reason}"));
            Ok(())
        })
    }
    pub fn record_restart(&self, id: &str) -> Result<(), String> {
        self.access(format!("restart:{id}"), |session| {
            if let Some(error) = session.errors.get("restart") {
                return Err(error.clone());
            }
            let plugin = session
                .plugins
                .get_mut(id)
                .filter(|plugin| plugin.installed)
                .ok_or("plugin.not-found")?;
            plugin.restarts += 1;
            session.journal.push(format!("restart:{id}"));
            Ok(())
        })
    }
    /// Stores an explicit admission/review result; does not infer approval.
    pub fn set_grant_state(
        &self,
        id: &str,
        grant: &str,
        risk: &str,
        state: &str,
    ) -> Result<(), String> {
        self.access(format!("set-grant-state:{id}:{grant}:{state}"), |session| {
            if let Some(error) = session.errors.get("set-grant-state") {
                return Err(error.clone());
            }
            let plugin = session
                .plugins
                .get_mut(id)
                .filter(|plugin| plugin.installed)
                .ok_or("plugin.not-found")?;
            if let Some(entry) = plugin.grants.iter_mut().find(|(name, _, _)| name == grant) {
                entry.1 = risk.into();
                entry.2 = state.into();
            } else {
                plugin
                    .grants
                    .push((grant.into(), risk.into(), state.into()));
            }
            if state != "granted" {
                plugin
                    .contributions
                    .retain(|(_, required)| required != grant);
            }
            session
                .journal
                .push(format!("set-grant-state:{id}:{grant}:{state}"));
            Ok(())
        })
    }
    pub fn revoke_grant(&self, id: &str, grant: &str) -> Result<(), String> {
        self.access(format!("revoke:{id}:{grant}"), |session| {
            if let Some(error) = session.errors.get("revoke") {
                return Err(error.clone());
            }
            let plugin = session
                .plugins
                .get_mut(id)
                .filter(|plugin| plugin.installed)
                .ok_or("plugin.not-found")?;
            let entry = plugin
                .grants
                .iter_mut()
                .find(|(name, _, _)| name == grant)
                .ok_or("grant.not-found")?;
            entry.2 = "revoked".into();
            plugin
                .contributions
                .retain(|(_, required)| required != grant);
            session.journal.push(format!("revoke:{id}:{grant}"));
            Ok(())
        })
    }
    pub fn record_uninstall(
        &self,
        id: &str,
        purge_data: bool,
        purge_log: bool,
    ) -> Result<(), String> {
        self.access(format!("uninstall:{id}"), |session| {
            if let Some(error) = session.errors.get("uninstall") {
                return Err(error.clone());
            }
            let plugin = session
                .plugins
                .get_mut(id)
                .filter(|plugin| plugin.installed)
                .ok_or("plugin.not-found")?;
            plugin.installed = false;
            plugin.state = "uninstalled".into();
            plugin.history.push("uninstalled".into());
            plugin.contributions.clear();
            plugin.grants.clear();
            if purge_data {
                plugin.data = false;
                plugin.cache = false;
                plugin.storage = 0;
                plugin.settings = 0;
            }
            if purge_log {
                plugin.logs.clear();
            }
            session.journal.push(format!("uninstall:{id}"));
            Ok(())
        })
    }
    pub fn set_log_level(&self, id: &str, level: &str) -> Result<(), String> {
        self.access(format!("set-log-level:{id}:{level}"), |session| {
            if let Some(error) = session.errors.get("log-level") {
                return Err(error.clone());
            }
            let plugin = session
                .plugins
                .get_mut(id)
                .filter(|plugin| plugin.installed)
                .ok_or("plugin.not-found")?;
            plugin.log_level = Some(level.into());
            session.journal.push(format!("log-level:{id}:{level}"));
            Ok(())
        })
    }
    pub fn set_trace_records(&self, id: &str, records: Vec<TraceRecord>) {
        self.access(format!("set-trace-records:{id}"), |session| {
            session.plugins.entry(id.into()).or_default().traces = records;
        });
    }
    pub fn set_last_tree(&self, id: &str, target: &str, tree: InspectNode) {
        self.access(format!("set-last-tree:{id}:{target}"), |session| {
            session
                .plugins
                .entry(id.into())
                .or_default()
                .trees
                .insert(target.into(), tree);
        });
    }
    pub fn set_profile(&self, id: &str, exports: Vec<String>, calls: Vec<ProfileCall>) {
        self.access(format!("set-profile:{id}"), |session| {
            let plugin = session.plugins.entry(id.into()).or_default();
            plugin.exports = exports;
            plugin.profile = calls;
        });
    }
    pub fn set_memory_views(&self, id: &str, views: Vec<MemoryView>) {
        self.access(format!("set-memory-views:{id}"), |session| {
            session.plugins.entry(id.into()).or_default().memory = views;
        });
    }
    pub fn set_counters(&self, id: &str, counters: Vec<(Side, Vec<HourBucket>)>) {
        self.access(format!("set-counters:{id}"), |session| {
            session.plugins.entry(id.into()).or_default().counters = counters;
        });
    }
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
            if let Some(error) = operation.and_then(|operation| session.errors.get(operation)) {
                return Err(error.clone());
            }
            Ok(())
        })?;
        match request {
            SessionRequest::Connect => self.connect().map(|()| SessionResponse::Done),
            SessionRequest::List => Ok(self.access("list".into(), |session| {
                SessionResponse::Plugins(
                    session
                        .plugins
                        .iter()
                        .filter(|(_, plugin)| plugin.installed)
                        .map(|(id, plugin)| Self::details(id, plugin))
                        .collect(),
                )
            })),
            SessionRequest::Describe { id } => self.access(format!("describe:{id}"), |session| {
                session
                    .plugins
                    .get(&id)
                    .filter(|plugin| plugin.installed)
                    .map(|plugin| SessionResponse::Plugin(Self::details(&id, plugin)))
                    .ok_or_else(|| "plugin.not-found".into())
            }),
            SessionRequest::Review { grants } => {
                self.review(&grants).map(SessionResponse::Decision)
            }
            SessionRequest::TrustPrompt => self.trust_prompt().map(SessionResponse::Decision),
            SessionRequest::RevokeGrant { id, grant } => self
                .revoke_grant(&id, &grant)
                .map(|()| SessionResponse::Done),
            SessionRequest::SetGrantState {
                id,
                grant,
                risk,
                state,
            } => self
                .set_grant_state(&id, &grant, &risk, &state)
                .map(|()| SessionResponse::Done),
            SessionRequest::RecordInstall { record, state } => self
                .record_install(record, &state)
                .map(|()| SessionResponse::Done),
            SessionRequest::Logs { id } => Ok(SessionResponse::Logs(self.log_lines(&id))),
            SessionRequest::ResetFailures { id } => {
                self.access(format!("reset-failures:{id}"), |session| {
                    if let Some(error) = session.errors.get("reset-failures") {
                        return Err(error.clone());
                    }
                    let plugin = session
                        .plugins
                        .get_mut(&id)
                        .filter(|plugin| plugin.installed)
                        .ok_or("plugin.not-found")?;
                    plugin.failure_count = 0;
                    session.journal.push(format!("reset-failures:{id}"));
                    Ok(SessionResponse::Done)
                })
            }
            SessionRequest::Activate { id } => {
                self.require_runtime_outcome(&id, "activate")?;
                self.record_activation(&id).map(|()| SessionResponse::Done)
            }
            SessionRequest::Deactivate { id, reason, state } => {
                self.require_runtime_outcome(&id, "deactivate")?;
                self.record_deactivation(&id, &reason, &state)
                    .map(|()| SessionResponse::Done)
            }
            SessionRequest::MarkUntrusted { id, reason } => self
                .record_deactivation(&id, &reason, "untrusted")
                .map(|()| SessionResponse::Done),
            SessionRequest::Restart { id } => {
                self.require_runtime_outcome(&id, "restart")?;
                self.record_restart(&id).map(|()| SessionResponse::Done)
            }
            SessionRequest::Uninstall {
                id,
                purge_data,
                purge_log,
            } => self
                .record_uninstall(&id, purge_data, purge_log)
                .map(|()| SessionResponse::Done),
            SessionRequest::SetLogLevel { id, level } => self
                .set_log_level(&id, &level)
                .map(|()| SessionResponse::Done),
            SessionRequest::GetLogLevel { id } => {
                if self.plugin(&id).is_none() {
                    return Err("plugin.not-found".into());
                }
                Ok(SessionResponse::Value(self.log_level(&id)))
            }
            SessionRequest::ResetLogLevel { id } => {
                self.access(format!("reset-log-level:{id}"), |session| {
                    let plugin = session
                        .plugins
                        .get_mut(&id)
                        .filter(|plugin| plugin.installed)
                        .ok_or("plugin.not-found")?;
                    plugin.log_level = None;
                    session.journal.push(format!("log-level-reset:{id}"));
                    Ok(SessionResponse::Done)
                })
            }
            SessionRequest::Enable { id } => {
                self.access(format!("enable-reset:{id}"), |session| {
                    session
                        .plugins
                        .get_mut(&id)
                        .filter(|plugin| plugin.installed)
                        .ok_or_else(|| "plugin.not-found".to_owned())?
                        .failure_count = 0;
                    Ok::<(), String>(())
                })?;
                self.record_activation(&id).map(|()| SessionResponse::Done)
            }
            SessionRequest::Disable { id } => self
                .record_deactivation(&id, "disabled", "disabled")
                .map(|()| SessionResponse::Done),
            SessionRequest::ReplaceInstalled { id } => {
                self.access(format!("replace-installed:{id}"), |session| {
                    let slot = session
                        .plugins
                        .get_mut(&id)
                        .filter(|plugin| plugin.installed)
                        .ok_or_else(|| "plugin.not-found".to_owned())?;
                    let mut prior = std::mem::take(slot);
                    let state = prior.state.clone();
                    if state != "disabled" {
                        prior.deactivations.push("disabled".into());
                        prior.state = "disabled".into();
                        prior.history.push("disabled".into());
                    }
                    slot.replaced = Some(Box::new(prior));
                    session.journal.push(format!("replace-installed:{id}"));
                    Ok(SessionResponse::Value(state))
                })
            }
            SessionRequest::RestoreReplaced { id, state } => {
                self.access(format!("restore-replaced:{id}"), |session| {
                    let slot = session
                        .plugins
                        .get_mut(&id)
                        .ok_or_else(|| "plugin.not-found".to_owned())?;
                    let mut prior = *slot
                        .replaced
                        .take()
                        .ok_or_else(|| "plugin.not-found".to_owned())?;
                    if prior.state != state {
                        if state == "running" {
                            prior.activations += 1;
                            prior.restored.push(prior.restore_bytes);
                        }
                        prior.state = state.clone();
                        prior.history.push(state);
                    }
                    prior.was_replaced = true;
                    *slot = prior;
                    session.journal.push(format!("restore-replaced:{id}"));
                    Ok(SessionResponse::Done)
                })
            }
            SessionRequest::Reload { id, side } => {
                self.access(format!("reload:{id}:{side}"), |session| {
                    if let Some(error) = session.errors.get("reload") {
                        return Err(error.clone());
                    }
                    let plugin = session
                        .plugins
                        .get_mut(&id)
                        .filter(|plugin| plugin.installed)
                        .ok_or_else(|| "plugin.not-found".to_owned())?;
                    let dropped = plugin.restore_bytes > dev::RESTORE_LIMIT_BYTES;
                    let restored_bytes = if dropped { 0 } else { plugin.restore_bytes };
                    if side == "server" {
                        plugin.deactivations.push("upgrade".into());
                        plugin.contributions.clear();
                        plugin.activations += 1;
                        plugin.restored.push(restored_bytes);
                        plugin.state = "running".into();
                        plugin.history.push("running".into());
                        session.journal.push(format!("reload:{id}"));
                    }
                    Ok(SessionResponse::Reloaded {
                        restored_bytes,
                        dropped,
                    })
                })
            }
            SessionRequest::AttachedClients => Ok(SessionResponse::Value(
                self.access("attached-clients".into(), |session| session.clients)
                    .to_string(),
            )),
        }
    }
    fn plugin(&self, id: &str) -> Option<SessionPlugin> {
        self.access(format!("plugin:{id}"), |session| {
            session
                .plugins
                .get(id)
                .filter(|plugin| plugin.installed)
                .map(|plugin| SessionPlugin {
                    dev: plugin.dev,
                    running: plugin.state == "running",
                })
        })
    }
    fn trace_records(&self, id: &str) -> Vec<TraceRecord> {
        self.access(format!("trace-records:{id}"), |session| {
            session
                .plugins
                .get(id)
                .map(|plugin| plugin.traces.clone())
                .unwrap_or_default()
        })
    }
    fn last_tree(&self, id: &str, target: &str) -> Option<InspectNode> {
        self.access(format!("last-tree:{id}:{target}"), |session| {
            session
                .plugins
                .get(id)
                .and_then(|plugin| plugin.trees.get(target).cloned())
        })
    }
    fn profile_exports(&self, id: &str) -> Vec<String> {
        self.access(format!("profile-exports:{id}"), |session| {
            session
                .plugins
                .get(id)
                .map(|plugin| plugin.exports.clone())
                .unwrap_or_default()
        })
    }
    fn profile_calls(&self, id: &str, duration_secs: u32) -> Vec<ProfileCall> {
        self.access(format!("profile-calls:{id}:{duration_secs}"), |session| {
            session
                .plugins
                .get(id)
                .map(|plugin| plugin.profile.clone())
                .unwrap_or_default()
        })
    }
    fn memory_views(&self, id: &str) -> Vec<MemoryView> {
        self.access(format!("memory-views:{id}"), |session| {
            session
                .plugins
                .get(id)
                .map(|plugin| plugin.memory.clone())
                .unwrap_or_default()
        })
    }
    fn counters(&self, id: &str) -> Vec<(Side, Vec<HourBucket>)> {
        self.access(format!("counters:{id}"), |session| {
            session
                .plugins
                .get(id)
                .map(|plugin| plugin.counters.clone())
                .unwrap_or_default()
        })
    }
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
            Ok(wasmparser::Payload::CustomSection(section)) if section.name() == "name" => {
                found = true
            }
            Ok(_) => {}
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

    /// Saved state larger than this is dropped on a reload (DEV-188).
    pub const RESTORE_LIMIT_BYTES: u64 = 1_048_576;

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
            Debouncer {
                window_millis: window.as_millis().min(u64::MAX as u128) as u64,
                last_change: None,
                ready: 0,
            }
        }

        /// Records a change at `at_millis`.
        pub fn change(&mut self, at_millis: u64) {
            let at_millis = self
                .last_change
                .map_or(at_millis, |last| at_millis.max(last));
            if self
                .last_change
                .is_some_and(|last| at_millis.saturating_sub(last) >= self.window_millis)
            {
                self.ready = self.ready.saturating_add(1);
            }
            self.last_change = Some(at_millis);
        }

        /// The rebuilds due at `now_millis`.
        pub fn due(&mut self, now_millis: u64) -> usize {
            if self
                .last_change
                .is_some_and(|last| now_millis >= last && now_millis - last >= self.window_millis)
            {
                self.ready = self.ready.saturating_add(1);
                self.last_change = None;
            }
            std::mem::take(&mut self.ready)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::time::{Duration, SystemTime};

    use ilium_plugin_api::Side;
    use serde_json::Value;

    use super::dev::{Debouncer, DEBOUNCE, RELOAD_BUDGET};
    use super::{
        component_has_name_section, project_input_hash, release_tag_matches, run, run_check_dir,
        run_cli, sha256_sidecar_line, source_map_path, CliEnv, CliOutput, MemorySession,
        SessionBackend, SessionPlugin, ToolProbe,
    };
    use crate::registry_client::{FetchError, Fetcher};

    /// A fetcher that always fails: the command line must work offline.
    struct NoNetwork;

    impl Fetcher for NoNetwork {
        fn get(&self, _url: &str) -> Result<Vec<u8>, FetchError> {
            Err(FetchError::Unreachable)
        }
    }

    /// A tool probe with fixed answers (DEV-010).
    struct FixedProbe {
        rustc_version: Option<String>,
        rust_target_installed: bool,
        free_bytes: u64,
    }

    impl ToolProbe for FixedProbe {
        fn tool_version(&self, tool: &str) -> Option<String> {
            if tool == "rustc" {
                return self.rustc_version.clone();
            }
            Some("1.2.4".to_owned())
        }

        fn rust_target_installed(&self, _target: &str) -> bool {
            self.rust_target_installed
        }

        fn free_disk_bytes(&self, _dir: &Path) -> u64 {
            self.free_bytes
        }
    }

    fn healthy_probe() -> FixedProbe {
        FixedProbe {
            rustc_version: Some("1.90.0".to_owned()),
            rust_target_installed: true,
            free_bytes: 1024 * 1024 * 1024,
        }
    }

    fn scratch_root() -> tempfile::TempDir {
        tempfile::tempdir().expect("scratch directory")
    }

    fn make_env(root: &Path) -> CliEnv {
        let project = root.join("project");
        fs::create_dir_all(&project).expect("project directory");
        CliEnv::new(
            root.join("data"),
            root.join("state"),
            root.join("config"),
            project,
            SystemTime::UNIX_EPOCH + Duration::from_secs(1_893_499_200),
            true,
            Arc::new(NoNetwork),
        )
    }

    /// An environment whose working directory holds a Cargo project (so the doctor's Rust checks apply).
    fn rust_project_env(root: &Path) -> CliEnv {
        let env = make_env(root);
        fs::write(env.cwd.join("Cargo.toml"), "[package]\nname = \"tools\"\n").expect("Cargo.toml");
        env
    }

    fn path_arg(path: &Path) -> String {
        path.to_string_lossy().into_owned()
    }

    fn run_words(env: &CliEnv, words: &[&str]) -> CliOutput {
        let owned: Vec<String> = words.iter().map(|word| (*word).to_owned()).collect();
        run(&owned, env)
    }

    fn records<'a>(out: &'a CliOutput, kind: &str) -> Vec<&'a Value> {
        out.stdout_jsonl
            .iter()
            .filter(|record| record["type"] == kind)
            .collect()
    }

    fn error_codes(out: &CliOutput) -> Vec<String> {
        records(out, "error")
            .iter()
            .filter_map(|record| record["code"].as_str().map(str::to_owned))
            .collect()
    }

    fn has_error_code(out: &CliOutput, code: &str) -> bool {
        error_codes(out).iter().any(|found| found == code)
    }

    /// Checks the JSONL envelope: `summary` then `result` as the last two lines, and the
    /// `result.exit_code` equal to the process exit code (DEV-007, DEV-017).
    fn assert_envelope(out: &CliOutput) {
        let count = out.stdout_jsonl.len();
        assert!(
            count >= 2,
            "a JSONL command ends with summary and result, got {count} records"
        );
        assert_eq!(out.stdout_jsonl[count - 2]["type"], "summary");
        assert_eq!(out.stdout_jsonl[count - 1]["type"], "result");
        assert_eq!(
            out.stdout_jsonl[count - 1]["exit_code"],
            i64::from(out.exit_code)
        );
    }

    fn any_stdout(out: &CliOutput) -> String {
        if out.stdout_raw.trim().is_empty() {
            return out.stdout_text.clone();
        }
        out.stdout_raw.clone()
    }

    fn doctor_field(out: &CliOutput, item: &str, field: &str) -> Option<String> {
        records(out, "doctor-check")
            .into_iter()
            .find(|record| record["item"] == item)
            .and_then(|record| record[field].as_str().map(str::to_owned))
    }

    fn doctor_any_fail(out: &CliOutput) -> bool {
        records(out, "doctor-check")
            .iter()
            .any(|record| record["status"] == "fail")
    }

    fn doctor_env(root: &Path, probe: FixedProbe) -> CliEnv {
        rust_project_env(root).with_probe(Arc::new(probe))
    }

    fn module_with_custom_section(section_name: &str) -> Vec<u8> {
        let mut payload = vec![u8::try_from(section_name.len()).expect("short section name")];
        payload.extend_from_slice(section_name.as_bytes());
        let mut bytes = b"\0asm\x01\x00\x00\x00".to_vec();
        bytes.push(0);
        bytes.push(u8::try_from(payload.len()).expect("short payload"));
        bytes.extend_from_slice(&payload);
        bytes
    }

    fn write_project(dir: &Path) {
        fs::create_dir_all(dir.join("src")).expect("source folder");
        fs::write(
            dir.join("manifest.json"),
            "{\"id\":\"example.tools\",\"version\":\"0.1.0\"}",
        )
        .expect("manifest");
        fs::write(dir.join("Cargo.toml"), "[package]\nname = \"tools\"\n").expect("Cargo.toml");
        fs::write(dir.join("Cargo.lock"), "version = 4\n").expect("lockfile");
        fs::write(
            dir.join("src").join("lib.rs"),
            "pub fn answer() -> u32 { 42 }\n",
        )
        .expect("source file");
    }

    // -----------------------------------------------------------------------------------------
    // Environment builders
    // -----------------------------------------------------------------------------------------

    /// REQ: DEV-005
    #[test]
    fn cli_env_new_leaves_optional_collaborators_unset() {
        let scratch = scratch_root();
        let env = make_env(scratch.path());
        assert!(env.probe.is_none());
        assert!(env.builder.is_none());
        assert!(env.runner.is_none());
        assert!(env.session.is_none());
        assert!(env.interrupt.is_none());
        assert!(env.dev_script.is_empty());
        assert!(env.env.is_empty());
    }

    /// REQ: DEV-029
    #[test]
    fn cli_env_with_var_inserts_and_overwrites_a_variable() {
        let scratch = scratch_root();
        let env = make_env(scratch.path())
            .with_var("ILIUM_PLUGIN_OFFLINE", "1")
            .with_var("ILIUM_PLUGIN_OFFLINE", "0");
        assert_eq!(
            env.env.get("ILIUM_PLUGIN_OFFLINE").map(String::as_str),
            Some("0")
        );
    }

    // -----------------------------------------------------------------------------------------
    // Debouncer of `plugin dev` and `build --watch`
    // -----------------------------------------------------------------------------------------

    /// REQ: DEV-853, DEV-185
    #[test]
    fn debounce_window_is_300_ms_and_reload_budget_is_one_second() {
        assert_eq!(DEBOUNCE, Duration::from_millis(300));
        assert_eq!(RELOAD_BUDGET, Duration::from_secs(1));
    }

    /// REQ: DEV-186
    #[test]
    fn debouncer_owes_no_rebuild_without_changes() {
        let mut debouncer = Debouncer::new(DEBOUNCE);
        assert_eq!(debouncer.due(0), 0);
        assert_eq!(debouncer.due(10_000), 0);
    }

    /// REQ: DEV-185
    #[test]
    fn debouncer_fires_once_exactly_300_ms_after_the_last_change() {
        let mut debouncer = Debouncer::new(DEBOUNCE);
        debouncer.change(0);
        assert_eq!(debouncer.due(299), 0);
        assert_eq!(debouncer.due(300), 1);
        assert_eq!(debouncer.due(301), 0);
    }

    /// REQ: DEV-185, DEV-186
    #[test]
    fn debouncer_coalesces_a_burst_into_one_rebuild() {
        let mut debouncer = Debouncer::new(DEBOUNCE);
        debouncer.change(0);
        debouncer.change(100);
        debouncer.change(250);
        assert_eq!(
            debouncer.due(549),
            0,
            "the quiet window restarts at the last change"
        );
        assert_eq!(debouncer.due(550), 1);
        assert_eq!(debouncer.due(900), 0);
    }

    /// REQ: DEV-186
    #[test]
    fn debouncer_queues_a_change_made_during_a_build_as_one_more_rebuild() {
        let mut debouncer = Debouncer::new(DEBOUNCE);
        debouncer.change(0);
        assert_eq!(debouncer.due(300), 1);
        debouncer.change(400);
        assert_eq!(debouncer.due(500), 0);
        assert_eq!(debouncer.due(700), 1);
        assert_eq!(debouncer.due(1000), 0);
    }

    // -----------------------------------------------------------------------------------------
    // Release and checksum helpers
    // -----------------------------------------------------------------------------------------

    /// REQ: DEV-741
    #[test]
    fn release_tag_accepts_v_prefix_of_the_manifest_version() {
        assert!(release_tag_matches("v0.3.0", "0.3.0"));
        assert!(release_tag_matches("v1.0.0-beta.1", "1.0.0-beta.1"));
    }

    /// REQ: DEV-741
    #[test]
    fn release_tag_without_v_prefix_does_not_match() {
        assert!(!release_tag_matches("0.3.0", "0.3.0"));
        assert!(!release_tag_matches("vv0.3.0", "0.3.0"));
    }

    /// REQ: DEV-741
    #[test]
    fn release_tag_rejects_other_versions_and_empty_tags() {
        assert!(!release_tag_matches("v0.3.1", "0.3.0"));
        assert!(!release_tag_matches("v0.3", "0.3.0"));
        assert!(!release_tag_matches("v0.3.0-rc1", "0.3.0"));
        assert!(!release_tag_matches("", "0.3.0"));
    }

    /// REQ: DEV-773
    #[test]
    fn sidecar_line_for_empty_input_uses_the_known_empty_digest() {
        assert_eq!(
            sha256_sidecar_line(b"", "plugin.iliumpkg"),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  plugin.iliumpkg"
        );
    }

    /// REQ: DEV-773
    #[test]
    fn sidecar_line_for_abc_uses_two_spaces_before_the_name() {
        assert_eq!(
            sha256_sidecar_line(b"abc", "dist.iliumpkg"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad  dist.iliumpkg"
        );
    }

    /// REQ: DEV-773
    #[test]
    fn sidecar_line_keeps_a_file_name_with_spaces_verbatim() {
        let line = sha256_sidecar_line(b"abc", "my plugin.iliumpkg");
        assert!(line.ends_with("  my plugin.iliumpkg"));
        assert_eq!(line.len(), 64 + 2 + "my plugin.iliumpkg".len());
    }

    // -----------------------------------------------------------------------------------------
    // Build output helpers
    // -----------------------------------------------------------------------------------------

    /// REQ: DEV-303, DEV-715
    #[test]
    fn source_map_path_is_dist_side_wasm_map() {
        assert_eq!(
            source_map_path(Side::Server),
            PathBuf::from("dist/server.wasm.map")
        );
        assert_eq!(
            source_map_path(Side::Client),
            PathBuf::from("dist/client.wasm.map")
        );
        assert_eq!(
            source_map_path(Side::Scene),
            PathBuf::from("dist/scene.wasm.map")
        );
    }

    /// REQ: DEV-303
    #[test]
    fn component_with_name_section_is_detected() {
        assert!(component_has_name_section(&module_with_custom_section(
            "name"
        )));
    }

    /// REQ: DEV-303
    #[test]
    fn component_with_only_other_custom_sections_has_no_name_section() {
        assert!(!component_has_name_section(&module_with_custom_section(
            "producers"
        )));
    }

    /// REQ: DEV-303
    #[test]
    fn empty_or_truncated_bytes_have_no_name_section() {
        assert!(!component_has_name_section(&[]));
        assert!(!component_has_name_section(b"not wasm at all"));
        let bytes = module_with_custom_section("name");
        assert!(!component_has_name_section(&bytes[..12]));
    }

    /// REQ: DEV-305
    #[test]
    fn input_hash_of_a_folder_without_manifest_fails() {
        let scratch = scratch_root();
        assert!(project_input_hash(scratch.path()).is_err());
    }

    /// REQ: DEV-305, DEV-018
    #[test]
    fn input_hash_is_64_lowercase_hex_characters() {
        let scratch = scratch_root();
        write_project(scratch.path());
        let hash = project_input_hash(scratch.path()).expect("hash of a complete project");
        assert_eq!(hash.len(), 64);
        assert!(hash
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)));
    }

    /// REQ: DEV-305
    #[test]
    fn input_hash_is_stable_for_unchanged_inputs() {
        let scratch = scratch_root();
        write_project(scratch.path());
        let first = project_input_hash(scratch.path()).expect("first hash");
        let second = project_input_hash(scratch.path()).expect("second hash");
        assert_eq!(first, second);
    }

    /// REQ: DEV-305
    #[test]
    fn input_hash_changes_when_a_source_file_changes() {
        let scratch = scratch_root();
        write_project(scratch.path());
        let before = project_input_hash(scratch.path()).expect("hash before the edit");
        fs::write(
            scratch.path().join("src").join("lib.rs"),
            "pub fn answer() -> u32 { 43 }\n",
        )
        .expect("edit source");
        let after = project_input_hash(scratch.path()).expect("hash after the edit");
        assert_ne!(before, after);
    }

    /// REQ: DEV-305
    #[test]
    fn input_hash_changes_when_the_manifest_changes() {
        let scratch = scratch_root();
        write_project(scratch.path());
        let before = project_input_hash(scratch.path()).expect("hash before the edit");
        fs::write(
            scratch.path().join("manifest.json"),
            "{\"id\":\"example.tools\",\"version\":\"0.1.1\"}",
        )
        .expect("edit manifest");
        let after = project_input_hash(scratch.path()).expect("hash after the edit");
        assert_ne!(before, after);
    }

    // -----------------------------------------------------------------------------------------
    // Output envelope, global flags, usage errors
    // -----------------------------------------------------------------------------------------

    /// REQ: DEV-010, DEV-016, DEV-017, DEV-007
    #[test]
    fn unknown_flag_is_a_usage_error_that_writes_nothing() {
        let scratch = scratch_root();
        let env = make_env(scratch.path());
        let target = scratch.path().join("fresh");
        let target_arg = path_arg(&target);
        let out = run_words(
            &env,
            &[
                "new",
                target_arg.as_str(),
                "--id",
                "example.tools",
                "--lang",
                "rust",
                "--template",
                "server",
                "--bogus",
            ],
        );
        assert_eq!(out.exit_code, 2);
        assert!(has_error_code(&out, "usage.unknown-flag"));
        assert_envelope(&out);
        assert!(
            !target.exists(),
            "a usage error returns before any file is written"
        );
    }

    /// REQ: DEV-026
    #[test]
    fn json_flag_on_a_jsonl_command_changes_nothing() {
        let scratch = scratch_root();
        let env = make_env(scratch.path());
        let target = scratch.path().join("fresh");
        let target_arg = path_arg(&target);
        let out = run_words(&env, &["new", target_arg.as_str(), "--json"]);
        assert_eq!(out.exit_code, 2);
        assert!(has_error_code(&out, "usage.missing-argument"));
        assert_envelope(&out);
    }

    /// REQ: DEV-003
    #[test]
    fn help_flag_prints_usage_and_exits_zero() {
        let scratch = scratch_root();
        let env = make_env(scratch.path());
        let out = run_words(&env, &["new", "--help"]);
        assert_eq!(out.exit_code, 0);
        assert!(!any_stdout(&out).trim().is_empty());
    }

    /// REQ: DEV-003, DEV-005
    #[test]
    fn offline_command_ignores_the_session_flag() {
        let scratch = scratch_root();
        let env = rust_project_env(scratch.path()).with_probe(Arc::new(healthy_probe()));
        let out = run_words(&env, &["doctor", "--json", "--session", "other"]);
        assert_ne!(out.exit_code, 6);
        assert!(!has_error_code(&out, "session.unreachable"));
    }

    /// REQ: DEV-020
    #[test]
    fn quiet_suppresses_progress_records_and_keeps_the_envelope() {
        let scratch = scratch_root();
        let env = make_env(scratch.path());
        let quiet_target = path_arg(&scratch.path().join("quiet"));
        let loud_target = path_arg(&scratch.path().join("loud"));
        let quiet = run_words(
            &env,
            &[
                "new",
                quiet_target.as_str(),
                "--id",
                "example.tools",
                "--lang",
                "rust",
                "--template",
                "server",
                "--quiet",
            ],
        );
        let loud = run_words(
            &env,
            &[
                "new",
                loud_target.as_str(),
                "--id",
                "example.tools",
                "--lang",
                "rust",
                "--template",
                "server",
            ],
        );
        assert!(records(&quiet, "progress").is_empty());
        assert!(!records(&loud, "progress").is_empty());
        assert_envelope(&quiet);
    }

    /// REQ: DEV-016, DEV-017
    #[test]
    fn run_cli_returns_the_same_jsonl_lines_as_run() {
        let scratch = scratch_root();
        let env = rust_project_env(scratch.path()).with_probe(Arc::new(healthy_probe()));
        let owned: Vec<String> = vec!["doctor".to_owned(), "--json".to_owned()];
        let raw = run_cli(&env, &owned);
        let parsed = run_words(&env, &["doctor", "--json"]);
        assert_eq!(raw.exit_code, parsed.exit_code);
        let lines: Vec<&str> = raw.stdout.lines().collect();
        assert_eq!(lines.len(), parsed.stdout_jsonl.len());
        for (index, line) in lines.iter().enumerate() {
            let value: Value =
                serde_json::from_str(line).expect("each stdout line is one JSON object");
            assert_eq!(value, parsed.stdout_jsonl[index]);
        }
    }

    /// REQ: API-080, DEV-070
    #[test]
    fn check_of_an_empty_directory_fails_with_a_manifest_code() {
        let scratch = scratch_root();
        let out = run_check_dir(scratch.path());
        assert_eq!(out.exit_code, 1);
        let mentions_manifest = error_codes(&out)
            .iter()
            .any(|code| code.starts_with("manifest."))
            || out.stderr.contains("manifest.");
        assert!(
            mentions_manifest,
            "a missing manifest is reported with a manifest.* code"
        );
    }

    // -----------------------------------------------------------------------------------------
    // new
    // -----------------------------------------------------------------------------------------

    /// REQ: DEV-040, DEV-010
    #[test]
    fn new_without_id_is_a_usage_error_and_writes_nothing() {
        let scratch = scratch_root();
        let env = make_env(scratch.path());
        let target = scratch.path().join("no-id");
        let target_arg = path_arg(&target);
        let out = run_words(
            &env,
            &[
                "new",
                target_arg.as_str(),
                "--lang",
                "rust",
                "--template",
                "server",
            ],
        );
        assert_eq!(out.exit_code, 2);
        assert!(has_error_code(&out, "usage.missing-argument"));
        assert!(!target.exists());
    }

    /// REQ: DEV-040
    #[test]
    fn new_rejects_template_and_side_that_name_different_templates() {
        let scratch = scratch_root();
        let env = make_env(scratch.path());
        let target = scratch.path().join("conflict");
        let target_arg = path_arg(&target);
        let out = run_words(
            &env,
            &[
                "new",
                target_arg.as_str(),
                "--id",
                "example.tools",
                "--lang",
                "rust",
                "--template",
                "server",
                "--side",
                "client",
                "--json",
            ],
        );
        assert_eq!(out.exit_code, 2);
        assert!(has_error_code(&out, "usage.conflict"));
        assert!(!target.exists());
    }

    /// REQ: DEV-040
    #[test]
    fn new_accepts_template_and_side_that_name_the_same_template() {
        let scratch = scratch_root();
        let env = make_env(scratch.path());
        let target_arg = path_arg(&scratch.path().join("matching"));
        let out = run_words(
            &env,
            &[
                "new",
                target_arg.as_str(),
                "--id",
                "example.tools",
                "--lang",
                "rust",
                "--template",
                "server",
                "--side",
                "server",
            ],
        );
        assert_eq!(out.exit_code, 0);
        assert_envelope(&out);
    }

    /// REQ: DEV-042
    #[test]
    fn new_rejects_an_ilium_id_without_first_party() {
        let scratch = scratch_root();
        let env = make_env(scratch.path());
        let target_arg = path_arg(&scratch.path().join("reserved"));
        let out = run_words(
            &env,
            &[
                "new",
                target_arg.as_str(),
                "--id",
                "ilium.tools",
                "--lang",
                "rust",
                "--template",
                "server",
                "--json",
            ],
        );
        assert_eq!(out.exit_code, 1);
        assert!(has_error_code(&out, "new.id-reserved"));
    }

    /// REQ: DEV-042
    #[test]
    fn new_rejects_ids_outside_the_pattern() {
        let scratch = scratch_root();
        let env = make_env(scratch.path());
        let ids = [
            "example",
            "Example.Tools",
            "example..tools",
            "example.tools.",
        ];
        for (index, id) in ids.iter().enumerate() {
            let target_arg = path_arg(&scratch.path().join(format!("bad-id-{index}")));
            let out = run_words(
                &env,
                &[
                    "new",
                    target_arg.as_str(),
                    "--id",
                    *id,
                    "--lang",
                    "rust",
                    "--template",
                    "server",
                    "--json",
                ],
            );
            assert_eq!(out.exit_code, 1, "id {id} must fail");
            assert!(
                has_error_code(&out, "new.id-invalid"),
                "id {id} gives new.id-invalid"
            );
        }
    }

    /// REQ: DEV-043
    #[test]
    fn new_rejects_an_unknown_template_before_writing() {
        let scratch = scratch_root();
        let env = make_env(scratch.path());
        let target = scratch.path().join("unknown-template");
        let target_arg = path_arg(&target);
        let out = run_words(
            &env,
            &[
                "new",
                target_arg.as_str(),
                "--id",
                "example.tools",
                "--lang",
                "rust",
                "--template",
                "nope",
                "--json",
            ],
        );
        assert_eq!(out.exit_code, 2);
        assert!(has_error_code(&out, "new.template-unknown"));
        assert!(!target.exists());
    }

    /// REQ: DEV-044, DEV-008
    #[test]
    fn new_reports_one_inventory_progress_record_per_written_file() {
        let scratch = scratch_root();
        let env = make_env(scratch.path());
        let target = scratch.path().join("fresh");
        let target_arg = path_arg(&target);
        let out = run_words(
            &env,
            &[
                "new",
                target_arg.as_str(),
                "--id",
                "example.tools",
                "--lang",
                "rust",
                "--template",
                "server",
            ],
        );
        assert_eq!(out.exit_code, 0);
        assert_envelope(&out);
        assert_eq!(out.stdout_jsonl[out.stdout_jsonl.len() - 1]["ok"], true);
        let inventory: Vec<&Value> = records(&out, "progress")
            .into_iter()
            .filter(|record| record["step"] == "inventory")
            .collect();
        assert!(
            !inventory.is_empty(),
            "new writes files, so it reports each one"
        );
        for record in inventory {
            let relative = record["message"]
                .as_str()
                .expect("inventory message is a path");
            assert!(!Path::new(relative).is_absolute());
            assert!(
                target.join(relative).is_file(),
                "{relative} is a file of the new project"
            );
        }
        assert!(records(&out, "artifact").is_empty());
    }

    /// REQ: DEV-041
    #[test]
    fn new_refuses_a_non_empty_folder_without_force() {
        let scratch = scratch_root();
        let env = make_env(scratch.path());
        let target = scratch.path().join("occupied");
        fs::create_dir_all(&target).expect("occupied folder");
        fs::write(target.join("keep.txt"), "keep").expect("existing file");
        let target_arg = path_arg(&target);
        let out = run_words(
            &env,
            &[
                "new",
                target_arg.as_str(),
                "--id",
                "example.tools",
                "--lang",
                "rust",
                "--template",
                "server",
                "--json",
            ],
        );
        assert_eq!(out.exit_code, 1);
        assert!(has_error_code(&out, "new.dir-not-empty"));
        assert_eq!(
            fs::read_to_string(target.join("keep.txt")).expect("kept file"),
            "keep"
        );
    }

    /// REQ: DEV-041
    #[test]
    fn new_accepts_an_existing_folder_with_no_files() {
        let scratch = scratch_root();
        let env = make_env(scratch.path());
        let target = scratch.path().join("empty-existing");
        fs::create_dir_all(&target).expect("empty folder");
        let target_arg = path_arg(&target);
        let out = run_words(
            &env,
            &[
                "new",
                target_arg.as_str(),
                "--id",
                "example.tools",
                "--lang",
                "rust",
                "--template",
                "server",
            ],
        );
        assert_eq!(out.exit_code, 0);
        assert_envelope(&out);
    }

    /// REQ: DEV-041
    #[test]
    fn new_with_force_leaves_unrelated_files_alone() {
        let scratch = scratch_root();
        let env = make_env(scratch.path());
        let target = scratch.path().join("forced");
        fs::create_dir_all(&target).expect("folder");
        fs::write(target.join("notes.md"), "mine").expect("unrelated file");
        let target_arg = path_arg(&target);
        let out = run_words(
            &env,
            &[
                "new",
                target_arg.as_str(),
                "--id",
                "example.tools",
                "--lang",
                "rust",
                "--template",
                "server",
                "--force",
            ],
        );
        assert_eq!(out.exit_code, 0);
        assert_eq!(
            fs::read_to_string(target.join("notes.md")).expect("unrelated file"),
            "mine"
        );
    }

    // -----------------------------------------------------------------------------------------
    // Sessions
    // -----------------------------------------------------------------------------------------

    /// REQ: DEV-004, DEV-006, DEV-014
    #[test]
    fn session_command_without_a_session_reports_unreachable_once() {
        let scratch = scratch_root();
        let env = make_env(scratch.path());
        let out = run_words(&env, &["list", "--json"]);
        assert_eq!(out.exit_code, 6);
        assert_eq!(error_codes(&out), vec!["session.unreachable".to_owned()]);
        assert_envelope(&out);
    }

    /// REQ: DEV-006
    #[test]
    fn unreachable_session_is_not_retried() {
        let scratch = scratch_root();
        let session = MemorySession::unreachable();
        let env = make_env(scratch.path()).with_session(session.clone());
        let out = run_words(&env, &["list", "--json"]);
        assert_eq!(out.exit_code, 6);
        assert_eq!(session.connect_attempts(), 1);
    }

    /// REQ: DEV-180
    #[test]
    fn dev_and_enable_without_a_session_exit_six() {
        let scratch = scratch_root();
        let env = make_env(scratch.path());
        let project_arg = path_arg(&scratch.path().join("project"));
        let dev = run_words(&env, &["dev", project_arg.as_str(), "--json"]);
        assert_eq!(dev.exit_code, 6);
        let enable = run_words(&env, &["enable", "example.tools", "--json"]);
        assert_eq!(enable.exit_code, 6);
    }

    /// REQ: DEV-165
    #[test]
    fn list_prints_each_installed_plugin_with_id_version_and_tier() {
        let scratch = scratch_root();
        let session = MemorySession::new();
        session.add_plugin(
            "example.tools",
            "0.1.0",
            "T1",
            "running",
            false,
            &[("fs.read", "low", "granted")],
        );
        let env = make_env(scratch.path()).with_session(session.clone());
        let out = run_words(&env, &["list"]);
        assert_eq!(out.exit_code, 0);
        assert!(out.stdout_text.contains("example.tools"));
        assert!(out.stdout_text.contains("0.1.0"));
        assert!(out.stdout_text.contains("T1"));
    }

    /// REQ: DEV-165
    #[test]
    fn list_marks_a_development_install_with_the_dev_badge() {
        let scratch = scratch_root();
        let session = MemorySession::new();
        session.add_plugin("example.tools", "0.1.0", "T3", "running", true, &[]);
        let env = make_env(scratch.path()).with_session(session.clone());
        let out = run_words(&env, &["list"]);
        assert_eq!(out.exit_code, 0);
        assert!(out.stdout_text.contains("[dev]"));
        assert!(out.stdout_text.contains("example.tools"));
    }

    /// REQ: DEV-165
    #[test]
    fn list_json_with_no_installed_plugins_prints_no_plugin_records() {
        let scratch = scratch_root();
        let session = MemorySession::new();
        let env = make_env(scratch.path()).with_session(session.clone());
        let out = run_words(&env, &["list", "--json"]);
        assert_eq!(out.exit_code, 0);
        assert!(records(&out, "plugin").is_empty());
        assert_envelope(&out);
    }

    // -----------------------------------------------------------------------------------------
    // Lifecycle: uninstall, enable, disable, grants
    // -----------------------------------------------------------------------------------------

    /// REQ: DEV-155
    #[test]
    fn uninstall_with_keep_data_and_purge_is_a_usage_conflict() {
        let scratch = scratch_root();
        let session = MemorySession::new();
        session.add_plugin("example.tools", "0.1.0", "T1", "running", false, &[]);
        let env = make_env(scratch.path()).with_session(session.clone());
        let out = run_words(
            &env,
            &[
                "uninstall",
                "example.tools",
                "--keep-data",
                "--purge",
                "--json",
            ],
        );
        assert_eq!(out.exit_code, 2);
        assert!(has_error_code(&out, "usage.conflict"));
        assert!(
            session.journal().is_empty(),
            "a usage error changes nothing"
        );
        assert!(session.package_installed("example.tools"));
    }

    /// REQ: DEV-157
    #[test]
    fn uninstall_of_an_unknown_plugin_is_not_found() {
        let scratch = scratch_root();
        let session = MemorySession::new();
        let env = make_env(scratch.path()).with_session(session.clone());
        let out = run_words(&env, &["uninstall", "example.missing", "--json"]);
        assert_eq!(out.exit_code, 5);
        assert!(has_error_code(&out, "plugin.not-found"));
    }

    /// REQ: DEV-161, DEV-160
    #[test]
    fn disable_twice_succeeds_and_keeps_the_same_state() {
        let scratch = scratch_root();
        let session = MemorySession::new();
        session.add_plugin("example.tools", "0.1.0", "T1", "running", false, &[]);
        let env = make_env(scratch.path()).with_session(session.clone());
        let first = run_words(&env, &["disable", "example.tools", "--json"]);
        assert_eq!(first.exit_code, 0);
        let state_after_first = session.plugin_state("example.tools");
        let second = run_words(&env, &["disable", "example.tools", "--json"]);
        assert_eq!(second.exit_code, 0);
        assert_eq!(session.plugin_state("example.tools"), state_after_first);
    }

    /// REQ: DEV-160
    #[test]
    fn enable_resets_the_failure_count_to_zero() {
        let scratch = scratch_root();
        let session = MemorySession::new();
        session.add_plugin("example.tools", "0.1.0", "T1", "disabled", false, &[]);
        session.set_failure_count("example.tools", 3);
        let env = make_env(scratch.path()).with_session(session.clone());
        let out = run_words(&env, &["enable", "example.tools", "--json"]);
        assert_eq!(out.exit_code, 0);
        assert_eq!(session.failure_count("example.tools"), 0);
    }

    /// REQ: DEV-170
    #[test]
    fn grants_lists_each_requested_grant_with_its_state() {
        let scratch = scratch_root();
        let session = MemorySession::new();
        session.add_plugin(
            "example.tools",
            "0.1.0",
            "T1",
            "running",
            false,
            &[("fs.read", "low", "granted")],
        );
        let env = make_env(scratch.path()).with_session(session.clone());
        let out = run_words(&env, &["grants", "example.tools"]);
        assert_eq!(out.exit_code, 0);
        assert!(out.stdout_text.contains("fs.read"));
        assert!(out.stdout_text.contains("granted"));
    }

    /// REQ: DEV-172
    #[test]
    fn grant_not_requested_by_the_manifest_is_refused() {
        let scratch = scratch_root();
        let session = MemorySession::new();
        session.add_plugin(
            "example.tools",
            "0.1.0",
            "T1",
            "running",
            false,
            &[("fs.read", "low", "granted")],
        );
        let env = make_env(scratch.path()).with_session(session.clone());
        let out = run_words(
            &env,
            &["grants", "example.tools", "--grant", "net.egress", "--json"],
        );
        assert_eq!(out.exit_code, 1);
        assert!(has_error_code(&out, "grants.not-requested"));
    }

    /// REQ: DEV-171
    #[test]
    fn revoke_withdraws_the_grant_and_restarts_the_plugin() {
        let scratch = scratch_root();
        let session = MemorySession::new();
        session.add_plugin(
            "example.tools",
            "0.1.0",
            "T1",
            "running",
            false,
            &[("fs.read", "low", "granted")],
        );
        let env = make_env(scratch.path()).with_session(session.clone());
        let out = run_words(
            &env,
            &["grants", "example.tools", "--revoke", "fs.read", "--json"],
        );
        assert_eq!(out.exit_code, 0);
        assert_eq!(
            session.grant_state("example.tools", "fs.read"),
            Some("revoked".to_owned())
        );
        assert_eq!(session.restarts("example.tools"), 1);
    }

    // -----------------------------------------------------------------------------------------
    // Signing, publishing and trust
    // -----------------------------------------------------------------------------------------

    /// REQ: DEV-135
    #[test]
    fn publish_without_registry_is_a_missing_argument() {
        let scratch = scratch_root();
        let env = make_env(scratch.path());
        let out = run_words(&env, &["publish", "pkg.iliumpkg", "--json"]);
        assert_eq!(out.exit_code, 2);
        assert!(has_error_code(&out, "usage.missing-argument"));
    }

    /// REQ: DEV-115
    #[test]
    fn sign_without_a_key_is_a_missing_argument() {
        let scratch = scratch_root();
        let env = make_env(scratch.path());
        let package = scratch.path().join("pkg.iliumpkg");
        fs::write(&package, "not a package").expect("package file");
        let package_arg = path_arg(&package);
        let out = run_words(&env, &["sign", package_arg.as_str(), "--json"]);
        assert_eq!(out.exit_code, 2);
        assert!(has_error_code(&out, "usage.missing-argument"));
    }

    /// REQ: DEV-116
    #[test]
    fn sign_with_an_unparseable_key_fails() {
        let scratch = scratch_root();
        let key = scratch.path().join("publisher.key");
        fs::write(&key, "not a key").expect("key file");
        let package = scratch.path().join("pkg.iliumpkg");
        fs::write(&package, "not a package").expect("package file");
        let env = make_env(scratch.path()).with_var("ILIUM_PLUGIN_SIGNING_KEY", &path_arg(&key));
        let package_arg = path_arg(&package);
        let out = run_words(&env, &["sign", package_arg.as_str(), "--json"]);
        assert_eq!(out.exit_code, 1);
        assert!(
            has_error_code(&out, "sign.key-unreadable")
                || has_error_code(&out, "sign.key-permissions"),
            "an unparseable key is refused with a sign.key code"
        );
    }

    /// REQ: DEV-127
    #[test]
    fn trust_remove_of_an_unlisted_key_is_not_found() {
        let scratch = scratch_root();
        let store = scratch.path().join("trust.json");
        let env = make_env(scratch.path()).with_var("ILIUM_TRUST_STORE", &path_arg(&store));
        let key_id = "0".repeat(64);
        let out = run_words(&env, &["trust", "remove", key_id.as_str(), "--json"]);
        assert_eq!(out.exit_code, 5);
        assert!(has_error_code(&out, "trust.not-found"));
    }

    /// REQ: DEV-128
    #[test]
    fn trust_list_with_an_absent_store_succeeds() {
        let scratch = scratch_root();
        let store = scratch.path().join("trust.json");
        let env = make_env(scratch.path()).with_var("ILIUM_TRUST_STORE", &path_arg(&store));
        let out = run_words(&env, &["trust", "list"]);
        assert_eq!(out.exit_code, 0);
    }

    // -----------------------------------------------------------------------------------------
    // Logs, trace, profile, inspect
    // -----------------------------------------------------------------------------------------

    /// REQ: DEV-191
    #[test]
    fn logs_of_an_unknown_plugin_are_not_found() {
        let scratch = scratch_root();
        let session = MemorySession::new();
        let env = make_env(scratch.path()).with_session(session.clone());
        let out = run_words(&env, &["logs", "example.missing", "--json"]);
        assert_eq!(out.exit_code, 5);
        assert!(has_error_code(&out, "plugin.not-found"));
    }

    /// REQ: DEV-820
    #[test]
    fn trace_of_an_unknown_plugin_is_not_found() {
        let scratch = scratch_root();
        let session = MemorySession::new();
        let env = make_env(scratch.path()).with_session(session.clone());
        let out = run_words(&env, &["trace", "example.missing", "--json"]);
        assert_eq!(out.exit_code, 5);
        assert!(has_error_code(&out, "plugin.not-found"));
    }

    /// REQ: DEV-823
    #[test]
    fn unredacted_trace_is_denied_for_a_non_dev_install() {
        let scratch = scratch_root();
        let session = MemorySession::new();
        session.add_plugin("example.tools", "0.1.0", "T1", "running", false, &[]);
        let env = make_env(scratch.path()).with_session(session.clone());
        let out = run_words(&env, &["trace", "example.tools", "--unredacted", "--json"]);
        assert_eq!(out.exit_code, 1);
        assert!(has_error_code(&out, "trace.unredacted-denied"));
    }

    /// REQ: DEV-830
    #[test]
    fn profile_duration_accepts_one_to_600_and_refuses_outside() {
        let scratch = scratch_root();
        let session = MemorySession::new();
        session.add_plugin("example.tools", "0.1.0", "T1", "running", false, &[]);
        let env = make_env(scratch.path()).with_session(session.clone());
        for rejected in ["0", "601"] {
            let out = run_words(
                &env,
                &["profile", "example.tools", "--duration", rejected, "--json"],
            );
            assert_eq!(out.exit_code, 2, "duration {rejected} is outside 1 to 600");
            assert!(has_error_code(&out, "usage.invalid-value"));
        }
        for accepted in ["1", "600"] {
            let out = run_words(
                &env,
                &["profile", "example.tools", "--duration", accepted, "--json"],
            );
            assert_eq!(out.exit_code, 0, "duration {accepted} is inside 1 to 600");
        }
    }

    /// REQ: DEV-013, DEV-800
    #[test]
    fn inspect_of_an_unknown_installed_id_is_not_found() {
        let scratch = scratch_root();
        let session = MemorySession::new();
        let env = make_env(scratch.path()).with_session(session.clone());
        let out = run_words(&env, &["inspect", "example.missing", "--json"]);
        assert_eq!(out.exit_code, 5);
        assert!(has_error_code(&out, "inspect.id-unknown"));
    }

    /// REQ: DEV-803
    #[test]
    fn inspect_of_a_text_file_is_not_a_package() {
        let scratch = scratch_root();
        let env = make_env(scratch.path());
        let notes = scratch.path().join("notes.txt");
        fs::write(&notes, "hello").expect("text file");
        let notes_arg = path_arg(&notes);
        let out = run_words(&env, &["inspect", notes_arg.as_str(), "--json"]);
        assert_eq!(out.exit_code, 1);
        assert!(has_error_code(&out, "inspect.not-a-package"));
    }

    /// REQ: DEV-800
    #[test]
    fn inspect_of_an_installed_id_needs_a_session() {
        let scratch = scratch_root();
        let env = make_env(scratch.path());
        let out = run_words(&env, &["inspect", "example.tools", "--json"]);
        assert_eq!(out.exit_code, 6);
        assert!(has_error_code(&out, "session.unreachable"));
    }

    // -----------------------------------------------------------------------------------------
    // doctor, check, lint
    // -----------------------------------------------------------------------------------------

    /// REQ: DEV-810
    #[test]
    fn doctor_prints_its_items_in_the_documented_order() {
        let scratch = scratch_root();
        let env = doctor_env(scratch.path(), healthy_probe());
        let out = run_words(&env, &["doctor", "--json"]);
        let items: Vec<String> = records(&out, "doctor-check")
            .iter()
            .filter_map(|record| record["item"].as_str().map(str::to_owned))
            .collect();
        let expected = vec![
            "rustc",
            "rust-target",
            "bun",
            "sdk-pin",
            "wit-pins",
            "host-version",
            "platform-tiers",
            "trust-store",
            "session",
            "disk",
        ];
        assert_eq!(items, expected);
    }

    /// REQ: DEV-811
    #[test]
    fn doctor_exits_four_exactly_when_an_item_fails() {
        let scratch = scratch_root();
        let probe = FixedProbe {
            rust_target_installed: false,
            ..healthy_probe()
        };
        let env = doctor_env(scratch.path(), probe);
        let out = run_words(&env, &["doctor", "--json"]);
        let expected_code = if doctor_any_fail(&out) { 4 } else { 0 };
        assert_eq!(out.exit_code, expected_code);
        assert_eq!(out.exit_code, 4);
    }

    /// REQ: DEV-810
    #[test]
    fn doctor_missing_rust_target_fails_with_its_fix_text() {
        let scratch = scratch_root();
        let probe = FixedProbe {
            rust_target_installed: false,
            ..healthy_probe()
        };
        let env = doctor_env(scratch.path(), probe);
        let out = run_words(&env, &["doctor", "--json"]);
        assert_eq!(
            doctor_field(&out, "rust-target", "status"),
            Some("fail".to_owned())
        );
        assert_eq!(
            doctor_field(&out, "rust-target", "fix"),
            Some("rustup target add wasm32-wasip2".to_owned())
        );
    }

    /// REQ: DEV-810
    #[test]
    fn doctor_rustc_older_than_1_90_fails_and_1_90_passes() {
        let scratch = scratch_root();
        let old = FixedProbe {
            rustc_version: Some("1.89.0".to_owned()),
            ..healthy_probe()
        };
        let old_out = run_words(&doctor_env(scratch.path(), old), &["doctor", "--json"]);
        assert_eq!(
            doctor_field(&old_out, "rustc", "status"),
            Some("fail".to_owned())
        );
        let exact = FixedProbe {
            rustc_version: Some("1.90.0".to_owned()),
            ..healthy_probe()
        };
        let exact_out = run_words(&doctor_env(scratch.path(), exact), &["doctor", "--json"]);
        assert_eq!(
            doctor_field(&exact_out, "rustc", "status"),
            Some("ok".to_owned())
        );
    }

    /// REQ: DEV-810
    #[test]
    fn doctor_disk_thresholds_follow_the_table() {
        let mebibyte: u64 = 1024 * 1024;
        let cases = [
            (127 * mebibyte, "fail"),
            (128 * mebibyte, "warn"),
            (511 * mebibyte, "warn"),
            (512 * mebibyte, "ok"),
        ];
        for (free_bytes, expected) in cases {
            let scratch = scratch_root();
            let probe = FixedProbe {
                free_bytes,
                ..healthy_probe()
            };
            let out = run_words(&doctor_env(scratch.path(), probe), &["doctor", "--json"]);
            assert_eq!(
                doctor_field(&out, "disk", "status"),
                Some(expected.to_owned()),
                "free bytes {free_bytes}"
            );
        }
    }

    /// REQ: DEV-810
    #[test]
    fn doctor_trust_store_that_is_absent_is_ok() {
        let scratch = scratch_root();
        let store = scratch.path().join("trust.json");
        let env = doctor_env(scratch.path(), healthy_probe())
            .with_var("ILIUM_TRUST_STORE", &path_arg(&store));
        let out = run_words(&env, &["doctor", "--json"]);
        assert_eq!(
            doctor_field(&out, "trust-store", "status"),
            Some("ok".to_owned())
        );
    }

    /// REQ: DEV-810
    #[test]
    fn doctor_warns_when_no_session_answers() {
        let scratch = scratch_root();
        let env = doctor_env(scratch.path(), healthy_probe());
        let out = run_words(&env, &["doctor", "--json"]);
        assert_eq!(
            doctor_field(&out, "session", "status"),
            Some("warn".to_owned())
        );
    }

    /// REQ: PKG-563
    #[test]
    fn check_print_schema_writes_a_json_object_and_exits_zero() {
        let scratch = scratch_root();
        let env = make_env(scratch.path());
        let out = run_words(&env, &["check", "--print-schema"]);
        assert_eq!(out.exit_code, 0);
        let text = any_stdout(&out);
        let schema: Value = serde_json::from_str(&text).expect("the schema is JSON");
        assert!(schema.is_object());
    }

    /// REQ: DEV-077
    #[test]
    fn lint_with_an_unknown_rule_id_is_an_invalid_value() {
        let scratch = scratch_root();
        let env = make_env(scratch.path());
        let project_arg = path_arg(&scratch.path().join("project"));
        let out = run_words(
            &env,
            &[
                "lint",
                project_arg.as_str(),
                "--rule",
                "lint.no-such-rule",
                "--json",
            ],
        );
        assert_eq!(out.exit_code, 2);
        assert!(has_error_code(&out, "usage.invalid-value"));
    }

    // -----------------------------------------------------------------------------------------
    // build
    // -----------------------------------------------------------------------------------------

    /// REQ: DEV-002
    #[test]
    fn build_of_a_folder_with_no_language_file_fails_with_language_unknown() {
        let scratch = scratch_root();
        let env = make_env(scratch.path());
        let target_arg = path_arg(&scratch.path().join("empty-project"));
        fs::create_dir_all(scratch.path().join("empty-project")).expect("empty project folder");
        let out = run_words(&env, &["build", target_arg.as_str()]);
        assert_eq!(out.exit_code, 4);
        assert!(has_error_code(&out, "toolchain.language-unknown"));
    }

    /// REQ: DEV-002
    #[test]
    fn build_with_both_cargo_and_package_json_fails_with_language_ambiguous() {
        let scratch = scratch_root();
        let env = make_env(scratch.path());
        let project = scratch.path().join("both-project");
        fs::create_dir_all(&project).expect("project folder");
        fs::write(project.join("Cargo.toml"), "[package]\nname = \"tools\"\n").expect("Cargo.toml");
        fs::write(project.join("package.json"), "{}").expect("package.json");
        let target_arg = path_arg(&project);
        let out = run_words(&env, &["build", target_arg.as_str()]);
        assert_eq!(out.exit_code, 4);
        assert!(has_error_code(&out, "toolchain.language-ambiguous"));
    }

    /// REQ: DEV-010
    #[test]
    fn build_with_both_debug_and_release_is_a_conflict_and_writes_nothing() {
        let scratch = scratch_root();
        let env = make_env(scratch.path());
        let project = scratch.path().join("conflict-project");
        fs::create_dir_all(&project).expect("project folder");
        let target_arg = path_arg(&project);
        let out = run_words(
            &env,
            &["build", target_arg.as_str(), "--debug", "--release"],
        );
        assert_eq!(out.exit_code, 2);
        assert!(has_error_code(&out, "usage.conflict"));
        assert!(!project.join("dist").exists());
    }

    // -----------------------------------------------------------------------------------------
    // Injected collaborators: the in-memory session
    // -----------------------------------------------------------------------------------------

    /// REQ: DEV-004
    #[test]
    fn new_memory_session_has_no_plugins_and_no_started_sessions() {
        let session = MemorySession::new();
        assert!(session.plugin("example.tools").is_none());
        assert!(session.plugin_state("example.tools").is_none());
        assert_eq!(session.sessions_started(), 0);
    }

    /// REQ: DEV-165
    #[test]
    fn memory_session_reports_running_only_for_running_state() {
        let session = MemorySession::new();
        session.add_plugin("example.running", "0.1.0", "T1", "running", false, &[]);
        session.add_plugin("example.disabled", "0.1.0", "T1", "disabled", true, &[]);
        assert_eq!(
            session.plugin("example.running"),
            Some(SessionPlugin {
                dev: false,
                running: true
            })
        );
        assert_eq!(
            session.plugin("example.disabled"),
            Some(SessionPlugin {
                dev: true,
                running: false
            })
        );
    }

    /// REQ: DEV-006
    #[test]
    fn unreachable_memory_session_holds_no_plugin_and_no_trace() {
        let session = MemorySession::unreachable();
        assert!(session.plugin("example.tools").is_none());
        assert!(session.trace_records("example.tools").is_empty());
    }
}

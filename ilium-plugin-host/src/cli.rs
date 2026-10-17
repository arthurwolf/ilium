//! Every `ilium plugin ...` subcommand as a pure function of arguments and environment
//! (reference-cli.md). The binary forwards here.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use ilium_plugin_api::{Platform, Side};
use serde_json::Value;

use crate::registry_client::Fetcher;
use crate::telemetry::{HourBucket, InspectNode, MemoryView, TraceRecord};

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
    /// Raised by Ctrl-C; a long command stops at the next step (DEV-050).
    pub interrupt: Option<Arc<AtomicBool>>,
    /// Scripted events `ilium plugin dev` consumes instead of watching files (DEV-050).
    pub dev_script: Vec<DevEvent>,
    /// How long a scripted build takes, milliseconds (DEV-053).
    pub build_duration_ms: u64,
}

impl CliEnv {
    /// An environment with the given directories, time, CI flag and fetcher; everything else
    /// defaults (no probe, builder, runner or session) (reference-cli.md).
    pub fn new(
        data_dir: PathBuf,
        state_dir: PathBuf,
        config_dir: PathBuf,
        cwd: PathBuf,
        now: SystemTime,
        is_ci: bool,
        fetcher: Arc<dyn Fetcher>,
    ) -> CliEnv {
        unimplemented!("host::cli::CliEnv::new")
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
    unimplemented!("host::cli::run")
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
    unimplemented!("host::cli::run_cli")
}

/// Runs `ilium plugin check <dir>` on an unpacked package directory (reference-requirements-api-host.md API-080).
pub fn run_check_dir(directory: &std::path::Path) -> CliOutput {
    unimplemented!("host::cli::run_check_dir")
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
    fn run_fixture(&self, side: &str, component: &[u8], fixture: &Value) -> Result<RunResult, String>;
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

/// An in-memory session for the CLI tests: a `SessionBackend` that also records what the
/// commands did to it (reference-plugin-cli.md DEV-004, DEV-100..160).
#[derive(Debug, Default)]
pub struct MemorySession {
    state: std::sync::Mutex<MemorySessionState>,
}

/// The mutable part of a `MemorySession`.
#[derive(Debug, Default)]
struct MemorySessionState {
    reachable: bool,
    journal: Vec<String>,
    installs: Vec<InstallRecord>,
}

impl MemorySession {
    /// A reachable, empty session.
    pub fn new() -> Arc<MemorySession> {
        unimplemented!("host::cli::MemorySession::new")
    }

    /// A session every connection attempt fails to reach (`session.unreachable`, DEV-004).
    pub fn unreachable() -> Arc<MemorySession> {
        unimplemented!("host::cli::MemorySession::unreachable")
    }

    /// Installs a plugin with a state (`running`, `disabled`, `stopped`, `untrusted`) and
    /// grants as `(grant, risk, state)` triples.
    pub fn add_plugin(&self, id: &str, version: &str, tier: &str, state: &str, dev: bool, grants: &[(&str, &str, &str)]) {
        unimplemented!("host::cli::MemorySession::add_plugin")
    }

    /// The same as `add_plugin`; kept for the tests that name the grants explicitly.
    pub fn add_plugin_with_grants(&self, id: &str, version: &str, tier: &str, state: &str, dev: bool, grants: &[(&str, &str, &str)]) {
        unimplemented!("host::cli::MemorySession::add_plugin_with_grants")
    }

    /// Records which key signed the installed package.
    pub fn set_signed_by(&self, id: &str, key_id: &str) {
        unimplemented!("host::cli::MemorySession::set_signed_by")
    }

    /// The state word of a plugin, when installed.
    pub fn plugin_state(&self, id: &str) -> Option<String> {
        unimplemented!("host::cli::MemorySession::plugin_state")
    }

    /// Whether an interactive client is attached (review dialogs need one).
    pub fn set_client_attached(&self, attached: bool) {
        unimplemented!("host::cli::MemorySession::set_client_attached")
    }

    /// Review dialogs shown.
    pub fn review_dialog_count(&self) -> usize {
        unimplemented!("host::cli::MemorySession::review_dialog_count")
    }

    /// Plugins started.
    pub fn started_plugin_count(&self) -> usize {
        unimplemented!("host::cli::MemorySession::started_plugin_count")
    }

    /// The grants of a plugin in state `granted`.
    pub fn granted(&self, id: &str) -> Vec<String> {
        unimplemented!("host::cli::MemorySession::granted")
    }

    /// Whether a grant is high risk.
    pub fn is_high_risk(&self, grant: &str) -> bool {
        unimplemented!("host::cli::MemorySession::is_high_risk")
    }

    /// Connection attempts made to the session.
    pub fn connect_attempts(&self) -> usize {
        unimplemented!("host::cli::MemorySession::connect_attempts")
    }

    /// Sessions a command started.
    pub fn sessions_started(&self) -> usize {
        unimplemented!("host::cli::MemorySession::sessions_started")
    }

    /// Every mutation the commands made, in order.
    pub fn journal(&self) -> Vec<String> {
        unimplemented!("host::cli::MemorySession::journal")
    }

    /// Adds a plugin log line (side, level, message).
    pub fn add_log_line(&self, id: &str, side: &str, level: &str, message: &str) {
        unimplemented!("host::cli::MemorySession::add_log_line")
    }

    /// Makes the attached client decline review dialogs.
    pub fn decline_review(&self, decline: bool) {
        unimplemented!("host::cli::MemorySession::decline_review")
    }

    /// Makes the attached client decline trust prompts.
    pub fn decline_trust(&self, decline: bool) {
        unimplemented!("host::cli::MemorySession::decline_trust")
    }

    /// The installed version of a plugin.
    pub fn plugin_version(&self, id: &str) -> Option<String> {
        unimplemented!("host::cli::MemorySession::plugin_version")
    }

    /// Activations of a plugin.
    pub fn activations(&self, id: &str) -> usize {
        unimplemented!("host::cli::MemorySession::activations")
    }

    /// Deactivations of a plugin.
    pub fn deactivations(&self, id: &str) -> usize {
        unimplemented!("host::cli::MemorySession::deactivations")
    }

    /// Restarts of a plugin.
    pub fn restarts(&self, id: &str) -> usize {
        unimplemented!("host::cli::MemorySession::restarts")
    }

    /// Whether the package files of a plugin are still installed.
    pub fn package_installed(&self, id: &str) -> bool {
        unimplemented!("host::cli::MemorySession::package_installed")
    }

    /// Adds a contribution of a plugin (kind, required grant).
    pub fn add_contribution(&self, id: &str, kind: &str, grant: &str) {
        unimplemented!("host::cli::MemorySession::add_contribution")
    }

    /// Contributions of a plugin still registered.
    pub fn contribution_count(&self, id: &str) -> usize {
        unimplemented!("host::cli::MemorySession::contribution_count")
    }

    /// Creates data, cache, storage buckets and settings values for a plugin.
    pub fn seed_plugin_data(&self, id: &str) {
        unimplemented!("host::cli::MemorySession::seed_plugin_data")
    }

    /// Whether the data folder of a plugin exists.
    pub fn data_folder_exists(&self, id: &str) -> bool {
        unimplemented!("host::cli::MemorySession::data_folder_exists")
    }

    /// Whether the cache folder of a plugin exists.
    pub fn cache_folder_exists(&self, id: &str) -> bool {
        unimplemented!("host::cli::MemorySession::cache_folder_exists")
    }

    /// Storage buckets of a plugin.
    pub fn storage_bucket_count(&self, id: &str) -> usize {
        unimplemented!("host::cli::MemorySession::storage_bucket_count")
    }

    /// Settings values of a plugin.
    pub fn settings_value_count(&self, id: &str) -> usize {
        unimplemented!("host::cli::MemorySession::settings_value_count")
    }

    /// Whether the log of a plugin was kept after uninstall.
    pub fn log_kept(&self, id: &str) -> bool {
        unimplemented!("host::cli::MemorySession::log_kept")
    }

    /// Keeps log lines (side, level, message) of an uninstalled plugin.
    pub fn keep_log(&self, id: &str, lines: &[(&str, &str, &str)]) {
        unimplemented!("host::cli::MemorySession::keep_log")
    }

    /// Sets the failure count of a plugin.
    pub fn set_failure_count(&self, id: &str, count: u32) {
        unimplemented!("host::cli::MemorySession::set_failure_count")
    }

    /// The failure count of a plugin.
    pub fn failure_count(&self, id: &str) -> u32 {
        unimplemented!("host::cli::MemorySession::failure_count")
    }

    /// The state word of one grant of a plugin.
    pub fn grant_state(&self, id: &str, grant: &str) -> Option<String> {
        unimplemented!("host::cli::MemorySession::grant_state")
    }

    /// Every install performed, in order.
    pub fn install_history(&self) -> Vec<InstallRecord> {
        unimplemented!("host::cli::MemorySession::install_history")
    }

    /// The state words a plugin went through; `with_initial` includes the installed state.
    pub fn state_history(&self, id: &str, with_initial: bool) -> Vec<String> {
        unimplemented!("host::cli::MemorySession::state_history")
    }

    /// The grants each review dialog listed, one entry per dialog.
    pub fn review_dialog_grants(&self) -> Vec<Vec<String>> {
        unimplemented!("host::cli::MemorySession::review_dialog_grants")
    }

    /// Attaches `count` interactive clients.
    pub fn attach_clients(&self, count: usize) {
        unimplemented!("host::cli::MemorySession::attach_clients")
    }

    /// Sets the bytes the next activation of a plugin restores.
    pub fn set_restore_bytes(&self, id: &str, bytes: u64) {
        unimplemented!("host::cli::MemorySession::set_restore_bytes")
    }

    /// The bytes restored by each activation of a plugin, in order.
    pub fn activation_restore_bytes(&self, id: &str) -> Vec<u64> {
        unimplemented!("host::cli::MemorySession::activation_restore_bytes")
    }

    /// The effective log level word of a plugin.
    pub fn log_level(&self, id: &str) -> String {
        unimplemented!("host::cli::MemorySession::log_level")
    }

    /// Sets the default log level word.
    pub fn set_default_log_level(&self, level: &str) {
        unimplemented!("host::cli::MemorySession::set_default_log_level")
    }
}

impl SessionBackend for MemorySession {
    fn plugin(&self, id: &str) -> Option<SessionPlugin> {
        unimplemented!("host::cli::MemorySession::plugin")
    }

    fn trace_records(&self, id: &str) -> Vec<TraceRecord> {
        unimplemented!("host::cli::MemorySession::trace_records")
    }

    fn last_tree(&self, id: &str, target: &str) -> Option<InspectNode> {
        unimplemented!("host::cli::MemorySession::last_tree")
    }

    fn profile_exports(&self, id: &str) -> Vec<String> {
        unimplemented!("host::cli::MemorySession::profile_exports")
    }

    fn profile_calls(&self, id: &str, duration_secs: u32) -> Vec<ProfileCall> {
        unimplemented!("host::cli::MemorySession::profile_calls")
    }

    fn memory_views(&self, id: &str) -> Vec<MemoryView> {
        unimplemented!("host::cli::MemorySession::memory_views")
    }

    fn counters(&self, id: &str) -> Vec<(Side, Vec<HourBucket>)> {
        unimplemented!("host::cli::MemorySession::counters")
    }
}

// ---------------------------------------------------------------------------------------------
// Build and release helpers (reference-plugin-cli.md DEV-020..045).
// ---------------------------------------------------------------------------------------------

/// The hash of every build input of a project, as `dist/build.json` records it (DEV-024).
pub fn project_input_hash(project: &Path) -> Result<String, String> {
    unimplemented!("host::cli::project_input_hash")
}

/// Whether a component carries a `name` custom section (debug builds keep it, DEV-041).
pub fn component_has_name_section(component: &[u8]) -> bool {
    unimplemented!("host::cli::component_has_name_section")
}

/// The project-relative path of the source map of one side (DEV-041).
pub fn source_map_path(side: Side) -> PathBuf {
    unimplemented!("host::cli::source_map_path")
}

/// Whether a release tag names the version of the manifest (DEV-044).
pub fn release_tag_matches(tag: &str, version: &str) -> bool {
    unimplemented!("host::cli::release_tag_matches")
}

/// The `<sha256>  <file name>` line of a sidecar checksum file (DEV-045).
pub fn sha256_sidecar_line(bytes: &[u8], file_name: &str) -> String {
    unimplemented!("host::cli::sha256_sidecar_line")
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
        window: Duration,
        last_change_ms: Option<u64>,
        owed: usize,
    }

    impl Debouncer {
        /// A debouncer with the given window.
        pub fn new(window: Duration) -> Debouncer {
            Debouncer { window, last_change_ms: None, owed: 0 }
        }

        /// Records a change at `at_millis`.
        pub fn change(&mut self, at_millis: u64) {
            unimplemented!("host::cli::dev::Debouncer::change")
        }

        /// The rebuilds due at `now_millis`.
        pub fn due(&mut self, now_millis: u64) -> usize {
            unimplemented!("host::cli::dev::Debouncer::due")
        }
    }
}

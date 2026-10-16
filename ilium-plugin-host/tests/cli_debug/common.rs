//! Shared helpers of the cli_debug tests: environment builder, fake session and tool probe,
//! record selectors.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use ilium_plugin_api::Side;
use ilium_plugin_host::cli::{
    self, CliEnv, CliOutput, ProfileCall, SessionBackend, SessionPlugin, ToolProbe,
};
use ilium_plugin_host::registry_client::{FetchError, Fetcher};
use ilium_plugin_host::telemetry::{HourBucket, InspectNode, MemoryView, TraceRecord};
use serde_json::Value;

/// The fixed "now" of the tests (2030-01-01 12:00:00 UTC).
pub fn fixed_now() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_893_499_200)
}

/// A fetcher that counts calls; telemetry and diagnostics must never reach the network.
#[derive(Default)]
pub struct CountingFetcher {
    pub calls: AtomicUsize,
}

impl Fetcher for CountingFetcher {
    fn get(&self, _url: &str) -> Result<Vec<u8>, FetchError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Err(FetchError::Unreachable)
    }
}

/// A tool probe with settable answers; every tool is fine by default.
pub struct FakeProbe {
    pub versions: HashMap<String, String>,
    pub rust_target: bool,
    pub free_bytes: u64,
}

impl Default for FakeProbe {
    fn default() -> FakeProbe {
        let mut versions = HashMap::new();
        versions.insert("rustc".to_owned(), "1.96.1".to_owned());
        versions.insert("bun".to_owned(), "1.2.5".to_owned());
        FakeProbe { versions, rust_target: true, free_bytes: 10 * 1024 * 1024 * 1024 }
    }
}

impl ToolProbe for FakeProbe {
    fn tool_version(&self, tool: &str) -> Option<String> {
        self.versions.get(tool).cloned()
    }

    fn rust_target_installed(&self, _target: &str) -> bool {
        self.rust_target
    }

    fn free_disk_bytes(&self, _dir: &Path) -> u64 {
        self.free_bytes
    }
}

/// A fake running session: one plugin with canned trace, tree, profile and memory data.
pub struct FakeBackend {
    pub plugin_id: String,
    pub dev: bool,
    pub running: bool,
    pub trace: Vec<TraceRecord>,
    pub tree: Option<InspectNode>,
    pub exports: Vec<String>,
    pub calls: Vec<ProfileCall>,
    pub memory: Vec<MemoryView>,
    pub counters: Vec<(Side, Vec<HourBucket>)>,
    pub requested_duration: Mutex<Option<u32>>,
}

impl FakeBackend {
    pub fn new(plugin_id: &str) -> FakeBackend {
        FakeBackend {
            plugin_id: plugin_id.to_owned(),
            dev: true,
            running: true,
            trace: Vec::new(),
            tree: None,
            exports: vec!["on-events".to_owned(), "render".to_owned()],
            calls: Vec::new(),
            memory: Vec::new(),
            counters: Vec::new(),
            requested_duration: Mutex::new(None),
        }
    }
}

impl SessionBackend for FakeBackend {
    fn plugin(&self, id: &str) -> Option<SessionPlugin> {
        (id == self.plugin_id).then(|| SessionPlugin { dev: self.dev, running: self.running })
    }

    fn trace_records(&self, id: &str) -> Vec<TraceRecord> {
        if id == self.plugin_id { self.trace.clone() } else { Vec::new() }
    }

    fn last_tree(&self, id: &str, _target: &str) -> Option<InspectNode> {
        if id == self.plugin_id { self.tree.clone() } else { None }
    }

    fn profile_exports(&self, _id: &str) -> Vec<String> {
        self.exports.clone()
    }

    fn profile_calls(&self, _id: &str, duration_secs: u32) -> Vec<ProfileCall> {
        *self.requested_duration.lock().expect("lock") = Some(duration_secs);
        self.calls.clone()
    }

    fn memory_views(&self, id: &str) -> Vec<MemoryView> {
        if id == self.plugin_id { self.memory.clone() } else { Vec::new() }
    }

    fn counters(&self, id: &str) -> Vec<(Side, Vec<HourBucket>)> {
        if id == self.plugin_id { self.counters.clone() } else { Vec::new() }
    }
}

/// An environment rooted in `root`, with no session and the default probe.
pub fn env(root: &Path) -> CliEnv {
    let project = root.join("project");
    std::fs::create_dir_all(&project).expect("create project dir");
    CliEnv::new(
        root.join("data"),
        root.join("state"),
        root.join("config"),
        project,
        fixed_now(),
        true,
        Arc::new(CountingFetcher::default()),
    )
    .with_probe(Arc::new(FakeProbe::default()))
}

/// The same environment attached to a fake session.
pub fn env_with_session(root: &Path, backend: FakeBackend) -> CliEnv {
    env(root).with_session(Arc::new(backend))
}

/// Runs `ilium plugin <args...>`.
pub fn run(env: &CliEnv, args: &[&str]) -> CliOutput {
    let owned: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
    cli::run(&owned, env)
}

/// The records of one `type` in output order.
pub fn records_of<'a>(output: &'a CliOutput, record_type: &str) -> Vec<&'a Value> {
    output
        .stdout_jsonl
        .iter()
        .filter(|record| record.get("type").and_then(Value::as_str) == Some(record_type))
        .collect()
}

/// The `code` of the first `error` record.
pub fn error_code(output: &CliOutput) -> Option<String> {
    records_of(output, "error")
        .first()
        .and_then(|record| record.get("code"))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

/// The `type` of every record, in order.
pub fn record_types(output: &CliOutput) -> Vec<String> {
    output
        .stdout_jsonl
        .iter()
        .map(|record| record.get("type").and_then(Value::as_str).unwrap_or("").to_owned())
        .collect()
}

/// A scratch directory the test owns.
pub fn scratch() -> tempfile::TempDir {
    tempfile::tempdir().expect("create scratch dir")
}

/// Path of the log file of a plugin for the default session of the environment's project.
pub fn log_file(env: &CliEnv, plugin: &str, suffix: &str) -> PathBuf {
    ilium_plugin_host::telemetry::plugin_log_dir(&env.state_dir, &env.cwd, "default")
        .join(format!("{plugin}.log{suffix}"))
}

/// Writes lines to a log file, creating the folder.
pub fn write_log(env: &CliEnv, plugin: &str, suffix: &str, lines: &[String]) {
    let path = log_file(env, plugin, suffix);
    std::fs::create_dir_all(path.parent().expect("log dir")).expect("create log dir");
    let mut text = lines.join("\n");
    text.push('\n');
    std::fs::write(path, text).expect("write log file");
}

/// One formatted log line (reference-debugging-tools.md line format).
pub fn log_line(time: &str, level: &str, side: &str, plugin: &str, message: &str) -> String {
    format!("{time} {level:<5} {side:<6} {plugin} {message}")
}

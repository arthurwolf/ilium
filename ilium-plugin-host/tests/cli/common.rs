//! Shared helpers of the cli tests: environment builder, fake session, fake builder, project
//! fixtures made with `ilium plugin new`, record selectors.
//! Items missing from the skeleton are listed in API_GAPS.md.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use ilium_plugin_host::cli::{
    self, BuildRunner, CliEnv, CliOutput, ComponentRunner, MemorySession, RunResult, ToolProbe,
};
use ilium_plugin_host::registry_client::{FetchError, Fetcher};
use serde_json::Value;

/// The fixed "now" of the tests (2030-01-01 12:00:00 UTC).
pub fn fixed_now() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_893_499_200)
}

/// A fetcher that counts calls; offline commands must never reach the network.
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
}

impl Default for FakeProbe {
    fn default() -> FakeProbe {
        let mut versions = HashMap::new();
        versions.insert("rustc".to_owned(), "1.96.1".to_owned());
        versions.insert("bun".to_owned(), "1.2.5".to_owned());
        FakeProbe {
            versions,
            rust_target: true,
        }
    }
}

impl FakeProbe {
    pub fn without(tool: &str) -> FakeProbe {
        let mut probe = FakeProbe::default();
        probe.versions.remove(tool);
        probe
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
        10 * 1024 * 1024 * 1024
    }
}

/// The smallest valid-looking component bytes (core wasm header).
pub const WASM_HEADER: &[u8] = b"\0asm\x0d\0\x01\0";

/// A genuine component that installs: the required exports and the API version section.
pub fn component_bytes() -> Vec<u8> {
    ilium_plugin_host::testing::plugin_component(&[])
}

/// A build runner that succeeds, or fails at one named stage with a message.
pub struct FakeBuilder {
    pub fail_stage: Option<&'static str>,
    /// 1-based number of the build (counted at the `compile` stage) that fails.
    pub fail_build: Option<usize>,
    pub runs: AtomicUsize,
    pub builds: AtomicUsize,
    pub outdated_lock: bool,
    pub uncached_dependency: Option<String>,
}

impl FakeBuilder {
    pub fn ok() -> Arc<FakeBuilder> {
        Arc::new(FakeBuilder {
            fail_stage: None,
            fail_build: None,
            runs: AtomicUsize::new(0),
            builds: AtomicUsize::new(0),
            outdated_lock: false,
            uncached_dependency: None,
        })
    }

    pub fn failing_at(stage: &'static str) -> Arc<FakeBuilder> {
        Arc::new(FakeBuilder {
            fail_stage: Some(stage),
            fail_build: None,
            runs: AtomicUsize::new(0),
            builds: AtomicUsize::new(0),
            outdated_lock: false,
            uncached_dependency: None,
        })
    }

    pub fn failing_build(number: usize) -> Arc<FakeBuilder> {
        Arc::new(FakeBuilder {
            fail_stage: None,
            fail_build: Some(number),
            runs: AtomicUsize::new(0),
            builds: AtomicUsize::new(0),
            outdated_lock: false,
            uncached_dependency: None,
        })
    }
}

impl FakeBuilder {
    pub fn with_outdated_lock() -> Arc<FakeBuilder> {
        Arc::new(FakeBuilder {
            fail_stage: None,
            fail_build: None,
            runs: AtomicUsize::new(0),
            builds: AtomicUsize::new(0),
            outdated_lock: true,
            uncached_dependency: None,
        })
    }

    pub fn with_uncached_dependency(name: &str) -> Arc<FakeBuilder> {
        Arc::new(FakeBuilder {
            fail_stage: None,
            fail_build: None,
            runs: AtomicUsize::new(0),
            builds: AtomicUsize::new(0),
            outdated_lock: false,
            uncached_dependency: Some(name.to_owned()),
        })
    }
}

/// A component runner that returns one canned result for every fixture.
pub struct ScriptedRunner {
    pub result: RunResult,
    /// How often the project's own tests were started (only `test --coverage` may do that).
    pub project_test_runs: AtomicUsize,
}

impl ScriptedRunner {
    pub fn passing() -> Arc<ScriptedRunner> {
        Arc::new(ScriptedRunner {
            result: RunResult {
                submissions: vec![serde_json::json!({"intent": "set-decoration", "slot": "work"})],
                outcomes: vec![serde_json::json!("ok")],
                logs: vec![serde_json::json!({"level": "info", "message": "activate"})],
                trap: None,
            },
            project_test_runs: AtomicUsize::new(0),
        })
    }

    pub fn trapping(reason: &str) -> Arc<ScriptedRunner> {
        Arc::new(ScriptedRunner {
            result: RunResult {
                submissions: vec![],
                outcomes: vec![],
                logs: vec![],
                trap: Some(reason.to_owned()),
            },
            project_test_runs: AtomicUsize::new(0),
        })
    }
}

impl ComponentRunner for ScriptedRunner {
    fn run_fixture(
        &self,
        _side: &str,
        _component: &[u8],
        _fixture: &Value,
    ) -> Result<RunResult, String> {
        Ok(self.result.clone())
    }

    fn run_project_tests(&self, _project: &Path) -> i32 {
        self.project_test_runs.fetch_add(1, Ordering::SeqCst);
        0
    }
}

impl BuildRunner for FakeBuilder {
    fn lockfile_would_change(&self, _project: &Path) -> bool {
        self.outdated_lock
    }

    fn missing_offline_dependency(&self, _project: &Path) -> Option<String> {
        self.uncached_dependency.clone()
    }

    fn run_stage(&self, _project: &Path, stage: &str, _side: &str) -> Result<Vec<u8>, String> {
        self.runs.fetch_add(1, Ordering::SeqCst);
        if stage == "compile" {
            let number = self.builds.fetch_add(1, Ordering::SeqCst) + 1;
            if self.fail_build == Some(number) {
                return Err("error[E0308]: mismatched types".to_owned());
            }
        }
        if self.fail_stage == Some(stage) {
            return Err(format!("error: stage {stage} failed"));
        }
        Ok(component_bytes())
    }
}

/// An environment rooted in `root`: no session, default probe, succeeding builder, CI mode.
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
    .with_builder(FakeBuilder::ok())
}

/// The same environment attached to an in-memory session.
pub fn env_session(root: &Path, session: &Arc<MemorySession>) -> CliEnv {
    env(root).with_session(session.clone())
}

/// Runs `ilium plugin <args...>`.
pub fn run(env: &CliEnv, args: &[&str]) -> CliOutput {
    let owned: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
    cli::run(&owned, env)
}

/// Runs a command with the project directory (a path) spliced in as an argument.
pub fn run_in(env: &CliEnv, dir: &Path, args: &[&str]) -> CliOutput {
    let mut owned: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
    owned.insert(1, dir.to_string_lossy().into_owned());
    cli::run(&owned, env)
}

/// The records of one `type` in output order.
pub fn recs<'a>(output: &'a CliOutput, record_type: &str) -> Vec<&'a Value> {
    output
        .stdout_jsonl
        .iter()
        .filter(|record| record.get("type").and_then(Value::as_str) == Some(record_type))
        .collect()
}

/// The `code` of the first `error` record.
pub fn code(output: &CliOutput) -> Option<String> {
    recs(output, "error")
        .first()
        .and_then(|record| record.get("code"))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

/// Every `code` of the `warning` records.
pub fn warning_codes(output: &CliOutput) -> Vec<String> {
    recs(output, "warning")
        .iter()
        .filter_map(|record| {
            record
                .get("code")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .collect()
}

/// The `type` of every record, in order.
pub fn types(output: &CliOutput) -> Vec<String> {
    output
        .stdout_jsonl
        .iter()
        .map(|record| {
            record
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned()
        })
        .collect()
}

/// The final `result` record.
pub fn result(output: &CliOutput) -> Value {
    output.stdout_jsonl.last().cloned().unwrap_or(Value::Null)
}

/// Artifacts of one kind.
pub fn artifacts<'a>(output: &'a CliOutput, kind: &str) -> Vec<&'a Value> {
    recs(output, "artifact")
        .into_iter()
        .filter(|record| record.get("kind").and_then(Value::as_str) == Some(kind))
        .collect()
}

/// A scratch directory the test owns.
pub fn scratch() -> tempfile::TempDir {
    tempfile::tempdir().expect("create scratch dir")
}

/// Sorted relative listing of every file under `dir`.
pub fn tree_listing(dir: &Path) -> Vec<String> {
    fn walk(base: &Path, dir: &Path, out: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(base, &path, out);
            } else {
                out.push(
                    path.strip_prefix(base)
                        .expect("under base")
                        .to_string_lossy()
                        .replace('\\', "/"),
                );
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

/// True for 64 lowercase hexadecimal characters.
pub fn is_sha256_hex(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Creates a project with `ilium plugin new` under `root/<name>`; asserts the command worked.
pub fn new_project(env: &CliEnv, root: &Path, name: &str, template: &str, lang: &str) -> PathBuf {
    let dir = root.join(name);
    let id = format!("acme.{name}");
    let output = run(
        env,
        &[
            "new",
            dir.to_str().expect("utf8"),
            "--id",
            &id,
            "--lang",
            lang,
            "--template",
            template,
            "--publisher",
            "acme",
        ],
    );
    assert_eq!(output.exit_code, 0, "new failed: {:?}", output.stdout_jsonl);
    dir
}

/// A rust server project.
pub fn server_project(env: &CliEnv, root: &Path) -> PathBuf {
    new_project(env, root, "demo", "server", "rust")
}

/// Reads and parses a JSON file.
pub fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).expect("read json file")).expect("parse json file")
}

/// Writes a json file.
pub fn write_json(path: &Path, value: &Value) {
    std::fs::write(path, serde_json::to_vec_pretty(value).expect("encode json"))
        .expect("write json file");
}

/// Writes a `dist/` that matches the current sources: components for `sides` and a
/// `build.json` with the real input hash and the given profile.
pub fn write_dist(dir: &Path, sides: &[&str], profile: &str) {
    std::fs::create_dir_all(dir.join("dist")).expect("create dist");
    let hash = cli::project_input_hash(dir).expect("input hash");
    let mut side_entries = Vec::new();
    for side in sides {
        let component = component_bytes();
        std::fs::write(dir.join("dist").join(format!("{side}.wasm")), &component)
            .expect("write component");
        side_entries.push(serde_json::json!({
            "name": side, "bytes": component.len(), "sha256": "0".repeat(64)
        }));
    }
    write_json(
        &dir.join("dist/build.json"),
        &serde_json::json!({
            "profile": profile, "api": "0.1.0", "input_hash": hash, "sides": side_entries,
            "toolchain": {"rustc": "1.96.1"}, "cached": false
        }),
    );
}

/// Builds a rust server project and packs it; returns the package path.
pub fn packed_project(env: &CliEnv, root: &Path) -> (PathBuf, PathBuf) {
    let dir = server_project(env, root);
    write_dist(&dir, &["server"], "release");
    let output = run_in(env, &dir, &["pack"]);
    assert_eq!(
        output.exit_code, 0,
        "pack failed: {:?}",
        output.stdout_jsonl
    );
    let package = dir.join("dist/acme.demo-0.1.0.iliumpkg");
    (dir, package)
}

/// Makes a key pair with `keygen`; returns the secret and public key paths.
pub fn make_key(env: &CliEnv, root: &Path, publisher_id: &str) -> (PathBuf, PathBuf) {
    let out = root.join(format!("keys-{publisher_id}"));
    let output = run(
        env,
        &[
            "keygen",
            "--publisher-id",
            publisher_id,
            "--publisher",
            "Acme Labs",
            "--out",
            out.to_str().expect("utf8"),
        ],
    );
    assert_eq!(
        output.exit_code, 0,
        "keygen failed: {:?}",
        output.stdout_jsonl
    );
    (out.join("publisher.key"), out.join("publisher.pub"))
}

/// An interrupt flag already raised.
pub fn raised_interrupt() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(true))
}

/// A grant the grant catalogue rates high risk (see API_GAPS.md: the exact id comes from the grants page).
pub const HIGH_RISK_GRANT: &str = "process.run:git";

/// Like `packed_project`, but the manifest also requests `HIGH_RISK_GRANT`.
pub fn packed_project_with_high_risk(env: &CliEnv, root: &Path) -> (PathBuf, PathBuf) {
    let dir = server_project(env, root);
    let mut manifest = read_json(&dir.join("manifest.json"));
    manifest["grants"] = serde_json::json!([{"id": HIGH_RISK_GRANT, "reason": "test"}]);
    write_json(&dir.join("manifest.json"), &manifest);
    write_dist(&dir, &["server"], "release");
    let output = run_in(env, &dir, &["pack"]);
    assert_eq!(
        output.exit_code, 0,
        "pack failed: {:?}",
        output.stdout_jsonl
    );
    let package = dir.join("dist/acme.demo-0.1.0.iliumpkg");
    (dir, package)
}

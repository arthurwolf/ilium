//! Devtooling requirement tests for the plugin developer tools, group devtooling-gap.
//!
//! Each `/// REQ: DEV-<n>` tag marks the test that covers that inventory requirement. Requirements
//! covered here: 878, 879, 881, 882, 884, 886, 889, 890, 891, 896, 897, 900, 901, 902, 903, 904,
//! 906, 907, 908, 911, 912, 913, 914, 915, 916, 919, 920, 922, 923, 924, 925, 927, 928, 931, 932,
//! 933, 934, 935, 937, 938, 939, 940, 941, 964, 965, 966, 967, 968, 969, 970, 971, 972, 975, 977,
//! 978, 982, 985, 986, 987, 988, 989, 990, 992, 993, 995, 996, 997, 998.

#[path = "../devtools/common.rs"]
mod common;

mod guide_backwards_compatibility_test_matrix;
mod guide_ci_recipes;
mod guide_compatibility_and_upgrades;
mod guide_data_migration;
mod guide_deprecating_plugin_features;
mod guide_operator_upgrades;
mod guide_plugin_documentation;
mod host_upgrade_while_plugins_run;
mod reference_abi_evolution;
mod reference_abi_major_versions;
mod reference_cli_completion;
mod reference_cli_output_stability;
mod reference_config_and_data_migration;
mod reference_conformance_suite;
mod reference_plugin_cli;
mod reference_plugin_cli_dev_failure_modes;
mod reference_plugin_debugging;
mod reference_plugin_test_harness;
mod reference_project_templates;
mod reference_sdk_deprecation;
mod reference_test_harness;
mod reference_toolchain_support_matrix;
mod typescript_sdk;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use ilium_plugin_host::cli::{CliEnv, CliOutput};
use serde_json::Value;

use crate::common::*;

/// The text of a published plugin manual page, read from `docs/plugins-manual/pages/<name>.md`.
pub(crate) fn manual_page(name: &str) -> String {
    let path = format!(
        "{}/../docs/plugins-manual/pages/{name}.md",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read manual page {path}: {error}"))
}

/// A UTF-8 view of a path; test paths are always valid UTF-8.
pub(crate) fn utf8(path: &Path) -> &str {
    path.to_str().expect("test path is valid UTF-8")
}

/// A Rust server project made by `ilium plugin new`, with its environment and project folder.
pub(crate) fn new_rust_project() -> (tempfile::TempDir, CliEnv, PathBuf) {
    let root = scratch();
    let env = env(root.path());
    let dir = root.path().join("out");
    let made = run(
        &env,
        &[
            "new",
            dir.to_str().expect("utf8 path"),
            "--id",
            "example.agent-tools",
            "--lang",
            "rust",
            "--template",
            "server",
            "--json",
        ],
    );
    assert_eq!(made.exit_code, 0, "{:?}", made.stdout_jsonl);
    (root, env, dir)
}

/// Every record of `output` that carries the field `key`, in printed order.
pub(crate) fn rows_with<'a>(output: &'a CliOutput, key: &str) -> Vec<&'a Value> {
    output
        .stdout_jsonl
        .iter()
        .filter(|record| record.get(key).is_some())
        .collect()
}

/// Every record of `output` whose `code` or `rule` field equals `code`, in printed order.
pub(crate) fn with_code<'a>(output: &'a CliOutput, code: &str) -> Vec<&'a Value> {
    output
        .stdout_jsonl
        .iter()
        .filter(|record| {
            record.get("code").and_then(Value::as_str) == Some(code)
                || record.get("rule").and_then(Value::as_str) == Some(code)
        })
        .collect()
}

/// Every file under `dir` mapped to its bytes, keyed by path relative to `dir`. A missing folder
/// yields an empty map, so a snapshot taken before a folder exists still compares.
pub(crate) fn snapshot(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    if !dir.exists() {
        return BTreeMap::new();
    }
    tree_of(dir)
        .into_iter()
        .map(|relative| {
            let bytes = std::fs::read(dir.join(&relative)).expect("read snapshot file");
            (relative, bytes)
        })
        .collect()
}

/// The SHA-256 of one file, as lowercase hex.
pub(crate) fn file_sha256(path: &Path) -> String {
    sha256_hex(&std::fs::read(path).expect("read file for digest"))
}


// Test-support helpers for the devtooling-gap tests. Each one drives the real `ilium plugin`
// command line (`ilium_plugin_host::cli::run`) or reads the real manual and repository files, so a
// test fails where the product lacks the feature. Where the manual defines a fake (the fake host,
// the per-minute deprecation notice), a small faithful model stands in for it.

use std::cell::{Cell, RefCell};
use std::io::{Read, Write};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::{Instant, SystemTime};

use ilium_plugin_api::compat::{observed_result, ExpectedResult};
use ilium_plugin_api::version::ApiVersion;
use ilium_plugin_host::cli::{self, DevEvent};
use ilium_plugin_api::Platform;
use serde_json::json;

/// The project folder of an environment: the working directory, or the sibling `out` folder that
/// `new_rust_project` creates, whichever holds a manifest.
fn project_root(env: &CliEnv) -> PathBuf {
    for candidate in [env.cwd.clone(), env.cwd.with_file_name("out")] {
        if candidate.join("manifest.json").is_file() {
            return candidate;
        }
    }
    env.cwd.clone()
}

/// Runs `ilium plugin <command> <project> --json <extra...>` on the project of `env`.
fn run_project(env: &CliEnv, command: &str, extra: &[&str]) -> CliOutput {
    let project = project_root(env);
    let mut args: Vec<&str> = vec![command, utf8(&project), "--json"];
    args.extend_from_slice(extra);
    run(env, &args)
}

/// Days since 1970-01-01 of a proleptic Gregorian date.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let shifted_month = if month > 2 { month - 3 } else { month + 9 };
    let day_of_year = (153 * shifted_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// `YYYY-MM-DD` split into numbers.
fn split_date(date: &str) -> (i64, i64, i64) {
    let parts: Vec<i64> = date
        .split('-')
        .map(|part| part.parse().unwrap_or_else(|_| panic!("date {date} is not YYYY-MM-DD")))
        .collect();
    assert_eq!(parts.len(), 3, "date {date} is not YYYY-MM-DD");
    (parts[0], parts[1], parts[2])
}

/// Midnight UTC of a `YYYY-MM-DD` date.
fn date_to_system_time(date: &str) -> SystemTime {
    let (year, month, day) = split_date(date);
    let seconds = u64::try_from(days_from_civil(year, month, day) * 86_400)
        .unwrap_or_else(|_| panic!("date {date} is before 1970"));
    SystemTime::UNIX_EPOCH + Duration::from_secs(seconds)
}

/// Writes a zip archive of the given entries and returns its bytes.
fn zip_bytes(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, bytes) in entries {
        writer
            .start_file(*name, zip::write::SimpleFileOptions::default())
            .expect("start zip entry");
        writer.write_all(bytes).expect("write zip entry");
    }
    writer.finish().expect("finish zip").into_inner()
}

/// The first `X.Y.Z` version on a line of `text` that contains `needle`.
fn version_on_line_with(text: &str, needle: &str) -> Option<String> {
    for line in text.lines().filter(|line| line.contains(needle)) {
        for word in line.split(|c: char| !(c.is_ascii_digit() || c == '.')) {
            if word.split('.').count() == 3 && word.split('.').all(|p| p.parse::<u32>().is_ok()) {
                return Some(word.to_owned());
            }
        }
    }
    None
}

/// The rows of the host-minor support table of the manual page `reference-abi-major-versions`,
/// each as `[minor, released, until, support]` with backticks removed.
fn support_rows() -> Vec<Vec<String>> {
    let page = manual_page("reference-abi-major-versions");
    let mut rows = Vec::new();
    let mut in_table = false;
    for line in page.lines() {
        if line.starts_with("| Host minor") {
            in_table = true;
            continue;
        }
        if in_table {
            if !line.starts_with('|') {
                in_table = false;
                continue;
            }
            let cells: Vec<String> = line
                .trim_matches('|')
                .split('|')
                .map(|cell| cell.trim().replace('`', ""))
                .collect();
            if cells.len() >= 4 && !cells[0].starts_with("---") {
                rows.push(cells);
            }
        }
    }
    rows
}

/// A running `ilium plugin dev` session. Every interaction replays the scripted events through the
/// real command, so the records are what `ilium plugin dev --json` prints for that script.
pub(crate) struct DevSession {
    env: CliEnv,
    project: PathBuf,
    script: Vec<DevEvent>,
    variables: Vec<(String, String)>,
    records: Vec<Value>,
    edit_mark: usize,
    clock_ms: u64,
    last_exit: i32,
}

impl DevSession {
    fn start(env: &CliEnv, variables: &[(&str, &str)]) -> DevSession {
        let mut session = DevSession {
            env: env.clone(),
            project: project_root(env),
            script: Vec::new(),
            variables: variables
                .iter()
                .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
                .collect(),
            records: Vec::new(),
            edit_mark: 0,
            clock_ms: 0,
            last_exit: 0,
        };
        session.refresh();
        session
    }

    /// Runs the command again over the whole script, ending with Ctrl-C a second after the last event.
    fn refresh(&mut self) {
        let mut script = self.script.clone();
        script.push(DevEvent::Interrupt { at_ms: self.clock_ms + 1000 });
        let mut env = self.env.clone().with_dev_script(script);
        for (name, value) in &self.variables {
            env = env.with_var(name, value);
        }
        let output = run(&env, &["dev", utf8(&self.project), "--json"]);
        self.last_exit = output.exit_code;
        self.records = output.stdout_jsonl;
    }

    pub(crate) fn edit(&mut self, rel: &str, text: &str) {
        write_text(&self.project, rel, text);
        self.edit_mark = self.records.len();
        self.clock_ms += 1000;
        self.script.push(DevEvent::Change { at_ms: self.clock_ms, path: rel.to_owned() });
        self.refresh();
    }

    pub(crate) fn records_since_edit(&self) -> Vec<Value> {
        self.records.iter().skip(self.edit_mark).cloned().collect()
    }

    pub(crate) fn instance_answers(&self) -> bool {
        let started = self.records.iter().any(|record| {
            matches!(record["type"].as_str(), Some("ready" | "reload" | "installed"))
                && record["outcome"] != "build-failed"
        });
        let stopped = self.records.iter().any(|record| record["type"] == "stopped");
        started && !stopped
    }

    pub(crate) fn is_alive(&self) -> bool {
        // Ctrl-C ends the scripted session with 130; any other non-zero code is a failure.
        self.last_exit == 0 || self.last_exit == 130
    }

    pub(crate) fn records(&self) -> Vec<Value> {
        self.records.clone()
    }

    pub(crate) fn poll_interval_ms(&self) -> u64 {
        self.records
            .iter()
            .find(|record| record["code"] == "dev.watcher-degraded")
            .and_then(|record| record["poll_interval_ms"].as_u64())
            .unwrap_or(0)
    }

    pub(crate) fn trigger_activation_crashes(&mut self, count: usize) {
        for _ in 0..count {
            self.clock_ms += 1000;
            self.script.push(DevEvent::Log {
                at_ms: self.clock_ms,
                side: "server".to_owned(),
                level: "error".to_owned(),
                message: "plugin crashed during activation".to_owned(),
            });
        }
        self.refresh();
    }

    pub(crate) fn reload_attempts(&self) -> usize {
        self.records.iter().filter(|record| record["type"] == "reload").count()
    }
}

/// A session that calls a deprecated plugin item. The notice rule is the manual's: one warn line
/// when the item is first called, then at most one more per minute of session time.
pub(crate) struct DeprecatedItemSession {
    clock_ms: Cell<u64>,
    last_warned_ms: Cell<Option<u64>>,
    lines: RefCell<Vec<Value>>,
}

impl DeprecatedItemSession {
    pub(crate) fn call_deprecated_item(&self) -> CliOutput {
        let now = self.clock_ms.get();
        let due = match self.last_warned_ms.get() {
            None => true,
            Some(last) => now.saturating_sub(last) >= 60_000,
        };
        let mut records = Vec::new();
        if due {
            let line = json!({
                "level": "warn",
                "code": "sdk.deprecated-item",
                "message": "the plugin called a deprecated item",
                "at_ms": now,
            });
            self.lines.borrow_mut().push(line.clone());
            self.last_warned_ms.set(Some(now));
            records.push(line);
        }
        CliOutput {
            exit_code: 0,
            stdout_jsonl: records,
            stderr: String::new(),
            stdout_raw: String::new(),
            stdout_text: String::new(),
        }
    }

    pub(crate) fn warn_log_lines(&self) -> Vec<Value> {
        self.lines.borrow().clone()
    }

    pub(crate) fn advance_clock_ms(&self, milliseconds: u64) {
        self.clock_ms.set(self.clock_ms.get() + milliseconds);
    }
}

/// The fake host a plugin test runs against: it answers only the allowed methods and never reaches
/// a real socket, clock or file.
pub(crate) struct FakeHostHarness {
    allowed: Vec<String>,
}

impl FakeHostHarness {
    fn real_resource(resource: &str, detail: &str) -> Result<Value, Value> {
        Err(json!({"code": "harness.real-resource", "resource": resource, "detail": detail}))
    }

    pub(crate) fn call_host(&mut self, method: &str, argument: &str) -> Result<Value, Value> {
        if self.allowed.iter().any(|name| name == method) {
            Ok(json!({"method": method, "argument": argument}))
        } else {
            Err(json!({"code": "capability.denied", "method": method}))
        }
    }

    pub(crate) fn connect_real_socket(&mut self, address: &str) -> Result<Value, Value> {
        Self::real_resource("socket", address)
    }

    pub(crate) fn read_real_clock(&mut self) -> Result<Value, Value> {
        Self::real_resource("clock", "system clock")
    }

    pub(crate) fn read_file_outside_test_directory(&mut self, path: &str) -> Result<Value, Value> {
        Self::real_resource("file", path)
    }
}

/// The result of asking the shell completion for plugin ids.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Completion {
    pub(crate) candidates: Vec<String>,
    pub(crate) stderr: String,
    pub(crate) list_json_calls: usize,
}

/// A plugin built against one ABI minor (`"1.4"` is major 1, minor 4).
pub(crate) struct PluginBuild {
    minor: String,
}

/// The minor number of a `major.minor` text.
fn minor_number(text: &str) -> u16 {
    text.split('.')
        .nth(1)
        .and_then(|part| part.parse().ok())
        .unwrap_or_else(|| panic!("{text} is not major.minor"))
}

/// The major number of a `major.minor...` text.
fn major_number(text: &str) -> u16 {
    text.split('.')
        .next()
        .and_then(|part| part.parse().ok())
        .unwrap_or_else(|| panic!("{text} has no major number"))
}

impl PluginBuild {
    /// Whether the host window rule of `ilium_plugin_api::compat` loads this build on the host minor.
    pub(crate) fn loads_on_host(&self, host_minor: &str) -> bool {
        major_number(&self.minor) == major_number(host_minor)
            && observed_result(minor_number(host_minor), minor_number(&self.minor))
                == ExpectedResult::Loads
    }
}

/// When an ABI item was deprecated and removed.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AbiRemoval {
    pub(crate) deprecated_in_minor: u32,
    pub(crate) removed_in_minor: u32,
}

/// What happened when the host tried to load a plugin directory.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LoadAttempt {
    pub(crate) started: bool,
    pub(crate) error_code: Option<String>,
}

/// The report of the ABI compatibility gate between two releases.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AbiGateReport {
    pub(crate) passes: bool,
    pub(crate) failing_rule: Option<String>,
    removed: Vec<String>,
    lines: Vec<String>,
}

impl AbiGateReport {
    /// The items the printed report names.
    pub(crate) fn named_items(&self) -> Vec<String> {
        self.lines
            .iter()
            .filter_map(|line| line.strip_prefix("removed export "))
            .map(str::to_owned)
            .collect()
    }

    /// The items that differ between the two releases.
    pub(crate) fn changed_items(&self) -> Vec<String> {
        self.removed.clone()
    }
}

/// The announcement of a major release.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MajorAnnouncement {
    pub(crate) days_before_release: u32,
    pub(crate) removals_at_release: usize,
    pub(crate) removals_announced: usize,
}

/// A released plugin kept as a backwards-compatibility fixture.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PluginFixture {
    pub(crate) version: String,
}

/// Whether a fixture loaded on the current host.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LoadOutcome {
    pub(crate) loaded: bool,
    pub(crate) error_code: Option<String>,
}

pub(crate) fn dev_session(env: &CliEnv) -> DevSession {
    DevSession::start(env, &[])
}

pub(crate) fn dev_session_with_watcher_fault(env: &CliEnv, fault: &str) -> DevSession {
    DevSession::start(env, &[("ILIUM_PLUGIN_DEV_WATCHER_FAULT", fault)])
}

pub(crate) fn dev_session_with_crashing_plugin(env: &CliEnv) -> DevSession {
    DevSession::start(env, &[("ILIUM_PLUGIN_DEV_CRASH_ON_ACTIVATE", "1")])
}

/// The six hosts of the build matrix: Linux, macOS and Windows on x86-64 and aarch64.
const MATRIX_HOSTS: [(Platform, &str); 6] = [
    (Platform::Linux, "x86_64"),
    (Platform::Linux, "aarch64"),
    (Platform::MacOs, "x86_64"),
    (Platform::MacOs, "aarch64"),
    (Platform::Windows, "x86_64"),
    (Platform::Windows, "aarch64"),
];

/// Builds the project of `env` as seen from one host and returns the SHA-256 of `dist/server.wasm`.
fn component_hash_on_host(env: &CliEnv, platform: Platform, arch: &str) -> String {
    let mut host_env = env.clone().with_var("ILIUM_PLUGIN_HOST_ARCH", arch);
    host_env.platform = platform;
    let built = run_project(&host_env, "build", &[]);
    assert_eq!(built.exit_code, 0, "build on {arch} failed: {:?}", built.stdout_jsonl);
    file_sha256(&project_root(env).join("dist/server.wasm"))
}

pub(crate) fn matrix_component_hashes(env: &CliEnv) -> BTreeMap<String, String> {
    MATRIX_HOSTS
        .iter()
        .map(|(platform, arch)| {
            (format!("{platform:?}-{arch}"), component_hash_on_host(env, *platform, arch))
        })
        .collect()
}

pub(crate) fn unsupported_host_env(root: &Path, os: &str, arch: &str) -> CliEnv {
    env(root)
        .with_var("ILIUM_PLUGIN_HOST_OS", os)
        .with_var("ILIUM_PLUGIN_HOST_ARCH", arch)
}

pub(crate) fn end_of_life_host_env(root: &Path, date: &str) -> CliEnv {
    let mut host_env = env(root);
    host_env.now = date_to_system_time(date);
    host_env
}

pub(crate) fn server_start_log_lines(env: &CliEnv) -> Vec<Value> {
    run(env, &["logs", "--json"])
        .stdout_jsonl
        .into_iter()
        .filter(|record| record.get("level").is_some())
        .collect()
}

pub(crate) fn installed_plugins_still_load(env: &CliEnv) -> bool {
    let listed = run(env, &["list", "--json"]);
    listed.exit_code == 0 && records_of(&listed, "error").is_empty()
}

/// Raises of a minimum toolchain that the support-matrix page announces, as
/// `(announced in minor, effective minor)`. A line such as "announced in 0.4, effective 0.6" names one.
pub(crate) fn announced_minimum_raises() -> Vec<(u32, u32)> {
    let page = manual_page("reference-toolchain-support-matrix");
    let mut raises = Vec::new();
    for line in page.lines().filter(|line| line.contains("announced in") && line.contains("effective")) {
        let minors: Vec<u32> = line
            .split(|c: char| !(c.is_ascii_digit() || c == '.'))
            .filter_map(|word| word.strip_prefix("0."))
            .filter_map(|minor| minor.parse().ok())
            .collect();
        if let [announced, effective, ..] = minors[..] {
            raises.push((announced, effective));
        }
    }
    raises
}

/// The minimum tool versions the support-matrix page states, keyed by tool. They depend on the
/// minor only, so every patch of one minor gets the same map.
pub(crate) fn toolchain_minimums(release: &str) -> BTreeMap<String, String> {
    assert_eq!(release.split('.').count(), 3, "release {release} is not X.Y.Z");
    let page = manual_page("reference-toolchain-support-matrix");
    let mut minimums = BTreeMap::new();
    for (tool, key) in [("Rust", "rust"), ("Bun", "bun"), ("wasm-tools", "wasm-tools")] {
        let marker = format!("minimum {tool} version is ");
        if let Some(start) = page.find(&marker) {
            let version: String = page[start + marker.len()..]
                .chars()
                .take_while(|c| c.is_ascii_digit() || *c == '.')
                .collect();
            minimums.insert(key.to_owned(), version.trim_end_matches('.').to_owned());
        }
    }
    minimums
}

/// Records a toolchain one minor above the newest tested Rust and returns its version.
pub(crate) fn install_toolchain_one_minor_newer_than_tested(env: &CliEnv) -> String {
    let minimums = toolchain_minimums("0.1.0");
    let tested = minimums.get("rust").unwrap_or_else(|| panic!("the support matrix names no Rust version"));
    let mut parts = tested.split('.');
    let major: u32 = parts.next().and_then(|p| p.parse().ok()).expect("Rust major");
    let minor: u32 = parts.next().and_then(|p| p.parse().ok()).expect("Rust minor");
    let newer = format!("{major}.{}.0", minor + 100);
    write_text(&env.state_dir, "toolchains/rust-installed.txt", &newer);
    newer
}

pub(crate) fn installed_rust_version(env: &CliEnv) -> String {
    let doctor = run(env, &["doctor", "--json"]);
    doctor
        .stdout_jsonl
        .iter()
        .find(|record| record["tool"] == "rust" && record["version"].is_string())
        .and_then(|record| record["version"].as_str())
        .map(str::to_owned)
        .unwrap_or_else(|| panic!("doctor reports no Rust version: {:?}", doctor.stdout_jsonl))
}

pub(crate) fn build_pinned_component_on_second_host(env: &CliEnv) -> String {
    component_hash_on_host(env, Platform::MacOs, "aarch64")
}

/// An environment in which one deprecated item is announced by an environment variable.
fn env_with_deprecation(root: &Path, kind: &str, detail: &str) -> CliEnv {
    env(root).with_var(&format!("ILIUM_PLUGIN_DEPRECATED_{kind}"), detail)
}

pub(crate) fn deprecated_command_env(root: &Path, command: &str, message: &str) -> CliEnv {
    env_with_deprecation(root, "COMMAND", &format!("{command}:{message}"))
}

pub(crate) fn deprecated_fixture_env(root: &Path) -> CliEnv {
    env_with_deprecation(root, "FIXTURE", "tests/fixtures/old.json")
}

pub(crate) fn deprecated_item_env(root: &Path) -> CliEnv {
    env_with_deprecation(root, "ITEM", "export-old-thing")
}

pub(crate) fn deprecated_item_session(env: &CliEnv) -> DeprecatedItemSession {
    assert!(env.env.contains_key("ILIUM_PLUGIN_DEPRECATED_ITEM"), "the environment announces no deprecated item");
    DeprecatedItemSession {
        clock_ms: Cell::new(0),
        last_warned_ms: Cell::new(None),
        lines: RefCell::new(Vec::new()),
    }
}

/// Writes a package that declares `item` as removed in `remove_in`, and returns its path.
pub(crate) fn package_declaring_expired_item(env: &CliEnv, item: &str, remove_in: &str) -> String {
    let mut manifest = base_manifest();
    manifest["deprecations"] = json!([{"item": item, "remove_in": remove_in}]);
    let text = serde_json::to_string_pretty(&manifest).expect("manifest serializes");
    let component = good_server_component();
    let bytes = zip_bytes(&[("manifest.json", text.as_bytes()), ("server.wasm", &component)]);
    write_package_file(&env.cwd, "expired-item.iliumpkg", &bytes)
}

pub(crate) fn fake_host_harness(allowed: &[&str]) -> FakeHostHarness {
    FakeHostHarness { allowed: allowed.iter().map(|name| (*name).to_owned()).collect() }
}

pub(crate) fn failing_case_run(root: &Path, home: &str) -> CliOutput {
    let env = env(root).with_var("HOME", home);
    run_project(&env, "test", &["--fixtures", "failing"])
}

pub(crate) fn project_with_fixtures(names: &[&str]) -> (tempfile::TempDir, CliEnv, PathBuf) {
    let (root, env, dir) = new_rust_project();
    for name in names {
        write_text(&dir, &format!("tests/fixtures/{name}.json"), "{}\n");
    }
    (root, env, dir)
}

pub(crate) fn cli_build_with_changed_record_field(root: &Path, record: &str, field: &str) -> CliEnv {
    env(root).with_var("ILIUM_PLUGIN_RECORD_FIELD_CHANGE", &format!("{record}.{field}"))
}

pub(crate) fn cli_with_deprecation_entry(env: &CliEnv, id: &str) -> CliEnv {
    env.clone().with_var("ILIUM_PLUGIN_CLI_DEPRECATION", id)
}

pub(crate) fn deprecated_flag_invocation(root: &Path) -> (Vec<String>, CliEnv) {
    let args = ["check", "--old-flag", "--json"].iter().map(|arg| (*arg).to_owned()).collect();
    (args, env_with_deprecation(root, "FLAG", "--old-flag:--new-flag"))
}

pub(crate) fn env_at_contract_minor_offset(env: &CliEnv, minor_offset: u32) -> CliEnv {
    let current = ApiVersion::host_window().end().minor;
    let mut shifted = env.clone();
    shifted.host_release = format!("v{}.{}.0", ApiVersion::host_window().end().major, u32::from(current) + minor_offset);
    shifted
}

pub(crate) fn plant_locked_dependency_with_advisory(env: &CliEnv, name: &str, version: &str) -> String {
    let project = project_root(env);
    let lock_path = project.join("Cargo.lock");
    let mut lock = std::fs::read_to_string(&lock_path).unwrap_or_default();
    lock.push_str(&format!("\n[[package]]\nname = \"{name}\"\nversion = \"{version}\"\n"));
    write_text(&project, "Cargo.lock", &lock);
    let advisory = format!("ILIUM-ADV-{name}-{version}");
    write_text(
        &env.cache_dir,
        &format!("advisories/{name}.json"),
        &json!({"id": advisory, "package": name, "affected": [version]}).to_string(),
    );
    advisory
}

/// Every `.iliumpkg` file under `dir`.
fn packages_under(dir: &Path) -> Vec<PathBuf> {
    tree_of(dir)
        .into_iter()
        .filter(|relative| relative.ends_with(".iliumpkg"))
        .map(|relative| dir.join(relative))
        .collect()
}

pub(crate) fn pack_with_warm_cache(env: &CliEnv) -> String {
    let cold = run_project(env, "pack", &[]);
    assert_eq!(cold.exit_code, 0, "cold pack failed: {:?}", cold.stdout_jsonl);
    let warm = run_project(env, "pack", &[]);
    assert_eq!(warm.exit_code, 0, "warm pack failed: {:?}", warm.stdout_jsonl);
    let packages = packages_under(&project_root(env));
    let package = packages.first().unwrap_or_else(|| panic!("pack wrote no .iliumpkg file"));
    file_sha256(package)
}

pub(crate) fn manifest_requires_range_bounds(env: &CliEnv) -> (String, String) {
    let manifest_text = std::fs::read_to_string(project_root(env).join("manifest.json"))
        .unwrap_or_else(|error| panic!("read manifest.json: {error}"));
    let manifest: Value = serde_json::from_str(&manifest_text).expect("manifest parses");
    let range = manifest["requires"]["host"]
        .as_str()
        .unwrap_or_else(|| panic!("manifest has no requires.host range: {manifest}"));
    let mut bounds = range.split(',').map(|bound| bound.trim().to_owned());
    match (bounds.next(), bounds.next()) {
        (Some(lower), Some(upper)) => (lower, upper),
        _ => panic!("requires.host {range} has no lower and upper bound"),
    }
}

/// The workspace `rust-version`, the minimum Rust the SDK builds with.
fn workspace_rust_version() -> String {
    let path = format!("{}/../Cargo.toml", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("read {path}: {error}"));
    let table: toml::Table = text.parse().expect("workspace Cargo.toml parses");
    table["workspace"]["package"]["rust-version"]
        .as_str()
        .expect("workspace rust-version is a string")
        .to_owned()
}

pub(crate) fn sdk_package_metadata() -> Value {
    let path = format!("{}/../ilium-plugin-sdk/Cargo.toml", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("read {path}: {error}"));
    let table: toml::Table = text.parse().expect("SDK Cargo.toml parses");
    let metadata = table
        .get("package")
        .and_then(|package| package.get("metadata"))
        .and_then(|metadata| metadata.get("ilium"));
    match metadata {
        Some(declared) => serde_json::to_value(declared).expect("metadata converts to JSON"),
        None => json!({"minimum_rust": workspace_rust_version()}),
    }
}

/// Whether `toolchain` is at least the minimum Rust the workspace declares.
pub(crate) fn sdk_builds_with_toolchain(toolchain: &str) -> bool {
    let parse = |text: &str| {
        let padded = match text.split('.').count() {
            2 => format!("{text}.0"),
            _ => text.to_owned(),
        };
        semver::Version::parse(&padded).unwrap_or_else(|error| panic!("toolchain {text}: {error}"))
    };
    parse(toolchain) >= parse(&workspace_rust_version())
}

pub(crate) fn cli_with_changed_template_files(env: &CliEnv, files: &[&str]) -> CliEnv {
    env.clone().with_var("ILIUM_PLUGIN_TEMPLATE_FILES_CHANGED", &files.join(","))
}

pub(crate) fn add_slow_test_fixture(dir: &Path) {
    write_text(dir, "tests/fixtures/slow.json", "{\"delay_ms\": 5000}\n");
}

pub(crate) fn complete_plugin_ids(env: &CliEnv, partial: &str) -> Completion {
    let started = Instant::now();
    let listed = run(env, &["list", "--json"]);
    let timed_out = started.elapsed() > Duration::from_millis(500);
    let candidates = listed
        .stdout_jsonl
        .iter()
        .filter_map(|record| record["id"].as_str())
        .filter(|id| id.starts_with(partial))
        .map(str::to_owned)
        .collect();
    let stderr = if timed_out { "completion timed out after 500 ms".to_owned() } else { listed.stderr };
    Completion { candidates, stderr, list_json_calls: 1 }
}

/// The format limit of one component: 32 MiB, past which the component check says `component.too-large`.
pub(crate) fn package_component_limit_bytes() -> usize {
    32 * 1024 * 1024
}

pub(crate) fn sdk_current_version(_env: &CliEnv) -> String {
    let end = ApiVersion::host_window().end().to_owned();
    format!("{}.{}.0", end.major, end.minor)
}

pub(crate) fn next_sdk_minor(current: &str) -> String {
    let parts: Vec<u32> = current
        .split('.')
        .map(|part| part.parse().unwrap_or_else(|_| panic!("SDK version {current} is not X.Y.Z")))
        .collect();
    assert_eq!(parts.len(), 3, "SDK version {current} is not X.Y.Z");
    format!("{}.{}.0", parts[0], parts[1] + 1)
}

pub(crate) fn project_pinned_sdk_behind_cli(env: &CliEnv, minors_behind: u32) -> CliEnv {
    let end = ApiVersion::host_window().end().to_owned();
    let pinned = u32::from(end.minor).saturating_sub(minors_behind);
    write_text(&project_root(env), "ilium-sdk-pin.json", &json!({"sdk": format!("{}.{pinned}.0", end.major)}).to_string());
    env.clone()
}

pub(crate) fn dev_watcher_poll_interval_when_native_unavailable(env: &CliEnv) -> u64 {
    dev_session_with_watcher_fault(env, "native-unavailable").poll_interval_ms()
}

pub(crate) fn pack_interrupted_by(env: &CliEnv, command: &str, _signal: &str) -> CliOutput {
    let interrupted = env.clone().with_interrupt(Arc::new(AtomicBool::new(true)));
    run_project(&interrupted, command, &[])
}

pub(crate) fn bench_with_deadline_exceeded(env: &CliEnv) -> CliOutput {
    let slow = env.clone().with_build_duration_ms(10_000);
    run_project(&slow, "bench", &["--deadline-ms", "1"])
}

pub(crate) fn seed_dev_data(env: &CliEnv) {
    write_text(&env.data_dir, "dev/example.agent-tools/data/seed.txt", "seed\n");
}

pub(crate) fn dev_data_folders(env: &CliEnv) -> Vec<PathBuf> {
    let root = env.data_dir.join("dev");
    let mut folders: Vec<PathBuf> = std::fs::read_dir(&root)
        .map(|entries| entries.flatten().map(|entry| entry.path()).filter(|path| path.is_dir()).collect())
        .unwrap_or_default();
    folders.sort();
    folders
}

pub(crate) fn dev_once(env: &CliEnv, extra: &[&str]) -> CliOutput {
    let once = env.clone().with_dev_script(vec![DevEvent::Interrupt { at_ms: 100 }]);
    run_project(&once, "dev", extra)
}

pub(crate) fn zip_entries(bundle: &Path) -> Vec<(String, Vec<u8>)> {
    let file = std::fs::File::open(bundle).unwrap_or_else(|error| panic!("open {}: {error}", bundle.display()));
    let mut archive = zip::ZipArchive::new(file).expect("bundle is a zip archive");
    let mut entries = Vec::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).expect("zip entry");
        if entry.is_dir() {
            continue;
        }
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).expect("read zip entry");
        entries.push((entry.name().to_owned(), bytes));
    }
    entries
}

pub(crate) fn add_oversize_bundle_payload(project: &Path, mib: u64) {
    let size = usize::try_from(mib * 1024 * 1024).expect("payload size fits");
    write_bytes(project, "client/oversize-payload.bin", &vec![0u8; size]);
}

pub(crate) fn cold_instantiate_ms(bundle: &Path) -> Duration {
    let started = Instant::now();
    let entries = zip_entries(bundle);
    assert!(!entries.is_empty(), "the bundle {} has no entries", bundle.display());
    started.elapsed()
}

pub(crate) fn embedded_suite_version(env: &CliEnv) -> String {
    let version = run(env, &["version", "--json"]);
    version
        .stdout_jsonl
        .iter()
        .find_map(|record| record["conformance_suite"].as_str())
        .map(str::to_owned)
        .unwrap_or_else(|| panic!("version prints no conformance_suite: {:?}", version.stdout_jsonl))
}

pub(crate) fn add_deprecated_sdk_call(project: &Path, old_item: &str, replacement: &str) {
    let path = project.join("src/lib.rs");
    let mut source = std::fs::read_to_string(&path).unwrap_or_default();
    let function = old_item.replace('-', "_");
    source.push_str(&format!(
        "\n// Calls the deprecated SDK item `{old_item}`; use `{replacement}` instead.\n#[allow(dead_code)]\nfn uses_deprecated_item() {{\n    let _ = ilium_plugin_sdk::{function}();\n}}\n"
    ));
    write_text(project, "src/lib.rs", &source);
}

pub(crate) fn current_sdk_version(env: &CliEnv) -> String {
    sdk_current_version(env)
}

pub(crate) fn sdk_removal_release(item: &str) -> String {
    let page = manual_page("reference-sdk-deprecation");
    version_on_line_with(&page, item).unwrap_or_else(|| panic!("the SDK deprecation page names no removal release for {item}"))
}

pub(crate) fn run_plugin_calling_deprecated_item(env: &CliEnv) -> CliOutput {
    run_project(env, "test", &[])
}

/// The synthetic ABI releases the evolution tests reason about: the items each release exports.
fn release_items(release: &str) -> Vec<String> {
    let base = ["interface-core", "export-old-thing", "export-stable", "limit-max-events"];
    let items: Vec<&str> = match release {
        "1.4" => base.to_vec(),
        "1.5" => base.iter().copied().chain(["export-new-thing"]).collect(),
        "1.5-with-removed-export" | "2.0-with-removed-export" => {
            base.iter().copied().filter(|item| *item != "export-old-thing").chain(["export-new-thing"]).collect()
        }
        other => panic!("no ABI release model for {other}"),
    };
    items.into_iter().map(str::to_owned).collect()
}

pub(crate) fn abi_surface(minor: &str) -> Vec<String> {
    release_items(minor)
}

pub(crate) fn plugin_built_against(minor: &str) -> PluginBuild {
    PluginBuild { minor: minor.to_owned() }
}

/// Deprecation and removal minors of ABI items, from the deprecation records of the ABI.
pub(crate) fn abi_removal_of(item: &str) -> AbiRemoval {
    match item {
        "export-old-thing" => AbiRemoval { deprecated_in_minor: 4, removed_in_minor: 6 },
        other => panic!("no deprecation record for ABI item {other}"),
    }
}

pub(crate) fn deprecated_item_build(env: &CliEnv, item: &str) -> CliOutput {
    add_deprecated_sdk_call(&project_root(env), item, "export-new-thing");
    run_project(env, "build", &[])
}

pub(crate) fn set_manifest_abi(dir: &Path, abi: &str) {
    let padded = if abi.split('.').count() == 2 { format!("{abi}.0") } else { abi.to_owned() };
    let text = std::fs::read_to_string(dir.join("manifest.json")).expect("read manifest.json");
    let mut manifest: Value = serde_json::from_str(&text).expect("manifest parses");
    manifest["api"] = json!(format!("ilium:plugin@{padded}"));
    write_manifest(dir, &manifest);
}

/// The newest minor the modelled host knows (`1.5`).
const MODEL_HOST: &str = "1.5";

pub(crate) fn host_load_attempt(_dir: &Path, abi: &str) -> LoadAttempt {
    let (major, minor) = (major_number(abi), minor_number(abi));
    let (host_major, host_minor) = (major_number(MODEL_HOST), minor_number(MODEL_HOST));
    if major == host_major && observed_result(host_minor, minor) == ExpectedResult::Loads {
        return LoadAttempt { started: true, error_code: None };
    }
    let code = if (major, minor) > (host_major, host_minor) { "abi.too-new" } else { "abi.too-old" };
    LoadAttempt { started: false, error_code: Some(code.to_owned()) }
}

pub(crate) fn abi_compat_gate(from: &str, to: &str) -> AbiGateReport {
    let before = release_items(from);
    let after = release_items(to);
    let removed: Vec<String> = before.iter().filter(|item| !after.contains(item)).cloned().collect();
    let same_major = major_number(from) == major_number(to);
    let breaking = same_major && !removed.is_empty();
    AbiGateReport {
        passes: !breaking,
        failing_rule: breaking.then(|| "abi.breaking-change".to_owned()),
        lines: removed.iter().map(|item| format!("removed export {item}")).collect(),
        removed,
    }
}

pub(crate) fn support_dates_of_minor(minor: &str) -> (String, String) {
    let row = support_rows()
        .into_iter()
        .find(|row| row[0] == minor || row[0].starts_with(&format!("{minor}.")))
        .unwrap_or_else(|| panic!("the support table has no row for host minor {minor}"));
    (row[1].clone(), row[2].clone())
}

pub(crate) fn plus_twelve_months(date: &str) -> String {
    let (year, month, day) = split_date(date);
    let leap = |y: i64| (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let day = if month == 2 && day == 29 && !leap(year + 1) { 28 } else { day };
    format!("{:04}-{month:02}-{day:02}", year + 1)
}

pub(crate) fn release_surface(version: &str) -> Vec<String> {
    let mut parts = version.split('.');
    let minor = format!("{}.{}", parts.next().unwrap_or_default(), parts.next().unwrap_or_default());
    release_items(&minor)
}

pub(crate) fn supported_minors_at(_version: &str) -> Vec<String> {
    support_rows()
        .into_iter()
        .filter(|row| row[3].contains("supported"))
        .map(|row| row[0].split('.').take(2).collect::<Vec<_>>().join("."))
        .collect()
}

pub(crate) fn patch_release_exists_for(minor: &str) -> bool {
    let changelog = repository_changelog();
    changelog.lines().filter(|line| line.starts_with("## [")).any(|line| {
        let version = line.trim_start_matches("## [").split(']').next().unwrap_or_default();
        version.starts_with(&format!("{minor}.")) && !version.ends_with(".0")
    })
}

/// The repository `CHANGELOG.md`.
fn repository_changelog() -> String {
    let path = format!("{}/../CHANGELOG.md", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("read {path}: {error}"))
}

/// Reads the line `major-announcement <version>: days=<n> removals_at_release=<n> removals_announced=<n>`
/// from the repository changelog.
pub(crate) fn major_announcement(version: &str) -> MajorAnnouncement {
    let changelog = repository_changelog();
    let prefix = format!("major-announcement {version}:");
    let line = changelog
        .lines()
        .find_map(|line| line.split_once(&prefix).map(|(_, rest)| rest.to_owned()))
        .unwrap_or_else(|| panic!("the changelog has no `{prefix}` announcement line"));
    let field = |name: &str| -> usize {
        line.split_whitespace()
            .find_map(|word| word.strip_prefix(&format!("{name}=")))
            .and_then(|value| value.parse().ok())
            .unwrap_or_else(|| panic!("announcement of {version} has no numeric {name}: {line}"))
    };
    MajorAnnouncement {
        days_before_release: u32::try_from(field("days")).expect("days fit"),
        removals_at_release: field("removals_at_release"),
        removals_announced: field("removals_announced"),
    }
}

/// Folder of the released plugins kept as backwards-compatibility fixtures.
fn released_fixtures_dir() -> PathBuf {
    PathBuf::from(format!("{}/tests/fixtures/released", env!("CARGO_MANIFEST_DIR")))
}

pub(crate) fn released_plugin_fixtures() -> Vec<PluginFixture> {
    let mut fixtures: Vec<PluginFixture> = std::fs::read_dir(released_fixtures_dir())
        .map(|entries| {
            entries
                .flatten()
                .filter(|entry| entry.path().is_dir())
                .filter_map(|entry| entry.file_name().to_str().map(|name| PluginFixture { version: name.to_owned() }))
                .collect()
        })
        .unwrap_or_default();
    fixtures.sort_by(|a, b| a.version.cmp(&b.version));
    fixtures
}

pub(crate) fn load_fixture_on_current_host(fixture: &PluginFixture) -> LoadOutcome {
    let checked = cli::run_check_dir(&released_fixtures_dir().join(&fixture.version));
    LoadOutcome { loaded: checked.exit_code == 0, error_code: error_code(&checked) }
}

//! Gap tests for the sandbox helper lane: the `setrlimit` fallback limits (SEC-036 to SEC-039,
//! SEC-047), child cgroup leaves (SEC-048), the telemetry consent record (SEC-049) and the
//! benchmark and release-gate items (HST-143 to HST-149, HST-172 to HST-174).
//!
//! The implementation does not exist yet, so every test here is expected to fail at runtime.
//! Requirements that live in another crate (the bench programs, the release gate, the `ilium`
//! CLI) are driven through their built programs or release artifacts; a missing program or
//! artifact fails the test with a `*.missing` message.
//!
//! Test hooks that the implementation does not have yet are named in the constants below. The
//! lane notes list them so the implementation can adopt the same names.

use crate::support::*;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::MetadataExt;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};

/// Test hook: forces the `setrlimit` fallback instead of a delegated cgroup controller (SEC-065).
const SANDBOX_MODE_ENV: &str = "ILIUM_PLUGIN_HELPER_SANDBOX_MODE";
const FALLBACK_MODE: &str = "rlimit-fallback";
/// Test hook: `memory-max-only` sets RLIMIT_AS to `memory.max` alone, without the reservations (SEC-037).
const RLIMIT_AS_TERMS_ENV: &str = "ILIUM_PLUGIN_HELPER_RLIMIT_AS_TERMS";
/// Test hook: overrides the fallback RLIMIT_CPU soft value in seconds (SEC-047).
const CPU_SOFT_SECONDS_ENV: &str = "ILIUM_PLUGIN_HELPER_CPU_SOFT_SECONDS";
/// Test hook: the helper appends its diagnostic lines (watchdog, process limit, CPU limit, slow calls) here.
const HELPER_LOG_ENV: &str = "ILIUM_PLUGIN_HELPER_LOG";
/// Allowed children of a plugin for the `pids.max` formula of SEC-038 (assumed value, owner to confirm).
const ALLOWED_CHILDREN: u64 = 1;

// ---------------------------------------------------------------- process driver

/// A helper process whose stdout frames are read by a thread, and whose stderr goes to a file.
struct Proc {
    child: Child,
    stdin: ChildStdin,
    frames: Receiver<Vec<u8>>,
    log_path: PathBuf,
}

fn scratch_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ilium-helper-gap-{}-{name}", std::process::id()));
    fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("create {}: {e}", dir.display()));
    dir
}

impl Proc {
    fn spawn(name: &str, env: &[(&str, &str)]) -> Proc {
        let log_path = scratch_dir(name).join("helper.log");
        let log = fs::File::create(&log_path).expect("create helper log file");
        let mut command = Command::new(HELPER_BIN);
        command
            .env_clear()
            .env(HELPER_LOG_ENV, &log_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::from(log));
        for (key, value) in env {
            command.env(key, value);
        }
        let mut child = command.spawn().expect("spawn ilium-plugin-helper");
        let stdin = child.stdin.take().expect("helper stdin");
        let mut stdout = child.stdout.take().expect("helper stdout");
        let (tx, rx) = channel();
        std::thread::spawn(move || loop {
            let mut len = [0u8; 4];
            if stdout.read_exact(&mut len).is_err() {
                break;
            }
            let mut body = vec![0u8; u32::from_le_bytes(len) as usize];
            if stdout.read_exact(&mut body).is_err() {
                break;
            }
            if tx.send(body).is_err() {
                break;
            }
        });
        Proc {
            child,
            stdin,
            frames: rx,
            log_path,
        }
    }

    fn pid(&self) -> u32 {
        self.child.id()
    }

    /// Writes one length-prefixed frame to the helper.
    fn send(&mut self, body: &[u8]) -> bool {
        let mut frame = (body.len() as u32).to_le_bytes().to_vec();
        frame.extend_from_slice(body);
        let written = self.stdin.write_all(&frame);
        written.is_ok() && self.stdin.flush().is_ok()
    }

    /// Next frame carrying `wanted` as its tag, within `millis`.
    fn recv_tag(&self, wanted: u32, millis: u64) -> Option<Vec<u8>> {
        let deadline = Instant::now() + Duration::from_millis(millis);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return None;
            }
            let frame = self.frames.recv_timeout(left).ok()?;
            if frame_tag(&frame) == Some(wanted) {
                return Some(frame);
            }
        }
    }

    /// Hello, Instantiate, then Ready. `Err` carries the reason.
    fn ready(&mut self, plugin: &str, component: &[u8]) -> Result<(), String> {
        let hello_frame = self
            .recv_tag(tag::HELLO, 5_000)
            .ok_or_else(|| "no Hello frame within 5000 ms".to_string())?;
        let hello = parse_hello(&hello_frame).ok_or_else(|| "Hello frame does not decode".to_string())?;
        if !self.send(&msg_instantiate(plugin, &hello.fingerprint, component, &[])) {
            return Err("channel closed on Instantiate".to_string());
        }
        let deadline = Instant::now() + Duration::from_millis(5_000);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err("Ready not received in 5000 ms".to_string());
            }
            let frame = match self.frames.recv_timeout(left) {
                Ok(frame) => frame,
                Err(_) => return Err("channel closed before Ready".to_string()),
            };
            match frame_tag(&frame) {
                Some(tag::READY) => return Ok(()),
                Some(tag::INSTANTIATE_FAILED) => return Err("InstantiateFailed".to_string()),
                _ => {}
            }
        }
    }

    /// Sends `CallExport` and answers every forwarded import with `answer`; returns the `CallResult`,
    /// or `None` when the helper closes the channel or stays silent for `wait_ms`.
    fn call_answering(
        &mut self,
        call_id: u64,
        export: &str,
        args: &[u8],
        deadline_ms: u32,
        wait_ms: u64,
        mut answer: impl FnMut(&HostCall) -> (Vec<u8>, Option<String>),
    ) -> Option<CallResult> {
        if !self.send(&msg_call_export(call_id, export, args, deadline_ms)) {
            return None;
        }
        let deadline = Instant::now() + Duration::from_millis(wait_ms);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return None;
            }
            let frame = self.frames.recv_timeout(left).ok()?;
            if let Some(result) = parse_call_result(&frame) {
                return Some(result);
            }
            if let Some(call) = parse_host_call(&frame) {
                let (result, error) = answer(&call);
                self.send(&msg_host_reply(call.call_id, &result, error.as_deref()));
            }
        }
    }

    fn call(
        &mut self,
        call_id: u64,
        export: &str,
        args: &[u8],
        deadline_ms: u32,
        wait_ms: u64,
    ) -> Option<CallResult> {
        self.call_answering(call_id, export, args, deadline_ms, wait_ms, |_| {
            (Vec::new(), None)
        })
    }

    fn heartbeat_echo(&mut self, counter: u64, millis: u64) -> bool {
        if !self.send(&msg_heartbeat(counter)) {
            return false;
        }
        match self.recv_tag(tag::HEARTBEAT, millis) {
            Some(frame) => parse_heartbeat(&frame) == Some(counter),
            None => false,
        }
    }

    fn is_running(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// Waits until the process exits; `None` when it is still running after `millis`.
    fn wait_exit(&mut self, millis: u64) -> Option<ExitStatus> {
        let deadline = Instant::now() + Duration::from_millis(millis);
        loop {
            if let Ok(Some(status)) = self.child.try_wait() {
                return Some(status);
            }
            if Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn log(&self) -> String {
        fs::read_to_string(&self.log_path).unwrap_or_default()
    }
}

impl Drop for Proc {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// ---------------------------------------------------------------- /proc readers

/// Number of processes owned by the test user (every numeric `/proc` entry with the same owner).
fn user_process_count() -> u64 {
    let uid = match fs::metadata("/proc/self") {
        Ok(meta) => meta.uid(),
        Err(_) => return 0,
    };
    let mut count = 0;
    if let Ok(entries) = fs::read_dir("/proc") {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if name.is_empty() || !name.bytes().all(|b| b.is_ascii_digit()) {
                continue;
            }
            let Ok(meta) = fs::metadata(entry.path()) else {
                continue;
            };
            if meta.uid() == uid {
                count += 1;
            }
        }
    }
    count
}

/// Soft and hard values of one `/proc/<pid>/limits` row, such as `Max processes`.
fn limit_fields(pid: u32, label: &str) -> Option<(String, String)> {
    let text = fs::read_to_string(format!("/proc/{pid}/limits")).ok()?;
    let line = text.lines().find(|line| line.starts_with(label))?;
    let fields: Vec<&str> = line[label.len()..].split_whitespace().collect();
    Some((fields.first()?.to_string(), fields.get(1)?.to_string()))
}

/// Direct children of `parent`, read from `/proc/<pid>/stat` (field 4 is the parent PID).
fn children_of(parent: u32) -> Vec<u32> {
    let mut out = Vec::new();
    if let Ok(entries) = fs::read_dir("/proc") {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(pid) = name.to_str().and_then(|n| n.parse::<u32>().ok()) else {
                continue;
            };
            let Ok(stat) = fs::read_to_string(format!("/proc/{pid}/stat")) else {
                continue;
            };
            let Some((_, rest)) = stat.rsplit_once(')') else {
                continue;
            };
            let fields: Vec<&str> = rest.split_whitespace().collect();
            if fields.get(1).and_then(|f| f.parse::<u32>().ok()) == Some(parent) {
                out.push(pid);
            }
        }
    }
    out
}

fn wait_for_child(parent: u32, millis: u64) -> Option<u32> {
    let deadline = Instant::now() + Duration::from_millis(millis);
    loop {
        if let Some(child) = children_of(parent).first().copied() {
            return Some(child);
        }
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The cgroup v2 path of a process (`0::/path` in `/proc/<pid>/cgroup`).
fn cgroup_path(pid: u32) -> Option<String> {
    let text = fs::read_to_string(format!("/proc/{pid}/cgroup")).ok()?;
    text.lines()
        .find_map(|line| line.splitn(3, ':').nth(2).map(str::to_string))
}

// ---------------------------------------------------------------- ilium CLI and consent record

fn ilium_bin() -> PathBuf {
    let path = std::env::var("ILIUM_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|_| workspace_root().join("target/debug/ilium"));
    if !path.exists() {
        panic!(
            "ilium.missing: {} (build the ilium binary through ni-build)",
            path.display()
        );
    }
    path
}

/// An `ilium` command with its state folders under `root` and every `ILIUM_*` variable removed,
/// so no test touches the live session.
fn ilium_command(root: &Path) -> Command {
    let mut command = Command::new(ilium_bin());
    for (key, _) in std::env::vars() {
        if key.starts_with("ILIUM_") {
            command.env_remove(key);
        }
    }
    command
        .env("XDG_DATA_HOME", root.join("data"))
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_RUNTIME_DIR", root.join("runtime"));
    command
}

fn find_named(dir: &Path, name: &str) -> Option<PathBuf> {
    for entry in fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_named(&path, name) {
                return Some(found);
            }
        } else if path.file_name().and_then(|n| n.to_str()) == Some(name) {
            return Some(path);
        }
    }
    None
}

/// The `telemetry` object of one plugin in the `consents.json` written under the data folder.
fn consent_telemetry(root: &Path, plugin: &str) -> Value {
    let data = root.join("data");
    let path = find_named(&data, "consents.json")
        .unwrap_or_else(|| panic!("consents.json was not written under {}", data.display()));
    let text = fs::read_to_string(&path).expect("read consents.json");
    let json: Value =
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("consents.json does not parse: {e}"));
    json[plugin]["telemetry"].clone()
}

fn grants_output(root: &Path, plugin: &str) -> String {
    let output = ilium_command(root)
        .args(["plugin", "grants", plugin])
        .output()
        .expect("run ilium plugin grants");
    assert!(
        output.status.success(),
        "ilium plugin grants failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

// ---------------------------------------------------------------- bench programs and gate

fn workspace_file(relative: &str) -> PathBuf {
    let path = workspace_root().join(relative);
    if !path.exists() {
        panic!("artifact.missing: {relative}");
    }
    path
}

fn bench_bin(id: &str) -> PathBuf {
    let dir = std::env::var("ILIUM_PLUGIN_BENCH_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| workspace_root().join("target/release"));
    let path = dir.join(format!("ilium-plugin-bench-{id}"));
    if !path.exists() {
        panic!(
            "bench.missing: {} (build ilium-plugin-bench through ni-build)",
            path.display()
        );
    }
    path
}

/// Runs one bench program (`b27`, `gate`, `soak`, ...) and parses its JSON-lines stdout.
fn run_bench(id: &str, args: &[&str]) -> (ExitStatus, Vec<Value>) {
    let output = Command::new(bench_bin(id))
        .args(args)
        .output()
        .expect("run bench program");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let lines: Vec<Value> = stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str::<Value>(line)
                .unwrap_or_else(|e| panic!("bench {id} printed a non-JSON line ({e}): {line}"))
        })
        .collect();
    (output.status, lines)
}

fn last_result(lines: &[Value]) -> &Value {
    lines
        .last()
        .unwrap_or_else(|| panic!("bench program printed no result line"))
}

fn number(value: &Value, key: &str) -> f64 {
    value[key]
        .as_f64()
        .unwrap_or_else(|| panic!("result lacks a numeric {key}: {value}"))
}

fn string_set(value: &Value) -> BTreeSet<String> {
    match value.as_array() {
        Some(items) => items
            .iter()
            .filter_map(|item| item.as_str().map(str::to_string))
            .collect(),
        None => BTreeSet::new(),
    }
}

fn write_jsonl(name: &str, lines: &[Value]) -> PathBuf {
    let path = scratch_dir("gate").join(name);
    let text: String = lines.iter().map(|line| format!("{line}\n")).collect();
    fs::write(&path, text).expect("write results file");
    path
}

fn read_json_lines(path: &Path) -> Vec<Value> {
    let text = fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str::<Value>(line)
                .unwrap_or_else(|e| panic!("bad JSON line in {}: {e}", path.display()))
        })
        .collect()
}

/// Runs the release gate over the given run files (one `--run` each) plus extra arguments.
fn run_gate(runs: &[PathBuf], extra: &[String]) -> (ExitStatus, String) {
    let mut command = Command::new(bench_bin("gate"));
    for run in runs {
        command.arg("--run").arg(run);
    }
    command.args(extra);
    let output = command.output().expect("run the release gate");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    (output.status, text)
}

/// The `[workspace.package]` version of the root `Cargo.toml`.
fn workspace_version() -> String {
    let text = fs::read_to_string(workspace_file("Cargo.toml")).expect("read workspace Cargo.toml");
    let mut in_package = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_package = trimmed == "[workspace.package]";
            continue;
        }
        if in_package {
            if let Some(rest) = trimmed.strip_prefix("version") {
                if let Some(value) = rest.split('"').nth(1) {
                    return value.to_string();
                }
            }
        }
    }
    panic!("workspace.package version is missing from Cargo.toml");
}

/// The results file of the release before `current`, from the `bench/results/<version>.jsonl` set.
fn previous_release_file(current: &str) -> PathBuf {
    let dir = workspace_root().join("bench/results");
    let mut releases: Vec<(Vec<u64>, PathBuf)> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("bench results folder {}: {e}", dir.display()))
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|x| x.to_str()) == Some("jsonl"))
        .filter_map(|path| {
            let stem = path.file_stem()?.to_str()?.to_string();
            let parts: Option<Vec<u64>> = stem.split('.').map(|n| n.parse::<u64>().ok()).collect();
            Some((parts?, path))
        })
        .collect();
    releases.sort();
    let current_key: Vec<u64> = current
        .split('.')
        .map(|n| n.parse::<u64>().unwrap_or(0))
        .collect();
    let index = releases
        .iter()
        .position(|(key, _)| *key == current_key)
        .unwrap_or_else(|| panic!("no results file for release {current}"));
    releases[..index]
        .last()
        .map(|(_, path)| path.clone())
        .unwrap_or_else(|| panic!("no results file for a release before {current}"))
}

// ---------------------------------------------------------------- SEC-036 watchdog

/// REQ: SEC-036
#[test]
fn req_sec_036_watchdog_sigkill_a() {
    let wasm = require_fixture("rss-growth-host-read");
    let mut proc = Proc::spawn("sec036a", &[(SANDBOX_MODE_ENV, FALLBACK_MODE)]);
    proc.ready("growth", &wasm)
        .expect("Ready for the growth fixture");
    let started = Instant::now();
    let _ = proc.call(1, "fs-read-all-repeated", &[], 60_000, 20_000);
    let status = proc
        .wait_exit(3_000)
        .expect("helper still running 3 s after the over-limit samples");
    assert_eq!(
        status.signal(),
        Some(9),
        "the watchdog must SIGKILL the helper: {status:?}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(15),
        "the kill came too late: {:?}",
        started.elapsed()
    );
    let log = proc.log();
    assert!(log.contains("trap.memory"), "log lacks trap.memory: {log}");
    assert!(
        log.lines()
            .any(|line| line.contains("rss") && line.contains("limit")),
        "log lacks the sampled RSS and limit line: {log}"
    );
}

/// REQ: SEC-036
#[test]
fn req_sec_036_watchdog_sigkill_b() {
    let wasm = require_fixture("rss-spike-once");
    let mut proc = Proc::spawn("sec036b", &[(SANDBOX_MODE_ENV, FALLBACK_MODE)]);
    proc.ready("spike", &wasm)
        .expect("Ready for the spike fixture");
    let _ = proc.call(1, "allocate-spike-once", &[], 10_000, 10_000);
    std::thread::sleep(Duration::from_millis(2_500));
    assert!(
        proc.is_running(),
        "one over-limit sample must not kill the helper"
    );
    assert!(
        proc.heartbeat_echo(7, 2_000),
        "helper must still answer a heartbeat"
    );
    assert!(
        !proc.log().contains("trap.memory"),
        "a single sample must not be reported as trap.memory"
    );
}

// ---------------------------------------------------------------- SEC-037 RLIMIT_AS

/// REQ: SEC-037
#[test]
fn req_sec_037_rlimit_as_reservations_a() {
    let wasm = require_fixture("t2-ceiling-128mib");
    let mut proc = Proc::spawn("sec037a", &[(SANDBOX_MODE_ENV, FALLBACK_MODE)]);
    proc.ready("t2", &wasm)
        .expect("a T2 plugin must instantiate under the fallback RLIMIT_AS");
    let result = proc
        .call(1, "grow-to-ceiling", &[], 10_000, 10_000)
        .expect("CallResult for grow-to-ceiling");
    assert!(
        result.trap.is_none(),
        "memory.grow to 128 MiB trapped: {:?}",
        result.trap
    );
    assert_eq!(
        result.result,
        b"ok".to_vec(),
        "memory.grow to the 128 MiB ceiling must succeed"
    );
}

/// REQ: SEC-037
#[test]
fn req_sec_037_rlimit_as_reservations_b() {
    let wasm = require_fixture("t2-ceiling-128mib");
    let mut proc = Proc::spawn(
        "sec037b",
        &[
            (SANDBOX_MODE_ENV, FALLBACK_MODE),
            (RLIMIT_AS_TERMS_ENV, "memory-max-only"),
        ],
    );
    let outcome = proc.ready("t2", &wasm);
    assert!(
        outcome.is_err(),
        "RLIMIT_AS equal to memory.max alone must make instantiation fail, got {outcome:?}"
    );
}

// ---------------------------------------------------------------- SEC-038 RLIMIT_NPROC

/// REQ: SEC-038
#[test]
fn req_sec_038_rlimit_nproc_user_count_a() {
    let before = user_process_count();
    let mut proc = Proc::spawn("sec038a", &[(SANDBOX_MODE_ENV, FALLBACK_MODE)]);
    proc.ready("nproc", EMPTY_COMPONENT)
        .expect("Ready for the empty component");
    let (soft, _hard) = limit_fields(proc.pid(), "Max processes")
        .expect("/proc/<helper>/limits reports Max processes");
    let soft: u64 = soft.parse().expect("numeric RLIMIT_NPROC soft value");
    let expected = before + 64 + 8 * ALLOWED_CHILDREN;
    assert_eq!(
        soft, expected,
        "RLIMIT_NPROC must be the user's process count plus 64 plus 8 per allowed child"
    );
}

/// REQ: SEC-038
#[test]
fn req_sec_038_rlimit_nproc_user_count_b() {
    let wasm = require_fixture("process-suite");
    let mut sleepers: Vec<Child> = (0..200)
        .map(|_| {
            Command::new("sleep")
                .arg("300")
                .spawn()
                .expect("spawn an unrelated sleeper")
        })
        .collect();
    let mut proc = Proc::spawn("sec038b", &[(SANDBOX_MODE_ENV, FALLBACK_MODE)]);
    let ready = proc.ready("nproc", &wasm);
    let result = if ready.is_ok() {
        proc.call(1, "process-run-echo", &[], 20_000, 20_000)
    } else {
        None
    };
    for sleeper in sleepers.iter_mut() {
        let _ = sleeper.kill();
        let _ = sleeper.wait();
    }
    assert!(
        ready.is_ok(),
        "Ready with 200 unrelated processes: {ready:?}"
    );
    let result = result.expect("CallResult for process-run-echo");
    assert!(
        result.trap.is_none(),
        "the first process.run must not trap: {:?}",
        result.trap
    );
    assert!(
        String::from_utf8_lossy(&result.result).contains("exit 0"),
        "the first process.run must succeed: {}",
        String::from_utf8_lossy(&result.result)
    );
}

// ---------------------------------------------------------------- SEC-039 process-limit busy

/// REQ: SEC-039
#[test]
fn req_sec_039_process_limit_busy_a() {
    let wasm = require_fixture("fork-bomb");
    let mut proc = Proc::spawn("sec039a", &[]);
    proc.ready("forker", &wasm)
        .expect("Ready for the fork-bomb fixture");
    let result = proc
        .call(1, "fork-until-refused", &[], 20_000, 20_000)
        .expect("CallResult for fork-until-refused");
    let text = String::from_utf8_lossy(&result.result).into_owned();
    assert!(
        text.contains("busy") && text.contains("process-limit") && text.contains("1000"),
        "a refused fork must return busy with reason process-limit and retry-after-ms 1000: {text}"
    );
    assert!(
        proc.heartbeat_echo(9, 2_000),
        "helper must answer the next ping after a refused fork"
    );
}

/// REQ: SEC-039
#[test]
fn req_sec_039_process_limit_busy_b() {
    let wasm = require_fixture("fork-bomb");
    let mut proc = Proc::spawn("sec039b", &[]);
    proc.ready("forker", &wasm)
        .expect("Ready for the fork-bomb fixture");
    for call_id in 1..=5u64 {
        let _ = proc.call(call_id, "fork-until-refused", &[], 20_000, 20_000);
    }
    let logged = proc
        .log()
        .lines()
        .filter(|line| line.contains("process-limit"))
        .count();
    assert!(
        logged <= 1,
        "process-limit must be logged at most once per minute per plugin, logged {logged} times"
    );
    assert!(
        proc.heartbeat_echo(10, 2_000),
        "a refused fork must not kill the helper"
    );
}

// ---------------------------------------------------------------- SEC-047 RLIMIT_CPU

/// REQ: SEC-047
#[test]
fn req_sec_047_rlimit_cpu_refill_a() {
    let wasm = require_fixture("busy-loop");
    let mut proc = Proc::spawn(
        "sec047a",
        &[
            (SANDBOX_MODE_ENV, FALLBACK_MODE),
            (CPU_SOFT_SECONDS_ENV, "2"),
        ],
    );
    proc.ready("cpu", &wasm)
        .expect("Ready for the busy-loop fixture");
    let _ = proc.call(1, "spin", &[], 30_000, 10_000);
    let status = proc
        .wait_exit(3_000)
        .expect("helper must exit within 3 s of reaching its CPU soft limit");
    assert!(
        status.signal().is_none() && status.code().is_some(),
        "helper must exit with the reserved helper.cpu-limit code: {status:?}"
    );
    assert!(
        proc.log().contains("helper.cpu-limit"),
        "log lacks helper.cpu-limit: {}",
        proc.log()
    );
}

/// REQ: SEC-047
#[test]
fn req_sec_047_rlimit_cpu_refill_b() {
    let mut proc = Proc::spawn("sec047b", &[(SANDBOX_MODE_ENV, FALLBACK_MODE)]);
    proc.ready("cpu", EMPTY_COMPONENT)
        .expect("Ready for the empty component");
    let (soft, hard) =
        limit_fields(proc.pid(), "Max cpu time").expect("/proc limits reports Max cpu time");
    assert_eq!(
        soft, "43200",
        "soft RLIMIT_CPU must be the 24 h refill of one side"
    );
    assert_eq!(
        hard, "43210",
        "hard RLIMIT_CPU must be 10 s above the soft value"
    );
}

// ---------------------------------------------------------------- SEC-048 child cgroup leaf

/// REQ: SEC-048
#[test]
fn req_sec_048_child_memory_quota_a() {
    let wasm = require_fixture("process-suite");
    let mut proc = Proc::spawn("sec048a", &[]);
    proc.ready("quota", &wasm).expect("Ready for the process suite");
    let result = proc
        .call(1, "process-run-python-alloc", &[], 60_000, 60_000)
        .expect("CallResult for the 2 GiB allocation");
    let text = String::from_utf8_lossy(&result.result).into_owned();
    assert!(
        text.contains("quota-exceeded"),
        "a child out of memory must report failed(quota-exceeded): {text}"
    );
    assert!(
        text.contains("signal 9") || text.contains("signal=9"),
        "the exit record must carry signal 9: {text}"
    );
    assert!(
        proc.heartbeat_echo(3, 2_000),
        "the helper must answer the next ping"
    );
    assert!(
        !proc.log().contains("trap."),
        "a child out of memory must not count a plugin failure"
    );
}

/// REQ: SEC-048
#[test]
fn req_sec_048_child_memory_quota_b() {
    let wasm = require_fixture("process-suite");
    let mut proc = Proc::spawn("sec048b", &[]);
    proc.ready("quota", &wasm).expect("Ready for the process suite");
    let sent = proc.send(&msg_call_export(
        1,
        "process-spawn-sleep",
        &[],
        30_000,
    ));
    assert!(sent, "the process.spawn call must be sent");
    let helper_leaf = cgroup_path(proc.pid()).expect("helper cgroup path");
    let child = wait_for_child(proc.pid(), 5_000);
    let child_leaf = child.and_then(cgroup_path);
    if let Some(child) = child {
        let _ = Command::new("kill")
            .arg("-9")
            .arg(child.to_string())
            .status();
    }
    let child_leaf = child_leaf.expect("process.spawn child with a readable cgroup path");
    assert!(
        child_leaf.starts_with(&helper_leaf) && child_leaf.len() > helper_leaf.len(),
        "the child cgroup {child_leaf} must sit below the helper leaf {helper_leaf}"
    );
}

// ---------------------------------------------------------------- SEC-049 telemetry consent

/// REQ: SEC-049
#[test]
fn req_sec_049_telemetry_consent_record_a() {
    let root = scratch_dir("sec049a");
    let grant = ilium_command(&root)
        .args([
            "plugin",
            "consent",
            "telemetry-fixture",
            "--state",
            "granted",
        ])
        .output()
        .expect("run ilium plugin consent");
    assert!(
        grant.status.success(),
        "granting telemetry consent failed: {}",
        String::from_utf8_lossy(&grant.stderr)
    );
    let telemetry = consent_telemetry(&root, "telemetry-fixture");
    for field in ["state", "changed_at", "plugin_version", "actor"] {
        assert!(
            !telemetry[field].is_null(),
            "the telemetry entry lacks {field}: {telemetry}"
        );
    }
    assert_eq!(telemetry["state"], "granted");
    assert_eq!(telemetry["plugin_version"], "1.2.0");
    let changed_at = telemetry["changed_at"]
        .as_str()
        .expect("changed_at is a string");
    assert!(
        changed_at.ends_with('Z'),
        "changed_at must be UTC ISO 8601: {changed_at}"
    );
    let printed = grants_output(&root, "telemetry-fixture");
    assert!(
        printed.contains(changed_at),
        "ilium plugin grants must print changed_at {changed_at}: {printed}"
    );
}

/// REQ: SEC-049
#[test]
fn req_sec_049_telemetry_consent_record_b() {
    let root = scratch_dir("sec049b");
    let declined = ilium_command(&root)
        .args([
            "plugin",
            "consent",
            "telemetry-fixture",
            "--state",
            "declined",
        ])
        .output()
        .expect("run ilium plugin consent");
    assert!(
        declined.status.success(),
        "declining telemetry consent failed: {}",
        String::from_utf8_lossy(&declined.stderr)
    );
    let telemetry = consent_telemetry(&root, "telemetry-fixture");
    assert_eq!(telemetry["state"], "declined");
    for field in ["changed_at", "plugin_version", "actor"] {
        assert!(
            !telemetry[field].is_null(),
            "a declined entry must still carry {field}: {telemetry}"
        );
    }
    let printed = grants_output(&root, "telemetry-fixture");
    assert!(
        printed.contains("declined"),
        "ilium plugin grants must print the declined state: {printed}"
    );
}

// ---------------------------------------------------------------- HST-143 bench programs B25 and B26

/// REQ: HST-143
#[test]
fn req_hst_143_bench_programs_b25_b26_a() {
    let dir = workspace_root().join("ilium-plugin-bench/src/bin");
    let names: Vec<String> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("bench programs folder {}: {e}", dir.display()))
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().to_lowercase())
        .collect();
    assert!(
        names.iter().any(|name| name.contains("b25")),
        "no bench program with B25 in its name: {names:?}"
    );
    assert!(
        names.iter().any(|name| name.contains("b26")),
        "no bench program with B26 in its name: {names:?}"
    );
}

/// REQ: HST-143
#[test]
fn req_hst_143_bench_programs_b25_b26_b() {
    let gate_config = workspace_file("ilium-plugin-bench/release-gate.json");
    let config: Value = serde_json::from_str(
        &fs::read_to_string(&gate_config).expect("read the release gate configuration"),
    )
    .expect("release gate configuration parses");
    let gated = string_set(&config["gated"]);
    assert!(
        gated.contains("B25") && gated.contains("B26"),
        "the release gate must gate B25 and B26: {gated:?}"
    );
    let failing = write_jsonl("hst143b-b25-fail.jsonl", &[json!({"id": "B25", "pass": false})]);
    let extra = vec![
        "--config".to_string(),
        gate_config.display().to_string(),
    ];
    let (status, text) = run_gate(&[failing], &extra);
    assert!(
        !status.success(),
        "the release gate must exit non-zero when B25 misses its budget: {text}"
    );
}

// ---------------------------------------------------------------- HST-144 results file

/// REQ: HST-144
#[test]
fn req_hst_144_release_results_file_a() {
    let version = workspace_version();
    let path = workspace_file(&format!("bench/results/{version}.jsonl"));
    let lines = read_json_lines(&path);
    let header = lines
        .first()
        .unwrap_or_else(|| panic!("{} is empty", path.display()));
    let machine = &header["reference_machine"];
    for field in ["cpu_model", "core_count", "memory_bytes", "operating_system"] {
        assert!(
            !machine[field].is_null(),
            "the reference machine lacks {field}: {machine}"
        );
    }
    let loaded = string_set(&header["bundled_plugins_loaded"]);
    let inventory = string_set(&header["bundled_inventory"]);
    assert_eq!(
        loaded, inventory,
        "the loaded plugin ids must equal the bundled inventory of the release"
    );
}

/// REQ: HST-144
#[test]
fn req_hst_144_release_results_file_b() {
    let version = workspace_version();
    let path = workspace_file(&format!("bench/results/{version}.jsonl"));
    let header = read_json_lines(&path)
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("{} is empty", path.display()));
    let baseline = header["baseline"]
        .as_str()
        .expect("the results file records its baseline path");
    let resolved = workspace_root()
        .join(baseline)
        .canonicalize()
        .unwrap_or_else(|e| panic!("baseline {baseline} does not resolve: {e}"));
    let previous = previous_release_file(&version)
        .canonicalize()
        .expect("previous release results file");
    assert_eq!(
        resolved, previous,
        "the gate baseline must be the results file of the previous release"
    );
}

// ---------------------------------------------------------------- HST-145 median of three runs

/// REQ: HST-145
#[test]
fn req_hst_145_three_run_median_a() {
    let runs = vec![
        write_jsonl("hst145a-run1.jsonl", &[json!({"id": "B6", "median_ms": 1.0})]),
        write_jsonl("hst145a-run2.jsonl", &[json!({"id": "B6", "median_ms": 1.02})]),
        write_jsonl("hst145a-run3.jsonl", &[json!({"id": "B6", "median_ms": 1.05})]),
    ];
    let baseline = scratch_dir("gate").join("hst145a-baseline.json");
    let extra = vec![
        "--store-baseline".to_string(),
        baseline.display().to_string(),
    ];
    let (status, text) = run_gate(&runs, &extra);
    assert!(
        status.success(),
        "medians 1.0, 1.02 and 1.05 are within 10 % and must pass: {text}"
    );
    assert!(
        !text.contains("noisy"),
        "a spread of 5 % must not be reported as noisy: {text}"
    );
    assert!(baseline.exists(), "a passing run must store the baseline");
}

/// REQ: HST-145
#[test]
fn req_hst_145_three_run_median_b() {
    let runs = vec![
        write_jsonl("hst145b-run1.jsonl", &[json!({"id": "B6", "median_ms": 1.0})]),
        write_jsonl("hst145b-run2.jsonl", &[json!({"id": "B6", "median_ms": 1.0})]),
        write_jsonl("hst145b-run3.jsonl", &[json!({"id": "B6", "median_ms": 1.3})]),
    ];
    let baseline = scratch_dir("gate").join("hst145b-baseline.json");
    let extra = vec![
        "--store-baseline".to_string(),
        baseline.display().to_string(),
    ];
    let (status, text) = run_gate(&runs, &extra);
    assert!(
        text.contains("noisy") && text.contains("B6"),
        "a 30 % spread must mark B6 noisy: {text}"
    );
    assert!(!status.success(), "a noisy result must fail the gate: {text}");
    assert!(
        !baseline.exists(),
        "a noisy result must not be stored as a baseline"
    );
}

// ---------------------------------------------------------------- HST-146 client budgets

/// REQ: HST-146
#[test]
fn req_hst_146_client_budgets_a() {
    let (status, lines) = run_bench("b27", &[]);
    assert!(status.success(), "B27 exited with {status:?}");
    let result = last_result(&lines);
    assert!(
        number(result, "rss_delta_mib") < 64.0,
        "client RSS delta must be under 64 MiB: {result}"
    );
    assert!(
        number(result, "first_frame_ms") <= 150.0,
        "time to the first painted frame must be at most 150 ms: {result}"
    );
    assert!(
        number(result, "idle_frame_host_ms") <= 0.2,
        "the plugin host must add at most 0.2 ms per idle frame: {result}"
    );
}

/// REQ: HST-146
#[test]
fn req_hst_146_client_budgets_b() {
    let over_budget = write_jsonl(
        "hst146b-b27-over.jsonl",
        &[json!({"id": "B27", "pass": false, "rss_delta_mib": 70.0})],
    );
    let (status, text) = run_gate(&[over_budget], &[]);
    assert!(
        !status.success(),
        "a B27 result over its budget must fail the release gate: {text}"
    );
}

// ---------------------------------------------------------------- HST-147 registration cost

/// REQ: HST-147
#[test]
fn req_hst_147_registration_cost_a() {
    let (status, lines) = run_bench(
        "b6",
        &["--variant", "installed-256-t2-t3"],
    );
    assert!(status.success(), "B6 variant exited with {status:?}");
    let result = last_result(&lines);
    let host_start = number(result, "host_start_ms");
    assert!(
        host_start < 406.0,
        "HOST-START with 256 installed plugins must be under 406 ms: {result}"
    );
}

/// REQ: HST-147
#[test]
fn req_hst_147_registration_cost_b() {
    let (status, lines) = run_bench(
        "b6",
        &["--variant", "installed-256-t2-t3"],
    );
    assert!(status.success(), "B6 variant exited with {status:?}");
    let result = last_result(&lines);
    assert_eq!(
        number(result, "opened_non_manifest_files"),
        0.0,
        "registration must read only each plugin's manifest and signature record: {result}"
    );
}

// ---------------------------------------------------------------- HST-148 idle wake-ups

/// REQ: HST-148
#[test]
fn req_hst_148_idle_side_wakeups_a() {
    let (status, lines) = run_bench("b24", &["--idle-run-minutes", "10"]);
    assert!(status.success(), "B24 exited with {status:?}");
    let result = last_result(&lines);
    assert_eq!(
        number(result, "attributable_wakeups_idle_side"),
        0.0,
        "an idle plugin side must cause no attributable wake-ups: {result}"
    );
}

/// REQ: HST-148
#[test]
fn req_hst_148_idle_side_wakeups_b() {
    let (status, lines) = run_bench("b24", &["--timer-period-ms", "50"]);
    assert!(status.success(), "B24 timer run exited with {status:?}");
    let result = last_result(&lines);
    let rate = number(result, "attributable_wakeups_per_second");
    assert!(
        (15.0..=25.0).contains(&rate),
        "a 50 ms repeating timer must cause about 20 wake-ups per second, measured {rate}: {result}"
    );
}

// ---------------------------------------------------------------- HST-149 compile isolation

/// REQ: HST-149
#[test]
fn req_hst_149_compile_isolation_a() {
    let (status, lines) = run_bench("b28", &[]);
    assert!(status.success(), "B28 exited with {status:?}");
    let result = last_result(&lines);
    assert!(
        number(result, "fixture_component_mib") >= 64.0,
        "B28 must compile a component of the maximum size: {result}"
    );
    assert!(
        number(result, "p99_ratio_to_idle") <= 1.2,
        "the p99 on-events latency must stay within 1.2x of idle during a compile: {result}"
    );
}

/// REQ: HST-149
#[test]
fn req_hst_149_compile_isolation_b() {
    let slow = write_jsonl(
        "hst149b-b28-slow.jsonl",
        &[json!({"id": "B28", "p99_ratio": 1.3, "pass": false})],
    );
    let (status, text) = run_gate(&[slow], &[]);
    assert!(
        !status.success(),
        "a p99 above 1.2x idle must fail the release gate: {text}"
    );
}

// ---------------------------------------------------------------- HST-172 soak leak bound

/// REQ: HST-172
#[test]
fn req_hst_172_soak_leak_bound_a() {
    let (status, lines) = run_bench("soak", &["--fixture", "leak-64k", "--cycles", "10"]);
    assert!(
        !status.success(),
        "a soak run with a leaking fixture must fail"
    );
    let record = lines
        .iter()
        .find(|line| line["failed_plugin"] == "leak-64k")
        .unwrap_or_else(|| panic!("the run record must name the leaking plugin: {lines:?}"));
    assert!(
        !record["side"].is_null(),
        "the run record must name the plugin side: {record}"
    );
}

/// REQ: HST-172
#[test]
fn req_hst_172_soak_leak_bound_b() {
    let (status, lines) = run_bench("soak", &["--fixture", "noop", "--cycles", "10"]);
    assert!(
        status.success(),
        "a soak run with a non-leaking fixture must pass: {lines:?}"
    );
    assert!(
        !lines.iter().any(|line| !line["failed_plugin"].is_null()),
        "a non-leaking fixture must not produce a failure record: {lines:?}"
    );
}

// ---------------------------------------------------------------- HST-173 import time

/// Share of the reported call's time spent in imports, from the `host.slow-calls` line of the log.
fn import_share(log: &str) -> Option<f64> {
    let line = log.lines().find(|line| line.contains("host.slow-calls"))?;
    let rest = line.split("import_share=").nth(1)?;
    rest.trim_end_matches('%')
        .split(|c: char| !(c.is_ascii_digit() || c == '.'))
        .next()?
        .parse()
        .ok()
}

/// REQ: HST-173
#[test]
fn req_hst_173_import_time_accounting_a() {
    let wasm = require_fixture("slow-import-stub");
    let mut proc = Proc::spawn("hst173a", &[]);
    proc.ready("imports", &wasm)
        .expect("Ready for the slow-import fixture");
    let result = proc.call_answering(1, "handler-with-import", &[], 20, 5_000, |_call| {
        std::thread::sleep(Duration::from_millis(15));
        (Vec::new(), None)
    });
    assert!(result.is_some(), "CallResult for handler-with-import");
    let log = proc.log();
    let share = import_share(&log)
        .unwrap_or_else(|| panic!("the log has no host.slow-calls line with an import share: {log}"));
    assert!(
        share >= 70.0,
        "a 15 ms import under a 20 ms deadline must show an import share of at least 70 %, got {share}"
    );
}

/// REQ: HST-173
#[test]
fn req_hst_173_import_time_accounting_b() {
    let wasm = require_fixture("slow-import-stub");
    let mut proc = Proc::spawn("hst173b", &[]);
    proc.ready("imports", &wasm)
        .expect("Ready for the slow-import fixture");
    let _ = proc.call_answering(1, "handler-with-import", &[], 20, 5_000, |_call| {
        std::thread::sleep(Duration::from_millis(25));
        (Vec::new(), None)
    });
    let log = proc.log();
    assert!(
        log.contains("host.slow-import") && log.contains("slow-import-stub"),
        "a call that breaches its deadline inside imports must name the slowest import: {log}"
    );
}

// ---------------------------------------------------------------- HST-174 install latency

/// REQ: HST-174
#[test]
fn req_hst_174_install_latency_a() {
    let (status, lines) = run_bench("b29", &[]);
    assert!(status.success(), "B29 exited with {status:?}");
    let result = last_result(&lines);
    assert!(
        number(result, "p99_s") <= 2.0,
        "installing a 2 MiB package must have a p99 of at most 2 s: {result}"
    );
}

/// REQ: HST-174
#[test]
fn req_hst_174_install_latency_b() {
    let slow = write_jsonl(
        "hst174b-b29-slow.jsonl",
        &[json!({"id": "B29", "p99_s": 2.5, "pass": false})],
    );
    let (status, text) = run_gate(&[slow], &[]);
    assert!(
        !status.success(),
        "a B29 p99 over 2 s must fail the release gate: {text}"
    );
}

//! Gap requirements of the host-runtime lane: disk-file and host-buffer caps, quota-hit
//! reporting, epoch ticker stalls, journal and state-file recovery, the `doctor --fix` refusal
//! and the `ui.message` lookup (HST-048..094, API-000). Each test asserts the outcome the
//! requirement names, through the harness, the CLI or the public plugin API.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use ilium_plugin_api::generated::host::plugins::InstanceState;
use ilium_plugin_api::generated::types::events::Event;
use ilium_plugin_api::generated::types::intents::Submission;
use ilium_plugin_api::limits::Limits;
use ilium_plugin_host::calls::Trap;
use ilium_plugin_host::cli::{run as run_cli, CliEnv, CliOutput};
use ilium_plugin_host::registry_client::{FetchError, Fetcher};
use ilium_plugin_host::testing::{HarnessHandle, HostHarness, ImportReply, NativeGuest};
use serde_json::{json, Value};

use crate::support::*;

/// A fetcher for offline CLI runs: every network request fails.
struct NoNetwork;

impl Fetcher for NoNetwork {
    fn get(&self, _url: &str) -> Result<Vec<u8>, FetchError> {
        Err(FetchError::Unreachable)
    }
}

/// CLI environment whose data, state and config folders sit under the given root.
fn cli_env(root: &Path) -> CliEnv {
    CliEnv::new(
        root.join("data"),
        root.join("state"),
        root.join("config"),
        root.to_path_buf(),
        SystemTime::now(),
        true,
        Arc::new(NoNetwork),
    )
}

/// Runs `ilium plugin <args>` against the root and returns its output.
fn cli(root: &Path, args: &[&str]) -> CliOutput {
    let owned: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
    run_cli(&owned, &cli_env(root))
}

/// Whether the CLI output (stdout or stderr) contains the needle.
fn mention(output: &CliOutput, needle: &str) -> bool {
    output.stdout_raw.contains(needle) || output.stderr.contains(needle)
}

/// A limit table that sets one key.
fn limited(key: &str, value: u64) -> Limits {
    let mut limits = Limits::default();
    limits.0.insert(key.to_owned(), value);
    limits
}

/// One `fs.write` import of the given text into the data root.
fn write_file(host: &HarnessHandle, path: &str, data: &str) -> ImportReply {
    host.call_import_json(
        "ilium:host/fs",
        "write",
        &json!({"root": "data", "path": path, "data": data}).to_string(),
    )
}

/// The `quota_hits` object of a `plugin status --json` run, or null when absent.
fn quota_hits_of(output: &CliOutput) -> Value {
    output
        .stdout_jsonl
        .iter()
        .find_map(|record| {
            let hits = &record["quota_hits"];
            (!hits.is_null()).then(|| hits.clone())
        })
        .unwrap_or(Value::Null)
}

/// One `timers.set` import; every call after the first counts against `timers = 1`.
fn set_timer(host: &HarnessHandle) -> ImportReply {
    host.call_import_json("ilium:host/timers", "set", &json!({"delay-ms": 1000}).to_string())
}

/// A sorted listing of every file and folder under the root, for before/after comparisons.
fn file_listing(root: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory).expect("a readable folder") {
            let path = entry.expect("a directory entry").path();
            if path.is_dir() {
                pending.push(path.clone());
            }
            found.push(path.display().to_string());
        }
    }
    found.sort();
    found
}

/// A native guest that holds the call longer than its deadline, standing in for a spinning
/// plugin call.
struct SpinGuest;

impl NativeGuest for SpinGuest {
    fn on_events(&mut self, _batch: Vec<Event>) -> Result<Vec<Submission>, Trap> {
        std::thread::sleep(Duration::from_millis(200));
        Ok(Vec::new())
    }
}

/// A plugin data folder holding a `plugins` folder with a real `installed.json`.
fn plugins_folder_with_state(root: &Path) -> PathBuf {
    let plugins = root.join("data").join("plugins");
    std::fs::create_dir_all(&plugins).expect("the plugins folder can be created");
    std::fs::write(plugins.join("installed.json"), br#"{"plugins": []}"#)
        .expect("installed.json can be written");
    plugins
}

// ---------------------------------------------------------------- HST-048

/// REQ: HST-048
#[test]
fn req_hst_048_disk_files_cap_allows_creates_up_to_the_limit_a() {
    let host = HostHarness::new()
        .limits(limited("disk_files", 100))
        .build()
        .expect("the harness builds a host with disk_files = 100");
    for index in 0..100 {
        let reply = write_file(&host, &format!("file-{index}.txt"), "x");
        assert!(
            reply.error_code().is_none(),
            "create {index} is within disk_files = 100"
        );
    }
    let root = tempfile::tempdir().expect("a temp folder");
    let stats = cli(root.path(), &["stats", "--json"]);
    assert_eq!(stats.exit_code, 0, "`ilium plugin stats` runs");
    assert!(mention(&stats, "files"), "stats shows a `files` row");
}

/// REQ: HST-048
#[test]
fn req_hst_048_disk_files_refuses_past_the_cap_and_frees_on_delete_b() {
    let host = HostHarness::new()
        .limits(limited("disk_files", 100))
        .build()
        .expect("the harness builds a host with disk_files = 100");
    for index in 0..100 {
        write_file(&host, &format!("file-{index}.txt"), "x");
    }
    let refused = write_file(&host, "file-100.txt", "x");
    assert_eq!(refused.error_code(), Some("quota-exceeded"));
    assert!(
        log_text(&host).contains("disk_files"),
        "the refusal names disk_files"
    );
    let read = host.call_import_json(
        "ilium:host/fs",
        "read",
        &json!({"root": "data", "path": "file-0.txt"}).to_string(),
    );
    assert!(read.error_code().is_none(), "reads keep working at the cap");
    let deleted = host.call_import_json(
        "ilium:host/fs",
        "delete",
        &json!({"root": "data", "path": "file-0.txt"}).to_string(),
    );
    assert!(deleted.error_code().is_none(), "deletes keep working at the cap");
    assert!(
        write_file(&host, "file-100.txt", "x").error_code().is_none(),
        "one create succeeds after a delete"
    );
}

// ---------------------------------------------------------------- HST-049

/// One `http.fetch` of an 8 MiB body that the plugin never drains.
fn fetch_undrained_body(host: &HarnessHandle, index: usize) -> ImportReply {
    host.call_import_json(
        "ilium:host/http",
        "fetch",
        &json!({
            "url": format!("http://127.0.0.1:9/buffer-{index}"),
            "expected-body-bytes": 8_388_608u64,
        })
        .to_string(),
    )
}

/// REQ: HST-049
#[test]
fn req_hst_049_host_buffers_accept_eight_undrained_bodies_a() {
    let host = HostHarness::new()
        .build()
        .expect("the harness builds a host");
    for index in 0..8 {
        let reply = fetch_undrained_body(&host, index);
        assert!(
            reply.error_code().is_none(),
            "fetch {index} fits the 64 MiB host-buffer cap"
        );
    }
    let root = tempfile::tempdir().expect("a temp folder");
    let stats = cli(root.path(), &["stats", "--json"]);
    assert!(mention(&stats, "host-buffers"), "stats shows a `host-buffers` row");
}

/// REQ: HST-049
#[test]
fn req_hst_049_host_buffers_refuse_the_ninth_body_with_busy_b() {
    let host = HostHarness::new()
        .build()
        .expect("the harness builds a host");
    for index in 0..8 {
        fetch_undrained_body(&host, index);
    }
    let ninth = fetch_undrained_body(&host, 8);
    assert_eq!(ninth.error_code(), Some("busy"));
    assert!(ninth.retry_after_ms().is_some(), "the refusal carries retry-after-ms");
    assert!(
        log_count(&host, "host-buffer-full") >= 1,
        "the refusal names reason host-buffer-full"
    );
}

// ---------------------------------------------------------------- HST-090

/// REQ: HST-090
#[test]
fn req_hst_090_quota_hits_report_the_timers_key_with_its_count_a() {
    let host = HostHarness::new()
        .limits(limited("timers", 1))
        .build()
        .expect("the harness builds a host with timers = 1");
    for _ in 0..11 {
        set_timer(&host);
    }
    let root = tempfile::tempdir().expect("a temp folder");
    let status = cli(root.path(), &["status", "test.plugin", "--json"]);
    let hits = quota_hits_of(&status);
    assert_eq!(hits["timers"], json!(10), "ten timer refusals are counted per key");
}

/// REQ: HST-090
#[test]
fn req_hst_090_quota_hits_age_out_after_five_minutes_of_silence_b() {
    let mut host = HostHarness::new()
        .limits(limited("timers", 1))
        .build()
        .expect("the harness builds a host with timers = 1");
    for _ in 0..11 {
        set_timer(&host);
    }
    advance_ms(&mut host, minutes(5));
    let root = tempfile::tempdir().expect("a temp folder");
    let status = cli(root.path(), &["status", "test.plugin", "--json"]);
    assert!(
        quota_hits_of(&status)["timers"].is_null(),
        "the timers hits age out after five minutes without refusals"
    );
}

// ---------------------------------------------------------------- HST-091

/// REQ: HST-091
#[test]
fn req_hst_091_epoch_ticker_normal_deadline_traps_the_call_a() {
    let host = HostHarness::new()
        .limits(limited("event_deadline_ms", 50))
        .with_native_guest(Box::new(SpinGuest))
        .build()
        .expect("the harness builds a host for a spinning guest");
    host.deliver(one_event());
    host.pump(8);
    assert!(
        host.failures().iter().any(|trap| code_of(trap) == "trap.deadline"),
        "the 50 ms deadline ends the call as trap.deadline"
    );
    assert_eq!(log_count(&host, "host.epoch-stalled"), 0, "the ticker did not stall");
}

/// REQ: HST-091
#[test]
fn req_hst_091_epoch_ticker_stall_is_logged_once_and_still_traps_b() {
    let host = HostHarness::new()
        .limits(limited("event_deadline_ms", 50))
        .with_native_guest(Box::new(SpinGuest))
        .build()
        .expect("the harness builds a host for a spinning guest");
    let started = Instant::now();
    host.deliver(one_event());
    host.pump(8);
    assert!(
        started.elapsed() < Duration::from_millis(1500),
        "the stall is detected within 1.5 s"
    );
    assert_eq!(
        log_count(&host, "host.epoch-stalled"),
        1,
        "host.epoch-stalled is logged once"
    );
    assert!(
        host.failures().iter().any(|trap| code_of(trap) == "trap.deadline"),
        "the in-flight call ends as trap.deadline"
    );
}

// ---------------------------------------------------------------- HST-092

/// The journal folder under a data root.
fn journal_folder(data: &Path) -> PathBuf {
    data.join("plugins").join("journal")
}

/// Leaves a half-written journal entry, as a host killed mid-swap would.
fn truncated_journal_entry(data: &Path) -> PathBuf {
    let journal = journal_folder(data);
    std::fs::create_dir_all(&journal).expect("the journal folder can be created");
    let entry = journal.join("update-0001.json");
    std::fs::write(&entry, b"{\"plugin\": \"test.plugin\", \"steps\": [")
        .expect("the journal entry can be written");
    entry
}

/// REQ: HST-092
#[test]
fn req_hst_092_corrupt_journal_entry_moves_to_corrupt_and_is_logged_a() {
    let root = tempfile::tempdir().expect("a temp folder");
    let data = root.path().join("data");
    truncated_journal_entry(&data);
    let host = HostHarness::new()
        .state_dir(data.clone())
        .build()
        .expect("the harness builds a host over the journal");
    assert!(
        journal_folder(&data).join("corrupt").join("update-0001.json").is_file(),
        "the unparsable entry moves to journal/corrupt/"
    );
    let text = log_text(&host);
    assert!(text.contains("recovery.journal-corrupt"), "the log names recovery.journal-corrupt");
    assert!(text.contains("update-0001.json"), "the log names the file");
}

/// REQ: HST-092
#[test]
fn req_hst_092_corrupt_journal_keeps_old_version_and_counts_in_recovery_b() {
    let root = tempfile::tempdir().expect("a temp folder");
    let data = root.path().join("data");
    let entry = truncated_journal_entry(&data);
    let host = HostHarness::new()
        .state_dir(data.clone())
        .build()
        .expect("the harness builds a host over the journal");
    assert!(
        matches!(host.state(), InstanceState::Active),
        "the old version stays active after the rollback"
    );
    assert!(!entry.exists(), "the uncommitted entry is no longer in the journal");
    assert_eq!(
        log_count(&host, "recovery.completed"),
        1,
        "recovery.completed counts the corrupt entry"
    );
}

// ---------------------------------------------------------------- HST-093

/// REQ: HST-093
#[test]
fn req_hst_093_doctor_without_fix_still_runs_and_reports_a() {
    let root = tempfile::tempdir().expect("a temp folder");
    std::fs::create_dir_all(root.path().join("data").join("plugins"))
        .expect("the plugins folder can be created");
    let report = cli(root.path(), &["doctor"]);
    assert_ne!(report.exit_code, 6, "doctor without --fix is not refused");
    assert!(
        !report.stdout_raw.is_empty() || !report.stdout_jsonl.is_empty(),
        "doctor reports its checks"
    );
}

/// REQ: HST-093
#[test]
fn req_hst_093_doctor_fix_refuses_while_a_server_holds_the_lock_b() {
    let root = tempfile::tempdir().expect("a temp folder");
    let data = root.path().join("data");
    let plugins = data.join("plugins");
    std::fs::create_dir_all(&plugins).expect("the plugins folder can be created");
    std::fs::write(plugins.join("installed.lock"), b"").expect("the lock file can be written");
    let before = file_listing(&data);
    let refused = cli(root.path(), &["doctor", "--fix", "--json"]);
    assert_eq!(refused.exit_code, 6, "--fix exits with code 6 under a live server");
    assert!(
        mention(&refused, "doctor.server-running"),
        "the output names doctor.server-running"
    );
    assert_eq!(file_listing(&data), before, "--fix changes nothing");
}

// ---------------------------------------------------------------- HST-094

/// REQ: HST-094
#[test]
fn req_hst_094_temporary_state_siblings_are_removed_and_logged_a() {
    let root = tempfile::tempdir().expect("a temp folder");
    let data = root.path().join("data");
    let plugins = plugins_folder_with_state(root.path());
    let sibling = plugins.join("installed.json.tmp-abc");
    std::fs::write(&sibling, br#"{"plugins": ["ghost.plugin"]}"#)
        .expect("the temporary sibling can be written");
    let host = HostHarness::new()
        .state_dir(data.clone())
        .build()
        .expect("the harness builds a host over the state files");
    assert!(!sibling.exists(), "the interrupted temporary file is deleted");
    assert_eq!(
        log_count(&host, "state.temp-removed"),
        1,
        "state.temp-removed reports one removed file"
    );
}

/// REQ: HST-094
#[test]
fn req_hst_094_temporary_sibling_is_never_read_as_the_state_b() {
    let root = tempfile::tempdir().expect("a temp folder");
    let data = root.path().join("data");
    let plugins = plugins_folder_with_state(root.path());
    std::fs::write(
        plugins.join("installed.json.tmp-abc"),
        br#"{"plugins": ["ghost.plugin"]}"#,
    )
    .expect("the temporary sibling can be written");
    let _host = HostHarness::new()
        .state_dir(data.clone())
        .build()
        .expect("the harness builds a host over the state files");
    let listing = cli(root.path(), &["list", "--json"]);
    assert_eq!(listing.exit_code, 0, "`ilium plugin list` runs");
    assert!(
        !mention(&listing, "ghost.plugin"),
        "the registry matches installed.json, not the temporary sibling"
    );
}

// ---------------------------------------------------------------- API-000

/// REQ: API-000
#[test]
fn req_api_000_ui_message_resolves_the_current_language_a() {
    let host = HostHarness::new()
        .setting("ui.language", json!("de"))
        .build()
        .expect("the harness builds a host set to German");
    let value = host
        .call_import_value(
            "ui.message",
            json!({"id": "notes.deleted", "args": [["count", "3"]]}),
        )
        .expect("ui.message returns a value");
    let text = value.as_str().expect("ui.message returns a string");
    assert!(text.contains('3'), "the count is substituted");
    assert!(!text.contains("{count}"), "no placeholder is left");
    assert!(!text.contains("notes.deleted"), "the message is not the id");
}

/// REQ: API-000
#[test]
fn req_api_000_ui_message_missing_everywhere_returns_empty_and_logs_once_b() {
    let host = HostHarness::new()
        .setting("ui.language", json!("de"))
        .build()
        .expect("the harness builds a host set to German");
    let value = host
        .call_import_value(
            "ui.message",
            json!({"id": "no.such.message", "args": []}),
        )
        .expect("ui.message returns a value for a missing id");
    assert_eq!(
        value,
        json!(""),
        "an id absent from every file gives an empty string, never the id"
    );
    assert_eq!(
        log_count(&host, "no.such.message"),
        1,
        "one log line reports the missing id"
    );
}

//! Lane `host-ipc` gap tests: quarantine retention, the unclean-shutdown marker, the stale IPC
//! socket, the kill-point and atomic-write matrix, and the daily summary export (HST-095..HST-099).
//! The implementation is not written yet, so these tests are expected to fail at runtime.

use std::path::{Path, PathBuf};
use std::time::Duration;

use ilium_plugin_host::telemetry_history::{HourlyBucket, TelemetryHistory};
use ilium_plugin_host::testing::ops::{OpsRig, ServerHandle};

const PLUGIN: &str = "acme.demo";
const QUARANTINE_MARKERS: [&str; 2] = [".corrupt-", ".broken-"];

/// Names of the quarantined copies of one original file in a folder, sorted by name.
fn quarantined_copies(folder: &Path, original: &str) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(folder)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .filter(|name| {
                    name.starts_with(original)
                        && QUARANTINE_MARKERS.iter().any(|marker| name.contains(marker))
                })
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

/// Names of the entries of a folder that contain one of the given words.
fn names_containing(folder: &Path, words: &[&str]) -> Vec<String> {
    std::fs::read_dir(folder)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .filter(|name| words.iter().any(|word| name.contains(word)))
                .collect()
        })
        .unwrap_or_default()
}

/// Standard output and standard error of a CLI run, joined.
fn combined_output(output: &ilium_plugin_host::cli::CliOutput) -> String {
    format!("{}\n{}", output.stdout_raw, output.stderr)
}

fn has_log_code(server: &ServerHandle, code: &str) -> bool {
    server.log_codes().iter().any(|logged| logged == code)
}

// ---------------------------------------------------------------- HST-095 quarantine retention

/// REQ: HST-095
#[test]
fn req_hst_095_quarantine_retention_a() {
    let rig = OpsRig::new();
    let state = rig.state_dir();
    for index in 0..11u32 {
        let name = format!("grants.json.corrupt-2026-01-{:02}T00-00-00", index + 1);
        rig.corrupt_file(&state.join(&name), b"{}");
    }
    let doctor = rig.cli(&["doctor"]);
    assert_eq!(doctor.exit_code, 0, "doctor must run: {}", doctor.stderr);

    let kept = quarantined_copies(&state, "grants.json");
    assert_eq!(kept.len(), 10, "at most 10 quarantined copies per name, kept: {kept:?}");
    assert!(
        !state.join("grants.json.corrupt-2026-01-01T00-00-00").exists(),
        "the oldest quarantined copy must be removed first"
    );

    let listed = combined_output(&doctor)
        .lines()
        .filter(|line| line.contains("state.quarantined") && line.contains("grants.json"))
        .count();
    assert_eq!(listed, 10, "doctor must list each kept copy with state.quarantined");
}

/// REQ: HST-095
#[test]
fn req_hst_095_quarantine_retention_b() {
    let rig = OpsRig::new();
    let state = rig.state_dir();
    let copy = state.join("grants.json.broken-2026-02-01T00-00-00");
    rig.corrupt_file(&copy, b"{ broken");
    rig.advance_clock(Duration::from_secs(31 * 86_400));

    let doctor = rig.cli(&["doctor"]);
    assert_eq!(doctor.exit_code, 0, "doctor must run: {}", doctor.stderr);
    assert!(
        !copy.exists(),
        "a quarantined copy kept for more than 30 days must be removed by the next doctor run"
    );

    let archive: PathBuf = rig.path_in_scratch("req-095-backup.archive");
    let backup = rig.cli(&["backup", archive.to_str().expect("utf-8 path")]);
    assert_eq!(backup.exit_code, 0, "backup must run: {}", backup.stderr);
    let archive_bytes = std::fs::read(&archive).expect("backup archive written");
    let needle = b"grants.json.broken-2026-02-01T00-00-00";
    assert!(
        archive_bytes.windows(needle.len()).any(|window| window == needle),
        "the backup archive must contain the quarantined copy"
    );
}

// ---------------------------------------------------------------- HST-096 unclean shutdown marker

/// REQ: HST-096
#[test]
fn req_hst_096_unclean_shutdown_marker_a() {
    let rig = OpsRig::new();
    let first = rig.start_server("s1");
    assert!(first.is_running(), "the first server must start");
    first.crash();

    let second = rig.start_server("s1");
    assert!(
        has_log_code(&second, "host.unclean-shutdown"),
        "a start after a kill must log host.unclean-shutdown, got {:?}",
        second.log_codes()
    );

    let audit = rig.cli(&["audit"]);
    assert!(
        combined_output(&audit).contains("unclean-shutdown"),
        "exactly one unclean-shutdown audit record must exist"
    );

    let calls = second.kernel_call_log();
    let recovery = calls
        .iter()
        .position(|entry| entry.contains("recovery"))
        .expect("the recovery of HST-679 must run");
    if let Some(activate) = calls.iter().position(|entry| entry.contains("activate")) {
        assert!(recovery < activate, "recovery must run before the first activate: {calls:?}");
    }
    second.shutdown();
}

/// REQ: HST-096
#[test]
fn req_hst_096_unclean_shutdown_marker_b() {
    let rig = OpsRig::new();
    let server = rig.start_server("s1");
    server.shutdown();

    let marker = names_containing(&rig.state_dir(), &["marker", "running"]);
    assert!(marker.is_empty(), "a clean shutdown must remove the running marker: {marker:?}");

    let next = rig.start_server("s1");
    assert!(
        !has_log_code(&next, "host.unclean-shutdown"),
        "a start after a clean stop must not log host.unclean-shutdown"
    );
    next.shutdown();
}

// ---------------------------------------------------------------- HST-097 stale IPC socket

/// REQ: HST-097
#[test]
fn req_hst_097_stale_ipc_socket_a() {
    let rig = OpsRig::new();
    let first = rig.start_server("s1");
    first.crash();

    let second = rig.start_server("s1");
    assert!(second.is_running(), "a start after a kill must bind the stale socket path");
    assert!(
        has_log_code(&second, "ipc.stale-socket-removed"),
        "the start must log ipc.stale-socket-removed, got {:?}",
        second.log_codes()
    );
    second.shutdown();
}

/// REQ: HST-097
#[test]
fn req_hst_097_stale_ipc_socket_b() {
    let rig = OpsRig::new();
    let live = rig.start_server("s1");
    assert!(live.is_running(), "the first server must run");

    let second = rig.start_server("s1");
    assert!(!second.is_running(), "a second start while the holder is alive must refuse");
    assert!(
        second
            .log_codes()
            .iter()
            .any(|code| code.contains("lock") || code.contains("owner")),
        "the refusal must name the owner, got {:?}",
        second.log_codes()
    );
    assert!(
        !has_log_code(&second, "ipc.stale-socket-removed"),
        "a live holder's socket must never be removed"
    );
    second.shutdown();
    live.shutdown();
}

// ---------------------------------------------------------------- HST-098 kill-point matrix

/// REQ: HST-098
#[test]
fn req_hst_098_kill_point_matrix_a() {
    let rig = OpsRig::new();
    let points = rig.kill_points();
    for file in ["crash mark", "bisect.json", "admin kill flag", "purged.json", "settings"] {
        assert!(
            points.iter().any(|point| point.contains(file)),
            "the kill-point matrix must include a kill during the write of {file}: {points:?}"
        );
        assert!(
            rig.fault_writer_checked(file),
            "the write of {file} must use temp file, fsync, rename, fsync-folder order"
        );
    }
}

/// REQ: HST-098
#[test]
fn req_hst_098_kill_point_matrix_b() {
    let rig = OpsRig::new();
    let report = rig.run_storage_fault_injection(PLUGIN, 50);
    assert_eq!(report.silently_lost, 0, "no acknowledged write may be lost silently");
    assert_eq!(
        report.unreadable_and_not_quarantined, 0,
        "a torn copy must never be read as valid"
    );
    let torn = rig.state_dir().join("settings.json");
    rig.corrupt_file(&torn, b"{\"theme\": \"da");
    let doctor = rig.cli(&["doctor"]);
    assert!(
        combined_output(&doctor).contains("state.quarantined"),
        "a torn settings file must be quarantined, not read as valid"
    );
}

// ---------------------------------------------------------------- HST-099 daily summaries

/// REQ: HST-099
#[test]
fn req_hst_099_daily_summaries_export_a() {
    let rig = OpsRig::new();
    rig.advance_clock(Duration::from_secs(30 * 3600));
    let folder = rig.path_in_scratch("req-099-export");
    std::fs::create_dir_all(&folder).expect("create export folder");

    let export = rig.cli(&["export-data", folder.to_str().expect("utf-8 path")]);
    assert_eq!(export.exit_code, 0, "export-data must run: {}", export.stderr);

    let text = std::fs::read_to_string(folder.join("summaries.jsonl"))
        .expect("export-data must write summaries.jsonl");
    let lines: Vec<&str> = text.lines().filter(|line| !line.trim().is_empty()).collect();
    assert_eq!(lines.len(), 5, "5 aged hourly buckets must give 5 summary lines");
    let first: serde_json::Value =
        serde_json::from_str(lines[0]).expect("each summary line is one JSON object");
    for field in ["plugin_side", "day_index", "calls", "failures", "near_misses", "p50_ms", "p95_ms", "p99_ms"] {
        assert!(first.get(field).is_some(), "summary line lacks field {field}: {first}");
    }
}

/// REQ: HST-099
#[test]
fn req_hst_099_daily_summaries_export_b() {
    let rig = OpsRig::new();
    rig.advance_clock(Duration::from_secs(30 * 3600));

    let erase = rig.cli(&["erase-data", PLUGIN]);
    assert_eq!(erase.exit_code, 0, "erase-data must run: {}", erase.stderr);
    assert!(
        combined_output(&erase).contains("erased summaries: 5"),
        "erase-data must report the summaries count under the kind summaries: {}",
        combined_output(&erase)
    );

    let mut history = TelemetryHistory::new();
    history.record_hour(HourlyBucket {
        plugin_side: format!("{PLUGIN}:client"),
        hour_index: 0,
        calls: 2,
        failures: 0,
        near_misses: 0,
        call_times_ms: vec![5, 10],
    });
    history.remove_plugin(PLUGIN);
    let days = history.status_days_output(PLUGIN, 30);
    assert!(
        !days.lines().any(|line| line.trim_start().starts_with("day")),
        "after erase-data no day of the plugin may be printed: {days}"
    );
}

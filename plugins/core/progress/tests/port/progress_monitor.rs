//! The probe contract as the monitor enforces it (original ilium-server/src/progress_monitor.rs).

use ilium_plugin_sdk::types::{ErrorCode, PluginError};
use serde_json::{Value, json};

use crate::common::*;
use crate::support::*;

fn check(host: &mut ilium_plugin_sdk::testing::FakeHost, pane: u64, stdout: &str) -> Run {
    expect_probe(host, PROBE, stdout);
    progress(host, pane, &["check", "--command", PROBE])
}

fn raw(job: &str, status: &str, percent: f64, message: &str, details: &str, error: Value) -> String {
    json!({"job_id": job, "status": status, "percent": percent, "message": message, "details": details, "error": error}).to_string()
}

/// REQ: PORT:ilium-server/src/progress_monitor.rs::cancelling_probe_future_terminates_descendant_processes
#[test]
fn port_059_clearing_a_monitor_while_its_probe_is_due_stops_all_further_probe_runs() {
    let (mut host, pane) = host_with_pane();
    register(&mut host, pane, PROBE, &running(JOB, 10, "W"), &[]);
    progress(&mut host, pane, &["clear", "--all"]);
    let runs = probe_runs(&mut host);
    host.process().allow_unexpected();
    tick(&mut host, 10);
    assert_eq!(probe_runs(&mut host), runs);
}

/// REQ: PORT:ilium-server/src/progress_monitor.rs::identity_and_status_regressions_are_observation_failures
#[test]
fn port_059_a_changed_job_id_and_a_status_regression_are_observation_failures() {
    let (mut host, pane) = host_with_pane();
    let id = register(&mut host, pane, PROBE, &running("job", 30, "Thirty"), &[]);
    expect_probe(&mut host, PROBE, &running("other-job", 31, "Other"));
    tick(&mut host, 1);
    assert_eq!(health_of(&status(&mut host, pane), id), "degraded");
    expect_probe(&mut host, PROBE, &raw("job", "not-started-yet", 0.0, "m", "", Value::Null));
    tick(&mut host, 1);
    let record = status(&mut host, pane);
    assert_eq!(record["monitors"][0]["report"]["job_id"], "job");
    assert_eq!(record["monitors"][0]["report"]["status"], "running");
}

/// REQ: PORT:ilium-server/src/progress_monitor.rs::monitor_fences_are_independent_per_monitor
#[test]
fn port_059_clearing_one_monitor_leaves_the_other_polling_with_its_own_nonzero_id() {
    let (mut host, pane) = host_with_pane();
    let first = register(&mut host, pane, "/opt/a.sh", &running("job-a", 0, "A"), &[]);
    let second = register(&mut host, pane, "/opt/b.sh", &running("job-b", 0, "B"), &[]);
    assert!(first != 0 && second != 0 && first != second);
    progress(&mut host, pane, &["clear", "--monitor-id", &first.to_string()]);
    expect_probe(&mut host, "/opt/b.sh", &running("job-b", 50, "B halfway"));
    tick(&mut host, 1);
    let record = status(&mut host, pane);
    assert_eq!(record["monitor_count"], 1);
    assert_eq!(record["monitors"][0]["monitor_id"], second);
    assert_eq!(record["monitors"][0]["report"]["percent"], 50.0);
}

/// REQ: PORT:ilium-server/src/progress_monitor.rs::preflight_accepts_all_exact_wire_statuses_and_normalizes_done
#[test]
fn port_059_check_accepts_every_wire_status_and_normalizes_done_to_100_percent() {
    let (mut host, pane) = host_with_pane();
    for (status_name, error) in [("not-started-yet", Value::Null), ("running", Value::Null), ("error", json!("boom"))] {
        let run = check(&mut host, pane, &raw("job", status_name, 40.0, "m", "", error));
        assert_eq!(record_of(&run.stdout, "progress_check")["report"]["status"], status_name);
    }
    let run = check(&mut host, pane, &raw("job", "done", 40.0, "m", "", Value::Null));
    assert_eq!(record_of(&run.stdout, "progress_check")["report"]["percent"].as_f64(), Some(100.0));
}

/// REQ: PORT:ilium-server/src/progress_monitor.rs::preflight_timestamps_successful_observation_after_probe_execution
#[test]
fn port_059_check_timestamps_the_observation_when_the_probe_has_run() {
    let (mut host, pane) = host_with_pane();
    let first = check(&mut host, pane, &running("job", 1, "W"));
    let first_at = record_of(&first.stdout, "progress_check")["checked_at_unix_millis"].as_u64().expect("timestamp");
    pass(&mut host, 5_000);
    let second = check(&mut host, pane, &running("job", 1, "W"));
    let second_at = record_of(&second.stdout, "progress_check")["checked_at_unix_millis"].as_u64().expect("timestamp");
    assert!(second_at >= first_at + 5_000, "{first_at} then {second_at}");
}

/// REQ: PORT:ilium-server/src/progress_monitor.rs::probe_bounds_stdout_and_execution_time
#[test]
fn port_059_probe_output_over_64_kib_is_rejected_probe_output_too_large() {
    let (mut host, pane) = host_with_pane();
    let base = running("job", 1, "W");
    let padded = format!("{base}{}", " ".repeat(64 * 1024 + 1 - base.len()));
    assert_eq!(padded.len(), 64 * 1024 + 1);
    assert_eq!(code(&check(&mut host, pane, &padded)), "probe-output-too-large");
    check(&mut host, pane, &base);
    assert_eq!(host.process().calls().last().expect("call").timeout_millis, 30_000);
}

/// REQ: PORT:ilium-server/src/progress_monitor.rs::probe_timeout_terminates_descendant_processes
#[test]
fn port_059_a_probe_that_exceeds_its_time_is_rejected_probe_timed_out() {
    let (mut host, pane) = host_with_pane();
    host.process().expect("sh", &["-c", PROBE]).fails(PluginError::new(ErrorCode::TimedOut, "timed out"));
    let run = progress(&mut host, pane, &["check", "--command", PROBE]);
    assert_eq!(code(&run), "probe-timed-out");
}

/// REQ: PORT:ilium-server/src/progress_monitor.rs::probe_carries_multi_line_details_alongside_the_compact_message
#[test]
fn port_059_multi_line_details_travel_next_to_a_single_line_message() {
    let (mut host, pane) = host_with_pane();
    let details = "What: build\nWhy: release\nNext: install";
    let run = check(&mut host, pane, &raw("job", "running", 10.0, "Building", details, Value::Null));
    let record = record_of(&run.stdout, "progress_check");
    assert_eq!(record["report"]["message"], "Building");
    assert_eq!(record["report"]["details"], details);
}

/// REQ: PORT:ilium-server/src/progress_monitor.rs::probe_rejects_trailing_json_unknown_fields_and_invalid_contracts
#[test]
fn port_059_trailing_json_unknown_fields_and_invalid_values_are_rejected_invalid_probe_report() {
    let (mut host, pane) = host_with_pane();
    let ok = running("job", 1, "W");
    let cases = [
        format!("{ok} {ok}"),
        r#"{"job_id":"j","status":"running","percent":1,"message":"m","details":"","error":null,"extra":1}"#.to_string(),
        raw("job", "error", 5.0, "m", "", Value::Null),
        raw("job", "running", 101.0, "m", "", Value::Null),
        raw("job", "running", 5.0, "two\nlines", "", Value::Null),
    ];
    for case in cases {
        assert_eq!(code(&check(&mut host, pane, &case)), "invalid-probe-report", "{case:?}");
    }
}

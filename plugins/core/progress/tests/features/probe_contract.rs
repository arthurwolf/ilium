//! The probe contract (F-059, progress-B07, progress-B08, progress-B09, progress-B15): the
//! process that is run, the limits on its output and the report fields, and the failure ladder.

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{ErrorCode, PluginError};
use serde_json::{json, Value};

use crate::common::*;
use crate::feature_support::*;

const PROBE: &str = "/opt/probe.sh";

fn check(host: &mut FakeHost, pane: u64, stdout: &str) -> Run {
    expect_probe(host, PROBE, stdout);
    progress(host, pane, &["check", "--command", PROBE])
}

fn code(run: &Run) -> String {
    record_of(&run.stdout, "progress_rejected")["code"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

fn report(
    job: &str,
    status: &str,
    percent: f64,
    message: &str,
    details: &str,
    error: Value,
) -> String {
    json!({"job_id": job, "status": status, "percent": percent, "message": message, "details": details, "error": error}).to_string()
}

fn ok_with(message: &str, details: &str) -> String {
    report("job", "running", 10.0, message, details, Value::Null)
}

/// REQ: progress-B08, F-059
#[test]
fn progress_b08_the_probe_runs_as_sh_dash_c_command() {
    let (mut host, pane) = host_with_pane();
    check(&mut host, pane, &running("job", 1, "W"));
    let calls = host.process().calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].program, "sh");
    assert_eq!(calls[0].arguments, ["-c", PROBE]);
}

/// REQ: progress-B08
#[test]
fn progress_b08_the_probe_has_a_thirty_second_timeout_and_no_stdin() {
    let (mut host, pane) = host_with_pane();
    check(&mut host, pane, &running("job", 1, "W"));
    let call = &host.process().calls()[0];
    assert_eq!(call.timeout_millis, 30_000);
    assert!(call.stdin.is_none());
}

/// REQ: progress-B08
#[test]
fn progress_b08_the_probe_runs_in_a_project_directory() {
    let (mut host, pane) = host_with_pane();
    check(&mut host, pane, &running("job", 1, "W"));
    assert!(host.process().calls()[0].cwd.is_some());
}

/// REQ: progress-B07
#[test]
fn progress_b07_job_id_of_256_bytes_is_accepted_and_257_is_rejected() {
    let (mut host, pane) = host_with_pane();
    let at = check(&mut host, pane, &running(&"j".repeat(256), 1, "W"));
    assert_eq!(at.exit, Some(0));
    let over = check(&mut host, pane, &running(&"j".repeat(257), 1, "W"));
    assert_eq!(code(&over), "invalid-probe-report");
}

/// REQ: progress-B07
#[test]
fn progress_b07_message_of_2048_bytes_is_accepted_and_2049_is_rejected() {
    let (mut host, pane) = host_with_pane();
    assert_eq!(
        check(&mut host, pane, &ok_with(&"m".repeat(2048), "")).exit,
        Some(0)
    );
    assert_eq!(
        code(&check(&mut host, pane, &ok_with(&"m".repeat(2049), ""))),
        "invalid-probe-report"
    );
}

/// REQ: progress-B07
#[test]
fn progress_b07_details_of_8192_bytes_is_accepted_and_8193_is_rejected() {
    let (mut host, pane) = host_with_pane();
    assert_eq!(
        check(&mut host, pane, &ok_with("m", &"d".repeat(8192))).exit,
        Some(0)
    );
    assert_eq!(
        code(&check(&mut host, pane, &ok_with("m", &"d".repeat(8193)))),
        "invalid-probe-report"
    );
}

/// REQ: progress-B07
#[test]
fn progress_b07_error_of_4096_bytes_is_accepted_and_4097_is_rejected() {
    let (mut host, pane) = host_with_pane();
    let at = report("job", "error", 5.0, "m", "", json!("e".repeat(4096)));
    assert_eq!(check(&mut host, pane, &at).exit, Some(0));
    let over = report("job", "error", 5.0, "m", "", json!("e".repeat(4097)));
    assert_eq!(code(&check(&mut host, pane, &over)), "invalid-probe-report");
}

/// REQ: progress-B07
#[test]
fn progress_b07_stdout_of_64_kib_is_accepted_and_one_more_byte_is_too_large() {
    let (mut host, pane) = host_with_pane();
    let base = ok_with("m", "");
    let pad_at = 64 * 1024 - base.len();
    let at = format!("{base}{}", " ".repeat(pad_at));
    assert_eq!(at.len(), 64 * 1024);
    assert_eq!(check(&mut host, pane, &at).exit, Some(0));
    let over = format!("{at} ");
    assert_eq!(
        code(&check(&mut host, pane, &over)),
        "probe-output-too-large"
    );
}

/// REQ: progress-B07
#[test]
fn progress_b07_stderr_of_16_kib_is_accepted_and_one_more_byte_is_too_large() {
    let (mut host, pane) = host_with_pane();
    for (size, expect_ok) in [(16 * 1024usize, true), (16 * 1024 + 1, false)] {
        host.process().expect("sh", &["-c", PROBE]).returns(
            ilium_plugin_sdk::host::process::ProcessExit {
                status: Some(0),
                signal: None,
                stdout: running("job", 1, "W").into_bytes(),
                stderr: vec![b'x'; size],
            },
        );
        let run = progress(&mut host, pane, &["check", "--command", PROBE]);
        if expect_ok {
            assert_eq!(run.exit, Some(0));
        } else {
            assert_eq!(code(&run), "probe-output-too-large");
        }
    }
}

/// REQ: progress-B15
#[test]
fn progress_b15_a_non_zero_exit_is_rejected_probe_exited_non_zero() {
    let (mut host, pane) = host_with_pane();
    expect_exit(&mut host, PROBE, failing_exit(3));
    let run = progress(&mut host, pane, &["check", "--command", PROBE]);
    assert_eq!(code(&run), "probe-exited-non-zero");
}

/// REQ: progress-B15
#[test]
fn progress_b15_a_probe_that_exceeds_the_timeout_is_rejected_probe_timed_out() {
    let (mut host, pane) = host_with_pane();
    host.process()
        .expect("sh", &["-c", PROBE])
        .fails(PluginError::new(ErrorCode::TimedOut, "timed out"));
    let run = progress(&mut host, pane, &["check", "--command", PROBE]);
    assert_eq!(code(&run), "probe-timed-out");
}

/// REQ: progress-B15
#[test]
fn progress_b15_a_probe_that_cannot_start_is_rejected_probe_spawn_failed() {
    let (mut host, pane) = host_with_pane();
    host.process()
        .expect("sh", &["-c", PROBE])
        .fails(PluginError::new(ErrorCode::NotFound, "no sh"));
    let run = progress(&mut host, pane, &["check", "--command", PROBE]);
    assert_eq!(code(&run), "probe-spawn-failed");
}

/// REQ: progress-B15
#[test]
fn progress_b15_malformed_reports_are_rejected_invalid_probe_report() {
    let (mut host, pane) = host_with_pane();
    let cases = [
        String::new(),
        "[]".to_string(),
        "42".to_string(),
        format!("{} {}", running("job", 1, "W"), running("job", 1, "W")),
        r#"{"job_id":"j","status":"running","percent":1,"message":"m","details":"","error":null,"extra":1}"#.to_string(),
        r#"{"job_id":"j","status":"paused","percent":1,"message":"m","details":"","error":null}"#.to_string(),
        r#"{"job_id":"","status":"running","percent":1,"message":"m","details":"","error":null}"#.to_string(),
        r#"{"job_id":"j","status":"running","percent":101,"message":"m","details":"","error":null}"#.to_string(),
        r#"{"job_id":"j","status":"running","percent":-1,"message":"m","details":"","error":null}"#.to_string(),
        r#"{"job_id":"j","status":"running","percent":"half","message":"m","details":"","error":null}"#.to_string(),
    ];
    for case in cases {
        let run = check(&mut host, pane, &case);
        assert_eq!(code(&run), "invalid-probe-report", "case {case:?}");
    }
}

/// REQ: progress-B15, F-059
#[test]
fn progress_b15_an_error_status_without_an_error_text_is_rejected_invalid_probe_report() {
    let (mut host, pane) = host_with_pane();
    for error in [Value::Null, json!("")] {
        let run = check(
            &mut host,
            pane,
            &report("job", "error", 5.0, "m", "", error),
        );
        assert_eq!(code(&run), "invalid-probe-report");
    }
}

/// REQ: progress-B15, F-059
#[test]
fn progress_b15_an_error_text_on_a_non_error_status_is_rejected_invalid_probe_report() {
    let (mut host, pane) = host_with_pane();
    let run = check(
        &mut host,
        pane,
        &report("job", "running", 5.0, "m", "", json!("boom")),
    );
    assert_eq!(code(&run), "invalid-probe-report");
}

/// REQ: F-059
#[test]
fn f_059_an_empty_error_string_counts_as_absent_on_a_running_report() {
    let (mut host, pane) = host_with_pane();
    let output = report("job", "running", 5.0, "m", "", json!(""));
    expect_probe(&mut host, PROBE, &output);
    let (run, stderr) = progress_with_stderr(&mut host, pane, &["check", "--command", PROBE]);
    assert_eq!(run.exit, Some(0));
    assert_eq!(code(&run), "", "no progress_rejected record expected");
    assert!(stderr.is_empty(), "{stderr}");
}

/// REQ: F-059
#[test]
fn f_059_details_may_hold_newlines_carriage_returns_and_tabs_but_message_may_not_hold_controls() {
    let (mut host, pane) = host_with_pane();
    assert_eq!(
        check(&mut host, pane, &ok_with("m", "a\nb\r\nc\td")).exit,
        Some(0)
    );
    assert_eq!(
        code(&check(&mut host, pane, &ok_with("a\u{7}b", ""))),
        "invalid-probe-report"
    );
}

/// REQ: F-059
#[test]
fn f_059_percent_bounds_zero_and_one_hundred_are_accepted() {
    let (mut host, pane) = host_with_pane();
    for percent in [0.0, 100.0] {
        let run = check(
            &mut host,
            pane,
            &report("job", "running", percent, "m", "", Value::Null),
        );
        assert_eq!(run.exit, Some(0), "percent {percent}");
    }
}

/// REQ: progress-B09
#[test]
fn progress_b09_one_and_two_failures_leave_the_monitor_degraded_with_the_last_good_report() {
    let (mut host, pane) = host_with_pane();
    let id = register(&mut host, pane, PROBE, &running("job", 30, "Thirty"), &[]);
    for failures in 1..=2 {
        expect_exit(&mut host, PROBE, failing_exit(1));
        tick(&mut host, 1);
        let record = status(&mut host, pane);
        assert_eq!(
            health_of(&record, id),
            "degraded",
            "after {failures} failures"
        );
        assert_eq!(
            record["monitors"][0]["report"]["percent"].as_f64(),
            Some(30.0)
        );
        assert_eq!(record["monitors"][0]["report"]["status"], "running");
    }
}

/// REQ: progress-B09
#[test]
fn progress_b09_the_third_consecutive_failure_makes_the_monitor_failed_and_stops_polling() {
    let (mut host, pane) = host_with_pane();
    let id = register(&mut host, pane, PROBE, &running("job", 30, "Thirty"), &[]);
    for _ in 0..3 {
        expect_exit(&mut host, PROBE, failing_exit(1));
        tick(&mut host, 1);
    }
    assert_eq!(health_of(&status(&mut host, pane), id), "failed");
    let before = probe_runs(&mut host);
    host.process().allow_unexpected();
    tick(&mut host, 10);
    assert_eq!(probe_runs(&mut host), before);
}

/// REQ: progress-B09
#[test]
fn progress_b09_a_good_report_resets_the_failure_counter() {
    let (mut host, pane) = host_with_pane();
    let id = register(&mut host, pane, PROBE, &running("job", 30, "Thirty"), &[]);
    for _ in 0..2 {
        expect_exit(&mut host, PROBE, failing_exit(1));
        tick(&mut host, 1);
    }
    expect_probe(&mut host, PROBE, &running("job", 40, "Forty"));
    tick(&mut host, 1);
    assert_eq!(health_of(&status(&mut host, pane), id), "healthy");
    for _ in 0..2 {
        expect_exit(&mut host, PROBE, failing_exit(1));
        tick(&mut host, 1);
    }
    assert_eq!(health_of(&status(&mut host, pane), id), "degraded");
}

/// REQ: progress-B09, F-059
#[test]
fn progress_b09_a_status_that_goes_back_from_running_counts_as_a_failure() {
    let (mut host, pane) = host_with_pane();
    let id = register(&mut host, pane, PROBE, &running("job", 30, "Thirty"), &[]);
    expect_probe(
        &mut host,
        PROBE,
        &report("job", "not-started-yet", 0.0, "m", "", Value::Null),
    );
    tick(&mut host, 1);
    let record = status(&mut host, pane);
    assert_eq!(health_of(&record, id), "degraded");
    assert_eq!(record["monitors"][0]["report"]["status"], "running");
}

/// REQ: progress-B09, F-059
#[test]
fn progress_b09_a_changed_job_id_counts_as_a_failure() {
    let (mut host, pane) = host_with_pane();
    let id = register(&mut host, pane, PROBE, &running("job", 30, "Thirty"), &[]);
    expect_probe(&mut host, PROBE, &running("other-job", 31, "Other"));
    tick(&mut host, 1);
    let record = status(&mut host, pane);
    assert_eq!(health_of(&record, id), "degraded");
    assert_eq!(record["monitors"][0]["report"]["job_id"], "job");
}

/// REQ: F-059
#[test]
fn f_059_not_started_yet_may_move_to_running_error_or_done() {
    for next in [
        running("job", 1, "W"),
        done("job", "D"),
        failed("job", 1, "F", "boom"),
    ] {
        let (mut host, pane) = host_with_pane();
        let id = register(
            &mut host,
            pane,
            PROBE,
            &report("job", "not-started-yet", 0.0, "Queued", "", Value::Null),
            &[],
        );
        expect_probe(&mut host, PROBE, &next);
        tick(&mut host, 1);
        assert_eq!(health_of(&status(&mut host, pane), id), "healthy");
    }
}

/// REQ: F-059, progress-B09
#[test]
fn f_059_error_and_done_are_final_so_polling_stops_after_them() {
    for last in [done("job", "D"), failed("job", 1, "F", "boom")] {
        let (mut host, pane) = host_with_pane();
        register(&mut host, pane, PROBE, &running("job", 1, "W"), &[]);
        expect_probe(&mut host, PROBE, &last);
        tick(&mut host, 1);
        let before = probe_runs(&mut host);
        host.process().allow_unexpected();
        tick(&mut host, 10);
        assert_eq!(probe_runs(&mut host), before);
    }
}

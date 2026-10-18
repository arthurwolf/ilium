//! Exit status and outcome of `ilium progress set --wait` and `ilium progress wait`
//! (progress-B01, progress-B02, F-060, F-059): 0 done, 2 usage or rejected, 3 task error,
//! 4 monitor failed, 5 cleared or no monitor, 6 `--timeout-seconds` elapsed.

use ilium_plugin_sdk::testing::FakeHost;
use proptest::prelude::*;
use serde_json::Value;

use crate::common::*;

const PROBE: &str = "/opt/probe.sh";
const JOB: &str = "export-412";

/// Registers `PROBE` with `--wait` and an interval of 5 s; returns the open command line.
fn wait_open(host: &mut FakeHost, pane: u64, extra: &[&str]) -> Open {
    expect_probe(host, PROBE, &running(JOB, 0, "Exporting 412 sessions"));
    let mut args = vec![
        "progress",
        "set",
        "--command",
        PROBE,
        "--interval-seconds",
        "5",
        "--wait",
    ];
    args.extend_from_slice(extra);
    start_in_pane(host, pane, &args)
}

fn wait_record(run: &Run) -> Value {
    record_of(&run.stdout, "progress_wait")
}

/// REQ: progress-B01, progress-B02, F-059
#[test]
fn progress_b01_done_exits_zero_with_outcome_done() {
    let (mut host, pane) = host_with_pane();
    let open = wait_open(&mut host, pane, &[]);
    expect_probe(
        &mut host,
        PROBE,
        &done(JOB, "Export of 412 sessions finished"),
    );
    tick(&mut host, 5);
    let run = open.read();
    assert_eq!(run.exit, Some(0));
    let record = wait_record(&run);
    assert_eq!(record["outcome"], "done");
    assert_eq!(record["exit_code"], 0);
}

/// REQ: progress-B01, progress-B02
#[test]
fn progress_b01_task_error_exits_three_with_outcome_error() {
    let (mut host, pane) = host_with_pane();
    let open = wait_open(&mut host, pane, &[]);
    expect_probe(
        &mut host,
        PROBE,
        &failed(JOB, 40, "Export stopped", "disk full"),
    );
    tick(&mut host, 5);
    let run = open.read();
    assert_eq!(run.exit, Some(3));
    let record = wait_record(&run);
    assert_eq!(record["outcome"], "error");
    assert_eq!(record["exit_code"], 3);
    assert_eq!(record["progress"]["report"]["error"], "disk full");
}

/// REQ: progress-B01, progress-B02, progress-B09
#[test]
fn progress_b01_monitor_failed_exits_four_with_outcome_monitor_failed() {
    let (mut host, pane) = host_with_pane();
    let open = wait_open(&mut host, pane, &[]);
    for _ in 0..3 {
        expect_exit(&mut host, PROBE, failing_exit(1));
        tick(&mut host, 5);
    }
    let run = open.read();
    assert_eq!(run.exit, Some(4));
    let record = wait_record(&run);
    assert_eq!(record["outcome"], "monitor-failed");
    assert_eq!(record["exit_code"], 4);
}

/// REQ: progress-B01, progress-B02
#[test]
fn progress_b01_clearing_the_monitor_ends_the_wait_with_exit_five_and_outcome_cleared() {
    let (mut host, pane) = host_with_pane();
    let open = wait_open(&mut host, pane, &[]);
    let cleared = progress(&mut host, pane, &["clear", "--all"]);
    host.settle();
    assert_eq!(cleared.exit, Some(0));
    let run = open.read();
    assert_eq!(run.exit, Some(5));
    assert_eq!(wait_record(&run)["outcome"], "cleared");
}

/// REQ: progress-B01, progress-B02
#[test]
fn progress_b01_replace_ends_the_wait_of_the_replaced_monitor_with_exit_five() {
    let (mut host, pane) = host_with_pane();
    let open = wait_open(&mut host, pane, &[]);
    expect_probe(
        &mut host,
        "/opt/other.sh",
        &running("other-1", 0, "Other task"),
    );
    let replaced = progress(
        &mut host,
        pane,
        &["set", "--command", "/opt/other.sh", "--replace"],
    );
    host.settle();
    assert_eq!(replaced.exit, Some(0));
    let run = open.read();
    assert_eq!(run.exit, Some(5));
    assert_eq!(wait_record(&run)["outcome"], "cleared");
}

/// REQ: progress-B01, progress-B02
#[test]
fn progress_b01_wait_on_a_pane_without_monitor_exits_five_with_outcome_no_monitor() {
    let (mut host, pane) = host_with_pane();
    let run = progress(&mut host, pane, &["wait"]);
    assert_eq!(run.exit, Some(5));
    let record = wait_record(&run);
    assert_eq!(record["outcome"], "no-monitor");
    assert_eq!(record["exit_code"], 5);
    assert_eq!(record["pane_id"], pane);
}

/// REQ: progress-B01, progress-B02
#[test]
fn progress_b01_timeout_exits_six_with_outcome_still_running_and_keeps_the_monitor() {
    let (mut host, pane) = host_with_pane();
    let open = wait_open(&mut host, pane, &["--timeout-seconds", "3"]);
    expect_probe(
        &mut host,
        PROBE,
        &running(JOB, 10, "Exporting 412 sessions"),
    );
    tick(&mut host, 3);
    let run = open.read();
    assert_eq!(run.exit, Some(6));
    let record = wait_record(&run);
    assert_eq!(record["outcome"], "still-running");
    assert_eq!(record["exit_code"], 6);
    let snapshot = status(&mut host, pane);
    assert_eq!(snapshot["monitor_count"], 1);
    assert_eq!(snapshot["running"], true);
}

/// REQ: progress-B01
#[test]
fn progress_b01_timeout_boundary_is_one_second_before_open_and_at_the_second_closed() {
    let (mut host, pane) = host_with_pane();
    let open = wait_open(&mut host, pane, &["--timeout-seconds", "4"]);
    tick(&mut host, 3);
    assert_eq!(
        open.read().exit,
        None,
        "still waiting one second before the timeout"
    );
    tick(&mut host, 1);
    assert_eq!(open.read().exit, Some(6));
}

proptest! {
    // REQ: progress-B01, progress-B02
    #[test]
    fn progress_b01_timeout_of_n_seconds_ends_the_wait_exactly_at_n(seconds in 1u64..=20) {
        let (mut host, pane) = host_with_pane();
        let seconds_text = seconds.to_string();
        let open = wait_open(&mut host, pane, &["--timeout-seconds", &seconds_text]);
        host.process().allow_unexpected();
        tick(&mut host, seconds - 1);
        prop_assert_eq!(open.read().exit, None);
        tick(&mut host, 1);
        prop_assert_eq!(open.read().exit, Some(6));
    }
}

/// REQ: progress-B01
#[test]
fn progress_b01_several_monitors_without_an_id_exit_two_monitor_ambiguous() {
    let (mut host, pane) = host_with_pane();
    let first = register(&mut host, pane, "/opt/a.sh", &running("job-a", 0, "A"), &[]);
    let second = register(&mut host, pane, "/opt/b.sh", &running("job-b", 0, "B"), &[]);
    let run = progress(&mut host, pane, &["wait"]);
    assert_eq!(run.exit, Some(2));
    let record = record_of(&run.stdout, "progress_rejected");
    assert_eq!(record["code"], "monitor-ambiguous");
    assert_eq!(record["monitor_ids"], serde_json::json!([first, second]));
}

/// REQ: progress-B01
#[test]
fn progress_b01_a_wait_without_an_id_picks_the_only_unsettled_monitor() {
    let (mut host, pane) = host_with_pane();
    let settled = register(&mut host, pane, "/opt/a.sh", &running("job-a", 0, "A"), &[]);
    let _unsettled = register(
        &mut host,
        pane,
        "/opt/b.sh",
        &running("job-b", 0, "B"),
        &["--interval-seconds", "5"],
    );
    expect_probe(&mut host, "/opt/a.sh", &done("job-a", "A finished"));
    tick(&mut host, 1);
    let open = start_in_pane(
        &mut host,
        pane,
        &["progress", "wait", "--timeout-seconds", "1"],
    );
    host.process().allow_unexpected();
    tick(&mut host, 1);
    let record = wait_record(&open.read());
    assert_ne!(
        record["monitor_id"], settled,
        "the settled monitor is skipped"
    );
    assert_eq!(record["outcome"], "still-running");
}

/// REQ: progress-B01
#[test]
fn progress_b01_an_explicit_id_waits_for_that_monitor_among_several() {
    let (mut host, pane) = host_with_pane();
    let first = register(&mut host, pane, "/opt/a.sh", &running("job-a", 0, "A"), &[]);
    let _second = register(
        &mut host,
        pane,
        "/opt/b.sh",
        &done("job-b", "B finished"),
        &[],
    );
    let id_text = first.to_string();
    let open = start_in_pane(
        &mut host,
        pane,
        &["progress", "wait", &id_text, "--timeout-seconds", "1"],
    );
    host.process().allow_unexpected();
    tick(&mut host, 1);
    let record = wait_record(&open.read());
    assert_eq!(record["monitor_id"], first);
    assert_eq!(record["outcome"], "still-running");
}

/// REQ: progress-B01, progress-B02
#[test]
fn progress_b01_waiting_on_an_already_settled_monitor_returns_its_outcome_at_once() {
    let (mut host, pane) = host_with_pane();
    let id = register(
        &mut host,
        pane,
        PROBE,
        &running(JOB, 0, "Exporting"),
        &["--interval-seconds", "5"],
    );
    expect_probe(&mut host, PROBE, &done(JOB, "Finished"));
    tick(&mut host, 5);
    let run = progress(&mut host, pane, &["wait", &id.to_string()]);
    assert_eq!(run.exit, Some(0));
    assert_eq!(wait_record(&run)["outcome"], "done");
}

/// REQ: progress-B01
#[test]
fn progress_b01_a_missing_command_option_is_a_usage_error_with_exit_two() {
    let (mut host, pane) = host_with_pane();
    let run = progress(&mut host, pane, &["set"]);
    assert_eq!(run.exit, Some(2));
    assert_eq!(probe_runs(&mut host), 0);
}

/// REQ: progress-B01
#[test]
fn progress_b01_an_unknown_option_is_a_usage_error_with_exit_two() {
    let (mut host, pane) = host_with_pane();
    let run = progress(&mut host, pane, &["status", "--bogus"]);
    assert_eq!(run.exit, Some(2));
}

/// REQ: progress-B01
#[test]
fn progress_b01_a_rejected_request_exits_two_with_a_progress_rejected_record() {
    let (mut host, pane) = host_with_pane();
    let run = progress(&mut host, pane, &["clear", "--monitor-id", "99"]);
    assert_eq!(run.exit, Some(2));
    let record = record_of(&run.stdout, "progress_rejected");
    assert_eq!(record["code"], "stale-monitor");
}

/// REQ: progress-B02
#[test]
fn progress_b02_exit_code_field_equals_the_process_exit_status_for_every_outcome() {
    for (probe_output, expected_exit, expected_outcome) in [
        (done(JOB, "Finished"), 0u8, "done"),
        (failed(JOB, 10, "Stopped", "no space"), 3u8, "error"),
    ] {
        let (mut host, pane) = host_with_pane();
        let open = wait_open(&mut host, pane, &[]);
        expect_probe(&mut host, PROBE, &probe_output);
        tick(&mut host, 5);
        let run = open.read();
        assert_eq!(run.exit, Some(expected_exit));
        let record = wait_record(&run);
        assert_eq!(record["outcome"], expected_outcome);
        assert_eq!(record["exit_code"], u64::from(expected_exit));
    }
}

/// REQ: progress-B02, progress-B01
#[test]
fn progress_b02_waited_seconds_counts_the_elapsed_whole_seconds() {
    let (mut host, pane) = host_with_pane();
    let open = wait_open(&mut host, pane, &[]);
    expect_probe(&mut host, PROBE, &done(JOB, "Finished"));
    tick(&mut host, 5);
    assert_eq!(wait_record(&open.read())["waited_seconds"], 5);
}

/// REQ: progress-B01, progress-B02, F-060
#[test]
fn progress_b02_set_with_wait_prints_the_registration_line_before_the_wait_line() {
    let (mut host, pane) = host_with_pane();
    let open = wait_open(&mut host, pane, &[]);
    expect_probe(&mut host, PROBE, &done(JOB, "Finished"));
    tick(&mut host, 5);
    let types: Vec<String> = records(&open.read().stdout)
        .iter()
        .map(|record| record["type"].as_str().unwrap_or_default().to_string())
        .collect();
    assert_eq!(types, ["progress_set", "progress_wait"]);
}

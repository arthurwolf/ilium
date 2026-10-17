//! `ilium progress set` (F-060, F-059): the registration record, the interval, the limit of 8
//! monitors per pane, the same probe twice, `--replace`, the `disabled` setting and the
//! rejections (progress-B05, progress-B06, progress-B11, progress-B15).

use ilium_plugin_sdk::testing::{FakeHost, TreeNode};
use ilium_plugin_sdk::types::{NodeClass, Outcome};
use proptest::prelude::*;
use serde_json::{json, Value};

use crate::common::*;

const PROBE: &str = "/opt/probe.sh";
const JOB: &str = "export-412";

fn first_report() -> String {
    running(JOB, 0, "Exporting 412 sessions")
}

fn rejection(run: &Run) -> Value {
    record_of(&run.stdout, "progress_rejected")
}

fn set_run(host: &mut FakeHost, pane: u64, command: &str, extra: &[&str]) -> Run {
    let mut args = vec!["set", "--command", command];
    args.extend_from_slice(extra);
    progress(host, pane, &args)
}

fn key_set(record: &Value) -> Vec<String> {
    let mut keys: Vec<String> = record
        .as_object()
        .expect("object")
        .keys()
        .cloned()
        .collect();
    keys.sort();
    keys
}

/// REQ: F-060, F-059
#[test]
fn f_060_set_prints_one_progress_set_record_and_exits_zero() {
    let (mut host, pane) = host_with_pane();
    expect_probe(&mut host, PROBE, &first_report());
    let run = set_run(&mut host, pane, PROBE, &[]);
    assert_eq!(run.exit, Some(0));
    assert_eq!(records(&run.stdout).len(), 1);
    assert_eq!(records(&run.stdout)[0]["type"], "progress_set");
}

/// REQ: F-060
#[test]
fn f_060_set_record_fields_come_in_the_baseline_order() {
    let (mut host, pane) = host_with_pane();
    expect_probe(&mut host, PROBE, &first_report());
    let run = set_run(&mut host, pane, PROBE, &[]);
    assert_eq!(
        top_level_keys(&line_of(&run.stdout, "progress_set")),
        ["type", "request_id", "pane_id", "monitor_id", "progress"]
    );
}

/// REQ: F-060
#[test]
fn f_060_set_record_carries_the_pane_the_monitor_and_the_first_accepted_report() {
    let (mut host, pane) = host_with_pane();
    expect_probe(&mut host, PROBE, &first_report());
    let run = set_run(&mut host, pane, PROBE, &[]);
    let record = record_of(&run.stdout, "progress_set");
    assert_eq!(record["pane_id"], pane);
    assert!(record["request_id"].is_u64());
    assert_eq!(record["progress"]["monitor_id"], record["monitor_id"]);
    let report = &record["progress"]["report"];
    assert_eq!(report["job_id"], JOB);
    assert_eq!(report["status"], "running");
    assert_eq!(report["percent"].as_f64(), Some(0.0));
    assert_eq!(report["message"], "Exporting 412 sessions");
    assert_eq!(report["details"], "");
    assert!(report["error"].is_null());
    assert_eq!(
        record["progress"]["monitor_health"],
        json!({"state": "healthy"})
    );
    assert!(record["progress"]["last_observed_unix_millis"].is_u64());
}

/// REQ: F-059
#[test]
fn f_059_registration_runs_the_probe_once_and_installs_the_monitor() {
    let (mut host, pane) = host_with_pane();
    expect_probe(&mut host, PROBE, &first_report());
    set_run(&mut host, pane, PROBE, &[]);
    assert_eq!(probe_runs(&mut host), 1);
    let snapshot = status(&mut host, pane);
    assert_eq!(snapshot["monitor_count"], 1);
}

/// REQ: F-060
#[test]
fn f_060_two_registrations_get_distinct_monitor_ids() {
    let (mut host, pane) = host_with_pane();
    let first = register(&mut host, pane, "/opt/a.sh", &running("job-a", 0, "A"), &[]);
    let second = register(&mut host, pane, "/opt/b.sh", &running("job-b", 0, "B"), &[]);
    assert_ne!(first, second);
    assert_eq!(status(&mut host, pane)["monitor_count"], 2);
}

/// REQ: progress-B06
#[test]
fn progress_b06_the_default_interval_is_one_second() {
    let (mut host, pane) = host_with_pane();
    register(&mut host, pane, PROBE, &first_report(), &[]);
    let before = probe_runs(&mut host);
    host.process().allow_unexpected();
    host.advance_clock(999);
    host.settle();
    assert_eq!(probe_runs(&mut host), before, "no run before one second");
    host.advance_clock(1);
    host.settle();
    assert_eq!(probe_runs(&mut host), before + 1, "one run at one second");
}

/// REQ: progress-B06
#[test]
fn progress_b06_an_interval_of_five_seconds_runs_the_probe_every_five_seconds() {
    let (mut host, pane) = host_with_pane();
    register(
        &mut host,
        pane,
        PROBE,
        &first_report(),
        &["--interval-seconds", "5"],
    );
    let before = probe_runs(&mut host);
    host.process().allow_unexpected();
    tick(&mut host, 4);
    assert_eq!(probe_runs(&mut host), before);
    tick(&mut host, 1);
    assert_eq!(probe_runs(&mut host), before + 1);
    tick(&mut host, 5);
    assert_eq!(probe_runs(&mut host), before + 2);
}

/// REQ: progress-B06, progress-B15
#[test]
fn progress_b06_an_interval_of_zero_is_refused_invalid_request() {
    let (mut host, pane) = host_with_pane();
    let run = set_run(&mut host, pane, PROBE, &["--interval-seconds", "0"]);
    assert_eq!(run.exit, Some(2));
    assert_eq!(rejection(&run)["code"], "invalid-request");
    assert_eq!(status(&mut host, pane)["monitor_count"], 0);
}

/// REQ: progress-B06, progress-B15
#[test]
fn progress_b06_an_interval_of_86401_is_refused_invalid_request() {
    let (mut host, pane) = host_with_pane();
    let run = set_run(&mut host, pane, PROBE, &["--interval-seconds", "86401"]);
    assert_eq!(run.exit, Some(2));
    assert_eq!(rejection(&run)["code"], "invalid-request");
}

/// REQ: progress-B06
#[test]
fn progress_b06_the_interval_bounds_one_and_86400_are_accepted() {
    for interval in ["1", "86400"] {
        let (mut host, pane) = host_with_pane();
        expect_probe(&mut host, PROBE, &first_report());
        let run = set_run(&mut host, pane, PROBE, &["--interval-seconds", interval]);
        assert_eq!(run.exit, Some(0), "interval {interval}");
    }
}

/// REQ: progress-B06
#[test]
fn progress_b06_a_non_numeric_interval_is_a_usage_error() {
    let (mut host, pane) = host_with_pane();
    let run = set_run(&mut host, pane, PROBE, &["--interval-seconds", "soon"]);
    assert_eq!(run.exit, Some(2));
}

proptest! {
    // REQ: progress-B06
    #[test]
    fn progress_b06_every_interval_in_range_is_accepted(interval in 1u32..=86400) {
        let (mut host, pane) = host_with_pane();
        expect_probe(&mut host, PROBE, &first_report());
        let text = interval.to_string();
        let run = set_run(&mut host, pane, PROBE, &["--interval-seconds", &text]);
        prop_assert_eq!(run.exit, Some(0));
    }

    // REQ: progress-B06, progress-B15
    #[test]
    fn progress_b06_every_interval_above_the_range_is_refused(interval in 86401u32..=4_000_000) {
        let (mut host, pane) = host_with_pane();
        let text = interval.to_string();
        let run = set_run(&mut host, pane, PROBE, &["--interval-seconds", &text]);
        prop_assert_eq!(run.exit, Some(2));
    }
}

/// REQ: progress-B05
#[test]
fn progress_b05_eight_monitors_are_accepted() {
    let (mut host, pane) = host_with_pane();
    for index in 0..8 {
        let command = format!("/opt/probe-{index}.sh");
        register(
            &mut host,
            pane,
            &command,
            &running(&format!("job-{index}"), 0, "Working"),
            &[],
        );
    }
    let snapshot = status(&mut host, pane);
    assert_eq!(snapshot["monitor_count"], 8);
    assert_eq!(snapshot["monitors"].as_array().expect("monitors").len(), 8);
}

/// REQ: progress-B05, progress-B15
#[test]
fn progress_b05_the_ninth_live_monitor_is_rejected_too_many_monitors() {
    let (mut host, pane) = host_with_pane();
    for index in 0..8 {
        let command = format!("/opt/probe-{index}.sh");
        register(
            &mut host,
            pane,
            &command,
            &running(&format!("job-{index}"), 0, "Working"),
            &[],
        );
    }
    expect_probe(&mut host, "/opt/ninth.sh", &running("job-9", 0, "Working"));
    let run = set_run(&mut host, pane, "/opt/ninth.sh", &[]);
    assert_eq!(run.exit, Some(2));
    assert_eq!(rejection(&run)["code"], "too-many-monitors");
    assert_eq!(status(&mut host, pane)["monitor_count"], 8);
}

/// REQ: progress-B05
#[test]
fn progress_b05_a_settled_monitor_with_a_delivered_outcome_makes_room() {
    let (mut host, pane) = host_with_pane();
    for index in 0..8 {
        let command = format!("/opt/probe-{index}.sh");
        register(
            &mut host,
            pane,
            &command,
            &running(&format!("job-{index}"), 0, "Working"),
            &[],
        );
    }
    expect_probe(&mut host, "/opt/probe-0.sh", &done("job-0", "Finished"));
    tick(&mut host, 1);
    for request in host.pending_requests() {
        host.complete(request, Outcome::Ok);
    }
    host.settle();
    expect_probe(&mut host, "/opt/ninth.sh", &running("job-9", 0, "Working"));
    let run = set_run(&mut host, pane, "/opt/ninth.sh", &[]);
    assert_eq!(run.exit, Some(0));
}

/// REQ: progress-B05, progress-B15
#[test]
fn progress_b05_a_settled_monitor_whose_outcome_is_not_delivered_does_not_make_room() {
    let (mut host, pane) = host_with_pane();
    for index in 0..8 {
        let command = format!("/opt/probe-{index}.sh");
        register(
            &mut host,
            pane,
            &command,
            &running(&format!("job-{index}"), 0, "Working"),
            &[],
        );
    }
    expect_probe(&mut host, "/opt/probe-0.sh", &done("job-0", "Finished"));
    tick(&mut host, 1);
    expect_probe(&mut host, "/opt/ninth.sh", &running("job-9", 0, "Working"));
    let run = set_run(&mut host, pane, "/opt/ninth.sh", &[]);
    assert_eq!(rejection(&run)["code"], "too-many-monitors");
}

/// REQ: progress-B05
#[test]
fn progress_b05_the_limit_is_per_pane() {
    let (mut host, pane) = host_with_pane();
    let other = add_shell(&mut host, "other");
    for index in 0..8 {
        let command = format!("/opt/probe-{index}.sh");
        register(
            &mut host,
            pane,
            &command,
            &running(&format!("job-{index}"), 0, "Working"),
            &[],
        );
    }
    expect_probe(&mut host, "/opt/other.sh", &running("job-o", 0, "Working"));
    let run = set_run(&mut host, other, "/opt/other.sh", &[]);
    assert_eq!(run.exit, Some(0));
}

/// REQ: progress-B11
#[test]
fn progress_b11_the_same_command_while_running_returns_the_existing_monitor_id() {
    let (mut host, pane) = host_with_pane();
    let first = register(&mut host, pane, PROBE, &first_report(), &[]);
    expect_probe(&mut host, PROBE, &first_report());
    let run = set_run(&mut host, pane, PROBE, &[]);
    assert_eq!(run.exit, Some(0));
    assert_eq!(record_of(&run.stdout, "progress_set")["monitor_id"], first);
    assert_eq!(status(&mut host, pane)["monitor_count"], 1);
}

/// REQ: progress-B11
#[test]
fn progress_b11_a_different_command_adds_a_second_monitor() {
    let (mut host, pane) = host_with_pane();
    let first = register(&mut host, pane, PROBE, &first_report(), &[]);
    let second = register(
        &mut host,
        pane,
        "/opt/probe.sh --other",
        &first_report(),
        &[],
    );
    assert_ne!(first, second);
    assert_eq!(status(&mut host, pane)["monitor_count"], 2);
}

/// REQ: progress-B11
#[test]
fn progress_b11_the_same_command_in_another_pane_is_a_separate_monitor() {
    let (mut host, pane) = host_with_pane();
    let other = add_shell(&mut host, "other");
    register(&mut host, pane, PROBE, &first_report(), &[]);
    register(&mut host, other, PROBE, &first_report(), &[]);
    assert_eq!(status(&mut host, pane)["monitor_count"], 1);
    assert_eq!(status(&mut host, other)["monitor_count"], 1);
}

/// REQ: progress-B11
#[test]
fn progress_b11_a_settled_monitor_does_not_absorb_a_new_registration_of_its_command() {
    let (mut host, pane) = host_with_pane();
    let first = register(&mut host, pane, PROBE, &first_report(), &[]);
    expect_probe(&mut host, PROBE, &done(JOB, "Finished"));
    tick(&mut host, 1);
    let second = register(&mut host, pane, PROBE, &first_report(), &[]);
    assert_ne!(first, second);
}

/// REQ: F-060
#[test]
fn f_060_replace_clears_every_other_monitor_of_the_pane_first() {
    let (mut host, pane) = host_with_pane();
    register(&mut host, pane, "/opt/a.sh", &running("job-a", 0, "A"), &[]);
    register(&mut host, pane, "/opt/b.sh", &running("job-b", 0, "B"), &[]);
    let id = register(
        &mut host,
        pane,
        "/opt/c.sh",
        &running("job-c", 0, "C"),
        &["--replace"],
    );
    let snapshot = status(&mut host, pane);
    assert_eq!(snapshot["monitor_count"], 1);
    assert_eq!(snapshot["monitors"][0]["monitor_id"], id);
}

/// REQ: F-060
#[test]
fn f_060_without_replace_the_other_monitors_keep_running() {
    let (mut host, pane) = host_with_pane();
    let first = register(&mut host, pane, "/opt/a.sh", &running("job-a", 0, "A"), &[]);
    let second = register(&mut host, pane, "/opt/b.sh", &running("job-b", 0, "B"), &[]);
    let snapshot = status(&mut host, pane);
    let ids: Vec<u64> = snapshot["monitors"]
        .as_array()
        .expect("monitors")
        .iter()
        .map(|m| m["monitor_id"].as_u64().unwrap())
        .collect();
    assert_eq!(ids, [first, second]);
}

/// REQ: progress-B15, F-059
#[test]
fn progress_b15_set_is_rejected_disabled_while_the_setting_is_off() {
    let (mut host, pane) = host_with_pane();
    set_kernel_setting(&mut host, "ui.progress_monitor_enabled", json!(false))
        .expect("setting accepted");
    let run = set_run(&mut host, pane, PROBE, &[]);
    assert_eq!(run.exit, Some(2));
    assert_eq!(rejection(&run)["code"], "disabled");
    assert_eq!(probe_runs(&mut host), 0);
}

/// REQ: progress-B15, F-059
#[test]
fn progress_b15_check_is_rejected_disabled_while_the_setting_is_off() {
    let (mut host, pane) = host_with_pane();
    set_kernel_setting(&mut host, "ui.progress_monitor_enabled", json!(false))
        .expect("setting accepted");
    let run = progress(&mut host, pane, &["check", "--command", PROBE]);
    assert_eq!(rejection(&run)["code"], "disabled");
}

/// REQ: progress-B15, F-059
#[test]
fn progress_b15_registration_works_again_after_the_setting_is_turned_back_on() {
    let (mut host, pane) = host_with_pane();
    set_kernel_setting(&mut host, "ui.progress_monitor_enabled", json!(false))
        .expect("setting accepted");
    set_kernel_setting(&mut host, "ui.progress_monitor_enabled", json!(true))
        .expect("setting accepted");
    expect_probe(&mut host, PROBE, &first_report());
    assert_eq!(set_run(&mut host, pane, PROBE, &[]).exit, Some(0));
}

/// REQ: F-059
#[test]
fn f_059_turning_the_setting_off_stops_observation_and_keeps_the_recorded_results() {
    let (mut host, pane) = host_with_pane();
    register(&mut host, pane, PROBE, &first_report(), &[]);
    let before = probe_runs(&mut host);
    set_kernel_setting(&mut host, "ui.progress_monitor_enabled", json!(false))
        .expect("setting accepted");
    host.process().allow_unexpected();
    tick(&mut host, 10);
    assert_eq!(
        probe_runs(&mut host),
        before,
        "no probe runs while monitoring is off"
    );
    let snapshot = status(&mut host, pane);
    assert_eq!(snapshot["monitors"][0]["report"]["job_id"], JOB);
}

/// REQ: F-060, progress-B15
#[test]
fn progress_b15_outside_a_pane_the_request_fails_with_pane_identity_unavailable() {
    let (mut host, _pane) = host_with_pane();
    set_pane_env(&mut host, None);
    let invocation = host.cli().invoke(&["progress", "status"], b"");
    let stdout = invocation.stdout();
    assert_ne!(invocation.exit_code(), Some(0));
    let record = record_of(&stdout, "progress_request_failed");
    assert_eq!(record["code"], "pane-identity-unavailable");
    assert_eq!(
        record["message"],
        "not running inside an ilium-managed pane: ILIUM_PANE_ID is not set"
    );
    assert!(record["pane_id"].is_null());
    assert_eq!(
        key_set(&record),
        [
            "code",
            "message",
            "operation",
            "pane_id",
            "request_id",
            "type"
        ]
    );
}

/// REQ: progress-B15
#[test]
fn progress_b15_an_unknown_pane_is_rejected_pane_not_found() {
    let (mut host, _pane) = host_with_pane();
    let run = set_run(&mut host, 999, PROBE, &[]);
    assert_eq!(rejection(&run)["code"], "pane-not-found");
}

/// REQ: progress-B15
#[test]
fn progress_b15_a_pane_that_is_not_a_terminal_is_rejected_invalid_request() {
    let (mut host, _pane) = host_with_pane();
    let editor = host.tree().add(TreeNode {
        id: 0,
        parent: None,
        name: "notes".to_string(),
        class: NodeClass::Pane,
        content_kind: Some("ilium.editor".to_string()),
        agent: None,
    });
    let run = set_run(&mut host, editor, PROBE, &[]);
    assert_eq!(rejection(&run)["code"], "invalid-request");
}

/// REQ: progress-B15, progress-B07
#[test]
fn progress_b15_an_empty_command_is_rejected_invalid_request() {
    let (mut host, pane) = host_with_pane();
    let run = set_run(&mut host, pane, "", &[]);
    assert_eq!(rejection(&run)["code"], "invalid-request");
    assert_eq!(probe_runs(&mut host), 0);
}

/// REQ: progress-B07
#[test]
fn progress_b07_a_command_of_exactly_16_kib_is_accepted() {
    let (mut host, pane) = host_with_pane();
    let command = format!("{:<16384}", "/opt/probe.sh");
    assert_eq!(command.len(), 16 * 1024);
    expect_probe(&mut host, &command, &first_report());
    let run = set_run(&mut host, pane, &command, &[]);
    assert_eq!(run.exit, Some(0));
}

/// REQ: progress-B07, progress-B15
#[test]
fn progress_b07_a_command_of_16_kib_plus_one_byte_is_rejected_invalid_request() {
    let (mut host, pane) = host_with_pane();
    let command = format!("{:<16385}", "/opt/probe.sh");
    assert_eq!(command.len(), 16 * 1024 + 1);
    let run = set_run(&mut host, pane, &command, &[]);
    assert_eq!(rejection(&run)["code"], "invalid-request");
    assert_eq!(probe_runs(&mut host), 0);
}

/// REQ: progress-B15
#[test]
fn progress_b15_a_rejection_record_has_operation_request_id_pane_id_code_and_message() {
    let (mut host, pane) = host_with_pane();
    let run = set_run(&mut host, pane, PROBE, &["--interval-seconds", "0"]);
    let record = rejection(&run);
    assert_eq!(
        key_set(&record),
        [
            "code",
            "message",
            "operation",
            "pane_id",
            "request_id",
            "type"
        ]
    );
    assert_eq!(record["operation"], "set");
    assert_eq!(record["pane_id"], pane);
}

/// REQ: progress-B15
#[test]
fn progress_b15_a_rejected_registration_leaves_no_monitor_behind() {
    let (mut host, pane) = host_with_pane();
    let run = set_run(&mut host, pane, PROBE, &["--interval-seconds", "0"]);
    assert_ne!(run.exit, Some(0));
    assert_eq!(status(&mut host, pane)["monitors"], json!([]));
}

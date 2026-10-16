//! `ilium progress status`, `check` and `clear` (F-060, progress-B01, progress-B15).

use serde_json::json;

use crate::common::*;

const PROBE: &str = "/opt/probe.sh";

/// REQ: F-060
#[test]
fn f_060_status_of_an_empty_pane_lists_no_monitors() {
    let (mut host, pane) = host_with_pane();
    let run = progress(&mut host, pane, &["status"]);
    assert_eq!(run.exit, Some(0));
    let record = record_of(&run.stdout, "progress_status");
    assert_eq!(record["monitor_count"], 0);
    assert_eq!(record["running"], 0);
    assert_eq!(record["monitors"], json!([]));
}

/// REQ: F-060
#[test]
fn f_060_status_record_fields_come_in_the_documented_order() {
    let (mut host, pane) = host_with_pane();
    let run = progress(&mut host, pane, &["status"]);
    assert_eq!(
        top_level_keys(&line_of(&run.stdout, "progress_status")),
        ["type", "request_id", "pane_id", "monitor_count", "running", "monitors"]
    );
}

/// REQ: F-060
#[test]
fn f_060_status_counts_running_monitors_separately_from_settled_ones() {
    let (mut host, pane) = host_with_pane();
    register(&mut host, pane, "/opt/a.sh", &running("job-a", 10, "A"), &[]);
    register(&mut host, pane, "/opt/b.sh", &running("job-b", 20, "B"), &[]);
    expect_probe(&mut host, "/opt/a.sh", &done("job-a", "A done"));
    host.process().allow_unexpected();
    tick(&mut host, 1);
    let record = status(&mut host, pane);
    assert_eq!(record["monitor_count"], 2);
    assert_eq!(record["running"], 1);
}

/// REQ: F-060
#[test]
fn f_060_status_lists_monitors_in_registration_order() {
    let (mut host, pane) = host_with_pane();
    let ids: Vec<u64> = (0..3)
        .map(|n| register(&mut host, pane, &format!("/opt/{n}.sh"), &running(&format!("job-{n}"), 0, "W"), &[]))
        .collect();
    let record = status(&mut host, pane);
    let listed: Vec<u64> = record["monitors"].as_array().unwrap().iter().map(|m| m["monitor_id"].as_u64().unwrap()).collect();
    assert_eq!(listed, ids);
}

/// REQ: F-060
#[test]
fn f_060_status_shows_only_the_monitors_of_the_calling_pane() {
    let (mut host, pane) = host_with_pane();
    let other = add_shell(&mut host, "other");
    register(&mut host, pane, PROBE, &running("job", 0, "W"), &[]);
    assert_eq!(status(&mut host, other)["monitor_count"], 0);
}

/// REQ: F-059, F-060
#[test]
fn f_059_check_runs_the_probe_once_and_installs_nothing() {
    let (mut host, pane) = host_with_pane();
    expect_probe(&mut host, PROBE, &running("job", 40, "Forty"));
    let run = progress(&mut host, pane, &["check", "--command", PROBE]);
    assert_eq!(run.exit, Some(0));
    let record = record_of(&run.stdout, "progress_check");
    assert_eq!(record["report"]["percent"].as_f64(), Some(40.0));
    assert_eq!(probe_runs(&mut host), 1);
    host.process().allow_unexpected();
    tick(&mut host, 10);
    assert_eq!(probe_runs(&mut host), 1);
    assert_eq!(status(&mut host, pane)["monitor_count"], 0);
}

/// REQ: F-060
#[test]
fn f_060_check_record_fields_come_in_the_documented_order() {
    let (mut host, pane) = host_with_pane();
    expect_probe(&mut host, PROBE, &running("job", 40, "Forty"));
    let run = progress(&mut host, pane, &["check", "--command", PROBE]);
    assert_eq!(
        top_level_keys(&line_of(&run.stdout, "progress_check")),
        ["type", "request_id", "pane_id", "checked_at_unix_millis", "report"]
    );
}

/// REQ: F-060, progress-B15
#[test]
fn f_060_check_of_a_bad_report_is_rejected_invalid_probe_report() {
    let (mut host, pane) = host_with_pane();
    expect_probe(&mut host, PROBE, "not json");
    let run = progress(&mut host, pane, &["check", "--command", PROBE]);
    assert_eq!(run.exit, Some(2));
    assert_eq!(record_of(&run.stdout, "progress_rejected")["code"], "invalid-probe-report");
}

/// REQ: F-060, progress-B01
#[test]
fn f_060_clear_with_a_monitor_id_removes_that_monitor_and_reports_it() {
    let (mut host, pane) = host_with_pane();
    let first = register(&mut host, pane, "/opt/a.sh", &running("job-a", 0, "A"), &[]);
    let second = register(&mut host, pane, "/opt/b.sh", &running("job-b", 0, "B"), &[]);
    let run = progress(&mut host, pane, &["clear", "--monitor-id", &first.to_string()]);
    assert_eq!(run.exit, Some(0));
    let record = record_of(&run.stdout, "progress_clear");
    assert_eq!(record["cleared"], 1);
    assert_eq!(record["cleared_monitor_ids"], json!([first]));
    assert_eq!(status(&mut host, pane)["monitors"][0]["monitor_id"], second);
}

/// REQ: F-060
#[test]
fn f_060_clear_record_fields_come_in_the_documented_order() {
    let (mut host, pane) = host_with_pane();
    register(&mut host, pane, PROBE, &running("job", 0, "W"), &[]);
    let run = progress(&mut host, pane, &["clear", "--all"]);
    assert_eq!(
        top_level_keys(&line_of(&run.stdout, "progress_clear")),
        ["type", "request_id", "pane_id", "cleared", "cleared_monitor_ids"]
    );
}

/// REQ: F-060
#[test]
fn f_060_clear_all_removes_every_monitor_of_the_pane() {
    let (mut host, pane) = host_with_pane();
    for n in 0..3 {
        register(&mut host, pane, &format!("/opt/{n}.sh"), &running(&format!("job-{n}"), 0, "W"), &[]);
    }
    let run = progress(&mut host, pane, &["clear", "--all"]);
    assert_eq!(record_of(&run.stdout, "progress_clear")["cleared"], 3);
    assert_eq!(status(&mut host, pane)["monitor_count"], 0);
}

/// REQ: F-060
#[test]
fn f_060_clear_without_an_id_removes_the_only_monitor() {
    let (mut host, pane) = host_with_pane();
    let id = register(&mut host, pane, PROBE, &running("job", 0, "W"), &[]);
    let run = progress(&mut host, pane, &["clear"]);
    assert_eq!(record_of(&run.stdout, "progress_clear")["cleared_monitor_ids"], json!([id]));
}

/// REQ: F-060, progress-B15
#[test]
fn f_060_clear_without_an_id_among_several_is_refused_monitor_ambiguous_with_the_ids() {
    let (mut host, pane) = host_with_pane();
    let first = register(&mut host, pane, "/opt/a.sh", &running("job-a", 0, "A"), &[]);
    let second = register(&mut host, pane, "/opt/b.sh", &running("job-b", 0, "B"), &[]);
    let run = progress(&mut host, pane, &["clear"]);
    assert_eq!(run.exit, Some(2));
    let record = record_of(&run.stdout, "progress_rejected");
    assert_eq!(record["code"], "monitor-ambiguous");
    assert_eq!(record["monitor_ids"], json!([first, second]));
    assert_eq!(status(&mut host, pane)["monitor_count"], 2);
}

/// REQ: progress-B15
#[test]
fn progress_b15_clearing_an_unknown_monitor_is_rejected_stale_monitor() {
    let (mut host, pane) = host_with_pane();
    let run = progress(&mut host, pane, &["clear", "--monitor-id", "424242"]);
    assert_eq!(run.exit, Some(2));
    assert_eq!(record_of(&run.stdout, "progress_rejected")["code"], "stale-monitor");
}

/// REQ: F-060
#[test]
fn f_060_clearing_a_monitor_stops_its_probe_runs() {
    let (mut host, pane) = host_with_pane();
    register(&mut host, pane, PROBE, &running("job", 0, "W"), &[]);
    progress(&mut host, pane, &["clear", "--all"]);
    let before = probe_runs(&mut host);
    host.process().allow_unexpected();
    tick(&mut host, 10);
    assert_eq!(probe_runs(&mut host), before);
}

/// REQ: F-060
#[test]
fn f_060_clear_all_on_an_empty_pane_clears_zero_and_exits_zero() {
    let (mut host, pane) = host_with_pane();
    let run = progress(&mut host, pane, &["clear", "--all"]);
    assert_eq!(run.exit, Some(0));
    assert_eq!(record_of(&run.stdout, "progress_clear")["cleared"], 0);
}

/// REQ: progress-B15
#[test]
fn progress_b15_a_request_for_an_unknown_pane_is_rejected_pane_not_found() {
    let (mut host, _pane) = host_with_pane();
    let run = progress(&mut host, 999, &["status"]);
    assert_eq!(record_of(&run.stdout, "progress_rejected")["code"], "pane-not-found");
}

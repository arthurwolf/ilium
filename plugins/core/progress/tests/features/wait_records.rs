//! The `progress_wait` record: field order, `next` text and `composer_notice` (progress-B02,
//! progress-B03, progress-B04, F-060). Extra fields are not allowed (page, Behaviour contract).

use ilium_plugin_sdk::testing::FakeHost;
use serde_json::Value;

use crate::common::*;

const PROBE: &str = "/opt/probe.sh";
const JOB: &str = "export-412";

const NEXT_DONE: &str = "The task finished. Verify its output, then continue.";
const NEXT_ERROR: &str = "The task reported failure. Inspect its log or error, fix, and retry.";
const NEXT_MONITOR_FAILED: &str =
    "The probe stopped working, so the task outcome is unknown. Check the task directly; do not assume success.";
const NEXT_CLEARED: &str =
    "The monitor was cleared, or is not registered on this pane. Check `ilium progress status`.";
const NEXT_NO_MONITOR: &str =
    "This pane has no progress monitor. Register one with `ilium progress set`.";
const NEXT_STILL_RUNNING: &str =
    "The task is still running. Run the same wait command again to keep waiting.";

fn clear_suffix(monitor: u64) -> String {
    format!(" When you have handled it, remove this monitor with `ilium progress clear --monitor-id {monitor}`.")
}

/// The text of the object that follows `key` (for example `"report":`) in `line`.
fn object_after<'a>(line: &'a str, key: &str) -> &'a str {
    let start = line.find(key).unwrap_or_else(|| panic!("{key} in {line}")) + key.len();
    let rest = &line[start..];
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escaped = false;
    for (offset, symbol) in rest.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if symbol == '\\' {
                escaped = true;
            } else if symbol == '"' {
                in_string = false;
            }
            continue;
        }
        match symbol {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return &rest[..=offset];
                }
            }
            _ => {}
        }
    }
    panic!("unterminated object after {key}");
}

/// Registers the probe with `--wait` and returns the open command line and the monitor id.
fn open_wait(host: &mut FakeHost, pane: u64, extra: &[&str]) -> (Open, u64) {
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
    let open = start_in_pane(host, pane, &args);
    let id = record_of(&open.read().stdout, "progress_set")["monitor_id"]
        .as_u64()
        .expect("monitor id");
    (open, id)
}

fn finish_with(host: &mut FakeHost, probe_output: &str) {
    expect_probe(host, PROBE, probe_output);
    tick(host, 5);
}

fn wait_line(open: &Open) -> String {
    line_of(&open.read().stdout, "progress_wait")
}

fn wait_value(open: &Open) -> Value {
    record_of(&open.read().stdout, "progress_wait")
}

/// REQ: progress-B02, F-060
#[test]
fn progress_b02_wait_record_fields_come_in_the_documented_order() {
    let (mut host, pane) = host_with_pane();
    let (open, _) = open_wait(&mut host, pane, &[]);
    finish_with(&mut host, &done(JOB, "Finished"));
    assert_eq!(
        top_level_keys(&wait_line(&open)),
        [
            "type",
            "pane_id",
            "monitor_id",
            "outcome",
            "exit_code",
            "waited_seconds",
            "composer_notice",
            "next",
            "progress"
        ]
    );
}

/// REQ: progress-B02, F-060
#[test]
fn progress_b02_nested_progress_object_has_exactly_four_fields_in_order() {
    let (mut host, pane) = host_with_pane();
    let (open, _) = open_wait(&mut host, pane, &[]);
    finish_with(&mut host, &done(JOB, "Finished"));
    let line = wait_line(&open);
    let progress = object_after(&line, "\"progress\":");
    assert_eq!(
        top_level_keys(progress),
        [
            "monitor_id",
            "report",
            "monitor_health",
            "last_observed_unix_millis"
        ]
    );
}

/// REQ: progress-B02, F-060
#[test]
fn progress_b02_report_object_has_the_six_probe_fields_in_order() {
    let (mut host, pane) = host_with_pane();
    let (open, _) = open_wait(&mut host, pane, &[]);
    finish_with(&mut host, &done(JOB, "Finished"));
    let line = wait_line(&open);
    let report = object_after(&line, "\"report\":");
    assert_eq!(
        top_level_keys(report),
        ["job_id", "status", "percent", "message", "details", "error"]
    );
}

/// REQ: progress-B02
#[test]
fn progress_b02_done_report_carries_percent_one_hundred_and_a_null_error() {
    let (mut host, pane) = host_with_pane();
    let (open, _) = open_wait(&mut host, pane, &[]);
    finish_with(&mut host, &done(JOB, "Export of 412 sessions finished"));
    let report = &wait_value(&open)["progress"]["report"];
    assert_eq!(report["job_id"], JOB);
    assert_eq!(report["status"], "done");
    assert_eq!(report["percent"].as_f64(), Some(100.0));
    assert_eq!(report["message"], "Export of 412 sessions finished");
    assert_eq!(report["details"], "");
    assert!(report["error"].is_null());
}

/// REQ: progress-B02
#[test]
fn progress_b02_monitor_health_of_a_settled_task_is_healthy() {
    let (mut host, pane) = host_with_pane();
    let (open, _) = open_wait(&mut host, pane, &[]);
    finish_with(&mut host, &done(JOB, "Finished"));
    assert_eq!(
        wait_value(&open)["progress"]["monitor_health"]["state"],
        "healthy"
    );
}

/// REQ: progress-B03
#[test]
fn progress_b03_done_next_text_with_the_clear_suffix() {
    let (mut host, pane) = host_with_pane();
    let (open, id) = open_wait(&mut host, pane, &[]);
    finish_with(&mut host, &done(JOB, "Finished"));
    assert_eq!(
        wait_value(&open)["next"],
        format!("{NEXT_DONE}{}", clear_suffix(id))
    );
}

/// REQ: progress-B03
#[test]
fn progress_b03_error_next_text_with_the_clear_suffix() {
    let (mut host, pane) = host_with_pane();
    let (open, id) = open_wait(&mut host, pane, &[]);
    finish_with(&mut host, &failed(JOB, 10, "Stopped", "no space"));
    assert_eq!(
        wait_value(&open)["next"],
        format!("{NEXT_ERROR}{}", clear_suffix(id))
    );
}

/// REQ: progress-B03
#[test]
fn progress_b03_monitor_failed_next_text_starts_with_the_documented_sentence() {
    let (mut host, pane) = host_with_pane();
    let (open, _) = open_wait(&mut host, pane, &[]);
    for _ in 0..3 {
        expect_exit(&mut host, PROBE, failing_exit(1));
        tick(&mut host, 5);
    }
    let next = wait_value(&open)["next"]
        .as_str()
        .expect("next")
        .to_string();
    assert!(next.starts_with(NEXT_MONITOR_FAILED), "{next}");
}

/// REQ: progress-B03
#[test]
fn progress_b03_cleared_next_text_is_exact_and_has_no_clear_suffix() {
    let (mut host, pane) = host_with_pane();
    let (open, _) = open_wait(&mut host, pane, &[]);
    progress(&mut host, pane, &["clear", "--all"]);
    host.settle();
    assert_eq!(wait_value(&open)["next"], NEXT_CLEARED);
}

/// REQ: progress-B03
#[test]
fn progress_b03_no_monitor_next_text_is_exact() {
    let (mut host, pane) = host_with_pane();
    let run = progress(&mut host, pane, &["wait"]);
    assert_eq!(
        record_of(&run.stdout, "progress_wait")["next"],
        NEXT_NO_MONITOR
    );
}

/// REQ: progress-B03
#[test]
fn progress_b03_still_running_next_text_is_exact_and_has_no_clear_suffix() {
    let (mut host, pane) = host_with_pane();
    let (open, _) = open_wait(&mut host, pane, &["--timeout-seconds", "2"]);
    host.process().allow_unexpected();
    tick(&mut host, 2);
    assert_eq!(wait_value(&open)["next"], NEXT_STILL_RUNNING);
}

/// REQ: progress-B04
#[test]
fn progress_b04_a_held_wait_takes_the_outcome_so_the_notice_is_suppressed() {
    let (mut host, pane) = host_with_pane();
    let (open, _) = open_wait(&mut host, pane, &[]);
    finish_with(&mut host, &done(JOB, "Finished"));
    assert_eq!(wait_value(&open)["composer_notice"], "suppressed");
}

/// REQ: progress-B04
#[test]
fn progress_b04_a_settled_outcome_without_a_wait_gives_may_also_arrive() {
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
    assert_eq!(
        record_of(&run.stdout, "progress_wait")["composer_notice"],
        "may-also-arrive"
    );
}

/// REQ: progress-B04
#[test]
fn progress_b04_a_running_timeout_and_a_missing_monitor_give_none() {
    let (mut host, pane) = host_with_pane();
    let empty = progress(&mut host, pane, &["wait"]);
    assert_eq!(
        record_of(&empty.stdout, "progress_wait")["composer_notice"],
        "none"
    );
    let (open, _) = open_wait(&mut host, pane, &["--timeout-seconds", "2"]);
    host.process().allow_unexpected();
    tick(&mut host, 2);
    assert_eq!(wait_value(&open)["composer_notice"], "none");
}

/// REQ: progress-B04, progress-B14
#[test]
fn progress_b04_a_cancelled_wait_leaves_the_message_to_the_composer() {
    // "If the wait command is killed ... the server falls back to typing the message."
    let (mut host, pane) = host_with_pane();
    let (open, _) = open_wait(&mut host, pane, &[]);
    open.cancel(&mut host);
    finish_with(&mut host, &done(JOB, "Finished"));
    assert_eq!(typed_texts(&host, pane).len(), 1);
}

/// REQ: progress-B02, progress-B03
#[test]
fn progress_b03_the_wait_record_ids_match_the_registration_record() {
    let (mut host, pane) = host_with_pane();
    let (open, id) = open_wait(&mut host, pane, &[]);
    finish_with(&mut host, &done(JOB, "Finished"));
    let value = wait_value(&open);
    assert_eq!(value["monitor_id"], id);
    assert_eq!(value["pane_id"], pane);
    assert_eq!(value["progress"]["monitor_id"], id);
}

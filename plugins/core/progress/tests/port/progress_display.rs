//! Footer visibility rules (original ilium-client/src/progress_display.rs).

use serde_json::{json, Value};

use crate::common::*;
use crate::support::*;

fn retained(status: &str, message: &str) -> Value {
    monitor_with(
        17,
        "expiry-regression",
        status,
        100.0,
        message,
        "",
        Value::Null,
    )
}

/// REQ: PORT:ilium-client/src/progress_display.rs::absent_report_is_hidden_and_future_timestamp_does_not_underflow
#[test]
fn port_061_an_absent_report_draws_no_row_and_a_future_timestamp_keeps_the_row() {
    let mut host = activated_client();
    let mut absent = retained("done", "retained result");
    absent["report"] = Value::Null;
    show(&mut host, vec![absent]);
    assert!(!footer(&mut host, 80, 12).contains("retained result"));
    let mut future = retained("done", "retained result");
    future["last_observed_unix_millis"] = json!(9_000_000_000_000u64);
    show(&mut host, vec![future]);
    assert!(footer(&mut host, 80, 12).contains("retained result"));
}

/// REQ: PORT:ilium-client/src/progress_display.rs::completed_footer_expires_at_configured_deadline_without_mutation
#[test]
fn port_061_a_done_footer_expires_at_the_configured_deadline_and_the_server_keeps_the_record() {
    let mut host = activated_client();
    show(&mut host, vec![retained("done", "retained result")]);
    pass(&mut host, 59_999);
    assert!(footer(&mut host, 80, 12).contains("retained result"));
    pass(&mut host, 1);
    assert!(!footer(&mut host, 80, 12).contains("retained result"));
    pass(&mut host, 40_000);
    assert!(!footer(&mut host, 80, 12).contains("retained result"));
    let (mut server, pane) = host_with_pane();
    let id = register(&mut server, pane, PROBE, &running(JOB, 10, "W"), &[]);
    settle_with(&mut server, &done(JOB, "retained result"));
    tick(&mut server, 120);
    assert_eq!(health_of(&status(&mut server, pane), id), "healthy");
    assert_eq!(status(&mut server, pane)["monitor_count"], 1);
}

/// REQ: PORT:ilium-client/src/progress_display.rs::running_at_one_hundred_percent_is_not_completion
#[test]
fn port_061_a_running_monitor_at_100_percent_is_never_hidden_even_when_its_probe_failed() {
    let mut host = activated_client();
    let mut failed_health = retained("running", "still going");
    failed_health["monitor_health"] = json!({"state": "failed", "consecutive_failures": 3,
        "last_error": "probe unavailable; task outcome unknown"});
    show(&mut host, vec![failed_health]);
    pass(&mut host, 100_000);
    assert!(footer(&mut host, 80, 12).contains("still going"));
}

/// REQ: PORT:ilium-client/src/progress_display.rs::task_errors_expire_without_removing_the_error_record
#[test]
fn port_061_a_task_error_expires_from_the_footer_but_the_error_stays_in_the_status() {
    let mut host = activated_client();
    set_kernel_setting(
        &mut host,
        "ui.completed_progress_hide_after_seconds",
        json!(2),
    )
    .expect("accepted");
    show(
        &mut host,
        vec![monitor_with(
            17,
            "expiry-regression",
            "error",
            100.0,
            "failed row",
            "",
            json!("task failed"),
        )],
    );
    pass(&mut host, 1_999);
    assert!(footer(&mut host, 80, 12).contains("failed row"));
    pass(&mut host, 1);
    assert!(!footer(&mut host, 80, 12).contains("failed row"));
    let (mut server, pane) = host_with_pane();
    register(
        &mut server,
        pane,
        PROBE,
        &running("expiry-regression", 10, "W"),
        &[],
    );
    settle_with(
        &mut server,
        &failed("expiry-regression", 100, "failed row", "task failed"),
    );
    tick(&mut server, 120);
    let record = status(&mut server, pane);
    assert_eq!(record["monitors"][0]["report"]["error"], "task failed");
}

/// REQ: PORT:ilium-client/src/progress_display.rs::zero_duration_keeps_completed_results_visible
#[test]
fn port_061_a_hide_delay_of_zero_keeps_a_done_and_an_error_row_visible() {
    let mut host = activated_client();
    set_kernel_setting(
        &mut host,
        "ui.completed_progress_hide_after_seconds",
        json!(0),
    )
    .expect("accepted");
    show(
        &mut host,
        vec![
            retained("done", "kept done"),
            monitor_with(18, "other", "error", 100.0, "kept error", "", json!("boom")),
        ],
    );
    pass(&mut host, 86_400_000);
    let text = footer(&mut host, 80, 12);
    assert!(
        text.contains("kept done") && text.contains("kept error"),
        "{text}"
    );
}

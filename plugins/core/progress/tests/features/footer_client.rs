//! The client side: the footer rows and the status segment (F-061, progress-B10 render rows).
//! The client receives the monitors from the server side as a `message` on the `monitors`
//! channel (assumed, see ../API_GAPS.md) in the shape of `GET /progress.json`.

use ilium_plugin_sdk::testing::{assert_golden, FakeHost};
use ilium_plugin_sdk::types::{Intent, ViewTarget};
use serde_json::{json, Value};

use crate::common::*;

fn monitor(id: u64, status: &str, percent: f64, message: &str) -> Value {
    json!({"monitor_id": id, "report": {"job_id": format!("job-{id}"), "status": status, "percent": percent,
        "message": message, "details": "", "error": null}, "monitor_health": {"state": "healthy"},
        "last_observed_unix_millis": 0})
}

fn show(host: &mut FakeHost, monitors: Vec<Value>) {
    host.deliver_one(monitors_message(&json!({"monitors": monitors})));
    host.settle();
}

fn footer(host: &mut FakeHost, cols: u16) -> String {
    host.render(ViewTarget::Panel(FOOTER_VIEW.to_string()), cols, 12)
        .text()
}

fn segments(host: &FakeHost) -> Vec<Intent> {
    host.submitted()
        .iter()
        .map(|s| s.intent.clone())
        .filter(|i| {
            matches!(
                i,
                Intent::SetStatusSegment(_) | Intent::ClearStatusSegment(_)
            )
        })
        .collect()
}

/// REQ: F-061
#[test]
fn f_061_the_footer_matches_the_documented_figure_at_50_columns() {
    let mut host = activated_client();
    show(
        &mut host,
        vec![
            monitor(1, "running", 42.0, "Running the unit tests, crate 9 of 22"),
            monitor(2, "done", 100.0, "Export of 412 sessions finished"),
        ],
    );
    let text = host
        .render(ViewTarget::Panel(FOOTER_VIEW.to_string()), 50, 6)
        .text();
    assert_golden(&golden_path("progress_footer_50.txt"), &text);
}

/// REQ: F-061
#[test]
fn f_061_every_footer_line_fits_the_width_at_40_80_and_120_columns() {
    for cols in [40u16, 80, 120] {
        let mut host = activated_client();
        show(
            &mut host,
            vec![monitor(
                1,
                "running",
                42.0,
                "Running the unit tests, crate 9 of 22",
            )],
        );
        for line in footer(&mut host, cols).lines() {
            assert!(line.chars().count() <= cols as usize, "{cols}: {line:?}");
        }
    }
}

/// REQ: F-061
#[test]
fn f_061_the_footer_shows_percent_and_message_at_every_width() {
    for cols in [40u16, 80, 120] {
        let mut host = activated_client();
        show(&mut host, vec![monitor(1, "running", 42.0, "Exporting")]);
        let text = footer(&mut host, cols);
        assert!(text.contains("42%"), "{cols}: {text}");
        assert!(text.contains("Exporting"), "{cols}: {text}");
    }
}

/// REQ: F-061
#[test]
fn f_061_a_running_monitor_shows_running_and_a_done_one_shows_done() {
    let mut host = activated_client();
    show(
        &mut host,
        vec![
            monitor(1, "running", 10.0, "A"),
            monitor(2, "done", 100.0, "B"),
        ],
    );
    let text = footer(&mut host, 80);
    assert!(text.contains("RUNNING"), "{text}");
    assert!(text.contains("DONE"), "{text}");
}

/// REQ: F-061
#[test]
fn f_061_an_error_monitor_shows_its_state_and_message() {
    let mut host = activated_client();
    show(&mut host, vec![monitor(1, "error", 30.0, "Stopped early")]);
    let text = footer(&mut host, 80);
    assert!(text.contains("ERROR"), "{text}");
    assert!(text.contains("Stopped early"), "{text}");
}

/// REQ: F-061
#[test]
fn f_061_the_status_segment_counts_running_monitors() {
    let mut host = activated_client();
    show(
        &mut host,
        vec![
            monitor(1, "running", 10.0, "A"),
            monitor(2, "running", 20.0, "B"),
            monitor(3, "done", 100.0, "C"),
        ],
    );
    let last = segments(&host).pop().expect("segment intent");
    match last {
        Intent::SetStatusSegment(spec) => {
            assert_eq!(spec.segment, STATUS_SEGMENT);
            let text: String = spec.content.iter().map(|span| span.text.clone()).collect();
            assert_eq!(text, "2 running");
        }
        other => panic!("expected a segment, got {other:?}"),
    }
}

/// REQ: F-061
#[test]
fn f_061_the_status_segment_is_cleared_when_nothing_is_running() {
    let mut host = activated_client();
    show(&mut host, vec![monitor(1, "running", 10.0, "A")]);
    show(&mut host, vec![monitor(1, "done", 100.0, "A")]);
    assert_eq!(
        segments(&host).pop(),
        Some(Intent::ClearStatusSegment(STATUS_SEGMENT.to_string()))
    );
}

/// REQ: F-061
#[test]
fn f_061_an_empty_monitor_list_draws_no_footer_rows() {
    let mut host = activated_client();
    show(&mut host, vec![]);
    assert!(!footer(&mut host, 80).contains('%'));
}

/// REQ: F-061
#[test]
fn f_061_a_done_row_is_hidden_after_the_hide_delay_of_60_seconds() {
    let mut host = activated_client();
    show(&mut host, vec![monitor(1, "done", 100.0, "Finished job")]);
    host.advance_clock(59_999);
    host.settle();
    assert!(footer(&mut host, 80).contains("Finished job"));
    host.advance_clock(1);
    host.settle();
    assert!(!footer(&mut host, 80).contains("Finished job"));
}

/// REQ: F-061
#[test]
fn f_061_a_hide_delay_of_zero_never_hides_a_done_row() {
    let mut host = activated_client();
    set_kernel_setting(
        &mut host,
        "ui.completed_progress_hide_after_seconds",
        json!(0),
    )
    .expect("accepted");
    show(&mut host, vec![monitor(1, "done", 100.0, "Finished job")]);
    host.advance_clock(86_400_000);
    host.settle();
    assert!(footer(&mut host, 80).contains("Finished job"));
}

/// REQ: F-061
#[test]
fn f_061_a_running_row_at_100_percent_is_not_hidden_as_completed() {
    let mut host = activated_client();
    show(
        &mut host,
        vec![monitor(1, "running", 100.0, "Almost there")],
    );
    host.advance_clock(120_000);
    host.settle();
    assert!(footer(&mut host, 80).contains("Almost there"));
}

/// REQ: F-061
#[test]
fn f_061_the_footer_is_limited_to_the_configured_number_of_lines() {
    let mut host = activated_client();
    set_kernel_setting(&mut host, "ui.progress_max_lines", json!(2)).expect("accepted");
    let many: Vec<Value> = (1..=6)
        .map(|n| monitor(n, "running", 10.0, &format!("Task number {n}")))
        .collect();
    show(&mut host, many);
    let rows = footer(&mut host, 80)
        .lines()
        .filter(|line| line.contains("Task number"))
        .count();
    assert!(rows <= 2, "{rows} rows");
}

/// REQ: F-061
#[test]
fn f_061_the_default_footer_height_is_four_lines() {
    let mut host = activated_client();
    let many: Vec<Value> = (1..=8)
        .map(|n| monitor(n, "running", 10.0, &format!("Task number {n}")))
        .collect();
    show(&mut host, many);
    let rows = footer(&mut host, 80)
        .lines()
        .filter(|line| line.contains("Task number"))
        .count();
    assert!(rows <= 4, "{rows} rows");
}

//! Footer rows (original ilium-client/src/progress_bar.rs).

use serde_json::{json, Value};

use crate::common::*;
use crate::support::*;

fn one(status: &str, percent: f64, message: &str, details: &str, error: Value) -> Value {
    monitor_with(17, JOB, status, percent, message, details, error)
}

const DETAILS: &str = "What: compile the app for release\nWhy: you asked for a deployable build\nNext: install and verify it";
const MESSAGE: &str = "Building the release binaries - crate 4 of 10";

/// REQ: PORT:ilium-client/src/progress_bar.rs::footer_advertises_hover_details_only_when_details_exist
#[test]
fn port_061_the_footer_advertises_hover_details_only_when_details_exist() {
    let mut host = activated_client();
    show(
        &mut host,
        vec![one("running", 40.0, MESSAGE, DETAILS, Value::Null)],
    );
    assert!(footer(&mut host, 100, 12).contains("hover for details"));
    show(
        &mut host,
        vec![one("running", 40.0, MESSAGE, "", Value::Null)],
    );
    assert!(!footer(&mut host, 100, 12).contains("hover for details"));
}

/// REQ: PORT:ilium-client/src/progress_bar.rs::footer_never_prints_the_long_description
#[test]
fn port_061_the_footer_never_prints_the_long_details_text() {
    let mut host = activated_client();
    show(
        &mut host,
        vec![one("running", 40.0, MESSAGE, DETAILS, Value::Null)],
    );
    let text = footer(&mut host, 100, 12);
    assert!(
        text.contains(MESSAGE) && !text.contains("deployable build"),
        "{text}"
    );
}

/// REQ: PORT:ilium-client/src/progress_bar.rs::hover_tooltip_carries_message_details_and_identity
#[test]
fn port_061_the_hover_tooltip_carries_message_details_and_identity() {
    let mut host = activated_client();
    show(
        &mut host,
        vec![one("running", 40.0, MESSAGE, DETAILS, Value::Null)],
    );
    let tooltip = hover(&mut host, 2, 1).expect("tooltip over the monitor row");
    assert!(
        tooltip.contains(MESSAGE) && tooltip.contains("deployable build"),
        "{tooltip}"
    );
    assert!(
        tooltip.contains(&format!("job {JOB}")) && tooltip.contains("monitor #17"),
        "{tooltip}"
    );
}

/// REQ: PORT:ilium-client/src/progress_bar.rs::identity_is_omitted_before_critical_evidence_when_space_is_tight
#[test]
fn port_061_identity_is_dropped_before_the_error_evidence_when_rows_are_tight() {
    let mut host = activated_client();
    set_kernel_setting(&mut host, "ui.progress_max_lines", json!(2)).expect("accepted");
    let mut degraded = one("error", 63.0, "Compile", "", json!("exit status 7"));
    degraded["monitor_health"] =
        json!({"state": "degraded", "consecutive_failures": 1, "last_error": "probe slow"});
    show(&mut host, vec![degraded]);
    let text = footer(&mut host, 100, 12);
    assert!(!text.contains("monitor #"), "{text}");
    assert!(text.contains("exit status 7"), "{text}");
}

/// REQ: PORT:ilium-client/src/progress_bar.rs::monitor_failure_is_not_rendered_as_task_error
#[test]
fn port_061_a_failed_monitor_is_shown_as_monitor_failed_and_not_as_a_task_error() {
    let mut host = activated_client();
    let mut broken = one("running", 42.0, "Compile", "", Value::Null);
    broken["monitor_health"] =
        json!({"state": "failed", "consecutive_failures": 5, "last_error": "probe timed out"});
    show(&mut host, vec![broken]);
    let text = footer(&mut host, 100, 12);
    assert!(
        text.contains("MONITOR FAILED") && text.contains("42%"),
        "{text}"
    );
    assert!(
        text.contains("Observation stopped (5): probe timed out"),
        "{text}"
    );
    assert!(!text.contains("Task error:"), "{text}");
}

/// REQ: PORT:ilium-client/src/progress_bar.rs::monitors_that_do_not_fit_are_counted_on_the_last_row
#[test]
fn port_061_monitors_that_do_not_fit_are_counted_on_the_last_row() {
    let mut host = activated_client();
    set_kernel_setting(&mut host, "ui.progress_max_lines", json!(3)).expect("accepted");
    show(
        &mut host,
        (1..=5).map(|n| monitor(n, "running", 10.0, "w")).collect(),
    );
    let text = footer(&mut host, 100, 12);
    assert!(
        text.contains("job-1") && text.contains("job-2") && !text.contains("job-3"),
        "{text}"
    );
    assert!(text.contains("+3 more monitors"), "{text}");
}

/// REQ: PORT:ilium-client/src/progress_bar.rs::progress_slot_exists_before_a_report
#[test]
fn port_061_the_footer_view_exists_before_any_report_and_draws_no_gauge() {
    let mut host = activated_client();
    let text = footer(&mut host, 80, 6);
    assert!(!text.contains('%') && !text.contains("RUNNING"), "{text}");
}

/// REQ: PORT:ilium-client/src/progress_bar.rs::several_monitors_share_the_footer_one_gauge_row_each
#[test]
fn port_061_several_monitors_share_the_footer_one_gauge_row_each() {
    let mut host = activated_client();
    show(
        &mut host,
        vec![
            monitor_with(1, "build", "running", 20.0, "working", "", Value::Null),
            monitor_with(2, "tests", "running", 70.0, "working", "", Value::Null),
        ],
    );
    let text = footer(&mut host, 80, 12);
    let rows: Vec<&str> = text.lines().filter(|line| line.contains('%')).collect();
    assert_eq!(rows.len(), 2, "{text}");
    assert!(rows[0].contains("20%") && rows[1].contains("70%"), "{text}");
    assert!(text.contains("build") && text.contains("tests"), "{text}");
}

/// REQ: PORT:ilium-client/src/progress_bar.rs::small_progress_slot_retains_the_end_of_task_details
#[test]
fn port_061_a_narrow_footer_keeps_the_start_and_the_end_of_the_message() {
    let mut host = activated_client();
    show(
        &mut host,
        vec![monitor(1, "running", 10.0, "FIRST SECOND THIRD LAST")],
    );
    let text = footer(&mut host, 24, 12);
    assert!(
        text.contains("FIRST") && text.contains("LAST") && !text.contains("SECOND"),
        "{text}"
    );
}

/// REQ: PORT:ilium-client/src/progress_bar.rs::terminal_statuses_have_distinct_labels_and_details
#[test]
fn port_061_done_and_error_have_distinct_labels_and_details() {
    let mut host = activated_client();
    show(
        &mut host,
        vec![one("done", 12.0, "Rendered", "", Value::Null)],
    );
    let text = footer(&mut host, 100, 12);
    assert!(
        text.contains("DONE") && text.contains("100%") && text.contains("Rendered"),
        "{text}"
    );
    assert!(text.contains(&format!("Job {JOB}")), "{text}");
    show(
        &mut host,
        vec![one("error", 63.0, "Rendering", "", json!("exit status 7"))],
    );
    let text = footer(&mut host, 100, 12);
    assert!(text.contains("ERROR") && text.contains("63%"), "{text}");
    assert!(text.contains("Task error: exit status 7"), "{text}");
}

/// REQ: PORT:ilium-client/src/progress_bar.rs::the_footer_reserves_status_plus_the_configured_detail_budget
#[test]
fn port_061_the_footer_is_limited_to_the_configured_line_budget() {
    let mut host = activated_client();
    show(
        &mut host,
        (1..=8).map(|n| monitor(n, "running", 10.0, "w")).collect(),
    );
    let default_rows = footer(&mut host, 100, 40)
        .lines()
        .filter(|line| line.contains("job-"))
        .count();
    // default ui.progress_max_lines is 4 (manual: Footer lines, 1 to 20)
    assert_eq!(default_rows, 4, "{default_rows}");
    set_kernel_setting(&mut host, "ui.progress_max_lines", json!(2)).expect("accepted");
    show(
        &mut host,
        (1..=8).map(|n| monitor(n, "running", 10.0, "w")).collect(),
    );
    let rows = footer(&mut host, 100, 40)
        .lines()
        .filter(|line| line.contains("job-"))
        .count();
    assert_eq!(rows, 2, "{rows}");
}

/// REQ: PORT:ilium-client/src/progress_bar.rs::the_gauge_uses_distinct_filled_and_unfilled_glyphs
#[test]
fn port_061_the_gauge_uses_distinct_filled_and_unfilled_glyphs() {
    let mut host = activated_client();
    show(&mut host, vec![monitor(1, "running", 42.0, "w")]);
    let text = footer(&mut host, 80, 12);
    assert!(
        text.contains('\u{2588}') && text.contains('\u{2591}'),
        "{text}"
    );
}

/// REQ: PORT:ilium-client/src/progress_bar.rs::zero_detail_budget_reserves_only_the_status_row
#[test]
fn port_061_a_line_budget_of_one_draws_one_row_and_a_budget_of_zero_is_rejected_out_of_range() {
    let mut host = activated_client();
    set_kernel_setting(&mut host, "ui.progress_max_lines", json!(1)).expect("accepted");
    show(
        &mut host,
        vec![
            monitor(1, "running", 10.0, "w"),
            monitor(2, "running", 10.0, "w"),
        ],
    );
    let rows = footer(&mut host, 100, 12)
        .lines()
        .filter(|line| line.contains("job-"))
        .count();
    assert_eq!(rows, 1, "{rows}");
    let refused = set_kernel_setting(&mut host, "ui.progress_max_lines", json!(0))
        .expect_err("0 is out of range");
    // manual pins the message, not an error code
    assert_eq!(
        refused.message,
        "ui.progress_max_lines = 0 must be between 1 and 20"
    );
}

/// REQ: PORT:ilium-client/src/progress_bar.rs::footer_advertises_hover_details_only_when_details_exist
#[test]
fn port_061_the_hover_hint_appears_once_for_the_one_row_that_has_details() {
    let mut host = activated_client();
    show(
        &mut host,
        vec![
            monitor_with(1, "with", "running", 20.0, "row with details", DETAILS, Value::Null),
            monitor_with(2, "without", "running", 70.0, "row without details", "", Value::Null),
        ],
    );
    let text = footer(&mut host, 120, 12);
    assert_eq!(text.matches("hover for details").count(), 1, "{text}");
}

/// REQ: PORT:ilium-client/src/progress_bar.rs::footer_never_prints_the_long_description
#[test]
fn port_061_the_long_details_stay_out_of_the_footer_even_on_a_large_screen() {
    let mut host = activated_client();
    show(
        &mut host,
        vec![one("running", 40.0, MESSAGE, DETAILS, Value::Null)],
    );
    let text = footer(&mut host, 200, 40);
    assert!(!text.contains("Why:") && !text.contains("What:"), "{text}");
}

/// REQ: PORT:ilium-client/src/progress_bar.rs::hover_tooltip_carries_message_details_and_identity
#[test]
fn port_061_no_tooltip_is_offered_on_a_cell_outside_the_monitor_rows() {
    let mut host = activated_client();
    show(
        &mut host,
        vec![one("running", 40.0, MESSAGE, DETAILS, Value::Null)],
    );
    assert!(hover(&mut host, 2, 11).is_none());
}

/// REQ: PORT:ilium-client/src/progress_bar.rs::identity_is_omitted_before_critical_evidence_when_space_is_tight
#[test]
fn port_061_the_hover_tooltip_keeps_identity_and_the_error_evidence_together() {
    let mut host = activated_client();
    show(
        &mut host,
        vec![one("error", 63.0, "Compile", "", json!("exit status 7"))],
    );
    let tooltip = hover(&mut host, 2, 1).expect("tooltip over the monitor row");
    assert!(tooltip.contains("monitor #17"), "{tooltip}");
    assert!(tooltip.contains("exit status 7"), "{tooltip}");
}

/// REQ: PORT:ilium-client/src/progress_bar.rs::monitor_failure_is_not_rendered_as_task_error
#[test]
fn port_061_a_failed_monitor_row_never_carries_the_task_error_label() {
    let mut host = activated_client();
    let mut broken = one("running", 42.0, "Compile", "", Value::Null);
    broken["monitor_health"] =
        json!({"state": "failed", "consecutive_failures": 5, "last_error": "probe timed out"});
    show(&mut host, vec![broken]);
    let text = footer(&mut host, 100, 12);
    assert!(!text.contains("ERROR"), "{text}");
}

/// REQ: PORT:ilium-client/src/progress_bar.rs::monitors_that_do_not_fit_are_counted_on_the_last_row
#[test]
fn port_061_a_single_row_budget_counts_every_other_monitor_on_that_row() {
    let mut host = activated_client();
    set_kernel_setting(&mut host, "ui.progress_max_lines", json!(1)).expect("accepted");
    show(
        &mut host,
        (1..=5).map(|n| monitor(n, "running", 10.0, "w")).collect(),
    );
    let text = footer(&mut host, 100, 12);
    assert!(text.contains("+4 more monitors"), "{text}");
    assert!(!text.contains("job-2"), "{text}");
}

/// REQ: PORT:ilium-client/src/progress_bar.rs::progress_slot_exists_before_a_report
#[test]
fn port_061_the_footer_renders_in_a_one_by_one_area_without_panicking() {
    let mut host = activated_client();
    let _ = footer(&mut host, 1, 1);
    show(&mut host, vec![monitor(1, "running", 10.0, "w")]);
    let _ = footer(&mut host, 1, 1);
}

/// REQ: PORT:ilium-client/src/progress_bar.rs::several_monitors_share_the_footer_one_gauge_row_each
#[test]
fn port_061_monitor_rows_keep_their_registration_order() {
    let mut host = activated_client();
    show(
        &mut host,
        vec![
            monitor_with(1, "build", "running", 20.0, "working", "", Value::Null),
            monitor_with(2, "tests", "running", 70.0, "working", "", Value::Null),
        ],
    );
    let text = footer(&mut host, 80, 12);
    let build_at = text.find("build").expect("build row");
    let tests_at = text.find("tests").expect("tests row");
    assert!(build_at < tests_at, "{text}");
}

/// REQ: PORT:ilium-client/src/progress_bar.rs::small_progress_slot_retains_the_end_of_task_details
#[test]
fn port_061_a_wide_footer_shows_the_whole_message_without_eliding_the_middle() {
    let mut host = activated_client();
    show(
        &mut host,
        vec![monitor(1, "running", 10.0, "FIRST SECOND THIRD LAST")],
    );
    let text = footer(&mut host, 60, 12);
    assert!(text.contains("SECOND"), "{text}");
}

/// REQ: PORT:ilium-client/src/progress_bar.rs::terminal_statuses_have_distinct_labels_and_details
#[test]
fn port_061_a_done_row_never_carries_the_error_label() {
    let mut host = activated_client();
    show(
        &mut host,
        vec![one("done", 12.0, "Rendered", "", Value::Null)],
    );
    let text = footer(&mut host, 100, 12);
    assert!(!text.contains("ERROR") && !text.contains("Task error:"), "{text}");
}

/// REQ: PORT:ilium-client/src/progress_bar.rs::the_footer_reserves_status_plus_the_configured_detail_budget
#[test]
fn port_061_the_default_budget_folds_the_remaining_monitors_into_one_count() {
    let mut host = activated_client();
    show(
        &mut host,
        (1..=8).map(|n| monitor(n, "running", 10.0, "w")).collect(),
    );
    let text = footer(&mut host, 100, 40);
    assert!(text.contains("+5 more monitors"), "{text}");
}

/// REQ: PORT:ilium-client/src/progress_bar.rs::zero_detail_budget_reserves_only_the_status_row
#[test]
fn port_061_a_one_line_budget_with_one_monitor_shows_no_overflow_count() {
    let mut host = activated_client();
    set_kernel_setting(&mut host, "ui.progress_max_lines", json!(1)).expect("accepted");
    show(&mut host, vec![monitor(1, "running", 10.0, "w")]);
    let text = footer(&mut host, 100, 12);
    assert!(!text.contains("more monitor"), "{text}");
}

/// REQ: PORT:ilium-client/src/progress_bar.rs::the_gauge_uses_distinct_filled_and_unfilled_glyphs
#[test]
fn port_061_a_zero_percent_gauge_draws_only_the_unfilled_glyph() {
    let mut host = activated_client();
    show(&mut host, vec![monitor(1, "running", 0.0, "w")]);
    let text = footer(&mut host, 80, 12);
    assert!(text.contains('\u{2591}') && !text.contains('\u{2588}'), "{text}");
}

//! The two renders of the page (48 columns) and the dialogs at widths 40, 80 and 120.

use ilium_plugin_sdk::testing::{assert_golden, FakeSession};
use ilium_plugin_sdk::types::{ActionKind, ActionValue};

use crate::common::*;
use crate::feature_support::*;

/// The n-th `pre.term` block of the page, tags removed, HTML unescaped, trailing spaces trimmed per row.
fn page_figure(index: usize) -> String {
    let page = page_text();
    let block = page
        .split("<pre class=\"term\">")
        .nth(index + 1)
        .expect("figure exists");
    let block = block.split("</pre>").next().expect("figure end");
    let mut text = String::new();
    let mut in_tag = false;
    for character in block.chars() {
        match character {
            '<' => in_tag = true,
            '>' => in_tag = false,
            other if !in_tag => text.push(other),
            _ => {}
        }
    }
    let text = text
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">");
    text.lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
}

fn schedule_dialog_like_the_page() -> FakeSession {
    let mut session = session();
    let pane = terminal_pane(&mut session, "claude-api");
    fill_schedule_dialog(&mut session, pane, "2", "0", "30", "npm test", true);
    session
}

fn queue_dialog_like_the_page() -> FakeSession {
    let mut session = session();
    let pane = terminal_pane(&mut session, "claude-api");
    invoke_menu(&mut session, QUEUE_COMMAND, pane);
    set_text(
        &mut session,
        QUEUE_VIEW,
        "prompt",
        "Run the full test suite and report",
    );
    let host = client(&mut session);
    host.send_action(
        popup(QUEUE_VIEW),
        "delivery",
        ActionKind::Selected,
        ActionValue::Index(1),
    );
    host.settle();
    set_text(&mut session, QUEUE_VIEW, "times", "2");
    session
}

fn rows_of(figure: &str) -> u16 {
    figure.lines().count() as u16
}

/// REQ: F-055, scheduler-B02
#[test]
fn render_055_schedule_dialog_matches_the_page_figure_at_48_columns() {
    let figure = page_figure(0);
    let mut session = schedule_dialog_like_the_page();
    let screen = draw(&mut session, SCHEDULE_VIEW, 48, rows_of(&figure));
    assert_eq!(screen.text(), figure);
}

/// REQ: F-055, scheduler-B02
#[test]
fn render_055_schedule_dialog_matches_its_golden_file() {
    let mut session = schedule_dialog_like_the_page();
    let rows = rows_of(&page_figure(0));
    let screen = draw(&mut session, SCHEDULE_VIEW, 48, rows);
    assert_golden(&golden_path("schedule_48.txt"), &screen.text());
}

/// REQ: F-057, scheduler-B11
#[test]
fn render_057_queue_dialog_matches_the_page_figure_at_48_columns() {
    let figure = page_figure(1);
    let mut session = queue_dialog_like_the_page();
    let screen = draw(&mut session, QUEUE_VIEW, 48, rows_of(&figure));
    assert_eq!(screen.text(), figure);
}

/// REQ: F-057, scheduler-B11
#[test]
fn render_057_queue_dialog_matches_its_golden_file() {
    let mut session = queue_dialog_like_the_page();
    let rows = rows_of(&page_figure(1));
    let screen = draw(&mut session, QUEUE_VIEW, 48, rows);
    assert_golden(&golden_path("queue_48.txt"), &screen.text());
}

/// REQ: F-055
#[test]
fn render_055_the_page_figure_is_48_columns_wide_and_9_rows_high() {
    let figure = page_figure(0);
    assert!(figure.lines().all(|row| row.chars().count() <= 48));
    assert_eq!(rows_of(&figure), 10);
    assert_eq!(
        figure.lines().next().map(|row| row.chars().count()),
        Some(48)
    );
}

/// REQ: F-057
#[test]
fn render_057_the_page_figure_is_48_columns_wide_and_8_rows_high() {
    let figure = page_figure(1);
    assert!(figure.lines().all(|row| row.chars().count() <= 48));
    assert_eq!(rows_of(&figure), 9);
}

/// REQ: F-055, scheduler-B02
#[test]
fn render_055_schedule_dialog_has_the_leader_rows_at_widths_40_80_120() {
    for width in [40u16, 80, 120] {
        let mut session = schedule_dialog_like_the_page();
        let screen = draw(&mut session, SCHEDULE_VIEW, width, 24);
        let text = screen.text();
        for label in ["Hours", "Minutes", "Seconds", "Payload", "Send Enter"] {
            assert!(text.contains(label), "{label} at width {width}");
        }
        assert!(
            text.contains("Schedule input for claude-api"),
            "title at width {width}"
        );
        assert!(
            text.lines()
                .all(|row| row.chars().count() <= usize::from(width)),
            "no row wider than {width}"
        );
    }
}

/// REQ: F-055, scheduler-B02
#[test]
fn render_055_a_form_row_connects_label_and_value_with_dotted_leaders() {
    let mut session = schedule_dialog_like_the_page();
    let text = draw(&mut session, SCHEDULE_VIEW, 80, 24).text();
    let row = text
        .lines()
        .find(|row| row.contains("Hours"))
        .expect("hours row");
    assert!(
        row.contains("Hours …"),
        "one blank cell between label and dots: {row}"
    );
    assert!(
        row.contains("… 2"),
        "one blank cell between dots and value: {row}"
    );
}

/// REQ: F-057, scheduler-B11
#[test]
fn render_057_queue_dialog_has_its_rows_at_widths_40_80_120() {
    for width in [40u16, 80, 120] {
        let mut session = queue_dialog_like_the_page();
        let text = draw(&mut session, QUEUE_VIEW, width, 24).text();
        assert!(
            text.contains("Queue prompt after agent finishes"),
            "title at width {width}"
        );
        assert!(text.contains("Delivery"), "delivery at width {width}");
        assert!(text.contains("Times"), "times at width {width}");
        assert!(text.contains("Run X times"), "mode at width {width}");
        assert!(
            text.lines()
                .all(|row| row.chars().count() <= usize::from(width)),
            "no row wider than {width}"
        );
    }
}

/// REQ: F-057, scheduler-B11
#[test]
fn render_057_queue_dialog_shows_the_hint_line() {
    let mut session = queue_dialog_like_the_page();
    assert!(draw(&mut session, QUEUE_VIEW, 80, 24)
        .text()
        .contains("Ctrl+Enter enqueue · Esc cancel"));
}

/// REQ: F-055, F-057
#[test]
fn render_055_render_is_pure_with_no_event_between_two_renders() {
    let mut session = schedule_dialog_like_the_page();
    let first = draw(&mut session, SCHEDULE_VIEW, 80, 24);
    let second = draw(&mut session, SCHEDULE_VIEW, 80, 24);
    assert_eq!(first, second);
}

//! Twins of ilium-server/src/scheduled_input.rs tests.

use ilium_plugin_sdk::types::{Delivery, ErrorCode};

use crate::common::*;
use crate::support::*;

/// REQ: PORT:ilium-server/src/scheduled_input.rs::relative_deadline_rejects_zero_and_overflow, scheduler-B04
#[test]
fn port_server_relative_deadline_rejects_zero_and_overflow() {
    let mut session = session();
    let pane = terminal_pane(&mut session, "shell");
    let zero = schedule(&mut session, pane, 0, 0, 0, "x", true).expect_err("zero is refused");
    assert_eq!(zero.code, ErrorCode::InvalidArgument);
    assert_eq!(zero.message, "Choose a delay of at least one second");
    let huge =
        schedule(&mut session, pane, u64::MAX, 0, 0, "x", true).expect_err("overflow is refused");
    assert_eq!(huge.code, ErrorCode::InvalidArgument);
    assert_eq!(huge.message, "The duration is too large");
}

/// REQ: PORT:ilium-server/src/scheduled_input.rs::nearest_deadline_tracks_replacement_and_multiple_panes, scheduler-B07, scheduler-B08
#[test]
fn port_server_nearest_deadline_tracks_replacement_and_multiple_panes() {
    let mut session = session();
    let first = terminal_pane(&mut session, "first");
    let second = terminal_pane(&mut session, "second");
    schedule(&mut session, first, 0, 0, 5, "first", false).expect("first");
    schedule(&mut session, second, 0, 0, 3, "", true).expect("second");
    // The nearest deadline (second, 3 s) fires first.
    assert_eq!(sent_after(&mut session, 3_000), 1);
    let sent = sends(&server(&mut session).submitted());
    assert_eq!(sent[0].0.pane, second);
    // A replacement moves the second pane's deadline; the first pane's stays the nearest.
    schedule(&mut session, second, 0, 0, 7, "replacement", true).expect("replacement");
    assert_eq!(
        sent_after(&mut session, 2_000),
        2,
        "the first pane fires at 5 s"
    );
    assert_eq!(sends(&server(&mut session).submitted())[1].0.pane, first);
    assert_eq!(
        sent_after(&mut session, 6_999),
        2,
        "the replaced deadline is 7 s after the replacement"
    );
    assert_eq!(sent_after(&mut session, 1), 3);
}

/// REQ: PORT:ilium-server/src/scheduled_input.rs::raw_payload_bytes_cover_text_only_and_enter_only, scheduler-B08
#[test]
fn port_server_raw_payload_bytes_cover_text_only_and_enter_only() {
    for (payload, send_enter, delivery) in [
        ("continue", false, Delivery::Typed),
        ("continue", true, Delivery::Submitted),
        ("", true, Delivery::Submitted),
    ] {
        let mut session = session();
        let pane = terminal_pane(&mut session, "shell");
        schedule(&mut session, pane, 0, 0, 1, payload, send_enter).expect("armed");
        server(&mut session).advance_clock(1_000);
        let sent = sends(&server(&mut session).submitted());
        assert_eq!(sent.len(), 1, "{payload:?} {send_enter}");
        assert_eq!(
            sent[0].0,
            plain_spec(pane, payload, delivery),
            "{payload:?} {send_enter}"
        );
    }
}

/// REQ: PORT:ilium-server/src/scheduled_input.rs::relative_deadline_rejects_zero_and_overflow
#[test]
fn port_server_one_hour_delay_is_accepted_and_pending() {
    let mut session = session();
    let pane = terminal_pane(&mut session, "shell");
    schedule(&mut session, pane, 1, 0, 0, "hour", true).expect("one hour is accepted");
    let reply = call_queue(
        &mut session,
        serde_json::json!({"method": "pending", "pane": pane.0}),
    )
    .expect("pending answers");
    assert_eq!(reply["schedules"].as_array().map(Vec::len), Some(1));
}

/// REQ: PORT:ilium-server/src/scheduled_input.rs::nearest_deadline_tracks_replacement_and_multiple_panes
#[test]
fn port_server_equal_deadlines_fire_both_panes_together() {
    let mut session = session();
    let first = terminal_pane(&mut session, "first");
    let second = terminal_pane(&mut session, "second");
    schedule(&mut session, first, 0, 0, 2, "a", true).expect("first");
    schedule(&mut session, second, 0, 0, 2, "b", true).expect("second");
    assert_eq!(sent_after(&mut session, 2_000), 2);
    let panes: Vec<_> = sends(&server(&mut session).submitted())
        .iter()
        .map(|submission| submission.0.pane)
        .collect();
    assert!(panes.contains(&first) && panes.contains(&second), "{panes:?}");
}

/// REQ: PORT:ilium-server/src/scheduled_input.rs::raw_payload_bytes_cover_text_only_and_enter_only
#[test]
fn port_server_empty_payload_without_enter_is_refused() {
    let mut session = session();
    let pane = terminal_pane(&mut session, "shell");
    let error = schedule(&mut session, pane, 0, 0, 1, "", false)
        .expect_err("nothing to send is refused");
    assert_eq!(error.code, ErrorCode::InvalidArgument);
    assert_eq!(
        error.message,
        "Enter text, enable Send Enter, or use both"
    );
}

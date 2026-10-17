//! Twin of ilium/tests/pty_tui_smoke.rs (the scheduled-input path through the context menu).

use ilium_plugin_sdk::types::Delivery;

use crate::common::*;
use crate::support::*;

/// REQ: PORT:ilium/tests/pty_tui_smoke.rs::terminal_context_menu_schedules_countdown_and_delivers_input, F-055, scheduler-B01, scheduler-B08
#[test]
fn port_pty_terminal_context_menu_schedules_countdown_and_delivers_input() {
    let (mut session, pane) = schedule_dialog("idle");
    let text = draw(&mut session, SCHEDULE_VIEW, 120, 40).text();
    for needle in [
        "Schedule input for idle",
        "Hours",
        "Minutes",
        "Seconds",
        "Send Enter",
    ] {
        assert!(text.contains(needle), "{needle}: {text}");
    }
    set_text(&mut session, SCHEDULE_VIEW, "seconds", "2");
    set_text(
        &mut session,
        SCHEDULE_VIEW,
        "payload",
        "scheduled-live-marker",
    );
    commit_schedule(&mut session);
    let pending = call_queue(
        &mut session,
        serde_json::json!({"method": "pending", "pane": pane.0}),
    )
    .expect("pending");
    assert_eq!(
        pending["schedules"].as_array().map(Vec::len),
        Some(1),
        "the countdown is visible as pending"
    );
    assert_eq!(sent_after(&mut session, 1_999), 0);
    assert_eq!(sent_after(&mut session, 1), 1);
    let sent = sends(&server(&mut session).submitted());
    assert_eq!(
        sent[0].0,
        plain_spec(pane, "scheduled-live-marker", Delivery::Submitted)
    );
}

//! Twin of ilium-server/tests/smoke.rs.

use crate::common::*;
use crate::support::*;

/// REQ: PORT:ilium-server/tests/smoke.rs::scheduled_input_executes_after_client_detaches_and_clears_its_countdown, F-055, scheduler-B08, scheduler-B10
#[test]
fn port_smoke_scheduled_input_executes_after_client_detaches_and_clears_its_countdown() {
    let mut session = session();
    let pane = terminal_pane(&mut session, "scheduled");
    schedule(&mut session, pane, 0, 0, 1, "detached payload", true).expect("armed");
    let pending = call_queue(
        &mut session,
        serde_json::json!({"method": "pending", "pane": pane.0}),
    )
    .expect("pending");
    assert_eq!(pending["schedules"].as_array().map(Vec::len), Some(1));
    // No client dialog or client-side state is involved: the server alone fires the timer.
    assert_eq!(sent_after(&mut session, 999), 0);
    assert_eq!(sent_after(&mut session, 1), 1);
    let sent = sends(&server(&mut session).submitted());
    assert_eq!(sent[0].0.text, "detached payload");
    let cleared = call_queue(
        &mut session,
        serde_json::json!({"method": "pending", "pane": pane.0}),
    )
    .expect("pending");
    assert_eq!(
        cleared["schedules"].as_array().map(Vec::len),
        Some(0),
        "the countdown is cleared after the send"
    );
}

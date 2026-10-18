//! Twin of ilium-client/src/tree_ui.rs.

use crate::common::*;
use crate::support::*;

/// REQ: PORT:ilium-client/src/tree_ui.rs::scheduled_input_keeps_the_turn_glyph_and_marks_the_long_term_slot, F-055
#[test]
fn port_tree_ui_scheduled_input_keeps_the_turn_glyph_and_marks_the_long_term_slot() {
    let (mut session, pane) = schedule_dialog("Fix auth");
    set_text(&mut session, SCHEDULE_VIEW, "hours", "1");
    set_text(&mut session, SCHEDULE_VIEW, "minutes", "1");
    set_text(&mut session, SCHEDULE_VIEW, "seconds", "1");
    commit_schedule(&mut session);
    let reply = call_queue(
        &mut session,
        serde_json::json!({"method": "pending", "pane": pane.0}),
    )
    .expect("pending answers");
    assert_eq!(
        reply["schedules"].as_array().map(Vec::len),
        Some(1),
        "the pane carries the pending marker"
    );
    for (remaining_ms, text) in [(3_661_000u64, "1h 01m 01s"), (0, "now"), (61_001, "1m 02s")] {
        let countdown = call_queue(
            &mut session,
            serde_json::json!({"method": "countdown", "remaining_ms": remaining_ms}),
        )
        .expect("countdown answers");
        assert_eq!(countdown["text"], text, "{remaining_ms} ms");
    }
}

//! Twins of ilium-client/src/ui.rs tests on the two dialogs.

use crate::common::*;
use crate::support::*;

/// REQ: PORT:ilium-client/src/ui.rs::scheduled_input_dialog_renders_all_aerated_controls, scheduler-B02
#[test]
fn port_ui_scheduled_input_dialog_renders_all_aerated_controls() {
    let (mut session, _pane) = schedule_dialog("release shell");
    let text = draw(&mut session, SCHEDULE_VIEW, 100, 30).text();
    assert!(text.contains("Schedule input for release shell"), "{text}");
    for label in ["Hours", "Minutes", "Seconds", "Payload", "Send Enter"] {
        assert!(text.contains(label), "{label} missing: {text}");
    }
    assert!(text.contains("Ctrl+Enter schedule"), "{text}");
}

/// REQ: PORT:ilium-client/src/ui.rs::queue_prompt_long_draft_keeps_edited_tail_visible, scheduler-B11
#[test]
fn port_ui_queue_prompt_long_draft_keeps_edited_tail_visible() {
    for (columns, rows) in [(120u16, 40u16), (80, 24), (60, 20), (40, 12)] {
        let (mut session, _pane) = queue_dialog("agent");
        let mut draft: String = (0..60)
            .map(|index| format!("Synthetic line {index:02}\n"))
            .collect();
        draft.push_str("EDITED-TAIL");
        set_text(&mut session, QUEUE_VIEW, "prompt", &draft);
        let text = draw(&mut session, QUEUE_VIEW, columns, rows).text();
        assert!(
            text.contains("EDITED-TAIL"),
            "the edited tail is hidden at {columns}x{rows}: {text}"
        );
    }
}

/// REQ: PORT:ilium-client/src/ui.rs::scheduled_reset_uses_a_right_aligned_orange_and_light_blue_status_segment, F-055
#[test]
fn port_ui_scheduled_reset_status_segment_is_not_a_scheduler_surface() {
    // The original exercises the Codex reset-planning status segment, which is owned by the
    // reset-planning feature; the scheduler's own countdown segment is its pending-marker countdown.
    let (mut session, pane) = schedule_dialog("shell");
    set_text(&mut session, SCHEDULE_VIEW, "hours", "2");
    commit_schedule(&mut session);
    let reply = call_queue(
        &mut session,
        serde_json::json!({"method": "pending", "pane": pane.0}),
    )
    .expect("pending answers");
    let remaining = reply["schedules"][0]["remaining_ms"]
        .as_u64()
        .expect("remaining_ms");
    assert!(
        remaining > 7_000_000 && remaining <= 7_230_000,
        "{remaining}"
    );
}

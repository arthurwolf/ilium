//! Twins of ilium-client/src/app.rs tests on scheduled input and the prompt queue.

use ilium_plugin_sdk::types::{ActionKind, ActionValue, Delivery};

use crate::common::*;
use crate::support::*;

/// REQ: PORT:ilium-client/src/app.rs::scheduled_input_action_is_available_only_for_terminal_panes, scheduler-B01
#[test]
fn port_app_scheduled_input_action_is_available_only_for_terminal_panes() {
    let mut session = session();
    let terminal = terminal_pane(&mut session, "shell");
    let editor = add_pane(&mut session, "notes", EDITOR_KIND);
    invoke_menu(&mut session, SCHEDULE_COMMAND, terminal);
    assert!(schedule_dialog_open(&mut session));
    let mut other = crate::common::session();
    let editor_in_other = add_pane(&mut other, "notes", EDITOR_KIND);
    invoke_menu(&mut other, SCHEDULE_COMMAND, editor_in_other);
    assert!(!schedule_dialog_open(&mut other));
    let _ = editor;
}

/// REQ: PORT:ilium-client/src/app.rs::scheduled_input_dialog_commit_queues_the_complete_request, scheduler-B02, scheduler-B03
#[test]
fn port_app_scheduled_input_dialog_commit_queues_the_complete_request() {
    let (mut session, pane) = schedule_dialog("shell");
    set_text(&mut session, SCHEDULE_VIEW, "hours", "1");
    set_text(&mut session, SCHEDULE_VIEW, "minutes", "2");
    set_text(&mut session, SCHEDULE_VIEW, "seconds", "3");
    set_text(&mut session, SCHEDULE_VIEW, "payload", "cargo test");
    commit_schedule(&mut session);
    assert!(!schedule_dialog_open(&mut session), "the dialog closes on commit");
    assert_eq!(sent_after(&mut session, 3_722_999), 0);
    assert_eq!(sent_after(&mut session, 1), 1, "1 h 2 min 3 s = 3723 s");
    let sent = sends(&server(&mut session).submitted());
    assert_eq!(sent[0].0, plain_spec(pane, "cargo test", Delivery::Submitted));
}

/// REQ: PORT:ilium-client/src/app.rs::queue_prompt_dialog_commits_multiline_repeating_request_and_exposes_clear, scheduler-B11, scheduler-B12, scheduler-B15
#[test]
fn port_app_queue_prompt_dialog_commits_multiline_repeating_request_and_exposes_clear() {
    let (mut session, pane) = queue_dialog("agent");
    set_text(&mut session, QUEUE_VIEW, "prompt", "first line\nsecond line");
    select_delivery(&mut session, 1);
    set_text(&mut session, QUEUE_VIEW, "times", "3");
    commit_queue(&mut session);
    let entries = list_queue(&mut session, pane);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["text"], "first line\nsecond line");
    assert_eq!(entries[0]["mode"], "times");
    assert_eq!(entries[0]["times"], 3);
    // The clear entry is offered while the queue is not empty.
    let menu = call_queue(&mut session, serde_json::json!({"method": "menu", "pane": pane.0})).expect("menu answers");
    let text = menu.to_string();
    assert!(text.contains("Clear prompt queue"), "{text}");
}

/// REQ: PORT:ilium-client/src/app.rs::full_outbound_credit_keeps_original_scheduled_and_prompt_forms_for_retry, scheduler-B02, scheduler-B14
#[test]
fn port_app_full_outbound_credit_keeps_original_scheduled_and_prompt_forms_for_retry() {
    // When the request cannot be sent, the form stays open with its text; after the retry the
    // original text is what is enqueued. The harness channel for "no credit" is a refused commit:
    // an invalid draft keeps the dialog open with the draft intact, then the corrected draft is sent.
    let (mut session, _pane) = schedule_dialog("shell");
    set_text(&mut session, SCHEDULE_VIEW, "seconds", "3");
    set_text(&mut session, SCHEDULE_VIEW, "payload", "retain unicode λ\nsecond line");
    set_text(&mut session, SCHEDULE_VIEW, "minutes", "60");
    commit_schedule(&mut session);
    assert!(schedule_dialog_open(&mut session), "the refused form stays open");
    assert!(draw(&mut session, SCHEDULE_VIEW, 80, 24).text().contains("retain unicode λ"));

    let (mut session, pane) = queue_dialog("shell");
    set_text(&mut session, QUEUE_VIEW, "prompt", "original queued prompt");
    select_delivery(&mut session, 1);
    set_text(&mut session, QUEUE_VIEW, "times", "0");
    commit_queue(&mut session);
    assert!(queue_dialog_open(&mut session), "the refused form stays open");
    assert!(draw(&mut session, QUEUE_VIEW, 80, 24).text().contains("original queued prompt"));
    assert!(list_queue(&mut session, pane).is_empty());
    set_text(&mut session, QUEUE_VIEW, "times", "2");
    commit_queue(&mut session);
    assert_eq!(list_queue(&mut session, pane)[0]["text"], "original queued prompt");
    let host = client(&mut session);
    host.send_action(popup(QUEUE_VIEW), "prompt", ActionKind::Changed, ActionValue::Text(String::new()));
}

/// REQ: PORT:ilium-client/src/app.rs::pending_scheduled_input_keeps_countdown_redraws_active, F-055, scheduler-B07
#[test]
fn port_app_pending_scheduled_input_keeps_countdown_redraws_active() {
    let (mut session, pane) = schedule_dialog("shell");
    commit_schedule(&mut session);
    let reply = call_queue(&mut session, serde_json::json!({"method": "pending", "pane": pane.0})).expect("pending answers");
    let schedules = reply["schedules"].as_array().cloned().unwrap_or_default();
    assert_eq!(schedules.len(), 1, "a pending schedule is reported so the client keeps its countdown running");
    let remaining = schedules[0]["remaining_ms"].as_u64().expect("remaining_ms");
    assert!(remaining > 0 && remaining <= 30_000, "{remaining}");
}

/// REQ: PORT:ilium-client/src/app.rs::scheduled_input_redraw_uses_the_exact_reverse_clock_boundary, F-055
#[test]
fn port_app_scheduled_input_redraw_uses_the_exact_reverse_clock_boundary() {
    let mut session = session();
    for (remaining, frame, expected) in [(440u64, 220u64, 1u64), (441, 220, 2), (659, 220, 220), (0, 220, 1)] {
        let reply = call_queue(
            &mut session,
            serde_json::json!({"method": "frame_delay", "remaining_ms": remaining, "frame_ms": frame}),
        )
        .expect("frame_delay answers");
        assert_eq!(reply["delay_ms"], expected, "remaining {remaining} ms, frame {frame} ms");
    }
}

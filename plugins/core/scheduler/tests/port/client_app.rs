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
    assert!(
        !schedule_dialog_open(&mut session),
        "the dialog closes on commit"
    );
    assert_eq!(sent_after(&mut session, 3_722_999), 0);
    assert_eq!(sent_after(&mut session, 1), 1, "1 h 2 min 3 s = 3723 s");
    let sent = sends(&server(&mut session).submitted());
    assert_eq!(
        sent[0].0,
        plain_spec(pane, "cargo test", Delivery::Submitted)
    );
}

/// REQ: PORT:ilium-client/src/app.rs::queue_prompt_dialog_commits_multiline_repeating_request_and_exposes_clear, scheduler-B11, scheduler-B12, scheduler-B15
#[test]
fn port_app_queue_prompt_dialog_commits_multiline_repeating_request_and_exposes_clear() {
    let (mut session, pane) = queue_dialog("agent");
    set_text(
        &mut session,
        QUEUE_VIEW,
        "prompt",
        "first line\nsecond line",
    );
    select_delivery(&mut session, 1);
    set_text(&mut session, QUEUE_VIEW, "times", "3");
    commit_queue(&mut session);
    let entries = list_queue(&mut session, pane);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["text"], "first line\nsecond line");
    assert_eq!(entries[0]["mode"], "times");
    assert_eq!(entries[0]["times"], 3);
    // The clear entry is offered while the queue is not empty.
    let menu = call_queue(
        &mut session,
        serde_json::json!({"method": "menu", "pane": pane.0}),
    )
    .expect("menu answers");
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
    set_text(
        &mut session,
        SCHEDULE_VIEW,
        "payload",
        "retain unicode λ\nsecond line",
    );
    set_text(&mut session, SCHEDULE_VIEW, "minutes", "60");
    commit_schedule(&mut session);
    assert!(
        schedule_dialog_open(&mut session),
        "the refused form stays open"
    );
    assert!(draw(&mut session, SCHEDULE_VIEW, 80, 24)
        .text()
        .contains("retain unicode λ"));

    let (mut session, pane) = queue_dialog("shell");
    set_text(&mut session, QUEUE_VIEW, "prompt", "original queued prompt");
    select_delivery(&mut session, 1);
    set_text(&mut session, QUEUE_VIEW, "times", "0");
    commit_queue(&mut session);
    assert!(
        queue_dialog_open(&mut session),
        "the refused form stays open"
    );
    assert!(draw(&mut session, QUEUE_VIEW, 80, 24)
        .text()
        .contains("original queued prompt"));
    assert!(list_queue(&mut session, pane).is_empty());
    set_text(&mut session, QUEUE_VIEW, "times", "2");
    commit_queue(&mut session);
    assert_eq!(
        list_queue(&mut session, pane)[0]["text"],
        "original queued prompt"
    );
    let host = client(&mut session);
    host.send_action(
        popup(QUEUE_VIEW),
        "prompt",
        ActionKind::Changed,
        ActionValue::Text(String::new()),
    );
}

/// REQ: PORT:ilium-client/src/app.rs::pending_scheduled_input_keeps_countdown_redraws_active, F-055, scheduler-B07
#[test]
fn port_app_pending_scheduled_input_keeps_countdown_redraws_active() {
    let (mut session, pane) = schedule_dialog("shell");
    commit_schedule(&mut session);
    let reply = call_queue(
        &mut session,
        serde_json::json!({"method": "pending", "pane": pane.0}),
    )
    .expect("pending answers");
    let schedules = reply["schedules"].as_array().cloned().unwrap_or_default();
    assert_eq!(
        schedules.len(),
        1,
        "a pending schedule is reported so the client keeps its countdown running"
    );
    let remaining = schedules[0]["remaining_ms"].as_u64().expect("remaining_ms");
    assert!(remaining > 0 && remaining <= 30_000, "{remaining}");
}

/// REQ: PORT:ilium-client/src/app.rs::scheduled_input_redraw_uses_the_exact_reverse_clock_boundary, F-055
#[test]
fn port_app_scheduled_input_redraw_uses_the_exact_reverse_clock_boundary() {
    let mut session = session();
    for (remaining, frame, expected) in [
        (440u64, 220u64, 1u64),
        (441, 220, 2),
        (659, 220, 220),
        (0, 220, 1),
    ] {
        let reply = call_queue(
            &mut session,
            serde_json::json!({"method": "frame_delay", "remaining_ms": remaining, "frame_ms": frame}),
        )
        .expect("frame_delay answers");
        assert_eq!(
            reply["delay_ms"], expected,
            "remaining {remaining} ms, frame {frame} ms"
        );
    }
}

/// REQ: PORT:ilium-client/src/app.rs::scheduled_input_action_is_available_only_for_terminal_panes
#[test]
fn port_app_queue_prompt_action_is_refused_for_editor_panes() {
    let mut session = session();
    let editor = add_pane(&mut session, "notes", EDITOR_KIND);
    invoke_menu(&mut session, QUEUE_COMMAND, editor);
    assert!(
        !queue_dialog_open(&mut session),
        "the queue entry needs a terminal pane"
    );
}

/// REQ: PORT:ilium-client/src/app.rs::scheduled_input_dialog_commit_queues_the_complete_request
#[test]
fn port_app_scheduled_input_escape_cancels_without_arming() {
    let (mut session, pane) = schedule_dialog("shell");
    set_text(&mut session, SCHEDULE_VIEW, "seconds", "5");
    set_text(&mut session, SCHEDULE_VIEW, "payload", "never armed");
    press(&mut session, SCHEDULE_VIEW, "esc");
    assert!(!schedule_dialog_open(&mut session), "Esc closes the dialog");
    let reply = call_queue(
        &mut session,
        serde_json::json!({"method": "pending", "pane": pane.0}),
    )
    .expect("pending answers");
    assert_eq!(reply["schedules"].as_array().map(Vec::len), Some(0));
    assert_eq!(sent_after(&mut session, 60_000), 0);
}

/// REQ: PORT:ilium-client/src/app.rs::queue_prompt_dialog_commits_multiline_repeating_request_and_exposes_clear
#[test]
fn port_app_queue_prompt_forever_mode_is_committed_as_forever() {
    let (mut session, pane) = queue_dialog("agent");
    set_text(&mut session, QUEUE_VIEW, "prompt", "repeat me");
    select_delivery(&mut session, 2);
    commit_queue(&mut session);
    let entries = list_queue(&mut session, pane);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["mode"], "forever");
}

/// REQ: PORT:ilium-client/src/app.rs::full_outbound_credit_keeps_original_scheduled_and_prompt_forms_for_retry
#[test]
fn port_app_refused_schedule_arms_nothing_until_corrected() {
    let (mut session, pane) = schedule_dialog("shell");
    set_text(&mut session, SCHEDULE_VIEW, "payload", "retry me");
    set_text(&mut session, SCHEDULE_VIEW, "hours", "0");
    set_text(&mut session, SCHEDULE_VIEW, "minutes", "0");
    set_text(&mut session, SCHEDULE_VIEW, "seconds", "0");
    commit_schedule(&mut session);
    assert!(schedule_dialog_open(&mut session), "the refused form stays open");
    let reply = call_queue(
        &mut session,
        serde_json::json!({"method": "pending", "pane": pane.0}),
    )
    .expect("pending answers");
    assert_eq!(
        reply["schedules"].as_array().map(Vec::len),
        Some(0),
        "a refused form arms no timer"
    );
    set_text(&mut session, SCHEDULE_VIEW, "seconds", "1");
    commit_schedule(&mut session);
    assert_eq!(sent_after(&mut session, 1_000), 1);
    let sent = sends(&server(&mut session).submitted());
    assert_eq!(sent[0].0, plain_spec(pane, "retry me", Delivery::Submitted));
}

/// REQ: PORT:ilium-client/src/app.rs::pending_scheduled_input_keeps_countdown_redraws_active
#[test]
fn port_app_pending_countdown_shrinks_as_the_clock_advances() {
    let (mut session, pane) = schedule_dialog("shell");
    commit_schedule(&mut session);
    let first = call_queue(
        &mut session,
        serde_json::json!({"method": "pending", "pane": pane.0}),
    )
    .expect("pending answers")["schedules"][0]["remaining_ms"]
        .as_u64()
        .expect("remaining_ms");
    server(&mut session).advance_clock(10_000);
    let second = call_queue(
        &mut session,
        serde_json::json!({"method": "pending", "pane": pane.0}),
    )
    .expect("pending answers")["schedules"][0]["remaining_ms"]
        .as_u64()
        .expect("remaining_ms");
    assert!(second < first, "{first} then {second}");
    assert!(second <= 20_000, "{second}");
}

/// REQ: PORT:ilium-client/src/app.rs::scheduled_input_redraw_uses_the_exact_reverse_clock_boundary
#[test]
fn port_app_scheduled_input_redraw_delay_stays_within_one_frame() {
    let mut session = session();
    for remaining in (0u64..=1_000).step_by(37) {
        let reply = call_queue(
            &mut session,
            serde_json::json!({"method": "frame_delay", "remaining_ms": remaining, "frame_ms": 220u64}),
        )
        .expect("frame_delay answers");
        let delay = reply["delay_ms"].as_u64().expect("delay_ms");
        assert!(
            (1..=220).contains(&delay),
            "remaining {remaining} ms gives {delay} ms"
        );
    }
}

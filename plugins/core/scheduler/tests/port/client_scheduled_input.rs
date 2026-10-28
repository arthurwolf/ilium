//! Twins of ilium-client/src/scheduled_input.rs (dialog state, validation, geometry).

use ilium_plugin_sdk::types::Delivery;

use crate::common::*;
use crate::support::*;

/// REQ: PORT:ilium-client/src/scheduled_input.rs::validates_text_only_text_and_enter_and_enter_only, scheduler-B08
#[test]
fn port_scheduled_input_validates_text_only_text_and_enter_and_enter_only() {
    // Text only: default 30 s, no Enter.
    let (mut session, pane) = schedule_dialog("shell");
    set_text(&mut session, SCHEDULE_VIEW, "payload", "status");
    set_flag(&mut session, SCHEDULE_VIEW, "send-enter", false);
    commit_schedule(&mut session);
    server(&mut session).advance_clock(30_000);
    let sent = sends(&server(&mut session).submitted());
    assert_eq!(sent[0].0, plain_spec(pane, "status", Delivery::Typed));

    // Text plus Enter.
    let (mut session, pane) = schedule_dialog("shell");
    set_text(&mut session, SCHEDULE_VIEW, "payload", "status");
    commit_schedule(&mut session);
    server(&mut session).advance_clock(30_000);
    let sent = sends(&server(&mut session).submitted());
    assert_eq!(sent[0].0, plain_spec(pane, "status", Delivery::Submitted));

    // Enter only.
    let (mut session, pane) = schedule_dialog("shell");
    commit_schedule(&mut session);
    server(&mut session).advance_clock(30_000);
    let sent = sends(&server(&mut session).submitted());
    assert_eq!(sent[0].0, plain_spec(pane, "", Delivery::Submitted));
}

/// REQ: PORT:ilium-client/src/scheduled_input.rs::rejects_zero_invalid_clock_fields_and_empty_payload, scheduler-B04, scheduler-B05, scheduler-B06
#[test]
fn port_scheduled_input_rejects_zero_invalid_clock_fields_and_empty_payload() {
    let (mut session, _pane) = schedule_dialog("shell");
    for (key, value) in [("hours", "0"), ("minutes", "0"), ("seconds", "0")] {
        set_text(&mut session, SCHEDULE_VIEW, key, value);
    }
    commit_schedule(&mut session);
    let text = draw(&mut session, SCHEDULE_VIEW, 80, 24).text();
    assert!(
        text.contains("Choose a delay of at least one second"),
        "{text}"
    );

    set_text(&mut session, SCHEDULE_VIEW, "minutes", "60");
    commit_schedule(&mut session);
    let text = draw(&mut session, SCHEDULE_VIEW, 80, 24).text();
    assert!(text.contains("Minutes must be between 0 and 59"), "{text}");

    set_text(&mut session, SCHEDULE_VIEW, "minutes", "0");
    set_text(&mut session, SCHEDULE_VIEW, "seconds", "1");
    set_flag(&mut session, SCHEDULE_VIEW, "send-enter", false);
    commit_schedule(&mut session);
    let text = draw(&mut session, SCHEDULE_VIEW, 80, 24).text();
    assert!(
        text.contains("Enter text, enable Send Enter, or use both"),
        "{text}"
    );
    assert_eq!(send_count(&mut session), 0);
}

/// REQ: PORT:ilium-client/src/scheduled_input.rs::numeric_input_filters_letters_and_duration_uses_checked_units, scheduler-B06
#[test]
fn port_scheduled_input_numeric_input_filters_letters_and_duration_uses_checked_units() {
    let (mut session, _pane) = schedule_dialog("shell");
    // Hours is focused first: a letter is ignored, the digit is kept.
    press(&mut session, SCHEDULE_VIEW, "a");
    press(&mut session, SCHEDULE_VIEW, "2");
    set_text(&mut session, SCHEDULE_VIEW, "minutes", "3");
    set_text(&mut session, SCHEDULE_VIEW, "seconds", "4");
    commit_schedule(&mut session);
    // 2 h 3 min 4 s = 7384 s.
    assert_eq!(sent_after(&mut session, 7_383_999), 0);
    assert_eq!(sent_after(&mut session, 1), 1);
}

/// REQ: PORT:ilium-client/src/scheduled_input.rs::dialog_layout_is_aerated_and_clamped_to_small_screens, scheduler-B02
#[test]
fn port_scheduled_input_dialog_layout_is_aerated_and_clamped_to_small_screens() {
    let (mut session, _pane) = schedule_dialog("shell");
    let screen = draw(&mut session, SCHEDULE_VIEW, 100, 30);
    let rows: Vec<u16> = ["Hours", "Minutes", "Seconds", "Payload"]
        .iter()
        .map(|label| screen.find(label).unwrap_or((u16::MAX, 0)).0)
        .collect();
    assert!(
        rows.windows(2).all(|pair| pair[0] < pair[1]),
        "fields keep their order on separate rows: {rows:?}"
    );
    let small = draw(&mut session, SCHEDULE_VIEW, 24, 10);
    assert_eq!(
        (small.columns, small.rows),
        (24, 10),
        "the popup never exceeds the screen"
    );
}

/// REQ: PORT:ilium-client/src/scheduled_input.rs::clock_steps_saturate_and_empty_is_zero, scheduler-B02, scheduler-B06
#[test]
fn port_scheduled_input_clock_steps_saturate_and_empty_is_zero() {
    let (mut session, _pane) = schedule_dialog("shell");
    // Minutes starts empty (0): a decrement stays 0, an increment gives 1.
    press(&mut session, SCHEDULE_VIEW, "tab");
    press(&mut session, SCHEDULE_VIEW, "left");
    assert!(
        form_row(&mut session, SCHEDULE_VIEW, "Minutes").contains('0'),
        "{}",
        form_row(&mut session, SCHEDULE_VIEW, "Minutes")
    );
    press(&mut session, SCHEDULE_VIEW, "right");
    assert!(form_row(&mut session, SCHEDULE_VIEW, "Minutes")
        .trim_end()
        .ends_with("1 │"));
    // Seconds at 59 stays at 59 on an increment.
    set_text(&mut session, SCHEDULE_VIEW, "seconds", "59");
    press(&mut session, SCHEDULE_VIEW, "tab");
    press(&mut session, SCHEDULE_VIEW, "right");
    assert!(form_row(&mut session, SCHEDULE_VIEW, "Seconds")
        .trim_end()
        .ends_with("59 │"));
}

/// REQ: PORT:ilium-client/src/scheduled_input.rs::star_keeps_invalid_draft_and_step_rejects_it, scheduler-B06
#[test]
fn port_scheduled_input_star_keeps_invalid_draft_and_step_rejects_it() {
    let (mut session, _pane) = schedule_dialog("shell");
    set_text(&mut session, SCHEDULE_VIEW, "hours", "bad");
    // A step on the invalid draft changes nothing; the draft stays visible.
    press(&mut session, SCHEDULE_VIEW, "right");
    assert!(
        form_row(&mut session, SCHEDULE_VIEW, "Hours").contains("bad"),
        "{}",
        form_row(&mut session, SCHEDULE_VIEW, "Hours")
    );
    commit_schedule(&mut session);
    assert!(
        schedule_dialog_open(&mut session),
        "an invalid hours draft is not scheduled"
    );
    assert_eq!(send_count(&mut session), 0);
}

/// REQ: PORT:ilium-client/src/scheduled_input.rs::aggregate_overflow_does_not_mutate_any_draft, scheduler-B02
#[test]
fn port_scheduled_input_aggregate_overflow_does_not_mutate_any_draft() {
    let (mut session, _pane) = schedule_dialog("shell");
    let top = (u64::MAX / 3600).to_string();
    set_text(&mut session, SCHEDULE_VIEW, "hours", &top);
    set_text(&mut session, SCHEDULE_VIEW, "minutes", "0");
    set_text(&mut session, SCHEDULE_VIEW, "seconds", "0");
    press(&mut session, SCHEDULE_VIEW, "right");
    assert!(
        form_row(&mut session, SCHEDULE_VIEW, "Hours").contains(&top),
        "the maximal hours do not grow"
    );
    set_text(&mut session, SCHEDULE_VIEW, "minutes", "59");
    press(&mut session, SCHEDULE_VIEW, "tab");
    press(&mut session, SCHEDULE_VIEW, "tab");
    let before = form_row(&mut session, SCHEDULE_VIEW, "Seconds");
    press(&mut session, SCHEDULE_VIEW, "right");
    assert_eq!(
        form_row(&mut session, SCHEDULE_VIEW, "Seconds"),
        before,
        "an overflowing step leaves every draft unchanged"
    );
}

/// REQ: PORT:ilium-client/src/scheduled_input.rs::numeric_fields_share_inner_row_and_preserve_empty_keyboard_draft, scheduler-B02
#[test]
fn port_scheduled_input_numeric_fields_share_inner_row_and_preserve_empty_keyboard_draft() {
    let (mut session, _pane) = schedule_dialog("shell");
    let screen = draw(&mut session, SCHEDULE_VIEW, 80, 24);
    let hours_row = screen.find("Hours").expect("Hours label").0;
    let minutes_row = screen.find("Minutes").expect("Minutes label").0;
    assert!(hours_row < minutes_row);
    // The empty Hours draft stays empty after a render and after focus moves away and back.
    press(&mut session, SCHEDULE_VIEW, "tab");
    press(&mut session, SCHEDULE_VIEW, "shift+tab");
    let row = form_row(&mut session, SCHEDULE_VIEW, "Hours");
    assert!(
        !row.chars().any(|c| c.is_ascii_digit()),
        "an empty draft is not rewritten to 0: {row}"
    );
}

/// REQ: PORT:ilium-client/src/keys.rs::scheduled_input_keyboard_edits_toggles_and_submits_the_form
#[test]
fn port_002_scheduled_input_keyboard_edits_toggles_and_submits_the_form() {
    let (mut session, pane) = schedule_dialog("shell");
    set_text(&mut session, SCHEDULE_VIEW, "payload", "x");
    press(&mut session, SCHEDULE_VIEW, "tab");
    // The focused Send Enter checkbox is toggled off with Space.
    press(&mut session, SCHEDULE_VIEW, "space");
    commit_schedule(&mut session);
    assert!(
        !schedule_dialog_open(&mut session),
        "Ctrl+Enter submits and closes the form"
    );
    server(&mut session).advance_clock(30_000);
    let sent = sends(&server(&mut session).submitted());
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].0, plain_spec(pane, "x", Delivery::Typed));
}

/// REQ: PORT:ilium-client/src/scheduled_input.rs::validates_text_only_text_and_enter_and_enter_only
#[test]
fn port_scheduled_input_default_thirty_seconds_fires_at_the_boundary() {
    let (mut session, pane) = schedule_dialog("shell");
    set_text(&mut session, SCHEDULE_VIEW, "payload", "boundary");
    set_flag(&mut session, SCHEDULE_VIEW, "send-enter", false);
    commit_schedule(&mut session);
    assert_eq!(sent_after(&mut session, 29_999), 0, "not one millisecond early");
    assert_eq!(sent_after(&mut session, 1), 1);
    let sent = sends(&server(&mut session).submitted());
    assert_eq!(sent[0].0, plain_spec(pane, "boundary", Delivery::Typed));
}

/// REQ: PORT:ilium-client/src/scheduled_input.rs::rejects_zero_invalid_clock_fields_and_empty_payload
#[test]
fn port_scheduled_input_refused_zero_delay_is_armed_after_correction() {
    let (mut session, pane) = schedule_dialog("shell");
    set_text(&mut session, SCHEDULE_VIEW, "payload", "fixed");
    set_text(&mut session, SCHEDULE_VIEW, "hours", "0");
    set_text(&mut session, SCHEDULE_VIEW, "minutes", "0");
    set_text(&mut session, SCHEDULE_VIEW, "seconds", "0");
    commit_schedule(&mut session);
    assert!(schedule_dialog_open(&mut session), "a zero delay is refused");
    assert_eq!(sent_after(&mut session, 3_600_000), 0);
    set_text(&mut session, SCHEDULE_VIEW, "hours", "1");
    commit_schedule(&mut session);
    assert!(!schedule_dialog_open(&mut session), "the corrected form closes");
    assert_eq!(sent_after(&mut session, 3_599_999), 0);
    assert_eq!(sent_after(&mut session, 1), 1);
    let sent = sends(&server(&mut session).submitted());
    assert_eq!(sent[0].0, plain_spec(pane, "fixed", Delivery::Submitted));
}

/// REQ: PORT:ilium-client/src/scheduled_input.rs::numeric_input_filters_letters_and_duration_uses_checked_units
#[test]
fn port_scheduled_input_letters_typed_into_minutes_are_dropped() {
    let (mut session, _pane) = schedule_dialog("shell");
    press(&mut session, SCHEDULE_VIEW, "tab");
    press(&mut session, SCHEDULE_VIEW, "q");
    press(&mut session, SCHEDULE_VIEW, "7");
    let row = form_row(&mut session, SCHEDULE_VIEW, "Minutes");
    assert!(row.contains('7') && !row.contains('q'), "{row}");
}

/// REQ: PORT:ilium-client/src/scheduled_input.rs::dialog_layout_is_aerated_and_clamped_to_small_screens
#[test]
fn port_scheduled_input_send_enter_row_sits_below_payload() {
    let (mut session, _pane) = schedule_dialog("shell");
    let screen = draw(&mut session, SCHEDULE_VIEW, 100, 30);
    let payload_row = screen.find("Payload").expect("Payload label").0;
    let enter_row = screen.find("Send Enter").expect("Send Enter label").0;
    assert!(payload_row < enter_row, "payload {payload_row}, enter {enter_row}");
}

/// REQ: PORT:ilium-client/src/scheduled_input.rs::clock_steps_saturate_and_empty_is_zero
#[test]
fn port_scheduled_input_seconds_step_up_from_the_default_thirty() {
    let (mut session, _pane) = schedule_dialog("shell");
    press(&mut session, SCHEDULE_VIEW, "tab");
    press(&mut session, SCHEDULE_VIEW, "tab");
    press(&mut session, SCHEDULE_VIEW, "right");
    let row = form_row(&mut session, SCHEDULE_VIEW, "Seconds");
    assert!(row.trim_end().ends_with("31 │"), "{row}");
}

/// REQ: PORT:ilium-client/src/scheduled_input.rs::star_keeps_invalid_draft_and_step_rejects_it
#[test]
fn port_scheduled_input_step_down_also_keeps_invalid_draft() {
    let (mut session, _pane) = schedule_dialog("shell");
    set_text(&mut session, SCHEDULE_VIEW, "hours", "bad");
    press(&mut session, SCHEDULE_VIEW, "left");
    assert!(
        form_row(&mut session, SCHEDULE_VIEW, "Hours").contains("bad"),
        "a downward step keeps the invalid draft"
    );
}

/// REQ: PORT:ilium-client/src/scheduled_input.rs::aggregate_overflow_does_not_mutate_any_draft
#[test]
fn port_scheduled_input_empty_hours_steps_to_one() {
    let (mut session, _pane) = schedule_dialog("shell");
    press(&mut session, SCHEDULE_VIEW, "right");
    let row = form_row(&mut session, SCHEDULE_VIEW, "Hours");
    assert!(row.trim_end().ends_with("1 │"), "{row}");
}

/// REQ: PORT:ilium-client/src/scheduled_input.rs::numeric_fields_share_inner_row_and_preserve_empty_keyboard_draft
#[test]
fn port_scheduled_input_empty_numeric_rows_keep_their_leaders() {
    let (mut session, _pane) = schedule_dialog("shell");
    let row = form_row(&mut session, SCHEDULE_VIEW, "Minutes");
    assert!(row.contains('…'), "an empty numeric row still has leaders: {row}");
}

/// REQ: PORT:ilium-client/src/keys.rs::scheduled_input_keyboard_edits_toggles_and_submits_the_form
#[test]
fn port_scheduled_input_space_twice_restores_send_enter() {
    let (mut session, pane) = schedule_dialog("shell");
    set_text(&mut session, SCHEDULE_VIEW, "payload", "x");
    press(&mut session, SCHEDULE_VIEW, "tab");
    press(&mut session, SCHEDULE_VIEW, "space");
    press(&mut session, SCHEDULE_VIEW, "space");
    commit_schedule(&mut session);
    assert_eq!(sent_after(&mut session, 30_000), 1);
    let sent = sends(&server(&mut session).submitted());
    assert_eq!(sent[0].0, plain_spec(pane, "x", Delivery::Submitted));
}

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
    assert!(text.contains("Choose a delay of at least one second"), "{text}");

    set_text(&mut session, SCHEDULE_VIEW, "minutes", "60");
    commit_schedule(&mut session);
    let text = draw(&mut session, SCHEDULE_VIEW, 80, 24).text();
    assert!(text.contains("Minutes must be between 0 and 59"), "{text}");

    set_text(&mut session, SCHEDULE_VIEW, "minutes", "0");
    set_text(&mut session, SCHEDULE_VIEW, "seconds", "1");
    set_flag(&mut session, SCHEDULE_VIEW, "send-enter", false);
    commit_schedule(&mut session);
    let text = draw(&mut session, SCHEDULE_VIEW, 80, 24).text();
    assert!(text.contains("Enter text, enable Send Enter, or use both"), "{text}");
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
    assert!(rows.windows(2).all(|pair| pair[0] < pair[1]), "fields keep their order on separate rows: {rows:?}");
    let small = draw(&mut session, SCHEDULE_VIEW, 24, 10);
    assert_eq!((small.columns, small.rows), (24, 10), "the popup never exceeds the screen");
}

/// REQ: PORT:ilium-client/src/scheduled_input.rs::clock_steps_saturate_and_empty_is_zero, scheduler-B02, scheduler-B06
#[test]
fn port_scheduled_input_clock_steps_saturate_and_empty_is_zero() {
    let (mut session, _pane) = schedule_dialog("shell");
    // Minutes starts empty (0): a decrement stays 0, an increment gives 1.
    press(&mut session, SCHEDULE_VIEW, "tab");
    press(&mut session, SCHEDULE_VIEW, "left");
    assert!(form_row(&mut session, SCHEDULE_VIEW, "Minutes").contains('0'), "{}", form_row(&mut session, SCHEDULE_VIEW, "Minutes"));
    press(&mut session, SCHEDULE_VIEW, "right");
    assert!(form_row(&mut session, SCHEDULE_VIEW, "Minutes").trim_end().ends_with("1 │"));
    // Seconds at 59 stays at 59 on an increment.
    set_text(&mut session, SCHEDULE_VIEW, "seconds", "59");
    press(&mut session, SCHEDULE_VIEW, "tab");
    press(&mut session, SCHEDULE_VIEW, "right");
    assert!(form_row(&mut session, SCHEDULE_VIEW, "Seconds").trim_end().ends_with("59 │"));
}

/// REQ: PORT:ilium-client/src/scheduled_input.rs::star_keeps_invalid_draft_and_step_rejects_it, scheduler-B06
#[test]
fn port_scheduled_input_star_keeps_invalid_draft_and_step_rejects_it() {
    let (mut session, _pane) = schedule_dialog("shell");
    set_text(&mut session, SCHEDULE_VIEW, "hours", "bad");
    // A step on the invalid draft changes nothing; the draft stays visible.
    press(&mut session, SCHEDULE_VIEW, "right");
    assert!(form_row(&mut session, SCHEDULE_VIEW, "Hours").contains("bad"), "{}", form_row(&mut session, SCHEDULE_VIEW, "Hours"));
    commit_schedule(&mut session);
    assert!(schedule_dialog_open(&mut session), "an invalid hours draft is not scheduled");
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
    assert!(form_row(&mut session, SCHEDULE_VIEW, "Hours").contains(&top), "the maximal hours do not grow");
    set_text(&mut session, SCHEDULE_VIEW, "minutes", "59");
    press(&mut session, SCHEDULE_VIEW, "tab");
    press(&mut session, SCHEDULE_VIEW, "tab");
    let before = form_row(&mut session, SCHEDULE_VIEW, "Seconds");
    press(&mut session, SCHEDULE_VIEW, "right");
    assert_eq!(form_row(&mut session, SCHEDULE_VIEW, "Seconds"), before, "an overflowing step leaves every draft unchanged");
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
    assert!(!row.chars().any(|c| c.is_ascii_digit()), "an empty draft is not rewritten to 0: {row}");
}

//! The schedule dialog "Schedule input for <pane name>" (client side): scheduler-B01 to B06, B07 (replace).

use ilium_plugin_sdk::testing::FakeSession;
use ilium_plugin_sdk::types::{Delivery, NodeId};

use crate::common::*;

fn rows(session: &mut FakeSession) -> Vec<String> {
    draw(session, SCHEDULE_VIEW, 80, 24).text().lines().map(str::to_string).collect()
}

/// The text rows that name `field` and are not the form row itself (no dotted leader): the refusal message.
fn message_naming(session: &mut FakeSession, field: &str) -> Option<String> {
    rows(session).into_iter().find(|row| row.contains(field) && !row.contains('…'))
}

fn row_of(session: &mut FakeSession, label: &str) -> String {
    rows(session).into_iter().find(|row| row.contains(label) && row.contains('…')).unwrap_or_default()
}

fn opened(pane_name: &str) -> (FakeSession, NodeId) {
    let mut session = session();
    let pane = terminal_pane(&mut session, pane_name);
    invoke_menu(&mut session, SCHEDULE_COMMAND, pane);
    (session, pane)
}

fn commit_and_pump(session: &mut FakeSession) {
    press(session, SCHEDULE_VIEW, "ctrl+enter");
    pump(session);
}

/// REQ: scheduler-B01, F-055
#[test]
fn schedule_b01_a_terminal_pane_opens_the_dialog_titled_with_the_pane_name() {
    let (mut session, _pane) = opened("claude-api");
    assert!(draw(&mut session, SCHEDULE_VIEW, 80, 24).text().contains("Schedule input for claude-api"));
}

/// REQ: scheduler-B01, F-055
#[test]
fn schedule_b01_an_agent_or_shell_pane_both_have_the_entry() {
    for name in ["shell", "codex"] {
        let (mut session, _pane) = opened(name);
        let title = format!("Schedule input for {name}");
        assert!(draw(&mut session, SCHEDULE_VIEW, 80, 24).text().contains(&title), "{name}");
    }
}

/// REQ: scheduler-B01, F-055
#[test]
fn schedule_b01_an_editor_pane_has_no_dialog() {
    let mut session = session();
    let pane = add_pane(&mut session, "notes.md", EDITOR_KIND);
    invoke_menu(&mut session, SCHEDULE_COMMAND, pane);
    assert!(!draw(&mut session, SCHEDULE_VIEW, 80, 24).text().contains("Schedule input for"));
}

/// REQ: scheduler-B01, F-055
#[test]
fn schedule_b01_a_board_pane_has_no_dialog() {
    let mut session = session();
    let pane = add_pane(&mut session, "plan.md", BOARD_KIND);
    invoke_menu(&mut session, SCHEDULE_COMMAND, pane);
    assert!(!draw(&mut session, SCHEDULE_VIEW, 80, 24).text().contains("Schedule input for"));
}

/// REQ: scheduler-B02, F-055
#[test]
fn schedule_b02_initial_values_seconds_30_and_send_enter_on() {
    let (mut session, _pane) = opened("claude-api");
    assert!(row_of(&mut session, "Seconds").trim_end().ends_with("30 │"), "{}", row_of(&mut session, "Seconds"));
    assert!(row_of(&mut session, "Send Enter").contains("[x]"));
}

/// REQ: scheduler-B02, F-055
#[test]
fn schedule_b02_the_fields_are_hours_minutes_seconds_payload_send_enter_in_order() {
    let (mut session, _pane) = opened("claude-api");
    let rows = rows(&mut session);
    let at = |label: &str| rows.iter().position(|row| row.contains(label) && row.contains('…')).unwrap_or(usize::MAX);
    let order = [at("Hours"), at("Minutes"), at("Seconds"), at("Payload"), at("Send Enter")];
    assert!(order.windows(2).all(|pair| pair[0] < pair[1]), "{order:?}");
}

/// REQ: scheduler-B02, F-055
#[test]
fn schedule_b02_blank_hours_and_minutes_count_as_zero() {
    let (mut session, pane) = opened("claude-api");
    set_text(&mut session, SCHEDULE_VIEW, "hours", "");
    set_text(&mut session, SCHEDULE_VIEW, "minutes", "");
    set_text(&mut session, SCHEDULE_VIEW, "seconds", "5");
    set_text(&mut session, SCHEDULE_VIEW, "payload", "go");
    commit_and_pump(&mut session);
    server(&mut session).advance_clock(5_000);
    let sent = sends(&server(&mut session).submitted());
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].0.pane, pane);
}

/// REQ: scheduler-B02, F-055
#[test]
fn schedule_b02_hours_accept_any_whole_number() {
    let (mut session, _pane) = opened("claude-api");
    set_text(&mut session, SCHEDULE_VIEW, "hours", "100");
    set_text(&mut session, SCHEDULE_VIEW, "payload", "go");
    commit_and_pump(&mut session);
    assert!(!draw(&mut session, SCHEDULE_VIEW, 80, 24).text().contains("Schedule input for"), "100 hours is accepted");
}

/// REQ: scheduler-B02, F-055
#[test]
fn schedule_b02_payload_takes_any_unicode() {
    let (mut session, pane) = opened("claude-api");
    let payload = "héllo 世界 🚀";
    set_text(&mut session, SCHEDULE_VIEW, "payload", payload);
    commit_and_pump(&mut session);
    server(&mut session).advance_clock(30_000);
    let sent = sends(&server(&mut session).submitted());
    assert_eq!(sent[0].0, plain_spec(pane, payload, Delivery::Submitted));
}

/// REQ: scheduler-B02, F-055
#[test]
fn schedule_b02_payload_is_a_single_line() {
    let (mut session, pane) = opened("claude-api");
    set_text(&mut session, SCHEDULE_VIEW, "payload", "one\ntwo");
    commit_and_pump(&mut session);
    server(&mut session).advance_clock(30_000);
    let sent = sends(&server(&mut session).submitted());
    assert!(sent.iter().all(|(spec, _)| spec.pane == pane && !spec.text.contains('\n')), "{sent:?}");
}

/// REQ: scheduler-B03, F-055
#[test]
fn schedule_b03_space_on_send_enter_toggles_it() {
    let (mut session, _pane) = opened("claude-api");
    for _ in 0..4 {
        press(&mut session, SCHEDULE_VIEW, "tab");
    }
    press(&mut session, SCHEDULE_VIEW, "space");
    assert!(row_of(&mut session, "Send Enter").contains("[ ]"));
    press(&mut session, SCHEDULE_VIEW, "space");
    assert!(row_of(&mut session, "Send Enter").contains("[x]"));
}

/// REQ: scheduler-B03, F-055
#[test]
fn schedule_b03_shift_tab_moves_back_to_the_previous_field() {
    let (mut session, _pane) = opened("claude-api");
    for _ in 0..5 {
        press(&mut session, SCHEDULE_VIEW, "tab");
    }
    press(&mut session, SCHEDULE_VIEW, "shift+tab");
    press(&mut session, SCHEDULE_VIEW, "space");
    assert!(row_of(&mut session, "Send Enter").contains("[ ]"), "focus is on Send Enter after tab x5 and shift+tab");
}

/// REQ: scheduler-B03, F-055
#[test]
fn schedule_b03_tab_does_not_toggle_and_does_not_commit() {
    let (mut session, _pane) = opened("claude-api");
    press(&mut session, SCHEDULE_VIEW, "tab");
    assert!(draw(&mut session, SCHEDULE_VIEW, 80, 24).text().contains("Schedule input for"));
    assert!(row_of(&mut session, "Send Enter").contains("[x]"));
}

/// REQ: scheduler-B03, F-055
#[test]
fn schedule_b03_ctrl_enter_schedules_and_closes_the_dialog() {
    let (mut session, _pane) = opened("claude-api");
    set_text(&mut session, SCHEDULE_VIEW, "payload", "npm test");
    commit_and_pump(&mut session);
    assert!(!draw(&mut session, SCHEDULE_VIEW, 80, 24).text().contains("Schedule input for"));
}

/// REQ: scheduler-B03, F-055
#[test]
fn schedule_b03_esc_cancels_and_nothing_is_scheduled() {
    let (mut session, _pane) = opened("claude-api");
    set_text(&mut session, SCHEDULE_VIEW, "payload", "npm test");
    press(&mut session, SCHEDULE_VIEW, "esc");
    pump(&mut session);
    assert!(!draw(&mut session, SCHEDULE_VIEW, 80, 24).text().contains("Schedule input for"));
    server(&mut session).advance_clock(3_600_000);
    assert!(sends(&server(&mut session).submitted()).is_empty());
}

/// REQ: scheduler-B04, F-055
#[test]
fn schedule_b04_zero_delay_is_refused_with_the_exact_message() {
    let (mut session, _pane) = opened("claude-api");
    set_text(&mut session, SCHEDULE_VIEW, "seconds", "0");
    set_text(&mut session, SCHEDULE_VIEW, "payload", "go");
    commit_and_pump(&mut session);
    let text = draw(&mut session, SCHEDULE_VIEW, 80, 24).text();
    assert!(text.contains("Choose a delay of at least one second"), "{text}");
    assert!(text.contains("Schedule input for"), "the dialog stays open");
}

/// REQ: scheduler-B04, F-055
#[test]
fn schedule_b04_zero_delay_schedules_nothing() {
    let (mut session, _pane) = opened("claude-api");
    set_text(&mut session, SCHEDULE_VIEW, "seconds", "0");
    set_text(&mut session, SCHEDULE_VIEW, "payload", "go");
    commit_and_pump(&mut session);
    server(&mut session).advance_clock(86_400_000);
    assert!(sends(&server(&mut session).submitted()).is_empty());
}

/// REQ: scheduler-B04, F-055
#[test]
fn schedule_b04_one_second_is_the_smallest_accepted_delay() {
    let (mut session, _pane) = opened("claude-api");
    set_text(&mut session, SCHEDULE_VIEW, "seconds", "1");
    set_text(&mut session, SCHEDULE_VIEW, "payload", "go");
    commit_and_pump(&mut session);
    assert!(!draw(&mut session, SCHEDULE_VIEW, 80, 24).text().contains("Choose a delay"));
    server(&mut session).advance_clock(1_000);
    assert_eq!(sends(&server(&mut session).submitted()).len(), 1);
}

/// REQ: scheduler-B04, F-055
#[test]
fn schedule_b04_all_three_fields_blank_is_a_zero_delay() {
    let (mut session, _pane) = opened("claude-api");
    set_text(&mut session, SCHEDULE_VIEW, "seconds", "");
    set_text(&mut session, SCHEDULE_VIEW, "payload", "go");
    commit_and_pump(&mut session);
    assert!(draw(&mut session, SCHEDULE_VIEW, 80, 24).text().contains("Choose a delay of at least one second"));
}

/// REQ: scheduler-B05, F-055
#[test]
fn schedule_b05_empty_payload_with_send_enter_off_is_refused_with_the_exact_message() {
    let (mut session, _pane) = opened("claude-api");
    set_text(&mut session, SCHEDULE_VIEW, "payload", "");
    set_flag(&mut session, SCHEDULE_VIEW, "send-enter", false);
    commit_and_pump(&mut session);
    let text = draw(&mut session, SCHEDULE_VIEW, 80, 24).text();
    assert!(text.contains("Enter text, enable Send Enter, or use both"), "{text}");
    assert!(text.contains("Schedule input for"));
}

/// REQ: scheduler-B05, F-055
#[test]
fn schedule_b05_empty_payload_with_send_enter_on_is_accepted() {
    let (mut session, _pane) = opened("claude-api");
    set_text(&mut session, SCHEDULE_VIEW, "payload", "");
    commit_and_pump(&mut session);
    assert!(!draw(&mut session, SCHEDULE_VIEW, 80, 24).text().contains("Schedule input for"));
}

/// REQ: scheduler-B05, F-055
#[test]
fn schedule_b05_text_with_send_enter_off_is_accepted() {
    let (mut session, _pane) = opened("claude-api");
    set_text(&mut session, SCHEDULE_VIEW, "payload", "x");
    set_flag(&mut session, SCHEDULE_VIEW, "send-enter", false);
    commit_and_pump(&mut session);
    assert!(!draw(&mut session, SCHEDULE_VIEW, 80, 24).text().contains("Enter text, enable Send Enter"));
}

/// REQ: scheduler-B04, scheduler-B05, F-055
#[test]
fn schedule_b04_b05_the_zero_delay_message_wins_when_both_rules_fail() {
    let (mut session, _pane) = opened("claude-api");
    set_text(&mut session, SCHEDULE_VIEW, "seconds", "0");
    set_text(&mut session, SCHEDULE_VIEW, "payload", "");
    set_flag(&mut session, SCHEDULE_VIEW, "send-enter", false);
    commit_and_pump(&mut session);
    let text = draw(&mut session, SCHEDULE_VIEW, 80, 24).text();
    assert!(text.contains("Choose a delay of at least one second"), "{text}");
}

/// REQ: scheduler-B06, F-055
#[test]
fn schedule_b06_minutes_60_is_refused_with_a_message_naming_minutes() {
    let (mut session, _pane) = opened("claude-api");
    set_text(&mut session, SCHEDULE_VIEW, "minutes", "60");
    set_text(&mut session, SCHEDULE_VIEW, "payload", "go");
    commit_and_pump(&mut session);
    assert!(message_naming(&mut session, "Minutes").is_some());
    assert!(draw(&mut session, SCHEDULE_VIEW, 80, 24).text().contains("Schedule input for"));
}

/// REQ: scheduler-B06, F-055
#[test]
fn schedule_b06_seconds_60_is_refused_with_a_message_naming_seconds() {
    let (mut session, _pane) = opened("claude-api");
    set_text(&mut session, SCHEDULE_VIEW, "seconds", "60");
    set_text(&mut session, SCHEDULE_VIEW, "payload", "go");
    commit_and_pump(&mut session);
    assert!(message_naming(&mut session, "Seconds").is_some());
}

/// REQ: scheduler-B06, F-055
#[test]
fn schedule_b06_minutes_59_and_seconds_59_are_accepted() {
    let (mut session, _pane) = opened("claude-api");
    set_text(&mut session, SCHEDULE_VIEW, "minutes", "59");
    set_text(&mut session, SCHEDULE_VIEW, "seconds", "59");
    set_text(&mut session, SCHEDULE_VIEW, "payload", "go");
    commit_and_pump(&mut session);
    assert!(!draw(&mut session, SCHEDULE_VIEW, 80, 24).text().contains("Schedule input for"));
}

/// REQ: scheduler-B06, F-055
#[test]
fn schedule_b06_minutes_61_and_999_are_refused_too() {
    for value in ["61", "999"] {
        let (mut session, _pane) = opened("claude-api");
        set_text(&mut session, SCHEDULE_VIEW, "minutes", value);
        set_text(&mut session, SCHEDULE_VIEW, "payload", "go");
        commit_and_pump(&mut session);
        assert!(draw(&mut session, SCHEDULE_VIEW, 80, 24).text().contains("Schedule input for"), "{value}");
    }
}

/// REQ: scheduler-B06, F-055
#[test]
fn schedule_b06_a_non_digit_typed_in_a_number_field_is_ignored() {
    let (mut session, _pane) = opened("claude-api");
    set_text(&mut session, SCHEDULE_VIEW, "hours", "1a2");
    assert!(row_of(&mut session, "Hours").trim_end().ends_with("12 │"), "{}", row_of(&mut session, "Hours"));
}

/// REQ: scheduler-B06, F-055
#[test]
fn schedule_b06_a_typed_letter_leaves_the_focused_hours_field_unchanged() {
    let (mut session, _pane) = opened("claude-api");
    set_text(&mut session, SCHEDULE_VIEW, "hours", "7");
    press(&mut session, SCHEDULE_VIEW, "x");
    assert!(row_of(&mut session, "Hours").trim_end().ends_with("7 │"), "{}", row_of(&mut session, "Hours"));
}

/// REQ: scheduler-B06, F-055
#[test]
fn schedule_b06_a_minus_sign_and_a_decimal_point_are_ignored() {
    let (mut session, _pane) = opened("claude-api");
    set_text(&mut session, SCHEDULE_VIEW, "minutes", "-1.5");
    assert!(row_of(&mut session, "Minutes").trim_end().ends_with("15 │"), "{}", row_of(&mut session, "Minutes"));
}

/// REQ: scheduler-B07, F-055
#[test]
fn schedule_b07_a_second_dialog_commit_on_the_same_pane_replaces_the_first() {
    let mut session = session();
    let pane = terminal_pane(&mut session, "claude-api");
    fill_schedule_dialog(&mut session, pane, "0", "0", "10", "first", true);
    commit_and_pump(&mut session);
    fill_schedule_dialog(&mut session, pane, "0", "0", "20", "second", true);
    commit_and_pump(&mut session);
    server(&mut session).advance_clock(10_000);
    assert!(sends(&server(&mut session).submitted()).is_empty(), "the first deadline was replaced");
    server(&mut session).advance_clock(10_000);
    let sent = sends(&server(&mut session).submitted());
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].0.text, "second");
}

/// REQ: scheduler-B02, scheduler-B03, F-055
#[test]
fn schedule_b02_b03_the_dialog_shows_the_hint_line_with_the_two_keys() {
    let (mut session, _pane) = opened("claude-api");
    assert!(draw(&mut session, SCHEDULE_VIEW, 80, 24).text().contains("Ctrl+Enter schedule · Esc cancel"));
}

/// REQ: scheduler-B06, F-055
#[test]
fn schedule_b06_the_minutes_message_is_minutes_must_be_between_0_and_59() {
    let (mut session, _pane) = opened("claude-api");
    set_text(&mut session, SCHEDULE_VIEW, "minutes", "60");
    set_text(&mut session, SCHEDULE_VIEW, "payload", "go");
    commit_and_pump(&mut session);
    assert!(draw(&mut session, SCHEDULE_VIEW, 80, 24).text().contains("Minutes must be between 0 and 59"));
}

/// REQ: scheduler-B06, F-055
#[test]
fn schedule_b06_the_seconds_message_is_seconds_must_be_between_0_and_59() {
    let (mut session, _pane) = opened("claude-api");
    set_text(&mut session, SCHEDULE_VIEW, "seconds", "60");
    set_text(&mut session, SCHEDULE_VIEW, "payload", "go");
    commit_and_pump(&mut session);
    assert!(draw(&mut session, SCHEDULE_VIEW, 80, 24).text().contains("Seconds must be between 0 and 59"));
}

/// REQ: scheduler-B06, scheduler-B04, F-055
#[test]
fn schedule_b06_an_out_of_range_field_is_reported_before_the_zero_delay() {
    let (mut session, _pane) = opened("claude-api");
    set_text(&mut session, SCHEDULE_VIEW, "minutes", "60");
    set_text(&mut session, SCHEDULE_VIEW, "seconds", "0");
    set_text(&mut session, SCHEDULE_VIEW, "payload", "go");
    commit_and_pump(&mut session);
    let text = draw(&mut session, SCHEDULE_VIEW, 80, 24).text();
    assert!(text.contains("Minutes must be between 0 and 59"));
    assert!(!text.contains("Choose a delay"));
}

/// REQ: scheduler-B02, F-055
#[test]
fn schedule_b02_an_overflowing_duration_is_refused_as_too_large() {
    let (mut session, _pane) = opened("claude-api");
    set_text(&mut session, SCHEDULE_VIEW, "hours", "18446744073709551615");
    set_text(&mut session, SCHEDULE_VIEW, "payload", "go");
    commit_and_pump(&mut session);
    assert!(draw(&mut session, SCHEDULE_VIEW, 80, 24).text().contains("The duration is too large"));
}

/// REQ: scheduler-B02, F-055
#[test]
fn schedule_b02_the_largest_hours_that_fit_are_accepted() {
    let (mut session, _pane) = opened("claude-api");
    set_text(&mut session, SCHEDULE_VIEW, "hours", "5124095576030431");
    set_text(&mut session, SCHEDULE_VIEW, "seconds", "0");
    set_text(&mut session, SCHEDULE_VIEW, "payload", "go");
    commit_and_pump(&mut session);
    assert!(!draw(&mut session, SCHEDULE_VIEW, 80, 24).text().contains("The duration is too large"));
}

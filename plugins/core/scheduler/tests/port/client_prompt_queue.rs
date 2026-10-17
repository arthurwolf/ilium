//! Twins of ilium-client/src/prompt_queue.rs (repeat count, delivery control, compact layout).

use crate::common::*;
use crate::support::*;

fn times_dialog() -> (ilium_plugin_sdk::testing::FakeSession, ilium_plugin_sdk::types::NodeId) {
    let (mut session, pane) = queue_dialog("agent");
    select_delivery(&mut session, 1);
    (session, pane)
}

fn times_value(session: &mut ilium_plugin_sdk::testing::FakeSession) -> String {
    form_row(session, QUEUE_VIEW, "Times").trim_end_matches('│').trim().rsplit(' ').next().unwrap_or_default().to_string()
}

/// REQ: PORT:ilium-client/src/prompt_queue.rs::repeat_count_buttons_step_and_saturate_without_submitting, scheduler-B12
#[test]
fn port_prompt_queue_repeat_count_buttons_step_and_saturate_without_submitting() {
    let (mut session, pane) = times_dialog();
    press(&mut session, QUEUE_VIEW, "tab");
    press(&mut session, QUEUE_VIEW, "tab");
    press(&mut session, QUEUE_VIEW, "right");
    assert_eq!(times_value(&mut session), "3");
    press(&mut session, QUEUE_VIEW, "left");
    assert_eq!(times_value(&mut session), "2");
    set_text(&mut session, QUEUE_VIEW, "times", "1");
    press(&mut session, QUEUE_VIEW, "left");
    assert_eq!(times_value(&mut session), "1", "the count never goes below one");
    set_text(&mut session, QUEUE_VIEW, "times", &u32::MAX.to_string());
    press(&mut session, QUEUE_VIEW, "right");
    assert_eq!(times_value(&mut session), u32::MAX.to_string(), "the count saturates at u32::MAX");
    assert!(queue_dialog_open(&mut session), "stepping never submits");
    assert!(list_queue(&mut session, pane).is_empty());
}

/// REQ: PORT:ilium-client/src/prompt_queue.rs::direct_entry_focus_preserves_draft_and_invalid_numbers, scheduler-B12
#[test]
fn port_prompt_queue_direct_entry_focus_preserves_draft_and_invalid_numbers() {
    let (mut session, pane) = times_dialog();
    set_text(&mut session, QUEUE_VIEW, "times", "not a count");
    set_text(&mut session, QUEUE_VIEW, "prompt", "p");
    press(&mut session, QUEUE_VIEW, "right");
    assert!(form_row(&mut session, QUEUE_VIEW, "Times").contains("not a count"), "an invalid draft is kept");
    commit_queue(&mut session);
    assert!(list_queue(&mut session, pane).is_empty(), "an invalid count is not enqueued");
}

/// REQ: PORT:ilium-client/src/prompt_queue.rs::repeat_count_has_no_active_controls_outside_times_mode, scheduler-B12
#[test]
fn port_prompt_queue_repeat_count_has_no_active_controls_outside_times_mode() {
    let (mut session, _pane) = queue_dialog("agent");
    let before = form_row(&mut session, QUEUE_VIEW, "Times");
    for chord in ["tab", "tab", "right", "left", "enter"] {
        press(&mut session, QUEUE_VIEW, chord);
    }
    assert_eq!(form_row(&mut session, QUEUE_VIEW, "Times"), before, "the Times row is inert in Once mode");
}

/// REQ: PORT:ilium-client/src/prompt_queue.rs::repeat_control_geometry_uses_inner_row_and_centers_value, scheduler-B12
#[test]
fn port_prompt_queue_repeat_control_geometry_uses_inner_row_and_centers_value() {
    let (mut session, _pane) = times_dialog();
    let screen = draw(&mut session, QUEUE_VIEW, 48, 12);
    let (row, column) = screen.find("Times").expect("Times label");
    let line = screen.row(row);
    assert!(line.contains('…'), "a form row keeps its leaders: {line}");
    assert!(column > 0, "the row sits inside the popup border");
    assert!(line.trim_end().ends_with("2 │"), "the default count 2 is the last value before the border: {line}");
}

/// REQ: PORT:ilium-client/src/prompt_queue.rs::delivery_glyphs_value_clicks_and_option_button_share_rendered_cells, scheduler-B12
#[test]
fn port_prompt_queue_delivery_glyphs_value_clicks_and_option_button_share_rendered_cells() {
    let (mut session, pane) = queue_dialog("agent");
    let screen = draw(&mut session, QUEUE_VIEW, 48, 12);
    let (row, _) = screen.find("Delivery").expect("Delivery label");
    let line = screen.row(row);
    for glyph in ["‹", "›"] {
        assert!(line.contains(glyph), "choice arrow {glyph} is drawn in the Delivery row: {line}");
    }
    // A left click on the value moves to the next choice.
    let (value_row, value_column) = screen.find("Once").expect("value");
    assert!(client(&mut session).click(popup(QUEUE_VIEW), value_column, value_row));
    assert!(draw(&mut session, QUEUE_VIEW, 48, 12).text().contains("Run X times"));
    set_text(&mut session, QUEUE_VIEW, "prompt", "p");
    commit_queue(&mut session);
    assert_eq!(list_queue(&mut session, pane)[0]["mode"], "times");
}

/// REQ: PORT:ilium-client/src/prompt_queue.rs::compact_queue_keeps_draft_controls_and_warning_inside_popup, scheduler-B13
#[test]
fn port_prompt_queue_compact_queue_keeps_draft_controls_and_warning_inside_popup() {
    let (mut session, _pane) = queue_dialog("agent");
    select_delivery(&mut session, 2);
    let screen = draw(&mut session, QUEUE_VIEW, 40, 12);
    assert_eq!((screen.columns, screen.rows), (40, 12));
    let text = screen.text();
    assert!(text.contains("Delivery"), "{text}");
    assert!(text.contains("Times"), "{text}");
    assert!(text.contains("DANGER"), "the warning stays visible at 40x12: {text}");
    for line in text.lines() {
        assert!(line.chars().count() <= 40, "{line}");
    }
}

/// REQ: PORT:ilium-client/src/keys.rs::prompt_queue_paste_normalizes_crlf_to_single_newlines
#[test]
fn port_001_prompt_queue_paste_normalizes_crlf_to_single_newlines() {
    let (mut session, pane) = queue_dialog("agent");
    // A paste reaches the dialog as one text change; CRLF and lone CR collapse to single LF.
    set_text(&mut session, QUEUE_VIEW, "prompt", "one\r\ntwo\rthree\nfour");
    commit_queue(&mut session);
    let entries = list_queue(&mut session, pane);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["text"], "one\ntwo\nthree\nfour");
}

//! The queue dialog "Queue prompt after agent finishes" (client side): scheduler-B11 to B15.

use ilium_plugin_sdk::testing::FakeSession;
use ilium_plugin_sdk::types::{ActionKind, ActionValue, AgentActivity, NodeId};

use crate::common::*;

const WARNING: &str = "DANGER: this will re-send forever whenever the agent finishes. Do not run it unmonitored.";

fn opened() -> (FakeSession, NodeId) {
    let mut session = session();
    let pane = terminal_pane(&mut session, "claude-api");
    invoke_menu(&mut session, QUEUE_COMMAND, pane);
    (session, pane)
}

fn screen_text(session: &mut FakeSession) -> String {
    draw(session, QUEUE_VIEW, 80, 24).text()
}

fn select_delivery(session: &mut FakeSession, index: u32) {
    let host = client(session);
    host.send_action(popup(QUEUE_VIEW), "delivery", ActionKind::Selected, ActionValue::Index(index));
    host.settle();
}

fn commit(session: &mut FakeSession) {
    press(session, QUEUE_VIEW, "ctrl+enter");
    pump(session);
}

/// REQ: scheduler-B11, F-057
#[test]
fn qdialog_b11_the_dialog_is_titled_queue_prompt_after_agent_finishes() {
    let (mut session, _pane) = opened();
    assert!(screen_text(&mut session).contains("Queue prompt after agent finishes"));
}

/// REQ: scheduler-B11, F-057
#[test]
fn qdialog_b11_an_editor_pane_has_no_queue_dialog() {
    let mut session = session();
    let pane = add_pane(&mut session, "notes.md", EDITOR_KIND);
    invoke_menu(&mut session, QUEUE_COMMAND, pane);
    assert!(!screen_text(&mut session).contains("Queue prompt after agent finishes"));
}

/// REQ: scheduler-B11, F-057
#[test]
fn qdialog_b11_ctrl_enter_enqueues_and_closes_the_dialog() {
    let (mut session, pane) = opened();
    set_text(&mut session, QUEUE_VIEW, "prompt", "run the tests");
    commit(&mut session);
    assert!(!screen_text(&mut session).contains("Queue prompt after agent finishes"));
    assert_eq!(list_queue(&mut session, pane).len(), 1);
}

/// REQ: scheduler-B11, F-057
#[test]
fn qdialog_b11_esc_cancels_and_enqueues_nothing() {
    let (mut session, pane) = opened();
    set_text(&mut session, QUEUE_VIEW, "prompt", "run the tests");
    press(&mut session, QUEUE_VIEW, "esc");
    pump(&mut session);
    assert!(!screen_text(&mut session).contains("Queue prompt after agent finishes"));
    assert!(list_queue(&mut session, pane).is_empty());
}

/// REQ: scheduler-B11, F-057
#[test]
fn qdialog_b11_enter_inserts_a_new_line_and_does_not_enqueue() {
    let (mut session, pane) = opened();
    set_text(&mut session, QUEUE_VIEW, "prompt", "line one");
    press(&mut session, QUEUE_VIEW, "enter");
    pump(&mut session);
    assert!(screen_text(&mut session).contains("Queue prompt after agent finishes"), "the dialog stays open");
    assert!(list_queue(&mut session, pane).is_empty());
}

/// REQ: scheduler-B11, F-057
#[test]
fn qdialog_b11_a_multiline_prompt_is_enqueued_with_its_newlines() {
    let (mut session, pane) = opened();
    set_text(&mut session, QUEUE_VIEW, "prompt", "first\nsecond");
    commit(&mut session);
    let entries = list_queue(&mut session, pane);
    assert_eq!(entries[0]["text"], "first\nsecond");
}

/// REQ: scheduler-B12, F-057
#[test]
fn qdialog_b12_the_default_delivery_is_once() {
    let (mut session, pane) = opened();
    set_text(&mut session, QUEUE_VIEW, "prompt", "p");
    commit(&mut session);
    assert_eq!(list_queue(&mut session, pane)[0]["mode"], "once");
}

/// REQ: scheduler-B12, F-057
#[test]
fn qdialog_b12_the_delivery_choices_cycle_once_times_forever_and_wrap() {
    let (mut session, _pane) = opened();
    let expected = ["Once", "Run X times", "Enqueue forever (DANGER)", "Once"];
    for label in expected {
        assert!(screen_text(&mut session).contains(label), "{label}");
        press(&mut session, QUEUE_VIEW, "tab");
        press(&mut session, QUEUE_VIEW, "right");
        press(&mut session, QUEUE_VIEW, "shift+tab");
    }
}

/// REQ: scheduler-B12, F-057
#[test]
fn qdialog_b12_left_moves_to_the_previous_choice_and_wraps_to_forever() {
    let (mut session, _pane) = opened();
    press(&mut session, QUEUE_VIEW, "tab");
    press(&mut session, QUEUE_VIEW, "left");
    assert!(screen_text(&mut session).contains("Enqueue forever (DANGER)"));
}

/// REQ: scheduler-B12, F-057
#[test]
fn qdialog_b12_space_on_delivery_changes_the_mode() {
    let (mut session, _pane) = opened();
    press(&mut session, QUEUE_VIEW, "tab");
    press(&mut session, QUEUE_VIEW, "space");
    assert!(screen_text(&mut session).contains("Run X times"));
}

/// REQ: scheduler-B12, F-057
#[test]
fn qdialog_b12_the_times_field_defaults_to_2_in_run_x_times() {
    let (mut session, pane) = opened();
    select_delivery(&mut session, 1);
    set_text(&mut session, QUEUE_VIEW, "prompt", "p");
    commit(&mut session);
    let entry = list_queue(&mut session, pane).remove(0);
    assert_eq!(entry["mode"], "times");
    assert_eq!(entry["times"], 2);
}

/// REQ: scheduler-B12, F-057
#[test]
fn qdialog_b12_the_times_row_is_inert_outside_run_x_times() {
    let (mut session, _pane) = opened();
    assert!(screen_text(&mut session).contains("(only for Run X times)"));
    select_delivery(&mut session, 1);
    assert!(!screen_text(&mut session).contains("(only for Run X times)"));
}

/// REQ: scheduler-B12, F-057
#[test]
fn qdialog_b12_a_non_digit_typed_in_times_is_ignored() {
    let (mut session, pane) = opened();
    select_delivery(&mut session, 1);
    set_text(&mut session, QUEUE_VIEW, "times", "4x2");
    set_text(&mut session, QUEUE_VIEW, "prompt", "p");
    commit(&mut session);
    assert_eq!(list_queue(&mut session, pane)[0]["times"], 42);
}

/// REQ: scheduler-B12, F-057
#[test]
fn qdialog_b12_zero_times_is_refused_as_not_a_positive_whole_number() {
    let (mut session, pane) = opened();
    select_delivery(&mut session, 1);
    set_text(&mut session, QUEUE_VIEW, "times", "0");
    set_text(&mut session, QUEUE_VIEW, "prompt", "p");
    commit(&mut session);
    assert!(screen_text(&mut session).contains("Run count must be a positive whole number"));
    assert!(list_queue(&mut session, pane).is_empty());
}

/// REQ: scheduler-B12, F-057
#[test]
fn qdialog_b12_an_empty_times_field_is_refused() {
    let (mut session, _pane) = opened();
    select_delivery(&mut session, 1);
    set_text(&mut session, QUEUE_VIEW, "times", "");
    set_text(&mut session, QUEUE_VIEW, "prompt", "p");
    commit(&mut session);
    assert!(screen_text(&mut session).contains("Run count must be a positive whole number"));
}

/// REQ: scheduler-B12, F-057
#[test]
fn qdialog_b12_one_time_is_the_smallest_accepted_count() {
    let (mut session, pane) = opened();
    select_delivery(&mut session, 1);
    set_text(&mut session, QUEUE_VIEW, "times", "1");
    set_text(&mut session, QUEUE_VIEW, "prompt", "p");
    commit(&mut session);
    assert_eq!(list_queue(&mut session, pane)[0]["times"], 1);
}

/// REQ: scheduler-B12, F-057
#[test]
fn qdialog_b12_the_largest_count_is_u32_max_and_one_more_is_refused() {
    let (mut session, pane) = opened();
    select_delivery(&mut session, 1);
    set_text(&mut session, QUEUE_VIEW, "times", "4294967295");
    set_text(&mut session, QUEUE_VIEW, "prompt", "p");
    commit(&mut session);
    assert_eq!(list_queue(&mut session, pane)[0]["times"], 4294967295u64);
    let (mut other, other_pane) = opened();
    select_delivery(&mut other, 1);
    set_text(&mut other, QUEUE_VIEW, "times", "4294967296");
    set_text(&mut other, QUEUE_VIEW, "prompt", "p");
    commit(&mut other);
    assert!(list_queue(&mut other, other_pane).is_empty());
}

/// REQ: scheduler-B13, F-057
#[test]
fn qdialog_b13_forever_shows_the_exact_danger_warning() {
    let (mut session, _pane) = opened();
    select_delivery(&mut session, 2);
    let text = screen_text(&mut session);
    let joined: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let wanted: String = WARNING.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(joined.contains(&wanted), "{text}");
}

/// REQ: scheduler-B13, F-057
#[test]
fn qdialog_b13_the_warning_is_hidden_for_once_and_run_x_times() {
    let (mut session, _pane) = opened();
    for index in [0, 1] {
        select_delivery(&mut session, index);
        assert!(!screen_text(&mut session).contains("DANGER: this will re-send forever"), "{index}");
    }
}

/// REQ: scheduler-B12, scheduler-B13, F-057
#[test]
fn qdialog_b12_forever_is_enqueued_as_mode_forever() {
    let (mut session, pane) = opened();
    select_delivery(&mut session, 2);
    set_text(&mut session, QUEUE_VIEW, "prompt", "p");
    commit(&mut session);
    assert_eq!(list_queue(&mut session, pane)[0]["mode"], "forever");
}

/// REQ: scheduler-B14, F-057
#[test]
fn qdialog_b14_an_empty_prompt_is_refused_with_the_exact_message() {
    let (mut session, pane) = opened();
    commit(&mut session);
    assert!(screen_text(&mut session).contains("Write a prompt before enqueueing it"));
    assert!(list_queue(&mut session, pane).is_empty());
}

/// REQ: scheduler-B14, F-057
#[test]
fn qdialog_b14_a_whitespace_only_prompt_is_refused() {
    let (mut session, pane) = opened();
    set_text(&mut session, QUEUE_VIEW, "prompt", "  \n\t ");
    commit(&mut session);
    assert!(screen_text(&mut session).contains("Write a prompt before enqueueing it"));
    assert!(list_queue(&mut session, pane).is_empty());
}

/// REQ: scheduler-B14, F-057
#[test]
fn qdialog_b14_the_service_refuses_an_empty_prompt_too() {
    let mut session = session();
    let pane = terminal_pane(&mut session, "claude-api");
    let error = enqueue(&mut session, pane, "   ", "once", 1).expect_err("refused");
    assert_eq!(error.message, "Write a prompt before enqueueing it");
}

/// REQ: scheduler-B14, F-057
#[test]
fn qdialog_b14_a_prompt_with_surrounding_spaces_is_kept_as_typed() {
    let (mut session, pane) = opened();
    set_text(&mut session, QUEUE_VIEW, "prompt", " keep me ");
    commit(&mut session);
    assert_eq!(list_queue(&mut session, pane)[0]["text"], " keep me ");
}

/// REQ: scheduler-B15, F-057
#[test]
fn qdialog_b15_clear_empties_the_queue_of_the_pane() {
    let mut session = session();
    let pane = terminal_pane(&mut session, "claude-api");
    enqueue(&mut session, pane, "a", "once", 1).expect("a");
    enqueue(&mut session, pane, "b", "once", 1).expect("b");
    invoke_menu(&mut session, CLEAR_COMMAND, pane);
    pump(&mut session);
    assert!(list_queue(&mut session, pane).is_empty());
}

/// REQ: scheduler-B15, F-057
#[test]
fn qdialog_b15_clear_leaves_the_queues_of_other_panes() {
    let mut session = session();
    let first = terminal_pane(&mut session, "one");
    let second = terminal_pane(&mut session, "two");
    enqueue(&mut session, first, "a", "once", 1).expect("a");
    enqueue(&mut session, second, "b", "once", 1).expect("b");
    invoke_menu(&mut session, CLEAR_COMMAND, first);
    pump(&mut session);
    assert_eq!(list_queue(&mut session, second).len(), 1);
}

/// REQ: scheduler-B15, F-057
#[test]
fn qdialog_b15_clear_on_an_empty_queue_changes_nothing() {
    let mut session = session();
    let pane = terminal_pane(&mut session, "claude-api");
    invoke_menu(&mut session, CLEAR_COMMAND, pane);
    pump(&mut session);
    assert!(list_queue(&mut session, pane).is_empty());
}

/// REQ: scheduler-B15, F-057
#[test]
fn qdialog_b15_the_clear_menu_entry_is_listed_only_while_the_queue_is_not_empty() {
    let mut session = session();
    let pane = terminal_pane(&mut session, "claude-api");
    let before = call_queue(&mut session, serde_json::json!({"method": "menu", "pane": pane.0})).expect("menu answers");
    assert!(!before["entries"].to_string().contains("Clear prompt queue"));
    enqueue(&mut session, pane, "a", "once", 1).expect("a");
    let after = call_queue(&mut session, serde_json::json!({"method": "menu", "pane": pane.0})).expect("menu answers");
    assert!(after["entries"].to_string().contains("Clear prompt queue"));
}

/// REQ: scheduler-B15, F-057
#[test]
fn qdialog_b15_the_menu_has_queue_prompt_on_terminal_panes() {
    let mut session = session();
    let pane = terminal_pane(&mut session, "claude-api");
    let menu = call_queue(&mut session, serde_json::json!({"method": "menu", "pane": pane.0})).expect("menu answers");
    let entries = menu["entries"].to_string();
    assert!(entries.contains("Queue prompt..."));
    assert!(entries.contains("Hit key(s) X time from now"));
}

/// REQ: scheduler-B01, scheduler-B11, F-055, F-057
#[test]
fn qdialog_b01_b11_the_menu_of_an_editor_pane_has_neither_entry() {
    let mut session = session();
    let pane = add_pane(&mut session, "notes.md", EDITOR_KIND);
    let menu = call_queue(&mut session, serde_json::json!({"method": "menu", "pane": pane.0})).expect("menu answers");
    let entries = menu["entries"].to_string();
    assert!(!entries.contains("Queue prompt..."));
    assert!(!entries.contains("Hit key(s) X time from now"));
}

/// REQ: scheduler-B11, F-057
#[test]
fn qdialog_b11_the_dialog_is_independent_of_the_agent_state() {
    let mut session = session();
    let pane = terminal_pane(&mut session, "claude-api");
    client(&mut session).tree().set_activity(pane.0, AgentActivity::Working);
    invoke_menu(&mut session, QUEUE_COMMAND, pane);
    assert!(screen_text(&mut session).contains("Queue prompt after agent finishes"));
}

//! Twins of ilium-client/src/text_prompt.rs: single-line editing of a text row of a settings tab
//! (the row owns a cursor; Enter commits, Esc cancels). The keys are named as in the
//! `key` channel of the harness; the committed value is read back from the settings.

use serde_json::json;

use crate::common::*;

const TAB: &str = "git";
const KEY: &str = "git.setup_command";

fn start(host: &mut ilium_plugin_sdk::testing::FakeHost, text: &str) {
    host.send_action(
        tab(TAB),
        KEY,
        ilium_plugin_sdk::types::ActionKind::Changed,
        ilium_plugin_sdk::types::ActionValue::Text(text.to_string()),
    );
}

/// REQ: PORT:ilium-client/src/text_prompt.rs::backspace_at_start_is_a_no_op, F-090
#[test]
fn port_090_backspace_in_an_empty_text_row_keeps_it_empty() {
    let mut host = client_host();
    start(&mut host, "");
    press(&mut host, TAB, &["Backspace", "Enter"]);
    assert_eq!(value_of(&mut host, KEY), Some(json!("")));
}

/// REQ: PORT:ilium-client/src/text_prompt.rs::backspace_removes_before_cursor, F-090
#[test]
fn port_090_backspace_removes_the_character_before_the_cursor() {
    let mut host = client_host();
    start(&mut host, "ab");
    press(&mut host, TAB, &["Backspace", "Enter"]);
    assert_eq!(value_of(&mut host, KEY), Some(json!("a")));
}

/// REQ: PORT:ilium-client/src/text_prompt.rs::delete_forward_removes_after_cursor, F-090
#[test]
fn port_090_delete_removes_the_character_after_the_cursor() {
    let mut host = client_host();
    start(&mut host, "abc");
    press(&mut host, TAB, &["Home", "Right", "Delete", "Enter"]);
    assert_eq!(value_of(&mut host, KEY), Some(json!("ac")));
}

/// REQ: PORT:ilium-client/src/text_prompt.rs::enter_commits_and_esc_cancels, F-090
#[test]
fn port_090_enter_commits_the_text() {
    let mut host = client_host();
    start(&mut host, "x");
    press(&mut host, TAB, &["Enter"]);
    assert_eq!(value_of(&mut host, KEY), Some(json!("x")));
}

/// REQ: PORT:ilium-client/src/text_prompt.rs::enter_commits_and_esc_cancels, F-090
#[test]
fn port_090_escape_cancels_and_keeps_the_stored_value() {
    let mut host = client_host();
    let before = value_of(&mut host, KEY);
    start(&mut host, "x");
    press(&mut host, TAB, &["Esc"]);
    host.advance_clock(600);
    host.settle();
    assert_eq!(value_of(&mut host, KEY), before);
}

/// REQ: PORT:ilium-client/src/text_prompt.rs::home_and_end_jump_to_bounds, F-090
#[test]
fn port_090_home_and_end_move_the_cursor_to_the_ends() {
    let mut host = client_host();
    start(&mut host, "abc");
    press(&mut host, TAB, &["Home", "z", "End", "y", "Enter"]);
    assert_eq!(value_of(&mut host, KEY), Some(json!("zabcy")));
}

/// REQ: PORT:ilium-client/src/text_prompt.rs::left_then_insert_splices_mid_buffer, F-090
#[test]
fn port_090_left_then_a_character_splices_into_the_middle() {
    let mut host = client_host();
    start(&mut host, "ac");
    press(&mut host, TAB, &["Left", "b", "Enter"]);
    assert_eq!(value_of(&mut host, KEY), Some(json!("abc")));
}

/// REQ: PORT:ilium-client/src/text_prompt.rs::multibyte_characters_are_never_split, F-090
#[test]
fn port_090_backspace_removes_a_whole_multibyte_character() {
    let mut host = client_host();
    start(&mut host, "caf\u{e9}");
    press(&mut host, TAB, &["Backspace", "Enter"]);
    assert_eq!(value_of(&mut host, KEY), Some(json!("caf")));
}

/// REQ: PORT:ilium-client/src/text_prompt.rs::new_places_cursor_at_end, F-090
#[test]
fn port_090_a_new_text_row_edit_starts_with_the_cursor_at_the_end() {
    let mut host = client_host();
    start(&mut host, "hello");
    press(&mut host, TAB, &["!", "Enter"]);
    assert_eq!(value_of(&mut host, KEY), Some(json!("hello!")));
}

/// REQ: PORT:ilium-client/src/text_prompt.rs::typing_inserts_at_cursor, F-090
#[test]
fn port_090_typing_inserts_characters_in_order() {
    let mut host = client_host();
    start(&mut host, "");
    press(&mut host, TAB, &["a", "b", "Enter"]);
    assert_eq!(value_of(&mut host, KEY), Some(json!("ab")));
}

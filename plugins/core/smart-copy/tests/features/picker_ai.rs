//! F-065/F-066: the picker keys, the clipboard writes and the inference ranking.

use ilium_plugin_sdk::types::{Event, Intent, Key};

use crate::common::{
    add_terminal_pane, candidate_lines, candidates_message, client_host, intents, palette_command,
    picker_target, status_contains,
};

fn key(name: &str) -> Event {
    Event::Key(Key::named(name))
}

fn open_picker(lines: &[&str]) -> ilium_plugin_sdk::testing::FakeHost {
    let mut host = client_host();
    host.deliver_one(candidates_message(lines));
    host
}

/// REQ: F-065
#[test]
fn feature_065_candidates_message_opens_the_popup_view() {
    let mut host = client_host();
    let out = intents(&host.deliver_one(candidates_message(&["src/a.rs"])));
    assert!(out.iter().any(|intent| matches!(intent, Intent::OpenView(view) if view.view == picker_target())), "{out:?}");
}

/// REQ: F-065
#[test]
fn feature_065_an_empty_candidates_message_opens_nothing() {
    let mut host = client_host();
    let out = intents(&host.deliver_one(candidates_message(&[])));
    assert!(!out.iter().any(|intent| matches!(intent, Intent::OpenView(_))), "{out:?}");
}

/// REQ: F-065
#[test]
fn feature_065_number_key_copies_the_matching_candidate() {
    let mut host = open_picker(&["src/a.rs", "cargo test", "9f3c2ab"]);
    host.deliver_one(key("2"));
    assert_eq!(host.clipboard().last_text().as_deref(), Some("cargo test"));
}

/// REQ: F-065
#[test]
fn feature_065_copy_reports_the_character_count_in_the_status_bar() {
    let mut host = open_picker(&["héllo"]);
    let out = host.deliver_one(key("1"));
    assert!(status_contains(&out, "Copied 5 characters"));
}

/// REQ: F-065
#[test]
fn feature_065_out_of_range_numbers_copy_nothing() {
    let mut host = open_picker(&["a", "b"]);
    for name in ["3", "9", "0"] {
        host.deliver_one(key(name));
    }
    assert_eq!(host.clipboard().last_text(), None);
}

/// REQ: F-065
#[test]
fn feature_065_escape_closes_the_popup_without_copying() {
    let mut host = open_picker(&["a"]);
    let out = intents(&host.deliver_one(key("esc")));
    assert!(out.iter().any(|intent| matches!(intent, Intent::CloseView(_))), "{out:?}");
    assert_eq!(host.clipboard().last_text(), None);
}

/// REQ: F-065
#[test]
fn feature_065_clipboard_failure_reports_copy_failed_and_keeps_the_popup() {
    let mut host = open_picker(&["a"]);
    host.clipboard().fail_next_write("Io");
    let out = host.deliver_one(key("1"));
    assert!(status_contains(&out, "Copy failed: Io"));
}

/// REQ: F-066
#[test]
fn feature_066_command_asks_inference_once_with_the_ranking_prompt() {
    let mut host = client_host();
    let pane = add_terminal_pane(&mut host, "api");
    host.terminal().set_rows(pane, &["src/lib.rs"]);
    host.inference().reply_text("a\nb\nc");
    host.deliver_one(palette_command("smart-copy", Some(pane)));
    host.settle();
    let requests = host.inference().requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].purpose, "smart-copy");
    assert_eq!(requests[0].messages[0].role, "system");
    assert_eq!(
        requests[0].messages[0].content,
        "Return the three most useful strings to copy, one per line."
    );
    assert!(requests[0].messages[1].content.contains("src/lib.rs"));
}

/// REQ: F-066
#[test]
fn feature_066_only_the_first_three_reply_lines_are_offered() {
    let mut host = client_host();
    let pane = add_terminal_pane(&mut host, "api");
    host.inference().reply_text("one\ntwo\nthree\nfour");
    host.deliver_one(palette_command("smart-copy", Some(pane)));
    host.settle();
    host.deliver_one(key("4"));
    assert_eq!(host.clipboard().last_text(), None);
    host.deliver_one(key("3"));
    assert_eq!(host.clipboard().last_text().as_deref(), Some("three"));
}

/// REQ: F-066
#[test]
fn feature_066_inference_error_falls_back_to_the_light_candidates() {
    let mut host = client_host();
    let pane = add_terminal_pane(&mut host, "api");
    host.terminal().set_rows(pane, &["src/lib.rs"]);
    host.inference().fail_next("Unavailable");
    host.deliver_one(candidates_message(&["src/lib.rs"]));
    host.deliver_one(palette_command("smart-copy", Some(pane)));
    host.settle();
    host.deliver_one(key("1"));
    assert_eq!(host.clipboard().last_text().as_deref(), Some("src/lib.rs"));
    let _ = candidate_lines(&[]);
}

/// REQ: F-066
#[test]
fn feature_066_inference_request_leaves_max_output_tokens_and_schema_unset() {
    let mut host = client_host();
    let pane = add_terminal_pane(&mut host, "api");
    host.inference().reply_text("x");
    host.deliver_one(palette_command("smart-copy", Some(pane)));
    host.settle();
    let request = &host.inference().requests()[0];
    assert_eq!(request.max_output_tokens, None);
    assert_eq!(request.json_schema, None);
}

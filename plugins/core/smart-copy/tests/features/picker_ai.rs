//! F-065/F-066: the picker keys, the clipboard writes and the inference ranking.

use ilium_plugin_sdk::types::{Event, GrabId, Intent, KeyEvent};

use crate::common::{
    add_terminal_pane, candidate_lines, deliver_candidates, client_host, intents, palette_command,
    picker_target, status_contains,
};

fn key(name: &str) -> Event {
    Event::KeyPressed(KeyEvent {
        grab: GrabId(0),
        pane: None,
        chord: name.to_string(),
        repeat: false,
    })
}

fn open_picker(lines: &[&str]) -> ilium_plugin_sdk::testing::FakeHost {
    let mut host = client_host();
    deliver_candidates(&mut host, lines);
    host
}

/// REQ: F-065
#[test]
fn feature_065_candidates_message_opens_the_popup_view() {
    let mut host = client_host();
    let out = intents(&deliver_candidates(&mut host, &["src/a.rs"]));
    assert!(
        out.iter()
            .any(|intent| matches!(intent, Intent::OpenView(view) if view.view == picker_target())),
        "{out:?}"
    );
}

/// REQ: F-065
#[test]
fn feature_065_an_empty_candidates_message_opens_nothing() {
    let mut host = client_host();
    let out = intents(&deliver_candidates(&mut host, &[]));
    assert!(
        !out.iter()
            .any(|intent| matches!(intent, Intent::OpenView(_))),
        "{out:?}"
    );
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
    assert!(
        out.iter()
            .any(|intent| matches!(intent, Intent::CloseView(_))),
        "{out:?}"
    );
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
    assert_eq!(requests[0].1.purpose, "smart-copy");
    assert_eq!(requests[0].1.messages[0].role, "system");
    assert!(requests[0].1.messages[0].content.contains("JSON Lines"));
    assert!(requests[0].1.messages[1].content.contains("src/lib.rs"));
}

/// REQ: F-066
#[test]
fn feature_066_model_candidates_can_be_paged() {
    let mut host = client_host();
    let pane = add_terminal_pane(&mut host, "api");
    host.terminal().set_rows(pane, &["one two three four"]);
    host.inference().reply_text(concat!(
        "{\"label\":\"one\",\"kind\":\"word\",\"parts\":[{\"line\":1,\"from\":\"w1\",\"through\":\"w1\"}]}\n",
        "{\"label\":\"two\",\"kind\":\"word\",\"parts\":[{\"line\":1,\"from\":\"w2\",\"through\":\"w2\"}]}\n",
        "{\"label\":\"three\",\"kind\":\"word\",\"parts\":[{\"line\":1,\"from\":\"w3\",\"through\":\"w3\"}]}\n",
        "{\"label\":\"four\",\"kind\":\"word\",\"parts\":[{\"line\":1,\"from\":\"w4\",\"through\":\"w4\"}]}"
    ));
    host.deliver_one(palette_command("smart-copy", Some(pane)));
    host.settle();
    host.deliver_one(key("4"));
    assert_eq!(host.clipboard().last_text(), None);
    host.deliver_one(key("n"));
    host.deliver_one(key("1"));
    assert_eq!(host.clipboard().last_text().as_deref(), Some("four"));
}

/// REQ: F-066
#[test]
fn feature_066_inference_error_falls_back_to_the_light_candidates() {
    let mut host = client_host();
    let pane = add_terminal_pane(&mut host, "api");
    host.terminal().set_rows(pane, &["src/lib.rs"]);
    host.inference().fail_next("Unavailable");
    host.deliver_one(palette_command("smart-copy", Some(pane)));
    deliver_candidates(&mut host, &["src/lib.rs"]);
    host.settle();
    host.deliver_one(key("1"));
    assert_eq!(host.clipboard().last_text().as_deref(), Some("src/lib.rs"));
    let _ = candidate_lines(&[]);
}

#[test]
fn model_generated_text_absent_from_the_screen_is_never_copied() {
    let mut host = client_host();
    let pane = add_terminal_pane(&mut host, "api");
    host.terminal().set_rows(pane, &["src/lib.rs"]);
    host.inference().reply_text("invented secret");
    host.deliver_one(palette_command("smart-copy", Some(pane)));
    deliver_candidates(&mut host, &["src/lib.rs"]);
    host.settle();
    host.deliver_one(key("1"));
    assert_eq!(host.clipboard().last_text().as_deref(), Some("src/lib.rs"));
}

#[test]
fn a_model_word_range_copies_text_from_the_captured_screen() {
    let mut host = client_host();
    let pane = add_terminal_pane(&mut host, "api");
    host.terminal().set_rows(pane, &["run cargo test now"]);
    host.inference().reply_text(
        r#"{"label":"command","kind":"command","parts":[{"line":1,"from":"w2","through":"w3"}]}"#,
    );
    host.deliver_one(palette_command("smart-copy", Some(pane)));
    deliver_candidates(&mut host, &["run cargo test now"]);
    host.settle();
    host.deliver_one(key("1"));
    assert_eq!(host.clipboard().last_text().as_deref(), Some("cargo test"));
}

#[test]
fn model_references_resolve_against_the_screen_captured_at_invocation() {
    let mut host = client_host();
    let pane = add_terminal_pane(&mut host, "api");
    host.terminal().set_rows(pane, &["run cargo test now"]);
    host.inference().reply_text(
        r#"{"label":"command","kind":"command","parts":[{"line":1,"from":"w2","through":"w3"}]}"#,
    );
    host.deliver_one(palette_command("smart-copy", Some(pane)));
    host.terminal().set_rows(pane, &["run rm files now"]);
    deliver_candidates(&mut host, &["run cargo test now"]);
    host.settle();
    host.deliver_one(key("1"));
    assert_eq!(host.clipboard().last_text().as_deref(), Some("cargo test"));
}

#[test]
fn invalid_model_references_are_skipped_without_discarding_valid_records() {
    let mut host = client_host();
    let pane = add_terminal_pane(&mut host, "api");
    host.terminal().set_rows(pane, &["run cargo test now"]);
    host.inference().reply_text(concat!(
        "{\"label\":\"bad\",\"kind\":\"word\",\"parts\":[{\"line\":1,\"from\":\"w9\"}]}\n",
        "{\"label\":\"good\",\"kind\":\"word\",\"parts\":[{\"line\":1,\"from\":\"w2\",\"through\":\"w3\"}]}"
    ));
    host.deliver_one(palette_command("smart-copy", Some(pane)));
    deliver_candidates(&mut host, &["run cargo test now"]);
    host.settle();
    host.deliver_one(key("1"));
    assert_eq!(host.clipboard().last_text().as_deref(), Some("cargo test"));
}

#[test]
fn detected_candidates_remain_available_when_model_reply_arrives_first() {
    let mut host = client_host();
    let pane = add_terminal_pane(&mut host, "api");
    host.terminal().set_rows(pane, &["cargo test src/lib.rs"]);
    host.inference().reply_text(
        r#"{"label":"command","kind":"command","parts":[{"line":1,"from":"w1","through":"w2"}]}"#,
    );
    host.deliver_one(palette_command("smart-copy", Some(pane)));
    host.settle();
    deliver_candidates(&mut host, &["src/lib.rs"]);
    host.deliver_one(key("2"));
    assert_eq!(host.clipboard().last_text().as_deref(), Some("src/lib.rs"));
}

/// REQ: F-066
#[test]
fn feature_066_inference_request_leaves_max_output_tokens_and_schema_unset() {
    let mut host = client_host();
    let pane = add_terminal_pane(&mut host, "api");
    host.terminal().set_rows(pane, &["src/lib.rs"]);
    host.inference().reply_text("x");
    host.deliver_one(palette_command("smart-copy", Some(pane)));
    host.settle();
    let request = &host.inference().requests()[0];
    assert_eq!(request.1.max_output_tokens, None);
    assert_eq!(request.1.json_schema, None);
}

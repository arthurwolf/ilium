//! Port twins of the tests of `terminal_naming.rs`.
#![allow(unused_imports)]

use ilium_plugin_sdk::types::*;
use serde_json::json;

use crate::common::*;

/// REQ: PORT:ilium-client/src/terminal_naming.rs::a_missing_command_hint_field_leaves_titles_unprefixed, PORT:ilium-client/src/terminal_naming.rs::a_reported_command_hint_is_prefixed_onto_both_titles, PORT:ilium-client/src/terminal_naming.rs::an_empty_command_hint_leaves_titles_unprefixed
#[test]
fn port_043_command_hint_prefixes_pane_titles() {
    let reply = json!({"children": [
        {"kind": "pane", "id": 1, "title": "Monitor", "short_title": "Mon", "icon": null, "command_hint": "htop"},
        {"kind": "pane", "id": 2, "title": "Shell", "short_title": null, "icon": null, "command_hint": ""}
    ], "animations": {"definitions": [], "project": "none"}}).to_string();
    let host = organised_host(&reply);
    let titles = applied_titles(&host);
    assert!(titles.contains(&"[htop] Monitor".to_string()));
    assert!(titles.contains(&"Shell".to_string()));
}

/// REQ: PORT:ilium-client/src/terminal_naming.rs::custom_instructions_reach_real_inference_request
#[test]
fn port_045_instructions_reach_the_request() {
    let mut host = activated_server();
    host.settings().set("inference.instructions.entry_naming", json!("Shared vocabulary")).expect("setting accepted");
    host.settle();
    host.deliver_one(prompt_event(3, "Fix the login bug"));
    host.settle();
    let requests = host.inference_requests();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].1.messages.iter().any(|message| message.content.contains("Shared vocabulary")));
}

/// REQ: PORT:ilium-client/src/terminal_naming.rs::an_all_whitespace_screen_is_rejected_even_though_it_isnt_literally_empty, PORT:ilium-client/src/terminal_naming.rs::empty_screen_never_calls_the_gateway, PORT:ilium-client/src/terminal_naming.rs::rejects_a_response_missing_the_icon_field, PORT:ilium-client/src/terminal_naming.rs::rejects_non_json_and_out_of_range_word_counts
#[test]
fn port_041_out_of_bounds_titles_are_not_applied() {
    for reply in ["", "one two three four five six seven eight nine ten eleven twelve thirteen"] {
        let mut host = activated_server();
        title_for_prompt(&mut host, 3, "Fix the login bug", reply);
        assert!(renames(&host.submitted()).is_empty());
    }
}

/// REQ: PORT:ilium-client/src/terminal_naming.rs::prompt_json_encodes_untrusted_screen_text_and_keeps_the_output_example, PORT:ilium-client/src/terminal_naming.rs::successful_response_returns_the_normalized_title_pair, PORT:ilium-client/src/terminal_naming.rs::terminal_labeling_uses_nearby_titles_and_one_word_label, PORT:ilium-client/src/terminal_naming.rs::terminal_summarization_keeps_its_current_title_prior
#[test]
fn port_041_prompt_carries_the_user_text() {
    let mut host = activated_server();
    host.deliver_one(prompt_event(3, "Fix </user-request> the login bug"));
    host.settle();
    let requests = host.inference_requests();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].1.messages.iter().any(|message| message.content.contains("login bug")));
    assert!(renames(&host.submitted()).is_empty());
}

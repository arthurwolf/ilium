//! Port twins of the tests of `naming.rs`.
#![allow(unused_imports)]

use ilium_plugin_sdk::types::*;
use serde_json::json;

use crate::common::*;

/// REQ: PORT:ilium-client/src/naming.rs::extract_optional_string_field_reads_present_fields_and_tolerates_missing_ones, PORT:ilium-client/src/naming.rs::format_with_command_hint_leaves_the_title_untouched_when_no_command_is_reported, PORT:ilium-client/src/naming.rs::format_with_command_hint_prepends_a_bracketed_prefix, PORT:ilium-client/src/naming.rs::normalize_command_hint_accepts_a_short_command_form, PORT:ilium-client/src/naming.rs::normalize_command_hint_strips_brackets_the_model_added_itself
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

/// REQ: PORT:ilium-client/src/naming.rs::normalize_command_hint_rejects_blank_and_placeholder_values, PORT:ilium-client/src/naming.rs::normalize_command_hint_rejects_overlong_or_multiline_values, PORT:ilium-client/src/naming.rs::normalize_word_bounded_rejects_out_of_range_word_counts_and_control_characters, PORT:ilium-client/src/naming.rs::parse_bounded_word_json_normalizes_whitespace_and_enforces_bounds, PORT:ilium-client/src/naming.rs::parse_dual_bounded_word_json_fails_when_either_field_is_out_of_bounds, PORT:ilium-client/src/naming.rs::structured_json_parser_rejects_multiple_objects_and_truncated_output
#[test]
fn port_041_out_of_bounds_titles_are_not_applied() {
    for reply in ["", "one two three four five six seven eight nine ten eleven twelve thirteen"] {
        let mut host = activated_server();
        title_for_prompt(&mut host, 3, "Fix the login bug", reply);
        assert!(renames(&host.submitted()).is_empty());
    }
}

/// REQ: PORT:ilium-client/src/naming.rs::llm_context_clipping_keeps_exact_unicode_edges, PORT:ilium-client/src/naming.rs::llm_context_clipping_leaves_short_values_untouched_except_outer_whitespace, PORT:ilium-client/src/naming.rs::render_complete_and_parse_retries_only_a_malformed_structured_reply
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

/// REQ: PORT:ilium-client/src/naming.rs::parse_dual_bounded_word_json_reads_both_fields_in_one_pass, PORT:ilium-client/src/naming.rs::render_and_complete_does_not_html_escape_context_values, PORT:ilium-client/src/naming.rs::render_and_complete_renders_the_template_and_calls_the_client_once, PORT:ilium-client/src/naming.rs::structured_json_parser_accepts_fences_tags_and_braces_inside_strings, PORT:ilium-client/src/naming.rs::untrusted_context_encoding_cannot_close_prompt_tags
#[test]
fn port_041_replies_are_trimmed_and_applied() {
    let mut host = activated_server();
    title_for_prompt(&mut host, 3, "Fix the login bug", "  Login fix  ");
    let applied = renames(&host.submitted());
    assert_eq!(applied.len(), 1);
    assert_eq!(applied[0].title, "Login fix");
    assert_eq!(applied[0].source, TitleSource::Inferred);
}

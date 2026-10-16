//! Port twins of the tests of `session_naming.rs`.
#![allow(unused_imports)]

use ilium_plugin_sdk::types::*;
use serde_json::json;

use crate::common::*;

/// REQ: PORT:ilium-client/src/session_naming.rs::custom_instructions_reach_real_inference_request
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

/// REQ: PORT:ilium-client/src/session_naming.rs::labeling_rejects_provider_name_as_session_title, PORT:ilium-client/src/session_naming.rs::response_still_requires_json_icon_and_bounded_title_lengths, PORT:ilium-client/src/session_naming.rs::transcript_from_another_project_is_rejected_before_provider_call, PORT:ilium-client/src/session_naming.rs::transcript_without_a_user_entry_never_calls_the_provider
#[test]
fn port_041_out_of_bounds_titles_are_not_applied() {
    for reply in ["", "one two three four five six seven eight nine ten eleven twelve thirteen"] {
        let mut host = activated_server();
        title_for_prompt(&mut host, 3, "Fix the login bug", reply);
        assert!(renames(&host.submitted()).is_empty());
    }
}

/// REQ: PORT:ilium-client/src/session_naming.rs::every_large_dynamic_value_uses_the_shared_edge_budget, PORT:ilium-client/src/session_naming.rs::labeling_encodes_each_neighbor_without_escaping_its_data_boundary, PORT:ilium-client/src/session_naming.rs::labeling_keeps_user_requests_in_order_and_encodes_them_as_data, PORT:ilium-client/src/session_naming.rs::labeling_prompt_uses_tree_context_and_accepts_one_word_labels, PORT:ilium-client/src/session_naming.rs::pane_inference_reads_user_assistant_and_tool_context_from_verified_jsonl, PORT:ilium-client/src/session_naming.rs::prompt_contains_all_live_metadata_and_typed_transcript_entries, PORT:ilium-client/src/session_naming.rs::summarization_keeps_its_current_title_prior
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

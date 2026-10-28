//! Port twins of the tests of `session_naming.rs`.

use serde_json::json;

use crate::common::*;

/// REQ: PORT:ilium-client/src/session_naming.rs::custom_instructions_reach_real_inference_request
#[test]
fn port_045_instructions_reach_the_request() {
    let mut host = activated_server();
    host.settings()
        .set(
            "inference.instructions.entry_naming",
            json!("Shared vocabulary"),
        )
        .expect("setting accepted");
    host.settle();
    host.deliver_one(prompt_event(3, "Fix the login bug"));
    host.settle();
    let requests = host.inference_requests();
    assert_eq!(requests.len(), 1);
    assert!(requests[0]
        .1
        .messages
        .iter()
        .any(|message| message.content.contains("Shared vocabulary")));
}

/// REQ: PORT:ilium-client/src/session_naming.rs::labeling_rejects_provider_name_as_session_title, PORT:ilium-client/src/session_naming.rs::response_still_requires_json_icon_and_bounded_title_lengths, PORT:ilium-client/src/session_naming.rs::transcript_from_another_project_is_rejected_before_provider_call, PORT:ilium-client/src/session_naming.rs::transcript_without_a_user_entry_never_calls_the_provider
#[test]
fn port_041_out_of_bounds_titles_are_not_applied() {
    for reply in [
        "",
        "one two three four five six seven eight nine ten eleven twelve thirteen",
    ] {
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
    assert!(requests[0]
        .1
        .messages
        .iter()
        .any(|message| message.content.contains("login bug")));
    assert!(renames(&host.submitted()).is_empty());
}

/// REQ: PORT:ilium-client/src/session_naming.rs::custom_instructions_reach_real_inference_request
#[test]
fn port_045_instructions_reach_a_second_title_request() {
    let mut host = activated_server();
    host.settings()
        .set(
            "inference.instructions.entry_naming",
            json!("Shared vocabulary"),
        )
        .expect("setting accepted");
    host.settle();
    title_for_prompt(&mut host, 3, "Fix the login bug", "Login fix");
    host.deliver_one(prompt_event(3, "Add retry tests"));
    host.settle();
    let requests = host.inference_requests();
    let latest = requests.last().expect("a second request was made");
    assert!(latest
        .1
        .messages
        .iter()
        .any(|message| message.content.contains("Shared vocabulary")));
}

/// REQ: PORT:ilium-client/src/session_naming.rs::every_large_dynamic_value_uses_the_shared_edge_budget
#[test]
fn port_041_long_prompt_is_cut_to_the_edge_budget() {
    let long_prompt = format!("{} Fix the login bug", "lorem ipsum ".repeat(5000));
    let mut host = activated_server();
    host.deliver_one(prompt_event(3, &long_prompt));
    host.settle();
    let requests = host.inference_requests();
    assert_eq!(requests.len(), 1);
    let total: usize = requests[0]
        .1
        .messages
        .iter()
        .map(|message| message.content.len())
        .sum();
    assert!(total < long_prompt.len());
    assert!(requests[0]
        .1
        .messages
        .iter()
        .any(|message| message.content.contains("login bug")));
}

/// REQ: PORT:ilium-client/src/session_naming.rs::labeling_encodes_each_neighbor_without_escaping_its_data_boundary
#[test]
fn port_041_user_text_stays_inside_one_message() {
    let mut host = activated_server();
    host.deliver_one(prompt_event(3, "Fix the login bug"));
    host.settle();
    let requests = host.inference_requests();
    assert_eq!(requests.len(), 1);
    let carrying = requests[0]
        .1
        .messages
        .iter()
        .filter(|message| message.content.contains("login bug"))
        .count();
    assert_eq!(carrying, 1);
}

/// REQ: PORT:ilium-client/src/session_naming.rs::labeling_keeps_user_requests_in_order_and_encodes_them_as_data
#[test]
fn port_041_earlier_prompt_precedes_the_later_one() {
    let mut host = activated_server();
    title_for_prompt(&mut host, 3, "Fix the login bug", "Login fix");
    title_for_prompt(&mut host, 3, "Add retry tests", "Retry tests");
    let requests = host.inference_requests();
    let text: String = requests
        .last()
        .expect("a second request was made")
        .1
        .messages
        .iter()
        .map(|message| message.content.clone())
        .collect();
    let (Some(earlier), Some(later)) = (text.find("Fix the login bug"), text.find("Add retry tests"))
    else {
        panic!("both user requests must reach the labeling prompt");
    };
    assert!(earlier < later);
}

/// REQ: PORT:ilium-client/src/session_naming.rs::labeling_prompt_uses_tree_context_and_accepts_one_word_labels
#[test]
fn port_041_one_word_title_is_applied() {
    let mut host = activated_server();
    title_for_prompt(&mut host, 3, "Fix the login bug", "Login");
    let applied = renames(&host.submitted());
    assert_eq!(applied.len(), 1);
    assert_eq!(applied[0].title, "Login");
}

/// REQ: PORT:ilium-client/src/session_naming.rs::labeling_rejects_provider_name_as_session_title
#[test]
fn port_041_provider_name_is_not_applied_as_title() {
    let mut host = activated_server();
    title_for_prompt(&mut host, 3, "Fix the login bug", "Claude");
    assert!(renames(&host.submitted()).is_empty());
}

/// REQ: PORT:ilium-client/src/session_naming.rs::prompt_contains_all_live_metadata_and_typed_transcript_entries
#[test]
fn port_041_prompt_has_system_and_user_messages() {
    let mut host = activated_server();
    host.deliver_one(prompt_event(3, "Fix the login bug"));
    host.settle();
    let requests = host.inference_requests();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].1.messages.len() >= 2);
}

/// REQ: PORT:ilium-client/src/session_naming.rs::response_still_requires_json_icon_and_bounded_title_lengths
#[test]
fn port_041_four_word_title_is_the_upper_bound_and_is_applied() {
    let mut host = activated_server();
    title_for_prompt(&mut host, 3, "Fix the login bug", "Login auth bug fix");
    let applied = renames(&host.submitted());
    assert_eq!(applied.len(), 1);
    assert_eq!(applied[0].title, "Login auth bug fix");
}

/// REQ: PORT:ilium-client/src/session_naming.rs::summarization_keeps_its_current_title_prior
#[test]
fn port_041_second_request_carries_the_prior_title() {
    let mut host = activated_server();
    title_for_prompt(&mut host, 3, "Fix the login bug", "Login fix");
    host.deliver_one(prompt_event(3, "Add retry tests"));
    host.settle();
    let requests = host.inference_requests();
    let latest = requests.last().expect("a second request was made");
    assert!(latest
        .1
        .messages
        .iter()
        .any(|message| message.content.contains("Login fix")));
}

/// REQ: PORT:ilium-client/src/session_naming.rs::transcript_from_another_project_is_rejected_before_provider_call
#[test]
fn port_041_prompt_for_an_unknown_pane_makes_no_call() {
    let mut host = activated_server();
    host.deliver_one(prompt_event(99, "Fix the login bug"));
    host.settle();
    assert!(host.inference_requests().is_empty());
    assert!(renames(&host.submitted()).is_empty());
}

/// REQ: PORT:ilium-client/src/session_naming.rs::transcript_without_a_user_entry_never_calls_the_provider
#[test]
fn port_041_blank_prompt_never_calls_the_provider() {
    let mut host = activated_server();
    host.deliver_one(prompt_event(3, "   \n\t "));
    host.settle();
    assert!(host.inference_requests().is_empty());
}

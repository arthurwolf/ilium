//! Port twins of the tests of `project_naming.rs`.

use ilium_plugin_sdk::types::*;
use serde_json::json;

use crate::common::*;

/// REQ: PORT:ilium-client/src/project_naming.rs::commit_preserves_a_valid_name_written_during_inference, PORT:ilium-client/src/project_naming.rs::inference_only_has_no_project_config_side_effect_until_commit, PORT:ilium-client/src/project_naming.rs::inference_update_preserves_project_scoped_ui_preferences, PORT:ilium-client/src/project_naming.rs::missing_name_collects_context_then_persists_one_inference, PORT:ilium-client/src/project_naming.rs::stored_name_skips_the_gateway_entirely
#[test]
fn port_041_inferred_title_is_applied_once() {
    let mut host = activated_server();
    title_for_prompt(&mut host, 3, "Fix the login bug", "  Login fix  ");
    let applied = renames(&host.submitted());
    assert_eq!(applied.len(), 1);
    assert_eq!(applied[0].title, "Login fix");
    assert_eq!(applied[0].source, TitleSource::Inferred);
}

/// REQ: PORT:ilium-client/src/project_naming.rs::custom_instructions_reach_real_inference_request
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

/// REQ: PORT:ilium-client/src/project_naming.rs::rejects_non_json_and_more_than_two_words
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

/// REQ: PORT:ilium-client/src/project_naming.rs::documents_and_root_listing_are_cropped_at_the_requested_bounds, PORT:ilium-client/src/project_naming.rs::prompt_is_xml_shaped_and_includes_the_json_output_example
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

/// REQ: PORT:ilium-client/src/project_naming.rs::commit_preserves_a_valid_name_written_during_inference
#[test]
fn port_041_typed_title_during_inference_stops_later_triggers() {
    let mut host = activated_server();
    host.deliver_one(prompt_event(3, "Fix the login bug"));
    host.settle();
    host.deliver_one(Event::PaneTitleChanged(PaneTitleChangedEvent {
        node: NodeId(3),
        title: "My own title".to_string(),
        source: TitleOrigin::Manual,
    }));
    host.settle();
    host.deliver_one(prompt_event(3, "Fix the login bug again"));
    host.settle();
    assert_eq!(host.inference_requests().len(), 1);
}

/// REQ: PORT:ilium-client/src/project_naming.rs::custom_instructions_reach_real_inference_request
#[test]
fn port_045_instructions_changed_mid_session_reach_the_next_request() {
    let mut host = activated_server();
    host.deliver_one(prompt_event(3, "Fix the login bug"));
    host.settle();
    host.settings()
        .set(
            "inference.instructions.entry_naming",
            json!("Shared vocabulary"),
        )
        .expect("setting accepted");
    host.settle();
    host.deliver_one(prompt_event(4, "Add a settings page"));
    host.settle();
    let requests = host.inference_requests();
    let (_, latest) = requests.last().expect("a second request was made");
    assert!(latest
        .messages
        .iter()
        .any(|message| message.content.contains("Shared vocabulary")));
}

/// REQ: PORT:ilium-client/src/project_naming.rs::documents_and_root_listing_are_cropped_at_the_requested_bounds
#[test]
fn port_041_long_prompt_is_cut_to_the_input_budget() {
    let mut host = activated_server();
    let long_prompt = format!("{}TAIL_MARK", "word ".repeat(20_000));
    host.deliver_one(prompt_event(3, &long_prompt));
    host.settle();
    let requests = host.inference_requests();
    let content: String = requests[0]
        .1
        .messages
        .iter()
        .map(|message| message.content.clone())
        .collect();
    assert!(content.len() < long_prompt.len());
    assert!(content.contains("TAIL_MARK"));
}

/// REQ: PORT:ilium-client/src/project_naming.rs::missing_name_collects_context_then_persists_one_inference
#[test]
fn port_041_at_most_two_title_requests_run_at_once() {
    let mut host = activated_server();
    for node in [3, 4, 5] {
        host.deliver_one(prompt_event(node, "Fix the login bug"));
    }
    host.settle();
    assert!(host.inference_requests().len() <= 2);
}

/// REQ: PORT:ilium-client/src/project_naming.rs::prompt_is_xml_shaped_and_includes_the_json_output_example
#[test]
fn port_041_prompt_wraps_the_user_text_in_request_tags() {
    let mut host = activated_server();
    host.deliver_one(prompt_event(3, "Fix the login bug"));
    host.settle();
    let requests = host.inference_requests();
    let content: String = requests[0]
        .1
        .messages
        .iter()
        .map(|message| message.content.clone())
        .collect();
    assert!(content.contains("<user-request>"));
    assert!(content.contains("</user-request>"));
}

/// REQ: PORT:ilium-client/src/project_naming.rs::rejects_non_json_and_more_than_two_words
#[test]
fn port_041_five_word_title_is_not_applied() {
    let mut host = activated_server();
    title_for_prompt(&mut host, 3, "Fix the login bug", "one two three four five");
    assert!(renames(&host.submitted()).is_empty());
}

/// REQ: PORT:ilium-client/src/project_naming.rs::stored_name_skips_the_gateway_entirely
#[test]
fn port_041_typed_title_makes_no_inference_call() {
    let mut host = activated_server();
    host.deliver_one(Event::PaneTitleChanged(PaneTitleChangedEvent {
        node: NodeId(3),
        title: "My own title".to_string(),
        source: TitleOrigin::Manual,
    }));
    host.settle();
    host.deliver_one(prompt_event(3, "Fix the login bug"));
    host.settle();
    assert!(host.inference_requests().is_empty());
}

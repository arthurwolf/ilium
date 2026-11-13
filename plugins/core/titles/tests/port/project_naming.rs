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

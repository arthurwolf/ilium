//! Automatic pane titles and session naming (F-041, F-042), samples 1 of the page.

use ilium_plugin_sdk::types::{ErrorCode, Event, NodeId, Outcome, RequestId, TitleSource};

use crate::common::*;

/// REQ: F-041
#[test]
fn f_041_input_submitted_makes_exactly_one_inference_request() {
    let mut host = activated_server();
    host.deliver_one(prompt_event(3, "Fix the login bug"));
    host.settle();
    assert_eq!(host.inference_requests().len(), 1);
}

/// REQ: F-041
#[test]
fn f_041_request_is_for_the_pane_title_purpose() {
    let mut host = activated_server();
    host.deliver_one(prompt_event(3, "Fix the login bug"));
    host.settle();
    let (_, request) = host.inference_requests().remove(0);
    assert_eq!(request.purpose, "pane-title");
}

/// REQ: F-041
#[test]
fn f_041_request_carries_a_system_message_first_and_the_prompt_as_the_user_message() {
    let mut host = activated_server();
    host.deliver_one(prompt_event(3, "Fix the login bug"));
    host.settle();
    let (_, request) = host.inference_requests().remove(0);
    assert_eq!(request.messages.first().expect("first").role, "system");
    let last = request.messages.last().expect("last");
    assert_eq!(last.role, "user");
    assert_eq!(last.content, "Fix the login bug");
}

/// REQ: F-041
#[test]
fn f_041_request_asks_for_the_models_maximum_output() {
    let mut host = activated_server();
    host.deliver_one(prompt_event(3, "Fix the login bug"));
    host.settle();
    let (_, request) = host.inference_requests().remove(0);
    assert_eq!(request.max_output_tokens, None);
}

/// REQ: F-041
#[test]
fn f_041_no_rename_is_submitted_before_the_completion_arrives() {
    let mut host = activated_server();
    host.deliver_one(prompt_event(3, "Fix the login bug"));
    host.settle();
    assert!(renames(&host.submitted()).is_empty());
}

/// REQ: F-041
#[test]
fn f_041_completion_becomes_a_rename_with_source_inferred() {
    let mut host = activated_server();
    title_for_prompt(&mut host, 3, "Fix the login bug", "Login fix");
    let renames = renames(&host.submitted());
    assert_eq!(renames.len(), 1);
    assert_eq!(renames[0].node, NodeId(3));
    assert_eq!(renames[0].title, "Login fix");
    assert_eq!(renames[0].source, TitleSource::Inferred);
}

/// REQ: F-041
#[test]
fn f_041_completion_text_is_trimmed() {
    let mut host = activated_server();
    title_for_prompt(&mut host, 3, "Fix the login bug", "  \nLogin fix \n");
    assert_eq!(renames(&host.submitted())[0].title, "Login fix");
}

/// REQ: F-041
#[test]
fn f_041_each_pane_gets_its_own_title_when_two_requests_overlap() {
    let mut host = activated_server();
    host.deliver_one(prompt_event(3, "Fix the login bug"));
    host.deliver_one(prompt_event(4, "Add the billing page"));
    host.settle();
    let requests = host.inference_requests();
    assert_eq!(requests.len(), 2);
    host.complete_inference(requests[1].0, completion("Billing page"));
    host.complete_inference(requests[0].0, completion("Login fix"));
    host.settle();
    let renames = renames(&host.submitted());
    assert_eq!(renames.len(), 2);
    let for_node = |node: u64| renames.iter().find(|rename| rename.node == NodeId(node)).expect("rename").title.clone();
    assert_eq!(for_node(3), "Login fix");
    assert_eq!(for_node(4), "Billing page");
}

/// REQ: F-041
#[test]
fn f_041_a_completion_of_an_unknown_request_submits_nothing() {
    let mut host = activated_server();
    host.deliver_one(call_event(RequestId(9999), Outcome::Ok));
    host.settle();
    assert!(host.submitted().is_empty());
}

/// REQ: F-041
#[test]
fn f_041_a_completion_is_applied_once_only() {
    let mut host = activated_server();
    title_for_prompt(&mut host, 3, "Fix the login bug", "Login fix");
    let (id, _) = host.inference_requests().remove(0);
    host.deliver_one(call_event(id, Outcome::Ok));
    host.settle();
    assert_eq!(renames(&host.submitted()).len(), 1);
}

/// REQ: F-041
#[test]
fn f_041_a_failed_inference_call_submits_no_rename_and_logs_a_warning() {
    let mut host = activated_server();
    host.inject_failure("inference.complete", plugin_error(ErrorCode::NotFound));
    host.deliver_one(prompt_event(3, "Fix the login bug"));
    host.settle();
    assert!(renames(&host.submitted()).is_empty());
    assert!(host.logs().iter().any(|line| line.level.eq_ignore_ascii_case("warn")));
}

/// REQ: F-041
#[test]
fn f_041_a_failed_inference_call_does_not_disable_the_plugin() {
    let mut host = activated_server();
    host.inject_failure("inference.complete", plugin_error(ErrorCode::NotFound));
    host.deliver_one(prompt_event(3, "Fix the login bug"));
    host.settle();
    assert!(!host.is_disabled());
    assert!(host.failures().is_empty());
}

/// REQ: F-041
#[test]
fn f_041_the_next_prompt_after_a_failed_call_is_titled_normally() {
    let mut host = activated_server();
    host.inject_failure("inference.complete", plugin_error(ErrorCode::NotFound));
    host.deliver_one(prompt_event(3, "Fix the login bug"));
    host.settle();
    title_for_prompt(&mut host, 3, "Fix the login bug again", "Login fix");
    assert_eq!(renames(&host.submitted()).len(), 1);
}

/// REQ: F-041
#[test]
fn f_041_a_refused_conflict_for_a_hand_typed_name_is_accepted_without_retry() {
    let mut host = activated_server();
    title_for_prompt(&mut host, 3, "Fix the login bug", "Login fix");
    let correlation = host.submitted().iter().find(|s| matches!(s.intent, ilium_plugin_sdk::types::Intent::RenameNode(_))).expect("rename").correlation;
    host.deliver_one(outcome_event(correlation, Outcome::Refused(plugin_error(ErrorCode::Conflict))));
    host.settle();
    assert_eq!(renames(&host.submitted()).len(), 1);
    assert_eq!(host.inference_requests().len(), 1);
    assert!(!host.is_disabled());
}

/// REQ: F-041
#[test]
fn f_041_a_refused_conflict_stops_the_icon_for_that_title_too() {
    let mut host = activated_server();
    title_for_prompt(&mut host, 3, "Fix the login bug", "Fix login");
    let correlation = host.submitted().iter().find(|s| matches!(s.intent, ilium_plugin_sdk::types::Intent::RenameNode(_))).expect("rename").correlation;
    host.deliver_one(outcome_event(correlation, Outcome::Refused(plugin_error(ErrorCode::Conflict))));
    host.settle();
    assert!(host.failures().is_empty());
}

/// REQ: F-041
#[test]
fn f_041_activation_succeeds_with_the_manifest_grants() {
    let mut host = server_host();
    let result = host.try_activate::<ilium_plugin_core_titles::TitlesServer>();
    assert!(result.is_ok(), "{result:?}");
}

/// REQ: F-041
#[test]
fn f_041_a_prompt_event_without_text_is_not_sent_to_the_model_as_an_empty_message() {
    let mut host = activated_server();
    host.deliver_one(prompt_event(3, ""));
    host.settle();
    for (_, request) in host.inference_requests() {
        assert!(request.messages.iter().all(|message| !message.content.is_empty()));
    }
}

/// REQ: F-041
#[test]
fn f_041_events_the_plugin_does_not_handle_make_no_request() {
    let mut host = activated_server();
    host.deliver_one(Event::ThemeChanged);
    host.settle();
    assert!(host.inference_requests().is_empty());
    assert!(host.submitted().is_empty());
}

/// REQ: F-042
#[test]
fn f_042_page_says_new_sessions_get_a_name_and_a_name_can_be_triggered_by_hand() {
    let page = page_text();
    assert!(page.contains("**Session naming.** New sessions get a name, and you can trigger a name by hand"));
}

/// REQ: F-042
#[test]
fn f_042_user_doc_lists_the_three_manual_triggers() {
    let doc = user_doc("titles-and-instructions.md");
    assert!(doc.contains("- **Retitle** control on an agent row"));
    assert!(doc.contains("- **Restructure** on the tree toolbar, or on a project row"));
    assert!(doc.contains("- Voice: ask the assistant to retitle an entry or restructure a project."));
    assert!(doc.contains("Manual requests follow the same task checks as automatic ones."));
}

/// REQ: F-042
#[test]
fn f_042_the_agent_session_ready_default_retitles_the_element() {
    let doc = user_doc("settings.md");
    let row = table_row(&doc, "| Agent session ready |");
    assert_eq!(row[2], "Retitle element");
}

/// REQ: F-041
#[test]
fn f_041_manual_name_rules_are_stated_on_the_page_and_in_the_user_docs() {
    assert!(page_text().contains("a title that you typed by hand is never replaced by an inferred one"));
    let doc = user_doc("titles-and-instructions.md");
    assert!(doc.contains("Inferred titles stay automatic even after an explicit AI retitle; only a literal rename makes a title manual."));
}

/// REQ: F-041
#[test]
fn f_041_slash_clear_resets_automatic_fields_to_new_until_a_replacement_session_is_verified() {
    let doc = user_doc("titles-and-instructions.md");
    assert!(doc.contains("Submitting exactly `/clear` to Claude or Codex discards that pane's detected session."));
    assert!(doc.contains("Automatic title fields reset to `<new>`"));
}

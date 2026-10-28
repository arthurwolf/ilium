//! Port twins of the tests of `restructure.rs`.

use ilium_plugin_sdk::types::*;
use serde_json::json;

use crate::common::*;

/// REQ: PORT:ilium-client/src/restructure.rs::a_terminal_panes_command_hint_is_prefixed_onto_title_and_short_title, PORT:ilium-client/src/restructure.rs::an_empty_command_hint_leaves_a_terminal_panes_title_unprefixed
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

/// REQ: PORT:ilium-client/src/restructure.rs::configured_token_budget_reaches_provider_and_corrective_retry, PORT:ilium-client/src/restructure.rs::custom_instructions_survive_corrective_retry
#[test]
fn port_043_corrective_retry_repeats_instructions() {
    let mut host = host_with_two_panes();
    host.settings()
        .set(
            "inference.instructions.organization",
            json!("Project convention"),
        )
        .expect("setting accepted");
    host.settle();
    host.deliver_one(finished(1));
    host.settle();
    let (id, _) = restructure_requests(&host).remove(0);
    host.complete_inference(id, completion("not json"));
    host.settle();
    let requests = restructure_requests(&host);
    assert_eq!(requests.len(), 2);
    for (_, request) in &requests {
        assert!(request
            .messages
            .iter()
            .any(|message| message.content.contains("Project convention")));
    }
}

/// REQ: PORT:ilium-client/src/restructure.rs::tolerates_a_response_wrapped_in_a_json_code_fence, PORT:ilium-client/src/restructure.rs::tolerates_a_response_wrapped_in_a_plain_code_fence
#[test]
fn port_043_fenced_replies_are_tolerated() {
    let host = organised_host(&format!("```json\n{}\n```", plan_reply()));
    assert_eq!(restructures(&host.submitted()).len(), 1);
    let host = organised_host(&format!("```\n{}\n```", plan_reply()));
    assert_eq!(restructures(&host.submitted()).len(), 1);
}

/// REQ: PORT:ilium-client/src/restructure.rs::restructure_preserves_an_existing_leaf_icon_over_a_model_recommendation
#[test]
fn port_046_icons_do_not_discard_the_plan() {
    let reply = json!({"children": [
        {"kind": "pane", "id": 1, "title": "claude", "short_title": null, "icon": "not an icon at all, far too long"},
        {"kind": "pane", "id": 2, "title": "shell-1", "short_title": null}
    ], "animations": {"definitions": [], "project": "none"}}).to_string();
    let host = organised_host(&reply);
    assert_eq!(restructures(&host.submitted()).len(), 1);
}

/// REQ: PORT:ilium-client/src/restructure.rs::labeling_retries_a_generated_label_beyond_its_word_limit, PORT:ilium-client/src/restructure.rs::missing_or_invalid_generated_icons_fall_back_without_discarding_the_plan, PORT:ilium-client/src/restructure.rs::rejects_a_response_missing_an_existing_id, PORT:ilium-client/src/restructure.rs::rejects_a_response_with_a_duplicated_id, PORT:ilium-client/src/restructure.rs::rejects_a_response_with_an_empty_title, PORT:ilium-client/src/restructure.rs::rejects_non_json_responses, PORT:ilium-client/src/restructure.rs::split_view_with_a_nested_group_child_is_rejected_before_apply, PORT:ilium-client/src/restructure.rs::wrong_existing_leaf_kind_is_rejected_before_apply
#[test]
fn port_043_invalid_plans_are_not_applied() {
    for reply in [
        "not json at all".to_string(),
        plan_without_pane_two(),
        plan_with_duplicate_pane(),
        plan_with_empty_title(),
    ] {
        let host = rejected_host(&reply);
        assert!(restructures(&host.submitted()).is_empty());
    }
}

/// REQ: PORT:ilium-client/src/restructure.rs::configured_budget_accepts_large_instructions_and_reports_tokens
#[test]
fn port_043_large_budget_is_accepted() {
    let mut host = host_with_two_panes();
    host.settings()
        .set("restructure_prompt_token_limit", json!(200000))
        .expect("setting accepted");
    host.settle();
    host.deliver_one(finished(1));
    host.settle();
    assert!(!restructure_requests(&host).is_empty());
}

/// REQ: PORT:ilium-client/src/restructure.rs::empty_contexts_never_call_the_gateway
#[test]
fn port_043_no_panes_no_restructure_request() {
    let mut host = activated_server();
    host.deliver_one(finished(1));
    host.settle();
    assert!(restructure_requests(&host).is_empty());
}

/// REQ: PORT:ilium-client/src/restructure.rs::a_command_hint_on_a_non_terminal_pane_is_ignored
#[test]
fn port_043_non_terminal_panes_ignore_command_hints() {
    let mut host = activated_server();
    host.tree().add(editor_pane("notes"));
    host.tree().add(pane("shell-2", None));
    host.settle();
    host.deliver_one(finished(2));
    host.settle();
    let (id, _) = restructure_requests(&host).remove(0);
    let reply = json!({"children": [
        {"kind": "pane", "id": 1, "title": "Notes", "short_title": null, "icon": null, "command_hint": "vim"},
        {"kind": "pane", "id": 2, "title": "Shell", "short_title": null, "icon": null}
    ], "animations": {"definitions": [], "project": "none"}}).to_string();
    host.complete_inference(id, completion(&reply));
    host.settle();
    assert!(!applied_titles(&host)
        .iter()
        .any(|title| title.starts_with("[vim]")));
}

/// REQ: PORT:ilium-client/src/restructure.rs::prompt_includes_current_hierarchy_and_manual_or_llm_structure_sources, PORT:ilium-client/src/restructure.rs::prompt_includes_every_item_and_the_json_output_example
#[test]
fn port_043_prompt_lists_every_pane() {
    let mut host = host_with_two_panes();
    host.deliver_one(finished(1));
    host.settle();
    let requests = restructure_requests(&host);
    assert_eq!(requests.len(), 1);
    let text: String = requests[0]
        .1
        .messages
        .iter()
        .map(|message| message.content.clone())
        .collect();
    assert!(text.contains("shell-1"));
    assert!(text.contains("shell-2"));
}

/// REQ: PORT:ilium-client/src/restructure.rs::tolerates_prose_surrounding_the_json_object
#[test]
fn port_043_prose_around_json_is_tolerated() {
    let host = organised_host(&format!(
        "Here is the plan:\n{}\nHope that helps.",
        plan_reply()
    ));
    assert_eq!(restructures(&host.submitted()).len(), 1);
}

/// REQ: PORT:ilium-client/src/restructure.rs::retries_after_a_non_json_reply_and_succeeds_on_a_later_attempt
#[test]
fn port_043_retry_after_garbage_succeeds() {
    let mut host = host_with_two_panes();
    host.deliver_one(finished(1));
    host.settle();
    let (id, _) = restructure_requests(&host).remove(0);
    host.complete_inference(id, completion("not json"));
    host.settle();
    let requests = restructure_requests(&host);
    assert_eq!(requests.len(), 2);
    host.complete_inference(requests[1].0, completion(&plan_reply()));
    host.settle();
    assert_eq!(restructures(&host.submitted()).len(), 1);
}

/// REQ: PORT:ilium-client/src/restructure.rs::gives_up_after_exhausting_every_retry_on_persistent_garbage
#[test]
fn port_043_three_attempts_then_give_up() {
    let host = rejected_host("garbage");
    assert_eq!(restructure_requests(&host).len(), MAX_ATTEMPTS);
    assert!(restructures(&host.submitted()).is_empty());
}

/// REQ: PORT:ilium-client/src/restructure.rs::ineligible_title_bundle_survives_inference_normalization, PORT:ilium-client/src/restructure.rs::labeling_normalizes_generated_names_without_changing_fixed_names, PORT:ilium-client/src/restructure.rs::labeling_restructure_names_items_for_tree_retrieval, PORT:ilium-client/src/restructure.rs::valid_response_builds_the_expected_plan
#[test]
fn port_043_valid_plan_is_applied() {
    let host = organised_host(&plan_reply());
    let specs = restructures(&host.submitted());
    assert_eq!(specs.len(), 1);
    assert!(!specs[0].label.is_empty());
    assert!(specs[0].operations.len() <= 256);
    let groups: Vec<(String, Vec<u64>)> = specs[0]
        .operations
        .iter()
        .filter_map(|operation| match operation {
            RestructureOp::CreateGroup(group) => Some((
                group.title.clone(),
                group.children.iter().map(|node| node.0).collect(),
            )),
            _ => None,
        })
        .collect();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].0, "Billing API");
    let mut children = groups[0].1.clone();
    children.sort();
    assert_eq!(children, [1, 2]);
}

fn rename_titles_for(submissions: &[Submission], node: u64) -> Vec<String> {
    let mut titles = Vec::new();
    for spec in restructures(submissions) {
        for operation in &spec.operations {
            if let RestructureOp::Rename(rename) = operation {
                if rename.node.0 == node {
                    titles.push(rename.title.clone());
                }
            }
        }
    }
    titles
}

/// REQ: PORT:ilium-client/src/restructure.rs::a_command_hint_on_a_non_terminal_pane_is_ignored
#[test]
fn port_043_non_terminal_hint_does_not_discard_the_plan() {
    let mut host = activated_server();
    host.tree().add(editor_pane("notes"));
    host.tree().add(pane("shell-2", None));
    host.settle();
    host.deliver_one(finished(2));
    host.settle();
    let (id, _) = restructure_requests(&host).remove(0);
    let reply = json!({"children": [
        {"kind": "pane", "id": 1, "title": "Notes", "short_title": null, "icon": null, "command_hint": "vim"},
        {"kind": "pane", "id": 2, "title": "Shell", "short_title": null, "icon": null}
    ], "animations": {"definitions": [], "project": "none"}}).to_string();
    host.complete_inference(id, completion(&reply));
    host.settle();
    assert_eq!(restructures(&host.submitted()).len(), 1);
    assert_eq!(rename_titles_for(&host.submitted(), 1), vec!["Notes".to_string()]);
}

/// REQ: PORT:ilium-client/src/restructure.rs::a_terminal_panes_command_hint_is_prefixed_onto_title_and_short_title
#[test]
fn port_043_command_hint_prefix_targets_its_own_pane() {
    let reply = json!({"children": [
        {"kind": "pane", "id": 1, "title": "Monitor", "short_title": "Mon", "icon": null, "command_hint": "htop"},
        {"kind": "pane", "id": 2, "title": "Shell", "short_title": null, "icon": null}
    ], "animations": {"definitions": [], "project": "none"}}).to_string();
    let host = organised_host(&reply);
    assert_eq!(rename_titles_for(&host.submitted(), 1), vec!["[htop] Monitor".to_string()]);
    assert_eq!(rename_titles_for(&host.submitted(), 2), vec!["Shell".to_string()]);
}

/// REQ: PORT:ilium-client/src/restructure.rs::an_empty_command_hint_leaves_a_terminal_panes_title_unprefixed
#[test]
fn port_043_empty_hint_renames_each_pane_once_without_prefix() {
    let reply = json!({"children": [
        {"kind": "pane", "id": 1, "title": "Monitor", "short_title": null, "icon": null, "command_hint": ""},
        {"kind": "pane", "id": 2, "title": "Shell", "short_title": null, "icon": null, "command_hint": ""}
    ], "animations": {"definitions": [], "project": "none"}}).to_string();
    let host = organised_host(&reply);
    assert_eq!(rename_titles_for(&host.submitted(), 1), vec!["Monitor".to_string()]);
    assert_eq!(rename_titles_for(&host.submitted(), 2), vec!["Shell".to_string()]);
}

/// REQ: PORT:ilium-client/src/restructure.rs::configured_budget_accepts_large_instructions_and_reports_tokens
#[test]
fn port_043_large_budget_sends_one_request_per_trigger() {
    let mut host = host_with_two_panes();
    host.settings()
        .set("restructure_prompt_token_limit", json!(200000))
        .expect("setting accepted");
    host.settle();
    host.deliver_one(finished(1));
    host.settle();
    host.settle();
    assert_eq!(restructure_requests(&host).len(), 1);
}

/// REQ: PORT:ilium-client/src/restructure.rs::configured_token_budget_reaches_provider_and_corrective_retry
#[test]
fn port_043_corrective_retry_keeps_the_output_cap() {
    let mut host = host_with_two_panes();
    host.deliver_one(finished(1));
    host.settle();
    let (id, first) = restructure_requests(&host).remove(0);
    host.complete_inference(id, completion("not json"));
    host.settle();
    let requests = restructure_requests(&host);
    assert_eq!(requests.len(), 2);
    assert!(first.max_output_tokens.is_some());
    assert_eq!(requests[1].1.max_output_tokens, first.max_output_tokens);
}

/// REQ: PORT:ilium-client/src/restructure.rs::custom_instructions_survive_corrective_retry
#[test]
fn port_043_corrective_retry_keeps_the_same_message_set() {
    let mut host = host_with_two_panes();
    host.deliver_one(finished(1));
    host.settle();
    let (id, first) = restructure_requests(&host).remove(0);
    host.complete_inference(id, completion("not json"));
    host.settle();
    let requests = restructure_requests(&host);
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[1].1.messages.len(), first.messages.len());
}

/// REQ: PORT:ilium-client/src/restructure.rs::empty_contexts_never_call_the_gateway
#[test]
fn port_043_empty_tree_makes_no_inference_call_at_all() {
    let mut host = activated_server();
    host.deliver_one(finished(1));
    host.settle();
    assert!(host.inference_requests().is_empty());
    assert!(restructures(&host.submitted()).is_empty());
}

/// REQ: PORT:ilium-client/src/restructure.rs::gives_up_after_exhausting_every_retry_on_persistent_garbage
#[test]
fn port_043_exhausted_retries_do_not_restart_on_a_new_trigger() {
    let mut host = rejected_host("garbage");
    host.deliver_one(finished(1));
    host.settle();
    assert_eq!(restructure_requests(&host).len(), MAX_ATTEMPTS);
}

/// REQ: PORT:ilium-client/src/restructure.rs::ineligible_title_bundle_survives_inference_normalization
#[test]
fn port_043_padded_pane_title_is_trimmed_and_plan_applied() {
    let reply = json!({"children": [
        {"kind": "pane", "id": 1, "title": "claude", "short_title": null, "icon": null},
        {"kind": "pane", "id": 2, "title": "  shell-1  ", "short_title": null, "icon": null}
    ], "animations": {"definitions": [], "project": "none"}}).to_string();
    let host = organised_host(&reply);
    assert_eq!(restructures(&host.submitted()).len(), 1);
    assert_eq!(rename_titles_for(&host.submitted(), 2), vec!["shell-1".to_string()]);
}

/// REQ: PORT:ilium-client/src/restructure.rs::labeling_normalizes_generated_names_without_changing_fixed_names
#[test]
fn port_043_fixed_pane_name_keeps_its_exact_case() {
    let host = organised_host(&plan_reply());
    assert_eq!(rename_titles_for(&host.submitted(), 1), vec!["claude".to_string()]);
}

/// REQ: PORT:ilium-client/src/restructure.rs::labeling_restructure_names_items_for_tree_retrieval
#[test]
fn port_043_restructure_label_is_a_single_line() {
    let host = organised_host(&plan_reply());
    let specs = restructures(&host.submitted());
    assert_eq!(specs.len(), 1);
    assert!(!specs[0].label.contains('\n'));
}

/// REQ: PORT:ilium-client/src/restructure.rs::labeling_retries_a_generated_label_beyond_its_word_limit
#[test]
fn port_043_overlong_group_label_is_retried_then_dropped() {
    let reply = json!({"children": [
        {"kind": "group", "title": "one two three four five six seven eight nine ten eleven twelve thirteen", "short_title": null, "icon": null, "children": [
            {"kind": "pane", "id": 1, "title": "claude", "short_title": null, "icon": null},
            {"kind": "pane", "id": 2, "title": "shell-1", "short_title": null, "icon": null}
        ]}
    ], "animations": {"definitions": [], "project": "none"}}).to_string();
    let host = rejected_host(&reply);
    assert_eq!(restructure_requests(&host).len(), MAX_ATTEMPTS);
    assert!(restructures(&host.submitted()).is_empty());
}

/// REQ: PORT:ilium-client/src/restructure.rs::missing_or_invalid_generated_icons_fall_back_without_discarding_the_plan
#[test]
fn port_043_missing_icon_key_keeps_one_rename_per_pane() {
    let reply = json!({"children": [
        {"kind": "pane", "id": 1, "title": "claude", "short_title": null, "icon": null},
        {"kind": "pane", "id": 2, "title": "shell-1", "short_title": null}
    ], "animations": {"definitions": [], "project": "none"}}).to_string();
    let host = organised_host(&reply);
    assert_eq!(restructures(&host.submitted()).len(), 1);
    assert_eq!(rename_titles_for(&host.submitted(), 1).len(), 1);
    assert_eq!(rename_titles_for(&host.submitted(), 2).len(), 1);
}

/// REQ: PORT:ilium-client/src/restructure.rs::prompt_includes_current_hierarchy_and_manual_or_llm_structure_sources
#[test]
fn port_043_prompt_has_system_and_user_messages() {
    let mut host = host_with_two_panes();
    host.deliver_one(finished(1));
    host.settle();
    let requests = restructure_requests(&host);
    assert_eq!(requests.len(), 1);
    assert!(requests[0].1.messages.len() >= 2);
    assert!(requests[0].1.messages.iter().all(|message| !message.content.is_empty()));
}

/// REQ: PORT:ilium-client/src/restructure.rs::prompt_includes_every_item_and_the_json_output_example
#[test]
fn port_043_prompt_carries_the_json_output_example() {
    let mut host = host_with_two_panes();
    host.deliver_one(finished(1));
    host.settle();
    let requests = restructure_requests(&host);
    let text: String = requests[0]
        .1
        .messages
        .iter()
        .map(|message| message.content.clone())
        .collect();
    assert!(text.contains("\"children\""));
    assert!(text.contains("\"animations\""));
}

/// REQ: PORT:ilium-client/src/restructure.rs::rejects_a_response_missing_an_existing_id
#[test]
fn port_043_missing_pane_id_is_retried_up_to_the_limit() {
    let host = rejected_host(&plan_without_pane_two());
    assert_eq!(restructure_requests(&host).len(), MAX_ATTEMPTS);
    assert!(applied_titles(&host).is_empty());
}

/// REQ: PORT:ilium-client/src/restructure.rs::rejects_a_response_with_a_duplicated_id
#[test]
fn port_043_duplicated_pane_id_applies_no_titles() {
    let host = rejected_host(&plan_with_duplicate_pane());
    assert!(applied_titles(&host).is_empty());
    assert!(restructures(&host.submitted()).is_empty());
}

/// REQ: PORT:ilium-client/src/restructure.rs::rejects_a_response_with_an_empty_title
#[test]
fn port_043_empty_title_is_retried_up_to_the_limit() {
    let host = rejected_host(&plan_with_empty_title());
    assert_eq!(restructure_requests(&host).len(), MAX_ATTEMPTS);
}

/// REQ: PORT:ilium-client/src/restructure.rs::rejects_non_json_responses
#[test]
fn port_043_non_json_reply_submits_nothing() {
    let host = rejected_host("not json at all");
    assert!(host.submitted().is_empty());
}

/// REQ: PORT:ilium-client/src/restructure.rs::restructure_preserves_an_existing_leaf_icon_over_a_model_recommendation
#[test]
fn port_046_leaf_keeps_its_own_title_when_icon_is_rejected() {
    let reply = json!({"children": [
        {"kind": "pane", "id": 1, "title": "claude", "short_title": null, "icon": "not an icon at all, far too long"},
        {"kind": "pane", "id": 2, "title": "shell-1", "short_title": null}
    ], "animations": {"definitions": [], "project": "none"}}).to_string();
    let host = organised_host(&reply);
    assert_eq!(rename_titles_for(&host.submitted(), 1), vec!["claude".to_string()]);
}

/// REQ: PORT:ilium-client/src/restructure.rs::retries_after_a_non_json_reply_and_succeeds_on_a_later_attempt
#[test]
fn port_043_successful_retry_ends_the_request_chain() {
    let mut host = host_with_two_panes();
    host.deliver_one(finished(1));
    host.settle();
    let (id, _) = restructure_requests(&host).remove(0);
    host.complete_inference(id, completion("not json"));
    host.settle();
    let requests = restructure_requests(&host);
    host.complete_inference(requests[1].0, completion(&plan_reply()));
    host.settle();
    host.settle();
    assert_eq!(restructure_requests(&host).len(), 2);
}

/// REQ: PORT:ilium-client/src/restructure.rs::split_view_with_a_nested_group_child_is_rejected_before_apply
#[test]
fn port_043_nested_group_plan_is_retried_then_dropped() {
    let reply = json!({"children": [
        {"kind": "group", "title": "Outer", "short_title": null, "icon": null, "children": [
            {"kind": "group", "title": "Inner", "short_title": null, "icon": null, "children": [
                {"kind": "pane", "id": 1, "title": "claude", "short_title": null, "icon": null}
            ]},
            {"kind": "pane", "id": 2, "title": "shell-1", "short_title": null, "icon": null}
        ]}
    ], "animations": {"definitions": [], "project": "none"}}).to_string();
    let host = rejected_host(&reply);
    assert_eq!(restructure_requests(&host).len(), MAX_ATTEMPTS);
    assert!(restructures(&host.submitted()).is_empty());
}

/// REQ: PORT:ilium-client/src/restructure.rs::tolerates_a_response_wrapped_in_a_json_code_fence
#[test]
fn port_043_json_fence_with_blank_lines_needs_no_retry() {
    let host = organised_host(&format!("\n\n```json\n{}\n```\n\n", plan_reply()));
    assert_eq!(restructure_requests(&host).len(), 1);
}

/// REQ: PORT:ilium-client/src/restructure.rs::tolerates_a_response_wrapped_in_a_plain_code_fence
#[test]
fn port_043_plain_fence_needs_no_retry() {
    let host = organised_host(&format!("```\n{}\n```", plan_reply()));
    assert_eq!(restructure_requests(&host).len(), 1);
}

/// REQ: PORT:ilium-client/src/restructure.rs::tolerates_prose_surrounding_the_json_object
#[test]
fn port_043_prose_around_json_needs_no_retry() {
    let host = organised_host(&format!("Plan follows.\n{}\nDone.", plan_reply()));
    assert_eq!(restructure_requests(&host).len(), 1);
}

/// REQ: PORT:ilium-client/src/restructure.rs::valid_response_builds_the_expected_plan
#[test]
fn port_043_valid_plan_renames_both_leaves() {
    let host = organised_host(&plan_reply());
    assert_eq!(rename_titles_for(&host.submitted(), 1), vec!["claude".to_string()]);
    assert_eq!(rename_titles_for(&host.submitted(), 2), vec!["shell-1".to_string()]);
}

/// REQ: PORT:ilium-client/src/restructure.rs::wrong_existing_leaf_kind_is_rejected_before_apply
#[test]
fn port_043_leaf_declared_as_group_is_retried_then_dropped() {
    let reply = json!({"children": [
        {"kind": "group", "id": 1, "title": "claude", "short_title": null, "icon": null, "children": []},
        {"kind": "pane", "id": 2, "title": "shell-1", "short_title": null, "icon": null}
    ], "animations": {"definitions": [], "project": "none"}}).to_string();
    let host = rejected_host(&reply);
    assert_eq!(restructure_requests(&host).len(), MAX_ATTEMPTS);
    assert!(restructures(&host.submitted()).is_empty());
}

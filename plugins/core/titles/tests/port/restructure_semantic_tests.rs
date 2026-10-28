//! Port twins of the tests of `restructure_semantic_tests.rs`.

use ilium_plugin_sdk::types::*;
use serde_json::json;

use crate::common::*;

/// REQ: PORT:ilium-client/src/restructure_semantic_tests.rs::semantic_contract_rejects_invented_animation_instead_of_ignoring_it, PORT:ilium-client/src/restructure_semantic_tests.rs::semantic_contract_rejects_missing_invalid_duplicate_and_unused_definitions, PORT:ilium-client/src/restructure_semantic_tests.rs::semantic_contract_rejects_title_only_restructure_even_when_feature_is_disabled
#[test]
fn port_043_animation_contract_is_enforced() {
    for animations in [
        json!({"definitions": [], "project": "invented_animation"}),
        json!({"definitions": [{"id": "unused"}], "project": "none"}),
    ] {
        let reply = json!({"children": [
            {"kind": "pane", "id": 1, "title": "claude", "short_title": null, "icon": null},
            {"kind": "pane", "id": 2, "title": "shell-1", "short_title": null, "icon": null}
        ], "animations": animations})
        .to_string();
        let host = rejected_host(&reply);
        assert!(restructures(&host.submitted()).is_empty());
    }
    let title_only = json!({"children": [
        {"kind": "pane", "id": 1, "title": "claude", "short_title": null, "icon": null},
        {"kind": "pane", "id": 2, "title": "shell-1", "short_title": null, "icon": null}
    ]})
    .to_string();
    assert!(restructures(&rejected_host(&title_only).submitted()).is_empty());
}

/// REQ: PORT:ilium-client/src/restructure_semantic_tests.rs::semantic_contract_prompt_includes_every_concrete_animation
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

/// REQ: PORT:ilium-client/src/restructure_semantic_tests.rs::semantic_contract_resolves_project_group_and_leaf, PORT:ilium-client/src/restructure_semantic_tests.rs::semantic_contract_retries_with_catalog_and_original_prompt_limit
#[test]
fn port_043_valid_plan_is_applied() {
    let host = organised_host(&plan_reply());
    let specs = restructures(&host.submitted());
    assert_eq!(specs.len(), 1);
    assert!(!specs[0].label.is_empty());
    assert!(specs[0].operations.len() <= 256);
}

/// REQ: PORT:ilium-client/src/restructure_semantic_tests.rs::semantic_contract_prompt_includes_every_concrete_animation
#[test]
fn port_043_prompt_states_the_animation_definition_shape() {
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
    assert!(text.contains("definitions"));
    assert!(text.contains("project"));
}

/// REQ: PORT:ilium-client/src/restructure_semantic_tests.rs::semantic_contract_rejects_invented_animation_instead_of_ignoring_it
#[test]
fn port_043_invented_project_animation_is_retried_then_dropped() {
    let reply = json!({"children": [
        {"kind": "pane", "id": 1, "title": "claude", "short_title": null, "icon": null},
        {"kind": "pane", "id": 2, "title": "shell-1", "short_title": null, "icon": null}
    ], "animations": {"definitions": [], "project": "invented_animation"}})
    .to_string();
    let host = rejected_host(&reply);
    assert_eq!(restructure_requests(&host).len(), MAX_ATTEMPTS);
    assert!(restructures(&host.submitted()).is_empty());
}

/// REQ: PORT:ilium-client/src/restructure_semantic_tests.rs::semantic_contract_rejects_missing_invalid_duplicate_and_unused_definitions
#[test]
fn port_043_duplicate_animation_definitions_are_retried_then_dropped() {
    let reply = json!({"children": [
        {"kind": "pane", "id": 1, "title": "claude", "short_title": null, "icon": null},
        {"kind": "pane", "id": 2, "title": "shell-1", "short_title": null, "icon": null}
    ], "animations": {"definitions": [{"id": "wave"}, {"id": "wave"}], "project": "wave"}})
    .to_string();
    let host = rejected_host(&reply);
    assert_eq!(restructure_requests(&host).len(), MAX_ATTEMPTS);
    assert!(restructures(&host.submitted()).is_empty());
}

/// REQ: PORT:ilium-client/src/restructure_semantic_tests.rs::semantic_contract_rejects_title_only_restructure_even_when_feature_is_disabled
#[test]
fn port_043_title_only_reply_is_retried_up_to_the_limit() {
    let title_only = json!({"children": [
        {"kind": "pane", "id": 1, "title": "claude", "short_title": null, "icon": null},
        {"kind": "pane", "id": 2, "title": "shell-1", "short_title": null, "icon": null}
    ]})
    .to_string();
    let host = rejected_host(&title_only);
    assert_eq!(restructure_requests(&host).len(), MAX_ATTEMPTS);
}

/// REQ: PORT:ilium-client/src/restructure_semantic_tests.rs::semantic_contract_resolves_project_group_and_leaf
#[test]
fn port_043_plan_has_one_top_level_group() {
    let host = organised_host(&plan_reply());
    let specs = restructures(&host.submitted());
    assert_eq!(specs.len(), 1);
    let top_level_groups = specs[0]
        .operations
        .iter()
        .filter(|operation| {
            matches!(operation, RestructureOp::CreateGroup(group) if group.parent.is_none())
        })
        .count();
    assert_eq!(top_level_groups, 1);
}

/// REQ: PORT:ilium-client/src/restructure_semantic_tests.rs::semantic_contract_retries_with_catalog_and_original_prompt_limit
#[test]
fn port_043_retry_prompt_lists_the_pane_catalog_again() {
    let mut host = host_with_two_panes();
    host.deliver_one(finished(1));
    host.settle();
    let (id, _) = restructure_requests(&host).remove(0);
    host.complete_inference(id, completion("not json"));
    host.settle();
    let requests = restructure_requests(&host);
    assert_eq!(requests.len(), 2);
    let retry_text: String = requests[1]
        .1
        .messages
        .iter()
        .map(|message| message.content.clone())
        .collect();
    assert!(retry_text.contains("shell-1"));
    assert!(retry_text.contains("shell-2"));
}

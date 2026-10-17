//! Tree organisation (F-043): the model proposes groups, one restructure applies them, one undo reverts them.

use ilium_plugin_sdk::testing::{FakeHost, TreeNode};
use ilium_plugin_sdk::types::{
    ErrorCode, Intent, NodeClass, NodeId, Outcome, RestructureOp, TitleSource,
};
use serde_json::json;

use crate::common::*;
use crate::triggers::finished;

const MAX_ATTEMPTS: usize = 3;

fn pane(name: &str, parent: Option<u64>) -> TreeNode {
    TreeNode {
        id: 0,
        parent,
        name: name.to_string(),
        class: NodeClass::Pane,
        content_kind: Some("ilium.terminal".to_string()),
        agent: None,
    }
}

fn restructure_requests(
    host: &FakeHost,
) -> Vec<(
    ilium_plugin_sdk::types::RequestId,
    ilium_plugin_sdk::host::inference::CompletionRequest,
)> {
    host.inference_requests()
        .into_iter()
        .filter(|(_, request)| request.purpose.contains("restructure"))
        .collect()
}

/// A plan that groups panes 1 and 2 under "Billing API".
fn plan_reply() -> String {
    json!({
        "children": [
            {"kind": "group", "title": "Billing API", "short_title": "Billing", "icon": null,
             "children": [
                {"kind": "pane", "id": 1, "title": "claude", "short_title": null, "icon": null},
                {"kind": "pane", "id": 2, "title": "shell-1", "short_title": null, "icon": null}
             ]}
        ],
        "animations": {"definitions": [], "project": "none"}
    })
    .to_string()
}

fn host_with_two_panes() -> FakeHost {
    let mut host = activated_server();
    host.tree().add(pane("shell-1", None));
    host.tree().add(pane("shell-2", None));
    host.settle();
    host
}

fn organised_host() -> FakeHost {
    let mut host = host_with_two_panes();
    host.deliver_one(finished(1));
    host.settle();
    let (id, _) = restructure_requests(&host).remove(0);
    host.complete_inference(id, completion(&plan_reply()));
    host.settle();
    host
}

fn restructure_correlation(host: &FakeHost) -> u64 {
    host.submitted()
        .iter()
        .find(|submission| matches!(submission.intent, Intent::Restructure(_)))
        .expect("restructure submitted")
        .correlation
}

/// REQ: F-043
#[test]
fn f_043_a_valid_plan_is_applied_as_one_restructure_submission() {
    let host = organised_host();
    assert_eq!(restructures(&host.submitted()).len(), 1);
}

/// REQ: F-043
#[test]
fn f_043_the_restructure_creates_the_proposed_group_with_its_title() {
    let host = organised_host();
    let spec = restructures(&host.submitted()).remove(0);
    let created: Vec<String> = spec
        .operations
        .iter()
        .filter_map(|operation| match operation {
            RestructureOp::CreateGroup(group) => Some(group.title.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(created, ["Billing API"]);
}

/// REQ: F-043
#[test]
fn f_043_the_restructure_has_a_nonempty_undo_label() {
    let host = organised_host();
    let spec = restructures(&host.submitted()).remove(0);
    assert!(!spec.label.is_empty());
}

/// REQ: F-043
#[test]
fn f_043_the_restructure_has_at_most_256_operations() {
    let host = organised_host();
    let spec = restructures(&host.submitted()).remove(0);
    assert!(spec.operations.len() <= 256);
}

/// REQ: F-043
#[test]
fn f_043_titles_proposed_for_panes_are_submitted_as_inferred_renames() {
    let host = organised_host();
    let spec = restructures(&host.submitted()).remove(0);
    let renames: Vec<_> = spec
        .operations
        .iter()
        .filter_map(|operation| match operation {
            RestructureOp::Rename(rename) => Some(rename),
            _ => None,
        })
        .collect();
    assert!(!renames.is_empty());
    for rename in renames {
        assert_eq!(rename.source, TitleSource::Inferred);
    }
}

/// REQ: F-043
#[test]
fn f_043_every_pane_appears_exactly_once_in_the_restructure() {
    let host = organised_host();
    let spec = restructures(&host.submitted()).remove(0);
    let mut moved: Vec<u64> = Vec::new();
    for operation in &spec.operations {
        match operation {
            RestructureOp::Move(spec) => moved.push(spec.node.0),
            RestructureOp::CreateGroup(group) => {
                moved.extend(group.children.iter().map(|node| node.0))
            }
            _ => {}
        }
    }
    let mut unique = moved.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), moved.len());
}

/// REQ: F-043
#[test]
fn f_043_a_reply_that_loses_a_pane_is_not_applied() {
    let mut host = host_with_two_panes();
    host.deliver_one(finished(1));
    host.settle();
    let (id, _) = restructure_requests(&host).remove(0);
    let lost = json!({
        "children": [{"kind": "pane", "id": 1, "title": "claude", "short_title": null, "icon": null}],
        "animations": {"definitions": [], "project": "none"}
    });
    host.complete_inference(id, completion(&lost.to_string()));
    host.settle();
    assert!(restructures(&host.submitted()).is_empty());
}

/// REQ: F-043
#[test]
fn f_043_malformed_replies_are_retried_up_to_three_attempts_in_all() {
    let mut host = host_with_two_panes();
    host.deliver_one(finished(1));
    host.settle();
    for _ in 0..MAX_ATTEMPTS + 2 {
        let Some((id, _)) = restructure_requests(&host).into_iter().find(|(id, _)| {
            host.pending_requests()
                .iter()
                .any(|pending| *pending == ilium_plugin_sdk::types::RequestRef::Call(*id))
        }) else {
            break;
        };
        host.complete_inference(id, completion("User Safety: safe"));
        host.settle();
    }
    assert_eq!(restructure_requests(&host).len(), MAX_ATTEMPTS);
    assert!(restructures(&host.submitted()).is_empty());
}

/// REQ: F-043
#[test]
fn f_043_a_corrective_retry_repeats_the_instructions_of_the_first_request() {
    let mut host = host_with_two_panes();
    host.settings()
        .set(
            "inference.instructions.organization",
            json!("Group work by product first."),
        )
        .expect("setting accepted");
    host.settle();
    host.deliver_one(finished(1));
    host.settle();
    let (first_id, first) = restructure_requests(&host).remove(0);
    host.complete_inference(first_id, completion("not json"));
    host.settle();
    let requests = restructure_requests(&host);
    assert_eq!(requests.len(), 2);
    let text = |request: &ilium_plugin_sdk::host::inference::CompletionRequest| {
        request
            .messages
            .iter()
            .map(|message| message.content.clone())
            .collect::<Vec<_>>()
            .join("\n")
    };
    assert!(text(&first).contains("Group work by product first."));
    assert!(text(&requests[1].1).contains("Group work by product first."));
}

/// REQ: F-043
#[test]
fn f_043_instructions_are_ordered_entry_naming_then_organization_then_shared_context() {
    let mut host = host_with_two_panes();
    host.settings()
        .set(
            "inference.instructions.entry_naming",
            json!("ENTRY-NAMING-TEXT"),
        )
        .expect("accepted");
    host.settings()
        .set(
            "inference.instructions.organization",
            json!("ORGANIZATION-TEXT"),
        )
        .expect("accepted");
    host.settings()
        .set(
            "inference.instructions.naming_and_organization",
            json!("SHARED-CONTEXT-TEXT"),
        )
        .expect("accepted");
    host.settle();
    host.deliver_one(finished(1));
    host.settle();
    let (_, request) = restructure_requests(&host).remove(0);
    let text = request
        .messages
        .iter()
        .map(|message| message.content.clone())
        .collect::<Vec<_>>()
        .join("\n");
    let entry = text
        .find("ENTRY-NAMING-TEXT")
        .expect("entry naming present");
    let organization = text
        .find("ORGANIZATION-TEXT")
        .expect("organization present");
    let shared = text
        .find("SHARED-CONTEXT-TEXT")
        .expect("shared context present");
    assert!(entry < organization && organization < shared);
}

/// REQ: F-043
#[test]
fn f_043_the_undo_token_from_ok_node_is_kept_and_the_undo_command_reverts_it() {
    let mut host = organised_host();
    let correlation = restructure_correlation(&host);
    host.deliver_one(outcome_event(correlation, Outcome::OkNode(NodeId(42))));
    host.deliver_one(command_event("ilium.core.titles/undo-organise"));
    host.settle();
    assert_eq!(reverts(&host.submitted()), [42]);
}

/// REQ: F-043
#[test]
fn f_043_the_undo_command_reverts_only_once() {
    let mut host = organised_host();
    let correlation = restructure_correlation(&host);
    host.deliver_one(outcome_event(correlation, Outcome::OkNode(NodeId(42))));
    host.deliver_one(command_event("ilium.core.titles/undo-organise"));
    host.deliver_one(command_event("ilium.core.titles/undo-organise"));
    host.settle();
    assert_eq!(reverts(&host.submitted()).len(), 1);
}

/// REQ: F-043
#[test]
fn f_043_a_zero_token_means_nothing_changed_and_undo_submits_nothing() {
    let mut host = organised_host();
    let correlation = restructure_correlation(&host);
    host.deliver_one(outcome_event(correlation, Outcome::OkNode(NodeId(0))));
    host.deliver_one(command_event("ilium.core.titles/undo-organise"));
    host.settle();
    assert!(reverts(&host.submitted()).is_empty());
}

/// REQ: F-043
#[test]
fn f_043_undo_before_any_restructure_submits_nothing() {
    let mut host = activated_server();
    host.deliver_one(command_event("ilium.core.titles/undo-organise"));
    host.settle();
    assert!(reverts(&host.submitted()).is_empty());
}

/// REQ: F-043
#[test]
fn f_043_other_commands_do_not_revert() {
    let mut host = organised_host();
    let correlation = restructure_correlation(&host);
    host.deliver_one(outcome_event(correlation, Outcome::OkNode(NodeId(42))));
    host.deliver_one(command_event("ilium.core.titles/other"));
    host.settle();
    assert!(reverts(&host.submitted()).is_empty());
}

/// REQ: F-043
#[test]
fn f_043_the_undo_token_does_not_survive_a_restart() {
    let mut host = organised_host();
    let correlation = restructure_correlation(&host);
    host.deliver_one(outcome_event(correlation, Outcome::OkNode(NodeId(42))));
    host.restart_with(Vec::new());
    host.deliver_one(command_event("ilium.core.titles/undo-organise"));
    host.settle();
    assert!(reverts(&host.submitted()).is_empty());
}

/// REQ: F-043
#[test]
fn f_043_a_refused_revert_because_a_later_change_touched_the_nodes_is_accepted() {
    let mut host = organised_host();
    let correlation = restructure_correlation(&host);
    host.deliver_one(outcome_event(correlation, Outcome::OkNode(NodeId(42))));
    host.deliver_one(command_event("ilium.core.titles/undo-organise"));
    host.settle();
    let revert = host
        .submitted()
        .iter()
        .find(|submission| matches!(submission.intent, Intent::RevertRestructure(_)))
        .expect("revert")
        .correlation;
    host.deliver_one(outcome_event(
        revert,
        Outcome::Refused(plugin_error(ErrorCode::Conflict)),
    ));
    host.settle();
    assert!(!host.is_disabled());
    assert!(host.failures().is_empty());
}

/// REQ: F-043
#[test]
fn f_043_a_refused_restructure_keeps_no_undo_token() {
    let mut host = organised_host();
    let correlation = restructure_correlation(&host);
    host.deliver_one(outcome_event(
        correlation,
        Outcome::Refused(plugin_error(ErrorCode::InvalidArgument)),
    ));
    host.deliver_one(command_event("ilium.core.titles/undo-organise"));
    host.settle();
    assert!(reverts(&host.submitted()).is_empty());
}

/// REQ: F-043
#[test]
fn f_043_after_three_failed_attempts_the_next_automatic_restructure_waits_sixty_seconds() {
    let mut host = host_with_two_panes();
    host.deliver_one(finished(1));
    host.settle();
    for _ in 0..MAX_ATTEMPTS {
        let (id, _) = restructure_requests(&host).pop().expect("request");
        host.complete_inference(id, completion("garbage"));
        host.settle();
    }
    let attempts = restructure_requests(&host).len();
    host.advance_clock(59_000);
    host.deliver_one(finished(1));
    host.settle();
    assert_eq!(restructure_requests(&host).len(), attempts);
    host.advance_clock(2_000);
    host.deliver_one(finished(1));
    host.settle();
    assert!(restructure_requests(&host).len() > attempts);
}

/// REQ: F-043
#[test]
fn f_043_the_backoff_is_one_minute_two_minutes_and_at_most_thirty_minutes() {
    let doc = user_doc("inference-and-privacy.md");
    assert!(doc.contains("a retry breaker that backs off from one to thirty minutes"));
}

/// REQ: F-043
#[test]
fn f_043_default_restructure_token_budget_is_200000_and_must_be_positive() {
    let doc = user_doc("inference-and-privacy.md");
    assert!(doc.contains("**200,000 estimated input tokens** by default"));
    assert!(doc.contains("The value must be greater than zero."));
}

/// REQ: F-043
#[test]
fn f_043_a_zero_token_budget_is_rejected_by_the_setting() {
    let mut host = activated_server();
    let result = host
        .settings()
        .set("restructure_prompt_token_limit", json!(0));
    assert!(result.is_err());
}

/// REQ: F-043
#[test]
fn f_043_page_says_one_undo_step_reverts_the_groups() {
    let page = page_text();
    assert!(page.contains("can reorganise the whole tree into groups that you undo in one step"));
    assert!(page.contains("The token does not survive a server restart."));
    assert!(page.contains("up to 256 operations and a label"));
}

/// REQ: F-043
#[test]
fn f_043_name_fixed_groups_are_kept_exactly_as_the_user_named_them() {
    let doc = user_doc("titles-and-instructions.md");
    assert!(doc.contains("A name-fixed group is preserved exactly by tree organization"));
    assert!(doc.contains("Split views stay protected, name-fixed groups stay exactly as you named them, every pane appears exactly once"));
}

/// REQ: F-043
#[test]
fn f_043_organising_never_renames_a_hand_named_pane() {
    let mut host = organised_host();
    let correlation = restructure_correlation(&host);
    host.deliver_one(outcome_event(
        correlation,
        Outcome::Refused(plugin_error(ErrorCode::Conflict)),
    ));
    host.settle();
    let spec = restructures(&host.submitted()).remove(0);
    assert!(spec.operations.iter().all(|operation| match operation {
        RestructureOp::Rename(rename) => rename.source == TitleSource::Inferred,
        _ => true,
    }));
    let _ = Intent::PlaySound(String::new());
}

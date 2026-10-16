//! Inferred-title icons (F-046), sample 2 of the page: the title also picks the icon of the row.

use ilium_plugin_sdk::types::{ErrorCode, Intent, NodeId, Outcome};

use crate::common::*;

fn icon_for(title_reply: &str) -> Vec<ilium_plugin_sdk::types::SetInferredIconSpec> {
    let mut host = activated_server();
    title_for_prompt(&mut host, 5, "work on something", title_reply);
    icons(&host.submitted())
}

/// REQ: F-046
#[test]
fn f_046_a_title_about_a_fix_gets_the_wrench() {
    let icons = icon_for("Fix login");
    assert_eq!(icons.len(), 1);
    assert_eq!(icons[0].icon.as_deref(), Some("🔧"));
}

/// REQ: F-046
#[test]
fn f_046_a_title_about_a_test_gets_the_test_tube() {
    let icons = icon_for("Test the importer");
    assert_eq!(icons[0].icon.as_deref(), Some("🧪"));
}

/// REQ: F-046
#[test]
fn f_046_a_title_about_a_deploy_gets_the_rocket() {
    let icons = icon_for("Deploy staging");
    assert_eq!(icons[0].icon.as_deref(), Some("🚀"));
}

/// REQ: F-046
#[test]
fn f_046_topic_matching_ignores_letter_case() {
    let icons = icon_for("DEPLOY STAGING");
    assert_eq!(icons[0].icon.as_deref(), Some("🚀"));
}

/// REQ: F-046
#[test]
fn f_046_topic_table_order_decides_when_two_topics_match() {
    let icons = icon_for("Fix the deploy test");
    assert_eq!(icons[0].icon.as_deref(), Some("🔧"));
}

/// REQ: F-046
#[test]
fn f_046_a_title_without_a_topic_word_submits_no_icon() {
    assert!(icon_for("Billing API").is_empty());
}

/// REQ: F-046
#[test]
fn f_046_the_icon_intent_names_the_node_of_the_rename() {
    let icons = icon_for("Fix login");
    assert_eq!(icons[0].node, NodeId(5));
}

/// REQ: F-046
#[test]
fn f_046_the_icon_source_is_titler() {
    let icons = icon_for("Fix login");
    assert_eq!(icons[0].source, "titler");
}

/// REQ: F-046
#[test]
fn f_046_the_icon_follows_the_rename_in_submission_order() {
    let mut host = activated_server();
    title_for_prompt(&mut host, 5, "work on something", "Fix login");
    let order: Vec<&str> = host
        .submitted()
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::RenameNode(_) => Some("rename"),
            Intent::SetInferredIcon(_) => Some("icon"),
            _ => None,
        })
        .collect();
    assert_eq!(order, ["rename", "icon"]);
}

/// REQ: F-046
#[test]
fn f_046_a_manual_icon_wins_and_the_refusal_is_not_retried() {
    let mut host = activated_server();
    title_for_prompt(&mut host, 5, "work on something", "Fix login");
    let correlation = host
        .submitted()
        .iter()
        .find(|submission| matches!(submission.intent, Intent::SetInferredIcon(_)))
        .expect("icon submission")
        .correlation;
    host.deliver_one(outcome_event(correlation, Outcome::Refused(plugin_error(ErrorCode::Conflict))));
    host.settle();
    assert_eq!(icons(&host.submitted()).len(), 1);
    assert!(host.failures().is_empty());
}

/// REQ: F-046
#[test]
fn f_046_each_titled_pane_gets_its_own_icon() {
    let mut host = activated_server();
    title_for_prompt(&mut host, 5, "one", "Fix login");
    title_for_prompt(&mut host, 6, "two", "Deploy staging");
    let icons = icons(&host.submitted());
    assert_eq!(icons.len(), 2);
    assert_eq!(icons[0].node, NodeId(5));
    assert_eq!(icons[1].node, NodeId(6));
}

/// REQ: F-046
#[test]
fn f_046_page_states_a_manual_icon_is_never_replaced() {
    let page = page_text();
    assert!(page.contains("An icon that you set by hand is never replaced: the host answers `refused(conflict)`."));
}

/// REQ: F-046
#[test]
fn f_046_tree_decorate_is_granted_for_the_icon_intent() {
    let manifest = manifest_value();
    let grants = manifest["capabilities"]["server"].as_array().expect("grants");
    assert!(grants.iter().any(|grant| grant == "tree.decorate"));
}

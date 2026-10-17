//! Attention marks on nodes (page sample 4): a node that needs the user carries a mark until the
//! user looks at it; the plugin submits `mark-attention` with a reason of at most 80 characters and
//! `ack-attention` for the same node. Traced to F-048, the alert for events you may miss.

use ilium_plugin_sdk::types::{AgentActivity, Event};

use crate::common::*;

/// REQ: F-048
#[test]
fn attention_048_an_approval_prompt_marks_the_node() {
    let mut host = server_host();
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Working));
    let submissions = transition(
        &mut host,
        node,
        AgentActivity::Working,
        AgentActivity::AwaitingApproval,
        1,
    );
    let found = marks(&submissions);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].node, node);
}

/// REQ: F-048
#[test]
fn attention_048_the_mark_reason_is_not_empty() {
    let mut host = server_host();
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Working));
    let submissions = transition(
        &mut host,
        node,
        AgentActivity::Working,
        AgentActivity::AwaitingApproval,
        1,
    );
    assert!(!marks(&submissions)[0].reason.trim().is_empty());
}

/// REQ: F-048
#[test]
fn attention_048_the_mark_reason_is_at_most_80_characters_even_for_a_very_long_pane_name() {
    let mut host = server_host();
    let long_name = "x".repeat(300);
    let node = add_pane(&mut host, &long_name, Some(AgentActivity::Working));
    let submissions = transition(
        &mut host,
        node,
        AgentActivity::Working,
        AgentActivity::AwaitingApproval,
        1,
    );
    assert!(marks(&submissions)[0].reason.chars().count() <= 80);
}

/// REQ: F-048
#[test]
fn attention_048_a_working_agent_is_not_marked() {
    let mut host = server_host();
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Idle));
    let submissions = transition(
        &mut host,
        node,
        AgentActivity::Idle,
        AgentActivity::Working,
        1,
    );
    assert!(marks(&submissions).is_empty());
}

/// REQ: F-048
#[test]
fn attention_048_the_first_classification_as_working_is_not_marked() {
    let mut host = server_host();
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Working));
    let submissions = transition(
        &mut host,
        node,
        AgentActivity::None,
        AgentActivity::Working,
        1,
    );
    assert!(marks(&submissions).is_empty());
}

/// REQ: F-048
#[test]
fn attention_048_the_mark_is_independent_of_the_desktop_master_switch() {
    let mut host = server_host();
    crate::common::set_setting(&mut host, "notifications.enabled", serde_json::json!(false));
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Working));
    let submissions = transition(
        &mut host,
        node,
        AgentActivity::Working,
        AgentActivity::AwaitingApproval,
        1,
    );
    assert_eq!(marks(&submissions).len(), 1);
}

/// REQ: F-048
#[test]
fn attention_048_a_marked_node_that_the_user_looks_at_is_acknowledged() {
    let mut host = server_host();
    let node = add_pane(
        &mut host,
        "claude-api",
        Some(AgentActivity::AwaitingApproval),
    );
    host.deliver_one(Event::PaneFocused(Some(node)));
    let submissions = host.deliver_one(attention_event(node, true));
    assert_eq!(acks(&submissions), [node]);
}

/// REQ: F-048
#[test]
fn attention_048_a_marked_node_the_user_has_not_seen_stays_marked() {
    let mut host = server_host();
    let node = add_pane(
        &mut host,
        "claude-api",
        Some(AgentActivity::AwaitingApproval),
    );
    let other = add_pane(&mut host, "other", None);
    host.deliver_one(Event::PaneFocused(Some(other)));
    let submissions = host.deliver_one(attention_event(node, true));
    assert!(acks(&submissions).is_empty());
}

/// REQ: F-048
#[test]
fn attention_048_an_unmark_event_is_not_acknowledged_again() {
    let mut host = server_host();
    let node = add_pane(&mut host, "claude-api", Some(AgentActivity::Working));
    host.deliver_one(Event::PaneFocused(Some(node)));
    let submissions = host.deliver_one(attention_event(node, false));
    assert!(acks(&submissions).is_empty());
}

/// REQ: F-048
#[test]
fn attention_048_the_acknowledgement_names_the_same_node_as_the_event() {
    let mut host = server_host();
    let first = add_pane(&mut host, "one", Some(AgentActivity::AwaitingApproval));
    let second = add_pane(&mut host, "two", Some(AgentActivity::AwaitingApproval));
    host.deliver_one(Event::PaneFocused(Some(second)));
    let submissions = host.deliver_one(attention_event(second, true));
    assert_eq!(acks(&submissions), [second]);
    assert!(!acks(&submissions).contains(&first));
}

/// REQ: F-048
#[test]
fn attention_048_the_manifest_holds_the_attention_and_tree_read_grants() {
    let grants = server_grants();
    assert!(grants.contains(&"tree.attention".to_string()));
    assert!(grants.contains(&"tree.read".to_string()));
}

//! F-030 Normal and Attention display modes: `display-mode-changed` switches the panel between all agents
//! and the agents that need the user (page "Normal and Attention modes").

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{AgentActivity, Event, ViewTarget};

use crate::common::{add_agent_pane, client_host};

fn host_with_four_agents() -> FakeHost {
    let mut host = client_host();
    add_agent_pane(&mut host, "alpha", "claude", AgentActivity::Working, None);
    add_agent_pane(&mut host, "bravo", "codex", AgentActivity::Idle, None);
    add_agent_pane(&mut host, "charlie", "claude", AgentActivity::AwaitingApproval, None);
    add_agent_pane(&mut host, "delta", "claude", AgentActivity::Done, None);
    host.settle();
    host
}

fn shown(host: &mut FakeHost, columns: u16) -> Vec<&'static str> {
    let text = host.render(ViewTarget::Panel("agent-detect.panel".to_string()), columns, 12).text();
    ["alpha", "bravo", "charlie", "delta"].into_iter().filter(|name| text.contains(name)).collect()
}

/// REQ: F-030
#[test]
fn feature_030_normal_mode_lists_every_agent() {
    let mut host = host_with_four_agents();
    assert_eq!(shown(&mut host, 80), ["alpha", "bravo", "charlie", "delta"]);
}

/// REQ: F-030
#[test]
fn feature_030_attention_mode_lists_only_agents_awaiting_approval_or_done() {
    let mut host = host_with_four_agents();
    host.deliver_one(Event::DisplayModeChanged(Some("attention".to_string())));
    assert_eq!(shown(&mut host, 80), ["charlie", "delta"]);
}

/// REQ: F-030
#[test]
fn feature_030_attention_mode_hides_working_agents() {
    let mut host = host_with_four_agents();
    host.deliver_one(Event::DisplayModeChanged(Some("attention".to_string())));
    assert!(!shown(&mut host, 80).contains(&"alpha"));
}

/// REQ: F-030
#[test]
fn feature_030_attention_mode_hides_idle_agents() {
    let mut host = host_with_four_agents();
    host.deliver_one(Event::DisplayModeChanged(Some("attention".to_string())));
    assert!(!shown(&mut host, 80).contains(&"bravo"));
}

/// REQ: F-030
#[test]
fn feature_030_switching_back_to_normal_lists_every_agent_again() {
    let mut host = host_with_four_agents();
    host.deliver_one(Event::DisplayModeChanged(Some("attention".to_string())));
    host.deliver_one(Event::DisplayModeChanged(Some("normal".to_string())));
    assert_eq!(shown(&mut host, 80), ["alpha", "bravo", "charlie", "delta"]);
}

/// REQ: F-030
#[test]
fn feature_030_no_mode_means_normal() {
    let mut host = host_with_four_agents();
    host.deliver_one(Event::DisplayModeChanged(Some("attention".to_string())));
    host.deliver_one(Event::DisplayModeChanged(None));
    assert_eq!(shown(&mut host, 80), ["alpha", "bravo", "charlie", "delta"]);
}

/// REQ: F-030
#[test]
fn feature_030_an_unknown_mode_name_means_normal() {
    let mut host = host_with_four_agents();
    host.deliver_one(Event::DisplayModeChanged(Some("loud".to_string())));
    assert_eq!(shown(&mut host, 80), ["alpha", "bravo", "charlie", "delta"]);
}

/// REQ: F-030
#[test]
fn feature_030_the_mode_change_handler_submits_no_intents() {
    let mut host = host_with_four_agents();
    let submissions = host.deliver_one(Event::DisplayModeChanged(Some("attention".to_string())));
    assert!(submissions.is_empty());
}

/// REQ: F-030
#[test]
fn feature_030_attention_mode_filters_at_forty_and_one_hundred_twenty_columns() {
    for columns in [40u16, 120] {
        let mut host = host_with_four_agents();
        host.deliver_one(Event::DisplayModeChanged(Some("attention".to_string())));
        assert_eq!(shown(&mut host, columns), ["charlie", "delta"], "{columns} columns");
    }
}

/// REQ: F-030
#[test]
fn feature_030_an_agent_that_starts_needing_the_user_appears_in_attention_mode() {
    let mut host = host_with_four_agents();
    host.deliver_one(Event::DisplayModeChanged(Some("attention".to_string())));
    host.tree().set_activity(1, AgentActivity::AwaitingApproval);
    host.settle();
    assert_eq!(shown(&mut host, 80), ["alpha", "charlie", "delta"]);
}

/// REQ: F-030
#[test]
fn feature_030_the_attention_mode_contribution_hides_unmarked_rows_on_the_activity_slot() {
    let text = std::fs::read_to_string(crate::common::manifest_path()).expect("manifest");
    let value: serde_json::Value = serde_json::from_str(&text).expect("json");
    let mode = &value["contributes"]["display_modes"][0];
    assert_eq!(mode["id"], "attention");
    assert_eq!(mode["slot"], "activity");
    assert_eq!(mode["hide_unmarked"], true);
}

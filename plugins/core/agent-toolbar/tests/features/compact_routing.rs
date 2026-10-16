//! F-081 Compact button routing: remote compaction when on and the pane has a verified transcript, else `/compact`.

use ilium_plugin_sdk::types::{AgentActivity, CommandOrigin, Intent, PluginSide};
use serde_json::json;

use crate::common::*;

const COMPACTION_COMMAND: &str = "ilium.core.compaction/compact";

fn client_with(remote_enabled: bool, verified_transcript: bool) -> (ilium_plugin_sdk::testing::FakeHost, u64) {
    let mut host = client_host();
    let pane = add_agent_pane(&mut host, "claude", "claude", AgentActivity::Idle);
    host.settings().set("remote_compaction.enabled", json!(remote_enabled)).expect("setting accepted");
    host.agents().set_transcript_verified(pane, verified_transcript);
    (host, pane)
}

fn press(host: &mut ilium_plugin_sdk::testing::FakeHost, pane: u64) -> Vec<ilium_plugin_sdk::types::Submission> {
    host.deliver_one(command_on("compact", CommandOrigin::Toolbar, Some(pane)))
}

fn invoked_commands(submissions: &[ilium_plugin_sdk::types::Submission]) -> Vec<String> {
    submissions
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::InvokeCommand(spec) => Some(spec.command.clone()),
            _ => None,
        })
        .collect()
}

/// REQ: F-081
#[test]
fn f_081_remote_compaction_off_sends_slash_compact_through_the_server() {
    let (mut host, pane) = client_with(false, true);
    let submissions = press(&mut host, pane);
    assert!(invoked_commands(&submissions).is_empty());
    let sent = messages(&submissions);
    assert_eq!(sent.len(), 1);
    assert_eq!((sent[0].1, sent[0].2.as_str()), (PluginSide::Server, "compact"));
}

/// REQ: F-081
#[test]
fn f_081_remote_compaction_on_with_a_verified_transcript_starts_the_compaction_job() {
    let (mut host, pane) = client_with(true, true);
    let submissions = press(&mut host, pane);
    assert_eq!(invoked_commands(&submissions), vec![COMPACTION_COMMAND.to_string()]);
}

/// REQ: F-081
#[test]
fn f_081_remote_routing_does_not_also_send_slash_compact() {
    let (mut host, pane) = client_with(true, true);
    let submissions = press(&mut host, pane);
    assert!(messages(&submissions).is_empty());
}

/// REQ: F-081
#[test]
fn f_081_remote_compaction_on_without_a_verified_transcript_still_sends_slash_compact() {
    let (mut host, pane) = client_with(true, false);
    let submissions = press(&mut host, pane);
    assert!(invoked_commands(&submissions).is_empty());
    assert_eq!(messages(&submissions).len(), 1);
}

/// REQ: F-081
#[test]
fn f_081_both_off_and_unverified_sends_slash_compact() {
    let (mut host, pane) = client_with(false, false);
    let submissions = press(&mut host, pane);
    assert_eq!(messages(&submissions).len(), 1);
}

/// REQ: F-081
#[test]
fn f_081_turning_the_setting_off_again_restores_the_slash_compact_route() {
    let (mut host, pane) = client_with(true, true);
    host.settings().set("remote_compaction.enabled", json!(false)).expect("setting accepted");
    let submissions = press(&mut host, pane);
    assert!(invoked_commands(&submissions).is_empty());
    assert_eq!(messages(&submissions).len(), 1);
}

/// REQ: F-081
#[test]
fn f_081_turning_the_setting_on_while_active_switches_the_route_live() {
    let (mut host, pane) = client_with(false, true);
    host.settings().set("remote_compaction.enabled", json!(true)).expect("setting accepted");
    let submissions = press(&mut host, pane);
    assert_eq!(invoked_commands(&submissions), vec![COMPACTION_COMMAND.to_string()]);
}

/// REQ: F-081
#[test]
fn f_081_the_route_is_decided_per_pane() {
    let mut host = client_host();
    let verified = add_agent_pane(&mut host, "one", "claude", AgentActivity::Idle);
    let unverified = add_agent_pane(&mut host, "two", "codex", AgentActivity::Idle);
    host.settings().set("remote_compaction.enabled", json!(true)).expect("setting accepted");
    host.agents().set_transcript_verified(verified, true);
    host.agents().set_transcript_verified(unverified, false);
    assert_eq!(invoked_commands(&press(&mut host, verified)).len(), 1);
    assert_eq!(messages(&press(&mut host, unverified)).len(), 1);
}

/// REQ: F-081
#[test]
fn f_081_the_command_without_a_pane_starts_nothing_in_either_route() {
    let (mut host, _pane) = client_with(true, true);
    let submissions = host.deliver_one(command_on("compact", CommandOrigin::Toolbar, None));
    assert!(invoked_commands(&submissions).is_empty());
    assert!(messages(&submissions).is_empty());
}

/// REQ: F-081
#[test]
fn f_081_the_default_of_remote_compaction_is_off_so_the_button_sends_slash_compact() {
    let mut host = client_host();
    let pane = add_agent_pane(&mut host, "claude", "claude", AgentActivity::Idle);
    host.agents().set_transcript_verified(pane, true);
    assert_eq!(host.settings().get("remote_compaction.enabled"), Some(json!(false)));
    let submissions = press(&mut host, pane);
    assert_eq!(messages(&submissions).len(), 1);
}

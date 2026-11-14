//! F-081 Compact button routing: a pane with a verified transcript compacts through the compaction plugin, which
//! compacts remotely when its setting is on and hands `/compact` back otherwise; any other pane sends `/compact`.

use ilium_plugin_sdk::types::{
    AgentActivity, CommandOrigin, Event, Intent, Outcome, PluginError, PluginSide, RequestRef,
    RequestResult,
};

use crate::common::*;

const COMPACTION_COMMAND: &str = "ilium.core.compaction/compact";

fn client_with(verified_transcript: bool) -> (ilium_plugin_sdk::testing::FakeHost, u64) {
    let mut host = client_host();
    let pane = add_agent_pane(&mut host, "claude", "claude", AgentActivity::Idle);
    host.agents()
        .set_transcript_verified(pane, verified_transcript);
    (host, pane)
}

fn press(
    host: &mut ilium_plugin_sdk::testing::FakeHost,
    pane: u64,
) -> Vec<ilium_plugin_sdk::types::Submission> {
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
fn f_081_a_verified_transcript_goes_through_the_compaction_plugin() {
    let (mut host, pane) = client_with(true);
    let submissions = press(&mut host, pane);
    assert_eq!(
        invoked_commands(&submissions),
        vec![COMPACTION_COMMAND.to_string()]
    );
    assert!(messages(&submissions).is_empty());
}

/// REQ: F-081
#[test]
fn f_081_an_unverified_transcript_sends_slash_compact_through_the_server() {
    let (mut host, pane) = client_with(false);
    let submissions = press(&mut host, pane);
    assert!(invoked_commands(&submissions).is_empty());
    let sent = messages(&submissions);
    assert_eq!(sent.len(), 1);
    assert_eq!(
        (sent[0].1, sent[0].2.as_str()),
        (PluginSide::Server, "compact")
    );
}

/// REQ: F-081
#[test]
fn f_081_a_refused_compaction_command_falls_back_to_slash_compact() {
    let (mut host, pane) = client_with(true);
    let submissions = press(&mut host, pane);
    let correlation = submissions
        .iter()
        .find(|submission| matches!(submission.intent, Intent::InvokeCommand(_)))
        .expect("the compaction command was invoked")
        .correlation;
    let refused = host.deliver_one(Event::RequestCompleted(RequestResult {
        request: RequestRef::Submission(correlation),
        outcome: Outcome::Refused(PluginError::not_found("no such command")),
    }));
    let sent = messages(&refused);
    assert_eq!(sent.len(), 1);
    assert_eq!(
        (sent[0].1, sent[0].2.as_str()),
        (PluginSide::Server, "compact")
    );
}

/// REQ: F-081
#[test]
fn f_081_an_accepted_compaction_command_sends_nothing_more() {
    let (mut host, pane) = client_with(true);
    let submissions = press(&mut host, pane);
    let correlation = submissions
        .iter()
        .find(|submission| matches!(submission.intent, Intent::InvokeCommand(_)))
        .expect("the compaction command was invoked")
        .correlation;
    let accepted = host.deliver_one(Event::RequestCompleted(RequestResult {
        request: RequestRef::Submission(correlation),
        outcome: Outcome::Ok,
    }));
    assert!(messages(&accepted).is_empty());
}

/// REQ: F-081
#[test]
fn f_081_the_route_is_decided_per_pane() {
    let mut host = client_host();
    let verified = add_agent_pane(&mut host, "one", "claude", AgentActivity::Idle);
    let unverified = add_agent_pane(&mut host, "two", "codex", AgentActivity::Idle);
    host.agents().set_transcript_verified(verified, true);
    host.agents().set_transcript_verified(unverified, false);
    assert_eq!(invoked_commands(&press(&mut host, verified)).len(), 1);
    assert_eq!(messages(&press(&mut host, unverified)).len(), 1);
}

/// REQ: F-081
#[test]
fn f_081_the_command_without_a_pane_starts_nothing_in_either_route() {
    let (mut host, _pane) = client_with(true);
    let submissions = host.deliver_one(command_on("compact", CommandOrigin::Toolbar, None));
    assert!(invoked_commands(&submissions).is_empty());
    assert!(messages(&submissions).is_empty());
}

//! Twins of the interstitial prompt tests: the resume dialog of Claude Code is answered with the key 2.

use ilium_plugin_sdk::testing::{FakeHost, TreeNode};
use ilium_plugin_sdk::types::{
    AgentIdentifiedEvent, Delivery, Event, Intent, NodeClass, NodeId, ScreenChangedEvent,
    Submission,
};

use crate::common::{fixture, server_host};

fn pane_showing(host: &mut FakeHost, signature: &str, screen_name: &str) -> NodeId {
    let id = host.tree().add(TreeNode {
        id: 0,
        parent: None,
        name: "agent".to_string(),
        class: NodeClass::Pane,
        content_kind: Some("ilium.terminal".to_string()),
        agent: None,
    });
    host.tree().set_screen(
        id,
        fixture(screen_name).lines().map(str::to_string).collect(),
    );
    host.deliver_one(Event::AgentIdentified(AgentIdentifiedEvent {
        node: NodeId(id),
        signature: signature.to_string(),
    }));
    NodeId(id)
}

fn screen_changed(node: NodeId) -> Event {
    Event::ScreenChanged(ScreenChangedEvent {
        pane: node,
        first_row: 0,
        last_row: 40,
        revision: 1,
    })
}

fn keystrokes(submissions: &[Submission]) -> Vec<(NodeId, String)> {
    submissions
        .iter()
        .filter_map(|submission| match &submission.intent {
            Intent::SendInput(spec) => Some((spec.pane, spec.text.clone())),
            _ => None,
        })
        .collect()
}

/// REQ: PORT:ilium-detect/src/lib.rs::claude_code_resume_full_session_prompt_yields_key_2
#[test]
fn port_029_the_resume_dialog_is_answered_with_key_two() {
    let mut host = server_host();
    let pane = pane_showing(
        &mut host,
        "claude",
        "claude_code_resume_full_session_prompt.txt",
    );
    let submissions = host.deliver_one(screen_changed(pane));
    assert_eq!(keystrokes(&submissions), [(pane, "2".to_string())]);
    match &submissions[0].intent {
        Intent::SendInput(spec) => assert_eq!(spec.delivery, Delivery::Typed),
        other => panic!("expected send-input, got {other:?}"),
    }
}

/// REQ: PORT:ilium-server/tests/live_agent_detection.rs::claude_resume_full_session_prompt_is_auto_answered
#[test]
fn port_029_a_claude_pane_with_the_dialog_is_auto_answered_once() {
    let mut host = server_host();
    let pane = pane_showing(
        &mut host,
        "claude",
        "claude_code_resume_full_session_prompt.txt",
    );
    assert_eq!(keystrokes(&host.deliver_one(screen_changed(pane))).len(), 1);
    assert!(keystrokes(&host.deliver_one(screen_changed(pane))).is_empty());
}

/// REQ: PORT:ilium-detect/src/lib.rs::resume_prompt_does_not_match_for_codex
#[test]
fn port_029_the_dialog_is_not_answered_for_codex() {
    let mut host = server_host();
    let pane = pane_showing(
        &mut host,
        "codex",
        "claude_code_resume_full_session_prompt.txt",
    );
    assert!(keystrokes(&host.deliver_one(screen_changed(pane))).is_empty());
}

/// REQ: PORT:ilium-detect/src/lib.rs::resume_prompt_wording_quoted_in_transcript_does_not_match
#[test]
fn port_029_quoted_dialog_wording_is_not_answered() {
    let mut host = server_host();
    let pane = pane_showing(
        &mut host,
        "claude",
        "claude_code_resume_prompt_quoted_in_transcript.txt",
    );
    assert!(keystrokes(&host.deliver_one(screen_changed(pane))).is_empty());
}

/// REQ: PORT:ilium-detect/src/lib.rs::plain_shell_has_no_interstitial_prompt
#[test]
fn port_029_a_plain_shell_has_no_interstitial_prompt() {
    let mut host = server_host();
    let pane = pane_showing(&mut host, "claude", "plain_shell.txt");
    assert!(keystrokes(&host.deliver_one(screen_changed(pane))).is_empty());
}

/// REQ: PORT:ilium-detect/src/lib.rs::plain_shell_has_no_interstitial_prompt
#[test]
fn port_029_a_pane_without_an_agent_identity_is_never_answered() {
    let mut host = server_host();
    let pane = host.tree().add(TreeNode {
        id: 0,
        parent: None,
        name: "shell".to_string(),
        class: NodeClass::Pane,
        content_kind: Some("ilium.terminal".to_string()),
        agent: None,
    });
    host.tree().set_screen(
        pane,
        fixture("plain_shell.txt")
            .lines()
            .map(str::to_string)
            .collect(),
    );
    assert!(keystrokes(&host.deliver_one(screen_changed(NodeId(pane)))).is_empty());
}

/// REQ: PORT:ilium-detect/src/lib.rs::resume_prompt_does_not_match_for_codex
#[test]
fn port_029_the_dialog_stays_unanswered_across_repeated_screen_changes_for_codex() {
    let mut host = server_host();
    let pane = pane_showing(
        &mut host,
        "codex",
        "claude_code_resume_full_session_prompt.txt",
    );
    assert!(keystrokes(&host.deliver_one(screen_changed(pane))).is_empty());
    assert!(keystrokes(&host.deliver_one(screen_changed(pane))).is_empty());
}

/// REQ: PORT:ilium-detect/src/lib.rs::resume_prompt_wording_quoted_in_transcript_does_not_match
#[test]
fn port_029_a_changed_dialog_wording_is_not_answered() {
    let mut host = server_host();
    let screen = fixture("claude_code_resume_full_session_prompt.txt").replace(
        "Resume full session as-is",
        "Resume full session, rewritten",
    );
    let pane = host.tree().add(TreeNode {
        id: 0,
        parent: None,
        name: "agent".to_string(),
        class: NodeClass::Pane,
        content_kind: Some("ilium.terminal".to_string()),
        agent: None,
    });
    host.tree()
        .set_screen(pane, screen.lines().map(str::to_string).collect());
    host.deliver_one(Event::AgentIdentified(AgentIdentifiedEvent {
        node: NodeId(pane),
        signature: "claude".to_string(),
    }));
    assert!(keystrokes(&host.deliver_one(screen_changed(NodeId(pane)))).is_empty());
}

/// REQ: PORT:ilium-server/tests/live_agent_detection.rs::claude_resume_full_session_prompt_is_auto_answered
#[test]
fn port_029_each_claude_process_with_the_dialog_is_answered_once() {
    let mut host = server_host();
    let first = pane_showing(
        &mut host,
        "claude",
        "claude_code_resume_full_session_prompt.txt",
    );
    let second = pane_showing(
        &mut host,
        "claude",
        "claude_code_resume_full_session_prompt.txt",
    );
    assert_eq!(
        keystrokes(&host.deliver_one(screen_changed(first))),
        [(first, "2".to_string())]
    );
    assert_eq!(
        keystrokes(&host.deliver_one(screen_changed(second))),
        [(second, "2".to_string())]
    );
}

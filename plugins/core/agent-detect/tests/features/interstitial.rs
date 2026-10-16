//! COR-011 auto-answering of interstitial prompts: Claude Code's "resume full session" dialog is answered with
//! the single key `2`, through `terminal.write` (an input intent), once per agent process.

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{AgentIdentifiedEvent, Delivery, Event, Intent, NodeId, ScreenChangedEvent, Submission};
use serde_json::json;

use crate::common::{fixture, server_host};

/// A terminal pane showing `screen_name` whose process was identified as `signature`.
fn pane_showing(host: &mut FakeHost, signature: &str, screen_name: &str) -> NodeId {
    let id = host.tree().add(ilium_plugin_sdk::testing::TreeNode {
        id: 0,
        parent: None,
        name: "agent".to_string(),
        class: ilium_plugin_sdk::types::NodeClass::Pane,
        content_kind: Some("ilium.terminal".to_string()),
        agent: None,
    });
    host.tree().set_screen(id, fixture(screen_name).lines().map(str::to_string).collect());
    host.deliver_one(Event::AgentIdentified(AgentIdentifiedEvent { node: NodeId(id), signature: signature.to_string() }));
    NodeId(id)
}

fn screen_changed(node: NodeId) -> Event {
    Event::ScreenChanged(ScreenChangedEvent { pane: node, first_row: 0, last_row: 40, revision: 1 })
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

/// REQ: COR-011
#[test]
fn core_011_the_resume_full_session_dialog_is_answered_with_key_two() {
    let mut host = server_host();
    let pane = pane_showing(&mut host, "claude", "claude_code_resume_full_session_prompt.txt");
    let submissions = host.deliver_one(screen_changed(pane));
    assert_eq!(keystrokes(&submissions), [(pane, "2".to_string())]);
}

/// REQ: COR-011
#[test]
fn core_011_the_key_is_typed_without_submitting_a_line() {
    let mut host = server_host();
    let pane = pane_showing(&mut host, "claude", "claude_code_resume_full_session_prompt.txt");
    let submissions = host.deliver_one(screen_changed(pane));
    match &submissions[0].intent {
        Intent::SendInput(spec) => {
            assert_eq!(spec.delivery, Delivery::Typed);
            assert!(spec.stages.is_empty());
        }
        other => panic!("expected send-input, got {other:?}"),
    }
}

/// REQ: COR-011
#[test]
fn core_011_the_same_process_is_never_answered_twice() {
    let mut host = server_host();
    let pane = pane_showing(&mut host, "claude", "claude_code_resume_full_session_prompt.txt");
    let first = host.deliver_one(screen_changed(pane));
    let second = host.deliver_one(screen_changed(pane));
    assert_eq!(keystrokes(&first).len(), 1);
    assert!(keystrokes(&second).is_empty());
}

/// REQ: COR-011
#[test]
fn core_011_a_second_pane_with_the_same_dialog_is_answered_separately() {
    let mut host = server_host();
    let first = pane_showing(&mut host, "claude", "claude_code_resume_full_session_prompt.txt");
    let second = pane_showing(&mut host, "claude", "claude_code_resume_full_session_prompt.txt");
    let one = host.deliver_one(screen_changed(first));
    let two = host.deliver_one(screen_changed(second));
    assert_eq!(keystrokes(&one), [(first, "2".to_string())]);
    assert_eq!(keystrokes(&two), [(second, "2".to_string())]);
}

/// REQ: COR-011
#[test]
fn core_011_the_dialog_wording_quoted_in_a_transcript_is_not_answered() {
    let mut host = server_host();
    let pane = pane_showing(&mut host, "claude", "claude_code_resume_prompt_quoted_in_transcript.txt");
    assert!(keystrokes(&host.deliver_one(screen_changed(pane))).is_empty());
}

/// REQ: COR-011
#[test]
fn core_011_the_dialog_is_not_answered_for_a_codex_pane() {
    let mut host = server_host();
    let pane = pane_showing(&mut host, "codex", "claude_code_resume_full_session_prompt.txt");
    assert!(keystrokes(&host.deliver_one(screen_changed(pane))).is_empty());
}

/// REQ: COR-011
#[test]
fn core_011_a_plain_shell_screen_is_not_answered() {
    let mut host = server_host();
    let pane = pane_showing(&mut host, "claude", "plain_shell.txt");
    assert!(keystrokes(&host.deliver_one(screen_changed(pane))).is_empty());
}

/// REQ: COR-011
#[test]
fn core_011_an_ordinary_claude_idle_screen_is_not_answered() {
    let mut host = server_host();
    let pane = pane_showing(&mut host, "claude", "claude_code_idle.txt");
    assert!(keystrokes(&host.deliver_one(screen_changed(pane))).is_empty());
}

/// REQ: COR-011
#[test]
fn core_011_turning_the_toggle_off_stops_the_auto_answer() {
    let mut host = server_host();
    host.settings().set("detection.auto_answer_interstitial_prompts", json!(false)).expect("setting");
    let pane = pane_showing(&mut host, "claude", "claude_code_resume_full_session_prompt.txt");
    assert!(keystrokes(&host.deliver_one(screen_changed(pane))).is_empty());
}

/// REQ: COR-011
#[test]
fn core_011_turning_the_toggle_back_on_answers_the_next_dialog() {
    let mut host = server_host();
    host.settings().set("detection.auto_answer_interstitial_prompts", json!(false)).expect("off");
    host.settings().set("detection.auto_answer_interstitial_prompts", json!(true)).expect("on");
    let pane = pane_showing(&mut host, "claude", "claude_code_resume_full_session_prompt.txt");
    assert_eq!(keystrokes(&host.deliver_one(screen_changed(pane))).len(), 1);
}

/// REQ: COR-011
#[test]
fn core_011_a_screen_change_before_the_agent_is_identified_is_not_answered() {
    let mut host = server_host();
    let id = host.tree().add(ilium_plugin_sdk::testing::TreeNode {
        id: 0,
        parent: None,
        name: "agent".to_string(),
        class: ilium_plugin_sdk::types::NodeClass::Pane,
        content_kind: Some("ilium.terminal".to_string()),
        agent: None,
    });
    host.tree().set_screen(id, fixture("claude_code_resume_full_session_prompt.txt").lines().map(str::to_string).collect());
    assert!(keystrokes(&host.deliver_one(screen_changed(NodeId(id)))).is_empty());
}

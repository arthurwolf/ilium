//! F-076: resume verified agent sessions (page sample "Resume verified agent sessions";
//! user doc session-recovery.md "Resuming agent sessions").
//!
//! The trigger is a `pane-created` event with `PaneInfo.is_restored` set (see ../API_GAPS.md).

use ilium_plugin_sdk::types::{FsPath, NodeId};
use proptest::prelude::*;

use crate::common::*;

fn restored_agent(signature_id: &str, template: Option<Vec<String>>, session: &str) -> (ilium_plugin_sdk::testing::FakeHost, u64) {
    let mut host = activated_server();
    let pane = add_agent(&mut host, "agent", signature_id);
    verify_agent(&mut host, pane, signature(signature_id, template), session);
    (host, pane)
}

/// REQ: F-076
#[test]
fn resume_076_claude_pane_resumes_with_claude_resume_and_the_session_id() {
    let (mut host, pane) = restored_agent("claude", Some(claude_template()), "c0ffee-1234");
    let submitted = restore_panes(&mut host, &[pane]);
    let commands = replace_commands(&submitted);
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].0.pane, NodeId(pane));
    assert_eq!(commands[0].0.command, ["claude", "--resume", "c0ffee-1234"]);
}

/// REQ: F-076
#[test]
fn resume_076_codex_pane_resumes_with_codex_resume_and_the_session_id() {
    let (mut host, pane) = restored_agent("codex", Some(codex_template()), "019a-bbbb");
    let commands = replace_commands(&restore_panes(&mut host, &[pane]));
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].0.command, ["codex", "resume", "019a-bbbb"]);
}

/// REQ: F-076
#[test]
fn resume_076_antigravity_pane_resumes_with_agy_conversation() {
    let (mut host, pane) = restored_agent("antigravity", Some(antigravity_template()), "conv-77");
    let commands = replace_commands(&restore_panes(&mut host, &[pane]));
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].0.command, ["agy", "--conversation", "conv-77"]);
}

/// REQ: F-076
#[test]
fn resume_076_scrollback_is_kept_and_cwd_is_left_unset() {
    let (mut host, pane) = restored_agent("claude", Some(claude_template()), "abc");
    let commands = replace_commands(&restore_panes(&mut host, &[pane]));
    assert!(commands[0].0.keep_scrollback);
    assert_eq!(commands[0].0.cwd, None::<FsPath>);
}

/// REQ: F-076
#[test]
fn resume_076_dedup_key_is_resume_dash_pane() {
    let (mut host, pane) = restored_agent("claude", Some(claude_template()), "abc");
    let commands = replace_commands(&restore_panes(&mut host, &[pane]));
    assert_eq!(commands[0].1, Some(format!("resume-{pane}")));
}

/// REQ: F-076
#[test]
fn resume_076_correlation_numbers_start_at_two_to_the_63() {
    let (mut host, pane) = restored_agent("claude", Some(claude_template()), "abc");
    let submitted = restore_panes(&mut host, &[pane]);
    let resume = submitted
        .iter()
        .find(|submission| matches!(submission.intent, ilium_plugin_sdk::types::Intent::ReplacePaneCommand(_)))
        .expect("a resume submission");
    assert!(resume.correlation >= 1u64 << 63);
}

/// REQ: F-076
#[test]
fn resume_076_pane_without_a_verified_session_is_not_resumed() {
    let mut host = activated_server();
    let pane = add_agent(&mut host, "agent", "claude");
    register_signature(&mut host, signature("claude", Some(claude_template())));
    assert!(replace_commands(&restore_panes(&mut host, &[pane])).is_empty());
}

/// REQ: F-076
#[test]
fn resume_076_plain_shell_pane_is_not_resumed() {
    let mut host = activated_server();
    let pane = add_shell(&mut host, "shell");
    assert!(replace_commands(&restore_panes(&mut host, &[pane])).is_empty());
}

/// REQ: F-076
#[test]
fn resume_076_signature_without_resume_template_is_not_resumed() {
    let (mut host, pane) = restored_agent("custom-agent", None, "abc");
    assert!(replace_commands(&restore_panes(&mut host, &[pane])).is_empty());
}

/// REQ: F-076
#[test]
fn resume_076_pane_created_live_is_not_resumed() {
    let (mut host, pane) = restored_agent("claude", Some(claude_template()), "abc");
    let submitted = host.deliver(vec![pane_created(pane, false)]);
    assert!(replace_commands(&submitted).is_empty());
}

/// REQ: F-076
#[test]
fn resume_076_every_session_token_of_the_template_is_replaced() {
    let template = vec!["tool".to_string(), "--a={session}".to_string(), "{session}".to_string()];
    let (mut host, pane) = restored_agent("tool", Some(template), "s-1");
    let commands = replace_commands(&restore_panes(&mut host, &[pane]));
    assert_eq!(commands[0].0.command, ["tool", "--a=s-1", "s-1"]);
}

/// REQ: F-076
#[test]
fn resume_076_template_parts_without_a_token_are_kept_verbatim() {
    let template = vec!["tool".to_string(), "--flag".to_string(), "{session}".to_string(), "--tail".to_string()];
    let (mut host, pane) = restored_agent("tool", Some(template), "s-2");
    let commands = replace_commands(&restore_panes(&mut host, &[pane]));
    assert_eq!(commands[0].0.command, ["tool", "--flag", "s-2", "--tail"]);
}

/// REQ: F-076
#[test]
fn resume_076_two_agent_panes_each_get_their_own_command_and_key() {
    let mut host = activated_server();
    let first = add_agent(&mut host, "one", "claude");
    let second = add_agent(&mut host, "two", "codex");
    verify_agent(&mut host, first, signature("claude", Some(claude_template())), "id-one");
    verify_agent(&mut host, second, signature("codex", Some(codex_template())), "id-two");
    let commands = replace_commands(&restore_panes(&mut host, &[first, second]));
    assert_eq!(commands.len(), 2);
    assert_eq!(commands[0].0.pane, NodeId(first));
    assert_eq!(commands[0].0.command, ["claude", "--resume", "id-one"]);
    assert_eq!(commands[0].1, Some(format!("resume-{first}")));
    assert_eq!(commands[1].0.pane, NodeId(second));
    assert_eq!(commands[1].0.command, ["codex", "resume", "id-two"]);
    assert_eq!(commands[1].1, Some(format!("resume-{second}")));
}

/// REQ: F-076
#[test]
fn resume_076_only_the_verified_pane_of_two_is_resumed() {
    let mut host = activated_server();
    let verified = add_agent(&mut host, "verified", "claude");
    let unverified = add_agent(&mut host, "unverified", "claude");
    verify_agent(&mut host, verified, signature("claude", Some(claude_template())), "only-me");
    let commands = replace_commands(&restore_panes(&mut host, &[verified, unverified]));
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].0.pane, NodeId(verified));
}

/// REQ: F-076
#[test]
fn resume_076_a_resumed_pane_is_still_frozen_during_the_restore() {
    let (mut host, pane) = restored_agent("claude", Some(claude_template()), "abc");
    let submitted = restore_panes(&mut host, &[pane]);
    assert_eq!(freezes(&submitted).len(), 1);
    assert_eq!(replace_commands(&submitted).len(), 1);
}

/// REQ: F-076
#[test]
fn resume_076_session_documentation_table_lists_the_three_commands() {
    let doc = user_doc("session-recovery.md");
    assert!(doc.contains("| Claude Code | `claude --resume <session id>` |"));
    assert!(doc.contains("| Codex | `codex resume <session id>` |"));
    assert!(doc.contains("| Antigravity | `agy --conversation <conversation id>` |"));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]

    // The session id lands verbatim in the resume command, whatever characters it holds.
    // REQ: F-076
    #[test]
    fn resume_076_session_id_round_trips_into_the_command(session in "[A-Za-z0-9-]{1,40}") {
        let (mut host, pane) = restored_agent("claude", Some(claude_template()), &session);
        let commands = replace_commands(&restore_panes(&mut host, &[pane]));
        prop_assert_eq!(commands.len(), 1);
        prop_assert_eq!(commands[0].0.command.clone(), vec!["claude".to_string(), "--resume".to_string(), session.clone()]);
    }
}

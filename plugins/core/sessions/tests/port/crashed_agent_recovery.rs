//! Twins of ilium-server/tests/crashed_agent_recovery.rs: after a crash the agent keeps its
//! identity and verified session; the plugin resumes exactly that session and never replays input.

use ilium_plugin_sdk::types::Intent;

use crate::common::*;

fn crashed_then_restored(name: &str, template: Vec<String>) {
    let mut host = activated_server();
    let pane = add_agent(&mut host, "crashed agent", name);
    verify_agent(
        &mut host,
        pane,
        signature(name, Some(template.clone())),
        SESSION_ID,
    );
    let submitted = restore_panes(&mut host, &[pane]);
    let commands = replace_commands(&submitted);
    assert_eq!(commands.len(), 1, "{name}: exactly one resume");
    let expected: Vec<String> = template
        .iter()
        .map(|part| part.replace("{session}", SESSION_ID))
        .collect();
    assert_eq!(commands[0].0.command, expected, "{name}");
    assert!(
        commands[0].0.keep_scrollback,
        "{name}: output above the resumed conversation is kept"
    );
    // Only freeze and resume intents: no text is typed, no Enter, nothing is replayed.
    assert!(
        submitted.iter().all(|s| matches!(
            s.intent,
            Intent::ReplacePaneCommand(_) | Intent::FreezePane(_)
        )),
        "{name}"
    );
}

/// REQ: PORT:ilium-server/tests/crashed_agent_recovery.rs::crashed_codex_retains_identity_session_and_exact_unflushed_prompt
#[test]
fn port_crashed_codex_retains_identity_session_and_exact_unflushed_prompt() {
    crashed_then_restored("codex", codex_template());
}

/// REQ: PORT:ilium-server/tests/crashed_agent_recovery.rs::crashed_claude_retains_identity_session_and_exact_unflushed_prompt
#[test]
fn port_crashed_claude_retains_identity_session_and_exact_unflushed_prompt() {
    crashed_then_restored("claude", claude_template());
}

/// REQ: PORT:ilium-server/tests/crashed_agent_recovery.rs::nested_codex_crash_preserves_prompt_and_fences_automation_from_shell, PORT:ilium-server/tests/crashed_agent_recovery.rs::codex_interpreted_launcher_loss_retains_native_recovery
#[test]
fn port_nested_codex_crash_preserves_prompt_and_fences_automation_from_shell() {
    // The shell owns the terminal: the resume addresses the pane's command, never typed input.
    crashed_then_restored("codex", codex_template());
    let mut host = activated_server();
    let shell = add_shell(&mut host, "nested shell");
    assert!(restore_panes(&mut host, &[shell])
        .iter()
        .all(|s| !matches!(s.intent, Intent::ReplacePaneCommand(_))));
}

/// REQ: PORT:ilium-server/tests/crashed_agent_recovery.rs::nested_claude_crash_preserves_prompt_and_fences_automation_from_shell, PORT:ilium-server/tests/crashed_agent_recovery.rs::claude_interpreted_launcher_loss_retains_native_recovery
#[test]
fn port_nested_claude_crash_preserves_prompt_and_fences_automation_from_shell() {
    crashed_then_restored("claude", claude_template());
    let mut host = activated_server();
    let shell = add_shell(&mut host, "nested shell");
    assert!(restore_panes(&mut host, &[shell])
        .iter()
        .all(|s| !matches!(s.intent, Intent::ReplacePaneCommand(_))));
}

/// REQ: PORT:ilium-server/tests/crashed_agent_recovery.rs::in_flight_automated_receipt_cancels_without_enter_or_replay_after_provider_loss
#[test]
fn port_in_flight_automated_receipt_cancels_without_enter_or_replay_after_provider_loss() {
    // After provider loss with an unverified session the plugin submits nothing: no Enter, no replay.
    let mut host = activated_server();
    let pane = add_agent(&mut host, "stalled", "codex");
    register_signature(&mut host, signature("codex", Some(codex_template())));
    let submitted = restore_panes(&mut host, &[pane]);
    assert!(replace_commands(&submitted).is_empty());
    // A second delivery of the same restore does not replay anything either.
    let again = restore_panes(&mut host, &[pane]);
    assert!(replace_commands(&again).is_empty());
}

/// REQ: PORT:ilium-server/tests/crashed_agent_recovery.rs::codex_interpreted_launcher_loss_retains_native_recovery
#[test]
fn port_a_verified_codex_pane_is_frozen_while_its_resume_runs() {
    let mut host = activated_server();
    let pane = add_agent(&mut host, "launcher lost", "codex");
    verify_agent(
        &mut host,
        pane,
        signature("codex", Some(codex_template())),
        SESSION_ID,
    );
    let frozen = freezes(&restore_panes(&mut host, &[pane]));
    assert_eq!(frozen.len(), 1);
    assert_eq!(frozen[0].0.pane.0, pane);
}

/// REQ: PORT:ilium-server/tests/crashed_agent_recovery.rs::claude_interpreted_launcher_loss_retains_native_recovery
#[test]
fn port_a_verified_claude_pane_is_frozen_while_its_resume_runs() {
    let mut host = activated_server();
    let pane = add_agent(&mut host, "launcher lost", "claude");
    verify_agent(
        &mut host,
        pane,
        signature("claude", Some(claude_template())),
        SESSION_ID,
    );
    let frozen = freezes(&restore_panes(&mut host, &[pane]));
    assert_eq!(frozen.len(), 1);
    assert_eq!(frozen[0].0.pane.0, pane);
}

/// REQ: PORT:ilium-server/tests/crashed_agent_recovery.rs::crashed_codex_retains_identity_session_and_exact_unflushed_prompt
#[test]
fn port_a_crashed_codex_pane_without_a_verified_session_is_not_resumed() {
    let mut host = activated_server();
    let pane = add_agent(&mut host, "crashed", "codex");
    register_signature(&mut host, signature("codex", Some(codex_template())));
    assert!(replace_commands(&restore_panes(&mut host, &[pane])).is_empty());
}

/// REQ: PORT:ilium-server/tests/crashed_agent_recovery.rs::crashed_claude_retains_identity_session_and_exact_unflushed_prompt
#[test]
fn port_a_crashed_claude_pane_without_a_verified_session_is_not_resumed() {
    let mut host = activated_server();
    let pane = add_agent(&mut host, "crashed", "claude");
    register_signature(&mut host, signature("claude", Some(claude_template())));
    assert!(replace_commands(&restore_panes(&mut host, &[pane])).is_empty());
}

/// REQ: PORT:ilium-server/tests/crashed_agent_recovery.rs::in_flight_automated_receipt_cancels_without_enter_or_replay_after_provider_loss
#[test]
fn port_an_unverified_pane_beside_a_verified_one_gets_no_input() {
    let mut host = activated_server();
    let verified = add_agent(&mut host, "verified", "codex");
    verify_agent(
        &mut host,
        verified,
        signature("codex", Some(codex_template())),
        SESSION_ID,
    );
    let stalled = add_agent(&mut host, "stalled", "codex");
    let submitted = restore_panes(&mut host, &[verified, stalled]);
    let commands = replace_commands(&submitted);
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].0.pane.0, verified);
    assert!(submitted.iter().all(|s| matches!(
        s.intent,
        Intent::ReplacePaneCommand(_) | Intent::FreezePane(_)
    )));
}

/// REQ: PORT:ilium-server/tests/crashed_agent_recovery.rs::nested_codex_crash_preserves_prompt_and_fences_automation_from_shell
#[test]
fn port_a_shell_restored_beside_a_resumed_codex_pane_gets_no_resume() {
    let mut host = activated_server();
    let agent = add_agent(&mut host, "agent", "codex");
    verify_agent(
        &mut host,
        agent,
        signature("codex", Some(codex_template())),
        SESSION_ID,
    );
    let shell = add_shell(&mut host, "nested shell");
    let commands = replace_commands(&restore_panes(&mut host, &[agent, shell]));
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].0.pane.0, agent);
}

/// REQ: PORT:ilium-server/tests/crashed_agent_recovery.rs::nested_claude_crash_preserves_prompt_and_fences_automation_from_shell
#[test]
fn port_a_shell_restored_beside_a_resumed_claude_pane_gets_no_resume() {
    let mut host = activated_server();
    let agent = add_agent(&mut host, "agent", "claude");
    verify_agent(
        &mut host,
        agent,
        signature("claude", Some(claude_template())),
        SESSION_ID,
    );
    let shell = add_shell(&mut host, "nested shell");
    let commands = replace_commands(&restore_panes(&mut host, &[agent, shell]));
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].0.pane.0, agent);
}

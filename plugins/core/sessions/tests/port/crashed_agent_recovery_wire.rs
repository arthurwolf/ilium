//! Twin of ilium-ipc/tests/crashed_agent_recovery_wire.rs: the identity, the session and the exact
//! prompt of an unavailable agent survive; the plugin observes them only through the verified
//! session reference and must not alter or replay the prompt.

use ilium_plugin_sdk::types::Intent;

use crate::common::*;

/// REQ: PORT:ilium-ipc/tests/crashed_agent_recovery_wire.rs::unavailable_agent_wire_preserves_identity_cause_and_exact_prompt_uncertainty
#[test]
fn port_unavailable_agent_wire_preserves_identity_cause_and_exact_prompt_uncertainty() {
    for (name, template) in [("codex", codex_template()), ("claude", claude_template())] {
        let mut host = activated_server();
        let pane = add_agent(&mut host, "stopped agent", name);
        verify_agent(
            &mut host,
            pane,
            signature(name, Some(template.clone())),
            SESSION_ID,
        );
        let submitted = restore_panes(&mut host, &[pane]);
        // The verified session id is carried exactly; no other intent (no prompt text) is submitted.
        let commands = replace_commands(&submitted);
        assert_eq!(commands.len(), 1, "{name}");
        let expected: Vec<String> = template
            .iter()
            .map(|part| part.replace("{session}", SESSION_ID))
            .collect();
        assert_eq!(commands[0].0.command, expected, "{name}");
        assert!(
            submitted.iter().all(|s| matches!(
                s.intent,
                Intent::ReplacePaneCommand(_) | Intent::FreezePane(_)
            )),
            "{name}"
        );
    }
}

/// REQ: PORT:ilium-ipc/tests/crashed_agent_recovery_wire.rs::unavailable_agent_wire_preserves_identity_cause_and_exact_prompt_uncertainty
#[test]
fn port_the_resume_command_carries_the_session_and_no_unexpanded_token() {
    for (name, template) in [("codex", codex_template()), ("claude", claude_template())] {
        let mut host = activated_server();
        let pane = add_agent(&mut host, "stopped agent", name);
        verify_agent(
            &mut host,
            pane,
            signature(name, Some(template)),
            SESSION_ID,
        );
        let submitted = restore_panes(&mut host, &[pane]);
        let commands = replace_commands(&submitted);
        assert_eq!(commands.len(), 1, "{name}");
        let parts = &commands[0].0.command;
        assert!(parts.iter().any(|part| part.contains(SESSION_ID)), "{name}");
        assert!(parts.iter().all(|part| !part.contains("{session}")), "{name}");
    }
}

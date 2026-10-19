//! Twins of ilium-core/src/agent_recovery.rs: a historical agent identity (a pane whose agent is
//! gone and whose shell owns the terminal) must never be treated as a live, addressable agent.
//! In the plugin world the observable consequence is what the Sessions plugin submits for it.

use crate::common::*;

/// REQ: PORT:ilium-core/src/agent_recovery.rs::historical_working_identity_does_not_authorize_a_live_agent
#[test]
fn port_historical_working_identity_does_not_authorize_a_live_agent() {
    // The pane is a shell again (no live agent view), its former agent is only a historical record.
    let mut host = activated_server();
    let pane = add_shell(&mut host, "former agent");
    register_signature(&mut host, signature("codex", Some(codex_template())));
    let submitted = restore_panes(&mut host, &[pane]);
    // No agent-directed automation: nothing is resumed in a pane that has no live verified agent.
    assert!(replace_commands(&submitted).is_empty());
}

/// REQ: PORT:ilium-core/src/agent_recovery.rs::unavailable_projection_does_not_present_a_historical_goal_or_turn_as_live
#[test]
fn port_unavailable_projection_does_not_present_a_historical_goal_or_turn_as_live() {
    // A pane whose agent is unavailable has no verified session reference: it is never resumed,
    // whichever signature it historically had.
    for (name, template) in [("codex", codex_template()), ("claude", claude_template())] {
        let mut host = activated_server();
        let pane = add_agent(&mut host, "unavailable", name);
        register_signature(&mut host, signature(name, Some(template)));
        assert!(
            replace_commands(&restore_panes(&mut host, &[pane])).is_empty(),
            "{name}"
        );
    }
}

/// REQ: PORT:ilium-core/src/agent_recovery.rs::exit_labels_distinguish_observed_status_from_unknown_cause
#[test]
fn port_exit_labels_distinguish_observed_status_from_unknown_cause() {
    // The labels stay kernel text; the plugin must not infer a crash from a missing session: an
    // unverified session yields no submission at all, and a verified one yields exactly one resume.
    let mut unverified = activated_server();
    let pane = add_agent(&mut unverified, "a", "codex");
    register_signature(&mut unverified, signature("codex", Some(codex_template())));
    assert!(restore_panes(&mut unverified, &[pane]).is_empty());

    let mut verified = activated_server();
    let pane = add_agent(&mut verified, "a", "codex");
    verify_agent(
        &mut verified,
        pane,
        signature("codex", Some(codex_template())),
        SESSION_ID,
    );
    assert_eq!(
        replace_commands(&restore_panes(&mut verified, &[pane])).len(),
        1
    );
}

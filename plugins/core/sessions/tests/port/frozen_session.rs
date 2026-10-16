//! Twins of ilium-server/tests/frozen_session.rs: a frozen pane keeps its saved provider session and
//! its worktree ownership, and thaws only when every restored pane runs.

use ilium_plugin_sdk::types::{NodeId, PaneLifecycleState};

use crate::common::*;

/// REQ: PORT:ilium-server/tests/frozen_session.rs::unfreeze_restarts_the_saved_provider_session_and_recovers_its_identity
#[test]
fn port_unfreeze_restarts_the_saved_provider_session_and_recovers_its_identity() {
    let mut host = activated_server();
    let pane = add_agent(&mut host, "claude", "claude");
    verify_agent(&mut host, pane, signature("claude", Some(claude_template())), SESSION_ID);
    let submitted = restore_panes(&mut host, &[pane]);
    let frozen = freezes(&submitted);
    assert_eq!(frozen.len(), 1);
    assert_eq!(frozen[0].0.pane, NodeId(pane));
    let commands = replace_commands(&submitted);
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].0.command, ["claude", "--resume", SESSION_ID]);

    let thaw = host.deliver(vec![lifecycle_event(pane, PaneLifecycleState::Running)]);
    let thawed = unfreezes(&thaw);
    assert_eq!(thawed.len(), 1);
    assert_eq!(thawed[0].0.pane, NodeId(pane));
    assert_eq!(thawed[0].1, Some(format!("thaw-{pane}")));
}

/// REQ: PORT:ilium-server/tests/frozen_session.rs::unfreeze_preserves_worktree_ownership
#[test]
fn port_unfreeze_preserves_worktree_ownership() {
    // Ownership stays with the pane id: the resume names the same pane and sets no cwd, so the
    // replacement is not re-parented, and the thaw addresses the same pane.
    let mut host = activated_server();
    let pane = add_agent(&mut host, "worktree agent", "claude");
    verify_agent(&mut host, pane, signature("claude", Some(claude_template())), SESSION_ID);
    let other = add_shell(&mut host, "workspace");
    let submitted = restore_panes(&mut host, &[pane, other]);
    let commands = replace_commands(&submitted);
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].0.pane, NodeId(pane));
    assert!(commands[0].0.cwd.is_none());
    assert!(commands[0].0.keep_scrollback);
    // Thaw only after both panes run.
    assert!(unfreezes(&host.deliver(vec![lifecycle_event(pane, PaneLifecycleState::Running)])).is_empty());
    let thaw = unfreezes(&host.deliver(vec![lifecycle_event(other, PaneLifecycleState::Running)]));
    assert_eq!(thaw.len(), 2);
}

//! Twins of ilium-server/tests/frozen_session.rs: a frozen pane keeps its saved provider session and
//! its worktree ownership, and thaws only when every restored pane runs.

use ilium_plugin_sdk::types::{Event, NodeId, PaneLifecycleState};

use crate::common::*;

/// REQ: PORT:ilium-server/tests/frozen_session.rs::unfreeze_restarts_the_saved_provider_session_and_recovers_its_identity
#[test]
fn port_unfreeze_restarts_the_saved_provider_session_and_recovers_its_identity() {
    let mut host = activated_server();
    let pane = add_agent(&mut host, "claude", "claude");
    verify_agent(
        &mut host,
        pane,
        signature("claude", Some(claude_template())),
        SESSION_ID,
    );
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
    verify_agent(
        &mut host,
        pane,
        signature("claude", Some(claude_template())),
        SESSION_ID,
    );
    let other = add_shell(&mut host, "workspace");
    let submitted = restore_panes(&mut host, &[pane, other]);
    let commands = replace_commands(&submitted);
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].0.pane, NodeId(pane));
    assert!(commands[0].0.cwd.is_none());
    assert!(commands[0].0.keep_scrollback);
    // Thaw only after both panes run.
    assert!(
        unfreezes(&host.deliver(vec![lifecycle_event(pane, PaneLifecycleState::Running)]))
            .is_empty()
    );
    let thaw = unfreezes(&host.deliver(vec![lifecycle_event(other, PaneLifecycleState::Running)]));
    assert_eq!(thaw.len(), 2);
}

/// REQ: PORT:ilium-server/tests/frozen_session.rs::unfreeze_restarts_the_saved_provider_session_and_recovers_its_identity
#[test]
fn port_a_frozen_pane_that_exits_instead_of_running_is_not_thawed() {
    let mut host = activated_server();
    let pane = add_agent(&mut host, "claude", "claude");
    verify_agent(
        &mut host,
        pane,
        signature("claude", Some(claude_template())),
        SESSION_ID,
    );
    restore_panes(&mut host, &[pane]);
    let thaw = unfreezes(&host.deliver(vec![lifecycle_event(pane, PaneLifecycleState::Exited)]));
    assert!(thaw.is_empty());
}

#[test]
fn an_exited_pane_does_not_leave_other_restored_panes_frozen() {
    let mut host = activated_server();
    let exited = add_shell(&mut host, "exited");
    let running = add_shell(&mut host, "running");
    restore_panes(&mut host, &[exited, running]);
    assert!(unfreezes(&host.deliver(vec![lifecycle_event(
        exited,
        PaneLifecycleState::Exited,
    )]))
    .is_empty());
    let submissions = host.deliver(vec![lifecycle_event(running, PaneLifecycleState::Running)]);
    let thawed = unfreezes(&submissions);
    assert_eq!(thawed.len(), 1);
    assert_eq!(thawed[0].0.pane, NodeId(running));
}

#[test]
fn removing_a_restored_pane_does_not_leave_the_others_frozen() {
    let mut host = activated_server();
    let removed = add_shell(&mut host, "removed");
    let running = add_shell(&mut host, "running");
    restore_panes(&mut host, &[removed, running]);
    host.deliver(vec![Event::NodeRemoved(NodeId(removed))]);
    let submissions = host.deliver(vec![lifecycle_event(running, PaneLifecycleState::Running)]);
    let thawed = unfreezes(&submissions);
    assert_eq!(thawed.len(), 1);
    assert_eq!(thawed[0].0.pane, NodeId(running));
}

#[test]
fn a_stalled_restored_pane_is_released_after_the_timeout() {
    let mut host = activated_server();
    let pane = add_shell(&mut host, "stalled");
    restore_panes(&mut host, &[pane]);
    host.active_timers().into_iter().find_map(|(id, repeat)| {
        repeat.is_none().then_some(id)
    }).expect("a restored pane must have a timeout");
    let before = host.submitted().len();
    host.advance_clock(30_000);
    let submissions = host.submitted()[before..].to_vec();
    let thawed = unfreezes(&submissions);
    assert_eq!(thawed.len(), 1);
    assert_eq!(thawed[0].0.pane, NodeId(pane));
}

#[test]
fn a_stalled_pane_timeout_does_not_release_another_waiting_pane() {
    let mut host = activated_server();
    let first = add_shell(&mut host, "first");
    let second = add_shell(&mut host, "second");
    restore_panes(&mut host, &[first]);
    assert!(!host.active_timers().is_empty(), "first timer");
    host.advance_clock(10_000);
    restore_panes(&mut host, &[second]);
    let before = host.submitted().len();
    host.advance_clock(20_000);
    let expired = host.submitted()[before..].to_vec();
    let thawed = unfreezes(&expired);
    assert_eq!(thawed.len(), 1);
    assert_eq!(thawed[0].0.pane, NodeId(first));
    let ready = host.deliver(vec![lifecycle_event(second, PaneLifecycleState::Running)]);
    let thawed = unfreezes(&ready);
    assert_eq!(thawed.len(), 1);
    assert_eq!(thawed[0].0.pane, NodeId(second));
}

#[test]
fn a_large_restore_uses_bounded_timers_and_freezes_every_pane() {
    let mut host = activated_server();
    let panes: Vec<_> = (0..100).map(|index| add_shell(&mut host, &format!("pane {index}"))).collect();
    let submissions = restore_panes(&mut host, &panes);
    assert_eq!(freezes(&submissions).len(), 100);
    assert!(host.active_timers().len() <= 1, "restore must not consume one timer per pane");
}

/// REQ: PORT:ilium-server/tests/frozen_session.rs::unfreeze_preserves_worktree_ownership
#[test]
fn port_a_repeated_running_report_does_not_thaw_the_panes_twice() {
    let mut host = activated_server();
    let pane = add_agent(&mut host, "worktree agent", "claude");
    verify_agent(
        &mut host,
        pane,
        signature("claude", Some(claude_template())),
        SESSION_ID,
    );
    let other = add_shell(&mut host, "workspace");
    restore_panes(&mut host, &[pane, other]);
    let first = unfreezes(&host.deliver(vec![lifecycle_event(other, PaneLifecycleState::Running)]));
    assert!(first.is_empty());
    let thaw = unfreezes(&host.deliver(vec![lifecycle_event(pane, PaneLifecycleState::Running)]));
    assert_eq!(thaw.len(), 2);
    let again = unfreezes(&host.deliver(vec![lifecycle_event(pane, PaneLifecycleState::Running)]));
    assert!(again.is_empty());
}

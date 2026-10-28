//! Twins of the integration tests under `ilium-server/tests` and `ilium/tests`.

use ilium_plugin_sdk::types::{AgentActivity, NodeId};

use crate::common::*;

/// REQ: PORT:ilium-server/tests/smoke.rs::new_pane_creates_a_shell_and_broadcasts_a_tree_snapshot_containing_it
#[test]
fn smoke_new_pane_creates_one_terminal_pane() {
    let mut host = server_host();
    add_session(&mut host, "default", true, &[]);
    let (_, _, exit) = run_cli(&mut host, &["new-pane", "--", "claude"], b"");
    assert_eq!(exit, Some(0));
    let specs = created_panes(&host);
    assert_eq!(specs.len(), 1);
    assert_eq!(specs[0].content_kind, "ilium.terminal");
}

/// REQ: PORT:ilium/tests/broadcast_e2e.rs::broadcast_reaches_selected_agents_across_projects_and_never_shells
#[test]
fn broadcast_e2e_reaches_selected_agents_and_never_shells() {
    let mut host = server_host();
    let claude = add_agent(&mut host, "claude agent", "claude", AgentActivity::Idle);
    let codex = add_agent(&mut host, "codex agent", "codex", AgentActivity::Idle);
    let shell = add_shell(&mut host, "shell");
    add_session(&mut host, "default", true, &[]);
    add_session(&mut host, "review", false, &[]);
    let (_, _, exit) = run_cli(&mut host, &["broadcast", "--agent", "claude", "hello"], b"");
    assert_eq!(exit, Some(0));
    assert_eq!(sent_panes(&host), [NodeId(claude)]);
    assert!(
        !sent_panes(&host).contains(&NodeId(codex)) && !sent_panes(&host).contains(&NodeId(shell))
    );
    let (_, _, _) = run_cli(&mut host, &["broadcast", "all"], b"");
    assert!(!sent_panes(&host).contains(&NodeId(shell)));
}

/// REQ: PORT:ilium-server/tests/smoke.rs::new_pane_creates_a_shell_and_broadcasts_a_tree_snapshot_containing_it
#[test]
fn smoke_new_pane_without_a_command_creates_a_shell_pane() {
    let mut host = server_host();
    add_session(&mut host, "default", true, &[]);
    let (_, _, exit) = run_cli(&mut host, &["new-pane"], b"");
    assert_eq!(exit, Some(0));
    let specs = created_panes(&host);
    assert_eq!(specs.len(), 1);
    assert!(specs[0].command.is_none());
}

/// REQ: PORT:ilium/tests/broadcast_e2e.rs::broadcast_reaches_selected_agents_across_projects_and_never_shells
#[test]
fn broadcast_e2e_codex_selection_reaches_only_the_codex_agent() {
    let mut host = server_host();
    let claude = add_agent(&mut host, "claude agent", "claude", AgentActivity::Idle);
    let codex = add_agent(&mut host, "codex agent", "codex", AgentActivity::Idle);
    add_shell(&mut host, "shell");
    add_session(&mut host, "default", true, &[]);
    let (_, _, exit) = run_cli(&mut host, &["broadcast", "--agent", "codex", "hello"], b"");
    assert_eq!(exit, Some(0));
    assert_eq!(sent_panes(&host), [NodeId(codex)]);
    assert!(!sent_panes(&host).contains(&NodeId(claude)));
}

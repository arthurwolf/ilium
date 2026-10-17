//! F-068 Start an agent in an existing worktree (src/docs/worktrees.md "Start an agent in an existing
//! worktree"). The wire shape of the request is not in the manual page: see API_GAPS.md.

use ilium_plugin_sdk::testing::FakeHost;
use serde_json::json;

use crate::common::*;

const WORKTREE: &str = "docs-sweep";

fn host_with_worktree() -> FakeHost {
    let mut host = server_host();
    host.workspace().add_foreign(WORKTREE, "docs-sweep");
    host
}

fn start_existing(host: &mut FakeHost) -> Vec<ilium_plugin_sdk::types::Submission> {
    host.deliver_one(new_agent_message(&json!({"worktree": WORKTREE})));
    host.settle();
    host.submitted()
}

/// REQ: F-068
#[test]
fn feature_068_an_agent_starts_in_the_listed_worktree_without_running_git_worktree_add() {
    let mut host = host_with_worktree();
    let submitted = start_existing(&mut host);
    assert!(worktree_add_calls(&host.process().calls()).is_empty());
    let panes = create_panes(&submitted);
    assert_eq!(panes.len(), 1);
    assert!(cwd_text(&panes[0]).contains(WORKTREE));
}

/// REQ: F-068
#[test]
fn feature_068_the_agent_command_is_claude_on_whatever_branch_the_worktree_has() {
    let mut host = host_with_worktree();
    let submitted = start_existing(&mut host);
    let panes = create_panes(&submitted);
    assert_eq!(panes[0].content_kind, "ilium.terminal");
    assert_eq!(panes[0].command, Some(vec!["claude".to_string()]));
}

/// REQ: F-068
#[test]
fn feature_068_using_an_existing_worktree_never_makes_the_plugin_its_owner() {
    let mut host = host_with_worktree();
    start_existing(&mut host);
    host.deliver_one(pane_created(21, "docs-sweep"));
    host.settle();
    assert!(registered(&mut host).is_empty());
}

/// REQ: F-068
#[test]
fn feature_068_a_worktree_that_is_not_registered_starts_nothing() {
    let mut host = server_host();
    let submitted = start_existing(&mut host);
    assert!(create_panes(&submitted).is_empty());
}

/// REQ: F-068
#[test]
fn feature_068_a_worktree_with_a_live_agent_cannot_be_selected_again() {
    let mut host = host_with_worktree();
    start_existing(&mut host);
    host.deliver_one(pane_created(21, "docs-sweep"));
    host.settle();
    let submitted = start_existing(&mut host);
    assert_eq!(create_panes(&submitted).len(), 1, "no second pane");
}

/// REQ: F-068
#[test]
fn feature_068_two_requests_at_once_admit_one_agent() {
    let mut host = host_with_worktree();
    host.deliver_one(new_agent_message(&json!({"worktree": WORKTREE})));
    host.deliver_one(new_agent_message(&json!({"worktree": WORKTREE})));
    host.settle();
    assert_eq!(create_panes(&host.submitted()).len(), 1);
}

/// REQ: F-068
#[test]
fn feature_068_the_worktree_is_selectable_again_after_its_agent_pane_closed() {
    let mut host = host_with_worktree();
    start_existing(&mut host);
    host.deliver_one(pane_created(21, "docs-sweep"));
    host.deliver_one(ilium_plugin_sdk::types::Event::NodeRemoved(
        ilium_plugin_sdk::types::NodeId(21),
    ));
    host.settle();
    let submitted = start_existing(&mut host);
    assert_eq!(create_panes(&submitted).len(), 2);
}

//! Twin of ilium/tests/worktree_cli.rs: what the `new-pane --worktree` request does once the
//! plugin owns worktree creation (the service `create`, the same server path as the command).

use serde_json::json;

use crate::common::*;
use crate::support::*;

const PROJECT: &str = "/tmp/isolated/project";
const BRANCH: &str = "agent/cli-test";

fn host_with_setup() -> ilium_plugin_sdk::testing::FakeHost {
    let mut host = server_host_at(PROJECT);
    set_git(&mut host, "setup_command", json!("test -r .local.env && printf setup-ready > setup.done")).expect("valid");
    host
}

/// REQ: F-067, F-071, PORT:ilium/tests/worktree_cli.rs::new_pane_worktree_cli_creates_owned_workspace_and_rejects_collision
#[test]
fn port_067_the_request_creates_the_worktree_beside_the_project_on_the_requested_branch() {
    let mut host = host_with_setup();
    create_worktree(&mut host, &request(BRANCH));
    let arguments = add_arguments(&mut host);
    assert_eq!(&arguments[..4], ["worktree", "add", "-b", BRANCH]);
    assert_eq!(arguments[4], "/tmp/isolated/project.worktrees/agent-cli-test");
}

/// REQ: F-067, PORT:ilium/tests/worktree_cli.rs::new_pane_worktree_cli_creates_owned_workspace_and_rejects_collision
#[test]
fn port_067_the_agent_pane_starts_in_the_new_worktree() {
    let mut host = host_with_setup();
    let submitted = create_worktree(&mut host, &request(BRANCH));
    let panes = create_panes(&submitted);
    assert_eq!(panes.len(), 1);
    assert!(cwd_text(&panes[0]).contains("/tmp/isolated/project.worktrees/agent-cli-test"));
}

/// REQ: F-067, PORT:ilium/tests/worktree_cli.rs::new_pane_worktree_cli_creates_owned_workspace_and_rejects_collision
#[test]
fn port_067_the_pane_owns_the_registered_worktree_with_its_branch() {
    let mut host = host_with_setup();
    create_worktree(&mut host, &request(BRANCH));
    host.deliver_one(pane_created(30, BRANCH));
    host.settle();
    let registered = registered(&mut host);
    assert_eq!(registered.len(), 1);
    assert_eq!(registered[0].branch, BRANCH);
    assert_eq!(registered[0].owner, Some(ilium_plugin_sdk::types::NodeId(30)));
}

/// REQ: F-071, PORT:ilium/tests/worktree_cli.rs::new_pane_worktree_cli_creates_owned_workspace_and_rejects_collision
#[test]
fn port_071_the_setup_runs_in_the_worktree_after_git_and_before_the_agent_pane() {
    let mut host = host_with_setup();
    create_worktree(&mut host, &request(BRANCH));
    let calls = host.process().calls();
    let shell = calls.iter().position(|call| call.program == "sh").expect("setup ran");
    let add = calls.iter().position(|call| call.program == "git" && call.arguments.get(1).map(String::as_str) == Some("add")).expect("git add ran");
    assert!(add < shell);
    assert_eq!(calls[shell].arguments, ["-c", "test -r .local.env && printf setup-ready > setup.done"]);
    assert!(format!("{:?}", calls[shell].cwd).contains("agent-cli-test"));
    assert!(host.order_of_calls_and_submissions().process_before_submission("sh", "create-pane"));
}

/// REQ: F-067, PORT:ilium/tests/worktree_cli.rs::new_pane_worktree_cli_creates_owned_workspace_and_rejects_collision
#[test]
fn port_067_a_second_request_for_the_same_branch_is_refused_and_creates_no_second_worktree() {
    let mut host = host_with_setup();
    create_worktree(&mut host, &request(BRANCH));
    host.deliver_one(pane_created(30, BRANCH));
    host.settle();
    host.process().on_args(&["branch", "--format=%(refname:short)"], stdout_exit(&format!("main\n{BRANCH}\n")));
    let submitted = create_worktree(&mut host, &request(BRANCH));
    assert_eq!(worktree_add_calls(&host.process().calls()).len(), 1);
    assert_eq!(create_panes(&submitted).len(), 1);
    assert_eq!(registered(&mut host).len(), 1);
}

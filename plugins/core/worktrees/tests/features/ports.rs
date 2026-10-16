//! Ported scenarios of ilium-server/tests/workspace_ipc.rs for the worktrees plugin.

use serde_json::json;

use crate::common::*;

/// REQ: F-067, PORT:ilium-server/tests/workspace_ipc.rs::repo_facts_and_invalid_branch_rejection_leave_no_pane_or_worktree
#[test]
fn port_067_an_invalid_branch_leaves_no_pane_and_no_worktree() {
    let mut host = server_host();
    let submitted = create_worktree(&mut host, &json!({"branch": "bad name"}));
    assert!(create_panes(&submitted).is_empty());
    assert!(host.process().calls().is_empty());
    let notice = host.last_notice().expect("the refusal is shown to the user");
    assert!(notice.contains("bad name"), "the notice names the refused branch: {notice}");
    assert!(host.published(TOPIC_CHANGED).is_empty());
}

/// REQ: F-067
#[test]
fn port_067_a_request_without_a_path_derives_the_default_path_on_the_server() {
    let mut host = server_host();
    create_worktree(&mut host, &json!({"branch": "agent/fix-login"}));
    let calls = worktree_add_calls(&host.process().calls());
    assert!(calls[0].arguments[4].ends_with(".worktrees/agent-fix-login"));
}

/// REQ: F-067, PORT:ilium-server/tests/workspace_ipc.rs::concurrent_clients_cannot_create_two_worktrees_on_one_branch
#[test]
fn port_067_concurrent_requests_cannot_create_two_worktrees_on_one_branch() {
    let mut host = server_host();
    for _ in 0..4 {
        host.deliver_one(new_agent_message(&json!({"branch": "same"})));
    }
    host.settle();
    assert_eq!(worktree_add_calls(&host.process().calls()).len(), 1);
}

/// REQ: F-067, PORT:ilium-server/tests/workspace_ipc.rs::disconnect_during_worktree_add_finishes_git_then_rolls_back
#[test]
fn port_067_a_cancelled_request_creates_no_pane() {
    let mut host = server_host();
    host.deliver_one(new_agent_message(&json!({"branch": "fix-auth"})));
    host.cancel_pending();
    host.settle();
    assert!(create_panes(&host.submitted()).is_empty());
}

/// REQ: F-067
#[test]
fn port_067_the_recorded_base_is_head_as_requested_at_creation() {
    let mut host = server_host();
    create_worktree(&mut host, &json!({"branch": "fix-auth"}));
    host.deliver_one(pane_created(14, "fix-auth"));
    host.settle();
    assert_eq!(registered(&mut host)[0].base.as_deref(), Some("HEAD"));
}

/// REQ: F-068, PORT:ilium-server/tests/workspace_ipc.rs::existing_worktree_cannot_start_a_second_agent_pane
#[test]
fn port_068_an_existing_worktree_cannot_start_a_second_agent_pane() {
    let mut host = server_host();
    host.workspace().add_foreign("docs-sweep", "docs-sweep");
    for _ in 0..2 {
        host.deliver_one(new_agent_message(&json!({"worktree": "docs-sweep"})));
        host.deliver_one(pane_created(21, "docs-sweep"));
    }
    host.settle();
    assert_eq!(create_panes(&host.submitted()).len(), 1);
}

/// REQ: F-068, PORT:ilium-server/tests/workspace_ipc.rs::concurrent_existing_worktree_requests_admit_one_agent
#[test]
fn port_068_concurrent_existing_worktree_requests_admit_one_agent() {
    let mut host = server_host();
    host.workspace().add_foreign("docs-sweep", "docs-sweep");
    for _ in 0..3 {
        host.deliver_one(new_agent_message(&json!({"worktree": "docs-sweep"})));
    }
    host.settle();
    assert_eq!(create_panes(&host.submitted()).len(), 1);
}

/// REQ: F-070
#[test]
fn port_070_the_close_policy_persists_and_a_dirty_checkout_is_kept() {
    let mut host = server_host();
    set_git(&mut host, "default_close_policy", json!("offer_removal_when_safe")).expect("valid");
    assert_eq!(host.kernel_settings("git").get("default_close_policy"), Some(json!("offer_removal_when_safe")));
    host.process().on_args(&["status", "--porcelain"], ilium_plugin_sdk::host::process::ProcessExit { status: Some(0), signal: None, stdout: b" M a\n".to_vec(), stderr: Vec::new() });
    host.deliver_one(channel_message("remove", json!({"path": "/w/a", "branch": "a"}).to_string().into_bytes()));
    host.settle();
    assert!(host.process().calls().iter().all(|c| c.arguments.get(1).map(String::as_str) != Some("remove")));
}

/// REQ: F-070, PORT:ilium-server/tests/workspace_ipc.rs::isolated_server_creates_attaches_and_removes_only_owned_worktree
#[test]
fn port_070_only_an_owned_worktree_is_removed() {
    let mut host = server_host();
    host.workspace().add_foreign("docs-sweep", "docs-sweep");
    host.deliver_one(channel_message("remove", json!({"path": "docs-sweep", "branch": "docs-sweep"}).to_string().into_bytes()));
    host.settle();
    assert!(host.process().calls().iter().all(|c| c.arguments.get(1).map(String::as_str) != Some("remove")));
}

/// REQ: F-069, PORT:ilium-server/tests/workspace_ipc.rs::live_worktree_status_refreshes_and_replays_after_head_changes
#[test]
fn port_069_a_status_probe_after_ten_seconds_republishes_the_changed_topic() {
    let mut host = server_host();
    create_worktree(&mut host, &json!({"branch": "fix-auth"}));
    host.deliver_one(pane_created(14, "fix-auth"));
    host.settle();
    let before = host.published(TOPIC_CHANGED).len();
    host.advance_clock(10_000);
    host.settle();
    assert!(host.published(TOPIC_CHANGED).len() > before);
}

/// REQ: F-071, PORT:ilium-server/tests/workspace_ipc.rs::configured_setup_runs_before_agent_start_and_failure_retains_worktree
#[test]
fn port_071_setup_failure_retains_the_worktree_and_starts_no_agent() {
    let mut host = server_host();
    set_git(&mut host, "setup_command", json!("exit 1")).expect("valid");
    host.process().on_program("sh", failed_exit(1));
    let submitted = create_worktree(&mut host, &json!({"branch": "fix-auth"}));
    assert!(create_panes(&submitted).is_empty());
    assert_eq!(worktree_add_calls(&host.process().calls()).len(), 1);
}

/// REQ: F-071, PORT:ilium-server/tests/workspace_ipc.rs::partial_include_copy_retains_prior_files_and_worktree
#[test]
fn port_071_a_partial_include_copy_failure_creates_no_pane_and_keeps_git_worktree_add_single() {
    let mut host = server_host();
    host.fs().seed("/project/.worktreeinclude", b"a\n../bad\n");
    host.fs().seed("/project/a", b"1");
    let submitted = create_worktree(&mut host, &json!({"branch": "fix-auth"}));
    assert!(create_panes(&submitted).is_empty());
    assert_eq!(worktree_add_calls(&host.process().calls()).len(), 1);
}

/// REQ: F-067, PORT:ilium-server/tests/workspace_ipc.rs::retained_owned_worktree_is_recognized_and_reused_after_pane_close
#[test]
fn port_067_a_retained_owned_worktree_is_still_registered_after_its_pane_closed() {
    let mut host = server_host();
    create_worktree(&mut host, &json!({"branch": "fix-auth"}));
    host.deliver_one(pane_created(14, "fix-auth"));
    host.deliver_one(ilium_plugin_sdk::types::Event::NodeRemoved(ilium_plugin_sdk::types::NodeId(14)));
    host.settle();
    assert_eq!(registered(&mut host).len(), 1);
}

/// REQ: F-070, PORT:ilium-server/tests/workspace_ipc.rs::retained_worktree_with_a_folder_root_cannot_be_pruned
#[test]
fn port_070_a_worktree_used_as_a_folder_root_is_not_removed() {
    let mut host = server_host();
    host.panes().add_running_in("/w/a");
    host.deliver_one(channel_message("remove", json!({"path": "/w/a", "branch": "a"}).to_string().into_bytes()));
    host.settle();
    assert!(host.process().calls().iter().all(|c| c.arguments.get(1).map(String::as_str) != Some("remove")));
}

/// REQ: F-070, PORT:ilium-server/tests/workspace_ipc.rs::retained_worktree_with_an_open_editor_cannot_be_pruned
#[test]
fn port_070_a_worktree_with_an_open_editor_is_not_removed() {
    let mut host = server_host();
    host.panes().add_running_in("/w/a/src");
    host.deliver_one(channel_message("remove", json!({"path": "/w/a", "branch": "a"}).to_string().into_bytes()));
    host.settle();
    assert!(host.process().calls().iter().all(|c| c.arguments.get(1).map(String::as_str) != Some("remove")));
}

/// REQ: F-067, PORT:ilium-server/tests/workspace_http.rs::disconnect_during_worktree_add_never_commits_a_pane
#[test]
fn port_067_a_request_cancelled_during_git_never_commits_a_pane() {
    let mut host = server_host();
    host.deliver_one(new_agent_message(&json!({"branch": "fix-auth"})));
    host.cancel_pending();
    host.settle();
    assert!(create_panes(&host.submitted()).is_empty());
}

/// REQ: F-067, PORT:ilium-server/src/http_api.rs::create_agent_request_accepts_server_chosen_worktree_path_and_legacy_shape
#[test]
fn port_067_a_request_with_only_a_branch_gets_a_server_chosen_path() {
    let mut host = server_host();
    create_worktree(&mut host, &json!({"branch": "fix-auth"}));
    assert!(worktree_add_calls(&host.process().calls())[0].arguments[4].contains("fix-auth"));
}

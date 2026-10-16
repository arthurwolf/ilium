//! Twins of ilium-client/src/worktree_manager.rs: the removal rules the manager enforced.

use serde_json::json;

use crate::common::*;
use crate::support::*;

/// REQ: F-070, PORT:ilium-client/src/worktree_manager.rs::safe_prune_defaults_to_keep_branch_and_requires_exact_result_identity
#[test]
fn port_070_a_removal_without_merged_flag_keeps_the_branch() {
    let mut host = server_host_at("/projects/acme");
    remove_request(&mut host, json!({"path": "/projects/acme.worktrees/a", "branch": "a"}));
    assert!(!host.process().calls().iter().any(|call| call.arguments.first().map(String::as_str) == Some("branch") && matches!(call.arguments.get(1).map(String::as_str), Some("-d") | Some("-D"))));
}

/// REQ: F-070, PORT:ilium-client/src/worktree_manager.rs::safe_prune_defaults_to_keep_branch_and_requires_exact_result_identity
#[test]
fn port_070_a_removal_result_of_another_worktree_does_not_remove_this_one() {
    let mut host = server_host_at("/projects/acme");
    remove_request(&mut host, json!({"path": "/projects/acme.worktrees/a", "branch": "a"}));
    let removed: Vec<_> = removal_calls(&mut host);
    assert_eq!(removed.len(), 1);
    assert!(removed[0].iter().any(|argument| argument == "/projects/acme.worktrees/a"));
    assert!(!removed[0].iter().any(|argument| argument.ends_with("/b")));
}

/// REQ: F-070, PORT:ilium-client/src/worktree_manager.rs::discard_requires_the_exact_displayed_path
#[test]
fn port_070_a_path_that_is_only_a_parent_of_the_worktree_is_never_removed() {
    let mut host = server_host_at("/projects/acme");
    remove_request(&mut host, json!({"path": "/projects/acme.worktrees", "branch": "a"}));
    assert!(removal_calls(&mut host).is_empty());
}

/// REQ: F-070, PORT:ilium-client/src/worktree_manager.rs::discard_requires_the_exact_displayed_path
#[test]
fn port_070_a_path_with_extra_text_after_the_worktree_is_never_removed() {
    let mut host = server_host_at("/projects/acme");
    remove_request(&mut host, json!({"path": "/projects/acme.worktrees/a/../b", "branch": "a"}));
    assert!(removal_calls(&mut host).is_empty());
}

/// REQ: F-070, F-072, PORT:ilium-client/src/worktree_manager.rs::branch_catalog_changes_only_policy_and_rejects_reopened_confirmation
#[test]
fn port_072_changing_the_close_policy_changes_no_other_git_key() {
    let mut host = server_host_at("/projects/acme");
    let keys = ["default_where", "branch_prefix", "worktree_location_template", "default_base", "branch_line", "setup_command"];
    let before: Vec<_> = keys.iter().map(|key| host.kernel_settings().get("git", key)).collect();
    set_git(&mut host, "default_close_policy", json!("offer_removal_when_safe")).expect("valid");
    let after: Vec<_> = keys.iter().map(|key| host.kernel_settings().get("git", key)).collect();
    assert_eq!(before, after);
}

/// REQ: F-070, PORT:ilium-client/src/worktree_manager.rs::branch_catalog_changes_only_policy_and_rejects_reopened_confirmation
#[test]
fn port_070_a_repeated_removal_request_for_the_same_worktree_runs_git_once() {
    let mut host = server_host_at("/projects/acme");
    remove_request(&mut host, json!({"path": "/projects/acme.worktrees/a", "branch": "a"}));
    remove_request(&mut host, json!({"path": "/projects/acme.worktrees/a", "branch": "a"}));
    assert_eq!(removal_calls(&mut host).len(), 1);
}

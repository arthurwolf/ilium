//! F-070 Safe cleanup: the before-remove veto carries path, branch, dirty and merged; removal is
//! refused while the worktree has uncommitted changes or a running process.

use ilium_plugin_sdk::exports::HookReply;
use serde_json::{json, Value};

use crate::common::*;

fn remove(
    host: &mut ilium_plugin_sdk::testing::FakeHost,
    payload: Value,
) -> Vec<ilium_plugin_sdk::types::Submission> {
    host.deliver_one(channel_message("remove", payload.to_string().into_bytes()));
    host.settle();
    host.submitted()
}

fn remove_calls(host: &mut ilium_plugin_sdk::testing::FakeHost) -> Vec<Vec<String>> {
    host.process()
        .calls()
        .into_iter()
        .filter(|c| c.program == "git" && c.arguments.get(1).map(String::as_str) == Some("remove"))
        .map(|c| c.arguments)
        .collect()
}

/// REQ: F-070, EXT-042
#[test]
fn feature_070_before_remove_receives_path_branch_dirty_and_merged() {
    let mut host = server_host();
    host.hook_chain(BEFORE_REMOVE).reply_all(HookReply::Pass);
    remove(
        &mut host,
        json!({"path": "/w/docs-sweep", "branch": "docs-sweep"}),
    );
    let payload = host
        .hook_chain(BEFORE_REMOVE)
        .last_payload()
        .expect("payload");
    for key in ["path", "branch", "dirty", "merged"] {
        assert!(payload.get(key).is_some(), "{key}");
    }
}

/// REQ: F-070, EXT-042
#[test]
fn feature_070_a_refuse_reply_keeps_the_worktree() {
    let mut host = server_host();
    host.hook_chain(BEFORE_REMOVE)
        .reply_all(HookReply::Refuse("busy".to_string()));
    remove(
        &mut host,
        json!({"path": "/w/docs-sweep", "branch": "docs-sweep"}),
    );
    assert!(remove_calls(&mut host).is_empty());
}

/// REQ: F-070
#[test]
fn feature_070_a_clean_merged_worktree_is_removed_with_git_worktree_remove() {
    let mut host = server_host();
    host.hook_chain(BEFORE_REMOVE).reply_all(HookReply::Pass);
    remove(
        &mut host,
        json!({"path": "/w/docs-sweep", "branch": "docs-sweep"}),
    );
    assert_eq!(remove_calls(&mut host).len(), 1);
}

/// REQ: F-070
#[test]
fn feature_070_uncommitted_changes_block_removal_with_the_documented_notice() {
    let mut host = server_host();
    host.process().on_args(
        &["status", "--porcelain"],
        ilium_plugin_sdk::host::process::ProcessExit {
            status: Some(0),
            signal: None,
            stdout: b" M a.rs\n".to_vec(),
            stderr: Vec::new(),
        },
    );
    remove(
        &mut host,
        json!({"path": "/w/docs-sweep", "branch": "docs-sweep"}),
    );
    assert!(remove_calls(&mut host).is_empty());
    let notice = host.last_notice().expect("notice");
    assert!(notice.contains("docs-sweep cannot be removed"), "{notice}");
    assert!(notice.contains("It has uncommitted changes."), "{notice}");
}

/// REQ: F-070
#[test]
fn feature_070_a_running_process_in_the_worktree_blocks_removal() {
    let mut host = server_host();
    host.panes().add_running_in("/w/docs-sweep");
    remove(
        &mut host,
        json!({"path": "/w/docs-sweep", "branch": "docs-sweep"}),
    );
    assert!(remove_calls(&mut host).is_empty());
}

/// REQ: F-070
#[test]
fn feature_070_removal_never_uses_force() {
    let mut host = server_host();
    remove(
        &mut host,
        json!({"path": "/w/docs-sweep", "branch": "docs-sweep"}),
    );
    for args in remove_calls(&mut host) {
        assert!(!args.iter().any(|a| a == "--force" || a == "-f"));
    }
}

/// REQ: F-070
#[test]
fn feature_070_a_merged_branch_is_deleted_with_dash_d_and_an_unmerged_one_is_kept() {
    let mut host = server_host();
    remove(
        &mut host,
        json!({"path": "/w/a", "branch": "a", "merged": true}),
    );
    let calls = host.process().calls();
    assert!(calls.iter().any(|c| c.arguments.starts_with(&[
        "branch".to_string(),
        "-d".to_string(),
        "a".to_string()
    ])));
    let mut host = server_host();
    remove(
        &mut host,
        json!({"path": "/w/b", "branch": "b", "merged": false}),
    );
    assert!(!host
        .process()
        .calls()
        .iter()
        .any(
            |c| c.arguments.first().map(String::as_str) == Some("branch")
                && c.arguments.get(1).map(String::as_str) == Some("-d")
        ));
}

/// REQ: F-070
#[test]
fn feature_070_a_path_outside_the_worktrees_scope_is_never_removed() {
    let mut host = server_host();
    remove(&mut host, json!({"path": "/etc", "branch": "x"}));
    assert!(remove_calls(&mut host).is_empty());
}

/// REQ: F-070
#[test]
fn feature_070_the_main_checkout_is_never_removed() {
    let mut host = server_host();
    let root = host.project_root().to_string_lossy().into_owned();
    remove(&mut host, json!({"path": root, "branch": "master"}));
    assert!(remove_calls(&mut host).is_empty());
}

/// REQ: F-070
#[test]
fn feature_070_an_unreadable_status_means_uncertain_and_nothing_is_removed() {
    let mut host = server_host_unactivated();
    host.process().default_exit(failed_exit(128));
    host.activate::<ilium_plugin_core_worktrees::WorktreesServer>();
    remove(&mut host, json!({"path": "/w/a", "branch": "a"}));
    assert!(remove_calls(&mut host).is_empty());
}

/// REQ: F-070, F-072
#[test]
fn feature_070_close_policy_keep_never_offers_removal() {
    let mut host = server_host();
    host.deliver_one(ilium_plugin_sdk::types::Event::NodeRemoved(
        ilium_plugin_sdk::types::NodeId(14),
    ));
    host.settle();
    assert!(host.offered_actions().is_empty());
}

/// REQ: F-070, F-072
#[test]
fn feature_070_close_policy_offer_removal_when_safe_offers_for_a_clean_merged_owned_worktree() {
    let mut host = server_host();
    set_git(
        &mut host,
        "default_close_policy",
        json!("offer_removal_when_safe"),
    )
    .expect("valid");
    create_worktree(&mut host, &json!({"branch": "fix-auth"}));
    host.deliver_one(pane_created(14, "fix-auth"));
    host.deliver_one(ilium_plugin_sdk::types::Event::NodeRemoved(
        ilium_plugin_sdk::types::NodeId(14),
    ));
    host.settle();
    assert_eq!(host.offered_actions().len(), 1);
}

/// REQ: F-070
#[test]
fn feature_070_a_worktree_the_plugin_does_not_own_is_never_offered_for_removal() {
    let mut host = server_host();
    set_git(
        &mut host,
        "default_close_policy",
        json!("offer_removal_when_safe"),
    )
    .expect("valid");
    host.deliver_one(ilium_plugin_sdk::types::Event::NodeRemoved(
        ilium_plugin_sdk::types::NodeId(99),
    ));
    host.settle();
    assert!(host.offered_actions().is_empty());
}

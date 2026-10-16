//! F-067 Agent in a new Git worktree: the server side runs `git worktree add`, submits `create-pane`
//! with the worktree as its start folder and on `pane-created` registers the pane as the owner.
//! Expected values come from the page samples and from src/docs/worktrees.md.

use ilium_plugin_sdk::exports::HookReply;
use ilium_plugin_sdk::testing::FakeSession;
use ilium_plugin_sdk::types::{ErrorCode, NodeId, PluginError};
use proptest::prelude::*;
use serde_json::{json, Value};

use crate::common::*;

fn add_call(host: &mut ilium_plugin_sdk::testing::FakeHost) -> ilium_plugin_sdk::host::process::ProcessSpec {
    let calls = worktree_add_calls(&host.process().calls());
    assert_eq!(calls.len(), 1, "exactly one git worktree add");
    calls[0].clone()
}

/// REQ: F-067
#[test]
fn feature_067_new_agent_runs_git_worktree_add_with_a_new_branch_from_head() {
    let mut host = server_host();
    create_worktree(&mut host, &json!({"branch": "fix-auth"}));
    let call = add_call(&mut host);
    assert_eq!(call.program, "git");
    assert_eq!(&call.arguments[..4], ["worktree", "add", "-b", "fix-auth"]);
    assert_eq!(call.arguments.len(), 6);
    assert_eq!(call.arguments[5], "HEAD");
}

/// REQ: F-067
#[test]
fn feature_067_git_runs_in_the_repository_with_a_sixty_second_timeout_and_no_stdin() {
    let mut host = server_host();
    create_worktree(&mut host, &json!({"branch": "fix-auth"}));
    let call = add_call(&mut host);
    assert!(call.cwd.is_some());
    assert_eq!(call.timeout_millis, 60_000);
    assert_eq!(call.stdin, None);
}

/// REQ: F-067
#[test]
fn feature_067_the_default_path_is_the_repository_name_dot_worktrees_and_the_branch_slug() {
    let mut host = server_host();
    create_worktree(&mut host, &json!({"branch": "fix-auth"}));
    let call = add_call(&mut host);
    assert!(call.arguments[4].ends_with(".worktrees/fix-auth"), "{}", call.arguments[4]);
}

/// REQ: F-067
#[test]
fn feature_067_the_branch_slug_replaces_the_slash_of_agent_fix_login() {
    let mut host = server_host();
    create_worktree(&mut host, &json!({"branch": "agent/fix-login"}));
    let call = add_call(&mut host);
    assert_eq!(call.arguments[3], "agent/fix-login");
    assert!(call.arguments[4].ends_with(".worktrees/agent-fix-login"), "{}", call.arguments[4]);
}

/// REQ: F-067, F-072
#[test]
fn feature_067_the_location_template_setting_decides_the_path() {
    let mut host = server_host();
    set_git(&mut host, "worktree_location_template", json!("/srv/trees/{branch_slug}")).expect("valid template");
    create_worktree(&mut host, &json!({"branch": "agent/fix-login"}));
    assert_eq!(add_call(&mut host).arguments[4], "/srv/trees/agent-fix-login");
}

/// REQ: F-067
#[test]
fn feature_067_a_base_in_the_request_replaces_head() {
    let mut host = server_host();
    create_worktree(&mut host, &json!({"branch": "agent/fix-login", "base": "main"}));
    assert_eq!(add_call(&mut host).arguments[5], "main");
}

/// REQ: F-067
#[test]
fn feature_067_the_registered_base_is_the_base_given_to_git() {
    let mut host = server_host();
    create_worktree(&mut host, &json!({"branch": "x", "base": "main"}));
    host.deliver_one(pane_created(7, "x"));
    host.settle();
    assert_eq!(registered(&mut host)[0].base.as_deref(), Some("main"));
}

/// REQ: F-067
#[test]
fn feature_067_create_pane_starts_a_claude_terminal_titled_with_the_branch() {
    let mut host = server_host();
    let submitted = create_worktree(&mut host, &json!({"branch": "fix-auth"}));
    let panes = create_panes(&submitted);
    assert_eq!(panes.len(), 1);
    assert_eq!(panes[0].title, "fix-auth");
    assert_eq!(panes[0].content_kind, "ilium.terminal");
    assert_eq!(panes[0].command, Some(vec!["claude".to_string()]));
    assert_eq!(panes[0].parent, None);
    assert_eq!(panes[0].initial_input, None);
}

/// REQ: F-067
#[test]
fn feature_067_the_pane_starts_in_the_worktree_folder() {
    let mut host = server_host();
    let submitted = create_worktree(&mut host, &json!({"branch": "fix-auth"}));
    let panes = create_panes(&submitted);
    assert!(cwd_text(&panes[0]).contains(".worktrees/fix-auth"), "{}", cwd_text(&panes[0]));
}

/// REQ: F-067
#[test]
fn feature_067_a_nonzero_git_exit_creates_no_pane_and_registers_nothing() {
    let mut host = server_host_unactivated();
    host.process().default_exit(failed_exit(128));
    host.activate::<ilium_plugin_core_worktrees::WorktreesServer>();
    let submitted = create_worktree(&mut host, &json!({"branch": "fix-auth"}));
    assert!(create_panes(&submitted).is_empty());
    host.deliver_one(pane_created(7, "fix-auth"));
    host.settle();
    assert!(registered(&mut host).is_empty());
}

/// REQ: F-067
#[test]
fn feature_067_a_refused_process_call_creates_no_pane() {
    let mut host = server_host();
    host.inject_failure(
        "process.run",
        PluginError { code: ErrorCode::NotPermitted, message: "no grant".to_string(), retry_after_ms: None },
    );
    let submitted = create_worktree(&mut host, &json!({"branch": "fix-auth"}));
    assert!(create_panes(&submitted).is_empty());
}

/// REQ: F-067
#[test]
fn feature_067_invalid_branch_names_never_reach_git() {
    for branch in ["", "bad name", "a..b", "@", "x@{1}", "a//b", "-lead", "end/", "end.", "x.lock", "a/.hidden", "a~b", "a^b", "a:b", "a?b", "a*b", "a[b", "a\\b"] {
        let mut host = server_host();
        let submitted = create_worktree(&mut host, &json!({"branch": branch}));
        assert!(host.process().calls().is_empty(), "git ran for {branch:?}");
        assert!(create_panes(&submitted).is_empty(), "pane for {branch:?}");
    }
}

/// REQ: F-067
#[test]
fn feature_067_a_request_without_a_branch_or_with_broken_json_never_reaches_git() {
    for payload in [b"{}".to_vec(), b"not json".to_vec(), Vec::new(), br#"{"branch": 5}"#.to_vec()] {
        let mut host = server_host();
        host.deliver_one(channel_message("new-agent", payload));
        host.settle();
        assert!(host.process().calls().is_empty());
    }
}

/// REQ: F-067
#[test]
fn feature_067_pane_created_registers_the_pane_as_owner_of_the_worktree() {
    let mut host = server_host();
    create_worktree(&mut host, &json!({"branch": "fix-auth"}));
    host.deliver_one(pane_created(14, "fix-auth"));
    host.settle();
    let records = registered(&mut host);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].branch, "fix-auth");
    assert_eq!(records[0].base.as_deref(), Some("HEAD"));
    assert_eq!(records[0].owner, Some(NodeId(14)));
    assert_eq!(records[0].path.scope, "worktrees");
}

/// REQ: F-067
#[test]
fn feature_067_the_registered_path_is_the_folder_of_the_git_call() {
    let mut host = server_host();
    create_worktree(&mut host, &json!({"branch": "agent/fix-login"}));
    host.deliver_one(pane_created(14, "agent/fix-login"));
    host.settle();
    let records = registered(&mut host);
    assert!(records[0].path.path.ends_with("agent-fix-login"), "{}", records[0].path.path);
    assert!(add_call(&mut host).arguments[4].ends_with(&records[0].path.path));
}

/// REQ: F-067
#[test]
fn feature_067_a_pane_created_with_an_unknown_title_registers_nothing() {
    let mut host = server_host();
    create_worktree(&mut host, &json!({"branch": "fix-auth"}));
    host.deliver_one(pane_created(14, "some-other-pane"));
    host.settle();
    assert!(registered(&mut host).is_empty());
}

/// REQ: F-067
#[test]
fn feature_067_the_same_pane_created_twice_registers_once() {
    let mut host = server_host();
    create_worktree(&mut host, &json!({"branch": "fix-auth"}));
    host.deliver_one(pane_created(14, "fix-auth"));
    host.deliver_one(pane_created(14, "fix-auth"));
    host.settle();
    assert_eq!(registered(&mut host).len(), 1);
}

/// REQ: F-067
#[test]
fn feature_067_pane_created_before_any_request_registers_nothing() {
    let mut host = server_host();
    host.deliver_one(pane_created(14, "fix-auth"));
    host.settle();
    assert!(registered(&mut host).is_empty());
}

/// REQ: F-067
#[test]
fn feature_067_two_requests_for_one_branch_run_git_once() {
    let mut host = server_host();
    host.deliver_one(new_agent_message(&json!({"branch": "fix-auth"})));
    host.deliver_one(new_agent_message(&json!({"branch": "fix-auth"})));
    host.settle();
    assert_eq!(worktree_add_calls(&host.process().calls()).len(), 1);
    assert_eq!(create_panes(&host.submitted()).len(), 1);
}

/// REQ: F-067
#[test]
fn feature_067_requests_for_two_branches_run_git_twice_and_create_two_panes() {
    let mut host = server_host();
    host.deliver_one(new_agent_message(&json!({"branch": "one"})));
    host.deliver_one(new_agent_message(&json!({"branch": "two"})));
    host.settle();
    assert_eq!(worktree_add_calls(&host.process().calls()).len(), 2);
    assert_eq!(create_panes(&host.submitted()).len(), 2);
}

/// REQ: F-067
#[test]
fn feature_067_the_changed_topic_is_published_after_creation_with_path_and_state() {
    let mut host = server_host();
    create_worktree(&mut host, &json!({"branch": "fix-auth"}));
    let published = host.published(TOPIC_CHANGED);
    assert!(!published.is_empty());
    let value: Value = serde_json::from_str(&published[0]).expect("topic payload is JSON");
    assert!(value["path"].as_str().expect("path").ends_with("fix-auth"));
    assert_eq!(value["state"], "clean");
}

/// REQ: F-067
#[test]
fn feature_067_the_changed_topic_is_not_published_when_git_fails() {
    let mut host = server_host_unactivated();
    host.process().default_exit(failed_exit(128));
    host.activate::<ilium_plugin_core_worktrees::WorktreesServer>();
    create_worktree(&mut host, &json!({"branch": "fix-auth"}));
    assert!(host.published(TOPIC_CHANGED).is_empty());
}

/// REQ: EXT-042, F-067
#[test]
fn feature_067_before_create_receives_repo_branch_base_and_agent() {
    let mut host = server_host_unactivated_at("/work/ilium");
    host.process().default_exit(ok_exit());
    host.activate::<ilium_plugin_core_worktrees::WorktreesServer>();
    host.hook_chain(BEFORE_CREATE).reply_all(HookReply::Pass);
    create_worktree(&mut host, &json!({"branch": "fix-auth"}));
    let payload = host.hook_chain(BEFORE_CREATE).last_payload().expect("payload");
    assert_eq!(payload["branch"], "fix-auth");
    assert_eq!(payload["base"], "HEAD");
    assert_eq!(payload["repo"], "/work/ilium", "repo is the main checkout path");
    assert_eq!(payload["agent"], "claude", "agent is the default agent command");
}

/// REQ: EXT-042, F-067
#[test]
fn feature_067_a_replace_reply_with_a_branch_changes_the_branch_git_creates() {
    let mut host = server_host();
    host.hook_chain(BEFORE_CREATE).reply_all(HookReply::Replace(json!({"branch": "renamed/x"}).to_string()));
    let submitted = create_worktree(&mut host, &json!({"branch": "fix-auth"}));
    let call = add_call(&mut host);
    assert_eq!(call.arguments[3], "renamed/x");
    assert_eq!(create_panes(&submitted)[0].title, "renamed/x");
}

/// REQ: EXT-042, F-067
#[test]
fn feature_067_a_refuse_reply_creates_neither_worktree_nor_pane() {
    let mut host = server_host();
    host.hook_chain(BEFORE_CREATE).reply_all(HookReply::Refuse("policy".to_string()));
    let submitted = create_worktree(&mut host, &json!({"branch": "fix-auth"}));
    assert!(host.process().calls().is_empty());
    assert!(create_panes(&submitted).is_empty());
}

/// REQ: EXT-042, F-067
#[test]
fn feature_067_a_pass_reply_keeps_the_requested_branch() {
    let mut host = server_host();
    host.hook_chain(BEFORE_CREATE).reply_all(HookReply::Pass);
    create_worktree(&mut host, &json!({"branch": "fix-auth"}));
    assert_eq!(add_call(&mut host).arguments[3], "fix-auth");
}

/// REQ: EXT-042, F-067
#[test]
fn feature_067_a_replacement_branch_that_is_not_a_valid_branch_name_never_reaches_git() {
    let mut host = server_host();
    host.hook_chain(BEFORE_CREATE).reply_all(HookReply::Replace(json!({"branch": "bad name"}).to_string()));
    create_worktree(&mut host, &json!({"branch": "fix-auth"}));
    assert!(worktree_add_calls(&host.process().calls()).is_empty());
}

/// REQ: EXT-042, F-067
#[test]
fn feature_067_the_hook_chain_runs_in_priority_order() {
    let mut host = server_host();
    host.hook_chain(BEFORE_CREATE).add("late", 200, HookReply::Replace(json!({"branch": "late"}).to_string()));
    host.hook_chain(BEFORE_CREATE).add("early", 100, HookReply::Replace(json!({"branch": "early"}).to_string()));
    create_worktree(&mut host, &json!({"branch": "fix-auth"}));
    assert_eq!(add_call(&mut host).arguments[3], "late");
}

/// REQ: F-067
#[test]
fn feature_067_the_create_service_runs_git_worktree_add_for_the_branch() {
    let mut session = FakeSession::new();
    session.install_core("worktrees").start();
    session.server(PLUGIN_ID).process().default_exit(ok_exit());
    let _reply = session
        .call_service("ilium.core.fleet", "ilium.core.worktrees/create", "^0.1", &json!({"branch": "svc"}).to_string())
        .expect("service call");
    session.pump().expect("settles");
    let calls = worktree_add_calls(&session.server(PLUGIN_ID).process().calls());
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].arguments[3], "svc");
}

/// REQ: F-067
#[test]
fn feature_067_the_create_service_replies_with_the_worktree_path() {
    let mut session = FakeSession::new();
    session.install_core("worktrees").start();
    session.server(PLUGIN_ID).process().default_exit(ok_exit());
    let reply = session
        .call_service("ilium.core.fleet", "ilium.core.worktrees/create", "^0.1", &json!({"branch": "svc"}).to_string())
        .expect("service call");
    session.pump().expect("settles");
    if let ilium_plugin_sdk::exports::ServiceReply::Done(text) = reply {
        let value: Value = serde_json::from_str(&text).expect("reply is JSON");
        assert!(value["path"].as_str().expect("path").ends_with(".worktrees/svc"));
    }
}

proptest! {
    // REQ: F-067
    #[test]
    fn feature_067_every_valid_branch_is_passed_to_git_unchanged_and_slugged_in_the_path(
        branch in "[a-z][a-z0-9]{0,7}(/[a-z][a-z0-9]{0,7}){0,2}"
    ) {
        let mut host = server_host();
        create_worktree(&mut host, &json!({"branch": branch.clone()}));
        let call = add_call(&mut host);
        prop_assert_eq!(call.arguments[3].clone(), branch.clone());
        let expected_tail = format!(".worktrees/{}", branch.replace('/', "-"));
        prop_assert!(call.arguments[4].ends_with(&expected_tail), "{}", call.arguments[4]);
    }

    // REQ: F-067
    #[test]
    fn feature_067_a_branch_with_a_forbidden_character_never_reaches_git(
        left in "[a-z]{1,5}",
        bad in prop::sample::select(vec![' ', '~', '^', ':', '?', '*', '[', '\\']),
        right in "[a-z]{1,5}",
    ) {
        let mut host = server_host();
        create_worktree(&mut host, &json!({"branch": format!("{left}{bad}{right}")}));
        prop_assert!(host.process().calls().is_empty());
    }
}

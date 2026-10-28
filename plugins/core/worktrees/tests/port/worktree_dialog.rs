//! Twins of ilium-client/src/worktree_dialog.rs: the defaults and checks the dialog applied are
//! decided by the server side of the plugin when the request arrives (branch, base, path, setup).

use ilium_plugin_sdk::testing::FakeHost;
use serde_json::json;

use crate::common::*;
use crate::support::*;

const PROJECT: &str = "/projects/acme";

/// REQ: F-067, PORT:ilium-client/src/worktree_dialog.rs::loads_facts_and_creates_collision_free_branch_in_main_sibling
#[test]
fn port_067_a_default_request_gets_a_collision_free_branch_a_base_of_the_checkout_and_a_main_sibling_path(
) {
    let mut host = server_host_at(PROJECT);
    with_repository_facts(&mut host);
    create_worktree(&mut host, &json!({}));
    let arguments = add_arguments(&mut host);
    assert_eq!(arguments[3], "agent/task-2");
    assert_eq!(arguments[4], "/projects/acme.worktrees/agent-task-2");
    assert_eq!(arguments[5], "topic");
}

/// REQ: F-067, PORT:ilium-client/src/worktree_dialog.rs::default_path_preserves_inserted_placeholder_literals_in_repository_name
#[test]
fn port_067_a_placeholder_text_inside_the_repository_name_is_kept_literally_in_the_path() {
    let mut host = server_host_at("/projects/{branch_slug}-repo");
    with_repository_facts(&mut host);
    create_worktree(&mut host, &json!({}));
    assert_eq!(
        add_arguments(&mut host)[4],
        "/projects/{branch_slug}-repo.worktrees/agent-task-2"
    );
}

/// REQ: F-071, PORT:ilium-client/src/worktree_dialog.rs::configured_setup_is_sent_with_new_worktree_only
#[test]
fn port_071_the_configured_setup_runs_for_a_new_worktree() {
    let mut host = server_host_at(PROJECT);
    set_git(
        &mut host,
        "setup_command",
        json!("printf ready > setup-result"),
    )
    .expect("valid");
    create_worktree(&mut host, &request("fix-auth"));
    let shell: Vec<_> = host
        .process()
        .calls()
        .into_iter()
        .filter(|call| call.program == "sh")
        .collect();
    assert_eq!(shell.len(), 1);
    assert_eq!(shell[0].arguments, ["-c", "printf ready > setup-result"]);
}

/// REQ: F-071, PORT:ilium-client/src/worktree_dialog.rs::configured_setup_is_sent_with_new_worktree_only
#[test]
fn port_071_the_configured_setup_does_not_run_for_an_existing_worktree() {
    let mut host = server_host_at(PROJECT);
    set_git(
        &mut host,
        "setup_command",
        json!("printf ready > setup-result"),
    )
    .expect("valid");
    host.workspace().add_foreign("docs-sweep", "docs-sweep");
    create_worktree(&mut host, &json!({"worktree": "docs-sweep"}));
    assert!(host
        .process()
        .calls()
        .iter()
        .all(|call| call.program != "sh"));
}

/// REQ: F-072, F-071, PORT:ilium-client/src/worktree_dialog.rs::oversized_setup_is_rejected_before_submission
#[test]
fn port_072_a_setup_command_of_8193_bytes_is_refused_and_names_the_limit_8192() {
    let mut host = server_host_at(PROJECT);
    let error =
        set_git(&mut host, "setup_command", json!("x".repeat(8 * 1024 + 1))).expect_err("refused");
    assert!(format!("{error:?}").contains("8192"), "{error:?}");
    create_worktree(&mut host, &request("fix-auth"));
    assert!(host
        .process()
        .calls()
        .iter()
        .all(|call| call.program != "sh"));
}

/// REQ: F-072, F-067, PORT:ilium-client/src/worktree_dialog.rs::git_defaults_set_branch_base_path_and_close_policy_without_overwriting_manual_branch
#[test]
fn port_072_prefix_template_and_default_base_decide_branch_path_and_base_of_a_default_request() {
    let mut host = server_host_at(PROJECT);
    with_repository_facts(&mut host);
    set_git(&mut host, "branch_prefix", json!("feature/")).expect("valid");
    set_git(
        &mut host,
        "worktree_location_template",
        json!("{project}/.ilium/worktrees/{branch_slug}"),
    )
    .expect("valid");
    set_git(&mut host, "default_base", json!("default_branch")).expect("valid");
    create_worktree(&mut host, &json!({}));
    let arguments = add_arguments(&mut host);
    assert_eq!(arguments[3], "feature/task");
    assert_eq!(arguments[4], "/projects/acme/.ilium/worktrees/feature-task");
    assert_eq!(arguments[5], "main");
}

/// REQ: F-072, F-070, PORT:ilium-client/src/worktree_dialog.rs::git_defaults_set_branch_base_path_and_close_policy_without_overwriting_manual_branch
#[test]
fn port_072_the_close_policy_default_is_stored_and_read_back() {
    let mut host: FakeHost = server_host_at(PROJECT);
    set_git(
        &mut host,
        "default_close_policy",
        json!("offer_removal_when_safe"),
    )
    .expect("valid");
    assert_eq!(
        host.kernel_settings().get("git", "default_close_policy"),
        Some(json!("offer_removal_when_safe"))
    );
}

/// REQ: F-067, PORT:ilium-client/src/worktree_dialog.rs::git_defaults_set_branch_base_path_and_close_policy_without_overwriting_manual_branch
#[test]
fn port_067_a_branch_given_in_the_request_is_never_replaced_by_the_prefix_default() {
    let mut host = server_host_at(PROJECT);
    with_repository_facts(&mut host);
    set_git(&mut host, "branch_prefix", json!("feature/")).expect("valid");
    create_worktree(&mut host, &request("custom/topic"));
    assert_eq!(add_arguments(&mut host)[3], "custom/topic");
}

/// REQ: F-067, PORT:ilium-client/src/worktree_dialog.rs::manual_branch_survives_prompt_changes_and_duplicate_is_rejected
#[test]
fn port_067_a_manual_branch_is_used_as_typed() {
    let mut host = server_host_at(PROJECT);
    with_repository_facts(&mut host);
    create_worktree(&mut host, &request("agent/my-choice"));
    assert_eq!(add_arguments(&mut host)[3], "agent/my-choice");
}

/// REQ: F-067, PORT:ilium-client/src/worktree_dialog.rs::manual_branch_survives_prompt_changes_and_duplicate_is_rejected
#[test]
fn port_067_a_branch_that_already_exists_is_refused_and_git_is_not_asked_to_add() {
    let mut host = server_host_at(PROJECT);
    with_repository_facts(&mut host);
    let submitted = create_worktree(&mut host, &request("main"));
    assert!(worktree_add_calls(&host.process().calls()).is_empty());
    assert!(create_panes(&submitted).is_empty());
}

/// REQ: F-067, PORT:ilium-client/src/worktree_dialog.rs::path_overlap_and_occupied_existing_are_blocked
#[test]
fn port_067_a_path_inside_the_main_checkout_is_refused() {
    let mut host = server_host_at(PROJECT);
    with_repository_facts(&mut host);
    let submitted = create_worktree(
        &mut host,
        &json!({"branch": "agent/nested", "path": "/projects/acme/nested"}),
    );
    assert!(worktree_add_calls(&host.process().calls()).is_empty());
    assert!(create_panes(&submitted).is_empty());
}

/// REQ: F-068, PORT:ilium-client/src/worktree_dialog.rs::path_overlap_and_occupied_existing_are_blocked
#[test]
fn port_068_an_existing_worktree_with_a_live_agent_is_refused_and_a_free_one_is_accepted() {
    let mut host = server_host_at(PROJECT);
    host.workspace().add_foreign("existing", "feature");
    host.workspace().add_foreign("free", "other");
    host.deliver_one(new_agent_message(&json!({"worktree": "existing"})));
    host.deliver_one(pane_created(9, "feature"));
    host.settle();
    let before = create_panes(&host.submitted()).len();
    host.deliver_one(new_agent_message(&json!({"worktree": "existing"})));
    host.settle();
    assert_eq!(
        create_panes(&host.submitted()).len(),
        before,
        "a second agent in the same worktree is refused"
    );
    host.deliver_one(new_agent_message(&json!({"worktree": "free"})));
    host.settle();
    assert_eq!(create_panes(&host.submitted()).len(), before + 1);
}

/// REQ: F-067, F-072, PORT:ilium-client/src/worktree_dialog.rs::template_slashes_become_the_platform_separator_without_touching_other_text
#[test]
fn port_067_on_windows_the_forward_slashes_of_the_template_become_backslashes() {
    let mut host = server_host_at(r"C:\repos\app");
    host.set_platform("windows");
    set_git(
        &mut host,
        "worktree_location_template",
        json!("{project}/.ilium/worktrees/{branch_slug}"),
    )
    .expect("valid");
    create_worktree(&mut host, &request("x"));
    assert_eq!(
        add_arguments(&mut host)[4],
        r"C:\repos\app\.ilium\worktrees\x"
    );
}

/// REQ: F-067, F-072, PORT:ilium-client/src/worktree_dialog.rs::template_slashes_become_the_platform_separator_without_touching_other_text
#[test]
fn port_067_on_linux_the_template_path_is_unchanged() {
    let mut host = server_host_at("/repos/app");
    set_git(
        &mut host,
        "worktree_location_template",
        json!("{project}/.ilium/worktrees/{branch_slug}"),
    )
    .expect("valid");
    create_worktree(&mut host, &request("x"));
    assert_eq!(add_arguments(&mut host)[4], "/repos/app/.ilium/worktrees/x");
}

/// REQ: PORT:ilium-client/src/connection.rs::accepted_dialog_error_owner_survives_stale_same_kind_response
#[test]
fn port_067_a_refused_request_is_not_revived_by_a_repeat_and_does_not_block_the_next_one() {
    let mut host = server_host_at(PROJECT);
    with_repository_facts(&mut host);
    let first = create_worktree(&mut host, &request("main"));
    assert!(worktree_add_calls(&host.process().calls()).is_empty());
    assert!(create_panes(&first).is_empty());
    let repeat = create_worktree(&mut host, &request("main"));
    assert!(
        worktree_add_calls(&host.process().calls()).is_empty(),
        "the repeated refusal adds nothing"
    );
    assert!(create_panes(&repeat).is_empty());
    create_worktree(&mut host, &request("agent/fresh"));
    assert_eq!(add_arguments(&mut host)[3], "agent/fresh");
}

/// REQ: PORT:ilium-client/src/worktree_dialog.rs::loads_facts_and_creates_collision_free_branch_in_main_sibling
#[test]
fn port_067_a_default_branch_skips_every_taken_suffix_and_the_path_follows_it() {
    let mut host = server_host_at(PROJECT);
    host.process().on_args(
        &["branch", "--format=%(refname:short)"],
        stdout_exit("main\nagent/task\nagent/task-2\n"),
    );
    host.process().on_args(
        &["rev-parse", "--abbrev-ref", "HEAD"],
        stdout_exit("topic\n"),
    );
    host.process().on_args(
        &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
        stdout_exit("origin/main\n"),
    );
    create_worktree(&mut host, &json!({}));
    let arguments = add_arguments(&mut host);
    assert_eq!(arguments[3], "agent/task-3");
    assert_eq!(arguments[4], "/projects/acme.worktrees/agent-task-3");
}

/// REQ: PORT:ilium-client/src/worktree_dialog.rs::default_path_preserves_inserted_placeholder_literals_in_repository_name
#[test]
fn port_067_a_placeholder_text_in_the_project_root_is_not_expanded_by_the_location_template() {
    let mut host = server_host_at("/projects/{branch_slug}-repo");
    with_repository_facts(&mut host);
    set_git(
        &mut host,
        "worktree_location_template",
        json!("{project}/.ilium/worktrees/{branch_slug}"),
    )
    .expect("valid");
    create_worktree(&mut host, &json!({}));
    assert_eq!(
        add_arguments(&mut host)[4],
        "/projects/{branch_slug}-repo/.ilium/worktrees/agent-task-2"
    );
}

/// REQ: PORT:ilium-client/src/worktree_dialog.rs::oversized_setup_is_rejected_before_submission
#[test]
fn port_072_a_setup_command_of_exactly_8192_bytes_is_accepted_and_runs_unchanged() {
    let mut host = server_host_at(PROJECT);
    let setup = "x".repeat(8 * 1024);
    set_git(&mut host, "setup_command", json!(setup.clone()))
        .expect("8192 bytes is within the limit");
    create_worktree(&mut host, &request("fix-auth"));
    let shell: Vec<_> = host
        .process()
        .calls()
        .into_iter()
        .filter(|call| call.program == "sh")
        .collect();
    assert_eq!(shell.len(), 1);
    assert_eq!(shell[0].arguments, ["-c", setup.as_str()]);
}

/// REQ: PORT:ilium-client/src/connection.rs::accepted_dialog_error_owner_survives_stale_same_kind_response
#[test]
fn port_067_a_refused_branch_does_not_block_a_default_request_that_follows() {
    let mut host = server_host_at(PROJECT);
    with_repository_facts(&mut host);
    let refused = create_worktree(&mut host, &request("main"));
    assert!(create_panes(&refused).is_empty());
    create_worktree(&mut host, &json!({}));
    assert_eq!(add_arguments(&mut host)[3], "agent/task-2");
}

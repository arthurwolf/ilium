//! F-072 Git and worktree defaults: the seven `[git]` keys, their defaults, their validation and
//! the messages of the existing loader (ilium-client/src/config.rs, GitSettings::validate).

use ilium_plugin_sdk::types::ErrorCode;
use proptest::prelude::*;
use serde_json::{json, Value};

use crate::common::{client_host, set_git};

fn read(host: &mut ilium_plugin_sdk::testing::FakeHost, key: &str) -> Option<Value> {
    host.kernel_settings().get("git", key)
}

fn rejected(key: &str, value: Value) -> ilium_plugin_sdk::types::PluginError {
    let mut host = client_host();
    set_git(&mut host, key, value).expect_err("the value must be refused")
}

/// REQ: F-072, COR-011
#[test]
fn feature_072_defaults_of_the_seven_git_keys() {
    let mut host = client_host();
    assert_eq!(read(&mut host, "default_where"), Some(json!("here")));
    assert_eq!(read(&mut host, "branch_prefix"), Some(json!("agent/")));
    assert_eq!(
        read(&mut host, "worktree_location_template"),
        Some(json!("{repo_parent}/{repo_name}.worktrees/{branch_slug}"))
    );
    assert_eq!(read(&mut host, "default_base"), Some(json!("current")));
    assert_eq!(read(&mut host, "branch_line"), Some(json!("worktree_only")));
    assert_eq!(read(&mut host, "setup_command"), Some(json!("")));
    assert_eq!(read(&mut host, "default_close_policy"), Some(json!("keep")));
}

/// REQ: F-072
#[test]
fn feature_072_a_valid_value_is_stored_and_read_back() {
    let mut host = client_host();
    set_git(&mut host, "branch_prefix", json!("feature/")).expect("valid");
    assert_eq!(read(&mut host, "branch_prefix"), Some(json!("feature/")));
}

/// REQ: F-072, COR-033
#[test]
fn feature_072_changing_one_key_leaves_the_other_six_unchanged() {
    let mut host = client_host();
    set_git(&mut host, "default_base", json!("default_branch")).expect("valid");
    assert_eq!(read(&mut host, "default_where"), Some(json!("here")));
    assert_eq!(read(&mut host, "branch_prefix"), Some(json!("agent/")));
    assert_eq!(read(&mut host, "branch_line"), Some(json!("worktree_only")));
    assert_eq!(read(&mut host, "setup_command"), Some(json!("")));
    assert_eq!(read(&mut host, "default_close_policy"), Some(json!("keep")));
    assert_eq!(
        read(&mut host, "default_base"),
        Some(json!("default_branch"))
    );
}

/// REQ: F-072
#[test]
fn feature_072_default_where_accepts_here_new_worktree_and_existing_worktree() {
    let mut host = client_host();
    for value in ["here", "new_worktree", "existing_worktree"] {
        set_git(&mut host, "default_where", json!(value))
            .unwrap_or_else(|error| panic!("{value}: {error:?}"));
        assert_eq!(read(&mut host, "default_where"), Some(json!(value)));
    }
}

/// REQ: F-072
#[test]
fn feature_072_default_where_refuses_an_unknown_value_with_invalid_argument() {
    assert_eq!(
        rejected("default_where", json!("elsewhere")).code,
        ErrorCode::InvalidArgument
    );
}

/// REQ: F-072
#[test]
fn feature_072_default_base_accepts_current_and_default_branch_only() {
    let mut host = client_host();
    for value in ["current", "default_branch"] {
        set_git(&mut host, "default_base", json!(value)).expect("valid");
    }
    assert_eq!(
        rejected("default_base", json!("main")).code,
        ErrorCode::InvalidArgument
    );
}

/// REQ: F-072, F-069
#[test]
fn feature_072_branch_line_accepts_worktree_only_and_off_only() {
    let mut host = client_host();
    for value in ["worktree_only", "off"] {
        set_git(&mut host, "branch_line", json!(value)).expect("valid");
    }
    assert_eq!(
        rejected("branch_line", json!("always")).code,
        ErrorCode::InvalidArgument
    );
}

/// REQ: F-072, F-070
#[test]
fn feature_072_default_close_policy_accepts_keep_and_offer_removal_when_safe_only() {
    let mut host = client_host();
    for value in ["keep", "offer_removal_when_safe"] {
        set_git(&mut host, "default_close_policy", json!(value)).expect("valid");
    }
    assert_eq!(
        rejected("default_close_policy", json!("remove")).code,
        ErrorCode::InvalidArgument
    );
}

/// REQ: F-072
#[test]
fn feature_072_a_template_without_branch_slug_is_refused_with_the_loader_message() {
    let error = rejected(
        "worktree_location_template",
        json!("{repo_parent}/{repo_name}.worktrees"),
    );
    assert_eq!(error.code, ErrorCode::InvalidArgument);
    assert_eq!(
        error.message,
        "Worktree location must include {branch_slug}"
    );
}

/// REQ: F-072
#[test]
fn feature_072_an_empty_or_blank_template_is_refused_with_the_branch_slug_message() {
    for value in ["", "   "] {
        let error = rejected("worktree_location_template", json!(value));
        assert_eq!(
            error.message,
            "Worktree location must include {branch_slug}"
        );
    }
}

/// REQ: F-072
#[test]
fn feature_072_an_unknown_placeholder_is_refused_naming_the_placeholder() {
    let error = rejected("worktree_location_template", json!("{home}/{branch_slug}"));
    assert_eq!(
        error.message,
        "Unsupported worktree location placeholder: {home}"
    );
}

/// REQ: F-072
#[test]
fn feature_072_an_unclosed_placeholder_is_refused() {
    let error = rejected(
        "worktree_location_template",
        json!("{repo_parent}/{branch_slug}/{repo_name"),
    );
    assert_eq!(
        error.message,
        "Worktree location has an unclosed placeholder"
    );
}

/// REQ: F-072
#[test]
fn feature_072_an_unmatched_closing_brace_before_a_placeholder_is_refused() {
    let error = rejected("worktree_location_template", json!("bad}/{branch_slug}"));
    assert_eq!(
        error.message,
        "Worktree location has an unmatched closing brace"
    );
}

/// REQ: F-072
#[test]
fn feature_072_an_unmatched_closing_brace_after_the_last_placeholder_is_refused() {
    let error = rejected("worktree_location_template", json!("{branch_slug}/tail}"));
    assert_eq!(
        error.message,
        "Worktree location has an unmatched closing brace"
    );
}

/// REQ: F-072
#[test]
fn feature_072_the_four_known_placeholders_are_accepted() {
    let mut host = client_host();
    set_git(
        &mut host,
        "worktree_location_template",
        json!("{project}/{repo_parent}/{repo_name}/{branch_slug}"),
    )
    .expect("valid");
    set_git(
        &mut host,
        "worktree_location_template",
        json!("{project}/.ilium/worktrees/{branch_slug}"),
    )
    .expect("valid");
}

/// REQ: F-072
#[test]
fn feature_072_a_branch_prefix_that_makes_an_invalid_branch_name_is_refused() {
    for prefix in ["bad prefix/", "../", "-lead/", "a..b/", "x:/"] {
        let error = rejected("branch_prefix", json!(prefix));
        assert_eq!(error.code, ErrorCode::InvalidArgument, "{prefix}");
        assert!(
            error.message.starts_with("Invalid branch prefix: "),
            "{prefix}: {}",
            error.message
        );
    }
}

/// REQ: F-072
#[test]
fn feature_072_an_empty_branch_prefix_is_valid_because_the_suggestion_is_the_slug_alone() {
    let mut host = client_host();
    set_git(&mut host, "branch_prefix", json!("")).expect("empty prefix gives the valid name task");
}

/// REQ: F-072, F-071
#[test]
fn feature_072_a_setup_command_of_exactly_8192_bytes_is_accepted() {
    let mut host = client_host();
    set_git(&mut host, "setup_command", json!("x".repeat(8192))).expect("n bytes");
}

/// REQ: F-072, F-071
#[test]
fn feature_072_a_setup_command_of_8193_bytes_is_refused_with_the_loader_message() {
    let error = rejected("setup_command", json!("x".repeat(8193)));
    assert_eq!(error.code, ErrorCode::InvalidArgument);
    assert_eq!(
        error.message,
        "Setup command must be at most 8192 bytes and contain no NUL"
    );
}

/// REQ: F-072, F-071
#[test]
fn feature_072_a_setup_command_of_8191_bytes_is_accepted() {
    let mut host = client_host();
    set_git(&mut host, "setup_command", json!("x".repeat(8191))).expect("n-1 bytes");
}

/// REQ: F-072, F-071
#[test]
fn feature_072_the_limit_counts_bytes_not_characters() {
    let two_byte = "é".repeat(4097);
    assert_eq!(two_byte.len(), 8194);
    let error = rejected("setup_command", json!(two_byte));
    assert_eq!(
        error.message,
        "Setup command must be at most 8192 bytes and contain no NUL"
    );
}

/// REQ: F-072, F-071
#[test]
fn feature_072_a_setup_command_with_a_nul_character_is_refused() {
    let error = rejected("setup_command", json!("bun install\u{0}"));
    assert_eq!(
        error.message,
        "Setup command must be at most 8192 bytes and contain no NUL"
    );
}

/// REQ: F-072, F-071
#[test]
fn feature_072_a_blank_setup_command_is_valid_and_means_no_command() {
    let mut host = client_host();
    set_git(&mut host, "setup_command", json!("   ")).expect("blank is allowed");
}

/// REQ: F-072
#[test]
fn feature_072_a_refused_value_leaves_the_stored_value_unchanged() {
    let mut host = client_host();
    set_git(&mut host, "branch_prefix", json!("feature/")).expect("valid");
    let _ = set_git(&mut host, "branch_prefix", json!("bad prefix/"));
    assert_eq!(read(&mut host, "branch_prefix"), Some(json!("feature/")));
}

/// REQ: F-072
#[test]
fn feature_072_an_unknown_key_is_refused() {
    let _ = rejected("worktree_root", json!("/tmp"));
}

/// REQ: F-072
#[test]
fn feature_072_a_wrong_value_type_is_refused() {
    let _ = rejected("setup_command", json!(5));
}

proptest! {
    // REQ: F-072
    #[test]
    fn feature_072_every_template_without_branch_slug_is_refused(template in "[a-z/._-]{0,40}") {
        let mut host = client_host();
        let error = set_git(&mut host, "worktree_location_template", json!(template)).expect_err("no slug");
        prop_assert_eq!(error.message, "Worktree location must include {branch_slug}");
    }

    // REQ: F-072
    #[test]
    fn feature_072_every_template_made_of_known_placeholders_is_accepted(
        head in prop::sample::select(vec!["{repo_parent}", "{project}", "/abs"]),
        mid in prop::sample::select(vec!["", "/{repo_name}", "/{repo_name}.worktrees", "/.ilium"]),
    ) {
        let mut host = client_host();
        let template = format!("{head}{mid}/{{branch_slug}}");
        prop_assert!(set_git(&mut host, "worktree_location_template", json!(template)).is_ok());
    }

    // REQ: F-072
    #[test]
    fn feature_072_setup_commands_up_to_the_byte_limit_without_nul_are_accepted(length in 0usize..=8192) {
        let mut host = client_host();
        prop_assert!(set_git(&mut host, "setup_command", json!("a".repeat(length))).is_ok());
    }
}

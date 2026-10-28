//! Twins of `ilium/src/pane_filter.rs`, driven through `ilium panes` selection options
//! (src/docs/cli-reference.md, "ilium panes").

use ilium_plugin_sdk::testing::FakeHost;
use serde_json::{json, Value};

use crate::common::*;

fn pane(
    id: u64,
    name: &str,
    kind: &str,
    agent: Option<&str>,
    state: Option<&str>,
    project: &str,
) -> Value {
    json!({
        "pane_id": id, "name": name, "kind": kind, "agent": agent, "state": state,
        "project": project, "cwd": project, "is_self": false,
    })
}

fn host_with(panes: &[Value]) -> FakeHost {
    let mut host = server_host();
    add_session(&mut host, "default", true, panes);
    host
}

fn ids(host: &mut FakeHost, args: &[&str]) -> Vec<u64> {
    let (stdout, _, _) = run_cli(host, args, b"");
    let all = records(&stdout);
    let mut found: Vec<u64> = of_type(&all, "pane")
        .iter()
        .map(|r| r["pane_id"].as_u64().unwrap())
        .collect();
    found.sort();
    found
}

fn error_code(host: &mut FakeHost, args: &[&str]) -> (String, Option<u8>) {
    let (stdout, _, exit) = run_cli(host, args, b"");
    (
        of_type(&records(&stdout), "error")[0]["code"]
            .as_str()
            .unwrap()
            .to_string(),
        exit,
    )
}

/// REQ: PORT:ilium/src/pane_filter.rs::an_empty_filter_keeps_every_pane
#[test]
fn pane_filter_an_empty_filter_keeps_every_pane() {
    let mut host = host_with(&[
        pane(1, "a", "agent", Some("Claude"), Some("working"), "/w/a"),
        pane(2, "b", "shell", None, None, "/w/b"),
    ]);
    assert_eq!(ids(&mut host, &["panes"]), [1, 2]);
}

/// REQ: PORT:ilium/src/pane_filter.rs::match_is_a_case_insensitive_substring_over_all_fields_by_default
#[test]
fn pane_filter_match_is_a_case_insensitive_substring_over_all_fields_by_default() {
    let mut host = host_with(&[
        pane(
            1,
            "other",
            "agent",
            Some("Claude"),
            Some("idle"),
            "/w/renderer",
        ),
        pane(
            2,
            "Render pass",
            "agent",
            Some("Codex"),
            Some("idle"),
            "/w/x",
        ),
        pane(3, "plain", "agent", Some("Codex"), Some("idle"), "/w/y"),
    ]);
    assert_eq!(ids(&mut host, &["panes", "--match", "RENDER"]), [1, 2]);
}

/// REQ: PORT:ilium/src/pane_filter.rs::repeated_text_patterns_are_alternatives_and_field_limits_the_search
#[test]
fn pane_filter_repeated_text_patterns_are_alternatives_and_field_limits_the_search() {
    let mut host = host_with(&[
        pane(
            1,
            "Alpha task",
            "agent",
            Some("Claude"),
            Some("idle"),
            "/w/x",
        ),
        pane(2, "BETA7", "agent", Some("Claude"), Some("idle"), "/w/x"),
        pane(3, "beta77", "agent", Some("Claude"), Some("idle"), "/w/x"),
        pane(
            4,
            "unrelated",
            "agent",
            Some("Claude"),
            Some("idle"),
            "/w/alpha",
        ),
    ]);
    let found = ids(
        &mut host,
        &[
            "panes",
            "--match",
            "alpha",
            "--regex",
            r"^beta\d$",
            "--field",
            "name",
        ],
    );
    assert_eq!(found, [1, 2]);
}

/// REQ: PORT:ilium/src/pane_filter.rs::invert_keeps_only_panes_the_text_filter_rejects
#[test]
fn pane_filter_invert_keeps_only_panes_the_text_filter_rejects() {
    let mut host = host_with(&[
        pane(
            1,
            "code review",
            "agent",
            Some("Codex"),
            Some("idle"),
            "/w/x",
        ),
        pane(2, "build", "agent", Some("Codex"), Some("idle"), "/w/x"),
        pane(3, "build", "agent", Some("Claude"), Some("idle"), "/w/x"),
    ]);
    assert_eq!(
        ids(
            &mut host,
            &["panes", "--regex", "review", "--invert", "--agent", "codex"]
        ),
        [2]
    );
}

/// REQ: PORT:ilium/src/pane_filter.rs::invert_without_a_text_pattern_and_bad_regexes_are_refused
#[test]
fn pane_filter_invert_without_a_text_pattern_and_bad_regexes_are_refused() {
    let mut host = host_with(&[pane(1, "a", "agent", Some("Claude"), Some("idle"), "/w/a")]);
    assert_eq!(
        error_code(&mut host, &["panes", "--invert"]),
        ("invalid-selection".to_string(), Some(2))
    );
    assert_eq!(
        error_code(&mut host, &["panes", "--regex", "("]),
        ("invalid-selection".to_string(), Some(2))
    );
}

/// REQ: PORT:ilium/src/pane_filter.rs::projects_select_by_directory_subtree_or_by_folder_name
#[test]
fn pane_filter_projects_select_by_directory_subtree_or_by_folder_name() {
    let mut host = host_with(&[
        pane(1, "p", "agent", Some("Claude"), Some("idle"), "/w/team"),
        pane(2, "p", "agent", Some("Claude"), Some("idle"), "/w/team/api"),
        pane(
            3,
            "p",
            "agent",
            Some("Claude"),
            Some("idle"),
            "/elsewhere/lumen",
        ),
        pane(4, "p", "agent", Some("Claude"), Some("idle"), "/w/teammate"),
        pane(5, "p", "agent", Some("Claude"), Some("idle"), "/w/other"),
    ]);
    assert_eq!(
        ids(
            &mut host,
            &["panes", "--project", "/w/team", "--project", "Lumen"]
        ),
        [1, 2, 3]
    );
}

/// REQ: PORT:ilium/src/pane_filter.rs::relative_project_directories_resolve_against_cwd
#[test]
fn pane_filter_relative_project_directories_resolve_against_cwd() {
    let mut host = host_with(&[
        pane(
            1,
            "p",
            "agent",
            Some("Claude"),
            Some("idle"),
            "/no/such/root/b",
        ),
        pane(
            2,
            "p",
            "agent",
            Some("Claude"),
            Some("idle"),
            "/no/such/root/a",
        ),
    ]);
    assert_eq!(
        ids(
            &mut host,
            &["panes", "--cwd", "/no/such/root/a", "--project", "../b"]
        ),
        [1]
    );
}

/// REQ: PORT:ilium/src/pane_filter.rs::excluded_projects_win_over_included_ones
#[test]
fn pane_filter_excluded_projects_win_over_included_ones() {
    let mut host = host_with(&[
        pane(1, "p", "agent", Some("Claude"), Some("idle"), "/w/public"),
        pane(2, "p", "agent", Some("Claude"), Some("idle"), "/w/private"),
    ]);
    assert_eq!(
        ids(
            &mut host,
            &[
                "panes",
                "--project",
                "/w",
                "--exclude-project",
                "/w/private"
            ]
        ),
        [1]
    );
}

/// REQ: PORT:ilium/src/pane_filter.rs::here_keeps_the_project_that_contains_cwd
#[test]
fn pane_filter_here_keeps_the_project_that_contains_cwd() {
    let mut host = host_with(&[
        pane(1, "p", "agent", Some("Claude"), Some("idle"), "/w/team"),
        pane(2, "p", "agent", Some("Claude"), Some("idle"), "/w/other"),
    ]);
    host.fs().write("/w/team/api/.keep", b"");
    assert_eq!(
        ids(&mut host, &["panes", "--cwd", "/w/team/api", "--here"]),
        [1]
    );
}

/// REQ: PORT:ilium/src/pane_filter.rs::agent_state_kind_session_and_pane_selectors_narrow_together
#[test]
fn pane_filter_agent_state_kind_session_and_pane_selectors_narrow_together() {
    let mut host = host_with(&[
        pane(1, "a", "agent", Some("Claude"), Some("idle"), "/w/a"),
        pane(2, "b", "agent", Some("Codex"), Some("done"), "/w/a"),
        pane(3, "c", "agent", Some("Codex"), Some("working"), "/w/a"),
        pane(4, "d", "shell", None, None, "/w/a"),
    ]);
    assert_eq!(
        ids(
            &mut host,
            &[
                "panes",
                "--agent",
                "claude,codex",
                "--state",
                "idle",
                "--session",
                "default"
            ]
        ),
        [1, 2]
    );
    assert_eq!(ids(&mut host, &["panes", "--state", "done"]), [2]);
    assert_eq!(ids(&mut host, &["panes", "--kind", "shell"]), [4]);
    assert_eq!(
        ids(
            &mut host,
            &["panes", "--pane", "1", "--pane", "2", "--agent", "codex"]
        ),
        [2]
    );
}

/// REQ: PORT:ilium/src/pane_filter.rs::pane_facts_describe_agents_shells_and_projects_from_a_tree
#[test]
fn pane_filter_pane_facts_describe_agents_shells_and_projects_from_a_tree() {
    let mut host = host_with(&[
        pane(1, "agent", "agent", Some("Codex"), Some("done"), "/w/a"),
        pane(2, "shell", "shell", None, None, "/w/a"),
    ]);
    let (stdout, _, _) = run_cli(&mut host, &["panes"], b"");
    let all = records(&stdout);
    let by_id = |id: u64| {
        of_type(&all, "pane")
            .into_iter()
            .find(|r| r["pane_id"] == id)
            .unwrap()
            .clone()
    };
    let agent = by_id(1);
    assert_eq!(
        (
            agent["agent"].as_str(),
            agent["state"].as_str(),
            agent["kind"].as_str()
        ),
        (Some("Codex"), Some("done"), Some("agent"))
    );
    let shell = by_id(2);
    assert_eq!(shell["kind"], "shell");
    assert!(shell["state"].is_null());
    assert_eq!(shell["project"], "/w/a");
}

/// REQ: PORT:ilium/src/pane_filter.rs::an_empty_filter_keeps_every_pane
#[test]
fn pane_filter_no_panes_in_the_session_yields_an_empty_listing() {
    let mut host = host_with(&[]);
    assert_eq!(ids(&mut host, &["panes"]), Vec::<u64>::new());
}

/// REQ: PORT:ilium/src/pane_filter.rs::match_is_a_case_insensitive_substring_over_all_fields_by_default
#[test]
fn pane_filter_field_limit_excludes_matches_in_other_fields() {
    let mut host = host_with(&[
        pane(
            1,
            "other",
            "agent",
            Some("Claude"),
            Some("idle"),
            "/w/renderer",
        ),
        pane(
            2,
            "Render pass",
            "agent",
            Some("Codex"),
            Some("idle"),
            "/w/x",
        ),
    ]);
    assert_eq!(
        ids(
            &mut host,
            &["panes", "--match", "renderer", "--field", "name"]
        ),
        Vec::<u64>::new()
    );
}

/// REQ: PORT:ilium/src/pane_filter.rs::repeated_text_patterns_are_alternatives_and_field_limits_the_search
#[test]
fn pane_filter_text_pattern_without_field_limit_matches_project_too() {
    let mut host = host_with(&[
        pane(
            1,
            "Alpha task",
            "agent",
            Some("Claude"),
            Some("idle"),
            "/w/x",
        ),
        pane(2, "BETA7", "agent", Some("Claude"), Some("idle"), "/w/x"),
        pane(3, "beta77", "agent", Some("Claude"), Some("idle"), "/w/x"),
        pane(
            4,
            "unrelated",
            "agent",
            Some("Claude"),
            Some("idle"),
            "/w/alpha",
        ),
    ]);
    assert_eq!(ids(&mut host, &["panes", "--match", "alpha"]), [1, 4]);
}

/// REQ: PORT:ilium/src/pane_filter.rs::invert_keeps_only_panes_the_text_filter_rejects
#[test]
fn pane_filter_invert_with_regex_keeps_every_non_matching_agent() {
    let mut host = host_with(&[
        pane(
            1,
            "code review",
            "agent",
            Some("Codex"),
            Some("idle"),
            "/w/x",
        ),
        pane(2, "build", "agent", Some("Codex"), Some("idle"), "/w/x"),
        pane(3, "build", "agent", Some("Claude"), Some("idle"), "/w/x"),
    ]);
    assert_eq!(
        ids(&mut host, &["panes", "--regex", "review", "--invert"]),
        [2, 3]
    );
}

/// REQ: PORT:ilium/src/pane_filter.rs::invert_without_a_text_pattern_and_bad_regexes_are_refused
#[test]
fn pane_filter_invert_with_a_text_pattern_is_accepted() {
    let mut host = host_with(&[pane(1, "a", "agent", Some("Claude"), Some("idle"), "/w/a")]);
    assert_eq!(
        ids(&mut host, &["panes", "--invert", "--match", "zzz"]),
        [1]
    );
}

/// REQ: PORT:ilium/src/pane_filter.rs::projects_select_by_directory_subtree_or_by_folder_name
#[test]
fn pane_filter_project_directory_selects_only_its_subtree() {
    let mut host = host_with(&[
        pane(1, "p", "agent", Some("Claude"), Some("idle"), "/w/team"),
        pane(2, "p", "agent", Some("Claude"), Some("idle"), "/w/team/api"),
        pane(
            3,
            "p",
            "agent",
            Some("Claude"),
            Some("idle"),
            "/elsewhere/lumen",
        ),
        pane(4, "p", "agent", Some("Claude"), Some("idle"), "/w/teammate"),
    ]);
    assert_eq!(ids(&mut host, &["panes", "--project", "/w/team"]), [1, 2]);
}

/// REQ: PORT:ilium/src/pane_filter.rs::relative_project_directories_resolve_against_cwd
#[test]
fn pane_filter_relative_dot_project_resolves_to_the_cwd_directory() {
    let mut host = host_with(&[
        pane(
            1,
            "p",
            "agent",
            Some("Claude"),
            Some("idle"),
            "/no/such/root/b",
        ),
        pane(
            2,
            "p",
            "agent",
            Some("Claude"),
            Some("idle"),
            "/no/such/root/a",
        ),
    ]);
    assert_eq!(
        ids(
            &mut host,
            &["panes", "--cwd", "/no/such/root/a", "--project", "."]
        ),
        [2]
    );
}

/// REQ: PORT:ilium/src/pane_filter.rs::excluded_projects_win_over_included_ones
#[test]
fn pane_filter_excluding_the_including_root_leaves_nothing() {
    let mut host = host_with(&[
        pane(1, "p", "agent", Some("Claude"), Some("idle"), "/w/public"),
        pane(2, "p", "agent", Some("Claude"), Some("idle"), "/w/private"),
    ]);
    assert_eq!(
        ids(
            &mut host,
            &["panes", "--project", "/w", "--exclude-project", "/w"]
        ),
        Vec::<u64>::new()
    );
}

/// REQ: PORT:ilium/src/pane_filter.rs::here_keeps_the_project_that_contains_cwd
#[test]
fn pane_filter_here_with_cwd_outside_every_project_keeps_nothing() {
    let mut host = host_with(&[
        pane(1, "p", "agent", Some("Claude"), Some("idle"), "/w/team"),
        pane(2, "p", "agent", Some("Claude"), Some("idle"), "/w/other"),
    ]);
    assert_eq!(
        ids(&mut host, &["panes", "--cwd", "/elsewhere", "--here"]),
        Vec::<u64>::new()
    );
}

/// REQ: PORT:ilium/src/pane_filter.rs::agent_state_kind_session_and_pane_selectors_narrow_together
#[test]
fn pane_filter_state_and_kind_selectors_narrow_together() {
    let mut host = host_with(&[
        pane(1, "a", "agent", Some("Claude"), Some("idle"), "/w/a"),
        pane(2, "b", "agent", Some("Codex"), Some("done"), "/w/a"),
        pane(3, "c", "agent", Some("Codex"), Some("working"), "/w/a"),
        pane(4, "d", "shell", None, None, "/w/a"),
    ]);
    assert_eq!(ids(&mut host, &["panes", "--state", "working"]), [3]);
    assert_eq!(
        ids(
            &mut host,
            &["panes", "--kind", "agent", "--agent", "codex", "--state", "done"]
        ),
        [2]
    );
}

/// REQ: PORT:ilium/src/pane_filter.rs::pane_facts_describe_agents_shells_and_projects_from_a_tree
#[test]
fn pane_filter_kind_agent_excludes_shells() {
    let mut host = host_with(&[
        pane(1, "agent", "agent", Some("Codex"), Some("done"), "/w/a"),
        pane(2, "shell", "shell", None, None, "/w/a"),
    ]);
    assert_eq!(ids(&mut host, &["panes", "--kind", "agent"]), [1]);
}

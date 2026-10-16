//! `ilium new-pane` (F-107, COR-040): plain command panes through the `create-pane` intent, the
//! success line, option validation and the `--worktree` argument rules
//! (src/docs/cli-reference.md "ilium new-pane").

use ilium_plugin_sdk::testing::FakeHost;
use proptest::prelude::*;

use crate::common::*;

fn host_with_session() -> FakeHost {
    let mut host = server_host();
    add_session(&mut host, "default", true, &[]);
    host
}

fn command_line(host: &FakeHost) -> String {
    let specs = created_panes(host);
    assert_eq!(specs.len(), 1, "one create-pane expected: {specs:?}");
    specs[0].title.clone()
}

/// REQ: F-107
#[test]
fn new_pane_107_a_plain_command_creates_one_terminal_pane() {
    let mut host = host_with_session();
    let (_, _, exit) = run_cli(&mut host, &["new-pane", "--", "claude"], b"");
    let specs = created_panes(&host);
    assert_eq!(exit, Some(0));
    assert_eq!(specs.len(), 1);
    assert_eq!(specs[0].content_kind, "ilium.terminal");
}

/// REQ: F-107
#[test]
fn new_pane_107_the_pane_is_added_to_the_top_level_of_the_tree() {
    let mut host = host_with_session();
    run_cli(&mut host, &["new-pane", "--", "claude"], b"");
    assert_eq!(created_panes(&host)[0].parent, None);
}

/// REQ: F-107
#[test]
fn new_pane_107_the_pane_starts_in_the_project_root() {
    let mut host = host_with_session();
    run_cli(&mut host, &["new-pane", "--", "claude"], b"");
    assert_eq!(created_panes(&host)[0].cwd, None);
}

/// REQ: F-107
#[test]
fn new_pane_107_the_command_is_shown_as_the_pane_name() {
    let mut host = host_with_session();
    run_cli(&mut host, &["new-pane", "--", "tail", "-f", "/var/log/syslog"], b"");
    assert_eq!(command_line(&host), "tail -f /var/log/syslog");
}

/// REQ: F-107
#[test]
fn new_pane_107_arguments_with_spaces_are_quoted_to_keep_their_boundaries() {
    let mut host = host_with_session();
    run_cli(&mut host, &["new-pane", "--", "ls", "my folder"], b"");
    assert_eq!(command_line(&host), "ls 'my folder'");
}

/// REQ: F-107
#[test]
fn new_pane_107_only_arguments_that_need_quoting_are_quoted() {
    let mut host = host_with_session();
    run_cli(&mut host, &["new-pane", "--", "sh", "-c", "npm run build && npm test"], b"");
    assert_eq!(command_line(&host), "sh -c 'npm run build && npm test'");
}

/// REQ: F-107
#[test]
fn new_pane_107_everything_after_the_double_dash_is_literal() {
    let mut host = host_with_session();
    run_cli(&mut host, &["new-pane", "--", "ls", "--keep-open", "--session-name"], b"");
    assert_eq!(command_line(&host), "ls --keep-open --session-name");
}

/// REQ: F-107
#[test]
fn new_pane_107_the_spawned_command_is_the_shell_dash_c_the_command_line() {
    let mut host = host_with_session();
    run_cli(&mut host, &["new-pane", "--", "echo", "hi"], b"");
    let command = created_panes(&host)[0].command.clone().expect("command given");
    assert_eq!(command.len(), 3, "{command:?}");
    assert_eq!(command[1], "-c");
    assert_eq!(command[2], "echo hi");
}

/// REQ: F-107
#[test]
fn new_pane_107_success_prints_one_plain_text_line_not_json() {
    let mut host = host_with_session();
    let (stdout, _, exit) = run_cli(&mut host, &["new-pane", "--", "claude"], b"");
    assert_eq!(exit, Some(0));
    assert_eq!(stdout, "pane created in project session \"default\"\n");
}

/// REQ: F-107
#[test]
fn new_pane_107_session_name_selects_the_session_named_in_the_success_line() {
    let mut host = host_with_session();
    add_session(&mut host, "review", false, &[]);
    let (stdout, _, _) = run_cli(&mut host, &["new-pane", "--session-name", "review", "--", "codex"], b"");
    assert_eq!(stdout, "pane created in project session \"review\"\n");
}

/// REQ: F-107
#[test]
fn new_pane_107_keep_open_is_accepted_for_a_plain_pane() {
    let mut host = host_with_session();
    let (_, _, exit) = run_cli(&mut host, &["new-pane", "--keep-open", "--", "tail", "-f", "log"], b"");
    assert_eq!(exit, Some(0));
    assert_eq!(created_panes(&host).len(), 1);
}

/// REQ: F-107
#[test]
fn new_pane_107_a_missing_command_creates_nothing_and_fails() {
    let mut host = host_with_session();
    let (_, stderr, exit) = run_cli(&mut host, &["new-pane", "--"], b"");
    assert_ne!(exit, Some(0));
    assert!(!stderr.is_empty());
    assert!(created_panes(&host).is_empty());
}

/// REQ: F-107
#[test]
fn new_pane_107_no_arguments_at_all_creates_nothing_and_fails() {
    let mut host = host_with_session();
    let (_, _, exit) = run_cli(&mut host, &["new-pane"], b"");
    assert_ne!(exit, Some(0));
    assert!(created_panes(&host).is_empty());
}

/// REQ: F-107
#[test]
fn new_pane_107_keep_open_cannot_be_combined_with_worktree() {
    let mut host = host_with_session();
    let (_, _, exit) = run_cli(&mut host, &["new-pane", "--keep-open", "--worktree", "--branch", "agent/x", "--", "claude"], b"");
    assert_ne!(exit, Some(0));
    assert!(created_panes(&host).is_empty());
}

/// REQ: F-107
#[test]
fn new_pane_107_worktree_requires_a_branch() {
    let mut host = host_with_session();
    let (_, _, exit) = run_cli(&mut host, &["new-pane", "--worktree", "--", "claude"], b"");
    assert_ne!(exit, Some(0));
    assert!(created_panes(&host).is_empty());
}

/// REQ: F-107
#[test]
fn new_pane_107_branch_requires_worktree() {
    let mut host = host_with_session();
    let (_, _, exit) = run_cli(&mut host, &["new-pane", "--branch", "agent/x", "--", "claude"], b"");
    assert_ne!(exit, Some(0));
    assert!(created_panes(&host).is_empty());
}

/// REQ: F-107
#[test]
fn new_pane_107_base_requires_worktree() {
    let mut host = host_with_session();
    let (_, _, exit) = run_cli(&mut host, &["new-pane", "--base", "main", "--", "claude"], b"");
    assert_ne!(exit, Some(0));
    assert!(created_panes(&host).is_empty());
}

/// REQ: F-107
#[test]
fn new_pane_107_worktree_accepts_exactly_one_built_in_agent_command() {
    let mut host = host_with_session();
    let (stdout, stderr, exit) = run_cli(&mut host, &["new-pane", "--worktree", "--branch", "agent/x", "--", "vim"], b"");
    assert_ne!(exit, Some(0));
    let text = format!("{stdout}{stderr}");
    assert!(text.contains("--worktree requires exactly one built-in agent command: claude, codex, or agy"), "{text}");
    assert!(created_panes(&host).is_empty());
}

/// REQ: F-107
#[test]
fn new_pane_107_worktree_with_two_words_is_refused_with_the_same_message() {
    let mut host = host_with_session();
    let (stdout, stderr, exit) =
        run_cli(&mut host, &["new-pane", "--worktree", "--branch", "agent/x", "--", "claude", "--resume"], b"");
    assert_ne!(exit, Some(0));
    assert!(format!("{stdout}{stderr}").contains("--worktree requires exactly one built-in agent command"));
}

/// REQ: F-107
#[test]
fn new_pane_107_worktree_names_all_three_built_in_agents_in_the_refusal() {
    let mut host = host_with_session();
    let (stdout, stderr, _) = run_cli(&mut host, &["new-pane", "--worktree", "--branch", "agent/x", "--", "bash"], b"");
    let text = format!("{stdout}{stderr}");
    for agent in ["claude", "codex", "agy"] {
        assert!(text.contains(agent), "{agent} missing in {text}");
    }
}

/// REQ: F-107
#[test]
fn new_pane_107_invalid_branch_names_fail_before_anything_is_created() {
    for branch in ["", "has space", "a~b", "a^b", "a:b", "a?b", "a*b", "a[b", "a\\b", "a..b", "a@{b", "a//b", "-lead", "trail/", "trail.", ".hidden", "ok/.hidden", "x.lock", "ok/x.lock"] {
        let mut host = host_with_session();
        let (stdout, stderr, exit) = run_cli(&mut host, &["new-pane", "--worktree", "--branch", branch, "--", "claude"], b"");
        assert_ne!(exit, Some(0), "branch {branch:?} must be refused: {stdout}{stderr}");
        assert!(created_panes(&host).is_empty(), "branch {branch:?}");
        assert!(host.service_calls().is_empty(), "branch {branch:?} must fail before the worktree service");
    }
}

/// REQ: F-107
#[test]
fn new_pane_107_a_control_character_in_the_branch_is_refused() {
    let mut host = host_with_session();
    let (_, _, exit) = run_cli(&mut host, &["new-pane", "--worktree", "--branch", "a\u{7}b", "--", "claude"], b"");
    assert_ne!(exit, Some(0));
    assert!(created_panes(&host).is_empty());
}

/// REQ: F-107
#[test]
fn new_pane_107_a_valid_branch_is_not_refused_by_the_name_check() {
    let mut host = host_with_session();
    host.stub_service("ilium.core.worktrees/create", "create", r#"{"ok":true}"#);
    let (stdout, _, _) = run_cli(&mut host, &["new-pane", "--worktree", "--branch", "agent/fix-login", "--", "codex"], b"");
    assert!(!stdout.contains("requires exactly one built-in agent command"), "{stdout}");
}

/// REQ: F-107
#[test]
fn new_pane_107_worktree_output_is_json_lines_with_a_progress_record_first() {
    let mut host = host_with_session();
    host.stub_service("ilium.core.worktrees/create", "create", r#"{"ok":true}"#);
    let (stdout, _, _) = run_cli(&mut host, &["new-pane", "--worktree", "--branch", "agent/fix-login", "--", "codex"], b"");
    let all = records(&stdout);
    assert!(!all.is_empty());
    assert_eq!(all[0]["type"], "progress");
    assert_eq!(all[0]["stage"], "querying-repository");
    assert!(all[0]["request_id"].is_number());
}

proptest! {
    // REQ: F-107
    #[test]
    fn new_pane_107_plain_words_without_special_characters_are_never_quoted(words in proptest::collection::vec("[a-z0-9./_-]{1,8}", 1..5)) {
        prop_assume!(!words[0].starts_with('-'));
        let mut host = host_with_session();
        let mut args = vec!["new-pane", "--"];
        args.extend(words.iter().map(String::as_str));
        run_cli(&mut host, &args, b"");
        prop_assert_eq!(command_line(&host), words.join(" "));
    }
}

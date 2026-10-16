//! `ilium panes` (F-109, COR-040): one `pane` record per pane across every live session, a
//! closing `summary`, selection options, the cost column from `ilium.core.cost/usage`.
//! Wire shape per src/docs/cli-reference.md "ilium panes" and the page sample (requests
//! `list-panes` with `{}` to every live session).

use serde_json::{Value, json};

use crate::common::*;

fn two_sessions(host: &mut ilium_plugin_sdk::testing::FakeHost) {
    add_session(
        host,
        "default",
        true,
        &[pane_json(3, "refactor parser", "agent", Some("Claude"), Some("working")), pane_json(5, "tests", "agent", Some("Codex"), Some("idle"))],
    );
    add_session(
        host,
        "review",
        false,
        &[pane_json(2, "review", "agent", Some("Claude"), Some("waiting-approval")), pane_json(4, "shell", "shell", None, None)],
    );
}

fn pane_ids(all: &[Value]) -> Vec<(String, u64)> {
    of_type(all, "pane")
        .iter()
        .map(|record| (record["session"].as_str().unwrap().to_string(), record["pane_id"].as_u64().unwrap()))
        .collect()
}

/// REQ: F-109
#[test]
fn panes_109_lists_one_pane_record_per_pane_of_every_session() {
    let mut host = server_host();
    two_sessions(&mut host);
    let (stdout, _, exit) = run_cli(&mut host, &["panes"], b"");
    let all = records(&stdout);
    assert_eq!(exit, Some(0));
    let mut ids = pane_ids(&all);
    ids.sort();
    assert_eq!(
        ids,
        [("default".to_string(), 3), ("default".to_string(), 5), ("review".to_string(), 2), ("review".to_string(), 4)]
    );
}

/// REQ: F-109
#[test]
fn panes_109_a_pane_record_carries_the_documented_fields() {
    let mut host = server_host();
    two_sessions(&mut host);
    let (stdout, _, _) = run_cli(&mut host, &["panes", "--pane", "3", "--session", "default"], b"");
    let all = records(&stdout);
    let record = of_type(&all, "pane")[0].clone();
    for field in ["session", "pane_id", "name", "kind", "agent", "state", "project", "cwd", "is_self"] {
        assert!(record.get(field).is_some(), "field {field} missing in {record}");
    }
    assert_eq!(record["name"], "refactor parser");
    assert_eq!(record["kind"], "agent");
    assert_eq!(record["state"], "working");
}

/// REQ: F-109
#[test]
fn panes_109_agent_and_state_are_null_for_a_shell_pane() {
    let mut host = server_host();
    two_sessions(&mut host);
    let (stdout, _, _) = run_cli(&mut host, &["panes", "--kind", "shell"], b"");
    let all = records(&stdout);
    let shells = of_type(&all, "pane");
    assert_eq!(shells.len(), 1);
    assert_eq!(shells[0]["agent"], Value::Null);
    assert_eq!(shells[0]["state"], Value::Null);
}

/// REQ: F-109
#[test]
fn panes_109_the_summary_counts_sessions_and_panes() {
    let mut host = server_host();
    two_sessions(&mut host);
    let (stdout, _, _) = run_cli(&mut host, &["panes"], b"");
    let summary = summary(&records(&stdout));
    assert_eq!(summary["command"], "panes");
    assert_eq!(summary["ok"], true);
    assert_eq!(summary["sessions"], 2);
    assert_eq!(summary["unreachable_sessions"], 0);
    assert_eq!(summary["panes"], 4);
}

/// REQ: F-109
#[test]
fn panes_109_the_summary_is_the_last_record() {
    let mut host = server_host();
    two_sessions(&mut host);
    let (stdout, _, _) = run_cli(&mut host, &["panes"], b"");
    let all = records(&stdout);
    assert_eq!(all.last().expect("records")["type"], "summary");
}

/// REQ: F-109
#[test]
fn panes_109_every_live_session_receives_one_list_panes_request_with_empty_arguments() {
    let mut host = server_host();
    two_sessions(&mut host);
    run_cli(&mut host, &["panes"], b"");
    let mut requests = host.sessions().requests();
    requests.sort();
    assert_eq!(
        requests,
        [
            ("default".to_string(), "list-panes".to_string(), "{}".to_string()),
            ("review".to_string(), "list-panes".to_string(), "{}".to_string()),
        ]
    );
}

/// REQ: F-109
#[test]
fn panes_109_no_live_session_finishes_with_exit_code_one() {
    let mut host = server_host();
    let (_, _, exit) = run_cli(&mut host, &["panes"], b"");
    assert_eq!(exit, Some(1));
}

/// REQ: F-109
#[test]
fn panes_109_an_unreachable_session_gives_a_warning_and_ok_false_but_exit_zero() {
    let mut host = server_host();
    two_sessions(&mut host);
    add_unreachable_session(&mut host, "stuck");
    let (stdout, _, exit) = run_cli(&mut host, &["panes"], b"");
    let all = records(&stdout);
    let warnings = of_type(&all, "warning");
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0]["code"], "session-unreachable");
    let summary = summary(&all);
    assert_eq!(summary["ok"], false);
    assert_eq!(summary["unreachable_sessions"], 1);
    assert_eq!(exit, Some(0));
}

/// REQ: F-109
#[test]
fn panes_109_an_unreachable_session_does_not_hide_the_panes_of_the_others() {
    let mut host = server_host();
    two_sessions(&mut host);
    add_unreachable_session(&mut host, "stuck");
    let (stdout, _, _) = run_cli(&mut host, &["panes"], b"");
    assert_eq!(pane_ids(&records(&stdout)).len(), 4);
}

/// REQ: F-109
#[test]
fn panes_109_selection_by_agent_keeps_only_that_agent() {
    let mut host = server_host();
    two_sessions(&mut host);
    let (stdout, _, _) = run_cli(&mut host, &["panes", "--agent", "codex"], b"");
    assert_eq!(pane_ids(&records(&stdout)), [("default".to_string(), 5)]);
}

/// REQ: F-109
#[test]
fn panes_109_selection_by_state_keeps_only_that_state() {
    let mut host = server_host();
    two_sessions(&mut host);
    let (stdout, _, _) = run_cli(&mut host, &["panes", "--state", "waiting-approval"], b"");
    assert_eq!(pane_ids(&records(&stdout)), [("review".to_string(), 2)]);
}

/// REQ: F-109
#[test]
fn panes_109_options_narrow_with_and_and_repeated_values_widen_with_or() {
    let mut host = server_host();
    two_sessions(&mut host);
    let (narrow, _, _) = run_cli(&mut host, &["panes", "--agent", "claude", "--state", "working"], b"");
    assert_eq!(pane_ids(&records(&narrow)), [("default".to_string(), 3)]);
    let (wide, _, _) = run_cli(&mut host, &["panes", "--state", "working", "--state", "idle"], b"");
    assert_eq!(pane_ids(&records(&wide)).len(), 2);
    let (comma, _, _) = run_cli(&mut host, &["panes", "--state", "working,idle"], b"");
    assert_eq!(pane_ids(&records(&comma)).len(), 2);
}

/// REQ: F-109
#[test]
fn panes_109_selection_by_session_pane_and_kind() {
    let mut host = server_host();
    two_sessions(&mut host);
    let (by_session, _, _) = run_cli(&mut host, &["panes", "--session", "review"], b"");
    assert_eq!(pane_ids(&records(&by_session)).len(), 2);
    let (by_pane, _, _) = run_cli(&mut host, &["panes", "--session", "review", "--pane", "4"], b"");
    assert_eq!(pane_ids(&records(&by_pane)), [("review".to_string(), 4)]);
    let (by_kind, _, _) = run_cli(&mut host, &["panes", "--kind", "agent"], b"");
    assert_eq!(pane_ids(&records(&by_kind)).len(), 3);
}

/// REQ: F-109
#[test]
fn panes_109_match_is_a_case_insensitive_substring_over_the_searched_fields() {
    let mut host = server_host();
    two_sessions(&mut host);
    let (stdout, _, _) = run_cli(&mut host, &["panes", "--match", "REFACTOR"], b"");
    assert_eq!(pane_ids(&records(&stdout)), [("default".to_string(), 3)]);
}

/// REQ: F-109
#[test]
fn panes_109_field_limits_the_text_search() {
    let mut host = server_host();
    two_sessions(&mut host);
    let (in_name, _, _) = run_cli(&mut host, &["panes", "--match", "codex", "--field", "name"], b"");
    assert!(pane_ids(&records(&in_name)).is_empty());
    let (in_agent, _, _) = run_cli(&mut host, &["panes", "--match", "codex", "--field", "agent"], b"");
    assert_eq!(pane_ids(&records(&in_agent)), [("default".to_string(), 5)]);
}

/// REQ: F-109
#[test]
fn panes_109_regex_selects_and_invert_flips_only_the_text_filter() {
    let mut host = server_host();
    two_sessions(&mut host);
    let (hit, _, _) = run_cli(&mut host, &["panes", "--regex", "^(tests|shell)$", "--field", "name"], b"");
    assert_eq!(pane_ids(&records(&hit)).len(), 2);
    let (inverted, _, _) = run_cli(&mut host, &["panes", "--regex", "^(tests|shell)$", "--field", "name", "--invert"], b"");
    assert_eq!(pane_ids(&records(&inverted)).len(), 2);
    let (inverted_and_kind, _, _) =
        run_cli(&mut host, &["panes", "--regex", "^(tests|shell)$", "--field", "name", "--invert", "--kind", "agent"], b"");
    assert_eq!(pane_ids(&records(&inverted_and_kind)).len(), 2);
}

/// REQ: F-109
#[test]
fn panes_109_an_invalid_regex_is_an_invalid_selection_with_exit_two() {
    let mut host = server_host();
    two_sessions(&mut host);
    let (stdout, _, exit) = run_cli(&mut host, &["panes", "--regex", "("], b"");
    let all = records(&stdout);
    assert_eq!(of_type(&all, "error")[0]["code"], "invalid-selection");
    assert_eq!(exit, Some(2));
}

/// REQ: F-109
#[test]
fn panes_109_invert_without_a_pattern_is_an_invalid_selection_with_exit_two() {
    let mut host = server_host();
    two_sessions(&mut host);
    let (stdout, _, exit) = run_cli(&mut host, &["panes", "--invert"], b"");
    assert_eq!(of_type(&records(&stdout), "error")[0]["code"], "invalid-selection");
    assert_eq!(exit, Some(2));
}

/// REQ: F-109
#[test]
fn panes_109_an_empty_selection_prints_a_summary_with_zero_panes() {
    let mut host = server_host();
    two_sessions(&mut host);
    let (stdout, _, _) = run_cli(&mut host, &["panes", "--match", "no-such-pane-anywhere"], b"");
    let all = records(&stdout);
    assert!(of_type(&all, "pane").is_empty());
    assert_eq!(summary(&all)["panes"], 0);
}

/// REQ: F-109
#[test]
fn panes_109_the_cost_service_is_called_with_the_session_window() {
    let mut host = server_host();
    two_sessions(&mut host);
    host.stub_service("ilium.core.cost/usage", "usage", &json!({"panes": {"3": 1.42}}).to_string());
    run_cli(&mut host, &["panes"], b"");
    assert_eq!(
        host.service_calls(),
        [("ilium.core.cost/usage".to_string(), "usage".to_string(), r#"{"window":"session"}"#.to_string())]
    );
}

/// REQ: F-109
#[test]
fn panes_109_the_cost_of_a_pane_appears_in_its_record() {
    let mut host = server_host();
    two_sessions(&mut host);
    host.stub_service("ilium.core.cost/usage", "usage", &json!({"panes": {"3": 1.42, "5": 0.31}}).to_string());
    let (stdout, _, _) = run_cli(&mut host, &["panes", "--session", "default"], b"");
    assert!(stdout.contains("1.42"), "{stdout}");
    assert!(stdout.contains("0.31"), "{stdout}");
}

/// REQ: F-109
#[test]
fn panes_109_a_failing_cost_service_logs_a_warning_and_the_listing_still_succeeds() {
    let mut host = server_host();
    two_sessions(&mut host);
    host.stub_service_failure("ilium.core.cost/usage", "usage", "unavailable");
    let (stdout, _, exit) = run_cli(&mut host, &["panes"], b"");
    assert_eq!(exit, Some(0));
    assert_eq!(pane_ids(&records(&stdout)).len(), 4);
    assert!(host.logs().iter().any(|line| line.level == "warn" && line.message.starts_with("cost service failed")));
}

/// REQ: F-109
#[test]
fn panes_109_the_listing_works_without_any_cost_figure() {
    let mut host = server_host();
    two_sessions(&mut host);
    host.stub_service("ilium.core.cost/usage", "usage", r#"{"panes":{}}"#);
    let (stdout, _, exit) = run_cli(&mut host, &["panes"], b"");
    assert_eq!(exit, Some(0));
    assert_eq!(pane_ids(&records(&stdout)).len(), 4);
}

/// REQ: F-109, COR-040
#[test]
fn panes_040_the_output_is_json_lines_only_when_not_on_a_terminal() {
    let mut host = server_host();
    two_sessions(&mut host);
    let (stdout, stderr, _) = run_cli(&mut host, &["panes"], b"");
    assert!(stdout.lines().all(|line| serde_json::from_str::<Value>(line).is_ok()), "{stdout}");
    assert!(!stderr.contains("SESSION"), "no table on stderr: {stderr}");
}

/// REQ: F-109
#[test]
fn panes_109_the_invocation_name_is_panes_and_arguments_are_kept() {
    let mut host = server_host();
    two_sessions(&mut host);
    let invocation = host.cli().invoke(&["panes", "--agent", "claude"], b"");
    assert_eq!(invocation.name, "panes");
    assert_eq!(invocation.arguments, ["--agent", "claude"]);
}

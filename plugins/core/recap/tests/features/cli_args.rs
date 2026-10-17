//! Argument handling of `ilium recap` (F-112): option values, ranges, selection errors, kinds.
//! The consent gate is closed (no kernel setting) in these tests, so no model is involved.

use crate::common::*;

fn run(args: &[&str]) -> (String, String, Option<u8>) {
    let mut host = activated_server();
    host.tree().add(agent_pane("refactor parser", "claude"));
    host.tree().add(shell_pane("build shell"));
    let mut full = vec!["recap"];
    full.extend_from_slice(args);
    run_cli(&mut host, &full)
}

/// REQ: F-112
#[test]
fn f_112_length_medium_is_refused_exit_two_with_the_documented_message() {
    let (stdout, stderr, code) = run(&["--length", "medium"]);
    assert_eq!(code, Some(2));
    assert_eq!(
        stderr.trim_end(),
        "recap: --length must be short, long or both"
    );
    assert!(stdout.is_empty());
}

/// REQ: F-112
#[test]
fn f_112_length_values_are_case_sensitive_and_the_empty_value_is_refused_exit_two() {
    for value in ["", "SHORT", "Long", "both ", "all"] {
        let (_, stderr, code) = run(&["--length", value]);
        assert_eq!(code, Some(2), "value {value:?}");
        assert_eq!(
            stderr.trim_end(),
            "recap: --length must be short, long or both",
            "value {value:?}"
        );
    }
}

/// REQ: F-112
#[test]
fn f_112_length_short_long_and_both_are_accepted() {
    for value in ["short", "long", "both"] {
        let (_, stderr, code) = run(&["--length", value]);
        assert_eq!(code, Some(0), "value {value}");
        assert!(!stderr.contains("--length must be"), "value {value}");
    }
}

/// REQ: F-112
#[test]
fn f_112_length_defaults_to_both_so_no_argument_is_a_finished_run() {
    let (stdout, _, code) = run(&[]);
    assert_eq!(code, Some(0));
    assert_eq!(summary(&jsonl(&stdout))["type"], "summary");
}

/// REQ: F-112
#[test]
fn f_112_length_short_leaves_the_long_value_null() {
    let (stdout, _, _) = run(&["--length", "short"]);
    let records = jsonl(&stdout);
    let recaps = of_type(&records, "recap");
    assert!(!recaps.is_empty());
    for record in recaps {
        assert!(
            record["long"].is_null(),
            "long must be null for --length short: {record}"
        );
    }
}

/// REQ: F-112
#[test]
fn f_112_format_yaml_is_refused_exit_two() {
    let (_, _, code) = run(&["--format", "yaml"]);
    assert_eq!(code, Some(2));
}

/// REQ: F-112
#[test]
fn f_112_format_jsonl_and_markdown_are_accepted() {
    for value in ["jsonl", "markdown"] {
        let (_, _, code) = run(&["--format", value]);
        assert_eq!(code, Some(0), "format {value}");
    }
}

/// REQ: F-112
#[test]
fn f_112_markdown_prints_one_section_per_pane_with_the_state_and_is_not_jsonl() {
    let (stdout, _, _) = run(&["--format", "markdown"]);
    assert!(stdout.contains("refactor parser"));
    assert!(stdout.contains("build shell"));
    assert!(stdout.contains("denied"));
    assert!(stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .any(|line| serde_json::from_str::<serde_json::Value>(line).is_err()));
}

/// Checks the boundaries of a numeric option: the two valid edges pass, the values just outside are refused.
fn check_range(option: &str, low: u32, high: u32) {
    for value in [low, high] {
        let (_, _, code) = run(&[option, &value.to_string()]);
        assert_eq!(code, Some(0), "{option} {value}");
    }
    for value in [low - 1, high + 1] {
        let (_, _, code) = run(&[option, &value.to_string()]);
        assert_eq!(code, Some(2), "{option} {value}");
    }
}

/// REQ: F-112
#[test]
fn f_112_timeout_seconds_accepts_1_to_600_and_refuses_0_and_601_exit_two() {
    check_range("--timeout-seconds", 1, 600);
}

/// REQ: F-112
#[test]
fn f_112_max_concurrency_accepts_1_to_8_and_refuses_0_and_9_exit_two() {
    check_range("--max-concurrency", 1, 8);
}

/// REQ: F-112
#[test]
fn f_112_max_panes_accepts_1_to_64_and_refuses_0_and_65_exit_two() {
    check_range("--max-panes", 1, 64);
}

/// REQ: F-112
#[test]
fn f_112_non_numeric_range_values_are_refused_exit_two() {
    for option in ["--timeout-seconds", "--max-concurrency", "--max-panes"] {
        let (_, _, code) = run(&[option, "many"]);
        assert_eq!(code, Some(2), "{option}");
    }
}

/// REQ: F-112
#[test]
fn f_112_invert_without_a_pattern_is_an_invalid_selection_error_record_exit_two() {
    let (stdout, _, code) = run(&["--invert"]);
    assert_eq!(code, Some(2));
    let records = jsonl(&stdout);
    let errors = of_type(&records, "error");
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0]["code"], "invalid-selection");
}

/// REQ: F-112
#[test]
fn f_112_invalid_regular_expression_is_an_invalid_selection_error_record_exit_two() {
    let (stdout, _, code) = run(&["--regex", "("]);
    assert_eq!(code, Some(2));
    let records = jsonl(&stdout);
    assert_eq!(of_type(&records, "error")[0]["code"], "invalid-selection");
}

/// REQ: F-112
#[test]
fn f_112_selection_error_prints_no_recap_and_no_summary_record() {
    let (stdout, _, _) = run(&["--regex", "("]);
    let records = jsonl(&stdout);
    assert!(of_type(&records, "recap").is_empty());
    assert!(of_type(&records, "summary").is_empty());
}

/// REQ: F-112
#[test]
fn f_112_kind_agent_selects_only_the_agent_pane() {
    let (stdout, _, _) = run(&["--kind", "agent"]);
    let records = jsonl(&stdout);
    let recaps = of_type(&records, "recap");
    assert_eq!(recaps.len(), 1);
    assert_eq!(recaps[0]["pane_name"], "refactor parser");
}

/// REQ: F-112
#[test]
fn f_112_kind_shell_selects_only_the_shell_pane() {
    let (stdout, _, _) = run(&["--kind", "shell"]);
    let records = jsonl(&stdout);
    let recaps = of_type(&records, "recap");
    assert_eq!(recaps.len(), 1);
    assert_eq!(recaps[0]["pane_name"], "build shell");
}

/// REQ: F-112
#[test]
fn f_112_match_filters_panes_by_name_ignoring_case() {
    let (stdout, _, _) = run(&["--match", "PARSER"]);
    let records = jsonl(&stdout);
    assert_eq!(of_type(&records, "recap").len(), 1);
    assert_eq!(summary(&records)["panes"], 1);
}

/// REQ: F-112
#[test]
fn f_112_editors_and_boards_are_skipped_even_when_selected_by_kind() {
    let mut host = activated_server();
    host.tree().add(editor_pane("notes"));
    host.tree().add(shell_pane("sh"));
    let (stdout, _, code) = run_cli(&mut host, &["recap", "--kind", "editor"]);
    assert_eq!(code, Some(0));
    assert!(of_type(&jsonl(&stdout), "recap").is_empty());
}

/// REQ: F-112
#[test]
fn f_112_default_selection_includes_agents_and_shells_but_not_editors() {
    let mut host = activated_server();
    host.tree().add(editor_pane("notes"));
    host.tree().add(shell_pane("sh"));
    host.tree().add(agent_pane("worker", "codex"));
    let (stdout, _, _) = run_cli(&mut host, &["recap"]);
    let records = jsonl(&stdout);
    assert_eq!(of_type(&records, "recap").len(), 2);
    assert_eq!(summary(&records)["panes"], 2);
}

/// REQ: F-112
#[test]
fn f_112_pane_option_keeps_only_the_named_pane_id() {
    let mut host = activated_server();
    let first = host.tree().add(shell_pane("one"));
    host.tree().add(shell_pane("two"));
    let (stdout, _, _) = run_cli(&mut host, &["recap", "--pane", &first.to_string()]);
    let records = jsonl(&stdout);
    let recaps = of_type(&records, "recap");
    assert_eq!(recaps.len(), 1);
    assert_eq!(recaps[0]["pane_id"], first.to_string());
}

/// REQ: F-112
#[test]
fn f_112_the_pane_that_runs_the_command_is_skipped() {
    let mut host = activated_server();
    let own = host.tree().add(shell_pane("mine"));
    host.tree().add(shell_pane("other"));
    host.cli().set_env("ILIUM_PANE_ID", &own.to_string());
    let (stdout, _, _) = run_cli(&mut host, &["recap"]);
    let records = jsonl(&stdout);
    let ids: Vec<String> = of_type(&records, "recap")
        .iter()
        .map(|r| r["pane_id"].as_str().expect("string").to_string())
        .collect();
    assert!(!ids.contains(&own.to_string()));
    assert_eq!(ids.len(), 1);
}

/// REQ: F-112
#[test]
fn f_112_include_self_recaps_the_pane_that_runs_the_command_too() {
    let mut host = activated_server();
    let own = host.tree().add(shell_pane("mine"));
    host.tree().add(shell_pane("other"));
    host.cli().set_env("ILIUM_PANE_ID", &own.to_string());
    let (stdout, _, _) = run_cli(&mut host, &["recap", "--include-self"]);
    let records = jsonl(&stdout);
    assert_eq!(of_type(&records, "recap").len(), 2);
}

/// REQ: F-112
#[test]
fn f_112_extra_panes_beyond_max_panes_produce_one_warning_record() {
    let mut host = activated_server();
    for index in 0..3 {
        host.tree().add(shell_pane(&format!("shell {index}")));
    }
    let (stdout, _, code) = run_cli(&mut host, &["recap", "--max-panes", "2"]);
    assert_eq!(code, Some(0));
    let records = jsonl(&stdout);
    assert_eq!(of_type(&records, "recap").len(), 2);
    let warnings = of_type(&records, "warning");
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0]["message"], "extra panes skipped");
}

/// REQ: F-112
#[test]
fn f_112_no_warning_when_the_selection_is_exactly_max_panes() {
    let mut host = activated_server();
    for index in 0..2 {
        host.tree().add(shell_pane(&format!("shell {index}")));
    }
    let (stdout, _, _) = run_cli(&mut host, &["recap", "--max-panes", "2"]);
    assert!(of_type(&jsonl(&stdout), "warning").is_empty());
}

/// REQ: F-112
#[test]
fn f_112_the_default_max_panes_is_32() {
    let mut host = activated_server();
    for index in 0..33 {
        host.tree().add(shell_pane(&format!("shell {index}")));
    }
    let (stdout, _, _) = run_cli(&mut host, &["recap"]);
    let records = jsonl(&stdout);
    assert_eq!(of_type(&records, "recap").len(), 32);
    assert_eq!(of_type(&records, "warning").len(), 1);
}

/// REQ: F-112
#[test]
fn f_112_recap_records_come_in_ascending_pane_id_order_before_the_summary() {
    let mut host = activated_server();
    for index in 0..4 {
        host.tree().add(shell_pane(&format!("shell {index}")));
    }
    let (stdout, _, _) = run_cli(&mut host, &["recap"]);
    let records = jsonl(&stdout);
    assert_eq!(records.last().expect("last")["type"], "summary");
    let ids: Vec<u64> = of_type(&records, "recap")
        .iter()
        .map(|r| {
            r["pane_id"]
                .as_str()
                .expect("string")
                .parse()
                .expect("number")
        })
        .collect();
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    assert_eq!(ids, sorted);
}

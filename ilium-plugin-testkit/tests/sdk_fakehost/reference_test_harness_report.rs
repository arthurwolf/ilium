//! reference-test-harness.md "Fixtures" (DEV-517) and "Test report records" (DEV-550 to DEV-552).
//! These rows run the `ilium plugin test` command; see `cli_support.rs`.

use crate::cli_support::{project, run_on_terminal, run_test};

const EMPTY_FIXTURE: &str = r#"{ "schema": 1 }"#;
const PASSING_EXPECT: &str = r#"{ "schema": 1, "outcomes": { "match": "exact", "items": [] } }"#;
const FAILING_EXPECT: &str =
    r#"{ "schema": 1, "outcomes": { "match": "exact", "items": ["ok"] } }"#;

fn test_case_names(run: &crate::cli_support::CliRun) -> Vec<String> {
    run.of_type("test-case")
        .iter()
        .map(|r| r["name"].as_str().unwrap_or_default().to_string())
        .collect()
}

// ---- DEV-517 ---------------------------------------------------------------------------------

/// REQ: DEV-517
#[test]
fn dev_517_every_fixture_file_becomes_a_test_case_named_fixture_colon_file() {
    let dir = project(
        "t517a",
        &[("a.json", EMPTY_FIXTURE), ("b.json", EMPTY_FIXTURE)],
    );
    let run = run_test(&dir, &["--json"]);
    let mut names = test_case_names(&run);
    names.sort();
    assert_eq!(
        names,
        vec!["fixture:a.json".to_string(), "fixture:b.json".to_string()]
    );
}

/// REQ: DEV-517
#[test]
fn dev_517_expectation_files_are_not_run_as_fixtures() {
    let dir = project(
        "t517b",
        &[("a.json", EMPTY_FIXTURE), ("a.expect.json", PASSING_EXPECT)],
    );
    let run = run_test(&dir, &["--json"]);
    assert_eq!(test_case_names(&run), vec!["fixture:a.json".to_string()]);
}

/// REQ: DEV-517
#[test]
fn dev_517_a_matching_expectation_file_passes() {
    let dir = project(
        "t517c",
        &[("a.json", EMPTY_FIXTURE), ("a.expect.json", PASSING_EXPECT)],
    );
    let run = run_test(&dir, &["--json"]);
    assert_eq!(run.code, 0, "stdout: {}", run.stdout);
    assert_eq!(run.of_type("test-case")[0]["status"], "pass");
}

/// REQ: DEV-517
#[test]
fn dev_517_a_differing_expectation_file_fails_the_case_and_exits_3() {
    let dir = project(
        "t517d",
        &[("a.json", EMPTY_FIXTURE), ("a.expect.json", FAILING_EXPECT)],
    );
    let run = run_test(&dir, &["--json"]);
    assert_eq!(run.code, 3, "stdout: {}", run.stdout);
    assert_eq!(run.of_type("test-case")[0]["status"], "fail");
}

/// REQ: DEV-517
#[test]
fn dev_517_a_project_with_no_fixture_has_no_test_case() {
    let dir = project("t517e", &[]);
    let run = run_test(&dir, &["--json"]);
    assert!(run.of_type("test-case").is_empty());
}

// ---- DEV-550 ---------------------------------------------------------------------------------

/// REQ: DEV-550
#[test]
fn dev_550_names_are_the_same_in_two_runs() {
    let dir = project(
        "t550a",
        &[
            ("a.json", EMPTY_FIXTURE),
            ("b.json", EMPTY_FIXTURE),
            ("c.json", EMPTY_FIXTURE),
        ],
    );
    let first = test_case_names(&run_test(&dir, &["--json"]));
    let second = test_case_names(&run_test(&dir, &["--json"]));
    assert_eq!(first, second);
    assert_eq!(first.len(), 3);
}

/// REQ: DEV-550
#[test]
fn dev_550_a_test_case_record_lists_name_status_and_duration_in_this_order() {
    let dir = project("t550b", &[("a.json", EMPTY_FIXTURE)]);
    let run = run_test(&dir, &["--json"]);
    let line = run
        .record_lines()
        .into_iter()
        .find(|line| line.contains("\"test-case\""))
        .expect("a test-case line");
    let name = line.find("\"name\"").expect("name");
    let status = line.find("\"status\"").expect("status");
    let duration = line.find("\"duration_ms\"").expect("duration_ms");
    assert!(name < status && status < duration, "line was {line}");
}

/// REQ: DEV-550
#[test]
fn dev_550_names_do_not_contain_a_time_or_a_random_part() {
    let dir = project("t550c", &[("a.json", EMPTY_FIXTURE)]);
    let names = test_case_names(&run_test(&dir, &["--json"]));
    assert_eq!(names, vec!["fixture:a.json".to_string()]);
}

// ---- DEV-551 ---------------------------------------------------------------------------------

/// REQ: DEV-551
#[test]
fn dev_551_a_fixture_without_an_expectation_file_passes() {
    let dir = project("t551a", &[("a.json", EMPTY_FIXTURE)]);
    let run = run_test(&dir, &["--json"]);
    assert_eq!(run.code, 0, "stdout: {}", run.stdout);
    assert_eq!(run.of_type("test-case")[0]["status"], "pass");
}

/// REQ: DEV-551
#[test]
fn dev_551_the_warning_test_no_expectation_is_printed() {
    let dir = project("t551b", &[("a.json", EMPTY_FIXTURE)]);
    let run = run_test(&dir, &["--json"]);
    let warnings = run.of_type("warning");
    assert!(
        warnings.iter().any(|w| w["code"] == "test.no-expectation"),
        "stdout: {}",
        run.stdout
    );
}

/// REQ: DEV-551
#[test]
fn dev_551_a_fixture_with_an_expectation_file_gets_no_such_warning() {
    let dir = project(
        "t551c",
        &[("a.json", EMPTY_FIXTURE), ("a.expect.json", PASSING_EXPECT)],
    );
    let run = run_test(&dir, &["--json"]);
    assert!(!run
        .of_type("warning")
        .iter()
        .any(|w| w["code"] == "test.no-expectation"));
}

/// REQ: DEV-551
#[test]
fn dev_551_one_warning_per_fixture_without_an_expectation() {
    let dir = project(
        "t551d",
        &[("a.json", EMPTY_FIXTURE), ("b.json", EMPTY_FIXTURE)],
    );
    let run = run_test(&dir, &["--json"]);
    let count = run
        .of_type("warning")
        .iter()
        .filter(|w| w["code"] == "test.no-expectation")
        .count();
    assert_eq!(count, 2);
}

// ---- DEV-552 ---------------------------------------------------------------------------------

/// REQ: DEV-552
#[test]
fn dev_552_piped_stdout_prints_only_json_lines() {
    let dir = project("t552a", &[("a.json", EMPTY_FIXTURE)]);
    let run = run_test(&dir, &[]);
    assert!(!run.stdout.trim().is_empty());
    for line in run.stdout.lines() {
        serde_json::from_str::<serde_json::Value>(line)
            .unwrap_or_else(|_| panic!("not JSON: {line}"));
    }
}

/// REQ: DEV-552
#[test]
fn dev_552_the_json_flag_gives_the_same_records_as_a_pipe() {
    let dir = project("t552b", &[("a.json", EMPTY_FIXTURE)]);
    let piped = run_test(&dir, &[]);
    let flagged = run_test(&dir, &["--json"]);
    let kinds = |run: &crate::cli_support::CliRun| -> Vec<String> {
        run.records()
            .iter()
            .map(|r| r["type"].as_str().unwrap_or_default().to_string())
            .collect()
    };
    assert_eq!(kinds(&piped), kinds(&flagged));
}

/// REQ: DEV-552
#[test]
fn dev_552_the_json_summary_record_has_passed_failed_skipped_and_duration() {
    let dir = project("t552c", &[("a.json", EMPTY_FIXTURE)]);
    let run = run_test(&dir, &["--json"]);
    let summary = &run.of_type("summary")[0];
    assert_eq!(summary["passed"], 1);
    assert_eq!(summary["failed"], 0);
    assert_eq!(summary["skipped"], 0);
    assert!(summary["duration_ms"].is_u64());
}

/// REQ: DEV-552
#[test]
fn dev_552_a_terminal_gets_one_line_per_test_and_the_summary_line() {
    let dir = project(
        "t552d",
        &[("a.json", EMPTY_FIXTURE), ("b.json", EMPTY_FIXTURE)],
    );
    let run = run_on_terminal(&dir, &[]);
    let lines: Vec<&str> = run
        .stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    let last = lines.last().expect("some output");
    assert!(
        last.starts_with("summary: 2 passed, 0 failed, 0 skipped ("),
        "last line was {last}"
    );
    assert!(last.ends_with(" ms)"), "last line was {last}");
    assert!(
        lines.iter().all(|line| !line.starts_with('{')),
        "no JSON on a terminal"
    );
}

/// REQ: DEV-552
#[test]
fn dev_552_the_terminal_summary_counts_a_failing_case() {
    let dir = project(
        "t552e",
        &[("a.json", EMPTY_FIXTURE), ("a.expect.json", FAILING_EXPECT)],
    );
    let run = run_on_terminal(&dir, &[]);
    let last = run
        .stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .last()
        .expect("output")
        .to_string();
    assert!(
        last.starts_with("summary: 0 passed, 1 failed, 0 skipped ("),
        "last line was {last}"
    );
}

/// REQ: DEV-552
#[test]
fn dev_552_the_json_flag_wins_on_a_terminal() {
    let dir = project("t552f", &[("a.json", EMPTY_FIXTURE)]);
    let run = run_on_terminal(&dir, &["--json"]);
    assert!(run.stdout.lines().any(|line| line.starts_with('{')));
    assert!(!run.stdout.lines().any(|line| line.starts_with("summary:")));
}

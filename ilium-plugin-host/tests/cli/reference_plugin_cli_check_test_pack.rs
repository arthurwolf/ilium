//! reference-plugin-cli.md: `check`, `lint`, `test` and `pack` (DEV-070..102).

use std::sync::atomic::Ordering;
use std::sync::Arc;

use ilium_plugin_host::package::read_package;
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::common::*;

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn empty_description(dir: &std::path::Path) {
    let mut manifest = read_json(&dir.join("manifest.json"));
    manifest["description"] = json!("");
    write_json(&dir.join("manifest.json"), &manifest);
}

fn package_path(dir: &std::path::Path) -> std::path::PathBuf {
    dir.join("dist/acme.demo-0.1.0.iliumpkg")
}

// ---------------------------------------------------------------- check

/// REQ: DEV-070
#[test]
fn dev_070_check_reports_only_known_rule_families() {
    let root = scratch();
    let fetcher = Arc::new(CountingFetcher::default());
    let mut env = env(root.path());
    env.fetcher = fetcher.clone();
    let dir = server_project(&env, root.path());
    std::fs::write(dir.join("manifest.json"), "{ \"id\": 7 }").expect("write");
    let output = run_in(&env, &dir, &["check", "--json"]);
    let rules: Vec<String> = recs(&output, "check")
        .iter()
        .map(|r| r["rule"].as_str().expect("rule").to_owned())
        .collect();
    assert!(!rules.is_empty());
    for rule in &rules {
        assert!(
            [
                "manifest.",
                "package.",
                "signature.",
                "component.",
                "check."
            ]
            .iter()
            .any(|p| rule.starts_with(p)),
            "{rule}"
        );
    }
    assert_eq!(fetcher.calls.load(Ordering::SeqCst), 0, "offline");
}

/// REQ: DEV-070
#[test]
fn dev_070_check_includes_component_rules() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    let output = run_in(&env, &dir, &["check", "--json"]);
    assert!(recs(&output, "check")
        .iter()
        .any(|r| r["rule"].as_str().expect("rule").starts_with("component.")));
}

/// REQ: DEV-070
#[test]
fn dev_070_manifest_fault_gets_a_manifest_code() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    write_dist(&dir, &["server"], "release");
    std::fs::write(dir.join("manifest.json"), "{ not json").expect("write");
    let output = run_in(&env, &dir, &["check", "--json"]);
    assert!(recs(&output, "check")
        .iter()
        .any(|r| r["rule"].as_str().expect("rule").starts_with("manifest.")));
}

/// REQ: DEV-071
#[test]
fn dev_071_missing_dist_is_a_component_missing_finding() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    let output = run_in(&env, &dir, &["check", "--json"]);
    assert!(recs(&output, "check")
        .iter()
        .any(|r| r["rule"] == "component.missing"));
}

/// REQ: DEV-071
#[test]
fn dev_071_missing_dist_is_not_an_error_record_of_the_command() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    let output = run_in(&env, &dir, &["check", "--json"]);
    assert!(recs(&output, "error").is_empty());
    assert_eq!(recs(&output, "summary")[0]["errors"], 0);
}

/// REQ: DEV-071
#[test]
fn dev_071_a_built_dist_removes_the_component_finding() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    run_in(&env, &dir, &["build"]);
    let output = run_in(&env, &dir, &["check", "--json"]);
    assert!(recs(&output, "check")
        .iter()
        .all(|r| r["rule"] != "component.missing"));
}

/// REQ: DEV-072
#[test]
fn dev_072_check_of_a_package_file_passes_a_valid_unsigned_package() {
    let root = scratch();
    let env = env(root.path());
    let (_, package) = packed_project(&env, root.path());
    let output = run(&env, &["check", package.to_str().expect("utf8"), "--json"]);
    assert_eq!(output.exit_code, 0, "{:?}", output.stdout_jsonl);
}

/// REQ: DEV-072
#[test]
fn dev_072_check_of_a_package_does_not_look_at_trust() {
    let root = scratch();
    let env = env(root.path());
    let (_, package) = packed_project(&env, root.path());
    let (key, _) = make_key(&env, root.path(), "acme");
    run(
        &env,
        &[
            "sign",
            package.to_str().expect("utf8"),
            "--key",
            key.to_str().expect("utf8"),
        ],
    );
    let output = run(&env, &["check", package.to_str().expect("utf8"), "--json"]);
    assert!(recs(&output, "check")
        .iter()
        .all(|r| !r["rule"].as_str().expect("rule").starts_with("trust.")));
    assert_eq!(output.exit_code, 0);
}

/// REQ: DEV-072
#[test]
fn dev_072_check_of_a_damaged_package_fails_with_findings() {
    let root = scratch();
    let env = env(root.path());
    let (_, package) = packed_project(&env, root.path());
    let bytes = std::fs::read(&package).expect("read");
    std::fs::write(&package, &bytes[..bytes.len() / 2]).expect("write");
    let output = run(&env, &["check", package.to_str().expect("utf8"), "--json"]);
    assert_eq!(output.exit_code, 3);
}

/// REQ: DEV-073
#[test]
fn dev_073_an_error_finding_exits_three() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    let output = run_in(&env, &dir, &["check", "--json"]);
    assert!(recs(&output, "check")
        .iter()
        .any(|r| r["severity"] == "error"));
    assert_eq!(output.exit_code, 3);
    assert_eq!(result(&output)["exit_code"], 3);
}

/// REQ: DEV-073
#[test]
fn dev_073_only_warn_findings_exit_zero() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    run_in(&env, &dir, &["build"]);
    std::fs::OpenOptions::new()
        .append(true)
        .open(dir.join("src/lib.rs"))
        .and_then(|mut file| {
            std::io::Write::write_all(&mut file, b"\n// changed after the build\n")
        })
        .expect("append");
    let output = run_in(&env, &dir, &["check", "--json"]);
    assert!(recs(&output, "check")
        .iter()
        .any(|r| r["rule"] == "check.stale-build" && r["severity"] == "warn"));
    assert_eq!(output.exit_code, 0);
}

// ---------------------------------------------------------------- lint

/// REQ: DEV-075
#[test]
fn dev_075_lint_reports_only_lint_rules() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    empty_description(&dir);
    let output = run_in(&env, &dir, &["lint", "--json"]);
    assert!(!recs(&output, "lint").is_empty());
    assert!(recs(&output, "check").is_empty());
    for record in recs(&output, "lint") {
        assert!(
            record["rule"].as_str().expect("rule").starts_with("lint."),
            "{record}"
        );
    }
}

/// REQ: DEV-075
#[test]
fn dev_075_lint_runs_offline() {
    let root = scratch();
    let fetcher = Arc::new(CountingFetcher::default());
    let mut env = env(root.path());
    env.fetcher = fetcher.clone();
    let dir = server_project(&env, root.path());
    run_in(&env, &dir, &["lint", "--json"]);
    assert_eq!(fetcher.calls.load(Ordering::SeqCst), 0);
}

/// REQ: DEV-076
#[test]
fn dev_076_deny_warnings_turns_a_warn_into_exit_three() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    empty_description(&dir);
    assert_eq!(run_in(&env, &dir, &["lint", "--json"]).exit_code, 0);
    assert_eq!(
        run_in(&env, &dir, &["lint", "--json", "--deny", "warnings"]).exit_code,
        3
    );
}

/// REQ: DEV-076
#[test]
fn dev_076_deny_warnings_with_no_finding_exits_zero() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    assert_eq!(
        run_in(&env, &dir, &["lint", "--json", "--deny", "warnings"]).exit_code,
        0
    );
}

/// REQ: DEV-077
#[test]
fn dev_077_rule_restricts_the_run_to_the_named_rule() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    empty_description(&dir);
    std::fs::remove_file(dir.join("README.md")).expect("remove");
    let output = run_in(
        &env,
        &dir,
        &["lint", "--json", "--rule", "lint.description-missing"],
    );
    let rules: Vec<&str> = recs(&output, "lint")
        .iter()
        .map(|r| r["rule"].as_str().expect("rule"))
        .collect();
    assert_eq!(rules, ["lint.description-missing"]);
}

/// REQ: DEV-077
#[test]
fn dev_077_rule_is_repeatable() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    empty_description(&dir);
    std::fs::remove_file(dir.join("README.md")).expect("remove");
    let output = run_in(
        &env,
        &dir,
        &[
            "lint",
            "--json",
            "--rule",
            "lint.description-missing",
            "--rule",
            "lint.readme-missing",
        ],
    );
    let mut rules: Vec<&str> = recs(&output, "lint")
        .iter()
        .map(|r| r["rule"].as_str().expect("rule"))
        .collect();
    rules.sort();
    assert_eq!(rules, ["lint.description-missing", "lint.readme-missing"]);
}

/// REQ: DEV-077
#[test]
fn dev_077_unknown_rule_fails_invalid_value() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    let output = run_in(
        &env,
        &dir,
        &["lint", "--json", "--rule", "lint.no-such-rule"],
    );
    assert_eq!(code(&output).as_deref(), Some("usage.invalid-value"));
    assert_eq!(output.exit_code, 2);
}

/// REQ: DEV-078
#[test]
fn dev_078_suppressed_finding_is_printed_with_suppressed_true() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    empty_description(&dir);
    std::fs::write(
        dir.join("lint.toml"),
        "[[suppress]]\nrule = \"lint.description-missing\"\nreason = \"on purpose\"\n",
    )
    .expect("write");
    let output = run_in(&env, &dir, &["lint", "--json"]);
    let found: Vec<_> = recs(&output, "lint")
        .into_iter()
        .filter(|r| r["rule"] == "lint.description-missing")
        .collect();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0]["suppressed"], true);
}

/// REQ: DEV-078
#[test]
fn dev_078_suppressed_finding_does_not_count_toward_the_exit_code() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    empty_description(&dir);
    std::fs::write(
        dir.join("lint.toml"),
        "[[suppress]]\nrule = \"lint.description-missing\"\nreason = \"on purpose\"\n",
    )
    .expect("write");
    assert_eq!(
        run_in(&env, &dir, &["lint", "--json", "--deny", "warnings"]).exit_code,
        0
    );
}

/// REQ: DEV-078
#[test]
fn dev_078_unsuppressed_findings_have_suppressed_false() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    empty_description(&dir);
    let output = run_in(&env, &dir, &["lint", "--json"]);
    assert!(recs(&output, "lint")
        .iter()
        .all(|r| r["suppressed"] == false));
}

// ---------------------------------------------------------------- test

fn tested_project(
    env: &ilium_plugin_host::cli::CliEnv,
    root: &std::path::Path,
) -> std::path::PathBuf {
    let dir = server_project(env, root);
    run_in(env, &dir, &["build"]);
    dir
}

fn write_fixture(
    dir: &std::path::Path,
    name: &str,
    fixture: serde_json::Value,
    expect: Option<serde_json::Value>,
) {
    let fixtures = dir.join("tests/fixtures");
    std::fs::create_dir_all(&fixtures).expect("create");
    write_json(&fixtures.join(format!("{name}.json")), &fixture);
    let expect_path = fixtures.join(format!("{name}.expect.json"));
    match expect {
        Some(value) => write_json(&expect_path, &value),
        None => {
            let _ = std::fs::remove_file(expect_path);
        }
    }
}

fn clear_fixtures(dir: &std::path::Path) {
    let _ = std::fs::remove_dir_all(dir.join("tests/fixtures"));
}

const MATCHING: fn() -> serde_json::Value = || json!({"schema": 1, "submissions": {"match": "subset", "items": [{"intent": "set-decoration", "slot": "work"}]}});

/// REQ: DEV-080
#[test]
fn dev_080_no_component_fails_component_missing() {
    let root = scratch();
    let env = env(root.path()).with_runner(ScriptedRunner::passing());
    let dir = server_project(&env, root.path());
    let output = run_in(&env, &dir, &["test"]);
    assert_eq!(code(&output).as_deref(), Some("test.component-missing"));
    assert_eq!(output.exit_code, 1);
}

/// REQ: DEV-080
#[test]
fn dev_080_a_built_component_is_run_in_component_mode() {
    let root = scratch();
    let env = env(root.path()).with_runner(ScriptedRunner::passing());
    let dir = tested_project(&env, root.path());
    let output = run_in(&env, &dir, &["test"]);
    assert_eq!(output.exit_code, 0);
    let cases = recs(&output, "test-case");
    assert!(!cases.is_empty());
    assert!(cases
        .iter()
        .all(|r| r["status"] == "pass" && r["side"] == "server"));
    assert!(recs(&output, "error")
        .iter()
        .all(|r| r["code"] != "test.component-missing"));
}

/// REQ: DEV-081
#[test]
fn dev_081_submissions_and_outcomes_come_before_the_test_case_record() {
    let root = scratch();
    let env = env(root.path()).with_runner(ScriptedRunner::passing());
    let dir = tested_project(&env, root.path());
    clear_fixtures(&dir);
    write_fixture(
        &dir,
        "one",
        json!({"schema": 1, "side": "server"}),
        Some(MATCHING()),
    );
    let output = run_in(&env, &dir, &["test"]);
    let kinds: Vec<String> = types(&output)
        .into_iter()
        .filter(|k| ["submission", "outcome", "test-case"].contains(&k.as_str()))
        .collect();
    assert_eq!(kinds, ["submission", "outcome", "test-case"]);
}

/// REQ: DEV-081
#[test]
fn dev_081_one_test_case_per_fixture_named_after_the_file() {
    let root = scratch();
    let env = env(root.path()).with_runner(ScriptedRunner::passing());
    let dir = tested_project(&env, root.path());
    clear_fixtures(&dir);
    write_fixture(
        &dir,
        "one",
        json!({"schema": 1, "side": "server"}),
        Some(MATCHING()),
    );
    write_fixture(
        &dir,
        "two",
        json!({"schema": 1, "side": "server"}),
        Some(MATCHING()),
    );
    let output = run_in(&env, &dir, &["test"]);
    let names: Vec<&str> = recs(&output, "test-case")
        .iter()
        .map(|r| r["name"].as_str().expect("name"))
        .collect();
    assert_eq!(names, ["fixture:one.json", "fixture:two.json"]);
}

/// REQ: DEV-082
#[test]
fn dev_082_a_matching_expectation_passes() {
    let root = scratch();
    let env = env(root.path()).with_runner(ScriptedRunner::passing());
    let dir = tested_project(&env, root.path());
    clear_fixtures(&dir);
    write_fixture(
        &dir,
        "one",
        json!({"schema": 1, "side": "server"}),
        Some(MATCHING()),
    );
    let output = run_in(&env, &dir, &["test"]);
    assert_eq!(recs(&output, "test-case")[0]["status"], "pass");
    assert!(recs(&output, "diff").is_empty());
    assert_eq!(output.exit_code, 0);
}

/// REQ: DEV-082
#[test]
fn dev_082_a_mismatch_prints_a_diff_fails_the_case_and_exits_three() {
    let root = scratch();
    let env = env(root.path()).with_runner(ScriptedRunner::passing());
    let dir = tested_project(&env, root.path());
    clear_fixtures(&dir);
    let wrong = json!({"schema": 1, "submissions": {"match": "subset", "items": [{"intent": "clear-decoration"}]}});
    write_fixture(
        &dir,
        "one",
        json!({"schema": 1, "side": "server"}),
        Some(wrong),
    );
    let output = run_in(&env, &dir, &["test"]);
    assert_eq!(recs(&output, "diff").len(), 1);
    assert_eq!(recs(&output, "test-case")[0]["status"], "fail");
    assert_eq!(output.exit_code, 3);
}

/// REQ: DEV-082
#[test]
fn dev_082_exact_mode_fails_on_a_count_mismatch() {
    let root = scratch();
    let env = env(root.path()).with_runner(ScriptedRunner::passing());
    let dir = tested_project(&env, root.path());
    clear_fixtures(&dir);
    let exact = json!({"schema": 1, "submissions": {"match": "exact", "items": []}});
    write_fixture(
        &dir,
        "one",
        json!({"schema": 1, "side": "server"}),
        Some(exact),
    );
    let output = run_in(&env, &dir, &["test"]);
    assert_eq!(recs(&output, "test-case")[0]["status"], "fail");
}

/// REQ: DEV-083
#[test]
fn dev_083_no_expectation_file_passes_with_a_warning() {
    let root = scratch();
    let env = env(root.path()).with_runner(ScriptedRunner::passing());
    let dir = tested_project(&env, root.path());
    clear_fixtures(&dir);
    write_fixture(&dir, "one", json!({"schema": 1, "side": "server"}), None);
    let output = run_in(&env, &dir, &["test"]);
    assert_eq!(recs(&output, "test-case")[0]["status"], "pass");
    assert_eq!(warning_codes(&output), ["test.no-expectation"]);
    assert_eq!(output.exit_code, 0);
}

/// REQ: DEV-083
#[test]
fn dev_083_no_expectation_file_still_fails_on_a_trap() {
    let root = scratch();
    let env = env(root.path()).with_runner(ScriptedRunner::trapping("unreachable"));
    let dir = tested_project(&env, root.path());
    clear_fixtures(&dir);
    write_fixture(&dir, "one", json!({"schema": 1, "side": "server"}), None);
    let output = run_in(&env, &dir, &["test"]);
    assert_eq!(recs(&output, "test-case")[0]["status"], "trap");
}

/// REQ: DEV-084
#[test]
fn dev_084_a_trap_sets_status_trap_with_the_reason_and_exits_three() {
    let root = scratch();
    let env = env(root.path()).with_runner(ScriptedRunner::trapping("unreachable"));
    let dir = tested_project(&env, root.path());
    clear_fixtures(&dir);
    write_fixture(
        &dir,
        "one",
        json!({"schema": 1, "side": "server"}),
        Some(MATCHING()),
    );
    let output = run_in(&env, &dir, &["test"]);
    let case = recs(&output, "test-case")[0];
    assert_eq!(case["status"], "trap");
    assert!(case["reason"]
        .as_str()
        .expect("reason")
        .contains("unreachable"));
    assert_eq!(output.exit_code, 3);
}

/// REQ: DEV-085
#[test]
fn dev_085_two_sides_without_side_flag_fail_missing_argument() {
    let root = scratch();
    let env = env(root.path()).with_runner(ScriptedRunner::passing());
    let dir = new_project(&env, root.path(), "both", "both", "rust");
    run_in(&env, &dir, &["build"]);
    let output = run_in(&env, &dir, &["test"]);
    assert_eq!(code(&output).as_deref(), Some("usage.missing-argument"));
    assert_eq!(output.exit_code, 2);
}

/// REQ: DEV-085
#[test]
fn dev_085_two_sides_with_side_flag_run() {
    let root = scratch();
    let env = env(root.path()).with_runner(ScriptedRunner::passing());
    let dir = new_project(&env, root.path(), "both", "both", "rust");
    run_in(&env, &dir, &["build"]);
    let output = run_in(&env, &dir, &["test", "--side", "server"]);
    assert_ne!(code(&output).as_deref(), Some("usage.missing-argument"));
}

/// REQ: DEV-085
#[test]
fn dev_085_one_side_needs_no_side_flag() {
    let root = scratch();
    let env = env(root.path()).with_runner(ScriptedRunner::passing());
    let dir = tested_project(&env, root.path());
    assert_eq!(run_in(&env, &dir, &["test"]).exit_code, 0);
}

/// REQ: DEV-086
#[test]
fn dev_086_update_rewrites_only_files_that_differ_and_warns_for_each() {
    let root = scratch();
    let env = env(root.path()).with_runner(ScriptedRunner::passing());
    let dir = tested_project(&env, root.path());
    clear_fixtures(&dir);
    let wrong = json!({"schema": 1, "submissions": {"match": "exact", "items": []}});
    write_fixture(
        &dir,
        "differs",
        json!({"schema": 1, "side": "server"}),
        Some(wrong),
    );
    write_fixture(
        &dir,
        "same",
        json!({"schema": 1, "side": "server"}),
        Some(MATCHING()),
    );
    let same_before = std::fs::read(dir.join("tests/fixtures/same.expect.json")).expect("read");
    let output = run_in(&env, &dir, &["test", "--update"]);
    assert_eq!(warning_codes(&output), ["test.updated"]);
    assert_eq!(
        std::fs::read(dir.join("tests/fixtures/same.expect.json")).expect("read"),
        same_before
    );
    assert_ne!(
        read_json(&dir.join("tests/fixtures/differs.expect.json")),
        json!({"schema": 1, "submissions": {"match": "exact", "items": []}})
    );
}

/// REQ: DEV-086
#[test]
fn dev_086_update_variable_has_the_same_effect() {
    let root = scratch();
    let env = env(root.path())
        .with_runner(ScriptedRunner::passing())
        .with_var("ILIUM_PLUGIN_UPDATE_GOLDEN", "1");
    let dir = tested_project(&env, root.path());
    clear_fixtures(&dir);
    let wrong = json!({"schema": 1, "submissions": {"match": "exact", "items": []}});
    write_fixture(
        &dir,
        "differs",
        json!({"schema": 1, "side": "server"}),
        Some(wrong),
    );
    let output = run_in(&env, &dir, &["test"]);
    assert_eq!(warning_codes(&output), ["test.updated"]);
}

/// REQ: DEV-087
#[test]
fn dev_087_every_case_passing_exits_zero() {
    let root = scratch();
    let env = env(root.path()).with_runner(ScriptedRunner::passing());
    let dir = tested_project(&env, root.path());
    clear_fixtures(&dir);
    write_fixture(
        &dir,
        "one",
        json!({"schema": 1, "side": "server"}),
        Some(MATCHING()),
    );
    assert_eq!(run_in(&env, &dir, &["test"]).exit_code, 0);
}

/// REQ: DEV-087
#[test]
fn dev_087_an_unreadable_fixture_exits_one_with_fixture_invalid() {
    let root = scratch();
    let env = env(root.path()).with_runner(ScriptedRunner::passing());
    let dir = tested_project(&env, root.path());
    clear_fixtures(&dir);
    std::fs::create_dir_all(dir.join("tests/fixtures")).expect("create");
    std::fs::write(dir.join("tests/fixtures/broken.json"), "{ not json").expect("write");
    let output = run_in(&env, &dir, &["test"]);
    assert_eq!(code(&output).as_deref(), Some("test.fixture-invalid"));
    assert_eq!(output.exit_code, 1);
}

/// REQ: DEV-087
#[test]
fn dev_087_a_failed_case_exits_three_not_one() {
    let root = scratch();
    let env = env(root.path()).with_runner(ScriptedRunner::passing());
    let dir = tested_project(&env, root.path());
    clear_fixtures(&dir);
    let wrong = json!({"schema": 1, "submissions": {"match": "exact", "items": []}});
    write_fixture(
        &dir,
        "one",
        json!({"schema": 1, "side": "server"}),
        Some(wrong),
    );
    assert_eq!(run_in(&env, &dir, &["test"]).exit_code, 3);
}

/// REQ: DEV-088
#[test]
fn dev_088_record_writes_the_replay_file_and_prints_an_artifact() {
    let root = scratch();
    let env = env(root.path()).with_runner(ScriptedRunner::passing());
    let dir = tested_project(&env, root.path());
    clear_fixtures(&dir);
    write_fixture(
        &dir,
        "one",
        json!({"schema": 1, "side": "server"}),
        Some(MATCHING()),
    );
    let replay = dir.join("run.replay");
    let output = run_in(
        &env,
        &dir,
        &[
            "test",
            "--fixture",
            "tests/fixtures/one.json",
            "--record",
            replay.to_str().expect("utf8"),
        ],
    );
    assert!(replay.is_file());
    assert_eq!(artifacts(&output, "replay").len(), 1);
}

/// REQ: DEV-088 MAY
#[test]
fn dev_088_without_record_no_replay_artifact_is_printed() {
    let root = scratch();
    let env = env(root.path()).with_runner(ScriptedRunner::passing());
    let dir = tested_project(&env, root.path());
    let output = run_in(&env, &dir, &["test"]);
    assert!(artifacts(&output, "replay").is_empty());
    assert_eq!(output.exit_code, 0);
    assert!(tree_listing(&dir)
        .iter()
        .all(|name| !name.ends_with(".replay")));
}

/// REQ: DEV-089
#[test]
fn dev_089_plain_test_does_not_start_the_projects_own_tests() {
    let root = scratch();
    let runner = ScriptedRunner::passing();
    let env = env(root.path()).with_runner(runner.clone());
    let dir = tested_project(&env, root.path());
    run_in(&env, &dir, &["test"]);
    assert_eq!(runner.project_test_runs.load(Ordering::SeqCst), 0);
}

/// REQ: DEV-089
#[test]
fn dev_089_coverage_starts_them_and_prints_their_exit_status_in_the_summary() {
    let root = scratch();
    let runner = ScriptedRunner::passing();
    let env = env(root.path()).with_runner(runner.clone());
    let dir = tested_project(&env, root.path());
    let output = run_in(&env, &dir, &["test", "--coverage"]);
    assert_eq!(runner.project_test_runs.load(Ordering::SeqCst), 1);
    let details = recs(&output, "summary")[0]["details"]
        .as_object()
        .expect("summary details")
        .clone();
    assert!(
        details.values().any(|value| value == &json!(0)),
        "{details:?}"
    );
}

// ---------------------------------------------------------------- pack

/// REQ: DEV-095
#[test]
fn dev_095_stale_dist_fails_stale_build() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    write_dist(&dir, &["server"], "release");
    std::fs::write(dir.join("src/lib.rs"), "// edited after the build\n").expect("write");
    let output = run_in(&env, &dir, &["pack"]);
    assert_eq!(code(&output).as_deref(), Some("pack.stale-build"));
    assert_eq!(output.exit_code, 1);
}

/// REQ: DEV-095
#[test]
fn dev_095_missing_component_fails_not_built() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    write_dist(&dir, &["server"], "release");
    std::fs::remove_file(dir.join("dist/server.wasm")).expect("remove");
    let output = run_in(&env, &dir, &["pack"]);
    assert_eq!(code(&output).as_deref(), Some("pack.not-built"));
}

/// REQ: DEV-095
#[test]
fn dev_095_no_dist_at_all_fails_not_built() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    assert_eq!(
        code(&run_in(&env, &dir, &["pack"])).as_deref(),
        Some("pack.not-built")
    );
}

/// REQ: DEV-095
#[test]
fn dev_095_a_fresh_dist_packs() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    write_dist(&dir, &["server"], "release");
    assert_eq!(run_in(&env, &dir, &["pack"]).exit_code, 0);
}

/// REQ: DEV-096
#[test]
fn dev_096_debug_build_is_refused() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    write_dist(&dir, &["server"], "debug");
    let output = run_in(&env, &dir, &["pack"]);
    assert_eq!(code(&output).as_deref(), Some("pack.debug-build"));
    assert!(!package_path(&dir).exists());
}

/// REQ: DEV-096
#[test]
fn dev_096_allow_debug_packs_with_a_warning_of_the_same_code() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    write_dist(&dir, &["server"], "debug");
    let output = run_in(&env, &dir, &["pack", "--allow-debug"]);
    assert_eq!(output.exit_code, 0);
    assert_eq!(warning_codes(&output), ["pack.debug-build"]);
}

/// REQ: DEV-096
#[test]
fn dev_096_a_release_build_needs_no_flag_and_prints_no_warning() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    write_dist(&dir, &["server"], "release");
    assert!(warning_codes(&run_in(&env, &dir, &["pack"])).is_empty());
}

/// REQ: DEV-097
#[test]
fn dev_097_packed_manifest_lists_length_and_sha256_of_every_other_entry() {
    let root = scratch();
    let env = env(root.path());
    let (_, package) = packed_project(&env, root.path());
    let archive = read_package(&std::fs::read(package).expect("read")).expect("valid package");
    assert!(!archive.manifest.files.is_empty());
    for entry in archive
        .entries
        .iter()
        .filter(|entry| entry.path != "manifest.json" && entry.path != "signature")
    {
        let listed = archive
            .manifest
            .files
            .iter()
            .find(|file| file.path == entry.path)
            .expect("listed");
        assert_eq!(listed.length, entry.bytes.len() as u64, "{}", entry.path);
        assert_eq!(listed.sha256, sha256_hex(&entry.bytes), "{}", entry.path);
    }
}

/// REQ: DEV-097
#[test]
fn dev_097_manifest_and_signature_are_not_listed() {
    let root = scratch();
    let env = env(root.path());
    let (_, package) = packed_project(&env, root.path());
    let archive = read_package(&std::fs::read(package).expect("read")).expect("valid package");
    assert!(archive
        .manifest
        .files
        .iter()
        .all(|file| file.path != "manifest.json" && file.path != "signature"));
}

/// REQ: DEV-098
#[test]
fn dev_098_sources_are_not_stored_by_default() {
    let root = scratch();
    let env = env(root.path());
    let (_, package) = packed_project(&env, root.path());
    let archive = read_package(&std::fs::read(package).expect("read")).expect("valid package");
    assert!(archive
        .entries
        .iter()
        .all(|entry| !entry.path.starts_with("src/")));
}

/// REQ: DEV-098
#[test]
fn dev_098_include_src_stores_the_sources() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    write_dist(&dir, &["server"], "release");
    run_in(&env, &dir, &["pack", "--include-src"]);
    let archive =
        read_package(&std::fs::read(package_path(&dir)).expect("read")).expect("valid package");
    assert!(archive
        .entries
        .iter()
        .any(|entry| entry.path == "src/lib.rs"));
}

/// REQ: DEV-098
#[test]
fn dev_098_include_src_stores_source_maps_of_a_debug_build_under_src() {
    let root = scratch();
    let env = env(root.path());
    let dir = new_project(&env, root.path(), "demo", "server", "typescript");
    write_dist(&dir, &["server"], "debug");
    std::fs::write(dir.join("dist/server.wasm.map"), "{}").expect("write");
    run_in(&env, &dir, &["pack", "--include-src", "--allow-debug"]);
    let archive =
        read_package(&std::fs::read(package_path(&dir)).expect("read")).expect("valid package");
    assert!(archive
        .entries
        .iter()
        .any(|entry| entry.path.starts_with("src/") && entry.path.ends_with(".map")));
}

/// REQ: DEV-099
#[test]
fn dev_099_two_runs_produce_byte_identical_packages() {
    let root = scratch();
    let env = env(root.path());
    let (dir, package) = packed_project(&env, root.path());
    let first = std::fs::read(&package).expect("read");
    std::fs::remove_file(&package).expect("remove");
    run_in(&env, &dir, &["pack"]);
    assert_eq!(std::fs::read(&package).expect("read"), first);
}

/// REQ: DEV-099
#[test]
fn dev_099_packing_into_two_folders_gives_the_same_bytes() {
    let root = scratch();
    let env = env(root.path());
    let (dir, package) = packed_project(&env, root.path());
    let other = root.path().join("copy.iliumpkg");
    run_in(
        &env,
        &dir,
        &["pack", "--out", other.to_str().expect("utf8")],
    );
    assert_eq!(
        std::fs::read(other).expect("read"),
        std::fs::read(package).expect("read")
    );
}

/// REQ: DEV-100
#[test]
fn dev_100_default_output_path_and_one_package_artifact() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    write_dist(&dir, &["server"], "release");
    let output = run_in(&env, &dir, &["pack"]);
    let packages = artifacts(&output, "package");
    assert_eq!(packages.len(), 1);
    assert_eq!(packages[0]["path"], "dist/acme.demo-0.1.0.iliumpkg");
    assert!(package_path(&dir).is_file());
}

/// REQ: DEV-100
#[test]
fn dev_100_pack_does_not_sign() {
    let root = scratch();
    let env = env(root.path());
    let (_, package) = packed_project(&env, root.path());
    assert!(read_package(&std::fs::read(package).expect("read"))
        .expect("valid package")
        .signature
        .is_none());
}

/// REQ: DEV-100
#[test]
fn dev_100_pack_prints_no_signature_artifact() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    write_dist(&dir, &["server"], "release");
    assert!(artifacts(&run_in(&env, &dir, &["pack"]), "signature").is_empty());
}

/// REQ: DEV-101
#[test]
fn dev_101_a_package_over_300_mib_fails_package_size_limit() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    let big = vec![0xa5u8; (300 * 1024 * 1024) + 1];
    std::fs::create_dir_all(dir.join("assets")).expect("create");
    std::fs::write(dir.join("assets/blob.bin"), &big).expect("write");
    write_dist(&dir, &["server"], "release");
    let output = run_in(&env, &dir, &["pack"]);
    assert_eq!(code(&output).as_deref(), Some("package.size-limit"));
}

/// REQ: DEV-101
#[test]
fn dev_101_a_package_at_100_mib_packs_with_a_size_cap_warning() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    let big = vec![0xa5u8; 101 * 1024 * 1024];
    std::fs::create_dir_all(dir.join("assets")).expect("create");
    std::fs::write(dir.join("assets/blob.bin"), &big).expect("write");
    write_dist(&dir, &["server"], "release");
    let output = run_in(&env, &dir, &["pack"]);
    assert_eq!(output.exit_code, 0);
    assert_eq!(warning_codes(&output), ["pack.size-cap"]);
}

/// REQ: DEV-101
#[test]
fn dev_101_a_small_package_prints_no_size_warning() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    write_dist(&dir, &["server"], "release");
    assert!(warning_codes(&run_in(&env, &dir, &["pack"])).is_empty());
}

/// REQ: DEV-102
#[test]
fn dev_102_out_refuses_to_overwrite_a_file_that_is_not_a_package() {
    let root = scratch();
    let env = env(root.path());
    let dir = server_project(&env, root.path());
    write_dist(&dir, &["server"], "release");
    let target = root.path().join("notes.txt");
    std::fs::write(&target, "precious").expect("write");
    let output = run_in(
        &env,
        &dir,
        &["pack", "--out", target.to_str().expect("utf8")],
    );
    assert_eq!(code(&output).as_deref(), Some("pack.out-exists"));
    assert_eq!(std::fs::read_to_string(target).expect("read"), "precious");
}

/// REQ: DEV-102
#[test]
fn dev_102_out_may_replace_a_package_of_the_same_id() {
    let root = scratch();
    let env = env(root.path());
    let (dir, package) = packed_project(&env, root.path());
    let output = run_in(
        &env,
        &dir,
        &["pack", "--out", package.to_str().expect("utf8")],
    );
    assert_eq!(output.exit_code, 0);
}

/// REQ: DEV-102
#[test]
fn dev_102_out_refuses_a_package_of_another_id() {
    let root = scratch();
    let env = env(root.path());
    let (_, other_package) = {
        let dir = new_project(&env, root.path(), "other", "server", "rust");
        write_dist(&dir, &["server"], "release");
        run_in(&env, &dir, &["pack"]);
        (dir.clone(), dir.join("dist/acme.other-0.1.0.iliumpkg"))
    };
    let dir = server_project(&env, root.path());
    write_dist(&dir, &["server"], "release");
    let output = run_in(
        &env,
        &dir,
        &["pack", "--out", other_package.to_str().expect("utf8")],
    );
    assert_eq!(code(&output).as_deref(), Some("pack.out-exists"));
}

/// REQ: DEV-084
#[test]
fn dev_084_a_trapped_fixture_is_reported_as_trap_and_never_as_fail() {
    let root = scratch();
    let env = env(root.path()).with_runner(ScriptedRunner::trapping("unreachable"));
    let dir = tested_project(&env, root.path());
    clear_fixtures(&dir);
    write_fixture(
        &dir,
        "one",
        json!({"schema": 1, "side": "server"}),
        Some(MATCHING()),
    );
    let output = run_in(&env, &dir, &["test"]);
    let cases = recs(&output, "test-case");
    assert_eq!(cases.len(), 1, "one fixture gives one test-case record");
    assert_ne!(
        cases[0]["status"], "fail",
        "a trap must not be reported as an ordinary failure"
    );
}

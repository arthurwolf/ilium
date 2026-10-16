//! reference-test-harness.md "Coverage": DEV-530 to DEV-532.

use ilium_plugin_testkit::codes;
use ilium_plugin_testkit::coverage::{CoverageSummary, check_minimum, collect};
use proptest::prelude::*;

use crate::cli_support::run_with_env;
use crate::common::{temp_dir, with_clean_env, write_file};

fn summary(percent: f64) -> CoverageSummary {
    CoverageSummary { lines_total: 1000, lines_covered: (percent * 10.0) as u64, percent }
}

fn minimal_rust_project(label: &str) -> std::path::PathBuf {
    let dir = temp_dir(label);
    write_file(
        &dir,
        "Cargo.toml",
        "[package]\nname = \"cov-demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    write_file(&dir, "src/lib.rs", "pub fn one() -> u32 { 1 }\n#[test]\nfn t() { assert_eq!(one(), 1); }\n");
    dir
}

// ---- DEV-530 ---------------------------------------------------------------------------------

/// REQ: DEV-530
#[test]
fn dev_530_a_missing_coverage_tool_fails_with_the_tool_missing_code() {
    with_clean_env(|| {
        let dir = minimal_rust_project("c530a");
        let saved = std::env::var("PATH").ok();
        std::env::set_var("PATH", dir.join("no-such-bin"));
        let result = collect(&dir);
        match saved {
            Some(value) => std::env::set_var("PATH", value),
            None => std::env::remove_var("PATH"),
        }
        let error = result.expect_err("no cargo-llvm-cov on the path");
        assert_eq!(error.code, codes::COVERAGE_TOOL_MISSING);
        assert_eq!(error.code, "toolchain.coverage-tool-missing");
    });
}

/// REQ: DEV-530
#[test]
fn dev_530_collect_returns_line_coverage_for_a_rust_project() {
    with_clean_env(|| {
        let dir = minimal_rust_project("c530b");
        let result = collect(&dir).expect("cargo llvm-cov is installed for this test");
        assert!(result.lines_total > 0);
        assert!(result.lines_covered <= result.lines_total);
    });
}

/// REQ: DEV-530
#[test]
fn dev_530_the_command_exits_4_when_the_tool_is_missing() {
    let dir = crate::cli_support::project("c530c", &[("a.json", r#"{ "schema": 1 }"#)]);
    let run = run_with_env(&dir, &["--coverage"], &[("PATH", "/nonexistent-ilium-test-path")]);
    assert_eq!(run.code, 4, "stdout: {}", run.stdout);
    assert!(run.stdout.contains("toolchain.coverage-tool-missing"));
}

// ---- DEV-531 ---------------------------------------------------------------------------------

/// REQ: DEV-531
#[test]
fn dev_531_summary_json_lists_the_three_fields_in_order() {
    let text = serde_json::to_string(&summary(71.2)).expect("serialize");
    let total = text.find("\"lines_total\"").expect("lines_total");
    let covered = text.find("\"lines_covered\"").expect("lines_covered");
    let percent = text.find("\"percent\"").expect("percent");
    assert!(total < covered && covered < percent, "order was {text}");
}

/// REQ: DEV-531
#[test]
fn dev_531_summary_round_trips_through_json() {
    let value = summary(71.2);
    let back: CoverageSummary =
        serde_json::from_str(&serde_json::to_string(&value).expect("serialize")).expect("parse");
    assert_eq!(back, value);
}

/// REQ: DEV-531
#[test]
fn dev_531_collect_writes_lcov_and_summary_under_the_ilium_coverage_folder() {
    with_clean_env(|| {
        let dir = minimal_rust_project("c531");
        let result = collect(&dir).expect("cargo llvm-cov is installed for this test");
        let folder = dir.join(".ilium/coverage");
        assert!(folder.join("lcov.info").is_file());
        let written: CoverageSummary = serde_json::from_str(
            &std::fs::read_to_string(folder.join("summary.json")).expect("summary.json"),
        )
        .expect("summary shape");
        assert_eq!(written, result);
    });
}

// ---- DEV-532 ---------------------------------------------------------------------------------

/// REQ: DEV-532
#[test]
fn dev_532_below_the_minimum_the_message_is_exact() {
    let error = check_minimum(&summary(71.2), 80.0).expect_err("below");
    assert_eq!(error, "coverage 71.2 percent is below 80");
}

/// REQ: DEV-532
#[test]
fn dev_532_equal_to_the_minimum_passes() {
    assert_eq!(check_minimum(&summary(80.0), 80.0).err(), None);
}

/// REQ: DEV-532
#[test]
fn dev_532_just_below_the_minimum_fails() {
    let error = check_minimum(&summary(79.9), 80.0).expect_err("below");
    assert_eq!(error, "coverage 79.9 percent is below 80");
}

/// REQ: DEV-532
#[test]
fn dev_532_above_the_minimum_passes() {
    assert_eq!(check_minimum(&summary(95.5), 80.0).err(), None);
}

/// REQ: DEV-532
#[test]
fn dev_532_a_fractional_minimum_is_printed_with_its_fraction() {
    let error = check_minimum(&summary(80.0), 80.5).expect_err("below");
    assert_eq!(error, "coverage 80 percent is below 80.5");
}

/// REQ: DEV-532
#[test]
fn dev_532_the_command_exits_3_when_coverage_is_lower() {
    let dir = crate::cli_support::project("c532", &[("a.json", r#"{ "schema": 1 }"#)]);
    let run = run_with_env(&dir, &["--coverage", "--min-coverage", "101"], &[]);
    assert_eq!(run.code, 3, "stdout: {}", run.stdout);
    assert!(run.stdout.contains("is below 101") || run.stderr.contains("is below 101"));
}

proptest! {
    // REQ: DEV-532
    #[test]
    fn dev_532_property_the_check_fails_exactly_below_the_minimum(
        percent in 0.0f64..100.0,
        minimum in 0.0f64..100.0,
    ) {
        let result = check_minimum(&summary(percent), minimum);
        prop_assert_eq!(result.is_err(), percent < minimum);
    }
}

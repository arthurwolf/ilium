//! PRF-020..PRF-026 from `docs/plugins-manual/pages/perf-benchmark-workflow.md`.
//!
//! PRF-020 and PRF-021 drive the bench crate's public API and fail until `to_json` and
//! `exit_code_for` meet the manual. The other bullets are process rules (review checklists,
//! commit messages, baselines, profiling) and pin the bullet's concrete wording in the manual.

use ilium_plugin_bench::{exit_code_for, BenchResult, Verdict};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// All manual pages concatenated, read from the repository copy.
fn manual_text() -> String {
    let pages = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../docs/plugins-manual/pages");
    let mut text = String::new();
    for entry in std::fs::read_dir(&pages).expect("manual pages directory exists") {
        let path = entry.expect("readable directory entry").path();
        if path.extension().is_some_and(|ext| ext == "md") {
            text.push_str(&std::fs::read_to_string(&path).expect("readable manual page"));
            text.push('\n');
        }
    }
    text
}

/// A bench record with fixed values, for checking what it serializes.
fn sample_result() -> BenchResult {
    BenchResult {
        median_ns: 1_234,
        p99_ns: 5_678,
        warmup_iterations: 100,
        measured_iterations: 1_000,
        pinned_core: Some(2),
        wasmtime_version: "0.0.0-test".to_string(),
        wasmtime_config: ilium_plugin_host::engine::runtime_settings(),
        cpu_model: "test-cpu".to_string(),
        core_count: 4,
        os: "linux".to_string(),
        commit: "abc1234".to_string(),
        target_features: String::new(),
        metrics: BTreeMap::new(),
    }
}

/// REQ: PRF-020
#[test]
fn perf_020_bench_record_is_labelled_synthetic_a() {
    let json = sample_result().to_json();
    assert_eq!(json["synthetic"], serde_json::Value::Bool(true));
}

/// REQ: PRF-020
#[test]
fn perf_020_bench_record_keeps_measured_median_b() {
    let json = sample_result().to_json();
    assert_eq!(json["median_ns"], serde_json::json!(1_234));
}

/// REQ: PRF-021
#[test]
fn perf_021_p99_over_deadline_exits_3_a() {
    assert_eq!(exit_code_for(&[Verdict::Fail]), 3);
}

/// REQ: PRF-021
#[test]
fn perf_021_p99_within_deadline_exits_0_b() {
    assert_eq!(exit_code_for(&[Verdict::Pass, Verdict::Recorded]), 0);
}

/// REQ: PRF-022
#[test]
fn perf_022_deadline_raise_needs_measured_need_pinned_by_manual_a() {
    assert!(manual_text().contains("unless the new value follows from a measured need"));
}

/// REQ: PRF-022
#[test]
fn perf_022_raising_commit_names_measured_numbers_pinned_by_manual_b() {
    assert!(manual_text().contains("The commit that makes the change MUST name the measured numbers"));
}

/// REQ: PRF-023
#[test]
fn perf_023_handler_p99_at_most_half_its_deadline_pinned_by_manual_a() {
    assert!(manual_text().contains("a bench p99 of at most half its deadline"));
}

/// REQ: PRF-023
#[test]
fn perf_023_on_events_reference_p99_under_10_ms_pinned_by_manual_b() {
    assert!(manual_text().contains("reports a p99 under 10 ms for `on-events`"));
}

/// REQ: PRF-024
#[test]
fn perf_024_timing_claim_gives_p99_of_ten_runs_and_spread_pinned_by_manual_a() {
    assert!(manual_text().contains("give the p99 of at least ten runs and the spread of those runs"));
}

/// REQ: PRF-024
#[test]
fn perf_024_review_checks_both_numbers_pinned_by_manual_b() {
    assert!(manual_text().contains("a review checklist item checks the message for both numbers"));
}

/// REQ: PRF-025
#[test]
fn perf_025_baseline_refreshed_only_in_its_own_commit_pinned_by_manual_a() {
    assert!(manual_text().contains("A baseline MUST be refreshed only in a commit of its own"));
}

/// REQ: PRF-025
#[test]
fn perf_025_baseline_mixed_commit_is_refused_pinned_by_manual_b() {
    assert!(manual_text().contains(
        "a commit that changes `bench/baseline.json` and any other file is refused by the review checklist"
    ));
}

/// REQ: PRF-026
#[test]
fn perf_026_profile_prints_one_record_per_export_call_pinned_by_manual_a() {
    assert!(manual_text().contains("MUST print one timing record for each export call that ran during the profile"));
}

/// REQ: PRF-026
#[test]
fn perf_026_profiling_changes_call_time_by_at_most_5_percent_pinned_by_manual_b() {
    assert!(manual_text().contains("MUST NOT change the call time of the plugin by more than 5 percent"));
}

//! HST-300..HST-313 (reference-host-budgets.md).

use ilium_plugin_bench::{
    exit_code_for, judge, pooling_allowed, run_benchmark, run_with, BenchError, PoolingEvidence,
    RunSpec, Verdict, MIN_MEASURED_ITERATIONS, MIN_WARMUP_ITERATIONS, RELEASE_GATE_IDS,
};
use ilium_plugin_host::telemetry::NearMissTracker;
use std::path::PathBuf;

fn benches_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("benches")
}

fn program_file_names() -> Vec<String> {
    let mut names = Vec::new();
    if let Ok(entries) = std::fs::read_dir(benches_dir()) {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                if name.ends_with(".rs") {
                    names.push(name.to_string());
                }
            }
        }
    }
    names.sort();
    names
}

fn program_number(file_name: &str) -> Option<u32> {
    let rest = file_name.strip_prefix('b')?;
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.len() != 2 || !rest[2..].starts_with('_') {
        return None;
    }
    digits.parse().ok()
}

fn quick_spec(warmup: u32, measured: u32) -> RunSpec {
    RunSpec {
        warmup_iterations: warmup,
        measured_iterations: measured,
    }
}

/// REQ: HST-300
#[test]
fn hst_300_minimum_constants_are_100_warmup_and_1000_measured() {
    assert_eq!(MIN_WARMUP_ITERATIONS, 100);
    assert_eq!(MIN_MEASURED_ITERATIONS, 1000);
}

/// REQ: HST-300
#[test]
fn hst_300_run_with_exact_minimums_records_the_counts() {
    let result = run_with("B1", &quick_spec(100, 1000)).expect("minimum run accepted");
    assert_eq!(result.warmup_iterations, 100);
    assert_eq!(result.measured_iterations, 1000);
}

/// REQ: HST-300
#[test]
fn hst_300_warmup_below_100_is_refused() {
    // manual does not pin the refusal message text, only the BelowMinimum variant
    let err = run_with("B1", &quick_spec(99, 1000)).expect_err("warm-up 99 refused");
    assert!(matches!(err, BenchError::BelowMinimum(_)), "{err:?}");
}

/// REQ: HST-300
#[test]
fn hst_300_measured_below_1000_is_refused() {
    // manual does not pin the refusal message text, only the BelowMinimum variant
    let err = run_with("B1", &quick_spec(100, 999)).expect_err("measured 999 refused");
    assert!(matches!(err, BenchError::BelowMinimum(_)), "{err:?}");
}

/// REQ: HST-300
#[test]
fn hst_300_run_above_minimums_is_accepted() {
    let result = run_with("B1", &quick_spec(101, 1001)).expect("above minimum accepted");
    assert_eq!(result.measured_iterations, 1001);
}

/// REQ: HST-300
#[test]
fn hst_300_run_is_pinned_to_one_core_on_linux() {
    let result = run_with("B1", &quick_spec(100, 1000)).expect("run accepted");
    assert!(result.pinned_core.is_some());
}

/// REQ: HST-301
#[test]
fn hst_301_result_records_median_and_p99() {
    let result = run_with("B1", &quick_spec(100, 1000)).expect("run accepted");
    assert!(result.median_ns > 0);
    assert!(result.p99_ns >= result.median_ns);
}

/// REQ: HST-301
#[test]
fn hst_301_result_records_environment_fields() {
    let result = run_with("B1", &quick_spec(100, 1000)).expect("run accepted");
    assert!(!result.wasmtime_version.is_empty());
    assert!(!result.cpu_model.is_empty());
    assert!(result.core_count >= 1);
    assert!(!result.os.is_empty());
    assert!(!result.commit.is_empty());
}

/// REQ: HST-301
#[test]
fn hst_301_result_records_the_runtime_config_output() {
    let result = run_with("B1", &quick_spec(100, 1000)).expect("run accepted");
    assert_eq!(
        result.wasmtime_config,
        ilium_plugin_host::engine::runtime_settings()
    );
}

/// REQ: HST-301
#[test]
fn hst_301_json_output_schema_has_every_required_key() {
    let result = run_with("B1", &quick_spec(100, 1000)).expect("run accepted");
    let json = result.to_json();
    for key in [
        "median_ns",
        "p99_ns",
        "wasmtime_version",
        "wasmtime_config",
        "cpu_model",
        "core_count",
        "os",
        "commit",
    ] {
        assert!(json.get(key).is_some(), "missing key {key}");
    }
}

/// REQ: HST-302
#[test]
fn hst_302_missed_budget_makes_the_run_exit_nonzero() {
    let verdicts = [Verdict::Pass, judge("B5", "p99_ms", 2.5)];
    // manual does not pin the numeric exit code, only that it is non-zero
    assert_ne!(exit_code_for(&verdicts), 0);
    assert_ne!(exit_code_for(&[judge("B5", "p99_ms", 2.5)]), 0);
    assert_eq!(exit_code_for(&[Verdict::Pass]), 0);
}

/// REQ: HST-302
#[test]
fn hst_302_all_passing_exits_zero() {
    assert_eq!(exit_code_for(&[Verdict::Pass, Verdict::Pass]), 0);
}

/// REQ: HST-302
#[test]
fn hst_302_record_rows_never_fail_the_run() {
    let verdict = judge("B21", "median_ms", 500.0);
    assert_eq!(verdict, Verdict::Recorded);
    assert_eq!(exit_code_for(&[verdict, Verdict::Pass]), 0);
}

/// REQ: HST-302
#[test]
fn hst_302_plan_stop_also_exits_nonzero() {
    assert_ne!(exit_code_for(&[Verdict::PlanStop]), 0);
}

/// REQ: HST-303
#[test]
fn hst_303_release_build_flag_is_true_when_run_for_budgets() {
    assert!(ilium_plugin_bench::is_release_build());
}

/// REQ: HST-303
#[test]
fn hst_303_debug_build_run_is_refused() {
    let outcome = run_benchmark("B1");
    if ilium_plugin_bench::is_release_build() {
        // A release build must never be refused for being a debug build.
        assert!(!matches!(outcome, Err(BenchError::NotReleaseBuild)));
    } else {
        assert_eq!(outcome.unwrap_err(), BenchError::NotReleaseBuild);
    }
}

/// REQ: HST-303
#[test]
fn hst_303_result_records_target_features_of_the_build() {
    let result = run_with("B1", &quick_spec(100, 1000)).expect("run accepted");
    #[cfg(target_arch = "x86_64")]
    assert!(
        result
            .target_features
            .split(',')
            .any(|feature| feature.trim() == "sse2"),
        "{}",
        result.target_features
    );
    assert_eq!(
        result.to_json()["target_features"],
        serde_json::Value::String(result.target_features.clone())
    );
}

/// REQ: HST-304
#[test]
fn hst_304_b1_over_twice_is_plan_stop() {
    assert_eq!(judge("B1", "median_ns", 2001.0), Verdict::PlanStop);
}

/// REQ: HST-304
#[test]
fn hst_304_b3_over_twice_is_plan_stop() {
    assert_eq!(judge("B3", "p99_ratio", 2.61), Verdict::PlanStop);
}

/// REQ: HST-304
#[test]
fn hst_304_b4_over_twice_is_plan_stop() {
    assert_eq!(judge("B4", "extra_ms", 2.01), Verdict::PlanStop);
}

/// REQ: HST-304
#[test]
fn hst_304_exactly_twice_is_an_ordinary_failure() {
    assert_eq!(judge("B1", "median_ns", 2000.0), Verdict::Fail);
}

/// REQ: HST-304
#[test]
fn hst_304_other_benchmarks_never_plan_stop() {
    assert_eq!(judge("B5", "p99_ms", 100.0), Verdict::Fail);
    assert_eq!(judge("B23", "cold_ms", 600.0), Verdict::Fail);
}

/// REQ: HST-305
#[test]
fn hst_305_one_program_file_exists_for_each_of_b1_to_b24() {
    let numbers: Vec<u32> = program_file_names()
        .iter()
        .filter_map(|n| program_number(n))
        .collect();
    for expected in 1..=24u32 {
        assert!(numbers.contains(&expected), "no program for B{expected}");
    }
}

/// REQ: HST-305
#[test]
fn hst_305_no_two_programs_share_an_id() {
    let mut numbers: Vec<u32> = program_file_names()
        .iter()
        .filter_map(|n| program_number(n))
        .collect();
    let total = numbers.len();
    numbers.sort_unstable();
    numbers.dedup();
    assert!(total > 0);
    assert_eq!(numbers.len(), total);
}

/// REQ: HST-305
#[test]
fn hst_305_no_program_outside_b1_to_b24() {
    let numbers: Vec<u32> = program_file_names()
        .iter()
        .filter_map(|n| program_number(n))
        .collect();
    assert_eq!(numbers.len(), 24);
    assert!(numbers.iter().all(|n| (1..=24).contains(n)));
}

/// REQ: HST-306
#[test]
fn hst_306_release_gate_ids_are_b1_to_b9_b16_to_b20_and_b23() {
    let mut expected: Vec<String> = (1..=9)
        .chain(16..=20)
        .chain(23..=23)
        .map(|n| format!("B{n}"))
        .collect();
    let mut actual: Vec<String> = RELEASE_GATE_IDS.iter().map(|s| s.to_string()).collect();
    expected.sort();
    actual.sort();
    assert_eq!(actual, expected);
}

/// REQ: HST-306
#[test]
fn hst_306_gate_fails_when_any_gated_budget_is_missed() {
    let verdicts = [Verdict::Pass, Verdict::Fail, Verdict::Pass];
    assert_ne!(exit_code_for(&verdicts), 0);
}

/// REQ: HST-306
#[test]
fn hst_306_gate_excludes_b10_to_b15_b21_b22_and_b24() {
    for id in [
        "B10", "B11", "B12", "B13", "B14", "B15", "B21", "B22", "B24",
    ] {
        assert!(
            !RELEASE_GATE_IDS.contains(&id),
            "{id} must not be in the gate"
        );
    }
}

/// REQ: HST-307
#[test]
fn hst_307_host_start_150ms_boundary() {
    assert_eq!(judge("B6", "host_start_ms", 149.0), Verdict::Pass);
    assert_eq!(judge("B6", "host_start_ms", 150.0), Verdict::Pass);
    assert_eq!(judge("B6", "host_start_ms", 151.0), Verdict::Fail);
}

/// REQ: HST-307
#[test]
#[ignore = "budget: run on the reference machine"]
fn hst_307_host_start_with_40_bundled_plugins_measured() {
    let result = run_benchmark("B6").expect("benchmark runs");
    let value = *result
        .metrics
        .get("host_start_ms")
        .expect("metric recorded");
    assert!(value <= 150.0, "host_start_ms={value}");
}

/// REQ: HST-308
#[test]
fn hst_308_cli_cold_60ms_boundary() {
    assert_eq!(judge("B23", "cold_ms", 59.0), Verdict::Pass);
    assert_eq!(judge("B23", "cold_ms", 60.0), Verdict::Pass);
    assert_eq!(judge("B23", "cold_ms", 61.0), Verdict::Fail);
}

/// REQ: HST-308
#[test]
#[ignore = "budget: run on the reference machine"]
fn hst_308_cli_cold_starts_no_server_and_no_unrelated_plugin() {
    let result = run_benchmark("B23").expect("benchmark runs");
    assert_eq!(result.metrics.get("servers_started").copied(), Some(0.0));
    assert_eq!(
        result.metrics.get("unrelated_plugins_started").copied(),
        Some(0.0)
    );
}

/// REQ: HST-309
#[test]
fn hst_309_added_rss_must_be_under_120_mib() {
    assert_eq!(judge("B22", "added_rss_mib", 119.0), Verdict::Pass);
    assert_eq!(judge("B22", "added_rss_mib", 121.0), Verdict::Fail);
}

/// REQ: HST-309
#[test]
fn hst_309_exactly_120_mib_is_not_under() {
    assert_eq!(judge("B22", "added_rss_mib", 120.0), Verdict::Fail);
}

/// REQ: HST-310
#[test]
fn hst_310_rust_wasm_ratio_boundary_1_3x() {
    assert_eq!(judge("B3", "p99_ratio", 1.29), Verdict::Pass);
    assert_eq!(judge("B3", "p99_ratio", 1.3), Verdict::Pass);
    assert_eq!(judge("B3", "p99_ratio", 1.31), Verdict::Fail);
}

/// REQ: HST-310
#[test]
#[ignore = "budget: run on the reference machine"]
fn hst_310_rust_wasm_scenes_p99_within_1_3x_native_measured() {
    let result = run_benchmark("B3").expect("benchmark runs");
    let ratio = *result.metrics.get("p99_ratio").expect("metric recorded");
    assert!(ratio <= 1.3, "p99_ratio={ratio}");
}

/// REQ: HST-311
#[test]
fn hst_311_epoch_overhead_boundary_5_percent() {
    assert_eq!(judge("B7", "overhead_percent", 4.9), Verdict::Pass);
    assert_eq!(judge("B7", "overhead_percent", 5.0), Verdict::Pass);
    assert_eq!(judge("B7", "overhead_percent", 5.1), Verdict::Fail);
}

/// REQ: HST-311
#[test]
#[ignore = "budget: run on the reference machine"]
fn hst_311_epoch_overhead_with_10ms_ticker_measured() {
    let result = run_benchmark("B7").expect("benchmark runs");
    let overhead = *result
        .metrics
        .get("overhead_percent")
        .expect("metric recorded");
    assert!(overhead <= 5.0, "overhead_percent={overhead}");
}

fn evidence(
    on_demand_start: f64,
    on_demand_b2: f64,
    pool_start: f64,
    pool_b2: f64,
    extra: f64,
) -> PoolingEvidence {
    PoolingEvidence {
        on_demand_host_start_ms: on_demand_start,
        on_demand_b2_wall_ms: on_demand_b2,
        pooling_host_start_ms: pool_start,
        pooling_b2_wall_ms: pool_b2,
        pooling_extra_rss_mib: extra,
    }
}

/// REQ: HST-312
#[test]
fn hst_312_pooling_enabled_when_on_demand_misses_start_and_pooling_meets_both() {
    assert!(pooling_allowed(&evidence(200.0, 4.0, 100.0, 4.0, 10.0)));
}

/// REQ: HST-312
#[test]
fn hst_312_pooling_enabled_when_on_demand_misses_b2_only() {
    assert!(pooling_allowed(&evidence(100.0, 6.0, 100.0, 4.0, 10.0)));
    assert!(pooling_allowed(&evidence(100.0, 6.0, 100.0, 4.0, 16.0)));
    assert!(!pooling_allowed(&evidence(100.0, 6.0, 100.0, 4.0, 16.5)));
}

/// REQ: HST-312
#[test]
fn hst_312_pooling_refused_when_on_demand_meets_both() {
    assert!(!pooling_allowed(&evidence(100.0, 4.0, 50.0, 2.0, 1.0)));
}

/// REQ: HST-312
#[test]
fn hst_312_pooling_refused_when_pooling_misses_host_start() {
    assert!(!pooling_allowed(&evidence(200.0, 4.0, 151.0, 4.0, 10.0)));
}

/// REQ: HST-312
#[test]
fn hst_312_pooling_refused_when_pooling_misses_b2() {
    assert!(!pooling_allowed(&evidence(200.0, 4.0, 100.0, 5.1, 10.0)));
}

/// REQ: HST-312
#[test]
fn hst_312_extra_rss_boundary_16_mib() {
    assert!(pooling_allowed(&evidence(200.0, 4.0, 100.0, 4.0, 16.0)));
    assert!(!pooling_allowed(&evidence(200.0, 4.0, 100.0, 4.0, 16.1)));
}

/// REQ: HST-313
#[test]
fn hst_313_call_at_80_percent_of_deadline_is_a_near_miss() {
    let mut tracker = NearMissTracker::new();
    assert!(tracker.record_call(0, 100, 80));
    assert_eq!(tracker.count_in_window(0), 1);
}

/// REQ: HST-313
#[test]
fn hst_313_call_at_79_percent_is_not_a_near_miss() {
    let mut tracker = NearMissTracker::new();
    assert!(!tracker.record_call(0, 100, 79));
    assert_eq!(tracker.count_in_window(0), 0);
}

/// REQ: HST-313
#[test]
fn hst_313_call_that_missed_its_deadline_is_not_a_near_miss() {
    let mut tracker = NearMissTracker::new();
    assert!(!tracker.record_call(0, 100, 101));
    assert_eq!(tracker.count_in_window(0), 0);
}

/// REQ: HST-313
#[test]
fn hst_313_call_exactly_at_deadline_is_in_time_and_a_near_miss() {
    let mut tracker = NearMissTracker::new();
    assert!(tracker.record_call(0, 100, 100));
}

/// REQ: HST-313
#[test]
fn hst_313_five_near_misses_in_five_minutes_make_the_plugin_degraded() {
    let mut tracker = NearMissTracker::new();
    for index in 0..5u64 {
        tracker.record_call(index * 1000, 100, 90);
    }
    assert!(tracker.is_degraded(4_000));
}

/// REQ: HST-313
#[test]
fn hst_313_four_near_misses_are_not_degraded() {
    let mut tracker = NearMissTracker::new();
    for index in 0..4u64 {
        tracker.record_call(index * 1000, 100, 90);
    }
    assert!(!tracker.is_degraded(3_000));
}

/// REQ: HST-313
#[test]
fn hst_313_near_misses_older_than_five_minutes_expire() {
    let mut tracker = NearMissTracker::new();
    for index in 0..5u64 {
        tracker.record_call(index * 1000, 100, 90);
    }
    assert!(!tracker.is_degraded(4_000 + 300_001));
}

//! Manual page `reference-host-budgets`: hook latency, startup phase timing, compiled-component
//! cache speed and bounds, the release regression gate and the benchmark result format
//! (HST-622, 623, 625, 626, 627, 646).

use super::common::{install_ok, pkg, rig, NOTES};

/// REQ: HST-622
#[test]
fn hst_622_hook_latency_p99_is_at_most_2_ms_with_40_plugins_and_hard_deadline_is_5_ms() {
    let rig = rig();
    rig.install_many(40);
    let report = rig.run_benchmark("B25");
    assert!(
        report.p99_latency_ms <= 2.0,
        "p99 input-to-echo latency <= 2 ms, got {}",
        report.p99_latency_ms
    );
    assert!(
        report.hook_deadline_ms == 5.0,
        "hooks run under a 5 ms hard deadline"
    );
}

/// REQ: HST-623
#[test]
fn hst_623_each_activation_phase_is_timed_and_shown_by_status_startup() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.activate(NOTES);
    let phases: Vec<String> = rig
        .telemetry_records("startup_phase_us", NOTES)
        .iter()
        .filter_map(|record| record["phase"].as_str().map(str::to_owned))
        .collect();
    for phase in [
        "load",
        "compile_or_cache_lookup",
        "instantiate",
        "init",
        "first_event",
    ] {
        assert!(
            phases.iter().any(|name| name == phase),
            "phase {phase} timed"
        );
    }
    let output = rig.cli(&["plugin", "status", NOTES, "--startup"]);
    assert!(
        output.stdout_raw.contains("first_event"),
        "phases shown by plugin status --startup"
    );
}

/// REQ: HST-625
#[test]
fn hst_625_warm_cache_start_is_at_least_5_times_faster_and_corrupt_entries_are_recompiled() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let cold = rig.cold_compile_time(NOTES);
    let warm = rig.warm_cache_start_time(NOTES);
    assert!(
        cold >= warm * 5,
        "warm start at least 5x faster: cold {cold:?}, warm {warm:?}"
    );
    rig.corrupt_compile_cache_entry(NOTES);
    rig.activate(NOTES);
    assert_eq!(
        rig.log_lines_with_code("host.cache.corrupt").len(),
        1,
        "corrupt entry logged and discarded"
    );
}

/// REQ: HST-626
#[test]
fn hst_626_compiled_cache_is_bounded_to_512_mib_evicts_lru_and_reports_stats() {
    let rig = rig();
    assert_eq!(
        rig.compile_cache_limit_bytes(),
        512 * 1024 * 1024,
        "default bound 512 MiB"
    );
    rig.fill_compile_cache_beyond_limit();
    assert!(
        rig.compile_cache_bytes() <= 512 * 1024 * 1024,
        "LRU eviction keeps the cache under the bound"
    );
    let output = rig.cli(&["plugin", "stats", "--cache"]);
    assert!(
        output.stdout_raw.contains("size") && output.stdout_raw.contains("hit"),
        "size and hit rate printed"
    );
}

/// REQ: HST-627
#[test]
fn hst_627_release_gate_fails_a_median_regression_over_10_percent_even_within_budget() {
    let rig = rig();
    rig.write_baseline_medians(&[("B25", 1_000_000)]);
    rig.write_current_medians(&[("B25", 1_150_000)], 5_000_000);
    let output = rig.run_release_gate("wave-1", "benchmark-regression-fixture");
    assert_ne!(
        output.exit_code, 0,
        "a 15 percent regression fails although the absolute budget is met"
    );
}

/// REQ: HST-646
#[test]
fn hst_646_benchmark_json_lines_have_required_fields_and_compare_exits_1_on_a_failed_pass() {
    let rig = rig();
    let results = rig.run_benchmark_harness();
    for line in results.lines() {
        let value: serde_json::Value =
            serde_json::from_str(line).expect("one JSON object per line");
        for field in ["id", "median_ns", "p99_ns", "budget_ns", "pass", "commit"] {
            assert!(value.get(field).is_some(), "benchmark line carries {field}");
        }
    }
    let failing = rig.write_results_file_with_failed_pass();
    let output = rig.cli(&[
        "ilium-plugin-bench",
        "--compare",
        failing.to_str().expect("utf-8 path"),
    ]);
    assert_eq!(
        output.exit_code, 1,
        "compare exits 1 when any pass is false"
    );
}

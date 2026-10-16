//! Budget verdict tests and measured budget tests (reference-host-budgets.md).

use ilium_plugin_bench::{judge, run_benchmark, Verdict};

/// REQ: B1
#[test]
fn b01_empty_host_call_median_below_budget_passes() {
    assert_eq!(judge("B1", "median_ns", 1000.0 * 0.9), Verdict::Pass);
}

/// REQ: B1
#[test]
fn b01_empty_host_call_median_above_budget_fails() {
    let verdict = judge("B1", "median_ns", 1000.0 * 1.01);
    assert_eq!(verdict, Verdict::Fail);
}

/// REQ: B1, HST-304
#[test]
fn b01_empty_host_call_median_over_twice_budget_is_plan_stop() {
    assert_eq!(judge("B1", "median_ns", 1000.0 * 2.01), Verdict::PlanStop);
    assert_eq!(judge("B1", "median_ns", 1000.0 * 2.0), Verdict::Fail);
}

/// REQ: B1
#[test]
#[ignore = "budget: run on the reference machine"]
fn b01_empty_host_call_median_measured_within_budget() {
    let result = run_benchmark("B1").expect("benchmark runs");
    let value = *result.metrics.get("median_ns").expect("metric recorded");
    assert_eq!(judge("B1", "median_ns", value), Verdict::Pass, "median_ns={value}");
}

/// REQ: B1
#[test]
fn b01_write_rows_400kb_below_budget_passes() {
    assert_eq!(judge("B1", "write_rows_400kb_ns", 200000.0 * 0.9), Verdict::Pass);
}

/// REQ: B1
#[test]
fn b01_write_rows_400kb_above_budget_fails() {
    let verdict = judge("B1", "write_rows_400kb_ns", 200000.0 * 1.01);
    assert_eq!(verdict, Verdict::Fail);
}

/// REQ: B1, HST-304
#[test]
fn b01_write_rows_400kb_over_twice_budget_is_plan_stop() {
    assert_eq!(judge("B1", "write_rows_400kb_ns", 200000.0 * 2.01), Verdict::PlanStop);
    assert_eq!(judge("B1", "write_rows_400kb_ns", 200000.0 * 2.0), Verdict::Fail);
}

/// REQ: B1
#[test]
#[ignore = "budget: run on the reference machine"]
fn b01_write_rows_400kb_measured_within_budget() {
    let result = run_benchmark("B1").expect("benchmark runs");
    let value = *result.metrics.get("write_rows_400kb_ns").expect("metric recorded");
    assert_eq!(judge("B1", "write_rows_400kb_ns", value), Verdict::Pass, "write_rows_400kb_ns={value}");
}

/// REQ: B2
#[test]
fn b02_rust_instantiation_wall_below_budget_passes() {
    assert_eq!(judge("B2", "wall_ms", 5.0 * 0.9), Verdict::Pass);
}

/// REQ: B2
#[test]
fn b02_rust_instantiation_wall_above_budget_fails() {
    let verdict = judge("B2", "wall_ms", 5.0 * 1.01);
    assert_eq!(verdict, Verdict::Fail);
}

/// REQ: B2
#[test]
#[ignore = "budget: run on the reference machine"]
fn b02_rust_instantiation_wall_measured_within_budget() {
    let result = run_benchmark("B2").expect("benchmark runs");
    let value = *result.metrics.get("wall_ms").expect("metric recorded");
    assert_eq!(judge("B2", "wall_ms", value), Verdict::Pass, "wall_ms={value}");
}

/// REQ: B2
#[test]
fn b02_rust_instantiation_rss_below_budget_passes() {
    assert_eq!(judge("B2", "rss_mib", 4.0 * 0.9), Verdict::Pass);
}

/// REQ: B2
#[test]
fn b02_rust_instantiation_rss_above_budget_fails() {
    let verdict = judge("B2", "rss_mib", 4.0 * 1.01);
    assert_eq!(verdict, Verdict::Fail);
}

/// REQ: B2
#[test]
#[ignore = "budget: run on the reference machine"]
fn b02_rust_instantiation_rss_measured_within_budget() {
    let result = run_benchmark("B2").expect("benchmark runs");
    let value = *result.metrics.get("rss_mib").expect("metric recorded");
    assert_eq!(judge("B2", "rss_mib", value), Verdict::Pass, "rss_mib={value}");
}

/// REQ: B3
#[test]
fn b03_rust_wasm_p99_vs_native_below_budget_passes() {
    assert_eq!(judge("B3", "p99_ratio", 1.3 * 0.9), Verdict::Pass);
}

/// REQ: B3
#[test]
fn b03_rust_wasm_p99_vs_native_above_budget_fails() {
    let verdict = judge("B3", "p99_ratio", 1.3 * 1.01);
    assert_eq!(verdict, Verdict::Fail);
}

/// REQ: B3, HST-304
#[test]
fn b03_rust_wasm_p99_vs_native_over_twice_budget_is_plan_stop() {
    assert_eq!(judge("B3", "p99_ratio", 1.3 * 2.01), Verdict::PlanStop);
    assert_eq!(judge("B3", "p99_ratio", 1.3 * 2.0), Verdict::Fail);
}

/// REQ: B3
#[test]
#[ignore = "budget: run on the reference machine"]
fn b03_rust_wasm_p99_vs_native_measured_within_budget() {
    let result = run_benchmark("B3").expect("benchmark runs");
    let value = *result.metrics.get("p99_ratio").expect("metric recorded");
    assert_eq!(judge("B3", "p99_ratio", value), Verdict::Pass, "p99_ratio={value}");
}

/// REQ: B4
#[test]
fn b04_decoration_updates_extra_below_budget_passes() {
    assert_eq!(judge("B4", "extra_ms", 1.0 * 0.9), Verdict::Pass);
}

/// REQ: B4
#[test]
fn b04_decoration_updates_extra_above_budget_fails() {
    let verdict = judge("B4", "extra_ms", 1.0 * 1.01);
    assert_eq!(verdict, Verdict::Fail);
}

/// REQ: B4, HST-304
#[test]
fn b04_decoration_updates_extra_over_twice_budget_is_plan_stop() {
    assert_eq!(judge("B4", "extra_ms", 1.0 * 2.01), Verdict::PlanStop);
    assert_eq!(judge("B4", "extra_ms", 1.0 * 2.0), Verdict::Fail);
}

/// REQ: B4
#[test]
#[ignore = "budget: run on the reference machine"]
fn b04_decoration_updates_extra_measured_within_budget() {
    let result = run_benchmark("B4").expect("benchmark runs");
    let value = *result.metrics.get("extra_ms").expect("metric recorded");
    assert_eq!(judge("B4", "extra_ms", value), Verdict::Pass, "extra_ms={value}");
}

/// REQ: B5
#[test]
fn b05_on_events_50_batch_30_plugins_p99_below_budget_passes() {
    assert_eq!(judge("B5", "p99_ms", 2.0 * 0.9), Verdict::Pass);
}

/// REQ: B5
#[test]
fn b05_on_events_50_batch_30_plugins_p99_above_budget_fails() {
    let verdict = judge("B5", "p99_ms", 2.0 * 1.01);
    assert_eq!(verdict, Verdict::Fail);
}

/// REQ: B5
#[test]
#[ignore = "budget: run on the reference machine"]
fn b05_on_events_50_batch_30_plugins_p99_measured_within_budget() {
    let result = run_benchmark("B5").expect("benchmark runs");
    let value = *result.metrics.get("p99_ms").expect("metric recorded");
    assert_eq!(judge("B5", "p99_ms", value), Verdict::Pass, "p99_ms={value}");
}

/// REQ: B6, HOST-START
#[test]
fn b06_host_start_below_budget_passes() {
    assert_eq!(judge("B6", "host_start_ms", 150.0 * 0.9), Verdict::Pass);
}

/// REQ: B6, HOST-START
#[test]
fn b06_host_start_above_budget_fails() {
    let verdict = judge("B6", "host_start_ms", 150.0 * 1.01);
    assert_eq!(verdict, Verdict::Fail);
}

/// REQ: B6, HOST-START
#[test]
#[ignore = "budget: run on the reference machine"]
fn b06_host_start_measured_within_budget() {
    let result = run_benchmark("B6").expect("benchmark runs");
    let value = *result.metrics.get("host_start_ms").expect("metric recorded");
    assert_eq!(judge("B6", "host_start_ms", value), Verdict::Pass, "host_start_ms={value}");
}

/// REQ: B6
#[test]
fn b06_added_rss_below_budget_passes() {
    assert_eq!(judge("B6", "added_rss_mib", 120.0 * 0.9), Verdict::Pass);
}

/// REQ: B6
#[test]
fn b06_added_rss_above_budget_fails() {
    let verdict = judge("B6", "added_rss_mib", 120.0 * 1.01);
    assert_eq!(verdict, Verdict::Fail);
}

/// REQ: B6
#[test]
#[ignore = "budget: run on the reference machine"]
fn b06_added_rss_measured_within_budget() {
    let result = run_benchmark("B6").expect("benchmark runs");
    let value = *result.metrics.get("added_rss_mib").expect("metric recorded");
    assert_eq!(judge("B6", "added_rss_mib", value), Verdict::Pass, "added_rss_mib={value}");
}

/// REQ: B7
#[test]
fn b07_epoch_overhead_below_budget_passes() {
    assert_eq!(judge("B7", "overhead_percent", 5.0 * 0.9), Verdict::Pass);
}

/// REQ: B7
#[test]
fn b07_epoch_overhead_above_budget_fails() {
    let verdict = judge("B7", "overhead_percent", 5.0 * 1.01);
    assert_eq!(verdict, Verdict::Fail);
}

/// REQ: B7
#[test]
#[ignore = "budget: run on the reference machine"]
fn b07_epoch_overhead_measured_within_budget() {
    let result = run_benchmark("B7").expect("benchmark runs");
    let value = *result.metrics.get("overhead_percent").expect("metric recorded");
    assert_eq!(judge("B7", "overhead_percent", value), Verdict::Pass, "overhead_percent={value}");
}

/// REQ: B8
#[test]
fn b08_classify_per_pane_below_budget_passes() {
    assert_eq!(judge("B8", "per_pane_ms", 0.5 * 0.9), Verdict::Pass);
}

/// REQ: B8
#[test]
fn b08_classify_per_pane_above_budget_fails() {
    let verdict = judge("B8", "per_pane_ms", 0.5 * 1.01);
    assert_eq!(verdict, Verdict::Fail);
}

/// REQ: B8
#[test]
#[ignore = "budget: run on the reference machine"]
fn b08_classify_per_pane_measured_within_budget() {
    let result = run_benchmark("B8").expect("benchmark runs");
    let value = *result.metrics.get("per_pane_ms").expect("metric recorded");
    assert_eq!(judge("B8", "per_pane_ms", value), Verdict::Pass, "per_pane_ms={value}");
}

/// REQ: B9
#[test]
fn b09_job_start_overhead_below_budget_passes() {
    assert_eq!(judge("B9", "start_ms", 5.0 * 0.9), Verdict::Pass);
}

/// REQ: B9
#[test]
fn b09_job_start_overhead_above_budget_fails() {
    let verdict = judge("B9", "start_ms", 5.0 * 1.01);
    assert_eq!(verdict, Verdict::Fail);
}

/// REQ: B9
#[test]
#[ignore = "budget: run on the reference machine"]
fn b09_job_start_overhead_measured_within_budget() {
    let result = run_benchmark("B9").expect("benchmark runs");
    let value = *result.metrics.get("start_ms").expect("metric recorded");
    assert_eq!(judge("B9", "start_ms", value), Verdict::Pass, "start_ms={value}");
}

/// REQ: B9
#[test]
fn b09_concurrent_on_events_p99_vs_idle_below_budget_passes() {
    assert_eq!(judge("B9", "p99_ratio_idle", 1.2 * 0.9), Verdict::Pass);
}

/// REQ: B9
#[test]
fn b09_concurrent_on_events_p99_vs_idle_above_budget_fails() {
    let verdict = judge("B9", "p99_ratio_idle", 1.2 * 1.01);
    assert_eq!(verdict, Verdict::Fail);
}

/// REQ: B9
#[test]
#[ignore = "budget: run on the reference machine"]
fn b09_concurrent_on_events_p99_vs_idle_measured_within_budget() {
    let result = run_benchmark("B9").expect("benchmark runs");
    let value = *result.metrics.get("p99_ratio_idle").expect("metric recorded");
    assert_eq!(judge("B9", "p99_ratio_idle", value), Verdict::Pass, "p99_ratio_idle={value}");
}

/// REQ: B10
#[test]
fn b10_hook_chain_of_5_below_budget_passes() {
    assert_eq!(judge("B10", "chain_ms", 1.0 * 0.9), Verdict::Pass);
}

/// REQ: B10
#[test]
fn b10_hook_chain_of_5_above_budget_fails() {
    let verdict = judge("B10", "chain_ms", 1.0 * 1.01);
    assert_eq!(verdict, Verdict::Fail);
}

/// REQ: B10
#[test]
#[ignore = "budget: run on the reference machine"]
fn b10_hook_chain_of_5_measured_within_budget() {
    let result = run_benchmark("B10").expect("benchmark runs");
    let value = *result.metrics.get("chain_ms").expect("metric recorded");
    assert_eq!(judge("B10", "chain_ms", value), Verdict::Pass, "chain_ms={value}");
}

/// REQ: B11
#[test]
fn b11_dynamic_menu_first_frame_below_budget_passes() {
    assert_eq!(judge("B11", "first_frame_ms", 5.0 * 0.9), Verdict::Pass);
}

/// REQ: B11
#[test]
fn b11_dynamic_menu_first_frame_above_budget_fails() {
    let verdict = judge("B11", "first_frame_ms", 5.0 * 1.01);
    assert_eq!(verdict, Verdict::Fail);
}

/// REQ: B11
#[test]
#[ignore = "budget: run on the reference machine"]
fn b11_dynamic_menu_first_frame_measured_within_budget() {
    let result = run_benchmark("B11").expect("benchmark runs");
    let value = *result.metrics.get("first_frame_ms").expect("metric recorded");
    assert_eq!(judge("B11", "first_frame_ms", value), Verdict::Pass, "first_frame_ms={value}");
}

/// REQ: B12
#[test]
fn b12_fairness_p99_vs_idle_below_budget_passes() {
    assert_eq!(judge("B12", "p99_ratio_idle", 1.5 * 0.9), Verdict::Pass);
}

/// REQ: B12
#[test]
fn b12_fairness_p99_vs_idle_above_budget_fails() {
    let verdict = judge("B12", "p99_ratio_idle", 1.5 * 1.01);
    assert_eq!(verdict, Verdict::Fail);
}

/// REQ: B12
#[test]
#[ignore = "budget: run on the reference machine"]
fn b12_fairness_p99_vs_idle_measured_within_budget() {
    let result = run_benchmark("B12").expect("benchmark runs");
    let value = *result.metrics.get("p99_ratio_idle").expect("metric recorded");
    assert_eq!(judge("B12", "p99_ratio_idle", value), Verdict::Pass, "p99_ratio_idle={value}");
}

/// REQ: B13
#[test]
fn b13_ui_tree_4096_nodes_painted_below_budget_passes() {
    assert_eq!(judge("B13", "painted_ms", 8.0 * 0.9), Verdict::Pass);
}

/// REQ: B13
#[test]
fn b13_ui_tree_4096_nodes_painted_above_budget_fails() {
    let verdict = judge("B13", "painted_ms", 8.0 * 1.01);
    assert_eq!(verdict, Verdict::Fail);
}

/// REQ: B13
#[test]
#[ignore = "budget: run on the reference machine"]
fn b13_ui_tree_4096_nodes_painted_measured_within_budget() {
    let result = run_benchmark("B13").expect("benchmark runs");
    let value = *result.metrics.get("painted_ms").expect("metric recorded");
    assert_eq!(judge("B13", "painted_ms", value), Verdict::Pass, "painted_ms={value}");
}

/// REQ: B14
#[test]
fn b14_replication_server_cpu_below_budget_passes() {
    assert_eq!(judge("B14", "server_cpu_percent", 5.0 * 0.9), Verdict::Pass);
}

/// REQ: B14
#[test]
fn b14_replication_server_cpu_above_budget_fails() {
    let verdict = judge("B14", "server_cpu_percent", 5.0 * 1.01);
    assert_eq!(verdict, Verdict::Fail);
}

/// REQ: B14
#[test]
#[ignore = "budget: run on the reference machine"]
fn b14_replication_server_cpu_measured_within_budget() {
    let result = run_benchmark("B14").expect("benchmark runs");
    let value = *result.metrics.get("server_cpu_percent").expect("metric recorded");
    assert_eq!(judge("B14", "server_cpu_percent", value), Verdict::Pass, "server_cpu_percent={value}");
}

/// REQ: B14
#[test]
fn b14_replication_mirror_lag_p99_below_budget_passes() {
    assert_eq!(judge("B14", "mirror_lag_p99_ms", 5.0 * 0.9), Verdict::Pass);
}

/// REQ: B14
#[test]
fn b14_replication_mirror_lag_p99_above_budget_fails() {
    let verdict = judge("B14", "mirror_lag_p99_ms", 5.0 * 1.01);
    assert_eq!(verdict, Verdict::Fail);
}

/// REQ: B14
#[test]
#[ignore = "budget: run on the reference machine"]
fn b14_replication_mirror_lag_p99_measured_within_budget() {
    let result = run_benchmark("B14").expect("benchmark runs");
    let value = *result.metrics.get("mirror_lag_p99_ms").expect("metric recorded");
    assert_eq!(judge("B14", "mirror_lag_p99_ms", value), Verdict::Pass, "mirror_lag_p99_ms={value}");
}

/// REQ: B15
#[test]
fn b15_client_submission_round_trip_p99_below_budget_passes() {
    assert_eq!(judge("B15", "p99_ms", 3.0 * 0.9), Verdict::Pass);
}

/// REQ: B15
#[test]
fn b15_client_submission_round_trip_p99_above_budget_fails() {
    let verdict = judge("B15", "p99_ms", 3.0 * 1.01);
    assert_eq!(verdict, Verdict::Fail);
}

/// REQ: B15
#[test]
#[ignore = "budget: run on the reference machine"]
fn b15_client_submission_round_trip_p99_measured_within_budget() {
    let result = run_benchmark("B15").expect("benchmark runs");
    let value = *result.metrics.get("p99_ms").expect("metric recorded");
    assert_eq!(judge("B15", "p99_ms", value), Verdict::Pass, "p99_ms={value}");
}

/// REQ: B16
#[test]
fn b16_key_to_call_t1_p99_below_budget_passes() {
    assert_eq!(judge("B16", "t1_p99_ms", 1.0 * 0.9), Verdict::Pass);
}

/// REQ: B16
#[test]
fn b16_key_to_call_t1_p99_above_budget_fails() {
    let verdict = judge("B16", "t1_p99_ms", 1.0 * 1.01);
    assert_eq!(verdict, Verdict::Fail);
}

/// REQ: B16
#[test]
#[ignore = "budget: run on the reference machine"]
fn b16_key_to_call_t1_p99_measured_within_budget() {
    let result = run_benchmark("B16").expect("benchmark runs");
    let value = *result.metrics.get("t1_p99_ms").expect("metric recorded");
    assert_eq!(judge("B16", "t1_p99_ms", value), Verdict::Pass, "t1_p99_ms={value}");
}

/// REQ: B16
#[test]
fn b16_key_to_call_t2_p99_below_budget_passes() {
    assert_eq!(judge("B16", "t2_p99_ms", 1.5 * 0.9), Verdict::Pass);
}

/// REQ: B16
#[test]
fn b16_key_to_call_t2_p99_above_budget_fails() {
    let verdict = judge("B16", "t2_p99_ms", 1.5 * 1.01);
    assert_eq!(verdict, Verdict::Fail);
}

/// REQ: B16
#[test]
#[ignore = "budget: run on the reference machine"]
fn b16_key_to_call_t2_p99_measured_within_budget() {
    let result = run_benchmark("B16").expect("benchmark runs");
    let value = *result.metrics.get("t2_p99_ms").expect("metric recorded");
    assert_eq!(judge("B16", "t2_p99_ms", value), Verdict::Pass, "t2_p99_ms={value}");
}

/// REQ: B17
#[test]
fn b17_key_to_pty_p99_below_budget_passes() {
    assert_eq!(judge("B17", "p99_ms", 5.0 * 0.9), Verdict::Pass);
}

/// REQ: B17
#[test]
fn b17_key_to_pty_p99_above_budget_fails() {
    let verdict = judge("B17", "p99_ms", 5.0 * 1.01);
    assert_eq!(verdict, Verdict::Fail);
}

/// REQ: B17
#[test]
#[ignore = "budget: run on the reference machine"]
fn b17_key_to_pty_p99_measured_within_budget() {
    let result = run_benchmark("B17").expect("benchmark runs");
    let value = *result.metrics.get("p99_ms").expect("metric recorded");
    assert_eq!(judge("B17", "p99_ms", value), Verdict::Pass, "p99_ms={value}");
}

/// REQ: B18
#[test]
fn b18_helper_round_trip_median_below_budget_passes() {
    assert_eq!(judge("B18", "median_ms", 0.2 * 0.9), Verdict::Pass);
}

/// REQ: B18
#[test]
fn b18_helper_round_trip_median_above_budget_fails() {
    let verdict = judge("B18", "median_ms", 0.2 * 1.01);
    assert_eq!(verdict, Verdict::Fail);
}

/// REQ: B18
#[test]
#[ignore = "budget: run on the reference machine"]
fn b18_helper_round_trip_median_measured_within_budget() {
    let result = run_benchmark("B18").expect("benchmark runs");
    let value = *result.metrics.get("median_ms").expect("metric recorded");
    assert_eq!(judge("B18", "median_ms", value), Verdict::Pass, "median_ms={value}");
}

/// REQ: B18
#[test]
fn b18_helper_round_trip_p99_below_budget_passes() {
    assert_eq!(judge("B18", "p99_ms", 0.5 * 0.9), Verdict::Pass);
}

/// REQ: B18
#[test]
fn b18_helper_round_trip_p99_above_budget_fails() {
    let verdict = judge("B18", "p99_ms", 0.5 * 1.01);
    assert_eq!(verdict, Verdict::Fail);
}

/// REQ: B18
#[test]
#[ignore = "budget: run on the reference machine"]
fn b18_helper_round_trip_p99_measured_within_budget() {
    let result = run_benchmark("B18").expect("benchmark runs");
    let value = *result.metrics.get("p99_ms").expect("metric recorded");
    assert_eq!(judge("B18", "p99_ms", value), Verdict::Pass, "p99_ms={value}");
}

/// REQ: B19
#[test]
fn b19_pty_frame_delay_full_queue_p99_below_budget_passes() {
    assert_eq!(judge("B19", "p99_ms", 1.0 * 0.9), Verdict::Pass);
}

/// REQ: B19
#[test]
fn b19_pty_frame_delay_full_queue_p99_above_budget_fails() {
    let verdict = judge("B19", "p99_ms", 1.0 * 1.01);
    assert_eq!(verdict, Verdict::Fail);
}

/// REQ: B19
#[test]
#[ignore = "budget: run on the reference machine"]
fn b19_pty_frame_delay_full_queue_p99_measured_within_budget() {
    let result = run_benchmark("B19").expect("benchmark runs");
    let value = *result.metrics.get("p99_ms").expect("metric recorded");
    assert_eq!(judge("B19", "p99_ms", value), Verdict::Pass, "p99_ms={value}");
}

/// REQ: B20
#[test]
fn b20_decoration_to_mirror_p99_below_budget_passes() {
    assert_eq!(judge("B20", "p99_ms", 5.0 * 0.9), Verdict::Pass);
}

/// REQ: B20
#[test]
fn b20_decoration_to_mirror_p99_above_budget_fails() {
    let verdict = judge("B20", "p99_ms", 5.0 * 1.01);
    assert_eq!(verdict, Verdict::Fail);
}

/// REQ: B20
#[test]
#[ignore = "budget: run on the reference machine"]
fn b20_decoration_to_mirror_p99_measured_within_budget() {
    let result = run_benchmark("B20").expect("benchmark runs");
    let value = *result.metrics.get("p99_ms").expect("metric recorded");
    assert_eq!(judge("B20", "p99_ms", value), Verdict::Pass, "p99_ms={value}");
}

/// REQ: B22
#[test]
fn b22_session_added_rss_below_budget_passes() {
    assert_eq!(judge("B22", "added_rss_mib", 120.0 * 0.9), Verdict::Pass);
}

/// REQ: B22
#[test]
fn b22_session_added_rss_above_budget_fails() {
    let verdict = judge("B22", "added_rss_mib", 120.0 * 1.01);
    assert_eq!(verdict, Verdict::Fail);
}

/// REQ: B22
#[test]
#[ignore = "budget: run on the reference machine"]
fn b22_session_added_rss_measured_within_budget() {
    let result = run_benchmark("B22").expect("benchmark runs");
    let value = *result.metrics.get("added_rss_mib").expect("metric recorded");
    assert_eq!(judge("B22", "added_rss_mib", value), Verdict::Pass, "added_rss_mib={value}");
}

/// REQ: B23, CLI-COLD
#[test]
fn b23_cli_cold_run_below_budget_passes() {
    assert_eq!(judge("B23", "cold_ms", 60.0 * 0.9), Verdict::Pass);
}

/// REQ: B23, CLI-COLD
#[test]
fn b23_cli_cold_run_above_budget_fails() {
    let verdict = judge("B23", "cold_ms", 60.0 * 1.01);
    assert_eq!(verdict, Verdict::Fail);
}

/// REQ: B23, CLI-COLD
#[test]
#[ignore = "budget: run on the reference machine"]
fn b23_cli_cold_run_measured_within_budget() {
    let result = run_benchmark("B23").expect("benchmark runs");
    let value = *result.metrics.get("cold_ms").expect("metric recorded");
    assert_eq!(judge("B23", "cold_ms", value), Verdict::Pass, "cold_ms={value}");
}

/// REQ: B24
#[test]
fn b24_idle_cpu_below_budget_passes() {
    assert_eq!(judge("B24", "cpu_percent", 0.5 * 0.9), Verdict::Pass);
}

/// REQ: B24
#[test]
fn b24_idle_cpu_above_budget_fails() {
    let verdict = judge("B24", "cpu_percent", 0.5 * 1.01);
    assert_eq!(verdict, Verdict::Fail);
}

/// REQ: B24
#[test]
#[ignore = "budget: run on the reference machine"]
fn b24_idle_cpu_measured_within_budget() {
    let result = run_benchmark("B24").expect("benchmark runs");
    let value = *result.metrics.get("cpu_percent").expect("metric recorded");
    assert_eq!(judge("B24", "cpu_percent", value), Verdict::Pass, "cpu_percent={value}");
}

/// REQ: B24
#[test]
fn b24_idle_wakeups_below_budget_passes() {
    assert_eq!(judge("B24", "wakeups_per_s", 100.0 * 0.9), Verdict::Pass);
}

/// REQ: B24
#[test]
fn b24_idle_wakeups_above_budget_fails() {
    let verdict = judge("B24", "wakeups_per_s", 100.0 * 1.01);
    assert_eq!(verdict, Verdict::Fail);
}

/// REQ: B24
#[test]
#[ignore = "budget: run on the reference machine"]
fn b24_idle_wakeups_measured_within_budget() {
    let result = run_benchmark("B24").expect("benchmark runs");
    let value = *result.metrics.get("wakeups_per_s").expect("metric recorded");
    assert_eq!(judge("B24", "wakeups_per_s", value), Verdict::Pass, "wakeups_per_s={value}");
}

/// REQ: B21
#[test]
fn b21_helper_start_is_record_only() {
    assert_eq!(judge("B21", "median_ms", 1.0), Verdict::Recorded);
    assert_eq!(judge("B21", "median_ms", 10_000.0), Verdict::Recorded);
}

/// REQ: B21
#[test]
#[ignore = "budget: run on the reference machine"]
fn b21_helper_start_2mib_component_recorded_median_under_80ms() {
    let result = run_benchmark("B21").expect("benchmark runs");
    let median = *result.metrics.get("median_ms").expect("metric recorded");
    assert!(median < 80.0, "median_ms={median}");
}

/// REQ: B2
#[test]
fn b02_javascript_instantiation_is_record_only() {
    assert_eq!(judge("B2", "js_wall_ms", 1_000.0), Verdict::Recorded);
}

/// REQ: B3
#[test]
fn b03_javascript_route_is_recorded_without_v8_baseline() {
    assert_eq!(judge("B3", "js_vs_v8_baseline_absent_ms", 5_000.0), Verdict::Recorded);
}

/// REQ: B3
#[test]
fn b03_javascript_route_within_1_1x_of_v8_where_baseline_exists() {
    assert_eq!(judge("B3", "js_vs_v8_ratio", 1.1 * 0.9), Verdict::Pass);
    assert_eq!(judge("B3", "js_vs_v8_ratio", 1.1 * 1.01), Verdict::Fail);
}

/// REQ: B6
#[test]
fn b06_selection_benchmark_has_no_fail_verdict_for_option_comparison() {
    assert_eq!(judge("B6", "binary_size_delta_mib", 500.0), Verdict::Recorded);
}

/// REQ: B1
#[test]
fn b01_unknown_benchmark_id_is_not_judged_as_pass() {
    assert_ne!(judge("B99", "median_ns", 0.0), Verdict::Pass);
}

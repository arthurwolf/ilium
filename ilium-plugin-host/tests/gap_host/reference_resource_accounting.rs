//! Manual page `reference-resource-accounting`: memory sampling and peak, the memory-high log and
//! out-of-memory trap, CPU accounting, open-handle limits, event queue charging, the top view and
//! the telemetry memory bound (HST-628, 629, 630, 631, 633, 634, 644).

use ilium_plugin_api::Side;

use super::common::{install_ok, pkg, rig, NOTES, OTHER};

/// REQ: HST-628
#[test]
fn hst_628_memory_is_sampled_each_second_and_peak_resets_only_on_instance_restart() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.activate(NOTES);
    rig.advance_clock(std::time::Duration::from_secs(3));
    let peak_before = rig.memory_bytes_peak(NOTES, Side::Server);
    assert!(
        rig.memory_samples_taken(NOTES) >= 3,
        "sampled at least once per second"
    );
    assert!(
        rig.memory_bytes_current(NOTES, Side::Server) > 0,
        "memory_bytes_current exposed"
    );
    rig.advance_clock(std::time::Duration::from_secs(1));
    assert_eq!(
        rig.memory_bytes_peak(NOTES, Side::Server),
        peak_before,
        "peak is not reset by time"
    );
    rig.restart_instance(NOTES);
    assert_eq!(
        rig.memory_bytes_peak(NOTES, Side::Server),
        0,
        "peak reset on instance restart"
    );
}

/// REQ: HST-629
#[test]
fn hst_629_memory_at_90_percent_logs_high_once_per_minute_and_out_of_memory_traps_only_that_plugin()
{
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    install_ok(&rig, &pkg(OTHER, "1.0.0"));
    rig.use_memory_fraction(NOTES, 0.91);
    rig.use_memory_fraction(NOTES, 0.92);
    assert_eq!(
        rig.log_lines_with_code("host.memory.high").len(),
        1,
        "host.memory.high once per 60 seconds"
    );
    rig.allocate_past_limit(NOTES);
    assert_eq!(
        rig.last_trap_code(NOTES, Side::Server),
        Some("out-of-memory".to_owned()),
        "allocation failure traps"
    );
    assert_eq!(
        rig.state_of(OTHER, Side::Server).to_string(),
        "running",
        "other plugins are unaffected"
    );
}

/// REQ: HST-630
#[test]
fn hst_630_cpu_ms_fields_use_thread_cpu_time_at_1_ms_resolution() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.spin_cpu(NOTES, std::time::Duration::from_millis(200));
    let stats = rig.plugin_side_stats(NOTES, Side::Server);
    let total = stats["cpu_ms_total"]
        .as_u64()
        .expect("cpu_ms_total present");
    assert!(
        (150..=400).contains(&total),
        "thread CPU time near 200 ms: {total}"
    );
    assert!(
        stats.get("cpu_ms_last_minute").is_some(),
        "cpu_ms_last_minute present"
    );
    assert_eq!(rig.cpu_resolution_ms(), 1, "resolution 1 ms");
}

/// REQ: HST-631
#[test]
fn hst_631_open_handles_are_counted_per_kind_refused_over_limit_and_released_on_stop() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.activate(NOTES);
    let limit = rig.handle_limit_per_side();
    for _ in 0..limit {
        rig.plugin_open_handle(NOTES, "file")
            .expect("open within the limit");
    }
    let refused = rig
        .plugin_open_handle(NOTES, "socket")
        .expect_err("open over the limit refused");
    assert_eq!(refused.code.as_str(), "quota-exceeded");
    assert_eq!(
        rig.handles_open_by_kind(NOTES, Side::Server, "file"),
        limit,
        "handles_open labelled per kind"
    );
    rig.stop_instance(NOTES);
    assert_eq!(
        rig.handles_open_total(NOTES, Side::Server),
        0,
        "all handles released within 1 second of stop"
    );
}

/// REQ: HST-633
#[test]
fn hst_633_event_queue_is_charged_per_byte_plus_64_per_event_and_drops_oldest_at_limit() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.activate(NOTES);
    rig.queue_events_without_consuming(NOTES, 3, 1_000);
    assert_eq!(
        rig.queue_charge_bytes(NOTES, Side::Server),
        3 * (1_000 + 64),
        "1 byte per payload byte plus 64 per event"
    );
    rig.queue_events_without_consuming(NOTES, 1_000_000, 1_000);
    assert!(
        rig.queue_dropped_oldest_count(NOTES) > 0,
        "drop-oldest applied at the queue limit"
    );
}

/// REQ: HST-634
#[test]
fn hst_634_top_shows_10_largest_sides_with_required_columns_and_refreshes_at_most_once_per_second()
{
    let rig = rig();
    rig.install_many(12);
    let output = rig.cli(&["plugin", "top", "--once"]);
    let header = output.stdout_raw.lines().next().expect("a header line");
    for column in [
        "plugin",
        "side",
        "cpu_pct",
        "memory_bytes",
        "handles",
        "calls_per_second",
    ] {
        assert!(header.contains(column), "column {column} shown");
    }
    assert_eq!(
        output.stdout_raw.lines().count(),
        1 + 10,
        "10 largest plugin sides shown"
    );
    assert!(
        rig.top_refresh_interval_ms() >= 1_000,
        "refreshed at most once per second"
    );
}

/// REQ: HST-644
#[test]
fn hst_644_telemetry_memory_is_bounded_to_16_mib_plus_64_kib_per_side_and_printed_by_stats_host() {
    let rig = rig();
    rig.install_many(4);
    rig.flood_telemetry_buffers();
    let bound = 16 * 1024 * 1024 + 64 * 1024 * 4 * 2;
    assert!(
        rig.telemetry_memory_bytes() <= bound,
        "telemetry memory bounded"
    );
    let output = rig.cli(&["plugin", "stats", "--host"]);
    assert!(
        output.stdout_raw.contains("telemetry_bytes"),
        "stats --host prints the current value"
    );
}

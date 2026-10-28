use ilium_plugin_api::{Side, Tier};
use ilium_plugin_host::{calls::CallClass, telemetry::*};
use serde_json::json;
use std::time::Duration;

#[test]
fn near_misses_exclude_late_calls_and_expire_at_five_minutes() {
    let mut tracker = NearMissTracker::new();
    assert!(!tracker.record_call(0, 100, 79));
    assert!(!tracker.record_call(0, 100, 101));
    assert!(!tracker.record_call(0, 0, 0));
    for _ in 0..4 {
        assert!(tracker.record_call(1, 100, 80));
    }
    assert!(!tracker.is_degraded(1));
    assert!(tracker.record_call(1, 100, 100));
    assert!(tracker.is_degraded(300_000));
    assert_eq!(tracker.count_in_window(300_001), 0);
    assert!(!tracker.is_degraded(300_001));
}

#[test]
fn near_miss_comparison_does_not_overflow_large_durations() {
    let mut tracker = NearMissTracker::new();
    assert!(tracker.record_call(u64::MAX, u64::MAX, u64::MAX));
    assert_eq!(tracker.count_in_window(u64::MAX), 1);
    assert_eq!(tracker.count_in_window(u64::MAX - 1), 0);
}

#[test]
fn utf8_log_cuts_include_the_marker_inside_the_byte_budget() {
    let long = "é".repeat(LOG_MESSAGE_MAX_BYTES);
    let cut = truncate_message(&long);
    assert!(cut.len() <= LOG_MESSAGE_MAX_BYTES);
    assert!(cut.ends_with("…[truncated]"));
    assert!(cut.is_char_boundary(cut.len()));
    let wasi = cut_wasi_line(&long);
    assert!(wasi.len() <= LOG_MESSAGE_MAX_BYTES);
    assert!(wasi.ends_with("[cut]"));
    assert_eq!(
        truncate_message(&"x".repeat(LOG_MESSAGE_MAX_BYTES)).len(),
        LOG_MESSAGE_MAX_BYTES
    );
}

#[test]
fn ring_eviction_is_global_bounded_and_plugin_filter_keeps_order() {
    let mut ring = LogRing::new(2);
    for (plugin, message) in [
        ("example.first", "old"),
        ("example.other", "other"),
        ("example.first", "new"),
    ] {
        ring.push(LogLine {
            plugin: plugin.into(),
            level: "info".into(),
            message: message.into(),
        });
    }
    assert_eq!(ring.lines_of("example.first")[0].message, "new");
    assert_eq!(ring.lines_of("example.other")[0].message, "other");
    let mut zero = LogRing::new(0);
    zero.push(LogLine {
        plugin: "example.first".into(),
        level: "info".into(),
        message: "ignored".into(),
    });
    assert!(zero.lines_of("example.first").is_empty());
}

#[test]
fn log_columns_escape_line_breaks_and_keep_unicode() {
    assert_eq!(
        format_log_line(
            "2030-01-01 09:14:02",
            LogLevel::Info,
            LogSide::Server,
            "example.notes",
            "prêt\nnext"
        ),
        "2030-01-01 09:14:02 info  server example.notes prêt\\nnext"
    );
    let host = format_log_line(
        "2030-01-01 09:14:02",
        LogLevel::Error,
        LogSide::Host,
        "example.notes",
        "failure",
    );
    assert!(host.contains("error host   example.notes"));
    assert_eq!(LogConfig::default().rate_per_second, 200);
    assert_eq!(LogConfig::for_tier(Tier::T1).rate_per_second, 1000);
    assert_eq!(LogConfig::for_dev_install().threshold, LogLevel::Debug);
    assert_eq!(
        resolve_threshold(Some(LogLevel::Error), Some("trace"), true),
        LogLevel::Error
    );
    assert_eq!(
        resolve_threshold(None, Some("trace"), false),
        LogLevel::Trace
    );
}

#[test]
fn host_conditions_follow_manual_levels_and_reject_unknown_ids() {
    assert_eq!(
        host_line("example.notes", Side::Server, "host.late", "class=event")
            .unwrap()
            .level,
        "warn"
    );
    assert_eq!(
        host_line("example.notes", Side::Server, "helper.died", "")
            .unwrap()
            .level,
        "error"
    );
    assert!(host_line("example.notes", Side::Server, "invented.condition", "").is_err());
}

#[test]
fn host_limiter_admits_twenty_and_summarizes_actual_suppression_once() {
    let mut limiter = HostLogLimiter::new();
    for _ in 0..20 {
        assert!(limiter.admit(Side::Server, "host.late", 0));
    }
    for _ in 0..3 {
        assert!(!limiter.admit(Side::Server, "host.late", 1));
    }
    assert!(limiter.admit(Side::Client, "host.late", 1));
    assert!(limiter.admit(Side::Server, "helper.died", 1));
    assert!(limiter
        .take_suppressed_summary(Side::Server, "host.late", 59_999)
        .is_none());
    assert!(limiter.admit(Side::Server, "host.late", 60_000));
    assert_eq!(
        limiter
            .take_suppressed_summary(Side::Server, "host.late", 60_000)
            .as_deref(),
        Some("3 similar lines suppressed")
    );
    assert!(limiter
        .take_suppressed_summary(Side::Server, "host.late", 60_001)
        .is_none());
    assert!(!limiter.admit(Side::Server, "unknown", 60_001));
}

#[test]
fn slow_report_needs_strict_half_threshold_and_samples_all_top_calls() {
    let mut reporter = SlowCallReporter::new();
    reporter.record(
        Side::Server,
        CallClass::Event,
        Duration::from_millis(10),
        Duration::from_millis(100),
        0,
    );
    reporter.record(
        Side::Server,
        CallClass::Render,
        Duration::from_millis(9),
        Duration::from_millis(10),
        1,
    );
    reporter.record(
        Side::Server,
        CallClass::Command,
        Duration::from_millis(2),
        Duration::from_millis(50),
        2,
    );
    assert!(reporter.due_report(Side::Server, 59_999).is_none());
    let report = reporter.due_report(Side::Server, 60_000).unwrap();
    assert_eq!(report.top.len(), 3);
    assert_eq!(report.top[0], (CallClass::Event, Duration::from_millis(10)));
    assert!(reporter.due_report(Side::Server, 60_001).is_none());
    reporter.record(
        Side::Client,
        CallClass::Event,
        Duration::from_millis(10),
        Duration::from_millis(20),
        0,
    );
    assert!(reporter.due_report(Side::Client, 60_000).is_none());
}

#[test]
fn spans_are_disabled_by_default_and_explicitly_drained_without_payloads() {
    let mut off = SpanRecorder::default();
    assert!(!off.is_enabled());
    let span = Span {
        plugin: "example.notes".into(),
        side: Side::Server,
        class: CallClass::Event,
        queue_time: Duration::from_micros(2),
        run_time: Duration::from_micros(3),
        forwarded_imports: 1,
    };
    off.record(span.clone());
    assert!(off.spans().is_empty());
    let mut on = SpanRecorder::from_env(Some("1"));
    for _ in 0..TRACE_RING_CAPACITY + 1 {
        on.record(span.clone());
    }
    assert_eq!(on.spans().len(), TRACE_RING_CAPACITY);
    assert_eq!(on.dropped(), 1);
    assert_eq!(on.take_spans().len(), TRACE_RING_CAPACITY);
    assert!(on.spans().is_empty());
    let value = serde_json::to_value(span).unwrap();
    assert_eq!(value["queue_time_us"], 2);
    assert_eq!(value.as_object().unwrap().len(), 6);
}

#[test]
fn trace_retains_old_instance_records_and_restarts_per_instance_sequence() {
    let mut ring = TraceRing::new();
    assert_eq!(ring.begin_instance(), 1);
    assert_eq!(
        ring.push(
            10,
            TraceKind::Event,
            json!({"tags":["pane-created"],"payload":"secret"})
        ),
        1
    );
    assert_eq!(ring.begin_instance(), 2);
    assert_eq!(
        ring.push(20, TraceKind::Lifecycle, json!({"name":"activate"})),
        1
    );
    let records = ring.records();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].instance, 1);
    assert_eq!(records[1].instance, 2);
    assert_eq!(records[0].detail["payload"], "<redacted 6 bytes>");
    assert_eq!(records[0].detail["tags"], json!(["pane-created"]));
    for _ in 0..TRACE_RING_CAPACITY {
        ring.push(30, TraceKind::Event, json!({"count":1}));
    }
    assert_eq!(ring.records().len(), TRACE_RING_CAPACITY);
    assert!(ring.records().iter().all(|record| record.instance == 2));
}

#[test]
fn recursive_trace_redaction_counts_utf8_and_bytes_and_refuses_normal_raw_capture() {
    let value = redact_detail(
        &json!({"tag":"message","nested":{"payload":"é","data":[0,1,255]},"opaque":"hidden","count":3,"duration_ms":2}),
    );
    assert_eq!(value["nested"]["payload"], "<redacted 2 bytes>");
    assert_eq!(value["nested"]["data"], "<redacted 3 bytes>");
    assert_eq!(value["opaque"], "<redacted 6 bytes>");
    assert_eq!(value["tag"], "message");
    assert_eq!(value["count"], 3);
    let mut ring = TraceRing::new();
    assert_eq!(
        ring.push_unredacted(0, TraceKind::Event, json!({"payload":"secret"}), false)
            .unwrap_err()
            .code,
        "trace.unredacted-denied"
    );
    assert!(ring.records().is_empty());
    ring.push_unredacted(0, TraceKind::Event, json!({"payload":"secret"}), true)
        .unwrap();
    assert_eq!(ring.records()[0].detail["payload"], "secret");
}

#[test]
fn profiler_samples_every_call_with_real_percentiles_and_strict_budgets() {
    let mut profiler = Profiler::new();
    let empty = profiler.timing("on-events");
    assert_eq!(empty.calls, 0);
    assert!(empty.p50_ms.is_none());
    assert!(empty.p95_ms.is_none());
    for milliseconds in [2, 4, 20, 21] {
        profiler.record("on-events", Duration::from_millis(milliseconds));
    }
    let timing = profiler.timing("on-events");
    assert_eq!(timing.calls, 4);
    assert!((timing.total_ms - 47.0).abs() < 0.0001);
    assert!((timing.p50_ms.unwrap() - 12.0).abs() < 0.0001);
    assert_eq!(timing.over_budget, 1);
    assert_eq!(timing.max_ms, Some(21.0));
    assert_eq!(
        profiler
            .timing_with_budget("on-events", Duration::from_millis(3))
            .over_budget,
        3
    );
    // No performance claim: overhead is the actual measured recorder cost.
    assert!(profiler.overhead_fraction() >= 0.0);
}

#[test]
fn inspector_uses_cached_nodes_explicit_limits_and_does_not_mutate_selection() {
    let child = InspectNode {
        kind: "text".into(),
        key: Some("greeting".into()),
        text: Some("prêt".into()),
        style: Some("title".into()),
        selected: true,
        children: vec![],
    };
    let tree = InspectNode {
        kind: "stack".into(),
        key: None,
        text: None,
        style: None,
        selected: false,
        children: vec![child],
    };
    let before = tree.clone();
    let rendered = render_inspector_with_limits("panel main", &tree, 120, 10, 4096, 32);
    assert!(rendered.contains("key=greeting"));
    assert!(rendered.contains("prêt"));
    assert!(rendered.contains("<- selected"));
    assert!(rendered.contains("nodes 2 of 4096   depth 2 of 32"));
    assert_eq!(tree, before);
    assert!(render_inspector_with_limits("panel main", &tree, 0, 10, 4096, 32).is_empty());
}

#[test]
fn memory_warning_is_strict_and_binary_units_are_actual_figures() {
    let mut view = MemoryView {
        side: Side::Server,
        linear_bytes: 512,
        peak_bytes: 80,
        limit_bytes: 100,
        storage_bytes: 1024,
        instances: 2,
        restarts: 1,
    };
    assert!(!render_memory_view("example.notes", &view).contains("warning:"));
    view.peak_bytes = 81;
    let text = render_memory_view("example.notes", &view);
    assert!(text.contains("warning: peak above 80 percent of memory_mib"));
    assert!(text.contains("storage 1 KiB of 1 MiB"));
    assert_eq!(format_binary(512), "512 B");
    let lines = why_lines(&HealthInputs {
        calls: 6,
        near_misses: 5,
        quota_rejections_last_minute: 2,
        memory_peak_bytes: 91,
        memory_limit_bytes: 100,
    });
    assert_eq!(lines.len(), 3);
    assert!(lines[0].contains("5 near-misses in 6 calls"));
}

#[test]
fn fixed_counter_memory_reports_actual_inline_size_and_never_grows() {
    let telemetry = SideTelemetry::new();
    let bytes = std::mem::size_of::<SideTelemetry>();
    assert_eq!(telemetry.memory_bytes(), bytes);
    assert!(
        bytes <= TELEMETRY_MAX_BYTES_PER_SIDE,
        "actual telemetry size: {bytes}"
    );
    println!("{{\"type\":\"result\",\"counter_store_bytes\":{bytes}}}");
    for index in 0..10_000 {
        telemetry.record_trap(&format!("trap.code-{index}"), 1);
        telemetry.record_render(&format!("view-{index}"), Duration::from_millis(1), 1);
    }
    assert_eq!(telemetry.memory_bytes(), bytes);
    let buckets = telemetry.snapshot(2);
    let bucket = &buckets[0];
    let retained: u64 = (0..12)
        .map(|index| bucket.traps(&format!("trap.code-{index}")))
        .sum();
    assert_eq!(
        retained + bucket.traps(SideTelemetry::overflow_series_name()),
        10_000
    );
    assert_eq!(bucket.render_ms("view-0").iter().sum::<u64>(), 1);
    assert_eq!(
        bucket
            .render_ms(SideTelemetry::overflow_series_name())
            .iter()
            .sum::<u64>(),
        9_999
    );
}

#[test]
fn counter_rotation_requires_explicit_custody_and_preserves_retry() {
    const HOUR: u64 = 3_600_000;
    let mut telemetry = SideTelemetry::new();
    assert_eq!(
        telemetry.record_call(
            CallClass::Event,
            Duration::from_millis(1),
            Duration::from_millis(20),
            1
        ),
        CounterAdmission::Recorded
    );
    assert_eq!(
        telemetry.record_call(
            CallClass::Event,
            Duration::from_millis(1),
            Duration::from_millis(20),
            25 * HOUR
        ),
        CounterAdmission::ClockNotPrepared
    );
    assert_eq!(telemetry.refusal_counts(), (1, 0));
    assert_eq!(
        telemetry
            .snapshot(25 * HOUR)
            .iter()
            .map(|hour| hour.calls(CallClass::Event))
            .sum::<u64>(),
        0
    );
    telemetry.advance_clock(TelemetryTime(25 * HOUR));
    assert_eq!(
        telemetry.record_call(
            CallClass::Event,
            Duration::from_millis(1),
            Duration::from_millis(20),
            25 * HOUR
        ),
        CounterAdmission::Recorded
    );
    assert_eq!(telemetry.record_cpu_ms(1, 0), CounterAdmission::Expired);
    assert_eq!(telemetry.refusal_counts(), (1, 1));
    telemetry.advance_clock(TelemetryTime(0));
    assert_eq!(telemetry.snapshot(25 * HOUR)[0].calls(CallClass::Event), 1);
    assert!(telemetry
        .end_instance(25 * HOUR)
        .unwrap()
        .contains("2 calls"));
    assert!(telemetry.end_instance(25 * HOUR).is_none());
}

#[test]
fn counter_hour_rotation_keeps_all_retained_samples_over_sixty_hours() {
    const HOUR: u64 = 3_600_000;
    let mut telemetry = SideTelemetry::with_clock(TelemetryTime(100 * HOUR));
    for hour in 100..160 {
        telemetry.advance_clock(TelemetryTime(hour * HOUR));
        assert_eq!(
            telemetry.record_call(
                CallClass::Event,
                Duration::from_micros(100),
                Duration::from_millis(20),
                hour * HOUR + 1
            ),
            CounterAdmission::Recorded
        );
    }
    let buckets = telemetry.snapshot(159 * HOUR + 2);
    assert_eq!(buckets.len(), 25);
    assert_eq!(buckets.first().unwrap().hour_start_ms, 135 * HOUR);
    assert_eq!(buckets.last().unwrap().hour_start_ms, 159 * HOUR);
    assert_eq!(
        buckets
            .iter()
            .map(|hour| hour.calls(CallClass::Event))
            .sum::<u64>(),
        25
    );
}

#[test]
fn concurrent_counter_samples_and_histograms_are_not_lost() {
    let telemetry = std::sync::Arc::new(SideTelemetry::new());
    std::thread::scope(|scope| {
        for _ in 0..8 {
            let telemetry = std::sync::Arc::clone(&telemetry);
            scope.spawn(move || {
                for index in 0..5_000 {
                    assert_eq!(
                        telemetry.record_call(
                            CallClass::Event,
                            Duration::from_millis(16),
                            Duration::from_millis(20),
                            index
                        ),
                        CounterAdmission::Recorded
                    );
                }
            });
        }
    });
    let buckets = telemetry.snapshot(10_000);
    assert_eq!(buckets[0].calls(CallClass::Event), 40_000);
    assert_eq!(
        buckets[0]
            .duration_histogram(CallClass::Event)
            .iter()
            .sum::<u64>(),
        40_000
    );
    assert_eq!(buckets[0].near_misses(CallClass::Event), 40_000);
}

#[test]
fn counter_saturation_and_exact_near_miss_boundaries() {
    let telemetry = SideTelemetry::new();
    telemetry.record_cpu_ms(u64::MAX, 0);
    telemetry.record_cpu_ms(1, 0);
    telemetry.record_event_dropped("node-renamed", u64::MAX, 0);
    telemetry.record_event_dropped("node-renamed", 1, 0);
    telemetry.record_queue_depth(u64::MAX, 0);
    telemetry.record_queue_depth(0, 0);
    telemetry.record_call(
        CallClass::Event,
        Duration::from_nanos(79),
        Duration::from_nanos(100),
        0,
    );
    telemetry.record_call(
        CallClass::Event,
        Duration::from_nanos(80),
        Duration::from_nanos(100),
        0,
    );
    telemetry.record_call(
        CallClass::Event,
        Duration::from_nanos(100),
        Duration::from_nanos(100),
        0,
    );
    telemetry.record_call(
        CallClass::Event,
        Duration::from_nanos(101),
        Duration::from_nanos(100),
        0,
    );
    telemetry.record_call(CallClass::Event, Duration::ZERO, Duration::ZERO, 0);
    let bucket = &telemetry.snapshot(1)[0];
    assert_eq!(bucket.cpu_ms(), u64::MAX);
    assert_eq!(bucket.events_dropped("node-renamed"), u64::MAX);
    assert_eq!(bucket.queue_depth_high_water(), u64::MAX);
    assert_eq!(bucket.near_misses(CallClass::Event), 2);
    assert_eq!(bucket.breaches(CallClass::Event, BreachKind::Soft), 0);
}

#[test]
fn counter_name_overflow_is_visible_and_never_retains_paths_or_payloads() {
    let telemetry = SideTelemetry::new();
    assert_eq!(
        telemetry.record_trap("/private/secret", 1),
        CounterAdmission::Aggregated
    );
    assert_eq!(
        telemetry.record_rate_limited("https://secret.example", 1),
        CounterAdmission::Aggregated
    );
    let buckets = telemetry.snapshot(1);
    assert_eq!(buckets[0].traps(SideTelemetry::overflow_series_name()), 1);
    let json = serde_json::to_string(&buckets).unwrap();
    assert!(!json.contains("private"));
    assert!(!json.contains("secret"));
    assert!(!json.contains("https"));
}

#[test]
fn telemetry_registry_separates_sides_and_refuses_even_self_raw_reads() {
    let registry = TelemetryRegistry::new();
    registry.side("example.notes", Side::Server).record_call(
        CallClass::Event,
        Duration::from_millis(16),
        Duration::from_millis(20),
        1,
    );
    registry.side("example.notes", Side::Client).record_call(
        CallClass::Render,
        Duration::from_millis(1),
        Duration::from_millis(10),
        1,
    );
    assert_eq!(
        registry
            .read(
                &Reader::Plugin("example.notes".into()),
                "example.notes",
                Side::Server,
                2
            )
            .unwrap_err()
            .code,
        "not-permitted"
    );
    assert!(registry
        .read(&Reader::Kernel, "example.notes", Side::Scene, 2)
        .unwrap()
        .is_empty());
    assert_eq!(
        registry
            .plugin_info("example.notes", "example.notes", false, 2)
            .unwrap(),
        Summary {
            calls: 2,
            near_misses: 1
        }
    );
    assert!(registry
        .plugin_info("other.plugin", "example.notes", false, 2)
        .is_err());
    assert_eq!(
        registry
            .plugin_info("other.plugin", "example.notes", true, 2)
            .unwrap()
            .calls,
        2
    );
}

#[test]
fn registry_clock_maintenance_requires_releasing_actual_recording_handles() {
    const HOUR: u64 = 3_600_000;
    let registry = TelemetryRegistry::new();
    let handle = registry.side("example.notes", Side::Server);
    assert_eq!(
        registry
            .advance_side_clock("example.notes", Side::Server, TelemetryTime(25 * HOUR))
            .unwrap_err()
            .code,
        "telemetry.handles-active"
    );
    assert_eq!(
        handle.record_cpu_ms(9, 25 * HOUR),
        CounterAdmission::ClockNotPrepared
    );
    drop(handle);
    registry
        .advance_side_clock("example.notes", Side::Server, TelemetryTime(25 * HOUR))
        .unwrap();
    let handle = registry.side("example.notes", Side::Server);
    assert_eq!(
        handle.record_cpu_ms(9, 25 * HOUR),
        CounterAdmission::Recorded
    );
    assert_eq!(
        registry
            .read(&Reader::Kernel, "example.notes", Side::Server, 25 * HOUR)
            .unwrap()[0]
            .cpu_ms(),
        9
    );
    assert_eq!(handle.refusal_counts(), (1, 0));
}

const LOG_AT: &str = "2030-01-01 09:14:02";

#[test]
fn persistent_log_reopens_and_shares_real_rate_and_suppression_state() {
    let directory = tempfile::tempdir().unwrap();
    let config = LogConfig {
        threshold: LogLevel::Info,
        rate_per_second: 2,
    };
    let mut first = PluginLog::open(directory.path(), "example.notes", config.clone()).unwrap();
    let mut second = PluginLog::open(directory.path(), "example.notes", config.clone()).unwrap();
    assert!(first.write_plugin(LOG_AT, 0, Side::Server, LogLevel::Info, "one"));
    assert!(second.write_plugin(LOG_AT, 1, Side::Client, LogLevel::Info, "two"));
    assert!(!first.write_plugin(LOG_AT, 2, Side::Scene, LogLevel::Info, "three"));
    assert!(!second.write_plugin(LOG_AT, 3, Side::Server, LogLevel::Info, "four"));
    drop(first);
    drop(second);
    let mut reopened = PluginLog::open(directory.path(), "example.notes", config).unwrap();
    reopened.tick(LOG_AT, 1000);
    reopened.tick(LOG_AT, 1001);
    assert!(reopened.write_plugin(LOG_AT, 1002, Side::Server, LogLevel::Info, "next"));
    let text = std::fs::read_to_string(directory.path().join("example.notes.log")).unwrap();
    assert_eq!(text.lines().count(), 4);
    assert_eq!(
        text.matches("2 lines dropped in the last second").count(),
        1
    );
    assert!(!text.contains("three"));
    assert!(!text.contains("four"));
    assert!(reopened.take_error().is_none());
}

#[test]
fn persistent_log_redacts_secrets_but_keeps_intentional_plugin_data() {
    let directory = tempfile::tempdir().unwrap();
    let mut log = PluginLog::open(directory.path(), "example.notes", LogConfig::default()).unwrap();
    log.set_secrets(&["secret-token".into()]);
    assert!(log.write_plugin(
        LOG_AT,
        0,
        Side::Server,
        LogLevel::Info,
        "intentional payload secret-token"
    ));
    assert!(log.write_host(LOG_AT, 0, LogLevel::Warn, "status secret-token"));
    assert!(!format!("{log:?}").contains("secret-token"));
    let text = std::fs::read_to_string(directory.path().join("example.notes.log")).unwrap();
    assert!(text.contains("intentional payload <redacted>"));
    assert!(!text.contains("secret-token"));
    assert!(!log.write_plugin(
        "2030-01-01\n09:14:02",
        1,
        Side::Server,
        LogLevel::Info,
        "forged"
    ));
    assert_eq!(log.take_error().unwrap().code, "log.time-invalid");
}

#[test]
fn persistent_log_rotates_real_files_and_reopens_the_active_generation() {
    let directory = tempfile::tempdir().unwrap();
    let mut first =
        PluginLog::open(directory.path(), "example.notes", LogConfig::default()).unwrap();
    let mut second =
        PluginLog::open(directory.path(), "example.notes", LogConfig::default()).unwrap();
    std::fs::write(
        directory.path().join("example.notes.log"),
        vec![b'x'; LOG_ROTATE_BYTES as usize - 2],
    )
    .unwrap();
    assert!(first.write_host(LOG_AT, 0, LogLevel::Info, "crosses rotation"));
    assert!(directory.path().join("example.notes.log.1").exists());
    assert!(second.write_host(LOG_AT, 1, LogLevel::Info, "new generation"));
    let active = std::fs::read_to_string(directory.path().join("example.notes.log")).unwrap();
    assert!(active.contains("new generation"));
    assert!(!active.contains("crosses rotation"));
    for _ in 0..5 {
        std::fs::write(
            directory.path().join("example.notes.log"),
            vec![b'x'; LOG_ROTATE_BYTES as usize - 2],
        )
        .unwrap();
        assert!(first.write_host(LOG_AT, 2, LogLevel::Info, "rotate"));
    }
    assert!(directory.path().join("example.notes.log.4").exists());
    assert!(!directory.path().join("example.notes.log.5").exists());
}

#[test]
fn log_retention_persists_actual_expiry_and_reinstall_cancels_it() {
    let directory = tempfile::tempdir().unwrap();
    let mut log = PluginLog::open(directory.path(), "example.notes", LogConfig::default()).unwrap();
    assert!(log.write_host(LOG_AT, 0, LogLevel::Info, "kept"));
    try_mark_uninstalled(directory.path(), "example.notes", 1000).unwrap();
    assert!(
        try_purge_expired_logs(directory.path(), 1000 + 7 * 86_400_000 - 1)
            .unwrap()
            .is_empty()
    );
    try_mark_reinstalled(directory.path(), "example.notes").unwrap();
    assert!(try_purge_expired_logs(directory.path(), 10 * 86_400_000)
        .unwrap()
        .is_empty());
    assert!(directory.path().join("example.notes.log").exists());
    try_mark_uninstalled(directory.path(), "example.notes", 1000).unwrap();
    assert_eq!(
        try_purge_expired_logs(directory.path(), 1000 + 7 * 86_400_000).unwrap(),
        ["example.notes"]
    );
    assert!(!directory.path().join("example.notes.log").exists());
    assert!(directory.path().join(".example.notes.log.lock").exists());
}

#[test]
fn malformed_retention_state_preserves_logs_and_original_bytes() {
    let directory = tempfile::tempdir().unwrap();
    PluginLog::open(directory.path(), "example.notes", LogConfig::default()).unwrap();
    let mark = directory.path().join(".example.notes.log-uninstalled.json");
    std::fs::write(&mark, "not-json").unwrap();
    assert!(try_purge_expired_logs(directory.path(), u64::MAX).is_err());
    assert!(directory.path().join("example.notes.log").exists());
    assert_eq!(std::fs::read_to_string(mark).unwrap(), "not-json");
    assert!(PluginLog::open(directory.path(), "../escaped", LogConfig::default()).is_err());
}

#[cfg(unix)]
#[test]
fn plugin_log_refuses_symlinked_files_and_ancestors() {
    let directory = tempfile::tempdir().unwrap();
    let outside = directory.path().join("original");
    std::fs::write(&outside, "authored original").unwrap();
    let logs = directory.path().join("logs");
    std::fs::create_dir(&logs).unwrap();
    std::os::unix::fs::symlink(&outside, logs.join("example.notes.log")).unwrap();
    assert!(PluginLog::open(&logs, "example.notes", LogConfig::default()).is_err());
    assert_eq!(
        std::fs::read_to_string(&outside).unwrap(),
        "authored original"
    );
    let alias = directory.path().join("alias");
    std::os::unix::fs::symlink(&logs, &alias).unwrap();
    assert!(PluginLog::open(&alias, "example.notes", LogConfig::default()).is_err());
}

#[test]
fn log_session_paths_are_scoped_and_cannot_escape_the_log_root() {
    let root = std::path::Path::new("/state");
    let first = plugin_log_dir(root, std::path::Path::new("/project/one"), "../../session");
    let second = plugin_log_dir(root, std::path::Path::new("/project/two"), "../../session");
    assert!(first.starts_with(root.join("plugins/logs")));
    assert_ne!(first, second);
    assert!(!first
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir)));
}

#[derive(Default)]
struct BoundaryDevBackend {
    release: bool,
    replacement_allowed: bool,
    outcomes: std::sync::Mutex<std::collections::VecDeque<RebuildOutcome>>,
    steps: std::sync::Mutex<Vec<String>>,
}
impl DevBackend for BoundaryDevBackend {
    fn release_installed(&self, _: &str) -> bool {
        self.release
    }
    fn replace_release(&self, _: &str) -> Result<(), Refusal> {
        if !self.replacement_allowed {
            return Err(Refusal {
                code: "dev.replace-refused".into(),
                message: "actual installer refused".into(),
            });
        }
        self.steps.lock().unwrap().push("replace".into());
        Ok(())
    }
    fn build(&self) -> RebuildOutcome {
        self.outcomes.lock().unwrap().pop_front().unwrap()
    }
    fn deactivate(&self, reason: &str) {
        self.steps
            .lock()
            .unwrap()
            .push(format!("deactivate:{reason}"));
    }
    fn load(&self, version: &str) {
        self.steps.lock().unwrap().push(format!("load:{version}"));
    }
    fn activate(&self) {
        self.steps.lock().unwrap().push("activate".into());
    }
}

#[test]
fn development_loop_requires_real_roots_and_rebuilds_before_runtime_changes() {
    let directory = tempfile::tempdir().unwrap();
    let backend = std::sync::Arc::new(BoundaryDevBackend::default());
    let mut missing = DevLoop::new(backend.clone());
    assert_eq!(
        missing.start("example.notes", false).unwrap_err().code,
        "dev.context-required"
    );
    let roots = DevDataRoots {
        dev: directory.path().join("dev"),
        release: directory.path().join("release"),
    };
    let mut dev = DevLoop::with_data_roots(backend.clone(), roots);
    let install = dev.start("example.notes", false).unwrap();
    assert!(install.data_dir.is_dir());
    assert_ne!(install.data_dir, install.release_data_dir);
    std::fs::write(install.data_dir.join("retained"), "authored storage").unwrap();
    backend.outcomes.lock().unwrap().extend([
        RebuildOutcome::Built {
            version: "0.1.0".into(),
        },
        RebuildOutcome::Failed {
            errors: vec!["real build failure".into()],
        },
    ]);
    assert!(dev.rebuild().reloaded);
    let report = dev.rebuild();
    assert!(!report.reloaded);
    assert_eq!(report.running_version, "0.1.0");
    assert_eq!(report.errors, ["real build failure"]);
    assert_eq!(
        *backend.steps.lock().unwrap(),
        ["deactivate:upgrade", "load:0.1.0", "activate"]
    );
    assert_eq!(
        std::fs::read_to_string(install.data_dir.join("retained")).unwrap(),
        "authored storage"
    );
}

#[test]
fn development_replacement_requires_actual_backend_success() {
    let directory = tempfile::tempdir().unwrap();
    let backend = std::sync::Arc::new(BoundaryDevBackend {
        release: true,
        ..Default::default()
    });
    let mut dev = DevLoop::with_data_roots(
        backend,
        DevDataRoots {
            dev: directory.path().join("dev"),
            release: directory.path().join("release"),
        },
    );
    assert_eq!(
        dev.start("example.notes", false).unwrap_err().code,
        "dev.id-installed"
    );
    assert_eq!(
        dev.start("example.notes", true).unwrap_err().code,
        "dev.replace-refused"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn local_log_timestamp_uses_the_actual_operating_system_timezone() {
    let output = std::process::Command::new("date")
        .args(["-d", "@0", "+%Y-%m-%d %H:%M:%S"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let expected = String::from_utf8(output.stdout).unwrap();
    assert_eq!(
        try_format_local_time(std::time::UNIX_EPOCH).unwrap(),
        expected.trim_end()
    );
}

#[test]
fn malformed_retention_mark_does_not_block_other_plugins_expiring() {
    let directory = tempfile::tempdir().unwrap();
    PluginLog::open(directory.path(), "example.bad", LogConfig::default()).unwrap();
    PluginLog::open(directory.path(), "example.expired", LogConfig::default()).unwrap();
    std::fs::write(
        directory.path().join(".example.bad.log-uninstalled.json"),
        "broken original",
    )
    .unwrap();
    try_mark_uninstalled(directory.path(), "example.expired", 0).unwrap();
    let report = purge_expired_logs_report(directory.path(), 7 * 86_400_000).unwrap();
    assert_eq!(report.removed, ["example.expired"]);
    assert_eq!(report.errors.len(), 1);
    assert!(directory.path().join("example.bad.log").exists());
    assert!(!directory.path().join("example.expired.log").exists());
}

#[test]
fn persisted_log_clock_regression_is_refused_until_host_supplies_new_epoch() {
    let directory = tempfile::tempdir().unwrap();
    let config = LogConfig {
        threshold: LogLevel::Info,
        rate_per_second: 1,
    };
    let mut log = PluginLog::open(directory.path(), "example.notes", config.clone()).unwrap();
    assert!(log.write_plugin(LOG_AT, 10_000, Side::Server, LogLevel::Info, "old epoch"));
    assert!(!log.write_plugin(LOG_AT, 10_001, Side::Server, LogLevel::Info, "lost"));
    drop(log);
    let mut log = PluginLog::open(directory.path(), "example.notes", config).unwrap();
    assert!(!log.write_plugin(LOG_AT, 0, Side::Server, LogLevel::Info, "unproven reset"));
    assert_eq!(log.take_error().unwrap().code, "log.clock-regressed");
    log.begin_rate_clock_epoch(LOG_AT, 0).unwrap();
    assert!(log.write_plugin(LOG_AT, 0, Side::Server, LogLevel::Info, "new epoch"));
    let text = std::fs::read_to_string(directory.path().join("example.notes.log")).unwrap();
    assert_eq!(
        text.matches("1 lines dropped in the last second").count(),
        1
    );
    assert!(!text.contains("unproven reset"));
}

#[test]
fn persistent_log_preserves_an_unterminated_previous_append() {
    let directory = tempfile::tempdir().unwrap();
    let mut log = PluginLog::open(directory.path(), "example.notes", LogConfig::default()).unwrap();
    let path = directory.path().join("example.notes.log");
    std::fs::write(&path, "previous authored partial").unwrap();
    assert!(log.write_host(LOG_AT, 0, LogLevel::Info, "new entry"));
    let text = std::fs::read_to_string(path).unwrap();
    assert!(text.starts_with("previous authored partial\n2030-01-01"));
    assert_eq!(text.lines().count(), 2);
}

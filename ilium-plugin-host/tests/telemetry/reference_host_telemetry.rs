//! reference-host-telemetry.md: counters, retention, host log lines, tracing (HST-350..360, 362, 363).

use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use ilium_plugin_api::Side;
use ilium_plugin_host::calls::CallClass;
use ilium_plugin_host::telemetry::{
    host_line, why_lines, BreachKind, HealthInputs, HostLogLimiter, Reader, SideTelemetry,
    SlowCallReporter, Span, SpanRecorder, TelemetryRegistry, COUNTER_NAMES,
    DURATION_BUCKET_BOUNDS_MS, TELEMETRY_MAX_BYTES_PER_SIDE,
};

use crate::common::{ms, us, ALL_CLASSES, HOUR_MS};

/// Counts the socket file descriptors of this process.
fn open_socket_count() -> usize {
    let Ok(entries) = std::fs::read_dir("/proc/self/fd") else {
        return 0;
    };
    entries
        .flatten()
        .filter(|entry| {
            std::fs::read_link(entry.path())
                .map(|target| target.to_string_lossy().starts_with("socket:"))
                .unwrap_or(false)
        })
        .count()
}

// ---------------------------------------------------------------- HST-350

/// REQ: HST-350
#[test]
fn hst_350_recording_and_reading_open_no_socket() {
    let before = open_socket_count();
    let telemetry = SideTelemetry::new();
    for class in ALL_CLASSES {
        telemetry.record_call(class, ms(1), ms(20), 1_000);
    }
    let buckets = telemetry.snapshot(2_000);
    assert_eq!(
        open_socket_count(),
        before,
        "telemetry must not open a socket"
    );
    assert_eq!(
        buckets
            .iter()
            .map(|b| b.calls(CallClass::Event))
            .sum::<u64>(),
        1
    );
}

/// REQ: HST-350
#[test]
fn hst_350_end_of_instance_summary_opens_no_socket() {
    let before = open_socket_count();
    let telemetry = SideTelemetry::new();
    telemetry.record_call(CallClass::Event, ms(1), ms(20), 10);
    let summary = telemetry.end_instance(20);
    assert!(summary.is_some());
    assert_eq!(open_socket_count(), before);
}

// ---------------------------------------------------------------- HST-351

/// REQ: HST-351
#[test]
fn hst_351_record_one_call_p99_at_most_200_ns() {
    let telemetry = SideTelemetry::new();
    for round in 0..2_000u64 {
        telemetry.record_call(CallClass::Event, us(300), ms(20), round);
    }
    let mut samples: Vec<u128> = Vec::with_capacity(50_000);
    for round in 0..50_000u64 {
        let started = Instant::now();
        telemetry.record_call(CallClass::Event, us(300 + (round % 7)), ms(20), 5_000);
        samples.push(started.elapsed().as_nanos());
    }
    samples.sort_unstable();
    let p99 = samples[samples.len() * 99 / 100];
    assert!(
        p99 <= 200,
        "p99 of one record_call was {p99} ns, limit 200 ns"
    );
}

/// REQ: HST-351
#[test]
fn hst_351_concurrent_recording_loses_no_count() {
    let telemetry = Arc::new(SideTelemetry::new());
    let mut handles = Vec::new();
    for _ in 0..8 {
        let shared = Arc::clone(&telemetry);
        handles.push(thread::spawn(move || {
            for round in 0..5_000u64 {
                shared.record_call(CallClass::Event, us(100), ms(20), round);
            }
        }));
    }
    for handle in handles {
        handle.join().expect("recording thread must not panic");
    }
    let total: u64 = telemetry
        .snapshot(10_000)
        .iter()
        .map(|b| b.calls(CallClass::Event))
        .sum();
    assert_eq!(total, 40_000);
}

// ---------------------------------------------------------------- HST-352

/// REQ: HST-352
#[test]
fn hst_352_limit_constant_is_64_kib() {
    assert_eq!(TELEMETRY_MAX_BYTES_PER_SIDE, 64 * 1024);
}

/// REQ: HST-352
#[test]
fn hst_352_fresh_side_is_within_64_kib() {
    assert!(SideTelemetry::new().memory_bytes() <= 64 * 1024);
}

/// REQ: HST-352
#[test]
fn hst_352_full_day_all_classes_is_within_64_kib() {
    let telemetry = SideTelemetry::new();
    for hour in 0..24u64 {
        for class in ALL_CLASSES {
            telemetry.record_call(class, us(250), ms(20), hour * HOUR_MS + 1);
            telemetry.record_breach(class, BreachKind::Hard, hour * HOUR_MS + 2);
            telemetry.record_breach(class, BreachKind::Soft, hour * HOUR_MS + 3);
        }
    }
    assert!(telemetry.memory_bytes() <= 64 * 1024);
}

/// REQ: HST-352
#[test]
fn hst_352_ten_thousand_distinct_keys_stay_within_64_kib() {
    let telemetry = SideTelemetry::new();
    for key in 0..10_000u64 {
        telemetry.record_trap(&format!("host.code-{key}"), key);
        telemetry.record_event_dropped(&format!("kind-{key}"), 1, key);
        telemetry.record_rate_limited(&format!("import-{key}"), key);
        telemetry.record_render(&format!("view-{key}"), ms(1), key);
        telemetry.record_restart(&format!("reason-{key}"), key);
    }
    assert!(telemetry.memory_bytes() <= 64 * 1024);
}

/// REQ: HST-352
#[test]
fn hst_352_hundred_thousand_calls_do_not_grow_memory() {
    let telemetry = SideTelemetry::new();
    telemetry.record_call(CallClass::Event, us(100), ms(20), 1);
    let before = telemetry.memory_bytes();
    for round in 0..100_000u64 {
        telemetry.record_call(CallClass::Event, us(100), ms(20), 2 + round % 50);
    }
    assert_eq!(telemetry.memory_bytes(), before);
}

// ---------------------------------------------------------------- HST-353

const FORBIDDEN_FIELD_WORDS: [&str; 6] = ["payload", "path", "url", "terminal", "prompt", "text"];

/// REQ: HST-353
#[test]
fn hst_353_counter_names_hold_no_content_words() {
    for name in COUNTER_NAMES {
        for word in FORBIDDEN_FIELD_WORDS {
            assert!(!name.contains(word), "counter {name} contains {word}");
        }
    }
}

fn collect_keys(value: &serde_json::Value, keys: &mut Vec<String>) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, inner) in map {
                keys.push(key.clone());
                collect_keys(inner, keys);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                collect_keys(item, keys);
            }
        }
        _ => {}
    }
}

/// REQ: HST-353
#[test]
fn hst_353_serialized_bucket_schema_has_no_payload_fields() {
    let telemetry = SideTelemetry::new();
    telemetry.record_call(CallClass::Event, ms(1), ms(20), 5);
    telemetry.record_trap("host.deadline", 6);
    let buckets = telemetry.snapshot(10);
    let value = serde_json::to_value(&buckets).expect("buckets serialize");
    let mut keys = Vec::new();
    collect_keys(&value, &mut keys);
    assert!(!keys.is_empty());
    for key in keys {
        for word in FORBIDDEN_FIELD_WORDS {
            assert!(
                !key.contains(word),
                "serialized field {key} contains {word}"
            );
        }
    }
}

/// REQ: HST-353
#[test]
fn hst_353_span_fields_hold_no_payload() {
    assert_eq!(
        Span::FIELD_NAMES,
        [
            "plugin",
            "side",
            "class",
            "queue_time_us",
            "run_time_us",
            "forwarded_imports"
        ]
    );
}

// ---------------------------------------------------------------- HST-354

/// REQ: HST-354
#[test]
fn hst_354_counter_set_is_the_sixteen_listed() {
    assert_eq!(
        COUNTER_NAMES,
        [
            "calls",
            "duration",
            "near_misses",
            "breaches",
            "traps",
            "queue_depth",
            "events_dropped",
            "events_coalesced",
            "quota_rejections",
            "rate_limited",
            "cpu_ms",
            "memory_peak",
            "helper_round_trips",
            "render_ms",
            "restarts",
            "failures",
        ]
    );
}

/// REQ: HST-354
#[test]
fn hst_354_duration_bucket_bounds_are_the_listed_twelve() {
    assert_eq!(
        DURATION_BUCKET_BOUNDS_MS,
        [0.1, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0, 500.0, 1000.0]
    );
}

/// REQ: HST-354
#[test]
fn hst_354_each_duration_lands_in_its_bucket() {
    // manual does not pin whether a bucket bound itself is inclusive; only interior samples are
    // asserted so the edge rule is not invented here.
    let samples_micros: [u64; 13] = [
        50, 150, 300, 700, 1_500, 3_000, 7_000, 15_000, 30_000, 70_000, 200_000, 700_000, 5_000_000,
    ];
    for (slot, micros) in samples_micros.iter().enumerate() {
        let telemetry = SideTelemetry::new();
        telemetry.record_call(CallClass::Event, us(*micros), ms(10_000), 10);
        let histogram = telemetry.snapshot(20)[0].duration_histogram(CallClass::Event);
        let mut expected = [0u64; 13];
        expected[slot] = 1;
        assert_eq!(histogram, expected, "{micros} us must land in slot {slot}");
    }
}

/// REQ: HST-354
#[test]
fn hst_354_calls_are_keyed_by_class() {
    let telemetry = SideTelemetry::new();
    telemetry.record_call(CallClass::Event, us(100), ms(20), 1);
    telemetry.record_call(CallClass::Event, us(100), ms(20), 2);
    telemetry.record_call(CallClass::Render, us(100), ms(10), 3);
    let bucket = &telemetry.snapshot(10)[0];
    assert_eq!(bucket.calls(CallClass::Event), 2);
    assert_eq!(bucket.calls(CallClass::Render), 1);
    assert_eq!(bucket.calls(CallClass::Job), 0);
}

/// REQ: HST-354
#[test]
fn hst_354_breaches_are_keyed_by_class_and_kind() {
    let telemetry = SideTelemetry::new();
    telemetry.record_breach(CallClass::Event, BreachKind::Hard, 1);
    telemetry.record_breach(CallClass::Event, BreachKind::Soft, 2);
    telemetry.record_breach(CallClass::Event, BreachKind::Soft, 3);
    let bucket = &telemetry.snapshot(10)[0];
    assert_eq!(bucket.breaches(CallClass::Event, BreachKind::Hard), 1);
    assert_eq!(bucket.breaches(CallClass::Event, BreachKind::Soft), 2);
    assert_eq!(bucket.breaches(CallClass::Render, BreachKind::Hard), 0);
}

/// REQ: HST-354
#[test]
fn hst_354_traps_events_and_other_counters_are_kept() {
    let telemetry = SideTelemetry::new();
    telemetry.record_trap("host.deadline", 1);
    telemetry.record_trap("host.deadline", 2);
    telemetry.record_trap("host.stack-overflow", 3);
    telemetry.record_queue_depth(7, 4);
    telemetry.record_queue_depth(3, 5);
    telemetry.record_event_dropped("node-renamed", 4, 6);
    telemetry.record_event_coalesced("node-renamed", 2, 7);
    telemetry.record_quota_rejection(8);
    telemetry.record_rate_limited("storage.put", 9);
    telemetry.record_cpu_ms(12, 10);
    telemetry.record_cpu_ms(8, 11);
    telemetry.record_memory(4_096, 12);
    telemetry.record_memory(2_048, 13);
    telemetry.record_restart("trap", 14);
    telemetry.record_failure("trap", 15);
    let bucket = &telemetry.snapshot(20)[0];
    assert_eq!(bucket.traps("host.deadline"), 2);
    assert_eq!(bucket.traps("host.stack-overflow"), 1);
    assert_eq!(bucket.queue_depth_high_water(), 7);
    assert_eq!(bucket.events_dropped("node-renamed"), 4);
    assert_eq!(bucket.events_coalesced("node-renamed"), 2);
    assert_eq!(bucket.quota_rejections(), 1);
    assert_eq!(bucket.rate_limited("storage.put"), 1);
    assert_eq!(bucket.cpu_ms(), 20);
    assert_eq!(bucket.memory_peak(), 4_096);
    assert_eq!(bucket.restarts("trap"), 1);
    assert_eq!(bucket.failures("trap"), 1);
}

/// REQ: HST-354
#[test]
fn hst_354_helper_and_render_histograms_use_the_same_buckets() {
    let telemetry = SideTelemetry::new();
    telemetry.record_helper_round_trip(us(300), 1);
    telemetry.record_render("main", ms(3), 2);
    let bucket = &telemetry.snapshot(10)[0];
    let mut helper = [0u64; 13];
    helper[2] = 1;
    let mut render = [0u64; 13];
    render[5] = 1;
    assert_eq!(bucket.helper_round_trips(), helper);
    assert_eq!(bucket.render_ms("main"), render);
    assert_eq!(bucket.render_ms("other"), [0u64; 13]);
}

// ---------------------------------------------------------------- HST-355

fn near_misses_for(duration_micros: u64, deadline_micros: u64) -> (u64, u64) {
    let telemetry = SideTelemetry::new();
    telemetry.record_call(
        CallClass::Event,
        us(duration_micros),
        us(deadline_micros),
        1,
    );
    let bucket = &telemetry.snapshot(10)[0];
    (
        bucket.near_misses(CallClass::Event),
        bucket.breaches(CallClass::Event, BreachKind::Soft),
    )
}

/// REQ: HST-355
#[test]
fn hst_355_exactly_80_percent_is_a_near_miss() {
    assert_eq!(near_misses_for(16_000, 20_000).0, 1);
}

/// REQ: HST-355
#[test]
fn hst_355_just_below_80_percent_is_not_a_near_miss() {
    assert_eq!(near_misses_for(15_999, 20_000).0, 0);
}

/// REQ: HST-355
#[test]
fn hst_355_just_inside_the_deadline_is_a_near_miss() {
    assert_eq!(near_misses_for(19_999, 20_000).0, 1);
}

/// REQ: HST-355
#[test]
fn hst_355_call_that_ends_in_time_is_not_a_breach() {
    assert_eq!(near_misses_for(19_000, 20_000), (1, 0));
}

/// REQ: HST-355
#[test]
fn hst_355_call_past_the_deadline_is_not_counted_as_near_miss() {
    assert_eq!(near_misses_for(25_000, 20_000).0, 0);
}

/// REQ: HST-355
#[test]
fn hst_355_near_miss_uses_the_deadline_of_its_class() {
    let telemetry = SideTelemetry::new();
    telemetry.record_call(CallClass::Render, ms(8), ms(10), 1);
    telemetry.record_call(CallClass::Event, ms(8), ms(20), 2);
    let bucket = &telemetry.snapshot(10)[0];
    assert_eq!(bucket.near_misses(CallClass::Render), 1);
    assert_eq!(bucket.near_misses(CallClass::Event), 0);
}

// ---------------------------------------------------------------- HST-356

/// REQ: HST-356
#[test]
fn hst_356_plugin_cannot_read_counters_of_another_plugin() {
    let registry = TelemetryRegistry::new();
    registry
        .side("example.one", Side::Server)
        .record_call(CallClass::Event, ms(1), ms(20), 5);
    let result = registry.read(
        &Reader::Plugin("example.two".to_owned()),
        "example.one",
        Side::Server,
        10,
    );
    // manual does not pin the code; not-permitted is the host's refusal word for reads without
    // authority.
    let error = result.expect_err("another plugin must not read the counters");
    assert_eq!(error.code, "not-permitted", "{error:?}");
}

/// REQ: HST-356
#[test]
fn hst_356_kernel_cli_and_ui_read_counters() {
    let registry = TelemetryRegistry::new();
    registry
        .side("example.one", Side::Server)
        .record_call(CallClass::Event, ms(1), ms(20), 5);
    for reader in [Reader::Kernel, Reader::Cli, Reader::Ui] {
        let buckets = registry
            .read(&reader, "example.one", Side::Server, 10)
            .expect("kernel, cli and ui read counters");
        assert_eq!(buckets[0].calls(CallClass::Event), 1);
    }
}

/// REQ: HST-356
#[test]
fn hst_356_plugin_can_read_its_own_counters_only_through_summary() {
    let registry = TelemetryRegistry::new();
    registry
        .side("example.one", Side::Server)
        .record_call(CallClass::Event, ms(1), ms(20), 5);
    let own = registry.read(
        &Reader::Plugin("example.one".to_owned()),
        "example.one",
        Side::Server,
        10,
    );
    // manual does not pin the code; not-permitted is the host's refusal word.
    let error = own.expect_err("the raw counters are not a plugin surface; plugin-info is");
    assert_eq!(error.code, "not-permitted", "{error:?}");
}

/// REQ: HST-356
#[test]
fn hst_356_sides_of_one_plugin_keep_separate_counters() {
    let registry = TelemetryRegistry::new();
    registry
        .side("example.one", Side::Server)
        .record_call(CallClass::Event, ms(1), ms(20), 5);
    let client = registry
        .read(&Reader::Kernel, "example.one", Side::Client, 10)
        .expect("client read")
        .iter()
        .map(|b| b.calls(CallClass::Event))
        .sum::<u64>();
    assert_eq!(client, 0);
    let server = registry
        .read(&Reader::Kernel, "example.one", Side::Server, 10)
        .expect("server counters");
    assert_eq!(server[0].calls(CallClass::Event), 1);
}

// ---------------------------------------------------------------- HST-357

/// REQ: HST-357
#[test]
fn hst_357_calls_in_one_hour_share_a_bucket() {
    let telemetry = SideTelemetry::new();
    telemetry.record_call(CallClass::Event, us(100), ms(20), 10 * 60_000);
    telemetry.record_call(CallClass::Event, us(100), ms(20), 50 * 60_000);
    telemetry.record_call(CallClass::Event, us(100), ms(20), HOUR_MS + 10 * 60_000);
    let buckets = telemetry.snapshot(HOUR_MS + 20 * 60_000);
    assert_eq!(buckets.len(), 2);
    assert_eq!(buckets[0].hour_start_ms, 0);
    assert_eq!(buckets[0].calls(CallClass::Event), 2);
    assert_eq!(buckets[1].hour_start_ms, HOUR_MS);
    assert_eq!(buckets[1].calls(CallClass::Event), 1);
}

/// REQ: HST-357
#[test]
fn hst_357_bucket_is_kept_at_24_hours() {
    let telemetry = SideTelemetry::new();
    telemetry.record_call(CallClass::Event, us(100), ms(20), 5 * 60_000);
    let buckets = telemetry.snapshot(24 * HOUR_MS);
    assert_eq!(
        buckets
            .iter()
            .map(|b| b.calls(CallClass::Event))
            .sum::<u64>(),
        1
    );
}

/// REQ: HST-357
#[test]
fn hst_357_bucket_older_than_24_hours_is_discarded() {
    let telemetry = SideTelemetry::new();
    telemetry.record_call(CallClass::Event, us(100), ms(20), 5 * 60_000);
    let buckets = telemetry.snapshot(25 * HOUR_MS);
    assert_eq!(
        buckets
            .iter()
            .map(|b| b.calls(CallClass::Event))
            .sum::<u64>(),
        0
    );
}

/// REQ: HST-357
#[test]
fn hst_357_at_most_25_buckets_after_a_long_run() {
    let telemetry = SideTelemetry::new();
    for hour in 0..60u64 {
        telemetry.record_call(CallClass::Event, us(100), ms(20), hour * HOUR_MS + 1);
    }
    let buckets = telemetry.snapshot(59 * HOUR_MS + 2);
    assert!(buckets.len() <= 25, "kept {} buckets", buckets.len());
    assert!(buckets.iter().any(|b| b.hour_start_ms == 59 * HOUR_MS));
    assert!(buckets.iter().all(|b| b.hour_start_ms >= 35 * HOUR_MS));
}

/// REQ: HST-357
#[test]
fn hst_357_summary_line_is_written_once() {
    let telemetry = SideTelemetry::new();
    telemetry.record_call(CallClass::Event, us(100), ms(20), 1);
    let first = telemetry.end_instance(10);
    let second = telemetry.end_instance(11);
    assert!(first.is_some());
    assert!(second.is_none());
}

/// REQ: HST-357
#[test]
fn hst_357_summary_line_carries_the_call_count() {
    let telemetry = SideTelemetry::new();
    for round in 0..3 {
        telemetry.record_call(CallClass::Event, us(100), ms(20), round);
    }
    let line = telemetry.end_instance(10).expect("summary line");
    // manual does not pin the summary format; require a standalone "3", not a digit inside an id.
    assert!(
        line.split(|c: char| !c.is_ascii_alphanumeric())
            .any(|token| token == "3"),
        "summary {line:?} must show 3 calls"
    );
    assert!(
        line.contains("call"),
        "summary {line:?} must name the calls"
    );
}

// ---------------------------------------------------------------- HST-358

const HOST_LINE_TABLE: [(&str, &str); 11] = [
    ("instance.started", "info"),
    ("instance.ended", "info"),
    ("host.deadline", "warn"),
    ("host.late", "warn"),
    ("host.queue-timeout", "warn"),
    ("events.dropped", "warn"),
    ("limits.clamped", "warn"),
    ("deactivate.deadline", "warn"),
    ("host.trap", "error"),
    ("helper.died", "error"),
    ("host.slow-calls", "info"),
];

/// REQ: HST-358
#[test]
fn hst_358_host_lines_carry_plugin_side_and_stable_id() {
    for (id, level) in HOST_LINE_TABLE {
        let line = host_line("example.notes", Side::Server, id, "detail").expect("known condition");
        assert_eq!(line.plugin, "example.notes");
        assert_eq!(line.side, Side::Server);
        assert_eq!(line.id, id);
        assert_eq!(line.level, level, "level of {id}");
    }
}

/// REQ: HST-358
#[test]
fn hst_358_unknown_condition_id_is_refused() {
    assert!(host_line("example.notes", Side::Server, "host.invented", "x").is_err());
}

/// REQ: HST-358
#[test]
fn hst_358_twenty_lines_per_minute_are_admitted() {
    let mut limiter = HostLogLimiter::new();
    let admitted = (0..20)
        .filter(|n| limiter.admit(Side::Server, "host.deadline", 1_000 + n))
        .count();
    assert_eq!(admitted, 20);
}

/// REQ: HST-358
#[test]
fn hst_358_the_21st_line_in_a_minute_is_suppressed() {
    let mut limiter = HostLogLimiter::new();
    for n in 0..20 {
        assert!(limiter.admit(Side::Server, "host.deadline", 1_000 + n));
    }
    assert!(!limiter.admit(Side::Server, "host.deadline", 1_500));
}

/// REQ: HST-358
#[test]
fn hst_358_budget_returns_after_a_minute() {
    let mut limiter = HostLogLimiter::new();
    for n in 0..25 {
        limiter.admit(Side::Server, "host.deadline", 1_000 + n);
    }
    assert!(limiter.admit(Side::Server, "host.deadline", 61_001));
}

/// REQ: HST-358
#[test]
fn hst_358_budget_is_per_side_and_per_kind() {
    let mut limiter = HostLogLimiter::new();
    for n in 0..20 {
        assert!(limiter.admit(Side::Server, "host.deadline", 1_000 + n));
    }
    assert!(limiter.admit(Side::Client, "host.deadline", 1_100));
    assert!(limiter.admit(Side::Server, "host.late", 1_100));
    assert!(!limiter.admit(Side::Server, "host.deadline", 1_100));
}

/// REQ: HST-358
#[test]
fn hst_358_suppressed_lines_are_summarised_in_one_line() {
    let mut limiter = HostLogLimiter::new();
    for n in 0..25 {
        limiter.admit(Side::Server, "host.deadline", 1_000 + n);
    }
    let summary = limiter.take_suppressed_summary(Side::Server, "host.deadline", 61_001);
    assert_eq!(summary.as_deref(), Some("5 similar lines suppressed"));
    assert_eq!(
        limiter.take_suppressed_summary(Side::Server, "host.deadline", 61_002),
        None
    );
}

/// REQ: HST-358
#[test]
fn hst_358_no_summary_when_nothing_was_suppressed() {
    let mut limiter = HostLogLimiter::new();
    limiter.admit(Side::Server, "host.deadline", 1_000);
    assert_eq!(
        limiter.take_suppressed_summary(Side::Server, "host.deadline", 61_001),
        None
    );
}

// ---------------------------------------------------------------- HST-359

fn slow_calls_reporter() -> SlowCallReporter {
    SlowCallReporter::new()
}

/// REQ: HST-359
#[test]
fn hst_359_call_over_50_percent_triggers_one_report_with_the_stable_id() {
    let mut reporter = slow_calls_reporter();
    reporter.record(Side::Server, CallClass::Event, ms(11), ms(20), 1_000);
    let report = reporter
        .due_report(Side::Server, 61_000)
        .expect("a call took over 50 percent");
    assert_eq!(report.id, "host.slow-calls");
}

/// REQ: HST-359
#[test]
fn hst_359_exactly_50_percent_is_not_over() {
    let mut reporter = slow_calls_reporter();
    reporter.record(Side::Server, CallClass::Event, ms(10), ms(20), 1_000);
    assert!(reporter.due_report(Side::Server, 61_000).is_none());
}

/// REQ: HST-359
#[test]
fn hst_359_no_report_when_all_calls_are_fast() {
    let mut reporter = slow_calls_reporter();
    for round in 0..10 {
        reporter.record(Side::Server, CallClass::Event, ms(1), ms(20), 1_000 + round);
    }
    assert!(reporter.due_report(Side::Server, 61_000).is_none());
}

/// REQ: HST-359
#[test]
fn hst_359_at_most_once_per_minute() {
    let mut reporter = slow_calls_reporter();
    reporter.record(Side::Server, CallClass::Event, ms(15), ms(20), 1_000);
    assert!(reporter.due_report(Side::Server, 61_000).is_some());
    reporter.record(Side::Server, CallClass::Event, ms(15), ms(20), 62_000);
    assert!(reporter.due_report(Side::Server, 70_000).is_none());
    assert!(reporter.due_report(Side::Server, 121_001).is_some());
}

/// REQ: HST-359
#[test]
fn hst_359_report_lists_the_three_slowest_calls() {
    let mut reporter = slow_calls_reporter();
    for (round, millis) in [12u64, 18, 14, 16, 11].into_iter().enumerate() {
        reporter.record(
            Side::Server,
            CallClass::Event,
            ms(millis),
            ms(20),
            1_000 + round as u64,
        );
    }
    let report = reporter.due_report(Side::Server, 61_000).expect("report");
    let top: Vec<Duration> = report.top.iter().map(|(_, duration)| *duration).collect();
    assert_eq!(top, vec![ms(18), ms(16), ms(14)]);
}

/// REQ: HST-359
#[test]
fn hst_359_sides_report_independently() {
    let mut reporter = slow_calls_reporter();
    reporter.record(Side::Server, CallClass::Event, ms(15), ms(20), 1_000);
    reporter.record(Side::Client, CallClass::Event, ms(15), ms(20), 1_000);
    assert!(reporter.due_report(Side::Server, 61_000).is_some());
    assert!(reporter.due_report(Side::Client, 61_000).is_some());
}

// ---------------------------------------------------------------- HST-360

/// REQ: HST-360
#[test]
fn hst_360_tracing_is_off_by_default() {
    assert!(!SpanRecorder::from_env(None).is_enabled());
}

/// REQ: HST-360
#[test]
fn hst_360_environment_value_1_turns_tracing_on() {
    assert!(SpanRecorder::from_env(Some("1")).is_enabled());
}

/// REQ: HST-360
#[test]
fn hst_360_other_values_leave_tracing_off() {
    for value in ["0", "", "true", "yes"] {
        assert!(
            !SpanRecorder::from_env(Some(value)).is_enabled(),
            "value {value:?}"
        );
    }
}

fn sample_span() -> Span {
    Span {
        plugin: "example.notes".to_owned(),
        side: Side::Server,
        class: CallClass::Event,
        queue_time: us(40),
        run_time: us(900),
        forwarded_imports: 3,
    }
}

/// REQ: HST-360
#[test]
fn hst_360_disabled_recorder_stores_no_span() {
    let mut recorder = SpanRecorder::from_env(None);
    recorder.record(sample_span());
    assert!(recorder.spans().is_empty());
}

/// REQ: HST-360
#[test]
fn hst_360_enabled_recorder_stores_one_span_per_call_without_payload() {
    let mut recorder = SpanRecorder::from_env(Some("1"));
    recorder.record(sample_span());
    assert_eq!(recorder.spans().len(), 1);
    let value = serde_json::to_value(&recorder.spans()[0]).expect("span serializes");
    let mut keys: Vec<String> = value.as_object().expect("object").keys().cloned().collect();
    keys.sort();
    let mut expected: Vec<String> = Span::FIELD_NAMES
        .iter()
        .map(|name| (*name).to_owned())
        .collect();
    expected.sort();
    assert_eq!(keys, expected);
}

// ---------------------------------------------------------------- HST-362

/// REQ: HST-362
#[test]
fn hst_362_plugin_reads_its_own_summary_without_a_grant() {
    let registry = TelemetryRegistry::new();
    let side = registry.side("example.notes", Side::Server);
    side.record_call(CallClass::Event, ms(1), ms(20), 5);
    side.record_call(CallClass::Event, ms(17), ms(20), 6);
    let summary = registry
        .plugin_info("example.notes", "example.notes", false, 10)
        .expect("own summary");
    assert_eq!(summary.calls, 2);
    assert_eq!(summary.near_misses, 1);
}

/// REQ: HST-362
#[test]
fn hst_362_plugin_cannot_read_another_summary_without_plugins_manage() {
    let registry = TelemetryRegistry::new();
    registry
        .side("example.other", Side::Server)
        .record_call(CallClass::Event, ms(1), ms(20), 5);
    // manual does not pin the code; not-permitted is the host's refusal word.
    let error = registry
        .plugin_info("example.notes", "example.other", false, 10)
        .expect_err("another summary needs plugins.manage");
    assert_eq!(error.code, "not-permitted", "{error:?}");
}

/// REQ: HST-362
#[test]
fn hst_362_plugins_manage_reads_another_summary() {
    let registry = TelemetryRegistry::new();
    registry
        .side("example.other", Side::Server)
        .record_call(CallClass::Event, ms(1), ms(20), 5);
    let summary = registry
        .plugin_info("example.notes", "example.other", true, 10)
        .expect("manage grant");
    assert_eq!(summary.calls, 1);
}

// ---------------------------------------------------------------- HST-363

/// REQ: HST-363
#[test]
fn hst_363_near_miss_signal_shows_a_why_line_with_counter_values() {
    let inputs = HealthInputs {
        calls: 100,
        near_misses: 30,
        ..HealthInputs::default()
    };
    let lines = why_lines(&inputs);
    assert_eq!(lines.len(), 1);
    assert!(lines[0].starts_with("Why"), "line {:?}", lines[0]);
    // manual does not pin the wording or the near-miss rate threshold.
    assert!(
        lines[0].to_lowercase().contains("near"),
        "line {:?} must name the near-miss signal",
        lines[0]
    );
    assert!(
        lines[0].contains("30") && lines[0].contains("100"),
        "line {:?}",
        lines[0]
    );
}

/// REQ: HST-363
#[test]
fn hst_363_quota_signal_shows_the_rejection_count() {
    let inputs = HealthInputs {
        calls: 10,
        quota_rejections_last_minute: 12,
        ..HealthInputs::default()
    };
    let lines = why_lines(&inputs);
    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("12"), "line {:?}", lines[0]);
}

/// REQ: HST-363
#[test]
fn hst_363_memory_signal_over_90_percent_shows_a_why_line() {
    let inputs = HealthInputs {
        calls: 10,
        memory_peak_bytes: 31 * 1024 * 1024,
        memory_limit_bytes: 32 * 1024 * 1024,
        ..HealthInputs::default()
    };
    assert_eq!(why_lines(&inputs).len(), 1);
}

/// REQ: HST-363
#[test]
fn hst_363_memory_at_exactly_90_percent_is_not_a_signal() {
    let inputs = HealthInputs {
        calls: 10,
        memory_peak_bytes: 90,
        memory_limit_bytes: 100,
        ..HealthInputs::default()
    };
    assert!(why_lines(&inputs).is_empty());
}

/// REQ: HST-363
#[test]
fn hst_363_healthy_counters_show_no_why_line() {
    let inputs = HealthInputs {
        calls: 100,
        ..HealthInputs::default()
    };
    assert!(why_lines(&inputs).is_empty());
}

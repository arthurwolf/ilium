//! reference-host-telemetry.md rows HST-524 and HST-525: daily summaries beyond the 24-hour
//! window and the `ilium plugin status --days` view.

use ilium_plugin_host::telemetry_history::{HourlyBucket, TelemetryHistory};

const SIDE: &str = "acme.notes/server";
const PLUGIN: &str = "acme.notes";

fn bucket(hour_index: u64, call_time_ms: u32) -> HourlyBucket {
    HourlyBucket {
        plugin_side: SIDE.to_string(),
        hour_index,
        calls: 10,
        failures: 0,
        near_misses: 0,
        call_times_ms: vec![call_time_ms; 10],
    }
}

/// Records `days` full days of hourly buckets, with call time rising by `step_ms` per day.
fn record_days(history: &mut TelemetryHistory, days: u64, step_ms: u32) {
    for day in 0..days {
        for hour in 0..24 {
            let hour_index = day * 24 + hour;
            history.record_hour(bucket(hour_index, 10 + day as u32 * step_ms));
        }
    }
}

/// REQ: HST-524
#[test]
fn hst_524_thirty_five_days_keep_exactly_30_daily_summaries() {
    let mut history = TelemetryHistory::new();
    record_days(&mut history, 35, 0);
    assert_eq!(history.daily_summaries(SIDE).len(), 30);
}

/// REQ: HST-524
#[test]
fn hst_524_a_daily_summary_adds_at_most_4_kib_per_plugin_side() {
    let mut history = TelemetryHistory::new();
    record_days(&mut history, 2, 1);
    assert!(history.bytes_added_by_last_fold(SIDE) <= 4 * 1024);
}

/// REQ: HST-524
#[test]
fn hst_524_a_daily_summary_holds_the_call_count_and_percentiles() {
    let mut history = TelemetryHistory::new();
    record_days(&mut history, 2, 0);
    let first = history
        .daily_summaries(SIDE)
        .into_iter()
        .next()
        .expect("a folded day");
    assert_eq!(first.calls, 240);
    assert_eq!(first.p50_ms, 10);
    assert_eq!(first.p95_ms, 10);
    assert_eq!(first.p99_ms, 10);
}

/// REQ: HST-525
#[test]
fn hst_525_the_status_output_marks_the_upgrade_day() {
    let mut history = TelemetryHistory::new();
    record_days(&mut history, 10, 0);
    history.record_upgrade(PLUGIN, 5);
    let output = history.status_days_output(PLUGIN, 30);
    let marked_lines = output
        .lines()
        .filter(|line| line.contains("upgrade"))
        .count();
    assert_eq!(marked_lines, 1);
}

/// REQ: HST-525
#[test]
fn hst_525_the_status_output_ends_with_the_rising_p95_trend() {
    let mut history = TelemetryHistory::new();
    record_days(&mut history, 10, 5);
    let output = history.status_days_output(PLUGIN, 30);
    let last_line = output.lines().last().expect("output is not empty");
    assert!(last_line.contains("rising"));
}

/// REQ: HST-525
#[test]
fn hst_525_uninstall_removes_the_daily_summaries() {
    let mut history = TelemetryHistory::new();
    record_days(&mut history, 3, 0);
    history.remove_plugin(PLUGIN);
    assert!(history.daily_summaries(SIDE).is_empty());
}

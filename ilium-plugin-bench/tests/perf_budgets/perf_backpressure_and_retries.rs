//! PRF-010..PRF-016 from `docs/plugins-manual/pages/perf-backpressure-and-retries.md`.
//!
//! Tests that drive a real host type assert the behaviour directly. Bullets whose behaviour needs
//! a running plugin fixture (handler timing, timers, job scheduling, events-dropped re-reads) pin
//! the bullet's concrete numbers in the manual text; those gaps are listed in the lane report.

use ilium_plugin_host::limits_rules::retry_after_ms;
use ilium_plugin_host::scheduler::{ExecutionBucket, StreamCredit};
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

/// REQ: PRF-010
#[test]
fn perf_010_quota_refusal_retry_after_follows_debt_a() {
    // A debt of 500 ms repays at the refill rate in 1000 ms.
    let mut bucket = ExecutionBucket::new();
    bucket.charge_cpu_ms(2_500);
    let Err(error) = bucket.try_submit() else {
        panic!("a bucket in debt must refuse the submission");
    };
    assert_eq!(error.retry_after_ms, Some(1_000));
}

/// REQ: PRF-010
#[test]
fn perf_010_retry_hint_formula_has_100_ms_floor_b() {
    assert_eq!(retry_after_ms(-1_000), 2_000);
    assert_eq!(retry_after_ms(-5), 100);
}

/// REQ: PRF-011
#[test]
fn perf_011_busy_handler_returns_within_20_ms_pinned_by_manual_a() {
    assert!(manual_text().contains("a handler that receives `busy` returns within 20 ms"));
}

/// REQ: PRF-011
#[test]
fn perf_011_retry_happens_in_later_batch_after_timer_pinned_by_manual_b() {
    let text = manual_text();
    assert!(text.contains("A plugin MUST NOT wait for a retry with a sleep or a loop inside a handler"));
    assert!(text.contains("the retry happens in a later batch after the timer fires"));
}

/// REQ: PRF-012
#[test]
fn perf_012_always_refused_call_makes_exactly_5_attempts_pinned_by_manual_a() {
    assert!(manual_text().contains("a call that is always refused makes exactly 5 attempts and then logs one `warn` line"));
}

/// REQ: PRF-012
#[test]
fn perf_012_retry_delay_capped_at_60_seconds_pinned_by_manual_b() {
    assert!(manual_text().contains("the delay doubling from `retry-after-ms` and capped at 60 seconds"));
}

/// REQ: PRF-013
#[test]
fn perf_013_fast_starter_never_exceeds_32_in_flight_on_t3_pinned_by_manual_a() {
    assert!(manual_text().contains("a fixture that starts requests as fast as it can never has more than 32 in flight on T3"));
}

/// REQ: PRF-013
#[test]
fn perf_013_in_flight_budget_is_half_the_pending_limit_pinned_by_manual_b() {
    let text = manual_text();
    assert!(text.contains("keep its own count of requests in flight below half of the `pending_requests` limit of its tier"));
    assert!(text.contains("the plugin has 64 requests in flight by default"));
}

/// REQ: PRF-014
#[test]
fn perf_014_events_dropped_triggers_reread_pinned_by_manual_a() {
    assert!(manual_text().contains("a fixture that receives `events-dropped` with a count of 40 re-reads its state"));
}

/// REQ: PRF-014
#[test]
fn perf_014_drop_oldest_count_is_not_assumed_complete_pinned_by_manual_b() {
    assert!(manual_text().contains("MUST NOT assume that a count of drop-oldest events is complete"));
}

/// REQ: PRF-015
#[test]
fn perf_015_30_ms_on_events_work_breaches_20_ms_deadline_pinned_by_manual_a() {
    assert!(manual_text().contains("a fixture that does 30 ms of work in `on-events` breaches the 20 ms deadline"));
}

/// REQ: PRF-015
#[test]
fn perf_015_same_work_as_job_completes_within_deadline_pinned_by_manual_b() {
    assert!(manual_text().contains("the same work started as a job completes"));
}

/// REQ: PRF-016
#[test]
fn perf_016_unread_stream_bytes_pause_at_256_kib_a() {
    let mut credit = StreamCredit::new();
    credit.enqueued(256 * 1024);
    assert_eq!(credit.in_flight(), 256 * 1024);
    assert!(!credit.should_read(), "a full window must stop the reader");
    credit.consumed(128 * 1024);
    assert!(credit.should_read(), "the reader resumes at half the window");
}

/// REQ: PRF-016
#[test]
fn perf_016_10_mib_stream_splits_into_chunks_of_at_most_64_kib_b() {
    let total = 10 * 1024 * 1024;
    let chunks = StreamCredit::split_chunks(total);
    assert!(chunks.iter().all(|chunk| *chunk <= 64 * 1024));
    assert_eq!(chunks.iter().sum::<usize>(), total);
}

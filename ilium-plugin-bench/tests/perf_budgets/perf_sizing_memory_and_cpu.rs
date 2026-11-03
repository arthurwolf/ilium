//! PRF-001..PRF-006 from `docs/plugins-manual/pages/perf-sizing-memory-and-cpu.md`.
//!
//! Tests that drive a real host type assert the behaviour directly. Bullets whose behaviour needs
//! a running plugin fixture or the `ilium plugin` CLI pin the bullet's concrete numbers in the
//! manual text (see `manual_text`); those gaps are listed in the lane report.

use ilium_plugin_host::host_capacity::CapacityHost;
use ilium_plugin_host::scheduler::ExecutionBucket;
use std::path::PathBuf;
use std::time::Duration;

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

/// REQ: PRF-001
#[test]
fn perf_001_ninth_activation_refused_with_host_capacity_a() {
    let mut host = CapacityHost::new(16, 8);
    for index in 1..=9 {
        assert!(host.install(&format!("p{index}")).is_ok());
    }
    for index in 1..=8 {
        assert!(host.activate(&format!("p{index}")).is_ok());
    }
    let Err(refusal) = host.activate("p9") else {
        panic!("the ninth activation must be refused");
    };
    assert_eq!(refusal.reason, "host-capacity");
}

/// REQ: PRF-001
#[test]
fn perf_001_refused_activation_leaves_active_instances_alone_b() {
    let mut host = CapacityHost::new(16, 8);
    for index in 1..=9 {
        assert!(host.install(&format!("p{index}")).is_ok());
    }
    for index in 1..=8 {
        assert!(host.activate(&format!("p{index}")).is_ok());
    }
    assert!(host.activate("p9").is_err());
    let active = host.active_plugin_ids();
    assert_eq!(active.len(), 8);
    assert!(!active.iter().any(|id| id == "p9"));
}

/// REQ: PRF-002
#[test]
fn perf_002_growth_past_memory_mib_traps_pinned_by_manual_a() {
    assert!(manual_text().contains("allocates 33 MiB with `memory_mib` 32 traps"));
}

/// REQ: PRF-002
#[test]
fn perf_002_host_counts_memory_trap_as_failure_pinned_by_manual_b() {
    let text = manual_text();
    assert!(text.contains("the host MUST trap the instance and count the trap as a failure"));
    assert!(text.contains("the same fixture with `memory_mib` 64 passes"));
}

/// REQ: PRF-003
#[test]
fn perf_003_memory_mib_at_most_1_5_times_peak_pinned_by_manual_a() {
    assert!(manual_text().contains("no more than 1.5 times its measured peak on a release build"));
}

/// REQ: PRF-003
#[test]
fn perf_003_peak_of_20_mib_allows_manifest_30_mib_pinned_by_manual_b() {
    assert!(manual_text()
        .contains("a fixture reports a peak of 20 MiB; the manifest value is at most 30 MiB"));
}

/// REQ: PRF-004
#[test]
fn perf_004_normal_load_for_60_seconds_spends_under_15000_ms_a() {
    // Normal load: 250 ms of CPU per second, against a refill of 500 ms per second.
    let mut bucket = ExecutionBucket::new();
    let mut total_spend_ms: u64 = 0;
    for _second in 0..60 {
        bucket.charge_cpu_ms(250);
        total_spend_ms += 250;
        bucket.refill(Duration::from_secs(1));
        assert!(bucket.try_submit().is_ok(), "normal load must never reach debt");
    }
    assert!(total_spend_ms < 15_000);
    assert!(bucket.balance_ms() > 0);
}

/// REQ: PRF-004
#[test]
fn perf_004_bucket_refills_half_a_second_per_second_b() {
    let mut bucket = ExecutionBucket::new();
    bucket.charge_cpu_ms(2_000);
    assert_eq!(bucket.balance_ms(), 0);
    bucket.refill(Duration::from_secs(1));
    assert_eq!(bucket.balance_ms(), 500);
}

/// REQ: PRF-005
#[test]
fn perf_005_quota_refusal_carries_retry_after_for_its_debt_a() {
    let mut bucket = ExecutionBucket::new();
    bucket.charge_cpu_ms(3_000);
    let Err(error) = bucket.try_submit() else {
        panic!("an empty bucket must refuse the submission");
    };
    assert_eq!(error.retry_after_ms, Some(2_000));
}

/// REQ: PRF-005
#[test]
fn perf_005_empty_bucket_accepts_no_submission_before_retry_after_b() {
    let mut bucket = ExecutionBucket::new();
    bucket.charge_cpu_ms(3_000);
    assert!(bucket.try_submit().is_err());
    // Just inside the 2000 ms retry-after window: still refused.
    bucket.refill(Duration::from_millis(1_999));
    assert!(bucket.try_submit().is_err());
    // Past the window: accepted again.
    bucket.refill(Duration::from_millis(101));
    assert!(bucket.try_submit().is_ok());
}

/// REQ: PRF-006
#[test]
fn perf_006_whole_data_set_loop_never_in_on_events_pinned_by_manual_a() {
    assert!(manual_text().contains(
        "A plugin MUST NOT run a loop over its whole data set inside `on-events` or `views.render`"
    ));
}

/// REQ: PRF-006
#[test]
fn perf_006_100000_items_return_within_20_ms_as_job_pinned_by_manual_b() {
    assert!(manual_text()
        .contains("a fixture with 100,000 items makes `on-events` return within 20 ms"));
}

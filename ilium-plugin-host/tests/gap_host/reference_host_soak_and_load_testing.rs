//! Manual page `reference-host-soak-and-load-testing`: the 72-hour endurance record, the file
//! descriptor budget and the storage fault-injection conformance suite (HST-514, 681, 699).

use std::time::Duration;

use super::common::{install_ok, pkg, rig, NOTES};

/// REQ: HST-514
#[test]
fn hst_514_soak_record_bounds_resident_memory_growth_and_lists_cache_sizes_per_cycle() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let record = rig.soak_run_record(Duration::from_secs(72 * 3600));
    assert!(
        record.duration_seconds >= 72 * 3600,
        "endurance run lasted at least 72 hours"
    );
    let steady = record.steady_state_rss_bytes as f64;
    for cycle in &record.cycles {
        assert!(
            cycle.rss_after_idle_bytes as f64 <= steady * 1.10,
            "RSS after forced idle within 10 percent of steady state"
        );
        assert!(
            cycle.compile_cache_bytes > 0,
            "compile cache size recorded in the run record"
        );
        assert!(
            cycle.engine_code_bytes > 0,
            "Engine code memory size recorded in the run record"
        );
    }
}

/// REQ: HST-681
#[test]
fn hst_681_fd_budget_refuses_opens_past_8_per_side_and_512_per_server_and_raises_soft_limit() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    rig.activate(NOTES);
    let per_side = rig.open_descriptors_until_refused(NOTES);
    assert_eq!(per_side.refused_with, "io", "refused with io");
    assert_eq!(per_side.message, "too-many-files", "message too-many-files");
    assert_eq!(per_side.opened, 8, "at most 8 per active plugin side");
    assert_eq!(
        rig.activation_refusal_code(NOTES),
        "host-capacity",
        "the activation is refused with host-capacity past the server budget"
    );
    assert_eq!(
        rig.soft_fd_limit(),
        rig.hard_fd_limit(),
        "soft limit raised to the hard limit at start"
    );
}

/// REQ: HST-699
#[test]
fn hst_699_fault_injection_suite_keeps_every_acknowledged_write_readable_or_quarantined() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let report = rig.run_storage_fault_injection(NOTES, 10_000);
    assert_eq!(report.acknowledged_writes, 10_000, "10,000 acknowledged writes under random kill points, disk-full, read-only flips and corrupt bytes");
    assert_eq!(
        report.silently_lost, 0,
        "zero acknowledged writes silently lost"
    );
    assert_eq!(
        report.unreadable_and_not_quarantined, 0,
        "each one readable after recovery or in quarantine"
    );
    assert!(
        rig.release_gate_failed_when_suite_fails(&report),
        "a release fails when this suite fails"
    );
}

/// REQ: HST-514
#[test]
fn hst_514_soak_run_record_contains_at_least_one_cycle() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let record = rig.soak_run_record(Duration::from_secs(72 * 3600));
    assert!(!record.cycles.is_empty(), "the run record lists its cycles");
}

/// REQ: HST-681
#[test]
fn hst_681_soft_descriptor_limit_is_raised_to_the_hard_limit_at_start() {
    let rig = rig();
    assert_eq!(
        rig.soft_fd_limit(),
        rig.hard_fd_limit(),
        "the soft limit equals the hard limit after start"
    );
}

/// REQ: HST-699
#[test]
fn hst_699_repeated_fault_injection_run_still_loses_no_acknowledged_write() {
    let rig = rig();
    install_ok(&rig, &pkg(NOTES, "1.0.0"));
    let second = rig.run_storage_fault_injection(NOTES, 10_000);
    assert_eq!(second.silently_lost, 0, "a second run loses no acknowledged write");
}

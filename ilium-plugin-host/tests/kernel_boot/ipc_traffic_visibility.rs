//! reference-host-ipc.md rows HST-522 and HST-523: counts and log gates for coalesced and
//! overload-dropped plugin traffic.

use ilium_plugin_host::ipc_traffic::{
    coalesced_notice_due, overload_disconnect_culprit, CoalesceLogGate, TrafficLedger,
};

/// REQ: HST-522
#[test]
fn hst_522_the_three_counts_and_the_latest_time_are_kept_per_plugin_and_client() {
    let mut ledger = TrafficLedger::new();
    ledger.record_coalesced("acme.notes", "client-1", 1_000);
    ledger.record_coalesced("acme.notes", "client-1", 2_000);
    ledger.record_resync("acme.notes", "client-1", 3_000);
    ledger.record_overload_disconnect("acme.notes", "client-1", 4_000);

    let counts = ledger.counts("acme.notes", "client-1");
    assert_eq!(counts.coalesced, 2);
    assert_eq!(counts.resyncs, 1);
    assert_eq!(counts.overload_disconnects, 1);
    assert_eq!(counts.latest_at_ms, Some(4_000));
}

/// REQ: HST-522
#[test]
fn hst_522_counts_of_one_client_do_not_include_another_client() {
    let mut ledger = TrafficLedger::new();
    ledger.record_coalesced("acme.notes", "client-1", 1_000);
    assert_eq!(ledger.counts("acme.notes", "client-2").coalesced, 0);
}

/// REQ: HST-523
#[test]
fn hst_523_a_coalesced_log_line_is_written_at_most_once_per_minute() {
    let mut gate = CoalesceLogGate::new();
    assert!(gate.should_log("acme.notes", "client-1", 0));
    assert!(!gate.should_log("acme.notes", "client-1", 59_999));
    assert!(gate.should_log("acme.notes", "client-1", 60_000));
}

/// REQ: HST-523
#[test]
fn hst_523_the_minute_limit_is_per_plugin_and_per_client() {
    let mut gate = CoalesceLogGate::new();
    assert!(gate.should_log("acme.notes", "client-1", 0));
    assert!(gate.should_log("acme.notes", "client-2", 1));
    assert!(gate.should_log("acme.sync", "client-1", 2));
}

/// REQ: HST-523
#[test]
fn hst_523_the_disconnect_names_the_plugin_with_the_largest_queued_share() {
    let queued = vec![
        ("acme.notes".to_string(), 4_096),
        ("acme.sync".to_string(), 65_536),
        ("acme.theme".to_string(), 1_024),
    ];
    assert_eq!(
        overload_disconnect_culprit(&queued).as_deref(),
        Some("acme.sync")
    );
}

/// REQ: HST-523
#[test]
fn hst_523_the_notice_appears_only_after_more_than_10_seconds_of_coalescing() {
    assert!(!coalesced_notice_due(0, 10_000));
    assert!(coalesced_notice_due(0, 10_001));
}

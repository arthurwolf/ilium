//! Page reference-requirements-api-host.md: crash counters and log collapse, the enable command,
//! the effective `limits` record and the memory admission queue (API-092..095).

use ilium_plugin_api::generated::host::plugins::InstanceState;
use ilium_plugin_host::admission_queue::AdmissionQueue;
use ilium_plugin_host::host_info::limits_record;

use crate::support::*;

// ---------------------------------------------------------------- API-092

/// REQ: API-092
#[test]
fn api_092_three_traps_in_ten_minutes_are_counted_as_three_failures() {
    let (host, _) = trapped_host_at(&[0, 2_000, 4_000]);
    assert_eq!(host.failure_count(), 3);
}

/// REQ: API-092
#[test]
fn api_092_repeated_trap_log_lines_within_ten_seconds_collapse_into_one() {
    let (host, _) = trapped_host_at(&[0, 2_000, 4_000]);
    assert_eq!(log_count(&host, "trap.panic"), 1);
}

// ---------------------------------------------------------------- API-093

/// REQ: API-093
#[test]
fn api_093_enable_clears_the_window_so_one_later_trap_restarts_the_plugin() {
    let (mut host, states) = trapped_host_at(&[0, 2_000, 4_000]);
    assert_eq!(states[2], InstanceState::StoppedByFailure);
    host.enable();
    fail_once(&mut host);
    assert_ne!(host.state(), InstanceState::StoppedByFailure);
    assert_eq!(host.failure_count(), 1);
}

// ---------------------------------------------------------------- API-094

/// REQ: API-094
#[test]
fn api_094_limits_record_names_pending_requests_with_value_and_source() {
    let record = limits_record();
    let pending = record
        .iter()
        .find(|entry| entry.name == "pending_requests")
        .expect("the limits record lists pending_requests");
    assert_eq!(pending.value, 64);
    assert!(matches!(pending.source.as_str(), "default" | "manifest" | "user"));
}

// ---------------------------------------------------------------- API-095

/// REQ: API-095
#[test]
fn api_095_refused_sides_are_admitted_first_in_first_out() {
    let mut queue = AdmissionQueue::new();
    queue.refuse("vendor.first", "client");
    queue.refuse("vendor.second", "client");
    assert_eq!(queue.admit_next(1).as_deref(), Some("vendor.first"));
}

/// REQ: API-095
#[test]
fn api_095_one_plugin_never_takes_two_slots_while_another_waits() {
    let mut queue = AdmissionQueue::new();
    queue.refuse("vendor.first", "server");
    queue.refuse("vendor.first", "client");
    queue.refuse("vendor.second", "server");
    assert_eq!(queue.admit_next(1).as_deref(), Some("vendor.first"));
    assert_eq!(queue.admit_next(1).as_deref(), Some("vendor.second"));
}

/// REQ: API-095
#[test]
fn api_095_waiting_side_shows_a_notice_naming_its_plugin_until_admitted() {
    let mut queue = AdmissionQueue::new();
    queue.refuse("vendor.second", "server");
    let notice = queue
        .notice_for("vendor.second")
        .expect("a waiting side shows a notice");
    assert!(notice.contains("vendor.second"));
    queue.admit_next(1);
    assert!(queue.notice_for("vendor.second").is_none());
}

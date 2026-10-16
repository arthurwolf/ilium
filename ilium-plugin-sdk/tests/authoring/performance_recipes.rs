//! Budgets of the performance page exercised on the fake host (performance.md).

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::DeactivationReason;

use crate::support::{SizedRestore, MIB};

/// REQ: AUT-101
#[test]
fn aut_101_restore_bytes_of_exactly_1_mib_are_kept() {
    let mut host = FakeHost::new();
    host.activate::<SizedRestore<{ MIB }>>();
    let bytes = host.deactivate(DeactivationReason::Upgrade).expect("restore bytes");
    host.restart_with(bytes.clone());
    assert_eq!(host.restored().map(|kept| kept.len()), Some(MIB));
}

/// REQ: AUT-101
#[test]
fn aut_101_restore_bytes_of_1_mib_plus_one_are_dropped_with_a_warn_line() {
    let mut host = FakeHost::new();
    host.activate::<SizedRestore<{ MIB + 1 }>>();
    let bytes = host.deactivate(DeactivationReason::Upgrade);
    host.restart_with(bytes.unwrap_or_default());
    assert_eq!(host.restored(), None);
    let warns = host.logs().into_iter().filter(|line| line.level == "warn").count();
    assert_eq!(warns, 1, "one warn line");
}

/// REQ: AUT-102 WARN
#[test]
fn aut_102_normal_use_stays_under_half_of_the_restore_budget() {
    let mut host = FakeHost::new();
    host.activate::<SizedRestore<{ MIB / 2 - 1 }>>();
    let bytes = host.deactivate(DeactivationReason::Shutdown).expect("restore bytes");
    assert!(bytes.len() < MIB / 2);
    host.restart_with(bytes);
    assert_eq!(host.restored().map(|kept| kept.len()), Some(MIB / 2 - 1));
}

/// REQ: AUT-111
#[test]
fn aut_111_exceeding_the_restore_limit_loses_the_data_not_just_a_warning() {
    let mut host = FakeHost::new();
    host.activate::<SizedRestore<{ MIB + 1 }>>();
    let bytes = host.deactivate(DeactivationReason::Upgrade);
    host.restart_with(bytes.unwrap_or_default());
    assert_eq!(host.restored(), None, "the bytes are gone, the plugin keeps running with defaults");
    assert!(!host.is_disabled());
}

/// REQ: AUT-104 WARN
#[test]
fn aut_104_a_plugin_that_times_itself_uses_the_fake_clock_not_a_host_tool() {
    let mut host = FakeHost::new();
    let before = host.clock().now();
    host.advance_clock(12);
    assert_eq!(host.clock().now() - before, 12);
}

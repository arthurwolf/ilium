//! reference-test-harness.md "Traps, failures and restarts": DEV-490 to DEV-498 (and DEV-485 via
//! the single-host restart rule; the session variant is in `reference_test_harness_session.rs`).

use ilium_plugin_sdk_core::host::InstanceState;
use ilium_plugin_sdk_core::types::{ErrorCode, Event, PluginError};
use ilium_plugin_testkit::FakeHost;
use ilium_plugin_testkit::errors::codes;

use crate::common::{
    DEFAULT_CLOCK_MS, Spy, assert_fails_with, client_batch, spy, spy_reset, temp_dir, with_clean_env,
    write_file,
};

const MINUTE: u64 = 60_000;

fn active() -> FakeHost {
    spy_reset();
    let mut host = FakeHost::new();
    host.activate::<Spy>();
    host
}

fn fail_at(host: &mut FakeHost, minutes_after_start: u64) {
    let target = DEFAULT_CLOCK_MS + minutes_after_start * MINUTE;
    let now = host.clock().now();
    if target > now {
        host.advance_clock(target - now);
    }
    host.inject_trap("trap.panic");
    host.deliver(client_batch(1));
}

// ---- DEV-490, DEV-485 ------------------------------------------------------------------------

/// REQ: DEV-490, DEV-485
#[test]
fn dev_490_first_failure_schedules_a_restart_after_one_second() {
    with_clean_env(|| {
        let mut host = active();
        fail_at(&mut host, 0);
        let restarts = host.restarts();
        assert_eq!(restarts.len(), 1);
        assert_eq!(restarts[0].delay_ms, 1_000);
        assert_eq!(host.lifecycle_state(), InstanceState::RestartScheduled);
        assert!(!host.is_disabled());
    });
}

/// REQ: DEV-490
#[test]
fn dev_490_second_failure_inside_the_window_restarts_after_five_seconds() {
    with_clean_env(|| {
        let mut host = active();
        fail_at(&mut host, 0);
        host.advance_clock(1_000);
        fail_at(&mut host, 2);
        let delays: Vec<u32> = host.restarts().iter().map(|r| r.delay_ms).collect();
        assert_eq!(delays, vec![1_000, 5_000]);
    });
}

/// REQ: DEV-490, DEV-498
#[test]
fn dev_490_third_failure_inside_the_window_disables_the_side() {
    with_clean_env(|| {
        let mut host = active();
        fail_at(&mut host, 0);
        host.advance_clock(1_000);
        fail_at(&mut host, 2);
        host.advance_clock(5_000);
        fail_at(&mut host, 4);
        assert!(host.is_disabled());
        assert_eq!(host.lifecycle_state(), InstanceState::StoppedByFailure);
        assert_eq!(host.restarts().len(), 2, "no third restart is scheduled");
    });
}

/// REQ: DEV-490
#[test]
fn dev_490_a_failure_expires_after_ten_minutes_so_two_failures_eleven_minutes_apart_both_restart_after_one_second() {
    with_clean_env(|| {
        let mut host = active();
        fail_at(&mut host, 0);
        host.advance_clock(1_000);
        fail_at(&mut host, 11);
        let delays: Vec<u32> = host.restarts().iter().map(|r| r.delay_ms).collect();
        assert_eq!(delays, vec![1_000, 1_000]);
    });
}

/// REQ: DEV-490
#[test]
fn dev_490_failures_at_0_6_and_11_minutes_give_one_second_then_five_seconds_and_no_disable() {
    with_clean_env(|| {
        let mut host = active();
        fail_at(&mut host, 0);
        host.advance_clock(1_000);
        fail_at(&mut host, 6);
        host.advance_clock(5_000);
        fail_at(&mut host, 11);
        let delays: Vec<u32> = host.restarts().iter().map(|r| r.delay_ms).collect();
        assert_eq!(delays, vec![1_000, 5_000, 5_000]);
        assert!(!host.is_disabled());
    });
}

/// REQ: DEV-490
#[test]
fn dev_490_a_failure_exactly_ten_minutes_old_is_expired() {
    with_clean_env(|| {
        let mut host = active();
        fail_at(&mut host, 0);
        host.advance_clock(1_000);
        fail_at(&mut host, 10);
        let delays: Vec<u32> = host.restarts().iter().map(|r| r.delay_ms).collect();
        assert_eq!(delays, vec![1_000, 1_000]);
    });
}

/// REQ: DEV-490
#[test]
fn dev_490_a_failure_nine_minutes_old_still_counts() {
    with_clean_env(|| {
        let mut host = active();
        fail_at(&mut host, 0);
        host.advance_clock(1_000);
        fail_at(&mut host, 9);
        let delays: Vec<u32> = host.restarts().iter().map(|r| r.delay_ms).collect();
        assert_eq!(delays, vec![1_000, 5_000]);
    });
}

// ---- DEV-491, DEV-492 ------------------------------------------------------------------------

/// REQ: DEV-491
#[test]
fn dev_491_a_restarted_instance_gets_a_new_plugin_object_and_tree_changed() {
    with_clean_env(|| {
        let mut host = active();
        let before = spy().constructed;
        fail_at(&mut host, 0);
        host.advance_clock(1_000);
        host.settle();
        assert_eq!(spy().constructed, before + 1, "fresh in-memory state");
        let tree_changed = spy().batches.iter().flatten().any(|event| matches!(event, Event::TreeChanged(_)));
        assert!(tree_changed, "restart replays tree-changed for every node");
    });
}

/// REQ: DEV-492
#[test]
fn dev_492_a_disabled_plugin_receives_no_events() {
    with_clean_env(|| {
        let mut host = active();
        fail_at(&mut host, 0);
        host.advance_clock(1_000);
        fail_at(&mut host, 1);
        host.advance_clock(5_000);
        fail_at(&mut host, 2);
        assert!(host.is_disabled());
        let seen = spy().batches.len();
        host.deliver(client_batch(3));
        host.settle();
        assert_eq!(spy().batches.len(), seen);
    });
}

/// REQ: DEV-492
#[test]
fn dev_492_a_healthy_plugin_is_not_disabled() {
    with_clean_env(|| {
        let host = active();
        assert!(!host.is_disabled());
        assert_eq!(host.lifecycle_state(), InstanceState::Active);
    });
}

// ---- DEV-493 ---------------------------------------------------------------------------------

/// REQ: DEV-493
#[test]
fn dev_493_inject_failure_with_a_code_outside_the_import_catalogue_fails_with_error_unknown() {
    with_clean_env(|| {
        let mut host = active();
        assert_fails_with(codes::ERROR_UNKNOWN, || {
            host.inject_failure("clock.now-unix-millis", PluginError::new(ErrorCode::QuotaExceeded, "x"))
        });
    });
}

/// REQ: DEV-493
#[test]
fn dev_493_inject_failure_with_a_catalogue_code_is_accepted() {
    with_clean_env(|| {
        let mut host = active();
        host.inject_failure("storage.put", PluginError::new(ErrorCode::QuotaExceeded, "full"));
        assert!(host.failures().is_empty(), "an import failure is not a plugin failure");
    });
}

// ---- DEV-494 ---------------------------------------------------------------------------------

/// REQ: DEV-494
#[test]
fn dev_494_restart_with_deactivates_for_upgrade_and_activates_with_the_bytes() {
    with_clean_env(|| {
        let mut host = active();
        host.restart_with(vec![4, 5]);
        let log = spy();
        assert_eq!(log.deactivations.last().map(|r| format!("{r:?}")), Some("Upgrade".to_string()));
        assert_eq!(log.activations.last().expect("activation").instance, 2);
        assert_eq!(log.activations.last().expect("activation").restored, Some(vec![4, 5]));
        assert_eq!(host.restored(), Some(vec![4, 5]));
    });
}

/// REQ: DEV-494
#[test]
fn dev_494_restart_with_discards_the_bytes_deactivate_returned_and_records_no_failure() {
    with_clean_env(|| {
        let mut host = active();
        crate::common::spy_edit(|state| state.restore = Some(vec![0xAA]));
        host.restart_with(vec![1]);
        assert_eq!(host.restored(), Some(vec![1]));
        assert!(host.failures().is_empty());
        assert!(host.restarts().is_empty());
    });
}

// ---- DEV-495 ---------------------------------------------------------------------------------

/// REQ: DEV-495
#[test]
fn dev_495_inject_trap_with_a_table_code_counts_as_a_failure() {
    with_clean_env(|| {
        let mut host = active();
        host.inject_trap("trap.deadline");
        host.deliver(client_batch(1));
        let failures = host.failures();
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].code, "trap.deadline");
    });
}

/// REQ: DEV-495
#[test]
fn dev_495_inject_trap_with_an_unknown_code_fails_with_error_unknown() {
    with_clean_env(|| {
        let mut host = active();
        assert_fails_with(codes::ERROR_UNKNOWN, || host.inject_trap("trap.not-real"));
        assert_fails_with(codes::ERROR_UNKNOWN, || host.inject_trap("deadline"));
    });
}

/// REQ: DEV-495
#[test]
fn dev_495_every_code_of_the_trap_table_is_accepted() {
    with_clean_env(|| {
        for code in codes::TRAP_CODES {
            let mut host = active();
            host.inject_trap(code);
            host.deliver(client_batch(1));
            assert_eq!(host.failures()[0].code, *code);
        }
    });
}

/// REQ: DEV-495
#[test]
fn dev_495_three_injected_traps_in_ten_minutes_disable_the_side() {
    with_clean_env(|| {
        let mut host = active();
        for minute in 0..3 {
            fail_at(&mut host, minute);
            host.advance_clock(6_000);
        }
        assert!(host.is_disabled());
    });
}

// ---- DEV-496 ---------------------------------------------------------------------------------

/// REQ: DEV-496
#[test]
fn dev_496_a_journal_entry_written_before_the_crash_is_visible_to_the_next_activation() {
    with_clean_env(|| {
        let mut host = active();
        host.crash_after_journal_write();
        host.deliver(client_batch(1));
        assert_eq!(host.failures().len(), 1);
        host.advance_clock(1_000);
        host.settle();
        assert!(spy().activations.len() >= 2, "the plugin reactivates after the crash");
    });
}

// ---- DEV-497 ---------------------------------------------------------------------------------

/// REQ: DEV-497
#[test]
fn dev_497_with_storage_snapshot_after_activate_fails_with_setup_closed() {
    with_clean_env(|| {
        spy_reset();
        let path = write_file(&temp_dir("t497a"), "s.json", "{}");
        let mut host = FakeHost::new();
        host.activate::<Spy>();
        assert_fails_with(codes::SETUP_CLOSED, || host.with_storage_snapshot(&path));
    });
}

/// REQ: DEV-497
#[test]
fn dev_497_with_data_version_after_activate_fails_with_setup_closed() {
    with_clean_env(|| {
        let mut host = active();
        assert_fails_with(codes::SETUP_CLOSED, || host.with_data_version(2));
    });
}

/// REQ: DEV-497
#[test]
fn dev_497_a_snapshot_with_an_unknown_shape_fails_with_snapshot_invalid() {
    with_clean_env(|| {
        let dir = temp_dir("t497b");
        for (name, body) in [
            ("list.json", "[1,2,3]"),
            ("bucket.json", r#"{"unknown-bucket":{"k":"AQ=="}}"#),
            ("value.json", r#"{"global":{"k":42}}"#),
            ("base64.json", r#"{"global":{"k":"!!not base64!!"}}"#),
            ("garbage.json", "not json"),
        ] {
            let path = write_file(&dir, name, body);
            assert_fails_with(codes::SNAPSHOT_INVALID, || {
                let mut host = FakeHost::new().with_storage_snapshot(&path);
                host.activate::<Spy>();
            });
        }
    });
}

// ---- DEV-498 ---------------------------------------------------------------------------------

/// REQ: DEV-498
#[test]
fn dev_498_failures_are_listed_oldest_first_with_time_and_code() {
    with_clean_env(|| {
        let mut host = active();
        fail_at(&mut host, 0);
        host.advance_clock(1_000);
        host.inject_trap("trap.memory");
        host.deliver(client_batch(1));
        let failures = host.failures();
        assert_eq!(failures.len(), 2);
        assert_eq!(failures[0].code, "trap.panic");
        assert_eq!(failures[0].at_ms, DEFAULT_CLOCK_MS);
        assert_eq!(failures[1].code, "trap.memory");
        assert!(failures[0].at_ms < failures[1].at_ms);
    });
}

/// REQ: DEV-498
#[test]
fn dev_498_failures_older_than_ten_minutes_drop_out_of_the_list() {
    with_clean_env(|| {
        let mut host = active();
        fail_at(&mut host, 0);
        host.advance_clock(11 * MINUTE);
        assert!(host.failures().is_empty());
    });
}

/// REQ: DEV-498
#[test]
fn dev_498_after_one_failure_the_state_is_restart_scheduled_and_after_the_restart_it_is_active() {
    with_clean_env(|| {
        let mut host = active();
        fail_at(&mut host, 0);
        assert_eq!(host.lifecycle_state(), InstanceState::RestartScheduled);
        host.advance_clock(1_000);
        host.settle();
        assert_eq!(host.lifecycle_state(), InstanceState::Active);
    });
}

/// REQ: DEV-498
#[test]
fn dev_498_a_host_that_never_activated_is_inactive() {
    with_clean_env(|| {
        let host = FakeHost::new();
        assert_eq!(host.lifecycle_state(), InstanceState::Inactive);
    });
}

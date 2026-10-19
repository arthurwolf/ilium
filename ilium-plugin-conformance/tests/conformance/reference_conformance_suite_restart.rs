//! reference-conformance-suite.md "Restart rule": 1 s after the first failure, 5 s after the
//! second, disabled by the third inside 10 minutes. Everything runs on fake time (DEV-611): a
//! `FakeHost` clock for the host subject and `RestartPolicy` for the pure rule.

use std::time::Instant;

use ilium_plugin_api::lifecycle::{
    RestartDecision, RestartPolicy, FAILURE_WINDOW_MS, FIRST_RESTART_DELAY_MS,
    SECOND_RESTART_DELAY_MS,
};
use ilium_plugin_testkit::{FakeHost, Restart};
use proptest::prelude::*;

use crate::common::{manifest_path, BOUND};

const MINUTE_MS: u64 = 60_000;

fn host() -> FakeHost {
    FakeHost::for_manifest(manifest_path("minimal.json")).expect("manifest loads")
}

/// Traps the host at fake time `at_ms` (moving the clock forward first).
fn trap_at(host: &mut FakeHost, at_ms: u64) {
    host.set_clock(at_ms);
    host.inject_trap("trap.deadline");
}

// ---- CONF-RESTART-01 ----

/// REQ: CONF-RESTART-01
#[test]
fn conf_restart_01_first_failure_restarts_after_1000_ms() {
    let mut host = host();
    trap_at(&mut host, 0);
    assert_eq!(
        host.restarts(),
        vec![Restart {
            at_ms: 0,
            delay_ms: 1_000
        }]
    );
    assert!(!host.is_disabled());
}

/// REQ: CONF-RESTART-01
#[test]
fn conf_restart_01_pure_rule_first_failure_is_restart_after_1000_ms() {
    let mut policy = RestartPolicy::default();
    assert_eq!(policy.on_failure(0), RestartDecision::RestartAfterMs(1_000));
    assert_eq!(FIRST_RESTART_DELAY_MS, 1_000);
}

/// REQ: CONF-RESTART-01
#[test]
fn conf_restart_01_no_restart_is_recorded_before_the_first_failure() {
    let host = host();
    assert!(host.restarts().is_empty());
    assert!(!host.is_disabled());
}

// ---- CONF-RESTART-02 ----

/// REQ: CONF-RESTART-02
#[test]
fn conf_restart_02_second_failure_inside_ten_minutes_restarts_after_5000_ms() {
    let mut host = host();
    trap_at(&mut host, 0);
    trap_at(&mut host, 30_000);
    let restarts = host.restarts();
    assert_eq!(restarts.len(), 2);
    assert_eq!(
        restarts[1],
        Restart {
            at_ms: 30_000,
            delay_ms: 5_000
        }
    );
}

/// REQ: CONF-RESTART-02
#[test]
fn conf_restart_02_pure_rule_second_failure_is_restart_after_5000_ms() {
    let mut policy = RestartPolicy::default();
    let _ = policy.on_failure(0);
    assert_eq!(policy.on_failure(1), RestartDecision::RestartAfterMs(5_000));
    assert_eq!(SECOND_RESTART_DELAY_MS, 5_000);
    assert_eq!(policy.failures_in_window(1), 2);
}

/// REQ: CONF-RESTART-02
#[test]
fn conf_restart_02_second_failure_one_millisecond_before_the_window_ends_still_counts() {
    let mut policy = RestartPolicy::default();
    let _ = policy.on_failure(0);
    assert_eq!(
        policy.on_failure(FAILURE_WINDOW_MS - 1),
        RestartDecision::RestartAfterMs(5_000)
    );
}

// ---- CONF-RESTART-03 ----

/// REQ: CONF-RESTART-03
#[test]
fn conf_restart_03_third_failure_inside_ten_minutes_disables_the_plugin() {
    let mut host = host();
    for at in [0, 10_000, 20_000] {
        trap_at(&mut host, at);
    }
    assert!(host.is_disabled());
    assert_eq!(
        host.restarts().len(),
        2,
        "the third failure schedules no restart"
    );
}

/// REQ: CONF-RESTART-03
#[test]
fn conf_restart_03_pure_rule_third_failure_is_disable() {
    let mut policy = RestartPolicy::default();
    let _ = policy.on_failure(0);
    let _ = policy.on_failure(1);
    assert_eq!(policy.on_failure(2), RestartDecision::Disable);
}

/// REQ: CONF-RESTART-03
#[test]
fn conf_restart_03_third_failure_at_window_minus_one_ms_disables() {
    let mut policy = RestartPolicy::default();
    let _ = policy.on_failure(0);
    let _ = policy.on_failure(1);
    assert_eq!(
        policy.on_failure(FAILURE_WINDOW_MS - 1),
        RestartDecision::Disable
    );
}

/// REQ: CONF-RESTART-03
#[test]
fn conf_restart_03_third_failure_at_window_plus_one_ms_does_not_disable() {
    let mut policy = RestartPolicy::default();
    let _ = policy.on_failure(0);
    let _ = policy.on_failure(300_000);
    assert_eq!(
        policy.on_failure(FAILURE_WINDOW_MS + 1),
        RestartDecision::RestartAfterMs(5_000)
    );
}

/// REQ: CONF-RESTART-03
#[test]
fn conf_restart_03_reset_after_a_manual_enable_forgets_every_failure() {
    let mut policy = RestartPolicy::default();
    let _ = policy.on_failure(0);
    let _ = policy.on_failure(1);
    let _ = policy.on_failure(2);
    policy.reset();
    assert_eq!(policy.failures_in_window(3), 0);
    assert_eq!(policy.on_failure(3), RestartDecision::RestartAfterMs(1_000));
}

// ---- CONF-RESTART-04 ----

/// REQ: CONF-RESTART-04
#[test]
fn conf_restart_04_traps_at_0_6_and_11_minutes_restart_after_1_5_and_5_seconds() {
    let mut host = host();
    for at in [0, 6 * MINUTE_MS, 11 * MINUTE_MS] {
        trap_at(&mut host, at);
    }
    let delays: Vec<u32> = host
        .restarts()
        .iter()
        .map(|restart| restart.delay_ms)
        .collect();
    assert_eq!(delays, vec![1_000, 5_000, 5_000]);
    assert!(!host.is_disabled());
}

/// REQ: CONF-RESTART-04
#[test]
fn conf_restart_04_pure_rule_expires_the_oldest_failure() {
    let mut policy = RestartPolicy::default();
    assert_eq!(policy.on_failure(0), RestartDecision::RestartAfterMs(1_000));
    assert_eq!(
        policy.on_failure(6 * MINUTE_MS),
        RestartDecision::RestartAfterMs(5_000)
    );
    assert_eq!(
        policy.on_failure(11 * MINUTE_MS),
        RestartDecision::RestartAfterMs(5_000)
    );
    assert_eq!(policy.failures_in_window(11 * MINUTE_MS), 2);
}

/// REQ: CONF-RESTART-04
#[test]
fn conf_restart_04_the_window_is_ten_minutes() {
    assert_eq!(FAILURE_WINDOW_MS, 10 * MINUTE_MS);
}

// ---- CONF-RESTART-05 ----

/// REQ: CONF-RESTART-05
#[test]
fn conf_restart_05_traps_at_0_and_11_minutes_both_restart_after_1_second() {
    let mut host = host();
    trap_at(&mut host, 0);
    trap_at(&mut host, 11 * MINUTE_MS);
    let delays: Vec<u32> = host
        .restarts()
        .iter()
        .map(|restart| restart.delay_ms)
        .collect();
    assert_eq!(delays, vec![1_000, 1_000]);
}

/// REQ: CONF-RESTART-05
#[test]
fn conf_restart_05_pure_rule_a_lone_old_failure_does_not_count() {
    let mut policy = RestartPolicy::default();
    let _ = policy.on_failure(0);
    assert_eq!(
        policy.on_failure(11 * MINUTE_MS),
        RestartDecision::RestartAfterMs(1_000)
    );
    assert_eq!(policy.failures_in_window(11 * MINUTE_MS), 1);
}

/// REQ: CONF-RESTART-05
#[test]
fn conf_restart_05_second_failure_exactly_at_the_window_edge_is_a_documented_boundary() {
    // n-1 counts, n+1 does not; the edge itself must be decided one way and stay there.
    let mut at_edge = RestartPolicy::default();
    let _ = at_edge.on_failure(0);
    let edge = at_edge.on_failure(FAILURE_WINDOW_MS);
    let mut before = RestartPolicy::default();
    let _ = before.on_failure(0);
    let inside = before.on_failure(FAILURE_WINDOW_MS - 1);
    let mut after = RestartPolicy::default();
    let _ = after.on_failure(0);
    let outside = after.on_failure(FAILURE_WINDOW_MS + 1);
    assert_eq!(inside, RestartDecision::RestartAfterMs(5_000));
    assert_eq!(outside, RestartDecision::RestartAfterMs(1_000));
    // The manual says a restart "that follows a 10-minute quiet period" starts the count again:
    // a gap of exactly 10 minutes is a full quiet period, so the edge counts as outside.
    assert_eq!(edge, RestartDecision::RestartAfterMs(1_000));
}

// ---- DEV-611: fake time only ----

/// REQ: CONF-RESTART-04, DEV-611
#[test]
fn dev_611_restart_cases_advance_eleven_minutes_of_fake_time_in_less_than_the_bound() {
    let started = Instant::now();
    let mut host = host();
    trap_at(&mut host, 0);
    host.advance_clock(11 * MINUTE_MS);
    trap_at(&mut host, 11 * MINUTE_MS);
    assert!(started.elapsed() < BOUND);
    assert_eq!(host.restarts().len(), 2);
}

proptest! {
    /// REQ: CONF-RESTART-01, CONF-RESTART-02, CONF-RESTART-03, CONF-RESTART-04, CONF-RESTART-05
    #[test]
    fn conf_restart_0x_pure_rule_matches_the_window_model(gaps in proptest::collection::vec(0u64..400_000, 1..12)) {
        let mut policy = RestartPolicy::default();
        let mut failures: Vec<u64> = Vec::new();
        let mut now = 0u64;
        for gap in gaps {
            now += gap;
            failures.retain(|at| now - *at < FAILURE_WINDOW_MS);
            failures.push(now);
            let expected = match failures.len() {
                1 => RestartDecision::RestartAfterMs(1_000),
                2 => RestartDecision::RestartAfterMs(5_000),
                _ => RestartDecision::Disable,
            };
            let actual = policy.on_failure(now);
            prop_assert_eq!(actual, expected, "failures {:?}", failures);
            if actual == RestartDecision::Disable {
                break;
            }
        }
    }
}

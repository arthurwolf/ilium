//! reference-errors-and-limits.md (HST-496..499) and the soft-deadline rule of
//! reference-host-scheduling.md.

use ilium_plugin_api::ErrorCode;
use ilium_plugin_host::calls::CallClass;
use ilium_plugin_host::limits_rules::{
    client_request_timeout_ms, reply_deadline_ms, retry_after_ms, soft_breach_verdict,
    ProcessQuota, ReplyKind, SoftVerdict,
};
use proptest::prelude::*;

fn expected_retry_after(balance_ms: i64) -> i64 {
    let raw = (-balance_ms * 2 + 9).div_euclid(10) * 10;
    raw.max(100)
}

/// REQ: HST-496
#[test]
fn hst_496_a_positive_balance_waits_the_minimum() {
    assert_eq!(retry_after_ms(100), 100);
}

/// REQ: HST-496
#[test]
fn hst_496_a_small_deficit_still_waits_the_minimum() {
    assert_eq!(retry_after_ms(-25), 100);
}

/// REQ: HST-496
#[test]
fn hst_496_a_deficit_of_33_rounds_up_to_100() {
    assert_eq!(retry_after_ms(-33), 100);
}

/// REQ: HST-496
#[test]
fn hst_496_an_empty_bucket_waits_the_minimum() {
    assert_eq!(retry_after_ms(0), 100);
}

/// REQ: HST-496
#[test]
fn hst_496_a_deficit_of_100_waits_200() {
    assert_eq!(retry_after_ms(-100), 200);
}

/// REQ: HST-496
#[test]
fn hst_496_a_deficit_of_51_rounds_up_to_110() {
    assert_eq!(retry_after_ms(-51), 110);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]
    // REQ: HST-496
    #[test]
    fn hst_496_matches_the_formula_and_stays_in_range(bucket in -500i64..=100) {
        let value = retry_after_ms(bucket);
        prop_assert_eq!(value, expected_retry_after(bucket));
        prop_assert!(value >= 100);
        prop_assert_eq!(value % 10, 0);
    }
}

/// REQ: HST-497
#[test]
fn hst_497_fifteen_children_leave_room_for_one_more() {
    let mut quota = ProcessQuota::new();
    for _ in 0..15 {
        quota.spawn().expect("within limit");
    }
    assert!(quota.run().is_ok());
}

/// REQ: HST-497
#[test]
fn hst_497_sixteen_children_fill_the_session() {
    let mut quota = ProcessQuota::new();
    for _ in 0..16 {
        quota.spawn().expect("within limit");
    }
    assert_eq!(quota.live(), 16);
}

/// REQ: HST-497
#[test]
fn hst_497_a_full_session_makes_run_return_busy() {
    let mut quota = ProcessQuota::new();
    for _ in 0..16 {
        quota.spawn().expect("within limit");
    }
    assert_eq!(quota.run().expect_err("full").code, ErrorCode::Busy);
}

/// REQ: HST-497
#[test]
fn hst_497_a_full_session_makes_spawn_return_quota_exceeded() {
    let mut quota = ProcessQuota::new();
    for _ in 0..16 {
        quota.spawn().expect("within limit");
    }
    assert_eq!(
        quota.spawn().expect_err("seventeenth").code,
        ErrorCode::QuotaExceeded
    );
}

/// REQ: HST-497
#[test]
fn hst_497_run_and_spawn_children_share_one_count() {
    let mut quota = ProcessQuota::new();
    for _ in 0..8 {
        quota.spawn().expect("spawn");
    }
    for _ in 0..8 {
        quota.hold_run().expect("run child");
    }
    assert_eq!(quota.live(), 16);
    assert_eq!(
        quota.spawn().expect_err("seventeenth").code,
        ErrorCode::QuotaExceeded
    );
}

/// REQ: HST-497
#[test]
fn hst_497_an_exited_child_frees_a_slot() {
    let mut quota = ProcessQuota::new();
    for _ in 0..16 {
        quota.spawn().expect("within limit");
    }
    quota.exit_one();
    assert!(quota.spawn().is_ok());
}

/// REQ: HST-498
#[test]
fn hst_498_streak_two_does_not_trap() {
    assert_eq!(
        soft_breach_verdict(CallClass::Annotate, 2, 10),
        SoftVerdict::Tolerated
    );
}

/// REQ: HST-498
#[test]
fn hst_498_streak_three_traps_and_counts_as_a_failure() {
    assert_eq!(
        soft_breach_verdict(CallClass::Annotate, 3, 10),
        SoftVerdict::TrapCounted
    );
}

/// REQ: HST-498
#[test]
fn hst_498_streak_four_traps_and_counts_as_a_failure() {
    assert_eq!(
        soft_breach_verdict(CallClass::Detector, 4, 10),
        SoftVerdict::TrapCounted
    );
}

/// REQ: HST-498
#[test]
fn hst_498_overshoot_99_ms_is_tolerated() {
    assert_eq!(
        soft_breach_verdict(CallClass::Annotate, 1, 99),
        SoftVerdict::Tolerated
    );
}

/// REQ: HST-498
#[test]
fn hst_498_overshoot_100_ms_traps() {
    assert_eq!(
        soft_breach_verdict(CallClass::Annotate, 1, 100),
        SoftVerdict::TrapCounted
    );
}

/// REQ: HST-498
#[test]
fn hst_498_overshoot_101_ms_traps() {
    assert_eq!(
        soft_breach_verdict(CallClass::Annotate, 1, 101),
        SoftVerdict::TrapCounted
    );
}

/// REQ: HST-498
#[test]
fn hst_498_scene_frame_streaks_29_30_31() {
    assert_eq!(
        soft_breach_verdict(CallClass::Frame, 29, 10),
        SoftVerdict::Tolerated
    );
    assert_eq!(
        soft_breach_verdict(CallClass::Frame, 30, 10),
        SoftVerdict::TrapCounted
    );
    assert_eq!(
        soft_breach_verdict(CallClass::Frame, 31, 10),
        SoftVerdict::TrapCounted
    );
}

/// REQ: HST-498
#[test]
fn hst_498_a_scene_frame_streak_of_three_is_still_tolerated() {
    assert_eq!(
        soft_breach_verdict(CallClass::Frame, 3, 10),
        SoftVerdict::Tolerated
    );
}

/// REQ: HST-498
#[test]
fn hst_498_a_hook_handler_breach_disables_the_handler_without_a_failure() {
    assert_eq!(
        soft_breach_verdict(CallClass::Annotate, 1, 10).clone(),
        SoftVerdict::Tolerated
    );
    assert_eq!(
        ilium_plugin_host::limits_rules::hook_handler_breach_verdict(3),
        SoftVerdict::DisableHandlerNoFailure
    );
}

/// REQ: HST-498
#[test]
fn hst_498_a_hook_handler_below_the_streak_is_only_tolerated() {
    assert_eq!(
        ilium_plugin_host::limits_rules::hook_handler_breach_verdict(2),
        SoftVerdict::Tolerated
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]
    // REQ: HST-498
    #[test]
    fn hst_498_trap_iff_streak_or_overshoot_reaches_its_limit(streak in 0u32..60, over in 0u64..300) {
        let verdict = soft_breach_verdict(CallClass::Annotate, streak, over);
        let expect_trap = streak >= 3 || over >= 100;
        prop_assert_eq!(verdict == SoftVerdict::TrapCounted, expect_trap);
    }
}

/// REQ: HST-499
#[test]
fn hst_499_fixed_reply_deadlines() {
    assert_eq!(reply_deadline_ms(ReplyKind::Command), 30_000);
    assert_eq!(reply_deadline_ms(ReplyKind::Service), 30_000);
    assert_eq!(reply_deadline_ms(ReplyKind::InferenceCompletion), 120_000);
    assert_eq!(reply_deadline_ms(ReplyKind::HttpApiRespond), 30_000);
    assert_eq!(reply_deadline_ms(ReplyKind::ClientRequestDefault), 5_000);
    assert_eq!(reply_deadline_ms(ReplyKind::JobCancelGrace), 1_000);
}

/// REQ: HST-499
#[test]
fn hst_499_client_request_timeout_zero_is_clamped_to_one() {
    assert_eq!(client_request_timeout_ms(0), 1);
}

/// REQ: HST-499
#[test]
fn hst_499_client_request_timeout_bounds_are_kept() {
    assert_eq!(client_request_timeout_ms(1), 1);
    assert_eq!(client_request_timeout_ms(30_000), 30_000);
}

/// REQ: HST-499
#[test]
fn hst_499_client_request_timeout_30001_is_clamped_to_30000() {
    assert_eq!(client_request_timeout_ms(30_001), 30_000);
}

/// REQ: HST-499
#[test]
fn hst_499_a_silent_provider_returns_timed_out_after_the_deadline() {
    for kind in [
        ReplyKind::Command,
        ReplyKind::Service,
        ReplyKind::InferenceCompletion,
        ReplyKind::HttpApiRespond,
        ReplyKind::ClientRequestDefault,
        ReplyKind::JobCancelGrace,
    ] {
        let mut waiter = ilium_plugin_host::limits_rules::ReplyWaiter::new(kind);
        waiter.advance_ms(reply_deadline_ms(kind) - 1);
        assert!(waiter.result().is_none(), "{kind:?} early");
        waiter.advance_ms(1);
        assert_eq!(
            waiter.result().expect("deadline reached").code,
            ErrorCode::TimedOut,
            "{kind:?}"
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    // REQ: HST-499
    #[test]
    fn hst_499_requested_timeout_is_always_clamped_into_range(requested in 0u32..100_000) {
        let value = client_request_timeout_ms(requested);
        prop_assert!((1..=30_000).contains(&value));
    }
}

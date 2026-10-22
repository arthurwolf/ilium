//! Overflow and release boundaries of the pure host limit rules.
use ilium_plugin_api::ErrorCode;
use ilium_plugin_host::limits_rules::{ProcessQuota, ReplyKind, ReplyWaiter, retry_after_ms};

#[test]
fn retry_hint_handles_extreme_balances_without_overflow() {
    assert_eq!(retry_after_ms(i64::MAX), 100);
    assert_eq!(retry_after_ms(i64::MIN), i64::MAX / 10 * 10);
}

#[test]
fn completed_short_children_and_empty_releases_do_not_consume_capacity() {
    let mut quota = ProcessQuota::new();
    quota.exit_one();
    for _ in 0..100 {
        quota.run().expect("short child completes");
    }
    assert_eq!(quota.live(), 0);
    for _ in 0..16 {
        quota.hold_run().expect("shared slot available");
    }
    assert_eq!(quota.run().expect_err("full").code, ErrorCode::Busy);
    quota.exit_one();
    quota.spawn().expect("held run released a shared slot");
    assert_eq!(quota.live(), 16);
}

#[test]
fn reply_time_accumulation_saturates_and_sessions_send_times_out() {
    let mut waiter = ReplyWaiter::new(ReplyKind::SessionsSend);
    waiter.advance_ms(29_999);
    assert!(waiter.result().is_none());
    waiter.advance_ms(u64::MAX);
    waiter.advance_ms(1);
    assert_eq!(
        waiter.result().expect("deadline reached").code,
        ErrorCode::TimedOut
    );
}

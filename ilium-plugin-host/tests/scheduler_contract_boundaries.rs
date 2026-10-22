//! Normative scheduling limits at boundaries omitted by the original sched lane.
use ilium_plugin_api::generated::types::requests::{Outcome, RequestRef};
use ilium_plugin_api::ErrorCode;
use ilium_plugin_host::scheduler::{
    cancellation_outcome, retry_after_ms, CancelCause, ExecutionBucket, RequestLedger,
    StreamCredit, CREDIT_WINDOW_BYTES,
};
use std::time::Duration;

#[test]
fn empty_execution_bucket_refuses_and_small_deficit_waits_at_least_100_ms() {
    let mut bucket = ExecutionBucket::new();
    bucket.charge_cpu_ms(2_000);
    let error = bucket.try_submit().expect_err("zero is empty, HST-207");
    assert_eq!(error.code, ErrorCode::QuotaExceeded);
    assert_eq!(error.retry_after_ms, Some(100));
    assert_eq!(retry_after_ms(-1), 100);
    assert_eq!(retry_after_ms(i64::MIN), u32::MAX / 10 * 10);
}

#[test]
fn fractional_refill_time_accumulates_and_partial_payloads_are_charged() {
    let mut bucket = ExecutionBucket::new();
    bucket.charge_payload_bytes(1);
    assert_eq!(bucket.balance_ms(), 1_999);
    bucket.refill(Duration::from_millis(1));
    assert_eq!(bucket.balance_ms(), 1_999);
    bucket.refill(Duration::from_millis(1));
    assert_eq!(bucket.balance_ms(), 2_000);
}

#[test]
fn credit_hysteresis_and_overflow_cannot_resume_a_full_reader() {
    let mut credit = StreamCredit::new();
    credit.enqueued(CREDIT_WINDOW_BYTES);
    credit.consumed(1);
    assert!(!credit.should_read());
    credit.consumed(CREDIT_WINDOW_BYTES);
    assert!(credit.should_read());
    assert_eq!(credit.in_flight(), 0);
}

#[test]
fn reopening_a_completed_request_cannot_create_a_second_completion() {
    let mut requests = RequestLedger::new();
    requests.open(RequestRef::Submission(7));
    requests.complete(RequestRef::Submission(7), Outcome::Ok);
    requests.open(RequestRef::Submission(7));
    requests.cancel(RequestRef::Submission(7));
    assert_eq!(
        requests.outcome_of(RequestRef::Submission(7)),
        Some(Outcome::Ok)
    );
    assert_eq!(requests.completion_count(RequestRef::Submission(7)), 1);
    assert!(
        matches!(cancellation_outcome(CancelCause::ProviderBreach), Outcome::Failed(error) if error.code == ErrorCode::Internal)
    );
}

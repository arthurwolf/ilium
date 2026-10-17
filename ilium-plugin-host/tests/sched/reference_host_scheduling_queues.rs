//! Event queues, batching, credit windows, rate limits and the execution bucket (HST-202..208).

use proptest::prelude::*;

use ilium_plugin_api::PluginError;
use ilium_plugin_host::scheduler::{
    event_queue_policy, retry_after_ms, BatchBuilder, EventPush, ExecutionBucket, QueuePolicy,
    StreamCredit, BATCH_FLUSH_MS, BATCH_MAX_EVENTS, CREDIT_WINDOW_BYTES, MAX_CHUNK_BYTES,
};

use crate::common::*;

// ---- HST-202 ------------------------------------------------------------------------------

/// REQ: HST-202
#[test]
fn hst_202_theme_and_keymap_coalesce_to_the_latest() {
    assert_eq!(event_queue_policy(&theme()), QueuePolicy::Coalesce);
    assert_eq!(event_queue_policy(&keymap()), QueuePolicy::Coalesce);
}

/// REQ: HST-202
#[test]
fn hst_202_tree_changed_and_pointer_coalesce() {
    assert_eq!(
        event_queue_policy(&tree_changed(&[1])),
        QueuePolicy::Coalesce
    );
    assert_eq!(event_queue_policy(&pointer()), QueuePolicy::Coalesce);
}

/// REQ: HST-202
#[test]
fn hst_202_user_input_kinds_drop_the_oldest() {
    for event in [
        key_pressed(),
        key_released(),
        command_invoked(),
        ui_action(),
    ] {
        assert_eq!(event_queue_policy(&event), QueuePolicy::DropOldest);
    }
}

/// REQ: HST-202
#[test]
fn hst_202_correctness_kinds_are_never_dropped() {
    for event in [
        message(),
        request_completed(),
        durable_timer(),
        input_uncertain(),
    ] {
        assert_eq!(event_queue_policy(&event), QueuePolicy::NeverDrop);
    }
}

/// REQ: HST-202
#[test]
fn hst_202_coalescing_replaces_the_queued_event_when_full() {
    let scheduler = Scheduler::new(10);
    let who = plugin("demo.queue");
    scheduler.register_side(&who, 2);
    assert_eq!(scheduler.push_event(&who, theme()), EventPush::Queued);
    assert_eq!(scheduler.push_event(&who, key_pressed()), EventPush::Queued);
    assert_eq!(scheduler.push_event(&who, theme()), EventPush::Coalesced);
}

/// REQ: HST-202
#[test]
fn hst_202_full_queue_drops_the_oldest_user_input() {
    let scheduler = Scheduler::new(10);
    let who = plugin("demo.queue");
    scheduler.register_side(&who, 2);
    scheduler.push_event(&who, key_pressed());
    scheduler.push_event(&who, key_released());
    assert_eq!(
        scheduler.push_event(&who, command_invoked()),
        EventPush::DroppedOldest
    );
}

/// REQ: HST-202
#[test]
fn hst_202_full_queue_of_pointer_discards_the_incoming_after_coalescing_fails() {
    let scheduler = Scheduler::new(10);
    let who = plugin("demo.queue");
    scheduler.register_side(&who, 1);
    scheduler.push_event(&who, message());
    assert_eq!(
        scheduler.push_event(&who, pointer()),
        EventPush::DiscardedIncoming
    );
}

/// REQ: HST-202
#[test]
fn hst_202_never_dropped_event_into_a_full_queue_marks_the_side_stuck() {
    let scheduler = Scheduler::new(10);
    let who = plugin("demo.queue");
    scheduler.register_side(&who, 2);
    scheduler.push_event(&who, message());
    scheduler.push_event(&who, durable_timer());
    assert_eq!(
        scheduler.push_event(&who, request_completed()),
        EventPush::Stuck
    );
}

/// REQ: HST-202
#[test]
fn hst_202_one_below_capacity_never_overflows() {
    let scheduler = Scheduler::new(10);
    let who = plugin("demo.queue");
    scheduler.register_side(&who, 3);
    scheduler.push_event(&who, message());
    assert_eq!(scheduler.push_event(&who, message()), EventPush::Queued);
}

/// REQ: HST-202
#[test]
fn hst_202_stuck_side_counts_as_a_failure() {
    let kernel = kernel_with_nodes(1);
    let handle = build_with(
        HostHarness::new().kernel(kernel),
        ScriptedGuest::new(Shared::new()),
    );
    handle.deliver((0..1_025).map(|_| message()).collect());
    assert!(handle.failure_count() >= 1);
}

proptest! {
    // REQ: HST-202
    #[test]
    fn hst_202_queue_never_exceeds_its_capacity(
        capacity in 1usize..8,
        pushes in proptest::collection::vec(0u8..4, 0..40),
    ) {
        let scheduler = Scheduler::new(10);
        let who = plugin("demo.queue");
        scheduler.register_side(&who, capacity);
        for kind in pushes {
            let event = match kind { 0 => theme(), 1 => key_pressed(), 2 => pointer(), _ => command_invoked() };
            scheduler.push_event(&who, event);
            prop_assert!(scheduler.queue_len(&who) <= capacity);
        }
    }
}

// ---- HST-203 ------------------------------------------------------------------------------

fn events(count: usize) -> Vec<Event> {
    (0..count).map(|_| theme()).collect()
}

/// REQ: HST-203
#[test]
fn hst_203_batch_of_255_events_is_not_ready() {
    let mut builder = BatchBuilder::new();
    for event in events(BATCH_MAX_EVENTS - 1) {
        builder.push(event, 0, false);
    }
    assert!(builder.take_ready(0).is_none());
}

/// REQ: HST-203
#[test]
fn hst_203_batch_of_256_events_flushes() {
    let mut builder = BatchBuilder::new();
    for event in events(BATCH_MAX_EVENTS) {
        builder.push(event, 0, false);
    }
    assert_eq!(builder.take_ready(0).map(|batch| batch.len()), Some(256));
}

/// REQ: HST-203
#[test]
fn hst_203_257_events_make_a_batch_of_256_and_one_left() {
    let mut builder = BatchBuilder::new();
    for event in events(BATCH_MAX_EVENTS + 1) {
        builder.push(event, 0, false);
    }
    assert_eq!(builder.take_ready(0).map(|batch| batch.len()), Some(256));
    assert!(builder.take_ready(0).is_none());
    assert_eq!(
        builder.take_ready(BATCH_FLUSH_MS).map(|batch| batch.len()),
        Some(1)
    );
}

/// REQ: HST-203
#[test]
fn hst_203_batch_waits_up_to_2_ms_for_more_events() {
    let mut builder = BatchBuilder::new();
    builder.push(theme(), 0, false);
    assert!(builder.take_ready(BATCH_FLUSH_MS - 1).is_none());
    assert!(builder.take_ready(BATCH_FLUSH_MS).is_some());
}

/// REQ: HST-203
#[test]
fn hst_203_events_inside_the_window_join_the_same_batch() {
    let mut builder = BatchBuilder::new();
    builder.push(theme(), 0, false);
    builder.push(keymap(), 1, false);
    assert_eq!(
        builder.take_ready(BATCH_FLUSH_MS).map(|batch| batch.len()),
        Some(2)
    );
}

/// REQ: HST-203
#[test]
fn hst_203_user_action_flushes_at_once() {
    let mut builder = BatchBuilder::new();
    builder.push(key_pressed(), 5, true);
    assert_eq!(builder.take_ready(5).map(|batch| batch.len()), Some(1));
}

/// REQ: HST-203
#[test]
fn hst_203_latency_critical_kinds_flush_without_waiting() {
    for event in [key_pressed(), command_invoked(), ui_action()] {
        let mut builder = BatchBuilder::new();
        builder.push(event, 0, true);
        assert!(builder.take_ready(0).is_some());
    }
}

proptest! {
    // REQ: HST-203
    #[test]
    fn hst_203_batches_never_exceed_256_and_keep_order(count in 0usize..700) {
        let mut builder = BatchBuilder::new();
        for index in 0..count {
            builder.push(tree_changed(&[index as u64]), 0, false);
        }
        let mut seen = Vec::new();
        let mut now = 0;
        while let Some(batch) = builder.take_ready(now) {
            prop_assert!(batch.len() <= 256);
            seen.extend(batch);
            now += BATCH_FLUSH_MS;
        }
        if let Some(rest) = builder.take_ready(now + BATCH_FLUSH_MS) { seen.extend(rest); }
        prop_assert_eq!(seen.len(), count);
        for (index, event) in seen.iter().enumerate() {
            prop_assert_eq!(event, &tree_changed(&[index as u64]));
        }
    }
}

// ---- HST-204 ------------------------------------------------------------------------------

/// REQ: HST-204
#[test]
fn hst_204_credit_window_is_256_kib_and_chunks_are_64_kib() {
    assert_eq!(CREDIT_WINDOW_BYTES, 256 * 1024);
    assert_eq!(MAX_CHUNK_BYTES, 64 * 1024);
}

/// REQ: HST-204
#[test]
fn hst_204_reading_continues_below_the_window() {
    let mut credit = StreamCredit::new();
    credit.enqueued(CREDIT_WINDOW_BYTES - 1);
    assert!(credit.should_read());
}

/// REQ: HST-204
#[test]
fn hst_204_reading_pauses_at_the_full_window() {
    let mut credit = StreamCredit::new();
    credit.enqueued(CREDIT_WINDOW_BYTES);
    assert!(!credit.should_read());
}

/// REQ: HST-204
#[test]
fn hst_204_reading_resumes_when_half_the_window_is_free() {
    let mut credit = StreamCredit::new();
    credit.enqueued(CREDIT_WINDOW_BYTES);
    credit.consumed(CREDIT_WINDOW_BYTES / 2 - 1);
    assert!(!credit.should_read());
    credit.consumed(1);
    assert!(credit.should_read());
}

/// REQ: HST-204
#[test]
fn hst_204_in_flight_bytes_never_exceed_the_window() {
    let mut credit = StreamCredit::new();
    credit.enqueued(CREDIT_WINDOW_BYTES);
    assert!(credit.in_flight() <= CREDIT_WINDOW_BYTES);
}

/// REQ: HST-204
#[test]
fn hst_204_a_large_payload_splits_into_chunks_of_at_most_64_kib() {
    let sizes = StreamCredit::split_chunks(MAX_CHUNK_BYTES * 2 + 1);
    assert_eq!(sizes, vec![MAX_CHUNK_BYTES, MAX_CHUNK_BYTES, 1]);
}

proptest! {
    // REQ: HST-204
    #[test]
    fn hst_204_chunks_sum_to_the_payload_and_stay_bounded(total in 0usize..1_000_000) {
        let sizes = StreamCredit::split_chunks(total);
        prop_assert_eq!(sizes.iter().sum::<usize>(), total);
        prop_assert!(sizes.iter().all(|size| *size <= MAX_CHUNK_BYTES && *size > 0));
    }
}

// ---- HST-205 ------------------------------------------------------------------------------

fn rate_limited_scheduler() -> (Scheduler, PluginId) {
    let scheduler = Scheduler::new(10);
    let who = plugin("demo.source");
    scheduler.register_side(&who, 8);
    scheduler.set_source_rate(&who, 2);
    (scheduler, who)
}

/// REQ: HST-205
#[test]
fn hst_205_source_within_its_rate_is_queued() {
    let (scheduler, who) = rate_limited_scheduler();
    assert_eq!(
        scheduler.push_event_from_source(&who, http_called(1)),
        Ok(EventPush::Queued)
    );
    assert_eq!(
        scheduler.push_event_from_source(&who, http_called(2)),
        Ok(EventPush::Queued)
    );
}

/// REQ: HST-205
#[test]
fn hst_205_source_over_its_rate_is_refused_with_rate_limited() {
    let (scheduler, who) = rate_limited_scheduler();
    scheduler
        .push_event_from_source(&who, http_called(1))
        .expect("first");
    scheduler
        .push_event_from_source(&who, http_called(2))
        .expect("second");
    let refused = scheduler.push_event_from_source(&who, http_called(3));
    assert!(matches!(refused, Err(PluginError { code, .. }) if code.as_str() == "rate-limited"));
}

/// REQ: HST-205
#[test]
fn hst_205_refused_event_never_reaches_the_queue() {
    let (scheduler, who) = rate_limited_scheduler();
    for call in 1..=3 {
        let _ = scheduler.push_event_from_source(&who, http_called(call));
    }
    assert_eq!(scheduler.queue_len(&who), 2);
}

// ---- HST-206 / HST-207 / HST-208 ----------------------------------------------------------

/// REQ: HST-206
#[test]
fn hst_206_bucket_holds_2000_ms_and_refills_500_ms_per_second() {
    let mut bucket = ExecutionBucket::new();
    assert_eq!(bucket.balance_ms(), 2_000);
    bucket.charge_cpu_ms(1_000);
    bucket.refill(Duration::from_secs(1));
    assert_eq!(bucket.balance_ms(), 1_500);
}

/// REQ: HST-206
#[test]
fn hst_206_refill_never_exceeds_the_capacity() {
    let mut bucket = ExecutionBucket::new();
    bucket.charge_cpu_ms(100);
    bucket.refill(Duration::from_secs(60));
    assert_eq!(bucket.balance_ms(), 2_000);
}

/// REQ: HST-206
#[test]
fn hst_206_payload_costs_1_ms_per_64_kib() {
    let mut bucket = ExecutionBucket::new();
    bucket.charge_payload_bytes(64 * 1024);
    assert_eq!(bucket.balance_ms(), 1_999);
    bucket.charge_payload_bytes(64 * 1024 * 10);
    assert_eq!(bucket.balance_ms(), 1_989);
}

/// REQ: HST-206
#[test]
fn hst_206_submission_is_accepted_with_a_non_negative_balance() {
    let mut bucket = ExecutionBucket::new();
    bucket.charge_cpu_ms(2_000);
    assert!(bucket.try_submit().is_ok());
}

/// REQ: HST-206
#[test]
fn hst_206_submission_with_a_negative_balance_is_refused_with_quota_exceeded() {
    let mut bucket = ExecutionBucket::new();
    bucket.charge_cpu_ms(2_001);
    let refused = bucket.try_submit().expect_err("refused");
    assert_eq!(refused.code.as_str(), "quota-exceeded");
}

proptest! {
    // REQ: HST-206
    #[test]
    fn hst_206_balance_stays_between_the_debt_and_the_capacity(
        steps in proptest::collection::vec((0u64..3_000, 0u64..5_000), 0..20)
    ) {
        let mut bucket = ExecutionBucket::new();
        for (cost, wait_ms) in steps {
            bucket.charge_cpu_ms(cost);
            bucket.refill(Duration::from_millis(wait_ms));
            prop_assert!(bucket.balance_ms() <= 2_000);
        }
    }
}

/// REQ: HST-207
#[test]
fn hst_207_retry_after_for_a_debt_of_100_ms_is_200() {
    assert_eq!(retry_after_ms(-100), 200);
}

/// REQ: HST-207
#[test]
fn hst_207_retry_after_boundaries_round_up_by_the_refill_rate() {
    assert_eq!(retry_after_ms(-50), 100);
    assert_eq!(retry_after_ms(-51), 110);
    assert_eq!(retry_after_ms(-1), 10);
}

/// REQ: HST-207
#[test]
fn hst_207_refused_submission_carries_the_retry_after_hint() {
    let mut bucket = ExecutionBucket::new();
    bucket.charge_cpu_ms(2_100);
    let refused = bucket.try_submit().expect_err("refused");
    assert!(refused.message.contains("200"));
}

proptest! {
    // REQ: HST-207
    #[test]
    fn hst_207_retry_after_is_enough_to_repay_the_debt(debt in 1i64..100_000) {
        let wait = retry_after_ms(-debt);
        prop_assert!(i64::from(wait) * 500 / 1_000 >= debt);
    }
}

/// REQ: HST-208
#[test]
fn hst_208_empty_bucket_does_not_hold_back_event_delivery() {
    let scheduler = Scheduler::new(10);
    let who = plugin("demo.broke");
    scheduler.set_bucket_balance_ms(&who, -5_000);
    let base = Instant::now();
    scheduler.enqueue(&who, CallKind::OnEvents, base, noop());
    scheduler.enqueue(&who, CallKind::Render, base, noop());
    scheduler.enqueue(&who, CallKind::Activate, base, noop());
    scheduler.enqueue(&who, CallKind::Deactivate, base, noop());
    drain(&scheduler, base, 4);
    assert_eq!(executed_count(&scheduler, &who), 4);
}

/// REQ: HST-208
#[test]
fn hst_208_empty_bucket_does_not_hold_back_provider_calls() {
    let scheduler = Scheduler::new(10);
    let who = plugin("demo.broke");
    scheduler.set_bucket_balance_ms(&who, -5_000);
    let base = Instant::now();
    scheduler.enqueue(&who, CallKind::MenuItems, base, noop());
    scheduler.enqueue(&who, CallKind::Tokens, base, noop());
    drain(&scheduler, base, 4);
    assert_eq!(executed_count(&scheduler, &who), 2);
}

/// REQ: HST-208
#[test]
fn hst_208_empty_bucket_still_refuses_new_submissions() {
    let mut bucket = ExecutionBucket::new();
    bucket.charge_cpu_ms(5_000);
    assert!(bucket.try_submit().is_err());
}

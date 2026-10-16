//! Latency budgets and queue-timeout accounting (HST-214..216).

use ilium_plugin_host::scheduler::BatchBuilder;

use crate::common::*;

/// REQ: HST-214
#[test]
fn hst_214_grabbed_key_flushes_its_batch_at_once() {
    let mut builder = BatchBuilder::new();
    builder.push(key_pressed(), 100, true);
    assert_eq!(builder.take_ready(100).map(|batch| batch.len()), Some(1));
}

/// REQ: HST-214
#[test]
fn hst_214_grabbed_key_carries_earlier_queued_events_with_it() {
    let mut builder = BatchBuilder::new();
    builder.push(theme(), 99, false);
    builder.push(key_pressed(), 100, true);
    assert_eq!(builder.take_ready(100).map(|batch| batch.len()), Some(2));
}

/// REQ: HST-214
#[test]
#[ignore = "budget: run on the reference machine"]
fn hst_214_key_to_call_start_p99_is_within_budget_for_every_tier() {
    for (budget_us, label) in [(1_000u128, "t1"), (1_500, "t2"), (1_500, "t3")] {
        let shared = Shared::new();
        let handle = build(ScriptedGuest::new(shared.clone()));
        let mut samples = Vec::new();
        for _ in 0..2_000 {
            let sent = std::time::Instant::now();
            handle.deliver(vec![key_pressed()]);
            handle.pump(1);
            samples.push(sent.elapsed().as_micros());
        }
        samples.sort_unstable();
        let p99 = samples[samples.len() * 99 / 100];
        assert!(p99 <= budget_us, "{label} p99 {p99} us");
    }
}

/// REQ: HST-215
#[test]
#[ignore = "budget: run on the reference machine"]
fn hst_215_key_to_pty_bytes_p99_is_at_most_5_ms() {
    let kernel = kernel_with_nodes(1);
    let guest = ScriptedGuest::new(Shared::new()).submitting(vec![play_sound_submission(1)]);
    let handle = build_with(HostHarness::new().kernel(kernel.clone()), guest.sleeping(0));
    let mut samples = Vec::new();
    for round in 0..2_000usize {
        let sent = std::time::Instant::now();
        handle.deliver(vec![key_pressed()]);
        handle.pump(1);
        while kernel.applied_intents().len() <= round {
            if sent.elapsed() > Duration::from_millis(50) {
                break;
            }
        }
        samples.push(sent.elapsed().as_micros());
    }
    samples.sort_unstable();
    assert!(samples[samples.len() * 99 / 100] <= 5_000);
}

/// REQ: HST-216
#[test]
fn hst_216_call_abandoned_in_the_queue_logs_host_queue_timeout() {
    let scheduler = Scheduler::new(10);
    let who = plugin("demo.waiting");
    let base = Instant::now();
    scheduler.set_store_busy(&who, true);
    scheduler.enqueue(&who, CallKind::MenuItems, base, noop());
    drain(&scheduler, base, 4);
    assert!(scheduler.log_lines().iter().any(|line| line.contains("host.queue-timeout")));
}

/// REQ: HST-216
#[test]
fn hst_216_abandoned_call_counts_a_breach_of_its_class() {
    let scheduler = Scheduler::new(10);
    let who = plugin("demo.waiting");
    let base = Instant::now();
    scheduler.set_store_busy(&who, true);
    scheduler.enqueue(&who, CallKind::MenuItems, base, noop());
    drain(&scheduler, base, 4);
    assert_eq!(scheduler.breach_count(&who, CallClass::MenuItems), 1);
    assert_eq!(scheduler.abandoned().len(), 1);
}

/// REQ: HST-216
#[test]
fn hst_216_call_that_started_in_time_logs_nothing_and_counts_nothing() {
    let scheduler = Scheduler::new(10);
    let who = plugin("demo.prompt");
    let base = Instant::now();
    scheduler.enqueue(&who, CallKind::MenuItems, base, noop());
    drain(&scheduler, base, 4);
    assert!(scheduler.log_lines().iter().all(|line| !line.contains("host.queue-timeout")));
    assert_eq!(scheduler.breach_count(&who, CallClass::MenuItems), 0);
}

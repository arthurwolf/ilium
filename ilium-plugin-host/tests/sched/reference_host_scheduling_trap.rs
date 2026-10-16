//! Activation, yielding and trap aftermath (HST-197..201).

use std::sync::atomic::{AtomicUsize, Ordering};

use ilium_plugin_api::generated::types::requests::Outcome;
use ilium_plugin_api::ids::NodeId;

use crate::common::*;

// ---- HST-197 ------------------------------------------------------------------------------

/// REQ: HST-197
#[test]
fn hst_197_calls_are_queued_while_the_side_is_activating() {
    let scheduler = Scheduler::new(10);
    let who = plugin("demo.activating");
    let base = Instant::now();
    scheduler.begin_activation(&who);
    scheduler.enqueue(&who, CallKind::OnEvents, base, noop());
    scheduler.enqueue(&who, CallKind::Render, base, noop());
    scheduler.tick(base);
    assert_eq!(executed_count(&scheduler, &who), 0);
}

/// REQ: HST-197
#[test]
fn hst_197_queued_calls_run_after_activate_returned_successfully() {
    let scheduler = Scheduler::new(10);
    let who = plugin("demo.activating");
    let base = Instant::now();
    scheduler.begin_activation(&who);
    scheduler.enqueue(&who, CallKind::OnEvents, base, noop());
    scheduler.enqueue(&who, CallKind::Render, base, noop());
    scheduler.tick(base);
    scheduler.finish_activation(&who, true);
    scheduler.tick(at(base, 10));
    assert_eq!(kinds_run(&scheduler), vec![CallKind::OnEvents, CallKind::Render]);
}

/// REQ: HST-197
#[test]
fn hst_197_no_call_reaches_the_instance_across_many_ticks_of_activation() {
    let scheduler = Scheduler::new(10);
    let who = plugin("demo.activating");
    let base = Instant::now();
    scheduler.begin_activation(&who);
    for _ in 0..3 {
        scheduler.enqueue(&who, CallKind::OnEvents, base, noop());
    }
    drain(&scheduler, base, 8);
    assert_eq!(executed_count(&scheduler, &who), 0);
}

/// REQ: HST-197
#[test]
fn hst_197_failed_activation_never_lets_queued_calls_into_the_instance() {
    let scheduler = Scheduler::new(10);
    let who = plugin("demo.activating");
    let base = Instant::now();
    scheduler.begin_activation(&who);
    scheduler.enqueue(&who, CallKind::OnEvents, base, noop());
    scheduler.finish_activation(&who, false);
    drain(&scheduler, base, 4);
    assert_eq!(executed_count(&scheduler, &who), 0);
}

/// REQ: HST-197
#[test]
fn hst_197_other_plugins_keep_running_while_one_activates() {
    let scheduler = Scheduler::new(10);
    let activating = plugin("demo.activating");
    let other = plugin("demo.other");
    let base = Instant::now();
    scheduler.begin_activation(&activating);
    scheduler.enqueue(&activating, CallKind::OnEvents, base, noop());
    scheduler.enqueue(&other, CallKind::OnEvents, base, noop());
    scheduler.tick(base);
    assert_eq!(executed_count(&scheduler, &other), 1);
    assert_eq!(executed_count(&scheduler, &activating), 0);
}

/// REQ: HST-197
#[test]
fn hst_197_activate_is_the_first_call_the_guest_sees() {
    let shared = Shared::new();
    let handle = build(ScriptedGuest::new(shared.clone()));
    handle.deliver(vec![theme()]);
    handle.pump(8);
    let phases: Vec<String> = shared.threads().into_iter().map(|entry| entry.0).collect();
    assert_eq!(phases.first().map(String::as_str), Some("activate"));
    assert!(phases.iter().any(|phase| phase == "on-events"));
}

// ---- HST-198 ------------------------------------------------------------------------------

/// REQ: HST-198
#[test]
fn hst_198_successful_call_applies_its_submissions() {
    let kernel = kernel_with_nodes(1);
    let guest = ScriptedGuest::new(Shared::new()).submitting(vec![play_sound_submission(1)]);
    let handle = build_with(HostHarness::new().kernel(kernel.clone()), guest);
    handle.deliver(vec![theme()]);
    handle.pump(8);
    assert_eq!(kernel.applied_intents().len(), 1);
}

/// REQ: HST-198
#[test]
fn hst_198_trapping_call_applies_none_of_its_submissions() {
    let kernel = kernel_with_nodes(1);
    let guest = ScriptedGuest::new(Shared::new())
        .submitting(vec![play_sound_submission(1), play_sound_submission(2)])
        .trapping(Box::new(|_, _| true));
    let handle = build_with(HostHarness::new().kernel(kernel.clone()), guest);
    handle.deliver(vec![theme()]);
    handle.pump(8);
    assert!(!handle.failures().is_empty());
    assert!(kernel.applied_intents().is_empty());
}

/// REQ: HST-198
#[test]
fn hst_198_in_flight_batch_of_a_non_surviving_kind_is_not_redelivered() {
    let shared = Shared::new();
    let guest = ScriptedGuest::new(shared.clone()).trapping(Box::new(|call, _| call == 1));
    let mut handle = build(guest);
    handle.deliver(vec![theme()]);
    settle(&mut handle);
    assert_eq!(count_kind(&shared.batches(), &theme()), 1);
}

/// REQ: HST-198
#[test]
fn hst_198_in_flight_key_press_is_not_redelivered_after_a_trap() {
    let shared = Shared::new();
    let guest = ScriptedGuest::new(shared.clone()).trapping(Box::new(|call, _| call == 1));
    let mut handle = build(guest);
    handle.deliver(vec![key_pressed()]);
    settle(&mut handle);
    assert_eq!(count_kind(&shared.batches(), &key_pressed()), 1);
}

/// REQ: HST-198
#[test]
fn hst_198_one_trap_is_recorded_for_one_trapping_call() {
    let guest = ScriptedGuest::new(Shared::new()).trapping(Box::new(|call, _| call == 1));
    let mut handle = build(guest);
    handle.deliver(vec![theme()]);
    settle(&mut handle);
    assert_eq!(handle.failures().len(), 1);
}

// ---- HST-199 ------------------------------------------------------------------------------

fn traps_first_n_times_on(kind: Event, times: usize) -> TrapRule {
    let seen = Arc::new(AtomicUsize::new(0));
    Box::new(move |_, batch| {
        let has_kind = batch
            .iter()
            .any(|event| std::mem::discriminant(event) == std::mem::discriminant(&kind));
        has_kind && seen.fetch_add(1, Ordering::SeqCst) < times
    })
}

fn deliveries_with_traps(kind: fn() -> Event, times: usize) -> (usize, HarnessHandle) {
    let shared = Shared::new();
    let guest = ScriptedGuest::new(shared.clone()).trapping(traps_first_n_times_on(kind(), times));
    let mut handle = build(guest);
    handle.deliver(vec![kind()]);
    settle(&mut handle);
    (count_kind(&shared.batches(), &kind()), handle)
}

fn poison_logs(handle: &HarnessHandle) -> usize {
    handle
        .logs()
        .iter()
        .filter(|line| line.message.contains("events.poison"))
        .count()
}

/// REQ: HST-199
#[test]
fn hst_199_request_completed_is_redelivered_once_after_a_trap() {
    let (deliveries, handle) = deliveries_with_traps(request_completed, 1);
    assert_eq!(deliveries, 2);
    assert_eq!(poison_logs(&handle), 0);
}

/// REQ: HST-199
#[test]
fn hst_199_durable_timer_fired_is_redelivered_once_after_a_trap() {
    let (deliveries, handle) = deliveries_with_traps(durable_timer, 1);
    assert_eq!(deliveries, 2);
    assert_eq!(poison_logs(&handle), 0);
}

/// REQ: HST-199
#[test]
fn hst_199_input_uncertain_is_redelivered_once_after_a_trap() {
    let (deliveries, handle) = deliveries_with_traps(input_uncertain, 1);
    assert_eq!(deliveries, 2);
    assert_eq!(poison_logs(&handle), 0);
}

/// REQ: HST-199
#[test]
fn hst_199_message_is_redelivered_once_after_a_trap() {
    let (deliveries, handle) = deliveries_with_traps(message, 1);
    assert_eq!(deliveries, 2);
    assert_eq!(poison_logs(&handle), 0);
}

/// REQ: HST-199
#[test]
fn hst_199_request_completed_is_dropped_with_events_poison_when_the_redelivery_traps() {
    let (deliveries, handle) = deliveries_with_traps(request_completed, 2);
    assert_eq!(deliveries, 2);
    assert_eq!(poison_logs(&handle), 1);
}

/// REQ: HST-199
#[test]
fn hst_199_message_is_dropped_with_events_poison_when_the_redelivery_traps() {
    let (deliveries, handle) = deliveries_with_traps(message, 2);
    assert_eq!(deliveries, 2);
    assert_eq!(poison_logs(&handle), 1);
}

/// REQ: HST-199
#[test]
fn hst_199_durable_timer_fired_is_dropped_with_events_poison_when_the_redelivery_traps() {
    let (deliveries, handle) = deliveries_with_traps(durable_timer, 2);
    assert_eq!(deliveries, 2);
    assert_eq!(poison_logs(&handle), 1);
}

/// REQ: HST-199
#[test]
fn hst_199_input_uncertain_is_dropped_with_events_poison_when_the_redelivery_traps() {
    let (deliveries, handle) = deliveries_with_traps(input_uncertain, 2);
    assert_eq!(deliveries, 2);
    assert_eq!(poison_logs(&handle), 1);
}

/// REQ: HST-199
#[test]
fn hst_199_poison_log_names_the_event_kind() {
    let (_, handle) = deliveries_with_traps(message, 2);
    let line = handle
        .logs()
        .into_iter()
        .find(|line| line.message.contains("events.poison"))
        .expect("poison line");
    assert!(line.message.contains("message"));
}

/// REQ: HST-199
#[test]
fn hst_199_redelivered_event_sits_at_the_head_of_the_new_queue() {
    let shared = Shared::new();
    let guest = ScriptedGuest::new(shared.clone()).trapping(traps_first_n_times_on(message(), 1));
    let mut handle = build(guest);
    handle.deliver(vec![message()]);
    handle.pump(1);
    handle.deliver(vec![theme()]);
    settle(&mut handle);
    let batches = shared.batches();
    let message_again = batches
        .iter()
        .enumerate()
        .filter(|(_, batch)| contains_kind(&[(*batch).clone()], &message()))
        .map(|(index, _)| index)
        .nth(1)
        .expect("message redelivered");
    let first_theme = batches
        .iter()
        .position(|batch| contains_kind(&[batch.clone()], &theme()))
        .expect("theme delivered");
    assert!(message_again <= first_theme);
}

// ---- HST-200 ------------------------------------------------------------------------------

/// Floods the queue until the side is stuck and returns the batches the guest saw afterwards.
fn stuck_restart(extra_events: Vec<Event>) -> (Arc<Shared>, HarnessHandle, Arc<ilium_plugin_host::testing::FakeKernel>) {
    let kernel = kernel_with_nodes(3);
    let shared = Shared::new();
    let guest = ScriptedGuest::new(shared.clone());
    let mut handle = build_with(HostHarness::new().kernel(kernel.clone()), guest);
    handle.deliver(extra_events);
    handle.deliver((0..1_025).map(|_| key_pressed()).collect());
    settle(&mut handle);
    (shared, handle, kernel)
}

/// REQ: HST-200
#[test]
fn hst_200_stuck_restart_discards_the_queued_user_input_events() {
    let (shared, _handle, _kernel) = stuck_restart(vec![command_invoked(), ui_action()]);
    let batches = shared.batches();
    assert_eq!(count_kind(&batches, &key_pressed()), 0);
    assert_eq!(count_kind(&batches, &command_invoked()), 0);
    assert_eq!(count_kind(&batches, &ui_action()), 0);
}

/// REQ: HST-200
#[test]
fn hst_200_stuck_restart_keeps_the_surviving_kinds() {
    let (shared, _handle, _kernel) =
        stuck_restart(vec![message(), durable_timer(), input_uncertain(), request_completed()]);
    let batches = shared.batches();
    assert_eq!(count_kind(&batches, &message()), 1);
    assert_eq!(count_kind(&batches, &durable_timer()), 1);
    assert_eq!(count_kind(&batches, &input_uncertain()), 1);
    assert_eq!(count_kind(&batches, &request_completed()), 1);
}

/// REQ: HST-200
#[test]
fn hst_200_call_behind_cli_invoked_completes_cancelled() {
    let (_shared, handle, _kernel) = stuck_restart(vec![cli_invoked(7)]);
    assert!(handle.call_outcomes().contains(&(7, Outcome::Cancelled)));
}

/// REQ: HST-200
#[test]
fn hst_200_call_behind_http_called_completes_cancelled() {
    let (_shared, handle, _kernel) = stuck_restart(vec![http_called(8)]);
    assert!(handle.call_outcomes().contains(&(8, Outcome::Cancelled)));
}

/// REQ: HST-200
#[test]
fn hst_200_call_behind_client_request_completes_cancelled() {
    let (_shared, handle, _kernel) = stuck_restart(vec![client_request(9)]);
    assert!(handle.call_outcomes().contains(&(9, Outcome::Cancelled)));
}

/// REQ: HST-200
#[test]
fn hst_200_new_instance_receives_one_tree_changed_batch_for_every_node() {
    let (shared, _handle, _kernel) = stuck_restart(Vec::new());
    let batches = shared.batches();
    let trees: Vec<&Vec<NodeId>> = batches
        .iter()
        .flatten()
        .filter_map(|event| match event {
            Event::TreeChanged(ids) => Some(ids),
            _ => None,
        })
        .collect();
    assert_eq!(trees.len(), 1);
    let mut ids = trees[0].clone();
    ids.sort_by_key(|id| id.0);
    assert_eq!(ids, vec![NodeId(1), NodeId(2), NodeId(3)]);
}

// ---- HST-201 ------------------------------------------------------------------------------

/// REQ: HST-201
#[test]
fn hst_201_retained_contributions_stay_after_a_trap() {
    let guest = ScriptedGuest::new(Shared::new()).trapping(Box::new(|call, _| call == 1));
    let mut handle = build(guest);
    handle.seed_retained("decoration:1:badge");
    handle.deliver(vec![theme()]);
    settle(&mut handle);
    let kept = handle.retained_contributions();
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].key, "decoration:1:badge");
}

/// REQ: HST-201
#[test]
fn hst_201_retained_contributions_of_a_trapped_instance_are_marked_stale() {
    let guest = ScriptedGuest::new(Shared::new()).trapping(Box::new(|call, _| call == 1));
    let mut handle = build(guest);
    handle.seed_retained("decoration:1:badge");
    handle.deliver(vec![theme()]);
    settle(&mut handle);
    assert!(handle.retained_contributions().iter().all(|entry| entry.stale));
}

/// REQ: HST-201
#[test]
fn hst_201_replacing_a_contribution_clears_its_stale_mark() {
    let guest = ScriptedGuest::new(Shared::new()).trapping(Box::new(|call, _| call == 1));
    let mut handle = build(guest);
    handle.seed_retained("decoration:1:badge");
    handle.seed_retained("decoration:1:other");
    handle.deliver(vec![theme()]);
    settle(&mut handle);
    handle.replace_retained("decoration:1:badge");
    let entries = handle.retained_contributions();
    let badge = entries.iter().find(|entry| entry.key == "decoration:1:badge").expect("badge");
    let other = entries.iter().find(|entry| entry.key == "decoration:1:other").expect("other");
    assert!(!badge.stale);
    assert!(other.stale);
}

/// REQ: HST-201
#[test]
fn hst_201_contributions_without_a_trap_are_not_stale() {
    let handle = build(ScriptedGuest::new(Shared::new()));
    handle.seed_retained("decoration:1:badge");
    handle.deliver(vec![theme()]);
    handle.pump(8);
    assert!(handle.retained_contributions().iter().all(|entry| !entry.stale));
}

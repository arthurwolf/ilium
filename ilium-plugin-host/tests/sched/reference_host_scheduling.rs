//! Thread model, per-instance serialization and fairness (HST-180..187).

use proptest::prelude::*;

use ilium_plugin_api::ProcessKind;
use ilium_plugin_host::scheduler::{
    executor_threads, job_bank_threads, AbandonedCall, UiWorkChannel, EPOCH_THREAD_NAME,
    EXECUTOR_THREAD_PREFIX, JOB_THREAD_PREFIX, SCENE_THREAD_NAME, SIDE_CALLS_PER_TICK, TICK_MS,
};

use crate::common::*;

// ---- HST-180 ------------------------------------------------------------------------------

/// REQ: HST-180
#[test]
fn hst_180_guest_calls_run_on_plugin_executor_threads() {
    let shared = Shared::new();
    let handle = build(ScriptedGuest::new(shared.clone()));
    handle.deliver(vec![theme()]);
    handle.pump(8);
    let seen = shared.threads();
    assert!(seen.iter().any(|(phase, _, _)| phase == "activate"));
    assert!(seen.iter().any(|(phase, _, _)| phase == "on-events"));
    for (phase, name, _) in seen {
        assert!(
            name.starts_with(EXECUTOR_THREAD_PREFIX),
            "{phase} ran on thread {name:?}"
        );
    }
}

/// REQ: HST-180
#[test]
fn hst_180_guest_never_runs_on_the_epoch_ticker_thread() {
    let shared = Shared::new();
    let handle = build(ScriptedGuest::new(shared.clone()));
    handle.deliver(vec![theme(), keymap()]);
    handle.pump(8);
    for (_, name, _) in shared.threads() {
        assert_ne!(name, EPOCH_THREAD_NAME);
    }
}

/// REQ: HST-180
#[test]
fn hst_180_guest_never_runs_on_the_calling_thread() {
    let shared = Shared::new();
    let handle = build(ScriptedGuest::new(shared.clone()));
    handle.deliver(vec![theme()]);
    handle.pump(8);
    let caller = std::thread::current().id();
    let seen = shared.threads();
    assert!(!seen.is_empty());
    for (_, _, id) in seen {
        assert_ne!(id, caller);
    }
}

/// REQ: HST-180
#[test]
fn hst_180_guest_never_runs_on_the_server_main_runtime_thread() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("runtime");
    let shared = Shared::new();
    let main_runtime_thread = runtime.block_on(async {
        let handle = build(ScriptedGuest::new(shared.clone()));
        handle.deliver(vec![theme()]);
        handle.pump(8);
        std::thread::current().id()
    });
    let seen = shared.threads();
    assert!(!seen.is_empty());
    for (_, _, id) in seen {
        assert_ne!(id, main_runtime_thread);
    }
}

// ---- HST-181 ------------------------------------------------------------------------------

/// REQ: HST-181
#[test]
fn hst_181_deliver_returns_before_a_slow_plugin_runs() {
    let shared = Shared::new();
    let handle = build(ScriptedGuest::new(shared.clone()).sleeping(80));
    let before = Instant::now();
    handle.deliver(vec![theme()]);
    let elapsed = before.elapsed();
    assert!(
        elapsed < ms(10),
        "deliver blocked the caller for {elapsed:?}"
    );
    handle.pump(8);
    assert!(!shared.batches().is_empty());
}

/// REQ: HST-181
#[test]
fn hst_181_frame_loop_is_not_delayed_by_a_slow_plugin() {
    let shared = Shared::new();
    let handle = build(ScriptedGuest::new(shared.clone()).sleeping(50));
    let mut slowest = Duration::ZERO;
    for _frame in 0..20 {
        let before = Instant::now();
        handle.deliver(vec![theme()]);
        slowest = slowest.max(before.elapsed());
    }
    assert!(slowest < ms(10), "slowest frame work {slowest:?}");
    handle.pump(64);
}

/// REQ: HST-181
#[test]
fn hst_181_ui_work_channel_is_bounded() {
    let channel: UiWorkChannel<u32> = UiWorkChannel::new(2);
    assert!(channel.try_send(1).is_ok());
    assert!(channel.try_send(2).is_ok());
    assert_eq!(channel.try_send(3), Err(3));
}

/// REQ: HST-181
#[test]
fn hst_181_ui_work_channel_accepts_again_after_the_executor_takes_work() {
    let channel: UiWorkChannel<u32> = UiWorkChannel::new(1);
    assert!(channel.try_send(1).is_ok());
    assert_eq!(channel.try_send(2), Err(2));
    assert_eq!(channel.try_recv(), Some(1));
    assert!(channel.try_send(2).is_ok());
    assert_eq!(channel.try_recv(), Some(2));
    assert_eq!(channel.try_recv(), None);
}

// ---- HST-182 ------------------------------------------------------------------------------

/// REQ: HST-182
#[test]
fn hst_182_server_executor_has_clamp_cores_over_2_between_2_and_4_threads() {
    let expected = [(1, 2), (2, 2), (4, 2), (8, 4), (64, 4)];
    for (cores, threads) in expected {
        assert_eq!(
            executor_threads(ProcessKind::Server, cores),
            threads,
            "cores {cores}"
        );
    }
}

/// REQ: HST-182
#[test]
fn hst_182_server_executor_boundaries_of_the_clamp() {
    let expected = [(3, 2), (5, 2), (6, 3), (7, 3), (8, 4), (9, 4)];
    for (cores, threads) in expected {
        assert_eq!(
            executor_threads(ProcessKind::Server, cores),
            threads,
            "cores {cores}"
        );
    }
}

/// REQ: HST-182
#[test]
fn hst_182_client_executor_has_clamp_cores_over_4_between_1_and_2_threads() {
    let expected = [(1, 1), (2, 1), (4, 1), (8, 2), (64, 2)];
    for (cores, threads) in expected {
        assert_eq!(
            executor_threads(ProcessKind::Client, cores),
            threads,
            "cores {cores}"
        );
    }
}

/// REQ: HST-182
#[test]
fn hst_182_client_executor_boundaries_of_the_clamp() {
    let expected = [(3, 1), (7, 1), (8, 2), (9, 2), (12, 2)];
    for (cores, threads) in expected {
        assert_eq!(
            executor_threads(ProcessKind::Client, cores),
            threads,
            "cores {cores}"
        );
    }
}

/// REQ: HST-182
#[test]
fn hst_182_cli_executor_is_a_one_thread_pool() {
    for cores in [1, 2, 8, 64] {
        assert_eq!(
            executor_threads(ProcessKind::Cli, cores),
            1,
            "cores {cores}"
        );
    }
}

/// REQ: HST-182
#[test]
fn hst_182_job_bank_is_min_4_cores_over_2_and_at_least_one() {
    let expected = [
        (1, 1),
        (2, 1),
        (3, 1),
        (4, 2),
        (7, 3),
        (8, 4),
        (9, 4),
        (64, 4),
    ];
    for (cores, threads) in expected {
        assert_eq!(job_bank_threads(cores), threads, "cores {cores}");
    }
}

proptest! {
    // REQ: HST-182
    #[test]
    fn hst_182_sizing_matches_the_formulas_for_any_core_count(cores in 1usize..4096) {
        prop_assert_eq!(executor_threads(ProcessKind::Server, cores), (cores / 2).clamp(2, 4));
        prop_assert_eq!(executor_threads(ProcessKind::Client, cores), (cores / 4).clamp(1, 2));
        prop_assert_eq!(job_bank_threads(cores), std::cmp::min(4, cores / 2).max(1));
    }
}

// ---- HST-183 ------------------------------------------------------------------------------

fn thread_probe() -> (Arc<Mutex<Vec<std::thread::ThreadId>>>, Work) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = seen.clone();
    let work: Work = Box::new(move || {
        sink.lock()
            .expect("probe lock")
            .push(std::thread::current().id());
    });
    (seen, work)
}

/// REQ: HST-183
#[test]
fn hst_183_scene_frame_runs_on_the_scene_thread() {
    let scheduler = Scheduler::new(10);
    let base = Instant::now();
    scheduler.enqueue(&plugin("demo.scene"), CallKind::SceneFrame, base, noop());
    scheduler.tick(base);
    let executed = scheduler.executed();
    assert_eq!(executed.len(), 1);
    assert_eq!(executed[0].thread_name, SCENE_THREAD_NAME);
}

/// REQ: HST-183
#[test]
fn hst_183_scene_frame_does_not_occupy_an_executor_thread() {
    let scheduler = Scheduler::new(10);
    let base = Instant::now();
    scheduler.enqueue(&plugin("demo.scene"), CallKind::SceneFrame, base, noop());
    scheduler.enqueue(&plugin("demo.server"), CallKind::OnEvents, base, noop());
    scheduler.tick(base);
    for call in scheduler.executed() {
        match call.kind {
            CallKind::SceneFrame => assert_eq!(call.thread_name, SCENE_THREAD_NAME),
            _ => {
                assert!(call.thread_name.starts_with(EXECUTOR_THREAD_PREFIX));
                assert_ne!(call.thread_name, SCENE_THREAD_NAME);
            }
        }
    }
}

/// REQ: HST-183
#[test]
fn hst_183_one_dedicated_scene_thread_serves_every_plugin() {
    let scheduler = Scheduler::new(10);
    let base = Instant::now();
    let (seen, _) = thread_probe();
    for name in ["demo.a", "demo.b", "demo.c"] {
        let sink = seen.clone();
        let work: Work = Box::new(move || {
            sink.lock()
                .expect("probe lock")
                .push(std::thread::current().id());
        });
        scheduler.enqueue(&plugin(name), CallKind::SceneFrame, base, work);
    }
    scheduler.tick(base);
    let ids = seen.lock().expect("probe lock").clone();
    assert_eq!(ids.len(), 3);
    assert!(ids.iter().all(|id| *id == ids[0]));
    assert_ne!(ids[0], std::thread::current().id());
}

/// REQ: HST-183
#[test]
fn hst_183_job_runs_use_the_job_bank_not_the_executor() {
    let scheduler = Scheduler::new(10);
    let base = Instant::now();
    scheduler.enqueue(&plugin("demo.jobs"), CallKind::JobRun, base, noop());
    scheduler.tick(base);
    let executed = scheduler.executed();
    assert_eq!(executed.len(), 1);
    assert!(executed[0].thread_name.starts_with(JOB_THREAD_PREFIX));
}

// ---- HST-184 ------------------------------------------------------------------------------

const PRIORITY_TWO: [CallKind; 12] = [
    CallKind::HookHandler,
    CallKind::Collect,
    CallKind::ServiceProvider,
    CallKind::InferenceProvider,
    CallKind::MenuItems,
    CallKind::QueryCatalog,
    CallKind::ListVirtualChildren,
    CallKind::Tokens,
    CallKind::SettingsValidate,
    CallKind::Detector,
    CallKind::ClientRequestAnswer,
    CallKind::ServiceCallWaitingForActivation,
];

/// REQ: HST-184
#[test]
fn hst_184_priority_table_activate_and_deactivate_first() {
    assert_eq!(CallKind::Activate.priority(), Some(1));
    assert_eq!(CallKind::Deactivate.priority(), Some(1));
}

/// REQ: HST-184
#[test]
fn hst_184_priority_table_calls_with_a_waiting_caller_are_second() {
    for kind in PRIORITY_TWO {
        assert_eq!(kind.priority(), Some(2), "{kind:?}");
    }
}

/// REQ: HST-184
#[test]
fn hst_184_priority_table_events_render_annotate() {
    assert_eq!(CallKind::OnEvents.priority(), Some(3));
    assert_eq!(CallKind::Render.priority(), Some(4));
    assert_eq!(CallKind::Annotate.priority(), Some(5));
}

/// REQ: HST-184
#[test]
fn hst_184_scene_frames_and_job_runs_are_not_in_the_instance_queue() {
    assert_eq!(CallKind::SceneFrame.priority(), None);
    assert_eq!(CallKind::JobRun.priority(), None);
}

/// REQ: HST-184
#[test]
fn hst_184_higher_priority_runs_first_whatever_the_arrival_order() {
    let scheduler = Scheduler::new(10);
    let who = plugin("demo.queue");
    let base = Instant::now();
    scheduler.enqueue(&who, CallKind::Render, base, noop());
    scheduler.enqueue(&who, CallKind::OnEvents, base, noop());
    scheduler.enqueue(&who, CallKind::HookHandler, base, noop());
    scheduler.enqueue(&who, CallKind::Activate, base, noop());
    scheduler.tick(base);
    assert_eq!(
        kinds_run(&scheduler),
        vec![
            CallKind::Activate,
            CallKind::HookHandler,
            CallKind::OnEvents,
            CallKind::Render
        ]
    );
}

/// REQ: HST-184
#[test]
fn hst_184_annotate_runs_after_render_and_events() {
    let scheduler = Scheduler::new(10);
    let who = plugin("demo.queue");
    let base = Instant::now();
    scheduler.enqueue(&who, CallKind::Annotate, base, noop());
    scheduler.enqueue(&who, CallKind::Render, base, noop());
    scheduler.enqueue(&who, CallKind::OnEvents, base, noop());
    scheduler.tick(base);
    assert_eq!(
        kinds_run(&scheduler),
        vec![CallKind::OnEvents, CallKind::Render, CallKind::Annotate]
    );
}

/// REQ: HST-184
#[test]
fn hst_184_equal_priority_runs_in_arrival_order() {
    let scheduler = Scheduler::new(10);
    let who = plugin("demo.queue");
    let base = Instant::now();
    let tickets: Vec<u64> = (0..4)
        .map(|_| scheduler.enqueue(&who, CallKind::OnEvents, base, noop()))
        .collect();
    scheduler.tick(base);
    let run: Vec<u64> = scheduler
        .executed()
        .iter()
        .map(|call| call.ticket)
        .collect();
    assert_eq!(run, tickets);
}

/// REQ: HST-184
#[test]
fn hst_184_priority_two_kinds_keep_arrival_order_among_themselves() {
    for pair in PRIORITY_TWO.windows(2) {
        let scheduler = Scheduler::new(10);
        let who = plugin("demo.queue");
        let base = Instant::now();
        scheduler.enqueue(&who, pair[1], base, noop());
        scheduler.enqueue(&who, pair[0], base, noop());
        scheduler.tick(base);
        assert_eq!(kinds_run(&scheduler), vec![pair[1], pair[0]], "{pair:?}");
    }
}

/// REQ: HST-184
#[test]
fn hst_184_activate_and_deactivate_share_priority_and_keep_arrival_order() {
    let scheduler = Scheduler::new(10);
    let who = plugin("demo.queue");
    let base = Instant::now();
    scheduler.enqueue(&who, CallKind::Deactivate, base, noop());
    scheduler.enqueue(&who, CallKind::Activate, base, noop());
    scheduler.tick(base);
    assert_eq!(
        kinds_run(&scheduler),
        vec![CallKind::Deactivate, CallKind::Activate]
    );
}

/// REQ: HST-184
#[test]
fn hst_184_never_two_calls_at_once_in_one_store() {
    let scheduler = Scheduler::new(10);
    let who = plugin("demo.queue");
    let base = Instant::now();
    for _ in 0..8 {
        scheduler.enqueue(&who, CallKind::OnEvents, base, sleeper(3));
    }
    drain(&scheduler, base, 6);
    assert_eq!(scheduler.executed().len(), 8);
    assert_eq!(scheduler.max_concurrent_calls_in_store(&who), 1);
}

proptest! {
    // REQ: HST-184
    #[test]
    fn hst_184_any_arrival_order_runs_by_priority_then_arrival(
        picks in proptest::collection::vec(0usize..4, 1..=4)
    ) {
        let kinds = [CallKind::Activate, CallKind::HookHandler, CallKind::OnEvents, CallKind::Render];
        let scheduler = Scheduler::new(10);
        let who = plugin("demo.queue");
        let base = Instant::now();
        let mut expected: Vec<(u8, u64, CallKind)> = Vec::new();
        for index in picks {
            let kind = kinds[index];
            let ticket = scheduler.enqueue(&who, kind, base, noop());
            expected.push((kind.priority().unwrap_or(u8::MAX), ticket, kind));
        }
        scheduler.tick(base);
        expected.sort();
        let wanted: Vec<CallKind> = expected.iter().map(|entry| entry.2).collect();
        prop_assert_eq!(kinds_run(&scheduler), wanted);
    }
}

// ---- HST-185 ------------------------------------------------------------------------------

/// REQ: HST-185
#[test]
fn hst_185_constants_are_4_calls_per_10_ms_tick() {
    assert_eq!(SIDE_CALLS_PER_TICK, 4);
    assert_eq!(TICK_MS, 10);
}

/// REQ: HST-185
#[test]
fn hst_185_a_side_gets_at_most_four_calls_in_one_tick() {
    let scheduler = Scheduler::new(10);
    let busy = plugin("demo.busy");
    let base = Instant::now();
    for _ in 0..10 {
        scheduler.enqueue(&busy, CallKind::OnEvents, base, noop());
    }
    scheduler.tick(base);
    assert_eq!(executed_count(&scheduler, &busy), 4);
}

/// REQ: HST-185
#[test]
fn hst_185_a_side_with_three_calls_runs_them_all_in_one_tick() {
    let scheduler = Scheduler::new(10);
    let side = plugin("demo.small");
    let base = Instant::now();
    for _ in 0..3 {
        scheduler.enqueue(&side, CallKind::OnEvents, base, noop());
    }
    scheduler.tick(base);
    assert_eq!(executed_count(&scheduler, &side), 3);
}

/// REQ: HST-185
#[test]
fn hst_185_a_side_with_ten_calls_needs_three_ticks() {
    let scheduler = Scheduler::new(10);
    let busy = plugin("demo.busy");
    let base = Instant::now();
    for _ in 0..10 {
        scheduler.enqueue(&busy, CallKind::OnEvents, base, noop());
    }
    scheduler.tick(at(base, 10));
    assert_eq!(executed_count(&scheduler, &busy), 4);
    scheduler.tick(at(base, 20));
    assert_eq!(executed_count(&scheduler, &busy), 8);
    scheduler.tick(at(base, 30));
    assert_eq!(executed_count(&scheduler, &busy), 10);
}

/// REQ: HST-185
#[test]
fn hst_185_two_busy_sides_each_get_four_calls_per_tick() {
    let scheduler = Scheduler::new(10);
    let first = plugin("demo.first");
    let second = plugin("demo.second");
    let base = Instant::now();
    for _ in 0..10 {
        scheduler.enqueue(&first, CallKind::OnEvents, base, noop());
        scheduler.enqueue(&second, CallKind::OnEvents, base, noop());
    }
    scheduler.tick(at(base, 10));
    assert_eq!(executed_count(&scheduler, &first), 4);
    assert_eq!(executed_count(&scheduler, &second), 4);
}

/// REQ: HST-185
#[test]
fn hst_185_sides_are_served_round_robin_one_call_per_turn() {
    let scheduler = Scheduler::new(10);
    let first = plugin("demo.first");
    let second = plugin("demo.second");
    let base = Instant::now();
    for _ in 0..3 {
        scheduler.enqueue(&first, CallKind::OnEvents, base, noop());
        scheduler.enqueue(&second, CallKind::OnEvents, base, noop());
    }
    scheduler.tick(at(base, 10));
    let order: Vec<PluginId> = scheduler
        .executed()
        .iter()
        .map(|call| call.plugin.clone())
        .collect();
    let expected = vec![
        first.clone(),
        second.clone(),
        first.clone(),
        second.clone(),
        first.clone(),
        second.clone(),
    ];
    assert_eq!(order, expected);
}

/// REQ: HST-185
#[test]
fn hst_185_a_flooding_side_cannot_starve_a_quiet_side() {
    let scheduler = Scheduler::new(10);
    let flood = plugin("demo.flood");
    let quiet = plugin("demo.quiet");
    let base = Instant::now();
    for _ in 0..100 {
        scheduler.enqueue(&flood, CallKind::OnEvents, base, noop());
    }
    scheduler.enqueue(&quiet, CallKind::OnEvents, base, noop());
    scheduler.tick(at(base, 10));
    assert_eq!(executed_count(&scheduler, &quiet), 1);
    assert_eq!(executed_count(&scheduler, &flood), 4);
}

// ---- HST-186 ------------------------------------------------------------------------------

/// REQ: HST-186
#[test]
fn hst_186_a_slow_call_does_not_delay_another_plugin() {
    let scheduler = Scheduler::new(10);
    let started = Arc::new(Mutex::new(None::<Instant>));
    let sink = started.clone();
    let base = Instant::now();
    scheduler.enqueue(&plugin("demo.slow"), CallKind::OnEvents, base, sleeper(100));
    let probe: Work = Box::new(move || {
        *sink.lock().expect("start lock") = Some(Instant::now());
    });
    scheduler.enqueue(&plugin("demo.fast"), CallKind::OnEvents, base, probe);
    scheduler.tick(base);
    let started_at = started.lock().expect("start lock").expect("fast call ran");
    assert!(
        started_at.duration_since(base) < ms(60),
        "fast call started {:?} after the tick",
        started_at.duration_since(base)
    );
}

/// REQ: HST-186
#[test]
fn hst_186_a_slow_call_does_not_delay_a_scene_frame() {
    let scheduler = Scheduler::new(10);
    let started = Arc::new(Mutex::new(None::<Instant>));
    let sink = started.clone();
    let base = Instant::now();
    scheduler.enqueue(&plugin("demo.slow"), CallKind::OnEvents, base, sleeper(100));
    let probe: Work = Box::new(move || {
        *sink.lock().expect("start lock") = Some(Instant::now());
    });
    scheduler.enqueue(&plugin("demo.scene"), CallKind::SceneFrame, base, probe);
    scheduler.tick(base);
    let started_at = started.lock().expect("start lock").expect("frame ran");
    assert!(started_at.duration_since(base) < ms(60));
}

/// REQ: HST-186
#[test]
fn hst_186_the_slow_plugin_still_runs_its_own_next_call_after_the_slow_one() {
    let scheduler = Scheduler::new(10);
    let slow = plugin("demo.slow");
    let base = Instant::now();
    scheduler.enqueue(&slow, CallKind::OnEvents, base, sleeper(30));
    scheduler.enqueue(&slow, CallKind::OnEvents, base, noop());
    drain(&scheduler, base, 4);
    assert_eq!(executed_count(&scheduler, &slow), 2);
    assert_eq!(scheduler.max_concurrent_calls_in_store(&slow), 1);
}

// ---- HST-187 ------------------------------------------------------------------------------

fn abandoned_after(kind: CallKind, wait_ms: u64) -> Vec<AbandonedCall> {
    let scheduler = Scheduler::new(10);
    let who = plugin("demo.waiting");
    let base = Instant::now();
    scheduler.set_store_busy(&who, true);
    scheduler.enqueue(&who, kind, base, noop());
    scheduler.tick(base);
    scheduler.tick(at(base, wait_ms));
    scheduler.abandoned()
}

/// REQ: HST-187
#[test]
fn hst_187_menu_items_call_abandoned_after_its_20_ms_deadline() {
    let gone = abandoned_after(CallKind::MenuItems, 21);
    assert_eq!(gone.len(), 1);
    assert_eq!(gone[0].kind, CallKind::MenuItems);
}

/// REQ: HST-187
#[test]
fn hst_187_menu_items_call_not_abandoned_before_its_deadline() {
    assert!(abandoned_after(CallKind::MenuItems, 19).is_empty());
}

/// REQ: HST-187
#[test]
fn hst_187_tokens_call_uses_its_own_10_ms_deadline() {
    assert!(abandoned_after(CallKind::Tokens, 9).is_empty());
    assert_eq!(abandoned_after(CallKind::Tokens, 11).len(), 1);
}

/// REQ: HST-187
#[test]
fn hst_187_settings_validate_uses_its_own_100_ms_deadline() {
    assert!(abandoned_after(CallKind::SettingsValidate, 99).is_empty());
    assert_eq!(abandoned_after(CallKind::SettingsValidate, 101).len(), 1);
}

/// REQ: HST-187
#[test]
fn hst_187_detector_call_uses_its_own_20_ms_deadline() {
    assert!(abandoned_after(CallKind::Detector, 19).is_empty());
    assert_eq!(abandoned_after(CallKind::Detector, 21).len(), 1);
}

/// REQ: HST-187
#[test]
fn hst_187_abandoned_call_never_runs_after_the_store_frees() {
    let scheduler = Scheduler::new(10);
    let who = plugin("demo.waiting");
    let base = Instant::now();
    scheduler.set_store_busy(&who, true);
    scheduler.enqueue(&who, CallKind::MenuItems, base, noop());
    scheduler.tick(base);
    scheduler.tick(at(base, 25));
    scheduler.set_store_busy(&who, false);
    drain(&scheduler, at(base, 25), 3);
    assert_eq!(executed_count(&scheduler, &who), 0);
}

/// REQ: HST-187
#[test]
fn hst_187_call_that_starts_in_time_is_not_abandoned() {
    let scheduler = Scheduler::new(10);
    let who = plugin("demo.waiting");
    let base = Instant::now();
    scheduler.set_store_busy(&who, true);
    scheduler.enqueue(&who, CallKind::MenuItems, base, noop());
    scheduler.tick(base);
    scheduler.set_store_busy(&who, false);
    scheduler.tick(at(base, 10));
    assert_eq!(executed_count(&scheduler, &who), 1);
    assert!(scheduler.abandoned().is_empty());
}

/// REQ: HST-187
#[test]
fn hst_187_on_events_and_render_have_no_caller_deadline() {
    let scheduler = Scheduler::new(10);
    let who = plugin("demo.waiting");
    let base = Instant::now();
    scheduler.set_store_busy(&who, true);
    scheduler.enqueue(&who, CallKind::OnEvents, base, noop());
    scheduler.enqueue(&who, CallKind::Render, base, noop());
    // Ticks stay below the one-second gap rule of HST-195.
    for step in 0..12u64 {
        scheduler.tick(at(base, step * 900));
    }
    assert!(scheduler.abandoned().is_empty());
    scheduler.set_store_busy(&who, false);
    scheduler.tick(at(base, 12 * 900));
    assert_eq!(executed_count(&scheduler, &who), 2);
}

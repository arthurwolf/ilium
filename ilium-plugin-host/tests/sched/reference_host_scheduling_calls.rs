//! Call classes, deadlines, soft and hard breaches, streaks, clamping and measurement
//! (HST-188..196).

use proptest::prelude::*;

use ilium_plugin_api::generated::types::lifecycle::DeactivationReason;
use ilium_plugin_api::limits::{ceiling, clamp, Limits};
use ilium_plugin_api::limits::LimitKey;
use ilium_plugin_api::Tier;
use ilium_plugin_host::calls::{
    batch_call_class, deadline_for, evaluate_breach, streak_limit, BreachVerdict, CallBudget,
    StreakCounter, SOFT_GRACE_MS,
};

use crate::common::*;

// ---- HST-188 ------------------------------------------------------------------------------

fn deadline(class: CallClass) -> Duration {
    ilium_plugin_host::calls::deadline_for(class, &default_limits())
}

/// REQ: HST-188
#[test]
fn hst_188_event_and_command_batches_default_to_20_and_50_ms() {
    assert_eq!(deadline(CallClass::Event), ms(20));
    assert_eq!(deadline(CallClass::Command), ms(50));
}

/// REQ: HST-188
#[test]
fn hst_188_render_and_annotate_default_to_10_ms() {
    assert_eq!(deadline(CallClass::Render), ms(10));
    assert_eq!(deadline(CallClass::Annotate), ms(10));
}

/// REQ: HST-188
#[test]
fn hst_188_activate_and_deactivate_default_to_2000_and_500_ms() {
    assert_eq!(deadline(CallClass::Activate), ms(2_000));
    assert_eq!(deadline(CallClass::Deactivate), ms(500));
}

/// REQ: HST-188
#[test]
fn hst_188_detector_frame_and_migrate_defaults() {
    assert_eq!(deadline(CallClass::Detector), ms(20));
    assert_eq!(deadline(CallClass::Frame), ms(50));
    assert_eq!(deadline(CallClass::Migrate), ms(30_000));
}

/// REQ: HST-188
#[test]
fn hst_188_fixed_provider_classes_ignore_the_limits_map() {
    assert_eq!(deadline(CallClass::MenuItems), ms(20));
    assert_eq!(deadline(CallClass::QueryCatalog), ms(20));
    assert_eq!(deadline(CallClass::ListVirtualChildren), ms(20));
    assert_eq!(deadline(CallClass::Tokens), ms(10));
    assert_eq!(deadline(CallClass::SettingsValidate), ms(100));
}

/// REQ: HST-188
#[test]
fn hst_188_effective_limit_follows_the_resolved_limits() {
    let mut limits = default_limits();
    limits.0.insert(LimitKey::EventDeadlineMs, 35);
    assert_eq!(deadline_for(CallClass::Event, &limits), ms(35));
}

/// REQ: HST-188
#[test]
fn hst_188_t1_ceilings_of_the_call_class_table() {
    let rows = [
        (LimitKey::EventDeadlineMs, 50),
        (LimitKey::CommandDeadlineMs, 100),
        (LimitKey::RenderDeadlineMs, 20),
        (LimitKey::AnnotateDeadlineMs, 20),
        (LimitKey::FrameDeadlineMs, 50),
        (LimitKey::DetectorDeadlineMs, 50),
        (LimitKey::ActivateDeadlineMs, 5_000),
        (LimitKey::DeactivateDeadlineMs, 1_000),
        (LimitKey::MigrateDeadlineMs, 60_000),
        (LimitKey::JobDeadlineMs, 60_000),
        (LimitKey::JobWallMs, 600_000),
    ];
    for (key, value) in rows {
        assert_eq!(ceiling(key, Tier::T1), value, "{key}");
    }
}

/// REQ: HST-188
#[test]
fn hst_188_t2_and_t3_ceilings_of_the_call_class_table() {
    let rows = [
        (LimitKey::EventDeadlineMs, 20),
        (LimitKey::CommandDeadlineMs, 50),
        (LimitKey::RenderDeadlineMs, 10),
        (LimitKey::AnnotateDeadlineMs, 10),
        (LimitKey::FrameDeadlineMs, 50),
        (LimitKey::DetectorDeadlineMs, 20),
        (LimitKey::ActivateDeadlineMs, 2_000),
        (LimitKey::DeactivateDeadlineMs, 500),
        (LimitKey::MigrateDeadlineMs, 30_000),
        (LimitKey::JobDeadlineMs, 10_000),
        (LimitKey::JobWallMs, 100_000),
    ];
    for tier in [Tier::T2, Tier::T3] {
        for (key, value) in rows {
            assert_eq!(ceiling(key, tier), value, "{key} {tier}");
        }
    }
}

/// REQ: HST-188
#[test]
fn hst_188_request_above_the_tier_ceiling_is_clamped_not_refused() {
    let mut requested = Limits::default();
    requested.0.insert("event_deadline_ms".to_owned(), 100);
    let t3 = clamp(&requested, Tier::T3).expect("clamped");
    assert_eq!(t3.get(&LimitKey::EventDeadlineMs), Some(&20));
    let t1 = clamp(&requested, Tier::T1).expect("clamped");
    assert_eq!(t1.get(&LimitKey::EventDeadlineMs), Some(&50));
}

/// REQ: HST-188
#[test]
fn doc_hst_188_call_class_table_has_one_row_per_class() {
    let page = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../docs/plugins-manual/pages/reference-host-scheduling.md");
    let text = std::fs::read_to_string(&page).expect("manual page readable");
    let rows = text
        .lines()
        .skip_while(|line| !line.starts_with("| Call class |"))
        .skip(2)
        .take_while(|line| line.starts_with('|'))
        .count();
    assert_eq!(rows, 21);
}

// ---- HST-189 ------------------------------------------------------------------------------

/// REQ: HST-189
#[test]
fn hst_189_hard_class_below_its_deadline_is_timely() {
    let limits = default_limits();
    for (class, deadline_ms) in [
        (CallClass::Event, 20),
        (CallClass::Command, 50),
        (CallClass::Render, 10),
        (CallClass::Activate, 2_000),
        (CallClass::Deactivate, 500),
        (CallClass::Migrate, 30_000),
    ] {
        assert_eq!(
            evaluate_breach(class, ms(deadline_ms - 1), &limits),
            BreachVerdict::Timely,
            "{class:?}"
        );
    }
}

/// REQ: HST-189
#[test]
fn hst_189_hard_class_past_its_deadline_traps() {
    let limits = default_limits();
    for (class, deadline_ms) in [
        (CallClass::Event, 20),
        (CallClass::Command, 50),
        (CallClass::Render, 10),
        (CallClass::Activate, 2_000),
        (CallClass::Migrate, 30_000),
    ] {
        assert_eq!(
            evaluate_breach(class, ms(deadline_ms + 25), &limits),
            BreachVerdict::HardTrap,
            "{class:?}"
        );
    }
}

/// REQ: HST-189
#[test]
fn hst_189_soft_class_below_its_deadline_is_timely() {
    assert_eq!(
        evaluate_breach(CallClass::Annotate, ms(9), &default_limits()),
        BreachVerdict::Timely
    );
}

/// REQ: HST-189
#[test]
fn hst_189_soft_class_past_its_deadline_is_late_and_its_result_dropped() {
    let limits = default_limits();
    assert_eq!(evaluate_breach(CallClass::Annotate, ms(11), &limits), BreachVerdict::SoftLate);
    assert_eq!(evaluate_breach(CallClass::Annotate, ms(109), &limits), BreachVerdict::SoftLate);
}

/// REQ: HST-189
#[test]
fn hst_189_soft_class_past_deadline_plus_100_ms_traps() {
    assert_eq!(SOFT_GRACE_MS, 100);
    let limits = default_limits();
    assert_eq!(evaluate_breach(CallClass::Annotate, ms(111), &limits), BreachVerdict::SoftTrap);
    assert_eq!(evaluate_breach(CallClass::Detector, ms(121), &limits), BreachVerdict::SoftTrap);
    assert_eq!(evaluate_breach(CallClass::Detector, ms(119), &limits), BreachVerdict::SoftLate);
}

/// REQ: HST-189
#[test]
fn hst_189_hard_breach_of_a_running_guest_traps_the_instance() {
    let shared = Shared::new();
    let handle = build(ScriptedGuest::new(shared).sleeping(120));
    handle.deliver(vec![theme()]);
    handle.pump(8);
    let failures = handle.failures();
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].code.0, "trap.deadline");
}

proptest! {
    // REQ: HST-189
    #[test]
    fn hst_189_soft_verdict_is_monotonic_in_elapsed_time(first in 0u64..300, extra in 0u64..300) {
        let limits = default_limits();
        let rank = |verdict: BreachVerdict| match verdict {
            BreachVerdict::Timely => 0,
            BreachVerdict::SoftLate => 1,
            BreachVerdict::SoftTrap | BreachVerdict::HardTrap => 2,
        };
        let early = evaluate_breach(CallClass::Annotate, ms(first), &limits);
        let late = evaluate_breach(CallClass::Annotate, ms(first + extra), &limits);
        prop_assert!(rank(early) <= rank(late));
    }
}

// ---- HST-190 ------------------------------------------------------------------------------

/// REQ: HST-190
#[test]
fn hst_190_streak_limit_is_3_for_the_listed_soft_classes() {
    for class in [
        CallClass::Annotate,
        CallClass::Detector,
        CallClass::MenuItems,
        CallClass::QueryCatalog,
        CallClass::ListVirtualChildren,
        CallClass::Tokens,
        CallClass::SettingsValidate,
    ] {
        assert_eq!(streak_limit(class), Some(3), "{class:?}");
    }
}

/// REQ: HST-190
#[test]
fn hst_190_streak_limit_is_30_for_scene_frames() {
    assert_eq!(streak_limit(CallClass::Frame), Some(30));
}

/// REQ: HST-190
#[test]
fn hst_190_hard_classes_have_no_streak() {
    for class in [CallClass::Event, CallClass::Render, CallClass::Activate, CallClass::Deactivate] {
        assert_eq!(streak_limit(class), None, "{class:?}");
    }
}

/// REQ: HST-190
#[test]
fn hst_190_third_breach_in_a_row_traps_for_annotate() {
    let mut streak = StreakCounter::new(CallClass::Annotate);
    assert!(!streak.record(false));
    assert!(!streak.record(false));
    assert!(streak.record(false));
}

/// REQ: HST-190
#[test]
fn hst_190_timely_call_resets_the_streak() {
    let mut streak = StreakCounter::new(CallClass::Tokens);
    assert!(!streak.record(false));
    assert!(!streak.record(false));
    assert!(!streak.record(true));
    assert!(!streak.record(false));
    assert!(!streak.record(false));
    assert!(streak.record(false));
}

/// REQ: HST-190
#[test]
fn hst_190_thirtieth_breach_in_a_row_traps_a_scene() {
    let mut streak = StreakCounter::new(CallClass::Frame);
    for _ in 0..29 {
        assert!(!streak.record(false));
    }
    assert!(streak.record(false));
}

/// REQ: HST-190
#[test]
fn hst_190_scene_streak_of_29_then_timely_starts_over() {
    let mut streak = StreakCounter::new(CallClass::Frame);
    for _ in 0..29 {
        assert!(!streak.record(false));
    }
    assert!(!streak.record(true));
    for _ in 0..29 {
        assert!(!streak.record(false));
    }
    assert!(streak.record(false));
}

proptest! {
    // REQ: HST-190
    #[test]
    fn hst_190_streak_traps_exactly_when_limit_breaches_come_in_a_row(
        history in proptest::collection::vec(any::<bool>(), 0..40)
    ) {
        let mut streak = StreakCounter::new(CallClass::Detector);
        let mut run = 0u32;
        for timely in history {
            let trapped = streak.record(timely);
            run = if timely { 0 } else { run + 1 };
            prop_assert_eq!(trapped, run >= 3);
            if trapped {
                break;
            }
        }
    }
}

// ---- HST-191 ------------------------------------------------------------------------------

fn slow_deactivating_handle() -> (Arc<Shared>, HarnessHandle) {
    let shared = Shared::new();
    let mut guest = ScriptedGuest::new(shared.clone());
    guest.deactivate_sleep_ms = 200;
    guest.restore = Some(vec![1, 2, 3]);
    let mut limits = Limits::default();
    limits.0.insert("deactivate_deadline_ms".to_owned(), 50);
    let handle = build_with(HostHarness::new().limits(limits), guest);
    (shared, handle)
}

/// REQ: HST-191
#[test]
fn hst_191_deactivate_breach_discards_the_restore_bytes() {
    let (_shared, handle) = slow_deactivating_handle();
    assert_eq!(handle.deactivate(DeactivationReason::Disabled), None);
}

/// REQ: HST-191
#[test]
fn hst_191_deactivate_breach_logs_deactivate_deadline_at_warn() {
    let (_shared, handle) = slow_deactivating_handle();
    handle.deactivate(DeactivationReason::Disabled);
    let lines: Vec<_> = handle
        .logs()
        .into_iter()
        .filter(|line| line.message.contains("deactivate.deadline"))
        .collect();
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].level, "warn");
}

/// REQ: HST-191
#[test]
fn hst_191_deactivate_breach_is_not_counted_as_a_failure() {
    let (_shared, handle) = slow_deactivating_handle();
    handle.deactivate(DeactivationReason::Disabled);
    assert_eq!(handle.counted_failures(), 0);
}

/// REQ: HST-191
#[test]
fn hst_191_timely_deactivate_keeps_the_restore_bytes() {
    let shared = Shared::new();
    let mut guest = ScriptedGuest::new(shared);
    guest.restore = Some(vec![9, 8, 7]);
    let handle = build(guest);
    assert_eq!(handle.deactivate(DeactivationReason::Disabled), Some(vec![9, 8, 7]));
    assert!(handle
        .logs()
        .iter()
        .all(|line| !line.message.contains("deactivate.deadline")));
}

// ---- HST-192 ------------------------------------------------------------------------------

/// REQ: HST-192
#[test]
fn hst_192_batch_with_command_invoked_selects_the_command_class() {
    assert_eq!(batch_call_class(&[theme(), command_invoked()]), CallClass::Command);
    assert_eq!(batch_call_class(&[command_invoked()]), CallClass::Command);
}

/// REQ: HST-192
#[test]
fn hst_192_command_invoked_anywhere_in_the_batch_selects_the_command_class() {
    assert_eq!(
        batch_call_class(&[theme(), keymap(), key_pressed(), command_invoked(), theme()]),
        CallClass::Command
    );
}

/// REQ: HST-192
#[test]
fn hst_192_batch_without_command_uses_the_event_class() {
    assert_eq!(batch_call_class(&[theme(), key_pressed(), ui_action()]), CallClass::Event);
    assert_eq!(batch_call_class(&[theme()]), CallClass::Event);
}

/// REQ: HST-192
#[test]
fn hst_192_command_batch_runs_under_command_deadline_ms() {
    let limits = default_limits();
    let class = batch_call_class(&[command_invoked()]);
    assert_eq!(deadline_for(class, &limits), ms(50));
    let class = batch_call_class(&[theme()]);
    assert_eq!(deadline_for(class, &limits), ms(20));
}

/// REQ: HST-192
#[test]
fn hst_192_a_35_ms_command_batch_is_not_trapped() {
    let shared = Shared::new();
    let handle = build(ScriptedGuest::new(shared).sleeping(35));
    handle.deliver(vec![command_invoked()]);
    handle.pump(8);
    assert!(handle.failures().is_empty());
}

/// REQ: HST-192
#[test]
fn hst_192_a_90_ms_batch_without_command_is_trapped_at_the_event_deadline() {
    let shared = Shared::new();
    let handle = build(ScriptedGuest::new(shared).sleeping(90));
    handle.deliver(vec![theme()]);
    handle.pump(8);
    assert_eq!(handle.failures().len(), 1);
}

// ---- HST-193 ------------------------------------------------------------------------------

fn clamp_lines(handle: &HarnessHandle, key: &str) -> usize {
    handle
        .logs()
        .iter()
        .filter(|line| line.message.contains(key))
        .count()
}

fn limits_with(entries: &[(&str, u64)]) -> Limits {
    let mut limits = Limits::default();
    for (key, value) in entries {
        limits.0.insert((*key).to_owned(), *value);
    }
    limits
}

/// REQ: HST-193
#[test]
fn hst_193_deadline_above_the_ceiling_is_clamped_for_the_tier() {
    let requested = limits_with(&[("event_deadline_ms", 500)]);
    let resolved = clamp(&requested, Tier::T3).expect("clamped");
    assert_eq!(deadline_for(CallClass::Event, &ResolvedLimits(resolved)), ms(20));
}

/// REQ: HST-193
#[test]
fn hst_193_t1_ceiling_is_higher_than_the_t3_ceiling() {
    let requested = limits_with(&[("event_deadline_ms", 500)]);
    let resolved = clamp(&requested, Tier::T1).expect("clamped");
    assert_eq!(deadline_for(CallClass::Event, &ResolvedLimits(resolved)), ms(50));
}

/// REQ: HST-193
#[test]
fn hst_193_clamp_is_logged_once_per_plugin_and_key() {
    let shared = Shared::new();
    let builder = HostHarness::new().limits(limits_with(&[("event_deadline_ms", 500)]));
    let handle = build_with(builder, ScriptedGuest::new(shared));
    for _ in 0..3 {
        handle.deliver(vec![theme()]);
        handle.pump(8);
    }
    assert_eq!(clamp_lines(&handle, "event_deadline_ms"), 1);
}

/// REQ: HST-193
#[test]
fn hst_193_each_clamped_key_is_logged_separately() {
    let shared = Shared::new();
    let builder = HostHarness::new().limits(limits_with(&[
        ("event_deadline_ms", 500),
        ("render_deadline_ms", 500),
    ]));
    let handle = build_with(builder, ScriptedGuest::new(shared));
    handle.deliver(vec![theme()]);
    handle.pump(8);
    assert_eq!(clamp_lines(&handle, "event_deadline_ms"), 1);
    assert_eq!(clamp_lines(&handle, "render_deadline_ms"), 1);
}

/// REQ: HST-193
#[test]
fn hst_193_value_within_the_ceiling_is_not_logged() {
    let shared = Shared::new();
    let builder = HostHarness::new().limits(limits_with(&[("event_deadline_ms", 20)]));
    let handle = build_with(builder, ScriptedGuest::new(shared));
    handle.deliver(vec![theme()]);
    handle.pump(8);
    assert_eq!(clamp_lines(&handle, "event_deadline_ms"), 0);
}

// ---- HST-194 / HST-195 --------------------------------------------------------------------

/// REQ: HST-194
#[test]
fn hst_194_five_5_ms_synchronous_imports_use_25_ms_of_the_budget() {
    let mut budget = CallBudget::new(CallClass::Event, &default_limits());
    for _ in 0..5 {
        budget.import_ms(5);
    }
    budget.returned();
    assert_eq!(budget.used(), ms(25));
    assert_eq!(budget.verdict(), BreachVerdict::HardTrap);
}

/// REQ: HST-194
#[test]
fn hst_194_import_time_and_guest_time_add_up() {
    let mut budget = CallBudget::new(CallClass::Event, &default_limits());
    budget.guest_ms(8);
    budget.import_ms(8);
    budget.returned();
    assert_eq!(budget.used(), ms(16));
    assert_eq!(budget.verdict(), BreachVerdict::Timely);
}

/// REQ: HST-194
#[test]
fn hst_194_time_after_the_call_returned_does_not_count() {
    let mut budget = CallBudget::new(CallClass::Event, &default_limits());
    budget.guest_ms(5);
    budget.returned();
    budget.post_return_ms(10_000);
    assert_eq!(budget.used(), ms(5));
    assert_eq!(budget.verdict(), BreachVerdict::Timely);
}

/// REQ: HST-194
#[test]
fn hst_194_import_that_ends_exactly_below_the_deadline_is_timely() {
    let mut budget = CallBudget::new(CallClass::Render, &default_limits());
    budget.import_ms(9);
    budget.returned();
    assert_eq!(budget.verdict(), BreachVerdict::Timely);
}

/// REQ: HST-195
#[test]
fn hst_195_tick_gap_over_one_second_is_ignored() {
    let mut budget = CallBudget::new(CallClass::Event, &default_limits());
    budget.guest_ms(5);
    budget.tick_gap(1_500);
    budget.returned();
    assert_eq!(budget.verdict(), BreachVerdict::Timely);
    assert_eq!(budget.used(), ms(5));
}

/// REQ: HST-195
#[test]
fn hst_195_tick_gap_of_exactly_one_second_is_not_ignored() {
    let mut budget = CallBudget::new(CallClass::Event, &default_limits());
    budget.tick_gap(1_000);
    budget.returned();
    assert_eq!(budget.verdict(), BreachVerdict::HardTrap);
}

/// REQ: HST-195
#[test]
fn hst_195_scheduler_ignores_the_tick_after_a_suspend_gap() {
    let scheduler = Scheduler::new(10);
    let who = plugin("demo.suspended");
    let base = Instant::now();
    scheduler.set_store_busy(&who, true);
    scheduler.enqueue(&who, CallKind::MenuItems, base, noop());
    scheduler.tick(base);
    scheduler.tick(at(base, 1_500));
    assert_eq!(scheduler.breach_count(&who, CallClass::MenuItems), 0);
    assert!(scheduler.abandoned().is_empty());
}

/// REQ: HST-195
#[test]
fn hst_195_scheduler_acts_on_a_tick_after_a_gap_of_one_second() {
    let scheduler = Scheduler::new(10);
    let who = plugin("demo.suspended");
    let base = Instant::now();
    scheduler.set_store_busy(&who, true);
    scheduler.enqueue(&who, CallKind::MenuItems, base, noop());
    scheduler.tick(base);
    scheduler.tick(at(base, 1_000));
    assert_eq!(scheduler.breach_count(&who, CallClass::MenuItems), 1);
}

/// REQ: HST-195
#[test]
fn hst_195_a_tick_that_goes_backwards_never_produces_a_breach() {
    let scheduler = Scheduler::new(10);
    let who = plugin("demo.clock");
    let base = Instant::now() + ms(500);
    scheduler.set_store_busy(&who, true);
    scheduler.enqueue(&who, CallKind::MenuItems, base, noop());
    scheduler.tick(base);
    scheduler.tick(base - ms(300));
    assert_eq!(scheduler.breach_count(&who, CallClass::MenuItems), 0);
}

// ---- HST-196 ------------------------------------------------------------------------------

/// REQ: HST-196
#[test]
fn hst_196_only_activate_jobs_and_activation_waiting_service_calls_may_yield() {
    for kind in CallKind::ALL {
        let allowed = matches!(
            kind,
            CallKind::Activate | CallKind::JobRun | CallKind::ServiceCallWaitingForActivation
        );
        assert_eq!(kind.may_yield(), allowed, "{kind:?}");
    }
}

/// REQ: HST-196
#[test]
fn hst_196_non_yielding_class_runs_to_completion_without_yielding() {
    let scheduler = Scheduler::new(10);
    let who = plugin("demo.long");
    let base = Instant::now();
    scheduler.enqueue(&who, CallKind::OnEvents, base, sleeper(35));
    drain(&scheduler, base, 6);
    let executed = scheduler.executed();
    assert_eq!(executed.len(), 1);
    assert_eq!(executed[0].yields, 0);
}

/// REQ: HST-196
#[test]
fn hst_196_non_yielding_calls_stay_on_one_thread_until_they_return() {
    let scheduler = Scheduler::new(10);
    let who = plugin("demo.long");
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = seen.clone();
    let work: Work = Box::new(move || {
        for _ in 0..4 {
            sink.lock().expect("seen lock").push(std::thread::current().id());
            std::thread::sleep(ms(8));
        }
    });
    let base = Instant::now();
    scheduler.enqueue(&who, CallKind::Render, base, work);
    drain(&scheduler, base, 6);
    let ids = seen.lock().expect("seen lock").clone();
    assert_eq!(ids.len(), 4);
    assert!(ids.iter().all(|id| *id == ids[0]));
}

//! api-clock-and-timers.md and api-durable-timers.md: `set`, `cancel`, `at` and `cancel-durable`
//! of `ilium:host/timers`. A timer needs no grant, holds at least 50 ms, never fires inside the
//! call that set it, and a plugin holds at most 64 timers (64 durable timers separately).
//!
//! Event observation uses `fixture.events()` (part 1 API); gaps are in API_GAPS_p2.md.

use std::time::Duration;

use ilium_plugin_api::generated::types::{DurableTimerEvent, EpochMs, ErrorCode, Event, TimerId};
use ilium_plugin_host::testing::imports_fixture::ImportsFixture;
use proptest::prelude::*;

use crate::support::*;

fn timer_fixture() -> ImportsFixture {
    fixture_t2(&[])
}

fn fired(fixture: &mut ImportsFixture) -> Vec<TimerId> {
    fixture
        .events()
        .into_iter()
        .filter_map(|event| match event {
            Event::TimerFired(id) => Some(id),
            _ => None,
        })
        .collect()
}

fn durable_fired(fixture: &mut ImportsFixture) -> Vec<DurableTimerEvent> {
    fixture
        .events()
        .into_iter()
        .filter_map(|event| match event {
            Event::DurableTimerFired(event) => Some(event),
            _ => None,
        })
        .collect()
}

fn run_for(fixture: &mut ImportsFixture, millis: u64) {
    fixture.advance(Duration::from_millis(millis));
    fixture.pump();
}

fn now_plus(millis: u64) -> EpochMs {
    EpochMs(TEST_CLOCK_START_MILLIS + millis)
}

// ---- set ---------------------------------------------------------------------------------------

/// REQ: WIT:ilium:host/timers.set
#[test]
fn timers_set_returns_a_timer_id_and_fires_it_once_after_the_delay() {
    let mut fixture = timer_fixture();
    let id = expect_ok(fixture.timers().set(100, None));
    run_for(&mut fixture, 100);
    assert_eq!(fired(&mut fixture), vec![id]);
    run_for(&mut fixture, 1000);
    assert!(fired(&mut fixture).is_empty());
}

/// REQ: WIT:ilium:host/timers.set
#[test]
fn timers_set_does_not_fire_inside_the_call_that_set_it() {
    let mut fixture = timer_fixture();
    let _ = expect_ok(fixture.timers().set(50, None));
    assert!(fired(&mut fixture).is_empty());
}

/// REQ: WIT:ilium:host/timers.set
#[test]
fn timers_set_does_not_fire_before_the_delay() {
    let mut fixture = timer_fixture();
    let _ = expect_ok(fixture.timers().set(200, None));
    run_for(&mut fixture, 199);
    assert!(fired(&mut fixture).is_empty());
}

/// REQ: WIT:ilium:host/timers.set
#[test]
fn timers_set_after_50_milliseconds_accepted() {
    let mut fixture = timer_fixture();
    expect_ok(fixture.timers().set(50, None));
}

/// REQ: WIT:ilium:host/timers.set
#[test]
fn timers_set_after_49_milliseconds_invalid_argument() {
    let mut fixture = timer_fixture();
    let code = expect_code(fixture.timers().set(49, None));
    assert_eq!(code, ErrorCode::InvalidArgument);
}

/// REQ: WIT:ilium:host/timers.set
#[test]
fn timers_set_after_zero_milliseconds_invalid_argument() {
    let mut fixture = timer_fixture();
    let code = expect_code(fixture.timers().set(0, None));
    assert_eq!(code, ErrorCode::InvalidArgument);
}

/// REQ: WIT:ilium:host/timers.set
#[test]
fn timers_set_repeat_of_50_milliseconds_accepted() {
    let mut fixture = timer_fixture();
    expect_ok(fixture.timers().set(50, Some(50)));
}

/// REQ: WIT:ilium:host/timers.set
#[test]
fn timers_set_repeat_of_49_milliseconds_invalid_argument() {
    let mut fixture = timer_fixture();
    let code = expect_code(fixture.timers().set(100, Some(49)));
    assert_eq!(code, ErrorCode::InvalidArgument);
}

/// REQ: WIT:ilium:host/timers.set
#[test]
fn timers_set_repeating_timer_fires_on_a_drift_free_schedule() {
    let mut fixture = timer_fixture();
    let id = expect_ok(fixture.timers().set(100, Some(100)));
    // Pump late: a drifting schedule (now + repeat) would then fire at 230, not 200.
    run_for(&mut fixture, 130);
    assert_eq!(fired(&mut fixture), vec![id]);
    run_for(&mut fixture, 70);
    assert_eq!(fired(&mut fixture), vec![id]);
    run_for(&mut fixture, 100);
    assert_eq!(fired(&mut fixture), vec![id]);
}

/// REQ: WIT:ilium:host/timers.set
#[test]
fn timers_set_repeating_timer_missed_ticks_coalesce_into_one_event() {
    let mut fixture = timer_fixture();
    let id = expect_ok(fixture.timers().set(100, Some(100)));
    run_for(&mut fixture, 550);
    assert_eq!(fired(&mut fixture), vec![id]);
    // The schedule stays first + n * repeat: the next tick is due at 600 ms.
    run_for(&mut fixture, 50);
    assert_eq!(fired(&mut fixture), vec![id]);
}

/// REQ: WIT:ilium:host/timers.set
#[test]
fn timers_set_two_timers_get_different_ids() {
    let mut fixture = timer_fixture();
    let first = expect_ok(fixture.timers().set(100, None));
    let second = expect_ok(fixture.timers().set(100, None));
    assert_ne!(first, second);
}

/// REQ: WIT:ilium:host/timers.set
#[test]
fn timers_set_64_held_timers_accepted() {
    let mut fixture = timer_fixture();
    for _ in 0..64 {
        expect_ok(fixture.timers().set(60_000, None));
    }
}

/// REQ: WIT:ilium:host/timers.set
#[test]
fn timers_set_the_65th_held_timer_quota_exceeded() {
    let mut fixture = timer_fixture();
    for _ in 0..64 {
        expect_ok(fixture.timers().set(60_000, None));
    }
    let code = expect_code(fixture.timers().set(60_000, None));
    assert_eq!(code, ErrorCode::QuotaExceeded);
}

/// REQ: WIT:ilium:host/timers.set
#[test]
fn timers_set_a_fired_one_shot_timer_frees_its_slot() {
    let mut fixture = timer_fixture();
    expect_ok(fixture.timers().set(100, None));
    for _ in 0..63 {
        expect_ok(fixture.timers().set(60_000, None));
    }
    run_for(&mut fixture, 100);
    let _ = fired(&mut fixture);
    expect_ok(fixture.timers().set(60_000, None));
}

/// REQ: WIT:ilium:host/timers.set
#[test]
fn timers_set_durable_timers_do_not_count_against_the_64_held_timers() {
    let mut fixture = timer_fixture();
    for index in 0..64 {
        expect_ok(fixture.timers().at(now_plus(60_000), format!("d{index}")));
    }
    expect_ok(fixture.timers().set(60_000, None));
}

proptest! {
    /// REQ: WIT:ilium:host/timers.set
    #[test]
    fn timers_set_accepts_exactly_delays_of_50_milliseconds_and_more(after in 0u32..200) {
        let mut fixture = timer_fixture();
        let result = fixture.timers().set(after, None);
        if after >= 50 {
            prop_assert!(result.is_ok());
        } else {
            prop_assert_eq!(result.err().map(|error| error.code), Some(ErrorCode::InvalidArgument));
        }
    }
}

// ---- cancel ------------------------------------------------------------------------------------

/// REQ: WIT:ilium:host/timers.cancel
#[test]
fn timers_cancel_stops_a_pending_timer() {
    let mut fixture = timer_fixture();
    let id = expect_ok(fixture.timers().set(100, None));
    fixture.timers().cancel(id);
    run_for(&mut fixture, 500);
    assert!(fired(&mut fixture).is_empty());
}

/// REQ: WIT:ilium:host/timers.cancel
#[test]
fn timers_cancel_stops_a_repeating_timer() {
    let mut fixture = timer_fixture();
    let id = expect_ok(fixture.timers().set(100, Some(100)));
    run_for(&mut fixture, 100);
    let _ = fired(&mut fixture);
    fixture.timers().cancel(id);
    run_for(&mut fixture, 1000);
    assert!(fired(&mut fixture).is_empty());
}

/// REQ: WIT:ilium:host/timers.cancel
#[test]
fn timers_cancel_removes_a_queued_event_that_was_not_yet_delivered() {
    let mut fixture = timer_fixture();
    let id = expect_ok(fixture.timers().set(100, None));
    fixture.advance(Duration::from_millis(100));
    fixture.timers().cancel(id);
    fixture.pump();
    assert!(fired(&mut fixture).is_empty());
}

/// REQ: WIT:ilium:host/timers.cancel
#[test]
fn timers_cancel_of_an_unknown_id_does_nothing() {
    let mut fixture = timer_fixture();
    fixture.timers().cancel(TimerId(987_654));
}

/// REQ: WIT:ilium:host/timers.cancel
#[test]
fn timers_cancel_twice_does_nothing_the_second_time() {
    let mut fixture = timer_fixture();
    let id = expect_ok(fixture.timers().set(100, None));
    fixture.timers().cancel(id);
    fixture.timers().cancel(id);
}

/// REQ: WIT:ilium:host/timers.cancel
#[test]
fn timers_cancel_of_a_fired_one_shot_id_does_nothing() {
    let mut fixture = timer_fixture();
    let id = expect_ok(fixture.timers().set(100, None));
    run_for(&mut fixture, 100);
    let _ = fired(&mut fixture);
    fixture.timers().cancel(id);
}

/// REQ: WIT:ilium:host/timers.cancel
#[test]
fn timers_cancel_frees_the_slot() {
    let mut fixture = timer_fixture();
    let mut ids = Vec::new();
    for _ in 0..64 {
        ids.push(expect_ok(fixture.timers().set(60_000, None)));
    }
    fixture.timers().cancel(ids[0]);
    expect_ok(fixture.timers().set(60_000, None));
}

/// REQ: WIT:ilium:host/timers.cancel
#[test]
fn timers_cancel_leaves_other_timers_running() {
    let mut fixture = timer_fixture();
    let cancelled = expect_ok(fixture.timers().set(100, None));
    let kept = expect_ok(fixture.timers().set(100, None));
    fixture.timers().cancel(cancelled);
    run_for(&mut fixture, 100);
    assert_eq!(fired(&mut fixture), vec![kept]);
}

// ---- at ----------------------------------------------------------------------------------------

/// REQ: WIT:ilium:host/timers.at
#[test]
fn timers_at_fires_a_durable_timer_event_with_its_name() {
    let mut fixture = timer_fixture();
    let id = expect_ok(fixture.timers().at(now_plus(5000), "refresh".to_owned()));
    run_for(&mut fixture, 5000);
    let events = durable_fired(&mut fixture);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].name, "refresh");
    assert_eq!(events[0].timer, id);
}

/// REQ: WIT:ilium:host/timers.at
#[test]
fn timers_at_reports_how_late_the_timer_fired() {
    let mut fixture = timer_fixture();
    expect_ok(fixture.timers().at(now_plus(1000), "late".to_owned()));
    run_for(&mut fixture, 1750);
    let events = durable_fired(&mut fixture);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].late_by_millis, 750);
}

/// REQ: WIT:ilium:host/timers.at
#[test]
fn timers_at_does_not_fire_before_the_wall_clock_time() {
    let mut fixture = timer_fixture();
    expect_ok(fixture.timers().at(now_plus(5000), "early".to_owned()));
    run_for(&mut fixture, 4999);
    assert!(durable_fired(&mut fixture).is_empty());
}

/// REQ: WIT:ilium:host/timers.at
#[test]
fn timers_at_name_of_64_characters_accepted() {
    let mut fixture = timer_fixture();
    expect_ok(fixture.timers().at(now_plus(5000), "n".repeat(64)));
}

/// REQ: WIT:ilium:host/timers.at
#[test]
fn timers_at_name_of_65_characters_invalid_argument() {
    let mut fixture = timer_fixture();
    let code = expect_code(fixture.timers().at(now_plus(5000), "n".repeat(65)));
    assert_eq!(code, ErrorCode::InvalidArgument);
}

/// REQ: WIT:ilium:host/timers.at
#[test]
fn timers_at_empty_name_invalid_argument() {
    let mut fixture = timer_fixture();
    let code = expect_code(fixture.timers().at(now_plus(5000), String::new()));
    assert_eq!(code, ErrorCode::InvalidArgument);
}

/// REQ: WIT:ilium:host/timers.at
#[test]
fn timers_at_same_name_replaces_the_earlier_timer() {
    let mut fixture = timer_fixture();
    expect_ok(fixture.timers().at(now_plus(1000), "job".to_owned()));
    expect_ok(fixture.timers().at(now_plus(3000), "job".to_owned()));
    run_for(&mut fixture, 1500);
    assert!(durable_fired(&mut fixture).is_empty());
    run_for(&mut fixture, 1600);
    assert_eq!(durable_fired(&mut fixture).len(), 1);
}

/// REQ: WIT:ilium:host/timers.at
#[test]
fn timers_at_a_time_more_than_one_second_in_the_past_invalid_argument() {
    let mut fixture = timer_fixture();
    let code = expect_code(
        fixture
            .timers()
            .at(EpochMs(TEST_CLOCK_START_MILLIS - 1001), "past".to_owned()),
    );
    assert_eq!(code, ErrorCode::InvalidArgument);
}

/// REQ: WIT:ilium:host/timers.at
#[test]
fn timers_at_a_time_up_to_one_second_in_the_past_accepted() {
    let mut fixture = timer_fixture();
    expect_ok(
        fixture
            .timers()
            .at(EpochMs(TEST_CLOCK_START_MILLIS - 1000), "recent".to_owned()),
    );
}

/// REQ: WIT:ilium:host/timers.at
#[test]
fn timers_at_64_durable_timers_accepted_and_the_65th_quota_exceeded() {
    let mut fixture = timer_fixture();
    for index in 0..64 {
        expect_ok(fixture.timers().at(now_plus(60_000), format!("d{index}")));
    }
    let code = expect_code(fixture.timers().at(now_plus(60_000), "d64".to_owned()));
    assert_eq!(code, ErrorCode::QuotaExceeded);
}

/// REQ: WIT:ilium:host/timers.at
#[test]
fn timers_at_replacing_a_name_at_the_limit_is_not_a_new_timer() {
    let mut fixture = timer_fixture();
    for index in 0..64 {
        expect_ok(fixture.timers().at(now_plus(60_000), format!("d{index}")));
    }
    expect_ok(fixture.timers().at(now_plus(90_000), "d0".to_owned()));
}

// ---- cancel-durable ----------------------------------------------------------------------------

/// REQ: WIT:ilium:host/timers.cancel-durable
#[test]
fn timers_cancel_durable_removes_the_named_timer() {
    let mut fixture = timer_fixture();
    expect_ok(fixture.timers().at(now_plus(1000), "gone".to_owned()));
    fixture.timers().cancel_durable("gone".to_owned());
    run_for(&mut fixture, 2000);
    assert!(durable_fired(&mut fixture).is_empty());
}

/// REQ: WIT:ilium:host/timers.cancel-durable
#[test]
fn timers_cancel_durable_of_an_unknown_name_does_nothing() {
    let mut fixture = timer_fixture();
    fixture.timers().cancel_durable("never-set".to_owned());
}

/// REQ: WIT:ilium:host/timers.cancel-durable
#[test]
fn timers_cancel_durable_keeps_the_other_timers() {
    let mut fixture = timer_fixture();
    expect_ok(fixture.timers().at(now_plus(1000), "gone".to_owned()));
    expect_ok(fixture.timers().at(now_plus(1000), "stay".to_owned()));
    fixture.timers().cancel_durable("gone".to_owned());
    run_for(&mut fixture, 1000);
    let events = durable_fired(&mut fixture);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].name, "stay");
}

/// REQ: WIT:ilium:host/timers.cancel-durable
#[test]
fn timers_cancel_durable_frees_the_slot() {
    let mut fixture = timer_fixture();
    for index in 0..64 {
        expect_ok(fixture.timers().at(now_plus(60_000), format!("d{index}")));
    }
    fixture.timers().cancel_durable("d10".to_owned());
    expect_ok(fixture.timers().at(now_plus(60_000), "again".to_owned()));
}

/// REQ: WIT:ilium:host/timers.cancel-durable
#[test]
fn timers_cancel_durable_twice_does_nothing_the_second_time() {
    let mut fixture = timer_fixture();
    expect_ok(fixture.timers().at(now_plus(1000), "once".to_owned()));
    fixture.timers().cancel_durable("once".to_owned());
    fixture.timers().cancel_durable("once".to_owned());
}

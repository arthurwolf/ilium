//! api-clock-and-timers.md, clock half: `clock.now-unix-millis` and `clock.monotonic-millis`.

use std::time::Duration;

use proptest::prelude::*;

use crate::support::*;

/// REQ: WIT:ilium:host/clock.now-unix-millis
#[test]
fn clock_now_unix_millis_starts_at_2030_01_01() {
    let mut fixture = fixture_t2(&[]);
    assert_eq!(fixture.clock().now_unix_millis(), TEST_CLOCK_START_MILLIS);
}

/// REQ: WIT:ilium:host/clock.now-unix-millis
#[test]
fn clock_now_unix_millis_moves_only_with_advance() {
    let mut fixture = fixture_t2(&[]);
    let first = fixture.clock().now_unix_millis();
    assert_eq!(fixture.clock().now_unix_millis(), first);
    fixture.advance(Duration::from_millis(1500));
    assert_eq!(fixture.clock().now_unix_millis(), first + 1500);
}

/// REQ: WIT:ilium:host/clock.now-unix-millis
#[test]
fn clock_now_unix_millis_advance_of_zero_changes_nothing() {
    let mut fixture = fixture_t2(&[]);
    fixture.advance(Duration::ZERO);
    assert_eq!(fixture.clock().now_unix_millis(), TEST_CLOCK_START_MILLIS);
}

/// REQ: WIT:ilium:host/clock.monotonic-millis
#[test]
fn clock_monotonic_millis_advances_by_the_advanced_time() {
    let mut fixture = fixture_t2(&[]);
    let before = fixture.clock().monotonic_millis();
    fixture.advance(Duration::from_secs(2));
    assert_eq!(fixture.clock().monotonic_millis(), before + 2000);
}

/// REQ: WIT:ilium:host/clock.monotonic-millis
#[test]
fn clock_monotonic_millis_is_stable_between_calls_without_advance() {
    let mut fixture = fixture_t2(&[]);
    let first = fixture.clock().monotonic_millis();
    assert_eq!(fixture.clock().monotonic_millis(), first);
}

/// REQ: WIT:ilium:host/clock.monotonic-millis
#[test]
fn clock_monotonic_millis_does_not_follow_a_wall_clock_step_back() {
    let mut fixture = fixture_t2(&[]);
    fixture.advance(Duration::from_secs(5));
    let before = fixture.clock().monotonic_millis();
    fixture.set_wall_clock_unix_millis(TEST_CLOCK_START_MILLIS - 3_600_000);
    assert!(fixture.clock().monotonic_millis() >= before);
}

proptest! {
    // REQ: WIT:ilium:host/clock.monotonic-millis
    #[test]
    fn clock_monotonic_millis_never_goes_backwards(steps in proptest::collection::vec(0u64..10_000, 1..30)) {
        let mut fixture = fixture_t2(&[]);
        let mut last = fixture.clock().monotonic_millis();
        for step in steps {
            fixture.advance(Duration::from_millis(step));
            let now = fixture.clock().monotonic_millis();
            prop_assert!(now >= last);
            prop_assert_eq!(now - last, step);
            last = now;
        }
    }
}

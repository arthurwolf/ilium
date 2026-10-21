//! Host implementation of `ilium:host/clock` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostClock` trait; hand-owned afterwards.

use ilium_plugin_api::generated::host::clock::HostClock;

/// Host side of the `clock` interface.
#[derive(Debug)]
pub struct ClockImport;

impl HostClock for ClockImport {
    fn now_unix_millis(&mut self) -> u64 {
        unimplemented!("host::imports::clock::now_unix_millis")
    }

    fn monotonic_millis(&mut self) -> u64 {
        unimplemented!("host::imports::clock::monotonic_millis")
    }
}

#[cfg(test)]
mod tests {
    use super::ClockImport;
    use ilium_plugin_api::generated::host::clock::HostClock;
    use std::thread;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    /// Allowed difference between the host reading and the test's own system-clock reading.
    const WALL_CLOCK_TOLERANCE_MILLIS: u64 = 5_000;

    /// Number of consecutive reads used to check that the guest-visible clock never steps backwards.
    const READ_COUNT: usize = 100;

    /// Current system wall-clock time in Unix milliseconds, computed independently of the import.
    fn system_unix_millis() -> u64 {
        let elapsed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("the system clock is after the Unix epoch");
        u64::try_from(elapsed.as_millis()).expect("current time in milliseconds fits in u64")
    }

    /// REQ: none
    #[test]
    fn now_unix_millis_matches_system_clock_in_milliseconds() {
        let mut clock = ClockImport;
        let host_reading = clock.now_unix_millis();
        let system_reading = system_unix_millis();
        let difference = host_reading.abs_diff(system_reading);
        assert!(
            difference <= WALL_CLOCK_TOLERANCE_MILLIS,
            "host wall clock {host_reading} differs from system clock {system_reading} by {difference} ms"
        );
    }

    /// REQ: none
    #[test]
    fn now_unix_millis_is_after_the_unix_epoch_in_millisecond_scale() {
        let mut clock = ClockImport;
        let host_reading = clock.now_unix_millis();
        // 2020-01-01 00:00:00 UTC in milliseconds; a value in seconds or microseconds would fall outside.
        let year_2020_unix_millis: u64 = 1_577_836_800_000;
        assert!(
            host_reading >= year_2020_unix_millis,
            "host wall clock {host_reading} is not a Unix epoch value in milliseconds"
        );
    }

    /// REQ: AUT-007
    #[test]
    fn monotonic_millis_never_goes_backwards_across_repeated_reads() {
        let mut clock = ClockImport;
        let mut previous = clock.monotonic_millis();
        for read_index in 0..READ_COUNT {
            let current = clock.monotonic_millis();
            assert!(
                current >= previous,
                "monotonic clock went backwards at read {read_index}: {previous} -> {current}"
            );
            previous = current;
        }
    }

    /// REQ: AUT-104
    #[test]
    fn monotonic_millis_advances_by_at_least_the_slept_duration() {
        let mut clock = ClockImport;
        let sleep_millis: u64 = 20;
        let started = clock.monotonic_millis();
        thread::sleep(Duration::from_millis(sleep_millis));
        let finished = clock.monotonic_millis();
        let elapsed = finished - started;
        assert!(
            elapsed >= sleep_millis,
            "a {sleep_millis} ms sleep measured only {elapsed} ms on the monotonic clock"
        );
    }

    /// REQ: SEC-347
    #[test]
    fn repeated_monotonic_reads_step_by_at_least_one_millisecond() {
        let mut clock = ClockImport;
        let mut previous = clock.monotonic_millis();
        for read_index in 0..READ_COUNT {
            let current = clock.monotonic_millis();
            let step = current - previous;
            assert!(
                step == 0 || step >= 1,
                "monotonic step {step} ms at read {read_index} is finer than 1 ms"
            );
            if step > 0 {
                previous = current;
            }
        }
    }

    /// REQ: none
    #[test]
    fn monotonic_origin_is_shared_by_separate_import_instances() {
        let mut first_instance = ClockImport;
        let mut second_instance = ClockImport;
        let first_reading = first_instance.monotonic_millis();
        let second_reading = second_instance.monotonic_millis();
        assert!(
            second_reading >= first_reading,
            "a later read from a second instance {second_reading} is behind {first_reading}"
        );
        assert!(
            second_reading - first_reading < 1_000,
            "two back-to-back reads from separate instances are {} ms apart",
            second_reading - first_reading
        );
    }
}

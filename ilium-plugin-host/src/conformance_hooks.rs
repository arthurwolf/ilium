//! Probe imports and budget probes used by `ilium-plugin-conformance` (reference-conformance-suite.md).

/// Busy-waits for the given number of wall-clock milliseconds in the guest (the `busy-wait` probe import; deadlines are epoch-based and fuel is off).
pub fn busy_wait_ms(_millis: u64) {
    unimplemented!("host::conformance_hooks::busy_wait_ms")
}

#[cfg(test)]
mod tests {
    use super::busy_wait_ms;
    use std::time::{Duration, Instant};

    /// Runs `busy_wait_ms` and returns the wall-clock time it took, measured with a monotonic clock.
    fn elapsed_for_busy_wait(millis: u64) -> Duration {
        let started_at = Instant::now();
        busy_wait_ms(millis);
        started_at.elapsed()
    }

    /// REQ: DEV-607
    #[test]
    fn dev_607_zero_millis_returns_without_waiting_for_a_measurable_time() {
        let elapsed = elapsed_for_busy_wait(0);
        assert!(
            elapsed < Duration::from_millis(100),
            "a zero-millisecond busy wait took {elapsed:?}"
        );
    }

    /// REQ: DEV-607
    #[test]
    fn dev_607_one_millisecond_busy_wait_takes_at_least_one_wall_clock_millisecond() {
        let elapsed = elapsed_for_busy_wait(1);
        assert!(
            elapsed >= Duration::from_millis(1),
            "a 1 ms busy wait returned after {elapsed:?}"
        );
    }

    /// REQ: DEV-607
    #[test]
    fn dev_607_busy_wait_does_not_return_before_the_requested_milliseconds_elapse() {
        let elapsed = elapsed_for_busy_wait(25);
        assert!(
            elapsed >= Duration::from_millis(25),
            "a 25 ms busy wait returned after {elapsed:?}"
        );
    }

    /// REQ: DEV-607
    #[test]
    fn dev_607_busy_wait_of_the_deactivate_probe_duration_is_not_cut_short() {
        // CONF-LIFE-03 busy-waits 600 ms in `deactivate` on the probe plugin.
        let elapsed = elapsed_for_busy_wait(600);
        assert!(
            elapsed >= Duration::from_millis(600),
            "a 600 ms busy wait returned after {elapsed:?}"
        );
    }

    /// REQ: DEV-607
    #[test]
    fn dev_607_busy_wait_is_a_real_wall_clock_wait_and_not_a_fixed_no_op() {
        let short_elapsed = elapsed_for_busy_wait(5);
        let longer_elapsed = elapsed_for_busy_wait(50);
        assert!(
            longer_elapsed > short_elapsed,
            "a 50 ms busy wait ({longer_elapsed:?}) was not longer than a 5 ms one ({short_elapsed:?})"
        );
    }
}

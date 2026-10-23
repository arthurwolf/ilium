//! Host implementation of `ilium:host/timers` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostTimers` trait; hand-owned afterwards.

use ilium_plugin_api::generated::host::timers::HostTimers;
use ilium_plugin_api::generated::types::errors::PluginError;
use ilium_plugin_api::generated::types::ids::{EpochMs, TimerId};

/// Host side of the `timers` interface.
#[derive(Debug)]
pub struct TimersImport;

impl HostTimers for TimersImport {
    fn set(
        &mut self,
        _after_millis: u32,
        _repeat_millis: Option<u32>,
    ) -> Result<TimerId, PluginError> {
        unimplemented!("host::imports::timers::set")
    }

    fn cancel(&mut self, _timer: TimerId) {
        unimplemented!("host::imports::timers::cancel")
    }

    fn at(&mut self, _epoch: EpochMs, _name: String) -> Result<TimerId, PluginError> {
        unimplemented!("host::imports::timers::at")
    }

    fn cancel_durable(&mut self, _name: String) {
        unimplemented!("host::imports::timers::cancel_durable")
    }
}

#[cfg(test)]
mod tests {
    use super::TimersImport;
    use ilium_plugin_api::generated::host::timers::HostTimers;
    use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
    use ilium_plugin_api::generated::types::ids::{EpochMs, TimerId};
    use std::time::{SystemTime, UNIX_EPOCH};

    /// Smallest `after-millis` and `repeat-millis` that the host accepts.
    const MINIMUM_INTERVAL_MILLIS: u32 = 50;

    /// Default held ordinary timers per plugin.
    const HELD_TIMER_LIMIT: usize = 64;

    /// A durable timer may be armed up to this far in the past before it is refused.
    const PAST_TOLERANCE_MILLIS: u64 = 1_000;

    /// Returns the current wall-clock time in milliseconds since the Unix epoch.
    fn now_unix_millis() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock is before the Unix epoch")
            .as_millis() as u64
    }

    /// Asserts that a call was refused with the expected error code.
    fn assert_refused<T: std::fmt::Debug>(result: Result<T, PluginError>, expected: ErrorCode) {
        match result {
            Ok(value) => {
                panic!("call must be refused with {expected:?}, but it returned {value:?}")
            }
            Err(error) => assert_eq!(
                error.code, expected,
                "wrong error code, message: {}",
                error.message
            ),
        }
    }

    /// REQ: API-062
    #[test]
    fn set_accepts_exactly_the_minimum_interval() {
        let mut timers = TimersImport;
        let result = timers.set(MINIMUM_INTERVAL_MILLIS, None);
        assert!(result.is_ok(), "50 ms must be accepted, got {result:?}");
    }

    /// REQ: API-062
    #[test]
    fn set_refuses_an_interval_just_below_the_minimum() {
        let mut timers = TimersImport;
        let result = timers.set(MINIMUM_INTERVAL_MILLIS - 1, None);
        assert_refused(result, ErrorCode::InvalidArgument);
    }

    /// REQ: API-062
    #[test]
    fn set_refuses_zero_after_millis() {
        let mut timers = TimersImport;
        let result = timers.set(0, None);
        assert_refused(result, ErrorCode::InvalidArgument);
    }

    /// REQ: API-062
    #[test]
    fn set_refuses_a_repeat_interval_just_below_the_minimum() {
        let mut timers = TimersImport;
        let result = timers.set(MINIMUM_INTERVAL_MILLIS, Some(MINIMUM_INTERVAL_MILLIS - 1));
        assert_refused(result, ErrorCode::InvalidArgument);
    }

    /// REQ: API-062
    #[test]
    fn set_refuses_a_zero_repeat_interval() {
        let mut timers = TimersImport;
        let result = timers.set(MINIMUM_INTERVAL_MILLIS, Some(0));
        assert_refused(result, ErrorCode::InvalidArgument);
    }

    /// REQ: API-062
    #[test]
    fn set_accepts_a_repeat_interval_of_exactly_the_minimum() {
        let mut timers = TimersImport;
        let result = timers.set(MINIMUM_INTERVAL_MILLIS, Some(MINIMUM_INTERVAL_MILLIS));
        assert!(
            result.is_ok(),
            "a 50 ms repeat must be accepted, got {result:?}"
        );
    }

    /// REQ: API-061
    #[test]
    fn set_refuses_the_65th_held_timer_with_quota_exceeded() {
        let mut timers = TimersImport;
        for index in 0..HELD_TIMER_LIMIT {
            let result = timers.set(MINIMUM_INTERVAL_MILLIS, None);
            assert!(
                result.is_ok(),
                "timer {index} of {HELD_TIMER_LIMIT} must be accepted, got {result:?}"
            );
        }
        let result = timers.set(MINIMUM_INTERVAL_MILLIS, None);
        assert_refused(result, ErrorCode::QuotaExceeded);
    }

    /// REQ: API-074
    #[test]
    fn cancel_of_an_unknown_timer_id_is_a_silent_no_op() {
        let mut timers = TimersImport;
        timers.cancel(TimerId(u64::MAX));
    }

    /// REQ: API-074
    #[test]
    fn cancel_of_an_already_cancelled_timer_is_a_silent_no_op() {
        let mut timers = TimersImport;
        let timer = timers
            .set(MINIMUM_INTERVAL_MILLIS, None)
            .expect("a 50 ms timer must be armed before it is cancelled");
        timers.cancel(timer);
        timers.cancel(timer);
    }

    /// REQ: API-060
    #[test]
    fn at_refuses_an_epoch_more_than_one_second_in_the_past() {
        let mut timers = TimersImport;
        let epoch = now_unix_millis() - PAST_TOLERANCE_MILLIS - 1_000;
        let result = timers.at(EpochMs(epoch), "stale-job".to_string());
        assert_refused(result, ErrorCode::InvalidArgument);
    }

    /// REQ: API-060
    #[test]
    fn at_refuses_epoch_zero() {
        let mut timers = TimersImport;
        let result = timers.at(EpochMs(0), "epoch-zero-job".to_string());
        assert_refused(result, ErrorCode::InvalidArgument);
    }

    /// REQ: API-060
    #[test]
    fn at_accepts_an_epoch_up_to_one_second_in_the_past() {
        let mut timers = TimersImport;
        let epoch = now_unix_millis() - PAST_TOLERANCE_MILLIS / 2;
        let result = timers.at(EpochMs(epoch), "recent-job".to_string());
        assert!(
            result.is_ok(),
            "an epoch 500 ms in the past must be accepted, got {result:?}"
        );
    }

    /// REQ: API-060
    #[test]
    fn at_accepts_an_epoch_in_the_future() {
        let mut timers = TimersImport;
        let epoch = now_unix_millis() + 60_000;
        let result = timers.at(EpochMs(epoch), "future-job".to_string());
        assert!(
            result.is_ok(),
            "an epoch one minute ahead must be accepted, got {result:?}"
        );
    }

    /// REQ: API-061
    #[test]
    fn at_refuses_the_65th_distinct_durable_timer_with_quota_exceeded() {
        let mut timers = TimersImport;
        let epoch = now_unix_millis() + 60_000;
        for index in 0..HELD_TIMER_LIMIT {
            let result = timers.at(EpochMs(epoch), format!("job-{index}"));
            assert!(
                result.is_ok(),
                "durable timer {index} of {HELD_TIMER_LIMIT} must be accepted, got {result:?}"
            );
        }
        let result = timers.at(EpochMs(epoch), format!("job-{HELD_TIMER_LIMIT}"));
        assert_refused(result, ErrorCode::QuotaExceeded);
    }

    /// REQ: API-061
    #[test]
    fn at_with_an_existing_name_replaces_the_timer_and_does_not_use_a_new_slot() {
        let mut timers = TimersImport;
        let epoch = now_unix_millis() + 60_000;
        for index in 0..HELD_TIMER_LIMIT {
            let result = timers.at(EpochMs(epoch), format!("job-{index}"));
            assert!(
                result.is_ok(),
                "durable timer {index} of {HELD_TIMER_LIMIT} must be accepted, got {result:?}"
            );
        }
        let result = timers.at(EpochMs(epoch + 1_000), "job-0".to_string());
        assert!(
            result.is_ok(),
            "re-arming an existing name must not hit the quota, got {result:?}"
        );
    }

    /// REQ: API-074
    #[test]
    fn cancel_durable_of_an_unknown_name_is_a_silent_no_op() {
        let mut timers = TimersImport;
        timers.cancel_durable("never-armed".to_string());
    }

    /// REQ: API-061
    #[test]
    fn cancel_durable_frees_a_slot_for_a_new_durable_timer() {
        let mut timers = TimersImport;
        let epoch = now_unix_millis() + 60_000;
        for index in 0..HELD_TIMER_LIMIT {
            let result = timers.at(EpochMs(epoch), format!("job-{index}"));
            assert!(
                result.is_ok(),
                "durable timer {index} of {HELD_TIMER_LIMIT} must be accepted, got {result:?}"
            );
        }
        timers.cancel_durable("job-0".to_string());
        let result = timers.at(EpochMs(epoch), "job-new".to_string());
        assert!(
            result.is_ok(),
            "a cancelled durable timer must free its slot, got {result:?}"
        );
    }
}

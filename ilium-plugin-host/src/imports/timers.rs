//! Host implementation of `ilium:host/timers`.
//!
//! Hand-owned. Each import holds a [`TimerBook`]: the timers of one plugin instance. The runtime
//! keeps a second handle on the book and calls [`take_due`] from its pump to learn which timers
//! fired; this import never delivers anything itself.

use ilium_plugin_api::generated::host::timers::HostTimers;
use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
use ilium_plugin_api::generated::types::ids::{EpochMs, TimerId};
use ilium_plugin_api::generated::types::{DurableTimerEvent, Event};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Smallest `after-millis` and `repeat-millis` the host accepts (API-062).
const MINIMUM_INTERVAL_MILLIS: u32 = 50;

/// Held ordinary timers, and separately held durable timers, per plugin (API-061).
const HELD_TIMER_LIMIT: usize = 64;

/// A durable timer may be armed up to this far in the past before it is refused (API-060).
const PAST_TOLERANCE_MILLIS: u64 = 1_000;

/// One held ordinary timer.
#[derive(Debug)]
struct Armed {
    due: Instant,
    repeat: Option<Duration>,
}

/// One held durable timer.
#[derive(Debug)]
struct Durable {
    id: u64,
    epoch_millis: u64,
}

/// The timers one plugin instance holds.
#[derive(Debug, Default)]
pub struct TimerState {
    next_id: u64,
    held: HashMap<u64, Armed>,
    durable: HashMap<String, Durable>,
}

/// The timers of one plugin instance, shared by its import and the runtime that fires them.
pub type TimerBook = Arc<Mutex<TimerState>>;

/// Locks a book. A poisoned lock is used as it is: the book holds plain bookkeeping that a
/// panic elsewhere cannot leave half-written in a harmful way.
fn locked(book: &TimerBook) -> MutexGuard<'_, TimerState> {
    book.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Removes the timers that are due and returns their events: ordinary timers in id order, then
/// durable timers in name order. A repeating timer stays held and is moved to its next
/// interval after `now`, so a late pump fires it once, not once for each missed interval.
pub fn take_due(book: &TimerBook, now: Instant, now_unix_millis: u64) -> Vec<Event> {
    let mut state = locked(book);
    let mut due: Vec<u64> = state
        .held
        .iter()
        .filter(|(_, armed)| armed.due <= now)
        .map(|(id, _)| *id)
        .collect();
    due.sort_unstable();
    let mut events = Vec::with_capacity(due.len());
    for id in due {
        let next = state.held.get(&id).and_then(|armed| {
            let repeat = armed.repeat.filter(|repeat| !repeat.is_zero())?;
            let mut next = armed.due + repeat;
            if next <= now {
                next = now + repeat;
            }
            Some((next, repeat))
        });
        match next {
            Some((due, repeat)) => {
                state.held.insert(
                    id,
                    Armed {
                        due,
                        repeat: Some(repeat),
                    },
                );
            }
            None => {
                state.held.remove(&id);
            }
        }
        events.push(Event::TimerFired(TimerId(id)));
    }
    let mut names: Vec<String> = state
        .durable
        .iter()
        .filter(|(_, durable)| durable.epoch_millis <= now_unix_millis)
        .map(|(name, _)| name.clone())
        .collect();
    names.sort_unstable();
    for name in names {
        if let Some(durable) = state.durable.remove(&name) {
            events.push(Event::DurableTimerFired(DurableTimerEvent {
                timer: TimerId(durable.id),
                name,
                late_by_millis: now_unix_millis.saturating_sub(durable.epoch_millis),
            }));
        }
    }
    events
}

/// How often a book that holds durable timers is looked at: they follow the wall clock, which
/// can jump, so they are polled instead of slept on.
const DURABLE_POLL: Duration = Duration::from_secs(1);

/// When a book next needs `take_due`, for a host that sleeps between pumps: the earliest
/// ordinary timer, or one poll interval from now when a durable timer is held. `None` when
/// the book holds no timer.
pub fn next_due(book: &TimerBook) -> Option<Instant> {
    let state = locked(book);
    let ordinary = state.held.values().map(|armed| armed.due).min();
    let durable = (!state.durable.is_empty()).then(|| Instant::now() + DURABLE_POLL);
    match (ordinary, durable) {
        (Some(ordinary), Some(durable)) => Some(ordinary.min(durable)),
        (ordinary, durable) => ordinary.or(durable),
    }
}

/// Builds a timers error with no retry delay.
fn timer_error(code: ErrorCode, message: &str) -> PluginError {
    PluginError {
        code,
        message: message.to_string(),
        retry_after_ms: None,
    }
}

/// Current wall-clock time in Unix milliseconds.
fn now_unix_millis() -> u64 {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
}

/// Host side of the `timers` interface. `default()` gives an import with a book of its own.
#[derive(Debug, Default)]
pub struct TimersImport {
    book: TimerBook,
}

impl TimersImport {
    /// An import that keeps its timers in a book the caller also holds.
    pub fn with_book(book: TimerBook) -> Self {
        Self { book }
    }
}

impl HostTimers for TimersImport {
    fn set(
        &mut self,
        after_millis: u32,
        repeat_millis: Option<u32>,
    ) -> Result<TimerId, PluginError> {
        if after_millis < MINIMUM_INTERVAL_MILLIS {
            return Err(timer_error(
                ErrorCode::InvalidArgument,
                "after-millis is below the 50 ms minimum",
            ));
        }
        if repeat_millis.is_some_and(|repeat| repeat < MINIMUM_INTERVAL_MILLIS) {
            return Err(timer_error(
                ErrorCode::InvalidArgument,
                "repeat-millis is below the 50 ms minimum",
            ));
        }
        let mut state = locked(&self.book);
        if state.held.len() >= HELD_TIMER_LIMIT {
            return Err(timer_error(
                ErrorCode::QuotaExceeded,
                "the plugin already holds 64 timers",
            ));
        }
        state.next_id += 1;
        let id = state.next_id;
        state.held.insert(
            id,
            Armed {
                due: Instant::now() + Duration::from_millis(u64::from(after_millis)),
                repeat: repeat_millis.map(|repeat| Duration::from_millis(u64::from(repeat))),
            },
        );
        Ok(TimerId(id))
    }

    fn cancel(&mut self, timer: TimerId) {
        locked(&self.book).held.remove(&timer.0);
    }

    fn at(&mut self, epoch: EpochMs, name: String) -> Result<TimerId, PluginError> {
        if name.is_empty() {
            return Err(timer_error(
                ErrorCode::InvalidArgument,
                "a durable timer needs a name",
            ));
        }
        if epoch.0 == 0 || epoch.0.saturating_add(PAST_TOLERANCE_MILLIS) < now_unix_millis() {
            return Err(timer_error(
                ErrorCode::InvalidArgument,
                "the epoch is more than one second in the past",
            ));
        }
        let mut state = locked(&self.book);
        if !state.durable.contains_key(&name) && state.durable.len() >= HELD_TIMER_LIMIT {
            return Err(timer_error(
                ErrorCode::QuotaExceeded,
                "the plugin already holds 64 durable timers",
            ));
        }
        state.next_id += 1;
        let id = state.next_id;
        state.durable.insert(
            name,
            Durable {
                id,
                epoch_millis: epoch.0,
            },
        );
        Ok(TimerId(id))
    }

    fn cancel_durable(&mut self, name: String) {
        locked(&self.book).durable.remove(&name);
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
        let mut timers = TimersImport::default();
        let result = timers.set(MINIMUM_INTERVAL_MILLIS, None);
        assert!(result.is_ok(), "50 ms must be accepted, got {result:?}");
    }

    /// REQ: API-062
    #[test]
    fn set_refuses_an_interval_just_below_the_minimum() {
        let mut timers = TimersImport::default();
        let result = timers.set(MINIMUM_INTERVAL_MILLIS - 1, None);
        assert_refused(result, ErrorCode::InvalidArgument);
    }

    /// REQ: API-062
    #[test]
    fn set_refuses_zero_after_millis() {
        let mut timers = TimersImport::default();
        let result = timers.set(0, None);
        assert_refused(result, ErrorCode::InvalidArgument);
    }

    /// REQ: API-062
    #[test]
    fn set_refuses_a_repeat_interval_just_below_the_minimum() {
        let mut timers = TimersImport::default();
        let result = timers.set(MINIMUM_INTERVAL_MILLIS, Some(MINIMUM_INTERVAL_MILLIS - 1));
        assert_refused(result, ErrorCode::InvalidArgument);
    }

    /// REQ: API-062
    #[test]
    fn set_refuses_a_zero_repeat_interval() {
        let mut timers = TimersImport::default();
        let result = timers.set(MINIMUM_INTERVAL_MILLIS, Some(0));
        assert_refused(result, ErrorCode::InvalidArgument);
    }

    /// REQ: API-062
    #[test]
    fn set_accepts_a_repeat_interval_of_exactly_the_minimum() {
        let mut timers = TimersImport::default();
        let result = timers.set(MINIMUM_INTERVAL_MILLIS, Some(MINIMUM_INTERVAL_MILLIS));
        assert!(
            result.is_ok(),
            "a 50 ms repeat must be accepted, got {result:?}"
        );
    }

    /// REQ: API-061
    #[test]
    fn set_refuses_the_65th_held_timer_with_quota_exceeded() {
        let mut timers = TimersImport::default();
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
        let mut timers = TimersImport::default();
        timers.cancel(TimerId(u64::MAX));
    }

    /// REQ: API-074
    #[test]
    fn cancel_of_an_already_cancelled_timer_is_a_silent_no_op() {
        let mut timers = TimersImport::default();
        let timer = timers
            .set(MINIMUM_INTERVAL_MILLIS, None)
            .expect("a 50 ms timer must be armed before it is cancelled");
        timers.cancel(timer);
        timers.cancel(timer);
    }

    /// REQ: API-060
    #[test]
    fn at_refuses_an_epoch_more_than_one_second_in_the_past() {
        let mut timers = TimersImport::default();
        let epoch = now_unix_millis() - PAST_TOLERANCE_MILLIS - 1_000;
        let result = timers.at(EpochMs(epoch), "stale-job".to_string());
        assert_refused(result, ErrorCode::InvalidArgument);
    }

    /// REQ: API-060
    #[test]
    fn at_refuses_epoch_zero() {
        let mut timers = TimersImport::default();
        let result = timers.at(EpochMs(0), "epoch-zero-job".to_string());
        assert_refused(result, ErrorCode::InvalidArgument);
    }

    /// REQ: API-060
    #[test]
    fn at_accepts_an_epoch_up_to_one_second_in_the_past() {
        let mut timers = TimersImport::default();
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
        let mut timers = TimersImport::default();
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
        let mut timers = TimersImport::default();
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
        let mut timers = TimersImport::default();
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
        let mut timers = TimersImport::default();
        timers.cancel_durable("never-armed".to_string());
    }

    /// REQ: API-061
    #[test]
    fn cancel_durable_frees_a_slot_for_a_new_durable_timer() {
        let mut timers = TimersImport::default();
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

    #[test]
    fn take_due_fires_a_one_shot_timer_once_and_frees_its_slot() {
        let book = super::TimerBook::default();
        let mut timers = TimersImport::with_book(std::sync::Arc::clone(&book));
        let timer = timers.set(50, None).expect("armed");
        let start = std::time::Instant::now();
        assert!(super::take_due(&book, start, 0).is_empty());
        let later = start + std::time::Duration::from_secs(1);
        assert_eq!(
            super::take_due(&book, later, 0),
            vec![ilium_plugin_api::generated::types::Event::TimerFired(timer)]
        );
        assert!(super::take_due(&book, later, 0).is_empty());
        assert_eq!(super::next_due(&book), None);
    }

    #[test]
    fn take_due_fires_a_late_repeating_timer_once_and_keeps_it() {
        let book = super::TimerBook::default();
        let mut timers = TimersImport::with_book(std::sync::Arc::clone(&book));
        let timer = timers.set(50, Some(50)).expect("armed");
        let late = std::time::Instant::now() + std::time::Duration::from_secs(10);
        assert_eq!(
            super::take_due(&book, late, 0),
            vec![ilium_plugin_api::generated::types::Event::TimerFired(timer)]
        );
        assert!(super::take_due(&book, late, 0).is_empty());
        assert!(super::next_due(&book).is_some_and(|due| due > late));
    }

    #[test]
    fn a_cancelled_timer_never_fires() {
        let book = super::TimerBook::default();
        let mut timers = TimersImport::with_book(std::sync::Arc::clone(&book));
        let timer = timers.set(50, None).expect("armed");
        timers.cancel(timer);
        let later = std::time::Instant::now() + std::time::Duration::from_secs(1);
        assert!(super::take_due(&book, later, 0).is_empty());
    }

    #[test]
    fn take_due_fires_a_durable_timer_with_its_lateness() {
        let book = super::TimerBook::default();
        let mut timers = TimersImport::with_book(std::sync::Arc::clone(&book));
        let epoch = now_unix_millis() + 60_000;
        timers
            .at(EpochMs(epoch), "nightly".to_string())
            .expect("armed");
        let now = std::time::Instant::now();
        assert!(super::take_due(&book, now, epoch - 1).is_empty());
        let events = super::take_due(&book, now, epoch + 250);
        match events.as_slice() {
            [ilium_plugin_api::generated::types::Event::DurableTimerFired(fired)] => {
                assert_eq!(fired.name, "nightly");
                assert_eq!(fired.late_by_millis, 250);
            }
            other => panic!("expected one durable timer event, got {other:?}"),
        }
        assert!(super::take_due(&book, now, epoch + 500).is_empty());
    }
}

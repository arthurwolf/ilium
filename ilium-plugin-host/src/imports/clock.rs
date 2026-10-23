//! Always-available wall and monotonic clocks (api-clock-and-timers.md).
use std::{sync::OnceLock, time::{Duration, Instant, SystemTime, UNIX_EPOCH}};

use ilium_plugin_api::generated::host::clock::HostClock;

// One fixed origin per host process, lazily established by the first monotonic
// read. Constructing/reloading a plugin never resets the host's duration clock.
static MONOTONIC_ORIGIN: OnceLock<Instant> = OnceLock::new();

/// Host side of the grant-free `clock` interface on every plugin side.
#[derive(Debug, Default, Clone, Copy)]
pub struct ClockImport;

/// Convert a wall-clock observation to the unsigned ABI representation.
/// Dates before the epoch clamp to zero; unrepresentable millisecond counts
/// saturate instead of wrapping. Wall-clock corrections remain observable.
pub fn unix_millis_at(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH).map(duration_millis).unwrap_or(0)
}

/// Millisecond conversion shared by both clocks, including truncation of
/// submillisecond fractions and saturation at the ABI's maximum integer.
pub fn duration_millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

impl HostClock for ClockImport {
    fn now_unix_millis(&mut self) -> u64 {
        unix_millis_at(SystemTime::now())
    }

    fn monotonic_millis(&mut self) -> u64 {
        duration_millis(MONOTONIC_ORIGIN.get_or_init(Instant::now).elapsed())
    }
}

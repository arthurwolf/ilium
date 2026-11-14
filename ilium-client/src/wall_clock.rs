//! The client's wall clock, in the unit the server and the persisted state use.

use std::time::{SystemTime, UNIX_EPOCH};

/// Milliseconds since the Unix epoch. A pre-epoch clock falls back to zero so countdowns and
/// freshness checks stay well defined.
pub fn unix_millis_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
        .unwrap_or(0)
}

//! Numeric-only startup diagnostics. Wall time correlates processes; monotonic
//! time measures intervals inside one process without wall-clock adjustments.
use std::sync::OnceLock;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

pub fn record(
    stage: &'static str,
    request_id: u64,
    pane_id: u64,
    child_process_id: u32,
    sequence: u64,
) {
    if !tracing::enabled!(target: "ilium_startup", tracing::Level::DEBUG) {
        return;
    }
    record_at(
        stage,
        request_id,
        pane_id,
        child_process_id,
        sequence,
        Instant::now(),
    );
}

/// `occurred_at` is the successful emission instant, not the later UI acknowledgement.
pub fn record_at(
    stage: &'static str,
    request_id: u64,
    pane_id: u64,
    child_process_id: u32,
    sequence: u64,
    occurred_at: Instant,
) {
    if !tracing::enabled!(target: "ilium_startup", tracing::Level::DEBUG) {
        return;
    }
    static ORIGIN: OnceLock<Instant> = OnceLock::new();
    let origin = ORIGIN.get_or_init(|| occurred_at);
    let monotonic_us = occurred_at
        .saturating_duration_since(*origin)
        .as_micros()
        .min(u64::MAX as u128) as u64;
    let age = Instant::now().saturating_duration_since(occurred_at);
    let unix_us = SystemTime::now()
        .checked_sub(age)
        .unwrap_or(UNIX_EPOCH)
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros()
        .min(u64::MAX as u128) as u64;
    tracing::debug!(target: "ilium_startup", stage, request_id, pane_id, child_process_id, sequence,
        process_id = std::process::id(), monotonic_us, unix_us, "startup timing");
}

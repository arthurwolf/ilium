//! Daily summaries that keep call history beyond the 24-hour bucket window (HST-524, HST-525).
//! Skeleton: every function with behaviour is `unimplemented!` until the implementation phase.

/// One hourly bucket of a plugin side (reference-host-telemetry.md).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HourlyBucket {
    pub plugin_side: String,
    pub hour_index: u64,
    pub calls: u64,
    pub failures: u64,
    pub near_misses: u64,
    pub call_times_ms: Vec<u32>,
}

/// One daily summary of a plugin side: counts and the 50th, 95th and 99th percentile of call
/// time (HST-524).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DailySummary {
    pub plugin_side: String,
    pub day_index: u64,
    pub calls: u64,
    pub failures: u64,
    pub near_misses: u64,
    pub p50_ms: u32,
    pub p95_ms: u32,
    pub p99_ms: u32,
}

/// Folds hourly buckets into daily summaries and keeps the last 30 (HST-524).
#[derive(Debug, Default)]
pub struct TelemetryHistory;

impl TelemetryHistory {
    /// An empty history.
    pub fn new() -> TelemetryHistory {
        unimplemented!("host::telemetry_history::TelemetryHistory::new")
    }

    /// Records one hourly bucket. A bucket that leaves the 24-hour window is folded into its
    /// day's summary (HST-524).
    pub fn record_hour(&mut self, _bucket: HourlyBucket) {
        unimplemented!("host::telemetry_history::TelemetryHistory::record_hour")
    }

    /// Marks a day on which the plugin was upgraded (HST-525).
    pub fn record_upgrade(&mut self, _plugin_id: &str, _day_index: u64) {
        unimplemented!("host::telemetry_history::TelemetryHistory::record_upgrade")
    }

    /// The daily summaries kept for one plugin side, oldest first (HST-524).
    pub fn daily_summaries(&self, _plugin_side: &str) -> Vec<DailySummary> {
        unimplemented!("host::telemetry_history::TelemetryHistory::daily_summaries")
    }

    /// Bytes the most recent fold added for one plugin side (HST-524).
    pub fn bytes_added_by_last_fold(&self, _plugin_side: &str) -> usize {
        unimplemented!("host::telemetry_history::TelemetryHistory::bytes_added_by_last_fold")
    }

    /// Removes all summaries of a plugin, as uninstall does (HST-525).
    pub fn remove_plugin(&mut self, _plugin_id: &str) {
        unimplemented!("host::telemetry_history::TelemetryHistory::remove_plugin")
    }

    /// The output of `ilium plugin status <plugin-id> --days <n>` (HST-525).
    pub fn status_days_output(&self, _plugin_id: &str, _days: u32) -> String {
        unimplemented!("host::telemetry_history::TelemetryHistory::status_days_output")
    }
}

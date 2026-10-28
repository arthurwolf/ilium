//! Daily summaries that keep call history beyond the 24-hour bucket window (HST-524, HST-525).

use std::collections::{BTreeMap, BTreeSet};

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

/// Days of daily summaries kept per plugin side (HST-524).
const DAYS_KEPT: usize = 30;
const HOURS_PER_DAY: u64 = 24;

/// Nearest-rank percentile of an ascending-sorted slice.
fn percentile(sorted: &[u32], percent: usize) -> u32 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = (sorted.len() * percent).div_ceil(100).max(1);
    sorted[rank - 1]
}

/// Folds hourly buckets into daily summaries and keeps the last 30 (HST-524).
#[derive(Debug, Default)]
pub struct TelemetryHistory {
    pending: BTreeMap<String, Vec<HourlyBucket>>,
    summaries: BTreeMap<String, Vec<DailySummary>>,
    last_fold_bytes: BTreeMap<String, usize>,
    upgrades: BTreeMap<String, BTreeSet<u64>>,
}

impl TelemetryHistory {
    /// An empty history.
    pub fn new() -> TelemetryHistory {
        TelemetryHistory::default()
    }

    /// Records one hourly bucket. A bucket that leaves the 24-hour window is folded into its
    /// day's summary (HST-524).
    pub fn record_hour(&mut self, bucket: HourlyBucket) {
        let side = bucket.plugin_side.clone();
        let pending = self.pending.entry(side.clone()).or_default();
        pending.push(bucket);
        let current_day = pending.iter().map(|b| b.hour_index).max().unwrap_or(0) / HOURS_PER_DAY;
        let mut days_to_fold: Vec<u64> = pending
            .iter()
            .map(|b| b.hour_index / HOURS_PER_DAY)
            .filter(|day| *day < current_day)
            .collect();
        days_to_fold.sort_unstable();
        days_to_fold.dedup();
        for day in days_to_fold {
            let (folded, kept): (Vec<_>, Vec<_>) = std::mem::take(pending)
                .into_iter()
                .partition(|b| b.hour_index / HOURS_PER_DAY == day);
            *pending = kept;
            let summary = Self::fold_day(&side, day, &folded);
            let list = self.summaries.entry(side.clone()).or_default();
            let added = std::mem::size_of::<DailySummary>() + summary.plugin_side.len();
            list.push(summary);
            list.sort_by_key(|entry| entry.day_index);
            if list.len() > DAYS_KEPT {
                let excess = list.len() - DAYS_KEPT;
                list.drain(..excess);
            }
            self.last_fold_bytes.insert(side.clone(), added);
        }
    }

    fn fold_day(side: &str, day: u64, buckets: &[HourlyBucket]) -> DailySummary {
        let mut times: Vec<u32> = buckets
            .iter()
            .flat_map(|b| b.call_times_ms.iter().copied())
            .collect();
        times.sort_unstable();
        DailySummary {
            plugin_side: side.to_string(),
            day_index: day,
            calls: buckets.iter().map(|b| b.calls).sum(),
            failures: buckets.iter().map(|b| b.failures).sum(),
            near_misses: buckets.iter().map(|b| b.near_misses).sum(),
            p50_ms: percentile(&times, 50),
            p95_ms: percentile(&times, 95),
            p99_ms: percentile(&times, 99),
        }
    }

    /// Marks a day on which the plugin was upgraded (HST-525).
    pub fn record_upgrade(&mut self, plugin_id: &str, day_index: u64) {
        self.upgrades
            .entry(plugin_id.to_string())
            .or_default()
            .insert(day_index);
    }

    /// The daily summaries kept for one plugin side, oldest first (HST-524).
    pub fn daily_summaries(&self, plugin_side: &str) -> Vec<DailySummary> {
        self.summaries.get(plugin_side).cloned().unwrap_or_default()
    }

    /// Bytes the most recent fold added for one plugin side (HST-524).
    pub fn bytes_added_by_last_fold(&self, plugin_side: &str) -> usize {
        self.last_fold_bytes.get(plugin_side).copied().unwrap_or(0)
    }

    fn side_belongs_to(side: &str, plugin_id: &str) -> bool {
        side.strip_prefix(plugin_id)
            .is_some_and(|rest| rest.starts_with('/'))
    }

    /// Removes all summaries of a plugin, as uninstall does (HST-525).
    pub fn remove_plugin(&mut self, plugin_id: &str) {
        self.pending.retain(|side, _| !Self::side_belongs_to(side, plugin_id));
        self.summaries.retain(|side, _| !Self::side_belongs_to(side, plugin_id));
        self.last_fold_bytes.retain(|side, _| !Self::side_belongs_to(side, plugin_id));
        self.upgrades.remove(plugin_id);
    }

    /// The output of `ilium plugin status <plugin-id> --days <n>` (HST-525): one line per day,
    /// the upgrade day marked, and a last line with the p95 trend.
    pub fn status_days_output(&self, plugin_id: &str, days: u32) -> String {
        // day -> (calls, failures, near misses, p50, p95, p99), worst percentile over the sides.
        let mut per_day: BTreeMap<u64, (u64, u64, u64, u32, u32, u32)> = BTreeMap::new();
        for (side, list) in &self.summaries {
            if !Self::side_belongs_to(side, plugin_id) {
                continue;
            }
            for summary in list {
                let row = per_day.entry(summary.day_index).or_default();
                row.0 += summary.calls;
                row.1 += summary.failures;
                row.2 += summary.near_misses;
                row.3 = row.3.max(summary.p50_ms);
                row.4 = row.4.max(summary.p95_ms);
                row.5 = row.5.max(summary.p99_ms);
            }
        }
        let skip = per_day.len().saturating_sub(days as usize);
        let shown: Vec<_> = per_day.into_iter().skip(skip).collect();
        let upgrade_days = self.upgrades.get(plugin_id);
        let mut output = String::new();
        for (day, (calls, failures, near_misses, p50, p95, p99)) in &shown {
            let marker = if upgrade_days.is_some_and(|set| set.contains(day)) {
                "  <- upgrade"
            } else {
                ""
            };
            output.push_str(&format!(
                "day {day}: calls {calls} failures {failures} near-misses {near_misses} p50 {p50} ms p95 {p95} ms p99 {p99} ms{marker}\n"
            ));
        }
        let trend = match (shown.first(), shown.last()) {
            (Some(first), Some(last)) if last.1 .4 > first.1 .4 => "rising",
            (Some(first), Some(last)) if last.1 .4 < first.1 .4 => "falling",
            _ => "flat",
        };
        output.push_str(&format!("p95 trend: {trend}"));
        output
    }
}

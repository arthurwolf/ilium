//! Counts and log gates for coalesced, resynced and overload-dropped plugin traffic
//! (HST-522, HST-523).

use std::collections::BTreeMap;

const LOG_INTERVAL_MS: u64 = 60_000;
const NOTICE_AFTER_MS: u64 = 10_000;

/// Per plugin and per client counts, with the time of the latest occurrence (HST-522).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TrafficCounts {
    pub coalesced: u64,
    pub resyncs: u64,
    pub overload_disconnects: u64,
    pub latest_at_ms: Option<u64>,
}

/// Records the traffic counts of each plugin and client (HST-522).
#[derive(Debug, Default)]
pub struct TrafficLedger {
    counts: BTreeMap<(String, String), TrafficCounts>,
}

impl TrafficLedger {
    /// An empty ledger.
    pub fn new() -> TrafficLedger {
        TrafficLedger::default()
    }

    fn entry(&mut self, plugin_id: &str, client_id: &str, at_ms: u64) -> &mut TrafficCounts {
        let counts = self
            .counts
            .entry((plugin_id.to_owned(), client_id.to_owned()))
            .or_default();
        counts.latest_at_ms = Some(counts.latest_at_ms.map_or(at_ms, |latest| latest.max(at_ms)));
        counts
    }

    /// Counts one coalesced update.
    pub fn record_coalesced(&mut self, plugin_id: &str, client_id: &str, at_ms: u64) {
        let counts = self.entry(plugin_id, client_id, at_ms);
        counts.coalesced = counts.coalesced.saturating_add(1);
    }

    /// Counts one flagged resync.
    pub fn record_resync(&mut self, plugin_id: &str, client_id: &str, at_ms: u64) {
        let counts = self.entry(plugin_id, client_id, at_ms);
        counts.resyncs = counts.resyncs.saturating_add(1);
    }

    /// Counts one client disconnected because its queue overflowed.
    pub fn record_overload_disconnect(&mut self, plugin_id: &str, client_id: &str, at_ms: u64) {
        let counts = self.entry(plugin_id, client_id, at_ms);
        counts.overload_disconnects = counts.overload_disconnects.saturating_add(1);
    }

    /// The counts of one plugin on one client.
    pub fn counts(&self, plugin_id: &str, client_id: &str) -> TrafficCounts {
        self.counts
            .get(&(plugin_id.to_owned(), client_id.to_owned()))
            .copied()
            .unwrap_or_default()
    }
}

/// Admits at most one `ipc.plugin-coalesced` log line per minute per plugin and client
/// (HST-523).
#[derive(Debug, Default)]
pub struct CoalesceLogGate {
    last_logged_ms: BTreeMap<(String, String), u64>,
}

impl CoalesceLogGate {
    /// A gate that has logged nothing yet.
    pub fn new() -> CoalesceLogGate {
        CoalesceLogGate::default()
    }

    /// True when a line may be written at `now_ms` for this plugin and client.
    pub fn should_log(&mut self, plugin_id: &str, client_id: &str, now_ms: u64) -> bool {
        let key = (plugin_id.to_owned(), client_id.to_owned());
        let due = match self.last_logged_ms.get(&key) {
            Some(last) => now_ms.saturating_sub(*last) >= LOG_INTERVAL_MS,
            None => true,
        };
        if due {
            self.last_logged_ms.insert(key, now_ms);
        }
        due
    }
}

/// The plugin with the largest queued share, named in an `ipc.client-too-slow` disconnect
/// (HST-523). `None` for an empty list.
pub fn overload_disconnect_culprit(queued_bytes_by_plugin: &[(String, u64)]) -> Option<String> {
    queued_bytes_by_plugin
        .iter()
        .max_by_key(|(_, queued)| *queued)
        .map(|(plugin_id, _)| plugin_id.clone())
}

/// True when traffic has been coalesced for more than 10 s without a break, which is when the
/// user interface shows its notice (HST-523).
pub fn coalesced_notice_due(coalesced_since_ms: u64, now_ms: u64) -> bool {
    now_ms.saturating_sub(coalesced_since_ms) > NOTICE_AFTER_MS
}

//! Counts and log gates for coalesced, resynced and overload-dropped plugin traffic
//! (HST-522, HST-523). Skeleton: every function with behaviour is `unimplemented!` until the
//! implementation phase.

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
pub struct TrafficLedger;

impl TrafficLedger {
    /// An empty ledger.
    pub fn new() -> TrafficLedger {
        unimplemented!("host::ipc_traffic::TrafficLedger::new")
    }

    /// Counts one coalesced update.
    pub fn record_coalesced(&mut self, _plugin_id: &str, _client_id: &str, _at_ms: u64) {
        unimplemented!("host::ipc_traffic::TrafficLedger::record_coalesced")
    }

    /// Counts one flagged resync.
    pub fn record_resync(&mut self, _plugin_id: &str, _client_id: &str, _at_ms: u64) {
        unimplemented!("host::ipc_traffic::TrafficLedger::record_resync")
    }

    /// Counts one client disconnected because its queue overflowed.
    pub fn record_overload_disconnect(&mut self, _plugin_id: &str, _client_id: &str, _at_ms: u64) {
        unimplemented!("host::ipc_traffic::TrafficLedger::record_overload_disconnect")
    }

    /// The counts of one plugin on one client.
    pub fn counts(&self, _plugin_id: &str, _client_id: &str) -> TrafficCounts {
        unimplemented!("host::ipc_traffic::TrafficLedger::counts")
    }
}

/// Admits at most one `ipc.plugin-coalesced` log line per minute per plugin and client
/// (HST-523).
#[derive(Debug, Default)]
pub struct CoalesceLogGate;

impl CoalesceLogGate {
    /// A gate that has logged nothing yet.
    pub fn new() -> CoalesceLogGate {
        unimplemented!("host::ipc_traffic::CoalesceLogGate::new")
    }

    /// True when a line may be written at `now_ms` for this plugin and client.
    pub fn should_log(&mut self, _plugin_id: &str, _client_id: &str, _now_ms: u64) -> bool {
        unimplemented!("host::ipc_traffic::CoalesceLogGate::should_log")
    }
}

/// The plugin with the largest queued share, named in an `ipc.client-too-slow` disconnect
/// (HST-523). `None` for an empty list.
pub fn overload_disconnect_culprit(_queued_bytes_by_plugin: &[(String, u64)]) -> Option<String> {
    unimplemented!("host::ipc_traffic::overload_disconnect_culprit")
}

/// True when traffic has been coalesced for more than 10 s without a break, which is when the
/// user interface shows its notice (HST-523).
pub fn coalesced_notice_due(_coalesced_since_ms: u64, _now_ms: u64) -> bool {
    unimplemented!("host::ipc_traffic::coalesced_notice_due")
}

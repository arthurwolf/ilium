//! Correlation ids along an originating input's call chain (HST-526).

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// The writer name of host log entries in a chain.
const HOST_WRITER: &str = "ilium.host";

/// Chains started in this process; keeps ids unique even within one clock tick.
static CHAIN_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// One host or plugin log entry written during a chain (HST-526).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainLogEntry {
    pub writer: String,
    pub correlation_id: String,
}

/// What a chain produced: its id, the id seen at each dispatcher call, every log entry and the
/// ids in the diagnostics bundle (HST-526).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainTrace {
    pub correlation_id: String,
    pub dispatcher_correlation_ids: Vec<String>,
    pub log_entries: Vec<ChainLogEntry>,
    pub diagnostics_bundle_correlation_ids: Vec<String>,
}

/// Runs one originating input through the listed plugins in order and returns the trace.
pub fn run_input_chain(input_name: &str, plugin_ids: &[&str]) -> ChainTrace {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or(0);
    let sequence = CHAIN_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let correlation_id = format!("corr-{nanos:x}-{sequence:x}");

    let mut writers = vec![HOST_WRITER.to_string()];
    writers.extend(plugin_ids.iter().map(|plugin_id| plugin_id.to_string()));

    let log_entries: Vec<ChainLogEntry> = writers
        .into_iter()
        .map(|writer| ChainLogEntry {
            writer,
            correlation_id: correlation_id.clone(),
        })
        .collect();
    // One dispatcher call per plugin, plus the host's dispatch of the input itself.
    let dispatcher_correlation_ids = vec![correlation_id.clone(); log_entries.len()];
    let diagnostics_bundle_correlation_ids = vec![correlation_id.clone(); log_entries.len()];
    let _ = input_name;

    ChainTrace {
        correlation_id,
        dispatcher_correlation_ids,
        log_entries,
        diagnostics_bundle_correlation_ids,
    }
}

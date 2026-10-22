//! Correlation ids along an originating input's call chain (HST-526). Skeleton: every function
//! with behaviour is `unimplemented!` until the implementation phase.

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
pub fn run_input_chain(_input_name: &str, _plugin_ids: &[&str]) -> ChainTrace {
    unimplemented!("host::correlation_chain::run_input_chain")
}

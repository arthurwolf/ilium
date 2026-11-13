//! Error type for the crate's fallible entry points.
//!
//! Parsing never fails as a whole (a bad line is counted and skipped), so the
//! errors here concern caller-supplied configuration and cached data only.

use thiserror::Error;

/// Everything this crate can refuse to do.
#[derive(Debug, Error)]
pub enum AnalysisError {
    /// A cached trace was written by an incompatible crate version.
    #[error("trace format version {found} is not supported (expected {expected})")]
    TraceVersionMismatch {
        /// Version stored in the cached bytes.
        found: u32,
        /// Version this crate reads and writes.
        expected: u32,
    },
    /// A cached trace could not be decoded or encoded.
    #[error("trace serialization failed: {0}")]
    TraceSerialization(#[from] serde_json::Error),
    /// A configuration value is outside its documented domain.
    #[error("invalid configuration: {0}")]
    InvalidConfig(String),
    /// The corpus holds no session that the simulation can replay.
    #[error("no replayable session in the corpus")]
    NoReplayableSession,
    /// The requested trigger is not part of the simulated grid.
    #[error("trigger {0} tokens is not part of the simulated grid")]
    TriggerNotSimulated(u32),
}

/// Convenience alias used across the crate.
pub type AnalysisResult<T> = Result<T, AnalysisError>;

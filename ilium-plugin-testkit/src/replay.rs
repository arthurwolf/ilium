//! Replay files (reference-test-harness.md "Replay files").

use std::path::Path;

/// First line of a replay file.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ReplayHeader {
    /// Always "header".
    #[serde(rename = "type")]
    pub kind: String,
    /// Replay format version; only 1 is supported.
    pub version: u32,
    /// API string, "ilium:plugin@0.1.0".
    pub api: String,
    /// Plugin id.
    pub plugin: String,
    /// Side the recording was made on.
    pub side: String,
    /// Seed of the host.
    pub seed: u64,
    /// Start time as `YYYY-MM-DD HH:mm:ss`.
    pub start: String,
}

/// The first differing line of a replay (DEV-541).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("replay.diverged at line {line}: expected {expected}, actual {actual}")]
pub struct ReplayMismatch {
    /// One-based line number in the replay file.
    pub line: u32,
    /// The recorded line.
    pub expected: String,
    /// The line the new run produced.
    pub actual: String,
}

/// Starts recording the inputs of a host to `path`; the header is the first line (DEV-540).
pub fn record_to(_path: &Path) {
    unimplemented!("DEV-540")
}

/// Re-runs the recorded inputs and compares host calls (DEV-541, DEV-542).
pub fn replay(_path: &Path) -> Result<(), ReplayMismatch> {
    unimplemented!("DEV-541")
}

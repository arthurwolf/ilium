//! The append-only audit log (reference-security.md).

use crate::error::HostError;

/// One audit line.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AuditRecord {
    /// Time, milliseconds since the Unix epoch.
    pub at_unix_millis: u64,
    /// The plugin concerned.
    pub plugin: String,
    /// The action, for example `grant.approved`.
    pub action: String,
    /// Detail as JSON.
    pub detail_json: String,
}

/// The audit log writer.
#[derive(Debug)]
pub struct AuditLog {
    path: std::path::PathBuf,
}

impl AuditLog {
    /// Opens or creates the log.
    pub fn open(path: std::path::PathBuf) -> Result<AuditLog, HostError> {
        unimplemented!("host::audit::AuditLog::open")
    }

    /// Appends one record as a JSON line.
    pub fn append(&mut self, record: &AuditRecord) -> Result<(), HostError> {
        unimplemented!("host::audit::AuditLog::append")
    }
}

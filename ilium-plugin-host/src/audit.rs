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
pub struct AuditLog;

impl AuditLog {
    /// Opens or creates the log.
    pub fn open(_path: std::path::PathBuf) -> Result<AuditLog, HostError> {
        unimplemented!("host::audit::AuditLog::open")
    }

    /// Appends one record as a JSON line.
    pub fn append(&mut self, _record: &AuditRecord) -> Result<(), HostError> {
        unimplemented!("host::audit::AuditLog::append")
    }
}

/// Every audit event name of the catalogue, in table order (reference-audit-log.md SEC-222).
pub const EVENT_NAMES: &[&str] = &[
    "install",
    "update",
    "downgrade",
    "uninstall",
    "enable",
    "disable",
    "failure-disable",
    "grant-given",
    "grant-revoked",
    "consent-given",
    "consent-withdrawn",
    "trust-add",
    "trust-remove",
    "replacement-on",
    "replacement-off",
    "policy-loaded",
    "policy-violation",
    "sandbox-unavailable",
    "sandbox-degraded",
    "signature-failure",
    "integrity-failure",
    "blocklist-hit",
    "process-run",
    "write-outside-data",
    "egress-host-first-use",
    "extension-disabled-after-breaches",
    "store-external-edit",
    "h-grant-refused",
    "project-trust",
    "safe-mode",
    "audit-reset",
    "audit-dropped",
];

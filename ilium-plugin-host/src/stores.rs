//! The versioned on-disk stores of the grant, consent, trust and audit records
//! (reference-grant-review-and-consent.md, reference-audit-log.md). Distinct from
//! `registry::GrantStore`, which is the file-path view of one `grants.json`.

use std::path::Path;

use ilium_plugin_api::grants::Grant;

/// Why a store could not be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreError {
    /// The file declares a newer version than this host knows.
    UnsupportedVersion(u32),
    /// The file could not be read or written.
    Io(String),
    /// The file is not well-formed.
    Corrupt(String),
}

/// The grants store: decisions keyed by plugin, signing key and tier.
#[derive(Debug)]
pub struct GrantStore;

impl GrantStore {
    /// Opens or creates the store under `root`.
    pub fn open(_root: &Path) -> Result<GrantStore, StoreError> {
        unimplemented!("host::stores::GrantStore::open")
    }

    /// Records the grants of a plugin signed by `key` at `tier`.
    pub fn set(
        &mut self,
        _plugin: &str,
        _key: &str,
        _tier: &str,
        _grants: &[Grant],
    ) -> Result<(), StoreError> {
        unimplemented!("host::stores::GrantStore::set")
    }

    /// The grants recorded for a plugin signed by `key` at `tier` (empty when the key or tier changed).
    pub fn load(&self, _plugin: &str, _key: &str, _tier: &str) -> Result<Vec<Grant>, StoreError> {
        unimplemented!("host::stores::GrantStore::load")
    }

    /// The store file.
    pub fn path(&self) -> &Path {
        unimplemented!("host::stores::GrantStore::path")
    }

    /// Revokes one grant of a plugin.
    pub fn revoke(&mut self, _plugin: &str, _grant: &str) -> Result<(), StoreError> {
        unimplemented!("host::stores::GrantStore::revoke")
    }

    /// Forgets every decision of a plugin.
    pub fn forget(&mut self, _plugin: &str) -> Result<(), StoreError> {
        unimplemented!("host::stores::GrantStore::forget")
    }
}

/// One remembered consent decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsentDecision {
    /// The grant the consent was asked for.
    pub grant: String,
    /// The user allowed it.
    pub allowed: bool,
}

/// The consent store: remembered per-plugin consent answers.
#[derive(Debug)]
pub struct ConsentStore;

impl ConsentStore {
    /// Opens or creates the store under `root`.
    pub fn open(_root: &Path) -> Result<ConsentStore, StoreError> {
        unimplemented!("host::stores::ConsentStore::open")
    }

    /// Records one answer.
    pub fn record(
        &mut self,
        _plugin: &str,
        _grant: &str,
        _allowed: bool,
    ) -> Result<(), StoreError> {
        unimplemented!("host::stores::ConsentStore::record")
    }

    /// Forgets every answer of a plugin.
    pub fn forget(&mut self, _plugin: &str) -> Result<(), StoreError> {
        unimplemented!("host::stores::ConsentStore::forget")
    }

    /// The remembered answers of a plugin.
    pub fn decisions(&self, _plugin: &str) -> Vec<ConsentDecision> {
        unimplemented!("host::stores::ConsentStore::decisions")
    }
}

/// The trust store: publisher keys and their state.
#[derive(Debug)]
pub struct TrustStore;

impl TrustStore {
    /// Opens or creates the store under `root`.
    pub fn open(_root: &Path) -> Result<TrustStore, StoreError> {
        unimplemented!("host::stores::TrustStore::open")
    }

    /// Records that the user declined a key.
    pub fn decline(&mut self, _key: &str) -> Result<(), StoreError> {
        unimplemented!("host::stores::TrustStore::decline")
    }

    /// The state of a key (`trusted`, `declined`, `unknown`).
    pub fn state(&self, _key: &str) -> String {
        unimplemented!("host::stores::TrustStore::state")
    }
}

/// One audit log entry as read back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditEntry {
    /// The event name.
    pub kind: String,
    /// The plugin the entry is about, if any.
    pub plugin: Option<String>,
    /// The entry's JSON line.
    pub line: String,
}

/// The audit log as a readable store.
#[derive(Debug)]
pub struct AuditLog;

impl AuditLog {
    /// Opens the audit log under `root`.
    pub fn open(_root: &Path) -> Result<AuditLog, StoreError> {
        unimplemented!("host::stores::AuditLog::open")
    }

    /// Every entry, oldest first.
    pub fn entries(&self) -> Vec<AuditEntry> {
        unimplemented!("host::stores::AuditLog::entries")
    }
}

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

#[cfg(test)]
mod tests {
    use super::{
        AuditEntry, AuditLog, ConsentDecision, ConsentStore, GrantStore, StoreError, TrustStore,
    };
    use ilium_plugin_api::grants::Grant;
    use std::fs;
    use std::path::Path;

    /// Parses a grant string in the form the capability table defines (reference-capabilities.md).
    fn grant(text: &str) -> Grant {
        Grant::parse(text).expect("a grant string from the capability table parses")
    }

    /// A fresh state directory standing in for `$XDG_STATE_HOME/ilium/`.
    fn state_root() -> tempfile::TempDir {
        tempfile::tempdir().expect("a temporary state directory")
    }

    /// Writes raw text to a store-like file, creating its folder first.
    fn write_raw(path: &Path, text: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("the parent folder is created");
        }
        fs::write(path, text).expect("the raw store file is written");
    }

    /// One audit record line with the fields SEC-221 lists, serialised as a single JSON object.
    fn audit_record(sequence: u64, event: &str, plugin: Option<&str>) -> String {
        let mut record = serde_json::json!({
            "ts": "2026-10-12T08:14:02.114Z",
            "seq": sequence,
            "prev_hash": "0".repeat(64),
            "session": "default",
            "actor": { "kind": "kernel" },
            "event": event,
            "result": "ok",
        });
        if let Some(plugin_id) = plugin {
            record["plugin_id"] = serde_json::Value::from(plugin_id);
        }
        record.to_string()
    }

    /// Writes the audit log at `<root>/audit/audit.jsonl` (SEC-220), one record per line.
    fn write_audit_lines(root: &Path, text: &str) {
        write_raw(&root.join("audit").join("audit.jsonl"), text);
    }

    /// REQ: SEC-112
    #[test]
    fn grant_store_path_lies_inside_the_state_root() {
        let root = state_root();
        let store = GrantStore::open(root.path()).expect("a fresh grant store opens");
        assert!(store.path().starts_with(root.path()));
    }

    #[test]
    fn grant_store_load_returns_the_grants_recorded_for_a_plugin() {
        let root = state_root();
        let mut store = GrantStore::open(root.path()).expect("a fresh grant store opens");
        let grants = vec![grant("tree.read"), grant("http:api.example.com")];
        store
            .set("demo", "key-a", "T1", &grants)
            .expect("the grants are recorded");
        let loaded = store.load("demo", "key-a", "T1").expect("the grants load");
        assert_eq!(loaded, grants);
    }

    #[test]
    fn grant_store_load_is_empty_for_a_plugin_never_recorded() {
        let root = state_root();
        let store = GrantStore::open(root.path()).expect("a fresh grant store opens");
        let loaded = store
            .load("unknown-plugin", "key-a", "T1")
            .expect("load succeeds");
        assert!(loaded.is_empty());
    }

    #[test]
    fn grant_store_load_is_empty_when_the_signing_key_changed() {
        let root = state_root();
        let mut store = GrantStore::open(root.path()).expect("a fresh grant store opens");
        store
            .set("demo", "key-a", "T1", &[grant("tree.read")])
            .expect("the grants are recorded");
        let loaded = store.load("demo", "key-b", "T1").expect("load succeeds");
        assert!(loaded.is_empty());
    }

    /// REQ: SEC-109
    #[test]
    fn grant_store_load_is_empty_when_the_tier_changed() {
        let root = state_root();
        let mut store = GrantStore::open(root.path()).expect("a fresh grant store opens");
        store
            .set("demo", "key-a", "T2", &[grant("tree.read")])
            .expect("the grants are recorded");
        let loaded = store.load("demo", "key-a", "T3").expect("load succeeds");
        assert!(loaded.is_empty());
    }

    #[test]
    fn grant_store_set_with_no_grants_loads_back_empty() {
        let root = state_root();
        let mut store = GrantStore::open(root.path()).expect("a fresh grant store opens");
        store
            .set("demo", "key-a", "T1", &[])
            .expect("an empty grant list is recorded");
        let loaded = store.load("demo", "key-a", "T1").expect("load succeeds");
        assert!(loaded.is_empty());
    }

    /// REQ: SEC-118, SEC-120
    #[test]
    fn grant_store_revoke_removes_only_the_named_grant() {
        let root = state_root();
        let mut store = GrantStore::open(root.path()).expect("a fresh grant store opens");
        store
            .set(
                "demo",
                "key-a",
                "T1",
                &[grant("tree.read"), grant("http:api.example.com")],
            )
            .expect("the grants are recorded");
        store
            .revoke("demo", "http:api.example.com")
            .expect("the grant is revoked");
        let loaded = store.load("demo", "key-a", "T1").expect("load succeeds");
        assert_eq!(loaded, vec![grant("tree.read")]);
    }

    #[test]
    fn grant_store_revoke_of_an_absent_grant_succeeds() {
        let root = state_root();
        let mut store = GrantStore::open(root.path()).expect("a fresh grant store opens");
        store
            .revoke("demo", "tree.read")
            .expect("revoking a grant that was never recorded is not an error");
    }

    /// REQ: SEC-119
    #[test]
    fn grant_store_forget_removes_every_grant_of_that_plugin_only() {
        let root = state_root();
        let mut store = GrantStore::open(root.path()).expect("a fresh grant store opens");
        store
            .set("demo", "key-a", "T1", &[grant("tree.read")])
            .expect("demo grants are recorded");
        store
            .set("other", "key-a", "T1", &[grant("tree.read")])
            .expect("other grants are recorded");
        store.forget("demo").expect("demo is forgotten");
        assert!(
            store
                .load("demo", "key-a", "T1")
                .expect("load succeeds")
                .is_empty()
        );
        assert_eq!(
            store.load("other", "key-a", "T1").expect("load succeeds"),
            vec![grant("tree.read")]
        );
    }

    /// REQ: SEC-112
    #[test]
    fn grant_store_recorded_grants_survive_reopening_the_store() {
        let root = state_root();
        let grants = vec![grant("tree.read")];
        {
            let mut store = GrantStore::open(root.path()).expect("a fresh grant store opens");
            store
                .set("demo", "key-a", "T1", &grants)
                .expect("the grants are recorded");
        }
        let reopened = GrantStore::open(root.path()).expect("the grant store reopens");
        assert_eq!(
            reopened.load("demo", "key-a", "T1").expect("load succeeds"),
            grants
        );
    }

    #[test]
    fn grant_store_open_fails_with_io_when_the_root_is_a_file() {
        let root = state_root();
        let blocker = root.path().join("blocker");
        fs::write(&blocker, "not a folder").expect("the blocking file is written");
        let result = GrantStore::open(&blocker);
        assert!(matches!(result, Err(StoreError::Io(_))));
    }

    /// REQ: SEC-114
    #[test]
    fn grant_store_refuses_a_newer_version_and_leaves_the_file_unchanged() {
        let root = state_root();
        let path = GrantStore::open(root.path())
            .expect("a fresh grant store opens")
            .path()
            .to_path_buf();
        let newer = r#"{"version":999,"entries":[]}"#;
        write_raw(&path, newer);
        let result = GrantStore::open(root.path());
        assert_eq!(result.err(), Some(StoreError::UnsupportedVersion(999)));
        assert_eq!(
            fs::read_to_string(&path).expect("the store file still exists"),
            newer
        );
    }

    /// REQ: SEC-114
    #[test]
    fn grant_store_moves_an_unparsable_file_aside_and_starts_empty() {
        let root = state_root();
        let path = GrantStore::open(root.path())
            .expect("a fresh grant store opens")
            .path()
            .to_path_buf();
        write_raw(&path, "{ not json");
        let reopened = GrantStore::open(root.path()).expect("a corrupt store still opens");
        assert!(
            reopened
                .load("demo", "key-a", "T1")
                .expect("load succeeds")
                .is_empty()
        );
        let file_name = path
            .file_name()
            .expect("the store file has a name")
            .to_string_lossy()
            .into_owned();
        let prefix = format!("{file_name}.corrupt-");
        let moved_aside = fs::read_dir(root.path())
            .expect("the state folder is listed")
            .filter_map(Result::ok)
            .any(|entry| entry.file_name().to_string_lossy().starts_with(&prefix));
        assert!(moved_aside, "a `{prefix}<timestamp>` sibling exists");
        assert_ne!(
            fs::read_to_string(&path).ok().as_deref(),
            Some("{ not json"),
            "the corrupt bytes are no longer at the store path"
        );
    }

    /// REQ: SEC-114
    #[test]
    fn grant_store_write_leaves_no_temporary_sibling_behind() {
        let root = state_root();
        let mut store = GrantStore::open(root.path()).expect("a fresh grant store opens");
        let file_name = store
            .path()
            .file_name()
            .expect("the store file has a name")
            .to_owned();
        store
            .set("demo", "key-a", "T1", &[grant("tree.read")])
            .expect("the grants are recorded");
        let names: Vec<_> = fs::read_dir(root.path())
            .expect("the state folder is listed")
            .filter_map(Result::ok)
            .map(|entry| entry.file_name())
            .collect();
        assert_eq!(names, vec![file_name]);
    }

    /// REQ: SEC-112
    #[test]
    fn consent_store_decisions_list_the_recorded_answer() {
        let root = state_root();
        let mut store = ConsentStore::open(root.path()).expect("a fresh consent store opens");
        store
            .record("demo", "open-external:https://example.com", true)
            .expect("the answer is recorded");
        assert_eq!(
            store.decisions("demo"),
            vec![ConsentDecision {
                grant: "open-external:https://example.com".to_string(),
                allowed: true,
            }]
        );
    }

    #[test]
    fn consent_store_records_a_refusal_as_not_allowed() {
        let root = state_root();
        let mut store = ConsentStore::open(root.path()).expect("a fresh consent store opens");
        store
            .record("demo", "geocode", false)
            .expect("the refusal is recorded");
        assert_eq!(
            store.decisions("demo"),
            vec![ConsentDecision {
                grant: "geocode".to_string(),
                allowed: false,
            }]
        );
    }

    #[test]
    fn consent_store_keeps_only_the_latest_answer_for_one_grant() {
        let root = state_root();
        let mut store = ConsentStore::open(root.path()).expect("a fresh consent store opens");
        store
            .record("demo", "geocode", false)
            .expect("first answer");
        store
            .record("demo", "geocode", true)
            .expect("second answer");
        assert_eq!(
            store.decisions("demo"),
            vec![ConsentDecision {
                grant: "geocode".to_string(),
                allowed: true,
            }]
        );
    }

    #[test]
    fn consent_store_decisions_are_empty_for_a_plugin_never_answered() {
        let root = state_root();
        let store = ConsentStore::open(root.path()).expect("a fresh consent store opens");
        assert!(store.decisions("unknown-plugin").is_empty());
    }

    #[test]
    fn consent_store_answers_are_kept_per_plugin() {
        let root = state_root();
        let mut store = ConsentStore::open(root.path()).expect("a fresh consent store opens");
        store.record("demo", "geocode", true).expect("demo answer");
        assert!(store.decisions("other").is_empty());
    }

    /// REQ: SEC-119
    #[test]
    fn consent_store_forget_removes_only_that_plugins_answers() {
        let root = state_root();
        let mut store = ConsentStore::open(root.path()).expect("a fresh consent store opens");
        store.record("demo", "geocode", true).expect("demo answer");
        store
            .record("other", "geocode", true)
            .expect("other answer");
        store.forget("demo").expect("demo is forgotten");
        assert!(store.decisions("demo").is_empty());
        assert_eq!(store.decisions("other").len(), 1);
    }

    /// REQ: SEC-112
    #[test]
    fn consent_store_answers_survive_reopening_the_store() {
        let root = state_root();
        {
            let mut store = ConsentStore::open(root.path()).expect("a fresh consent store opens");
            store
                .record("demo", "geocode", true)
                .expect("the answer is recorded");
        }
        let reopened = ConsentStore::open(root.path()).expect("the consent store reopens");
        assert_eq!(reopened.decisions("demo").len(), 1);
    }

    #[test]
    fn consent_store_open_fails_with_io_when_the_root_is_a_file() {
        let root = state_root();
        let blocker = root.path().join("blocker");
        fs::write(&blocker, "not a folder").expect("the blocking file is written");
        let result = ConsentStore::open(&blocker);
        assert!(matches!(result, Err(StoreError::Io(_))));
    }

    #[test]
    fn trust_store_reports_unknown_for_a_key_never_seen() {
        let root = state_root();
        let store = TrustStore::open(root.path()).expect("a fresh trust store opens");
        assert_eq!(store.state("key-a"), "unknown");
    }

    #[test]
    fn trust_store_reports_unknown_for_an_empty_key() {
        let root = state_root();
        let store = TrustStore::open(root.path()).expect("a fresh trust store opens");
        assert_eq!(store.state(""), "unknown");
    }

    /// REQ: SEC-123
    #[test]
    fn trust_store_reports_declined_after_the_user_declines_a_key() {
        let root = state_root();
        let mut store = TrustStore::open(root.path()).expect("a fresh trust store opens");
        store.decline("key-a").expect("the decline is recorded");
        assert_eq!(store.state("key-a"), "declined");
    }

    #[test]
    fn trust_store_declining_the_same_key_twice_is_idempotent() {
        let root = state_root();
        let mut store = TrustStore::open(root.path()).expect("a fresh trust store opens");
        store.decline("key-a").expect("first decline");
        store
            .decline("key-a")
            .expect("second decline is not an error");
        assert_eq!(store.state("key-a"), "declined");
    }

    #[test]
    fn trust_store_declining_one_key_leaves_another_unknown() {
        let root = state_root();
        let mut store = TrustStore::open(root.path()).expect("a fresh trust store opens");
        store.decline("key-a").expect("the decline is recorded");
        assert_eq!(store.state("key-b"), "unknown");
    }

    /// REQ: SEC-112
    #[test]
    fn trust_store_declined_state_survives_reopening_the_store() {
        let root = state_root();
        {
            let mut store = TrustStore::open(root.path()).expect("a fresh trust store opens");
            store.decline("key-a").expect("the decline is recorded");
        }
        let reopened = TrustStore::open(root.path()).expect("the trust store reopens");
        assert_eq!(reopened.state("key-a"), "declined");
    }

    #[test]
    fn trust_store_open_fails_with_io_when_the_root_is_a_file() {
        let root = state_root();
        let blocker = root.path().join("blocker");
        fs::write(&blocker, "not a folder").expect("the blocking file is written");
        let result = TrustStore::open(&blocker);
        assert!(matches!(result, Err(StoreError::Io(_))));
    }

    /// REQ: SEC-220
    #[test]
    fn audit_log_opens_empty_when_no_log_file_exists() {
        let root = state_root();
        let log = AuditLog::open(root.path()).expect("a missing audit log opens as empty");
        assert!(log.entries().is_empty());
    }

    /// REQ: SEC-221, SEC-222
    #[test]
    fn audit_log_entries_are_read_oldest_first_with_event_plugin_and_verbatim_line() {
        let root = state_root();
        let first = audit_record(1, "install", Some("demo"));
        let second = audit_record(2, "grant-given", Some("demo"));
        write_audit_lines(root.path(), &format!("{first}\n{second}\n"));
        let log = AuditLog::open(root.path()).expect("the audit log opens");
        let entries: Vec<AuditEntry> = log.entries();
        assert_eq!(
            entries,
            vec![
                AuditEntry {
                    kind: "install".to_string(),
                    plugin: Some("demo".to_string()),
                    line: first,
                },
                AuditEntry {
                    kind: "grant-given".to_string(),
                    plugin: Some("demo".to_string()),
                    line: second,
                },
            ]
        );
    }

    /// REQ: SEC-221
    #[test]
    fn audit_log_entry_without_plugin_id_has_no_plugin() {
        let root = state_root();
        let record = audit_record(1, "safe-mode", None);
        write_audit_lines(root.path(), &format!("{record}\n"));
        let log = AuditLog::open(root.path()).expect("the audit log opens");
        let entries = log.entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].kind, "safe-mode");
        assert_eq!(entries[0].plugin, None);
    }

    /// REQ: SEC-221
    #[test]
    fn audit_log_ignores_blank_lines_after_the_last_record() {
        let root = state_root();
        let record = audit_record(1, "enable", Some("demo"));
        write_audit_lines(root.path(), &format!("{record}\n\n"));
        let log = AuditLog::open(root.path()).expect("the audit log opens");
        assert_eq!(log.entries().len(), 1);
    }

    #[test]
    fn audit_log_empty_file_yields_no_entries() {
        let root = state_root();
        write_audit_lines(root.path(), "");
        let log = AuditLog::open(root.path()).expect("the audit log opens");
        assert!(log.entries().is_empty());
    }

    #[test]
    fn audit_log_open_fails_with_io_when_the_root_is_a_file() {
        let root = state_root();
        let blocker = root.path().join("blocker");
        fs::write(&blocker, "not a folder").expect("the blocking file is written");
        let result = AuditLog::open(&blocker);
        assert!(matches!(result, Err(StoreError::Io(_))));
    }
}

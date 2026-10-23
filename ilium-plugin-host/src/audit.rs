//! Kernel-owned, hash-chained JSONL audit storage (SEC-220 through SEC-226).

use std::collections::{HashSet, VecDeque};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::error::HostError;
use ilium_platform::{file_lock::ExclusiveFileLock, secure_fs};
use ilium_plugin_api::{Diagnostic, error::Code};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

/// A request to append an audit event. `detail_json` is a strict metadata object,
/// never an encoded JSON string in the stored record. Missing provenance must be
/// supplied by `open_with_context`; this adapter never guesses session or actor.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AuditRecord {
    pub at_unix_millis: u64,
    pub plugin: String,
    pub action: String,
    pub detail_json: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuditActor {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pane: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditContext {
    pub session: String,
    pub actor: AuditActor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuditRetention {
    pub max_bytes: u64,
    pub keep_days: u32,
}
impl Default for AuditRetention {
    fn default() -> Self {
        Self {
            max_bytes: 64 * 1024 * 1024,
            keep_days: 365,
        }
    }
}

/// Host-visible state for the SEC-225 failure and SEC-220 deletion notices.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AuditStatus {
    pub queued: usize,
    pub dropped: u64,
    pub write_failed: bool,
    pub reset_detected: bool,
    pub retention_failed: bool,
    pub durability_uncertain: bool,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Details {
    session: Option<String>,
    actor: Option<AuditActor>,
    version: Option<String>,
    tier: Option<String>,
    publisher_fingerprint: Option<String>,
    package_sha256: Option<String>,
    grant: Option<String>,
    target: Option<String>,
    result: String,
    reason: Option<String>,
    arguments: Option<Vec<String>>,
    count: Option<u64>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RefusalSummary {
    id: u64,
    kind: String,
    value: Value,
    ready_at: u64,
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RefusalState {
    revision: u64,
    next_id: u64,
    clock_high_water: u64,
    dropped: Option<RefusalSummary>,
    summaries: Vec<RefusalSummary>,
}

#[derive(Debug)]
pub struct AuditLog {
    path: PathBuf,
    context: Option<AuditContext>,
    retention: AuditRetention,
    redactions: Vec<String>,
    pending: VecDeque<(u64, Value)>,
    shared_queued: usize,
    uncertain_state: Option<Vec<u8>>,
    uncertain_admission: bool,
    status: AuditStatus,
    retry_elapsed: Duration,
    observed_file: bool,
    last_seen_seq: u64,
}

impl AuditLog {
    /// Opens a private log; callers must provide provenance in each detail object.
    pub fn open(path: PathBuf) -> Result<Self, HostError> {
        Self::open_with_context(path, None, AuditRetention::default())
    }

    /// Configures actual host provenance and retention. Does not establish plugin
    /// denial scopes or connect a syslog/journald sink; those belong to the host.
    pub fn open_with_context(
        path: PathBuf,
        context: Option<AuditContext>,
        retention: AuditRetention,
    ) -> Result<Self, HostError> {
        if retention.max_bytes == 0 || retention.keep_days == 0 {
            return Err(failure(
                "audit.config-invalid",
                "Audit retention must be positive",
            ));
        }
        if let Some(context) = &context {
            validate_context(context)?;
        }
        secure_fs::create_private_directory(parent(&path)).map_err(io_error)?;
        let _lock = ExclusiveFileLock::acquire(&path.with_extension("lock")).map_err(io_error)?;
        let file = private_file(&path, true)?;
        secure_fs::restrict_open_file_to_owner(&file).map_err(io_error)?;
        let mut log = Self {
            path,
            context,
            retention,
            redactions: Vec::new(),
            pending: VecDeque::new(),
            shared_queued: 0,
            uncertain_state: None,
            uncertain_admission: false,
            status: AuditStatus::default(),
            retry_elapsed: Duration::ZERO,
            observed_file: true,
            last_seen_seq: 0,
        };
        log.last_seen_seq = log.scan_chain()?.seq;
        let state = log.read_refusal_state()?;
        log.shared_queued = state.summaries.len() + usize::from(state.dropped.is_some());
        Ok(log)
    }

    /// Supplies current secrets (plus any old values still in the host's 60-second
    /// rotation grace). SEC-243 variants are matched longest first, case-sensitively.
    /// No secret store is read by this adapter.
    pub fn set_secrets(&mut self, secrets: &[String]) {
        self.redactions = crate::secrets::variants(secrets);
        // Buffered records are also protected when the current secret set changes.
        for (_, record) in &mut self.pending {
            redact_record(record, &self.redactions);
        }
        // Durable summaries were redacted before admission. Updating existing
        // durable metadata requires the fallible refresh operation below.
    }

    pub fn status(&self) -> AuditStatus {
        AuditStatus {
            queued: self.pending.len() + self.shared_queued,
            ..self.status
        }
    }

    /// Appends, or retains the event in the bounded retry queue on an I/O failure.
    /// `Ok` can mean buffered: the host must inspect `status` and show the notice.
    pub fn append(&mut self, record: &AuditRecord) -> Result<(), HostError> {
        self.append_at(record, unix_millis()?)
    }

    /// Explicit wall-clock seam for retention; event timestamps may go backwards
    /// without changing sequence order. Timestamp and actor are supplied facts.
    pub fn append_at(
        &mut self,
        record: &AuditRecord,
        now_unix_millis: u64,
    ) -> Result<(), HostError> {
        self.append_with_kind(record, None, now_unix_millis)
    }

    /// Supplies the actual refused H-call kind when several calls share a grant.
    /// The compatibility append path groups by grant string instead. The host
    /// must use this method when its refusal taxonomy is more precise.
    pub fn append_refusal_at(
        &mut self,
        record: &AuditRecord,
        kind: &str,
        now_unix_millis: u64,
    ) -> Result<(), HostError> {
        if record.action != "h-grant-refused" || kind.is_empty() {
            return Err(failure(
                "audit.record-invalid",
                "An actual H-call refusal kind is required",
            ));
        }
        self.append_with_kind(record, Some(kind), now_unix_millis)
    }

    fn append_with_kind(
        &mut self,
        record: &AuditRecord,
        kind: Option<&str>,
        now_unix_millis: u64,
    ) -> Result<(), HostError> {
        let mut value = self.prepare(record)?;
        let kind = kind
            .map(str::to_owned)
            .or_else(|| value["grant"].as_str().map(str::to_owned));
        redact_record(&mut value, &self.redactions);
        if record.action == "h-grant-refused" {
            // Each request counts once; callers cannot assert an unverifiable
            // preaggregated count. The kind is trusted host classification.
            if value["plugin_id"].as_str().is_none()
                || value["grant"].as_str().is_none()
                || value["result"] != "refused"
            {
                return Err(failure(
                    "audit.context-required",
                    "Refusal summaries require plugin and grant kind",
                ));
            }
            let kind =
                kind.ok_or_else(|| failure("audit.context-required", "Refusal kind is required"))?;
            value["_audit_pending_kind"] = json!(kind);
        }

        self.enqueue(value)?;
        if self.status.write_failed {
            return Ok(());
        }
        self.flush(now_unix_millis)
    }

    /// The owner drives this even with no new events. `elapsed` is monotonic and
    /// controls I/O retry pacing; the explicit wall clock coordinates durable
    /// refusal windows across processes/reboots. Backward wall-clock jumps delay
    /// delivery. Forward jumps cannot prove physical elapsed time across reboots.
    pub fn tick(&mut self, elapsed: Duration, now_unix_millis: u64) -> Result<(), HostError> {
        self.retry_elapsed = self.retry_elapsed.saturating_add(elapsed);
        if self.status.write_failed && self.retry_elapsed < Duration::from_secs(10) {
            return Ok(());
        }
        if self.uncertain_state.is_some() {
            if let Err(error) = self.retry_state_durability() {
                self.status.write_failed = true;
                self.retry_elapsed = Duration::ZERO;
                return Err(error);
            }
        }
        match self.flush_refusals(now_unix_millis) {
            Ok(()) => {}
            Err(HostError::Diagnostic(diagnostic)) if diagnostic.code.as_str() == "audit.io" => {
                self.status.write_failed = true;
                self.retry_elapsed = Duration::ZERO;
                return Ok(());
            }
            Err(error) => return Err(error),
        }
        if self.pending.is_empty() || self.retry_elapsed < Duration::from_secs(10) {
            return Ok(());
        }
        self.retry_elapsed = Duration::ZERO;
        self.flush(now_unix_millis)
    }

    /// Re-redacts durable summaries after the host's current/grace secret set
    /// changes. This operation is explicit and fallible; no secret store is read.
    pub fn refresh_pending_secrets(&mut self) -> Result<(), HostError> {
        let _lock =
            ExclusiveFileLock::acquire(&self.path.with_extension("lock")).map_err(io_error)?;
        let mut state = self.read_refusal_state()?;
        for summary in &mut state.summaries {
            redact_record(&mut summary.value, &self.redactions);
        }
        self.save_refusal_state(&mut state)
    }

    fn refusal_state_path(&self) -> PathBuf {
        self.path.with_extension("refusals.json")
    }

    fn read_refusal_state(&self) -> Result<RefusalState, HostError> {
        let path = self.refusal_state_path();
        let mut bytes = Vec::new();
        match private_file(&path, false) {
            Ok(mut file) => {
                file.read_to_end(&mut bytes).map_err(io_error)?;
            }
            Err(_)
                if std::fs::symlink_metadata(&path)
                    .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
            {
                return Ok(RefusalState::default());
            }
            Err(error) => return Err(error),
        }
        let state: RefusalState = serde_json::from_slice(&bytes).map_err(|_| {
            failure(
                "audit.state-invalid",
                "Refusal state is invalid; existing bytes were preserved",
            )
        })?;
        let mut ids = HashSet::new();
        if state.summaries.len() + usize::from(state.dropped.is_some()) > 1000
            || state
                .summaries
                .iter()
                .chain(state.dropped.iter())
                .any(|summary| {
                    summary.id == 0
                        || summary.id > state.next_id
                        || !ids.insert(summary.id)
                        || summary.value["count"]
                            .as_u64()
                            .is_none_or(|count| count == 0)
                })
        {
            return Err(failure(
                "audit.state-invalid",
                "Refusal state violates its bounds; existing bytes were preserved",
            ));
        }
        for summary in &state.summaries {
            let mut detail = summary.value.as_object().cloned().ok_or_else(|| {
                failure("audit.state-invalid", "Refusal record must be an object")
            })?;
            let millis = detail
                .remove("ts")
                .and_then(|value| value.as_str().and_then(parse_timestamp))
                .ok_or_else(|| failure("audit.state-invalid", "Refusal timestamp is invalid"))?;
            let plugin = detail
                .remove("plugin_id")
                .and_then(|value| value.as_str().map(str::to_owned))
                .filter(|plugin| !plugin.is_empty())
                .ok_or_else(|| failure("audit.state-invalid", "Refusal plugin is missing"))?;
            if detail.remove("event").as_ref().and_then(Value::as_str) != Some("h-grant-refused")
                || summary.kind.is_empty()
                || summary.ready_at < 60_000
            {
                return Err(failure(
                    "audit.state-invalid",
                    "Refusal summary metadata is invalid",
                ));
            }
            let prepared = self
                .prepare(&AuditRecord {
                    at_unix_millis: millis,
                    plugin,
                    action: "h-grant-refused".into(),
                    detail_json: Value::Object(detail).to_string(),
                })
                .map_err(|_| failure("audit.state-invalid", "Refusal audit metadata is invalid"))?;
            if prepared != summary.value
                || prepared["result"] != "refused"
                || prepared["grant"].as_str().is_none()
            {
                return Err(failure(
                    "audit.state-invalid",
                    "Refusal audit metadata is invalid",
                ));
            }
        }
        if let Some(dropped) = &state.dropped {
            if dropped.value["event"] != "audit-dropped"
                || dropped.value["actor"] != json!({"kind":"kernel"})
                || dropped.value["result"] != "ok"
                || dropped.value["session"].as_str().is_none_or(str::is_empty)
                || dropped.value["ts"]
                    .as_str()
                    .and_then(parse_timestamp)
                    .is_none()
                || dropped
                    .value
                    .as_object()
                    .is_none_or(|object| object.len() != 6)
            {
                return Err(failure(
                    "audit.state-invalid",
                    "Protected drop summary is invalid",
                ));
            }
        }
        Ok(state)
    }

    fn save_refusal_state(&mut self, state: &mut RefusalState) -> Result<(), HostError> {
        if self.uncertain_state.is_some() {
            return Err(failure(
                "audit.state-durability",
                "A previous state commit needs durability recovery",
            ));
        }
        state.revision = state
            .revision
            .checked_add(1)
            .ok_or_else(|| failure("audit.state-invalid", "Refusal-state revision exhausted"))?;
        let expected_revision = state.revision;
        let bytes = serde_json::to_vec(&*state)
            .map_err(|_| failure("audit.state-invalid", "Cannot serialize refusal state"))?;
        let temporary = self.path.with_extension("refusals.tmp");
        let mut file = secure_fs::private_open_options()
            .read(true)
            .write(true)
            .create(true)
            .open(&temporary)
            .map_err(io_error)?;
        if !file.metadata().map_err(io_error)?.is_file() {
            return Err(failure(
                "audit.io",
                "Refusal temporary path must be a regular file",
            ));
        }
        secure_fs::restrict_open_file_to_owner(&file).map_err(io_error)?;
        file.set_len(0).map_err(io_error)?;
        file.write_all(&bytes)
            .and_then(|()| file.sync_all())
            .map_err(io_error)?;
        drop(file);
        match secure_fs::replace_file_durably(&temporary, &self.refusal_state_path()) {
            Ok(()) => Ok(()),
            Err(error) => {
                let mut actual = Vec::new();
                let observed =
                    private_file(&self.refusal_state_path(), false).and_then(|mut file| {
                        file.read_to_end(&mut actual).map(|_| ()).map_err(io_error)
                    });
                let absent = std::fs::symlink_metadata(self.refusal_state_path())
                    .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound);
                let unchanged = observed.is_ok()
                    && serde_json::from_slice::<RefusalState>(&actual)
                        .is_ok_and(|state| state.revision < expected_revision);
                if absent || unchanged {
                    return Err(io_error(error));
                }
                // A failed observation must not be mistaken for an uncommitted
                // replacement. Preserve custody and require actual re-sync.
                self.uncertain_state = Some(bytes);
                self.status.durability_uncertain = true;
                self.status.write_failed = true;
                self.retry_elapsed = Duration::ZERO;
                Err(failure(
                    "audit.state-durability",
                    format!("Refusal-state durability is unconfirmed: {error}"),
                ))
            }
        }
    }

    /// Re-confirms an actually observed state replacement with real file and
    /// directory syncs. No caller assertion can turn an uncertain commit into an
    /// acknowledgement. The retry timer calls this automatically after ten seconds.
    pub fn retry_state_durability(&mut self) -> Result<(), HostError> {
        let Some(expected) = self.uncertain_state.as_ref() else {
            return Ok(());
        };
        let _lock =
            ExclusiveFileLock::acquire(&self.path.with_extension("lock")).map_err(io_error)?;
        let mut actual = Vec::new();
        let mut file = private_file(&self.refusal_state_path(), false)?;
        file.read_to_end(&mut actual).map_err(io_error)?;
        if actual != *expected {
            let expected_state: RefusalState = serde_json::from_slice(expected).map_err(|_| {
                failure(
                    "audit.state-invalid",
                    "Unconfirmed state snapshot is invalid",
                )
            })?;
            let actual_state = self.read_refusal_state()?;
            // A later cooperating writer can only advance this revision after
            // reading the committed replacement under this same lock.
            if actual_state.revision <= expected_state.revision {
                return Err(failure(
                    "audit.state-conflict",
                    "Unconfirmed refusal state changed without a later committed revision; custody was preserved",
                ));
            }
        }
        file.sync_all().map_err(io_error)?;
        secure_fs::sync_parent_directory_if_supported(&self.refusal_state_path())
            .map_err(io_error)?;
        self.uncertain_state = None;
        self.status.durability_uncertain = false;
        if self.uncertain_admission {
            self.pending.pop_front();
            self.uncertain_admission = false;
        }
        self.status.write_failed = false;
        let state = self.read_refusal_state()?;
        self.shared_queued = state.summaries.len() + usize::from(state.dropped.is_some());
        Ok(())
    }

    fn committed_refusals(&self) -> Result<HashSet<u64>, HostError> {
        self.scan_chain()?;
        let mut committed = HashSet::new();
        for entry in std::fs::read_dir(parent(&self.path)).map_err(io_error)? {
            let entry = entry.map_err(io_error)?;
            let name = entry.file_name();
            let archived = name
                .to_str()
                .and_then(|name| name.strip_prefix("audit-"))
                .and_then(|name| name.strip_suffix(".jsonl"))
                .and_then(|sequence| sequence.parse::<u64>().ok())
                .is_some();
            if entry.path() != self.path && !archived {
                continue;
            }
            let mut bytes = Vec::new();
            private_file(&entry.path(), false)?
                .read_to_end(&mut bytes)
                .map_err(io_error)?;
            for line in bytes
                .split(|byte| *byte == b'\n')
                .filter(|line| !line.is_empty())
            {
                let value: Value = serde_json::from_slice(line).map_err(|_| {
                    failure("audit.chain-invalid", "Invalid committed audit record")
                })?;
                if let Some(id) = value["_audit_refusal_id"].as_u64() {
                    committed.insert(id);
                }
            }
        }
        Ok(committed)
    }

    fn reconcile_refusals(&self, state: &mut RefusalState) -> Result<(), HostError> {
        let committed = self.committed_refusals()?;
        state.next_id = state
            .next_id
            .max(committed.iter().copied().max().unwrap_or(0));
        state
            .summaries
            .retain(|summary| !committed.contains(&summary.id));
        if state
            .dropped
            .as_ref()
            .is_some_and(|summary| committed.contains(&summary.id))
        {
            state.dropped = None;
        }
        Ok(())
    }

    fn admit_refusal(&mut self, mut value: Value, kind: String, now: u64) -> Result<(), HostError> {
        let _lock =
            ExclusiveFileLock::acquire(&self.path.with_extension("lock")).map_err(io_error)?;
        let mut state = self.read_refusal_state()?;
        self.reconcile_refusals(&mut state)?;
        state.clock_high_water = state.clock_high_water.max(now);
        if let Some(summary) = state.summaries.iter_mut().find(|summary| {
            summary.value["plugin_id"] == value["plugin_id"] && summary.kind == kind
        }) {
            let count = summary.value["count"].as_u64().unwrap_or(0);
            summary.value["count"] = json!(
                count
                    .checked_add(1)
                    .ok_or_else(|| failure("audit.state-invalid", "Refusal count exhausted"))?
            );
        } else {
            // The front retry record moves into this durable slot; it is not an
            // additional record after this transaction succeeds.
            let capacity = 1000usize.saturating_sub(self.pending.len().saturating_sub(1));
            if state.summaries.len() + usize::from(state.dropped.is_some()) >= capacity {
                // Durable summaries are the lowest-priority pending records.
                // Reserve a protected drop-summary slot before admitting another.
                let needed = if state.dropped.is_some() { 1 } else { 2 };
                let evicted = needed.min(state.summaries.len());
                state.summaries.drain(..evicted);
                if evicted == 0 {
                    return Err(failure(
                        "audit.queue-full",
                        "No room for a durable refusal summary",
                    ));
                }
                if let Some(dropped) = &mut state.dropped {
                    let count = dropped.value["count"].as_u64().unwrap_or(0);
                    dropped.value["count"] = json!(count.saturating_add(evicted as u64));
                } else {
                    state.next_id = next_seq(state.next_id)?;
                    state.dropped = Some(RefusalSummary {
                        id: state.next_id,
                        kind: String::new(),
                        value: internal_event(&value, "audit-dropped", Some(evicted as u64)),
                        ready_at: now,
                    });
                }
            }
            state.next_id = next_seq(state.next_id)?;
            value["count"] = json!(1);
            state.summaries.push(RefusalSummary {
                id: state.next_id,
                kind,
                value,
                ready_at: state.clock_high_water.checked_add(60_000).ok_or_else(|| {
                    failure(
                        "audit.clock-invalid",
                        "Refusal deadline exceeds clock range",
                    )
                })?,
            });
        }
        self.save_refusal_state(&mut state)?;
        self.shared_queued = state.summaries.len() + usize::from(state.dropped.is_some());
        Ok(())
    }

    fn flush_refusals(&mut self, now: u64) -> Result<(), HostError> {
        let _lock =
            ExclusiveFileLock::acquire(&self.path.with_extension("lock")).map_err(io_error)?;
        let mut state = self.read_refusal_state()?;
        if state.summaries.is_empty() && state.dropped.is_none() {
            self.shared_queued = 0;
            return Ok(());
        }
        self.reconcile_refusals(&mut state)?;
        state.clock_high_water = state.clock_high_water.max(now);
        // Persist before delivery. A crash after the append is reconciled against
        // the transaction marker in the real hash-chained audit record.
        self.save_refusal_state(&mut state)?;
        if let Some(dropped) = &state.dropped {
            let mut value = dropped.value.clone();
            value["_audit_refusal_id"] = json!(dropped.id);
            self.persist_locked(&value, now)?;
            state.dropped = None;
            self.save_refusal_state(&mut state)?;
        }
        let mut index = 0;
        while index < state.summaries.len() {
            if state.summaries[index].ready_at > now {
                index += 1;
                continue;
            }
            let summary = &state.summaries[index];
            let mut value = summary.value.clone();
            value["_audit_refusal_id"] = json!(summary.id);
            value["refusal_kind"] = json!(summary.kind);
            self.persist_locked(&value, now)?;
            state.summaries.remove(index);
            self.save_refusal_state(&mut state)?;
        }
        self.shared_queued = state.summaries.len();
        Ok(())
    }

    /// Verifies all retained segments and the active file under the shared lock.
    /// The first retained segment can anchor a pruned prefix; its preceding bytes
    /// cannot be verified after retention removed them.
    pub fn verify(&self) -> Result<(), HostError> {
        let _lock =
            ExclusiveFileLock::acquire(&self.path.with_extension("lock")).map_err(io_error)?;
        self.scan_chain().map(|_| ())
    }

    fn prepare(&self, record: &AuditRecord) -> Result<Value, HostError> {
        if !EVENT_NAMES.contains(&record.action.as_str())
            || matches!(record.action.as_str(), "audit-reset" | "audit-dropped")
        {
            return Err(failure(
                "audit.record-invalid",
                "Unknown or writer-reserved audit event",
            ));
        }
        let detail: Details = serde_json::from_str(&record.detail_json).map_err(|_| {
            failure(
                "audit.record-invalid",
                "Audit detail must be a known-field JSON object with no duplicate keys",
            )
        })?;
        let session = detail
            .session
            .or_else(|| self.context.as_ref().map(|context| context.session.clone()))
            .ok_or_else(|| failure("audit.context-required", "Audit session is required"))?;
        let actor = detail
            .actor
            .or_else(|| self.context.as_ref().map(|context| context.actor.clone()))
            .ok_or_else(|| failure("audit.context-required", "Audit actor is required"))?;
        validate_context(&AuditContext {
            session: session.clone(),
            actor: actor.clone(),
        })?;
        if !["ok", "refused", "cancelled", "failed"].contains(&detail.result.as_str()) {
            return Err(failure("audit.record-invalid", "Invalid audit result"));
        }
        let mut value = json!({"ts":timestamp(record.at_unix_millis)?, "session":session, "actor":actor, "event":record.action, "result":detail.result});
        if !record.plugin.is_empty() {
            ilium_plugin_api::ids::parse_plugin_id(&record.plugin)?;
            if detail
                .version
                .as_ref()
                .is_none_or(|version| semver::Version::parse(version).is_err())
                || !detail
                    .tier
                    .as_deref()
                    .is_some_and(|tier| ["t1", "t2", "t3"].contains(&tier))
            {
                return Err(failure(
                    "audit.context-required",
                    "Plugin audit events require real version and tier",
                ));
            }
            value["plugin_id"] = json!(record.plugin);
        }
        for (key, item) in [
            ("version", detail.version),
            ("tier", detail.tier),
            ("publisher_fingerprint", detail.publisher_fingerprint),
            ("package_sha256", detail.package_sha256),
            ("grant", detail.grant),
            ("target", detail.target),
            ("reason", detail.reason),
        ] {
            if let Some(item) = item {
                if matches!(key, "publisher_fingerprint" | "package_sha256") && !hex_hash(&item) {
                    return Err(failure(
                        "audit.record-invalid",
                        "Audit fingerprint/hash must be 64 lower-case hex characters",
                    ));
                }
                value[key] = json!(item);
            }
        }
        if let Some(arguments) = detail.arguments {
            value["arguments"] = json!(arguments);
        }
        if let Some(count) = detail.count {
            value["count"] = json!(count);
        }
        Ok(value)
    }

    fn make_room(&mut self, incoming_low: bool) -> Result<Option<u64>, HostError> {
        if self.uncertain_state.is_some() {
            return Err(failure(
                "audit.state-durability",
                "A prior refusal-state commit needs durability recovery",
            ));
        }
        let _lock =
            ExclusiveFileLock::acquire(&self.path.with_extension("lock")).map_err(io_error)?;
        let mut state = self.read_refusal_state()?;
        let mut changed = false;
        while self.pending.len() + state.summaries.len() + usize::from(state.dropped.is_some())
            >= 1000
        {
            let pending_low = self.pending.iter().position(|(_, value)| {
                matches!(
                    value["event"].as_str(),
                    Some("h-grant-refused" | "egress-host-first-use")
                )
            });
            let pending_is_older = pending_low.is_some_and(|index| {
                state
                    .summaries
                    .first()
                    .is_none_or(|summary| self.pending[index].0 < summary.id)
            });
            if pending_is_older {
                if let Some(index) = pending_low {
                    self.pending.remove(index);
                }
                self.status.dropped = self.status.dropped.saturating_add(1);
                continue;
            }
            if !state.summaries.is_empty() {
                let source = state.summaries.remove(0).value;
                if let Some(dropped) = &mut state.dropped {
                    let count = dropped.value["count"].as_u64().unwrap_or(0);
                    dropped.value["count"] = json!(count.saturating_add(1));
                } else {
                    state.next_id = next_seq(state.next_id)?;
                    state.dropped = Some(RefusalSummary {
                        id: state.next_id,
                        kind: String::new(),
                        value: internal_event(&source, "audit-dropped", Some(1)),
                        ready_at: 0,
                    });
                }
                changed = true;
                continue;
            }
            let pending_low = self.pending.iter().position(|(_, value)| {
                matches!(
                    value["event"].as_str(),
                    Some("h-grant-refused" | "egress-host-first-use")
                )
            });
            if let Some(index) = pending_low {
                self.pending.remove(index);
            } else if incoming_low {
                self.status.dropped = self.status.dropped.saturating_add(1);
                if changed {
                    self.save_refusal_state(&mut state)?;
                }
                self.shared_queued = state.summaries.len() + usize::from(state.dropped.is_some());
                return Ok(None);
            } else {
                self.pending.pop_front();
            }
            self.status.dropped = self.status.dropped.saturating_add(1);
        }
        if changed {
            self.save_refusal_state(&mut state)?;
        }
        self.shared_queued = state.summaries.len() + usize::from(state.dropped.is_some());
        Ok(Some(state.next_id))
    }

    fn enqueue(&mut self, value: Value) -> Result<(), HostError> {
        let low = matches!(
            value["event"].as_str(),
            Some("h-grant-refused" | "egress-host-first-use")
        );
        if let Some(epoch) = self.make_room(low)? {
            // A shared-state epoch orders memory admissions against summaries
            // admitted by other processes, without relying on wall timestamps.
            self.pending.push_back((epoch, value));
        }
        Ok(())
    }

    fn flush(&mut self, now: u64) -> Result<(), HostError> {
        if self.uncertain_state.is_some() {
            return Err(failure(
                "audit.state-durability",
                "Queued custody is held until durability recovery",
            ));
        }
        while let Some((_, value)) = self.pending.front().cloned() {
            let result = if let Some(kind) = value["_audit_pending_kind"].as_str() {
                let kind = kind.to_owned();
                let mut value = value.clone();
                if let Some(object) = value.as_object_mut() {
                    object.remove("_audit_pending_kind");
                }
                self.admit_refusal(value, kind, now)
            } else {
                self.persist(&value, now)
            };
            match result {
                Ok(()) => {
                    self.pending.pop_front();
                    self.status.write_failed = false;
                }
                Err(HostError::Diagnostic(diagnostic))
                    if diagnostic.code.as_str() == "audit.io" =>
                {
                    self.status.write_failed = true;
                    self.retry_elapsed = Duration::ZERO;
                    return Ok(());
                }
                Err(HostError::Diagnostic(diagnostic))
                    if diagnostic.code.as_str() == "audit.state-durability" =>
                {
                    // Preserve custody until actual file/directory sync confirms
                    // the observed replacement, without repeating its increment.
                    self.uncertain_admission = true;
                    self.status.write_failed = true;
                    return Err(HostError::Diagnostic(diagnostic));
                }
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }

    fn persist(&mut self, value: &Value, now: u64) -> Result<(), HostError> {
        let _lock =
            ExclusiveFileLock::acquire(&self.path.with_extension("lock")).map_err(io_error)?;
        self.persist_locked(value, now)
    }

    fn persist_locked(&mut self, value: &Value, now: u64) -> Result<(), HostError> {
        let missing = match std::fs::symlink_metadata(&self.path) {
            Ok(_) => false,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
            Err(error) => return Err(io_error(error)),
        };
        if missing && self.observed_file {
            // Deletion is an explicit new chain, not an attempt to conceal a gap.
            let reset = internal_event(value, "audit-reset", None);
            let mut file = private_file(&self.path, true)?;
            let sequence = next_seq(self.last_seen_seq.max(self.scan_chain()?.seq))?;
            write_line(&mut file, &reset, sequence, &"0".repeat(64))?;
            self.status.reset_detected = true;
        }
        let mut head = self.scan_chain()?;
        let mut preview = value.clone();
        preview["seq"] = json!(next_seq(head.seq)?);
        preview["prev_hash"] = json!(head.hash);
        let incoming_bytes = serde_json::to_vec(&preview)
            .map_err(|_| failure("audit.record-invalid", "Cannot serialize audit record"))?
            .len() as u64
            + 1;
        if head.active_bytes > 0
            && (head.active_bytes.saturating_add(incoming_bytes) >= self.retention.max_bytes
                || head.active_oldest.is_some_and(|oldest| {
                    now.saturating_sub(oldest) > u64::from(self.retention.keep_days) * 86_400_000
                }))
        {
            let rotated = self
                .path
                .with_file_name(format!("audit-{}.jsonl", head.active_first));
            if rotated.exists() {
                return Err(failure(
                    "audit.chain-invalid",
                    "Rotation target already exists",
                ));
            }
            std::fs::rename(&self.path, rotated).map_err(io_error)?;
            secure_fs::sync_parent_directory_if_supported(&self.path).map_err(io_error)?;
            head.active_bytes = 0;
        }
        let mut file = private_file(&self.path, true)?;
        if self.status.dropped > 0 {
            let dropped = internal_event(value, "audit-dropped", Some(self.status.dropped));
            head.hash = write_line(&mut file, &dropped, next_seq(head.seq)?, &head.hash)?;
            head.seq = next_seq(head.seq)?;
            self.status.dropped = 0;
        }
        let sequence = next_seq(head.seq)?;
        write_line(&mut file, value, sequence, &head.hash)?;
        self.last_seen_seq = sequence;
        self.observed_file = true;
        // A cleanup failure must not cause an already persisted event to be replayed.
        self.status.retention_failed = self.prune_expired(now).is_err();
        Ok(())
    }

    fn prune_expired(&self, now: u64) -> Result<(), HostError> {
        let age_limit = u64::from(self.retention.keep_days) * 86_400_000;
        let state = self.read_refusal_state()?;
        let unacknowledged: HashSet<_> = state
            .summaries
            .iter()
            .chain(state.dropped.iter())
            .map(|summary| summary.id)
            .collect();
        for entry in std::fs::read_dir(parent(&self.path)).map_err(io_error)? {
            let entry = entry.map_err(io_error)?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if name
                .strip_prefix("audit-")
                .and_then(|name| name.strip_suffix(".jsonl"))
                .and_then(|sequence| sequence.parse::<u64>().ok())
                .is_none()
            {
                continue;
            }
            let path = entry.path();
            let mut bytes = Vec::new();
            private_file(&path, false)?
                .read_to_end(&mut bytes)
                .map_err(io_error)?;
            if bytes
                .split(|byte| *byte == b'\n')
                .filter(|line| !line.is_empty())
                .filter_map(|line| serde_json::from_slice::<Value>(line).ok())
                .filter_map(|value| value["_audit_refusal_id"].as_u64())
                .any(|id| unacknowledged.contains(&id))
            {
                continue;
            }
            // Retain the segment until every record in it is past retention,
            // including when the wall clock went backwards between records.
            let newest = bytes
                .split(|byte| *byte == b'\n')
                .filter(|line| !line.is_empty())
                .filter_map(|line| serde_json::from_slice::<Value>(line).ok())
                .filter_map(|line| line["ts"].as_str().and_then(parse_timestamp))
                .max();
            if newest.is_some_and(|newest| now.saturating_sub(newest) > age_limit) {
                std::fs::remove_file(path).map_err(io_error)?;
            }
        }
        Ok(())
    }

    fn scan_chain(&self) -> Result<ChainHead, HostError> {
        let mut segments = Vec::new();
        for entry in std::fs::read_dir(parent(&self.path)).map_err(io_error)? {
            let entry = entry.map_err(io_error)?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if let Some(seq) = name
                .strip_prefix("audit-")
                .and_then(|name| name.strip_suffix(".jsonl"))
                .and_then(|seq| seq.parse::<u64>().ok())
            {
                segments.push((seq, entry.path()));
            }
        }
        segments.sort_by_key(|(seq, _)| *seq);
        let mut head = ChainHead::default();
        for (_, path) in segments {
            scan_file(&path, &mut head, false)?;
        }
        if self.path.exists() {
            scan_file(&self.path, &mut head, true)?;
        }
        Ok(head)
    }
}

#[derive(Debug)]
struct ChainHead {
    seq: u64,
    hash: String,
    active_bytes: u64,
    active_first: u64,
    active_oldest: Option<u64>,
}
impl Default for ChainHead {
    fn default() -> Self {
        Self {
            seq: 0,
            hash: "0".repeat(64),
            active_bytes: 0,
            active_first: 0,
            active_oldest: None,
        }
    }
}

fn parent(path: &Path) -> &Path {
    path.parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}
fn failure(code: &'static str, message: impl Into<String>) -> HostError {
    Diagnostic::error(Code::from_static(code), "audit", message).into()
}
fn io_error(error: std::io::Error) -> HostError {
    failure("audit.io", error.to_string())
}
fn next_seq(seq: u64) -> Result<u64, HostError> {
    seq.checked_add(1)
        .ok_or_else(|| failure("audit.chain-invalid", "Audit sequence exhausted"))
}
fn hex_hash(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
fn validate_context(context: &AuditContext) -> Result<(), HostError> {
    if context.session.is_empty()
        || !["user-tui", "cli", "policy", "kernel", "plugin"].contains(&context.actor.kind.as_str())
        || context.actor.kind == "plugin" && context.actor.plugin.is_none()
    {
        return Err(failure(
            "audit.context-required",
            "Real session and actor provenance are required",
        ));
    }
    if let Some(plugin) = &context.actor.plugin {
        ilium_plugin_api::ids::parse_plugin_id(plugin)?;
    }
    Ok(())
}
fn private_file(path: &Path, create: bool) -> Result<std::fs::File, HostError> {
    let file = secure_fs::private_open_options()
        .read(true)
        .append(true)
        .create(create)
        .open(path)
        .map_err(io_error)?;
    if !file.metadata().map_err(io_error)?.is_file() {
        return Err(failure("audit.io", "Audit path must be a regular file"));
    }
    secure_fs::restrict_open_file_to_owner(&file).map_err(io_error)?;
    Ok(file)
}
fn write_line(
    file: &mut std::fs::File,
    value: &Value,
    seq: u64,
    previous: &str,
) -> Result<String, HostError> {
    let mut value = value.clone();
    value["seq"] = json!(seq);
    value["prev_hash"] = json!(previous);
    let mut bytes = serde_json::to_vec(&value)
        .map_err(|_| failure("audit.record-invalid", "Audit record cannot be serialized"))?;
    let hash = format!("{:x}", Sha256::digest(&bytes));
    bytes.push(b'\n');
    let original = file.metadata().map_err(io_error)?.len();
    if let Err(error) = file.write_all(&bytes).and_then(|()| file.sync_data()) {
        // Roll back only this incomplete append, under the cross-process lock.
        if file
            .set_len(original)
            .and_then(|()| file.sync_data())
            .is_err()
        {
            return Err(failure(
                "audit.partial-write",
                "Audit append failed and its partial tail could not be restored",
            ));
        }
        return Err(io_error(error));
    }
    Ok(hash)
}
fn scan_file(path: &Path, head: &mut ChainHead, active: bool) -> Result<(), HostError> {
    let mut file = private_file(path, false)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(io_error)?;
    if !bytes.is_empty() && !bytes.ends_with(b"\n") {
        return Err(failure(
            "audit.chain-invalid",
            format!("Incomplete audit line after seq {}", head.seq),
        ));
    }
    if bytes.is_empty() {
        return Ok(());
    }
    for line in bytes[..bytes.len() - 1].split(|byte| *byte == b'\n') {
        if line.is_empty() {
            return Err(failure(
                "audit.chain-invalid",
                format!("Empty audit line after seq {}", head.seq),
            ));
        }
        let value: Value = serde_json::from_slice(line).map_err(|_| {
            failure(
                "audit.chain-invalid",
                format!("Invalid audit JSON after seq {}", head.seq),
            )
        })?;
        let seq = value["seq"]
            .as_u64()
            .ok_or_else(|| failure("audit.chain-invalid", "Missing audit sequence"))?;
        let previous = value["prev_hash"]
            .as_str()
            .filter(|hash| hex_hash(hash))
            .ok_or_else(|| failure("audit.chain-invalid", format!("Invalid hash at seq {seq}")))?;
        // Retained history may begin after an expired prefix. Deleted-file resets
        // are explicit anchors, distinguished from a silent sequence gap.
        let reset = seq > head.seq && previous == "0".repeat(64) && value["event"] == "audit-reset";
        if head.seq != 0 && !reset && (seq != next_seq(head.seq)? || previous != head.hash)
            || head.seq == 0 && (seq == 0 || seq == 1 && previous != "0".repeat(64))
        {
            return Err(failure(
                "audit.chain-invalid",
                format!("Broken audit chain at seq {seq}"),
            ));
        }
        let timestamp = value["ts"]
            .as_str()
            .and_then(parse_timestamp)
            .ok_or_else(|| {
                failure(
                    "audit.chain-invalid",
                    format!("Invalid timestamp at seq {seq}"),
                )
            })?;
        if active {
            if head.active_first == 0 {
                head.active_first = seq;
            }
            head.active_oldest = Some(
                head.active_oldest
                    .map_or(timestamp, |oldest| oldest.min(timestamp)),
            );
        }
        head.seq = seq;
        head.hash = format!("{:x}", Sha256::digest(line));
    }
    if active {
        head.active_bytes = bytes.len() as u64;
    }
    Ok(())
}
fn internal_event(source: &Value, event: &str, count: Option<u64>) -> Value {
    let mut value = json!({"ts":source["ts"],"session":source["session"],"actor":{"kind":"kernel"},"event":event,"result":"ok"});
    if let Some(count) = count {
        value["count"] = json!(count);
    }
    value
}
fn redact_record(value: &mut Value, variants: &[String]) {
    for key in ["arguments", "target", "reason", "grant", "session"] {
        if let Some(value) = value.get_mut(key) {
            redact(value, variants);
        }
    }
    if let Some(pane) = value
        .get_mut("actor")
        .and_then(|actor| actor.get_mut("pane"))
    {
        redact(pane, variants);
    }
}
fn redact(value: &mut Value, variants: &[String]) {
    match value {
        Value::String(text) => {
            *text = crate::secrets::redact_variants(text, variants);
        }
        Value::Array(values) => {
            for value in values {
                redact(value, variants);
            }
        }
        Value::Object(values) => {
            for value in values.values_mut() {
                redact(value, variants);
            }
        }
        _ => {}
    }
}
fn unix_millis() -> Result<u64, HostError> {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
        .ok_or_else(|| {
            failure(
                "audit.clock-invalid",
                "Audit wall clock is before Unix epoch or overflows",
            )
        })
}
// Gregorian conversion for the documented bounded ISO-8601 UTC representation.
fn timestamp(millis: u64) -> Result<String, HostError> {
    if millis > 253_402_300_799_999 {
        return Err(failure(
            "audit.clock-invalid",
            "Audit timestamp exceeds year 9999",
        ));
    }
    let days = (millis / 86_400_000) as i64 + 719_468;
    let era = days / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    let seconds = millis / 1000;
    Ok(format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        seconds / 3600 % 24,
        seconds / 60 % 60,
        seconds % 60,
        millis % 1000
    ))
}
fn parse_timestamp(text: &str) -> Option<u64> {
    if text.len() != 24 {
        return None;
    }
    let year: i64 = text.get(0..4)?.parse().ok()?;
    let month: i64 = text.get(5..7)?.parse().ok()?;
    let day: i64 = text.get(8..10)?.parse().ok()?;
    if !(1970..=9999).contains(&year) || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let adjusted = year - i64::from(month <= 2);
    let era = adjusted / 400;
    let y = adjusted - era * 400;
    let days = era * 146_097 + y * 365 + y / 4 - y / 100
        + (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5
        + day
        - 1
        - 719_468;
    let hour: u64 = text.get(11..13)?.parse().ok()?;
    let minute: u64 = text.get(14..16)?.parse().ok()?;
    let second: u64 = text.get(17..19)?.parse().ok()?;
    let millis: u64 = text.get(20..23)?.parse().ok()?;
    if days < 0 || hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    let value =
        days as u64 * 86_400_000 + hour * 3_600_000 + minute * 60_000 + second * 1000 + millis;
    timestamp(value)
        .ok()
        .filter(|canonical| canonical == text)
        .map(|_| value)
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

#[cfg(test)]
mod tests {
    use super::{AuditLog, AuditRecord, EVENT_NAMES};
    use sha2::{Digest, Sha256};
    use std::collections::HashSet;
    use std::path::{Path, PathBuf};

    const MANUAL_AUDIT_LOG_PAGE: &str =
        include_str!("../../docs/plugins-manual/pages/reference-audit-log.md");

    fn sample_record(action: &str) -> AuditRecord {
        AuditRecord {
            at_unix_millis: 1_786_000_000_000,
            plugin: "example.notes".to_owned(),
            action: action.to_owned(),
            detail_json: "{}".to_owned(),
        }
    }

    fn audit_file_path(directory: &Path) -> PathBuf {
        directory.join("audit.jsonl")
    }

    fn open_log(log_path: &Path) -> AuditLog {
        AuditLog::open(log_path.to_path_buf()).expect("the audit log opens")
    }

    fn append_all(log_path: &Path, records: &[AuditRecord]) {
        let mut log = open_log(log_path);
        for record in records {
            log.append(record).expect("the record is appended");
        }
    }

    fn read_lines(log_path: &Path) -> Vec<String> {
        std::fs::read_to_string(log_path)
            .expect("the audit file exists and is readable")
            .lines()
            .map(str::to_owned)
            .collect()
    }

    fn parse_line_as_json(line: &str) -> serde_json::Value {
        serde_json::from_str(line).expect("each audit line is one JSON object")
    }

    fn sha256_hex(bytes: &[u8]) -> String {
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    /// Reads the event names from the "| Event | When |" table of the manual page.
    fn catalogue_names_in_manual() -> Vec<String> {
        MANUAL_AUDIT_LOG_PAGE
            .lines()
            .skip_while(|line| !line.starts_with("| Event | When |"))
            .skip(2)
            .take_while(|line| line.starts_with('|'))
            .filter_map(|line| line.split('|').nth(1))
            .flat_map(|cell| {
                cell.split(',')
                    .map(|name| name.trim().trim_matches('`').to_owned())
                    .collect::<Vec<String>>()
            })
            .filter(|name| !name.is_empty())
            .collect()
    }

    /// REQ: SEC-222
    #[test]
    fn sec_222_event_names_match_the_manual_catalogue_in_table_order() {
        let manual_names = catalogue_names_in_manual();
        assert!(
            !manual_names.is_empty(),
            "the manual's event catalogue table is found and parsed"
        );
        let event_names: Vec<String> = EVENT_NAMES.iter().map(|name| (*name).to_owned()).collect();
        assert_eq!(event_names, manual_names);
    }

    /// REQ: SEC-222
    #[test]
    fn sec_222_event_names_are_unique() {
        let mut seen_names = HashSet::new();
        for name in EVENT_NAMES {
            assert!(seen_names.insert(*name), "duplicate event name {name}");
        }
    }

    /// REQ: SEC-222
    #[test]
    fn sec_222_event_names_are_nonempty_lowercase_kebab_case() {
        for name in EVENT_NAMES {
            assert!(!name.is_empty(), "an event name is empty");
            assert!(
                name.bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte == b'-'),
                "event name {name} is not lowercase kebab-case"
            );
            assert!(
                !name.starts_with('-') && !name.ends_with('-'),
                "event name {name} starts or ends with a hyphen"
            );
        }
    }

    /// REQ: SEC-220
    #[test]
    fn sec_220_append_writes_the_audit_file_at_the_given_path() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let log_path = audit_file_path(directory.path());
        append_all(&log_path, &[sample_record("install")]);
        assert!(
            log_path.is_file(),
            "audit.jsonl exists after the first append"
        );
    }

    /// REQ: SEC-221
    #[test]
    fn sec_221_append_writes_one_json_object_per_line_with_trailing_newline() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let log_path = audit_file_path(directory.path());
        append_all(&log_path, &[sample_record("install")]);
        let raw_content = std::fs::read_to_string(&log_path).expect("the audit file is readable");
        assert!(raw_content.ends_with('\n'), "the line ends with a newline");
        assert_eq!(raw_content.lines().count(), 1, "one record is one line");
        assert!(parse_line_as_json(raw_content.trim_end_matches('\n')).is_object());
    }

    /// REQ: SEC-221
    #[test]
    fn sec_221_appended_records_read_back_unchanged_and_in_order() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let log_path = audit_file_path(directory.path());
        let records = vec![
            sample_record("install"),
            sample_record("grant-given"),
            sample_record("enable"),
        ];
        append_all(&log_path, &records);
        let lines = read_lines(&log_path);
        assert_eq!(lines.len(), records.len());
        for (line, record) in lines.iter().zip(records.iter()) {
            let read_back: AuditRecord =
                serde_json::from_str(line).expect("each line deserializes as an audit record");
            assert_eq!(&read_back, record);
        }
    }

    /// REQ: SEC-221
    #[test]
    fn sec_221_reopened_log_keeps_existing_bytes_and_appends_after_them() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let log_path = audit_file_path(directory.path());
        append_all(&log_path, &[sample_record("install")]);
        let bytes_before_reopen = std::fs::read(&log_path).expect("the audit file is readable");
        append_all(&log_path, &[sample_record("enable")]);
        let bytes_after_reopen = std::fs::read(&log_path).expect("the audit file is readable");
        assert!(
            bytes_after_reopen.starts_with(&bytes_before_reopen),
            "earlier bytes are never rewritten"
        );
        assert_eq!(read_lines(&log_path).len(), 2);
    }

    /// REQ: SEC-221
    #[test]
    fn sec_221_detail_containing_newlines_stays_on_one_line() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let log_path = audit_file_path(directory.path());
        let record = AuditRecord {
            at_unix_millis: 1,
            plugin: "example.notes".to_owned(),
            action: "process-run".to_owned(),
            detail_json: "first line\nsecond line".to_owned(),
        };
        append_all(&log_path, std::slice::from_ref(&record));
        let lines = read_lines(&log_path);
        assert_eq!(
            lines.len(),
            1,
            "an embedded newline is escaped inside the JSON line"
        );
        let read_back: AuditRecord =
            serde_json::from_str(&lines[0]).expect("the line deserializes as an audit record");
        assert_eq!(read_back, record);
    }

    /// REQ: SEC-221
    #[test]
    fn sec_221_empty_and_extreme_field_values_round_trip() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let log_path = audit_file_path(directory.path());
        let empty_record = AuditRecord {
            at_unix_millis: 0,
            plugin: String::new(),
            action: String::new(),
            detail_json: String::new(),
        };
        let extreme_record = AuditRecord {
            at_unix_millis: u64::MAX,
            plugin: "x".repeat(4096),
            action: "audit-dropped".to_owned(),
            detail_json: "{\"count\":18446744073709551615}".to_owned(),
        };
        append_all(&log_path, &[empty_record.clone(), extreme_record.clone()]);
        let lines = read_lines(&log_path);
        assert_eq!(lines.len(), 2);
        let first: AuditRecord = serde_json::from_str(&lines[0]).expect("first line parses");
        let second: AuditRecord = serde_json::from_str(&lines[1]).expect("second line parses");
        assert_eq!(first, empty_record);
        assert_eq!(second, extreme_record);
    }

    /// REQ: SEC-221
    #[test]
    fn sec_221_seq_starts_at_one_and_increases_by_one_across_reopen() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let log_path = audit_file_path(directory.path());
        append_all(
            &log_path,
            &[sample_record("install"), sample_record("enable")],
        );
        append_all(&log_path, &[sample_record("disable")]);
        let lines = read_lines(&log_path);
        assert_eq!(lines.len(), 3);
        for (index, line) in lines.iter().enumerate() {
            let value = parse_line_as_json(line);
            assert_eq!(
                value["seq"].as_u64(),
                Some(index as u64 + 1),
                "line {}",
                index + 1
            );
        }
    }

    /// REQ: SEC-221
    #[test]
    fn sec_221_first_record_prev_hash_is_sixty_four_zeros() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let log_path = audit_file_path(directory.path());
        append_all(&log_path, &[sample_record("install")]);
        let lines = read_lines(&log_path);
        let first_line = lines.first().expect("the first record is written");
        let zero_hash = "0".repeat(64);
        assert_eq!(
            parse_line_as_json(first_line)["prev_hash"].as_str(),
            Some(zero_hash.as_str())
        );
    }

    /// REQ: SEC-221
    #[test]
    fn sec_221_each_prev_hash_is_sha256_of_the_previous_line_without_newline() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let log_path = audit_file_path(directory.path());
        append_all(
            &log_path,
            &[sample_record("install"), sample_record("enable")],
        );
        append_all(&log_path, &[sample_record("disable")]);
        let lines = read_lines(&log_path);
        assert_eq!(lines.len(), 3);
        for index in 1..lines.len() {
            let expected_hash = sha256_hex(lines[index - 1].as_bytes());
            assert_eq!(
                parse_line_as_json(&lines[index])["prev_hash"].as_str(),
                Some(expected_hash.as_str()),
                "record {} links to record {}",
                index + 1,
                index
            );
        }
    }

    /// REQ: SEC-224
    #[test]
    fn sec_224_deleted_log_file_is_recreated_with_audit_reset_and_zero_prev_hash() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let log_path = audit_file_path(directory.path());
        let mut log = open_log(&log_path);
        log.append(&sample_record("install"))
            .expect("the first record is appended");
        std::fs::remove_file(&log_path).expect("the audit file is removed");
        log.append(&sample_record("enable"))
            .expect("the record after deletion is appended");
        let lines = read_lines(&log_path);
        assert!(
            lines.iter().any(|line| line.contains("audit-reset")),
            "a recreated file starts with an audit-reset record"
        );
        let first_line = lines.first().expect("the recreated file has a first line");
        let zero_hash = "0".repeat(64);
        assert_eq!(
            parse_line_as_json(first_line)["prev_hash"].as_str(),
            Some(zero_hash.as_str())
        );
    }
}

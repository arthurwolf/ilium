//! Resolves Claude Code, Codex, and Antigravity CLI transcript identities at
//! their shared filesystem boundary. Both `ilium-server` session discovery and
//! `ilium-client` title inference use this crate so an ID cannot be accepted
//! under one set of path rules and later read under a weaker set.
//!
//! A filename is never sufficient evidence by itself. Claude project slugs
//! are lossy (`_` and `-` both become `-`), while Codex stores every project
//! together under date directories. Every returned transcript therefore has
//! metadata that proves both its session ID and its canonical project cwd.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use ilium_core::AgentClass;
use serde_json::Value;

mod request_evidence;
pub use request_evidence::{
    genuine_request_text, is_codex_injected_message, GenuineRequestEvidence,
};

/// A transcript whose filename, embedded session ID, and project cwd agree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedTranscript {
    pub session_id: String,
    pub path: PathBuf,
}

/// Raw first-record evidence, not a verified transcript or exclusive claim.
/// Canonical project/store checks belong to the filesystem adapter. For an
/// Antigravity history record, a missing workspace is authoritative refusal
/// for that conversation; a later record must not repair that first binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptIdentity {
    pub session_id: String,
    pub project_cwd: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetadataParseFailure {
    Cancelled,
    AdmissionRefused,
    WorkerFailed,
}

/// Caller-owned CPU service. Ownership of the actual bounded line transfers
/// to this service; it returns only typed identity evidence. No library pool,
/// parser thread, or implicit unbounded channel is created by the locator.
pub trait TranscriptMetadataParser: std::fmt::Debug + Send + Sync {
    fn parse(
        &self,
        class: &AgentClass,
        line: Vec<u8>,
    ) -> Result<Option<TranscriptIdentity>, MetadataParseFailure>;
}

/// Parses one already-admitted metadata line without accessing the filesystem.
/// Worker callers run this on CPU, reserving parser peak and result storage
/// before submission. None means no authoritative record in this line, never
/// proof of unique ownership or completion of a bounded discovery attempt.
pub fn parse_transcript_identity(class: &AgentClass, line: &[u8]) -> Option<TranscriptIdentity> {
    let entry = serde_json::from_slice::<Value>(line).ok()?;
    let (session_id, project_cwd) = match class {
        AgentClass::Claude => {
            let (session_id, cwd) = claude_identity(&entry)?;
            (session_id, Some(cwd))
        }
        AgentClass::Codex => {
            let (session_id, cwd) = codex_identity(&entry)?;
            (session_id, Some(cwd))
        }
        AgentClass::Antigravity => (
            entry.get("conversationId")?.as_str()?,
            entry.get("workspace").and_then(Value::as_str),
        ),
        AgentClass::Other(_) => return None,
    };
    Some(TranscriptIdentity {
        session_id: session_id.to_string(),
        project_cwd: project_cwd.map(str::to_string),
    })
}

/// Cooperative upper bounds for one complete transcript-discovery attempt.
#[derive(Debug, Clone, Copy)]
pub struct TranscriptReadLimits {
    pub line_bytes: usize,
    pub total_read_bytes: usize,
    pub scanned_entries: usize,
    pub retained_path_bytes: usize,
}

/// Budget for one interactive lookup (context-menu history path, pane
/// transcript actions). Codex keeps every rollout under `sessions/YYYY/MM/DD`,
/// so the whole store is walked on each lookup. Sized well above a multi-year
/// store (a live store had ~8.2k entries) while still bounding the walk.
pub const INTERACTIVE_TRANSCRIPT_LOOKUP_LIMITS: TranscriptReadLimits = TranscriptReadLimits {
    line_bytes: 64 * 1024,
    total_read_bytes: 8 * 1024 * 1024,
    scanned_entries: 262_144,
    retained_path_bytes: 1024 * 1024,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptReadLimitFailure {
    pub resource: &'static str,
    pub used: usize,
    pub requested: usize,
    pub limit: usize,
}
impl std::fmt::Display for TranscriptReadLimitFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let observed = self.used.saturating_add(self.requested);
        write!(
            formatter,
            "{resource} limit reached: used {used}, next item requires {requested}, limit {limit}, observed at least {observed} (at least {over} over); remaining discovery work was not measured",
            resource = self.resource,
            used = self.used,
            requested = self.requested,
            limit = self.limit,
            observed = observed,
            over = observed.saturating_sub(self.limit),
        )
    }
}
#[derive(Debug)]
struct TranscriptReadBudget {
    limits: TranscriptReadLimits,
    read_bytes: std::sync::atomic::AtomicUsize,
    staged_fetched_bytes: std::sync::atomic::AtomicUsize,
    staged_lines: std::sync::atomic::AtomicUsize,
    staged_jobs: std::sync::atomic::AtomicUsize,
    entries: std::sync::atomic::AtomicUsize,
    path_bytes: std::sync::atomic::AtomicUsize,
    exhausted: std::sync::atomic::AtomicBool,
    parser_failure: std::sync::atomic::AtomicU8,
    limit_failure: std::sync::Mutex<Option<TranscriptReadLimitFailure>>,
}
impl TranscriptReadBudget {
    fn new(limits: TranscriptReadLimits) -> Self {
        Self {
            limits,
            read_bytes: 0.into(),
            staged_fetched_bytes: 0.into(),
            staged_lines: 0.into(),
            staged_jobs: 0.into(),
            entries: 0.into(),
            path_bytes: 0.into(),
            exhausted: false.into(),
            parser_failure: 0.into(),
            limit_failure: std::sync::Mutex::new(None),
        }
    }
    fn charge(
        &self,
        counter: &std::sync::atomic::AtomicUsize,
        amount: usize,
        limit: usize,
        resource: &'static str,
    ) -> bool {
        use std::sync::atomic::Ordering;
        if self.exhausted.load(Ordering::Acquire) {
            return false;
        }
        if counter
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                used.checked_add(amount).filter(|total| *total <= limit)
            })
            .is_ok()
        {
            return true;
        }
        let used = counter.load(Ordering::Acquire);
        self.record_limit_failure(resource, used, amount, limit);
        false
    }
    fn record_limit_failure(
        &self,
        resource: &'static str,
        used: usize,
        requested: usize,
        limit: usize,
    ) {
        if let Ok(mut failure) = self.limit_failure.lock() {
            failure.get_or_insert(TranscriptReadLimitFailure {
                resource,
                used,
                requested,
                limit,
            });
        }
        self.exhausted
            .store(true, std::sync::atomic::Ordering::Release);
    }
    fn line_limit_failure(&self, used: usize, requested: usize) {
        self.record_limit_failure(
            "transcript line bytes",
            used,
            requested,
            self.limits.line_bytes,
        );
    }
    fn fail(&self) -> std::io::Error {
        self.exhausted
            .store(true, std::sync::atomic::Ordering::Release);
        std::io::Error::other("transcript discovery resource limit reached")
    }
    fn note_parser_failure(&self, reason: MetadataParseFailure) {
        use std::sync::atomic::Ordering;
        let value = match reason {
            MetadataParseFailure::Cancelled => 1,
            MetadataParseFailure::AdmissionRefused => 2,
            MetadataParseFailure::WorkerFailed => 3,
        };
        let _ = self
            .parser_failure
            .compare_exchange(0, value, Ordering::AcqRel, Ordering::Acquire);
        self.exhausted.store(true, Ordering::Release);
    }
}
fn parse_bounded_identity(
    class: &AgentClass,
    line: Vec<u8>,
    budget: &TranscriptReadBudget,
    parser: Option<&dyn TranscriptMetadataParser>,
) -> Result<Option<TranscriptIdentity>, MetadataParseFailure> {
    let result = match parser {
        Some(parser) => parser.parse(class, line),
        None => Ok(parse_transcript_identity(class, &line)),
    };
    if let Err(reason) = &result {
        budget.note_parser_failure(*reason);
    }
    result
}
fn bounded_line(
    reader: &mut impl BufRead,
    budget: &TranscriptReadBudget,
) -> std::io::Result<Option<Vec<u8>>> {
    let mut line = Vec::new();
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return Ok((!line.is_empty()).then_some(line));
        }
        let count = available
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(available.len(), |position| position + 1);
        if line.len().saturating_add(count) > budget.limits.line_bytes {
            budget.line_limit_failure(line.len(), count);
            return Err(budget.fail());
        }
        if !budget.charge(
            &budget.read_bytes,
            count,
            budget.limits.total_read_bytes,
            "transcript bytes",
        ) {
            return Err(budget.fail());
        }
        let ends_line = available[count - 1] == b'\n';
        line.extend_from_slice(&available[..count]);
        reader.consume(count);
        if ends_line {
            return Ok(Some(line));
        }
    }
}
fn transcript_metadata_matches_bounded(
    class: &AgentClass,
    path: &Path,
    expected_session_id: &str,
    expected_project_cwd: &Path,
    budget: &TranscriptReadBudget,
    parser: Option<&dyn TranscriptMetadataParser>,
) -> bool {
    let Ok(file) = ilium_platform::secure_fs::open_regular_file(path) else {
        return false;
    };
    let mut reader = BufReader::new(file);
    while let Ok(Some(line)) = bounded_line(&mut reader, budget) {
        let Ok(identity) = parse_bounded_identity(class, line, budget, parser) else {
            return false;
        };
        if let Some(identity) = identity {
            return identity.session_id == expected_session_id
                && identity.project_cwd.as_deref().is_some_and(|cwd| {
                    same_canonical_project(Path::new(cwd), expected_project_cwd)
                });
        }
    }
    false
}
fn bounded_candidate_paths(
    directory: &Path,
    recursive: bool,
    extension: &str,
    class: &AgentClass,
    session_id: &str,
    budget: &TranscriptReadBudget,
) -> Vec<PathBuf> {
    let mut pending = vec![directory.to_owned()];
    let mut paths = Vec::new();
    while let Some(directory) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(directory) else {
            continue;
        };
        // Codex stores are `YYYY/MM/DD`, so zero-padded names sort
        // chronologically. Visiting newest first lets recent transcripts be
        // found first. The walk stays exhaustive: a single match is never
        // accepted before every entry is charged, so ambiguity is still caught.
        let mut child_directories = Vec::new();
        let mut matching_paths = Vec::new();
        for entry in entries {
            if !budget.charge(
                &budget.entries,
                1,
                budget.limits.scanned_entries,
                "filesystem entries",
            ) {
                return Vec::new();
            }
            let Ok(entry) = entry else {
                continue;
            };
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if !(kind.is_file() || recursive && kind.is_dir()) {
                continue;
            }
            if kind.is_dir() {
                let path = entry.path();
                if !budget.charge(
                    &budget.path_bytes,
                    path.capacity()
                        .saturating_mul(2)
                        .saturating_add(std::mem::size_of::<PathBuf>() * 2),
                    budget.limits.retained_path_bytes,
                    "retained path bytes",
                ) {
                    return Vec::new();
                }
                child_directories.push(path);
            } else if entry.path().extension().and_then(|value| value.to_str()) == Some(extension)
                && session_id_from_transcript_path(class, &entry.path()).as_deref()
                    == Some(session_id)
            {
                let path = entry.path();
                if !budget.charge(
                    &budget.path_bytes,
                    path.capacity()
                        .saturating_mul(2)
                        .saturating_add(std::mem::size_of::<PathBuf>() * 2),
                    budget.limits.retained_path_bytes,
                    "retained path bytes",
                ) {
                    return Vec::new();
                }
                matching_paths.push(path);
            }
        }
        // `pending` is a stack: pushing ascending names pops the newest first.
        child_directories.sort();
        pending.extend(child_directories);
        matching_paths.sort_by(|left, right| right.cmp(left));
        paths.extend(matching_paths);
    }
    paths
}

/// Project-scoped access to the local Claude Code, Codex, and Antigravity
/// transcript stores.
#[derive(Debug, Clone)]
pub struct TranscriptLocator {
    home: PathBuf,
    project_cwd: PathBuf,
    read_budget: Option<std::sync::Arc<TranscriptReadBudget>>,
    metadata_parser: Option<std::sync::Arc<dyn TranscriptMetadataParser>>,
}

/// A single open, bounded metadata stream. The file and all filesystem
/// decisions stay with the admitted I/O job; CPU jobs see only bounded raw
/// lines and parse them in their original order.
#[derive(Debug)]
pub struct StagedMetadataCursor {
    locator: TranscriptLocator,
    class: AgentClass,
    path: PathBuf,
    session_id: String,
    reader: BufReader<std::fs::File>,
    deferred_limit: Option<TranscriptReadLimitFailure>,
    ended: bool,
}

/// One transfer holds at most 256 lines, 128 KiB of short lines plus one
/// permitted 1 MiB line. The CPU result charges only the prefix it inspected:
/// read-ahead after an authoritative record cannot consume a later rank's
/// shared budget or turn a valid first record into an oversized-tail failure.
const STAGED_BATCH_LINES: usize = 256;
const STAGED_BATCH_BYTES: usize = 128 * 1024;
pub const STAGED_METADATA_LINE_LIMIT: usize = 16_384;
pub const STAGED_METADATA_JOB_LIMIT: usize = 2_048;

#[derive(Debug)]
pub enum StagedMetadataStep {
    Batch {
        cursor: StagedMetadataCursor,
        lines: Vec<Vec<u8>>,
    },
    Finished(Option<VerifiedTranscript>),
}

#[derive(Debug)]
pub struct StagedBatchParse {
    identity: Option<TranscriptIdentity>,
    consumed_lines: usize,
    consumed_bytes: usize,
}

impl StagedMetadataCursor {
    /// Pure CPU work: the first Claude/Codex identity record, or the first
    /// Antigravity binding for this conversation, is authoritative. A malformed
    /// line has no identity and cannot terminate the scan.
    pub fn parse_batch(&self, lines: &[Vec<u8>]) -> StagedBatchParse {
        let mut consumed_bytes = 0usize;
        for (index, line) in lines.iter().enumerate() {
            consumed_bytes = consumed_bytes.saturating_add(line.len());
            let Some(identity) = parse_transcript_identity(&self.class, line) else {
                continue;
            };
            if matches!(&self.class, AgentClass::Antigravity)
                && identity.session_id != self.session_id
            {
                continue;
            }
            return StagedBatchParse {
                identity: Some(identity),
                consumed_lines: index + 1,
                consumed_bytes,
            };
        }
        StagedBatchParse {
            identity: None,
            consumed_lines: lines.len(),
            consumed_bytes,
        }
    }

    /// Called only by an admitted I/O job. The previous CPU result commits
    /// exactly its inspected prefix before canonical path comparison or the
    /// next read. A deferred later-line failure matters only when no earlier
    /// authoritative record was found in the same batch.
    pub fn advance(mut self, parsed: Option<StagedBatchParse>) -> StagedMetadataStep {
        let Some(budget) = self.locator.read_budget.as_deref() else {
            return StagedMetadataStep::Finished(None);
        };
        if self.locator.read_limit_reached() {
            return StagedMetadataStep::Finished(None);
        }
        if let Some(parsed) = parsed {
            if parsed.consumed_lines == 0
                || !budget.charge(
                    &budget.staged_lines,
                    parsed.consumed_lines,
                    STAGED_METADATA_LINE_LIMIT,
                    "staged metadata lines",
                )
                || !budget.charge(
                    &budget.read_bytes,
                    parsed.consumed_bytes,
                    budget.limits.total_read_bytes,
                    "transcript bytes",
                )
            {
                return StagedMetadataStep::Finished(None);
            }
            if let Some(identity) = parsed.identity {
                let matches = identity.session_id == self.session_id
                    && identity.project_cwd.as_deref().is_some_and(|cwd| {
                        same_canonical_project(Path::new(cwd), &self.locator.project_cwd)
                    });
                return StagedMetadataStep::Finished(matches.then(|| VerifiedTranscript {
                    session_id: self.session_id,
                    path: self.path,
                }));
            }
            if let Some(failure) = self.deferred_limit.take() {
                budget.record_limit_failure(
                    failure.resource,
                    failure.used,
                    failure.requested,
                    failure.limit,
                );
                return StagedMetadataStep::Finished(None);
            }
            if self.ended {
                return StagedMetadataStep::Finished(None);
            }
        }
        self.read_batch()
    }

    fn read_batch(mut self) -> StagedMetadataStep {
        let Some(budget) = self.locator.read_budget.as_deref() else {
            return StagedMetadataStep::Finished(None);
        };
        let committed_bytes = budget.read_bytes.load(std::sync::atomic::Ordering::Acquire);
        // Fetched read-ahead is also subject to the shared attempt byte cap.
        // Only the CPU-inspected prefix is committed to the logical counter.
        budget
            .staged_fetched_bytes
            .fetch_max(committed_bytes, std::sync::atomic::Ordering::AcqRel);
        let committed_lines = budget
            .staged_lines
            .load(std::sync::atomic::Ordering::Acquire);
        let mut lines = Vec::new();
        let mut batch_bytes = 0usize;
        while lines.len() < STAGED_BATCH_LINES && batch_bytes < STAGED_BATCH_BYTES {
            let current_lines = committed_lines.saturating_add(lines.len());
            if current_lines >= STAGED_METADATA_LINE_LIMIT {
                match self.reader.fill_buf() {
                    Ok([]) => self.ended = true,
                    Ok(_) => {
                        self.deferred_limit = Some(TranscriptReadLimitFailure {
                            resource: "staged metadata lines",
                            used: current_lines,
                            requested: 1,
                            limit: STAGED_METADATA_LINE_LIMIT,
                        });
                    }
                    Err(_) => self.ended = true,
                }
                break;
            }
            let mut line = Vec::new();
            let mut eof = false;
            loop {
                let available = match self.reader.fill_buf() {
                    Ok(available) => available,
                    Err(_) => {
                        self.ended = true;
                        break;
                    }
                };
                if available.is_empty() {
                    eof = true;
                    break;
                }
                let count = available
                    .iter()
                    .position(|byte| *byte == b'\n')
                    .map_or(available.len(), |position| position + 1);
                if line.len().saturating_add(count) > budget.limits.line_bytes {
                    self.deferred_limit = Some(TranscriptReadLimitFailure {
                        resource: "transcript line bytes",
                        used: line.len(),
                        requested: count,
                        limit: budget.limits.line_bytes,
                    });
                    break;
                }
                if committed_bytes
                    .saturating_add(batch_bytes)
                    .saturating_add(line.len())
                    .saturating_add(count)
                    > budget.limits.total_read_bytes
                {
                    self.deferred_limit = Some(TranscriptReadLimitFailure {
                        resource: "transcript bytes",
                        used: committed_bytes
                            .saturating_add(batch_bytes)
                            .saturating_add(line.len()),
                        requested: count,
                        limit: budget.limits.total_read_bytes,
                    });
                    break;
                }
                if budget
                    .staged_fetched_bytes
                    .fetch_update(
                        std::sync::atomic::Ordering::AcqRel,
                        std::sync::atomic::Ordering::Acquire,
                        |used| {
                            used.checked_add(count)
                                .filter(|total| *total <= budget.limits.total_read_bytes)
                        },
                    )
                    .is_err()
                {
                    self.deferred_limit = Some(TranscriptReadLimitFailure {
                        resource: "transcript bytes",
                        used: budget
                            .staged_fetched_bytes
                            .load(std::sync::atomic::Ordering::Acquire),
                        requested: count,
                        limit: budget.limits.total_read_bytes,
                    });
                    break;
                }
                let ends_line = available[count - 1] == b'\n';
                line.extend_from_slice(&available[..count]);
                self.reader.consume(count);
                if ends_line {
                    break;
                }
            }
            if !line.is_empty() && self.deferred_limit.is_none() {
                batch_bytes = batch_bytes.saturating_add(line.len());
                lines.push(line);
            }
            if eof || self.ended {
                self.ended = true;
                break;
            }
            if self.deferred_limit.is_some() {
                break;
            }
        }
        if lines.is_empty() {
            if let Some(failure) = self.deferred_limit {
                budget.record_limit_failure(
                    failure.resource,
                    failure.used,
                    failure.requested,
                    failure.limit,
                );
            }
            StagedMetadataStep::Finished(None)
        } else {
            StagedMetadataStep::Batch {
                cursor: self,
                lines,
            }
        }
    }
}

impl TranscriptLocator {
    /// Builds one locator for a canonical ilium project session.
    pub fn new(home: &Path, project_cwd: &Path) -> Self {
        Self {
            home: home.to_path_buf(),
            project_cwd: canonical_or_original(project_cwd),
            read_budget: None,
            metadata_parser: None,
        }
    }

    /// Bounds discovery across every locator operation in one evidence job.
    /// Exhaustion is sticky: a partial scan must never prove unique ownership.
    pub fn new_bounded(home: &Path, project_cwd: &Path, limits: TranscriptReadLimits) -> Self {
        let mut locator = Self::new(home, project_cwd);
        locator.read_budget = Some(std::sync::Arc::new(TranscriptReadBudget::new(limits)));
        locator
    }
    pub fn with_read_limits(&self, limits: TranscriptReadLimits) -> Self {
        Self {
            home: self.home.clone(),
            project_cwd: self.project_cwd.clone(),
            read_budget: Some(std::sync::Arc::new(TranscriptReadBudget::new(limits))),
            metadata_parser: self.metadata_parser.clone(),
        }
    }
    pub fn read_limits(&self) -> Option<TranscriptReadLimits> {
        self.read_budget.as_ref().map(|budget| budget.limits)
    }
    /// Installs an explicitly owned CPU parser only on a bounded attempt.
    /// The caller must run discovery on its admitted I/O owner and provide a
    /// parser service with independent cancellation and completion custody.
    pub fn with_metadata_parser(
        mut self,
        parser: std::sync::Arc<dyn TranscriptMetadataParser>,
    ) -> Result<Self, MetadataParseFailure> {
        if self.read_budget.is_none() {
            return Err(MetadataParseFailure::AdmissionRefused);
        }
        self.metadata_parser = Some(parser);
        Ok(self)
    }
    pub fn metadata_parse_failure(&self) -> Option<MetadataParseFailure> {
        let value = self
            .read_budget
            .as_ref()?
            .parser_failure
            .load(std::sync::atomic::Ordering::Acquire);
        match value {
            1 => Some(MetadataParseFailure::Cancelled),
            2 => Some(MetadataParseFailure::AdmissionRefused),
            3 => Some(MetadataParseFailure::WorkerFailed),
            _ => None,
        }
    }
    /// Propagates a failed CPU admission, cancellation, or lost worker to the
    /// same sticky bounded attempt that owns all candidate path/read limits.
    pub fn mark_metadata_parse_failure(&self, reason: MetadataParseFailure) {
        if let Some(budget) = &self.read_budget {
            budget.note_parser_failure(reason);
        }
    }

    /// Enumerate exactly the same format-shaped candidate paths as the
    /// synchronous lookup. Call on an admitted I/O job and retain this
    /// locator across all ranks in one attempt so budgets stay cumulative.
    pub fn staged_candidate_paths_for_session(
        &self,
        class: &AgentClass,
        session_id: &str,
    ) -> Vec<PathBuf> {
        if self.read_budget.is_none() || !looks_like_uuid(session_id) {
            return Vec::new();
        }
        self.candidate_paths_for_session(class, session_id)
    }

    /// Opens a format- and store-checked candidate on an admitted I/O job.
    /// Antigravity database bytes are never interpreted as JSON: its binding
    /// comes from the provider's history file, as in transcript_from_path.
    pub fn begin_staged_metadata(
        &self,
        class: &AgentClass,
        path: &Path,
    ) -> Option<StagedMetadataCursor> {
        self.read_budget.as_ref()?;
        let session_id = session_id_from_transcript_path(class, path)?;
        if !self.path_is_in_expected_store(class, path) {
            return None;
        }
        let metadata_path = if matches!(class, AgentClass::Antigravity) {
            self.home
                .join(".gemini")
                .join("antigravity-cli")
                .join("history.jsonl")
        } else {
            path.to_path_buf()
        };
        let file = ilium_platform::secure_fs::open_regular_file(&metadata_path).ok()?;
        Some(StagedMetadataCursor {
            locator: self.clone(),
            class: class.clone(),
            path: path.to_path_buf(),
            session_id,
            reader: BufReader::new(file),
            deferred_limit: None,
            ended: false,
        })
    }
    /// Adapter overflow invalidates every partial ownership conclusion in the
    /// same bounded attempt, including stronger ranks evaluated earlier.
    /// Claim the finite coordinator jobs for one bounded attempt before
    /// submitting them. The caller may preclaim the CPU and following I/O
    /// handoff together so a valid parsed record always gets verified.
    pub fn claim_staged_jobs(&self, jobs: usize) -> bool {
        self.read_budget.as_deref().is_some_and(|budget| {
            budget.charge(
                &budget.staged_jobs,
                jobs,
                STAGED_METADATA_JOB_LIMIT,
                "staged metadata jobs",
            )
        })
    }

    pub fn mark_read_limit_reached(&self) {
        if let Some(budget) = &self.read_budget {
            budget
                .exhausted
                .store(true, std::sync::atomic::Ordering::Release);
        }
    }
    pub fn read_limit_reached(&self) -> bool {
        self.read_budget
            .as_ref()
            .is_some_and(|budget| budget.exhausted.load(std::sync::atomic::Ordering::Acquire))
    }
    pub fn read_limit_failure(&self) -> Option<TranscriptReadLimitFailure> {
        self.read_budget
            .as_ref()?
            .limit_failure
            .lock()
            .ok()?
            .clone()
    }

    /// Finds exactly one verified transcript for `session_id` in this project.
    /// Duplicate valid paths are treated as ambiguous instead of choosing by
    /// filesystem iteration order.
    pub fn transcript_for_session(
        &self,
        class: &AgentClass,
        session_id: &str,
    ) -> Option<VerifiedTranscript> {
        if !looks_like_uuid(session_id) {
            return None;
        }

        // `transcript_from_path` re-derives its session id from the same
        // `path` the preceding filter already constrained to `session_id`,
        // so every yielded transcript is already known to match; no further
        // filtering on `transcript.session_id` is needed here.
        let mut matches = self
            .candidate_paths_for_session(class, session_id)
            .into_iter()
            .filter_map(|path| self.transcript_from_path(class, &path));
        let transcript = matches.next()?;
        if matches.next().is_some() || self.read_limit_reached() {
            return None;
        }
        Some(transcript)
    }

    /// Verifies one known path, used by the server's `/proc/<pid>/fd` rank.
    pub fn transcript_from_path(
        &self,
        class: &AgentClass,
        path: &Path,
    ) -> Option<VerifiedTranscript> {
        let session_id = session_id_from_transcript_path(class, path)?;
        if !self.path_is_in_expected_store(class, path) {
            return None;
        }
        // Antigravity ownership is proven solely by `history.jsonl`; the `.db`
        // file is binary SQLite and must never be opened as line metadata.
        if matches!(class, AgentClass::Antigravity) {
            if !self.antigravity_history_matches(&session_id) {
                return None;
            }
            return Some(VerifiedTranscript {
                session_id,
                path: path.to_path_buf(),
            });
        }
        let matches = match &self.read_budget {
            Some(budget) => transcript_metadata_matches_bounded(
                class,
                path,
                &session_id,
                &self.project_cwd,
                budget,
                self.metadata_parser.as_deref(),
            ),
            None => transcript_metadata_matches(class, path, &session_id, &self.project_cwd),
        };
        if !matches || self.read_limit_reached() {
            return None;
        }
        Some(VerifiedTranscript {
            session_id,
            path: path.to_path_buf(),
        })
    }

    /// Reports genuine-request evidence only after path, session ID, and
    /// project cwd have been verified by the same locator. Missing or
    /// unreadable history is never treated as a verified empty conversation.
    pub fn genuine_request_evidence(
        &self,
        class: &AgentClass,
        session_id: &str,
    ) -> std::io::Result<GenuineRequestEvidence> {
        let Some(transcript) = self.transcript_for_session(class, session_id) else {
            return Ok(GenuineRequestEvidence::Unavailable);
        };
        match self.read_budget.as_deref() {
            Some(budget) => request_evidence::request_evidence_from_path_with_budget(
                class,
                &transcript.path,
                &transcript.session_id,
                budget,
            ),
            None => request_evidence::request_evidence_from_path(
                class,
                &transcript.path,
                &transcript.session_id,
            ),
        }
    }

    /// Enumerates format-shaped paths without making any ownership claim.
    fn candidate_paths(&self, class: &AgentClass) -> Vec<PathBuf> {
        match class {
            AgentClass::Claude => transcript_files_directly_under(&self.claude_project_dir()),
            AgentClass::Codex => transcript_files_recursively_under(&self.codex_sessions_dir()),
            AgentClass::Antigravity => {
                antigravity_conversation_databases(&self.antigravity_conversations_dir())
            }
            AgentClass::Other(_) => Vec::new(),
        }
    }

    fn candidate_paths_for_session(&self, class: &AgentClass, session_id: &str) -> Vec<PathBuf> {
        let Some(budget) = &self.read_budget else {
            return self
                .candidate_paths(class)
                .into_iter()
                .filter(|path| {
                    session_id_from_transcript_path(class, path).as_deref() == Some(session_id)
                })
                .collect();
        };
        match class {
            AgentClass::Claude => vec![self
                .claude_project_dir()
                .join(format!("{session_id}.jsonl"))],
            AgentClass::Codex => bounded_candidate_paths(
                &self.codex_sessions_dir(),
                true,
                "jsonl",
                class,
                session_id,
                budget,
            ),
            AgentClass::Antigravity => vec![self
                .antigravity_conversations_dir()
                .join(format!("{session_id}.db"))],
            AgentClass::Other(_) => Vec::new(),
        }
    }

    /// Ensures a PID-held path is within the expected agent store before its
    /// metadata is opened. Claude must be a direct project transcript, not a
    /// nested subagent transcript; Codex may be nested only below sessions/.
    /// Compared after resolving both sides, never lexically.
    ///
    /// A path reaching this from a process's open descriptors is whatever the
    /// kernel reports, and macOS reports it fully resolved: a store rooted at
    /// `/var/folders/...` is handed back as `/private/var/folders/...`, so a
    /// textual comparison rejects an agent's own transcript.
    fn path_is_in_expected_store(&self, class: &AgentClass, path: &Path) -> bool {
        let path = canonical_or_original(path);
        match class {
            AgentClass::Claude => {
                path.parent() == Some(canonical_or_original(&self.claude_project_dir()).as_path())
            }
            AgentClass::Codex => {
                path.starts_with(canonical_or_original(&self.codex_sessions_dir()))
            }
            AgentClass::Antigravity => {
                path.parent()
                    == Some(canonical_or_original(&self.antigravity_conversations_dir()).as_path())
            }
            AgentClass::Other(_) => false,
        }
    }

    fn claude_project_dir(&self) -> PathBuf {
        self.home
            .join(".claude")
            .join("projects")
            .join(slugify_claude_project_path(&self.project_cwd))
    }

    fn codex_sessions_dir(&self) -> PathBuf {
        self.home.join(".codex").join("sessions")
    }

    fn antigravity_conversations_dir(&self) -> PathBuf {
        self.home
            .join(".gemini")
            .join("antigravity-cli")
            .join("conversations")
    }

    /// Antigravity keeps active conversations in UUID-named SQLite files and
    /// appends the authoritative project binding to `history.jsonl`. The
    /// database filename proves the conversation id held open by this exact
    /// process; the history record proves that id belongs to this project.
    ///
    /// `history.jsonl` is append-only, so a given `conversationId` can appear
    /// on more than one line. Only its first recorded workspace binding is
    /// authoritative -- mirroring `transcript_metadata_matches`, where later
    /// content can never redefine a rollout's owner -- so a later, unrelated
    /// line pairing this id with a different workspace must not be able to
    /// grant a second project ownership of the same conversation.
    fn antigravity_history_matches(&self, expected_session_id: &str) -> bool {
        let history_path = self
            .home
            .join(".gemini")
            .join("antigravity-cli")
            .join("history.jsonl");
        let Ok(file) = ilium_platform::secure_fs::open_regular_file(&history_path) else {
            return false;
        };
        if let Some(budget) = &self.read_budget {
            let mut reader = BufReader::new(file);
            while let Ok(Some(line)) = bounded_line(&mut reader, budget) {
                let Ok(identity) = parse_bounded_identity(
                    &AgentClass::Antigravity,
                    line,
                    budget,
                    self.metadata_parser.as_deref(),
                ) else {
                    return false;
                };
                let Some(identity) = identity else {
                    continue;
                };
                if identity.session_id != expected_session_id {
                    continue;
                }
                return identity.project_cwd.as_deref().is_some_and(|workspace| {
                    same_canonical_project(Path::new(workspace), &self.project_cwd)
                });
            }
            return false;
        }
        BufReader::new(file)
            .lines()
            .map_while(Result::ok)
            .find_map(|line| {
                let identity =
                    parse_transcript_identity(&AgentClass::Antigravity, line.as_bytes())?;
                if identity.session_id != expected_session_id {
                    return None;
                }
                // Preserve the unbounded legacy reader's missing-workspace
                // behavior; bounded worker discovery rejects the first binding.
                let workspace = identity.project_cwd?;
                Some(same_canonical_project(
                    Path::new(&workspace),
                    &self.project_cwd,
                ))
            })
            .unwrap_or(false)
    }
}

/// Validates the first authoritative identity record in a transcript. Later
/// content cannot redefine a rollout's owner; rejecting malformed or missing
/// metadata leaves discovery unresolved rather than inferring from a filename.
fn transcript_metadata_matches(
    class: &AgentClass,
    path: &Path,
    expected_session_id: &str,
    expected_project_cwd: &Path,
) -> bool {
    let Ok(file) = ilium_platform::secure_fs::open_regular_file(path) else {
        return false;
    };
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        let Some(identity) = parse_transcript_identity(class, line.as_bytes()) else {
            continue;
        };
        return identity.session_id == expected_session_id
            && identity
                .project_cwd
                .as_deref()
                .is_some_and(|cwd| same_canonical_project(Path::new(cwd), expected_project_cwd));
    }
    false
}

/// Claude repeats these fields on user/assistant records; queue bookkeeping
/// records without a cwd are intentionally not authoritative.
fn claude_identity(entry: &Value) -> Option<(&str, &str)> {
    let session_id = entry.get("sessionId")?.as_str()?;
    let cwd = entry.get("cwd")?.as_str()?;
    Some((session_id, cwd))
}

/// Codex writes one `session_meta` record whose payload owns the rollout ID
/// and cwd. Other payload shapes are not identity evidence.
fn codex_identity(entry: &Value) -> Option<(&str, &str)> {
    if entry.get("type")?.as_str()? != "session_meta" {
        return None;
    }
    let payload = entry.get("payload")?;
    Some((payload.get("id")?.as_str()?, payload.get("cwd")?.as_str()?))
}

fn transcript_files_directly_under(directory: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("jsonl"))
        .collect()
}

fn transcript_files_recursively_under(directory: &Path) -> Vec<PathBuf> {
    let mut pending_directories = vec![directory.to_path_buf()];
    let mut paths = Vec::new();
    while let Some(current_directory) = pending_directories.pop() {
        let Ok(entries) = std::fs::read_dir(current_directory) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            if file_type.is_dir() {
                pending_directories.push(path);
            } else if file_type.is_file()
                && path.extension().and_then(|value| value.to_str()) == Some("jsonl")
            {
                paths.push(path);
            }
        }
    }
    paths
}

/// Returns only primary Antigravity conversation databases. Sidecar
/// `-wal`/`-shm` files can be open too, but are neither independent sessions
/// nor stable enough to use as the provenance boundary.
fn antigravity_conversation_databases(directory: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("db"))
        .filter(|path| {
            path.file_stem()
                .and_then(|value| value.to_str())
                .is_some_and(looks_like_uuid)
        })
        .collect()
}

fn session_id_from_transcript_path(class: &AgentClass, path: &Path) -> Option<String> {
    let expected_extension = match class {
        AgentClass::Antigravity => "db",
        AgentClass::Claude | AgentClass::Codex | AgentClass::Other(_) => "jsonl",
    };
    if path.extension().and_then(|value| value.to_str()) != Some(expected_extension) {
        return None;
    }
    let stem = path.file_stem()?.to_str()?;
    match class {
        AgentClass::Claude => looks_like_uuid(stem).then(|| stem.to_string()),
        AgentClass::Codex => {
            let after_prefix = stem.strip_prefix("rollout-")?;
            let uuid_start = after_prefix.len().checked_sub(36)?;
            let candidate = after_prefix.get(uuid_start..)?;
            (uuid_start > 0
                && after_prefix.as_bytes().get(uuid_start - 1) == Some(&b'-')
                && looks_like_uuid(candidate))
            .then(|| candidate.to_string())
        }
        // The extension was already confirmed to be "db" by the
        // `expected_extension` check above; only the UUID shape remains.
        AgentClass::Antigravity => looks_like_uuid(stem).then(|| stem.to_string()),
        AgentClass::Other(_) => None,
    }
}

fn same_canonical_project(recorded_cwd: &Path, expected_cwd: &Path) -> bool {
    canonical_or_original(recorded_cwd) == canonical_or_original(expected_cwd)
}

fn canonical_or_original(path: &Path) -> PathBuf {
    // Through `ilium_platform`, not `std`: the provider derives its transcript
    // directory name from the project path, and Windows' own canonical form
    // carries a `\\?\` prefix that the provider never sees, so slugifying it
    // would look for a directory that does not exist.
    ilium_platform::paths::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Where Claude Code stores the transcript of `session_id` for a session
/// launched in `launch_cwd`. A path, not a proof: the file may not exist yet
/// (Claude writes it with the first message).
pub fn claude_transcript_path(home: &Path, launch_cwd: &Path, session_id: &str) -> PathBuf {
    home.join(".claude")
        .join("projects")
        .join(slugify_claude_project_path(&canonical_or_original(
            launch_cwd,
        )))
        .join(format!("{session_id}.jsonl"))
}

fn slugify_claude_project_path(cwd: &Path) -> String {
    cwd.to_string_lossy()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .collect()
}

fn looks_like_uuid(value: &str) -> bool {
    const HYPHEN_INDICES: [usize; 4] = [8, 13, 18, 23];
    value.len() == 36
        && value.chars().enumerate().all(|(index, character)| {
            if HYPHEN_INDICES.contains(&index) {
                character == '-'
            } else {
                character.is_ascii_hexdigit()
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn transcript_reads_reject_fifo_without_waiting_for_a_writer() {
        use std::{sync::mpsc, time::Duration};
        for reader in 0..3 {
            let directory = tempfile::tempdir().expect("private FIFO fixture");
            let path = directory.path().join("transcript.jsonl");
            assert!(std::process::Command::new("/usr/bin/mkfifo")
                .arg(&path)
                .status()
                .expect("mkfifo installed")
                .success());
            let source = path.clone();
            let (sender, receiver) = mpsc::channel();
            let child = std::thread::spawn(move || {
                let matched = match reader {
                    0 => transcript_metadata_matches_bounded(
                        &AgentClass::Claude,
                        &source,
                        "session",
                        Path::new("/project"),
                        &TranscriptReadBudget::new(TranscriptReadLimits {
                            line_bytes: 4096,
                            total_read_bytes: 8192,
                            scanned_entries: 64,
                            retained_path_bytes: 65536,
                        }),
                        None,
                    ),
                    1 => transcript_metadata_matches(
                        &AgentClass::Claude,
                        &source,
                        "session",
                        Path::new("/project"),
                    ),
                    _ => {
                        let home = source.parent().expect("fixture parent");
                        let history = home.join(".gemini/antigravity-cli");
                        std::fs::create_dir_all(&history).expect("history fixture");
                        std::fs::rename(&source, history.join("history.jsonl"))
                            .expect("history FIFO");
                        TranscriptLocator::new_bounded(
                            home,
                            Path::new("/project"),
                            TranscriptReadLimits {
                                line_bytes: 4096,
                                total_read_bytes: 8192,
                                scanned_entries: 64,
                                retained_path_bytes: 65536,
                            },
                        )
                        .antigravity_history_matches("session")
                    }
                };
                sender.send(matched).expect("result receiver remains owned");
            });
            let before_release = receiver.recv_timeout(Duration::from_secs(1));
            if matches!(before_release, Err(mpsc::RecvTimeoutError::Timeout)) {
                // Release the original blocking reader and physically join it
                // before failing the assertion. No orphan test thread remains.
                let fifo = if reader == 2 {
                    directory
                        .path()
                        .join(".gemini/antigravity-cli/history.jsonl")
                } else {
                    path
                };
                drop(
                    std::fs::OpenOptions::new()
                        .write(true)
                        .open(fifo)
                        .expect("release original reader"),
                );
            }
            child.join().expect("reader physically joined");
            assert!(
                before_release.is_ok(),
                "transcript reader {reader} blocked waiting for a FIFO writer"
            );
            assert!(!before_release.expect("timely result"));
        }
    }

    #[test]
    fn raw_metadata_parser_preserves_unresolved_project_spelling() {
        let line = br#"{"sessionId":"opaque-id","cwd":"/missing/../project"}"#;
        assert_eq!(
            parse_transcript_identity(&AgentClass::Claude, line),
            Some(TranscriptIdentity {
                session_id: "opaque-id".into(),
                project_cwd: Some("/missing/../project".into()),
            })
        );
        assert!(parse_transcript_identity(&AgentClass::Claude, b"\xff").is_none());
        assert!(
            parse_transcript_identity(&AgentClass::Claude, br#"{"sessionId":"opaque-id"}"#)
                .is_none()
        );
    }

    #[test]
    fn raw_codex_parser_requires_authoritative_record_shape() {
        let other = br#"{"type":"event_msg","payload":{"id":"id","cwd":"/project"}}"#;
        assert!(parse_transcript_identity(&AgentClass::Codex, other).is_none());
        let metadata = br#"{"type":"session_meta","payload":{"id":"id","cwd":"/project"}}"#;
        assert_eq!(
            parse_transcript_identity(&AgentClass::Codex, metadata),
            Some(TranscriptIdentity {
                session_id: "id".into(),
                project_cwd: Some("/project".into()),
            })
        );
    }

    #[test]
    fn raw_history_parser_retains_missing_workspace_refusal() {
        let line = br#"{"conversationId":"id","workspace":null}"#;
        assert_eq!(
            parse_transcript_identity(&AgentClass::Antigravity, line),
            Some(TranscriptIdentity {
                session_id: "id".into(),
                project_cwd: None,
            })
        );
    }

    #[derive(Debug)]
    struct RecordingParser {
        calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
        failure: Option<MetadataParseFailure>,
    }
    impl TranscriptMetadataParser for RecordingParser {
        fn parse(
            &self,
            class: &AgentClass,
            line: Vec<u8>,
        ) -> Result<Option<TranscriptIdentity>, MetadataParseFailure> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
            match self.failure {
                Some(reason) => Err(reason),
                None => Ok(parse_transcript_identity(class, &line)),
            }
        }
    }
    fn parser_limits() -> TranscriptReadLimits {
        TranscriptReadLimits {
            line_bytes: 16 * 1024,
            total_read_bytes: 64 * 1024,
            scanned_entries: 64,
            retained_path_bytes: 64 * 1024,
        }
    }

    fn drive_staged_cursor(
        locator: &TranscriptLocator,
        class: &AgentClass,
        path: &Path,
    ) -> Option<VerifiedTranscript> {
        let mut cursor = locator.begin_staged_metadata(class, path)?;
        let mut parsed = None;
        loop {
            match cursor.advance(parsed) {
                StagedMetadataStep::Batch {
                    cursor: next,
                    lines,
                } => {
                    parsed = Some(next.parse_batch(&lines));
                    cursor = next;
                }
                StagedMetadataStep::Finished(transcript) => return transcript,
            }
        }
    }

    #[test]
    fn staged_cursor_stops_at_first_authoritative_record_before_oversized_tail() {
        let home = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let session_id = "123e4567-e89b-12d3-a456-426614174000";
        let path = write_claude_transcript(home.path(), project.path(), project.path(), session_id);
        let first = std::fs::read(&path).unwrap();
        std::fs::write(
            &path,
            [first, b"\n".to_vec(), vec![b'x'; 32 * 1024]].concat(),
        )
        .unwrap();
        let locator = TranscriptLocator::new_bounded(home.path(), project.path(), parser_limits());

        let verified = drive_staged_cursor(&locator, &AgentClass::Claude, &path);

        assert_eq!(
            verified.map(|transcript| transcript.session_id),
            Some(session_id.into())
        );
        assert!(!locator.read_limit_reached());
    }

    #[test]
    fn staged_cursor_skips_malformed_line_then_preserves_first_binding() {
        let home = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let session_id = "123e4567-e89b-12d3-a456-426614174000";
        let path = write_claude_transcript(home.path(), project.path(), project.path(), session_id);
        let first = std::fs::read(&path).unwrap();
        std::fs::write(&path, [b"{\n".to_vec(), first, b"\n".to_vec()].concat()).unwrap();
        let locator = TranscriptLocator::new_bounded(home.path(), project.path(), parser_limits());
        assert_eq!(
            drive_staged_cursor(&locator, &AgentClass::Claude, &path)
                .map(|transcript| transcript.session_id),
            Some(session_id.into()),
        );
        assert!(!locator.read_limit_reached());
    }

    #[test]
    fn staged_antigravity_refuses_missing_first_workspace_binding() {
        let home = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let session_id = "123e4567-e89b-12d3-a456-426614174000";
        let path = write_antigravity_conversation(home.path(), project.path(), session_id);
        let history = home.path().join(".gemini/antigravity-cli/history.jsonl");
        std::fs::write(
            &history,
            format!(
                "{}\n{}\n",
                serde_json::json!({"conversationId": session_id}),
                serde_json::json!({"conversationId": session_id, "workspace": project.path()}),
            ),
        )
        .unwrap();
        let locator = TranscriptLocator::new_bounded(home.path(), project.path(), parser_limits());
        assert_eq!(
            drive_staged_cursor(&locator, &AgentClass::Antigravity, &path),
            None
        );
        assert!(!locator.read_limit_reached());
    }

    #[test]
    fn staged_cursor_refuses_identity_after_parser_failure() {
        let home = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let session_id = "123e4567-e89b-12d3-a456-426614174000";
        let path = write_claude_transcript(home.path(), project.path(), project.path(), session_id);
        let locator = TranscriptLocator::new_bounded(home.path(), project.path(), parser_limits());
        let cursor = locator
            .begin_staged_metadata(&AgentClass::Claude, &path)
            .unwrap();
        let StagedMetadataStep::Batch { cursor, lines } = cursor.advance(None) else {
            panic!("first batch must be read")
        };
        let parsed = cursor.parse_batch(&lines);
        locator.mark_metadata_parse_failure(MetadataParseFailure::WorkerFailed);
        assert!(matches!(
            cursor.advance(Some(parsed)),
            StagedMetadataStep::Finished(None)
        ));
    }

    #[test]
    fn bounded_real_transcript_uses_caller_owned_metadata_parser() {
        let home = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let session_id = "123e4567-e89b-12d3-a456-426614174000";
        let path = write_claude_transcript(home.path(), project.path(), project.path(), session_id);
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let parser = std::sync::Arc::new(RecordingParser {
            calls: calls.clone(),
            failure: None,
        });
        let locator = TranscriptLocator::new_bounded(home.path(), project.path(), parser_limits())
            .with_metadata_parser(parser)
            .unwrap();
        let verified = locator
            .transcript_for_session(&AgentClass::Claude, session_id)
            .unwrap();
        assert_eq!(verified.path, path);
        assert_eq!(calls.load(std::sync::atomic::Ordering::Acquire), 1);
        assert_eq!(locator.metadata_parse_failure(), None);
        assert!(!locator.read_limit_reached());
    }

    #[test]
    fn staged_metadata_cursor_returns_raw_batch_before_identity_verification() {
        let home = tempfile::tempdir().expect("home");
        let project = tempfile::tempdir().expect("project");
        let session_id = "123e4567-e89b-12d3-a456-426614174000";
        let path = write_claude_transcript(home.path(), project.path(), project.path(), session_id);
        let locator = TranscriptLocator::new_bounded(home.path(), project.path(), parser_limits());

        let candidates =
            locator.staged_candidate_paths_for_session(&AgentClass::Claude, session_id);
        assert_eq!(candidates, vec![path.clone()]);
        let cursor = locator
            .begin_staged_metadata(&AgentClass::Claude, &path)
            .expect("bounded transcript cursor");
        let (cursor, lines) = match cursor.advance(None) {
            StagedMetadataStep::Batch { cursor, lines } => (cursor, lines),
            StagedMetadataStep::Finished(result) => {
                panic!("cursor finished before yielding metadata: {result:?}")
            }
        };
        let parsed = cursor.parse_batch(&lines);
        assert_eq!(parsed.identity.as_ref().unwrap().session_id, session_id);
        assert_eq!(parsed.consumed_lines, 1);
        match cursor.advance(Some(parsed)) {
            StagedMetadataStep::Finished(Some(verified)) => {
                assert_eq!(verified.session_id, session_id);
                assert_eq!(verified.path, path);
            }
            StagedMetadataStep::Finished(None) => panic!("matching identity was rejected"),
            StagedMetadataStep::Batch { .. } => panic!("verified first record must finish cursor"),
        }
        assert!(!locator.read_limit_reached());
    }

    #[test]
    fn staged_batches_bound_tiny_line_handoffs_and_keep_first_record_order() {
        let home = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let session_id = "123e4567-e89b-12d3-a456-426614174000";
        let path = write_claude_transcript(home.path(), project.path(), project.path(), session_id);
        let authoritative = std::fs::read(&path).unwrap();
        let mut content = b"{}\n".repeat(8_192);
        content.extend(authoritative);
        std::fs::write(&path, content).unwrap();
        let locator = TranscriptLocator::new_bounded(
            home.path(),
            project.path(),
            TranscriptReadLimits {
                line_bytes: 1024 * 1024,
                total_read_bytes: 16 * 1024 * 1024,
                scanned_entries: 4096,
                retained_path_bytes: 4 * 1024 * 1024,
            },
        );
        let mut cursor = locator
            .begin_staged_metadata(&AgentClass::Claude, &path)
            .unwrap();
        let mut parsed = None;
        let mut batches = 0usize;
        let verified = loop {
            match cursor.advance(parsed) {
                StagedMetadataStep::Batch {
                    cursor: next,
                    lines,
                } => {
                    assert!(lines.len() <= STAGED_BATCH_LINES);
                    batches += 1;
                    parsed = Some(next.parse_batch(&lines));
                    cursor = next;
                }
                StagedMetadataStep::Finished(verified) => break verified,
            }
        };
        assert_eq!(verified.unwrap().session_id, session_id);
        assert!(
            batches <= 33,
            "8,192 short lines need at most 33 batches: {batches}"
        );
        assert!(!locator.read_limit_reached());
    }

    #[test]
    fn staged_read_ahead_stays_within_the_shared_attempt_byte_cap() {
        let home = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let session_id = "123e4567-e89b-12d3-a456-426614174000";
        let path = write_claude_transcript(home.path(), project.path(), project.path(), session_id);
        let authoritative = std::fs::read(&path).unwrap();
        assert!(authoritative.len() < 512);
        let mut content = authoritative;
        content.extend(b"{}\n".repeat(512));
        std::fs::write(&path, content).unwrap();
        let locator = TranscriptLocator::new_bounded(
            home.path(),
            project.path(),
            TranscriptReadLimits {
                line_bytes: 1024 * 1024,
                total_read_bytes: 512,
                scanned_entries: 4096,
                retained_path_bytes: 4 * 1024 * 1024,
            },
        );
        assert!(drive_staged_cursor(&locator, &AgentClass::Claude, &path).is_some());
        assert!(
            !locator.read_limit_reached(),
            "valid first record survives deferred tail overflow"
        );
        assert!(drive_staged_cursor(&locator, &AgentClass::Claude, &path).is_none());
        assert!(
            locator.read_limit_reached(),
            "read-ahead shares the attempt byte cap"
        );
    }

    #[test]
    fn staged_line_and_job_caps_fail_closed() {
        let home = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let session_id = "123e4567-e89b-12d3-a456-426614174000";
        let path = write_claude_transcript(home.path(), project.path(), project.path(), session_id);
        let authoritative = std::fs::read(&path).unwrap();
        let mut content = b"{}\n".repeat(STAGED_METADATA_LINE_LIMIT);
        content.extend(authoritative);
        std::fs::write(&path, content).unwrap();
        let locator = TranscriptLocator::new_bounded(
            home.path(),
            project.path(),
            TranscriptReadLimits {
                line_bytes: 1024 * 1024,
                total_read_bytes: 16 * 1024 * 1024,
                scanned_entries: 4096,
                retained_path_bytes: 4 * 1024 * 1024,
            },
        );
        assert!(drive_staged_cursor(&locator, &AgentClass::Claude, &path).is_none());
        assert!(locator.read_limit_reached());
        assert_eq!(
            locator.read_limit_failure().unwrap().resource,
            "staged metadata lines"
        );
        let jobs = TranscriptLocator::new_bounded(home.path(), project.path(), parser_limits());
        assert!(jobs.claim_staged_jobs(STAGED_METADATA_JOB_LIMIT));
        assert!(!jobs.claim_staged_jobs(1));
        assert!(jobs.read_limit_reached());
        assert_eq!(
            jobs.read_limit_failure().unwrap().resource,
            "staged metadata jobs"
        );
    }

    #[test]
    fn bounded_request_evidence_stops_at_the_shared_transcript_byte_limit() {
        let home = tempfile::tempdir().expect("home");
        let project = tempfile::tempdir().expect("project");
        let session_id = "123e4567-e89b-12d3-a456-426614174000";
        let path = write_codex_transcript(home.path(), project.path(), session_id);
        let metadata = serde_json::json!({
            "type": "session_meta",
            "payload": {"id": session_id, "cwd": project.path()},
        });
        let monitor = serde_json::json!({
            "type": "event_msg",
            "payload": {"type": "user_message", "message": "Ilium progress monitor 42 reports completion"},
        });
        let authored = serde_json::json!({
            "type": "event_msg",
            "payload": {"type": "user_message", "message": "fix the pane title"},
        });
        let mut transcript = format!("{}\n", metadata);
        for _ in 0..24 {
            transcript.push_str(&monitor.to_string());
            transcript.push('\n');
        }
        transcript.push_str(&authored.to_string());
        transcript.push('\n');
        std::fs::write(path, transcript).expect("oversized transcript");

        let locator = TranscriptLocator::new_bounded(
            home.path(),
            project.path(),
            TranscriptReadLimits {
                line_bytes: 4096,
                total_read_bytes: 1024,
                scanned_entries: 64,
                retained_path_bytes: 64 * 1024,
            },
        );

        assert_eq!(
            locator
                .genuine_request_evidence(&AgentClass::Codex, session_id)
                .expect("bounded evidence is a structured result"),
            GenuineRequestEvidence::Unavailable,
            "a request beyond the transcript budget must not authorize a title"
        );
        assert!(locator.read_limit_reached());
        assert_eq!(
            locator.read_limit_failure().map(|failure| failure.resource),
            Some("transcript bytes")
        );
    }

    #[test]
    fn bounded_request_evidence_refuses_an_oversized_jsonl_record() {
        let home = tempfile::tempdir().expect("home");
        let project = tempfile::tempdir().expect("project");
        let session_id = "123e4567-e89b-12d3-a456-426614174000";
        let path = write_codex_transcript(home.path(), project.path(), session_id);
        let metadata = serde_json::json!({
            "type": "session_meta",
            "payload": {"id": session_id, "cwd": project.path()},
        });
        let transcript = format!("{}\n{}\n", metadata, "x".repeat(1024));
        std::fs::write(path, transcript).expect("oversized record");

        let locator = TranscriptLocator::new_bounded(
            home.path(),
            project.path(),
            TranscriptReadLimits {
                line_bytes: 512,
                total_read_bytes: 4096,
                scanned_entries: 64,
                retained_path_bytes: 64 * 1024,
            },
        );

        assert_eq!(
            locator
                .genuine_request_evidence(&AgentClass::Codex, session_id)
                .expect("line-limit refusal is structured evidence"),
            GenuineRequestEvidence::Unavailable
        );
        assert!(locator.read_limit_reached());
        assert_eq!(
            locator.read_limit_failure().map(|failure| failure.resource),
            Some("transcript line bytes")
        );
    }

    #[test]
    fn metadata_worker_failure_invalidates_partial_identity_attempt() {
        let home = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let session_id = "123e4567-e89b-12d3-a456-426614174000";
        write_claude_transcript(home.path(), project.path(), project.path(), session_id);
        for reason in [
            MetadataParseFailure::Cancelled,
            MetadataParseFailure::AdmissionRefused,
            MetadataParseFailure::WorkerFailed,
        ] {
            let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let parser = std::sync::Arc::new(RecordingParser {
                calls: calls.clone(),
                failure: Some(reason),
            });
            let locator =
                TranscriptLocator::new_bounded(home.path(), project.path(), parser_limits())
                    .with_metadata_parser(parser)
                    .unwrap();
            assert!(locator
                .transcript_for_session(&AgentClass::Claude, session_id)
                .is_none());
            assert!(locator.read_limit_reached());
            assert_eq!(locator.metadata_parse_failure(), Some(reason));
            assert_eq!(calls.load(std::sync::atomic::Ordering::Acquire), 1);
        }
    }

    #[test]
    fn unbounded_locator_cannot_install_worker_parser() {
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let parser = std::sync::Arc::new(RecordingParser {
            calls: calls.clone(),
            failure: None,
        });
        let locator = TranscriptLocator::new(Path::new("/home"), Path::new("/project"));
        assert!(matches!(
            locator.with_metadata_parser(parser),
            Err(MetadataParseFailure::AdmissionRefused)
        ));
        assert_eq!(calls.load(std::sync::atomic::Ordering::Acquire), 0);
    }

    #[test]
    fn bounded_discovery_rejects_long_lines_and_partial_directory_scans() {
        let home = tempfile::tempdir().expect("home");
        let project = tempfile::tempdir().expect("project");
        let session_id = "11111111-1111-4111-8111-111111111111";
        let path = write_claude_transcript(home.path(), project.path(), project.path(), session_id);
        let limits = TranscriptReadLimits {
            line_bytes: 4096,
            total_read_bytes: 8192,
            scanned_entries: 64,
            retained_path_bytes: 65536,
        };
        let locator = TranscriptLocator::new_bounded(home.path(), project.path(), limits);
        assert!(locator
            .transcript_for_session(&AgentClass::Claude, session_id)
            .is_some());
        let oversized = TranscriptLocator::new_bounded(
            home.path(),
            project.path(),
            TranscriptReadLimits {
                line_bytes: 16,
                ..limits
            },
        );
        assert!(oversized
            .transcript_from_path(&AgentClass::Claude, &path)
            .is_none());
        assert!(oversized.read_limit_reached());
        std::fs::write(path.with_file_name("unrelated.jsonl"), "{}").expect("scan fixture");
        let partial = TranscriptLocator::new_bounded(
            home.path(),
            project.path(),
            TranscriptReadLimits {
                scanned_entries: 1,
                ..limits
            },
        );
        assert_eq!(
            partial
                .transcript_for_session(&AgentClass::Claude, session_id)
                .expect("unrelated files do not block exact Claude lookup")
                .path,
            path
        );
        assert!(!partial.read_limit_reached());
    }

    #[test]
    fn bounded_scan_reports_the_measured_entry_limit_overrun() {
        let home = tempfile::tempdir().expect("home");
        let project = tempfile::tempdir().expect("project");
        let session_id = "123e4567-e89b-12d3-a456-426614174000";
        write_codex_transcript(home.path(), project.path(), session_id);
        let locator = TranscriptLocator::new_bounded(
            home.path(),
            project.path(),
            TranscriptReadLimits {
                line_bytes: 4096,
                total_read_bytes: 8192,
                scanned_entries: 0,
                retained_path_bytes: 4096,
            },
        );

        assert!(locator
            .transcript_for_session(&AgentClass::Codex, session_id)
            .is_none());
        let failure = locator
            .read_limit_failure()
            .expect("limit exhaustion retains structured measurement");
        assert_eq!(failure.resource, "filesystem entries");
        assert_eq!((failure.used, failure.requested, failure.limit), (0, 1, 0));
        assert!(failure.to_string().contains("observed at least 1"));
        assert!(failure.to_string().contains("at least 1 over"));
    }

    #[test]
    fn codex_lookup_finds_requested_transcript_without_retaining_the_whole_store() {
        let home = tempfile::tempdir().expect("home");
        let project = tempfile::tempdir().expect("project");
        let session_id = "123e4567-e89b-12d3-a456-426614174000";
        let target = write_codex_transcript(home.path(), project.path(), session_id);
        let directory = target.parent().expect("Codex date directory");
        for index in 0..5_000 {
            std::fs::write(directory.join(format!("unrelated-{index}.jsonl")), "{}").unwrap();
        }

        let locator = TranscriptLocator::new_bounded(
            home.path(),
            project.path(),
            TranscriptReadLimits {
                line_bytes: 4096,
                total_read_bytes: 64 * 1024,
                scanned_entries: 10_000,
                retained_path_bytes: 64 * 1024,
            },
        );
        let found = locator
            .transcript_for_session(&AgentClass::Codex, session_id)
            .expect("requested session remains discoverable in a large store");

        assert_eq!(found.path, target);
        assert!(!locator.read_limit_reached());
    }

    /// Builds a store shaped like the live `~/.codex/sessions`: nested
    /// `YYYY/MM/DD` folders, legacy flat `.json` rollouts at the top level,
    /// and well over 4096 filesystem entries in total.
    fn write_realistic_codex_store(home: &Path, metadata_cwd: &Path, session_id: &str) -> PathBuf {
        let sessions = home.join(".codex").join("sessions");
        std::fs::create_dir_all(&sessions).unwrap();
        for index in 0..20 {
            std::fs::write(
                sessions.join(format!("rollout-2025-04-16-legacy-{index}.json")),
                "{}",
            )
            .unwrap();
        }
        for year in ["2025", "2026"] {
            for month in 1..=12 {
                for day in 1..=21 {
                    let directory = sessions
                        .join(year)
                        .join(format!("{month:02}"))
                        .join(format!("{day:02}"));
                    std::fs::create_dir_all(&directory).unwrap();
                    for index in 0..17 {
                        std::fs::write(
                            directory
                                .join(format!("rollout-{year}-{month:02}-{day:02}-{index}.jsonl")),
                            "{}",
                        )
                        .unwrap();
                    }
                }
            }
        }
        let directory = sessions.join("2026").join("12").join("21");
        let path = directory.join(format!("rollout-2026-12-21T09-00-00-{session_id}.jsonl"));
        let entry = serde_json::json!({
            "type": "session_meta",
            "payload": {"id": session_id, "cwd": metadata_cwd}
        });
        std::fs::write(&path, entry.to_string()).unwrap();
        path
    }

    /// Regression: the terminal context menu resolves Codex history with the
    /// same bounded locator as this test. A realistic store must still resolve,
    /// or the menu silently drops "Copy history file path".
    #[test]
    fn codex_lookup_resolves_realistic_store_under_menu_limits() {
        let home = tempfile::tempdir().expect("home");
        let project = tempfile::tempdir().expect("project");
        let session_id = "55555555-5555-4555-8555-555555555555";
        let target = write_realistic_codex_store(home.path(), project.path(), session_id);
        let locator = TranscriptLocator::new_bounded(
            home.path(),
            project.path(),
            INTERACTIVE_TRANSCRIPT_LOOKUP_LIMITS,
        );

        let found = locator.transcript_for_session(&AgentClass::Codex, session_id);

        assert_eq!(found.map(|transcript| transcript.path), Some(target));
        assert!(!locator.read_limit_reached());
    }

    /// Codex candidates come back newest `YYYY/MM/DD` first, so a duplicate
    /// identity is listed with its most recent transcript leading.
    #[test]
    fn codex_candidates_are_listed_newest_directory_first() {
        let home = tempfile::tempdir().expect("home");
        let project = tempfile::tempdir().expect("project");
        let session_id = "56565656-5656-4656-8656-565656565656";
        let mut expected = Vec::new();
        for date in ["2025/01/03", "2026/09/05", "2026/02/01"] {
            let directory = home.path().join(".codex").join("sessions").join(date);
            std::fs::create_dir_all(&directory).unwrap();
            let stamp = date.replace('/', "-");
            let path = directory.join(format!("rollout-{stamp}T12-00-00-{session_id}.jsonl"));
            std::fs::write(
                &path,
                serde_json::json!({
                    "type": "session_meta",
                    "payload": {"id": session_id, "cwd": project.path()}
                })
                .to_string(),
            )
            .unwrap();
            expected.push(path);
        }
        expected.sort_by(|left, right| right.cmp(left));
        let locator = TranscriptLocator::new_bounded(
            home.path(),
            project.path(),
            INTERACTIVE_TRANSCRIPT_LOOKUP_LIMITS,
        );

        assert_eq!(
            locator.candidate_paths_for_session(&AgentClass::Codex, session_id),
            expected
        );
        assert_eq!(
            expected
                .first()
                .map(|path| path.to_string_lossy().contains("2026-09-05")),
            Some(true)
        );
        // Two valid copies of one identity remain ambiguous, never a pick.
        assert_eq!(
            locator.transcript_for_session(&AgentClass::Codex, session_id),
            None
        );
    }

    fn write_claude_transcript(
        home: &Path,
        directory_cwd: &Path,
        metadata_cwd: &Path,
        session_id: &str,
    ) -> PathBuf {
        let directory = home
            .join(".claude")
            .join("projects")
            .join(slugify_claude_project_path(directory_cwd));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join(format!("{session_id}.jsonl"));
        let entry = serde_json::json!({
            "type": "user",
            "sessionId": session_id,
            "cwd": metadata_cwd,
            "message": {"content": "test prompt"}
        });
        std::fs::write(&path, entry.to_string()).unwrap();
        path
    }

    fn write_codex_transcript(home: &Path, metadata_cwd: &Path, session_id: &str) -> PathBuf {
        let directory = home.join(".codex").join("sessions").join("2026/07/14");
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join(format!("rollout-2026-07-14T12-00-00-{session_id}.jsonl"));
        let entry = serde_json::json!({
            "type": "session_meta",
            "payload": {"id": session_id, "cwd": metadata_cwd}
        });
        std::fs::write(&path, entry.to_string()).unwrap();
        path
    }

    fn write_antigravity_conversation(
        home: &Path,
        metadata_cwd: &Path,
        session_id: &str,
    ) -> PathBuf {
        let root = home.join(".gemini").join("antigravity-cli");
        let conversations = root.join("conversations");
        std::fs::create_dir_all(&conversations).unwrap();
        let path = conversations.join(format!("{session_id}.db"));
        std::fs::write(&path, "sqlite fixture placeholder").unwrap();
        // Trailing newline mirrors the real append-only `history.jsonl`, so
        // tests can append further lines behind this one without corrupting
        // it into one unparseable line.
        std::fs::write(
            root.join("history.jsonl"),
            format!(
                "{}\n",
                serde_json::json!({
                    "display": "write a provider adapter",
                    "workspace": metadata_cwd,
                    "conversationId": session_id,
                })
            ),
        )
        .unwrap();
        path
    }

    #[test]
    fn claude_slug_collision_cannot_cross_project_metadata_boundary() {
        let home = tempfile::tempdir().unwrap();
        let expected_cwd = Path::new("/work/a-b");
        let colliding_cwd = Path::new("/work/a_b");
        let session_id = "11111111-1111-4111-8111-111111111111";
        assert_eq!(
            slugify_claude_project_path(expected_cwd),
            slugify_claude_project_path(colliding_cwd)
        );
        write_claude_transcript(home.path(), expected_cwd, colliding_cwd, session_id);

        let locator = TranscriptLocator::new(home.path(), expected_cwd);
        assert_eq!(
            locator.transcript_for_session(&AgentClass::Claude, session_id),
            None
        );
    }

    #[test]
    fn codex_global_store_is_clamped_by_session_meta_cwd() {
        let home = tempfile::tempdir().unwrap();
        let session_id = "22222222-2222-4222-8222-222222222222";
        write_codex_transcript(home.path(), Path::new("/work/ilium"), session_id);

        let wrong_project = TranscriptLocator::new(home.path(), Path::new("/work/money"));
        assert_eq!(
            wrong_project.transcript_for_session(&AgentClass::Codex, session_id),
            None
        );

        let right_project = TranscriptLocator::new(home.path(), Path::new("/work/ilium"));
        assert_eq!(
            right_project
                .transcript_for_session(&AgentClass::Codex, session_id)
                .map(|transcript| transcript.session_id),
            Some(session_id.to_string())
        );
    }

    #[test]
    fn antigravity_database_and_history_must_agree_on_project_ownership() {
        let home = tempfile::tempdir().unwrap();
        let session_id = "2a222222-2222-4222-8222-222222222222";
        let conversation =
            write_antigravity_conversation(home.path(), Path::new("/work/ilium"), session_id);

        let wrong_project = TranscriptLocator::new(home.path(), Path::new("/work/money"));
        assert_eq!(
            wrong_project.transcript_for_session(&AgentClass::Antigravity, session_id),
            None
        );

        let right_project = TranscriptLocator::new(home.path(), Path::new("/work/ilium"));
        assert_eq!(
            right_project
                .transcript_from_path(&AgentClass::Antigravity, &conversation)
                .map(|transcript| transcript.session_id),
            Some(session_id.to_string())
        );
    }

    #[test]
    fn antigravity_history_first_workspace_binding_wins_over_a_later_rebind() {
        let home = tempfile::tempdir().unwrap();
        let session_id = "2b222222-2222-4222-8222-222222222222";
        let conversation =
            write_antigravity_conversation(home.path(), Path::new("/work/ilium"), session_id);
        // Simulate an append-only history log that later pairs the same
        // conversation id with a different workspace (a stale entry or a
        // hostile append). The first-recorded binding must still win.
        let history_path = home
            .path()
            .join(".gemini")
            .join("antigravity-cli")
            .join("history.jsonl");
        let mut history = std::fs::OpenOptions::new()
            .append(true)
            .open(&history_path)
            .unwrap();
        use std::io::Write as _;
        writeln!(
            history,
            "{}",
            serde_json::json!({
                "display": "later rebind attempt",
                "workspace": "/work/money",
                "conversationId": session_id,
            })
        )
        .unwrap();

        let original_project = TranscriptLocator::new(home.path(), Path::new("/work/ilium"));
        assert_eq!(
            original_project
                .transcript_from_path(&AgentClass::Antigravity, &conversation)
                .map(|transcript| transcript.session_id),
            Some(session_id.to_string()),
            "the first recorded workspace binding must still be honored"
        );

        let later_rebind_project = TranscriptLocator::new(home.path(), Path::new("/work/money"));
        assert_eq!(
            later_rebind_project.transcript_from_path(&AgentClass::Antigravity, &conversation),
            None,
            "a later history line must not be able to rebind an already-owned conversation"
        );
    }

    #[test]
    fn pid_path_must_match_the_detected_agent_class() {
        let home = tempfile::tempdir().unwrap();
        let cwd = Path::new("/work/project");
        let session_id = "33333333-3333-4333-8333-333333333333";
        let path = write_codex_transcript(home.path(), cwd, session_id);
        let locator = TranscriptLocator::new(home.path(), cwd);

        assert_eq!(
            locator.transcript_from_path(&AgentClass::Claude, &path),
            None
        );
        assert_eq!(
            locator
                .transcript_from_path(&AgentClass::Codex, &path)
                .map(|transcript| transcript.session_id),
            Some(session_id.to_string())
        );
    }

    #[test]
    fn missing_identity_metadata_is_never_treated_as_verified() {
        let home = tempfile::tempdir().unwrap();
        let cwd = Path::new("/work/project");
        let session_id = "44444444-4444-4444-8444-444444444444";
        let directory = home
            .path()
            .join(".claude/projects")
            .join(slugify_claude_project_path(cwd));
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join(format!("{session_id}.jsonl")),
            r#"{"type":"queue-operation","sessionId":"44444444-4444-4444-8444-444444444444"}"#,
        )
        .unwrap();

        let locator = TranscriptLocator::new(home.path(), cwd);
        assert_eq!(
            locator.transcript_for_session(&AgentClass::Claude, session_id),
            None
        );
    }
}

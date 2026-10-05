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
#[derive(Debug)]
struct TranscriptReadBudget {
    limits: TranscriptReadLimits,
    read_bytes: std::sync::atomic::AtomicUsize,
    entries: std::sync::atomic::AtomicUsize,
    path_bytes: std::sync::atomic::AtomicUsize,
    exhausted: std::sync::atomic::AtomicBool,
    parser_failure: std::sync::atomic::AtomicU8,
}
impl TranscriptReadBudget {
    fn new(limits: TranscriptReadLimits) -> Self {
        Self {
            limits,
            read_bytes: 0.into(),
            entries: 0.into(),
            path_bytes: 0.into(),
            exhausted: false.into(),
            parser_failure: 0.into(),
        }
    }
    fn charge(&self, counter: &std::sync::atomic::AtomicUsize, bytes: usize, limit: usize) -> bool {
        use std::sync::atomic::Ordering;
        if self.exhausted.load(Ordering::Acquire) {
            return false;
        }
        if counter
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                used.checked_add(bytes).filter(|total| *total <= limit)
            })
            .is_ok()
        {
            return true;
        }
        self.exhausted.store(true, Ordering::Release);
        false
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
        if line.len().saturating_add(count) > budget.limits.line_bytes
            || !budget.charge(&budget.read_bytes, count, budget.limits.total_read_bytes)
        {
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
    let Ok(file) = std::fs::File::open(path) else {
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
    budget: &TranscriptReadBudget,
) -> Vec<PathBuf> {
    let mut pending = vec![directory.to_owned()];
    let mut paths = Vec::new();
    while let Some(directory) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(directory) else {
            continue;
        };
        for entry in entries {
            if !budget.charge(&budget.entries, 1, budget.limits.scanned_entries) {
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
            let path = entry.path();
            if !budget.charge(
                &budget.path_bytes,
                path.capacity()
                    .saturating_mul(2)
                    .saturating_add(std::mem::size_of::<PathBuf>() * 2),
                budget.limits.retained_path_bytes,
            ) {
                return Vec::new();
            }
            if kind.is_dir() {
                pending.push(path);
            } else if path.extension().and_then(|value| value.to_str()) == Some(extension) {
                paths.push(path);
            }
        }
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
    /// Adapter overflow invalidates every partial ownership conclusion in the
    /// same bounded attempt, including stronger ranks evaluated earlier.
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
            .candidate_paths(class)
            .into_iter()
            .filter(|path| {
                session_id_from_transcript_path(class, path).as_deref() == Some(session_id)
            })
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
        request_evidence::request_evidence_from_path(
            class,
            &transcript.path,
            &transcript.session_id,
        )
    }

    /// Enumerates format-shaped paths without making any ownership claim.
    fn candidate_paths(&self, class: &AgentClass) -> Vec<PathBuf> {
        if let Some(budget) = &self.read_budget {
            let (directory, recursive, extension) = match class {
                AgentClass::Claude => (self.claude_project_dir(), false, "jsonl"),
                AgentClass::Codex => (self.codex_sessions_dir(), true, "jsonl"),
                AgentClass::Antigravity => (self.antigravity_conversations_dir(), false, "db"),
                AgentClass::Other(_) => return Vec::new(),
            };
            return bounded_candidate_paths(&directory, recursive, extension, budget);
        }
        match class {
            AgentClass::Claude => transcript_files_directly_under(&self.claude_project_dir()),
            AgentClass::Codex => transcript_files_recursively_under(&self.codex_sessions_dir()),
            AgentClass::Antigravity => {
                antigravity_conversation_databases(&self.antigravity_conversations_dir())
            }
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
        let Ok(file) = std::fs::File::open(history_path) else {
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
    let Ok(file) = std::fs::File::open(path) else {
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
        assert!(partial
            .transcript_for_session(&AgentClass::Claude, session_id)
            .is_none());
        assert!(partial.read_limit_reached());
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

//! Applies one numeric setting to another tool's user configuration file, and
//! reverts it again.
//!
//! The two targets are the auto-compaction thresholds the compaction optimizer
//! recommends: Claude Code's `autoCompactWindow` (top-level key of
//! `~/.claude/settings.json`) and Codex's `model_auto_compact_token_limit`
//! (top-level key of `~/.codex/config.toml`). Both files belong to the user and
//! are also rewritten by the agent CLIs themselves, so this module never
//! re-serializes them. It patches the bytes of exactly one key and leaves every
//! other byte (comments, ordering, indentation, line endings, the presence or
//! absence of a trailing newline) alone:
//!
//! - JSON: a small span scanner locates the top-level member; the value is
//!   replaced in place, or the member is inserted as the first key reusing the
//!   file's own indentation. `serde_json` only validates and cross-checks; the
//!   crate-wide `preserve_order` feature stays off.
//! - TOML: `toml_edit` parses the document and reports byte spans, so the
//!   integer is replaced in place, or one line is inserted after the last
//!   top-level key-value (that is, before the first table). A same-named key
//!   inside a `[table]` is never touched.
//!
//! Every edit is verified before it is published: the edited text must parse
//! again, must hold exactly the requested value for the key, and (TOML via
//! `toml`, JSON via `serde_json`) must differ from the original document in
//! that one key only.
//!
//! Publishing is a compare-and-swap. The user sees a plan (old value, new
//! value, a content fingerprint of the file); [`apply`] takes an exclusive lock
//! on a sidecar lock file (never on the target), re-reads the target inside the
//! lock, refuses with [`WriteError::Stale`] when the content no longer matches
//! the plan, writes a temporary file next to the real file (symlinks are
//! resolved first), fsyncs it, re-checks the target once more immediately
//! before the rename, renames it over the target and fsyncs the directory.
//! A concurrent writer that ignores our lock (the agent CLIs do) can therefore
//! only slip in between that last check and the rename.
//!
//! A [`ApplyRecord`] persisted in an Ilium data directory (not a backup of the
//! file) keeps the previous value so [`revert`] can restore it, again only when
//! the file still holds the value this module wrote.
//!
//! Nothing here touches a process: no agent is restarted, and a running agent
//! keeps the limit it already loaded (the change affects new sessions only).

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io::{self, Read, Write};
use std::ops::{Range, RangeInclusive};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use ilium_platform::file_lock::ExclusiveFileLock;
use ilium_platform::secure_fs;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

/// Configuration files larger than this are refused instead of read.
pub const MAX_CONFIG_BYTES: u64 = 16 * 1024 * 1024;

/// The persisted revert record is a few hundred bytes; anything larger is not ours.
const MAX_RECORD_BYTES: u64 = 64 * 1024;

/// How long [`apply`] and [`revert`] wait for the sidecar lock before giving up.
const DEFAULT_LOCK_WAIT: Duration = Duration::from_secs(3);

/// Poll interval while waiting for a contended lock.
const LOCK_POLL_INTERVAL: Duration = Duration::from_millis(20);

/// Environment variable that Claude Code reads with precedence over the file.
pub const CLAUDE_AUTO_COMPACT_ENV: &str = "CLAUDE_CODE_AUTO_COMPACT_WINDOW";

/// Which agent setting is being written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentConfigTarget {
    /// Claude Code `autoCompactWindow` in `settings.json`.
    ClaudeAutoCompactWindow,
    /// Codex `model_auto_compact_token_limit` in `config.toml`.
    CodexAutoCompactTokenLimit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConfigFormat {
    Json,
    Toml,
}

impl AgentConfigTarget {
    /// Human-readable agent name.
    pub const fn label(self) -> &'static str {
        match self {
            Self::ClaudeAutoCompactWindow => "Claude Code",
            Self::CodexAutoCompactTokenLimit => "Codex",
        }
    }

    /// The top-level key this target writes.
    pub const fn key(self) -> &'static str {
        match self {
            Self::ClaudeAutoCompactWindow => "autoCompactWindow",
            Self::CodexAutoCompactTokenLimit => "model_auto_compact_token_limit",
        }
    }

    /// File name of the user-level configuration file.
    pub const fn file_name(self) -> &'static str {
        match self {
            Self::ClaudeAutoCompactWindow => "settings.json",
            Self::CodexAutoCompactTokenLimit => "config.toml",
        }
    }

    /// Values this module accepts for the key. Claude documents the window as
    /// 100000..=1000000; Codex takes any token count, bounded here to values a
    /// context window can plausibly hold.
    pub const fn valid_range(self) -> RangeInclusive<u64> {
        match self {
            Self::ClaudeAutoCompactWindow => 100_000..=1_000_000,
            Self::CodexAutoCompactTokenLimit => 1_000..=10_000_000,
        }
    }

    /// Where this target's user-level file lives under `paths`.
    pub fn config_path(self, paths: &ConfigPaths) -> PathBuf {
        match self {
            Self::ClaudeAutoCompactWindow => paths.claude_dir.join(self.file_name()),
            Self::CodexAutoCompactTokenLimit => paths.codex_dir.join(self.file_name()),
        }
    }

    const fn format(self) -> ConfigFormat {
        match self {
            Self::ClaudeAutoCompactWindow => ConfigFormat::Json,
            Self::CodexAutoCompactTokenLimit => ConfigFormat::Toml,
        }
    }

    const fn record_file_name(self) -> &'static str {
        match self {
            Self::ClaudeAutoCompactWindow => "agent-config-revert-claude-auto-compact-window.json",
            Self::CodexAutoCompactTokenLimit => {
                "agent-config-revert-codex-auto-compact-token-limit.json"
            }
        }
    }
}

/// Every directory this module reads from or writes beside. Injectable so
/// tests never touch the real home directory.
#[derive(Debug, Clone)]
pub struct ConfigPaths {
    /// Directory holding Claude Code's `settings.json` (`~/.claude`).
    pub claude_dir: PathBuf,
    /// Directory holding Codex's `config.toml` (`~/.codex`).
    pub codex_dir: PathBuf,
    /// Directory for the sidecar lock files (never the agents' own directories).
    pub lock_dir: PathBuf,
    /// Project directories whose project-level override files are inspected
    /// for shadowing warnings.
    pub project_dirs: Vec<PathBuf>,
    /// Environment variables that can shadow a file value. Only the variables
    /// this module knows about are consulted; [`ConfigPaths::system`] captures
    /// them from Ilium's own environment.
    pub environment: BTreeMap<String, String>,
}

impl ConfigPaths {
    /// Explicit roots with no project directories and an empty environment.
    pub fn with_roots(claude_dir: PathBuf, codex_dir: PathBuf, lock_dir: PathBuf) -> Self {
        Self {
            claude_dir,
            codex_dir,
            lock_dir,
            project_dirs: Vec::new(),
            environment: BTreeMap::new(),
        }
    }

    /// The real locations: `CLAUDE_CONFIG_DIR` or `~/.claude`, `CODEX_HOME` or
    /// `~/.codex`, Ilium's private runtime directory for lock files, and the
    /// shadowing environment variable of this process.
    pub fn system() -> Result<Self, WriteError> {
        let home = directories::BaseDirs::new()
            .map(|dirs| dirs.home_dir().to_path_buf())
            .ok_or(WriteError::HomeUnavailable)?;
        let claude_dir = std::env::var_os("CLAUDE_CONFIG_DIR")
            .map(PathBuf::from)
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| home.join(".claude"));
        let codex_dir = std::env::var_os("CODEX_HOME")
            .map(PathBuf::from)
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| home.join(".codex"));
        let lock_dir =
            ilium_platform::runtime_dir::session_socket_directory().map_err(|source| {
                WriteError::Io {
                    operation: "resolve the lock directory",
                    path: PathBuf::from("runtime directory"),
                    source,
                }
            })?;
        let mut environment = BTreeMap::new();
        if let Ok(value) = std::env::var(CLAUDE_AUTO_COMPACT_ENV) {
            environment.insert(CLAUDE_AUTO_COMPACT_ENV.to_string(), value);
        }
        Ok(Self {
            claude_dir,
            codex_dir,
            lock_dir,
            project_dirs: Vec::new(),
            environment,
        })
    }

    /// Adds a project directory whose override files are checked for warnings.
    pub fn with_project_dir(mut self, project_dir: PathBuf) -> Self {
        self.project_dirs.push(project_dir);
        self
    }
}

/// Why a read, plan, apply or revert did not happen. Variants are distinct so
/// the UI can word each one; none of them leaves a partial file behind.
#[derive(Debug, Error)]
pub enum WriteError {
    /// The configuration file does not exist (it is never created).
    #[error("{} does not exist", .path.display())]
    Missing { path: PathBuf },
    /// The path is not a regular file (directory, FIFO, socket, device).
    #[error("{} is not a regular file", .path.display())]
    NotRegular { path: PathBuf },
    /// The file exceeds [`MAX_CONFIG_BYTES`].
    #[error("{} is {size} bytes, over the {limit} byte limit", .path.display())]
    TooLarge {
        path: PathBuf,
        size: u64,
        limit: u64,
    },
    /// The file (or the edited result) is not valid, or the key holds an
    /// unexpected shape; nothing was changed.
    #[error("{}: {reason}", .path.display())]
    ParseFailed { path: PathBuf, reason: String },
    /// The requested value is outside the accepted range for the target.
    #[error("{value} is outside the accepted range {min}..={max} for {key}")]
    OutOfRange {
        key: &'static str,
        value: u64,
        min: u64,
        max: u64,
    },
    /// The file changed after it was planned (or no longer holds the value
    /// this module wrote); nothing was changed.
    #[error("{} changed since it was read: {reason}", .path.display())]
    Stale { path: PathBuf, reason: String },
    /// A filesystem operation failed.
    #[error("{operation} failed for {}: {source}", .path.display())]
    Io {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    /// The sidecar lock could not be taken (busy past the wait limit, or the
    /// lock file could not be opened).
    #[error("lock {} unavailable: {reason}", .path.display())]
    Lock { path: PathBuf, reason: String },
    /// No home directory could be resolved for the default paths.
    #[error("the home directory could not be determined")]
    HomeUnavailable,
}

fn io_error(operation: &'static str, path: &Path) -> impl FnOnce(io::Error) -> WriteError {
    let path = path.to_path_buf();
    move |source| WriteError::Io {
        operation,
        path,
        source,
    }
}

fn parse_failed(path: &Path, reason: impl Into<String>) -> WriteError {
    WriteError::ParseFailed {
        path: path.to_path_buf(),
        reason: reason.into(),
    }
}

/// Content identity of a file at one moment. Staleness compares size and
/// SHA-256 only: identical bytes cannot make a compare-and-swap unsafe, while a
/// mere `touch` should not reject a valid plan. The modification time is kept
/// for display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileFingerprint {
    pub size: u64,
    pub modified: Option<SystemTime>,
    pub sha256: String,
}

impl FileFingerprint {
    fn of(bytes: &[u8], modified: Option<SystemTime>) -> Self {
        Self {
            size: bytes.len() as u64,
            modified,
            sha256: format!("{:x}", Sha256::digest(bytes)),
        }
    }

    /// Whether both fingerprints describe identical content.
    pub fn same_content(&self, other: &Self) -> bool {
        self.size == other.size && self.sha256 == other.sha256
    }
}

/// The key's present state in the user-level file.
#[derive(Debug, Clone)]
pub struct CurrentSetting {
    pub target: AgentConfigTarget,
    /// Canonical path of the file (symlinks resolved).
    pub path: PathBuf,
    /// `None` when the key is absent.
    pub value: Option<u64>,
    pub fingerprint: FileFingerprint,
}

/// A condition the confirmation modal should mention before applying.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyWarning {
    /// An environment variable Claude Code reads with precedence is set.
    EnvironmentOverride { variable: String, value: String },
    /// A project-level file also sets the key and takes precedence there.
    ProjectOverride { path: PathBuf, value: String },
    /// A Codex profile sets the key for sessions that select that profile.
    ProfileOverride {
        profile: String,
        value: String,
        is_selected: bool,
    },
    /// The file already holds the requested value; applying changes nothing.
    ValueUnchanged,
}

impl ApplyWarning {
    /// One-sentence description for the modal.
    pub fn message(&self) -> String {
        match self {
            Self::EnvironmentOverride { variable, value } => format!(
                "{variable}={value} is set in Ilium's environment and takes precedence over \
                 the file for sessions started from it"
            ),
            Self::ProjectOverride { path, value } => format!(
                "{} sets this key to {value} and overrides the user-level value in that project",
                path.display()
            ),
            Self::ProfileOverride {
                profile,
                value,
                is_selected,
            } => {
                let selection = if *is_selected {
                    "the selected profile"
                } else {
                    "that profile when it is selected"
                };
                format!(
                    "profile \"{profile}\" sets this key to {value}; it overrides the top-level value for {selection}"
                )
            }
            Self::ValueUnchanged => "the file already holds this value; nothing will change".into(),
        }
    }
}

impl fmt::Display for ApplyWarning {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message())
    }
}

/// Everything the confirmation modal shows, and the compare-and-swap token.
#[derive(Debug, Clone)]
pub struct ApplyPlan {
    pub target: AgentConfigTarget,
    /// Canonical path the write will replace.
    pub path: PathBuf,
    pub old_value: Option<u64>,
    pub new_value: u64,
    /// Content of the file when the plan was made.
    pub fingerprint: FileFingerprint,
    /// Sidecar lock file taken by [`apply`].
    pub lock_path: PathBuf,
    /// Multi-line, human-readable change description.
    pub diff: String,
    pub warnings: Vec<ApplyWarning>,
}

/// What [`apply`] did, and what [`revert`] needs.
#[derive(Debug, Clone)]
pub struct ApplyReceipt {
    /// `false` when the file already held the value (nothing written, no
    /// record persisted).
    pub changed: bool,
    pub record: ApplyRecord,
    /// Fingerprint of the file after the write.
    pub fingerprint: FileFingerprint,
}

/// Persisted in the Ilium data directory so a later session can still revert.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplyRecord {
    pub target: AgentConfigTarget,
    /// Canonical path that was written.
    pub path: PathBuf,
    /// `None` when the key was absent before the write.
    pub previous_value: Option<u64>,
    pub written_value: u64,
    /// `YYYY-MM-DD HH:mm:ss`, local time.
    pub applied_at: String,
}

/// What [`revert`] did.
#[derive(Debug, Clone)]
pub struct RevertReceipt {
    pub target: AgentConfigTarget,
    pub path: PathBuf,
    /// The value now in the file; `None` when the key was removed again.
    pub restored_value: Option<u64>,
    pub fingerprint: FileFingerprint,
    /// Whether the persisted record was removed (a leftover record is
    /// harmless: a second revert reports [`WriteError::Stale`]).
    pub record_cleared: bool,
}

/// Reads the key from the user-level file and fingerprints the file.
pub fn read_current(
    target: AgentConfigTarget,
    paths: &ConfigPaths,
) -> Result<CurrentSetting, WriteError> {
    let loaded = load_config_file(&target.config_path(paths))?;
    let value =
        parse_setting(target, &loaded.text).map_err(|reason| parse_failed(&loaded.path, reason))?;
    Ok(CurrentSetting {
        target,
        path: loaded.path,
        value,
        fingerprint: loaded.fingerprint,
    })
}

/// Project-level and profile settings that shadow the user-level value, for
/// display only. A missing or unreadable user file contributes no profiles.
pub fn read_overrides(target: AgentConfigTarget, paths: &ConfigPaths) -> Vec<ApplyWarning> {
    let user_text = load_config_file(&target.config_path(paths))
        .map(|loaded| loaded.text)
        .unwrap_or_default();
    collect_warnings(target, paths, &user_text)
}

/// Prepares one change without writing anything: validates the file and the
/// value, previews the edit, and gathers shadowing warnings.
pub fn plan_apply(
    target: AgentConfigTarget,
    paths: &ConfigPaths,
    new_value: u64,
) -> Result<ApplyPlan, WriteError> {
    check_range(target, new_value)?;
    let loaded = load_config_file(&target.config_path(paths))?;
    let old_value =
        parse_setting(target, &loaded.text).map_err(|reason| parse_failed(&loaded.path, reason))?;
    edit_config_text(target, &loaded.text, KeyChange::Set(new_value))
        .map_err(|reason| parse_failed(&loaded.path, reason))?;

    let mut warnings = collect_warnings(target, paths, &loaded.text);
    if old_value == Some(new_value) {
        warnings.push(ApplyWarning::ValueUnchanged);
    }
    let diff = render_diff(target, &loaded.path, old_value, new_value);
    let lock_path = lock_path_for(paths, &loaded.path);
    Ok(ApplyPlan {
        target,
        path: loaded.path,
        old_value,
        new_value,
        fingerprint: loaded.fingerprint,
        lock_path,
        diff,
        warnings,
    })
}

/// Writes the planned change. Refuses with [`WriteError::Stale`] when the file
/// no longer matches the plan. `record_dir` receives the revert record, which
/// is persisted before the file is replaced.
pub fn apply(plan: &ApplyPlan, record_dir: &Path) -> Result<ApplyReceipt, WriteError> {
    apply_with(plan, record_dir, DEFAULT_LOCK_WAIT, &|| {})
}

/// Restores the value recorded by [`apply`], only when the file still holds
/// the value that was written; otherwise [`WriteError::Stale`].
pub fn revert(
    record: &ApplyRecord,
    paths: &ConfigPaths,
    record_dir: &Path,
) -> Result<RevertReceipt, WriteError> {
    revert_with(record, paths, record_dir, DEFAULT_LOCK_WAIT)
}

/// Loads the persisted record for `target`, if one exists.
pub fn load_record(
    record_dir: &Path,
    target: AgentConfigTarget,
) -> Result<Option<ApplyRecord>, WriteError> {
    let path = record_dir.join(target.record_file_name());
    let file = match secure_fs::open_regular_file(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => return Err(io_error("open the revert record", &path)(source)),
    };
    let mut bytes = Vec::new();
    file.take(MAX_RECORD_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error("read the revert record", &path))?;
    if bytes.len() as u64 > MAX_RECORD_BYTES {
        return Err(parse_failed(&path, "revert record is unexpectedly large"));
    }
    let record: ApplyRecord = serde_json::from_slice(&bytes)
        .map_err(|error| parse_failed(&path, format!("revert record is invalid: {error}")))?;
    if record.target != target {
        return Err(parse_failed(
            &path,
            "revert record belongs to another target",
        ));
    }
    Ok(Some(record))
}

/// Removes the persisted record for `target`; absent is success.
pub fn clear_record(record_dir: &Path, target: AgentConfigTarget) -> Result<(), WriteError> {
    let path = record_dir.join(target.record_file_name());
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(io_error("remove the revert record", &path)(source)),
    }
}

fn check_range(target: AgentConfigTarget, value: u64) -> Result<(), WriteError> {
    let range = target.valid_range();
    if range.contains(&value) {
        return Ok(());
    }
    Err(WriteError::OutOfRange {
        key: target.key(),
        value,
        min: *range.start(),
        max: *range.end(),
    })
}

fn apply_with(
    plan: &ApplyPlan,
    record_dir: &Path,
    lock_wait: Duration,
    after_temporary_written: &dyn Fn(),
) -> Result<ApplyReceipt, WriteError> {
    let target = plan.target;
    check_range(target, plan.new_value)?;
    let record = ApplyRecord {
        target,
        path: plan.path.clone(),
        previous_value: plan.old_value,
        written_value: plan.new_value,
        applied_at: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
    };
    let precondition = |loaded: &LoadedFile| -> Result<(), WriteError> {
        if loaded.path != plan.path {
            return Err(WriteError::Stale {
                path: plan.path.clone(),
                reason: format!("the path now resolves to {}", loaded.path.display()),
            });
        }
        if !loaded.fingerprint.same_content(&plan.fingerprint) {
            return Err(WriteError::Stale {
                path: plan.path.clone(),
                reason: "its content differs from the planned content".into(),
            });
        }
        Ok(())
    };
    // The record about to be overwritten, kept so a failed write can put it
    // back instead of losing the revert of an earlier, still-live apply.
    let prior_record = std::cell::RefCell::new(None);
    let persist_record = || {
        *prior_record.borrow_mut() = load_record(record_dir, target).ok().flatten();
        save_record(record_dir, &record)
    };
    let outcome = locked_edit(&LockedEdit {
        target,
        path: &plan.path,
        lock_path: &plan.lock_path,
        lock_wait,
        change: KeyChange::Set(plan.new_value),
        precondition: &precondition,
        before_publish: &persist_record,
        after_temporary_written,
    });
    match outcome {
        Ok(outcome) => Ok(ApplyReceipt {
            changed: outcome.changed,
            record,
            fingerprint: outcome.fingerprint,
        }),
        Err(error) => {
            // The record is written before the replacement; a failed
            // replacement must not leave a record for a write that never
            // happened. A record that cannot be restored or removed is
            // harmless because revert refuses unless the file holds the
            // recorded value.
            if matches!(load_record(record_dir, target), Ok(Some(saved)) if saved == record) {
                match prior_record.take() {
                    Some(prior) => {
                        let _ = save_record(record_dir, &prior);
                    }
                    None => {
                        let _ = clear_record(record_dir, target);
                    }
                }
            }
            Err(error)
        }
    }
}

fn revert_with(
    record: &ApplyRecord,
    paths: &ConfigPaths,
    record_dir: &Path,
    lock_wait: Duration,
) -> Result<RevertReceipt, WriteError> {
    let target = record.target;
    if record.path.file_name().and_then(|name| name.to_str()) != Some(target.file_name()) {
        return Err(parse_failed(
            &record.path,
            format!("record does not describe a {} file", target.file_name()),
        ));
    }
    let precondition = |loaded: &LoadedFile| -> Result<(), WriteError> {
        let current = parse_setting(target, &loaded.text)
            .map_err(|reason| parse_failed(&loaded.path, reason))?;
        if current == Some(record.written_value) {
            return Ok(());
        }
        let found = current.map_or_else(|| "absent".to_string(), |value| value.to_string());
        Err(WriteError::Stale {
            path: loaded.path.clone(),
            reason: format!(
                "{} is {found}, not the {} Ilium wrote",
                target.key(),
                record.written_value
            ),
        })
    };
    let change = record
        .previous_value
        .map_or(KeyChange::Remove, KeyChange::Set);
    let lock_path = lock_path_for(paths, &record.path);
    let outcome = locked_edit(&LockedEdit {
        target,
        path: &record.path,
        lock_path: &lock_path,
        lock_wait,
        change,
        precondition: &precondition,
        before_publish: &|| Ok(()),
        after_temporary_written: &|| {},
    })?;
    let record_cleared = clear_record(record_dir, target).is_ok();
    Ok(RevertReceipt {
        target,
        path: outcome.path,
        restored_value: record.previous_value,
        fingerprint: outcome.fingerprint,
        record_cleared,
    })
}

// ---------------------------------------------------------------------------
// Locked read-verify-write
// ---------------------------------------------------------------------------

struct LockedEdit<'a> {
    target: AgentConfigTarget,
    path: &'a Path,
    lock_path: &'a Path,
    lock_wait: Duration,
    change: KeyChange,
    /// Runs on the freshly re-read file, inside the lock.
    precondition: &'a dyn Fn(&LoadedFile) -> Result<(), WriteError>,
    /// Runs once the temporary file is complete, right before the final
    /// content re-check; an error aborts without touching the target.
    before_publish: &'a dyn Fn() -> Result<(), WriteError>,
    /// Test seam between the temporary file and the final re-check.
    after_temporary_written: &'a dyn Fn(),
}

struct EditOutcome {
    path: PathBuf,
    changed: bool,
    fingerprint: FileFingerprint,
}

fn locked_edit(edit: &LockedEdit<'_>) -> Result<EditOutcome, WriteError> {
    let _lock = acquire_lock(edit.lock_path, edit.lock_wait)?;
    let loaded = load_config_file(edit.path)?;
    (edit.precondition)(&loaded)?;
    let edited = edit_config_text(edit.target, &loaded.text, edit.change)
        .map_err(|reason| parse_failed(&loaded.path, reason))?;
    if edited == loaded.text {
        return Ok(EditOutcome {
            path: loaded.path,
            changed: false,
            fingerprint: loaded.fingerprint,
        });
    }
    publish_atomically(&loaded, edited.as_bytes(), edit)?;
    let after = load_config_file(&loaded.path)?;
    Ok(EditOutcome {
        path: loaded.path,
        changed: true,
        fingerprint: after.fingerprint,
    })
}

fn acquire_lock(path: &Path, wait: Duration) -> Result<ExclusiveFileLock, WriteError> {
    let deadline = Instant::now() + wait;
    loop {
        match ExclusiveFileLock::try_acquire(path) {
            Ok(Some(lock)) => return Ok(lock),
            Ok(None) if Instant::now() >= deadline => {
                return Err(WriteError::Lock {
                    path: path.to_path_buf(),
                    reason: "another Ilium process is writing this file".into(),
                });
            }
            Ok(None) => std::thread::sleep(LOCK_POLL_INTERVAL),
            Err(error) => {
                return Err(WriteError::Lock {
                    path: path.to_path_buf(),
                    reason: error.to_string(),
                });
            }
        }
    }
}

/// The sidecar lock for one canonical target. A fixed FNV-1a digest keeps the
/// name stable across Rust releases (`DefaultHasher` makes no such promise); a
/// collision only serializes two unrelated targets.
fn lock_path_for(paths: &ConfigPaths, canonical: &Path) -> PathBuf {
    let mut digest = 0xcbf2_9ce4_8422_2325_u64;
    for byte in canonical.to_string_lossy().as_bytes() {
        digest ^= u64::from(*byte);
        digest = digest.wrapping_mul(0x0100_0000_01b3);
    }
    paths
        .lock_dir
        .join(format!("agent-config-{digest:016x}.lock"))
}

// ---------------------------------------------------------------------------
// File access
// ---------------------------------------------------------------------------

struct LoadedFile {
    /// Canonical path (symlinks resolved).
    path: PathBuf,
    text: String,
    fingerprint: FileFingerprint,
    permissions: fs::Permissions,
}

fn load_config_file(path: &Path) -> Result<LoadedFile, WriteError> {
    let canonical = match ilium_platform::paths::canonicalize(path) {
        Ok(canonical) => canonical,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(WriteError::Missing {
                path: path.to_path_buf(),
            });
        }
        Err(source) => return Err(io_error("resolve the path", path)(source)),
    };
    let metadata = fs::metadata(&canonical).map_err(io_error("inspect", &canonical))?;
    if !metadata.is_file() {
        return Err(WriteError::NotRegular { path: canonical });
    }
    let file = secure_fs::open_regular_file(&canonical).map_err(|source| {
        if source.kind() == io::ErrorKind::NotFound {
            WriteError::Missing {
                path: canonical.clone(),
            }
        } else {
            io_error("open", &canonical)(source)
        }
    })?;
    let handle_metadata = file.metadata().map_err(io_error("inspect", &canonical))?;
    if !handle_metadata.is_file() {
        return Err(WriteError::NotRegular { path: canonical });
    }
    if handle_metadata.len() > MAX_CONFIG_BYTES {
        return Err(WriteError::TooLarge {
            path: canonical,
            size: handle_metadata.len(),
            limit: MAX_CONFIG_BYTES,
        });
    }
    let mut bytes = Vec::new();
    file.take(MAX_CONFIG_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error("read", &canonical))?;
    if bytes.len() as u64 > MAX_CONFIG_BYTES {
        return Err(WriteError::TooLarge {
            path: canonical,
            size: bytes.len() as u64,
            limit: MAX_CONFIG_BYTES,
        });
    }
    let fingerprint = FileFingerprint::of(&bytes, handle_metadata.modified().ok());
    let text = String::from_utf8(bytes)
        .map_err(|_| parse_failed(&canonical, "the file is not valid UTF-8"))?;
    Ok(LoadedFile {
        path: canonical,
        text,
        fingerprint,
        permissions: handle_metadata.permissions(),
    })
}

/// Writes `bytes` next to the real file, flushes it, re-checks that the target
/// still holds the content that was edited, and renames it into place. The
/// temporary file is removed on any failure.
fn publish_atomically(
    loaded: &LoadedFile,
    bytes: &[u8],
    edit: &LockedEdit<'_>,
) -> Result<(), WriteError> {
    let parent = loaded.path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = loaded
        .path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "config".to_string());
    let temporary = parent.join(format!(".{file_name}.ilium-tmp-{}", uuid::Uuid::new_v4()));
    let result = write_and_replace(loaded, bytes, &temporary, edit);
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn write_and_replace(
    loaded: &LoadedFile,
    bytes: &[u8],
    temporary: &Path,
    edit: &LockedEdit<'_>,
) -> Result<(), WriteError> {
    // Created owner-only: the file may hold secrets and its final mode is only
    // applied after the content is written.
    let mut file = secure_fs::private_open_options()
        .write(true)
        .create_new(true)
        .open(temporary)
        .map_err(io_error("create the temporary file", temporary))?;
    file.write_all(bytes)
        .map_err(io_error("write the temporary file", temporary))?;
    file.set_permissions(loaded.permissions.clone())
        .map_err(io_error("copy the permissions", temporary))?;
    file.sync_all()
        .map_err(io_error("flush the temporary file", temporary))?;
    drop(file);

    (edit.before_publish)()?;
    (edit.after_temporary_written)();

    let current = load_config_file(&loaded.path)?;
    if current.path != loaded.path || !current.fingerprint.same_content(&loaded.fingerprint) {
        return Err(WriteError::Stale {
            path: loaded.path.clone(),
            reason: "it was modified while the replacement was being prepared".into(),
        });
    }
    secure_fs::replace_file_durably(temporary, &loaded.path)
        .map_err(io_error("replace the file", &loaded.path))
}

fn save_record(record_dir: &Path, record: &ApplyRecord) -> Result<(), WriteError> {
    secure_fs::create_private_directory(record_dir)
        .map_err(io_error("create the record directory", record_dir))?;
    let destination = record_dir.join(record.target.record_file_name());
    let temporary = record_dir.join(format!(
        ".{}.tmp-{}",
        record.target.record_file_name(),
        uuid::Uuid::new_v4()
    ));
    let bytes = serde_json::to_vec_pretty(record)
        .map_err(|error| parse_failed(&destination, error.to_string()))?;
    let write = || -> Result<(), WriteError> {
        let mut file = secure_fs::private_open_options()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(io_error("create the record", &temporary))?;
        file.write_all(&bytes)
            .map_err(io_error("write the record", &temporary))?;
        file.sync_all()
            .map_err(io_error("flush the record", &temporary))?;
        drop(file);
        secure_fs::replace_file_durably(&temporary, &destination)
            .map_err(io_error("publish the record", &destination))
    };
    let result = write();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

// ---------------------------------------------------------------------------
// Warnings and diff text
// ---------------------------------------------------------------------------

fn collect_warnings(
    target: AgentConfigTarget,
    paths: &ConfigPaths,
    user_text: &str,
) -> Vec<ApplyWarning> {
    let mut warnings = Vec::new();
    if target == AgentConfigTarget::ClaudeAutoCompactWindow {
        if let Some(value) = paths
            .environment
            .get(CLAUDE_AUTO_COMPACT_ENV)
            .filter(|value| !value.trim().is_empty())
        {
            warnings.push(ApplyWarning::EnvironmentOverride {
                variable: CLAUDE_AUTO_COMPACT_ENV.to_string(),
                value: value.clone(),
            });
        }
    }
    for project_dir in &paths.project_dirs {
        for candidate in project_override_files(target, project_dir) {
            let Ok(loaded) = load_config_file(&candidate) else {
                continue;
            };
            if let Some(value) = raw_key_text(target, &loaded.text) {
                warnings.push(ApplyWarning::ProjectOverride {
                    path: loaded.path,
                    value,
                });
            }
        }
    }
    if target == AgentConfigTarget::CodexAutoCompactTokenLimit {
        warnings.extend(codex_profile_overrides(user_text));
    }
    warnings
}

fn project_override_files(target: AgentConfigTarget, project_dir: &Path) -> Vec<PathBuf> {
    match target {
        AgentConfigTarget::ClaudeAutoCompactWindow => vec![
            project_dir.join(".claude").join("settings.json"),
            project_dir.join(".claude").join("settings.local.json"),
        ],
        AgentConfigTarget::CodexAutoCompactTokenLimit => {
            vec![project_dir.join(".codex").join("config.toml")]
        }
    }
}

/// The key's value as written, whatever its type; `None` when absent or when
/// the file does not parse (an unreadable override file is not a warning).
fn raw_key_text(target: AgentConfigTarget, text: &str) -> Option<String> {
    match target.format() {
        ConfigFormat::Json => {
            let value: serde_json::Value = serde_json::from_str(text).ok()?;
            value.get(target.key()).map(ToString::to_string)
        }
        ConfigFormat::Toml => {
            let document = toml_edit::Document::parse(text).ok()?;
            let item = document.as_table().get(target.key())?;
            Some(item.as_value()?.to_string().trim().to_string())
        }
    }
}

fn codex_profile_overrides(text: &str) -> Vec<ApplyWarning> {
    let Ok(document) = toml_edit::Document::parse(text) else {
        return Vec::new();
    };
    let root = document.as_table();
    let selected = root.get("profile").and_then(|item| item.as_str());
    let Some(profiles) = root.get("profiles").and_then(|item| item.as_table_like()) else {
        return Vec::new();
    };
    profiles
        .iter()
        .filter_map(|(name, item)| {
            let profile = item.as_table_like()?;
            let value = profile
                .get(AgentConfigTarget::CodexAutoCompactTokenLimit.key())?
                .as_value()?
                .to_string()
                .trim()
                .to_string();
            Some(ApplyWarning::ProfileOverride {
                profile: name.to_string(),
                value,
                is_selected: selected == Some(name),
            })
        })
        .collect()
}

fn render_diff(
    target: AgentConfigTarget,
    path: &Path,
    old_value: Option<u64>,
    new_value: u64,
) -> String {
    let line = |value: u64| match target.format() {
        ConfigFormat::Json => format!("\"{}\": {value}", target.key()),
        ConfigFormat::Toml => format!("{} = {value}", target.key()),
    };
    let mut diff = format!(
        "{} configuration: {}\nKey: {} (top level)\n",
        target.label(),
        path.display(),
        target.key()
    );
    match old_value {
        Some(old_value) => {
            diff.push_str(&format!("- {}\n+ {}\n", line(old_value), line(new_value)));
        }
        None => {
            let placement = match target.format() {
                ConfigFormat::Json => "inserted as the first key",
                ConfigFormat::Toml => "inserted before the first table",
            };
            diff.push_str(&format!("+ {}   ({placement})\n", line(new_value)));
        }
    }
    diff.push_str("No other byte of the file changes. Only new sessions use the new value.");
    diff
}

// ---------------------------------------------------------------------------
// Pure text editing
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyChange {
    Set(u64),
    Remove,
}

/// Reads the target key from the document text. `Ok(None)` when absent.
fn parse_setting(target: AgentConfigTarget, text: &str) -> Result<Option<u64>, String> {
    match target.format() {
        ConfigFormat::Json => parse_json_setting(text, target.key()),
        ConfigFormat::Toml => parse_toml_setting(text, target.key()),
    }
}

fn parse_json_setting(text: &str, key: &str) -> Result<Option<u64>, String> {
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|error| format!("invalid JSON: {error}"))?;
    let Some(object) = value.as_object() else {
        return Err("the top level of the file is not a JSON object".into());
    };
    // `serde_json` keeps the last of two equal keys; the span scanner sees
    // both, so a duplicate is refused rather than guessed at.
    json_find_member(&scan_json_root(text)?, key)?;
    match object.get(key) {
        None => Ok(None),
        Some(value) => value
            .as_u64()
            .map(Some)
            .ok_or_else(|| format!("{key} is {value}, not a non-negative integer")),
    }
}

/// TOML parse errors render the offending source line, which can hold a secret
/// elsewhere in the file; report only the message and the line number.
fn toml_error_text(
    prefix: &str,
    text: &str,
    message: &str,
    span: Option<std::ops::Range<usize>>,
) -> String {
    match span {
        Some(span) => {
            let start = span.start.min(text.len());
            let line = text.as_bytes()[..start]
                .iter()
                .filter(|b| **b == b'\n')
                .count()
                + 1;
            format!("{prefix}: {message} (line {line})")
        }
        None => format!("{prefix}: {message}"),
    }
}

fn parse_toml_setting(text: &str, key: &str) -> Result<Option<u64>, String> {
    let document = toml_edit::Document::parse(text)
        .map_err(|error| toml_error_text("invalid TOML", text, error.message(), error.span()))?;
    match document.as_table().get(key) {
        None => Ok(None),
        Some(item) => match item.as_value().and_then(toml_edit::Value::as_integer) {
            Some(integer) => u64::try_from(integer)
                .map(Some)
                .map_err(|_| format!("{key} is {integer}, not a non-negative integer")),
            None => Err(format!("{key} is not an integer")),
        },
    }
}

/// Applies `change` to the document text and verifies the result.
fn edit_config_text(
    target: AgentConfigTarget,
    text: &str,
    change: KeyChange,
) -> Result<String, String> {
    // Parsing first also refuses duplicate keys and wrongly typed values.
    parse_setting(target, text)?;
    let edited = match target.format() {
        ConfigFormat::Json => edit_json_text(text, target.key(), change)?,
        ConfigFormat::Toml => edit_toml_text(text, target.key(), change)?,
    };
    verify_edit(target, text, &edited, change)?;
    Ok(edited)
}

fn verify_edit(
    target: AgentConfigTarget,
    original: &str,
    edited: &str,
    change: KeyChange,
) -> Result<(), String> {
    let expected = match change {
        KeyChange::Set(value) => Some(value),
        KeyChange::Remove => None,
    };
    let actual = parse_setting(target, edited).map_err(|reason| format!("edit check: {reason}"))?;
    if actual != expected {
        return Err(format!(
            "edit check: {} reads back as {actual:?}, expected {expected:?}",
            target.key()
        ));
    }
    match target.format() {
        ConfigFormat::Json => verify_json_otherwise_equal(original, edited, target.key(), change),
        ConfigFormat::Toml => verify_toml_otherwise_equal(original, edited, target.key(), change),
    }
}

fn verify_json_otherwise_equal(
    original: &str,
    edited: &str,
    key: &str,
    change: KeyChange,
) -> Result<(), String> {
    let mut expected: serde_json::Value =
        serde_json::from_str(original).map_err(|error| error.to_string())?;
    let actual: serde_json::Value =
        serde_json::from_str(edited).map_err(|error| format!("edit check: {error}"))?;
    let Some(object) = expected.as_object_mut() else {
        return Err("the top level of the file is not a JSON object".into());
    };
    match change {
        KeyChange::Set(value) => {
            object.insert(key.to_string(), serde_json::Value::from(value));
        }
        KeyChange::Remove => {
            object.remove(key);
        }
    }
    if expected != actual {
        return Err("edit check: the edit changed something besides the target key".into());
    }
    Ok(())
}

fn verify_toml_otherwise_equal(
    original: &str,
    edited: &str,
    key: &str,
    change: KeyChange,
) -> Result<(), String> {
    // `toml` implements the stricter 1.0 grammar while `toml_edit` follows 1.1;
    // when the original is outside what `toml` accepts, `toml_edit` alone vouches.
    let Ok(mut expected) = original.parse::<toml::Table>() else {
        return Ok(());
    };
    match change {
        KeyChange::Set(value) => {
            let integer =
                i64::try_from(value).map_err(|_| "value exceeds the TOML integer range")?;
            expected.insert(key.to_string(), toml::Value::Integer(integer));
        }
        KeyChange::Remove => {
            expected.remove(key);
        }
    }
    let actual = edited.parse::<toml::Table>().map_err(|error| {
        toml_error_text(
            "edit check: the edited TOML does not parse",
            edited,
            error.message(),
            error.span(),
        )
    })?;
    if expected != actual {
        return Err("edit check: the edit changed something besides the target key".into());
    }
    Ok(())
}

fn replace_range(text: &str, range: Range<usize>, replacement: &str) -> Result<String, String> {
    let (Some(head), Some(tail)) = (text.get(..range.start), text.get(range.end..)) else {
        return Err("internal span error".into());
    };
    Ok(format!("{head}{replacement}{tail}"))
}

fn line_ending(text: &str) -> &'static str {
    match text.find('\n') {
        Some(index) if index > 0 && text.as_bytes().get(index - 1) == Some(&b'\r') => "\r\n",
        _ => "\n",
    }
}

// --- TOML ------------------------------------------------------------------

fn edit_toml_text(text: &str, key: &str, change: KeyChange) -> Result<String, String> {
    let document = toml_edit::Document::parse(text)
        .map_err(|error| toml_error_text("invalid TOML", text, error.message(), error.span()))?;
    let root = document.as_table();
    match (root.get_key_value(key), change) {
        (Some((_, item)), KeyChange::Set(value)) => {
            let span = item.span().ok_or("the key has no source span")?;
            replace_range(text, span, &value.to_string())
        }
        (None, KeyChange::Set(value)) => insert_toml_line(text, root, key, value),
        (Some((key_node, item)), KeyChange::Remove) => {
            let key_span = key_node.span().ok_or("the key has no source span")?;
            let value_span = item.span().ok_or("the value has no source span")?;
            remove_toml_line(text, key_span.start, value_span.end)
        }
        (None, KeyChange::Remove) => Ok(text.to_string()),
    }
}

/// End of the last top-level key-value (dotted keys included), which is where
/// the root table's own keys stop and the first `[table]` begins.
fn last_root_value_end(table: &toml_edit::Table) -> Option<usize> {
    table
        .iter()
        .filter_map(|(_, item)| match item {
            toml_edit::Item::Value(value) => value.span().map(|span| span.end),
            toml_edit::Item::Table(inner) if inner.is_dotted() => last_root_value_end(inner),
            _ => None,
        })
        .max()
}

fn insert_toml_line(
    text: &str,
    root: &toml_edit::Table,
    key: &str,
    value: u64,
) -> Result<String, String> {
    let eol = line_ending(text);
    let line = format!("{key} = {value}");
    let Some(last_end) = last_root_value_end(root) else {
        // No top-level key-value at all: the new line leads the file.
        return replace_range(text, 0..0, &format!("{line}{eol}"));
    };
    match text.get(last_end..).and_then(|rest| rest.find('\n')) {
        Some(offset) => {
            let at = last_end + offset + 1;
            replace_range(text, at..at, &format!("{line}{eol}"))
        }
        // The last line has no terminator: keep it that way for the new last line.
        None => replace_range(text, text.len()..text.len(), &format!("{eol}{line}")),
    }
}

/// Removes the whole line holding `key = value`, provided nothing else lives
/// on it (a comment added after the value counts as something else).
fn remove_toml_line(text: &str, key_start: usize, value_end: usize) -> Result<String, String> {
    let line_start = text
        .get(..key_start)
        .ok_or("internal span error")?
        .rfind('\n')
        .map_or(0, |index| index + 1);
    if !text[line_start..key_start].trim().is_empty() {
        return Err("the key shares its line with other content".into());
    }
    let rest = text.get(value_end..).ok_or("internal span error")?;
    let (line_tail, has_terminator) = match rest.find('\n') {
        Some(index) => (&rest[..index], true),
        None => (rest, false),
    };
    if !line_tail.trim().is_empty() {
        return Err("the key's line has a comment or other content after the value".into());
    }
    if has_terminator {
        let end = value_end + line_tail.len() + 1;
        return replace_range(text, line_start..end, "");
    }
    // Last line without terminator: drop the terminator that precedes it too.
    let mut start = line_start;
    if text[..start].ends_with("\r\n") {
        start -= 2;
    } else if text[..start].ends_with('\n') {
        start -= 1;
    }
    replace_range(text, start..text.len(), "")
}

// --- JSON ------------------------------------------------------------------

struct JsonMember {
    key: String,
    key_start: usize,
    key_end: usize,
    value_start: usize,
    value_end: usize,
}

struct JsonRoot {
    open: usize,
    close: usize,
    members: Vec<JsonMember>,
}

fn json_skip_whitespace(bytes: &[u8], mut index: usize) -> usize {
    while matches!(bytes.get(index), Some(b' ' | b'\t' | b'\n' | b'\r')) {
        index += 1;
    }
    index
}

/// Index just past the closing quote of the string that starts at `start`.
fn json_string_end(bytes: &[u8], start: usize) -> Result<usize, String> {
    let mut index = start + 1;
    while let Some(byte) = bytes.get(index) {
        match byte {
            b'\\' => index += 2,
            b'"' => return Ok(index + 1),
            _ => index += 1,
        }
    }
    Err("unterminated JSON string".into())
}

/// Index just past the value that starts at `start`.
fn json_value_end(bytes: &[u8], start: usize) -> Result<usize, String> {
    match bytes.get(start) {
        None => Err("missing JSON value".into()),
        Some(b'"') => json_string_end(bytes, start),
        Some(b'{' | b'[') => {
            let mut depth = 0_usize;
            let mut index = start;
            loop {
                match bytes.get(index) {
                    None => return Err("unterminated JSON container".into()),
                    Some(b'"') => {
                        index = json_string_end(bytes, index)?;
                        continue;
                    }
                    Some(b'{' | b'[') => depth += 1,
                    Some(b'}' | b']') => {
                        depth = depth.checked_sub(1).ok_or("unbalanced JSON container")?;
                        if depth == 0 {
                            return Ok(index + 1);
                        }
                    }
                    Some(_) => {}
                }
                index += 1;
            }
        }
        Some(_) => {
            let mut index = start;
            while let Some(byte) = bytes.get(index) {
                if matches!(byte, b',' | b'}' | b']' | b' ' | b'\t' | b'\n' | b'\r') {
                    break;
                }
                index += 1;
            }
            if index == start {
                return Err("missing JSON value".into());
            }
            Ok(index)
        }
    }
}

/// Locates every member of the top-level object. The text has already been
/// validated by `serde_json`; this only needs the byte offsets.
fn scan_json_root(text: &str) -> Result<JsonRoot, String> {
    let bytes = text.as_bytes();
    let open = json_skip_whitespace(bytes, 0);
    if bytes.get(open) != Some(&b'{') {
        return Err("the top level of the file is not a JSON object".into());
    }
    let mut members = Vec::new();
    let mut index = open + 1;
    let close = loop {
        index = json_skip_whitespace(bytes, index);
        match bytes.get(index) {
            Some(b'}') => break index,
            Some(b'"') => {}
            _ => return Err("unexpected content in the top-level JSON object".into()),
        }
        let key_start = index;
        let key_end = json_string_end(bytes, key_start)?;
        let key: String = serde_json::from_str(&text[key_start..key_end])
            .map_err(|error| format!("invalid JSON key: {error}"))?;
        index = json_skip_whitespace(bytes, key_end);
        if bytes.get(index) != Some(&b':') {
            return Err("expected ':' after a JSON key".into());
        }
        let value_start = json_skip_whitespace(bytes, index + 1);
        let value_end = json_value_end(bytes, value_start)?;
        members.push(JsonMember {
            key,
            key_start,
            key_end,
            value_start,
            value_end,
        });
        index = json_skip_whitespace(bytes, value_end);
        match bytes.get(index) {
            Some(b',') => index += 1,
            Some(b'}') => break index,
            _ => return Err("expected ',' or '}' in the top-level JSON object".into()),
        }
    };
    Ok(JsonRoot {
        open,
        close,
        members,
    })
}

/// The one member named `key`; two of them are refused.
fn json_find_member<'a>(root: &'a JsonRoot, key: &str) -> Result<Option<&'a JsonMember>, String> {
    let mut matches = root.members.iter().filter(|member| member.key == key);
    let first = matches.next();
    if matches.next().is_some() {
        return Err(format!("the key {key} appears more than once"));
    }
    Ok(first)
}

fn edit_json_text(text: &str, key: &str, change: KeyChange) -> Result<String, String> {
    let root = scan_json_root(text)?;
    let member = json_find_member(&root, key)?;
    match (member, change) {
        (Some(member), KeyChange::Set(value)) => replace_range(
            text,
            member.value_start..member.value_end,
            &value.to_string(),
        ),
        (None, KeyChange::Set(value)) => {
            insert_json_member(text, &root, key, &serde_json::Value::from(value))
        }
        (Some(member), KeyChange::Remove) => remove_json_member(text, &root, member),
        (None, KeyChange::Remove) => Ok(text.to_string()),
    }
}

/// Edits one top-level JSON value while preserving every other byte. Used by
/// managed integrations that need to own a structured setting temporarily.
pub(crate) fn edit_top_level_json_value(
    text: &str,
    key: &str,
    replacement: Option<serde_json::Value>,
) -> Result<String, String> {
    let mut expected: serde_json::Value =
        serde_json::from_str(text).map_err(|error| format!("invalid JSON: {error}"))?;
    let object = expected
        .as_object_mut()
        .ok_or_else(|| "the top level of the file is not a JSON object".to_owned())?;
    let root = scan_json_root(text)?;
    let member = json_find_member(&root, key)?;
    let edited = match (member, replacement.as_ref()) {
        (Some(member), Some(value)) => replace_range(
            text,
            member.value_start..member.value_end,
            &serde_json::to_string(value).map_err(|error| error.to_string())?,
        )?,
        (None, Some(value)) => insert_json_member(text, &root, key, value)?,
        (Some(member), None) => remove_json_member(text, &root, member)?,
        (None, None) => text.to_owned(),
    };
    match replacement {
        Some(value) => {
            object.insert(key.to_owned(), value);
        }
        None => {
            object.remove(key);
        }
    }
    let actual: serde_json::Value = serde_json::from_str(&edited)
        .map_err(|error| format!("edited JSON is invalid: {error}"))?;
    if expected != actual {
        return Err("edit check: the edit changed something besides the target key".into());
    }
    Ok(edited)
}

/// Restores one top-level JSON value from its original source slice so a
/// managed setting can put the user's exact formatting back when disabling.
pub(crate) fn restore_top_level_json_value_text(
    text: &str,
    key: &str,
    replacement: Option<&str>,
) -> Result<String, String> {
    let mut expected: serde_json::Value =
        serde_json::from_str(text).map_err(|error| format!("invalid JSON: {error}"))?;
    let object = expected
        .as_object_mut()
        .ok_or_else(|| "the top level of the file is not a JSON object".to_owned())?;
    let replacement_value: Option<serde_json::Value> = replacement
        .map(serde_json::from_str)
        .transpose()
        .map_err(|error| format!("invalid replacement JSON: {error}"))?;
    let root = scan_json_root(text)?;
    let member = json_find_member(&root, key)?;
    let edited = match (member, replacement) {
        (Some(member), Some(raw)) => {
            replace_range(text, member.value_start..member.value_end, raw)?
        }
        (None, Some(_)) => {
            let value = replacement_value
                .as_ref()
                .expect("parsed replacement exists");
            insert_json_member(text, &root, key, value)?
        }
        (Some(member), None) => remove_json_member(text, &root, member)?,
        (None, None) => text.to_owned(),
    };
    match replacement_value {
        Some(value) => {
            object.insert(key.to_owned(), value);
        }
        None => {
            object.remove(key);
        }
    }
    let actual: serde_json::Value = serde_json::from_str(&edited)
        .map_err(|error| format!("edited JSON is invalid: {error}"))?;
    if expected != actual {
        return Err("edit check: the edit changed something besides the target key".into());
    }
    Ok(edited)
}

pub(crate) fn top_level_json_value_text(text: &str, key: &str) -> Result<Option<String>, String> {
    let _: serde_json::Value =
        serde_json::from_str(text).map_err(|error| format!("invalid JSON: {error}"))?;
    let root = scan_json_root(text)?;
    let Some(member) = json_find_member(&root, key)? else {
        return Ok(None);
    };
    Ok(Some(
        text.get(member.value_start..member.value_end)
            .ok_or("internal JSON span error")?
            .to_owned(),
    ))
}

/// Inserts `"key": value` as the first member, reusing the whitespace that
/// already follows `{` and the separator style of the existing first member.
fn insert_json_member(
    text: &str,
    root: &JsonRoot,
    key: &str,
    value: &serde_json::Value,
) -> Result<String, String> {
    let value = serde_json::to_string(value).map_err(|error| error.to_string())?;
    if let Some(first) = root.members.first() {
        let leading = text
            .get(root.open + 1..first.key_start)
            .ok_or("internal span error")?;
        let separator = text
            .get(first.key_end..first.value_start)
            .filter(|separator| !separator.contains('\n'))
            .unwrap_or(": ");
        let member = format!("\"{key}\"{separator}{value},{leading}");
        return replace_range(text, first.key_start..first.key_start, &member);
    }
    let interior = text
        .get(root.open + 1..root.close)
        .ok_or("internal span error")?;
    let replacement = match interior.rfind('\n') {
        Some(index) => {
            let eol = line_ending(text);
            let closing_indent = &interior[index + 1..];
            format!("{eol}{closing_indent}  \"{key}\": {value}{eol}{closing_indent}")
        }
        None => format!("\"{key}\": {value}"),
    };
    replace_range(text, root.open + 1..root.close, &replacement)
}

/// Removes one member together with the comma and whitespace that tie it to a
/// neighbour. For a member this module inserted as the first key, this restores
/// the original bytes exactly.
fn remove_json_member(text: &str, root: &JsonRoot, member: &JsonMember) -> Result<String, String> {
    let position = root
        .members
        .iter()
        .position(|candidate| candidate.key_start == member.key_start)
        .ok_or("internal member lookup error")?;
    if let Some(next) = root.members.get(position + 1) {
        return replace_range(text, member.key_start..next.key_start, "");
    }
    match position
        .checked_sub(1)
        .and_then(|prior| root.members.get(prior))
    {
        Some(previous) => replace_range(text, previous.value_end..member.value_end, ""),
        None => replace_range(text, root.open + 1..root.close, ""),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLAUDE: AgentConfigTarget = AgentConfigTarget::ClaudeAutoCompactWindow;
    const CODEX: AgentConfigTarget = AgentConfigTarget::CodexAutoCompactTokenLimit;

    struct Fixture {
        root: tempfile::TempDir,
        paths: ConfigPaths,
        record_dir: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let root = tempfile::tempdir().expect("temp dir");
            let claude_dir = root.path().join("claude");
            let codex_dir = root.path().join("codex");
            fs::create_dir_all(&claude_dir).expect("claude dir");
            fs::create_dir_all(&codex_dir).expect("codex dir");
            let paths = ConfigPaths::with_roots(claude_dir, codex_dir, root.path().join("locks"));
            let record_dir = root.path().join("data");
            Self {
                root,
                paths,
                record_dir,
            }
        }

        fn path(&self, target: AgentConfigTarget) -> PathBuf {
            target.config_path(&self.paths)
        }

        fn write(&self, target: AgentConfigTarget, contents: &str) {
            fs::write(self.path(target), contents).expect("write fixture");
        }

        fn read(&self, target: AgentConfigTarget) -> String {
            fs::read_to_string(self.path(target)).expect("read fixture")
        }

        fn apply(&self, target: AgentConfigTarget, value: u64) -> ApplyReceipt {
            let plan = plan_apply(target, &self.paths, value).expect("plan");
            apply(&plan, &self.record_dir).expect("apply")
        }

        fn leftover_temporaries(&self, directory: &Path) -> usize {
            fs::read_dir(directory)
                .expect("read dir")
                .filter_map(Result::ok)
                .filter(|entry| entry.file_name().to_string_lossy().contains("ilium-tmp"))
                .count()
        }
    }

    fn json_set(text: &str, value: u64) -> String {
        edit_config_text(CLAUDE, text, KeyChange::Set(value)).expect("json set")
    }

    fn toml_set(text: &str, value: u64) -> String {
        edit_config_text(CODEX, text, KeyChange::Set(value)).expect("toml set")
    }

    // --- JSON text edits ---------------------------------------------------

    #[test]
    fn json_replaces_the_value_and_keeps_every_other_byte() {
        let original = "{\n  \"theme\": \"dark\",\n  \"autoCompactWindow\":   567000 ,\n  \"name\": \"Zoë 😀\",\n  \"list\": [1,  2]\n}\n";
        let edited = json_set(original, 250_000);
        assert_eq!(
            edited,
            "{\n  \"theme\": \"dark\",\n  \"autoCompactWindow\":   250000 ,\n  \"name\": \"Zoë 😀\",\n  \"list\": [1,  2]\n}\n"
        );
    }

    #[test]
    fn json_inserts_first_key_with_tab_indentation_and_crlf() {
        let original = "{\r\n\t\"alpha\": 1,\r\n\t\"beta\": [1, 2]\r\n}";
        let edited = json_set(original, 300_000);
        assert_eq!(
            edited,
            "{\r\n\t\"autoCompactWindow\": 300000,\r\n\t\"alpha\": 1,\r\n\t\"beta\": [1, 2]\r\n}"
        );
        assert!(
            !edited.ends_with('\n'),
            "trailing newline presence is preserved"
        );
    }

    #[test]
    fn json_inserts_first_key_with_two_space_indentation_and_trailing_newline() {
        let original = "{\n  \"alpha\": {\"x\": 1},\n  \"beta\": true\n}\n";
        assert_eq!(
            json_set(original, 250_000),
            "{\n  \"autoCompactWindow\": 250000,\n  \"alpha\": {\"x\": 1},\n  \"beta\": true\n}\n"
        );
    }

    #[test]
    fn json_inserts_into_compact_and_spaced_objects() {
        assert_eq!(
            json_set("{\"a\":1}", 200_000),
            "{\"autoCompactWindow\":200000,\"a\":1}"
        );
        assert_eq!(
            json_set("{ \"a\" : 1 }", 200_000),
            "{ \"autoCompactWindow\" : 200000, \"a\" : 1 }"
        );
    }

    #[test]
    fn json_inserts_into_empty_objects() {
        assert_eq!(json_set("{}", 200_000), "{\"autoCompactWindow\": 200000}");
        assert_eq!(
            json_set("{\n}\n", 200_000),
            "{\n  \"autoCompactWindow\": 200000\n}\n"
        );
        assert_eq!(
            json_set("{\r\n}", 200_000),
            "{\r\n  \"autoCompactWindow\": 200000\r\n}"
        );
    }

    #[test]
    fn json_nested_same_named_key_is_not_the_top_level_key() {
        let original = "{\n  \"nested\": {\"autoCompactWindow\": 5},\n  \"other\": 1\n}\n";
        assert_eq!(parse_setting(CLAUDE, original), Ok(None));
        let edited = json_set(original, 250_000);
        assert!(edited.contains("\"nested\": {\"autoCompactWindow\": 5}"));
        assert_eq!(parse_setting(CLAUDE, &edited), Ok(Some(250_000)));
    }

    #[test]
    fn json_values_with_braces_and_escapes_do_not_confuse_the_scanner() {
        let original = "{\n  \"cmd\": \"echo \\\"}\\\" , {\",\n  \"arr\": [\"]\", {\"k\": \"}\"}],\n  \"autoCompactWindow\": 400000\n}\n";
        let edited = json_set(original, 250_000);
        assert_eq!(edited, original.replace("400000", "250000"));
    }

    #[test]
    fn json_insert_then_remove_restores_the_original_bytes() {
        let shapes = [
            "{\n  \"a\": 1,\n  \"b\": {\"c\": [1, 2, 3]}\n}\n",
            "{\r\n\t\"a\": 1\r\n}",
            "{\"a\":1,\"b\":2}",
            "{ \"a\": 1, \"b\": 2 }\n",
            "{\n  \"only\": \"é\"\n}",
        ];
        for original in shapes {
            let inserted = json_set(original, 250_000);
            let removed = edit_config_text(CLAUDE, &inserted, KeyChange::Remove).expect("remove");
            assert_eq!(removed, original, "shape {original:?}");
        }
    }

    #[test]
    fn json_removal_handles_first_middle_last_and_only_members() {
        let first = "{\n  \"autoCompactWindow\": 1,\n  \"b\": 2\n}\n";
        assert_eq!(
            edit_json_text(first, "autoCompactWindow", KeyChange::Remove).unwrap(),
            "{\n  \"b\": 2\n}\n"
        );
        let middle = "{\n  \"a\": 1,\n  \"autoCompactWindow\": 1,\n  \"b\": 2\n}\n";
        assert_eq!(
            edit_json_text(middle, "autoCompactWindow", KeyChange::Remove).unwrap(),
            "{\n  \"a\": 1,\n  \"b\": 2\n}\n"
        );
        let last = "{\n  \"a\": 1,\n  \"autoCompactWindow\": 1\n}\n";
        assert_eq!(
            edit_json_text(last, "autoCompactWindow", KeyChange::Remove).unwrap(),
            "{\n  \"a\": 1\n}\n"
        );
        assert_eq!(
            edit_json_text(
                "{\"autoCompactWindow\": 1}",
                "autoCompactWindow",
                KeyChange::Remove
            )
            .unwrap(),
            "{}"
        );
    }

    #[test]
    fn json_invalid_duplicate_and_mistyped_input_is_refused() {
        let cases = [
            (
                "{\"autoCompactWindow\": 1, \"autoCompactWindow\": 2}",
                "more than once",
            ),
            ("{\"a\": 1,}", "invalid JSON"),
            ("{\"a\": 1", "invalid JSON"),
            ("// comment\n{\"a\": 1}", "invalid JSON"),
            ("[1, 2]", "not a JSON object"),
            ("", "invalid JSON"),
            (
                "{\"autoCompactWindow\": \"250000\"}",
                "not a non-negative integer",
            ),
            (
                "{\"autoCompactWindow\": 250000.5}",
                "not a non-negative integer",
            ),
            (
                "{\"autoCompactWindow\": null}",
                "not a non-negative integer",
            ),
            ("{\"autoCompactWindow\": -4}", "not a non-negative integer"),
        ];
        for (input, expected) in cases {
            let error = edit_config_text(CLAUDE, input, KeyChange::Set(250_000)).unwrap_err();
            assert!(error.contains(expected), "{input:?} -> {error}");
        }
    }

    // --- TOML text edits ---------------------------------------------------

    const CODEX_SAMPLE: &str = "# Codex config\nmodel = \"gpt-6\"   # keep me\n\n# Trust prompts\n[projects.\"/home/a\"]\ntrust_level = \"trusted\"\nmodel_auto_compact_token_limit = 7\n\n[projects.\"/home/b\"]\ntrust_level = \"untrusted\"\n";

    #[test]
    fn toml_inserts_before_the_first_table_and_leaves_table_keys_alone() {
        let edited = toml_set(CODEX_SAMPLE, 250_000);
        assert_eq!(
            edited,
            "# Codex config\nmodel = \"gpt-6\"   # keep me\nmodel_auto_compact_token_limit = 250000\n\n# Trust prompts\n[projects.\"/home/a\"]\ntrust_level = \"trusted\"\nmodel_auto_compact_token_limit = 7\n\n[projects.\"/home/b\"]\ntrust_level = \"untrusted\"\n"
        );
        let document = toml_edit::Document::parse(edited.as_str()).unwrap();
        let table = document.as_table()["projects"]["/home/a"]
            .as_table()
            .unwrap();
        assert_eq!(
            table["model_auto_compact_token_limit"].as_integer(),
            Some(7)
        );
    }

    #[test]
    fn toml_replaces_an_existing_top_level_value_keeping_its_comment() {
        let original =
            "a = 1\nmodel_auto_compact_token_limit = 100_000   # tuned by hand\n\n[t]\nx = 1\n";
        assert_eq!(
            toml_set(original, 250_000),
            "a = 1\nmodel_auto_compact_token_limit = 250000   # tuned by hand\n\n[t]\nx = 1\n"
        );
    }

    #[test]
    fn toml_insert_then_remove_restores_the_original_bytes() {
        let shapes = [
            CODEX_SAMPLE,
            "# only a comment\n\n[t]\nx = 1\n",
            "[t]\nx = 1\n",
            "a = 1\nb = 2",
            "a = 1\r\nb = 2\r\n\r\n[t]\r\nx = 1\r\n",
            "a = 1",
            "",
            "# header only\n",
            "a.b = 1\nc = [\n  1,\n  2,\n]\n[t]\n",
            "title = \"é 😀\"\n[t]\nx = 1\n",
        ];
        for original in shapes {
            let inserted = toml_set(original, 250_000);
            assert_eq!(
                parse_setting(CODEX, &inserted),
                Ok(Some(250_000)),
                "{original:?}"
            );
            let removed = edit_config_text(CODEX, &inserted, KeyChange::Remove).expect("remove");
            assert_eq!(removed, original, "shape {original:?}");
        }
    }

    #[test]
    fn toml_insertion_follows_the_file_line_ending() {
        let edited = toml_set("a = 1\r\n\r\n[t]\r\nx = 1\r\n", 250_000);
        assert_eq!(
            edited,
            "a = 1\r\nmodel_auto_compact_token_limit = 250000\r\n\r\n[t]\r\nx = 1\r\n"
        );
        let unterminated = toml_set("a = 1", 250_000);
        assert_eq!(
            unterminated,
            "a = 1\nmodel_auto_compact_token_limit = 250000"
        );
    }

    #[test]
    fn toml_parse_errors_never_echo_source_lines() {
        let text = "a = 1\nSECRET_KEY = \"sk-SECRET123\" oops\n";
        let error = parse_toml_setting(text, "model_auto_compact_token_limit").unwrap_err();
        assert!(error.starts_with("invalid TOML"), "{error}");
        assert!(!error.contains("SECRET"), "{error}");
        assert!(error.contains("line 2"), "{error}");
    }

    #[test]
    fn toml_invalid_duplicate_and_mistyped_input_is_refused() {
        let cases = [
            (
                "model_auto_compact_token_limit = 1\nmodel_auto_compact_token_limit = 2\n",
                "invalid TOML",
            ),
            ("a = \n", "invalid TOML"),
            ("[t\nx = 1\n", "invalid TOML"),
            (
                "model_auto_compact_token_limit = \"250000\"\n",
                "not an integer",
            ),
            ("model_auto_compact_token_limit = 2.5\n", "not an integer"),
            (
                "model_auto_compact_token_limit = -3\n",
                "not a non-negative integer",
            ),
        ];
        for (input, expected) in cases {
            let error = edit_config_text(CODEX, input, KeyChange::Set(250_000)).unwrap_err();
            assert!(error.contains(expected), "{input:?} -> {error}");
        }
    }

    #[test]
    fn toml_removal_refuses_a_line_that_gained_a_comment() {
        let text = "a = 1\nmodel_auto_compact_token_limit = 5 # mine\n";
        let error = edit_config_text(CODEX, text, KeyChange::Remove).unwrap_err();
        assert!(error.contains("comment"), "{error}");
    }

    #[test]
    fn toml_large_file_with_many_tables_changes_only_one_line() {
        let mut original = String::from("# header\nmodel = \"gpt-6\"\n\n");
        for index in 0..1_488 {
            original.push_str(&format!(
                "# project {index}\n[projects.\"/home/arthur/dev/p{index}\"]\ntrust_level = \"trusted\"\nmodel_auto_compact_token_limit = {index}\n\n"
            ));
        }
        let edited = toml_set(&original, 250_000);
        let inserted_line = "model_auto_compact_token_limit = 250000\n";
        assert_eq!(edited.replacen(inserted_line, "", 1), original);
        assert!(edited.starts_with(
            "# header\nmodel = \"gpt-6\"\nmodel_auto_compact_token_limit = 250000\n\n# project 0\n"
        ));
    }

    // --- read_current ------------------------------------------------------

    #[test]
    fn read_current_reports_value_absence_and_fingerprint() {
        let fixture = Fixture::new();
        fixture.write(CLAUDE, "{\"autoCompactWindow\": 300000}\n");
        fixture.write(CODEX, "a = 1\n");
        let claude = read_current(CLAUDE, &fixture.paths).unwrap();
        assert_eq!(claude.value, Some(300_000));
        assert_eq!(claude.fingerprint.size, 30);
        assert_eq!(claude.fingerprint.sha256.len(), 64);
        assert!(claude.fingerprint.modified.is_some());
        let codex = read_current(CODEX, &fixture.paths).unwrap();
        assert_eq!(codex.value, None);
    }

    #[test]
    fn read_current_distinguishes_missing_not_regular_too_large_and_unparsable() {
        let fixture = Fixture::new();
        assert!(matches!(
            read_current(CLAUDE, &fixture.paths),
            Err(WriteError::Missing { .. })
        ));
        fs::create_dir(fixture.path(CLAUDE)).unwrap();
        assert!(matches!(
            read_current(CLAUDE, &fixture.paths),
            Err(WriteError::NotRegular { .. })
        ));
        fs::remove_dir(fixture.path(CLAUDE)).unwrap();
        fixture.write(CLAUDE, "{not json");
        assert!(matches!(
            read_current(CLAUDE, &fixture.paths),
            Err(WriteError::ParseFailed { .. })
        ));
        let oversized = vec![b' '; (MAX_CONFIG_BYTES + 1) as usize];
        fs::write(fixture.path(CODEX), oversized).unwrap();
        assert!(matches!(
            read_current(CODEX, &fixture.paths),
            Err(WriteError::TooLarge { .. })
        ));
        fs::write(fixture.path(CODEX), [0xff, 0xfe, 0x00]).unwrap();
        assert!(matches!(
            read_current(CODEX, &fixture.paths),
            Err(WriteError::ParseFailed { .. })
        ));
    }

    // --- plan_apply --------------------------------------------------------

    #[test]
    fn plan_rejects_out_of_range_values() {
        let fixture = Fixture::new();
        fixture.write(CLAUDE, "{}");
        fixture.write(CODEX, "");
        for value in [0, 99_999, 1_000_001, u64::MAX] {
            assert!(matches!(
                plan_apply(CLAUDE, &fixture.paths, value),
                Err(WriteError::OutOfRange { .. })
            ));
        }
        assert!(plan_apply(CLAUDE, &fixture.paths, 100_000).is_ok());
        assert!(plan_apply(CLAUDE, &fixture.paths, 1_000_000).is_ok());
        assert!(matches!(
            plan_apply(CODEX, &fixture.paths, 999),
            Err(WriteError::OutOfRange { .. })
        ));
    }

    #[test]
    fn plan_diff_names_file_key_and_old_and_new_values() {
        let fixture = Fixture::new();
        fixture.write(CLAUDE, "{\"autoCompactWindow\": 567000}");
        fixture.write(CODEX, "a = 1\n");
        let claude = plan_apply(CLAUDE, &fixture.paths, 250_000).unwrap();
        assert_eq!(claude.old_value, Some(567_000));
        assert!(claude.diff.contains(&claude.path.display().to_string()));
        assert!(claude.diff.contains("- \"autoCompactWindow\": 567000"));
        assert!(claude.diff.contains("+ \"autoCompactWindow\": 250000"));
        assert!(claude.diff.contains("new sessions"));
        let codex = plan_apply(CODEX, &fixture.paths, 200_000).unwrap();
        assert_eq!(codex.old_value, None);
        assert!(codex.diff.contains(
            "+ model_auto_compact_token_limit = 200000   (inserted before the first table)"
        ));
        assert!(codex.warnings.is_empty());
    }

    #[test]
    fn plan_warns_when_the_environment_shadows_the_claude_file_value() {
        let mut fixture = Fixture::new();
        fixture.write(CLAUDE, "{}");
        fixture
            .paths
            .environment
            .insert(CLAUDE_AUTO_COMPACT_ENV.into(), "400000".into());
        let plan = plan_apply(CLAUDE, &fixture.paths, 250_000).unwrap();
        assert_eq!(
            plan.warnings,
            vec![ApplyWarning::EnvironmentOverride {
                variable: CLAUDE_AUTO_COMPACT_ENV.into(),
                value: "400000".into()
            }]
        );
        assert!(plan.warnings[0].message().contains("takes precedence"));
        // A blank variable shadows nothing, and Codex ignores it.
        fixture
            .paths
            .environment
            .insert(CLAUDE_AUTO_COMPACT_ENV.into(), "  ".into());
        assert!(plan_apply(CLAUDE, &fixture.paths, 250_000)
            .unwrap()
            .warnings
            .is_empty());
        fixture
            .paths
            .environment
            .insert(CLAUDE_AUTO_COMPACT_ENV.into(), "400000".into());
        fixture.write(CODEX, "");
        assert!(plan_apply(CODEX, &fixture.paths, 250_000)
            .unwrap()
            .warnings
            .is_empty());
    }

    #[test]
    fn plan_warns_about_project_level_override_files() {
        let fixture = Fixture::new();
        fixture.write(CLAUDE, "{}");
        fixture.write(CODEX, "");
        let project = fixture.root.path().join("project");
        fs::create_dir_all(project.join(".claude")).unwrap();
        fs::create_dir_all(project.join(".codex")).unwrap();
        fs::write(
            project.join(".claude/settings.local.json"),
            "{\"autoCompactWindow\": 150000}",
        )
        .unwrap();
        fs::write(project.join(".claude/settings.json"), "{\"other\": 1}").unwrap();
        fs::write(
            project.join(".codex/config.toml"),
            "model_auto_compact_token_limit = 90000\n",
        )
        .unwrap();
        let paths = fixture.paths.clone().with_project_dir(project.clone());
        let claude = plan_apply(CLAUDE, &paths, 250_000).unwrap();
        assert_eq!(claude.warnings.len(), 1);
        assert!(matches!(
            &claude.warnings[0],
            ApplyWarning::ProjectOverride { path, value }
                if path.ends_with("settings.local.json") && value == "150000"
        ));
        let codex = plan_apply(CODEX, &paths, 250_000).unwrap();
        assert!(matches!(
            &codex.warnings[..],
            [ApplyWarning::ProjectOverride { value, .. }] if value == "90000"
        ));
    }

    #[test]
    fn plan_warns_about_codex_profiles_that_set_the_key() {
        let fixture = Fixture::new();
        fixture.write(
            CODEX,
            "profile = \"fast\"\n\n[profiles.fast]\nmodel_auto_compact_token_limit = 50000\n\n[profiles.slow]\nmodel = \"x\"\n",
        );
        let plan = plan_apply(CODEX, &fixture.paths, 250_000).unwrap();
        assert_eq!(
            plan.warnings,
            vec![ApplyWarning::ProfileOverride {
                profile: "fast".into(),
                value: "50000".into(),
                is_selected: true
            }]
        );
    }

    #[test]
    fn plan_flags_an_unchanged_value() {
        let fixture = Fixture::new();
        fixture.write(CLAUDE, "{\"autoCompactWindow\": 250000}");
        let plan = plan_apply(CLAUDE, &fixture.paths, 250_000).unwrap();
        assert!(plan.warnings.contains(&ApplyWarning::ValueUnchanged));
    }

    // --- apply / revert ----------------------------------------------------

    #[test]
    fn apply_updates_an_existing_json_value_and_reverts_it() {
        let fixture = Fixture::new();
        let original = "{\n  \"theme\": \"dark\",\n  \"autoCompactWindow\": 567000\n}\n";
        fixture.write(CLAUDE, original);
        let receipt = fixture.apply(CLAUDE, 250_000);
        assert!(receipt.changed);
        assert_eq!(receipt.record.previous_value, Some(567_000));
        assert_eq!(fixture.read(CLAUDE), original.replace("567000", "250000"));
        assert_eq!(fixture.leftover_temporaries(&fixture.paths.claude_dir), 0);

        let record = load_record(&fixture.record_dir, CLAUDE).unwrap().unwrap();
        assert_eq!(record, receipt.record);
        let reverted = revert(&record, &fixture.paths, &fixture.record_dir).unwrap();
        assert_eq!(reverted.restored_value, Some(567_000));
        assert!(reverted.record_cleared);
        assert_eq!(fixture.read(CLAUDE), original);
        assert!(load_record(&fixture.record_dir, CLAUDE).unwrap().is_none());
    }

    #[test]
    fn apply_inserts_an_absent_key_and_revert_removes_it_byte_exactly() {
        let fixture = Fixture::new();
        let claude = "{\r\n\t\"theme\": \"dark\"\r\n}";
        fixture.write(CLAUDE, claude);
        let codex = CODEX_SAMPLE;
        fixture.write(CODEX, codex);
        for (target, original) in [(CLAUDE, claude), (CODEX, codex)] {
            let receipt = fixture.apply(target, 250_000);
            assert_eq!(receipt.record.previous_value, None);
            assert_eq!(
                parse_setting(target, &fixture.read(target)),
                Ok(Some(250_000))
            );
            revert(&receipt.record, &fixture.paths, &fixture.record_dir).unwrap();
            assert_eq!(fixture.read(target), original);
        }
    }

    #[test]
    fn apply_changes_codex_values_inside_the_top_level_region_only() {
        let fixture = Fixture::new();
        fixture.write(CODEX, CODEX_SAMPLE);
        fixture.apply(CODEX, 123_456);
        let text = fixture.read(CODEX);
        let first_table = text.find("[projects.").unwrap();
        let key_line = text
            .find("model_auto_compact_token_limit = 123456")
            .unwrap();
        assert!(key_line < first_table);
        assert_eq!(text.matches("model_auto_compact_token_limit").count(), 2);
    }

    #[test]
    fn apply_with_an_unchanged_value_writes_nothing_and_keeps_no_record() {
        let fixture = Fixture::new();
        fixture.write(CLAUDE, "{\"autoCompactWindow\": 250000}");
        let before = fs::metadata(fixture.path(CLAUDE))
            .unwrap()
            .modified()
            .unwrap();
        let receipt = fixture.apply(CLAUDE, 250_000);
        assert!(!receipt.changed);
        assert_eq!(
            fs::metadata(fixture.path(CLAUDE))
                .unwrap()
                .modified()
                .unwrap(),
            before
        );
        assert!(load_record(&fixture.record_dir, CLAUDE).unwrap().is_none());
    }

    #[test]
    fn apply_refuses_a_file_changed_between_plan_and_apply() {
        let fixture = Fixture::new();
        fixture.write(CLAUDE, "{\"autoCompactWindow\": 567000}");
        let plan = plan_apply(CLAUDE, &fixture.paths, 250_000).unwrap();
        let concurrent = "{\"autoCompactWindow\": 567000, \"added\": \"by the agent\"}";
        fixture.write(CLAUDE, concurrent);
        let error = apply(&plan, &fixture.record_dir).unwrap_err();
        assert!(matches!(error, WriteError::Stale { .. }), "{error}");
        assert_eq!(fixture.read(CLAUDE), concurrent);
        assert!(load_record(&fixture.record_dir, CLAUDE).unwrap().is_none());
        assert_eq!(fixture.leftover_temporaries(&fixture.paths.claude_dir), 0);
    }

    #[test]
    fn apply_refuses_a_codex_file_changed_between_plan_and_apply() {
        let fixture = Fixture::new();
        fixture.write(CODEX, "a = 1\n");
        let plan = plan_apply(CODEX, &fixture.paths, 250_000).unwrap();
        fixture.write(CODEX, "a = 2\n");
        assert!(matches!(
            apply(&plan, &fixture.record_dir),
            Err(WriteError::Stale { .. })
        ));
        assert_eq!(fixture.read(CODEX), "a = 2\n");
    }

    #[test]
    fn a_concurrent_write_after_the_temporary_file_is_caught_by_the_final_check() {
        let fixture = Fixture::new();
        fixture.write(CLAUDE, "{\"autoCompactWindow\": 567000}");
        let plan = plan_apply(CLAUDE, &fixture.paths, 250_000).unwrap();
        let target_path = fixture.path(CLAUDE);
        let concurrent = "{\"autoCompactWindow\": 567000, \"x\": 1}";
        let interfere = || fs::write(&target_path, concurrent).unwrap();
        let error = apply_with(
            &plan,
            &fixture.record_dir,
            Duration::from_secs(1),
            &interfere,
        )
        .unwrap_err();
        assert!(matches!(error, WriteError::Stale { .. }), "{error}");
        assert_eq!(fixture.read(CLAUDE), concurrent);
        assert_eq!(fixture.leftover_temporaries(&fixture.paths.claude_dir), 0);
        assert!(
            load_record(&fixture.record_dir, CLAUDE).unwrap().is_none(),
            "no record may survive a write that did not happen"
        );
    }

    #[test]
    fn a_failing_record_store_aborts_before_the_file_is_touched() {
        let fixture = Fixture::new();
        let original = "{\"autoCompactWindow\": 567000}";
        fixture.write(CLAUDE, original);
        let plan = plan_apply(CLAUDE, &fixture.paths, 250_000).unwrap();
        // A regular file where the record directory should be.
        let blocked = fixture.root.path().join("blocked");
        fs::write(&blocked, "x").unwrap();
        let error = apply(&plan, &blocked.join("records")).unwrap_err();
        assert!(matches!(error, WriteError::Io { .. }), "{error}");
        assert_eq!(fixture.read(CLAUDE), original);
        assert_eq!(fixture.leftover_temporaries(&fixture.paths.claude_dir), 0);
    }

    #[test]
    fn apply_validates_the_value_again_even_for_a_hand_built_plan() {
        let fixture = Fixture::new();
        fixture.write(CLAUDE, "{}");
        let mut plan = plan_apply(CLAUDE, &fixture.paths, 250_000).unwrap();
        plan.new_value = 5;
        assert!(matches!(
            apply(&plan, &fixture.record_dir),
            Err(WriteError::OutOfRange { .. })
        ));
        assert_eq!(fixture.read(CLAUDE), "{}");
    }

    #[test]
    fn revert_refuses_when_the_file_no_longer_holds_the_written_value() {
        let fixture = Fixture::new();
        fixture.write(CLAUDE, "{\"autoCompactWindow\": 567000}");
        let receipt = fixture.apply(CLAUDE, 250_000);

        let user_edit = "{\"autoCompactWindow\": 321000}";
        fixture.write(CLAUDE, user_edit);
        let error = revert(&receipt.record, &fixture.paths, &fixture.record_dir).unwrap_err();
        assert!(matches!(error, WriteError::Stale { .. }), "{error}");
        assert_eq!(fixture.read(CLAUDE), user_edit);
        assert!(
            load_record(&fixture.record_dir, CLAUDE).unwrap().is_some(),
            "a refused revert keeps its record"
        );

        fixture.write(CLAUDE, "{}");
        assert!(matches!(
            revert(&receipt.record, &fixture.paths, &fixture.record_dir),
            Err(WriteError::Stale { .. })
        ));
        assert_eq!(fixture.read(CLAUDE), "{}");

        fs::remove_file(fixture.path(CLAUDE)).unwrap();
        assert!(matches!(
            revert(&receipt.record, &fixture.paths, &fixture.record_dir),
            Err(WriteError::Missing { .. })
        ));
    }

    #[test]
    fn revert_twice_reports_stale_instead_of_changing_the_file_again() {
        let fixture = Fixture::new();
        fixture.write(CODEX, "a = 1\n");
        let receipt = fixture.apply(CODEX, 250_000);
        revert(&receipt.record, &fixture.paths, &fixture.record_dir).unwrap();
        assert!(matches!(
            revert(&receipt.record, &fixture.paths, &fixture.record_dir),
            Err(WriteError::Stale { .. })
        ));
        assert_eq!(fixture.read(CODEX), "a = 1\n");
    }

    #[test]
    fn revert_rejects_a_record_that_names_an_unrelated_file() {
        let fixture = Fixture::new();
        let victim = fixture.root.path().join("notes.txt");
        fs::write(&victim, "{\"autoCompactWindow\": 250000}").unwrap();
        let record = ApplyRecord {
            target: CLAUDE,
            path: victim.clone(),
            previous_value: None,
            written_value: 250_000,
            applied_at: "2026-10-05 12:00:00".into(),
        };
        assert!(matches!(
            revert(&record, &fixture.paths, &fixture.record_dir),
            Err(WriteError::ParseFailed { .. })
        ));
        assert_eq!(
            fs::read_to_string(victim).unwrap(),
            "{\"autoCompactWindow\": 250000}"
        );
    }

    #[test]
    fn a_second_apply_records_the_value_it_replaced() {
        let fixture = Fixture::new();
        fixture.write(CLAUDE, "{\"autoCompactWindow\": 567000}");
        fixture.apply(CLAUDE, 300_000);
        let second = fixture.apply(CLAUDE, 200_000);
        assert_eq!(second.record.previous_value, Some(300_000));
        let record = load_record(&fixture.record_dir, CLAUDE).unwrap().unwrap();
        assert_eq!(record.written_value, 200_000);
    }

    #[test]
    fn a_failed_second_apply_keeps_the_first_applys_revert_record() {
        let fixture = Fixture::new();
        fixture.write(CLAUDE, "{\"autoCompactWindow\": 567000}");
        let first = fixture.apply(CLAUDE, 300_000);
        let plan = plan_apply(CLAUDE, &fixture.paths, 200_000).unwrap();
        let target_path = fixture.path(CLAUDE);
        let interfere =
            || fs::write(&target_path, "{\"autoCompactWindow\": 300000, \"x\": 1}").unwrap();
        let error = apply_with(
            &plan,
            &fixture.record_dir,
            Duration::from_secs(1),
            &interfere,
        )
        .unwrap_err();
        assert!(matches!(error, WriteError::Stale { .. }), "{error}");
        assert_eq!(
            load_record(&fixture.record_dir, CLAUDE).unwrap(),
            Some(first.record)
        );
    }

    #[test]
    fn records_round_trip_through_json_and_belong_to_one_target() {
        let fixture = Fixture::new();
        fixture.write(CLAUDE, "{}");
        let receipt = fixture.apply(CLAUDE, 250_000);
        let stored =
            fs::read_to_string(fixture.record_dir.join(CLAUDE.record_file_name())).unwrap();
        assert!(stored.contains("\"target\": \"claude_auto_compact_window\""));
        assert!(stored.contains("\"previous_value\": null"));
        assert_eq!(
            load_record(&fixture.record_dir, CLAUDE).unwrap(),
            Some(receipt.record.clone())
        );
        assert!(load_record(&fixture.record_dir, CODEX).unwrap().is_none());
        // A record file moved under the wrong name is refused.
        fs::copy(
            fixture.record_dir.join(CLAUDE.record_file_name()),
            fixture.record_dir.join(CODEX.record_file_name()),
        )
        .unwrap();
        assert!(matches!(
            load_record(&fixture.record_dir, CODEX),
            Err(WriteError::ParseFailed { .. })
        ));
        clear_record(&fixture.record_dir, CLAUDE).unwrap();
        clear_record(&fixture.record_dir, CLAUDE).unwrap();
        assert!(load_record(&fixture.record_dir, CLAUDE).unwrap().is_none());
    }

    #[cfg(unix)]
    #[test]
    fn apply_writes_through_a_symlink_and_keeps_it_a_symlink() {
        use std::os::unix::fs::symlink;

        let fixture = Fixture::new();
        let real_dir = fixture.root.path().join("dotfiles");
        fs::create_dir(&real_dir).unwrap();
        let real = real_dir.join("settings.json");
        fs::write(&real, "{\"autoCompactWindow\": 567000}\n").unwrap();
        symlink(&real, fixture.path(CLAUDE)).unwrap();

        let plan = plan_apply(CLAUDE, &fixture.paths, 250_000).unwrap();
        assert_eq!(plan.path, fs::canonicalize(&real).unwrap());
        apply(&plan, &fixture.record_dir).unwrap();

        assert!(fs::symlink_metadata(fixture.path(CLAUDE))
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(
            fs::read_to_string(&real).unwrap(),
            "{\"autoCompactWindow\": 250000}\n"
        );
        assert_eq!(fixture.leftover_temporaries(&real_dir), 0);
        assert_eq!(fixture.leftover_temporaries(&fixture.paths.claude_dir), 0);
    }

    #[cfg(unix)]
    #[test]
    fn apply_refuses_when_the_symlink_is_retargeted_after_planning() {
        use std::os::unix::fs::symlink;

        let fixture = Fixture::new();
        let first = fixture.root.path().join("first.json");
        let second = fixture.root.path().join("second.json");
        fs::write(&first, "{\"autoCompactWindow\": 567000}").unwrap();
        fs::write(&second, "{\"autoCompactWindow\": 567000}").unwrap();
        symlink(&first, fixture.path(CLAUDE)).unwrap();
        let plan = plan_apply(CLAUDE, &fixture.paths, 250_000).unwrap();
        fs::remove_file(fixture.path(CLAUDE)).unwrap();
        symlink(&second, fixture.path(CLAUDE)).unwrap();
        // The plan names the canonical `first.json`, which is still unchanged,
        // so the write lands there and `second.json` is never touched.
        apply(&plan, &fixture.record_dir).unwrap();
        assert_eq!(
            fs::read_to_string(&second).unwrap(),
            "{\"autoCompactWindow\": 567000}"
        );
        assert_eq!(
            fs::read_to_string(&first).unwrap(),
            "{\"autoCompactWindow\": 250000}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn apply_and_revert_preserve_file_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let fixture = Fixture::new();
        fixture.write(CODEX, "a = 1\n");
        fixture.write(CLAUDE, "{}");
        for (target, mode) in [(CODEX, 0o640), (CLAUDE, 0o600)] {
            fs::set_permissions(fixture.path(target), fs::Permissions::from_mode(mode)).unwrap();
            let receipt = fixture.apply(target, 250_000);
            let mode_after = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode_after(&fixture.path(target)), mode);
            revert(&receipt.record, &fixture.paths, &fixture.record_dir).unwrap();
            assert_eq!(mode_after(&fixture.path(target)), mode);
        }
    }

    #[test]
    fn lock_contention_is_reported_without_touching_the_file() {
        let fixture = Fixture::new();
        let original = "{\"autoCompactWindow\": 567000}";
        fixture.write(CLAUDE, original);
        let plan = plan_apply(CLAUDE, &fixture.paths, 250_000).unwrap();
        assert!(
            !plan.lock_path.starts_with(&fixture.paths.claude_dir),
            "the lock is a sidecar outside the agent's directory"
        );
        let held = ExclusiveFileLock::acquire(&plan.lock_path).unwrap();
        let error = apply_with(
            &plan,
            &fixture.record_dir,
            Duration::from_millis(80),
            &|| {},
        )
        .unwrap_err();
        assert!(matches!(error, WriteError::Lock { .. }), "{error}");
        assert_eq!(fixture.read(CLAUDE), original);
        drop(held);
        apply(&plan, &fixture.record_dir).unwrap();
        assert_eq!(fixture.read(CLAUDE), "{\"autoCompactWindow\": 250000}");
    }

    #[test]
    fn revert_also_waits_for_the_lock_and_then_gives_up() {
        let fixture = Fixture::new();
        fixture.write(CLAUDE, "{}");
        let receipt = fixture.apply(CLAUDE, 250_000);
        let lock_path = lock_path_for(&fixture.paths, &receipt.record.path);
        let held = ExclusiveFileLock::acquire(&lock_path).unwrap();
        assert!(matches!(
            revert_with(
                &receipt.record,
                &fixture.paths,
                &fixture.record_dir,
                Duration::from_millis(80)
            ),
            Err(WriteError::Lock { .. })
        ));
        drop(held);
        revert(&receipt.record, &fixture.paths, &fixture.record_dir).unwrap();
        assert_eq!(fixture.read(CLAUDE), "{}");
    }

    #[test]
    fn apply_refuses_a_file_that_stopped_being_valid() {
        let fixture = Fixture::new();
        fixture.write(CODEX, "a = 1\n");
        let plan = plan_apply(CODEX, &fixture.paths, 250_000).unwrap();
        fixture.write(CODEX, "a = \n");
        // Stale is reported first because the content no longer matches.
        assert!(matches!(
            apply(&plan, &fixture.record_dir),
            Err(WriteError::Stale { .. })
        ));
        assert_eq!(fixture.read(CODEX), "a = \n");
        assert!(matches!(
            plan_apply(CODEX, &fixture.paths, 250_000),
            Err(WriteError::ParseFailed { .. })
        ));
    }

    #[test]
    fn a_missing_file_is_never_created() {
        let fixture = Fixture::new();
        assert!(matches!(
            plan_apply(CLAUDE, &fixture.paths, 250_000),
            Err(WriteError::Missing { .. })
        ));
        assert!(!fixture.path(CLAUDE).exists());
    }

    #[test]
    fn the_nine_distinct_error_variants_have_distinct_messages() {
        let path = PathBuf::from("/x/settings.json");
        let messages = [
            WriteError::Missing { path: path.clone() }.to_string(),
            WriteError::NotRegular { path: path.clone() }.to_string(),
            WriteError::TooLarge {
                path: path.clone(),
                size: 2,
                limit: 1,
            }
            .to_string(),
            parse_failed(&path, "bad").to_string(),
            WriteError::OutOfRange {
                key: "k",
                value: 1,
                min: 2,
                max: 3,
            }
            .to_string(),
            WriteError::Stale {
                path: path.clone(),
                reason: "r".into(),
            }
            .to_string(),
            io_error("open", &path)(io::Error::other("boom")).to_string(),
            WriteError::Lock {
                path,
                reason: "busy".into(),
            }
            .to_string(),
            WriteError::HomeUnavailable.to_string(),
        ];
        let distinct: std::collections::BTreeSet<_> = messages.iter().collect();
        assert_eq!(distinct.len(), messages.len());
    }
}

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

/// A transcript whose filename, embedded session ID, and project cwd agree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedTranscript {
    pub session_id: String,
    pub path: PathBuf,
}

/// Project-scoped access to the local Claude Code, Codex, and Antigravity
/// transcript stores.
#[derive(Debug, Clone)]
pub struct TranscriptLocator {
    home: PathBuf,
    project_cwd: PathBuf,
}

impl TranscriptLocator {
    /// Builds one locator for a canonical ilium project session.
    pub fn new(home: &Path, project_cwd: &Path) -> Self {
        Self {
            home: home.to_path_buf(),
            project_cwd: canonical_or_original(project_cwd),
        }
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
        if matches.next().is_some() {
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
        if matches!(class, AgentClass::Antigravity) && self.antigravity_history_matches(&session_id)
        {
            return Some(VerifiedTranscript {
                session_id,
                path: path.to_path_buf(),
            });
        }
        if !transcript_metadata_matches(class, path, &session_id, &self.project_cwd) {
            return None;
        }
        Some(VerifiedTranscript {
            session_id,
            path: path.to_path_buf(),
        })
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
        BufReader::new(file)
            .lines()
            .map_while(Result::ok)
            .find_map(|line| {
                let entry = serde_json::from_str::<Value>(&line).ok()?;
                let session_id = entry.get("conversationId").and_then(Value::as_str)?;
                if session_id != expected_session_id {
                    return None;
                }
                let workspace = entry.get("workspace").and_then(Value::as_str)?;
                Some(same_canonical_project(
                    Path::new(workspace),
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
        let Ok(entry) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let identity = match class {
            AgentClass::Claude => claude_identity(&entry),
            AgentClass::Codex => codex_identity(&entry),
            AgentClass::Antigravity => None,
            AgentClass::Other(_) => None,
        };
        let Some((session_id, cwd)) = identity else {
            continue;
        };
        return session_id == expected_session_id
            && same_canonical_project(Path::new(cwd), expected_project_cwd);
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

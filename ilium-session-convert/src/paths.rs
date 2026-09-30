//! Filesystem layout shared by both conversion directions: where Claude Code
//! and Codex keep transcripts, and how a rollout is found and verified.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use ilium_agent_session::TranscriptLocator;
use ilium_core::AgentClass;
use serde_json::Value;

/// Canonical form of a project path, matching what `ilium-agent-session`
/// compares against (and what Claude Code slugifies).
pub(crate) fn canonical_or_original(path: &Path) -> PathBuf {
    ilium_platform::paths::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Claude Code's project directory name: every non `[A-Za-z0-9]` character of
/// the canonical cwd becomes `-`.
///
/// Mirrors the private `slugify_claude_project_path` in `ilium-agent-session`
/// (that crate does not export it). Tests assert that `TranscriptLocator`
/// finds every transcript written under this slug, so drift is caught.
pub(crate) fn claude_project_slug(cwd: &Path) -> String {
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

pub(crate) fn claude_project_dir(home_dir: &Path, project_cwd: &Path) -> PathBuf {
    home_dir
        .join(".claude")
        .join("projects")
        .join(claude_project_slug(project_cwd))
}

/// `codex_home` when given, else `$CODEX_HOME`, else `<home>/.codex`.
pub(crate) fn resolve_codex_home(home_dir: &Path, requested: Option<&Path>) -> PathBuf {
    if let Some(explicit) = requested {
        return explicit.to_path_buf();
    }
    match std::env::var_os("CODEX_HOME") {
        Some(value) if !value.is_empty() => PathBuf::from(value),
        _ => home_dir.join(".codex"),
    }
}

/// First `session_meta` identity of a Codex rollout: `(session id, cwd)`.
pub(crate) fn rollout_identity(path: &Path) -> Option<(String, String)> {
    let file = std::fs::File::open(path).ok()?;
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        let Ok(entry) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if entry.get("type").and_then(Value::as_str) != Some("session_meta") {
            continue;
        }
        let payload = entry.get("payload")?;
        let id = payload.get("id")?.as_str()?.to_string();
        let cwd = payload.get("cwd")?.as_str()?.to_string();
        return Some((id, cwd));
    }
    None
}

/// Every `rollout-*-<session_id>.jsonl` below `<codex_home>/sessions`.
pub(crate) fn rollouts_named_for(codex_home: &Path, session_id: &str) -> Vec<PathBuf> {
    let suffix = format!("-{session_id}.jsonl");
    let mut pending = vec![codex_home.join("sessions")];
    let mut found = Vec::new();
    while let Some(directory) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            if file_type.is_dir() {
                pending.push(path);
                continue;
            }
            let matches = path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("rollout-") && name.ends_with(&suffix));
            if file_type.is_file() && matches {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

/// A Codex rollout whose identity was proven.
#[derive(Debug, Clone)]
pub(crate) struct LocatedRollout {
    pub(crate) path: PathBuf,
    /// The cwd recorded in the rollout's `session_meta`, when readable.
    pub(crate) recorded_cwd: Option<String>,
    /// Whether the recorded cwd is the same project as the request's.
    pub(crate) cwd_matches: bool,
}

/// Finds exactly one rollout for `session_id`. The standard store goes through
/// [`TranscriptLocator`] (which also proves the project cwd); a custom
/// `CODEX_HOME` outside `<home>/.codex` is scanned directly, because the
/// locator only knows the default store.
pub(crate) fn locate_codex_rollout(
    home_dir: &Path,
    project_cwd: &Path,
    codex_home: &Path,
    session_id: &str,
) -> Option<LocatedRollout> {
    let project = canonical_or_original(project_cwd);
    let uses_default_store =
        canonical_or_original(codex_home) == canonical_or_original(&home_dir.join(".codex"));
    if uses_default_store {
        let locator = TranscriptLocator::new(home_dir, &project);
        if let Some(verified) = locator.transcript_for_session(&AgentClass::Codex, session_id) {
            return Some(LocatedRollout {
                recorded_cwd: Some(project.to_string_lossy().into_owned()),
                path: verified.path,
                cwd_matches: true,
            });
        }
    }
    let mut candidates = rollouts_named_for(codex_home, session_id);
    if candidates.len() != 1 {
        return None;
    }
    let path = candidates.remove(0);
    let (id, recorded_cwd) = rollout_identity(&path)?;
    if id != session_id {
        return None;
    }
    let cwd_matches = canonical_or_original(Path::new(&recorded_cwd)) == project;
    Some(LocatedRollout {
        path,
        recorded_cwd: Some(recorded_cwd),
        cwd_matches,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_replaces_every_non_alphanumeric_character() {
        assert_eq!(
            claude_project_slug(Path::new("/home/a.b/c_d e")),
            "-home-a-b-c-d-e"
        );
    }

    #[test]
    fn explicit_codex_home_wins() {
        assert_eq!(
            resolve_codex_home(Path::new("/h"), Some(Path::new("/x/codex"))),
            PathBuf::from("/x/codex")
        );
    }
}

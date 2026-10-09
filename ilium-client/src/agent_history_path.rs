//! Resolves absolute JSONL transcript paths for detected agent sessions.
//!
//! Provider layouts stay out of the terminal menu: Claude's project slug is
//! lossy and Codex's date directory cannot be reconstructed from an ID.
//! `TranscriptLocator` verifies the transcript's embedded identity and launch
//! directory before this module exposes a paste-ready path.

use std::path::{Path, PathBuf};

use ilium_agent_session::TranscriptLocator;
use ilium_core::AgentClass;

/// Finds the verified absolute JSONL transcript for one supported agent session.
///
/// Antigravity currently persists conversations as SQLite databases and custom
/// signatures have no declared transcript contract, so neither can truthfully
/// offer a JSONL history-file path.
pub fn verified_jsonl_history_path(
    home_dir: &Path,
    pane_cwd: &Path,
    agent_class: &AgentClass,
    session_id: &str,
) -> Option<PathBuf> {
    verified_jsonl_history_path_with_hint(home_dir, pane_cwd, agent_class, session_id, None)
}

/// Like [`verified_jsonl_history_path`], but first tries the server-verified
/// `transcript_hint` for this exact session.
///
/// The hint is untrusted input to this function: it is accepted only when the
/// same store and launch-directory checks pass and it names `session_id`.
/// Anything else falls back to the bounded directory scan.
pub fn verified_jsonl_history_path_with_hint(
    home_dir: &Path,
    pane_cwd: &Path,
    agent_class: &AgentClass,
    session_id: &str,
    transcript_hint: Option<&Path>,
) -> Option<PathBuf> {
    if !matches!(agent_class, AgentClass::Claude | AgentClass::Codex) {
        return None;
    }

    // Every caller is an interactive action that waits on this lookup, so it
    // uses the interactive budget. Exhaustion yields `None`, never a partial guess.
    let locator = TranscriptLocator::new_bounded(
        home_dir,
        pane_cwd,
        ilium_agent_session::INTERACTIVE_TRANSCRIPT_LOOKUP_LIMITS,
    );
    resolve_history_path(&locator, agent_class, session_id, transcript_hint)
}

/// Resolves the absolute history path through `locator`, preferring a verified
/// hint. Callers that need to know whether the lookup exhausted its budget keep
/// the locator and check `read_limit_reached()` after this returns.
pub fn resolve_history_path(
    locator: &TranscriptLocator,
    agent_class: &AgentClass,
    session_id: &str,
    transcript_hint: Option<&Path>,
) -> Option<PathBuf> {
    transcript_hint
        .and_then(|hint| locator.transcript_from_path(agent_class, hint))
        .filter(|transcript| transcript.session_id == session_id)
        .or_else(|| locator.transcript_for_session(agent_class, session_id))
        // Clipboard consumers need a path independent of their current directory.
        .map(|transcript| transcript.path)
        .filter(|path| path.is_absolute())
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use ilium_core::AgentClass;

    use super::{verified_jsonl_history_path, verified_jsonl_history_path_with_hint};

    fn write_claude_transcript(home: &Path, project_path: &Path, session_id: &str) -> PathBuf {
        let project_slug: String = project_path
            .to_string_lossy()
            .chars()
            .map(|character| {
                if character.is_ascii_alphanumeric() {
                    character
                } else {
                    '-'
                }
            })
            .collect();
        let directory = home.join(".claude").join("projects").join(project_slug);
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join(format!("{session_id}.jsonl"));
        std::fs::write(
            &path,
            serde_json::json!({
                "type": "user",
                "sessionId": session_id,
                "cwd": project_path,
                "message": {"content": "test prompt"}
            })
            .to_string(),
        )
        .unwrap();
        path
    }

    fn write_codex_transcript(home: &Path, project_path: &Path, session_id: &str) -> PathBuf {
        let directory = home.join(".codex").join("sessions").join("2026/09/05");
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join(format!("rollout-2026-09-05T12-00-00-{session_id}.jsonl"));
        std::fs::write(
            &path,
            serde_json::json!({
                "type": "session_meta",
                "payload": {"id": session_id, "cwd": project_path}
            })
            .to_string(),
        )
        .unwrap();
        path
    }

    #[test]
    fn resolves_verified_absolute_paths_for_claude_and_codex_sessions() {
        let home = tempfile::tempdir().unwrap();
        let project_path = Path::new("/work/history-path-test");
        let claude_session_id = "11111111-1111-4111-8111-111111111111";
        let codex_session_id = "22222222-2222-4222-8222-222222222222";
        let claude_path = write_claude_transcript(home.path(), project_path, claude_session_id);
        let codex_path = write_codex_transcript(home.path(), project_path, codex_session_id);

        assert_eq!(
            verified_jsonl_history_path(
                home.path(),
                project_path,
                &AgentClass::Claude,
                claude_session_id,
            ),
            Some(claude_path)
        );
        assert_eq!(
            verified_jsonl_history_path(
                home.path(),
                project_path,
                &AgentClass::Codex,
                codex_session_id,
            ),
            Some(codex_path)
        );
    }

    #[test]
    fn worktree_history_requires_the_panes_launch_cwd() {
        let home = tempfile::tempdir().unwrap();
        let project_cwd = Path::new("/work/history-path-test");
        let worktree_cwd = Path::new("/work/history-path-test.worktrees/agent-task");
        let session_id = "44444444-4444-4444-8444-444444444444";
        let transcript_path = write_codex_transcript(home.path(), worktree_cwd, session_id);

        assert_eq!(
            verified_jsonl_history_path(home.path(), worktree_cwd, &AgentClass::Codex, session_id),
            Some(transcript_path)
        );
        assert_eq!(
            verified_jsonl_history_path(home.path(), project_cwd, &AgentClass::Codex, session_id),
            None
        );
    }

    /// Regression: a live Codex store exceeded the old 4096-entry menu budget,
    /// so the lookup exhausted and the "Copy history file path" action vanished.
    #[test]
    fn resolves_codex_history_in_a_store_larger_than_the_old_menu_budget() {
        let home = tempfile::tempdir().unwrap();
        let project_path = Path::new("/work/history-path-test");
        let session_id = "55555555-5555-4555-8555-555555555555";
        let target = write_codex_transcript(home.path(), project_path, session_id);
        let sessions = home.path().join(".codex").join("sessions");
        for month in 1..=12 {
            for day in 1..=28 {
                let directory = sessions
                    .join("2025")
                    .join(format!("{month:02}"))
                    .join(format!("{day:02}"));
                std::fs::create_dir_all(&directory).unwrap();
                for index in 0..14 {
                    std::fs::write(directory.join(format!("rollout-{index}.jsonl")), "{}").unwrap();
                }
            }
        }

        assert_eq!(
            verified_jsonl_history_path(home.path(), project_path, &AgentClass::Codex, session_id),
            Some(target)
        );
    }

    #[test]
    fn refuses_providers_without_a_jsonl_history_contract() {
        let home = tempfile::tempdir().unwrap();
        let session_id = "33333333-3333-4333-8333-333333333333";

        assert_eq!(
            verified_jsonl_history_path(
                home.path(),
                Path::new("/work/history-path-test"),
                &AgentClass::Antigravity,
                session_id,
            ),
            None
        );
        assert_eq!(
            verified_jsonl_history_path(
                home.path(),
                Path::new("/work/history-path-test"),
                &AgentClass::Other("aider".to_string()),
                session_id,
            ),
            None
        );
    }

    /// The server's exact-PID hint must answer even when the directory scan has
    /// no budget left, and a scan-only lookup must still fail under that budget.
    #[test]
    fn server_verified_hint_resolves_when_the_directory_scan_budget_is_exhausted() {
        let home = tempfile::tempdir().unwrap();
        let project_path = Path::new("/work/history-path-test");
        let session_id = "66666666-6666-4666-8666-666666666666";
        let target = write_codex_transcript(home.path(), project_path, session_id);
        let exhausted_scan = ilium_agent_session::TranscriptReadLimits {
            scanned_entries: 1,
            ..ilium_agent_session::INTERACTIVE_TRANSCRIPT_LOOKUP_LIMITS
        };

        let scan_only = ilium_agent_session::TranscriptLocator::new_bounded(
            home.path(),
            project_path,
            exhausted_scan,
        );
        assert_eq!(
            super::resolve_history_path(&scan_only, &AgentClass::Codex, session_id, None),
            None
        );

        let hinted = ilium_agent_session::TranscriptLocator::new_bounded(
            home.path(),
            project_path,
            exhausted_scan,
        );
        assert_eq!(
            super::resolve_history_path(&hinted, &AgentClass::Codex, session_id, Some(&target)),
            Some(target)
        );
    }

    #[test]
    fn hint_naming_another_session_is_ignored() {
        let home = tempfile::tempdir().unwrap();
        let project_path = Path::new("/work/history-path-test");
        let requested_id = "77777777-7777-4777-8777-777777777777";
        let other_id = "88888888-8888-4888-8888-888888888888";
        let requested_path = write_codex_transcript(home.path(), project_path, requested_id);
        let other_path = write_codex_transcript(home.path(), project_path, other_id);

        assert_eq!(
            verified_jsonl_history_path_with_hint(
                home.path(),
                project_path,
                &AgentClass::Codex,
                requested_id,
                Some(&other_path),
            ),
            Some(requested_path)
        );
    }

    #[test]
    fn hint_outside_the_expected_store_is_rejected() {
        let home = tempfile::tempdir().unwrap();
        let project_path = Path::new("/work/history-path-test");
        let session_id = "99999999-9999-4999-8999-999999999999";
        let foreign_directory = tempfile::tempdir().unwrap();
        let foreign_path = foreign_directory
            .path()
            .join(format!("rollout-2026-09-05T12-00-00-{session_id}.jsonl"));
        std::fs::write(
            &foreign_path,
            serde_json::json!({
                "type": "session_meta",
                "payload": {"id": session_id, "cwd": project_path}
            })
            .to_string(),
        )
        .unwrap();
        let exhausted_scan = ilium_agent_session::TranscriptReadLimits {
            scanned_entries: 1,
            ..ilium_agent_session::INTERACTIVE_TRANSCRIPT_LOOKUP_LIMITS
        };
        let locator = ilium_agent_session::TranscriptLocator::new_bounded(
            home.path(),
            project_path,
            exhausted_scan,
        );

        assert_eq!(
            super::resolve_history_path(
                &locator,
                &AgentClass::Codex,
                session_id,
                Some(&foreign_path),
            ),
            None
        );
    }
}

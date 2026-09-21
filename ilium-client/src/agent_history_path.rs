//! Resolves absolute JSONL transcript paths for detected agent sessions.
//!
//! Provider layouts stay out of the terminal menu: Claude's project slug is
//! lossy and Codex's date directory cannot be reconstructed from an ID.
//! `TranscriptLocator` verifies the transcript's embedded identity and project
//! before this module exposes a paste-ready path.

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
    project_path: &Path,
    agent_class: &AgentClass,
    session_id: &str,
) -> Option<PathBuf> {
    if !matches!(agent_class, AgentClass::Claude | AgentClass::Codex) {
        return None;
    }

    let path = TranscriptLocator::new(home_dir, project_path)
        .transcript_for_session(agent_class, session_id)?
        .path;

    // Clipboard consumers need a path independent of their current directory.
    path.is_absolute().then_some(path)
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use ilium_core::AgentClass;

    use super::verified_jsonl_history_path;

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
}

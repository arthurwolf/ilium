//! Revalidates server-provided transcript hints on the statistics I/O lane.

use super::StatsRequest;
use ilium_agent_session::{TranscriptLocator, TranscriptReadLimits};
use ilium_core::AgentClass;
use std::path::PathBuf;

pub(super) fn locator(
    request: &StatsRequest,
) -> Result<(TranscriptLocator, Option<PathBuf>), String> {
    let mut locator = TranscriptLocator::new_bounded(
        &request.home,
        &request.project_path,
        TranscriptReadLimits {
            line_bytes: 1024 * 1024,
            total_read_bytes: 16 * 1024 * 1024,
            scanned_entries: 1_000_000,
            retained_path_bytes: 1024 * 1024,
        },
    );
    let Some(path) = &request.transcript_hint else {
        return Ok((locator, None));
    };
    if !path.is_absolute() {
        return Err("Verified transcript hint is not an absolute path.".into());
    }
    // The server establishes process ownership. Here we derive only its store
    // root and repeat the existing session/project metadata validation; a hint
    // never authorizes reading an arbitrary or mismatched JSONL file.
    let directory = match request.class {
        AgentClass::Claude => "projects",
        AgentClass::Codex => "sessions",
        _ => return Err("Statistics transcript hints are unsupported for this provider.".into()),
    };
    let root = path
        .ancestors()
        .find(|ancestor| ancestor.file_name().is_some_and(|name| name == directory))
        .and_then(|ancestor| ancestor.parent())
        .ok_or("Verified transcript hint has an unsupported store layout.")?;
    locator = match request.class {
        AgentClass::Claude => locator.with_claude_config_dir(root),
        AgentClass::Codex => locator.with_codex_home(root),
        _ => return Err("Statistics transcript provider changed.".into()),
    };
    let transcript = locator
        .transcript_from_path(&request.class, path)
        .filter(|transcript| transcript.session_id == request.session_id)
        .ok_or("Verified transcript hint no longer matches this session and project.")?;
    Ok((locator, Some(transcript.path)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weekly_usage_custom_store_hints_require_matching_session_and_project() {
        let home = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let session_id = "fed1b0ad-8afe-4ee2-9555-112233445566";
        for class in [AgentClass::Claude, AgentClass::Codex] {
            let store = home.path().join("custom-provider");
            let path = if class == AgentClass::Claude {
                let slug: String = project
                    .path()
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
                store
                    .join("projects")
                    .join(slug)
                    .join(format!("{session_id}.jsonl"))
            } else {
                store
                    .join("sessions/2026/11/16")
                    .join(format!("rollout-2026-11-16T00-00-00-{session_id}.jsonl"))
            };
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            let record = if class == AgentClass::Claude {
                serde_json::json!({"type": "user", "sessionId": session_id, "cwd": project.path()})
            } else {
                serde_json::json!({"type": "session_meta", "payload": {"id": session_id, "cwd": project.path()}})
            };
            std::fs::write(&path, format!("{record}\n")).unwrap();
            let request = StatsRequest {
                transcript_hint: Some(path.clone()),
                class,
                session_id: session_id.into(),
                project_path: project.path().to_path_buf(),
                home: home.path().to_path_buf(),
            };
            assert_eq!(locator(&request).unwrap().1, Some(path.clone()));
            assert!(locator(&StatsRequest {
                session_id: "other-session".into(),
                ..request.clone()
            })
            .is_err());
            assert!(locator(&StatsRequest {
                project_path: home.path().to_path_buf(),
                ..request.clone()
            })
            .is_err());
            assert!(locator(&StatsRequest {
                transcript_hint: Some("relative.jsonl".into()),
                ..request.clone()
            })
            .is_err());
            std::fs::remove_file(&path).unwrap();
            assert!(locator(&request).is_err());
        }
    }
}

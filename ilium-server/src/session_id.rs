//! Discovers the active supported-agent session ID for one detected
//! agent process. Every accepted result is tied to the exact agent class,
//! canonical ilium project, and an on-disk transcript whose embedded metadata
//! agrees with its filename. Uncertain directory/screen guesses deliberately
//! return no ID.
//!
//! Admissible evidence, in order:
//!
//! 1. A verified transcript held open by this exact agent PID.
//! 2. Explicit CLI identity (`claude --resume/--session-id`, `codex resume`,
//!    `agy --conversation`)
//!    whose transcript independently verifies the project.
//!
//! The former environment rank was removed because environment variables are
//! inherited across projects. The former screen, content-correlation, and
//! sole-new-transcript ranks were removed because none can prove which process
//! owns an ID. When neither admissible source proves ownership, the answer is
//! deliberately no ID.

use std::collections::HashSet;
use std::path::Path;

use ilium_agent_session::TranscriptLocator;
use ilium_core::{AgentClass, AgentProvider};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

/// Auditable evidence that produced a session ID. Kept server-internal because
/// the client only needs the verified identity, not the discovery mechanics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoverySource {
    GeneratedAtLaunch,
    Arguments,
    OpenFile,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredSession {
    pub session_id: String,
    pub source: DiscoverySource,
}

/// Refreshes only fields used by discovery for the detected agent PIDs.
pub fn refresh_for_discovery(system: &mut System, pids: &[Pid]) {
    if pids.is_empty() {
        return;
    }
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(pids),
        false,
        ProcessRefreshKind::nothing()
            .with_cmd(UpdateKind::Always)
            .with_cwd(UpdateKind::Always),
    );
}

/// Resolves one project-verified session ID, excluding every ID already owned
/// by another pane. Startup arguments are ignored after an in-process session
/// transition command because they still describe the session used at launch.
pub fn discover(
    system: &System,
    pid: Pid,
    class: &AgentClass,
    locator: &TranscriptLocator,
    project_cwd: &Path,
    ignore_startup_arguments: bool,
    excluded_session_ids: &HashSet<String>,
) -> Option<DiscoveredSession> {
    class.provider()?;
    let process = system.process(pid)?;
    if !same_canonical_path(process.cwd()?, project_cwd) {
        return None;
    }
    let arguments: Vec<String> = process
        .cmd()
        .iter()
        .map(|argument| argument.to_string_lossy().into_owned())
        .collect();

    // A current descriptor owned by the exact detected PID is stronger than
    // immutable launch arguments: an in-process resume can legitimately make
    // those arguments stale even if input tracking missed the transition.
    if let Some(session_id) = from_open_files(pid.as_u32(), class, locator) {
        if !excluded_session_ids.contains(&session_id) {
            return Some(DiscoveredSession {
                session_id,
                source: DiscoverySource::OpenFile,
            });
        }
    }

    if !ignore_startup_arguments {
        if let Some(session_id) = from_arguments(class, &arguments) {
            if verified_and_unclaimed(locator, class, &session_id, excluded_session_ids) {
                return Some(DiscoveredSession {
                    session_id,
                    source: DiscoverySource::Arguments,
                });
            }
        }
    }

    None
}

fn verified_and_unclaimed(
    locator: &TranscriptLocator,
    class: &AgentClass,
    session_id: &str,
    excluded_session_ids: &HashSet<String>,
) -> bool {
    !excluded_session_ids.contains(session_id)
        && locator.transcript_for_session(class, session_id).is_some()
}

/// Delegates exact CLI syntax to the detected provider. This keeps a new
/// provider's resume grammar beside its command and launch metadata rather
/// than growing a parallel parser in the server.
fn from_arguments(class: &AgentClass, arguments: &[String]) -> Option<String> {
    class.provider()?.session_id_from_arguments(arguments)
}

#[cfg(target_os = "linux")]
fn from_open_files(pid: u32, class: &AgentClass, locator: &TranscriptLocator) -> Option<String> {
    let entries = std::fs::read_dir(format!("/proc/{pid}/fd")).ok()?;
    let session_ids: HashSet<String> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| std::fs::read_link(entry.path()).ok())
        .filter_map(|target| locator.transcript_from_path(class, &target))
        .map(|transcript| transcript.session_id)
        .collect();
    exactly_one(session_ids)
}

#[cfg(not(target_os = "linux"))]
fn from_open_files(_pid: u32, _class: &AgentClass, _locator: &TranscriptLocator) -> Option<String> {
    None
}

fn exactly_one(values: HashSet<String>) -> Option<String> {
    (values.len() == 1)
        .then(|| values.into_iter().next())
        .flatten()
}

fn same_canonical_path(left: &Path, right: &Path) -> bool {
    let left = std::fs::canonicalize(left).unwrap_or_else(|_| left.to_path_buf());
    let right = std::fs::canonicalize(right).unwrap_or_else(|_| right.to_path_buf());
    left == right
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_claude_transcript(home: &Path, cwd: &Path, session_id: &str) {
        let slug: String = cwd
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
        let directory = home.join(".claude/projects").join(slug);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join(format!("{session_id}.jsonl")),
            serde_json::json!({
                "type": "user",
                "sessionId": session_id,
                "cwd": cwd,
                "message": {"content": "test"},
            })
            .to_string(),
        )
        .unwrap();
    }

    #[test]
    fn arguments_are_class_specific_and_uuid_only() {
        let session_id = "95fd0645-3331-408b-a7e5-36e6007bfb78";
        assert_eq!(
            from_arguments(
                &AgentClass::Claude,
                &["claude".into(), "--session-id".into(), session_id.into()]
            ),
            Some(session_id.to_string())
        );
        assert_eq!(
            from_arguments(
                &AgentClass::Codex,
                &["codex".into(), "resume".into(), session_id.into()]
            ),
            Some(session_id.to_string())
        );
        assert_eq!(
            from_arguments(
                &AgentClass::Codex,
                &["codex".into(), "--thread".into(), session_id.into()]
            ),
            None
        );
        assert_eq!(
            from_arguments(
                &AgentClass::Codex,
                &[
                    "codex".into(),
                    "prompt".into(),
                    "resume".into(),
                    session_id.into()
                ]
            ),
            None
        );
        assert_eq!(
            from_arguments(
                &AgentClass::Claude,
                &["claude".into(), "--resume".into(), "named-session".into()]
            ),
            None
        );
        assert_eq!(
            from_arguments(
                &AgentClass::Claude,
                &[
                    "claude".into(),
                    "--".into(),
                    "--resume".into(),
                    session_id.into()
                ]
            ),
            None,
            "prompt arguments after the option terminator are never identity evidence"
        );
    }

    #[test]
    fn argument_rank_declines_conflicting_ids() {
        assert_eq!(
            from_arguments(
                &AgentClass::Claude,
                &[
                    "claude".into(),
                    "--resume".into(),
                    "11111111-1111-4111-8111-111111111111".into(),
                    "--session-id".into(),
                    "22222222-2222-4222-8222-222222222222".into(),
                ]
            ),
            None
        );
        assert_eq!(
            from_arguments(
                &AgentClass::Claude,
                &[
                    "claude".into(),
                    "--resume".into(),
                    "11111111-1111-4111-8111-111111111111".into(),
                    "--fork-session".into(),
                ]
            ),
            None
        );
    }

    #[test]
    fn a_session_claimed_by_another_pane_is_not_admissible() {
        let home = tempfile::tempdir().unwrap();
        let cwd = home.path().join("project");
        std::fs::create_dir_all(&cwd).unwrap();
        let session_id = "55555555-5555-4555-8555-555555555555";
        write_claude_transcript(home.path(), &cwd, session_id);
        let locator = TranscriptLocator::new(home.path(), &cwd);

        assert!(verified_and_unclaimed(
            &locator,
            &AgentClass::Claude,
            session_id,
            &HashSet::new()
        ));
        assert!(!verified_and_unclaimed(
            &locator,
            &AgentClass::Claude,
            session_id,
            &HashSet::from([session_id.to_string()])
        ));
    }
}

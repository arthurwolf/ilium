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

/// One explicit discovery phase and its result, retained for the per-agent
/// debug history rather than disappearing behind a final `Option`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionDiscoveryPhase {
    pub phase: &'static str,
    pub outcome: &'static str,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionDiscoveryAttempt {
    pub discovered: Option<DiscoveredSession>,
    pub phases: Vec<SessionDiscoveryPhase>,
}

/// Verified identities visible through the exact process's open transcript
/// descriptors. Keeping the complete candidate set makes ambiguity and stale
/// descriptor retention observable instead of collapsing both to `None`.
struct OpenFileDiscovery {
    discovered_session_id: Option<String>,
    verified_session_ids: Vec<String>,
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

/// Resolves one project-verified session ID while retaining enough structured
/// evidence to explain every accepted, skipped, and rejected phase in the
/// agent debug view. Startup arguments are ignored after an in-process session
/// transition because they still describe the identity used at launch.
pub fn discover_with_trace(
    system: &System,
    pid: Pid,
    class: &AgentClass,
    locator: &TranscriptLocator,
    project_cwd: &Path,
    ignore_startup_arguments: bool,
    excluded_session_ids: &HashSet<String>,
) -> SessionDiscoveryAttempt {
    let mut phases = Vec::new();
    if class.provider().is_none() {
        phases.push(SessionDiscoveryPhase {
            phase: "provider contract",
            outcome: "unsupported",
            detail: format!("{class:?} has no verified transcript ownership adapter"),
        });
        return SessionDiscoveryAttempt {
            discovered: None,
            phases,
        };
    }
    phases.push(SessionDiscoveryPhase {
        phase: "provider contract",
        outcome: "accepted",
        detail: format!("using the built-in {class:?} provider contract"),
    });

    let Some(process) = system.process(pid) else {
        phases.push(SessionDiscoveryPhase {
            phase: "process lookup",
            outcome: "missing",
            detail: format!("PID {} was absent after targeted refresh", pid.as_u32()),
        });
        return SessionDiscoveryAttempt {
            discovered: None,
            phases,
        };
    };
    phases.push(SessionDiscoveryPhase {
        phase: "process lookup",
        outcome: "accepted",
        detail: format!("found exact agent PID {}", pid.as_u32()),
    });

    // `sysinfo` does not report a working directory on every platform, and
    // where it does not, session discovery would stop here even though the
    // agent has already been identified -- which is exactly what happened on
    // macOS: the pane showed `Agent(Claude, Idle)` while its session id never
    // resolved. `ilium_platform` asks the OS directly as a fallback.
    let process_cwd = process
        .cwd()
        .map(std::path::Path::to_path_buf)
        .or_else(|| ilium_platform::process_info::working_directory(pid.as_u32()));
    let Some(process_cwd) = process_cwd else {
        phases.push(SessionDiscoveryPhase {
            phase: "project ownership",
            outcome: "missing",
            detail: "process cwd was unavailable".to_string(),
        });
        return SessionDiscoveryAttempt {
            discovered: None,
            phases,
        };
    };
    let process_cwd = process_cwd.as_path();
    if !same_canonical_path(process_cwd, project_cwd) {
        phases.push(SessionDiscoveryPhase {
            phase: "project ownership",
            outcome: "rejected",
            detail: format!(
                "process cwd {} does not match project {}",
                process_cwd.display(),
                project_cwd.display()
            ),
        });
        return SessionDiscoveryAttempt {
            discovered: None,
            phases,
        };
    }
    phases.push(SessionDiscoveryPhase {
        phase: "project ownership",
        outcome: "accepted",
        detail: format!("canonical cwd matches {}", project_cwd.display()),
    });
    // Through `ilium_detect`, which expands a shell wrapper's single
    // command-line argument into the tokens the program itself sees. macOS
    // keeps `sh -c "<command line>"` intact, so scanning raw arguments for
    // `--resume` or `--session-id` finds nothing there.
    let arguments = ilium_detect::effective_arguments(process);

    // A current descriptor owned by the exact detected PID is stronger than
    // immutable launch arguments: an in-process resume can legitimately make
    // those arguments stale even if input tracking missed the transition.
    let excluded_session_id_list = sorted_session_ids(excluded_session_ids.iter().cloned());
    let open_file_discovery = from_open_files(pid.as_u32(), class, locator, excluded_session_ids);
    if let Some(session_id) = open_file_discovery
        .as_ref()
        .and_then(|discovery| discovery.discovered_session_id.as_ref())
    {
        phases.push(SessionDiscoveryPhase {
            phase: "open transcript descriptors",
            outcome: "resolved",
            detail: format!(
                "exact PID owns verified transcript {session_id}; verified descriptor identities: {}; excluded identities: {}",
                display_session_ids(
                    &open_file_discovery
                        .as_ref()
                        .map_or_else(Vec::new, |discovery| discovery.verified_session_ids.clone())
                ),
                display_session_ids(&excluded_session_id_list),
            ),
        });
        return SessionDiscoveryAttempt {
            discovered: Some(DiscoveredSession {
                session_id: session_id.clone(),
                source: DiscoverySource::OpenFile,
            }),
            phases,
        };
    }
    phases.push(SessionDiscoveryPhase {
        phase: "open transcript descriptors",
        outcome: "unresolved",
        detail: open_file_discovery.map_or_else(
            || format!(
                "the exact PID descriptor directory was unavailable; excluded identities: {}",
                display_session_ids(&excluded_session_id_list),
            ),
            |discovery| format!(
                "no single admissible verified transcript; verified descriptor identities: {}; excluded identities: {}",
                display_session_ids(&discovery.verified_session_ids),
                display_session_ids(&excluded_session_id_list),
            ),
        ),
    });

    if ignore_startup_arguments {
        phases.push(SessionDiscoveryPhase {
            phase: "startup arguments",
            outcome: "skipped",
            detail: "an in-process session transition made launch arguments stale".to_string(),
        });
    } else if let Some(session_id) = from_arguments(class, &arguments) {
        if verified_and_unclaimed(locator, class, &session_id, excluded_session_ids) {
            phases.push(SessionDiscoveryPhase {
                phase: "startup arguments",
                outcome: "resolved",
                detail: format!("verified explicit session argument {session_id}"),
            });
            return SessionDiscoveryAttempt {
                discovered: Some(DiscoveredSession {
                    session_id,
                    source: DiscoverySource::Arguments,
                }),
                phases,
            };
        }
        phases.push(SessionDiscoveryPhase {
            phase: "startup arguments",
            outcome: "rejected",
            detail: format!(
                "candidate {session_id} was excluded or had no project-verified transcript"
            ),
        });
    } else {
        phases.push(SessionDiscoveryPhase {
            phase: "startup arguments",
            outcome: "unresolved",
            detail: "provider arguments contained no explicit session identity".to_string(),
        });
    }

    SessionDiscoveryAttempt {
        discovered: None,
        phases,
    }
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

/// Identifies the session by which transcript file the agent process currently
/// holds open.
///
/// Returns `None` where the platform cannot enumerate another process's open
/// files at all, which is a different answer from "it has none open": the
/// caller must fall back to other evidence rather than concluding the agent
/// has no session. `ilium_platform::process_info` documents which platforms
/// can answer.
fn from_open_files(
    pid: u32,
    class: &AgentClass,
    locator: &TranscriptLocator,
    excluded_session_ids: &HashSet<String>,
) -> Option<OpenFileDiscovery> {
    if !ilium_platform::process_info::open_files_are_observable() {
        return None;
    }
    let verified_session_ids = sorted_session_ids(
        ilium_platform::process_info::open_file_paths(pid)
            .into_iter()
            .filter_map(|target| {
                locator
                    .transcript_from_path(class, &target)
                    .map(|transcript| transcript.session_id)
            }),
    );
    let discovered_session_id =
        uniquely_discovered_session_id(verified_session_ids.iter().cloned(), excluded_session_ids);
    Some(OpenFileDiscovery {
        discovered_session_id,
        verified_session_ids,
    })
}

fn sorted_session_ids(session_ids: impl Iterator<Item = String>) -> Vec<String> {
    let mut session_ids: Vec<String> = session_ids.collect();
    session_ids.sort();
    session_ids.dedup();
    session_ids
}

fn display_session_ids(session_ids: &[String]) -> String {
    if session_ids.is_empty() {
        "none".to_string()
    } else {
        session_ids.join(", ")
    }
}

/// Discards identities already proven inadmissible before checking ownership.
/// Duplicate descriptors for one remaining transcript are accepted, while two
/// distinct admissible identities still make exact-PID ownership ambiguous.
fn uniquely_discovered_session_id(
    session_ids: impl Iterator<Item = String>,
    excluded_session_ids: &HashSet<String>,
) -> Option<String> {
    let mut discovered_session_id = None;
    for session_id in session_ids {
        if excluded_session_ids.contains(&session_id) {
            continue;
        }
        match &discovered_session_id {
            Some(existing) if existing != &session_id => return None,
            Some(_) => {}
            None => discovered_session_id = Some(session_id),
        }
    }
    discovered_session_id
}

fn same_canonical_path(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }
    let left = std::fs::canonicalize(left).unwrap_or_else(|_| left.to_path_buf());
    let right = std::fs::canonicalize(right).unwrap_or_else(|_| right.to_path_buf());
    left == right
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_file_ownership_stops_at_the_first_conflicting_session() {
        let session_ids = ["same", "same", "different"]
            .into_iter()
            .map(str::to_string)
            .chain(std::iter::once_with(|| {
                panic!("iterator must not be consumed after ambiguity is proven")
            }));
        assert_eq!(
            uniquely_discovered_session_id(session_ids, &HashSet::new()),
            None
        );
        assert_eq!(
            uniquely_discovered_session_id(
                ["same", "same"].into_iter().map(str::to_string),
                &HashSet::new(),
            ),
            Some("same".to_string())
        );
    }

    #[test]
    fn excluded_open_file_identity_is_removed_before_ambiguity_check() {
        let session_ids = ["old", "new", "old", "new"].into_iter().map(str::to_string);
        let excluded_session_ids = HashSet::from(["old".to_string()]);

        assert_eq!(
            uniquely_discovered_session_id(session_ids, &excluded_session_ids),
            Some("new".to_string())
        );
    }

    fn write_claude_transcript(home: &Path, cwd: &Path, session_id: &str) {
        // Resolved first, because `TranscriptLocator` resolves before it
        // slugifies. macOS reaches a temp directory through a `/var` ->
        // `/private/var` symlink and Windows hands out 8.3 short names, so an
        // unresolved slug names a directory the locator never looks in.
        let resolved =
            ilium_platform::paths::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
        let slug: String = resolved
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
        let directory = home.join(".claude").join("projects").join(slug);
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

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
use std::path::PathBuf;

use ilium_agent_session::{
    MetadataParseFailure, StagedMetadataStep, TranscriptLocator, VerifiedTranscript,
};
use ilium_core::{AgentClass, AgentProvider};
use ilium_execution::{JobCost, Lane};
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
    /// Transcript path the server itself verified for `session_id`. Only the
    /// exact-PID descriptor source provides one; the client re-verifies it.
    pub transcript_path: Option<PathBuf>,
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
    /// The one verified transcript path for `discovered_session_id`. Absent
    /// when that identity appears at more than one path, which is ambiguous.
    discovered_transcript_path: Option<PathBuf>,
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
            .without_tasks()
            .with_cmd(UpdateKind::Always)
            .with_cwd(UpdateKind::Always),
    );
}

pub(crate) const TRANSCRIPT_READ_LIMITS: ilium_agent_session::TranscriptReadLimits =
    ilium_agent_session::TranscriptReadLimits {
        line_bytes: 1024 * 1024,
        total_read_bytes: 16 * 1024 * 1024,
        scanned_entries: 4096,
        retained_path_bytes: 4 * 1024 * 1024,
    };

/// The immutable process facts needed after the process-table lock is released.
/// Capture this on the admitted I/O owner, including the platform cwd fallback.
#[derive(Debug)]
pub struct ProcessDiscoverySnapshot {
    pub pid: u32,
    pub process_cwd: Option<PathBuf>,
    pub project_matches: bool,
    pub arguments: Vec<String>,
}

pub fn capture_process_discovery(
    system: &System,
    pid: Pid,
    project_cwd: &Path,
) -> Option<ProcessDiscoverySnapshot> {
    let process = system.process(pid)?;
    let process_cwd = process
        .cwd()
        .map(Path::to_path_buf)
        .or_else(|| ilium_platform::process_info::working_directory(pid.as_u32()));
    let project_matches = process_cwd
        .as_deref()
        .is_some_and(|cwd| same_canonical_path(cwd, project_cwd));
    Some(ProcessDiscoverySnapshot {
        pid: pid.as_u32(),
        process_cwd,
        project_matches,
        arguments: ilium_detect::effective_arguments(process),
    })
}

/// Recheck process project ownership after staged waits. An unreadable cwd is
/// unknown and cannot grant a new session claim.
pub(crate) fn current_process_project_matches(pid: u32, project_cwd: &Path) -> bool {
    ilium_platform::process_info::working_directory(pid)
        .as_deref()
        .is_some_and(|cwd| same_canonical_path(cwd, project_cwd))
}

fn staged_error(error: impl std::fmt::Display) -> crate::error::ServerError {
    std::io::Error::other(format!("staged transcript metadata: {error}")).into()
}

fn parser_failure(
    error: &crate::execution::ExecutionError<std::io::Error>,
) -> MetadataParseFailure {
    match error {
        crate::execution::ExecutionError::Rejected(_) => MetadataParseFailure::AdmissionRefused,
        crate::execution::ExecutionError::Cancelled => MetadataParseFailure::Cancelled,
        crate::execution::ExecutionError::Failed(error)
            if error.view().kind() == std::io::ErrorKind::Interrupted =>
        {
            MetadataParseFailure::Cancelled
        }
        crate::execution::ExecutionError::Failed(_)
        | crate::execution::ExecutionError::Panicked
        | crate::execution::ExecutionError::Lost => MetadataParseFailure::WorkerFailed,
    }
}

/// Verify one candidate by alternating admitted I/O reads and CPU JSON decode.
/// No execution callback waits for another job. Each result moves its cursor,
/// raw line and retention into the next job; one bounded line is live at once.
pub async fn verify_path_staged(
    execution: &crate::execution::ExecutionClient,
    locator: &TranscriptLocator,
    class: &AgentClass,
    path: &Path,
) -> Result<Option<VerifiedTranscript>, crate::error::ServerError> {
    if !locator.claim_staged_jobs(1) {
        return Err(staged_error(
            "metadata job limit reached before opening transcript",
        ));
    }
    let source = locator.clone();
    let class_for_open = class.clone();
    let path = path.to_path_buf();
    let begun = execution
        .run(
            Lane::Io,
            JobCost {
                input_bytes: 2 * 1024 * 1024,
                result_bytes: 2 * 1024 * 1024,
            },
            move |context: ilium_execution::JobContext| -> Result<_, std::io::Error> {
                if context.stop_requested() {
                    return Err(std::io::Error::from(std::io::ErrorKind::Interrupted));
                }
                Ok(source.begin_staged_metadata(&class_for_open, &path))
            },
        )
        .await
        .map_err(staged_error)?;
    let (cursor, retention) = begun.into_parts();
    let Some(cursor) = cursor else {
        return Ok(None);
    };
    let mut pending = retention.retain((Box::new(cursor), None));
    let mut prepaid_io = false;
    loop {
        if !prepaid_io && !locator.claim_staged_jobs(1) {
            return Err(staged_error(
                "metadata job limit reached before reading transcript",
            ));
        }
        let (input, previous_retention) = pending.into_parts();
        let next = execution
            .run(
                Lane::Io,
                JobCost {
                    input_bytes: 2 * 1024 * 1024,
                    result_bytes: 2 * 1024 * 1024,
                },
                move |context: ilium_execution::JobContext| -> Result<_, std::io::Error> {
                    let _previous_retention = previous_retention;
                    if context.stop_requested() {
                        return Err(std::io::Error::from(std::io::ErrorKind::Interrupted));
                    }
                    Ok(input.0.advance(input.1))
                },
            )
            .await
            .map_err(staged_error)?;
        match next.view() {
            StagedMetadataStep::Finished(transcript) => return Ok(transcript.clone()),
            StagedMetadataStep::Batch { .. } => {}
        }
        let (step, previous_retention) = next.into_parts();
        let StagedMetadataStep::Batch { cursor, lines } = step else {
            unreachable!("verified staged step changed after inspection")
        };
        // Preclaim CPU decode and the next I/O verification together. Even at
        // the cap, a first authoritative record can finish on its I/O owner.
        if !locator.claim_staged_jobs(2) {
            return Err(staged_error("metadata job limit reached before CPU decode"));
        }
        pending = match execution
            .run(
                Lane::Cpu,
                // The 1 MiB line limit also bounds the transient serde_json
                // tree. The larger input declaration covers its peak nodes.
                JobCost {
                    input_bytes: 32 * 1024 * 1024,
                    result_bytes: 2 * 1024 * 1024,
                },
                move |context: ilium_execution::JobContext| -> Result<_, std::io::Error> {
                    let _previous_retention = previous_retention;
                    if context.stop_requested() {
                        return Err(std::io::Error::from(std::io::ErrorKind::Interrupted));
                    }
                    let parsed = cursor.parse_batch(&lines);
                    Ok((cursor, Some(parsed)))
                },
            )
            .await
        {
            Ok(parsed) => parsed,
            Err(error) => {
                locator.mark_metadata_parse_failure(parser_failure(&error));
                return Err(staged_error(error));
            }
        };
        prepaid_io = true;
    }
}

/// Exactly-one verified path, including Codex's bounded date-directory scan.
/// The same locator is reused for generated, descriptor and argument ranks.
pub async fn verify_session_staged(
    execution: &crate::execution::ExecutionClient,
    locator: &TranscriptLocator,
    class: &AgentClass,
    session_id: &str,
) -> Result<Option<VerifiedTranscript>, crate::error::ServerError> {
    if !locator.claim_staged_jobs(1) {
        return Err(staged_error(
            "metadata job limit reached before candidate scan",
        ));
    }
    let source = locator.clone();
    let class_for_scan = class.clone();
    let session_id = session_id.to_string();
    let paths = execution
        .run(
            Lane::Io,
            JobCost {
                input_bytes: 2 * 1024 * 1024,
                result_bytes: 8 * 1024 * 1024,
            },
            move |context: ilium_execution::JobContext| -> Result<_, std::io::Error> {
                if context.stop_requested() {
                    return Err(std::io::Error::from(std::io::ErrorKind::Interrupted));
                }
                Ok(source.staged_candidate_paths_for_session(&class_for_scan, &session_id))
            },
        )
        .await
        .map_err(staged_error)?;
    let mut verified = None;
    for path in paths.view() {
        if let Some(transcript) = verify_path_staged(execution, locator, class, path).await? {
            if verified.is_some() {
                return Ok(None);
            }
            verified = Some(transcript);
        }
    }
    if locator.read_limit_reached() {
        return Ok(None);
    }
    Ok(verified)
}

async fn open_files_staged(
    execution: &crate::execution::ExecutionClient,
    pid: u32,
    class: &AgentClass,
    locator: &TranscriptLocator,
    excluded_session_ids: &HashSet<String>,
) -> Result<Option<OpenFileDiscovery>, crate::error::ServerError> {
    if !locator.claim_staged_jobs(1) {
        return Err(staged_error(
            "metadata job limit reached before descriptor scan",
        ));
    }
    let limits = locator.read_limits().unwrap_or(TRANSCRIPT_READ_LIMITS);
    let paths = execution
        .run(
            Lane::Io,
            JobCost {
                input_bytes: 2 * 1024 * 1024,
                result_bytes: 8 * 1024 * 1024,
            },
            move |context: ilium_execution::JobContext| -> Result<_, std::io::Error> {
                if context.stop_requested() {
                    return Err(std::io::Error::from(std::io::ErrorKind::Interrupted));
                }
                if !ilium_platform::process_info::open_files_are_observable() {
                    return Ok((None, false));
                }
                match ilium_platform::process_info::open_file_paths_bounded(
                    pid,
                    ilium_platform::process_info::OpenFilePathLimits {
                        descriptors: limits.scanned_entries,
                        retained_bytes: limits.retained_path_bytes,
                    },
                ) {
                    Ok(paths) => Ok((Some(paths), false)),
                    Err(error) => Ok((None, error.kind() == std::io::ErrorKind::OutOfMemory)),
                }
            },
        )
        .await
        .map_err(staged_error)?;
    let (paths, overflow) = paths.view();
    if *overflow {
        locator.mark_read_limit_reached();
    }
    let Some(paths) = paths else { return Ok(None) };
    let mut verified_transcripts = Vec::new();
    for path in paths {
        if let Some(transcript) = verify_path_staged(execution, locator, class, path).await? {
            verified_transcripts.push((transcript.session_id, transcript.path));
        }
    }
    let verified_session_ids = sorted_session_ids(
        verified_transcripts
            .iter()
            .map(|(session_id, _)| session_id.clone()),
    );
    let discovered_session_id =
        uniquely_discovered_session_id(verified_session_ids.iter().cloned(), excluded_session_ids);
    let discovered_transcript_path = discovered_session_id
        .as_ref()
        .and_then(|session_id| unique_transcript_path(&verified_transcripts, session_id));
    Ok(Some(OpenFileDiscovery {
        discovered_session_id,
        discovered_transcript_path,
        verified_session_ids,
    }))
}

#[allow(clippy::too_many_arguments)] // Preserve explicit process, transcript and exclusion evidence inputs.
pub async fn discover_with_trace_staged(
    execution: &crate::execution::ExecutionClient,
    snapshot: Option<&ProcessDiscoverySnapshot>,
    pid: u32,
    class: &AgentClass,
    locator: &TranscriptLocator,
    project_cwd: &Path,
    ignore_startup_arguments: bool,
    excluded_session_ids: &HashSet<String>,
) -> Result<SessionDiscoveryAttempt, crate::error::ServerError> {
    let mut phases = Vec::new();
    if class.provider().is_none() {
        phases.push(SessionDiscoveryPhase {
            phase: "provider contract",
            outcome: "unsupported",
            detail: format!("{class:?} has no verified transcript ownership adapter"),
        });
        return Ok(SessionDiscoveryAttempt {
            discovered: None,
            phases,
        });
    }
    phases.push(SessionDiscoveryPhase {
        phase: "provider contract",
        outcome: "accepted",
        detail: format!("using the built-in {class:?} provider contract"),
    });
    let Some(snapshot) = snapshot.filter(|snapshot| snapshot.pid == pid) else {
        phases.push(SessionDiscoveryPhase {
            phase: "process lookup",
            outcome: "missing",
            detail: format!("PID {pid} was absent after targeted refresh"),
        });
        return Ok(SessionDiscoveryAttempt {
            discovered: None,
            phases,
        });
    };
    phases.push(SessionDiscoveryPhase {
        phase: "process lookup",
        outcome: "accepted",
        detail: format!("found exact agent PID {pid}"),
    });
    let Some(process_cwd) = &snapshot.process_cwd else {
        phases.push(SessionDiscoveryPhase {
            phase: "project ownership",
            outcome: "missing",
            detail: "process cwd was unavailable".to_string(),
        });
        return Ok(SessionDiscoveryAttempt {
            discovered: None,
            phases,
        });
    };
    if !snapshot.project_matches {
        phases.push(SessionDiscoveryPhase {
            phase: "project ownership",
            outcome: "rejected",
            detail: format!(
                "process cwd {} does not match project {}",
                process_cwd.display(),
                project_cwd.display()
            ),
        });
        return Ok(SessionDiscoveryAttempt {
            discovered: None,
            phases,
        });
    }
    phases.push(SessionDiscoveryPhase {
        phase: "project ownership",
        outcome: "accepted",
        detail: format!("canonical cwd matches {}", project_cwd.display()),
    });

    let excluded_list = sorted_session_ids(excluded_session_ids.iter().cloned());
    let open_file_discovery =
        open_files_staged(execution, pid, class, locator, excluded_session_ids).await?;
    let discovered = if let Some(open_file) = open_file_discovery
        .as_ref()
        .filter(|open_file| open_file.discovered_session_id.is_some())
    {
        let session_id = open_file
            .discovered_session_id
            .as_ref()
            .expect("filtered above");
        phases.push(SessionDiscoveryPhase {
            phase: "open transcript descriptors", outcome: "resolved",
            detail: format!(
                "exact PID owns verified transcript {session_id}; verified descriptor identities: {}; excluded identities: {}",
                display_session_ids(&open_file.verified_session_ids), display_session_ids(&excluded_list),
            ),
        });
        Some(DiscoveredSession {
            session_id: session_id.clone(),
            source: DiscoverySource::OpenFile,
            transcript_path: open_file.discovered_transcript_path.clone(),
        })
    } else {
        phases.push(SessionDiscoveryPhase {
            phase: "open transcript descriptors", outcome: "unresolved",
            detail: open_file_discovery.map_or_else(
                || format!("the exact PID descriptor directory was unavailable; excluded identities: {}", display_session_ids(&excluded_list)),
                |discovery| format!("no single admissible verified transcript; verified descriptor identities: {}; excluded identities: {}",
                    display_session_ids(&discovery.verified_session_ids), display_session_ids(&excluded_list)),
            ),
        });
        if ignore_startup_arguments {
            phases.push(SessionDiscoveryPhase {
                phase: "startup arguments",
                outcome: "skipped",
                detail: "an in-process session transition made launch arguments stale".to_string(),
            });
            None
        } else if let Some(session_id) = from_arguments(class, &snapshot.arguments) {
            if !excluded_session_ids.contains(&session_id)
                && verify_session_staged(execution, locator, class, &session_id)
                    .await?
                    .is_some()
            {
                phases.push(SessionDiscoveryPhase {
                    phase: "startup arguments",
                    outcome: "resolved",
                    detail: format!("verified explicit session argument {session_id}"),
                });
                Some(DiscoveredSession {
                    session_id,
                    source: DiscoverySource::Arguments,
                    transcript_path: None,
                })
            } else {
                phases.push(SessionDiscoveryPhase {
                    phase: "startup arguments",
                    outcome: "rejected",
                    detail: format!(
                        "candidate {session_id} was excluded or had no project-verified transcript"
                    ),
                });
                None
            }
        } else {
            phases.push(SessionDiscoveryPhase {
                phase: "startup arguments",
                outcome: "unresolved",
                detail: "provider arguments contained no explicit session identity".to_string(),
            });
            None
        }
    };
    let mut attempt = SessionDiscoveryAttempt { discovered, phases };
    if locator.read_limit_reached() {
        attempt.discovered = None;
        attempt.phases.push(SessionDiscoveryPhase {
            phase: "evidence admission", outcome: "unresolved",
            detail: "transcript read/scan resource limit reached; partial evidence cannot claim a session".into(),
        });
    }
    Ok(attempt)
}

/// Bounded evidence variant. A partial filesystem scan never proves exclusive
/// ownership, even if an earlier rank appeared to find one admissible ID.
#[cfg(test)]
#[expect(
    dead_code,
    reason = "retained as an internal test entry point for structured session discovery evidence"
)]
pub fn discover_with_trace_bounded(
    system: &System,
    pid: Pid,
    class: &AgentClass,
    locator: &TranscriptLocator,
    project_cwd: &Path,
    ignore_startup_arguments: bool,
    excluded_session_ids: &HashSet<String>,
) -> SessionDiscoveryAttempt {
    let locator = if locator.read_limits().is_some() {
        locator.clone()
    } else {
        locator.with_read_limits(TRANSCRIPT_READ_LIMITS)
    };
    let mut attempt = discover_with_trace(
        system,
        pid,
        class,
        &locator,
        project_cwd,
        ignore_startup_arguments,
        excluded_session_ids,
    );
    if locator.read_limit_reached() {
        attempt.discovered = None;
        attempt.phases.push(SessionDiscoveryPhase { phase: "evidence admission", outcome: "unresolved", detail: "transcript read/scan resource limit reached; partial evidence cannot claim a session".into() });
    }
    attempt
}

/// Resolves one project-verified session ID while retaining enough structured
/// evidence to explain every accepted, skipped, and rejected phase in the
/// agent debug view. Startup arguments are ignored after an in-process session
/// transition because they still describe the identity used at launch.
#[cfg(test)]
#[expect(
    dead_code,
    reason = "retained as an internal test entry point for structured session discovery evidence"
)]
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
                transcript_path: open_file_discovery
                    .as_ref()
                    .and_then(|discovery| discovery.discovered_transcript_path.clone()),
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
                    transcript_path: None,
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

#[cfg(test)]
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
#[cfg(test)]
#[expect(
    dead_code,
    reason = "retained for test-only open-transcript evidence inspection"
)]
fn from_open_files(
    pid: u32,
    class: &AgentClass,
    locator: &TranscriptLocator,
    excluded_session_ids: &HashSet<String>,
) -> Option<OpenFileDiscovery> {
    if !ilium_platform::process_info::open_files_are_observable() {
        return None;
    }
    let paths = match locator.read_limits() {
        Some(limits) => match ilium_platform::process_info::open_file_paths_bounded(
            pid,
            ilium_platform::process_info::OpenFilePathLimits {
                descriptors: limits.scanned_entries,
                retained_bytes: limits.retained_path_bytes,
            },
        ) {
            Ok(paths) => paths,
            Err(error) => {
                if error.kind() == std::io::ErrorKind::OutOfMemory {
                    locator.mark_read_limit_reached();
                }
                return None;
            }
        },
        None => ilium_platform::process_info::open_file_paths(pid),
    };
    let verified_transcripts: Vec<(String, PathBuf)> = paths
        .into_iter()
        .filter_map(|target| {
            locator
                .transcript_from_path(class, &target)
                .map(|transcript| (transcript.session_id, transcript.path))
        })
        .collect();
    let verified_session_ids = sorted_session_ids(
        verified_transcripts
            .iter()
            .map(|(session_id, _)| session_id.clone()),
    );
    let discovered_session_id =
        uniquely_discovered_session_id(verified_session_ids.iter().cloned(), excluded_session_ids);
    let discovered_transcript_path = discovered_session_id
        .as_ref()
        .and_then(|session_id| unique_transcript_path(&verified_transcripts, session_id));
    Some(OpenFileDiscovery {
        discovered_session_id,
        discovered_transcript_path,
        verified_session_ids,
    })
}

/// Returns the path only when every verified descriptor for `session_id` names
/// the same file. Two paths for one identity are an ambiguous store, so no hint.
fn unique_transcript_path(
    verified_transcripts: &[(String, PathBuf)],
    session_id: &str,
) -> Option<PathBuf> {
    let mut candidate_paths = verified_transcripts
        .iter()
        .filter(|(verified_session_id, _)| verified_session_id == session_id)
        .map(|(_, path)| path);
    let first_path = candidate_paths.next()?;
    candidate_paths
        .all(|path| path == first_path)
        .then(|| first_path.clone())
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

    #[tokio::test]
    async fn staged_verification_uses_io_and_cpu_lanes() {
        let home = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let session_id = "95fd0645-3331-408b-a7e5-36e6007bfb78";
        write_claude_transcript(home.path(), project.path(), session_id);
        let locator =
            TranscriptLocator::new_bounded(home.path(), project.path(), TRANSCRIPT_READ_LIMITS);
        let owner = crate::execution::ServerExecution::start().expect("execution bank");
        let monitor = owner.test_monitor();
        let path = locator
            .staged_candidate_paths_for_session(&AgentClass::Claude, session_id)
            .pop()
            .unwrap();
        let authoritative = std::fs::read(&path).unwrap();
        let mut content = b"{}\n".to_vec();
        content.extend(authoritative);
        std::fs::write(path, content).unwrap();
        let before = monitor.health();

        let verified =
            verify_session_staged(&owner.client, &locator, &AgentClass::Claude, session_id)
                .await
                .unwrap();

        assert_eq!(
            verified.map(|transcript| transcript.session_id),
            Some(session_id.into())
        );
        assert!(!locator.read_limit_reached());
        let after = monitor.health();
        // Execution health preserves the canonical [CPU, I/O, service] lane order.
        assert!(
            after.lanes[0].succeeded > before.lanes[0].succeeded,
            "staged transcript JSON must be decoded on the CPU lane"
        );
        assert!(
            after.lanes[1].succeeded > before.lanes[1].succeeded,
            "staged transcript discovery and reads must use the I/O lane"
        );
        owner.request_shutdown();
    }

    #[tokio::test]
    async fn staged_tiny_lines_use_bounded_jobs_and_overload_fails_before_submission() {
        let home = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let session_id = "95fd0645-3331-408b-a7e5-36e6007bfb78";
        write_claude_transcript(home.path(), project.path(), session_id);
        let locator =
            TranscriptLocator::new_bounded(home.path(), project.path(), TRANSCRIPT_READ_LIMITS);
        let path = locator
            .staged_candidate_paths_for_session(&AgentClass::Claude, session_id)
            .pop()
            .unwrap();
        let authoritative = std::fs::read(&path).unwrap();
        let mut content = b"{}\n".repeat(8_192);
        content.extend(authoritative);
        std::fs::write(&path, content).unwrap();
        let owner = crate::execution::ServerExecution::start().unwrap();
        let monitor = owner.test_monitor();
        let enqueued = || {
            monitor
                .health()
                .lanes
                .iter()
                .map(|lane| lane.enqueued)
                .sum::<usize>()
        };
        let before = enqueued();
        let verified = verify_path_staged(&owner.client, &locator, &AgentClass::Claude, &path)
            .await
            .unwrap();
        assert_eq!(verified.unwrap().session_id, session_id);
        assert!(
            enqueued() - before <= 70,
            "batched decode must not schedule per-line jobs"
        );
        let overloaded =
            TranscriptLocator::new_bounded(home.path(), project.path(), TRANSCRIPT_READ_LIMITS);
        assert!(overloaded.claim_staged_jobs(ilium_agent_session::STAGED_METADATA_JOB_LIMIT));
        let before = enqueued();
        assert!(
            verify_path_staged(&owner.client, &overloaded, &AgentClass::Claude, &path)
                .await
                .is_err()
        );
        assert_eq!(
            enqueued(),
            before,
            "exhausted attempts must not submit jobs"
        );
        assert!(overloaded.read_limit_reached());
        owner.request_shutdown();
    }

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

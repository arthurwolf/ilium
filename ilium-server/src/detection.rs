//! The adaptive agent-detection loop: the timing/scheduling/backoff logic
//! that calls into `ilium-detect`'s pure `identify_agent`/`classify_activity`
//! functions for every pty-backed pane (see README "Poll cadence" and
//! `ilium-detect`'s module docs -- that crate deliberately owns none of
//! this loop itself).
//!
//! One task per session (not one task per pane): a single `sysinfo::System`
//! refresh serves every pane due on a given tick (see
//! `ilium_detect::refresh`'s doc comment on why refreshing once per tick
//! is the intended usage), and a single task is trivially one
//! `JoinHandle` to track and cancel at session shutdown, rather than a
//! fleet of per-pane tasks whose lifecycle would need to be individually
//! wired to pane creation/removal.

use std::time::{Duration, Instant};

use ilium_agent_debug::{
    AgentDebugContext, AgentDebugEventDraft, AgentDebugEventKind, AgentDebugField,
    AgentDebugSeverity, AgentDebugSource,
};
use ilium_agent_session::TranscriptLocator;
use ilium_core::{AgentActivity, NodeId, PaneStatus};
use ilium_ipc::ServerEvent;
use sysinfo::{Pid, System};
use tokio::task::JoinHandle;

use crate::notifications::{self, PendingNotification};
use crate::pane::{ConfirmedGoalOwner, PaneResource};
use crate::sounds::{self, PlaybackRequest};
use crate::state::ServerState;

/// Minimum time between two "force an immediate recheck" requests actually
/// taking effect for the same pane. A focus transition (entering/exiting a
/// pane) or an Enter keypress each *ask* for an immediate recheck (see
/// [`force_check`]), but coalescing repeated asks within this window keeps
/// rapid focus-flicking or Enter-mashing from pinning a pane's
/// classification to run every single base tick regardless of its
/// configured poll tier.
const FORCE_CHECK_DEBOUNCE: Duration = Duration::from_secs(5);
/// Focused panes retain the previous one-second classification cadence, while
/// the scheduler itself now sleeps to exact deadlines instead of polling.
const FOCUSED_POLL_INTERVAL: Duration = Duration::from_secs(1);

/// Spawns the detection loop as a single tracked task and returns its
/// handle. The loop runs until aborted (session shutdown) -- it has no
/// other exit condition, matching the lifetime of the session itself.
pub fn spawn(state: std::sync::Arc<ServerState>) -> JoinHandle<()> {
    tokio::spawn(run_loop(state))
}

/// Niceness applied to the thread running a detection tick's `sysinfo`
/// refresh (see `run_loop`'s `spawn_blocking` call). Deliberately an
/// *absolute* value applied via `setpriority`, not a delta applied via
/// `nice` -- `spawn_blocking` closures run on a small pool of threads tokio
/// reuses across many calls, so a relative adjustment would silently
/// compound (thread niceness creeping up every tick) rather than settling
/// at a fixed, always-correct value.
#[cfg(target_os = "linux")]
const DETECTION_THREAD_NICENESS: i32 = 10;

/// Lowers this thread's scheduling niceness so `ilium_detect::refresh`'s
/// unavoidable `/proc` scan never competes on equal footing with
/// keystroke-path work when *other* processes on the machine are loading
/// the CPU. `setpriority(PRIO_PROCESS, 0, _)` with a pid of 0 only ever
/// targets the calling thread (see `man 2 setpriority`) -- never another
/// thread or process -- so this always succeeds under normal user
/// permissions; only *lowering* niceness numerically (raising scheduling
/// priority) needs `CAP_SYS_NICE`. Linux-only: niceness is a Linux/POSIX
/// scheduling concept with no equivalent this crate targets elsewhere.
/// Failure is logged and otherwise ignored -- a tick must still run at
/// default priority rather than not run at all.
#[cfg(target_os = "linux")]
fn lower_current_thread_niceness() {
    // SAFETY: `setpriority` takes no pointers; `PRIO_PROCESS` + pid `0`
    // restricts its effect to the calling thread alone (see doc comment
    // above), which is always permitted regardless of caller privilege.
    let result = unsafe { libc::setpriority(libc::PRIO_PROCESS, 0, DETECTION_THREAD_NICENESS) };
    // `setpriority` only returns -1 on genuine failure here: the target
    // value (10) is not itself a valid "already there, returned -1 by
    // coincidence" case, so there's no ambiguity to disambiguate via errno.
    if result == -1 {
        let error = std::io::Error::last_os_error();
        tracing::warn!(
            "detection loop: failed to set thread niceness to {DETECTION_THREAD_NICENESS}: \
             {error}; continuing at default scheduling priority"
        );
    }
}

/// Non-Linux no-op: no scheduling-niceness equivalent is wired up on other
/// targets, so a detection tick simply runs at default thread priority.
#[cfg(not(target_os = "linux"))]
fn lower_current_thread_niceness() {}

async fn run_loop(state: std::sync::Arc<ServerState>) {
    let mut system = System::new();

    loop {
        // Sleep to the exact nearest pane deadline. New panes and debounced
        // force-checks notify this loop, so an earlier deadline interrupts
        // the sleep immediately without a fixed polling granularity.
        match next_detection_delay(&state, Instant::now()).await {
            Some(delay) if !delay.is_zero() => {
                tokio::select! {
                    () = tokio::time::sleep(delay) => {}
                    () = state.detection_schedule_changed.notified() => continue,
                }
            }
            Some(_) => {}
            None => {
                state.detection_schedule_changed.notified().await;
                continue;
            }
        }

        // The refresh is the syscall-heavy part (`/proc` reads for every
        // process on the machine); running it on a blocking thread keeps
        // this tick from stalling the tokio runtime's async tasks (other
        // panes' IO, other connections) while it happens. `identify_agent`
        // itself, called below, is pure in-memory iteration over the
        // already-refreshed snapshot, so it does not need the same
        // treatment.
        let refreshed = tokio::task::spawn_blocking(move || {
            // Lower this thread's scheduling niceness before paying the
            // `/proc` scan cost below -- under heavy machine-wide load from
            // *other* processes, this keeps the scan from competing on
            // equal footing with keystroke-path work for CPU time. Only
            // ever adjusts the calling thread's own niceness (never another
            // thread's), so this never needs elevated privileges.
            lower_current_thread_niceness();
            ilium_detect::refresh(&mut system);
            system
        })
        .await;
        match refreshed {
            Ok(refreshed_system) => system = refreshed_system,
            Err(join_error) => {
                // The blocking task panicked, taking the `System` it owned
                // with it -- there is no way to recover that value, so
                // this tick reinitializes with a fresh (empty until the
                // next successful refresh) one rather than leaving `system`
                // uninitialized for the next loop iteration. Logged and
                // continued rather than taking the whole detection loop
                // (and with it every pane's status updates) down over one
                // bad refresh.
                //
                // Deliberately `continue`s rather than falling through to
                // `run_due_panes` below: classifying every due pane against
                // this empty snapshot would find no process tree for any of
                // them, misreporting every currently-detected agent pane as
                // `PlainShell` for a tick (a real, broadcast status
                // regression, not a no-op) instead of simply deferring
                // classification to the next tick once a real refresh
                // succeeds.
                tracing::error!("detection loop: sysinfo refresh task panicked: {join_error}");
                system = System::new();
                continue;
            }
        }

        if let Err(error) = run_due_panes(&state, &mut system).await {
            tracing::error!("detection loop: tick failed: {error}");
        }
    }
}

/// Returns the exact wait until the nearest terminal detection deadline.
/// `None` means no terminal panes exist, so the loop can park on its notify.
async fn next_detection_delay(state: &ServerState, now: Instant) -> Option<Duration> {
    let panes = state.panes.read().await;
    minimum_detection_delay(
        panes.values().filter_map(|resource| match resource {
            PaneResource::Terminal(runtime) => Some(runtime.detection_schedule.next_due),
            PaneResource::Editor { .. } => None,
        }),
        now,
    )
}

/// Selects the nearest deadline without imposing any polling quantum.
fn minimum_detection_delay(
    deadlines: impl Iterator<Item = Instant>,
    now: Instant,
) -> Option<Duration> {
    deadlines
        .map(|deadline| deadline.saturating_duration_since(now))
        .min()
}

/// Separates uniquely owned IDs from legacy/corrupt duplicate claims. A
/// duplicate has no defensible owner, so every claimant must be invalidated
/// instead of preserving whichever pane happened to appear first in a hash
/// map's iteration order.
fn partition_session_claims(
    claims: impl IntoIterator<Item = (String, NodeId)>,
) -> (
    std::collections::HashMap<String, NodeId>,
    std::collections::HashSet<String>,
) {
    let mut unique_claims = std::collections::HashMap::new();
    let mut ambiguous_session_ids = std::collections::HashSet::new();
    for (session_id, pane_id) in claims {
        if unique_claims.insert(session_id.clone(), pane_id).is_some() {
            ambiguous_session_ids.insert(session_id);
        }
    }
    for session_id in &ambiguous_session_ids {
        unique_claims.remove(session_id);
    }
    (unique_claims, ambiguous_session_ids)
}

/// Checks and (if due) reschedules every terminal pane, updating the tree
/// and broadcasting a `PaneStatusChanged` event for any pane whose status
/// changed. A single pane's classification failure never stops the others
/// from being checked (see the per-pane `catch_unwind`-free error
/// handling below -- there is nothing fallible left once
/// `identify_agent`/`classify_activity` are called, both are pure and
/// infallible, so this loop's only real failure mode is a lock/task
/// issue, not a per-pane one; the structure is still one pane at a time so
/// a future fallible step here stays isolated per pane).
///
/// A `Working -> Done` transition (see
/// `notifications::is_finished_transition`) queues a desktop notification,
/// but the notification itself is only sent after the `tree`/`panes` locks
/// below are dropped -- a slow or unavailable notification daemon must
/// never hold up an attached client's tree access.
///
/// Structured in three phases so the expensive part -- classification --
/// never runs while `tree`/`panes` are write-locked. Every attached
/// client's keystrokes (`ipc::handlers::handle_key_input`) need that same
/// `tree` + `panes` write-lock pair for every pane on every keystroke; if
/// classification (an `identify_agent_with_extra` process-tree walk per
/// due pane, potentially over many due panes at once) ran inside that
/// lock, it would stall every keystroke to every pane in the session for
/// the whole tick:
///
/// 1. Snapshot the (cheap, read-only) per-pane inputs classification
///    needs -- `shell_pid` and a `screen_text` dump -- under a brief
///    `panes` *read* lock, for panes whose `next_due` has passed.
/// 2. Classify every snapshotted pane against `system` with no lock held
///    at all (`identify_agent_with_extra`/`classify_activity` are pure
///    given their inputs).
/// 3. Take `tree`+`panes` *write* locks once, briefly, only to apply the
///    already-computed results (tree status update, schedule/tracker
///    updates, notification queuing, broadcast) -- the part that actually
///    needs mutable access.
async fn run_due_panes(
    state: &ServerState,
    system: &mut System,
) -> Result<(), crate::error::ServerError> {
    let now = Instant::now();

    /// One due pane's classification inputs, snapshotted under a brief
    /// `panes` read lock (phase 1) so phase 2's actual classification can
    /// run with no lock held at all.
    struct DuePane {
        pane_id: NodeId,
        shell_pid: Option<u32>,
        screen_generation: u64,
        request_generation: u64,
        confirmed_goal_owner: Option<ConfirmedGoalOwner>,
        session_id: Option<String>,
        session_agent_class: Option<ilium_core::AgentClass>,
        is_session_identity_invalidated: bool,
        invalidated_session_id: Option<String>,
        session_process_id: Option<u32>,
        pending_generated_session_id: Option<String>,
    }

    // Phase 1: snapshot inputs under a read lock only. Also collects every
    // terminal pane's *already-known* session ID (not just due panes' --
    // an idle pane not due this tick still needs to keep excluding its
    // claimed transcript from another due pane's admissible candidates), the
    // starting point for `claimed_session_ids` phase 2 mutates as it
    // resolves each due pane in turn.
    let (due_panes, mut claimed_session_ids, ambiguous_session_ids): (
        Vec<DuePane>,
        std::collections::HashMap<String, NodeId>,
        std::collections::HashSet<String>,
    ) = {
        let panes = state.panes.read().await;
        let (claimed_session_ids, ambiguous_session_ids) =
            partition_session_claims(panes.iter().filter_map(|(pane_id, resource)| {
                match resource {
                    PaneResource::Terminal(runtime) if !runtime.is_session_identity_invalidated => {
                        runtime
                            .session_id
                            .as_ref()
                            .map(|session_id| (session_id.clone(), *pane_id))
                    }
                    PaneResource::Terminal(_) | PaneResource::Editor { .. } => None,
                }
            }));
        let due_panes = panes
            .iter()
            .filter_map(|(pane_id, resource)| {
                let PaneResource::Terminal(runtime) = resource else {
                    return None;
                };
                if runtime.detection_schedule.next_due > now {
                    return None;
                }
                Some(DuePane {
                    pane_id: *pane_id,
                    shell_pid: runtime.session.process_id(),
                    screen_generation: runtime.session.screen_generation(),
                    request_generation: runtime.detection_schedule.request_generation,
                    confirmed_goal_owner: runtime.confirmed_goal_owner.clone(),
                    session_id: runtime.session_id.clone(),
                    session_agent_class: runtime.session_agent_class.clone(),
                    is_session_identity_invalidated: runtime.is_session_identity_invalidated,
                    invalidated_session_id: runtime.invalidated_session_id.clone(),
                    session_process_id: runtime.session_process_id,
                    pending_generated_session_id: runtime.pending_generated_session_id.clone(),
                })
            })
            .collect();
        (due_panes, claimed_session_ids, ambiguous_session_ids)
    };

    if due_panes.is_empty() {
        return Ok(());
    }

    // Phase 2a: identify process trees with no lock held. Screen contents
    // are not captured until identity succeeds, so ordinary shell panes do
    // not allocate a full vt100 text snapshot on every slow-tier check.
    let children_index = ilium_detect::ProcessChildrenIndex::build(system);
    struct IdentifiedPane {
        due: DuePane,
        identity: Option<ilium_detect::AgentIdentity>,
    }
    let identified_panes: Vec<IdentifiedPane> = due_panes
        .into_iter()
        .map(|due| {
            let identity = due.shell_pid.and_then(|shell_pid| {
                ilium_detect::identify_agent_with_extra(
                    system,
                    Pid::from_u32(shell_pid),
                    &children_index,
                    &state.custom_signatures,
                )
            });
            IdentifiedPane { due, identity }
        })
        .collect();

    let identified_ids: std::collections::HashSet<NodeId> = identified_panes
        .iter()
        .filter(|pane| pane.identity.is_some())
        .map(|pane| pane.due.pane_id)
        .collect();
    let screen_snapshots: std::collections::HashMap<NodeId, ilium_pty::ScreenSnapshot> = {
        let panes = state.panes.read().await;
        panes
            .iter()
            .filter_map(|(pane_id, resource)| {
                if !identified_ids.contains(pane_id) {
                    return None;
                }
                match resource {
                    PaneResource::Terminal(runtime) => {
                        Some((*pane_id, runtime.session.screen_snapshot()))
                    }
                    PaneResource::Editor { .. } => None,
                }
            })
            .collect()
    };

    struct ClassifiedPane {
        pane_id: NodeId,
        status: PaneStatus,
        identity: Option<ilium_detect::AgentIdentity>,
        screen_generation: u64,
        request_generation: u64,
        confirmed_goal_owner: Option<ConfirmedGoalOwner>,
        is_fresh_agent_screen: bool,
        is_session_identity_invalidated: bool,
        invalidated_session_id: Option<String>,
        session_process_id: Option<u32>,
        pending_generated_session_id: Option<String>,
        needs_session_discovery: bool,
        shell_pid: Option<u32>,
        activity_evidence: Option<ilium_detect::ActivityEvidence>,
        activity_evidence_line: Option<String>,
        goal_evidence: Option<ilium_detect::GoalEvidence>,
        goal_evidence_line: Option<String>,
        goal_was_retained: bool,
    }

    let classifications: Vec<ClassifiedPane> = identified_panes
        .into_iter()
        .map(|identified| {
            let due_pane = identified.due;
            let identity = identified.identity;
            let screen_snapshot = screen_snapshots
                .get(&due_pane.pane_id)
                .cloned()
                .unwrap_or_else(|| ilium_pty::ScreenSnapshot {
                    generation: due_pane.screen_generation,
                    text: String::new(),
                });
            let classified_identity = classify_identity(
                identity.as_ref(),
                &screen_snapshot.text,
                due_pane.confirmed_goal_owner.as_ref(),
            );
            let is_fresh_agent_screen = identity.as_ref().is_some_and(|identity| {
                ilium_detect::is_fresh_agent_screen(&identity.class, &screen_snapshot.text)
            });
            let has_stable_session_owner = identity.as_ref().is_some_and(|identity| {
                session_owner_is_stable(
                    due_pane.session_id.as_deref(),
                    due_pane.session_agent_class.as_ref(),
                    due_pane.session_process_id,
                    due_pane.is_session_identity_invalidated,
                    identity,
                    &ambiguous_session_ids,
                )
            });
            let needs_session_discovery = identity.is_some() && !has_stable_session_owner;
            ClassifiedPane {
                pane_id: due_pane.pane_id,
                status: classified_identity.status,
                identity,
                screen_generation: screen_snapshot.generation,
                request_generation: due_pane.request_generation,
                confirmed_goal_owner: classified_identity.confirmed_goal_owner,
                is_fresh_agent_screen,
                is_session_identity_invalidated: due_pane.is_session_identity_invalidated,
                invalidated_session_id: due_pane.invalidated_session_id,
                session_process_id: due_pane.session_process_id,
                pending_generated_session_id: due_pane.pending_generated_session_id,
                needs_session_discovery,
                shell_pid: due_pane.shell_pid,
                activity_evidence: classified_identity.activity_evidence,
                activity_evidence_line: classified_identity.activity_evidence_line,
                goal_evidence: classified_identity.goal_evidence,
                goal_evidence_line: classified_identity.goal_evidence_line,
                goal_was_retained: classified_identity.goal_was_retained,
            }
        })
        .collect();

    // Refresh command/cwd fields only for identified process IDs. Discovery
    // itself accepts only built-in provider classes; custom signatures do
    // not have a transcript format with a project-verifiable ownership
    // contract, so they intentionally receive no session ID.
    let discovery_pids: Vec<Pid> = classifications
        .iter()
        .filter(|pane| pane.needs_session_discovery)
        .filter_map(|pane| pane.identity.as_ref())
        .map(|identity| Pid::from_u32(identity.pid))
        .collect();
    crate::session_id::refresh_for_discovery(system, &discovery_pids);
    // Sequential (not a one-shot `filter_map`/`collect`) so `claimed_session_ids`
    // accumulates *within* this same tick: once pane A resolves to session
    // S, pane B -- classified later in this same due-batch -- must never
    // also resolve to S. See `crate::session_id`'s module docs on why that
    // invariant is what actually fixes the same-project-directory
    // misattribution, independent of which tier finds the answer.
    let mut discovered_session_ids: std::collections::HashMap<NodeId, String> =
        std::collections::HashMap::new();
    let mut session_discovery_traces: std::collections::HashMap<
        NodeId,
        Vec<crate::session_id::SessionDiscoveryPhase>,
    > = std::collections::HashMap::new();
    let mut session_discovery_exclusions: std::collections::HashMap<NodeId, Vec<String>> =
        std::collections::HashMap::new();
    let pane_project_cwds: std::collections::HashMap<NodeId, std::path::PathBuf> = {
        let tree = state.tree.read().await;
        classifications
            .iter()
            .filter(|pane| pane.needs_session_discovery)
            .filter_map(|pane| {
                tree.project_path_for(pane.pane_id)
                    .map(|path| (pane.pane_id, path.to_path_buf()))
            })
            .collect()
    };
    for pane in &classifications {
        if !pane.needs_session_discovery {
            continue;
        }
        let Some(identity) = pane.identity.as_ref() else {
            continue;
        };
        let mut exclusion_details: Vec<String> = claimed_session_ids
            .iter()
            .filter(|(_, owner)| **owner != pane.pane_id)
            .map(|(session_id, owner)| format!("{session_id} (already owned by pane {})", owner.0))
            .collect();
        exclusion_details.extend(
            ambiguous_session_ids
                .iter()
                .map(|session_id| format!("{session_id} (claimed by multiple panes)")),
        );
        let mut excluded_session_ids: std::collections::HashSet<String> = claimed_session_ids
            .iter()
            .filter(|(_, owner)| **owner != pane.pane_id)
            .map(|(session_id, _)| session_id.clone())
            .collect();
        let project_cwd = pane_project_cwds
            .get(&pane.pane_id)
            .unwrap_or(&state.session_cwd);
        let transcript_locator = TranscriptLocator::new(&state.home_dir, project_cwd);
        excluded_session_ids.extend(ambiguous_session_ids.iter().cloned());
        // `/resume` can leave the old transcript descriptor open until the
        // CLI finishes switching. For the same process, that old ID is known
        // stale even though the open-file evidence would otherwise be exact.
        if pane.is_session_identity_invalidated
            && pane
                .session_process_id
                .is_some_and(|owner_pid| owner_pid == identity.pid)
        {
            excluded_session_ids.extend(pane.invalidated_session_id.iter().cloned());
            exclusion_details.extend(pane.invalidated_session_id.iter().map(|session_id| {
                format!(
                    "{session_id} (invalidated for the still-running process {})",
                    identity.pid
                )
            }));
        }
        exclusion_details.sort();
        exclusion_details.dedup();
        session_discovery_exclusions.insert(pane.pane_id, exclusion_details);
        let generated_candidate = pane
            .pending_generated_session_id
            .as_ref()
            .filter(|session_id| {
                identity.class == ilium_core::AgentClass::Claude
                    && !excluded_session_ids.contains(*session_id)
                    // Supplying `--session-id` proves what ilium requested;
                    // transcript metadata proves the launched CLI accepted it
                    // for this canonical project. Until then, no ID is safer.
                    && transcript_locator
                        .transcript_for_session(&identity.class, session_id)
                        .is_some()
            })
            .map(|session_id| crate::session_id::DiscoveredSession {
                session_id: session_id.clone(),
                source: crate::session_id::DiscoverySource::GeneratedAtLaunch,
            });
        let (discovered_session, mut discovery_phases) =
            if let Some(generated_session) = generated_candidate {
                (
                    Some(generated_session.clone()),
                    vec![crate::session_id::SessionDiscoveryPhase {
                        phase: "generated launch identity",
                        outcome: "resolved",
                        detail: format!(
                            "ilium-supplied Claude session {} has a project-verified transcript",
                            generated_session.session_id
                        ),
                    }],
                )
            } else {
                let generated_detail = match &pane.pending_generated_session_id {
                    None => "no ilium-generated launch identity was pending".to_string(),
                    Some(session_id) if identity.class != ilium_core::AgentClass::Claude => {
                        format!("pending identity {session_id} belongs to a non-Claude process")
                    }
                    Some(session_id) if excluded_session_ids.contains(session_id) => {
                        format!("pending identity {session_id} is already claimed or invalidated")
                    }
                    Some(session_id) => format!(
                        "pending identity {session_id} has no verified project transcript yet"
                    ),
                };
                let attempt = crate::session_id::discover_with_trace(
                    system,
                    Pid::from_u32(identity.pid),
                    &identity.class,
                    &transcript_locator,
                    project_cwd,
                    pane.is_session_identity_invalidated
                        && pane
                            .session_process_id
                            .is_none_or(|owner_pid| owner_pid == identity.pid),
                    &excluded_session_ids,
                );
                let mut phases = vec![crate::session_id::SessionDiscoveryPhase {
                    phase: "generated launch identity",
                    outcome: "unresolved",
                    detail: generated_detail,
                }];
                phases.extend(attempt.phases);
                (attempt.discovered, phases)
            };
        discovery_phases.push(crate::session_id::SessionDiscoveryPhase {
            phase: "overall result",
            outcome: if discovered_session.is_some() {
                "resolved"
            } else {
                "unresolved"
            },
            detail: discovered_session.as_ref().map_or_else(
                || "no admissible ownership evidence resolved a session".to_string(),
                |session| format!("selected {} from {:?}", session.session_id, session.source),
            ),
        });
        session_discovery_traces.insert(pane.pane_id, discovery_phases);
        let Some(discovered_session) = discovered_session else {
            continue;
        };
        let session_id = discovered_session.session_id;
        tracing::debug!(
            pane_id = ?pane.pane_id,
            session_id,
            source = ?discovered_session.source,
            "resolved project-verified agent session"
        );
        claimed_session_ids.insert(session_id.clone(), pane.pane_id);
        discovered_session_ids.insert(pane.pane_id, session_id);
    }

    // Phase 3: brief write-locked critical section applying results.
    let sound_settings = state.sound_settings.read().await.clone();
    let mut pending_notifications = Vec::new();
    let mut pending_sounds = Vec::new();
    let mut completed_pane_ids = Vec::new();
    let mut pending_title_clears = Vec::new();
    let mut tree_snapshot_changed = false;
    let mut pending_debug_events = Vec::new();
    {
        // Lock ordering: `tree` before `panes` (see `ServerState` docs).
        let mut tree = state.tree.write().await;
        let mut panes = state.panes.write().await;

        for classified_pane in classifications {
            let pane_id = classified_pane.pane_id;
            let Some(PaneResource::Terminal(runtime)) = panes.get_mut(&pane_id) else {
                // The pane was closed (or is no longer a terminal) between
                // phase 1's snapshot and this phase -- nothing left to
                // apply a status update to.
                continue;
            };

            // A user-triggered force request that arrived after phase 2's
            // snapshot explicitly asks for a newer sample. Never let this stale
            // pass overwrite that request's due deadline or status.
            if runtime.detection_schedule.request_generation != classified_pane.request_generation {
                runtime.detection_schedule.next_due = runtime.detection_schedule.next_due.min(now);
                continue;
            }

            // Preserve the ownership state that discovery evaluated. Runtime
            // fields may be cleared or replaced below before the diagnostic
            // event is assembled, but the log must show both sides of the
            // decision instead of reconstructing the old side afterward.
            let session_id_before_check = runtime.session_id.clone();
            let session_process_id_before_check = runtime.session_process_id;
            let invalidated_session_id_before_check = runtime.invalidated_session_id.clone();
            let identity_was_invalidated_before_check = runtime.is_session_identity_invalidated;
            let transition_correlation_id_before_check =
                runtime.pending_session_transition_correlation_id.clone();

            // PTY output can arrive continuously while an agent works. Applying
            // this coherent frame is safe, but it must not push the next check
            // to a slow tier when a newer frame already exists. The focused-tier
            // delay converges promptly without turning animated output into an
            // unbounded process-table scan loop.
            //
            // Scoped to panes where an agent process was actually identified:
            // per README "Poll cadence", `Idle`/`Done`/`PlainShell` panes are
            // meant to poll slow specifically *because* they don't change on
            // their own -- but an ordinary shell can still produce continuous
            // PTY output on its own (`tail -f`, a build log, `top`). Without
            // this scope, such a pane's screen generation would keep bumping
            // between snapshot and apply forever, pinning it to
            // `FOCUSED_POLL_INTERVAL` and defeating the slow tier's entire
            // purpose of not burning CPU on a process-table scan for panes
            // that were never running an agent to begin with.
            let screen_changed_after_snapshot = classified_pane.identity.is_some()
                && runtime.session.screen_generation() != classified_pane.screen_generation;

            runtime.confirmed_goal_owner = classified_pane.confirmed_goal_owner.clone();

            let previous_status = tree.get(pane_id).and_then(|node| match &node.kind {
                ilium_core::NodeKind::Pane { status, .. } => Some(status.clone()),
                ilium_core::NodeKind::Container(_) | ilium_core::NodeKind::Folder { .. } => None,
            });

            // Raw classification (`classify_pane`/`ilium_detect::classify_activity`)
            // never produces `Done` -- it has no memory of what a pane was
            // doing a moment ago. `promote_to_done` is what actually turns
            // "just went idle" into the durable completed-turn state; see its
            // doc comment.
            let raw_status = classified_pane.status.clone();
            let new_status = match classified_pane.status {
                PaneStatus::Agent(class, raw_activity) => PaneStatus::Agent(
                    class,
                    promote_to_done(previous_status.as_ref(), raw_activity),
                ),
                PaneStatus::AgentWithGoal(class, raw_activity) => PaneStatus::AgentWithGoal(
                    class,
                    promote_to_done(previous_status.as_ref(), raw_activity),
                ),
                other => other,
            };

            runtime.detection_schedule.current_interval = interval_for(
                &new_status,
                runtime.detection_schedule.client_focused,
                &state.detection_config,
            );
            runtime.detection_schedule.next_due = if screen_changed_after_snapshot {
                now + FOCUSED_POLL_INTERVAL
            } else {
                now + runtime.detection_schedule.current_interval
            };

            let detected_agent_class = classified_pane
                .identity
                .as_ref()
                .map(|identity| identity.class.clone());
            runtime.detected_agent_process_id = classified_pane
                .identity
                .as_ref()
                .map(|identity| identity.pid);
            runtime.detected_agent_class = detected_agent_class.clone();
            let became_fresh_agent_screen =
                classified_pane.is_fresh_agent_screen && !runtime.is_showing_fresh_agent_screen;
            runtime.is_showing_fresh_agent_screen = classified_pane.is_fresh_agent_screen;
            if became_fresh_agent_screen {
                runtime.title_generation = runtime.title_generation.wrapping_add(1);
                pending_title_clears.push((pane_id, runtime.title_generation));
                match tree.set_automatic_pane_title(
                    pane_id,
                    crate::pane::FRESH_AGENT_TITLE,
                    None,
                    None,
                ) {
                    Ok(changed) => tree_snapshot_changed |= changed,
                    Err(error) => tracing::warn!(
                        "detection loop: failed to reset title for fresh agent pane \
                         {pane_id:?}: {error}"
                    ),
                }
            }
            if classified_pane.pending_generated_session_id.is_some()
                && detected_agent_class
                    .as_ref()
                    .is_some_and(|class| *class != ilium_core::AgentClass::Claude)
            {
                runtime.pending_generated_session_id = None;
            }
            let session_belongs_to_different_class = runtime.session_id.is_some()
                && runtime.session_agent_class.is_some()
                && detected_agent_class.is_some()
                && runtime.session_agent_class != detected_agent_class;
            let owning_process_disappeared = runtime.session_id.is_some()
                && runtime.session_process_id.is_some()
                && classified_pane.identity.is_none()
                && matches!(
                    previous_status.as_ref(),
                    Some(PaneStatus::Agent(..) | PaneStatus::AgentWithGoal(..))
                )
                && matches!(&new_status, PaneStatus::PlainShell);
            let owning_process_changed_without_reverification = runtime.session_id.is_some()
                && runtime.session_process_id.is_some()
                && classified_pane
                    .identity
                    .as_ref()
                    .is_some_and(|identity| Some(identity.pid) != runtime.session_process_id)
                && discovered_session_ids.get(&pane_id) != runtime.session_id.as_ref();
            let session_is_ambiguously_claimed = runtime
                .session_id
                .as_ref()
                .is_some_and(|session_id| ambiguous_session_ids.contains(session_id));
            let should_clear_session_id = session_identity_is_stale(
                runtime.is_session_identity_invalidated,
                session_belongs_to_different_class,
                owning_process_disappeared,
                owning_process_changed_without_reverification,
                session_is_ambiguously_claimed,
            );
            let mut session_was_cleared = false;
            let mut cleared_session_id = None;
            if should_clear_session_id && runtime.session_id.is_some() {
                cleared_session_id = runtime.session_id.clone();
                if session_is_ambiguously_claimed {
                    runtime.invalidated_session_id = runtime.session_id.clone();
                    runtime.is_session_identity_invalidated = true;
                }
                runtime.session_id = None;
                session_was_cleared = true;
                runtime.title_generation = runtime.title_generation.wrapping_add(1);
                runtime.session_agent_class = None;
                if !runtime.is_session_identity_invalidated {
                    runtime.session_process_id = None;
                }
                state.request_snapshot_save();
                state.broadcast(ServerEvent::PaneSessionIdCleared {
                    pane_id,
                    title_generation: runtime.title_generation,
                });
                match tree.set_automatic_pane_title(
                    pane_id,
                    runtime.origin.pane_name_without_stale_session(),
                    None,
                    None,
                ) {
                    Ok(changed) => tree_snapshot_changed |= changed,
                    Err(error) => tracing::warn!(
                        "detection loop: failed to reset automatic title for pane \
                         {pane_id:?} after clearing its session ID: {error}"
                    ),
                }
            }

            let mut newly_resolved_session = None;
            if let Some(session_id) = discovered_session_ids.get(&pane_id) {
                if runtime.session_id.as_ref() != Some(session_id) {
                    let invalidated_session_id = runtime.invalidated_session_id.clone();
                    let correlation_id = runtime.pending_session_transition_correlation_id.take();
                    runtime.session_id = Some(session_id.clone());
                    runtime.session_agent_class = detected_agent_class.clone();
                    runtime.session_process_id = classified_pane
                        .identity
                        .as_ref()
                        .map(|identity| identity.pid);
                    runtime.is_session_identity_invalidated = false;
                    runtime.invalidated_session_id = None;
                    runtime.pending_generated_session_id = None;
                    newly_resolved_session =
                        Some((session_id.clone(), invalidated_session_id, correlation_id));
                    state.request_snapshot_save();
                    state.broadcast(ServerEvent::PaneSessionIdResolved {
                        pane_id,
                        session_id: session_id.clone(),
                        process_id: runtime.session_process_id,
                        title_generation: runtime.title_generation,
                    });
                } else {
                    let previous_process_id = runtime.session_process_id;
                    runtime.session_agent_class = detected_agent_class;
                    runtime.session_process_id = classified_pane
                        .identity
                        .as_ref()
                        .map(|identity| identity.pid);
                    if runtime.session_process_id != previous_process_id {
                        state.broadcast(ServerEvent::PaneSessionIdResolved {
                            pane_id,
                            session_id: session_id.clone(),
                            process_id: runtime.session_process_id,
                            title_generation: runtime.title_generation,
                        });
                    }
                }
            }

            let debug_context = AgentDebugContext {
                class: classified_pane
                    .identity
                    .as_ref()
                    .map(|identity| identity.class.clone()),
                activity: status_activity(&new_status),
                process_id: classified_pane
                    .identity
                    .as_ref()
                    .map(|identity| identity.pid),
                session_id: runtime.session_id.clone(),
                title_generation: runtime.title_generation,
            };
            if let Some(phases) = session_discovery_traces.get(&pane_id) {
                let project_cwd = pane_project_cwds
                    .get(&pane_id)
                    .unwrap_or(&state.session_cwd);
                let exclusion_details = session_discovery_exclusions
                    .get(&pane_id)
                    .filter(|details| !details.is_empty())
                    .map_or_else(|| "none".to_string(), |details| details.join("\n"));
                let mut discovery_fields = vec![
                    AgentDebugField::plain(
                        "Provider under check",
                        classified_pane.identity.as_ref().map_or_else(
                            || "none".to_string(),
                            |identity| agent_class_name(&identity.class).to_string(),
                        ),
                    ),
                    AgentDebugField::plain(
                        "Agent PID under check",
                        classified_pane.identity.as_ref().map_or_else(
                            || "unavailable".to_string(),
                            |identity| identity.pid.to_string(),
                        ),
                    ),
                    AgentDebugField::plain(
                        "Canonical project boundary",
                        project_cwd.display().to_string(),
                    ),
                    AgentDebugField::sensitive(
                        "Session ID before check",
                        session_id_before_check
                            .clone()
                            .unwrap_or_else(|| "none".to_string()),
                    ),
                    AgentDebugField::plain(
                        "Owning PID before check",
                        session_process_id_before_check.map_or_else(
                            || "none".to_string(),
                            |process_id| process_id.to_string(),
                        ),
                    ),
                    AgentDebugField::plain(
                        "Identity invalidated before check",
                        identity_was_invalidated_before_check.to_string(),
                    ),
                    AgentDebugField::sensitive(
                        "Invalidated session ID",
                        invalidated_session_id_before_check
                            .clone()
                            .unwrap_or_else(|| "none".to_string()),
                    ),
                    AgentDebugField::sensitive("Excluded session IDs", exclusion_details),
                ];
                discovery_fields.extend(phases.iter().map(|phase| {
                    AgentDebugField::plain(
                        format!("Phase: {}", sentence_case(phase.phase)),
                        format!("{}: {}", sentence_case(phase.outcome), phase.detail),
                    )
                }));
                discovery_fields.push(AgentDebugField::sensitive(
                    "Session ID after check",
                    runtime
                        .session_id
                        .clone()
                        .unwrap_or_else(|| "none".to_string()),
                ));
                let did_resolve = discovered_session_ids.contains_key(&pane_id);
                pending_debug_events.push((
                    pane_id,
                    AgentDebugSource::SessionDiscovery,
                    debug_context.clone(),
                    AgentDebugEventDraft {
                        severity: if did_resolve {
                            AgentDebugSeverity::Success
                        } else {
                            AgentDebugSeverity::Information
                        },
                        kind: AgentDebugEventKind::SessionDiscovery,
                        summary: if did_resolve {
                            "Session discovery selected a project-verified identity".to_string()
                        } else {
                            "Session discovery found no admissible identity".to_string()
                        },
                        fields: discovery_fields,
                        correlation_id: transition_correlation_id_before_check,
                        metadata: Default::default(),
                    },
                ));
            }
            let mut detection_fields = vec![
                AgentDebugField::plain(
                    "Agent identity decision",
                    explain_identity_decision(
                        classified_pane.identity.as_ref(),
                        classified_pane.shell_pid,
                    ),
                ),
                AgentDebugField::plain(
                    "Activity decision",
                    explain_activity_decision(
                        &raw_status,
                        &new_status,
                        classified_pane.activity_evidence,
                    ),
                ),
                AgentDebugField::plain(
                    "Goal decision",
                    explain_goal_decision(
                        &new_status,
                        classified_pane.goal_evidence,
                        classified_pane.goal_was_retained,
                    ),
                ),
                AgentDebugField::plain(
                    "Session identity decision",
                    explain_session_decision(
                        classified_pane.identity.as_ref(),
                        runtime.session_id.as_deref(),
                    ),
                ),
                AgentDebugField::plain("Applied pane state", describe_pane_status(&new_status)),
            ];
            if let Some(line) = classified_pane.activity_evidence_line.as_deref() {
                detection_fields.push(AgentDebugField::sensitive(
                    "Matched activity evidence",
                    normalize_dynamic_terminal_evidence(line),
                ));
            }
            if let Some(line) = classified_pane.goal_evidence_line.as_deref() {
                detection_fields.push(AgentDebugField::sensitive(
                    "Matched goal evidence",
                    normalize_dynamic_terminal_evidence(line),
                ));
            }
            let was_agent = matches!(
                previous_status.as_ref(),
                Some(PaneStatus::Agent(..) | PaneStatus::AgentWithGoal(..))
            );
            if classified_pane.identity.is_some() || was_agent {
                pending_debug_events.push((
                    pane_id,
                    AgentDebugSource::Detector,
                    debug_context.clone(),
                    AgentDebugEventDraft::information(
                        AgentDebugEventKind::DetectionCycle,
                        detection_summary(&new_status, classified_pane.identity.as_ref()),
                    )
                    .with_fields(detection_fields),
                ));
            }
            if session_was_cleared {
                let mut reasons = Vec::new();
                if session_belongs_to_different_class {
                    reasons.push("a different agent provider now owns the pane");
                }
                if owning_process_disappeared {
                    reasons.push("the process that owned the session disappeared");
                }
                if owning_process_changed_without_reverification {
                    reasons.push("the agent process changed without re-verifying the session");
                }
                if session_is_ambiguously_claimed {
                    reasons.push("more than one pane claimed the same session");
                }
                pending_debug_events.push((
                    pane_id,
                    AgentDebugSource::SessionDiscovery,
                    debug_context.clone(),
                    AgentDebugEventDraft {
                        severity: AgentDebugSeverity::Warning,
                        kind: AgentDebugEventKind::SessionCleared,
                        summary: "Stale agent session ownership was cleared".to_string(),
                        fields: vec![
                            AgentDebugField::plain(
                                "Why it was cleared",
                                format!(
                                    "The session became unsafe to keep because {}.",
                                    reasons.join(" and ")
                                ),
                            ),
                            AgentDebugField::sensitive(
                                "Session ID before clearing",
                                cleared_session_id.unwrap_or_else(|| "unavailable".to_string()),
                            ),
                        ],
                        correlation_id: None,
                        metadata: Default::default(),
                    },
                ));
            }
            if let Some((session_id, invalidated_session_id, correlation_id)) =
                newly_resolved_session
            {
                let acceptance_reason = session_discovery_traces
                    .get(&pane_id)
                    .and_then(|phases| {
                        phases
                            .iter()
                            .find(|phase| phase.outcome == "resolved" && phase.phase != "overall result")
                    })
                    .map_or_else(
                        || "provider, process, transcript, and project ownership checks all accepted it".to_string(),
                        |phase| phase.detail.clone(),
                    );
                pending_debug_events.push((
                    pane_id,
                    AgentDebugSource::SessionDiscovery,
                    debug_context.clone(),
                    AgentDebugEventDraft {
                        severity: AgentDebugSeverity::Success,
                        kind: AgentDebugEventKind::SessionResolved,
                        summary: "Project-verified agent session resolved".to_string(),
                        fields: vec![
                            AgentDebugField::sensitive("Accepted session ID", session_id),
                            AgentDebugField::sensitive(
                                "Replaced invalidated session ID",
                                invalidated_session_id.unwrap_or_else(|| "none".to_string()),
                            ),
                            AgentDebugField::plain("Why it was accepted", acceptance_reason),
                        ],
                        correlation_id,
                        metadata: Default::default(),
                    },
                ));
            }

            if previous_status.as_ref() == Some(&new_status) {
                continue;
            }
            if matches!(
                (previous_status.as_ref(), &new_status),
                (
                    Some(PaneStatus::PlainShell),
                    PaneStatus::Agent(..) | PaneStatus::AgentWithGoal(..)
                ) | (
                    Some(PaneStatus::Agent(..) | PaneStatus::AgentWithGoal(..)),
                    PaneStatus::PlainShell
                )
            ) {
                if let Some(tracker) = &mut runtime.shell_command_tracker {
                    tracker.reset_pending_line();
                }
            }

            let (pane_name_before_update, short_pane_name_before_update) = tree
                .get(pane_id)
                .map(|node| (Some(node.name.clone()), node.short_name.clone()))
                .unwrap_or_default();

            if let Err(error) = tree.set_pane_status(pane_id, new_status.clone()) {
                // A pane present in the registry but missing from the tree
                // would be an invariant violation elsewhere (both are
                // always updated together on create/close); log and skip
                // rather than letting one inconsistent entry stop every
                // other pane's status update this tick.
                tracing::error!("detection loop: pane {pane_id:?} status update rejected: {error}");
                continue;
            }

            // Queued only once the status update actually took -- a
            // rejected `set_pane_status` above (a stale/inconsistent
            // registry entry) must never fire a notification for a
            // transition that didn't actually happen.
            if state.notifications_config.enabled
                && notifications::is_finished_transition(previous_status.as_ref(), &new_status)
            {
                pending_notifications.push(PendingNotification::from_pane_titles(
                    state.session_name.clone(),
                    pane_name_before_update.clone().unwrap_or_default(),
                    short_pane_name_before_update,
                ));
            }

            if is_agent_finished_transition(previous_status.as_ref(), &new_status) {
                completed_pane_ids.push(pane_id);
            }

            if let Some(event) =
                ilium_sound::event_for_transition(previous_status.as_ref(), &new_status)
            {
                if sound_settings.events.is_enabled(event) {
                    pending_sounds.push(PlaybackRequest {
                        settings: sound_settings.clone(),
                        event: Some(event),
                        pane_name: pane_name_before_update.clone(),
                    });
                }
            }

            state.broadcast(ServerEvent::PaneStatusChanged {
                pane_id,
                status: new_status,
            });
        }
    }

    for (pane_id, source, context, event) in pending_debug_events {
        let _ =
            crate::agent_debug::record_with_context(state, pane_id, source, context, event).await;
    }

    for (pane_id, title_generation) in pending_title_clears {
        state.broadcast(ServerEvent::PaneSessionTitleCleared {
            pane_id,
            title_generation,
        });
    }

    if tree_snapshot_changed {
        let snapshot = state.tree.read().await.clone();
        state.broadcast(ServerEvent::TreeSnapshot(snapshot));
        state.request_snapshot_save();
    }

    for pane_id in completed_pane_ids {
        crate::prompt_queue::deliver_next_after_completion(state, pane_id).await;
    }

    for pending in pending_notifications {
        notifications::send(pending).await;
    }
    for pending in pending_sounds {
        sounds::enqueue(state, pending);
    }

    Ok(())
}

fn status_activity(status: &PaneStatus) -> Option<AgentActivity> {
    match status {
        PaneStatus::Agent(_, activity) | PaneStatus::AgentWithGoal(_, activity) => Some(*activity),
        PaneStatus::PlainShell | PaneStatus::Editor { .. } | PaneStatus::Board => None,
    }
}

fn detection_summary(
    status: &PaneStatus,
    identity: Option<&ilium_detect::AgentIdentity>,
) -> String {
    let Some(identity) = identity else {
        return "No supported agent process is currently detected".to_string();
    };
    let activity = status_activity(status)
        .map(activity_name)
        .unwrap_or("not evaluated");
    let goal = if matches!(status, PaneStatus::AgentWithGoal(..)) {
        " with an active goal"
    } else {
        ""
    };
    format!(
        "Detected {} (process {}) and classified it as {activity}{goal}",
        agent_class_name(&identity.class),
        identity.pid,
    )
}

fn explain_identity_decision(
    identity: Option<&ilium_detect::AgentIdentity>,
    shell_pid: Option<u32>,
) -> String {
    let shell_process = shell_pid.map_or_else(
        || "an unavailable pane-shell process".to_string(),
        |process_id| format!("pane-shell process {process_id}"),
    );
    let Some(identity) = identity else {
        return format!(
            "No known agent executable was found in the process tree below {shell_process}."
        );
    };
    let process_position = if identity.process_tree_depth == 0 {
        "the process launched directly by the pane".to_string()
    } else {
        format!(
            "{} process-tree edge(s) below the pane shell",
            identity.process_tree_depth
        )
    };
    format!(
        "{} was selected because executable \"{}\" at process {} matched registry signature \"{}\"; it is {process_position} below {shell_process}.",
        agent_class_name(&identity.class),
        identity.process_name,
        identity.pid,
        identity.matched_signature,
    )
}

fn explain_activity_decision(
    raw_status: &PaneStatus,
    applied_status: &PaneStatus,
    evidence: Option<ilium_detect::ActivityEvidence>,
) -> String {
    let Some(applied_activity) = status_activity(applied_status) else {
        return "Activity was not evaluated because no agent process was detected.".to_string();
    };
    let raw_activity = status_activity(raw_status).unwrap_or(applied_activity);
    let evidence_explanation = match evidence {
        Some(ilium_detect::ActivityEvidence::InterruptMarker) => {
            "the visible terminal contains the agent's explicit interrupt hint"
        }
        Some(ilium_detect::ActivityEvidence::GenericLiveStatus) => {
            "the visible terminal contains a live status line with an ellipsis and elapsed-time token"
        }
        Some(ilium_detect::ActivityEvidence::ClaudeLiveStatus) => {
            "Claude Code's visible terminal contains its live ellipsis-and-elapsed-time status line"
        }
        Some(ilium_detect::ActivityEvidence::CodexLiveStatus) => {
            "Codex's visible terminal contains a present-tense working status with an elapsed-time token"
        }
        Some(ilium_detect::ActivityEvidence::BackgroundWait) => {
            "the visible terminal says the agent is waiting for background agents or tasks"
        }
        Some(ilium_detect::ActivityEvidence::ConfirmationPrompt) => {
            "the visible terminal contains a yes/no confirmation question"
        }
        Some(ilium_detect::ActivityEvidence::SelectionPrompt) => {
            "the visible terminal contains an interactive selection menu or select/cancel footer"
        }
        Some(ilium_detect::ActivityEvidence::NoActiveMarker) => {
            "the visible terminal contains no working, background-wait, confirmation, or selection marker"
        }
        None => "activity evidence was unavailable",
    };

    if raw_activity == AgentActivity::Idle && applied_activity == AgentActivity::Done {
        return format!(
            "Done — {evidence_explanation}. The pane had previously been active, so ilium keeps that completed turn marked until new terminal input or renewed activity acknowledges it."
        );
    }
    format!(
        "{} — {evidence_explanation}.",
        sentence_case(activity_name(applied_activity))
    )
}

fn explain_goal_decision(
    status: &PaneStatus,
    evidence: Option<ilium_detect::GoalEvidence>,
    goal_was_retained: bool,
) -> String {
    if status_activity(status).is_none() {
        return "Goal state was not evaluated because no agent process was detected.".to_string();
    }
    if goal_was_retained {
        return "Kept active — this frame had no decisive goal footer, but the exact same agent process and provider previously confirmed the goal. Transient redraws therefore do not clear it.".to_string();
    }
    match evidence {
        Some(ilium_detect::GoalEvidence::Active) => "Active — the newest provider-owned status line explicitly reports an active, paused, blocked, or usage-limited goal.".to_string(),
        Some(ilium_detect::GoalEvidence::Inactive) => "Inactive — the newest provider-owned status line explicitly reports that the goal was achieved or cleared.".to_string(),
        Some(ilium_detect::GoalEvidence::Unknown) => "Inactive — no provider-owned goal marker was visible and this exact agent process had no previously confirmed goal to retain.".to_string(),
        None => "Goal evidence was unavailable.".to_string(),
    }
}

fn explain_session_decision(
    identity: Option<&ilium_detect::AgentIdentity>,
    session_id: Option<&str>,
) -> String {
    let Some(identity) = identity else {
        return "No session identity was evaluated because no agent process was detected."
            .to_string();
    };
    match session_id {
        Some(session_id) => format!(
            "Verified session \"{session_id}\" belongs to the same {} process {} and canonical project.",
            agent_class_name(&identity.class),
            identity.pid,
        ),
        None => "No session ID was accepted. ilium requires exact process ownership plus a provider transcript that verifies this canonical project; the checks below show where resolution stopped.".to_string(),
    }
}

fn describe_pane_status(status: &PaneStatus) -> String {
    match status {
        PaneStatus::PlainShell => "Plain shell; no agent badge is applied.".to_string(),
        PaneStatus::Agent(class, activity) => format!(
            "{} agent; activity is {}; no active goal badge.",
            agent_class_name(class),
            activity_name(*activity),
        ),
        PaneStatus::AgentWithGoal(class, activity) => format!(
            "{} agent; activity is {}; active goal badge shown.",
            agent_class_name(class),
            activity_name(*activity),
        ),
        PaneStatus::Editor { .. } => "Editor pane; agent detection does not apply.".to_string(),
        PaneStatus::Board => "Board pane; agent detection does not apply.".to_string(),
    }
}

fn agent_class_name(class: &ilium_core::AgentClass) -> &str {
    match class {
        ilium_core::AgentClass::Claude => "Claude Code",
        ilium_core::AgentClass::Codex => "Codex",
        ilium_core::AgentClass::Antigravity => "Antigravity",
        ilium_core::AgentClass::Other(name) => name.as_str(),
    }
}

const fn activity_name(activity: AgentActivity) -> &'static str {
    match activity {
        AgentActivity::Working => "working",
        AgentActivity::WaitingApproval => "waiting for your approval",
        AgentActivity::WaitingBackground => "waiting for background work",
        AgentActivity::Idle => "idle",
        AgentActivity::Done => "done",
    }
}

/// Replaces volatile counters inside a matched terminal-chrome excerpt. The
/// evidence shape remains visible, but an elapsed-time/token counter increasing
/// alone cannot turn the next one-second poll into a new durable event.
fn normalize_dynamic_terminal_evidence(line: &str) -> String {
    let mut normalized = String::new();
    let mut inside_number = false;
    for character in line.chars() {
        if character.is_ascii_digit() {
            if !inside_number {
                normalized.push_str("<number>");
                inside_number = true;
            }
        } else {
            inside_number = false;
            normalized.push(character);
        }
    }
    normalized
}

fn sentence_case(value: &str) -> String {
    let mut characters = value.chars();
    let Some(first) = characters.next() else {
        return String::new();
    };
    first.to_uppercase().chain(characters).collect()
}

/// Returns whether existing transcript ownership remains conclusive enough
/// to avoid repeated command/cwd refreshes and `/proc/<pid>/fd` scans.
fn session_owner_is_stable(
    session_id: Option<&str>,
    session_agent_class: Option<&ilium_core::AgentClass>,
    session_process_id: Option<u32>,
    is_invalidated: bool,
    identity: &ilium_detect::AgentIdentity,
    ambiguous_session_ids: &std::collections::HashSet<String>,
) -> bool {
    session_id.is_some()
        && session_agent_class == Some(&identity.class)
        && session_process_id == Some(identity.pid)
        && !is_invalidated
        && !session_id.is_some_and(|session_id| ambiguous_session_ids.contains(session_id))
}

fn is_agent_finished_transition(previous: Option<&PaneStatus>, next: &PaneStatus) -> bool {
    matches!(
        (previous, next),
        (
            Some(
                PaneStatus::Agent(_, AgentActivity::Working)
                    | PaneStatus::AgentWithGoal(_, AgentActivity::Working)
            ),
            PaneStatus::Agent(_, AgentActivity::Done)
                | PaneStatus::AgentWithGoal(_, AgentActivity::Done)
        )
    )
}

#[cfg(test)]
mod prompt_queue_transition_tests {
    use super::*;
    use ilium_core::AgentClass;

    #[test]
    fn only_working_to_done_opens_the_prompt_queue_gate() {
        let working = PaneStatus::Agent(AgentClass::Codex, AgentActivity::Working);
        let done = PaneStatus::Agent(AgentClass::Codex, AgentActivity::Done);
        let idle = PaneStatus::Agent(AgentClass::Codex, AgentActivity::Idle);
        assert!(is_agent_finished_transition(Some(&working), &done));
        assert!(!is_agent_finished_transition(Some(&idle), &done));
        assert!(!is_agent_finished_transition(Some(&working), &idle));
    }
}

fn session_identity_is_stale(
    is_session_identity_invalidated: bool,
    session_belongs_to_different_class: bool,
    owning_process_disappeared: bool,
    owning_process_changed_without_reverification: bool,
    session_is_ambiguously_claimed: bool,
) -> bool {
    is_session_identity_invalidated
        || session_belongs_to_different_class
        || owning_process_disappeared
        || owning_process_changed_without_reverification
        || session_is_ambiguously_claimed
}

/// Pulls `schedule.next_due` forward so the deadline-driven loop picks this
/// pane up immediately, or at the end of an active debounce window. Every
/// request advances `request_generation`, even when its deadline is coalesced,
/// so an in-flight classification can never overwrite a newer user request.
/// Called from
/// `ipc::handlers::handle_key_input` (Enter keypress) and
/// `handle_set_pane_focus` (a pane gaining or losing client focus), the
/// two triggers this crate treats as "the user just did something that
/// means this pane's status may be stale right now."
pub fn force_check(schedule: &mut crate::pane::DetectionSchedule, now: Instant) -> bool {
    schedule.request_generation = schedule.request_generation.wrapping_add(1);

    let requested_deadline = match schedule.last_forced {
        Some(last_forced) if now.saturating_duration_since(last_forced) < FORCE_CHECK_DEBOUNCE => {
            last_forced + FORCE_CHECK_DEBOUNCE
        }
        Some(_) | None => {
            schedule.last_forced = Some(now);
            now
        }
    };
    let previous_deadline = schedule.next_due;
    schedule.next_due = schedule.next_due.min(requested_deadline);
    schedule.next_due < previous_deadline
}

/// One pure identity/screen reduction result. Goal ownership stays separate
/// from `PaneStatus` so the caller can persist it across inconclusive frames.
struct IdentityClassification {
    status: PaneStatus,
    confirmed_goal_owner: Option<ConfirmedGoalOwner>,
    activity_evidence: Option<ilium_detect::ActivityEvidence>,
    activity_evidence_line: Option<String>,
    goal_evidence: Option<ilium_detect::GoalEvidence>,
    goal_evidence_line: Option<String>,
    goal_was_retained: bool,
}

/// Combines an already-resolved process identity with the screen snapshot
/// captured only for that identified agent. Process-tree traversal remains
/// separate so plain shells never pay for a vt100 text allocation. Positive
/// and negative goal evidence update ownership; silence retains it only for
/// the exact same PID and provider class.
fn classify_identity(
    identity: Option<&ilium_detect::AgentIdentity>,
    screen_text: &str,
    previous_goal_owner: Option<&ConfirmedGoalOwner>,
) -> IdentityClassification {
    match identity {
        Some(identity) => {
            let activity_classification =
                ilium_detect::classify_activity_for_agent_detailed(&identity.class, screen_text);
            let activity = activity_classification.activity;
            let current_owner = ConfirmedGoalOwner {
                process_id: identity.pid,
                agent_class: identity.class.clone(),
            };
            let goal_classification =
                ilium_detect::goal_evidence_for_agent_detailed(&identity.class, screen_text);
            let goal_was_retained = goal_classification.evidence
                == ilium_detect::GoalEvidence::Unknown
                && previous_goal_owner == Some(&current_owner);
            let confirmed_goal_owner = match goal_classification.evidence {
                ilium_detect::GoalEvidence::Active => Some(current_owner),
                ilium_detect::GoalEvidence::Inactive => None,
                ilium_detect::GoalEvidence::Unknown
                    if previous_goal_owner == Some(&current_owner) =>
                {
                    Some(current_owner)
                }
                ilium_detect::GoalEvidence::Unknown => None,
            };
            let status = if confirmed_goal_owner.is_some() {
                PaneStatus::AgentWithGoal(identity.class.clone(), activity)
            } else {
                PaneStatus::Agent(identity.class.clone(), activity)
            };
            IdentityClassification {
                status,
                confirmed_goal_owner,
                activity_evidence: Some(activity_classification.evidence),
                activity_evidence_line: activity_classification.matched_line,
                goal_evidence: Some(goal_classification.evidence),
                goal_evidence_line: goal_classification.matched_line,
                goal_was_retained,
            }
        }
        None => IdentityClassification {
            status: PaneStatus::PlainShell,
            confirmed_goal_owner: None,
            activity_evidence: None,
            activity_evidence_line: None,
            goal_evidence: None,
            goal_evidence_line: None,
            goal_was_retained: false,
        },
    }
}

/// Turns a raw, memory-less classification (`ilium_detect::classify_activity`
/// only ever returns `Working`/`WaitingBackground`/`WaitingApproval`/`Idle`
/// -- never `Done`, since it looks at nothing but the current screen text)
/// into the stateful activity this loop actually records: a pane that just
/// went idle after active work reads as `Done`, not silently as plain
/// `Idle`. This completion state is shared by sounds and title presentation.
///
/// - Any raw activity other than `Idle` passes through unchanged -- only
///   "just went idle" is ever eligible to become `Done`.
/// - `Done` is sticky through repeated idle reclassifications until fresh
///   terminal input acknowledges it or non-idle detection proves that the
///   agent started processing again.
fn promote_to_done(
    previous: Option<&PaneStatus>,
    raw_activity: ilium_core::AgentActivity,
) -> ilium_core::AgentActivity {
    if raw_activity != ilium_core::AgentActivity::Idle {
        return raw_activity;
    }
    match previous {
        Some(
            PaneStatus::Agent(_, ilium_core::AgentActivity::Working)
            | PaneStatus::AgentWithGoal(_, ilium_core::AgentActivity::Working)
            | PaneStatus::Agent(_, ilium_core::AgentActivity::WaitingApproval)
            | PaneStatus::AgentWithGoal(_, ilium_core::AgentActivity::WaitingApproval)
            | PaneStatus::Agent(_, ilium_core::AgentActivity::WaitingBackground)
            | PaneStatus::AgentWithGoal(_, ilium_core::AgentActivity::WaitingBackground)
            | PaneStatus::Agent(_, ilium_core::AgentActivity::Done)
            | PaneStatus::AgentWithGoal(_, ilium_core::AgentActivity::Done),
        ) => ilium_core::AgentActivity::Done,
        _ => ilium_core::AgentActivity::Idle,
    }
}

/// The next poll interval for a pane just classified as `status`, per
/// README "Poll cadence": `Working`, `WaitingBackground`, and
/// `WaitingApproval` panes poll fast, everything else (idle, done, or no
/// agent detected at all) polls slow.
///
/// `WaitingApproval` and `WaitingBackground` deliberately share the fast
/// tier with `Working` rather than sitting in the slow one with
/// genuinely-static states: both are states most likely to change within
/// seconds (the user answers, or the background subagents finish), and
/// both are states a single mis-firing classification on one transient
/// screen (e.g. a numbered list in an agent's own prose that briefly
/// resembles a selection menu) is most disruptive to leave stale in -- see
/// `ilium-detect::classify_activity`'s heuristics, which only look at
/// *current* screen text and have no memory of their own past verdicts.
/// Fast-repolling means either case self-corrects within one
/// `working_poll_interval`, not up to a full `idle_poll_interval` later.
///
/// `client_focused` overrides all of the above: a pane the attached
/// client currently has open (`ilium_ipc::ClientRequest::SetPaneFocus`)
/// always polls at `FOCUSED_POLL_INTERVAL`, the loop's own fastest tier
/// cadence -- the pane the user is actually looking at right now should
/// never lag behind the coarser working/idle tiers, regardless of what
/// its last classification was.
///
/// Takes `&DetectionConfig` rather than `&ServerState` -- this is a pure
/// decision over the classified status, the focus flag, and the two
/// configured durations, with no need for anything else `ServerState`
/// carries; a narrower parameter keeps it unit-testable without
/// constructing a whole server.
fn interval_for(
    status: &PaneStatus,
    client_focused: bool,
    detection_config: &crate::config::DetectionConfig,
) -> Duration {
    if client_focused {
        return FOCUSED_POLL_INTERVAL;
    }
    match status {
        PaneStatus::Agent(
            _,
            ilium_core::AgentActivity::Working
            | ilium_core::AgentActivity::WaitingBackground
            | ilium_core::AgentActivity::WaitingApproval,
        )
        | PaneStatus::AgentWithGoal(
            _,
            ilium_core::AgentActivity::Working
            | ilium_core::AgentActivity::WaitingBackground
            | ilium_core::AgentActivity::WaitingApproval,
        ) => detection_config.working_poll_interval,
        _ => detection_config.idle_poll_interval,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::DetectionConfig;
    use ilium_core::{AgentActivity, AgentClass};

    fn config() -> DetectionConfig {
        DetectionConfig {
            working_poll_interval: Duration::from_secs(5),
            idle_poll_interval: Duration::from_secs(45),
        }
    }

    #[test]
    fn nearest_detection_deadline_is_exact_and_overdue_is_immediate() {
        let now = Instant::now();
        assert_eq!(
            minimum_detection_delay(
                [
                    now + Duration::from_millis(875),
                    now + Duration::from_millis(23),
                    now + Duration::from_secs(4),
                ]
                .into_iter(),
                now,
            ),
            Some(Duration::from_millis(23))
        );
        assert_eq!(
            minimum_detection_delay([now - Duration::from_millis(1)].into_iter(), now),
            Some(Duration::ZERO)
        );
        assert_eq!(minimum_detection_delay(std::iter::empty(), now), None);
    }

    #[test]
    fn verified_same_process_session_skips_rediscovery_until_ambiguous() {
        let identity = ilium_detect::AgentIdentity {
            class: AgentClass::Codex,
            pid: 42,
            process_name: "codex".to_string(),
            matched_signature: "codex".to_string(),
            process_tree_depth: 1,
        };
        let mut ambiguous = std::collections::HashSet::new();
        assert!(session_owner_is_stable(
            Some("session-a"),
            Some(&AgentClass::Codex),
            Some(42),
            false,
            &identity,
            &ambiguous,
        ));

        ambiguous.insert("session-a".to_string());
        assert!(!session_owner_is_stable(
            Some("session-a"),
            Some(&AgentClass::Codex),
            Some(42),
            false,
            &identity,
            &ambiguous,
        ));
        ambiguous.clear();
        assert!(!session_owner_is_stable(
            Some("session-a"),
            Some(&AgentClass::Codex),
            Some(99),
            false,
            &identity,
            &ambiguous,
        ));
        assert!(!session_owner_is_stable(
            Some("session-a"),
            Some(&AgentClass::Codex),
            Some(42),
            true,
            &identity,
            &ambiguous,
        ));
    }

    #[test]
    fn confirmed_goal_survives_inconclusive_frames_only_for_the_same_agent_process() {
        let codex = ilium_detect::AgentIdentity {
            class: AgentClass::Codex,
            pid: 42,
            process_name: "codex".to_string(),
            matched_signature: "codex".to_string(),
            process_tree_depth: 1,
        };
        let active = classify_identity(
            Some(&codex),
            "model · workspace · Working · Pursuing goal (16m)",
            None,
        );
        assert!(matches!(
            active.status,
            PaneStatus::AgentWithGoal(AgentClass::Codex, _)
        ));
        let owner = active
            .confirmed_goal_owner
            .expect("positive footer evidence must establish process ownership");

        let transient = classify_identity(Some(&codex), "Working (esc to interrupt)", Some(&owner));
        assert!(matches!(
            transient.status,
            PaneStatus::AgentWithGoal(AgentClass::Codex, _)
        ));
        assert_eq!(transient.confirmed_goal_owner.as_ref(), Some(&owner));

        let completed = classify_identity(Some(&codex), "Goal achieved (20m)", Some(&owner));
        assert!(matches!(
            completed.status,
            PaneStatus::Agent(AgentClass::Codex, _)
        ));
        assert_eq!(completed.confirmed_goal_owner, None);

        let replacement = ilium_detect::AgentIdentity {
            class: AgentClass::Codex,
            pid: 43,
            process_name: "codex".to_string(),
            matched_signature: "codex".to_string(),
            process_tree_depth: 1,
        };
        let replacement_without_evidence =
            classify_identity(Some(&replacement), "Send a message", Some(&owner));
        assert!(matches!(
            replacement_without_evidence.status,
            PaneStatus::Agent(AgentClass::Codex, _)
        ));
        assert_eq!(replacement_without_evidence.confirmed_goal_owner, None);
    }

    #[test]
    fn volatile_terminal_counters_do_not_create_distinct_detection_evidence() {
        let first =
            normalize_dynamic_terminal_evidence("Moonwalking… 1/2 · 6s · 42 tokens · process 314");
        let second =
            normalize_dynamic_terminal_evidence("Moonwalking… 2/2 · 7s · 99 tokens · process 2718");

        assert_eq!(first, second);
        assert_eq!(
            first,
            "Moonwalking… <number>/<number> · <number>s · <number> tokens · process <number>"
        );
    }

    #[test]
    fn debug_explanations_state_why_goal_and_activity_decisions_were_applied() {
        let status = PaneStatus::AgentWithGoal(AgentClass::Codex, AgentActivity::Done);

        assert!(
            explain_goal_decision(&status, Some(ilium_detect::GoalEvidence::Unknown), true,)
                .contains("exact same agent process")
        );
        assert!(explain_activity_decision(
            &PaneStatus::Agent(AgentClass::Codex, AgentActivity::Idle),
            &status,
            Some(ilium_detect::ActivityEvidence::NoActiveMarker),
        )
        .contains("previously been active"));
    }

    #[test]
    fn verified_session_explanation_is_stable_after_the_resolution_tick() {
        let identity = ilium_detect::AgentIdentity {
            class: AgentClass::Codex,
            pid: 42,
            process_name: "codex".to_string(),
            matched_signature: "codex".to_string(),
            process_tree_depth: 1,
        };

        assert_eq!(
            explain_session_decision(Some(&identity), Some("session-a")),
            "Verified session \"session-a\" belongs to the same Codex process 42 and canonical project."
        );
    }

    /// Regression test: `WaitingApproval` must poll on the fast tier, same
    /// as `Working` -- previously it shared the slow `idle_poll_interval`
    /// tier with genuinely-static states, so a pane that was ever
    /// misclassified as `WaitingApproval` on one transient screen (or had
    /// its prompt genuinely answered) could show a stale badge for up to
    /// `idle_poll_interval` after the real screen content had already moved
    /// on.
    #[test]
    fn waiting_approval_polls_on_the_fast_tier_like_working() {
        let config = config();
        let waiting = PaneStatus::Agent(AgentClass::Claude, AgentActivity::WaitingApproval);
        let working = PaneStatus::Agent(AgentClass::Claude, AgentActivity::Working);
        assert_eq!(
            interval_for(&waiting, false, &config),
            config.working_poll_interval
        );
        assert_eq!(
            interval_for(&working, false, &config),
            config.working_poll_interval
        );
    }

    #[test]
    fn waiting_background_polls_on_the_fast_tier_like_working() {
        let config = config();
        let waiting_background =
            PaneStatus::Agent(AgentClass::Claude, AgentActivity::WaitingBackground);
        assert_eq!(
            interval_for(&waiting_background, false, &config),
            config.working_poll_interval
        );
    }

    #[test]
    fn idle_done_and_plain_shell_poll_on_the_slow_tier() {
        let config = config();
        let idle = PaneStatus::Agent(AgentClass::Claude, AgentActivity::Idle);
        let done = PaneStatus::Agent(AgentClass::Claude, AgentActivity::Done);
        assert_eq!(
            interval_for(&idle, false, &config),
            config.idle_poll_interval
        );
        assert_eq!(
            interval_for(&done, false, &config),
            config.idle_poll_interval
        );
        assert_eq!(
            interval_for(&PaneStatus::PlainShell, false, &config),
            config.idle_poll_interval
        );
    }

    /// A client-focused pane always polls at `FOCUSED_POLL_INTERVAL`,
    /// overriding even the slow tier -- the pane the user is actually
    /// looking at right now must never lag behind coarser tiers.
    #[test]
    fn focused_pane_polls_on_the_focused_tier_regardless_of_status() {
        let config = config();
        let idle = PaneStatus::Agent(AgentClass::Claude, AgentActivity::Idle);
        assert_eq!(interval_for(&idle, true, &config), FOCUSED_POLL_INTERVAL);
        assert_eq!(
            interval_for(&PaneStatus::PlainShell, true, &config),
            FOCUSED_POLL_INTERVAL
        );
    }

    /// `force_check` pulls `next_due` to `now` on first call. A second call
    /// within `FORCE_CHECK_DEBOUNCE` coalesces at the existing window boundary
    /// while still advancing the generation that invalidates an in-flight pass.
    #[test]
    fn force_check_is_debounced() {
        let mut schedule = crate::pane::DetectionSchedule {
            next_due: Instant::now() + Duration::from_secs(999),
            current_interval: Duration::from_secs(45),
            client_focused: false,
            last_forced: None,
            request_generation: 0,
        };
        let t0 = Instant::now();
        assert!(force_check(&mut schedule, t0));
        assert_eq!(schedule.next_due, t0);
        assert_eq!(schedule.request_generation, 1);

        let t1 = t0 + Duration::from_secs(1);
        schedule.next_due = t0 + Duration::from_secs(30);
        assert!(force_check(&mut schedule, t1));
        assert_eq!(
            schedule.next_due,
            t0 + FORCE_CHECK_DEBOUNCE,
            "a second force must be coalesced at the current debounce boundary"
        );
        assert_eq!(schedule.request_generation, 2);

        let t2 = t0 + FORCE_CHECK_DEBOUNCE;
        schedule.next_due = t0 + Duration::from_secs(30);
        assert!(force_check(&mut schedule, t2));
        assert_eq!(
            schedule.next_due, t2,
            "a force after the debounce window must take effect"
        );
        assert_eq!(schedule.request_generation, 3);

        let t3 = t2 + Duration::from_secs(1);
        assert!(!force_check(&mut schedule, t3));
        assert_eq!(schedule.next_due, t2);
        assert_eq!(
            schedule.request_generation, 4,
            "even an already-earlier deadline must invalidate an in-flight pass"
        );
    }

    fn agent(activity: AgentActivity) -> PaneStatus {
        PaneStatus::Agent(AgentClass::Claude, activity)
    }

    #[test]
    fn finishing_active_turn_becomes_done() {
        assert_eq!(
            promote_to_done(Some(&agent(AgentActivity::Working)), AgentActivity::Idle),
            AgentActivity::Done
        );
    }

    #[test]
    fn done_stays_done_through_repeated_idle_detection() {
        assert_eq!(
            promote_to_done(Some(&agent(AgentActivity::Done)), AgentActivity::Idle),
            AgentActivity::Done
        );
    }

    #[test]
    fn already_idle_stays_idle_rather_than_becoming_done() {
        assert_eq!(
            promote_to_done(Some(&agent(AgentActivity::Idle)), AgentActivity::Idle),
            AgentActivity::Idle
        );
    }

    #[test]
    fn no_prior_status_never_promotes_to_done() {
        assert_eq!(
            promote_to_done(None, AgentActivity::Idle),
            AgentActivity::Idle
        );
    }

    #[test]
    fn non_idle_raw_activity_always_passes_through_unchanged() {
        for raw in [
            AgentActivity::Working,
            AgentActivity::WaitingApproval,
            AgentActivity::WaitingBackground,
        ] {
            assert_eq!(promote_to_done(Some(&agent(AgentActivity::Done)), raw), raw);
        }
    }

    #[test]
    fn waiting_approval_or_background_finishing_unfocused_becomes_done() {
        assert_eq!(
            promote_to_done(
                Some(&agent(AgentActivity::WaitingApproval)),
                AgentActivity::Idle
            ),
            AgentActivity::Done
        );
        assert_eq!(
            promote_to_done(
                Some(&agent(AgentActivity::WaitingBackground)),
                AgentActivity::Idle
            ),
            AgentActivity::Done
        );
    }

    #[test]
    fn duplicate_session_claims_have_no_arbitrary_winner() {
        let shared_session_id = "11111111-1111-4111-8111-111111111111";
        let unique_session_id = "22222222-2222-4222-8222-222222222222";
        let (unique_claims, ambiguous_session_ids) = partition_session_claims([
            (shared_session_id.to_string(), NodeId(10)),
            (unique_session_id.to_string(), NodeId(20)),
            (shared_session_id.to_string(), NodeId(30)),
        ]);

        assert_eq!(unique_claims.get(unique_session_id), Some(&NodeId(20)));
        assert!(!unique_claims.contains_key(shared_session_id));
        assert_eq!(
            ambiguous_session_ids,
            std::collections::HashSet::from([shared_session_id.to_string()])
        );
    }

    #[test]
    fn every_ownership_break_clears_a_stale_session_identity() {
        assert!(session_identity_is_stale(true, false, false, false, false));
        assert!(session_identity_is_stale(false, true, false, false, false));
        assert!(session_identity_is_stale(false, false, true, false, false));
        assert!(session_identity_is_stale(false, false, false, true, false));
        assert!(session_identity_is_stale(false, false, false, false, true));
        assert!(!session_identity_is_stale(
            false, false, false, false, false
        ));
    }
}

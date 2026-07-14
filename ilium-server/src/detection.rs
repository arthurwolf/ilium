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

use ilium_agent_session::TranscriptLocator;
use ilium_core::{NodeId, PaneStatus};
use ilium_ipc::ServerEvent;
use sysinfo::{Pid, System};
use tokio::task::JoinHandle;

use crate::notifications::{self, PendingNotification};
use crate::pane::PaneResource;
use crate::sounds::{self, PlaybackRequest};
use crate::state::ServerState;

/// How often the loop wakes to check which panes are due. Independent of
/// the *configured* working/idle poll intervals (`DetectionConfig`) --
/// this is just the scheduling granularity; a pane's effective poll rate
/// is `current_interval`, rounded up to the next multiple of this tick.
const BASE_TICK_INTERVAL: Duration = Duration::from_secs(1);

/// Minimum time between two "force an immediate recheck" requests actually
/// taking effect for the same pane. A focus transition (entering/exiting a
/// pane) or an Enter keypress each *ask* for an immediate recheck (see
/// [`force_check`]), but coalescing repeated asks within this window keeps
/// rapid focus-flicking or Enter-mashing from pinning a pane's
/// classification to run every single base tick regardless of its
/// configured poll tier.
const FORCE_CHECK_DEBOUNCE: Duration = Duration::from_secs(5);

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
    let mut ticker = tokio::time::interval(BASE_TICK_INTERVAL);
    // `Burst` (default) makes a delayed tick fire immediately followed by
    // however many ticks it "owes"; `Delay` (chosen here) just resumes on
    // the normal cadence from whenever it actually fires. A detection tick
    // that ran long (e.g. under test-runner load) should not then fire a
    // burst of catch-up ticks -- there is nothing to "catch up" on, the
    // per-pane `next_due` timestamps already say what's still due.
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        ticker.tick().await;

        // Skip the expensive `/proc` refresh entirely when nothing is due
        // yet -- its cost scales with *every* process on the machine, not
        // with ilium's own workload, so paying it on every ~1s tick even
        // when e.g. every pane is sitting on the 45s idle-tier interval
        // wastes CPU under heavy machine load for no benefit (no pane's
        // classification would even be looked at this tick). Only a brief
        // `panes` read lock is needed to check this -- no `tree` lock, and
        // nothing here mutates anything.
        if !any_pane_due(&state, Instant::now()).await {
            continue;
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

/// True if at least one terminal pane's `next_due` deadline has already
/// passed, i.e. this tick actually has work to do. Takes only a brief
/// `panes` read lock (no `tree` lock -- nothing here needs it) to check
/// timestamps; does not classify anything.
async fn any_pane_due(state: &ServerState, now: Instant) -> bool {
    let panes = state.panes.read().await;
    panes.values().any(|resource| {
        matches!(
            resource,
            PaneResource::Terminal(runtime) if runtime.detection_schedule.next_due <= now
        )
    })
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
/// A `Working -> Done`/`Idle` transition (see
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
        screen_text: String,
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
                    screen_text: runtime.session.screen_text(),
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

    // Phase 2: pure classification, no locks held. `children_index` is
    // built once here and shared across every due pane below --
    // `identify_agent_with_extra`'s process-tree walk then only visits
    // processes actually reachable as descendants of each pane's own
    // shell, never the whole system process table (see
    // `ilium_detect::ProcessChildrenIndex`).
    let children_index = ilium_detect::ProcessChildrenIndex::build(system);
    struct ClassifiedPane {
        pane_id: NodeId,
        status: PaneStatus,
        identity: Option<ilium_detect::AgentIdentity>,
        is_session_identity_invalidated: bool,
        invalidated_session_id: Option<String>,
        session_process_id: Option<u32>,
        pending_generated_session_id: Option<String>,
    }

    let classifications: Vec<ClassifiedPane> = due_panes
        .into_iter()
        .map(|due_pane| {
            let (status, identity) = classify_pane(
                system,
                &children_index,
                due_pane.shell_pid,
                &due_pane.screen_text,
                &state.custom_signatures,
            );
            ClassifiedPane {
                pane_id: due_pane.pane_id,
                status,
                identity,
                is_session_identity_invalidated: due_pane.is_session_identity_invalidated,
                invalidated_session_id: due_pane.invalidated_session_id,
                session_process_id: due_pane.session_process_id,
                pending_generated_session_id: due_pane.pending_generated_session_id,
            }
        })
        .collect();

    // Refresh command/cwd fields only for identified process IDs. Discovery
    // itself accepts only built-in provider classes; custom signatures do
    // not have a transcript format with a project-verifiable ownership
    // contract, so they intentionally receive no session ID.
    let discovery_pids: Vec<Pid> = classifications
        .iter()
        .filter_map(|pane| {
            pane.identity
                .as_ref()
                .map(|identity| Pid::from_u32(identity.pid))
        })
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
    let transcript_locator = TranscriptLocator::new(&state.home_dir, &state.session_cwd);
    for pane in &classifications {
        let Some(identity) = pane.identity.as_ref() else {
            continue;
        };
        let mut excluded_session_ids: std::collections::HashSet<String> = claimed_session_ids
            .iter()
            .filter(|(_, owner)| **owner != pane.pane_id)
            .map(|(session_id, _)| session_id.clone())
            .collect();
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
        }
        let generated_session = pane
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
        let discovered_session = generated_session.or_else(|| {
            crate::session_id::discover(
                system,
                Pid::from_u32(identity.pid),
                &identity.class,
                &transcript_locator,
                &state.session_cwd,
                pane.is_session_identity_invalidated
                    && pane
                        .session_process_id
                        .is_none_or(|owner_pid| owner_pid == identity.pid),
                &excluded_session_ids,
            )
        });
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
    let mut tree_snapshot_changed = false;
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

            let previous_status = tree.get(pane_id).and_then(|node| match &node.kind {
                ilium_core::NodeKind::Pane { status, .. } => Some(status.clone()),
                ilium_core::NodeKind::Container(_) | ilium_core::NodeKind::Folder { .. } => None,
            });

            // Raw classification (`classify_pane`/`ilium_detect::classify_activity`)
            // never produces `Done` -- it has no memory of what a pane was
            // doing a moment ago. `promote_to_done` is what actually turns
            // "just went idle" into "done" when nobody was watching; see its
            // doc comment.
            let new_status = match classified_pane.status {
                PaneStatus::Agent(class, raw_activity) => PaneStatus::Agent(
                    class,
                    promote_to_done(
                        previous_status.as_ref(),
                        raw_activity,
                        runtime.detection_schedule.client_focused,
                    ),
                ),
                PaneStatus::AgentWithGoal(class, raw_activity) => PaneStatus::AgentWithGoal(
                    class,
                    promote_to_done(
                        previous_status.as_ref(),
                        raw_activity,
                        runtime.detection_schedule.client_focused,
                    ),
                ),
                other => other,
            };

            runtime.detection_schedule.current_interval = interval_for(
                &new_status,
                runtime.detection_schedule.client_focused,
                &state.detection_config,
            );
            runtime.detection_schedule.next_due = now + runtime.detection_schedule.current_interval;

            let detected_agent_class = classified_pane
                .identity
                .as_ref()
                .map(|identity| identity.class.clone());
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
            if should_clear_session_id && runtime.session_id.is_some() {
                if session_is_ambiguously_claimed {
                    runtime.invalidated_session_id = runtime.session_id.clone();
                    runtime.is_session_identity_invalidated = true;
                }
                runtime.session_id = None;
                runtime.session_agent_class = None;
                if !runtime.is_session_identity_invalidated {
                    runtime.session_process_id = None;
                }
                state.request_snapshot_save();
                state.broadcast(ServerEvent::PaneSessionIdCleared { pane_id });
                match tree.set_automatic_pane_title(
                    pane_id,
                    runtime.origin.pane_name_without_stale_session(),
                    None,
                ) {
                    Ok(changed) => tree_snapshot_changed |= changed,
                    Err(error) => tracing::warn!(
                        "detection loop: failed to reset automatic title for pane \
                         {pane_id:?} after clearing its session ID: {error}"
                    ),
                }
            }

            if let Some(session_id) = discovered_session_ids.get(&pane_id) {
                if runtime.session_id.as_ref() != Some(session_id) {
                    runtime.session_id = Some(session_id.clone());
                    runtime.session_agent_class = detected_agent_class.clone();
                    runtime.session_process_id = classified_pane
                        .identity
                        .as_ref()
                        .map(|identity| identity.pid);
                    runtime.is_session_identity_invalidated = false;
                    runtime.invalidated_session_id = None;
                    runtime.pending_generated_session_id = None;
                    state.request_snapshot_save();
                    state.broadcast(ServerEvent::PaneSessionIdResolved {
                        pane_id,
                        session_id: session_id.clone(),
                    });
                } else {
                    runtime.session_agent_class = detected_agent_class;
                    runtime.session_process_id = classified_pane
                        .identity
                        .as_ref()
                        .map(|identity| identity.pid);
                }
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

    if tree_snapshot_changed {
        let snapshot = state.tree.read().await.clone();
        state.broadcast(ServerEvent::TreeSnapshot(snapshot));
    }

    for pending in pending_notifications {
        notifications::send(pending).await;
    }
    for pending in pending_sounds {
        sounds::enqueue(state, pending);
    }

    Ok(())
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

/// Pulls `schedule.next_due` forward to `now` so the next detection tick
/// (at most `BASE_TICK_INTERVAL` away) picks this pane up immediately,
/// unless a previous force-check request already did so within
/// `FORCE_CHECK_DEBOUNCE` -- in which case this is a no-op, leaving
/// whatever `next_due`/`last_forced` were already in place. Called from
/// `ipc::handlers::handle_key_input` (Enter keypress) and
/// `handle_set_pane_focus` (a pane gaining or losing client focus), the
/// two triggers this crate treats as "the user just did something that
/// means this pane's status may be stale right now."
pub fn force_check(schedule: &mut crate::pane::DetectionSchedule, now: Instant) {
    if schedule
        .last_forced
        .is_some_and(|since| now.duration_since(since) < FORCE_CHECK_DEBOUNCE)
    {
        return;
    }
    schedule.last_forced = Some(now);
    schedule.next_due = now;
}

/// Runs the identity + activity classification for one terminal pane, from
/// already-snapshotted inputs (`shell_pid`, `screen_text`) rather than a
/// live `TerminalPaneRuntime` reference -- so this can run entirely outside
/// the `tree`/`panes` locks (see `run_due_panes`'s phase breakdown).
/// `children_index` is built once per tick and shared across every due
/// pane's call (see `ilium_detect::ProcessChildrenIndex`).
/// `extra_signatures` is the session's user-configured
/// `[[detection.custom_signatures]]` list (`ServerState::custom_signatures`),
/// checked alongside `ilium-detect`'s built-in registry via
/// `identify_agent_with_extra`.
fn classify_pane(
    system: &System,
    children_index: &ilium_detect::ProcessChildrenIndex,
    shell_pid: Option<u32>,
    screen_text: &str,
    extra_signatures: &[ilium_detect::AgentSignature],
) -> (PaneStatus, Option<ilium_detect::AgentIdentity>) {
    let Some(shell_pid) = shell_pid else {
        // The platform never reported a pid for this pane's shell (should
        // not happen on the platforms ilium targets, but `process_id`'s
        // own signature allows it) -- nothing to walk a process tree from,
        // so this pane can only ever be reported as a plain shell.
        return (PaneStatus::PlainShell, None);
    };

    match ilium_detect::identify_agent_with_extra(
        system,
        Pid::from_u32(shell_pid),
        children_index,
        extra_signatures,
    ) {
        Some(identity) => {
            let activity = ilium_detect::classify_activity(screen_text);
            let status = if ilium_detect::has_visible_goal(screen_text) {
                PaneStatus::AgentWithGoal(identity.class.clone(), activity)
            } else {
                PaneStatus::Agent(identity.class.clone(), activity)
            };
            (status, Some(identity))
        }
        None => (PaneStatus::PlainShell, None),
    }
}

/// Turns a raw, memory-less classification (`ilium_detect::classify_activity`
/// only ever returns `Working`/`WaitingBackground`/`WaitingApproval`/`Idle`
/// -- never `Done`, since it looks at nothing but the current screen text)
/// into the stateful activity this loop actually records: a pane that just
/// went idle while nobody was looking at it reads as "finished, unseen"
/// (`Done`), not silently as plain `Idle`. Mirrors the pre-client/server
/// `App`'s `next_activity` combinator.
///
/// - Any raw activity other than `Idle` passes through unchanged -- only
///   "just went idle" is ever eligible to become `Done`.
/// - `client_focused` short-circuits to `Idle`: a pane the user is actually
///   looking at right now must never show a stale "come look" badge.
/// - Otherwise, `Done` is sticky: it holds through repeated idle
///   reclassifications (`previous` itself `Done`) until either the pane
///   gets focused or starts a new turn, so a slow-to-look-back user doesn't
///   miss the badge because a later tick happened to reclassify while they
///   were away.
fn promote_to_done(
    previous: Option<&PaneStatus>,
    raw_activity: ilium_core::AgentActivity,
    client_focused: bool,
) -> ilium_core::AgentActivity {
    if raw_activity != ilium_core::AgentActivity::Idle || client_focused {
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
/// always polls at `BASE_TICK_INTERVAL`, the loop's own fastest possible
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
        return BASE_TICK_INTERVAL;
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

    /// A client-focused pane always polls at `BASE_TICK_INTERVAL`,
    /// overriding even the slow tier -- the pane the user is actually
    /// looking at right now must never lag behind coarser tiers.
    #[test]
    fn focused_pane_polls_on_the_base_tick_regardless_of_status() {
        let config = config();
        let idle = PaneStatus::Agent(AgentClass::Claude, AgentActivity::Idle);
        assert_eq!(interval_for(&idle, true, &config), BASE_TICK_INTERVAL);
        assert_eq!(
            interval_for(&PaneStatus::PlainShell, true, &config),
            BASE_TICK_INTERVAL
        );
    }

    /// `force_check` pulls `next_due` to `now` on first call, but a second
    /// call within `FORCE_CHECK_DEBOUNCE` must not push it out again --
    /// otherwise rapid focus-flicking or Enter-mashing would starve the
    /// detection loop into checking a pane every base tick indefinitely.
    #[test]
    fn force_check_is_debounced() {
        let mut schedule = crate::pane::DetectionSchedule {
            next_due: Instant::now() + Duration::from_secs(999),
            current_interval: Duration::from_secs(45),
            client_focused: false,
            last_forced: None,
        };
        let t0 = Instant::now();
        force_check(&mut schedule, t0);
        assert_eq!(schedule.next_due, t0);

        let t1 = t0 + Duration::from_secs(1);
        schedule.next_due = t0 + Duration::from_secs(30);
        force_check(&mut schedule, t1);
        assert_eq!(
            schedule.next_due,
            t0 + Duration::from_secs(30),
            "a second force within the debounce window must not move next_due"
        );

        let t2 = t0 + FORCE_CHECK_DEBOUNCE;
        force_check(&mut schedule, t2);
        assert_eq!(
            schedule.next_due, t2,
            "a force after the debounce window must take effect"
        );
    }

    fn agent(activity: AgentActivity) -> PaneStatus {
        PaneStatus::Agent(AgentClass::Claude, activity)
    }

    #[test]
    fn finishing_while_unfocused_becomes_done() {
        assert_eq!(
            promote_to_done(
                Some(&agent(AgentActivity::Working)),
                AgentActivity::Idle,
                false
            ),
            AgentActivity::Done
        );
    }

    #[test]
    fn finishing_while_focused_stays_idle() {
        assert_eq!(
            promote_to_done(
                Some(&agent(AgentActivity::Working)),
                AgentActivity::Idle,
                true
            ),
            AgentActivity::Idle
        );
    }

    #[test]
    fn done_stays_done_until_focused() {
        assert_eq!(
            promote_to_done(
                Some(&agent(AgentActivity::Done)),
                AgentActivity::Idle,
                false
            ),
            AgentActivity::Done
        );
        assert_eq!(
            promote_to_done(Some(&agent(AgentActivity::Done)), AgentActivity::Idle, true),
            AgentActivity::Idle
        );
    }

    #[test]
    fn already_idle_stays_idle_rather_than_becoming_done() {
        assert_eq!(
            promote_to_done(
                Some(&agent(AgentActivity::Idle)),
                AgentActivity::Idle,
                false
            ),
            AgentActivity::Idle
        );
    }

    #[test]
    fn no_prior_status_never_promotes_to_done() {
        assert_eq!(
            promote_to_done(None, AgentActivity::Idle, false),
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
            assert_eq!(
                promote_to_done(Some(&agent(AgentActivity::Done)), raw, false),
                raw
            );
            assert_eq!(
                promote_to_done(Some(&agent(AgentActivity::Done)), raw, true),
                raw
            );
        }
    }

    #[test]
    fn waiting_approval_or_background_finishing_unfocused_becomes_done() {
        assert_eq!(
            promote_to_done(
                Some(&agent(AgentActivity::WaitingApproval)),
                AgentActivity::Idle,
                false
            ),
            AgentActivity::Done
        );
        assert_eq!(
            promote_to_done(
                Some(&agent(AgentActivity::WaitingBackground)),
                AgentActivity::Idle,
                false
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

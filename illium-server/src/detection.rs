//! The adaptive agent-detection loop: the timing/scheduling/backoff logic
//! that calls into `illium-detect`'s pure `identify_agent`/`classify_activity`
//! functions for every pty-backed pane (see README "Poll cadence" and
//! `illium-detect`'s module docs -- that crate deliberately owns none of
//! this loop itself).
//!
//! One task per session (not one task per pane): a single `sysinfo::System`
//! refresh serves every pane due on a given tick (see
//! `illium_detect::refresh`'s doc comment on why refreshing once per tick
//! is the intended usage), and a single task is trivially one
//! `JoinHandle` to track and cancel at session shutdown, rather than a
//! fleet of per-pane tasks whose lifecycle would need to be individually
//! wired to pane creation/removal.

use std::time::{Duration, Instant};

use illium_core::PaneStatus;
use illium_ipc::ServerEvent;
use sysinfo::{Pid, System};
use tokio::task::JoinHandle;

use crate::pane::PaneResource;
use crate::state::ServerState;

/// How often the loop wakes to check which panes are due. Independent of
/// the *configured* working/idle poll intervals (`DetectionConfig`) --
/// this is just the scheduling granularity; a pane's effective poll rate
/// is `current_interval`, rounded up to the next multiple of this tick.
const BASE_TICK_INTERVAL: Duration = Duration::from_secs(1);

/// Spawns the detection loop as a single tracked task and returns its
/// handle. The loop runs until aborted (session shutdown) -- it has no
/// other exit condition, matching the lifetime of the session itself.
pub fn spawn(state: std::sync::Arc<ServerState>) -> JoinHandle<()> {
    tokio::spawn(run_loop(state))
}

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

        // The refresh is the syscall-heavy part (`/proc` reads for every
        // process on the machine); running it on a blocking thread keeps
        // this tick from stalling the tokio runtime's async tasks (other
        // panes' IO, other connections) while it happens. `identify_agent`
        // itself, called below, is pure in-memory iteration over the
        // already-refreshed snapshot, so it does not need the same
        // treatment.
        let refreshed = tokio::task::spawn_blocking(move || {
            illium_detect::refresh(&mut system);
            system
        })
        .await;
        system = match refreshed {
            Ok(system) => system,
            Err(join_error) => {
                // The blocking task panicked, taking the `System` it owned
                // with it -- there is no way to recover that value, so
                // this tick continues with a fresh (empty until the next
                // successful refresh) one rather than leaving `system`
                // uninitialized for the next loop iteration. Logged and
                // continued rather than taking the whole detection loop
                // (and with it every pane's status updates) down over one
                // bad refresh.
                tracing::error!("detection loop: sysinfo refresh task panicked: {join_error}");
                System::new()
            }
        };

        if let Err(error) = run_due_panes(&state, &system).await {
            tracing::error!("detection loop: tick failed: {error}");
        }
    }
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
async fn run_due_panes(
    state: &ServerState,
    system: &System,
) -> Result<(), crate::error::ServerError> {
    let now = Instant::now();
    // Lock ordering: `tree` before `panes` (see `ServerState` docs).
    let mut tree = state.tree.write().await;
    let mut panes = state.panes.write().await;

    for (pane_id, resource) in panes.iter_mut() {
        let PaneResource::Terminal(runtime) = resource else {
            continue;
        };
        if runtime.detection_schedule.next_due > now {
            continue;
        }

        let new_status = classify_pane(system, runtime);
        runtime.detection_schedule.current_interval = interval_for(&new_status, state);
        runtime.detection_schedule.next_due = now + runtime.detection_schedule.current_interval;

        let previous_status = tree.get(*pane_id).and_then(|node| match &node.kind {
            illium_core::NodeKind::Pane { status, .. } => Some(status.clone()),
            illium_core::NodeKind::Group { .. } => None,
        });
        if previous_status.as_ref() == Some(&new_status) {
            continue;
        }

        if let Err(error) = tree.set_pane_status(*pane_id, new_status.clone()) {
            // A pane present in the registry but missing from the tree
            // would be an invariant violation elsewhere (both are always
            // updated together on create/close); log and skip rather than
            // letting one inconsistent entry stop every other pane's
            // classification this tick.
            tracing::error!("detection loop: pane {pane_id:?} status update rejected: {error}");
            continue;
        }
        state.broadcast(ServerEvent::PaneStatusChanged {
            pane_id: *pane_id,
            status: new_status,
        });
    }

    Ok(())
}

/// Runs the identity + activity classification for one terminal pane.
fn classify_pane(system: &System, runtime: &crate::pane::TerminalPaneRuntime) -> PaneStatus {
    let Some(shell_pid) = runtime.session.process_id() else {
        // The platform never reported a pid for this pane's shell (should
        // not happen on the platforms illium targets, but `process_id`'s
        // own signature allows it) -- nothing to walk a process tree from,
        // so this pane can only ever be reported as a plain shell.
        return PaneStatus::PlainShell;
    };

    match illium_detect::identify_agent(system, Pid::from_u32(shell_pid)) {
        Some(identity) => {
            let screen_text = runtime.session.screen_text();
            let activity = illium_detect::classify_activity(&screen_text);
            PaneStatus::Agent(identity.class, activity)
        }
        None => PaneStatus::PlainShell,
    }
}

/// The next poll interval for a pane just classified as `status`, per
/// README "Poll cadence": actively `Working` panes poll fast, everything
/// else (idle, waiting on approval, done, or no agent detected at all)
/// polls slow -- none of those states change on their own between polls,
/// so there is no benefit to checking them often.
fn interval_for(status: &PaneStatus, state: &ServerState) -> Duration {
    match status {
        PaneStatus::Agent(_, illium_core::AgentActivity::Working) => {
            state.detection_config.working_poll_interval
        }
        _ => state.detection_config.idle_poll_interval,
    }
}

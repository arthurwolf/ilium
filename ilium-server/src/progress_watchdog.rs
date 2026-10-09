//! Periodic reconciliation of progress monitors against their agents.
//!
//! An agent that parks on `ilium progress` is only woken by a message Ilium
//! types into its composer. Every path that can end without that message
//! would leave the agent waiting forever, so this module is the last line of
//! defence behind the event-driven paths:
//!
//! * a nonterminal monitor whose coordinator task is gone (panic, abort, a
//!   lost restore) is converted into a *failed* monitor, which the sidebar
//!   shows and which is then delivered like any other failure;
//! * a settled outcome (task done/error or monitor failed) that is not in the
//!   agent's composer, and has no task working on it, is delivered again --
//!   including after an agent exit or restart turned a queued result into
//!   `NotDeliverable`, or a server restart left an attempt `Uncertain`.
//!
//! Delivery stays at-least-once: a re-sent result is marked as a possible
//! duplicate, because an agent that never sees a result is the worse failure.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use ilium_core::{NodeId, PaneProgress, ProgressMonitorHealth};
use ilium_ipc::ServerEvent;
use tokio::task::JoinHandle;

use crate::pane::{PaneResource, ProgressReconcileAction, TerminalPaneRuntime};
use crate::state::ServerState;

/// How often every pane is reconciled. Cheap: one read-guard pass.
const RECONCILE_INTERVAL: Duration = Duration::from_secs(20);
/// Redelivery attempts per monitor generation before the reconciler stops
/// retrying and leaves the failure visible in the sidebar.
const MAXIMUM_REDELIVERY_ATTEMPTS: u32 = 5;

/// Failure text for a monitor whose observation task vanished.
pub(crate) const OBSERVATION_STOPPED_UNEXPECTEDLY: &str = "Ilium stopped observing this job unexpectedly (its observation task ended); the task outcome is unknown";
/// Failure text for a monitor stopped by the global progress kill switch.
pub(crate) const OBSERVATION_DISABLED: &str =
    "progress monitoring was disabled in Ilium settings; the task outcome is unknown";

/// Starts the reconciler. Its handle is owned and aborted by
/// `ilium_server::run` during server shutdown.
pub(crate) fn spawn(state: Arc<ServerState>) -> JoinHandle<()> {
    tokio::spawn(run(state))
}

async fn run(state: Arc<ServerState>) {
    let mut attempts: HashMap<(NodeId, u64), u32> = HashMap::new();
    loop {
        tokio::time::sleep(RECONCILE_INTERVAL).await;
        if !state.is_progress_monitor_enabled() {
            continue;
        }
        reconcile(&state, &mut attempts).await;
    }
}

/// One reconciliation pass over every terminal pane.
pub(crate) async fn reconcile(
    state: &Arc<ServerState>,
    attempts: &mut HashMap<(NodeId, u64), u32>,
) {
    let actions: Vec<(NodeId, ProgressReconcileAction)> = {
        let panes = state.panes.read().await;
        panes
            .iter()
            .filter_map(|(pane_id, resource)| match resource {
                PaneResource::Terminal(runtime) => match runtime.progress_reconcile_action() {
                    ProgressReconcileAction::None => None,
                    action => Some((*pane_id, action)),
                },
                _ => None,
            })
            .collect()
    };
    // Generations that no longer exist cannot be retried; forget them.
    attempts.retain(|(pane_id, _), _| actions.iter().any(|(candidate, _)| candidate == pane_id));
    for (pane_id, action) in actions {
        match action {
            ProgressReconcileAction::None => {}
            ProgressReconcileAction::ObservationStopped { monitor_id } => {
                stop_observation(state, pane_id, monitor_id, OBSERVATION_STOPPED_UNEXPECTEDLY)
                    .await;
            }
            ProgressReconcileAction::Redeliver { monitor_id, .. } => {
                let attempt = attempts.entry((pane_id, monitor_id)).or_insert(0);
                if *attempt >= MAXIMUM_REDELIVERY_ATTEMPTS {
                    continue;
                }
                *attempt += 1;
                redeliver(state, pane_id, monitor_id).await;
            }
        }
    }
}

/// Builds the failed presentation of a monitor that can no longer observe.
fn failed_progress(progress: &PaneProgress, reason: &str) -> Option<PaneProgress> {
    let mut failed = progress.clone();
    failed.monitor_health = ProgressMonitorHealth::Failed {
        consecutive_failures: crate::progress_monitor::MAXIMUM_CONSECUTIVE_OBSERVATION_FAILURES,
        last_error: reason.to_string(),
    };
    match failed.validate() {
        Ok(()) => Some(failed),
        Err(error) => {
            tracing::warn!(%error, "failed-monitor evidence did not validate");
            None
        }
    }
}

/// Converts a nonterminal monitor into sticky failed evidence in place, stopping
/// its tasks. The caller holds the tree and pane registry write guards. Returns
/// the new presentation to broadcast, or `None` when nothing changed.
pub(crate) fn mark_observation_stopped(
    tree: &mut ilium_core::Tree,
    pane_id: NodeId,
    runtime: &mut TerminalPaneRuntime,
    reason: &str,
) -> Option<PaneProgress> {
    let monitor = runtime.progress_monitor.as_ref()?;
    let monitor_id = monitor.monitor_id;
    if monitor.latest_progress.is_terminal() || monitor.latest_progress.monitor_health.is_failed() {
        return None;
    }
    let failed = failed_progress(&monitor.latest_progress, reason)?;
    runtime.stop_progress_tasks_preserving_state();
    if !runtime.update_progress_monitor_progress(monitor_id, failed.clone()) {
        return None;
    }
    if let Err(error) = tree.set_pane_progress(pane_id, Some(failed.clone())) {
        tracing::warn!(pane_id = pane_id.0, monitor_id, %error, "could not present stopped monitor");
    }
    Some(failed)
}

async fn stop_observation(
    state: &Arc<ServerState>,
    pane_id: NodeId,
    monitor_id: u64,
    reason: &str,
) {
    let failed = {
        let mut tree = state.tree.write().await;
        let mut panes = state.panes.write().await;
        let Some(PaneResource::Terminal(runtime)) = panes.get_mut(&pane_id) else {
            return;
        };
        // Re-evaluate under the write guard: the monitor may have settled or
        // been replaced since the read-guard pass.
        if runtime.progress_reconcile_action()
            != (ProgressReconcileAction::ObservationStopped { monitor_id })
        {
            return;
        }
        let Some(failed) = mark_observation_stopped(&mut tree, pane_id, runtime, reason) else {
            return;
        };
        runtime.claim_progress_outcome_notification(monitor_id);
        failed
    };
    tracing::warn!(
        pane_id = pane_id.0,
        monitor_id,
        "progress monitor lost its observation task; marked failed so its agent is told"
    );
    state.broadcast(ServerEvent::PaneProgressChanged {
        pane_id,
        progress: Some(failed.clone()),
    });
    state.request_snapshot_save();
    crate::ipc::handlers::alert_task_outcome(state, pane_id, &failed).await;
    // The next pass delivers the failure to the agent.
}

async fn redeliver(state: &Arc<ServerState>, pane_id: NodeId, monitor_id: u64) {
    let mut panes = state.panes.write().await;
    let Some(PaneResource::Terminal(runtime)) = panes.get_mut(&pane_id) else {
        return;
    };
    let ProgressReconcileAction::Redeliver {
        monitor_id: current_monitor_id,
        possible_duplicate,
    } = runtime.progress_reconcile_action()
    else {
        return;
    };
    if current_monitor_id != monitor_id {
        return;
    }
    let Some(progress) = runtime
        .progress_monitor
        .as_ref()
        .map(|monitor| monitor.latest_progress.clone())
    else {
        return;
    };
    let message = crate::agent_delivery::settled_result_message(&progress, possible_duplicate);
    runtime.requeue_progress_delivery();
    let delivery_state = Arc::clone(state);
    let task = tokio::spawn(async move {
        if let Err(error) =
            crate::agent_delivery::deliver_result(delivery_state, pane_id, monitor_id, message)
                .await
        {
            tracing::warn!(pane_id = pane_id.0, monitor_id, %error, "reconciled progress delivery stopped");
        }
    });
    runtime.set_progress_delivery_task(task);
    drop(panes);
    tracing::info!(
        pane_id = pane_id.0,
        monitor_id,
        possible_duplicate,
        "redelivering a settled progress outcome the agent has not received"
    );
    state.request_snapshot_save();
}

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
            .flat_map(|(pane_id, resource)| match resource {
                PaneResource::Terminal(runtime) => runtime
                    .progress_monitor_ids()
                    .into_iter()
                    .filter_map(
                        |monitor_id| match runtime.progress_reconcile_action(monitor_id) {
                            ProgressReconcileAction::None => None,
                            action => Some((*pane_id, action)),
                        },
                    )
                    .collect::<Vec<_>>(),
                _ => Vec::new(),
            })
            .collect()
    };
    // Monitors that no longer need work cannot be retried; forget them.
    attempts.retain(|(pane_id, monitor_id), _| {
        actions.iter().any(|(candidate, action)| {
            candidate == pane_id
                && matches!(action, ProgressReconcileAction::Redeliver { monitor_id: id, .. } if id == monitor_id)
        })
    });
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

/// Converts one nonterminal monitor into sticky failed evidence in place,
/// stopping its tasks. The caller holds the tree and pane registry write
/// guards. Returns the failed report, or `None` when nothing changed.
pub(crate) fn mark_observation_stopped(
    tree: &mut ilium_core::Tree,
    pane_id: NodeId,
    runtime: &mut TerminalPaneRuntime,
    monitor_id: u64,
    reason: &str,
) -> Option<PaneProgress> {
    let monitor = runtime.progress_monitor(monitor_id)?;
    if monitor.is_settled() {
        return None;
    }
    let failed = failed_progress(&monitor.latest_progress, reason)?;
    runtime.stop_progress_monitor_tasks(monitor_id);
    if !runtime.update_progress_monitor_progress(monitor_id, failed.clone()) {
        return None;
    }
    if let Err(error) = tree.upsert_pane_progress(pane_id, failed.clone()) {
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
        if runtime.progress_reconcile_action(monitor_id)
            != (ProgressReconcileAction::ObservationStopped { monitor_id })
        {
            return;
        }
        let Some(failed) =
            mark_observation_stopped(&mut tree, pane_id, runtime, monitor_id, reason)
        else {
            return;
        };
        runtime.claim_progress_outcome_notification(monitor_id);
        (failed, runtime.progress_reports())
    };
    let (failed, progress_monitors) = failed;
    tracing::warn!(
        pane_id = pane_id.0,
        monitor_id,
        "progress monitor lost its observation task; marked failed so its agent is told"
    );
    state.broadcast(ServerEvent::PaneProgressChanged {
        pane_id,
        progress_monitors,
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
        possible_duplicate, ..
    } = runtime.progress_reconcile_action(monitor_id)
    else {
        return;
    };
    let Some(progress) = runtime
        .progress_monitor(monitor_id)
        .map(|monitor| monitor.latest_progress.clone())
    else {
        return;
    };
    let message = crate::agent_delivery::settled_result_message(&progress, possible_duplicate);
    runtime.requeue_progress_delivery(monitor_id);
    let delivery_state = Arc::clone(state);
    let task = tokio::spawn(async move {
        if let Err(error) =
            crate::agent_delivery::deliver_result(delivery_state, pane_id, monitor_id, message)
                .await
        {
            tracing::warn!(pane_id = pane_id.0, monitor_id, %error, "reconciled progress delivery stopped");
        }
    });
    runtime.set_progress_delivery_task(monitor_id, task);
    drop(panes);
    tracing::info!(
        pane_id = pane_id.0,
        monitor_id,
        possible_duplicate,
        "redelivering a settled progress outcome the agent has not received"
    );
    state.request_snapshot_save();
}

#[cfg(all(test, unix))]
mod tests {
    use std::collections::HashMap;
    use std::time::Duration;

    use ilium_core::{
        AgentClass, NodeId, ProgressMonitorHealth, ProgressTaskReport, ProgressTaskStatus,
    };

    use super::*;
    use crate::pane::{PaneResource, ProgressDeliveryState, TerminalOrigin};
    use crate::progress_monitor::ProgressMonitorRegistration;
    use crate::state::{ServerState, ServerStateOptions};

    const MONITOR_ID: u64 = 7;

    fn progress(status: ProgressTaskStatus, percent: f32) -> PaneProgress {
        let report = ProgressTaskReport::new(
            "job".to_string(),
            status,
            percent,
            String::new(),
            String::new(),
            (status == ProgressTaskStatus::Error).then(|| "failed".to_string()),
        )
        .expect("valid report");
        PaneProgress::new(MONITOR_ID, report, 1).expect("valid progress")
    }

    /// A real terminal runtime over `cat`, the same fixture the pane tests use.
    fn runtime() -> TerminalPaneRuntime {
        let directory = tempfile::tempdir().expect("isolated directory");
        let session = ilium_pty::PtySession::spawn(
            ilium_pty::PtyCommand::new("/bin/sh", directory.path(), 24, 80)
                .arg("-c")
                .arg("exec cat"),
        )
        .expect("fixture PTY");
        TerminalPaneRuntime::new(
            session,
            TerminalOrigin::PlainShell,
            None,
            Duration::from_secs(1),
        )
    }

    fn install(runtime: &mut TerminalPaneRuntime, status: ProgressTaskStatus, percent: f32) {
        runtime
            .install_progress_monitor(ProgressMonitorRegistration {
                monitor_id: MONITOR_ID,
                pane_id: NodeId(1),
                command: "/bin/true".to_string(),
                interval: Duration::from_secs(5),
                initial_progress: progress(status, percent),
            })
            .expect("accepted monitor");
    }

    fn set_delivery(runtime: &mut TerminalPaneRuntime, delivery: ProgressDeliveryState) {
        runtime
            .progress_monitor_mut(MONITOR_ID)
            .expect("installed monitor")
            .result_delivery = delivery;
    }

    fn finish(mut runtime: TerminalPaneRuntime) {
        runtime.session.kill().expect("close fixture");
    }

    #[test]
    fn running_monitor_without_observation_task_is_stopped() {
        let mut runtime = runtime();
        install(&mut runtime, ProgressTaskStatus::Running, 40.0);
        assert_eq!(
            runtime.progress_reconcile_action(MONITOR_ID),
            ProgressReconcileAction::ObservationStopped {
                monitor_id: MONITOR_ID
            }
        );
        finish(runtime);
    }

    #[tokio::test]
    async fn running_monitor_with_live_observation_task_is_left_alone() {
        let mut runtime = runtime();
        install(&mut runtime, ProgressTaskStatus::Running, 40.0);
        runtime.set_progress_monitor_task(MONITOR_ID, tokio::spawn(std::future::pending::<()>()));
        assert_eq!(
            runtime.progress_reconcile_action(MONITOR_ID),
            ProgressReconcileAction::None
        );
        runtime.cancel_progress_delivery_task(MONITOR_ID);
        finish(runtime);
    }

    #[tokio::test]
    async fn settled_outcome_not_yet_in_composer_is_redelivered_once() {
        let mut runtime = runtime();
        runtime.detected_agent_class = Some(AgentClass::Codex);
        install(&mut runtime, ProgressTaskStatus::Done, 100.0);
        set_delivery(&mut runtime, ProgressDeliveryState::NotQueued);
        assert_eq!(
            runtime.progress_reconcile_action(MONITOR_ID),
            ProgressReconcileAction::Redeliver {
                monitor_id: MONITOR_ID,
                possible_duplicate: false,
            }
        );
        finish(runtime);
    }

    #[tokio::test]
    async fn uncertain_and_attempted_outcomes_are_redelivered_as_possible_duplicates() {
        for delivery in [
            ProgressDeliveryState::Attempted,
            ProgressDeliveryState::Uncertain,
        ] {
            let mut runtime = runtime();
            runtime.detected_agent_class = Some(AgentClass::Codex);
            install(&mut runtime, ProgressTaskStatus::Error, 50.0);
            set_delivery(&mut runtime, delivery);
            assert_eq!(
                runtime.progress_reconcile_action(MONITOR_ID),
                ProgressReconcileAction::Redeliver {
                    monitor_id: MONITOR_ID,
                    possible_duplicate: true,
                },
                "{delivery:?}"
            );
            finish(runtime);
        }
    }

    #[tokio::test]
    async fn delivered_outcome_is_never_redelivered() {
        for delivery in [
            ProgressDeliveryState::DeliveredToPty,
            ProgressDeliveryState::CollectedByWaiter,
        ] {
            let mut runtime = runtime();
            runtime.detected_agent_class = Some(AgentClass::Codex);
            install(&mut runtime, ProgressTaskStatus::Done, 100.0);
            set_delivery(&mut runtime, delivery);
            assert_eq!(
                runtime.progress_reconcile_action(MONITOR_ID),
                ProgressReconcileAction::None,
                "{delivery:?}"
            );
            finish(runtime);
        }
    }

    #[tokio::test]
    async fn outcome_without_supported_composer_is_not_redelivered() {
        let mut runtime = runtime();
        install(&mut runtime, ProgressTaskStatus::Done, 100.0);
        set_delivery(&mut runtime, ProgressDeliveryState::NotQueued);
        assert_eq!(
            runtime.progress_reconcile_action(MONITOR_ID),
            ProgressReconcileAction::None
        );
        finish(runtime);
    }

    #[tokio::test]
    async fn outcome_with_live_delivery_task_is_left_alone() {
        let mut runtime = runtime();
        runtime.detected_agent_class = Some(AgentClass::Codex);
        install(&mut runtime, ProgressTaskStatus::Done, 100.0);
        set_delivery(&mut runtime, ProgressDeliveryState::Queued);
        runtime.set_progress_delivery_task(MONITOR_ID, tokio::spawn(std::future::pending::<()>()));
        assert_eq!(
            runtime.progress_reconcile_action(MONITOR_ID),
            ProgressReconcileAction::None
        );
        runtime.cancel_progress_delivery_task(MONITOR_ID);
        finish(runtime);
    }

    #[test]
    fn failed_evidence_is_sticky_failed_health_with_the_reason() {
        let failed = failed_progress(&progress(ProgressTaskStatus::Running, 40.0), "lost task")
            .expect("failed evidence validates");
        assert!(failed.monitor_health.is_failed());
        assert!(matches!(
            &failed.monitor_health,
            ProgressMonitorHealth::Failed { last_error, .. } if last_error == "lost task"
        ));
        assert_eq!(failed.monitor_id, MONITOR_ID);
    }

    fn server_state(directory: &tempfile::TempDir) -> ServerState {
        let (sound_requests, _playback_task) = crate::sounds::spawn(
            Arc::new(crate::NoopSoundPlayer),
            crate::execution::test_general_client(),
        );
        ServerState::new(ServerStateOptions {
            session_name: "watchdog-test".to_string(),
            session_cwd: directory.path().to_path_buf(),
            home_dir: directory.path().to_path_buf(),
            snapshot_path: directory.path().join("watchdog-test.snapshot.json"),
            socket_path: directory.path().join("watchdog-test.sock"),
            detection_config: crate::config::DetectionConfig::default(),
            notifications_config: crate::config::NotificationsConfig::default(),
            sound_settings: crate::sounds::test_settings(ilium_sound::SoundSettings::default()),
            sound_requests,
            custom_signatures: Vec::new(),
            agent_debug_menu_enabled: false,
            progress_monitor_enabled: true,
        })
    }

    /// Full pass over a server registry: a monitor whose observation task
    /// vanished becomes sticky failed evidence, and a second pass is a no-op.
    #[tokio::test]
    async fn reconcile_fails_a_monitor_whose_observation_task_vanished() {
        let directory = tempfile::tempdir().expect("tempdir");
        let state = Arc::new(server_state(&directory));
        let mut runtime = runtime();
        install(&mut runtime, ProgressTaskStatus::Running, 40.0);
        state
            .panes
            .write()
            .await
            .insert(NodeId(1), PaneResource::Terminal(Box::new(runtime)));
        let mut attempts = HashMap::new();

        reconcile(&state, &mut attempts).await;
        let first = failed_monitor_reason(&state).await;
        assert_eq!(first.as_deref(), Some(OBSERVATION_STOPPED_UNEXPECTEDLY));

        reconcile(&state, &mut attempts).await;
        assert_eq!(failed_monitor_reason(&state).await, first);
    }

    async fn failed_monitor_reason(state: &ServerState) -> Option<String> {
        let panes = state.panes.read().await;
        let Some(PaneResource::Terminal(runtime)) = panes.get(&NodeId(1)) else {
            return None;
        };
        let monitor = runtime.progress_monitors().next()?;
        match &monitor.latest_progress.monitor_health {
            ProgressMonitorHealth::Failed { last_error, .. } => Some(last_error.clone()),
            ProgressMonitorHealth::Healthy | ProgressMonitorHealth::Degraded { .. } => None,
        }
    }
}

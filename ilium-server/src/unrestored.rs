//! Saved panes that could not start during restore.
//!
//! A failed start never removes a pane: the pane keeps its tree node and is
//! registered as [`PaneResource::Unrestored`], so every later snapshot still
//! contains its saved command. One owned task retries these panes with
//! backoff until each one starts or the user closes it.

use std::sync::{Arc, Weak};
use std::time::Duration;

use ilium_agent_debug::{
    AgentDebugEventDraft, AgentDebugEventKind, AgentDebugField, AgentDebugSeverity,
    AgentDebugSource,
};
use ilium_core::NodeId;

use crate::pane::{PaneResource, PaneSnapshotKind};
use crate::persistence::PersistedProgressMonitor;
use crate::state::ServerState;
use crate::SnapshotPaneStart;

/// Delay before each retry round; the last value repeats forever.
const RETRY_DELAYS: [Duration; 6] = [
    Duration::from_secs(5),
    Duration::from_secs(15),
    Duration::from_secs(30),
    Duration::from_secs(60),
    Duration::from_secs(120),
    Duration::from_secs(300),
];

/// How many failed pane IDs a user-facing summary names before eliding.
const SUMMARY_PANE_LIMIT: usize = 8;

/// Records a failed start on the unrestored resource and in the journal.
pub(crate) async fn record_start_failure(state: &Arc<ServerState>, pane_id: NodeId, reason: &str) {
    let attempts = {
        let mut panes = state.panes.write().await;
        match panes.get_mut(&pane_id) {
            Some(PaneResource::Unrestored(unrestored)) => {
                unrestored.failure = Some(reason.to_string());
                unrestored.attempts = unrestored.attempts.saturating_add(1);
                unrestored.attempts
            }
            _ => return,
        }
    };
    let mut draft = AgentDebugEventDraft::information(
        AgentDebugEventKind::Error,
        "Pane could not start from the session snapshot; kept for retry",
    )
    .with_fields(vec![
        AgentDebugField::plain("reason", reason.to_string()),
        AgentDebugField::plain("attempts", attempts.to_string()),
    ]);
    draft.severity = AgentDebugSeverity::Error;
    let _ = crate::agent_debug::record(state, pane_id, AgentDebugSource::Persistence, draft).await;
    crate::lifecycle_log::record(
        state,
        crate::lifecycle_log::LifecycleEvent::PaneStartFailed {
            pane_id: pane_id.0,
            attempts,
            reason: reason.to_string(),
        },
    );
}

/// One user-facing line describing a partial restore.
pub(crate) fn failure_summary(pane_count: usize, failed: &[(NodeId, String)]) -> String {
    let mut named = failed
        .iter()
        .take(SUMMARY_PANE_LIMIT)
        .map(|(pane_id, _)| pane_id.0.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    if failed.len() > SUMMARY_PANE_LIMIT {
        named.push_str(&format!(" and {} more", failed.len() - SUMMARY_PANE_LIMIT));
    }
    let first_reason = failed
        .first()
        .map(|(_, reason)| reason.as_str())
        .unwrap_or("unknown");
    format!(
        "Restore incomplete: {} of {pane_count} saved panes could not start (panes {named}; \
         first error: {first_reason}). They are kept, saved, and retried automatically.",
        failed.len()
    )
}

/// Starts (or replaces) the single retry task for unrestored panes.
pub(crate) fn spawn_retry_task(state: &Arc<ServerState>) {
    // The task holds only a weak reference between rounds, so it never keeps
    // a shut-down server alive; shutdown also aborts it explicitly.
    let weak = Arc::downgrade(state);
    let handle = tokio::spawn(retry_loop(weak));
    state.set_unrestored_retry_task(handle);
}

async fn retry_loop(weak: Weak<ServerState>) {
    for round in 0_usize.. {
        let delay = RETRY_DELAYS[round.min(RETRY_DELAYS.len() - 1)];
        tokio::time::sleep(delay).await;
        let Some(state) = weak.upgrade() else {
            return;
        };
        if state.is_session_killed() || !retry_unrestored_panes(&state).await {
            return;
        }
    }
}

/// A placeholder due for a retry: its pane, saved kind, progress monitors and
/// last known terminal size.
type PendingRetry = (
    NodeId,
    PaneSnapshotKind,
    Vec<PersistedProgressMonitor>,
    Option<(u16, u16)>,
);

/// One retry round. Returns whether any pane is still unrestored.
pub(crate) async fn retry_unrestored_panes(state: &Arc<ServerState>) -> bool {
    let pending: Vec<PendingRetry> = {
        let panes = state.panes.read().await;
        panes
            .iter()
            .filter_map(|(pane_id, resource)| match resource {
                PaneResource::Unrestored(unrestored) if unrestored.failure.is_some() => Some((
                    *pane_id,
                    unrestored.kind.clone(),
                    unrestored.progress_monitors.clone(),
                    unrestored.size,
                )),
                _ => None,
            })
            .collect()
    };
    if pending.is_empty() {
        return false;
    }
    let mut started = Vec::new();
    let mut is_any_remaining = false;
    for (pane_id, kind, progress_monitors, size) in pending {
        match crate::start_snapshot_pane(state, pane_id, kind).await {
            SnapshotPaneStart::Started { missing_workspace } => {
                started.push(pane_id);
                apply_requested_size(state, pane_id, size).await;
                restore_progress_monitors(state, pane_id, progress_monitors, missing_workspace)
                    .await;
                let _ = crate::agent_debug::record(
                    state,
                    pane_id,
                    AgentDebugSource::Persistence,
                    AgentDebugEventDraft::information(
                        AgentDebugEventKind::PaneRestored,
                        "Pane restored from the session snapshot after a retry",
                    ),
                )
                .await;
                crate::lifecycle_log::record(
                    state,
                    crate::lifecycle_log::LifecycleEvent::PaneStartRetried { pane_id: pane_id.0 },
                );
            }
            SnapshotPaneStart::Removed => {}
            SnapshotPaneStart::Failed(reason) => {
                tracing::warn!(pane_id = pane_id.0, %reason, "unrestored pane retry failed");
                record_start_failure(state, pane_id, &reason).await;
                is_any_remaining = true;
            }
        }
    }
    if !started.is_empty() {
        let names = started
            .iter()
            .map(|pane_id| pane_id.0.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        state.broadcast(ilium_ipc::ServerEvent::Error {
            message: format!("Restored previously failed panes {names}."),
        });
        crate::ipc::handlers::broadcast_and_persist(state).await;
    }
    is_any_remaining
}

async fn apply_requested_size(state: &Arc<ServerState>, pane_id: NodeId, size: Option<(u16, u16)>) {
    let Some((rows, cols)) = size else {
        return;
    };
    let input = match state.panes.read().await.get(&pane_id) {
        Some(PaneResource::Terminal(runtime)) => runtime.session.input_handle(),
        _ => return,
    };
    match input.resize(rows, cols) {
        Ok(receipt) => {
            if let Err(error) = receipt.wait().await {
                tracing::warn!(pane_id = pane_id.0, %error, "restored pane resize failed");
            }
        }
        Err(error) => tracing::warn!(pane_id = pane_id.0, %error, "restored pane resize refused"),
    }
}

async fn restore_progress_monitors(
    state: &Arc<ServerState>,
    pane_id: NodeId,
    progress_monitors: Vec<PersistedProgressMonitor>,
    missing_workspace: bool,
) {
    if progress_monitors.is_empty() {
        return;
    }
    if missing_workspace {
        if let Some(PaneResource::Terminal(runtime)) = state.panes.write().await.get_mut(&pane_id) {
            runtime.deferred_progress_monitors = progress_monitors;
        }
        return;
    }
    for progress_monitor in progress_monitors {
        if let Err(error) =
            crate::ipc::handlers::restore_persisted_progress_monitor(state, progress_monitor).await
        {
            tracing::warn!(pane_id = pane_id.0, %error, "retried pane progress monitor not restored");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failure_summary_names_panes_and_elides_long_lists() {
        let failed = (1..=10)
            .map(|id| (NodeId(id), "worker quota exhausted".to_string()))
            .collect::<Vec<_>>();
        let summary = failure_summary(44, &failed);
        assert!(summary.contains("10 of 44"), "{summary}");
        assert!(
            summary.contains("panes 1, 2, 3, 4, 5, 6, 7, 8 and 2 more"),
            "{summary}"
        );
        assert!(summary.contains("worker quota exhausted"), "{summary}");
        assert!(summary.contains("kept"), "{summary}");
    }
}

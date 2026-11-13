//! On-demand deterministic signals for active monitoring readers.
use crate::{pane::PaneResource, state::ServerState};
use ilium_core::{AgentActivity, NodeKind};
use std::collections::HashSet;
use std::sync::Arc;

pub(crate) async fn refresh(state: &Arc<ServerState>) {
    // One refresh owns the scan cursor; concurrent consumers share deduplication.
    let mut scan = state.monitoring_signal_refresh.lock().await;
    let selected = {
        let tree = state.tree.read().await;
        tree.panes()
            .filter_map(|pane| match &pane.kind {
                NodeKind::Pane { status, .. } => status.agent_state().map(|agent| {
                    (
                        pane.id,
                        agent.activity(),
                        ilium_control::pane_state_name(status),
                    )
                }),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let quiet = state
        .recent_activity
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .snapshot()
        .clone();
    let monitored: HashSet<_> = state
        .panes
        .read()
        .await
        .iter()
        .filter_map(|(pane, resource)| {
            let PaneResource::Terminal(runtime) = resource else {
                return None;
            };
            runtime
                .progress_monitors()
                .any(|monitor| {
                    matches!(
                        monitor.latest_progress.report.status,
                        ilium_core::ProgressTaskStatus::Running
                            | ilium_core::ProgressTaskStatus::NotStartedYet
                    )
                })
                .then_some(*pane)
        })
        .collect();
    let mut holding = HashSet::new();
    if let Some(service) = state.resources.get() {
        if let Ok(snapshot) = crate::resources::snapshot(state).await {
            if let ilium_ipc::ServerEvent::ResourceSnapshot { catalog, .. } = &*snapshot {
                for reservation in catalog
                    .resources
                    .values()
                    .flat_map(|resource| &resource.reservations)
                {
                    if reservation.state.occupied()
                        && reservation.agent.runtime_generation == service.incarnation
                    {
                        holding.insert(reservation.agent.pane_id);
                    }
                }
            }
        }
    }
    let count = selected.len();
    for offset in 0..count {
        let index = (*scan + offset) % count;
        let (pane, activity, name) = selected[index];
        let (elapsed, error) = {
            let mut log = state
                .events
                .monitoring
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            (
                log.observe_state(pane, name).unwrap_or(0),
                log.repeated_error(pane),
            )
        };
        // Bound transcript work to 32 panes per refresh; successive follow
        // deadlines rotate the scan so large sessions are covered completely.
        let message =
            if offset < 32 && matches!(activity, AgentActivity::Idle | AgentActivity::Done) {
                crate::monitoring_transcript::read_reply(state, pane, true).await
            } else {
                None
            };
        let current = {
            let tree = state.tree.read().await;
            tree.get(pane).is_some_and(|node| {
                matches!(&node.kind, NodeKind::Pane { status, .. }
                if ilium_control::pane_state_name(status) == name)
            })
        };
        if !current {
            continue;
        }
        let signals = ilium_control::signals::evaluate(&ilium_control::signals::SignalInput {
            activity,
            elapsed_seconds: elapsed,
            quiet_seconds: quiet.get(&pane).map_or(elapsed, |observation| {
                observation.observed_at.elapsed().as_secs().min(elapsed)
            }),
            latest_message: message.as_deref(),
            repeated_error: error.as_ref().map(|(text, count)| (text.as_str(), *count)),
            holding_resource: holding.contains(&pane),
            monitored: monitored.contains(&pane),
        });
        let changed = state
            .events
            .monitoring
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .changed_signals(pane, signals, offset < 32);
        for signal in changed {
            state
                .events
                .record_monitoring(Some(pane), signal.kind, &signal.summary);
        }
    }
    *scan = if count == 0 { 0 } else { (*scan + 32) % count };
}

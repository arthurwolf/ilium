//! Readiness-gated, generation-fenced progress lifecycle delivery.
//!
//! PTY writes prove only that bytes reached the terminal. This module records
//! exactly that boundary and never upgrades it to semantic acknowledgement.

use std::sync::Arc;
use std::time::Duration;

use ilium_core::{NodeId, PaneProgress, ProgressTaskStatus};
use ilium_ipc::PromptSubmissionSource;

use crate::ipc::handlers::{submit_terminal_body_locked, submit_terminal_text_locked};
use crate::pane::{PaneResource, ProgressDeliveryState};
use crate::state::ServerState;

const READINESS_RECHECK_INTERVAL: Duration = Duration::from_millis(100);
const MAXIMUM_DELIVERY_TEXT_BYTES: usize = 8 * 1024;

/// Shared safe boundary for the one-shot prompt attached to a new agent pane.
/// Unlike progress delivery it permits the provider-registry screen fallback
/// during the short window before process detection arrives.
pub(crate) async fn deliver_initial_prompt_when_ready(
    state: &ServerState,
    pane_id: NodeId,
    body: &[u8],
) -> Result<(), String> {
    let mut screen_changed = {
        let panes = state.panes.read().await;
        let Some(PaneResource::Terminal(runtime)) = panes.get(&pane_id) else {
            return Err("pane is no longer a terminal".to_string());
        };
        runtime.session.subscribe_screen_changed()
    };

    loop {
        let input_gate = {
            let panes = state.panes.read().await;
            let Some(PaneResource::Terminal(runtime)) = panes.get(&pane_id) else {
                return Err("pane closed before prompt delivery".to_string());
            };
            initial_prompt_is_ready(runtime).then(|| Arc::clone(&runtime.input_gate))
        };
        if let Some(input_gate) = input_gate {
            let _input_guard = input_gate.lock().await;
            let still_ready = {
                let panes = state.panes.read().await;
                let Some(PaneResource::Terminal(runtime)) = panes.get(&pane_id) else {
                    return Err("pane closed before prompt delivery".to_string());
                };
                Arc::ptr_eq(&input_gate, &runtime.input_gate) && initial_prompt_is_ready(runtime)
            };
            if still_ready {
                return submit_terminal_body_locked(
                    state,
                    pane_id,
                    body,
                    PromptSubmissionSource::InitialAgentPrompt,
                    &input_gate,
                )
                .await;
            }
        }

        tokio::select! {
            changed = screen_changed.changed() => {
                if changed.is_err() {
                    return Err("pane closed before prompt delivery".to_string());
                }
            }
            () = tokio::time::sleep(READINESS_RECHECK_INTERVAL) => {}
        }
    }
}

fn initial_prompt_is_ready(runtime: &crate::pane::TerminalPaneRuntime) -> bool {
    let screen = runtime.session.screen_snapshot();
    if let Some(agent_class) = runtime.detected_agent_class.as_ref() {
        return ilium_detect::is_agent_prompt_ready_at_cursor(
            agent_class,
            &screen.text,
            screen.cursor_position.0,
            screen.cursor_position.1,
            &screen.dimmed_cells,
        );
    }
    ilium_core::BuiltinAgentProvider::ALL
        .into_iter()
        .any(|provider| {
            use ilium_core::AgentProvider;
            ilium_detect::is_agent_prompt_ready_at_cursor(
                &provider.class(),
                &screen.text,
                screen.cursor_position.0,
                screen.cursor_position.1,
                &screen.dimmed_cells,
            )
        })
}

/// Delivers a terminal or monitor-failure notification at the next safe
/// composer boundary. Progress monitoring never touches the agent's `/goal`:
/// this notification is its only effect on the pane's input.
pub(crate) async fn deliver_result(
    state: Arc<ServerState>,
    pane_id: NodeId,
    monitor_id: u64,
    message: String,
) -> Result<(), String> {
    queue_result_delivery(&state, pane_id, monitor_id).await?;
    deliver_when_ready(
        &state,
        pane_id,
        monitor_id,
        &sanitize_delivery_text(&message),
    )
    .await
}

pub(crate) fn terminal_result_message(progress: &PaneProgress) -> String {
    let report = &progress.report;
    match report.status {
        ProgressTaskStatus::Done => format!(
            "Ilium progress monitor {} reports that {} completed successfully.\nFinal progress: 100%.\nStatus: {}.",
            progress.monitor_id,
            report.job_id,
            nonempty_status(&report.message)
        ),
        ProgressTaskStatus::Error => format!(
            "Ilium progress monitor {} reports that {} failed.\nFinal progress: {:.1}%.\nStatus: {}.\nError: {}.",
            progress.monitor_id,
            report.job_id,
            report.percent,
            nonempty_status(&report.message),
            report.error.as_deref().unwrap_or("the task reported an unspecified error")
        ),
        ProgressTaskStatus::NotStartedYet | ProgressTaskStatus::Running => format!(
            "Ilium progress monitor {} stopped before {} reached a terminal task status. The task outcome is unknown.\nLast progress: {:.1}%.\nStatus: {}.",
            progress.monitor_id,
            report.job_id,
            report.percent,
            nonempty_status(&report.message)
        ),
    }
}

pub(crate) fn monitor_failure_message(progress: &PaneProgress, error: &str) -> String {
    format!(
        "Ilium progress monitor {} could no longer observe {}. The task outcome is unknown.\nLast progress: {:.1}%.\nMonitor error: {}.",
        progress.monitor_id,
        progress.report.job_id,
        progress.report.percent,
        sanitize_delivery_text(error)
    )
}

async fn deliver_when_ready(
    state: &ServerState,
    pane_id: NodeId,
    monitor_id: u64,
    text: &str,
) -> Result<(), String> {
    let (mut screen_changed, effect_gate) = {
        let panes = state.panes.read().await;
        let Some(PaneResource::Terminal(runtime)) = panes.get(&pane_id) else {
            return Err("pane is no longer a terminal".to_string());
        };
        (
            runtime.session.subscribe_screen_changed(),
            Arc::clone(&runtime.progress_effect_gate),
        )
    };

    loop {
        if readiness_snapshot(state, pane_id, monitor_id).await {
            let _effect_guard = effect_gate.lock().await;
            let input_gate = {
                let panes = state.panes.read().await;
                let Some(PaneResource::Terminal(runtime)) = panes.get(&pane_id) else {
                    return Err("pane closed before progress delivery".to_string());
                };
                if !Arc::ptr_eq(&effect_gate, &runtime.progress_effect_gate)
                    || !result_delivery_is_queued(runtime, monitor_id)
                    || !runtime_has_ready_composer(runtime)
                {
                    continue;
                }
                Arc::clone(&runtime.input_gate)
            };
            let _input_guard = input_gate.lock().await;

            // Final validation while both the effect and pane-input gates are
            // held. Registration replacement/clear takes the effect gate;
            // every user/automated input takes the input gate.
            {
                let mut panes = state.panes.write().await;
                let Some(PaneResource::Terminal(runtime)) = panes.get_mut(&pane_id) else {
                    return Err("pane closed before progress delivery".to_string());
                };
                if !Arc::ptr_eq(&effect_gate, &runtime.progress_effect_gate)
                    || !Arc::ptr_eq(&input_gate, &runtime.input_gate)
                    || !result_delivery_is_queued(runtime, monitor_id)
                    || !runtime_has_ready_composer(runtime)
                {
                    continue;
                }
                set_result_delivery(runtime, ProgressDeliveryState::Attempted)?;
            }
            // The attempted state is the replay fence for this irreversible
            // PTY effect. It must be on disk before any bytes can reach the
            // agent; a debounced best-effort save would leave a crash window
            // where restore sees `Queued` and submits the same result twice.
            if let Err(error) = crate::persistence::await_snapshot_durability_barrier(state).await {
                rollback_delivery_attempt(state, pane_id, monitor_id).await;
                return Err(format!(
                    "could not persist progress delivery intent before PTY submission: {error}"
                ));
            }

            if let Err(error) = submit_terminal_text_locked(
                state,
                pane_id,
                text,
                PromptSubmissionSource::ProgressResult,
                &input_gate,
            )
            .await
            {
                mark_delivery_uncertain(state, pane_id, monitor_id).await;
                return Err(error);
            }

            {
                let mut panes = state.panes.write().await;
                let Some(PaneResource::Terminal(runtime)) = panes.get_mut(&pane_id) else {
                    return Err("pane closed after progress delivery".to_string());
                };
                if !runtime.is_current_progress_monitor(monitor_id) {
                    return Err("progress monitor changed during delivery".to_string());
                }
                set_result_delivery(runtime, ProgressDeliveryState::DeliveredToPty)?;
            }
            state.request_snapshot_save();
            return Ok(());
        }

        tokio::select! {
            changed = screen_changed.changed() => {
                if changed.is_err() {
                    return Err("pane closed before progress delivery".to_string());
                }
            }
            () = tokio::time::sleep(READINESS_RECHECK_INTERVAL) => {}
        }
    }
}

async fn readiness_snapshot(state: &ServerState, pane_id: NodeId, monitor_id: u64) -> bool {
    let panes = state.panes.read().await;
    let Some(PaneResource::Terminal(runtime)) = panes.get(&pane_id) else {
        return false;
    };
    result_delivery_is_queued(runtime, monitor_id) && runtime_has_ready_composer(runtime)
}

pub(crate) fn runtime_has_ready_composer(runtime: &crate::pane::TerminalPaneRuntime) -> bool {
    let Some(agent_class) = runtime.detected_agent_class.as_ref() else {
        return false;
    };
    let screen = runtime.session.screen_snapshot();
    ilium_detect::is_agent_prompt_ready_at_cursor(
        agent_class,
        &screen.text,
        screen.cursor_position.0,
        screen.cursor_position.1,
        &screen.dimmed_cells,
    )
}

fn result_delivery_is_queued(runtime: &crate::pane::TerminalPaneRuntime, monitor_id: u64) -> bool {
    runtime.is_current_progress_monitor(monitor_id)
        && runtime
            .progress_monitor
            .as_ref()
            .is_some_and(|monitor| matches!(monitor.result_delivery, ProgressDeliveryState::Queued))
}

fn set_result_delivery(
    runtime: &mut crate::pane::TerminalPaneRuntime,
    delivery: ProgressDeliveryState,
) -> Result<(), String> {
    let monitor = runtime
        .progress_monitor
        .as_mut()
        .ok_or_else(|| "progress monitor was cleared".to_string())?;
    monitor.result_delivery = delivery;
    Ok(())
}

/// Returns an attempt to its retryable in-memory state only when the durable
/// pre-effect barrier failed. No PTY bytes have been written on this path, so
/// restoring `Queued` is both honest and safe. The ordinary snapshot writer
/// may later persist this rollback; an older on-disk `Queued` state is
/// already equivalent and safe to retry after restart.
async fn rollback_delivery_attempt(state: &ServerState, pane_id: NodeId, monitor_id: u64) {
    let mut panes = state.panes.write().await;
    let Some(PaneResource::Terminal(runtime)) = panes.get_mut(&pane_id) else {
        return;
    };
    if !runtime.is_current_progress_monitor(monitor_id) {
        return;
    }
    let Some(monitor) = runtime.progress_monitor.as_mut() else {
        return;
    };
    if monitor.result_delivery == ProgressDeliveryState::Attempted {
        monitor.result_delivery = ProgressDeliveryState::Queued;
    }
    drop(panes);
    state.request_snapshot_save();
}

async fn mark_delivery_uncertain(state: &ServerState, pane_id: NodeId, monitor_id: u64) {
    let mut panes = state.panes.write().await;
    let Some(PaneResource::Terminal(runtime)) = panes.get_mut(&pane_id) else {
        return;
    };
    if !runtime.is_current_progress_monitor(monitor_id) {
        return;
    }
    let Some(monitor) = runtime.progress_monitor.as_mut() else {
        return;
    };
    monitor.result_delivery = ProgressDeliveryState::Uncertain;
    drop(panes);
    state.request_snapshot_save();
}

async fn queue_result_delivery(
    state: &ServerState,
    pane_id: NodeId,
    monitor_id: u64,
) -> Result<(), String> {
    let mut panes = state.panes.write().await;
    let Some(PaneResource::Terminal(runtime)) = panes.get_mut(&pane_id) else {
        return Err("pane closed before result delivery".to_string());
    };
    if !runtime.is_current_progress_monitor(monitor_id) {
        return Err(format!("progress monitor {monitor_id} is stale"));
    }
    let monitor = runtime
        .progress_monitor
        .as_mut()
        .expect("current monitor was checked above");
    match monitor.result_delivery {
        ProgressDeliveryState::NotQueued | ProgressDeliveryState::Queued => {
            monitor.result_delivery = ProgressDeliveryState::Queued;
        }
        ProgressDeliveryState::Attempted
        | ProgressDeliveryState::DeliveredToPty
        | ProgressDeliveryState::Uncertain => {
            return Err("progress result delivery was already attempted".to_string());
        }
    }
    drop(panes);
    state.request_snapshot_save();
    Ok(())
}

fn nonempty_status(message: &str) -> &str {
    if message.trim().is_empty() {
        "no additional status was reported"
    } else {
        message
    }
}

fn sanitize_delivery_text(text: &str) -> String {
    let mut sanitized = String::with_capacity(text.len().min(MAXIMUM_DELIVERY_TEXT_BYTES));
    for character in text.chars() {
        if sanitized.len() >= MAXIMUM_DELIVERY_TEXT_BYTES {
            break;
        }
        if (character == '\n' || character == '\t' || !character.is_control())
            && sanitized.len() + character.len_utf8() <= MAXIMUM_DELIVERY_TEXT_BYTES
        {
            sanitized.push(character);
        }
    }
    sanitized
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_core::{ProgressTaskReport, ProgressTaskStatus};

    fn progress(status: ProgressTaskStatus) -> PaneProgress {
        PaneProgress::new(
            17,
            ProgressTaskReport::new(
                "render-17".to_string(),
                status,
                42.5,
                "frame 425/1000".to_string(),
                (status == ProgressTaskStatus::Error).then(|| "renderer crashed".to_string()),
            )
            .unwrap(),
            1,
        )
        .unwrap()
    }

    #[test]
    fn terminal_messages_distinguish_task_error_from_unknown_monitor_outcome() {
        let task_error = terminal_result_message(&progress(ProgressTaskStatus::Error));
        assert!(task_error.contains("reports that render-17 failed"));
        assert!(task_error.contains("renderer crashed"));

        let monitor_error =
            monitor_failure_message(&progress(ProgressTaskStatus::Running), "probe timed out");
        assert!(monitor_error.contains("task outcome is unknown"));
        assert!(monitor_error.contains("probe timed out"));
    }

    #[test]
    fn task_controlled_delivery_text_drops_terminal_controls_and_is_bounded() {
        let source = format!(
            "ok\u{1b}[31m{}",
            "x".repeat(MAXIMUM_DELIVERY_TEXT_BYTES * 2)
        );
        let sanitized = sanitize_delivery_text(&source);
        assert!(!sanitized.contains('\u{1b}'));
        assert!(sanitized.len() <= MAXIMUM_DELIVERY_TEXT_BYTES);
    }
}

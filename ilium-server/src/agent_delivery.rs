//! Readiness-gated, generation-fenced progress lifecycle delivery.
//!
//! PTY writes prove only that bytes reached the terminal. This module records
//! exactly that boundary and never upgrades it to semantic acknowledgement.

use std::sync::Arc;
use std::time::Duration;

use ilium_core::{GoalState, NodeId, PaneProgress, ProgressTaskStatus};
use ilium_ipc::PromptSubmissionSource;

use crate::ipc::handlers::{submit_terminal_body_locked, submit_terminal_text_locked};
use crate::pane::{
    PaneResource, ProgressDeliveryState, ProgressGoalBinding, ProgressGoalResumeState,
};
use crate::state::ServerState;

const READINESS_RECHECK_INTERVAL: Duration = Duration::from_millis(100);
const GOAL_PAUSE_CONFIRMATION_TIMEOUT: Duration = Duration::from_secs(30);
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeliveryReceipt {
    /// Screen generation observed after the complete text + delayed Enter
    /// sequence reached the PTY. A subsequent command can require a newer
    /// generation to prove a fresh composer boundary.
    pub screen_generation: u64,
}

#[derive(Debug, Clone)]
enum DeliveryCondition {
    Result {
        monitor_id: u64,
        minimum_screen_generation: Option<u64>,
    },
    GoalPause(ProgressGoalBinding),
    GoalResume {
        binding: ProgressGoalBinding,
        minimum_screen_generation: u64,
    },
}

impl DeliveryCondition {
    fn monitor_id(&self) -> u64 {
        match self {
            Self::Result { monitor_id, .. } => *monitor_id,
            Self::GoalPause(binding) | Self::GoalResume { binding, .. } => binding.monitor_id,
        }
    }

    fn minimum_screen_generation(&self) -> Option<u64> {
        match self {
            Self::Result {
                minimum_screen_generation,
                ..
            } => *minimum_screen_generation,
            Self::GoalPause(_) => None,
            Self::GoalResume {
                minimum_screen_generation,
                ..
            } => Some(*minimum_screen_generation),
        }
    }
}

/// Queues and submits `/goal pause`, then owns the pause only after detection
/// observes `Paused` for the exact process/session/goal epoch captured by arm.
pub(crate) async fn pause_goal_for_monitor(
    state: Arc<ServerState>,
    pane_id: NodeId,
    binding: ProgressGoalBinding,
) -> Result<(), String> {
    deliver_when_ready(
        &state,
        pane_id,
        "/goal pause",
        PromptSubmissionSource::ProgressGoalPause,
        DeliveryCondition::GoalPause(binding.clone()),
    )
    .await?;

    {
        let mut panes = state.panes.write().await;
        let Some(PaneResource::Terminal(runtime)) = panes.get_mut(&pane_id) else {
            return Err("pane closed after goal-pause delivery".to_string());
        };
        if !binding_identity_still_matches(runtime, &binding)
            || !runtime.confirmed_goal_owner.as_ref().is_some_and(|owner| {
                owner.process_id == binding.process_id
                    && owner.agent_class == ilium_core::AgentClass::Codex
                    && matches!(owner.goal_state, GoalState::Active | GoalState::Paused)
            })
        {
            mark_goal_unsafe(
                runtime,
                &binding,
                "goal ownership changed during pause delivery",
            );
            return Err("goal ownership changed during pause delivery".to_string());
        }
        let Some(monitor) = runtime.progress_monitor.as_mut() else {
            return Err("progress monitor was cleared during goal-pause delivery".to_string());
        };
        monitor.goal_resume = ProgressGoalResumeState::PauseSubmitted(binding.clone());
    }
    state.request_snapshot_save();

    let confirmation = tokio::time::timeout(
        GOAL_PAUSE_CONFIRMATION_TIMEOUT,
        wait_for_owned_pause(&state, pane_id, &binding),
    )
    .await;
    match confirmation {
        Ok(result) => result,
        Err(_) => {
            mark_goal_unsafe_in_state(
                &state,
                pane_id,
                &binding,
                "Codex did not confirm the owned paused state in time",
            )
            .await;
            Err("Codex did not confirm the owned paused state in time".to_string())
        }
    }
}

/// Delivers the terminal result first. Only after a later fresh ready-composer
/// boundary does it resume an exact pause still owned by this monitor.
pub(crate) async fn deliver_result_then_resume(
    state: Arc<ServerState>,
    pane_id: NodeId,
    monitor_id: u64,
    message: String,
) -> Result<(), String> {
    // A very fast task may finish while the independently started goal-pause
    // transaction is still waiting for the current turn's composer. Do not
    // race the result past it and then miss the owned pause forever.
    wait_for_goal_pause_settlement(&state, pane_id, monitor_id).await?;
    queue_result_delivery(&state, pane_id, monitor_id).await?;
    let result_receipt = deliver_when_ready(
        &state,
        pane_id,
        &sanitize_delivery_text(&message),
        PromptSubmissionSource::ProgressResult,
        DeliveryCondition::Result {
            monitor_id,
            minimum_screen_generation: None,
        },
    )
    .await?;

    let binding = {
        let panes = state.panes.read().await;
        let Some(PaneResource::Terminal(runtime)) = panes.get(&pane_id) else {
            return Ok(());
        };
        let Some(monitor) = runtime.progress_monitor.as_ref() else {
            return Ok(());
        };
        match &monitor.goal_resume {
            ProgressGoalResumeState::OwnedPause(binding)
                if monitor.result_delivery == ProgressDeliveryState::DeliveredToPty =>
            {
                Some(binding.clone())
            }
            _ => None,
        }
    };
    let Some(binding) = binding else {
        return Ok(());
    };

    queue_goal_resume_delivery(&state, pane_id, &binding).await?;
    deliver_when_ready(
        &state,
        pane_id,
        "/goal resume",
        PromptSubmissionSource::ProgressGoalResume,
        DeliveryCondition::GoalResume {
            binding,
            minimum_screen_generation: result_receipt.screen_generation,
        },
    )
    .await?;
    Ok(())
}

async fn wait_for_goal_pause_settlement(
    state: &ServerState,
    pane_id: NodeId,
    monitor_id: u64,
) -> Result<(), String> {
    let wait = async {
        let mut screen_changed = {
            let panes = state.panes.read().await;
            let Some(PaneResource::Terminal(runtime)) = panes.get(&pane_id) else {
                return Err("pane closed before result delivery".to_string());
            };
            runtime.session.subscribe_screen_changed()
        };
        loop {
            let is_settled = {
                let panes = state.panes.read().await;
                let Some(PaneResource::Terminal(runtime)) = panes.get(&pane_id) else {
                    return Err("pane closed before result delivery".to_string());
                };
                if !runtime.is_current_progress_monitor(monitor_id) {
                    return Err(format!("progress monitor {monitor_id} is stale"));
                }
                let monitor = runtime
                    .progress_monitor
                    .as_ref()
                    .expect("current monitor was checked above");
                !matches!(
                    monitor.goal_resume,
                    ProgressGoalResumeState::Armed(_) | ProgressGoalResumeState::PauseSubmitted(_)
                )
            };
            if is_settled {
                return Ok(());
            }
            tokio::select! {
                changed = screen_changed.changed() => {
                    if changed.is_err() {
                        return Err("pane closed before result delivery".to_string());
                    }
                }
                () = tokio::time::sleep(READINESS_RECHECK_INTERVAL) => {}
            }
        }
    };

    match tokio::time::timeout(GOAL_PAUSE_CONFIRMATION_TIMEOUT, wait).await {
        Ok(result) => result,
        Err(_) => {
            let mut panes = state.panes.write().await;
            let Some(PaneResource::Terminal(runtime)) = panes.get_mut(&pane_id) else {
                return Err("pane closed before result delivery".to_string());
            };
            if !runtime.is_current_progress_monitor(monitor_id) {
                return Err(format!("progress monitor {monitor_id} is stale"));
            }
            let Some(monitor) = runtime.progress_monitor.as_mut() else {
                return Err("progress monitor was cleared before result delivery".to_string());
            };
            monitor.goal_resume = ProgressGoalResumeState::Unsafe(
                "goal pause did not settle before terminal result delivery".to_string(),
            );
            monitor.goal_resume_delivery = ProgressDeliveryState::Uncertain;
            drop(panes);
            state.request_snapshot_save();
            Ok(())
        }
    }
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
    text: &str,
    source: PromptSubmissionSource,
    condition: DeliveryCondition,
) -> Result<DeliveryReceipt, String> {
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
        if readiness_snapshot(state, pane_id, &condition).await {
            let _effect_guard = effect_gate.lock().await;
            let input_gate = {
                let panes = state.panes.read().await;
                let Some(PaneResource::Terminal(runtime)) = panes.get(&pane_id) else {
                    return Err("pane closed before progress delivery".to_string());
                };
                if !Arc::ptr_eq(&effect_gate, &runtime.progress_effect_gate)
                    || !delivery_condition_matches(runtime, &condition)
                    || !runtime_has_ready_composer(runtime, condition.minimum_screen_generation())
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
                    || !delivery_condition_matches(runtime, &condition)
                    || !runtime_has_ready_composer(runtime, condition.minimum_screen_generation())
                {
                    continue;
                }
                mark_delivery_attempted(runtime, &condition)?;
            }
            // The attempted state is the replay fence for this irreversible
            // PTY effect. It must be on disk before any bytes can reach the
            // agent; a debounced best-effort save would leave a crash window
            // where restore sees `Queued` and submits the same result or
            // `/goal resume` twice.
            if let Err(error) = crate::persistence::await_snapshot_durability_barrier(state).await {
                rollback_delivery_attempt(state, pane_id, &condition).await;
                return Err(format!(
                    "could not persist progress delivery intent before PTY submission: {error}"
                ));
            }

            if let Err(error) =
                submit_terminal_text_locked(state, pane_id, text, source, &input_gate).await
            {
                mark_delivery_uncertain(state, pane_id, &condition).await;
                return Err(error);
            }

            let screen_generation = {
                let mut panes = state.panes.write().await;
                let Some(PaneResource::Terminal(runtime)) = panes.get_mut(&pane_id) else {
                    return Err("pane closed after progress delivery".to_string());
                };
                if !delivery_condition_still_current(runtime, &condition) {
                    return Err("progress monitor changed during delivery".to_string());
                }
                mark_delivery_completed(runtime, &condition)?;
                runtime.session.screen_generation()
            };
            state.request_snapshot_save();
            return Ok(DeliveryReceipt { screen_generation });
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

async fn readiness_snapshot(
    state: &ServerState,
    pane_id: NodeId,
    condition: &DeliveryCondition,
) -> bool {
    let panes = state.panes.read().await;
    let Some(PaneResource::Terminal(runtime)) = panes.get(&pane_id) else {
        return false;
    };
    delivery_condition_matches(runtime, condition)
        && runtime_has_ready_composer(runtime, condition.minimum_screen_generation())
}

fn runtime_has_ready_composer(
    runtime: &crate::pane::TerminalPaneRuntime,
    minimum_screen_generation: Option<u64>,
) -> bool {
    let Some(agent_class) = runtime.detected_agent_class.as_ref() else {
        return false;
    };
    let screen = runtime.session.screen_snapshot();
    if minimum_screen_generation.is_some_and(|minimum| screen.generation <= minimum) {
        return false;
    }
    ilium_detect::is_agent_prompt_ready_at_cursor(
        agent_class,
        &screen.text,
        screen.cursor_position.0,
        screen.cursor_position.1,
        &screen.dimmed_cells,
    )
}

fn delivery_condition_matches(
    runtime: &crate::pane::TerminalPaneRuntime,
    condition: &DeliveryCondition,
) -> bool {
    if !runtime.is_current_progress_monitor(condition.monitor_id()) {
        return false;
    }
    let Some(monitor) = runtime.progress_monitor.as_ref() else {
        return false;
    };
    match condition {
        DeliveryCondition::Result { .. } => {
            matches!(monitor.result_delivery, ProgressDeliveryState::Queued)
        }
        DeliveryCondition::GoalPause(binding) => {
            matches!(&monitor.goal_resume, ProgressGoalResumeState::Armed(current) if current == binding)
                && runtime.progress_goal_binding_matches(binding, GoalState::Active)
        }
        DeliveryCondition::GoalResume { binding, .. } => {
            monitor.result_delivery == ProgressDeliveryState::DeliveredToPty
                && monitor.goal_resume_delivery == ProgressDeliveryState::Queued
                && matches!(&monitor.goal_resume, ProgressGoalResumeState::OwnedPause(current) if current == binding)
                && runtime.progress_goal_binding_matches(binding, GoalState::Paused)
        }
    }
}

fn delivery_condition_still_current(
    runtime: &crate::pane::TerminalPaneRuntime,
    condition: &DeliveryCondition,
) -> bool {
    if !runtime.is_current_progress_monitor(condition.monitor_id()) {
        return false;
    }
    match condition {
        DeliveryCondition::Result { .. } => true,
        DeliveryCondition::GoalPause(binding) => {
            runtime.progress_goal_binding_matches(binding, GoalState::Active)
                || (binding_identity_still_matches(runtime, binding)
                    && runtime.confirmed_goal_owner.as_ref().is_some_and(|owner| {
                        owner.process_id == binding.process_id
                            && owner.agent_class == ilium_core::AgentClass::Codex
                            && owner.goal_state == GoalState::Paused
                    }))
        }
        DeliveryCondition::GoalResume { binding, .. } => {
            binding_identity_still_matches(runtime, binding)
                && runtime.confirmed_goal_owner.as_ref().is_some_and(|owner| {
                    owner.process_id == binding.process_id
                        && owner.agent_class == ilium_core::AgentClass::Codex
                        && matches!(owner.goal_state, GoalState::Paused | GoalState::Active)
                })
        }
    }
}

fn mark_delivery_attempted(
    runtime: &mut crate::pane::TerminalPaneRuntime,
    condition: &DeliveryCondition,
) -> Result<(), String> {
    let monitor = runtime
        .progress_monitor
        .as_mut()
        .ok_or_else(|| "progress monitor was cleared".to_string())?;
    match condition {
        DeliveryCondition::Result { .. } => {
            monitor.result_delivery = ProgressDeliveryState::Attempted;
        }
        DeliveryCondition::GoalPause(binding) => {
            monitor.goal_resume = ProgressGoalResumeState::PauseSubmitted(binding.clone());
        }
        DeliveryCondition::GoalResume { .. } => {
            monitor.goal_resume_delivery = ProgressDeliveryState::Attempted;
        }
    }
    Ok(())
}

/// Returns an attempt to its retryable in-memory state only when the durable
/// pre-effect barrier failed. No PTY bytes have been written on this path, so
/// restoring `Queued`/`Armed` is both honest and safe. The ordinary snapshot
/// writer may later persist this rollback; an older on-disk `Queued` state is
/// already equivalent and safe to retry after restart.
async fn rollback_delivery_attempt(
    state: &ServerState,
    pane_id: NodeId,
    condition: &DeliveryCondition,
) {
    let mut panes = state.panes.write().await;
    let Some(PaneResource::Terminal(runtime)) = panes.get_mut(&pane_id) else {
        return;
    };
    if !runtime.is_current_progress_monitor(condition.monitor_id()) {
        return;
    }
    let Some(monitor) = runtime.progress_monitor.as_mut() else {
        return;
    };
    match condition {
        DeliveryCondition::Result { .. } => {
            if monitor.result_delivery == ProgressDeliveryState::Attempted {
                monitor.result_delivery = ProgressDeliveryState::Queued;
            }
        }
        DeliveryCondition::GoalPause(binding) => {
            if matches!(
                &monitor.goal_resume,
                ProgressGoalResumeState::PauseSubmitted(current) if current == binding
            ) {
                monitor.goal_resume = ProgressGoalResumeState::Armed(binding.clone());
            }
        }
        DeliveryCondition::GoalResume { binding, .. } => {
            if monitor.goal_resume_delivery == ProgressDeliveryState::Attempted
                && matches!(
                    &monitor.goal_resume,
                    ProgressGoalResumeState::OwnedPause(current) if current == binding
                )
            {
                monitor.goal_resume_delivery = ProgressDeliveryState::Queued;
            }
        }
    }
    drop(panes);
    state.request_snapshot_save();
}

fn mark_delivery_completed(
    runtime: &mut crate::pane::TerminalPaneRuntime,
    condition: &DeliveryCondition,
) -> Result<(), String> {
    let monitor = runtime
        .progress_monitor
        .as_mut()
        .ok_or_else(|| "progress monitor was cleared".to_string())?;
    match condition {
        DeliveryCondition::Result { .. } => {
            monitor.result_delivery = ProgressDeliveryState::DeliveredToPty;
        }
        DeliveryCondition::GoalPause(_) => {}
        DeliveryCondition::GoalResume { binding, .. } => {
            monitor.goal_resume_delivery = ProgressDeliveryState::DeliveredToPty;
            monitor.goal_resume = ProgressGoalResumeState::ResumeDeliveredToPty(binding.clone());
        }
    }
    Ok(())
}

async fn mark_delivery_uncertain(
    state: &ServerState,
    pane_id: NodeId,
    condition: &DeliveryCondition,
) {
    let mut panes = state.panes.write().await;
    let Some(PaneResource::Terminal(runtime)) = panes.get_mut(&pane_id) else {
        return;
    };
    if !runtime.is_current_progress_monitor(condition.monitor_id()) {
        return;
    }
    let Some(monitor) = runtime.progress_monitor.as_mut() else {
        return;
    };
    match condition {
        DeliveryCondition::Result { .. } => {
            monitor.result_delivery = ProgressDeliveryState::Uncertain;
        }
        DeliveryCondition::GoalPause(binding) => {
            monitor.goal_resume = ProgressGoalResumeState::Unsafe(format!(
                "goal pause delivery for monitor {} was uncertain",
                binding.monitor_id
            ));
        }
        DeliveryCondition::GoalResume { binding, .. } => {
            monitor.goal_resume_delivery = ProgressDeliveryState::Uncertain;
            monitor.goal_resume = ProgressGoalResumeState::Unsafe(format!(
                "goal resume delivery for monitor {} was uncertain",
                binding.monitor_id
            ));
        }
    }
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

async fn queue_goal_resume_delivery(
    state: &ServerState,
    pane_id: NodeId,
    binding: &ProgressGoalBinding,
) -> Result<(), String> {
    let mut panes = state.panes.write().await;
    let Some(PaneResource::Terminal(runtime)) = panes.get_mut(&pane_id) else {
        return Err("pane closed before goal resume".to_string());
    };
    if !runtime.progress_goal_binding_matches(binding, GoalState::Paused) {
        mark_goal_unsafe(runtime, binding, "owned goal pause was no longer valid");
        return Err("owned goal pause was no longer valid".to_string());
    }
    let monitor = runtime
        .progress_monitor
        .as_mut()
        .expect("current monitor was checked above");
    if !matches!(&monitor.goal_resume, ProgressGoalResumeState::OwnedPause(current) if current == binding)
    {
        return Err("goal pause is not owned by this monitor".to_string());
    }
    monitor.goal_resume_delivery = ProgressDeliveryState::Queued;
    drop(panes);
    state.request_snapshot_save();
    Ok(())
}

async fn wait_for_owned_pause(
    state: &ServerState,
    pane_id: NodeId,
    binding: &ProgressGoalBinding,
) -> Result<(), String> {
    let mut screen_changed = {
        let panes = state.panes.read().await;
        let Some(PaneResource::Terminal(runtime)) = panes.get(&pane_id) else {
            return Err("pane closed while confirming goal pause".to_string());
        };
        runtime.session.subscribe_screen_changed()
    };
    loop {
        {
            let mut panes = state.panes.write().await;
            let Some(PaneResource::Terminal(runtime)) = panes.get_mut(&pane_id) else {
                return Err("pane closed while confirming goal pause".to_string());
            };
            if runtime.progress_goal_binding_matches(binding, GoalState::Paused) {
                let monitor = runtime
                    .progress_monitor
                    .as_mut()
                    .expect("current monitor was checked by binding match");
                if matches!(&monitor.goal_resume, ProgressGoalResumeState::PauseSubmitted(current) if current == binding)
                {
                    monitor.goal_resume = ProgressGoalResumeState::OwnedPause(binding.clone());
                    drop(panes);
                    state.request_snapshot_save();
                    return Ok(());
                }
            } else if !binding_identity_still_matches(runtime, binding) {
                mark_goal_unsafe(
                    runtime,
                    binding,
                    "goal owner changed before pause confirmation",
                );
                return Err("goal owner changed before pause confirmation".to_string());
            } else if runtime.confirmed_goal_owner.as_ref().is_some_and(|owner| {
                matches!(
                    owner.goal_state,
                    GoalState::Blocked | GoalState::UsageLimited | GoalState::Reached
                )
            }) {
                mark_goal_unsafe(runtime, binding, "goal entered a non-resumable state");
                return Err("goal entered a non-resumable state".to_string());
            }
        }
        tokio::select! {
            changed = screen_changed.changed() => {
                if changed.is_err() {
                    return Err("pane closed while confirming goal pause".to_string());
                }
            }
            () = tokio::time::sleep(READINESS_RECHECK_INTERVAL) => {}
        }
    }
}

fn binding_identity_still_matches(
    runtime: &crate::pane::TerminalPaneRuntime,
    binding: &ProgressGoalBinding,
) -> bool {
    runtime.is_current_progress_monitor(binding.monitor_id)
        && runtime.goal_owner_epoch == binding.goal_owner_epoch
        && !runtime.is_session_identity_invalidated
        && runtime.session_id.as_deref() == Some(binding.session_id.as_str())
        && runtime.session_process_id == Some(binding.process_id)
        && runtime.detected_agent_process_id == Some(binding.process_id)
}

async fn mark_goal_unsafe_in_state(
    state: &ServerState,
    pane_id: NodeId,
    binding: &ProgressGoalBinding,
    reason: &str,
) {
    let mut panes = state.panes.write().await;
    if let Some(PaneResource::Terminal(runtime)) = panes.get_mut(&pane_id) {
        mark_goal_unsafe(runtime, binding, reason);
    }
    drop(panes);
    state.request_snapshot_save();
}

fn mark_goal_unsafe(
    runtime: &mut crate::pane::TerminalPaneRuntime,
    binding: &ProgressGoalBinding,
    reason: &str,
) {
    if !runtime.is_current_progress_monitor(binding.monitor_id) {
        return;
    }
    if let Some(monitor) = runtime.progress_monitor.as_mut() {
        monitor.goal_resume = ProgressGoalResumeState::Unsafe(reason.to_string());
        monitor.goal_resume_delivery = ProgressDeliveryState::Uncertain;
    }
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

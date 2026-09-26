//! Agent-facing paused-goal status and agent-requested `/goal resume`.
//!
//! Ilium never pauses or resumes a goal by itself; a paused goal -- an Esc
//! interrupt, a server restart -- waits for the user. This module lets the
//! agent in the pane ask Ilium to submit `/goal resume` instead (`ilium goal
//! resume`), and reports the same decision to the agents' Stop hook (`ilium
//! goal status`/`stop-hook`).
//!
//! The request is fenced so it never overrides a pause the user typed or a
//! different goal owner, and it is submitted only at a ready composer: Codex
//! readiness excludes a working turn, so the requesting turn has ended.
//! Codex is the only provider with `/goal resume`; Claude Code continues a
//! paused goal when any message is sent and is reported as unsupported.

use std::sync::Arc;
use std::time::Duration;

use ilium_core::{AgentClass, GoalState, NodeId};
use ilium_ipc::{PaneGoalResumability, PaneGoalStatus, PromptSubmissionSource, ServerEvent};

use crate::pane::{
    AgentGoalResumeRequest, ConfirmedGoalOwner, GoalReminderState, PaneResource,
    TerminalPaneRuntime,
};
use crate::state::ServerState;

const READINESS_RECHECK_INTERVAL: Duration = Duration::from_millis(100);
/// Upper bound for one queued request. A turn that stays busy this long is
/// no longer the turn that asked; the agent can ask again.
const MAXIMUM_RESUME_WAIT: Duration = Duration::from_secs(2 * 60 * 60);
/// How long a goal must stay paused, unowned, and resumable at an idle
/// composer before Ilium reminds the agent once. Long enough for a user who
/// interrupted to type their next message; short enough that nobody waits
/// on a goal that only needed `/goal resume`.
const IDLE_REMINDER_DELAY: Duration = Duration::from_secs(5 * 60);
const IDLE_REMINDER_TICK: Duration = Duration::from_secs(30);
/// How long after a `/goal resume` write the pause still counts as being
/// resumed. Covers detection latency, never a genuinely new pause.
const RESUME_IN_FLIGHT_WINDOW: Duration = Duration::from_secs(20);

/// The pane facts that decide resumability, borrowed from one runtime so the
/// decision itself stays a pure, unit-tested function.
#[derive(Debug, Clone, Copy)]
struct GoalFacts<'a> {
    owner: Option<&'a ConfirmedGoalOwner>,
    detected_agent_process_id: Option<u32>,
    goal_owner_epoch: u64,
    user_paused_goal_epoch: Option<u64>,
    active_seen_epoch: Option<u64>,
    pending_resume: Option<&'a AgentGoalResumeRequest>,
    is_resume_in_flight: bool,
}

impl<'a> GoalFacts<'a> {
    fn from_runtime(runtime: &'a TerminalPaneRuntime) -> Self {
        Self {
            owner: runtime.confirmed_goal_owner.as_ref(),
            detected_agent_process_id: runtime.detected_agent_process_id,
            goal_owner_epoch: runtime.goal_owner_epoch,
            user_paused_goal_epoch: runtime.user_paused_goal_epoch,
            active_seen_epoch: runtime.goal_active_seen_epoch,
            pending_resume: runtime.agent_goal_resume.as_ref(),
            is_resume_in_flight: runtime
                .goal_resume_submitted_at
                .is_some_and(|submitted_at| submitted_at.elapsed() < RESUME_IN_FLIGHT_WINDOW),
        }
    }
}

/// Decides who may resume the pane's goal. Order matters: a pause by the user
/// outranks the agent, and a queued request is reported before `Resumable` so
/// repeated requests stay idempotent.
fn resumability(facts: GoalFacts<'_>) -> PaneGoalResumability {
    let Some(owner) = facts.owner else {
        return PaneGoalResumability::NoGoal;
    };
    if owner.goal_state != GoalState::Paused {
        return PaneGoalResumability::NotPaused;
    }
    match &owner.agent_class {
        AgentClass::Codex => {}
        AgentClass::Claude => {
            return PaneGoalResumability::Unsupported {
                reason: "Claude Code has no /goal resume; its paused goal continues when a message is sent".to_string(),
            };
        }
        other => {
            return PaneGoalResumability::Unsupported {
                reason: format!(
                    "agent-requested goal resume supports Codex only, not {}",
                    other.label()
                ),
            };
        }
    }
    if facts.detected_agent_process_id != Some(owner.process_id) {
        return PaneGoalResumability::Unsupported {
            reason: "the paused goal's process is no longer the detected agent".to_string(),
        };
    }
    if facts.is_resume_in_flight {
        return PaneGoalResumability::ResumeQueued;
    }
    if facts.user_paused_goal_epoch == Some(facts.goal_owner_epoch) {
        return PaneGoalResumability::PausedByUser;
    }
    if facts.active_seen_epoch != Some(facts.goal_owner_epoch) {
        return PaneGoalResumability::PauseOriginUnknown;
    }
    if facts.pending_resume.is_some_and(|request| {
        request.goal_owner_epoch == facts.goal_owner_epoch && request.process_id == owner.process_id
    }) {
        return PaneGoalResumability::ResumeQueued;
    }
    PaneGoalResumability::Resumable
}

fn pane_goal_status(runtime: &TerminalPaneRuntime, pane_id: NodeId) -> PaneGoalStatus {
    let owner = runtime.confirmed_goal_owner.as_ref();
    PaneGoalStatus {
        pane_id,
        agent: owner.map(|owner| owner.agent_class.label().to_string()),
        goal_state: owner.map(|owner| owner.goal_state),
        resumability: resumability(GoalFacts::from_runtime(runtime)),
    }
}

/// Human-readable refusal for every non-queueable status.
fn refusal_reason(resumability: &PaneGoalResumability) -> String {
    match resumability {
        PaneGoalResumability::NoGoal => "no /goal is confirmed for this pane".to_string(),
        PaneGoalResumability::NotPaused => "the goal is not paused".to_string(),
        PaneGoalResumability::PausedByUser => {
            "the user typed /goal pause; only the user resumes this goal".to_string()
        }
        PaneGoalResumability::PauseOriginUnknown => {
            "Ilium never saw this goal active, so the user may have paused it; only the user resumes it".to_string()
        }
        PaneGoalResumability::Unsupported { reason } => reason.clone(),
        PaneGoalResumability::ResumeQueued | PaneGoalResumability::Resumable => {
            "the goal is resumable".to_string()
        }
    }
}

/// Correlated reply to `GetPaneGoalStatus`.
pub(crate) async fn goal_status_event(
    state: &ServerState,
    request_id: u64,
    pane_id: NodeId,
) -> ServerEvent {
    let result = match state.panes.read().await.get(&pane_id) {
        Some(PaneResource::Terminal(runtime)) => Ok(pane_goal_status(runtime, pane_id)),
        _ => Err(format!("pane {pane_id:?} is not a live terminal pane")),
    };
    ServerEvent::PaneGoalStatusReported {
        request_id,
        pane_id,
        result,
    }
}

/// Correlated reply to `RequestPaneGoalResume`: queues the resume when the
/// goal is `Resumable`, succeeds idempotently when one is already queued, and
/// otherwise refuses with the status that decided it.
pub(crate) async fn request_resume_event(
    state: &Arc<ServerState>,
    request_id: u64,
    pane_id: NodeId,
) -> ServerEvent {
    let result = queue_agent_goal_resume(state, pane_id).await;
    ServerEvent::PaneGoalResumeRequested {
        request_id,
        pane_id,
        result,
    }
}

async fn queue_agent_goal_resume(
    state: &Arc<ServerState>,
    pane_id: NodeId,
) -> Result<PaneGoalStatus, (String, Option<PaneGoalStatus>)> {
    let mut panes = state.panes.write().await;
    let Some(PaneResource::Terminal(runtime)) = panes.get_mut(&pane_id) else {
        return Err((
            format!("pane {pane_id:?} is not a live terminal pane"),
            None,
        ));
    };
    let status = pane_goal_status(runtime, pane_id);
    match &status.resumability {
        PaneGoalResumability::ResumeQueued => return Ok(status),
        PaneGoalResumability::Resumable => {}
        other => return Err((refusal_reason(other), Some(status))),
    }
    let owner_process_id = runtime
        .confirmed_goal_owner
        .as_ref()
        .map(|owner| owner.process_id)
        .expect("a Resumable status requires a confirmed goal owner");
    let request = AgentGoalResumeRequest {
        goal_owner_epoch: runtime.goal_owner_epoch,
        process_id: owner_process_id,
    };
    runtime.agent_goal_resume = Some(request.clone());
    let task_state = Arc::clone(state);
    let task_request = request.clone();
    let task = tokio::spawn(async move {
        let outcome = tokio::time::timeout(
            MAXIMUM_RESUME_WAIT,
            deliver_agent_goal_resume(&task_state, pane_id, &task_request),
        )
        .await;
        let message = match outcome {
            Ok(Ok(message)) => {
                tracing::info!(pane_id = pane_id.0, %message, "agent-requested goal resume finished");
                return;
            }
            Ok(Err(message)) => message,
            Err(_) => "the requesting turn did not end in time".to_string(),
        };
        tracing::warn!(pane_id = pane_id.0, %message, "agent-requested goal resume dropped");
        let mut panes = task_state.panes.write().await;
        if let Some(PaneResource::Terminal(runtime)) = panes.get_mut(&pane_id) {
            forget_request(runtime, &task_request);
        }
    });
    runtime.set_agent_goal_resume_task(task);
    tracing::info!(pane_id = pane_id.0, "agent-requested goal resume queued");
    Ok(pane_goal_status(runtime, pane_id))
}

/// Clears the pending request only if it is still this one, so a newer
/// request queued after a cancellation is never discarded by an old waiter.
fn forget_request(runtime: &mut TerminalPaneRuntime, request: &AgentGoalResumeRequest) {
    if runtime.agent_goal_resume.as_ref() == Some(request) {
        runtime.agent_goal_resume = None;
    }
}

/// Whether `request` still describes the pane: same pending request and a
/// goal that still resolves to `ResumeQueued` for it.
fn request_is_current(runtime: &TerminalPaneRuntime, request: &AgentGoalResumeRequest) -> bool {
    runtime.agent_goal_resume.as_ref() == Some(request)
        && resumability(GoalFacts::from_runtime(runtime)) == PaneGoalResumability::ResumeQueued
}

/// Waits for a ready (not working) composer, then submits `/goal resume` while
/// holding the pane's input gate. `Ok` means submitted or already resumed;
/// `Err` names why the request no longer applies.
async fn deliver_agent_goal_resume(
    state: &ServerState,
    pane_id: NodeId,
    request: &AgentGoalResumeRequest,
) -> Result<String, String> {
    let mut screen_changed = {
        let panes = state.panes.read().await;
        let Some(PaneResource::Terminal(runtime)) = panes.get(&pane_id) else {
            return Err("pane closed before goal resume".to_string());
        };
        runtime.session.subscribe_screen_changed()
    };
    loop {
        let input_gate = {
            let mut panes = state.panes.write().await;
            let Some(PaneResource::Terminal(runtime)) = panes.get_mut(&pane_id) else {
                return Err("pane closed before goal resume".to_string());
            };
            if runtime.agent_goal_resume.as_ref() != Some(request) {
                return Err("the request was cancelled or replaced".to_string());
            }
            if runtime
                .confirmed_goal_owner
                .as_ref()
                .is_some_and(|owner| owner.goal_state == GoalState::Active)
            {
                forget_request(runtime, request);
                return Ok("the goal was already resumed".to_string());
            }
            if !request_is_current(runtime, request) {
                let status = resumability(GoalFacts::from_runtime(runtime));
                return Err(refusal_reason(&status));
            }
            crate::agent_delivery::runtime_has_ready_composer(runtime)
                .then(|| Arc::clone(&runtime.input_gate))
        };
        if let Some(input_gate) = input_gate {
            let _input_guard = input_gate.lock().await;
            let still_ready = {
                let panes = state.panes.read().await;
                let Some(PaneResource::Terminal(runtime)) = panes.get(&pane_id) else {
                    return Err("pane closed before goal resume".to_string());
                };
                Arc::ptr_eq(&input_gate, &runtime.input_gate)
                    && request_is_current(runtime, request)
                    && crate::agent_delivery::runtime_has_ready_composer(runtime)
            };
            if still_ready {
                crate::ipc::handlers::submit_terminal_text_locked(
                    state,
                    pane_id,
                    "/goal resume",
                    PromptSubmissionSource::AgentGoalResume,
                    &input_gate,
                )
                .await?;
                let mut panes = state.panes.write().await;
                if let Some(PaneResource::Terminal(runtime)) = panes.get_mut(&pane_id) {
                    runtime.goal_resume_submitted_at = Some(std::time::Instant::now());
                    forget_request(runtime, request);
                }
                return Ok("submitted /goal resume".to_string());
            }
        }
        tokio::select! {
            changed = screen_changed.changed() => {
                if changed.is_err() {
                    return Err("pane closed before goal resume".to_string());
                }
            }
            () = tokio::time::sleep(READINESS_RECHECK_INTERVAL) => {}
        }
    }
}

/// Advances one pane's reminder episode. Returns the new state and whether
/// the reminder is due now. Any tick that is not `Resumable` ends the
/// episode, so a later pause starts a fresh one with a fresh delay.
fn advance_reminder(
    current: Option<GoalReminderState>,
    is_resumable: bool,
    goal_owner_epoch: u64,
    now: std::time::Instant,
) -> (Option<GoalReminderState>, bool) {
    if !is_resumable {
        return (None, false);
    }
    match current {
        Some(episode) if episode.goal_owner_epoch == goal_owner_epoch => {
            let is_due = !episode.is_delivered
                && now.duration_since(episode.resumable_since) >= IDLE_REMINDER_DELAY;
            (Some(episode), is_due)
        }
        _ => (
            Some(GoalReminderState {
                goal_owner_epoch,
                resumable_since: now,
                is_delivered: false,
            }),
            false,
        ),
    }
}

const IDLE_REMINDER_TEXT: &str = "Ilium: your /goal has stayed paused for several minutes while you were idle, and the user did not pause it. Unless a stop request, handoff, or pending user decision blocks the work, run `ilium goal resume` and end the turn. Otherwise say what blocks it, and make the last line of your message exactly: ACTION NEEDED: type /goal resume";

/// Background watchdog for the case nothing else covers: an agent that ended
/// its turn with an unblocked goal paused. It never resumes the goal; it
/// sends the agent one reminder per pause episode, at a ready composer, so
/// the agent resumes it or hands the decision to the user visibly.
pub(crate) fn spawn_idle_reminder(state: Arc<ServerState>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(IDLE_REMINDER_TICK);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            for pane_id in panes_due_for_reminder(&state).await {
                if let Err(message) = deliver_idle_reminder(&state, pane_id).await {
                    tracing::warn!(pane_id = pane_id.0, %message, "paused-goal reminder not delivered");
                }
            }
        }
    })
}

async fn panes_due_for_reminder(state: &ServerState) -> Vec<NodeId> {
    let now = std::time::Instant::now();
    let mut panes = state.panes.write().await;
    let mut due = Vec::new();
    for (pane_id, resource) in panes.iter_mut() {
        let PaneResource::Terminal(runtime) = resource else {
            continue;
        };
        // The delay counts continuous idle time only: a working turn (no
        // ready composer) ends the episode, so the reminder can never race
        // the agent's own Stop-hook decision.
        let is_idle_and_resumable = crate::agent_delivery::runtime_has_ready_composer(runtime)
            && resumability(GoalFacts::from_runtime(runtime)) == PaneGoalResumability::Resumable;
        let (next, is_due) = advance_reminder(
            runtime.goal_reminder,
            is_idle_and_resumable,
            runtime.goal_owner_epoch,
            now,
        );
        runtime.goal_reminder = next;
        if is_due {
            due.push(*pane_id);
        }
    }
    due.sort_by_key(|pane_id| pane_id.0);
    due
}

/// Submits the reminder under the pane's input gate after re-validating that
/// the goal is still resumable and the composer still ready. The episode is
/// marked delivered before the write, so a failed write is never retried
/// into a pane that may have received part of it.
async fn deliver_idle_reminder(state: &ServerState, pane_id: NodeId) -> Result<(), String> {
    let input_gate = {
        let panes = state.panes.read().await;
        let Some(PaneResource::Terminal(runtime)) = panes.get(&pane_id) else {
            return Err("pane closed before reminder".to_string());
        };
        Arc::clone(&runtime.input_gate)
    };
    let _input_guard = input_gate.lock().await;
    {
        let mut panes = state.panes.write().await;
        let Some(PaneResource::Terminal(runtime)) = panes.get_mut(&pane_id) else {
            return Err("pane closed before reminder".to_string());
        };
        let is_still_due = Arc::ptr_eq(&input_gate, &runtime.input_gate)
            && resumability(GoalFacts::from_runtime(runtime)) == PaneGoalResumability::Resumable
            && crate::agent_delivery::runtime_has_ready_composer(runtime);
        let Some(episode) = runtime.goal_reminder.as_mut().filter(|episode| {
            is_still_due
                && !episode.is_delivered
                && episode.goal_owner_epoch == runtime.goal_owner_epoch
        }) else {
            return Ok(());
        };
        episode.is_delivered = true;
    }
    crate::ipc::handlers::submit_terminal_text_locked(
        state,
        pane_id,
        IDLE_REMINDER_TEXT,
        PromptSubmissionSource::GoalPauseReminder,
        &input_gate,
    )
    .await?;
    tracing::info!(pane_id = pane_id.0, "paused-goal reminder delivered");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owner(agent_class: AgentClass, goal_state: GoalState) -> ConfirmedGoalOwner {
        ConfirmedGoalOwner {
            process_id: 42,
            agent_class,
            goal_state,
        }
    }

    fn facts(owner: Option<&ConfirmedGoalOwner>) -> GoalFacts<'_> {
        GoalFacts {
            owner,
            detected_agent_process_id: Some(42),
            goal_owner_epoch: 3,
            user_paused_goal_epoch: None,
            active_seen_epoch: Some(3),
            pending_resume: None,
            is_resume_in_flight: false,
        }
    }

    #[test]
    fn unowned_codex_pause_is_resumable_and_other_phases_are_not() {
        let paused = owner(AgentClass::Codex, GoalState::Paused);
        assert_eq!(
            resumability(facts(Some(&paused))),
            PaneGoalResumability::Resumable
        );
        assert_eq!(resumability(facts(None)), PaneGoalResumability::NoGoal);
        for goal_state in [
            GoalState::Active,
            GoalState::Blocked,
            GoalState::UsageLimited,
            GoalState::Reached,
        ] {
            let other = owner(AgentClass::Codex, goal_state);
            assert_eq!(
                resumability(facts(Some(&other))),
                PaneGoalResumability::NotPaused,
                "{goal_state:?}"
            );
        }
    }

    #[test]
    fn user_pauses_outrank_the_agent() {
        let paused = owner(AgentClass::Codex, GoalState::Paused);
        let mut user_paused = facts(Some(&paused));
        user_paused.user_paused_goal_epoch = Some(3);
        assert_eq!(
            resumability(user_paused),
            PaneGoalResumability::PausedByUser
        );

        // A pause typed for an earlier goal owner does not bind this one.
        user_paused.user_paused_goal_epoch = Some(2);
        assert_eq!(resumability(user_paused), PaneGoalResumability::Resumable);
    }

    #[test]
    fn a_pause_never_seen_active_fails_closed_and_a_written_resume_is_not_repeated() {
        let paused = owner(AgentClass::Codex, GoalState::Paused);
        let mut restarted = facts(Some(&paused));
        restarted.active_seen_epoch = Some(2);
        assert_eq!(
            resumability(restarted),
            PaneGoalResumability::PauseOriginUnknown
        );
        restarted.active_seen_epoch = None;
        assert_eq!(
            resumability(restarted),
            PaneGoalResumability::PauseOriginUnknown
        );

        // Between a `/goal resume` write and detection seeing `Active`, the
        // pause reads as queued so neither the hook nor the agent re-queues.
        let mut in_flight = facts(Some(&paused));
        in_flight.is_resume_in_flight = true;
        assert_eq!(resumability(in_flight), PaneGoalResumability::ResumeQueued);
    }

    #[test]
    fn a_queued_request_is_reported_only_for_its_own_goal_owner() {
        let paused = owner(AgentClass::Codex, GoalState::Paused);
        let current = AgentGoalResumeRequest {
            goal_owner_epoch: 3,
            process_id: 42,
        };
        let mut queued = facts(Some(&paused));
        queued.pending_resume = Some(&current);
        assert_eq!(resumability(queued), PaneGoalResumability::ResumeQueued);

        let stale = AgentGoalResumeRequest {
            goal_owner_epoch: 2,
            ..current
        };
        queued.pending_resume = Some(&stale);
        assert_eq!(resumability(queued), PaneGoalResumability::Resumable);
    }

    #[test]
    fn claude_other_providers_and_replaced_processes_are_unsupported() {
        let claude = owner(AgentClass::Claude, GoalState::Paused);
        assert!(matches!(
            resumability(facts(Some(&claude))),
            PaneGoalResumability::Unsupported { reason } if reason.contains("Claude Code has no /goal resume")
        ));
        let custom = owner(AgentClass::Other("aider".to_string()), GoalState::Paused);
        assert!(matches!(
            resumability(facts(Some(&custom))),
            PaneGoalResumability::Unsupported { .. }
        ));
        let paused = owner(AgentClass::Codex, GoalState::Paused);
        let mut replaced = facts(Some(&paused));
        replaced.detected_agent_process_id = Some(43);
        assert!(matches!(
            resumability(replaced),
            PaneGoalResumability::Unsupported { .. }
        ));
    }

    #[test]
    fn a_reminder_is_due_once_per_resumable_episode_after_the_delay() {
        let start = std::time::Instant::now();
        let (episode, is_due) = advance_reminder(None, true, 3, start);
        assert!(!is_due);
        let (episode, is_due) = advance_reminder(episode, true, 3, start + Duration::from_secs(60));
        assert!(!is_due, "not before the delay");
        let later = start + IDLE_REMINDER_DELAY;
        let (episode, is_due) = advance_reminder(episode, true, 3, later);
        assert!(is_due);
        let delivered = episode.map(|episode| GoalReminderState {
            is_delivered: true,
            ..episode
        });
        let (episode, is_due) = advance_reminder(delivered, true, 3, later);
        assert!(!is_due, "only once per episode");

        // Resuming (or any non-resumable tick) ends the episode; a later
        // pause starts a fresh delay.
        let (ended, _) = advance_reminder(episode, false, 3, later);
        assert_eq!(ended, None);
        let (fresh, is_due) = advance_reminder(ended, true, 3, later);
        assert!(!is_due);
        assert_eq!(fresh.map(|episode| episode.resumable_since), Some(later));

        // A different goal owner starts a fresh episode too.
        let (replaced, is_due) = advance_reminder(delivered, true, 4, later + IDLE_REMINDER_DELAY);
        assert!(!is_due);
        assert_eq!(replaced.map(|episode| episode.goal_owner_epoch), Some(4));
    }

    #[test]
    fn every_refusal_names_its_cause() {
        for (resumability, fragment) in [
            (PaneGoalResumability::NoGoal, "no /goal"),
            (PaneGoalResumability::NotPaused, "not paused"),
            (PaneGoalResumability::PausedByUser, "user typed /goal pause"),
            (
                PaneGoalResumability::PauseOriginUnknown,
                "never saw this goal active",
            ),
        ] {
            assert!(
                refusal_reason(&resumability).contains(fragment),
                "{fragment}"
            );
        }
    }
}

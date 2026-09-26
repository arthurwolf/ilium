//! `ilium goal`: agent-facing paused-goal status, agent-requested resume, and
//! the provider Stop-hook adapter built on both.
//!
//! Like `ilium progress`, these commands run from inside an Ilium pane and
//! address that pane through `pane_identity_from_env`. `status` and `resume`
//! print exactly one JSONL record. `stop-hook` instead speaks the Claude
//! Code / Codex Stop-hook contract: it reads the hook's JSON from stdin and
//! prints nothing (allow), `{"decision":"block","reason":...}` (continue the
//! turn so the agent resumes or hands off), or `{"systemMessage":...}`
//! (allow, and tell the user the goal waits on them). A hook must never
//! break the agent, so every failure there degrades to "allow".

use std::io::Read;
use std::time::Duration;

use clap::{Subcommand, ValueEnum};
use ilium_ipc::{PaneGoalResumability, PaneGoalStatus};

use crate::{json_string, next_progress_request_id, pane_identity_from_env, CliError};

/// Keeps a Stop hook from stalling the agent when the server is busy.
const STOP_HOOK_REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const GOAL_REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Subcommand, Debug)]
pub(crate) enum GoalCommand {
    /// Reports whether this pane's `/goal` is paused and who may resume it:
    /// `resumable` (run `ilium goal resume`), `paused-by-user`,
    /// `pause-origin-unknown`, `resume-queued`, `not-paused`, `no-goal`, or
    /// `unsupported`.
    Status,
    /// Asks Ilium to submit `/goal resume` in this pane once the current
    /// turn has ended. Accepted only when `status` reports `resumable`;
    /// repeating it while a resume is queued is harmless.
    Resume,
    /// Stop-hook adapter for Claude Code and Codex. Reads the hook input on
    /// stdin; blocks the stop once when the goal is paused and resumable, and
    /// shows the user a message when the goal waits on them.
    StopHook {
        #[arg(long, value_enum)]
        provider: StopHookProvider,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub(crate) enum StopHookProvider {
    Claude,
    Codex,
}

/// Outcome of one Stop-hook evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
enum StopHookDecision {
    Allow,
    /// Continue the turn; the reason is shown to the agent.
    Block(String),
    /// Allow the stop; the message is shown to the user.
    Notify(String),
}

pub(crate) async fn goal(command: GoalCommand) -> Result<(), CliError> {
    let request_id = next_progress_request_id();
    match command {
        GoalCommand::Status => match request_goal_status(request_id, GOAL_REQUEST_TIMEOUT).await {
            Ok(Ok(status)) => {
                println!(
                    "{}",
                    goal_status_json("goal_status", request_id, &status, None)
                );
                Ok(())
            }
            Ok(Err(message)) => {
                print_goal_failure("status", request_id, "pane-unavailable", &message);
                Err(CliError::ServerReportedError(message))
            }
            Err(error) => {
                print_goal_failure("status", request_id, "request-failed", &error.to_string());
                Err(error)
            }
        },
        GoalCommand::Resume => match request_goal_resume(request_id).await {
            Ok(Ok(status)) => {
                println!(
                    "{}",
                    goal_status_json("goal_resume_queued", request_id, &status, None)
                );
                Ok(())
            }
            Ok(Err((message, Some(status)))) => {
                println!(
                    "{}",
                    goal_status_json("goal_resume_rejected", request_id, &status, Some(&message))
                );
                Err(CliError::ServerReportedError(message))
            }
            Ok(Err((message, None))) => {
                print_goal_failure("resume", request_id, "pane-unavailable", &message);
                Err(CliError::ServerReportedError(message))
            }
            Err(error) => {
                print_goal_failure("resume", request_id, "request-failed", &error.to_string());
                Err(error)
            }
        },
        GoalCommand::StopHook { provider } => {
            stop_hook(provider).await;
            Ok(())
        }
    }
}

/// Never fails: an unreadable input, a pane outside Ilium, or an unreachable
/// server all mean "allow", so the hook can be installed globally.
async fn stop_hook(provider: StopHookProvider) {
    let mut input = String::new();
    let _ = std::io::stdin().read_to_string(&mut input);
    let is_continuation = stop_hook_active_from_input(&input);
    if pane_identity_from_env().is_err() {
        return;
    }
    let request_id = next_progress_request_id();
    let status = match request_goal_status(request_id, STOP_HOOK_REQUEST_TIMEOUT).await {
        Ok(Ok(status)) => status,
        Ok(Err(_)) => return,
        Err(error) => {
            eprintln!("ilium goal stop-hook ({provider:?}): {error}");
            return;
        }
    };
    match stop_hook_decision(is_continuation, &status) {
        StopHookDecision::Allow => {}
        StopHookDecision::Block(reason) => println!(
            "{{\"decision\":\"block\",\"reason\":{}}}",
            json_string(&reason)
        ),
        StopHookDecision::Notify(message) => {
            println!("{{\"systemMessage\":{}}}", json_string(&message));
        }
    }
}

/// The paused-goal policy the hook enforces. It blocks at most once per stop
/// sequence (`is_continuation` is the providers' `stop_hook_active`): the
/// agent then either runs `ilium goal resume` or states why it cannot, and
/// the next stop is allowed with a user-visible reminder instead of looping.
fn stop_hook_decision(is_continuation: bool, status: &PaneGoalStatus) -> StopHookDecision {
    match &status.resumability {
        PaneGoalResumability::Resumable if !is_continuation => StopHookDecision::Block(
            "Your /goal is paused, and the user did not pause it. Unless a stop request, handoff, or pending user decision blocks the work, run `ilium goal resume` now (Ilium submits /goal resume after this turn ends), then end the turn. If something does block it, say what, and make the last line of your message exactly: ACTION NEEDED: type /goal resume".to_string(),
        ),
        PaneGoalResumability::Resumable => StopHookDecision::Notify(
            "The /goal in this pane is paused and was not resumed. Type /goal resume to continue it.".to_string(),
        ),
        PaneGoalResumability::PausedByUser => StopHookDecision::Notify(
            "The /goal in this pane is paused because you paused it. Type /goal resume when ready.".to_string(),
        ),
        PaneGoalResumability::PauseOriginUnknown => StopHookDecision::Notify(
            "The /goal in this pane is paused, and Ilium cannot tell who paused it. Type /goal resume to continue it.".to_string(),
        ),
        PaneGoalResumability::NoGoal
        | PaneGoalResumability::NotPaused
        | PaneGoalResumability::ResumeQueued
        | PaneGoalResumability::Unsupported { .. } => StopHookDecision::Allow,
    }
}

/// Reads the providers' `stop_hook_active` flag without a JSON dependency:
/// the key is a fixed top-level boolean in both Claude Code's and Codex's
/// Stop-hook input. Anything unparseable counts as a first stop.
fn stop_hook_active_from_input(input: &str) -> bool {
    const KEY: &str = "\"stop_hook_active\"";
    let Some(start) = input.find(KEY) else {
        return false;
    };
    let rest = input[start + KEY.len()..].trim_start();
    let Some(value) = rest.strip_prefix(':') else {
        return false;
    };
    value.trim_start().starts_with("true")
}

type ResumeResult = Result<PaneGoalStatus, (String, Option<PaneGoalStatus>)>;

async fn request_goal_status(
    request_id: u64,
    timeout: Duration,
) -> Result<Result<PaneGoalStatus, String>, CliError> {
    let request = |pane_id| ilium_ipc::ClientRequest::GetPaneGoalStatus {
        request_id,
        pane_id,
    };
    exchange(request, timeout, |event| match event {
        ilium_ipc::ServerEvent::PaneGoalStatusReported {
            request_id: response_id,
            result,
            ..
        } if response_id == request_id => Some(result),
        _ => None,
    })
    .await
}

async fn request_goal_resume(request_id: u64) -> Result<ResumeResult, CliError> {
    let request = |pane_id| ilium_ipc::ClientRequest::RequestPaneGoalResume {
        request_id,
        pane_id,
    };
    exchange(request, GOAL_REQUEST_TIMEOUT, |event| match event {
        ilium_ipc::ServerEvent::PaneGoalResumeRequested {
            request_id: response_id,
            result,
            ..
        } if response_id == request_id => Some(result),
        _ => None,
    })
    .await
}

/// Sends one pane-addressed request and waits for the event `matcher`
/// correlates to it. Silence is a timeout, never an answer.
async fn exchange<T>(
    build_request: impl FnOnce(ilium_core::NodeId) -> ilium_ipc::ClientRequest,
    timeout: Duration,
    mut matcher: impl FnMut(ilium_ipc::ServerEvent) -> Option<T>,
) -> Result<T, CliError> {
    let identity = pane_identity_from_env()?;
    let mut connection = ilium_client::connection::Connection::connect(
        &identity.socket_path,
        identity.session_name.clone(),
    )
    .await?;
    if connection
        .requests
        .send(build_request(identity.pane_id))
        .await
        .is_err()
    {
        return Err(CliError::ServerReportedError(
            "connection closed before the request was sent".to_string(),
        ));
    }
    let response = tokio::time::timeout(timeout, async {
        while let Some(event) = connection.events.recv().await {
            if let Some(response) = matcher(event) {
                return Ok(response);
            }
        }
        Err(CliError::ServerReportedError(
            "connection closed before the correlated goal response arrived".to_string(),
        ))
    })
    .await
    .map_err(|_| {
        CliError::ServerReportedError(format!(
            "timed out after {timeout:?} waiting for the goal response; an Ilium server older than this CLI does not answer goal requests"
        ))
    })?;
    let _ = connection
        .requests
        .send(ilium_ipc::ClientRequest::Detach)
        .await;
    response
}

fn resumability_name(resumability: &PaneGoalResumability) -> &'static str {
    match resumability {
        PaneGoalResumability::NoGoal => "no-goal",
        PaneGoalResumability::NotPaused => "not-paused",
        PaneGoalResumability::PausedByUser => "paused-by-user",
        PaneGoalResumability::PauseOriginUnknown => "pause-origin-unknown",
        PaneGoalResumability::ResumeQueued => "resume-queued",
        PaneGoalResumability::Resumable => "resumable",
        PaneGoalResumability::Unsupported { .. } => "unsupported",
    }
}

/// What the agent should do next for each status, so the JSONL record is
/// actionable without the agent re-deriving the policy.
fn next_action(resumability: &PaneGoalResumability) -> &'static str {
    match resumability {
        PaneGoalResumability::Resumable => {
            "run `ilium goal resume` unless a stop, handoff, or user decision blocks the work; otherwise end your final message with: ACTION NEEDED: type /goal resume"
        }
        PaneGoalResumability::PausedByUser | PaneGoalResumability::PauseOriginUnknown => {
            "do not resume; end your final message with: ACTION NEEDED: type /goal resume when ready"
        }
        PaneGoalResumability::ResumeQueued => {
            "end the turn: Ilium submits /goal resume once the composer is free"
        }
        PaneGoalResumability::NoGoal | PaneGoalResumability::NotPaused => "nothing",
        PaneGoalResumability::Unsupported { .. } => {
            "if the goal is paused and work remains, tell the user how to continue it"
        }
    }
}

const fn goal_state_name(goal_state: ilium_core::GoalState) -> &'static str {
    match goal_state {
        ilium_core::GoalState::Active => "active",
        ilium_core::GoalState::Paused => "paused",
        ilium_core::GoalState::Blocked => "blocked",
        ilium_core::GoalState::UsageLimited => "usage-limited",
        ilium_core::GoalState::Reached => "reached",
    }
}

fn goal_status_json(
    record_type: &str,
    request_id: u64,
    status: &PaneGoalStatus,
    rejection: Option<&str>,
) -> String {
    let optional = |value: Option<&str>| value.map_or_else(|| "null".to_string(), json_string);
    let reason = match &status.resumability {
        PaneGoalResumability::Unsupported { reason } => Some(reason.as_str()),
        _ => None,
    };
    format!(
        "{{\"type\":{},\"request_id\":{request_id},\"pane_id\":{},\"agent\":{},\"goal_state\":{},\"resumability\":{},\"reason\":{},\"rejection\":{},\"next_action\":{}}}",
        json_string(record_type),
        status.pane_id.0,
        optional(status.agent.as_deref()),
        optional(status.goal_state.map(goal_state_name)),
        json_string(resumability_name(&status.resumability)),
        optional(reason),
        optional(rejection),
        json_string(next_action(&status.resumability)),
    )
}

fn print_goal_failure(operation: &str, request_id: u64, code: &str, message: &str) {
    println!(
        "{{\"type\":\"goal_request_failed\",\"operation\":{},\"request_id\":{request_id},\"code\":{},\"message\":{}}}",
        json_string(operation),
        json_string(code),
        json_string(message)
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_core::NodeId;

    fn status(resumability: PaneGoalResumability) -> PaneGoalStatus {
        PaneGoalStatus {
            pane_id: NodeId(4),
            agent: Some("Codex".to_string()),
            goal_state: Some(ilium_core::GoalState::Paused),
            resumability,
        }
    }

    #[test]
    fn a_resumable_pause_blocks_once_then_tells_the_user() {
        let resumable = status(PaneGoalResumability::Resumable);
        assert!(matches!(
            stop_hook_decision(false, &resumable),
            StopHookDecision::Block(reason) if reason.contains("ilium goal resume") && reason.contains("ACTION NEEDED: type /goal resume")
        ));
        assert!(matches!(
            stop_hook_decision(true, &resumable),
            StopHookDecision::Notify(message) if message.contains("/goal resume")
        ));
    }

    #[test]
    fn user_pauses_notify_and_queued_or_unsupported_pauses_allow() {
        for waits_on_user in [
            PaneGoalResumability::PausedByUser,
            PaneGoalResumability::PauseOriginUnknown,
        ] {
            assert!(matches!(
                stop_hook_decision(false, &status(waits_on_user)),
                StopHookDecision::Notify(_)
            ));
        }
        for resumability in [
            PaneGoalResumability::NoGoal,
            PaneGoalResumability::NotPaused,
            PaneGoalResumability::ResumeQueued,
            PaneGoalResumability::Unsupported {
                reason: "Claude".to_string(),
            },
        ] {
            assert_eq!(
                stop_hook_decision(false, &status(resumability.clone())),
                StopHookDecision::Allow,
                "{resumability:?}"
            );
        }
    }

    #[test]
    fn stop_hook_active_is_read_from_both_providers_input_shapes() {
        assert!(stop_hook_active_from_input(
            r#"{"session_id":"s","stop_hook_active": true,"cwd":"/x"}"#
        ));
        assert!(!stop_hook_active_from_input(
            r#"{"stop_hook_active":false,"hook_event_name":"Stop"}"#
        ));
        assert!(!stop_hook_active_from_input(""));
        assert!(!stop_hook_active_from_input("not json"));
    }

    #[test]
    fn status_record_is_one_line_with_the_next_action() {
        let record = goal_status_json(
            "goal_status",
            9,
            &status(PaneGoalResumability::Resumable),
            None,
        );
        assert!(!record.contains('\n'));
        assert!(record.contains("\"type\":\"goal_status\""));
        assert!(record.contains("\"goal_state\":\"paused\""));
        assert!(record.contains("\"resumability\":\"resumable\""));
        assert!(record.contains("\"next_action\":"));
    }
}

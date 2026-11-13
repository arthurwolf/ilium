//! `ilium close-pane`: closes a pane of the running session, optionally with
//! its Ilium-created Git worktree. The pane is normally the one this command
//! runs in (`ILIUM_PANE_ID`), so an agent can wrap up its own worktree after
//! it has committed and merged its work.
//!
//! The server does the work through `ClientRequest::ClosePaneWithWorkspaceDisposition`.
//! For `--worktree remove` the server stops the pane's processes, checks the
//! worktree (clean, merged, no running process, no custody ticket), removes
//! it, and only then closes the pane. A refusal keeps the worktree. A refusal
//! before the pane's processes stop leaves the pane open; a refusal after they
//! stopped also closes the pane. Ilium
//! never removes a worktree it did not create.
//!
//! Self-close caveat: closing the pane stops every process in it, this command
//! included. The server still completes the request, but the lines printed
//! after the pane stops may never reach the agent. Check the outcome with
//! `git worktree list` and the pane tree when the output is missing.
//!
//! Output is JSONL on stdout: one `progress` record, then exactly one of
//! `result` (`outcome` is `closed` or `removed`), `blocked` (the server refused;
//! `reasons` says why), `uncertain` (the server did not confirm the outcome
//! in time or reported a failed removal; inspect the worktree before retrying),
//! or `error` (the request never ran). Exit status: 0 for `result`, 1 for
//! `blocked` or `uncertain`, 2 for `error`.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use clap::{Args, ValueEnum};
use ilium_core::NodeId;
use ilium_ipc::{ClientRequest, ServerEvent, WorkspaceDisposition};
use serde_json::json;

use crate::pane_scan;
use crate::CliError;

const COMMAND_NAME: &str = "close-pane";
/// Prefix the server puts on a removal reason when it cannot prove the
/// checkout is intact (see `complete_workspace_removal`).
const UNCERTAIN_REASON_PREFIX: &str = "REMOVAL OUTCOME UNCERTAIN";

#[derive(Args, Debug)]
pub(crate) struct ClosePaneArgs {
    /// Pane to close, in the session of this pane. Defaults to the pane this
    /// command runs in.
    #[arg(long, value_name = "PANE_ID")]
    pane: Option<u64>,

    /// What happens to the pane's Git worktree. `keep` leaves it on disk.
    /// `remove` deletes it when clean, merged and unused. `remove-and-branch`
    /// also deletes its creation branch when that is safe. Only worktrees
    /// Ilium created can be removed.
    #[arg(long, value_enum, default_value_t = WorktreeChoice::Keep)]
    worktree: WorktreeChoice,

    /// How long to wait for the server's answer.
    #[arg(long, default_value_t = 120)]
    timeout_seconds: u64,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WorktreeChoice {
    Keep,
    Remove,
    RemoveAndBranch,
}

impl WorktreeChoice {
    fn disposition(self) -> WorkspaceDisposition {
        match self {
            Self::Keep => WorkspaceDisposition::Keep,
            Self::Remove => WorkspaceDisposition::RemoveWorktree,
            Self::RemoveAndBranch => WorkspaceDisposition::RemoveWorktreeAndBranch,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Keep => "keep",
            Self::Remove => "remove",
            Self::RemoveAndBranch => "remove-and-branch",
        }
    }
}

/// The server's answer, reduced to what the agent must act on.
#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    Closed,
    Removed,
    Blocked(Vec<String>),
    Uncertain(Vec<String>),
}

pub(crate) async fn close_pane(args: ClosePaneArgs) -> Result<(), CliError> {
    let request_id = next_request_id();
    let choice = args.worktree;
    let identity = match crate::pane_identity_from_env() {
        Ok(identity) => identity,
        Err(error) => {
            print_error(
                request_id,
                None,
                "not-inside-ilium-pane",
                &error.to_string(),
            );
            return Err(CliError::ExitStatus(2));
        }
    };
    let pane_id = args.pane.map(NodeId).unwrap_or(identity.pane_id);
    println!(
        "{}",
        json!({
            "type": "progress",
            "command": COMMAND_NAME,
            "request_id": request_id,
            "pane_id": pane_id.0,
            "worktree": choice.label(),
            "stage": "requesting-close",
        })
    );

    let mut attached =
        match pane_scan::attach_as(&identity.socket_path, &identity.session_name, None).await {
            Ok(attached) => attached,
            Err(reason) => {
                print_error(request_id, Some(pane_id), "session-unreachable", &reason);
                return Err(CliError::ExitStatus(2));
            }
        };
    if attached.tree.get(pane_id).is_none() {
        attached.detach().await;
        print_error(
            request_id,
            Some(pane_id),
            "pane-not-found",
            "no pane with this id in this session",
        );
        return Err(CliError::ExitStatus(2));
    }

    let request = ClientRequest::ClosePaneWithWorkspaceDisposition {
        request_id,
        pane_id,
        disposition: choice.disposition(),
    };
    if attached.connection.requests.send(request).await.is_err() {
        print_error(
            request_id,
            Some(pane_id),
            "connection-closed",
            "the connection closed before the close request was sent",
        );
        return Err(CliError::ExitStatus(2));
    }

    let timeout = Duration::from_secs(args.timeout_seconds);
    let answer = tokio::time::timeout(
        timeout,
        wait_for_answer(&mut attached.connection, request_id, pane_id, choice),
    )
    .await;
    attached.detach().await;
    let outcome = match answer {
        Ok(Ok(outcome)) => outcome,
        Ok(Err(message)) => {
            print_error(request_id, Some(pane_id), "server-error", &message);
            return Err(CliError::ExitStatus(2));
        }
        Err(_elapsed) => Outcome::Uncertain(vec![format!(
            "no answer from the server within {} s; the close may still complete",
            timeout.as_secs()
        )]),
    };
    print_outcome(request_id, pane_id, choice, &outcome);
    match outcome {
        Outcome::Closed | Outcome::Removed => Ok(()),
        Outcome::Blocked(_) | Outcome::Uncertain(_) => Err(CliError::ExitStatus(1)),
    }
}

/// Reads events until the answer for `request_id` arrives. A `Keep` close is
/// confirmed by the pane leaving the tree. A worktree removal is confirmed
/// only by its own event, because the server closes the pane before it
/// reports the removal.
async fn wait_for_answer(
    connection: &mut ilium_client::connection::Connection,
    request_id: u64,
    pane_id: NodeId,
    choice: WorktreeChoice,
) -> Result<Outcome, String> {
    while let Some(event) = connection.events.recv().await {
        let (event, _event_retention) = event.into_parts();
        match event {
            ServerEvent::WorkspaceRemoved {
                request_id: answered,
                pane_id: removed,
            } if answered == request_id && removed == pane_id => return Ok(Outcome::Removed),
            ServerEvent::WorkspaceRemovalBlocked {
                request_id: answered,
                pane_id: refused,
                reasons,
            } if answered == request_id && refused == pane_id => {
                return Ok(classify_refusal(reasons));
            }
            ServerEvent::TreeSnapshot(tree)
                if choice == WorktreeChoice::Keep && tree.get(pane_id).is_none() =>
            {
                return Ok(Outcome::Closed);
            }
            ServerEvent::Error { message } => return Err(message),
            _ => {}
        }
    }
    Ok(Outcome::Uncertain(vec![
        "the connection closed before the server answered".to_owned(),
    ]))
}

fn classify_refusal(reasons: Vec<String>) -> Outcome {
    let uncertain = reasons
        .iter()
        .any(|reason| reason.starts_with(UNCERTAIN_REASON_PREFIX));
    if uncertain {
        Outcome::Uncertain(reasons)
    } else {
        Outcome::Blocked(reasons)
    }
}

fn print_outcome(request_id: u64, pane_id: NodeId, choice: WorktreeChoice, outcome: &Outcome) {
    let (record_type, outcome_name, reasons) = match outcome {
        Outcome::Closed => ("result", "closed", Vec::new()),
        Outcome::Removed => ("result", "removed", Vec::new()),
        Outcome::Blocked(reasons) => ("blocked", "blocked", reasons.clone()),
        Outcome::Uncertain(reasons) => ("uncertain", "uncertain", reasons.clone()),
    };
    println!(
        "{}",
        json!({
            "type": record_type,
            "command": COMMAND_NAME,
            "request_id": request_id,
            "pane_id": pane_id.0,
            "worktree": choice.label(),
            "outcome": outcome_name,
            "reasons": reasons,
        })
    );
}

fn print_error(request_id: u64, pane_id: Option<NodeId>, code: &str, message: &str) {
    println!(
        "{}",
        json!({
            "type": "error",
            "command": COMMAND_NAME,
            "request_id": request_id,
            "pane_id": pane_id.map(|id| id.0),
            "code": code,
            "message": message,
        })
    );
}

fn next_request_id() -> u64 {
    let unix_nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos() as u64);
    let request_id = unix_nanos ^ u64::from(std::process::id()).rotate_left(32);
    request_id.max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worktree_choices_map_to_server_dispositions() {
        assert_eq!(
            WorktreeChoice::Keep.disposition(),
            WorkspaceDisposition::Keep
        );
        assert_eq!(
            WorktreeChoice::Remove.disposition(),
            WorkspaceDisposition::RemoveWorktree
        );
        assert_eq!(
            WorktreeChoice::RemoveAndBranch.disposition(),
            WorkspaceDisposition::RemoveWorktreeAndBranch
        );
    }

    #[test]
    fn refusal_with_uncertain_prefix_is_uncertain_not_blocked() {
        let reasons = vec![format!("{UNCERTAIN_REASON_PREFIX}: git reported an error")];
        assert!(matches!(classify_refusal(reasons), Outcome::Uncertain(_)));
        assert!(matches!(
            classify_refusal(vec!["worktree has uncommitted changes".into()]),
            Outcome::Blocked(_)
        ));
    }

    #[test]
    fn request_ids_are_never_zero() {
        assert_ne!(next_request_id(), 0);
    }
}

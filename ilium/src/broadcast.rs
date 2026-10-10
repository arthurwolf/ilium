//! `ilium broadcast`: sends one message to every agent, in every running
//! session on this machine, that passes the shared pane selection (see
//! `pane_filter`).
//!
//! Delivery is the same as the client's "Send message to all": the text is
//! inserted into each agent's composer and submitted with Enter
//! (`ClientRequest::SubmitTerminalText`), one pane at a time, and counts as
//! delivered once the server reports the prompt submitted
//! (`ServerEvent::PanePromptSubmitted`). With `--when-idle`, busy agents get
//! the message through their prompt queue instead
//! (`ClientRequest::EnqueuePrompt`), so it is sent when their current turn
//! finishes. Only existing requests are used, so servers started by an older
//! build receive broadcasts too.
//!
//! Recipients are live agent panes only; shells, editors, boards and agents
//! that are not running are never typed into. The pane running this command
//! is skipped unless `--include-self` is given.
//!
//! Output is JSONL on stdout: a `progress` record, a `warning` per session
//! that could not be read, one `result` per recipient (`outcome` is
//! `delivered`, `queued`, `failed`, `skipped` or, with `--dry-run`,
//! `would-send`/`would-queue`), then one `summary`. The exit status is 1 when
//! nothing was selected or any delivery or session failed.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::Args;
use ilium_client::connection::Connection;
use ilium_core::{NodeId, NodeKind, PromptQueueDelivery, Tree};
use ilium_ipc::{ClientRequest, PromptSubmissionSource, ServerEvent};
use serde_json::json;

use crate::pane_filter::{PaneFacts, PaneFilterArgs, PaneKind};
use crate::pane_scan::{self, CallerPane};
use crate::panes::unreachable_warning;
use crate::CliError;

const COMMAND_NAME: &str = "broadcast";
/// The token that reads the message from standard input instead.
const STDIN_TOKEN: &str = "-";

#[derive(Args, Debug)]
pub(crate) struct BroadcastArgs {
    /// The message. Several words are joined with spaces; a lone `-` reads
    /// the whole message from standard input. Multi-line messages need
    /// agents that accept bracketed paste (Claude Code and Codex do).
    #[arg(value_name = "MESSAGE", required_unless_present = "file")]
    message: Vec<String>,

    /// Read the message from this file instead.
    #[arg(long, conflicts_with = "message")]
    file: Option<PathBuf>,

    /// Send now only to idle agents; queue the message for busy ones so it is
    /// sent when their current turn finishes, instead of typing into a
    /// running turn.
    #[arg(long)]
    when_idle: bool,

    /// List the recipients and what would happen, without sending anything.
    #[arg(long)]
    dry_run: bool,

    /// Also send to the pane running this command.
    #[arg(long)]
    include_self: bool,

    /// How long to wait for each pane's delivery confirmation.
    #[arg(long, default_value_t = 15, value_parser = clap::value_parser!(u64).range(1..=600))]
    timeout_seconds: u64,

    #[command(flatten)]
    filter: PaneFilterArgs,
}

/// What happens, or happened, to one recipient.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Outcome {
    Delivered,
    Queued,
    WouldSend,
    WouldQueue,
    Skipped(String),
    Failed(String),
}

impl Outcome {
    fn name(&self) -> &'static str {
        match self {
            Self::Delivered => "delivered",
            Self::Queued => "queued",
            Self::WouldSend => "would-send",
            Self::WouldQueue => "would-queue",
            Self::Skipped(_) => "skipped",
            Self::Failed(_) => "failed",
        }
    }

    fn reason(&self) -> Option<&str> {
        match self {
            Self::Skipped(reason) | Self::Failed(reason) => Some(reason),
            _ => None,
        }
    }
}

/// How one recipient receives the message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Delivery {
    Now,
    Queue,
}

#[derive(Debug, Default)]
struct Tally {
    recipients: usize,
    delivered: usize,
    queued: usize,
    planned: usize,
    skipped: usize,
    failed: usize,
}

impl Tally {
    fn count(&mut self, outcome: &Outcome) {
        match outcome {
            Outcome::Delivered => self.delivered += 1,
            Outcome::Queued => self.queued += 1,
            Outcome::WouldSend | Outcome::WouldQueue => self.planned += 1,
            Outcome::Skipped(_) => self.skipped += 1,
            Outcome::Failed(_) => self.failed += 1,
        }
        if !matches!(outcome, Outcome::Skipped(_)) {
            self.recipients += 1;
        }
    }
}

pub(crate) async fn broadcast(args: BroadcastArgs, cwd: &Path) -> Result<(), CliError> {
    let prepared = read_message(&args, &mut std::io::stdin().lock())
        .and_then(|message| args.filter.compile(cwd).map(|filter| (message, filter)));
    let (message, filter) = match prepared {
        Ok(prepared) => prepared,
        Err(message) => {
            println!(
                "{}",
                json!({"type": "error", "command": COMMAND_NAME, "code": "invalid-request", "message": message})
            );
            return Err(CliError::ExitStatus(2));
        }
    };
    let caller = CallerPane::from_env();
    let sockets = pane_scan::live_sockets()?;
    println!(
        "{}",
        json!({
            "type": "progress",
            "command": COMMAND_NAME,
            "stage": if args.dry_run { "planning" } else { "sending" },
            "sessions": sockets.len(),
            "message_bytes": message.len(),
        })
    );

    let timeout = Duration::from_secs(args.timeout_seconds);
    let mut tally = Tally::default();
    let mut unreachable = 0_usize;
    for socket in &sockets {
        let mut attached = match pane_scan::attach(socket).await {
            Ok(attached) => attached,
            Err(reason) => {
                unreachable += 1;
                println!(
                    "{}",
                    unreachable_warning(COMMAND_NAME, &socket.socket_path, &reason)
                );
                continue;
            }
        };
        let recipients: Vec<PaneFacts> = attached
            .panes(caller.as_ref())
            .into_iter()
            .filter(|pane| pane.kind == PaneKind::Agent && filter.accepts(pane))
            .collect();
        for pane in recipients {
            let outcome = if pane.is_self && !args.include_self {
                Outcome::Skipped("the pane running this command (use --include-self)".to_owned())
            } else {
                let delivery = choose_delivery(&pane, args.when_idle);
                if args.dry_run {
                    match delivery {
                        Delivery::Now => Outcome::WouldSend,
                        Delivery::Queue => Outcome::WouldQueue,
                    }
                } else {
                    deliver(
                        &mut attached.connection,
                        &attached.tree,
                        pane.pane_id,
                        &message,
                        delivery,
                        timeout,
                    )
                    .await
                }
            };
            tally.count(&outcome);
            println!("{}", result_record(&pane, &outcome));
        }
        attached.detach().await;
    }

    let ok = tally.recipients > 0 && tally.failed == 0 && unreachable == 0;
    println!(
        "{}",
        json!({
            "type": "summary",
            "command": COMMAND_NAME,
            "ok": ok,
            "dry_run": args.dry_run,
            "sessions": sockets.len(),
            "unreachable_sessions": unreachable,
            "recipients": tally.recipients,
            "delivered": tally.delivered,
            "queued": tally.queued,
            "planned": tally.planned,
            "skipped": tally.skipped,
            "failed": tally.failed,
        })
    );
    if ok {
        Ok(())
    } else {
        Err(CliError::ExitStatus(1))
    }
}

/// The message from the positional words, `-` (standard input) or `--file`,
/// with Windows line endings and a trailing newline removed.
fn read_message(args: &BroadcastArgs, stdin: &mut impl Read) -> Result<String, String> {
    let raw = if let Some(path) = &args.file {
        std::fs::read_to_string(path)
            .map_err(|error| format!("cannot read message file {path:?}: {error}"))?
    } else if args.message.len() == 1 && args.message[0] == STDIN_TOKEN {
        let mut text = String::new();
        stdin
            .read_to_string(&mut text)
            .map_err(|error| format!("cannot read the message from standard input: {error}"))?;
        text
    } else {
        args.message.join(" ")
    };
    let message = raw.replace("\r\n", "\n");
    let message = message.trim_end_matches('\n');
    if message.trim().is_empty() {
        return Err("the message is empty".to_owned());
    }
    Ok(message.to_owned())
}

fn choose_delivery(pane: &PaneFacts, when_idle: bool) -> Delivery {
    let is_idle = pane.state.is_some_and(|state| state.is_idle());
    if when_idle && !is_idle {
        Delivery::Queue
    } else {
        Delivery::Now
    }
}

async fn deliver(
    connection: &mut Connection,
    tree: &Tree,
    pane_id: NodeId,
    message: &str,
    delivery: Delivery,
    timeout: Duration,
) -> Outcome {
    let request = match delivery {
        // The client's "Send message to all" uses the same source: the text
        // is the user's own message, typed on their behalf.
        Delivery::Now => ClientRequest::SubmitTerminalText {
            pane_id,
            text: message.to_owned(),
            source: PromptSubmissionSource::Keyboard,
        },
        Delivery::Queue => ClientRequest::EnqueuePrompt {
            pane_id,
            text: message.to_owned(),
            delivery: PromptQueueDelivery::Once,
        },
    };
    if connection.requests.send(request).await.is_err() {
        return Outcome::Failed("the connection closed before the message was sent".to_owned());
    }
    let already_queued = queued_copies(tree, pane_id, message);
    let confirmation = tokio::time::timeout(timeout, async {
        while let Some(event) = connection.events.recv().await {
            let (event, _event_retention) = event.into_parts();
            match (delivery, event) {
                (
                    Delivery::Now,
                    ServerEvent::PanePromptSubmitted {
                        pane_id: submitted,
                        source: PromptSubmissionSource::Keyboard,
                    },
                ) if submitted == pane_id => return Outcome::Delivered,
                (Delivery::Queue, ServerEvent::TreeSnapshot(tree))
                | (Delivery::Queue, ServerEvent::PaneStateSnapshot { tree, .. })
                    if queued_copies(&tree, pane_id, message) > already_queued =>
                {
                    return Outcome::Queued;
                }
                (_, ServerEvent::Error { message }) => return Outcome::Failed(message),
                _ => {}
            }
        }
        Outcome::Failed("the server closed the connection before confirming".to_owned())
    })
    .await;
    confirmation.unwrap_or_else(|_| {
        Outcome::Failed(format!(
            "no confirmation within {} s; the message may still arrive",
            timeout.as_secs()
        ))
    })
}

/// How many copies of `message` wait in the pane's prompt queue.
fn queued_copies(tree: &Tree, pane_id: NodeId, message: &str) -> usize {
    match tree.get(pane_id).map(|node| &node.kind) {
        Some(NodeKind::Pane { prompt_queue, .. }) => prompt_queue
            .iter()
            .filter(|queued| queued.text == message)
            .count(),
        _ => 0,
    }
}

fn result_record(pane: &PaneFacts, outcome: &Outcome) -> serde_json::Value {
    let mut record = pane.to_json();
    record["type"] = json!("result");
    record["command"] = json!(COMMAND_NAME);
    record["outcome"] = json!(outcome.name());
    if let Some(reason) = outcome.reason() {
        record["reason"] = json!(reason);
    }
    record
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pane_filter::AgentStateName;
    use clap::Parser;

    #[derive(Parser)]
    struct Harness {
        #[command(flatten)]
        args: BroadcastArgs,
    }

    fn parse(arguments: &[&str]) -> BroadcastArgs {
        Harness::try_parse_from(std::iter::once("broadcast").chain(arguments.iter().copied()))
            .expect("valid arguments")
            .args
    }

    #[test]
    fn message_words_join_and_stdin_and_line_endings_normalise() {
        let words = parse(&["please", "re-read", "the rules"]);
        assert_eq!(
            read_message(&words, &mut std::io::empty()).unwrap(),
            "please re-read the rules"
        );
        let stdin = parse(&["-"]);
        assert_eq!(
            read_message(&stdin, &mut "line one\r\nline two\r\n".as_bytes()).unwrap(),
            "line one\nline two"
        );
        let empty = parse(&["  "]);
        assert!(read_message(&empty, &mut std::io::empty()).is_err());
    }

    #[test]
    fn a_message_or_file_is_required_and_selection_flags_parse() {
        assert!(Harness::try_parse_from(["broadcast"]).is_err());
        let args = parse(&[
            "--project",
            "ilium,/w/lumen",
            "--regex",
            "review|audit",
            "--invert",
            "--agent",
            "codex",
            "--state",
            "idle,working",
            "--when-idle",
            "hello",
        ]);
        assert_eq!(args.filter.projects, vec!["ilium", "/w/lumen"]);
        assert_eq!(args.filter.regexes, vec!["review|audit"]);
        assert!(args.filter.invert && args.when_idle && !args.dry_run);
        assert_eq!(args.filter.states.len(), 2);
        assert_eq!(args.message, vec!["hello"]);
    }

    #[test]
    fn when_idle_queues_only_for_busy_agents() {
        let mut pane = PaneFacts {
            session_name: "default".to_owned(),
            pane_id: NodeId(4),
            name: "agent".to_owned(),
            short_name: None,
            project: None,
            cwd: None,
            kind: PaneKind::Agent,
            agent: Some("Codex".to_owned()),
            state: Some(AgentStateName::Working),
            is_self: false,
        };
        assert_eq!(choose_delivery(&pane, false), Delivery::Now);
        assert_eq!(choose_delivery(&pane, true), Delivery::Queue);
        pane.state = Some(AgentStateName::Done);
        assert_eq!(choose_delivery(&pane, true), Delivery::Now);
    }

    #[test]
    fn tally_counts_skips_apart_from_recipients() {
        let mut tally = Tally::default();
        for outcome in [
            Outcome::Delivered,
            Outcome::Queued,
            Outcome::Failed("x".to_owned()),
            Outcome::Skipped("self".to_owned()),
        ] {
            tally.count(&outcome);
        }
        assert_eq!(tally.recipients, 3);
        assert_eq!(
            (tally.delivered, tally.queued, tally.failed, tally.skipped),
            (1, 1, 1, 1)
        );
    }
}

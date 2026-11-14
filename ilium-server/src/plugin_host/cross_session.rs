//! Connections from this server to the sessions of every project on the machine
//! (`ilium:host/sessions`, capability `sessions.cross`).
//!
//! Each project session has its own server and socket, so there is no registry to ask: the live
//! sockets of the session socket directory are the sessions. A request attaches to one of them
//! the way `ilium new-pane` does, reads its tree, does its one job and detaches:
//!
//! - `list-panes` answers one record per pane (`pane_id`, `name`, `short_name`, `kind`,
//!   `agent`, `state`, `project`, `cwd`, `is_self`);
//! - `broadcast` types a message into the listed panes (`targets`: `{pane_id, queue}`) through
//!   the requests the client's "Send message to all" uses, and answers one outcome per pane.
//!
//! The answer to a request is its JSON value, or `{"error": "<message>"}` when the session
//! could not be reached.

use std::path::{Path, PathBuf};
use std::time::Duration;

use ilium_core::{AgentTurn, NodeId, NodeKind, PaneStatus, PromptQueueDelivery, Tree};
use ilium_ipc::{read_frame, write_frame, ClientRequest, PromptSubmissionSource, ServerEvent};
use ilium_plugin_api::kernel::LiveSessionInfo;
use ilium_transport::{Liveness, SessionEndpoint, SessionReadHalf, SessionWriteHalf};
use serde_json::{json, Value};

/// How long one session may take to deliver its attach-time tree.
const ATTACH_TIMEOUT: Duration = Duration::from_secs(10);
/// How long a delivery waits for the pane's confirmation unless the request says otherwise.
const DEFAULT_DELIVERY_TIMEOUT: Duration = Duration::from_secs(15);
/// Most panes one `broadcast` request may name.
const MAX_TARGETS: usize = 512;
/// Characters of a session name, as the CLI validates them.
const MAX_SESSION_NAME_BYTES: usize = 48;
/// Hex digits of the project digest inside a socket file name.
const DIGEST_HEX_DIGITS: usize = 12;

/// One live session endpoint with the names it may carry.
#[derive(Debug, Clone)]
pub(super) struct LiveSocket {
    pub(super) socket_path: PathBuf,
    pub(super) socket_id: String,
    /// Session names the file name can encode, most likely first. Attaching with a wrong one
    /// is refused by the server, so they are tried in order.
    pub(super) candidates: Vec<String>,
}

/// Every live session socket, sorted by path. Dead endpoints are skipped, never removed.
pub(super) fn live_sockets() -> Vec<LiveSocket> {
    let Ok(directory) = ilium_platform::runtime_dir::session_socket_directory() else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut sockets: Vec<LiveSocket> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "sock"))
        .filter_map(|socket_path| {
            let socket_id = socket_path.file_stem()?.to_str()?.to_owned();
            let candidates = session_name_candidates(&socket_id);
            if candidates.is_empty()
                || SessionEndpoint::from_path(&socket_path).probe_liveness() != Liveness::Live
            {
                return None;
            }
            Some(LiveSocket {
                socket_path,
                socket_id,
                candidates,
            })
        })
        .collect();
    sockets.sort_by(|left, right| left.socket_path.cmp(&right.socket_path));
    sockets
}

/// The live sessions as the plugin host lists them; `own_socket` marks this server's own.
pub(super) fn live_sessions(
    own_socket: &Path,
    own_name: &str,
    own_project: &str,
) -> Vec<LiveSessionInfo> {
    let own_id = own_socket
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(str::to_owned);
    let mut sessions: Vec<LiveSessionInfo> = live_sockets()
        .into_iter()
        .map(|socket| {
            let is_self = own_id.as_deref() == Some(socket.socket_id.as_str());
            LiveSessionInfo {
                name: if is_self {
                    own_name.to_owned()
                } else {
                    socket.candidates.first().cloned().unwrap_or_default()
                },
                project: if is_self {
                    own_project.to_owned()
                } else {
                    String::new()
                },
                socket_id: socket.socket_id,
                is_self,
            }
        })
        .collect();
    if !sessions.iter().any(|session| session.is_self) {
        // The own socket is bound before plugins start, so this only fills a gap while the
        // directory is unreadable; the session can always talk about itself.
        sessions.insert(
            0,
            LiveSessionInfo {
                name: own_name.to_owned(),
                project: own_project.to_owned(),
                socket_id: own_id.unwrap_or_else(|| own_name.to_owned()),
                is_self: true,
            },
        );
    }
    sessions
}

/// The session names a socket key (`<slug>-<12 hex digest>-<session>`) can end with. Both the
/// slug and the session name may contain `-` and hex digits, so every digest-shaped split is a
/// candidate; the right-most split (the shortest name) comes first.
fn session_name_candidates(socket_key: &str) -> Vec<String> {
    let bytes = socket_key.as_bytes();
    let mut candidates = Vec::new();
    for start in (0..bytes.len()).rev() {
        let digest_end = start + 1 + DIGEST_HEX_DIGITS;
        if bytes[start] != b'-' || digest_end >= bytes.len() || bytes[digest_end] != b'-' {
            continue;
        }
        let is_digest = bytes[start + 1..digest_end]
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte));
        if !is_digest {
            continue;
        }
        let session_name = &socket_key[digest_end + 1..];
        let is_valid = !session_name.is_empty()
            && session_name.len() <= MAX_SESSION_NAME_BYTES
            && session_name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_');
        if is_valid {
            candidates.push(session_name.to_owned());
        }
    }
    candidates
}

/// One attached session: its tree and the open halves of its connection.
struct Attached {
    read: SessionReadHalf,
    write: SessionWriteHalf,
    tree: Tree,
}

async fn attach(socket: &LiveSocket) -> Result<Attached, String> {
    let mut last_error = String::from("no session name candidates");
    for name in &socket.candidates {
        match attach_as(&socket.socket_path, name).await {
            Ok(attached) => return Ok(attached),
            Err(error) => last_error = error,
        }
    }
    Err(last_error)
}

async fn attach_as(socket_path: &Path, session_name: &str) -> Result<Attached, String> {
    let stream = SessionEndpoint::from_path(socket_path)
        .connect()
        .await
        .map_err(|error| format!("cannot connect: {error}"))?;
    let (mut read, mut write) = stream.into_split();
    write_frame(
        &mut write,
        &ClientRequest::AttachInteractive {
            session: session_name.to_owned(),
            hello: Default::default(),
        },
    )
    .await
    .map_err(|error| format!("cannot attach: {error}"))?;
    let initial = tokio::time::timeout(ATTACH_TIMEOUT, async {
        let mut tree = None;
        loop {
            match read_frame::<ServerEvent, _>(&mut read).await {
                Ok(ServerEvent::PaneStateSnapshot { tree: snapshot, .. }) => tree = Some(snapshot),
                Ok(ServerEvent::TreeSnapshot(snapshot)) if tree.is_none() => tree = Some(snapshot),
                Ok(ServerEvent::InitialStateSyncComplete) => {
                    return tree
                        .ok_or_else(|| "session attach completed without a tree".to_owned());
                }
                Ok(ServerEvent::Error { message }) => return Err(message),
                Ok(_) => {}
                Err(_) => {
                    return Err("connection closed before the session attach completed".to_owned());
                }
            }
        }
    })
    .await;
    match initial {
        Ok(Ok(tree)) => Ok(Attached { read, write, tree }),
        Ok(Err(message)) => {
            let _ = write_frame(&mut write, &ClientRequest::Detach).await;
            Err(message)
        }
        Err(_elapsed) => {
            let _ = write_frame(&mut write, &ClientRequest::Detach).await;
            Err(format!(
                "no session state within {} s",
                ATTACH_TIMEOUT.as_secs()
            ))
        }
    }
}

/// Runs one command against one session and returns the value of its `ok` answer.
pub(super) async fn run(
    socket: LiveSocket,
    command: String,
    arguments: Value,
) -> Result<Value, String> {
    let mut attached = attach(&socket).await?;
    let answer = match command.as_str() {
        "list-panes" => Ok(Value::Array(pane_records(&attached.tree))),
        "broadcast" => broadcast(&mut attached, &arguments).await,
        other => Err(format!("unknown cross-session command {other}")),
    };
    let _ = write_frame(&mut attached.write, &ClientRequest::Detach).await;
    answer
}

/// One record per pane of the tree, in tree order.
fn pane_records(tree: &Tree) -> Vec<Value> {
    tree.pane_ids_in_tree_order()
        .into_iter()
        .filter_map(|pane_id| {
            let node = tree.get(pane_id)?;
            let NodeKind::Pane { status, .. } = &node.kind else {
                return None;
            };
            let state = status.agent_state().map(|agent| match agent.turn {
                AgentTurn::Working => "working",
                AgentTurn::WaitingApproval => "waiting-approval",
                AgentTurn::WaitingSubagents => "waiting-subagents",
                AgentTurn::Settling => "settling",
                AgentTurn::Idle if agent.completion_unread => "done",
                AgentTurn::Idle => "idle",
            });
            Some(json!({
                "pane_id": pane_id.0,
                "name": node.name,
                "short_name": node.short_name,
                "kind": pane_kind_name(status),
                "agent": status.known_agent_state().map(|agent| agent.class.label().to_owned()),
                "state": state,
                "project": tree.project_path_for(pane_id).map(|path| path.to_string_lossy().into_owned()),
                "cwd": tree.pane_cwd(pane_id).map(|path| path.to_string_lossy().into_owned()),
                "is_self": false,
            }))
        })
        .collect()
}

fn pane_kind_name(status: &PaneStatus) -> &'static str {
    match status {
        PaneStatus::Agent(_) => "agent",
        PaneStatus::PlainShell => "shell",
        PaneStatus::Editor { .. } => "editor",
        PaneStatus::Board => "board",
        PaneStatus::AgentUnavailable(_) => "unavailable-agent",
        PaneStatus::Plugin { .. } => "plugin",
    }
}

/// Types the message into every listed pane and answers one outcome per pane.
async fn broadcast(attached: &mut Attached, arguments: &Value) -> Result<Value, String> {
    let text = arguments
        .get("text")
        .and_then(Value::as_str)
        .filter(|text| !text.trim().is_empty())
        .ok_or_else(|| "broadcast needs a non-empty text".to_owned())?;
    let targets = arguments
        .get("targets")
        .and_then(Value::as_array)
        .ok_or_else(|| "broadcast needs a targets list".to_owned())?;
    if targets.len() > MAX_TARGETS {
        return Err(format!("broadcast takes at most {MAX_TARGETS} targets"));
    }
    let timeout = arguments
        .get("timeout_seconds")
        .and_then(Value::as_u64)
        .map_or(DEFAULT_DELIVERY_TIMEOUT, |seconds| {
            Duration::from_secs(seconds.clamp(1, 600))
        });
    let mut outcomes = Vec::with_capacity(targets.len());
    for target in targets {
        let Some(pane_id) = target.get("pane_id").and_then(Value::as_u64).map(NodeId) else {
            continue;
        };
        let queue = target.get("queue").and_then(Value::as_bool).unwrap_or(false);
        let outcome = deliver(attached, pane_id, text, queue, timeout).await;
        outcomes.push(match outcome {
            Ok(name) => json!({"pane_id": pane_id.0, "outcome": name}),
            Err(reason) => json!({"pane_id": pane_id.0, "outcome": "failed", "reason": reason}),
        });
    }
    Ok(Value::Array(outcomes))
}

/// Sends the message to one pane and waits for the server to confirm. `Ok` carries the outcome
/// name (`delivered` or `queued`).
async fn deliver(
    attached: &mut Attached,
    pane_id: NodeId,
    message: &str,
    queue: bool,
    timeout: Duration,
) -> Result<&'static str, String> {
    let request = if queue {
        ClientRequest::EnqueuePrompt {
            pane_id,
            text: message.to_owned(),
            delivery: PromptQueueDelivery::Once,
        }
    } else {
        // The client's "Send message to all" uses the same source: the text is the user's own
        // message, typed on their behalf.
        ClientRequest::SubmitTerminalText {
            pane_id,
            text: message.to_owned(),
            source: PromptSubmissionSource::Keyboard,
        }
    };
    let already_queued = queued_copies(&attached.tree, pane_id, message);
    write_frame(&mut attached.write, &request)
        .await
        .map_err(|_| "the connection closed before the message was sent".to_owned())?;
    let read = &mut attached.read;
    let confirmation = tokio::time::timeout(timeout, async {
        loop {
            match read_frame::<ServerEvent, _>(read).await {
                Ok(ServerEvent::PanePromptSubmitted {
                    pane_id: submitted,
                    source: PromptSubmissionSource::Keyboard,
                }) if !queue && submitted == pane_id => return Ok("delivered"),
                Ok(
                    ServerEvent::TreeSnapshot(tree)
                    | ServerEvent::PaneStateSnapshot { tree, .. },
                ) if queue && queued_copies(&tree, pane_id, message) > already_queued => {
                    return Ok("queued");
                }
                Ok(ServerEvent::Error { message }) => return Err(message),
                Ok(_) => {}
                Err(_) => {
                    return Err("the server closed the connection before confirming".to_owned());
                }
            }
        }
    })
    .await;
    confirmation.unwrap_or_else(|_| {
        Err(format!(
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

#[cfg(test)]
mod tests {
    use super::session_name_candidates;

    #[test]
    fn the_shortest_session_name_comes_first() {
        let candidates = session_name_candidates("home-me-dev-ilium-0123456789ab-review");
        assert_eq!(candidates.first().map(String::as_str), Some("review"));
    }

    #[test]
    fn a_key_without_a_digest_has_no_candidates() {
        assert!(session_name_candidates("not-a-socket-key").is_empty());
    }

    #[test]
    fn a_session_name_with_a_digest_shaped_prefix_offers_both_splits() {
        let candidates = session_name_candidates("proj-0123456789ab-fedcba987654-main");
        assert_eq!(candidates.first().map(String::as_str), Some("main"));
        assert!(candidates.contains(&"fedcba987654-main".to_owned()));
    }
}

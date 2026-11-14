//! Finds the panes of every running Ilium session on this machine, for the
//! commands that act across projects (`ilium ctl`, `ilium close-pane`).
//!
//! Each project session has its own server and socket, so there is no
//! central registry to ask. The scan lists the live sockets in the session
//! socket directory, attaches to each one in turn (the same metadata-only
//! attach `new-pane` uses), and reads its tree. Sessions are visited one at a
//! time and each connection is closed before the next opens, so one CLI
//! process never owns more than one connection.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use ilium::session::{self, LiveSessionSocket};
use ilium_client::connection::Connection;
use ilium_core::{NodeId, Tree};
use ilium_ipc::{ClientRequest, ServerEvent};

use crate::CliError;

/// How long one session may take to deliver its attach-time tree.
const ATTACH_TIMEOUT: Duration = Duration::from_secs(10);

/// One attached session: its tree and the open connection for follow-up
/// requests. Call [`AttachedSession::detach`] when done.
pub(crate) struct AttachedSession {
    pub(crate) socket_path: PathBuf,
    pub(crate) session_name: String,
    pub(crate) connection: Connection,
    pub(crate) tree: Tree,
}

impl AttachedSession {
    /// Requests only selected shell screens; never subscribes to retained journals.
    /// One request/reply at a time bounds memory and leaves per-pane failures explicit.
    pub(crate) async fn collect_shell_screens(
        &mut self,
        pane_ids: &[NodeId],
        evidence: &mut PaneEvidence,
    ) {
        for &pane_id in pane_ids.iter().take(64) {
            let capture = tokio::time::timeout(Duration::from_secs(2), async {
                self.connection
                    .requests
                    .send(ClientRequest::GetPaneRecapScreen { pane_id })
                    .await
                    .map_err(|_| "screen request unavailable".to_owned())?;
                while let Some(event) = self.connection.events.recv().await {
                    let (event, _retention) = event.into_parts();
                    if let ServerEvent::PaneRecapScreen {
                        pane_id: replied,
                        result,
                    } = event
                    {
                        if replied == pane_id {
                            return result;
                        }
                    }
                }
                Err("screen connection closed".to_owned())
            })
            .await
            .unwrap_or_else(|_| Err("screen capture timed out".to_owned()));
            match capture {
                Ok(text) => {
                    evidence.screen_text.insert(pane_id, text);
                }
                Err(message) => {
                    evidence.screen_errors.insert(pane_id, message);
                }
            }
        }
    }

    pub(crate) async fn detach(self) {
        let _ = self.connection.requests.send(ClientRequest::Detach).await;
    }
}

/// Every live session socket, or a typed error when the socket directory
/// cannot be read.
pub(crate) fn live_sockets() -> Result<Vec<LiveSessionSocket>, CliError> {
    session::list_live_session_sockets()
}

/// Verified attach-time agent identities and selected current shell screens.
/// No terminal journals are retained by one-shot recap consumers.
#[derive(Default)]
pub(crate) struct PaneEvidence {
    pub(crate) session_ids: HashMap<NodeId, String>,
    pub(crate) screen_text: HashMap<NodeId, String>,
    pub(crate) screen_errors: HashMap<NodeId, String>,
}

pub(crate) async fn attach(socket: &LiveSessionSocket) -> Result<AttachedSession, String> {
    attach_inner(socket, None).await
}

async fn attach_inner(
    socket: &LiveSessionSocket,
    mut evidence: Option<&mut PaneEvidence>,
) -> Result<AttachedSession, String> {
    let mut last_error = String::from("no session name candidates");
    for session_name in &socket.session_name_candidates {
        if let Some(evidence) = evidence.as_deref_mut() {
            *evidence = PaneEvidence::default();
        }
        match attach_as(&socket.socket_path, session_name, evidence.as_deref_mut()).await {
            Ok(attached) => return Ok(attached),
            Err(error) => last_error = error,
        }
    }
    Err(last_error)
}

pub(crate) async fn attach_as(
    socket_path: &Path,
    session_name: &str,
    mut evidence: Option<&mut PaneEvidence>,
) -> Result<AttachedSession, String> {
    let mut connection = Connection::connect(socket_path, session_name.to_owned())
        .await
        .map_err(|error| format!("cannot connect: {error}"))?;
    let attach = tokio::time::timeout(ATTACH_TIMEOUT, async {
        let mut initial_tree = None;
        while let Some(event) = connection.events.recv().await {
            let (event, _event_retention) = event.into_parts();
            match event {
                ServerEvent::PaneStateSnapshot { tree, .. } => initial_tree = Some(tree),
                ServerEvent::TreeSnapshot(tree) if initial_tree.is_none() => {
                    initial_tree = Some(tree);
                }
                ServerEvent::InitialStateSyncComplete => {
                    return initial_tree
                        .ok_or_else(|| "session attach completed without a tree".to_owned());
                }
                ServerEvent::Error { message } => return Err(message),
                ServerEvent::PaneSessionIdResolved {
                    pane_id,
                    session_id,
                    ..
                } => {
                    if let Some(evidence) = evidence.as_deref_mut() {
                        evidence.session_ids.insert(pane_id, session_id);
                    }
                }
                _ => {}
            }
        }
        Err("connection closed before the session attach completed".to_owned())
    })
    .await;
    let tree = match attach {
        Ok(result) => result,
        Err(_elapsed) => Err(format!(
            "no session state within {} s",
            ATTACH_TIMEOUT.as_secs()
        )),
    };
    match tree {
        Ok(tree) => Ok(AttachedSession {
            socket_path: socket_path.to_path_buf(),
            session_name: session_name.to_owned(),
            connection,
            tree,
        }),
        Err(error) => {
            let _ = connection.requests.send(ClientRequest::Detach).await;
            Err(error)
        }
    }
}

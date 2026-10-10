//! Finds the panes of every running Ilium session on this machine, for the
//! commands that act across projects (`ilium panes`, `ilium broadcast`).
//!
//! Each project session has its own server and socket, so there is no
//! central registry to ask. The scan lists the live sockets in the session
//! socket directory, attaches to each one in turn (the same metadata-only
//! attach `new-pane` uses), and reads its tree. Sessions are visited one at a
//! time and each connection is closed before the next opens, so one CLI
//! process never owns more than one connection.

use std::path::{Path, PathBuf};
use std::time::Duration;

use ilium::session::{self, LiveSessionSocket};
use ilium_client::connection::Connection;
use ilium_core::{NodeId, Tree};
use ilium_ipc::{ClientRequest, ServerEvent};

use crate::pane_filter::{pane_facts, PaneFacts};
use crate::{pane_identity_from_env, CliError};

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
    /// The session's panes, with the calling pane marked.
    pub(crate) fn panes(&self, caller: Option<&CallerPane>) -> Vec<PaneFacts> {
        let mut panes = pane_facts(&self.tree, &self.session_name);
        if let Some(caller) = caller.filter(|caller| caller.is_in(&self.socket_path)) {
            for pane in &mut panes {
                pane.is_self = pane.pane_id == caller.pane_id;
            }
        }
        panes
    }

    pub(crate) async fn detach(self) {
        let _ = self.connection.requests.send(ClientRequest::Detach).await;
    }
}

/// The pane this command runs in, when it runs inside Ilium.
pub(crate) struct CallerPane {
    socket_path: PathBuf,
    pane_id: NodeId,
}

impl CallerPane {
    pub(crate) fn from_env() -> Option<Self> {
        let identity = pane_identity_from_env().ok()?;
        Some(Self {
            socket_path: canonical_or_raw(&identity.socket_path),
            pane_id: identity.pane_id,
        })
    }

    fn is_in(&self, socket_path: &Path) -> bool {
        canonical_or_raw(socket_path) == self.socket_path
    }
}

fn canonical_or_raw(path: &Path) -> PathBuf {
    ilium_platform::paths::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Every live session socket, or a typed error when the socket directory
/// cannot be read.
pub(crate) fn live_sockets() -> Result<Vec<LiveSessionSocket>, CliError> {
    session::list_live_session_sockets()
}

/// Attaches to one live socket, trying each candidate session name until
/// the server accepts one.
pub(crate) async fn attach(socket: &LiveSessionSocket) -> Result<AttachedSession, String> {
    let mut last_error = String::from("no session name candidates");
    for session_name in &socket.session_name_candidates {
        match attach_as(&socket.socket_path, session_name).await {
            Ok(attached) => return Ok(attached),
            Err(error) => last_error = error,
        }
    }
    Err(last_error)
}

async fn attach_as(socket_path: &Path, session_name: &str) -> Result<AttachedSession, String> {
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

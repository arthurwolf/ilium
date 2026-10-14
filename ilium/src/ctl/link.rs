//! Connecting to a session and carrying out one [`Operation`] on it.

use std::path::Path;
use std::time::{Duration, Instant};

use crate::session::{self, LiveSessionSocket};
use ilium_control::operation::WorktreeAction;
use ilium_control::Operation;
use ilium_core::{NodeId, Tree, ROOT_ID};
use ilium_ipc::{ClientRequest, ServerEvent};
use serde_json::{json, Value};

use super::args::Target;
use super::CtlFailure;
use crate::pane_scan::{self, AttachedSession};

/// How long to wait for the server to apply a structural change.
const CONFIRM_TIMEOUT: Duration = Duration::from_secs(30);
/// Without a tree event for this long, re-read the session to check progress.
const QUIET_BEFORE_RECHECK: Duration = Duration::from_millis(700);
/// How long an input-only request waits for a possible server error.
const INPUT_GRACE: Duration = Duration::from_millis(600);

/// An attached session plus what is needed to re-attach and check it.
pub(crate) struct Link {
    pub(crate) attached: AttachedSession,
    socket: LiveSessionSocket,
}

/// Resolves `target` to a live socket and attaches. `start_server` lets a
/// creating command start the project's session when it is not running yet.
pub(crate) async fn open(
    target: &Target,
    cwd: &Path,
    start_server: bool,
) -> Result<Link, CtlFailure> {
    let socket = resolve_socket(target, cwd, start_server).await?;
    let attached = pane_scan::attach(&socket)
        .await
        .map_err(|message| CtlFailure::no_session(format!("cannot attach: {message}")))?;
    Ok(Link { attached, socket })
}

async fn resolve_socket(
    target: &Target,
    cwd: &Path,
    start_server: bool,
) -> Result<LiveSessionSocket, CtlFailure> {
    if let Some(socket_path) = &target.socket {
        let wanted = ilium_platform::paths::canonicalize(socket_path)
            .unwrap_or_else(|_| socket_path.clone());
        let live =
            pane_scan::live_sockets().map_err(|error| CtlFailure::no_session(error.to_string()))?;
        return live
            .into_iter()
            .find(|socket| {
                ilium_platform::paths::canonicalize(&socket.socket_path)
                    .unwrap_or_else(|_| socket.socket_path.clone())
                    == wanted
            })
            .ok_or_else(|| {
                CtlFailure::no_session(format!("no live session at {}", socket_path.display()))
            });
    }
    if target.session.is_none() {
        if let Ok(identity) = crate::pane_identity_from_env() {
            return Ok(LiveSessionSocket {
                socket_path: identity.socket_path,
                session_name_candidates: vec![identity.session_name],
            });
        }
    }
    let name = target
        .session
        .as_deref()
        .unwrap_or(session::DEFAULT_SESSION_NAME);
    let project_session = session::resolve_project_session(cwd, name)
        .map_err(|error| CtlFailure::usage(error.to_string()))?;
    if !session::is_session_live(&project_session.socket_path) {
        if !start_server {
            return Err(CtlFailure::no_session(format!(
                "session {name:?} of {} is not running; `ilium ctl sessions` lists live ones",
                project_session.project_root.display()
            )));
        }
        session::ensure_server_running(&project_session)
            .await
            .map_err(|error| CtlFailure::no_session(error.to_string()))?;
    }
    Ok(LiveSessionSocket {
        socket_path: project_session.socket_path,
        session_name_candidates: vec![name.to_owned()],
    })
}

impl Link {
    pub(crate) fn tree(&self) -> &Tree {
        &self.attached.tree
    }

    pub(crate) fn session_name(&self) -> &str {
        &self.attached.session_name
    }

    pub(crate) async fn send(&self, request: ClientRequest) -> Result<(), CtlFailure> {
        self.attached
            .connection
            .requests
            .send(request)
            .await
            .map_err(|_| CtlFailure::no_session("the session closed the connection"))
    }

    /// Reads the session's tree over a fresh connection.
    pub(crate) async fn fresh_tree(&self) -> Result<Tree, CtlFailure> {
        let attached = pane_scan::attach(&self.socket).await.map_err(|message| {
            CtlFailure::no_session(format!("cannot re-read the session: {message}"))
        })?;
        let tree = attached.tree.clone();
        attached.detach().await;
        Ok(tree)
    }

    /// Replaces the cached tree with the session's current one.
    pub(crate) async fn refresh(&mut self) -> Result<(), CtlFailure> {
        self.attached.tree = self.fresh_tree().await?;
        Ok(())
    }

    pub(crate) async fn detach(self) {
        self.attached.detach().await;
    }

    /// Carries out `operation` and returns the confirmed result details.
    pub(crate) async fn apply(&mut self, operation: &Operation) -> Result<Value, CtlFailure> {
        let before = self.attached.tree.clone();
        if let Operation::Close { pane, worktree } = operation {
            if !matches!(worktree, WorktreeAction::Keep) {
                refuse_removal_from_inside_worktree(&before, *pane)?;
            }
        }
        let requests = operation.requests(&before, crate::next_progress_request_id())?;
        for request in requests {
            self.send(request).await?;
        }
        if operation.is_input_only() {
            return self.wait_for_rejection().await;
        }
        let deadline = Instant::now() + CONFIRM_TIMEOUT;
        let mut latest = before.clone();
        loop {
            if let Some(details) = operation.outcome(&before, &latest) {
                return Ok(details);
            }
            let now = Instant::now();
            if now >= deadline {
                return Err(CtlFailure::rejected(format!(
                    "the server did not confirm {} within {} s; inspect the tree before retrying",
                    operation.name(),
                    CONFIRM_TIMEOUT.as_secs()
                )));
            }
            let wait = QUIET_BEFORE_RECHECK.min(deadline - now);
            match tokio::time::timeout(wait, self.attached.connection.events.recv()).await {
                Ok(Some(event)) => {
                    let (event, _retention) = event.into_parts();
                    match event {
                        ServerEvent::Error { message } => {
                            return Err(CtlFailure::rejected(message))
                        }
                        ServerEvent::WorkspaceRemovalBlocked { reasons, .. } => {
                            return Err(CtlFailure::rejected(removal_refusal(&reasons)))
                        }
                        ServerEvent::TreeSnapshot(tree) => latest = tree,
                        _ => {}
                    }
                }
                Ok(None) => {
                    latest = self.fresh_tree().await?;
                    return operation.outcome(&before, &latest).ok_or_else(|| {
                        CtlFailure::rejected("the session closed before confirming the change")
                    });
                }
                Err(_quiet) => latest = self.fresh_tree().await?,
            }
        }
    }

    /// The server refuses a worktree removal while any same-user process has its
/// working directory inside the worktree, and the command asking for the
/// removal is such a process when it runs there. Refuse here with the reason
/// instead of sending a request that can only time out.
fn refuse_removal_from_inside_worktree(tree: &Tree, pane: u64) -> Result<(), CtlFailure> {
    let Some(workspace) = tree.pane_workspace(NodeId(pane)) else {
        return Ok(());
    };
    let Ok(current_directory) = std::env::current_dir() else {
        return Ok(());
    };
    let current_directory = std::fs::canonicalize(&current_directory).unwrap_or(current_directory);
    let worktree_root = std::fs::canonicalize(&workspace.worktree_root)
        .unwrap_or_else(|_| workspace.worktree_root.clone());
    if current_directory.starts_with(&worktree_root) {
        return Err(CtlFailure::rejected(format!(
            "this command runs inside the worktree {} that it would remove; run it from another directory",
            worktree_root.display()
        )));
    }
    Ok(())
}

/// Turns the server's refusal reasons into the one message the user reads.
fn removal_refusal(reasons: &[String]) -> String {
    if reasons.is_empty() {
        return "the server refused the worktree removal without giving a reason".into();
    }
    format!("the server refused the worktree removal: {}", reasons.join("; "))
}

/// Input requests change no tree; success means no error arrived shortly.
    async fn wait_for_rejection(&mut self) -> Result<Value, CtlFailure> {
        let deadline = Instant::now() + INPUT_GRACE;
        loop {
            let now = Instant::now();
            if now >= deadline {
                return Ok(json!({}));
            }
            match tokio::time::timeout(deadline - now, self.attached.connection.events.recv()).await
            {
                Ok(Some(event)) => {
                    let (event, _retention) = event.into_parts();
                    match event {
                        ServerEvent::Error { message } => {
                            return Err(CtlFailure::rejected(message));
                        }
                        ServerEvent::WorkspaceRemovalBlocked { reasons, .. } => {
                            return Err(CtlFailure::rejected(removal_refusal(&reasons)));
                        }
                        _ => {}
                    }
                }
                Ok(None) | Err(_) => return Ok(json!({})),
            }
        }
    }
}

/// The top-level project containing `node`.
pub(crate) fn owning_project(tree: &Tree, node: NodeId) -> Option<NodeId> {
    let mut current = node;
    loop {
        let parent = tree.parent_of(current)?;
        if parent == ROOT_ID {
            return Some(current);
        }
        current = parent;
    }
}

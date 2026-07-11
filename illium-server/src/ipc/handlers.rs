//! Translates each `illium_ipc::ClientRequest` variant into a mutation on
//! `ServerState`'s tree/pane registry, broadcasting the resulting
//! `ServerEvent` to every attached client for structural changes (tree
//! shape, pane status) or replying only to the requesting connection for
//! everything else (the initial `Attach` snapshot, request-specific
//! errors). See `crate::ipc::connection` for how the two reply channels
//! (`ServerState::events` broadcast vs. this connection's own `direct_tx`)
//! are wired together on the write side.

use std::path::PathBuf;
use std::sync::Arc;

use illium_core::{NodeId, PaneContentKind, Tree, TreeError};
use illium_ipc::{ClientRequest, NewPaneKind, ServerEvent};
use tokio::sync::mpsc;

use crate::mouse::to_crossterm_event;
use crate::pane::{PaneResource, TerminalOrigin};
use crate::state::ServerState;
use crate::{pane, persistence};

/// Handles one request from an attached client. Returns `true` when the
/// connection this request arrived on should close afterward (`Detach`,
/// `KillSession`) -- the caller (`crate::ipc::connection`) is what
/// actually stops reading further frames.
pub async fn handle_request(
    state: &Arc<ServerState>,
    request: ClientRequest,
    direct_tx: &mpsc::UnboundedSender<ServerEvent>,
) -> bool {
    match request {
        ClientRequest::Attach { session } => {
            handle_attach(state, &session, direct_tx).await;
            false
        }
        ClientRequest::NewPane { parent_group, kind } => {
            handle_new_pane(state, parent_group, kind, direct_tx).await;
            false
        }
        ClientRequest::ClosePane { pane_id } => {
            handle_close_pane(state, pane_id, direct_tx).await;
            false
        }
        ClientRequest::MoveNode { node_id, direction } => {
            handle_tree_mutation(state, direct_tx, |tree| {
                tree.move_node_one_step(node_id, direction).map(|_moved| ())
            })
            .await;
            false
        }
        ClientRequest::RenameNode { node_id, title } => {
            handle_tree_mutation(state, direct_tx, |tree| tree.rename_node(node_id, title)).await;
            false
        }
        ClientRequest::ResizePane {
            pane_id,
            rows,
            cols,
        } => {
            handle_resize_pane(state, pane_id, rows, cols, direct_tx).await;
            false
        }
        ClientRequest::KeyInput { pane_id, bytes } => {
            handle_key_input(state, pane_id, &bytes, direct_tx).await;
            false
        }
        ClientRequest::MouseInput {
            pane_id,
            kind,
            column,
            row,
            modifiers,
        } => {
            handle_mouse_input(state, pane_id, kind, column, row, modifiers, direct_tx).await;
            false
        }
        ClientRequest::Detach => true,
        ClientRequest::KillSession => {
            handle_kill_session(state).await;
            true
        }
    }
}

fn send_direct(direct_tx: &mpsc::UnboundedSender<ServerEvent>, event: ServerEvent) {
    // An error here only means this connection's writer task has already
    // ended (client disconnected mid-request); nothing left to do with the
    // reply.
    let _ = direct_tx.send(event);
}

fn send_direct_error(direct_tx: &mpsc::UnboundedSender<ServerEvent>, message: impl Into<String>) {
    send_direct(
        direct_tx,
        ServerEvent::Error {
            message: message.into(),
        },
    );
}

async fn save_snapshot_and_log(state: &ServerState) {
    if let Err(error) = persistence::save_snapshot(state).await {
        tracing::error!("failed to write crash-recovery snapshot: {error}");
    }
}

async fn handle_attach(
    state: &ServerState,
    session: &str,
    direct_tx: &mpsc::UnboundedSender<ServerEvent>,
) {
    if session != state.session_name {
        send_direct_error(
            direct_tx,
            format!(
                "this server serves session {:?}, not {session:?}",
                state.session_name
            ),
        );
        return;
    }
    let tree = state.tree.read().await;
    let snapshot = tree.clone();
    drop(tree);
    send_direct(direct_tx, ServerEvent::TreeSnapshot(snapshot));
}

/// Shared plumbing for the two tree-only mutations (`MoveNode`,
/// `RenameNode`): apply `mutate` under the tree write lock, and on success
/// broadcast the resulting snapshot to every client and persist a
/// crash-recovery snapshot; on failure, reply only to the requester with
/// the `TreeError`.
async fn handle_tree_mutation(
    state: &Arc<ServerState>,
    direct_tx: &mpsc::UnboundedSender<ServerEvent>,
    mutate: impl FnOnce(&mut Tree) -> Result<(), TreeError>,
) {
    let mut tree = state.tree.write().await;
    let result = mutate(&mut tree);
    match result {
        Ok(()) => {
            let snapshot = tree.clone();
            drop(tree);
            state.broadcast(ServerEvent::TreeSnapshot(snapshot));
            save_snapshot_and_log(state).await;
        }
        Err(error) => {
            drop(tree);
            send_direct_error(direct_tx, format!("tree operation failed: {error}"));
        }
    }
}

/// Resolves where a `NewPane` request should actually land: `requested`
/// itself, unless it's the session root, in which case panes fall back to
/// the tree's default top-level group (creating one if none exists yet).
/// `illium_ipc::ClientRequest` has no "create group" request yet, so a
/// client with no group to target passes `illium_core::ROOT_ID` as
/// `parent_group` and relies on this fallback -- matching
/// `Tree::ensure_default_group`'s own documented purpose ("a UI fallback
/// with no more specific target") rather than rejecting the request with
/// `TreeError::PanesRequireGroup`.
fn resolve_parent_group(tree: &mut Tree, requested: NodeId) -> NodeId {
    if requested == illium_core::ROOT_ID {
        tree.ensure_default_group("default")
    } else {
        requested
    }
}

fn editor_pane_name(path: &std::path::Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "untitled".to_string())
}

/// The cwd new terminal panes are spawned in. `illium_ipc::ClientRequest::NewPane`
/// carries no per-pane cwd (only `Editor`'s file path implies a location),
/// so this crate uses the server process's own working directory -- the
/// directory the `illium` CLI wrapper launched `illium-server` from,
/// matching how the pre-refactor bin crate's panes always shared one
/// session-wide cwd. Extending the protocol with a per-`NewPane` cwd is a
/// reasonable future addition but not one this stage's scope covers.
fn default_pane_cwd() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"))
}

async fn handle_new_pane(
    state: &Arc<ServerState>,
    parent_group: NodeId,
    kind: NewPaneKind,
    direct_tx: &mpsc::UnboundedSender<ServerEvent>,
) {
    match kind {
        NewPaneKind::Editor(path) => {
            let mut tree = state.tree.write().await;
            let parent_group = resolve_parent_group(&mut tree, parent_group);
            let pane_id = match tree.add_pane(
                parent_group,
                editor_pane_name(&path),
                PaneContentKind::Editor,
            ) {
                Ok(id) => id,
                Err(error) => {
                    drop(tree);
                    send_direct_error(direct_tx, format!("failed to create editor pane: {error}"));
                    return;
                }
            };
            let snapshot = tree.clone();
            drop(tree);

            let mut panes = state.panes.write().await;
            panes.insert(pane_id, PaneResource::Editor { path: Some(path) });
            drop(panes);

            state.broadcast(ServerEvent::TreeSnapshot(snapshot));
            save_snapshot_and_log(state).await;
        }
        NewPaneKind::PlainShell | NewPaneKind::Command(_) => {
            let origin = match kind {
                NewPaneKind::PlainShell => TerminalOrigin::PlainShell,
                NewPaneKind::Command(command_line) => TerminalOrigin::Command(command_line),
                NewPaneKind::Editor(_) => unreachable!("Editor handled in the outer match arm"),
            };

            let mut session = match pane::spawn_terminal_session(&origin, &default_pane_cwd()) {
                Ok(session) => session,
                Err(error) => {
                    send_direct_error(direct_tx, format!("failed to spawn pane: {error}"));
                    return;
                }
            };

            let mut tree = state.tree.write().await;
            let parent_group = resolve_parent_group(&mut tree, parent_group);
            let pane_id = match tree.add_pane(
                parent_group,
                origin.default_pane_name(),
                PaneContentKind::Terminal,
            ) {
                Ok(id) => id,
                Err(error) => {
                    drop(tree);
                    // The tree rejected the spot to put this pane (e.g. a
                    // bad/stale parent_group); the pty was already spawned,
                    // so it must be killed rather than left running
                    // detached from any tree node.
                    let _ = session.kill();
                    send_direct_error(direct_tx, format!("failed to create pane: {error}"));
                    return;
                }
            };
            let snapshot = tree.clone();
            drop(tree);

            let forward_task = tokio::spawn(forward_output_bytes(
                Arc::clone(state),
                pane_id,
                session.subscribe_output_bytes(),
            ));
            let runtime = crate::pane::TerminalPaneRuntime::new(
                session,
                origin,
                state.detection_config.idle_poll_interval,
                forward_task,
            );

            let mut panes = state.panes.write().await;
            panes.insert(pane_id, PaneResource::Terminal(runtime));
            drop(panes);

            state.broadcast(ServerEvent::TreeSnapshot(snapshot));
            save_snapshot_and_log(state).await;
        }
    }
}

/// Forwards one pane's raw pty output bytes to every attached client as
/// `ServerEvent::ScreenUpdate` frames, until the pane's pty reader thread
/// exits (child process gone) or this task is aborted (pane closed --
/// see `TerminalPaneRuntime::abort_background_tasks`).
async fn forward_output_bytes(
    state: Arc<ServerState>,
    pane_id: NodeId,
    mut receiver: tokio::sync::broadcast::Receiver<Vec<u8>>,
) {
    loop {
        match receiver.recv().await {
            Ok(bytes) => state.broadcast(ServerEvent::ScreenUpdate { pane_id, bytes }),
            Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                tracing::warn!(
                    "pane {pane_id:?} output forwarder lagged, skipped {skipped} chunk(s)"
                );
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
        }
    }
}

/// All pane ids in the subtree rooted at `id` (inclusive) -- `id` itself
/// if it's a pane, or every pane transitively nested under it if it's a
/// group. Used by `handle_close_pane` and `handle_kill_session` to know
/// exactly which pane-registry entries a tree removal must tear down;
/// `Tree::remove_node` removes a group's whole subtree from the tree in
/// one call but has no reason to know about `illium-server`'s pane
/// registry, so this crate computes the affected set itself before
/// calling it.
fn collect_pane_descendants(tree: &Tree, id: NodeId) -> Vec<NodeId> {
    let mut result = Vec::new();
    let mut frontier = vec![id];
    while let Some(current) = frontier.pop() {
        let Some(node) = tree.get(current) else {
            continue;
        };
        if node.is_pane() {
            result.push(current);
        } else if let Ok(children) = tree.children_of(current) {
            frontier.extend(children.iter().copied());
        }
    }
    result
}

fn teardown_pane_resource(pane_id: NodeId, mut resource: PaneResource) {
    resource.abort_background_tasks();
    if let PaneResource::Terminal(runtime) = &mut resource {
        if let Err(error) = runtime.session.kill() {
            tracing::warn!(
                "pane {pane_id:?} kill failed (process may have already exited): {error}"
            );
        }
    }
}

async fn handle_close_pane(
    state: &Arc<ServerState>,
    pane_id: NodeId,
    direct_tx: &mpsc::UnboundedSender<ServerEvent>,
) {
    let mut tree = state.tree.write().await;
    if tree.get(pane_id).is_none() {
        drop(tree);
        send_direct_error(direct_tx, format!("no such node {pane_id:?}"));
        return;
    }
    let descendant_pane_ids = collect_pane_descendants(&tree, pane_id);
    if let Err(error) = tree.remove_node(pane_id) {
        drop(tree);
        send_direct_error(direct_tx, format!("failed to close pane: {error}"));
        return;
    }
    let snapshot = tree.clone();
    drop(tree);

    let mut panes = state.panes.write().await;
    for id in descendant_pane_ids {
        if let Some(resource) = panes.remove(&id) {
            teardown_pane_resource(id, resource);
        }
    }
    drop(panes);

    state.broadcast(ServerEvent::TreeSnapshot(snapshot));
    save_snapshot_and_log(state).await;
}

async fn handle_resize_pane(
    state: &Arc<ServerState>,
    pane_id: NodeId,
    rows: u16,
    cols: u16,
    direct_tx: &mpsc::UnboundedSender<ServerEvent>,
) {
    let panes = state.panes.read().await;
    match panes.get(&pane_id) {
        Some(PaneResource::Terminal(runtime)) => {
            if let Err(error) = runtime.session.resize(rows, cols) {
                send_direct_error(
                    direct_tx,
                    format!("failed to resize pane {pane_id:?}: {error}"),
                );
            }
        }
        Some(PaneResource::Editor { .. }) => {
            send_direct_error(
                direct_tx,
                format!("pane {pane_id:?} is an editor, not a terminal"),
            );
        }
        None => send_direct_error(direct_tx, format!("no pane found for node {pane_id:?}")),
    }
}

async fn handle_key_input(
    state: &Arc<ServerState>,
    pane_id: NodeId,
    bytes: &[u8],
    direct_tx: &mpsc::UnboundedSender<ServerEvent>,
) {
    let panes = state.panes.read().await;
    match panes.get(&pane_id) {
        Some(PaneResource::Terminal(runtime)) => {
            if let Err(error) = runtime.session.write(bytes) {
                send_direct_error(
                    direct_tx,
                    format!("failed to write to pane {pane_id:?}: {error}"),
                );
            }
        }
        Some(PaneResource::Editor { .. }) => {
            send_direct_error(
                direct_tx,
                format!("pane {pane_id:?} is an editor, not a terminal"),
            );
        }
        None => send_direct_error(direct_tx, format!("no pane found for node {pane_id:?}")),
    }
}

#[allow(clippy::too_many_arguments)]
async fn handle_mouse_input(
    state: &Arc<ServerState>,
    pane_id: NodeId,
    kind: illium_ipc::MouseEventKind,
    column: u16,
    row: u16,
    modifiers: illium_ipc::MouseModifiers,
    direct_tx: &mpsc::UnboundedSender<ServerEvent>,
) {
    let panes = state.panes.read().await;
    match panes.get(&pane_id) {
        Some(PaneResource::Terminal(runtime)) => {
            let event = to_crossterm_event(kind, column, row, modifiers);
            if let Err(error) = runtime.session.write_mouse_input(event, column, row) {
                send_direct_error(
                    direct_tx,
                    format!("failed to forward mouse input to pane {pane_id:?}: {error}"),
                );
            }
        }
        Some(PaneResource::Editor { .. }) => {
            send_direct_error(
                direct_tx,
                format!("pane {pane_id:?} is an editor, not a terminal"),
            );
        }
        None => send_direct_error(direct_tx, format!("no pane found for node {pane_id:?}")),
    }
}

async fn handle_kill_session(state: &Arc<ServerState>) {
    let mut tree = state.tree.write().await;
    *tree = Tree::new();
    let snapshot = tree.clone();
    // Lock ordering: `tree` before `panes` (see `ServerState` docs) --
    // held together here even though the two teardown steps are logically
    // independent, so this handler never has to be re-checked if that
    // ordering rule changes elsewhere.
    let mut panes = state.panes.write().await;
    for (pane_id, resource) in panes.drain() {
        teardown_pane_resource(pane_id, resource);
    }
    drop(panes);
    drop(tree);

    state.broadcast(ServerEvent::TreeSnapshot(snapshot));
    // A cleanly-killed session has nothing left worth recovering.
    match tokio::fs::remove_file(&state.snapshot_path).await {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => tracing::warn!("failed to remove snapshot file on session kill: {error}"),
    }
    // Deliberately does not abort other connections' tasks from here --
    // this handler runs *inside* one of those very connection tasks, and
    // aborting a `JoinHandle` cancels at the task's next `.await`, which
    // could cut this connection's own writer off before the
    // `TreeSnapshot` broadcast just sent above is actually flushed to any
    // attached client (including this one). `crate::server::run`'s
    // shutdown path (triggered by the `notify_waiters` call below) is
    // where connection tasks get aborted, after a short grace period --
    // see its comments.
    state.shutdown.notify_waiters();
}

//! Translates each `ilium_ipc::ClientRequest` variant into a mutation on
//! `ServerState`'s tree/pane registry, broadcasting the resulting
//! `ServerEvent` to every attached client for structural changes (tree
//! shape, pane status) or replying only to the requesting connection for
//! everything else (the initial `Attach` snapshot, request-specific
//! errors). See `crate::ipc::connection` for how the two reply channels
//! (`ServerState::events` broadcast vs. this connection's own `direct_tx`)
//! are wired together on the write side.

use std::sync::Arc;

use ilium_core::{
    AgentActivity, NodeId, NodeKind, PaneContentKind, PaneStatus, PaneTitleSource, RestructurePlan,
    ScheduledPaneInput, Tree, TreeError,
};
use ilium_ipc::{ClientRequest, NewPaneKind, NewPaneWorkingDirectory, ServerEvent};
use ilium_pty::PtyError;
use tokio::sync::mpsc;

use crate::mouse::to_crossterm_event;
use crate::pane;
use crate::pane::{PaneResource, PaneSnapshotKind, TerminalOrigin};
use crate::state::ServerState;

/// Handles one request from an attached client. Returns `true` when the
/// connection this request arrived on should close afterward (`Detach`,
/// `KillSession`) -- the caller (`crate::ipc::connection`) is what
/// actually stops reading further frames.
pub async fn handle_request(
    state: &Arc<ServerState>,
    request: ClientRequest,
    direct_tx: &mpsc::Sender<ServerEvent>,
) -> bool {
    match request {
        ClientRequest::Attach { session } => {
            handle_attach(state, &session, direct_tx).await;
            false
        }
        ClientRequest::ResolveSessionRecovery { restore } => {
            handle_session_recovery_resolution(state, restore, direct_tx).await;
            false
        }
        ClientRequest::NewPane {
            parent_group,
            kind,
            working_directory,
        } => {
            handle_new_pane(state, parent_group, kind, working_directory, direct_tx).await;
            false
        }
        ClientRequest::NewGroup { parent_group, name } => {
            handle_tree_mutation(state, direct_tx, |tree| {
                tree.add_group(parent_group, name).map(|_id| ())
            })
            .await;
            false
        }
        ClientRequest::NewFolder { parent_group, path } => {
            handle_tree_mutation(state, direct_tx, |tree| {
                tree.add_folder(parent_group, path).map(|_id| ())
            })
            .await;
            false
        }
        ClientRequest::NewBoard {
            parent_group,
            name,
            storage,
        } => {
            handle_tree_mutation(state, direct_tx, |tree| {
                let parent_group = resolve_parent_group(tree, parent_group);
                tree.add_board(parent_group, name, storage).map(|_id| ())
            })
            .await;
            false
        }
        ClientRequest::CreateSplitView {
            parent_group,
            name,
            orientation,
            pane_ids,
        } => {
            handle_tree_mutation(state, direct_tx, |tree| {
                let parent_group = resolve_parent_group(tree, parent_group);
                tree.create_split_view(parent_group, name, orientation, &pane_ids)
                    .map(|_id| ())
            })
            .await;
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
        ClientRequest::RenameNode {
            node_id,
            title,
            short_title,
        } => {
            handle_tree_mutation(state, direct_tx, |tree| {
                tree.rename_node(node_id, title, short_title)
            })
            .await;
            false
        }
        ClientRequest::SetAutomaticPaneTitle {
            pane_id,
            title,
            short_title,
        } => {
            handle_automatic_pane_title(state, pane_id, title, short_title).await;
            false
        }
        ClientRequest::SetSessionPaneTitle {
            pane_id,
            expected_session_id,
            expected_title_generation,
            title,
            short_title,
            title_source,
        } => {
            handle_session_pane_title(
                state,
                pane_id,
                &expected_session_id,
                expected_title_generation,
                title,
                short_title,
                title_source,
            )
            .await;
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
        ClientRequest::ReparentNode {
            node_id,
            new_parent,
            index,
        } => {
            handle_tree_mutation(state, direct_tx, |tree| {
                tree.move_node(node_id, new_parent, index)
            })
            .await;
            false
        }
        ClientRequest::Detach => true,
        ClientRequest::KillSession => {
            handle_kill_session(state).await;
            true
        }
        ClientRequest::SetPaneFocus { pane_id, focused } => {
            handle_set_pane_focus(state, pane_id, focused).await;
            false
        }
        ClientRequest::RestartServer => {
            // A development refresh intentionally retains the tree and pane
            // snapshot. `ilium_server::run` flushes this dirty snapshot
            // before exiting, then the replacement process restores it.
            state.request_snapshot_save();
            state.shutdown.notify_waiters();
            true
        }
        ClientRequest::UpdateSoundSettings { settings } => {
            *state.sound_settings.write().await = settings;
            false
        }
        ClientRequest::PreviewSound { source, file } => {
            let settings = ilium_sound::SoundSettings {
                source,
                file,
                ..state.sound_settings.read().await.clone()
            };
            crate::sounds::enqueue(
                state,
                crate::sounds::PlaybackRequest {
                    settings,
                    event: None,
                    pane_name: None,
                },
            );
            false
        }
        ClientRequest::SchedulePaneInput {
            pane_id,
            delay_seconds,
            text,
            send_enter,
        } => {
            handle_schedule_pane_input(state, pane_id, delay_seconds, text, send_enter, direct_tx)
                .await;
            false
        }
        ClientRequest::ApplyRestructurePlan(plan) => {
            handle_apply_restructure_plan(state, plan, direct_tx).await;
            false
        }
        ClientRequest::RevertLastRestructure => {
            handle_revert_last_restructure(state, direct_tx).await;
            false
        }
    }
}

/// Awaits capacity on this connection's bounded direct-reply queue before
/// enqueuing. A stalled client backpressures the connection's own request
/// handling instead of letting replies pile up in memory without bound.
async fn send_direct(direct_tx: &mpsc::Sender<ServerEvent>, event: ServerEvent) {
    // An error here only means this connection's writer task has already
    // ended (client disconnected mid-request); nothing left to do with the
    // reply.
    let _ = direct_tx.send(event).await;
}

async fn send_direct_error(direct_tx: &mpsc::Sender<ServerEvent>, message: impl Into<String>) {
    send_direct(
        direct_tx,
        ServerEvent::Error {
            message: message.into(),
        },
    )
    .await;
}

/// Clones a fresh tree snapshot under a brief **read** lock. Callers that
/// just mutated the tree must have already dropped their write-lock guard
/// before calling this -- the whole point is that this crate's one
/// genuinely O(n) tree operation (cloning the whole thing for a broadcast
/// payload) never runs while pinning the write lock every keystroke in
/// `handle_key_input` also needs. See `broadcast_and_persist` below, this
/// module's only caller.
async fn tree_snapshot(state: &ServerState) -> Tree {
    let tree = state.tree.read().await;
    tree.clone()
}

/// Shared tail end of every structural-mutation handler: broadcast a
/// fresh `TreeSnapshot` to every attached client and mark the
/// crash-recovery snapshot dirty for the background debounced writer to
/// pick up (`crate::persistence::spawn_snapshot_writer`) -- never awaits a
/// disk write inline on the request path. Callers must call this only
/// after dropping any tree/pane write-lock guard their own mutation held,
/// so this function's read-locked clone can never contend with a pending
/// writer (see `ServerState`'s lock-ordering docs).
pub(crate) async fn broadcast_and_persist(state: &Arc<ServerState>) {
    let snapshot = tree_snapshot(state).await;
    state.broadcast(ServerEvent::TreeSnapshot(snapshot));
    state.request_snapshot_save();
}

/// Validates and persists one timer before waking the detached executor. The
/// absolute deadline is server-derived so all clients and crash recovery share
/// one clock instead of trusting whichever UI happened to create the action.
async fn handle_schedule_pane_input(
    state: &Arc<ServerState>,
    pane_id: NodeId,
    delay_seconds: u64,
    text: String,
    send_enter: bool,
    direct_tx: &mpsc::Sender<ServerEvent>,
) {
    let execute_at_unix_millis = match crate::scheduled_input::deadline_from_delay(delay_seconds) {
        Ok(deadline) => deadline,
        Err(message) => {
            send_direct_error(direct_tx, message).await;
            return;
        }
    };
    // Replacement and the executor's final check/write are one transaction,
    // so accepting this schedule guarantees an older action cannot fire later.
    let transaction = state.scheduled_input_transaction.lock().await;
    let result = {
        let mut tree = state.tree.write().await;
        tree.schedule_pane_input(
            pane_id,
            ScheduledPaneInput {
                execute_at_unix_millis,
                text,
                send_enter,
            },
        )
    };
    // The authoritative replacement is now visible; snapshots and client
    // broadcasts do not need to delay the executor's next freshness check.
    drop(transaction);
    if let Err(error) = result {
        send_direct_error(direct_tx, format!("failed to schedule pane input: {error}")).await;
        return;
    }
    broadcast_and_persist(state).await;
    state.scheduled_input_changed.notify_one();
}

async fn handle_attach(state: &ServerState, session: &str, direct_tx: &mpsc::Sender<ServerEvent>) {
    if session != state.session_name {
        send_direct_error(
            direct_tx,
            format!(
                "this server serves session {:?}, not {session:?}",
                state.session_name
            ),
        )
        .await;
        return;
    }
    let tree = state.tree.read().await;
    let snapshot = tree.clone();
    drop(tree);
    send_direct(direct_tx, ServerEvent::TreeSnapshot(snapshot)).await;
    if let Some(snapshot) = state.pending_session_recovery.lock().await.as_ref() {
        send_direct(
            direct_tx,
            ServerEvent::SessionRecoveryAvailable {
                pane_count: snapshot.panes.len(),
            },
        )
        .await;
    }

    // Terminal scrollback, session IDs, and editor paths belong to live pane
    // resources rather than the persisted tree wire shape, so replay them
    // explicitly after the attachment snapshot has established matching node
    // ids. A replay is captured atomically with its output sequence by
    // `PtySession`; the client uses that sequence to drop any duplicate live
    // update that was queued while this attach was in flight.
    //
    // Collect the replay events under the read lock, then drop the lock
    // before sending any of them -- same rationale as every other handler
    // in this module (`handle_key_input`, `handle_resize_pane`, ...):
    // `send_direct` awaits capacity on this connection's bounded
    // direct-reply queue, and `state.panes` is a write-preferring lock, so
    // a slow-draining attaching client awaited while still holding this
    // read lock would stall every other connection's pending
    // `state.panes.write()` (every key/mouse/resize/new-pane/close-pane
    // request) behind it.
    let replay_events: Vec<ServerEvent> = {
        let panes = state.panes.read().await;
        panes
            .iter()
            .flat_map(|(pane_id, resource)| match resource {
                PaneResource::Terminal(runtime) => {
                    let replay = runtime.session.output_replay();
                    let mut events = vec![ServerEvent::TerminalReplay {
                        pane_id: *pane_id,
                        through_sequence: replay.through_sequence,
                        bytes: replay.bytes,
                        is_complete: replay.is_complete,
                    }];
                    if let Some(session_id) = runtime.session_id.clone() {
                        events.push(ServerEvent::PaneSessionIdResolved {
                            pane_id: *pane_id,
                            session_id,
                            title_generation: runtime.title_generation,
                        });
                    }
                    events
                }
                PaneResource::Editor { path } => vec![ServerEvent::PaneEditorPathResolved {
                    pane_id: *pane_id,
                    path: path.clone(),
                }],
            })
            .collect()
    };

    for event in replay_events {
        send_direct(direct_tx, event).await;
    }
}

async fn handle_session_recovery_resolution(
    state: &Arc<ServerState>,
    restore: bool,
    direct_tx: &mpsc::Sender<ServerEvent>,
) {
    let Some(snapshot) = state.pending_session_recovery.lock().await.take() else {
        send_direct_error(
            direct_tx,
            "No session recovery decision is pending".to_string(),
        )
        .await;
        return;
    };
    if restore {
        crate::restore_snapshot(state, snapshot).await;
        broadcast_and_persist(state).await;
    } else if let Err(error) = tokio::fs::remove_file(&state.snapshot_path).await {
        if error.kind() != std::io::ErrorKind::NotFound {
            send_direct_error(
                direct_tx,
                format!("Could not discard stored session snapshot: {error}"),
            )
            .await;
        }
    }
}

/// Shared plumbing for the two tree-only mutations (`MoveNode`,
/// `RenameNode`): apply `mutate` under the tree write lock, and on success
/// broadcast the resulting snapshot to every client and persist a
/// crash-recovery snapshot; on failure, reply only to the requester with
/// the `TreeError`.
async fn handle_tree_mutation(
    state: &Arc<ServerState>,
    direct_tx: &mpsc::Sender<ServerEvent>,
    mutate: impl FnOnce(&mut Tree) -> Result<(), TreeError>,
) {
    let mut tree = state.tree.write().await;
    let result = mutate(&mut tree);
    // Drop the write guard before doing anything else -- in particular,
    // before the broadcast snapshot's own O(n) clone (see
    // `broadcast_and_persist`), so this write lock is only ever held for
    // the mutation itself.
    drop(tree);
    match result {
        Ok(()) => broadcast_and_persist(state).await,
        Err(error) => send_direct_error(direct_tx, format!("tree operation failed: {error}")).await,
    }
}

/// Applies a full-tree restructure plan (see `ilium_core::Tree::apply_restructure`).
/// The tree exactly as it was before this mutation is kept in
/// `state.restructure_undo`'s one slot only when the plan actually applies
/// cleanly -- a rejected plan leaves both the tree and any earlier undo
/// buffer untouched.
async fn handle_apply_restructure_plan(
    state: &Arc<ServerState>,
    plan: RestructurePlan,
    direct_tx: &mpsc::Sender<ServerEvent>,
) {
    let mut tree = state.tree.write().await;
    let before = tree.clone();
    let result = tree.apply_restructure(plan);
    drop(tree);
    match result {
        Ok(()) => {
            *state.restructure_undo.lock().await = Some(before);
            broadcast_and_persist(state).await;
        }
        Err(error) => send_direct_error(direct_tx, format!("restructure failed: {error}")).await,
    }
}

/// Restores the tree from the one-slot undo buffer left by the most recent
/// successful `ApplyRestructurePlan`, if any. Consumes the slot: reverting
/// twice in a row without a new restructure in between is a no-op reported
/// as an error, not a toggle back and forth between two states.
///
/// The undo buffer is not time-boxed on the client -- arbitrary structural
/// work (most notably `NewPane`) can happen between the restructure and this
/// revert. `Tree::apply_restructure` itself guarantees the pane/folder leaf
/// set never changes, so any pane present in the tree being discarded here
/// but absent from the restored tree must have been created *after* that
/// restructure. Once `*tree` below is overwritten, such a pane's tree node
/// is gone, but its `PaneResource` (PTY session, output-forwarder task) is
/// still sitting in `state.panes` with nothing left to ever remove it --
/// exactly like the descendant teardown `handle_close_pane` does, this
/// tears those orphaned resources down before returning.
async fn handle_revert_last_restructure(
    state: &Arc<ServerState>,
    direct_tx: &mpsc::Sender<ServerEvent>,
) {
    let previous = state.restructure_undo.lock().await.take();
    match previous {
        Some(previous_tree) => {
            let mut tree = state.tree.write().await;
            let orphaned_pane_ids: Vec<NodeId> =
                collect_pane_descendants(&tree, ilium_core::ROOT_ID)
                    .into_iter()
                    .filter(|pane_id| previous_tree.get(*pane_id).is_none())
                    .collect();
            *tree = previous_tree;
            // Drop the write guard before the pane-registry teardown below
            // (which needs no tree access) and before `broadcast_and_persist`'s
            // own read-locked clone -- see that function's docs.
            drop(tree);

            if !orphaned_pane_ids.is_empty() {
                let mut panes = state.panes.write().await;
                for pane_id in orphaned_pane_ids {
                    if let Some(resource) = panes.remove(&pane_id) {
                        teardown_pane_resource(pane_id, resource);
                    }
                }
                drop(panes);
            }

            broadcast_and_persist(state).await;
        }
        None => send_direct_error(direct_tx, "no restructure to revert").await,
    }
}

/// Applies an automatic title only while the user has not explicitly named
/// the pane, then publishes the changed tree just like any other title edit.
async fn handle_automatic_pane_title(
    state: &Arc<ServerState>,
    pane_id: NodeId,
    title: String,
    short_title: Option<String>,
) {
    let title_changed = {
        let mut tree = state.tree.write().await;
        match tree.set_automatic_pane_title(pane_id, title, short_title) {
            Ok(changed) => changed,
            Err(error) => {
                tracing::warn!("automatic title update rejected for pane {pane_id:?}: {error}");
                false
            }
        }
    };
    if title_changed {
        broadcast_and_persist(state).await;
    }
}

/// Applies an LLM title as a compare-and-set against the server's live
/// session identity. A stale client or in-flight worker can never title the
/// replacement session, regardless of IPC event/request ordering.
async fn handle_session_pane_title(
    state: &Arc<ServerState>,
    pane_id: NodeId,
    expected_session_id: &str,
    expected_title_generation: u64,
    title: String,
    short_title: Option<String>,
    title_source: PaneTitleSource,
) {
    // Lock ordering is tree before panes throughout the server. Both remain
    // held through the identity check and title write so `/resume` cannot
    // invalidate the session between those two operations.
    let mut tree = state.tree.write().await;
    let panes = state.panes.read().await;
    let Some(PaneResource::Terminal(runtime)) = panes.get(&pane_id) else {
        return;
    };
    if runtime.is_session_identity_invalidated
        || runtime.session_id.as_deref() != Some(expected_session_id)
        || runtime.title_generation != expected_title_generation
    {
        return;
    }
    let changed = match title_source {
        PaneTitleSource::Automatic => tree
            .set_automatic_pane_title(pane_id, title, short_title)
            .unwrap_or_else(|error| {
                tracing::warn!("session title update rejected for pane {pane_id:?}: {error}");
                false
            }),
        PaneTitleSource::UserSpecified => match tree.rename_node(pane_id, title, short_title) {
            Ok(()) => true,
            Err(error) => {
                tracing::warn!("session retitle rejected for pane {pane_id:?}: {error}");
                false
            }
        },
    };
    drop(panes);
    drop(tree);
    if changed {
        broadcast_and_persist(state).await;
    }
}

/// Records whether the attached client currently has `pane_id` as its
/// active view, and forces an immediate (debounced) recheck on every
/// focus transition -- see `crate::detection::interval_for` (the
/// client-focused fast tier) and `crate::detection::force_check`. No
/// `direct_tx`/error surfaced on a missing pane: a focus message racing a
/// pane's closure is an entirely ordinary, harmless timing window (the
/// client can't always know a `ClosePane` beat its `SetPaneFocus` to the
/// server), not something the user needs to see.
///
/// A focus-gain that finds the pane `Done` also clears it to `Idle`
/// synchronously, right here, instead of leaving that solely to the
/// detection loop's forced recheck. The detection loop only wakes once per
/// `BASE_TICK_INTERVAL`; a focus-then-unfocus faster than that window
/// (an entirely ordinary quick glance) can flip `client_focused` back to
/// `false` before any tick ever observes it `true`, so the tree's
/// authoritative status never actually leaves `Done` -- the next tick then
/// sees `raw_activity == Idle`, `client_focused == false`, `previous ==
/// Done` and `promote_to_done`'s stickiness re-stamps `Done`, silently
/// overwriting the client's own local `Done -> Idle` clear (`app.rs`'s
/// `mark_seen`) and resurrecting the "look at me" badge on a pane the user
/// already looked at.
async fn handle_set_pane_focus(state: &Arc<ServerState>, pane_id: NodeId, focused: bool) {
    // Lock ordering: `tree` before `panes` (see `ServerState` docs) --
    // needed together here since a focus-gain may also clear tree status.
    let mut tree = state.tree.write().await;
    let mut panes = state.panes.write().await;
    let Some(PaneResource::Terminal(runtime)) = panes.get_mut(&pane_id) else {
        return;
    };
    runtime.detection_schedule.client_focused = focused;
    crate::detection::force_check(&mut runtime.detection_schedule, std::time::Instant::now());

    let cleared_status = focused
        .then(|| match tree.get(pane_id).map(|node| &node.kind) {
            Some(NodeKind::Pane { status, .. }) => {
                let new_status = match status {
                    PaneStatus::Agent(class, AgentActivity::Done) => {
                        Some(PaneStatus::Agent(class.clone(), AgentActivity::Idle))
                    }
                    PaneStatus::AgentWithGoal(class, AgentActivity::Done) => Some(
                        PaneStatus::AgentWithGoal(class.clone(), AgentActivity::Idle),
                    ),
                    _ => None,
                };
                new_status.and_then(|new_status| {
                    tree.set_pane_status(pane_id, new_status.clone())
                        .ok()
                        .map(|()| new_status)
                })
            }
            _ => None,
        })
        .flatten();

    drop(panes);
    drop(tree);

    if let Some(status) = cleared_status {
        state.broadcast(ServerEvent::PaneStatusChanged { pane_id, status });
    }
}

/// Resolves where a `NewPane` request should actually land: `requested`
/// itself, unless it's the session root, in which case panes fall back to
/// the tree's default top-level group (creating one if none exists yet).
/// Panes are never direct children of the root (`Tree::add_pane`'s own
/// invariant), so a client with no group to target passes
/// `ilium_core::ROOT_ID` as `parent_group` and relies on this fallback --
/// matching `Tree::ensure_default_group`'s own documented purpose ("a UI
/// fallback with no more specific target") rather than rejecting the
/// request with `TreeError::PanesRequireGroup`. `NewGroup` needs no
/// equivalent fallback: the domain tree allows a group directly under the
/// root, so its `parent_group` is used as-is.
fn resolve_parent_group(tree: &mut Tree, requested: NodeId) -> NodeId {
    if requested == ilium_core::ROOT_ID {
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

/// Fully-resolved server work for one pane-creation request. Initial input is
/// deliberately transient: crash recovery should relaunch the agent session,
/// never re-submit the original task a second time.
struct NewPanePlan {
    spawn_kind: PaneSnapshotKind,
    name: String,
    content_kind: PaneContentKind,
    initial_input: Option<String>,
}

/// Turns a client's `NewPaneKind` into the one server-owned creation plan used
/// for spawning, persistence identity, tree presentation, and optional first
/// submission.
fn new_pane_plan(kind: NewPaneKind) -> NewPanePlan {
    match kind {
        NewPaneKind::Editor(path) => {
            let name = editor_pane_name(&path);
            NewPanePlan {
                spawn_kind: PaneSnapshotKind::Editor { path: Some(path) },
                name,
                content_kind: PaneContentKind::Editor,
                initial_input: None,
            }
        }
        NewPaneKind::PlainShell => {
            let origin = TerminalOrigin::PlainShell;
            let name = origin.default_pane_name().to_string();
            NewPanePlan {
                spawn_kind: PaneSnapshotKind::Terminal(origin),
                name,
                content_kind: PaneContentKind::Terminal,
                initial_input: None,
            }
        }
        NewPaneKind::Command(command_line) => {
            let origin = TerminalOrigin::Command(command_line);
            let name = origin.default_pane_name().to_string();
            NewPanePlan {
                spawn_kind: PaneSnapshotKind::Terminal(origin),
                name,
                content_kind: PaneContentKind::Terminal,
                initial_input: None,
            }
        }
        NewPaneKind::CommandWithInitialInput {
            command_line,
            initial_input,
        } => {
            let origin = TerminalOrigin::Command(command_line);
            let name = origin.default_pane_name().to_string();
            NewPanePlan {
                spawn_kind: PaneSnapshotKind::Terminal(origin),
                name,
                content_kind: PaneContentKind::Terminal,
                initial_input: Some(initial_input),
            }
        }
    }
}

/// Encodes multi-line textarea content as one bracketed paste so embedded
/// newlines remain part of the agent prompt; the final Enter is sent
/// separately by `handle_new_pane` and remains the only submission keystroke.
fn initial_input_bytes(initial_input: &str) -> Vec<u8> {
    if !initial_input.contains('\n') {
        return initial_input.as_bytes().to_vec();
    }

    let mut bytes = Vec::with_capacity(initial_input.len() + 12);
    bytes.extend_from_slice(b"\x1b[200~");
    bytes.extend_from_slice(initial_input.as_bytes());
    bytes.extend_from_slice(b"\x1b[201~");
    bytes
}

async fn handle_new_pane(
    state: &Arc<ServerState>,
    parent_group: NodeId,
    kind: NewPaneKind,
    working_directory: NewPaneWorkingDirectory,
    direct_tx: &mpsc::Sender<ServerEvent>,
) {
    let plan = new_pane_plan(kind);

    let mut tree = state.tree.write().await;
    let parent_group = resolve_parent_group(&mut tree, parent_group);
    let pane_id = match tree.add_pane(parent_group, plan.name, plan.content_kind) {
        Ok(id) => id,
        Err(error) => {
            drop(tree);
            send_direct_error(direct_tx, format!("failed to create pane: {error}")).await;
            return;
        }
    };
    // Drop the write guard before spawning (a pty spawn + registering it
    // in `state.panes` needs no tree access at all) and before the
    // eventual broadcast snapshot's O(n) clone -- see `broadcast_and_persist`.
    drop(tree);

    let cwd = resolve_new_pane_working_directory(state, working_directory).await;
    match spawn_and_register_pane_in_directory(state, pane_id, plan.spawn_kind, &cwd).await {
        Ok(()) => {}
        Err(error) => {
            // The tree node exists (created just above) but has no resource
            // behind it -- nothing was broadcast yet, so no attached client has
            // seen it; remove it rather than leaving a phantom node no client
            // could ever interact with.
            let mut tree = state.tree.write().await;
            let _ = tree.remove_node(pane_id);
            drop(tree);
            send_direct_error(direct_tx, format!("failed to spawn pane: {error}")).await;
            return;
        }
    }

    if let Some(initial_input) = plan.initial_input {
        let bytes = initial_input_bytes(&initial_input);
        handle_key_input(state, pane_id, &bytes, direct_tx).await;
        handle_key_input(state, pane_id, b"\r", direct_tx).await;
    }

    broadcast_and_persist(state).await;
}

async fn resolve_new_pane_working_directory(
    state: &Arc<ServerState>,
    policy: NewPaneWorkingDirectory,
) -> std::path::PathBuf {
    match policy {
        NewPaneWorkingDirectory::ProjectRoot => state.session_cwd.clone(),
        NewPaneWorkingDirectory::LastUsed => state
            .last_terminal_working_directory
            .lock()
            .await
            .clone()
            .unwrap_or_else(|| state.session_cwd.clone()),
        NewPaneWorkingDirectory::FocusedTerminal => {
            let panes = state.panes.read().await;
            panes
                .values()
                .find_map(|resource| match resource {
                    PaneResource::Terminal(runtime)
                        if runtime.detection_schedule.client_focused =>
                    {
                        runtime.session.current_working_directory()
                    }
                    _ => None,
                })
                .unwrap_or_else(|| state.session_cwd.clone())
        }
    }
}

/// Spawns (for a `Terminal` origin) or registers (for an `Editor`) the
/// `PaneResource` for `pane_id` per `kind`, inserting it into
/// `state.panes`. `pane_id` must already exist in `state.tree` as a pane
/// node -- this function only ever touches the pane registry, never the
/// tree.
///
/// Shared by two callers that both need exactly this "given a tree node
/// id and what it should run, make it live" step: `handle_new_pane` above
/// (whose tree node was just created) and `crate::run`'s startup
/// crash-recovery restore path (whose tree nodes already exist as part of
/// a loaded snapshot). Keeping this in one place means a future change to
/// how a terminal's output-forwarder task is spawned, or how its detection
/// schedule is seeded, can never drift between the two call sites.
pub(crate) async fn spawn_and_register_pane(
    state: &Arc<ServerState>,
    pane_id: NodeId,
    kind: PaneSnapshotKind,
) -> Result<(), PtyError> {
    let cwd = state.session_cwd.clone();
    spawn_and_register_pane_in_directory(state, pane_id, kind, &cwd).await
}

pub(crate) async fn spawn_and_register_pane_in_directory(
    state: &Arc<ServerState>,
    pane_id: NodeId,
    kind: PaneSnapshotKind,
    cwd: &std::path::Path,
) -> Result<(), PtyError> {
    let is_terminal = matches!(kind, PaneSnapshotKind::Terminal(_));
    let resource = match kind {
        PaneSnapshotKind::Editor { path } => PaneResource::Editor { path },
        PaneSnapshotKind::Terminal(origin) => {
            let spawned = pane::spawn_terminal_session(&origin, cwd)?;
            let pending_generated_session_id = spawned.session_id;
            let session = spawned.session;
            let forward_task = tokio::spawn(forward_output_bytes(
                Arc::clone(state),
                pane_id,
                session.subscribe_output_bytes(),
            ));
            let runtime = crate::pane::TerminalPaneRuntime::new(
                session,
                origin,
                pending_generated_session_id,
                state.detection_config.idle_poll_interval,
                forward_task,
            );
            PaneResource::Terminal(Box::new(runtime))
        }
    };

    let mut panes = state.panes.write().await;
    panes.insert(pane_id, resource);
    if is_terminal {
        *state.last_terminal_working_directory.lock().await = Some(cwd.to_path_buf());
    }
    Ok(())
}

/// Forwards one pane's raw pty output bytes to every attached client as
/// `ServerEvent::ScreenUpdate` frames, until the pane's pty reader thread
/// exits (child process gone) or this task is aborted (pane closed --
/// see `TerminalPaneRuntime::abort_background_tasks`).
async fn forward_output_bytes(
    state: Arc<ServerState>,
    pane_id: NodeId,
    mut receiver: tokio::sync::broadcast::Receiver<ilium_pty::PtyOutputChunk>,
) {
    loop {
        match receiver.recv().await {
            Ok(chunk) => state.broadcast(ServerEvent::ScreenUpdate {
                pane_id,
                sequence: chunk.sequence,
                bytes: chunk.bytes,
            }),
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
/// one call but has no reason to know about `ilium-server`'s pane
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
    direct_tx: &mpsc::Sender<ServerEvent>,
) {
    let mut tree = state.tree.write().await;
    if tree.get(pane_id).is_none() {
        drop(tree);
        send_direct_error(direct_tx, format!("no such node {pane_id:?}")).await;
        return;
    }
    let descendant_pane_ids = collect_pane_descendants(&tree, pane_id);
    if let Err(error) = tree.remove_node(pane_id) {
        drop(tree);
        send_direct_error(direct_tx, format!("failed to close pane: {error}")).await;
        return;
    }
    // Drop the write guard before the pane-registry teardown below (which
    // needs no tree access) and before the eventual broadcast snapshot's
    // O(n) clone -- see `broadcast_and_persist`.
    drop(tree);

    let mut panes = state.panes.write().await;
    for id in descendant_pane_ids {
        if let Some(resource) = panes.remove(&id) {
            teardown_pane_resource(id, resource);
        }
    }
    drop(panes);

    broadcast_and_persist(state).await;
    state.scheduled_input_changed.notify_one();
}

async fn handle_resize_pane(
    state: &Arc<ServerState>,
    pane_id: NodeId,
    rows: u16,
    cols: u16,
    direct_tx: &mpsc::Sender<ServerEvent>,
) {
    // Compute the outcome under the read lock, then drop it before awaiting
    // `send_direct` below -- awaiting a possibly-full direct-reply channel
    // while still holding `state.panes` would stall every other pane's
    // resize/key/mouse handling on this one connection's slow client.
    let panes = state.panes.read().await;
    let error_message = match panes.get(&pane_id) {
        Some(PaneResource::Terminal(runtime)) => runtime
            .session
            .resize(rows, cols)
            .err()
            .map(|error| format!("failed to resize pane {pane_id:?}: {error}")),
        Some(PaneResource::Editor { .. }) => {
            Some(format!("pane {pane_id:?} is an editor, not a terminal"))
        }
        None => Some(format!("no pane found for node {pane_id:?}")),
    };
    drop(panes);

    if let Some(message) = error_message {
        send_direct_error(direct_tx, message).await;
    }
}

async fn handle_key_input(
    state: &Arc<ServerState>,
    pane_id: NodeId,
    bytes: &[u8],
    direct_tx: &mpsc::Sender<ServerEvent>,
) {
    if let Err(message) = write_key_input(state, pane_id, bytes).await {
        send_direct_error(direct_tx, message).await;
    }
}

/// Writes terminal bytes through the same title-tracking and detection path
/// for both live client keys and server-scheduled input. Keeping one input
/// boundary prevents delayed Enter from behaving differently from a key the
/// user pressed directly.
pub(crate) async fn write_key_input(
    state: &Arc<ServerState>,
    pane_id: NodeId,
    bytes: &[u8],
) -> Result<(), String> {
    // A cheap read-lock check up front: only an automatic-title
    // plain-shell pane can ever need this keystroke to touch the tree at
    // all. Escalating straight to `tree.write()` on every keystroke (as
    // this used to) held the same lock every structural mutation
    // (`handle_tree_mutation`, `handle_new_pane`, ...) needs, for far
    // longer than necessary -- see this crate's CLAUDE.md ISSUE notes.
    // The write lock only actually gets taken below, and only on the rare
    // keystroke that completes a shell command line worth naming the pane
    // after.
    let is_automatic_plain_shell = {
        let tree = state.tree.read().await;
        matches!(
            tree.get(pane_id).map(|node| &node.kind),
            Some(NodeKind::Pane {
                status: PaneStatus::PlainShell,
                title_source: PaneTitleSource::Automatic,
                ..
            })
        )
    };

    // Write lock (not read) on `panes`: a `KeyInput` always targets the
    // client's currently-focused pane (the client only ever forwards raw
    // keys for `self.focused_pane`), which `ClientRequest::SetPaneFocus`
    // already pins to `BASE_TICK_INTERVAL` regardless of status -- so most
    // keystrokes need no extra scheduling push here. Enter is the
    // exception: it's the clearest possible signal a command/prompt was
    // just submitted, so it still forces an immediate (debounced) recheck
    // below, rather than waiting up to one base tick.
    // As with the read lock above: compute the outcome (including any
    // error message) while holding the write lock, then drop it before
    // returning an error -- this is `state.panes`' write lock, held by every
    // pane's key/mouse/resize handling, so no caller may await unrelated work
    // while it remains held.
    let mut panes = state.panes.write().await;
    let mut observed_title = None;
    let mut cleared_session_origin_name = None;
    let mut cleared_session_title_generation = None;
    let mut cleared_conversation_origin_name = None;
    let mut cleared_conversation_title_generation = None;
    let error_message = match panes.get_mut(&pane_id) {
        Some(PaneResource::Terminal(runtime)) => {
            let is_shell_foreground = matches!(&runtime.origin, TerminalOrigin::PlainShell)
                && matches!(
                    (
                        runtime.session.foreground_process_group_id(),
                        runtime.session.process_id(),
                    ),
                    (Some(foreground_group_id), Some(shell_process_id))
                        if foreground_group_id == shell_process_id
                );
            let should_track_title = is_automatic_plain_shell && is_shell_foreground;
            if let Err(error) = runtime.session.write(bytes) {
                Some(format!("failed to write to pane {pane_id:?}: {error}"))
            } else {
                if let Some(tracker) = &mut runtime.shell_command_tracker {
                    if should_track_title {
                        observed_title = tracker.observe(bytes);
                    } else {
                        tracker.reset_pending_line();
                    }
                }
                let submitted_line = runtime.session_command_tracker.observe(bytes);
                let session_identity_invalidated = submitted_line
                    .as_deref()
                    .is_some_and(crate::pane::invalidates_agent_session_identity);
                if session_identity_invalidated {
                    runtime.is_session_identity_invalidated = true;
                    runtime.pending_generated_session_id = None;
                    runtime.title_generation = runtime.title_generation.wrapping_add(1);
                    cleared_session_title_generation = Some(runtime.title_generation);
                    if let Some(invalidated_session_id) = runtime.session_id.take() {
                        runtime.invalidated_session_id = Some(invalidated_session_id);
                        runtime.session_agent_class = None;
                        cleared_session_origin_name =
                            Some(runtime.origin.pane_name_without_stale_session().to_string());
                    }
                } else if submitted_line
                    .as_deref()
                    .is_some_and(crate::pane::clears_agent_conversation)
                    && runtime.session_agent_class.is_some()
                {
                    // `/clear` can retain the same live agent process and
                    // transcript identity. Its title lifecycle is separate
                    // from session discovery, so invalidate only the
                    // LLM-title generation and retain the verified ID.
                    runtime.title_generation = runtime.title_generation.wrapping_add(1);
                    runtime.is_showing_fresh_agent_screen = true;
                    cleared_conversation_title_generation = Some(runtime.title_generation);
                    cleared_conversation_origin_name =
                        Some(runtime.origin.pane_name_without_stale_session().to_string());
                }
                if bytes.contains(&b'\r') {
                    crate::detection::force_check(
                        &mut runtime.detection_schedule,
                        std::time::Instant::now(),
                    );
                }
                None
            }
        }
        Some(PaneResource::Editor { .. }) => {
            Some(format!("pane {pane_id:?} is an editor, not a terminal"))
        }
        None => Some(format!("no pane found for node {pane_id:?}")),
    };
    drop(panes);

    if let Some(message) = error_message {
        return Err(message);
    }

    if let Some(title_generation) = cleared_session_title_generation {
        state.request_snapshot_save();
        state.broadcast(ServerEvent::PaneSessionIdCleared {
            pane_id,
            title_generation,
        });
    }
    if let Some(title_generation) = cleared_conversation_title_generation {
        state.broadcast(ServerEvent::PaneSessionTitleCleared {
            pane_id,
            title_generation,
        });
    }

    // A session-transition reset takes precedence over a shell title from
    // the same byte batch. In practice they are mutually exclusive, but the
    // ordering makes the stale LLM title impossible to retain if input and
    // foreground detection race.
    let automatic_title = cleared_session_origin_name
        .or(cleared_conversation_origin_name)
        .or(observed_title);
    let Some(title) = automatic_title else {
        return Ok(());
    };
    let title_changed = {
        let mut tree = state.tree.write().await;
        // The typed-command echo has no distinct short form, unlike an LLM
        // inference -- `None` clears any stale short title left over from
        // an earlier automatic inference for this same pane.
        match tree.set_automatic_pane_title(pane_id, title, None) {
            Ok(changed) => changed,
            Err(error) => {
                tracing::error!("automatic title update rejected for pane {pane_id:?}: {error}");
                false
            }
        }
    };
    if title_changed {
        broadcast_and_persist(state).await;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn handle_mouse_input(
    state: &Arc<ServerState>,
    pane_id: NodeId,
    kind: ilium_ipc::MouseEventKind,
    column: u16,
    row: u16,
    modifiers: ilium_ipc::MouseModifiers,
    direct_tx: &mpsc::Sender<ServerEvent>,
) {
    // Same rationale as `handle_resize_pane`/`handle_key_input`: resolve the
    // outcome under the lock, send only after dropping it.
    let panes = state.panes.read().await;
    let error_message = match panes.get(&pane_id) {
        Some(PaneResource::Terminal(runtime)) => {
            let event = to_crossterm_event(kind, column, row, modifiers);
            runtime
                .session
                .write_mouse_input(event, column, row)
                .err()
                .map(|error| format!("failed to forward mouse input to pane {pane_id:?}: {error}"))
        }
        Some(PaneResource::Editor { .. }) => {
            Some(format!("pane {pane_id:?} is an editor, not a terminal"))
        }
        None => Some(format!("no pane found for node {pane_id:?}")),
    };
    drop(panes);

    if let Some(message) = error_message {
        send_direct_error(direct_tx, message).await;
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

    // A cleanly-killed session has nothing left worth recovering. Clear
    // the dirty flag first so the background debounced writer
    // (`crate::persistence::spawn_snapshot_writer`) never recreates the
    // file after we remove it below just because an earlier mutation's
    // dirty flag was still set; then take the same write lock
    // `persistence::save_snapshot` holds for a save's entire
    // build+serialize+write+rename, so a write already in flight when
    // this handler runs is guaranteed to finish (writing the *old*
    // snapshot) before we remove the file, rather than racing it.
    state
        .snapshot_dirty
        .store(false, std::sync::atomic::Ordering::Release);
    {
        let _write_guard = state.snapshot_write_lock.lock().await;
        match tokio::fs::remove_file(&state.snapshot_path).await {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                tracing::warn!("failed to remove snapshot file on session kill: {error}")
            }
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_line_initial_input_is_written_verbatim() {
        assert_eq!(initial_input_bytes("/goal one line"), b"/goal one line");
    }

    #[test]
    fn multiline_initial_input_uses_one_bracketed_paste() {
        assert_eq!(
            initial_input_bytes("/goal first\nsecond"),
            b"\x1b[200~/goal first\nsecond\x1b[201~"
        );
    }
}

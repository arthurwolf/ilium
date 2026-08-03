//! Translates each `ilium_ipc::ClientRequest` variant into a mutation on
//! `ServerState`'s tree/pane registry, broadcasting the resulting
//! `ServerEvent` to every attached client for structural changes (tree
//! shape, pane status) or replying only to the requesting connection for
//! everything else (the initial `Attach` snapshot, request-specific
//! errors). See `crate::ipc::connection` for how the two reply channels
//! (`ServerState::events` broadcast vs. this connection's own `direct_tx`)
//! are wired together on the write side.

use std::collections::HashMap;
use std::sync::Arc;

use ilium_agent_debug::{
    AgentDebugEventDraft, AgentDebugEventKind, AgentDebugField, AgentDebugSeverity,
    AgentDebugSource, PaneResizeCause,
};
use ilium_core::{
    AgentProvider, BuiltinAgentProvider, NodeId, NodeKind, PaneContentKind, PaneStatus,
    PaneTitleSource, PromptQueueDelivery, QueuedPrompt, RestructurePlan, ScheduledPaneInput,
    SessionIdentityTransitionRule, Tree, TreeError,
};
use ilium_ipc::{
    ClientRequest, NewPaneKind, NewPaneWorkingDirectory, PromptSubmissionSource, ServerEvent,
};
use ilium_pty::PtyError;
use tokio::sync::mpsc;

use crate::mouse::to_crossterm_event;
use crate::pane;
use crate::pane::{PaneResource, PaneSnapshotKind, TerminalOrigin};
use crate::state::ServerState;

/// Immutable forensic facts captured before an input-driven transition clears
/// the runtime's current owner fields. The debug event is emitted only after
/// releasing the pane lock, so it must not reconstruct these facts afterward.
struct SessionTransitionObservation {
    previous_session_id: Option<String>,
    previous_agent_class: Option<ilium_core::AgentClass>,
    previous_process_id: Option<u32>,
    previous_title_generation: u64,
    next_title_generation: u64,
    submitted_command: String,
    rule: SessionIdentityTransitionRule,
}

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
                let parent_group = resolve_parent_group(tree, parent_group, &state.session_cwd);
                tree.add_group(parent_group, name).map(|_id| ())
            })
            .await;
            false
        }
        ClientRequest::NewFolder { parent_group, path } => {
            handle_tree_mutation(state, direct_tx, |tree| {
                let parent_group = resolve_parent_group(tree, parent_group, &state.session_cwd);
                tree.add_folder(parent_group, path).map(|_id| ())
            })
            .await;
            false
        }
        ClientRequest::NewProject { path } => {
            handle_new_project(state, path, direct_tx).await;
            false
        }
        ClientRequest::ChangeProjectFolder { project_id, path } => {
            handle_change_project_folder(state, project_id, path, direct_tx).await;
            false
        }
        ClientRequest::NewBoard {
            parent_group,
            name,
            storage,
        } => {
            handle_tree_mutation(state, direct_tx, |tree| {
                let parent_group = resolve_parent_group(tree, parent_group, &state.session_cwd);
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
                let parent_group = resolve_parent_group(tree, parent_group, &state.session_cwd);
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
            inferred_icon,
        } => {
            handle_tree_mutation(state, direct_tx, |tree| {
                tree.rename_node(node_id, title, short_title, inferred_icon)
            })
            .await;
            false
        }
        ClientRequest::SetNodeBookmarked {
            node_id,
            is_bookmarked,
        } => {
            handle_tree_mutation(state, direct_tx, |tree| {
                tree.set_node_bookmarked(node_id, is_bookmarked)
            })
            .await;
            false
        }
        ClientRequest::RecordNodeActivity { node_id } => {
            if let Err(error) = record_node_activity(state, node_id).await {
                send_direct_error(direct_tx, error).await;
            }
            false
        }
        ClientRequest::SetAutomaticPaneTitle {
            pane_id,
            title,
            short_title,
            inferred_icon,
        } => {
            handle_automatic_pane_title(state, pane_id, title, short_title, inferred_icon).await;
            false
        }
        ClientRequest::SetSessionPaneTitle {
            pane_id,
            expected_session_id,
            expected_title_generation,
            title,
            short_title,
            inferred_icon,
            title_source,
        } => {
            handle_session_pane_title(
                state,
                SessionPaneTitleUpdate {
                    pane_id,
                    expected_session_id: &expected_session_id,
                    expected_title_generation,
                    title,
                    short_title,
                    inferred_icon,
                    title_source,
                },
            )
            .await;
            false
        }
        ClientRequest::ResizePane {
            pane_id,
            rows,
            cols,
            cause,
        } => {
            handle_resize_pane(state, pane_id, rows, cols, cause, direct_tx).await;
            false
        }
        ClientRequest::KeyInput {
            pane_id,
            bytes,
            submission,
        } => {
            handle_key_input(state, pane_id, &bytes, submission, direct_tx).await;
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
        ClientRequest::EnqueuePrompt {
            pane_id,
            text,
            delivery,
        } => {
            handle_enqueue_prompt(state, pane_id, text, delivery, direct_tx).await;
            false
        }
        ClientRequest::ClearPromptQueue { pane_id } => {
            handle_clear_prompt_queue(state, pane_id, direct_tx).await;
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
        ClientRequest::ApplyProjectRestructurePlan {
            project_id,
            plan,
            inference_activity_revisions,
        } => {
            handle_apply_project_restructure_plan(
                state,
                project_id,
                plan,
                &inference_activity_revisions,
                direct_tx,
            )
            .await;
            false
        }
        ClientRequest::RevertProjectRestructure { project_id } => {
            handle_revert_project_restructure(state, project_id, direct_tx).await;
            false
        }
        ClientRequest::UpdateDebugLogging { enabled } => {
            if !enabled {
                tracing::info!("server file logging disabled from Debug settings");
            }
            if let Err(error) = ilium_logging::set_enabled(enabled) {
                tracing::error!(%error, "failed to apply Debug file logging setting");
                send_direct_error(
                    direct_tx,
                    format!("failed to apply Debug file logging setting: {error}"),
                )
                .await;
            } else {
                if enabled {
                    tracing::info!("server file logging enabled from Debug settings");
                }
                state.broadcast(ServerEvent::DebugLoggingChanged { enabled });
            }
            false
        }
        ClientRequest::UpdateAgentDebugMenu { enabled } => {
            handle_update_agent_debug_menu(state, enabled).await;
            false
        }
        ClientRequest::GetPaneDebugLog {
            pane_id,
            after_sequence,
        } => {
            handle_get_pane_debug_log(state, pane_id, after_sequence, direct_tx).await;
            false
        }
        ClientRequest::RecordAgentDebugEvent {
            pane_id,
            expected_session_id,
            expected_title_generation,
            event,
        } => {
            let outcome = crate::agent_debug::record_client_event(
                state,
                pane_id,
                expected_session_id.as_deref(),
                expected_title_generation,
                event,
            )
            .await;
            if outcome != crate::agent_debug::ClientEventRecordOutcome::Recorded {
                tracing::debug!(
                    pane_id = pane_id.0,
                    ?outcome,
                    "best-effort agent debug event was not recorded"
                );
            }
            false
        }
    }
}

async fn handle_update_agent_debug_menu(state: &ServerState, enabled: bool) {
    if state.agent_debug.is_enabled() == enabled {
        state.broadcast(ServerEvent::AgentDebugMenuChanged { enabled });
        return;
    }

    if enabled {
        state.agent_debug.set_enabled(true);
    }
    let pane_ids: Vec<NodeId> = {
        let tree = state.tree.read().await;
        tree.all_ids()
            .filter(|pane_id| {
                tree.get(*pane_id).is_some_and(|node| {
                    matches!(
                        &node.kind,
                        NodeKind::Pane {
                            status: PaneStatus::Agent(_, _) | PaneStatus::AgentWithGoal(_, _),
                            ..
                        }
                    )
                })
            })
            .collect()
    };
    let event_kind = if enabled {
        AgentDebugEventKind::RecordingEnabled
    } else {
        AgentDebugEventKind::RecordingDisabled
    };
    let summary = if enabled {
        "Agent debug recording enabled"
    } else {
        "Agent debug recording disabled"
    };
    for pane_id in pane_ids {
        let _ = crate::agent_debug::record(
            state,
            pane_id,
            AgentDebugSource::Server,
            AgentDebugEventDraft::information(event_kind.clone(), summary),
        )
        .await;
    }
    if !enabled {
        state.agent_debug.set_enabled(false);
    }
    state.broadcast(ServerEvent::AgentDebugMenuChanged { enabled });
}

async fn handle_get_pane_debug_log(
    state: &ServerState,
    pane_id: NodeId,
    after_sequence: Option<u64>,
    direct_tx: &mpsc::Sender<ServerEvent>,
) {
    let is_terminal_pane = state.tree.read().await.get(pane_id).is_some_and(|node| {
        matches!(
            node.kind,
            NodeKind::Pane {
                content: PaneContentKind::Terminal,
                ..
            }
        )
    });
    if !is_terminal_pane {
        send_direct_error(direct_tx, format!("no terminal pane found for {pane_id:?}")).await;
        return;
    }

    let (through_sequence, retained_from_sequence, dropped_entry_count, entries) = state
        .agent_debug
        .replay(pane_id, after_sequence)
        .await
        .unwrap_or((0, 1, 0, Vec::new()));
    send_direct(
        direct_tx,
        ServerEvent::PaneDebugLogSnapshot {
            pane_id,
            through_sequence,
            retained_from_sequence,
            dropped_entry_count,
            entries,
        },
    )
    .await;
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
    let message = message.into();
    tracing::error!(%message, "request failed");
    send_direct(direct_tx, ServerEvent::Error { message }).await;
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
pub(crate) async fn broadcast_and_persist(state: &ServerState) {
    let snapshot = tree_snapshot(state).await;
    // Takes its own fresh tree read rather than reusing `snapshot` above --
    // see `ServerState::prune_stale_restructure_undo`'s doc comment for why
    // that snapshot, taken moments earlier, is not safe to reuse here.
    state.prune_stale_restructure_undo().await;
    state.broadcast(ServerEvent::TreeSnapshot(snapshot));
    state.request_snapshot_save();
}

/// Advances one entry revision and notifies every attachment. Persistence is
/// needed only when either independent checkpoint first becomes stale; later
/// live revisions still fence in-flight plans without causing a snapshot write
/// for every output chunk.
pub(crate) async fn record_node_activity(
    state: &ServerState,
    node_id: NodeId,
) -> Result<u64, String> {
    let update = {
        let mut tree = state.tree.write().await;
        tree.record_node_activity(node_id)
            .map_err(|error| format!("could not record activity for {node_id:?}: {error}"))?
    };
    publish_node_activity_update(state, node_id, update);
    Ok(update.activity_revision)
}

/// Publishes one already-committed core activity transition. Detection owns a
/// larger tree/pane transaction and therefore records inside that transaction;
/// ordinary input/output callers use [`record_node_activity`] above.
pub(crate) fn publish_node_activity_update(
    state: &ServerState,
    node_id: NodeId,
    update: ilium_core::NodeActivityUpdate,
) {
    state.broadcast(ServerEvent::NodeActivityChanged {
        node_id,
        activity_revision: update.activity_revision,
    });
    if update.became_unrestructured || update.became_unread_since_focus {
        state.request_snapshot_save();
    }
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
    let event_text = text.clone();
    let result = {
        let mut tree = state.tree.write().await;
        let was_replacement = tree
            .scheduled_pane_inputs()
            .any(|(candidate_id, _)| candidate_id == pane_id);
        tree.schedule_pane_input(
            pane_id,
            ScheduledPaneInput {
                execute_at_unix_millis,
                text,
                send_enter,
            },
        )
        .map(|()| was_replacement)
    };
    // The authoritative replacement is now visible; snapshots and client
    // broadcasts do not need to delay the executor's next freshness check.
    drop(transaction);
    let was_replacement = match result {
        Ok(was_replacement) => was_replacement,
        Err(error) => {
            send_direct_error(direct_tx, format!("failed to schedule pane input: {error}")).await;
            return;
        }
    };
    broadcast_and_persist(state).await;
    let _ = crate::agent_debug::record(
        state,
        pane_id,
        AgentDebugSource::Server,
        AgentDebugEventDraft::information(
            if was_replacement {
                AgentDebugEventKind::ScheduledInputReplaced
            } else {
                AgentDebugEventKind::ScheduledInputCreated
            },
            if was_replacement {
                "Scheduled input replaced"
            } else {
                "Scheduled input created"
            },
        )
        .with_fields(vec![
            AgentDebugField::plain("delay seconds", delay_seconds.to_string()),
            AgentDebugField::plain("execute at", execute_at_unix_millis.to_string()),
            AgentDebugField::plain("sends Enter", send_enter.to_string()),
            AgentDebugField::sensitive("input", event_text),
        ]),
    )
    .await;
    state.scheduled_input_changed.notify_one();
}

async fn handle_enqueue_prompt(
    state: &Arc<ServerState>,
    pane_id: NodeId,
    text: String,
    delivery: PromptQueueDelivery,
    direct_tx: &mpsc::Sender<ServerEvent>,
) {
    let event_text = text.clone();
    let event_delivery = format!("{delivery:?}");
    let _transaction = state.prompt_queue_transaction.lock().await;
    let result = state
        .tree
        .write()
        .await
        .enqueue_prompt(pane_id, QueuedPrompt { text, delivery });
    drop(_transaction);
    if let Err(error) = result {
        send_direct_error(direct_tx, format!("failed to enqueue prompt: {error}")).await;
        return;
    }
    broadcast_and_persist(state).await;
    let _ = crate::agent_debug::record(
        state,
        pane_id,
        AgentDebugSource::Server,
        AgentDebugEventDraft::information(
            AgentDebugEventKind::PromptQueued,
            "Prompt added to the completion queue",
        )
        .with_fields(vec![
            AgentDebugField::plain("delivery", event_delivery),
            AgentDebugField::sensitive("prompt", event_text),
        ]),
    )
    .await;
}

async fn handle_clear_prompt_queue(
    state: &Arc<ServerState>,
    pane_id: NodeId,
    direct_tx: &mpsc::Sender<ServerEvent>,
) {
    let _transaction = state.prompt_queue_transaction.lock().await;
    let result = {
        let mut tree = state.tree.write().await;
        let cleared_count = tree
            .get(pane_id)
            .and_then(|node| match &node.kind {
                NodeKind::Pane { prompt_queue, .. } => Some(prompt_queue.len()),
                _ => None,
            })
            .unwrap_or(0);
        tree.clear_prompt_queue(pane_id).map(|()| cleared_count)
    };
    drop(_transaction);
    let cleared_count = match result {
        Ok(cleared_count) => cleared_count,
        Err(error) => {
            send_direct_error(direct_tx, format!("failed to clear prompt queue: {error}")).await;
            return;
        }
    };
    broadcast_and_persist(state).await;
    let _ = crate::agent_debug::record(
        state,
        pane_id,
        AgentDebugSource::Server,
        AgentDebugEventDraft::information(
            AgentDebugEventKind::PromptQueueCleared,
            "Prompt queue cleared",
        )
        .with_fields(vec![AgentDebugField::plain(
            "removed prompts",
            cleared_count.to_string(),
        )]),
    )
    .await;
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
    let recovery_pane_count = state
        .pending_session_recovery
        .lock()
        .await
        .as_ref()
        .map(|snapshot| snapshot.panes.len());
    if let Some(pane_count) = recovery_pane_count {
        let snapshot = state.tree.read().await.clone();
        send_direct(direct_tx, ServerEvent::TreeSnapshot(snapshot)).await;
        send_direct(
            direct_tx,
            ServerEvent::SessionRecoveryAvailable { pane_count },
        )
        .await;
        return;
    }

    send_initial_state(state, direct_tx).await;
}

/// Sends one ordered, complete client render-cache seed. Both normal attach
/// and post-recovery resolution use this exact path so the startup trigger
/// always observes the same state boundary.
async fn send_initial_state(state: &ServerState, direct_tx: &mpsc::Sender<ServerEvent>) {
    for event in initial_state_events(state, true).await {
        send_direct(direct_tx, event).await;
    }
}

/// Selects how terminal journals synchronize for one connection. Attach has
/// no prior parser and needs full replays; live recovery appends exact missing
/// deltas while retained, falling back to replay only past the journal window.
enum TerminalOutputSynchronization<'a> {
    All,
    RecoverAfter(&'a HashMap<NodeId, u64>),
}

impl TerminalOutputSynchronization<'_> {
    /// Builds the smallest event that makes one terminal parser current.
    fn event_for(&self, pane_id: NodeId, session: &ilium_pty::PtySession) -> Option<ServerEvent> {
        match self {
            Self::All => Some(terminal_replay_event(pane_id, session.output_replay())),
            Self::RecoverAfter(delivered_sequences) => {
                let after_sequence = delivered_sequences
                    .get(&pane_id)
                    .copied()
                    .unwrap_or_default();
                match session.output_recovery_after(after_sequence) {
                    Some(ilium_pty::PtyOutputRecovery::Delta(chunk)) => {
                        Some(ServerEvent::ScreenUpdate {
                            pane_id,
                            first_sequence: after_sequence.saturating_add(1),
                            sequence: chunk.sequence,
                            bytes: chunk.bytes,
                        })
                    }
                    Some(ilium_pty::PtyOutputRecovery::Replay(replay)) => {
                        Some(terminal_replay_event(pane_id, replay))
                    }
                    None => None,
                }
            }
        }
    }
}

/// Builds the complete attach stream. Startup alone gets the explicit
/// completion boundary used by automatic triggers.
pub(crate) async fn initial_state_events(
    state: &ServerState,
    include_initial_sync_complete: bool,
) -> Vec<ServerEvent> {
    state_synchronization_events(
        state,
        TerminalOutputSynchronization::All,
        include_initial_sync_complete,
    )
    .await
}

/// Builds a live lag-recovery stream without replaying terminal histories
/// this exact connection has already received. Tree and pane metadata remain
/// full snapshots because they are small and replaceable; raw PTY history is
/// the expensive, parser-resetting state that must stay pane-scoped.
pub(crate) async fn resynchronization_events(
    state: &ServerState,
    delivered_terminal_sequences: &HashMap<NodeId, u64>,
) -> Vec<ServerEvent> {
    state_synchronization_events(
        state,
        TerminalOutputSynchronization::RecoverAfter(delivered_terminal_sequences),
        false,
    )
    .await
}

/// Builds one ordered render-cache seed from current server authority.
async fn state_synchronization_events(
    state: &ServerState,
    terminal_output_synchronization: TerminalOutputSynchronization<'_>,
    include_initial_sync_complete: bool,
) -> Vec<ServerEvent> {
    // Terminal scrollback, session IDs, and editor paths belong to live pane
    // resources rather than the persisted tree wire shape, so replay them
    // explicitly after the attachment snapshot has established matching node
    // ids. A replay is captured atomically with its output sequence by
    // `PtySession`; the client uses that sequence to drop any duplicate live
    // update that was queued while this attach was in flight.
    //
    // Capture tree and pane resources under the documented tree-before-panes
    // lock order. Otherwise a concurrent create/close could put output for a
    // pane outside the accompanying tree snapshot, making the client discard
    // bytes while this connection incorrectly advanced its delivery
    // watermark. No socket write occurs under either lock.
    let (snapshot, replay_events): (Tree, Vec<ServerEvent>) = {
        let tree = state.tree.read().await;
        let panes = state.panes.read().await;
        let snapshot = tree.clone();
        let replay_events = panes
            .iter()
            .flat_map(|(pane_id, resource)| match resource {
                PaneResource::Terminal(runtime) => {
                    let mut events = Vec::new();
                    if let Some(event) =
                        terminal_output_synchronization.event_for(*pane_id, &runtime.session)
                    {
                        events.push(event);
                    }
                    if let Some(session_id) = runtime.session_id.clone() {
                        events.push(ServerEvent::PaneSessionIdResolved {
                            pane_id: *pane_id,
                            session_id,
                            process_id: runtime.session_process_id,
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
            .collect();
        (snapshot, replay_events)
    };
    let mut events = vec![ServerEvent::TreeSnapshot(snapshot)];
    events.extend(replay_events);
    if include_initial_sync_complete {
        events.push(ServerEvent::InitialStateSyncComplete);
    }
    events
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
    send_initial_state(state, direct_tx).await;
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

async fn handle_new_project(
    state: &Arc<ServerState>,
    path: std::path::PathBuf,
    direct_tx: &mpsc::Sender<ServerEvent>,
) {
    let path = match canonical_project_directory(path) {
        Ok(path) => path,
        Err(message) => {
            send_direct_error(direct_tx, message).await;
            return;
        }
    };
    handle_tree_mutation(state, direct_tx, |tree| tree.add_project(path).map(|_| ())).await;
}

async fn handle_change_project_folder(
    state: &Arc<ServerState>,
    project_id: NodeId,
    path: std::path::PathBuf,
    direct_tx: &mpsc::Sender<ServerEvent>,
) {
    let path = match canonical_project_directory(path) {
        Ok(path) => path,
        Err(message) => {
            send_direct_error(direct_tx, message).await;
            return;
        }
    };
    handle_tree_mutation(state, direct_tx, |tree| {
        tree.change_project_folder(project_id, path)
    })
    .await;
}

fn canonical_project_directory(path: std::path::PathBuf) -> Result<std::path::PathBuf, String> {
    let canonical = path
        .canonicalize()
        .map_err(|error| format!("project folder is unavailable: {error}"))?;
    if !canonical.is_dir() {
        return Err("project folder must be a directory".to_string());
    }
    Ok(canonical)
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
    let Some(project_id) = tree.project_ids().into_iter().next() else {
        drop(tree);
        send_direct_error(direct_tx, "restructure failed: no project exists").await;
        return;
    };
    if tree.project_ids().len() != 1 {
        drop(tree);
        send_direct_error(
            direct_tx,
            "restructure failed: select a project to restructure",
        )
        .await;
        return;
    }
    let before = tree.clone();
    let result = tree.apply_project_restructure(project_id, plan);
    drop(tree);
    match result {
        Ok(()) => {
            state
                .restructure_undo
                .lock()
                .await
                .insert(project_id, before);
            broadcast_and_persist(state).await;
        }
        Err(error) => send_direct_error(direct_tx, format!("restructure failed: {error}")).await,
    }
}

/// Restores one project's one-slot undo buffer from its latest successful
/// restructure. A successful revert consumes that project's slot, so a
/// second revert without another restructure is an error rather than a
/// toggle back and forth between two states.
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
    let project_ids = state.tree.read().await.project_ids();
    if project_ids.len() != 1 {
        send_direct_error(direct_tx, "select a project to revert its restructure").await;
        return;
    }
    handle_revert_project_restructure(state, project_ids[0], direct_tx).await;
}

async fn handle_apply_project_restructure_plan(
    state: &Arc<ServerState>,
    project_id: NodeId,
    plan: RestructurePlan,
    inference_activity_revisions: &[ilium_core::NodeActivityRevision],
    direct_tx: &mpsc::Sender<ServerEvent>,
) {
    let mut tree = state.tree.write().await;
    let before = tree.clone();
    let result = tree.apply_project_restructure_with_activity_checkpoint(
        project_id,
        plan,
        inference_activity_revisions,
    );
    drop(tree);
    match result {
        Ok(checkpoint_activity_revisions) => {
            state
                .restructure_undo
                .lock()
                .await
                .insert(project_id, before);
            broadcast_and_persist(state).await;
            send_direct(
                direct_tx,
                ServerEvent::ProjectRestructureApplied {
                    project_id,
                    checkpoint_activity_revisions,
                },
            )
            .await;
        }
        Err(error) => {
            let message = format!("restructure failed: {error}");
            tracing::error!(%message, "request failed");
            send_direct(
                direct_tx,
                ServerEvent::ProjectRestructureRejected {
                    project_id,
                    message,
                },
            )
            .await;
        }
    }
}

async fn handle_revert_project_restructure(
    state: &Arc<ServerState>,
    project_id: NodeId,
    direct_tx: &mpsc::Sender<ServerEvent>,
) {
    let previous = state.restructure_undo.lock().await.remove(&project_id);
    match previous {
        Some(previous_tree) => {
            let mut tree = state.tree.write().await;
            let orphaned_pane_ids: Vec<NodeId> = collect_pane_descendants(&tree, project_id)
                .into_iter()
                .filter(|pane_id| previous_tree.get(*pane_id).is_none())
                .collect();
            let result = tree.restore_project_from(project_id, &previous_tree);
            if let Err(error) = result {
                drop(tree);
                state
                    .restructure_undo
                    .lock()
                    .await
                    .insert(project_id, previous_tree);
                send_direct_error(direct_tx, format!("could not revert restructure: {error}"))
                    .await;
                return;
            }
            // Keep the write guard held across the pane-registry teardown
            // below -- see `spawn_and_register_pane_in_directory`'s doc
            // comment for why this tree-overwrite-then-sweep pair must stay
            // atomic with respect to that function's own tree-check-then-
            // panes-insert pair, under the "tree before panes" ordering
            // `state.rs` documents. Only dropped afterward, still before
            // `broadcast_and_persist`'s own read-locked clone -- see that
            // function's docs.
            if !orphaned_pane_ids.is_empty() {
                let mut panes = state.panes.write().await;
                for pane_id in &orphaned_pane_ids {
                    if let Some(resource) = panes.remove(pane_id) {
                        teardown_pane_resource(*pane_id, resource);
                    }
                }
                drop(panes);
            }
            drop(tree);
            // Mirrors `handle_close_pane`'s teardown ordering -- evict the
            // orphaned panes' `AgentDebugRecorder` journals (and their
            // change-only observation state) once the tree/panes locks are
            // released, so a reverted restructure never leaves a dangling
            // per-pane journal that keeps being re-persisted into every
            // future session snapshot (see `AgentDebugRecorder` docs).
            if !orphaned_pane_ids.is_empty() {
                state.agent_debug.remove(&orphaned_pane_ids).await;
            }

            broadcast_and_persist(state).await;
        }
        None => send_direct_error(direct_tx, "no restructure to revert for this project").await,
    }
}

/// Applies an automatic title only while the user has not explicitly named
/// the pane, then publishes the changed tree just like any other title edit.
async fn handle_automatic_pane_title(
    state: &Arc<ServerState>,
    pane_id: NodeId,
    title: String,
    short_title: Option<String>,
    inferred_icon: Option<String>,
) {
    let tree_changed = {
        let mut tree = state.tree.write().await;
        match tree.set_automatic_pane_title(pane_id, title, short_title, inferred_icon) {
            Ok(changed) => changed,
            Err(error) => {
                tracing::warn!("automatic title update rejected for pane {pane_id:?}: {error}");
                false
            }
        }
    };
    if tree_changed {
        broadcast_and_persist(state).await;
    }
}

/// Applies an LLM title as a compare-and-set against the server's live
/// session identity. A stale client or in-flight worker can never title the
/// replacement session, regardless of IPC event/request ordering.
struct SessionPaneTitleUpdate<'a> {
    pane_id: NodeId,
    expected_session_id: &'a str,
    expected_title_generation: u64,
    title: String,
    short_title: Option<String>,
    inferred_icon: Option<String>,
    title_source: PaneTitleSource,
}

async fn handle_session_pane_title(state: &Arc<ServerState>, update: SessionPaneTitleUpdate<'_>) {
    let proposed_title = update.title.clone();
    let expected_session_id = update.expected_session_id.to_string();
    let expected_title_generation = update.expected_title_generation;
    // Lock ordering is tree before panes throughout the server. Both remain
    // held through the identity check and title write so `/resume` cannot
    // invalidate the session between those two operations.
    let mut tree = state.tree.write().await;
    let panes = state.panes.read().await;
    let Some(PaneResource::Terminal(runtime)) = panes.get(&update.pane_id) else {
        return;
    };
    if runtime.is_session_identity_invalidated
        || runtime.session_id.as_deref() != Some(update.expected_session_id)
        || runtime.title_generation != update.expected_title_generation
    {
        drop(panes);
        drop(tree);
        let _ = crate::agent_debug::record(
            state,
            update.pane_id,
            AgentDebugSource::Inference,
            AgentDebugEventDraft {
                severity: AgentDebugSeverity::Warning,
                kind: AgentDebugEventKind::TitleInferenceDiscarded,
                summary: "Stale title result rejected by the server".to_string(),
                fields: vec![
                    AgentDebugField::plain("expected session", expected_session_id),
                    AgentDebugField::plain(
                        "expected title generation",
                        expected_title_generation.to_string(),
                    ),
                    AgentDebugField::plain("proposed title", proposed_title),
                ],
                correlation_id: None,
                metadata: Default::default(),
            },
        )
        .await;
        return;
    }
    let changed = match update.title_source {
        PaneTitleSource::Automatic => tree
            .set_automatic_pane_title(
                update.pane_id,
                update.title,
                update.short_title,
                update.inferred_icon,
            )
            .unwrap_or_else(|error| {
                tracing::warn!(
                    "session title update rejected for pane {:?}: {error}",
                    update.pane_id
                );
                false
            }),
        PaneTitleSource::UserSpecified => {
            match tree.rename_node(
                update.pane_id,
                update.title,
                update.short_title,
                update.inferred_icon,
            ) {
                Ok(()) => true,
                Err(error) => {
                    tracing::warn!(
                        "session retitle rejected for pane {:?}: {error}",
                        update.pane_id
                    );
                    false
                }
            }
        }
    };
    drop(panes);
    drop(tree);
    if changed {
        broadcast_and_persist(state).await;
        let _ = crate::agent_debug::record(
            state,
            update.pane_id,
            AgentDebugSource::Inference,
            AgentDebugEventDraft {
                severity: AgentDebugSeverity::Success,
                kind: AgentDebugEventKind::TitleApplied,
                summary: "Agent pane title applied".to_string(),
                fields: vec![
                    AgentDebugField::plain("title", proposed_title),
                    AgentDebugField::plain("session", expected_session_id),
                    AgentDebugField::plain(
                        "title generation",
                        expected_title_generation.to_string(),
                    ),
                ],
                correlation_id: None,
                metadata: Default::default(),
            },
        )
        .await;
    }
}

/// Records whether the attached client currently has `pane_id` as its active
/// view and forces an immediate (debounced) recheck on every focus transition.
/// Entering a pane also acknowledges its completed turn: the bell is an
/// unread-work indicator, so it must clear after the user opens that pane. No
/// error is surfaced for a missing pane because focus can harmlessly race pane
/// closure.
async fn handle_set_pane_focus(state: &Arc<ServerState>, pane_id: NodeId, focused: bool) {
    let focus_checkpoint = {
        let mut tree = state.tree.write().await;
        match tree.mark_node_focused(pane_id) {
            Ok(checkpoint) => checkpoint,
            Err(ilium_core::TreeError::NodeNotFound(_)) => return,
            Err(error) => {
                tracing::error!(
                    "focus activity acknowledgement rejected for pane {pane_id:?}: {error}"
                );
                None
            }
        }
    };
    if let Some(activity_revision) = focus_checkpoint {
        state.broadcast(ServerEvent::NodeFocusCheckpointChanged {
            node_id: pane_id,
            activity_revision,
        });
        state.request_snapshot_save();
    }

    let (detection_was_forced, is_terminal) = {
        let mut panes = state.panes.write().await;
        match panes.get_mut(&pane_id) {
            Some(PaneResource::Terminal(runtime)) => {
                runtime.detection_schedule.client_focused = focused;
                (
                    crate::detection::force_check(
                        &mut runtime.detection_schedule,
                        std::time::Instant::now(),
                    ),
                    true,
                )
            }
            Some(PaneResource::Editor { .. }) | None => (false, false),
        }
    };

    // The server remains authoritative for the acknowledgement. A client must
    // wait for this broadcast rather than mutating its cached tree locally, so
    // every attachment sees the same bell state and a concurrent detector can
    // still win with newer Working/Waiting activity.
    let acknowledged_status = if focused && is_terminal {
        let mut tree = state.tree.write().await;
        match tree.acknowledge_agent_completion(pane_id) {
            Ok(status) => status,
            Err(error) => {
                tracing::error!(
                    "agent completion acknowledgement on focus rejected for pane {pane_id:?}: {error}"
                );
                None
            }
        }
    } else {
        None
    };

    if detection_was_forced {
        state.detection_schedule_changed.notify_one();
    }
    if let Some(status) = acknowledged_status {
        state.broadcast(ServerEvent::PaneStatusChanged { pane_id, status });
    }
    let _ = crate::agent_debug::record(
        state,
        pane_id,
        AgentDebugSource::Server,
        AgentDebugEventDraft::information(
            AgentDebugEventKind::PaneFocused,
            if focused {
                "Agent pane focused"
            } else {
                "Agent pane unfocused"
            },
        )
        .with_fields(vec![AgentDebugField::plain("focused", focused.to_string())]),
    )
    .await;
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
fn resolve_parent_group(
    tree: &mut Tree,
    requested: NodeId,
    session_cwd: &std::path::Path,
) -> NodeId {
    if requested == ilium_core::ROOT_ID {
        tree.ensure_launch_project(session_cwd.to_path_buf())
            .expect("the session root can always create its launch project")
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

async fn handle_new_pane(
    state: &Arc<ServerState>,
    parent_group: NodeId,
    kind: NewPaneKind,
    working_directory: NewPaneWorkingDirectory,
    direct_tx: &mpsc::Sender<ServerEvent>,
) {
    let plan = new_pane_plan(kind);
    let spawn_description = format!("{:?}", plan.spawn_kind);

    let mut tree = state.tree.write().await;
    let parent_group = if parent_group == ilium_core::ROOT_ID {
        let project_id = tree
            .ensure_launch_project(state.session_cwd.clone())
            .expect("the session root can always create its launch project");
        tree.ensure_project_default_group(project_id, "default")
            .expect("a launch project always accepts its default group")
    } else {
        resolve_parent_group(&mut tree, parent_group, &state.session_cwd)
    };
    let pane_id = match tree.add_pane(parent_group, plan.name, plan.content_kind) {
        Ok(id) => id,
        Err(error) => {
            drop(tree);
            send_direct_error(direct_tx, format!("failed to create pane: {error}")).await;
            return;
        }
    };
    let project_cwd = tree
        .project_path_for(pane_id)
        .map(std::path::Path::to_path_buf)
        .unwrap_or_else(|| state.session_cwd.clone());
    // Drop the write guard before spawning (a pty spawn + registering it
    // in `state.panes` needs no tree access at all) and before the
    // eventual broadcast snapshot's O(n) clone -- see `broadcast_and_persist`.
    drop(tree);

    // The client only names a *policy*; the server alone holds the live PTY
    // process state (`panes`) and last-launch memory needed to resolve it to
    // an actual directory -- see `NewPaneWorkingDirectory`'s doc comment.
    let cwd = resolve_new_pane_working_directory(state, working_directory, &project_cwd).await;

    match spawn_and_register_pane_in_directory(state, pane_id, plan.spawn_kind, &cwd).await {
        Ok(()) => {}
        Err(RegisterPaneError::NodeRemoved(_)) => {
            // A concurrent request (`ClosePane` on an ancestor group,
            // `RevertLastRestructure`, a session-recovery restore) removed
            // `pane_id` from the tree while the spawn above was in flight.
            // That request already removed the node and broadcast the tree
            // without it, and `spawn_and_register_pane_in_directory` has
            // already torn the now-orphaned resource back down -- there is
            // nothing left for this call to remove or report.
            return;
        }
        Err(RegisterPaneError::Spawn(error)) => {
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

    // Make the pane addressable on every attached client before a semantic
    // initial-prompt event can arrive for it.
    broadcast_and_persist(state).await;
    let _ = crate::agent_debug::record(
        state,
        pane_id,
        AgentDebugSource::Server,
        AgentDebugEventDraft::information(
            AgentDebugEventKind::PaneCreated,
            "Pane created and resource registered",
        )
        .with_fields(vec![
            AgentDebugField::plain("origin", spawn_description),
            AgentDebugField::plain("working directory", cwd.display().to_string()),
        ]),
    )
    .await;

    if let Some(initial_input) = plan.initial_input {
        let _completion =
            crate::initial_prompt::start(Arc::clone(state), pane_id, initial_input).await;
    }
}

/// Creates one built-in coding agent inside the project represented by
/// `project_cwd`, then waits until the existing composer-readiness boundary
/// has submitted `prompt`. HTTP automation uses this instead of emulating an
/// IPC client, so the server remains the sole owner of project placement,
/// PTY lifecycle, and readiness-safe input delivery.
pub(crate) async fn create_agent_with_prompt(
    state: &Arc<ServerState>,
    provider: BuiltinAgentProvider,
    project_cwd: std::path::PathBuf,
    prompt: String,
) -> Result<NodeId, String> {
    let command_line = provider.command_line().to_string();
    let pane_id = {
        let mut tree = state.tree.write().await;
        let project_id = tree
            .project_ids()
            .into_iter()
            .find(|project_id| {
                tree.get(*project_id)
                    .and_then(ilium_core::Node::project_path)
                    .is_some_and(|path| path == project_cwd)
            })
            .map(Ok)
            .unwrap_or_else(|| tree.add_project(project_cwd.clone()))
            .map_err(|error| format!("failed to add project: {error}"))?;
        let group_id = tree
            .ensure_project_default_group(project_id, "default")
            .map_err(|error| format!("failed to prepare project group: {error}"))?;
        tree.add_pane(group_id, command_line.clone(), PaneContentKind::Terminal)
            .map_err(|error| format!("failed to create agent pane: {error}"))?
    };

    let origin = TerminalOrigin::Command(command_line.clone());
    if let Err(error) = spawn_and_register_pane_in_directory(
        state,
        pane_id,
        PaneSnapshotKind::Terminal(origin),
        &project_cwd,
    )
    .await
    {
        let mut tree = state.tree.write().await;
        let _ = tree.remove_node(pane_id);
        return Err(format!("failed to spawn {command_line}: {error}"));
    }

    broadcast_and_persist(state).await;
    let _ = crate::agent_debug::record(
        state,
        pane_id,
        AgentDebugSource::Server,
        AgentDebugEventDraft::information(
            AgentDebugEventKind::PaneCreated,
            "Agent created by HTTP API",
        )
        .with_fields(vec![
            AgentDebugField::plain("provider", provider.label()),
            AgentDebugField::plain("working directory", project_cwd.display().to_string()),
        ]),
    )
    .await;

    let completion = crate::initial_prompt::start(Arc::clone(state), pane_id, prompt).await;
    match tokio::time::timeout(std::time::Duration::from_secs(120), completion).await {
        Ok(Ok(Ok(()))) => Ok(pane_id),
        Ok(Ok(Err(error))) => Err(format!("agent prompt was not delivered: {error}")),
        Ok(Err(_)) => Err("agent prompt delivery was cancelled".to_string()),
        Err(_) => Err("timed out waiting for the agent input prompt".to_string()),
    }
}

/// Resolves a client's `NewPaneWorkingDirectory` policy to an actual starting
/// directory. `FocusedTerminal` and `LastUsed` both fall back to
/// `project_cwd` whenever no live candidate is available -- no pane is
/// currently client-focused, its cwd cannot be read on this platform, or no
/// terminal has launched yet this session -- mirroring
/// `PtySession::current_working_directory`'s own "fall back to the project
/// root safely" contract.
async fn resolve_new_pane_working_directory(
    state: &ServerState,
    working_directory: NewPaneWorkingDirectory,
    project_cwd: &std::path::Path,
) -> std::path::PathBuf {
    match working_directory {
        NewPaneWorkingDirectory::ProjectRoot => project_cwd.to_path_buf(),
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
                .unwrap_or_else(|| project_cwd.to_path_buf())
        }
        NewPaneWorkingDirectory::LastUsed => state
            .last_terminal_working_directory
            .lock()
            .await
            .clone()
            .unwrap_or_else(|| project_cwd.to_path_buf()),
    }
}

/// Failure from [`spawn_and_register_pane_in_directory`]. Distinguishes an
/// actual pty spawn failure (`Spawn`, unchanged behavior: report it to the
/// requesting client and remove the tree node that has no resource behind
/// it) from a node the tree no longer has by the time the spawned resource
/// was ready to register (`NodeRemoved`): a concurrent `ClosePane` on an
/// ancestor group, `RevertLastRestructure`, or a session-recovery restore
/// can remove `pane_id` from the tree while the pty spawn above is still in
/// flight. That is not a spawn failure -- the resource spawned fine -- and
/// the concurrent request that removed the node already broadcast the tree
/// without it, so callers must not try to remove the (already gone) tree
/// node again or report a spurious spawn error. See this function's doc
/// comment for how `NodeRemoved` is made unreachable-without-cleanup.
#[derive(Debug, thiserror::Error)]
pub(crate) enum RegisterPaneError {
    #[error(transparent)]
    Spawn(#[from] PtyError),
    #[error("pane node {0:?} was removed before it could be registered")]
    NodeRemoved(NodeId),
}

/// Spawns (for a `Terminal` origin) or registers (for an `Editor`) the
/// `PaneResource` for `pane_id` per `kind`, inserting it into
/// `state.panes`. `pane_id` must already exist in `state.tree` as a pane
/// node when this is called, but -- unlike its name once implied -- this
/// function does read the tree, precisely to guard against that node
/// having stopped existing by the time the (possibly slow) pty spawn below
/// completes; see [`RegisterPaneError::NodeRemoved`].
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
) -> Result<(), RegisterPaneError> {
    let cwd = state
        .tree
        .read()
        .await
        .project_path_for(pane_id)
        .map(std::path::Path::to_path_buf)
        .unwrap_or_else(|| state.session_cwd.clone());
    spawn_and_register_pane_in_directory(state, pane_id, kind, &cwd).await
}

pub(crate) async fn spawn_and_register_pane_in_directory(
    state: &Arc<ServerState>,
    pane_id: NodeId,
    kind: PaneSnapshotKind,
    cwd: &std::path::Path,
) -> Result<(), RegisterPaneError> {
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

    // Hold `tree` (read suffices -- this never mutates it) across both the
    // presence check and the `panes` insert below, per this crate's
    // documented "tree before panes" lock ordering (see `state.rs`).
    // `handle_close_pane`, `handle_revert_last_restructure`, and
    // `crate::restore_snapshot` each hold `tree`'s *write* lock across
    // their own remove-node-then-sweep-`panes` pair, so a `tokio::sync::
    // RwLock` read/write conflict makes this critical section and theirs
    // mutually exclusive: either this whole check-and-insert finishes
    // before such a removal starts (the node was live, the insert
    // succeeds, and that remover's later sweep correctly finds and tears
    // this entry down when it does run) or the removal -- and its sweep,
    // which finds nothing here yet because this resource is not inserted
    // until this line -- finishes first, and the check below observes the
    // node gone. Without this, the two operations could interleave with
    // the insert landing after the sweep had already run, permanently
    // orphaning the resource: its tree node would already be gone, so no
    // future sweep would ever look for it again.
    let tree = state.tree.read().await;
    if tree.get(pane_id).is_none() {
        drop(tree);
        teardown_pane_resource(pane_id, resource);
        return Err(RegisterPaneError::NodeRemoved(pane_id));
    }

    let mut panes = state.panes.write().await;
    panes.insert(pane_id, resource);
    drop(panes);
    drop(tree);

    if is_terminal {
        *state.last_terminal_working_directory.lock().await = Some(cwd.to_path_buf());
        state.detection_schedule_changed.notify_one();
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
            Ok(first_chunk) => {
                if let Err(error) = record_node_activity(&state, pane_id).await {
                    tracing::warn!("{error}");
                }
                // A bursty PTY reader commonly splits one visual update into
                // many tiny chunks.  Merge only already-queued chunks: this
                // adds no deliberate latency, while avoiding one broadcast,
                // one frame encode, and one client event per tiny read.
                const MAX_MERGED_CHUNKS: usize = 32;
                // Keep one forwarded frame below two PTY reader buffers so
                // a single chatty pane cannot monopolize a client turn even
                // after batching.
                const MAX_MERGED_BYTES: usize = 16 * 1024;
                let first_sequence = first_chunk.sequence;
                let mut sequence = first_sequence;
                let mut bytes = first_chunk.bytes;
                let mut replay_required = false;
                for _ in 1..MAX_MERGED_CHUNKS {
                    if bytes.len() >= MAX_MERGED_BYTES {
                        break;
                    }
                    match receiver.try_recv() {
                        Ok(mut chunk) => {
                            sequence = chunk.sequence;
                            bytes.append(&mut chunk.bytes);
                        }
                        Err(tokio::sync::broadcast::error::TryRecvError::Empty) => break,
                        Err(tokio::sync::broadcast::error::TryRecvError::Lagged(skipped)) => {
                            tracing::warn!(
                                "pane {pane_id:?} output forwarder lagged, skipped {skipped} chunk(s)"
                            );
                            replay_required = true;
                            break;
                        }
                        Err(tokio::sync::broadcast::error::TryRecvError::Closed) => break,
                    }
                }
                if replay_required {
                    broadcast_terminal_replay(&state, pane_id).await;
                    continue;
                }
                state.broadcast(ServerEvent::ScreenUpdate {
                    pane_id,
                    first_sequence,
                    sequence,
                    bytes,
                });
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                tracing::warn!(
                    "pane {pane_id:?} output forwarder lagged, skipped {skipped} chunk(s)"
                );
                broadcast_terminal_replay(&state, pane_id).await;
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
        }
    }
}

/// Repairs every attached client's terminal parser after this pane's
/// server-side output forwarder misses raw chunks. The PTY journal is the
/// authoritative replay source and carries a sequence watermark, so queued
/// live bytes at or below it are safely ignored by clients.
async fn broadcast_terminal_replay(state: &ServerState, pane_id: NodeId) {
    let replay_event = {
        let panes = state.panes.read().await;
        let Some(PaneResource::Terminal(runtime)) = panes.get(&pane_id) else {
            return;
        };
        terminal_replay_event(pane_id, runtime.session.output_replay())
    };
    state.broadcast(replay_event);
}

/// Converts one atomically captured PTY journal snapshot into the protocol
/// event used by both attach-time reconstruction and live lag recovery.
fn terminal_replay_event(pane_id: NodeId, replay: ilium_pty::PtyOutputReplay) -> ServerEvent {
    ServerEvent::TerminalReplay {
        pane_id,
        through_sequence: replay.through_sequence,
        bytes: replay.bytes,
        is_complete: replay.is_complete,
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
pub(crate) fn collect_pane_descendants(tree: &Tree, id: NodeId) -> Vec<NodeId> {
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

pub(crate) fn teardown_pane_resource(pane_id: NodeId, mut resource: PaneResource) {
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
    // Keep the write guard held across the pane-registry teardown below --
    // see `spawn_and_register_pane_in_directory`'s doc comment for why this
    // remove-then-sweep pair must stay atomic with respect to that
    // function's own tree-check-then-panes-insert pair, under the same
    // "tree before panes" ordering `state.rs` documents. Only dropped
    // afterward, still before the eventual broadcast snapshot's O(n) clone
    // -- see `broadcast_and_persist`.
    let mut panes = state.panes.write().await;
    for id in &descendant_pane_ids {
        if let Some(resource) = panes.remove(id) {
            teardown_pane_resource(*id, resource);
        }
    }
    drop(panes);
    drop(tree);
    state.agent_debug.remove(&descendant_pane_ids).await;

    broadcast_and_persist(state).await;
    state.scheduled_input_changed.notify_one();
}

async fn handle_resize_pane(
    state: &Arc<ServerState>,
    pane_id: NodeId,
    rows: u16,
    cols: u16,
    cause: PaneResizeCause,
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
    } else {
        let _ = crate::agent_debug::record(
            state,
            pane_id,
            AgentDebugSource::Pty,
            AgentDebugEventDraft::information(
                AgentDebugEventKind::PaneResized,
                "Agent PTY resized",
            )
            .with_fields(vec![
                AgentDebugField::plain("rows", rows.to_string()),
                AgentDebugField::plain("columns", cols.to_string()),
                AgentDebugField::plain("cause", cause.label()),
            ])
            .with_pane_resize_cause(cause),
        )
        .await;
    }
}

async fn handle_key_input(
    state: &Arc<ServerState>,
    pane_id: NodeId,
    bytes: &[u8],
    submission: Option<PromptSubmissionSource>,
    direct_tx: &mpsc::Sender<ServerEvent>,
) {
    if let Err(message) = write_key_input(state, pane_id, bytes, submission).await {
        let _ = crate::agent_debug::record(
            state,
            pane_id,
            AgentDebugSource::Pty,
            AgentDebugEventDraft {
                severity: AgentDebugSeverity::Error,
                kind: AgentDebugEventKind::Error,
                summary: "PTY input write failed".to_string(),
                fields: vec![AgentDebugField::multiline("error", message.clone())],
                correlation_id: None,
                metadata: Default::default(),
            },
        )
        .await;
        send_direct_error(direct_tx, message).await;
    }
}

/// Writes terminal bytes through the same title-tracking and detection path
/// for both live client keys and server-scheduled input. Keeping one input
/// boundary prevents delayed Enter from behaving differently from a key the
/// user pressed directly.
pub(crate) async fn write_key_input(
    state: &ServerState,
    pane_id: NodeId,
    bytes: &[u8],
    submission: Option<PromptSubmissionSource>,
) -> Result<(), String> {
    if submission.is_some() && bytes.last() != Some(&b'\r') {
        return Err("prompt submission metadata requires a trailing Enter".to_owned());
    }

    // The tracker below decides whether these bytes actually completed a
    // semantic line. Looking for CR/LF here would misclassify newlines inside
    // a bracketed paste as submissions.
    let mut submission_correlation_id = None;

    // One cheap tree read supplies both title-tracking eligibility and the
    // currently detected provider. Session discovery can temporarily have no
    // accepted ID, but `/clear` must still honor a verified Claude/Codex pane.
    let (is_automatic_plain_shell, detected_agent_class) = {
        let tree = state.tree.read().await;
        tree.get(pane_id).map_or((false, None), |node| {
            let NodeKind::Pane {
                status,
                title_source,
                ..
            } = &node.kind
            else {
                return (false, None);
            };
            let agent_class = match status {
                PaneStatus::Agent(class, _) | PaneStatus::AgentWithGoal(class, _) => {
                    Some(class.clone())
                }
                PaneStatus::PlainShell | PaneStatus::Editor { .. } | PaneStatus::Board => None,
            };
            (
                matches!(status, PaneStatus::PlainShell)
                    && *title_source == PaneTitleSource::Automatic,
                agent_class,
            )
        })
    };

    // Write lock (not read) on `panes`: a `KeyInput` always targets the
    // client's currently-focused pane (the client only ever forwards raw
    // keys for `self.focused_pane`), which `ClientRequest::SetPaneFocus`
    // already puts on the focused fast tier regardless of status -- so most
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
    let mut cleared_conversation_title_generation = None;
    let mut detection_was_forced = false;
    let mut tracked_submission = None;
    let mut goal_was_cleared = false;
    let mut session_transition_observation = None;
    let mut conversation_title_generation_before = None;
    let error_message = match panes.get_mut(&pane_id) {
        Some(PaneResource::Terminal(runtime)) => {
            if !bytes.is_empty()
                && !matches!(submission, Some(PromptSubmissionSource::InitialAgentPrompt))
            {
                runtime.cancel_initial_prompt_delivery();
            }
            // Always false on Windows: ConPTY has no foreground process group,
            // so `foreground_process_group_id` reports nothing and shell
            // command titles stay inactive there. That is a missing capability
            // rather than a bug -- inferring a title without knowing whether a
            // command owns the terminal would retitle panes from keystrokes
            // typed into a running program.
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
                let submitted_input = runtime.session_command_tracker.observe_submission(bytes);
                let did_submit_line = submitted_input.is_some();
                if did_submit_line {
                    // One identifier follows the tracker-confirmed line
                    // through prompt observation, command-driven invalidation,
                    // and the later verified replacement identity.
                    submission_correlation_id = Some(uuid::Uuid::new_v4().to_string());
                }
                let submitted_line = submitted_input
                    .as_ref()
                    .and_then(|submission| submission.exact_text().map(str::to_owned));
                tracked_submission = submitted_input;
                if submitted_line
                    .as_deref()
                    .is_some_and(crate::pane::clears_agent_goal)
                {
                    // The successful PTY write is authoritative user intent.
                    // Clear retained ownership immediately so a footer-hidden
                    // `/goal clear` cannot leave a sticky sidebar flag.
                    runtime.confirmed_goal_owner = None;
                    goal_was_cleared = true;
                }
                let active_agent_class = runtime
                    .session_agent_class
                    .clone()
                    .or_else(|| detected_agent_class.clone());
                let session_transition_rule =
                    submitted_line.as_deref().and_then(|submitted_line| {
                        crate::pane::agent_session_identity_transition_rule(
                            active_agent_class.as_ref(),
                            submitted_line,
                        )
                    });
                let session_identity_invalidated = session_transition_rule.is_some();
                let conversation_cleared = submitted_line
                    .as_deref()
                    .is_some_and(crate::pane::clears_agent_conversation)
                    && matches!(
                        session_transition_rule,
                        Some(SessionIdentityTransitionRule::ClaudeOrCodexClearStartsFreshSession)
                    );
                if let Some(rule) = session_transition_rule {
                    let previous_title_generation = runtime.title_generation;
                    let previous_session_id = runtime.session_id.clone();
                    let previous_agent_class = active_agent_class.clone();
                    let previous_process_id = runtime.session_process_id;
                    runtime.is_session_identity_invalidated = true;
                    runtime.pending_generated_session_id = None;
                    runtime.title_generation = runtime.title_generation.wrapping_add(1);
                    runtime.pending_session_transition_correlation_id =
                        submission_correlation_id.clone();
                    cleared_session_title_generation = Some(runtime.title_generation);
                    if let Some(invalidated_session_id) = runtime.session_id.take() {
                        runtime.invalidated_session_id = Some(invalidated_session_id);
                        cleared_session_origin_name =
                            Some(runtime.origin.pane_name_without_stale_session().to_string());
                    }
                    runtime.session_agent_class = None;
                    session_transition_observation = Some(SessionTransitionObservation {
                        previous_session_id,
                        previous_agent_class,
                        previous_process_id,
                        previous_title_generation,
                        next_title_generation: runtime.title_generation,
                        submitted_command: submitted_line.clone().unwrap_or_default(),
                        rule,
                    });
                }
                if conversation_cleared {
                    // `conversation_cleared` only becomes true when
                    // `session_transition_rule` matched
                    // `ClaudeOrCodexClearStartsFreshSession` above, so
                    // `session_identity_invalidated` is always true here too
                    // and that branch above has already bumped
                    // `title_generation` for this same submitted line. Both
                    // transitions share that one generation bump so stale
                    // title workers have one atomic fence; "before" is simply
                    // that bump's predecessor.
                    debug_assert!(session_identity_invalidated);
                    conversation_title_generation_before =
                        Some(runtime.title_generation.wrapping_sub(1));
                    runtime.is_showing_fresh_agent_screen = true;
                    cleared_conversation_title_generation = Some(runtime.title_generation);
                }
                if did_submit_line {
                    detection_was_forced = crate::detection::force_check(
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

    if detection_was_forced {
        state.detection_schedule_changed.notify_one();
    }

    if let Some(message) = error_message {
        return Err(message);
    }

    // Fresh terminal input also acknowledges a completed turn. This
    // conditional tree transition cannot overwrite a concurrent detector's
    // newer Working/Waiting state.
    if !bytes.is_empty() {
        let _ = record_node_activity(state, pane_id).await?;
        let acknowledged_status = {
            let mut tree = state.tree.write().await;
            match tree.acknowledge_agent_completion(pane_id) {
                Ok(status) => status,
                Err(error) => {
                    tracing::error!(
                        "agent completion acknowledgement rejected for pane {pane_id:?}: {error}"
                    );
                    None
                }
            }
        };
        if let Some(status) = acknowledged_status {
            state.broadcast(ServerEvent::PaneStatusChanged { pane_id, status });
        }
    }

    if submission.is_some() || tracked_submission.is_some() {
        let (text, exactness, opaque_reason) = match tracked_submission.as_ref() {
            Some(submission) if submission.was_truncated => (
                submission.text.clone().unwrap_or_default(),
                "truncated",
                "input exceeded the 4096-character reconstruction limit",
            ),
            Some(submission) if submission.opaque_reason.is_some() => (
                "Input unavailable because terminal editing state was opaque".to_string(),
                "unavailable",
                submission
                    .opaque_reason
                    .map_or("unknown terminal editing state", |reason| {
                        reason.explanation()
                    }),
            ),
            Some(submission) => (
                submission.text.clone().unwrap_or_default(),
                "exact",
                "none; the submitted line was reconstructed exactly",
            ),
            None => (
                "Input unavailable because no semantic line was reconstructed".to_string(),
                "unavailable",
                "no submission boundary was reconstructed from the written bytes",
            ),
        };
        let lifecycle_decision = if let Some(transition) = &session_transition_observation {
            transition.rule.explanation()
        } else if exactness == "exact" {
            "the exact input matched no persisted-session invalidation rule"
        } else {
            "session transition classification was skipped because the submitted line was not exact"
        };
        let _ = crate::agent_debug::record(
            state,
            pane_id,
            AgentDebugSource::Pty,
            AgentDebugEventDraft::information(
                AgentDebugEventKind::PromptSubmitted,
                "Prompt accepted by the agent PTY",
            )
            .with_fields(vec![
                AgentDebugField::plain(
                    "source",
                    submission.map_or_else(
                        || "raw PTY input without semantic source metadata".to_string(),
                        |source| format!("{source:?}"),
                    ),
                ),
                AgentDebugField::plain("exactness", exactness),
                AgentDebugField::plain("reconstruction evidence", opaque_reason),
                AgentDebugField::plain("session lifecycle decision", lifecycle_decision),
                AgentDebugField::plain(
                    "forced immediate detection",
                    detection_was_forced.to_string(),
                ),
                AgentDebugField::plain("written bytes", bytes.len().to_string()),
                AgentDebugField::sensitive("submitted input", text),
            ])
            .with_correlation_id(submission_correlation_id.clone()),
        )
        .await;
    }

    if goal_was_cleared {
        let _ = crate::agent_debug::record(
            state,
            pane_id,
            AgentDebugSource::Pty,
            AgentDebugEventDraft::information(
                AgentDebugEventKind::GoalCleared,
                "Persistent agent goal cleared by user input",
            )
            .with_correlation_id(submission_correlation_id.clone()),
        )
        .await;
    }

    if let Some(title_generation) = cleared_session_title_generation {
        state.request_snapshot_save();
        state.broadcast(ServerEvent::PaneSessionIdCleared {
            pane_id,
            title_generation,
        });
        if let Some(transition) = session_transition_observation.as_ref() {
            let _ = crate::agent_debug::record(
                state,
                pane_id,
                AgentDebugSource::SessionDiscovery,
                AgentDebugEventDraft::information(
                    AgentDebugEventKind::SessionCleared,
                    "Agent session identity invalidated by submitted command",
                )
                .with_fields(vec![
                    AgentDebugField::plain(
                        "provider before invalidation",
                        transition
                            .previous_agent_class
                            .as_ref()
                            .map_or("unverified".to_string(), |class| class.label().to_string()),
                    ),
                    AgentDebugField::plain(
                        "agent process before invalidation",
                        transition.previous_process_id.map_or(
                            "unavailable".to_string(),
                            |process_id| process_id.to_string(),
                        ),
                    ),
                    AgentDebugField::sensitive(
                        "session ID before invalidation",
                        transition
                            .previous_session_id
                            .clone()
                            .unwrap_or_else(|| "none".to_string()),
                    ),
                    AgentDebugField::sensitive(
                        "submitted transition command",
                        transition.submitted_command.clone(),
                    ),
                    AgentDebugField::plain("invalidation rule", transition.rule.explanation()),
                    AgentDebugField::plain(
                        "title generation before",
                        transition.previous_title_generation.to_string(),
                    ),
                    AgentDebugField::plain(
                        "title generation after",
                        transition.next_title_generation.to_string(),
                    ),
                    AgentDebugField::plain(
                        "next session check",
                        "the old ID is quarantined; only a different project-verified identity from the exact agent process can be accepted",
                    ),
                ])
                .with_correlation_id(submission_correlation_id.clone()),
            )
            .await;
        } else {
            tracing::error!(
                pane_id = pane_id.0,
                title_generation,
                "session identity cleared without captured transition evidence"
            );
        }
    }
    if let Some(title_generation) = cleared_conversation_title_generation {
        state.broadcast(ServerEvent::PaneSessionTitleCleared {
            pane_id,
            title_generation,
        });
        let _ = crate::agent_debug::record(
            state,
            pane_id,
            AgentDebugSource::Pty,
            AgentDebugEventDraft::information(
                AgentDebugEventKind::ConversationCleared,
                "Agent conversation cleared",
            )
            .with_fields(vec![
                AgentDebugField::plain(
                    "title generation before",
                    conversation_title_generation_before
                        .unwrap_or_else(|| title_generation.wrapping_sub(1))
                        .to_string(),
                ),
                AgentDebugField::plain("title generation after", title_generation.to_string()),
                AgentDebugField::plain(
                    "persisted session identity",
                    if session_transition_observation.is_some() {
                        "invalidated by the provider rule recorded in the correlated session event"
                    } else {
                        "retained; this provider's /clear resets only visible conversation state"
                    },
                ),
            ])
            .with_correlation_id(submission_correlation_id),
        )
        .await;
    }

    // Identity/generation invalidations must reach clients before the prompt
    // trigger they fence. Otherwise that trigger queues debug and inference
    // work against the old generation and creates deterministic stale noise.
    if let Some(source) = submission {
        state.broadcast(ServerEvent::PanePromptSubmitted { pane_id, source });
    }

    // A session-transition reset takes precedence over a shell title from
    // the same byte batch. In practice they are mutually exclusive, but the
    // ordering makes the stale LLM title impossible to retain if input and
    // foreground detection race.
    let tree_changed = {
        let mut tree = state.tree.write().await;
        if cleared_conversation_title_generation.is_some() {
            match tree
                .reset_terminal_pane_for_fresh_conversation(pane_id, crate::pane::FRESH_AGENT_TITLE)
            {
                Ok(changed) => changed,
                Err(error) => {
                    tracing::error!(
                        "fresh-agent title and placement reset rejected for pane {pane_id:?}: {error}"
                    );
                    false
                }
            }
        } else if let Some(title) = cleared_session_origin_name.or(observed_title) {
            // A typed shell command or non-clear session transition has no
            // distinct short form, so remove any stale inferred alternative.
            match tree.set_automatic_pane_title(pane_id, title, None, None) {
                Ok(changed) => changed,
                Err(error) => {
                    tracing::error!(
                        "automatic title update rejected for pane {pane_id:?}: {error}"
                    );
                    false
                }
            }
        } else {
            false
        }
    };
    if tree_changed {
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
    } else if let Err(error) = record_node_activity(state, pane_id).await {
        send_direct_error(direct_tx, error).await;
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
    state.agent_debug.clear().await;

    state.broadcast(ServerEvent::TreeSnapshot(snapshot));

    // A cleanly-killed session has nothing left worth recovering. Set
    // `session_killed` first: `crate::run`'s shutdown grace period keeps
    // other attached connections alive for a short window after this
    // returns, and without this flag one of them could still call
    // `request_snapshot_save` (e.g. via `NewPane`) and resurrect a
    // snapshot for a session already decided to be unrecoverable -- see
    // `ServerState::session_killed`'s doc comment. Then clear the dirty
    // flag so the background debounced writer
    // (`crate::persistence::spawn_snapshot_writer`) never recreates the
    // file after we remove it below just because an earlier mutation's
    // dirty flag was still set; then take the same write lock
    // `persistence::save_snapshot` holds for a save's entire
    // build+serialize+write+rename, so a write already in flight when
    // this handler runs is guaranteed to finish (writing the *old*
    // snapshot) before we remove the file, rather than racing it.
    state
        .session_killed
        .store(true, std::sync::atomic::Ordering::Release);
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

    /// A command that reads standard input and stays alive, spelled per platform.
    ///
    /// These fixtures need a pane whose process keeps running until the test kills
    /// it. `cat` is the obvious choice on Unix and does not exist on Windows, where
    /// a snapshot naming it simply fails to respawn and the restored tree no longer
    /// matches what was saved.
    fn long_running_pane_command() -> String {
        if cfg!(windows) { "findstr ^" } else { "cat" }.to_string()
    }
    use super::*;
    use crate::initial_prompt::initial_input_bytes;
    use ilium_core::{NodeId, RestructureNode, SplitOrientation};
    use std::time::Duration;

    /// Waits for the command-backed test pane's reader thread to journal at
    /// least one chunk without relying on scheduler timing.
    /// Waits until a pane's output sequence stops advancing.
    ///
    /// [`wait_for_output_sequence`] returns at the *first* byte, which is
    /// enough when a test only needs some output to exist. It is not enough
    /// when a test snapshots what each pane has delivered and then asserts
    /// which panes are behind: a shell that emits its prompt in more than one
    /// chunk keeps advancing after the snapshot, so a pane that was current
    /// becomes legitimately stale and the assertion sees an extra event.
    async fn wait_for_settled_output_sequence(state: &ServerState, pane_id: NodeId) -> u64 {
        /// Consecutive quiet polls before output counts as settled.
        const REQUIRED_STABLE_POLLS: usize = 5;
        const POLL_INTERVAL: Duration = Duration::from_millis(20);

        let mut last_sequence = wait_for_output_sequence(state, pane_id).await;
        let mut stable_polls = 0;
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                tokio::time::sleep(POLL_INTERVAL).await;
                let sequence = {
                    let panes = state.panes.read().await;
                    let Some(PaneResource::Terminal(runtime)) = panes.get(&pane_id) else {
                        panic!("test terminal pane must remain registered");
                    };
                    runtime.session.output_replay().through_sequence
                };
                if sequence == last_sequence {
                    stable_polls += 1;
                    if stable_polls >= REQUIRED_STABLE_POLLS {
                        return sequence;
                    }
                } else {
                    stable_polls = 0;
                    last_sequence = sequence;
                }
            }
        })
        .await
        .expect("test terminal output should stop advancing")
    }

    async fn wait_for_output_sequence(state: &ServerState, pane_id: NodeId) -> u64 {
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let sequence = {
                    let panes = state.panes.read().await;
                    let Some(PaneResource::Terminal(runtime)) = panes.get(&pane_id) else {
                        panic!("test terminal pane must remain registered");
                    };
                    runtime.session.output_replay().through_sequence
                };
                if sequence > 0 {
                    return sequence;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("test terminal should produce output")
    }

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

    #[test]
    fn terminal_replay_event_preserves_the_journal_watermark() {
        let event = terminal_replay_event(
            NodeId(7),
            ilium_pty::PtyOutputReplay {
                through_sequence: 19,
                bytes: b"full terminal state".to_vec(),
                is_complete: true,
            },
        );
        assert_eq!(
            event,
            ServerEvent::TerminalReplay {
                pane_id: NodeId(7),
                through_sequence: 19,
                bytes: b"full terminal state".to_vec(),
                is_complete: true,
            }
        );
    }

    #[tokio::test]
    async fn bookmark_request_broadcasts_the_authoritative_tree_and_marks_it_for_persistence() {
        let directory = tempfile::tempdir().expect("create bookmark test directory");
        let (sound_requests, sound_task) = crate::sounds::spawn(Arc::new(crate::NoopSoundPlayer));
        let state = Arc::new(ServerState::new(crate::state::ServerStateOptions {
            session_name: "bookmark-request".to_string(),
            session_cwd: directory.path().to_path_buf(),
            home_dir: directory.path().to_path_buf(),
            snapshot_path: directory.path().join("bookmark.snapshot.json"),
            detection_config: crate::config::DetectionConfig::default(),
            notifications_config: crate::config::NotificationsConfig::default(),
            sound_settings: ilium_sound::SoundSettings::default(),
            sound_requests,
            custom_signatures: Vec::new(),
            agent_debug_menu_enabled: false,
        }));
        let group_id = {
            let mut tree = state.tree.write().await;
            let project_id = tree
                .project_ids()
                .into_iter()
                .next()
                .expect("fresh server has its launch project");
            tree.add_group(project_id, "work")
                .expect("launch project accepts a group")
        };
        let mut events = state.events.subscribe();
        let (direct_tx, mut direct_rx) = mpsc::channel(1);

        assert!(
            !handle_request(
                &state,
                ClientRequest::SetNodeBookmarked {
                    node_id: group_id,
                    is_bookmarked: true,
                },
                &direct_tx,
            )
            .await
        );

        let ServerEvent::TreeSnapshot(snapshot) = events.recv().await.expect("tree broadcast")
        else {
            panic!("bookmark update must broadcast a tree snapshot");
        };
        assert!(snapshot.get(group_id).unwrap().is_bookmarked);
        assert!(state.tree.read().await.get(group_id).unwrap().is_bookmarked);
        assert!(state
            .snapshot_dirty
            .load(std::sync::atomic::Ordering::Acquire));
        assert!(direct_rx.try_recv().is_err());
        sound_task.abort();
    }

    #[tokio::test]
    async fn focus_acknowledges_only_unread_activity_not_restructure_activity() {
        let directory = tempfile::tempdir().expect("create activity test directory");
        let (sound_requests, sound_task) = crate::sounds::spawn(Arc::new(crate::NoopSoundPlayer));
        let state = Arc::new(ServerState::new(crate::state::ServerStateOptions {
            session_name: "focus-activity".to_string(),
            session_cwd: directory.path().to_path_buf(),
            home_dir: directory.path().to_path_buf(),
            snapshot_path: directory.path().join("focus-activity.snapshot.json"),
            detection_config: crate::config::DetectionConfig::default(),
            notifications_config: crate::config::NotificationsConfig::default(),
            sound_settings: ilium_sound::SoundSettings::default(),
            sound_requests,
            custom_signatures: Vec::new(),
            agent_debug_menu_enabled: false,
        }));
        let (project_id, pane_id) = {
            let mut tree = state.tree.write().await;
            let project_id = tree.project_ids()[0];
            let pane_id = tree
                .add_pane(project_id, "notes", PaneContentKind::Editor)
                .expect("project accepts an editor");
            tree.apply_project_restructure(
                project_id,
                RestructurePlan {
                    children: vec![RestructureNode::Pane {
                        id: pane_id,
                        title: "notes".to_string(),
                        short_title: None,
                        icon: None,
                    }],
                },
            )
            .expect("initial restructure checkpoints project");
            tree.mark_node_focused(pane_id)
                .expect("initial focus checkpoints pane");
            (project_id, pane_id)
        };
        let mut events = state.events.subscribe();

        let activity_revision = record_node_activity(&state, pane_id)
            .await
            .expect("editor activity is accepted");
        assert!(matches!(
            events.recv().await,
            Ok(ServerEvent::NodeActivityChanged {
                node_id,
                activity_revision: received_revision,
            }) if node_id == pane_id && received_revision == activity_revision
        ));
        {
            let tree = state.tree.read().await;
            assert!(tree
                .project_has_unrestructured_activity(project_id)
                .unwrap());
            assert!(tree.get(pane_id).unwrap().has_activity_since_focus());
        }

        handle_set_pane_focus(&state, pane_id, true).await;
        assert!(matches!(
            events.recv().await,
            Ok(ServerEvent::NodeFocusCheckpointChanged {
                node_id,
                activity_revision: received_revision,
            }) if node_id == pane_id && received_revision == activity_revision
        ));
        {
            let tree = state.tree.read().await;
            assert!(
                tree.project_has_unrestructured_activity(project_id)
                    .unwrap(),
                "focus must not checkpoint project restructure activity"
            );
            assert!(!tree.get(pane_id).unwrap().has_activity_since_focus());
        }

        sound_task.abort();
    }

    #[tokio::test]
    async fn accepted_terminal_input_and_output_each_advance_activity() {
        let directory = tempfile::tempdir().expect("create terminal activity directory");
        let (sound_requests, sound_task) = crate::sounds::spawn(Arc::new(crate::NoopSoundPlayer));
        let state = Arc::new(ServerState::new(crate::state::ServerStateOptions {
            session_name: "terminal-activity".to_string(),
            session_cwd: directory.path().to_path_buf(),
            home_dir: directory.path().to_path_buf(),
            snapshot_path: directory.path().join("terminal-activity.snapshot.json"),
            detection_config: crate::config::DetectionConfig::default(),
            notifications_config: crate::config::NotificationsConfig::default(),
            sound_settings: ilium_sound::SoundSettings::default(),
            sound_requests,
            custom_signatures: Vec::new(),
            agent_debug_menu_enabled: false,
        }));
        let pane_id = {
            let mut tree = state.tree.write().await;
            let project_id = tree.project_ids()[0];
            tree.add_pane(project_id, "cat", PaneContentKind::Terminal)
                .expect("project accepts a terminal")
        };
        spawn_and_register_pane(
            &state,
            pane_id,
            PaneSnapshotKind::Terminal(TerminalOrigin::Command(long_running_pane_command())),
        )
        .await
        .expect("spawn terminal activity fixture");
        let mut events = state.events.subscribe();

        write_key_input(&state, pane_id, b"x", None)
            .await
            .expect("write terminal input");
        let output_revision = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                match events.recv().await {
                    Ok(ServerEvent::NodeActivityChanged {
                        node_id,
                        activity_revision,
                    }) if node_id == pane_id && activity_revision >= 2 => {
                        break activity_revision;
                    }
                    Ok(_) => {}
                    Err(error) => panic!("terminal activity event stream closed: {error}"),
                }
            }
        })
        .await
        .expect("PTY echo should produce output activity");
        assert!(
            output_revision >= 2,
            "one accepted input and its PTY output must create distinct revisions"
        );
        assert_eq!(
            state
                .tree
                .read()
                .await
                .get(pane_id)
                .unwrap()
                .activity_revision,
            output_revision
        );

        let resources: Vec<_> = state.panes.write().await.drain().collect();
        for (resource_pane_id, resource) in resources {
            teardown_pane_resource(resource_pane_id, resource);
        }
        sound_task.abort();
    }

    #[tokio::test]
    async fn live_recovery_emits_only_the_missing_pane_tail() {
        let directory = tempfile::tempdir().expect("create recovery test directory");
        let (sound_requests, sound_task) = crate::sounds::spawn(Arc::new(crate::NoopSoundPlayer));
        let state = Arc::new(ServerState::new(crate::state::ServerStateOptions {
            session_name: "pane-scoped-recovery".to_string(),
            session_cwd: directory.path().to_path_buf(),
            home_dir: directory.path().to_path_buf(),
            snapshot_path: directory.path().join("pane-scoped-recovery.snapshot.json"),
            detection_config: crate::config::DetectionConfig::default(),
            notifications_config: crate::config::NotificationsConfig::default(),
            sound_settings: ilium_sound::SoundSettings::default(),
            sound_requests,
            custom_signatures: Vec::new(),
            agent_debug_menu_enabled: false,
        }));
        let (missing_pane_id, current_pane_id) = {
            let mut tree = state.tree.write().await;
            let project_id = tree
                .project_ids()
                .into_iter()
                .next()
                .expect("fresh state has one launch project");
            let group_id = tree
                .add_group(project_id, "recovery")
                .expect("launch project accepts a group");
            let missing_pane_id = tree
                .add_pane(group_id, "missing", PaneContentKind::Terminal)
                .expect("group accepts first terminal");
            let current_pane_id = tree
                .add_pane(group_id, "current", PaneContentKind::Terminal)
                .expect("group accepts second terminal");
            (missing_pane_id, current_pane_id)
        };
        for (pane_id, marker) in [
            (missing_pane_id, "missing-pane-marker"),
            (current_pane_id, "current-pane-marker"),
        ] {
            spawn_and_register_pane(
                &state,
                pane_id,
                PaneSnapshotKind::Terminal(TerminalOrigin::Command(format!(
                    "printf '{marker}\\n'"
                ))),
            )
            .await
            .expect("register command-backed test terminal");
        }

        // Both panes must be quiet before their sequences are snapshotted, or
        // the "current" pane can advance afterwards and legitimately need a
        // tail of its own, which this test would read as a second event.
        let missing_sequence = wait_for_settled_output_sequence(&state, missing_pane_id).await;
        let current_sequence = wait_for_settled_output_sequence(&state, current_pane_id).await;
        let delivered_sequences = HashMap::from([
            (missing_pane_id, missing_sequence.saturating_sub(1)),
            (current_pane_id, current_sequence),
        ]);

        let events = resynchronization_events(&state, &delivered_sequences).await;
        let terminal_events: Vec<&ServerEvent> = events
            .iter()
            .filter(|event| {
                matches!(
                    event,
                    ServerEvent::ScreenUpdate { .. } | ServerEvent::TerminalReplay { .. }
                )
            })
            .collect();

        assert_eq!(terminal_events.len(), 1);
        assert!(matches!(
            terminal_events[0],
            ServerEvent::ScreenUpdate {
                pane_id,
                first_sequence,
                sequence,
                bytes,
            } if *pane_id == missing_pane_id
                && *first_sequence == missing_sequence
                && *sequence == missing_sequence
                && !bytes.is_empty()
        ));
        assert!(!events.contains(&ServerEvent::InitialStateSyncComplete));

        let resources: Vec<_> = state.panes.write().await.drain().collect();
        for (pane_id, resource) in resources {
            teardown_pane_resource(pane_id, resource);
        }
        sound_task.abort();
    }

    #[tokio::test]
    async fn project_restructure_handler_rebases_activity_and_preserves_splits() {
        let directory = tempfile::tempdir().expect("create restructure test directory");
        let (sound_requests, sound_task) = crate::sounds::spawn(Arc::new(crate::NoopSoundPlayer));
        let state = Arc::new(ServerState::new(crate::state::ServerStateOptions {
            session_name: "protected-split-restructure".to_string(),
            session_cwd: directory.path().to_path_buf(),
            home_dir: directory.path().to_path_buf(),
            snapshot_path: directory.path().join("protected-split.snapshot.json"),
            detection_config: crate::config::DetectionConfig::default(),
            notifications_config: crate::config::NotificationsConfig::default(),
            sound_settings: ilium_sound::SoundSettings::default(),
            sound_requests,
            custom_signatures: Vec::new(),
            agent_debug_menu_enabled: false,
        }));
        let (project_id, original_group, split_view, first, second) = {
            let mut tree = state.tree.write().await;
            let project_id = tree.project_ids()[0];
            let original_group = tree
                .add_group(project_id, "original")
                .expect("launch project accepts a group");
            let first = tree
                .add_pane(original_group, "first", PaneContentKind::Terminal)
                .expect("group accepts first terminal");
            let second = tree
                .add_pane(original_group, "second", PaneContentKind::Terminal)
                .expect("group accepts second terminal");
            let split_view = tree
                .create_split_view(
                    original_group,
                    "User Split",
                    SplitOrientation::Horizontal,
                    &[first, second],
                )
                .expect("group accepts a split");
            (project_id, original_group, split_view, first, second)
        };
        let inference_tree = state.tree.read().await.clone();
        let inference_activity_revisions = inference_tree
            .project_activity_revisions(project_id)
            .expect("project revisions are readable");
        state
            .tree
            .write()
            .await
            .record_node_activity(first)
            .expect("terminal activity is recorded while inference runs");
        let before_apply = state.tree.read().await.clone();
        let mut events = state.events.subscribe();
        let (direct_tx, mut direct_rx) = mpsc::channel(8);

        handle_apply_project_restructure_plan(
            &state,
            project_id,
            RestructurePlan {
                children: vec![RestructureNode::Group {
                    title: "regrouped".to_string(),
                    short_title: None,
                    icon: None,
                    children: vec![RestructureNode::ExistingSplitView {
                        id: split_view,
                        children: vec![
                            RestructureNode::Pane {
                                id: first,
                                title: "renamed first".to_string(),
                                short_title: None,
                                icon: None,
                            },
                            RestructureNode::Pane {
                                id: second,
                                title: "renamed second".to_string(),
                                short_title: None,
                                icon: None,
                            },
                        ],
                    }],
                }],
            },
            &inference_activity_revisions,
            &direct_tx,
        )
        .await;

        let Some(ServerEvent::ProjectRestructureApplied {
            project_id: applied_project_id,
            checkpoint_activity_revisions,
        }) = direct_rx.recv().await
        else {
            panic!("activity during inference must not reject a valid restructure");
        };
        assert_eq!(applied_project_id, project_id);
        assert_eq!(
            checkpoint_activity_revisions
                .iter()
                .find(|revision| revision.node_id == first)
                .map(|revision| revision.activity_revision),
            Some(inference_tree.get(first).unwrap().activity_revision)
        );
        let snapshot = tokio::time::timeout(Duration::from_secs(1), events.recv())
            .await
            .expect("successful restructure broadcasts")
            .expect("broadcast channel remains open");
        let ServerEvent::TreeSnapshot(snapshot) = snapshot else {
            panic!("successful restructure should broadcast a tree snapshot");
        };
        assert!(snapshot.get(original_group).is_none());
        assert!(snapshot
            .get(split_view)
            .is_some_and(ilium_core::Node::is_split_view));
        assert_eq!(snapshot.children_of(split_view).unwrap(), &[first, second]);
        assert_eq!(
            snapshot.split_orientation(split_view),
            Some(SplitOrientation::Horizontal)
        );
        assert_eq!(
            snapshot.get(first).unwrap().activity_revision,
            before_apply.get(first).unwrap().activity_revision
        );
        assert_eq!(
            snapshot
                .get(first)
                .unwrap()
                .last_restructure_activity_revision,
            Some(inference_tree.get(first).unwrap().activity_revision)
        );
        assert!(snapshot
            .project_has_unrestructured_activity(project_id)
            .unwrap());
        assert_eq!(
            state.restructure_undo.lock().await.get(&project_id),
            Some(&before_apply)
        );

        let valid_tree = state.tree.read().await.clone();
        let inference_activity_revisions = valid_tree
            .project_activity_revisions(project_id)
            .expect("project revisions are readable");
        let prior_undo = state
            .restructure_undo
            .lock()
            .await
            .get(&project_id)
            .cloned();
        handle_apply_project_restructure_plan(
            &state,
            project_id,
            RestructurePlan {
                children: vec![
                    RestructureNode::Pane {
                        id: first,
                        title: "dissolved first".to_string(),
                        short_title: None,
                        icon: None,
                    },
                    RestructureNode::Pane {
                        id: second,
                        title: "dissolved second".to_string(),
                        short_title: None,
                        icon: None,
                    },
                ],
            },
            &inference_activity_revisions,
            &direct_tx,
        )
        .await;

        assert!(matches!(
            direct_rx.recv().await,
            Some(ServerEvent::ProjectRestructureRejected {
                project_id: rejected_project_id,
                message,
            })
                if rejected_project_id == project_id
                    && message.contains("changed the protected split-view set")
        ));
        assert_eq!(*state.tree.read().await, valid_tree);
        assert_eq!(
            state.restructure_undo.lock().await.get(&project_id),
            prior_undo.as_ref()
        );
        assert!(events.try_recv().is_err());

        sound_task.abort();
    }

    /// Regression test for the leak fixed in `handle_revert_project_restructure`:
    /// a pane created after a restructure (so it is absent from the tree the
    /// revert restores) must have its `AgentDebugRecorder` journal evicted,
    /// not just its tree node and `PaneResource` -- otherwise the orphaned
    /// journal keeps being written into every future session snapshot.
    #[tokio::test]
    async fn reverting_a_restructure_evicts_the_orphaned_panes_agent_debug_journal() {
        let directory = tempfile::tempdir().expect("create revert test directory");
        let (sound_requests, sound_task) = crate::sounds::spawn(Arc::new(crate::NoopSoundPlayer));
        let state = Arc::new(ServerState::new(crate::state::ServerStateOptions {
            session_name: "revert-orphan-debug".to_string(),
            session_cwd: directory.path().to_path_buf(),
            home_dir: directory.path().to_path_buf(),
            snapshot_path: directory.path().join("revert-orphan-debug.snapshot.json"),
            detection_config: crate::config::DetectionConfig::default(),
            notifications_config: crate::config::NotificationsConfig::default(),
            sound_settings: ilium_sound::SoundSettings::default(),
            sound_requests,
            custom_signatures: Vec::new(),
            agent_debug_menu_enabled: true,
        }));

        let project_id = state
            .tree
            .read()
            .await
            .project_ids()
            .into_iter()
            .next()
            .expect("fresh state has one launch project");

        // Snapshot the tree before the soon-to-be-orphaned pane exists --
        // mirrors what `handle_apply_project_restructure_plan` stores in
        // `state.restructure_undo` as the pre-restructure `before` tree.
        let previous_tree = state.tree.read().await.clone();

        let orphaned_pane_id = {
            let mut tree = state.tree.write().await;
            let group_id = tree
                .add_group(project_id, "orphaned-group")
                .expect("launch project accepts a group");
            tree.add_pane(group_id, "orphaned", PaneContentKind::Terminal)
                .expect("group accepts a terminal pane")
        };
        spawn_and_register_pane(
            &state,
            orphaned_pane_id,
            PaneSnapshotKind::Terminal(TerminalOrigin::Command(
                "printf 'orphan-marker\\n'".to_string(),
            )),
        )
        .await
        .expect("register command-backed test terminal");
        state
            .agent_debug
            .append(
                orphaned_pane_id,
                AgentDebugSource::Pty,
                ilium_agent_debug::AgentDebugContext::default(),
                AgentDebugEventDraft::information(
                    AgentDebugEventKind::PromptSubmitted,
                    "orphaned pane journal entry",
                ),
            )
            .await
            .expect("enabled recorder appends");

        state
            .restructure_undo
            .lock()
            .await
            .insert(project_id, previous_tree);

        let (direct_tx, mut direct_rx) = mpsc::channel(8);
        handle_revert_project_restructure(&state, project_id, &direct_tx).await;
        drop(direct_tx);
        assert!(
            direct_rx.recv().await.is_none(),
            "revert of a freshly recorded restructure must not error"
        );

        assert!(state.tree.read().await.get(orphaned_pane_id).is_none());
        assert!(!state.panes.read().await.contains_key(&orphaned_pane_id));
        assert!(
            state
                .agent_debug
                .replay(orphaned_pane_id, None)
                .await
                .is_none(),
            "reverted restructure must evict the orphaned pane's agent-debug journal"
        );

        sound_task.abort();
    }
}

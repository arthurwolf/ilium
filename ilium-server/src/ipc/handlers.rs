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
use ilium_platform::paths;
use ilium_pty::PtyError;
use tokio::sync::mpsc;

use crate::mouse::to_crossterm_event;
use crate::pane;
use crate::pane::{PaneResource, PaneSnapshotKind, TerminalOrigin};
use crate::state::{
    ProgressSetRequestIdentity, ProgressSetRequestOutcome, ProgressSetRequestRecord,
    ProgressSetResult, ServerState, MAXIMUM_CACHED_PROGRESS_SET_REQUESTS,
};

/// Caps activity-revision mutations during one continuous PTY output burst.
struct OutputActivityGate {
    next_record_at: Option<std::time::Instant>,
}

impl OutputActivityGate {
    /// The first chunk after an idle gap records immediately. During a
    /// continuous stream, two revisions per second are sufficient to fence
    /// multi-second restructure inference while avoiding 20 tree-lock and
    /// IPC-broadcast cycles per pane per second.
    const INTERVAL: std::time::Duration = std::time::Duration::from_millis(500);

    fn new() -> Self {
        Self {
            next_record_at: None,
        }
    }

    fn should_record(&mut self, now: std::time::Instant) -> bool {
        if self.next_record_at.is_some_and(|deadline| now < deadline) {
            return false;
        }
        self.next_record_at = Some(now + Self::INTERVAL);
        true
    }
}

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
            handle_attach(state, &session, direct_tx, true).await;
            false
        }
        ClientRequest::AttachInteractive { session } => {
            handle_attach(state, &session, direct_tx, false).await;
            false
        }
        ClientRequest::SetVisiblePanes { .. } => {
            // Per-connection stream selection is intercepted by
            // `ipc::connection` before generic request dispatch. Reaching
            // this fallback is harmless for direct handler tests and future
            // non-streaming transports, but no session-global state exists
            // to mutate here.
            false
        }
        ClientRequest::UpdateTextTriggers { settings } => {
            if let Some(message) = crate::text_triggers::validate_settings(&settings) {
                send_direct_error(direct_tx, message).await;
                return false;
            }
            let mut accepted = state.text_trigger_settings.write().await;
            accepted.settings = settings.clone();
            accepted.revision = accepted.revision.saturating_add(1);
            drop(accepted);
            state.broadcast(ServerEvent::TextTriggersChanged { settings });
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
        ClientRequest::SetNodeExpanded { node_id, expanded } => {
            handle_tree_mutation(state, direct_tx, |tree| {
                tree.set_node_expanded(node_id, expanded)
            })
            .await;
            false
        }
        ClientRequest::SetNodeLockedClosed {
            node_id,
            locked_closed,
        } => {
            handle_tree_mutation(state, direct_tx, |tree| {
                tree.set_node_locked_closed(node_id, locked_closed)
            })
            .await;
            false
        }
        ClientRequest::ReportLastPromptFromTranscript {
            pane_id,
            expected_session_id,
            last_prompt,
        } => {
            handle_last_prompt_from_transcript(state, pane_id, &expected_session_id, last_prompt)
                .await;
            false
        }
        ClientRequest::CheckPaneProgressMonitor {
            request_id,
            pane_id,
            command,
        } => {
            handle_check_pane_progress_monitor(state, request_id, pane_id, &command, direct_tx)
                .await;
            false
        }
        ClientRequest::SetPaneProgressMonitor {
            request_id,
            pane_id,
            command,
            interval_seconds,
        } => {
            handle_set_pane_progress_monitor(
                state,
                request_id,
                pane_id,
                command,
                interval_seconds,
                direct_tx,
            )
            .await;
            false
        }
        ClientRequest::GetPaneProgressMonitorStatus {
            request_id,
            pane_id,
        } => {
            handle_get_pane_progress_monitor_status(state, request_id, pane_id, direct_tx).await;
            false
        }
        ClientRequest::ClearPaneProgressMonitor {
            request_id,
            pane_id,
            expected_monitor_id,
        } => {
            handle_clear_pane_progress_monitor(
                state,
                request_id,
                pane_id,
                expected_monitor_id,
                direct_tx,
            )
            .await;
            false
        }
        ClientRequest::UpdateProgressMonitorEnabled { enabled } => {
            handle_update_progress_monitor_enabled(state, enabled).await;
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
        ClientRequest::SubmitTerminalText {
            pane_id,
            text,
            source,
        } => {
            if let Err(message) = submit_terminal_text(state, pane_id, &text, source).await {
                send_direct_error(direct_tx, message).await;
            }
            false
        }
        ClientRequest::GetPaneGoalStatus {
            request_id,
            pane_id,
        } => {
            let event = crate::goal_control::goal_status_event(state, request_id, pane_id).await;
            send_direct(direct_tx, event).await;
            false
        }
        ClientRequest::RequestPaneGoalResume {
            request_id,
            pane_id,
        } => {
            let event = crate::goal_control::request_resume_event(state, request_id, pane_id).await;
            send_direct(direct_tx, event).await;
            false
        }
        ClientRequest::RegisterVoiceTextReceiver => {
            state.voice_text.register_receiver(direct_tx.clone());
            false
        }
        ClientRequest::SubmitVoiceText {
            request_id,
            sentences,
            start_voice,
        } => {
            tracing::info!(
                request_id,
                sentence_count = sentences.len(),
                start_voice,
                "voice text request received"
            );
            let result = state
                .voice_text
                .submit(request_id, sentences, start_voice)
                .await;
            send_direct(
                direct_tx,
                ServerEvent::VoiceTextResult { request_id, result },
            )
            .await;
            false
        }
        ClientRequest::AnswerVoiceText { request_id, result } => {
            state.voice_text.answer(request_id, result);
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
                            status: PaneStatus::Agent(_, _) | PaneStatus::AgentWithGoal(_, _, _),
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

/// Advances output activity without broadcasting revisions a hidden pane's
/// clients cannot render. The first unread/unrestructured edge is always
/// published; a later visible-pane subscription explicitly synchronizes the
/// newest revision before recovering terminal bytes.
async fn record_terminal_output_activity(
    state: &ServerState,
    node_id: NodeId,
) -> Result<u64, String> {
    let update = {
        let mut tree = state.tree.write().await;
        tree.record_node_activity(node_id)
            .map_err(|error| format!("could not record activity for {node_id:?}: {error}"))?
    };
    if state.has_terminal_subscribers(node_id)
        || update.became_unrestructured
        || update.became_unread_since_focus
    {
        publish_node_activity_update(state, node_id, update);
    }
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

async fn handle_attach(
    state: &ServerState,
    session: &str,
    direct_tx: &mpsc::Sender<ServerEvent>,
    include_terminal_output: bool,
) {
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

    send_initial_state(state, direct_tx, include_terminal_output).await;
}

/// Sends one ordered, complete client render-cache seed. Both normal attach
/// and post-recovery resolution use this exact path so the startup trigger
/// always observes the same state boundary.
async fn send_initial_state(
    state: &ServerState,
    direct_tx: &mpsc::Sender<ServerEvent>,
    include_terminal_output: bool,
) {
    for event in initial_state_events(state, true, include_terminal_output).await {
        send_direct(direct_tx, event).await;
    }
}

/// Selects how terminal journals synchronize for one connection. Attach has
/// no prior parser and needs full replays; live recovery appends exact missing
/// deltas while retained, falling back to replay only past the journal window.
enum TerminalOutputSynchronization<'a> {
    All,
    None,
    RecoverAfter(&'a HashMap<NodeId, u64>),
}

impl TerminalOutputSynchronization<'_> {
    /// Builds the smallest event that makes one terminal parser current.
    fn event_for(&self, pane_id: NodeId, session: &ilium_pty::PtySession) -> Option<ServerEvent> {
        match self {
            Self::All => Some(terminal_replay_event(pane_id, session.output_replay())),
            Self::None => None,
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
                            bytes: chunk.bytes.to_vec(),
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
    include_terminal_output: bool,
) -> Vec<ServerEvent> {
    state_synchronization_events(
        state,
        if include_terminal_output {
            TerminalOutputSynchronization::All
        } else {
            TerminalOutputSynchronization::None
        },
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
    // Every real caller reaches this only through `AttachInteractive` (the
    // TUI never issues the legacy `Attach`), which promises metadata-only
    // startup with terminal bytes recovered lazily once the client names its
    // visible panes via `SetVisiblePanes`. Passing `true` here would eagerly
    // clone and queue a full multi-megabyte-per-pane replay on every
    // crash-recovery resolution -- work `ipc::connection`'s writer then
    // silently discards for this exact connection because its terminal
    // stream selection is still `None` at this point (see
    // `should_forward_terminal_event`). `false` matches the same
    // `AttachInteractive` contract `handle_attach` already applies before a
    // recovery decision is pending.
    send_initial_state(state, direct_tx, false).await;
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
    // `paths::canonicalize`, not `std::fs::canonicalize`: this becomes the new
    // project node's stored path and a future pane's spawn directory, so a raw
    // Windows extended-length prefix would both show up in the tree and break
    // `cmd.exe`, which rejects it as a working directory.
    let canonical = paths::canonicalize(&path)
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

/// Applies the last-user-message a background client worker found in the
/// agent CLI's own session transcript (see `ilium-client`'s
/// `transcript_context::recent_user_prompts`). Preferred over live keystroke
/// reconstruction when the two disagree: the transcript is the agent's own
/// authoritative record, so it stays correct even for a submission live
/// tracking could not reconstruct exactly (shell history recall, an
/// unsupported escape sequence, ...). Discarded, same as a stale title
/// result, when the pane's session has since changed or been invalidated --
/// `expected_session_id` was captured before the worker's (possibly slow)
/// transcript read, so the pane may already be on a different session by
/// the time this arrives.
async fn handle_last_prompt_from_transcript(
    state: &Arc<ServerState>,
    pane_id: NodeId,
    expected_session_id: &str,
    last_prompt: String,
) {
    let panes = state.panes.read().await;
    let Some(PaneResource::Terminal(runtime)) = panes.get(&pane_id) else {
        return;
    };
    if runtime.is_session_identity_invalidated
        || runtime.session_id.as_deref() != Some(expected_session_id)
    {
        drop(panes);
        let _ = crate::agent_debug::record(
            state,
            pane_id,
            AgentDebugSource::Inference,
            AgentDebugEventDraft {
                severity: AgentDebugSeverity::Warning,
                kind: AgentDebugEventKind::Custom("last_prompt_transcript_discarded".to_string()),
                summary: "Stale transcript-sourced last prompt rejected by the server".to_string(),
                fields: vec![AgentDebugField::plain(
                    "expected session",
                    expected_session_id.to_string(),
                )],
                correlation_id: None,
                metadata: Default::default(),
            },
        )
        .await;
        return;
    }
    drop(panes);
    let updated = {
        let mut tree = state.tree.write().await;
        tree.set_last_prompt(pane_id, Some(last_prompt.clone()))
            .is_ok()
    };
    if updated {
        state.request_snapshot_save();
        state.broadcast(ServerEvent::PaneLastPromptChanged {
            pane_id,
            last_prompt: Some(last_prompt),
        });
    }
}

/// Starts (or replaces) `pane_id`'s server-run progress monitor -- see
/// `crate::progress_monitor`'s module doc for the full loop contract. Any
/// connection may send this (a bare CLI connection has the same authority as
/// the attached TUI -- see `ipc::connection`), so the server's own live
/// `ServerState::is_progress_monitor_enabled` setting is the only gate.
async fn handle_check_pane_progress_monitor(
    state: &Arc<ServerState>,
    request_id: u64,
    pane_id: NodeId,
    command: &str,
    direct_tx: &mpsc::Sender<ServerEvent>,
) {
    let result = if !state.is_progress_monitor_enabled() {
        Err(progress_rejection(
            ilium_ipc::ProgressMonitorRejectionCode::Disabled,
            "progress monitor is disabled by server settings",
        ))
    } else if !matches!(
        state.panes.read().await.get(&pane_id),
        Some(PaneResource::Terminal(_))
    ) {
        Err(progress_rejection(
            ilium_ipc::ProgressMonitorRejectionCode::PaneNotFound,
            format!("pane {pane_id:?} is not a live terminal pane"),
        ))
    } else {
        crate::progress_monitor::preflight(command)
            .await
            .map_err(|error| error.rejection())
    };
    send_direct(
        direct_tx,
        ServerEvent::ProgressMonitorCheckCompleted {
            request_id,
            pane_id,
            result,
        },
    )
    .await;
}

fn progress_rejection(
    code: ilium_ipc::ProgressMonitorRejectionCode,
    message: impl Into<String>,
) -> ilium_ipc::ProgressMonitorRejection {
    ilium_ipc::ProgressMonitorRejection {
        code,
        message: message.into(),
    }
}

async fn handle_set_pane_progress_monitor(
    state: &Arc<ServerState>,
    request_id: u64,
    pane_id: NodeId,
    command: String,
    interval_seconds: u32,
    direct_tx: &mpsc::Sender<ServerEvent>,
) {
    let identity = ProgressSetRequestIdentity {
        pane_id,
        command,
        interval_seconds,
    };
    let result = idempotent_install_progress_monitor(state, request_id, identity).await;
    send_direct(
        direct_tx,
        ServerEvent::ProgressMonitorSetCompleted {
            request_id,
            pane_id,
            result,
        },
    )
    .await;
}

/// Applies session-scoped idempotency to progress registration. The CLI uses
/// high-entropy request IDs and may reconnect after losing its acknowledgement,
/// so connection-local replay state would be insufficient. Reusing an ID with
/// different arguments is rejected as a collision; exact concurrent retries
/// wait for and reuse the leader's result without rerunning preflight.
async fn idempotent_install_progress_monitor(
    state: &Arc<ServerState>,
    request_id: u64,
    identity: ProgressSetRequestIdentity,
) -> Result<ilium_ipc::ProgressMonitorAccepted, ilium_ipc::ProgressMonitorRejection> {
    enum Decision {
        Lead,
        Wait(tokio::sync::watch::Receiver<Option<ProgressSetResult>>),
        Return(ProgressSetResult),
    }

    loop {
        let decision = {
            let mut cache = state.progress_set_requests.lock().await;
            match cache.records.get(&request_id) {
                Some(record) if record.identity != identity => Decision::Return(Err(
                    progress_rejection(
                        ilium_ipc::ProgressMonitorRejectionCode::InvalidRequest,
                        format!(
                            "progress set request_id {request_id} was already used with different arguments"
                        ),
                    ),
                )),
                Some(ProgressSetRequestRecord {
                    outcome: ProgressSetRequestOutcome::Pending(completed),
                    ..
                }) => Decision::Wait(completed.subscribe()),
                Some(ProgressSetRequestRecord {
                    outcome: ProgressSetRequestOutcome::Complete(result),
                    ..
                }) => Decision::Return(result.clone()),
                None => {
                    while cache.records.len() >= MAXIMUM_CACHED_PROGRESS_SET_REQUESTS {
                        let Some(expired) = cache.completed_order.pop_front() else {
                            break;
                        };
                        cache.records.remove(&expired);
                    }
                    if cache.records.len() >= MAXIMUM_CACHED_PROGRESS_SET_REQUESTS {
                        Decision::Return(Err(progress_rejection(
                            ilium_ipc::ProgressMonitorRejectionCode::InvalidRequest,
                            "too many progress set requests are currently pending; retry later",
                        )))
                    } else {
                        let (completed, _completion_rx) = tokio::sync::watch::channel(None);
                        cache.records.insert(
                            request_id,
                            ProgressSetRequestRecord {
                                identity: identity.clone(),
                                outcome: ProgressSetRequestOutcome::Pending(completed),
                            },
                        );
                        Decision::Lead
                    }
                }
            }
        };

        match decision {
            Decision::Return(result) => return result,
            Decision::Wait(mut completed) => {
                let _ = completed.changed().await;
            }
            Decision::Lead => break,
        }
    }

    let result = install_progress_monitor(
        state,
        identity.pane_id,
        identity.command.clone(),
        identity.interval_seconds,
    )
    .await;
    let completed = {
        let mut cache = state.progress_set_requests.lock().await;
        let completed = match cache.records.get_mut(&request_id) {
            Some(record) => {
                let ProgressSetRequestOutcome::Pending(completed) = &record.outcome else {
                    unreachable!("the progress set leader owns a pending cache entry")
                };
                let completed = completed.clone();
                record.outcome = ProgressSetRequestOutcome::Complete(result.clone());
                completed
            }
            None => unreachable!("the progress set leader's cache entry must remain present"),
        };
        cache.completed_order.push_back(request_id);
        while cache.completed_order.len() > MAXIMUM_CACHED_PROGRESS_SET_REQUESTS {
            if let Some(expired) = cache.completed_order.pop_front() {
                cache.records.remove(&expired);
            }
        }
        completed
    };
    completed.send_replace(Some(result.clone()));
    result
}

async fn install_progress_monitor(
    state: &Arc<ServerState>,
    pane_id: NodeId,
    command: String,
    interval_seconds: u32,
) -> Result<ilium_ipc::ProgressMonitorAccepted, ilium_ipc::ProgressMonitorRejection> {
    if !state.is_progress_monitor_enabled() {
        return Err(progress_rejection(
            ilium_ipc::ProgressMonitorRejectionCode::Disabled,
            "progress monitor is disabled by server settings",
        ));
    }
    let effect_gate = {
        let panes = state.panes.read().await;
        match panes.get(&pane_id) {
            Some(PaneResource::Terminal(runtime)) => Arc::clone(&runtime.progress_effect_gate),
            _ => {
                return Err(progress_rejection(
                    ilium_ipc::ProgressMonitorRejectionCode::PaneNotFound,
                    format!("pane {pane_id:?} is not a live terminal pane"),
                ));
            }
        }
    };
    let interval = std::time::Duration::from_secs(u64::from(interval_seconds));
    if !(crate::progress_monitor::MIN_INTERVAL..=crate::progress_monitor::MAX_INTERVAL)
        .contains(&interval)
    {
        return Err(progress_rejection(
            ilium_ipc::ProgressMonitorRejectionCode::InvalidRequest,
            format!(
                "progress interval must be between {} and {} seconds",
                crate::progress_monitor::MIN_INTERVAL.as_secs(),
                crate::progress_monitor::MAX_INTERVAL.as_secs()
            ),
        ));
    }
    // Preflight occurs before the effect gate and before replacement: a bad
    // candidate never interrupts the monitor that is already active.
    let preflight = crate::progress_monitor::preflight(&command)
        .await
        .map_err(|error| error.rejection())?;
    let monitor_id = state.allocate_progress_monitor_id();
    let progress = ilium_core::PaneProgress::new(
        monitor_id,
        preflight.report,
        preflight.checked_at_unix_millis,
    )
    .map_err(|error| {
        progress_rejection(
            ilium_ipc::ProgressMonitorRejectionCode::InvalidProbeReport,
            error.to_string(),
        )
    })?;
    let registration = crate::progress_monitor::ProgressMonitorRegistration {
        monitor_id,
        pane_id,
        command,
        interval,
        initial_progress: progress.clone(),
    };
    commit_progress_monitor(state, registration, effect_gate).await?;
    Ok(ilium_ipc::ProgressMonitorAccepted {
        monitor_id,
        progress,
    })
}

async fn commit_progress_monitor(
    state: &Arc<ServerState>,
    registration: crate::progress_monitor::ProgressMonitorRegistration,
    effect_gate: Arc<tokio::sync::Mutex<()>>,
) -> Result<(), ilium_ipc::ProgressMonitorRejection> {
    let pane_id = registration.pane_id;
    let monitor_id = registration.monitor_id;
    let progress = registration.initial_progress.clone();
    let _effect_guard = effect_gate.lock().await;
    {
        let panes = state.panes.read().await;
        let Some(PaneResource::Terminal(runtime)) = panes.get(&pane_id) else {
            return Err(progress_rejection(
                ilium_ipc::ProgressMonitorRejectionCode::PaneNotFound,
                format!("pane {pane_id:?} closed before registration persistence"),
            ));
        };
        if !Arc::ptr_eq(&effect_gate, &runtime.progress_effect_gate) {
            return Err(progress_rejection(
                ilium_ipc::ProgressMonitorRejectionCode::PaneNotFound,
                format!("pane {pane_id:?} changed before registration persistence"),
            ));
        }
    }
    let durable_candidate = crate::persistence::PersistedProgressMonitor {
        pane_id,
        command: registration.command.clone(),
        interval_seconds: registration.interval.as_secs(),
        latest_progress: progress.clone(),
        result_delivery: crate::persistence::PersistedProgressDeliveryState::NotQueued,
    };
    let snapshot_write_guard =
        crate::persistence::await_progress_monitor_durability_barrier(state, &durable_candidate)
            .await
            .map_err(|error| {
                progress_rejection(
                    ilium_ipc::ProgressMonitorRejectionCode::InvalidRequest,
                    format!("could not durably persist progress monitor: {error}"),
                )
            })?;
    let mut tree = state.tree.write().await;
    let mut panes = state.panes.write().await;
    let Some(PaneResource::Terminal(runtime)) = panes.get_mut(&pane_id) else {
        drop(panes);
        drop(tree);
        drop(snapshot_write_guard);
        return Err(repair_rejected_staged_progress_monitor(
            state,
            progress_rejection(
                ilium_ipc::ProgressMonitorRejectionCode::PaneNotFound,
                format!("pane {pane_id:?} closed before registration commit"),
            ),
        )
        .await);
    };
    if !Arc::ptr_eq(&effect_gate, &runtime.progress_effect_gate) {
        drop(panes);
        drop(tree);
        drop(snapshot_write_guard);
        return Err(repair_rejected_staged_progress_monitor(
            state,
            progress_rejection(
                ilium_ipc::ProgressMonitorRejectionCode::PaneNotFound,
                format!("pane {pane_id:?} changed before registration commit"),
            ),
        )
        .await);
    }
    let fence = match runtime.install_progress_monitor(registration.clone()) {
        Ok(fence) => fence,
        Err(message) => {
            let rejection = progress_rejection(
                ilium_ipc::ProgressMonitorRejectionCode::InvalidRequest,
                message,
            );
            drop(panes);
            drop(tree);
            drop(snapshot_write_guard);
            return Err(repair_rejected_staged_progress_monitor(state, rejection).await);
        }
    };
    if let Err(error) = tree.set_pane_progress(pane_id, Some(progress.clone())) {
        runtime.cancel_progress_monitor();
        let rejection = progress_rejection(
            ilium_ipc::ProgressMonitorRejectionCode::PaneNotFound,
            error.to_string(),
        );
        drop(panes);
        drop(tree);
        drop(snapshot_write_guard);
        return Err(repair_rejected_staged_progress_monitor(state, rejection).await);
    }
    let probe_task = crate::progress_monitor::spawn(Arc::clone(state), registration, fence);
    let outcome_state = Arc::clone(state);
    let outcome_task = tokio::spawn(async move {
        match probe_task.await {
            Ok(outcome) => {
                handle_progress_monitor_outcome(&outcome_state, pane_id, monitor_id, outcome).await
            }
            Err(error) if error.is_cancelled() => {}
            Err(error) => {
                tracing::warn!(pane_id = pane_id.0, monitor_id, %error, "progress coordinator task failed")
            }
        }
    });
    runtime.set_progress_monitor_task(outcome_task);
    drop(panes);
    drop(tree);
    // The live monitor now exactly matches the staged bytes. Releasing this
    // guard lets a background writer proceed, but it can only build from the
    // committed state and therefore cannot overwrite the durable acceptance
    // with the previous monitor.
    drop(snapshot_write_guard);
    state.broadcast(ServerEvent::PaneProgressChanged {
        pane_id,
        progress: Some(progress),
    });
    state.request_snapshot_save();
    Ok(())
}

/// Repairs the small stage-before-commit crash window after a pane or goal
/// changed while its candidate snapshot was being written. The old live
/// monitor was not touched, so a fresh current-state barrier restores disk.
async fn repair_rejected_staged_progress_monitor(
    state: &ServerState,
    mut rejection: ilium_ipc::ProgressMonitorRejection,
) -> ilium_ipc::ProgressMonitorRejection {
    if let Err(error) = crate::persistence::await_snapshot_durability_barrier(state).await {
        rejection.message.push_str(&format!(
            "; additionally failed to repair the staged snapshot: {error}"
        ));
    }
    rejection
}

async fn handle_progress_monitor_outcome(
    state: &Arc<ServerState>,
    pane_id: NodeId,
    monitor_id: u64,
    outcome: crate::progress_monitor::ProgressMonitorOutcome,
) {
    let final_progress = match &outcome {
        crate::progress_monitor::ProgressMonitorOutcome::TaskTerminal(progress)
        | crate::progress_monitor::ProgressMonitorOutcome::MonitorFailed { progress, .. } => {
            Some(progress.clone())
        }
        _ => None,
    };
    if let Some(progress) = final_progress {
        let mut panes = state.panes.write().await;
        let Some(PaneResource::Terminal(runtime)) = panes.get_mut(&pane_id) else {
            return;
        };
        if !runtime.update_progress_monitor_progress(monitor_id, progress.clone()) {
            return;
        }
        drop(panes);
        state.request_snapshot_save();
        if state.notifications_config.enabled {
            let pane_name = state
                .tree
                .read()
                .await
                .get(pane_id)
                .map(|node| node.name.clone())
                .unwrap_or_default();
            if let Some(pending) = crate::notifications::PendingNotification::for_task_outcome(
                state.session_name.clone(),
                pane_name,
                &progress,
            ) {
                // `send` never fails and runs the blocking D-Bus call on its
                // own blocking thread; delivery below waits for a ready
                // composer anyway, so this short await does not delay it.
                crate::notifications::send(pending).await;
            }
        }
    }
    let message = match outcome {
        crate::progress_monitor::ProgressMonitorOutcome::TaskTerminal(progress) => {
            crate::agent_delivery::terminal_result_message(&progress)
        }
        crate::progress_monitor::ProgressMonitorOutcome::MonitorFailed { progress, error } => {
            crate::agent_delivery::monitor_failure_message(&progress, &error.message)
        }
        crate::progress_monitor::ProgressMonitorOutcome::Disabled
        | crate::progress_monitor::ProgressMonitorOutcome::Superseded
        | crate::progress_monitor::ProgressMonitorOutcome::PaneUnavailable => return,
    };
    if let Err(error) =
        crate::agent_delivery::deliver_result(Arc::clone(state), pane_id, monitor_id, message).await
    {
        tracing::warn!(pane_id = pane_id.0, monitor_id, %error, "progress result delivery stopped");
    }
}

async fn handle_get_pane_progress_monitor_status(
    state: &Arc<ServerState>,
    request_id: u64,
    pane_id: NodeId,
    direct_tx: &mpsc::Sender<ServerEvent>,
) {
    let result = match state.panes.read().await.get(&pane_id) {
        Some(PaneResource::Terminal(runtime)) => Ok(runtime.progress_monitor_status(pane_id)),
        _ => Err(progress_rejection(
            ilium_ipc::ProgressMonitorRejectionCode::PaneNotFound,
            format!("pane {pane_id:?} is not a live terminal pane"),
        )),
    };
    send_direct(
        direct_tx,
        ServerEvent::ProgressMonitorStatusReported {
            request_id,
            pane_id,
            result,
        },
    )
    .await;
}

/// Stops one generation and clears its sticky presentation. A supplied ID is
/// an optimistic-concurrency fence, so an old agent cannot clear a replacement.
async fn handle_clear_pane_progress_monitor(
    state: &Arc<ServerState>,
    request_id: u64,
    pane_id: NodeId,
    expected_monitor_id: Option<u64>,
    direct_tx: &mpsc::Sender<ServerEvent>,
) {
    let effect_gate = {
        let panes = state.panes.read().await;
        match panes.get(&pane_id) {
            Some(PaneResource::Terminal(runtime)) => {
                Some(Arc::clone(&runtime.progress_effect_gate))
            }
            _ => None,
        }
    };
    let result = if let Some(effect_gate) = effect_gate {
        let _effect_guard = effect_gate.lock().await;
        let mut tree = state.tree.write().await;
        let mut panes = state.panes.write().await;
        let Some(PaneResource::Terminal(runtime)) = panes.get_mut(&pane_id) else {
            send_direct(
                direct_tx,
                ServerEvent::ProgressMonitorCleared {
                    request_id,
                    pane_id,
                    result: Err(progress_rejection(
                        ilium_ipc::ProgressMonitorRejectionCode::PaneNotFound,
                        format!("pane {pane_id:?} closed before clear"),
                    )),
                },
            )
            .await;
            return;
        };
        let current = runtime
            .progress_monitor
            .as_ref()
            .map(|monitor| monitor.monitor_id);
        if expected_monitor_id.is_some() && expected_monitor_id != current {
            Err(progress_rejection(
                ilium_ipc::ProgressMonitorRejectionCode::StaleMonitor,
                format!(
                    "expected progress monitor {:?}, but current monitor is {:?}",
                    expected_monitor_id, current
                ),
            ))
        } else {
            runtime.cancel_progress_monitor();
            let _ = tree.set_pane_progress(pane_id, None);
            Ok(current)
        }
    } else {
        Err(progress_rejection(
            ilium_ipc::ProgressMonitorRejectionCode::PaneNotFound,
            format!("pane {pane_id:?} is not a live terminal pane"),
        ))
    };
    if result.is_ok() {
        state.broadcast(ServerEvent::PaneProgressChanged {
            pane_id,
            progress: None,
        });
        state.request_snapshot_save();
        crate::persistence::flush_pending_snapshot(state).await;
    }
    send_direct(
        direct_tx,
        ServerEvent::ProgressMonitorCleared {
            request_id,
            pane_id,
            result,
        },
    )
    .await;
}

/// Reconstitutes one persisted registration after its pane has respawned.
/// Monitor IDs never cross the process boundary. Only a
/// nonterminal report whose fresh preflight has the same job identity resumes
/// recurring observation, and only delivery known never to have been
/// attempted is eligible for replay.
pub(crate) async fn restore_persisted_progress_monitor(
    state: &Arc<ServerState>,
    persisted: crate::persistence::PersistedProgressMonitor,
) -> Result<(), String> {
    if !state.is_progress_monitor_enabled() {
        return Err("progress monitoring is disabled".to_string());
    }
    let pane_id = persisted.pane_id;
    let effect_gate = {
        let panes = state.panes.read().await;
        let Some(PaneResource::Terminal(runtime)) = panes.get(&pane_id) else {
            return Err(format!("persisted monitor pane {pane_id:?} is unavailable"));
        };
        Arc::clone(&runtime.progress_effect_gate)
    };
    let monitor_id = state.allocate_progress_monitor_id();
    let (fresh_preflight, restoration_failure) = if persisted.requires_probe_before_restore() {
        match crate::progress_monitor::preflight(&persisted.command).await {
            Ok(preflight) if persisted.accepts_restored_preflight(&preflight) => {
                (Some(preflight), None)
            }
            Ok(preflight) => (
                None,
                Some(format!(
                    "progress observation identity could not be restored: current job_id {:?} does not match persisted job_id {:?}; task outcome is unknown",
                    preflight.report.job_id, persisted.latest_progress.report.job_id
                )),
            ),
            Err(error) => (
                None,
                Some(format!(
                    "progress observation could not be restored: {error}; task outcome is unknown"
                )),
            ),
        }
    } else {
        (None, None)
    };
    let mut registration = persisted
        .restored_registration(monitor_id)
        .map_err(|error| error.to_string())?;
    if let Some(preflight) = fresh_preflight {
        registration.initial_progress = ilium_core::PaneProgress::new(
            monitor_id,
            preflight.report,
            preflight.checked_at_unix_millis,
        )
        .map_err(|error| error.to_string())?;
    } else if let Some(restoration_failure) = restoration_failure.as_deref() {
        let mut failed_progress = registration.initial_progress.clone();
        failed_progress.monitor_health = ilium_core::ProgressMonitorHealth::Failed {
            consecutive_failures: crate::progress_monitor::MAXIMUM_CONSECUTIVE_OBSERVATION_FAILURES,
            last_error: bounded_restoration_failure(restoration_failure),
        };
        failed_progress
            .validate()
            .map_err(|error| format!("restored failure evidence is invalid: {error}"))?;
        registration.initial_progress = failed_progress;
    }
    let progress = registration.initial_progress.clone();
    let should_run_probe = !progress.is_terminal() && !progress.monitor_health.is_failed();
    let should_deliver = (persisted.result_delivery
        == crate::persistence::PersistedProgressDeliveryState::NotQueued
        || persisted.result_delivery.may_retry_after_restart())
        && (progress.is_terminal() || progress.monitor_health.is_failed());

    let _effect_guard = effect_gate.lock().await;
    let mut tree = state.tree.write().await;
    let mut panes = state.panes.write().await;
    let Some(PaneResource::Terminal(runtime)) = panes.get_mut(&pane_id) else {
        return Err(format!(
            "persisted monitor pane {pane_id:?} closed during restore"
        ));
    };
    let fence = runtime
        .install_progress_monitor(registration.clone())
        .map_err(|error| format!("persisted monitor was rejected: {error}"))?;
    runtime
        .restore_progress_delivery_state(persisted.result_delivery)
        .map_err(|error| format!("persisted delivery state was rejected: {error}"))?;
    tree.set_pane_progress(pane_id, Some(progress.clone()))
        .map_err(|error| error.to_string())?;

    if should_run_probe {
        let probe_task = crate::progress_monitor::spawn(Arc::clone(state), registration, fence);
        let outcome_state = Arc::clone(state);
        let task = tokio::spawn(async move {
            match probe_task.await {
                Ok(outcome) => {
                    handle_progress_monitor_outcome(&outcome_state, pane_id, monitor_id, outcome)
                        .await;
                }
                Err(error) if error.is_cancelled() => {}
                Err(error) => {
                    tracing::warn!(pane_id = pane_id.0, monitor_id, %error, "restored progress coordinator failed")
                }
            }
        });
        runtime.set_progress_monitor_task(task);
    } else if should_deliver {
        let message = if let Some(restoration_failure) = restoration_failure {
            format!(
                "Ilium could not restore this task's progress observation or identity. The task outcome is unknown. Details: {}",
                bounded_restoration_failure(&restoration_failure)
            )
        } else if progress.is_terminal() {
            crate::agent_delivery::terminal_result_message(&progress)
        } else {
            let error = match &progress.monitor_health {
                ilium_core::ProgressMonitorHealth::Failed { last_error, .. } => last_error.as_str(),
                _ => "progress observation stopped",
            };
            crate::agent_delivery::monitor_failure_message(&progress, error)
        };
        let delivery_state = Arc::clone(state);
        let task = tokio::spawn(async move {
            if let Err(error) =
                crate::agent_delivery::deliver_result(delivery_state, pane_id, monitor_id, message)
                    .await
            {
                tracing::warn!(pane_id = pane_id.0, monitor_id, %error, "restored progress result delivery stopped");
            }
        });
        runtime.set_progress_delivery_task(task);
    }
    drop(panes);
    drop(tree);
    state.broadcast(ServerEvent::PaneProgressChanged {
        pane_id,
        progress: Some(progress),
    });
    state.request_snapshot_save();
    Ok(())
}

fn bounded_restoration_failure(message: &str) -> String {
    let maximum_bytes = ilium_core::MAXIMUM_PROGRESS_MONITOR_ERROR_BYTES;
    if message.len() <= maximum_bytes {
        return message.to_string();
    }
    let mut boundary = maximum_bytes.saturating_sub(3).min(message.len());
    while boundary > 0 && !message.is_char_boundary(boundary) {
        boundary -= 1;
    }
    format!("{}...", &message[..boundary])
}

/// Applies a live kill switch. Nonterminal registrations are cancelled and
/// removed; terminal task evidence and failed-monitor evidence remain sticky
/// until explicit clear, but all of their pending automated delivery work is
/// stopped. Thus `false` always means no probe or progress-owned PTY task is
/// executing without destroying already-observed outcomes.
async fn handle_update_progress_monitor_enabled(state: &Arc<ServerState>, enabled: bool) {
    state.set_progress_monitor_enabled(enabled);
    state.broadcast(ServerEvent::ProgressMonitorEnabledChanged { enabled });
    if enabled {
        return;
    }

    let mut tree = state.tree.write().await;
    let mut panes = state.panes.write().await;
    let mut cleared_pane_ids = Vec::new();
    for (pane_id, resource) in panes.iter_mut() {
        let PaneResource::Terminal(runtime) = resource else {
            continue;
        };
        let preserve_evidence = runtime.progress_monitor.as_ref().is_some_and(|monitor| {
            monitor.latest_progress.is_terminal()
                || monitor.latest_progress.monitor_health.is_failed()
        });
        if preserve_evidence {
            runtime.stop_progress_tasks_preserving_state();
        } else if runtime.progress_monitor.is_some() {
            runtime.cancel_progress_monitor();
            let _ = tree.set_pane_progress(*pane_id, None);
            cleared_pane_ids.push(*pane_id);
        }
    }
    drop(panes);
    drop(tree);
    for pane_id in cleared_pane_ids {
        state.broadcast(ServerEvent::PaneProgressChanged {
            pane_id,
            progress: None,
        });
    }
    state.request_snapshot_save();
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
    if focused && is_terminal {
        acknowledge_progress_outcome(state, pane_id).await;
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
            // behind it; remove it rather than leaving a phantom node no
            // client could ever interact with. This handler has not broadcast
            // the node itself, but a concurrent structural mutation may have
            // snapshotted and broadcast the tree while the spawn was in
            // flight -- so broadcast (and re-mark the recovery snapshot)
            // after the removal, ensuring every attached client and the
            // persisted snapshot converge on the tree without the phantom.
            let mut tree = state.tree.write().await;
            let _ = tree.remove_node(pane_id);
            drop(tree);
            broadcast_and_persist(state).await;
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
    match spawn_and_register_pane_in_directory(
        state,
        pane_id,
        PaneSnapshotKind::Terminal(origin),
        &project_cwd,
    )
    .await
    {
        Ok(()) => {}
        Err(RegisterPaneError::NodeRemoved(_)) => {
            // A concurrent request removed the node while the spawn was in
            // flight; it already broadcast the tree without it and the
            // now-orphaned resource has been torn back down (see
            // `RegisterPaneError::NodeRemoved`) -- nothing to remove here.
            return Err(format!(
                "the {command_line} pane was removed before it could start"
            ));
        }
        Err(RegisterPaneError::Spawn(error)) => {
            // Same convergence rule as `handle_new_pane`'s spawn-error path:
            // remove the resourceless node, then broadcast so any client
            // that saw it via a concurrent mutation's snapshot drops it.
            let mut tree = state.tree.write().await;
            let _ = tree.remove_node(pane_id);
            drop(tree);
            broadcast_and_persist(state).await;
            return Err(format!("failed to spawn {command_line}: {error}"));
        }
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
    let (resource, output_receiver) = match kind {
        PaneSnapshotKind::Editor { path } => (PaneResource::Editor { path }, None),
        PaneSnapshotKind::Terminal(origin) => {
            let identity = pane::PaneIdentityEnv {
                pane_id,
                session_name: &state.session_name,
                socket_path: &state.socket_path,
            };
            let spawned = pane::spawn_terminal_session(&origin, cwd, &identity)?;
            let pending_generated_session_id = spawned.session_id;
            let session = spawned.session;
            // Subscribe before registration so the receiver retains output
            // produced during the short registration window. The task itself
            // starts only after the runtime is addressable (below).
            let output_receiver = session.subscribe_output_bytes();
            let runtime = crate::pane::TerminalPaneRuntime::new(
                session,
                origin,
                pending_generated_session_id,
                state.detection_config.idle_poll_interval,
            );
            (
                PaneResource::Terminal(Box::new(runtime)),
                Some(output_receiver),
            )
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
    if let Some(output_receiver) = output_receiver {
        let forward_task = tokio::spawn(forward_output_bytes(
            Arc::clone(state),
            pane_id,
            output_receiver,
        ));
        let Some(PaneResource::Terminal(runtime)) = panes.get_mut(&pane_id) else {
            forward_task.abort();
            unreachable!("the just-inserted terminal runtime must remain addressable");
        };
        runtime.set_forward_task(forward_task);
    }
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
    let mut activity_gate = OutputActivityGate::new();
    let mut subscription_cache = TerminalSubscriptionCache::new();
    let mut text_trigger_tracker = crate::text_triggers::TriggerTracker::default();
    let (trigger_delivery_sender, trigger_delivery_receiver) = tokio::sync::mpsc::channel(64);
    let _trigger_delivery_task = crate::task_guard::AbortOnDropHandle::new(tokio::spawn(
        crate::text_triggers::run_deliveries(
            std::sync::Arc::clone(&state),
            pane_id,
            trigger_delivery_receiver,
        ),
    ));
    loop {
        match receiver.recv().await {
            Ok(first_chunk) => {
                if activity_gate.should_record(std::time::Instant::now()) {
                    if let Err(error) = record_terminal_output_activity(&state, pane_id).await {
                        tracing::warn!("{error}");
                    }
                }
                // The PTY reader already parsed and journaled these bytes.
                // Hidden panes still feed Text Triggers even without clients.
                if !subscription_cache.has_subscribers(&state, pane_id) {
                    crate::text_triggers::process_output(
                        &state,
                        pane_id,
                        &mut text_trigger_tracker,
                        &first_chunk.bytes,
                        &trigger_delivery_sender,
                    )
                    .await;
                    continue;
                }
                match collect_output_burst(first_chunk, &mut receiver).await {
                    OutputBurst::Merged {
                        first_sequence,
                        sequence,
                        bytes,
                    } => {
                        // The visible stream merges extra PTY chunks. Match
                        // against that complete byte range, otherwise a
                        // regexp spanning a drained chunk is never observed.
                        state.broadcast(ServerEvent::ScreenUpdate {
                            pane_id,
                            first_sequence,
                            sequence,
                            bytes: bytes.clone(),
                        });
                        crate::text_triggers::process_output(
                            &state,
                            pane_id,
                            &mut text_trigger_tracker,
                            &bytes,
                            &trigger_delivery_sender,
                        )
                        .await;
                    }
                    OutputBurst::ReplayRequired { skipped } => {
                        // The pane's own screen already holds these bytes;
                        // adopt it rather than replaying a partial stream.
                        crate::text_triggers::resync_after_gap(
                            &state,
                            pane_id,
                            &mut text_trigger_tracker,
                            &trigger_delivery_sender,
                        )
                        .await;
                        tracing::warn!(
                            "pane {pane_id:?} output forwarder lagged, skipped {skipped} chunk(s)"
                        );
                        broadcast_terminal_replay(&state, pane_id).await;
                    }
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                crate::text_triggers::resync_after_gap(
                    &state,
                    pane_id,
                    &mut text_trigger_tracker,
                    &trigger_delivery_sender,
                )
                .await;
                tracing::warn!(
                    "pane {pane_id:?} output forwarder lagged, skipped {skipped} chunk(s)"
                );
                // Hidden panes deliberately need no live stream: their
                // journal is repaired only if they become visible. Avoid a
                // potentially 32 MiB replay allocation and an irrelevant IPC
                // broadcast merely because a hidden forwarder fell behind.
                if subscription_cache.has_subscribers(&state, pane_id) {
                    broadcast_terminal_replay(&state, pane_id).await;
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
        }
    }
}

/// Per-forwarder cache of the session-wide terminal-demand index. The output
/// path is intentionally lock-free while attachments and visible panes are
/// unchanged; connection transitions invalidate every cache through one
/// monotonic atomic revision.
struct TerminalSubscriptionCache {
    observed_revision: u64,
    has_subscribers: bool,
}

impl TerminalSubscriptionCache {
    fn new() -> Self {
        Self {
            observed_revision: u64::MAX,
            has_subscribers: false,
        }
    }

    fn has_subscribers(&mut self, state: &ServerState, pane_id: NodeId) -> bool {
        let current_revision = state.terminal_subscription_revision();
        if current_revision != self.observed_revision {
            self.has_subscribers = state.has_terminal_subscribers(pane_id);
            self.observed_revision = current_revision;
        }
        self.has_subscribers
    }
}

enum OutputBurst {
    Merged {
        first_sequence: u64,
        sequence: u64,
        bytes: Vec<u8>,
    },
    ReplayRequired {
        skipped: u64,
    },
}

/// Merges already-ready chunks plus one immediately-following PTY read.
async fn collect_output_burst(
    first_chunk: ilium_pty::PtyOutputChunk,
    receiver: &mut tokio::sync::broadcast::Receiver<ilium_pty::PtyOutputChunk>,
) -> OutputBurst {
    const MAX_MERGED_CHUNKS: usize = 32;
    const MAX_MERGED_BYTES: usize = 16 * 1024;
    const FOLLOWUP_WINDOW: std::time::Duration = std::time::Duration::from_micros(750);

    let first_sequence = first_chunk.sequence;
    let mut sequence = first_sequence;
    let mut bytes = first_chunk.bytes.to_vec();
    let mut merged_chunks = 1;
    let mut waited_for_followup = false;
    while merged_chunks < MAX_MERGED_CHUNKS && bytes.len() < MAX_MERGED_BYTES {
        let next = match receiver.try_recv() {
            Ok(chunk) => Some(Ok(chunk)),
            Err(tokio::sync::broadcast::error::TryRecvError::Empty) if !waited_for_followup => {
                waited_for_followup = true;
                tokio::time::timeout(FOLLOWUP_WINDOW, receiver.recv())
                    .await
                    .ok()
            }
            Err(tokio::sync::broadcast::error::TryRecvError::Empty)
            | Err(tokio::sync::broadcast::error::TryRecvError::Closed) => None,
            Err(tokio::sync::broadcast::error::TryRecvError::Lagged(skipped)) => {
                return OutputBurst::ReplayRequired { skipped };
            }
        };
        let Some(next) = next else {
            break;
        };
        match next {
            Ok(chunk) => {
                sequence = chunk.sequence;
                bytes.extend_from_slice(&chunk.bytes);
                merged_chunks += 1;
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                return OutputBurst::ReplayRequired { skipped };
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
        }
    }
    OutputBurst::Merged {
        first_sequence,
        sequence,
        bytes,
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

/// Builds the smallest terminal event needed when one connection makes a
/// previously hidden pane visible. Hidden output remains in the PTY-owned
/// journal; no session-global parser or subscription state is duplicated.
pub(crate) async fn terminal_recovery_event(
    state: &ServerState,
    pane_id: NodeId,
    after_sequence: u64,
) -> Option<ServerEvent> {
    let panes = state.panes.read().await;
    let PaneResource::Terminal(runtime) = panes.get(&pane_id)? else {
        return None;
    };
    match runtime.session.output_recovery_after(after_sequence)? {
        ilium_pty::PtyOutputRecovery::Delta(chunk) => Some(ServerEvent::ScreenUpdate {
            pane_id,
            first_sequence: after_sequence.saturating_add(1),
            sequence: chunk.sequence,
            bytes: chunk.bytes.to_vec(),
        }),
        ilium_pty::PtyOutputRecovery::Replay(replay) => {
            Some(terminal_replay_event(pane_id, replay))
        }
    }
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

/// Marks a pane's unread task outcome as seen and tells every client. Only
/// genuine human attention calls this (pane focus, client keyboard input);
/// automated PTY deliveries never do, because a written result message is not
/// proof that anyone read it.
pub(crate) async fn acknowledge_progress_outcome(state: &ServerState, pane_id: NodeId) {
    let acknowledged = {
        let mut tree = state.tree.write().await;
        let acknowledged = match tree.acknowledge_progress_outcome(pane_id) {
            Ok(acknowledged) => acknowledged,
            Err(error) => {
                tracing::error!(
                    pane_id = pane_id.0,
                    %error,
                    "progress outcome acknowledgement rejected"
                );
                None
            }
        };
        if let Some(progress) = acknowledged.as_ref() {
            // The runtime copy is what crash-recovery persists; keep both in
            // step so a restart does not resurrect an already-read outcome.
            let mut panes = state.panes.write().await;
            if let Some(PaneResource::Terminal(runtime)) = panes.get_mut(&pane_id) {
                runtime.update_progress_monitor_progress(progress.monitor_id, progress.clone());
            }
        }
        acknowledged
    };
    if let Some(progress) = acknowledged {
        state.request_snapshot_save();
        state.broadcast(ServerEvent::PaneProgressChanged {
            pane_id,
            progress: Some(progress),
        });
    }
}

async fn handle_key_input(
    state: &Arc<ServerState>,
    pane_id: NodeId,
    bytes: &[u8],
    submission: Option<PromptSubmissionSource>,
    direct_tx: &mpsc::Sender<ServerEvent>,
) {
    let result = write_key_input(state, pane_id, bytes, submission).await;
    if result.is_ok() && !bytes.is_empty() {
        acknowledge_progress_outcome(state, pane_id).await;
    }
    if let Err(message) = result {
        // `write_key_input` returns this same `Err(String)` both for an
        // actual PTY write failure and for a downstream bookkeeping failure
        // (e.g. `record_node_activity` rejecting a pane removed concurrently
        // with a write that already succeeded) -- label the diagnostic event
        // generically rather than asserting the write itself failed when it
        // may not have.
        let _ = crate::agent_debug::record(
            state,
            pane_id,
            AgentDebugSource::Pty,
            AgentDebugEventDraft {
                severity: AgentDebugSeverity::Error,
                kind: AgentDebugEventKind::Error,
                summary: "PTY input handling failed".to_string(),
                fields: vec![AgentDebugField::multiline("error", message.clone())],
                correlation_id: None,
                metadata: Default::default(),
            },
        )
        .await;
        send_direct_error(direct_tx, message).await;
    }
}

/// Codex's composer can consume an Enter arriving in the same input burst as
/// text as a paste newline or autocomplete acceptance. Its model picker was
/// verified live with a 280 ms text-to-Enter gap; use that established gap for
/// every automated submission, including ones owned by the detached server.
const AUTOMATED_ENTER_DELAY: std::time::Duration = std::time::Duration::from_millis(280);

/// Writes exactly the caller's raw bytes. Keyboard input and explicit
/// Enter-only actions retain their existing encoding and no staged behavior.
pub(crate) async fn write_key_input(
    state: &ServerState,
    pane_id: NodeId,
    bytes: &[u8],
    submission: Option<PromptSubmissionSource>,
) -> Result<(), String> {
    if submission.is_some() && bytes.last() != Some(&b'\r') {
        return Err("prompt submission metadata requires a trailing Enter".to_owned());
    }

    let input_gate = pane_input_gate(state, pane_id).await?;
    let _input_guard = input_gate.lock().await;
    let is_initial_prompt = submission == Some(PromptSubmissionSource::InitialAgentPrompt);
    write_key_input_unlocked(
        state,
        pane_id,
        bytes,
        submission,
        is_initial_prompt,
        &input_gate,
    )
    .await
}

async fn pane_input_gate(
    state: &ServerState,
    pane_id: NodeId,
) -> Result<std::sync::Arc<tokio::sync::Mutex<()>>, String> {
    let panes = state.panes.read().await;
    match panes.get(&pane_id) {
        Some(PaneResource::Terminal(runtime)) => Ok(std::sync::Arc::clone(&runtime.input_gate)),
        Some(PaneResource::Editor { .. }) => {
            Err(format!("pane {pane_id:?} is an editor, not a terminal"))
        }
        None => Err(format!("no pane found for node {pane_id:?}")),
    }
}

/// Inserts literal text and delivers a later standalone Enter. Only semantic
/// producers call this; raw `KeyInput` never changes its byte interpretation.
pub(crate) async fn submit_terminal_text(
    state: &ServerState,
    pane_id: NodeId,
    text: &str,
    source: PromptSubmissionSource,
) -> Result<(), String> {
    let input_gate = pane_input_gate(state, pane_id).await?;
    let _input_guard = input_gate.lock().await;
    submit_terminal_text_locked(state, pane_id, text, source, &input_gate).await
}

/// A queued Text Trigger is claimed only after acquiring this pane's input
/// gate. Edits accepted while it waited invalidate the old occurrence before
/// any PTY bytes are written, including an A-to-B-to-A settings cycle.
pub(crate) async fn submit_text_trigger_if_current(
    state: &ServerState,
    pane_id: NodeId,
    trigger_id: &str,
    message: &str,
    expected_revision: u64,
) -> Result<bool, String> {
    let input_gate = pane_input_gate(state, pane_id).await?;
    let _input_guard = input_gate.lock().await;
    let current_target = {
        let accepted = state.text_trigger_settings.read().await;
        (accepted.revision == expected_revision)
            .then(|| {
                accepted.settings.triggers.iter().find(|trigger| {
                    trigger.enabled && trigger.id == trigger_id && trigger.message == message
                })
            })
            .flatten()
            .map(|trigger| trigger.target)
    };
    let Some(target) = current_target else {
        return Ok(false);
    };
    let status = state
        .tree
        .read()
        .await
        .get(pane_id)
        .and_then(|node| match &node.kind {
            NodeKind::Pane { status, .. } => Some(status.clone()),
            _ => None,
        });
    if !status.is_some_and(|status| crate::text_triggers::target_matches(target, &status)) {
        return Ok(false);
    }
    submit_terminal_text_locked(
        state,
        pane_id,
        message,
        PromptSubmissionSource::TextTrigger,
        &input_gate,
    )
    .await?;
    Ok(true)
}

pub(crate) async fn submit_terminal_text_locked(
    state: &ServerState,
    pane_id: NodeId,
    text: &str,
    source: PromptSubmissionSource,
    input_gate: &std::sync::Arc<tokio::sync::Mutex<()>>,
) -> Result<(), String> {
    let wants_bracketed_paste = {
        let panes = state.panes.read().await;
        let Some(PaneResource::Terminal(runtime)) = panes.get(&pane_id) else {
            return Err(format!("pane {pane_id:?} closed before text insertion"));
        };
        if !std::sync::Arc::ptr_eq(input_gate, &runtime.input_gate) {
            return Err(format!("pane {pane_id:?} changed before text insertion"));
        }
        runtime
            .session
            .with_screen(|screen| screen.bracketed_paste())
    };
    let body = automated_submission_body(text.as_bytes(), wants_bracketed_paste)?;
    submit_terminal_body_locked(state, pane_id, &body, source, input_gate).await
}

pub(crate) async fn submit_terminal_body_locked(
    state: &ServerState,
    pane_id: NodeId,
    body: &[u8],
    source: PromptSubmissionSource,
    input_gate: &std::sync::Arc<tokio::sync::Mutex<()>>,
) -> Result<(), String> {
    let is_initial_prompt = source == PromptSubmissionSource::InitialAgentPrompt;
    if !body.is_empty() {
        write_key_input_unlocked(state, pane_id, body, None, is_initial_prompt, input_gate).await?;
        tokio::time::sleep(AUTOMATED_ENTER_DELAY).await;
    }
    write_key_input_unlocked(
        state,
        pane_id,
        b"\r",
        Some(source),
        is_initial_prompt,
        input_gate,
    )
    .await
}

/// A multiline agent prompt must be one paste operation so inner newlines do
/// not become premature Enter presses. Initial-agent prompts arrive already
/// framed; all other automatic producers carry literal UTF-8 text.
fn automated_submission_body(bytes: &[u8], wants_bracketed_paste: bool) -> Result<Vec<u8>, String> {
    const PASTE_START: &[u8] = b"\x1b[200~";
    const PASTE_END: &[u8] = b"\x1b[201~";
    let Ok(text) = std::str::from_utf8(bytes) else {
        return Ok(bytes.to_vec());
    };
    if text.contains("\x1b[200~") || text.contains("\x1b[201~") {
        return Err("terminal submission contains a bracketed-paste delimiter".to_owned());
    }
    if !text.contains(['\r', '\n']) {
        return Ok(bytes.to_vec());
    }
    if !wants_bracketed_paste {
        return Err("multiline terminal submission requires bracketed-paste support".to_owned());
    }
    let mut framed = Vec::with_capacity(PASTE_START.len() + bytes.len() + PASTE_END.len());
    framed.extend_from_slice(PASTE_START);
    framed.extend_from_slice(bytes);
    framed.extend_from_slice(PASTE_END);
    Ok(framed)
}

/// The established title, session-identity, activity and event path for one
/// physical PTY write. Call only while holding this pane's `input_gate`.
async fn write_key_input_unlocked(
    state: &ServerState,
    pane_id: NodeId,
    bytes: &[u8],
    submission: Option<PromptSubmissionSource>,
    is_initial_prompt: bool,
    expected_input_gate: &std::sync::Arc<tokio::sync::Mutex<()>>,
) -> Result<(), String> {
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
                PaneStatus::Agent(class, _) | PaneStatus::AgentWithGoal(class, _, _) => {
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
        Some(PaneResource::Terminal(runtime))
            if !std::sync::Arc::ptr_eq(expected_input_gate, &runtime.input_gate) =>
        {
            Some(format!(
                "pane {pane_id:?} runtime changed during input delivery"
            ))
        }
        Some(PaneResource::Terminal(runtime)) => {
            if !bytes.is_empty() && !is_initial_prompt {
                runtime.cancel_initial_prompt_delivery();
            }
            // A typed command only becomes a title while the shell itself owns
            // the terminal, which is how "the user is typing at a prompt" is
            // told apart from "a running command owns the terminal". A
            // platform that cannot tell answers `None`, and this stays false:
            // inferring a title without knowing who owns the terminal would
            // retitle panes from keystrokes typed into a running program.
            let is_shell_foreground = matches!(&runtime.origin, TerminalOrigin::PlainShell)
                && runtime.session.shell_owns_terminal().unwrap_or(false);
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
                if let (
                    Some(PromptSubmissionSource::Keyboard | PromptSubmissionSource::VoiceControl),
                    Some(line),
                ) = (submission, submitted_line.as_deref())
                {
                    // Only a human's `/goal pause` binds agent-requested
                    // resume; Ilium's own progress pause is not user intent.
                    runtime.observe_user_goal_command(line);
                }
                if submitted_line
                    .as_deref()
                    .is_some_and(crate::pane::clears_agent_goal)
                {
                    // The successful PTY write is authoritative user intent.
                    // Clear retained ownership immediately so a footer-hidden
                    // `/goal clear` cannot leave a sticky sidebar flag.
                    runtime.clear_confirmed_goal_owner();
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
        if let Err(error) = record_node_activity(state, pane_id).await {
            // The PTY write already succeeded. A later tree mutation must not
            // turn this into a retryable delivery failure for queued work.
            tracing::warn!(pane_id = pane_id.0, %error, "input activity bookkeeping failed after PTY write");
        }
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
                AgentDebugField::sensitive("submitted input", text.clone()),
            ])
            .with_correlation_id(submission_correlation_id.clone()),
        )
        .await;

        // Only a hand-typed or pasted, exactly reconstructed line updates the
        // banner -- other submission sources (voice, scheduled/queued
        // prompts, toolbar actions) already have their own presentation, and
        // an inexact/opaque reconstruction must leave the last good value in
        // place rather than overwrite it with placeholder text.
        if submission == Some(PromptSubmissionSource::Keyboard)
            && exactness == "exact"
            && !text.is_empty()
        {
            let updated = {
                let mut tree = state.tree.write().await;
                tree.set_last_prompt(pane_id, Some(text.clone())).is_ok()
            };
            if updated {
                state.request_snapshot_save();
                state.broadcast(ServerEvent::PaneLastPromptChanged {
                    pane_id,
                    last_prompt: Some(text),
                });
            }
        }
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
    let input_gate = match pane_input_gate(state, pane_id).await {
        Ok(input_gate) => input_gate,
        Err(message) => {
            send_direct_error(direct_tx, message).await;
            return;
        }
    };
    let _input_guard = input_gate.lock().await;
    let panes = state.panes.read().await;
    let error_message = match panes.get(&pane_id) {
        Some(PaneResource::Terminal(runtime))
            if std::sync::Arc::ptr_eq(&input_gate, &runtime.input_gate) =>
        {
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
        Some(PaneResource::Terminal(_)) => {
            Some(format!("pane {pane_id:?} changed before mouse input"))
        }
        None => Some(format!("no pane found for node {pane_id:?}")),
    };
    drop(panes);
    drop(_input_guard);

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

    // A cleanly-killed session has nothing left worth recovering. Marking it
    // killed both refuses every later `request_snapshot_save` -- `crate::run`'s
    // shutdown grace period keeps other attached connections alive for a short
    // window after this returns, and one of them could otherwise resurrect a
    // snapshot via `NewPane` -- and discards any pending write, so the
    // background debounced writer
    // (`crate::persistence::spawn_snapshot_writer`) cannot recreate the file
    // after we remove it below just because an earlier mutation had left one
    // owed. Both are the same atomic step; see `crate::snapshot_state` for
    // why they must be. Then take the same write lock
    // `persistence::save_snapshot` holds for a save's entire
    // build+serialize+write+rename, so a write already in flight when this
    // handler runs is guaranteed to finish (writing the *old* snapshot)
    // before we remove the file, rather than racing it.
    state.mark_session_killed();
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

    #[tokio::test]
    async fn output_burst_collects_an_immediately_following_reader_chunk() {
        let (sender, mut receiver) = tokio::sync::broadcast::channel(8);
        sender
            .send(ilium_pty::PtyOutputChunk {
                sequence: 1,
                bytes: b"first".to_vec().into(),
            })
            .unwrap();
        let first = receiver.recv().await.unwrap();
        let producer = tokio::spawn(async move {
            tokio::task::yield_now().await;
            sender
                .send(ilium_pty::PtyOutputChunk {
                    sequence: 2,
                    bytes: b"second".to_vec().into(),
                })
                .unwrap();
        });

        let OutputBurst::Merged {
            first_sequence,
            sequence,
            bytes,
        } = collect_output_burst(first, &mut receiver).await
        else {
            panic!("contiguous output must not require replay");
        };
        producer.await.unwrap();
        assert_eq!((first_sequence, sequence), (1, 2));
        assert_eq!(bytes, b"firstsecond");
    }

    #[tokio::test]
    #[ignore = "manual performance benchmark"]
    async fn benchmark_output_subframe_coalescing() {
        const CHUNKS: u64 = 32;
        let (sender, mut receiver) = tokio::sync::broadcast::channel(64);
        sender
            .send(ilium_pty::PtyOutputChunk {
                sequence: 1,
                bytes: vec![b'x'; 64].into(),
            })
            .unwrap();
        let first = receiver.recv().await.unwrap();
        let producer = tokio::spawn(async move {
            tokio::task::yield_now().await;
            for sequence in 2..=CHUNKS {
                sender
                    .send(ilium_pty::PtyOutputChunk {
                        sequence,
                        bytes: vec![b'x'; 64].into(),
                    })
                    .unwrap();
            }
        });
        let started_at = std::time::Instant::now();
        let burst = collect_output_burst(first, &mut receiver).await;
        let elapsed = started_at.elapsed();
        producer.await.unwrap();
        let OutputBurst::Merged {
            sequence, bytes, ..
        } = burst
        else {
            panic!("benchmark burst must remain contiguous");
        };
        println!(
            "PERF server.output_coalescing baseline_frames={CHUNKS} merged_frames=1 elapsed_ns={} merged_bytes={} through_sequence={sequence}",
            elapsed.as_nanos(),
            bytes.len(),
        );
    }

    #[test]
    fn output_activity_gate_records_first_and_periodic_burst_activity() {
        let started_at = std::time::Instant::now();
        let mut gate = OutputActivityGate::new();

        assert!(gate.should_record(started_at));
        assert!(!gate.should_record(started_at + std::time::Duration::from_millis(499)));
        assert!(gate.should_record(started_at + std::time::Duration::from_millis(500)));
    }

    #[test]
    #[ignore = "manual performance benchmark"]
    fn benchmark_output_activity_debounce() {
        const CHUNKS: usize = 10_000;
        let mut baseline_tree = Tree::new();
        let baseline_group = baseline_tree
            .add_group(ilium_core::ROOT_ID, "work")
            .unwrap();
        let baseline_pane = baseline_tree
            .add_pane(baseline_group, "pane", PaneContentKind::Terminal)
            .unwrap();
        let baseline_started_at = std::time::Instant::now();
        for _chunk in 0..CHUNKS {
            std::hint::black_box(baseline_tree.record_node_activity(baseline_pane).unwrap());
        }
        let baseline_elapsed = baseline_started_at.elapsed();

        let mut debounced_tree = Tree::new();
        let debounced_group = debounced_tree
            .add_group(ilium_core::ROOT_ID, "work")
            .unwrap();
        let debounced_pane = debounced_tree
            .add_pane(debounced_group, "pane", PaneContentKind::Terminal)
            .unwrap();
        let now = std::time::Instant::now();
        let mut gate = OutputActivityGate::new();
        let debounced_started_at = std::time::Instant::now();
        let mut recorded = 0;
        for _chunk in 0..CHUNKS {
            if gate.should_record(now) {
                recorded += 1;
                std::hint::black_box(debounced_tree.record_node_activity(debounced_pane).unwrap());
            }
        }
        let debounced_elapsed = debounced_started_at.elapsed();
        println!(
            "PERF server.output_activity baseline_ns={} debounced_ns={} recorded={recorded}",
            baseline_elapsed.as_nanos(),
            debounced_elapsed.as_nanos(),
        );
    }

    /// A command that reads standard input and stays alive, spelled per platform.
    ///
    /// These fixtures need a pane whose process keeps running until the test kills
    /// it. `cat` is the obvious choice on Unix and does not exist on Windows, where
    /// a snapshot naming it simply fails to respawn and the restored tree no longer
    /// matches what was saved.
    fn long_running_pane_command() -> String {
        if cfg!(windows) { "findstr x" } else { "cat" }.to_string()
    }
    use super::*;
    use crate::initial_prompt::initial_input_bytes;
    use ilium_core::{NodeId, RestructureNode, SplitOrientation};
    use std::time::Duration;

    #[test]
    fn automated_multiline_body_preserves_literal_text_inside_bracketed_paste() {
        assert_eq!(
            automated_submission_body(b"first\r\nsecond\nthird", true).unwrap(),
            b"\x1b[200~first\r\nsecond\nthird\x1b[201~"
        );
        assert_eq!(
            automated_submission_body(b"/model", true).unwrap(),
            b"/model"
        );
        assert_eq!(
            automated_submission_body(b"first\nsecond", false).unwrap_err(),
            "multiline terminal submission requires bracketed-paste support"
        );
    }

    #[test]
    fn automated_multiline_body_rejects_embedded_paste_delimiter_without_writing() {
        assert!(automated_submission_body(b"first\n\x1b[201~second", true).is_err());
    }

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
            socket_path: directory.path().join("test.sock"),
            detection_config: crate::config::DetectionConfig::default(),
            notifications_config: crate::config::NotificationsConfig::default(),
            sound_settings: ilium_sound::SoundSettings::default(),
            sound_requests,
            custom_signatures: Vec::new(),
            agent_debug_menu_enabled: false,
            progress_monitor_enabled: true,
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
        assert!(state.is_snapshot_dirty());
        assert!(direct_rx.try_recv().is_err());
        sound_task.abort();
    }

    #[tokio::test]
    async fn locking_a_folder_closed_collapses_it_and_rejects_a_stale_expand_request() {
        let directory = tempfile::tempdir().expect("create lock test directory");
        let (sound_requests, sound_task) = crate::sounds::spawn(Arc::new(crate::NoopSoundPlayer));
        let state = Arc::new(ServerState::new(crate::state::ServerStateOptions {
            session_name: "lock-request".to_string(),
            session_cwd: directory.path().to_path_buf(),
            home_dir: directory.path().to_path_buf(),
            snapshot_path: directory.path().join("lock.snapshot.json"),
            socket_path: directory.path().join("test.sock"),
            detection_config: crate::config::DetectionConfig::default(),
            notifications_config: crate::config::NotificationsConfig::default(),
            sound_settings: ilium_sound::SoundSettings::default(),
            sound_requests,
            custom_signatures: Vec::new(),
            agent_debug_menu_enabled: false,
            progress_monitor_enabled: true,
        }));
        let folder_id = {
            let mut tree = state.tree.write().await;
            let project_id = tree
                .project_ids()
                .into_iter()
                .next()
                .expect("fresh server has its launch project");
            let group_id = tree
                .add_group(project_id, "work")
                .expect("launch project accepts a group");
            tree.add_folder(group_id, directory.path().to_path_buf())
                .expect("group accepts a folder")
        };
        let mut events = state.events.subscribe();
        let (direct_tx, mut direct_rx) = mpsc::channel(1);

        assert!(
            !handle_request(
                &state,
                ClientRequest::SetNodeLockedClosed {
                    node_id: folder_id,
                    locked_closed: true,
                },
                &direct_tx,
            )
            .await
        );
        let ServerEvent::TreeSnapshot(snapshot) = events.recv().await.expect("tree broadcast")
        else {
            panic!("lock update must broadcast a tree snapshot");
        };
        assert_eq!(
            snapshot.get(folder_id).unwrap().is_locked_closed(),
            Some(true)
        );
        assert_eq!(snapshot.get(folder_id).unwrap().is_expanded(), Some(false));
        assert!(state.is_snapshot_dirty());
        assert!(direct_rx.try_recv().is_err());

        // A stale client that doesn't know about the lock still can't
        // expand it -- the server rejects the mutation and sends only a
        // direct error, no broadcast.
        assert!(
            !handle_request(
                &state,
                ClientRequest::SetNodeExpanded {
                    node_id: folder_id,
                    expanded: true,
                },
                &direct_tx,
            )
            .await
        );
        assert!(events.try_recv().is_err());
        assert!(direct_rx.try_recv().is_ok());

        assert!(
            !handle_request(
                &state,
                ClientRequest::SetNodeLockedClosed {
                    node_id: folder_id,
                    locked_closed: false,
                },
                &direct_tx,
            )
            .await
        );
        let ServerEvent::TreeSnapshot(unlocked) = events.recv().await.expect("tree broadcast")
        else {
            panic!("unlock must broadcast a tree snapshot");
        };
        assert_eq!(
            unlocked.get(folder_id).unwrap().is_locked_closed(),
            Some(false)
        );
        assert_eq!(unlocked.get(folder_id).unwrap().is_expanded(), Some(true));
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
            socket_path: directory.path().join("test.sock"),
            detection_config: crate::config::DetectionConfig::default(),
            notifications_config: crate::config::NotificationsConfig::default(),
            sound_settings: ilium_sound::SoundSettings::default(),
            sound_requests,
            custom_signatures: Vec::new(),
            agent_debug_menu_enabled: false,
            progress_monitor_enabled: true,
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
    async fn hidden_terminal_output_publishes_only_the_first_unread_revision() {
        let directory = tempfile::tempdir().expect("create hidden activity directory");
        let (sound_requests, sound_task) = crate::sounds::spawn(Arc::new(crate::NoopSoundPlayer));
        let state = Arc::new(ServerState::new(crate::state::ServerStateOptions {
            session_name: "hidden-terminal-activity".to_string(),
            session_cwd: directory.path().to_path_buf(),
            home_dir: directory.path().to_path_buf(),
            snapshot_path: directory
                .path()
                .join("hidden-terminal-activity.snapshot.json"),
            socket_path: directory.path().join("test.sock"),
            detection_config: crate::config::DetectionConfig::default(),
            notifications_config: crate::config::NotificationsConfig::default(),
            sound_settings: ilium_sound::SoundSettings::default(),
            sound_requests,
            custom_signatures: Vec::new(),
            agent_debug_menu_enabled: false,
            progress_monitor_enabled: true,
        }));
        let pane_id = {
            let mut tree = state.tree.write().await;
            let project_id = tree.project_ids()[0];
            let pane_id = tree
                .add_pane(project_id, "hidden", PaneContentKind::Terminal)
                .expect("project accepts a terminal");
            tree.mark_node_focused(pane_id)
                .expect("initial focus checkpoints the terminal");
            pane_id
        };
        let mut events = state.events.subscribe();

        let first_revision = record_terminal_output_activity(&state, pane_id)
            .await
            .expect("first hidden output is accepted");
        assert!(matches!(
            events.recv().await,
            Ok(ServerEvent::NodeActivityChanged { node_id, activity_revision })
                if node_id == pane_id && activity_revision == first_revision
        ));

        let second_revision = record_terminal_output_activity(&state, pane_id)
            .await
            .expect("later hidden output is accepted");
        assert_eq!(second_revision, first_revision + 1);
        assert!(events.try_recv().is_err());
        assert_eq!(
            state
                .tree
                .read()
                .await
                .get(pane_id)
                .unwrap()
                .activity_revision,
            second_revision,
            "suppressed broadcasts must not weaken authoritative revision fencing"
        );

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
            socket_path: directory.path().join("test.sock"),
            detection_config: crate::config::DetectionConfig::default(),
            notifications_config: crate::config::NotificationsConfig::default(),
            sound_settings: ilium_sound::SoundSettings::default(),
            sound_requests,
            custom_signatures: Vec::new(),
            agent_debug_menu_enabled: false,
            progress_monitor_enabled: true,
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
        state.replace_terminal_subscriptions(
            false,
            &std::collections::HashSet::new(),
            false,
            &std::collections::HashSet::from([pane_id]),
        );
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
        // At least, not exactly: the event is published after the tree is
        // updated, so the tree can only be at or ahead of an announced
        // revision. A PTY is free to deliver its output in more chunks than
        // one -- ConPTY routinely does -- and each chunk advances activity
        // again, so demanding equality would be asserting a property of the
        // platform's buffering rather than of ilium.
        let tree_revision = state
            .tree
            .read()
            .await
            .get(pane_id)
            .unwrap()
            .activity_revision;
        assert!(
            tree_revision >= output_revision,
            "tree revision {tree_revision} is behind the announced revision {output_revision}"
        );

        let resources: Vec<_> = state.panes.write().await.drain().collect();
        for (resource_pane_id, resource) in resources {
            teardown_pane_resource(resource_pane_id, resource);
        }
        sound_task.abort();
    }

    /// Builds a session with one live `cat` terminal pane, ready for
    /// progress-monitor requests. Returns the state, that pane's id, and the
    /// backing `TempDir` -- callers must keep the `TempDir` binding alive for
    /// the rest of the test (dropping it early removes the pane's cwd) and
    /// are responsible for draining `state.panes` at the end (see
    /// `teardown_state_panes`); the sound actor needs no cleanup since
    /// `NoopSoundPlayer` plays nothing.
    async fn state_with_one_terminal_pane(
        session_name: &str,
    ) -> (Arc<ServerState>, NodeId, tempfile::TempDir) {
        let directory = tempfile::tempdir().expect("create progress monitor test directory");
        let (sound_requests, _sound_task) = crate::sounds::spawn(Arc::new(crate::NoopSoundPlayer));
        let state = Arc::new(ServerState::new(crate::state::ServerStateOptions {
            session_name: session_name.to_string(),
            session_cwd: directory.path().to_path_buf(),
            home_dir: directory.path().to_path_buf(),
            snapshot_path: directory.path().join("progress-monitor.snapshot.json"),
            socket_path: directory.path().join("test.sock"),
            detection_config: crate::config::DetectionConfig::default(),
            notifications_config: crate::config::NotificationsConfig::default(),
            sound_settings: ilium_sound::SoundSettings::default(),
            sound_requests,
            custom_signatures: Vec::new(),
            agent_debug_menu_enabled: false,
            progress_monitor_enabled: true,
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
        .expect("spawn progress monitor fixture pane");
        (state, pane_id, directory)
    }

    fn teardown_state_panes(state: &Arc<ServerState>) {
        let resources: Vec<_> = state
            .panes
            .try_write()
            .expect("no concurrent pane access at test teardown")
            .drain()
            .collect();
        for (resource_pane_id, resource) in resources {
            teardown_pane_resource(resource_pane_id, resource);
        }
    }

    fn persisted_running_progress_monitor(
        pane_id: NodeId,
        command: String,
    ) -> crate::persistence::PersistedProgressMonitor {
        crate::persistence::PersistedProgressMonitor {
            pane_id,
            command,
            interval_seconds: 60,
            latest_progress: ilium_core::PaneProgress::new(
                99,
                ilium_core::ProgressTaskReport::new(
                    "persisted-job".to_string(),
                    ilium_core::ProgressTaskStatus::Running,
                    55.0,
                    "last observed before restart".to_string(),
                    None,
                )
                .unwrap(),
                1_700_000_000_000,
            )
            .unwrap(),
            result_delivery: crate::persistence::PersistedProgressDeliveryState::NotQueued,
        }
    }

    #[tokio::test]
    async fn set_pane_progress_monitor_runs_the_command_and_broadcasts_reported_progress() {
        let (state, pane_id, _directory) =
            state_with_one_terminal_pane("progress-monitor-set-and-report").await;
        let mut events = state.events.subscribe();
        let (direct_tx, mut direct_rx) = mpsc::channel(1);

        assert!(
            !handle_request(
                &state,
                ClientRequest::SetPaneProgressMonitor {
                    request_id: 11,
                    pane_id,
                    command: r#"printf '%s' '{"job_id":"render-11","status":"running","percent":42.5,"message":"frame 10/100"}'"#
                        .to_string(),
                    interval_seconds: 1,
                },
                &direct_tx,
            )
            .await
        );
        let accepted = direct_rx
            .recv()
            .await
            .expect("registration acknowledgement");
        assert!(matches!(
            accepted,
            ServerEvent::ProgressMonitorSetCompleted {
                request_id: 11,
                result: Ok(_),
                ..
            }
        ));

        let progress = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                match events.recv().await {
                    Ok(ServerEvent::PaneProgressChanged {
                        pane_id: event_pane_id,
                        progress: Some(progress),
                    }) if event_pane_id == pane_id => break progress,
                    Ok(_) => {}
                    Err(error) => panic!("progress event stream closed: {error}"),
                }
            }
        })
        .await
        .expect("the monitor command's first tick should report progress");
        assert_eq!(progress.report.percent, 42.5);
        assert_eq!(progress.report.message, "frame 10/100");
        assert_eq!(
            state.tree.read().await.pane_progress(pane_id),
            Some(&progress)
        );

        teardown_state_panes(&state);
    }

    #[tokio::test]
    async fn failed_replacement_and_stale_clear_preserve_the_accepted_monitor() {
        let (state, pane_id, _directory) =
            state_with_one_terminal_pane("progress-monitor-transactional-replacement").await;
        let (direct_tx, mut direct_rx) = mpsc::channel(4);
        handle_request(
            &state,
            ClientRequest::SetPaneProgressMonitor {
                request_id: 15,
                pane_id,
                command: r#"printf '%s' '{"job_id":"kept-job","status":"running","percent":25,"message":"healthy"}'"#.to_string(),
                interval_seconds: 60,
            },
            &direct_tx,
        )
        .await;
        let accepted_id = match direct_rx.recv().await.unwrap() {
            ServerEvent::ProgressMonitorSetCompleted {
                result: Ok(accepted),
                ..
            } => accepted.monitor_id,
            event => panic!("unexpected registration response: {event:?}"),
        };

        handle_request(
            &state,
            ClientRequest::SetPaneProgressMonitor {
                request_id: 16,
                pane_id,
                command: "printf '%s' 'not-json'".to_string(),
                interval_seconds: 1,
            },
            &direct_tx,
        )
        .await;
        assert!(matches!(
            direct_rx.recv().await,
            Some(ServerEvent::ProgressMonitorSetCompleted {
                request_id: 16,
                result: Err(_),
                ..
            })
        ));

        handle_request(
            &state,
            ClientRequest::ClearPaneProgressMonitor {
                request_id: 17,
                pane_id,
                expected_monitor_id: Some(accepted_id + 1),
            },
            &direct_tx,
        )
        .await;
        assert!(matches!(
            direct_rx.recv().await,
            Some(ServerEvent::ProgressMonitorCleared {
                request_id: 17,
                result: Err(ilium_ipc::ProgressMonitorRejection {
                    code: ilium_ipc::ProgressMonitorRejectionCode::StaleMonitor,
                    ..
                }),
                ..
            })
        ));

        handle_request(
            &state,
            ClientRequest::GetPaneProgressMonitorStatus {
                request_id: 18,
                pane_id,
            },
            &direct_tx,
        )
        .await;
        assert!(matches!(
            direct_rx.recv().await,
            Some(ServerEvent::ProgressMonitorStatusReported {
                request_id: 18,
                result: Ok(ilium_ipc::ProgressMonitorStatus {
                    progress: Some(progress),
                    ..
                }),
                ..
            }) if progress.monitor_id == accepted_id && progress.report.job_id == "kept-job"
        ));
        teardown_state_panes(&state);
    }

    #[tokio::test]
    async fn repeated_progress_set_request_replays_one_committed_result_without_rerunning_probe() {
        let (state, pane_id, directory) =
            state_with_one_terminal_pane("progress-monitor-idempotent-set").await;
        let invocation_log = directory.path().join("probe-invocations.log");
        let command = format!(
            "printf x >> '{}'; sleep 0.1; printf '%s' '{{\"job_id\":\"idempotent-job\",\"status\":\"running\",\"percent\":12,\"message\":\"running\"}}'",
            invocation_log.display()
        );
        let (direct_tx, mut direct_rx) = mpsc::channel(2);
        let request = || ClientRequest::SetPaneProgressMonitor {
            request_id: 19,
            pane_id,
            command: command.clone(),
            interval_seconds: 60,
        };

        let (first_handled, second_handled) = tokio::join!(
            handle_request(&state, request(), &direct_tx),
            handle_request(&state, request(), &direct_tx)
        );
        assert!(!first_handled && !second_handled);
        let first = direct_rx.recv().await.expect("first set acknowledgement");
        let second = direct_rx
            .recv()
            .await
            .expect("replayed set acknowledgement");
        let first_result = match first {
            ServerEvent::ProgressMonitorSetCompleted { result, .. } => result,
            event => panic!("unexpected first response: {event:?}"),
        };
        let second_result = match second {
            ServerEvent::ProgressMonitorSetCompleted { result, .. } => result,
            event => panic!("unexpected replay response: {event:?}"),
        };
        assert_eq!(second_result, first_result);
        assert!(first_result.is_ok());
        assert_eq!(
            tokio::fs::read_to_string(&invocation_log)
                .await
                .expect("probe invocation log"),
            "x",
            "the exact retry must not rerun preflight"
        );

        handle_request(
            &state,
            ClientRequest::SetPaneProgressMonitor {
                request_id: 19,
                pane_id,
                command: r#"printf '%s' '{"job_id":"collision","status":"running","percent":1,"message":"different"}'"#.to_string(),
                interval_seconds: 60,
            },
            &direct_tx,
        )
        .await;
        assert!(matches!(
            direct_rx.recv().await,
            Some(ServerEvent::ProgressMonitorSetCompleted {
                result: Err(ilium_ipc::ProgressMonitorRejection {
                    code: ilium_ipc::ProgressMonitorRejectionCode::InvalidRequest,
                    ..
                }),
                ..
            })
        ));

        teardown_state_panes(&state);
    }

    #[tokio::test]
    async fn progress_set_acknowledges_only_after_snapshot_contains_the_monitor() {
        let (state, pane_id, _directory) =
            state_with_one_terminal_pane("progress-monitor-durable-ack").await;
        let (direct_tx, mut direct_rx) = mpsc::channel(1);
        handle_request(
            &state,
            ClientRequest::SetPaneProgressMonitor {
                request_id: 20,
                pane_id,
                command: r#"printf '%s' '{"job_id":"durable-job","status":"running","percent":17,"message":"running"}'"#.to_string(),
                interval_seconds: 60,
            },
            &direct_tx,
        )
        .await;
        let accepted_monitor_id = match direct_rx.recv().await.expect("set acknowledgement") {
            ServerEvent::ProgressMonitorSetCompleted {
                result: Ok(accepted),
                ..
            } => accepted.monitor_id,
            event => panic!("unexpected set response: {event:?}"),
        };
        let snapshot = crate::persistence::load_snapshot(&state.snapshot_path)
            .await
            .expect("durable snapshot is readable")
            .expect("durable snapshot exists before acknowledgement is observed");
        assert!(snapshot.progress_monitors.iter().any(|monitor| {
            monitor.pane_id == pane_id
                && monitor.latest_progress.monitor_id == accepted_monitor_id
                && monitor.latest_progress.report.job_id == "durable-job"
        }));

        teardown_state_panes(&state);
    }

    #[tokio::test]
    async fn durability_barrier_writes_even_after_background_writer_claims_dirty_flag() {
        let (state, pane_id, _directory) =
            state_with_one_terminal_pane("snapshot-barrier-after-dirty-claim").await;
        state.request_snapshot_save();
        assert!(
            state.take_pending_snapshot(),
            "fixture simulates the background writer having claimed the dirty flag"
        );
        assert!(!state.is_snapshot_dirty());

        crate::persistence::await_snapshot_durability_barrier(&state)
            .await
            .expect("barrier must not depend on the dirty flag");
        let snapshot = crate::persistence::load_snapshot(&state.snapshot_path)
            .await
            .expect("barrier snapshot is readable")
            .expect("barrier creates a snapshot");
        assert!(snapshot.panes.iter().any(|pane| pane.node_id == pane_id));

        teardown_state_panes(&state);
    }

    #[tokio::test]
    async fn failed_progress_snapshot_write_rejects_replacement_and_preserves_old_monitor() {
        let (state, pane_id, _directory) =
            state_with_one_terminal_pane("progress-monitor-persistence-failure").await;
        let (direct_tx, mut direct_rx) = mpsc::channel(3);
        handle_request(
            &state,
            ClientRequest::SetPaneProgressMonitor {
                request_id: 23,
                pane_id,
                command: r#"printf '%s' '{"job_id":"preserved-job","status":"running","percent":23,"message":"running"}'"#.to_string(),
                interval_seconds: 60,
            },
            &direct_tx,
        )
        .await;
        let preserved_monitor_id = match direct_rx.recv().await.unwrap() {
            ServerEvent::ProgressMonitorSetCompleted {
                result: Ok(accepted),
                ..
            } => accepted.monitor_id,
            event => panic!("unexpected initial response: {event:?}"),
        };
        tokio::fs::remove_file(&state.snapshot_path)
            .await
            .expect("remove writable snapshot");
        tokio::fs::create_dir(&state.snapshot_path)
            .await
            .expect("replace snapshot file with an unwritable directory target");

        handle_request(
            &state,
            ClientRequest::SetPaneProgressMonitor {
                request_id: 24,
                pane_id,
                command: r#"printf '%s' '{"job_id":"unacknowledged-job","status":"running","percent":24,"message":"running"}'"#.to_string(),
                interval_seconds: 60,
            },
            &direct_tx,
        )
        .await;
        assert!(matches!(
            direct_rx.recv().await,
            Some(ServerEvent::ProgressMonitorSetCompleted {
                request_id: 24,
                result: Err(ilium_ipc::ProgressMonitorRejection { message, .. }),
                ..
            }) if message.contains("durably persist")
        ));
        handle_request(
            &state,
            ClientRequest::GetPaneProgressMonitorStatus {
                request_id: 25,
                pane_id,
            },
            &direct_tx,
        )
        .await;
        assert!(matches!(
            direct_rx.recv().await,
            Some(ServerEvent::ProgressMonitorStatusReported {
                result: Ok(ilium_ipc::ProgressMonitorStatus {
                    progress: Some(progress),
                    ..
                }),
                ..
            }) if progress.monitor_id == preserved_monitor_id
                && progress.report.job_id == "preserved-job"
        ));

        teardown_state_panes(&state);
    }

    #[tokio::test]
    async fn restore_probe_failure_keeps_sticky_unknown_outcome_evidence_and_queues_notice() {
        let (state, pane_id, _directory) =
            state_with_one_terminal_pane("progress-monitor-restore-probe-failure").await;
        let persisted = persisted_running_progress_monitor(
            pane_id,
            "printf '%s' 'probe failed' >&2; exit 7".to_string(),
        );

        restore_persisted_progress_monitor(&state, persisted)
            .await
            .expect("a failed restored probe becomes sticky evidence");
        tokio::task::yield_now().await;
        let (progress, delivery) = {
            let panes = state.panes.read().await;
            let PaneResource::Terminal(runtime) = panes.get(&pane_id).unwrap() else {
                panic!("fixture pane must remain terminal");
            };
            let monitor = runtime.progress_monitor.as_ref().unwrap();
            (monitor.latest_progress.clone(), monitor.result_delivery)
        };
        assert!(matches!(
            progress.monitor_health,
            ilium_core::ProgressMonitorHealth::Failed { ref last_error, .. }
                if last_error.contains("could not be restored")
                    && last_error.contains("task outcome is unknown")
        ));
        assert_ne!(
            progress.monitor_id, 99,
            "restore must allocate a fresh fence"
        );
        assert_eq!(progress.report.job_id, "persisted-job");
        assert!(matches!(
            delivery,
            crate::pane::ProgressDeliveryState::Queued
                | crate::pane::ProgressDeliveryState::Attempted
                | crate::pane::ProgressDeliveryState::DeliveredToPty
        ));
        assert_eq!(
            state.tree.read().await.pane_progress(pane_id),
            Some(&progress)
        );

        teardown_state_panes(&state);
    }

    #[tokio::test]
    async fn restored_job_identity_mismatch_is_failed_evidence_not_a_dropped_monitor() {
        let (state, pane_id, _directory) =
            state_with_one_terminal_pane("progress-monitor-restore-identity-mismatch").await;
        let mut persisted = persisted_running_progress_monitor(
            pane_id,
            r#"printf '%s' '{"job_id":"replacement-job","status":"running","percent":1,"message":"different process"}'"#.to_string(),
        );
        persisted.result_delivery = crate::persistence::PersistedProgressDeliveryState::Attempted;

        restore_persisted_progress_monitor(&state, persisted)
            .await
            .expect("an identity mismatch becomes sticky evidence");
        let progress = state
            .tree
            .read()
            .await
            .pane_progress(pane_id)
            .cloned()
            .expect("failed restore evidence remains visible");
        assert!(matches!(
            progress.monitor_health,
            ilium_core::ProgressMonitorHealth::Failed { ref last_error, .. }
                if last_error.contains("identity could not be restored")
                    && last_error.contains("replacement-job")
                    && last_error.contains("persisted-job")
        ));
        assert_eq!(progress.report.job_id, "persisted-job");
        let delivery = {
            let panes = state.panes.read().await;
            let PaneResource::Terminal(runtime) = panes.get(&pane_id).unwrap() else {
                panic!("fixture pane must remain terminal");
            };
            runtime.progress_monitor.as_ref().unwrap().result_delivery
        };
        assert_eq!(
            delivery,
            crate::pane::ProgressDeliveryState::Attempted,
            "restore must not replay a notification whose prior attempt is uncertain"
        );

        teardown_state_panes(&state);
    }

    #[tokio::test]
    async fn clear_pane_progress_monitor_stops_the_loop_and_clears_reported_progress() {
        let (state, pane_id, _directory) =
            state_with_one_terminal_pane("progress-monitor-clear-stops-loop").await;
        let mut events = state.events.subscribe();
        let (direct_tx, mut direct_rx) = mpsc::channel(1);

        handle_request(
            &state,
            ClientRequest::SetPaneProgressMonitor {
                request_id: 21,
                pane_id,
                command: r#"printf '%s' '{"job_id":"render-21","status":"running","percent":10,"message":"starting"}'"#.to_string(),
                interval_seconds: 1,
            },
            &direct_tx,
        )
        .await;
        let monitor_id = match direct_rx
            .recv()
            .await
            .expect("registration acknowledgement")
        {
            ServerEvent::ProgressMonitorSetCompleted {
                result: Ok(accepted),
                ..
            } => accepted.monitor_id,
            event => panic!("unexpected registration response: {event:?}"),
        };
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Ok(ServerEvent::PaneProgressChanged {
                    progress: Some(_), ..
                }) = events.recv().await
                {
                    break;
                }
            }
        })
        .await
        .expect("the monitor's first tick should land before it is cleared");

        handle_request(
            &state,
            ClientRequest::ClearPaneProgressMonitor {
                request_id: 22,
                pane_id,
                expected_monitor_id: Some(monitor_id),
            },
            &direct_tx,
        )
        .await;
        assert!(matches!(
            direct_rx.recv().await,
            Some(ServerEvent::ProgressMonitorCleared {
                request_id: 22,
                result: Ok(Some(id)),
                ..
            }) if id == monitor_id
        ));

        let cleared = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if let Ok(ServerEvent::PaneProgressChanged {
                    pane_id: event_pane_id,
                    progress: None,
                }) = events.recv().await
                {
                    if event_pane_id == pane_id {
                        break true;
                    }
                }
            }
        })
        .await
        .unwrap_or(false);
        assert!(cleared, "clearing must broadcast a None progress event");
        assert_eq!(state.tree.read().await.pane_progress(pane_id), None);

        // The loop must actually have stopped, not merely have its last
        // report cleared -- give it several intervals' worth of time and
        // confirm no further report arrives.
        let resurfaced = tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if let Ok(ServerEvent::PaneProgressChanged {
                    progress: Some(_), ..
                }) = events.recv().await
                {
                    return true;
                }
            }
        })
        .await
        .unwrap_or(false);
        assert!(
            !resurfaced,
            "a cleared monitor must not keep reporting afterward"
        );

        teardown_state_panes(&state);
    }

    #[tokio::test]
    async fn set_pane_progress_monitor_rejects_an_empty_command_and_a_non_terminal_pane() {
        let (state, pane_id, _directory) =
            state_with_one_terminal_pane("progress-monitor-rejects-bad-requests").await;
        let (direct_tx, mut direct_rx) = mpsc::channel(1);

        handle_request(
            &state,
            ClientRequest::SetPaneProgressMonitor {
                request_id: 31,
                pane_id,
                command: "   ".to_string(),
                interval_seconds: 1,
            },
            &direct_tx,
        )
        .await;
        assert!(
            matches!(
                direct_rx.try_recv(),
                Ok(ServerEvent::ProgressMonitorSetCompleted {
                    request_id: 31,
                    result: Err(_),
                    ..
                })
            ),
            "an empty command must be rejected with a direct error"
        );

        let missing_pane_id = NodeId(999_999);
        handle_request(
            &state,
            ClientRequest::SetPaneProgressMonitor {
                request_id: 32,
                pane_id: missing_pane_id,
                command: "true".to_string(),
                interval_seconds: 1,
            },
            &direct_tx,
        )
        .await;
        assert!(
            matches!(
                direct_rx.try_recv(),
                Ok(ServerEvent::ProgressMonitorSetCompleted {
                    request_id: 32,
                    result: Err(_),
                    ..
                })
            ),
            "a nonexistent pane must be rejected with a direct error"
        );

        teardown_state_panes(&state);
    }

    #[tokio::test]
    async fn disabling_progress_monitor_setting_rejects_new_requests_and_stops_running_ones() {
        let (state, pane_id, _directory) =
            state_with_one_terminal_pane("progress-monitor-disable-setting").await;
        let mut events = state.events.subscribe();
        let (direct_tx, mut direct_rx) = mpsc::channel(1);

        handle_request(
            &state,
            ClientRequest::SetPaneProgressMonitor {
                request_id: 41,
                pane_id,
                command: r#"printf '%s' '{"job_id":"render-41","status":"running","percent":5,"message":"running"}'"#.to_string(),
                interval_seconds: 1,
            },
            &direct_tx,
        )
        .await;
        assert!(matches!(
            direct_rx.recv().await,
            Some(ServerEvent::ProgressMonitorSetCompleted {
                request_id: 41,
                result: Ok(_),
                ..
            })
        ));
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Ok(ServerEvent::PaneProgressChanged {
                    progress: Some(_), ..
                }) = events.recv().await
                {
                    break;
                }
            }
        })
        .await
        .expect("the monitor's first tick should land before the setting is disabled");

        handle_request(
            &state,
            ClientRequest::UpdateProgressMonitorEnabled { enabled: false },
            &direct_tx,
        )
        .await;

        let cleared = tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                match events.recv().await {
                    Ok(ServerEvent::ProgressMonitorEnabledChanged { enabled: false }) => {}
                    Ok(ServerEvent::PaneProgressChanged {
                        pane_id: event_pane_id,
                        progress: None,
                    }) if event_pane_id == pane_id => return true,
                    Ok(_) => {}
                    Err(error) => panic!("progress event stream closed: {error}"),
                }
            }
        })
        .await
        .unwrap_or(false);
        assert!(
            cleared,
            "disabling the setting must clear the running monitor's progress"
        );
        assert!(!state.is_progress_monitor_enabled());

        handle_request(
            &state,
            ClientRequest::SetPaneProgressMonitor {
                request_id: 42,
                pane_id,
                command: "true".to_string(),
                interval_seconds: 1,
            },
            &direct_tx,
        )
        .await;
        assert!(
            matches!(
                direct_rx.try_recv(),
                Ok(ServerEvent::ProgressMonitorSetCompleted {
                    request_id: 42,
                    result: Err(_),
                    ..
                })
            ),
            "a new request must be rejected while the setting is disabled"
        );

        teardown_state_panes(&state);
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
            socket_path: directory.path().join("test.sock"),
            detection_config: crate::config::DetectionConfig::default(),
            notifications_config: crate::config::NotificationsConfig::default(),
            sound_settings: ilium_sound::SoundSettings::default(),
            sound_requests,
            custom_signatures: Vec::new(),
            agent_debug_menu_enabled: false,
            progress_monitor_enabled: true,
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

        // The claim is that an up-to-date pane gets no *redundant* tail, not
        // that no pane can produce output ever again. A pane's process can emit
        // more between settling and this call -- on Windows especially, where
        // ConPTY writes its own setup sequences -- and resending those is
        // correct, so the test asserts what each event says rather than how
        // many arrived.
        let missing_tail = terminal_events
            .iter()
            .find(|event| matches!(event, ServerEvent::ScreenUpdate { pane_id, .. } if *pane_id == missing_pane_id))
            .unwrap_or_else(|| panic!("the pane behind by one frame should be sent its tail: {terminal_events:#?}"));
        let ServerEvent::ScreenUpdate {
            first_sequence,
            sequence,
            bytes,
            ..
        } = *missing_tail
        else {
            panic!("the tail found for the behind pane was not a screen update: {missing_tail:#?}");
        };
        assert_eq!(
            *first_sequence, missing_sequence,
            "the tail must begin at exactly the frame the client is missing, not earlier"
        );
        // At least, for the same reason the count is not asserted above: the
        // pane's process may legitimately have produced further frames since
        // it settled, and a tail that carries them too is still correct.
        assert!(
            *sequence >= missing_sequence,
            "the tail must reach at least the frame the client is missing, got {sequence}"
        );
        assert!(
            !bytes.is_empty(),
            "a tail with no bytes replays nothing: {missing_tail:#?}"
        );
        for event in &terminal_events {
            if let ServerEvent::ScreenUpdate {
                pane_id,
                first_sequence,
                ..
            } = event
            {
                if *pane_id == current_pane_id {
                    assert!(
                        *first_sequence > current_sequence,
                        "an up-to-date pane must only be sent output it has not already seen: \
                         {terminal_events:#?}"
                    );
                }
            }
        }
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
            socket_path: directory.path().join("test.sock"),
            detection_config: crate::config::DetectionConfig::default(),
            notifications_config: crate::config::NotificationsConfig::default(),
            sound_settings: ilium_sound::SoundSettings::default(),
            sound_requests,
            custom_signatures: Vec::new(),
            agent_debug_menu_enabled: false,
            progress_monitor_enabled: true,
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
            socket_path: directory.path().join("test.sock"),
            detection_config: crate::config::DetectionConfig::default(),
            notifications_config: crate::config::NotificationsConfig::default(),
            sound_settings: ilium_sound::SoundSettings::default(),
            sound_requests,
            custom_signatures: Vec::new(),
            agent_debug_menu_enabled: true,
            progress_monitor_enabled: true,
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

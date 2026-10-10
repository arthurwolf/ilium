//! Translates each `ilium_ipc::ClientRequest` variant into a mutation on
//! `ServerState`'s tree/pane registry, broadcasting the resulting
//! `ServerEvent` to every attached client for structural changes (tree
//! shape, pane status) or replying only to the requesting connection for
//! everything else (the initial `Attach` snapshot, request-specific
//! errors). See `crate::ipc::connection` for how the two reply channels
//! (`ServerState::events` broadcast vs. this connection's own `direct_tx`)
//! are wired together on the write side.

use ilium_core::animation_recommendation::RecommendedRestructurePlan;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use ilium_agent_debug::{
    AgentDebugEventDraft, AgentDebugEventKind, AgentDebugField, AgentDebugSeverity,
    AgentDebugSource, PaneResizeCause,
};
use ilium_core::{
    AgentProcessKey, AgentProvider, AgentRecovery, BuiltinAgentProvider, NodeId, NodeKind,
    PaneContentKind, PaneStatus, PaneTitleSource, PromptQueueDelivery, QueuedPrompt,
    RestructurePlan, ScheduledPaneInput, SessionIdentityTransitionRule, Tree, TreeError,
};
use ilium_ipc::{
    ClientRequest, NewPaneKind, NewPaneWorkingDirectory, PaneTitleObservation,
    PromptSubmissionSource, ServerEvent,
};
use ilium_platform::paths;
use ilium_pty::{
    OwnerStatus, PtyError, PtyInput, PtyOutputRecovery, PtyOutputRecoveryEstimate, ShutdownReason,
};
use tokio::sync::{mpsc, oneshot};

use crate::foreground_observation::{self, ProbeObservation, ProbeRequest};
use crate::ipc::DirectEventSender;
use crate::mouse::to_crossterm_event;
use crate::pane;
use crate::pane::{PaneResource, PaneSnapshotKind, TerminalOrigin};
use crate::state::{
    ProgressSetRequestIdentity, ProgressSetRequestOutcome, ProgressSetRequestRecord,
    ProgressSetResult, ServerState, MAXIMUM_CACHED_PROGRESS_SET_REQUESTS,
};
use crate::title_eligibility::{
    self, CollectedTitleEvidence, TitleEvidenceCandidate, TitleRuntimeSnapshot,
};

/// Limits each selected-pane replay write so the client can parse and publish
/// useful content before the full retained journal has crossed the socket.
const TERMINAL_RECOVERY_EVENT_MAX_BYTES: usize = 256 * 1024;

#[derive(Clone, Copy)]
enum ConnectionEventStorageClass {
    General,
    VisiblePaneRecovery,
}

impl ConnectionEventStorageClass {
    fn log_name(self) -> &'static str {
        match self {
            Self::General => "connection-event",
            Self::VisiblePaneRecovery => "visible-pane-recovery-prefix",
        }
    }
}

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
    direct_tx: &DirectEventSender,
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
        ClientRequest::DiscardTerminalDelivery { .. } => {
            // Intercepted by `ipc::connection`: delivery watermarks belong to
            // the connection writer.
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
            if state.text_trigger_config_path.get().is_some() {
                // Configured servers use the durable file as their authority.
                // A delayed client's payload must not replace a newer save.
                match crate::text_trigger_config::refresh(state).await {
                    Ok(snapshot) => send_direct(direct_tx, snapshot).await,
                    Err(message) => send_direct_error(direct_tx, message).await,
                }
                return false;
            }
            let settings =
                match crate::text_triggers::validate_in_worker(state, settings, None).await {
                    Ok(settings) => settings,
                    Err(message) => {
                        send_direct_error(direct_tx, message).await;
                        return false;
                    }
                };
            let crate::text_triggers::AcceptedCandidate {
                settings,
                retention,
                ..
            } = settings;
            let mut accepted = state.text_trigger_settings.write().await;
            accepted.settings = settings;
            accepted.revision = accepted.revision.saturating_add(1);
            accepted.retention = Some(retention);
            let settings = accepted.settings.clone();
            drop(accepted);
            state.broadcast(ServerEvent::TextTriggersChanged { settings });
            false
        }
        ClientRequest::UpdateAgentDetectionSettings {
            request_id,
            settings,
        } => {
            handle_update_agent_detection_settings(state, settings, request_id, direct_tx).await;
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
        ClientRequest::QueryWorkspaceInventory {
            request_id,
            project,
        } => {
            let result = tokio::time::timeout(
                std::time::Duration::from_secs(30),
                crate::workspace_prune::inventory(state, project),
            )
            .await
            .unwrap_or_else(|_| {
                Err("worktree inventory exceeded its read-only time budget".into())
            });
            let event = ServerEvent::WorkspaceInventoryReported {
                request_id,
                project,
                result,
            };
            let _ = tokio::time::timeout(std::time::Duration::from_secs(5), direct_tx.send(event))
                .await;
            false
        }
        ClientRequest::QueryWorkspaceCloseOffer {
            request_id,
            pane_id,
        } => {
            let can_offer = tokio::time::timeout(
                std::time::Duration::from_secs(30),
                crate::workspace_prune::can_offer_close(state, pane_id),
            )
            .await
            .unwrap_or(false);
            let event = ServerEvent::WorkspaceCloseOfferReported {
                request_id,
                pane_id,
                can_offer,
            };
            let _ = tokio::time::timeout(std::time::Duration::from_secs(5), direct_tx.send(event))
                .await;
            false
        }
        ClientRequest::PruneWorkspace {
            request_id,
            project,
            target,
            mode,
            branch_policy,
        } => {
            start_retained_workspace_prune(
                state,
                request_id,
                project,
                target,
                mode,
                branch_policy,
                direct_tx,
            )
            .await;
            false
        }
        ClientRequest::QueryRepoFacts {
            request_id,
            project,
        } => {
            let result = crate::workspace::repo_facts(state, project).await;
            let _ = direct_tx
                .send(ServerEvent::RepoFactsReported {
                    request_id,
                    project,
                    result,
                })
                .await;
            false
        }
        ClientRequest::CreateAgentInWorkspace {
            request_id,
            parent_group,
            provider,
            spec,
            initial_input,
        } => {
            // The connection reader must be free to observe EOF. A client
            // disconnect cancels only at safe coordinator boundaries; it
            // must never drop a future while Git has an in-flight mutation.
            let creation_state = Arc::clone(state);
            let reply_sender = direct_tx.clone();
            let (start_tx, start_rx) = oneshot::channel();
            let handle = tokio::spawn(async move {
                if start_rx.await.is_err() {
                    return;
                }
                let reply = crate::ipc::EventReply::Direct(&reply_sender);
                let result = crate::workspace::create_agent_in_workspace(
                    &creation_state,
                    crate::workspace::CreateAgentOptions {
                        request_id,
                        parent_group,
                        project_override: None,
                        provider,
                        spec,
                        initial_input,
                        wait_for_prompt: false,
                    },
                    Some(&reply),
                )
                .await;
                let event = match result {
                    Ok(pane_id) => {
                        let _ = creation_state.queue_full_git_status(pane_id);
                        ServerEvent::WorkspaceCreated {
                            request_id,
                            pane_id,
                        }
                    }
                    Err(error) => ServerEvent::WorkspaceCreateFailed { request_id, error },
                };
                // A stalled reader cannot hold this server-owned task forever.
                if tokio::time::timeout(std::time::Duration::from_secs(5), reply.send(event))
                    .await
                    .is_err()
                {
                    tracing::warn!(
                        request_id,
                        "workspace response could not be delivered promptly"
                    );
                }
            });
            if state.track_workspace_creation_task(handle) {
                let _ = start_tx.send(());
            } else {
                let _ = direct_tx
                    .send(ServerEvent::WorkspaceCreateFailed {
                        request_id,
                        error: "session is shutting down".into(),
                    })
                    .await;
            }
            false
        }
        ClientRequest::RefreshPaneGitStatus { pane_id } => {
            if !state.queue_full_git_status(pane_id) {
                send_direct_error(direct_tx, "Git status refresh queue is busy".to_string()).await;
            }
            false
        }
        ClientRequest::RemoveWorkspace {
            request_id,
            pane_id,
            force_path,
            remove_branch,
        } => {
            handle_workspace_removal(
                state,
                request_id,
                pane_id,
                force_path,
                remove_branch,
                direct_tx,
            )
            .await;
            false
        }
        ClientRequest::ClosePaneWithWorkspaceDisposition {
            request_id,
            pane_id,
            disposition,
        } => {
            if disposition == ilium_ipc::WorkspaceDisposition::Keep {
                crate::lifecycle_log::record_close_request(state, pane_id, "client_keep_workspace");
                handle_close_pane(state, pane_id, direct_tx).await;
            } else {
                handle_workspace_removal(
                    state,
                    request_id,
                    pane_id,
                    None,
                    disposition == ilium_ipc::WorkspaceDisposition::RemoveWorktreeAndBranch,
                    direct_tx,
                )
                .await;
            }
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
            crate::lifecycle_log::record_close_request(state, pane_id, "client");
            handle_close_pane(state, pane_id, direct_tx).await;
            false
        }
        ClientRequest::TerminatePaneProcess { pane_id } => {
            handle_terminate_pane_process(state, pane_id, direct_tx).await;
            false
        }
        ClientRequest::FreezePane {
            pane_id,
            resume_command,
        } => {
            handle_freeze_pane(state, pane_id, resume_command, direct_tx).await;
            false
        }
        ClientRequest::UnfreezePane { pane_id } => {
            handle_unfreeze_pane(state, pane_id, direct_tx).await;
            false
        }
        ClientRequest::UpdateAntigravityStatusline { generation, action } => {
            crate::antigravity_statusline_delivery::update_running_panes(
                state, generation, action, direct_tx,
            )
            .await;
            false
        }
        ClientRequest::ReplacePaneWithCommand {
            pane_id,
            command_line,
        } => {
            handle_replace_pane_with_command(state, pane_id, command_line, direct_tx, None, None)
                .await;
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
        ClientRequest::ReportAgentPromptFromTranscript {
            pane_id,
            expected_session_id,
            prompt_epoch,
            last_prompt,
        } => {
            handle_exact_agent_prompt_from_transcript(
                state,
                pane_id,
                &expected_session_id,
                &prompt_epoch,
                last_prompt,
            )
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
        ClientRequest::WaitPaneProgressMonitor {
            request_id,
            pane_id,
            monitor_id,
        } => {
            handle_wait_pane_progress_monitor(state, request_id, pane_id, monitor_id, direct_tx)
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
            expected_presentation_revision,
            expected_process_id,
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
                    expected_presentation_revision,
                    expected_process_id,
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
            handle_key_input(state, pane_id, &bytes, submission, false, None, direct_tx).await;
            false
        }
        ClientRequest::UserKeyInput {
            pane_id,
            bytes,
            submission,
            prompt_epoch,
        } => {
            handle_key_input(
                state,
                pane_id,
                &bytes,
                submission,
                true,
                prompt_epoch,
                direct_tx,
            )
            .await;
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
        ClientRequest::KillSession => match handle_kill_session(state).await {
            Ok(()) => true,
            Err(message) => {
                send_direct_error(direct_tx, message.to_string()).await;
                false
            }
        },
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
            match state.sound_requests.admit_settings(settings) {
                Ok(settings) => *state.sound_settings.write().await = settings,
                Err((reason, original)) => {
                    send_direct_error(
                        direct_tx,
                        format!("sound settings admission refused: {reason:?}"),
                    )
                    .await;
                    drop(original);
                }
            }
            false
        }
        ClientRequest::PreviewSound { source, file } => {
            let current = state.sound_settings.read().await;
            let settings = ilium_sound::SoundSettings {
                source,
                file,
                design: current.design.clone(),
                events: current.events,
            };
            drop(current);
            let settings = match state.sound_requests.admit_settings(settings) {
                Ok(settings) => settings,
                Err((reason, original)) => {
                    send_direct_error(
                        direct_tx,
                        format!("sound preview admission refused: {reason:?}"),
                    )
                    .await;
                    drop(original);
                    return false;
                }
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
        ClientRequest::PreviewSoundSettings { settings } => {
            let settings = match state.sound_requests.admit_settings(settings) {
                Ok(settings) => settings,
                Err((reason, original)) => {
                    send_direct_error(
                        direct_tx,
                        format!("sound preview admission refused: {reason:?}"),
                    )
                    .await;
                    drop(original);
                    return false;
                }
            };
            crate::sounds::enqueue_preview(
                state,
                crate::sounds::PlaybackRequest {
                    settings,
                    event: None,
                    pane_name: None,
                },
                direct_tx.clone(),
            )
            .await;
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
        ClientRequest::ApplyRestructurePlan {
            plan,
            title_observations,
        } => {
            handle_apply_restructure_plan(state, plan, &title_observations, direct_tx).await;
            false
        }
        ClientRequest::RevertLastRestructure => {
            handle_revert_last_restructure(state, direct_tx).await;
            false
        }
        ClientRequest::ApplyRecommendedProjectRestructurePlan {
            project_id,
            plan,
            inference_activity_revisions,
            title_observations,
        } => {
            handle_apply_recommended_project_restructure_plan(
                state,
                project_id,
                plan,
                &inference_activity_revisions,
                &title_observations,
                direct_tx,
            )
            .await;
            false
        }
        ClientRequest::ApplyProjectRestructurePlan {
            project_id,
            plan,
            inference_activity_revisions,
            title_observations,
        } => {
            handle_apply_project_restructure_plan(
                state,
                project_id,
                plan,
                &inference_activity_revisions,
                &title_observations,
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
            let _logging_transaction = state.debug_logging_transaction.lock().await;
            if !enabled {
                tracing::info!("server file logging disabled from Debug settings");
            }
            let applied = match ilium_logging::request_set_enabled(enabled) {
                Ok(receipt) => receipt.await,
                Err(error) => Err(error),
            };
            if let Err(error) = applied {
                // A stalled client's error delivery must not hold up another
                // connection's logging transition.
                drop(_logging_transaction);
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
                            status: PaneStatus::Agent(_),
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
    direct_tx: &DirectEventSender,
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
async fn send_direct(direct_tx: &DirectEventSender, event: ServerEvent) {
    // An error here only means this connection's writer task has already
    // ended (client disconnected mid-request); nothing left to do with the
    // reply.
    let _ = direct_tx.send(event).await;
}

pub(crate) async fn send_antigravity_statusline_completed(
    direct_tx: &DirectEventSender,
    generation: u64,
    result: Result<(), String>,
) {
    send_direct(
        direct_tx,
        ServerEvent::AntigravityStatuslineCompleted { generation, result },
    )
    .await;
}

async fn send_direct_error(direct_tx: &DirectEventSender, message: impl Into<String>) {
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
    state
        .workspace_git_status_cache
        .write()
        .await
        .retain(|pane_id, _| snapshot.pane_workspace(*pane_id).is_some());
    // Takes its own fresh tree read rather than reusing `snapshot` above --
    // see `ServerState::prune_stale_restructure_undo`'s doc comment for why
    // that snapshot, taken moments earlier, is not safe to reuse here.
    state.prune_stale_restructure_undo().await;
    state.broadcast(ServerEvent::TreeSnapshot(snapshot));
    state.request_snapshot_save();
}

/// Publishes a change confined to one pane (no node added, removed or moved)
/// as that pane's node alone, then schedules the same snapshot save as
/// [`broadcast_and_persist`]. With hundreds of panes a full `TreeSnapshot`
/// per title change made naming N agents cost O(N^2) bytes and decoding.
pub(crate) async fn broadcast_pane_and_persist(state: &ServerState, pane_id: NodeId) {
    let pane = state.tree.read().await.get(pane_id).cloned();
    match pane {
        Some(pane) if pane.is_pane() => {
            state.broadcast(ServerEvent::PaneNodeChanged(Box::new(pane)));
            state.request_snapshot_save();
        }
        // Closed meanwhile: whatever closed it already sent a snapshot.
        _ => broadcast_and_persist(state).await,
    }
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

/// A completed PTY write may outlive its pane. Revalidate the exact owner
/// while holding the final tree mutation lock (tree before panes), so a new
/// runtime at the same NodeId never receives stale activity bookkeeping.
async fn record_input_activity_if_current(
    state: &ServerState,
    pane_id: NodeId,
    input: &PtyInput,
) -> Result<bool, String> {
    let update = {
        let mut tree = state.tree.write().await;
        let panes = state.panes.read().await;
        let is_current = matches!(panes.get(&pane_id),
            Some(PaneResource::Terminal(runtime))
                if input.same_session(&runtime.session.input_handle()));
        if !is_current {
            return Ok(false);
        }
        tree.record_node_activity(pane_id)
            .map_err(|error| format!("could not record activity for {pane_id:?}: {error}"))?
    };
    publish_node_activity_update(state, pane_id, update);
    Ok(true)
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
    direct_tx: &DirectEventSender,
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
    direct_tx: &DirectEventSender,
) {
    let event_text = text.clone();
    let event_delivery = format!("{delivery:?}");
    let _transaction = state.prompt_queue_transaction.lock().await;
    let result = state.tree.write().await.enqueue_prompt(
        pane_id,
        QueuedPrompt {
            text,
            delivery,
            attempted_delivery: false,
        },
    );
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
    direct_tx: &DirectEventSender,
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
    direct_tx: &DirectEventSender,
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
    loop {
        match state.recovery.attach_status().await {
            crate::recovery::AttachStatus::Pending { pane_count } => {
                let snapshot = state.tree.read().await.clone();
                send_direct(
                    direct_tx,
                    ServerEvent::PaneStateSnapshot {
                        tree: snapshot,
                        detection_evidence: Vec::new(),
                    },
                )
                .await;
                let (detection, custom_signatures) =
                    state.agent_detection_settings_snapshot().await;
                send_direct(
                    direct_tx,
                    ServerEvent::AgentDetectionSettingsChanged {
                        request_id: None,
                        result: Ok(crate::config::agent_detection_settings(
                            &detection,
                            &custom_signatures,
                        )),
                    },
                )
                .await;
                let settings = state.text_trigger_settings.read().await.settings.clone();
                send_direct(direct_tx, ServerEvent::TextTriggersChanged { settings }).await;
                send_direct(
                    direct_tx,
                    ServerEvent::SessionRecoveryAvailable { pane_count },
                )
                .await;
                return;
            }
            crate::recovery::AttachStatus::Resolving(completion) => {
                if let Err(message) = crate::recovery::wait_for_result(completion).await {
                    send_direct_error(direct_tx, message.to_string()).await;
                    return;
                }
            }
            crate::recovery::AttachStatus::Failed(message) => {
                send_direct_error(direct_tx, message.to_string()).await;
                return;
            }
            crate::recovery::AttachStatus::Settled => break,
        }
    }

    send_initial_state(state, direct_tx, include_terminal_output).await;
}

/// Sends one ordered, complete client render-cache seed. Both normal attach
/// and post-recovery resolution use this exact path so the startup trigger
/// always observes the same state boundary.
async fn send_initial_state(
    state: &ServerState,
    direct_tx: &DirectEventSender,
    include_terminal_output: bool,
) {
    let synchronization = if include_terminal_output {
        TerminalOutputSynchronization::All
    } else {
        TerminalOutputSynchronization::None
    };
    let (events, producer_storage) =
        match admitted_state_synchronization_events(state, synchronization, true).await {
            Ok(batch) => batch,
            Err(error) => {
                send_direct_error(direct_tx, error).await;
                return;
            }
        };
    for event in events {
        let _ = direct_tx
            .send_with_storage(event, producer_storage.clone())
            .await;
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

struct TerminalOutputEstimate {
    input: PtyInput,
    estimate: PtyOutputRecoveryEstimate,
    after_sequence: u64,
}

impl TerminalOutputSynchronization<'_> {
    fn estimate_for(
        &self,
        pane_id: NodeId,
        session: &ilium_pty::PtySession,
    ) -> Option<TerminalOutputEstimate> {
        let (after_sequence, estimate) = match self {
            Self::All => (
                0,
                PtyOutputRecoveryEstimate::Replay(session.output_replay_estimate()),
            ),
            Self::None => return None,
            Self::RecoverAfter(delivered_sequences) => {
                let after_sequence = delivered_sequences
                    .get(&pane_id)
                    .copied()
                    .unwrap_or_default();
                (
                    after_sequence,
                    session.output_recovery_estimate_after(after_sequence)?,
                )
            }
        };
        Some(TerminalOutputEstimate {
            input: session.input_handle(),
            estimate,
            after_sequence,
        })
    }

    fn event_from_estimate(
        &self,
        pane_id: NodeId,
        session: &ilium_pty::PtySession,
        estimated: &TerminalOutputEstimate,
    ) -> Option<ServerEvent> {
        match self {
            Self::All => {
                let PtyOutputRecoveryEstimate::Replay(replay_estimate) = estimated.estimate else {
                    return None;
                };
                let replay = session.output_replay_through_if_retained(replay_estimate)?;
                Some(terminal_replay_event(pane_id, replay))
            }
            Self::None => None,
            Self::RecoverAfter(_) => match session
                .output_recovery_if_unchanged(estimated.after_sequence, estimated.estimate)?
            {
                PtyOutputRecovery::Delta(chunk) => Some(ServerEvent::ScreenUpdate {
                    pane_id,
                    first_sequence: estimated.after_sequence.saturating_add(1),
                    sequence: chunk.sequence,
                    bytes: chunk.bytes.to_vec(),
                }),
                PtyOutputRecovery::Replay(replay) => Some(terminal_replay_event(pane_id, replay)),
            },
        }
    }
}

impl TerminalOutputSynchronization<'_> {
    /// Builds the smallest event that makes one terminal parser current.
    #[cfg(test)]
    fn event_for(&self, pane_id: NodeId, session: &ilium_pty::PtySession) -> Option<ServerEvent> {
        match self {
            Self::All => Some(terminal_replay_event(pane_id, session.output_replay())),
            Self::None => None,
            Self::RecoverAfter(delivered_sequences) => {
                let after_sequence = delivered_sequences
                    .get(&pane_id)
                    .copied()
                    .unwrap_or_default();
                terminal_recovery_from_session(pane_id, session, after_sequence)
            }
        }
    }
}

/// Builds the complete attach stream. Startup alone gets the explicit
/// completion boundary used by automatic triggers.
#[cfg(test)]
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
#[cfg(test)]
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

pub(crate) async fn admitted_resynchronization_events(
    state: &ServerState,
    delivered_terminal_sequences: &HashMap<NodeId, u64>,
) -> Result<
    (
        Vec<ServerEvent>,
        Option<Arc<ilium_execution::StorageAdmission>>,
    ),
    String,
> {
    admitted_state_synchronization_events(
        state,
        TerminalOutputSynchronization::RecoverAfter(delivered_terminal_sequences),
        false,
    )
    .await
}

pub(crate) async fn admitted_terminal_recovery_prefix(
    state: &ServerState,
    pane_id: NodeId,
    after_sequence: u64,
    through_sequence: u64,
) -> Result<Option<(ServerEvent, Option<Arc<ilium_execution::StorageAdmission>>)>, String> {
    if through_sequence <= after_sequence {
        return Ok(None);
    }
    for _attempt in 0..8 {
        let (expected_input, estimate) = {
            let panes = state.panes.read().await;
            let Some(PaneResource::Terminal(runtime)) = panes.get(&pane_id) else {
                return Ok(None);
            };
            let Some(estimate) = runtime
                .session
                .output_recovery_estimate_after(after_sequence)
            else {
                return Ok(None);
            };
            (runtime.session.input_handle(), estimate)
        };
        let byte_len = match estimate {
            PtyOutputRecoveryEstimate::Delta { byte_len, .. } => byte_len,
            PtyOutputRecoveryEstimate::Replay(replay) => replay.byte_len,
        }
        .min(TERMINAL_RECOVERY_EVENT_MAX_BYTES);
        let producer_storage = reserve_connection_event_storage(
            state,
            byte_len,
            1,
            ConnectionEventStorageClass::VisiblePaneRecovery,
            Some(pane_id),
        )
        .await?;
        let event = {
            let panes = state.panes.read().await;
            let Some(PaneResource::Terminal(runtime)) = panes.get(&pane_id) else {
                return Ok(None);
            };
            if !expected_input.same_session(&runtime.session.input_handle()) {
                None
            } else {
                let recovery = runtime
                    .session
                    .output_recovery_prefix_through(
                        after_sequence,
                        through_sequence,
                        TERMINAL_RECOVERY_EVENT_MAX_BYTES,
                    )
                    .or_else(|| {
                        // Retention can advance past the captured target while
                        // other selected panes are being written. Rebase to
                        // the current retained journal instead of failing the
                        // whole connection for a stale watermark.
                        runtime.session.output_recovery_prefix_after(
                            after_sequence,
                            TERMINAL_RECOVERY_EVENT_MAX_BYTES,
                        )
                    });
                match recovery {
                    Some(PtyOutputRecovery::Delta(chunk)) => Some(ServerEvent::ScreenUpdate {
                        pane_id,
                        first_sequence: after_sequence.saturating_add(1),
                        sequence: chunk.sequence,
                        bytes: chunk.bytes.to_vec(),
                    }),
                    Some(PtyOutputRecovery::Replay(replay)) => {
                        Some(terminal_replay_event(pane_id, replay))
                    }
                    None => None,
                }
            }
        };
        if let Some(event) = event {
            return Ok(Some((event, producer_storage)));
        }
        drop(producer_storage);
    }
    Err("terminal output changed repeatedly; retry recovery".to_owned())
}

pub(crate) async fn admitted_terminal_recovery_event(
    state: &ServerState,
    pane_id: NodeId,
    after_sequence: u64,
) -> Result<Option<(ServerEvent, Option<Arc<ilium_execution::StorageAdmission>>)>, String> {
    for _attempt in 0..8 {
        let (expected_input, estimate) = {
            let panes = state.panes.read().await;
            let Some(PaneResource::Terminal(runtime)) = panes.get(&pane_id) else {
                return Ok(None);
            };
            let Some(estimate) = runtime
                .session
                .output_recovery_estimate_after(after_sequence)
            else {
                return Ok(None);
            };
            (runtime.session.input_handle(), estimate)
        };
        let byte_len = match estimate {
            PtyOutputRecoveryEstimate::Delta { byte_len, .. } => byte_len,
            PtyOutputRecoveryEstimate::Replay(replay) => replay.byte_len,
        };
        let producer_storage = reserve_connection_event_storage(
            state,
            byte_len,
            1,
            ConnectionEventStorageClass::General,
            Some(pane_id),
        )
        .await?;
        let event = {
            let panes = state.panes.read().await;
            let Some(PaneResource::Terminal(runtime)) = panes.get(&pane_id) else {
                return Ok(None);
            };
            if !expected_input.same_session(&runtime.session.input_handle()) {
                None
            } else {
                runtime
                    .session
                    .output_recovery_if_unchanged(after_sequence, estimate)
                    .map(|recovery| match recovery {
                        PtyOutputRecovery::Delta(chunk) => ServerEvent::ScreenUpdate {
                            pane_id,
                            first_sequence: after_sequence.saturating_add(1),
                            sequence: chunk.sequence,
                            bytes: chunk.bytes.to_vec(),
                        },
                        PtyOutputRecovery::Replay(replay) => terminal_replay_event(pane_id, replay),
                    })
            }
        };
        if let Some(event) = event {
            return Ok(Some((event, producer_storage)));
        }
        drop(producer_storage);
    }
    Err("terminal output changed repeatedly; retry recovery".to_owned())
}

pub(crate) async fn terminal_output_sequence(state: &ServerState, pane_id: NodeId) -> Option<u64> {
    let panes = state.panes.read().await;
    let PaneResource::Terminal(runtime) = panes.get(&pane_id)? else {
        return None;
    };
    Some(runtime.session.output_replay_estimate().through_sequence)
}

async fn reserve_connection_event_storage(
    state: &ServerState,
    payload_bytes: usize,
    event_count: usize,
    storage_class: ConnectionEventStorageClass,
    pane_id: Option<NodeId>,
) -> Result<Option<Arc<ilium_execution::StorageAdmission>>, String> {
    if event_count == 0 {
        return Ok(None);
    }
    // The prepared events and encoded frame coexist until socket flush.
    // Reserve both payload copies plus per-event metadata before materializing.
    let metadata = event_count
        .checked_mul(16 * 1024)
        .ok_or_else(|| "connection event admission size overflow".to_owned())?;
    let reservation_bytes = payload_bytes
        .checked_mul(2)
        .and_then(|bytes| bytes.checked_add(metadata))
        .ok_or_else(|| "connection event admission size overflow".to_owned())?;
    let execution = state
        .execution
        .get()
        .ok_or_else(|| "server execution is not initialized".to_owned())?;
    let reservation_bytes = reservation_bytes.max(1);
    let reservation_client = match storage_class {
        ConnectionEventStorageClass::General => &execution.client,
        ConnectionEventStorageClass::VisiblePaneRecovery => &execution.visible_recovery,
    };
    let waiting_started_at = std::time::Instant::now();
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        reservation_client.reserve_storage(reservation_bytes),
    )
    .await;
    let result = match result {
        Ok(result) => result,
        Err(_) => {
            let ledger = reservation_client.quota_group().snapshot();
            let waited_ms =
                u64::try_from(waiting_started_at.elapsed().as_millis()).unwrap_or(u64::MAX);
            tracing::warn!(
                admission_context = storage_class.log_name(),
                pane_id = ?pane_id,
                payload_bytes,
                event_count,
                reservation_bytes,
                waited_ms,
                worker_bytes = ledger.worker_bytes,
                worker_byte_limit = ledger.limits.worker_bytes,
                "connection event storage admission is still waiting"
            );
            reservation_client.reserve_storage(reservation_bytes).await
        }
    };
    result
        .map(Some)
        .map_err(|error| format!("connection event admission refused: {error:?}"))
}

async fn admitted_state_synchronization_events(
    state: &ServerState,
    synchronization: TerminalOutputSynchronization<'_>,
    include_initial_sync_complete: bool,
) -> Result<
    (
        Vec<ServerEvent>,
        Option<Arc<ilium_execution::StorageAdmission>>,
    ),
    String,
> {
    for _attempt in 0..8 {
        let (estimates, snapshot_bytes) = {
            let tree = state.tree.read().await;
            let panes = state.panes.read().await;
            let estimates = panes
                .iter()
                .filter_map(|(pane_id, resource)| match resource {
                    PaneResource::Terminal(runtime) => synchronization
                        .estimate_for(*pane_id, &runtime.session)
                        .map(|estimate| (*pane_id, estimate)),
                    PaneResource::Editor { .. } | PaneResource::Unrestored(_) => None,
                })
                .collect::<HashMap<_, _>>();
            let snapshot_bytes = crate::snapshot_io::estimated_tree_bytes(&tree)
                .checked_mul(2)
                .and_then(|bytes| {
                    // Detection evidence duplicates status and transcript-derived
                    // strings in the tree snapshot. Reserve a bounded allowance
                    // per pane before cloning either snapshot representation.
                    panes
                        .len()
                        .checked_mul(128 * 1024)
                        .and_then(|evidence| bytes.checked_add(evidence))
                });
            let Some(snapshot_bytes) = snapshot_bytes else {
                return Err("state snapshot admission size overflow".to_owned());
            };
            (estimates, snapshot_bytes)
        };
        let estimated_payload_bytes = estimates.values().try_fold(0usize, |total, estimate| {
            let bytes = match estimate.estimate {
                PtyOutputRecoveryEstimate::Delta { byte_len, .. } => byte_len,
                PtyOutputRecoveryEstimate::Replay(replay) => replay.byte_len,
            };
            total.checked_add(bytes)
        });
        let estimated_payload_bytes = estimated_payload_bytes
            .and_then(|bytes| bytes.checked_add(snapshot_bytes))
            .ok_or_else(|| "state synchronization admission size overflow".to_owned())?;
        let producer_storage = reserve_connection_event_storage(
            state,
            estimated_payload_bytes,
            estimates.len().saturating_add(1),
            ConnectionEventStorageClass::General,
            None,
        )
        .await?;

        // Revalidate the current tree's measured retained size against the
        // reservation before cloning the retry fence or the queued snapshot.
        // If it grew while admission waited, release and estimate again.
        let expected_tree = {
            let tree = state.tree.read().await;
            let current_tree_bytes = crate::snapshot_io::estimated_tree_bytes(&tree);
            if current_tree_bytes > snapshot_bytes / 2 {
                drop(tree);
                drop(producer_storage);
                continue;
            }
            tree.clone()
        };

        let built = {
            let tree = state.tree.read().await;
            if *tree != expected_tree {
                None
            } else {
                let panes = state.panes.read().await;
                let snapshot = tree.clone();
                let detection_evidence = panes
                    .iter()
                    .filter_map(|(pane_id, resource)| match resource {
                        PaneResource::Terminal(runtime) => runtime
                            .detection_evidence
                            .clone()
                            .map(|evidence| (*pane_id, evidence)),
                        PaneResource::Editor { .. } | PaneResource::Unrestored(_) => None,
                    })
                    .collect();
                let mut matched_estimates = HashSet::new();
                let mut replay_events = Vec::new();
                let mut stale = false;
                for (pane_id, resource) in panes.iter() {
                    match resource {
                        PaneResource::Terminal(runtime) => {
                            if matches!(runtime.origin, TerminalOrigin::Frozen { .. }) {
                                replay_events.push(ServerEvent::PaneFrozen {
                                    pane_id: *pane_id,
                                    result: Ok(()),
                                });
                            }
                            if let Some(estimate) = estimates.get(pane_id) {
                                if !estimate.input.same_session(&runtime.session.input_handle()) {
                                    stale = true;
                                    break;
                                }
                                matched_estimates.insert(*pane_id);
                                match synchronization.event_from_estimate(
                                    *pane_id,
                                    &runtime.session,
                                    estimate,
                                ) {
                                    Some(event) => replay_events.push(event),
                                    None => {
                                        stale = true;
                                        break;
                                    }
                                }
                            }
                            if let Some(session_id) = runtime.session_id.clone() {
                                replay_events.push(ServerEvent::PaneSessionIdResolved {
                                    pane_id: *pane_id,
                                    session_id,
                                    process_id: runtime.session_process_id,
                                    title_generation: runtime.title_generation,
                                    transcript_path: runtime.session_transcript_path.clone(),
                                });
                            }
                        }
                        PaneResource::Editor { path } => {
                            replay_events.push(ServerEvent::PaneEditorPathResolved {
                                pane_id: *pane_id,
                                path: path.clone(),
                            });
                        }
                        PaneResource::Unrestored(_) => {}
                    }
                }
                if stale || matched_estimates.len() != estimates.len() {
                    None
                } else {
                    let mut events = vec![ServerEvent::PaneStateSnapshot {
                        tree: snapshot,
                        detection_evidence,
                    }];
                    events.extend(replay_events);
                    drop(panes);
                    drop(tree);
                    let statuses = state.workspace_git_status_cache.read().await;
                    let status_replays: Vec<_> = match &events[0] {
                        ServerEvent::PaneStateSnapshot { tree, .. } => statuses
                            .iter()
                            .filter_map(|(pane_id, status)| {
                                tree.pane_workspace(*pane_id).map(|_| {
                                    ServerEvent::PaneGitStatusChanged {
                                        pane_id: *pane_id,
                                        status: status.clone(),
                                    }
                                })
                            })
                            .collect(),
                        _ => Vec::new(),
                    };
                    events.extend(status_replays);
                    drop(statuses);
                    let settings = state.text_trigger_settings.read().await.settings.clone();
                    events.push(ServerEvent::TextTriggersChanged { settings });
                    if include_initial_sync_complete {
                        let (detection, custom_signatures) =
                            state.agent_detection_settings_snapshot().await;
                        events.push(ServerEvent::AgentDetectionSettingsChanged {
                            request_id: None,
                            result: Ok(crate::config::agent_detection_settings(
                                &detection,
                                &custom_signatures,
                            )),
                        });
                        events.push(ServerEvent::InitialStateSyncComplete);
                    }
                    Some(events)
                }
            }
        };
        if let Some(events) = built {
            let retained_event_bytes = events.iter().try_fold(0usize, |total, event| {
                total.checked_add(event.retained_bytes())
            });
            let admitted_bytes = producer_storage
                .as_ref()
                .map_or(0, |storage| storage.resident_bytes());
            if retained_event_bytes.is_none_or(|bytes| bytes > admitted_bytes) {
                return Err("state synchronization exceeded byte admission".to_owned());
            }
            return Ok((events, producer_storage));
        }
        drop(producer_storage);
    }
    Err("state synchronization changed repeatedly; retry attach".to_owned())
}

/// Builds one ordered render-cache seed from current server authority.
#[cfg(test)]
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
    let (snapshot, detection_evidence, replay_events): (
        Tree,
        Vec<(NodeId, ilium_ipc::PaneDetectionEvidence)>,
        Vec<ServerEvent>,
    ) = {
        let tree = state.tree.read().await;
        let panes = state.panes.read().await;
        let snapshot = tree.clone();
        let detection_evidence = panes
            .iter()
            .filter_map(|(pane_id, resource)| match resource {
                PaneResource::Terminal(runtime) => runtime
                    .detection_evidence
                    .clone()
                    .map(|evidence| (*pane_id, evidence)),
                PaneResource::Editor { .. } | PaneResource::Unrestored(_) => None,
            })
            .collect();
        let replay_events = panes
            .iter()
            .flat_map(|(pane_id, resource)| match resource {
                PaneResource::Terminal(runtime) => {
                    let mut events = Vec::new();
                    if matches!(runtime.origin, TerminalOrigin::Frozen { .. }) {
                        events.push(ServerEvent::PaneFrozen {
                            pane_id: *pane_id,
                            result: Ok(()),
                        });
                    }
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
                            transcript_path: runtime.session_transcript_path.clone(),
                        });
                    }
                    events
                }
                PaneResource::Editor { path } => vec![ServerEvent::PaneEditorPathResolved {
                    pane_id: *pane_id,
                    path: path.clone(),
                }],
                PaneResource::Unrestored(_) => Vec::new(),
            })
            .collect();
        (snapshot, detection_evidence, replay_events)
    };
    let mut events = vec![ServerEvent::PaneStateSnapshot {
        tree: snapshot,
        detection_evidence,
    }];
    events.extend(replay_events);
    let statuses = state.workspace_git_status_cache.read().await;
    let status_replays: Vec<_> = match &events[0] {
        ServerEvent::PaneStateSnapshot { tree, .. } => statuses
            .iter()
            .filter_map(|(pane_id, status)| {
                tree.pane_workspace(*pane_id)
                    .map(|_| ServerEvent::PaneGitStatusChanged {
                        pane_id: *pane_id,
                        status: status.clone(),
                    })
            })
            .collect(),
        _ => Vec::new(),
    };
    events.extend(status_replays);
    drop(statuses);
    // Rules are server-owned live state, so an attachment or lag recovery
    // needs them even when no client has changed the list during this stream.
    let settings = state.text_trigger_settings.read().await.settings.clone();
    events.push(ServerEvent::TextTriggersChanged { settings });
    if include_initial_sync_complete {
        let (detection, custom_signatures) = state.agent_detection_settings_snapshot().await;
        events.push(ServerEvent::AgentDetectionSettingsChanged {
            request_id: None,
            result: Ok(crate::config::agent_detection_settings(
                &detection,
                &custom_signatures,
            )),
        });
        events.push(ServerEvent::InitialStateSyncComplete);
    }
    events
}

pub(crate) async fn handle_session_recovery_resolution(
    state: &Arc<ServerState>,
    restore: bool,
    direct_tx: &DirectEventSender,
) {
    let completion = match state.recovery.admit(Arc::clone(state), restore).await {
        Ok(completion) => completion,
        Err(refusal) => {
            send_direct_error(direct_tx, refusal.to_string()).await;
            return;
        }
    };
    if let Err(message) = crate::recovery::wait_for_result(completion).await {
        send_direct_error(direct_tx, message.to_string()).await;
        return;
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

/// Shared plumbing for tree-only mutations, including project path changes:
/// apply `mutate` under the tree write lock, and on success
/// broadcast the resulting snapshot to every client and persist a
/// crash-recovery snapshot; on failure, reply only to the requester with
/// the `TreeError`.
async fn handle_tree_mutation(
    state: &Arc<ServerState>,
    direct_tx: &DirectEventSender,
    mutate: impl FnOnce(&mut Tree) -> Result<(), TreeError>,
) {
    // A new or moved project root can protect a worktree from pruning.
    let publish_guard = state.workspace_spawn_lock.lock().await;
    let mut tree = state.tree.write().await;
    let result = mutate(&mut tree);
    // Drop the write guard before doing anything else -- in particular,
    // before the broadcast snapshot's own O(n) clone (see
    // `broadcast_and_persist`), so this write lock is only ever held for
    // the mutation itself.
    drop(tree);
    drop(publish_guard);
    match result {
        Ok(()) => broadcast_and_persist(state).await,
        Err(error) => send_direct_error(direct_tx, format!("tree operation failed: {error}")).await,
    }
}

async fn handle_new_project(
    state: &Arc<ServerState>,
    path: std::path::PathBuf,
    direct_tx: &DirectEventSender,
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
    direct_tx: &DirectEventSender,
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
async fn collect_observed_title_evidence(
    state: &ServerState,
    observations: &[PaneTitleObservation],
) -> title_eligibility::CollectedTitleEvidenceBatch {
    let candidates = {
        let tree = state.tree.read().await;
        let panes = state.panes.read().await;
        observations
            .iter()
            .filter_map(|observation| {
                let node = tree.get(observation.pane_id)?;
                let project_cwd = tree.pane_cwd(node.id).map(std::path::Path::to_path_buf)?;
                let runtime = match panes.get(&node.id) {
                    Some(PaneResource::Terminal(runtime)) => {
                        Some(TitleRuntimeSnapshot::capture(node, runtime, false))
                    }
                    _ => None,
                };
                Some(TitleEvidenceCandidate {
                    observation: observation.clone(),
                    project_cwd,
                    runtime,
                })
            })
            .collect()
    };
    title_eligibility::collect_title_evidence(
        state.execution.get().map(|execution| &execution.client),
        state.home_dir.clone(),
        candidates,
    )
    .await
}

/// Resets legacy AI text only when the exact current conversation is proven
/// empty. An unavailable history is never a destructive repair signal.
pub(crate) async fn reconcile_empty_agent_titles(state: &ServerState, pane_ids: &[NodeId]) {
    let observations = {
        let tree = state.tree.read().await;
        let panes = state.panes.read().await;
        pane_ids
            .iter()
            .filter_map(|id| {
                let node = tree.get(*id)?;
                let Some(PaneResource::Terminal(runtime)) = panes.get(id) else {
                    return None;
                };
                Some(TitleRuntimeSnapshot::capture(node, runtime, false).observation)
            })
            .collect::<Vec<_>>()
    };
    let evidence = collect_observed_title_evidence(state, &observations).await;
    let changed = {
        let mut tree = state.tree.write().await;
        let panes = state.panes.read().await;
        let mut changed = false;
        for evidence in evidence.iter() {
            let id = evidence.observation().pane_id;
            let Some(node) = tree.get(id) else {
                continue;
            };
            let Some(PaneResource::Terminal(runtime)) = panes.get(&id) else {
                continue;
            };
            let current = TitleRuntimeSnapshot::capture(node, runtime, false);
            if !evidence.proves_current_empty_history(node, &current)
                || runtime
                    .authored_title_receipt
                    .as_ref()
                    .is_some_and(|receipt| receipt.matches(&current))
            {
                continue;
            }
            let repaired = tree
                .repair_legacy_restructure_title_source(id)
                .unwrap_or(false);
            let reset = tree
                .set_automatic_pane_title(id, pane::FRESH_AGENT_TITLE, None, None)
                .unwrap_or(false);
            changed |= repaired || reset;
        }
        changed
    };
    if changed {
        broadcast_and_persist(state).await;
    }
}

fn title_grants_under_lock(
    tree: &Tree,
    panes: &HashMap<NodeId, PaneResource>,
    evidence: &[CollectedTitleEvidence],
    shell_observations: &HashMap<NodeId, ProbeObservation>,
) -> Vec<ilium_core::NodePresentationRevision> {
    evidence
        .iter()
        .filter_map(|evidence| {
            let node = tree.get(evidence.observation().pane_id)?;
            let runtime = match panes.get(&node.id) {
                Some(PaneResource::Terminal(runtime)) => Some(runtime),
                _ => None,
            };
            let snapshot = runtime.map(|runtime| {
                let shell_confirmed = shell_observations
                    .get(&node.id)
                    .filter(|observed| observed.same_session(runtime))
                    .is_some_and(|observed| observed.shell_owns_terminal() == Some(true));
                TitleRuntimeSnapshot::capture(node, runtime, shell_confirmed)
            });
            title_eligibility::accepted_title_grant(
                node,
                snapshot.as_ref(),
                evidence,
                runtime.and_then(|runtime| runtime.authored_title_receipt.as_ref()),
            )
        })
        .collect()
}

/// Native foreground inspection runs after evidence collection and before
/// the title transaction. Unknown, refused, or late observations grant no
/// plain-shell title. The final locked snapshot still fences the PTY lifetime,
/// session, agent generation, process birth, and presentation revision.
async fn observe_title_shells(
    state: &ServerState,
    evidence: &[CollectedTitleEvidence],
) -> HashMap<NodeId, ProbeObservation> {
    let requests = {
        let panes = state.panes.read().await;
        evidence
            .iter()
            .filter(|item| item.needs_shell_confirmation())
            .filter_map(|item| {
                let pane_id = item.observation().pane_id;
                let Some(PaneResource::Terminal(runtime)) = panes.get(&pane_id) else {
                    return None;
                };
                matches!(&runtime.origin, TerminalOrigin::PlainShell).then(|| {
                    (
                        pane_id,
                        ProbeRequest::for_shell(runtime.session.shell_observer()),
                    )
                })
            })
            .collect::<Vec<_>>()
    };
    let mut observed = HashMap::new();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(750);
    for (pane_id, request) in requests {
        let result =
            tokio::time::timeout_at(deadline, foreground_observation::observe(state, request))
                .await;
        match result {
            Ok(Ok(proof)) => {
                observed.insert(pane_id, proof);
            }
            Ok(Err(error)) => {
                tracing::debug!(pane_id = pane_id.0, %error, "title foreground unavailable")
            }
            Err(_) => break,
        }
    }
    observed
}

async fn commit_project_restructure(
    state: &ServerState,
    project_id: NodeId,
    evidence: &[CollectedTitleEvidence],
    apply: impl FnOnce(
        &mut Tree,
        &[ilium_core::NodePresentationRevision],
    ) -> Result<Vec<ilium_core::NodeActivityRevision>, TreeError>,
) -> Result<Vec<ilium_core::NodeActivityRevision>, TreeError> {
    let shell_observations = observe_title_shells(state, evidence).await;
    let mut tree = state.tree.write().await;
    let panes = state.panes.read().await;
    let grants = title_grants_under_lock(&tree, &panes, evidence, &shell_observations);
    let mut undo = state.restructure_undo.lock().await;
    let mut title_undo = state.restructure_title_revisions.lock().await;
    let before = tree.clone();
    let checkpoints = apply(&mut tree, &grants)?;
    let accepted_revisions = tree.project_presentation_revisions(project_id)?;
    undo.insert(project_id, before);
    title_undo.insert(project_id, accepted_revisions);
    state.request_snapshot_save();
    Ok(checkpoints)
}
async fn project_restructure_event(
    state: &ServerState,
    project_id: NodeId,
    result: Result<Vec<ilium_core::NodeActivityRevision>, TreeError>,
) -> ServerEvent {
    match result {
        Ok(checkpoint_activity_revisions) => {
            broadcast_and_persist(state).await;
            ServerEvent::ProjectRestructureApplied {
                project_id,
                checkpoint_activity_revisions,
            }
        }
        Err(error) => {
            let message = format!("restructure failed: {error}");
            tracing::error!(%message, "request failed");
            ServerEvent::ProjectRestructureRejected {
                project_id,
                message,
            }
        }
    }
}

async fn handle_apply_restructure_plan(
    state: &Arc<ServerState>,
    plan: RestructurePlan,
    title_observations: &[PaneTitleObservation],
    direct_tx: &DirectEventSender,
) {
    let transaction = state.restructure_transaction.lock().await;
    let evidence = collect_observed_title_evidence(state, title_observations).await;
    let projects = state.tree.read().await.project_ids();
    if projects.len() != 1 {
        drop(transaction);
        send_direct_error(
            direct_tx,
            "restructure failed: select the single project to restructure",
        )
        .await;
        return;
    }
    let project_id = projects[0];
    let result = commit_project_restructure(state, project_id, &evidence, |tree, grants| {
        if tree.project_ids().len() != 1 {
            return Err(TreeError::RootRequiresProject);
        }
        tree.apply_project_restructure_with_title_grants(project_id, plan, &[], grants)?;
        Ok(Vec::new())
    })
    .await;
    if result.is_ok() {
        broadcast_and_persist(state).await;
    }
    drop(transaction);
    if let Err(error) = result {
        send_direct_error(direct_tx, format!("restructure failed: {error}")).await;
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
async fn handle_revert_last_restructure(state: &Arc<ServerState>, direct_tx: &DirectEventSender) {
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
    title_observations: &[PaneTitleObservation],
    direct_tx: &DirectEventSender,
) {
    let transaction = state.restructure_transaction.lock().await;
    let evidence = collect_observed_title_evidence(state, title_observations).await;
    let result = commit_project_restructure(state, project_id, &evidence, |tree, grants| {
        tree.apply_project_restructure_with_title_grants(
            project_id,
            plan,
            inference_activity_revisions,
            grants,
        )
    })
    .await;
    let event = project_restructure_event(state, project_id, result).await;
    drop(transaction);
    send_direct(direct_tx, event).await;
}

async fn handle_apply_recommended_project_restructure_plan(
    state: &Arc<ServerState>,
    project_id: NodeId,
    plan: RecommendedRestructurePlan,
    inference_activity_revisions: &[ilium_core::NodeActivityRevision],
    title_observations: &[PaneTitleObservation],
    direct_tx: &DirectEventSender,
) {
    let transaction = state.restructure_transaction.lock().await;
    let evidence = collect_observed_title_evidence(state, title_observations).await;
    let result = commit_project_restructure(state, project_id, &evidence, |tree, grants| {
        tree.apply_recommended_project_restructure_with_title_grants(
            project_id,
            plan,
            inference_activity_revisions,
            grants,
        )
    })
    .await;
    let event = project_restructure_event(state, project_id, result).await;
    drop(transaction);
    send_direct(direct_tx, event).await;
}

async fn handle_revert_project_restructure(
    state: &Arc<ServerState>,
    project_id: NodeId,
    direct_tx: &DirectEventSender,
) {
    let transaction = state.restructure_transaction.lock().await;
    let publish_guard = state.workspace_spawn_lock.lock().await;
    let mut tree = state.tree.write().await;
    let mut undo = state.restructure_undo.lock().await;
    let Some(previous_tree) = undo.get(&project_id) else {
        drop(undo);
        drop(tree);
        drop(publish_guard);
        drop(transaction);
        send_direct_error(direct_tx, "no restructure to revert for this project").await;
        return;
    };
    let orphaned_pane_ids: Vec<NodeId> = collect_pane_descendants(&tree, project_id)
        .into_iter()
        .filter(|pane_id| previous_tree.get(*pane_id).is_none())
        .collect();
    // Acquire every resource guard before publishing the restore. A cancelled
    // connection cannot remove tree nodes while their live resources remain.
    let mut panes = if orphaned_pane_ids.is_empty() {
        None
    } else {
        Some(state.panes.write().await)
    };
    let mut title_undo = state.restructure_title_revisions.lock().await;
    let preserve_ids = title_undo
        .get(&project_id)
        .into_iter()
        .flatten()
        .filter_map(|accepted| {
            tree.get(accepted.node_id)
                .filter(|node| node.presentation_revision != accepted.revision)
                .map(|node| node.id)
        })
        .collect::<Vec<_>>();
    if let Err(error) =
        tree.restore_project_from_preserving_presentations(project_id, previous_tree, &preserve_ids)
    {
        drop(title_undo);
        drop(panes);
        drop(undo);
        drop(tree);
        drop(publish_guard);
        drop(transaction);
        send_direct_error(direct_tx, format!("could not revert restructure: {error}")).await;
        return;
    }
    undo.remove(&project_id);
    title_undo.remove(&project_id);
    drop(title_undo);
    state.request_snapshot_save();
    drop(undo);
    if let Some(panes) = &mut panes {
        for pane_id in &orphaned_pane_ids {
            if let Some(resource) = panes.remove(pane_id) {
                crate::lifecycle_log::record(
                    state,
                    crate::lifecycle_log::LifecycleEvent::PaneClosed {
                        pane_id: pane_id.0,
                        reason: "restructure_revert",
                        resource: crate::lifecycle_log::describe_resource(&resource),
                    },
                );
                teardown_pane_resource(*pane_id, resource);
            }
        }
    }
    drop(panes);
    drop(tree);
    drop(publish_guard);
    if !orphaned_pane_ids.is_empty() {
        state.agent_debug.remove(&orphaned_pane_ids).await;
        let mut preferences = state.workspace_close_preferences.write().await;
        for pane_id in &orphaned_pane_ids {
            preferences.remove(pane_id);
        }
    }
    broadcast_and_persist(state).await;
    drop(transaction);
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
    handle_automatic_pane_title_with_probe(
        state,
        pane_id,
        title,
        short_title,
        inferred_icon,
        |state, request| async move { foreground_observation::observe(&state, request).await },
    )
    .await;
}

async fn handle_automatic_pane_title_with_probe<Probe, ProbeFuture>(
    state: &Arc<ServerState>,
    pane_id: NodeId,
    title: String,
    short_title: Option<String>,
    inferred_icon: Option<String>,
    probe: Probe,
) where
    Probe: FnOnce(Arc<ServerState>, ProbeRequest) -> ProbeFuture,
    ProbeFuture:
        std::future::Future<Output = Result<ProbeObservation, foreground_observation::ProbeError>>,
{
    let (baseline, request) = {
        let tree = state.tree.read().await;
        let panes = state.panes.read().await;
        let Some(node) = tree.get(pane_id) else {
            return;
        };
        let Some(PaneResource::Terminal(runtime)) = panes.get(&pane_id) else {
            return;
        };
        (
            TitleRuntimeSnapshot::capture(node, runtime, false),
            ProbeRequest::for_shell(runtime.session.shell_observer()),
        )
    };
    let shell_observation = probe(Arc::clone(state), request).await.ok();
    let tree_changed = {
        let mut tree = state.tree.write().await;
        let panes = state.panes.read().await;
        let Some(node) = tree.get(pane_id) else {
            return;
        };
        let Some(PaneResource::Terminal(runtime)) = panes.get(&pane_id) else {
            return;
        };
        if TitleRuntimeSnapshot::capture(node, runtime, false) != baseline {
            return;
        }
        let snapshot = TitleRuntimeSnapshot::capture(
            node,
            runtime,
            shell_observation
                .as_ref()
                .filter(|proof| proof.same_session(runtime))
                .is_some_and(|proof| proof.shell_owns_terminal() == Some(true)),
        );
        if snapshot.kind != title_eligibility::TerminalTitleKind::ConfirmedPlainShell {
            return;
        }
        match tree.set_automatic_pane_title(pane_id, title, short_title, inferred_icon) {
            Ok(changed) => changed,
            Err(error) => {
                tracing::warn!("automatic title update rejected for pane {pane_id:?}: {error}");
                false
            }
        }
    };
    if tree_changed {
        broadcast_pane_and_persist(state, pane_id).await;
    }
}

/// Applies an LLM title as a compare-and-set against the server's live
/// session identity. A stale client or in-flight worker can never title the
/// replacement session, regardless of IPC event/request ordering.
struct SessionPaneTitleUpdate<'a> {
    pane_id: NodeId,
    expected_session_id: &'a str,
    expected_title_generation: u64,
    expected_presentation_revision: u64,
    expected_process_id: Option<u32>,
    title: String,
    short_title: Option<String>,
    inferred_icon: Option<String>,
    title_source: PaneTitleSource,
}

async fn handle_session_pane_title(state: &Arc<ServerState>, update: SessionPaneTitleUpdate<'_>) {
    let observation = {
        let tree = state.tree.read().await;
        let panes = state.panes.read().await;
        let Some(node) = tree.get(update.pane_id) else {
            return;
        };
        let Some(PaneResource::Terminal(runtime)) = panes.get(&update.pane_id) else {
            return;
        };
        let mut observation = TitleRuntimeSnapshot::capture(node, runtime, false).observation;
        observation.presentation_revision = update.expected_presentation_revision;
        observation.session_id = Some(update.expected_session_id.to_owned());
        observation.process_id = update.expected_process_id;
        observation.title_generation = update.expected_title_generation;
        observation
    };
    let evidence = collect_observed_title_evidence(state, &[observation]).await;
    let shell_observations = observe_title_shells(state, &evidence).await;
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
        || title_grants_under_lock(&tree, &panes, &evidence, &shell_observations).is_empty()
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
    let changed = tree
        .accept_session_pane_title(
            update.pane_id,
            update.expected_presentation_revision,
            update.title,
            update.short_title,
            update.inferred_icon,
            update.title_source,
        )
        .unwrap_or_else(|error| {
            tracing::warn!(pane_id = update.pane_id.0, %error, "session title rejected");
            false
        });
    drop(panes);
    drop(tree);
    if changed {
        broadcast_pane_and_persist(state, update.pane_id).await;
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

/// Compatibility request without a submission token: bootstrap a wholly
/// empty prompt only. It cannot replace any input observed since attach.
async fn handle_last_prompt_from_transcript(
    state: &Arc<ServerState>,
    pane_id: NodeId,
    expected_session_id: &str,
    last_prompt: String,
) {
    if last_prompt.is_empty() {
        return;
    }
    let mut tree = state.tree.write().await;
    let mut panes = state.panes.write().await;
    let Some(PaneResource::Terminal(runtime)) = panes.get_mut(&pane_id) else {
        return;
    };
    let Some(owner) = runtime.agent_process_key.as_ref() else {
        return;
    };
    if runtime.is_session_identity_invalidated
        || runtime.session_id.as_deref() != Some(expected_session_id)
        || runtime.session_agent_class.as_ref() != Some(&owner.class)
        || runtime.session_process_id != Some(owner.process_id)
        || runtime.session_process_started_at_unix_seconds != Some(owner.started_at_unix_seconds)
        || runtime.legacy_prompt_fallback_blocked
        || runtime.last_agent_prompt.is_some()
        || tree.last_prompt(pane_id).is_some()
    {
        return;
    }
    runtime.last_agent_prompt = Some(last_prompt.clone());
    update_unavailable_recovery_prompt(
        &mut tree,
        pane_id,
        owner,
        Some(&last_prompt),
        None,
        false,
        state,
    );
    if tree
        .set_last_prompt(pane_id, Some(last_prompt.clone()))
        .is_ok()
    {
        state.request_snapshot_save();
        state.broadcast(ServerEvent::PaneLastPromptChanged {
            pane_id,
            last_prompt: Some(last_prompt),
        });
    }
}

/// Exact provider text is a correction to one confirmed physical Enter.
/// The epoch was bound to this invocation only after PTY delivery; a later
/// successful submission or unattributed byte batch revokes it.
async fn handle_exact_agent_prompt_from_transcript(
    state: &Arc<ServerState>,
    pane_id: NodeId,
    expected_session_id: &str,
    prompt_epoch: &str,
    last_prompt: String,
) {
    if last_prompt.is_empty() {
        return;
    }
    let mut tree = state.tree.write().await;
    let mut panes = state.panes.write().await;
    let Some(PaneResource::Terminal(runtime)) = panes.get_mut(&pane_id) else {
        return;
    };
    let Some(epoch) = runtime.prompt_transcript_epoch.as_ref() else {
        return;
    };
    if epoch.token != prompt_epoch
        || epoch.session_id != expected_session_id
        || epoch.generation != runtime.agent_generation
        // Provider stores can flush a previous row after this Enter. An
        // exact PTY-receipt-backed prompt is already authoritative; only an
        // unknown latest prompt can be repaired by transcript evidence.
        || !runtime.latest_agent_prompt_unavailable
        || runtime.agent_process_key.as_ref() != Some(&epoch.process)
        || runtime.is_session_identity_invalidated
        || runtime.session_id.as_deref() != Some(expected_session_id)
        || runtime.session_agent_class.as_ref() != Some(&epoch.process.class)
        || runtime.session_process_id != Some(epoch.process.process_id)
        || runtime.session_process_started_at_unix_seconds
            != Some(epoch.process.started_at_unix_seconds)
    {
        return;
    }
    let owner = epoch.process.clone();
    runtime.prompt_transcript_epoch = None;
    runtime.last_agent_prompt = Some(last_prompt.clone());
    runtime.latest_agent_prompt_unavailable = false;
    update_unavailable_recovery_prompt(
        &mut tree,
        pane_id,
        &owner,
        Some(&last_prompt),
        None,
        false,
        state,
    );
    if tree
        .set_last_prompt(pane_id, Some(last_prompt.clone()))
        .is_ok()
    {
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
    direct_tx: &DirectEventSender,
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
    direct_tx: &DirectEventSender,
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
                Some(record) if record.identity != identity => {
                    Decision::Return(Err(progress_rejection(
                        ilium_ipc::ProgressMonitorRejectionCode::InvalidRequest,
                        format!(
                            "progress set request_id {request_id} was already used with different arguments"
                        ),
                    )))
                }
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
            Some(PaneResource::Terminal(runtime)) if runtime.missing_workspace.is_some() => {
                return Err(progress_rejection(
                    ilium_ipc::ProgressMonitorRejectionCode::InvalidRequest,
                    format!("pane {pane_id:?} is waiting for its saved worktree"),
                ));
            }
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
        if runtime.missing_workspace.is_some() {
            return Err(progress_rejection(
                ilium_ipc::ProgressMonitorRejectionCode::InvalidRequest,
                format!("pane {pane_id:?} is waiting for its saved worktree"),
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

/// Raises the sound and desktop notification for a monitored task's first
/// terminal outcome. Both outputs obey the same policy: the per-event
/// toggles, suppression when the pane's agent is idle or parked (its own
/// "agent finished" alert follows), and per-pane coalescing of bursts. The
/// sidebar signal is unaffected by any of this.
pub(crate) async fn alert_task_outcome(
    state: &Arc<ServerState>,
    pane_id: NodeId,
    progress: &ilium_core::PaneProgress,
) {
    let Some(kind) = crate::notifications::TaskOutcomeKind::from_progress(progress) else {
        return;
    };
    let notification_settings = *state.notifications_config.read().await;
    let sound_settings = state.sound_settings.read().await.clone();
    let is_sound_enabled = sound_settings.events.is_enabled(kind.sound_event());
    let is_notification_enabled = notification_settings.is_enabled(kind.notification_event());
    if !is_sound_enabled && !is_notification_enabled {
        return;
    }
    let (pane_name, session_status) = {
        let tree = state.tree.read().await;
        let Some(node) = tree.get(pane_id) else {
            return;
        };
        let pane_name = node.short_name.clone().unwrap_or_else(|| node.name.clone());
        let status = match &node.kind {
            ilium_core::NodeKind::Pane { status, .. } => Some(status.clone()),
            _ => None,
        };
        (pane_name, status)
    };
    let is_redundant = session_status.as_ref().is_some_and(|status| {
        crate::notifications::is_task_outcome_redundant(&notification_settings, status, progress)
    });
    if is_redundant {
        return;
    }
    let is_admitted = state
        .task_outcome_coalescer
        .lock()
        .map(|mut coalescer| {
            coalescer.admit(
                pane_id,
                kind,
                std::time::Instant::now(),
                notification_settings.task_coalesce_seconds,
            )
        })
        // A poisoned lock only loses burst suppression; never drop the alert.
        .unwrap_or(true);
    if !is_admitted {
        return;
    }
    if is_sound_enabled {
        match state.sound_requests.prepare(
            &sound_settings,
            Some(kind.sound_event()),
            Some(&pane_name),
        ) {
            Ok(request) => crate::sounds::enqueue_prepared(state, request),
            Err(reason) => {
                tracing::warn!(?reason, "task sound preparation refused before allocation")
            }
        }
    }
    if is_notification_enabled {
        let is_agent_working = session_status
            .as_ref()
            .is_some_and(|status| crate::notifications::is_agent_mid_turn(status, progress));
        let pending = crate::notifications::PendingNotification::for_task_outcome(
            state.session_name.clone(),
            pane_name,
            kind,
            progress.report.job_id.clone(),
            is_agent_working,
        );
        if let Some(execution) = state.execution.get() {
            crate::notifications::send(&execution.client, pending);
        } else {
            tracing::warn!(
                "desktop notification skipped because the execution bank is unavailable"
            );
        }
    }
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
        let is_first_outcome_notification = runtime.claim_progress_outcome_notification(monitor_id);
        drop(panes);
        state.request_snapshot_save();
        if is_first_outcome_notification {
            alert_task_outcome(state, pane_id, &progress).await;
        }
        if hand_outcome_to_progress_waiters(state, pane_id, monitor_id, &progress).await {
            return;
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
    direct_tx: &DirectEventSender,
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

/// Holds an `ilium progress wait` request. An already-settled monitor is
/// answered at once; otherwise the waiter is parked on the pane and answered
/// by the settle, replacement, or clear path. Holding the effect gate orders
/// the already-settled claim against a delivery's final validation, so a
/// `composer_notice_suppressed: true` reply really means no composer write.
async fn handle_wait_pane_progress_monitor(
    state: &Arc<ServerState>,
    request_id: u64,
    pane_id: NodeId,
    monitor_id: u64,
    direct_tx: &DirectEventSender,
) {
    let effect_gate = match state.panes.read().await.get(&pane_id) {
        Some(PaneResource::Terminal(runtime)) => Some(Arc::clone(&runtime.progress_effect_gate)),
        _ => None,
    };
    let Some(effect_gate) = effect_gate else {
        let rejection = progress_rejection(
            ilium_ipc::ProgressMonitorRejectionCode::PaneNotFound,
            format!("pane {pane_id:?} is not a live terminal pane"),
        );
        send_progress_wait_completed(direct_tx, request_id, pane_id, Err(rejection)).await;
        return;
    };
    let _effect_guard = effect_gate.lock().await;
    let mut panes = state.panes.write().await;
    let Some(PaneResource::Terminal(runtime)) = panes.get_mut(&pane_id) else {
        drop(panes);
        let rejection = progress_rejection(
            ilium_ipc::ProgressMonitorRejectionCode::PaneNotFound,
            format!("pane {pane_id:?} closed before the wait started"),
        );
        send_progress_wait_completed(direct_tx, request_id, pane_id, Err(rejection)).await;
        return;
    };
    let current = runtime
        .progress_monitor
        .as_ref()
        .map(|monitor| (monitor.monitor_id, monitor.latest_progress.clone()));
    let immediate = match current {
        None => Some(ilium_ipc::ProgressWaitOutcome {
            monitor_id,
            end: ilium_ipc::ProgressWaitEnd::Cleared,
            progress: None,
            composer_notice_suppressed: false,
        }),
        Some((current_id, _)) if current_id != monitor_id => Some(ilium_ipc::ProgressWaitOutcome {
            monitor_id,
            end: ilium_ipc::ProgressWaitEnd::Superseded,
            progress: None,
            composer_notice_suppressed: false,
        }),
        Some((_, progress)) if progress.is_terminal() || progress.monitor_health.is_failed() => {
            let composer_notice_suppressed = runtime
                .collect_progress_outcome_by_waiter(monitor_id)
                .unwrap_or(false);
            Some(ilium_ipc::ProgressWaitOutcome {
                monitor_id,
                end: ilium_ipc::ProgressWaitEnd::Settled,
                progress: Some(progress),
                composer_notice_suppressed,
            })
        }
        Some(_) => {
            runtime.add_progress_waiter(crate::pane::ProgressWaiter {
                pane_id,
                monitor_id,
                request_id,
                reply: direct_tx.clone(),
            });
            None
        }
    };
    drop(panes);
    let Some(outcome) = immediate else {
        return;
    };
    if outcome.composer_notice_suppressed {
        state.request_snapshot_save();
    }
    send_progress_wait_completed(direct_tx, request_id, pane_id, Ok(outcome)).await;
}

async fn send_progress_wait_completed(
    direct_tx: &DirectEventSender,
    request_id: u64,
    pane_id: NodeId,
    result: Result<ilium_ipc::ProgressWaitOutcome, ilium_ipc::ProgressMonitorRejection>,
) {
    send_direct(
        direct_tx,
        ServerEvent::ProgressWaitCompleted {
            request_id,
            pane_id,
            result,
        },
    )
    .await;
}

/// Hands a settled outcome to every still-connected `wait` for it. Returns
/// whether at least one waiter accepted it, in which case the composer
/// notification is skipped: the agent already has the result.
async fn hand_outcome_to_progress_waiters(
    state: &Arc<ServerState>,
    pane_id: NodeId,
    monitor_id: u64,
    progress: &ilium_core::PaneProgress,
) -> bool {
    let effect_gate = match state.panes.read().await.get(&pane_id) {
        Some(PaneResource::Terminal(runtime)) => Arc::clone(&runtime.progress_effect_gate),
        _ => return false,
    };
    let _effect_guard = effect_gate.lock().await;
    let (waiters, is_marked) = {
        let mut panes = state.panes.write().await;
        let Some(PaneResource::Terminal(runtime)) = panes.get_mut(&pane_id) else {
            return false;
        };
        let waiters = runtime.take_progress_waiters(monitor_id);
        if waiters.is_empty() {
            return false;
        }
        // Mark before sending so no delivery can start in between; undone
        // below if every waiter turns out to be gone.
        let is_marked = runtime
            .collect_progress_outcome_by_waiter(monitor_id)
            .unwrap_or(false);
        (waiters, is_marked)
    };
    let mut accepted = 0_usize;
    for waiter in waiters {
        let event = ServerEvent::ProgressWaitCompleted {
            request_id: waiter.request_id,
            pane_id,
            result: Ok(ilium_ipc::ProgressWaitOutcome {
                monitor_id,
                end: ilium_ipc::ProgressWaitEnd::Settled,
                progress: Some(progress.clone()),
                composer_notice_suppressed: is_marked,
            }),
        };
        if waiter.reply.send(event).await.is_ok() {
            accepted += 1;
        }
    }
    if accepted > 0 && is_marked {
        state.request_snapshot_save();
        return true;
    }
    if is_marked {
        let mut panes = state.panes.write().await;
        if let Some(PaneResource::Terminal(runtime)) = panes.get_mut(&pane_id) {
            runtime.release_progress_outcome_from_waiter(monitor_id);
        }
    }
    false
}

/// Stops one generation and clears its sticky presentation. A supplied ID is
/// an optimistic-concurrency fence, so an old agent cannot clear a replacement.
async fn handle_clear_pane_progress_monitor(
    state: &Arc<ServerState>,
    request_id: u64,
    pane_id: NodeId,
    expected_monitor_id: Option<u64>,
    direct_tx: &DirectEventSender,
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
    let pane_id = persisted.pane_id;
    let effect_gate = {
        let panes = state.panes.read().await;
        let Some(PaneResource::Terminal(runtime)) = panes.get(&pane_id) else {
            return Err(format!("persisted monitor pane {pane_id:?} is unavailable"));
        };
        if runtime.missing_workspace.is_some() {
            return Err(format!(
                "persisted monitor pane {pane_id:?} is waiting for its saved worktree"
            ));
        }
        Arc::clone(&runtime.progress_effect_gate)
    };
    let monitor_id = state.allocate_progress_monitor_id();
    let (fresh_preflight, restoration_failure) = if persisted.requires_probe_before_restore()
        && !state.is_progress_monitor_enabled()
    {
        // Dropping the record would leave its agent waiting forever. Keep it
        // as sticky failed evidence; the reconciler tells the agent once
        // monitoring is enabled again.
        (
            None,
            Some(crate::progress_watchdog::OBSERVATION_DISABLED.to_string()),
        )
    } else if persisted.requires_probe_before_restore() {
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
        && (progress.is_terminal() || progress.monitor_health.is_failed())
        && state.is_progress_monitor_enabled();

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
            ilium_prompts::render_value(
                "naming/progress-restoration-failure",
                &serde_json::json!({"details": bounded_restoration_failure(&restoration_failure)}),
            )
        } else if progress.is_terminal() {
            crate::agent_delivery::terminal_result_message(&progress)
        } else {
            let error = match &progress.monitor_health {
                ilium_core::ProgressMonitorHealth::Failed { last_error, .. } => last_error.as_str(),
                _ => ilium_prompts::agent::PROGRESS_OBSERVATION_STOPPED,
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
/// converted to sticky failed evidence ("outcome unknown") rather than
/// removed, so their agent is told once monitoring is enabled again; terminal task evidence and failed-monitor evidence remain sticky
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
    let mut stopped_progress = Vec::new();
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
            // Removing the registration would leave its agent waiting for a
            // result that can never arrive. Keep sticky failed evidence
            // instead; the reconciler tells the agent once monitoring is
            // enabled again.
            if let Some(failed) = crate::progress_watchdog::mark_observation_stopped(
                &mut tree,
                *pane_id,
                runtime,
                crate::progress_watchdog::OBSERVATION_DISABLED,
            ) {
                stopped_progress.push((*pane_id, failed));
            }
        }
    }
    drop(panes);
    drop(tree);
    for (pane_id, progress) in stopped_progress {
        state.broadcast(ServerEvent::PaneProgressChanged {
            pane_id,
            progress: Some(progress),
        });
    }
    state.request_snapshot_save();
}

async fn handle_update_agent_detection_settings(
    state: &Arc<ServerState>,
    settings: ilium_ipc::AgentDetectionSettings,
    request_id: Option<u64>,
    direct_tx: &DirectEventSender,
) {
    let config_dir = match crate::paths::config_dir() {
        Ok(path) => path,
        Err(error) => {
            send_direct(
                direct_tx,
                ServerEvent::AgentDetectionSettingsChanged {
                    request_id,
                    result: Err(ilium_ipc::AgentDetectionSettingsError {
                        message: error.to_string(),
                    }),
                },
            )
            .await;
            return;
        }
    };
    match apply_agent_detection_settings(state, settings, config_dir).await {
        Ok(settings) => {
            // The persistence operation has finished before either publication.
            // Only the originating connection receives its correlation ID.
            if let Some(request_id) = request_id {
                send_direct(
                    direct_tx,
                    ServerEvent::AgentDetectionSettingsChanged {
                        request_id: Some(request_id),
                        result: Ok(settings.clone()),
                    },
                )
                .await;
            }
            state.broadcast(ServerEvent::AgentDetectionSettingsChanged {
                request_id: None,
                result: Ok(settings),
            });
        }
        Err(error) => {
            send_direct(
                direct_tx,
                ServerEvent::AgentDetectionSettingsChanged {
                    request_id,
                    result: Err(error),
                },
            )
            .await;
        }
    }
}

async fn apply_agent_detection_settings(
    state: &Arc<ServerState>,
    settings: ilium_ipc::AgentDetectionSettings,
    config_dir: std::path::PathBuf,
) -> Result<ilium_ipc::AgentDetectionSettings, ilium_ipc::AgentDetectionSettingsError> {
    // Serialize accepted writes, then preserve the server-only automation
    // switch while replacing observation settings. Persistence precedes live
    // application so a failed write never leaves a success-shaped runtime
    // state that disappears on restart.
    let _transaction = state.agent_detection_settings_transaction.lock().await;
    let (current_detection, _) = state.agent_detection_settings_snapshot().await;

    // Reserve bounded I/O capacity before validation and copies create the
    // immutable writer input. The reader and serializer both cap config.toml
    // at MAX_CONFIG_BYTES, while this declaration also covers path and
    // settings copies retained by the callback and its error result.
    let cost = agent_detection_settings_save_cost(&config_dir, &settings)?;
    let execution =
        state
            .execution
            .get()
            .ok_or_else(|| ilium_ipc::AgentDetectionSettingsError {
                message: "settings persistence worker is unavailable".to_string(),
            })?;
    let reservation = execution
        .client
        .reserve(ilium_execution::Lane::Io, cost)
        .await
        .map_err(|reason| ilium_ipc::AgentDetectionSettingsError {
            message: format!("settings persistence admission failed: {reason:?}"),
        })?;

    let mut validated = crate::config::validate_agent_detection_settings(&settings)?;
    validated.detection.auto_answer_interstitial_prompts =
        current_detection.auto_answer_interstitial_prompts;
    let accepted_settings =
        crate::config::agent_detection_settings(&validated.detection, &validated.custom_signatures);
    let persisted_settings = accepted_settings.clone();
    save_agent_detection_settings_reserved(
        &execution.client,
        reservation,
        config_dir,
        persisted_settings,
        crate::config::save_agent_detection_settings,
    )
    .await?;

    state
        .replace_agent_detection_settings(validated.detection, validated.custom_signatures)
        .await;
    {
        let now = std::time::Instant::now();
        let mut panes = state.panes.write().await;
        for resource in panes.values_mut() {
            if let PaneResource::Terminal(runtime) = resource {
                runtime.detection_schedule.next_due = now;
            }
        }
    }
    state.detection_schedule_changed.notify_one();
    Ok(accepted_settings)
}

fn agent_detection_settings_save_cost(
    config_dir: &std::path::PathBuf,
    settings: &ilium_ipc::AgentDetectionSettings,
) -> Result<ilium_execution::JobCost, ilium_ipc::AgentDetectionSettingsError> {
    use std::mem::size_of;

    const TOML_DOCUMENT_ALLOCATION_FACTOR: usize = 16;
    const SETTINGS_CAPTURE_FACTOR: usize = 8;
    let invalid = || ilium_ipc::AgentDetectionSettingsError {
        message: "settings persistence input exceeds its bounded allocation limit".to_string(),
    };
    let path_bytes = config_dir.capacity();
    if path_bytes > 64 * 1024 {
        return Err(invalid());
    }
    let mut settings_bytes = settings
        .custom_signatures
        .capacity()
        .checked_mul(size_of::<ilium_ipc::CustomAgentSignature>())
        .ok_or_else(invalid)?;
    for signature in &settings.custom_signatures {
        settings_bytes = settings_bytes
            .checked_add(signature.name_substring.capacity())
            .and_then(|bytes| match &signature.class {
                ilium_core::AgentClass::Other(label) => bytes.checked_add(label.capacity()),
                _ => Some(bytes),
            })
            .ok_or_else(invalid)?;
    }
    if settings_bytes > crate::config::MAX_CONFIG_BYTES {
        return Err(invalid());
    }
    let input_bytes = path_bytes
        .checked_mul(4)
        .and_then(|bytes| bytes.checked_add(settings_bytes.checked_mul(SETTINGS_CAPTURE_FACTOR)?))
        // Parsing TOML constructs tables and value nodes in addition to the
        // bounded source string; reserve room for that representation and
        // the subsequent serialized document together.
        .and_then(|bytes| {
            bytes.checked_add(crate::config::MAX_CONFIG_BYTES * TOML_DOCUMENT_ALLOCATION_FACTOR)
        })
        .and_then(|bytes| bytes.checked_add(16 * 1024))
        .ok_or_else(invalid)?;
    let result_bytes = crate::config::MAX_CONFIG_BYTES
        .checked_add(path_bytes.checked_mul(4).ok_or_else(invalid)?)
        .and_then(|bytes| bytes.checked_add(16 * 1024))
        .ok_or_else(invalid)?;
    Ok(ilium_execution::JobCost {
        input_bytes,
        result_bytes,
    })
}

async fn save_agent_detection_settings_reserved(
    client: &crate::execution::ExecutionClient,
    reservation: ilium_execution::Reservation,
    config_dir: std::path::PathBuf,
    settings: ilium_ipc::AgentDetectionSettings,
    write: impl FnOnce(
            &std::path::Path,
            &ilium_ipc::AgentDetectionSettings,
        ) -> Result<(), ilium_ipc::AgentDetectionSettingsError>
        + Send
        + 'static,
) -> Result<(), ilium_ipc::AgentDetectionSettingsError> {
    match client
        .run_reserved(reservation, move |_context| {
            Ok::<_, std::convert::Infallible>(write(&config_dir, &settings))
        })
        .await
    {
        Ok(completion) => completion.view().clone(),
        Err(error) => Err(ilium_ipc::AgentDetectionSettingsError {
            message: format!("settings persistence worker failed: {error}"),
        }),
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
            Some(PaneResource::Editor { .. } | PaneResource::Unrestored(_)) | None => {
                (false, false)
            }
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
    /// Close the pane once its command exits (`ilium new-pane` default).
    close_on_exit: bool,
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
                close_on_exit: false,
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
                close_on_exit: false,
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
                close_on_exit: false,
            }
        }
        NewPaneKind::CommandClosingOnExit(command_line) => {
            let origin = TerminalOrigin::Command(command_line);
            let name = origin.default_pane_name().to_string();
            NewPanePlan {
                spawn_kind: PaneSnapshotKind::Terminal(origin),
                name,
                content_kind: PaneContentKind::Terminal,
                initial_input: None,
                close_on_exit: true,
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
                close_on_exit: false,
            }
        }
    }
}

async fn handle_new_pane(
    state: &Arc<ServerState>,
    parent_group: NodeId,
    kind: NewPaneKind,
    working_directory: NewPaneWorkingDirectory,
    direct_tx: &DirectEventSender,
) {
    let plan = new_pane_plan(kind);
    let spawn_description = format!("{:?}", plan.spawn_kind);

    // Resolve the live-terminal policy before creating the tree node. The
    // eventual node and its actual launch cwd must appear in one tree write,
    // even if another request snapshots the session while spawning awaits.
    let cwd_candidate = candidate_new_pane_working_directory(state, working_directory).await;
    if matches!(working_directory, NewPaneWorkingDirectory::WorkspacePane(_))
        && cwd_candidate.is_none()
    {
        send_direct_error(
            direct_tx,
            "worktree pane or its launch directory is unavailable".to_string(),
        )
        .await;
        return;
    }

    // Pruning holds this fence while it proves that no pane uses a checkout.
    // Publish the new node under the same fence; the spawn path takes it again
    // after its repository admission, so release it before spawning.
    let publish_guard = state.workspace_spawn_lock.lock().await;
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
            drop(publish_guard);
            send_direct_error(direct_tx, format!("failed to create pane: {error}")).await;
            return;
        }
    };
    let project_cwd = tree
        .project_path_for(pane_id)
        .map(std::path::Path::to_path_buf)
        .unwrap_or_else(|| state.session_cwd.clone());
    let cwd = cwd_candidate.unwrap_or(project_cwd);
    if plan.content_kind == PaneContentKind::Terminal {
        if let Err(error) = tree.set_pane_launch_cwd(pane_id, cwd.clone()) {
            let _ = tree.remove_node(pane_id);
            drop(tree);
            drop(publish_guard);
            send_direct_error(
                direct_tx,
                format!("failed to record pane directory: {error}"),
            )
            .await;
            return;
        }
    }
    // Drop the write guard before spawning (a pty spawn + registering it
    // in `state.panes` needs no tree access at all) and before the
    // eventual broadcast snapshot's O(n) clone -- see `broadcast_and_persist`.
    drop(tree);
    drop(publish_guard);

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
    if plan.close_on_exit {
        start_close_on_exit_watcher(state, pane_id).await;
    }
}

/// A closing pane must live at least this long, so the creating CLI can
/// observe it in a tree snapshot even when its command exits immediately.
const CLOSE_ON_EXIT_MINIMUM_LIFETIME: std::time::Duration = std::time::Duration::from_secs(3);
const CLOSE_ON_EXIT_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(500);

/// Why a close-on-exit watcher should keep waiting or stop.
enum CloseOnExitCheck {
    Gone,
    Running,
    Exited,
}

async fn check_close_on_exit(state: &Arc<ServerState>, pane_id: NodeId) -> CloseOnExitCheck {
    let panes = state.panes.read().await;
    let Some(PaneResource::Terminal(runtime)) = panes.get(&pane_id) else {
        return CloseOnExitCheck::Gone;
    };
    if runtime.session.child_exit().is_none() {
        return CloseOnExitCheck::Running;
    }
    // A live progress monitor still owes its agent a final report; closing
    // the pane now would cancel it. Wait until the monitor reaches an outcome.
    let is_monitor_live = runtime
        .progress_monitor
        .as_ref()
        .is_some_and(|monitor| monitor.latest_progress.is_live());
    if is_monitor_live {
        CloseOnExitCheck::Running
    } else {
        CloseOnExitCheck::Exited
    }
}

/// Installs the pane-owned task that closes an `ilium new-pane` pane after its
/// command exits. The task ends when the pane disappears (the pane aborts it on
/// close). The close itself runs in a separate short task from the server's
/// drained registry, so aborting the watcher during teardown cannot cut the
/// close short.
async fn start_close_on_exit_watcher(state: &Arc<ServerState>, pane_id: NodeId) {
    let watcher_state = Arc::clone(state);
    let handle = tokio::spawn(async move {
        tokio::time::sleep(CLOSE_ON_EXIT_MINIMUM_LIFETIME).await;
        loop {
            match check_close_on_exit(&watcher_state, pane_id).await {
                CloseOnExitCheck::Gone => return,
                CloseOnExitCheck::Exited => break,
                CloseOnExitCheck::Running => {}
            }
            tokio::time::sleep(CLOSE_ON_EXIT_POLL_INTERVAL).await;
        }
        let close_state = Arc::clone(&watcher_state);
        let close = tokio::spawn(async move {
            // No client asked for this close, so errors have nowhere to go.
            let (silent_sender, _silent_receiver) = DirectEventSender::channel(1);
            crate::lifecycle_log::record_close_request(&close_state, pane_id, "close_on_exit");
            handle_close_pane(&close_state, pane_id, &silent_sender).await;
        });
        if !watcher_state.track_workspace_mutation_task(close) {
            tracing::warn!("server is shutting down; pane {pane_id:?} will not auto-close");
        }
    });
    let mut panes = state.panes.write().await;
    match panes.get_mut(&pane_id) {
        Some(PaneResource::Terminal(runtime)) => runtime.set_close_on_exit_task(handle),
        _ => handle.abort(),
    }
}

/// Native process-tree termination timeout; no registry guard spans this wait.
const TERMINATE_PANE_PROCESS_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

/// Stops the process tree behind one terminal pane while keeping its node,
/// PTY, and last screen registered, so a client can freeze the viewport and
/// act on the dead session. The requester always receives exactly one
/// `PaneProcessTerminated`.
async fn handle_terminate_pane_process(
    state: &Arc<ServerState>,
    pane_id: NodeId,
    direct_tx: &DirectEventSender,
) {
    let outcome = terminate_pane_process_with_work(state, pane_id, |control| {
        match control.terminate_process_tree(TERMINATE_PANE_PROCESS_TIMEOUT) {
            Ok(_) => Ok(()),
            Err(tree_error) => {
                tracing::warn!(%tree_error, "pane process-tree proof unavailable; killing captured direct child");
                control.kill_direct_child().map_err(std::io::Error::other)
            }
        }
    }).await;
    let _ = crate::agent_debug::record(
        state,
        pane_id,
        AgentDebugSource::Server,
        AgentDebugEventDraft::information(
            AgentDebugEventKind::PaneCreated,
            "Pane process terminated for session conversion",
        )
        .with_fields(vec![AgentDebugField::plain(
            "result",
            match &outcome {
                Ok(()) => "stopped".to_string(),
                Err(error) => error.clone(),
            },
        )]),
    )
    .await;
    let _ = direct_tx
        .send(ServerEvent::PaneProcessTerminated {
            pane_id,
            result: outcome,
        })
        .await;
}

async fn handle_freeze_pane(
    state: &Arc<ServerState>,
    pane_id: NodeId,
    resume_command: String,
    direct_tx: &DirectEventSender,
) {
    let requested_binding = BuiltinAgentProvider::resume_binding(&resume_command);
    let matches_authoritative_session =
        if let Some((requested_provider, requested_session_id)) = requested_binding {
            let tree = state.tree.read().await;
            let panes = state.panes.read().await;
            match (tree.get(pane_id), panes.get(&pane_id)) {
                (Some(node), Some(PaneResource::Terminal(runtime))) => {
                    let snapshot = TitleRuntimeSnapshot::capture(node, runtime, false);
                    snapshot
                        .observation
                        .agent_class
                        .as_ref()
                        .and_then(|class| class.provider())
                        == Some(requested_provider)
                        && snapshot.observation.session_id.as_deref()
                            == Some(requested_session_id.as_str())
                }
                _ => false,
            }
        } else {
            false
        };
    if !matches_authoritative_session {
        let _ = direct_tx
            .send(ServerEvent::PaneFrozen {
                pane_id,
                result: Err("freeze requires the pane's current provider session identity".into()),
            })
            .await;
        return;
    }

    let outcome = terminate_pane_process_with_work(state, pane_id, |control| {
        control
            .terminate_process_tree(TERMINATE_PANE_PROCESS_TIMEOUT)
            .map(|_| ())
            .or_else(|_| control.kill_direct_child().map_err(std::io::Error::other))
    })
    .await;
    if outcome.is_ok() {
        let mut panes = state.panes.write().await;
        if let Some(PaneResource::Terminal(runtime)) = panes.get_mut(&pane_id) {
            runtime.origin = TerminalOrigin::Frozen { resume_command };
        }
    }
    if outcome.is_ok() {
        broadcast_and_persist(state).await;
    }
    let _ = direct_tx
        .send(ServerEvent::PaneFrozen {
            pane_id,
            result: outcome,
        })
        .await;
}

/// Resumes only an authoritative frozen runtime. Saved session IDs come from
/// the server origin; older bare provider origins use a provider-owned picker
/// when available, so recovery never guesses the most recent conversation.
async fn handle_unfreeze_pane(
    state: &Arc<ServerState>,
    pane_id: NodeId,
    direct_tx: &DirectEventSender,
) {
    let Some(reservation) = PaneReplacementReservation::try_new(state, pane_id) else {
        send_direct_error(
            direct_tx,
            format!("pane {pane_id:?} is already being replaced"),
        )
        .await;
        return;
    };

    let resume_command = {
        let panes = state.panes.read().await;
        match panes.get(&pane_id) {
            Some(PaneResource::Terminal(runtime)) => match &runtime.origin {
                TerminalOrigin::Frozen { resume_command } => {
                    if BuiltinAgentProvider::resume_binding(resume_command).is_some() {
                        Ok(resume_command.clone())
                    } else if let Some(provider) =
                        BuiltinAgentProvider::from_command_line(resume_command.trim())
                    {
                        match provider.resume_picker_command() {
                            Some(command) => Ok(command.to_string()),
                            None => Err(format!(
                                "is a legacy frozen {} pane without a saved session ID; no safe session picker is available",
                                provider.label()
                            )),
                        }
                    } else {
                        Err("has no recognized session-specific resume command".to_string())
                    }
                }
                _ => Err("is not frozen".to_string()),
            },
            _ => Err("is not a live terminal pane".to_string()),
        }
    };
    let resume_command = match resume_command {
        Ok(command) => command,
        Err(reason) => {
            send_direct_error(direct_tx, format!("pane {pane_id:?} {reason}")).await;
            return;
        }
    };
    handle_replace_pane_with_command(
        state,
        pane_id,
        resume_command,
        direct_tx,
        Some(reservation),
        None,
    )
    .await;
}

struct PaneReplacementReservation<'a> {
    state: &'a Arc<ServerState>,
    pane_id: NodeId,
}

impl<'a> PaneReplacementReservation<'a> {
    fn try_new(state: &'a Arc<ServerState>, pane_id: NodeId) -> Option<Self> {
        let inserted = state
            .replacing_panes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(pane_id);
        inserted.then_some(Self { state, pane_id })
    }
}

impl Drop for PaneReplacementReservation<'_> {
    fn drop(&mut self) {
        self.state
            .replacing_panes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&self.pane_id);
    }
}

/// Native termination owns the captured child handle until physical return.
/// A caller timeout reports uncertainty; it neither joins a stuck callback
/// nor claims that the original child or a replacement pane was stopped.
async fn terminate_pane_process_with_work<Work>(
    state: &Arc<ServerState>,
    pane_id: NodeId,
    work: Work,
) -> Result<(), String>
where
    Work: FnOnce(ilium_pty::PtyTerminationHandle) -> std::io::Result<()> + Send + 'static,
{
    let control = {
        let panes = state.panes.read().await;
        let Some(PaneResource::Terminal(runtime)) = panes.get(&pane_id) else {
            return Err(format!("pane {pane_id:?} has no running terminal process"));
        };
        runtime.session.termination_handle()
    };
    let Some(execution) = state.execution.get() else {
        return Err("pane termination unavailable: server execution is not running".to_owned());
    };
    let captured_control = control.clone();
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        execution.client.run(
            ilium_execution::Lane::Io,
            ilium_execution::JobCost {
                input_bytes: 4096,
                result_bytes: 4096,
            },
            move |_| work(captured_control),
        ),
    )
    .await;
    let outcome = match result {
        Ok(result) => result.map(|_| ()).map_err(|error| format!("pane termination failed: {error}")),
        Err(_) => Err("pane termination timed out; captured native work remains owned and its outcome is uncertain".to_owned()),
    };
    let panes = state.panes.read().await;
    let same_session = matches!(panes.get(&pane_id), Some(PaneResource::Terminal(runtime)) if control.same_session(&runtime.session.termination_handle()));
    if !same_session {
        return Err(format!(
            "pane {pane_id:?} changed during termination; result belongs to the original child and does not establish replacement termination ({outcome:?})"
        ));
    }
    outcome
}

/// Replaces one terminal pane with a new pane running `command_line` in the
/// same parent, position, launch directory, and worktree ownership. The new
/// pane is spawned before the old one is closed, so a spawn failure leaves
/// the old pane intact. A worktree custody ticket moves to the new runtime.
async fn handle_replace_pane_with_command(
    state: &Arc<ServerState>,
    pane_id: NodeId,
    command_line: String,
    direct_tx: &DirectEventSender,
    reservation: Option<PaneReplacementReservation<'_>>,
    custody_ticket: Option<crate::workspace_custody::CustodyTicket>,
) {
    let Some(_reservation) =
        reservation.or_else(|| PaneReplacementReservation::try_new(state, pane_id))
    else {
        send_direct_error(
            direct_tx,
            format!("pane {pane_id:?} is already being replaced"),
        )
        .await;
        return;
    };
    let origin = TerminalOrigin::Command(command_line.clone());
    let publish_guard = state.workspace_spawn_lock.lock().await;
    let custody_ticket = if custody_ticket.is_some() {
        custody_ticket
    } else {
        state
            .panes
            .read()
            .await
            .get(&pane_id)
            .and_then(|resource| match resource {
                PaneResource::Terminal(runtime) => runtime.custody_ticket.clone(),
                PaneResource::Editor { .. } | PaneResource::Unrestored(_) => None,
            })
    };
    let continuity_observation = {
        let tree = state.tree.read().await;
        let panes = state.panes.read().await;
        match (
            BuiltinAgentProvider::resume_binding(&command_line),
            tree.get(pane_id),
            panes.get(&pane_id),
        ) {
            (Some((provider, session_id)), Some(node), Some(PaneResource::Terminal(runtime))) => {
                let snapshot = TitleRuntimeSnapshot::capture(node, runtime, false);
                (snapshot
                    .observation
                    .agent_class
                    .as_ref()
                    .and_then(|class| class.provider())
                    == Some(provider)
                    && snapshot.observation.session_id.as_deref() == Some(session_id.as_str()))
                .then_some(snapshot.observation)
            }
            _ => None,
        }
    };
    let continuity_evidence = collect_observed_title_evidence(
        state,
        &continuity_observation.into_iter().collect::<Vec<_>>(),
    )
    .await;
    let close_preference = state
        .workspace_close_preferences
        .read()
        .await
        .get(&pane_id)
        .cloned();
    let mut tree = state.tree.write().await;
    let Some(parent) = tree.parent_of(pane_id) else {
        drop(tree);
        drop(publish_guard);
        send_direct_error(direct_tx, format!("no such pane {pane_id:?}")).await;
        return;
    };
    let is_terminal = tree.get(pane_id).is_some_and(|node| {
        matches!(
            node.kind,
            ilium_core::NodeKind::Pane {
                content: PaneContentKind::Terminal,
                ..
            }
        )
    });
    if !is_terminal {
        drop(tree);
        drop(publish_guard);
        send_direct_error(
            direct_tx,
            "only a terminal pane can be replaced".to_string(),
        )
        .await;
        return;
    }
    let workspace = tree.pane_workspace(pane_id).cloned();
    let position = tree
        .children_of(parent)
        .ok()
        .and_then(|children| children.iter().position(|child| *child == pane_id));
    let cwd = tree
        .pane_cwd(pane_id)
        .map(std::path::Path::to_path_buf)
        .unwrap_or_else(|| state.session_cwd.clone());
    let new_pane_id = match tree.add_pane(
        parent,
        origin.default_pane_name().to_string(),
        PaneContentKind::Terminal,
    ) {
        Ok(id) => id,
        Err(error) => {
            drop(tree);
            drop(publish_guard);
            send_direct_error(
                direct_tx,
                format!("failed to create replacement pane: {error}"),
            )
            .await;
            return;
        }
    };
    let has_verified_continuity = {
        let panes = state.panes.read().await;
        !title_grants_under_lock(&tree, &panes, &continuity_evidence, &HashMap::new()).is_empty()
    };
    let placed = tree
        .move_node(new_pane_id, parent, position)
        .and_then(|()| tree.set_pane_launch_cwd(new_pane_id, cwd.clone()))
        .and_then(|()| tree.set_pane_workspace(new_pane_id, workspace.clone()))
        .and_then(|()| {
            tree.inherit_pane_title_with_verified_continuity(
                pane_id,
                new_pane_id,
                has_verified_continuity,
            )
        });
    if let Err(error) = placed {
        let _ = tree.remove_node(new_pane_id);
        drop(tree);
        drop(publish_guard);
        send_direct_error(
            direct_tx,
            format!("failed to place replacement pane: {error}"),
        )
        .await;
        return;
    }
    drop(tree);
    if let Some(mut preference) = close_preference {
        preference.pane_id = new_pane_id;
        state
            .workspace_close_preferences
            .write()
            .await
            .insert(new_pane_id, preference);
    }
    drop(publish_guard);

    match spawn_and_register_pane_in_directory_with_custody_ticket(
        state,
        new_pane_id,
        PaneSnapshotKind::Terminal(origin),
        &cwd,
        custody_ticket.clone(),
    )
    .await
    {
        Ok(()) => {}
        Err(RegisterPaneError::NodeRemoved(_)) => {
            state
                .workspace_close_preferences
                .write()
                .await
                .remove(&new_pane_id);
            return;
        }
        Err(RegisterPaneError::Spawn(error)) => {
            let mut tree = state.tree.write().await;
            let _ = tree.remove_node(new_pane_id);
            drop(tree);
            state
                .workspace_close_preferences
                .write()
                .await
                .remove(&new_pane_id);
            broadcast_and_persist(state).await;
            send_direct_error(
                direct_tx,
                format!("failed to spawn {command_line}: {error}"),
            )
            .await;
            return;
        }
    }
    let _ = crate::agent_debug::record(
        state,
        new_pane_id,
        AgentDebugSource::Server,
        AgentDebugEventDraft::information(
            AgentDebugEventKind::PaneCreated,
            "Pane created by session conversion",
        )
        .with_fields(vec![
            AgentDebugField::plain("replaced pane", format!("{pane_id:?}")),
            AgentDebugField::plain("command", command_line),
            AgentDebugField::plain("working directory", cwd.display().to_string()),
        ]),
    )
    .await;
    // Serialize completion with close requests. If a close removed the old
    // pane while the replacement was starting, remove the replacement too;
    // otherwise close the old pane atomically with publishing the swap.
    let _close_spawn_guard = state.workspace_spawn_lock.lock().await;
    let old_pane_is_live = state.tree.read().await.get(pane_id).is_some();
    if old_pane_is_live && custody_ticket.is_some() {
        let mut panes = state.panes.write().await;
        if let Some(PaneResource::Terminal(runtime)) = panes.get_mut(&pane_id) {
            // The replacement runtime already owns a clone of this durable
            // ticket. Drop the old in-memory handle before closing the frozen
            // PTY so teardown cannot try to clear custody while the resumed
            // process is using the worktree.
            drop(runtime.custody_ticket.take());
        }
    }
    let completed_pane_id = if old_pane_is_live {
        pane_id
    } else {
        new_pane_id
    };
    handle_close_pane_with_spawn_guard(state, completed_pane_id, direct_tx).await;
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
    let publish_guard = state.workspace_spawn_lock.lock().await;
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
    drop(publish_guard);

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

/// Resolves the live portion of a client's starting-directory policy before
/// creating a pane. `None` means use the project root found under the same
/// tree write that inserts the pane, so its persisted cwd is never stale.
async fn candidate_new_pane_working_directory(
    state: &ServerState,
    working_directory: NewPaneWorkingDirectory,
) -> Option<std::path::PathBuf> {
    match working_directory {
        NewPaneWorkingDirectory::ProjectRoot => None,
        NewPaneWorkingDirectory::FocusedTerminal => {
            focused_terminal_working_directory_with_work(state, |process_id| {
                ilium_platform::process_info::working_directory(process_id)
            })
            .await
        }
        NewPaneWorkingDirectory::LastUsed => {
            state.last_terminal_working_directory.lock().await.clone()
        }
        NewPaneWorkingDirectory::WorkspacePane(pane_id) => {
            let tree = state.tree.read().await;
            tree.pane_workspace(pane_id)
                .and_then(|workspace| {
                    tree.pane_cwd(pane_id)
                        .filter(|cwd| *cwd == workspace.worktree_root.as_path())
                })
                .map(std::path::Path::to_path_buf)
        }
    }
}

/// Reading a live process directory can enter native process inspection on
/// Windows. Capture the focused PTY under a short guard, then inspect on the
/// finite IO bank and reject a late result after focus or lifetime changes.
async fn focused_terminal_working_directory_with_work<Work>(
    state: &ServerState,
    work: Work,
) -> Option<std::path::PathBuf>
where
    Work: FnOnce(u32) -> Option<std::path::PathBuf> + Send + 'static,
{
    let (pane_id, session_identity, process_id) = {
        let panes = state.panes.read().await;
        panes
            .iter()
            .find_map(|(pane_id, resource)| match resource {
                PaneResource::Terminal(runtime) if runtime.detection_schedule.client_focused => {
                    Some((
                        *pane_id,
                        runtime.session.identity(),
                        runtime.session.process_id()?,
                    ))
                }
                _ => None,
            })?
    };
    let execution = state.execution.get()?;
    let result = tokio::time::timeout(
        std::time::Duration::from_millis(500),
        execution.client.run(
            ilium_execution::Lane::Io,
            ilium_execution::JobCost {
                input_bytes: 4096,
                result_bytes: 128 * 1024,
            },
            move |_| -> Result<_, std::convert::Infallible> { Ok(work(process_id)) },
        ),
    )
    .await
    .ok()?
    .ok()?;
    let panes = state.panes.read().await;
    let Some(PaneResource::Terminal(runtime)) = panes.get(&pane_id) else {
        return None;
    };
    if !runtime.detection_schedule.client_focused
        || runtime.session.identity() != session_identity
        || runtime.session.process_id() != Some(process_id)
        || runtime.session.child_exit().is_some()
    {
        return None;
    }
    result.view().clone()
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
    #[error("pane node {0:?} was removed or refused admission before registration")]
    // Covers pre-spawn directory-generation rejection.
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
        .pane_cwd(pane_id)
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
    spawn_and_register_pane_with_deferred_workspace(state, pane_id, kind, cwd, None, None).await
}

/// Registers a replacement runtime that inherits an existing worktree
/// custody ticket. The caller retains the original ticket until the new
/// runtime has been registered successfully.
async fn spawn_and_register_pane_in_directory_with_custody_ticket(
    state: &Arc<ServerState>,
    pane_id: NodeId,
    kind: PaneSnapshotKind,
    cwd: &std::path::Path,
    custody_ticket: Option<crate::workspace_custody::CustodyTicket>,
) -> Result<(), RegisterPaneError> {
    spawn_and_register_pane_with_deferred_workspace(state, pane_id, kind, cwd, None, custody_ticket)
        .await
}

/// A missing saved worktree is restored as a project-root shell while its
/// original launch command remains available for a later safe recovery.
pub(crate) async fn spawn_and_register_pane_with_deferred_workspace(
    state: &Arc<ServerState>,
    pane_id: NodeId,
    kind: PaneSnapshotKind,
    cwd: &std::path::Path,
    deferred_workspace: Option<(TerminalOrigin, String)>,
    inherited_custody_ticket: Option<crate::workspace_custody::CustodyTicket>,
) -> Result<(), RegisterPaneError> {
    let (detection_config, _) = state.agent_detection_settings_snapshot().await;
    let before_admission = ilium_platform::secure_fs::spawn_directory_generation(cwd);
    let is_terminal = matches!(&kind, PaneSnapshotKind::Terminal(_));
    let repository_admission =
        crate::workspace_prune::spawn_repository_admission(state, cwd, is_terminal).await;
    let _spawn_guard = state.workspace_spawn_lock.lock().await;
    let after_admission = ilium_platform::secure_fs::spawn_directory_generation(cwd);
    let directory_unchanged = matches!(
        (&before_admission, &after_admission),
        (Ok(before), Ok(after)) if before == after
    );
    if repository_admission.is_err() || !directory_unchanged {
        let mut tree = state.tree.write().await;
        let panes = state.panes.read().await;
        // Never tear down an already registered resource as a side effect of failed admission.
        if !panes.contains_key(&pane_id) {
            let _ = tree.remove_node(pane_id);
        }
        drop(panes);
        drop(tree);
        let reason = repository_admission
            .as_ref()
            .err()
            .cloned()
            .unwrap_or_else(|| {
                "launch directory disappeared, changed identity, or is unsafe".into()
            });
        state.broadcast(ServerEvent::Error {
            message: format!("pane {} was not started: {reason}", pane_id.0),
        });
        broadcast_and_persist(state).await;
        return Err(RegisterPaneError::NodeRemoved(pane_id));
    }
    // A node removed while this caller waited must never cause even a transient PTY spawn.
    if state.tree.read().await.get(pane_id).is_none() {
        return Err(RegisterPaneError::NodeRemoved(pane_id));
    }
    // Reserve custody only after both admission and the node-generation
    // fence pass. It is still durable before the PTY launch begins.
    let custody_ticket = if is_terminal {
        if inherited_custody_ticket.is_some() {
            Ok(inherited_custody_ticket)
        } else {
            match repository_admission.as_ref().ok().and_then(Option::as_ref) {
                Some(admission) => admission.begin_custody().await,
                None => Ok(None),
            }
        }
    } else {
        Ok(None)
    };
    let custody_ticket = match custody_ticket {
        Ok(ticket) => ticket,
        Err(reason) => {
            let mut tree = state.tree.write().await;
            let panes = state.panes.read().await;
            if !panes.contains_key(&pane_id) {
                let _ = tree.remove_node(pane_id);
            }
            drop(panes);
            drop(tree);
            state.broadcast(ServerEvent::Error {
                message: format!("pane {} was not started: {reason}", pane_id.0),
            });
            broadcast_and_persist(state).await;
            return Err(RegisterPaneError::NodeRemoved(pane_id));
        }
    };
    let (resource, output_receiver) = match kind {
        PaneSnapshotKind::Editor { path } => (PaneResource::Editor { path }, None),
        PaneSnapshotKind::Terminal(origin) => {
            let worktree_root = if deferred_workspace.is_none() {
                state
                    .tree
                    .read()
                    .await
                    .pane_workspace(pane_id)
                    .map(|workspace| workspace.worktree_root.clone())
            } else {
                None
            };
            let identity = pane::PaneIdentityEnv {
                pane_id,
                session_name: &state.session_name,
                socket_path: &state.socket_path,
                worktree_root: worktree_root.as_deref(),
            };
            let execution = state.execution.get().ok_or_else(|| {
                PtyError::Io(
                    std::io::Error::other("server execution admission is not initialized").into(),
                )
            })?;
            let quota = execution.pty_quota_group();
            let spawned = pane::spawn_terminal_session(&origin, cwd, &identity, &quota)?;
            let pending_generated_session_id = spawned.session_id;
            let session = spawned.session;
            // Subscribe before registration so the receiver retains output
            // produced during the short registration window. The task itself
            // starts only after the runtime is addressable (below).
            let output_receiver = (session.subscribe_output_bytes(), session.input_handle());
            let mut runtime = crate::pane::TerminalPaneRuntime::new(
                session,
                origin,
                pending_generated_session_id,
                detection_config.idle_poll_interval,
            );
            runtime.custody_ticket = custody_ticket;
            if let Some((original_origin, reason)) = deferred_workspace {
                runtime.deferred_workspace_origin = Some(original_origin);
                runtime.missing_workspace = Some(reason);
            }
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
    if let Some((output_receiver, input)) = output_receiver {
        let forward_task = tokio::spawn(forward_output_bytes(
            Arc::clone(state),
            pane_id,
            output_receiver,
            input,
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
    receiver: tokio::sync::broadcast::Receiver<ilium_pty::PtyOutputChunk>,
    input: PtyInput,
) {
    let owner_status = input.subscribe_status();
    forward_output_with_owner_status(state, pane_id, receiver, input, owner_status).await;
}

async fn forward_output_with_owner_status(
    state: Arc<ServerState>,
    pane_id: NodeId,
    mut receiver: tokio::sync::broadcast::Receiver<ilium_pty::PtyOutputChunk>,
    input: PtyInput,
    mut owner_status: tokio::sync::watch::Receiver<OwnerStatus>,
) {
    let mut activity_gate = OutputActivityGate::new();
    let mut subscription_cache = TerminalSubscriptionCache::new();
    let mut text_trigger_tracker = crate::text_triggers::Matcher::default();
    // This is the newest sequence the forwarder already broadcast, or saw
    // while nobody subscribed. Lag recovery starts after it, never at the
    // beginning of the entire bounded journal.
    let mut covered_sequence = 0_u64;
    let (trigger_delivery_sender, trigger_delivery_receiver) = tokio::sync::mpsc::channel(64);
    let trigger_delivery_task = crate::task_guard::AbortOnDropHandle::new(tokio::spawn(
        crate::text_triggers::run_deliveries(
            std::sync::Arc::clone(&state),
            pane_id,
            input.clone(),
            trigger_delivery_receiver,
        ),
    ));
    let mut owner_finished = false;
    let mut drain_trigger_deliveries = false;
    loop {
        {
            let panes = state.panes.read().await;
            if !matches!(panes.get(&pane_id), Some(PaneResource::Terminal(runtime))
                if input.same_session(&runtime.session.input_handle()))
            {
                break;
            }
        }
        // Inspect the initial state too: failure may precede registration.
        if !owner_finished {
            let status = owner_status.borrow_and_update().clone();
            if let OwnerStatus::Stopped { reason, error } = status {
                owner_finished = true;
                if !matches!(reason, ShutdownReason::Requested | ShutdownReason::Eof) {
                    let message =
                        format!("Terminal input/output stopped for pane {pane_id:?}: {reason:?}");
                    let panes = state.panes.read().await;
                    let is_current = matches!(panes.get(&pane_id),
                        Some(PaneResource::Terminal(runtime))
                            if input.same_session(&runtime.session.input_handle()));
                    if is_current {
                        tracing::error!(pane_id = pane_id.0, %message, ?error, "PTY owner failed");
                        state.broadcast(ServerEvent::Error { message });
                    }
                }
            }
        }
        // Continue forwarding already-published bytes after a terminal status;
        // this task remains owned/cancelled by the same pane runtime as before.
        let received = tokio::select! {
            received = receiver.recv() => received,
            changed = owner_status.changed(), if !owner_finished => {
                if changed.is_err() {
                    owner_finished = true;
                }
                continue;
            }
        };
        match received {
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
                    covered_sequence = covered_sequence.max(first_chunk.sequence);
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
                        if sequence > covered_sequence {
                            if first_sequence == covered_sequence.saturating_add(1) {
                                state.broadcast(ServerEvent::ScreenUpdate {
                                    pane_id,
                                    first_sequence,
                                    sequence,
                                    bytes: bytes.clone(),
                                });
                                covered_sequence = sequence;
                            } else if let Some(sequence) = broadcast_terminal_recovery_after(
                                &state,
                                pane_id,
                                &input,
                                covered_sequence,
                            )
                            .await
                            {
                                covered_sequence = sequence;
                            }
                        }
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
                        if let Some(sequence) = broadcast_terminal_recovery_after(
                            &state,
                            pane_id,
                            &input,
                            covered_sequence,
                        )
                        .await
                        {
                            covered_sequence = sequence;
                        }
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
                    if let Some(sequence) =
                        broadcast_terminal_recovery_after(&state, pane_id, &input, covered_sequence)
                            .await
                    {
                        covered_sequence = sequence;
                    }
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                drain_trigger_deliveries = true;
                break;
            }
        }
    }
    drop(trigger_delivery_sender);
    if drain_trigger_deliveries {
        // Natural EOF closes admission and finishes already matched decisions.
        // Pane replacement/cancellation instead drops the abort-on-drop owner;
        // the semantic writer also fences its exact original PTY identity.
        if let Err(error) = trigger_delivery_task.join().await {
            tracing::error!(pane_id = pane_id.0, %error, "text trigger delivery owner failed during drain");
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

/// Repairs a forwarder gap with the exact tail it has not yet covered. Keep
/// the pane read lock through the synchronous broadcast so a replacement
/// cannot interleave its new tree state before this old-session event.
async fn broadcast_terminal_recovery_after(
    state: &ServerState,
    pane_id: NodeId,
    expected_input: &PtyInput,
    after_sequence: u64,
) -> Option<u64> {
    let panes = state.panes.read().await;
    let PaneResource::Terminal(runtime) = panes.get(&pane_id)? else {
        return None;
    };
    if !expected_input.same_session(&runtime.session.input_handle()) {
        return None;
    }
    let event = terminal_recovery_from_session(pane_id, &runtime.session, after_sequence)?;
    let through_sequence = match &event {
        ServerEvent::ScreenUpdate { sequence, .. } => *sequence,
        ServerEvent::TerminalReplay {
            through_sequence, ..
        } => *through_sequence,
        _ => return None,
    };
    state.broadcast(event);
    Some(through_sequence)
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
    terminal_recovery_from_session(pane_id, &runtime.session, after_sequence)
}

fn terminal_recovery_from_session(
    pane_id: NodeId,
    session: &ilium_pty::PtySession,
    after_sequence: u64,
) -> Option<ServerEvent> {
    match session.output_recovery_after(after_sequence)? {
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

pub(crate) fn teardown_pane_resource(_pane_id: NodeId, mut resource: PaneResource) {
    resource.abort_background_tasks();
    if let PaneResource::Terminal(runtime) = &mut resource {
        // The original child is signalled/reaped by its existing owned worker.
        // Refused native termination is logged there without blocking Tokio.
        runtime.session.request_shutdown();
    }
}

async fn teardown_closed_pane_resource(
    state: &Arc<ServerState>,
    pane_id: NodeId,
    resource: PaneResource,
) {
    let has_custody =
        matches!(&resource, PaneResource::Terminal(runtime) if runtime.custody_ticket.is_some());
    if !has_custody {
        teardown_pane_resource(pane_id, resource);
        return;
    }

    const MAX_CUSTODY_PROCESS_USERS: usize = 4096;
    const CUSTODY_SCAN_WORKING_BYTES: usize = 8 * 1024 * 1024;
    const CUSTODY_SCAN_RESULT_BYTES: usize = 64 * 1024;
    let Some(execution) = state
        .execution
        .get()
        .map(|execution| execution.client.clone())
    else {
        tracing::warn!(
            "pane {pane_id:?} closed while custody worker was unavailable; custody retained"
        );
        teardown_pane_resource(pane_id, resource);
        return;
    };
    let path_bytes = match &resource {
        PaneResource::Terminal(runtime) => runtime
            .custody_ticket
            .as_ref()
            .map(|ticket| ticket.worktree_root().as_os_str().len()),
        PaneResource::Editor { .. } | PaneResource::Unrestored(_) => None,
    };
    let Some(input_bytes) = path_bytes
        .and_then(|bytes| CUSTODY_SCAN_WORKING_BYTES.checked_add(bytes.saturating_mul(4)))
    else {
        tracing::warn!("pane {pane_id:?} closed with invalid custody scan input; custody retained");
        teardown_pane_resource(pane_id, resource);
        return;
    };
    let reservation = match execution
        .reserve(
            ilium_execution::Lane::Io,
            ilium_execution::JobCost {
                input_bytes,
                result_bytes: CUSTODY_SCAN_RESULT_BYTES,
            },
        )
        .await
    {
        Ok(reservation) => reservation,
        Err(error) => {
            tracing::warn!(
                "pane {pane_id:?} custody I/O admission failed: {error:?}; custody retained"
            );
            teardown_pane_resource(pane_id, resource);
            return;
        }
    };

    let result = execution
        .run_reserved(reservation, move |context: ilium_execution::JobContext| {
            let mut resource = resource;
            let proof = match &mut resource {
                PaneResource::Terminal(runtime) => {
                    let terminated = runtime
                        .session
                        .terminate_process_tree(std::time::Duration::from_secs(5))
                        .map_err(|error| {
                            format!("PTY descendants cannot be proven stopped: {error}")
                        });
                    terminated.and_then(|_| {
                        if context.stop_requested() {
                            return Err("custody close cancelled after PTY termination".into());
                        }
                        let Some(ticket) = runtime.custody_ticket.take() else {
                            return Err("custody ticket disappeared during close".into());
                        };
                        let users =
                            ilium_platform::process_control::processes_using_directory_bounded(
                                ticket.worktree_root(),
                                MAX_CUSTODY_PROCESS_USERS,
                            )
                            .map_err(|error| {
                                format!("worktree process scan unavailable: {error}")
                            })?;
                        if !users.is_empty() {
                            return Err(format!("worktree still has process users: {users:?}"));
                        }
                        ticket
                            .clear_after_proof_in_worker(&context)
                            .map_err(|error| format!("worktree custody clear failed: {error}"))
                    })
                }
                PaneResource::Editor { .. } | PaneResource::Unrestored(_) => {
                    Err("custody resource was not terminal".into())
                }
            };
            Ok::<_, std::convert::Infallible>((resource, proof))
        })
        .await;
    match result {
        Ok(completion) => {
            let ((resource, proof), retention) = completion.into_parts();
            if let Err(error) = proof {
                tracing::warn!(
                    "pane {pane_id:?} could not complete worktree custody cleanup; marker may remain: {error}"
                );
            }
            teardown_pane_resource(pane_id, resource);
            drop(retention);
        }
        Err(error) => {
            tracing::warn!("pane {pane_id:?} custody worker failed: {error}; custody retained");
        }
    }
}

async fn handle_close_pane(
    state: &Arc<ServerState>,
    pane_id: NodeId,
    direct_tx: &DirectEventSender,
) {
    let _close_spawn_guard = state.workspace_spawn_lock.lock().await;
    let is_replacing = state
        .replacing_panes
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .contains(&pane_id);
    if is_replacing {
        send_direct_error(
            direct_tx,
            format!("pane {pane_id:?} is already being replaced"),
        )
        .await;
        return;
    }
    handle_close_pane_with_spawn_guard(state, pane_id, direct_tx).await;
}

/// Removes a pane while the caller holds `workspace_spawn_lock`, making the
/// tree removal atomic with respect to replacement publication.
async fn handle_close_pane_with_spawn_guard(
    state: &Arc<ServerState>,
    pane_id: NodeId,
    direct_tx: &DirectEventSender,
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
    let mut closed_resources = Vec::new();
    for id in &descendant_pane_ids {
        if let Some(resource) = panes.remove(id) {
            closed_resources.push((*id, resource));
        }
    }
    drop(panes);
    drop(tree);
    for (id, resource) in closed_resources {
        crate::lifecycle_log::record(
            state,
            crate::lifecycle_log::LifecycleEvent::PaneClosed {
                pane_id: id.0,
                reason: "close_pane_request",
                resource: crate::lifecycle_log::describe_resource(&resource),
            },
        );
        teardown_closed_pane_resource(state, id, resource).await;
    }
    let mut preferences = state.workspace_close_preferences.write().await;
    for id in &descendant_pane_ids {
        preferences.remove(id);
    }
    drop(preferences);
    state.agent_debug.remove(&descendant_pane_ids).await;
    let mut workspace_git_statuses = state.workspace_git_status_cache.write().await;
    for id in &descendant_pane_ids {
        workspace_git_statuses.remove(id);
    }
    drop(workspace_git_statuses);

    broadcast_and_persist(state).await;
    state.scheduled_input_changed.notify_one();
}

async fn start_retained_workspace_prune(
    state: &Arc<ServerState>,
    request_id: u64,
    project: NodeId,
    target: ilium_ipc::WorkspacePruneTarget,
    mode: ilium_ipc::WorkspacePruneMode,
    branch_policy: ilium_ipc::WorkspacePruneBranchPolicy,
    direct_tx: &DirectEventSender,
) {
    let mutation_state = Arc::clone(state);
    let reply_sender = direct_tx.clone();
    let rejection_target = target.clone();
    let (start_tx, start_rx) = oneshot::channel();
    let handle = tokio::spawn(async move {
        if start_rx.await.is_err() {
            return;
        }
        let reply = crate::ipc::EventReply::Direct(&reply_sender);
        let result = crate::workspace_prune::remove_retained(
            &mutation_state,
            project,
            &target,
            &mode,
            branch_policy,
            Some(&reply),
        )
        .await;
        let event = ServerEvent::WorkspacePruneCompleted {
            request_id,
            project,
            target,
            result,
        };
        let _ = tokio::time::timeout(std::time::Duration::from_secs(5), reply.send(event)).await;
    });
    if state.track_workspace_mutation_task(handle) {
        let _ = start_tx.send(());
        return;
    }
    let result = crate::workspace_prune::blocked("session is shutting down; no mutation started");
    let event = ServerEvent::WorkspacePruneCompleted {
        request_id,
        project,
        target: rejection_target,
        result,
    };
    let _ = tokio::time::timeout(std::time::Duration::from_secs(5), direct_tx.send(event)).await;
}
/// Preserve the existing callers while transferring mutation lifetime to the shutdown drain.
async fn handle_workspace_removal(
    state: &Arc<ServerState>,
    request_id: u64,
    pane_id: NodeId,
    force_path: Option<std::path::PathBuf>,
    remove_branch: bool,
    direct_tx: &DirectEventSender,
) {
    let mutation_state = Arc::clone(state);
    let reply = direct_tx.clone();
    let (start_tx, start_rx) = oneshot::channel();
    let handle = tokio::spawn(async move {
        if start_rx.await.is_err() {
            return;
        }
        if reply.is_closed() {
            return;
        }
        complete_workspace_removal(
            &mutation_state,
            request_id,
            pane_id,
            force_path,
            remove_branch,
            &reply,
        )
        .await;
    });
    if state.track_workspace_mutation_task(handle) {
        let _ = start_tx.send(());
        return;
    }
    let event = ServerEvent::WorkspaceRemovalBlocked {
        request_id,
        pane_id,
        reasons: vec!["session is shutting down; no mutation started".into()],
    };
    let _ = tokio::time::timeout(std::time::Duration::from_secs(5), direct_tx.send(event)).await;
}

async fn complete_workspace_removal(
    // Finish one server-owned, shutdown-tracked removal task.
    state: &Arc<ServerState>,
    request_id: u64,
    pane_id: NodeId,
    force_path: Option<std::path::PathBuf>,
    remove_branch: bool,
    direct_tx: &DirectEventSender,
) {
    let outcome =
        crate::workspace::remove_workspace(state, pane_id, force_path, remove_branch).await;
    let (event, close, warning) = match outcome {
        crate::workspace::WorkspaceRemovalOutcome::Removed { branch_warning } => (
            ServerEvent::WorkspaceRemoved {
                request_id,
                pane_id,
            },
            true,
            branch_warning,
        ),
        crate::workspace::WorkspaceRemovalOutcome::Blocked {
            reasons,
            pane_closed,
        } => (
            ServerEvent::WorkspaceRemovalBlocked {
                request_id,
                pane_id,
                reasons,
            },
            pane_closed,
            None,
        ),
        crate::workspace::WorkspaceRemovalOutcome::Uncertain {
            mut reasons,
            pane_closed,
        } => {
            reasons.insert(0, "REMOVAL OUTCOME UNCERTAIN: this response does not establish that files or the checkout remain intact".into());
            (
                ServerEvent::WorkspaceRemovalBlocked {
                    request_id,
                    pane_id,
                    reasons,
                },
                pane_closed,
                None,
            )
        }
    };
    let pane_exists = state.tree.read().await.get(pane_id).is_some();
    if close && pane_exists {
        crate::lifecycle_log::record_close_request(state, pane_id, "workspace_removal");
        handle_close_pane(state, pane_id, direct_tx).await;
    }
    let _ = tokio::time::timeout(std::time::Duration::from_secs(5), direct_tx.send(event)).await;
    if let Some(message) = warning {
        let _ = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            direct_tx.send(ServerEvent::Error { message }),
        )
        .await;
    }
}

/// A pane still waiting to start keeps the requested size instead of
/// rejecting it, so the PTY opens at the client's real geometry later.
async fn remember_unrestored_pane_size(
    state: &Arc<ServerState>,
    pane_id: NodeId,
    rows: u16,
    cols: u16,
) -> bool {
    if !matches!(
        state.panes.read().await.get(&pane_id),
        Some(PaneResource::Unrestored(_))
    ) {
        return false;
    }
    let mut panes = state.panes.write().await;
    match panes.get_mut(&pane_id) {
        Some(PaneResource::Unrestored(unrestored)) => {
            unrestored.size = Some((rows, cols));
            true
        }
        _ => false,
    }
}

async fn handle_resize_pane(
    state: &Arc<ServerState>,
    pane_id: NodeId,
    rows: u16,
    cols: u16,
    cause: PaneResizeCause,
    direct_tx: &DirectEventSender,
) {
    if remember_unrestored_pane_size(state, pane_id, rows, cols).await {
        return;
    }
    let input = {
        let panes = state.panes.read().await;
        match panes.get(&pane_id) {
            Some(PaneResource::Terminal(runtime)) => Ok(runtime.session.input_handle()),
            Some(PaneResource::Editor { .. }) => {
                Err(format!("pane {pane_id:?} is an editor, not a terminal"))
            }
            Some(PaneResource::Unrestored(unrestored)) => {
                Err(unrestored.unavailable_message(pane_id))
            }
            None => Err(format!("no pane found for node {pane_id:?}")),
        }
    };
    let error_message = match input {
        Ok(input) => match input.resize(rows, cols) {
            Ok(receipt) => receipt
                .wait()
                .await
                .err()
                .map(|error| format!("failed to resize pane {pane_id:?}: {error}")),
            Err(error) => Some(format!(
                "failed to admit resize for pane {pane_id:?}: {error}"
            )),
        },
        Err(message) => Some(message),
    };

    if let Some(message) = error_message {
        tracing::error!(?pane_id, rows, cols, %message, "pane resize rejected");
        send_direct(
            direct_tx,
            ServerEvent::PaneResizeRejected {
                pane_id,
                rows,
                cols,
                message,
            },
        )
        .await;
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
    acknowledge_progress_outcome_for_input(state, pane_id, None).await;
}

async fn acknowledge_progress_outcome_for_input(
    state: &ServerState,
    pane_id: NodeId,
    expected_input: Option<&PtyInput>,
) {
    let acknowledged = {
        let mut tree = state.tree.write().await;
        let mut panes = state.panes.write().await;
        if let Some(expected) = expected_input {
            if !matches!(panes.get(&pane_id), Some(PaneResource::Terminal(runtime))
                if expected.same_session(&runtime.session.input_handle()))
            {
                return;
            }
        }
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
    is_user_directed: bool,
    prompt_epoch: Option<String>,
    direct_tx: &DirectEventSender,
) {
    let expected_input = {
        let panes = state.panes.read().await;
        match panes.get(&pane_id) {
            Some(PaneResource::Terminal(runtime)) => Some(runtime.session.input_handle()),
            _ => None,
        }
    };
    let result = write_key_input_with_origin(
        state,
        pane_id,
        bytes,
        submission,
        is_user_directed,
        prompt_epoch.as_deref(),
    )
    .await;
    if result.is_ok() && !bytes.is_empty() {
        if let Some(input) = expected_input.as_ref() {
            acknowledge_progress_outcome_for_input(state, pane_id, Some(input)).await;
        }
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
#[cfg(test)]
pub(crate) async fn write_key_input(
    state: &ServerState,
    pane_id: NodeId,
    bytes: &[u8],
    submission: Option<PromptSubmissionSource>,
) -> Result<(), String> {
    write_key_input_with_origin(state, pane_id, bytes, submission, false, None).await
}

async fn write_key_input_with_origin(
    state: &ServerState,
    pane_id: NodeId,
    bytes: &[u8],
    submission: Option<PromptSubmissionSource>,
    is_user_directed: bool,
    prompt_epoch: Option<&str>,
) -> Result<(), String> {
    if submission.is_some() && bytes.last() != Some(&b'\r') {
        return Err("prompt submission metadata requires a trailing Enter".to_owned());
    }
    if prompt_epoch.is_some() && (!is_user_directed || submission.is_none()) {
        return Err("prompt epoch requires a direct user Enter".to_owned());
    }
    if is_user_directed
        && !matches!(
            submission,
            None | Some(PromptSubmissionSource::Keyboard | PromptSubmissionSource::VoiceControl)
        )
    {
        return Err("user terminal input cannot claim automated submission".to_owned());
    }
    let input_gate = pane_input_gate(state, pane_id).await?;
    let _input_guard = input_gate.lock().await;
    let is_initial_prompt = submission == Some(PromptSubmissionSource::InitialAgentPrompt);
    write_key_input_unlocked(
        state,
        pane_id,
        bytes,
        submission,
        InputWriteOrigin {
            is_initial_prompt,
            is_user_directed,
            prompt_epoch,
            expected_invocation: None,
            required_ready_agent_class: None,
            required_statusline_generation: None,
        },
        &input_gate,
    )
    .await
}

/// Raw scheduled text and Enter-only countdowns use the same missing-worktree
/// fence as staged automated submissions, without restricting user keystrokes.
pub(crate) async fn write_scheduled_key_input(
    state: &ServerState,
    pane_id: NodeId,
    bytes: &[u8],
    submission: Option<PromptSubmissionSource>,
) -> Result<(), String> {
    let input_gate = pane_input_gate(state, pane_id).await?;
    let _input_guard = input_gate.lock().await;
    {
        let panes = state.panes.read().await;
        let Some(PaneResource::Terminal(runtime)) = panes.get(&pane_id) else {
            return Err(format!("pane {pane_id:?} closed before scheduled input"));
        };
        if !std::sync::Arc::ptr_eq(&input_gate, &runtime.input_gate) {
            return Err(format!("pane {pane_id:?} changed before scheduled input"));
        }
        if let Some(reason) = &runtime.missing_workspace {
            return Err(format!(
                "pane {pane_id:?} is waiting for its worktree; scheduled input was held: {reason}"
            ));
        }
    }
    write_key_input_unlocked(
        state,
        pane_id,
        bytes,
        submission,
        InputWriteOrigin {
            is_initial_prompt: false,
            is_user_directed: false,
            prompt_epoch: None,
            expected_invocation: None,
            required_ready_agent_class: None,
            required_statusline_generation: None,
        },
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
        Some(PaneResource::Unrestored(unrestored)) => Err(unrestored.unavailable_message(pane_id)),
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
    expected_session: &PtyInput,
) -> Result<bool, String> {
    let input_gate = pane_input_gate(state, pane_id).await?;
    let _input_guard = input_gate.lock().await;
    {
        let panes = state.panes.read().await;
        if !matches!(panes.get(&pane_id), Some(PaneResource::Terminal(runtime))
            if expected_session.same_session(&runtime.session.input_handle()))
        {
            return Ok(false);
        }
    }
    let current_target = {
        let accepted = state.text_trigger_settings.read().await;
        // Delayed deliveries outlive unrelated settings edits, so the fence is
        // the rule itself (identity, enabled, message) rather than the revision.
        accepted
            .settings
            .triggers
            .iter()
            .find(|trigger| {
                trigger.enabled && trigger.id == trigger_id && trigger.message == message
            })
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

fn invalidate_submitted_title_observation(
    tree: &mut Tree,
    runtime: &mut pane::TerminalPaneRuntime,
    pane_id: NodeId,
) -> bool {
    // An acknowledged Enter supersedes earlier inference even when terminal
    // editing prevents exact-text recovery. Unknown input never grants a title.
    runtime.authored_title_receipt = None;
    match tree.invalidate_presentation(pane_id) {
        Ok(()) => true,
        Err(error) => {
            tracing::warn!(pane_id = pane_id.0, %error, "submitted task cannot advance title observation");
            false
        }
    }
}

fn record_authored_title_receipt(
    tree: &mut Tree,
    runtime: &mut pane::TerminalPaneRuntime,
    pane_id: NodeId,
    text: &str,
    source: PromptSubmissionSource,
) -> bool {
    if !matches!(
        source,
        PromptSubmissionSource::Keyboard
            | PromptSubmissionSource::VoiceControl
            | PromptSubmissionSource::QueuedPrompt
            | PromptSubmissionSource::ScheduledInput
            | PromptSubmissionSource::InitialAgentPrompt
    ) {
        return false;
    }
    let Some(process) = runtime.agent_process_key.as_ref() else {
        return false;
    };
    let entry = match process.class {
        ilium_core::AgentClass::Codex => {
            serde_json::json!({"type":"event_msg", "payload":{"type":"user_message", "message":text}})
        }
        ilium_core::AgentClass::Claude => {
            serde_json::json!({"type":"user", "message":{"content":text}})
        }
        ilium_core::AgentClass::Antigravity => serde_json::json!({"display":text}),
        ilium_core::AgentClass::Other(_) => return false,
    };
    if ilium_agent_session::genuine_request_text(&process.class, &entry).is_none() {
        return false;
    }
    let Some(node) = tree.get(pane_id) else {
        return false;
    };
    let snapshot = TitleRuntimeSnapshot::capture(node, runtime, false);
    let Some(receipt) = title_eligibility::AuthoredRequestReceipt::for_invocation(&snapshot) else {
        return false;
    };
    runtime.authored_title_receipt = Some(receipt);
    true
}

pub(crate) async fn submit_terminal_text_locked(
    state: &ServerState,
    pane_id: NodeId,
    text: &str,
    source: PromptSubmissionSource,
    input_gate: &std::sync::Arc<tokio::sync::Mutex<()>>,
) -> Result<(), String> {
    {
        let panes = state.panes.read().await;
        let Some(PaneResource::Terminal(runtime)) = panes.get(&pane_id) else {
            return Err(format!("pane {pane_id:?} closed before text insertion"));
        };
        if !std::sync::Arc::ptr_eq(input_gate, &runtime.input_gate) {
            return Err(format!("pane {pane_id:?} changed before text insertion"));
        }
    }
    let wants_bracketed_paste =
        crate::pane::read_current_terminal_screen(state, pane_id, vt100::Screen::bracketed_paste)
            .await
            .ok_or_else(|| format!("pane {pane_id:?} has no current screen for text insertion"))?;
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
    let expected_invocation = {
        let panes = state.panes.read().await;
        let Some(PaneResource::Terminal(runtime)) = panes.get(&pane_id) else {
            return Err(format!("pane {pane_id:?} closed before automated input"));
        };
        if !std::sync::Arc::ptr_eq(input_gate, &runtime.input_gate) {
            return Err(format!("pane {pane_id:?} changed before automated input"));
        }
        if !matches!(
            runtime.session.input_handle().status(),
            OwnerStatus::Running
        ) {
            return Err(format!(
                "pane {pane_id:?} PTY owner stopped before automated input"
            ));
        }
        if let Some(reason) = &runtime.missing_workspace {
            return Err(format!(
                "pane {pane_id:?} is waiting for its worktree; automated input was held: {reason}"
            ));
        }
        let input_cancel_generation = *runtime.agent_input_cancel.borrow();
        AgentInputInvocation {
            generation: runtime.agent_generation,
            process: runtime.agent_process_key.clone(),
            input_cancel_generation,
        }
    };
    let is_initial_prompt = source == PromptSubmissionSource::InitialAgentPrompt;
    if !body.is_empty() {
        write_key_input_unlocked(
            state,
            pane_id,
            body,
            None,
            InputWriteOrigin {
                is_initial_prompt,
                is_user_directed: false,
                prompt_epoch: None,
                expected_invocation: Some(&expected_invocation),
                required_ready_agent_class: None,
                required_statusline_generation: None,
            },
            input_gate,
        )
        .await?;
        tokio::time::sleep(AUTOMATED_ENTER_DELAY).await;
    }
    {
        let panes = state.panes.read().await;
        let Some(PaneResource::Terminal(runtime)) = panes.get(&pane_id) else {
            return Err(format!("pane {pane_id:?} closed before automated input"));
        };
        if !std::sync::Arc::ptr_eq(input_gate, &runtime.input_gate) {
            return Err(format!("pane {pane_id:?} changed before automated input"));
        }
        if !matches!(
            runtime.session.input_handle().status(),
            OwnerStatus::Running
        ) {
            return Err(format!(
                "pane {pane_id:?} PTY owner stopped before automated input"
            ));
        }
        if let Some(reason) = &runtime.missing_workspace {
            return Err(format!(
                "pane {pane_id:?} is waiting for its worktree; automated input was held: {reason}"
            ));
        }
    }
    write_key_input_unlocked(
        state,
        pane_id,
        b"\r",
        Some(source),
        InputWriteOrigin {
            is_initial_prompt,
            is_user_directed: false,
            prompt_epoch: None,
            expected_invocation: Some(&expected_invocation),
            required_ready_agent_class: None,
            required_statusline_generation: None,
        },
        input_gate,
    )
    .await?;
    let changed = {
        let mut tree = state.tree.write().await;
        let mut panes = state.panes.write().await;
        match panes.get_mut(&pane_id) {
            Some(PaneResource::Terminal(runtime))
                if runtime.agent_generation == expected_invocation.generation
                    && runtime.agent_process_key == expected_invocation.process
                    && Arc::ptr_eq(input_gate, &runtime.input_gate) =>
            {
                let text = std::str::from_utf8(body)
                    .unwrap_or_default()
                    .trim_start_matches("\x1b[200~")
                    .trim_end_matches("\x1b[201~");
                let changed = invalidate_submitted_title_observation(&mut tree, runtime, pane_id);
                if changed {
                    record_authored_title_receipt(&mut tree, runtime, pane_id, text, source);
                }
                changed
            }
            _ => false,
        }
    };
    if changed {
        broadcast_pane_and_persist(state, pane_id).await;
    }
    state.broadcast(ServerEvent::PanePromptSubmitted { pane_id, source });
    Ok(())
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

/// Updates a stopped pane's recovery object in the same receipt-backed
/// transaction as its tree prompt. Same-invocation evidence survives a crash
/// while an unrelated replacement can never inherit it.
fn update_unavailable_recovery_prompt(
    tree: &mut Tree,
    pane_id: NodeId,
    owner: &AgentProcessKey,
    latest: Option<&str>,
    previous: Option<&str>,
    latest_unavailable: bool,
    state: &ServerState,
) {
    let Some(NodeKind::Pane {
        status: PaneStatus::AgentUnavailable(recovery),
        ..
    }) = tree.get(pane_id).map(|node| &node.kind)
    else {
        return;
    };
    if &recovery.process != owner {
        return;
    }
    let mut recovery = (**recovery).clone();
    set_recovery_prompt(&mut recovery, latest, previous, latest_unavailable);
    let status = PaneStatus::AgentUnavailable(Box::new(recovery));
    if tree.set_pane_status(pane_id, status.clone()).is_ok() {
        state.request_snapshot_save();
        state.broadcast(ServerEvent::PaneStatusChanged { pane_id, status });
    }
}

fn set_recovery_prompt(
    recovery: &mut AgentRecovery,
    latest: Option<&str>,
    previous: Option<&str>,
    latest_unavailable: bool,
) {
    recovery.last_prompt = latest.map(str::to_owned);
    recovery.previous_exact_prompt = latest_unavailable
        .then(|| previous.map(str::to_owned))
        .flatten();
    recovery.latest_prompt_unavailable = latest_unavailable;
}

/// A cancelled or partially known PTY write may already have reached the
/// composer. Fence its transcript epoch and never replay it as exact input.
async fn invalidate_uncertain_agent_input(
    state: &ServerState,
    pane_id: NodeId,
    expected_input_gate: &std::sync::Arc<tokio::sync::Mutex<()>>,
    expected_generation: u64,
    expected_process: &Option<AgentProcessKey>,
    possibly_submitted: bool,
) {
    let mut tree = state.tree.write().await;
    let mut panes = state.panes.write().await;
    let Some(PaneResource::Terminal(runtime)) = panes.get_mut(&pane_id) else {
        return;
    };
    if !Arc::ptr_eq(expected_input_gate, &runtime.input_gate)
        || runtime.agent_generation != expected_generation
        || &runtime.agent_process_key != expected_process
    {
        return;
    }
    runtime
        .agent_prompt_tracker
        .observe_unattributed_written(&[]);
    runtime.prompt_transcript_epoch = None;
    runtime.legacy_prompt_fallback_blocked = true;
    if possibly_submitted {
        runtime.latest_agent_prompt_unavailable = true;
        if tree.set_last_prompt(pane_id, None).is_ok() {
            state.request_snapshot_save();
            state.broadcast(ServerEvent::PaneLastPromptChanged {
                pane_id,
                last_prompt: None,
            });
        }
        if let Some(owner) = expected_process.as_ref() {
            update_unavailable_recovery_prompt(
                &mut tree,
                pane_id,
                owner,
                None,
                runtime.last_agent_prompt.as_deref(),
                true,
                state,
            );
        }
    }
}

/// One automatic submission's body and Enter belong to the same invocation,
/// even when detection replaces the agent without replacing its PTY.
pub(crate) struct AgentInputInvocation {
    pub(crate) generation: u64,
    pub(crate) process: Option<AgentProcessKey>,
    pub(crate) input_cancel_generation: u64,
}

/// Provenance retained across admission and the ordered delivery receipt.
pub(crate) struct InputWriteOrigin<'a> {
    pub(crate) is_initial_prompt: bool,
    pub(crate) is_user_directed: bool,
    pub(crate) prompt_epoch: Option<&'a str>,
    pub(crate) expected_invocation: Option<&'a AgentInputInvocation>,
    pub(crate) required_ready_agent_class: Option<ilium_core::AgentClass>,
    pub(crate) required_statusline_generation: Option<u64>,
}

/// The established title, session-identity, activity and event path for one
/// physical PTY write. Call only while holding this pane's `input_gate`.
pub(crate) async fn write_key_input_unlocked(
    state: &ServerState,
    pane_id: NodeId,
    bytes: &[u8],
    submission: Option<PromptSubmissionSource>,
    origin: InputWriteOrigin<'_>,
    expected_input_gate: &std::sync::Arc<tokio::sync::Mutex<()>>,
) -> Result<(), String> {
    write_key_input_unlocked_with_probe(
        state,
        pane_id,
        bytes,
        submission,
        origin,
        expected_input_gate,
        |request| foreground_observation::observe(state, request),
    )
    .await
}

async fn write_key_input_unlocked_with_probe<Probe, ProbeFuture>(
    state: &ServerState,
    pane_id: NodeId,
    bytes: &[u8],
    submission: Option<PromptSubmissionSource>,
    origin: InputWriteOrigin<'_>,
    expected_input_gate: &std::sync::Arc<tokio::sync::Mutex<()>>,
    probe: Probe,
) -> Result<(), String>
where
    Probe: FnOnce(ProbeRequest) -> ProbeFuture,
    ProbeFuture:
        std::future::Future<Output = Result<ProbeObservation, foreground_observation::ProbeError>>,
{
    write_key_input_unlocked_with_marker(
        state,
        pane_id,
        bytes,
        submission,
        origin,
        expected_input_gate,
        probe,
        None,
    )
    .await
}

pub(crate) async fn write_key_input_unlocked_with_screen_marker(
    state: &ServerState,
    pane_id: NodeId,
    bytes: &[u8],
    submission: Option<PromptSubmissionSource>,
    origin: InputWriteOrigin<'_>,
    expected_input_gate: &std::sync::Arc<tokio::sync::Mutex<()>>,
    screen_changed: &mut tokio::sync::watch::Receiver<()>,
) -> Result<(), String> {
    write_key_input_unlocked_with_marker(
        state,
        pane_id,
        bytes,
        submission,
        origin,
        expected_input_gate,
        |request| foreground_observation::observe(state, request),
        Some(screen_changed),
    )
    .await
}

async fn write_key_input_unlocked_with_marker<Probe, ProbeFuture>(
    state: &ServerState,
    pane_id: NodeId,
    bytes: &[u8],
    submission: Option<PromptSubmissionSource>,
    origin: InputWriteOrigin<'_>,
    expected_input_gate: &std::sync::Arc<tokio::sync::Mutex<()>>,
    probe: Probe,
    mut screen_changed: Option<&mut tokio::sync::watch::Receiver<()>>,
) -> Result<(), String>
where
    Probe: FnOnce(ProbeRequest) -> ProbeFuture,
    ProbeFuture:
        std::future::Future<Output = Result<ProbeObservation, foreground_observation::ProbeError>>,
{
    let InputWriteOrigin {
        is_initial_prompt,
        is_user_directed,
        prompt_epoch,
        expected_invocation,
        required_ready_agent_class,
        required_statusline_generation,
    } = origin;
    // The tracker below decides whether these bytes actually completed a
    // semantic line. Looking for CR/LF here would misclassify newlines inside
    // a bracketed paste as submissions.
    let mut submission_correlation_id = None;

    // The pane gate serializes other input, but detection and replacement can
    // still change its runtime while native observation is pending. Capture a
    // session-bound request under the registry guard, inspect on the existing
    // finite IO bank, then compare again at the admission point below.
    let (probe_request, preflight_cancel_generation) = {
        let panes = state.panes.read().await;
        let Some(PaneResource::Terminal(runtime)) = panes.get(&pane_id) else {
            return Err(format!("pane {pane_id:?} closed before input preflight"));
        };
        if !Arc::ptr_eq(expected_input_gate, &runtime.input_gate) {
            return Err(format!("pane {pane_id:?} changed before input preflight"));
        }
        let input_cancel_generation = *runtime.agent_input_cancel.borrow();
        (ProbeRequest::for_runtime(runtime), input_cancel_generation)
    };
    let probe_observation = if probe_request.needs_inspection() {
        match probe(probe_request).await {
            Ok(observed) => Some(observed),
            Err(error) => {
                tracing::debug!(pane_id = pane_id.0, %error, "input preflight unavailable");
                None
            }
        }
    } else {
        None
    };

    // Validate current ownership immediately before PTY admission. The pane
    // input gate is held by the caller, while global locks are dropped before
    // waiting for the ordered writer receipt.
    let (
        input,
        was_shell_foreground,
        is_automatic_plain_shell,
        expected_generation,
        expected_process,
        prewrite_agent_class,
        mut cancel_automated,
        statusline_receipt,
    ) = {
        let tree = state.tree.read().await;
        let mut panes = state.panes.write().await;
        let Some(PaneResource::Terminal(runtime)) = panes.get_mut(&pane_id) else {
            return Err(format!("pane {pane_id:?} closed before input delivery"));
        };
        if !Arc::ptr_eq(expected_input_gate, &runtime.input_gate) {
            return Err(format!("pane {pane_id:?} changed before input delivery"));
        }
        if expected_invocation.is_some_and(|expected| {
            runtime.agent_generation != expected.generation
                || runtime.agent_process_key != expected.process
                || *runtime.agent_input_cancel.borrow() != expected.input_cancel_generation
        }) {
            return Err(format!(
                "automatic input refused for pane {pane_id:?}: agent invocation changed during submission"
            ));
        }
        let Some(NodeKind::Pane {
            status,
            title_source,
            ..
        }) = tree.get(pane_id).map(|node| &node.kind)
        else {
            return Err(format!("pane {pane_id:?} has no terminal state"));
        };
        let is_automatic_plain_shell =
            matches!(status, PaneStatus::PlainShell) && *title_source == PaneTitleSource::Automatic;
        if let (Some(agent_class), Some(statusline_generation)) = (
            required_ready_agent_class.as_ref(),
            required_statusline_generation,
        ) {
            if runtime.antigravity_statusline_generation != statusline_generation
                || runtime
                    .agent_process_key
                    .as_ref()
                    .is_none_or(|process| process.class != *agent_class)
                || runtime.verified_agent_exit.is_some()
                || !crate::agent_delivery::runtime_has_ready_composer(runtime)
            {
                return Err(format!(
                    "Antigravity status-line delivery refused for pane {pane_id:?}: composer or generation changed"
                ));
            }
        }
        let cancelled_during_preflight =
            *runtime.agent_input_cancel.borrow() != preflight_cancel_generation;
        let rejection = if cancelled_during_preflight && runtime.agent_process_key.is_some() {
            Some("agent input ownership changed during preflight".to_string())
        } else {
            runtime.automated_agent_input_rejection(status, probe_observation.as_ref())
        };
        let active_agent = runtime.agent_process_key.is_some() && rejection.is_none();
        if !is_user_directed {
            if let Some(reason) = rejection {
                return Err(format!(
                    "automatic input refused for pane {pane_id:?}: {reason}"
                ));
            }
        }
        let prewrite_agent_class = active_agent
            .then(|| status.agent_state().map(|agent| agent.class.clone()))
            .flatten();
        if !bytes.is_empty() && !is_initial_prompt {
            runtime.cancel_initial_prompt_delivery();
        }
        let shell_foreground = matches!(&runtime.origin, TerminalOrigin::PlainShell)
            && probe_observation
                .as_ref()
                .filter(|observed| observed.same_runtime(runtime))
                .is_some_and(|observed| observed.shell_owns_terminal() == Some(true));
        let statusline_receipt = if required_statusline_generation.is_some() {
            if let Some(screen_changed) = screen_changed.as_deref_mut() {
                let _ = *screen_changed.borrow_and_update();
            }
            Some(
                runtime
                    .session
                    .input_handle()
                    .write(bytes)
                    .map_err(|error| {
                        format!("failed to admit status-line input for pane {pane_id:?}: {error}")
                    })?,
            )
        } else {
            None
        };
        (
            runtime.session.input_handle(),
            shell_foreground,
            is_automatic_plain_shell,
            runtime.agent_generation,
            runtime.agent_process_key.clone(),
            prewrite_agent_class,
            runtime.agent_input_cancel.subscribe(),
            statusline_receipt,
        )
    };
    let receipt = match statusline_receipt {
        Some(receipt) => receipt,
        None => input
            .write(bytes)
            .map_err(|error| format!("failed to admit input for pane {pane_id:?}: {error}"))?,
    };
    let delivered = if is_user_directed || expected_process.is_none() {
        Some(receipt.wait().await)
    } else {
        tokio::select! {
            biased;
            result = receipt.wait() => Some(result),
            _ = cancel_automated.changed() => None,
        }
    };
    let Some(delivered) = delivered else {
        // Dropping the receipt requests cancellation. A prefix may have
        // reached the PTY; its next transcript record is not an exact proof.
        invalidate_uncertain_agent_input(
            state,
            pane_id,
            expected_input_gate,
            expected_generation,
            &expected_process,
            bytes.last() == Some(&b'\r') && prewrite_agent_class.is_some(),
        )
        .await;
        return Err(format!(
            "automatic input cancelled for pane {pane_id:?}; delivery disposition uncertain"
        ));
    };
    if let Err(error) = delivered {
        if !error.proves_zero_delivery() {
            invalidate_uncertain_agent_input(
                state,
                pane_id,
                expected_input_gate,
                expected_generation,
                &expected_process,
                bytes.last() == Some(&b'\r') && prewrite_agent_class.is_some(),
            )
            .await;
        }
        return Err(format!(
            "failed to deliver input for pane {pane_id:?}: {error}"
        ));
    }

    // Write lock (not read) on `panes`: a `KeyInput` always targets the
    // client's currently-focused pane (the client only ever forwards raw
    // keys for `self.focused_pane`), which `ClientRequest::SetPaneFocus`
    // already puts on the focused fast tier regardless of status -- so most
    // keystrokes need no extra scheduling push here. Enter is the
    // exception: it's the clearest possible signal a command/prompt was
    // just submitted, so it still forces an immediate (debounced) recheck
    // below, rather than waiting up to one base tick.
    // This is `state.panes`' write lock, held by every pane's input handling;
    // release it before any later asynchronous bookkeeping.
    let mut tree = state.tree.write().await;
    let mut panes = state.panes.write().await;
    let mut observed_title = None;
    let mut authored_title_receipt_changed = false;
    let mut cleared_session_origin_name = None;
    let mut cleared_session_title_generation = None;
    let mut cleared_conversation_title_generation = None;
    let mut detection_was_forced = false;
    let tracked_submission;
    let mut goal_was_cleared = false;
    let mut session_transition_observation = None;
    let mut conversation_title_generation_before = None;
    match panes.get_mut(&pane_id) {
        Some(PaneResource::Terminal(runtime))
            if !Arc::ptr_eq(expected_input_gate, &runtime.input_gate)
                || !input.same_session(&runtime.session.input_handle())
                || runtime.agent_generation != expected_generation
                || runtime.agent_process_key != expected_process =>
        {
            // The completed delivery belongs to the old runtime. It must
            // never be reported as a retryable failure or booked on the new.
            return Ok(());
        }
        Some(PaneResource::Terminal(runtime)) => {
            // A typed command only becomes a title while the shell itself owns
            // the terminal, which is how "the user is typing at a prompt" is
            // told apart from "a running command owns the terminal". A
            // platform that cannot tell answers `None`, and this stays false:
            // inferring a title without knowing who owns the terminal would
            // retitle panes from keystrokes typed into a running program.
            let should_track_title = is_automatic_plain_shell && was_shell_foreground;
            {
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
                if prewrite_agent_class.is_some() && !bytes.is_empty() {
                    // A delayed unfenced bootstrap report cannot claim a
                    // transcript line after this invocation received input.
                    runtime.legacy_prompt_fallback_blocked = true;
                }
                if !is_user_directed && !bytes.is_empty() {
                    runtime.prompt_transcript_epoch = None;
                }
                if prewrite_agent_class.is_some() {
                    let observed = if is_user_directed {
                        runtime.agent_prompt_tracker.observe_written(bytes)
                    } else {
                        runtime
                            .agent_prompt_tracker
                            .observe_unattributed_written(bytes)
                    };
                    if let Some(prompt) = observed {
                        runtime.legacy_prompt_fallback_blocked = true;
                        runtime.prompt_transcript_epoch = None;
                        let title_observation_advanced = is_user_directed
                            && invalidate_submitted_title_observation(&mut tree, runtime, pane_id);
                        authored_title_receipt_changed |= title_observation_advanced;
                        match prompt.exact_text {
                            Some(text) if !text.is_empty() => {
                                if title_observation_advanced {
                                    record_authored_title_receipt(
                                        &mut tree,
                                        runtime,
                                        pane_id,
                                        &text,
                                        submission.unwrap_or(PromptSubmissionSource::Keyboard),
                                    );
                                }
                                runtime.last_agent_prompt = Some(text.clone());
                                runtime.latest_agent_prompt_unavailable = false;
                                if tree.set_last_prompt(pane_id, Some(text.clone())).is_ok() {
                                    state.request_snapshot_save();
                                    state.broadcast(ServerEvent::PaneLastPromptChanged {
                                        pane_id,
                                        last_prompt: Some(text.clone()),
                                    });
                                }
                                if let Some(owner) = expected_process.as_ref() {
                                    update_unavailable_recovery_prompt(
                                        &mut tree,
                                        pane_id,
                                        owner,
                                        Some(&text),
                                        runtime.last_agent_prompt.as_deref(),
                                        false,
                                        state,
                                    );
                                }
                            }
                            Some(_) => {}
                            None => {
                                runtime.latest_agent_prompt_unavailable = true;
                                if tree.set_last_prompt(pane_id, None).is_ok() {
                                    state.request_snapshot_save();
                                    state.broadcast(ServerEvent::PaneLastPromptChanged {
                                        pane_id,
                                        last_prompt: None,
                                    });
                                }
                                if let Some(owner) = expected_process.as_ref() {
                                    update_unavailable_recovery_prompt(
                                        &mut tree,
                                        pane_id,
                                        owner,
                                        None,
                                        runtime.last_agent_prompt.as_deref(),
                                        true,
                                        state,
                                    );
                                }
                            }
                        }
                    }
                    if submission.is_some() {
                        runtime.prompt_transcript_epoch = prompt_epoch.and_then(|token| {
                            Some(crate::pane::PromptTranscriptEpoch {
                                token: token.to_owned(),
                                generation: runtime.agent_generation,
                                process: expected_process.clone()?,
                                session_id: runtime.session_id.clone()?,
                            })
                        });
                    }
                }
                if prewrite_agent_class.is_some()
                    && submitted_line
                        .as_deref()
                        .is_some_and(crate::pane::clears_agent_goal)
                {
                    // The successful PTY write is authoritative user intent.
                    // Clear retained ownership immediately so a footer-hidden
                    // `/goal clear` cannot leave a sticky sidebar flag.
                    runtime.confirmed_goal_owner = None;
                    goal_was_cleared = true;
                }
                let active_agent_class = prewrite_agent_class.clone();
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
                    runtime.authored_title_receipt = None;
                    runtime.is_session_identity_invalidated = true;
                    runtime.prompt_transcript_epoch = None;
                    runtime.pending_generated_session_id = None;
                    runtime.title_generation = runtime.title_generation.saturating_add(1);
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
            }
        }
        Some(PaneResource::Editor { .. } | PaneResource::Unrestored(_)) | None => return Ok(()),
    };
    drop(panes);
    drop(tree);

    if detection_was_forced {
        state.detection_schedule_changed.notify_one();
    }

    // Fresh terminal input also acknowledges a completed turn. This
    // conditional tree transition cannot overwrite a concurrent detector's
    // newer Working/Waiting state.
    if !bytes.is_empty() {
        if let Err(error) = record_input_activity_if_current(state, pane_id, &input).await {
            // The PTY write already succeeded. A later tree mutation must not
            // turn this into a retryable delivery failure for queued work.
            tracing::warn!(pane_id = pane_id.0, %error, "input activity bookkeeping failed after PTY write");
        }
        let acknowledged_status = {
            let mut tree = state.tree.write().await;
            let panes = state.panes.read().await;
            let is_current = matches!(panes.get(&pane_id),
                Some(PaneResource::Terminal(runtime))
                    if input.same_session(&runtime.session.input_handle()));
            if !is_current {
                None
            } else {
                match tree.acknowledge_agent_completion(pane_id) {
                    Ok(status) => status,
                    Err(error) => {
                        tracing::error!(
                            "agent completion acknowledgement rejected for pane {pane_id:?}: {error}"
                        );
                        None
                    }
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
    if authored_title_receipt_changed {
        broadcast_pane_and_persist(state, pane_id).await;
    }
    if let Some(source) = submission.filter(|_| expected_invocation.is_none()) {
        state.broadcast(ServerEvent::PanePromptSubmitted { pane_id, source });
    }

    // A session-transition reset takes precedence over a shell title from
    // the same byte batch. In practice they are mutually exclusive, but the
    // ordering makes the stale LLM title impossible to retain if input and
    // foreground detection race.
    let tree_changed = {
        let mut tree = state.tree.write().await;
        let panes = state.panes.read().await;
        if !matches!(panes.get(&pane_id), Some(PaneResource::Terminal(runtime))
            if input.same_session(&runtime.session.input_handle()))
        {
            return Ok(());
        }
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
    direct_tx: &DirectEventSender,
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
    let input = {
        let panes = state.panes.read().await;
        match panes.get(&pane_id) {
            Some(PaneResource::Terminal(runtime))
                if Arc::ptr_eq(&input_gate, &runtime.input_gate) =>
            {
                Ok(runtime.session.input_handle())
            }
            Some(PaneResource::Terminal(_)) => {
                Err(format!("pane {pane_id:?} changed before mouse input"))
            }
            Some(PaneResource::Editor { .. }) => {
                Err(format!("pane {pane_id:?} is an editor, not a terminal"))
            }
            Some(PaneResource::Unrestored(unrestored)) => {
                Err(unrestored.unavailable_message(pane_id))
            }
            None => Err(format!("no pane found for node {pane_id:?}")),
        }
    };
    let (input, error_message) = match input {
        Ok(input) => {
            let event = to_crossterm_event(kind, column, row, modifiers);
            let error = match input.write_mouse_input(event, column, row) {
                Ok(receipt) => receipt.wait().await.err(),
                Err(error) => Some(error),
            };
            (
                Some(input),
                error.map(|error| {
                    format!("failed to forward mouse input to pane {pane_id:?}: {error}")
                }),
            )
        }
        Err(message) => (None, Some(message)),
    };
    drop(_input_guard);

    if let Some(message) = error_message {
        send_direct_error(direct_tx, message).await;
    } else if let Some(input) = input {
        if let Err(error) = record_input_activity_if_current(state, pane_id, &input).await {
            send_direct_error(direct_tx, error).await;
        }
    }
}

async fn handle_kill_session(state: &Arc<ServerState>) -> crate::recovery::Resolution {
    // A kill must finish or refuse the accepted restore before clearing its
    // tree and deleting its file; otherwise that restore could republish panes.
    state.recovery.close_and_drain().await?;
    let mut tree = state.tree.write().await;
    // Fence server-owned workspace tasks while holding the same tree lock
    // they need for pane commit. Any earlier commit is cleared below; any
    // later one observes creation closed and rolls its checkout back.
    state.mark_session_killed();
    *tree = Tree::new();
    let snapshot = tree.clone();
    // Lock ordering: `tree` before `panes` (see `ServerState` docs) --
    // held together here even though the two teardown steps are logically
    // independent, so this handler never has to be re-checked if that
    // ordering rule changes elsewhere.
    let mut panes = state.panes.write().await;
    // Move the existing registry without allocating a second resource list.
    // Child termination and resource destructors run after both global guards
    // are released, as they do on the individual pane-close path.
    let closed_resources = std::mem::take(&mut *panes);
    drop(panes);
    drop(tree);
    for (pane_id, resource) in closed_resources {
        teardown_pane_resource(pane_id, resource);
    }
    state.workspace_close_preferences.write().await.clear();
    state.agent_debug.clear().await;

    state.broadcast(ServerEvent::TreeSnapshot(snapshot));

    // The kill fence above also refuses every later `request_snapshot_save` -- `crate::run`'s
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
    {
        let write_guard = Arc::clone(&state.snapshot_write_lock).lock_owned().await;
        match crate::persistence::remove_snapshot_ordered(state, write_guard).await {
            Ok(_write_guard) => {}
            Err(error) => tracing::warn!("failed to remove snapshot file on session kill: {error}"),
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
    Ok(())
}

#[cfg(test)]
mod tests {
    fn snapshot_io_handler_state(directory: &tempfile::TempDir) -> Arc<ServerState> {
        let (sound_requests, _sound_receiver) = crate::sounds::test_channel(1);
        Arc::new(ServerState::new(crate::state::ServerStateOptions {
            session_name: "snapshot-handler-test".into(),
            session_cwd: directory.path().to_owned(),
            home_dir: directory.path().to_owned(),
            snapshot_path: directory.path().join("session.json"),
            socket_path: directory.path().join("isolated.sock"),
            detection_config: crate::config::DetectionConfig::default(),
            notifications_config: crate::config::NotificationsConfig::default(),
            sound_settings: crate::sounds::test_settings(ilium_sound::SoundSettings::default()),
            sound_requests,
            custom_signatures: Vec::new(),
            agent_debug_menu_enabled: false,
            progress_monitor_enabled: true,
        }))
    }

    #[cfg(unix)]
    async fn shell_title_fixture(
        state: &Arc<ServerState>,
        directory: &tempfile::TempDir,
    ) -> NodeId {
        let group = state
            .tree
            .write()
            .await
            .add_group(ilium_core::ROOT_ID, "title fixture")
            .expect("fixture group");
        let pane_id = state
            .tree
            .write()
            .await
            .add_pane(group, "original", PaneContentKind::Terminal)
            .expect("fixture pane");
        let session = ilium_pty::PtySession::spawn(
            ilium_pty::PtyCommand::new("/bin/sh", directory.path(), 24, 80)
                .arg("-c")
                .arg("exec cat"),
        )
        .expect("isolated shell PTY");
        state.panes.write().await.insert(
            pane_id,
            PaneResource::Terminal(Box::new(crate::pane::TerminalPaneRuntime::new(
                session,
                TerminalOrigin::PlainShell,
                None,
                Duration::from_secs(1),
            ))),
        );
        pane_id
    }

    #[cfg(unix)]
    async fn assert_no_stale_input_after_sentinel(
        session: &ilium_pty::PtySession,
        stale: &[u8],
        sentinel: &[u8],
    ) {
        assert_eq!(stale.last(), Some(&b'\r'));
        assert_eq!(sentinel.last(), Some(&b'\r'));
        let stale_text = &stale[..stale.len() - 1];
        let sentinel_text = &sentinel[..sentinel.len() - 1];
        let mut changed = session.subscribe_screen_changed();
        session
            .input_handle()
            .write(sentinel)
            .expect("sentinel admitted")
            .wait()
            .await
            .expect("sentinel delivered");
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let output = session.output_replay().bytes;
                if output
                    .windows(sentinel_text.len())
                    .any(|window| window == sentinel_text)
                {
                    assert!(!output
                        .windows(stale_text.len())
                        .any(|window| window == stale_text));
                    break;
                }
                changed.changed().await.expect("fixture reader stays live");
            }
        })
        .await
        .expect("sentinel reached fixture PTY");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn blocked_key_probe_rejects_replaced_input_gate_without_admitting_stale_bytes() {
        const STALE: &[u8] = b"stale-replaced-pty\r";
        let directory = tempfile::tempdir().expect("isolated directory");
        let state = snapshot_io_handler_state(&directory);
        let pane_id = shell_title_fixture(&state, &directory).await;
        let input_gate = pane_input_gate(&state, pane_id).await.expect("input gate");
        let owner = crate::execution::ServerExecution::start().expect("finite server bank");
        let client = owner.client.clone();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let task_state = Arc::clone(&state);
        let input_task = tokio::spawn(async move {
            let _gate_guard = input_gate.lock().await;
            write_key_input_unlocked_with_probe(
                &task_state,
                pane_id,
                STALE,
                None,
                InputWriteOrigin {
                    is_initial_prompt: false,
                    is_user_directed: false,
                    prompt_epoch: None,
                    expected_invocation: None,
                    required_ready_agent_class: None,
                    required_statusline_generation: None,
                },
                &input_gate,
                move |request| async move {
                    foreground_observation::observe_with(
                        &client,
                        request,
                        Duration::from_secs(5),
                        move |_, _| {
                            let _ = started_tx.send(());
                            release_rx
                                .recv_timeout(Duration::from_secs(5))
                                .expect("release native fixture");
                            (Some(true), None)
                        },
                    )
                    .await
                },
            )
            .await
        });
        started_rx.await.expect("native preflight started");
        let replacement = ilium_pty::PtySession::spawn(
            ilium_pty::PtyCommand::new("/bin/sh", directory.path(), 24, 80)
                .arg("-c")
                .arg("exec cat"),
        )
        .expect("replacement PTY");
        let previous = tokio::time::timeout(Duration::from_secs(2), async {
            let _tree = state.tree.write().await;
            state
                .panes
                .write()
                .await
                .insert(
                    pane_id,
                    PaneResource::Terminal(Box::new(crate::pane::TerminalPaneRuntime::new(
                        replacement,
                        TerminalOrigin::PlainShell,
                        None,
                        Duration::from_secs(1),
                    ))),
                )
                .expect("original PTY")
        })
        .await
        .expect("key preflight must not hold tree or panes guards");
        release_tx.send(()).expect("release native fixture");
        let error = input_task
            .await
            .expect("input task")
            .expect_err("replaced gate refuses old input");
        assert!(error.contains("changed before input delivery"), "{error}");
        let PaneResource::Terminal(mut old_runtime) = previous else {
            panic!("original fixture remains terminal");
        };
        assert_no_stale_input_after_sentinel(&old_runtime.session, STALE, b"old-sentinel\r").await;
        old_runtime.session.kill().expect("close original fixture");
        let removed = { state.panes.write().await.remove(&pane_id) };
        let Some(PaneResource::Terminal(mut new_runtime)) = removed else {
            panic!("replacement fixture remains terminal");
        };
        assert_no_stale_input_after_sentinel(&new_runtime.session, STALE, b"new-sentinel\r").await;
        new_runtime
            .session
            .kill()
            .expect("close replacement fixture");
        owner.request_shutdown();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn blocked_key_probe_rejects_cancelled_agent_epoch_without_admitting_bytes() {
        const STALE: &[u8] = b"stale-cancelled-agent\r";
        let directory = tempfile::tempdir().expect("isolated directory");
        let state = snapshot_io_handler_state(&directory);
        let pane_id = shell_title_fixture(&state, &directory).await;
        state
            .tree
            .write()
            .await
            .set_pane_status(
                pane_id,
                PaneStatus::from_activity(
                    ilium_core::AgentClass::Codex,
                    ilium_core::AgentActivity::Working,
                    None,
                ),
            )
            .expect("agent status");
        {
            let mut panes = state.panes.write().await;
            let Some(PaneResource::Terminal(runtime)) = panes.get_mut(&pane_id) else {
                panic!("fixture runtime");
            };
            let identity = ilium_detect::AgentIdentity {
                class: ilium_core::AgentClass::Codex,
                pid: u32::MAX - 1,
                started_at_unix_seconds: 1,
                process_name: "codex".into(),
                matched_signature: "codex".into(),
                process_tree_depth: 1,
            };
            runtime.agent_process_key = Some(crate::pane::agent_process_key(&identity));
            runtime.detection_schedule.cached_identity = Some(identity);
            runtime.agent_input_available = true;
        }
        let input_gate = pane_input_gate(&state, pane_id).await.expect("input gate");
        let owner = crate::execution::ServerExecution::start().expect("finite server bank");
        let client = owner.client.clone();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let task_state = Arc::clone(&state);
        let input_task = tokio::spawn(async move {
            let _gate_guard = input_gate.lock().await;
            write_key_input_unlocked_with_probe(
                &task_state,
                pane_id,
                STALE,
                None,
                InputWriteOrigin {
                    is_initial_prompt: false,
                    is_user_directed: false,
                    prompt_epoch: None,
                    expected_invocation: None,
                    required_ready_agent_class: None,
                    required_statusline_generation: None,
                },
                &input_gate,
                move |request| async move {
                    foreground_observation::observe_with(
                        &client,
                        request,
                        Duration::from_secs(5),
                        move |_, _| {
                            let _ = started_tx.send(());
                            release_rx
                                .recv_timeout(Duration::from_secs(5))
                                .expect("release native fixture");
                            (Some(false), Some(true))
                        },
                    )
                    .await
                },
            )
            .await
        });
        started_rx.await.expect("native preflight started");
        tokio::time::timeout(Duration::from_secs(2), async {
            let _tree = state.tree.write().await;
            let mut panes = state.panes.write().await;
            let Some(PaneResource::Terminal(runtime)) = panes.get_mut(&pane_id) else {
                panic!("fixture runtime");
            };
            runtime
                .agent_input_cancel
                .send_modify(|generation| *generation = generation.wrapping_add(1));
        })
        .await
        .expect("key preflight must not hold tree or panes guards");
        release_tx.send(()).expect("release native fixture");
        let error = input_task
            .await
            .expect("input task")
            .expect_err("cancelled epoch refuses automatic input");
        assert!(error.contains("agent input ownership changed"), "{error}");
        let removed = { state.panes.write().await.remove(&pane_id) };
        let Some(PaneResource::Terminal(mut runtime)) = removed else {
            panic!("cancelled fixture remains terminal");
        };
        assert_no_stale_input_after_sentinel(&runtime.session, STALE, b"cancel-sentinel\r").await;
        runtime.session.kill().expect("close fixture");
        owner.request_shutdown();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn blocked_automatic_title_probe_releases_registry_guards_and_rejects_replacement() {
        let directory = tempfile::tempdir().expect("isolated directory");
        let state = snapshot_io_handler_state(&directory);
        let pane_id = shell_title_fixture(&state, &directory).await;
        let owner = crate::execution::ServerExecution::start().expect("finite server bank");
        let client = owner.client.clone();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let task_state = Arc::clone(&state);
        let title_task = tokio::spawn(async move {
            handle_automatic_pane_title_with_probe(
                &task_state,
                pane_id,
                "stale title".to_owned(),
                None,
                None,
                move |_, request| async move {
                    foreground_observation::observe_with(
                        &client,
                        request,
                        Duration::from_secs(5),
                        move |_, _| {
                            let _ = started_tx.send(());
                            release_rx
                                .recv_timeout(Duration::from_secs(5))
                                .expect("release native fixture");
                            (Some(true), None)
                        },
                    )
                    .await
                },
            )
            .await;
        });
        started_rx.await.expect("native observation started");
        let old_runtime = tokio::time::timeout(Duration::from_secs(2), async {
            let tree = state.tree.write().await;
            let mut panes = state.panes.write().await;
            assert_eq!(tree.get(pane_id).expect("pane").name, "original");
            let replacement = ilium_pty::PtySession::spawn(
                ilium_pty::PtyCommand::new("/bin/sh", directory.path(), 24, 80)
                    .arg("-c")
                    .arg("exec cat"),
            )
            .expect("replacement PTY");
            panes
                .insert(
                    pane_id,
                    PaneResource::Terminal(Box::new(crate::pane::TerminalPaneRuntime::new(
                        replacement,
                        TerminalOrigin::PlainShell,
                        None,
                        Duration::from_secs(1),
                    ))),
                )
                .expect("original PTY")
        })
        .await
        .expect("title probe must not hold tree or panes guards");
        release_tx.send(()).expect("release native fixture");
        title_task.await.expect("title task");
        assert_eq!(
            state.tree.read().await.get(pane_id).expect("pane").name,
            "original"
        );
        if let PaneResource::Terminal(mut old_runtime) = old_runtime {
            old_runtime.session.kill().expect("close original fixture");
        }
        if let Some(PaneResource::Terminal(mut replacement)) =
            state.panes.write().await.remove(&pane_id)
        {
            replacement
                .session
                .kill()
                .expect("close replacement fixture");
        }
        owner.request_shutdown();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn current_plain_shell_title_still_applies_after_off_lock_confirmation() {
        let directory = tempfile::tempdir().expect("isolated directory");
        let state = snapshot_io_handler_state(&directory);
        let pane_id = shell_title_fixture(&state, &directory).await;
        let owner = crate::execution::ServerExecution::start().expect("finite server bank");
        let client = owner.client.clone();
        handle_automatic_pane_title_with_probe(
            &state,
            pane_id,
            "current title".to_owned(),
            None,
            None,
            move |_, request| async move {
                foreground_observation::observe_with(
                    &client,
                    request,
                    Duration::from_secs(2),
                    |_, _| (Some(true), None),
                )
                .await
            },
        )
        .await;
        assert_eq!(
            state.tree.read().await.get(pane_id).expect("pane").name,
            "current title"
        );
        if let Some(PaneResource::Terminal(mut runtime)) =
            state.panes.write().await.remove(&pane_id)
        {
            runtime.session.kill().expect("close fixture");
        }
        owner.request_shutdown();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn blocked_termination_releases_registry_and_never_kills_replacement() {
        let directory = tempfile::tempdir().expect("isolated directory");
        let state = snapshot_io_handler_state(&directory);
        assert!(state
            .execution
            .set(crate::execution::ServerExecution::start().expect("bank"))
            .is_ok());
        let pane_id = shell_title_fixture(&state, &directory).await;
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let task_state = Arc::clone(&state);
        let task = tokio::spawn(async move {
            terminate_pane_process_with_work(&task_state, pane_id, move |control| {
                let _ = started_tx.send(());
                release_rx
                    .recv_timeout(Duration::from_secs(10))
                    .map_err(std::io::Error::other)?;
                control.kill_direct_child().map_err(std::io::Error::other)
            })
            .await
        });
        tokio::time::timeout(Duration::from_secs(2), started_rx)
            .await
            .expect("native callback starts")
            .expect("native started channel");
        let replacement = ilium_pty::PtySession::spawn(
            ilium_pty::PtyCommand::new("/bin/sh", directory.path(), 24, 80)
                .arg("-c")
                .arg("exec cat"),
        )
        .expect("replacement fixture");
        let old = tokio::time::timeout(Duration::from_secs(2), async {
            let tree = state.tree.write().await;
            let mut panes = state.panes.write().await;
            assert!(tree.get(pane_id).is_some());
            panes
                .insert(
                    pane_id,
                    PaneResource::Terminal(Box::new(crate::pane::TerminalPaneRuntime::new(
                        replacement,
                        TerminalOrigin::PlainShell,
                        None,
                        Duration::from_secs(1),
                    ))),
                )
                .expect("old runtime")
        })
        .await
        .expect("native termination must release both global registries");
        release_tx.send(()).expect("release callback");
        let result = tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .expect("callback completes")
            .expect("termination task");
        assert!(result
            .expect_err("replacement result must refuse")
            .contains("changed during termination"));
        let replacement = state
            .panes
            .write()
            .await
            .remove(&pane_id)
            .expect("replacement still registered");
        let PaneResource::Terminal(mut replacement) = replacement else {
            panic!("replacement terminal")
        };
        assert!(
            !replacement.session.has_exited(),
            "captured termination must not signal replacement"
        );
        replacement.session.kill().expect("cleanup replacement");
        if let PaneResource::Terminal(mut old) = old {
            old.session.kill().expect("cleanup captured original");
        }
        state.execution.get().expect("bank").request_shutdown();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn current_termination_and_unavailable_bank_have_explicit_dispositions() {
        let directory = tempfile::tempdir().expect("isolated directory");
        let state = snapshot_io_handler_state(&directory);
        let pane_id = shell_title_fixture(&state, &directory).await;
        let refusal = terminate_pane_process_with_work(&state, pane_id, |control| {
            control.kill_direct_child().map_err(std::io::Error::other)
        })
        .await
        .expect_err("missing bank cannot execute kill");
        assert!(refusal.contains("server execution is not running"));
        {
            let mut panes = state.panes.write().await;
            let Some(PaneResource::Terminal(runtime)) = panes.get_mut(&pane_id) else {
                panic!("fixture")
            };
            assert!(
                !runtime.session.has_exited(),
                "refused operation cannot signal child"
            );
        }
        assert!(state
            .execution
            .set(crate::execution::ServerExecution::start().expect("bank"))
            .is_ok());
        terminate_pane_process_with_work(&state, pane_id, |control| {
            control.kill_direct_child().map_err(std::io::Error::other)
        })
        .await
        .expect("current captured child termination succeeds");
        if let Some(PaneResource::Terminal(mut runtime)) =
            state.panes.write().await.remove(&pane_id)
        {
            runtime.session.kill().expect("cleanup current fixture");
        }
        state.execution.get().expect("bank").request_shutdown();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn termination_deadline_retains_physical_native_admission() {
        let directory = tempfile::tempdir().expect("isolated directory");
        let state = snapshot_io_handler_state(&directory);
        assert!(state
            .execution
            .set(crate::execution::ServerExecution::start().expect("bank"))
            .is_ok());
        let pane_id = shell_title_fixture(&state, &directory).await;
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let task_state = Arc::clone(&state);
        let task = tokio::spawn(async move {
            terminate_pane_process_with_work(&task_state, pane_id, move |_| {
                let _ = started_tx.send(());
                release_rx
                    .recv_timeout(Duration::from_secs(15))
                    .map_err(std::io::Error::other)
            })
            .await
        });
        started_rx.await.expect("physical callback starts");
        let result = task.await.expect("caller ends after its deadline");
        assert!(result
            .expect_err("blocked native must time out")
            .contains("outcome is uncertain"));
        let owner = state.execution.get().expect("bank");
        let full_cost = ilium_execution::JobCost {
            input_bytes: 512 * 1024 * 1024,
            result_bytes: 128,
        };
        assert!(matches!(
            owner
                .client
                .foundation
                .try_reserve(ilium_execution::Lane::Io, full_cost),
            Err(ilium_execution::RejectReason::InputBytes)
        ));
        release_tx.send(()).expect("release physical callback");
        let released = tokio::time::timeout(
            Duration::from_secs(2),
            owner.client.reserve(ilium_execution::Lane::Io, full_cost),
        )
        .await
        .expect("physical release wakeup")
        .expect("credit available after callback");
        drop(released);
        if let Some(PaneResource::Terminal(mut runtime)) =
            state.panes.write().await.remove(&pane_id)
        {
            runtime.session.kill().expect("cleanup fixture");
        }
        owner.request_shutdown();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn blocked_focused_directory_probe_releases_guards_and_rejects_focus_change() {
        let directory = tempfile::tempdir().expect("isolated directory");
        let state = snapshot_io_handler_state(&directory);
        assert!(state
            .execution
            .set(crate::execution::ServerExecution::start().expect("bank"))
            .is_ok());
        let pane_id = shell_title_fixture(&state, &directory).await;
        {
            let mut panes = state.panes.write().await;
            let Some(PaneResource::Terminal(runtime)) = panes.get_mut(&pane_id) else {
                panic!("fixture")
            };
            runtime.detection_schedule.client_focused = true;
        }
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let task_state = Arc::clone(&state);
        let proposed = directory.path().to_path_buf();
        let task = tokio::spawn(async move {
            focused_terminal_working_directory_with_work(&task_state, move |_| {
                let _ = started_tx.send(());
                release_rx.recv_timeout(Duration::from_secs(5)).ok()?;
                Some(proposed)
            })
            .await
        });
        started_rx.await.expect("native callback starts");
        tokio::time::timeout(Duration::from_millis(250), async {
            let _tree = state.tree.write().await;
            let mut panes = state.panes.write().await;
            let Some(PaneResource::Terminal(runtime)) = panes.get_mut(&pane_id) else {
                panic!("fixture")
            };
            runtime.detection_schedule.client_focused = false;
        })
        .await
        .expect("native read must release both shared registries");
        release_tx.send(()).expect("release native callback");
        assert!(task.await.expect("directory task").is_none());
        {
            let mut panes = state.panes.write().await;
            let Some(PaneResource::Terminal(runtime)) = panes.get_mut(&pane_id) else {
                panic!("fixture")
            };
            runtime.detection_schedule.client_focused = true;
        }
        let current =
            candidate_new_pane_working_directory(&state, NewPaneWorkingDirectory::FocusedTerminal)
                .await;
        assert_eq!(current.as_deref(), Some(directory.path()));
        if let Some(PaneResource::Terminal(mut runtime)) =
            state.panes.write().await.remove(&pane_id)
        {
            runtime.session.kill().expect("cleanup fixture");
        }
        state.execution.get().expect("bank").request_shutdown();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn current_agent_title_receipt_survives_off_lock_collection_and_stale_generation_refuses()
    {
        let directory = tempfile::tempdir().expect("isolated title directory");
        let state = snapshot_io_handler_state(&directory);
        let pane_id = shell_title_fixture(&state, &directory).await;
        let input = {
            let mut tree = state.tree.write().await;
            tree.set_pane_launch_cwd(pane_id, directory.path().to_path_buf())
                .expect("fixture cwd");
            tree.set_pane_status(
                pane_id,
                PaneStatus::from_activity(
                    ilium_core::AgentClass::Codex,
                    ilium_core::AgentActivity::Working,
                    None,
                ),
            )
            .expect("synthetic agent fixture status");
            let mut panes = state.panes.write().await;
            let Some(PaneResource::Terminal(runtime)) = panes.get_mut(&pane_id) else {
                panic!("fixture runtime")
            };
            runtime.agent_process_key = Some(AgentProcessKey {
                class: ilium_core::AgentClass::Codex,
                process_id: runtime.session.process_id().expect("owned fixture PID"),
                started_at_unix_seconds: 1,
            });
            runtime.agent_generation = 5;
            runtime.title_generation = 3;
            runtime.session_id = Some("fixture-session".to_owned());
            runtime.session_agent_class = Some(ilium_core::AgentClass::Codex);
            runtime.session.input_handle()
        };
        // The fake agent uses an owned cat PTY. Construct its authored receipt
        // only after real delivery completes; no transcript file is invented.
        input
            .write(b"authored task for title fixture\r")
            .expect("authored fixture input admitted")
            .wait()
            .await
            .expect("actual fixture delivery completed");
        let baseline = {
            let mut tree = state.tree.write().await;
            let mut panes = state.panes.write().await;
            let Some(PaneResource::Terminal(runtime)) = panes.get_mut(&pane_id) else {
                panic!("fixture runtime")
            };
            assert!(record_authored_title_receipt(
                &mut tree,
                runtime,
                pane_id,
                "authored task for title fixture",
                PromptSubmissionSource::Keyboard
            ));
            TitleRuntimeSnapshot::capture(tree.get(pane_id).expect("fixture node"), runtime, false)
        };
        assert_eq!(
            baseline.observation.session_id.as_deref(),
            Some("fixture-session"),
            "the positive fixture must publish a class-bound session identity"
        );
        handle_session_pane_title(
            &state,
            SessionPaneTitleUpdate {
                pane_id,
                expected_session_id: "fixture-session",
                expected_title_generation: baseline.observation.title_generation,
                expected_presentation_revision: baseline.observation.presentation_revision,
                expected_process_id: baseline.observation.process_id,
                title: "current agent title".to_owned(),
                short_title: None,
                inferred_icon: None,
                title_source: PaneTitleSource::Automatic,
            },
        )
        .await;
        let revision = {
            let tree = state.tree.read().await;
            let node = tree.get(pane_id).expect("fixture node");
            assert_eq!(node.name, "current agent title");
            node.presentation_revision
        };
        {
            let mut panes = state.panes.write().await;
            let Some(PaneResource::Terminal(runtime)) = panes.get_mut(&pane_id) else {
                panic!("fixture runtime")
            };
            runtime.title_generation += 1;
        }
        handle_session_pane_title(
            &state,
            SessionPaneTitleUpdate {
                pane_id,
                expected_session_id: "fixture-session",
                expected_title_generation: baseline.observation.title_generation,
                expected_presentation_revision: revision,
                expected_process_id: baseline.observation.process_id,
                title: "stale title must not apply".to_owned(),
                short_title: None,
                inferred_icon: None,
                title_source: PaneTitleSource::Automatic,
            },
        )
        .await;
        assert_eq!(
            state
                .tree
                .read()
                .await
                .get(pane_id)
                .expect("fixture node")
                .name,
            "current agent title"
        );
        if let Some(PaneResource::Terminal(mut runtime)) =
            state.panes.write().await.remove(&pane_id)
        {
            runtime.session.kill().expect("cleanup owned fixture");
        };
    }

    #[tokio::test]
    async fn missing_pane_resize_reports_the_rejected_geometry() {
        let directory = tempfile::tempdir().expect("isolated directory");
        let state = snapshot_io_handler_state(&directory);
        let (direct_tx, mut direct_rx) = DirectEventSender::channel(1);
        let pane_id = NodeId(u64::MAX);
        handle_resize_pane(
            &state,
            pane_id,
            24,
            80,
            PaneResizeCause::HostTerminal,
            &direct_tx,
        )
        .await;
        match direct_rx.try_recv().expect("resize rejection") {
            ServerEvent::PaneResizeRejected {
                pane_id: rejected,
                rows,
                cols,
                message,
            } => {
                assert_eq!(rejected, pane_id);
                assert_eq!((rows, cols), (24, 80));
                assert!(message.contains("no pane found"));
            }
            other => panic!("expected correlated resize rejection, got {other:?}"),
        }
        assert!(direct_rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn kill_handler_waits_for_snapshot_guard_then_removes_without_recreation() {
        let directory = tempfile::tempdir().expect("directory");
        let state = snapshot_io_handler_state(&directory);
        crate::persistence::save_snapshot(&state)
            .await
            .expect("initial save");
        let write_guard = Arc::clone(&state.snapshot_write_lock).lock_owned().await;
        let mut events = state.events.subscribe_owned();
        let killing_state = Arc::clone(&state);
        let kill = tokio::spawn(async move { handle_kill_session(&killing_state).await });
        let event = tokio::time::timeout(Duration::from_secs(5), events.recv())
            .await
            .expect("kill reached deletion fence")
            .expect("event");
        assert!(matches!(event, ServerEvent::TreeSnapshot(_)));
        assert!(state.is_session_killed());
        assert!(
            state.snapshot_path.exists(),
            "existing guard fences actual deletion"
        );
        drop(write_guard);
        tokio::time::timeout(Duration::from_secs(5), kill)
            .await
            .expect("kill completed")
            .expect("kill task")
            .expect("kill succeeded");
        assert!(!state.snapshot_path.exists());
        assert!(crate::persistence::save_snapshot(&state).await.is_err());
        crate::persistence::shutdown_snapshot_service(&state)
            .await
            .expect("drain");
        assert!(!state.snapshot_path.exists());
    }

    #[tokio::test]
    async fn recovery_discard_removes_via_snapshot_owner_and_keeps_live_session_writable() {
        let directory = tempfile::tempdir().expect("directory");
        let state = snapshot_io_handler_state(&directory);
        crate::persistence::save_snapshot(&state)
            .await
            .expect("initial save");
        let snapshot = crate::persistence::load_snapshot_for_state(&state)
            .await
            .expect("readback")
            .expect("saved snapshot");
        assert!(state.recovery.install_initial(snapshot).await.is_ok());
        let (direct_tx, _direct_rx) = DirectEventSender::channel(128);
        handle_session_recovery_resolution(&state, false, &direct_tx).await;
        assert!(!state.snapshot_path.exists());
        assert!(!state.is_session_killed(), "discard is not a session kill");
        crate::persistence::save_snapshot(&state)
            .await
            .expect("fresh live save");
        assert!(state.snapshot_path.exists());
        crate::persistence::shutdown_snapshot_service(&state)
            .await
            .expect("drain");
    }

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

    #[cfg(unix)]
    async fn forwarder_fixture_marker(
        state: &ServerState,
        pane_id: NodeId,
        changed: &mut tokio::sync::watch::Receiver<()>,
        marker: &str,
    ) -> ilium_pty::PtyOutputReplay {
        tokio::time::timeout(Duration::from_secs(120), async {
            loop {
                let replay = {
                    let panes = state.panes.read().await;
                    let Some(PaneResource::Terminal(runtime)) = panes.get(&pane_id) else {
                        panic!("fixture terminal was removed");
                    };
                    if runtime.session.screen_text().contains(marker) {
                        let replay = runtime.session.output_replay();
                        replay
                            .bytes
                            .windows(marker.len())
                            .any(|window| window == marker.as_bytes())
                            .then_some(replay)
                    } else {
                        None
                    }
                };
                if let Some(replay) = replay {
                    return replay;
                }
                changed.changed().await.expect("fixture PTY remains live");
            }
        })
        .await
        .expect("fixture marker did not reach the owned PTY journal")
    }

    #[cfg(unix)]
    async fn forwarder_fixture_chunks_through(
        receiver: &mut tokio::sync::broadcast::Receiver<ilium_pty::PtyOutputChunk>,
        after_sequence: u64,
        through_sequence: u64,
    ) -> Vec<ilium_pty::PtyOutputChunk> {
        assert!(through_sequence > after_sequence);
        tokio::time::timeout(Duration::from_secs(10), async {
            let mut chunks = Vec::new();
            let mut expected = after_sequence + 1;
            while expected <= through_sequence {
                let chunk = receiver
                    .recv()
                    .await
                    .expect("small fixture phase must retain every native PTY chunk");
                assert_eq!(
                    chunk.sequence, expected,
                    "native PTY sequence is contiguous"
                );
                chunks.push(chunk);
                expected += 1;
            }
            chunks
        })
        .await
        .expect("native PTY output did not reach the fixture receiver")
    }

    #[cfg(unix)]
    async fn forwarder_fixture_next_terminal_event(
        receiver: &mut crate::state::OwnedServerEventReceiver,
        pane_id: NodeId,
    ) -> ServerEvent {
        tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                let event = receiver
                    .recv()
                    .await
                    .expect("fixture server event channel remains open");
                if matches!(
                    &event,
                    ServerEvent::ScreenUpdate { pane_id: id, .. }
                        | ServerEvent::TerminalReplay { pane_id: id, .. }
                        if *id == pane_id
                ) {
                    return event;
                }
            }
        })
        .await
        .expect("owned forwarder did not publish a terminal event")
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "current_thread")]
    async fn owned_forwarder_repairs_two_forced_lags_without_duplicate_bytes() {
        let directory = tempfile::tempdir().expect("private fixture directory");
        let bulk_path = directory.path().join("bulk-output.bin");
        let bulk = std::fs::File::create(&bulk_path).expect("create sparse eviction input");
        // Real bytes cross the ordinary PTY owner and its unchanged 32 MiB journal.
        // The extra MiB guarantees eviction even when PTY read chunk sizes vary.
        bulk.set_len(33 * 1024 * 1024)
            .expect("size sparse eviction input");
        let (sound_requests, _sound_receiver) = crate::sounds::test_channel(1);
        let state = Arc::new(ServerState::new(crate::state::ServerStateOptions {
            session_name: "forwarder-two-lags".into(),
            session_cwd: directory.path().to_owned(),
            home_dir: directory.path().to_owned(),
            snapshot_path: directory.path().join("session.json"),
            socket_path: directory.path().join("isolated.sock"),
            detection_config: crate::config::DetectionConfig::default(),
            notifications_config: crate::config::NotificationsConfig::default(),
            sound_settings: crate::sounds::test_settings(ilium_sound::SoundSettings::default()),
            sound_requests,
            custom_signatures: Vec::new(),
            agent_debug_menu_enabled: false,
            progress_monitor_enabled: true,
        }));
        let pane_id = {
            let mut tree = state.tree.write().await;
            let group = tree
                .add_group(ilium_core::ROOT_ID, "forwarder fixture")
                .expect("fixture group");
            tree.add_pane(group, "one owner", PaneContentKind::Terminal)
                .expect("fixture pane")
        };
        let session = ilium_pty::PtySession::spawn(
            ilium_pty::PtyCommand::new("/bin/sh", directory.path(), 24, 80)
                .arg("-c")
                .arg(
                    "read gate; printf 'baseline-ready\\n'; \
                     read gate; printf 'gap-one-a\\n'; \
                     read gate; printf 'gap-one-b\\n'; \
                     read gate; cat bulk-output.bin; printf 'eviction-ready\\n'; read hold",
                ),
        )
        .expect("spawn one owned PTY");
        let mut native_output = session.subscribe_output_bytes();
        let mut screen_changed = session.subscribe_screen_changed();
        let input = session.input_handle();
        let owner_status = input.subscribe_status();
        assert_eq!(
            session.output_replay().through_sequence,
            0,
            "the gated shell must produce no bytes before subscription"
        );
        state.panes.write().await.insert(
            pane_id,
            PaneResource::Terminal(Box::new(crate::pane::TerminalPaneRuntime::new(
                session,
                TerminalOrigin::Command("forwarder fixture".into()),
                None,
                Duration::from_secs(1),
            ))),
        );
        let no_panes = std::collections::HashSet::new();
        state.replace_terminal_subscriptions(false, &no_panes, true, &no_panes);
        let mut published = state.events.subscribe_owned();
        // This one-slot receiver belongs only to this test. Sending two real
        // journaled chunks without yielding forces RecvError::Lagged exactly.
        let (forward_sender, forward_receiver) = tokio::sync::broadcast::channel(1);
        let forwarder = tokio::spawn(forward_output_with_owner_status(
            Arc::clone(&state),
            pane_id,
            forward_receiver,
            input.clone(),
            owner_status,
        ));

        input
            .write(b"next\r")
            .expect("baseline input accepted")
            .wait()
            .await
            .expect("baseline input delivered");
        let baseline =
            forwarder_fixture_marker(&state, pane_id, &mut screen_changed, "baseline-ready").await;
        assert!(baseline.is_complete);
        let baseline_chunks =
            forwarder_fixture_chunks_through(&mut native_output, 0, baseline.through_sequence)
                .await;
        let mut delivered_baseline = Vec::new();
        for chunk in baseline_chunks {
            let sequence = chunk.sequence;
            let bytes = chunk.bytes.to_vec();
            forward_sender
                .send(chunk)
                .expect("forwarder receiver remains open");
            let event = forwarder_fixture_next_terminal_event(&mut published, pane_id).await;
            assert_eq!(
                event,
                ServerEvent::ScreenUpdate {
                    pane_id,
                    first_sequence: sequence,
                    sequence,
                    bytes: bytes.clone(),
                }
            );
            delivered_baseline.extend_from_slice(&bytes);
        }
        assert_eq!(delivered_baseline, baseline.bytes);

        input
            .write(b"next\r")
            .expect("first gap input accepted")
            .wait()
            .await
            .expect("first gap input delivered");
        let first =
            forwarder_fixture_marker(&state, pane_id, &mut screen_changed, "gap-one-a").await;
        let mut missing = forwarder_fixture_chunks_through(
            &mut native_output,
            baseline.through_sequence,
            first.through_sequence,
        )
        .await;
        input
            .write(b"next\r")
            .expect("second gap input accepted")
            .wait()
            .await
            .expect("second gap input delivered");
        let after_first_gap =
            forwarder_fixture_marker(&state, pane_id, &mut screen_changed, "gap-one-b").await;
        missing.extend(
            forwarder_fixture_chunks_through(
                &mut native_output,
                first.through_sequence,
                after_first_gap.through_sequence,
            )
            .await,
        );
        assert!(
            missing.len() >= 2,
            "two gated writes must create two PTY chunks"
        );
        assert!(after_first_gap.bytes.starts_with(&baseline.bytes));
        for chunk in missing {
            forward_sender
                .send(chunk)
                .expect("forwarder receiver remains open");
        }
        let recovered = forwarder_fixture_next_terminal_event(&mut published, pane_id).await;
        let missing_bytes = after_first_gap.bytes[baseline.bytes.len()..].to_vec();
        assert_eq!(
            recovered,
            ServerEvent::ScreenUpdate {
                pane_id,
                first_sequence: baseline.through_sequence + 1,
                sequence: after_first_gap.through_sequence,
                bytes: missing_bytes.clone(),
            },
            "first forced lag must publish exactly the contiguous missing tail"
        );
        let mut conserved = delivered_baseline;
        conserved.extend_from_slice(&missing_bytes);
        assert_eq!(conserved, after_first_gap.bytes);

        input
            .write(b"next\r")
            .expect("eviction input accepted")
            .wait()
            .await
            .expect("eviction input delivered");
        let evicted =
            forwarder_fixture_marker(&state, pane_id, &mut screen_changed, "eviction-ready").await;
        assert!(
            !evicted.is_complete,
            "33 MiB must evict the oldest journal bytes"
        );
        assert!(
            evicted.bytes.starts_with(b"\x1bc"),
            "evicted replay resets the parser"
        );
        assert!(evicted.through_sequence > after_first_gap.through_sequence + 1);

        let (native_receiver_lagged, last_two) =
            tokio::time::timeout(Duration::from_secs(10), async {
                let mut lagged = false;
                let mut last_two = std::collections::VecDeque::new();
                loop {
                    match native_output.recv().await {
                        Ok(chunk) => {
                            let is_last = chunk.sequence == evicted.through_sequence;
                            last_two.push_back(chunk);
                            if last_two.len() > 2 {
                                last_two.pop_front();
                            }
                            if is_last {
                                return (lagged, last_two);
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                            lagged = true;
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                            panic!("gated PTY output channel closed")
                        }
                    }
                }
            })
            .await
            .expect("eviction marker chunk did not reach the native receiver");
        assert!(
            native_receiver_lagged,
            "bulk PTY output must overflow its reader ring"
        );
        assert_eq!(last_two.len(), 2);
        assert_eq!(last_two[1].sequence, evicted.through_sequence);
        for chunk in last_two {
            forward_sender
                .send(chunk)
                .expect("forwarder receiver remains open");
        }
        let recovered = forwarder_fixture_next_terminal_event(&mut published, pane_id).await;
        assert_eq!(
            recovered,
            ServerEvent::TerminalReplay {
                pane_id,
                through_sequence: evicted.through_sequence,
                bytes: evicted.bytes,
                is_complete: false,
            },
            "second forced lag must send exactly the retained reset replay"
        );

        drop(forward_sender);
        tokio::time::timeout(Duration::from_secs(30), forwarder)
            .await
            .expect("owned forwarder did not drain")
            .expect("owned forwarder panicked");
        loop {
            match published.try_recv() {
                Ok(ServerEvent::ScreenUpdate { pane_id: id, .. })
                | Ok(ServerEvent::TerminalReplay { pane_id: id, .. })
                    if id == pane_id =>
                {
                    panic!("forwarder duplicated terminal bytes after recovery")
                }
                Ok(_) => {}
                Err(tokio::sync::broadcast::error::TryRecvError::Empty) => break,
                Err(error) => panic!("server event ring lost fixture evidence: {error}"),
            }
        }
        state.replace_terminal_subscriptions(true, &no_panes, false, &no_panes);
        let Some(PaneResource::Terminal(mut runtime)) = state.panes.write().await.remove(&pane_id)
        else {
            panic!("fixture PTY was not registered");
        };
        runtime.session.kill().expect("close owned fixture PTY");
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

    #[tokio::test]
    async fn detector_settings_writer_uses_admitted_io_owner_and_reads_back() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let settings = ilium_ipc::AgentDetectionSettings {
            working_poll_seconds: 3,
            idle_poll_seconds: 12,
            custom_signatures: vec![ilium_ipc::CustomAgentSignature {
                name_substring: "owned-agent".to_string(),
                class: ilium_core::AgentClass::Claude,
            }],
        };
        let execution = crate::execution::ServerExecution::start().expect("execution bank");
        let cost = agent_detection_settings_save_cost(&directory.path().to_path_buf(), &settings)
            .expect("save cost");
        let reservation = execution
            .client
            .reserve(ilium_execution::Lane::Io, cost)
            .await
            .expect("I/O admission");
        let caller_thread = std::thread::current().id();
        let (thread_sender, thread_receiver) = std::sync::mpsc::channel();

        save_agent_detection_settings_reserved(
            &execution.client,
            reservation,
            directory.path().to_path_buf(),
            settings,
            move |config_dir, settings| {
                thread_sender
                    .send(std::thread::current().id())
                    .expect("observe the physical I/O owner");
                crate::config::save_agent_detection_settings(config_dir, settings)
            },
        )
        .await
        .expect("persist detector settings");

        assert_ne!(
            thread_receiver.recv().expect("writer thread identity"),
            caller_thread
        );
        let loaded = crate::config::load(directory.path()).expect("read back config");
        assert_eq!(
            loaded.detection.working_poll_interval,
            Duration::from_secs(3)
        );
        assert_eq!(loaded.detection.idle_poll_interval, Duration::from_secs(12));
        assert_eq!(loaded.custom_signatures[0].name_substring, "owned-agent");
        execution.request_shutdown();
    }

    #[test]
    fn detector_settings_save_admission_covers_config_and_rejects_oversized_inputs() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let valid = ilium_ipc::AgentDetectionSettings {
            working_poll_seconds: 2,
            idle_poll_seconds: 10,
            custom_signatures: vec![ilium_ipc::CustomAgentSignature {
                name_substring: "agent".to_string(),
                class: ilium_core::AgentClass::Codex,
            }],
        };
        let cost = agent_detection_settings_save_cost(&directory.path().to_path_buf(), &valid)
            .expect("bounded settings fit the admitted save");
        assert!(cost.input_bytes >= crate::config::MAX_CONFIG_BYTES * 16);
        assert!(cost.result_bytes >= crate::config::MAX_CONFIG_BYTES);

        let oversized_settings = ilium_ipc::AgentDetectionSettings {
            custom_signatures: vec![ilium_ipc::CustomAgentSignature {
                name_substring: "x".repeat(crate::config::MAX_CONFIG_BYTES + 1),
                class: ilium_core::AgentClass::Codex,
            }],
            ..valid.clone()
        };
        assert!(agent_detection_settings_save_cost(
            &directory.path().to_path_buf(),
            &oversized_settings,
        )
        .is_err());

        let oversized_path = std::path::PathBuf::from("x".repeat(64 * 1024 + 1));
        assert!(agent_detection_settings_save_cost(&oversized_path, &valid).is_err());
    }

    #[test]
    fn pane_replacement_reservations_are_per_pane_and_released_on_drop() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let state = snapshot_io_handler_state(&directory);
        let pane_id = NodeId(17);

        let reservation = PaneReplacementReservation::try_new(&state, pane_id)
            .expect("first replacement claims pane");
        assert!(PaneReplacementReservation::try_new(&state, pane_id).is_none());
        let other_reservation = PaneReplacementReservation::try_new(&state, NodeId(18))
            .expect("unrelated pane can replace independently");

        drop(reservation);
        assert!(PaneReplacementReservation::try_new(&state, pane_id).is_some());
        drop(other_reservation);
    }

    #[test]
    fn same_epoch_exact_transcript_repairs_unknown_historical_prompt() {
        let mut recovery = ilium_core::AgentRecovery {
            last_known_state: ilium_core::AgentState::from_activity(
                ilium_core::AgentClass::Codex,
                ilium_core::AgentActivity::Working,
                None,
            ),
            process: ilium_core::AgentProcessKey {
                class: ilium_core::AgentClass::Codex,
                process_id: 42,
                started_at_unix_seconds: 1,
            },
            availability: ilium_core::AgentAvailability::Unverified,
            signal_name: None,
            session_id: Some("verified-session".to_string()),
            last_prompt: None,
            previous_exact_prompt: Some("older exact".to_string()),
            latest_prompt_unavailable: true,
        };

        // The token/session/process fence is checked by the caller before
        // this mutation. An opaque direct Enter can later gain exact provider
        // evidence for that same still-pending epoch, including trailing text.
        set_recovery_prompt(&mut recovery, Some("new\nline  "), None, false);
        assert_eq!(recovery.last_prompt.as_deref(), Some("new\nline  "));
        assert_eq!(recovery.previous_exact_prompt, None);
        assert!(!recovery.latest_prompt_unavailable);
    }

    #[tokio::test]
    async fn delayed_transcript_cannot_replace_exact_receipt_backed_prompt() {
        let directory = tempfile::tempdir().expect("create transcript recovery test directory");
        let (sound_requests, sound_task) = crate::sounds::spawn(
            Arc::new(crate::NoopSoundPlayer),
            crate::execution::test_general_client(),
        );
        let state = Arc::new(ServerState::new(crate::state::ServerStateOptions {
            session_name: "exact-prompt-transcript-fence".to_string(),
            session_cwd: ilium_platform::paths::canonicalize(directory.path())
                .expect("canonical test launch directory"),
            home_dir: directory.path().to_path_buf(),
            snapshot_path: directory.path().join("exact-prompt.snapshot.json"),
            socket_path: directory.path().join("test.sock"),
            detection_config: crate::config::DetectionConfig::default(),
            notifications_config: crate::config::NotificationsConfig::default(),
            sound_settings: crate::sounds::test_settings(ilium_sound::SoundSettings::default()),
            sound_requests,
            custom_signatures: Vec::new(),
            agent_debug_menu_enabled: false,
            progress_monitor_enabled: true,
        }));
        // Pane admission runs repository probes on the execution service, so a
        // fixture without one would have every spawn rejected and its node removed.
        assert!(state
            .execution
            .set(crate::execution::ServerExecution::start().expect("finite server bank"))
            .is_ok());
        let pane_id = {
            let mut tree = state.tree.write().await;
            let project_id = tree.project_ids()[0];
            let group_id = tree.add_group(project_id, "work").unwrap();
            tree.add_pane(group_id, "agent", PaneContentKind::Terminal)
                .unwrap()
        };
        spawn_and_register_pane(
            &state,
            pane_id,
            PaneSnapshotKind::Terminal(TerminalOrigin::Command(long_running_pane_command())),
        )
        .await
        .expect("register command-backed test terminal");

        let owner = AgentProcessKey {
            class: ilium_core::AgentClass::Codex,
            process_id: 42,
            started_at_unix_seconds: 1,
        };
        {
            let mut panes = state.panes.write().await;
            let Some(PaneResource::Terminal(runtime)) = panes.get_mut(&pane_id) else {
                panic!("registered pane must be a terminal");
            };
            let unverified_agent = PaneStatus::from_activity(
                ilium_core::AgentClass::Codex,
                ilium_core::AgentActivity::Working,
                None,
            );
            assert!(runtime
                .automated_agent_input_rejection(&unverified_agent, None)
                .is_some());
            assert!(runtime
                .automated_agent_input_rejection(&PaneStatus::PlainShell, None)
                .is_none());
            runtime.agent_process_key = Some(owner.clone());
            runtime.agent_generation = 7;
            runtime.session_id = Some("verified-session".to_string());
            runtime.session_agent_class = Some(owner.class.clone());
            runtime.session_process_id = Some(owner.process_id);
            runtime.session_process_started_at_unix_seconds = Some(owner.started_at_unix_seconds);
            runtime.prompt_transcript_epoch = Some(crate::pane::PromptTranscriptEpoch {
                token: "epoch-7".to_string(),
                generation: 7,
                process: owner.clone(),
                session_id: "verified-session".to_string(),
            });
            runtime.last_agent_prompt = Some("current exact  ".to_string());
            runtime.latest_agent_prompt_unavailable = false;
        }
        {
            let mut tree = state.tree.write().await;
            tree.set_pane_status(
                pane_id,
                PaneStatus::AgentUnavailable(Box::new(AgentRecovery {
                    last_known_state: ilium_core::AgentState::from_activity(
                        ilium_core::AgentClass::Codex,
                        ilium_core::AgentActivity::Working,
                        None,
                    ),
                    process: owner,
                    availability: ilium_core::AgentAvailability::Unverified,
                    signal_name: None,
                    session_id: Some("verified-session".to_string()),
                    last_prompt: Some("current exact  ".to_string()),
                    previous_exact_prompt: None,
                    latest_prompt_unavailable: false,
                })),
            )
            .unwrap();
            tree.set_last_prompt(pane_id, Some("current exact  ".to_string()))
                .unwrap();
        }

        // A prior provider row can be flushed after the Enter baseline and
        // pass the worker's offset/timestamp checks. The exact PTY receipt
        // remains authoritative for this same epoch.
        handle_exact_agent_prompt_from_transcript(
            &state,
            pane_id,
            "verified-session",
            "epoch-7",
            "older delayed row".to_string(),
        )
        .await;

        let tree = state.tree.read().await;
        assert_eq!(tree.last_prompt(pane_id), Some("current exact  "));
        let Some(NodeKind::Pane {
            status: PaneStatus::AgentUnavailable(recovery),
            ..
        }) = tree.get(pane_id).map(|node| &node.kind)
        else {
            panic!("historical recovery must remain available");
        };
        assert_eq!(recovery.last_prompt.as_deref(), Some("current exact  "));
        assert!(!recovery.latest_prompt_unavailable);
        drop(tree);
        let panes = state.panes.read().await;
        let Some(PaneResource::Terminal(runtime)) = panes.get(&pane_id) else {
            panic!("registered pane must remain a terminal");
        };
        assert_eq!(
            runtime.last_agent_prompt.as_deref(),
            Some("current exact  ")
        );
        drop(panes);

        // The real handler must also repair opaque input only for the same
        // retained invocation/session/Enter, including after the agent exits.
        {
            let mut tree = state.tree.write().await;
            let mut panes = state.panes.write().await;
            let Some(PaneResource::Terminal(runtime)) = panes.get_mut(&pane_id) else {
                panic!("registered pane must remain a terminal");
            };
            runtime.latest_agent_prompt_unavailable = true;
            let owner = runtime.agent_process_key.clone().unwrap();
            tree.set_last_prompt(pane_id, None).unwrap();
            update_unavailable_recovery_prompt(
                &mut tree,
                pane_id,
                &owner,
                None,
                runtime.last_agent_prompt.as_deref(),
                true,
                &state,
            );
        }
        for (session_id, epoch) in [
            ("replacement-session", "epoch-7"),
            ("verified-session", "obsolete-enter"),
        ] {
            handle_exact_agent_prompt_from_transcript(
                &state,
                pane_id,
                session_id,
                epoch,
                "stale correction".to_string(),
            )
            .await;
            assert_eq!(state.tree.read().await.last_prompt(pane_id), None);
        }
        // A delayed worker reply must not repair a different invocation,
        // even when its public session ID and Enter token happen to match.
        for mismatch in [
            "generation",
            "process_id",
            "process_birth",
            "provider",
            "invalidated_session",
            "session_provider",
            "session_process_id",
            "session_process_birth",
        ] {
            {
                let mut panes = state.panes.write().await;
                let Some(PaneResource::Terminal(runtime)) = panes.get_mut(&pane_id) else {
                    panic!("registered test terminal");
                };
                match mismatch {
                    "generation" => runtime.agent_generation += 1,
                    "process_id" => runtime.agent_process_key.as_mut().unwrap().process_id += 1,
                    "process_birth" => {
                        runtime
                            .agent_process_key
                            .as_mut()
                            .unwrap()
                            .started_at_unix_seconds += 1;
                    }
                    "provider" => {
                        runtime.agent_process_key.as_mut().unwrap().class =
                            ilium_core::AgentClass::Claude;
                    }
                    "invalidated_session" => runtime.is_session_identity_invalidated = true,
                    "session_provider" => {
                        runtime.session_agent_class = Some(ilium_core::AgentClass::Claude);
                    }
                    "session_process_id" => runtime.session_process_id = Some(43),
                    "session_process_birth" => {
                        runtime.session_process_started_at_unix_seconds = Some(2);
                    }
                    _ => unreachable!("fixed mismatch inventory"),
                }
            }
            handle_exact_agent_prompt_from_transcript(
                &state,
                pane_id,
                "verified-session",
                "epoch-7",
                "stale invocation correction".to_string(),
            )
            .await;
            assert_eq!(
                state.tree.read().await.last_prompt(pane_id),
                None,
                "{mismatch}"
            );
            let mut panes = state.panes.write().await;
            let Some(PaneResource::Terminal(runtime)) = panes.get_mut(&pane_id) else {
                panic!("registered test terminal");
            };
            assert!(runtime.latest_agent_prompt_unavailable, "{mismatch}");
            let epoch = runtime.prompt_transcript_epoch.as_ref().unwrap();
            assert_eq!(epoch.token, "epoch-7", "{mismatch}");
            runtime.agent_process_key = Some(epoch.process.clone());
            runtime.agent_generation = 7;
            runtime.is_session_identity_invalidated = false;
            runtime.session_agent_class = Some(ilium_core::AgentClass::Codex);
            runtime.session_process_id = Some(42);
            runtime.session_process_started_at_unix_seconds = Some(1);
        }
        let repaired_prompt = "recovered\nexact trailing  ";
        handle_exact_agent_prompt_from_transcript(
            &state,
            pane_id,
            "verified-session",
            "epoch-7",
            repaired_prompt.to_string(),
        )
        .await;
        {
            let tree = state.tree.read().await;
            assert_eq!(tree.last_prompt(pane_id), Some(repaired_prompt));
            let recovery = tree.agent_recovery(pane_id).unwrap();
            assert_eq!(recovery.last_prompt.as_deref(), Some(repaired_prompt));
            assert!(!recovery.latest_prompt_unavailable);
            assert_eq!(recovery.previous_exact_prompt, None);
        }
        {
            let panes = state.panes.read().await;
            let Some(PaneResource::Terminal(runtime)) = panes.get(&pane_id) else {
                panic!("registered pane must remain a terminal");
            };
            assert_eq!(runtime.last_agent_prompt.as_deref(), Some(repaired_prompt));
            assert!(runtime.prompt_transcript_epoch.is_none());
        }
        teardown_state_panes(&state);
        sound_task.abort();
    }

    #[tokio::test]
    async fn staged_automatic_enter_rejects_changed_invocation_with_same_pty() {
        let directory = tempfile::tempdir().expect("isolated staged-invocation fixture");
        let (sound_requests, sound_task) = crate::sounds::spawn(
            Arc::new(crate::NoopSoundPlayer),
            crate::execution::test_general_client(),
        );
        let state = Arc::new(ServerState::new(crate::state::ServerStateOptions {
            session_name: "staged-invocation-fence".to_string(),
            session_cwd: ilium_platform::paths::canonicalize(directory.path())
                .expect("canonical isolated launch directory"),
            home_dir: directory.path().to_path_buf(),
            snapshot_path: directory.path().join("staged-invocation.snapshot.json"),
            socket_path: directory.path().join("test.sock"),
            detection_config: crate::config::DetectionConfig::default(),
            notifications_config: crate::config::NotificationsConfig::default(),
            sound_settings: crate::sounds::test_settings(ilium_sound::SoundSettings::default()),
            sound_requests,
            custom_signatures: Vec::new(),
            agent_debug_menu_enabled: false,
            progress_monitor_enabled: true,
        }));
        // Pane admission runs repository probes on the execution service, so a
        // fixture without one would have every spawn rejected and its node removed.
        assert!(state
            .execution
            .set(crate::execution::ServerExecution::start().expect("finite server bank"))
            .is_ok());
        let pane_id = {
            let mut tree = state.tree.write().await;
            let project_id = tree.project_ids()[0];
            tree.add_pane(project_id, "staged fixture", PaneContentKind::Terminal)
                .expect("fixture terminal")
        };
        spawn_and_register_pane(
            &state,
            pane_id,
            PaneSnapshotKind::Terminal(TerminalOrigin::Command(long_running_pane_command())),
        )
        .await
        .expect("owned actual PTY");
        // Deliberately preserve the established plain-terminal policy: a fabricated
        // live agent key would independently fail fresh process validation even
        // without the new fence and would not provide a discriminating regression.
        state
            .tree
            .write()
            .await
            .set_pane_status(pane_id, PaneStatus::PlainShell)
            .unwrap();
        let (input_gate, input, expected) = {
            let panes = state.panes.read().await;
            let Some(PaneResource::Terminal(runtime)) = panes.get(&pane_id) else {
                panic!("terminal fixture runtime");
            };
            assert!(runtime.agent_process_key.is_none());
            let input_cancel_generation = *runtime.agent_input_cancel.borrow();
            (
                Arc::clone(&runtime.input_gate),
                runtime.session.input_handle(),
                AgentInputInvocation {
                    generation: runtime.agent_generation,
                    process: None,
                    input_cancel_generation,
                },
            )
        };
        let input_guard = input_gate.lock().await;
        let mut events = state.events.subscribe_owned();
        let body_result = write_key_input_unlocked(
            &state,
            pane_id,
            b"staged invocation body",
            None,
            InputWriteOrigin {
                is_initial_prompt: false,
                is_user_directed: false,
                prompt_epoch: None,
                expected_invocation: Some(&expected),
                required_ready_agent_class: None,
                required_statusline_generation: None,
            },
            &input_gate,
        )
        .await;
        // The body has completed its actual writer receipt and bookkeeping.
        // Model only the generation transition so every old admission guard still
        // permits the terminal: this isolates the new cross-stage fence.
        {
            let mut panes = state.panes.write().await;
            let Some(PaneResource::Terminal(runtime)) = panes.get_mut(&pane_id) else {
                panic!("retained terminal runtime");
            };
            runtime.agent_generation = expected.generation.checked_add(1).unwrap();
            assert!(Arc::ptr_eq(&runtime.input_gate, &input_gate));
            assert!(input.same_session(&runtime.session.input_handle()));
            assert!(runtime
                .automated_agent_input_rejection(&PaneStatus::PlainShell, None)
                .is_none());
        }
        let enter_result = write_key_input_unlocked(
            &state,
            pane_id,
            b"\r",
            Some(PromptSubmissionSource::QueuedPrompt),
            InputWriteOrigin {
                is_initial_prompt: false,
                is_user_directed: false,
                prompt_epoch: None,
                expected_invocation: Some(&expected),
                required_ready_agent_class: None,
                required_statusline_generation: None,
            },
            &input_gate,
        )
        .await;
        let mut submitted = false;
        while let Ok(event) = events.try_recv() {
            submitted |= matches!(event, ServerEvent::PanePromptSubmitted { pane_id: changed, .. }
                if changed == pane_id);
        }
        let baseline_enter_result = write_key_input_unlocked(
            &state,
            pane_id,
            b"\r",
            Some(PromptSubmissionSource::QueuedPrompt),
            InputWriteOrigin {
                is_initial_prompt: false,
                is_user_directed: false,
                prompt_epoch: None,
                expected_invocation: None,
                required_ready_agent_class: None,
                required_statusline_generation: None,
            },
            &input_gate,
        )
        .await;
        let mut baseline_submitted = false;
        while let Ok(event) = events.try_recv() {
            baseline_submitted |= matches!(event,
                ServerEvent::PanePromptSubmitted { pane_id: changed, .. } if changed == pane_id);
        }
        drop(input_guard);
        // Release local native-writer handle before the owned session cleanup.
        drop(input);
        teardown_state_panes(&state);
        sound_task.abort();
        let _ = sound_task.await;
        assert!(
            body_result.is_ok(),
            "real body delivery failed: {body_result:?}"
        );
        assert!(
            enter_result.as_ref().is_err_and(
                |message| message.contains("agent invocation changed during submission")
            ),
            "changed invocation must reject Enter before admission: {enter_result:?}"
        );
        assert!(!submitted, "rejected Enter cannot emit PanePromptSubmitted");
        assert!(
            baseline_enter_result.is_ok(),
            "unfenced PlainShell baseline: {baseline_enter_result:?}"
        );
        assert!(
            baseline_submitted,
            "accepted baseline Enter must emit PanePromptSubmitted"
        );
    }

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
        let (sound_requests, sound_task) = crate::sounds::spawn(
            Arc::new(crate::NoopSoundPlayer),
            crate::execution::test_general_client(),
        );
        let state = Arc::new(ServerState::new(crate::state::ServerStateOptions {
            session_name: "bookmark-request".to_string(),
            session_cwd: ilium_platform::paths::canonicalize(directory.path())
                .expect("canonical test launch directory"),
            home_dir: directory.path().to_path_buf(),
            snapshot_path: directory.path().join("bookmark.snapshot.json"),
            socket_path: directory.path().join("test.sock"),
            detection_config: crate::config::DetectionConfig::default(),
            notifications_config: crate::config::NotificationsConfig::default(),
            sound_settings: crate::sounds::test_settings(ilium_sound::SoundSettings::default()),
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
        let mut events = state.events.subscribe_owned();
        let (direct_tx, mut direct_rx) = DirectEventSender::channel(1);

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
        let (sound_requests, sound_task) = crate::sounds::spawn(
            Arc::new(crate::NoopSoundPlayer),
            crate::execution::test_general_client(),
        );
        let state = Arc::new(ServerState::new(crate::state::ServerStateOptions {
            session_name: "lock-request".to_string(),
            session_cwd: ilium_platform::paths::canonicalize(directory.path())
                .expect("canonical test launch directory"),
            home_dir: directory.path().to_path_buf(),
            snapshot_path: directory.path().join("lock.snapshot.json"),
            socket_path: directory.path().join("test.sock"),
            detection_config: crate::config::DetectionConfig::default(),
            notifications_config: crate::config::NotificationsConfig::default(),
            sound_settings: crate::sounds::test_settings(ilium_sound::SoundSettings::default()),
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
        let mut events = state.events.subscribe_owned();
        let (direct_tx, mut direct_rx) = DirectEventSender::channel(1);

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
        let (sound_requests, sound_task) = crate::sounds::spawn(
            Arc::new(crate::NoopSoundPlayer),
            crate::execution::test_general_client(),
        );
        let state = Arc::new(ServerState::new(crate::state::ServerStateOptions {
            session_name: "focus-activity".to_string(),
            session_cwd: ilium_platform::paths::canonicalize(directory.path())
                .expect("canonical test launch directory"),
            home_dir: directory.path().to_path_buf(),
            snapshot_path: directory.path().join("focus-activity.snapshot.json"),
            socket_path: directory.path().join("test.sock"),
            detection_config: crate::config::DetectionConfig::default(),
            notifications_config: crate::config::NotificationsConfig::default(),
            sound_settings: crate::sounds::test_settings(ilium_sound::SoundSettings::default()),
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
        let mut events = state.events.subscribe_owned();

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
        let (sound_requests, sound_task) = crate::sounds::spawn(
            Arc::new(crate::NoopSoundPlayer),
            crate::execution::test_general_client(),
        );
        let state = Arc::new(ServerState::new(crate::state::ServerStateOptions {
            session_name: "hidden-terminal-activity".to_string(),
            session_cwd: ilium_platform::paths::canonicalize(directory.path())
                .expect("canonical test launch directory"),
            home_dir: directory.path().to_path_buf(),
            snapshot_path: directory
                .path()
                .join("hidden-terminal-activity.snapshot.json"),
            socket_path: directory.path().join("test.sock"),
            detection_config: crate::config::DetectionConfig::default(),
            notifications_config: crate::config::NotificationsConfig::default(),
            sound_settings: crate::sounds::test_settings(ilium_sound::SoundSettings::default()),
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
        let mut events = state.events.subscribe_owned();

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
        let (sound_requests, sound_task) = crate::sounds::spawn(
            Arc::new(crate::NoopSoundPlayer),
            crate::execution::test_general_client(),
        );
        let state = Arc::new(ServerState::new(crate::state::ServerStateOptions {
            session_name: "terminal-activity".to_string(),
            session_cwd: ilium_platform::paths::canonicalize(directory.path())
                .expect("canonical test launch directory"),
            home_dir: directory.path().to_path_buf(),
            snapshot_path: directory.path().join("terminal-activity.snapshot.json"),
            socket_path: directory.path().join("test.sock"),
            detection_config: crate::config::DetectionConfig::default(),
            notifications_config: crate::config::NotificationsConfig::default(),
            sound_settings: crate::sounds::test_settings(ilium_sound::SoundSettings::default()),
            sound_requests,
            custom_signatures: Vec::new(),
            agent_debug_menu_enabled: false,
            progress_monitor_enabled: true,
        }));
        // Pane admission runs repository probes on the execution service, so a
        // fixture without one would have every spawn rejected and its node removed.
        assert!(state
            .execution
            .set(crate::execution::ServerExecution::start().expect("finite server bank"))
            .is_ok());
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
        let mut events = state.events.subscribe_owned();

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
        let (sound_requests, _sound_task) = crate::sounds::spawn(
            Arc::new(crate::NoopSoundPlayer),
            crate::execution::test_general_client(),
        );
        let state = Arc::new(ServerState::new(crate::state::ServerStateOptions {
            session_name: session_name.to_string(),
            session_cwd: ilium_platform::paths::canonicalize(directory.path())
                .expect("canonical test launch directory"),
            home_dir: directory.path().to_path_buf(),
            snapshot_path: directory.path().join("progress-monitor.snapshot.json"),
            socket_path: directory.path().join("test.sock"),
            detection_config: crate::config::DetectionConfig::default(),
            notifications_config: crate::config::NotificationsConfig::default(),
            sound_settings: crate::sounds::test_settings(ilium_sound::SoundSettings::default()),
            sound_requests,
            custom_signatures: Vec::new(),
            agent_debug_menu_enabled: false,
            progress_monitor_enabled: true,
        }));
        // Pane admission runs repository probes on the execution service, so a
        // fixture without one would have every spawn rejected and its node removed.
        assert!(state
            .execution
            .set(crate::execution::ServerExecution::start().expect("finite server bank"))
            .is_ok());
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

    #[tokio::test]
    async fn queued_trigger_cannot_cross_pty_replacement_and_fresh_owner_has_real_readback() {
        let (state, pane_id, _directory) =
            state_with_one_terminal_pane("trigger-owner-fence").await;
        let original_input = {
            let panes = state.panes.read().await;
            let Some(PaneResource::Terminal(runtime)) = panes.get(&pane_id) else {
                panic!("terminal fixture");
            };
            runtime.session.input_handle()
        };
        {
            let mut accepted = state.text_trigger_settings.write().await;
            accepted.settings = ilium_ipc::TextTriggerSettings {
                triggers: vec![ilium_ipc::TextTrigger {
                    id: "owner-fenced-trigger".into(),
                    regexp: "never-matched-fixture-marker".into(),
                    message: "fresh owner literal receipt".into(),
                    target: ilium_ipc::TextTriggerTarget::Terminals,
                    ..ilium_ipc::TextTrigger::default()
                }],
            };
            accepted.revision = 1;
        }
        let previous = state
            .panes
            .write()
            .await
            .remove(&pane_id)
            .expect("original pane");
        spawn_and_register_pane(
            &state,
            pane_id,
            PaneSnapshotKind::Terminal(TerminalOrigin::Command(long_running_pane_command())),
        )
        .await
        .expect("replacement isolated PTY");
        assert!(!submit_text_trigger_if_current(
            &state,
            pane_id,
            "owner-fenced-trigger",
            "fresh owner literal receipt",
            &original_input,
        )
        .await
        .expect("stale semantic decision rejected"));
        let replacement_input = {
            let panes = state.panes.read().await;
            let Some(PaneResource::Terminal(runtime)) = panes.get(&pane_id) else {
                panic!("replacement fixture");
            };
            runtime.session.input_handle()
        };
        assert!(submit_text_trigger_if_current(
            &state,
            pane_id,
            "owner-fenced-trigger",
            "fresh owner literal receipt",
            &replacement_input,
        )
        .await
        .expect("fresh semantic decision acknowledged"));
        wait_for_settled_output_sequence(&state, pane_id).await;
        let text =
            crate::pane::read_current_terminal_screen(&state, pane_id, vt100::Screen::contents)
                .await
                .expect("authoritative replacement screen");
        assert!(text.contains("fresh owner literal receipt"), "{text}");
        teardown_pane_resource(pane_id, previous);
        teardown_state_panes(&state);
    }

    #[tokio::test]
    async fn initial_attach_includes_authoritative_agent_detection_settings_before_sync_complete() {
        let (state, _pane_id, _directory) =
            state_with_one_terminal_pane("agent-detection-attach-settings").await;

        let events = initial_state_events(&state, true, false).await;
        let settings_index = events
            .iter()
            .position(|event| matches!(event, ServerEvent::AgentDetectionSettingsChanged { .. }))
            .expect("attach sends detection settings");
        let sync_index = events
            .iter()
            .position(|event| matches!(event, ServerEvent::InitialStateSyncComplete))
            .expect("attach sends sync boundary");
        let ServerEvent::AgentDetectionSettingsChanged {
            result: Ok(settings),
            ..
        } = &events[settings_index]
        else {
            panic!("initial settings must be authoritative success");
        };

        assert_eq!(settings.working_poll_seconds, 10);
        assert_eq!(settings.idle_poll_seconds, 45);
        assert!(settings.custom_signatures.is_empty());
        assert!(settings_index < sync_index);
        teardown_state_panes(&state);
    }

    #[tokio::test]
    async fn visible_recovery_uses_reserved_storage_when_general_quota_is_full() {
        let directory = tempfile::tempdir().expect("create visible recovery admission directory");
        let state = snapshot_io_handler_state(&directory);
        assert!(state
            .execution
            .set(crate::execution::ServerExecution::start().expect("finite server bank"))
            .is_ok());
        let execution = state.execution.get().expect("execution bank");
        let general_before = execution.client.quota_group().snapshot();
        let general_remaining = general_before
            .limits
            .worker_bytes
            .checked_sub(general_before.worker_bytes)
            .expect("general storage usage remains within its configured quota");
        let general_quota_filler = execution
            .client
            .try_reserve_storage(general_remaining)
            .expect("fill all remaining general worker-byte storage");
        assert_eq!(
            execution.client.quota_group().snapshot().worker_bytes,
            general_before.limits.worker_bytes
        );

        let storage = tokio::time::timeout(
            Duration::from_secs(1),
            reserve_connection_event_storage(
                &state,
                TERMINAL_RECOVERY_EVENT_MAX_BYTES,
                1,
                ConnectionEventStorageClass::VisiblePaneRecovery,
                Some(NodeId(1)),
            ),
        )
        .await
        .expect("visible-pane recovery must bypass unrelated general storage pressure")
        .expect("visible-pane recovery admission succeeds")
        .expect("one recovery event owns byte admission");
        assert!(storage.resident_bytes() >= 2 * TERMINAL_RECOVERY_EVENT_MAX_BYTES);

        drop(storage);
        drop(general_quota_filler);
        execution.request_shutdown();
    }

    #[tokio::test]
    async fn initial_state_snapshot_stays_byte_admitted_until_direct_queue_consumes_it() {
        let (state, _pane_id, _directory) =
            state_with_one_terminal_pane("initial-state-snapshot-storage").await;
        // The fixture already installed the execution service its pane spawn needs.
        assert!(state.execution.get().is_some());
        let (direct_tx, mut direct_rx) = DirectEventSender::channel(16);

        send_initial_state(&state, &direct_tx, false).await;
        drop(direct_tx);

        let mut snapshot_storage = None;
        let mut batch_storage = None;
        let mut queued_events = 0;
        while let Some(queued) = direct_rx.recv_queued().await {
            let storage = queued
                .producer_storage
                .expect("every state synchronization event retains byte admission");
            if let Some(batch_storage) = &batch_storage {
                assert!(std::sync::Arc::ptr_eq(batch_storage, &storage));
            } else {
                batch_storage = Some(std::sync::Arc::clone(&storage));
            }
            queued_events += 1;
            if matches!(queued.event, ServerEvent::PaneStateSnapshot { .. }) {
                snapshot_storage = Some(storage);
            }
        }
        assert!(queued_events > 0);
        let storage = snapshot_storage.expect("snapshot queue entry retains byte admission");
        assert!(std::sync::Arc::ptr_eq(
            &storage,
            batch_storage.as_ref().expect("batch has a storage lease")
        ));
        assert!(storage.resident_bytes() > 0);
        teardown_state_panes(&state);
    }

    #[tokio::test]
    async fn invalid_agent_detection_update_is_rejected_without_changing_live_settings() {
        let (state, _pane_id, _directory) =
            state_with_one_terminal_pane("agent-detection-invalid-settings").await;
        let (direct_tx, mut direct_rx) = DirectEventSender::channel(1);
        let before = state.agent_detection_settings_snapshot().await;
        let before_wire = crate::config::agent_detection_settings(&before.0, &before.1);

        handle_request(
            &state,
            ClientRequest::UpdateAgentDetectionSettings {
                request_id: None,
                settings: ilium_ipc::AgentDetectionSettings {
                    working_poll_seconds: 10,
                    idle_poll_seconds: 45,
                    custom_signatures: vec![ilium_ipc::CustomAgentSignature {
                        name_substring: "  ".to_string(),
                        class: ilium_core::AgentClass::Claude,
                    }],
                },
            },
            &direct_tx,
        )
        .await;

        assert!(matches!(
            direct_rx.recv().await,
            Some(ServerEvent::AgentDetectionSettingsChanged { result: Err(_), .. })
        ));
        let after = state.agent_detection_settings_snapshot().await;
        let after_wire = crate::config::agent_detection_settings(&after.0, &after.1);
        assert_eq!(before_wire, after_wire);
        teardown_state_panes(&state);
    }

    #[tokio::test]
    async fn correlated_agent_detection_rejection_echoes_id_without_changing_live_settings() {
        let (state, _pane_id, _directory) =
            state_with_one_terminal_pane("agent-detection-invalid-settings").await;
        let (direct_tx, mut direct_rx) = DirectEventSender::channel(1);
        let before = state.agent_detection_settings_snapshot().await;
        let before_wire = crate::config::agent_detection_settings(&before.0, &before.1);

        handle_request(
            &state,
            ClientRequest::UpdateAgentDetectionSettings {
                request_id: Some(73),
                settings: ilium_ipc::AgentDetectionSettings {
                    working_poll_seconds: 10,
                    idle_poll_seconds: 45,
                    custom_signatures: vec![ilium_ipc::CustomAgentSignature {
                        name_substring: "  ".to_string(),
                        class: ilium_core::AgentClass::Claude,
                    }],
                },
            },
            &direct_tx,
        )
        .await;

        assert!(matches!(
            direct_rx.recv().await,
            Some(ServerEvent::AgentDetectionSettingsChanged {
                request_id: Some(73),
                result: Err(_)
            })
        ));
        let after = state.agent_detection_settings_snapshot().await;
        let after_wire = crate::config::agent_detection_settings(&after.0, &after.1);
        assert_eq!(before_wire, after_wire);
        teardown_state_panes(&state);
    }

    #[tokio::test]
    async fn accepted_agent_detection_update_persists_and_replaces_the_live_detector_snapshot() {
        let (state, _pane_id, _directory) =
            state_with_one_terminal_pane("agent-detection-update-settings").await;
        let config_directory = tempfile::tempdir().expect("create isolated config directory");
        std::fs::write(
            config_directory.path().join("config.toml"),
            "[detection]\nauto_answer_interstitial_prompts = false\n",
        )
        .expect("seed the existing server-only setting");
        state
            .agent_detection_settings
            .write()
            .await
            .detection
            .auto_answer_interstitial_prompts = false;
        let desired = ilium_ipc::AgentDetectionSettings {
            working_poll_seconds: 2,
            idle_poll_seconds: 17,
            custom_signatures: vec![ilium_ipc::CustomAgentSignature {
                name_substring: "  MY-AGENT  ".to_string(),
                class: ilium_core::AgentClass::Codex,
            }],
        };

        let accepted =
            apply_agent_detection_settings(&state, desired, config_directory.path().to_path_buf())
                .await
                .expect("valid settings apply");

        assert_eq!(accepted.working_poll_seconds, 2);
        assert_eq!(accepted.idle_poll_seconds, 17);
        assert_eq!(accepted.custom_signatures[0].name_substring, "my-agent");
        let (detection, signatures) = state.agent_detection_settings_snapshot().await;
        assert_eq!(detection.working_poll_interval, Duration::from_secs(2));
        assert_eq!(detection.idle_poll_interval, Duration::from_secs(17));
        assert!(!detection.auto_answer_interstitial_prompts);
        assert_eq!(signatures[0].name_substring, "my-agent");

        let persisted =
            crate::config::load(config_directory.path()).expect("new config file reloads");
        assert_eq!(
            persisted.detection.working_poll_interval,
            Duration::from_secs(2)
        );
        assert_eq!(
            persisted.detection.idle_poll_interval,
            Duration::from_secs(17)
        );
        assert!(!persisted.detection.auto_answer_interstitial_prompts);
        assert_eq!(persisted.custom_signatures.len(), 1);
        assert_eq!(persisted.custom_signatures[0].name_substring, "my-agent");
        teardown_state_panes(&state);
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

    fn wait_test_progress(
        monitor_id: u64,
        status: ilium_core::ProgressTaskStatus,
    ) -> ilium_core::PaneProgress {
        ilium_core::PaneProgress::new(
            monitor_id,
            ilium_core::ProgressTaskReport::new(
                "wait-job".to_string(),
                status,
                if status.is_terminal() { 100.0 } else { 10.0 },
                "step".to_string(),
                String::new(),
                None,
            )
            .unwrap(),
            1,
        )
        .unwrap()
    }

    async fn install_wait_test_monitor(state: &Arc<ServerState>, pane_id: NodeId, monitor_id: u64) {
        let mut panes = state.panes.write().await;
        let PaneResource::Terminal(runtime) = panes.get_mut(&pane_id).unwrap() else {
            panic!("fixture pane must remain terminal");
        };
        runtime.detected_agent_class = Some(ilium_core::AgentClass::Codex);
        runtime
            .install_progress_monitor(crate::progress_monitor::ProgressMonitorRegistration {
                monitor_id,
                pane_id,
                command: "true".to_string(),
                interval: Duration::from_secs(60),
                initial_progress: wait_test_progress(
                    monitor_id,
                    ilium_core::ProgressTaskStatus::Running,
                ),
            })
            .unwrap();
    }

    async fn settle_wait_test_monitor(
        state: &Arc<ServerState>,
        pane_id: NodeId,
        monitor_id: u64,
    ) -> ilium_core::PaneProgress {
        let done = wait_test_progress(monitor_id, ilium_core::ProgressTaskStatus::Done);
        let mut panes = state.panes.write().await;
        let PaneResource::Terminal(runtime) = panes.get_mut(&pane_id).unwrap() else {
            panic!("fixture pane must remain terminal");
        };
        assert!(runtime.update_progress_monitor_progress(monitor_id, done.clone()));
        done
    }

    async fn wait_test_delivery(
        state: &Arc<ServerState>,
        pane_id: NodeId,
    ) -> crate::pane::ProgressDeliveryState {
        let panes = state.panes.read().await;
        let Some(PaneResource::Terminal(runtime)) = panes.get(&pane_id) else {
            panic!("fixture pane must remain terminal");
        };
        runtime.progress_monitor.as_ref().unwrap().result_delivery
    }

    async fn next_wait_reply(
        receiver: &mut crate::ipc::direct_events::DirectEventReceiver,
    ) -> Option<ilium_ipc::ProgressWaitOutcome> {
        match tokio::time::timeout(Duration::from_millis(200), receiver.recv()).await {
            Ok(Some(ServerEvent::ProgressWaitCompleted { result, .. })) => Some(result.unwrap()),
            Ok(other) => panic!("unexpected reply {other:?}"),
            Err(_) => None,
        }
    }

    #[tokio::test]
    async fn held_progress_wait_takes_the_outcome_instead_of_the_composer() {
        let (state, pane_id, _directory) = state_with_one_terminal_pane("progress-held-wait").await;

        // A connected waiter receives the settled outcome; no composer
        // notification may follow.
        install_wait_test_monitor(&state, pane_id, 7).await;
        let (reply, mut receiver) = DirectEventSender::channel(8);
        handle_wait_pane_progress_monitor(&state, 41, pane_id, 7, &reply).await;
        assert!(
            next_wait_reply(&mut receiver).await.is_none(),
            "running monitor holds the wait"
        );
        let done = settle_wait_test_monitor(&state, pane_id, 7).await;
        assert!(hand_outcome_to_progress_waiters(&state, pane_id, 7, &done).await);
        let outcome = next_wait_reply(&mut receiver).await.expect("settled reply");
        assert_eq!(outcome.end, ilium_ipc::ProgressWaitEnd::Settled);
        assert!(outcome.composer_notice_suppressed);
        assert_eq!(outcome.progress, Some(done));
        assert_eq!(
            wait_test_delivery(&state, pane_id).await,
            crate::pane::ProgressDeliveryState::CollectedByWaiter
        );
        {
            let panes = state.panes.read().await;
            let Some(PaneResource::Terminal(runtime)) = panes.get(&pane_id) else {
                panic!("fixture pane must remain terminal");
            };
            assert_eq!(
                runtime.progress_reconcile_action(),
                crate::pane::ProgressReconcileAction::None,
                "a collected outcome is never redelivered"
            );
        }

        // A waiter whose process died leaves the normal notification intact.
        install_wait_test_monitor(&state, pane_id, 8).await;
        let (reply, receiver) = DirectEventSender::channel(8);
        handle_wait_pane_progress_monitor(&state, 42, pane_id, 8, &reply).await;
        drop(receiver);
        let done = settle_wait_test_monitor(&state, pane_id, 8).await;
        assert!(!hand_outcome_to_progress_waiters(&state, pane_id, 8, &done).await);
        assert_eq!(
            wait_test_delivery(&state, pane_id).await,
            crate::pane::ProgressDeliveryState::NotQueued
        );

        // Waiting on an already-settled monitor answers at once and claims it.
        let (reply, mut receiver) = DirectEventSender::channel(8);
        handle_wait_pane_progress_monitor(&state, 43, pane_id, 8, &reply).await;
        let outcome = next_wait_reply(&mut receiver)
            .await
            .expect("immediate reply");
        assert_eq!(outcome.end, ilium_ipc::ProgressWaitEnd::Settled);
        assert!(outcome.composer_notice_suppressed);

        // Replacement and clear both end a held wait.
        install_wait_test_monitor(&state, pane_id, 9).await;
        let (reply, mut receiver) = DirectEventSender::channel(8);
        handle_wait_pane_progress_monitor(&state, 44, pane_id, 9, &reply).await;
        install_wait_test_monitor(&state, pane_id, 10).await;
        let outcome = next_wait_reply(&mut receiver)
            .await
            .expect("superseded reply");
        assert_eq!(outcome.end, ilium_ipc::ProgressWaitEnd::Superseded);
        assert!(!outcome.composer_notice_suppressed);

        let (reply, mut receiver) = DirectEventSender::channel(8);
        handle_wait_pane_progress_monitor(&state, 45, pane_id, 10, &reply).await;
        {
            let mut panes = state.panes.write().await;
            let PaneResource::Terminal(runtime) = panes.get_mut(&pane_id).unwrap() else {
                panic!("fixture pane must remain terminal");
            };
            runtime.cancel_progress_monitor();
        }
        let outcome = next_wait_reply(&mut receiver).await.expect("cleared reply");
        assert_eq!(outcome.end, ilium_ipc::ProgressWaitEnd::Cleared);

        // Waiting with a stale ID reports the replacement immediately.
        install_wait_test_monitor(&state, pane_id, 11).await;
        let (reply, mut receiver) = DirectEventSender::channel(8);
        handle_wait_pane_progress_monitor(&state, 46, pane_id, 9, &reply).await;
        let outcome = next_wait_reply(&mut receiver).await.expect("stale reply");
        assert_eq!(outcome.end, ilium_ipc::ProgressWaitEnd::Superseded);
        teardown_state_panes(&state);
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
                    String::new(),
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
    async fn reconcile_action_redelivers_unreceived_outcomes_only_to_supported_agents() {
        use crate::pane::ProgressReconcileAction as Action;
        use crate::persistence::PersistedProgressDeliveryState as Persisted;

        let (state, pane_id, _directory) =
            state_with_one_terminal_pane("progress-reconcile-action").await;
        let mut panes = state.panes.write().await;
        let PaneResource::Terminal(runtime) = panes.get_mut(&pane_id).unwrap() else {
            panic!("fixture pane must remain terminal");
        };
        let report = |status| {
            ilium_core::ProgressTaskReport::new(
                "job".to_string(),
                status,
                100.0,
                "finished".to_string(),
                String::new(),
                None,
            )
            .unwrap()
        };
        runtime
            .install_progress_monitor(crate::progress_monitor::ProgressMonitorRegistration {
                monitor_id: 7,
                pane_id,
                command: "true".to_string(),
                interval: Duration::from_secs(60),
                initial_progress: ilium_core::PaneProgress::new(
                    7,
                    report(ilium_core::ProgressTaskStatus::Running),
                    1,
                )
                .unwrap(),
            })
            .unwrap();
        // Running with no coordinator task: nothing can ever settle it.
        assert_eq!(
            runtime.progress_reconcile_action(),
            Action::ObservationStopped { monitor_id: 7 }
        );
        let done =
            ilium_core::PaneProgress::new(7, report(ilium_core::ProgressTaskStatus::Done), 2)
                .unwrap();
        assert!(runtime.update_progress_monitor_progress(7, done));

        runtime
            .restore_progress_delivery_state(Persisted::NotDeliverable)
            .unwrap();
        assert_eq!(
            runtime.progress_reconcile_action(),
            Action::None,
            "a plain shell has no composer, so nothing may be typed into it"
        );

        runtime.detected_agent_class = Some(ilium_core::AgentClass::Codex);
        for (persisted, expected) in [
            (Persisted::NotDeliverable, false),
            (Persisted::NotQueued, false),
            (Persisted::Queued, false),
            (Persisted::Attempted, true),
            (Persisted::Uncertain, true),
        ] {
            runtime.restore_progress_delivery_state(persisted).unwrap();
            assert_eq!(
                runtime.progress_reconcile_action(),
                Action::Redeliver {
                    monitor_id: 7,
                    possible_duplicate: expected
                },
                "{persisted:?}"
            );
        }
        runtime
            .restore_progress_delivery_state(Persisted::DeliveredToPty)
            .unwrap();
        assert_eq!(runtime.progress_reconcile_action(), Action::None);
        drop(panes);
        teardown_state_panes(&state);
    }

    #[tokio::test]
    async fn terminal_outcome_notification_is_once_and_plain_shell_keeps_result() {
        let (state, pane_id, _directory) =
            state_with_one_terminal_pane("progress-outcome-notification-once").await;
        let mut panes = state.panes.write().await;
        let PaneResource::Terminal(runtime) = panes.get_mut(&pane_id).unwrap() else {
            panic!("fixture pane must remain terminal");
        };
        for monitor_id in [1, 2] {
            let initial_progress = ilium_core::PaneProgress::new(
                monitor_id,
                ilium_core::ProgressTaskReport::new(
                    "job".to_string(),
                    ilium_core::ProgressTaskStatus::NotStartedYet,
                    0.0,
                    "pending".to_string(),
                    String::new(),
                    None,
                )
                .unwrap(),
                1,
            )
            .unwrap();
            runtime
                .install_progress_monitor(crate::progress_monitor::ProgressMonitorRegistration {
                    monitor_id,
                    pane_id,
                    command: "true".to_string(),
                    interval: Duration::from_secs(60),
                    initial_progress,
                })
                .unwrap();
            assert!(!runtime.claim_progress_outcome_notification(monitor_id));
            let final_progress = ilium_core::PaneProgress::new(
                monitor_id,
                ilium_core::ProgressTaskReport::new(
                    "job".to_string(),
                    ilium_core::ProgressTaskStatus::Done,
                    100.0,
                    "finished".to_string(),
                    String::new(),
                    None,
                )
                .unwrap(),
                2,
            )
            .unwrap();
            assert!(runtime.update_progress_monitor_progress(monitor_id, final_progress));
            assert!(runtime.claim_progress_outcome_notification(monitor_id));
            assert!(!runtime.claim_progress_outcome_notification(monitor_id));
        }
        assert!(!runtime.claim_progress_outcome_notification(1));
        let final_progress = runtime
            .progress_monitor
            .as_ref()
            .unwrap()
            .latest_progress
            .clone();
        drop(panes);
        state
            .tree
            .write()
            .await
            .set_pane_progress(pane_id, Some(final_progress.clone()))
            .unwrap();
        crate::agent_delivery::deliver_result(
            Arc::clone(&state),
            pane_id,
            2,
            "task finished".to_string(),
        )
        .await
        .unwrap();
        let panes = state.panes.read().await;
        let PaneResource::Terminal(runtime) = panes.get(&pane_id).unwrap() else {
            panic!("fixture pane must remain terminal");
        };
        assert_eq!(
            runtime.progress_monitor.as_ref().unwrap().result_delivery,
            crate::pane::ProgressDeliveryState::NotDeliverable
        );
        drop(panes);
        assert_eq!(
            state.tree.read().await.pane_progress(pane_id),
            Some(&final_progress)
        );
        teardown_state_panes(&state);
    }

    #[tokio::test]
    async fn set_pane_progress_monitor_runs_the_command_and_broadcasts_reported_progress() {
        let (state, pane_id, _directory) =
            state_with_one_terminal_pane("progress-monitor-set-and-report").await;
        let mut events = state.events.subscribe_owned();
        let (direct_tx, mut direct_rx) = DirectEventSender::channel(1);

        assert!(
            !handle_request(
                &state,
                ClientRequest::SetPaneProgressMonitor {
                    request_id: 11,
                    pane_id,
                    command: crate::progress_monitor::test_probes::emit_text(r#"{"job_id":"render-11","status":"running","percent":42.5,"message":"frame 10/100"}"#),
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
        let (direct_tx, mut direct_rx) = DirectEventSender::channel(4);
        handle_request(
            &state,
            ClientRequest::SetPaneProgressMonitor {
                request_id: 15,
                pane_id,
                command: crate::progress_monitor::test_probes::emit_text(
                    r#"{"job_id":"kept-job","status":"running","percent":25,"message":"healthy"}"#,
                ),
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
                command: crate::progress_monitor::test_probes::emit_text("not-json"),
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
        let command = crate::progress_monitor::test_probes::append_marker_pause_emit(
            &invocation_log,
            r#"{"job_id":"idempotent-job","status":"running","percent":12,"message":"running"}"#,
        );
        let (direct_tx, mut direct_rx) = DirectEventSender::channel(2);
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
                .expect("probe invocation log")
                // Windows `echo` terminates the marker with CRLF; a rerun would
                // still leave a second marker after trimming.
                .trim(),
            "x",
            "the exact retry must not rerun preflight"
        );

        handle_request(
            &state,
            ClientRequest::SetPaneProgressMonitor {
                request_id: 19,
                pane_id,
                command: crate::progress_monitor::test_probes::emit_text(r#"{"job_id":"collision","status":"running","percent":1,"message":"different"}"#),
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
        let (direct_tx, mut direct_rx) = DirectEventSender::channel(1);
        handle_request(
            &state,
            ClientRequest::SetPaneProgressMonitor {
                request_id: 20,
                pane_id,
                command: crate::progress_monitor::test_probes::emit_text(r#"{"job_id":"durable-job","status":"running","percent":17,"message":"running"}"#),
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
        let (direct_tx, mut direct_rx) = DirectEventSender::channel(3);
        handle_request(
            &state,
            ClientRequest::SetPaneProgressMonitor {
                request_id: 23,
                pane_id,
                command: crate::progress_monitor::test_probes::emit_text(r#"{"job_id":"preserved-job","status":"running","percent":23,"message":"running"}"#),
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
                command: crate::progress_monitor::test_probes::emit_text(r#"{"job_id":"unacknowledged-job","status":"running","percent":24,"message":"running"}"#),
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
            crate::progress_monitor::test_probes::stderr_then_exit_seven(),
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
                | crate::pane::ProgressDeliveryState::NotDeliverable
                | crate::pane::ProgressDeliveryState::CollectedByWaiter
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
            crate::progress_monitor::test_probes::emit_text(
                r#"{"job_id":"replacement-job","status":"running","percent":1,"message":"different process"}"#,
            ),
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
        let mut events = state.events.subscribe_owned();
        let (direct_tx, mut direct_rx) = DirectEventSender::channel(1);

        handle_request(
            &state,
            ClientRequest::SetPaneProgressMonitor {
                request_id: 21,
                pane_id,
                command: crate::progress_monitor::test_probes::emit_text(r#"{"job_id":"render-21","status":"running","percent":10,"message":"starting"}"#),
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
        let (direct_tx, mut direct_rx) = DirectEventSender::channel(1);

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
    async fn reconciler_fails_a_monitor_whose_observation_task_vanished() {
        let (state, pane_id, _directory) =
            state_with_one_terminal_pane("progress-monitor-reconcile-lost-task").await;
        let mut events = state.events.subscribe_owned();
        let (direct_tx, mut direct_rx) = DirectEventSender::channel(8);

        handle_request(
            &state,
            ClientRequest::SetPaneProgressMonitor {
                request_id: 51,
                pane_id,
                command: crate::progress_monitor::test_probes::emit_text(
                    r#"{"job_id":"render-51","status":"running","percent":5,"message":"running"}"#,
                ),
                interval_seconds: 1,
            },
            &direct_tx,
        )
        .await;
        assert!(matches!(
            direct_rx.recv().await,
            Some(ServerEvent::ProgressMonitorSetCompleted {
                request_id: 51,
                result: Ok(_),
                ..
            })
        ));

        // Replace the coordinator with a task that has already ended, as a
        // panicked or lost coordinator would leave it.
        {
            let mut panes = state.panes.write().await;
            let Some(PaneResource::Terminal(runtime)) = panes.get_mut(&pane_id) else {
                panic!("fixture pane must be a terminal");
            };
            let finished = tokio::spawn(async {});
            while !finished.is_finished() {
                tokio::task::yield_now().await;
            }
            runtime.set_progress_monitor_task(finished);
        }

        let mut attempts = std::collections::HashMap::new();
        crate::progress_watchdog::reconcile(&state, &mut attempts).await;

        let failed = tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                match events.recv().await {
                    Ok(ServerEvent::PaneProgressChanged {
                        pane_id: event_pane_id,
                        progress: Some(progress),
                    }) if event_pane_id == pane_id && progress.monitor_health.is_failed() => {
                        return true;
                    }
                    Ok(_) => {}
                    Err(error) => panic!("progress event stream closed: {error}"),
                }
            }
        })
        .await
        .unwrap_or(false);
        assert!(
            failed,
            "a monitor that lost its observation task must become failed evidence"
        );

        teardown_state_panes(&state);
    }

    #[tokio::test]
    async fn disabling_progress_monitor_setting_rejects_new_requests_and_stops_running_ones() {
        let (state, pane_id, _directory) =
            state_with_one_terminal_pane("progress-monitor-disable-setting").await;
        let mut events = state.events.subscribe_owned();
        let (direct_tx, mut direct_rx) = DirectEventSender::channel(1);

        handle_request(
            &state,
            ClientRequest::SetPaneProgressMonitor {
                request_id: 41,
                pane_id,
                command: crate::progress_monitor::test_probes::emit_text(
                    r#"{"job_id":"render-41","status":"running","percent":5,"message":"running"}"#,
                ),
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
                        progress: Some(progress),
                    }) if event_pane_id == pane_id && progress.monitor_health.is_failed() => {
                        return true;
                    }
                    Ok(_) => {}
                    Err(error) => panic!("progress event stream closed: {error}"),
                }
            }
        })
        .await
        .unwrap_or(false);
        assert!(
            cleared,
            "disabling the setting must turn the running monitor into failed evidence"
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
        let (sound_requests, sound_task) = crate::sounds::spawn(
            Arc::new(crate::NoopSoundPlayer),
            crate::execution::test_general_client(),
        );
        let state = Arc::new(ServerState::new(crate::state::ServerStateOptions {
            session_name: "pane-scoped-recovery".to_string(),
            session_cwd: ilium_platform::paths::canonicalize(directory.path())
                .expect("canonical test launch directory"),
            home_dir: directory.path().to_path_buf(),
            snapshot_path: directory.path().join("pane-scoped-recovery.snapshot.json"),
            socket_path: directory.path().join("test.sock"),
            detection_config: crate::config::DetectionConfig::default(),
            notifications_config: crate::config::NotificationsConfig::default(),
            sound_settings: crate::sounds::test_settings(ilium_sound::SoundSettings::default()),
            sound_requests,
            custom_signatures: Vec::new(),
            agent_debug_menu_enabled: false,
            progress_monitor_enabled: true,
        }));
        // Pane admission runs repository probes on the execution service, so a
        // fixture without one would have every spawn rejected and its node removed.
        assert!(state
            .execution
            .set(crate::execution::ServerExecution::start().expect("finite server bank"))
            .is_ok());
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

        // The forwarder publishes the exact retained tail for its gap,
        // rather than a parser-resetting copy of the entire journal.
        let (missing_input, other_input) = {
            let panes = state.panes.read().await;
            let Some(PaneResource::Terminal(missing)) = panes.get(&missing_pane_id) else {
                panic!("missing pane is not a terminal");
            };
            let Some(PaneResource::Terminal(other)) = panes.get(&current_pane_id) else {
                panic!("other pane is not a terminal");
            };
            (missing.session.input_handle(), other.session.input_handle())
        };
        let mut output_rx = state.events.subscribe_owned();
        let published_sequence = broadcast_terminal_recovery_after(
            &state,
            missing_pane_id,
            &missing_input,
            missing_sequence.saturating_sub(1),
        )
        .await
        .expect("retained output needs a recovery broadcast");
        let published_event = tokio::time::timeout(std::time::Duration::from_secs(1), async {
            loop {
                let event = output_rx.recv().await.expect("broadcast remains open");
                if matches!(&event, ServerEvent::ScreenUpdate { pane_id, .. } if *pane_id == missing_pane_id) {
                    break event;
                }
            }
        })
        .await
        .expect("missing pane recovery was not broadcast");
        let ServerEvent::ScreenUpdate {
            first_sequence,
            sequence,
            bytes,
            ..
        } = published_event
        else {
            panic!("retained tail must be a live update");
        };
        assert_eq!(first_sequence, missing_sequence);
        assert_eq!(sequence, published_sequence);
        assert!(!bytes.is_empty());
        assert_eq!(
            broadcast_terminal_recovery_after(
                &state,
                missing_pane_id,
                &other_input,
                missing_sequence.saturating_sub(1),
            )
            .await,
            None,
            "a replaced pane owner must not broadcast its predecessor's output"
        );

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
        let (sound_requests, sound_task) = crate::sounds::spawn(
            Arc::new(crate::NoopSoundPlayer),
            crate::execution::test_general_client(),
        );
        let state = Arc::new(ServerState::new(crate::state::ServerStateOptions {
            session_name: "protected-split-restructure".to_string(),
            session_cwd: ilium_platform::paths::canonicalize(directory.path())
                .expect("canonical test launch directory"),
            home_dir: directory.path().to_path_buf(),
            snapshot_path: directory.path().join("protected-split.snapshot.json"),
            socket_path: directory.path().join("test.sock"),
            detection_config: crate::config::DetectionConfig::default(),
            notifications_config: crate::config::NotificationsConfig::default(),
            sound_settings: crate::sounds::test_settings(ilium_sound::SoundSettings::default()),
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
        let mut events = state.events.subscribe_owned();
        let (direct_tx, mut direct_rx) = DirectEventSender::channel(8);

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
            &[],
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
            &[],
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
        let (sound_requests, sound_task) = crate::sounds::spawn(
            Arc::new(crate::NoopSoundPlayer),
            crate::execution::test_general_client(),
        );
        let state = Arc::new(ServerState::new(crate::state::ServerStateOptions {
            session_name: "revert-orphan-debug".to_string(),
            session_cwd: ilium_platform::paths::canonicalize(directory.path())
                .expect("canonical test launch directory"),
            home_dir: directory.path().to_path_buf(),
            snapshot_path: directory.path().join("revert-orphan-debug.snapshot.json"),
            socket_path: directory.path().join("test.sock"),
            detection_config: crate::config::DetectionConfig::default(),
            notifications_config: crate::config::NotificationsConfig::default(),
            sound_settings: crate::sounds::test_settings(ilium_sound::SoundSettings::default()),
            sound_requests,
            custom_signatures: Vec::new(),
            agent_debug_menu_enabled: true,
            progress_monitor_enabled: true,
        }));
        // Pane admission runs repository probes on the execution service, so a
        // fixture without one would have every spawn rejected and its node removed.
        assert!(state
            .execution
            .set(crate::execution::ServerExecution::start().expect("finite server bank"))
            .is_ok());

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

        let (direct_tx, mut direct_rx) = DirectEventSender::channel(8);
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
    #[tokio::test]
    async fn recommended_restructure_orders_undo_and_round_trips_persistence_and_attach() {
        use ilium_core::animation_recommendation::{
            AnimationRecommendation, PlanAnimationEntry, RecommendedRestructurePlan, ResourcePolicy,
        };
        use std::future::Future;
        struct StopSound(tokio::task::JoinHandle<()>);
        impl Drop for StopSound {
            fn drop(&mut self) {
                self.0.abort();
            }
        }
        let directory = tempfile::tempdir().unwrap();
        let (sound_requests, sound_task) = crate::sounds::spawn(
            Arc::new(crate::NoopSoundPlayer),
            crate::execution::test_general_client(),
        );
        let _sound_guard = StopSound(sound_task);
        let state = Arc::new(ServerState::new(crate::state::ServerStateOptions {
            session_name: "semantic-transaction".into(),
            session_cwd: ilium_platform::paths::canonicalize(directory.path()).unwrap(),
            home_dir: directory.path().to_path_buf(),
            snapshot_path: directory.path().join("semantic.snapshot.json"),
            socket_path: directory.path().join("test.sock"),
            detection_config: Default::default(),
            notifications_config: Default::default(),
            sound_settings: crate::sounds::test_settings(Default::default()),
            sound_requests,
            custom_signatures: vec![],
            agent_debug_menu_enabled: false,
            progress_monitor_enabled: true,
        }));
        let (project_id, pane_id) = {
            let mut tree = state.tree.write().await;
            let project = tree.project_ids()[0];
            let group = tree.add_group(project, "work").unwrap();
            let pane = tree
                .add_pane(group, "shell", PaneContentKind::Terminal)
                .unwrap();
            (project, pane)
        };
        let before = state.tree.read().await.clone();
        let revisions = before.project_activity_revisions(project_id).unwrap();
        let recommendation = AnimationRecommendation {
            version: 1,
            kind: "shoreline".into(),
            resources: ResourcePolicy::Catalog,
            parameters: vec![],
        };
        let plan = RecommendedRestructurePlan {
            structure: RestructurePlan {
                children: vec![RestructureNode::Pane {
                    id: pane_id,
                    title: "Work".into(),
                    short_title: None,
                    icon: None,
                }],
            },
            expected_animation_generation: 0,
            project: recommendation.clone(),
            entries: vec![PlanAnimationEntry {
                path: vec![0],
                recommendation,
            }],
        };
        let (direct_tx, mut direct_rx) = DirectEventSender::channel(8);
        let held_undo = state.restructure_undo.lock().await;
        let apply = handle_apply_recommended_project_restructure_plan(
            &state,
            project_id,
            plan.clone(),
            &revisions,
            &[],
            &direct_tx,
        );
        tokio::pin!(apply);
        tokio::time::timeout(
            Duration::from_secs(5),
            std::future::poll_fn(|cx| {
                assert!(apply.as_mut().poll(cx).is_pending());
                if state.tree.try_read().is_err() {
                    std::task::Poll::Ready(())
                } else {
                    // Eligibility collection runs outside the transaction lock;
                    // wait for its completion before testing undo lock ordering.
                    cx.waker().wake_by_ref();
                    std::task::Poll::Pending
                }
            }),
        )
        .await
        .expect("apply reaches the transaction while undo is held");
        assert!(state.tree.try_read().is_err());
        drop(held_undo);
        tokio::time::timeout(Duration::from_secs(5), apply)
            .await
            .unwrap();
        assert!(matches!(
            direct_rx.try_recv(),
            Ok(ServerEvent::ProjectRestructureApplied { .. })
        ));
        assert_eq!(
            state.restructure_undo.lock().await.get(&project_id),
            Some(&before)
        );
        let accepted = state.tree.read().await.clone();
        assert!(accepted
            .get(project_id)
            .unwrap()
            .inferred_animation
            .is_some());
        assert!(accepted.get(pane_id).unwrap().inferred_animation.is_some());
        assert!(state.panes.read().await.is_empty());
        let attach = initial_state_events(&state, false, false).await;
        assert!(
            matches!(&attach[0], ServerEvent::PaneStateSnapshot { tree, .. } if tree == &accepted)
        );
        crate::persistence::flush_pending_snapshot(&state).await;
        let restored = crate::persistence::load_snapshot(&state.snapshot_path)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(restored.tree, accepted);
        handle_apply_recommended_project_restructure_plan(
            &state,
            project_id,
            plan,
            &revisions,
            &[],
            &direct_tx,
        )
        .await;
        assert!(matches!(
            direct_rx.try_recv(),
            Ok(ServerEvent::ProjectRestructureRejected { .. })
        ));
        assert_eq!(*state.tree.read().await, accepted);
        assert_eq!(
            state.restructure_undo.lock().await.get(&project_id),
            Some(&before)
        );
        let later_pane = state
            .tree
            .write()
            .await
            .add_pane(project_id, "created after apply", PaneContentKind::Terminal)
            .unwrap();
        let before_cancellation = state.tree.read().await.clone();
        let held_panes = state.panes.write().await;
        {
            let mut revert = Box::pin(handle_revert_project_restructure(
                &state, project_id, &direct_tx,
            ));
            std::future::poll_fn(|cx| {
                assert!(revert.as_mut().poll(cx).is_pending());
                std::task::Poll::Ready(())
            })
            .await;
            // Dropping a connection's blocked handler must precede its commit.
        }
        assert_eq!(*state.tree.read().await, before_cancellation);
        assert!(state.tree.read().await.get(later_pane).is_some());
        assert_eq!(
            state.restructure_undo.lock().await.get(&project_id),
            Some(&before)
        );
        drop(held_panes);
        let held_tree = state.tree.write().await;
        let revert = handle_revert_project_restructure(&state, project_id, &direct_tx);
        tokio::pin!(revert);
        std::future::poll_fn(|cx| {
            assert!(revert.as_mut().poll(cx).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
        assert!(state
            .restructure_undo
            .try_lock()
            .unwrap()
            .contains_key(&project_id));
        drop(held_tree);
        tokio::time::timeout(Duration::from_secs(5), revert)
            .await
            .unwrap();
        let undone = state.tree.read().await.clone();
        assert!(undone.get(project_id).unwrap().inferred_animation.is_none());
        assert!(undone.get(pane_id).unwrap().inferred_animation.is_none());
        assert_eq!(undone.project_animation_generation(project_id).unwrap(), 2);
        assert!(!state
            .restructure_undo
            .lock()
            .await
            .contains_key(&project_id));
    }
    mod title_safety_regressions {
        include!("title_safety_tests.rs");
    }

    mod title_delivery_regressions {
        include!("title_delivery_tests.rs");
    }

    mod ordered_owner_regressions {
        use super::*;
        include!("ordered_owner_regressions.rs");
    }
}

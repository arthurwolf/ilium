//! Applies incoming `ServerEvent`s to `App`'s render-cache state. This is
//! the one place the client-local `tree`/`panes` maps are ever written to
//! from network input -- see `app.rs`'s module docs for why that's the
//! only kind of write they ever get (everything else flows the other way,
//! as a `ClientRequest`).

use ilium_core::{NodeId, NodeKind, PaneContentKind, PaneStatus};
use ilium_ipc::{PromptSubmissionSource, ServerEvent};

use crate::app::{App, PaneRuntime};
use crate::board::BoardPane;
use crate::editor_pane::EditorPane;
use crate::terminal_view::TerminalView;
use crate::trigger_settings::{event_for_sound, TriggerEvent, TriggerOccurrence};

/// How a new authoritative tree snapshot affects the current sidebar
/// selection. Keeping the removed-without-successor state explicit ensures a
/// stale widget path is cleared even when the session became empty.
enum SelectionReconciliation {
    Unchanged,
    Removed(Option<NodeId>),
}

/// Applies one `ServerEvent` to `app`. Called from the connection task's
/// read loop for every frame it decodes. Returns what this event means for
/// the automatic-work trigger occurrence represented by this event, when
/// there is one. State is always applied before the occurrence is returned.
pub fn apply(app: &mut App, event: ServerEvent) -> Option<TriggerOccurrence> {
    match event {
        ServerEvent::TreeSnapshot(tree) => {
            apply_tree_snapshot(app, tree);
            None
        }
        ServerEvent::SessionRecoveryAvailable { pane_count } => {
            app.mode = crate::app::Mode::ConfirmSessionRecovery { pane_count };
            None
        }
        ServerEvent::ScreenUpdate {
            pane_id,
            first_sequence,
            sequence,
            bytes,
        } => {
            let should_track_visible_text_change = app.tree.get(pane_id).is_some_and(|node| {
                matches!(
                    node.kind,
                    NodeKind::Pane {
                        content: PaneContentKind::Terminal,
                        status: PaneStatus::PlainShell,
                        ..
                    }
                )
            });
            let did_visible_text_change =
                if let Some(PaneRuntime::Terminal(view)) = app.panes.get_mut(&pane_id) {
                    view.apply_live_output(
                        first_sequence,
                        sequence,
                        &bytes,
                        should_track_visible_text_change,
                    )
                } else {
                    false
                };
            app.record_terminal_screen_change(pane_id, did_visible_text_change);
            None
        }
        ServerEvent::TerminalReplay {
            pane_id,
            through_sequence,
            bytes,
            is_complete,
        } => {
            if let Some(PaneRuntime::Terminal(view)) = app.panes.get_mut(&pane_id) {
                view.apply_replay(&bytes, through_sequence, is_complete);
            }
            None
        }
        ServerEvent::PaneStatusChanged { pane_id, status } => {
            // Read before `status` moves into `set_pane_status` below. The
            // server already dedups identical consecutive statuses before
            // broadcasting (see `ilium-server`'s `detection` module), but
            // this client-side check is its own independent guard against
            // re-firing `PaneBecameDone` -- and therefore a fresh title
            // inference attempt -- on a `Done` -> `Done` "change" that
            // isn't actually a transition, should that server invariant
            // ever be violated by a future code path.
            let previous_status = app.tree.get(pane_id).and_then(|node| match &node.kind {
                NodeKind::Pane { status, .. } => Some(status.clone()),
                NodeKind::Container(_) | NodeKind::Folder { .. } => None,
            });
            let became_agent = matches!(
                status,
                PaneStatus::Agent(..) | PaneStatus::AgentWithGoal(..)
            ) && !matches!(
                previous_status.as_ref(),
                Some(PaneStatus::Agent(..) | PaneStatus::AgentWithGoal(..))
            );
            // Only report `PaneBecameDone` -- and thus trigger a title
            // inference attempt -- if the status actually landed in the
            // tree. If `pane_id` doesn't resolve to a pane here (a status
            // event for a pane this client's tree doesn't know about yet),
            // reporting the transition anyway would be describing a state
            // change that never actually happened.
            match app.tree.set_pane_status(pane_id, status) {
                Ok(()) => {
                    // The common path a pane first becomes a detected agent:
                    // one incremental `PaneStatusChanged`, not a full
                    // `TreeSnapshot`. Latching and resizing here (not only in
                    // `apply_tree_snapshot`) is what makes the toolbar appear
                    // -- and the PTY's row count shrink to match -- the same
                    // tick detection actually fires, instead of waiting for
                    // some unrelated later snapshot.
                    if became_agent {
                        app.agent_toolbar_latched_panes.insert(pane_id);
                        app.resize_displayed_panes(
                            ilium_ipc::PaneResizeCause::RightPanelPresentation,
                        );
                    }
                    let is_plain_terminal = app.tree.get(pane_id).is_some_and(|node| {
                        matches!(
                            node.kind,
                            NodeKind::Pane {
                                content: PaneContentKind::Terminal,
                                status: PaneStatus::PlainShell,
                                ..
                            }
                        )
                    });
                    if is_plain_terminal {
                        if let Some(PaneRuntime::Terminal(view)) = app.panes.get_mut(&pane_id) {
                            view.synchronize_visible_text_fingerprint();
                        }
                    }
                    // Type ordering distinguishes agents from plain shells;
                    // invalidate structural hit testing when detection changes
                    // that visible rank even though no TreeSnapshot follows.
                    app.bump_tree_version();
                    let lifecycle_event = app
                        .tree
                        .get(pane_id)
                        .and_then(|node| match &node.kind {
                            NodeKind::Pane { status, .. } => {
                                ilium_sound::event_for_transition(previous_status.as_ref(), status)
                            }
                            NodeKind::Container(_) | NodeKind::Folder { .. } => None,
                        })
                        .map(event_for_sound);
                    lifecycle_event
                        .or_else(|| {
                            (became_agent && app.agent_session_ids.contains_key(&pane_id))
                                .then_some(TriggerEvent::AgentSessionReady)
                        })
                        .map(|event| TriggerOccurrence::for_pane(event, pane_id))
                }
                Err(error) => {
                    tracing::warn!("dropping PaneStatusChanged for pane {pane_id:?}: {error}");
                    None
                }
            }
        }
        ServerEvent::Error { message } => {
            tracing::error!(%message, "server reported a request error");
            app.status_message = Some(format!("Server error: {message}"));
            None
        }
        ServerEvent::PaneSessionIdResolved {
            pane_id,
            session_id,
            process_id,
            title_generation,
        } => {
            app.agent_title_generations
                .insert(pane_id, title_generation);
            match process_id {
                Some(process_id) => {
                    app.agent_process_ids.insert(pane_id, process_id);
                }
                None => {
                    app.agent_process_ids.remove(&pane_id);
                }
            }
            let previous_session_id = app.agent_session_ids.insert(pane_id, session_id.clone());
            let changed = previous_session_id.as_ref() != Some(&session_id);
            if changed {
                // A `/resume` can replace the agent session inside the same
                // terminal pane. The old title describes another transcript.
                app.inferred_title_session_ids.remove(&pane_id);
                // A prior worker cannot be cancelled safely, but it carries
                // its own session ID and will be discarded on completion.
                // Clearing this pane-level display guard lets the new
                // session start its own worker immediately.
                app.titles_loading.remove(&pane_id);
                // `title_inference_attempts` is keyed by `(pane_id,
                // session_id)`, not by `pane_id` alone, so `apply_tree_snapshot`'s
                // `live_pane_ids`-based pruning never reaches an entry for a
                // session this pane has since moved on from -- the pane
                // itself is still live. A pane that gets `/resume`d
                // repeatedly over a long-running client session would
                // otherwise accumulate one stale attempt-counter entry per
                // past session for as long as the pane stays open. Drop the
                // previous session's entry here, the one place that already
                // knows it just became unreachable.
                if let Some(previous_session_id) = previous_session_id {
                    app.title_inference_attempts
                        .remove(&(pane_id, previous_session_id));
                }
            }
            (changed
                && app.tree.get(pane_id).is_some_and(|node| {
                    matches!(
                        node.kind,
                        NodeKind::Pane {
                            status: PaneStatus::Agent(..) | PaneStatus::AgentWithGoal(..),
                            ..
                        }
                    )
                }))
            .then_some(TriggerOccurrence::for_pane(
                TriggerEvent::AgentSessionReady,
                pane_id,
            ))
        }
        ServerEvent::PaneSessionIdCleared {
            pane_id,
            title_generation,
        } => {
            app.agent_title_generations
                .insert(pane_id, title_generation);
            app.agent_process_ids.remove(&pane_id);
            if let Some(previous_session_id) = app.agent_session_ids.remove(&pane_id) {
                app.title_inference_attempts
                    .remove(&(pane_id, previous_session_id));
            }
            app.inferred_title_session_ids.remove(&pane_id);
            app.titles_loading.remove(&pane_id);
            None
        }
        ServerEvent::PaneSessionTitleCleared {
            pane_id,
            title_generation,
        } => {
            app.agent_title_generations
                .insert(pane_id, title_generation);
            if let Some(session_id) = app.agent_session_ids.get(&pane_id) {
                app.title_inference_attempts
                    .remove(&(pane_id, session_id.clone()));
            }
            app.inferred_title_session_ids.remove(&pane_id);
            app.titles_loading.remove(&pane_id);
            None
        }
        ServerEvent::PaneEditorPathResolved { pane_id, path } => {
            let Some(path) = path else {
                app.status_message = Some("Restored editor has no file path".to_string());
                return None;
            };
            app.restored_editor_paths.insert(pane_id, path);
            load_restored_editor(app, pane_id);
            None
        }
        ServerEvent::InitialStateSyncComplete => {
            Some(TriggerOccurrence::global(TriggerEvent::StartupComplete))
        }
        ServerEvent::PanePromptSubmitted { pane_id, source } => {
            let is_agent = source == PromptSubmissionSource::InitialAgentPrompt
                || app.tree.get(pane_id).is_some_and(|node| {
                    matches!(
                        node.kind,
                        NodeKind::Pane {
                            status: PaneStatus::Agent(..) | PaneStatus::AgentWithGoal(..),
                            ..
                        }
                    )
                });
            if is_agent {
                Some(TriggerOccurrence::for_pane(
                    TriggerEvent::AgentPromptSubmitted,
                    pane_id,
                ))
            } else if crate::terminal_title_inference::terminal_ready_for_retitle(app, pane_id) {
                let count = app.enter_press_counts.entry(pane_id).or_insert(0);
                *count += 1;
                if *count >= crate::terminal_title_inference::RETITLE_ENTER_INTERVAL {
                    *count = 0;
                    Some(TriggerOccurrence::for_pane(
                        TriggerEvent::TerminalActivityCheckpoint,
                        pane_id,
                    ))
                } else {
                    None
                }
            } else {
                None
            }
        }
        // The runtime event loop intercepts this event to synchronize the
        // process writer before applying the UI value. Keeping this state-only
        // branch makes direct render-cache callers and exhaustive tests obey
        // the same visible contract without creating an IPC echo loop.
        ServerEvent::DebugLoggingChanged { enabled } => {
            app.debug_settings.file_logging_enabled = enabled;
            None
        }
        ServerEvent::AgentDebugMenuChanged { enabled } => {
            app.ui_settings.agent_debug_menu_enabled = enabled;
            let is_agent_debug_flow = matches!(
                app.mode,
                crate::app::Mode::AgentDebugLog(_) | crate::app::Mode::AgentDebugSavePath(..)
            ) || app
                .modal_stack
                .iter()
                .any(|mode| matches!(mode, crate::app::Mode::AgentDebugLog(_)));
            if !enabled {
                app.remove_agent_debug_action_from_terminal_menu();
            }
            if !enabled && is_agent_debug_flow {
                app.close_modal_flow();
            }
            None
        }
        ServerEvent::PaneDebugLogSnapshot {
            pane_id,
            through_sequence,
            retained_from_sequence,
            dropped_entry_count,
            entries,
        } => {
            let cache = app.agent_debug_logs.entry(pane_id).or_default();
            for entry in entries {
                cache.log.merge_synced_entry(entry);
            }
            // `merge_synced_entry` already enforces the same
            // count/byte-budget retention the server itself applies, but a
            // reconciled snapshot also carries the server's authoritative
            // `retained_from_sequence`, so apply it directly rather than
            // trusting only the client's own approximate byte accounting.
            // This goes through `retain_from_sequence` (not a direct filter
            // on `entries`) so `retained_approximate_bytes` stays correct --
            // otherwise a later live append's retention pass could try to
            // evict from an already-empty vector and panic.
            cache.log.retain_from_sequence(retained_from_sequence);
            cache.through_sequence = cache.through_sequence.max(through_sequence);
            cache.log.dropped_entry_count = cache.log.dropped_entry_count.max(dropped_entry_count);
            cache.has_loaded_retained_history = true;
            cache.is_loading = false;
            None
        }
        ServerEvent::PaneDebugEntryAppended { pane_id, entry } => {
            let cache = app.agent_debug_logs.entry(pane_id).or_default();
            cache.through_sequence = cache.through_sequence.max(entry.sequence);
            // `PaneDebugLog::merge_synced_entry` enforces the same
            // `MAXIMUM_AGENT_DEBUG_ENTRIES`/`_BYTES` retention on this
            // live-append path that the snapshot branch above enforces on
            // replay, so a pane whose debug log the operator never opens
            // still cannot accumulate broadcasts unbounded for the life of
            // the pane.
            cache.log.merge_synced_entry(entry);
            None
        }
        ServerEvent::NodeActivityChanged {
            node_id,
            activity_revision,
        } => {
            app.apply_node_activity(node_id, activity_revision);
            None
        }
        ServerEvent::NodeFocusCheckpointChanged {
            node_id,
            activity_revision,
        } => {
            app.apply_node_focus_checkpoint(node_id, activity_revision);
            None
        }
        ServerEvent::ProjectRestructureApplied {
            project_id,
            checkpoint_activity_revisions,
        } => {
            app.confirm_project_restructure_applied(project_id, &checkpoint_activity_revisions);
            None
        }
        ServerEvent::ProjectRestructureRejected {
            project_id,
            message,
        } => {
            app.reject_project_restructure(project_id, message);
            None
        }
    }
}

/// Replaces the render-cache tree wholesale and reconciles `app.panes`
/// against it: a pane id present in the new tree but missing from
/// `app.panes` gets a fresh local runtime created (a blank `TerminalView`
/// for a PTY-backed pane, at `last_known_pane_size`), and a pane id no
/// longer present in the tree has its local runtime dropped.
///
/// Editor pane runtimes are the one exception: an editor's `EditorPane`
/// holds live, client-local buffer state (unsaved edits, cursor position,
/// undo history) the server's tree snapshot has no way to reconstruct --
/// it only knows the node exists and its display name (the tree carries
/// no file path at all). A new editor node this client itself just asked
/// the server to create is loaded from disk here via
/// `App::take_matching_pending_editor_open` (matched by basename -- see
/// that field's doc comment) and focused, the same "open and jump to the
/// new pane" behavior the pre-client/server design had. An editor node
/// this client did *not* request (another attached client created it, or
/// it existed before this client attached) has no path to load from and
/// renders as an empty placeholder until this client opens it itself --
/// a known limitation of a multi-client session, not papered over with a
/// guess.
fn apply_tree_snapshot(app: &mut App, tree: ilium_core::Tree) {
    let selection_reconciliation = selection_reconciliation(app, &tree);
    app.track_tree_snapshot_change(&tree);
    app.tree = tree;
    app.restore_expanded_groups();
    app.reconcile_selected_tree_path();
    app.bump_tree_version();
    app.prune_recently_created();

    let live_pane_ids: std::collections::HashSet<_> =
        app.tree.panes().map(|node| node.id).collect();
    app.panes
        .retain(|pane_id, _| live_pane_ids.contains(pane_id));
    app.retain_requested_pane_sizes(&live_pane_ids);
    // Latches every pane currently showing a detected agent so its toolbar
    // row, once reserved, survives a later detection revert back to
    // `PlainShell` -- see `agent_toolbar_latched_panes`'s doc comment for
    // why that latch (rather than reserving purely off live status) is what
    // keeps a real agent PTY from getting resized by detection noise.
    for node in app.tree.panes() {
        if matches!(
            &node.kind,
            ilium_core::NodeKind::Pane {
                status: ilium_core::PaneStatus::Agent(_, _)
                    | ilium_core::PaneStatus::AgentWithGoal(_, _),
                ..
            }
        ) {
            app.agent_toolbar_latched_panes.insert(node.id);
        }
    }
    app.agent_toolbar_latched_panes
        .retain(|pane_id| live_pane_ids.contains(pane_id));
    app.agent_toolbar_effort
        .retain(|pane_id, _| live_pane_ids.contains(pane_id));
    // `NodeId` is never reused (see `ilium_core::Tree`), so every one of
    // these pane-keyed caches would otherwise grow by one entry per pane
    // ever created for the life of the client process -- a slow but
    // genuine leak across long-running sessions with heavy pane churn.
    // Pruned against the same `live_pane_ids` set as `app.panes` above so a
    // closed pane's cached title-inference/session-id state is dropped in
    // the same place its runtime is.
    app.agent_session_ids
        .retain(|pane_id, _| live_pane_ids.contains(pane_id));
    app.agent_process_ids
        .retain(|pane_id, _| live_pane_ids.contains(pane_id));
    app.restored_editor_paths
        .retain(|pane_id, _| live_pane_ids.contains(pane_id));
    app.title_inference_attempts
        .retain(|(pane_id, _), _| live_pane_ids.contains(pane_id));
    app.inferred_title_session_ids
        .retain(|pane_id, _| live_pane_ids.contains(pane_id));
    app.agent_title_generations
        .retain(|pane_id, _| live_pane_ids.contains(pane_id));
    app.enter_press_counts
        .retain(|pane_id, _| live_pane_ids.contains(pane_id));
    app.terminal_retitle_content_hashes
        .retain(|pane_id, _| live_pane_ids.contains(pane_id));
    app.titles_loading
        .retain(|pane_id| live_pane_ids.contains(pane_id));
    app.agent_debug_logs
        .retain(|pane_id, _| live_pane_ids.contains(pane_id));
    // Same idea as the pane-keyed caches above, but keyed by project Group
    // id: a project removed from the tree must not leave its restructure
    // job/retry state (and, via `restructure_status_text`, a permanent
    // footer entry) behind for the life of the client process.
    let live_project_ids: std::collections::HashSet<_> =
        app.tree.project_ids().into_iter().collect();
    let restructure_jobs_before = app.project_restructure_jobs.len();
    app.project_restructure_jobs
        .retain(|project_id, _| live_project_ids.contains(project_id));
    app.automatic_restructure_retries
        .retain(|project_id, _| live_project_ids.contains(project_id));
    if app.project_restructure_jobs.len() != restructure_jobs_before {
        // A pruned job may have been the last Queued/Running entry keeping
        // the loading spinner lit, so recompute it from the pruned map
        // rather than leaving a stale "still loading" state behind.
        app.refresh_structure_loading();
    }
    let debug_target_closed = match &app.mode {
        crate::app::Mode::TerminalPaneContextMenu(menu) => !live_pane_ids.contains(&menu.pane_id),
        crate::app::Mode::AgentDebugLog(view) => !live_pane_ids.contains(&view.pane_id),
        crate::app::Mode::AgentDebugSavePath(pane_id, _) => !live_pane_ids.contains(pane_id),
        _ => false,
    } || app.modal_stack.iter().any(|mode| match mode {
        crate::app::Mode::TerminalPaneContextMenu(menu) => !live_pane_ids.contains(&menu.pane_id),
        crate::app::Mode::AgentDebugLog(view) => !live_pane_ids.contains(&view.pane_id),
        crate::app::Mode::AgentDebugSavePath(pane_id, _) => !live_pane_ids.contains(pane_id),
        _ => false,
    });
    if debug_target_closed {
        app.close_modal_flow();
    }
    if app
        .hovered_tree_node
        .is_some_and(|hit| app.tree.get(hit.id).is_none())
    {
        app.hovered_tree_node = None;
    }
    if app
        .hovered_agent_toolbar_action
        .is_some_and(|(pane_id, _)| !live_pane_ids.contains(&pane_id))
    {
        app.hovered_agent_toolbar_action = None;
    }

    let (rows, cols) = app.last_known_pane_size;
    // Editors and boards already need a local runtime before they can be
    // presented. Terminal/agent panes now use the same focus path once this
    // client confirms it requested that exact new node.
    let mut newly_opened_pane: Option<NodeId> = None;
    let new_pane_nodes: Vec<_> = app
        .tree
        .panes()
        .filter(|node| !app.panes.contains_key(&node.id))
        .map(|node| (node.id, node.name.clone(), node.kind.clone()))
        .collect();
    for (pane_id, name, kind) in new_pane_nodes {
        let NodeKind::Pane { content, .. } = kind else {
            continue;
        };
        match content {
            PaneContentKind::Terminal => {
                app.panes.insert(
                    pane_id,
                    PaneRuntime::Terminal(Box::new(TerminalView::with_scrollback_budget_mib(
                        rows,
                        cols,
                        app.terminal_settings.scrollback_budget_mib,
                    ))),
                );
                if app.take_matching_pending_pane_focus(pane_id, PaneContentKind::Terminal, &name) {
                    newly_opened_pane = Some(pane_id);
                }
            }
            PaneContentKind::Editor => {
                let pending_open = app
                    .restored_editor_paths
                    .get(&pane_id)
                    .cloned()
                    .map(|path| (path, None, None))
                    .or_else(|| {
                        app.take_matching_pending_editor_open(&name)
                            .map(|pending| (pending.path, pending.line, pending.column))
                    });
                let Some((path, line, column)) = pending_open else {
                    continue;
                };
                match EditorPane::load(path) {
                    Ok(mut editor) => {
                        editor.apply_defaults(&app.editor_settings);
                        if let Some(line) = line {
                            editor.jump_to_location(
                                line.saturating_sub(1) as usize,
                                column.unwrap_or(1_u32).saturating_sub(1) as usize,
                            );
                        }
                        app.panes
                            .insert(pane_id, PaneRuntime::Editor(Box::new(editor)));
                        newly_opened_pane = Some(pane_id);
                    }
                    Err(error) => {
                        app.status_message = Some(format!("Failed to open file: {error}"));
                    }
                }
            }
            PaneContentKind::Board => {
                let Some(NodeKind::Pane {
                    board_storage: Some(storage),
                    ..
                }) = app.tree.get(pane_id).map(|node| &node.kind)
                else {
                    continue;
                };
                let board_result = if storage.path().exists() {
                    BoardPane::load(storage.clone())
                } else {
                    BoardPane::create(storage.clone())
                };
                match board_result {
                    Ok(board) => {
                        app.panes
                            .insert(pane_id, PaneRuntime::Board(Box::new(board)));
                        newly_opened_pane = Some(pane_id);
                    }
                    Err(error) => {
                        app.status_message = Some(format!("Failed to open board: {error}"))
                    }
                }
            }
        }
    }
    app.reconcile_right_panel_target();
    if let Some(pane_id) = newly_opened_pane {
        app.focus_pane(pane_id);
    } else if let SelectionReconciliation::Removed(replacement) = selection_reconciliation {
        if let Some(node_id) = replacement {
            app.activate_tree_successor(node_id);
        } else {
            app.tree_state.select(Vec::new());
            app.leave_pane_focus();
        }
    }
    app.resize_displayed_panes(ilium_ipc::PaneResizeCause::RightPanelPresentation);
}

/// Chooses the next surviving visible row below a removed selection, falling
/// back to the nearest surviving row above only when the removed row was last.
/// The walk uses the old snapshot because it preserves the closed row's rank.
fn selection_reconciliation(app: &App, new_tree: &ilium_core::Tree) -> SelectionReconciliation {
    let Some(selected_node_id) = app.selected_node_id() else {
        return SelectionReconciliation::Unchanged;
    };
    if new_tree.get(selected_node_id).is_some() {
        return SelectionReconciliation::Unchanged;
    }

    let visible_node_ids = crate::tree_ui::visible_tree_node_ids(
        &app.tree,
        &app.tree_state,
        app.ui_settings.tree_order,
    );
    let Some(selected_index) = visible_node_ids
        .iter()
        .position(|node_id| *node_id == selected_node_id)
    else {
        return SelectionReconciliation::Removed(None);
    };

    let next_surviving_node = visible_node_ids[selected_index + 1..]
        .iter()
        .copied()
        .find(|node_id| new_tree.get(*node_id).is_some());
    let previous_surviving_node = visible_node_ids[..selected_index]
        .iter()
        .rev()
        .copied()
        .find(|node_id| new_tree.get(*node_id).is_some());

    SelectionReconciliation::Removed(next_surviving_node.or(previous_surviving_node))
}

/// Hydrates one restored editor after its server-owned path arrives. Attach
/// sends the tree first and this event second, but keeping this separate
/// also makes the ordering safe if a later transport change interleaves
/// events differently.
fn load_restored_editor(app: &mut App, pane_id: NodeId) {
    if app.panes.contains_key(&pane_id)
        || !matches!(
            app.tree.get(pane_id).map(|node| &node.kind),
            Some(NodeKind::Pane {
                content: PaneContentKind::Editor,
                ..
            })
        )
    {
        return;
    }
    let Some(path) = app.restored_editor_paths.get(&pane_id).cloned() else {
        return;
    };
    match EditorPane::load(path) {
        Ok(mut editor) => {
            // `EditorPane::load` only sets hard-coded defaults; without this
            // call a restored editor's line numbers/minimap/autosave/markdown
            // rendering would silently depend on whether `TreeSnapshot` or
            // `PaneEditorPathResolved` arrived first, since the snapshot-arm
            // sibling in `apply_tree_snapshot` always applies it.
            editor.apply_defaults(&app.editor_settings);
            app.panes
                .insert(pane_id, PaneRuntime::Editor(Box::new(editor)));
        }
        Err(error) => {
            app.status_message = Some(format!("Failed to open restored editor: {error}"));
        }
    }
}

#[cfg(test)]
mod tests {
    use ilium_core::{
        AgentActivity, AgentClass, PaneContentKind, RestructureNode, RestructurePlan,
        SplitOrientation, ROOT_ID,
    };
    use ilium_ipc::{
        AgentDebugContext, AgentDebugEntry, AgentDebugEventKind, AgentDebugSeverity,
        AgentDebugSource, ClientRequest,
    };
    use ratatui::layout::{Position, Rect};

    use super::*;
    use crate::terminal_activity::TerminalActivityPhase;

    fn app() -> App {
        App::new("test".to_string(), std::env::temp_dir())
    }

    fn debug_entry(sequence: u64, summary: &str) -> AgentDebugEntry {
        AgentDebugEntry {
            sequence,
            occurred_at_unix_millis: 1_700_000_000_000 + sequence as i64,
            severity: AgentDebugSeverity::Information,
            source: AgentDebugSource::Detector,
            kind: AgentDebugEventKind::DetectionCycle,
            summary: summary.to_string(),
            fields: Vec::new(),
            correlation_id: None,
            context: AgentDebugContext::default(),
            metadata: Default::default(),
        }
    }

    #[test]
    fn tree_snapshot_creates_a_terminal_view_for_a_new_pane() {
        let mut app = app();
        let mut tree = ilium_core::Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        let pane_id = tree
            .add_pane(group, "shell", PaneContentKind::Terminal)
            .unwrap();

        apply(&mut app, ServerEvent::TreeSnapshot(tree));
        assert!(matches!(
            app.panes.get(&pane_id),
            Some(PaneRuntime::Terminal(_))
        ));
    }

    #[test]
    fn locally_created_terminal_opens_in_the_right_panel_after_confirmation() {
        let mut app = app();
        let mut tree = ilium_core::Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        apply(&mut app, ServerEvent::TreeSnapshot(tree.clone()));

        app.request_new_terminal(group);
        app.take_outbound_requests();
        let pane_id = tree
            .add_pane(group, "shell", PaneContentKind::Terminal)
            .unwrap();
        apply(&mut app, ServerEvent::TreeSnapshot(tree));

        assert_eq!(app.active_pane_id(), Some(pane_id));
        assert_eq!(app.focus, crate::app::FocusTarget::Pane);
        assert_eq!(
            app.right_panel_target,
            crate::app::RightPanelTarget::Pane { pane_id }
        );
    }

    #[test]
    fn locally_created_agent_opens_in_the_right_panel_after_confirmation() {
        let mut app = app();
        let mut tree = ilium_core::Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        apply(&mut app, ServerEvent::TreeSnapshot(tree.clone()));

        app.request_new_command_pane(group, "codex".to_string());
        app.take_outbound_requests();
        let pane_id = tree
            .add_pane(group, "codex", PaneContentKind::Terminal)
            .unwrap();
        apply(&mut app, ServerEvent::TreeSnapshot(tree));

        assert_eq!(app.active_pane_id(), Some(pane_id));
        assert_eq!(app.focus, crate::app::FocusTarget::Pane);
    }

    #[test]
    fn unrequested_terminal_snapshot_does_not_steal_right_panel_focus() {
        let mut app = app();
        let mut tree = ilium_core::Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        let pane_id = tree
            .add_pane(group, "shell", PaneContentKind::Terminal)
            .unwrap();

        apply(&mut app, ServerEvent::TreeSnapshot(tree));

        assert_eq!(app.active_pane_id(), None);
        assert_eq!(app.right_panel_target, crate::app::RightPanelTarget::Empty);
        assert!(app.panes.contains_key(&pane_id));
    }

    #[test]
    fn removing_a_pane_forgets_its_server_resize_deduplication_state() {
        let mut app = app();
        app.set_screen_area(Rect::new(0, 0, 140, 40));
        let mut tree = ilium_core::Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        let pane_id = tree
            .add_pane(group, "agent", PaneContentKind::Terminal)
            .unwrap();

        apply(&mut app, ServerEvent::TreeSnapshot(tree.clone()));
        app.focus_pane(pane_id);
        assert!(app.take_outbound_requests().into_iter().any(|request| {
            matches!(
                request,
                ClientRequest::ResizePane {
                    pane_id: resized_pane_id,
                    ..
                } if resized_pane_id == pane_id
            )
        }));

        apply(&mut app, ServerEvent::TreeSnapshot(ilium_core::Tree::new()));
        app.take_outbound_requests();
        apply(&mut app, ServerEvent::TreeSnapshot(tree));
        app.focus_pane(pane_id);

        assert!(app.take_outbound_requests().into_iter().any(|request| {
            matches!(
                request,
                ClientRequest::ResizePane {
                    pane_id: resized_pane_id,
                    ..
                } if resized_pane_id == pane_id
            )
        }));
    }

    #[test]
    fn focused_split_member_survives_a_restructure_snapshot() {
        let mut app = app();
        let mut tree = ilium_core::Tree::new();
        let project = tree.add_project(std::env::temp_dir()).unwrap();
        let original_group = tree.add_group(project, "original").unwrap();
        let first = tree
            .add_pane(original_group, "first", PaneContentKind::Terminal)
            .unwrap();
        let second = tree
            .add_pane(original_group, "second", PaneContentKind::Terminal)
            .unwrap();
        let outside = tree
            .add_pane(original_group, "outside", PaneContentKind::Terminal)
            .unwrap();
        let split_view = tree
            .create_split_view(
                original_group,
                "User Split",
                SplitOrientation::Vertical,
                &[first, second],
            )
            .unwrap();
        apply(&mut app, ServerEvent::TreeSnapshot(tree.clone()));
        app.focus_pane(second);

        tree.apply_project_restructure(
            project,
            RestructurePlan {
                children: vec![RestructureNode::Group {
                    title: "AI regrouped".to_string(),
                    short_title: None,
                    icon: None,
                    children: vec![
                        RestructureNode::ExistingSplitView {
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
                        },
                        RestructureNode::Pane {
                            id: outside,
                            title: "renamed outside".to_string(),
                            short_title: None,
                            icon: None,
                        },
                    ],
                }],
            },
        )
        .unwrap();
        let regrouped = tree.children_of(project).unwrap()[0];

        apply(&mut app, ServerEvent::TreeSnapshot(tree));

        assert_eq!(
            app.right_panel_target,
            crate::app::RightPanelTarget::SplitView {
                split_id: split_view,
                active_pane_id: Some(second),
            }
        );
        assert_eq!(app.displayed_pane_ids(), vec![first, second]);
        assert_eq!(app.selected_node_id(), Some(second));
        assert_eq!(
            app.tree_state.selected(),
            &[project, regrouped, split_view, second]
        );
        assert_eq!(app.focus, crate::app::FocusTarget::Pane);
    }

    #[test]
    fn tree_focused_split_row_survives_a_restructure_snapshot() {
        let mut app = app();
        let mut tree = ilium_core::Tree::new();
        let project = tree.add_project(std::env::temp_dir()).unwrap();
        let original_group = tree.add_group(project, "original").unwrap();
        let first = tree
            .add_pane(original_group, "first", PaneContentKind::Terminal)
            .unwrap();
        let second = tree
            .add_pane(original_group, "second", PaneContentKind::Terminal)
            .unwrap();
        let split_view = tree
            .create_split_view(
                original_group,
                "User Split",
                SplitOrientation::Horizontal,
                &[first, second],
            )
            .unwrap();
        apply(&mut app, ServerEvent::TreeSnapshot(tree.clone()));
        app.select_node(split_view);
        app.focus = crate::app::FocusTarget::Tree;
        app.right_panel_target = crate::app::RightPanelTarget::SplitView {
            split_id: split_view,
            active_pane_id: None,
        };

        tree.apply_project_restructure(
            project,
            RestructurePlan {
                children: vec![RestructureNode::Group {
                    title: "AI regrouped".to_string(),
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
        )
        .unwrap();
        let regrouped = tree.children_of(project).unwrap()[0];

        apply(&mut app, ServerEvent::TreeSnapshot(tree));

        assert_eq!(app.selected_node_id(), Some(split_view));
        assert_eq!(app.tree_state.selected(), &[project, regrouped, split_view]);
        assert_eq!(
            app.right_panel_target,
            crate::app::RightPanelTarget::SplitView {
                split_id: split_view,
                active_pane_id: None,
            }
        );
        assert_eq!(app.displayed_pane_ids(), vec![first, second]);
        assert_eq!(app.focus, crate::app::FocusTarget::Tree);
    }

    #[test]
    fn debug_snapshot_and_live_events_merge_in_sequence_order_without_duplicates() {
        let mut app = app();
        let pane_id = ilium_core::NodeId(7);

        apply(
            &mut app,
            ServerEvent::PaneDebugLogSnapshot {
                pane_id,
                through_sequence: 3,
                retained_from_sequence: 1,
                dropped_entry_count: 0,
                entries: vec![debug_entry(1, "first"), debug_entry(3, "third")],
            },
        );
        apply(
            &mut app,
            ServerEvent::PaneDebugEntryAppended {
                pane_id,
                entry: debug_entry(2, "second"),
            },
        );
        apply(
            &mut app,
            ServerEvent::PaneDebugEntryAppended {
                pane_id,
                entry: debug_entry(3, "third replay copy"),
            },
        );

        let cache = app.agent_debug_logs.get(&pane_id).unwrap();
        assert_eq!(
            cache
                .log
                .entries
                .iter()
                .map(|entry| entry.sequence)
                .collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        assert_eq!(cache.log.entries[2].summary, "third replay copy");
        assert_eq!(cache.through_sequence, 3);
        assert!(!cache.is_loading);

        apply(
            &mut app,
            ServerEvent::PaneDebugLogSnapshot {
                pane_id,
                through_sequence: 3,
                retained_from_sequence: 3,
                dropped_entry_count: 2,
                entries: Vec::new(),
            },
        );
        let cache = app.agent_debug_logs.get(&pane_id).unwrap();
        assert_eq!(
            cache
                .log
                .entries
                .iter()
                .map(|entry| entry.sequence)
                .collect::<Vec<_>>(),
            vec![3]
        );
        assert_eq!(cache.log.dropped_entry_count, 2);
    }

    /// Regression test: the server broadcasts `PaneDebugEntryAppended` to
    /// every connected client for every recorded entry whenever the
    /// agent-debug journal is enabled, whether or not that client ever
    /// opened the debug view for that pane (which is the only path that
    /// sends a `PaneDebugLogSnapshot`). Driving `apply()` with nothing but
    /// live appends -- no snapshot ever received -- past
    /// `MAXIMUM_AGENT_DEBUG_ENTRIES` proves the live-append arm alone keeps
    /// this cache bounded, not just the on-demand replay path.
    #[test]
    fn live_appends_alone_stay_within_the_retention_cap_without_a_snapshot() {
        let mut app = app();
        let pane_id = ilium_core::NodeId(7);

        for sequence in 1..=(ilium_agent_debug::MAXIMUM_AGENT_DEBUG_ENTRIES as u64 + 25) {
            apply(
                &mut app,
                ServerEvent::PaneDebugEntryAppended {
                    pane_id,
                    entry: debug_entry(sequence, &format!("event {sequence}")),
                },
            );
        }

        let cache = app.agent_debug_logs.get(&pane_id).unwrap();
        assert_eq!(
            cache.log.entries.len(),
            ilium_agent_debug::MAXIMUM_AGENT_DEBUG_ENTRIES
        );
        assert_eq!(cache.log.dropped_entry_count, 25);
        assert!(cache.log.retained_from_sequence() > 1);
    }

    #[test]
    fn closing_a_pane_closes_its_nested_debug_save_flow_and_cache() {
        let mut app = app();
        let mut tree = ilium_core::Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        let pane_id = tree
            .add_pane(group, "agent", PaneContentKind::Terminal)
            .unwrap();
        apply(&mut app, ServerEvent::TreeSnapshot(tree));
        app.agent_debug_logs.insert(pane_id, Default::default());
        app.modal_stack.push(crate::app::Mode::AgentDebugLog(
            crate::app::AgentDebugLogViewState {
                pane_id,
                scroll_position: crate::app::AgentDebugLogScrollPosition::FromNewest(0),
            },
        ));
        app.mode = crate::app::Mode::AgentDebugSavePath(
            pane_id,
            crate::text_prompt::TextPromptState::new("/tmp/agent.log"),
        );

        apply(&mut app, ServerEvent::TreeSnapshot(ilium_core::Tree::new()));

        assert!(matches!(app.mode, crate::app::Mode::Normal));
        assert!(app.modal_stack.is_empty());
        assert!(!app.agent_debug_logs.contains_key(&pane_id));
    }

    #[test]
    fn type_order_hit_testing_rebuilds_when_a_shell_becomes_an_agent() {
        let mut app = app();
        app.set_screen_area(Rect::new(0, 0, 100, 30));
        app.ui_settings.tree_order = crate::config::TreeOrder::Type;
        let mut tree = ilium_core::Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        let shell = tree
            .add_pane(group, "a-shell", PaneContentKind::Terminal)
            .unwrap();
        let agent = tree
            .add_pane(group, "z-agent", PaneContentKind::Terminal)
            .unwrap();
        apply(&mut app, ServerEvent::TreeSnapshot(tree));
        app.tree_state.open(vec![group]);
        let list = crate::tree_ui::list_area(app.layout.tree_area);

        assert_eq!(
            app.tree_node_at(Position::new(list.x, list.y + 1))
                .map(|hit| hit.id),
            Some(shell)
        );

        apply(
            &mut app,
            ServerEvent::PaneStatusChanged {
                pane_id: agent,
                status: PaneStatus::Agent(AgentClass::Codex, AgentActivity::Idle),
            },
        );

        assert_eq!(
            app.tree_node_at(Position::new(list.x, list.y + 1))
                .map(|hit| hit.id),
            Some(agent)
        );
    }

    #[test]
    fn pane_status_changed_alone_latches_and_reserves_the_agent_toolbar_row() {
        // Regression test: the toolbar must not depend on a full
        // `TreeSnapshot` following detection -- the server's actual
        // detection path broadcasts an incremental `PaneStatusChanged`
        // (`ilium-server::detection`), never a snapshot, so latching only
        // inside `apply_tree_snapshot` left the toolbar permanently dark for
        // every pane whose agent was detected after creation (the common
        // case: a user types `codex` into an already-open shell pane).
        let mut app = app();
        let mut tree = ilium_core::Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        let pane_id = tree
            .add_pane(group, "shell", PaneContentKind::Terminal)
            .unwrap();
        apply(&mut app, ServerEvent::TreeSnapshot(tree));
        app.panes.insert(
            pane_id,
            PaneRuntime::Terminal(Box::new(TerminalView::new(24, 80))),
        );
        app.right_panel_target = crate::app::RightPanelTarget::Pane { pane_id };
        app.set_screen_area(Rect::new(0, 0, 120, 40));
        let before = app.pane_viewport(pane_id).unwrap();
        assert_eq!(before.toolbar_area, None);
        app.take_outbound_requests(); // drop the initial-size ResizePane

        apply(
            &mut app,
            ServerEvent::PaneStatusChanged {
                pane_id,
                status: PaneStatus::Agent(AgentClass::Codex, AgentActivity::Working),
            },
        );

        assert!(app.agent_toolbar_latched_panes.contains(&pane_id));
        assert!(app.shows_agent_toolbar(pane_id));
        let after = app.pane_viewport(pane_id).unwrap();
        assert!(after.toolbar_area.is_some());
        assert_eq!(after.content_area.height, before.content_area.height - 1);
        // The PTY must be told the *reduced* size -- not just the render
        // area -- or the agent's own bottom row (its input/prompt line)
        // renders past what the real terminal was told it has.
        let resize = app
            .take_outbound_requests()
            .into_iter()
            .find_map(|request| match request {
                ilium_ipc::ClientRequest::ResizePane {
                    pane_id: resized_pane,
                    rows,
                    cols,
                    ..
                } if resized_pane == pane_id => Some((rows, cols)),
                _ => None,
            });
        assert_eq!(
            resize,
            Some((after.content_area.height, after.content_area.width))
        );
    }

    #[test]
    fn tree_snapshot_drops_runtimes_for_removed_panes() {
        let mut app = app();
        let mut tree = ilium_core::Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        let pane_id = tree
            .add_pane(group, "shell", PaneContentKind::Terminal)
            .unwrap();
        apply(&mut app, ServerEvent::TreeSnapshot(tree.clone()));
        assert!(app.panes.contains_key(&pane_id));

        tree.remove_node(pane_id).unwrap();
        apply(&mut app, ServerEvent::TreeSnapshot(tree));

        assert!(!app.panes.contains_key(&pane_id));
    }

    #[test]
    fn removing_the_selected_pane_focuses_the_next_visible_pane_below_it() {
        let mut app = app();
        let mut tree = ilium_core::Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        tree.add_pane(group, "first", PaneContentKind::Terminal)
            .unwrap();
        let selected = tree
            .add_pane(group, "selected", PaneContentKind::Terminal)
            .unwrap();
        let next = tree
            .add_pane(group, "next", PaneContentKind::Terminal)
            .unwrap();
        apply(&mut app, ServerEvent::TreeSnapshot(tree.clone()));
        app.tree_state.open(vec![group]);
        app.focus_pane(selected);

        tree.remove_node(selected).unwrap();
        apply(&mut app, ServerEvent::TreeSnapshot(tree));

        assert_eq!(app.selected_node_id(), Some(next));
        assert_eq!(app.active_pane_id(), Some(next));
        assert_eq!(app.focus, crate::app::FocusTarget::Pane);
    }

    #[test]
    fn selected_pane_path_is_rebuilt_after_the_pane_moves_to_another_group() {
        let mut app = app();
        let mut tree = ilium_core::Tree::new();
        let source_group = tree.add_group(ROOT_ID, "source").unwrap();
        let destination_group = tree.add_group(ROOT_ID, "destination").unwrap();
        let pane_id = tree
            .add_pane(source_group, "selected", PaneContentKind::Terminal)
            .unwrap();
        apply(&mut app, ServerEvent::TreeSnapshot(tree.clone()));
        app.focus_pane(pane_id);
        assert_eq!(app.tree_state.selected(), &[source_group, pane_id]);

        tree.move_node(pane_id, destination_group, None).unwrap();
        apply(&mut app, ServerEvent::TreeSnapshot(tree));

        assert_eq!(app.selected_node_id(), Some(pane_id));
        assert_eq!(app.tree_state.selected(), &[destination_group, pane_id]);
        assert!(app.tree_state.opened().contains(&vec![destination_group]));
        assert_eq!(app.active_pane_id(), Some(pane_id));
    }

    #[test]
    fn removing_a_container_skips_its_removed_descendants_for_the_next_row() {
        let mut app = app();
        let mut tree = ilium_core::Tree::new();
        let removed_group = tree.add_group(ROOT_ID, "remove me").unwrap();
        tree.add_pane(removed_group, "child", PaneContentKind::Terminal)
            .unwrap();
        let next_group = tree.add_group(ROOT_ID, "next group").unwrap();
        apply(&mut app, ServerEvent::TreeSnapshot(tree.clone()));
        app.tree_state.open(vec![removed_group]);
        app.select_node(removed_group);
        app.focus = crate::app::FocusTarget::Tree;

        tree.remove_node(removed_group).unwrap();
        apply(&mut app, ServerEvent::TreeSnapshot(tree));

        assert_eq!(app.selected_node_id(), Some(next_group));
        assert_eq!(app.active_pane_id(), None);
        assert_eq!(app.focus, crate::app::FocusTarget::Tree);
    }

    #[test]
    fn removed_selection_successor_follows_the_configured_sidebar_order() {
        let mut app = app();
        app.ui_settings.tree_order = crate::config::TreeOrder::NameAscending;
        let mut tree = ilium_core::Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        let beta = tree
            .add_pane(group, "beta", PaneContentKind::Terminal)
            .unwrap();
        tree.add_pane(group, "zulu", PaneContentKind::Terminal)
            .unwrap();
        let alpha = tree
            .add_pane(group, "alpha", PaneContentKind::Terminal)
            .unwrap();
        apply(&mut app, ServerEvent::TreeSnapshot(tree.clone()));
        app.tree_state.open(vec![group]);
        app.focus_pane(alpha);

        tree.remove_node(alpha).unwrap();
        apply(&mut app, ServerEvent::TreeSnapshot(tree));

        assert_eq!(app.selected_node_id(), Some(beta));
        assert_eq!(app.active_pane_id(), Some(beta));
    }

    #[test]
    fn removing_the_last_visible_row_falls_back_to_the_previous_row() {
        let mut app = app();
        let mut tree = ilium_core::Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        let previous = tree
            .add_pane(group, "previous", PaneContentKind::Terminal)
            .unwrap();
        let selected = tree
            .add_pane(group, "selected", PaneContentKind::Terminal)
            .unwrap();
        apply(&mut app, ServerEvent::TreeSnapshot(tree.clone()));
        app.tree_state.open(vec![group]);
        app.focus_pane(selected);

        tree.remove_node(selected).unwrap();
        apply(&mut app, ServerEvent::TreeSnapshot(tree));

        assert_eq!(app.selected_node_id(), Some(previous));
        assert_eq!(app.active_pane_id(), Some(previous));
    }

    #[test]
    fn removing_an_active_split_member_activates_the_next_split_member() {
        let mut app = app();
        let mut tree = ilium_core::Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        let selected = tree
            .add_pane(group, "selected", PaneContentKind::Terminal)
            .unwrap();
        let next = tree
            .add_pane(group, "next", PaneContentKind::Terminal)
            .unwrap();
        let split = tree
            .create_split_view(
                group,
                "split",
                ilium_core::SplitOrientation::Vertical,
                &[selected, next],
            )
            .unwrap();
        apply(&mut app, ServerEvent::TreeSnapshot(tree.clone()));
        app.tree_state.open(vec![group]);
        app.tree_state.open(vec![group, split]);
        app.focus_pane(selected);

        tree.remove_node(selected).unwrap();
        apply(&mut app, ServerEvent::TreeSnapshot(tree));

        assert_eq!(app.selected_node_id(), Some(next));
        assert_eq!(app.active_pane_id(), Some(next));
        assert_eq!(app.displayed_pane_ids(), vec![next]);
    }

    #[test]
    fn removing_the_only_visible_subtree_clears_selection_and_the_right_panel() {
        let mut app = app();
        let mut tree = ilium_core::Tree::new();
        let group = tree.add_group(ROOT_ID, "only group").unwrap();
        let pane = tree
            .add_pane(group, "only pane", PaneContentKind::Terminal)
            .unwrap();
        apply(&mut app, ServerEvent::TreeSnapshot(tree.clone()));
        app.tree_state.open(vec![group]);
        app.focus_pane(pane);
        app.select_node(group);

        tree.remove_node(group).unwrap();
        apply(&mut app, ServerEvent::TreeSnapshot(tree));

        assert_eq!(app.selected_node_id(), None);
        assert_eq!(app.active_pane_id(), None);
        assert_eq!(app.focus, crate::app::FocusTarget::Tree);
    }

    #[test]
    fn new_board_snapshot_loads_an_existing_markdown_file_without_overwriting_it() {
        let path = std::env::temp_dir().join(format!(
            "ilium-existing-board-{}-{}.md",
            std::process::id(),
            crate::scheduled_input::unix_millis_now()
        ));
        let original = "# General\n\n* [ ] Existing task\n";
        std::fs::write(&path, original).unwrap();

        let mut app = app();
        let mut tree = ilium_core::Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        let board_id = tree
            .add_board(
                group,
                "General".to_string(),
                ilium_core::BoardStorage::MarkdownFile { path: path.clone() },
            )
            .unwrap();

        apply(&mut app, ServerEvent::TreeSnapshot(tree));

        let Some(PaneRuntime::Board(board)) = app.panes.get(&board_id) else {
            panic!("existing Markdown storage should hydrate a board runtime");
        };
        assert_eq!(board.columns[0].title, "General");
        assert_eq!(board.columns[0].cards[0].title, "[ ] Existing task");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        let _ = std::fs::remove_file(path);
    }

    /// Regression test: `NodeId` is never reused (see `ilium_core::Tree`),
    /// so every pane-keyed cache here that isn't pruned alongside
    /// `app.panes` accumulates one stale entry per pane ever created for
    /// the life of the client process -- a real, if slow, memory leak
    /// across long-running sessions with heavy pane churn.
    #[test]
    fn tree_snapshot_prunes_pane_keyed_caches_for_removed_panes() {
        let mut app = app();
        let mut tree = ilium_core::Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        let pane_id = tree
            .add_pane(group, "claude", PaneContentKind::Terminal)
            .unwrap();
        apply(&mut app, ServerEvent::TreeSnapshot(tree.clone()));

        apply(
            &mut app,
            ServerEvent::PaneSessionIdResolved {
                pane_id,
                session_id: "session-1".to_string(),
                process_id: Some(12345),
                title_generation: 0,
            },
        );
        app.title_inference_attempts
            .insert((pane_id, "session-1".to_string()), 1);
        app.inferred_title_session_ids
            .insert(pane_id, "session-1".to_string());
        app.enter_press_counts.insert(pane_id, 3);
        app.terminal_retitle_content_hashes.insert(pane_id, 42);
        app.titles_loading.insert(pane_id);
        app.restored_editor_paths
            .insert(pane_id, std::path::PathBuf::from("/tmp/does-not-matter.md"));
        assert!(app.agent_session_ids.contains_key(&pane_id));
        assert_eq!(app.agent_process_ids.get(&pane_id), Some(&12345));
        assert!(app
            .title_inference_attempts
            .contains_key(&(pane_id, "session-1".to_string())));
        assert!(app.inferred_title_session_ids.contains_key(&pane_id));
        assert!(app.enter_press_counts.contains_key(&pane_id));
        assert!(app.terminal_retitle_content_hashes.contains_key(&pane_id));
        assert!(app.titles_loading.contains(&pane_id));
        assert!(app.restored_editor_paths.contains_key(&pane_id));

        tree.remove_node(pane_id).unwrap();
        apply(&mut app, ServerEvent::TreeSnapshot(tree));

        assert!(!app.agent_session_ids.contains_key(&pane_id));
        assert!(!app.agent_process_ids.contains_key(&pane_id));
        assert!(!app
            .title_inference_attempts
            .contains_key(&(pane_id, "session-1".to_string())));
        assert!(!app.inferred_title_session_ids.contains_key(&pane_id));
        assert!(!app.enter_press_counts.contains_key(&pane_id));
        assert!(!app.terminal_retitle_content_hashes.contains_key(&pane_id));
        assert!(!app.titles_loading.contains(&pane_id));
        assert!(!app.restored_editor_paths.contains_key(&pane_id));
    }

    /// Regression test: `title_inference_attempts` is keyed by `(pane_id,
    /// session_id)`, so a `/resume` that changes a still-live pane's session
    /// id is invisible to `apply_tree_snapshot`'s `live_pane_ids`-based
    /// pruning -- that pane never leaves the tree. Without pruning the
    /// previous session's entry at the point the session id changes, each
    /// resume of the same pane would leave one more stale attempt-counter
    /// entry behind for the life of the client process.
    #[test]
    fn session_id_change_prunes_the_previous_sessions_attempt_counter() {
        let mut app = app();
        let mut tree = ilium_core::Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        let pane_id = tree
            .add_pane(group, "claude", PaneContentKind::Terminal)
            .unwrap();
        apply(&mut app, ServerEvent::TreeSnapshot(tree));

        apply(
            &mut app,
            ServerEvent::PaneSessionIdResolved {
                pane_id,
                session_id: "session-1".to_string(),
                process_id: Some(12345),
                title_generation: 0,
            },
        );
        app.title_inference_attempts
            .insert((pane_id, "session-1".to_string()), 1);

        apply(
            &mut app,
            ServerEvent::PaneSessionIdResolved {
                pane_id,
                session_id: "session-2".to_string(),
                process_id: Some(23456),
                title_generation: 1,
            },
        );

        assert!(!app
            .title_inference_attempts
            .contains_key(&(pane_id, "session-1".to_string())));
    }

    #[test]
    fn process_refresh_for_the_same_session_updates_metadata_without_retriggering() {
        let mut app = app();
        let mut tree = ilium_core::Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        let pane_id = tree
            .add_pane(group, "claude", PaneContentKind::Terminal)
            .unwrap();
        tree.set_pane_status(
            pane_id,
            PaneStatus::Agent(AgentClass::Claude, AgentActivity::Working),
        )
        .unwrap();
        apply(&mut app, ServerEvent::TreeSnapshot(tree));

        let first_occurrence = apply(
            &mut app,
            ServerEvent::PaneSessionIdResolved {
                pane_id,
                session_id: "session-1".to_string(),
                process_id: Some(12345),
                title_generation: 0,
            },
        );
        assert_eq!(
            first_occurrence,
            Some(TriggerOccurrence::for_pane(
                TriggerEvent::AgentSessionReady,
                pane_id,
            ))
        );

        app.title_inference_attempts
            .insert((pane_id, "session-1".to_string()), 2);
        let refresh_occurrence = apply(
            &mut app,
            ServerEvent::PaneSessionIdResolved {
                pane_id,
                session_id: "session-1".to_string(),
                process_id: Some(23456),
                title_generation: 0,
            },
        );

        assert_eq!(refresh_occurrence, None);
        assert_eq!(app.agent_process_ids.get(&pane_id), Some(&23456));
        assert_eq!(
            app.title_inference_attempts
                .get(&(pane_id, "session-1".to_string())),
            Some(&2)
        );
    }

    #[test]
    fn session_id_clear_removes_every_title_inference_guard() {
        let mut app = app();
        let mut tree = ilium_core::Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        let pane_id = tree
            .add_pane(group, "claude", PaneContentKind::Terminal)
            .unwrap();
        apply(&mut app, ServerEvent::TreeSnapshot(tree));
        apply(
            &mut app,
            ServerEvent::PaneSessionIdResolved {
                pane_id,
                session_id: "session-1".to_string(),
                process_id: Some(12345),
                title_generation: 0,
            },
        );
        app.title_inference_attempts
            .insert((pane_id, "session-1".to_string()), 2);
        app.inferred_title_session_ids
            .insert(pane_id, "session-1".to_string());
        app.titles_loading.insert(pane_id);

        apply(
            &mut app,
            ServerEvent::PaneSessionIdCleared {
                pane_id,
                title_generation: 1,
            },
        );

        assert!(!app.agent_session_ids.contains_key(&pane_id));
        assert!(!app.agent_process_ids.contains_key(&pane_id));
        assert!(!app
            .title_inference_attempts
            .contains_key(&(pane_id, "session-1".to_string())));
        assert!(!app.inferred_title_session_ids.contains_key(&pane_id));
        assert!(!app.titles_loading.contains(&pane_id));
    }

    #[test]
    fn fresh_conversation_clear_keeps_the_session_but_drops_its_title_guards() {
        let mut app = app();
        let mut tree = ilium_core::Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        let pane_id = tree
            .add_pane(group, "claude", PaneContentKind::Terminal)
            .unwrap();
        apply(&mut app, ServerEvent::TreeSnapshot(tree));
        apply(
            &mut app,
            ServerEvent::PaneSessionIdResolved {
                pane_id,
                session_id: "session-1".to_string(),
                process_id: Some(12345),
                title_generation: 0,
            },
        );
        app.title_inference_attempts
            .insert((pane_id, "session-1".to_string()), 2);
        app.inferred_title_session_ids
            .insert(pane_id, "session-1".to_string());
        app.titles_loading.insert(pane_id);

        apply(
            &mut app,
            ServerEvent::PaneSessionTitleCleared {
                pane_id,
                title_generation: 1,
            },
        );

        assert_eq!(
            app.agent_session_ids.get(&pane_id),
            Some(&"session-1".to_string())
        );
        assert_eq!(app.agent_title_generations.get(&pane_id), Some(&1));
        assert!(!app
            .title_inference_attempts
            .contains_key(&(pane_id, "session-1".to_string())));
        assert!(!app.inferred_title_session_ids.contains_key(&pane_id));
        assert!(!app.titles_loading.contains(&pane_id));
    }

    #[test]
    fn screen_update_feeds_the_matching_terminal_view() {
        let mut app = app();
        let mut tree = ilium_core::Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        let pane_id = tree
            .add_pane(group, "shell", PaneContentKind::Terminal)
            .unwrap();
        apply(&mut app, ServerEvent::TreeSnapshot(tree));

        apply(
            &mut app,
            ServerEvent::ScreenUpdate {
                pane_id,
                first_sequence: 1,
                sequence: 1,
                bytes: b"hello".to_vec(),
            },
        );

        let Some(PaneRuntime::Terminal(view)) = app.panes.get(&pane_id) else {
            panic!("expected a terminal view");
        };
        assert!(view
            .with_screen(|screen| screen.contents())
            .contains("hello"));
        assert_eq!(
            app.terminal_activity
                .phase(pane_id, app.started_at.elapsed().as_millis()),
            Some(TerminalActivityPhase::Fast)
        );
    }

    #[test]
    fn attach_replay_does_not_mark_a_plain_terminal_active() {
        let mut app = app();
        let mut tree = ilium_core::Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        let pane_id = tree
            .add_pane(group, "shell", PaneContentKind::Terminal)
            .unwrap();
        apply(&mut app, ServerEvent::TreeSnapshot(tree));

        apply(
            &mut app,
            ServerEvent::TerminalReplay {
                pane_id,
                through_sequence: 1,
                bytes: b"restored output".to_vec(),
                is_complete: true,
            },
        );

        assert!(!app
            .terminal_activity
            .has_visible_activity(app.started_at.elapsed().as_millis()));
    }

    #[test]
    fn known_agent_output_skips_activity_and_plain_transition_resynchronizes() {
        let mut app = app();
        let mut tree = ilium_core::Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        let pane_id = tree
            .add_pane(group, "terminal", PaneContentKind::Terminal)
            .unwrap();
        apply(&mut app, ServerEvent::TreeSnapshot(tree));
        apply(
            &mut app,
            ServerEvent::PaneStatusChanged {
                pane_id,
                status: PaneStatus::Agent(
                    ilium_core::AgentClass::Codex,
                    ilium_core::AgentActivity::Working,
                ),
            },
        );
        apply(
            &mut app,
            ServerEvent::ScreenUpdate {
                pane_id,
                first_sequence: 1,
                sequence: 1,
                bytes: b"agent output".to_vec(),
            },
        );

        assert!(!app
            .terminal_activity
            .has_visible_activity(app.started_at.elapsed().as_millis()));

        apply(
            &mut app,
            ServerEvent::PaneStatusChanged {
                pane_id,
                status: PaneStatus::PlainShell,
            },
        );
        apply(
            &mut app,
            ServerEvent::ScreenUpdate {
                pane_id,
                first_sequence: 2,
                sequence: 2,
                bytes: b"\x1b[33m".to_vec(),
            },
        );

        assert!(!app
            .terminal_activity
            .has_visible_activity(app.started_at.elapsed().as_millis()));

        apply(
            &mut app,
            ServerEvent::ScreenUpdate {
                pane_id,
                first_sequence: 3,
                sequence: 3,
                bytes: b"\rplain output".to_vec(),
            },
        );

        assert_eq!(
            app.terminal_activity
                .phase(pane_id, app.started_at.elapsed().as_millis()),
            Some(TerminalActivityPhase::Fast)
        );
    }

    #[test]
    fn pane_status_changed_updates_the_tree() {
        let mut app = app();
        let mut tree = ilium_core::Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        let pane_id = tree
            .add_pane(group, "claude", PaneContentKind::Terminal)
            .unwrap();
        apply(&mut app, ServerEvent::TreeSnapshot(tree));
        app.agent_session_ids
            .insert(pane_id, "session-ready".to_owned());

        let applied = apply(
            &mut app,
            ServerEvent::PaneStatusChanged {
                pane_id,
                status: ilium_core::PaneStatus::Agent(
                    ilium_core::AgentClass::Claude,
                    ilium_core::AgentActivity::Working,
                ),
            },
        );

        assert_eq!(
            applied,
            Some(TriggerOccurrence::for_pane(
                TriggerEvent::AgentSessionReady,
                pane_id,
            ))
        );

        match &app.tree.get(pane_id).unwrap().kind {
            NodeKind::Pane { status, .. } => assert_eq!(
                *status,
                ilium_core::PaneStatus::Agent(
                    ilium_core::AgentClass::Claude,
                    ilium_core::AgentActivity::Working
                )
            ),
            _ => panic!("expected a pane"),
        }
    }

    #[test]
    fn agent_finished_trigger_uses_the_sound_transition_classifier() {
        let mut app = app();
        let mut tree = ilium_core::Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        let pane_id = tree
            .add_pane(group, "codex", PaneContentKind::Terminal)
            .unwrap();
        tree.set_pane_status(
            pane_id,
            PaneStatus::Agent(AgentClass::Codex, AgentActivity::WaitingBackground),
        )
        .unwrap();
        apply(&mut app, ServerEvent::TreeSnapshot(tree));

        let finished = apply(
            &mut app,
            ServerEvent::PaneStatusChanged {
                pane_id,
                status: PaneStatus::Agent(AgentClass::Codex, AgentActivity::Idle),
            },
        );

        assert_eq!(
            finished,
            Some(TriggerOccurrence::for_pane(
                TriggerEvent::AgentFinishedWork,
                pane_id,
            ))
        );
        assert_eq!(
            ilium_sound::event_for_transition(
                Some(&PaneStatus::Agent(
                    AgentClass::Codex,
                    AgentActivity::WaitingBackground,
                )),
                &PaneStatus::Agent(AgentClass::Codex, AgentActivity::Idle),
            ),
            Some(ilium_sound::SoundEvent::AgentFinished)
        );
    }

    #[test]
    fn prompt_events_distinguish_agent_submissions_from_terminal_cadence() {
        let mut app = app();
        let mut tree = ilium_core::Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        let shell_id = tree
            .add_pane(group, "shell", PaneContentKind::Terminal)
            .unwrap();
        let agent_id = tree
            .add_pane(group, "agent", PaneContentKind::Terminal)
            .unwrap();
        tree.set_pane_status(
            agent_id,
            PaneStatus::Agent(AgentClass::Claude, AgentActivity::Working),
        )
        .unwrap();
        apply(&mut app, ServerEvent::TreeSnapshot(tree));

        assert_eq!(
            apply(
                &mut app,
                ServerEvent::PanePromptSubmitted {
                    pane_id: shell_id,
                    source: PromptSubmissionSource::Keyboard,
                },
            ),
            None
        );
        assert_eq!(
            apply(
                &mut app,
                ServerEvent::PanePromptSubmitted {
                    pane_id: shell_id,
                    source: PromptSubmissionSource::ScheduledInput,
                },
            ),
            Some(TriggerOccurrence::for_pane(
                TriggerEvent::TerminalActivityCheckpoint,
                shell_id,
            ))
        );
        assert_eq!(
            apply(
                &mut app,
                ServerEvent::PanePromptSubmitted {
                    pane_id: agent_id,
                    source: PromptSubmissionSource::QueuedPrompt,
                },
            ),
            Some(TriggerOccurrence::for_pane(
                TriggerEvent::AgentPromptSubmitted,
                agent_id,
            ))
        );
    }

    #[test]
    fn initial_state_complete_is_the_global_startup_trigger() {
        let mut app = app();
        assert_eq!(
            apply(&mut app, ServerEvent::InitialStateSyncComplete),
            Some(TriggerOccurrence::global(TriggerEvent::StartupComplete))
        );
    }
}

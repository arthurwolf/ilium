//! Applies incoming `ServerEvent`s to `App`'s render-cache state. This is
//! the one place the client-local `tree`/`panes` maps are ever written to
//! from network input -- see `app.rs`'s module docs for why that's the
//! only kind of write they ever get (everything else flows the other way,
//! as a `ClientRequest`).

use ilium_core::{AgentActivity, NodeId, NodeKind, PaneContentKind, PaneStatus};
use ilium_ipc::ServerEvent;

use crate::app::{App, PaneRuntime};
use crate::board::BoardPane;
use crate::editor_pane::EditorPane;
use crate::terminal_view::TerminalView;
use crate::title_inference::AppliedEvent;

/// How a new authoritative tree snapshot affects the current sidebar
/// selection. Keeping the removed-without-successor state explicit ensures a
/// stale widget path is cleared even when the session became empty.
enum SelectionReconciliation {
    Unchanged,
    Removed(Option<NodeId>),
}

/// Applies one `ServerEvent` to `app`. Called from the connection task's
/// read loop for every frame it decodes. Returns what this event means for
/// `crate::title_inference`'s triggers -- `AppliedEvent::Other` for
/// every event/transition that isn't one of them.
pub fn apply(app: &mut App, event: ServerEvent) -> AppliedEvent {
    match event {
        ServerEvent::TreeSnapshot(tree) => {
            apply_tree_snapshot(app, tree);
            AppliedEvent::Other
        }
        ServerEvent::SessionRecoveryAvailable { pane_count } => {
            app.mode = crate::app::Mode::ConfirmSessionRecovery { pane_count };
            AppliedEvent::Other
        }
        ServerEvent::ScreenUpdate {
            pane_id,
            sequence,
            bytes,
        } => {
            if let Some(PaneRuntime::Terminal(view)) = app.panes.get_mut(&pane_id) {
                view.apply_live_output(sequence, &bytes);
            }
            AppliedEvent::Other
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
            AppliedEvent::Other
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
            let became_done = matches!(
                status,
                PaneStatus::Agent(_, AgentActivity::Done)
                    | PaneStatus::AgentWithGoal(_, AgentActivity::Done)
            ) && previous_status.as_ref() != Some(&status);
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
                    // Type ordering distinguishes agents from plain shells;
                    // invalidate structural hit testing when detection changes
                    // that visible rank even though no TreeSnapshot follows.
                    app.bump_tree_version();
                    if became_done {
                        AppliedEvent::PaneBecameDone { pane_id }
                    } else if became_agent {
                        AppliedEvent::PaneBecameAgent { pane_id }
                    } else {
                        AppliedEvent::Other
                    }
                }
                Err(error) => {
                    tracing::warn!("dropping PaneStatusChanged for pane {pane_id:?}: {error}");
                    AppliedEvent::Other
                }
            }
        }
        ServerEvent::Error { message } => {
            app.status_message = Some(format!("Server error: {message}"));
            AppliedEvent::Other
        }
        ServerEvent::PaneSessionIdResolved {
            pane_id,
            session_id,
            title_generation,
        } => {
            app.agent_title_generations
                .insert(pane_id, title_generation);
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
            AppliedEvent::SessionIdResolved { pane_id }
        }
        ServerEvent::PaneSessionIdCleared {
            pane_id,
            title_generation,
        } => {
            app.agent_title_generations
                .insert(pane_id, title_generation);
            if let Some(previous_session_id) = app.agent_session_ids.remove(&pane_id) {
                app.title_inference_attempts
                    .remove(&(pane_id, previous_session_id));
            }
            app.inferred_title_session_ids.remove(&pane_id);
            app.titles_loading.remove(&pane_id);
            AppliedEvent::Other
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
            AppliedEvent::Other
        }
        ServerEvent::PaneEditorPathResolved { pane_id, path } => {
            let Some(path) = path else {
                app.status_message = Some("Restored editor has no file path".to_string());
                return AppliedEvent::Other;
            };
            app.restored_editor_paths.insert(pane_id, path);
            load_restored_editor(app, pane_id);
            AppliedEvent::Other
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
    app.bump_tree_version();
    app.prune_recently_created();

    let live_pane_ids: std::collections::HashSet<_> =
        app.tree.panes().map(|node| node.id).collect();
    app.panes
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
    if app
        .hovered_tree_node
        .is_some_and(|hit| app.tree.get(hit.id).is_none())
    {
        app.hovered_tree_node = None;
    }

    let (rows, cols) = app.last_known_pane_size;
    let mut newly_opened_editor: Option<NodeId> = None;
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
                        newly_opened_editor = Some(pane_id);
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
                        newly_opened_editor = Some(pane_id);
                    }
                    Err(error) => {
                        app.status_message = Some(format!("Failed to open board: {error}"))
                    }
                }
            }
        }
    }
    app.reconcile_right_panel_target();
    if let Some(pane_id) = newly_opened_editor {
        app.focus_pane(pane_id);
    } else if let SelectionReconciliation::Removed(replacement) = selection_reconciliation {
        if let Some(node_id) = replacement {
            app.activate_tree_successor(node_id);
        } else {
            app.tree_state.select(Vec::new());
            app.leave_pane_focus();
        }
    }
    app.resize_displayed_panes();
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
        Ok(editor) => {
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
    use ilium_core::{AgentClass, PaneContentKind, ROOT_ID};
    use ratatui::layout::{Position, Rect};

    use super::*;

    fn app() -> App {
        App::new("test".to_string(), std::env::temp_dir())
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
                title_generation: 1,
            },
        );

        assert!(!app
            .title_inference_attempts
            .contains_key(&(pane_id, "session-1".to_string())));
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

        assert_eq!(applied, AppliedEvent::PaneBecameAgent { pane_id });

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
}

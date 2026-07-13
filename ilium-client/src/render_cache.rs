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

/// Applies one `ServerEvent` to `app`. Called from the connection task's
/// read loop for every frame it decodes. Returns what this event means for
/// `crate::title_inference`'s two triggers -- `AppliedEvent::Other` for
/// every event/transition that isn't one of them.
pub fn apply(app: &mut App, event: ServerEvent) -> AppliedEvent {
    match event {
        ServerEvent::TreeSnapshot(tree) => {
            apply_tree_snapshot(app, tree);
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
            let became_done = matches!(status, PaneStatus::Agent(_, AgentActivity::Done))
                && previous_status.as_ref() != Some(&status);
            // Only report `PaneBecameDone` -- and thus trigger a title
            // inference attempt -- if the status actually landed in the
            // tree. If `pane_id` doesn't resolve to a pane here (a status
            // event for a pane this client's tree doesn't know about yet),
            // reporting the transition anyway would be describing a state
            // change that never actually happened.
            match app.tree.set_pane_status(pane_id, status) {
                Ok(()) if became_done => AppliedEvent::PaneBecameDone { pane_id },
                Ok(()) => AppliedEvent::Other,
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
        } => {
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
    app.track_newly_created_nodes(&tree);
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
    app.enter_press_counts
        .retain(|pane_id, _| live_pane_ids.contains(pane_id));
    app.terminal_retitle_content_hashes
        .retain(|pane_id, _| live_pane_ids.contains(pane_id));
    app.titles_loading
        .retain(|pane_id| live_pane_ids.contains(pane_id));

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
                    PaneRuntime::Terminal(Box::new(TerminalView::new(rows, cols))),
                );
            }
            PaneContentKind::Editor => {
                let path = app
                    .restored_editor_paths
                    .get(&pane_id)
                    .cloned()
                    .or_else(|| app.take_matching_pending_editor_open(&name));
                let Some(path) = path else {
                    continue;
                };
                match EditorPane::load(path) {
                    Ok(editor) => {
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
    if let Some(pane_id) = newly_opened_editor {
        app.focus_pane(pane_id);
    }

    app.reconcile_right_panel_target();
    app.resize_displayed_panes();
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
    use ilium_core::{PaneContentKind, ROOT_ID};

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
            },
        );
        app.title_inference_attempts
            .insert((pane_id, "session-1".to_string()), 1);

        apply(
            &mut app,
            ServerEvent::PaneSessionIdResolved {
                pane_id,
                session_id: "session-2".to_string(),
            },
        );

        assert!(!app
            .title_inference_attempts
            .contains_key(&(pane_id, "session-1".to_string())));
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

        apply(
            &mut app,
            ServerEvent::PaneStatusChanged {
                pane_id,
                status: ilium_core::PaneStatus::Agent(
                    ilium_core::AgentClass::Claude,
                    ilium_core::AgentActivity::Working,
                ),
            },
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
}

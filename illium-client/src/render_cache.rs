//! Applies incoming `ServerEvent`s to `App`'s render-cache state. This is
//! the one place the client-local `tree`/`panes` maps are ever written to
//! from network input -- see `app.rs`'s module docs for why that's the
//! only kind of write they ever get (everything else flows the other way,
//! as a `ClientRequest`).

use illium_core::{NodeId, NodeKind, PaneContentKind};
use illium_ipc::ServerEvent;

use crate::app::{App, PaneRuntime};
use crate::editor_pane::EditorPane;
use crate::terminal_view::TerminalView;

/// Applies one `ServerEvent` to `app`. Called from the connection task's
/// read loop for every frame it decodes.
pub fn apply(app: &mut App, event: ServerEvent) {
    match event {
        ServerEvent::TreeSnapshot(tree) => apply_tree_snapshot(app, tree),
        ServerEvent::ScreenUpdate { pane_id, bytes } => {
            if let Some(PaneRuntime::Terminal(view)) = app.panes.get_mut(&pane_id) {
                view.feed(&bytes);
            }
        }
        ServerEvent::PaneStatusChanged { pane_id, status } => {
            let _ = app.tree.set_pane_status(pane_id, status);
        }
        ServerEvent::Error { message } => {
            app.status_message = Some(format!("Server error: {message}"));
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
fn apply_tree_snapshot(app: &mut App, tree: illium_core::Tree) {
    app.tree = tree;

    let live_pane_ids: std::collections::HashSet<_> =
        app.tree.panes().map(|node| node.id).collect();
    app.panes
        .retain(|pane_id, _| live_pane_ids.contains(pane_id));

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
                let Some(path) = app.take_matching_pending_editor_open(&name) else {
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
        }
    }
    if let Some(pane_id) = newly_opened_editor {
        app.focus_pane(pane_id);
    }

    if let Some(focused) = app.focused_pane {
        if app.tree.get(focused).is_none() {
            app.focused_pane = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use illium_core::{PaneContentKind, ROOT_ID};

    use super::*;

    fn app() -> App {
        App::new("test".to_string(), std::env::temp_dir())
    }

    #[test]
    fn tree_snapshot_creates_a_terminal_view_for_a_new_pane() {
        let mut app = app();
        let mut tree = illium_core::Tree::new();
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
        let mut tree = illium_core::Tree::new();
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
    fn screen_update_feeds_the_matching_terminal_view() {
        let mut app = app();
        let mut tree = illium_core::Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        let pane_id = tree
            .add_pane(group, "shell", PaneContentKind::Terminal)
            .unwrap();
        apply(&mut app, ServerEvent::TreeSnapshot(tree));

        apply(
            &mut app,
            ServerEvent::ScreenUpdate {
                pane_id,
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
        let mut tree = illium_core::Tree::new();
        let group = tree.add_group(ROOT_ID, "work").unwrap();
        let pane_id = tree
            .add_pane(group, "claude", PaneContentKind::Terminal)
            .unwrap();
        apply(&mut app, ServerEvent::TreeSnapshot(tree));

        apply(
            &mut app,
            ServerEvent::PaneStatusChanged {
                pane_id,
                status: illium_core::PaneStatus::Agent(
                    illium_core::AgentClass::Claude,
                    illium_core::AgentActivity::Working,
                ),
            },
        );

        match &app.tree.get(pane_id).unwrap().kind {
            NodeKind::Pane { status, .. } => assert_eq!(
                *status,
                illium_core::PaneStatus::Agent(
                    illium_core::AgentClass::Claude,
                    illium_core::AgentActivity::Working
                )
            ),
            _ => panic!("expected a pane"),
        }
    }
}

//! Mouse dispatch: routes crossterm mouse events according to
//! `App::mode`/the last-rendered layout, mirroring the pre-client/server
//! `App::handle_mouse_event`'s structure. Terminal-pane clicks/drags become
//! a queued `MouseInput` request (see `to_ipc_mouse_event`); tree-panel
//! selection, hover, and modal/editor-chrome interaction stay purely local,
//! except for a tree-row drag-and-drop, which queues a `ReparentNode`
//! request (see `compute_drop_target`) the same way any other structural
//! tree edit does.

use crossterm::event::{Event, MouseButton, MouseEvent, MouseEventKind};
use illium_core::{NodeId, NodeKind, Tree, ROOT_ID};
use ratatui::layout::Position;

use crate::app::{App, ContextMenu, CreateGroupState, FocusTarget, Mode};
use crate::explorer_overlay::ExplorerOverlay;
use crate::tree_ui::{self, TreeRowAction, TreeToolbarAction};

/// Converts a crossterm mouse event's kind/modifiers into the wire shapes
/// `illium_ipc::ClientRequest::MouseInput` carries. The two enums are a
/// deliberate 1:1 mirror of each other (see `illium_server::mouse`'s
/// reverse conversion), so this never needs to drop or approximate a kind.
pub fn to_ipc_mouse_event(
    mouse: MouseEvent,
) -> (illium_ipc::MouseEventKind, illium_ipc::MouseModifiers) {
    let kind = match mouse.kind {
        MouseEventKind::Down(button) => illium_ipc::MouseEventKind::Down(to_ipc_button(button)),
        MouseEventKind::Up(button) => illium_ipc::MouseEventKind::Up(to_ipc_button(button)),
        MouseEventKind::Drag(button) => illium_ipc::MouseEventKind::Drag(to_ipc_button(button)),
        MouseEventKind::Moved => illium_ipc::MouseEventKind::Moved,
        MouseEventKind::ScrollUp => illium_ipc::MouseEventKind::ScrollUp,
        MouseEventKind::ScrollDown => illium_ipc::MouseEventKind::ScrollDown,
        MouseEventKind::ScrollLeft => illium_ipc::MouseEventKind::ScrollLeft,
        MouseEventKind::ScrollRight => illium_ipc::MouseEventKind::ScrollRight,
    };
    let modifiers = illium_ipc::MouseModifiers {
        shift: mouse
            .modifiers
            .contains(crossterm::event::KeyModifiers::SHIFT),
        alt: mouse
            .modifiers
            .contains(crossterm::event::KeyModifiers::ALT),
        control: mouse
            .modifiers
            .contains(crossterm::event::KeyModifiers::CONTROL),
    };
    (kind, modifiers)
}

fn to_ipc_button(button: MouseButton) -> illium_ipc::MouseButton {
    match button {
        MouseButton::Left => illium_ipc::MouseButton::Left,
        MouseButton::Right => illium_ipc::MouseButton::Right,
        MouseButton::Middle => illium_ipc::MouseButton::Middle,
    }
}

/// Top-level mouse dispatch, called for every `Event::Mouse`.
pub fn handle_mouse_event(app: &mut App, mouse: MouseEvent) {
    let position = Position::new(mouse.column, mouse.row);
    app.set_terminal_focused(true);
    app.set_pointer_position(Some(position));

    // Only actually take `app.mode` out when it's a variant this function
    // handles -- mirrors the pre-client/server design's own care here (see
    // its comment): swapping out any mode unconditionally and never putting
    // it back would destroy in-progress modal state (e.g. the file picker)
    // on the very next mouse event.
    if matches!(app.mode, Mode::ContextMenu(_)) {
        let Mode::ContextMenu(menu) = std::mem::replace(&mut app.mode, Mode::Normal) else {
            unreachable!("just matched Mode::ContextMenu above");
        };
        handle_context_menu_mouse(app, menu, mouse);
        return;
    }
    if matches!(app.mode, Mode::CreateGroup(_)) {
        let Mode::CreateGroup(state) = std::mem::replace(&mut app.mode, Mode::Normal) else {
            unreachable!("just matched Mode::CreateGroup above");
        };
        handle_create_group_mouse(app, state, mouse);
        return;
    }
    if matches!(app.mode, Mode::Explorer(..)) {
        let Mode::Explorer(overlay, target) = std::mem::replace(&mut app.mode, Mode::Normal) else {
            unreachable!("just matched Mode::Explorer above");
        };
        handle_explorer_mouse(app, overlay, target, mouse);
        return;
    }

    // Modal keyboard overlays intentionally ignore pointer input until
    // they grow their own hit-testing contract.
    if matches!(
        app.mode,
        Mode::Help
            | Mode::Rename(_)
            | Mode::CommandPrompt(_)
            | Mode::SaveAs(..)
            | Mode::ConfirmClose(_)
    ) {
        return;
    }

    if app.layout.tree_area.contains(position) {
        handle_tree_mouse(app, mouse, position);
        return;
    }
    app.set_hovered_tree_node(None);
    app.set_tree_toolbar_hover(false, None);

    if app.layout.pane_area.contains(position) {
        app.handle_pane_mouse(mouse, position);
        if matches!(mouse.kind, MouseEventKind::Up(MouseButton::Left)) {
            app.set_drag_source(None);
        }
        return;
    }

    // A drag released outside the tree is a cancelled tree move.
    if matches!(mouse.kind, MouseEventKind::Up(MouseButton::Left)) {
        app.set_drag_source(None);
    }
}

/// Selects tree rows, opens the right-click menu, scrolls, and handles the
/// toolbar/row hover controls.
fn handle_tree_mouse(app: &mut App, mouse: MouseEvent, position: Position) {
    app.focus = FocusTarget::Tree;
    update_tree_hover(app, position);

    if matches!(mouse.kind, MouseEventKind::Moved) {
        return;
    }

    if let Some(action) = tree_ui::toolbar_action_at(app.layout.tree_area, position) {
        if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
            execute_tree_toolbar_action(app, action);
        }
        return;
    }

    if let Some(hit) = app.hovered_tree_node {
        if let Some(action) = tree_ui::row_action_at(app.layout.tree_area, hit.row, position) {
            if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
                handle_tree_row_action(app, hit.id, action);
            }
            return;
        }
    }

    match mouse.kind {
        MouseEventKind::ScrollUp => {
            app.tree_state.scroll_up(3);
        }
        MouseEventKind::ScrollDown => {
            app.tree_state.scroll_down(3);
        }
        MouseEventKind::Down(MouseButton::Right) => {
            // No node under the click means empty space below the last
            // entry -- fall back to ROOT_ID so "New group" lands at the
            // top level instead of doing nothing.
            let target = app
                .tree_node_at(position)
                .map(|hit| hit.id)
                .unwrap_or(ROOT_ID);
            app.select_node(target);
            app.open_context_menu(target, mouse.column, mouse.row);
        }
        MouseEventKind::Down(MouseButton::Left) => {
            if let Some(hit) = app.tree_node_at(position) {
                app.select_node(hit.id);
                app.set_drag_source(Some(hit.id));
                if matches!(
                    app.tree.get(hit.id).map(|node| &node.kind),
                    Some(NodeKind::Group { .. })
                ) {
                    app.tree_state.toggle_selected();
                } else {
                    app.focus_pane(hit.id);
                }
            }
        }
        MouseEventKind::Up(MouseButton::Left) => {
            // Arbitrary drag-and-drop: drop onto another tree row (its
            // `NodeId`) or `None` for the empty space below the last row
            // (meaning "append at the top level") -- see
            // `compute_drop_target`'s doc comment for the exact target
            // rules and the cases that are rejected client-side rather
            // than round-tripped to the server.
            if let Some(dragged_id) = app.drag_source() {
                let drop_target = app.tree_node_at(position).map(|hit| hit.id);
                if let Some((new_parent, index)) =
                    compute_drop_target(&app.tree, dragged_id, drop_target)
                {
                    app.request_reparent(dragged_id, new_parent, index);
                }
            }
            app.set_drag_source(None);
        }
        _ => {}
    }
}

/// Updates the two independent hover affordances (row hit + toolbar) from
/// the pointer's current tree-panel-relative position.
fn update_tree_hover(app: &mut App, position: Position) {
    let hit = app.tree_node_at(position);
    app.set_hovered_tree_node(hit);
    let toolbar_action = tree_ui::toolbar_action_at(app.layout.tree_area, position);
    let toolbar_hovered = tree_ui::toolbar_area(app.layout.tree_area).contains(position);
    app.set_tree_toolbar_hover(toolbar_hovered, toolbar_action);
}

fn handle_tree_row_action(app: &mut App, id: illium_core::NodeId, action: TreeRowAction) {
    app.select_node(id);
    match action {
        TreeRowAction::Rename => app.action_start_rename(),
        TreeRowAction::MoveUp => app.request_move(id, illium_core::TreeMoveDirection::Up),
        TreeRowAction::MoveDown => app.request_move(id, illium_core::TreeMoveDirection::Down),
        TreeRowAction::Close => app.action_close_selected(),
    }
}

/// True if `ancestor` is `node` itself or a transitive parent of `node` --
/// a client-side mirror of `illium_core::Tree`'s own (private) check of the
/// same name. Needed here so a drop onto the dragged node's own descendant
/// is rejected before ever forming a request, not just after a round trip
/// to the server.
fn is_ancestor_of(tree: &Tree, ancestor: NodeId, node: NodeId) -> bool {
    let mut current = Some(node);
    while let Some(id) = current {
        if id == ancestor {
            return true;
        }
        current = tree.parent_of(id);
    }
    false
}

/// Computes the `ReparentNode` target (new parent + insertion index) for
/// dropping `dragged_id` onto `drop_target` -- `None` means the drop landed
/// in the empty space below the last tree row, meaning "append at the top
/// level". Dropping onto a `Group` row places `dragged_id` as the last
/// child of that group; dropping onto a `Pane` row places it as that
/// pane's immediate predecessor, within the pane's own parent group.
///
/// Returns `None` -- meaning nothing should be sent at all -- for the
/// cases that are unambiguously invalid without asking the server: dropping
/// a node onto itself or one of its own descendants, and dropping a pane at
/// the top level (panes always need an enclosing group). Every other
/// outcome is still just a request; the server has the final say, and any
/// other rejection (e.g. a stale id from a race with a concurrent
/// structural change) surfaces as `ServerEvent::Error` rather than crashing
/// the client -- see `crate::render_cache::apply`.
fn compute_drop_target(
    tree: &Tree,
    dragged_id: NodeId,
    drop_target: Option<NodeId>,
) -> Option<(NodeId, Option<usize>)> {
    let dragged_is_pane = matches!(
        tree.get(dragged_id).map(|node| &node.kind),
        Some(NodeKind::Pane { .. })
    );

    let (new_parent, index) = match drop_target {
        None => (ROOT_ID, None),
        Some(target_id) => {
            if target_id == dragged_id || is_ancestor_of(tree, dragged_id, target_id) {
                return None;
            }
            match tree.get(target_id).map(|node| &node.kind) {
                Some(NodeKind::Group { .. }) => (target_id, None),
                Some(NodeKind::Pane { .. }) => {
                    let parent = tree.parent_of(target_id)?;
                    let siblings = tree.children_of(parent).ok()?;
                    let position = siblings.iter().position(|&sibling| sibling == target_id)?;
                    (parent, Some(position))
                }
                None => return None,
            }
        }
    };

    if new_parent == ROOT_ID && dragged_is_pane {
        return None;
    }
    Some((new_parent, index))
}

/// Executes a bottom-toolbar creation action. Agent entries create their
/// pane pre-loaded with the exact command line (`NewPaneKind::Command`),
/// which the server runs directly rather than typing it into an
/// interactive shell -- see `illium_server::pane::TerminalOrigin::Command`.
fn execute_tree_toolbar_action(app: &mut App, action: TreeToolbarAction) {
    if matches!(action, TreeToolbarAction::Group) {
        let preselected = app.create_group_preselect_target();
        app.open_create_group_dialog(preselected);
        return;
    }
    match action {
        TreeToolbarAction::Shell => app.action_new_terminal(),
        TreeToolbarAction::Claude => app.action_new_command_pane("claude"),
        TreeToolbarAction::Codex => app.action_new_command_pane("codex"),
        TreeToolbarAction::Editor => app.action_new_editor(),
        TreeToolbarAction::Group => unreachable!("Group returned early above"),
    }
    app.status_message = Some(format!("Created {}", action.description()));
}

/// Handles an activated context-menu entry or dismisses a click outside.
fn handle_context_menu_mouse(app: &mut App, mut menu: ContextMenu, mouse: MouseEvent) {
    if !matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
        app.mode = Mode::ContextMenu(menu);
        return;
    }
    let position = Position::new(mouse.column, mouse.row);
    if !menu.area.contains(position) {
        app.mode = Mode::Normal;
        return;
    }
    let item_row = position.y.saturating_sub(menu.area.y.saturating_add(1)) as usize;
    if item_row >= menu.actions.len() {
        app.mode = Mode::ContextMenu(menu);
        return;
    }
    menu.selected_index = item_row;
    app.select_node(menu.target);
    app.execute_context_action(menu.actions[item_row], menu.target);
}

/// Mouse handling for the create-group dialog: clicking a destination row
/// immediately creates the group there, clicking outside cancels.
fn handle_create_group_mouse(app: &mut App, state: CreateGroupState, mouse: MouseEvent) {
    if !matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
        app.mode = Mode::CreateGroup(state);
        return;
    }
    let position = Position::new(mouse.column, mouse.row);
    if !state.area.contains(position) {
        app.mode = Mode::Normal;
        return;
    }
    let layout = crate::modal::create_group_layout(state.area);
    let window = crate::modal::create_group_visible_window(
        state.selected_index,
        state.destinations.len(),
        crate::modal::CREATE_GROUP_MAX_VISIBLE,
    );
    match crate::modal::create_group_row_at(&layout, window, position) {
        Some(index) => {
            let mut state = state;
            state.selected_index = index;
            app.commit_create_group(&state);
        }
        None => app.mode = Mode::CreateGroup(state),
    }
}

/// Routes a mouse event to the open file picker overlay -- the same
/// row-select/scroll/click-to-activate hit-testing it already owns.
fn handle_explorer_mouse(
    app: &mut App,
    mut overlay: Box<ExplorerOverlay>,
    target: illium_core::NodeId,
    mouse: MouseEvent,
) {
    match overlay.handle(&Event::Mouse(mouse), app.layout.screen_area) {
        Ok(Some(path)) => {
            app.request_new_editor(target, path);
            app.mode = Mode::Normal;
        }
        Ok(None) => app.mode = Mode::Explorer(overlay, target),
        Err(err) => {
            app.status_message = Some(format!("File picker error: {err}"));
            app.mode = Mode::Explorer(overlay, target);
        }
    }
}

#[cfg(test)]
mod drop_target_tests {
    use super::*;

    /// Two top-level groups, `a` (containing pane `b`) and `c` (containing
    /// pane `d`).
    fn sample_tree() -> (Tree, NodeId, NodeId, NodeId, NodeId) {
        let mut tree = Tree::new();
        let group_a = tree.add_group(ROOT_ID, "a").unwrap();
        let pane_b = tree
            .add_pane(group_a, "b", illium_core::PaneContentKind::Terminal)
            .unwrap();
        let group_c = tree.add_group(ROOT_ID, "c").unwrap();
        let pane_d = tree
            .add_pane(group_c, "d", illium_core::PaneContentKind::Terminal)
            .unwrap();
        (tree, group_a, pane_b, group_c, pane_d)
    }

    #[test]
    fn dropping_onto_a_group_appends_as_its_last_child() {
        let (tree, group_a, _pane_b, group_c, _pane_d) = sample_tree();
        assert_eq!(
            compute_drop_target(&tree, group_a, Some(group_c)),
            Some((group_c, None))
        );
    }

    #[test]
    fn dropping_onto_a_pane_inserts_right_before_it_in_its_parent() {
        let (tree, group_a, _pane_b, group_c, pane_d) = sample_tree();
        assert_eq!(
            compute_drop_target(&tree, group_a, Some(pane_d)),
            Some((group_c, Some(0)))
        );
    }

    #[test]
    fn dropping_in_empty_space_appends_at_the_top_level() {
        let (tree, group_a, _pane_b, _group_c, _pane_d) = sample_tree();
        assert_eq!(
            compute_drop_target(&tree, group_a, None),
            Some((ROOT_ID, None))
        );
    }

    #[test]
    fn dropping_a_pane_in_empty_space_is_rejected_since_panes_require_a_group() {
        let (tree, _group_a, pane_b, ..) = sample_tree();
        assert_eq!(compute_drop_target(&tree, pane_b, None), None);
    }

    #[test]
    fn dropping_a_node_onto_itself_is_rejected() {
        let (tree, group_a, ..) = sample_tree();
        assert_eq!(compute_drop_target(&tree, group_a, Some(group_a)), None);
    }

    #[test]
    fn dropping_a_group_onto_its_own_descendant_is_rejected() {
        let (tree, group_a, pane_b, ..) = sample_tree();
        assert_eq!(compute_drop_target(&tree, group_a, Some(pane_b)), None);
    }
}

//! Mouse dispatch: routes crossterm mouse events according to
//! `App::mode`/the last-rendered layout, mirroring the pre-client/server
//! `App::handle_mouse_event`'s structure. Terminal-pane clicks/drags become
//! a queued `MouseInput` request (see `to_ipc_mouse_event`); everything
//! about the tree panel, modals, and editor chrome stays purely local
//! since none of that needs the server at all.

use crossterm::event::{Event, MouseButton, MouseEvent, MouseEventKind};
use illium_core::{NodeKind, ROOT_ID};
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
            // Arbitrary drag-and-drop reordering (dropping a node onto an
            // arbitrary sibling position, possibly under a different
            // parent) is not implemented: it needs a reparent-to-index
            // tree mutation `illium_ipc::ClientRequest` has no shape for
            // yet (only the one-step `MoveNode { direction }`) -- see
            // `crate::app::ContextMenuAction`'s doc comment for the same
            // protocol gap. The drag source is cleared without attempting
            // a move; `Mode::Move` (keyboard, leader `m`) and the tree
            // row's ↑/↓ hover buttons both still work today since they
            // only ever need a one-step move.
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

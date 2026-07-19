//! Mouse dispatch: routes crossterm mouse events according to
//! `App::mode`/the last-rendered layout, mirroring the pre-client/server
//! `App::handle_mouse_event`'s structure. Terminal-pane clicks/drags become
//! a queued `MouseInput` request (see `to_ipc_mouse_event`); tree-panel
//! selection, hover, and modal/editor-chrome interaction stay purely local,
//! except for a tree-row drag-and-drop, which queues a `ReparentNode`
//! request (see `compute_drop_target`) the same way any other structural
//! tree edit does.

use crossterm::event::{Event, MouseButton, MouseEvent, MouseEventKind};
use ilium_core::{AgentProvider, NodeId, NodeKind, Tree, ROOT_ID};
use ratatui::layout::{Position, Rect};

use crate::agent_from_line::{CreateAgentFocus, CreateAgentFromLineState, EditorLineContextMenu};
use crate::app::{App, ContextMenu, CreateGroupState, Mode};
use crate::explorer_overlay::ExplorerOverlay;
use crate::prompt_queue::{PromptQueueDialogState, PromptQueueFocus};
use crate::scheduled_input::{ScheduledInputDialogState, ScheduledInputFocus};
use crate::tree_ui::{self, TreeRowAction, TreeToolbarAction};

/// Converts a crossterm mouse event's kind/modifiers into the wire shapes
/// `ilium_ipc::ClientRequest::MouseInput` carries. The two enums are a
/// deliberate 1:1 mirror of each other (see `ilium_server::mouse`'s
/// reverse conversion), so this never needs to drop or approximate a kind.
pub fn to_ipc_mouse_event(
    mouse: MouseEvent,
) -> (ilium_ipc::MouseEventKind, ilium_ipc::MouseModifiers) {
    let kind = match mouse.kind {
        MouseEventKind::Down(button) => ilium_ipc::MouseEventKind::Down(to_ipc_button(button)),
        MouseEventKind::Up(button) => ilium_ipc::MouseEventKind::Up(to_ipc_button(button)),
        MouseEventKind::Drag(button) => ilium_ipc::MouseEventKind::Drag(to_ipc_button(button)),
        MouseEventKind::Moved => ilium_ipc::MouseEventKind::Moved,
        MouseEventKind::ScrollUp => ilium_ipc::MouseEventKind::ScrollUp,
        MouseEventKind::ScrollDown => ilium_ipc::MouseEventKind::ScrollDown,
        MouseEventKind::ScrollLeft => ilium_ipc::MouseEventKind::ScrollLeft,
        MouseEventKind::ScrollRight => ilium_ipc::MouseEventKind::ScrollRight,
    };
    let modifiers = ilium_ipc::MouseModifiers {
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

fn to_ipc_button(button: MouseButton) -> ilium_ipc::MouseButton {
    match button {
        MouseButton::Left => ilium_ipc::MouseButton::Left,
        MouseButton::Right => ilium_ipc::MouseButton::Right,
        MouseButton::Middle => ilium_ipc::MouseButton::Middle,
    }
}

/// Top-level mouse dispatch, called for every `Event::Mouse`.
pub fn handle_mouse_event(app: &mut App, mouse: MouseEvent) {
    let position = Position::new(mouse.column, mouse.row);
    app.set_terminal_focused(true);
    app.set_pointer_position(Some(position));

    // The voice control is global and rendered above every full-screen view,
    // so its hit-test must precede modal dispatch as well.
    if app.layout.voice_control_area.contains(position)
        && matches!(mouse.kind, MouseEventKind::Up(MouseButton::Left))
    {
        app.toggle_voice_control();
        return;
    }

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
    if matches!(app.mode, Mode::Search(_)) {
        let Mode::Search(mut state) = std::mem::replace(&mut app.mode, Mode::Normal) else {
            unreachable!("just matched Mode::Search above");
        };
        handle_search_mouse(app, &mut state, mouse);
        return;
    }
    if matches!(app.mode, Mode::SchedulePaneInput(_)) {
        let Mode::SchedulePaneInput(state) = std::mem::replace(&mut app.mode, Mode::Normal) else {
            unreachable!("just matched Mode::SchedulePaneInput above");
        };
        handle_scheduled_input_mouse(app, state, mouse);
        return;
    }
    if matches!(app.mode, Mode::QueuePrompt(_)) {
        let Mode::QueuePrompt(state) = std::mem::replace(&mut app.mode, Mode::Normal) else {
            unreachable!("just matched Mode::QueuePrompt above");
        };
        handle_prompt_queue_mouse(app, state, mouse);
        return;
    }
    if matches!(app.mode, Mode::EditorLineContextMenu(_)) {
        let Mode::EditorLineContextMenu(menu) = std::mem::replace(&mut app.mode, Mode::Normal)
        else {
            unreachable!("just matched Mode::EditorLineContextMenu above");
        };
        handle_editor_line_context_menu_mouse(app, menu, mouse);
        return;
    }
    if matches!(app.mode, Mode::CreateAgentFromLine(_)) {
        let Mode::CreateAgentFromLine(state) = std::mem::replace(&mut app.mode, Mode::Normal)
        else {
            unreachable!("just matched Mode::CreateAgentFromLine above");
        };
        handle_create_agent_from_line_mouse(app, state, mouse);
        return;
    }
    if matches!(app.mode, Mode::ExplorerFileMenu(_)) {
        let Mode::ExplorerFileMenu(menu) = std::mem::replace(&mut app.mode, Mode::Normal) else {
            unreachable!("just matched Mode::ExplorerFileMenu");
        };
        handle_explorer_file_menu_mouse(app, menu, mouse);
        return;
    }
    if matches!(app.mode, Mode::CreateGroup(_)) {
        let Mode::CreateGroup(state) = std::mem::replace(&mut app.mode, Mode::Normal) else {
            unreachable!("just matched Mode::CreateGroup above");
        };
        handle_create_group_mouse(app, state, mouse);
        return;
    }
    if matches!(app.mode, Mode::CreateSplitOrientation(_)) {
        let Mode::CreateSplitOrientation(state) = std::mem::replace(&mut app.mode, Mode::Normal)
        else {
            unreachable!("just matched Mode::CreateSplitOrientation above");
        };
        handle_create_split_orientation_mouse(app, state, mouse);
        return;
    }
    if matches!(app.mode, Mode::CreateSplitMembers(_)) {
        let Mode::CreateSplitMembers(state) = std::mem::replace(&mut app.mode, Mode::Normal) else {
            unreachable!("just matched Mode::CreateSplitMembers above");
        };
        handle_create_split_members_mouse(app, state, mouse);
        return;
    }
    if matches!(app.mode, Mode::CreateBoard(_)) {
        let Mode::CreateBoard(state) = std::mem::replace(&mut app.mode, Mode::Normal) else {
            unreachable!("just matched Mode::CreateBoard");
        };
        handle_create_board_mouse(app, state, mouse);
        return;
    }
    if matches!(
        app.mode,
        Mode::Explorer(..)
            | Mode::FolderExplorer(..)
            | Mode::ProjectFolderExplorer(..)
            | Mode::BoardPathPicker(..)
    ) {
        match std::mem::replace(&mut app.mode, Mode::Normal) {
            Mode::Explorer(overlay, target) => handle_explorer_mouse(app, overlay, target, mouse),
            Mode::FolderExplorer(overlay, target) => {
                handle_folder_explorer_mouse(app, overlay, target, mouse)
            }
            Mode::ProjectFolderExplorer(overlay, selection) => {
                handle_project_folder_explorer_mouse(app, overlay, selection, mouse)
            }
            Mode::BoardPathPicker(mut overlay, mut state) => {
                match overlay.handle(&Event::Mouse(mouse), app.layout.screen_area) {
                    Ok(Some(path)) => {
                        state.path =
                            crate::text_prompt::TextPromptState::new(path.display().to_string());
                        state.editing_path = true;
                        app.mode = Mode::CreateBoard(state);
                    }
                    Ok(None) => app.mode = Mode::BoardPathPicker(overlay, state),
                    Err(error) => {
                        app.status_message = Some(format!("Board path picker error: {error}"));
                        app.mode = Mode::BoardPathPicker(overlay, state);
                    }
                }
            }
            _ => unreachable!("folder explorer match must preserve its mode"),
        }
        return;
    }
    if matches!(app.mode, Mode::Settings(_)) {
        let Mode::Settings(state) = std::mem::replace(&mut app.mode, Mode::Normal) else {
            unreachable!("just matched Mode::Settings above");
        };
        handle_settings_mouse(app, state, mouse);
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
            | Mode::BoardCardPrompt(_, _)
            | Mode::BoardColumnPrompt(_, _)
            | Mode::BoardRenamePrompt(_, _, _)
            | Mode::BoardDeleteConfirm(_, _)
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

/// Mouse parity for the keyboard-first full-screen search screen: wheel
/// scrolls results, while clicking a result opens it immediately at its
/// recorded terminal/editor location.
fn handle_search_mouse(
    app: &mut App,
    state: &mut crate::search_ui::SearchState,
    mouse: MouseEvent,
) {
    match mouse.kind {
        MouseEventKind::ScrollUp => {
            state.move_selection(
                -3,
                crate::search_ui::visible_result_rows(app.layout.screen_area),
            );
            app.mode = Mode::Search(Box::new(std::mem::take(state)));
        }
        MouseEventKind::ScrollDown => {
            state.move_selection(
                3,
                crate::search_ui::visible_result_rows(app.layout.screen_area),
            );
            app.mode = Mode::Search(Box::new(std::mem::take(state)));
        }
        MouseEventKind::Down(MouseButton::Left) => {
            let position = Position::new(mouse.column, mouse.row);
            if let Some(index) =
                crate::search_ui::result_at(app.layout.screen_area, state, position)
            {
                state.selected_index = index;
                if let Some(result) = state.selected_result().cloned() {
                    app.activate_search_result(result);
                    return;
                }
            }
            app.mode = Mode::Search(Box::new(std::mem::take(state)));
        }
        _ => app.mode = Mode::Search(Box::new(std::mem::take(state))),
    }
}

fn handle_create_split_orientation_mouse(
    app: &mut App,
    state: crate::app::CreateSplitOrientationState,
    mouse: MouseEvent,
) {
    if !matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
        app.mode = Mode::CreateSplitOrientation(state);
        return;
    }
    let position = Position::new(mouse.column, mouse.row);
    let popup = crate::modal::create_split_orientation_dialog_area(app.layout.screen_area);
    let orientation = match mouse.row {
        row if row == popup.y.saturating_add(3) => Some(ilium_core::SplitOrientation::Vertical),
        row if row == popup.y.saturating_add(4) => Some(ilium_core::SplitOrientation::Horizontal),
        _ => None,
    };
    if let Some(orientation) = orientation {
        app.continue_create_split(orientation);
    } else if mouse.row == popup.y.saturating_add(6) && popup.contains(position) {
        app.commit_empty_split(state.orientation);
    } else {
        app.mode = Mode::CreateSplitOrientation(state);
    }
}

fn handle_create_split_members_mouse(
    app: &mut App,
    mut state: crate::app::CreateSplitMembersState,
    mouse: MouseEvent,
) {
    if !matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
        app.mode = Mode::CreateSplitMembers(state);
        return;
    }
    let position = Position::new(mouse.column, mouse.row);
    if let Some(index) = crate::modal::create_split_member_row_at(
        app.layout.screen_area,
        state.selected_index,
        state.choices.len(),
        position,
    ) {
        state.selected_index = index;
        app.toggle_create_split_member(&mut state);
        app.mode = Mode::CreateSplitMembers(state);
        return;
    }
    let popup =
        crate::modal::create_split_members_dialog_area(app.layout.screen_area, state.choices.len());
    if mouse.row == popup.bottom().saturating_sub(2) && popup.contains(position) {
        app.commit_create_split(state);
    } else {
        app.mode = Mode::CreateSplitMembers(state);
    }
}

/// Selects tree rows, opens the right-click menu, scrolls, and handles the
/// toolbar/row hover controls.
fn handle_tree_mouse(app: &mut App, mouse: MouseEvent, position: Position) {
    app.leave_pane_focus();
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
        if let Some(action) =
            tree_ui::row_action_at(&app.tree, hit.id, app.layout.tree_area, hit.row, position)
        {
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
                if let Some(entry) = tree_ui::folder_entry(&app.tree, hit.id) {
                    app.select_tree_path(entry.identifier_path);
                    if entry.is_directory {
                        app.toggle_selected_tree_node();
                    } else {
                        app.request_new_editor(
                            app.tree.parent_of(entry.root_id).unwrap_or(ROOT_ID),
                            entry.path,
                        );
                    }
                    return;
                }
                app.select_node(hit.id);
                app.set_drag_source(Some(hit.id));
                if app
                    .tree
                    .get(hit.id)
                    .is_some_and(ilium_core::Node::is_split_view)
                {
                    app.toggle_selected_tree_node();
                    app.show_split_view(hit.id);
                } else if matches!(
                    app.tree.get(hit.id).map(|node| &node.kind),
                    Some(NodeKind::Container(_) | NodeKind::Folder { .. })
                ) {
                    app.toggle_selected_tree_node();
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

fn handle_tree_row_action(app: &mut App, id: ilium_core::NodeId, action: TreeRowAction) {
    app.select_node(id);
    match action {
        TreeRowAction::Rename => app.action_start_rename(),
        TreeRowAction::MoveUp => app.request_move(id, ilium_core::TreeMoveDirection::Up),
        TreeRowAction::MoveDown => app.request_move(id, ilium_core::TreeMoveDirection::Down),
        TreeRowAction::Close => app.action_close_selected(),
        TreeRowAction::Retitle => app.action_request_retitle(id),
        TreeRowAction::ProjectRestructure => app.action_request_project_restructure(id),
    }
}

/// True if `ancestor` is `node` itself or a transitive parent of `node` --
/// a client-side mirror of `ilium_core::Tree`'s own (private) check of the
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
                Some(NodeKind::Container(container)) => {
                    if container.is_split_view()
                        && (!dragged_is_pane
                            || (tree.parent_of(dragged_id) != Some(target_id)
                                && container.children.len()
                                    >= ilium_core::MAXIMUM_SPLIT_VIEW_PANES))
                    {
                        return None;
                    }
                    (target_id, None)
                }
                Some(NodeKind::Pane { .. }) => {
                    let parent = tree.parent_of(target_id)?;
                    let siblings = tree.children_of(parent).ok()?;
                    let position = siblings.iter().position(|&sibling| sibling == target_id)?;
                    (parent, Some(position))
                }
                Some(NodeKind::Folder { .. }) => return None,
                None => return None,
            }
        }
    };

    if new_parent == ROOT_ID && !tree.get(dragged_id).is_some_and(|node| node.is_project()) {
        return None;
    }
    let dragged_project = tree.project_ancestor(dragged_id);
    let destination_project = tree.project_ancestor(new_parent);
    if dragged_project.is_some()
        && destination_project.is_some()
        && dragged_project != destination_project
    {
        return None;
    }
    Some((new_parent, index))
}

/// Executes a bottom-toolbar creation action. Agent entries create their
/// pane pre-loaded with the exact command line (`NewPaneKind::Command`),
/// which the server runs directly rather than typing it into an
/// interactive shell -- see `ilium_server::pane::TerminalOrigin::Command`.
fn execute_tree_toolbar_action(app: &mut App, action: TreeToolbarAction) {
    match action {
        TreeToolbarAction::Search => {
            app.action_open_search();
            return;
        }
        TreeToolbarAction::Group => {
            let preselected = app.create_group_preselect_target();
            app.open_create_group_dialog(preselected);
            return;
        }
        TreeToolbarAction::Project => {
            app.action_new_project();
            return;
        }
        TreeToolbarAction::Split => {
            app.open_create_split_dialog();
            return;
        }
        // `action_new_editor` only opens the file picker (or reports its
        // own failure via `status_message`); nothing is created yet, so it
        // must own the status message rather than have it clobbered below
        // by a premature "Created" success message.
        TreeToolbarAction::Editor => {
            app.action_new_editor();
            return;
        }
        TreeToolbarAction::Board => {
            app.open_create_board_dialog();
            return;
        }
        TreeToolbarAction::Folder => {
            app.action_new_folder();
            return;
        }
        TreeToolbarAction::Settings => {
            app.action_open_settings();
            return;
        }
        TreeToolbarAction::Restructure => {
            app.action_request_restructure();
            return;
        }
        TreeToolbarAction::Shell => app.action_new_terminal(),
        TreeToolbarAction::Agent(provider) => app.action_new_command_pane(provider.command_line()),
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

    if let Some(submenu) = &menu.tree_order_submenu {
        if submenu.area.contains(position) {
            let content_top = submenu.area.y.saturating_add(1);
            if position.y < content_top {
                app.mode = Mode::ContextMenu(menu);
                return;
            }
            let item_row = usize::from(position.y - content_top);
            let Some(tree_order) = crate::config::TreeOrder::ALL.get(item_row).copied() else {
                app.mode = Mode::ContextMenu(menu);
                return;
            };
            app.settings_set_tree_order(tree_order);
            app.mode = Mode::Normal;
            return;
        }
        if !menu.area.contains(position) {
            app.mode = Mode::Normal;
            return;
        }
    }
    if !menu.area.contains(position) {
        app.mode = Mode::Normal;
        return;
    }
    // The block's top border occupies `menu.area.y` itself, so the first
    // action row starts one line below it. A plain `saturating_sub` would
    // silently clamp a click on that border row to `0`, misattributing it
    // to the first action instead of treating it as a click on the frame.
    let content_top = menu.area.y.saturating_add(1);
    if position.y < content_top {
        app.mode = Mode::ContextMenu(menu);
        return;
    }
    let item_row = (position.y - content_top) as usize;
    if item_row >= menu.actions.len() {
        app.mode = Mode::ContextMenu(menu);
        return;
    }
    menu.selected_index = item_row;
    app.select_node(menu.target);
    let action = menu.actions[item_row];
    if action == crate::app::ContextMenuAction::OrderBy {
        app.open_context_tree_order_submenu(&mut menu);
        app.mode = Mode::ContextMenu(menu);
        return;
    }
    app.execute_context_action(action, menu.target);
}

/// Handles the source-line popup independently from tree selection state.
fn handle_editor_line_context_menu_mouse(
    app: &mut App,
    mut menu: EditorLineContextMenu,
    mouse: MouseEvent,
) {
    if !matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
        app.mode = Mode::EditorLineContextMenu(menu);
        return;
    }
    let position = Position::new(mouse.column, mouse.row);
    if !menu.area.contains(position) {
        app.mode = Mode::Normal;
        return;
    }
    let content_top = menu.area.y.saturating_add(1);
    if position.y < content_top {
        app.mode = Mode::EditorLineContextMenu(menu);
        return;
    }
    let item_row = usize::from(position.y - content_top);
    if item_row >= menu.actions.len() {
        app.mode = Mode::EditorLineContextMenu(menu);
        return;
    }
    menu.selected_index = item_row;
    app.execute_editor_line_context_action(menu.actions[item_row], menu.source);
}

/// Handles direct manipulation of the agent selector, textarea, and explicit
/// Create button. The layout comes from the same module the renderer uses.
fn handle_create_agent_from_line_mouse(
    app: &mut App,
    mut state: Box<CreateAgentFromLineState>,
    mouse: MouseEvent,
) {
    use ratatui_textarea::CursorMove;

    let layout = crate::agent_from_line::dialog_layout(app.layout.screen_area);
    let position = Position::new(mouse.column, mouse.row);
    if !matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
        app.mode = Mode::CreateAgentFromLine(state);
        return;
    }
    if !layout.popup.contains(position) {
        app.mode = Mode::Normal;
        return;
    }
    if let Some(agent_type) = crate::agent_from_line::agent_type_at(layout.agent_row, position) {
        state.agent_type = agent_type;
        state.focus = CreateAgentFocus::AgentType;
        app.mode = Mode::CreateAgentFromLine(state);
        return;
    }
    let prompt_inner = Rect::new(
        layout.prompt_area.x.saturating_add(1),
        layout.prompt_area.y.saturating_add(1),
        layout.prompt_area.width.saturating_sub(2),
        layout.prompt_area.height.saturating_sub(2),
    );
    if prompt_inner.contains(position) {
        let row = position.y.saturating_sub(prompt_inner.y);
        let column = position.x.saturating_sub(prompt_inner.x);
        state.prompt.move_cursor(CursorMove::Jump(row, column));
        state.focus = CreateAgentFocus::Prompt;
        app.mode = Mode::CreateAgentFromLine(state);
        return;
    }
    if layout.create_button.contains(position) {
        state.focus = CreateAgentFocus::CreateButton;
        app.commit_create_agent_from_line(state);
        return;
    }
    app.mode = Mode::CreateAgentFromLine(state);
}

/// Gives every visible form control a direct mouse target. Clicking outside
/// follows ilium's other creation dialogs and cancels; clicking a field moves
/// its cursor close to the selected cell before keyboard input resumes.
fn handle_scheduled_input_mouse(
    app: &mut App,
    mut state: Box<ScheduledInputDialogState>,
    mouse: MouseEvent,
) {
    if !matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
        app.mode = Mode::SchedulePaneInput(state);
        return;
    }
    let position = Position::new(mouse.column, mouse.row);
    let layout = crate::scheduled_input::dialog_layout(app.layout.screen_area);
    if !layout.popup.contains(position) {
        app.mode = Mode::Normal;
        return;
    }
    if layout.hours.contains(position) {
        state.focus = ScheduledInputFocus::Hours;
        place_prompt_cursor(&mut state.hours, layout.hours, position);
    } else if layout.minutes.contains(position) {
        state.focus = ScheduledInputFocus::Minutes;
        place_prompt_cursor(&mut state.minutes, layout.minutes, position);
    } else if layout.seconds.contains(position) {
        state.focus = ScheduledInputFocus::Seconds;
        place_prompt_cursor(&mut state.seconds, layout.seconds, position);
    } else if layout.text.contains(position) {
        state.focus = ScheduledInputFocus::Text;
        place_prompt_cursor(&mut state.text, layout.text, position);
    } else if layout.send_enter.contains(position) {
        state.focus = ScheduledInputFocus::SendEnter;
        state.send_enter = !state.send_enter;
    } else if layout.schedule_button.contains(position) {
        state.focus = ScheduledInputFocus::ScheduleButton;
        app.commit_scheduled_pane_input(state);
        return;
    }
    app.mode = Mode::SchedulePaneInput(state);
}

fn handle_prompt_queue_mouse(
    app: &mut App,
    mut state: Box<PromptQueueDialogState>,
    mouse: MouseEvent,
) {
    if !matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
        app.mode = Mode::QueuePrompt(state);
        return;
    }
    let position = Position::new(mouse.column, mouse.row);
    let layout = crate::prompt_queue::dialog_layout(app.layout.screen_area);
    if !layout.popup.contains(position) {
        app.mode = Mode::Normal;
        return;
    }
    if layout.text.contains(position) {
        state.focus = PromptQueueFocus::Text;
    } else if layout.delivery.contains(position) {
        state.focus = PromptQueueFocus::Delivery;
        state.cycle_delivery(1);
    } else if layout.times.contains(position) {
        state.focus = PromptQueueFocus::Times;
    } else if layout.enqueue_button.contains(position) {
        state.focus = PromptQueueFocus::EnqueueButton;
        app.commit_queued_prompt(state);
        return;
    }
    app.mode = Mode::QueuePrompt(state);
}

fn place_prompt_cursor(
    prompt: &mut crate::text_prompt::TextPromptState,
    field_area: Rect,
    position: Position,
) {
    let inner_x = field_area.x.saturating_add(1);
    let clicked_offset = usize::from(position.x.saturating_sub(inner_x));
    prompt.cursor = clicked_offset.min(prompt.buf.chars().count());
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

/// The board dialog keeps text entry keyboard-first, while its explicit
/// Browse button opens the same mouse-capable picker used elsewhere.
fn handle_create_board_mouse(
    app: &mut App,
    state: crate::app::CreateBoardState,
    mouse: MouseEvent,
) {
    if !matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
        app.mode = Mode::CreateBoard(state);
        return;
    }
    let popup = crate::modal::centered_fixed_rect(68, 11, app.layout.screen_area);
    let browse_row = popup.y.saturating_add(4);
    if mouse.row == browse_row && mouse.column >= popup.x.saturating_add(1) {
        app.open_board_path_picker(state);
        return;
    }
    if popup.contains(Position::new(mouse.column, mouse.row)) {
        app.mode = Mode::CreateBoard(state);
    } else {
        app.mode = Mode::Normal;
    }
}

/// Rows scrolled per wheel notch over the settings screen's content panel --
/// matches `App`'s own `TERMINAL_WHEEL_SCROLL_LINES`/`tree_state.scroll_up(3)`
/// per-notch amount elsewhere in this crate.
const SETTINGS_WHEEL_SCROLL_LINES: u16 = 3;

/// Mouse handling for the full-screen settings view (`Mode::Settings`):
/// clicking the header's close button closes the screen, clicking a tab
/// switches to it, clicking a row's `‹`/value control decrements/increments
/// it, and the wheel scrolls the content panel -- see `crate::settings_ui`'s
/// module doc comment for the shared layout/hit-test functions this
/// reproduces no arithmetic of its own from.
fn handle_settings_mouse(app: &mut App, mut state: crate::app::SettingsState, mouse: MouseEvent) {
    let position = Position::new(mouse.column, mouse.row);
    let layout = crate::settings_ui::compute_layout(app.layout.screen_area);

    if let Some(action) = state.keyboard_picker.take() {
        if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
            let available = crate::keymap::available_keys(&app.keybindings);
            if let Some(key) = crate::settings_ui::keyboard_picker_key_at(
                app.layout.screen_area,
                position,
                &available,
            ) {
                app.settings_assign_key(action, key);
                state.keyboard_picker = None;
            } else {
                state.keyboard_picker = Some(action);
            }
        } else {
            state.keyboard_picker = Some(action);
        }
        app.mode = Mode::Settings(state);
        return;
    }

    if let Some(picker) = state.icon_picker.take() {
        if matches!(
            mouse.kind,
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp
        ) {
            let next_scroll = if matches!(mouse.kind, MouseEventKind::ScrollDown) {
                picker.scroll_row.saturating_add(3)
            } else {
                picker.scroll_row.saturating_sub(3)
            };
            let columns = crate::settings_ui::icon_picker_grid_columns(
                app.layout.screen_area,
                picker.column_mode,
            );
            let visible_rows = usize::from(
                crate::settings_ui::icon_picker_layout(app.layout.screen_area)
                    .document_area
                    .height,
            )
            .max(1);
            let total_rows =
                crate::settings_ui::icon_picker_document_row_count(&picker.search_results, columns);
            state.icon_picker = Some(crate::app::IconPickerState {
                scroll_row: next_scroll.min(total_rows.saturating_sub(visible_rows)),
                ..picker
            });
            app.mode = Mode::Settings(state);
            return;
        }
        let next_picker = if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
            match crate::settings_ui::icon_picker_hit(app.layout.screen_area, &picker, position) {
                Some(crate::settings_ui::IconPickerHit::Close) => None,
                Some(crate::settings_ui::IconPickerHit::ToggleColumnMode) => {
                    let mut next_picker = picker;
                    next_picker.column_mode = next_picker.column_mode.toggle();
                    next_picker.scroll_row = crate::settings_ui::icon_picker_scroll_for_entry(
                        app.layout.screen_area,
                        &next_picker,
                    );
                    Some(next_picker)
                }
                Some(crate::settings_ui::IconPickerHit::ActivateSearch) => {
                    Some(crate::app::IconPickerState {
                        is_searching: true,
                        ..picker
                    })
                }
                Some(crate::settings_ui::IconPickerHit::ScrollTo(scroll_row)) => {
                    Some(crate::app::IconPickerState {
                        scroll_row,
                        ..picker
                    })
                }
                Some(crate::settings_ui::IconPickerHit::Entry(entry_index)) => {
                    if let Some(entry) = picker.search_results.entry(entry_index) {
                        app.settings_set_icon(picker.target, entry.glyph.to_string());
                        None
                    } else {
                        Some(picker)
                    }
                }
                None => Some(picker),
            }
        } else {
            Some(picker)
        };
        state.icon_picker = next_picker;
        app.mode = Mode::Settings(state);
        return;
    }

    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            if crate::settings_ui::close_button_hit(layout.header_area, position) {
                app.mode = Mode::Normal;
                return;
            }
            if let Some(tab) = crate::settings_ui::tab_at(layout.tab_list_area, position) {
                if tab != state.tab {
                    state.tab = tab;
                    state.selected_row = 0;
                    state.scroll = 0;
                }
            } else if state.tab == crate::app::SettingsTab::Inference {
                if let Some((row, direction)) = crate::settings_ui::inference_content_hit(
                    layout.content_area,
                    state.scroll,
                    position,
                    &app.inference_settings,
                ) {
                    if let Some(index) = crate::settings_ui::inference_rows(&app.inference_settings)
                        .iter()
                        .position(|candidate| *candidate == row)
                    {
                        state.selected_row = index;
                    }
                    match row {
                        crate::app::InferenceRow::Provider => {
                            app.settings_adjust_inference_provider(direction)
                        }
                        crate::app::InferenceRow::RefreshOllamaModels => {
                            app.request_ollama_model_refresh()
                        }
                        crate::app::InferenceRow::Field(
                            crate::app::InferenceSettingField::OllamaModel,
                        ) => app.settings_adjust_ollama_model(direction),
                        crate::app::InferenceRow::Field(field) => {
                            app.settings_open_inference_field(field);
                            return;
                        }
                        crate::app::InferenceRow::Test => app.request_inference_test(),
                    }
                }
            } else if state.tab == crate::app::SettingsTab::Icons {
                if let Some(real_tree) =
                    crate::settings_ui::icons_preview_mode_hit(layout.content_area, position)
                {
                    state.icons_preview_real = real_tree;
                } else if let Some(hit) =
                    crate::settings_ui::icons_table_hit(layout.content_area, state.scroll, position)
                {
                    state.selected_row = crate::icon_settings::IconTarget::ALL
                        .iter()
                        .position(|candidate| *candidate == hit.target)
                        .unwrap_or(0);
                    match hit.action {
                        crate::settings_ui::IconTableAction::OpenCatalogue => {
                            state.icon_picker = Some(crate::app::IconPickerState::new(hit.target));
                        }
                        crate::settings_ui::IconTableAction::CycleSuggestion => {
                            app.settings_cycle_icon(hit.target, 1);
                        }
                    }
                }
            } else if state.tab == crate::app::SettingsTab::VoiceControl {
                if let Some((index, direction)) = crate::settings_ui::simple_content_hit(
                    layout.content_area,
                    state.scroll,
                    position,
                    crate::voice_settings::VoiceRow::ALL.len(),
                ) {
                    state.selected_row = index;
                    if let Some(row) = crate::voice_settings::VoiceRow::ALL.get(index).copied() {
                        app.settings_adjust_voice_row(row, direction);
                        if !matches!(app.mode, Mode::Normal) {
                            return;
                        }
                    }
                }
            } else if state.tab == crate::app::SettingsTab::Appearance {
                if let Some((row, direction)) = crate::settings_ui::appearance_content_hit(
                    layout.content_area,
                    state.scroll,
                    position,
                ) {
                    if let Some(index) = crate::app::AppearanceRow::ALL
                        .iter()
                        .position(|candidate| *candidate == row)
                    {
                        state.selected_row = index;
                    }
                    app.settings_adjust_row(row, direction);
                }
            } else if state.tab == crate::app::SettingsTab::Terminal {
                if let Some((index, direction)) = crate::settings_ui::simple_content_hit(
                    layout.content_area,
                    state.scroll,
                    position,
                    crate::app::TerminalRow::ALL.len(),
                ) {
                    state.selected_row = index;
                    if let Some(row) = crate::app::TerminalRow::ALL.get(index).copied() {
                        app.settings_adjust_terminal_row(row, direction);
                    }
                }
            } else if state.tab == crate::app::SettingsTab::Editor {
                if let Some((index, direction)) = crate::settings_ui::simple_content_hit(
                    layout.content_area,
                    state.scroll,
                    position,
                    crate::app::EditorRow::ALL.len(),
                ) {
                    state.selected_row = index;
                    if let Some(row) = crate::app::EditorRow::ALL.get(index).copied() {
                        app.settings_adjust_editor_row(row, direction);
                    }
                }
            } else if state.tab == crate::app::SettingsTab::Session {
                if let Some((index, direction)) = crate::settings_ui::simple_content_hit(
                    layout.content_area,
                    state.scroll,
                    position,
                    crate::app::SessionRow::ALL.len(),
                ) {
                    state.selected_row = index;
                    if let Some(row) = crate::app::SessionRow::ALL.get(index).copied() {
                        app.settings_adjust_session_row(row, direction);
                    }
                }
            } else if state.tab == crate::app::SettingsTab::Keyboard {
                if let Some(preset) = crate::settings_ui::keyboard_keymap_preset_at(
                    layout.content_area,
                    state.scroll,
                    position,
                ) {
                    app.settings_apply_keymap_preset(preset);
                } else if let Some(direction) = crate::settings_ui::keyboard_content_hit(
                    layout.content_area,
                    state.scroll,
                    position,
                ) {
                    app.settings_adjust_shortcut_base(direction);
                } else if let Some(action) = crate::settings_ui::keyboard_table_hit(
                    layout.content_area,
                    state.scroll,
                    position,
                    &app.keybindings,
                ) {
                    match action {
                        crate::settings_ui::KeyboardTableAction::Assign(key) => {
                            let row = position
                                .y
                                .saturating_sub(layout.content_area.y)
                                .saturating_add(state.scroll);
                            let index = usize::from(
                                row.saturating_sub(crate::settings_ui::KEYBOARD_TABLE_FIRST_ROW),
                            );
                            if let Some(binding) = app.keybindings.get(index) {
                                app.settings_assign_key(binding.action, key);
                            }
                        }
                        crate::settings_ui::KeyboardTableAction::OpenPicker(action) => {
                            state.keyboard_picker = Some(action);
                        }
                    }
                }
            } else if state.tab == crate::app::SettingsTab::KanbanBoard {
                if let Some((row, direction)) = crate::settings_ui::kanban_board_content_hit(
                    layout.content_area,
                    state.scroll,
                    position,
                ) {
                    if let Some(index) = crate::app::KanbanBoardRow::ALL
                        .iter()
                        .position(|candidate| *candidate == row)
                    {
                        state.selected_row = index;
                    }
                    app.settings_adjust_kanban_board_row(row, direction);
                }
            } else if state.tab == crate::app::SettingsTab::Sound {
                match crate::settings_ui::sound_content_hit(
                    layout.content_area,
                    state.scroll,
                    position,
                    &app.sound_discovery,
                ) {
                    Some(crate::settings_ui::SoundContentHit::Row { row, direction }) => {
                        if let Some(index) = crate::app::SoundRow::ALL
                            .iter()
                            .position(|candidate| *candidate == row)
                        {
                            state.selected_row = index;
                        }
                        app.settings_adjust_sound_row(row, direction);
                    }
                    Some(crate::settings_ui::SoundContentHit::File(index)) => {
                        app.settings_select_sound_file(index);
                    }
                    None => {}
                }
            }
        }
        MouseEventKind::ScrollUp => {
            state.scroll = state.scroll.saturating_sub(SETTINGS_WHEEL_SCROLL_LINES);
        }
        MouseEventKind::ScrollDown => {
            state.scroll = state.scroll.saturating_add(SETTINGS_WHEEL_SCROLL_LINES);
        }
        _ => {}
    }

    let max_scroll =
        crate::settings_ui::max_scroll(state.tab, app, state.selected_row, layout.content_area);
    state.scroll = state.scroll.min(max_scroll);
    app.mode = Mode::Settings(state);
}

/// Routes a mouse event to the open file picker overlay -- the same
/// row-select/scroll/click-to-activate hit-testing it already owns.
fn handle_explorer_mouse(
    app: &mut App,
    mut overlay: Box<ExplorerOverlay>,
    target: ilium_core::NodeId,
    mouse: MouseEvent,
) {
    if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Right)) {
        if let Some(path) = overlay.file_at_mouse(mouse, app.layout.screen_area) {
            if crate::editor_pane::is_markdown_path(&path) {
                app.open_explorer_file_menu(
                    overlay,
                    target,
                    path,
                    Position::new(mouse.column, mouse.row),
                );
                return;
            }
        }
    }
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

fn handle_explorer_file_menu_mouse(
    app: &mut App,
    menu: crate::app::ExplorerFileMenu,
    mouse: MouseEvent,
) {
    if !matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
        app.mode = Mode::ExplorerFileMenu(menu);
        return;
    }
    if menu.area.contains(Position::new(mouse.column, mouse.row))
        && mouse.row == menu.area.y.saturating_add(1)
    {
        app.request_new_markdown_board(menu.target_group, menu.file_path);
        app.mode = Mode::Normal;
    } else {
        app.mode = Mode::Explorer(menu.overlay, menu.target_group);
    }
}

fn handle_folder_explorer_mouse(
    app: &mut App,
    mut overlay: Box<ExplorerOverlay>,
    target: ilium_core::NodeId,
    mouse: MouseEvent,
) {
    match overlay.handle(&Event::Mouse(mouse), app.layout.screen_area) {
        Ok(Some(path)) => {
            app.request_new_folder(target, path);
            app.mode = Mode::Normal;
        }
        Ok(None) => app.mode = Mode::FolderExplorer(overlay, target),
        Err(err) => {
            app.status_message = Some(format!("Folder picker error: {err}"));
            app.mode = Mode::FolderExplorer(overlay, target);
        }
    }
}

fn handle_project_folder_explorer_mouse(
    app: &mut App,
    mut overlay: Box<crate::explorer_overlay::ExplorerOverlay>,
    selection: crate::app::ProjectFolderSelection,
    mouse: MouseEvent,
) {
    match overlay.handle(&Event::Mouse(mouse), app.layout.screen_area) {
        Ok(Some(path)) => {
            match selection {
                crate::app::ProjectFolderSelection::NewProject => app.request_new_project(path),
                crate::app::ProjectFolderSelection::ChangeProject(project_id) => {
                    app.request_change_project_folder(project_id, path)
                }
            }
            app.mode = Mode::Normal;
        }
        Ok(None) => app.mode = Mode::ProjectFolderExplorer(overlay, selection),
        Err(err) => {
            app.status_message = Some(format!("Project picker error: {err}"));
            app.mode = Mode::ProjectFolderExplorer(overlay, selection);
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
            .add_pane(group_a, "b", ilium_core::PaneContentKind::Terminal)
            .unwrap();
        let group_c = tree.add_group(ROOT_ID, "c").unwrap();
        let pane_d = tree
            .add_pane(group_c, "d", ilium_core::PaneContentKind::Terminal)
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
    fn dropping_a_project_in_empty_space_appends_at_the_top_level() {
        let (mut tree, _group_a, _pane_b, _group_c, _pane_d) = sample_tree();
        let project = tree
            .add_project(std::path::PathBuf::from("/tmp/project"))
            .unwrap();
        assert_eq!(
            compute_drop_target(&tree, project, None),
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

#[cfg(test)]
mod agent_from_line_mouse_tests {
    use super::*;
    use crate::agent_from_line::{CreateAgentFromLineState, EditorSourceLine};
    use crate::app::App;
    use crossterm::event::KeyModifiers;
    use ilium_core::PaneContentKind;
    use std::path::PathBuf;

    #[test]
    fn clicking_create_submits_the_dialog_prompt() {
        let mut app = App::new("test-session".to_string(), PathBuf::from("/tmp"));
        app.set_screen_area(Rect::new(0, 0, 120, 40));
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        let editor_id = app
            .tree
            .add_pane(group, "main.rs", PaneContentKind::Editor)
            .unwrap();
        app.mode = Mode::CreateAgentFromLine(Box::new(CreateAgentFromLineState::new(
            EditorSourceLine {
                pane_id: editor_id,
                path: PathBuf::from("/work/main.rs"),
                line_number: 3,
                text: "do_work();".to_string(),
            },
            group,
        )));
        app.take_outbound_requests();
        let layout = crate::agent_from_line::dialog_layout(app.layout.screen_area);

        handle_mouse_event(
            &mut app,
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: layout.create_button.x + layout.create_button.width / 2,
                row: layout.create_button.y,
                modifiers: KeyModifiers::NONE,
            },
        );

        assert!(matches!(app.mode, Mode::Normal));
        assert!(matches!(
            app.take_outbound_requests().as_slice(),
            [ilium_ipc::ClientRequest::NewPane {
                parent_group,
                kind: ilium_ipc::NewPaneKind::CommandWithInitialInput {
                    command_line,
                    initial_input,
                },
                ..
            }] if *parent_group == group
                && command_line == "claude"
                && initial_input.contains("do_work();")
        ));
    }
}

#[cfg(test)]
mod scheduled_input_mouse_tests {
    use super::*;
    use crossterm::event::KeyModifiers;
    use std::path::PathBuf;

    fn click(app: &mut App, area: Rect) {
        handle_mouse_event(
            app,
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: area.x + area.width.saturating_sub(1) / 2,
                row: area.y,
                modifiers: KeyModifiers::NONE,
            },
        );
    }

    #[test]
    fn scheduled_input_checkbox_and_button_have_real_mouse_targets() {
        let mut app = App::new("test-session".to_string(), PathBuf::from("/tmp"));
        app.set_screen_area(Rect::new(0, 0, 100, 30));
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        let pane_id = app
            .tree
            .add_pane(group, "shell", ilium_core::PaneContentKind::Terminal)
            .unwrap();
        app.mode = Mode::SchedulePaneInput(Box::new(ScheduledInputDialogState::new(pane_id)));
        let layout = crate::scheduled_input::dialog_layout(app.layout.screen_area);

        click(&mut app, layout.send_enter);

        let Mode::SchedulePaneInput(state) = &mut app.mode else {
            panic!("checkbox click should keep the form open");
        };
        assert!(!state.send_enter);
        assert_eq!(state.focus, ScheduledInputFocus::SendEnter);
        state.text = crate::text_prompt::TextPromptState::new("status");

        click(&mut app, layout.schedule_button);

        assert_eq!(
            app.take_outbound_requests(),
            vec![ilium_ipc::ClientRequest::SchedulePaneInput {
                pane_id,
                delay_seconds: 30,
                text: "status".to_string(),
                send_enter: false,
            }]
        );
    }
}

#[cfg(test)]
mod tree_order_context_mouse_tests {
    use super::*;
    use crossterm::event::KeyModifiers;
    use std::path::PathBuf;

    fn click(app: &mut App, column: u16, row: u16) {
        handle_mouse_event(
            app,
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column,
                row,
                modifiers: KeyModifiers::NONE,
            },
        );
    }

    #[test]
    fn context_menu_mouse_opens_order_submenu_and_applies_clicked_mode() {
        let mut app = App::new("test-session".to_string(), PathBuf::from("/tmp"));
        app.set_screen_area(Rect::new(0, 0, 100, 30));
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        let pane = app
            .tree
            .add_pane(group, "shell", ilium_core::PaneContentKind::Terminal)
            .unwrap();
        app.open_context_menu(pane, 2, 2);
        let (order_column, order_row) = match &app.mode {
            Mode::ContextMenu(menu) => {
                let index = menu
                    .actions
                    .iter()
                    .position(|action| *action == crate::app::ContextMenuAction::OrderBy)
                    .unwrap();
                (menu.area.x + 1, menu.area.y + 1 + index as u16)
            }
            _ => panic!("context menu should be open"),
        };

        click(&mut app, order_column, order_row);
        let (submenu_column, submenu_row) = match &app.mode {
            Mode::ContextMenu(menu) => {
                let submenu = menu
                    .tree_order_submenu
                    .as_ref()
                    .expect("Order by click should open its submenu");
                let index = crate::config::TreeOrder::ALL
                    .iter()
                    .position(|tree_order| *tree_order == crate::config::TreeOrder::NameAscending)
                    .unwrap();
                (submenu.area.x + 1, submenu.area.y + 1 + index as u16)
            }
            _ => panic!("parent context menu should remain open"),
        };

        click(&mut app, submenu_column, submenu_row);

        assert_eq!(
            app.ui_settings.tree_order,
            crate::config::TreeOrder::NameAscending
        );
        assert!(matches!(app.mode, Mode::Normal));
    }
}

#[cfg(test)]
mod markdown_board_context_mouse_tests {
    use super::*;
    use crossterm::event::KeyModifiers;
    use std::path::PathBuf;

    #[test]
    fn right_clicking_a_markdown_editor_row_exposes_and_runs_create_board() {
        let path = std::env::temp_dir().join(format!(
            "ilium-board-mouse-{}-{}.md",
            std::process::id(),
            crate::scheduled_input::unix_millis_now()
        ));
        std::fs::write(&path, "# Work\n\n* [ ] Mouse task\n").unwrap();

        let mut app = App::new("test-session".to_string(), PathBuf::from("/tmp"));
        app.set_screen_area(Rect::new(0, 0, 120, 40));
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        let editor_id = app
            .tree
            .add_pane(group, "work.md", ilium_core::PaneContentKind::Editor)
            .unwrap();
        let editor = crate::editor_pane::EditorPane::load(path.clone()).unwrap();
        app.panes
            .insert(editor_id, crate::app::PaneRuntime::Editor(Box::new(editor)));
        app.tree_state.open(vec![group]);
        let tree_area = app.layout.tree_area;
        let editor_row = (tree_area.y..tree_area.bottom())
            .find(|row| {
                app.tree_node_at(Position::new(tree_area.x + 1, *row))
                    .is_some_and(|hit| hit.id == editor_id)
            })
            .expect("editor row should be visible");

        handle_mouse_event(
            &mut app,
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Right),
                column: tree_area.x + 1,
                row: editor_row,
                modifiers: KeyModifiers::NONE,
            },
        );

        let (action_column, action_row) = match &app.mode {
            Mode::ContextMenu(menu) => {
                assert_eq!(menu.target, editor_id);
                let action_index = menu
                    .actions
                    .iter()
                    .position(|action| {
                        *action == crate::app::ContextMenuAction::CreateBoardFromMarkdown
                    })
                    .expect("Markdown editor menu should contain create-board action");
                (
                    menu.area.x + 1,
                    menu.area.y + 1 + u16::try_from(action_index).unwrap(),
                )
            }
            _ => panic!("right click should open the tree context menu"),
        };
        handle_mouse_event(
            &mut app,
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: action_column,
                row: action_row,
                modifiers: KeyModifiers::NONE,
            },
        );

        assert!(matches!(
            app.take_outbound_requests().as_slice(),
            [ilium_ipc::ClientRequest::NewBoard {
                parent_group,
                storage: ilium_core::BoardStorage::MarkdownFile { path: board_path },
                ..
            }] if *parent_group == group && board_path == &path
        ));
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(test)]
mod row_action_click_tests {
    use super::*;
    use crate::app::App;
    use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
    use std::path::PathBuf;

    /// Locates `id`'s on-screen row (via the same hit-test path rendering
    /// uses) and clicks the row-action strip's `Retitle` slot on it, through
    /// the real `handle_mouse_event` mouse
    /// pipeline -- a `Moved` event to set hover, then a `Down` click,
    /// mirroring what crossterm actually delivers.
    fn click_retitle_slot(app: &mut App, id: ilium_core::NodeId) {
        let area = app.layout.tree_area;
        let row = (area.y..area.bottom())
            .find(|&y| {
                app.tree_node_at(ratatui::layout::Position::new(area.x + 1, y))
                    .is_some_and(|hit| hit.id == id)
            })
            .expect("row must be visible in the rendered tree list");

        let list = tree_ui::list_area(area);
        let controls_start = list.right() - tree_ui::ROW_ACTION_WIDTH * tree_ui::ROW_ACTION_COUNT;
        let retitle_index = TreeRowAction::ALL
            .iter()
            .position(|action| *action == TreeRowAction::Retitle)
            .expect("retitle action is registered") as u16;
        let click_pos = ratatui::layout::Position::new(
            controls_start + retitle_index * tree_ui::ROW_ACTION_WIDTH,
            row,
        );

        for kind in [
            MouseEventKind::Moved,
            MouseEventKind::Down(MouseButton::Left),
        ] {
            handle_mouse_event(
                app,
                MouseEvent {
                    kind,
                    column: click_pos.x,
                    row: click_pos.y,
                    modifiers: KeyModifiers::empty(),
                },
            );
        }
    }

    fn test_app() -> App {
        let mut app = App::new("test-session".to_string(), PathBuf::from("/tmp"));
        app.set_screen_area(ratatui::layout::Rect::new(0, 0, 120, 40));
        app
    }

    #[test]
    fn bottom_right_voice_control_toggles_before_full_screen_mode_dispatch() {
        let mut app = test_app();
        app.mode = Mode::Settings(crate::app::SettingsState::new());
        let area = app.layout.voice_control_area;

        handle_mouse_event(
            &mut app,
            MouseEvent {
                kind: MouseEventKind::Up(MouseButton::Left),
                column: area.x,
                row: area.y,
                modifiers: KeyModifiers::NONE,
            },
        );

        assert!(app.voice_settings.enabled);
        assert!(matches!(app.mode, Mode::Settings(_)));
        assert_eq!(
            app.take_voice_runtime_request(),
            Some(crate::app::VoiceRuntimeRequest::Start)
        );
    }

    #[test]
    fn clicking_retitle_on_a_plain_shell_row_queues_a_retitle_request() {
        let mut app = test_app();
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        let pane_id = app
            .tree
            .add_pane(group, "shell", ilium_core::PaneContentKind::Terminal)
            .unwrap();
        app.panes.insert(
            pane_id,
            crate::app::PaneRuntime::Terminal(Box::new(crate::terminal_view::TerminalView::new(
                24, 80,
            ))),
        );
        app.tree_state.open(vec![group]);

        click_retitle_slot(&mut app, pane_id);

        assert_eq!(app.status_message, None);
        assert_eq!(app.take_pending_retitle_requests().len(), 1);
    }

    #[test]
    fn clicking_the_retitle_slot_on_a_group_row_is_a_no_op() {
        let mut app = test_app();
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();

        click_retitle_slot(&mut app, group);

        assert_eq!(app.take_pending_retitle_requests().len(), 0);
    }

    #[test]
    fn clicking_the_retitle_slot_on_an_editor_row_is_a_no_op() {
        let mut app = test_app();
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        let editor_id = app
            .tree
            .add_pane(group, "notes.md", ilium_core::PaneContentKind::Editor)
            .unwrap();
        app.tree_state.open(vec![group]);

        click_retitle_slot(&mut app, editor_id);

        assert_eq!(app.take_pending_retitle_requests().len(), 0);
    }

    #[test]
    fn settings_toolbar_press_and_release_keep_the_settings_view_open() {
        let mut app = test_app();
        let settings_area = tree_ui::toolbar_button_rects(app.layout.tree_area)
            .into_iter()
            .find_map(|(action, area)| (action == TreeToolbarAction::Settings).then_some(area))
            .expect("settings button should fit in the default tree toolbar");
        let settings_position = Position::new(settings_area.x, settings_area.y);

        handle_mouse_event(
            &mut app,
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: settings_position.x,
                row: settings_position.y,
                modifiers: KeyModifiers::NONE,
            },
        );

        assert!(matches!(app.mode, Mode::Settings(_)));

        handle_mouse_event(
            &mut app,
            MouseEvent {
                kind: MouseEventKind::Up(MouseButton::Left),
                column: settings_position.x,
                row: settings_position.y,
                modifiers: KeyModifiers::NONE,
            },
        );

        assert!(matches!(app.mode, Mode::Settings(_)));
    }

    #[test]
    fn search_toolbar_action_opens_the_full_screen_workspace_finder() {
        let mut app = test_app();
        execute_tree_toolbar_action(&mut app, TreeToolbarAction::Search);
        assert!(matches!(app.mode, Mode::Search(_)));
    }

    #[test]
    fn antigravity_toolbar_action_launches_its_registered_command() {
        let mut app = test_app();

        execute_tree_toolbar_action(
            &mut app,
            TreeToolbarAction::Agent(ilium_core::BuiltinAgentProvider::Antigravity),
        );

        assert!(matches!(
            app.take_outbound_requests().as_slice(),
            [ilium_ipc::ClientRequest::NewPane {
                kind: ilium_ipc::NewPaneKind::Command(command_line),
                ..
            }] if command_line == "agy"
        ));
    }
}

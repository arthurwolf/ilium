//! Mouse dispatch: routes crossterm mouse events according to
//! `App::mode`/the last-rendered layout, mirroring the pre-client/server
//! `App::handle_mouse_event`'s structure. Terminal-pane clicks/drags become
//! a queued `MouseInput` request (see `to_ipc_mouse_event`); tree-panel
//! selection, hover, and modal/editor-chrome interaction stay purely local,
//! except for a tree-row drag-and-drop, which queues a `ReparentNode`
//! request (see `compute_drop_target`) the same way any other structural
//! tree edit does.

use crossterm::event::{Event, MouseButton, MouseEvent, MouseEventKind};
use ilium_core::{NodeId, NodeKind, Tree, ROOT_ID};
use ratatui::layout::Position;

use crate::app::{App, ContextMenu, CreateGroupState, Mode};
use crate::explorer_overlay::ExplorerOverlay;
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
    if matches!(app.mode, Mode::CreateBoard(_)) {
        let Mode::CreateBoard(state) = std::mem::replace(&mut app.mode, Mode::Normal) else {
            unreachable!("just matched Mode::CreateBoard");
        };
        handle_create_board_mouse(app, state, mouse);
        return;
    }
    if matches!(
        app.mode,
        Mode::Explorer(..) | Mode::FolderExplorer(..) | Mode::BoardPathPicker(..)
    ) {
        match std::mem::replace(&mut app.mode, Mode::Normal) {
            Mode::Explorer(overlay, target) => handle_explorer_mouse(app, overlay, target, mouse),
            Mode::FolderExplorer(overlay, target) => {
                handle_folder_explorer_mouse(app, overlay, target, mouse)
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
                if let Some((folder_id, path, is_dir)) =
                    tree_ui::folder_entry_path(&app.tree, hit.id)
                {
                    app.select_node(hit.id);
                    if is_dir {
                        app.tree_state.toggle_selected();
                    } else {
                        app.request_new_editor(
                            app.tree.parent_of(folder_id).unwrap_or(ROOT_ID),
                            path,
                        );
                    }
                    return;
                }
                app.select_node(hit.id);
                app.set_drag_source(Some(hit.id));
                if app.tree.get(hit.id).is_some_and(ilium_core::Node::is_split_view) {
                    app.tree_state.toggle_selected();
                    app.show_split_view(hit.id);
                } else if matches!(
                    app.tree.get(hit.id).map(|node| &node.kind),
                    Some(NodeKind::Container(_) | NodeKind::Folder { .. })
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

fn handle_tree_row_action(app: &mut App, id: ilium_core::NodeId, action: TreeRowAction) {
    app.select_node(id);
    match action {
        TreeRowAction::Rename => app.action_start_rename(),
        TreeRowAction::MoveUp => app.request_move(id, ilium_core::TreeMoveDirection::Up),
        TreeRowAction::MoveDown => app.request_move(id, ilium_core::TreeMoveDirection::Down),
        TreeRowAction::Close => app.action_close_selected(),
        TreeRowAction::Retitle => app.action_request_retitle(id),
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
                Some(NodeKind::Container(_)) => (target_id, None),
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

    if new_parent == ROOT_ID && dragged_is_pane {
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
        TreeToolbarAction::Group => {
            let preselected = app.create_group_preselect_target();
            app.open_create_group_dialog(preselected);
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
        TreeToolbarAction::Shell => app.action_new_terminal(),
        TreeToolbarAction::Claude => app.action_new_command_pane("claude"),
        TreeToolbarAction::Codex => app.action_new_command_pane("codex"),
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

    let max_scroll = crate::settings_ui::max_scroll(
        state.tab,
        &app.ui_settings,
        state.selected_row,
        layout.content_area,
    );
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
            if path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
            {
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

#[cfg(test)]
mod row_action_click_tests {
    use super::*;
    use crate::app::App;
    use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
    use std::path::PathBuf;

    /// Locates `id`'s on-screen row (via the same hit-test path rendering
    /// uses) and clicks the row-action strip's `Retitle` slot (index 4,
    /// the rightmost) on it, through the real `handle_mouse_event` mouse
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
        let controls_start = list.right() - ROW_ACTION_WIDTH_FOR_TESTS;
        let click_pos = ratatui::layout::Position::new(controls_start + 4 * 2, row);

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

    // Mirrors `tree_ui`'s private `ROW_ACTION_WIDTH * ROW_ACTION_COUNT`.
    const ROW_ACTION_WIDTH_FOR_TESTS: u16 = 2 * 5;

    fn test_app() -> App {
        let mut app = App::new("test-session".to_string(), PathBuf::from("/tmp"));
        app.set_screen_area(ratatui::layout::Rect::new(0, 0, 120, 40));
        app
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
}

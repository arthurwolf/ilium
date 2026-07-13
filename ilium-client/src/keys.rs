//! Keyboard dispatch: routes crossterm key events according to `App::mode`,
//! mirroring the pre-client/server `App::handle_event`'s per-mode structure
//! but translating every structural mutation into a queued `ClientRequest`
//! (see `app.rs`'s module docs) instead of a direct tree edit.

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind};
use ilium_core::{NodeId, NodeKind, Tree, TreeMoveDirection, ROOT_ID};
use ilium_ipc::ClientRequest;

use crate::app::{
    App, AppearanceRow, BoardRenameTarget, BoardStorageKind, CreateBoardState, FocusTarget, Mode,
    SettingsState, SettingsTab,
};
use crate::keymap::{self, Action, ShortcutBase};
use crate::text_prompt::{self, PromptOutcome, TextPromptState};

fn is_press(key: &KeyEvent) -> bool {
    matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat)
}

fn is_escape(event: &Event) -> bool {
    matches!(event, Event::Key(key) if key.code == KeyCode::Esc && is_press(key))
}

/// Top-level per-mode dispatch, called for every non-mouse `Event` (key
/// presses, resizes are handled by the caller before reaching here).
pub fn handle_event(app: &mut App, event: Event) {
    match std::mem::replace(&mut app.mode, Mode::Normal) {
        Mode::Help => handle_help_event(app, &event),
        Mode::Explorer(overlay, target) => handle_explorer_event(app, overlay, target, &event),
        Mode::ExplorerFileMenu(menu) => handle_explorer_file_menu_event(app, menu, &event),
        Mode::FolderExplorer(overlay, target) => {
            handle_folder_explorer_event(app, overlay, target, &event)
        }
        Mode::Rename(state) => handle_rename_event(app, state, &event),
        Mode::CommandPrompt(state) => handle_command_prompt_event(app, state, &event),
        Mode::SaveAs(id, state) => handle_save_as_event(app, id, state, &event),
        Mode::ContextMenu(menu) => handle_context_menu_event(app, menu, &event),
        Mode::CreateGroup(state) => handle_create_group_event(app, state, &event),
        Mode::CreateBoard(state) => handle_create_board_event(app, state, &event),
        Mode::BoardPathPicker(overlay, state) => {
            handle_board_path_picker_event(app, overlay, state, &event)
        }
        Mode::BoardCardPrompt(pane_id, state) => {
            handle_board_card_prompt(app, pane_id, state, &event)
        }
        Mode::BoardColumnPrompt(pane_id, state) => {
            handle_board_column_prompt(app, pane_id, state, &event)
        }
        Mode::BoardRenamePrompt(pane_id, target, state) => {
            handle_board_rename_prompt(app, pane_id, target, state, &event)
        }
        Mode::BoardDeleteConfirm(pane_id, target) => {
            handle_board_delete_confirm(app, pane_id, target, &event)
        }
        Mode::ConfirmClose(target) => handle_confirm_close_event(app, target, &event),
        Mode::Settings(state) => handle_settings_event(app, state, &event),
        Mode::Move => {
            app.mode = Mode::Move;
            if let Event::Key(key) = &event {
                if is_press(key) {
                    handle_move_mode_key(app, key);
                }
            }
        }
        Mode::Normal => {
            app.mode = Mode::Normal;
            handle_normal_or_leader(app, event);
        }
        Mode::LeaderPending => {
            app.mode = Mode::LeaderPending;
            handle_normal_or_leader(app, event);
        }
    }
}

fn handle_board_column_prompt(
    app: &mut App,
    pane_id: NodeId,
    mut state: TextPromptState,
    event: &Event,
) {
    let Event::Key(key) = event else {
        app.mode = Mode::BoardColumnPrompt(pane_id, state);
        return;
    };
    if !is_press(key) {
        app.mode = Mode::BoardColumnPrompt(pane_id, state);
        return;
    }
    match text_prompt::handle_key(&mut state, key.code) {
        PromptOutcome::Commit => {
            app.commit_board_column(pane_id, state.buf);
            app.mode = Mode::Normal;
        }
        PromptOutcome::Cancel => app.mode = Mode::Normal,
        PromptOutcome::Continue => app.mode = Mode::BoardColumnPrompt(pane_id, state),
    }
}

fn handle_create_board_event(app: &mut App, mut state: CreateBoardState, event: &Event) {
    let Event::Key(key) = event else {
        app.mode = Mode::CreateBoard(state);
        return;
    };
    if !is_press(key) {
        app.mode = Mode::CreateBoard(state);
        return;
    }
    match key.code {
        KeyCode::Esc => app.mode = Mode::Normal,
        KeyCode::Tab => state.editing_path = !state.editing_path,
        KeyCode::Char('p') => {
            app.open_board_path_picker(state);
            return;
        }
        KeyCode::Left | KeyCode::Right if !state.editing_path => {
            state.storage_kind = match state.storage_kind {
                BoardStorageKind::Folder => BoardStorageKind::MarkdownFile,
                BoardStorageKind::MarkdownFile => BoardStorageKind::Folder,
            }
        }
        KeyCode::Enter => {
            app.commit_create_board(&state);
            return;
        }
        _ => {
            let prompt = if state.editing_path {
                &mut state.path
            } else {
                &mut state.name
            };
            let _ = text_prompt::handle_key(prompt, key.code);
        }
    }
    app.mode = Mode::CreateBoard(state);
}

fn handle_board_path_picker_event(
    app: &mut App,
    mut overlay: Box<crate::explorer_overlay::ExplorerOverlay>,
    mut state: CreateBoardState,
    event: &Event,
) {
    match overlay.handle(event, app.layout.screen_area) {
        Ok(Some(path)) => {
            state.path = TextPromptState::new(path.display().to_string());
            state.editing_path = true;
            app.mode = Mode::CreateBoard(state);
        }
        Ok(None) if is_escape(event) => app.mode = Mode::CreateBoard(state),
        Ok(None) => app.mode = Mode::BoardPathPicker(overlay, state),
        Err(error) => {
            app.status_message = Some(format!("Board path picker error: {error}"));
            app.mode = Mode::BoardPathPicker(overlay, state);
        }
    }
}

fn handle_board_card_prompt(
    app: &mut App,
    pane_id: NodeId,
    mut state: TextPromptState,
    event: &Event,
) {
    let Event::Key(key) = event else {
        app.mode = Mode::BoardCardPrompt(pane_id, state);
        return;
    };
    if !is_press(key) {
        app.mode = Mode::BoardCardPrompt(pane_id, state);
        return;
    }
    match text_prompt::handle_key(&mut state, key.code) {
        PromptOutcome::Commit => {
            app.commit_board_card(pane_id, state.buf);
            app.mode = Mode::Normal;
        }
        PromptOutcome::Cancel => app.mode = Mode::Normal,
        PromptOutcome::Continue => app.mode = Mode::BoardCardPrompt(pane_id, state),
    }
}

fn handle_board_rename_prompt(
    app: &mut App,
    pane_id: NodeId,
    target: BoardRenameTarget,
    mut state: TextPromptState,
    event: &Event,
) {
    let Event::Key(key) = event else {
        app.mode = Mode::BoardRenamePrompt(pane_id, target, state);
        return;
    };
    if !is_press(key) {
        app.mode = Mode::BoardRenamePrompt(pane_id, target, state);
        return;
    }
    match text_prompt::handle_key(&mut state, key.code) {
        PromptOutcome::Commit => {
            app.commit_board_rename(pane_id, target, state.buf);
            app.mode = Mode::Normal;
        }
        PromptOutcome::Cancel => app.mode = Mode::Normal,
        PromptOutcome::Continue => app.mode = Mode::BoardRenamePrompt(pane_id, target, state),
    }
}

fn handle_board_delete_confirm(
    app: &mut App,
    pane_id: NodeId,
    target: crate::app::BoardDeleteTarget,
    event: &Event,
) {
    let Event::Key(key) = event else {
        app.mode = Mode::BoardDeleteConfirm(pane_id, target);
        return;
    };
    if !is_press(key) {
        app.mode = Mode::BoardDeleteConfirm(pane_id, target);
        return;
    }
    match key.code {
        KeyCode::Char('y' | 'Y') | KeyCode::Enter => {
            app.commit_board_delete(pane_id, target);
            app.mode = Mode::Normal;
        }
        KeyCode::Char('n' | 'N') | KeyCode::Esc => app.mode = Mode::Normal,
        _ => app.mode = Mode::BoardDeleteConfirm(pane_id, target),
    }
}

/// `Mode::Normal` / `Mode::LeaderPending` dispatch: leader-key detection
/// and letter lookup, falling through to ordinary tree-navigation or
/// pane-input handling otherwise.
fn handle_normal_or_leader(app: &mut App, event: Event) {
    let Event::Key(key) = event else {
        return;
    };
    if !is_press(&key) {
        return;
    }

    if matches!(app.mode, Mode::LeaderPending) {
        if let KeyCode::Char(c) = key.code {
            if let Some(action) = keymap::action_for(c) {
                execute_action(app, action);
            }
        }
        // Actions that open a sub-mode (Rename/Move/Help/Explorer/...)
        // already moved `app.mode` on; only fall back to Normal if
        // nothing did.
        if matches!(app.mode, Mode::LeaderPending) {
            app.mode = Mode::Normal;
        }
        return;
    }

    if keymap::is_leader_key(&key, app.keyboard_settings.shortcut_base) {
        app.mode = Mode::LeaderPending;
        return;
    }

    match app.focus {
        FocusTarget::Tree => app.handle_tree_key(key),
        FocusTarget::Pane => app.handle_pane_key(key),
    }
}

/// Runs one leader-key action.
fn execute_action(app: &mut App, action: Action) {
    match action {
        Action::NewTerminal => app.action_new_terminal(),
        Action::NewEditor => app.action_new_editor(),
        Action::NewBoard => app.open_create_board_dialog(),
        Action::ClosePane => app.action_close_selected(),
        Action::NewGroup => {
            let preselected = app.create_group_preselect_target();
            app.open_create_group_dialog(preselected);
        }
        Action::NewFolder => app.action_new_folder(),
        Action::Rename => app.action_start_rename(),
        Action::ToggleMove => app.mode = Mode::Move,
        Action::FocusTree => app.leave_pane_focus(),
        Action::FocusPane => {
            if let Some(id) = app.active_pane_id() {
                app.focus_pane(id);
            } else {
                app.focus = FocusTarget::Pane;
            }
        }
        Action::Save => app.action_save_focused_editor(),
        Action::RunCommand => app.mode = Mode::CommandPrompt(TextPromptState::new("")),
        // Only reachable when Help *wasn't* already open (see
        // `handle_help_event`, which intercepts everything while it is),
        // so this always means "open it".
        Action::Help => app.mode = Mode::Help,
        Action::Quit => {
            app.queue_request(ClientRequest::Detach);
            app.should_quit = true;
        }
        Action::ToggleEditorViewMode => app.action_toggle_editor_view_mode(),
        Action::ToggleLineNumbers => app.action_toggle_editor_line_numbers(),
        Action::ToggleMinimap => app.action_toggle_editor_minimap(),
        Action::ToggleAutosave => app.action_toggle_editor_autosave(),
        Action::Settings => app.action_open_settings(),
    }
}

/// While `Mode::Move` is active: up/down (arrows or `k`/`j`) reorder the
/// selected node one step via a queued `MoveNode` request; left/right
/// (arrows or `h`/`l`) outdent/indent it via a queued `ReparentNode`
/// request (see `compute_outdent_target`/`compute_indent_target`);
/// `Enter`/`m`/`Esc` exits back to Normal.
fn handle_move_mode_key(app: &mut App, key: &KeyEvent) {
    let Some(id) = app.selected_node_id() else {
        return;
    };
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => app.request_move(id, TreeMoveDirection::Up),
        KeyCode::Down | KeyCode::Char('j') => app.request_move(id, TreeMoveDirection::Down),
        KeyCode::Left | KeyCode::Char('h') => {
            if let Some((new_parent, index)) = compute_outdent_target(&app.tree, id) {
                app.request_reparent(id, new_parent, index);
            }
        }
        KeyCode::Right | KeyCode::Char('l') => {
            if let Some((new_parent, index)) = compute_indent_target(&app.tree, id) {
                app.request_reparent(id, new_parent, index);
            }
        }
        KeyCode::Enter | KeyCode::Char('m') | KeyCode::Esc => {
            app.mode = Mode::Normal;
            return;
        }
        _ => {}
    }
    app.mode = Mode::Move;
}

/// Computes the "indent into the previous group" `ReparentNode` target for
/// `id`: the nearest preceding sibling that is a `Group`, walking outward
/// from `id`'s own position among its current siblings, appended at the
/// end of that group's children. Returns `None` when there is no
/// preceding sibling group to indent into (already first, or every
/// preceding sibling is a pane) -- nothing is sent in that case rather
/// than a request that could only be rejected.
fn compute_indent_target(tree: &Tree, id: NodeId) -> Option<(NodeId, Option<usize>)> {
    let parent = tree.parent_of(id)?;
    let siblings = tree.children_of(parent).ok()?;
    let own_index = siblings.iter().position(|&sibling| sibling == id)?;
    let preceding_group = siblings[..own_index].iter().rev().find(|&&candidate| {
        matches!(
            tree.get(candidate).map(|node| &node.kind),
            Some(NodeKind::Container(container)) if container.is_group()
        )
    })?;
    Some((*preceding_group, None))
}

/// Computes the "outdent out of the current group" `ReparentNode` target
/// for `id`: `id`'s current group's own parent, positioned right after
/// that group among its new siblings. Returns `None` when `id` is already
/// at the top level (no enclosing group to outdent out of) or when
/// outdenting would leave a pane parentless at the top level (panes always
/// need an enclosing group) -- nothing is sent in either case rather than
/// a request that could only be rejected.
fn compute_outdent_target(tree: &Tree, id: NodeId) -> Option<(NodeId, Option<usize>)> {
    let current_group = tree.parent_of(id)?;
    if current_group == ROOT_ID {
        return None;
    }
    let grandparent = tree.parent_of(current_group)?;
    let is_pane = matches!(
        tree.get(id).map(|node| &node.kind),
        Some(NodeKind::Pane { .. })
    );
    if grandparent == ROOT_ID && is_pane {
        return None;
    }
    let group_siblings = tree.children_of(grandparent).ok()?;
    let group_index = group_siblings
        .iter()
        .position(|&sibling| sibling == current_group)?;
    Some((grandparent, Some(group_index + 1)))
}

/// While `Mode::Help` is active: only `Esc`, or the configured shortcut base
/// followed by `?`,
/// closes it -- every other key (including other leader letters) is
/// swallowed and Help stays open.
fn handle_help_event(app: &mut App, event: &Event) {
    app.mode = Mode::Help;
    let Event::Key(key) = event else {
        return;
    };
    if !is_press(key) {
        return;
    }

    if app.help_leader_pending() {
        app.set_help_leader_pending(false);
        if key.code == KeyCode::Char('?') {
            app.mode = Mode::Normal;
        }
        return;
    }

    if key.code == KeyCode::Esc {
        app.mode = Mode::Normal;
    } else if keymap::is_leader_key(key, app.keyboard_settings.shortcut_base) {
        app.set_help_leader_pending(true);
    }
}

/// While `Mode::Explorer` is active: forwards the event to the overlay,
/// and on a file pick, queues a `NewPane` request for it under `target`
/// (the destination group -- see `Mode::Explorer`'s doc comment). `Esc`
/// cancels with no request sent.
fn handle_explorer_event(
    app: &mut App,
    mut overlay: Box<crate::explorer_overlay::ExplorerOverlay>,
    target: ilium_core::NodeId,
    event: &Event,
) {
    match overlay.handle(event, app.layout.screen_area) {
        Ok(Some(path)) => {
            app.request_new_editor(target, path);
            app.mode = Mode::Normal;
        }
        Ok(None) => {
            if is_escape(event) {
                app.mode = Mode::Normal;
            } else {
                app.mode = Mode::Explorer(overlay, target);
            }
        }
        Err(err) => {
            app.status_message = Some(format!("File picker error: {err}"));
            app.mode = Mode::Explorer(overlay, target);
        }
    }
}

fn handle_explorer_file_menu_event(
    app: &mut App,
    menu: crate::app::ExplorerFileMenu,
    event: &Event,
) {
    let Event::Key(key) = event else {
        app.mode = Mode::ExplorerFileMenu(menu);
        return;
    };
    if !is_press(key) {
        app.mode = Mode::ExplorerFileMenu(menu);
        return;
    }
    match key.code {
        KeyCode::Enter => {
            app.request_new_markdown_board(menu.target_group, menu.file_path);
            app.mode = Mode::Normal;
        }
        KeyCode::Esc => app.mode = Mode::Explorer(menu.overlay, menu.target_group),
        _ => app.mode = Mode::ExplorerFileMenu(menu),
    }
}

fn handle_folder_explorer_event(
    app: &mut App,
    mut overlay: Box<crate::explorer_overlay::ExplorerOverlay>,
    target: ilium_core::NodeId,
    event: &Event,
) {
    match overlay.handle(event, app.layout.screen_area) {
        Ok(Some(path)) => {
            app.request_new_folder(target, path);
            app.mode = Mode::Normal;
        }
        Ok(None) if is_escape(event) => app.mode = Mode::Normal,
        Ok(None) => app.mode = Mode::FolderExplorer(overlay, target),
        Err(err) => {
            app.status_message = Some(format!("Folder picker error: {err}"));
            app.mode = Mode::FolderExplorer(overlay, target);
        }
    }
}

/// While `Mode::Rename(state)` is active: `Enter` queues a `RenameNode`
/// request, `Esc` cancels. Every other key is delegated to
/// `text_prompt::handle_key`, shared with the other text-prompt modes.
fn handle_rename_event(app: &mut App, mut state: TextPromptState, event: &Event) {
    let Event::Key(key) = event else {
        app.mode = Mode::Rename(state);
        return;
    };
    if !is_press(key) {
        app.mode = Mode::Rename(state);
        return;
    }

    match text_prompt::handle_key(&mut state, key.code) {
        PromptOutcome::Commit => {
            if let Some(id) = app.selected_node_id() {
                app.request_rename(id, state.buf, None);
            }
            app.mode = Mode::Normal;
        }
        PromptOutcome::Cancel => app.mode = Mode::Normal,
        PromptOutcome::Continue => app.mode = Mode::Rename(state),
    }
}

/// While `Mode::CommandPrompt(state)` is active: `Enter` spawns a new
/// terminal pane running the typed command line, `Esc` cancels.
fn handle_command_prompt_event(app: &mut App, mut state: TextPromptState, event: &Event) {
    let Event::Key(key) = event else {
        app.mode = Mode::CommandPrompt(state);
        return;
    };
    if !is_press(key) {
        app.mode = Mode::CommandPrompt(state);
        return;
    }

    match text_prompt::handle_key(&mut state, key.code) {
        PromptOutcome::Commit => {
            app.mode = Mode::Normal;
            if !state.buf.trim().is_empty() {
                app.action_new_command_pane(state.buf);
            }
        }
        PromptOutcome::Cancel => app.mode = Mode::Normal,
        PromptOutcome::Continue => app.mode = Mode::CommandPrompt(state),
    }
}

/// While `Mode::SaveAs(id, state)` is active: `Enter` writes `id`'s
/// editor pane to the typed path, `Esc` cancels without touching the file.
fn handle_save_as_event(
    app: &mut App,
    id: ilium_core::NodeId,
    mut state: TextPromptState,
    event: &Event,
) {
    let Event::Key(key) = event else {
        app.mode = Mode::SaveAs(id, state);
        return;
    };
    if !is_press(key) {
        app.mode = Mode::SaveAs(id, state);
        return;
    }

    match text_prompt::handle_key(&mut state, key.code) {
        PromptOutcome::Commit => {
            app.action_save_as(id, state.buf);
            app.mode = Mode::Normal;
        }
        PromptOutcome::Cancel => app.mode = Mode::Normal,
        PromptOutcome::Continue => app.mode = Mode::SaveAs(id, state),
    }
}

/// While `Mode::ConfirmClose(target)` is active: `y`/`Enter` confirms
/// (queues the `ClosePane` request), `n`/`Esc` cancels back to `Normal`.
fn handle_confirm_close_event(app: &mut App, target: ilium_core::NodeId, event: &Event) {
    let Event::Key(key) = event else {
        app.mode = Mode::ConfirmClose(target);
        return;
    };
    if !is_press(key) {
        app.mode = Mode::ConfirmClose(target);
        return;
    }
    match key.code {
        KeyCode::Char('y' | 'Y') | KeyCode::Enter => {
            app.request_close(target);
            app.mode = Mode::Normal;
        }
        KeyCode::Char('n' | 'N') | KeyCode::Esc => app.mode = Mode::Normal,
        _ => app.mode = Mode::ConfirmClose(target),
    }
}

/// While `Mode::CreateGroup(state)` is active: `Up`/`Down` move the
/// destination selection, `Enter` confirms it, and every other key edits
/// the name field.
fn handle_create_group_event(
    app: &mut App,
    mut state: crate::app::CreateGroupState,
    event: &Event,
) {
    let Event::Key(key) = event else {
        app.mode = Mode::CreateGroup(state);
        return;
    };
    if !is_press(key) {
        app.mode = Mode::CreateGroup(state);
        return;
    }
    match key.code {
        KeyCode::Esc => app.mode = Mode::Normal,
        KeyCode::Up => {
            state.selected_index = state.selected_index.saturating_sub(1);
            app.mode = Mode::CreateGroup(state);
        }
        KeyCode::Down => {
            state.selected_index =
                (state.selected_index + 1).min(state.destinations.len().saturating_sub(1));
            app.mode = Mode::CreateGroup(state);
        }
        KeyCode::Enter => app.commit_create_group(&state),
        _ => {
            text_prompt::handle_key(&mut state.name, key.code);
            app.mode = Mode::CreateGroup(state);
        }
    }
}

/// While `Mode::ContextMenu(menu)` is active: `Up`/`Down` move the
/// selection, `Enter` performs the selected action, `Esc` cancels.
fn handle_context_menu_event(app: &mut App, mut menu: crate::app::ContextMenu, event: &Event) {
    let Event::Key(key) = event else {
        app.mode = Mode::ContextMenu(menu);
        return;
    };
    if !is_press(key) {
        app.mode = Mode::ContextMenu(menu);
        return;
    }
    match key.code {
        KeyCode::Esc => app.mode = Mode::Normal,
        KeyCode::Up | KeyCode::Char('k') => {
            menu.selected_index = menu.selected_index.saturating_sub(1);
            app.mode = Mode::ContextMenu(menu);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            menu.selected_index =
                (menu.selected_index + 1).min(menu.actions.len().saturating_sub(1));
            app.mode = Mode::ContextMenu(menu);
        }
        KeyCode::Enter => {
            app.select_node(menu.target);
            app.execute_context_action(menu.actions[menu.selected_index], menu.target);
        }
        _ => app.mode = Mode::ContextMenu(menu),
    }
}

/// How many lines `PageUp`/`PageDown` scroll the settings screen's content
/// panel per press -- a fixed step rather than a full page, since a page
/// height varies wildly with terminal size and the content itself is short.
const SETTINGS_PAGE_SCROLL_LINES: u16 = 5;

/// While `Mode::Settings(state)` is active: `Esc` or `q` closes it (mirrored
/// by the on-screen hint and the header's close button, see
/// `crate::settings_ui::render_header`/`close_button_hit`); `Tab`/`BackTab`
/// switch tabs; `Up`/`Down` (or `k`/`j`) move the selected Appearance row;
/// `Left`/`Right` (or `h`/`l`, `Enter`, `Space`) decrement/increment the
/// selected row's value; `PageUp`/`PageDown` scroll the content panel. See
/// `crate::settings_ui`'s module doc comment for the design this mirrors.
fn handle_settings_event(app: &mut App, mut state: SettingsState, event: &Event) {
    let Event::Key(key) = event else {
        app.mode = Mode::Settings(state);
        return;
    };
    if !is_press(key) {
        app.mode = Mode::Settings(state);
        return;
    }

    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => {
            app.mode = Mode::Normal;
            return;
        }
        KeyCode::Tab => {
            state.tab = state.tab.next();
            state.scroll = 0;
        }
        KeyCode::BackTab => {
            state.tab = state.tab.previous();
            state.scroll = 0;
        }
        KeyCode::Up | KeyCode::Char('k') if state.tab == SettingsTab::Appearance => {
            state.selected_row = state.selected_row.saturating_sub(1);
        }
        KeyCode::Down | KeyCode::Char('j') if state.tab == SettingsTab::Appearance => {
            state.selected_row =
                (state.selected_row + 1).min(AppearanceRow::ALL.len().saturating_sub(1));
        }
        KeyCode::Left | KeyCode::Char('h') if state.tab == SettingsTab::Appearance => {
            if let Some(row) = AppearanceRow::ALL.get(state.selected_row).copied() {
                app.settings_adjust_row(row, -1);
            }
        }
        KeyCode::Right | KeyCode::Char('l') | KeyCode::Enter | KeyCode::Char(' ')
            if state.tab == SettingsTab::Appearance =>
        {
            if let Some(row) = AppearanceRow::ALL.get(state.selected_row).copied() {
                app.settings_adjust_row(row, 1);
            }
        }
        KeyCode::Left | KeyCode::Char('h') if state.tab == SettingsTab::Keyboard => {
            app.settings_adjust_shortcut_base(-1);
        }
        KeyCode::Right | KeyCode::Char('l') | KeyCode::Enter | KeyCode::Char(' ')
            if state.tab == SettingsTab::Keyboard =>
        {
            app.settings_adjust_shortcut_base(1);
        }
        KeyCode::Char('1') if state.tab == SettingsTab::Keyboard => {
            app.settings_set_shortcut_base(ShortcutBase::A);
        }
        KeyCode::Char('2') if state.tab == SettingsTab::Keyboard => {
            app.settings_set_shortcut_base(ShortcutBase::B);
        }
        KeyCode::Char(letter)
            if state.tab == SettingsTab::Keyboard && letter.is_ascii_uppercase() =>
        {
            if let Some(shortcut_base) = ShortcutBase::parse(&letter.to_string()) {
                app.settings_set_shortcut_base(shortcut_base);
            }
        }
        KeyCode::PageUp => state.scroll = state.scroll.saturating_sub(SETTINGS_PAGE_SCROLL_LINES),
        KeyCode::PageDown => state.scroll = state.scroll.saturating_add(SETTINGS_PAGE_SCROLL_LINES),
        _ => {}
    }

    let content_area = crate::settings_ui::compute_layout(app.layout.screen_area).content_area;
    let max_scroll = crate::settings_ui::max_scroll(
        state.tab,
        &app.ui_settings,
        state.selected_row,
        content_area,
    );
    state.scroll = state.scroll.min(max_scroll);
    app.mode = Mode::Settings(state);
}

#[cfg(test)]
mod indent_outdent_tests {
    use super::*;

    /// `top` (group) containing `sibling_group` (group, empty) and `pane`
    /// (pane), in that order, plus a second top-level group `other`.
    fn sample_tree() -> (Tree, NodeId, NodeId, NodeId, NodeId) {
        let mut tree = Tree::new();
        let top = tree.add_group(ROOT_ID, "top").unwrap();
        let sibling_group = tree.add_group(top, "sibling_group").unwrap();
        let pane = tree
            .add_pane(top, "pane", ilium_core::PaneContentKind::Terminal)
            .unwrap();
        let other = tree.add_group(ROOT_ID, "other").unwrap();
        (tree, top, sibling_group, pane, other)
    }

    #[test]
    fn indent_moves_into_the_nearest_preceding_sibling_group() {
        let (tree, _top, sibling_group, pane, _other) = sample_tree();
        assert_eq!(
            compute_indent_target(&tree, pane),
            Some((sibling_group, None))
        );
    }

    #[test]
    fn indent_with_no_preceding_sibling_group_is_a_no_op() {
        let (tree, _top, sibling_group, ..) = sample_tree();
        // `sibling_group` is the first child of `top`; nothing precedes it.
        assert_eq!(compute_indent_target(&tree, sibling_group), None);
    }

    #[test]
    fn outdent_moves_out_into_the_parent_group_right_after_the_current_group() {
        // top_a -> [inner_b -> [pane_x], pane_c]
        let mut tree = Tree::new();
        let top_a = tree.add_group(ROOT_ID, "top_a").unwrap();
        let inner_b = tree.add_group(top_a, "inner_b").unwrap();
        let pane_x = tree
            .add_pane(inner_b, "x", ilium_core::PaneContentKind::Terminal)
            .unwrap();
        tree.add_pane(top_a, "c", ilium_core::PaneContentKind::Terminal)
            .unwrap();

        // Outdenting `pane_x` out of `inner_b` lands it in `top_a` (the
        // group's own parent) right after `inner_b` among `top_a`'s
        // children -- index 1, ahead of `pane_c`.
        assert_eq!(
            compute_outdent_target(&tree, pane_x),
            Some((top_a, Some(1)))
        );
    }

    #[test]
    fn outdent_at_the_top_level_is_a_no_op() {
        let (tree, top, ..) = sample_tree();
        assert_eq!(compute_outdent_target(&tree, top), None);
    }

    #[test]
    fn outdent_rejects_leaving_a_pane_parentless_at_the_top_level() {
        let mut tree = Tree::new();
        let group = tree.add_group(ROOT_ID, "group").unwrap();
        let pane = tree
            .add_pane(group, "pane", ilium_core::PaneContentKind::Terminal)
            .unwrap();
        // `group` is already top-level, so outdenting `pane` out of it
        // would leave the pane parentless at the root -- rejected.
        assert_eq!(compute_outdent_target(&tree, pane), None);
    }
}

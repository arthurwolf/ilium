//! Keyboard dispatch: routes crossterm key events according to `App::mode`,
//! mirroring the pre-client/server `App::handle_event`'s per-mode structure
//! but translating every structural mutation into a queued `ClientRequest`
//! (see `app.rs`'s module docs) instead of a direct tree edit.

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind};
use illium_core::TreeMoveDirection;
use illium_ipc::ClientRequest;

use crate::app::{App, FocusTarget, Mode};
use crate::keymap::{self, Action};
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
        Mode::Rename(state) => handle_rename_event(app, state, &event),
        Mode::CommandPrompt(state) => handle_command_prompt_event(app, state, &event),
        Mode::SaveAs(id, state) => handle_save_as_event(app, id, state, &event),
        Mode::ContextMenu(menu) => handle_context_menu_event(app, menu, &event),
        Mode::CreateGroup(state) => handle_create_group_event(app, state, &event),
        Mode::ConfirmClose(target) => handle_confirm_close_event(app, target, &event),
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

    if keymap::is_leader_key(&key) {
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
        Action::ClosePane => app.action_close_selected(),
        Action::NewGroup => {
            let preselected = app.create_group_preselect_target();
            app.open_create_group_dialog(preselected);
        }
        Action::Rename => app.action_start_rename(),
        Action::ToggleMove => app.mode = Mode::Move,
        Action::FocusTree => app.focus = FocusTarget::Tree,
        Action::FocusPane => app.focus = FocusTarget::Pane,
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
    }
}

/// While `Mode::Move` is active: up/down (arrows or hjkl) reorder the
/// selected node one step via a queued `MoveNode` request; `Enter`/`m`/
/// `Esc` exits back to Normal.
///
/// Unlike the pre-client/server design, left/right ("outdent"/"indent
/// into the previous group") are not available here: those required an
/// arbitrary reparent-to-any-position tree mutation that
/// `illium_ipc::ClientRequest` has no shape for yet (only the one-step
/// `MoveNode { direction }`, backing `Tree::move_node_one_step` -- see
/// `crate::app::ContextMenuAction`'s doc comment for the same gap).
fn handle_move_mode_key(app: &mut App, key: &KeyEvent) {
    let Some(id) = app.selected_node_id() else {
        return;
    };
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => app.request_move(id, TreeMoveDirection::Up),
        KeyCode::Down | KeyCode::Char('j') => app.request_move(id, TreeMoveDirection::Down),
        KeyCode::Enter | KeyCode::Char('m') | KeyCode::Esc => {
            app.mode = Mode::Normal;
            return;
        }
        _ => {}
    }
    app.mode = Mode::Move;
}

/// While `Mode::Help` is active: only `Esc`, or `Ctrl+A` followed by `?`,
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
    } else if keymap::is_leader_key(key) {
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
    target: illium_core::NodeId,
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
                app.request_rename(id, state.buf);
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
    id: illium_core::NodeId,
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
fn handle_confirm_close_event(app: &mut App, target: illium_core::NodeId, event: &Event) {
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

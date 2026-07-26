//! Keyboard dispatch: routes crossterm key events according to `App::mode`,
//! mirroring the pre-client/server `App::handle_event`'s per-mode structure
//! but translating every structural mutation into a queued `ClientRequest`
//! (see `app.rs`'s module docs) instead of a direct tree edit.

use std::time::Instant;

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ilium_core::{NodeId, NodeKind, Tree, TreeMoveDirection, ROOT_ID};
#[cfg(test)]
use ilium_ipc::ClientRequest;

use crate::agent_from_line::{CreateAgentFocus, CreateAgentFromLineState, EditorLineContextMenu};
use crate::app::{
    App, AppearanceRow, BoardRenameTarget, BoardStorageKind, ClientExitReason, CreateBoardState,
    FocusTarget, KanbanBoardRow, Mode, ProjectFolderSelection, SettingsState, SettingsTab,
    SoundRow,
};
use crate::icon_settings::IconTarget;
use crate::keymap::{self, Action};
use crate::prompt_queue::{PromptQueueDialogState, PromptQueueFocus};
use crate::scheduled_input::{ScheduledInputDialogState, ScheduledInputFocus};
use crate::search_ui::SearchState;
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
    if handle_voice_shortcut(app, &event) {
        return;
    }

    // Enabling bracketed paste changes one clipboard operation from a stream
    // of ordinary key events into `Event::Paste`. The two multiline dialogs
    // already consume that event directly; every other non-terminal mode is
    // replayed through its existing key contract so enabling the host mode
    // cannot silently disable paste in names, paths, searches, or editors.
    if let Event::Paste(pasted) = &event {
        let has_native_paste_handler = matches!(
            &app.mode,
            Mode::Normal
                | Mode::LeaderPending
                | Mode::NavigationLeaderPending
                | Mode::SchedulePaneInput(_)
                | Mode::QueuePrompt(_)
        );
        if !has_native_paste_handler {
            replay_pasted_text_as_keys(app, pasted);
            return;
        }
    }

    match std::mem::replace(&mut app.mode, Mode::Normal) {
        Mode::Help => handle_help_event(app, &event),
        Mode::Explorer(overlay, target) => handle_explorer_event(app, overlay, target, &event),
        Mode::ExplorerFileMenu(menu) => handle_explorer_file_menu_event(app, menu, &event),
        Mode::FolderExplorer(overlay, target) => {
            handle_folder_explorer_event(app, overlay, target, &event)
        }
        Mode::ProjectFolderExplorer(overlay, selection) => {
            handle_project_folder_explorer_event(app, overlay, selection, &event)
        }
        Mode::Rename(state) => handle_rename_event(app, state, &event),
        Mode::CommandPrompt(state) => handle_command_prompt_event(app, state, &event),
        Mode::InferenceSettingPrompt(field, state) => {
            handle_inference_setting_prompt(app, field, state, &event)
        }
        Mode::VoiceSettingPrompt(field, state) => {
            handle_voice_setting_prompt(app, field, state, &event)
        }
        Mode::VoicePromptEditor(state) => handle_voice_prompt_editor(app, state, &event),
        Mode::SaveAs(id, state) => handle_save_as_event(app, id, state, &event),
        Mode::ContextMenu(menu) => handle_context_menu_event(app, menu, &event),
        Mode::TerminalPaneContextMenu(menu) => {
            handle_terminal_pane_context_menu_event(app, menu, &event)
        }
        Mode::AgentDebugLog(state) => handle_agent_debug_log_event(app, state, &event),
        Mode::AgentDebugSavePath(pane_id, state) => {
            handle_agent_debug_save_path_event(app, pane_id, state, &event)
        }
        Mode::SchedulePaneInput(state) => handle_scheduled_input_event(app, state, &event),
        Mode::QueuePrompt(state) => handle_prompt_queue_event(app, state, &event),
        Mode::EditorLineContextMenu(menu) => {
            handle_editor_line_context_menu_event(app, menu, &event)
        }
        Mode::CreateAgentFromLine(state) => handle_create_agent_from_line_event(app, state, &event),
        Mode::CreateGroup(state) => handle_create_group_event(app, state, &event),
        Mode::CreateSplitOrientation(state) => {
            handle_create_split_orientation_event(app, state, &event)
        }
        Mode::CreateSplitMembers(state) => handle_create_split_members_event(app, state, &event),
        Mode::CreateBoard(state) => handle_create_board_event(app, state, &event),
        Mode::BoardPathPicker(overlay) => handle_board_path_picker_event(app, overlay, &event),
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
        Mode::ConfirmSessionRecovery { pane_count } => {
            handle_session_recovery_event(app, pane_count, &event)
        }
        Mode::Settings(state) => handle_settings_event(app, state, &event),
        Mode::Search(state) => handle_search_event(app, state, &event),
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
        Mode::NavigationLeaderPending => {
            app.mode = Mode::NavigationLeaderPending;
            handle_normal_or_leader(app, event);
        }
    }
}

fn handle_terminal_pane_context_menu_event(
    app: &mut App,
    mut menu: crate::terminal_context_menu::TerminalPaneContextMenu,
    event: &Event,
) {
    let Event::Key(key) = event else {
        app.mode = Mode::TerminalPaneContextMenu(menu);
        return;
    };
    if !is_press(key) {
        app.mode = Mode::TerminalPaneContextMenu(menu);
        return;
    }
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => app.mode = Mode::Normal,
        KeyCode::Up | KeyCode::Char('k') => {
            menu.selected_index = menu.selected_index.saturating_sub(1);
            app.mode = Mode::TerminalPaneContextMenu(menu);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            menu.selected_index =
                (menu.selected_index + 1).min(menu.actions.len().saturating_sub(1));
            app.mode = Mode::TerminalPaneContextMenu(menu);
        }
        KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => {
            let action = menu.actions[menu.selected_index];
            app.execute_terminal_context_action(action, menu);
        }
        _ => app.mode = Mode::TerminalPaneContextMenu(menu),
    }
}

fn handle_agent_debug_log_event(
    app: &mut App,
    mut state: crate::app::AgentDebugLogViewState,
    event: &Event,
) {
    let Event::Key(key) = event else {
        app.mode = Mode::AgentDebugLog(state);
        return;
    };
    if !is_press(key) {
        app.mode = Mode::AgentDebugLog(state);
        return;
    }
    match key.code {
        KeyCode::Esc | KeyCode::Backspace | KeyCode::Char('q') => app.mode = Mode::Normal,
        KeyCode::Char('s' | 'S') => app.open_agent_debug_save_path(state),
        KeyCode::Char('r' | 'R') => {
            app.toggle_agent_debug_tree_resize_filter();
            app.mode = Mode::AgentDebugLog(state);
        }
        KeyCode::Up | KeyCode::Char('k') => {
            state.scroll_older(1);
            app.mode = Mode::AgentDebugLog(state);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            state.scroll_newer(1);
            app.mode = Mode::AgentDebugLog(state);
        }
        KeyCode::PageUp => {
            state.scroll_older(10);
            app.mode = Mode::AgentDebugLog(state);
        }
        KeyCode::PageDown => {
            state.scroll_newer(10);
            app.mode = Mode::AgentDebugLog(state);
        }
        KeyCode::Home => {
            state.show_oldest();
            app.mode = Mode::AgentDebugLog(state);
        }
        KeyCode::End => {
            state.follow_newest();
            app.mode = Mode::AgentDebugLog(state);
        }
        _ => app.mode = Mode::AgentDebugLog(state),
    }
}

fn handle_agent_debug_save_path_event(
    app: &mut App,
    pane_id: ilium_core::NodeId,
    mut state: TextPromptState,
    event: &Event,
) {
    let Event::Key(key) = event else {
        app.mode = Mode::AgentDebugSavePath(pane_id, state);
        return;
    };
    if !is_press(key) {
        app.mode = Mode::AgentDebugSavePath(pane_id, state);
        return;
    }
    match text_prompt::handle_key(&mut state, key.code) {
        PromptOutcome::Commit if app.save_agent_debug_log(pane_id, &state.buf) => app.pop_modal(),
        PromptOutcome::Commit | PromptOutcome::Continue => {
            app.mode = Mode::AgentDebugSavePath(pane_id, state)
        }
        PromptOutcome::Cancel => app.pop_modal(),
    }
}

/// F8 is global because voice control must remain reachable while any ilium
/// panel or modal owns ordinary text input. Semantic VAD uses it as a toggle;
/// push-to-talk preserves distinct press and release edges.
fn handle_voice_shortcut(app: &mut App, event: &Event) -> bool {
    let Event::Key(key) = event else {
        return false;
    };
    if key.code != KeyCode::F(8) {
        return false;
    }

    match app.voice_settings.input_mode {
        ilium_voice::VoiceInputMode::SemanticVad => {
            if matches!(key.kind, KeyEventKind::Press) {
                app.toggle_voice_control();
            }
        }
        ilium_voice::VoiceInputMode::PushToTalk => match key.kind {
            KeyEventKind::Press => {
                if !app.voice_settings.enabled {
                    app.toggle_voice_control();
                }
                app.request_voice_interaction(crate::app::VoiceInteractionRequest::StartPushToTalk);
            }
            KeyEventKind::Release => {
                app.request_voice_interaction(crate::app::VoiceInteractionRequest::StopPushToTalk)
            }
            KeyEventKind::Repeat => {}
        },
    }
    true
}

/// Replays pasted text through modes that still own character-oriented input.
/// CRLF is one Enter, while bare CR/LF and Tab retain the same key semantics
/// they had before the host terminal started reporting atomic paste events.
fn replay_pasted_text_as_keys(app: &mut App, pasted: &str) {
    let mut characters = pasted.chars().peekable();
    while let Some(character) = characters.next() {
        let code = match character {
            '\r' => {
                if characters.peek() == Some(&'\n') {
                    characters.next();
                }
                KeyCode::Enter
            }
            '\n' => KeyCode::Enter,
            '\t' => KeyCode::Tab,
            character => KeyCode::Char(character),
        };
        handle_event(app, Event::Key(KeyEvent::new(code, KeyModifiers::NONE)));
    }
}

fn handle_session_recovery_event(app: &mut App, pane_count: usize, event: &Event) {
    let Event::Key(key) = event else {
        app.mode = Mode::ConfirmSessionRecovery { pane_count };
        return;
    };
    if !is_press(key) {
        app.mode = Mode::ConfirmSessionRecovery { pane_count };
        return;
    }
    match key.code {
        KeyCode::Char('y' | 'Y') | KeyCode::Enter => {
            app.resolve_session_recovery(true);
            app.mode = Mode::Normal;
        }
        KeyCode::Char('n' | 'N') | KeyCode::Esc => {
            app.resolve_session_recovery(false);
            app.mode = Mode::Normal;
        }
        _ => app.mode = Mode::ConfirmSessionRecovery { pane_count },
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
        KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
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
    event: &Event,
) {
    match overlay.handle(event, app.layout.screen_area) {
        Ok(Some(path)) => app.return_to_create_board(Some(path)),
        Ok(None) if is_escape(event) => app.return_to_create_board(None),
        Ok(None) => app.mode = Mode::BoardPathPicker(overlay),
        Err(error) => {
            app.status_message = Some(format!("Board path picker error: {error}"));
            app.mode = Mode::BoardPathPicker(overlay);
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

/// `Mode::Normal` / leader-pending dispatch: leader-key detection and key
/// lookup, falling through to ordinary tree-navigation or
/// pane-input handling otherwise.
fn handle_normal_or_leader(app: &mut App, event: Event) {
    if let Event::Paste(pasted) = event {
        // A leader prefix followed by a clipboard operation is not a valid
        // command. Cancel it rather than interpreting the pasted text as a
        // sequence of ilium shortcuts.
        if matches!(
            app.mode,
            Mode::LeaderPending | Mode::NavigationLeaderPending
        ) {
            app.mode = Mode::Normal;
            return;
        }

        // Terminal panes preserve the paste as one semantic operation. Other
        // pane kinds and tree focus retain their existing per-key behavior.
        if app.focus == FocusTarget::Pane && app.handle_terminal_paste(&pasted) {
            return;
        }
        replay_pasted_text_as_keys(app, &pasted);
        return;
    }

    let Event::Key(key) = event else {
        return;
    };
    if !is_press(&key) {
        return;
    }

    if app.pending_terminal_link.is_some() {
        if key.code == KeyCode::Esc {
            app.pending_terminal_link = None;
            app.status_message = Some("Link action cancelled".to_string());
            return;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('o') {
            app.confirm_terminal_link(true);
            return;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            app.confirm_terminal_link(false);
            return;
        }
    }

    if matches!(
        app.mode,
        Mode::LeaderPending | Mode::NavigationLeaderPending
    ) {
        let only_navigation_actions = matches!(app.mode, Mode::NavigationLeaderPending);
        if let Some(binding_key) = keymap::BindingKey::from_key_event(&key) {
            if let Some(action) = keymap::action_for_table(&app.keybindings, binding_key) {
                if !only_navigation_actions || action.uses_navigation_prefix() {
                    execute_action(app, action);
                }
            }
        }
        // Actions that open a sub-mode (Rename/Move/Help/Explorer/...)
        // already moved `app.mode` on; only fall back to Normal if
        // nothing did.
        if matches!(
            app.mode,
            Mode::LeaderPending | Mode::NavigationLeaderPending
        ) {
            app.mode = Mode::Normal;
        }
        return;
    }

    if keymap::is_leader_key(&key, app.keyboard_settings.shortcut_base) {
        app.mode = Mode::LeaderPending;
        return;
    }

    if keymap::is_leader_key(&key, app.keyboard_settings.navigation_shortcut_base) {
        app.mode = Mode::NavigationLeaderPending;
        return;
    }

    match app.focus {
        FocusTarget::Tree => app.handle_tree_key(key),
        FocusTarget::Pane => app.handle_pane_key(key),
    }
}

/// Full-screen search treats typing as an immediate query edit rather than a
/// submitted form. Navigation keys never mutate the query, and Enter uses the
/// selected hit's durable location to open the right pane/buffer position.
fn handle_search_event(app: &mut App, mut state: Box<SearchState>, event: &Event) {
    let Event::Key(key) = event else {
        app.mode = Mode::Search(state);
        return;
    };
    if !is_press(key) {
        app.mode = Mode::Search(state);
        return;
    }
    match key.code {
        KeyCode::Esc => app.mode = Mode::Normal,
        KeyCode::Enter => {
            if let Some(result) = state.selected_result().cloned() {
                app.activate_search_result(result);
            } else {
                app.mode = Mode::Search(state);
            }
        }
        KeyCode::Up => {
            state.move_selection(
                -1,
                crate::search_ui::visible_result_rows(app.layout.screen_area),
            );
            app.mode = Mode::Search(state);
        }
        KeyCode::Down => {
            state.move_selection(
                1,
                crate::search_ui::visible_result_rows(app.layout.screen_area),
            );
            app.mode = Mode::Search(state);
        }
        KeyCode::PageUp => {
            let rows = crate::search_ui::visible_result_rows(app.layout.screen_area) as i32;
            state.move_selection(
                -rows,
                crate::search_ui::visible_result_rows(app.layout.screen_area),
            );
            app.mode = Mode::Search(state);
        }
        KeyCode::PageDown => {
            let rows = crate::search_ui::visible_result_rows(app.layout.screen_area) as i32;
            state.move_selection(
                rows,
                crate::search_ui::visible_result_rows(app.layout.screen_area),
            );
            app.mode = Mode::Search(state);
        }
        _ => {
            let query_len_before = state.query.buf.len();
            let _ = text_prompt::handle_key(&mut state.query, key.code);
            if state.query.buf.len() != query_len_before {
                state.note_query_changed(Instant::now());
            }
            app.mode = Mode::Search(state);
        }
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
        Action::NewSplitView => app.open_create_split_dialog(),
        Action::NewFolder => app.action_new_folder(),
        Action::Rename => app.action_start_rename(),
        Action::ToggleMove => app.mode = Mode::Move,
        Action::FocusTree => app.leave_pane_focus(),
        Action::FocusPane => {
            if let Some(id) = app.active_pane_id() {
                app.focus_pane(id);
            } else if let Some(id) = app.displayed_pane_ids().first().copied() {
                app.focus_pane(id);
            } else {
                app.focus = FocusTarget::Pane;
            }
        }
        Action::FocusNextPane => app.focus_visible_pane(1),
        Action::FocusPreviousPane => app.focus_visible_pane(-1),
        Action::FocusPaneLeft => app.focus_visible_pane_in_direction(-1, 0),
        Action::FocusPaneRight => app.focus_visible_pane_in_direction(1, 0),
        Action::FocusPaneUp => app.focus_visible_pane_in_direction(0, -1),
        Action::FocusPaneDown => app.focus_visible_pane_in_direction(0, 1),
        Action::CycleNextInGroup => app.cycle_pane_in_current_group(1),
        Action::CyclePreviousInGroup => app.cycle_pane_in_current_group(-1),
        Action::JumpNextGroup => app.jump_to_group(1),
        Action::JumpPreviousGroup => app.jump_to_group(-1),
        Action::ScrollbackUp => app.scroll_focused_terminal(-1),
        Action::ScrollbackDown => app.scroll_focused_terminal(1),
        Action::Save => app.action_save_focused_editor(),
        Action::RunCommand => app.mode = Mode::CommandPrompt(TextPromptState::new("")),
        Action::Search => app.action_open_search(),
        // Only reachable when Help *wasn't* already open (see
        // `handle_help_event`, which intercepts everything while it is),
        // so this always means "open it".
        Action::Help => app.mode = Mode::Help,
        Action::Detach => app.request_client_exit(ClientExitReason::Quit),
        Action::Quit => app.request_session_kill(),
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
    let moving_is_pane = tree.get(id).is_some_and(ilium_core::Node::is_pane);
    let preceding_container =
        siblings[..own_index].iter().rev().find(|&&candidate| {
            match tree.get(candidate).map(|node| &node.kind) {
                Some(NodeKind::Container(container)) if container.is_group() => true,
                Some(NodeKind::Container(container)) if container.is_split_view() => {
                    moving_is_pane
                        && container.children.len() < ilium_core::MAXIMUM_SPLIT_VIEW_PANES
                }
                _ => false,
            }
        })?;
    Some((*preceding_container, None))
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
    // Split views are exempt: `Tree::move_node`/`create_split_view` only
    // require the new parent to satisfy `is_group()`, and the session root
    // itself does (see `ilium_core::Tree`'s "one root normal group" doc
    // comment) -- only panes are specifically barred from sitting directly
    // under `ROOT_ID`.
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
/// followed by the currently configured Help key,
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
        if crate::keymap::BindingKey::from_key_event(key).is_some_and(|binding_key| {
            crate::keymap::action_for_table(&app.keybindings, binding_key) == Some(Action::Help)
        }) {
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
            app.close_modal_flow();
        }
        KeyCode::Esc => app.pop_modal(),
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

fn handle_project_folder_explorer_event(
    app: &mut App,
    mut overlay: Box<crate::explorer_overlay::ExplorerOverlay>,
    selection: ProjectFolderSelection,
    event: &Event,
) {
    match overlay.handle(event, app.layout.screen_area) {
        Ok(Some(path)) => {
            match selection {
                ProjectFolderSelection::NewProject => app.request_new_project(path),
                ProjectFolderSelection::ChangeProject(project_id) => {
                    app.request_change_project_folder(project_id, path)
                }
            }
            app.mode = Mode::Normal;
        }
        Ok(None) if is_escape(event) => app.mode = Mode::Normal,
        Ok(None) => app.mode = Mode::ProjectFolderExplorer(overlay, selection),
        Err(err) => {
            app.status_message = Some(format!("Project picker error: {err}"));
            app.mode = Mode::ProjectFolderExplorer(overlay, selection);
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
                app.request_rename(id, state.buf, None, None);
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

/// Edits one inference provider field while preserving the Settings screen as
/// the return destination rather than dropping the user into normal mode.
fn handle_inference_setting_prompt(
    app: &mut App,
    field: crate::app::InferenceSettingField,
    mut state: TextPromptState,
    event: &Event,
) {
    let Event::Key(key) = event else {
        app.mode = Mode::InferenceSettingPrompt(field, state);
        return;
    };
    if !is_press(key) {
        app.mode = Mode::InferenceSettingPrompt(field, state);
        return;
    }
    match text_prompt::handle_key(&mut state, key.code) {
        PromptOutcome::Commit => {
            app.settings_commit_inference_field(field, state.buf);
            app.pop_modal();
        }
        PromptOutcome::Cancel => app.pop_modal(),
        PromptOutcome::Continue => app.mode = Mode::InferenceSettingPrompt(field, state),
    }
}

fn handle_voice_setting_prompt(
    app: &mut App,
    field: crate::voice_settings::VoiceSettingField,
    mut state: TextPromptState,
    event: &Event,
) {
    let Event::Key(key) = event else {
        app.mode = Mode::VoiceSettingPrompt(field, state);
        return;
    };
    if !is_press(key) {
        app.mode = Mode::VoiceSettingPrompt(field, state);
        return;
    }
    match text_prompt::handle_key(&mut state, key.code) {
        PromptOutcome::Commit => {
            app.settings_commit_voice_field(field, state.buf);
            app.pop_modal();
        }
        PromptOutcome::Cancel => app.pop_modal(),
        PromptOutcome::Continue => app.mode = Mode::VoiceSettingPrompt(field, state),
    }
}

fn handle_voice_prompt_editor(
    app: &mut App,
    mut state: Box<crate::voice_settings::VoicePromptEditorState>,
    event: &Event,
) {
    let Event::Key(key) = event else {
        app.mode = Mode::VoicePromptEditor(state);
        return;
    };
    if !is_press(key) {
        app.mode = Mode::VoicePromptEditor(state);
        return;
    }
    match (key.code, key.modifiers) {
        (KeyCode::Esc, _) => app.pop_modal(),
        (KeyCode::Char('s'), modifiers) if modifiers.contains(KeyModifiers::CONTROL) => {
            app.settings_commit_voice_prompt(state.text());
            app.pop_modal();
        }
        _ => {
            state.textarea.input(*key);
            app.mode = Mode::VoicePromptEditor(state);
        }
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

fn handle_create_split_orientation_event(
    app: &mut App,
    mut state: crate::app::CreateSplitOrientationState,
    event: &Event,
) {
    let Event::Key(key) = event else {
        app.mode = Mode::CreateSplitOrientation(state);
        return;
    };
    if !is_press(key) {
        app.mode = Mode::CreateSplitOrientation(state);
        return;
    }
    match key.code {
        KeyCode::Esc => app.mode = Mode::Normal,
        KeyCode::Left | KeyCode::Right | KeyCode::Up | KeyCode::Down | KeyCode::Tab => {
            state.orientation = match state.orientation {
                ilium_core::SplitOrientation::Vertical => ilium_core::SplitOrientation::Horizontal,
                ilium_core::SplitOrientation::Horizontal => ilium_core::SplitOrientation::Vertical,
            };
            app.mode = Mode::CreateSplitOrientation(state);
        }
        KeyCode::Enter => app.continue_create_split(state.orientation),
        KeyCode::Char('e') | KeyCode::Char('E') => app.commit_empty_split(state.orientation),
        _ => app.mode = Mode::CreateSplitOrientation(state),
    }
}

fn handle_create_split_members_event(
    app: &mut App,
    mut state: crate::app::CreateSplitMembersState,
    event: &Event,
) {
    let Event::Key(key) = event else {
        app.mode = Mode::CreateSplitMembers(state);
        return;
    };
    if !is_press(key) {
        app.mode = Mode::CreateSplitMembers(state);
        return;
    }
    match key.code {
        KeyCode::Esc => app.mode = Mode::Normal,
        KeyCode::Up | KeyCode::Char('k') => {
            state.selected_index = state.selected_index.saturating_sub(1);
            app.mode = Mode::CreateSplitMembers(state);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            state.selected_index =
                (state.selected_index + 1).min(state.choices.len().saturating_sub(1));
            app.mode = Mode::CreateSplitMembers(state);
        }
        KeyCode::Char(' ') => {
            app.toggle_create_split_member(&mut state);
            app.mode = Mode::CreateSplitMembers(state);
        }
        KeyCode::Enter => app.commit_create_split(state),
        _ => app.mode = Mode::CreateSplitMembers(state),
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

    if let Some(submenu) = menu.tree_order_submenu.as_mut() {
        match key.code {
            KeyCode::Esc | KeyCode::Left | KeyCode::Char('h') => {
                menu.tree_order_submenu = None;
                app.mode = Mode::ContextMenu(menu);
            }
            KeyCode::Up | KeyCode::Char('k') => {
                submenu.selected_index = submenu.selected_index.saturating_sub(1);
                app.mode = Mode::ContextMenu(menu);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                submenu.selected_index = (submenu.selected_index + 1)
                    .min(crate::config::TreeOrder::ALL.len().saturating_sub(1));
                app.mode = Mode::ContextMenu(menu);
            }
            KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => {
                if let Some(tree_order) = crate::config::TreeOrder::ALL
                    .get(submenu.selected_index)
                    .copied()
                {
                    app.settings_set_tree_order(tree_order);
                }
                app.mode = Mode::Normal;
            }
            _ => app.mode = Mode::ContextMenu(menu),
        }
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
        KeyCode::Right | KeyCode::Char('l')
            if menu.actions[menu.selected_index] == crate::app::ContextMenuAction::OrderBy =>
        {
            app.open_context_tree_order_submenu(&mut menu);
            app.mode = Mode::ContextMenu(menu);
        }
        KeyCode::Enter => {
            if menu.actions[menu.selected_index] == crate::app::ContextMenuAction::OrderBy {
                app.open_context_tree_order_submenu(&mut menu);
                app.mode = Mode::ContextMenu(menu);
                return;
            }
            app.select_node(menu.target);
            app.execute_context_action(menu.actions[menu.selected_index], menu.target);
        }
        _ => app.mode = Mode::ContextMenu(menu),
    }
}

/// Source-line menus use the same keyboard contract as tree menus while
/// dispatching their file-line target through a separate action boundary.
fn handle_editor_line_context_menu_event(
    app: &mut App,
    mut menu: EditorLineContextMenu,
    event: &Event,
) {
    let Event::Key(key) = event else {
        app.mode = Mode::EditorLineContextMenu(menu);
        return;
    };
    if !is_press(key) {
        app.mode = Mode::EditorLineContextMenu(menu);
        return;
    }

    match key.code {
        KeyCode::Esc => app.mode = Mode::Normal,
        KeyCode::Up | KeyCode::Char('k') => {
            menu.selected_index = menu.selected_index.saturating_sub(1);
            app.mode = Mode::EditorLineContextMenu(menu);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            menu.selected_index =
                (menu.selected_index + 1).min(menu.actions.len().saturating_sub(1));
            app.mode = Mode::EditorLineContextMenu(menu);
        }
        KeyCode::Enter => {
            let action = menu.actions[menu.selected_index];
            app.execute_editor_line_context_action(action, menu.source);
        }
        _ => app.mode = Mode::EditorLineContextMenu(menu),
    }
}

/// Edits the multi-line prompt, changes the selected agent, or commits the
/// creation dialog. Enter remains a real textarea newline; Ctrl+Enter and the
/// focused Create button are the explicit keyboard submission paths.
fn handle_create_agent_from_line_event(
    app: &mut App,
    mut state: Box<CreateAgentFromLineState>,
    event: &Event,
) {
    use crossterm::event::KeyModifiers;

    let Event::Key(key) = event else {
        app.mode = Mode::CreateAgentFromLine(state);
        return;
    };
    if !is_press(key) {
        app.mode = Mode::CreateAgentFromLine(state);
        return;
    }
    if key.code == KeyCode::Esc {
        app.mode = Mode::Normal;
        return;
    }
    if key.code == KeyCode::Enter && key.modifiers.contains(KeyModifiers::CONTROL) {
        app.commit_create_agent_from_line(state);
        return;
    }

    match key.code {
        KeyCode::Tab => state.focus = state.focus.next(),
        KeyCode::BackTab => state.focus = state.focus.previous(),
        KeyCode::Left | KeyCode::Char('h') if state.focus == CreateAgentFocus::AgentType => {
            state.agent_type = state.agent_type.stepped(-1)
        }
        KeyCode::Right | KeyCode::Char('l') if state.focus == CreateAgentFocus::AgentType => {
            state.agent_type = state.agent_type.stepped(1)
        }
        KeyCode::Char('c') if state.focus == CreateAgentFocus::AgentType => {
            state.agent_type = crate::agent_from_line::AgentLaunchType::Claude
        }
        KeyCode::Char('x') if state.focus == CreateAgentFocus::AgentType => {
            state.agent_type = crate::agent_from_line::AgentLaunchType::Codex
        }
        KeyCode::Char('a') if state.focus == CreateAgentFocus::AgentType => {
            state.agent_type = crate::agent_from_line::AgentLaunchType::Antigravity
        }
        KeyCode::Enter if state.focus == CreateAgentFocus::AgentType => {
            state.focus = CreateAgentFocus::Prompt
        }
        KeyCode::Enter if state.focus == CreateAgentFocus::CreateButton => {
            app.commit_create_agent_from_line(state);
            return;
        }
        _ if state.focus == CreateAgentFocus::Prompt => {
            state.prompt.input(event.clone());
        }
        _ => {}
    }
    app.mode = Mode::CreateAgentFromLine(state);
}

/// Edits the six controls in the scheduled-input form. Tab order mirrors the
/// visual order, Ctrl+Enter is a form-wide submit shortcut, and plain Enter
/// advances until the explicit button is focused.
fn handle_scheduled_input_event(
    app: &mut App,
    mut state: Box<ScheduledInputDialogState>,
    event: &Event,
) {
    use crossterm::event::KeyModifiers;

    if let Event::Paste(pasted) = event {
        state.paste(pasted);
        app.mode = Mode::SchedulePaneInput(state);
        return;
    }
    let Event::Key(key) = event else {
        app.mode = Mode::SchedulePaneInput(state);
        return;
    };
    if !is_press(key) {
        app.mode = Mode::SchedulePaneInput(state);
        return;
    }
    if key.code == KeyCode::Esc {
        app.mode = Mode::Normal;
        return;
    }
    if key.code == KeyCode::Enter && key.modifiers.contains(KeyModifiers::CONTROL) {
        app.commit_scheduled_pane_input(state);
        return;
    }

    match key.code {
        KeyCode::Tab => state.focus = state.focus.next(),
        KeyCode::BackTab => state.focus = state.focus.previous(),
        KeyCode::Enter if state.focus == ScheduledInputFocus::SendEnter => {
            state.send_enter = !state.send_enter;
            state.focus = state.focus.next();
        }
        KeyCode::Char(' ') if state.focus == ScheduledInputFocus::SendEnter => {
            state.send_enter = !state.send_enter;
        }
        KeyCode::Enter if state.focus == ScheduledInputFocus::ScheduleButton => {
            app.commit_scheduled_pane_input(state);
            return;
        }
        KeyCode::Enter => state.focus = state.focus.next(),
        code if matches!(
            state.focus,
            ScheduledInputFocus::Hours
                | ScheduledInputFocus::Minutes
                | ScheduledInputFocus::Seconds
        ) =>
        {
            state.handle_numeric_key(code);
        }
        code if state.focus == ScheduledInputFocus::Text => state.handle_text_key(code),
        _ => {}
    }
    app.mode = Mode::SchedulePaneInput(state);
}

fn handle_prompt_queue_event(app: &mut App, mut state: Box<PromptQueueDialogState>, event: &Event) {
    use crossterm::event::KeyModifiers;

    if let Event::Paste(pasted) = event {
        if state.focus == PromptQueueFocus::Text {
            for character in pasted.chars() {
                state.handle_text_key(KeyCode::Char(character));
            }
        }
        app.mode = Mode::QueuePrompt(state);
        return;
    }
    let Event::Key(key) = event else {
        app.mode = Mode::QueuePrompt(state);
        return;
    };
    if !is_press(key) {
        app.mode = Mode::QueuePrompt(state);
        return;
    }
    if key.code == KeyCode::Esc {
        app.mode = Mode::Normal;
        return;
    }
    if key.code == KeyCode::Enter && key.modifiers.contains(KeyModifiers::CONTROL) {
        app.commit_queued_prompt(state);
        return;
    }
    match key.code {
        KeyCode::Tab => state.focus = state.focus.next(),
        KeyCode::BackTab => state.focus = state.focus.previous(),
        KeyCode::Left if state.focus == PromptQueueFocus::Delivery => state.cycle_delivery(-1),
        KeyCode::Right if state.focus == PromptQueueFocus::Delivery => state.cycle_delivery(1),
        KeyCode::Char(' ') if state.focus == PromptQueueFocus::Delivery => state.cycle_delivery(1),
        KeyCode::Enter if state.focus == PromptQueueFocus::EnqueueButton => {
            app.commit_queued_prompt(state);
            return;
        }
        KeyCode::Enter if state.focus != PromptQueueFocus::Text => state.focus = state.focus.next(),
        code if state.focus == PromptQueueFocus::Text => state.handle_text_key(code),
        code if state.focus == PromptQueueFocus::Times => state.handle_times_key(code),
        _ => {}
    }
    app.mode = Mode::QueuePrompt(state);
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

    if let Some(mut picker) = state.icon_picker.take() {
        let entry_count = picker.search_results.entry_count;
        let grid_columns = crate::settings_ui::icon_picker_grid_columns(
            app.layout.screen_area,
            picker.column_mode,
        );
        let mut selection_moved = false;
        match key.code {
            KeyCode::Esc => {
                if picker.is_searching {
                    picker.is_searching = false;
                } else {
                    app.mode = Mode::Settings(state);
                    return;
                }
            }
            KeyCode::Char('/') if !picker.is_searching => picker.is_searching = true,
            KeyCode::Char('v') if !picker.is_searching => {
                picker.column_mode = picker.column_mode.toggle();
                picker.scroll_row = crate::settings_ui::icon_picker_scroll_for_entry(
                    app.layout.screen_area,
                    &picker,
                );
            }
            KeyCode::Backspace if picker.is_searching => {
                picker.search_query.pop();
                if let Some(request) = picker.begin_semantic_search() {
                    app.queue_icon_semantic_search(request);
                }
            }
            KeyCode::Enter if picker.is_searching => picker.is_searching = false,
            KeyCode::Char(character) if picker.is_searching => {
                picker.search_query.push(character);
                if let Some(request) = picker.begin_semantic_search() {
                    app.queue_icon_semantic_search(request);
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                picker.selected_entry = picker.selected_entry.saturating_sub(grid_columns);
                selection_moved = true;
            }
            KeyCode::Down | KeyCode::Char('j') => {
                picker.selected_entry =
                    (picker.selected_entry + grid_columns).min(entry_count.saturating_sub(1));
                selection_moved = true;
            }
            KeyCode::Left | KeyCode::Char('h') => {
                picker.selected_entry = picker.selected_entry.saturating_sub(1);
                selection_moved = true;
            }
            KeyCode::Right | KeyCode::Char('l') => {
                picker.selected_entry =
                    (picker.selected_entry + 1).min(entry_count.saturating_sub(1));
                selection_moved = true;
            }
            KeyCode::PageUp => {
                picker.scroll_row = picker.scroll_row.saturating_sub(usize::from(
                    crate::settings_ui::icon_picker_layout(app.layout.screen_area)
                        .document_area
                        .height,
                ));
            }
            KeyCode::PageDown => {
                picker.scroll_row = picker.scroll_row.saturating_add(usize::from(
                    crate::settings_ui::icon_picker_layout(app.layout.screen_area)
                        .document_area
                        .height,
                ));
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                if let Some(entry) = picker.search_results.entry(picker.selected_entry) {
                    app.settings_set_icon(picker.target, entry.glyph.to_string());
                }
                app.mode = Mode::Settings(state);
                return;
            }
            _ => {}
        }
        if selection_moved {
            picker.scroll_row =
                crate::settings_ui::icon_picker_scroll_for_entry(app.layout.screen_area, &picker);
        }
        state.icon_picker = Some(picker);
        app.mode = Mode::Settings(state);
        return;
    }

    if let Some(action) = state.keyboard_picker.take() {
        match key.code {
            KeyCode::Esc => {}
            KeyCode::Char(key) => {
                if crate::keymap::available_keys(&app.keybindings).contains(&key) {
                    app.settings_assign_key(action, crate::keymap::BindingKey::Character(key));
                } else {
                    state.keyboard_picker = Some(action);
                }
            }
            KeyCode::Up | KeyCode::Down | KeyCode::PageUp | KeyCode::PageDown => {
                let binding_key = crate::keymap::BindingKey::from_key_event(key)
                    .expect("the matched navigation key is representable");
                if app
                    .keybindings
                    .iter()
                    .all(|binding| binding.key != binding_key || binding.action == action)
                {
                    app.settings_assign_key(action, binding_key);
                } else {
                    state.keyboard_picker = Some(action);
                }
            }
            _ => state.keyboard_picker = Some(action),
        }
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
            state.selected_row = 0;
            state.trigger_action_cursor = 0;
            state.scroll = 0;
        }
        KeyCode::BackTab => {
            state.tab = state.tab.previous();
            state.selected_row = 0;
            state.trigger_action_cursor = 0;
            state.scroll = 0;
        }
        KeyCode::Up | KeyCode::Char('k') if state.tab == SettingsTab::Icons => {
            state.selected_row = state.selected_row.saturating_sub(1);
        }
        KeyCode::Down | KeyCode::Char('j') if state.tab == SettingsTab::Icons => {
            state.selected_row =
                (state.selected_row + 1).min(IconTarget::ALL.len().saturating_sub(1));
        }
        KeyCode::Left | KeyCode::Char('h') if state.tab == SettingsTab::Icons => {
            if let Some(target) = IconTarget::ALL.get(state.selected_row).copied() {
                app.settings_cycle_icon(target, -1);
            }
        }
        KeyCode::Right | KeyCode::Char('l') if state.tab == SettingsTab::Icons => {
            if let Some(target) = IconTarget::ALL.get(state.selected_row).copied() {
                app.settings_cycle_icon(target, 1);
            }
        }
        KeyCode::Enter | KeyCode::Char(' ') if state.tab == SettingsTab::Icons => {
            if let Some(target) = IconTarget::ALL.get(state.selected_row).copied() {
                state.icon_picker = Some(crate::app::IconPickerState::new(target));
            }
        }
        KeyCode::Char('r') if state.tab == SettingsTab::Icons => {
            state.icons_preview_real = !state.icons_preview_real
        }
        KeyCode::Up | KeyCode::Char('k') if state.tab == SettingsTab::Inference => {
            state.selected_row = state.selected_row.saturating_sub(1);
        }
        KeyCode::Down | KeyCode::Char('j') if state.tab == SettingsTab::Inference => {
            state.selected_row = (state.selected_row + 1).min(
                crate::settings_ui::inference_rows(&app.inference_settings)
                    .len()
                    .saturating_sub(1),
            );
        }
        KeyCode::Left | KeyCode::Char('h') if state.tab == SettingsTab::Inference => {
            match crate::settings_ui::inference_rows(&app.inference_settings)
                .get(state.selected_row)
            {
                Some(crate::app::InferenceRow::Provider) => {
                    app.settings_adjust_inference_provider(-1)
                }
                Some(crate::app::InferenceRow::Field(
                    crate::app::InferenceSettingField::OllamaModel,
                )) => app.settings_adjust_ollama_model(-1),
                Some(crate::app::InferenceRow::KiloGatewayModel) => {
                    app.settings_adjust_kilo_gateway_model(-1)
                }
                _ => {}
            }
        }
        KeyCode::Right | KeyCode::Char('l') | KeyCode::Enter | KeyCode::Char(' ')
            if state.tab == SettingsTab::Inference =>
        {
            match crate::settings_ui::inference_rows(&app.inference_settings)
                .get(state.selected_row)
                .copied()
            {
                Some(crate::app::InferenceRow::Provider) => {
                    app.settings_adjust_inference_provider(1)
                }
                Some(crate::app::InferenceRow::RefreshModels) => app.request_model_refresh(),
                Some(crate::app::InferenceRow::KiloGatewayModel) => {
                    app.settings_adjust_kilo_gateway_model(1)
                }
                Some(crate::app::InferenceRow::Field(
                    crate::app::InferenceSettingField::OllamaModel,
                )) if matches!(key.code, KeyCode::Right | KeyCode::Char('l')) => {
                    app.settings_adjust_ollama_model(1)
                }
                Some(crate::app::InferenceRow::Field(field)) => {
                    app.mode = Mode::Settings(state);
                    app.settings_open_inference_field(field);
                    return;
                }
                Some(crate::app::InferenceRow::Test) => app.request_inference_test(),
                None => {}
            }
        }
        KeyCode::Up | KeyCode::Char('k') if state.tab == SettingsTab::Triggers => {
            state.selected_row = state.selected_row.saturating_sub(1);
            let choice_count = crate::trigger_settings::TriggerEvent::ALL[state.selected_row]
                .available_actions()
                .len()
                + 1;
            state.trigger_action_cursor = state.trigger_action_cursor.min(choice_count - 1);
        }
        KeyCode::Down | KeyCode::Char('j') if state.tab == SettingsTab::Triggers => {
            state.selected_row = (state.selected_row + 1).min(
                crate::trigger_settings::TriggerEvent::ALL
                    .len()
                    .saturating_sub(1),
            );
            let choice_count = crate::trigger_settings::TriggerEvent::ALL[state.selected_row]
                .available_actions()
                .len()
                + 1;
            state.trigger_action_cursor = state.trigger_action_cursor.min(choice_count - 1);
        }
        KeyCode::Left | KeyCode::Char('h') if state.tab == SettingsTab::Triggers => {
            state.trigger_action_cursor = state.trigger_action_cursor.saturating_sub(1);
        }
        KeyCode::Right | KeyCode::Char('l') if state.tab == SettingsTab::Triggers => {
            let event = crate::trigger_settings::TriggerEvent::ALL[state.selected_row];
            state.trigger_action_cursor =
                (state.trigger_action_cursor + 1).min(event.available_actions().len());
        }
        KeyCode::Enter | KeyCode::Char(' ') if state.tab == SettingsTab::Triggers => {
            let event = crate::trigger_settings::TriggerEvent::ALL[state.selected_row];
            let action = state
                .trigger_action_cursor
                .checked_sub(1)
                .and_then(|index| event.available_actions().get(index).copied());
            app.settings_toggle_trigger_action(event, action);
        }
        KeyCode::Up | KeyCode::Char('k') if state.tab == SettingsTab::VoiceControl => {
            state.selected_row = state.selected_row.saturating_sub(1);
        }
        KeyCode::Down | KeyCode::Char('j') if state.tab == SettingsTab::VoiceControl => {
            state.selected_row = (state.selected_row + 1)
                .min(crate::voice_settings::VoiceRow::ALL.len().saturating_sub(1));
        }
        KeyCode::Left | KeyCode::Char('h') if state.tab == SettingsTab::VoiceControl => {
            if let Some(row) = crate::voice_settings::VoiceRow::ALL
                .get(state.selected_row)
                .copied()
            {
                app.mode = Mode::Settings(state);
                app.settings_adjust_voice_row(row, -1);
                return;
            }
        }
        KeyCode::Right | KeyCode::Char('l') | KeyCode::Enter | KeyCode::Char(' ')
            if state.tab == SettingsTab::VoiceControl =>
        {
            if let Some(row) = crate::voice_settings::VoiceRow::ALL
                .get(state.selected_row)
                .copied()
            {
                app.mode = Mode::Settings(state);
                app.settings_adjust_voice_row(row, 1);
                return;
            }
        }
        KeyCode::Up | KeyCode::Char('k') if state.tab == SettingsTab::Debug => {
            state.selected_row = state.selected_row.saturating_sub(1);
        }
        KeyCode::Down | KeyCode::Char('j') if state.tab == SettingsTab::Debug => {
            state.selected_row =
                (state.selected_row + 1).min(crate::app::DebugRow::ALL.len().saturating_sub(1));
        }
        KeyCode::Left
        | KeyCode::Char('h')
        | KeyCode::Right
        | KeyCode::Char('l')
        | KeyCode::Enter
        | KeyCode::Char(' ')
            if state.tab == SettingsTab::Debug =>
        {
            if matches!(
                crate::app::DebugRow::ALL.get(state.selected_row),
                Some(crate::app::DebugRow::FileLogging)
            ) {
                app.settings_toggle_file_logging();
            }
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
        KeyCode::Up | KeyCode::Char('k') if state.tab == SettingsTab::Terminal => {
            state.selected_row = state.selected_row.saturating_sub(1)
        }
        KeyCode::Down | KeyCode::Char('j') if state.tab == SettingsTab::Terminal => {
            state.selected_row =
                (state.selected_row + 1).min(crate::app::TerminalRow::ALL.len() - 1)
        }
        KeyCode::Left | KeyCode::Char('h') if state.tab == SettingsTab::Terminal => {
            if let Some(row) = crate::app::TerminalRow::ALL
                .get(state.selected_row)
                .copied()
            {
                app.settings_adjust_terminal_row(row, -1);
            }
        }
        KeyCode::Right | KeyCode::Char('l') | KeyCode::Enter | KeyCode::Char(' ')
            if state.tab == SettingsTab::Terminal =>
        {
            if let Some(row) = crate::app::TerminalRow::ALL
                .get(state.selected_row)
                .copied()
            {
                app.settings_adjust_terminal_row(row, 1);
            }
        }
        KeyCode::Up | KeyCode::Char('k') if state.tab == SettingsTab::Editor => {
            state.selected_row = state.selected_row.saturating_sub(1)
        }
        KeyCode::Down | KeyCode::Char('j') if state.tab == SettingsTab::Editor => {
            state.selected_row = (state.selected_row + 1).min(crate::app::EditorRow::ALL.len() - 1)
        }
        KeyCode::Left | KeyCode::Char('h') if state.tab == SettingsTab::Editor => {
            if let Some(row) = crate::app::EditorRow::ALL.get(state.selected_row).copied() {
                app.settings_adjust_editor_row(row, -1);
            }
        }
        KeyCode::Right | KeyCode::Char('l') | KeyCode::Enter | KeyCode::Char(' ')
            if state.tab == SettingsTab::Editor =>
        {
            if let Some(row) = crate::app::EditorRow::ALL.get(state.selected_row).copied() {
                app.settings_adjust_editor_row(row, 1);
            }
        }
        KeyCode::Left
        | KeyCode::Char('h')
        | KeyCode::Right
        | KeyCode::Char('l')
        | KeyCode::Enter
        | KeyCode::Char(' ')
            if state.tab == SettingsTab::Session =>
        {
            if let Some(row) = crate::app::SessionRow::ALL.get(state.selected_row).copied() {
                app.settings_adjust_session_row(row, 1);
            }
        }
        KeyCode::Up | KeyCode::Char('k') if state.tab == SettingsTab::Keyboard => {
            state.selected_row = state.selected_row.saturating_sub(1);
        }
        KeyCode::Down | KeyCode::Char('j') if state.tab == SettingsTab::Keyboard => {
            state.selected_row = (state.selected_row + 1).min(
                usize::from(crate::settings_ui::KEYBOARD_TABLE_FIRST_ROW)
                    .saturating_add(app.keybindings.len())
                    .saturating_sub(1),
            );
        }
        KeyCode::Left | KeyCode::Char('h')
            if state.tab == SettingsTab::Keyboard && state.selected_row == 0 =>
        {
            app.settings_adjust_shortcut_base(-1);
        }
        KeyCode::Right | KeyCode::Char('l') | KeyCode::Enter | KeyCode::Char(' ')
            if state.tab == SettingsTab::Keyboard && state.selected_row == 0 =>
        {
            app.settings_adjust_shortcut_base(1);
        }
        KeyCode::Left | KeyCode::Char('h')
            if state.tab == SettingsTab::Keyboard && state.selected_row == 1 =>
        {
            app.settings_adjust_navigation_shortcut_base(-1);
        }
        KeyCode::Right | KeyCode::Char('l') | KeyCode::Enter | KeyCode::Char(' ')
            if state.tab == SettingsTab::Keyboard && state.selected_row == 1 =>
        {
            app.settings_adjust_navigation_shortcut_base(1);
        }
        KeyCode::Enter | KeyCode::Char(' ')
            if state.tab == SettingsTab::Keyboard
                && state.selected_row
                    >= usize::from(crate::settings_ui::KEYBOARD_TABLE_FIRST_ROW) =>
        {
            if let Some(binding) = app
                .keybindings
                .get(state.selected_row - usize::from(crate::settings_ui::KEYBOARD_TABLE_FIRST_ROW))
            {
                state.keyboard_picker = Some(binding.action);
            }
        }
        KeyCode::Char('1') if state.tab == SettingsTab::Keyboard => {
            app.settings_apply_keymap_preset(crate::keymap::KeymapPreset::Screen);
        }
        KeyCode::Char('2') if state.tab == SettingsTab::Keyboard => {
            app.settings_apply_keymap_preset(crate::keymap::KeymapPreset::Tmux);
        }
        KeyCode::Up | KeyCode::Char('k') if state.tab == SettingsTab::KanbanBoard => {
            state.selected_row = state.selected_row.saturating_sub(1);
        }
        KeyCode::Down | KeyCode::Char('j') if state.tab == SettingsTab::KanbanBoard => {
            state.selected_row =
                (state.selected_row + 1).min(KanbanBoardRow::ALL.len().saturating_sub(1));
        }
        KeyCode::Left | KeyCode::Char('h') if state.tab == SettingsTab::KanbanBoard => {
            if let Some(row) = KanbanBoardRow::ALL.get(state.selected_row).copied() {
                app.settings_adjust_kanban_board_row(row, -1);
            }
        }
        KeyCode::Right | KeyCode::Char('l') | KeyCode::Enter | KeyCode::Char(' ')
            if state.tab == SettingsTab::KanbanBoard =>
        {
            if let Some(row) = KanbanBoardRow::ALL.get(state.selected_row).copied() {
                app.settings_adjust_kanban_board_row(row, 1);
            }
        }
        KeyCode::Up | KeyCode::Char('k') if state.tab == SettingsTab::Sound => {
            state.selected_row = state.selected_row.saturating_sub(1);
        }
        KeyCode::Down | KeyCode::Char('j') if state.tab == SettingsTab::Sound => {
            state.selected_row =
                (state.selected_row + 1).min(SoundRow::ALL.len().saturating_sub(1));
        }
        KeyCode::Left | KeyCode::Char('h') if state.tab == SettingsTab::Sound => {
            if let Some(row) = SoundRow::ALL.get(state.selected_row).copied() {
                app.settings_adjust_sound_row(row, -1);
            }
        }
        KeyCode::Right | KeyCode::Char('l') | KeyCode::Enter | KeyCode::Char(' ')
            if state.tab == SettingsTab::Sound =>
        {
            if let Some(row) = SoundRow::ALL.get(state.selected_row).copied() {
                app.settings_adjust_sound_row(row, 1);
            }
        }
        KeyCode::PageUp => state.scroll = state.scroll.saturating_sub(SETTINGS_PAGE_SCROLL_LINES),
        KeyCode::PageDown => state.scroll = state.scroll.saturating_add(SETTINGS_PAGE_SCROLL_LINES),
        _ => {}
    }

    let content_area = crate::settings_ui::compute_layout(app.layout.screen_area).content_area;
    if state.tab == SettingsTab::Triggers {
        state.scroll = crate::trigger_settings_ui::scroll_for_selection(
            app,
            content_area,
            state.selected_row,
            state.trigger_action_cursor,
            state.scroll,
        );
    }
    let max_scroll =
        crate::settings_ui::max_scroll(state.tab, app, state.selected_row, content_area);
    state.scroll = state.scroll.min(max_scroll);
    app.mode = Mode::Settings(state);
}

#[cfg(test)]
mod indent_outdent_tests {
    use super::*;
    use crossterm::event::KeyModifiers;
    use std::path::PathBuf;

    #[test]
    fn f8_toggles_semantic_vad_and_preserves_push_to_talk_edges() {
        let mut app = App::new("test".to_string(), PathBuf::from("/tmp"));

        handle_event(
            &mut app,
            Event::Key(KeyEvent::new_with_kind(
                KeyCode::F(8),
                KeyModifiers::NONE,
                KeyEventKind::Press,
            )),
        );
        assert!(app.voice_settings.enabled);
        assert_eq!(
            app.take_voice_runtime_request(),
            Some(crate::app::VoiceRuntimeRequest::Start)
        );

        app.voice_settings.input_mode = ilium_voice::VoiceInputMode::PushToTalk;
        handle_event(
            &mut app,
            Event::Key(KeyEvent::new_with_kind(
                KeyCode::F(8),
                KeyModifiers::NONE,
                KeyEventKind::Press,
            )),
        );
        handle_event(
            &mut app,
            Event::Key(KeyEvent::new_with_kind(
                KeyCode::F(8),
                KeyModifiers::NONE,
                KeyEventKind::Release,
            )),
        );
        assert_eq!(
            app.take_voice_interaction_requests(),
            vec![
                crate::app::VoiceInteractionRequest::StartPushToTalk,
                crate::app::VoiceInteractionRequest::StopPushToTalk,
            ]
        );
    }

    #[test]
    fn debug_log_s_key_opens_path_prompt_and_enter_saves_complete_report() {
        let directory = tempfile::tempdir().expect("temporary project");
        let mut app = App::new("test".to_string(), directory.path().to_path_buf());
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        let pane_id = app
            .tree
            .add_pane(group, "codex agent", ilium_core::PaneContentKind::Terminal)
            .unwrap();
        app.agent_debug_logs.insert(
            pane_id,
            crate::app::AgentDebugLogCache {
                has_loaded_retained_history: true,
                ..crate::app::AgentDebugLogCache::default()
            },
        );
        app.mode = Mode::AgentDebugLog(crate::app::AgentDebugLogViewState {
            pane_id,
            scroll_position: crate::app::AgentDebugLogScrollPosition::FromNewest(0),
        });

        handle_event(
            &mut app,
            Event::Key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE)),
        );

        let Mode::AgentDebugSavePath(target, prompt) = &app.mode else {
            panic!("S should open the editable debug-log destination prompt");
        };
        assert_eq!(*target, pane_id);
        assert!(prompt
            .buf
            .starts_with(&directory.path().display().to_string()));
        assert!(prompt.buf.ends_with(".log"));

        let export_path = directory.path().join("keyboard-export.log");
        app.mode = Mode::AgentDebugSavePath(
            pane_id,
            TextPromptState::new(export_path.display().to_string()),
        );
        handle_event(
            &mut app,
            Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        );

        assert!(matches!(app.mode, Mode::AgentDebugLog(_)));
        assert!(std::fs::read_to_string(export_path)
            .unwrap()
            .starts_with("ILIUM AGENT DEBUG LOG\n"));
    }

    #[test]
    fn debug_log_r_key_reveals_panel_animation_resize_events() {
        let mut app = App::new("test".to_string(), PathBuf::from("/tmp"));
        let pane_id = ilium_core::NodeId(7);
        app.mode = Mode::AgentDebugLog(crate::app::AgentDebugLogViewState {
            pane_id,
            scroll_position: crate::app::AgentDebugLogScrollPosition::FromNewest(0),
        });
        assert!(
            app.agent_debug_log_filter
                .is_tree_panel_animation_resize_hidden
        );

        handle_event(
            &mut app,
            Event::Key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE)),
        );

        assert!(matches!(app.mode, Mode::AgentDebugLog(_)));
        assert!(
            !app.agent_debug_log_filter
                .is_tree_panel_animation_resize_hidden
        );
    }

    #[test]
    fn search_typing_updates_the_visible_prompt_without_running_a_scan() {
        let mut app = App::new("test".to_string(), PathBuf::from("/tmp"));
        app.action_open_search();

        handle_event(
            &mut app,
            Event::Key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE)),
        );

        let Mode::Search(state) = &app.mode else {
            panic!("typing should remain in workspace search");
        };
        assert_eq!(state.query.buf, "n");
        assert!(state.results.is_empty());
        assert!(state.is_waiting_for_debounce());
    }

    #[test]
    fn icon_picker_queues_cpu_semantic_search_and_selects_its_result() {
        let mut app = App::new("test".to_string(), PathBuf::from("/tmp"));
        app.set_screen_area(ratatui::layout::Rect::new(0, 0, 160, 45));
        app.mode = Mode::Settings(SettingsState {
            tab: SettingsTab::Icons,
            icon_picker: Some(crate::app::IconPickerState::new(IconTarget::Group)),
            ..SettingsState::default()
        });

        handle_event(
            &mut app,
            Event::Key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE)),
        );
        for character in "rocket".chars() {
            handle_event(
                &mut app,
                Event::Key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE)),
            );
        }

        let Mode::Settings(state) = &app.mode else {
            panic!("icon picker should remain open while searching");
        };
        let picker = state.icon_picker.as_ref().expect("the picker remains open");
        assert_eq!(picker.search_query, "rocket");
        assert!(matches!(
            picker.search_status,
            crate::app::IconPickerSearchStatus::Searching
        ));
        let request = app
            .take_pending_icon_semantic_search()
            .expect("typing should queue the latest semantic query");
        assert_eq!(request.query, "rocket");
        app.apply_icon_semantic_search_event(
            crate::icon_search_workers::IconSemanticSearchEvent::Results {
                revision: request.revision,
                results: crate::icon_settings::semantic_picker_search_results(vec![
                    crate::icon_settings::IconSemanticSearchHit {
                        category_label: "Travel · Air",
                        family: crate::icon_settings::IconCatalogFamily::OfficialUtf8,
                        entry: crate::icon_settings::IconCatalogEntry {
                            glyph: "🚀",
                            name: "rocket",
                        },
                    },
                ]),
            },
        );

        handle_event(
            &mut app,
            Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        );
        handle_event(
            &mut app,
            Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        );

        assert!(matches!(app.mode, Mode::Settings(_)));
        assert_eq!(app.ui_settings.icons.group, "🚀");
    }

    #[test]
    fn icon_picker_v_switches_between_dense_and_single_column_modes() {
        let mut app = App::new("test".to_string(), PathBuf::from("/tmp"));
        app.set_screen_area(ratatui::layout::Rect::new(0, 0, 160, 45));
        app.mode = Mode::Settings(SettingsState {
            tab: SettingsTab::Icons,
            icon_picker: Some(crate::app::IconPickerState::new(IconTarget::Group)),
            ..SettingsState::default()
        });

        handle_event(
            &mut app,
            Event::Key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE)),
        );
        let Mode::Settings(state) = &app.mode else {
            panic!("view switching must keep the picker open");
        };
        assert_eq!(
            state
                .icon_picker
                .as_ref()
                .expect("picker state")
                .column_mode,
            crate::app::IconPickerColumnMode::SingleColumn
        );

        handle_event(
            &mut app,
            Event::Key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE)),
        );
        let Mode::Settings(state) = &app.mode else {
            panic!("view switching must keep the picker open");
        };
        assert_eq!(
            state
                .icon_picker
                .as_ref()
                .expect("picker state")
                .column_mode,
            crate::app::IconPickerColumnMode::MultiColumn
        );
    }

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
    fn changing_the_shortcut_base_updates_dispatch_immediately() {
        let mut app = App::new("test".to_string(), PathBuf::from("/tmp"));
        let ctrl_a = Event::Key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL));
        let ctrl_b = Event::Key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL));

        handle_event(&mut app, ctrl_a.clone());
        assert!(matches!(app.mode, Mode::LeaderPending));
        app.mode = Mode::Normal;

        handle_event(&mut app, ctrl_b.clone());
        assert!(matches!(app.mode, Mode::NavigationLeaderPending));
        app.mode = Mode::Normal;

        app.settings_set_shortcut_base(crate::keymap::ShortcutBase::B);
        handle_event(&mut app, ctrl_b);
        assert!(matches!(app.mode, Mode::LeaderPending));
        handle_event(&mut app, ctrl_a);
        assert!(matches!(app.mode, Mode::Normal));

        app.mode = Mode::Normal;
        app.settings_set_navigation_shortcut_base(crate::keymap::ShortcutBase::A);
        handle_event(
            &mut app,
            Event::Key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL)),
        );
        assert!(matches!(app.mode, Mode::NavigationLeaderPending));
    }

    #[test]
    fn ctrl_b_arrow_and_page_defaults_dispatch_group_navigation_actions() {
        let mut app = App::new("test".to_string(), PathBuf::from("/tmp"));
        let first_group = app.tree.add_group(ROOT_ID, "first").unwrap();
        let first = app
            .tree
            .add_pane(first_group, "first", ilium_core::PaneContentKind::Terminal)
            .unwrap();
        let second = app
            .tree
            .add_pane(first_group, "second", ilium_core::PaneContentKind::Editor)
            .unwrap();
        let next_group = app.tree.add_group(ROOT_ID, "next").unwrap();
        let next = app
            .tree
            .add_pane(next_group, "next", ilium_core::PaneContentKind::Board)
            .unwrap();
        app.focus_pane(first);
        app.take_outbound_requests();

        handle_event(
            &mut app,
            Event::Key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL)),
        );
        handle_event(
            &mut app,
            Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)),
        );
        assert_eq!(app.active_pane_id(), Some(second));

        handle_event(
            &mut app,
            Event::Key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL)),
        );
        handle_event(
            &mut app,
            Event::Key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE)),
        );
        assert_eq!(app.active_pane_id(), Some(next));
    }

    #[test]
    fn board_dialog_types_plain_p_and_reserves_control_p_for_browse() {
        let mut app = App::new("test".to_string(), PathBuf::from("/tmp"));
        app.mode = Mode::CreateBoard(CreateBoardState {
            parent_group: ilium_core::ROOT_ID,
            name: crate::text_prompt::TextPromptState::new(""),
            path: crate::text_prompt::TextPromptState::new("/tmp/board.md"),
            storage_kind: BoardStorageKind::MarkdownFile,
            editing_path: false,
        });

        handle_event(
            &mut app,
            Event::Key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE)),
        );
        let Mode::CreateBoard(state) = &app.mode else {
            panic!("plain p should keep editing the board form");
        };
        assert_eq!(state.name.buf, "p");

        handle_event(
            &mut app,
            Event::Key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL)),
        );
        assert!(matches!(app.mode, Mode::BoardPathPicker(_)));
        assert!(
            matches!(app.modal_stack.as_slice(), [Mode::CreateBoard(state)] if state.name.buf == "p")
        );

        handle_event(
            &mut app,
            Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        );
        assert!(matches!(&app.mode, Mode::CreateBoard(state) if state.name.buf == "p"));
        assert!(app.modal_stack.is_empty());
    }

    #[test]
    fn voice_api_key_prompt_preserves_exact_settings_parent_until_commit() {
        let mut app = App::new("test".to_string(), PathBuf::from("/tmp"));
        app.mode = Mode::Settings(SettingsState {
            tab: SettingsTab::VoiceControl,
            selected_row: 1,
            scroll: 7,
            ..SettingsState::default()
        });

        handle_event(
            &mut app,
            Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        );
        assert!(matches!(app.mode, Mode::VoiceSettingPrompt(_, _)));
        assert!(matches!(
            app.modal_stack.as_slice(),
            [Mode::Settings(state)]
                if state.tab == SettingsTab::VoiceControl
                    && state.selected_row == 1
                    && state.scroll == 7
        ));

        for character in "sk-live-test".chars() {
            handle_event(
                &mut app,
                Event::Key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE)),
            );
        }
        handle_event(
            &mut app,
            Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        );

        assert_eq!(app.voice_settings.api_key, "sk-live-test");
        assert!(matches!(
            &app.mode,
            Mode::Settings(state)
                if state.tab == SettingsTab::VoiceControl
                    && state.selected_row == 1
                    && state.scroll == 7
        ));
        assert!(app.modal_stack.is_empty());
    }

    #[test]
    fn modal_stack_restores_arbitrary_depth_in_last_opened_first_closed_order() {
        let mut app = App::new("test".to_string(), PathBuf::from("/tmp"));
        app.mode = Mode::Settings(SettingsState {
            tab: SettingsTab::VoiceControl,
            selected_row: 1,
            ..SettingsState::default()
        });
        app.push_modal(Mode::VoiceSettingPrompt(
            crate::voice_settings::VoiceSettingField::ApiKey,
            TextPromptState::new(""),
        ));
        app.push_modal(Mode::ConfirmSessionRecovery { pane_count: 2 });

        assert_eq!(app.modal_stack.len(), 2);
        app.pop_modal();
        assert!(matches!(app.mode, Mode::VoiceSettingPrompt(_, _)));
        app.pop_modal();
        assert!(matches!(
            app.mode,
            Mode::Settings(SettingsState {
                tab: SettingsTab::VoiceControl,
                selected_row: 1,
                ..
            })
        ));
        assert!(app.modal_stack.is_empty());
    }

    #[test]
    fn inference_field_and_file_action_menu_restore_their_exact_parents() {
        let mut app = App::new("test".to_string(), PathBuf::from("/tmp"));
        app.inference_settings.selected_provider = ilium_inference::InferenceProviderKind::OpenAi;
        let selected_row = crate::settings_ui::inference_rows(&app.inference_settings)
            .iter()
            .position(|row| {
                *row == crate::app::InferenceRow::Field(
                    crate::app::InferenceSettingField::OpenAiApiKey,
                )
            })
            .expect("OpenAI API key row");
        app.mode = Mode::Settings(SettingsState {
            tab: SettingsTab::Inference,
            selected_row,
            scroll: 3,
            ..SettingsState::default()
        });

        handle_event(
            &mut app,
            Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        );
        assert!(matches!(app.mode, Mode::InferenceSettingPrompt(_, _)));
        handle_event(
            &mut app,
            Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        );
        assert!(matches!(
            &app.mode,
            Mode::Settings(state)
                if state.tab == SettingsTab::Inference
                    && state.selected_row == selected_row
                    && state.scroll == 3
        ));

        let overlay = crate::explorer_overlay::ExplorerOverlay::open_at(&PathBuf::from("/tmp"))
            .expect("open temporary explorer");
        app.open_explorer_file_menu(
            Box::new(overlay),
            ilium_core::ROOT_ID,
            PathBuf::from("/tmp/example.md"),
            ratatui::layout::Position::new(2, 2),
        );
        assert!(matches!(app.mode, Mode::ExplorerFileMenu(_)));
        assert!(matches!(app.modal_stack.last(), Some(Mode::Explorer(_, _))));

        handle_event(
            &mut app,
            Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        );
        assert!(matches!(app.mode, Mode::Explorer(_, _)));
        assert!(app.modal_stack.is_empty());
    }

    #[test]
    fn triggers_keyboard_navigation_toggles_chips_and_none_clears_actions() {
        let mut app = App::new("test".to_owned(), PathBuf::from("/tmp"));
        app.mode = Mode::Settings(SettingsState {
            tab: SettingsTab::Triggers,
            selected_row: 6,
            trigger_action_cursor: 0,
            ..SettingsState::default()
        });

        handle_event(
            &mut app,
            Event::Key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE)),
        );
        handle_event(
            &mut app,
            Event::Key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE)),
        );
        assert!(!app
            .trigger_settings
            .agent_finished_work
            .contains(&crate::trigger_settings::TriggerAction::RetitleElement));
        assert!(app
            .trigger_settings
            .agent_finished_work
            .contains(&crate::trigger_settings::TriggerAction::RestructureProject));

        handle_event(
            &mut app,
            Event::Key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE)),
        );
        handle_event(
            &mut app,
            Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        );
        assert!(app.trigger_settings.agent_finished_work.is_empty());
    }

    #[test]
    fn context_menu_keyboard_opens_order_submenu_and_applies_selection() {
        let mut app = App::new("test".to_string(), PathBuf::from("/tmp"));
        app.set_screen_area(ratatui::layout::Rect::new(0, 0, 100, 30));
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        let pane = app
            .tree
            .add_pane(group, "shell", ilium_core::PaneContentKind::Terminal)
            .unwrap();
        app.open_context_menu(pane, 2, 2);
        let Mode::ContextMenu(menu) = &mut app.mode else {
            panic!("context menu should be open");
        };
        menu.selected_index = menu
            .actions
            .iter()
            .position(|action| *action == crate::app::ContextMenuAction::OrderBy)
            .unwrap();

        handle_event(
            &mut app,
            Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        );
        assert!(matches!(
            app.mode,
            Mode::ContextMenu(crate::app::ContextMenu {
                tree_order_submenu: Some(_),
                ..
            })
        ));

        handle_event(
            &mut app,
            Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)),
        );
        handle_event(
            &mut app,
            Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        );

        assert_eq!(app.ui_settings.tree_order, crate::config::TreeOrder::Type);
        assert!(matches!(app.mode, Mode::Normal));
    }

    #[test]
    fn scheduled_input_keyboard_edits_toggles_and_submits_the_form() {
        let mut app = App::new("test".to_string(), PathBuf::from("/tmp"));
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        let pane_id = app
            .tree
            .add_pane(group, "shell", ilium_core::PaneContentKind::Terminal)
            .unwrap();
        let mut state = ScheduledInputDialogState::new(pane_id);
        state.focus = ScheduledInputFocus::Text;
        app.mode = Mode::SchedulePaneInput(Box::new(state));

        handle_event(
            &mut app,
            Event::Key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE)),
        );
        handle_event(
            &mut app,
            Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)),
        );
        handle_event(
            &mut app,
            Event::Key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE)),
        );
        handle_event(
            &mut app,
            Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL)),
        );

        assert_eq!(
            app.take_outbound_requests(),
            vec![ClientRequest::SchedulePaneInput {
                pane_id,
                delay_seconds: 30,
                text: "x".to_string(),
                send_enter: false,
            }]
        );
        assert!(matches!(app.mode, Mode::Normal));
    }

    #[test]
    fn atomic_paste_reuses_existing_character_input_for_text_prompt_modes() {
        let mut app = App::new("test".to_string(), PathBuf::from("/tmp"));
        app.mode = Mode::Rename(TextPromptState::new("before-"));

        handle_event(&mut app, Event::Paste("café 世界".to_string()));

        let Mode::Rename(state) = &app.mode else {
            panic!("pasting text should keep the rename prompt open");
        };
        assert_eq!(state.buf, "before-café 世界");
    }

    #[test]
    fn split_orientation_dialog_can_skip_the_optional_member_picker() {
        let mut app = App::new("test".to_string(), PathBuf::from("/tmp"));
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        app.tree_state.select(vec![group]);
        app.open_create_split_dialog();

        handle_event(
            &mut app,
            Event::Key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE)),
        );

        assert!(matches!(app.mode, Mode::Normal));
        assert!(matches!(
            app.take_outbound_requests().as_slice(),
            [ClientRequest::CreateSplitView {
                parent_group,
                orientation: ilium_core::SplitOrientation::Vertical,
                pane_ids,
                ..
            }] if *parent_group == group && pane_ids.is_empty()
        ));
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
    fn outdent_allows_a_split_view_out_of_a_top_level_group_to_the_root() {
        // ROOT -> top (group) -> sv (split view, no panes yet). The root
        // itself is a group (`ilium_core::Tree`'s doc comment), so a split
        // view landing directly under it is a request `Tree::move_node`
        // actually accepts -- only panes are barred from the bare root.
        let mut tree = Tree::new();
        let top = tree.add_group(ROOT_ID, "top").unwrap();
        let split_view = tree
            .create_split_view(top, "split", ilium_core::SplitOrientation::Vertical, &[])
            .unwrap();
        assert_eq!(
            compute_outdent_target(&tree, split_view),
            Some((ROOT_ID, Some(1)))
        );
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

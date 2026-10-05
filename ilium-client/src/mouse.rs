//! Mouse dispatch: routes crossterm mouse events according to
//! `App::mode`/the last-rendered layout, mirroring the pre-client/server
//! `App::handle_mouse_event`'s structure. Terminal-pane clicks/drags become
//! a queued `MouseInput` request (see `to_ipc_mouse_event`); tree-panel
//! selection, hover, and modal/editor-chrome interaction stay purely local,
//! except for a tree-row drag-and-drop, which queues a `ReparentNode`
//! request (see `compute_drop_target`) the same way any other structural
//! tree edit does.

use std::time::{Duration, Instant};

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ilium_core::{AgentProvider, NodeId, NodeKind, Tree, ROOT_ID};
use ratatui::layout::{Position, Rect};

use crate::agent_from_line::{CreateAgentFocus, CreateAgentFromLineState, EditorLineContextMenu};
use crate::agent_toolbar::AgentToolbarAction;
use crate::app::{
    AgentToolbarModelSubmenuState, App, ContextMenu, CreateGroupState, Mode, RightPanelTarget,
};
use crate::explorer_overlay::{ExplorerOutcome, ExplorerOverlay};
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

/// Environment variable naming a file to append one line per mouse event to.
///
/// Off unless set, and set by nothing in normal use. It exists because a
/// terminal host can deliver mouse input differently enough that a surface
/// stops responding while everything visible looks correct, and from outside
/// the process that is indistinguishable from the surface ignoring a click it
/// did receive. Nothing else in the client can answer which of the two it is.
pub const MOUSE_TRACE_FILE_ENV: &str = "ILIUM_MOUSE_TRACE_FILE";

/// Appends what the client actually received, and the mode it arrived in, when
/// [`MOUSE_TRACE_FILE_ENV`] names a file. Best-effort: a diagnostic that fails
/// must never disturb the input path it is observing.
fn trace_mouse_event(app: &App, mouse: &MouseEvent) {
    use std::io::Write;

    let Some(path) = std::env::var_os(MOUSE_TRACE_FILE_ENV) else {
        return;
    };
    // Only the mode this trace exists to explain carries detail; the rest
    // just need to be distinguishable, and `Mode` is not `Debug`.
    let mode = match &app.mode {
        Mode::ContextMenu(menu) => format!("ContextMenu(area={:?})", menu.area),
        Mode::Normal => "Normal".to_string(),
        _ => "other".to_string(),
    };
    let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    else {
        return;
    };
    let _ = writeln!(
        file,
        "{:?} at column {} row {} in mode {mode}",
        mouse.kind, mouse.column, mouse.row
    );
}

/// Top-level mouse dispatch, called for every `Event::Mouse`.
pub fn handle_mouse_event(app: &mut App, mouse: MouseEvent) {
    if !app.pointer_geometry_is_current() {
        // A completed menu action changes mode before its matching release.
        // Discard that stale release without replacing the action's result.
        if !matches!(mouse.kind, MouseEventKind::Up(_)) {
            app.status_message =
                Some("Waiting for terminal presentation before pointer input".into());
        }
        // Keep the latest press so a click made just after a state change (a
        // menu item right after the menu opened) is not lost.
        if matches!(mouse.kind, MouseEventKind::Down(_)) {
            app.deferred_pointer_press = Some((mouse, app.layout));
        }
        return;
    }
    if crate::onboarding::integration::handle_mouse(app, mouse) {
        return;
    }
    if app.handle_plugin_permission_mouse(mouse) {
        return;
    }
    trace_mouse_event(app, &mouse);
    let position = Position::new(mouse.column, mouse.row);
    app.set_terminal_focused(true);
    app.set_pointer_position(Some(position));
    if !app.layout.tree_area.contains(position) {
        app.update_agent_popover_pointer(position, Instant::now());
    }

    if ends_tree_double_click_pair(app, &mouse, position) {
        app.last_tree_click = None;
    }

    if matches!(app.mode, Mode::ValueDialog(_)) {
        let Mode::ValueDialog(mut host) = std::mem::replace(&mut app.mode, Mode::Normal) else {
            unreachable!("matched value dialog");
        };
        let screen = app.layout.screen_area;
        let outcome = match mouse.kind {
            _ if host.is_saving() => crate::value_dialog::DialogOutcome::Continue,
            MouseEventKind::Down(MouseButton::Left) => host.dialog.handle_pointer(
                screen,
                position,
                crate::value_control::PointerButton::Left,
            ),
            MouseEventKind::Down(MouseButton::Right) => host.dialog.handle_pointer(
                screen,
                position,
                crate::value_control::PointerButton::Right,
            ),
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                match &mut host.dialog {
                    crate::value_dialog::ValueDialogState::Choice(choice)
                        if crate::value_dialog::dialog_layout(screen)
                            .document
                            .contains(position) =>
                    {
                        choice.scroll_by(
                            screen,
                            if mouse.kind == MouseEventKind::ScrollUp {
                                -3
                            } else {
                                3
                            },
                        )
                    }
                    _ => {}
                }
                crate::value_dialog::DialogOutcome::Continue
            }
            _ => crate::value_dialog::DialogOutcome::Continue,
        };
        app.finish_value_dialog(host, outcome);
        return;
    }

    // Holding the Smart Copy light key over a terminal starts the model-free
    // selection mode; the triggering event is consumed.
    if app.try_start_smart_copy_light(&mouse, position) {
        return;
    }

    // Smart Copy owns mouse events for the frozen pane, including releases
    // from a pre-existing tree/chatroom drag gesture. Route it before any
    // other stateful mouse owner can swallow the selection click.
    if matches!(app.mode, Mode::SmartCopy) {
        handle_smart_copy_mouse(app, mouse, position);
        return;
    }

    // A settings slider retains its gesture through release, including when
    // the pointer crosses another globally interactive surface.
    if matches!(&app.mode, Mode::Settings(state)
        if state.tab == crate::app::SettingsTab::Animations && state.animation_slider_drag.is_some())
        && matches!(
            mouse.kind,
            MouseEventKind::Drag(MouseButton::Left) | MouseEventKind::Up(MouseButton::Left)
        )
    {
        let Mode::Settings(state) = std::mem::replace(&mut app.mode, Mode::Normal) else {
            unreachable!("just matched owned Animations slider gesture above");
        };
        handle_settings_mouse(app, state, mouse);
        return;
    }

    // An active scrollbar drag retains ownership even when the pointer leaves
    // the chatroom panel, so releasing over the tree or status row cannot
    // leave the drag latched or route the same gesture into another surface.
    if app.is_chatroom_scrollbar_dragging()
        && matches!(
            mouse.kind,
            MouseEventKind::Drag(MouseButton::Left) | MouseEventKind::Up(MouseButton::Left)
        )
    {
        app.handle_chatroom_mouse(mouse, position);
        return;
    }

    // The voice control is global and rendered above every full-screen view,
    // so its hit-test must precede modal dispatch as well.
    if app.layout.voice_control_area.contains(position)
        && matches!(mouse.kind, MouseEventKind::Up(MouseButton::Left))
    {
        app.toggle_voice_control();
        return;
    }

    // A drag gesture that began on a tree row likewise retains ownership
    // wherever the pointer travels (keyed off the press itself, so even the
    // first drag event after the press cannot slip through): drag motion
    // outside the tree must not leak into a terminal pane as PTY mouse
    // input, and a release outside the tree is the drag's cancellation --
    // never a click on whatever surface (pane, voice control) sits there.
    if app.drag_source().is_some()
        && matches!(
            mouse.kind,
            MouseEventKind::Drag(MouseButton::Left) | MouseEventKind::Up(MouseButton::Left)
        )
    {
        if app.layout.tree_area.contains(position) {
            handle_tree_mouse(app, mouse, position);
        } else if matches!(mouse.kind, MouseEventKind::Up(MouseButton::Left)) {
            app.clear_tree_drag();
        }
        return;
    }

    // Only actually take `app.mode` out when it's a variant this function
    if matches!(app.mode, Mode::WaitingWorkspaceCloseOffer { .. }) {
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
    if matches!(app.mode, Mode::TerminalPaneContextMenu(_)) {
        let Mode::TerminalPaneContextMenu(menu) = std::mem::replace(&mut app.mode, Mode::Normal)
        else {
            unreachable!("just matched Mode::TerminalPaneContextMenu above");
        };
        handle_terminal_pane_context_menu_mouse(app, menu, mouse);
        return;
    }
    if matches!(app.mode, Mode::AgentDebugLog(_)) {
        handle_agent_debug_log_mouse(app, mouse, position);
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
    if matches!(app.mode, Mode::AgentToolbarModelSubmenu(_)) {
        let Mode::AgentToolbarModelSubmenu(state) = std::mem::replace(&mut app.mode, Mode::Normal)
        else {
            unreachable!("just matched Mode::AgentToolbarModelSubmenu above");
        };
        handle_agent_toolbar_model_submenu_mouse(app, state, mouse);
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
    if matches!(app.mode, Mode::CreateAgentWorkspace(_)) {
        let Mode::CreateAgentWorkspace(state) = std::mem::replace(&mut app.mode, Mode::Normal)
        else {
            unreachable!("just matched Mode::CreateAgentWorkspace above");
        };
        handle_create_agent_workspace_mouse(app, state, mouse);
        return;
    }
    if matches!(app.mode, Mode::WorktreeManager(_)) {
        let Mode::WorktreeManager(state) = std::mem::replace(&mut app.mode, Mode::Normal) else {
            unreachable!("just matched Mode::WorktreeManager above");
        };
        handle_worktree_manager_mouse(app, state, mouse);
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
    if let Mode::TextTriggerDialog(state) = &app.mode {
        let delay = crate::text_trigger_dialog::delay_control(app.layout.screen_area, state);
        if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left))
            && delay.geometry().row.contains(position)
        {
            let Mode::TextTriggerDialog(mut state) = std::mem::replace(&mut app.mode, Mode::Normal)
            else {
                unreachable!("matched trigger dialog");
            };
            state.focus = crate::text_trigger_dialog::TextTriggerFocus::Delay;
            if let Some(action) = delay.hit(position, crate::value_control::PointerButton::Left) {
                if let Err(error) = state.apply_delay_control(action) {
                    app.status_message = Some(error);
                }
            }
            app.mode = Mode::TextTriggerDialog(state);
            return;
        }
        let control = crate::text_trigger_dialog::target_control(app.layout.screen_area, state);
        if control.geometry().row.contains(position)
            && matches!(
                mouse.kind,
                MouseEventKind::Down(MouseButton::Left | MouseButton::Right)
            )
        {
            let Mode::TextTriggerDialog(mut state) = std::mem::replace(&mut app.mode, Mode::Normal)
            else {
                unreachable!("matched trigger dialog");
            };
            state.focus = crate::text_trigger_dialog::TextTriggerFocus::Target;
            let button = if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Right)) {
                crate::value_control::PointerButton::Right
            } else {
                crate::value_control::PointerButton::Left
            };
            match control.hit(position, button) {
                Some(crate::value_control::ControlAction::OpenChoices) => {
                    app.begin_trigger_scope_dialog(state)
                }
                Some(
                    action @ (crate::value_control::ControlAction::NextChoice
                    | crate::value_control::ControlAction::PreviousChoice),
                ) => {
                    let choices = ilium_ipc::TextTriggerTarget::ALL;
                    let index = choices
                        .iter()
                        .position(|value| *value == state.target)
                        .unwrap_or(0);
                    let direction = if action == crate::value_control::ControlAction::PreviousChoice
                    {
                        -1
                    } else {
                        1
                    };
                    state.target = choices
                        [(index as i32 + direction).rem_euclid(choices.len() as i32) as usize];
                    app.mode = Mode::TextTriggerDialog(state);
                }
                _ => app.mode = Mode::TextTriggerDialog(state),
            }
            return;
        }
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
            Mode::BoardPathPicker(mut overlay) => {
                match overlay.handle(&Event::Mouse(mouse), app.layout.screen_area) {
                    Ok(ExplorerOutcome::Picked(path)) => app.return_to_create_board(Some(path)),
                    Ok(_) => app.mode = Mode::BoardPathPicker(overlay),
                    Err(error) => {
                        app.status_message = Some(format!("Board path picker error: {error}"));
                        app.mode = Mode::BoardPathPicker(overlay);
                    }
                }
            }
            _ => unreachable!("folder explorer match must preserve its mode"),
        }
        return;
    }
    if matches!(app.mode, Mode::SettingsHelp(_)) {
        let Mode::SettingsHelp(mut state) = std::mem::replace(&mut app.mode, Mode::Normal) else {
            unreachable!("just matched Mode::SettingsHelp above");
        };
        let layout = crate::settings_help::dialog::layout(app.layout.screen_area);
        let position = Position::new(mouse.column, mouse.row);
        state.focus_panel_at(layout, position.x, position.y);
        match mouse.kind {
            MouseEventKind::ScrollUp => state.scroll_focused_panel(-3),
            MouseEventKind::ScrollDown => state.scroll_focused_panel(3),
            _ => {}
        }
        app.mode = Mode::SettingsHelp(state);
        return;
    }

    if matches!(app.mode, Mode::LocationPicker(_)) {
        handle_location_picker_mouse(app, mouse);
        return;
    }

    if matches!(app.mode, Mode::Settings(_)) {
        let Mode::Settings(state) = std::mem::replace(&mut app.mode, Mode::Normal) else {
            unreachable!("just matched Mode::Settings above");
        };
        handle_settings_mouse(app, state, mouse);
        return;
    }

    if matches!(app.mode, Mode::AgentSetupPrompt(_)) {
        handle_agent_setup_prompt_mouse(app, mouse);
        return;
    }

    if is_shared_action_dialog(&app.mode) {
        handle_shared_action_dialog_mouse(app, mouse);
        return;
    }

    // The conversion dialog is keyboard-only; pointer events must not reach
    // the frozen pane or the tree while it runs.
    if matches!(app.mode, Mode::ConvertSession) {
        return;
    }

    // The remote-compaction dialog is keyboard-only except for the privacy
    // banner's close button; every other pointer event is swallowed.
    if matches!(app.mode, Mode::RemoteCompaction) {
        if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
            let close = app
                .remote_compaction
                .as_ref()
                .map(|state| state.banner_close_rect.get())
                .unwrap_or_default();
            let position = ratatui::layout::Position::new(mouse.column, mouse.row);
            if close.width > 0 && close.contains(position) {
                app.dismiss_remote_compaction_privacy_banner();
            }
        }
        return;
    }

    // Help is a read-only full-screen reference rather than an action
    // dialog, so pointer events stay inert until it has scroll controls.
    if matches!(app.mode, Mode::Help) {
        return;
    }

    // The costs-and-stats popover floats over the pane surface, so it claims
    // its own pointer events before the tree or the pane can see them.
    if app.handle_stats_popover_mouse(mouse, position) {
        return;
    }

    if app.layout.tree_area.contains(position) {
        handle_tree_mouse(app, mouse, position);
        return;
    }
    app.set_hovered_tree_node(None);
    app.set_tree_toolbar_hover(false, None);
    if !app.layout.pane_area.contains(position) {
        app.set_agent_toolbar_hover(None);
    }

    if app.layout.pane_area.contains(position) {
        if matches!(app.right_panel_target, RightPanelTarget::Chatroom { .. }) {
            app.handle_chatroom_mouse(mouse, position);
        } else {
            app.handle_pane_mouse(mouse, position);
        }
        if matches!(mouse.kind, MouseEventKind::Up(MouseButton::Left)) {
            app.clear_tree_drag();
        }
        return;
    }

    // A drag released outside the tree is a cancelled tree move.
    if matches!(mouse.kind, MouseEventKind::Up(MouseButton::Left)) {
        app.clear_tree_drag();
    }
}

fn handle_smart_copy_mouse(app: &mut App, mouse: MouseEvent, position: Position) {
    if let Some(light_key) = app.smart_copy_light_key() {
        // Terminals report held modifiers on mouse events but not the key
        // release itself, so an event without the modifier means "released".
        if !mouse.modifiers.contains(light_key.modifier()) {
            app.finish_smart_copy_light();
            return;
        }
        app.note_smart_copy_light_key_held(Instant::now());
        if app.smart_copy_session.is_none() {
            // The frame is still being captured; nothing to hover yet.
            return;
        }
    }
    let Some(pane_id) = app
        .smart_copy_session
        .as_ref()
        .map(|session| session.pane_id)
    else {
        app.mode = Mode::Normal;
        return;
    };
    let Some(viewport) = app.pane_viewport(pane_id) else {
        app.exit_smart_copy();
        return;
    };
    let content_area = app
        .smart_copy_terminal_area(pane_id)
        .unwrap_or(viewport.content_area);
    if viewport
        .toolbar_area
        .is_some_and(|area| crate::smart_copy::exit_button_rect(area).contains(position))
        && matches!(mouse.kind, MouseEventKind::Up(MouseButton::Left))
    {
        app.exit_smart_copy();
        return;
    }
    app.smart_copy_set_hover(position);
    match mouse.kind {
        MouseEventKind::ScrollUp => app.smart_copy_cycle_overlap(-1),
        MouseEventKind::ScrollDown => app.smart_copy_cycle_overlap(1),
        MouseEventKind::Up(MouseButton::Left) if content_area.contains(position) => {
            app.smart_copy_copy_current();
        }
        _ => {}
    }
}

/// Returns whether the active mode uses `crate::modal`'s shared action row.
fn is_shared_action_dialog(mode: &Mode) -> bool {
    matches!(
        mode,
        Mode::Rename(_)
            | Mode::CommandPrompt(_)
            | Mode::InferenceSettingPrompt(_, _)
            | Mode::VoiceSettingPrompt(_, _)
            | Mode::ApiSettingPrompt(_)
            | Mode::GitSettingPrompt(_, _)
            | Mode::AnimationTextPrompt(_, _)
            | Mode::AgentSetupPathPrompt(_, _)
            | Mode::VoicePromptEditor(_)
            | Mode::SaveAs(..)
            | Mode::AgentDebugSavePath(..)
            | Mode::ConfirmClose(_)
            | Mode::ConfirmWorkspaceCloseOffer(_)
            | Mode::ConfirmRemoveWorkspace(_)
            | Mode::BoardCardPrompt(_, _)
            | Mode::BoardColumnPrompt(_, _)
            | Mode::BoardRenamePrompt(_, _, _)
            | Mode::BoardDeleteConfirm(_, _)
            | Mode::ConfirmSessionRecovery { .. }
    )
}

/// Handles all blocking dialogs whose renderer uses the shared Cancel/primary
/// action row. Button clicks are translated into their existing keyboard
/// contract, keeping validation and nested-modal restoration in one place.
fn handle_shared_action_dialog_mouse(app: &mut App, mouse: MouseEvent) {
    if !matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
        return;
    }
    let position = Position::new(mouse.column, mouse.row);

    if matches!(
        app.mode,
        Mode::ConfirmClose(_)
            | Mode::ConfirmWorkspaceCloseOffer(_)
            | Mode::ConfirmRemoveWorkspace(_)
            | Mode::BoardDeleteConfirm(_, _)
            | Mode::ConfirmSessionRecovery { .. }
    ) {
        let layout = crate::modal::confirm_dialog_layout(app.layout.screen_area);
        if let Some(action) = layout.actions.action_at(position) {
            dispatch_confirmation_action(app, action);
        }
        return;
    }

    if matches!(app.mode, Mode::VoicePromptEditor(_)) {
        let layout = crate::modal::multiline_prompt_dialog_layout(app.layout.screen_area);
        if let Some(action) = layout.actions.action_at(position) {
            dispatch_multiline_prompt_action(app, action);
            return;
        }
        let editor_area = crate::instruction_settings::editor_area(app.layout.screen_area);
        if editor_area.contains(position) {
            let Mode::VoicePromptEditor(state) = &mut app.mode else {
                unreachable!("the multiline dialog match above preserves its mode");
            };
            let row = position.y.saturating_sub(editor_area.y);
            let column = position.x.saturating_sub(editor_area.x);
            state
                .textarea
                .move_cursor(ratatui_textarea::CursorMove::Jump(row, column));
        }
        return;
    }

    let layout = crate::modal::text_prompt_dialog_layout(app.layout.screen_area);
    if let Some(action) = layout.actions.action_at(position) {
        dispatch_form_action(app, action);
        return;
    }
    if !layout.input_box.contains(position) {
        return;
    }
    let state = match &mut app.mode {
        Mode::Rename(state)
        | Mode::CommandPrompt(state)
        | Mode::InferenceSettingPrompt(_, state)
        | Mode::VoiceSettingPrompt(_, state)
        | Mode::ApiSettingPrompt(state)
        | Mode::GitSettingPrompt(_, state)
        | Mode::AnimationTextPrompt(_, state)
        | Mode::AgentSetupPathPrompt(_, state)
        | Mode::SaveAs(_, state)
        | Mode::AgentDebugSavePath(_, state)
        | Mode::BoardCardPrompt(_, state)
        | Mode::BoardColumnPrompt(_, state)
        | Mode::BoardRenamePrompt(_, _, state) => state,
        _ => return,
    };
    place_prompt_cursor(state, layout.input_box, position);
}

fn handle_agent_setup_prompt_mouse(app: &mut App, mouse: MouseEvent) {
    if !matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
        return;
    }
    let position = Position::new(mouse.column, mouse.row);
    let Some(focus) = crate::setup_prompt::hit_test(app.layout.screen_area, position) else {
        return;
    };
    let Mode::AgentSetupPrompt(mut state) = std::mem::replace(&mut app.mode, Mode::Normal) else {
        unreachable!("setup prompt match must preserve its mode");
    };
    match state.activate(focus) {
        crate::setup_prompt::SetupPromptOutcome::Continue => {
            app.mode = Mode::AgentSetupPrompt(state)
        }
        crate::setup_prompt::SetupPromptOutcome::Apply { chatroom, progress } => {
            app.mode = Mode::AgentSetupPrompt(state.clone());
            if app.apply_agent_setup_prompt(&state.scope, chatroom, progress) {
                app.mode = Mode::Normal;
                app.maybe_show_agent_setup_prompt();
            } else {
                app.mode = Mode::AgentSetupPrompt(state);
            }
        }
        crate::setup_prompt::SetupPromptOutcome::NotNow => {
            app.maybe_show_agent_setup_prompt();
        }
        crate::setup_prompt::SetupPromptOutcome::NeverAsk => {
            if app.suppress_agent_setup_prompt(&state.scope) {
                app.maybe_show_agent_setup_prompt();
            } else {
                app.mode = Mode::AgentSetupPrompt(state);
            }
        }
    }
}

/// Reuses the Y/N keyboard path so button clicks cannot drift from key
/// behavior as confirmation modes evolve.
fn dispatch_confirmation_action(app: &mut App, action: crate::modal::DialogAction) {
    let code = match action {
        crate::modal::DialogAction::Cancel => KeyCode::Char('n'),
        crate::modal::DialogAction::Confirm => KeyCode::Char('y'),
    };
    crate::keys::handle_event(app, Event::Key(KeyEvent::new(code, KeyModifiers::NONE)));
}

/// Reuses the Enter/Esc form contract, including validation and any parent
/// modal that must be restored after the child closes.
fn dispatch_form_action(app: &mut App, action: crate::modal::DialogAction) {
    let code = match action {
        crate::modal::DialogAction::Cancel => KeyCode::Esc,
        crate::modal::DialogAction::Confirm => KeyCode::Enter,
    };
    crate::keys::handle_event(app, Event::Key(KeyEvent::new(code, KeyModifiers::NONE)));
}

/// The multiline prompt deliberately applies with Ctrl+S because Enter is
/// meaningful text input inside its editor.
fn dispatch_multiline_prompt_action(app: &mut App, action: crate::modal::DialogAction) {
    let (code, modifiers) = match action {
        crate::modal::DialogAction::Cancel => (KeyCode::Esc, KeyModifiers::NONE),
        crate::modal::DialogAction::Confirm => (KeyCode::Char('s'), KeyModifiers::CONTROL),
    };
    crate::keys::handle_event(app, Event::Key(KeyEvent::new(code, modifiers)));
}

fn handle_terminal_pane_context_menu_mouse(
    app: &mut App,
    mut menu: crate::terminal_context_menu::TerminalPaneContextMenu,
    mouse: MouseEvent,
) {
    if !matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
        app.mode = Mode::TerminalPaneContextMenu(menu);
        return;
    }
    let position = Position::new(mouse.column, mouse.row);
    if !menu.area.contains(position) || position.y < menu.area.y.saturating_add(1) {
        app.mode = Mode::Normal;
        return;
    }
    let action_index = usize::from(position.y - menu.area.y.saturating_add(1));
    let Some(action) = menu.actions.get(action_index).cloned() else {
        app.mode = Mode::TerminalPaneContextMenu(menu);
        return;
    };
    menu.selected_index = action_index;
    app.execute_terminal_context_action(action, menu);
}

/// Left clicks inside the location picker: fields, results, map and buttons
/// are resolved through the same `location_picker::layout` the renderer uses.
fn handle_location_picker_mouse(app: &mut App, mouse: MouseEvent) {
    use crate::location_picker::PickerOutcome;
    if !matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
        return;
    }
    let position = Position::new(mouse.column, mouse.row);
    let screen = app.layout.screen_area;
    let mut picker = match std::mem::replace(&mut app.mode, Mode::Normal) {
        Mode::LocationPicker(picker) => picker,
        other => {
            app.mode = other;
            return;
        }
    };
    match picker.click(position, screen) {
        PickerOutcome::Continue => app.mode = Mode::LocationPicker(picker),
        PickerOutcome::Cancel => app.pop_modal(),
        PickerOutcome::Confirm => match app.confirm_location_picker(&mut picker) {
            Ok(()) if picker.is_saving() => app.mode = Mode::LocationPicker(picker),
            Ok(()) => app.pop_modal(),
            Err(error) => {
                picker.status = Some(error.clone());
                app.status_message = Some(error);
                app.mode = Mode::LocationPicker(picker);
            }
        },
    }
}

fn handle_agent_debug_log_mouse(app: &mut App, mouse: MouseEvent, position: Position) {
    if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left))
        && crate::agent_debug_ui::resize_filter_button_area(app.layout.pane_area).contains(position)
    {
        app.toggle_agent_debug_tree_resize_filter();
        return;
    }
    if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left))
        && crate::agent_debug_ui::save_button_area(app.layout.pane_area).contains(position)
    {
        let Mode::AgentDebugLog(state) = std::mem::replace(&mut app.mode, Mode::Normal) else {
            return;
        };
        app.open_agent_debug_save_path(state);
        return;
    }
    let Mode::AgentDebugLog(state) = &mut app.mode else {
        return;
    };
    match mouse.kind {
        MouseEventKind::ScrollUp => state.scroll_older(3),
        MouseEventKind::ScrollDown => state.scroll_newer(3),
        MouseEventKind::Down(MouseButton::Left)
            if crate::agent_debug_ui::back_button_area(app.layout.pane_area).contains(position) =>
        {
            app.mode = Mode::Normal;
        }
        _ => {}
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
    // A click outside the popup (e.g. over the tree/pane area, which is
    // still live underneath this modal) must cancel rather than fall
    // through to the row-only checks below, which otherwise match on
    // `mouse.row` alone and would misfire for any column on that terminal
    // row -- mirroring every sibling dialog handler's outside-click cancel.
    if !popup.contains(position) {
        app.mode = Mode::Normal;
        return;
    }
    let orientation = match mouse.row {
        row if row == popup.y.saturating_add(3) => Some(ilium_core::SplitOrientation::Vertical),
        row if row == popup.y.saturating_add(4) => Some(ilium_core::SplitOrientation::Horizontal),
        _ => None,
    };
    if let Some(orientation) = orientation {
        app.continue_create_split(orientation);
    } else if mouse.row == popup.y.saturating_add(6) {
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
    // A click outside the popup cancels, matching every other creation
    // dialog's mouse handler in this file (create-group, create-board,
    // create-agent-from-line, scheduled-input, prompt-queue, ...).
    if !popup.contains(position) {
        app.mode = Mode::Normal;
        return;
    }
    if mouse.row == popup.bottom().saturating_sub(2) {
        app.commit_create_split(state);
    } else {
        app.mode = Mode::CreateSplitMembers(state);
    }
}

/// Selects tree rows, opens the right-click menu, scrolls, and handles the
/// toolbar/row hover controls.
fn handle_tree_mouse(app: &mut App, mouse: MouseEvent, position: Position) {
    // The press may focus a terminal leaf. Its release must finish the gesture
    // without reversing that focus and routing recalled-prompt keys to the tree.
    if !matches!(mouse.kind, MouseEventKind::Up(_)) {
        app.leave_pane_focus();
    }
    update_tree_hover(app, position);

    if let Some(popover) = &app.agent_popover {
        if popover.is_visible(Instant::now()) {
            if let Some(geometry) = crate::popover::layout(app.layout.tree_area, popover) {
                if let Some(crate::popover::PopoverHit::Choice(choice)) = geometry.hit(position) {
                    if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
                        if let Some(reason) = popover.unavailable_reason(choice) {
                            app.status_message = Some(reason.to_string());
                        } else {
                            let provider = popover.provider;
                            let target = app.selected_node_id().unwrap_or(ROOT_ID);
                            app.open_create_agent_workspace_dialog(
                                provider,
                                target,
                                choice == crate::popover::PopoverChoice::ExistingWorktree,
                            );
                        }
                        return;
                    }
                } else if popover.pinned
                    && matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left))
                    && !geometry.area.contains(position)
                    && !geometry.anchor.contains(position)
                {
                    app.agent_popover = None;
                }
            }
        }
    }

    if matches!(mouse.kind, MouseEventKind::Moved) {
        return;
    }

    if let Some(action) = app.emitted_tree_toolbar_at(position) {
        if let TreeToolbarAction::Agent(provider) = action {
            if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Right)) {
                if let Some((_, anchor)) = tree_ui::toolbar_button_rects(app.layout.tree_area)
                    .into_iter()
                    .find(|(button, _)| *button == action)
                {
                    app.hover_agent_popover(provider, anchor, Instant::now(), true);
                }
                return;
            }
        }
        if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
            app.last_tree_click = None;
            execute_tree_toolbar_action(app, action);
        }
        return;
    }

    if let Some((hit, action)) = app.emitted_tree_action_at(position) {
        if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
            app.last_tree_click = None;
            handle_tree_row_action(app, hit.id, action);
        }
        return;
    }

    match mouse.kind {
        MouseEventKind::ScrollUp => {
            app.tree_state.scroll_up(3);
        }
        MouseEventKind::ScrollDown => {
            app.tree_state.scroll_down(3);
        }
        MouseEventKind::Down(MouseButton::Right) => {
            app.last_tree_click = None;
            // No node under the click means empty space below the last
            // entry -- fall back to ROOT_ID so "New group" lands at the
            // top level instead of doing nothing.
            let selected_target = app
                .tree_node_at(position)
                .map(|hit| hit.id)
                .unwrap_or(ROOT_ID);
            let target =
                tree_ui::chatroom_project(&app.tree, selected_target, &app.chatroom_projects)
                    .unwrap_or(selected_target);
            app.select_node(target);
            app.open_context_menu(target, mouse.column, mouse.row);
        }
        MouseEventKind::Down(MouseButton::Left) => {
            if let Some(hit) = app.tree_node_at(position) {
                if let Some(project_id) =
                    tree_ui::chatroom_project(&app.tree, hit.id, &app.chatroom_projects)
                {
                    app.last_tree_click = None;
                    app.show_chatroom(project_id);
                    return;
                }
                if let Some(entry) = tree_ui::folder_entry(&app.tree, hit.id, &app.sidebar_snapshot)
                {
                    app.last_tree_click = None;
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
                if is_tree_rename_double_click(app, hit.id) {
                    app.action_start_rename();
                    return;
                }
                app.begin_tree_drag(hit.id);
                if app
                    .tree
                    .get(hit.id)
                    .is_some_and(ilium_core::Node::is_split_view)
                {
                    app.toggle_selected_tree_node();
                    app.show_split_view(hit.id);
                } else if app.tree.get(hit.id).is_some_and(is_lockable) {
                    handle_lockable_left_click(app, hit.id);
                } else {
                    app.focus_pane(hit.id);
                }
            }
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            app.last_tree_click = None;
            app.mark_tree_drag_in_progress();
        }
        MouseEventKind::Up(MouseButton::Left) => {
            // Arbitrary drag-and-drop: drop onto another tree row (its
            // `NodeId`) or `None` for the empty space below the last row
            // (meaning "append at the top level") -- see
            // `compute_drop_target`'s doc comment for the exact target
            // rules and the cases that are rejected client-side rather
            // than round-tripped to the server.
            if app.is_tree_drag_in_progress() {
                let Some(dragged_id) = app.drag_source() else {
                    app.clear_tree_drag();
                    return;
                };
                let drop_target = app.tree_node_at(position).map(|hit| hit.id);
                if let Some((new_parent, index)) =
                    compute_drop_target(&app.tree, dragged_id, drop_target)
                {
                    app.request_reparent(dragged_id, new_parent, index);
                }
            }
            app.clear_tree_drag();
        }
        _ => {}
    }
}

/// Whether this event ends a pending tree rename double-click pair without being
/// its second half. Only two consecutive clicks on the same tree row are a
/// double-click; any other left press in between -- a modal's button, a
/// pane, the agent toolbar -- is a separate interaction, and the pair must
/// not survive it. Without this, a click on a row, an intervening dialog
/// dismissed by mouse, and a click back on the same row inside
/// `TREE_DOUBLE_CLICK_WINDOW` would read as one deliberate double-click and
/// rename the row without the user asking to rename it.
fn ends_tree_double_click_pair(app: &App, mouse: &MouseEvent, position: Position) -> bool {
    matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left))
        && !(matches!(app.mode, Mode::Normal) && app.layout.tree_area.contains(position))
}

/// Whether a left click on this node should be routed to
/// `handle_lockable_left_click` rather than a plain focus/toggle. Project
/// and normal group containers plus folders all support the lock-closed
/// feature; a split view is a container too but keeps its own dedicated
/// single-click behavior (see the split-view branch this is checked after),
/// so it is deliberately excluded here.
fn is_lockable(node: &ilium_core::Node) -> bool {
    node.is_folder() || node.is_group() || node.is_project()
}

/// A second left click on the same real tree node within the gesture window
/// opens the existing Rename prompt. The first click keeps the normal tree
/// behavior; this state is deliberately client-local because the terminal
/// only reports individual mouse presses.
fn is_tree_rename_double_click(app: &mut App, id: NodeId) -> bool {
    let now = Instant::now();
    let is_double_click = app.last_tree_click.is_some_and(|(last_id, last_at)| {
        last_id == id && now.duration_since(last_at) <= TREE_DOUBLE_CLICK_WINDOW
    });
    app.last_tree_click = if is_double_click {
        None
    } else {
        Some((id, now))
    };
    is_double_click
}

/// Routes a single click on a project/group/folder row to a plain
/// expand/collapse toggle. A locked row remains inert until the explicit
/// unlock action in its context menu is used.
fn handle_lockable_left_click(app: &mut App, id: NodeId) {
    let lock_feature_enabled = app.ui_settings.lock_closed_enabled;
    let is_locked_closed = lock_feature_enabled
        && app
            .tree
            .get(id)
            .and_then(ilium_core::Node::is_locked_closed)
            .unwrap_or(false);
    if is_locked_closed {
        return;
    }
    app.toggle_selected_tree_node();
}

/// A second left click on the same row within this window counts as a
/// double-click. Long enough for a deliberate double-click, short enough
/// that two unrelated single clicks on the same row are not misread as one.
const TREE_DOUBLE_CLICK_WINDOW: Duration = Duration::from_millis(400);

/// Updates the two independent hover affordances (row hit + toolbar) from
/// the pointer's current tree-panel-relative position.
fn update_tree_hover(app: &mut App, position: Position) {
    let previous_branch = app
        .hovered_tree_node
        .filter(|hit| hit.line == 1)
        .map(|hit| hit.id);
    let hit = app.tree_node_at(position);
    if let Some(branch_hit) = hit.filter(|hit| hit.line == 1) {
        if previous_branch != Some(branch_hit.id)
            && app.tree.pane_workspace(branch_hit.id).is_some()
        {
            app.queue_request(ilium_ipc::ClientRequest::RefreshPaneGitStatus {
                pane_id: branch_hit.id,
            });
        }
    }
    app.set_hovered_tree_node(hit);
    app.hovered_status_slot = app.tree_status_slot_at(position);
    let toolbar_action = tree_ui::toolbar_action_at(app.layout.tree_area, position);
    let toolbar_hovered = tree_ui::toolbar_area(app.layout.tree_area).contains(position);
    app.set_tree_toolbar_hover(toolbar_hovered, toolbar_action);
    let now = Instant::now();
    if let Some(TreeToolbarAction::Agent(provider)) = toolbar_action {
        if let Some((_, anchor)) = tree_ui::toolbar_button_rects(app.layout.tree_area)
            .into_iter()
            .find(|(button, _)| *button == TreeToolbarAction::Agent(provider))
        {
            app.hover_agent_popover(provider, anchor, now, false);
        }
    }
    app.update_agent_popover_pointer(position, now);
}

fn handle_tree_row_action(app: &mut App, id: ilium_core::NodeId, action: TreeRowAction) {
    app.select_node(id);
    match action {
        TreeRowAction::Rename => app.action_start_rename(),
        TreeRowAction::MoveUp => {
            app.request_move(id, ilium_core::TreeMoveDirection::Up);
        }
        TreeRowAction::MoveDown => {
            app.request_move(id, ilium_core::TreeMoveDirection::Down);
        }
        TreeRowAction::Close => app.action_close_selected(),
        TreeRowAction::Retitle => app.action_request_retitle(id),
        TreeRowAction::ProjectRestructure => app.action_request_project_restructure(id),
        TreeRowAction::AskForUpdate => app.action_ask_for_update(id),
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
        TreeToolbarAction::Agent(provider) => {
            app.agent_popover = None;
            app.action_new_command_pane(provider.command_line());
        }
    }
    app.status_message = Some(format!("Created {}", action.description()));
}

/// Handles an activated context-menu entry or dismisses a click outside.
fn handle_context_menu_mouse(app: &mut App, mut menu: ContextMenu, mouse: MouseEvent) {
    let position = Position::new(mouse.column, mouse.row);
    if matches!(mouse.kind, MouseEventKind::Moved) {
        if let Some(submenu) = menu.submenu.as_mut() {
            if submenu.area.contains(position) {
                let row = usize::from(position.y.saturating_sub(submenu.area.y.saturating_add(1)));
                if position.y > submenu.area.y && row < submenu.items.len() {
                    submenu.selected_index = row;
                    if let Some(reason) = &submenu.items[row].disabled_reason {
                        app.status_message = Some(reason.clone());
                    }
                }
                menu.hover_candidate = None;
                app.mode = Mode::ContextMenu(menu);
                return;
            }
        }
        if menu.area.contains(position) {
            let row = usize::from(position.y.saturating_sub(menu.area.y.saturating_add(1)));
            if position.y > menu.area.y && row < menu.actions.len() {
                menu.selected_index = row;
                let action = menu.actions[row];
                if action.has_submenu() {
                    if menu
                        .submenu
                        .as_ref()
                        .is_some_and(|submenu| submenu.parent == action)
                    {
                        menu.hover_candidate = None;
                    } else if menu.hover_candidate.map(|candidate| candidate.0) != Some(action) {
                        menu.hover_candidate = Some((action, Instant::now()));
                    }
                } else {
                    menu.submenu = None;
                    menu.hover_candidate = None;
                }
            }
        } else {
            menu.hover_candidate = None;
        }
        app.mode = Mode::ContextMenu(menu);
        return;
    }
    if !matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
        app.mode = Mode::ContextMenu(menu);
        return;
    }

    if let Some(submenu) = &menu.submenu {
        if submenu.area.contains(position) {
            let content_top = submenu.area.y.saturating_add(1);
            if position.y < content_top {
                app.mode = Mode::ContextMenu(menu);
                return;
            }
            let item_row = usize::from(position.y - content_top);
            let Some(item) = submenu.items.get(item_row) else {
                app.mode = Mode::ContextMenu(menu);
                return;
            };
            if let Some(reason) = &item.disabled_reason {
                app.status_message = Some(reason.clone());
                app.mode = Mode::ContextMenu(menu);
                return;
            }
            app.execute_context_submenu_item(item, menu.target);
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
    if action.has_submenu() {
        app.open_context_submenu(&mut menu, action);
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
    let action = menu.actions[item_row].clone();
    app.execute_editor_line_context_action(action, menu.source);
}

/// Handles a click inside (or outside) the Codex Sol/Astra/Luna
/// reasoning-strength submenu, mirroring
/// `handle_editor_line_context_menu_mouse`'s outside-click/border/row
/// structure exactly.
fn handle_agent_toolbar_model_submenu_mouse(
    app: &mut App,
    mut state: AgentToolbarModelSubmenuState,
    mouse: MouseEvent,
) {
    if !matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
        app.mode = Mode::AgentToolbarModelSubmenu(state);
        return;
    }
    let position = Position::new(mouse.column, mouse.row);
    if !state.area.contains(position) {
        app.mode = Mode::Normal;
        return;
    }
    let content_top = state.area.y.saturating_add(1);
    if position.y < content_top {
        app.mode = Mode::AgentToolbarModelSubmenu(state);
        return;
    }
    let item_row = usize::from(position.y - content_top);
    let level_count =
        crate::agent_toolbar::codex_reasoning_levels(usize::from(state.tier_index)).len();
    if item_row >= level_count {
        app.mode = Mode::AgentToolbarModelSubmenu(state);
        return;
    }
    state.selected_index = item_row;
    let pane_id = state.pane_id;
    let action = AgentToolbarAction::CodexReasoningLevel(state.tier_index, item_row as u8);
    app.execute_agent_toolbar_action(pane_id, action);
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
    let control = crate::agent_from_line::provider_control(app.layout.screen_area, &state);
    if control.geometry().row.contains(position)
        && matches!(
            mouse.kind,
            MouseEventKind::Down(MouseButton::Left | MouseButton::Right)
        )
    {
        state.focus = CreateAgentFocus::AgentType;
        let button = if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Right)) {
            crate::value_control::PointerButton::Right
        } else {
            crate::value_control::PointerButton::Left
        };
        match control.hit(position, button) {
            Some(crate::value_control::ControlAction::OpenChoices) => {
                app.begin_agent_from_line_provider_dialog(state)
            }
            Some(crate::value_control::ControlAction::PreviousChoice) => {
                state.agent_type = state.agent_type.stepped(-1);
                app.mode = Mode::CreateAgentFromLine(state);
            }
            Some(crate::value_control::ControlAction::NextChoice) => {
                state.agent_type = state.agent_type.stepped(1);
                app.mode = Mode::CreateAgentFromLine(state);
            }
            _ => app.mode = Mode::CreateAgentFromLine(state),
        }
        return;
    }
    if !matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
        app.mode = Mode::CreateAgentFromLine(state);
        return;
    }
    if !layout.popup.contains(position) {
        app.mode = Mode::Normal;
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

fn handle_worktree_manager_mouse(
    app: &mut App,
    mut state: Box<crate::worktree_manager::WorktreeManagerState>,
    mouse: MouseEvent,
) {
    use crate::worktree_manager::{ManagerHit, ManagerView};
    if matches!(
        mouse.kind,
        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
    ) {
        if matches!(state.view, ManagerView::Browsing) {
            state.select_delta(if matches!(mouse.kind, MouseEventKind::ScrollUp) {
                -1
            } else {
                1
            });
        }
        app.mode = Mode::WorktreeManager(state);
        return;
    }
    if let MouseEventKind::Down(button @ (MouseButton::Left | MouseButton::Right)) = mouse.kind {
        if let Some(control) =
            crate::worktree_manager::branch_control(app.layout.screen_area, &state)
        {
            let position = Position::new(mouse.column, mouse.row);
            if control.geometry().row.contains(position) {
                use crate::value_control::{ControlAction, PointerButton};
                let pointer = if button == MouseButton::Left {
                    PointerButton::Left
                } else {
                    PointerButton::Right
                };
                match control.hit(position, pointer) {
                    Some(ControlAction::OpenChoices) => {
                        app.mode = Mode::WorktreeManager(state);
                        app.begin_prune_branch_dialog();
                        return;
                    }
                    Some(ControlAction::PreviousChoice | ControlAction::NextChoice) => {
                        state.toggle_branch_policy()
                    }
                    _ => {}
                }
                app.mode = Mode::WorktreeManager(state);
                return;
            }
        }
    }
    if !matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
        app.mode = Mode::WorktreeManager(state);
        return;
    }
    let position = Position::new(mouse.column, mouse.row);
    match state.hit_test(app.layout.screen_area, position) {
        Some(ManagerHit::Row(index)) => state.select(index),
        Some(ManagerHit::Safe) => {
            if let Err(error) = state.begin_safe_confirmation() {
                app.status_message = Some(error);
            }
        }
        Some(ManagerHit::Discard) => {
            if let Err(error) = state.begin_discard_confirmation() {
                app.status_message = Some(error);
            }
        }
        Some(ManagerHit::ToggleBranch) => state.toggle_branch_policy(),
        Some(ManagerHit::Refresh) => {
            app.refresh_worktree_manager(state);
            return;
        }
        Some(ManagerHit::Close) if !matches!(state.view, ManagerView::Pruning) => return,
        Some(ManagerHit::Confirm) => {
            app.submit_worktree_manager_prune(state);
            return;
        }
        Some(ManagerHit::Cancel) => state.cancel_confirmation(),
        _ => {}
    }
    app.mode = Mode::WorktreeManager(state);
}

fn handle_create_agent_workspace_mouse(
    app: &mut App,
    mut state: Box<crate::worktree_dialog::WorktreeDialogState>,
    mouse: MouseEvent,
) {
    use crate::worktree_dialog::{self, WorktreeDialogFocus, WorktreeDialogStatus};
    use ratatui_textarea::CursorMove;

    let position = Position::new(mouse.column, mouse.row);
    let layout = worktree_dialog::dialog_layout(app.layout.screen_area, &state);
    if matches!(state.status, WorktreeDialogStatus::Creating(_)) {
        app.mode = Mode::CreateAgentWorkspace(state);
        return;
    }
    if let MouseEventKind::Down(button @ (MouseButton::Left | MouseButton::Right)) = mouse.kind {
        use crate::value_control::{ControlAction, PointerButton};
        for field in crate::value_worktree::WorkspaceChoice::ALL {
            if field == crate::value_worktree::WorkspaceChoice::ClosePolicy && !state.advanced {
                continue;
            }
            let control = field.control(app.layout.screen_area, &state);
            if !control.geometry().row.contains(position) {
                continue;
            }
            let pointer = if button == MouseButton::Left {
                PointerButton::Left
            } else {
                PointerButton::Right
            };
            state.focus = field.focus();
            match control.hit(position, pointer) {
                Some(ControlAction::OpenChoices) => app.begin_workspace_choice_dialog(state, field),
                Some(ControlAction::NextChoice | ControlAction::PreviousChoice) => {
                    let direction =
                        if control.hit(position, pointer) == Some(ControlAction::NextChoice) {
                            1
                        } else {
                            -1
                        };
                    if let Err(error) = field.step(&mut state, direction) {
                        app.status_message = Some(error);
                    }
                    app.mode = Mode::CreateAgentWorkspace(state);
                }
                _ => app.mode = Mode::CreateAgentWorkspace(state),
            }
            return;
        }
    }

    if matches!(
        mouse.kind,
        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
    ) && layout.existing_list.contains(position)
    {
        state.move_existing_selection(if matches!(mouse.kind, MouseEventKind::ScrollUp) {
            -1
        } else {
            1
        });
        app.mode = Mode::CreateAgentWorkspace(state);
        return;
    }
    if !matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
        app.mode = Mode::CreateAgentWorkspace(state);
        return;
    }
    if !layout.popup.contains(position) {
        app.mode = Mode::Normal;
        return;
    }
    let Some(focus) = worktree_dialog::hit_test(&state, &layout, position) else {
        app.mode = Mode::CreateAgentWorkspace(state);
        return;
    };
    state.focus = focus;
    match focus {
        WorktreeDialogFocus::Provider => {}
        WorktreeDialogFocus::Prompt => {
            let inner = Rect::new(
                layout.prompt_area.x.saturating_add(1),
                layout.prompt_area.y.saturating_add(1),
                layout.prompt_area.width.saturating_sub(2),
                layout.prompt_area.height.saturating_sub(2),
            );
            if inner.contains(position) {
                state.prompt.move_cursor(CursorMove::Jump(
                    position.y.saturating_sub(inner.y),
                    position.x.saturating_sub(inner.x),
                ));
            }
        }
        WorktreeDialogFocus::Where => {}
        WorktreeDialogFocus::ExistingWorktree => {
            if let Some(index) = worktree_dialog::existing_row_at(&state, &layout, position) {
                state.selected_existing = index;
            }
        }
        WorktreeDialogFocus::Advanced => state.advanced = !state.advanced,
        WorktreeDialogFocus::ClosePolicy => {}
        WorktreeDialogFocus::Create => {
            app.submit_create_agent_workspace(state);
            return;
        }
        WorktreeDialogFocus::Cancel => {
            app.mode = Mode::Normal;
            return;
        }
        WorktreeDialogFocus::Branch | WorktreeDialogFocus::Base | WorktreeDialogFocus::Path => {}
    }
    app.mode = Mode::CreateAgentWorkspace(state);
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
        let error = state
            .duration_control(ScheduledInputFocus::Hours, layout.hours)
            .hit(position, crate::value_control::PointerButton::Left)
            .and_then(|action| {
                state
                    .apply_duration_control(ScheduledInputFocus::Hours, action)
                    .err()
            });
        if let Some(message) = error {
            app.status_message = Some(message);
        }
    } else if layout.minutes.contains(position) {
        let error = state
            .duration_control(ScheduledInputFocus::Minutes, layout.minutes)
            .hit(position, crate::value_control::PointerButton::Left)
            .and_then(|action| {
                state
                    .apply_duration_control(ScheduledInputFocus::Minutes, action)
                    .err()
            });
        if let Some(message) = error {
            app.status_message = Some(message);
        }
    } else if layout.seconds.contains(position) {
        let error = state
            .duration_control(ScheduledInputFocus::Seconds, layout.seconds)
            .hit(position, crate::value_control::PointerButton::Left)
            .and_then(|action| {
                state
                    .apply_duration_control(ScheduledInputFocus::Seconds, action)
                    .err()
            });
        if let Some(message) = error {
            app.status_message = Some(message);
        }
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
    let position = Position::new(mouse.column, mouse.row);
    let layout = crate::prompt_queue::dialog_layout(app.layout.screen_area);
    if matches!(
        mouse.kind,
        MouseEventKind::Down(MouseButton::Left | MouseButton::Right)
    ) && layout.delivery.contains(position)
    {
        let button = if mouse.kind == MouseEventKind::Down(MouseButton::Right) {
            crate::value_control::PointerButton::Right
        } else {
            crate::value_control::PointerButton::Left
        };
        if let Some(action) = state
            .delivery_control(layout.delivery)
            .hit(position, button)
        {
            state.focus = PromptQueueFocus::Delivery;
            match action {
                crate::value_control::ControlAction::PreviousChoice => state.cycle_delivery(-1),
                crate::value_control::ControlAction::NextChoice => state.cycle_delivery(1),
                crate::value_control::ControlAction::OpenChoices => {
                    app.begin_queue_delivery_dialog(state);
                    return;
                }
                _ => {}
            }
        }
        app.mode = Mode::QueuePrompt(state);
        return;
    }
    if !matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
        app.mode = Mode::QueuePrompt(state);
        return;
    }
    if !layout.popup.contains(position) {
        app.mode = Mode::Normal;
        return;
    }
    if layout.text.contains(position) {
        state.focus = PromptQueueFocus::Text;
    } else if layout.times.contains(position) {
        let error = state
            .times_control(layout.times)
            .hit(position, crate::value_control::PointerButton::Left)
            .and_then(|action| state.apply_times_control(action).err());
        if let Some(message) = error {
            app.status_message = Some(message);
        }
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
            if !app.commit_create_group(&state) {
                app.mode = Mode::CreateGroup(state);
            }
        }
        None => app.mode = Mode::CreateGroup(state),
    }
}

/// Gives board creation the same direct field selection and explicit
/// Cancel/Create targets as the shared prompt dialogs.
fn handle_create_board_mouse(
    app: &mut App,
    mut state: crate::app::CreateBoardState,
    mouse: MouseEvent,
) {
    let position = Position::new(mouse.column, mouse.row);
    let control = crate::modal::create_board_storage_control(
        app.layout.screen_area,
        state.storage_kind.label(),
    );
    if control.geometry().row.contains(position)
        && matches!(
            mouse.kind,
            MouseEventKind::Down(MouseButton::Left | MouseButton::Right)
        )
    {
        let button = if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Right)) {
            crate::value_control::PointerButton::Right
        } else {
            crate::value_control::PointerButton::Left
        };
        match control.hit(position, button) {
            Some(crate::value_control::ControlAction::OpenChoices) => {
                app.begin_board_storage_dialog(state)
            }
            Some(
                crate::value_control::ControlAction::PreviousChoice
                | crate::value_control::ControlAction::NextChoice,
            ) => {
                state.storage_kind = match state.storage_kind {
                    crate::app::BoardStorageKind::Folder => {
                        crate::app::BoardStorageKind::MarkdownFile
                    }
                    crate::app::BoardStorageKind::MarkdownFile => {
                        crate::app::BoardStorageKind::Folder
                    }
                };
                app.mode = Mode::CreateBoard(state);
            }
            _ => app.mode = Mode::CreateBoard(state),
        }
        return;
    }
    if !matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
        app.mode = Mode::CreateBoard(state);
        return;
    }
    let position = Position::new(mouse.column, mouse.row);
    let layout = crate::modal::create_board_dialog_layout(app.layout.screen_area);
    if let Some(action) = layout.actions.action_at(position) {
        app.mode = Mode::CreateBoard(state);
        dispatch_form_action(app, action);
        return;
    }
    if layout.name_box.contains(position) {
        state.editing_path = false;
        place_prompt_cursor(&mut state.name, layout.name_box, position);
        app.mode = Mode::CreateBoard(state);
        return;
    }
    if layout.path_box.contains(position) {
        state.editing_path = true;
        place_prompt_cursor(&mut state.path, layout.path_box, position);
        app.mode = Mode::CreateBoard(state);
        return;
    }
    if layout.browse_button.contains(position) {
        app.open_board_path_picker(state);
        return;
    }
    if layout.popup.contains(position) {
        app.mode = Mode::CreateBoard(state);
    } else {
        app.mode = Mode::Normal;
    }
}

/// Rows scrolled per wheel notch over the settings screen's content panel --
/// matches `App`'s own `TERMINAL_WHEEL_SCROLL_LINES`/`tree_state.scroll_up(3)`
/// per-notch amount elsewhere in this crate.
const SETTINGS_WHEEL_SCROLL_LINES: u16 = 3;

/// Keeps the Animations disabled-option hover popover in step with the
/// pointer: a Moved event over a row that has disabled options (re)starts the
/// rest timer, anything else (other rows, clicks, wheel, other tabs)
/// dismisses it. The tick reveals it once the pointer has rested.
fn update_animation_hover(
    app: &mut App,
    state: &crate::app::SettingsState,
    layout: &crate::settings_ui::SettingsLayout,
    mouse: MouseEvent,
    position: Position,
) {
    let is_animations_controls = state.tab == crate::app::SettingsTab::Animations
        && !state.animation_fullscreen
        && state.animation_source_tab == crate::animation_plugins::AnimationSourceTab::Native;
    if !is_animations_controls || !matches!(mouse.kind, MouseEventKind::Moved) {
        app.clear_animation_hover();
        return;
    }
    let model = app.animation_row_model();
    let row = match crate::animation_settings_ui::hit(
        layout.content_area,
        &model,
        crate::animation_settings_ui::Scrolls::of(state),
        position,
    ) {
        Some(
            crate::animation_settings_ui::AnimationHit::Select(row)
            | crate::animation_settings_ui::AnimationHit::Activate(row)
            | crate::animation_settings_ui::AnimationHit::DisabledOption(row),
        ) => model
            .view(row)
            .filter(|view| !view.disabled_options.is_empty())
            .map(|_| row),
        _ => None,
    };
    app.set_animation_hover(row, Instant::now());
}

/// Mouse handling for the full-screen settings view (`Mode::Settings`):
/// clicking the header's close button closes the screen, clicking a tab
/// switches to it, clicking a row's `‹`/value control decrements/increments
/// it, and the wheel scrolls the content panel -- see `crate::settings_ui`'s
/// module doc comment for the shared layout/hit-test functions this
/// reproduces no arithmetic of its own from.
fn handle_settings_mouse(app: &mut App, mut state: crate::app::SettingsState, mouse: MouseEvent) {
    let selected_before = state.selected_row;
    let position = Position::new(mouse.column, mouse.row);
    let mut layout =
        crate::settings_ui::compute_layout_for_mode(app.layout.screen_area, app, &state);
    // An open Apply confirmation owns the pointer: everything under it is inert.
    if state.tab == crate::app::SettingsTab::Optimization
        && app.optimization.pending_apply.is_some()
    {
        app.optimization_modal_mouse(mouse, app.layout.screen_area);
        app.mode = Mode::Settings(state);
        return;
    }
    if mouse.kind == MouseEventKind::Down(MouseButton::Left)
        && crate::settings_ui::onboarding_button_area(layout.header_area).contains(position)
    {
        app.mode = Mode::Settings(state);
        crate::onboarding::integration::open(app, true);
        return;
    }
    let instruction_height =
        crate::instruction_settings::panel_height(state.tab, layout.content_area);
    if instruction_height > 0 {
        let panel = Rect {
            height: instruction_height,
            ..layout.content_area
        };
        if panel.contains(position)
            && matches!(
                mouse.kind,
                MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
            )
        {
            let fields = crate::instruction_settings::fields(state.tab);
            let index = state
                .selected_row
                .saturating_sub(crate::instruction_settings::SELECTION_BASE)
                .min(fields.len() - 1);
            let next = if mouse.kind == MouseEventKind::ScrollUp {
                index.saturating_sub(1)
            } else {
                (index + 1).min(fields.len() - 1)
            };
            state.selected_row = crate::instruction_settings::SELECTION_BASE + next;
            app.mode = Mode::Settings(state);
            return;
        }
        if panel.contains(position) && matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left))
        {
            let index = usize::from(position.y.saturating_sub(panel.y + 1)) / 3
                + crate::instruction_settings::first_visible(state.tab, panel, state.selected_row);
            if position.y > panel.y {
                if let Some(field) = crate::instruction_settings::fields(state.tab)
                    .get(index)
                    .copied()
                {
                    state.selected_row = crate::instruction_settings::SELECTION_BASE + index;
                    app.mode = Mode::Settings(state);
                    app.settings_open_instruction(field);
                    return;
                }
            }
        }
        if state.tab != crate::app::SettingsTab::LlmInstructions {
            layout.content_area.y += instruction_height;
            layout.content_area.height = layout
                .content_area
                .height
                .saturating_sub(instruction_height);
        }
    }

    if state.tab == crate::app::SettingsTab::Icons && state.icon_picker.is_none() {
        if let MouseEventKind::Down(button @ (MouseButton::Left | MouseButton::Right)) = mouse.kind
        {
            use crate::value_control::{ControlAction, PointerButton};
            let pointer = if button == MouseButton::Left {
                PointerButton::Left
            } else {
                PointerButton::Right
            };
            for (index, target) in crate::agent_monitoring::general_icon_targets()
                .into_iter()
                .enumerate()
            {
                let Some(control) = crate::settings_ui::icon_assignment_control(
                    layout.content_area,
                    state.scroll,
                    index,
                    app.ui_settings.icons.glyph(target),
                ) else {
                    continue;
                };
                if !control.geometry().row.contains(position) {
                    continue;
                }
                match control.hit(position, pointer) {
                    Some(ControlAction::OpenChoices) => {
                        state.selected_row = index;
                        state.icon_picker = Some(crate::app::IconPickerState::new(target));
                    }
                    Some(ControlAction::PreviousChoice | ControlAction::NextChoice) => {
                        state.selected_row = index;
                        app.settings_cycle_icon(
                            target,
                            if control.hit(position, pointer) == Some(ControlAction::PreviousChoice)
                            {
                                -1
                            } else {
                                1
                            },
                        );
                    }
                    _ => {}
                }
                app.mode = Mode::Settings(state);
                return;
            }
            if button == MouseButton::Right {
                if let Some(hit) =
                    crate::settings_ui::icons_table_hit(layout.content_area, state.scroll, position)
                {
                    match hit.action {
                        crate::settings_ui::IconTableAction::OpenCatalogue => {
                            state.icon_picker = Some(crate::app::IconPickerState::new(hit.target))
                        }
                        crate::settings_ui::IconTableAction::CycleSuggestion => {
                            app.settings_cycle_icon(hit.target, -1)
                        }
                    }
                    app.mode = Mode::Settings(state);
                    return;
                }
            }
        }
    }

    // The full-screen animation preview hides every control: any click returns.
    if state.tab == crate::app::SettingsTab::Animations && state.animation_fullscreen {
        if matches!(mouse.kind, MouseEventKind::Down(_)) {
            state.animation_fullscreen = false;
        }
        app.mode = Mode::Settings(state);
        return;
    }

    if state.tab == crate::app::SettingsTab::Animations && layout.content_area.contains(position) {
        use crate::animation_plugins::{AnimationSourceTab, PluginEditorKind, PluginPanelRow};
        if let Some(mut editor) = state.plugin_editor.take() {
            let mut close = false;
            if mouse.kind == MouseEventKind::Down(MouseButton::Left) {
                match crate::animation_settings_ui::plugin_editor_action_at(
                    layout.content_area,
                    position,
                ) {
                    Some(crate::animation_settings_ui::PluginEditorAction::Apply) => {
                        match app.submit_plugin_editor(&editor) {
                            Ok(()) => close = true,
                            Err(error) => editor.error = Some(error),
                        }
                    }
                    Some(crate::animation_settings_ui::PluginEditorAction::Cancel) => close = true,
                    None => {}
                }
                if let Some(index) = crate::animation_settings_ui::plugin_editor_option_at(
                    layout.content_area,
                    &editor,
                    position,
                ) {
                    if let PluginEditorKind::Choice { cursor, .. } = &mut editor.kind {
                        *cursor = index;
                    }
                }
            }
            if !close {
                state.plugin_editor = Some(editor);
            }
            app.mode = Mode::Settings(state);
            return;
        }
        if mouse.kind == MouseEventKind::Down(MouseButton::Left) {
            if let Some(tab) = crate::animation_plugins::source_tabs(
                crate::animation_settings_ui::source_tab_area(layout.content_area),
            )
            .hit_test(position)
            {
                state.animation_source_tab = tab;
                state.animation_slider_drag = None;
                if tab == AnimationSourceTab::Plugin {
                    app.request_plugin_catalogue();
                }
                app.mode = Mode::Settings(state);
                return;
            }
        }
        if state.animation_source_tab == AnimationSourceTab::Plugin {
            let model = app.plugin_panel_model();
            let area = crate::animation_settings_ui::plugin_panel_area(layout.content_area);
            match mouse.kind {
                MouseEventKind::Down(MouseButton::Left | MouseButton::Right) => {
                    if let Some(row) = crate::animation_plugins::plugin_row_at(
                        area,
                        state.plugin_panel.scroll,
                        model.rows.len(),
                        position,
                    ) {
                        state.plugin_panel.cursor = row;
                        if let Some(control) = crate::value_plugin::panel_control(
                            app,
                            area,
                            &model,
                            &state.plugin_panel,
                            row,
                        ) {
                            use crate::value_control::{ControlAction, PointerButton};
                            let button = if mouse.kind == MouseEventKind::Down(MouseButton::Right) {
                                PointerButton::Right
                            } else {
                                PointerButton::Left
                            };
                            if let Some(action) = control.hit(position, button) {
                                match action {
                                    ControlAction::PreviousChoice | ControlAction::Decrement => {
                                        app.settings_adjust_plugin_row(row, -1)
                                    }
                                    ControlAction::NextChoice | ControlAction::Increment => {
                                        app.settings_adjust_plugin_row(row, 1)
                                    }
                                    ControlAction::OpenChoices | ControlAction::EditNumber => {
                                        if let Some(PluginPanelRow::Common(index)) =
                                            model.rows.get(row)
                                        {
                                            let index = *index;
                                            app.mode = Mode::Settings(state);
                                            app.begin_animation_value_dialog(index);
                                            return;
                                        }
                                        app.mode = Mode::Settings(state);
                                        app.begin_plugin_value_dialog(row);
                                        return;
                                    }
                                }
                            }
                        } else if mouse.kind == MouseEventKind::Down(MouseButton::Right) {
                            app.settings_adjust_plugin_row(row, -1);
                        } else {
                            if let Some(PluginPanelRow::Common(index)) = model.rows.get(row) {
                                if matches!(
                                    app.animation_row_model()
                                        .view(*index)
                                        .map(|view| &view.kind),
                                    Some(
                                        crate::animation_rows::RowKind::Choice
                                            | crate::animation_rows::RowKind::Slider(_)
                                    )
                                ) {
                                    let index = *index;
                                    app.mode = Mode::Settings(state);
                                    app.begin_animation_value_dialog(index);
                                    return;
                                }
                            }
                            if !app.begin_plugin_editor(&mut state, row) {
                                app.settings_adjust_plugin_row(row, 1);
                            }
                        }
                    }
                }
                MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                    let delta = usize::from(SETTINGS_WHEEL_SCROLL_LINES);
                    state.plugin_panel.scroll = if mouse.kind == MouseEventKind::ScrollUp {
                        state.plugin_panel.scroll.saturating_sub(delta)
                    } else {
                        state
                            .plugin_panel
                            .scroll
                            .saturating_add(delta)
                            .min(model.rows.len().saturating_sub(usize::from(area.height)))
                    };
                }
                _ => {}
            }
            app.clear_animation_hover();
            app.mode = Mode::Settings(state);
            return;
        }
    }

    update_animation_hover(app, &state, &layout, mouse, position);

    if let Some(action) = state.keyboard_picker.take() {
        if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
            let available = crate::keymap::available_keys(&app.keybindings);
            if let Some(key) = crate::settings_ui::keyboard_picker_key_at(
                app.layout.screen_area,
                position,
                &available,
            ) {
                app.settings_assign_key(action, crate::keymap::BindingKey::Character(key));
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

    if let Some(mut picker) = state.icon_picker.take() {
        if let MouseEventKind::Down(button @ (MouseButton::Left | MouseButton::Right)) = mouse.kind
        {
            let control = crate::value_icon::column_control(app.layout.screen_area, &picker);
            if control.geometry().row.contains(position) {
                use crate::value_control::{ControlAction, PointerButton};
                let pointer = if button == MouseButton::Left {
                    PointerButton::Left
                } else {
                    PointerButton::Right
                };
                match control.hit(position, pointer) {
                    Some(ControlAction::OpenChoices) => {
                        state.icon_picker = Some(picker);
                        app.mode = Mode::Settings(state);
                        app.begin_icon_column_dialog();
                        return;
                    }
                    Some(ControlAction::PreviousChoice | ControlAction::NextChoice) => {
                        picker.column_mode = picker.column_mode.toggle();
                        picker.scroll_row = crate::settings_ui::icon_picker_scroll_for_entry(
                            app.layout.screen_area,
                            &picker,
                        );
                    }
                    _ => {}
                }
                state.icon_picker = Some(picker);
                app.mode = Mode::Settings(state);
                return;
            }
        }
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
                Some(crate::settings_ui::IconPickerHit::ColumnMode(_)) => {
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

    if matches!(
        mouse.kind,
        MouseEventKind::Down(MouseButton::Left) | MouseEventKind::Down(MouseButton::Right)
    ) {
        use crate::value_control::{ControlAction, PointerButton};
        let button = if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
            PointerButton::Left
        } else {
            PointerButton::Right
        };
        if state.tab == crate::app::SettingsTab::Keyboard {
            for field in crate::value_keyboard::KeyboardPrefix::ALL {
                let Some(control) = field.settings_control(
                    layout.content_area,
                    state.scroll,
                    app.keyboard_settings,
                ) else {
                    continue;
                };
                if !control.geometry().row.contains(position) {
                    continue;
                }
                state.selected_row = field.row();
                match control.hit(position, button) {
                    Some(ControlAction::PreviousChoice) => app.step_keyboard_prefix(field, -1),
                    Some(ControlAction::NextChoice) => app.step_keyboard_prefix(field, 1),
                    Some(ControlAction::OpenChoices) => {
                        app.mode = Mode::Settings(state);
                        app.begin_keyboard_prefix_dialog(field);
                        return;
                    }
                    _ => {}
                }
                app.mode = Mode::Settings(state);
                return;
            }
        }
        // A bounded inventory: unrecognized rows retain their own handlers.
        let rows = crate::settings_ui::settings_number_row_count(app, state.tab);
        for row in 0..rows {
            if let Some((field, control)) =
                crate::settings_ui::settings_choice_control(layout.content_area, app, &state, row)
            {
                if control.geometry().row.contains(position) {
                    state.selected_row = row;
                    match control.hit(position, button) {
                        Some(ControlAction::PreviousChoice) => app.step_settings_choice(field, -1),
                        Some(ControlAction::NextChoice) => app.step_settings_choice(field, 1),
                        Some(ControlAction::OpenChoices) => {
                            app.mode = Mode::Settings(state);
                            app.begin_settings_choice_dialog(field);
                            return;
                        }
                        _ => {}
                    }
                    app.mode = Mode::Settings(state);
                    return;
                }
            }
            let Some((field, control)) =
                crate::settings_ui::settings_number_control(layout.content_area, app, &state, row)
            else {
                continue;
            };
            if !control.geometry().row.contains(position) {
                continue;
            }
            state.selected_row = row;
            match control.hit(position, button) {
                Some(ControlAction::Decrement) => app.step_settings_number(field, -1),
                Some(ControlAction::Increment) => app.step_settings_number(field, 1),
                Some(ControlAction::EditNumber) => {
                    app.mode = Mode::Settings(state);
                    app.begin_settings_number_dialog(field);
                    return;
                }
                _ => {}
            }
            app.mode = Mode::Settings(state);
            return;
        }
    }

    if state.tab == crate::app::SettingsTab::Animations {
        use crate::value_control::{ControlAction, PointerButton};
        let button = match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => Some(PointerButton::Left),
            MouseEventKind::Down(MouseButton::Right) => Some(PointerButton::Right),
            _ => None,
        };
        if let Some((row, action)) = button.and_then(|button| {
            crate::animation_settings_ui::value_hit(
                layout.content_area,
                &app.animation_row_model(),
                crate::animation_settings_ui::Scrolls::of(&state),
                position,
                button,
            )
        }) {
            state.selected_row = row;
            state.animation_slider_drag = None;
            match action {
                ControlAction::PreviousChoice | ControlAction::Decrement => {
                    app.settings_adjust_animation_row(row, -1)
                }
                ControlAction::NextChoice | ControlAction::Increment => {
                    app.settings_adjust_animation_row(row, 1)
                }
                ControlAction::OpenChoices | ControlAction::EditNumber => {
                    app.mode = Mode::Settings(state);
                    app.begin_animation_value_dialog(row);
                    return;
                }
            }
            app.mode = Mode::Settings(state);
            return;
        }
        if button.is_some() {
            let model = app.animation_row_model();
            if let Some(row) = (0..model.len()).find(|&row| {
                crate::animation_settings_ui::value_control(
                    layout.content_area,
                    &model,
                    row,
                    crate::animation_settings_ui::Scrolls::of(&state),
                )
                .is_some_and(|control| control.geometry().row.contains(position))
            }) {
                state.selected_row = row;
                state.animation_slider_drag = None;
                app.mode = Mode::Settings(state);
                return;
            }
        }
    }

    if state.tab == crate::app::SettingsTab::Cost {
        use crate::value_control::{ControlAction, PointerButton};
        let button = match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => Some(PointerButton::Left),
            MouseEventKind::Down(MouseButton::Right) => Some(PointerButton::Right),
            _ => None,
        };
        if let Some((index, row, action)) = button.and_then(|button| {
            crate::cost_settings_ui::value_hit(
                layout.content_area,
                state.scroll,
                position,
                button,
                app,
            )
        }) {
            state.selected_row = index;
            match action {
                ControlAction::PreviousChoice | ControlAction::Decrement => {
                    app.settings_adjust_cost_row(row, -1)
                }
                ControlAction::NextChoice | ControlAction::Increment => {
                    app.settings_adjust_cost_row(row, 1)
                }
                ControlAction::OpenChoices | ControlAction::EditNumber => {
                    app.mode = Mode::Settings(state);
                    app.begin_cost_value_dialog(row);
                    return;
                }
            }
            app.mode = Mode::Settings(state);
            return;
        }
        if button.is_some() {
            let rows = crate::cost_settings_ui::rows(app);
            let view =
                crate::cost_settings_ui::view(app, state.selected_row, layout.content_area.width);
            if let Some(span) = view.rows.iter().find(|span| {
                crate::cost_settings_ui::value_control(layout.content_area, state.scroll, span, app)
                    .is_some_and(|control| control.geometry().row.contains(position))
            }) {
                state.selected_row = rows
                    .iter()
                    .position(|row| *row == span.row)
                    .unwrap_or(state.selected_row);
                app.mode = Mode::Settings(state);
                return;
            }
        }
    }

    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            state.animation_slider_drag = None;
            if crate::settings_ui::close_button_hit(layout.header_area, position) {
                app.mode = Mode::Normal;
                return;
            }
            if let Some(topic_id) =
                crate::settings_ui::settings_help_at(&layout, app, &state, position)
            {
                if let Some(topic) = crate::settings_help::catalog::by_id(&topic_id) {
                    let help = crate::settings_help::dialog::SettingsHelpState::new(
                        topic_id,
                        topic.frames.len(),
                        app.ui_settings.motion_level,
                    );
                    app.push_modal_over(Mode::Settings(state), Mode::SettingsHelp(help));
                    return;
                }
            }
            if let Some(tab) =
                crate::settings_ui::tab_at_for_active(layout.tab_list_area, position, state.tab)
            {
                if tab != state.tab {
                    state.tab = tab;
                    state.selected_row = usize::from(
                        tab == crate::app::SettingsTab::Titles
                            && app.inference_settings.title_style
                                == ilium_inference::TitleStyle::Summarization,
                    );
                    if tab == crate::app::SettingsTab::Animations {
                        state.selected_row = crate::background_animation::AnimationKind::ALL
                            .iter()
                            .position(|kind| *kind == app.animation_settings.kind)
                            .unwrap_or(0);
                        state.scene_scroll = 0;
                        state.global_scroll = 0;
                        state.scroll = 0;
                        crate::animation_settings_ui::sync_scrolls(
                            layout.content_area,
                            &app.animation_row_model(),
                            &mut state,
                        );
                    } else {
                        state.scroll = 0;
                    }
                    state.trigger_action_cursor = 0;
                }
            } else if state.tab == crate::app::SettingsTab::Animations {
                use crate::animation_rows::AnimationRowOutcome;
                use crate::animation_settings_ui::AnimationHit;
                let model = app.animation_row_model();
                match crate::animation_settings_ui::hit(
                    layout.content_area,
                    &model,
                    crate::animation_settings_ui::Scrolls::of(&state),
                    position,
                ) {
                    Some(direction @ (AnimationHit::PreviousScene | AnimationHit::NextScene)) => {
                        let delta = if direction == AnimationHit::PreviousScene {
                            -1
                        } else {
                            1
                        };
                        let kind = crate::animation_settings_ui::adjacent_scene(
                            app.animation_settings.kind,
                            delta,
                        );
                        if let Some(index) = crate::background_animation::AnimationKind::ALL
                            .iter()
                            .position(|candidate| *candidate == kind)
                        {
                            state.selected_row = index;
                            app.settings_preview_select_animation_row(index);
                        }
                    }
                    Some(AnimationHit::Select(row)) => {
                        state.selected_row = row;
                        app.settings_preview_select_animation_row(row);
                    }
                    Some(AnimationHit::Activate(row)) => {
                        state.selected_row = row;
                        match app.settings_activate_animation_row(row) {
                            AnimationRowOutcome::Done => {}
                            AnimationRowOutcome::FullScreenPreview => {
                                state.animation_fullscreen = true;
                            }
                            AnimationRowOutcome::LocationPicker => {
                                app.mode = Mode::Settings(state);
                                app.open_location_picker();
                                return;
                            }
                            AnimationRowOutcome::TextPrompt {
                                control,
                                label,
                                hint,
                                current,
                            } => {
                                app.mode = Mode::Settings(state);
                                app.begin_animation_text_prompt(control, label, hint, current);
                                return;
                            }
                        }
                    }
                    Some(AnimationHit::Slider { row, value }) => {
                        state.selected_row = row;
                        state.animation_slider_drag = Some(row);
                        app.settings_set_animation_slider(row, value);
                    }
                    Some(AnimationHit::ScrollTo(region, offset)) => {
                        let mut scrolls = crate::animation_settings_ui::Scrolls::of(&state);
                        scrolls.set(region, offset);
                        scrolls.store(&mut state);
                    }
                    Some(AnimationHit::DisabledOption(row)) => {
                        state.selected_row = row;
                        if let Some(notice) =
                            model.view(row).and_then(|view| view.disabled_notice())
                        {
                            app.status_message = Some(notice);
                        }
                    }
                    None => {}
                }
            } else if state.tab == crate::app::SettingsTab::Setup {
                if let Some(index) = crate::settings_ui::setup_content_hit(
                    layout.content_area,
                    state.scroll,
                    position,
                    app,
                ) {
                    state.selected_row = index;
                    if let Some(row) = app.agent_setup_rows().get(index).cloned() {
                        match row {
                            crate::app::AgentSetupRow::GlobalFile { feature, .. } => {
                                app.mode = Mode::Settings(state);
                                app.settings_open_agent_setup_path(feature);
                                return;
                            }
                            _ => app.settings_toggle_agent_setup_row(&row),
                        }
                    }
                }
            } else if state.tab == crate::app::SettingsTab::Titles {
                if let Some(style) = crate::settings_ui::title_style_content_hit(
                    layout.content_area,
                    state.scroll,
                    position,
                ) {
                    state.selected_row =
                        usize::from(style == ilium_inference::TitleStyle::Summarization);
                    app.settings_select_title_style(style);
                }
            } else if state.tab == crate::app::SettingsTab::Inference {
                if let Some((row, direction)) = crate::settings_ui::inference_content_hit_with_test(
                    layout.content_area,
                    state.scroll,
                    position,
                    &app.inference_settings,
                    &app.inference_test_state,
                    &app.model_discovery,
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
                        crate::app::InferenceRow::RefreshModels => app.request_model_refresh(),
                        crate::app::InferenceRow::KiloGatewayModel => {
                            app.settings_adjust_kilo_gateway_model(direction)
                        }
                        crate::app::InferenceRow::Field(
                            crate::app::InferenceSettingField::OllamaModel,
                        ) => app.settings_adjust_ollama_model(direction),
                        crate::app::InferenceRow::Field(
                            crate::app::InferenceSettingField::OpenAiModel,
                        ) => app.settings_adjust_openai_model(direction),
                        crate::app::InferenceRow::Field(field) => {
                            app.mode = Mode::Settings(state);
                            app.settings_open_inference_field(field);
                            return;
                        }
                        crate::app::InferenceRow::Test => app.request_inference_test(),
                    }
                }
            } else if state.tab == crate::app::SettingsTab::TextTriggers {
                if let Some(index) = crate::settings_ui::text_trigger_content_hit(
                    layout.content_area,
                    state.scroll,
                    position,
                    app.text_trigger_settings.triggers.len(),
                ) {
                    state.selected_row = index;
                    let editing_index =
                        (index < app.text_trigger_settings.triggers.len()).then_some(index);
                    app.mode = Mode::Settings(state);
                    app.open_text_trigger_dialog(editing_index);
                    return;
                }
            } else if state.tab == crate::app::SettingsTab::Triggers {
                if let Some((event, action_hit)) = crate::trigger_settings_ui::hit_test(
                    app,
                    layout.content_area,
                    state.scroll,
                    position,
                    state.selected_row,
                    state.trigger_action_cursor,
                ) {
                    state.selected_row = crate::trigger_settings::TriggerEvent::ALL
                        .iter()
                        .position(|candidate| *candidate == event)
                        .unwrap_or(0);
                    if let Some(action) = action_hit {
                        state.trigger_action_cursor = action
                            .and_then(|action| {
                                event
                                    .available_actions()
                                    .iter()
                                    .position(|candidate| *candidate == action)
                                    .map(|index| index + 1)
                            })
                            .unwrap_or(0);
                        app.settings_toggle_trigger_action(event, action);
                    }
                }
            } else if state.tab == crate::app::SettingsTab::Icons {
                if let Some(real_tree) = crate::settings_ui::icons_preview_mode_hit(
                    layout.content_area,
                    position,
                    state.icons_preview_real,
                ) {
                    state.icons_preview_real = real_tree;
                } else if let Some(hit) =
                    crate::settings_ui::icons_table_hit(layout.content_area, state.scroll, position)
                {
                    state.selected_row = crate::agent_monitoring::general_icon_targets()
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
            } else if state.tab == crate::app::SettingsTab::AgentMonitoring {
                if let Some(hit) = crate::settings_ui::agent_monitoring_content_hit(
                    layout.content_area,
                    state.scroll,
                    position,
                    app,
                ) {
                    match hit {
                        crate::settings_ui::AgentMonitoringContentHit::Mode(mode) => {
                            state.selected_row = 0;
                            app.settings_set_agent_monitoring_mode(mode);
                        }
                        crate::settings_ui::AgentMonitoringContentHit::Row { row, direction } => {
                            let rows = crate::settings_ui::agent_monitoring_rows(app);
                            state.selected_row = rows
                                .iter()
                                .position(|candidate| *candidate == row)
                                .unwrap_or(state.selected_row);
                            if direction != 0 {
                                app.settings_adjust_agent_monitoring_row(row, direction);
                            } else {
                                match row {
                                    crate::app::AgentMonitoringRow::ProgressMonitor => {
                                        app.settings_toggle_progress_monitor();
                                    }
                                    crate::app::AgentMonitoringRow::ProgressMonitorMaxLines
                                    | crate::app::AgentMonitoringRow::ProgressFillStyle => {
                                        app.settings_adjust_agent_monitoring_row(row, 1);
                                    }
                                    crate::app::AgentMonitoringRow::AddCustomSignature => {
                                        app.settings_begin_custom_agent_signature();
                                    }
                                    crate::app::AgentMonitoringRow::StatusIcon(target) => {
                                        state.icon_picker =
                                            Some(crate::app::IconPickerState::new(target));
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                    app.mode = Mode::Settings(state);
                    return;
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
                        app.mode = Mode::Settings(state);
                        app.settings_adjust_voice_row(row, direction);
                        return;
                    }
                }
            } else if state.tab == crate::app::SettingsTab::Cost {
                if let Some(hit) =
                    crate::cost_settings_ui::hit(layout.content_area, state.scroll, position, app)
                {
                    state.selected_row = hit.index;
                    app.mode = Mode::Settings(state);
                    if app.cost_settings.number_spec(hit.row).is_none()
                        && !crate::value_cost::is_choice(hit.row)
                    {
                        app.settings_adjust_cost_row(hit.row, hit.direction);
                    }
                    return;
                }
            } else if state.tab == crate::app::SettingsTab::Optimization {
                app.optimization_click(&mut state, layout.content_area, position);
            } else if state.tab == crate::app::SettingsTab::RemoteCompaction {
                if let Some(hit) = crate::remote_compaction_settings_ui::hit(
                    layout.content_area,
                    state.scroll,
                    position,
                    app,
                ) {
                    state.selected_row = hit.index;
                    app.mode = Mode::Settings(state);
                    if let crate::remote_compaction_settings_ui::HitAction::Adjust(direction) =
                        hit.action
                    {
                        app.settings_adjust_remote_compaction_row(hit.row, direction);
                    }
                    return;
                }
            } else if state.tab == crate::app::SettingsTab::ResetPlanning {
                if let Some((index, _direction)) = crate::settings_ui::simple_content_hit(
                    layout.content_area,
                    state.scroll,
                    position,
                    3,
                ) {
                    state.selected_row = index;
                    app.mode = Mode::Settings(state);
                    app.settings_adjust_reset_planning_row(index);
                    return;
                }
            } else if state.tab == crate::app::SettingsTab::Debug {
                if let Some((index, _direction)) = crate::settings_ui::simple_content_hit(
                    layout.content_area,
                    state.scroll,
                    position,
                    crate::app::DebugRow::ALL.len(),
                ) {
                    state.selected_row = index;
                    if matches!(
                        crate::app::DebugRow::ALL.get(index),
                        Some(crate::app::DebugRow::FileLogging)
                    ) {
                        app.settings_toggle_file_logging();
                    }
                }
            } else if state.tab == crate::app::SettingsTab::Api {
                if let Some((index, _direction)) = crate::settings_ui::simple_content_hit(
                    layout.content_area,
                    state.scroll,
                    position,
                    crate::app::ApiRow::ALL.len(),
                ) {
                    state.selected_row = index;
                    app.mode = Mode::Settings(state);
                    app.settings_open_api_port();
                    return;
                }
            } else if state.tab == crate::app::SettingsTab::Appearance {
                if let Some(hit) = crate::settings_ui::appearance_content_hit(
                    layout.content_area,
                    state.scroll,
                    position,
                    &app.ui_settings,
                ) {
                    match hit {
                        crate::settings_ui::AppearanceContentHit::Mode(mode) => {
                            state.selected_row = 0;
                            app.settings_set_left_panel_sizing_mode(mode);
                        }
                        crate::settings_ui::AppearanceContentHit::Row { row, direction } => {
                            let rows = crate::app::AppearanceRow::visible(
                                app.ui_settings.left_panel_sizing.mode,
                            );
                            if let Some(index) = rows.iter().position(|candidate| *candidate == row)
                            {
                                state.selected_row = index;
                            }
                            app.settings_adjust_row(row, direction);
                        }
                    }
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
            } else if state.tab == crate::app::SettingsTab::Git {
                if let Some((index, direction)) = crate::settings_ui::simple_content_hit(
                    layout.content_area,
                    state.scroll,
                    position,
                    crate::app::GitRow::ALL.len(),
                ) {
                    state.selected_row = index;
                    if let Some(row) = crate::app::GitRow::ALL.get(index).copied() {
                        if matches!(
                            row,
                            crate::app::GitRow::BranchPrefix
                                | crate::app::GitRow::WorktreeLocationTemplate
                                | crate::app::GitRow::SetupCommand
                        ) {
                            app.mode = Mode::Settings(state);
                            app.settings_adjust_git_row(row, direction);
                            return;
                        }
                        app.settings_adjust_git_row(row, direction);
                    }
                }
            } else if state.tab == crate::app::SettingsTab::Keyboard {
                if let Some(preset) = crate::settings_ui::keyboard_keymap_preset_at(
                    layout.content_area,
                    state.scroll,
                    position,
                ) {
                    app.settings_apply_keymap_preset(preset);
                } else if let Some((row, direction)) = crate::settings_ui::keyboard_content_hit(
                    layout.content_area,
                    state.scroll,
                    position,
                ) {
                    state.selected_row = row;
                    if row == crate::settings_ui::KEYBOARD_NAVIGATION_BASE_ROW {
                        app.settings_adjust_navigation_shortcut_base(direction);
                    } else {
                        app.settings_adjust_shortcut_base(direction);
                    }
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
                                app.settings_assign_key(
                                    binding.action,
                                    crate::keymap::BindingKey::Character(key),
                                );
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
        MouseEventKind::Drag(MouseButton::Left) => {
            if state.tab == crate::app::SettingsTab::Animations {
                if let Some(row) = state.animation_slider_drag {
                    if let Some(value) = crate::animation_settings_ui::slider_value_at(
                        layout.content_area,
                        &app.animation_row_model(),
                        row,
                        crate::animation_settings_ui::Scrolls::of(&state),
                        mouse.column,
                    ) {
                        app.settings_set_animation_slider(row, value);
                    }
                }
            }
        }
        MouseEventKind::Up(MouseButton::Left) => state.animation_slider_drag = None,
        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
            state.animation_slider_drag = None;
            let delta = i32::from(SETTINGS_WHEEL_SCROLL_LINES);
            let delta = if mouse.kind == MouseEventKind::ScrollUp {
                -delta
            } else {
                delta
            };
            // On the Animations tab the wheel scrolls the region under the
            // pointer (scene list, global settings or the right column); the
            // scene list scrolls its window without selecting a scene.
            let mut scrolls = crate::animation_settings_ui::Scrolls::of(&state);
            let handled = state.tab == crate::app::SettingsTab::Animations
                && !state.animation_fullscreen
                && crate::animation_settings_ui::wheel_scroll(
                    layout.content_area,
                    &app.animation_row_model(),
                    &mut scrolls,
                    position,
                    delta,
                );
            if handled {
                scrolls.store(&mut state);
            } else {
                state.scroll = if delta < 0 {
                    state.scroll.saturating_sub(delta.unsigned_abs() as u16)
                } else {
                    state.scroll.saturating_add(delta as u16)
                };
            }
        }
        _ => {}
    }

    if state.tab == crate::app::SettingsTab::Animations {
        // A scene switch can change the row count under the selection.
        let model = app.animation_row_model();
        state.selected_row = state.selected_row.min(model.len().saturating_sub(1));
        crate::animation_settings_ui::Scrolls::of(&state)
            .clamped(layout.content_area, &model)
            .store(&mut state);
        // Keyboard-less selection changes (a click, Prev/Next) keep the
        // selected row visible in its own region.
        if state.selected_row != selected_before {
            crate::animation_settings_ui::sync_scrolls(layout.content_area, &model, &mut state);
        }
    }
    let max_scroll =
        crate::settings_ui::max_scroll(state.tab, app, state.selected_row, layout.content_area);
    state.scroll = state.scroll.min(max_scroll);
    app.optimization_sync_visibility(state.tab == crate::app::SettingsTab::Optimization);
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
        Ok(ExplorerOutcome::Picked(path)) => {
            if app.request_new_editor(target, path) {
                app.mode = Mode::Normal;
            } else {
                app.mode = Mode::Explorer(overlay, target);
            }
        }
        Ok(_) => app.mode = Mode::Explorer(overlay, target),
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
        app.close_modal_flow();
    } else {
        app.pop_modal();
    }
}

fn handle_folder_explorer_mouse(
    app: &mut App,
    mut overlay: Box<ExplorerOverlay>,
    target: ilium_core::NodeId,
    mouse: MouseEvent,
) {
    match overlay.handle(&Event::Mouse(mouse), app.layout.screen_area) {
        Ok(ExplorerOutcome::Picked(path)) => {
            if app.request_new_folder(target, path) {
                app.mode = Mode::Normal;
            } else {
                app.mode = Mode::FolderExplorer(overlay, target);
            }
        }
        Ok(_) => app.mode = Mode::FolderExplorer(overlay, target),
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
        Ok(ExplorerOutcome::Picked(path)) => {
            let admitted = match selection {
                crate::app::ProjectFolderSelection::NewProject => app.request_new_project(path),
                crate::app::ProjectFolderSelection::ChangeProject(project_id) => {
                    app.request_change_project_folder(project_id, path)
                }
            };
            if admitted {
                app.mode = Mode::Normal;
            } else {
                app.mode = Mode::ProjectFolderExplorer(overlay, selection);
            }
        }
        Ok(_) => app.mode = Mode::ProjectFolderExplorer(overlay, selection),
        Err(err) => {
            app.status_message = Some(format!("Project picker error: {err}"));
            app.mode = Mode::ProjectFolderExplorer(overlay, selection);
        }
    }
}

/// A hit is valid only for the exact source instance/revision that reached the terminal.
pub(crate) fn emitted_editor_position(
    app: &App,
    pane_id: ilium_core::NodeId,
    position: ratatui::layout::Position,
) -> Option<(usize, usize)> {
    let source = app.emitted_editor_source(pane_id)?;
    let crate::app::PaneRuntime::Editor(editor) = app.panes.get(&pane_id)? else {
        return None;
    };
    if !std::sync::Arc::ptr_eq(&source.installed.key.identity, &editor.instance_identity())
        || source.installed.key.revision != editor.content_revision()
        || source.installed.key.path.as_path()
            != editor.path.as_deref().unwrap_or(std::path::Path::new(""))
    {
        return None;
    }
    source.position(position.x, position.y)
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
mod create_split_orientation_mouse_tests {
    use super::*;
    use crate::app::App;
    use crossterm::event::KeyModifiers;

    /// Regression test: the Vertical/Horizontal row hit-tests used to match
    /// on `mouse.row` alone, with no `x`/popup-containment guard -- unlike
    /// the same function's own empty-split branch. A click on that row's
    /// `y` but far outside the popup's `x` range (e.g. over the tree panel
    /// rendered underneath this modal) must cancel, not advance the flow.
    #[test]
    fn clicking_outside_the_popup_on_an_orientation_row_cancels_instead_of_advancing() {
        let mut app = App::new("test-session".to_string(), std::env::temp_dir());
        app.set_screen_area(Rect::new(0, 0, 120, 40));
        app.open_create_split_dialog();
        let popup = crate::modal::create_split_orientation_dialog_area(app.layout.screen_area);
        assert!(
            popup.x > 0,
            "popup must be inset from the screen edge for this test to be meaningful"
        );

        handle_mouse_event(
            &mut app,
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 0,
                row: popup.y + 3,
                modifiers: KeyModifiers::NONE,
            },
        );

        assert!(matches!(app.mode, Mode::Normal));
    }

    /// The row-only match is otherwise correct: clicking the Vertical row
    /// inside the popup still advances into the pane-picker step.
    #[test]
    fn clicking_inside_the_popup_on_the_vertical_row_advances_to_the_member_picker() {
        let mut app = App::new("test-session".to_string(), std::env::temp_dir());
        app.set_screen_area(Rect::new(0, 0, 120, 40));
        app.open_create_split_dialog();
        let popup = crate::modal::create_split_orientation_dialog_area(app.layout.screen_area);

        handle_mouse_event(
            &mut app,
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: popup.x + 1,
                row: popup.y + 3,
                modifiers: KeyModifiers::NONE,
            },
        );

        assert!(matches!(app.mode, Mode::CreateSplitMembers(_)));
    }
}

#[cfg(test)]
mod tree_rename_mouse_tests {
    use super::*;
    use crate::app::App;
    use crossterm::event::KeyModifiers;
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    fn app_with_folder() -> (App, NodeId) {
        let mut app = App::new("test-session".to_string(), std::env::temp_dir());
        app.set_screen_area(Rect::new(0, 0, 120, 40));
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        let folder = app
            .tree
            .add_folder(group, PathBuf::from("/tmp/lock-mouse"))
            .unwrap();
        app.select_node(folder);
        app.take_outbound_requests();
        (app, folder)
    }

    fn tree_row_position(app: &mut App, id: NodeId) -> Position {
        let area = app.layout.tree_area;
        (area.y..area.bottom())
            .flat_map(|row| (area.x..area.right()).map(move |column| Position::new(column, row)))
            .find(|&position| app.tree_node_at(position).is_some_and(|hit| hit.id == id))
            .expect("tree node must be visible")
    }

    fn click(app: &mut App, position: Position, kind: MouseEventKind) {
        handle_mouse_event(
            app,
            MouseEvent {
                kind,
                column: position.x,
                row: position.y,
                modifiers: KeyModifiers::NONE,
            },
        );
    }

    #[test]
    fn single_click_toggles_expand_and_persists_it() {
        let (mut app, folder) = app_with_folder();
        handle_lockable_left_click(&mut app, folder);
        assert_eq!(
            app.take_outbound_requests(),
            vec![ilium_ipc::ClientRequest::SetNodeExpanded {
                node_id: folder,
                expanded: true,
            }]
        );
    }

    #[test]
    fn rapid_second_click_opens_the_existing_rename_prompt() {
        let (mut app, folder) = app_with_folder();
        assert!(!is_tree_rename_double_click(&mut app, folder));
        app.take_outbound_requests();

        assert!(is_tree_rename_double_click(&mut app, folder));
        app.action_start_rename();
        assert!(matches!(app.mode, Mode::Rename(_)));
        assert!(app.take_outbound_requests().is_empty());
        assert!(app.last_tree_click.is_none());
    }

    #[test]
    fn a_click_after_the_double_click_window_is_a_fresh_single_click() {
        let (mut app, folder) = app_with_folder();
        app.last_tree_click = Some((folder, Instant::now() - Duration::from_millis(500)));

        assert!(!is_tree_rename_double_click(&mut app, folder));
        assert!(app.last_tree_click.is_some_and(|(id, _)| id == folder));
    }

    #[test]
    fn a_locked_folder_ignores_a_plain_click_but_a_double_click_renames_it() {
        let (mut app, folder) = app_with_folder();
        app.tree.set_node_locked_closed(folder, true).unwrap();

        handle_lockable_left_click(&mut app, folder);
        assert!(app.take_outbound_requests().is_empty());

        assert!(!is_tree_rename_double_click(&mut app, folder));
        assert!(is_tree_rename_double_click(&mut app, folder));
        app.action_start_rename();
        assert!(matches!(app.mode, Mode::Rename(_)));
    }

    #[test]
    fn disabling_lock_closed_does_not_disable_double_click_rename() {
        let (mut app, folder) = app_with_folder();
        app.ui_settings.lock_closed_enabled = false;

        assert!(!is_tree_rename_double_click(&mut app, folder));
        assert!(is_tree_rename_double_click(&mut app, folder));
        app.action_start_rename();
        assert!(matches!(app.mode, Mode::Rename(_)));
    }

    /// Regression: clicking a row, dismissing a modal with the mouse, then
    /// clicking the same row again inside `TREE_DOUBLE_CLICK_WINDOW` is two
    /// separate interactions, not a double-click -- the intervening press
    /// must drop the pending pair, or the row enters Rename without anyone
    /// asking for it.
    #[test]
    fn an_intervening_modal_click_cancels_a_pending_rename_double_click() {
        let (mut app, folder) = app_with_folder();
        app.set_screen_area(Rect::new(0, 0, 120, 40));
        assert!(!is_tree_rename_double_click(&mut app, folder));
        assert!(app.last_tree_click.is_some());

        // A press dispatched while a modal owns the screen: not part of the
        // pair, whatever it lands on.
        app.open_create_split_dialog();
        let popup = crate::modal::create_split_orientation_dialog_area(app.layout.screen_area);
        handle_mouse_event(
            &mut app,
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: popup.x + 1,
                row: popup.y + 3,
                modifiers: KeyModifiers::NONE,
            },
        );
        assert!(app.last_tree_click.is_none());
        app.take_outbound_requests();

        // The next click on the original row is therefore a fresh single
        // click: it expands the folder, not a rename.
        app.mode = Mode::Normal;
        assert!(!is_tree_rename_double_click(&mut app, folder));
        handle_lockable_left_click(&mut app, folder);
        assert_eq!(
            app.take_outbound_requests(),
            vec![ilium_ipc::ClientRequest::SetNodeExpanded {
                node_id: folder,
                expanded: true,
            }]
        );
    }

    #[test]
    fn real_tree_rows_double_click_through_the_full_mouse_pipeline() {
        let mut app = App::new("test-session".to_string(), std::env::temp_dir());
        app.set_screen_area(Rect::new(0, 0, 120, 40));
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        let pane = app
            .tree
            .add_pane(group, "a", ilium_core::PaneContentKind::Terminal)
            .unwrap();
        let split = app
            .tree
            .create_split_view(
                group,
                "split",
                ilium_core::SplitOrientation::Vertical,
                &[pane],
            )
            .unwrap();
        app.tree_state.open(vec![group]);
        app.tree_state.open(vec![group, split]);
        let position = tree_row_position(&mut app, pane);

        click(&mut app, position, MouseEventKind::Down(MouseButton::Left));
        click(&mut app, position, MouseEventKind::Up(MouseButton::Left));
        click(&mut app, position, MouseEventKind::Down(MouseButton::Left));

        assert!(matches!(app.mode, Mode::Rename(_)));
        assert_eq!(app.selected_node_id(), Some(pane));
        assert!(app.last_tree_click.is_none());
        assert!(app.tree.get(split).is_some());
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
        let mut app = App::new("test-session".to_string(), std::env::temp_dir());
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
        let mut app = App::new("test-session".to_string(), std::env::temp_dir());
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
mod agent_debug_mouse_tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    #[test]
    fn clicking_the_top_save_button_opens_the_destination_path_prompt() {
        let mut app = App::new("test-session".to_string(), std::env::temp_dir());
        app.set_screen_area(Rect::new(0, 0, 120, 40));
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        let pane_id = app
            .tree
            .add_pane(group, "codex", ilium_core::PaneContentKind::Terminal)
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
        let button = crate::agent_debug_ui::save_button_area(app.layout.pane_area);

        handle_mouse_event(
            &mut app,
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: button.x,
                row: button.y,
                modifiers: KeyModifiers::NONE,
            },
        );

        assert!(matches!(
            app.mode,
            Mode::AgentDebugSavePath(target, _) if target == pane_id
        ));
        assert!(matches!(
            app.modal_stack.as_slice(),
            [Mode::AgentDebugLog(crate::app::AgentDebugLogViewState { pane_id: parent, .. })]
                if *parent == pane_id
        ));
    }

    #[test]
    fn clicking_the_top_resize_filter_switch_reveals_animation_resizes() {
        let mut app = App::new("test-session".to_string(), std::env::temp_dir());
        app.set_screen_area(Rect::new(0, 0, 120, 40));
        let pane_id = ilium_core::NodeId(7);
        app.mode = Mode::AgentDebugLog(crate::app::AgentDebugLogViewState {
            pane_id,
            scroll_position: crate::app::AgentDebugLogScrollPosition::FromNewest(0),
        });
        let button = crate::agent_debug_ui::resize_filter_button_area(app.layout.pane_area);

        handle_mouse_event(
            &mut app,
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: button.x,
                row: button.y,
                modifiers: KeyModifiers::NONE,
            },
        );

        assert!(matches!(app.mode, Mode::AgentDebugLog(_)));
        assert!(
            !app.agent_debug_log_filter
                .is_tree_panel_animation_resize_hidden
        );
    }
}

#[cfg(test)]
mod tree_order_context_mouse_tests {
    use super::*;
    use crossterm::event::KeyModifiers;

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
        let mut app = App::new("test-session".to_string(), std::env::temp_dir());
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
                    .submenu
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

    #[test]
    fn hovering_agent_parent_opens_submenu_and_disabled_item_explains_itself() {
        let mut app = App::new("test-session".to_string(), std::env::temp_dir());
        app.set_screen_area(Rect::new(0, 0, 100, 30));
        app.open_context_menu(ROOT_ID, 2, 2);
        let (column, row) = match &app.mode {
            Mode::ContextMenu(menu) => {
                let index = menu
                    .actions
                    .iter()
                    .position(|action| {
                        *action
                            == crate::app::ContextMenuAction::NewAgent(
                                ilium_core::BuiltinAgentProvider::Claude,
                            )
                    })
                    .unwrap();
                (menu.area.x + 1, menu.area.y + 1 + index as u16)
            }
            _ => panic!("context menu should open"),
        };
        handle_mouse_event(
            &mut app,
            MouseEvent {
                kind: MouseEventKind::Moved,
                column,
                row,
                modifiers: KeyModifiers::NONE,
            },
        );
        let since = match &app.mode {
            Mode::ContextMenu(menu) => menu.hover_candidate.expect("hover candidate").1,
            _ => panic!("context menu should remain open"),
        };
        assert!(app.tick_context_menu_hover(since + Duration::from_millis(180)));
        let (item_column, item_row) = match &app.mode {
            Mode::ContextMenu(menu) => {
                let submenu = menu.submenu.as_ref().expect("agent submenu");
                (submenu.area.x + 1, submenu.area.y + 2)
            }
            _ => panic!("context menu should remain open"),
        };
        click(&mut app, item_column, item_row);
        assert!(matches!(app.mode, Mode::ContextMenu(_)));
        assert_eq!(
            app.status_message.as_deref(),
            Some("Checking Git repository…")
        );
    }
}

#[cfg(test)]
mod markdown_board_context_mouse_tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    #[test]
    fn right_clicking_a_markdown_editor_row_exposes_and_runs_create_board() {
        let path = std::env::temp_dir().join(format!(
            "ilium-board-mouse-{}-{}.md",
            std::process::id(),
            crate::scheduled_input::unix_millis_now()
        ));
        std::fs::write(&path, "# Work\n\n* [ ] Mouse task\n").unwrap();
        // Canonicalized through the same helper the app uses: `%TEMP%` hands
        // out an 8.3 short path (`RUNNER~1`) while resolving it yields the long
        // name, so an unresolved expectation cannot match what the app stores.
        let path = ilium_platform::paths::canonicalize(&path).unwrap_or(path);

        let mut app = App::new("test-session".to_string(), std::env::temp_dir());
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

        app.settle_filesystem_for_test();
        let requests = app.take_outbound_requests();
        assert!(
            matches!(
                requests.as_slice(),
                [ilium_ipc::ClientRequest::QueryRepoFacts { .. }, ilium_ipc::ClientRequest::NewBoard {
                    parent_group,
                    storage: ilium_core::BoardStorage::MarkdownFile { path: board_path },
                    ..
                }] if *parent_group == group && board_path == &path
            ),
            "{requests:?}"
        );
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(test)]
mod chatroom_scroll_mouse_tests {
    use super::*;
    use crate::app::{ChatroomViewState, RightPanelTarget};
    use crate::chatroom::ChatMessage;
    use crossterm::event::KeyModifiers;
    use std::path::PathBuf;

    fn mouse(kind: MouseEventKind, position: Position) -> MouseEvent {
        MouseEvent {
            kind,
            column: position.x,
            row: position.y,
            modifiers: KeyModifiers::NONE,
        }
    }

    #[test]
    fn wheel_and_scrollbar_drag_route_through_the_virtual_chatroom_view() {
        let mut app = App::new("test-session".to_string(), std::env::temp_dir());
        let project_id = app
            .tree
            .add_project(PathBuf::from("/tmp/chatroom-scroll-mouse"))
            .unwrap();
        app.chatrooms.insert(
            project_id,
            ChatroomViewState {
                messages: (0..30)
                    .map(|index| ChatMessage {
                        timestamp: format!("2026-07-26 14:{index:02}:00 +02:00"),
                        author: "agent:codex".to_string(),
                        content: format!("mouse history message {index:02}"),
                    })
                    .collect(),
                ..ChatroomViewState::default()
            },
        );
        app.right_panel_target = RightPanelTarget::Chatroom { project_id };
        app.set_screen_area(Rect::new(0, 0, 100, 18));
        let room_layout = crate::chatroom_ui::layout(app.layout.pane_area);
        let content_position = Position::new(
            room_layout.message_content_area.x,
            room_layout.message_content_area.y,
        );

        handle_mouse_event(&mut app, mouse(MouseEventKind::ScrollUp, content_position));
        assert_eq!(app.chatrooms[&project_id].scroll_from_newest, 3);

        handle_mouse_event(
            &mut app,
            mouse(MouseEventKind::ScrollDown, content_position),
        );
        assert_eq!(app.chatrooms[&project_id].scroll_from_newest, 0);

        let top_of_scrollbar =
            Position::new(room_layout.scrollbar_area.x, room_layout.scrollbar_area.y);
        handle_mouse_event(
            &mut app,
            mouse(MouseEventKind::Down(MouseButton::Left), top_of_scrollbar),
        );
        let oldest_metrics = app.chatroom_scroll_metrics(project_id).unwrap();
        assert_eq!(oldest_metrics.top, 0);
        assert!(app.is_chatroom_scrollbar_dragging());

        let bottom_row = room_layout.scrollbar_area.bottom().saturating_sub(1);
        let outside_pane_horizontally = Position::new(app.layout.tree_area.x, bottom_row);
        handle_mouse_event(
            &mut app,
            mouse(
                MouseEventKind::Drag(MouseButton::Left),
                outside_pane_horizontally,
            ),
        );
        let newest_metrics = app.chatroom_scroll_metrics(project_id).unwrap();
        assert_eq!(newest_metrics.top, newest_metrics.maximum_top);
        assert_eq!(app.chatrooms[&project_id].scroll_from_newest, 0);

        handle_mouse_event(
            &mut app,
            mouse(
                MouseEventKind::Up(MouseButton::Left),
                outside_pane_horizontally,
            ),
        );
        assert!(!app.is_chatroom_scrollbar_dragging());
    }
}

#[cfg(test)]
mod row_action_click_tests {
    use super::*;
    use crate::app::App;
    use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

    /// Locates `id`'s on-screen row (via the same hit-test path rendering
    /// uses) and clicks the row-action strip's rightmost (`Retitle`) slot on it, through
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

        // The strip is flush right and `Retitle` is always its last slot.
        let list = tree_ui::list_area(area);
        let click_pos = ratatui::layout::Position::new(list.right() - 1, row);

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
        let mut app = App::new("test-session".to_string(), std::env::temp_dir());
        app.set_screen_area(ratatui::layout::Rect::new(0, 0, 120, 40));
        app
    }

    #[test]
    fn project_separator_mouse_clicks_select_the_rendered_entry() {
        let mut app = test_app();
        app.ui_settings.show_project_separators = true;
        app.ui_settings.show_tree_row_management_controls = false;
        let mut panes = Vec::new();
        for index in 0..3 {
            let project = app
                .tree
                .add_project(std::path::PathBuf::from(format!(
                    "/nonexistent/ilium-separator-regression-{index}"
                )))
                .unwrap();
            let pane = app
                .tree
                .add_pane(
                    project,
                    format!("shell-{index}"),
                    ilium_core::PaneContentKind::Terminal,
                )
                .unwrap();
            app.panes.insert(
                pane,
                crate::app::PaneRuntime::Terminal(Box::new(
                    crate::terminal_view::TerminalView::new(24, 80),
                )),
            );
            app.tree_state.open(vec![project]);
            panes.push(pane);
        }
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 40)).unwrap();
        terminal
            .draw(|frame| crate::ui::draw(frame, &mut app))
            .unwrap();
        let list = tree_ui::list_area(app.layout.tree_area);
        // Project, pane, bar; project, pane, bar; project, pane.
        for (offset, expected) in [(4, panes[1]), (7, panes[2])] {
            let position = Position::new(list.x + 1, list.y + offset);
            let row: String = (list.x..list.right())
                .map(|x| terminal.backend().buffer()[(x, position.y)].symbol())
                .collect();
            assert!(
                row.contains(if offset == 4 { "shell-1" } else { "shell-2" }),
                "fixture row: {row}"
            );
            assert_eq!(
                tree_ui::row_action_at(
                    &app.tree,
                    expected,
                    app.layout.tree_area,
                    position.y,
                    position,
                    false
                ),
                None,
                "entry click must be outside the row action strip",
            );
            handle_mouse_event(
                &mut app,
                MouseEvent {
                    kind: MouseEventKind::Down(MouseButton::Left),
                    column: position.x,
                    row: position.y,
                    modifiers: KeyModifiers::NONE,
                },
            );
            assert_eq!(
                app.selected_node_id(),
                Some(expected),
                "clicked row {offset}"
            );
            assert_eq!(app.active_pane_id(), Some(expected));
        }
        let selected = app.selected_node_id();
        for offset in [2, 5] {
            handle_mouse_event(
                &mut app,
                MouseEvent {
                    kind: MouseEventKind::Down(MouseButton::Left),
                    column: list.x + 1,
                    row: list.y + offset,
                    modifiers: KeyModifiers::NONE,
                },
            );
            assert_eq!(app.selected_node_id(), selected, "bar must be inert");
        }
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
    fn appearance_sizing_cards_are_direct_mouse_targets() {
        let mut app = test_app();
        app.mode = Mode::Settings(crate::app::SettingsState::new());
        let content = crate::settings_ui::compute_layout(app.layout.screen_area).content_area;
        let width_dependent_column = content.right().saturating_sub(2);
        let card_row = content.y + 4;

        handle_mouse_event(
            &mut app,
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: width_dependent_column,
                row: card_row,
                modifiers: KeyModifiers::NONE,
            },
        );

        assert_eq!(
            app.ui_settings.left_panel_sizing.mode,
            crate::config::LeftPanelSizingMode::TerminalWidthDependent
        );
        assert!(matches!(
            &app.mode,
            Mode::Settings(state) if state.selected_row == 0
        ));
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

#[cfg(test)]
mod shared_dialog_mouse_tests {
    use super::*;
    use crate::app::{BoardStorageKind, CreateBoardState, SettingsState};
    use crate::text_prompt::TextPromptState;
    use crossterm::event::KeyModifiers;
    use ilium_ipc::ClientRequest;

    /// Clicks the center of one exact shared dialog target.
    fn click(app: &mut App, area: Rect) {
        handle_mouse_event(
            app,
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: area.x.saturating_add(area.width.saturating_sub(1) / 2),
                row: area.y.saturating_add(area.height.saturating_sub(1) / 2),
                modifiers: KeyModifiers::NONE,
            },
        );
    }

    /// Builds a predictable viewport so centered modal geometry is stable.
    fn app() -> App {
        let mut app = App::new("dialog-mouse-test".to_owned(), std::env::temp_dir());
        app.set_screen_area(Rect::new(0, 0, 120, 40));
        app
    }

    #[test]
    fn close_confirmation_buttons_preserve_cancel_and_confirm_keyboard_behavior() {
        let mut app = app();
        let group_id = app.tree.add_group(ROOT_ID, "work").unwrap();
        app.tree
            .add_pane(group_id, "shell", ilium_core::PaneContentKind::Terminal)
            .unwrap();
        let actions = crate::modal::confirm_dialog_layout(app.layout.screen_area).actions;

        app.mode = Mode::ConfirmClose(group_id);
        click(&mut app, actions.cancel_button);

        assert!(matches!(app.mode, Mode::Normal));
        assert!(app.take_outbound_requests().is_empty());

        app.mode = Mode::ConfirmClose(group_id);
        click(&mut app, actions.confirm_button);

        assert!(matches!(app.mode, Mode::Normal));
        assert_eq!(
            app.take_outbound_requests(),
            vec![ClientRequest::ClosePane { pane_id: group_id }]
        );
    }

    #[test]
    fn session_recovery_buttons_do_not_leak_clicks_to_the_obscured_tui() {
        let mut app = app();
        let actions = crate::modal::confirm_dialog_layout(app.layout.screen_area).actions;

        app.mode = Mode::ConfirmSessionRecovery { pane_count: 3 };
        click(&mut app, actions.cancel_button);
        assert_eq!(
            app.take_outbound_requests(),
            vec![ClientRequest::ResolveSessionRecovery { restore: false }]
        );

        app.mode = Mode::ConfirmSessionRecovery { pane_count: 3 };
        click(&mut app, actions.confirm_button);
        assert_eq!(
            app.take_outbound_requests(),
            vec![ClientRequest::ResolveSessionRecovery { restore: true }]
        );
    }

    #[test]
    fn text_prompt_accepts_cursor_placement_and_named_action_button_clicks() {
        let mut app = app();
        let group_id = app.tree.add_group(ROOT_ID, "before").unwrap();
        app.tree_state.select(vec![group_id]);
        let layout = crate::modal::text_prompt_dialog_layout(app.layout.screen_area);

        app.mode = Mode::Rename(TextPromptState::new("renamed"));
        click(&mut app, layout.actions.cancel_button);
        assert!(matches!(app.mode, Mode::Normal));
        assert!(app.take_outbound_requests().is_empty());

        app.mode = Mode::Rename(TextPromptState::new("renamed"));
        handle_mouse_event(
            &mut app,
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: layout.input_box.x.saturating_add(3),
                row: layout.input_box.y.saturating_add(1),
                modifiers: KeyModifiers::NONE,
            },
        );
        assert!(
            matches!(&app.mode, Mode::Rename(state) if state.cursor == 2),
            "input clicks should place the text cursor relative to the field"
        );

        click(&mut app, layout.actions.confirm_button);
        assert!(matches!(
            app.take_outbound_requests().as_slice(),
            [ClientRequest::RenameNode {
                node_id,
                title,
                short_title: None,
                inferred_icon: None,
            }] if *node_id == group_id && title == "renamed"
        ));
    }

    #[test]
    fn multiline_prompt_buttons_restore_the_exact_parent_and_apply_text() {
        let mut app = app();
        app.mode = Mode::Settings(SettingsState::default());
        app.push_modal(Mode::VoicePromptEditor(Box::new(
            crate::voice_settings::VoicePromptEditorState::new("Speak concisely."),
        )));
        let actions = crate::modal::multiline_prompt_dialog_layout(app.layout.screen_area).actions;

        click(&mut app, actions.confirm_button);

        assert!(matches!(app.mode, Mode::Settings(_)));
        assert!(app.modal_stack.is_empty());
        assert_eq!(app.voice_settings.custom_prompt, "Speak concisely.");
    }

    #[test]
    fn line_provider_pointer_cycles_both_directions_and_picker_retains_prompt() {
        use crate::agent_from_line::{
            AgentLaunchType, CreateAgentFocus, CreateAgentFromLineState, EditorSourceLine,
        };
        let directory = tempfile::tempdir().unwrap();
        let mut app = App::new("line-provider".into(), directory.path().into());
        app.set_screen_area(Rect::new(0, 0, 100, 30));
        let mut state = CreateAgentFromLineState::new(
            EditorSourceLine {
                pane_id: NodeId(12),
                path: directory.path().join("source.rs"),
                line_number: 17,
                text: "source line".into(),
            },
            ROOT_ID,
        );
        state.prompt =
            ratatui_textarea::TextArea::from(["authored first line", "authored second line"]);
        state.focus = CreateAgentFocus::AgentType;
        let original_provider = state.agent_type;
        let authored = state.prompt_text();
        let geometry =
            crate::agent_from_line::provider_control(app.layout.screen_area, &state).geometry();
        app.mode = Mode::CreateAgentFromLine(Box::new(state));
        click(&mut app, geometry.value);
        assert!(
            matches!(&app.mode, Mode::CreateAgentFromLine(state) if state.agent_type == original_provider.stepped(1) && state.prompt_text() == authored)
        );
        handle_mouse_event(
            &mut app,
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Right),
                column: geometry.value.x,
                row: geometry.value.y,
                modifiers: KeyModifiers::NONE,
            },
        );
        assert!(
            matches!(&app.mode, Mode::CreateAgentFromLine(state) if state.agent_type == original_provider)
        );
        click(&mut app, geometry.open);
        let Mode::ValueDialog(host) = &app.mode else {
            panic!("full provider catalog");
        };
        let crate::value_dialog::ValueDialogState::Choice(choice) = &host.dialog else {
            panic!("choice");
        };
        assert_eq!(choice.options().len(), AgentLaunchType::ALL.len());
        let Mode::ValueDialog(host) = std::mem::replace(&mut app.mode, Mode::Normal) else {
            panic!("catalog");
        };
        app.finish_value_dialog(
            host,
            crate::value_dialog::DialogOutcome::Choose("Codex".into()),
        );
        assert!(
            matches!(&app.mode, Mode::CreateAgentFromLine(state) if state.agent_type == AgentLaunchType::Codex && state.prompt_text() == authored)
        );
        assert!(app.take_outbound_requests().is_empty());
    }

    #[test]
    fn board_form_fields_and_create_button_are_fully_mouse_operable() {
        let project_path = std::env::temp_dir().join(format!(
            "ilium-board-dialog-mouse-{}-{}",
            std::process::id(),
            crate::scheduled_input::unix_millis_now()
        ));
        std::fs::create_dir_all(&project_path).unwrap();
        let mut app = App::new("dialog-mouse-test".to_owned(), project_path.clone());
        app.set_screen_area(Rect::new(0, 0, 120, 40));
        let group_id = app.tree.add_group(ROOT_ID, "work").unwrap();
        let board_path = project_path.join("planning.md");
        app.mode = Mode::CreateBoard(CreateBoardState {
            parent_group: group_id,
            name: TextPromptState::new("Planning"),
            path: TextPromptState::new(board_path.display().to_string()),
            storage_kind: BoardStorageKind::Folder,
            editing_path: false,
        });
        let layout = crate::modal::create_board_dialog_layout(app.layout.screen_area);

        let storage = crate::modal::create_board_storage_control(
            app.layout.screen_area,
            BoardStorageKind::Folder.label(),
        );
        click(&mut app, storage.geometry().next);
        assert!(matches!(
            &app.mode,
            Mode::CreateBoard(state)
                if state.storage_kind == BoardStorageKind::MarkdownFile
        ));

        click(&mut app, layout.path_box);
        assert!(matches!(
            &app.mode,
            Mode::CreateBoard(state) if state.editing_path
        ));

        click(&mut app, layout.actions.confirm_button);
        app.settle_filesystem_for_test();
        assert!(board_path.is_file());
        assert!(matches!(
            app.take_outbound_requests().as_slice(),
            [ClientRequest::NewBoard {
                parent_group,
                name,
                storage: ilium_core::BoardStorage::MarkdownFile { path },
            }] if *parent_group == group_id && name == "Planning" && path == &board_path
        ));
        std::fs::remove_dir_all(project_path).unwrap();
    }
}

#[cfg(test)]
mod smart_copy_mouse_tests {
    use super::*;
    use crate::agent_toolbar::AgentToolbarAction;
    use crate::app::{App, FocusTarget, PaneRuntime, RightPanelTarget};
    use crate::terminal_view::TerminalView;
    use crossterm::event::KeyModifiers;
    use ilium_core::{AgentActivity, AgentClass, PaneContentKind, ROOT_ID};

    fn app_with_candidate() -> (App, NodeId) {
        let mut app = App::new("smart-copy-mouse-test".to_owned(), std::env::temp_dir());
        let group_id = app.tree.add_group(ROOT_ID, "work").unwrap();
        let pane_id = app
            .tree
            .add_pane(group_id, "codex", PaneContentKind::Terminal)
            .unwrap();
        app.tree
            .set_pane_status(
                pane_id,
                ilium_core::PaneStatus::from_activity(AgentClass::Codex, AgentActivity::Idle, None),
            )
            .unwrap();
        let mut view = TerminalView::new(4, 40);
        view.feed(b"curl https://example.test/api\r\n");
        app.panes
            .insert(pane_id, PaneRuntime::Terminal(Box::new(view)));
        app.right_panel_target = RightPanelTarget::Pane { pane_id };
        app.focus = FocusTarget::Pane;
        app.set_screen_area(Rect::new(0, 0, 120, 40));
        app.execute_agent_toolbar_action(pane_id, AgentToolbarAction::SmartCopy);
        let request = app.take_pending_smart_copy_request().unwrap();
        app.apply_smart_copy_worker_event(crate::smart_copy_workers::SmartCopyWorkerEvent {
            retention: None,
            generation: request.generation,
            pane_id,
            update: crate::smart_copy_workers::SmartCopyWorkerUpdate::JsonLine(
                r#"{"label":"url","kind":"url","parts":[{"lines":[1]}]}"#.to_owned(),
            ),
        });
        (app, pane_id)
    }

    fn candidate_position(app: &App, pane_id: NodeId) -> Position {
        let area = app.smart_copy_terminal_area(pane_id).unwrap();
        let candidate = app
            .smart_copy_session
            .as_ref()
            .unwrap()
            .candidates
            .iter()
            .find(|candidate| candidate.kind == "url")
            .expect("the pre-scan should provide a URL candidate");
        let span = candidate.spans[0];
        Position::new(
            area.x.saturating_add(span.start_column),
            area.y.saturating_add(span.row),
        )
    }

    fn mouse(kind: MouseEventKind, position: Position) -> MouseEvent {
        MouseEvent {
            kind,
            column: position.x,
            row: position.y,
            modifiers: KeyModifiers::NONE,
        }
    }

    fn app_with_plain_terminal() -> (App, NodeId) {
        let mut app = App::new("smart-copy-light-test".to_owned(), std::env::temp_dir());
        let group_id = app.tree.add_group(ROOT_ID, "work").unwrap();
        let pane_id = app
            .tree
            .add_pane(group_id, "shell", PaneContentKind::Terminal)
            .unwrap();
        let mut view = TerminalView::new(4, 40);
        view.feed(b"curl https://example.test/api\r\n");
        app.panes
            .insert(pane_id, PaneRuntime::Terminal(Box::new(view)));
        app.right_panel_target = RightPanelTarget::Pane { pane_id };
        app.focus = FocusTarget::Pane;
        app.set_screen_area(Rect::new(0, 0, 120, 40));
        app.terminal_clipboard = Some(crate::terminal_clipboard::ClipboardService::fixture_queue(
            crate::execution::test_client(),
        ));
        (app, pane_id)
    }

    fn assert_failed_light_copy_restores_exact_existing_selection(app: &mut App) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while app.light_copy_retry_generation.is_none() {
            app.collect_light_copy_selection();
            if let Some(reply) = app
                .terminal_clipboard
                .as_ref()
                .unwrap()
                .fixture_process_next(|_| Err("fixture native unavailable".into()))
            {
                app.accept_clipboard_completion(reply);
            }
            assert!(
                Instant::now() < deadline,
                "owned original restitution deadline"
            );
            std::thread::yield_now();
        }
        assert!(matches!(app.mode, Mode::SmartCopy));
        assert!(
            app.smart_copy_preview.is_none(),
            "native failure cannot claim copied preview"
        );
        let original = app.smart_copy_session.as_ref().unwrap();
        assert_eq!(original.selected_count(), 1);
        assert_eq!(
            original.selected_parts().collect::<Vec<_>>(),
            ["https://example.test/api"]
        );
        assert!(app
            .status_message
            .as_ref()
            .unwrap()
            .contains("Enter retries"));
    }

    fn modified(kind: MouseEventKind, position: Position, modifiers: KeyModifiers) -> MouseEvent {
        MouseEvent {
            kind,
            column: position.x,
            row: position.y,
            modifiers,
        }
    }

    fn inside_pane(app: &App, pane_id: NodeId) -> Position {
        let area = app.pane_viewport(pane_id).unwrap().content_area;
        Position::new(area.x + 1, area.y)
    }

    #[test]
    fn holding_the_light_key_selects_clicks_and_copies_on_release() {
        let (mut app, pane_id) = app_with_plain_terminal();
        let inside = inside_pane(&app, pane_id);
        handle_mouse_event(
            &mut app,
            modified(MouseEventKind::Moved, inside, KeyModifiers::CONTROL),
        );
        assert!(matches!(app.mode, Mode::SmartCopy));
        assert!(app.smart_copy_light.is_some());
        let session = app.smart_copy_session.as_ref().expect("session");
        assert!(session.is_light);
        assert!(
            app.take_pending_smart_copy_request().is_none(),
            "no model call"
        );

        let url = candidate_position(&app, pane_id);
        handle_mouse_event(
            &mut app,
            modified(MouseEventKind::Moved, url, KeyModifiers::CONTROL),
        );
        handle_mouse_event(
            &mut app,
            modified(
                MouseEventKind::Up(MouseButton::Left),
                url,
                KeyModifiers::CONTROL,
            ),
        );
        assert_eq!(app.smart_copy_session.as_ref().unwrap().selected_count(), 1);
        assert!(
            app.smart_copy_preview.is_none(),
            "nothing is copied before release"
        );

        handle_mouse_event(
            &mut app,
            modified(MouseEventKind::Moved, url, KeyModifiers::NONE),
        );
        assert!(matches!(app.mode, Mode::Normal));
        assert!(app.smart_copy_light.is_none());
        assert_failed_light_copy_restores_exact_existing_selection(&mut app);
    }

    #[test]
    fn light_mode_respects_the_setting_and_the_configured_key() {
        let (mut app, pane_id) = app_with_plain_terminal();
        let position = inside_pane(&app, pane_id);
        let mut settings = app.terminal_settings;
        settings.smart_copy_light = false;
        app.apply_terminal_settings(settings);
        handle_mouse_event(
            &mut app,
            modified(MouseEventKind::Moved, position, KeyModifiers::CONTROL),
        );
        assert!(matches!(app.mode, Mode::Normal));

        settings.smart_copy_light = true;
        settings.smart_copy_light_key = crate::config::SmartCopyLightKey::Alt;
        app.apply_terminal_settings(settings);
        handle_mouse_event(
            &mut app,
            modified(MouseEventKind::Moved, position, KeyModifiers::CONTROL),
        );
        assert!(
            matches!(app.mode, Mode::Normal),
            "Ctrl is not the configured key"
        );
        handle_mouse_event(
            &mut app,
            modified(MouseEventKind::Moved, position, KeyModifiers::ALT),
        );
        assert!(matches!(app.mode, Mode::SmartCopy));
    }

    #[test]
    fn the_wheel_with_the_modifier_does_not_start_light_mode() {
        let (mut app, pane_id) = app_with_plain_terminal();
        let position = inside_pane(&app, pane_id);
        handle_mouse_event(
            &mut app,
            modified(MouseEventKind::ScrollUp, position, KeyModifiers::CONTROL),
        );
        assert!(matches!(app.mode, Mode::Normal));
    }

    #[test]
    fn key_release_event_escape_and_idle_grace_end_light_mode() {
        use crossterm::event::{
            Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, ModifierKeyCode,
        };
        let start = |app: &mut App, pane_id: NodeId| {
            let inside = inside_pane(app, pane_id);
            handle_mouse_event(
                app,
                modified(MouseEventKind::Moved, inside, KeyModifiers::CONTROL),
            );
            let url = candidate_position(app, pane_id);
            handle_mouse_event(
                app,
                modified(MouseEventKind::Moved, url, KeyModifiers::CONTROL),
            );
            handle_mouse_event(
                app,
                modified(
                    MouseEventKind::Up(MouseButton::Left),
                    url,
                    KeyModifiers::CONTROL,
                ),
            );
        };

        let (mut app, pane_id) = app_with_plain_terminal();
        start(&mut app, pane_id);
        crate::keys::handle_event(
            &mut app,
            Event::Key(KeyEvent {
                code: KeyCode::Modifier(ModifierKeyCode::LeftControl),
                modifiers: KeyModifiers::NONE,
                kind: KeyEventKind::Release,
                state: KeyEventState::NONE,
            }),
        );
        assert!(matches!(app.mode, Mode::Normal));
        assert_failed_light_copy_restores_exact_existing_selection(&mut app);

        let (mut app, pane_id) = app_with_plain_terminal();
        start(&mut app, pane_id);
        crate::keys::handle_event(
            &mut app,
            Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        );
        assert!(matches!(app.mode, Mode::Normal));
        assert!(app.smart_copy_preview.is_none(), "Escape copies nothing");

        let (mut app, pane_id) = app_with_plain_terminal();
        start(&mut app, pane_id);
        let now = Instant::now();
        assert!(!app.tick_smart_copy_light(now) || app.smart_copy_light.is_some());
        assert!(
            app.smart_copy_light.is_some(),
            "still held within the grace period"
        );
        app.tick_smart_copy_light(
            now + crate::smart_copy_light::RELEASE_IDLE_GRACE + Duration::from_millis(10),
        );
        assert!(app.smart_copy_light.is_none());
        assert_failed_light_copy_restores_exact_existing_selection(&mut app);
    }

    #[test]
    fn preview_disappears_after_a_second() {
        let (mut app, _pane_id) = app_with_plain_terminal();
        let now = Instant::now();
        app.smart_copy_preview = Some(crate::smart_copy_light::SmartCopyPreview::new(
            "text".into(),
            1,
            true,
            now,
        ));
        assert!(app.tick_smart_copy_light(now + Duration::from_millis(500)));
        assert!(app.smart_copy_preview.is_some());
        app.tick_smart_copy_light(now + Duration::from_millis(1001));
        assert!(app.smart_copy_preview.is_none());
    }

    #[test]
    fn smart_copy_release_is_not_lost_to_stale_tree_drag() {
        let (mut app, pane_id) = app_with_candidate();
        let position = candidate_position(&app, pane_id);
        app.begin_tree_drag(pane_id);

        handle_mouse_event(&mut app, mouse(MouseEventKind::Moved, position));
        assert_eq!(
            app.smart_copy_session
                .as_ref()
                .and_then(|session| session.current_candidate())
                .map(|candidate| candidate.kind.as_str()),
            Some("url")
        );
        app.status_message = None;
        handle_mouse_event(
            &mut app,
            mouse(MouseEventKind::Up(MouseButton::Left), position),
        );
        assert_ne!(
            app.status_message.as_deref(),
            Some("Move over a highlighted Smart Copy selection")
        );
    }

    #[test]
    fn smart_copy_release_only_hosts_still_copy_the_hovered_candidate() {
        let (mut app, pane_id) = app_with_candidate();
        let position = candidate_position(&app, pane_id);
        handle_mouse_event(&mut app, mouse(MouseEventKind::Moved, position));
        app.status_message = None;
        handle_mouse_event(
            &mut app,
            mouse(MouseEventKind::Up(MouseButton::Left), position),
        );
        assert_ne!(
            app.status_message.as_deref(),
            Some("Move over a highlighted Smart Copy selection")
        );
    }
}

#[cfg(test)]
mod cost_settings_mouse_tests {
    use super::*;
    use crate::app::{App, SettingsState, SettingsTab};
    use crate::cost_settings::{CostDisplay, CostRow, CostVisibility};
    use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

    fn cost_app() -> (tempfile::TempDir, App, ratatui::layout::Rect) {
        let directory = tempfile::tempdir().unwrap();
        let mut app = App::new("test-session".to_string(), directory.path().to_owned());
        app.config_dir = Some(directory.path().to_owned());
        app.set_screen_area(ratatui::layout::Rect::new(0, 0, 130, 260));
        app.mode = Mode::Settings(SettingsState {
            tab: SettingsTab::Cost,
            ..SettingsState::default()
        });
        let Mode::Settings(state) = &app.mode else {
            panic!("cost settings fixture");
        };
        let content =
            crate::settings_ui::compute_layout_for_mode(app.layout.screen_area, &app, state)
                .content_area;
        (directory, app, content)
    }

    fn click(app: &mut App, column: u16, row: u16) {
        handle_mouse_event(
            app,
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column,
                row,
                modifiers: KeyModifiers::empty(),
            },
        );
    }

    fn span_of(
        app: &App,
        content: ratatui::layout::Rect,
        row: CostRow,
    ) -> crate::cost_settings_ui::RowSpan {
        *crate::cost_settings_ui::view(app, 0, content.width)
            .rows
            .iter()
            .find(|span| span.row == row)
            .expect("the row is on the page")
    }

    #[test]
    fn clicks_toggle_options_choose_calibrations_and_step_values() {
        use crate::cost_model::Calibration;
        let (_directory, mut app, content) = cost_app();

        // Checkbox line of an option.
        let sparkline = span_of(&app, content, CostRow::Display(CostDisplay::Sparkline));
        click(&mut app, content.x + 8, content.y + sparkline.first_line);
        assert!(app.cost_settings.sparkline.enabled);

        // Its visibility selector.
        let visibility = span_of(&app, content, CostRow::Visibility(CostDisplay::Sparkline));
        let visibility_control =
            crate::cost_settings_ui::value_control(content, 0, &visibility, &app).unwrap();
        let visibility_value = visibility_control.geometry().value;
        click(&mut app, visibility_value.x, visibility_value.y);
        assert_eq!(
            app.cost_settings.sparkline.visibility,
            CostVisibility::Always
        );

        // A radio card anywhere inside it.
        let budget = span_of(&app, content, CostRow::Calibration(Calibration::Budget));
        click(&mut app, content.x + 10, content.y + budget.first_line + 1);
        assert_eq!(app.cost_settings.calibration, Calibration::Budget);

        // The parameter row that appeared: its rendered increment/decrement buttons.
        let amount = span_of(&app, content, CostRow::Budget);
        let amount_control =
            crate::cost_settings_ui::value_control(content, 0, &amount, &app).unwrap();
        let amount_geometry = amount_control.geometry();
        assert_eq!(app.cost_settings.budget_usd, 10.0);
        click(&mut app, amount_geometry.next.x, amount_geometry.next.y);
        assert_eq!(app.cost_settings.budget_usd, 20.0);
        click(
            &mut app,
            amount_geometry.previous.x,
            amount_geometry.previous.y,
        );
        assert_eq!(app.cost_settings.budget_usd, 10.0);

        // A click on a stepper's description changes nothing.
        click(&mut app, amount_geometry.next.x, amount_geometry.next.y + 1);
        assert_eq!(app.cost_settings.budget_usd, 10.0);

        let Mode::Settings(state) = &app.mode else {
            panic!("settings stay open");
        };
        assert!(state.selected_row > 0, "a click moves the selection");
    }
}

#[cfg(test)]
mod text_trigger_mouse_tests {
    use super::*;
    use crate::app::{App, SettingsState, SettingsTab};

    fn click_trigger_line(app: &mut App, line: u16) {
        let Mode::Settings(state) = &app.mode else {
            panic!("settings expected")
        };
        let area = crate::settings_ui::compute_layout_for_mode(app.layout.screen_area, app, state)
            .content_area;
        let click_row = area.y + line - state.scroll;
        handle_mouse_event(
            app,
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: area.x + 8,
                row: click_row,
                modifiers: crossterm::event::KeyModifiers::empty(),
            },
        );
    }

    fn settings_app(scroll: u16) -> App {
        let mut app = App::new("trigger-test".to_owned(), std::env::temp_dir());
        app.set_screen_area(Rect::new(0, 0, 120, 40));
        app.mode = Mode::Settings(SettingsState {
            tab: SettingsTab::TextTriggers,
            scroll,
            ..SettingsState::default()
        });
        app
    }

    #[test]
    fn clicking_add_opens_editor_and_escape_returns_to_settings() {
        let mut app = settings_app(0);
        click_trigger_line(&mut app, 4);
        assert!(
            matches!(&app.mode, Mode::TextTriggerDialog(state) if state.editing_index.is_none())
        );
        crate::keys::handle_event(
            &mut app,
            crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Esc,
                crossterm::event::KeyModifiers::empty(),
            )),
        );
        assert!(matches!(app.mode, Mode::Settings(_)));
    }

    #[test]
    fn wheel_scrolling_a_long_list_then_clicking_edits_the_visible_rule() {
        let mut app = settings_app(0);
        app.set_screen_area(Rect::new(0, 0, 80, 24));
        app.text_trigger_settings.triggers = (0..32)
            .map(|index| ilium_ipc::TextTrigger {
                id: format!("rule-{index}"),
                regexp: format!("pattern-{index}"),
                message: "reply".to_owned(),
                ..ilium_ipc::TextTrigger::default()
            })
            .collect();
        let Mode::Settings(state) = &app.mode else {
            panic!("settings expected")
        };
        let area = crate::settings_ui::compute_layout_for_mode(app.layout.screen_area, &app, state)
            .content_area;
        handle_mouse_event(
            &mut app,
            MouseEvent {
                kind: MouseEventKind::ScrollDown,
                column: area.x + 8,
                row: area.y + 4,
                modifiers: crossterm::event::KeyModifiers::empty(),
            },
        );
        let Mode::Settings(state) = &app.mode else {
            panic!("settings expected")
        };
        assert!(state.scroll > 0);
        click_trigger_line(&mut app, 10);
        assert!(
            matches!(&app.mode, Mode::TextTriggerDialog(state) if state.editing_index == Some(7) && state.regexp.buf == "pattern-7")
        );
    }

    #[test]
    fn clicking_headers_spacers_and_hint_does_not_open_an_editor() {
        for line in [0, 1, 2, 3, 5] {
            let mut app = settings_app(0);
            click_trigger_line(&mut app, line);
            assert!(
                matches!(app.mode, Mode::Settings(_)),
                "line {line} is inert"
            );
            assert!(app.take_outbound_requests().is_empty());
        }
    }

    #[test]
    fn clicking_existing_scrolled_rule_opens_edit_without_mutation() {
        let mut app = settings_app(2);
        let rule = ilium_ipc::TextTrigger {
            id: "stable-rule".to_owned(),
            regexp: "test".to_owned(),
            message: "reply".to_owned(),
            ..ilium_ipc::TextTrigger::default()
        };
        app.text_trigger_settings.triggers.push(rule.clone());
        click_trigger_line(&mut app, 3);
        assert!(
            matches!(&app.mode, Mode::TextTriggerDialog(state) if state.editing_index == Some(0) && state.regexp.buf == "test")
        );
        assert_eq!(app.text_trigger_settings.triggers, vec![rule]);
    }
}

#[cfg(test)]
mod tree_pane_focus_release_tests {
    use super::*;
    use crate::app::{FocusTarget, PaneRuntime};
    use crossterm::event::KeyModifiers;

    #[test]
    fn stale_release_keeps_completed_action_status_and_still_fences_new_press() {
        let mut app = App::new("release-status-fixture".into(), std::env::temp_dir());
        app.set_screen_area(Rect::new(0, 0, 120, 40));
        app.mode = Mode::Settings(crate::app::SettingsState::default());
        let emitted = app.capture_emitted_geometry(1);
        app.commit_emitted_geometry(emitted);
        // Completion closes the presented dialog before its queued release.
        app.mode = Mode::Normal;
        app.status_message = Some("History file path copied to clipboard".into());
        assert!(!app.pointer_geometry_is_current());
        let event = |kind| MouseEvent {
            kind,
            column: 10,
            row: 10,
            modifiers: KeyModifiers::NONE,
        };
        handle_mouse_event(&mut app, event(MouseEventKind::Up(MouseButton::Left)));
        assert_eq!(
            app.status_message.as_deref(),
            Some("History file path copied to clipboard")
        );
        assert!(app.take_outbound_requests().is_empty());
        handle_mouse_event(&mut app, event(MouseEventKind::Down(MouseButton::Left)));
        assert_eq!(
            app.status_message.as_deref(),
            Some("Waiting for terminal presentation before pointer input")
        );
        assert!(app.take_outbound_requests().is_empty());
    }
    #[test]
    fn terminal_tree_click_keeps_pane_focus_after_presented_press_and_release() {
        let mut app = App::new("tree-focus-fixture".into(), std::env::temp_dir());
        app.set_screen_area(Rect::new(0, 0, 120, 40));
        app.ui_settings.show_tree_row_management_controls = false;
        let group = app.tree.add_group(ROOT_ID, "recovery").unwrap();
        let pane_id = app
            .tree
            .add_pane(group, "agent", ilium_core::PaneContentKind::Terminal)
            .unwrap();
        app.panes.insert(
            pane_id,
            PaneRuntime::Terminal(Box::new(crate::terminal_view::TerminalView::new(24, 80))),
        );
        app.tree_state.open(vec![group]);
        let area = app.layout.tree_area;
        let row = (area.y..area.bottom())
            .find(|&row| {
                app.tree_node_at(Position::new(area.x + 12, row))
                    .is_some_and(|hit| hit.id == pane_id)
            })
            .expect("the exact terminal row must be visible");
        let event = |kind| MouseEvent {
            kind,
            column: area.x + 12,
            row,
            modifiers: KeyModifiers::NONE,
        };
        handle_mouse_event(&mut app, event(MouseEventKind::Down(MouseButton::Left)));
        assert_eq!(
            app.focus,
            FocusTarget::Pane,
            "the terminal press must focus its pane"
        );
        assert_eq!(app.active_pane_id(), Some(pane_id));
        app.take_outbound_requests();
        // Accept a matching emitted frame between press and release. A stale
        // geometry refusal must not accidentally hide the release regression.
        let emitted = app.capture_emitted_geometry(1);
        app.commit_emitted_geometry(emitted);
        assert!(app.pointer_geometry_is_current());
        handle_mouse_event(&mut app, event(MouseEventKind::Up(MouseButton::Left)));
        assert_eq!(
            app.focus,
            FocusTarget::Pane,
            "accepted tree release stole recalled-prompt keyboard focus"
        );
        assert_eq!(app.active_pane_id(), Some(pane_id));
        assert!(!app.take_outbound_requests().iter().any(|request| matches!(request,
            ilium_ipc::ClientRequest::SetPaneFocus { pane_id: id, focused: false } if *id == pane_id)),
            "click release must not publish a false pane blur");
    }
}

#[cfg(test)]
mod icon_assignment_control_tests {
    use super::*;

    #[test]
    fn assignment_pointer_preserves_label_and_reverses_value_then_opens_full_catalog() {
        let directory = tempfile::tempdir().unwrap();
        let mut app = App::new("icon-assignment-pointer".into(), directory.path().into());
        app.config_dir = Some(directory.path().into());
        let screen = Rect::new(0, 0, 140, 45);
        app.set_screen_area(screen);
        let target = crate::agent_monitoring::general_icon_targets()[0];
        let suggestions = target.suggestions();
        app.ui_settings.icons.set(target, suggestions[0].into());
        let mut state = crate::app::SettingsState {
            tab: crate::app::SettingsTab::Icons,
            ..Default::default()
        };
        let mut area =
            crate::settings_ui::compute_layout_for_mode(screen, &app, &state).content_area;
        let height = crate::instruction_settings::panel_height(state.tab, area);
        area.y += height;
        area.height = area.height.saturating_sub(height);
        for (button, part, expected, opens) in [
            (MouseButton::Left, 0, 0, false),
            (MouseButton::Left, 1, 1, false),
            (MouseButton::Right, 1, 0, false),
            (MouseButton::Left, 2, 0, true),
        ] {
            let g = crate::settings_ui::icon_assignment_control(
                area,
                0,
                0,
                app.ui_settings.icons.glyph(target),
            )
            .unwrap()
            .geometry();
            let rect = match part {
                0 => g.label,
                1 => g.value,
                _ => g.open,
            };
            handle_settings_mouse(
                &mut app,
                state,
                MouseEvent {
                    kind: MouseEventKind::Down(button),
                    column: rect.x,
                    row: rect.y,
                    modifiers: crossterm::event::KeyModifiers::NONE,
                },
            );
            assert_eq!(app.ui_settings.icons.glyph(target), suggestions[expected]);
            let Mode::Settings(next) = std::mem::replace(&mut app.mode, Mode::Normal) else {
                panic!("settings retained")
            };
            state = next;
            assert_eq!(state.icon_picker.is_some(), opens);
        }
        assert_eq!(state.icon_picker.unwrap().target, target);
    }
}

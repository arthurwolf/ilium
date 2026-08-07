//! Synchronous command execution against `App`'s existing semantic methods.

use std::path::PathBuf;

use ilium_core::{
    AgentProvider, BuiltinAgentProvider, PromptQueueDelivery, SplitOrientation, TreeMoveDirection,
    MAXIMUM_SPLIT_VIEW_PANES,
};
use ilium_ipc::{ClientRequest, PromptSubmissionSource};
use serde::Serialize;
use serde_json::{json, Value};

use crate::app::{
    App, BoardStorageKind, ClientExitReason, CreateBoardState, Mode, PaneRuntime, SettingsState,
    SettingsTab,
};
use crate::text_prompt::TextPromptState;

use super::command::*;
use super::resolver::{resolve_node, resolve_parent};

#[derive(Debug, Serialize)]
pub struct ExecutionReceipt {
    pub status: &'static str,
    pub message: String,
    pub data: Value,
    /// Provider-control metadata, omitted from the JSON result sent back to
    /// the model. The voice actor consumes it only after writing that result.
    #[serde(skip)]
    pub terminate_session_after_delivery: bool,
}

impl ExecutionReceipt {
    pub(super) fn immediate(message: impl Into<String>) -> Self {
        Self {
            status: "ok",
            message: message.into(),
            data: Value::Null,
            terminate_session_after_delivery: false,
        }
    }

    pub(super) fn queued(message: impl Into<String>) -> Self {
        Self {
            status: "queued",
            message: message.into(),
            data: json!({ "verification": "Read ilium_get_state after the server confirms the mutation." }),
            terminate_session_after_delivery: false,
        }
    }

    /// Completes a function call without asking the provider for another
    /// response because this result is the final frame of the voice session.
    fn terminating(message: impl Into<String>) -> Self {
        Self {
            status: "ok",
            message: message.into(),
            data: Value::Null,
            terminate_session_after_delivery: true,
        }
    }
}

pub fn execute(app: &mut App, command: ControlCommand) -> Result<ExecutionReceipt, String> {
    match command {
        ControlCommand::State(command) => {
            let snapshot = super::snapshot::capture(app, command.detail, &command.target)?;
            Ok(ExecutionReceipt {
                status: "ok",
                message: "Current ilium state".to_owned(),
                data: serde_json::to_value(snapshot).map_err(|error| error.to_string())?,
                terminate_session_after_delivery: false,
            })
        }
        ControlCommand::Ui(command) => execute_ui(app, command),
        ControlCommand::Tree(command) => execute_tree(app, command),
        ControlCommand::TerminalSubmission(command) => execute_terminal_submission(app, command),
        ControlCommand::TerminalTyping(command) => execute_terminal_typing(app, command),
        ControlCommand::Terminal(command) => execute_terminal(app, command),
        ControlCommand::Editor(command) => execute_editor(app, command),
        ControlCommand::Board(command) => execute_board(app, command),
        ControlCommand::Settings(command) => super::settings::execute(app, command),
        ControlCommand::Search(command) => execute_search(app, command),
        ControlCommand::Session(command) => execute_session(app, command),
        ControlCommand::StopVoiceMode(command) => execute_stop_voice_mode(app, command),
    }
}

/// Sends voice-originated text and its submission key in one request so the
/// PTY cannot observe the command without the final Enter or interleave input.
fn execute_terminal_submission(
    app: &mut App,
    command: TerminalSubmissionCommand,
) -> Result<ExecutionReceipt, String> {
    let pane_id = resolve_node(app, &command.target)?;
    require_terminal(app, pane_id)?;
    let mut bytes = required_nonempty(Some(command.text), "text")?.into_bytes();
    bytes.push(b'\r');
    app.send_terminal_bytes(pane_id, bytes, Some(PromptSubmissionSource::VoiceControl));
    Ok(ExecutionReceipt::queued(
        "Sent and submitted text to the terminal",
    ))
}

/// Types text without Enter for an explicit staging request or the local
/// submission-confirmation preparation step.
fn execute_terminal_typing(
    app: &mut App,
    command: TerminalTypingCommand,
) -> Result<ExecutionReceipt, String> {
    let pane_id = resolve_node(app, &command.target)?;
    require_terminal(app, pane_id)?;
    let bytes = required_nonempty(Some(command.text), "text")?.into_bytes();
    app.send_terminal_bytes(pane_id, bytes, None);
    Ok(ExecutionReceipt::queued("Staged text in the terminal"))
}

fn execute_search(app: &mut App, command: SearchCommand) -> Result<ExecutionReceipt, String> {
    let requested_query = if matches!(command.action, SearchAction::Query) {
        Some(required_nonempty(command.query.clone(), "query")?)
    } else {
        None
    };
    let mut state = match std::mem::replace(&mut app.mode, Mode::Normal) {
        Mode::Search(state) => state,
        _ => Box::default(),
    };
    match command.action {
        SearchAction::Query => {
            state.query = TextPromptState::new(requested_query.unwrap_or_default());
            app.refresh_search_results(&mut state);
            if let Some(index) = command.index {
                if index >= state.results.len() {
                    app.mode = Mode::Search(state);
                    return Err(format!("Search result index {index} does not exist"));
                }
                state.selected_index = index;
            }
        }
        SearchAction::SelectNext => state.move_selection(1, usize::MAX),
        SearchAction::SelectPrevious => state.move_selection(-1, usize::MAX),
        SearchAction::OpenResult => {
            if let Some(index) = command.index {
                if index >= state.results.len() {
                    app.mode = Mode::Search(state);
                    return Err(format!("Search result index {index} does not exist"));
                }
                state.selected_index = index;
            }
            let Some(result) = state.selected_result().cloned() else {
                app.mode = Mode::Search(state);
                return Err("There is no selected search result".to_owned());
            };
            app.activate_search_result(result);
            return Ok(ExecutionReceipt::immediate(
                "Opened the selected search result",
            ));
        }
        SearchAction::Close => {
            app.mode = Mode::Normal;
            return Ok(ExecutionReceipt::immediate("Closed workspace search"));
        }
    }
    let results = state
        .results
        .iter()
        .enumerate()
        .map(|(index, result)| {
            json!({
                "index": index,
                "selected": index == state.selected_index,
                "pane_id": result.pane_id.0,
                "kind": result.kind.label(),
                "name": result.object_name,
                "path": result.path.as_ref().map(|path| path.display().to_string()),
                "context": format!("{}{}{}", result.before, result.matched, result.after),
            })
        })
        .collect::<Vec<_>>();
    let query = state.query.buf.clone();
    app.mode = Mode::Search(state);
    Ok(ExecutionReceipt {
        status: "ok",
        message: format!("Found {} workspace results for {query:?}", results.len()),
        data: json!({ "query": query, "results": results }),
        terminate_session_after_delivery: false,
    })
}

fn execute_ui(app: &mut App, command: UiCommand) -> Result<ExecutionReceipt, String> {
    match command.action {
        UiAction::FocusTree => {
            app.leave_pane_focus();
            Ok(ExecutionReceipt::immediate("Focused the left tree panel"))
        }
        UiAction::FocusPane => {
            let pane_id = resolve_node(app, &command.target)?;
            require_runtime_kind(app, pane_id, "pane")?;
            app.focus_pane(pane_id);
            Ok(ExecutionReceipt::immediate(format!(
                "Focused pane {}",
                pane_id.0
            )))
        }
        UiAction::FocusNextPane => {
            app.focus_visible_pane(1);
            Ok(ExecutionReceipt::immediate("Focused the next visible pane"))
        }
        UiAction::FocusPreviousPane => {
            app.focus_visible_pane(-1);
            Ok(ExecutionReceipt::immediate(
                "Focused the previous visible pane",
            ))
        }
        UiAction::FocusPaneDirection => {
            let direction = command.direction.ok_or("direction is required")?;
            let (horizontal, vertical) = match direction {
                Direction::Left => (-1, 0),
                Direction::Right => (1, 0),
                Direction::Up => (0, -1),
                Direction::Down => (0, 1),
            };
            app.focus_visible_pane_in_direction(horizontal, vertical);
            Ok(ExecutionReceipt::immediate(
                "Moved focus within the visible split",
            ))
        }
        UiAction::ShowSplit => {
            let split_id = resolve_node(app, &command.target)?;
            if !app
                .tree
                .get(split_id)
                .is_some_and(|node| node.is_split_view())
            {
                return Err(format!("Node {} is not a split view", split_id.0));
            }
            app.show_split_view(split_id);
            Ok(ExecutionReceipt::immediate(format!(
                "Showing split {}",
                split_id.0
            )))
        }
        UiAction::OpenSettings => {
            app.action_open_settings();
            Ok(ExecutionReceipt::immediate("Opened Settings"))
        }
        UiAction::ShowSettingsTab => {
            let tab = settings_tab(command.tab.as_deref().ok_or("tab is required")?)?;
            let mut state = match std::mem::replace(&mut app.mode, Mode::Normal) {
                Mode::Settings(state) => state,
                _ => SettingsState::new(),
            };
            state.tab = tab;
            state.selected_row = 0;
            state.scroll = 0;
            app.mode = Mode::Settings(state);
            Ok(ExecutionReceipt::immediate(format!(
                "Opened {} settings",
                tab.label()
            )))
        }
        UiAction::OpenSearch => {
            app.action_open_search();
            Ok(ExecutionReceipt::immediate("Opened workspace search"))
        }
        UiAction::OpenHelp => {
            app.mode = Mode::Help;
            Ok(ExecutionReceipt::immediate("Opened Help"))
        }
        UiAction::CloseOverlay => {
            app.mode = Mode::Normal;
            Ok(ExecutionReceipt::immediate("Closed the current overlay"))
        }
    }
}

fn execute_tree(app: &mut App, command: TreeCommand) -> Result<ExecutionReceipt, String> {
    match command.action {
        TreeAction::CreateTerminal => {
            let parent = resolve_parent(app, &command.parent)?;
            app.request_new_terminal(parent);
            Ok(ExecutionReceipt::queued("Creating a terminal pane"))
        }
        TreeAction::CreateAgent => {
            let parent = resolve_parent(app, &command.parent)?;
            let provider = match command.provider.ok_or("provider is required")? {
                AgentProviderChoice::Claude => BuiltinAgentProvider::Claude,
                AgentProviderChoice::Codex => BuiltinAgentProvider::Codex,
                AgentProviderChoice::Antigravity => BuiltinAgentProvider::Antigravity,
            };
            if let Some(initial_input) = command.initial_input {
                app.request_new_command_pane_with_input(
                    parent,
                    provider.command_line().to_owned(),
                    initial_input,
                );
            } else {
                app.request_new_command_pane(parent, provider.command_line().to_owned());
            }
            Ok(ExecutionReceipt::queued(format!(
                "Creating a {} agent pane",
                provider.label()
            )))
        }
        TreeAction::CreateCommandPane => {
            let parent = resolve_parent(app, &command.parent)?;
            let command_line = required_nonempty(command.command_line, "command_line")?;
            if let Some(initial_input) = command.initial_input {
                app.request_new_command_pane_with_input(parent, command_line, initial_input);
            } else {
                app.request_new_command_pane(parent, command_line);
            }
            Ok(ExecutionReceipt::queued("Creating the command pane"))
        }
        TreeAction::OpenEditor => {
            let parent = resolve_parent(app, &command.parent)?;
            let path = resolve_filesystem_path(app, required_nonempty(command.path, "path")?);
            app.request_new_editor(parent, path);
            Ok(ExecutionReceipt::queued(
                "Opening the file in an editor pane",
            ))
        }
        TreeAction::AddFolder => {
            let parent = resolve_parent(app, &command.parent)?;
            let path = resolve_filesystem_path(app, required_nonempty(command.path, "path")?);
            app.request_new_folder(parent, path);
            Ok(ExecutionReceipt::queued(
                "Adding the folder to the left panel",
            ))
        }
        TreeAction::AddProject => {
            let path = resolve_filesystem_path(app, required_nonempty(command.path, "path")?);
            app.request_new_project(path);
            Ok(ExecutionReceipt::queued("Adding the project"))
        }
        TreeAction::ChangeProjectFolder => {
            let project_id = resolve_node(app, &command.target)?;
            let path = resolve_filesystem_path(app, required_nonempty(command.path, "path")?);
            app.request_change_project_folder(project_id, path);
            Ok(ExecutionReceipt::queued("Changing the project's folder"))
        }
        TreeAction::CreateGroup => {
            let parent = resolve_parent(app, &command.parent)?;
            let name = required_nonempty(command.name, "name")?;
            app.request_new_group(parent, name);
            Ok(ExecutionReceipt::queued("Creating the group"))
        }
        TreeAction::CreateBoard => {
            let parent_group = resolve_parent(app, &command.parent)?;
            let name = required_nonempty(command.name, "name")?;
            let path = required_nonempty(command.path, "path")?;
            let storage_kind = match command.storage.unwrap_or(BoardStorageChoice::Markdown) {
                BoardStorageChoice::Markdown => BoardStorageKind::MarkdownFile,
                BoardStorageChoice::Folder => BoardStorageKind::Folder,
            };
            app.commit_create_board(&CreateBoardState {
                parent_group,
                name: TextPromptState::new(name),
                path: TextPromptState::new(path),
                storage_kind,
                editing_path: false,
            });
            if matches!(app.mode, Mode::CreateBoard(_)) {
                return Err(app
                    .status_message
                    .clone()
                    .unwrap_or_else(|| "Board creation failed".to_owned()));
            }
            Ok(ExecutionReceipt::queued("Creating the board"))
        }
        TreeAction::CreateSplit => {
            let parent_group = resolve_parent(app, &command.parent)?;
            let name = command.name.unwrap_or_else(|| "Split".to_owned());
            let orientation = match command.orientation.unwrap_or(SplitDirection::Vertical) {
                SplitDirection::Horizontal => SplitOrientation::Horizontal,
                SplitDirection::Vertical => SplitOrientation::Vertical,
            };
            let pane_ids = command
                .members
                .iter()
                .map(|target| resolve_node(app, target))
                .collect::<Result<Vec<_>, _>>()?;
            if pane_ids.len() > MAXIMUM_SPLIT_VIEW_PANES {
                return Err(format!(
                    "A split view can contain at most {MAXIMUM_SPLIT_VIEW_PANES} panes"
                ));
            }
            app.queue_request(ClientRequest::CreateSplitView {
                parent_group,
                name,
                orientation,
                pane_ids,
            });
            Ok(ExecutionReceipt::queued("Creating the split view"))
        }
        TreeAction::Rename => {
            let node_id = resolve_node(app, &command.target)?;
            let name = required_nonempty(command.name, "name")?;
            app.request_rename(node_id, name, None, None);
            Ok(ExecutionReceipt::queued("Renaming the tree item"))
        }
        TreeAction::MoveUp | TreeAction::MoveDown => {
            let node_id = resolve_node(app, &command.target)?;
            let direction = if matches!(command.action, TreeAction::MoveUp) {
                TreeMoveDirection::Up
            } else {
                TreeMoveDirection::Down
            };
            app.request_move(node_id, direction);
            Ok(ExecutionReceipt::queued("Moving the tree item"))
        }
        TreeAction::Reparent => {
            let node_id = resolve_node(app, &command.target)?;
            let new_parent = resolve_parent(app, &command.parent)?;
            app.request_reparent(node_id, new_parent, command.index);
            Ok(ExecutionReceipt::queued("Reparenting the tree item"))
        }
        TreeAction::Close => {
            let node_id = resolve_node(app, &command.target)?;
            app.request_close(node_id);
            Ok(ExecutionReceipt::queued("Closing the tree item"))
        }
        TreeAction::ToggleExpanded => {
            let node_id = resolve_node(app, &command.target)?;
            if !app
                .tree
                .get(node_id)
                .is_some_and(|node| node.is_container())
            {
                return Err(format!("Node {} cannot be expanded", node_id.0));
            }
            app.select_node(node_id);
            app.toggle_selected_tree_node();
            Ok(ExecutionReceipt::immediate("Toggled the left-panel group"))
        }
        TreeAction::Retitle => {
            let node_id = resolve_node(app, &command.target)?;
            app.action_request_retitle(node_id);
            Ok(ExecutionReceipt::queued(
                app.status_message
                    .clone()
                    .unwrap_or_else(|| "Started automatic retitling".to_owned()),
            ))
        }
        TreeAction::RestructureAllProjects => {
            app.action_request_restructure();
            Ok(ExecutionReceipt::queued(
                app.restructure_status_text()
                    .unwrap_or_else(|| "Requested project restructuring".to_owned()),
            ))
        }
        TreeAction::RestructureProject => {
            let project_id = resolve_node(app, &command.target)?;
            app.action_request_project_restructure(project_id);
            Ok(ExecutionReceipt::queued(
                app.restructure_status_text()
                    .unwrap_or_else(|| "Requested project restructuring".to_owned()),
            ))
        }
        TreeAction::RevertProjectRestructure => {
            let project_id = resolve_node(app, &command.target)?;
            app.request_revert_project_restructure(project_id);
            Ok(ExecutionReceipt::queued(
                "Reverting the project's latest restructure",
            ))
        }
    }
}

fn execute_terminal(app: &mut App, command: TerminalCommand) -> Result<ExecutionReceipt, String> {
    let pane_id = resolve_node(app, &command.target)?;
    require_terminal(app, pane_id)?;
    match command.action {
        TerminalAction::PressKey => {
            let key = command.key.ok_or("key is required")?;
            app.queue_request(ClientRequest::KeyInput {
                pane_id,
                bytes: terminal_key_bytes(key).to_vec(),
                submission: matches!(key, TerminalKey::Enter)
                    .then_some(PromptSubmissionSource::VoiceControl),
            });
            Ok(ExecutionReceipt::queued("Sent the key to the terminal"))
        }
        TerminalAction::ScrollUp | TerminalAction::ScrollDown => {
            let lines = command.lines.unwrap_or(app.last_known_pane_size.0.max(1));
            let Some(PaneRuntime::Terminal(view)) = app.panes.get_mut(&pane_id) else {
                return Err("Target is not a terminal pane".to_owned());
            };
            if matches!(command.action, TerminalAction::ScrollUp) {
                view.scroll_up(lines);
            } else {
                view.scroll_down(lines);
            }
            Ok(ExecutionReceipt::immediate("Scrolled the terminal"))
        }
        TerminalAction::ScrollToBottom => {
            if let Some(PaneRuntime::Terminal(view)) = app.panes.get_mut(&pane_id) {
                view.scroll_to_bottom();
            }
            Ok(ExecutionReceipt::immediate(
                "Returned to live terminal output",
            ))
        }
        TerminalAction::ScheduleInput => {
            let text = command.text.unwrap_or_default();
            app.queue_request(ClientRequest::SchedulePaneInput {
                pane_id,
                delay_seconds: command.delay_seconds.unwrap_or(0),
                text,
                send_enter: true,
            });
            Ok(ExecutionReceipt::queued("Scheduled terminal input"))
        }
        TerminalAction::QueuePrompt => {
            let text = required_nonempty(command.text, "text")?;
            let delivery = match command.delivery.unwrap_or(PromptDeliveryChoice::Once) {
                PromptDeliveryChoice::Once => PromptQueueDelivery::Once,
                PromptDeliveryChoice::Times => PromptQueueDelivery::Times {
                    remaining_runs: command
                        .runs
                        .filter(|runs| *runs > 0)
                        .ok_or("runs is required and must be positive when delivery is times")?,
                },
                PromptDeliveryChoice::Forever => PromptQueueDelivery::Forever,
            };
            app.queue_request(ClientRequest::EnqueuePrompt {
                pane_id,
                text,
                delivery,
            });
            Ok(ExecutionReceipt::queued(
                "Queued the prompt for the agent's next completion",
            ))
        }
        TerminalAction::ClearPromptQueue => {
            app.queue_request(ClientRequest::ClearPromptQueue { pane_id });
            Ok(ExecutionReceipt::queued("Clearing the pane's prompt queue"))
        }
    }
}

fn execute_editor(app: &mut App, command: EditorCommand) -> Result<ExecutionReceipt, String> {
    let pane_id = resolve_node(app, &command.target)?;
    require_editor(app, pane_id)?;
    app.focus_pane(pane_id);
    let content_revision_before = match app.panes.get(&pane_id) {
        Some(PaneRuntime::Editor(editor)) => editor.content_revision(),
        _ => return Err("Target is not an editor pane".to_owned()),
    };
    match command.action {
        EditorAction::Save => app.action_save_focused_editor(),
        EditorAction::SaveAs => {
            app.action_save_as(pane_id, required_nonempty(command.path, "path")?)
        }
        EditorAction::InsertText => {
            let text = command.text.ok_or("text is required")?;
            let Some(PaneRuntime::Editor(editor)) = app.panes.get_mut(&pane_id) else {
                return Err("Target is not an editor pane".to_owned());
            };
            editor.insert_text(&text);
        }
        EditorAction::ReplaceDocument => {
            let text = command.text.ok_or("text is required")?;
            let Some(PaneRuntime::Editor(editor)) = app.panes.get_mut(&pane_id) else {
                return Err("Target is not an editor pane".to_owned());
            };
            editor.replace_contents(&text);
        }
        EditorAction::JumpTo => {
            let line = command.line.ok_or("line is required")?;
            let column = command.column.unwrap_or(1);
            let Some(PaneRuntime::Editor(editor)) = app.panes.get_mut(&pane_id) else {
                return Err("Target is not an editor pane".to_owned());
            };
            editor.jump_to_location(line.saturating_sub(1), column.saturating_sub(1));
        }
        EditorAction::ToggleRendered => app.action_toggle_editor_view_mode(),
        EditorAction::ToggleLineNumbers => app.action_toggle_editor_line_numbers(),
        EditorAction::ToggleMinimap => app.action_toggle_editor_minimap(),
        EditorAction::ToggleAutosave => app.action_toggle_editor_autosave(),
    }
    if app
        .panes
        .get(&pane_id)
        .is_some_and(|runtime| matches!(runtime, PaneRuntime::Editor(editor) if editor.content_revision() != content_revision_before))
    {
        app.record_client_node_activity(pane_id);
    }
    Ok(ExecutionReceipt::immediate(
        app.status_message
            .clone()
            .unwrap_or_else(|| "Editor updated".to_owned()),
    ))
}

fn execute_board(app: &mut App, command: BoardCommand) -> Result<ExecutionReceipt, String> {
    let pane_id = resolve_node(app, &command.target)?;
    let Some(PaneRuntime::Board(board)) = app.panes.get_mut(&pane_id) else {
        return Err("Target is not a board pane".to_owned());
    };
    let content_revision_before = board.content_revision();
    match command.action {
        BoardAction::SelectColumn => {
            board.select_column(command.column.ok_or("column is required")?)
        }
        BoardAction::SelectCard => board.select_card(
            command.column.ok_or("column is required")?,
            command.card.ok_or("card is required")?,
        ),
        BoardAction::OpenCard => board.open_card_details(
            command.column.ok_or("column is required")?,
            command.card.ok_or("card is required")?,
        ),
        BoardAction::CloseCard => board.close_card_details(),
        BoardAction::AddColumn => board.add_column(required_nonempty(command.title, "title")?)?,
        BoardAction::AddCard => {
            if let Some(column) = command.column {
                board.select_column(column);
            }
            board.add_card(required_nonempty(command.title, "title")?)?;
        }
        BoardAction::UpdateCard => board.update_card(
            command.column.ok_or("column is required")?,
            command.card.ok_or("card is required")?,
            command.title,
            command.body,
        )?,
        BoardAction::RenameColumn => {
            board.select_column(command.column.ok_or("column is required")?);
            board.rename_selected_column(required_nonempty(command.title, "title")?)?;
        }
        BoardAction::MoveCard => board.move_card(
            command.column.ok_or("column is required")?,
            command.card.ok_or("card is required")?,
            command
                .destination_column
                .ok_or("destination_column is required")?,
            command.destination_card.unwrap_or(usize::MAX),
        )?,
        BoardAction::ToggleCheckbox => board.toggle_card_checkbox(
            command.column.ok_or("column is required")?,
            command.card.ok_or("card is required")?,
            command.checkbox.ok_or("checkbox is required")?,
        )?,
        BoardAction::DeleteCard => {
            board.select_card(
                command.column.ok_or("column is required")?,
                command.card.ok_or("card is required")?,
            );
            board.delete_selected_card()?;
        }
        BoardAction::DeleteColumn => {
            board.select_column(command.column.ok_or("column is required")?);
            board.delete_selected_column()?;
        }
    }
    let did_change_content = board.content_revision() != content_revision_before;
    if did_change_content {
        app.record_client_node_activity(pane_id);
    }
    Ok(ExecutionReceipt::immediate("Board updated and persisted"))
}

fn execute_session(app: &mut App, command: SessionCommand) -> Result<ExecutionReceipt, String> {
    match command.action {
        SessionAction::Detach => app.request_client_exit(ClientExitReason::Quit),
        SessionAction::RestartClient => app.request_client_exit(ClientExitReason::RestartRequested),
        SessionAction::RestartServer => app.queue_request(ClientRequest::RestartServer),
        SessionAction::KillSession => app.request_session_kill(),
    }
    Ok(ExecutionReceipt::queued("Session lifecycle request queued"))
}

/// Disables voice through the same persisted App boundary as F8/settings, but
/// marks this receipt so the provider result is flushed before actor teardown.
fn execute_stop_voice_mode(
    app: &mut App,
    _command: StopVoiceModeCommand,
) -> Result<ExecutionReceipt, String> {
    app.stop_voice_control();
    Ok(ExecutionReceipt::terminating("Voice mode stopped"))
}

fn require_runtime_kind(app: &App, pane_id: ilium_core::NodeId, label: &str) -> Result<(), String> {
    app.panes
        .contains_key(&pane_id)
        .then_some(())
        .ok_or_else(|| format!("Node {} is not a live {label}", pane_id.0))
}

fn require_terminal(app: &App, pane_id: ilium_core::NodeId) -> Result<(), String> {
    matches!(app.panes.get(&pane_id), Some(PaneRuntime::Terminal(_)))
        .then_some(())
        .ok_or_else(|| format!("Node {} is not a terminal pane", pane_id.0))
}

fn require_editor(app: &App, pane_id: ilium_core::NodeId) -> Result<(), String> {
    matches!(app.panes.get(&pane_id), Some(PaneRuntime::Editor(_)))
        .then_some(())
        .ok_or_else(|| format!("Node {} is not an editor pane", pane_id.0))
}

fn resolve_filesystem_path(app: &App, path: String) -> PathBuf {
    let path = PathBuf::from(path);
    if path.is_absolute() {
        path
    } else {
        app.session_cwd.join(path)
    }
}

fn required_nonempty(value: Option<String>, field: &str) -> Result<String, String> {
    let value = value.ok_or_else(|| format!("{field} is required"))?;
    if value.trim().is_empty() {
        return Err(format!("{field} must not be empty"));
    }
    Ok(value)
}

fn terminal_key_bytes(key: TerminalKey) -> &'static [u8] {
    match key {
        TerminalKey::Enter => b"\r",
        TerminalKey::Escape => b"\x1b",
        TerminalKey::Tab => b"\t",
        TerminalKey::Backtab => b"\x1b[Z",
        TerminalKey::Up => b"\x1b[A",
        TerminalKey::Down => b"\x1b[B",
        TerminalKey::Right => b"\x1b[C",
        TerminalKey::Left => b"\x1b[D",
        TerminalKey::Home => b"\x1b[H",
        TerminalKey::End => b"\x1b[F",
        TerminalKey::PageUp => b"\x1b[5~",
        TerminalKey::PageDown => b"\x1b[6~",
        TerminalKey::Backspace => b"\x7f",
        TerminalKey::Delete => b"\x1b[3~",
        TerminalKey::Space => b" ",
        TerminalKey::ControlC => b"\x03",
        TerminalKey::ControlD => b"\x04",
        TerminalKey::ControlL => b"\x0c",
        TerminalKey::ControlZ => b"\x1a",
    }
}

fn settings_tab(label: &str) -> Result<SettingsTab, String> {
    SettingsTab::ALL
        .into_iter()
        .find(|tab| {
            tab.label().eq_ignore_ascii_case(label)
                || tab.label().replace(' ', "_").eq_ignore_ascii_case(label)
        })
        .ok_or_else(|| format!("Unknown settings tab {label:?}"))
}

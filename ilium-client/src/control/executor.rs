//! Synchronous command execution against `App`'s existing semantic methods.

use std::path::PathBuf;

use ilium_core::{
    AgentProvider, BuiltinAgentProvider, PromptQueueDelivery, SplitOrientation, TreeMoveDirection,
    MAXIMUM_SPLIT_VIEW_PANES,
};
use ilium_ipc::{ClientRequest, PromptSubmissionSource, WorkspaceCreateSpec};
use serde::Serialize;
use serde_json::{json, Value};

use crate::app::{
    App, BoardStorageKind, ClientExitReason, CreateBoardState, Mode, PaneRuntime, SettingsState,
    SettingsTab,
};
use crate::board::BoardPane;
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
            data: json!({ "verification": ilium_prompts::voice::VOICE_EXECUTOR_READ_ILIUM_GET_STATE_AFTER_THE_SERVER }),
            terminate_session_after_delivery: false,
        }
    }

    pub(super) fn local_write_pending(domain: &str, target: Value, revision: u64) -> Self {
        Self {
            status: "queued",
            message: format!("{domain} write accepted; durable completion is pending"),
            data: json!({"write_domain":domain,"target":target,"source_revision":revision,"durability":"pending"}),
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
                message: ilium_prompts::voice::VOICE_EXECUTOR_CURRENT_ILIUM_STATE.to_owned(),
                data: snapshot.prepare()?,
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

/// Queues voice-originated text for the detached server's semantic Enter path.
fn execute_terminal_submission(
    app: &mut App,
    command: TerminalSubmissionCommand,
) -> Result<ExecutionReceipt, String> {
    let pane_id = resolve_node(app, &command.target)?;
    require_terminal(app, pane_id)?;
    let text = required_nonempty(Some(command.text), "text")?;
    app.send_terminal_submission(pane_id, text, PromptSubmissionSource::VoiceControl)
        .map_err(|_original_request| {
            "Terminal submission rejected before admission; retry the original voice command"
                .to_string()
        })?;
    Ok(ExecutionReceipt::queued(
        ilium_prompts::voice::VOICE_EXECUTOR_QUEUED_TEXT_AND_ENTER_FOR_THE_TERMINAL,
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
    app.send_user_terminal_bytes(pane_id, bytes, None)
        .map_err(|_original_request| {
            "Terminal typing rejected before admission; retry the original voice command"
                .to_string()
        })?;
    Ok(ExecutionReceipt::queued(
        ilium_prompts::voice::VOICE_EXECUTOR_STAGED_TEXT_IN_THE_TERMINAL,
    ))
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
            let query = requested_query.unwrap_or_default();
            state.query = TextPromptState::new(&query);
            state.note_query_changed(std::time::Instant::now());
            if let Some(index) = command.index {
                state.queue_control(crate::search_ui::SearchContinuationAction::Select(index))?;
            }
            let revision = state.revision();
            app.mode = Mode::Search(state);
            return Ok(ExecutionReceipt {
                status: "queued",
                message: "Workspace search accepted; results and optional selection are pending preparation".into(),
                data: json!({ "query": query, "revision": revision, "preparation": "pending", "index": command.index, "verification": "Read ilium_get_state after search preparation completes" }),
                terminate_session_after_delivery: false,
            });
        }
        SearchAction::SelectNext | SearchAction::SelectPrevious => {
            let delta = if matches!(command.action, SearchAction::SelectNext) {
                1
            } else {
                -1
            };
            if state.is_preparing() {
                let queued =
                    state.queue_control(crate::search_ui::SearchContinuationAction::Move(delta));
                let revision = state.revision();
                app.mode = Mode::Search(state);
                queued?;
                return Ok(ExecutionReceipt {
                    status: "queued",
                    message: "Search navigation is pending preparation of the current query".into(),
                    data: json!({ "revision": revision, "preparation": "pending" }),
                    terminate_session_after_delivery: false,
                });
            }
            state.move_selection(delta, usize::MAX);
        }
        SearchAction::OpenResult => {
            if state.is_preparing() {
                let queued = state.queue_control(crate::search_ui::SearchContinuationAction::Open(
                    command.index,
                ));
                let revision = state.revision();
                app.mode = Mode::Search(state);
                queued?;
                return Ok(ExecutionReceipt {
                    status: "queued",
                    message:
                        "Opening the search result is pending preparation of the current query"
                            .into(),
                    data: json!({ "revision": revision, "preparation": "pending", "index": command.index }),
                    terminate_session_after_delivery: false,
                });
            }
            if let Some(index) = command.index {
                if index >= state.results.len() {
                    app.mode = Mode::Search(state);
                    return Err(ilium_prompts::render_value(
                        "voice/executor/search-result-index-v0-does-not-exist",
                        &serde_json::json!({"v0": (index).to_string()}),
                    ));
                }
                state.selected_index = index;
            }
            let Some(result) = state.selected_result().cloned() else {
                app.mode = Mode::Search(state);
                return Err(
                    ilium_prompts::voice::VOICE_EXECUTOR_THERE_IS_NO_SELECTED_SEARCH_RESULT
                        .to_owned(),
                );
            };
            app.mode = Mode::Search(state);
            app.activate_search_result(result);
            return Ok(ExecutionReceipt::immediate(
                ilium_prompts::voice::VOICE_EXECUTOR_OPENED_THE_SELECTED_SEARCH_RESULT,
            ));
        }
        SearchAction::Close => {
            app.mode = Mode::Normal;
            return Ok(ExecutionReceipt::immediate(
                ilium_prompts::voice::VOICE_EXECUTOR_CLOSED_WORKSPACE_SEARCH,
            ));
        }
    }
    let revision = state.revision();
    let selected_index = state.selected_index;
    let result_count = state.results.len();
    app.mode = Mode::Search(state);
    Ok(ExecutionReceipt {
        status: "ok",
        message: "Workspace search selection updated; read ilium_get_state for completed results"
            .into(),
        data: json!({ "revision": revision, "selected_index": selected_index, "result_count": result_count }),
        terminate_session_after_delivery: false,
    })
}

fn execute_ui(app: &mut App, command: UiCommand) -> Result<ExecutionReceipt, String> {
    match command.action {
        UiAction::FocusTree => {
            app.leave_pane_focus();
            Ok(ExecutionReceipt::immediate(
                ilium_prompts::voice::VOICE_EXECUTOR_FOCUSED_THE_LEFT_TREE_PANEL,
            ))
        }
        UiAction::FocusPane => {
            let pane_id = resolve_node(app, &command.target)?;
            require_runtime_kind(app, pane_id, "pane")?;
            app.focus_pane(pane_id);
            Ok(ExecutionReceipt::immediate(ilium_prompts::render_value(
                "voice/executor/focused-pane",
                &serde_json::json!({"v0": (pane_id.0).to_string()}),
            )))
        }
        UiAction::FocusNextPane => {
            app.focus_visible_pane(1);
            Ok(ExecutionReceipt::immediate(
                ilium_prompts::voice::VOICE_EXECUTOR_FOCUSED_THE_NEXT_VISIBLE_PANE,
            ))
        }
        UiAction::FocusPreviousPane => {
            app.focus_visible_pane(-1);
            Ok(ExecutionReceipt::immediate(
                ilium_prompts::voice::VOICE_EXECUTOR_FOCUSED_THE_PREVIOUS_VISIBLE_PANE,
            ))
        }
        UiAction::FocusPaneDirection => {
            let direction = command
                .direction
                .ok_or(ilium_prompts::voice::VOICE_EXECUTOR_DIRECTION_IS_REQUIRED)?;
            let (horizontal, vertical) = match direction {
                Direction::Left => (-1, 0),
                Direction::Right => (1, 0),
                Direction::Up => (0, -1),
                Direction::Down => (0, 1),
            };
            app.focus_visible_pane_in_direction(horizontal, vertical);
            Ok(ExecutionReceipt::immediate(
                ilium_prompts::voice::VOICE_EXECUTOR_MOVED_FOCUS_WITHIN_THE_VISIBLE_SPLIT,
            ))
        }
        UiAction::ShowSplit => {
            let split_id = resolve_node(app, &command.target)?;
            if !app
                .tree
                .get(split_id)
                .is_some_and(|node| node.is_split_view())
            {
                return Err(ilium_prompts::render_value(
                    "voice/executor/node-v0-is-not-a-split-view",
                    &serde_json::json!({"v0": (split_id.0).to_string()}),
                ));
            }
            app.show_split_view(split_id);
            Ok(ExecutionReceipt::immediate(ilium_prompts::render_value(
                "voice/executor/showing-split",
                &serde_json::json!({"v0": (split_id.0).to_string()}),
            )))
        }
        UiAction::OpenSettings => {
            app.action_open_settings();
            Ok(ExecutionReceipt::immediate(
                ilium_prompts::voice::VOICE_EXECUTOR_OPENED_SETTINGS,
            ))
        }
        UiAction::ShowSettingsTab => {
            let tab = settings_tab(
                command
                    .tab
                    .as_deref()
                    .ok_or(ilium_prompts::voice::VOICE_EXECUTOR_TAB_IS_REQUIRED)?,
            )?;
            let mut state = match std::mem::replace(&mut app.mode, Mode::Normal) {
                Mode::Settings(state) => state,
                _ => SettingsState::new(),
            };
            state.tab = tab;
            state.selected_row = 0;
            state.scroll = 0;
            app.mode = Mode::Settings(state);
            Ok(ExecutionReceipt::immediate(ilium_prompts::render_value(
                "voice/executor/opened-v0-settings",
                &serde_json::json!({"v0": (tab.label()).to_string()}),
            )))
        }
        UiAction::OpenSearch => {
            app.action_open_search();
            Ok(ExecutionReceipt::immediate(
                ilium_prompts::voice::VOICE_EXECUTOR_OPENED_WORKSPACE_SEARCH,
            ))
        }
        UiAction::OpenHelp => {
            app.mode = Mode::Help;
            Ok(ExecutionReceipt::immediate(
                ilium_prompts::voice::VOICE_EXECUTOR_OPENED_HELP,
            ))
        }
        UiAction::CloseOverlay => {
            app.mode = Mode::Normal;
            Ok(ExecutionReceipt::immediate(
                ilium_prompts::voice::VOICE_EXECUTOR_CLOSED_THE_CURRENT_OVERLAY,
            ))
        }
    }
}

fn execute_tree(app: &mut App, command: TreeCommand) -> Result<ExecutionReceipt, String> {
    match command.action {
        TreeAction::CreateTerminal => {
            let parent = resolve_parent(app, &command.parent)?;
            let admitted = app.request_new_terminal(parent);
            require_request_admission(app, admitted)?;
            Ok(ExecutionReceipt::queued(
                ilium_prompts::voice::VOICE_EXECUTOR_CREATING_A_TERMINAL_PANE,
            ))
        }
        TreeAction::CreateAgent => {
            let parent = resolve_parent(app, &command.parent)?;
            let provider = match command
                .provider
                .ok_or(ilium_prompts::voice::VOICE_EXECUTOR_PROVIDER_IS_REQUIRED)?
            {
                AgentProviderChoice::Claude => BuiltinAgentProvider::Claude,
                AgentProviderChoice::Codex => BuiltinAgentProvider::Codex,
                AgentProviderChoice::Antigravity => BuiltinAgentProvider::Antigravity,
            };
            if let Some(workspace) = command.workspace {
                ilium_core::validate_branch_name(&workspace.branch).map_err(|error| {
                    ilium_prompts::render_value(
                        "voice/executor/invalid-workspace-branch",
                        &serde_json::json!({"v0": (error).to_string()}),
                    )
                })?;
                if workspace
                    .base
                    .as_ref()
                    .is_some_and(|base| base.trim().is_empty())
                {
                    return Err(
                        ilium_prompts::voice::VOICE_EXECUTOR_WORKSPACE_BASE_MUST_NOT_BE_EMPTY
                            .into(),
                    );
                }
                app.queue_control_workspace_create(
                    parent,
                    provider,
                    WorkspaceCreateSpec::NewAtDefaultPath {
                        branch: workspace.branch,
                        base_ref: workspace.base,
                    },
                    command.initial_input,
                )?;
                return Ok(ExecutionReceipt::queued(ilium_prompts::render_value(
                    "voice/executor/creating-a-v0-agent-in-a-git",
                    &serde_json::json!({"v0": (provider.label()).to_string()}),
                )));
            }
            if let Some(initial_input) = command.initial_input {
                let admitted = app.request_new_command_pane_with_input(
                    parent,
                    provider.command_line().to_owned(),
                    initial_input,
                );
                require_request_admission(app, admitted)?;
            } else {
                let admitted =
                    app.request_new_command_pane(parent, provider.command_line().to_owned());
                require_request_admission(app, admitted)?;
            }
            Ok(ExecutionReceipt::queued(ilium_prompts::render_value(
                "voice/executor/creating-a-v0-agent-pane",
                &serde_json::json!({"v0": (provider.label()).to_string()}),
            )))
        }
        TreeAction::CreateCommandPane => {
            let parent = resolve_parent(app, &command.parent)?;
            let command_line = required_nonempty(command.command_line, "command_line")?;
            if let Some(initial_input) = command.initial_input {
                let admitted =
                    app.request_new_command_pane_with_input(parent, command_line, initial_input);
                require_request_admission(app, admitted)?;
            } else {
                let admitted = app.request_new_command_pane(parent, command_line);
                require_request_admission(app, admitted)?;
            }
            Ok(ExecutionReceipt::queued(
                ilium_prompts::voice::VOICE_EXECUTOR_CREATING_THE_COMMAND_PANE,
            ))
        }
        TreeAction::OpenEditor => {
            let parent = resolve_parent(app, &command.parent)?;
            let path = resolve_filesystem_path(app, required_nonempty(command.path, "path")?);
            let admitted = app.request_new_editor(parent, path);
            require_request_admission(app, admitted)?;
            Ok(ExecutionReceipt::queued(
                ilium_prompts::voice::VOICE_EXECUTOR_OPENING_THE_FILE_IN_AN_EDITOR_PANE,
            ))
        }
        TreeAction::AddFolder => {
            let parent = resolve_parent(app, &command.parent)?;
            let path = resolve_filesystem_path(app, required_nonempty(command.path, "path")?);
            let admitted = app.request_new_folder(parent, path);
            require_request_admission(app, admitted)?;
            Ok(ExecutionReceipt::queued(
                ilium_prompts::voice::VOICE_EXECUTOR_ADDING_THE_FOLDER_TO_THE_LEFT_PANEL,
            ))
        }
        TreeAction::AddProject => {
            let path = resolve_filesystem_path(app, required_nonempty(command.path, "path")?);
            let admitted = app.request_new_project(path);
            require_request_admission(app, admitted)?;
            Ok(ExecutionReceipt::queued(
                ilium_prompts::voice::VOICE_EXECUTOR_ADDING_THE_PROJECT,
            ))
        }
        TreeAction::ChangeProjectFolder => {
            let project_id = resolve_node(app, &command.target)?;
            let path = resolve_filesystem_path(app, required_nonempty(command.path, "path")?);
            let admitted = app.request_change_project_folder(project_id, path);
            require_request_admission(app, admitted)?;
            Ok(ExecutionReceipt::queued(
                ilium_prompts::voice::VOICE_EXECUTOR_CHANGING_THE_PROJECT_S_FOLDER,
            ))
        }
        TreeAction::CreateGroup => {
            let parent = resolve_parent(app, &command.parent)?;
            let name = required_nonempty(command.name, "name")?;
            let admitted = app.request_new_group(parent, name);
            require_request_admission(app, admitted)?;
            Ok(ExecutionReceipt::queued(
                ilium_prompts::voice::VOICE_EXECUTOR_CREATING_THE_GROUP,
            ))
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
                // `commit_create_board` re-opens the interactive dialog on
                // failure for the keyboard flow. The voice flow reports the
                // failure through this receipt instead, so close the dialog
                // rather than leaving a modal the user never opened -- one a
                // corrected voice retry would clobber mid-edit anyway.
                app.mode = Mode::Normal;
                return Err(app.status_message.clone().unwrap_or_else(|| {
                    ilium_prompts::voice::VOICE_EXECUTOR_BOARD_CREATION_FAILED.to_owned()
                }));
            }
            Ok(ExecutionReceipt::queued(
                ilium_prompts::voice::VOICE_EXECUTOR_CREATING_THE_BOARD,
            ))
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
                return Err(ilium_prompts::render_value(
                    "voice/executor/a-split-view-can-contain-at-most",
                    &serde_json::json!({"v0": (MAXIMUM_SPLIT_VIEW_PANES).to_string()}),
                ));
            }
            let admitted = app.queue_request(ClientRequest::CreateSplitView {
                parent_group,
                name,
                orientation,
                pane_ids,
            });
            require_request_admission(app, admitted)?;
            Ok(ExecutionReceipt::queued(
                ilium_prompts::voice::VOICE_EXECUTOR_CREATING_THE_SPLIT_VIEW,
            ))
        }
        TreeAction::Rename => {
            let node_id = resolve_node(app, &command.target)?;
            let name = required_nonempty(command.name, "name")?;
            let admitted = app.request_rename(node_id, name, None, None);
            require_request_admission(app, admitted)?;
            Ok(ExecutionReceipt::queued(
                ilium_prompts::voice::VOICE_EXECUTOR_RENAMING_THE_TREE_ITEM,
            ))
        }
        TreeAction::MoveUp | TreeAction::MoveDown => {
            let node_id = resolve_node(app, &command.target)?;
            let direction = if matches!(command.action, TreeAction::MoveUp) {
                TreeMoveDirection::Up
            } else {
                TreeMoveDirection::Down
            };
            let admitted = app.request_move(node_id, direction);
            require_request_admission(app, admitted)?;
            Ok(ExecutionReceipt::queued(
                ilium_prompts::voice::VOICE_EXECUTOR_MOVING_THE_TREE_ITEM,
            ))
        }
        TreeAction::Reparent => {
            let node_id = resolve_node(app, &command.target)?;
            let new_parent = resolve_parent(app, &command.parent)?;
            let admitted = app.request_reparent(node_id, new_parent, command.index);
            require_request_admission(app, admitted)?;
            Ok(ExecutionReceipt::queued(
                ilium_prompts::voice::VOICE_EXECUTOR_REPARENTING_THE_TREE_ITEM,
            ))
        }
        TreeAction::Close => {
            let node_id = resolve_node(app, &command.target)?;
            let admitted = app.request_close(node_id);
            require_request_admission(app, admitted)?;
            Ok(ExecutionReceipt::queued(
                ilium_prompts::voice::VOICE_EXECUTOR_CLOSING_THE_TREE_ITEM,
            ))
        }
        TreeAction::ToggleExpanded => {
            let node_id = resolve_node(app, &command.target)?;
            if !app
                .tree
                .get(node_id)
                .is_some_and(|node| node.is_container())
            {
                return Err(ilium_prompts::render_value(
                    "voice/executor/node-v0-cannot-be-expanded",
                    &serde_json::json!({"v0": (node_id.0).to_string()}),
                ));
            }
            app.select_node(node_id);
            app.toggle_selected_tree_node();
            Ok(ExecutionReceipt::immediate(
                ilium_prompts::voice::VOICE_EXECUTOR_TOGGLED_THE_LEFT_PANEL_GROUP,
            ))
        }
        TreeAction::Retitle => {
            let node_id = resolve_node(app, &command.target)?;
            app.action_request_retitle(node_id);
            Ok(ExecutionReceipt::queued(
                app.status_message.clone().unwrap_or_else(|| {
                    ilium_prompts::voice::VOICE_EXECUTOR_STARTED_AUTOMATIC_RETITLING.to_owned()
                }),
            ))
        }
        TreeAction::RestructureAllProjects => {
            app.action_request_restructure();
            Ok(ExecutionReceipt::queued(
                app.restructure_status_text().unwrap_or_else(|| {
                    ilium_prompts::voice::VOICE_EXECUTOR_REQUESTED_PROJECT_RESTRUCTURING.to_owned()
                }),
            ))
        }
        TreeAction::RestructureProject => {
            let project_id = resolve_node(app, &command.target)?;
            app.action_request_project_restructure(project_id);
            Ok(ExecutionReceipt::queued(
                app.restructure_status_text().unwrap_or_else(|| {
                    ilium_prompts::voice::VOICE_EXECUTOR_REQUESTED_PROJECT_RESTRUCTURING.to_owned()
                }),
            ))
        }
        TreeAction::RevertProjectRestructure => {
            let project_id = resolve_node(app, &command.target)?;
            let admitted = app.request_revert_project_restructure(project_id);
            require_request_admission(app, admitted)?;
            Ok(ExecutionReceipt::queued(
                ilium_prompts::voice::VOICE_EXECUTOR_REVERTING_THE_PROJECT_S_LATEST_RESTRUCTURE,
            ))
        }
    }
}

fn require_request_admission(app: &App, admitted: bool) -> Result<(), String> {
    if admitted {
        Ok(())
    } else {
        Err(app.status_message.clone().unwrap_or_else(|| {
            "Request admission refused; retry after pending work completes".into()
        }))
    }
}

fn execute_terminal(app: &mut App, command: TerminalCommand) -> Result<ExecutionReceipt, String> {
    let pane_id = resolve_node(app, &command.target)?;
    require_terminal(app, pane_id)?;
    match command.action {
        TerminalAction::PressKey => {
            let key = command
                .key
                .ok_or(ilium_prompts::voice::VOICE_EXECUTOR_KEY_IS_REQUIRED)?;
            let admitted = app.queue_request(ClientRequest::UserKeyInput {
                pane_id,
                bytes: terminal_key_bytes(key).to_vec(),
                submission: matches!(key, TerminalKey::Enter)
                    .then_some(PromptSubmissionSource::VoiceControl),
                prompt_epoch: None,
            });
            require_request_admission(app, admitted)?;
            Ok(ExecutionReceipt::queued(
                ilium_prompts::voice::VOICE_EXECUTOR_SENT_THE_KEY_TO_THE_TERMINAL,
            ))
        }
        TerminalAction::ScrollUp | TerminalAction::ScrollDown => {
            let lines = command.lines.unwrap_or(app.last_known_pane_size.0.max(1));
            let Some(PaneRuntime::Terminal(view)) = app.panes.get_mut(&pane_id) else {
                return Err(
                    ilium_prompts::voice::VOICE_EXECUTOR_TARGET_IS_NOT_A_TERMINAL_PANE.to_owned(),
                );
            };
            if matches!(command.action, TerminalAction::ScrollUp) {
                view.scroll_up(lines);
            } else {
                view.scroll_down(lines);
            }
            Ok(ExecutionReceipt::immediate(
                ilium_prompts::voice::VOICE_EXECUTOR_SCROLLED_THE_TERMINAL,
            ))
        }
        TerminalAction::ScrollToBottom => {
            if let Some(PaneRuntime::Terminal(view)) = app.panes.get_mut(&pane_id) {
                view.scroll_to_bottom();
            }
            Ok(ExecutionReceipt::immediate(
                ilium_prompts::voice::VOICE_EXECUTOR_RETURNED_TO_LIVE_TERMINAL_OUTPUT,
            ))
        }
        TerminalAction::ScheduleInput => {
            let text = command.text.unwrap_or_default();
            let admitted = app.queue_request(ClientRequest::SchedulePaneInput {
                pane_id,
                delay_seconds: command.delay_seconds.unwrap_or(0),
                text,
                send_enter: true,
            });
            require_request_admission(app, admitted)?;
            Ok(ExecutionReceipt::queued(
                ilium_prompts::voice::VOICE_EXECUTOR_SCHEDULED_TERMINAL_INPUT,
            ))
        }
        TerminalAction::QueuePrompt => {
            let text = required_nonempty(command.text, "text")?;
            let delivery = match command.delivery.unwrap_or(PromptDeliveryChoice::Once) {
                PromptDeliveryChoice::Once => PromptQueueDelivery::Once,
                PromptDeliveryChoice::Times => PromptQueueDelivery::Times {
                    remaining_runs: command.runs.filter(|runs| *runs > 0).ok_or(
                        ilium_prompts::voice::VOICE_POLICY_RUNS_IS_REQUIRED_AND_MUST_BE_POSITIVE,
                    )?,
                },
                PromptDeliveryChoice::Forever => PromptQueueDelivery::Forever,
            };
            let admitted = app.queue_request(ClientRequest::EnqueuePrompt {
                pane_id,
                text,
                delivery,
            });
            require_request_admission(app, admitted)?;
            Ok(ExecutionReceipt::queued(
                ilium_prompts::voice::VOICE_EXECUTOR_QUEUED_THE_PROMPT_FOR_THE_AGENT_S,
            ))
        }
        TerminalAction::ClearPromptQueue => {
            let admitted = app.queue_request(ClientRequest::ClearPromptQueue { pane_id });
            require_request_admission(app, admitted)?;
            Ok(ExecutionReceipt::queued(
                ilium_prompts::voice::VOICE_EXECUTOR_CLEARING_THE_PANE_S_PROMPT_QUEUE,
            ))
        }
    }
}

fn execute_editor(app: &mut App, command: EditorCommand) -> Result<ExecutionReceipt, String> {
    let pane_id = resolve_node(app, &command.target)?;
    require_editor(app, pane_id)?;
    app.focus_pane(pane_id);
    let content_revision_before = match app.panes.get(&pane_id) {
        Some(PaneRuntime::Editor(editor)) => editor.content_revision(),
        _ => {
            return Err(
                ilium_prompts::voice::VOICE_EXECUTOR_TARGET_IS_NOT_AN_EDITOR_PANE.to_owned(),
            )
        }
    };
    let message = match command.action {
        EditorAction::Save => {
            let path = match app.panes.get(&pane_id) {
                Some(PaneRuntime::Editor(editor)) => editor.path.clone(),
                _ => None,
            }
            .ok_or_else(|| {
                ilium_prompts::voice::VOICE_EXECUTOR_THIS_EDITOR_HAS_NO_FILE_PATH_YET.to_owned()
            })?;
            app.enqueue_editor_save(
                pane_id,
                path.clone(),
                crate::filesystem::editors::SavePurpose::Explicit,
            )?;
            return Ok(ExecutionReceipt::local_write_pending(
                "Editor",
                json!({"pane_id":pane_id.0,"path":path}),
                content_revision_before,
            ));
        }
        EditorAction::SaveAs => {
            let path = PathBuf::from(required_nonempty(command.path, "path")?);
            let path = if path.is_absolute() {
                path
            } else {
                app.session_cwd.join(path)
            };
            app.enqueue_editor_save(
                pane_id,
                path.clone(),
                crate::filesystem::editors::SavePurpose::SaveAs,
            )?;
            return Ok(ExecutionReceipt::local_write_pending(
                "Editor",
                json!({"pane_id":pane_id.0,"path":path}),
                content_revision_before,
            ));
        }
        EditorAction::InsertText => {
            let text = command
                .text
                .ok_or(ilium_prompts::voice::VOICE_EXECUTOR_TEXT_IS_REQUIRED)?;
            let Some(PaneRuntime::Editor(editor)) = app.panes.get_mut(&pane_id) else {
                return Err(
                    ilium_prompts::voice::VOICE_EXECUTOR_TARGET_IS_NOT_AN_EDITOR_PANE.to_owned(),
                );
            };
            editor.insert_text(&text);
            // `mark_dirty` drops the pane's cached Rendered-mode document
            // (it can go stale from a non-keyboard edit like this one) --
            // rebuild it immediately rather than leaving a pane the user is
            // actively looking at on the "Rendering…" placeholder until the
            // next resize or view-mode toggle.
            app.rebuild_rendered_markdown(pane_id);
            ilium_prompts::voice::VOICE_EXECUTOR_INSERTED_TEXT_INTO_THE_EDITOR.to_owned()
        }
        EditorAction::ReplaceDocument => {
            let text = command
                .text
                .ok_or(ilium_prompts::voice::VOICE_EXECUTOR_TEXT_IS_REQUIRED)?;
            let Some(PaneRuntime::Editor(editor)) = app.panes.get_mut(&pane_id) else {
                return Err(
                    ilium_prompts::voice::VOICE_EXECUTOR_TARGET_IS_NOT_AN_EDITOR_PANE.to_owned(),
                );
            };
            editor.replace_contents(&text);
            app.rebuild_rendered_markdown(pane_id);
            ilium_prompts::voice::VOICE_EXECUTOR_REPLACED_THE_EDITOR_S_DOCUMENT.to_owned()
        }
        EditorAction::JumpTo => {
            let line = command
                .line
                .ok_or(ilium_prompts::voice::VOICE_EXECUTOR_LINE_IS_REQUIRED)?;
            let column = command.column.unwrap_or(1);
            let Some(PaneRuntime::Editor(editor)) = app.panes.get_mut(&pane_id) else {
                return Err(
                    ilium_prompts::voice::VOICE_EXECUTOR_TARGET_IS_NOT_AN_EDITOR_PANE.to_owned(),
                );
            };
            editor.jump_to_location(line.saturating_sub(1), column.saturating_sub(1));
            ilium_prompts::voice::VOICE_EXECUTOR_MOVED_THE_EDITOR_CURSOR.to_owned()
        }
        EditorAction::ToggleRendered => {
            app.action_toggle_editor_view_mode();
            ilium_prompts::voice::VOICE_EXECUTOR_TOGGLED_THE_RENDERED_SOURCE_VIEW.to_owned()
        }
        EditorAction::ToggleLineNumbers => {
            app.action_toggle_editor_line_numbers();
            ilium_prompts::voice::VOICE_EXECUTOR_TOGGLED_LINE_NUMBERS.to_owned()
        }
        EditorAction::ToggleMinimap => {
            app.action_toggle_editor_minimap();
            ilium_prompts::voice::VOICE_EXECUTOR_TOGGLED_THE_MINIMAP.to_owned()
        }
        EditorAction::ToggleAutosave => {
            app.action_toggle_editor_autosave();
            ilium_prompts::voice::VOICE_EXECUTOR_TOGGLED_AUTOSAVE.to_owned()
        }
    };
    if app
        .panes
        .get(&pane_id)
        .is_some_and(|runtime| matches!(runtime, PaneRuntime::Editor(editor) if editor.content_revision() != content_revision_before))
    {
        app.record_client_node_activity(pane_id);
    }
    Ok(ExecutionReceipt::immediate(message))
}

fn execute_board(app: &mut App, command: BoardCommand) -> Result<ExecutionReceipt, String> {
    let pane_id = resolve_node(app, &command.target)?;
    let Some(PaneRuntime::Board(board)) = app.panes.get_mut(&pane_id) else {
        return Err(ilium_prompts::voice::VOICE_POLICY_TARGET_IS_NOT_A_BOARD_PANE.to_owned());
    };
    let content_revision_before = board.content_revision();
    let intent_revision_before = board.intent_revision();
    match command.action {
        BoardAction::SelectColumn => select_column(
            board,
            command
                .column
                .ok_or(ilium_prompts::voice::VOICE_POLICY_COLUMN_IS_REQUIRED)?,
        )?,
        BoardAction::SelectCard => select_card(
            board,
            command
                .column
                .ok_or(ilium_prompts::voice::VOICE_POLICY_COLUMN_IS_REQUIRED)?,
            command
                .card
                .ok_or(ilium_prompts::voice::VOICE_POLICY_CARD_IS_REQUIRED)?,
        )?,
        BoardAction::OpenCard => {
            // `BoardPane::open_card_details` no-ops on out-of-range indices;
            // validate through `select_card` first so a bad index fails
            // loudly instead of reporting success while nothing opened.
            let column = command
                .column
                .ok_or(ilium_prompts::voice::VOICE_POLICY_COLUMN_IS_REQUIRED)?;
            let card = command
                .card
                .ok_or(ilium_prompts::voice::VOICE_POLICY_CARD_IS_REQUIRED)?;
            select_card(board, column, card)?;
            board.open_card_details(column, card);
        }
        BoardAction::CloseCard => board.close_card_details(),
        BoardAction::AddColumn => board.add_column(required_nonempty(command.title, "title")?)?,
        BoardAction::AddCard => {
            if let Some(column) = command.column {
                select_column(board, column)?;
            }
            board.add_card(required_nonempty(command.title, "title")?)?;
        }
        BoardAction::UpdateCard => board.update_card(
            command
                .column
                .ok_or(ilium_prompts::voice::VOICE_POLICY_COLUMN_IS_REQUIRED)?,
            command
                .card
                .ok_or(ilium_prompts::voice::VOICE_POLICY_CARD_IS_REQUIRED)?,
            command.title,
            command.body,
        )?,
        BoardAction::RenameColumn => {
            select_column(
                board,
                command
                    .column
                    .ok_or(ilium_prompts::voice::VOICE_POLICY_COLUMN_IS_REQUIRED)?,
            )?;
            board.rename_selected_column(required_nonempty(command.title, "title")?)?;
        }
        BoardAction::MoveCard => board.move_card(
            command
                .column
                .ok_or(ilium_prompts::voice::VOICE_POLICY_COLUMN_IS_REQUIRED)?,
            command
                .card
                .ok_or(ilium_prompts::voice::VOICE_POLICY_CARD_IS_REQUIRED)?,
            command
                .destination_column
                .ok_or(ilium_prompts::voice::VOICE_EXECUTOR_DESTINATION_COLUMN_IS_REQUIRED)?,
            command.destination_card.unwrap_or(usize::MAX),
        )?,
        BoardAction::ToggleCheckbox => board.toggle_card_checkbox(
            command
                .column
                .ok_or(ilium_prompts::voice::VOICE_POLICY_COLUMN_IS_REQUIRED)?,
            command
                .card
                .ok_or(ilium_prompts::voice::VOICE_POLICY_CARD_IS_REQUIRED)?,
            command
                .checkbox
                .ok_or(ilium_prompts::voice::VOICE_EXECUTOR_CHECKBOX_IS_REQUIRED)?,
        )?,
        BoardAction::DeleteCard => {
            select_card(
                board,
                command
                    .column
                    .ok_or(ilium_prompts::voice::VOICE_POLICY_COLUMN_IS_REQUIRED)?,
                command
                    .card
                    .ok_or(ilium_prompts::voice::VOICE_POLICY_CARD_IS_REQUIRED)?,
            )?;
            board.delete_selected_card()?;
        }
        BoardAction::DeleteColumn => {
            select_column(
                board,
                command
                    .column
                    .ok_or(ilium_prompts::voice::VOICE_POLICY_COLUMN_IS_REQUIRED)?,
            )?;
            board.delete_selected_column()?;
        }
    }
    if board.intent_revision() != intent_revision_before && board.pending_writes() != 0 {
        return Ok(ExecutionReceipt::local_write_pending(
            "Board",
            json!({"pane_id":pane_id.0}),
            board.intent_revision(),
        ));
    }
    let did_change_content = board.content_revision() != content_revision_before;
    if did_change_content {
        app.record_client_node_activity(pane_id);
    }
    Ok(ExecutionReceipt::immediate("Board command completed"))
}

fn execute_session(app: &mut App, command: SessionCommand) -> Result<ExecutionReceipt, String> {
    let admitted = match command.action {
        SessionAction::Detach => app.request_client_exit(ClientExitReason::Quit),
        SessionAction::RestartClient => app.request_client_exit(ClientExitReason::RestartRequested),
        SessionAction::RestartServer => app.queue_request(ClientRequest::RestartServer),
        SessionAction::KillSession => app.request_session_kill(),
    };
    require_request_admission(app, admitted)?;
    Ok(ExecutionReceipt::queued(
        ilium_prompts::voice::VOICE_EXECUTOR_SESSION_LIFECYCLE_REQUEST_QUEUED,
    ))
}

/// Disables voice through the same persisted App boundary as F8/settings, but
/// marks this receipt so the provider result is flushed before actor teardown.
fn execute_stop_voice_mode(
    app: &mut App,
    _command: StopVoiceModeCommand,
) -> Result<ExecutionReceipt, String> {
    app.stop_voice_control();
    Ok(ExecutionReceipt::terminating(
        ilium_prompts::voice::VOICE_EXECUTOR_VOICE_MODE_STOPPED,
    ))
}

fn require_runtime_kind(app: &App, pane_id: ilium_core::NodeId, label: &str) -> Result<(), String> {
    app.panes
        .contains_key(&pane_id)
        .then_some(())
        .ok_or_else(|| {
            ilium_prompts::render_value(
                "voice/executor/node-v0-is-not-a-live",
                &serde_json::json!({"v0": (pane_id.0).to_string(), "v1": (label).to_string()}),
            )
        })
}

fn require_terminal(app: &App, pane_id: ilium_core::NodeId) -> Result<(), String> {
    matches!(app.panes.get(&pane_id), Some(PaneRuntime::Terminal(_)))
        .then_some(())
        .ok_or_else(|| {
            ilium_prompts::render_value(
                "voice/executor/node-v0-is-not-a-terminal-pane",
                &serde_json::json!({"v0": (pane_id.0).to_string()}),
            )
        })
}

fn require_editor(app: &App, pane_id: ilium_core::NodeId) -> Result<(), String> {
    matches!(app.panes.get(&pane_id), Some(PaneRuntime::Editor(_)))
        .then_some(())
        .ok_or_else(|| {
            ilium_prompts::render_value(
                "voice/executor/node-v0-is-not-an-editor-pane",
                &serde_json::json!({"v0": (pane_id.0).to_string()}),
            )
        })
}

fn resolve_filesystem_path(app: &App, path: String) -> PathBuf {
    // Trim first -- otherwise a leading space survives into the path, and a
    // leading space before a leading `/` defeats `is_absolute()` entirely,
    // silently nesting an intended-absolute path under `session_cwd` (the
    // same hazard `App::action_save_as` documents and guards against).
    let path = PathBuf::from(path.trim());
    if path.is_absolute() {
        path
    } else {
        app.session_cwd.join(path)
    }
}

/// Selects `column_index`, failing loudly instead of silently keeping the
/// board's previous selection. `BoardPane::select_column` no-ops on an
/// out-of-range index, which would otherwise let a caller that trusts the
/// selection afterward (delete/rename/add) act on whichever column was
/// selected before this command instead of the one actually requested.
fn select_column(board: &mut BoardPane, column_index: usize) -> Result<(), String> {
    board.select_column(column_index);
    if board.selected_column == column_index {
        Ok(())
    } else {
        Err(ilium_prompts::render_value(
            "voice/policy/board-has-no-column",
            &serde_json::json!({"v0": (column_index).to_string()}),
        ))
    }
}

/// Selects `(column_index, card_index)`; see `select_column` for why this
/// must fail rather than leave a stale prior selection in place for a
/// subsequent destructive operation to act on.
fn select_card(
    board: &mut BoardPane,
    column_index: usize,
    card_index: usize,
) -> Result<(), String> {
    board.select_card(column_index, card_index);
    if board.selected_column == column_index && board.selected_card == Some(card_index) {
        Ok(())
    } else {
        Err(ilium_prompts::render_value(
            "voice/policy/board-has-no-card-v0-in-column",
            &serde_json::json!({"v0": (card_index).to_string(), "v1": (column_index).to_string()}),
        ))
    }
}

fn required_nonempty(value: Option<String>, field: &str) -> Result<String, String> {
    let value = value.ok_or_else(|| {
        ilium_prompts::render_value(
            "voice/executor/v0-is-required",
            &serde_json::json!({"v0": (field).to_string()}),
        )
    })?;
    if value.trim().is_empty() {
        return Err(ilium_prompts::render_value(
            "voice/executor/v0-must-not-be-empty",
            &serde_json::json!({"v0": (field).to_string()}),
        ));
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
        .ok_or_else(|| {
            ilium_prompts::render_value(
                "voice/executor/unknown-settings-tab",
                &serde_json::json!({"v0": format!("{:?}", label)}),
            )
        })
}

#[cfg(test)]
mod admission_tests {
    use super::*;

    #[test]
    fn full_shared_outbound_credit_refuses_semantic_control_and_exit_receipts() {
        let mut app = App::new(
            "bounded-control-fixture".into(),
            PathBuf::from("/tmp/bounded-control-fixture"),
        );
        let group = app.tree.add_group(ilium_core::ROOT_ID, "work").unwrap();
        let pane = app
            .tree
            .add_pane(group, "shell", ilium_core::PaneContentKind::Terminal)
            .unwrap();
        app.panes.insert(
            pane,
            PaneRuntime::Terminal(Box::new(crate::terminal_view::TerminalView::new(24, 80))),
        );
        let mut limits = crate::ipc_preparation::request_limits();
        limits.jobs = 1;
        app.outbound_admission = Some(
            app.outbound_admission
                .as_ref()
                .unwrap()
                .child(limits)
                .unwrap(),
        );
        assert!(app.queue_request(ClientRequest::UpdateDebugLogging { enabled: true }));
        for action in [
            "press_key",
            "schedule_input",
            "queue_prompt",
            "clear_prompt_queue",
        ] {
            let command:TerminalCommand=serde_json::from_value(json!({"action":action,"target":{"id":pane.0},"key":"enter","text":"original text"})).unwrap();
            let error = execute_terminal(&mut app, command)
                .expect_err("no success receipt before actual admission");
            assert!(error.contains("rejected before admission"));
        }
        let command: SessionCommand = serde_json::from_value(json!({"action":"detach"})).unwrap();
        assert!(execute_session(&mut app, command).is_err());
        assert!(
            app.exit_reason.is_none(),
            "refused detach cannot terminate accepted work"
        );
        assert_eq!(
            app.take_outbound_requests(),
            vec![ClientRequest::UpdateDebugLogging { enabled: true }]
        );
        let command: TerminalCommand = serde_json::from_value(
            json!({"action":"schedule_input","target":{"id":pane.0},"text":"original text"}),
        )
        .unwrap();
        assert_eq!(
            execute_terminal(&mut app, command).unwrap().status,
            "queued"
        );
        assert!(
            matches!(app.take_outbound_requests().as_slice(),[ClientRequest::SchedulePaneInput{text,..}] if text=="original text")
        );
    }
}

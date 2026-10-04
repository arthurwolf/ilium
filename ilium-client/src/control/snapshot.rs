//! Redacted, bounded state snapshots returned to control providers.

use ilium_core::{AgentActivity, AgentClass, NodeId, NodeKind, PaneContentKind, PaneStatus};
use serde::Serialize;
use serde_json::{json, Value};

use crate::app::{App, FocusTarget, Mode, PaneRuntime, RightPanelTarget};

use super::command::{NodeTarget, StateDetail};
use super::resolver::{node_path, resolve_node};

const MAX_PANE_CONTENT_CHARACTERS: usize = 6_000;

#[derive(Debug, Serialize)]
pub struct ControlSnapshot {
    pub session: String,
    pub project_directory: String,
    pub mode: &'static str,
    pub focus: &'static str,
    pub selected_node_id: Option<u64>,
    pub active_pane_id: Option<u64>,
    pub right_panel: Value,
    pub nodes: Vec<NodeSnapshot>,
    pub settings: Value,
    pub status_message: Option<String>,
    #[serde(skip)]
    pub(crate) search: Option<SearchCapture>,
}

#[derive(Debug)]
pub(crate) struct SearchCapture {
    query: String,
    revision: u64,
    pending: bool,
    selected: usize,
    error: Option<String>,
    results: std::sync::Arc<Vec<crate::search_ui::SearchResult>>,
    _storage: Option<std::sync::Arc<ilium_execution::StorageAdmission>>,
}
impl ControlSnapshot {
    /// CPU-owner-only projection; raw result heaps remain shared until this ends.
    pub(crate) fn prepare(mut self) -> Result<Value, String> {
        for node in &mut self.nodes {
            if let Some(source) = node.pending_content.take() {
                node.content = Some(source.prepare());
            }
        }
        let search = self.search.take();
        let mut value = serde_json::to_value(self).map_err(|error| error.to_string())?;
        if let Some(search) = search {
            let results = search.results.iter().enumerate().map(|(index, result)| json!({
                "index": index, "selected": index == search.selected,
                "pane_id": result.pane_id.0, "kind": result.kind.label(), "name": result.object_name,
                "path": result.path.as_ref().map(|path| path.display().to_string()),
                "context": format!("{}{}{}", result.before, result.matched, result.after),
            })).collect::<Vec<_>>();
            let mut projection = serde_json::Map::new();
            projection.insert("query".into(), Value::String(search.query));
            projection.insert("revision".into(), Value::from(search.revision));
            projection.insert(
                "preparation".into(),
                Value::String(
                    if search.pending {
                        "pending"
                    } else {
                        "complete"
                    }
                    .into(),
                ),
            );
            projection.insert(
                "error".into(),
                search.error.map(Value::String).unwrap_or(Value::Null),
            );
            projection.insert("selected_index".into(), Value::from(search.selected));
            projection.insert("results".into(), Value::Array(results));
            value
                .as_object_mut()
                .ok_or("Control snapshot was not an object")?
                .insert("search".into(), Value::Object(projection));
        } else {
            value
                .as_object_mut()
                .ok_or("Control snapshot was not an object")?
                .insert("search".into(), Value::Null);
        }
        Ok(value)
    }
}

#[derive(Debug, Serialize)]
pub struct NodeSnapshot {
    pub id: u64,
    pub parent_id: Option<u64>,
    pub name: String,
    pub path: String,
    pub kind: String,
    pub status: Option<Value>,
    pub content: Option<Value>,
    #[serde(skip)]
    pending_content: Option<PaneCapture>,
}

/// Admission before capturing owned strings. Eightfold expansion accounts for
/// JSON/map nodes, escaping and simultaneously retained input/output copies.
/// This is cooperative allocation accounting, not an allocator RSS guarantee.
pub(super) fn preflight(app: &App) -> Result<(), String> {
    use ilium_core::AllocationSize;
    let mut bytes = app.tree.retained_bytes().saturating_add(1024 * 1024);
    for id in app.tree.all_ids() {
        bytes = bytes.saturating_add(4096);
        let mut ancestor = Some(id);
        while let Some(id) = ancestor {
            let Some(node) = app.tree.get(id) else {
                break;
            };
            bytes = bytes.saturating_add(node.name.len().saturating_mul(2).saturating_add(1));
            ancestor = node.parent;
        }
    }
    for pane in app.panes.values() {
        match pane {
            PaneRuntime::Terminal(terminal) => {
                // Screen allocation is already charged by the parser. Reserve
                // the largest raw text materialization separately here.
                bytes = bytes.saturating_add(terminal.with_screen(|screen| {
                    let (rows, columns) = screen.size();
                    usize::from(rows)
                        .saturating_mul(usize::from(columns))
                        .saturating_mul(32)
                }));
            }
            PaneRuntime::Editor(editor) => {
                bytes = bytes.saturating_add(
                    editor
                        .path
                        .as_ref()
                        .map_or(0, |path| path.as_os_str().len().saturating_mul(3)),
                );
                bytes = editor.textarea.lines().iter().fold(bytes, |bytes, line| {
                    bytes.saturating_add(line.len()).saturating_add(64)
                });
            }
            PaneRuntime::Board(board) => {
                bytes =
                    bytes.saturating_add(board.storage.path().as_os_str().len().saturating_mul(3));
                for column in &board.columns {
                    bytes = bytes
                        .saturating_add(column.title.len())
                        .saturating_add(4096);
                    for card in &column.cards {
                        bytes = bytes
                            .saturating_add(card.title.len())
                            .saturating_add(card.body.len())
                            .saturating_add(24_000)
                            .saturating_add(4096);
                    }
                }
            }
        }
    }
    for text in [
        &app.session_name,
        &app.git_settings.branch_prefix,
        &app.git_settings.worktree_location_template,
        &app.git_settings.setup_command,
        &app.inference_settings.kilo_gateway.model,
        &app.inference_settings.ollama.base_url,
        &app.inference_settings.ollama.model,
        &app.inference_settings.openai.base_url,
        &app.inference_settings.openai.model,
        &app.inference_settings.anthropic.base_url,
        &app.inference_settings.anthropic.model,
        &app.inference_settings.openrouter.model,
        &app.voice_settings.custom_prompt,
    ] {
        bytes = bytes.saturating_add(text.len());
    }
    for text in [
        app.status_message.as_ref(),
        app.voice_settings.input_device_name.as_ref(),
        app.voice_settings.output_device_name.as_ref(),
        app.reset_monitor_state.claude.last_error.as_ref(),
        app.reset_monitor_state.codex.last_error.as_ref(),
    ] {
        bytes = bytes.saturating_add(text.map_or(0, String::len));
    }
    if let ilium_voice::VoiceConnectionState::Failed(error) = &app.voice_connection_state {
        bytes = bytes.saturating_add(error.len().saturating_mul(2));
    }
    bytes = bytes.saturating_add(app.session_cwd.as_os_str().len().saturating_mul(3));
    bytes = bytes.saturating_add(
        app.sound_settings
            .file
            .as_ref()
            .map_or(0, |path| path.as_os_str().len().saturating_mul(3)),
    );
    for text in app
        .kilo_gateway_models
        .iter()
        .chain(&app.ollama_models)
        .chain(&app.ui_settings.icons.task_progress_frames)
    {
        bytes = bytes.saturating_add(text.len()).saturating_add(256);
    }
    for target in crate::icon_settings::IconTarget::ALL {
        bytes = bytes
            .saturating_add(app.ui_settings.icons.glyph(target).len())
            .saturating_add(256);
    }
    for event in crate::trigger_settings::TriggerEvent::ALL {
        bytes = bytes.saturating_add(
            app.trigger_settings
                .actions_for(event)
                .len()
                .saturating_mul(256),
        );
    }
    if let Mode::Search(state) = &app.mode {
        bytes = bytes
            .saturating_add(state.query.buf.len())
            .saturating_add(state.control_error.as_ref().map_or(0, String::len));
        for result in state.results.iter() {
            bytes = bytes
                .saturating_add(result.object_name.len())
                .saturating_add(result.before.len())
                .saturating_add(result.matched.len())
                .saturating_add(result.after.len())
                .saturating_add(4096)
                .saturating_add(
                    result
                        .path
                        .as_ref()
                        .map_or(0, |path| path.as_os_str().len().saturating_mul(3)),
                );
        }
    }
    if bytes.saturating_mul(8) > 128 * 1024 * 1024 {
        return Err("Control state exceeds bounded preparation admission; request a smaller workspace snapshot".into());
    }
    Ok(())
}

pub fn capture(
    app: &App,
    detail: StateDetail,
    target: &NodeTarget,
) -> Result<ControlSnapshot, String> {
    // A blank `name`/`path` (e.g. `{"path": ""}`) must behave exactly like an
    // omitted target: falling through to `resolve_node` for it would let a
    // request with no active/selected node hard-fail the whole snapshot,
    // where truly omitting the field succeeds (see `NodeTarget::is_specified`).
    let requested_node = if target.is_specified() {
        Some(resolve_node(app, target)?)
    } else {
        None
    };
    let mut node_ids = app.tree.all_ids().collect::<Vec<_>>();
    node_ids.sort_unstable();
    let mut nodes = Vec::with_capacity(node_ids.len());
    for node_id in node_ids {
        let include_content = matches!(detail, StateDetail::Full)
            && requested_node
                .map(|requested| requested == node_id)
                .unwrap_or_else(|| app.active_pane_id() == Some(node_id));
        if let Some(mut node) = node_snapshot(app, node_id) {
            if include_content {
                node.pending_content = capture_pane(app, node_id)?;
            }
            nodes.push(node);
        }
    }

    Ok(ControlSnapshot {
        session: app.session_name.clone(),
        project_directory: app.session_cwd.display().to_string(),
        mode: mode_label(&app.mode),
        focus: match app.focus {
            FocusTarget::Tree => "tree",
            FocusTarget::Pane => "pane",
        },
        selected_node_id: app.selected_node_id().map(|id| id.0),
        active_pane_id: app.active_pane_id().map(|id| id.0),
        right_panel: right_panel_snapshot(&app.right_panel_target),
        nodes,
        settings: settings_snapshot(app),
        status_message: app.status_message.clone(),
        search: match &app.mode {
            Mode::Search(state) => Some(SearchCapture {
                query: state.query.buf.clone(),
                revision: state.revision(),
                pending: state.is_preparing(),
                selected: state.selected_index,
                error: state.control_error.clone(),
                results: std::sync::Arc::clone(&state.results),
                _storage: state.result_retention.clone(),
            }),
            _ => None,
        },
    })
}

fn node_snapshot(app: &App, node_id: NodeId) -> Option<NodeSnapshot> {
    let node = app.tree.get(node_id)?;
    let (kind, status) = match &node.kind {
        NodeKind::Container(_) if node.is_project() => ("project".to_owned(), None),
        NodeKind::Container(_) if node.is_split_view() => ("split_view".to_owned(), None),
        NodeKind::Container(_) => ("group".to_owned(), None),
        NodeKind::Folder { .. } => ("folder".to_owned(), None),
        NodeKind::Pane {
            content, status, ..
        } => (
            match content {
                PaneContentKind::Terminal => "terminal",
                PaneContentKind::Editor => "editor",
                PaneContentKind::Board => "board",
            }
            .to_owned(),
            Some(pane_status_snapshot(status)),
        ),
    };
    Some(NodeSnapshot {
        id: node_id.0,
        parent_id: node.parent.map(|id| id.0),
        name: node.name.clone(),
        path: node_path(app, node_id),
        kind,
        status,
        content: None,
        pending_content: None,
    })
}

/// A control API consumer needs stable, snake_case JSON, not `PaneStatus`'s
/// derived `Debug` output -- that mixes Rust tuple/struct-literal syntax and
/// PascalCase variant names into a field where every other value in this
/// snapshot is deliberately snake_case (see `right_panel_snapshot`'s matching
/// `"kind"`-tagged shape for the same discriminated-union convention).
fn pane_status_snapshot(status: &PaneStatus) -> Value {
    match status {
        PaneStatus::PlainShell => json!({ "kind": "plain_shell" }),
        PaneStatus::Agent(agent) => match agent.goal {
            Some(goal_state) => json!({
                "kind": "agent_with_goal",
                "agent_class": agent_class_key(&agent.class),
                "activity": agent_activity_key(&agent.activity()),
                "goal_state": goal_state_key(&goal_state),
                "completion_unread": agent.completion_unread,
            }),
            None => json!({
                "kind": "agent",
                "agent_class": agent_class_key(&agent.class),
                "activity": agent_activity_key(&agent.activity()),
                "completion_unread": agent.completion_unread,
            }),
        },
        PaneStatus::AgentUnavailable(recovery) => json!({
            "kind": "agent_unavailable",
            "agent_class": agent_class_key(&recovery.process.class),
            "last_known_activity": agent_activity_key(&recovery.last_known_state.activity()),
            "availability": match recovery.availability {
                ilium_core::AgentAvailability::Unverified => "unverified",
                ilium_core::AgentAvailability::ShellForeground => "shell_foreground",
                ilium_core::AgentAvailability::Exited(_) => "exited",
            },
            "exit_outcome": match recovery.availability {
                ilium_core::AgentAvailability::Exited(outcome) => Some(outcome.label()),
                _ => None,
            },
            "exit_signal_name": recovery.signal_name.as_deref(),
            "has_verified_session": recovery.session_id.is_some(),
            "latest_prompt_unavailable": recovery.latest_prompt_unavailable,
        }),
        PaneStatus::Editor { dirty } => json!({ "kind": "editor", "dirty": dirty }),
        PaneStatus::Board => json!({ "kind": "board" }),
    }
}

fn goal_state_key(goal_state: &ilium_core::GoalState) -> &'static str {
    match goal_state {
        ilium_core::GoalState::Active => "active",
        ilium_core::GoalState::Paused => "paused",
        ilium_core::GoalState::Blocked => "blocked",
        ilium_core::GoalState::UsageLimited => "usage_limited",
        ilium_core::GoalState::Reached => "reached",
    }
}

/// Machine-stable identifier for an agent class, matching the lowercase keys
/// `BuiltinAgentProvider::from_config_name` already accepts. `AgentClass::label`
/// stays reserved for the capitalized, human-facing product name.
fn agent_class_key(class: &AgentClass) -> String {
    match class {
        AgentClass::Claude => "claude".to_owned(),
        AgentClass::Codex => "codex".to_owned(),
        AgentClass::Antigravity => "antigravity".to_owned(),
        AgentClass::Other(name) => name.clone(),
    }
}

fn agent_activity_key(activity: &AgentActivity) -> &'static str {
    match activity {
        AgentActivity::Working => "working",
        AgentActivity::WaitingBackground => "waiting_background",
        AgentActivity::BackgroundTaskStillRunning => "background_task_still_running",
        AgentActivity::WaitingApproval => "waiting_approval",
        AgentActivity::Done => "done",
        AgentActivity::Idle => "idle",
    }
}

enum PaneCapture {
    Terminal(crate::terminal_view::PreparationSnapshot),
    Editor {
        lines: Vec<String>,
        metadata: serde_json::Map<String, Value>,
    },
    Board {
        columns: Vec<crate::board::BoardColumn>,
        metadata: serde_json::Map<String, Value>,
    },
}
impl std::fmt::Debug for PaneCapture {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Terminal(_) => "TerminalCapture",
            Self::Editor { .. } => "EditorCapture",
            Self::Board { .. } => "BoardCapture",
        })
    }
}
impl PaneCapture {
    fn prepare(self) -> Value {
        match self {
            Self::Terminal(snapshot) => {
                let visible_text = snapshot.visible.contents();
                json!({ "visible_text": suffix_by_characters(&visible_text, MAX_PANE_CONTENT_CHARACTERS),
                    "scrollback_position":snapshot.scrollback_position,"scrollback_total":snapshot.scrollback_total })
            }
            Self::Editor {
                lines,
                mut metadata,
            } => {
                let full_text = lines.join("\n");
                let start = suffix_start_index(&full_text, MAX_PANE_CONTENT_CHARACTERS);
                let text_start_line = full_text[..start].matches('\n').count() + 1;
                metadata.insert("text".into(), Value::String(full_text[start..].into()));
                metadata.insert("text_truncated".into(), Value::Bool(start > 0));
                metadata.insert("text_start_line".into(), Value::from(text_start_line));
                Value::Object(metadata)
            }
            Self::Board {
                columns,
                mut metadata,
            } => {
                let columns = columns
                    .into_iter()
                    .enumerate()
                    .map(|(column_index, column)| {
                        let cards = column
                            .cards
                            .into_iter()
                            .enumerate()
                            .map(|(card_index, card)| {
                                let mut value = serde_json::Map::new();
                                value.insert("index".into(), Value::from(card_index));
                                value.insert("title".into(), Value::String(card.title));
                                value.insert(
                                    "body".into(),
                                    Value::String(suffix_by_characters(
                                        &card.body,
                                        MAX_PANE_CONTENT_CHARACTERS,
                                    )),
                                );
                                Value::Object(value)
                            })
                            .collect::<Vec<_>>();
                        let mut value = serde_json::Map::new();
                        value.insert("index".into(), Value::from(column_index));
                        value.insert("title".into(), Value::String(column.title));
                        value.insert("cards".into(), Value::Array(cards));
                        Value::Object(value)
                    })
                    .collect();
                metadata.insert("columns".into(), Value::Array(columns));
                Value::Object(metadata)
            }
        }
    }
}
fn capture_pane(app: &App, pane_id: NodeId) -> Result<Option<PaneCapture>, String> {
    let Some(pane) = app.panes.get(&pane_id) else {
        return Ok(None);
    };
    Ok(Some(match pane {
        PaneRuntime::Terminal(terminal) => {
            PaneCapture::Terminal(terminal.try_preparation_snapshot()?)
        }
        PaneRuntime::Editor(editor) => {
            let metadata = json!({
                "path":editor.path.as_ref().map(|path|path.display().to_string()),"dirty":editor.dirty,
                "view_mode":format!("{:?}",editor.view_mode).to_ascii_lowercase(),
                "line_numbers":editor.show_line_numbers,"minimap":editor.show_minimap,"autosave":editor.show_autosave,
                "cursor":{"line":editor.textarea.cursor().0+1,"column":editor.textarea.cursor().1+1},
            });
            let Value::Object(metadata) = metadata else {
                unreachable!("object literal");
            };
            PaneCapture::Editor {
                lines: editor.textarea.lines().to_vec(),
                metadata,
            }
        }
        PaneRuntime::Board(board) => {
            let metadata = json!({"storage":board.storage.path().display().to_string(),"selected_column":board.selected_column,
                "selected_card":board.selected_card,"detail_panel_open":board.is_detail_panel_open});
            let Value::Object(metadata) = metadata else {
                unreachable!("object literal");
            };
            PaneCapture::Board {
                columns: board.columns.clone(),
                metadata,
            }
        }
    }))
}

fn settings_snapshot(app: &App) -> Value {
    let icons = crate::icon_settings::IconTarget::ALL
        .into_iter()
        .map(|target| {
            (
                target.key().to_owned(),
                json!(app.ui_settings.icons.glyph(target)),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    let keybindings = app
        .keybindings
        .iter()
        .map(|binding| {
            (
                crate::keymap::action_name(binding.action).to_owned(),
                json!(crate::keymap::key_label(binding.key)),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    json!({
        "writable_path_patterns": [
            "ui.left_panel_sizing_mode", "ui.left_panel_fixed_width",
            "ui.left_panel_unfocused_width", "ui.left_panel_focused_width",
            "ui.left_panel_minimum_terminal_width", "ui.tree_order",
            "ui.tree_row_management_controls", "ui.agent_identifier_mode", "ui.color_scheme", "ui.motion_level",
            "ui.sidebar_density", "ui.stable_glyphs", "ui.agent_debug_menu_enabled", "ui.task_progress_style", "ui.icons.<icon_key>",
            "terminal.scrollback_budget_mib", "terminal.new_pane_directory",
            "terminal.smart_copy_light", "terminal.smart_copy_light_key",
            "editor.line_numbers", "editor.minimap", "editor.autosave",
            "editor.autosave_delay_ms", "editor.markdown_rendered_by_default",
            "session.recovery_policy", "session.backups_enabled", "keyboard.shortcut_base", "keyboard.preset",
            "keyboard.bindings.<action_name>", "kanban_board.card_preview_lines",
            "kanban_board.minimum_column_width", "sound.source", "sound.file",
            "sound.events.agent_finished", "sound.events.approval_required",
            "sound.events.agent_started", "sound.events.waiting_background",
            "sound.events.task_succeeded", "sound.events.task_failed",
            "notifications.enabled", "notifications.agent_finished",
            "notifications.approval_required", "notifications.task_succeeded",
            "notifications.task_failed", "notifications.suppress_redundant_task_outcomes",
            "notifications.task_coalesce_seconds",
            "triggers.<event_key>",
            "inference.provider", "inference.title_style", "inference.restructure_prompt_token_limit", "inference.kilo_gateway.model",
            "inference.ollama.url", "inference.ollama.model",
            "inference.openai.url", "inference.openai.api_key", "inference.openai.model",
            "inference.anthropic.url", "inference.anthropic.api_key", "inference.anthropic.model",
            "inference.openrouter.api_key", "inference.openrouter.model",
            "voice.enabled", "voice.api_key", "voice.model", "voice.voice",
            "voice.reasoning_effort", "voice.input_mode", "voice.vad_eagerness",
            "voice.input_device", "voice.output_device", "voice.output_volume_percent",
            "voice.confirm_terminal_submissions", "voice.pause_media_while_active",
            "voice.custom_prompt",
            "reset_planning.monitor_claude", "reset_planning.monitor_codex",
            "reset_planning.time_style",
            "debug.file_logging_enabled",
            "git.default_where", "git.branch_prefix", "git.worktree_location_template",
            "git.default_base", "git.branch_line", "git.setup_command", "git.default_close_policy"
        ],
        "ui": {
            "left_panel_sizing_mode": match app.ui_settings.left_panel_sizing.mode {
                crate::config::LeftPanelSizingMode::Fixed => "fixed",
                crate::config::LeftPanelSizingMode::FocusDependent => "focus_dependent",
                crate::config::LeftPanelSizingMode::TerminalWidthDependent => {
                    "terminal_width_dependent"
                }
            },
            "left_panel_fixed_width": app.ui_settings.left_panel_sizing.fixed_width,
            "left_panel_unfocused_width": app.ui_settings.left_panel_sizing.unfocused_width,
            "left_panel_focused_width": app.ui_settings.left_panel_sizing.focused_width,
            "left_panel_minimum_terminal_width": app.ui_settings.left_panel_sizing.minimum_terminal_width,
            "tree_order": format!("{:?}", app.ui_settings.tree_order).to_ascii_lowercase(),
            "tree_row_management_controls": app.ui_settings.show_tree_row_management_controls,
            "color_scheme": format!("{:?}", app.ui_settings.color_scheme).to_ascii_lowercase(),
            "stable_glyphs": app.ui_settings.use_stable_glyphs,
            "agent_identifier_mode": app.ui_settings.agent_identifiers.mode.label(),
            "motion_level": app.ui_settings.motion_level.label(),
            "sidebar_density": app.ui_settings.sidebar_density.label(),
            "task_progress_style": crate::icon_settings::task_progress_preset_index(
                &app.ui_settings.icons.task_progress_frames
            ).map(|index| crate::icon_settings::TASK_PROGRESS_STYLE_NAMES[index]).unwrap_or("Custom"),
            "task_progress_frames": &app.ui_settings.icons.task_progress_frames,
            "agent_debug_menu_enabled": app.ui_settings.agent_debug_menu_enabled,
            "icons": icons,
        },
        "terminal": {
            "scrollback_budget_mib": app.terminal_settings.scrollback_budget_mib,
            "new_pane_directory": format!("{:?}", app.terminal_settings.new_pane_directory).to_ascii_lowercase(),
            "smart_copy_light": app.terminal_settings.smart_copy_light,
            "smart_copy_light_key": app.terminal_settings.smart_copy_light_key.config_name(),
        },
        "editor": {
            "line_numbers": app.editor_settings.show_line_numbers,
            "minimap": app.editor_settings.show_minimap,
            "autosave": app.editor_settings.autosave_enabled,
            "autosave_delay_ms": app.editor_settings.autosave_delay_ms,
            "markdown_rendered_by_default": app.editor_settings.markdown_rendered_by_default,
        },
        "kanban_board": {
            "card_preview_lines": app.kanban_board_settings.card_preview_lines,
            "minimum_column_width": app.kanban_board_settings.minimum_column_width,
        },
        "session": {
            "recovery_policy": app.session_settings.recovery_policy.label(),
            "backups_enabled": app.session_settings.backups_enabled,
        },
        "git": {
            "default_where": match app.git_settings.default_where {
                crate::config::GitDefaultWhere::Here => "here",
                crate::config::GitDefaultWhere::NewWorktree => "new_worktree",
                crate::config::GitDefaultWhere::ExistingWorktree => "existing_worktree",
            },
            "branch_prefix": app.git_settings.branch_prefix,
            "worktree_location_template": app.git_settings.worktree_location_template,
            "default_base": match app.git_settings.default_base {
                crate::config::GitDefaultBase::Current => "current",
                crate::config::GitDefaultBase::DefaultBranch => "default_branch",
            },
            "branch_line": match app.git_settings.branch_line {
                crate::config::GitBranchLine::WorktreeOnly => "worktree_only",
                crate::config::GitBranchLine::Off => "off",
            },
            "setup_command": app.git_settings.setup_command,
            "default_close_policy": match app.git_settings.default_close_policy {
                crate::config::GitClosePolicy::Keep => "keep",
                crate::config::GitClosePolicy::OfferRemovalWhenSafe => "offer_removal_when_safe",
            },
        },
        "keyboard": {
            "shortcut_base": app.keyboard_settings.shortcut_base.label(),
            "bindings": keybindings,
        },
        "sound": {
            "source": app.sound_settings.source.label(),
            "file": app.sound_settings.file.as_ref().map(|path| path.display().to_string()),
            "events": {
                "agent_finished": app.sound_settings.events.agent_finished,
                "approval_required": app.sound_settings.events.approval_required,
                "agent_started": app.sound_settings.events.agent_started,
                "waiting_background": app.sound_settings.events.waiting_background,
                "task_succeeded": app.sound_settings.events.task_succeeded,
                "task_failed": app.sound_settings.events.task_failed,
            },
        },
        "notifications": {
            "enabled": app.notification_settings.enabled,
            "agent_finished": app.notification_settings.agent_finished,
            "approval_required": app.notification_settings.approval_required,
            "task_succeeded": app.notification_settings.task_succeeded,
            "task_failed": app.notification_settings.task_failed,
            "suppress_redundant_task_outcomes":
                app.notification_settings.suppress_redundant_task_outcomes,
            "task_coalesce_seconds": app.notification_settings.task_coalesce_seconds,
        },
        "inference": {
            "provider": app.inference_settings.selected_provider.label(),
            "title_style": match app.inference_settings.title_style {
                ilium_inference::TitleStyle::Labeling => "labeling",
                ilium_inference::TitleStyle::Summarization => "summarization",
            },
            "restructure_prompt_token_limit": app.inference_settings.restructure_prompt_token_limit,
            "selected_model": app.inference_settings.selected_model(),
            "kilo_gateway": {
                "model": app.inference_settings.kilo_gateway.model,
                "available_free_models": app.kilo_gateway_models,
            },
            "ollama": {
                "url": app.inference_settings.ollama.base_url,
                "model": app.inference_settings.ollama.model,
                "available_models": app.ollama_models,
            },
            "openai": {
                "url": app.inference_settings.openai.base_url,
                "model": app.inference_settings.openai.model,
                "api_key_configured": !app.inference_settings.openai.api_key.is_empty(),
            },
            "anthropic": {
                "url": app.inference_settings.anthropic.base_url,
                "model": app.inference_settings.anthropic.model,
                "api_key_configured": !app.inference_settings.anthropic.api_key.is_empty(),
            },
            "openrouter": {
                "model": app.inference_settings.openrouter.model,
                "api_key_configured": !app.inference_settings.openrouter.api_key.is_empty(),
            },
            "credentials": "redacted",
        },
        "triggers": &app.trigger_settings,
        "voice": {
            "enabled": app.voice_settings.enabled,
            "connection_state": format!("{:?}", app.voice_connection_state),
            "api_key": "redacted",
            "api_key_configured": !app.voice_settings.api_key.is_empty() || std::env::var_os("OPENAI_API_KEY").is_some(),
            "model": app.voice_settings.model.api_name(),
            "voice": app.voice_settings.voice.api_name(),
            "reasoning_effort": app.voice_settings.reasoning_effort.api_name(),
            "input_mode": app.voice_settings.input_mode.label(),
            "vad_eagerness": app.voice_settings.vad_eagerness.api_name(),
            "input_device": app.voice_settings.input_device_name,
            "output_device": app.voice_settings.output_device_name,
            "output_volume_percent": app.voice_settings.output_volume_percent,
            "confirm_terminal_submissions": app.voice_settings.confirm_terminal_submissions,
            "pause_media_while_active": app.voice_settings.pause_media_while_active,
            "custom_prompt": app.voice_settings.custom_prompt,
        },
        "debug": {
            "file_logging_enabled": app.debug_settings.file_logging_enabled,
        },
        "reset_planning": {
            "monitor_claude": app.reset_planning_settings.monitor_claude,
            "monitor_codex": app.reset_planning_settings.monitor_codex,
            "time_style": match app.reset_planning_settings.time_style {
                crate::reset_planning::ResetTimeStyle::Exact => "exact",
                crate::reset_planning::ResetTimeStyle::Human => "human",
            },
            "claude_last_error": app.reset_monitor_state.claude.last_error,
            "codex_last_error": app.reset_monitor_state.codex.last_error,
        },
    })
}

fn right_panel_snapshot(target: &RightPanelTarget) -> Value {
    match target {
        RightPanelTarget::Empty => json!({ "kind": "empty" }),
        RightPanelTarget::Chatroom { project_id } => {
            json!({ "kind": "chatroom", "project_id": project_id.0 })
        }
        RightPanelTarget::Pane { pane_id } => {
            json!({ "kind": "pane", "active_pane_id": pane_id.0 })
        }
        RightPanelTarget::SplitView {
            split_id,
            active_pane_id,
        } => json!({
            "kind": "split_view",
            "split_id": split_id.0,
            "active_pane_id": active_pane_id.map(|id| id.0),
        }),
    }
}

pub(crate) fn mode_label(mode: &Mode) -> &'static str {
    match mode {
        Mode::Normal => "normal",
        Mode::LeaderPending => "leader_pending",
        Mode::NavigationLeaderPending => "navigation_leader_pending",
        Mode::Move => "move",
        Mode::Rename(_) => "rename",
        Mode::CommandPrompt(_) => "command_prompt",
        Mode::InferenceSettingPrompt(_, _) => "inference_setting_prompt",
        Mode::VoiceSettingPrompt(_, _) => "voice_setting_prompt",
        Mode::ApiSettingPrompt(_) => "api_setting_prompt",
        Mode::AgentSetupPathPrompt(_, _) => "agent_setup_path_prompt",
        Mode::AgentSetupPrompt(_) => "agent_setup_prompt",
        Mode::VoicePromptEditor(_) => "voice_prompt_editor",
        Mode::SaveAs(..) => "save_as",
        Mode::Help => "help",
        Mode::Explorer(..) => "file_picker",
        Mode::ExplorerFileMenu(_) => "file_actions",
        Mode::FolderExplorer(..) => "folder_picker",
        Mode::ProjectFolderExplorer(..) => "project_folder_picker",
        Mode::ContextMenu(_) => "context_menu",
        Mode::TerminalPaneContextMenu(_) => "terminal_pane_context_menu",
        Mode::AgentToolbarModelSubmenu(_) => "agent_toolbar_model_submenu",
        Mode::SmartCopy => "smart_copy",
        Mode::AgentDebugLog(_) => "agent_debug_log",
        Mode::AgentDebugSavePath(_, _) => "agent_debug_save_path",
        Mode::SchedulePaneInput(_) => "schedule_input",
        Mode::QueuePrompt(_) => "queue_prompt",
        Mode::ValueDialog(_) => "value_options",
        Mode::TextTriggerDialog(_) => "text_trigger_dialog",
        Mode::EditorLineContextMenu(_) => "editor_line_menu",
        Mode::CreateAgentFromLine(_) => "create_agent",
        Mode::CreateAgentWorkspace(_) => "create_agent_workspace",
        Mode::WorktreeManager(_) => "worktree_manager",
        Mode::WaitingWorkspaceCloseOffer { .. } => "waiting_workspace_close_offer",
        Mode::ConfirmWorkspaceCloseOffer(_) => "confirm_workspace_close_offer",
        Mode::GitSettingPrompt(_, _) => "git_setting_prompt",
        Mode::CreateGroup(_) => "create_group",
        Mode::CreateSplitOrientation(_) => "create_split_orientation",
        Mode::CreateSplitMembers(_) => "create_split_members",
        Mode::CreateBoard(_) => "create_board",
        Mode::BoardPathPicker(_) => "board_path_picker",
        Mode::BoardCardPrompt(..) => "board_card_prompt",
        Mode::BoardColumnPrompt(..) => "board_column_prompt",
        Mode::BoardRenamePrompt(..) => "board_rename_prompt",
        Mode::BoardDeleteConfirm(..) => "board_delete_confirm",
        Mode::ConfirmClose(_) => "confirm_close",
        Mode::ConvertSession => "convert_session",
        Mode::ConfirmRemoveWorkspace(_) => "confirm_remove_workspace",
        Mode::ConfirmSessionRecovery { .. } => "confirm_session_recovery",
        Mode::Settings(_) => "settings",
        Mode::SettingsHelp(_) => "settings_help",
        Mode::Search(_) => "search",
        Mode::AnimationTextPrompt(_, _) => "animation_text_prompt",
        Mode::LocationPicker(_) => "location_picker",
    }
}

fn suffix_by_characters(text: &str, maximum_characters: usize) -> String {
    let start = suffix_start_index(text, maximum_characters);
    text[start..].to_owned()
}

/// Byte offset of the first character kept by [`suffix_by_characters`], so
/// callers that also need to describe *where* the kept suffix begins (e.g.
/// which source line) don't have to redo the same char-boundary walk.
fn suffix_start_index(text: &str, maximum_characters: usize) -> usize {
    text.char_indices()
        .rev()
        .nth(maximum_characters)
        .map(|(index, character)| index + character.len_utf8())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_snapshot_exposes_title_style_as_a_writable_choice() {
        let mut app = App::new(
            "default".to_owned(),
            std::path::PathBuf::from("/tmp/project"),
        );
        app.inference_settings.title_style = ilium_inference::TitleStyle::Labeling;
        let snapshot = settings_snapshot(&app);
        assert_eq!(snapshot["inference"]["title_style"], "labeling");
        assert!(snapshot["writable_path_patterns"]
            .as_array()
            .is_some_and(|paths| paths.iter().any(|path| path == "inference.title_style")));
    }

    #[test]
    fn redaction_suffix_keeps_valid_unicode_and_the_requested_limit() {
        let value = format!("{}tail", "é".repeat(20));
        let suffix = suffix_by_characters(&value, 7);

        assert_eq!(suffix.chars().count(), 7);
        assert_eq!(suffix, "ééétail");
    }
}

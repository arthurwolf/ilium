//! Redacted, bounded state snapshots returned to control providers.

use ilium_core::{NodeId, NodeKind, PaneContentKind};
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
}

#[derive(Debug, Serialize)]
pub struct NodeSnapshot {
    pub id: u64,
    pub parent_id: Option<u64>,
    pub name: String,
    pub path: String,
    pub kind: String,
    pub status: Option<String>,
    pub content: Option<Value>,
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
    let nodes = node_ids
        .into_iter()
        .filter_map(|node_id| {
            let include_content = matches!(detail, StateDetail::Full)
                && requested_node
                    .map(|requested| requested == node_id)
                    .unwrap_or_else(|| app.active_pane_id() == Some(node_id));
            node_snapshot(app, node_id, include_content)
        })
        .collect();

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
    })
}

fn node_snapshot(app: &App, node_id: NodeId, include_content: bool) -> Option<NodeSnapshot> {
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
            Some(format!("{status:?}")),
        ),
    };
    Some(NodeSnapshot {
        id: node_id.0,
        parent_id: node.parent.map(|id| id.0),
        name: node.name.clone(),
        path: node_path(app, node_id),
        kind,
        status,
        content: include_content
            .then(|| pane_content_snapshot(app, node_id))
            .flatten(),
    })
}

fn pane_content_snapshot(app: &App, pane_id: NodeId) -> Option<Value> {
    match app.panes.get(&pane_id)? {
        PaneRuntime::Terminal(terminal) => {
            let visible_text = terminal.with_screen(|screen| screen.contents());
            Some(json!({
                "visible_text": suffix_by_characters(&visible_text, MAX_PANE_CONTENT_CHARACTERS),
                "scrollback_position": terminal.scrollback_position(),
                "scrollback_total": terminal.scrollback_total(),
            }))
        }
        PaneRuntime::Editor(editor) => {
            let full_text = editor.textarea.lines().join("\n");
            let truncation_start = suffix_start_index(&full_text, MAX_PANE_CONTENT_CHARACTERS);
            // `cursor.line` is a buffer-absolute line number, but `text` may be
            // only the tail of the buffer once it exceeds the character
            // budget. Without `text_start_line`, a consumer combining the two
            // fields has no way to tell that "line 500" isn't the 500th line
            // of the (truncated) `text` string -- report where `text` actually
            // starts so the two stay mutually consistent.
            let text_start_line = full_text[..truncation_start].matches('\n').count() + 1;
            Some(json!({
                "path": editor.path.as_ref().map(|path| path.display().to_string()),
                "dirty": editor.dirty,
                "view_mode": format!("{:?}", editor.view_mode).to_ascii_lowercase(),
                "line_numbers": editor.show_line_numbers,
                "minimap": editor.show_minimap,
                "autosave": editor.show_autosave,
                "cursor": { "line": editor.textarea.cursor().0 + 1, "column": editor.textarea.cursor().1 + 1 },
                "text": &full_text[truncation_start..],
                "text_truncated": truncation_start > 0,
                "text_start_line": text_start_line,
            }))
        }
        PaneRuntime::Board(board) => Some(json!({
            "storage": board.storage.path().display().to_string(),
            "selected_column": board.selected_column,
            "selected_card": board.selected_card,
            "detail_panel_open": board.is_detail_panel_open,
            "columns": board.columns.iter().enumerate().map(|(column_index, column)| json!({
                "index": column_index,
                "title": column.title,
                "cards": column.cards.iter().enumerate().map(|(card_index, card)| json!({
                    "index": card_index,
                    "title": card.title,
                    "body": suffix_by_characters(&card.body, MAX_PANE_CONTENT_CHARACTERS),
                })).collect::<Vec<_>>()
            })).collect::<Vec<_>>()
        })),
    }
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
            "ui.sidebar_density", "ui.stable_glyphs", "ui.agent_debug_menu_enabled", "ui.icons.<icon_key>",
            "terminal.scrollback_budget_mib", "terminal.new_pane_directory",
            "editor.line_numbers", "editor.minimap", "editor.autosave",
            "editor.autosave_delay_ms", "editor.markdown_rendered_by_default",
            "session.recovery_policy", "keyboard.shortcut_base", "keyboard.preset",
            "keyboard.bindings.<action_name>", "kanban_board.card_preview_lines",
            "kanban_board.minimum_column_width", "sound.source", "sound.file",
            "sound.events.agent_finished", "sound.events.approval_required",
            "sound.events.agent_started", "sound.events.waiting_background",
            "triggers.<event_key>",
            "inference.provider", "inference.kilo_gateway.model",
            "inference.ollama.url", "inference.ollama.model",
            "inference.openai.url", "inference.openai.api_key", "inference.openai.model",
            "inference.anthropic.url", "inference.anthropic.api_key", "inference.anthropic.model",
            "inference.openrouter.api_key", "inference.openrouter.model",
            "voice.enabled", "voice.api_key", "voice.model", "voice.voice",
            "voice.reasoning_effort", "voice.input_mode", "voice.vad_eagerness",
            "voice.input_device", "voice.output_device", "voice.output_volume_percent",
            "voice.confirm_terminal_submissions", "voice.custom_prompt",
            "debug.file_logging_enabled"
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
            "agent_debug_menu_enabled": app.ui_settings.agent_debug_menu_enabled,
            "icons": icons,
        },
        "terminal": {
            "scrollback_budget_mib": app.terminal_settings.scrollback_budget_mib,
            "new_pane_directory": format!("{:?}", app.terminal_settings.new_pane_directory).to_ascii_lowercase(),
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
            },
        },
        "inference": {
            "provider": app.inference_settings.selected_provider.label(),
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
            "custom_prompt": app.voice_settings.custom_prompt,
        },
        "debug": {
            "file_logging_enabled": app.debug_settings.file_logging_enabled,
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
        Mode::VoicePromptEditor(_) => "voice_prompt_editor",
        Mode::SaveAs(..) => "save_as",
        Mode::Help => "help",
        Mode::Explorer(..) => "file_picker",
        Mode::ExplorerFileMenu(_) => "file_actions",
        Mode::FolderExplorer(..) => "folder_picker",
        Mode::ProjectFolderExplorer(..) => "project_folder_picker",
        Mode::ContextMenu(_) => "context_menu",
        Mode::TerminalPaneContextMenu(_) => "terminal_pane_context_menu",
        Mode::AgentDebugLog(_) => "agent_debug_log",
        Mode::AgentDebugSavePath(_, _) => "agent_debug_save_path",
        Mode::SchedulePaneInput(_) => "schedule_input",
        Mode::QueuePrompt(_) => "queue_prompt",
        Mode::EditorLineContextMenu(_) => "editor_line_menu",
        Mode::CreateAgentFromLine(_) => "create_agent",
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
        Mode::ConfirmSessionRecovery { .. } => "confirm_session_recovery",
        Mode::Settings(_) => "settings",
        Mode::Search(_) => "search",
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
    fn redaction_suffix_keeps_valid_unicode_and_the_requested_limit() {
        let value = format!("{}tail", "é".repeat(20));
        let suffix = suffix_by_characters(&value, 7);

        assert_eq!(suffix.chars().count(), 7);
        assert_eq!(suffix, "ééétail");
    }
}

//! Stable Realtime function schemas for ilium's semantic control surface.

use ilium_voice::VoiceToolDefinition;
use serde_json::{json, Value};

pub(super) const GET_STATE_TOOL_NAME: &str = "ilium_get_state";
pub(super) const STOP_VOICE_MODE_TOOL_NAME: &str = "ilium_stop_voice_mode";
pub(super) const UI_TOOL_NAME: &str = "ilium_ui";
pub(super) const TREE_TOOL_NAME: &str = "ilium_tree";
pub(super) const SEND_TO_TERMINAL_TOOL_NAME: &str = "ilium_send_to_terminal";
pub(super) const TYPE_IN_TERMINAL_TOOL_NAME: &str = "ilium_type_in_terminal";
pub(super) const TERMINAL_TOOL_NAME: &str = "ilium_terminal";
pub(super) const EDITOR_TOOL_NAME: &str = "ilium_editor";
pub(super) const BOARD_TOOL_NAME: &str = "ilium_board";
pub(super) const SETTINGS_TOOL_NAME: &str = "ilium_settings";
pub(super) const SEARCH_TOOL_NAME: &str = "ilium_search";
pub(super) const SESSION_TOOL_NAME: &str = "ilium_session";
pub(super) const CONFIRM_ACTION_TOOL_NAME: &str = "ilium_confirm_action";

pub fn definitions() -> Vec<VoiceToolDefinition> {
    vec![
        tool(
            GET_STATE_TOOL_NAME,
            "Inspect the current ilium session, tree, focus, visible right panel, and optionally pane content. Call this before acting when a target is ambiguous.",
            json!({
                "type": "object",
                "properties": {
                    "detail": { "type": "string", "enum": ["compact", "full"] },
                    "target": target_schema(),
                },
                "additionalProperties": false,
            }),
        ),
        tool(
            STOP_VOICE_MODE_TOOL_NAME,
            "Immediately stop and disable the current ilium voice mode when the user asks to stop, disable, turn off, exit, or end voice mode. This ends the voice session, so do not merely acknowledge the request in speech.",
            json!({
                "type": "object",
                "properties": {},
                "additionalProperties": false,
            }),
        ),
        tool(
            UI_TOOL_NAME,
            "Navigate global UI surfaces: focus tree or panes, move through a split, show a split, open settings/search/help, choose a settings tab, or close the current overlay.",
            json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["focus_tree", "focus_pane", "focus_next_pane", "focus_previous_pane", "focus_pane_direction", "show_split", "open_settings", "show_settings_tab", "open_search", "open_help", "close_overlay"] },
                    "target": target_schema(),
                    "tab": { "type": "string" },
                    "direction": direction_schema(),
                },
                "required": ["action"],
                "additionalProperties": false,
            }),
        ),
        tool(
            TREE_TOOL_NAME,
            "Create, open, organize, rename, move, reparent, split, or close any left-panel object. Structural changes are queued to ilium-server and confirmed by a later state snapshot.",
            json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["create_terminal", "create_agent", "create_command_pane", "open_editor", "add_folder", "add_project", "change_project_folder", "create_group", "create_board", "create_split", "rename", "move_up", "move_down", "reparent", "close", "toggle_expanded", "retitle", "restructure_all_projects", "restructure_project", "revert_project_restructure"] },
                    "target": target_schema(),
                    "parent": target_schema(),
                    "name": { "type": "string" },
                    "path": { "type": "string" },
                    "command_line": { "type": "string" },
                    "initial_input": { "type": "string" },
                    "orientation": { "type": "string", "enum": ["horizontal", "vertical"] },
                    "members": { "type": "array", "items": target_schema(), "maxItems": 4 },
                    "index": { "type": "integer", "minimum": 0 },
                    "storage": { "type": "string", "enum": ["markdown", "folder"] },
                    "provider": { "type": "string", "enum": ["claude", "codex", "antigravity"] },
                },
                "required": ["action"],
                "additionalProperties": false,
            }),
        ),
        tool(
            SEND_TO_TERMINAL_TOOL_NAME,
            "Primary voice-dictation action. Send the exact dictated text to a terminal or coding-agent pane and submit it with a final Enter key. Omit target for the currently active/open pane. Every call submits; this tool cannot leave text staged or unsent. Never merely say the text as a substitute for calling this tool.",
            json!({
                "type": "object",
                "properties": {
                    "target": target_schema(),
                    "text": { "type": "string", "description": "Exact text to type, preserving slash commands, punctuation, paths, and code." },
                },
                "required": ["text"],
                "additionalProperties": false,
            }),
        ),
        tool(
            TYPE_IN_TERMINAL_TOOL_NAME,
            "Type exact text into a terminal without pressing Enter. Use only when the user explicitly asks to type, write, or stage text without sending it. If the user asks to send, submit, or press Enter afterward, use ilium_send_to_terminal instead.",
            json!({
                "type": "object",
                "properties": {
                    "target": target_schema(),
                    "text": { "type": "string", "description": "Exact text to leave visible and unsubmitted in the terminal." },
                },
                "required": ["text"],
                "additionalProperties": false,
            }),
        ),
        tool(
            TERMINAL_TOOL_NAME,
            "Control non-dictation terminal behavior: press a supported key, scroll, schedule future submitted input, or manage the durable completion prompt queue. Scheduled and queued text is always submitted with Enter. Use ilium_send_to_terminal for immediate dictated text.",
            json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["press_key", "scroll_up", "scroll_down", "scroll_to_bottom", "schedule_input", "queue_prompt", "clear_prompt_queue"] },
                    "target": target_schema(),
                    "text": { "type": "string" },
                    "key": { "type": "string", "enum": ["enter", "escape", "tab", "backtab", "up", "down", "left", "right", "home", "end", "page_up", "page_down", "backspace", "delete", "space", "control_c", "control_d", "control_l", "control_z"] },
                    "lines": { "type": "integer", "minimum": 1, "maximum": 10000 },
                    "delay_seconds": { "type": "integer", "minimum": 0 },
                    "delivery": { "type": "string", "enum": ["once", "times", "forever"] },
                    "runs": { "type": "integer", "minimum": 1 },
                },
                "required": ["action"],
                "additionalProperties": false,
            }),
        ),
        tool(
            EDITOR_TOOL_NAME,
            "Operate a built-in editor pane: save, save as, insert, replace the full document, jump to a location, or toggle its toolbar modes.",
            json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["save", "save_as", "insert_text", "replace_document", "jump_to", "toggle_rendered", "toggle_line_numbers", "toggle_minimap", "toggle_autosave"] },
                    "target": target_schema(),
                    "text": { "type": "string" },
                    "path": { "type": "string" },
                    "line": { "type": "integer", "minimum": 1 },
                    "column": { "type": "integer", "minimum": 1 },
                },
                "required": ["action"],
                "additionalProperties": false,
            }),
        ),
        tool(
            BOARD_TOOL_NAME,
            "Operate every kanban surface: select/open cards, add/update/rename/move/delete items, close details, and toggle task checkboxes. Column and card indices are zero-based and available from full state.",
            json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["select_column", "select_card", "open_card", "close_card", "add_column", "add_card", "update_card", "rename_column", "move_card", "toggle_checkbox", "delete_card", "delete_column"] },
                    "target": target_schema(),
                    "title": { "type": "string" },
                    "body": { "type": "string" },
                    "column": { "type": "integer", "minimum": 0 },
                    "card": { "type": "integer", "minimum": 0 },
                    "destination_column": { "type": "integer", "minimum": 0 },
                    "destination_card": { "type": "integer", "minimum": 0 },
                    "checkbox": { "type": "integer", "minimum": 0 },
                },
                "required": ["action"],
                "additionalProperties": false,
            }),
        ),
        tool(
            SETTINGS_TOOL_NAME,
            "Get, set, or adjust any persisted setting by dotted path, test inference, refresh Ollama models, or preview sound. Call get to inspect redacted values and the complete writable path catalog before changing an unfamiliar setting.",
            json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["get", "set", "adjust", "test_inference", "refresh_ollama_models", "preview_sound"] },
                    "path": { "type": "string" },
                    "value": {},
                    "direction": direction_schema(),
                },
                "required": ["action"],
                "additionalProperties": false,
            }),
        ),
        tool(
            SEARCH_TOOL_NAME,
            "Search all retained terminal output, open editor buffers, and Kanban content; inspect results, move the selection, or open an exact result.",
            json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["query", "select_next", "select_previous", "open_result", "close"] },
                    "query": { "type": "string" },
                    "index": { "type": "integer", "minimum": 0 },
                },
                "required": ["action"],
                "additionalProperties": false,
            }),
        ),
        tool(
            SESSION_TOOL_NAME,
            "Detach, restart the TUI client, restart the detached server while preserving recovery state, or permanently kill the entire session.",
            json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["detach", "restart_client", "restart_server", "kill_session"] }
                },
                "required": ["action"],
                "additionalProperties": false,
            }),
        ),
        tool(
            CONFIRM_ACTION_TOOL_NAME,
            "Confirm or cancel one pending ilium action. For staged terminal text, yes presses Enter in the original terminal and no leaves the visible text unsubmitted. Only call after the user explicitly answers the preceding confirmation question.",
            json!({
                "type": "object",
                "properties": {
                    "token": { "type": "string" },
                    "confirmed": { "type": "boolean" }
                },
                "required": ["token", "confirmed"],
                "additionalProperties": false,
            }),
        ),
    ]
}

fn tool(name: &str, description: &str, parameters: Value) -> VoiceToolDefinition {
    VoiceToolDefinition {
        name: name.to_owned(),
        description: description.to_owned(),
        parameters,
    }
}

fn target_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "id": { "type": "integer", "minimum": 0 },
            "name": { "type": "string" },
            "path": { "type": "string", "description": "Slash-separated node-name path from the session root" }
        },
        "additionalProperties": false,
    })
}

fn direction_schema() -> Value {
    json!({ "type": "string", "enum": ["up", "down", "left", "right"] })
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn every_tool_name_is_unique_namespaced_and_openai_compatible() {
        let definitions = definitions();
        let names = definitions
            .iter()
            .map(|definition| definition.name.as_str())
            .collect::<HashSet<_>>();

        assert_eq!(names.len(), definitions.len());
        assert!(names.iter().all(|name| {
            name.starts_with("ilium_")
                && name.len() <= 64
                && name.chars().all(|character| {
                    character.is_ascii_alphanumeric() || matches!(character, '_' | '-')
                })
        }));
    }

    #[test]
    fn every_tool_rejects_unknown_arguments() {
        for definition in definitions() {
            assert_eq!(definition.parameters["additionalProperties"], false);
        }
    }

    #[test]
    fn voice_self_stop_has_one_argument_free_explicit_tool() {
        let definitions = definitions();
        let stop_tools = definitions
            .iter()
            .filter(|definition| definition.name == STOP_VOICE_MODE_TOOL_NAME)
            .collect::<Vec<_>>();

        assert_eq!(stop_tools.len(), 1);
        assert_eq!(stop_tools[0].parameters["properties"], json!({}));
        assert!(stop_tools[0].description.contains("stop and disable"));
        assert!(stop_tools[0].description.contains("ends the voice session"));
    }

    #[test]
    fn focused_dictation_has_one_explicit_required_text_tool() {
        let definitions = definitions();
        let send_tool = definitions
            .iter()
            .find(|definition| definition.name == SEND_TO_TERMINAL_TOOL_NAME)
            .expect("dedicated terminal dictation tool");
        let type_tool = definitions
            .iter()
            .find(|definition| definition.name == TYPE_IN_TERMINAL_TOOL_NAME)
            .expect("dedicated unsubmitted terminal typing tool");
        let terminal_tool = definitions
            .iter()
            .find(|definition| definition.name == TERMINAL_TOOL_NAME)
            .expect("general terminal tool");

        assert_eq!(send_tool.parameters["required"], json!(["text"]));
        assert!(send_tool.description.contains("currently active/open pane"));
        assert!(send_tool.description.contains("final Enter key"));
        assert!(send_tool.description.contains("Never merely say the text"));
        assert!(send_tool.parameters["properties"]
            .get("send_enter")
            .is_none());
        assert_eq!(type_tool.parameters["required"], json!(["text"]));
        assert!(type_tool.description.contains("without pressing Enter"));
        assert!(type_tool.description.contains("ilium_send_to_terminal"));
        assert!(terminal_tool.parameters["properties"]
            .get("send_enter")
            .is_none());
        assert!(!terminal_tool.parameters["properties"]["action"]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .any(|action| action == "write"));
    }
}

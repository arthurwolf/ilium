//! Kernel entry points that hand a user action to a plugin command (foundation contract
//! section 15.3).
//!
//! A ported feature keeps its key binding, toolbar button and menu entry in the kernel until
//! the plugins that own those surfaces are ported; what they do is run the plugin's command.
//! The kernel passes what only it knows (the selection) as the command's arguments and
//! decides nothing else.

use ilium_plugin_api::generated::types::CommandOrigin;

use crate::app::App;

/// The Layout plugin's "New split view" command.
pub(crate) const NEW_SPLIT_COMMAND: &str = "ilium.core.layout/new-split";

/// The Kanban plugin's "New board" command. Without a `path` argument it opens the plugin's own
/// creation dialog; with one it opens a board on that storage.
pub(crate) const NEW_BOARD_COMMAND: &str = "ilium.core.kanban/new-board";

/// The pane kind of the Kanban plugin's board pane.
pub(crate) const BOARD_PANE_KIND: &str = "ilium.core.kanban.board";

/// The Kanban plugin's agent-control command: `{ "pane", "action", ... }` applied to a board.
pub(crate) const BOARD_CONTROL_COMMAND: &str = "ilium.core.kanban/control";

/// The Search plugin's "Search the workspace" command; it opens the search panel.
pub(crate) const SEARCH_COMMAND: &str = "ilium.core.search/open";

/// The Keys and help plugin's "Show the key reference" command; it opens the `keys` panel.
pub(crate) const HELP_COMMAND: &str = "ilium.core.keys-and-help/show-keys";

/// The Smart Copy plugin's command; it freezes the pane and opens the candidate picker.
pub(crate) const SMART_COPY_COMMAND: &str = "ilium.core.smart-copy/smart-copy";

/// The Diagnostics plugin's debug-log viewer; the kernel passes the retained history it fetched.
pub(crate) const DEBUG_LOG_COMMAND: &str = "ilium.core.diagnostics/show-debug-log";

/// The most bytes of entries the kernel hands to the viewer; the command arguments are capped at
/// 64 KiB by the host, so the newest entries that fit are sent.
const DEBUG_LOG_ARGUMENT_BUDGET_BYTES: usize = 56 * 1024;

/// The Settings plugin's "Editor line display" command; it opens the clip or wrap choice.
pub(crate) const EDITOR_LINE_DISPLAY_COMMAND: &str = "ilium.core.settings/editor-line-display";

/// The Layout plugin's tree row layout editor; the kernel passes the current layout and the
/// glyphs of the specimen rows.
pub(crate) const TREE_LAYOUT_COMMAND: &str = "ilium.core.layout/tree-layout";

impl App {
    /// Runs a plugin command. Tells the user when no plugin host is running; a command that
    /// no enabled plugin contributes is reported by the host as a toast.
    pub(crate) fn invoke_plugin_command(
        &mut self,
        command: &str,
        arguments_json: String,
        origin: CommandOrigin,
    ) {
        let screen = self.popup_screen();
        let posted = self
            .wasm_plugins
            .as_ref()
            .is_some_and(|plugins| plugins.invoke_command(command, arguments_json, origin, screen));
        if !posted {
            let owner = command.split_once('/').map_or(command, |(owner, _)| owner);
            self.status_message = Some(format!(
                "{owner} is not running, so this action is unavailable"
            ));
        }
    }

    /// "New split view" from the key binding, the tree toolbar or the context menu.
    pub(crate) fn action_new_split_view(&mut self, origin: CommandOrigin) {
        let arguments = self.new_split_arguments();
        self.invoke_plugin_command(NEW_SPLIT_COMMAND, arguments, origin);
    }

    /// The editor toolbar's "Editor line display" chooser. The kernel passes the pane and the mode
    /// it shows now; the Settings plugin draws the choice and sets the mode through
    /// `kernel.set-editor-option`.
    pub(crate) fn action_editor_line_display(
        &mut self,
        pane: ilium_core::NodeId,
        origin: CommandOrigin,
    ) {
        if !matches!(self.mode, crate::app::Mode::Normal) {
            return;
        }
        let Some(crate::app::PaneRuntime::Editor(editor)) = self.panes.get(&pane) else {
            return;
        };
        let arguments = serde_json::json!({
            "pane": pane.0,
            "line_display": match editor.line_display {
                crate::config::LineDisplay::Clip => "clip",
                crate::config::LineDisplay::Wrap => "wrap",
            },
        })
        .to_string();
        self.invoke_plugin_command(EDITOR_LINE_DISPLAY_COMMAND, arguments, origin);
    }

    /// The tree row layout editor (the Icons settings page's `L` key). The Layout plugin draws the
    /// dialog and sets the layout through `kernel.set-tree-row-layout`; the kernel passes the
    /// layout it uses now and the icon glyphs the specimen rows show, each padded to the width
    /// of its slot in the live tree.
    pub(crate) fn action_tree_layout(&mut self, origin: CommandOrigin) {
        let arguments = self.tree_layout_arguments();
        self.invoke_plugin_command(TREE_LAYOUT_COMMAND, arguments, origin);
    }

    fn tree_layout_arguments(&self) -> String {
        use crate::icon_settings::IconTarget;
        let icons = &self.ui_settings.icons;
        let glyph = |target: IconTarget, width: usize| padded_glyph(icons.glyph(target), width);
        serde_json::json!({
            "layout": self.ui_settings.tree_row_layout,
            "glyphs": {
                "group": glyph(IconTarget::Group, 3),
                "split": glyph(IconTarget::SplitVertical, 3),
                "claude": glyph(IconTarget::Claude, 3),
                "codex": glyph(IconTarget::Codex, 3),
                "other_agent": glyph(IconTarget::OtherAgent, 3),
                "goal_active": glyph(IconTarget::GoalActive, crate::status_icons::OBJECTIVE_COLUMN_WIDTH),
                "goal_blocked": glyph(IconTarget::GoalBlocked, crate::status_icons::OBJECTIVE_COLUMN_WIDTH),
                "task_done": glyph(IconTarget::TaskDone, crate::status_icons::OBJECTIVE_COLUMN_WIDTH),
                "working": glyph(IconTarget::Working, crate::status_icons::NOW_COLUMN_WIDTH),
                "waiting_approval": glyph(IconTarget::WaitingApproval, crate::status_icons::NOW_COLUMN_WIDTH),
                "done": glyph(IconTarget::Done, crate::status_icons::NOW_COLUMN_WIDTH),
            },
        })
        .to_string()
    }

    /// Hands the retained debug history of an agent pane to the Diagnostics plugin, which draws
    /// the viewer. The newest entries that fit the command argument limit are sent.
    pub(crate) fn show_agent_debug_log(
        &mut self,
        pane_id: ilium_core::NodeId,
        dropped_entry_count: u64,
        entries: &[ilium_ipc::AgentDebugEntry],
    ) {
        let pane_name = self
            .tree
            .get(pane_id)
            .map_or_else(|| "agent".to_string(), |node| node.name.clone());
        let arguments = debug_log_arguments(pane_id, &pane_name, dropped_entry_count, entries);
        self.invoke_plugin_command(DEBUG_LOG_COMMAND, arguments, CommandOrigin::Menu);
    }

    /// "Search the workspace" from the key binding, the tree toolbar or the context menu.
    pub(crate) fn action_open_search(&mut self, origin: CommandOrigin) {
        self.invoke_plugin_command(SEARCH_COMMAND, "{}".to_owned(), origin);
    }

    /// "Help" from the key binding or the control surface: the key reference the Keys and help
    /// plugin draws. Without that plugin there is no key reference.
    pub(crate) fn action_open_help(&mut self, origin: CommandOrigin) {
        self.invoke_plugin_command(HELP_COMMAND, "{}".to_owned(), origin);
    }

    /// "New board" from the key binding, the tree toolbar or the context menu: the Kanban plugin
    /// opens its own creation dialog under the group the selection points at.
    pub(crate) fn action_new_board(&mut self, origin: CommandOrigin) {
        let parent = self.group_for_new_node();
        self.invoke_plugin_command(
            NEW_BOARD_COMMAND,
            serde_json::json!({ "parent": parent.0 }).to_string(),
            origin,
        );
    }

    /// Opens a board on an existing Markdown file (the explorer and editor "Open as board"
    /// entries). The Kanban plugin owns the storage, so the kernel only hands over the path.
    pub(crate) fn action_new_markdown_board(
        &mut self,
        parent_group: ilium_core::NodeId,
        path: &std::path::Path,
        origin: CommandOrigin,
    ) {
        self.invoke_plugin_command(
            NEW_BOARD_COMMAND,
            serde_json::json!({
                "parent": parent_group.0,
                "path": path.display().to_string(),
                "kind": "markdown",
            })
            .to_string(),
            origin,
        );
    }

    /// "Create board from Markdown" on an editor pane: first flushes any unsaved buffer (the
    /// board reads the file, so it must see what the user sees), then hands the file to the
    /// Kanban plugin.
    pub fn request_board_from_markdown_editor(&mut self, editor_pane_id: ilium_core::NodeId) {
        let dirty_path = match self.panes.get(&editor_pane_id) {
            Some(crate::app::PaneRuntime::Editor(editor))
                if editor.is_markdown() && editor.dirty =>
            {
                editor.path.clone()
            }
            _ => None,
        };
        if let Some(path) = dirty_path {
            if let Err(error) = self.enqueue_editor_save(
                editor_pane_id,
                path,
                crate::filesystem::editors::SavePurpose::OpenAsBoard,
            ) {
                self.status_message = Some(format!(
                    "Could not save Markdown before opening board: {error}"
                ));
            }
            return;
        }
        let path = match self.panes.get_mut(&editor_pane_id) {
            Some(crate::app::PaneRuntime::Editor(editor)) if editor.is_markdown() => {
                editor.path.clone()
            }
            _ => self.restored_editor_paths.get(&editor_pane_id).cloned(),
        };
        let Some(path) = path.filter(|path| crate::editor_pane::is_markdown_path(path)) else {
            self.status_message = Some("This editor is not backed by a Markdown file".to_string());
            return;
        };
        let parent_group = self
            .normal_group_for_node(editor_pane_id)
            .unwrap_or(ilium_core::ROOT_ID);
        self.action_new_markdown_board(parent_group, &path, CommandOrigin::Menu);
    }

    /// The arguments of `new-split`: the group the selection points at. The plugin reads the
    /// tree itself and decides which panes may join.
    fn new_split_arguments(&self) -> String {
        serde_json::json!({ "parent": self.split_parent_group().0 }).to_string()
    }
}

/// The arguments of `show-debug-log`: the pane, its name, how many older entries retention
/// discarded, and the newest entries that fit the argument budget (oldest first).
fn debug_log_arguments(
    pane_id: ilium_core::NodeId,
    pane_name: &str,
    dropped_entry_count: u64,
    entries: &[ilium_ipc::AgentDebugEntry],
) -> String {
    let mut used = 0usize;
    let mut kept: Vec<serde_json::Value> = Vec::new();
    for entry in entries.iter().rev() {
        let Ok(value) = serde_json::to_value(entry) else {
            continue;
        };
        let size = value.to_string().len().saturating_add(1);
        if used.saturating_add(size) > DEBUG_LOG_ARGUMENT_BUDGET_BYTES {
            break;
        }
        used += size;
        kept.push(value);
    }
    let omitted = entries.len() - kept.len();
    kept.reverse();
    serde_json::json!({
        "pane": pane_id.0,
        "name": pane_name,
        "dropped": dropped_entry_count.saturating_add(omitted as u64),
        "entries": kept,
    })
    .to_string()
}

/// `glyph` cut to whole graphemes within `width` cells and padded with spaces to exactly
/// `width`, like a slot of the live tree row (a wide or custom glyph never splits).
fn padded_glyph(glyph: &str, width: usize) -> String {
    use unicode_segmentation::UnicodeSegmentation;
    use unicode_width::UnicodeWidthStr;
    let mut text = String::new();
    let mut used = 0;
    for grapheme in glyph.graphemes(true) {
        let cells = UnicodeWidthStr::width(grapheme);
        if used + cells > width {
            break;
        }
        text.push_str(grapheme);
        used += cells;
    }
    text.push_str(&" ".repeat(width.saturating_sub(used)));
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilium_core::ROOT_ID;

    fn entry(sequence: u64, summary: &str) -> ilium_ipc::AgentDebugEntry {
        ilium_ipc::AgentDebugEntry {
            sequence,
            occurred_at_unix_millis: 1_700_000_000_000,
            severity: ilium_ipc::AgentDebugSeverity::Information,
            source: ilium_ipc::AgentDebugSource::Detector,
            kind: ilium_ipc::AgentDebugEventKind::DetectionCycle,
            summary: summary.to_string(),
            fields: Vec::new(),
            correlation_id: None,
            context: ilium_ipc::AgentDebugContext::default(),
            metadata: Default::default(),
        }
    }

    #[test]
    fn debug_log_arguments_keep_the_newest_entries_that_fit_the_budget() {
        let big = "x".repeat(8 * 1024);
        let entries: Vec<_> = (1..=10).map(|sequence| entry(sequence, &big)).collect();

        let arguments: serde_json::Value = serde_json::from_str(&debug_log_arguments(
            ilium_core::NodeId(4),
            "codex",
            2,
            &entries,
        ))
        .unwrap();

        let sent = arguments["entries"].as_array().unwrap();
        assert!(sent.len() < entries.len());
        assert_eq!(sent.last().unwrap()["sequence"], 10);
        assert_eq!(
            arguments["dropped"].as_u64().unwrap(),
            2 + (entries.len() - sent.len()) as u64
        );
        assert!(arguments.to_string().len() <= 64 * 1024);
        assert_eq!(arguments["pane"], 4);
    }

    #[test]
    fn debug_log_arguments_of_a_short_history_are_complete() {
        let entries = vec![entry(1, "first"), entry(2, "second")];

        let arguments: serde_json::Value = serde_json::from_str(&debug_log_arguments(
            ilium_core::NodeId(4),
            "codex",
            0,
            &entries,
        ))
        .unwrap();

        assert_eq!(arguments["entries"].as_array().unwrap().len(), 2);
        assert_eq!(arguments["entries"][0]["sequence"], 1);
        assert_eq!(arguments["dropped"], 0);
    }

    #[test]
    fn new_split_arguments_carry_the_selected_group() {
        let mut app = App::new("test".to_string(), std::env::temp_dir());
        let group = app.tree.add_group(ROOT_ID, "work").unwrap();
        app.tree_state.select(vec![group]);

        let arguments: serde_json::Value =
            serde_json::from_str(&app.new_split_arguments()).unwrap();

        assert_eq!(arguments, serde_json::json!({ "parent": group.0 }));
    }

    #[test]
    fn the_search_action_without_a_plugin_host_names_the_search_plugin() {
        let mut app = App::new("test".to_string(), std::env::temp_dir());
        app.action_open_search(CommandOrigin::TreeToolbar);
        assert_eq!(
            app.status_message.as_deref(),
            Some("ilium.core.search is not running, so this action is unavailable")
        );
        assert!(!matches!(app.mode, crate::app::Mode::Settings(_)));
    }

    #[test]
    fn the_help_action_without_a_plugin_host_names_the_keys_and_help_plugin() {
        let mut app = App::new("test".to_string(), std::env::temp_dir());
        app.action_open_help(CommandOrigin::Keybinding);
        assert_eq!(
            app.status_message.as_deref(),
            Some("ilium.core.keys-and-help is not running, so this action is unavailable")
        );
        assert!(matches!(app.mode, crate::app::Mode::Normal));
    }

    #[test]
    fn a_plugin_command_without_a_plugin_host_tells_the_user() {
        let mut app = App::new("test".to_string(), std::env::temp_dir());
        app.action_new_split_view(CommandOrigin::Keybinding);
        assert_eq!(
            app.status_message.as_deref(),
            Some("ilium.core.layout is not running, so this action is unavailable")
        );
    }
}

#[cfg(test)]
mod tree_layout_argument_tests {
    use super::*;

    #[test]
    fn specimen_glyphs_match_the_slot_widths_without_splitting_a_glyph() {
        assert_eq!(
            unicode_width::UnicodeWidthStr::width(padded_glyph("🦀", 3).as_str()),
            3
        );
        assert_eq!(padded_glyph("⠋", 2), "⠋ ");
        assert_eq!(padded_glyph("👩‍💻", 1), " ");
    }

    #[test]
    fn the_arguments_carry_the_layout_and_every_specimen_glyph() {
        let app = App::new("tree layout arguments".to_owned(), std::env::temp_dir());
        let value: serde_json::Value =
            serde_json::from_str(&app.tree_layout_arguments()).expect("arguments are JSON");
        assert_eq!(
            value["layout"]["order"],
            serde_json::json!(["identity", "objective", "activity", "title"])
        );
        for key in [
            "group",
            "split",
            "claude",
            "codex",
            "other_agent",
            "goal_active",
            "goal_blocked",
            "task_done",
            "working",
            "waiting_approval",
            "done",
        ] {
            assert!(value["glyphs"][key].is_string(), "missing glyph {key}");
        }
    }
}

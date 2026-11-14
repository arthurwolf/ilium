//! Immutable terminal text captured for the terminal-pane context menu.
//!
//! A terminal screen can continue changing while its context menu is open.
//! Capturing the exact visible line and screen text at the right-click keeps a
//! copy action anchored to the user's target, just like the editor line menu.

use ilium_core::NodeId;
use ratatui::layout::Rect;

use crate::open_target::OpenTarget;

/// Actions available from a terminal pane's right-click menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalContextAction {
    /// Present only when the pane has an active, non-empty text selection
    /// at the moment the menu opens (see `crate::terminal_selection`).
    CopySelectionToClipboard,
    /// Exact authored text captured at menu opening, independent of live detection.
    CopyLastSubmittedPromptToClipboard {
        prompt: String,
    },
    /// Historical exact text, explicitly older than an opaque submission.
    CopyPreviousExactPromptToClipboard {
        prompt: String,
    },
    LastSubmittedPromptUnavailable,
    CopyLineToClipboard,
    CopyVisibleTerminalToClipboard,
    CopyFullTerminalHistoryToClipboard,
    /// A locally verified absolute JSONL path captured when this menu opened.
    CopyHistoryFilePathToClipboard {
        path: std::path::PathBuf,
    },
    PasteClipboard,
    ShowAgentDebugLog,
    /// Present only for a currently detected agent pane. Flips the same
    /// global `ui_settings.agent_toolbar_enabled` flag the toolbar's own X
    /// button and the hamburger icon drive; `currently_visible` only decides
    /// this entry's label ("Show"/"Hide").
    ToggleAgentToolbar {
        currently_visible: bool,
    },
    /// Present only when the clicked cell resolved to an allowed-scheme URL
    /// or a path that exists on disk right now (see
    /// `crate::open_target::resolve_at`).
    OpenExternally(OpenTarget),
    /// Opens the already-verified file target in an ilium editor pane beside
    /// the terminal where the menu was opened.
    OpenInEditor {
        path: std::path::PathBuf,
    },
    /// An entry a plugin contributes to the terminal menu (`contributes.context_menu` with
    /// `target: "terminal"`, gap G6). Choosing it runs `command` with the pane as `node`.
    Plugin {
        title: String,
        command: String,
    },
    /// A link a plugin's `link_detectors` entry matched under the click (G5). Choosing it
    /// runs the detector's command and sends `link-activated` to the plugin, as Ctrl+O does.
    /// The fields are those of the plugin host's link match, kept as plain text so this
    /// public menu type does not expose the crate-private plugin host types.
    PluginLink {
        plugin: String,
        detector_id: String,
        command: String,
        text: String,
    },
}

impl TerminalContextAction {
    pub const fn menu_order(&self) -> (u8, u8) {
        match self {
            Self::OpenInEditor { .. } => (0, 0),
            Self::OpenExternally(_) => (0, 1),
            Self::CopySelectionToClipboard => (1, 0),
            Self::CopyLineToClipboard => (1, 1),
            Self::CopyVisibleTerminalToClipboard => (1, 2),
            Self::CopyFullTerminalHistoryToClipboard => (1, 3),
            Self::CopyLastSubmittedPromptToClipboard { .. }
            | Self::LastSubmittedPromptUnavailable => (2, 0),
            Self::CopyPreviousExactPromptToClipboard { .. } => (2, 1),
            Self::CopyHistoryFilePathToClipboard { .. } => (2, 2),
            Self::PasteClipboard => (3, 0),
            Self::ToggleAgentToolbar { .. } => (4, 0),
            Self::ShowAgentDebugLog => (4, 1),
            Self::Plugin { .. } => (5, 0),
            Self::PluginLink { .. } => (0, 2),
        }
    }

    pub const fn menu_group(&self) -> u8 {
        self.menu_order().0
    }

    /// Returns the shared icon role used by this terminal action.
    pub const fn icon_target(&self) -> crate::icon_settings::IconTarget {
        use crate::icon_settings::IconTarget;
        match self {
            Self::CopySelectionToClipboard => IconTarget::AgentToolbarCopyScreen,
            Self::CopyLastSubmittedPromptToClipboard { .. }
            | Self::CopyPreviousExactPromptToClipboard { .. }
            | Self::CopyLineToClipboard
            | Self::CopyFullTerminalHistoryToClipboard
            | Self::CopyHistoryFilePathToClipboard { .. } => IconTarget::Editor,
            Self::LastSubmittedPromptUnavailable => IconTarget::AgentUnavailable,
            Self::CopyVisibleTerminalToClipboard => IconTarget::Terminal,
            Self::PasteClipboard => IconTarget::ScreenTransferDown,
            Self::ShowAgentDebugLog => IconTarget::Done,
            Self::ToggleAgentToolbar { .. } => IconTarget::AgentToolbarConfig,
            Self::OpenExternally(OpenTarget::Directory(_)) => IconTarget::Folder,
            Self::OpenExternally(OpenTarget::Url(_) | OpenTarget::File(_)) => {
                IconTarget::OpenExternal
            }
            Self::OpenInEditor { .. } => IconTarget::Editor,
            Self::Plugin { .. } => IconTarget::Project,
            Self::PluginLink { .. } => IconTarget::OpenExternal,
        }
    }

    /// Returns the user-facing menu label for this terminal action.
    pub fn label(&self) -> String {
        match self {
            Self::CopySelectionToClipboard => "Copy selection".to_string(),
            Self::CopyLastSubmittedPromptToClipboard { .. } => {
                "Copy last submitted prompt".to_string()
            }
            Self::CopyPreviousExactPromptToClipboard { .. } => {
                "Copy previous exact prompt".to_string()
            }
            Self::LastSubmittedPromptUnavailable => "Last submitted prompt unavailable".to_string(),
            Self::CopyLineToClipboard => "Copy line".to_string(),
            Self::CopyVisibleTerminalToClipboard => "Copy visible screen".to_string(),
            Self::CopyFullTerminalHistoryToClipboard => "Copy full history".to_string(),
            Self::CopyHistoryFilePathToClipboard { .. } => "Copy history file path".to_string(),
            Self::PasteClipboard => "Paste clipboard".to_string(),
            Self::ShowAgentDebugLog => "Show debug log".to_string(),
            Self::ToggleAgentToolbar { currently_visible } => if *currently_visible {
                "Hide agent toolbar"
            } else {
                "Show agent toolbar"
            }
            .to_string(),
            Self::OpenExternally(target) => target.menu_label().to_string(),
            Self::OpenInEditor { .. } => "Open in editor".to_string(),
            Self::Plugin { title, .. } => title.clone(),
            Self::PluginLink { text, .. } => plugin_link_label(text),
        }
    }
}

/// Longest link text shown in a plugin link entry, in characters.
const PLUGIN_LINK_LABEL_CHARACTERS: usize = 40;

/// "Open link <text>", the text sanitized and cut so the menu stays narrow.
fn plugin_link_label(text: &str) -> String {
    let shown = ilium_plugin_host::render::sanitize_plugin_text(text, PLUGIN_LINK_LABEL_CHARACTERS);
    format!("Open link {shown}")
}

/// Mouse-anchored terminal menu with the text that was visible on open.
pub struct TerminalPaneContextMenu {
    pub pane_id: NodeId,
    pub source_line_text: String,
    pub visible_contents: String,
    pub full_history: String,
    /// The pane's selected text at the moment the menu opened, if it had a
    /// non-empty selection -- see `crate::terminal_selection`.
    pub selection_text: Option<String>,
    pub area: Rect,
    pub actions: Vec<TerminalContextAction>,
    pub selected_index: usize,
    pub(crate) row_offset: usize,
    pub(crate) preparation_generation: u64,
    pub(crate) _preparation_hold:
        Option<ilium_execution::Retained<Option<ilium_execution::Retained<()>>>>,
}

impl TerminalPaneContextMenu {
    pub(crate) fn layout(&self) -> crate::context_menu_layout::MenuLayout {
        let groups: Vec<_> = self
            .actions
            .iter()
            .map(TerminalContextAction::menu_group)
            .collect();
        crate::context_menu_layout::MenuLayout::with_offset(
            &groups,
            self.selected_index,
            self.area.height.saturating_sub(2),
            self.row_offset,
        )
    }
}

#[cfg(test)]
mod plugin_link_tests {
    use super::TerminalContextAction;

    fn link(text: &str) -> TerminalContextAction {
        TerminalContextAction::PluginLink {
            plugin: "example.plugin".to_owned(),
            detector_id: "url".to_owned(),
            command: "example.plugin/open".to_owned(),
            text: text.to_owned(),
        }
    }

    #[test]
    fn a_plugin_link_entry_sits_with_the_open_entries_and_names_its_text() {
        let action = link("https://example.test/x");
        assert_eq!(action.menu_order().0, 0);
        assert_eq!(
            action.icon_target(),
            crate::icon_settings::IconTarget::OpenExternal
        );
        assert_eq!(action.label(), "Open link https://example.test/x");
    }

    #[test]
    fn a_plugin_link_label_drops_control_sequences_and_stays_short() {
        let label = link(&format!("\u{1b}[31m{}", "a".repeat(200))).label();
        assert!(!label.contains('\u{1b}'));
        assert!(label.chars().count() <= "Open link ".len() + 41);
    }
}

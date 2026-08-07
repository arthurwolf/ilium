//! Immutable terminal text captured for the terminal-pane context menu.
//!
//! A terminal screen can continue changing while its context menu is open.
//! Capturing the exact visible line and screen text at the right-click keeps a
//! copy action anchored to the user's target, just like the editor line menu.

use ilium_core::NodeId;
use ratatui::layout::Rect;

use crate::open_target::OpenTarget;
use crate::split_layout::PaneDirection;

/// Actions available from a terminal pane's right-click menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalContextAction {
    CopyLineToClipboard,
    CopyVisibleTerminalToClipboard,
    CopyFullTerminalHistoryToClipboard,
    PasteClipboard,
    PasteScreenInto {
        destination_pane_id: NodeId,
        direction: PaneDirection,
        destination_label: String,
    },
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
}

impl TerminalContextAction {
    /// Returns the shared icon role used by this terminal action.
    pub const fn icon_target(&self) -> crate::icon_settings::IconTarget {
        use crate::icon_settings::IconTarget;
        match self {
            Self::CopyLineToClipboard | Self::CopyFullTerminalHistoryToClipboard => {
                IconTarget::Editor
            }
            Self::CopyVisibleTerminalToClipboard => IconTarget::Terminal,
            Self::PasteClipboard => IconTarget::ScreenTransferDown,
            Self::PasteScreenInto { direction, .. } => match direction {
                PaneDirection::Left => IconTarget::ScreenTransferLeft,
                PaneDirection::Right => IconTarget::ScreenTransferRight,
                PaneDirection::Up => IconTarget::ScreenTransferUp,
                PaneDirection::Down => IconTarget::ScreenTransferDown,
            },
            Self::ShowAgentDebugLog => IconTarget::Done,
            Self::ToggleAgentToolbar { .. } => IconTarget::AgentToolbarConfig,
            Self::OpenExternally(OpenTarget::Directory(_)) => IconTarget::Folder,
            Self::OpenExternally(OpenTarget::Url(_) | OpenTarget::File(_)) => {
                IconTarget::OpenExternal
            }
        }
    }

    /// Returns the user-facing menu label for this terminal action.
    pub fn label(&self) -> String {
        match self {
            Self::CopyLineToClipboard => "Copy line to clipboard".to_string(),
            Self::CopyVisibleTerminalToClipboard => {
                "Copy visible terminal to clipboard".to_string()
            }
            Self::CopyFullTerminalHistoryToClipboard => "Copy full terminal history".to_string(),
            Self::PasteClipboard => "Paste clipboard".to_string(),
            Self::PasteScreenInto {
                direction,
                destination_label,
                ..
            } => format!(
                "Paste screen into {destination_label} {}",
                direction.label()
            ),
            Self::ShowAgentDebugLog => "Show debug log".to_string(),
            Self::ToggleAgentToolbar { currently_visible } => if *currently_visible {
                "Hide agent toolbar"
            } else {
                "Show agent toolbar"
            }
            .to_string(),
            Self::OpenExternally(target) => target.menu_label().to_string(),
        }
    }
}

/// Mouse-anchored terminal menu with the text that was visible on open.
pub struct TerminalPaneContextMenu {
    pub pane_id: NodeId,
    pub source_line_text: String,
    pub visible_contents: String,
    pub full_history: String,
    pub area: Rect,
    pub actions: Vec<TerminalContextAction>,
    pub selected_index: usize,
}

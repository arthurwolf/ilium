//! Immutable terminal text captured for the terminal-pane context menu.
//!
//! A terminal screen can continue changing while its context menu is open.
//! Capturing the exact visible line and screen text at the right-click keeps a
//! copy action anchored to the user's target, just like the editor line menu.

use ilium_core::NodeId;
use ratatui::layout::Rect;

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
}

impl TerminalContextAction {
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

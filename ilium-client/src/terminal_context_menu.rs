//! Immutable terminal text captured for the terminal-pane context menu.
//!
//! A terminal screen can continue changing while its context menu is open.
//! Capturing the exact visible line and screen text at the right-click keeps a
//! copy action anchored to the user's target, just like the editor line menu.

use ilium_core::NodeId;
use ratatui::layout::Rect;

/// Actions available from a terminal pane's right-click menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalContextAction {
    CopyLineToClipboard,
    CopyVisibleTerminalToClipboard,
    CopyFullTerminalHistoryToClipboard,
    PasteClipboard,
    ShowAgentDebugLog,
}

impl TerminalContextAction {
    /// Returns the user-facing menu label for this terminal action.
    pub const fn label(self) -> &'static str {
        match self {
            Self::CopyLineToClipboard => "Copy line to clipboard",
            Self::CopyVisibleTerminalToClipboard => "Copy visible terminal to clipboard",
            Self::CopyFullTerminalHistoryToClipboard => "Copy full terminal history",
            Self::PasteClipboard => "Paste clipboard",
            Self::ShowAgentDebugLog => "Show debug log",
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

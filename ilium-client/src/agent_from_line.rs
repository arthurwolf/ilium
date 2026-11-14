//! The editor source-line context menu: the file line a right-click captured, the
//! actions the menu offers for it and the mouse-anchored menu state. The editor mouse
//! handler only identifies the source line and opens the menu. "Create agent from line"
//! is the Editor plugin's command (`ilium.core.editor/create-agent-from-line`): its dialog,
//! prompt and agent choice live in `plugins/core/editor`, and the kernel only names the line.

use std::path::PathBuf;

use ilium_core::NodeId;
use ratatui::layout::Rect;

/// Immutable file context captured at the right-click position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorSourceLine {
    pub pane_id: NodeId,
    pub path: PathBuf,
    /// One-based physical line number shown to the user and sent to the agent.
    pub line_number: usize,
    pub text: String,
}

/// Actions exposed by the source-line context menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorLineContextAction {
    CopyLineToClipboard,
    /// Opens the exact regular-file target captured at the right-click cell
    /// beside the originating editor pane.
    OpenInEditor {
        path: PathBuf,
    },
    CopyChapterToClipboard,
    CopyEntireFileToClipboard,
    CreateAgentFromLine,
    /// Present only when the clicked cell resolved to an allowed-scheme URL
    /// or a path that exists on disk right now (see
    /// `crate::open_target::resolve_at`).
    OpenExternally(crate::open_target::OpenTarget),
}

impl EditorLineContextAction {
    /// Returns the shared icon role used by this editor-line action.
    pub const fn icon_target(&self) -> crate::icon_settings::IconTarget {
        use crate::icon_settings::IconTarget;
        match self {
            Self::CopyLineToClipboard
            | Self::CopyChapterToClipboard
            | Self::CopyEntireFileToClipboard
            | Self::OpenInEditor { .. } => IconTarget::Editor,
            Self::CreateAgentFromLine => IconTarget::OtherAgent,
            Self::OpenExternally(crate::open_target::OpenTarget::Directory(_)) => {
                IconTarget::Folder
            }
            Self::OpenExternally(
                crate::open_target::OpenTarget::Url(_) | crate::open_target::OpenTarget::File(_),
            ) => IconTarget::OpenExternal,
        }
    }

    pub fn label(&self) -> String {
        match self {
            Self::CopyLineToClipboard => "Copy line to clipboard".to_string(),
            Self::OpenInEditor { .. } => "Open in editor".to_string(),
            Self::CopyChapterToClipboard => "Copy chapter to clipboard".to_string(),
            Self::CopyEntireFileToClipboard => "Copy entire file to clipboard".to_string(),
            Self::CreateAgentFromLine => "Create agent from line\u{2026}".to_string(),
            Self::OpenExternally(target) => target.menu_label().to_string(),
        }
    }
}

/// Mouse-anchored source-line menu, kept separate from the tree context menu
/// because its target is a file line rather than a mutable tree node.
pub struct EditorLineContextMenu {
    pub source: EditorSourceLine,
    pub area: Rect,
    pub actions: Vec<EditorLineContextAction>,
    pub selected_index: usize,
}

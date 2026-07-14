//! State and shared geometry for creating an agent from one editor source
//! line. The editor mouse handler only identifies the source line and opens
//! the context menu; this module owns the line-derived prompt, agent registry,
//! dialog focus, and render/input hit-test rectangles so those concerns do not
//! accumulate in `app.rs`, `mouse.rs`, and `ui.rs` independently.

use std::path::{Path, PathBuf};

use ilium_core::NodeId;
use ratatui::layout::{Constraint, Direction, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui_textarea::TextArea;

/// Agent CLIs that the create-from-line dialog can launch today.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentLaunchType {
    Claude,
    Codex,
}

impl AgentLaunchType {
    // TODO: Add future supported agent CLIs here; rendering, selection, and
    // launch behavior all walk this registry rather than branching elsewhere.
    pub const ALL: [Self; 2] = [Self::Claude, Self::Codex];

    /// Human-readable selector label.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Claude => "Claude",
            Self::Codex => "Codex",
        }
    }

    /// Literal command line handed to the server's command-pane adapter.
    pub const fn command_line(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }

    /// Selects the adjacent registry entry, wrapping at both ends.
    pub fn stepped(self, direction: i32) -> Self {
        let index = Self::ALL
            .iter()
            .position(|candidate| *candidate == self)
            .unwrap_or(0) as i32;
        let count = Self::ALL.len() as i32;
        Self::ALL[(index + direction.signum()).rem_euclid(count) as usize]
    }
}

/// Which control receives keyboard input in the creation dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreateAgentFocus {
    AgentType,
    Prompt,
    CreateButton,
}

impl CreateAgentFocus {
    /// Moves focus forward through the selector, textarea, and submit button.
    pub const fn next(self) -> Self {
        match self {
            Self::AgentType => Self::Prompt,
            Self::Prompt => Self::CreateButton,
            Self::CreateButton => Self::AgentType,
        }
    }

    /// Moves focus backward through the same stable control order.
    pub const fn previous(self) -> Self {
        match self {
            Self::AgentType => Self::CreateButton,
            Self::Prompt => Self::AgentType,
            Self::CreateButton => Self::Prompt,
        }
    }
}

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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorLineContextAction {
    CreateAgentFromLine,
}

impl EditorLineContextAction {
    pub const fn label(self) -> &'static str {
        match self {
            Self::CreateAgentFromLine => "Create agent from line\u{2026}",
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

/// All state required to render, edit, and commit the creation dialog.
pub struct CreateAgentFromLineState {
    pub source: EditorSourceLine,
    /// The new pane is created beside the originating editor in this container.
    pub parent_group: NodeId,
    pub agent_type: AgentLaunchType,
    pub prompt: TextArea<'static>,
    pub focus: CreateAgentFocus,
}

impl CreateAgentFromLineState {
    /// Builds the dialog with Claude selected and the requested contextual
    /// `/goal` prompt ready for editing.
    pub fn new(source: EditorSourceLine, parent_group: NodeId) -> Self {
        let default_prompt = default_agent_prompt(&source.path, source.line_number, &source.text);
        let mut prompt = TextArea::from([default_prompt]);
        // A neutral current-line style avoids painting a light full-width bar
        // inside the dark modal; the block border communicates focus instead.
        prompt.set_cursor_line_style(Style::new());
        prompt.set_cursor_style(
            Style::new()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        );

        Self {
            source,
            parent_group,
            agent_type: AgentLaunchType::Claude,
            prompt,
            // The textarea starts active so the prefilled instruction can be
            // edited immediately; the selector remains one Tab/Shift-Tab away.
            focus: CreateAgentFocus::Prompt,
        }
    }

    /// Returns the exact textarea contents, preserving user-authored newlines.
    pub fn prompt_text(&self) -> String {
        self.prompt.lines().join("\n")
    }
}

/// Produces the requested contextual instruction from a physical file line.
pub fn default_agent_prompt(path: &Path, line_number: usize, line_text: &str) -> String {
    format!(
        "/goal please do the following task: \"{line_text}\", note this text comes from the file {} at line {line_number} in case this can help you gather more context",
        path.display()
    )
}

/// Shared dialog geometry for rendering and mouse hit testing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CreateAgentFromLineLayout {
    pub popup: Rect,
    pub agent_row: Rect,
    pub prompt_area: Rect,
    pub create_button: Rect,
    pub hint_row: Rect,
}

/// Centers a responsive dialog and assigns stable rows to its controls.
pub fn dialog_layout(screen_area: Rect) -> CreateAgentFromLineLayout {
    let popup = crate::modal::centered_fixed_rect(88, 16, screen_area);
    let inner = Rect::new(
        popup.x.saturating_add(1),
        popup.y.saturating_add(1),
        popup.width.saturating_sub(2),
        popup.height.saturating_sub(2),
    );
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(3),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(inner);

    CreateAgentFromLineLayout {
        popup,
        agent_row: rows[0],
        prompt_area: rows[2],
        create_button: rows[4],
        hint_row: rows[5],
    }
}

/// Size-only entry point used by PTY integration tests and other callers that
/// do not otherwise need to construct Ratatui geometry themselves.
pub fn dialog_layout_for_size(width: u16, height: u16) -> CreateAgentFromLineLayout {
    dialog_layout(Rect::new(0, 0, width, height))
}

/// Maps a selector-row click to its agent registry entry.
pub fn agent_type_at(area: Rect, position: Position) -> Option<AgentLaunchType> {
    if !area.contains(position) {
        return None;
    }
    let relative_column = position.x.saturating_sub(area.x);
    match relative_column {
        7..=16 => Some(AgentLaunchType::Claude),
        20..=28 => Some(AgentLaunchType::Codex),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_prompt_contains_line_file_and_one_based_line_number() {
        let prompt = default_agent_prompt(Path::new("/work/src/main.rs"), 42, "fix_this();");

        assert_eq!(
            prompt,
            "/goal please do the following task: \"fix_this();\", note this text comes from the file /work/src/main.rs at line 42 in case this can help you gather more context"
        );
    }

    #[test]
    fn agent_registry_steps_both_directions_and_wraps() {
        assert_eq!(AgentLaunchType::Claude.stepped(1), AgentLaunchType::Codex);
        assert_eq!(AgentLaunchType::Claude.stepped(-1), AgentLaunchType::Codex);
        assert_eq!(AgentLaunchType::Codex.stepped(1), AgentLaunchType::Claude);
    }

    #[test]
    fn dialog_layout_stays_inside_small_screens() {
        let screen = Rect::new(0, 0, 30, 8);
        let layout = dialog_layout(screen);

        assert!(screen.contains(layout.popup.as_position()));
        assert!(layout.popup.right() <= screen.right());
        assert!(layout.popup.bottom() <= screen.bottom());
    }
}

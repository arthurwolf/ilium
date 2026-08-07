//! Global icon vocabulary shared by the tree renderer and the Settings UI.
//!
//! This module deliberately stores glyphs as UTF-8 strings rather than a
//! closed enum: the curated quick choices stay useful, while the full picker
//! can legitimately select any catalogued symbol without requiring a client
//! release for each new choice.

use std::collections::BTreeMap;
use std::sync::OnceLock;

/// Every independently configurable semantic icon rendered by the sidebar,
/// including its creation toolbar and per-row action controls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IconTarget {
    Group,
    TopLevel,
    Project,
    SplitVertical,
    SplitHorizontal,
    Folder,
    Terminal,
    Editor,
    Board,
    Claude,
    Codex,
    Antigravity,
    OtherAgent,
    Working,
    WaitingBackground,
    WaitingApproval,
    Done,
    Idle,
    Goal,
    Bookmark,
    ToolbarSearch,
    ToolbarRestructure,
    ToolbarSettings,
    RowRename,
    RowMoveUp,
    RowMoveDown,
    RowClose,
    RowRetitle,
    RowProjectRestructure,
    ScreenTransferLeft,
    ScreenTransferRight,
    ScreenTransferUp,
    ScreenTransferDown,
    OpenExternal,
    AgentToolbarClose,
    AgentToolbarCompact,
    AgentToolbarClear,
    AgentToolbarConfig,
    AgentToolbarStop,
    AgentToolbarCopyScreen,
    AgentToolbarCopyLastMessage,
    AgentToolbarEffort,
    AgentToolbarModel,
    AgentToolbarExit,
    AgentToolbarFast,
}

impl IconTarget {
    pub const ALL: [Self; 45] = [
        Self::Group,
        Self::TopLevel,
        Self::Project,
        Self::SplitVertical,
        Self::SplitHorizontal,
        Self::Folder,
        Self::Terminal,
        Self::Editor,
        Self::Board,
        Self::Claude,
        Self::Codex,
        Self::Antigravity,
        Self::OtherAgent,
        Self::Working,
        Self::WaitingBackground,
        Self::WaitingApproval,
        Self::Done,
        Self::Idle,
        Self::Goal,
        Self::Bookmark,
        Self::ToolbarSearch,
        Self::ToolbarRestructure,
        Self::ToolbarSettings,
        Self::RowRename,
        Self::RowMoveUp,
        Self::RowMoveDown,
        Self::RowClose,
        Self::RowRetitle,
        Self::RowProjectRestructure,
        Self::ScreenTransferLeft,
        Self::ScreenTransferRight,
        Self::ScreenTransferUp,
        Self::ScreenTransferDown,
        Self::OpenExternal,
        Self::AgentToolbarClose,
        Self::AgentToolbarCompact,
        Self::AgentToolbarClear,
        Self::AgentToolbarConfig,
        Self::AgentToolbarStop,
        Self::AgentToolbarCopyScreen,
        Self::AgentToolbarCopyLastMessage,
        Self::AgentToolbarEffort,
        Self::AgentToolbarModel,
        Self::AgentToolbarExit,
        Self::AgentToolbarFast,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Group => "Group",
            Self::TopLevel => "Top-level destination",
            Self::Project => "Project",
            Self::SplitVertical => "Vertical split",
            Self::SplitHorizontal => "Horizontal split",
            Self::Folder => "Folder",
            Self::Terminal => "Terminal",
            Self::Editor => "Editor",
            Self::Board => "Board",
            Self::Claude => "Claude agent",
            Self::Codex => "Codex agent",
            Self::Antigravity => "Antigravity agent",
            Self::OtherAgent => "Other agent",
            Self::Working => "Working",
            Self::WaitingBackground => "Background wait",
            Self::WaitingApproval => "Needs approval",
            Self::Done => "Done",
            Self::Idle => "Idle",
            Self::Goal => "Goal attached",
            Self::Bookmark => "Bookmarked tree item",
            Self::ToolbarSearch => "Toolbar: search",
            Self::ToolbarRestructure => "Toolbar: restructure",
            Self::ToolbarSettings => "Toolbar: settings",
            Self::RowRename => "Row action: rename",
            Self::RowMoveUp => "Row action: move up",
            Self::RowMoveDown => "Row action: move down",
            Self::RowClose => "Row action: close",
            Self::RowRetitle => "Row action: retitle",
            Self::RowProjectRestructure => "Row action: restructure project",
            Self::ScreenTransferLeft => "Screen transfer: paste left",
            Self::ScreenTransferRight => "Screen transfer: paste right",
            Self::ScreenTransferUp => "Screen transfer: paste up",
            Self::ScreenTransferDown => "Screen transfer: paste down",
            Self::OpenExternal => "Open in OS (browser/file/folder)",
            Self::AgentToolbarClose => "Agent toolbar: close",
            Self::AgentToolbarCompact => "Agent toolbar: compact",
            Self::AgentToolbarClear => "Agent toolbar: clear",
            Self::AgentToolbarConfig => "Agent toolbar: config",
            Self::AgentToolbarStop => "Agent toolbar: stop",
            Self::AgentToolbarCopyScreen => "Agent toolbar: copy screen",
            Self::AgentToolbarCopyLastMessage => "Agent toolbar: copy last message",
            Self::AgentToolbarEffort => "Agent toolbar: effort",
            Self::AgentToolbarModel => "Agent toolbar: model",
            Self::AgentToolbarExit => "Agent toolbar: exit",
            Self::AgentToolbarFast => "Agent toolbar: fast",
        }
    }

    pub const fn key(self) -> &'static str {
        match self {
            Self::Group => "group",
            Self::TopLevel => "top_level",
            Self::Project => "project",
            Self::SplitVertical => "split_vertical",
            Self::SplitHorizontal => "split_horizontal",
            Self::Folder => "folder",
            Self::Terminal => "terminal",
            Self::Editor => "editor",
            Self::Board => "board",
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Antigravity => "antigravity",
            Self::OtherAgent => "other_agent",
            Self::Working => "working",
            Self::WaitingBackground => "waiting_background",
            Self::WaitingApproval => "waiting_approval",
            Self::Done => "done",
            Self::Idle => "idle",
            Self::Goal => "goal",
            Self::Bookmark => "bookmark",
            Self::ToolbarSearch => "toolbar_search",
            Self::ToolbarRestructure => "toolbar_restructure",
            Self::ToolbarSettings => "toolbar_settings",
            Self::RowRename => "row_rename",
            Self::RowMoveUp => "row_move_up",
            Self::RowMoveDown => "row_move_down",
            Self::RowClose => "row_close",
            Self::RowRetitle => "row_retitle",
            Self::RowProjectRestructure => "row_project_restructure",
            Self::ScreenTransferLeft => "screen_transfer_left",
            Self::ScreenTransferRight => "screen_transfer_right",
            Self::ScreenTransferUp => "screen_transfer_up",
            Self::ScreenTransferDown => "screen_transfer_down",
            Self::OpenExternal => "open_external",
            Self::AgentToolbarClose => "agent_toolbar_close",
            Self::AgentToolbarCompact => "agent_toolbar_compact",
            Self::AgentToolbarClear => "agent_toolbar_clear",
            Self::AgentToolbarConfig => "agent_toolbar_config",
            Self::AgentToolbarStop => "agent_toolbar_stop",
            Self::AgentToolbarCopyScreen => "agent_toolbar_copy_screen",
            Self::AgentToolbarCopyLastMessage => "agent_toolbar_copy_last_message",
            Self::AgentToolbarEffort => "agent_toolbar_effort",
            Self::AgentToolbarModel => "agent_toolbar_model",
            Self::AgentToolbarExit => "agent_toolbar_exit",
            Self::AgentToolbarFast => "agent_toolbar_fast",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|target| target.key() == key)
    }

    pub const fn default_glyph(self) -> &'static str {
        match self {
            Self::Group => "📁",
            Self::TopLevel => "⌂",
            Self::Project => "🗂️",
            Self::SplitVertical => "▥",
            Self::SplitHorizontal => "▤",
            Self::Folder => "📂",
            Self::Terminal => "📟",
            Self::Editor => "🗒️",
            Self::Board => "▦",
            Self::Claude => "🧠",
            Self::Codex => "⚙️",
            Self::Antigravity => "✦",
            Self::OtherAgent => "🤖",
            Self::Working => "⠋",
            Self::WaitingBackground => "◷",
            Self::WaitingApproval => "?",
            Self::Done => "🔔",
            Self::Idle => "●",
            Self::Goal => "🏁",
            Self::Bookmark => "★",
            Self::ToolbarSearch => "⌕",
            Self::ToolbarRestructure => "♻️",
            Self::ToolbarSettings => "🎚️",
            Self::RowRename => "✏️",
            Self::RowMoveUp => "🔼",
            Self::RowMoveDown => "🔽",
            Self::RowClose => "🚫",
            Self::RowRetitle => "♻️",
            Self::RowProjectRestructure => "♻️",
            Self::ScreenTransferLeft => "←",
            Self::ScreenTransferRight => "→",
            Self::ScreenTransferUp => "↑",
            Self::ScreenTransferDown => "↓",
            Self::OpenExternal => "🔗",
            Self::AgentToolbarClose => "✕",
            Self::AgentToolbarCompact => "🗜️",
            Self::AgentToolbarClear => "🧹",
            Self::AgentToolbarConfig => "🎛️",
            Self::AgentToolbarStop => "⏹",
            Self::AgentToolbarCopyScreen => "📋",
            Self::AgentToolbarCopyLastMessage => "💬",
            Self::AgentToolbarEffort => "🎚️",
            Self::AgentToolbarModel => "🧩",
            Self::AgentToolbarExit => "🚪",
            Self::AgentToolbarFast => "⚡",
        }
    }

    pub const fn suggestions(self) -> &'static [&'static str] {
        match self {
            Self::Group | Self::Project | Self::Folder => &["📁", "📂", "🗂️", "🧺"],
            Self::TopLevel => &["⌂", "⌘", "🏠", "◆"],
            Self::SplitVertical | Self::SplitHorizontal => &["▥", "▤", "⊞", "▦"],
            Self::Terminal => &["📟", "▸", "⌘", "🖥️"],
            Self::Editor => &["🗒️", "📝", "✎", "📄"],
            Self::Board => &["▦", "▤", "☷", "🗃️"],
            Self::Claude => &["🧠", "🪄", "🧭", "🦀"],
            Self::Codex => &["⚙️", "🛠️", "🧬", "📖"],
            Self::Antigravity => &["✦", "🛰️", "🪐", "⚛️"],
            Self::OtherAgent => &["🤖", "✦", "◈", "◆"],
            Self::Working => &["⠋", "◌", "◒", "✦"],
            Self::WaitingBackground => &["◷", "◴", "⌛", "⏳"],
            Self::WaitingApproval => &["?", "!", "⚑", "✋"],
            Self::Done => &["🔔", "✓", "●", "✦"],
            Self::Idle => &["●", "·", "○", "—"],
            Self::Goal => &["🏁", "⚑", "◆", "✦"],
            Self::Bookmark => &["★", "☆", "🔖", "📌"],
            Self::ToolbarSearch => &["⌕", "🔎", "🔍", "◉"],
            Self::ToolbarRestructure | Self::RowRetitle | Self::RowProjectRestructure => {
                &["♻️", "↻", "⟳", "✦"]
            }
            Self::ToolbarSettings => &["🎚️", "⚙️", "🔧", "☷"],
            Self::RowRename => &["✏️", "✎", "📝", "🖊️"],
            Self::RowMoveUp => &["🔼", "↑", "⬆️", "⤴️"],
            Self::RowMoveDown => &["🔽", "↓", "⬇️", "⤵️"],
            Self::RowClose => &["🚫", "×", "✕", "⛔"],
            Self::ScreenTransferLeft => &["←", "⇐", "⬅️", "◀"],
            Self::ScreenTransferRight => &["→", "⇒", "➡️", "▶"],
            Self::ScreenTransferUp => &["↑", "⇑", "⬆️", "▲"],
            Self::ScreenTransferDown => &["↓", "⇓", "⬇️", "▼"],
            Self::OpenExternal => &["🔗", "↗️", "🌐", "📤"],
            Self::AgentToolbarClose => &["✕", "×", "✗", "⨯"],
            Self::AgentToolbarCompact => &["🗜️", "📦", "🤏", "📉"],
            Self::AgentToolbarClear => &["🧹", "🗑️", "♻️", "⌫"],
            Self::AgentToolbarConfig => &["🎛️", "⚙️", "🔧", "🛠️"],
            Self::AgentToolbarStop => &["⏹", "✋", "🛑", "⏸"],
            Self::AgentToolbarCopyScreen => &["📋", "🖥️", "📸", "🗂️"],
            Self::AgentToolbarCopyLastMessage => &["💬", "📨", "✉️", "🗨️"],
            Self::AgentToolbarEffort => &["🎚️", "⚖️", "📶", "🌡️"],
            Self::AgentToolbarModel => &["🧩", "🧠", "🤖", "✦"],
            Self::AgentToolbarExit => &["🚪", "⏏️", "👋", "⛔"],
            Self::AgentToolbarFast => &["⚡", "🚀", "💨", "🏃"],
        }
    }
}

/// The persisted, globally shared icon choices. All fields remain public so
/// the renderer has a simple, allocation-free lookup path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IconSettings {
    pub group: String,
    pub top_level: String,
    pub project: String,
    pub split_vertical: String,
    pub split_horizontal: String,
    pub folder: String,
    pub terminal: String,
    pub editor: String,
    pub board: String,
    pub claude: String,
    pub codex: String,
    pub antigravity: String,
    pub other_agent: String,
    pub working: String,
    pub waiting_background: String,
    pub waiting_approval: String,
    pub done: String,
    pub idle: String,
    pub goal: String,
    pub bookmark: String,
    pub toolbar_search: String,
    pub toolbar_restructure: String,
    pub toolbar_settings: String,
    pub row_rename: String,
    pub row_move_up: String,
    pub row_move_down: String,
    pub row_close: String,
    pub row_retitle: String,
    pub row_project_restructure: String,
    pub screen_transfer_left: String,
    pub screen_transfer_right: String,
    pub screen_transfer_up: String,
    pub screen_transfer_down: String,
    pub open_external: String,
    pub agent_toolbar_close: String,
    pub agent_toolbar_compact: String,
    pub agent_toolbar_clear: String,
    pub agent_toolbar_config: String,
    pub agent_toolbar_stop: String,
    pub agent_toolbar_copy_screen: String,
    pub agent_toolbar_copy_last_message: String,
    pub agent_toolbar_effort: String,
    pub agent_toolbar_model: String,
    pub agent_toolbar_exit: String,
    pub agent_toolbar_fast: String,
}

impl Default for IconSettings {
    fn default() -> Self {
        Self::from_target(|target| target.default_glyph().to_string())
    }
}

impl IconSettings {
    pub fn from_target(mut value: impl FnMut(IconTarget) -> String) -> Self {
        Self {
            group: value(IconTarget::Group),
            top_level: value(IconTarget::TopLevel),
            project: value(IconTarget::Project),
            split_vertical: value(IconTarget::SplitVertical),
            split_horizontal: value(IconTarget::SplitHorizontal),
            folder: value(IconTarget::Folder),
            terminal: value(IconTarget::Terminal),
            editor: value(IconTarget::Editor),
            board: value(IconTarget::Board),
            claude: value(IconTarget::Claude),
            codex: value(IconTarget::Codex),
            antigravity: value(IconTarget::Antigravity),
            other_agent: value(IconTarget::OtherAgent),
            working: value(IconTarget::Working),
            waiting_background: value(IconTarget::WaitingBackground),
            waiting_approval: value(IconTarget::WaitingApproval),
            done: value(IconTarget::Done),
            idle: value(IconTarget::Idle),
            goal: value(IconTarget::Goal),
            bookmark: value(IconTarget::Bookmark),
            toolbar_search: value(IconTarget::ToolbarSearch),
            toolbar_restructure: value(IconTarget::ToolbarRestructure),
            toolbar_settings: value(IconTarget::ToolbarSettings),
            row_rename: value(IconTarget::RowRename),
            row_move_up: value(IconTarget::RowMoveUp),
            row_move_down: value(IconTarget::RowMoveDown),
            row_close: value(IconTarget::RowClose),
            row_retitle: value(IconTarget::RowRetitle),
            row_project_restructure: value(IconTarget::RowProjectRestructure),
            screen_transfer_left: value(IconTarget::ScreenTransferLeft),
            screen_transfer_right: value(IconTarget::ScreenTransferRight),
            screen_transfer_up: value(IconTarget::ScreenTransferUp),
            screen_transfer_down: value(IconTarget::ScreenTransferDown),
            open_external: value(IconTarget::OpenExternal),
            agent_toolbar_close: value(IconTarget::AgentToolbarClose),
            agent_toolbar_compact: value(IconTarget::AgentToolbarCompact),
            agent_toolbar_clear: value(IconTarget::AgentToolbarClear),
            agent_toolbar_config: value(IconTarget::AgentToolbarConfig),
            agent_toolbar_stop: value(IconTarget::AgentToolbarStop),
            agent_toolbar_copy_screen: value(IconTarget::AgentToolbarCopyScreen),
            agent_toolbar_copy_last_message: value(IconTarget::AgentToolbarCopyLastMessage),
            agent_toolbar_effort: value(IconTarget::AgentToolbarEffort),
            agent_toolbar_model: value(IconTarget::AgentToolbarModel),
            agent_toolbar_exit: value(IconTarget::AgentToolbarExit),
            agent_toolbar_fast: value(IconTarget::AgentToolbarFast),
        }
    }

    pub fn glyph(&self, target: IconTarget) -> &str {
        match target {
            IconTarget::Group => &self.group,
            IconTarget::TopLevel => &self.top_level,
            IconTarget::Project => &self.project,
            IconTarget::SplitVertical => &self.split_vertical,
            IconTarget::SplitHorizontal => &self.split_horizontal,
            IconTarget::Folder => &self.folder,
            IconTarget::Terminal => &self.terminal,
            IconTarget::Editor => &self.editor,
            IconTarget::Board => &self.board,
            IconTarget::Claude => &self.claude,
            IconTarget::Codex => &self.codex,
            IconTarget::Antigravity => &self.antigravity,
            IconTarget::OtherAgent => &self.other_agent,
            IconTarget::Working => &self.working,
            IconTarget::WaitingBackground => &self.waiting_background,
            IconTarget::WaitingApproval => &self.waiting_approval,
            IconTarget::Done => &self.done,
            IconTarget::Idle => &self.idle,
            IconTarget::Goal => &self.goal,
            IconTarget::Bookmark => &self.bookmark,
            IconTarget::ToolbarSearch => &self.toolbar_search,
            IconTarget::ToolbarRestructure => &self.toolbar_restructure,
            IconTarget::ToolbarSettings => &self.toolbar_settings,
            IconTarget::RowRename => &self.row_rename,
            IconTarget::RowMoveUp => &self.row_move_up,
            IconTarget::RowMoveDown => &self.row_move_down,
            IconTarget::RowClose => &self.row_close,
            IconTarget::RowRetitle => &self.row_retitle,
            IconTarget::RowProjectRestructure => &self.row_project_restructure,
            IconTarget::ScreenTransferLeft => &self.screen_transfer_left,
            IconTarget::ScreenTransferRight => &self.screen_transfer_right,
            IconTarget::ScreenTransferUp => &self.screen_transfer_up,
            IconTarget::ScreenTransferDown => &self.screen_transfer_down,
            IconTarget::OpenExternal => &self.open_external,
            IconTarget::AgentToolbarClose => &self.agent_toolbar_close,
            IconTarget::AgentToolbarCompact => &self.agent_toolbar_compact,
            IconTarget::AgentToolbarClear => &self.agent_toolbar_clear,
            IconTarget::AgentToolbarConfig => &self.agent_toolbar_config,
            IconTarget::AgentToolbarStop => &self.agent_toolbar_stop,
            IconTarget::AgentToolbarCopyScreen => &self.agent_toolbar_copy_screen,
            IconTarget::AgentToolbarCopyLastMessage => &self.agent_toolbar_copy_last_message,
            IconTarget::AgentToolbarEffort => &self.agent_toolbar_effort,
            IconTarget::AgentToolbarModel => &self.agent_toolbar_model,
            IconTarget::AgentToolbarExit => &self.agent_toolbar_exit,
            IconTarget::AgentToolbarFast => &self.agent_toolbar_fast,
        }
    }

    pub fn set(&mut self, target: IconTarget, glyph: String) {
        let slot = match target {
            IconTarget::Group => &mut self.group,
            IconTarget::TopLevel => &mut self.top_level,
            IconTarget::Project => &mut self.project,
            IconTarget::SplitVertical => &mut self.split_vertical,
            IconTarget::SplitHorizontal => &mut self.split_horizontal,
            IconTarget::Folder => &mut self.folder,
            IconTarget::Terminal => &mut self.terminal,
            IconTarget::Editor => &mut self.editor,
            IconTarget::Board => &mut self.board,
            IconTarget::Claude => &mut self.claude,
            IconTarget::Codex => &mut self.codex,
            IconTarget::Antigravity => &mut self.antigravity,
            IconTarget::OtherAgent => &mut self.other_agent,
            IconTarget::Working => &mut self.working,
            IconTarget::WaitingBackground => &mut self.waiting_background,
            IconTarget::WaitingApproval => &mut self.waiting_approval,
            IconTarget::Done => &mut self.done,
            IconTarget::Idle => &mut self.idle,
            IconTarget::Goal => &mut self.goal,
            IconTarget::Bookmark => &mut self.bookmark,
            IconTarget::ToolbarSearch => &mut self.toolbar_search,
            IconTarget::ToolbarRestructure => &mut self.toolbar_restructure,
            IconTarget::ToolbarSettings => &mut self.toolbar_settings,
            IconTarget::RowRename => &mut self.row_rename,
            IconTarget::RowMoveUp => &mut self.row_move_up,
            IconTarget::RowMoveDown => &mut self.row_move_down,
            IconTarget::RowClose => &mut self.row_close,
            IconTarget::RowRetitle => &mut self.row_retitle,
            IconTarget::RowProjectRestructure => &mut self.row_project_restructure,
            IconTarget::ScreenTransferLeft => &mut self.screen_transfer_left,
            IconTarget::ScreenTransferRight => &mut self.screen_transfer_right,
            IconTarget::ScreenTransferUp => &mut self.screen_transfer_up,
            IconTarget::ScreenTransferDown => &mut self.screen_transfer_down,
            IconTarget::OpenExternal => &mut self.open_external,
            IconTarget::AgentToolbarClose => &mut self.agent_toolbar_close,
            IconTarget::AgentToolbarCompact => &mut self.agent_toolbar_compact,
            IconTarget::AgentToolbarClear => &mut self.agent_toolbar_clear,
            IconTarget::AgentToolbarConfig => &mut self.agent_toolbar_config,
            IconTarget::AgentToolbarStop => &mut self.agent_toolbar_stop,
            IconTarget::AgentToolbarCopyScreen => &mut self.agent_toolbar_copy_screen,
            IconTarget::AgentToolbarCopyLastMessage => &mut self.agent_toolbar_copy_last_message,
            IconTarget::AgentToolbarEffort => &mut self.agent_toolbar_effort,
            IconTarget::AgentToolbarModel => &mut self.agent_toolbar_model,
            IconTarget::AgentToolbarExit => &mut self.agent_toolbar_exit,
            IconTarget::AgentToolbarFast => &mut self.agent_toolbar_fast,
        };
        *slot = glyph;
    }
}

/// One selectable glyph and the words used to find it in the picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IconCatalogEntry {
    pub glyph: &'static str,
    pub name: &'static str,
}

/// A detailed Unicode CLDR subgroup. The official data gives the picker more
/// than a hundred useful categories (for example Animals · Birds, Computer,
/// Mathematics, Warning and Country flags), rather than one unwieldy list.
#[derive(Debug)]
pub struct IconCategory {
    pub label: &'static str,
    pub family: IconCatalogFamily,
    pub entries: Vec<IconCatalogEntry>,
}

/// Rendering contract for the two intentionally separate catalogue sections.
/// Official glyphs are portable UTF-8; Nerd Font glyphs live in Unicode's
/// Private Use Area and require a Nerd Font terminal to render as designed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconCatalogFamily {
    OfficialUtf8,
    NerdFont,
}

impl IconCatalogFamily {
    pub const fn label(self) -> &'static str {
        match self {
            Self::OfficialUtf8 => "Official UTF-8",
            Self::NerdFont => "Nerd Font",
        }
    }
}

/// Returns the complete selectable catalogue. It combines the full English
/// Unicode emoji data set, the compact terminal symbols used by ilium, and
/// the complete Nerd Font icon ranges installed on the developer machine.
///
/// The `emoji` crate owns the Unicode data and its CLDR group/subgroup/name
/// metadata. Nerd Font ranges are Unicode Private Use Area contracts, so we
/// generate their glyphs once here instead of embedding a brittle 10,000-line
/// source fixture. A Nerd Font terminal renders those categories as the
/// intended developer icons; every standard Unicode choice remains portable.
pub fn icon_categories() -> &'static [IconCategory] {
    static CATALOGUE: OnceLock<Vec<IconCategory>> = OnceLock::new();
    CATALOGUE.get_or_init(build_icon_categories)
}

/// One visible chapter in the picker document. The picker owns no duplicate
/// category state: a query simply removes chapters that have no matching
/// entries, preserving the catalogue's official-first ordering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IconPickerChapter {
    pub label: &'static str,
    pub family: IconCatalogFamily,
    pub entries: Vec<IconCatalogEntry>,
}

/// Cached search result owned by an open picker. Filtering 12,000 choices is
/// intentionally done only when the query changes, never while a user moves
/// through the viewport or the terminal redraws an animation frame.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct IconPickerSearchResults {
    pub chapters: Vec<IconPickerChapter>,
    pub entry_count: usize,
}

/// One ranked result from the dense-vector index. The score itself remains an
/// implementation detail of the worker; ordering is already semantic when
/// this reaches the presentation layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IconSemanticSearchHit {
    pub category_label: &'static str,
    pub family: IconCatalogFamily,
    pub entry: IconCatalogEntry,
}

impl IconPickerSearchResults {
    pub fn entry(&self, selected_entry: usize) -> Option<IconCatalogEntry> {
        self.chapters
            .iter()
            .flat_map(|chapter| chapter.entries.iter().copied())
            .nth(selected_entry)
    }
}

/// Returns the unfiltered catalogue as one category-preserving document.
/// Non-empty queries never flow through this function: they are ranked by
/// `icon_search_workers`' CPU dense-vector index instead.
pub fn all_picker_search_results() -> IconPickerSearchResults {
    let chapters = icon_categories()
        .iter()
        .map(|category| IconPickerChapter {
            label: category.label,
            family: category.family,
            entries: category.entries.clone(),
        })
        .collect::<Vec<_>>();
    let entry_count = chapters.iter().map(|chapter| chapter.entries.len()).sum();
    IconPickerSearchResults {
        chapters,
        entry_count,
    }
}

/// Builds a chaptered document exclusively from dense-vector hits. Hits are
/// ranked before this function receives them; grouping only preserves the
/// visible official-UTF-8-then-Nerd-Font boundary required by the picker.
pub fn semantic_picker_search_results(hits: Vec<IconSemanticSearchHit>) -> IconPickerSearchResults {
    let mut chapters = Vec::<IconPickerChapter>::new();
    for hit in hits {
        if let Some(chapter) = chapters
            .iter_mut()
            .find(|chapter| chapter.label == hit.category_label && chapter.family == hit.family)
        {
            chapter.entries.push(hit.entry);
        } else {
            chapters.push(IconPickerChapter {
                label: hit.category_label,
                family: hit.family,
                entries: vec![hit.entry],
            });
        }
    }
    let entry_count = chapters.iter().map(|chapter| chapter.entries.len()).sum();
    IconPickerSearchResults {
        chapters,
        entry_count,
    }
}

/// Counts actual choices, not categories, for an honest picker status line.
pub fn catalogue_icon_count() -> usize {
    icon_categories()
        .iter()
        .map(|category| category.entries.len())
        .sum()
}

/// Counts the selectable choices in one deliberate family boundary.
pub fn catalogue_icon_count_for(family: IconCatalogFamily) -> usize {
    icon_categories()
        .iter()
        .filter(|category| category.family == family)
        .map(|category| category.entries.len())
        .sum()
}

/// Builds the category list from the official Unicode CLDR classifications.
fn build_icon_categories() -> Vec<IconCategory> {
    let mut official_grouped = BTreeMap::<&'static str, Vec<IconCatalogEntry>>::new();
    for emoji in emoji::lookup_by_name::iter_emoji() {
        if !matches!(emoji.status, emoji::Status::FullyQualified) {
            continue;
        }
        official_grouped
            .entry(category_label(emoji.subgroup))
            .or_default()
            .push(IconCatalogEntry {
                glyph: emoji.glyph,
                name: emoji.name,
            });
    }
    let mut categories = vec![IconCategory {
        label: "Quick picks",
        family: IconCatalogFamily::OfficialUtf8,
        entries: QUICK_PICK_GLYPHS
            .iter()
            .map(|(glyph, name)| IconCatalogEntry { glyph, name })
            .collect(),
    }];
    categories.extend(official_grouped.into_iter().map(|(label, mut entries)| {
        entries.sort_unstable_by_key(|entry| entry.name);
        IconCategory {
            label,
            family: IconCatalogFamily::OfficialUtf8,
            entries,
        }
    }));
    categories.extend(nerd_font_icon_categories());
    categories
}

/// Adds the major Nerd Font v3 icon families. These are intentionally kept as
/// separate categories: a user looking for a coding glyph should not have to
/// sift through Material symbols, weather icons, or Powerline separators.
fn nerd_font_icon_categories() -> Vec<IconCategory> {
    const RANGES: &[(&str, &str, u32, u32)] = &[
        ("Nerd Font · Codicons", "Codicon", 0xEA60, 0xEC1E),
        ("Nerd Font · Devicons", "Devicon", 0xE700, 0xE7C5),
        ("Nerd Font · Font Awesome", "Font Awesome", 0xF000, 0xF2FF),
        (
            "Nerd Font · Material Design",
            "Material Design",
            0xF0001,
            0xF1AF0,
        ),
        ("Nerd Font · Octicons", "Octicon", 0xF400, 0xF4A8),
        ("Nerd Font · Powerline", "Powerline", 0xE0A0, 0xE0D7),
        ("Nerd Font · Seti UI", "Seti UI", 0xE5FA, 0xE62B),
        ("Nerd Font · Weather", "Weather", 0xE300, 0xE3E3),
    ];

    RANGES
        .iter()
        .map(|(label, family, start, end)| {
            let mut entries = Vec::with_capacity((end - start + 1) as usize);
            for codepoint in *start..=*end {
                let Some(glyph) = char::from_u32(codepoint) else {
                    continue;
                };
                entries.push(IconCatalogEntry {
                    glyph: Box::leak(glyph.to_string().into_boxed_str()),
                    name: Box::leak(format!("{family} icon U+{codepoint:04X}").into_boxed_str()),
                });
            }
            IconCategory {
                label,
                family: IconCatalogFamily::NerdFont,
                entries,
            }
        })
        .collect()
}

/// Existing carefully chosen terminal-friendly symbols remain available even
/// though they are not all fully-qualified emoji.
const QUICK_PICK_GLYPHS: &[(&str, &str)] = &[
    ("📁", "folder"),
    ("📂", "open folder"),
    ("🗂️", "card index"),
    ("▥", "vertical split"),
    ("▤", "horizontal split"),
    ("⊞", "squared plus"),
    ("▦", "grid"),
    ("☷", "trigram"),
    ("📟", "pager"),
    ("🖥️", "computer"),
    ("⌘", "command key"),
    ("▸", "right triangle"),
    ("📝", "memo"),
    ("✎", "pencil"),
    ("📄", "document"),
    ("🗃️", "card file"),
    ("🧠", "brain"),
    ("🪄", "magic wand"),
    ("🧭", "compass"),
    ("🦀", "crab"),
    ("🦞", "lobster"),
    ("⚙️", "gear"),
    ("🛠️", "tools"),
    ("🧬", "DNA"),
    ("📖", "open book"),
    ("🤖", "robot"),
    ("🛰️", "satellite"),
    ("🪐", "ringed planet"),
    ("⚛️", "atom"),
    ("⠋", "braille spinner"),
    ("◌", "dotted circle"),
    ("◒", "half circle"),
    ("✦", "four point star"),
    ("◷", "seven clock"),
    ("◴", "three clock"),
    ("⌛", "hourglass"),
    ("⏳", "flowing hourglass"),
    ("?", "question mark"),
    ("!", "exclamation mark"),
    ("⚑", "outlined flag"),
    ("✋", "raised hand"),
    ("🔔", "bell"),
    ("✓", "check mark"),
    ("●", "filled circle"),
    ("○", "empty circle"),
    ("·", "middle dot"),
    ("◆", "filled diamond"),
    ("◇", "empty diamond"),
    ("◈", "diamond dot"),
    ("⬡", "hexagon"),
    ("⬢", "filled hexagon"),
    ("✚", "cross"),
    ("✿", "flower"),
    ("☀", "sun"),
    ("☁", "cloud"),
    ("☾", "moon"),
    ("⚡", "lightning"),
    ("∞", "infinity"),
];

/// Turns CLDR's terse subgroup keys into readable terminal labels while
/// preserving all its fine-grained categories.
fn category_label(subgroup: &'static str) -> &'static str {
    match subgroup {
        "animal-mammal" => "Animals · Mammals",
        "animal-bird" => "Animals · Birds",
        "animal-amphibian" => "Animals · Amphibians",
        "animal-reptile" => "Animals · Reptiles",
        "animal-marine" => "Animals · Marine",
        "animal-bug" => "Animals · Insects",
        "computer" => "Computer",
        "math" => "Mathematics",
        "other-symbol" => "Signs & symbols",
        "transport-sign" => "Transport signs",
        "geometric" => "Geometric shapes",
        "arrow" => "Arrows",
        "warning" => "Warnings",
        "currency" => "Currency",
        "keycap" => "Keycaps",
        "alphanum" => "Letters & numbers",
        "food-fruit" => "Food · Fruit",
        "food-vegetable" => "Food · Vegetables",
        "food-marine" => "Food · Seafood",
        "food-sweet" => "Food · Sweets",
        "food-prepared" => "Food · Prepared",
        "sky & weather" => "Sky & weather",
        "transport-air" => "Travel · Air",
        "transport-ground" => "Travel · Ground",
        "transport-water" => "Travel · Water",
        "place-map" => "Places · Maps",
        "place-geographic" => "Places · Geographic",
        "place-building" => "Places · Buildings",
        "place-religious" => "Places · Religious",
        "time" => "Time",
        "tool" => "Tools",
        "book-paper" => "Books & paper",
        "clothing" => "Clothing",
        "game" => "Games",
        "arts & crafts" => "Arts & crafts",
        _ => subgroup,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        all_picker_search_results, catalogue_icon_count, catalogue_icon_count_for, icon_categories,
        semantic_picker_search_results, IconCatalogFamily, IconSemanticSearchHit,
    };

    #[test]
    fn catalogue_contains_thousands_of_named_icons_in_detailed_categories() {
        assert!(catalogue_icon_count() >= 10_680);
        assert!(icon_categories().len() >= 50);
        assert!(icon_categories()
            .iter()
            .any(|category| category.label == "Animals · Mammals"));
        assert!(icon_categories()
            .iter()
            .any(|category| category.label == "Computer"));
        assert!(icon_categories()
            .iter()
            .any(|category| category.label == "Mathematics"));
        assert!(icon_categories()
            .iter()
            .any(|category| category.label == "Nerd Font · Material Design"));
        assert!(catalogue_icon_count_for(IconCatalogFamily::OfficialUtf8) >= 3_500);
        assert!(catalogue_icon_count_for(IconCatalogFamily::NerdFont) >= 8_000);
        let nerd_font_start = icon_categories()
            .iter()
            .position(|category| category.family == IconCatalogFamily::NerdFont)
            .expect("the catalogue has a Nerd Font section");
        assert!(nerd_font_start > 0);
        assert!(icon_categories()[..nerd_font_start]
            .iter()
            .all(|category| category.family == IconCatalogFamily::OfficialUtf8));
        assert!(icon_categories()[nerd_font_start..]
            .iter()
            .all(|category| category.family == IconCatalogFamily::NerdFont));
    }

    #[test]
    fn full_browse_document_preserves_family_order() {
        let all_chapters = all_picker_search_results().chapters;
        let first_nerd_font = all_chapters
            .iter()
            .position(|chapter| chapter.family == IconCatalogFamily::NerdFont)
            .expect("the picker has Nerd Font chapters");
        assert!(all_chapters[..first_nerd_font]
            .iter()
            .all(|chapter| chapter.family == IconCatalogFamily::OfficialUtf8));
    }

    #[test]
    fn semantic_hits_group_without_reintroducing_lexical_filtering() {
        let rocket = icon_categories()
            .iter()
            .flat_map(|category| category.entries.iter().map(move |entry| (category, *entry)))
            .find(|(_, entry)| entry.glyph == "🚀")
            .expect("the Unicode catalogue includes rocket");
        let results = semantic_picker_search_results(vec![IconSemanticSearchHit {
            category_label: rocket.0.label,
            family: rocket.0.family,
            entry: rocket.1,
        }]);
        assert_eq!(results.entry_count, 1);
        assert_eq!(results.entry(0).expect("one semantic hit").glyph, "🚀");
    }
}

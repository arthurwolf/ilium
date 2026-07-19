//! ilium-client's own `~/.config/ilium/config.toml` tables:
//! `[keybindings]` (remap which letter triggers an existing
//! `keymap::Action`), `[keyboard]` (choose the `Ctrl+letter` shortcut base),
//! and `[theme]` (override a handful of `theme::Theme`'s colors). Server-side config (`ilium-server/src/config.rs`: poll
//! intervals, custom detection signatures) lives in a different crate and
//! a different process, so the two never share a loader -- both happen to
//! read the same `config.toml` path, but each only ever looks at the
//! table(s) it owns.
//!
//! `crate::run` calls [`load`] once at startup, before the terminal enters
//! raw/alternate-screen mode and before any render call, and installs the
//! result via `keymap::init_effective_bindings`/`theme::init`.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use ilium_inference::InferenceSettings;
use ilium_sound::SoundSettings;
use ilium_voice::{ReasoningEffort, VadEagerness, VoiceInputMode, VoiceModel, VoiceName};
use ratatui::style::Color;
use serde::{Deserialize, Serialize};

use crate::error::ClientError;
use crate::icon_settings::{IconSettings, IconTarget};
use crate::keymap::{self, KeyBinding, ShortcutBase, LEADER_BINDINGS};
use crate::layout::{DEFAULT_TREE_WIDTH, MAX_TREE_WIDTH, MIN_TREE_WIDTH};
use crate::theme::{ColorScheme, Theme};

/// The four client-side config tables, already validated. What [`load`]
/// returns; a config file that fails to load falls back to
/// [`ClientConfig::default`] at the call site (`crate::run`) rather than
/// this module hardcoding that fallback here -- mirrors
/// `ilium_server::config::ServerConfig`'s own doc comment.
#[derive(Debug, Clone)]
pub struct ClientConfig {
    /// The effective leader-key binding table -- `LEADER_BINDINGS` with any
    /// `[keybindings]` overrides applied.
    pub keybindings: Vec<KeyBinding>,
    /// The live-editable `[keyboard]` settings shown in the Keyboard tab.
    pub keyboard: KeyboardSettings,
    pub theme: Theme,
    /// `[ui]` -- the settings screen's own settings (`crate::app::Mode::Settings`).
    /// Kept separate from `theme`/`keybindings` since it's the one table the
    /// running client can also *write*, not just read at startup -- see
    /// [`save_ui_settings`].
    pub ui: UiSettings,
    /// User-global sound source and event checkboxes. The client persists
    /// changes; the detached server owns actual playback.
    pub sound: SoundSettings,
    /// Board presentation settings shown in the Kanban Board tab.
    pub kanban_board: KanbanBoardSettings,
    /// Provider selection and credentials used only by client-owned naming workers.
    pub inference: InferenceSettings,
    /// Terminal presentation and creation defaults.
    pub terminal: TerminalSettings,
    /// Defaults applied when a local editor buffer is opened.
    pub editor: EditorSettings,
    /// Policy the detached server reads the next time it starts this session.
    pub session: SessionSettings,
    /// OpenAI Realtime voice-controller settings. The API key remains on the
    /// client side and is never included in semantic state snapshots.
    pub voice: VoiceSettings,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            keybindings: LEADER_BINDINGS.to_vec(),
            keyboard: KeyboardSettings::default(),
            theme: Theme::default(),
            ui: UiSettings::default(),
            sound: SoundSettings::default(),
            kanban_board: KanbanBoardSettings::default(),
            inference: InferenceSettings::default(),
            terminal: TerminalSettings::default(),
            editor: EditorSettings::default(),
            session: SessionSettings::default(),
            voice: VoiceSettings::default(),
        }
    }
}

/// Durable `[voice]` settings. Runtime ownership lives in `crate::run`; this
/// is a validated value object shared by persistence, Settings, and `App`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct VoiceSettings {
    pub enabled: bool,
    pub api_key: String,
    pub model: VoiceModel,
    pub voice: VoiceName,
    pub reasoning_effort: ReasoningEffort,
    pub input_mode: VoiceInputMode,
    pub vad_eagerness: VadEagerness,
    pub input_device_name: Option<String>,
    pub output_device_name: Option<String>,
    pub output_volume_percent: u8,
    pub confirm_terminal_submissions: bool,
    pub custom_prompt: String,
}

impl Default for VoiceSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            api_key: String::new(),
            model: VoiceModel::default(),
            voice: VoiceName::default(),
            reasoning_effort: ReasoningEffort::default(),
            input_mode: VoiceInputMode::default(),
            vad_eagerness: VadEagerness::default(),
            input_device_name: None,
            output_device_name: None,
            output_volume_percent: 80,
            confirm_terminal_submissions: false,
            custom_prompt: String::new(),
        }
    }
}

impl VoiceSettings {
    /// Returns whether two settings values configure the same owned audio and
    /// Realtime actor. Terminal confirmation is client-side control policy, so
    /// toggling it must not interrupt an otherwise healthy voice connection.
    pub fn has_same_runtime_configuration(&self, other: &Self) -> bool {
        self.api_key == other.api_key
            && self.model == other.model
            && self.voice == other.voice
            && self.reasoning_effort == other.reasoning_effort
            && self.input_mode == other.input_mode
            && self.vad_eagerness == other.vad_eagerness
            && self.input_device_name == other.input_device_name
            && self.output_device_name == other.output_device_name
            && self.output_volume_percent == other.output_volume_percent
            && self.custom_prompt == other.custom_prompt
    }
}

/// A deliberate motion preference: reduced motion retains state feedback but
/// removes spatial transitions, while off suppresses decorative animation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MotionLevel {
    #[default]
    Full,
    Reduced,
    Off,
}
impl MotionLevel {
    pub const ALL: [Self; 3] = [Self::Full, Self::Reduced, Self::Off];
    pub const fn label(self) -> &'static str {
        match self {
            Self::Full => "Full",
            Self::Reduced => "Reduced",
            Self::Off => "Off",
        }
    }
    pub fn stepped(self, direction: i32) -> Self {
        stepped_value(&Self::ALL, self, direction)
    }
}

/// Vertical information density of the tree sidebar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SidebarDensity {
    Compact,
    #[default]
    Standard,
    Comfortable,
}
impl SidebarDensity {
    pub const ALL: [Self; 3] = [Self::Compact, Self::Standard, Self::Comfortable];
    pub const fn label(self) -> &'static str {
        match self {
            Self::Compact => "Compact",
            Self::Standard => "Standard",
            Self::Comfortable => "Comfortable",
        }
    }
    pub fn stepped(self, direction: i32) -> Self {
        stepped_value(&Self::ALL, self, direction)
    }
}

/// Amount of terminal history retained per pane, measured in MiB.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalSettings {
    pub scrollback_budget_mib: u16,
    pub new_pane_directory: NewPaneDirectory,
}
impl Default for TerminalSettings {
    fn default() -> Self {
        Self {
            scrollback_budget_mib: 32,
            new_pane_directory: NewPaneDirectory::ProjectRoot,
        }
    }
}
impl TerminalSettings {
    pub const MIN_SCROLLBACK_BUDGET_MIB: u16 = 4;
    pub const MAX_SCROLLBACK_BUDGET_MIB: u16 = 512;
    pub fn stepped_scrollback_budget_mib(self, direction: i32) -> u16 {
        let value = self.scrollback_budget_mib as i32 + direction * 4;
        value.clamp(
            Self::MIN_SCROLLBACK_BUDGET_MIB as i32,
            Self::MAX_SCROLLBACK_BUDGET_MIB as i32,
        ) as u16
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NewPaneDirectory {
    #[default]
    ProjectRoot,
    FocusedTerminal,
    LastUsed,
}
impl NewPaneDirectory {
    pub const ALL: [Self; 3] = [Self::ProjectRoot, Self::FocusedTerminal, Self::LastUsed];
    pub const fn label(self) -> &'static str {
        match self {
            Self::ProjectRoot => "Project root",
            Self::FocusedTerminal => "Focused terminal",
            Self::LastUsed => "Last used",
        }
    }
    pub fn stepped(self, direction: i32) -> Self {
        stepped_value(&Self::ALL, self, direction)
    }
}

/// Defaults for newly opened local editor panes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorSettings {
    pub show_line_numbers: bool,
    pub show_minimap: bool,
    pub autosave_enabled: bool,
    pub autosave_delay_ms: u16,
    pub markdown_rendered_by_default: bool,
}
impl Default for EditorSettings {
    fn default() -> Self {
        Self {
            show_line_numbers: true,
            show_minimap: true,
            autosave_enabled: true,
            autosave_delay_ms: 1000,
            markdown_rendered_by_default: false,
        }
    }
}
impl EditorSettings {
    pub fn stepped_autosave_delay_ms(self, direction: i32) -> u16 {
        const VALUES: [u16; 5] = [250, 500, 1000, 2000, 5000];
        let current = VALUES
            .iter()
            .position(|value| *value == self.autosave_delay_ms)
            .unwrap_or(2);
        let offset = if direction < 0 { VALUES.len() - 1 } else { 1 };
        VALUES[(current + offset) % VALUES.len()]
    }
}

/// Server startup behavior for a persisted project-session snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SessionRecoveryPolicy {
    #[default]
    RestoreAutomatically,
    AskBeforeRestore,
    StartFresh,
}
impl SessionRecoveryPolicy {
    pub const ALL: [Self; 3] = [
        Self::RestoreAutomatically,
        Self::AskBeforeRestore,
        Self::StartFresh,
    ];
    pub const fn label(self) -> &'static str {
        match self {
            Self::RestoreAutomatically => "Restore automatically",
            Self::AskBeforeRestore => "Ask before restoring",
            Self::StartFresh => "Start fresh",
        }
    }
    pub fn stepped(self, direction: i32) -> Self {
        stepped_value(&Self::ALL, self, direction)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SessionSettings {
    pub recovery_policy: SessionRecoveryPolicy,
}

/// `[keyboard]` settings, validated before they reach input dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct KeyboardSettings {
    pub shortcut_base: ShortcutBase,
}

pub const MIN_CARD_PREVIEW_LINES: u16 = 1;
pub const MAX_CARD_PREVIEW_LINES: u16 = 10;
pub const DEFAULT_CARD_PREVIEW_LINES: u16 = 3;
pub const MIN_BOARD_COLUMN_WIDTH: u16 = 10;
pub const MAX_BOARD_COLUMN_WIDTH: u16 = 80;
pub const DEFAULT_BOARD_COLUMN_WIDTH: u16 = 20;

/// `[kanban_board]` settings, validated before they reach board layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KanbanBoardSettings {
    pub card_preview_lines: u16,
    pub minimum_column_width: u16,
}

impl Default for KanbanBoardSettings {
    fn default() -> Self {
        Self {
            card_preview_lines: DEFAULT_CARD_PREVIEW_LINES,
            minimum_column_width: DEFAULT_BOARD_COLUMN_WIDTH,
        }
    }
}

/// How a detected agent's type is identified in the left tree panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AgentIdentifierMode {
    #[default]
    FullName,
    Letter,
    Icon,
    Hidden,
}

impl AgentIdentifierMode {
    pub const ALL: [Self; 4] = [Self::FullName, Self::Letter, Self::Icon, Self::Hidden];

    pub const fn label(self) -> &'static str {
        match self {
            Self::FullName => "Full name",
            Self::Letter => "Single letter",
            Self::Icon => "Selected icon",
            Self::Hidden => "Nothing",
        }
    }

    pub fn stepped(self, direction: i32) -> Self {
        stepped_value(&Self::ALL, self, direction)
    }
}

/// Claude-specific icon choices. A closed enum prevents hand-edited config
/// from introducing an empty, over-wide, or otherwise unstable tree glyph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClaudeAgentIcon {
    #[default]
    Brain,
    MagicWand,
    Compass,
    Thread,
    Crab,
    Lobster,
}

impl ClaudeAgentIcon {
    pub const ALL: [Self; 6] = [
        Self::Brain,
        Self::MagicWand,
        Self::Compass,
        Self::Thread,
        Self::Crab,
        Self::Lobster,
    ];

    pub const fn glyph(self) -> &'static str {
        match self {
            Self::Brain => "🧠",
            Self::MagicWand => "🪄",
            Self::Compass => "🧭",
            Self::Thread => "🧵",
            Self::Crab => "🦀",
            Self::Lobster => "🦞",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Brain => "🧠 Brain",
            Self::MagicWand => "🪄 Magic wand",
            Self::Compass => "🧭 Compass",
            Self::Thread => "🧵 Thread",
            Self::Crab => "🦀 Crab",
            Self::Lobster => "🦞 Lobster",
        }
    }

    pub fn stepped(self, direction: i32) -> Self {
        stepped_value(&Self::ALL, self, direction)
    }
}

/// Codex-specific icon choices, kept separate from Claude's choices so an
/// impossible cross-agent selection cannot be represented in memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CodexAgentIcon {
    #[default]
    Gear,
    Tools,
    Dna,
    Book,
    CrossMark,
}

/// Antigravity-specific icon choices. Kept separate from the other provider
/// palettes so every saved setting remains a valid, intentionally curated
/// glyph for the provider it represents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AntigravityAgentIcon {
    #[default]
    Spark,
    Satellite,
    Orbit,
    Atom,
}

impl AntigravityAgentIcon {
    pub const ALL: [Self; 4] = [Self::Spark, Self::Satellite, Self::Orbit, Self::Atom];

    pub const fn glyph(self) -> &'static str {
        match self {
            Self::Spark => "✦",
            Self::Satellite => "🛰️",
            Self::Orbit => "🪐",
            Self::Atom => "⚛️",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Spark => "✦ Spark",
            Self::Satellite => "🛰️ Satellite",
            Self::Orbit => "🪐 Orbit",
            Self::Atom => "⚛️ Atom",
        }
    }

    pub fn stepped(self, direction: i32) -> Self {
        stepped_value(&Self::ALL, self, direction)
    }
}

impl CodexAgentIcon {
    pub const ALL: [Self; 5] = [
        Self::Gear,
        Self::Tools,
        Self::Dna,
        Self::Book,
        Self::CrossMark,
    ];

    pub const fn glyph(self) -> &'static str {
        match self {
            Self::Gear => "⚙️",
            Self::Tools => "🛠️",
            Self::Dna => "🧬",
            Self::Book => "📖",
            Self::CrossMark => "❎",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Gear => "⚙️ Gear",
            Self::Tools => "🛠️ Tools",
            Self::Dna => "🧬 DNA",
            Self::Book => "📖 Book",
            Self::CrossMark => "❎ Cross mark",
        }
    }

    pub fn stepped(self, direction: i32) -> Self {
        stepped_value(&Self::ALL, self, direction)
    }
}

/// Complete, validated agent-identifier presentation contract consumed by
/// the settings screen and tree renderer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AgentIdentifierSettings {
    pub mode: AgentIdentifierMode,
    pub claude_icon: ClaudeAgentIcon,
    pub codex_icon: CodexAgentIcon,
    pub antigravity_icon: AntigravityAgentIcon,
}

/// How the left tree presents the durable child order owned by the server.
/// Automatic modes are a client-side view only, so returning to `Manual`
/// reveals and edits the exact order persisted in the project snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TreeOrder {
    #[default]
    Manual,
    Type,
    AgeAscending,
    AgeDescending,
    NameAscending,
    NameDescending,
}

impl TreeOrder {
    /// Shared registry for the context submenu and settings selector.
    pub const ALL: [Self; 6] = [
        Self::Manual,
        Self::Type,
        Self::AgeAscending,
        Self::AgeDescending,
        Self::NameAscending,
        Self::NameDescending,
    ];

    /// User-facing label. Age means elapsed age, so ascending puts the
    /// youngest nodes first and descending puts the oldest nodes first.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Manual => "Manual",
            Self::Type => "Type",
            Self::AgeAscending => "Age up (newest first)",
            Self::AgeDescending => "Age down (oldest first)",
            Self::NameAscending => "Name A-Z",
            Self::NameDescending => "Name Z-A",
        }
    }

    pub fn stepped(self, direction: i32) -> Self {
        stepped_value(&Self::ALL, self, direction)
    }
}

/// Cycles one value in a small ordered settings registry, wrapping in either
/// direction. Every caller passes an enum's non-empty `ALL` array.
fn stepped_value<T: Copy + PartialEq>(values: &[T], current: T, direction: i32) -> T {
    let index = values
        .iter()
        .position(|value| *value == current)
        .unwrap_or(0);
    let offset = if direction < 0 { values.len() - 1 } else { 1 };
    values[(index + offset) % values.len()]
}

/// `[ui]`'s settings, already validated -- what the settings screen reads
/// from and writes to (via `App`'s live copy, persisted with
/// [`save_ui_settings`]). Read this crate's `CLAUDE.md`-style reminder in
/// `crate::app`'s `SettingsState` doc comment before adding a fourth entry
/// here: every new setting needs a `Raw` field, a validated field here, a
/// default, a settings-screen row, and a `[ui]` table key -- in that order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiSettings {
    /// Whether the tree panel widens while it has mouse/keyboard focus (see
    /// `crate::layout::TreeWidthAnimation`). Some users find the motion
    /// distracting or simply prefer a fixed-width sidebar; this is a pure
    /// on/off switch for that whole affordance.
    pub auto_resize_tree_on_focus: bool,
    /// The tree panel's collapsed (or, with auto-resize disabled, only)
    /// width -- `crate::layout::TreeWidthAnimation`'s base width.
    pub tree_width: u16,
    pub color_scheme: ColorScheme,
    /// Agent-type representation in the left tree. Activity remains a
    /// separate, always-visible status column regardless of this choice.
    pub agent_identifiers: AgentIdentifierSettings,
    /// Presentation order applied independently inside every normal group.
    /// Split-view child order remains structural because it controls pane
    /// placement in the right panel.
    pub tree_order: TreeOrder,
    pub motion_level: MotionLevel,
    pub sidebar_density: SidebarDensity,
    /// Uses single-cell text symbols in the row-action hover overlay. This
    /// is an accessibility/terminal-compatibility preference only; rich
    /// Unicode icons remain the normal, default presentation.
    pub use_stable_glyphs: bool,
    /// Shows LLM-suggested per-title icons before node names in the left tree.
    pub show_inferred_title_icons: bool,
    /// Global glyph assignments for every configurable sidebar icon role.
    pub icons: IconSettings,
}

impl Default for UiSettings {
    fn default() -> Self {
        Self {
            auto_resize_tree_on_focus: true,
            tree_width: DEFAULT_TREE_WIDTH,
            color_scheme: ColorScheme::Dark,
            agent_identifiers: AgentIdentifierSettings::default(),
            tree_order: TreeOrder::Manual,
            motion_level: MotionLevel::Full,
            sidebar_density: SidebarDensity::Standard,
            use_stable_glyphs: false,
            show_inferred_title_icons: false,
            icons: IconSettings::default(),
        }
    }
}

/// The on-disk shape of `config.toml`'s client-side tables. Kept separate
/// from [`ClientConfig`] (which uses real `KeyBinding`/`Theme` values and
/// enforces the validation invariants) so a partially-specified or invalid
/// config file can be validated in one place ([`merge_keybindings`],
/// [`merge_theme`]) rather than every field needing its own serde
/// validator.
#[derive(Debug, Default, Deserialize)]
struct RawClientConfig {
    /// `action_name -> letter`, e.g. `new_terminal = "c"`. Only remaps
    /// which letter triggers an existing `Action` -- see `keymap`'s module
    /// doc comment for why defining brand-new actions is out of scope.
    #[serde(default)]
    keybindings: HashMap<String, String>,
    #[serde(default)]
    keyboard: RawKeyboardConfig,
    #[serde(default)]
    theme: RawThemeConfig,
    #[serde(default)]
    ui: RawUiConfig,
    #[serde(default)]
    sound: SoundSettings,
    #[serde(default)]
    kanban_board: RawKanbanBoardConfig,
    #[serde(default)]
    inference: InferenceSettings,
    #[serde(default)]
    terminal: RawTerminalConfig,
    #[serde(default)]
    editor: RawEditorConfig,
    #[serde(default)]
    session: RawSessionConfig,
    #[serde(default)]
    voice: VoiceSettings,
}

/// `[keyboard]`'s optional on-disk shape.
#[derive(Debug, Default, Deserialize)]
struct RawKeyboardConfig {
    shortcut_base: Option<String>,
}

/// `[kanban_board]`'s optional on-disk shape.
#[derive(Debug, Default, Deserialize)]
struct RawKanbanBoardConfig {
    card_preview_lines: Option<u16>,
    minimum_column_width: Option<u16>,
}

/// `[ui]`'s raw, possibly-partial on-disk shape -- see [`UiSettings`] for
/// the validated equivalent [`merge_ui`] produces.
#[derive(Debug, Default, Deserialize)]
struct RawUiConfig {
    auto_resize_tree_on_focus: Option<bool>,
    tree_width: Option<u16>,
    /// `"dark"` or `"light"` (case-insensitive) -- see
    /// [`parse_color_scheme`].
    color_scheme: Option<String>,
    agent_identifier_mode: Option<String>,
    claude_agent_icon: Option<String>,
    codex_agent_icon: Option<String>,
    antigravity_agent_icon: Option<String>,
    tree_order: Option<String>,
    motion_level: Option<String>,
    sidebar_density: Option<String>,
    use_stable_glyphs: Option<bool>,
    show_inferred_title_icons: Option<bool>,
    #[serde(default)]
    icons: HashMap<String, String>,
}

#[derive(Debug, Default, Deserialize)]
struct RawTerminalConfig {
    scrollback_budget_mib: Option<u16>,
    new_pane_directory: Option<String>,
}
#[derive(Debug, Default, Deserialize)]
struct RawEditorConfig {
    show_line_numbers: Option<bool>,
    show_minimap: Option<bool>,
    autosave_enabled: Option<bool>,
    autosave_delay_ms: Option<u16>,
    markdown_rendered_by_default: Option<bool>,
}
#[derive(Debug, Default, Deserialize)]
struct RawSessionConfig {
    recovery_policy: Option<String>,
}

/// `[theme]`'s color overrides, each an optional `"#rrggbb"` (or `rrggbb`)
/// hex string. Covers the four colors `theme::Theme` currently exposes --
/// see `theme`'s module doc comment on why the rest of the app's
/// `ratatui::style::Style` values stay hardcoded.
#[derive(Debug, Default, Deserialize)]
struct RawThemeConfig {
    accent_bg: Option<String>,
    accent_fg: Option<String>,
    border_focused: Option<String>,
    border_unfocused: Option<String>,
}

/// Why `~/.config/ilium/config.toml`'s client-side tables could not be
/// loaded -- kept separate from `ClientError::ConfigLoad`'s `path` field so
/// the two concerns (which file, why it failed) stay independently
/// testable, mirroring `ilium_server::error::ConfigLoadError`.
#[derive(Debug, thiserror::Error)]
pub enum ConfigLoadError {
    #[error("failed to read config file: {0}")]
    Read(#[from] std::io::Error),
    #[error("failed to parse config file as TOML: {0}")]
    Parse(#[from] toml::de::Error),
    /// A `[keybindings]` key isn't any known `Action`'s `action_name`.
    #[error("keybindings.{0:?} is not a known action")]
    UnknownAction(String),
    /// A `[keybindings]` value isn't exactly one character.
    #[error("keybindings.{action:?} = {value:?} must be exactly one character")]
    InvalidLetter { action: String, value: String },
    /// `[keyboard].shortcut_base` is not exactly one ASCII letter.
    #[error("keyboard.shortcut_base = {0:?} must be exactly one letter from A to Z")]
    InvalidShortcutBase(String),
    /// Two actions ended up bound to the same letter after applying every
    /// override -- `action_for` can only ever dispatch one of them, so this
    /// is rejected rather than silently picking whichever comes first in
    /// table order.
    #[error("keybindings config binds more than one action to the letter {0:?}")]
    DuplicateLetter(char),
    /// A `[theme]` value isn't a valid `#rrggbb`/`rrggbb` hex color.
    #[error("theme.{field:?} = {value:?} is not a valid #rrggbb hex color")]
    InvalidColor { field: &'static str, value: String },
    /// `ui.tree_width` is outside `[MIN_TREE_WIDTH, MAX_TREE_WIDTH]`.
    #[error("ui.tree_width = {0} must be between {MIN_TREE_WIDTH} and {MAX_TREE_WIDTH}")]
    InvalidTreeWidth(u16),
    /// `ui.color_scheme` isn't `"dark"` or `"light"`.
    #[error("ui.color_scheme = {0:?} must be \"dark\" or \"light\"")]
    InvalidColorScheme(String),
    /// `ui.agent_identifier_mode` is outside the closed display-mode set.
    #[error(
        "ui.agent_identifier_mode = {0:?} must be \"full_name\", \"letter\", \"icon\", or \"hidden\""
    )]
    InvalidAgentIdentifierMode(String),
    /// `ui.claude_agent_icon` is not one of the curated Claude choices.
    #[error("ui.claude_agent_icon = {0:?} is not a supported Claude icon")]
    InvalidClaudeAgentIcon(String),
    /// `ui.codex_agent_icon` is not one of the curated Codex choices.
    #[error("ui.codex_agent_icon = {0:?} is not a supported Codex icon")]
    InvalidCodexAgentIcon(String),
    /// `ui.antigravity_agent_icon` is not one of the curated choices.
    #[error("ui.antigravity_agent_icon = {0:?} is not a supported Antigravity icon")]
    InvalidAntigravityAgentIcon(String),
    /// `ui.tree_order` is outside the closed ordering-mode set.
    #[error(
        "ui.tree_order = {0:?} must be \"manual\", \"type\", \"age_ascending\", \"age_descending\", \"name_ascending\", or \"name_descending\""
    )]
    InvalidTreeOrder(String),
    #[error("ui.motion_level = {0:?} must be full, reduced, or off")]
    InvalidMotionLevel(String),
    #[error("ui.sidebar_density = {0:?} must be compact, standard, or comfortable")]
    InvalidSidebarDensity(String),
    #[error("terminal.scrollback_budget_mib = {0} must be between 4 and 512")]
    InvalidScrollbackBudget(u16),
    #[error("terminal.new_pane_directory = {0:?} is not supported")]
    InvalidNewPaneDirectory(String),
    #[error("session.recovery_policy = {0:?} is not supported")]
    InvalidSessionRecoveryPolicy(String),
    /// `kanban_board.card_preview_lines` is outside the supported range.
    #[error(
        "kanban_board.card_preview_lines = {0} must be between {MIN_CARD_PREVIEW_LINES} and {MAX_CARD_PREVIEW_LINES}"
    )]
    InvalidCardPreviewLines(u16),
    /// `kanban_board.minimum_column_width` is outside the supported range.
    #[error(
        "kanban_board.minimum_column_width = {0} must be between {MIN_BOARD_COLUMN_WIDTH} and {MAX_BOARD_COLUMN_WIDTH}"
    )]
    InvalidBoardColumnWidth(u16),
    #[error("voice.output_volume_percent = {0} must be between 0 and 100")]
    InvalidVoiceOutputVolume(u8),
}

/// Why persisting a settings-screen change to `config.toml` failed -- see
/// [`save_ui_settings`].
#[derive(Debug, thiserror::Error)]
pub enum ConfigSaveError {
    #[error("failed to read existing config file: {0}")]
    Read(std::io::Error),
    #[error("failed to parse existing config file as TOML: {0}")]
    Parse(toml::de::Error),
    #[error("failed to serialize config as TOML: {0}")]
    Serialize(#[from] toml::ser::Error),
    #[error("failed to write config file: {0}")]
    Write(std::io::Error),
}

/// Loads `<config_dir>/config.toml`'s `[keybindings]`, `[keyboard]`, `[theme]`,
/// and `[ui]` tables. A
/// missing file is not an error -- most users never create one -- and
/// loads as [`ClientConfig::default`]. A file that exists but fails to
/// read, parse, or validate *is* a [`ClientError::ConfigLoad`]; the caller
/// (`crate::run`) logs it and falls back to defaults rather than refusing
/// to start the client over a typo in an optional config file.
pub fn load(config_dir: &Path) -> Result<ClientConfig, ClientError> {
    let path = config_dir.join("config.toml");
    if !path.exists() {
        return Ok(ClientConfig::default());
    }
    let contents = std::fs::read_to_string(&path).map_err(|source| ClientError::ConfigLoad {
        path: path.clone(),
        source: ConfigLoadError::Read(source),
    })?;
    let raw: RawClientConfig =
        toml::from_str(&contents).map_err(|source| ClientError::ConfigLoad {
            path: path.clone(),
            source: ConfigLoadError::Parse(source),
        })?;

    let keybindings =
        merge_keybindings(&raw.keybindings).map_err(|source| ClientError::ConfigLoad {
            path: path.clone(),
            source,
        })?;
    let keyboard = merge_keyboard(raw.keyboard).map_err(|source| ClientError::ConfigLoad {
        path: path.clone(),
        source,
    })?;
    let ui = merge_ui(raw.ui).map_err(|source| ClientError::ConfigLoad {
        path: path.clone(),
        source,
    })?;
    let kanban_board =
        merge_kanban_board(raw.kanban_board).map_err(|source| ClientError::ConfigLoad {
            path: path.clone(),
            source,
        })?;
    let terminal = merge_terminal(raw.terminal).map_err(|source| ClientError::ConfigLoad {
        path: path.clone(),
        source,
    })?;
    let editor = merge_editor(raw.editor);
    let session = merge_session(raw.session).map_err(|source| ClientError::ConfigLoad {
        path: path.clone(),
        source,
    })?;
    if raw.voice.output_volume_percent > 100 {
        return Err(ClientError::ConfigLoad {
            path: path.clone(),
            source: ConfigLoadError::InvalidVoiceOutputVolume(raw.voice.output_volume_percent),
        });
    }
    let theme =
        merge_theme(raw.theme, ui.color_scheme).map_err(|source| ClientError::ConfigLoad {
            path: path.clone(),
            source,
        })?;

    Ok(ClientConfig {
        keybindings,
        keyboard,
        theme,
        ui,
        sound: raw.sound,
        kanban_board,
        inference: raw.inference,
        terminal,
        editor,
        session,
        voice: raw.voice,
    })
}

/// Resolves the shortcut base while keeping an absent `[keyboard]` table on
/// the portable `Ctrl+A` default.
fn merge_keyboard(raw: RawKeyboardConfig) -> Result<KeyboardSettings, ConfigLoadError> {
    let shortcut_base = match raw.shortcut_base {
        Some(value) => ShortcutBase::parse(&value)
            .ok_or_else(|| ConfigLoadError::InvalidShortcutBase(value.clone()))?,
        None => ShortcutBase::default(),
    };
    Ok(KeyboardSettings { shortcut_base })
}

/// Applies the optional board preview height over the three-line default.
fn merge_kanban_board(raw: RawKanbanBoardConfig) -> Result<KanbanBoardSettings, ConfigLoadError> {
    let card_preview_lines = match raw.card_preview_lines {
        Some(lines) if (MIN_CARD_PREVIEW_LINES..=MAX_CARD_PREVIEW_LINES).contains(&lines) => lines,
        Some(lines) => return Err(ConfigLoadError::InvalidCardPreviewLines(lines)),
        None => DEFAULT_CARD_PREVIEW_LINES,
    };
    let minimum_column_width = match raw.minimum_column_width {
        Some(width) if (MIN_BOARD_COLUMN_WIDTH..=MAX_BOARD_COLUMN_WIDTH).contains(&width) => width,
        Some(width) => return Err(ConfigLoadError::InvalidBoardColumnWidth(width)),
        None => DEFAULT_BOARD_COLUMN_WIDTH,
    };
    Ok(KanbanBoardSettings {
        card_preview_lines,
        minimum_column_width,
    })
}

/// Applies `[ui]` overrides onto [`UiSettings::default`]. Pure, same
/// rationale as [`merge_keybindings`]/[`merge_theme`] for staying
/// side-effect-free.
fn merge_ui(raw: RawUiConfig) -> Result<UiSettings, ConfigLoadError> {
    let defaults = UiSettings::default();
    let tree_width = match raw.tree_width {
        Some(width) if (MIN_TREE_WIDTH..=MAX_TREE_WIDTH).contains(&width) => width,
        Some(width) => return Err(ConfigLoadError::InvalidTreeWidth(width)),
        None => defaults.tree_width,
    };
    let color_scheme = match raw.color_scheme {
        Some(value) => parse_color_scheme(&value)?,
        None => defaults.color_scheme,
    };
    let mode = match raw.agent_identifier_mode {
        Some(value) => parse_agent_identifier_mode(&value)?,
        None => defaults.agent_identifiers.mode,
    };
    let claude_icon = match raw.claude_agent_icon {
        Some(value) => parse_claude_agent_icon(&value)?,
        None => defaults.agent_identifiers.claude_icon,
    };
    let codex_icon = match raw.codex_agent_icon {
        Some(value) => parse_codex_agent_icon(&value)?,
        None => defaults.agent_identifiers.codex_icon,
    };
    let antigravity_icon = match raw.antigravity_agent_icon {
        Some(value) => parse_antigravity_agent_icon(&value)?,
        None => defaults.agent_identifiers.antigravity_icon,
    };
    let tree_order = match raw.tree_order {
        Some(value) => parse_tree_order(&value)?,
        None => defaults.tree_order,
    };
    let motion_level = raw
        .motion_level
        .as_deref()
        .map(parse_motion_level)
        .transpose()?
        .unwrap_or_default();
    let sidebar_density = raw
        .sidebar_density
        .as_deref()
        .map(parse_sidebar_density)
        .transpose()?
        .unwrap_or_default();
    let icons = IconSettings::from_target(|target| {
        raw.icons
            .get(target.key())
            .filter(|glyph| !glyph.trim().is_empty())
            .cloned()
            .unwrap_or_else(|| target.default_glyph().to_string())
    });
    Ok(UiSettings {
        auto_resize_tree_on_focus: raw
            .auto_resize_tree_on_focus
            .unwrap_or(defaults.auto_resize_tree_on_focus),
        tree_width,
        color_scheme,
        agent_identifiers: AgentIdentifierSettings {
            mode,
            claude_icon,
            codex_icon,
            antigravity_icon,
        },
        tree_order,
        motion_level,
        sidebar_density,
        use_stable_glyphs: raw.use_stable_glyphs.unwrap_or(defaults.use_stable_glyphs),
        show_inferred_title_icons: raw
            .show_inferred_title_icons
            .unwrap_or(defaults.show_inferred_title_icons),
        icons,
    })
}

fn merge_terminal(raw: RawTerminalConfig) -> Result<TerminalSettings, ConfigLoadError> {
    let defaults = TerminalSettings::default();
    let scrollback_budget_mib = raw
        .scrollback_budget_mib
        .unwrap_or(defaults.scrollback_budget_mib);
    if !(TerminalSettings::MIN_SCROLLBACK_BUDGET_MIB..=TerminalSettings::MAX_SCROLLBACK_BUDGET_MIB)
        .contains(&scrollback_budget_mib)
    {
        return Err(ConfigLoadError::InvalidScrollbackBudget(
            scrollback_budget_mib,
        ));
    }
    let new_pane_directory = raw
        .new_pane_directory
        .as_deref()
        .map(parse_new_pane_directory)
        .transpose()?
        .unwrap_or(defaults.new_pane_directory);
    Ok(TerminalSettings {
        scrollback_budget_mib,
        new_pane_directory,
    })
}
fn merge_editor(raw: RawEditorConfig) -> EditorSettings {
    let defaults = EditorSettings::default();
    EditorSettings {
        show_line_numbers: raw.show_line_numbers.unwrap_or(defaults.show_line_numbers),
        show_minimap: raw.show_minimap.unwrap_or(defaults.show_minimap),
        autosave_enabled: raw.autosave_enabled.unwrap_or(defaults.autosave_enabled),
        autosave_delay_ms: raw
            .autosave_delay_ms
            .unwrap_or(defaults.autosave_delay_ms)
            .clamp(250, 5000),
        markdown_rendered_by_default: raw
            .markdown_rendered_by_default
            .unwrap_or(defaults.markdown_rendered_by_default),
    }
}
fn merge_session(raw: RawSessionConfig) -> Result<SessionSettings, ConfigLoadError> {
    Ok(SessionSettings {
        recovery_policy: raw
            .recovery_policy
            .as_deref()
            .map(parse_session_recovery_policy)
            .transpose()?
            .unwrap_or_default(),
    })
}

fn parse_motion_level(value: &str) -> Result<MotionLevel, ConfigLoadError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "full" => Ok(MotionLevel::Full),
        "reduced" => Ok(MotionLevel::Reduced),
        "off" => Ok(MotionLevel::Off),
        _ => Err(ConfigLoadError::InvalidMotionLevel(value.to_string())),
    }
}
fn parse_sidebar_density(value: &str) -> Result<SidebarDensity, ConfigLoadError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "compact" => Ok(SidebarDensity::Compact),
        "standard" => Ok(SidebarDensity::Standard),
        "comfortable" => Ok(SidebarDensity::Comfortable),
        _ => Err(ConfigLoadError::InvalidSidebarDensity(value.to_string())),
    }
}
fn parse_new_pane_directory(value: &str) -> Result<NewPaneDirectory, ConfigLoadError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "project_root" => Ok(NewPaneDirectory::ProjectRoot),
        "focused_terminal" => Ok(NewPaneDirectory::FocusedTerminal),
        "last_used" => Ok(NewPaneDirectory::LastUsed),
        _ => Err(ConfigLoadError::InvalidNewPaneDirectory(value.to_string())),
    }
}
fn parse_session_recovery_policy(value: &str) -> Result<SessionRecoveryPolicy, ConfigLoadError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "restore_automatically" => Ok(SessionRecoveryPolicy::RestoreAutomatically),
        "ask_before_restore" => Ok(SessionRecoveryPolicy::AskBeforeRestore),
        "start_fresh" => Ok(SessionRecoveryPolicy::StartFresh),
        _ => Err(ConfigLoadError::InvalidSessionRecoveryPolicy(
            value.to_string(),
        )),
    }
}

/// Parses the stable on-disk name for the tree's agent identifier mode.
fn parse_agent_identifier_mode(value: &str) -> Result<AgentIdentifierMode, ConfigLoadError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "full_name" => Ok(AgentIdentifierMode::FullName),
        "letter" => Ok(AgentIdentifierMode::Letter),
        "icon" => Ok(AgentIdentifierMode::Icon),
        "hidden" => Ok(AgentIdentifierMode::Hidden),
        _ => Err(ConfigLoadError::InvalidAgentIdentifierMode(
            value.to_string(),
        )),
    }
}

/// Parses a curated Claude icon by semantic name rather than embedding emoji
/// in `config.toml`, keeping hand-edited config readable across fonts.
fn parse_claude_agent_icon(value: &str) -> Result<ClaudeAgentIcon, ConfigLoadError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "brain" => Ok(ClaudeAgentIcon::Brain),
        "magic_wand" => Ok(ClaudeAgentIcon::MagicWand),
        "compass" => Ok(ClaudeAgentIcon::Compass),
        "thread" => Ok(ClaudeAgentIcon::Thread),
        "crab" => Ok(ClaudeAgentIcon::Crab),
        "lobster" => Ok(ClaudeAgentIcon::Lobster),
        _ => Err(ConfigLoadError::InvalidClaudeAgentIcon(value.to_string())),
    }
}

/// Parses a curated Codex icon by semantic name; see
/// [`parse_claude_agent_icon`] for the storage rationale.
fn parse_codex_agent_icon(value: &str) -> Result<CodexAgentIcon, ConfigLoadError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "gear" => Ok(CodexAgentIcon::Gear),
        "tools" => Ok(CodexAgentIcon::Tools),
        "dna" => Ok(CodexAgentIcon::Dna),
        "book" => Ok(CodexAgentIcon::Book),
        "cross_mark" => Ok(CodexAgentIcon::CrossMark),
        _ => Err(ConfigLoadError::InvalidCodexAgentIcon(value.to_string())),
    }
}

/// Parses a curated Antigravity icon by semantic name; see
/// [`parse_claude_agent_icon`] for the storage rationale.
fn parse_antigravity_agent_icon(value: &str) -> Result<AntigravityAgentIcon, ConfigLoadError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "spark" => Ok(AntigravityAgentIcon::Spark),
        "satellite" => Ok(AntigravityAgentIcon::Satellite),
        "orbit" => Ok(AntigravityAgentIcon::Orbit),
        "atom" => Ok(AntigravityAgentIcon::Atom),
        _ => Err(ConfigLoadError::InvalidAntigravityAgentIcon(
            value.to_string(),
        )),
    }
}

/// Parses the stable config spelling shared with [`tree_order_name`].
fn parse_tree_order(value: &str) -> Result<TreeOrder, ConfigLoadError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "manual" => Ok(TreeOrder::Manual),
        "type" => Ok(TreeOrder::Type),
        "age_ascending" => Ok(TreeOrder::AgeAscending),
        "age_descending" => Ok(TreeOrder::AgeDescending),
        "name_ascending" => Ok(TreeOrder::NameAscending),
        "name_descending" => Ok(TreeOrder::NameDescending),
        _ => Err(ConfigLoadError::InvalidTreeOrder(value.to_string())),
    }
}

/// Parses `[ui].color_scheme`'s string value, case-insensitively.
fn parse_color_scheme(value: &str) -> Result<ColorScheme, ConfigLoadError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "dark" => Ok(ColorScheme::Dark),
        "light" => Ok(ColorScheme::Light),
        _ => Err(ConfigLoadError::InvalidColorScheme(value.to_string())),
    }
}

/// The stable, on-disk string for a [`ColorScheme`] -- the inverse of
/// [`parse_color_scheme`], used both when validating and when
/// [`save_ui_settings`] writes a value back out.
fn color_scheme_name(scheme: ColorScheme) -> &'static str {
    match scheme {
        ColorScheme::Dark => "dark",
        ColorScheme::Light => "light",
    }
}

fn agent_identifier_mode_name(mode: AgentIdentifierMode) -> &'static str {
    match mode {
        AgentIdentifierMode::FullName => "full_name",
        AgentIdentifierMode::Letter => "letter",
        AgentIdentifierMode::Icon => "icon",
        AgentIdentifierMode::Hidden => "hidden",
    }
}

fn claude_agent_icon_name(icon: ClaudeAgentIcon) -> &'static str {
    match icon {
        ClaudeAgentIcon::Brain => "brain",
        ClaudeAgentIcon::MagicWand => "magic_wand",
        ClaudeAgentIcon::Compass => "compass",
        ClaudeAgentIcon::Thread => "thread",
        ClaudeAgentIcon::Crab => "crab",
        ClaudeAgentIcon::Lobster => "lobster",
    }
}

fn codex_agent_icon_name(icon: CodexAgentIcon) -> &'static str {
    match icon {
        CodexAgentIcon::Gear => "gear",
        CodexAgentIcon::Tools => "tools",
        CodexAgentIcon::Dna => "dna",
        CodexAgentIcon::Book => "book",
        CodexAgentIcon::CrossMark => "cross_mark",
    }
}

fn antigravity_agent_icon_name(icon: AntigravityAgentIcon) -> &'static str {
    match icon {
        AntigravityAgentIcon::Spark => "spark",
        AntigravityAgentIcon::Satellite => "satellite",
        AntigravityAgentIcon::Orbit => "orbit",
        AntigravityAgentIcon::Atom => "atom",
    }
}

/// Stable on-disk spelling for one tree-order mode.
fn tree_order_name(tree_order: TreeOrder) -> &'static str {
    match tree_order {
        TreeOrder::Manual => "manual",
        TreeOrder::Type => "type",
        TreeOrder::AgeAscending => "age_ascending",
        TreeOrder::AgeDescending => "age_descending",
        TreeOrder::NameAscending => "name_ascending",
        TreeOrder::NameDescending => "name_descending",
    }
}

/// Applies `[keybindings]` overrides onto a copy of `LEADER_BINDINGS`,
/// producing the effective table `keymap::init_effective_bindings`
/// installs. Pure and side-effect-free -- unlike installing the result,
/// which touches `keymap`'s process-lifetime `OnceLock`, this is safe to
/// call from a unit test as many times as needed.
fn merge_keybindings(
    overrides: &HashMap<String, String>,
) -> Result<Vec<KeyBinding>, ConfigLoadError> {
    let mut bindings = LEADER_BINDINGS.to_vec();

    for (action_name, letter_value) in overrides {
        let action = keymap::action_from_name(action_name)
            .ok_or_else(|| ConfigLoadError::UnknownAction(action_name.clone()))?;
        let letter = single_char(letter_value).ok_or_else(|| ConfigLoadError::InvalidLetter {
            action: action_name.clone(),
            value: letter_value.clone(),
        })?;
        if !keymap::BINDABLE_KEYS.contains(&letter) {
            return Err(ConfigLoadError::InvalidLetter {
                action: action_name.clone(),
                value: letter_value.clone(),
            });
        }

        // `LEADER_BINDINGS` has exactly one entry per `Action` (enforced by
        // `action_for`/`action_name` both being total functions over every
        // binding), so this always finds a match.
        if let Some(binding) = bindings.iter_mut().find(|binding| binding.action == action) {
            binding.letter = letter;
        }
    }

    reject_duplicate_letters(&bindings)?;
    Ok(bindings)
}

/// `letter_value` if it is exactly one `char`, else `None` -- rejects both
/// an empty string and a multi-character one (e.g. an accidental `"cc"` or
/// a pasted multi-byte grapheme cluster the single-letter leader dispatch
/// couldn't act on anyway).
fn single_char(letter_value: &str) -> Option<char> {
    let mut chars = letter_value.chars();
    let letter = chars.next()?;
    match chars.next() {
        Some(_) => None,
        None => Some(letter),
    }
}

/// Rejects a binding table where two actions share a letter -- see
/// `ConfigLoadError::DuplicateLetter`'s doc comment.
fn reject_duplicate_letters(bindings: &[KeyBinding]) -> Result<(), ConfigLoadError> {
    let mut seen_letters = HashSet::with_capacity(bindings.len());
    for binding in bindings {
        if !seen_letters.insert(binding.letter) {
            return Err(ConfigLoadError::DuplicateLetter(binding.letter));
        }
    }
    Ok(())
}

/// Applies `[theme]`'s per-color hex overrides onto `scheme`'s preset
/// (`Theme::for_scheme`, itself `[ui].color_scheme`'s resolved value) --
/// pure, same rationale as [`merge_keybindings`] for staying
/// side-effect-free. Layering hex overrides on top of a preset (rather than
/// always starting from `Theme::default()`) is what lets a user pick
/// `color_scheme = "light"` in the settings screen while still hand-tuning
/// one specific color via `[theme]` in a hand-edited `config.toml`.
fn merge_theme(raw: RawThemeConfig, scheme: ColorScheme) -> Result<Theme, ConfigLoadError> {
    let defaults = Theme::for_scheme(scheme);
    Ok(Theme {
        accent_bg: resolve_color("accent_bg", raw.accent_bg, defaults.accent_bg)?,
        accent_fg: resolve_color("accent_fg", raw.accent_fg, defaults.accent_fg)?,
        border_focused: resolve_color(
            "border_focused",
            raw.border_focused,
            defaults.border_focused,
        )?,
        border_unfocused: resolve_color(
            "border_unfocused",
            raw.border_unfocused,
            defaults.border_unfocused,
        )?,
    })
}

/// `parse_hex_color(value)` if `value` is `Some`, else `default` unchanged.
fn resolve_color(
    field: &'static str,
    value: Option<String>,
    default: Color,
) -> Result<Color, ConfigLoadError> {
    match value {
        Some(value) => parse_hex_color(field, &value),
        None => Ok(default),
    }
}

/// Parses a `"#rrggbb"` or `"rrggbb"` hex string into an RGB `Color`.
fn parse_hex_color(field: &'static str, value: &str) -> Result<Color, ConfigLoadError> {
    let hex = value.trim().strip_prefix('#').unwrap_or(value.trim());
    let invalid = || ConfigLoadError::InvalidColor {
        field,
        value: value.to_string(),
    };
    if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(invalid());
    }
    let component =
        |range: std::ops::Range<usize>| u8::from_str_radix(&hex[range], 16).map_err(|_| invalid());
    Ok(Color::Rgb(
        component(0..2)?,
        component(2..4)?,
        component(4..6)?,
    ))
}

/// Persists `ui` into `<config_dir>/config.toml`'s `[ui]` table, preserving
/// every other table (`[keybindings]`, `[keyboard]`, `[theme]`) the file already has
/// rather than overwriting the whole document. Called by `App` every time
/// the settings screen changes a value (`crate::app::Mode::Settings`), so a
/// choice made in one session survives into the next without the user ever
/// hand-editing TOML.
///
/// Round-trips through a generic `toml::Value` rather than a typed struct:
/// [`RawClientConfig`] (this module's *deserialize*-only shape) is
/// deliberately not also given a `Serialize` impl, since its other tables
/// may hold values (or comments, or key ordering) this process never parsed
/// out of them and has no business normalizing. This does not preserve
/// comments or key ordering in a hand-edited file -- ilium ships no
/// commented template `config.toml`, so there is nothing to lose in
/// practice, but a user who hand-annotated their own file should expect an
/// automated write from the settings screen to reformat it.
pub fn save_ui_settings(config_dir: &Path, ui: &UiSettings) -> Result<(), ClientError> {
    let path = config_dir.join("config.toml");
    let mut document = read_toml_document(&path)?;

    let table = document
        .as_table_mut()
        // Invariant: a successfully-parsed TOML document's root is always a
        // table (TOML has no other valid top-level shape); the fresh-file
        // branch above also constructs one directly.
        .expect("a TOML document's root is always a table");
    table.insert("ui".to_string(), ui_settings_to_toml(ui));

    write_toml_document(&path, &document)
}

pub fn save_terminal_settings(
    config_dir: &Path,
    settings: &TerminalSettings,
) -> Result<(), ClientError> {
    save_table(config_dir, "terminal", terminal_settings_to_toml(settings))
}
pub fn save_editor_settings(
    config_dir: &Path,
    settings: &EditorSettings,
) -> Result<(), ClientError> {
    save_table(config_dir, "editor", editor_settings_to_toml(settings))
}
pub fn save_session_settings(
    config_dir: &Path,
    settings: &SessionSettings,
) -> Result<(), ClientError> {
    save_table(config_dir, "session", session_settings_to_toml(settings))
}
fn save_table(config_dir: &Path, name: &str, value: toml::Value) -> Result<(), ClientError> {
    let path = config_dir.join("config.toml");
    let mut document = read_toml_document(&path)?;
    document
        .as_table_mut()
        .expect("a TOML document's root is always a table")
        .insert(name.to_string(), value);
    write_toml_document(&path, &document)
}

/// Persists `[keyboard]` without replacing any unrelated config tables.
pub fn save_keyboard_settings(
    config_dir: &Path,
    keyboard: &KeyboardSettings,
) -> Result<(), ClientError> {
    let path = config_dir.join("config.toml");
    let mut document = read_toml_document(&path)?;
    let table = document
        .as_table_mut()
        .expect("a TOML document's root is always a table");
    table.insert("keyboard".to_string(), keyboard_settings_to_toml(keyboard));
    write_toml_document(&path, &document)
}

/// Persists the complete live keyboard model together so a prefix and its
/// leader map cannot diverge after a preset is selected from Settings.
pub fn save_keymap_settings(
    config_dir: &Path,
    keyboard: &KeyboardSettings,
    bindings: &[KeyBinding],
) -> Result<(), ClientError> {
    reject_duplicate_letters(bindings).map_err(|source| ClientError::ConfigLoad {
        path: config_dir.join("config.toml"),
        source,
    })?;
    let path = config_dir.join("config.toml");
    let mut document = read_toml_document(&path)?;
    let table = document
        .as_table_mut()
        .expect("a TOML document's root is always a table");
    table.insert("keyboard".to_string(), keyboard_settings_to_toml(keyboard));
    let mut keybindings = toml::value::Table::new();
    for binding in bindings {
        keybindings.insert(
            keymap::action_name(binding.action).to_string(),
            toml::Value::String(binding.letter.to_string()),
        );
    }
    table.insert("keybindings".to_string(), toml::Value::Table(keybindings));
    write_toml_document(&path, &document)
}

/// Persists `[kanban_board]` while preserving every unrelated config table.
pub fn save_kanban_board_settings(
    config_dir: &Path,
    kanban_board: &KanbanBoardSettings,
) -> Result<(), ClientError> {
    let path = config_dir.join("config.toml");
    let mut document = read_toml_document(&path)?;
    let table = document
        .as_table_mut()
        .expect("a TOML document's root is always a table");
    table.insert(
        "kanban_board".to_string(),
        kanban_board_settings_to_toml(kanban_board),
    );
    write_toml_document(&path, &document)
}

/// Persists the complete `[sound]` table without replacing settings owned by
/// either the client or server. The server receives the same typed value over
/// IPC for immediate application; this file is the restart/global-session
/// source of truth.
pub fn save_sound_settings(config_dir: &Path, sound: &SoundSettings) -> Result<(), ClientError> {
    let path = config_dir.join("config.toml");
    let mut document = read_toml_document(&path)?;
    let table = document
        .as_table_mut()
        .expect("a TOML document's root is always a table");
    let sound_value = toml::Value::try_from(sound).map_err(|source| ClientError::ConfigSave {
        path: path.clone(),
        source: Box::new(ConfigSaveError::Serialize(source)),
    })?;
    table.insert("sound".to_string(), sound_value);
    write_toml_document(&path, &document)
}

/// Persists only the client-owned `[inference]` table, preserving all server
/// and unrelated client configuration. The settings UI deliberately avoids
/// displaying API-key values after they are entered.
pub fn save_inference_settings(
    config_dir: &Path,
    inference: &InferenceSettings,
) -> Result<(), ClientError> {
    let path = config_dir.join("config.toml");
    let mut document = read_toml_document(&path)?;
    let table = document
        .as_table_mut()
        .expect("a TOML document's root is always a table");
    let value = toml::Value::try_from(inference).map_err(|source| ClientError::ConfigSave {
        path: path.clone(),
        source: Box::new(ConfigSaveError::Serialize(source)),
    })?;
    table.insert("inference".to_string(), value);
    write_toml_document(&path, &document)
}

/// Persists only `[voice]`, preserving every unrelated client and server
/// table. API keys are stored with the same local-file policy as inference
/// provider keys and are never written to logs or tool results.
pub fn save_voice_settings(config_dir: &Path, voice: &VoiceSettings) -> Result<(), ClientError> {
    let path = config_dir.join("config.toml");
    let mut document = read_toml_document(&path)?;
    let table = document
        .as_table_mut()
        .expect("a TOML document's root is always a table");
    let value = toml::Value::try_from(voice).map_err(|source| ClientError::ConfigSave {
        path: path.clone(),
        source: Box::new(ConfigSaveError::Serialize(source)),
    })?;
    table.insert("voice".to_owned(), value);
    write_toml_document(&path, &document)
}

/// Reads and parses `path` as a generic TOML document for [`save_ui_settings`]
/// to merge into -- an absent file starts from an empty table rather than an
/// error, matching [`load`]'s own "no config file yet" handling.
fn read_toml_document(path: &Path) -> Result<toml::Value, ClientError> {
    if !path.exists() {
        return Ok(toml::Value::Table(toml::value::Table::new()));
    }
    let contents = std::fs::read_to_string(path).map_err(|source| ClientError::ConfigSave {
        path: path.to_path_buf(),
        source: Box::new(ConfigSaveError::Read(source)),
    })?;
    toml::from_str(&contents).map_err(|source| ClientError::ConfigSave {
        path: path.to_path_buf(),
        source: Box::new(ConfigSaveError::Parse(source)),
    })
}

/// Serializes and writes a merged config document.
fn write_toml_document(path: &Path, document: &toml::Value) -> Result<(), ClientError> {
    let serialized =
        toml::to_string_pretty(document).map_err(|source| ClientError::ConfigSave {
            path: path.to_path_buf(),
            source: Box::new(ConfigSaveError::Serialize(source)),
        })?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| ClientError::ConfigSave {
            path: path.to_path_buf(),
            source: Box::new(ConfigSaveError::Write(source)),
        })?;
    }
    let temporary_path = path.with_extension(format!("toml.tmp-{}", std::process::id()));
    std::fs::write(&temporary_path, serialized).map_err(|source| ClientError::ConfigSave {
        path: path.to_path_buf(),
        source: Box::new(ConfigSaveError::Write(source)),
    })?;
    std::fs::rename(&temporary_path, path).map_err(|source| {
        let _ = std::fs::remove_file(&temporary_path);
        ClientError::ConfigSave {
            path: path.to_path_buf(),
            source: Box::new(ConfigSaveError::Write(source)),
        }
    })
}

/// Builds the `[ui]` table `save_ui_settings` writes -- the inverse of
/// [`RawUiConfig`]'s deserialization, always fully populated (unlike the
/// `Option`-everything `Raw` shape read at load time) since this always
/// writes `App`'s live, fully-resolved settings.
fn ui_settings_to_toml(ui: &UiSettings) -> toml::Value {
    let mut table = toml::value::Table::new();
    table.insert(
        "auto_resize_tree_on_focus".to_string(),
        toml::Value::Boolean(ui.auto_resize_tree_on_focus),
    );
    table.insert(
        "tree_width".to_string(),
        toml::Value::Integer(i64::from(ui.tree_width)),
    );
    table.insert(
        "color_scheme".to_string(),
        toml::Value::String(color_scheme_name(ui.color_scheme).to_string()),
    );
    table.insert(
        "agent_identifier_mode".to_string(),
        toml::Value::String(agent_identifier_mode_name(ui.agent_identifiers.mode).to_string()),
    );
    table.insert(
        "claude_agent_icon".to_string(),
        toml::Value::String(claude_agent_icon_name(ui.agent_identifiers.claude_icon).to_string()),
    );
    table.insert(
        "codex_agent_icon".to_string(),
        toml::Value::String(codex_agent_icon_name(ui.agent_identifiers.codex_icon).to_string()),
    );
    table.insert(
        "antigravity_agent_icon".to_string(),
        toml::Value::String(
            antigravity_agent_icon_name(ui.agent_identifiers.antigravity_icon).to_string(),
        ),
    );
    table.insert(
        "tree_order".to_string(),
        toml::Value::String(tree_order_name(ui.tree_order).to_string()),
    );
    table.insert(
        "motion_level".to_string(),
        toml::Value::String(
            match ui.motion_level {
                MotionLevel::Full => "full",
                MotionLevel::Reduced => "reduced",
                MotionLevel::Off => "off",
            }
            .to_string(),
        ),
    );
    table.insert(
        "sidebar_density".to_string(),
        toml::Value::String(
            match ui.sidebar_density {
                SidebarDensity::Compact => "compact",
                SidebarDensity::Standard => "standard",
                SidebarDensity::Comfortable => "comfortable",
            }
            .to_string(),
        ),
    );
    table.insert(
        "use_stable_glyphs".to_string(),
        toml::Value::Boolean(ui.use_stable_glyphs),
    );
    table.insert(
        "show_inferred_title_icons".to_string(),
        toml::Value::Boolean(ui.show_inferred_title_icons),
    );
    let icons = IconTarget::ALL
        .into_iter()
        .map(|target| {
            (
                target.key().to_string(),
                toml::Value::String(ui.icons.glyph(target).to_string()),
            )
        })
        .collect();
    table.insert("icons".to_string(), toml::Value::Table(icons));
    toml::Value::Table(table)
}

fn terminal_settings_to_toml(settings: &TerminalSettings) -> toml::Value {
    let mut table = toml::value::Table::new();
    table.insert(
        "scrollback_budget_mib".into(),
        toml::Value::Integer(i64::from(settings.scrollback_budget_mib)),
    );
    table.insert(
        "new_pane_directory".into(),
        toml::Value::String(
            match settings.new_pane_directory {
                NewPaneDirectory::ProjectRoot => "project_root",
                NewPaneDirectory::FocusedTerminal => "focused_terminal",
                NewPaneDirectory::LastUsed => "last_used",
            }
            .into(),
        ),
    );
    toml::Value::Table(table)
}
fn editor_settings_to_toml(settings: &EditorSettings) -> toml::Value {
    let mut table = toml::value::Table::new();
    table.insert(
        "show_line_numbers".into(),
        toml::Value::Boolean(settings.show_line_numbers),
    );
    table.insert(
        "show_minimap".into(),
        toml::Value::Boolean(settings.show_minimap),
    );
    table.insert(
        "autosave_enabled".into(),
        toml::Value::Boolean(settings.autosave_enabled),
    );
    table.insert(
        "autosave_delay_ms".into(),
        toml::Value::Integer(i64::from(settings.autosave_delay_ms)),
    );
    table.insert(
        "markdown_rendered_by_default".into(),
        toml::Value::Boolean(settings.markdown_rendered_by_default),
    );
    toml::Value::Table(table)
}
fn session_settings_to_toml(settings: &SessionSettings) -> toml::Value {
    let mut table = toml::value::Table::new();
    table.insert(
        "recovery_policy".into(),
        toml::Value::String(
            match settings.recovery_policy {
                SessionRecoveryPolicy::RestoreAutomatically => "restore_automatically",
                SessionRecoveryPolicy::AskBeforeRestore => "ask_before_restore",
                SessionRecoveryPolicy::StartFresh => "start_fresh",
            }
            .into(),
        ),
    );
    toml::Value::Table(table)
}

/// Builds the complete `[kanban_board]` table written by the settings UI.
fn kanban_board_settings_to_toml(settings: &KanbanBoardSettings) -> toml::Value {
    let mut table = toml::value::Table::new();
    table.insert(
        "card_preview_lines".to_string(),
        toml::Value::Integer(i64::from(settings.card_preview_lines)),
    );
    table.insert(
        "minimum_column_width".to_string(),
        toml::Value::Integer(i64::from(settings.minimum_column_width)),
    );
    toml::Value::Table(table)
}

/// Builds the complete `[keyboard]` table written by the settings screen.
fn keyboard_settings_to_toml(keyboard: &KeyboardSettings) -> toml::Value {
    let mut table = toml::value::Table::new();
    table.insert(
        "shortcut_base".to_string(),
        toml::Value::String(keyboard.shortcut_base.letter().to_string()),
    );
    toml::Value::Table(table)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir()
            .join("ilium-client-config-tests")
            .join(format!("{:?}", std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        dir
    }

    #[test]
    fn missing_config_file_loads_defaults() {
        let dir = scratch_dir();
        let config = load(&dir).expect("missing file is not an error");
        assert_eq!(config.keybindings.len(), LEADER_BINDINGS.len());
        assert_eq!(config.keyboard, KeyboardSettings::default());
        assert_eq!(config.theme, Theme::default());
        assert_eq!(config.sound, SoundSettings::default());
        assert_eq!(
            config.inference.selected_provider,
            ilium_inference::InferenceProviderKind::KiloGateway
        );
    }

    #[test]
    fn inference_settings_round_trip_without_replacing_server_tables() {
        let dir = scratch_dir();
        std::fs::write(
            dir.join("config.toml"),
            "[detection]\nworking_poll_seconds = 5\n",
        )
        .unwrap();
        let inference = InferenceSettings {
            selected_provider: ilium_inference::InferenceProviderKind::OpenRouter,
            openrouter: ilium_inference::OpenRouterSettings {
                api_key: "test-key".to_string(),
                model: "openrouter/free".to_string(),
            },
            ..InferenceSettings::default()
        };
        save_inference_settings(&dir, &inference).unwrap();

        let saved = std::fs::read_to_string(dir.join("config.toml")).unwrap();
        assert!(saved.contains("[detection]"));
        let loaded = load(&dir).unwrap();
        assert_eq!(loaded.inference, inference);
    }

    #[test]
    fn keyboard_shortcut_base_loads_case_insensitively() {
        let dir = scratch_dir();
        std::fs::write(
            dir.join("config.toml"),
            "[keyboard]\nshortcut_base = \"B\"\n",
        )
        .unwrap();
        let config = load(&dir).expect("valid keyboard config should load");
        assert_eq!(config.keyboard.shortcut_base, ShortcutBase::B);
    }

    #[test]
    fn keyboard_shortcut_base_rejects_non_letters() {
        let dir = scratch_dir();
        std::fs::write(
            dir.join("config.toml"),
            "[keyboard]\nshortcut_base = \"?\"\n",
        )
        .unwrap();
        assert!(matches!(
            load(&dir),
            Err(ClientError::ConfigLoad {
                source: ConfigLoadError::InvalidShortcutBase(_),
                ..
            })
        ));
    }

    #[test]
    fn a_keybinding_override_remaps_only_that_action() {
        let dir = scratch_dir();
        std::fs::write(
            dir.join("config.toml"),
            "[keybindings]\nquit = \"x\"\nclose_pane = \"q\"\n",
        )
        .unwrap();

        let config = load(&dir).expect("valid config should load");
        assert_eq!(
            keymap::action_for_table(&config.keybindings, 'x'),
            Some(keymap::Action::Quit)
        );
        assert_eq!(
            keymap::action_for_table(&config.keybindings, 'q'),
            Some(keymap::Action::ClosePane)
        );
        // Every other binding keeps its default letter.
        assert_eq!(
            keymap::action_for_table(&config.keybindings, 'c'),
            Some(keymap::Action::NewTerminal)
        );
    }

    #[test]
    fn an_unknown_action_name_is_a_clear_config_error() {
        let overrides = HashMap::from([("not_a_real_action".to_string(), "q".to_string())]);
        let result = merge_keybindings(&overrides);
        assert!(matches!(result, Err(ConfigLoadError::UnknownAction(_))));
    }

    #[test]
    fn a_multi_character_letter_is_a_clear_config_error() {
        let overrides = HashMap::from([("quit".to_string(), "qq".to_string())]);
        let result = merge_keybindings(&overrides);
        assert!(matches!(result, Err(ConfigLoadError::InvalidLetter { .. })));
    }

    #[test]
    fn an_empty_letter_is_a_clear_config_error() {
        let overrides = HashMap::from([("quit".to_string(), String::new())]);
        let result = merge_keybindings(&overrides);
        assert!(matches!(result, Err(ConfigLoadError::InvalidLetter { .. })));
    }

    #[test]
    fn remapping_two_actions_onto_the_same_letter_is_a_clear_config_error() {
        let overrides = HashMap::from([
            ("quit".to_string(), "c".to_string()),
            ("new_terminal".to_string(), "c".to_string()),
        ]);
        let result = merge_keybindings(&overrides);
        assert!(matches!(result, Err(ConfigLoadError::DuplicateLetter('c'))));
    }

    #[test]
    fn a_theme_override_replaces_only_the_specified_color() {
        let dir = scratch_dir();
        std::fs::write(
            dir.join("config.toml"),
            "[theme]\naccent_bg = \"#ff0000\"\n",
        )
        .unwrap();

        let config = load(&dir).expect("valid config should load");
        assert_eq!(config.theme.accent_bg, Color::Rgb(0xff, 0x00, 0x00));
        // Everything else keeps its default.
        assert_eq!(config.theme.accent_fg, Theme::default().accent_fg);
        assert_eq!(config.theme.border_focused, Theme::default().border_focused);
    }

    #[test]
    fn a_theme_override_accepts_a_hex_string_without_a_leading_hash() {
        let dir = scratch_dir();
        std::fs::write(dir.join("config.toml"), "[theme]\naccent_fg = \"00ff00\"\n").unwrap();

        let config = load(&dir).expect("valid config should load");
        assert_eq!(config.theme.accent_fg, Color::Rgb(0x00, 0xff, 0x00));
    }

    #[test]
    fn an_invalid_hex_color_is_a_clear_config_error() {
        let dir = scratch_dir();
        std::fs::write(
            dir.join("config.toml"),
            "[theme]\naccent_bg = \"not-a-color\"\n",
        )
        .unwrap();

        let result = load(&dir);
        assert!(matches!(
            result,
            Err(ClientError::ConfigLoad {
                source: ConfigLoadError::InvalidColor { .. },
                ..
            })
        ));
    }

    #[test]
    fn malformed_toml_is_a_config_load_error_not_a_panic() {
        let dir = scratch_dir();
        std::fs::write(dir.join("config.toml"), "not valid [ toml").unwrap();

        let result = load(&dir);
        assert!(matches!(
            result,
            Err(ClientError::ConfigLoad {
                source: ConfigLoadError::Parse(_),
                ..
            })
        ));
    }

    #[test]
    fn missing_ui_table_loads_ui_defaults() {
        let dir = scratch_dir();
        let config = load(&dir).expect("missing file is not an error");
        assert_eq!(config.ui, UiSettings::default());
    }

    #[test]
    fn a_ui_override_replaces_only_the_specified_field() {
        let dir = scratch_dir();
        std::fs::write(dir.join("config.toml"), "[ui]\ntree_width = 40\n").unwrap();

        let config = load(&dir).expect("valid config should load");
        assert_eq!(config.ui.tree_width, 40);
        assert_eq!(
            config.ui.auto_resize_tree_on_focus,
            UiSettings::default().auto_resize_tree_on_focus
        );
        assert_eq!(config.ui.color_scheme, ColorScheme::Dark);
        assert_eq!(
            config.ui.agent_identifiers,
            AgentIdentifierSettings::default()
        );
    }

    #[test]
    fn agent_identifier_settings_load_every_curated_choice() {
        for mode in AgentIdentifierMode::ALL {
            assert_eq!(
                parse_agent_identifier_mode(agent_identifier_mode_name(mode)).unwrap(),
                mode
            );
        }
        for icon in ClaudeAgentIcon::ALL {
            assert_eq!(
                parse_claude_agent_icon(claude_agent_icon_name(icon)).unwrap(),
                icon
            );
        }
        for icon in CodexAgentIcon::ALL {
            assert_eq!(
                parse_codex_agent_icon(codex_agent_icon_name(icon)).unwrap(),
                icon
            );
        }
        for icon in AntigravityAgentIcon::ALL {
            assert_eq!(
                parse_antigravity_agent_icon(antigravity_agent_icon_name(icon)).unwrap(),
                icon
            );
        }
    }

    #[test]
    fn invalid_agent_identifier_values_are_clear_config_errors() {
        let dir = scratch_dir();
        std::fs::write(
            dir.join("config.toml"),
            "[ui]\nagent_identifier_mode = \"emoji_soup\"\n",
        )
        .unwrap();
        assert!(matches!(
            load(&dir),
            Err(ClientError::ConfigLoad {
                source: ConfigLoadError::InvalidAgentIdentifierMode(_),
                ..
            })
        ));

        std::fs::write(
            dir.join("config.toml"),
            "[ui]\nclaude_agent_icon = \"robot\"\n",
        )
        .unwrap();
        assert!(matches!(
            load(&dir),
            Err(ClientError::ConfigLoad {
                source: ConfigLoadError::InvalidClaudeAgentIcon(_),
                ..
            })
        ));

        std::fs::write(
            dir.join("config.toml"),
            "[ui]\ncodex_agent_icon = \"robot\"\n",
        )
        .unwrap();
        assert!(matches!(
            load(&dir),
            Err(ClientError::ConfigLoad {
                source: ConfigLoadError::InvalidCodexAgentIcon(_),
                ..
            })
        ));

        std::fs::write(
            dir.join("config.toml"),
            "[ui]\nantigravity_agent_icon = \"robot\"\n",
        )
        .unwrap();
        assert!(matches!(
            load(&dir),
            Err(ClientError::ConfigLoad {
                source: ConfigLoadError::InvalidAntigravityAgentIcon(_),
                ..
            })
        ));
    }

    #[test]
    fn agent_identifier_controls_wrap_in_both_directions() {
        assert_eq!(
            AgentIdentifierMode::FullName.stepped(-1),
            AgentIdentifierMode::Hidden
        );
        assert_eq!(
            AgentIdentifierMode::Hidden.stepped(1),
            AgentIdentifierMode::FullName
        );
        assert_eq!(ClaudeAgentIcon::Brain.stepped(-1), ClaudeAgentIcon::Lobster);
        assert_eq!(CodexAgentIcon::CrossMark.stepped(1), CodexAgentIcon::Gear);
        assert_eq!(
            AntigravityAgentIcon::Spark.stepped(-1),
            AntigravityAgentIcon::Atom
        );
    }

    #[test]
    fn tree_order_settings_parse_every_mode_and_wrap_in_both_directions() {
        for tree_order in TreeOrder::ALL {
            assert_eq!(
                parse_tree_order(tree_order_name(tree_order)).unwrap(),
                tree_order
            );
        }
        assert_eq!(TreeOrder::Manual.stepped(-1), TreeOrder::NameDescending);
        assert_eq!(TreeOrder::NameDescending.stepped(1), TreeOrder::Manual);
    }

    #[test]
    fn invalid_tree_order_is_a_clear_config_error() {
        let dir = scratch_dir();
        std::fs::write(dir.join("config.toml"), "[ui]\ntree_order = \"random\"\n").unwrap();

        assert!(matches!(
            load(&dir),
            Err(ClientError::ConfigLoad {
                source: ConfigLoadError::InvalidTreeOrder(_),
                ..
            })
        ));
    }

    #[test]
    fn ui_color_scheme_selects_the_matching_theme_preset() {
        let dir = scratch_dir();
        std::fs::write(dir.join("config.toml"), "[ui]\ncolor_scheme = \"light\"\n").unwrap();

        let config = load(&dir).expect("valid config should load");
        assert_eq!(config.ui.color_scheme, ColorScheme::Light);
        assert_eq!(config.theme, Theme::light());
    }

    #[test]
    fn a_theme_hex_override_layers_on_top_of_the_selected_preset() {
        let dir = scratch_dir();
        std::fs::write(
            dir.join("config.toml"),
            "[ui]\ncolor_scheme = \"light\"\n[theme]\naccent_bg = \"#ff0000\"\n",
        )
        .unwrap();

        let config = load(&dir).expect("valid config should load");
        assert_eq!(config.theme.accent_bg, Color::Rgb(0xff, 0x00, 0x00));
        assert_eq!(config.theme.border_focused, Theme::light().border_focused);
    }

    #[test]
    fn a_tree_width_below_the_minimum_is_a_clear_config_error() {
        let dir = scratch_dir();
        std::fs::write(dir.join("config.toml"), "[ui]\ntree_width = 1\n").unwrap();

        let result = load(&dir);
        assert!(matches!(
            result,
            Err(ClientError::ConfigLoad {
                source: ConfigLoadError::InvalidTreeWidth(1),
                ..
            })
        ));
    }

    #[test]
    fn an_unknown_color_scheme_is_a_clear_config_error() {
        let dir = scratch_dir();
        std::fs::write(dir.join("config.toml"), "[ui]\ncolor_scheme = \"purple\"\n").unwrap();

        let result = load(&dir);
        assert!(matches!(
            result,
            Err(ClientError::ConfigLoad {
                source: ConfigLoadError::InvalidColorScheme(_),
                ..
            })
        ));
    }

    #[test]
    fn color_scheme_parsing_is_case_insensitive() {
        assert_eq!(parse_color_scheme("DARK").unwrap(), ColorScheme::Dark);
        assert_eq!(parse_color_scheme("Light").unwrap(), ColorScheme::Light);
    }

    #[test]
    fn save_ui_settings_round_trips_through_load() {
        let dir = scratch_dir();
        let ui = UiSettings {
            auto_resize_tree_on_focus: false,
            tree_width: 24,
            color_scheme: ColorScheme::Light,
            agent_identifiers: AgentIdentifierSettings {
                mode: AgentIdentifierMode::Icon,
                claude_icon: ClaudeAgentIcon::Lobster,
                codex_icon: CodexAgentIcon::Dna,
                antigravity_icon: AntigravityAgentIcon::Orbit,
            },
            tree_order: TreeOrder::NameDescending,
            motion_level: MotionLevel::Reduced,
            sidebar_density: SidebarDensity::Comfortable,
            show_inferred_title_icons: true,
            use_stable_glyphs: true,
            icons: IconSettings::default(),
        };
        save_ui_settings(&dir, &ui).expect("save should succeed");

        let config = load(&dir).expect("saved config should load back");
        assert_eq!(config.ui, ui);
    }

    #[test]
    fn stable_glyphs_are_opt_in_and_default_to_normal_icons() {
        let dir = scratch_dir();
        assert!(
            !load(&dir)
                .expect("missing config should use defaults")
                .ui
                .use_stable_glyphs
        );

        std::fs::write(dir.join("config.toml"), "[ui]\nuse_stable_glyphs = true\n")
            .expect("write UI preference");
        assert!(
            load(&dir)
                .expect("stable glyph preference should load")
                .ui
                .use_stable_glyphs
        );
    }

    #[test]
    fn save_ui_settings_preserves_an_existing_keybindings_table() {
        let dir = scratch_dir();
        std::fs::write(dir.join("config.toml"), "[keybindings]\nquit = \"z\"\n").unwrap();

        save_ui_settings(&dir, &UiSettings::default()).expect("save should succeed");

        let config = load(&dir).expect("saved config should load back");
        assert_eq!(
            keymap::action_for_table(&config.keybindings, 'z'),
            Some(keymap::Action::Quit)
        );
        assert_eq!(config.ui, UiSettings::default());
    }

    #[test]
    fn kanban_board_uses_three_preview_lines_and_twenty_column_width_by_default() {
        let dir = scratch_dir();

        let config = load(&dir).expect("missing config should use defaults");

        assert_eq!(config.kanban_board.card_preview_lines, 3);
        assert_eq!(config.kanban_board.minimum_column_width, 20);
    }

    #[test]
    fn kanban_board_preview_lines_validate_and_round_trip() {
        let dir = scratch_dir();
        let settings = KanbanBoardSettings {
            card_preview_lines: 10,
            minimum_column_width: 37,
        };

        save_kanban_board_settings(&dir, &settings).expect("board settings should save");
        let config = load(&dir).expect("saved board settings should load");

        assert_eq!(config.kanban_board, settings);

        std::fs::write(
            dir.join("config.toml"),
            "[kanban_board]\ncard_preview_lines = 11\n",
        )
        .unwrap();
        assert!(matches!(
            load(&dir),
            Err(ClientError::ConfigLoad {
                source: ConfigLoadError::InvalidCardPreviewLines(11),
                ..
            })
        ));

        std::fs::write(
            dir.join("config.toml"),
            "[kanban_board]\nminimum_column_width = 9\n",
        )
        .unwrap();
        assert!(matches!(
            load(&dir),
            Err(ClientError::ConfigLoad {
                source: ConfigLoadError::InvalidBoardColumnWidth(9),
                ..
            })
        ));
    }

    #[test]
    fn save_keyboard_settings_round_trips_and_preserves_other_tables() {
        let dir = scratch_dir();
        std::fs::write(
            dir.join("config.toml"),
            "[ui]\ntree_width = 40\n[keybindings]\nquit = \"z\"\n",
        )
        .unwrap();
        let keyboard = KeyboardSettings {
            shortcut_base: ShortcutBase::B,
        };
        save_keyboard_settings(&dir, &keyboard).expect("keyboard save should succeed");

        let config = load(&dir).expect("saved keyboard config should load back");
        assert_eq!(config.keyboard, keyboard);
        assert_eq!(config.ui.tree_width, 40);
        assert_eq!(
            keymap::action_for_table(&config.keybindings, 'z'),
            Some(keymap::Action::Quit)
        );
    }

    #[test]
    fn save_keymap_settings_round_trips_prefix_and_every_action_binding() {
        let dir = scratch_dir();
        let keyboard = KeyboardSettings {
            shortcut_base: ShortcutBase::B,
        };
        let bindings = keymap::preset_bindings(keymap::KeymapPreset::Tmux);

        save_keymap_settings(&dir, &keyboard, &bindings).expect("keymap save should succeed");

        let config = load(&dir).expect("saved keymap should load back");
        assert_eq!(config.keyboard, keyboard);
        assert_eq!(config.keybindings, bindings);
    }

    #[test]
    fn sound_config_loads_partial_events_from_defaults() {
        let dir = scratch_dir();
        std::fs::write(
            dir.join("config.toml"),
            concat!(
                "[sound]\n",
                "source = \"sound_file\"\n",
                "file = \"/usr/share/sounds/complete.oga\"\n",
                "[sound.events]\n",
                "approval_required = true\n",
            ),
        )
        .unwrap();

        let config = load(&dir).expect("valid sound config should load");
        assert_eq!(config.sound.source, ilium_sound::SoundSourceKind::SoundFile);
        assert!(config.sound.events.agent_finished);
        assert!(config.sound.events.approval_required);
        assert!(!config.sound.events.agent_started);
    }

    #[test]
    fn save_sound_settings_round_trips_and_preserves_server_tables() {
        let dir = scratch_dir();
        std::fs::write(
            dir.join("config.toml"),
            "[detection]\nworking_poll_seconds = 3\n[notifications]\nenabled = false\n",
        )
        .unwrap();
        let sound = SoundSettings {
            source: ilium_sound::SoundSourceKind::SoundFile,
            file: Some(std::path::PathBuf::from(
                "/usr/share/sounds/freedesktop/stereo/complete.oga",
            )),
            events: ilium_sound::SoundEventSettings {
                approval_required: true,
                ..ilium_sound::SoundEventSettings::default()
            },
        };

        save_sound_settings(&dir, &sound).expect("sound save should succeed");

        let config = load(&dir).expect("saved sound config should load back");
        assert_eq!(config.sound, sound);
        let raw = std::fs::read_to_string(dir.join("config.toml")).unwrap();
        assert!(raw.contains("working_poll_seconds = 3"));
        assert!(raw.contains("enabled = false"));
    }

    #[test]
    fn voice_settings_round_trip_and_reject_invalid_volume_without_replacing_other_tables() {
        let dir = scratch_dir();
        std::fs::write(
            dir.join("config.toml"),
            "[notifications]\nenabled = false\n",
        )
        .unwrap();
        let voice = VoiceSettings {
            enabled: true,
            api_key: "secret-test-key".to_owned(),
            model: VoiceModel::GptRealtimeMini,
            voice: VoiceName::Cedar,
            reasoning_effort: ReasoningEffort::Medium,
            input_mode: VoiceInputMode::PushToTalk,
            vad_eagerness: VadEagerness::Low,
            input_device_name: Some("Studio microphone".to_owned()),
            output_device_name: Some("Desk speakers".to_owned()),
            output_volume_percent: 65,
            confirm_terminal_submissions: true,
            custom_prompt: "Prefer terse confirmations.\nNever guess targets.".to_owned(),
        };

        save_voice_settings(&dir, &voice).expect("voice settings should save");
        let config = load(&dir).expect("saved voice settings should load");
        assert_eq!(config.voice, voice);
        let raw = std::fs::read_to_string(dir.join("config.toml")).unwrap();
        assert!(raw.contains("[notifications]"));
        assert!(raw.contains("enabled = false"));
        assert!(raw.contains("confirm_terminal_submissions = true"));

        std::fs::write(
            dir.join("config.toml"),
            "[voice]\noutput_volume_percent = 101\n",
        )
        .unwrap();
        assert!(matches!(
            load(&dir),
            Err(ClientError::ConfigLoad {
                source: ConfigLoadError::InvalidVoiceOutputVolume(101),
                ..
            })
        ));
    }

    #[test]
    fn terminal_submission_confirmation_defaults_off_and_is_not_runtime_configuration() {
        let base = VoiceSettings::default();
        let mut policy_changed = base.clone();
        policy_changed.confirm_terminal_submissions = true;
        let mut prompt_changed = base.clone();
        prompt_changed.custom_prompt = "Use a calm voice".to_owned();

        assert!(!base.confirm_terminal_submissions);
        assert!(base.has_same_runtime_configuration(&policy_changed));
        assert!(!base.has_same_runtime_configuration(&prompt_changed));
    }
}

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

use ratatui::style::Color;
use serde::Deserialize;

use crate::error::ClientError;
use crate::keymap::{self, KeyBinding, ShortcutBase, LEADER_BINDINGS};
use crate::layout::{DEFAULT_TREE_WIDTH, MAX_TREE_WIDTH, MIN_TREE_WIDTH};
use crate::theme::{ColorScheme, Theme};

/// The three client-side config tables, already validated. What [`load`]
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
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            keybindings: LEADER_BINDINGS.to_vec(),
            keyboard: KeyboardSettings::default(),
            theme: Theme::default(),
            ui: UiSettings::default(),
        }
    }
}

/// `[keyboard]` settings, validated before they reach input dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct KeyboardSettings {
    pub shortcut_base: ShortcutBase,
}

/// `[ui]`'s settings, already validated -- what the settings screen reads
/// from and writes to (via `App`'s live copy, persisted with
/// [`save_ui_settings`]). Read this crate's `CLAUDE.md`-style reminder in
/// `crate::app`'s `SettingsState` doc comment before adding a fourth entry
/// here: every new setting needs a `Raw` field, a validated field here, a
/// default, a settings-screen row, and a `[ui]` table key -- in that order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
}

impl Default for UiSettings {
    fn default() -> Self {
        Self {
            auto_resize_tree_on_focus: true,
            tree_width: DEFAULT_TREE_WIDTH,
            color_scheme: ColorScheme::Dark,
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
}

/// `[keyboard]`'s optional on-disk shape.
#[derive(Debug, Default, Deserialize)]
struct RawKeyboardConfig {
    shortcut_base: Option<String>,
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

/// Loads `<config_dir>/config.toml`'s `[keybindings]`/`[theme]` tables. A
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
    Ok(UiSettings {
        auto_resize_tree_on_focus: raw
            .auto_resize_tree_on_focus
            .unwrap_or(defaults.auto_resize_tree_on_focus),
        tree_width,
        color_scheme,
    })
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
/// every other table (`[keybindings]`, `[theme]`) the file already has
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
    table.insert(
        "keyboard".to_string(),
        keyboard_settings_to_toml(keyboard),
    );
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
    std::fs::write(path, serialized).map_err(|source| ClientError::ConfigSave {
        path: path.to_path_buf(),
        source: Box::new(ConfigSaveError::Write(source)),
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
        assert_eq!(config.theme, Theme::default());
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
        };
        save_ui_settings(&dir, &ui).expect("save should succeed");

        let config = load(&dir).expect("saved config should load back");
        assert_eq!(config.ui, ui);
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
}

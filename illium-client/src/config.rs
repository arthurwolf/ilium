//! illium-client's own `~/.config/illium/config.toml` tables:
//! `[keybindings]` (remap which letter triggers an existing
//! `keymap::Action`) and `[theme]` (override a handful of `theme::Theme`'s
//! colors). Server-side config (`illium-server/src/config.rs`: poll
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
use crate::keymap::{self, KeyBinding, LEADER_BINDINGS};
use crate::theme::Theme;

/// The two client-side config tables, already validated. What [`load`]
/// returns; a config file that fails to load falls back to
/// [`ClientConfig::default`] at the call site (`crate::run`) rather than
/// this module hardcoding that fallback here -- mirrors
/// `illium_server::config::ServerConfig`'s own doc comment.
#[derive(Debug, Clone)]
pub struct ClientConfig {
    /// The effective leader-key binding table -- `LEADER_BINDINGS` with any
    /// `[keybindings]` overrides applied.
    pub keybindings: Vec<KeyBinding>,
    pub theme: Theme,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            keybindings: LEADER_BINDINGS.to_vec(),
            theme: Theme::default(),
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
    theme: RawThemeConfig,
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

/// Why `~/.config/illium/config.toml`'s client-side tables could not be
/// loaded -- kept separate from `ClientError::ConfigLoad`'s `path` field so
/// the two concerns (which file, why it failed) stay independently
/// testable, mirroring `illium_server::error::ConfigLoadError`.
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
    /// Two actions ended up bound to the same letter after applying every
    /// override -- `action_for` can only ever dispatch one of them, so this
    /// is rejected rather than silently picking whichever comes first in
    /// table order.
    #[error("keybindings config binds more than one action to the letter {0:?}")]
    DuplicateLetter(char),
    /// A `[theme]` value isn't a valid `#rrggbb`/`rrggbb` hex color.
    #[error("theme.{field:?} = {value:?} is not a valid #rrggbb hex color")]
    InvalidColor { field: &'static str, value: String },
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
    let theme = merge_theme(raw.theme).map_err(|source| ClientError::ConfigLoad {
        path: path.clone(),
        source,
    })?;

    Ok(ClientConfig { keybindings, theme })
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

/// Applies `[theme]` overrides onto [`Theme::default`]. Pure, same
/// rationale as [`merge_keybindings`] for staying side-effect-free.
fn merge_theme(raw: RawThemeConfig) -> Result<Theme, ConfigLoadError> {
    let defaults = Theme::default();
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

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir()
            .join("illium-client-config-tests")
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
}

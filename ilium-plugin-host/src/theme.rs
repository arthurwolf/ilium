//! Built-in and user themes with contrast checks (guide-accessibility, USR-301, USR-302).

use ilium_plugin_api::PluginError;

/// A colour role of a theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Errors.
    Danger,
    /// Warnings.
    Warning,
    /// Successes.
    Success,
    /// Secondary text.
    Muted,
    /// Accents.
    Accent,
    /// Informational text.
    Info,
}

impl Role {
    /// Every role.
    pub const ALL: [Role; 6] = [
        Role::Danger,
        Role::Warning,
        Role::Success,
        Role::Muted,
        Role::Accent,
        Role::Info,
    ];
}

/// An RGB colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

/// A theme.
#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    _private: (),
}

impl Theme {
    /// The theme name.
    pub fn name(&self) -> &str {
        unimplemented!("host::theme::Theme::name")
    }

    /// The contrast ratio of a role against the background.
    pub fn contrast_ratio(&self, _role: Role) -> f64 {
        unimplemented!("host::theme::Theme::contrast_ratio")
    }

    /// The high-contrast variant of this theme.
    pub fn high_contrast(&self) -> Theme {
        unimplemented!("host::theme::Theme::high_contrast")
    }

    /// The colour of a role.
    pub fn role_color(&self, _role: Role) -> Rgb {
        unimplemented!("host::theme::Theme::role_color")
    }

    /// The foreground colour.
    pub fn foreground(&self) -> Rgb {
        unimplemented!("host::theme::Theme::foreground")
    }

    /// The extras passed to plugins.
    pub fn extras(&self) -> serde_json::Value {
        unimplemented!("host::theme::Theme::extras")
    }
}

/// Every built-in theme.
pub fn builtin_themes() -> Vec<Theme> {
    unimplemented!("host::theme::builtin_themes")
}

/// Loads a theme from TOML text; refuses a theme below the contrast floor.
pub fn load_theme(_toml: &str) -> Result<Theme, PluginError> {
    unimplemented!("host::theme::load_theme")
}

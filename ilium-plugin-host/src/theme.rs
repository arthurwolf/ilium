//! Built-in and user themes with contrast checks (guide-accessibility, USR-301, USR-302).

use ilium_plugin_api::error::Code;
use ilium_plugin_api::{ErrorCode, PluginError};

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

impl Role {
    /// The lowercase name used in theme files and refusal messages.
    pub fn name(self) -> &'static str {
        match self {
            Role::Danger => "danger",
            Role::Warning => "warning",
            Role::Success => "success",
            Role::Muted => "muted",
            Role::Accent => "accent",
            Role::Info => "info",
        }
    }

    fn index(self) -> usize {
        match self {
            Role::Danger => 0,
            Role::Warning => 1,
            Role::Success => 2,
            Role::Muted => 3,
            Role::Accent => 4,
            Role::Info => 5,
        }
    }
}

/// Minimum contrast ratio of every role (USR-301).
const CONTRAST_FLOOR: f64 = 4.5;
/// Minimum contrast ratio in high-contrast mode (USR-302).
const HIGH_CONTRAST_FLOOR: f64 = 7.0;

/// A theme.
#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    name: String,
    background: Rgb,
    foreground: Rgb,
    colors: [Rgb; 6],
    is_high_contrast: bool,
}

fn channel_luminance(channel: u8) -> f64 {
    let value = f64::from(channel) / 255.0;
    if value <= 0.03928 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

fn relative_luminance(color: Rgb) -> f64 {
    0.2126 * channel_luminance(color.0)
        + 0.7152 * channel_luminance(color.1)
        + 0.0722 * channel_luminance(color.2)
}

/// WCAG 2.x contrast ratio between two colours.
fn contrast_between(first: Rgb, second: Rgb) -> f64 {
    let first_luminance = relative_luminance(first);
    let second_luminance = relative_luminance(second);
    let (lighter, darker) = if first_luminance >= second_luminance {
        (first_luminance, second_luminance)
    } else {
        (second_luminance, first_luminance)
    };
    (lighter + 0.05) / (darker + 0.05)
}

fn blend_toward(from: Rgb, toward: Rgb, step_percent: u32) -> Rgb {
    let mix = |start: u8, end: u8| -> u8 {
        let start = i64::from(start);
        let end = i64::from(end);
        (start + (end - start) * i64::from(step_percent) / 100) as u8
    };
    Rgb(mix(from.0, toward.0), mix(from.1, toward.1), mix(from.2, toward.2))
}

impl Theme {
    /// The theme name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The contrast ratio of a role against the background.
    pub fn contrast_ratio(&self, role: Role) -> f64 {
        contrast_between(self.role_color(role), self.background)
    }

    /// The high-contrast variant of this theme: every role at 7:1 or more, muted text in the
    /// foreground colour (USR-302). A role never gets lower contrast than before.
    pub fn high_contrast(&self) -> Theme {
        let mut high = self.clone();
        high.is_high_contrast = true;
        for role in Role::ALL {
            let mut color = self.role_color(role);
            if role == Role::Muted {
                color = self.foreground;
            } else {
                let mut step = 0;
                while contrast_between(color, self.background) < HIGH_CONTRAST_FLOOR && step < 100 {
                    step += 10;
                    color = blend_toward(self.role_color(role), self.foreground, step);
                }
            }
            high.colors[role.index()] = color;
        }
        high
    }

    /// The colour of a role.
    pub fn role_color(&self, role: Role) -> Rgb {
        self.colors[role.index()]
    }

    /// The foreground colour.
    pub fn foreground(&self) -> Rgb {
        self.foreground
    }

    /// The extras passed to plugins.
    pub fn extras(&self) -> serde_json::Value {
        serde_json::json!({
            "contrast": if self.is_high_contrast { "high" } else { "normal" },
            "theme": self.name,
        })
    }
}

fn dark_theme() -> Theme {
    Theme {
        name: "ilium-dark".to_string(),
        background: Rgb(0, 0, 0),
        foreground: Rgb(255, 255, 255),
        colors: [
            Rgb(255, 100, 100),
            Rgb(255, 200, 0),
            Rgb(80, 220, 80),
            Rgb(150, 150, 150),
            Rgb(120, 160, 255),
            Rgb(90, 200, 255),
        ],
        is_high_contrast: false,
    }
}

fn light_theme() -> Theme {
    Theme {
        name: "ilium-light".to_string(),
        background: Rgb(255, 255, 255),
        foreground: Rgb(0, 0, 0),
        colors: [
            Rgb(178, 0, 0),
            Rgb(140, 90, 0),
            Rgb(0, 110, 0),
            Rgb(100, 100, 100),
            Rgb(0, 70, 200),
            Rgb(0, 90, 140),
        ],
        is_high_contrast: false,
    }
}

/// Every built-in theme.
pub fn builtin_themes() -> Vec<Theme> {
    vec![dark_theme(), light_theme()]
}

fn theme_error(code: &'static str, message: &str) -> PluginError {
    PluginError::with_prefix(ErrorCode::InvalidArgument, &Code::from_static(code), message)
}

fn parse_hex_color(text: &str) -> Option<Rgb> {
    let digits = text.strip_prefix('#')?;
    if digits.len() != 6 || !digits.is_ascii() {
        return None;
    }
    let channel = |range: std::ops::Range<usize>| u8::from_str_radix(&digits[range], 16).ok();
    Some(Rgb(channel(0..2)?, channel(2..4)?, channel(4..6)?))
}

/// Loads a theme from TOML text; refuses a theme below the contrast floor.
pub fn load_theme(toml_text: &str) -> Result<Theme, PluginError> {
    let document: toml::Table = toml::from_str(toml_text)
        .map_err(|error| theme_error("theme.invalid", &format!("theme is not valid TOML: {error}")))?;
    let mut theme = dark_theme();
    if let Some(name) = document
        .get("theme")
        .and_then(|table| table.get("name"))
        .and_then(|value| value.as_str())
    {
        theme.name = name.to_string();
    }
    if let Some(colors) = document.get("colors").and_then(|value| value.as_table()) {
        for role in Role::ALL {
            let Some(value) = colors.get(role.name()) else {
                continue;
            };
            let color = value.as_str().and_then(parse_hex_color).ok_or_else(|| {
                theme_error(
                    "theme.invalid",
                    &format!("theme {:?} role {} is not a #rrggbb colour", theme.name, role.name()),
                )
            })?;
            theme.colors[role.index()] = color;
        }
    }
    for role in Role::ALL {
        let ratio = theme.contrast_ratio(role);
        if ratio < CONTRAST_FLOOR {
            return Err(theme_error(
                "theme.low-contrast",
                &format!(
                    "theme {:?} role {} has contrast {ratio:.2}:1, below the {CONTRAST_FLOOR}:1 floor",
                    theme.name,
                    role.name()
                ),
            ));
        }
    }
    Ok(theme)
}

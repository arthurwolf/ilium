//! User settings of the attention queue (`[attention]` in `config.toml`).
//!
//! Pure value types: persistence lives in `config.rs`, the Settings rows in
//! `value_settings*.rs`. Every numeric field is clamped by
//! [`AttentionSettings::normalized`], which the loader and every writer call,
//! so a hand-edited file cannot put the queue in an unusable state.

use serde::{Deserialize, Serialize};

/// How the attention queue is fed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AttentionMode {
    /// The queue is hidden and nothing is computed.
    Off,
    /// Only what agent detection already knows. No LLM is ever called.
    InstantOnly,
    /// The attention question rides on the Sort (restructure) call.
    #[default]
    Shared,
    /// One parallel LLM call per agent, bounded by `turbo_concurrency`.
    Turbo,
}

impl AttentionMode {
    pub const ALL: [AttentionMode; 4] = [
        AttentionMode::Off,
        AttentionMode::InstantOnly,
        AttentionMode::Shared,
        AttentionMode::Turbo,
    ];

    /// Label shown in the Settings choice dialog.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::InstantOnly => "Instant only (no LLM)",
            Self::Shared => "Shared with Sort",
            Self::Turbo => "Turbo (parallel LLM)",
        }
    }

    /// Stable machine name used by the control API and the snapshot.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::InstantOnly => "instant_only",
            Self::Shared => "shared",
            Self::Turbo => "turbo",
        }
    }

    /// Parses a control-API value: the machine name, case-insensitive, with
    /// `-` or space accepted for `_`.
    pub fn parse(text: &str) -> Option<Self> {
        let normalized = text.trim().to_ascii_lowercase().replace(['-', ' '], "_");
        Self::ALL.into_iter().find(|mode| mode.name() == normalized)
    }

    /// Whether this mode may call an LLM at all.
    pub const fn uses_llm(self) -> bool {
        matches!(self, Self::Shared | Self::Turbo)
    }
}

pub const TURBO_CONCURRENCY_MIN: u8 = 1;
pub const TURBO_CONCURRENCY_MAX: u8 = 64;
pub const EXPIRY_SECONDS_MIN: u32 = 60;
pub const EXPIRY_SECONDS_MAX: u32 = 86_400;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AttentionSettings {
    pub mode: AttentionMode,
    /// Pressing Sort also refreshes the attention queue.
    pub press_sorts: bool,
    /// Parallel LLM calls in flight in Turbo mode.
    pub turbo_concurrency: u8,
    /// An item no longer supported by the live state is dropped after this long.
    pub expiry_seconds: u32,
    /// Model for attention calls. `None` means the same model as titles.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_override: Option<String>,
}

impl Default for AttentionSettings {
    fn default() -> Self {
        Self {
            mode: AttentionMode::Shared,
            press_sorts: true,
            turbo_concurrency: 16,
            expiry_seconds: 1_800,
            model_override: None,
        }
    }
}

impl AttentionSettings {
    /// Clamps numbers into their ranges and turns a blank model into `None`.
    #[must_use]
    pub fn normalized(mut self) -> Self {
        self.turbo_concurrency = self
            .turbo_concurrency
            .clamp(TURBO_CONCURRENCY_MIN, TURBO_CONCURRENCY_MAX);
        self.expiry_seconds = self
            .expiry_seconds
            .clamp(EXPIRY_SECONDS_MIN, EXPIRY_SECONDS_MAX);
        self.model_override = self
            .model_override
            .map(|model| model.trim().to_owned())
            .filter(|model| !model.is_empty());
        self
    }

    /// Sets the model override, normalising an empty string to `None`.
    pub fn set_model_override(&mut self, model: Option<String>) {
        self.model_override = model
            .map(|model| model.trim().to_owned())
            .filter(|model| !model.is_empty());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_decided_values() {
        let settings = AttentionSettings::default();
        assert_eq!(settings.mode, AttentionMode::Shared);
        assert!(settings.press_sorts);
        assert_eq!(settings.turbo_concurrency, 16);
        assert_eq!(settings.expiry_seconds, 1_800);
        assert_eq!(settings.model_override, None);
        assert_eq!(settings.clone().normalized(), settings);
    }

    #[test]
    fn normalized_clamps_both_ends_and_blanks_the_model() {
        let low = AttentionSettings {
            turbo_concurrency: 0,
            expiry_seconds: 1,
            model_override: Some("   ".to_owned()),
            ..AttentionSettings::default()
        }
        .normalized();
        assert_eq!(low.turbo_concurrency, 1);
        assert_eq!(low.expiry_seconds, 60);
        assert_eq!(low.model_override, None);
        let high = AttentionSettings {
            turbo_concurrency: 200,
            expiry_seconds: u32::MAX,
            model_override: Some(" gpt-x ".to_owned()),
            ..AttentionSettings::default()
        }
        .normalized();
        assert_eq!(high.turbo_concurrency, 64);
        assert_eq!(high.expiry_seconds, 86_400);
        assert_eq!(high.model_override.as_deref(), Some("gpt-x"));
    }

    #[test]
    fn set_model_override_turns_empty_into_none() {
        let mut settings = AttentionSettings::default();
        settings.set_model_override(Some(String::new()));
        assert_eq!(settings.model_override, None);
        settings.set_model_override(Some("m".to_owned()));
        assert_eq!(settings.model_override.as_deref(), Some("m"));
    }

    #[test]
    fn mode_names_parse_back_and_only_two_modes_use_an_llm() {
        for mode in AttentionMode::ALL {
            assert_eq!(AttentionMode::parse(mode.name()), Some(mode));
        }
        assert_eq!(
            AttentionMode::parse("Instant-Only"),
            Some(AttentionMode::InstantOnly)
        );
        assert_eq!(AttentionMode::parse("nope"), None);
        let llm: Vec<_> = AttentionMode::ALL
            .into_iter()
            .filter(|mode| mode.uses_llm())
            .collect();
        assert_eq!(llm, [AttentionMode::Shared, AttentionMode::Turbo]);
    }

    #[test]
    fn toml_round_trip_keeps_every_field_and_tolerates_missing_ones() {
        let settings = AttentionSettings {
            mode: AttentionMode::InstantOnly,
            press_sorts: false,
            turbo_concurrency: 3,
            expiry_seconds: 600,
            model_override: Some("claude-haiku".to_owned()),
        };
        let text = toml::to_string(&settings).unwrap();
        assert!(text.contains("mode = \"instant_only\""));
        assert_eq!(
            toml::from_str::<AttentionSettings>(&text).unwrap(),
            settings
        );
        let partial: AttentionSettings = toml::from_str("mode = \"turbo\"\n").unwrap();
        assert_eq!(partial.mode, AttentionMode::Turbo);
        assert_eq!(partial.turbo_concurrency, 16);
    }
}

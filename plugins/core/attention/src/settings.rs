//! The Attention tab: how the queue is fed and what is shown. The plugin is the only writer of
//! these rows (`contributes.settings`), read back with `HostSettings::get_json`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// How the attention queue is fed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AttentionMode {
    /// The queue is hidden and nothing is computed.
    Off,
    /// Only what agent detection already knows. No LLM is ever called.
    InstantOnly,
    /// One model call at a time for each changed agent.
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

    /// Stable machine name used by the settings row and the stored queue.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::InstantOnly => "instant_only",
            Self::Shared => "shared",
            Self::Turbo => "turbo",
        }
    }

    /// Parses a stored value: the machine name, case-insensitive, with `-` or space accepted for `_`.
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
    /// Pressing the button also refreshes the model's view of the queue.
    pub press_sorts: bool,
    /// Draw the three-row button.
    pub large_buttons: bool,
    /// Show the briefing box (why you were brought here) at the top of an agent reached through
    /// the queue. Only effective while the mode is not Off.
    pub show_briefing: bool,
    /// Parallel LLM calls in flight in Turbo mode.
    pub turbo_concurrency: u8,
    /// An item no longer supported by the live state is dropped after this long.
    pub expiry_seconds: u32,
}

impl Default for AttentionSettings {
    fn default() -> Self {
        Self {
            mode: AttentionMode::Shared,
            press_sorts: true,
            large_buttons: true,
            show_briefing: true,
            turbo_concurrency: 16,
            expiry_seconds: 1_800,
        }
    }
}

fn flag(map: &serde_json::Map<String, Value>, key: &str, default: bool) -> bool {
    map.get(key).and_then(Value::as_bool).unwrap_or(default)
}

fn number(map: &serde_json::Map<String, Value>, key: &str) -> Option<u64> {
    map.get(key).and_then(|value| {
        value
            .as_u64()
            .or_else(|| value.as_f64().filter(|n| *n >= 0.0).map(|n| n as u64))
    })
}

impl AttentionSettings {
    /// Reads the plugin's own settings object; a missing or unreadable key keeps its default.
    /// The rows sit in the `queue` section, so a nested `queue` object is read as well.
    #[must_use]
    pub fn from_json(text: &str) -> Self {
        let parsed: Option<Value> = serde_json::from_str(text).ok();
        let empty = serde_json::Map::new();
        let root = parsed.as_ref().and_then(Value::as_object).unwrap_or(&empty);
        let nested = root.get("queue").and_then(Value::as_object);
        let mut merged = root.clone();
        if let Some(section) = nested {
            for (key, value) in section {
                merged.insert(key.clone(), value.clone());
            }
        }
        let defaults = Self::default();
        Self {
            mode: merged
                .get("mode")
                .and_then(Value::as_str)
                .and_then(AttentionMode::parse)
                .unwrap_or(defaults.mode),
            press_sorts: flag(&merged, "press_sorts", defaults.press_sorts),
            large_buttons: flag(&merged, "large_buttons", defaults.large_buttons),
            show_briefing: flag(&merged, "show_briefing", defaults.show_briefing),
            turbo_concurrency: number(&merged, "turbo_concurrency")
                .map(|value| value.min(u64::from(u8::MAX)) as u8)
                .unwrap_or(defaults.turbo_concurrency),
            expiry_seconds: number(&merged, "expiry_seconds")
                .map(|value| value.min(u64::from(u32::MAX)) as u32)
                .unwrap_or(defaults.expiry_seconds),
        }
        .normalized()
    }

    /// Clamps numbers into their ranges.
    #[must_use]
    pub fn normalized(mut self) -> Self {
        self.turbo_concurrency = self
            .turbo_concurrency
            .clamp(TURBO_CONCURRENCY_MIN, TURBO_CONCURRENCY_MAX);
        self.expiry_seconds = self
            .expiry_seconds
            .clamp(EXPIRY_SECONDS_MIN, EXPIRY_SECONDS_MAX);
        self
    }

    /// Whether the queue is switched on at all.
    pub const fn enabled(&self) -> bool {
        !matches!(self.mode, AttentionMode::Off)
    }

    /// Whether the briefing box may appear: the user wants it and the queue is on.
    pub const fn briefing_enabled(&self) -> bool {
        self.show_briefing && self.enabled()
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
        assert!(settings.large_buttons);
        assert!(settings.show_briefing, "the briefing is on by default");
        assert_eq!(settings.turbo_concurrency, 16);
        assert_eq!(settings.expiry_seconds, 1_800);
        assert_eq!(settings.clone().normalized(), settings);
    }

    #[test]
    fn normalized_clamps_both_ends() {
        let low = AttentionSettings {
            turbo_concurrency: 0,
            expiry_seconds: 1,
            ..AttentionSettings::default()
        }
        .normalized();
        assert_eq!(low.turbo_concurrency, 1);
        assert_eq!(low.expiry_seconds, 60);
        let high = AttentionSettings {
            turbo_concurrency: 200,
            expiry_seconds: u32::MAX,
            ..AttentionSettings::default()
        }
        .normalized();
        assert_eq!(high.turbo_concurrency, 64);
        assert_eq!(high.expiry_seconds, 86_400);
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
    fn json_reads_flat_and_sectioned_rows_and_tolerates_missing_ones() {
        let flat = AttentionSettings::from_json(
            r#"{"mode":"instant_only","press_sorts":false,"turbo_concurrency":3,
                "expiry_seconds":600,"show_briefing":false}"#,
        );
        assert_eq!(flat.mode, AttentionMode::InstantOnly);
        assert!(!flat.press_sorts);
        assert!(!flat.show_briefing);
        assert_eq!(flat.turbo_concurrency, 3);
        assert_eq!(flat.expiry_seconds, 600);
        let sectioned = AttentionSettings::from_json(r#"{"queue":{"mode":"turbo"}}"#);
        assert_eq!(sectioned.mode, AttentionMode::Turbo);
        assert_eq!(sectioned.turbo_concurrency, 16);
        assert_eq!(
            AttentionSettings::from_json("not json"),
            AttentionSettings::default()
        );
    }

    #[test]
    fn the_briefing_is_effective_only_while_the_queue_is_on() {
        let mut settings = AttentionSettings::default();
        assert!(settings.briefing_enabled());
        settings.mode = AttentionMode::InstantOnly;
        assert!(
            settings.briefing_enabled(),
            "instant-only shows the automatic briefing"
        );
        settings.mode = AttentionMode::Off;
        assert!(!settings.briefing_enabled());
        settings.mode = AttentionMode::Turbo;
        settings.show_briefing = false;
        assert!(!settings.briefing_enabled());
    }
}

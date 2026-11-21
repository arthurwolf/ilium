//! Model strength tiers for the usage view's cool-to-hot colour ramp.
//!
//! The ranking is by what a model is positioned as, not by its price, so a
//! user override of the price table never reshuffles the colours.

/// Positioning of a model, weakest first. The order is the heat order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum StrengthTier {
    Light,
    Standard,
    Strong,
    Frontier,
}

impl StrengthTier {
    pub fn label(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Standard => "standard",
            Self::Strong => "strong",
            Self::Frontier => "frontier",
        }
    }
    /// Base colour, cool to hot. Shared by every provider so the same heat
    /// means the same positioning everywhere.
    pub fn base_rgb(self) -> (u8, u8, u8) {
        match self {
            Self::Light => (74, 144, 226),
            Self::Standard => (46, 184, 156),
            Self::Strong => (240, 160, 40),
            Self::Frontier => (224, 62, 62),
        }
    }
}

/// Neutral colour for models whose positioning is not known.
pub const UNKNOWN_RGB: (u8, u8, u8) = (140, 140, 150);

/// Tier of a model name, or `None` when the name carries no known marker.
pub fn strength_tier(model: &str) -> Option<StrengthTier> {
    let name = model.to_ascii_lowercase();
    let has = |markers: &[&str]| markers.iter().any(|marker| name.contains(marker));
    // "mini" also occurs inside "gemini", which says nothing about strength.
    let mini = name.contains("mini") && !name.contains("gemini");
    if mini || has(&["haiku", "luna", "nano", "flash"]) {
        Some(StrengthTier::Light)
    } else if has(&["sonnet", "terra"]) {
        Some(StrengthTier::Standard)
    } else if has(&["fable", "astra", "mythos"]) {
        Some(StrengthTier::Frontier)
    } else if has(&["opus", "sol", "gpt-", "codex"]) {
        Some(StrengthTier::Strong)
    } else {
        None
    }
}

/// Position of a model in the legend and in each bar: weakest first, unknown
/// last, then by name so the order is stable.
pub fn heat_sort_key(model: &str) -> (u8, String) {
    (
        strength_tier(model).map_or(u8::MAX, |tier| tier as u8),
        model.to_owned(),
    )
}

/// Colour of the `index`-th (of `count`) distinct model within one tier. Models
/// sharing a tier get lighter shades of the tier colour, never another hue, so
/// heat stays readable.
pub fn shaded_rgb(tier: Option<StrengthTier>, index: usize, count: usize) -> (u8, u8, u8) {
    let (red, green, blue) = tier.map_or(UNKNOWN_RGB, StrengthTier::base_rgb);
    if count <= 1 {
        return (red, green, blue);
    }
    // 0% (darkest) .. 45% lighter, evenly spread.
    let lift = 0.45 * index as f64 / (count - 1) as f64;
    let mix =
        |channel: u8| (f64::from(channel) + (255.0 - f64::from(channel)) * lift).round() as u8;
    (mix(red), mix(green), mix(blue))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiers_follow_model_strength_across_providers() {
        assert_eq!(
            strength_tier("claude-haiku-4-5-20251001"),
            Some(StrengthTier::Light)
        );
        assert_eq!(
            strength_tier("claude-sonnet-5-5"),
            Some(StrengthTier::Standard)
        );
        assert_eq!(strength_tier("claude-opus-4-8"), Some(StrengthTier::Strong));
        assert_eq!(
            strength_tier("claude-fable-5-2"),
            Some(StrengthTier::Frontier)
        );
        assert_eq!(strength_tier("gpt-6.1-luna"), Some(StrengthTier::Light));
        assert_eq!(strength_tier("gpt-6.1-terra"), Some(StrengthTier::Standard));
        assert_eq!(strength_tier("gpt-6.1-sol"), Some(StrengthTier::Strong));
        assert_eq!(strength_tier("gpt-5.4-mini"), Some(StrengthTier::Light));
        assert_eq!(strength_tier("mystery"), None);
        assert_eq!(strength_tier("gemini-3-pro"), None);
        assert_eq!(strength_tier("gemini-3-flash"), Some(StrengthTier::Light));
        assert!(StrengthTier::Light < StrengthTier::Frontier);
    }

    #[test]
    fn sort_key_orders_cool_to_hot_with_unknown_last() {
        let mut models = vec![
            "mystery",
            "claude-opus-4-8",
            "claude-haiku-4-5",
            "claude-fable-5",
        ];
        models.sort_by_key(|model| heat_sort_key(model));
        assert_eq!(
            models,
            [
                "claude-haiku-4-5",
                "claude-opus-4-8",
                "claude-fable-5",
                "mystery"
            ]
        );
    }

    #[test]
    fn same_tier_models_get_distinct_shades_and_a_lone_model_keeps_the_base() {
        let tier = Some(StrengthTier::Strong);
        assert_eq!(shaded_rgb(tier, 0, 1), StrengthTier::Strong.base_rgb());
        assert_ne!(shaded_rgb(tier, 0, 2), shaded_rgb(tier, 1, 2));
    }
}

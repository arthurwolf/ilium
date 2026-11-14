//! List prices the report uses for dollar figures: a built-in table matched by the longest model-name prefix, plus
//! the user's overrides. The cost plugin keeps its own copy of the table; the two stay separate.

use std::collections::BTreeMap;

/// Dollar prices per million tokens for one model.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModelPrice {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_write: f64,
}

impl ModelPrice {
    const fn new(input: f64, output: f64, cache_read: f64, cache_write: f64) -> Self {
        Self {
            input,
            output,
            cache_read,
            cache_write,
        }
    }
}

/// Built-in list prices, matched by the longest model-name prefix so a dated snapshot such as
/// `claude-haiku-4-5-20251001` finds its family row.
const BUILT_IN_PRICES: &[(&str, ModelPrice)] = &[
    ("claude-fable-5-1", ModelPrice::new(10.0, 50.0, 0.25, 12.5)),
    ("claude-fable-5", ModelPrice::new(10.0, 50.0, 0.25, 12.5)),
    ("claude-opus-5-5", ModelPrice::new(4.0, 20.0, 0.20, 5.0)),
    ("claude-opus-5", ModelPrice::new(5.0, 25.0, 0.50, 6.25)),
    ("claude-opus-4-8", ModelPrice::new(5.0, 25.0, 0.50, 6.25)),
    ("claude-opus-4-7", ModelPrice::new(5.0, 25.0, 0.50, 6.25)),
    ("claude-opus-4-6", ModelPrice::new(5.0, 25.0, 0.50, 6.25)),
    ("claude-opus-4-5", ModelPrice::new(5.0, 25.0, 0.50, 6.25)),
    ("claude-sonnet-5-5", ModelPrice::new(2.0, 10.0, 0.20, 2.5)),
    ("claude-sonnet-5", ModelPrice::new(2.0, 10.0, 0.20, 2.5)),
    ("claude-sonnet-4-6", ModelPrice::new(3.0, 15.0, 0.30, 3.75)),
    ("claude-sonnet-4-5", ModelPrice::new(3.0, 15.0, 0.30, 3.75)),
    ("claude-haiku-4-5", ModelPrice::new(1.0, 5.0, 0.10, 1.25)),
    ("gpt-5.6-sol", ModelPrice::new(4.0, 20.0, 0.40, 0.0)),
    ("gpt-5.6-terra", ModelPrice::new(2.0, 12.0, 0.20, 0.0)),
    ("gpt-5.6-luna", ModelPrice::new(0.20, 1.20, 0.02, 0.0)),
    ("gpt-5.3-codex", ModelPrice::new(1.75, 14.0, 0.175, 0.0)),
    ("gpt-5.2-codex", ModelPrice::new(1.75, 14.0, 0.175, 0.0)),
    ("gpt-5.1-codex-max", ModelPrice::new(1.25, 10.0, 0.125, 0.0)),
    ("gpt-5.1-codex-mini", ModelPrice::new(0.25, 2.0, 0.025, 0.0)),
    ("gpt-5.1-codex", ModelPrice::new(1.25, 10.0, 0.125, 0.0)),
    ("gpt-5-codex", ModelPrice::new(1.25, 10.0, 0.125, 0.0)),
];

/// Built-in prices plus the user's overrides.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PriceTable {
    overrides: BTreeMap<String, ModelPrice>,
}

impl PriceTable {
    /// A table with the user's per-model overrides on top of the built-in prices.
    pub fn with_overrides(overrides: &BTreeMap<String, ModelPrice>) -> Self {
        Self {
            overrides: overrides
                .iter()
                .map(|(model, price)| (model.to_ascii_lowercase(), *price))
                .collect(),
        }
    }

    /// Longest-prefix match across overrides and built-ins. A more specific prefix always wins; on equal length
    /// the user's override does.
    pub fn lookup(&self, model: &str) -> Option<ModelPrice> {
        let model = model.to_ascii_lowercase();
        let user = self
            .overrides
            .iter()
            .filter(|(prefix, _)| model.starts_with(prefix.as_str()))
            .map(|(prefix, price)| (prefix.len(), true, *price));
        let built_in = BUILT_IN_PRICES
            .iter()
            .filter(|(prefix, _)| model.starts_with(prefix))
            .map(|(prefix, price)| (prefix.len(), false, *price));
        user.chain(built_in)
            .max_by_key(|(length, is_user, _)| (*length, *is_user))
            .map(|(_, _, price)| price)
    }
}

//! Price weights: how many "input-token equivalents" (or dollars) each token
//! class costs.
//!
//! The crate never knows real prices. The caller supplies a lookup from a model
//! name to [`PriceWeights`] expressed in dollars per million tokens (for
//! example derived from the client's `PriceTable`); when the lookup has no
//! entry the unitless research weights are used and dollar figures are
//! reported as unavailable.

use serde::{Deserialize, Serialize};

use crate::agent::AgentKind;
use crate::trace::{SessionTrace, TurnSample};

/// Caller-supplied model-name to price lookup (dollars per million tokens).
pub type PriceLookup<'a> = &'a dyn Fn(&str) -> Option<PriceWeights>;

/// Price of each token class.
///
/// Dollars per million tokens when supplied by the caller; unitless
/// multipliers of the input price after [`PriceWeights::relative_weights`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PriceWeights {
    /// Uncached input tokens.
    pub input: f64,
    /// Output tokens (Codex: includes reasoning tokens).
    pub output: f64,
    /// Tokens read from the prompt cache.
    pub cache_read: f64,
    /// Tokens written to the 5-minute cache tier.
    pub cache_write_5m: f64,
    /// Tokens written to the 1-hour cache tier.
    pub cache_write_1h: f64,
}

/// Token counts per class, as floating point so replays can use fractions.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TokenCounts {
    /// Uncached input tokens.
    pub input: f64,
    /// Output tokens.
    pub output: f64,
    /// Cache-read tokens.
    pub cache_read: f64,
    /// 5-minute-tier cache-write tokens.
    pub cache_write_5m: f64,
    /// 1-hour-tier cache-write tokens.
    pub cache_write_1h: f64,
}

impl PriceWeights {
    /// The unitless weights of the Anthropic research: input 1, output 5,
    /// cache read 0.1, cache write 1.25 (5 min) and 2.0 (1 h).
    pub const fn anthropic_research() -> Self {
        Self {
            input: 1.0,
            output: 5.0,
            cache_read: 0.1,
            cache_write_5m: 1.25,
            cache_write_1h: 2.0,
        }
    }

    /// The unitless weights of the Codex research: no cache-write premium.
    pub const fn codex_research() -> Self {
        Self {
            input: 1.0,
            output: 5.0,
            cache_read: 0.1,
            cache_write_5m: 1.0,
            cache_write_1h: 1.0,
        }
    }

    /// The research weights for an agent and model.
    ///
    /// The research priced Opus 5.5 cache reads at 0.05 of the input price.
    pub fn research_for(agent: AgentKind, model: &str) -> Self {
        match agent {
            AgentKind::ClaudeCode => {
                let mut weights = Self::anthropic_research();
                if model.contains("opus-5-5") {
                    weights.cache_read = 0.05;
                }
                weights
            }
            AgentKind::Codex => Self::codex_research(),
        }
    }

    /// Weights divided by the input price: input becomes 1, the others
    /// multipliers. Returns `self` unchanged when the input price is not
    /// positive.
    pub fn relative_weights(&self) -> PriceWeights {
        if !self.input.is_finite() || self.input <= 0.0 {
            return *self;
        }
        PriceWeights {
            input: 1.0,
            output: self.output / self.input,
            cache_read: self.cache_read / self.input,
            cache_write_5m: self.cache_write_5m / self.input,
            cache_write_1h: self.cache_write_1h / self.input,
        }
    }

    /// Cost of a token vector under these weights.
    pub fn cost(&self, tokens: &TokenCounts) -> f64 {
        self.input * tokens.input
            + self.output * tokens.output
            + self.cache_read * tokens.cache_read
            + self.cache_write_5m * tokens.cache_write_5m
            + self.cache_write_1h * tokens.cache_write_1h
    }

    /// Weight of one cache-write token given the share written to the 5 min
    /// tier.
    pub fn blended_write(&self, share_5m: f64) -> f64 {
        let share = share_5m.clamp(0.0, 1.0);
        share * self.cache_write_5m + (1.0 - share) * self.cache_write_1h
    }
}

impl TokenCounts {
    /// The measured token vector of one logged turn.
    pub fn of_turn(turn: &TurnSample) -> Self {
        Self {
            input: f64::from(turn.input_tokens),
            output: f64::from(turn.output_tokens),
            cache_read: f64::from(turn.cache_read_tokens),
            cache_write_5m: f64::from(turn.cache_write_5m_tokens),
            cache_write_1h: f64::from(turn.cache_write_1h_tokens),
        }
    }

    /// Component-wise sum.
    pub fn add(&mut self, other: &TokenCounts) {
        self.input += other.input;
        self.output += other.output;
        self.cache_read += other.cache_read;
        self.cache_write_5m += other.cache_write_5m;
        self.cache_write_1h += other.cache_write_1h;
    }
}

/// Measured cost of one turn under `weights`.
pub fn turn_cost(turn: &TurnSample, weights: &PriceWeights) -> f64 {
    weights.cost(&TokenCounts::of_turn(turn))
}

/// Resolved weights of one model: relative (always) and dollars (when known).
#[derive(Debug, Clone, Copy)]
pub(crate) struct ResolvedPrice {
    /// Unitless weights, input price = 1.
    pub relative: PriceWeights,
    /// Dollars per token (not per million) when the lookup knew the model.
    pub dollars_per_token: Option<PriceWeights>,
}

impl ResolvedPrice {
    /// Resolves a model through the caller's lookup with research fallback.
    pub(crate) fn resolve(agent: AgentKind, model: &str, lookup: Option<PriceLookup<'_>>) -> Self {
        let looked_up = lookup.and_then(|find| find(model)).map(|mut prices| {
            if agent == AgentKind::Codex {
                // Codex bills no cache-write premium: a freshly processed
                // token costs the input price whatever the lookup says.
                prices.cache_write_5m = prices.input;
                prices.cache_write_1h = prices.input;
            }
            prices
        });
        match looked_up {
            Some(prices) if prices.input > 0.0 => Self {
                relative: prices.relative_weights(),
                dollars_per_token: Some(PriceWeights {
                    input: prices.input / 1.0e6,
                    output: prices.output / 1.0e6,
                    cache_read: prices.cache_read / 1.0e6,
                    cache_write_5m: prices.cache_write_5m / 1.0e6,
                    cache_write_1h: prices.cache_write_1h / 1.0e6,
                }),
            },
            _ => Self {
                relative: PriceWeights::research_for(agent, model),
                dollars_per_token: None,
            },
        }
    }
}

/// Resolves the price of every model in a trace's model table.
pub(crate) fn resolve_trace_prices(
    trace: &SessionTrace,
    lookup: Option<PriceLookup<'_>>,
) -> Vec<ResolvedPrice> {
    let mut resolved: Vec<ResolvedPrice> = trace
        .models
        .iter()
        .map(|model| ResolvedPrice::resolve(trace.agent, model, lookup))
        .collect();
    if resolved.is_empty() {
        resolved.push(ResolvedPrice::resolve(trace.agent, "", lookup));
    }
    resolved
}

/// The price entry of a model index, clamped to the table.
pub(crate) fn price_of(prices: &[ResolvedPrice], model: u16) -> &ResolvedPrice {
    let last = prices.len().saturating_sub(1);
    &prices[usize::from(model).min(last)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_weights_normalise_by_input() {
        let dollars = PriceWeights {
            input: 3.0,
            output: 15.0,
            cache_read: 0.3,
            cache_write_5m: 3.75,
            cache_write_1h: 6.0,
        };
        let relative = dollars.relative_weights();
        let expected = PriceWeights::anthropic_research();
        assert!((relative.output - expected.output).abs() < 1e-12);
        assert!((relative.cache_read - expected.cache_read).abs() < 1e-12);
        assert!((relative.cache_write_5m - expected.cache_write_5m).abs() < 1e-12);
        assert!((relative.cache_write_1h - expected.cache_write_1h).abs() < 1e-12);
        assert_eq!(relative.input, 1.0);
    }

    #[test]
    fn research_weights_per_agent() {
        assert_eq!(
            PriceWeights::research_for(AgentKind::ClaudeCode, "claude-sonnet-5").cache_write_1h,
            2.0
        );
        assert_eq!(
            PriceWeights::research_for(AgentKind::ClaudeCode, "claude-opus-5-5").cache_read,
            0.05
        );
        assert_eq!(
            PriceWeights::research_for(AgentKind::Codex, "gpt-6.1-sol").cache_write_1h,
            1.0
        );
    }

    #[test]
    fn cost_is_a_dot_product() {
        let weights = PriceWeights::anthropic_research();
        let tokens = TokenCounts {
            input: 10.0,
            output: 2.0,
            cache_read: 100.0,
            cache_write_5m: 4.0,
            cache_write_1h: 1.0,
        };
        assert!((weights.cost(&tokens) - (10.0 + 10.0 + 10.0 + 5.0 + 2.0)).abs() < 1e-12);
        assert!((weights.blended_write(0.5) - 1.625).abs() < 1e-12);
    }

    #[test]
    fn resolve_prefers_lookup_and_falls_back() {
        let lookup = |model: &str| {
            (model == "known").then_some(PriceWeights {
                input: 2.0,
                output: 10.0,
                cache_read: 0.2,
                cache_write_5m: 2.5,
                cache_write_1h: 4.0,
            })
        };
        let known = ResolvedPrice::resolve(AgentKind::ClaudeCode, "known", Some(&lookup));
        assert!(known.dollars_per_token.is_some());
        assert!((known.relative.output - 5.0).abs() < 1e-12);
        let unknown = ResolvedPrice::resolve(AgentKind::Codex, "other", Some(&lookup));
        assert!(unknown.dollars_per_token.is_none());
        assert_eq!(unknown.relative, PriceWeights::codex_research());
    }
}

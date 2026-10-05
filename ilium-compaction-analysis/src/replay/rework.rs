//! The rework penalty: what a compaction costs beyond its own requests.
//!
//! After a compaction the agent re-reads files and re-runs searches the
//! summary dropped. That cost is the pivotal quantity of the optimisation and
//! the hardest to measure, so v1 uses priors measured on one corpus (Claude
//! 72k, Codex 40k weighted tokens per compaction) with a 0.5x / 1x / 2x
//! sensitivity. A caller that measured the rework from the user's own logs
//! (at least [`ReworkModel::MEASURED_MIN_COMPACTIONS`] compactions) can supply
//! it with [`ReworkModel::measured`].

use serde::{Deserialize, Serialize};

use crate::agent::AgentKind;
use crate::rework::MeasuredRework;

/// Where the rework figure comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReworkSource {
    /// The research prior of the agent (one corpus).
    Prior,
    /// Measured by the caller on the user's own logs.
    Measured,
}

/// How the penalty enters the simulated cost.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReworkAccounting {
    /// Every simulated compaction pays the penalty. Used when the replayed
    /// stream had the observed post-compaction work removed (Claude).
    Absolute,
    /// Only compactions beyond the observed count pay (or refund) the penalty,
    /// because the observed rework is still inside the replayed stream
    /// (Codex).
    Differential,
}

/// Rework penalty per compaction.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ReworkModel {
    /// Weighted tokens (input-token equivalents) per compaction at 1x.
    pub tokens_per_compaction: f64,
    /// Sensitivity multiplier (0.5, 1, 2).
    pub multiplier: f64,
    /// Provenance of `tokens_per_compaction`.
    pub source: ReworkSource,
    /// Compactions the measurement rests on (`None` for a prior or a bare
    /// caller-supplied value).
    #[serde(default)]
    pub measured_from_compactions: Option<u32>,
    /// 95% bootstrap interval of the measured value, weighted tokens.
    #[serde(default)]
    pub measured_ci_tokens: Option<(f64, f64)>,
    /// The measurement was noisy, so `tokens_per_compaction` is a 50/50 blend
    /// of the measured value and the prior.
    #[serde(default)]
    pub blended_with_prior: bool,
}

impl ReworkModel {
    /// Number of observed compactions from which a measured value should
    /// replace the prior.
    pub const MEASURED_MIN_COMPACTIONS: usize = 30;

    /// The research prior of an agent at multiplier 1.
    pub fn prior_for(agent: AgentKind) -> Self {
        Self {
            tokens_per_compaction: agent.profile().rework_prior_weighted_tokens,
            multiplier: 1.0,
            source: ReworkSource::Prior,
            measured_from_compactions: None,
            measured_ci_tokens: None,
            blended_with_prior: false,
        }
    }

    /// A caller-measured value.
    pub fn measured(tokens_per_compaction: f64) -> Self {
        Self {
            tokens_per_compaction,
            multiplier: 1.0,
            source: ReworkSource::Measured,
            measured_from_compactions: None,
            measured_ci_tokens: None,
            blended_with_prior: false,
        }
    }

    /// The measured rework of a corpus ([`crate::rework::measure`]) when there
    /// is one, else the agent's prior. This is the constructor the
    /// simulation and optimizer configs use.
    pub fn from_measurement(agent: AgentKind, measurement: Option<&MeasuredRework>) -> Self {
        match measurement {
            Some(measured) => {
                let prior = Self::prior_for(agent).tokens_per_compaction;
                let blended = measured.noisy;
                let tokens = if blended {
                    0.5 * measured.tokens_per_compaction + 0.5 * prior
                } else {
                    measured.tokens_per_compaction
                };
                Self {
                    tokens_per_compaction: tokens,
                    multiplier: 1.0,
                    source: ReworkSource::Measured,
                    measured_from_compactions: u32::try_from(measured.compactions).ok(),
                    measured_ci_tokens: Some((measured.ci_low_tokens, measured.ci_high_tokens)),
                    blended_with_prior: blended,
                }
            }
            None => Self::prior_for(agent),
        }
    }

    /// One-line description: "rework measured from N compactions (X weighted
    /// tokens, 95% CI a to b)" or the prior's label.
    pub fn describe(&self) -> String {
        match (self.source, self.measured_from_compactions, self.measured_ci_tokens) {
            (ReworkSource::Measured, Some(count), Some((low, high))) if self.blended_with_prior => format!(
                "rework measured from {count} compactions but noisy (95% CI {low:.0} to {high:.0}), blended 50/50 with the research prior: {:.0} weighted tokens",
                self.tokens_per_compaction
            ),
            (ReworkSource::Measured, Some(count), Some((low, high))) => format!(
                "rework measured from {count} compactions ({:.0} weighted tokens, 95% CI {low:.0} to {high:.0})",
                self.tokens_per_compaction
            ),
            (ReworkSource::Measured, _, _) => format!(
                "rework supplied by the caller ({:.0} weighted tokens)",
                self.tokens_per_compaction
            ),
            (ReworkSource::Prior, _, _) => format!(
                "rework is a research prior ({:.0} weighted tokens), not measured on these logs",
                self.tokens_per_compaction
            ),
        }
    }

    /// Uses `measured` (tokens, number of compactions it was measured on) when
    /// at least [`Self::MEASURED_MIN_COMPACTIONS`] compactions back it, else
    /// the prior.
    pub fn prefer_measured(agent: AgentKind, measured: Option<(f64, usize)>) -> Self {
        match measured {
            Some((tokens, count)) if count >= Self::MEASURED_MIN_COMPACTIONS && tokens >= 0.0 => {
                Self::measured(tokens)
            }
            _ => Self::prior_for(agent),
        }
    }

    /// The same model at another sensitivity multiplier.
    pub fn with_multiplier(self, multiplier: f64) -> Self {
        Self { multiplier, ..self }
    }

    /// Weighted tokens charged per compaction after the multiplier.
    pub fn effective_tokens(&self) -> f64 {
        self.tokens_per_compaction * self.multiplier
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn priors_and_measured_override() {
        assert_eq!(
            ReworkModel::prior_for(AgentKind::ClaudeCode).tokens_per_compaction,
            72_000.0
        );
        assert_eq!(
            ReworkModel::prior_for(AgentKind::Codex).tokens_per_compaction,
            40_000.0
        );
        let few = ReworkModel::prefer_measured(AgentKind::Codex, Some((10_000.0, 29)));
        assert_eq!(few.source, ReworkSource::Prior);
        let enough = ReworkModel::prefer_measured(AgentKind::Codex, Some((10_000.0, 30)));
        assert_eq!(enough.source, ReworkSource::Measured);
        assert_eq!(enough.with_multiplier(2.0).effective_tokens(), 20_000.0);
    }
}

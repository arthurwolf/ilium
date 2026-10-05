//! Per-agent mapping between the *effective trigger* the simulation reasons
//! about (context size at which a compaction fires) and the *configuration
//! value* a user writes.
//!
//! Both CLIs changed these semantics across versions, so each mapping is a
//! versioned assumption ([`SEMANTICS_VERSION`]) that can be re-measured from
//! the data when compactions exist:
//!
//! * **Claude Code** `autoCompactWindow` (also `/autocompact`,
//!   `CLAUDE_CODE_AUTO_COMPACT_WINDOW`): accepted 100,000..=1,000,000. The
//!   compaction fires `offset` tokens *before* the setting; the offset was
//!   measured at about 33,000 on v2.1.289 and can be re-measured as
//!   `median(setting - preTokens)` when the setting in force is known.
//! * **Codex** `model_auto_compact_token_limit`: clamped to 90 % of the
//!   resolved context window (a larger setting is silently reduced), and the
//!   quantity that fires is the last response's input + output, so the
//!   realized trigger can differ slightly from the limit (measured
//!   realized/limit ratio, 1.0 by default).

use serde::{Deserialize, Serialize};

use crate::agent::AgentKind;
use crate::stats::{median, Quantiles};
use crate::trace::{CompactionTrigger, SessionTrace};

/// Version of the encoded semantics. Bump when a mapping default changes.
pub const SEMANTICS_VERSION: u32 = 1;

/// Claude Code's configuration key.
pub const CLAUDE_SETTING_NAME: &str = "autoCompactWindow";
/// Codex's configuration key.
pub const CODEX_SETTING_NAME: &str = "model_auto_compact_token_limit";

/// Observations needed before a measured value replaces the default.
pub const MIN_MEASUREMENT_OBSERVATIONS: usize = 5;

/// Where a mapping parameter comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParameterSource {
    /// The documented / researched default.
    Default,
    /// Re-measured on the user's logs.
    Measured,
}

/// Claude Code mapping: `setting = trigger + offset`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ClaudeSemantics {
    /// Tokens between the firing size and the configured window.
    pub offset_tokens: u32,
    /// Smallest accepted setting.
    pub min_setting_tokens: u32,
    /// Largest accepted setting.
    pub max_setting_tokens: u32,
    /// Provenance of `offset_tokens`.
    pub offset_source: ParameterSource,
}

/// Codex mapping: clamp at a fraction of the window, scale by the realized
/// ratio.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CodexSemantics {
    /// Resolved context window.
    pub window_tokens: u32,
    /// Largest accepted fraction of the window (0.9).
    pub clamp_fraction: f64,
    /// Measured `realized trigger / effective limit` (1.0 by default).
    pub realized_ratio: f64,
    /// Provenance of `realized_ratio`.
    pub ratio_source: ParameterSource,
}

/// The versioned semantics of one agent.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum AgentSemantics {
    /// Claude Code.
    Claude(ClaudeSemantics),
    /// Codex.
    Codex(CodexSemantics),
}

/// A configuration value for a trigger.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SettingMapping {
    /// The configuration key.
    pub setting_name: String,
    /// The value to write.
    pub value: u32,
    /// Whether the value had to be clamped into the accepted range (the
    /// realized trigger then differs from the requested one).
    pub clamped: bool,
    /// The trigger the clamped value really produces.
    pub realized_trigger_tokens: u32,
}

/// What the agent does when nothing is configured.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CliDefaults {
    /// Effective trigger of the CLI default.
    pub trigger_tokens: u32,
    /// How the figure arises.
    pub note: String,
}

/// A Claude observation: the window setting in force and the size at which a
/// compaction fired.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OffsetObservation {
    /// `autoCompactWindow` in force.
    pub setting_tokens: u32,
    /// Logged `preTokens` of the compaction.
    pub pre_tokens: u32,
}

/// A Codex observation: the limit in force and the size at which a compaction
/// fired.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RatioObservation {
    /// `model_auto_compact_token_limit` in force.
    pub limit_tokens: u32,
    /// Last-response input + output at the compaction.
    pub realized_tokens: u32,
}

/// A re-measured parameter with its support.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MeasuredParameter {
    /// Median value.
    pub value: f64,
    /// Observations behind it.
    pub observations: usize,
    /// Spread of the observations.
    pub spread: Quantiles,
}

impl AgentSemantics {
    /// The default semantics of an agent.
    pub fn default_for(agent: AgentKind) -> Self {
        match agent {
            AgentKind::ClaudeCode => AgentSemantics::Claude(ClaudeSemantics {
                offset_tokens: 33_000,
                min_setting_tokens: 100_000,
                max_setting_tokens: 1_000_000,
                offset_source: ParameterSource::Default,
            }),
            AgentKind::Codex => AgentSemantics::Codex(CodexSemantics {
                window_tokens: agent.profile().default_context_window_tokens,
                clamp_fraction: 0.9,
                realized_ratio: 1.0,
                ratio_source: ParameterSource::Default,
            }),
        }
    }

    /// Codex semantics for a stated context window.
    pub fn codex_with_window(window_tokens: u32) -> Self {
        AgentSemantics::Codex(CodexSemantics {
            window_tokens,
            clamp_fraction: 0.9,
            realized_ratio: 1.0,
            ratio_source: ParameterSource::Default,
        })
    }

    /// The agent these semantics describe.
    pub fn agent(&self) -> AgentKind {
        match self {
            AgentSemantics::Claude(_) => AgentKind::ClaudeCode,
            AgentSemantics::Codex(_) => AgentKind::Codex,
        }
    }

    /// The configuration key written by Apply.
    pub fn setting_name(&self) -> &'static str {
        match self {
            AgentSemantics::Claude(_) => CLAUDE_SETTING_NAME,
            AgentSemantics::Codex(_) => CODEX_SETTING_NAME,
        }
    }

    /// Accepted range of the setting.
    pub fn allowed_setting_range(&self) -> (u32, u32) {
        match self {
            AgentSemantics::Claude(claude) => {
                (claude.min_setting_tokens, claude.max_setting_tokens)
            }
            AgentSemantics::Codex(codex) => (1, codex.setting_cap()),
        }
    }

    /// The setting that produces `trigger_tokens`, clamped into the accepted
    /// range.
    pub fn trigger_to_setting(&self, trigger_tokens: u32) -> SettingMapping {
        let (low, high) = self.allowed_setting_range();
        let unclamped = match self {
            AgentSemantics::Claude(claude) => {
                u64::from(trigger_tokens) + u64::from(claude.offset_tokens)
            }
            AgentSemantics::Codex(codex) => {
                (f64::from(trigger_tokens) / codex.realized_ratio.max(f64::EPSILON)).round() as u64
            }
        };
        let value = u32::try_from(unclamped.clamp(u64::from(low), u64::from(high))).unwrap_or(high);
        SettingMapping {
            setting_name: self.setting_name().to_string(),
            value,
            clamped: u64::from(value) != unclamped,
            realized_trigger_tokens: self.setting_to_trigger(value),
        }
    }

    /// The effective trigger a configured value produces.
    pub fn setting_to_trigger(&self, setting_tokens: u32) -> u32 {
        match self {
            AgentSemantics::Claude(claude) => {
                let setting =
                    setting_tokens.clamp(claude.min_setting_tokens, claude.max_setting_tokens);
                setting.saturating_sub(claude.offset_tokens)
            }
            AgentSemantics::Codex(codex) => {
                let effective = setting_tokens.min(codex.setting_cap());
                (f64::from(effective) * codex.realized_ratio).round() as u32
            }
        }
    }

    /// The CLI default for a model whose context window is `window_tokens`.
    pub fn cli_defaults(&self, window_tokens: u32) -> CliDefaults {
        match self {
            AgentSemantics::Claude(_) => claude_cli_defaults(window_tokens),
            AgentSemantics::Codex(codex) => {
                let cap = (codex.clamp_fraction * f64::from(window_tokens)).floor();
                CliDefaults {
                    trigger_tokens: (cap * codex.realized_ratio).round() as u32,
                    note: format!(
                        "90% of the {window_tokens}-token window (the derived default; configured limits are clamped to it)"
                    ),
                }
            }
        }
    }

    /// Replaces the Claude offset by a measured one.
    pub fn with_measured_offset(self, offset_tokens: u32) -> Self {
        match self {
            AgentSemantics::Claude(claude) => AgentSemantics::Claude(ClaudeSemantics {
                offset_tokens,
                offset_source: ParameterSource::Measured,
                ..claude
            }),
            other => other,
        }
    }

    /// Replaces the Codex realized ratio by a measured one.
    pub fn with_measured_ratio(self, realized_ratio: f64) -> Self {
        match self {
            AgentSemantics::Codex(codex) if realized_ratio > 0.0 => {
                AgentSemantics::Codex(CodexSemantics {
                    realized_ratio,
                    ratio_source: ParameterSource::Measured,
                    ..codex
                })
            }
            other => other,
        }
    }
}

impl CodexSemantics {
    /// Largest setting Codex honours: `floor(clamp_fraction * window)`.
    pub fn setting_cap(&self) -> u32 {
        (self.clamp_fraction * f64::from(self.window_tokens)).floor() as u32
    }
}

/// Claude's default: about 967,000 for native 1M-token models, the window
/// boundary otherwise.
pub fn claude_cli_defaults(window_tokens: u32) -> CliDefaults {
    if window_tokens >= 1_000_000 {
        CliDefaults {
            trigger_tokens: 967_000,
            note: "about 967K tokens by default on native 1M-token models (about 33K reserved below the window)"
                .to_string(),
        }
    } else {
        CliDefaults {
            trigger_tokens: window_tokens,
            note: format!(
                "the {window_tokens}-token window boundary on models without a 1M window"
            ),
        }
    }
}

/// Measures the Claude offset as `median(setting - preTokens)`; `None` below
/// [`MIN_MEASUREMENT_OBSERVATIONS`] usable observations. Observations whose
/// compaction fired above the setting are discarded.
pub fn measure_claude_offset(observations: &[OffsetObservation]) -> Option<MeasuredParameter> {
    let mut differences: Vec<f64> = observations
        .iter()
        .filter(|observation| observation.pre_tokens <= observation.setting_tokens)
        .map(|observation| f64::from(observation.setting_tokens - observation.pre_tokens))
        .collect();
    if differences.len() < MIN_MEASUREMENT_OBSERVATIONS {
        return None;
    }
    let value = median(&differences);
    let spread = Quantiles::of(&mut differences)?;
    Some(MeasuredParameter {
        value,
        observations: spread.n,
        spread,
    })
}

/// Measures the Codex realized/limit ratio relative to the clamped limit.
pub fn measure_codex_ratio(
    observations: &[RatioObservation],
    semantics: &CodexSemantics,
) -> Option<MeasuredParameter> {
    let cap = semantics.setting_cap();
    let mut ratios: Vec<f64> = observations
        .iter()
        .filter(|observation| observation.limit_tokens > 0 && observation.realized_tokens > 0)
        .map(|observation| {
            f64::from(observation.realized_tokens) / f64::from(observation.limit_tokens.min(cap))
        })
        .collect();
    if ratios.len() < MIN_MEASUREMENT_OBSERVATIONS {
        return None;
    }
    let value = median(&ratios);
    let spread = Quantiles::of(&mut ratios)?;
    Some(MeasuredParameter {
        value,
        observations: spread.n,
        spread,
    })
}

/// Collects Claude offset observations from a corpus. `setting_in_force`
/// returns the `autoCompactWindow` active at a timestamp (Unix milliseconds)
/// when the caller knows it. Only automatic compactions count; `regime`
/// (typically `RegimeSet::most_recent()`'s
/// [`pre_token_range`](crate::stats::Regime::pre_token_range)) restricts them
/// to one compaction regime, because an older regime ran under another
/// setting.
pub fn claude_offset_observations(
    traces: &[SessionTrace],
    regime: Option<std::ops::RangeInclusive<u32>>,
    setting_in_force: &dyn Fn(i64) -> Option<u32>,
) -> Vec<OffsetObservation> {
    traces
        .iter()
        .filter(|trace| trace.agent == AgentKind::ClaudeCode)
        .flat_map(|trace| trace.compactions.iter())
        .filter(|event| event.trigger == CompactionTrigger::Auto && event.pre_tokens > 0)
        .filter(|event| {
            regime
                .as_ref()
                .is_none_or(|range| range.contains(&event.pre_tokens))
        })
        .filter_map(|event| {
            setting_in_force(event.timestamp_ms).map(|setting| OffsetObservation {
                setting_tokens: setting,
                pre_tokens: event.pre_tokens,
            })
        })
        .collect()
}

/// Collects Codex ratio observations from a corpus; `limit_in_force` returns
/// the `model_auto_compact_token_limit` active at a timestamp. `regime`
/// restricts the compactions as in [`claude_offset_observations`].
pub fn codex_ratio_observations(
    traces: &[SessionTrace],
    regime: Option<std::ops::RangeInclusive<u32>>,
    limit_in_force: &dyn Fn(i64) -> Option<u32>,
) -> Vec<RatioObservation> {
    traces
        .iter()
        .filter(|trace| trace.agent == AgentKind::Codex)
        .flat_map(|trace| trace.compactions.iter())
        .filter(|event| event.pre_tokens > 0)
        .filter(|event| {
            regime
                .as_ref()
                .is_none_or(|range| range.contains(&event.pre_tokens))
        })
        .filter_map(|event| {
            limit_in_force(event.timestamp_ms).map(|limit| RatioObservation {
                limit_tokens: limit,
                realized_tokens: event.pre_tokens,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_maps_trigger_to_window_with_clamping() {
        let semantics = AgentSemantics::default_for(AgentKind::ClaudeCode);
        let mapping = semantics.trigger_to_setting(217_000);
        assert_eq!(mapping.value, 250_000);
        assert!(!mapping.clamped);
        assert_eq!(mapping.setting_name, "autoCompactWindow");
        assert_eq!(semantics.setting_to_trigger(250_000), 217_000);
        // Below the accepted minimum the value is raised and flagged.
        let low = semantics.trigger_to_setting(50_000);
        assert_eq!(low.value, 100_000);
        assert!(low.clamped);
        assert_eq!(low.realized_trigger_tokens, 67_000);
        let high = semantics.trigger_to_setting(990_000);
        assert_eq!(high.value, 1_000_000);
        assert!(high.clamped);
    }

    #[test]
    fn claude_defaults_depend_on_the_window() {
        let semantics = AgentSemantics::default_for(AgentKind::ClaudeCode);
        assert_eq!(semantics.cli_defaults(1_000_000).trigger_tokens, 967_000);
        assert_eq!(semantics.cli_defaults(200_000).trigger_tokens, 200_000);
    }

    #[test]
    fn codex_clamps_at_ninety_percent_of_the_window() {
        let semantics = AgentSemantics::default_for(AgentKind::Codex);
        assert_eq!(semantics.cli_defaults(258_400).trigger_tokens, 232_560);
        let inside = semantics.trigger_to_setting(180_000);
        assert_eq!(inside.value, 180_000);
        assert!(!inside.clamped);
        let beyond = semantics.trigger_to_setting(250_000);
        assert_eq!(beyond.value, 232_560);
        assert!(beyond.clamped);
        assert_eq!(semantics.setting_to_trigger(300_000), 232_560);
    }

    #[test]
    fn measured_values_override_defaults() {
        let observations: Vec<OffsetObservation> = (0..8)
            .map(|index| OffsetObservation {
                setting_tokens: 600_000,
                pre_tokens: 566_000 + index * 200,
            })
            .collect();
        let measured = measure_claude_offset(&observations).unwrap();
        assert!((measured.value - 33_300.0).abs() < 1.0);
        let semantics = AgentSemantics::default_for(AgentKind::ClaudeCode)
            .with_measured_offset(measured.value.round() as u32);
        assert_eq!(semantics.trigger_to_setting(217_000).value, 250_300);
        assert!(measure_claude_offset(&observations[..4]).is_none());

        let codex = CodexSemantics {
            window_tokens: 258_400,
            clamp_fraction: 0.9,
            realized_ratio: 1.0,
            ratio_source: ParameterSource::Default,
        };
        let ratios: Vec<RatioObservation> = (0..6)
            .map(|_| RatioObservation {
                limit_tokens: 200_000,
                realized_tokens: 204_000,
            })
            .collect();
        let ratio = measure_codex_ratio(&ratios, &codex).unwrap();
        assert!((ratio.value - 1.02).abs() < 1e-9);
        let adjusted = AgentSemantics::Codex(codex).with_measured_ratio(ratio.value);
        assert_eq!(adjusted.setting_to_trigger(200_000), 204_000);
        assert_eq!(adjusted.trigger_to_setting(204_000).value, 200_000);
    }
}

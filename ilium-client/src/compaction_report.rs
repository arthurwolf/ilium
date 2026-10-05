//! View-model of the compaction optimizer report (Settings > Optimization).
//!
//! Plain data, no ratatui and no I/O. [`CompactionReport::build`] takes the
//! parsed traces of one agent, runs the pure analysis crate
//! (`ilium-compaction-analysis`: statistics, trace-driven replay, optimizer,
//! trigger/setting semantics) and arranges everything the tab shows
//! (design section 4) into rows and cards, so the UI only lays them out. The
//! formatting helpers (humanized tokens, dollars, percentages) live here for
//! the same reason.
//!
//! The scan job in [`crate::compaction_scan`] owns file access and calls
//! [`CompactionReport::build`] once every transcript is a trace.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use ilium_compaction_analysis::optimize::{
    optimize, three_way_comparison, ComparisonPoint, ObservedCompactions, OptimizeConfig,
    PointQuality, Recommendation,
};
use ilium_compaction_analysis::price::PriceWeights;
use ilium_compaction_analysis::replay::{simulate, ReworkSource, SimConfig};
use ilium_compaction_analysis::semantics::{
    claude_offset_observations, codex_ratio_observations, measure_claude_offset,
    measure_codex_ratio, AgentSemantics, ParameterSource,
};
use ilium_compaction_analysis::stats::{CorpusStats, Quantiles, StatsConfig};
use ilium_compaction_analysis::trace::SessionTrace;
use ilium_compaction_analysis::{AgentKind, AnalysisError};

pub use ilium_compaction_analysis::optimize::{Confidence, PickRule};

use crate::cost_model::PriceTable;

/// Everything the caller decides about one scan (the UI executor fills it).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ScanSettingsInput {
    /// The value currently written in the agent's configuration (Claude
    /// `autoCompactWindow`, Codex `model_auto_compact_token_limit`), when set.
    pub current_setting_value: Option<u64>,
    /// Prices used for dollar figures (built-in table plus the user's
    /// overrides). Unknown models fall back to the research weights.
    pub price_table: PriceTable,
}

/// What the scan itself saw (file level), carried into the report.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ScanSummary {
    /// Transcript files found.
    pub files_listed: u64,
    /// Files parsed in this scan.
    pub files_parsed: u64,
    /// Files served from the trace cache.
    pub files_from_cache: u64,
    /// Files that could not be read (vanished, permissions) and were skipped.
    pub files_skipped: u64,
    /// Files whose content moved while they were read (kept out of the cache).
    pub files_changed_during_scan: u64,
    /// Total size of the listed files.
    pub bytes_total: u64,
    /// Requests the cross-file dedupe removed (resumed or forked transcripts).
    pub duplicate_turns_removed: u64,
    /// Compactions the cross-file dedupe removed.
    pub duplicate_compactions_removed: u64,
    /// Sessions dropped because the retained traces exceeded the byte cap.
    pub sessions_dropped_for_cap: u64,
    /// Wall-clock scan time in seconds.
    pub scan_seconds: f64,
    /// Scan-level warnings (listing limits, vanished files, cache problems).
    pub warnings: Vec<String>,
}

/// Input of [`CompactionReport::build`].
#[derive(Debug, Clone, Copy)]
pub struct ReportInput<'a> {
    /// The agent the traces belong to.
    pub agent: AgentKind,
    /// Parsed transcripts, already sorted (main sessions first) and deduped.
    pub traces: &'a [SessionTrace],
    /// Caller-supplied settings.
    pub settings: &'a ScanSettingsInput,
    /// File-level facts of the scan.
    pub scan: &'a ScanSummary,
    /// Report time, Unix milliseconds.
    pub generated_at_unix_ms: i64,
}

/// Whether the report holds a recommendation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportStatus {
    /// Statistics and recommendation are present.
    Complete,
    /// No transcript was found for the agent.
    NoSessionsFound,
    /// Sessions exist, but none reached the minimum replay context.
    NothingToReplay,
}

/// Corpus facts.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CorpusSummary {
    pub files_listed: u64,
    pub files_parsed: u64,
    pub files_from_cache: u64,
    pub files_skipped: u64,
    pub bytes_total: u64,
    pub main_sessions: u64,
    pub subagent_sessions: u64,
    pub main_requests: u64,
    pub subagent_requests: u64,
    /// Share of requests that belong to subagents (0..1).
    pub subagent_request_share: f64,
    pub first_timestamp_ms: i64,
    pub last_timestamp_ms: i64,
    pub span_days: f64,
    pub lines_skipped: u64,
    pub duplicate_turns_removed: u64,
    pub sessions_dropped_for_cap: u64,
    pub scan_seconds: f64,
}

/// Compaction statistics.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CompactionSummary {
    pub total: u64,
    pub auto: u64,
    pub manual: u64,
    /// Compactions whose trigger kind the log does not state (Codex).
    pub unknown_trigger: u64,
    pub measured: u64,
    pub sessions_with_compaction: u64,
    /// Size the trigger fired at (tokens), every compaction.
    pub pre_tokens: Option<Quantiles>,
    /// Same, automatic compactions only.
    pub pre_tokens_auto: Option<Quantiles>,
    /// Size of the first request after a compaction (tokens).
    pub first_post_request_tokens: Option<Quantiles>,
    /// Share of that request served from cache (0..1).
    pub first_post_cache_read_share: Option<Quantiles>,
    pub summary_tokens: Option<Quantiles>,
    pub duration_seconds: Option<Quantiles>,
    pub cycles: u64,
    pub cycle_requests: Option<Quantiles>,
    pub cycle_minutes: Option<Quantiles>,
    /// Cycles of at most 3 requests: the refill thrash.
    pub thrash_cycles: u64,
    pub cycles_at_most_10: u64,
    pub cycles_at_most_30: u64,
    pub requests_to_first_compaction: Option<Quantiles>,
}

/// One model's share of the spend.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelUsageRow {
    pub model: String,
    pub family: String,
    pub requests: u64,
    /// Share of the weighted cost of all models (0..1).
    pub weighted_cost_share: f64,
    pub usd: Option<f64>,
}

/// How much of the cost sits above a context size.
#[derive(Debug, Clone, PartialEq)]
pub struct ReachRow {
    pub threshold_tokens: u32,
    pub sessions_reaching: u64,
    /// Share of the total cost spent by requests above the threshold.
    pub cost_share_above: f64,
}

/// Cost mix by token class (weighted by the price table).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CostMixSummary {
    pub total_weighted_tokens: f64,
    pub total_usd: Option<f64>,
    pub input_share: f64,
    pub output_share: f64,
    pub cache_read_share: f64,
    pub cache_write_share: f64,
    pub models: Vec<ModelUsageRow>,
    pub reach: Vec<ReachRow>,
}

/// Cold-cache requests and their cost.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ColdCacheSummary {
    pub requests_considered: u64,
    pub cache_cold_share: f64,
    pub gap_cold_share: f64,
    /// Share of the weighted cost spent rewriting prefixes after a cold cache.
    pub avoidable_rewrite_cost_share: f64,
}

/// The fixed prefix C0 and the rebuilt post-compaction prefix.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FixedPrefixSummary {
    /// Context of the first request of each main session (C0), tokens.
    pub first_request_tokens: Option<Quantiles>,
    /// Context of the first request after each compaction, tokens.
    pub post_compaction_request_tokens: Option<Quantiles>,
    /// The first-order rule the design states.
    pub rule_of_thumb: String,
}

/// How the trigger maps to the configuration value.
#[derive(Debug, Clone, PartialEq)]
pub struct SemanticsSummary {
    pub setting_key: String,
    /// Context window the mapping assumes, tokens.
    pub window_tokens: u32,
    /// Smallest and largest accepted setting.
    pub allowed_setting_range: (u32, u32),
    /// The trigger the CLI uses when nothing is configured.
    pub cli_default_trigger_tokens: u32,
    pub cli_default_note: String,
    /// The mapping rule and where its parameter comes from.
    pub mapping_note: String,
}

/// One compaction regime (a cluster of similar trigger sizes).
#[derive(Debug, Clone, PartialEq)]
pub struct RegimeRow {
    pub center_tokens: u32,
    pub min_tokens: u32,
    pub max_tokens: u32,
    pub count: u64,
    pub is_most_recent: bool,
}

/// One candidate trigger of the simulation table.
#[derive(Debug, Clone, PartialEq)]
pub struct SimulationRow {
    pub trigger_tokens: u32,
    /// The configuration value that produces this trigger.
    pub setting_value: u32,
    /// Weighted cost in input-token equivalents, rework included.
    pub cost_weighted_tokens: f64,
    /// Dollars (API-price equivalent), when every model has a price.
    pub cost_usd: Option<f64>,
    /// Simulated compactions summed over the replayed sessions.
    pub compactions: f64,
    /// Cost above the best candidate (0 for the best).
    pub relative_to_best: f64,
    /// Fraction of the observed compactions at or below this trigger (0..1).
    pub observed_support: f64,
    pub within_2_percent: bool,
    pub within_5_percent: bool,
    pub is_best: bool,
    pub is_pick: bool,
    pub is_cli_default: bool,
    pub is_current: bool,
}

/// The optimum of one model family.
#[derive(Debug, Clone, PartialEq)]
pub struct PerModelRow {
    pub family: String,
    pub sessions: u64,
    pub optimum_trigger_tokens: u32,
    pub optimum_setting_value: u32,
    pub band_low_tokens: u32,
    pub band_high_tokens: u32,
    /// 95% bootstrap interval of the family's optimum.
    pub interval_tokens: (u32, u32),
}

/// One rework sensitivity row.
#[derive(Debug, Clone, PartialEq)]
pub struct ReworkRow {
    pub multiplier: f64,
    pub optimum_trigger_tokens: u32,
    pub band_low_tokens: u32,
    pub band_high_tokens: u32,
    /// Extra cost of the recommended trigger at this multiplier.
    pub pick_overhead: f64,
}

/// Which of the three compared configurations a row describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComparisonKind {
    CliDefault,
    CurrentSetting,
    Recommended,
}

/// CLI default | current setting | recommended.
#[derive(Debug, Clone, PartialEq)]
pub struct ComparisonRow {
    pub kind: ComparisonKind,
    pub label: String,
    pub trigger_tokens: u32,
    pub setting_value: Option<u32>,
    pub cost_weighted_tokens: f64,
    pub cost_usd: Option<f64>,
    pub compactions: f64,
    /// Cost relative to the CLI default (the number users look at).
    pub relative_to_default: f64,
    pub relative_to_recommended: f64,
    pub quality: PointQuality,
    pub quality_note: String,
}

/// The bootstrap of the optimum.
#[derive(Debug, Clone, PartialEq)]
pub struct BootstrapRow {
    pub resamples: u64,
    pub argmin_p2_5: u32,
    pub argmin_median: u32,
    pub argmin_p97_5: u32,
    /// Share of resamples in which the pick stayed within 2% of the best.
    pub pick_within_band_share: f64,
}

/// The recommendation shown at the top of the report.
#[derive(Debug, Clone, PartialEq)]
pub struct RecommendationCard {
    /// Effective trigger, tokens.
    pub trigger_tokens: u32,
    pub setting_key: String,
    pub setting_value: u32,
    /// The value had to be clamped into the accepted range.
    pub setting_clamped: bool,
    /// "Optimal: autoCompactWindow 250,000 (trigger ~217k, within 2% of best)".
    pub headline_text: String,
    /// No observed compaction supports this level.
    pub extrapolated: bool,
    /// "extrapolated: 0% of observed compactions at or below this level".
    pub observed_support_text: String,
    pub observed_support_fraction: f64,
    pub confidence: Confidence,
    /// Why this trigger (the crate's basis text).
    pub basis: String,
    /// Which branch of the pick rule produced the pick.
    pub pick_rule: PickRule,
    /// The pick rule in a sentence, for the card.
    pub pick_rule_text: String,
    /// Pick rule "supported optimum" only: the cheaper trigger that no
    /// observed compaction supports.
    pub unsupported_argmin_tokens: Option<u32>,
    /// Same: how much cheaper it is in the simulation, `1 - cost / cost(pick)`.
    pub unsupported_relative_saving: Option<f64>,
    /// The plain argmin still sits at the lowest or highest simulated trigger
    /// (after one automatic widening of the grid), so the true optimum may lie
    /// outside the grid.
    pub grid_floor_limited: bool,
    /// The grid was widened once because the first optimum sat at its edge.
    pub grid_widened: bool,
    /// Present when the pick is the supported one (pick rule b or c) and a
    /// cheaper trigger exists that no observed compaction supports: the same
    /// recommendation for that trigger, so the user can choose it knowingly.
    /// Always `None` for the plain-argmin pick (it is itself extrapolated).
    pub extrapolated_alternative: Option<ExtrapolatedAlternative>,
    /// Lowest and highest trigger within 2% of the best.
    pub band_2_percent_tokens: (u32, u32),
    pub band_5_percent_tokens: (u32, u32),
    /// Cost of the current setting relative to the recommendation, when a
    /// current setting exists ("+8.2%": the current one is costlier).
    pub current_vs_recommended: Option<f64>,
    pub warnings: Vec<String>,
}

/// A ready-to-apply alternative to the supported pick: the simulated optimum
/// that no observed compaction supports.
#[derive(Debug, Clone, PartialEq)]
pub struct ExtrapolatedAlternative {
    /// Effective trigger, tokens.
    pub trigger_tokens: u32,
    pub setting_key: String,
    /// The configuration value that produces the trigger (clamped into the
    /// accepted range, see `setting_clamped`).
    pub setting_value: u64,
    pub setting_clamped: bool,
    /// How much cheaper than the supported pick it is in the simulation
    /// (`1 - cost / cost(pick)`, 0..1).
    pub relative_saving: f64,
    /// "extrapolated: 0.0% of observed compactions at or below this level".
    pub observed_support_text: String,
    /// "Simulated optimum: autoCompactWindow 233,000 (trigger ~200k, 39.7%
    /// cheaper than the supported pick, EXTRAPOLATED: no observed compactions
    /// at or below this level)".
    pub headline_text: String,
}

impl RecommendationCard {
    /// Lowercase label of [`Self::confidence`].
    pub fn confidence_label(&self) -> &'static str {
        match self.confidence {
            Confidence::Low => "low",
            Confidence::Medium => "medium",
            Confidence::High => "high",
        }
    }
}

/// Everything the Optimization tab shows for one agent.
#[derive(Debug, Clone, PartialEq)]
pub struct CompactionReport {
    pub agent: AgentKind,
    pub generated_at_unix_ms: i64,
    pub status: ReportStatus,
    /// Human sentence explaining a non-complete status.
    pub status_message: Option<String>,
    pub corpus: CorpusSummary,
    pub compactions: CompactionSummary,
    pub cost_mix: CostMixSummary,
    pub cold_cache: ColdCacheSummary,
    pub fixed_prefix: FixedPrefixSummary,
    pub semantics: SemanticsSummary,
    pub regimes: Vec<RegimeRow>,
    /// Absent when nothing could be replayed.
    pub recommendation: Option<RecommendationCard>,
    pub simulation: Vec<SimulationRow>,
    pub bootstrap: Option<BootstrapRow>,
    pub per_model: Vec<PerModelRow>,
    /// The visible message that the setting cannot be applied per model.
    pub per_model_note: String,
    pub rework: Vec<ReworkRow>,
    /// Where the rework figure comes from and how large it is.
    pub rework_note: String,
    pub comparison: Vec<ComparisonRow>,
    /// Extrapolation, priors, unpriced models, truncation, price caveat.
    pub warnings: Vec<String>,
}

const FIXED_PREFIX_RULE: &str = "Rule of thumb (first order): every ~10k tokens removed from the fixed prefix lowers the optimal trigger by about 10-15k tokens. Ilium's own injected instruction text counts toward the prefix.";
/// Trigger candidates added to the grid in addition to the crate's defaults.
const CLAUDE_EXTRA_TRIGGERS: [u32; 3] = [167_000, 217_000, 267_000];
/// Offsets (setting minus firing size) a re-measurement may report; a value
/// outside means the newest regime ran under another setting.
const PLAUSIBLE_CLAUDE_OFFSET: std::ops::RangeInclusive<f64> = 10_000.0..=60_000.0;
/// Realized trigger over limit a re-measurement may report.
const PLAUSIBLE_CODEX_RATIO: std::ops::RangeInclusive<f64> = 0.8..=1.2;
/// Lowest Codex trigger the automatic grid widening tries.
const CODEX_TRIGGER_FLOOR: u32 = 60_000;
/// Models the unpriced warning names before it truncates.
const UNPRICED_LIST_LIMIT: usize = 5;

impl CompactionReport {
    /// Computes the report. Pure: only calls into the analysis crate.
    pub fn build(input: &ReportInput<'_>) -> Self {
        let agent = input.agent;
        let traces = input.traces;
        let table_prices = &input.settings.price_table;
        let lookup = |model: &str| price_weights(table_prices, model);
        let stats = CorpusStats::compute(agent, traces, &StatsConfig::default(), Some(&lookup));
        let current_setting = input
            .settings
            .current_setting_value
            .map(|value| u32::try_from(value).unwrap_or(u32::MAX));
        let semantics = semantics_for(agent, traces, &stats, current_setting);
        let window_tokens = semantics_window(&semantics, agent);
        let defaults = semantics.cli_defaults(window_tokens);
        let current_trigger = current_setting.map(|value| semantics.setting_to_trigger(value));

        let mut config = SimConfig::for_agent(agent);
        config.triggers = default_grid(agent, &semantics);
        let mut extras = vec![defaults.trigger_tokens];
        extras.extend(current_trigger);
        let config = config.with_extra_triggers(&extras);

        let mut warnings: Vec<String> = input.scan.warnings.clone();
        let mut report = Self {
            agent,
            generated_at_unix_ms: input.generated_at_unix_ms,
            status: ReportStatus::Complete,
            status_message: None,
            corpus: corpus_summary(&stats, input.scan),
            compactions: compaction_summary(&stats),
            cost_mix: cost_mix_summary(&stats),
            cold_cache: cold_cache_summary(&stats),
            fixed_prefix: fixed_prefix_summary(&stats),
            semantics: semantics_summary(&semantics, window_tokens, &defaults),
            regimes: regime_rows(&stats),
            recommendation: None,
            simulation: Vec::new(),
            bootstrap: None,
            per_model: Vec::new(),
            per_model_note: String::new(),
            rework: Vec::new(),
            rework_note: String::new(),
            comparison: Vec::new(),
            warnings: Vec::new(),
        };

        let outcome = run_optimizer(
            agent,
            traces,
            &config,
            &lookup,
            &semantics,
            defaults.trigger_tokens,
            current_trigger,
        );
        match outcome {
            Ok(run) => {
                report.fill_from_recommendation(
                    &run,
                    &semantics,
                    defaults.trigger_tokens,
                    current_trigger,
                    current_setting,
                );
                if let Some((low, high)) = run.widened_grid {
                    warnings.push(format!(
                        "The simulated grid was widened to {}-{} because the first optimum sat at its edge, so the reported optimum is not an artefact of the grid.",
                        humanize_tokens(f64::from(low)),
                        humanize_tokens(f64::from(high))
                    ));
                }
                warnings.extend(run.recommendation.warnings.iter().cloned());
            }
            Err(failure) => {
                let (status, message) = no_recommendation(failure, &stats, agent);
                report.status = status;
                report.status_message = Some(message);
            }
        }

        warnings.extend(report_warnings(agent, &stats, input.scan, &report));
        report.warnings = unique(warnings);
        if let Some(card) = report.recommendation.as_mut() {
            card.warnings = report.warnings.clone();
        }
        report
    }

    fn fill_from_recommendation(
        &mut self,
        run: &OptimizerRun,
        semantics: &AgentSemantics,
        default_trigger: u32,
        current_trigger: Option<u32>,
        current_setting: Option<u32>,
    ) {
        let recommendation = &run.recommendation;
        let comparison = &run.comparison;
        let grid_widened = run.widened_grid.is_some();
        let band_2 = &recommendation.bands.within_2_percent;
        let band_5 = &recommendation.bands.within_5_percent;
        let pick = recommendation.trigger_tokens;
        self.simulation = recommendation
            .candidates
            .iter()
            .map(|row| SimulationRow {
                trigger_tokens: row.trigger_tokens,
                setting_value: semantics.trigger_to_setting(row.trigger_tokens).value,
                cost_weighted_tokens: row.cost,
                cost_usd: row.usd,
                compactions: row.compactions,
                relative_to_best: row.excess_fraction.max(0.0),
                observed_support: row.observed_support,
                within_2_percent: band_2.members.contains(&row.trigger_tokens),
                within_5_percent: band_5.members.contains(&row.trigger_tokens),
                is_best: row.trigger_tokens == recommendation.bands.argmin_tokens,
                is_pick: row.trigger_tokens == pick,
                is_cli_default: row.trigger_tokens == default_trigger,
                is_current: current_trigger == Some(row.trigger_tokens),
            })
            .collect();
        self.bootstrap = Some(BootstrapRow {
            resamples: recommendation.bootstrap.resamples as u64,
            argmin_p2_5: recommendation.bootstrap.argmin_p2_5,
            argmin_median: recommendation.bootstrap.argmin_median,
            argmin_p97_5: recommendation.bootstrap.argmin_p97_5,
            pick_within_band_share: recommendation.bootstrap.pick_within_band_share,
        });
        self.per_model = recommendation
            .per_model
            .iter()
            .map(|model| PerModelRow {
                family: model.family.clone(),
                sessions: model.sessions as u64,
                optimum_trigger_tokens: model.argmin_tokens,
                optimum_setting_value: semantics.trigger_to_setting(model.argmin_tokens).value,
                band_low_tokens: model.within_2_percent.low_tokens,
                band_high_tokens: model.within_2_percent.high_tokens,
                interval_tokens: model.argmin_interval_tokens,
            })
            .collect();
        self.per_model_note = format!(
            "The setting ({}) is one global value and cannot be applied per model. Apply uses the cost-weighted optimum of all models; the per-model optima only show the spread.",
            semantics.setting_name()
        );
        self.rework = recommendation
            .rework_sensitivity
            .iter()
            .map(|row| ReworkRow {
                multiplier: row.multiplier,
                optimum_trigger_tokens: row.argmin_tokens,
                band_low_tokens: row.within_2_percent.low_tokens,
                band_high_tokens: row.within_2_percent.high_tokens,
                pick_overhead: row.pick_overhead_fraction.max(0.0),
            })
            .collect();
        self.rework_note = rework_note(recommendation);
        self.comparison = comparison
            .iter()
            .map(|point| comparison_row(point, semantics, current_setting))
            .collect();
        let current_vs_recommended = comparison
            .iter()
            .find(|point| point.label == "current")
            .map(|point| point.relative_to_recommended);
        self.recommendation = Some(recommendation_card(
            recommendation,
            semantics,
            current_vs_recommended,
            grid_widened,
        ));
    }

    /// Whether the report carries a recommendation.
    pub fn has_recommendation(&self) -> bool {
        self.recommendation.is_some()
    }

    /// Multi-line text of the whole report, for tests and log export.
    pub fn to_plain_text(&self) -> String {
        let mut text = String::new();
        let _ = writeln!(
            text,
            "Compaction report: {}",
            match self.agent {
                AgentKind::ClaudeCode => "Claude Code",
                AgentKind::Codex => "Codex",
            }
        );
        if let Some(message) = &self.status_message {
            let _ = writeln!(text, "Status: {message}");
        }
        if let Some(card) = &self.recommendation {
            let _ = writeln!(text, "\n== Recommendation ==");
            let _ = writeln!(text, "{}", card.headline_text);
            let _ = writeln!(
                text,
                "Support: {} | confidence {}",
                card.observed_support_text,
                card.confidence_label()
            );
            let _ = writeln!(text, "Basis: {}", card.basis);
            let _ = writeln!(text, "Pick rule: {}", card.pick_rule_text);
            if let (Some(tokens), Some(saving)) = (
                card.unsupported_argmin_tokens,
                card.unsupported_relative_saving,
            ) {
                let _ = writeln!(
                    text,
                    "Cheaper but unsupported: {} ({} cheaper in simulation, no observed compactions there)",
                    humanize_tokens(f64::from(tokens)),
                    format_percent(saving.max(0.0))
                );
            }
            if let Some(alternative) = &card.extrapolated_alternative {
                let _ = writeln!(text, "Alternative: {}", alternative.headline_text);
            }
            if card.grid_widened {
                let _ = writeln!(text, "The simulated grid was widened automatically.");
            }
            if card.grid_floor_limited {
                let _ = writeln!(
                    text,
                    "The optimum is still at the edge of the grid; the true optimum may lie outside it."
                );
            }
            let _ = writeln!(
                text,
                "Flat band: within 2% {}-{}, within 5% {}-{}",
                humanize_tokens(f64::from(card.band_2_percent_tokens.0)),
                humanize_tokens(f64::from(card.band_2_percent_tokens.1)),
                humanize_tokens(f64::from(card.band_5_percent_tokens.0)),
                humanize_tokens(f64::from(card.band_5_percent_tokens.1))
            );
        }
        self.write_corpus(&mut text);
        self.write_compactions(&mut text);
        self.write_cost(&mut text);
        self.write_simulation(&mut text);
        self.write_comparison(&mut text);
        if !self.warnings.is_empty() {
            let _ = writeln!(text, "\n== Warnings ==");
            for warning in &self.warnings {
                let _ = writeln!(text, "- {warning}");
            }
        }
        text
    }

    fn write_corpus(&self, text: &mut String) {
        let corpus = &self.corpus;
        let _ = writeln!(text, "\n== Corpus ==");
        let _ = writeln!(
            text,
            "{} files ({}), {} main sessions / {} subagent sessions, {} / {} requests ({} subagent), {:.1} days",
            group_thousands(corpus.files_listed),
            format_bytes(corpus.bytes_total),
            group_thousands(corpus.main_sessions),
            group_thousands(corpus.subagent_sessions),
            group_thousands(corpus.main_requests),
            group_thousands(corpus.subagent_requests),
            format_percent(corpus.subagent_request_share),
            corpus.span_days
        );
        let _ = writeln!(
            text,
            "Scan: {} parsed, {} from cache, {} skipped, {:.1}s",
            corpus.files_parsed, corpus.files_from_cache, corpus.files_skipped, corpus.scan_seconds
        );
        let _ = writeln!(text, "{}", self.semantics.mapping_note);
        for regime in &self.regimes {
            let _ = writeln!(
                text,
                "Regime{}: {} compactions around {} ({}-{})",
                if regime.is_most_recent {
                    " (most recent)"
                } else {
                    ""
                },
                regime.count,
                humanize_tokens(f64::from(regime.center_tokens)),
                humanize_tokens(f64::from(regime.min_tokens)),
                humanize_tokens(f64::from(regime.max_tokens))
            );
        }
    }

    fn write_compactions(&self, text: &mut String) {
        let stats = &self.compactions;
        let _ = writeln!(text, "\n== Compactions ==");
        let unknown = if stats.unknown_trigger > 0 {
            format!(", {} of unstated trigger", stats.unknown_trigger)
        } else {
            String::new()
        };
        let _ = writeln!(
            text,
            "{} total ({} auto, {} manual{unknown}, {} measured) in {} sessions",
            stats.total, stats.auto, stats.manual, stats.measured, stats.sessions_with_compaction
        );
        if let Some(quantiles) = &stats.pre_tokens {
            let _ = writeln!(text, "Fired at: {}", format_token_quantiles(quantiles));
        }
        if let Some(quantiles) = &stats.first_post_request_tokens {
            let _ = writeln!(
                text,
                "First request after: {}",
                format_token_quantiles(quantiles)
            );
        }
        if let Some(quantiles) = &stats.summary_tokens {
            let _ = writeln!(text, "Summary: {}", format_token_quantiles(quantiles));
        }
        let _ = writeln!(
            text,
            "Cycles: {} ({} of at most 3 requests, {} of at most 10, {} of at most 30)",
            stats.cycles, stats.thrash_cycles, stats.cycles_at_most_10, stats.cycles_at_most_30
        );
        if let Some(quantiles) = &self.fixed_prefix.first_request_tokens {
            let _ = writeln!(
                text,
                "Fixed prefix C0: {}",
                format_token_quantiles(quantiles)
            );
        }
        let _ = writeln!(text, "{}", self.fixed_prefix.rule_of_thumb);
    }

    fn write_cost(&self, text: &mut String) {
        let mix = &self.cost_mix;
        let _ = writeln!(text, "\n== Cost mix ==");
        let _ = writeln!(
            text,
            "input {} | output {} | cache read {} | cache write {} | cold-cache requests {} (idle gap {}), avoidable rewrites {} of cost",
            format_percent(mix.input_share),
            format_percent(mix.output_share),
            format_percent(mix.cache_read_share),
            format_percent(mix.cache_write_share),
            format_percent(self.cold_cache.cache_cold_share),
            format_percent(self.cold_cache.gap_cold_share),
            format_percent(self.cold_cache.avoidable_rewrite_cost_share)
        );
        for reach in &mix.reach {
            let _ = writeln!(
                text,
                "{} of cost above {} context ({} sessions reach it)",
                format_percent(reach.cost_share_above),
                humanize_tokens(f64::from(reach.threshold_tokens)),
                reach.sessions_reaching
            );
        }
    }

    fn write_simulation(&self, text: &mut String) {
        if self.simulation.is_empty() {
            return;
        }
        let _ = writeln!(text, "\n== Simulation ==");
        for row in &self.simulation {
            let _ = writeln!(
                text,
                "{:>7} (setting {:>7}) cost {:>8} {:>10} {:>7.1} compactions {:>7} support {:>4}{}{}{}{}",
                humanize_tokens(f64::from(row.trigger_tokens)),
                group_thousands(u64::from(row.setting_value)),
                humanize_tokens(row.cost_weighted_tokens),
                row.cost_usd.map_or_else(|| "n/a".into(), format_usd),
                row.compactions,
                format_signed_percent(row.relative_to_best),
                format_percent(row.observed_support),
                if row.is_pick { " <pick" } else { "" },
                if row.is_best { " <best" } else { "" },
                if row.within_2_percent { " [2%]" } else { "" },
                if row.within_5_percent { " [5%]" } else { "" },
            );
        }
        for row in &self.per_model {
            let _ = writeln!(
                text,
                "Model {}: {} sessions, optimum {} (band {}-{})",
                row.family,
                row.sessions,
                humanize_tokens(f64::from(row.optimum_trigger_tokens)),
                humanize_tokens(f64::from(row.band_low_tokens)),
                humanize_tokens(f64::from(row.band_high_tokens))
            );
        }
        let _ = writeln!(text, "{}", self.per_model_note);
        let _ = writeln!(text, "{}", self.rework_note);
        for row in &self.rework {
            let _ = writeln!(
                text,
                "Rework x{}: optimum {} (band {}-{})",
                row.multiplier,
                humanize_tokens(f64::from(row.optimum_trigger_tokens)),
                humanize_tokens(f64::from(row.band_low_tokens)),
                humanize_tokens(f64::from(row.band_high_tokens))
            );
        }
    }

    fn write_comparison(&self, text: &mut String) {
        if self.comparison.is_empty() {
            return;
        }
        let _ = writeln!(text, "\n== CLI default | current | recommended ==");
        for row in &self.comparison {
            let _ = writeln!(
                text,
                "{}: trigger {} cost {} {} vs default {} ({})",
                row.label,
                humanize_tokens(f64::from(row.trigger_tokens)),
                humanize_tokens(row.cost_weighted_tokens),
                row.cost_usd.map_or_else(|| "n/a".into(), format_usd),
                format_signed_percent(row.relative_to_default),
                row.quality_note
            );
        }
    }
}

// ----------------------------------------------------------------- pipeline

/// Replays the corpus, optimizes, and prices the three-way comparison.
/// What one optimizer run produced.
struct OptimizerRun {
    recommendation: Recommendation,
    comparison: Vec<ComparisonPoint>,
    /// Lowest and highest trigger of the grid when it had to be widened.
    widened_grid: Option<(u32, u32)>,
}

/// Replays the corpus, optimizes, and prices the three-way comparison. When
/// the plain optimum sits at the edge of the grid, the grid is widened once
/// and the optimizer re-run, so the reported optimum is not an artefact of the
/// grid; the crate's edge warning stays if it is still at the edge.
fn run_optimizer(
    agent: AgentKind,
    traces: &[SessionTrace],
    config: &SimConfig,
    lookup: &dyn Fn(&str) -> Option<PriceWeights>,
    semantics: &AgentSemantics,
    default_trigger: u32,
    current_trigger: Option<u32>,
) -> Result<OptimizerRun, AnalysisError> {
    // The support population is the one the replayed optimum covers.
    let observed = ObservedCompactions::main_sessions_only(traces, agent);
    let optimize_config = OptimizeConfig::for_agent(agent);
    let mut table = simulate(agent, traces, config, Some(lookup))?;
    let mut recommendation = optimize(&table, &observed, semantics, &optimize_config)?;
    let mut widened_grid = None;
    if recommendation.grid_floor_limited {
        let extra = widening_triggers(
            agent,
            semantics,
            &table.triggers,
            recommendation.bands.argmin_tokens,
        );
        if !extra.is_empty() {
            let widened = config.clone().with_extra_triggers(&extra);
            let wider_table = simulate(agent, traces, &widened, Some(lookup))?;
            recommendation = optimize(&wider_table, &observed, semantics, &optimize_config)?;
            widened_grid = Some((
                wider_table.triggers.first().copied().unwrap_or(0),
                wider_table.triggers.last().copied().unwrap_or(0),
            ));
            table = wider_table;
        }
    }
    let comparison = three_way_comparison(
        &table,
        &recommendation.rework,
        default_trigger,
        current_trigger,
        recommendation.trigger_tokens,
    );
    Ok(OptimizerRun {
        recommendation,
        comparison,
        widened_grid,
    })
}

/// Triggers added when the plain optimum is the lowest or highest grid point.
///
/// Floor: four points from the lowest trigger a setting can produce (Claude:
/// the minimum accepted setting minus the offset; Codex: 60k) up to the current
/// floor. Ceiling: four points from the current ceiling up to the highest
/// trigger the setting range allows (Claude 1M window minus the offset; Codex
/// already ends at its cap). Empty when the grid already reaches the limit.
fn widening_triggers(
    agent: AgentKind,
    semantics: &AgentSemantics,
    grid: &[u32],
    argmin: u32,
) -> Vec<u32> {
    let (Some(&first), Some(&last)) = (grid.first(), grid.last()) else {
        return Vec::new();
    };
    let round = |tokens: u64| u32::try_from(tokens / 1_000 * 1_000).unwrap_or(u32::MAX);
    let ladder = |from: u32, to: u32| -> Vec<u32> {
        (0..4_u64)
            .map(|step| round(u64::from(from) + (u64::from(to) - u64::from(from)) * step / 4))
            .collect()
    };
    let (setting_low, setting_high) = semantics.allowed_setting_range();
    if argmin == first {
        let limit = match agent {
            AgentKind::ClaudeCode => semantics.setting_to_trigger(setting_low),
            AgentKind::Codex => CODEX_TRIGGER_FLOOR,
        };
        if limit < first {
            let mut points = ladder(limit, first);
            points.retain(|&point| point < first);
            return points;
        }
    }
    if argmin == last {
        let limit = semantics.setting_to_trigger(setting_high);
        if limit > last {
            let mut points = ladder(last, limit);
            points.retain(|&point| point > last);
            points.push(limit);
            return points;
        }
    }
    Vec::new()
}

fn no_recommendation(
    failure: AnalysisError,
    stats: &CorpusStats,
    agent: AgentKind,
) -> (ReportStatus, String) {
    let sessions = stats.counts.main_sessions + stats.counts.subagent_sessions;
    let minimum = agent.profile().replay_min_context_tokens;
    match failure {
        AnalysisError::NoReplayableSession if sessions == 0 => (
            ReportStatus::NoSessionsFound,
            "No sessions found: no transcript with usable requests exists for this agent, so there is nothing to analyse.".into(),
        ),
        AnalysisError::NoReplayableSession => (
            ReportStatus::NothingToReplay,
            format!(
                "No session grew beyond {} tokens of context, so no compaction trigger can be simulated yet. The statistics above still describe your {} sessions.",
                humanize_tokens(f64::from(minimum)),
                sessions
            ),
        ),
        other => (
            ReportStatus::NothingToReplay,
            format!("The simulation could not run: {other}."),
        ),
    }
}

/// The mapping of trigger to setting. The defaults are the researched
/// values; when the caller knows the setting now in force, the parameter is
/// re-measured from the most recent compaction regime (design section 9.1).
///
/// The history of settings is unknown, so the current setting is assumed to
/// have been in force for the newest regime. A measurement that contradicts
/// that assumption (an implausible offset or ratio, because the regime ran
/// under another setting) is discarded and the default kept.
fn semantics_for(
    agent: AgentKind,
    traces: &[SessionTrace],
    stats: &CorpusStats,
    current_setting: Option<u32>,
) -> AgentSemantics {
    let defaults = match agent {
        AgentKind::ClaudeCode => AgentSemantics::default_for(agent),
        AgentKind::Codex => match stated_window(traces, agent) {
            Some(window) => AgentSemantics::codex_with_window(window),
            None => AgentSemantics::default_for(agent),
        },
    };
    let Some(setting) = current_setting else {
        return defaults;
    };
    let regime = stats
        .regimes
        .most_recent()
        .map(|regime| regime.pre_token_range());
    let in_force = |_timestamp_ms: i64| Some(setting);
    match defaults {
        AgentSemantics::Claude(_) => {
            let observations = claude_offset_observations(traces, regime, &in_force);
            match measure_claude_offset(&observations) {
                Some(measured) if PLAUSIBLE_CLAUDE_OFFSET.contains(&measured.value) => {
                    defaults.with_measured_offset(measured.value.round() as u32)
                }
                _ => defaults,
            }
        }
        AgentSemantics::Codex(codex) => {
            let observations = codex_ratio_observations(traces, regime, &in_force);
            match measure_codex_ratio(&observations, &codex) {
                Some(measured) if PLAUSIBLE_CODEX_RATIO.contains(&measured.value) => {
                    defaults.with_measured_ratio(measured.value)
                }
                _ => defaults,
            }
        }
    }
}

/// The most common context window the logs state for `agent`.
fn stated_window(traces: &[SessionTrace], agent: AgentKind) -> Option<u32> {
    let mut counts: std::collections::BTreeMap<u32, usize> = std::collections::BTreeMap::new();
    for window in traces
        .iter()
        .filter(|trace| trace.agent == agent && !trace.is_subagent)
        .filter_map(|trace| trace.context_window_tokens)
        .filter(|&window| window > 0)
    {
        *counts.entry(window).or_default() += 1;
    }
    counts
        .into_iter()
        .max_by_key(|&(window, count)| (count, window))
        .map(|(window, _)| window)
}

fn semantics_window(semantics: &AgentSemantics, agent: AgentKind) -> u32 {
    match semantics {
        AgentSemantics::Codex(codex) => codex.window_tokens,
        AgentSemantics::Claude(_) => agent.profile().default_context_window_tokens,
    }
}

/// The candidate triggers: Claude 120k..600k in 25k steps plus the historical
/// 567k and the points a round setting produces (setting minus the offset,
/// for example 250,000 gives 217,000); Codex 100k..230k in 10k steps plus
/// 225k and the window cap.
fn default_grid(agent: AgentKind, semantics: &AgentSemantics) -> Vec<u32> {
    let mut grid: Vec<u32> = match agent {
        AgentKind::ClaudeCode => {
            let mut points: Vec<u32> = vec![120_000, 567_000];
            points.extend((125_000..=600_000).step_by(25_000));
            points.extend(CLAUDE_EXTRA_TRIGGERS);
            points
        }
        AgentKind::Codex => {
            let mut points: Vec<u32> = (100_000..=230_000).step_by(10_000).collect();
            points.push(225_000);
            points
        }
    };
    let cap = semantics.setting_to_trigger(semantics.allowed_setting_range().1);
    if agent == AgentKind::Codex {
        grid.retain(|&trigger| trigger <= cap);
        grid.push(cap);
    }
    grid.sort_unstable();
    grid.dedup();
    grid
}

/// Dollars-per-million weights of the client's price table. The table's cache
/// write price is the 5-minute rate (1.25x input); the 1-hour tier costs 2x
/// the input price. The analysis crate drops the write premium for Codex.
fn price_weights(table: &PriceTable, model: &str) -> Option<PriceWeights> {
    let price = table.lookup(model)?;
    Some(PriceWeights {
        input: price.input,
        output: price.output,
        cache_read: price.cache_read,
        cache_write_5m: price.cache_write,
        cache_write_1h: price.input * 2.0,
    })
}

// ----------------------------------------------------------- section builders

fn corpus_summary(stats: &CorpusStats, scan: &ScanSummary) -> CorpusSummary {
    let counts = &stats.counts;
    let requests = counts.main_turns + counts.subagent_turns;
    let span_ms = (counts.last_timestamp_ms - counts.first_timestamp_ms).max(0);
    CorpusSummary {
        files_listed: scan.files_listed,
        files_parsed: scan.files_parsed,
        files_from_cache: scan.files_from_cache,
        files_skipped: scan.files_skipped,
        bytes_total: scan.bytes_total,
        main_sessions: counts.main_sessions as u64,
        subagent_sessions: counts.subagent_sessions as u64,
        main_requests: counts.main_turns as u64,
        subagent_requests: counts.subagent_turns as u64,
        subagent_request_share: if requests == 0 {
            0.0
        } else {
            counts.subagent_turns as f64 / requests as f64
        },
        first_timestamp_ms: counts.first_timestamp_ms,
        last_timestamp_ms: counts.last_timestamp_ms,
        span_days: span_ms as f64 / 86_400_000.0,
        lines_skipped: counts.lines_skipped,
        duplicate_turns_removed: scan.duplicate_turns_removed,
        sessions_dropped_for_cap: scan.sessions_dropped_for_cap,
        scan_seconds: scan.scan_seconds,
    }
}

fn compaction_summary(stats: &CorpusStats) -> CompactionSummary {
    let counts = &stats.counts;
    let compactions = &stats.compactions;
    let cycles = &stats.cycles;
    CompactionSummary {
        total: counts.compactions as u64,
        auto: counts.auto_compactions as u64,
        manual: counts.manual_compactions as u64,
        unknown_trigger: counts
            .compactions
            .saturating_sub(counts.auto_compactions + counts.manual_compactions)
            as u64,
        measured: counts.measured_compactions as u64,
        sessions_with_compaction: counts.sessions_with_compaction as u64,
        pre_tokens: compactions.pre_tokens,
        pre_tokens_auto: compactions.pre_tokens_auto,
        first_post_request_tokens: compactions.first_post_request_tokens,
        first_post_cache_read_share: compactions.first_post_cache_read_share,
        summary_tokens: compactions.summary_tokens,
        duration_seconds: compactions.duration_seconds,
        cycles: cycles.cycles as u64,
        cycle_requests: cycles.turns,
        cycle_minutes: cycles.minutes,
        thrash_cycles: cycles.at_most_3_turns as u64,
        cycles_at_most_10: cycles.at_most_10_turns as u64,
        cycles_at_most_30: cycles.at_most_30_turns as u64,
        requests_to_first_compaction: cycles.turns_to_first_compaction,
    }
}

fn cost_mix_summary(stats: &CorpusStats) -> CostMixSummary {
    let mix = &stats.cost_mix;
    let total_weighted: f64 = stats.models.iter().map(|model| model.weighted_cost).sum();
    CostMixSummary {
        total_weighted_tokens: mix.total_weighted,
        total_usd: mix.usd_complete.then_some(mix.total_usd),
        input_share: mix.input_share,
        output_share: mix.output_share,
        cache_read_share: mix.cache_read_share,
        cache_write_share: mix.cache_write_share,
        models: stats
            .models
            .iter()
            .map(|model| ModelUsageRow {
                model: model.model.clone(),
                family: model.family.clone(),
                requests: model.turns,
                weighted_cost_share: if total_weighted > 0.0 {
                    model.weighted_cost / total_weighted
                } else {
                    0.0
                },
                usd: model.usd,
            })
            .collect(),
        reach: stats
            .reach
            .iter()
            .map(|row| ReachRow {
                threshold_tokens: row.threshold_tokens,
                sessions_reaching: row.sessions_reaching as u64,
                cost_share_above: row.cost_share_of_turns_above,
            })
            .collect(),
    }
}

fn cold_cache_summary(stats: &CorpusStats) -> ColdCacheSummary {
    let cold = &stats.cold_cache;
    ColdCacheSummary {
        requests_considered: cold.turns_considered,
        cache_cold_share: cold.cache_cold_share,
        gap_cold_share: cold.gap_cold_share,
        avoidable_rewrite_cost_share: cold.rewrite_cost_share,
    }
}

fn fixed_prefix_summary(stats: &CorpusStats) -> FixedPrefixSummary {
    FixedPrefixSummary {
        first_request_tokens: stats.first_request_tokens,
        post_compaction_request_tokens: stats.compactions.first_post_request_tokens,
        rule_of_thumb: FIXED_PREFIX_RULE.to_owned(),
    }
}

fn semantics_summary(
    semantics: &AgentSemantics,
    window_tokens: u32,
    defaults: &ilium_compaction_analysis::semantics::CliDefaults,
) -> SemanticsSummary {
    let mapping_note = match semantics {
        AgentSemantics::Claude(claude) => format!(
            "autoCompactWindow = trigger + {} tokens ({}); accepted {}-{}.",
            group_thousands(u64::from(claude.offset_tokens)),
            source_label(claude.offset_source),
            group_thousands(u64::from(claude.min_setting_tokens)),
            group_thousands(u64::from(claude.max_setting_tokens)),
        ),
        AgentSemantics::Codex(codex) => format!(
            "model_auto_compact_token_limit is clamped to {:.0}% of the {}-token window (cap {}); realized trigger = limit x {:.2} ({}).",
            codex.clamp_fraction * 100.0,
            group_thousands(u64::from(codex.window_tokens)),
            group_thousands(u64::from(codex.setting_cap())),
            codex.realized_ratio,
            source_label(codex.ratio_source),
        ),
    };
    SemanticsSummary {
        setting_key: semantics.setting_name().to_owned(),
        window_tokens,
        allowed_setting_range: semantics.allowed_setting_range(),
        cli_default_trigger_tokens: defaults.trigger_tokens,
        cli_default_note: defaults.note.clone(),
        mapping_note,
    }
}

fn source_label(source: ParameterSource) -> &'static str {
    match source {
        ParameterSource::Default => "researched default, not re-measured on these logs",
        ParameterSource::Measured => "measured on these logs",
    }
}

fn regime_rows(stats: &CorpusStats) -> Vec<RegimeRow> {
    stats
        .regimes
        .regimes
        .iter()
        .map(|regime| RegimeRow {
            center_tokens: regime.center_tokens,
            min_tokens: regime.min_tokens,
            max_tokens: regime.max_tokens,
            count: regime.count as u64,
            is_most_recent: regime.is_most_recent,
        })
        .collect()
}

fn rework_note(recommendation: &Recommendation) -> String {
    let rework = &recommendation.rework;
    let origin = match rework.source {
        ReworkSource::Prior => {
            "a default measured on one research corpus, not on these logs (it is replaced by a measurement once your logs hold about 30 compactions)"
        }
        ReworkSource::Measured => "measured on these logs",
    };
    format!(
        "Rework per compaction: {} weighted tokens, {origin}. Rows show the optimum at 0.5x, 1x and 2x of it.",
        humanize_tokens(rework.effective_tokens())
    )
}

fn comparison_row(
    point: &ComparisonPoint,
    semantics: &AgentSemantics,
    current_setting: Option<u32>,
) -> ComparisonRow {
    let (kind, label) = match point.label.as_str() {
        "default" => (ComparisonKind::CliDefault, "CLI default"),
        "current" => (ComparisonKind::CurrentSetting, "Current setting"),
        _ => (ComparisonKind::Recommended, "Recommended"),
    };
    let setting_value = match kind {
        ComparisonKind::CurrentSetting => current_setting,
        _ => Some(semantics.trigger_to_setting(point.trigger_tokens).value),
    };
    let quality_note = match point.quality {
        PointQuality::Exact => "simulated point",
        PointQuality::Interpolated => "interpolated between two simulated points",
        PointQuality::OutsideGrid => "outside the simulated range; nearest point shown",
    };
    ComparisonRow {
        kind,
        label: label.to_owned(),
        trigger_tokens: point.trigger_tokens,
        setting_value,
        cost_weighted_tokens: point.cost,
        cost_usd: point.usd,
        compactions: point.compactions,
        relative_to_default: point.relative_to_default,
        relative_to_recommended: point.relative_to_recommended,
        quality: point.quality,
        quality_note: quality_note.to_owned(),
    }
}

fn recommendation_card(
    recommendation: &Recommendation,
    semantics: &AgentSemantics,
    current_vs_recommended: Option<f64>,
    grid_widened: bool,
) -> RecommendationCard {
    let pick = recommendation.trigger_tokens;
    let pick_row = recommendation
        .candidates
        .iter()
        .find(|row| row.trigger_tokens == pick);
    let excess = pick_row.map_or(0.0, |row| row.excess_fraction.max(0.0));
    let closeness = if excess < 0.0005 {
        "the simulated best".to_owned()
    } else if excess <= 0.02 {
        "within 2% of best".to_owned()
    } else {
        format!("{} above the best", format_percent(excess))
    };
    let headline_text = format!(
        "Optimal: {} {} (trigger ~{}, {}{})",
        recommendation.setting_name,
        group_thousands(u64::from(recommendation.setting_value)),
        humanize_tokens(f64::from(pick)),
        closeness,
        if recommendation.extrapolated {
            ", extrapolated"
        } else {
            ""
        },
    );
    let support = recommendation.observed_support_at_pick;
    let observed_support_text = if recommendation.extrapolated {
        format!(
            "extrapolated: {} of observed compactions at or below this level",
            format_percent(support)
        )
    } else {
        format!(
            "{} of observed compactions fired at or below this level",
            format_percent(support)
        )
    };
    let band =
        |band: &ilium_compaction_analysis::optimize::Band| (band.low_tokens, band.high_tokens);
    RecommendationCard {
        trigger_tokens: pick,
        setting_key: recommendation.setting_name.clone(),
        setting_value: recommendation.setting_value,
        setting_clamped: recommendation.setting_clamped,
        headline_text,
        extrapolated: recommendation.extrapolated,
        observed_support_text,
        observed_support_fraction: support,
        confidence: recommendation.confidence,
        basis: recommendation.basis.clone(),
        pick_rule: recommendation.pick_rule,
        pick_rule_text: pick_rule_text(recommendation),
        unsupported_argmin_tokens: recommendation.unsupported_argmin_tokens,
        unsupported_relative_saving: recommendation.unsupported_relative_saving,
        grid_floor_limited: recommendation.grid_floor_limited,
        grid_widened,
        extrapolated_alternative: extrapolated_alternative(recommendation, semantics),
        band_2_percent_tokens: band(&recommendation.bands.within_2_percent),
        band_5_percent_tokens: band(&recommendation.bands.within_5_percent),
        current_vs_recommended,
        warnings: Vec::new(),
    }
}

/// The simulated optimum that no observed compaction supports, as a
/// ready-to-apply recommendation (pick rules b and c only).
fn extrapolated_alternative(
    recommendation: &Recommendation,
    semantics: &AgentSemantics,
) -> Option<ExtrapolatedAlternative> {
    if recommendation.pick_rule == PickRule::PlainArgmin {
        return None;
    }
    let trigger = recommendation.unsupported_argmin_tokens?;
    let saving = recommendation.unsupported_relative_saving?.max(0.0);
    let mapping = semantics.trigger_to_setting(trigger);
    let support = recommendation
        .candidates
        .iter()
        .find(|row| row.trigger_tokens == trigger)
        .map_or(0.0, |row| row.observed_support);
    let support_clause = if support <= 0.0 {
        "no observed compactions at or below this level".to_owned()
    } else {
        format!(
            "only {} of observed compactions at or below this level",
            format_percent(support)
        )
    };
    Some(ExtrapolatedAlternative {
        trigger_tokens: trigger,
        setting_key: mapping.setting_name.clone(),
        setting_value: u64::from(mapping.value),
        setting_clamped: mapping.clamped,
        relative_saving: saving,
        observed_support_text: format!(
            "extrapolated: {} of observed compactions at or below this level",
            format_percent(support)
        ),
        headline_text: format!(
            "Simulated optimum: {} {} (trigger ~{}, {} cheaper than the supported pick, EXTRAPOLATED: {})",
            mapping.setting_name,
            group_thousands(u64::from(mapping.value)),
            humanize_tokens(f64::from(trigger)),
            format_percent(saving),
            support_clause
        ),
    })
}

/// The pick rule in a sentence.
fn pick_rule_text(recommendation: &Recommendation) -> String {
    match recommendation.pick_rule {
        PickRule::PlainArgmin => "plain optimum, extrapolated".to_owned(),
        PickRule::BandWithSupport => {
            "lowest candidate within 2% of best with observed support".to_owned()
        }
        PickRule::SupportedOptimum => {
            let below = match (
                recommendation.unsupported_argmin_tokens,
                recommendation.unsupported_relative_saving,
            ) {
                (Some(tokens), Some(saving)) => format!(
                    "; cost keeps falling below {} ({} cheaper in simulation) but no compactions observed there",
                    humanize_tokens(f64::from(tokens)),
                    format_percent(saving.max(0.0))
                ),
                _ => String::new(),
            };
            format!("cheapest candidate with observed support{below}")
        }
    }
}

fn report_warnings(
    agent: AgentKind,
    stats: &CorpusStats,
    scan: &ScanSummary,
    report: &CompactionReport,
) -> Vec<String> {
    let mut warnings = Vec::new();
    if scan.sessions_dropped_for_cap > 0 {
        warnings.push(format!(
            "The corpus is larger than the retained-result cap: only the most recent sessions are analysed ({} older sessions dropped).",
            group_thousands(scan.sessions_dropped_for_cap)
        ));
    }
    let unpriced: BTreeSet<&str> = stats
        .models
        .iter()
        .filter(|model| model.usd.is_none())
        .map(|model| model.model.as_str())
        .collect();
    if !unpriced.is_empty() {
        let mut names: Vec<&str> = unpriced.iter().copied().take(UNPRICED_LIST_LIMIT).collect();
        if unpriced.len() > UNPRICED_LIST_LIMIT {
            names.push("...");
        }
        warnings.push(format!(
            "No price for {}: relative research weights are used for them and dollar figures are unavailable.",
            names.join(", ")
        ));
    }
    warnings.push(match agent {
        AgentKind::ClaudeCode => "Dollar figures are API-price equivalents. Claude plan limits weight cache reads differently (unpublished), so relative consumption on a plan can differ.".to_owned(),
        AgentKind::Codex => "Dollar figures are API-price equivalents. Codex credits weight cached input at 10%, which matches the price table.".to_owned(),
    });
    if report.corpus.subagent_sessions > 0 {
        warnings.push(format!(
            "The optimum is computed over main sessions only; {} subagent sessions are reported separately.",
            group_thousands(report.corpus.subagent_sessions)
        ));
    }
    if report.semantics.mapping_note.contains("not re-measured") {
        warnings.push(
            "The mapping from trigger to setting uses the researched default. It is re-measured from the newest compactions when the current setting is known and consistent with them; compactions that ran under another setting shift it.".to_owned(),
        );
    }
    warnings
}

fn unique(warnings: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    warnings
        .into_iter()
        .filter(|warning| seen.insert(warning.clone()))
        .collect()
}

// ---------------------------------------------------------------- formatting

/// Tokens as `950`, `217k`, `217.5k`, `1.25M`.
pub fn humanize_tokens(tokens: f64) -> String {
    let value = if tokens.is_finite() {
        tokens.max(0.0)
    } else {
        0.0
    };
    if value < 1_000.0 {
        format!("{value:.0}")
    } else if value < 999_500.0 {
        format!("{}k", trim_decimals(value / 1_000.0, 1))
    } else if value < 999_500_000.0 {
        format!("{}M", trim_decimals(value / 1_000_000.0, 2))
    } else {
        format!("{}G", trim_decimals(value / 1_000_000_000.0, 2))
    }
}

fn trim_decimals(value: f64, decimals: usize) -> String {
    let text = format!("{value:.decimals$}");
    if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.').to_owned()
    } else {
        text
    }
}

/// `1234567` as `1,234,567`.
pub fn group_thousands(value: u64) -> String {
    let digits = value.to_string();
    let mut grouped = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    grouped
}

/// Dollars: `$0.42`, `$12.30`, `$1,234`.
pub fn format_usd(usd: f64) -> String {
    let usd = if usd.is_finite() { usd } else { 0.0 };
    if usd.abs() >= 1_000.0 {
        let rounded = usd.abs().round() as u64;
        format!(
            "{}${}",
            if usd < 0.0 { "-" } else { "" },
            group_thousands(rounded)
        )
    } else {
        format!("${usd:.2}")
    }
}

/// A 0..1 fraction as `12.3%`.
pub fn format_percent(fraction: f64) -> String {
    let fraction = if fraction.is_finite() { fraction } else { 0.0 };
    let percent = fraction * 100.0;
    if percent != 0.0 && percent.abs() < 0.1 {
        "<0.1%".to_owned()
    } else {
        format!("{percent:.1}%")
    }
}

/// A fraction with an explicit sign: `+3.2%`, `-0.4%`, `0.0%`.
pub fn format_signed_percent(fraction: f64) -> String {
    let fraction = if fraction.is_finite() { fraction } else { 0.0 };
    let percent = fraction * 100.0;
    if percent.abs() < 0.05 {
        "0.0%".to_owned()
    } else {
        format!("{percent:+.1}%")
    }
}

/// Bytes as `812 B`, `3.4 MB`, `35.2 GB` (decimal units).
pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1_000.0 && unit < UNITS.len() - 1 {
        value /= 1_000.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// Seconds as `12s`, `3m 05s`, `1h 02m`.
pub fn format_elapsed(seconds: f64) -> String {
    let total = seconds.max(0.0).round() as u64;
    match total {
        0..=59 => format!("{total}s"),
        60..=3_599 => format!("{}m {:02}s", total / 60, total % 60),
        _ => format!("{}h {:02}m", total / 3_600, total % 3_600 / 60),
    }
}

/// `p10 120k | p50 217k | p90 567k (n=12)` of a token distribution.
pub fn format_token_quantiles(quantiles: &Quantiles) -> String {
    format!(
        "p10 {} | p50 {} | p90 {} (n={})",
        humanize_tokens(quantiles.p10),
        humanize_tokens(quantiles.p50),
        humanize_tokens(quantiles.p90),
        group_thousands(quantiles.n as u64)
    )
}

#[cfg(test)]
mod tests;

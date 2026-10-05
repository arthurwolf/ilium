//! Corpus statistics: everything the report shows before any simulation.
//!
//! [`CorpusStats::compute`] aggregates many [`SessionTrace`]s of one agent:
//! counts, per-model usage, compaction size distributions, cycle lengths,
//! cold-cache share and its cost share, how much of the cost sits at large
//! contexts, growth per turn against context size, compaction regimes and the
//! fixed-prefix size C0.

mod quantiles;
mod regimes;

pub use quantiles::{median, percentile_sorted, Quantiles};
pub use regimes::{cluster_regimes, Regime, RegimeSet};

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::agent::AgentKind;
use crate::price::{price_of, resolve_trace_prices, PriceLookup, TokenCounts};
use crate::trace::{
    CompactionEvent, CompactionTrigger, SessionTrace, FLAG_CACHE_COLD, FLAG_FIRST_AFTER_COMPACTION,
    FLAG_GAP_COLD,
};

/// Settings of [`CorpusStats::compute`].
#[derive(Debug, Clone)]
pub struct StatsConfig {
    /// Ascending lower edges (tokens) of the growth-per-turn context bins; the
    /// last bin is open-ended.
    pub growth_bin_edges: Vec<u32>,
    /// Context thresholds (tokens) for the "cost share above" table.
    pub cost_share_thresholds: Vec<u32>,
    /// Neighbouring pre-compaction sizes further apart than this ratio start
    /// a new regime.
    pub regime_gap_ratio: f64,
    /// Growth steps above this many tokens are treated as context swaps and
    /// excluded.
    pub growth_outlier_tokens: u32,
}

impl Default for StatsConfig {
    fn default() -> Self {
        Self {
            growth_bin_edges: (0..11).map(|step| step * 50_000).collect(),
            cost_share_thresholds: vec![150_000, 200_000, 250_000, 350_000],
            regime_gap_ratio: 1.2,
            growth_outlier_tokens: 400_000,
        }
    }
}

/// Corpus size and parse health.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CorpusCounts {
    /// Main sessions.
    pub main_sessions: usize,
    /// Subagent sessions.
    pub subagent_sessions: usize,
    /// Requests in main sessions.
    pub main_turns: usize,
    /// Requests in subagent sessions.
    pub subagent_turns: usize,
    /// Compactions of any kind.
    pub compactions: usize,
    /// Compactions logged as automatic.
    pub auto_compactions: usize,
    /// Compactions logged as manual.
    pub manual_compactions: usize,
    /// Compactions with a measured post-compaction request.
    pub measured_compactions: usize,
    /// Main sessions with at least one compaction.
    pub sessions_with_compaction: usize,
    /// Earliest request time, Unix milliseconds (0 when none).
    pub first_timestamp_ms: i64,
    /// Latest request time, Unix milliseconds (0 when none).
    pub last_timestamp_ms: i64,
    /// Lines skipped for any reason across the corpus.
    pub lines_skipped: u64,
    /// Bytes fed to the parsers.
    pub bytes_fed: u64,
}

/// Usage and weighted cost of one model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelUsage {
    /// Model name.
    pub model: String,
    /// Coarse family (see [`AgentKind::model_family`]).
    pub family: String,
    /// Requests.
    pub turns: u64,
    /// Uncached input tokens.
    pub input_tokens: u64,
    /// Output tokens.
    pub output_tokens: u64,
    /// Cache-read tokens.
    pub cache_read_tokens: u64,
    /// 5-minute-tier cache-write tokens.
    pub cache_write_5m_tokens: u64,
    /// 1-hour-tier cache-write tokens.
    pub cache_write_1h_tokens: u64,
    /// Cost in input-token equivalents (relative weights).
    pub weighted_cost: f64,
    /// Cost in dollars; `None` when the price lookup did not know the model.
    pub usd: Option<f64>,
}

/// Distribution of compaction sizes.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CompactionStats {
    /// Size the trigger fired at, every compaction with a known size.
    pub pre_tokens: Option<Quantiles>,
    /// Same, automatic compactions only.
    pub pre_tokens_auto: Option<Quantiles>,
    /// Size of the first request after the compaction (the rebuilt prefix).
    pub first_post_request_tokens: Option<Quantiles>,
    /// Share of that request served from the cache (0..1).
    pub first_post_cache_read_share: Option<Quantiles>,
    /// Tokens of that request that missed the cache.
    pub first_post_fresh_tokens: Option<Quantiles>,
    /// Summary size in tokens (estimated for Claude, measured for Codex).
    pub summary_tokens: Option<Quantiles>,
    /// Time a compaction took, seconds.
    pub duration_seconds: Option<Quantiles>,
}

/// Cycle lengths between compactions.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CycleStats {
    /// Cycles analysed (segments that start after a compaction).
    pub cycles: usize,
    /// Work requests per cycle.
    pub turns: Option<Quantiles>,
    /// Weighted cost per cycle.
    pub weighted_cost: Option<Quantiles>,
    /// Wall-clock minutes per cycle.
    pub minutes: Option<Quantiles>,
    /// Cycles of at most 3 requests (refill thrash).
    pub at_most_3_turns: usize,
    /// Cycles of at most 10 requests.
    pub at_most_10_turns: usize,
    /// Cycles of at most 30 requests.
    pub at_most_30_turns: usize,
    /// Requests before the first compaction of a session.
    pub turns_to_first_compaction: Option<Quantiles>,
}

/// Cold-cache requests and what they cost.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ColdCacheStats {
    /// Requests considered (first request and post-compaction ones excluded).
    pub turns_considered: u64,
    /// Requests whose cache read fell below half of the previous context.
    pub cache_cold_turns: u64,
    /// Requests preceded by an idle gap above the agent's threshold.
    pub gap_cold_turns: u64,
    /// `cache_cold_turns / turns_considered`.
    pub cache_cold_share: f64,
    /// `gap_cold_turns / turns_considered`.
    pub gap_cold_share: f64,
    /// Weighted cost of the avoidable prefix rewrites on cold requests
    /// (cache writes beyond the normal growth) as a share of total cost.
    pub rewrite_cost_share: f64,
}

/// Cost mix by token class.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CostMix {
    /// Total weighted cost (input-token equivalents).
    pub total_weighted: f64,
    /// Total dollars over the models with known prices.
    pub total_usd: f64,
    /// Whether every model had a dollar price.
    pub usd_complete: bool,
    /// Share of weighted cost: uncached input.
    pub input_share: f64,
    /// Share of weighted cost: output.
    pub output_share: f64,
    /// Share of weighted cost: cache reads (the price of carrying context).
    pub cache_read_share: f64,
    /// Share of weighted cost: cache writes.
    pub cache_write_share: f64,
}

/// One bin of growth per request against context size.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GrowthBin {
    /// Inclusive lower edge of the previous request's context, tokens.
    pub lower_tokens: u32,
    /// Exclusive upper edge (`None` for the last bin).
    pub upper_tokens: Option<u32>,
    /// Growth `context_t - context_{t-1}` distribution.
    pub growth: Option<Quantiles>,
    /// Negative steps excluded from the distribution.
    pub negative_excluded: u64,
}

/// How much of the cost sits at large contexts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReachRow {
    /// Context threshold, tokens.
    pub threshold_tokens: u32,
    /// Sessions whose context ever exceeded it.
    pub sessions_reaching: usize,
    /// Share of total cost spent by requests above the threshold.
    pub cost_share_of_turns_above: f64,
    /// Share of total cost spent by the sessions that reached it.
    pub cost_share_of_reaching_sessions: f64,
    /// Requests above the threshold.
    pub turns_above: u64,
}

/// Everything [`CorpusStats::compute`] derives from a corpus of traces.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CorpusStats {
    /// The agent the statistics describe.
    pub agent: Option<AgentKind>,
    /// Sizes and parse health.
    pub counts: CorpusCounts,
    /// Per-model usage, largest weighted cost first.
    pub models: Vec<ModelUsage>,
    /// Compaction size distributions.
    pub compactions: CompactionStats,
    /// Cycle lengths.
    pub cycles: CycleStats,
    /// Cold-cache statistics (main sessions).
    pub cold_cache: ColdCacheStats,
    /// Cost mix (main sessions).
    pub cost_mix: CostMix,
    /// Growth per request by context bin (main sessions).
    pub growth: Vec<GrowthBin>,
    /// Cost share at large contexts (main sessions).
    pub reach: Vec<ReachRow>,
    /// Compaction regimes (automatic / unknown-trigger compactions).
    pub regimes: RegimeSet,
    /// Context of the first request of each main session (fixed prefix C0).
    pub first_request_tokens: Option<Quantiles>,
}

impl CorpusStats {
    /// Computes the statistics of the traces written by `agent`.
    pub fn compute(
        agent: AgentKind,
        traces: &[SessionTrace],
        config: &StatsConfig,
        lookup: Option<PriceLookup<'_>>,
    ) -> Self {
        let selected: Vec<&SessionTrace> =
            traces.iter().filter(|trace| trace.agent == agent).collect();
        let mut stats = CorpusStats {
            agent: Some(agent),
            ..CorpusStats::default()
        };
        stats.counts = count_corpus(&selected);
        let main: Vec<&SessionTrace> = selected
            .iter()
            .copied()
            .filter(|trace| !trace.is_subagent)
            .collect();
        stats.models = model_usage(agent, &selected, lookup);
        stats.compactions = compaction_stats(&selected);
        stats.cycles = cycle_stats(&main, lookup);
        stats.cold_cache = cold_cache_stats(&main, lookup);
        stats.cost_mix = cost_mix(&main, lookup);
        stats.growth = growth_bins(&main, config);
        stats.reach = reach_rows(&main, config, lookup);
        stats.regimes = regime_set(&selected, config);
        stats.first_request_tokens =
            Quantiles::of_u32(main.iter().filter_map(|trace| trace.first_context_tokens()));
        stats
    }
}

fn count_corpus(traces: &[&SessionTrace]) -> CorpusCounts {
    let mut counts = CorpusCounts::default();
    let mut first = i64::MAX;
    let mut last = 0_i64;
    for trace in traces {
        if trace.is_subagent {
            counts.subagent_sessions += 1;
            counts.subagent_turns += trace.turns.len();
        } else {
            counts.main_sessions += 1;
            counts.main_turns += trace.turns.len();
            if !trace.compactions.is_empty() {
                counts.sessions_with_compaction += 1;
            }
        }
        for event in &trace.compactions {
            counts.compactions += 1;
            match event.trigger {
                CompactionTrigger::Auto => counts.auto_compactions += 1,
                CompactionTrigger::Manual => counts.manual_compactions += 1,
                CompactionTrigger::Unknown => {}
            }
            if event.is_measured() {
                counts.measured_compactions += 1;
            }
        }
        for turn in trace.turns.iter().filter(|turn| turn.timestamp_ms > 0) {
            first = first.min(turn.timestamp_ms);
            last = last.max(turn.timestamp_ms);
        }
        let skipped = &trace.counters;
        counts.lines_skipped +=
            skipped.lines_skipped_oversize + skipped.lines_not_json + skipped.lines_unparseable;
        counts.bytes_fed += skipped.bytes_fed;
    }
    counts.first_timestamp_ms = if first == i64::MAX { 0 } else { first };
    counts.last_timestamp_ms = last;
    counts
}

fn model_usage(
    agent: AgentKind,
    traces: &[&SessionTrace],
    lookup: Option<PriceLookup<'_>>,
) -> Vec<ModelUsage> {
    let mut by_model: BTreeMap<String, ModelUsage> = BTreeMap::new();
    for trace in traces {
        let prices = resolve_trace_prices(trace, lookup);
        for turn in &trace.turns {
            let name = trace.model_name(turn.model).to_string();
            let price = price_of(&prices, turn.model);
            let tokens = TokenCounts::of_turn(turn);
            let usage = by_model.entry(name.clone()).or_insert_with(|| ModelUsage {
                family: agent.model_family(&name),
                model: name,
                turns: 0,
                input_tokens: 0,
                output_tokens: 0,
                cache_read_tokens: 0,
                cache_write_5m_tokens: 0,
                cache_write_1h_tokens: 0,
                weighted_cost: 0.0,
                usd: price.dollars_per_token.map(|_| 0.0),
            });
            usage.turns += 1;
            usage.input_tokens += u64::from(turn.input_tokens);
            usage.output_tokens += u64::from(turn.output_tokens);
            usage.cache_read_tokens += u64::from(turn.cache_read_tokens);
            usage.cache_write_5m_tokens += u64::from(turn.cache_write_5m_tokens);
            usage.cache_write_1h_tokens += u64::from(turn.cache_write_1h_tokens);
            usage.weighted_cost += price.relative.cost(&tokens);
            if let (Some(total), Some(dollars)) = (usage.usd.as_mut(), price.dollars_per_token) {
                *total += dollars.cost(&tokens);
            }
        }
    }
    let mut models: Vec<ModelUsage> = by_model.into_values().collect();
    models.sort_by(|left, right| right.weighted_cost.total_cmp(&left.weighted_cost));
    models
}

fn compaction_stats(traces: &[&SessionTrace]) -> CompactionStats {
    let events: Vec<&CompactionEvent> = traces
        .iter()
        .flat_map(|trace| trace.compactions.iter())
        .collect();
    let measured: Vec<&&CompactionEvent> =
        events.iter().filter(|event| event.is_measured()).collect();
    CompactionStats {
        pre_tokens: Quantiles::of_u32(
            events
                .iter()
                .map(|event| event.pre_tokens)
                .filter(|&size| size > 0),
        ),
        pre_tokens_auto: Quantiles::of_u32(
            events
                .iter()
                .filter(|event| event.trigger == CompactionTrigger::Auto)
                .map(|event| event.pre_tokens)
                .filter(|&size| size > 0),
        ),
        first_post_request_tokens: Quantiles::of_u32(
            measured.iter().map(|event| event.post_context_tokens),
        ),
        first_post_cache_read_share: {
            let mut shares: Vec<f64> = measured
                .iter()
                .filter(|event| event.post_context_tokens > 0)
                .map(|event| {
                    f64::from(event.post_cache_read_tokens) / f64::from(event.post_context_tokens)
                })
                .collect();
            Quantiles::of(&mut shares)
        },
        first_post_fresh_tokens: Quantiles::of_u32(
            measured.iter().map(|event| event.post_fresh_tokens()),
        ),
        summary_tokens: Quantiles::of_u32(
            events
                .iter()
                .map(|event| event.summary_tokens)
                .filter(|&size| size > 0),
        ),
        duration_seconds: {
            let mut seconds: Vec<f64> = events
                .iter()
                .filter(|event| event.duration_ms > 0)
                .map(|event| f64::from(event.duration_ms) / 1_000.0)
                .collect();
            Quantiles::of(&mut seconds)
        },
    }
}

fn cycle_stats(main: &[&SessionTrace], lookup: Option<PriceLookup<'_>>) -> CycleStats {
    let mut turn_counts = Vec::new();
    let mut costs = Vec::new();
    let mut minutes = Vec::new();
    let mut first_segments = Vec::new();
    for trace in main {
        let prices = resolve_trace_prices(trace, lookup);
        let mut boundaries: Vec<usize> = trace
            .measured_compactions()
            .map(|event| event.turn_index as usize)
            .filter(|&index| index < trace.turns.len())
            .collect();
        boundaries.sort_unstable();
        boundaries.dedup();
        if let Some(&first) = boundaries.first() {
            first_segments.push(first as f64);
        }
        for pair in boundaries.windows(2) {
            let segment = &trace.turns[pair[0]..pair[1]];
            let work: Vec<_> = segment
                .iter()
                .filter(|turn| !turn.is_compaction_request())
                .collect();
            if work.is_empty() {
                continue;
            }
            turn_counts.push(work.len() as f64);
            costs.push(
                work.iter()
                    .map(|turn| {
                        price_of(&prices, turn.model)
                            .relative
                            .cost(&TokenCounts::of_turn(turn))
                    })
                    .sum(),
            );
            let span_ms = work.last().map_or(0, |last| last.timestamp_ms) - work[0].timestamp_ms;
            minutes.push((span_ms.max(0) as f64) / 60_000.0);
        }
    }
    CycleStats {
        cycles: turn_counts.len(),
        at_most_3_turns: turn_counts.iter().filter(|&&count| count <= 3.0).count(),
        at_most_10_turns: turn_counts.iter().filter(|&&count| count <= 10.0).count(),
        at_most_30_turns: turn_counts.iter().filter(|&&count| count <= 30.0).count(),
        turns: Quantiles::of(&mut turn_counts),
        weighted_cost: Quantiles::of(&mut costs),
        minutes: Quantiles::of(&mut minutes),
        turns_to_first_compaction: Quantiles::of(&mut first_segments),
    }
}

fn cold_cache_stats(main: &[&SessionTrace], lookup: Option<PriceLookup<'_>>) -> ColdCacheStats {
    let mut stats = ColdCacheStats::default();
    let mut rewrite_cost = 0.0;
    let mut total_cost = 0.0;
    for trace in main {
        let prices = resolve_trace_prices(trace, lookup);
        let write_5m_share = trace.cache_write_5m_share();
        let mut previous_context: Option<u32> = None;
        for turn in &trace.turns {
            let price = price_of(&prices, turn.model);
            total_cost += price.relative.cost(&TokenCounts::of_turn(turn));
            let considered = previous_context.is_some()
                && !turn.has_flag(FLAG_FIRST_AFTER_COMPACTION)
                && !turn.is_compaction_request();
            if considered {
                stats.turns_considered += 1;
                if turn.has_flag(FLAG_GAP_COLD) {
                    stats.gap_cold_turns += 1;
                }
                if turn.has_flag(FLAG_CACHE_COLD) {
                    stats.cache_cold_turns += 1;
                    let growth = turn
                        .context_tokens
                        .saturating_sub(previous_context.unwrap_or(0));
                    let excess_written = turn.cache_write_tokens().saturating_sub(growth);
                    rewrite_cost +=
                        price.relative.blended_write(write_5m_share) * f64::from(excess_written);
                }
            }
            if !turn.is_compaction_request() {
                previous_context = Some(turn.context_tokens);
            }
        }
    }
    let denominator = stats.turns_considered.max(1) as f64;
    stats.cache_cold_share = stats.cache_cold_turns as f64 / denominator;
    stats.gap_cold_share = stats.gap_cold_turns as f64 / denominator;
    stats.rewrite_cost_share = if total_cost > 0.0 {
        rewrite_cost / total_cost
    } else {
        0.0
    };
    stats
}

fn cost_mix(main: &[&SessionTrace], lookup: Option<PriceLookup<'_>>) -> CostMix {
    let mut mix = CostMix {
        usd_complete: true,
        ..CostMix::default()
    };
    let (mut input, mut output, mut read, mut write) = (0.0, 0.0, 0.0, 0.0);
    for trace in main {
        let prices = resolve_trace_prices(trace, lookup);
        for turn in &trace.turns {
            let price = price_of(&prices, turn.model);
            let tokens = TokenCounts::of_turn(turn);
            input += price.relative.input * tokens.input;
            output += price.relative.output * tokens.output;
            read += price.relative.cache_read * tokens.cache_read;
            write += price.relative.cache_write_5m * tokens.cache_write_5m
                + price.relative.cache_write_1h * tokens.cache_write_1h;
            match price.dollars_per_token {
                Some(dollars) => mix.total_usd += dollars.cost(&tokens),
                None => mix.usd_complete = false,
            }
        }
    }
    let total = input + output + read + write;
    mix.total_weighted = total;
    if total > 0.0 {
        mix.input_share = input / total;
        mix.output_share = output / total;
        mix.cache_read_share = read / total;
        mix.cache_write_share = write / total;
    }
    mix
}

fn growth_bins(main: &[&SessionTrace], config: &StatsConfig) -> Vec<GrowthBin> {
    let edges = &config.growth_bin_edges;
    let mut values: Vec<Vec<f64>> = vec![Vec::new(); edges.len()];
    let mut negatives = vec![0_u64; edges.len()];
    for trace in main {
        let mut previous: Option<&crate::trace::TurnSample> = None;
        for turn in &trace.turns {
            if turn.is_compaction_request() {
                continue;
            }
            if let Some(before) = previous {
                let skip = turn.has_flag(FLAG_FIRST_AFTER_COMPACTION)
                    || (turn.timestamp_ms > 0 && turn.timestamp_ms < before.timestamp_ms);
                if !skip {
                    let growth = i64::from(turn.context_tokens) - i64::from(before.context_tokens);
                    let bin = edges
                        .iter()
                        .rposition(|&edge| before.context_tokens >= edge)
                        .unwrap_or(0);
                    if growth < 0 {
                        negatives[bin] += 1;
                    } else if growth <= i64::from(config.growth_outlier_tokens) {
                        values[bin].push(growth as f64);
                    }
                }
            }
            previous = Some(turn);
        }
    }
    edges
        .iter()
        .enumerate()
        .map(|(index, &lower)| GrowthBin {
            lower_tokens: lower,
            upper_tokens: edges.get(index + 1).copied(),
            growth: Quantiles::of(&mut values[index]),
            negative_excluded: negatives[index],
        })
        .collect()
}

fn reach_rows(
    main: &[&SessionTrace],
    config: &StatsConfig,
    lookup: Option<PriceLookup<'_>>,
) -> Vec<ReachRow> {
    let mut total = 0.0;
    let mut rows: Vec<(u32, usize, f64, f64, u64)> = config
        .cost_share_thresholds
        .iter()
        .map(|&threshold| (threshold, 0, 0.0, 0.0, 0))
        .collect();
    for trace in main {
        let prices = resolve_trace_prices(trace, lookup);
        let max_context = trace.max_context_tokens();
        let session_cost: f64 = trace
            .turns
            .iter()
            .map(|turn| {
                price_of(&prices, turn.model)
                    .relative
                    .cost(&TokenCounts::of_turn(turn))
            })
            .sum();
        total += session_cost;
        for row in &mut rows {
            if max_context > row.0 {
                row.1 += 1;
                row.3 += session_cost;
            }
            for turn in trace
                .turns
                .iter()
                .filter(|turn| turn.context_tokens > row.0)
            {
                row.2 += price_of(&prices, turn.model)
                    .relative
                    .cost(&TokenCounts::of_turn(turn));
                row.4 += 1;
            }
        }
    }
    rows.into_iter()
        .map(
            |(threshold, sessions, above, reaching, turns_above)| ReachRow {
                threshold_tokens: threshold,
                sessions_reaching: sessions,
                cost_share_of_turns_above: if total > 0.0 { above / total } else { 0.0 },
                cost_share_of_reaching_sessions: if total > 0.0 { reaching / total } else { 0.0 },
                turns_above,
            },
        )
        .collect()
}

fn regime_set(traces: &[&SessionTrace], config: &StatsConfig) -> RegimeSet {
    let points: Vec<(u32, i64)> = traces
        .iter()
        .flat_map(|trace| trace.compactions.iter())
        .filter(|event| event.trigger != CompactionTrigger::Manual)
        .map(|event| (event.pre_tokens, event.timestamp_ms))
        .collect();
    cluster_regimes(&points, config.regime_gap_ratio)
}

/// The measured compactions that belong to the most recent regime, falling
/// back to every measured non-manual compaction when no regime exists.
pub fn most_recent_regime_events<'a>(
    traces: &'a [SessionTrace],
    agent: AgentKind,
    config: &StatsConfig,
) -> Vec<&'a CompactionEvent> {
    let selected: Vec<&SessionTrace> = traces.iter().filter(|trace| trace.agent == agent).collect();
    let regimes = regime_set(&selected, config);
    let bounds = regimes
        .most_recent()
        .map(|regime| (regime.min_tokens, regime.max_tokens));
    selected
        .iter()
        .flat_map(|trace| trace.compactions.iter())
        .filter(|event| event.is_measured() && event.trigger != CompactionTrigger::Manual)
        .filter(|event| bounds.is_none_or(|(low, high)| (low..=high).contains(&event.pre_tokens)))
        .collect()
}

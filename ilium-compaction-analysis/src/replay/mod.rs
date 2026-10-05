//! Trace-driven simulator of compaction triggers.
//!
//! For every main session whose context ever exceeded the agent's replay
//! threshold (Claude 120k, Codex 100k) the measured per-request *work* (see
//! `rows`) is replayed under each candidate effective trigger. When the
//! simulated context reaches the trigger (and at least 5 requests passed since
//! the previous compaction) a compaction is applied:
//!
//! * the compaction request over the full context (cache read at the read rate
//!   while warm, otherwise the full input rate),
//! * the summary output at the output rate,
//! * the cache rebuild of the post-compaction prefix, drawn **jointly** (size
//!   and cache split together) from the measured compactions of the most
//!   recent regime with a deterministic seeded generator (common random
//!   numbers across triggers and sessions, so differences between triggers are
//!   not Monte-Carlo noise),
//! * the rework penalty ([`ReworkModel`]), added after the simulation so the
//!   0.5x / 1x / 2x sensitivity needs no re-run.
//!
//! The result is a [`SimTable`] of per-session, per-trigger outcomes that
//! [`crate::optimize`] consumes.
//!
//! Assumptions that hold only inside the observed range and are therefore the
//! source of the `extrapolated` label: growth per request does not depend on
//! the trigger, the post-compaction prefix and the rework do not depend on the
//! trigger, and summary quality is unaffected by more frequent summaries.

mod engine;
mod rework;
mod rows;

pub use engine::PostDraw;
pub use rework::{ReworkAccounting, ReworkModel, ReworkSource};
pub use rows::{MissModel, TriggerRule};

use serde::{Deserialize, Serialize};

use crate::agent::AgentKind;
use crate::error::{AnalysisError, AnalysisResult};
use crate::price::{resolve_trace_prices, PriceLookup, TokenCounts};
use crate::rng::SeededRng;
use crate::stats::{most_recent_regime_events, StatsConfig};
use crate::trace::{CompactionEvent, SessionTrace};
use engine::{run_session, EngineSession, EngineWeights};
use rows::build_rows;

/// Agent-specific replay rules (the research's choices).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplayProfile {
    /// When the trigger fires.
    pub trigger_rule: TriggerRule,
    /// How cache misses are modelled.
    pub miss_model: MissModel,
    /// How the rework penalty is charged.
    pub rework_accounting: ReworkAccounting,
}

impl ReplayProfile {
    /// The profile of an agent.
    pub fn for_agent(agent: AgentKind) -> Self {
        match agent {
            AgentKind::ClaudeCode => Self {
                trigger_rule: TriggerRule::NextContext,
                miss_model: MissModel::Binary,
                rework_accounting: ReworkAccounting::Absolute,
            },
            AgentKind::Codex => Self {
                trigger_rule: TriggerRule::LastResponseTotal,
                miss_model: MissModel::Fraction,
                rework_accounting: ReworkAccounting::Differential,
            },
        }
    }
}

/// Settings of [`simulate`].
#[derive(Debug, Clone)]
pub struct SimConfig {
    /// Candidate effective triggers in tokens, ascending after
    /// [`SimConfig::normalize`].
    pub triggers: Vec<u32>,
    /// Sessions whose context never exceeded this are not replayed.
    pub min_context_tokens: u32,
    /// Guard: minimum requests between two compactions.
    pub min_turns_between_compactions: u32,
    /// Number of Monte-Carlo draw lists averaged per trigger.
    pub draw_lists: usize,
    /// Draws per list (a session wraps around when it compacts more often).
    pub draws_per_list: usize,
    /// Seed of the draw generator.
    pub seed: u64,
    /// Replay subagent sessions too (default: main sessions only).
    pub include_subagents: bool,
    /// Minimum measured compactions in the most recent regime to draw from it
    /// alone; below that every measured non-manual compaction is used.
    pub min_regime_draw_events: usize,
    /// Claude: requests after an observed compaction replaced by a mirror of
    /// the work before it.
    pub mirror_turns: usize,
    /// Rework model used when reporting totals (sensitivity is applied later).
    pub rework: ReworkModel,
    /// Settings of the regime clustering.
    pub stats: StatsConfig,
}

impl SimConfig {
    /// The research defaults of an agent: its trigger grid, 5-request guard,
    /// 40 draw lists of 60 draws, seed 12345.
    pub fn for_agent(agent: AgentKind) -> Self {
        let profile = agent.profile();
        Self {
            triggers: profile.default_trigger_grid.to_vec(),
            min_context_tokens: profile.replay_min_context_tokens,
            min_turns_between_compactions: 5,
            draw_lists: 40,
            draws_per_list: 60,
            seed: 12_345,
            include_subagents: false,
            min_regime_draw_events: 10,
            mirror_turns: 30,
            rework: ReworkModel::prior_for(agent),
            stats: StatsConfig::default(),
        }
    }

    /// The research defaults of an agent with the rework **measured on the
    /// corpus** when at least 30 compactions back it (else the prior).
    pub fn for_corpus(
        agent: AgentKind,
        traces: &[SessionTrace],
        lookup: Option<PriceLookup<'_>>,
    ) -> Self {
        let measured = crate::rework::measure_with(traces, agent, lookup, &Default::default());
        Self::for_agent(agent).with_rework(ReworkModel::from_measurement(agent, measured.as_ref()))
    }

    /// Replaces the rework model.
    pub fn with_rework(mut self, rework: ReworkModel) -> Self {
        self.rework = rework;
        self
    }

    /// Adds triggers (for example the configured and the default ones) to the
    /// grid, sorting and deduplicating it.
    pub fn with_extra_triggers(mut self, extra: &[u32]) -> Self {
        self.triggers.extend_from_slice(extra);
        self.normalize();
        self
    }

    /// Sorts and deduplicates the trigger grid.
    pub fn normalize(&mut self) {
        self.triggers.sort_unstable();
        self.triggers.dedup();
    }

    fn validate(&self) -> AnalysisResult<()> {
        if self.triggers.is_empty() {
            return Err(AnalysisError::InvalidConfig(
                "the trigger grid is empty".into(),
            ));
        }
        if self.draw_lists == 0 || self.draws_per_list == 0 {
            return Err(AnalysisError::InvalidConfig(
                "draw_lists and draws_per_list must be positive".into(),
            ));
        }
        Ok(())
    }
}

/// Where the post-compaction draws came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DrawSource {
    /// Measured compactions of the most recent regime.
    RecentRegime,
    /// Every measured non-manual compaction (the regime was too small).
    AllMeasured,
    /// Research defaults: the corpus holds no measured compaction.
    ResearchDefault,
}

/// Summary of the draw pool.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DrawPoolInfo {
    /// Where the draws came from.
    pub source: DrawSource,
    /// Number of distinct draws.
    pub events: usize,
}

/// Outcome of one session at one trigger (mean over the draw lists).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TriggerOutcome {
    /// Weighted cost (input-token equivalents), rework excluded.
    pub cost: f64,
    /// Dollars, rework excluded; `None` when a model had no price.
    pub usd: Option<f64>,
    /// Mean number of compactions.
    pub compactions: f64,
}

/// One replayed session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionSim {
    /// Index of the session in the slice given to [`simulate`].
    pub trace_index: usize,
    /// Family of the dominant model.
    pub model_family: String,
    /// Dominant model name.
    pub dominant_model: String,
    /// Requests of the session.
    pub turns: u32,
    /// Largest context of the session.
    pub max_context_tokens: u32,
    /// Negative growth steps (context shrinking between work requests,
    /// clamped to zero in the replay).
    pub negative_growth_steps: u32,
    /// Measured weighted cost of the whole session.
    pub measured_cost: f64,
    /// Measured dollars; `None` when a model had no price.
    pub measured_usd: Option<f64>,
    /// Compactions observed in the session (their work was removed).
    pub observed_compactions: u32,
    /// Replay cost with compaction disabled.
    pub no_compaction_cost: f64,
    /// Dollars per weighted token for the rework penalty (the dominant
    /// model's input price); `None` when unknown.
    pub rework_usd_per_weighted_token: Option<f64>,
    /// One outcome per trigger of [`SimTable::triggers`].
    pub outcomes: Vec<TriggerOutcome>,
}

/// The simulation result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SimTable {
    /// The simulated agent.
    pub agent: AgentKind,
    /// Candidate triggers, ascending.
    pub triggers: Vec<u32>,
    /// How the rework penalty is charged.
    pub rework_accounting: ReworkAccounting,
    /// Rework model of the configuration.
    pub rework: ReworkModel,
    /// Draw pool used.
    pub draw_pool: DrawPoolInfo,
    /// Replayed sessions.
    pub sessions: Vec<SessionSim>,
    /// Sessions skipped (too small, no usable requests, or subagent).
    pub sessions_skipped: usize,
    /// Human-readable caveats.
    pub warnings: Vec<String>,
}

impl SimTable {
    /// Rework cost of `session` at trigger index `trigger_index` in weighted
    /// tokens.
    pub fn rework_cost(
        &self,
        session: &SessionSim,
        trigger_index: usize,
        rework: &ReworkModel,
    ) -> f64 {
        let simulated = session.outcomes[trigger_index].compactions;
        let chargeable = match self.rework_accounting {
            ReworkAccounting::Absolute => simulated,
            ReworkAccounting::Differential => simulated - f64::from(session.observed_compactions),
        };
        rework.effective_tokens() * chargeable
    }

    /// Cost of one session at one trigger, rework included.
    pub fn session_cost(
        &self,
        session: &SessionSim,
        trigger_index: usize,
        rework: &ReworkModel,
    ) -> f64 {
        session.outcomes[trigger_index].cost + self.rework_cost(session, trigger_index, rework)
    }

    /// Dollar cost of one session at one trigger, rework included, when known.
    pub fn session_usd(
        &self,
        session: &SessionSim,
        trigger_index: usize,
        rework: &ReworkModel,
    ) -> Option<f64> {
        let base = session.outcomes[trigger_index].usd?;
        let per_token = session.rework_usd_per_weighted_token?;
        Some(base + per_token * self.rework_cost(session, trigger_index, rework))
    }

    /// Total weighted cost per trigger over the sessions accepted by `filter`.
    pub fn total_costs(
        &self,
        rework: &ReworkModel,
        filter: &dyn Fn(&SessionSim) -> bool,
    ) -> Vec<f64> {
        (0..self.triggers.len())
            .map(|index| {
                self.sessions
                    .iter()
                    .filter(|session| filter(session))
                    .map(|session| self.session_cost(session, index, rework))
                    .sum()
            })
            .collect()
    }

    /// Total dollars per trigger, `None` entries when any accepted session
    /// lacks a price.
    pub fn total_usd(
        &self,
        rework: &ReworkModel,
        filter: &dyn Fn(&SessionSim) -> bool,
    ) -> Vec<Option<f64>> {
        (0..self.triggers.len())
            .map(|index| {
                self.sessions
                    .iter()
                    .filter(|session| filter(session))
                    .try_fold(0.0, |total, session| {
                        self.session_usd(session, index, rework)
                            .map(|usd| total + usd)
                    })
            })
            .collect()
    }

    /// Mean compactions per trigger over the accepted sessions.
    pub fn total_compactions(&self, filter: &dyn Fn(&SessionSim) -> bool) -> Vec<f64> {
        (0..self.triggers.len())
            .map(|index| {
                self.sessions
                    .iter()
                    .filter(|session| filter(session))
                    .map(|session| session.outcomes[index].compactions)
                    .sum()
            })
            .collect()
    }

    /// Per-session median of the cost at each trigger.
    pub fn median_session_costs(&self, rework: &ReworkModel) -> Vec<f64> {
        (0..self.triggers.len())
            .map(|index| {
                let costs: Vec<f64> = self
                    .sessions
                    .iter()
                    .map(|session| self.session_cost(session, index, rework))
                    .collect();
                crate::stats::median(&costs)
            })
            .collect()
    }

    /// One summary row per trigger: totals, compactions, per-model split and
    /// the median session.
    pub fn summary(&self, rework: &ReworkModel) -> Vec<TriggerSummary> {
        let totals = self.total_costs(rework, &|_| true);
        let usd = self.total_usd(rework, &|_| true);
        let compactions = self.total_compactions(&|_| true);
        let medians = self.median_session_costs(rework);
        let mut families: Vec<&str> = self
            .sessions
            .iter()
            .map(|session| session.model_family.as_str())
            .collect();
        families.sort_unstable();
        families.dedup();
        let per_family: Vec<(String, Vec<f64>)> = families
            .iter()
            .map(|family| {
                let costs = self.total_costs(rework, &|session| session.model_family == *family);
                (family.to_string(), costs)
            })
            .collect();
        (0..self.triggers.len())
            .map(|index| TriggerSummary {
                trigger_tokens: self.triggers[index],
                weighted_cost: totals[index],
                usd: usd[index],
                compactions: compactions[index],
                median_session_cost: medians[index],
                per_model_cost: per_family
                    .iter()
                    .map(|(family, costs)| (family.clone(), costs[index]))
                    .collect(),
            })
            .collect()
    }

    /// Index of an exactly simulated trigger.
    pub fn trigger_index(&self, trigger_tokens: u32) -> Option<usize> {
        self.triggers.binary_search(&trigger_tokens).ok()
    }

    /// Like [`SimTable::trigger_index`], but an error for a trigger that is not
    /// on the grid (add it with [`SimConfig::with_extra_triggers`] and re-run).
    pub fn require_trigger_index(&self, trigger_tokens: u32) -> AnalysisResult<usize> {
        self.trigger_index(trigger_tokens)
            .ok_or(AnalysisError::TriggerNotSimulated(trigger_tokens))
    }

    /// Total measured weighted cost of the replayed sessions.
    pub fn measured_total(&self) -> f64 {
        self.sessions
            .iter()
            .map(|session| session.measured_cost)
            .sum()
    }
}

/// One row of [`SimTable::summary`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TriggerSummary {
    /// Candidate trigger, tokens.
    pub trigger_tokens: u32,
    /// Total weighted cost over the replayed sessions, rework included.
    pub weighted_cost: f64,
    /// Total dollars, rework included; `None` when a price is unknown.
    pub usd: Option<f64>,
    /// Mean simulated compactions summed over the sessions.
    pub compactions: f64,
    /// Median per-session weighted cost.
    pub median_session_cost: f64,
    /// Total weighted cost per model family.
    pub per_model_cost: Vec<(String, f64)>,
}

/// Builds the post-compaction draw pool.
fn build_draw_pool(
    traces: &[SessionTrace],
    agent: AgentKind,
    config: &SimConfig,
    warnings: &mut Vec<String>,
) -> (Vec<PostDraw>, DrawPoolInfo) {
    let to_draw = |event: &&CompactionEvent| PostDraw {
        context_tokens: f64::from(event.post_context_tokens),
        input_tokens: f64::from(event.post_input_tokens),
        cache_read_tokens: f64::from(event.post_cache_read_tokens),
        cache_write_5m_tokens: f64::from(event.post_cache_write_5m_tokens),
        cache_write_1h_tokens: f64::from(event.post_cache_write_1h_tokens),
        summary_tokens: f64::from(event.summary_tokens),
    };
    let recent = most_recent_regime_events(traces, agent, &config.stats);
    if recent.len() >= config.min_regime_draw_events {
        let pool: Vec<PostDraw> = recent.iter().map(to_draw).collect();
        let info = DrawPoolInfo {
            source: DrawSource::RecentRegime,
            events: pool.len(),
        };
        return (pool, info);
    }
    let all: Vec<&CompactionEvent> = traces
        .iter()
        .filter(|trace| trace.agent == agent)
        .flat_map(|trace| trace.compactions.iter())
        .filter(|event| {
            event.is_measured() && event.trigger != crate::trace::CompactionTrigger::Manual
        })
        .collect();
    if !all.is_empty() {
        warnings.push(format!(
            "the most recent compaction regime holds only {} measured compactions; post-compaction draws use all {} measured non-manual compactions",
            recent.len(),
            all.len()
        ));
        let pool: Vec<PostDraw> = all.iter().map(to_draw).collect();
        let info = DrawPoolInfo {
            source: DrawSource::AllMeasured,
            events: pool.len(),
        };
        return (pool, info);
    }
    warnings.push(
        "no measured compaction exists in the corpus; post-compaction draws use the research defaults of the agent"
            .to_string(),
    );
    let pool = vec![research_default_draw(agent)];
    let info = DrawPoolInfo {
        source: DrawSource::ResearchDefault,
        events: 1,
    };
    (pool, info)
}

/// Medians of the research corpora: Claude (cache read 28.6k, 1-hour write
/// 75.4k, summary 4.7k); Codex (first request 51.2k of which 12.8k cached,
/// summary 4k).
fn research_default_draw(agent: AgentKind) -> PostDraw {
    match agent {
        AgentKind::ClaudeCode => PostDraw {
            context_tokens: 104_000.0,
            input_tokens: 3.0,
            cache_read_tokens: 28_600.0,
            cache_write_5m_tokens: 0.0,
            cache_write_1h_tokens: 75_400.0,
            summary_tokens: 4_700.0,
        },
        AgentKind::Codex => PostDraw {
            context_tokens: 51_200.0,
            input_tokens: 38_400.0,
            cache_read_tokens: 12_800.0,
            cache_write_5m_tokens: 0.0,
            cache_write_1h_tokens: 0.0,
            summary_tokens: 4_000.0,
        },
    }
}

/// Replays the traces of `agent` under every trigger of the grid.
///
/// `lookup` supplies dollar prices per model; without it (or for unknown
/// models) only the unitless weighted cost is produced.
pub fn simulate(
    agent: AgentKind,
    traces: &[SessionTrace],
    config: &SimConfig,
    lookup: Option<PriceLookup<'_>>,
) -> AnalysisResult<SimTable> {
    let mut config = config.clone();
    config.normalize();
    config.validate()?;
    let profile = ReplayProfile::for_agent(agent);
    let mut warnings = Vec::new();
    let (draw_pool, draw_info) = build_draw_pool(traces, agent, &config, &mut warnings);
    let mut rng = SeededRng::new(config.seed);
    // Common random numbers: the same draw lists for every trigger and session.
    let draw_lists: Vec<Vec<u32>> = (0..config.draw_lists)
        .map(|_| {
            (0..config.draws_per_list)
                .map(|_| rng.next_below(draw_pool.len().max(1)) as u32)
                .collect()
        })
        .collect();
    let context = ReplayContext {
        config: &config,
        profile: &profile,
        lookup,
        draw_pool: &draw_pool,
        draw_lists: &draw_lists,
    };
    let mut sessions = Vec::new();
    let mut skipped = 0;
    for (trace_index, trace) in traces.iter().enumerate() {
        if trace.agent != agent || (trace.is_subagent && !config.include_subagents) {
            skipped += 1;
            continue;
        }
        if trace.max_context_tokens() <= config.min_context_tokens {
            skipped += 1;
            continue;
        }
        let Some(session_rows) = build_rows(trace, config.mirror_turns) else {
            skipped += 1;
            continue;
        };
        let simulated = replay_session(&context, trace_index, trace, &session_rows);
        sessions.push(simulated);
    }
    if sessions.is_empty() {
        return Err(AnalysisError::NoReplayableSession);
    }
    if draw_info.source != DrawSource::RecentRegime {
        warnings.push(
            "post-compaction rebuild costs are not drawn from the most recent regime".to_string(),
        );
    }
    Ok(SimTable {
        agent,
        triggers: config.triggers.clone(),
        rework_accounting: profile.rework_accounting,
        rework: config.rework,
        draw_pool: draw_info,
        sessions,
        sessions_skipped: skipped,
        warnings,
    })
}

/// Everything shared by the replays of all sessions.
struct ReplayContext<'a> {
    config: &'a SimConfig,
    profile: &'a ReplayProfile,
    lookup: Option<PriceLookup<'a>>,
    draw_pool: &'a [PostDraw],
    draw_lists: &'a [Vec<u32>],
}

fn replay_session(
    context: &ReplayContext<'_>,
    trace_index: usize,
    trace: &SessionTrace,
    session_rows: &rows::SessionRows,
) -> SessionSim {
    let ReplayContext {
        config,
        profile,
        lookup,
        draw_pool,
        draw_lists,
    } = context;
    let prices = resolve_trace_prices(trace, *lookup);
    let dollars_known = prices.iter().all(|price| price.dollars_per_token.is_some());
    let weights: Vec<EngineWeights> = prices
        .iter()
        .map(|price| EngineWeights {
            relative: price.relative,
            dollars: price.dollars_per_token,
        })
        .collect();
    let engine_session = EngineSession {
        rows: &session_rows.rows,
        first_context: f64::from(session_rows.first_context.max(20_000)),
        first_output: f64::from(session_rows.first_turn.output_tokens),
        weights: &weights,
        write_5m_share: trace.cache_write_5m_share(),
        trigger_rule: profile.trigger_rule,
        miss_model: profile.miss_model,
        min_turns_between: config.min_turns_between_compactions,
    };
    let first_tokens = TokenCounts::of_turn(&session_rows.first_turn);
    let first_model = session_rows.first_turn.model;
    let run = |trigger: u32, draw_list: &[u32]| {
        run_session(
            &engine_session,
            trigger,
            &first_tokens,
            first_model,
            draw_pool,
            draw_list,
        )
    };
    let no_compaction = run(u32::MAX, &draw_lists[0]);
    let outcomes: Vec<TriggerOutcome> = config
        .triggers
        .iter()
        .map(|&trigger| {
            let first = run(trigger, &draw_lists[0]);
            if first.compactions == 0 {
                // Nothing fired: the result does not depend on the draws.
                return outcome_of(&[first], dollars_known);
            }
            let mut runs = vec![first];
            runs.extend(draw_lists.iter().skip(1).map(|list| run(trigger, list)));
            outcome_of(&runs, dollars_known)
        })
        .collect();
    let (measured_cost, measured_usd) = measured_totals(trace, &prices);
    let dominant = trace.dominant_model().unwrap_or(0);
    let dominant_name = trace.model_name(dominant).to_string();
    SessionSim {
        trace_index,
        model_family: trace.agent.model_family(&dominant_name),
        dominant_model: dominant_name,
        turns: u32::try_from(trace.turns.len()).unwrap_or(u32::MAX),
        max_context_tokens: trace.max_context_tokens(),
        negative_growth_steps: session_rows.negative_growth_steps,
        measured_cost,
        measured_usd: if dollars_known {
            Some(measured_usd)
        } else {
            None
        },
        observed_compactions: session_rows.observed_compactions,
        no_compaction_cost: no_compaction.weighted,
        rework_usd_per_weighted_token: crate::price::price_of(&prices, dominant)
            .dollars_per_token
            .map(|dollars| dollars.input),
        outcomes,
    }
}

fn outcome_of(runs: &[engine::RunTotals], dollars_known: bool) -> TriggerOutcome {
    let count = runs.len() as f64;
    TriggerOutcome {
        cost: runs.iter().map(|run| run.weighted).sum::<f64>() / count,
        usd: dollars_known.then(|| runs.iter().map(|run| run.dollars).sum::<f64>() / count),
        compactions: runs
            .iter()
            .map(|run| f64::from(run.compactions))
            .sum::<f64>()
            / count,
    }
}

fn measured_totals(trace: &SessionTrace, prices: &[crate::price::ResolvedPrice]) -> (f64, f64) {
    let mut weighted = 0.0;
    let mut dollars = 0.0;
    for turn in &trace.turns {
        let price = crate::price::price_of(prices, turn.model);
        let tokens = TokenCounts::of_turn(turn);
        weighted += price.relative.cost(&tokens);
        if let Some(per_token) = price.dollars_per_token {
            dollars += per_token.cost(&tokens);
        }
    }
    (weighted, dollars)
}

//! From a [`SimTable`] to a recommendation.
//!
//! The output is a **range with a pick**, not a false-precision point:
//!
//! * `argmin` of the total cost over the sessions,
//! * the flat bands (every trigger within 2 % and within 5 % of the best),
//! * a bootstrap over sessions (seeded, at least 200 resamples) for the argmin
//!   distribution and its 95 % interval,
//! * per-model optima (the setting itself is global; the report shows the
//!   spread),
//! * the rework sensitivity (0.5x / 1x / 2x),
//! * `observed_support`: the fraction of observed compactions at or below each
//!   candidate.
//!
//! **Pick rule**: the lowest trigger inside the 2 % band that has observed
//! support (at least `min_support_fraction` of the observed compactions fired
//! at or below it). If no band member has support, the plain argmin is picked
//! and flagged `extrapolated`, with the support at that level spelled out
//! (typically "0% of observed compactions at or below this level").

use serde::{Deserialize, Serialize};

use crate::agent::AgentKind;
use crate::error::{AnalysisError, AnalysisResult};
use crate::replay::{ReworkModel, ReworkSource, SessionSim, SimTable};
use crate::rng::SeededRng;
use crate::semantics::AgentSemantics;
use crate::trace::{CompactionTrigger, SessionTrace};

/// Minimum number of bootstrap resamples.
pub const MIN_BOOTSTRAP_RESAMPLES: usize = 200;

/// Settings of [`optimize`].
#[derive(Debug, Clone)]
pub struct OptimizeConfig {
    /// Rework model of the central estimate.
    pub rework: ReworkModel,
    /// Multipliers of the rework sensitivity table.
    pub sensitivity_multipliers: Vec<f64>,
    /// Bootstrap resamples (raised to [`MIN_BOOTSTRAP_RESAMPLES`] when lower).
    pub bootstrap_resamples: usize,
    /// Seed of the bootstrap.
    pub seed: u64,
    /// Flat-band tolerances (fractions of the best cost): 2 % and 5 %.
    pub band_fractions: (f64, f64),
    /// Fraction of observed compactions at or below a trigger needed for it to
    /// count as having observed support.
    pub min_support_fraction: f64,
    /// Smallest number of sessions for a per-model optimum.
    pub min_sessions_per_model: usize,
}

impl OptimizeConfig {
    /// Defaults for an agent with the rework **measured on the corpus** when at
    /// least 30 compactions back it (else the prior). Use the same
    /// measurement for [`SimConfig::for_corpus`](crate::replay::SimConfig::for_corpus).
    pub fn for_corpus(
        agent: AgentKind,
        traces: &[SessionTrace],
        lookup: Option<crate::price::PriceLookup<'_>>,
    ) -> Self {
        let measured = crate::rework::measure_with(traces, agent, lookup, &Default::default());
        Self::for_agent(agent).with_rework(ReworkModel::from_measurement(agent, measured.as_ref()))
    }

    /// Replaces the rework model (the sensitivity rows stay at 0.5x/1x/2x of
    /// it).
    pub fn with_rework(mut self, rework: ReworkModel) -> Self {
        self.rework = rework;
        self
    }

    /// Defaults for an agent: its rework prior at 1x, 0.5x/1x/2x sensitivity,
    /// 400 resamples, 2 % / 5 % bands, 1 % support threshold.
    pub fn for_agent(agent: AgentKind) -> Self {
        Self {
            rework: ReworkModel::prior_for(agent),
            sensitivity_multipliers: vec![0.5, 1.0, 2.0],
            bootstrap_resamples: 400,
            seed: 7,
            band_fractions: (0.02, 0.05),
            min_support_fraction: 0.01,
            min_sessions_per_model: 5,
        }
    }
}

/// Sizes at which compactions were actually observed.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ObservedCompactions {
    sizes: Vec<u32>,
}

impl ObservedCompactions {
    /// The non-manual compactions of `agent`'s traces, **subagent sessions
    /// included**. The optimum is computed over main sessions only, so prefer
    /// [`ObservedCompactions::main_sessions_only`] for the support rule.
    pub fn from_traces(traces: &[SessionTrace], agent: AgentKind) -> Self {
        Self::from_sizes(
            traces
                .iter()
                .filter(|trace| trace.agent == agent)
                .flat_map(|trace| trace.compactions.iter())
                .filter(|event| event.trigger != CompactionTrigger::Manual)
                .map(|event| event.pre_tokens),
        )
    }

    /// The non-manual compactions of `agent`'s **main** (non-subagent)
    /// sessions: the population the replayed optimum covers, hence the right
    /// basis for `observed_support`.
    pub fn main_sessions_only(traces: &[SessionTrace], agent: AgentKind) -> Self {
        Self::from_sizes(
            traces
                .iter()
                .filter(|trace| trace.agent == agent && !trace.is_subagent)
                .flat_map(|trace| trace.compactions.iter())
                .filter(|event| event.trigger != CompactionTrigger::Manual)
                .map(|event| event.pre_tokens),
        )
    }

    /// From raw sizes (zeros are ignored).
    pub fn from_sizes(sizes: impl IntoIterator<Item = u32>) -> Self {
        let mut sizes: Vec<u32> = sizes.into_iter().filter(|&size| size > 0).collect();
        sizes.sort_unstable();
        Self { sizes }
    }

    /// Number of observed compactions.
    pub fn count(&self) -> usize {
        self.sizes.len()
    }

    /// Fraction fired at or below `trigger_tokens` (0 when none observed).
    pub fn fraction_at_or_below(&self, trigger_tokens: u32) -> f64 {
        if self.sizes.is_empty() {
            return 0.0;
        }
        let at_or_below = self.sizes.partition_point(|&size| size <= trigger_tokens);
        at_or_below as f64 / self.sizes.len() as f64
    }
}

/// A flat band: every candidate within a tolerance of the best.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Band {
    /// Tolerance as a fraction of the best cost.
    pub tolerance: f64,
    /// Lowest member, tokens.
    pub low_tokens: u32,
    /// Highest member, tokens.
    pub high_tokens: u32,
    /// Every member, ascending.
    pub members: Vec<u32>,
}

/// Argmin and both bands.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bands {
    /// Trigger with the lowest cost.
    pub argmin_tokens: u32,
    /// Within 2 % of the best.
    pub within_2_percent: Band,
    /// Within 5 % of the best.
    pub within_5_percent: Band,
}

/// Bootstrap over sessions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BootstrapSummary {
    /// Resamples run.
    pub resamples: usize,
    /// 2.5th percentile of the argmin.
    pub argmin_p2_5: u32,
    /// Median argmin.
    pub argmin_median: u32,
    /// 97.5th percentile of the argmin.
    pub argmin_p97_5: u32,
    /// Share of resamples in which each trigger was the argmin.
    pub argmin_frequency: Vec<(u32, f64)>,
    /// Share of resamples in which the picked trigger was within 2 % of that
    /// resample's best.
    pub pick_within_band_share: f64,
}

/// One row of the rework sensitivity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SensitivityRow {
    /// Rework multiplier.
    pub multiplier: f64,
    /// Argmin at that multiplier.
    pub argmin_tokens: u32,
    /// 2 % band at that multiplier.
    pub within_2_percent: Band,
    /// Extra cost of the picked trigger relative to that multiplier's best.
    pub pick_overhead_fraction: f64,
}

/// The optimum of one model family.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelOptimum {
    /// Model family.
    pub family: String,
    /// Sessions of the family.
    pub sessions: usize,
    /// Argmin for the family alone.
    pub argmin_tokens: u32,
    /// 2 % band for the family alone.
    pub within_2_percent: Band,
    /// Bootstrap 95 % interval of the family's argmin.
    pub argmin_interval_tokens: (u32, u32),
}

/// One candidate trigger of the simulated grid.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CandidateRow {
    /// Candidate trigger.
    pub trigger_tokens: u32,
    /// Total weighted cost with rework.
    pub cost: f64,
    /// Total dollars with rework, when every price is known.
    pub usd: Option<f64>,
    /// Cost relative to the best (0 for the best).
    pub excess_fraction: f64,
    /// Mean simulated compactions summed over the sessions.
    pub compactions: f64,
    /// Fraction of observed compactions at or below this trigger.
    pub observed_support: f64,
}

/// How much to trust the pick.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    /// Few sessions, an unstable bootstrap or too little data.
    Low,
    /// Extrapolated, a prior rework figure, or a moderately stable bootstrap.
    Medium,
    /// Supported by observed compactions, a measured rework and a stable
    /// bootstrap.
    High,
}

/// Which branch of the pick rule produced the recommendation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PickRule {
    /// (a) No candidate has observed support: the plain argmin, extrapolated.
    PlainArgmin,
    /// (b) The lowest candidate inside the 2% band that has observed support.
    BandWithSupport,
    /// (c) The 2% band has no support but other candidates do: the cheapest
    /// supported candidate (not extrapolated; the unsupported argmin and its
    /// saving are reported separately).
    SupportedOptimum,
}

/// The recommendation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Recommendation {
    /// Recommended effective trigger, tokens.
    pub trigger_tokens: u32,
    /// Configuration key to write.
    pub setting_name: String,
    /// Configuration value to write.
    pub setting_value: u32,
    /// Whether the value had to be clamped to the accepted range.
    pub setting_clamped: bool,
    /// Why this trigger: the rule that produced the pick.
    pub basis: String,
    /// Trust in the pick.
    pub confidence: Confidence,
    /// No observed compaction supports this level; the pick rests on the
    /// simulation's assumptions alone.
    pub extrapolated: bool,
    /// Fraction of observed compactions at or below the pick.
    pub observed_support_at_pick: f64,
    /// Pick rule (c) only: the cheaper trigger the pick passed over because no
    /// observed compaction supports it (`None` otherwise).
    pub unsupported_argmin_tokens: Option<u32>,
    /// Pick rule (c) only: how much cheaper that unsupported argmin is
    /// relative to the pick, `1 - cost(argmin) / cost(pick)` (`None`
    /// otherwise).
    pub unsupported_relative_saving: Option<f64>,
    /// The plain argmin is the lowest or highest grid point, so the true
    /// optimum may lie outside the simulated grid; widen the grid and re-run.
    pub grid_floor_limited: bool,
    /// Which pick rule produced `trigger_tokens`.
    pub pick_rule: PickRule,
    /// Argmin and flat bands.
    pub bands: Bands,
    /// Bootstrap over sessions.
    pub bootstrap: BootstrapSummary,
    /// Optimum per model family.
    pub per_model: Vec<ModelOptimum>,
    /// Rework sensitivity.
    pub rework_sensitivity: Vec<SensitivityRow>,
    /// Every candidate with its cost and support.
    pub candidates: Vec<CandidateRow>,
    /// Sessions the optimum is computed over.
    pub sessions: usize,
    /// Rework model of the central estimate.
    pub rework: ReworkModel,
    /// Things the reader must know.
    pub warnings: Vec<String>,
}

/// Index of the lowest cost (first on ties).
fn argmin(costs: &[f64]) -> usize {
    costs
        .iter()
        .enumerate()
        .fold((0, f64::INFINITY), |best, (index, &cost)| {
            if cost < best.1 {
                (index, cost)
            } else {
                best
            }
        })
        .0
}

/// The flat band within `tolerance` of the best.
fn band_of(triggers: &[u32], costs: &[f64], tolerance: f64) -> Band {
    let best = costs[argmin(costs)];
    let members: Vec<u32> = triggers
        .iter()
        .zip(costs)
        .filter(|(_, &cost)| cost <= best * (1.0 + tolerance))
        .map(|(&trigger, _)| trigger)
        .collect();
    Band {
        tolerance,
        low_tokens: members.first().copied().unwrap_or(0),
        high_tokens: members.last().copied().unwrap_or(0),
        members,
    }
}

fn bands_of(triggers: &[u32], costs: &[f64], fractions: (f64, f64)) -> Bands {
    Bands {
        argmin_tokens: triggers[argmin(costs)],
        within_2_percent: band_of(triggers, costs, fractions.0),
        within_5_percent: band_of(triggers, costs, fractions.1),
    }
}

/// Session-by-trigger cost matrix with rework applied.
fn cost_matrix(table: &SimTable, rework: &ReworkModel, sessions: &[&SessionSim]) -> Vec<Vec<f64>> {
    sessions
        .iter()
        .map(|session| {
            (0..table.triggers.len())
                .map(|index| table.session_cost(session, index, rework))
                .collect()
        })
        .collect()
}

fn column_sums(matrix: &[Vec<f64>], picks: &[usize], width: usize) -> Vec<f64> {
    let mut sums = vec![0.0; width];
    for &row in picks {
        for (sum, value) in sums.iter_mut().zip(&matrix[row]) {
            *sum += value;
        }
    }
    sums
}

struct BootstrapRaw {
    argmins: Vec<usize>,
    bests: Vec<f64>,
    totals: Vec<Vec<f64>>,
}

fn bootstrap(
    matrix: &[Vec<f64>],
    width: usize,
    resamples: usize,
    seed: u64,
    keep_totals: bool,
) -> BootstrapRaw {
    let mut rng = SeededRng::new(seed);
    let rows = matrix.len();
    let mut raw = BootstrapRaw {
        argmins: Vec::with_capacity(resamples),
        bests: Vec::with_capacity(resamples),
        totals: Vec::new(),
    };
    for _ in 0..resamples {
        let picks: Vec<usize> = (0..rows).map(|_| rng.next_below(rows)).collect();
        let sums = column_sums(matrix, &picks, width);
        let best = argmin(&sums);
        raw.argmins.push(best);
        raw.bests.push(sums[best]);
        if keep_totals {
            raw.totals.push(sums);
        }
    }
    raw
}

fn percentile_of_indices(sorted_triggers: &[u32], mut indices: Vec<usize>, percent: f64) -> u32 {
    indices.sort_unstable();
    let position = ((indices.len() - 1) as f64 * percent / 100.0).round() as usize;
    sorted_triggers[indices[position.min(indices.len() - 1)]]
}

/// Computes the recommendation of a simulation.
///
/// `observed` carries the sizes of the compactions in the corpus (for the
/// support rule), `semantics` maps the trigger to the configuration value.
pub fn optimize(
    table: &SimTable,
    observed: &ObservedCompactions,
    semantics: &AgentSemantics,
    config: &OptimizeConfig,
) -> AnalysisResult<Recommendation> {
    if table.sessions.is_empty() || table.triggers.is_empty() {
        return Err(AnalysisError::NoReplayableSession);
    }
    let resamples = config.bootstrap_resamples.max(MIN_BOOTSTRAP_RESAMPLES);
    let width = table.triggers.len();
    let all_sessions: Vec<&SessionSim> = table.sessions.iter().collect();
    let central = ReworkModel {
        multiplier: config.rework.multiplier,
        ..config.rework
    };
    let matrix = cost_matrix(table, &central, &all_sessions);
    let everyone: Vec<usize> = (0..matrix.len()).collect();
    let totals = column_sums(&matrix, &everyone, width);
    let bands = bands_of(&table.triggers, &totals, config.band_fractions);
    let support: Vec<f64> = table
        .triggers
        .iter()
        .map(|&trigger| observed.fraction_at_or_below(trigger))
        .collect();

    let decision = pick(&table.triggers, &totals, &support, config);
    let pick_index = decision.index;
    let pick_trigger = table.triggers[pick_index];
    let mapping = semantics.trigger_to_setting(pick_trigger);

    let raw = bootstrap(&matrix, width, resamples, config.seed, true);
    let supported_mask: Option<Vec<bool>> =
        (decision.rule == PickRule::SupportedOptimum).then(|| {
            support
                .iter()
                .map(|&value| is_supported(value, config))
                .collect()
        });
    let bootstrap_summary = summarize_bootstrap(
        table,
        &raw,
        pick_index,
        config.band_fractions.0,
        supported_mask.as_deref(),
    );
    let usd = table.total_usd(&central, &|_| true);
    let compactions = table.total_compactions(&|_| true);
    let best_cost = totals[argmin(&totals)];
    let candidates: Vec<CandidateRow> = (0..width)
        .map(|index| CandidateRow {
            trigger_tokens: table.triggers[index],
            cost: totals[index],
            usd: usd[index],
            excess_fraction: totals[index] / best_cost - 1.0,
            compactions: compactions[index],
            observed_support: support[index],
        })
        .collect();

    let rework_sensitivity = sensitivity(table, config, &all_sessions, pick_index);
    let per_model = per_model_optima(table, config, resamples);
    let unsupported = (decision.rule == PickRule::SupportedOptimum).then(|| {
        let saving = 1.0 - totals[decision.argmin_index] / totals[pick_index];
        (table.triggers[decision.argmin_index], saving)
    });
    let grid_floor_limited = decision.argmin_index == 0 || decision.argmin_index + 1 == width;
    let warnings = collect_warnings(&WarningContext {
        table,
        observed,
        config,
        decision: &decision,
        support: &support,
        unsupported,
        grid_floor_limited,
        bands: &bands,
        bootstrap: &bootstrap_summary,
        per_model: &per_model,
        clamp_note: clamp_note(&mapping),
    });
    let confidence = grade(
        table,
        config,
        // Rule (c) passed over a cheaper unsupported candidate: never "High".
        decision.extrapolated || decision.rule == PickRule::SupportedOptimum,
        &bootstrap_summary,
        observed.count(),
    );
    let basis = format!(
        "{}; {}",
        describe_basis(&decision, &support, &table.triggers, config),
        central.describe()
    );
    Ok(Recommendation {
        trigger_tokens: pick_trigger,
        setting_name: mapping.setting_name.clone(),
        setting_value: mapping.value,
        setting_clamped: mapping.clamped,
        basis,
        confidence,
        extrapolated: decision.extrapolated,
        observed_support_at_pick: support[pick_index],
        unsupported_argmin_tokens: unsupported.map(|(trigger, _)| trigger),
        unsupported_relative_saving: unsupported.map(|(_, saving)| saving),
        grid_floor_limited,
        pick_rule: decision.rule,
        bands,
        bootstrap: bootstrap_summary,
        per_model,
        rework_sensitivity,
        candidates,
        sessions: table.sessions.len(),
        rework: central,
        warnings,
    })
}

struct PickDecision {
    index: usize,
    extrapolated: bool,
    argmin_index: usize,
    rule: PickRule,
}

fn is_supported(support: f64, config: &OptimizeConfig) -> bool {
    support >= config.min_support_fraction && support > 0.0
}

/// The pick rule (see the module docs). With S the candidates whose observed
/// support reaches the threshold:
///
/// * (a) S empty: the plain argmin, extrapolated;
/// * (b) the 2% band intersects S: the lowest candidate in band and S;
/// * (c) otherwise: the cheapest candidate of S, not extrapolated.
fn pick(
    triggers: &[u32],
    totals: &[f64],
    support: &[f64],
    config: &OptimizeConfig,
) -> PickDecision {
    let argmin_index = argmin(totals);
    let supported: Vec<usize> = (0..triggers.len())
        .filter(|&index| is_supported(support[index], config))
        .collect();
    if supported.is_empty() {
        return PickDecision {
            index: argmin_index,
            extrapolated: true,
            argmin_index,
            rule: PickRule::PlainArgmin,
        };
    }
    let band = band_of(triggers, totals, config.band_fractions.0);
    let in_band = supported
        .iter()
        .copied()
        .find(|&index| band.members.contains(&triggers[index]));
    if let Some(index) = in_band {
        return PickDecision {
            index,
            extrapolated: false,
            argmin_index,
            rule: PickRule::BandWithSupport,
        };
    }
    let cheapest = supported.iter().copied().fold(supported[0], |best, index| {
        if totals[index] < totals[best] {
            index
        } else {
            best
        }
    });
    PickDecision {
        index: cheapest,
        extrapolated: false,
        argmin_index,
        rule: PickRule::SupportedOptimum,
    }
}

fn describe_basis(
    decision: &PickDecision,
    support: &[f64],
    triggers: &[u32],
    config: &OptimizeConfig,
) -> String {
    let percent = |index: usize| (support[index] * 100.0).round();
    match decision.rule {
        PickRule::PlainArgmin => format!(
            "plain argmin ({} tokens); extrapolated: {:.0}% of observed compactions at or below this level (no candidate reaches the {:.0}% support threshold)",
            triggers[decision.argmin_index],
            percent(decision.index),
            config.min_support_fraction * 100.0
        ),
        PickRule::BandWithSupport if decision.index == decision.argmin_index => format!(
            "argmin ({} tokens), supported by {:.0}% of observed compactions at or below it",
            triggers[decision.index],
            percent(decision.index)
        ),
        PickRule::BandWithSupport => format!(
            "lowest value inside the {:.0}% band with observed support ({} tokens; {:.0}% of observed compactions at or below it); the plain argmin is {} tokens",
            config.band_fractions.0 * 100.0,
            triggers[decision.index],
            percent(decision.index),
            triggers[decision.argmin_index]
        ),
        PickRule::SupportedOptimum => format!(
            "supported optimum: the cheapest candidate with observed support ({} tokens; {:.0}% of observed compactions at or below it); the {:.0}% band around the plain argmin ({} tokens) has no observed support",
            triggers[decision.index],
            percent(decision.index),
            config.band_fractions.0 * 100.0,
            triggers[decision.argmin_index]
        ),
    }
}

fn summarize_bootstrap(
    table: &SimTable,
    raw: &BootstrapRaw,
    pick_index: usize,
    band_fraction: f64,
    supported_mask: Option<&[bool]>,
) -> BootstrapSummary {
    let resamples = raw.argmins.len();
    let mut frequency = vec![0_usize; table.triggers.len()];
    for &index in &raw.argmins {
        frequency[index] += 1;
    }
    // Rule (c) picks the cheapest *supported* candidate, so each resample is
    // judged against the best supported candidate, not the unconstrained one.
    let within = raw
        .totals
        .iter()
        .zip(&raw.bests)
        .filter(|(sums, &best)| {
            let reference = match supported_mask {
                Some(mask) => sums
                    .iter()
                    .zip(mask)
                    .filter(|(_, &supported)| supported)
                    .map(|(&sum, _)| sum)
                    .fold(f64::INFINITY, f64::min),
                None => best,
            };
            sums[pick_index] <= reference * (1.0 + band_fraction)
        })
        .count();
    BootstrapSummary {
        resamples,
        argmin_p2_5: percentile_of_indices(&table.triggers, raw.argmins.clone(), 2.5),
        argmin_median: percentile_of_indices(&table.triggers, raw.argmins.clone(), 50.0),
        argmin_p97_5: percentile_of_indices(&table.triggers, raw.argmins.clone(), 97.5),
        argmin_frequency: frequency
            .iter()
            .enumerate()
            .filter(|(_, &count)| count > 0)
            .map(|(index, &count)| (table.triggers[index], count as f64 / resamples as f64))
            .collect(),
        pick_within_band_share: within as f64 / resamples as f64,
    }
}

fn sensitivity(
    table: &SimTable,
    config: &OptimizeConfig,
    sessions: &[&SessionSim],
    pick_index: usize,
) -> Vec<SensitivityRow> {
    config
        .sensitivity_multipliers
        .iter()
        .map(|&multiplier| {
            let rework = config.rework.with_multiplier(multiplier);
            let matrix = cost_matrix(table, &rework, sessions);
            let everyone: Vec<usize> = (0..matrix.len()).collect();
            let totals = column_sums(&matrix, &everyone, table.triggers.len());
            let best = totals[argmin(&totals)];
            SensitivityRow {
                multiplier,
                argmin_tokens: table.triggers[argmin(&totals)],
                within_2_percent: band_of(&table.triggers, &totals, config.band_fractions.0),
                pick_overhead_fraction: totals[pick_index] / best - 1.0,
            }
        })
        .collect()
}

fn per_model_optima(
    table: &SimTable,
    config: &OptimizeConfig,
    resamples: usize,
) -> Vec<ModelOptimum> {
    let mut families: Vec<String> = table
        .sessions
        .iter()
        .map(|session| session.model_family.clone())
        .collect();
    families.sort();
    families.dedup();
    let central = ReworkModel {
        multiplier: config.rework.multiplier,
        ..config.rework
    };
    let mut optima = Vec::new();
    for family in families {
        let sessions: Vec<&SessionSim> = table
            .sessions
            .iter()
            .filter(|session| session.model_family == family)
            .collect();
        if sessions.len() < config.min_sessions_per_model {
            continue;
        }
        let matrix = cost_matrix(table, &central, &sessions);
        let everyone: Vec<usize> = (0..matrix.len()).collect();
        let totals = column_sums(&matrix, &everyone, table.triggers.len());
        let raw = bootstrap(
            &matrix,
            table.triggers.len(),
            resamples.min(200),
            config.seed,
            false,
        );
        optima.push(ModelOptimum {
            family,
            sessions: sessions.len(),
            argmin_tokens: table.triggers[argmin(&totals)],
            within_2_percent: band_of(&table.triggers, &totals, config.band_fractions.0),
            argmin_interval_tokens: (
                percentile_of_indices(&table.triggers, raw.argmins.clone(), 2.5),
                percentile_of_indices(&table.triggers, raw.argmins.clone(), 97.5),
            ),
        });
    }
    optima.sort_by_key(|optimum| std::cmp::Reverse(optimum.sessions));
    optima
}

fn grade(
    table: &SimTable,
    config: &OptimizeConfig,
    extrapolated: bool,
    bootstrap: &BootstrapSummary,
    observed_compactions: usize,
) -> Confidence {
    if table.sessions.len() < 10
        || bootstrap.pick_within_band_share < 0.6
        || observed_compactions < 5
    {
        return Confidence::Low;
    }
    let stable = bootstrap.pick_within_band_share >= 0.9;
    let measured = config.rework.source == ReworkSource::Measured;
    if !extrapolated && stable && measured {
        Confidence::High
    } else {
        Confidence::Medium
    }
}

fn clamp_note(mapping: &crate::semantics::SettingMapping) -> Option<String> {
    mapping.clamped.then(|| {
        format!(
            "the configuration value {} had to be clamped into the accepted range; it realizes a trigger of {} tokens instead",
            mapping.value, mapping.realized_trigger_tokens
        )
    })
}

/// Inputs of the warning collection.
struct WarningContext<'a> {
    table: &'a SimTable,
    observed: &'a ObservedCompactions,
    config: &'a OptimizeConfig,
    decision: &'a PickDecision,
    support: &'a [f64],
    unsupported: Option<(u32, f64)>,
    grid_floor_limited: bool,
    bands: &'a Bands,
    bootstrap: &'a BootstrapSummary,
    per_model: &'a [ModelOptimum],
    clamp_note: Option<String>,
}

fn collect_warnings(context: &WarningContext<'_>) -> Vec<String> {
    let WarningContext {
        table,
        observed,
        config,
        decision,
        support,
        unsupported,
        grid_floor_limited,
        bands,
        bootstrap,
        per_model,
        clamp_note,
    } = context;
    let mut warnings = Vec::new();
    warnings.extend(table.warnings.iter().cloned());
    if decision.extrapolated {
        warnings.push(
            "extrapolated: the corpus holds no (or too few) compactions at this level; the simulation assumes growth per request, the post-compaction prefix and the rework do not depend on the trigger, and the quality loss of more frequent summaries is not priced".to_string(),
        );
    }
    if config.rework.source == ReworkSource::Prior {
        warnings.push(
            "the rework penalty is a research prior from one corpus, not measured on these logs; see the 0.5x / 1x / 2x sensitivity".to_string(),
        );
    } else {
        warnings.push(format!(
            "{}; the sensitivity rows scale it by 0.5x, 1x and 2x",
            config.rework.describe()
        ));
    }
    if table.sessions.len() < 30 {
        warnings.push(format!(
            "only {} replayable sessions; the optimum is sensitive to individual sessions",
            table.sessions.len()
        ));
    }
    if observed.count() < 30 && config.rework.source == ReworkSource::Prior {
        warnings.push(format!(
            "only {} observed compactions; the rework prior was not replaced by a measurement",
            observed.count()
        ));
    }
    if let Some((unsupported_trigger, saving)) = unsupported {
        warnings.push(format!(
            "cost keeps falling below {} tokens (down to {} tokens, {:.1}% cheaper in the simulation) but no observed compactions there ({:.0}% at or below {} tokens)",
            table.triggers[decision.index],
            unsupported_trigger,
            saving * 100.0,
            support[decision.argmin_index] * 100.0,
            unsupported_trigger
        ));
    }
    if *grid_floor_limited {
        let side = if bands.argmin_tokens == table.triggers.first().copied().unwrap_or(0) {
            "lowest"
        } else {
            "highest"
        };
        warnings.push(format!(
            "the optimum is the {side} simulated grid point ({} tokens): the true optimum may lie outside the grid; widen the grid and re-run",
            bands.argmin_tokens
        ));
    }
    if bootstrap.pick_within_band_share < 0.9 {
        warnings.push(format!(
            "the pick is within 2% of the best in only {:.0}% of bootstrap resamples",
            bootstrap.pick_within_band_share * 100.0
        ));
    }
    if per_model.len() > 1 {
        let distinct: std::collections::BTreeSet<u32> =
            per_model.iter().map(|model| model.argmin_tokens).collect();
        warnings.push(if distinct.len() > 1 {
            "model families have different optima, but the setting is a single global value and cannot be applied per model; Apply uses the cost-weighted optimum".to_string()
        } else {
            "the setting is a single global value and cannot be applied per model (the per-model optima agree on this corpus)".to_string()
        });
    }
    if let Some(note) = clamp_note {
        warnings.push(note.clone());
    }
    warnings
}

mod compare;

pub use compare::{three_way_comparison, ComparisonPoint, PointQuality};

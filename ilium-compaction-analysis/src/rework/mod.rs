//! Measured rework: what a compaction costs beyond its own requests.
//!
//! After a compaction the agent re-reads files and re-runs commands that the
//! summary dropped. The research measured that on two corpora (Claude about
//! 72,000, Codex about 40,000 weighted tokens per compaction); this module
//! measures it on the user's own logs once at least
//! [`ReworkConfig::min_compactions`] compactions back it, and
//! [`ReworkModel`](crate::replay::ReworkModel) then replaces the prior.
//!
//! # Method (K = 30 requests, main sessions only)
//!
//! For each measured compaction, three windows of work requests are compared:
//!
//! * **A**: the first K requests after the compaction;
//! * **B**: the K requests just before it (the previous cycle's end);
//! * **M**: a K-request window in the middle of the following cycle (only
//!   when that cycle is at least 100 requests long): the control for "normal"
//!   work at a similar context size.
//!
//! Per window the compact tool features give: read-like calls, requests that
//! read, requests that re-read a path read *before the compaction* ("lost"
//! paths), repeated identical calls, requests until the first edit. The
//! excess of A over B or M is converted to weighted tokens as
//! `excess requests x mean weighted cost of an A request` (the first request
//! after a compaction is excluded from that mean: its cold rebuild is already
//! priced by the compaction itself).
//!
//! Estimators ([`EstimatorKind`]):
//!
//! * `ExcessCallsVsPre`: `(calls(A) - calls(B)) x (non-read cost per call +
//!   read price x mean context)`, unclamped mean.
//! * `LostRereadTurnsVsControl`, `ReadingTurnsVsControl`,
//!   `LostRereadTurnsVsPre`: `max(0, excess requests) x weighted cost per
//!   request`.
//!
//! # Central policy: the median of the estimators
//!
//! No single estimator is trusted. In the research they disagreed, even in
//! sign, and on real logs the single "excess calls" estimator can be noisy
//! (a confidence interval spanning zero) while the others sit at two to four
//! times its value. The central value is therefore, for both agents, the
//! **median of the means of every estimator backed by at least
//! `min_compactions` compactions** (the middle of the spread), and its 95%
//! interval comes from bootstrapping that median over whole compactions (all
//! estimators are recomputed on the same resample). The individual estimator
//! summaries and the spread stay in the result.
//!
//! # Noise guard
//!
//! [`MeasuredRework::noisy`] is set when the central interval's lower bound is
//! at or below zero AND the estimator spread (high / low) exceeds
//! [`NOISY_SPREAD_RATIO`] (a non-positive low end counts as unbounded). A
//! noisy measurement is blended 50/50 with the research prior by
//! [`ReworkModel::from_measurement`](crate::replay::ReworkModel::from_measurement).

mod windows;

use serde::{Deserialize, Serialize};

use crate::agent::AgentKind;
use crate::price::PriceLookup;
use crate::rng::SeededRng;
use crate::stats::{median, percentile_sorted};
use crate::trace::SessionTrace;
use windows::{collect_compaction_samples, CompactionSample, SampleSettings};

/// A measurement is noisy when its central 95% interval reaches zero and the
/// estimators span more than this ratio (high / low).
pub const NOISY_SPREAD_RATIO: f64 = 3.0;

/// Which estimator the central value comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CentralEstimator {
    /// The median of the means of every estimator at the minimum (the policy).
    MedianOfEstimators,
    /// Only one estimator reached the minimum, so the median is that one.
    Single(EstimatorKind),
}

impl CentralEstimator {
    /// Short label for reports.
    pub fn label(&self) -> String {
        match self {
            Self::MedianOfEstimators => "median of estimators".to_string(),
            Self::Single(kind) => format!("single estimator {kind:?}"),
        }
    }
}

/// Settings of [`measure_with`].
#[derive(Debug, Clone)]
pub struct ReworkConfig {
    /// Window length K in requests (the research used 30).
    pub window_turns: usize,
    /// Fewest compactions an estimator needs to count (default 30).
    pub min_compactions: usize,
    /// A mid-cycle control window needs a following cycle at least this long.
    pub min_control_cycle_turns: usize,
    /// Bootstrap resamples for the confidence intervals.
    pub bootstrap_resamples: usize,
    /// Seed of the bootstrap.
    pub seed: u64,
}

impl Default for ReworkConfig {
    fn default() -> Self {
        Self {
            window_turns: 30,
            min_compactions: 30,
            min_control_cycle_turns: 100,
            bootstrap_resamples: 1_000,
            seed: 11,
        }
    }
}

/// The ways to turn the window comparison into weighted tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EstimatorKind {
    /// Excess read-like calls after the compaction versus before it.
    ExcessCallsVsPre,
    /// Excess requests re-reading a pre-compaction path, versus the mid-cycle
    /// control.
    LostRereadTurnsVsControl,
    /// Excess requests that read, versus the mid-cycle control.
    ReadingTurnsVsControl,
    /// Excess requests re-reading a pre-compaction path, versus before.
    LostRereadTurnsVsPre,
}

impl EstimatorKind {
    /// Every estimator.
    pub const ALL: [EstimatorKind; 4] = [
        EstimatorKind::ExcessCallsVsPre,
        EstimatorKind::LostRereadTurnsVsControl,
        EstimatorKind::ReadingTurnsVsControl,
        EstimatorKind::LostRereadTurnsVsPre,
    ];
}

/// One estimator's result.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct EstimatorSummary {
    /// Which estimator.
    pub kind: EstimatorKind,
    /// Compactions it could be computed for.
    pub compactions: usize,
    /// Mean weighted tokens per compaction (unclamped).
    pub mean_tokens: f64,
    /// Median weighted tokens per compaction.
    pub median_tokens: f64,
    /// 2.5th percentile of the bootstrapped mean.
    pub ci_low_tokens: f64,
    /// 97.5th percentile of the bootstrapped mean.
    pub ci_high_tokens: f64,
}

/// Mean window metrics (for the report; not used by the estimators' maths
/// beyond what is described in the module docs).
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct WindowMeans {
    /// Windows averaged.
    pub windows: usize,
    /// Read-like calls per window.
    pub read_calls: f64,
    /// Requests with a read per window.
    pub reading_turns: f64,
    /// Requests re-reading a pre-compaction path per window.
    pub lost_reread_turns: f64,
    /// Share of the distinct paths read that were read before the compaction.
    pub lost_path_share: f64,
    /// Repeated identical calls inside the window.
    pub repeated_calls: f64,
    /// Commands identical to a pre-compaction command.
    pub lost_command_repeats: f64,
    /// Requests until the first edit or write (window length when none).
    pub turns_to_first_edit: f64,
    /// Mean weighted cost of a request (first request after a compaction
    /// excluded for the A window).
    pub weighted_cost_per_turn: f64,
}

/// The measured rework.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MeasuredRework {
    /// The agent measured.
    pub agent: AgentKind,
    /// Central estimate, weighted tokens per compaction, clamped at zero.
    pub tokens_per_compaction: f64,
    /// Central value before clamping: the median of the estimator means.
    pub mean_tokens: f64,
    /// Median of the estimators' per-compaction medians.
    pub median_tokens: f64,
    /// 95% bootstrap interval of the central value, low end.
    pub ci_low_tokens: f64,
    /// 95% bootstrap interval of the central value, high end.
    pub ci_high_tokens: f64,
    /// Compactions backing the central estimate (the smallest count among the
    /// estimators used).
    pub compactions: usize,
    /// How the central value was chosen.
    pub central_estimator: CentralEstimator,
    /// The estimators that reached the minimum and feed the central value.
    pub central_estimators: Vec<EstimatorKind>,
    /// Central interval reaches zero and the estimators span more than
    /// [`NOISY_SPREAD_RATIO`]: the value should be blended with the prior.
    pub noisy: bool,
    /// Every estimator, including those below the minimum.
    pub estimators: Vec<EstimatorSummary>,
    /// Lowest estimator mean (among those at the minimum).
    pub spread_low_tokens: f64,
    /// Highest estimator mean (among those at the minimum).
    pub spread_high_tokens: f64,
    /// Measured compactions of main sessions examined.
    pub examined_compactions: usize,
    /// Window length K.
    pub window_turns: usize,
    /// Mean metrics of the windows after the compactions.
    pub after: WindowMeans,
    /// Mean metrics of the windows before the compactions.
    pub before: WindowMeans,
    /// Mean metrics of the mid-cycle control windows.
    pub control: WindowMeans,
}

impl MeasuredRework {
    /// One-line description for reports and warnings.
    pub fn describe(&self) -> String {
        format!(
            "rework measured from {} compactions ({:.0} weighted tokens, 95% CI {:.0} to {:.0}; {}; estimators span {:.0} to {:.0}{})",
            self.compactions,
            self.tokens_per_compaction,
            self.ci_low_tokens,
            self.ci_high_tokens,
            self.central_estimator.label(),
            self.spread_low_tokens,
            self.spread_high_tokens,
            if self.noisy { "; noisy" } else { "" }
        )
    }
}

/// Measures the rework of `agent`'s main sessions with the defaults and the
/// research weights; `None` unless at least 30 compactions back it.
pub fn measure(traces: &[SessionTrace], agent: AgentKind) -> Option<MeasuredRework> {
    measure_with(traces, agent, None, &ReworkConfig::default())
}

/// [`measure`] with a price lookup (relative weights come from it) and explicit
/// settings.
pub fn measure_with(
    traces: &[SessionTrace],
    agent: AgentKind,
    lookup: Option<PriceLookup<'_>>,
    config: &ReworkConfig,
) -> Option<MeasuredRework> {
    let settings = SampleSettings {
        window_turns: config.window_turns,
        min_control_cycle_turns: config.min_control_cycle_turns,
    };
    let mut samples: Vec<CompactionSample> = Vec::new();
    for trace in traces
        .iter()
        .filter(|trace| trace.agent == agent && !trace.is_subagent)
    {
        collect_compaction_samples(trace, lookup, &settings, &mut samples);
    }
    if samples.is_empty() {
        return None;
    }
    let per_call = mean_nonread_cost_per_call(&samples);
    // Per-compaction value of every estimator (None when its windows are
    // missing); the bootstrap resamples compactions jointly over these.
    let table: Vec<[Option<f64>; 4]> = samples
        .iter()
        .map(|sample| EstimatorKind::ALL.map(|kind| estimate(kind, sample, per_call)))
        .collect();
    let mut rng = SeededRng::new(config.seed);
    let estimators: Vec<EstimatorSummary> = EstimatorKind::ALL
        .iter()
        .enumerate()
        .filter_map(|(column, &kind)| {
            let values: Vec<f64> = table.iter().filter_map(|row| row[column]).collect();
            summarize(kind, &values, config, &mut rng)
        })
        .collect();
    let qualifying: Vec<(usize, &EstimatorSummary)> = EstimatorKind::ALL
        .iter()
        .enumerate()
        .filter_map(|(column, kind)| {
            estimators
                .iter()
                .find(|summary| summary.kind == *kind)
                .filter(|summary| summary.compactions >= config.min_compactions)
                .map(|summary| (column, summary))
        })
        .collect();
    if qualifying.is_empty() {
        return None;
    }
    let columns: Vec<usize> = qualifying.iter().map(|(column, _)| *column).collect();
    let means: Vec<f64> = qualifying
        .iter()
        .map(|(_, summary)| summary.mean_tokens)
        .collect();
    let medians: Vec<f64> = qualifying
        .iter()
        .map(|(_, summary)| summary.median_tokens)
        .collect();
    let central_mean = median(&means);
    let (ci_low, ci_high) = bootstrap_median_ci(&table, &columns, config, &mut rng);
    let spread_low = means.iter().copied().fold(f64::INFINITY, f64::min);
    let spread_high = means.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let noisy = is_noisy(ci_low, spread_low, spread_high);
    let central_estimators: Vec<EstimatorKind> =
        qualifying.iter().map(|(_, summary)| summary.kind).collect();
    Some(MeasuredRework {
        agent,
        tokens_per_compaction: central_mean.max(0.0),
        mean_tokens: central_mean,
        median_tokens: median(&medians),
        ci_low_tokens: ci_low,
        ci_high_tokens: ci_high,
        compactions: qualifying
            .iter()
            .map(|(_, summary)| summary.compactions)
            .min()
            .unwrap_or(0),
        central_estimator: match central_estimators.as_slice() {
            [only] => CentralEstimator::Single(*only),
            _ => CentralEstimator::MedianOfEstimators,
        },
        central_estimators,
        noisy,
        spread_low_tokens: spread_low,
        spread_high_tokens: spread_high,
        estimators,
        examined_compactions: samples.len(),
        window_turns: config.window_turns,
        after: window_means(samples.iter().map(|sample| &sample.after).map(Some)),
        before: window_means(samples.iter().map(|sample| sample.before.as_ref())),
        control: window_means(samples.iter().map(|sample| sample.control.as_ref())),
    })
}

/// The noise guard: the central interval reaches zero AND the estimators span
/// more than [`NOISY_SPREAD_RATIO`] (a non-positive low end is unbounded).
pub fn is_noisy(ci_low: f64, spread_low: f64, spread_high: f64) -> bool {
    if ci_low > 0.0 {
        return false;
    }
    if spread_low <= 0.0 {
        return spread_high > 0.0;
    }
    spread_high / spread_low > NOISY_SPREAD_RATIO
}

/// 95% interval of the median of the selected estimators' means, bootstrapping
/// whole compactions (every estimator recomputed on the same resample).
fn bootstrap_median_ci(
    table: &[[Option<f64>; 4]],
    columns: &[usize],
    config: &ReworkConfig,
    rng: &mut SeededRng,
) -> (f64, f64) {
    let mut medians: Vec<f64> = (0..config.bootstrap_resamples.max(1))
        .map(|_| {
            let mut sums = [0.0_f64; 4];
            let mut counts = [0_u32; 4];
            for _ in 0..table.len() {
                let row = &table[rng.next_below(table.len())];
                for &column in columns {
                    if let Some(value) = row[column] {
                        sums[column] += value;
                        counts[column] += 1;
                    }
                }
            }
            let means: Vec<f64> = columns
                .iter()
                .filter(|&&column| counts[column] > 0)
                .map(|&column| sums[column] / f64::from(counts[column]))
                .collect();
            median(&means)
        })
        .collect();
    medians.sort_by(f64::total_cmp);
    (
        percentile_sorted(&medians, 2.5),
        percentile_sorted(&medians, 97.5),
    )
}

/// Mean non-read weighted cost per tool call over the post-compaction windows
/// (the research's `per_call_nonread`).
fn mean_nonread_cost_per_call(samples: &[CompactionSample]) -> f64 {
    let values: Vec<f64> = samples
        .iter()
        .map(|sample| sample.after.nonread_cost_total / f64::from(sample.after.tool_calls.max(1)))
        .collect();
    if values.is_empty() {
        0.0
    } else {
        values.iter().sum::<f64>() / values.len() as f64
    }
}

/// The estimator's value for one compaction, when its windows exist.
fn estimate(kind: EstimatorKind, sample: &CompactionSample, per_call_nonread: f64) -> Option<f64> {
    let after = &sample.after;
    let turn_cost = after.weighted_cost_per_turn;
    match kind {
        EstimatorKind::ExcessCallsVsPre => {
            let before = sample.before.as_ref()?;
            let excess = f64::from(after.read_calls) - f64::from(before.read_calls);
            Some(excess * (per_call_nonread + sample.read_price * after.mean_context))
        }
        EstimatorKind::LostRereadTurnsVsControl => {
            let control = sample.control.as_ref()?;
            let excess = f64::from(after.lost_reread_turns) - f64::from(control.lost_reread_turns);
            Some(excess.max(0.0) * turn_cost)
        }
        EstimatorKind::ReadingTurnsVsControl => {
            let control = sample.control.as_ref()?;
            let excess = f64::from(after.reading_turns) - f64::from(control.reading_turns);
            Some(excess.max(0.0) * turn_cost)
        }
        EstimatorKind::LostRereadTurnsVsPre => {
            let before = sample
                .before
                .as_ref()
                .filter(|_| sample.has_previous_compaction)?;
            let excess = f64::from(after.lost_reread_turns) - f64::from(before.lost_reread_turns);
            Some(excess.max(0.0) * turn_cost)
        }
    }
}

fn summarize(
    kind: EstimatorKind,
    values: &[f64],
    config: &ReworkConfig,
    rng: &mut SeededRng,
) -> Option<EstimatorSummary> {
    if values.is_empty() {
        return None;
    }
    let count = values.len() as f64;
    let mean = values.iter().sum::<f64>() / count;
    let mut means: Vec<f64> = (0..config.bootstrap_resamples.max(1))
        .map(|_| {
            let total: f64 = (0..values.len())
                .map(|_| values[rng.next_below(values.len())])
                .sum();
            total / count
        })
        .collect();
    means.sort_by(f64::total_cmp);
    Some(EstimatorSummary {
        kind,
        compactions: values.len(),
        mean_tokens: mean,
        median_tokens: median(values),
        ci_low_tokens: percentile_sorted(&means, 2.5),
        ci_high_tokens: percentile_sorted(&means, 97.5),
    })
}

fn window_means<'a>(
    windows: impl Iterator<Item = Option<&'a windows::WindowMetrics>>,
) -> WindowMeans {
    let present: Vec<&windows::WindowMetrics> = windows.flatten().collect();
    if present.is_empty() {
        return WindowMeans::default();
    }
    let count = present.len() as f64;
    let mean = |extract: &dyn Fn(&windows::WindowMetrics) -> f64| -> f64 {
        present.iter().map(|window| extract(window)).sum::<f64>() / count
    };
    WindowMeans {
        windows: present.len(),
        read_calls: mean(&|window| f64::from(window.read_calls)),
        reading_turns: mean(&|window| f64::from(window.reading_turns)),
        lost_reread_turns: mean(&|window| f64::from(window.lost_reread_turns)),
        lost_path_share: mean(&|window| {
            if window.distinct_paths == 0 {
                0.0
            } else {
                f64::from(window.lost_paths) / f64::from(window.distinct_paths)
            }
        }),
        repeated_calls: mean(&|window| f64::from(window.repeated_calls)),
        lost_command_repeats: mean(&|window| f64::from(window.lost_command_repeats)),
        turns_to_first_edit: mean(&|window| f64::from(window.turns_to_first_edit)),
        weighted_cost_per_turn: mean(&|window| window.weighted_cost_per_turn),
    }
}

//! Pure cost policy for the agent-spend indicators.
//!
//! Nothing here touches I/O, the UI, or the clock. It turns token counts into
//! estimated dollars ([`PriceTable`]), decides what counts as "a lot"
//! ([`Calibration`] -> [`LevelScale`] -> [`CostLevel`]), and renders the small
//! text visuals ([`CostLevel::glyph`], [`CostLevel::meter`],
//! [`sparkline_glyphs`]) so the tree, the detail card and the settings preview
//! all draw exactly the same thing.
//!
//! Prices are API list prices per million tokens. The Anthropic rows come from
//! Anthropic's published model table; cache reads use the published discount
//! and cache writes assume the 1.25x five-minute rate. The OpenAI rows are
//! third-party reports of the published GPT-5.6 prices with a 0.1x cache read;
//! OpenAI has no cache-write charge. A user-configured override table always
//! wins, and a model with no price is reported as unpriced rather than
//! guessed, so the UI can mark the figure as a lower bound.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::session_stats::{ModelUsage, TokenTotals};

/// Number of distinct expense levels every visual can express.
pub const LEVEL_COUNT: usize = 5;
/// History shorter than this many sessions is too thin to calibrate against.
pub const MIN_HISTORY_SESSIONS: usize = 8;

/// Dollar prices per million tokens for one model.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ModelPrice {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    #[serde(default)]
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

    /// Dollars for `tokens`. Reasoning tokens are already inside `output`.
    pub fn cost(&self, tokens: &TokenTotals) -> f64 {
        (tokens.input as f64 * self.input
            + tokens.output as f64 * self.output
            + tokens.cache_read as f64 * self.cache_read
            + tokens.cache_write as f64 * self.cache_write)
            / 1_000_000.0
    }
}

/// Built-in list prices, matched by the longest model-name prefix so a dated
/// snapshot such as `claude-haiku-4-5-20251001` finds its family row.
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
];

/// Built-in prices plus the user's overrides.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PriceTable {
    overrides: BTreeMap<String, ModelPrice>,
}

impl PriceTable {
    pub fn with_overrides(overrides: &BTreeMap<String, ModelPrice>) -> Self {
        Self {
            overrides: overrides
                .iter()
                .map(|(model, price)| (model.to_ascii_lowercase(), *price))
                .collect(),
        }
    }

    /// Longest-prefix match across overrides and built-ins. A more specific
    /// prefix always wins; on equal length the user's override does.
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

    /// Dollars for one model's tokens, or `None` when the model has no price.
    pub fn cost(&self, model: &str, tokens: &TokenTotals) -> Option<f64> {
        self.lookup(model).map(|price| price.cost(tokens))
    }

    /// Whole-session estimate from the per-model totals.
    pub fn estimate(&self, models: &[ModelUsage]) -> CostEstimate {
        let mut estimate = CostEstimate::default();
        for usage in models {
            match self.cost(&usage.model, &usage.tokens) {
                Some(usd) => estimate.usd += usd,
                None => estimate.unpriced_tokens += usage.tokens.total(),
            }
        }
        estimate
    }
}

/// Estimated dollars plus how many tokens could not be priced.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CostEstimate {
    pub usd: f64,
    pub unpriced_tokens: u64,
}

impl CostEstimate {
    /// True when some tokens had no price, so `usd` is only a lower bound.
    pub fn is_lower_bound(&self) -> bool {
        self.unpriced_tokens > 0
    }
}

// ------------------------------------------------------------------- levels

/// One of [`LEVEL_COUNT`] expense steps; `0` is the cheapest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct CostLevel(u8);

impl CostLevel {
    pub const MAX: u8 = (LEVEL_COUNT - 1) as u8;

    pub fn new(index: usize) -> Self {
        Self(index.min(usize::from(Self::MAX)) as u8)
    }

    pub const fn index(self) -> usize {
        self.0 as usize
    }

    /// Single-cell height glyph: the colour-blind-safe carrier of the level.
    pub fn glyph(self) -> char {
        ['▁', '▂', '▄', '▆', '█'][self.index()]
    }

    /// Five-cell fill meter; the level fills `level + 1` cells.
    pub fn meter(self) -> String {
        meter_string(Some(self))
    }
}

/// Five-cell meter; `None` (no data yet) draws an empty track.
pub fn meter_string(level: Option<CostLevel>) -> String {
    let filled = level.map_or(0, |level| level.index() + 1);
    (0..LEVEL_COUNT)
        .map(|cell| if cell < filled { '▰' } else { '▱' })
        .collect()
}

/// Four ascending cut values turning a number into a [`CostLevel`]: a value
/// reaching cut `n` is at least level `n + 1`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LevelScale {
    pub cuts: [f64; LEVEL_COUNT - 1],
}

impl LevelScale {
    pub fn level(&self, value: f64) -> CostLevel {
        CostLevel::new(self.cuts.iter().filter(|cut| value >= **cut).count())
    }
}

/// What the level is computed from: a session's running total, or how fast it
/// is currently spending.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScaleBasis {
    TotalUsd,
    BurnUsdPerHour,
}

/// What is being measured per agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CostMetric {
    /// Estimated API-list-price dollars, for Claude Code and Codex alike.
    #[default]
    Dollars,
    /// Percentage points of the plan's rate-limit window used up while the
    /// agent ran. Only Codex transcripts record plan quota.
    Quota,
}

impl CostMetric {
    pub const ALL: [Self; 2] = [Self::Dollars, Self::Quota];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Dollars => "API dollars",
            Self::Quota => "Plan quota",
        }
    }
}

/// Which Codex rate-limit window the quota metric follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuotaWindow {
    /// The shorter, rolling window.
    #[default]
    Primary,
    /// The longer, weekly window.
    Secondary,
}

impl QuotaWindow {
    pub const ALL: [Self; 2] = [Self::Primary, Self::Secondary];

    /// The window name inside a Codex `rate_limits` record.
    pub const fn key(self) -> &'static str {
        match self {
            Self::Primary => "primary",
            Self::Secondary => "secondary",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Primary => "Short window",
            Self::Secondary => "Long window",
        }
    }
}

/// Which of the five ways of deciding "a lot" is in force.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Calibration {
    /// Fixed dollar cut points.
    FixedBands,
    /// Compared with the median of the agents open right now.
    PeerRelative,
    /// Percentiles of your own past sessions.
    #[default]
    OwnHistory,
    /// Fraction of a per-agent dollar budget.
    Budget,
    /// Current spend rate in dollars per hour.
    BurnRate,
}

impl Calibration {
    pub const ALL: [Self; 5] = [
        Self::FixedBands,
        Self::PeerRelative,
        Self::OwnHistory,
        Self::Budget,
        Self::BurnRate,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::FixedBands => "Fixed bands",
            Self::PeerRelative => "Relative to open agents",
            Self::OwnHistory => "Relative to your history",
            Self::Budget => "Budget",
            Self::BurnRate => "Burn rate",
        }
    }
}

/// The numbers a calibration needs besides the user's own settings.
#[derive(Debug, Clone, Copy)]
pub struct CalibrationInputs<'a> {
    pub fixed_cuts: [f64; LEVEL_COUNT - 1],
    pub burn_cuts: [f64; LEVEL_COUNT - 1],
    /// Per-agent budget in the active metric's unit (dollars or quota percent).
    pub budget: f64,
    /// Totals of every tracked agent right now.
    pub peer_totals: &'a [f64],
    /// Totals of past sessions, sorted ascending; `None` while unscanned.
    pub history_sorted: Option<&'a [f64]>,
}

/// Why a calibration produced the scale it did, for the settings preview.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalibrationNote {
    Ready,
    /// History is still being scanned or is too short; fixed bands stand in.
    HistoryUnavailable,
    /// No open agent has spent anything yet; fixed bands stand in.
    NoPeers,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Calibrated {
    pub scale: LevelScale,
    pub basis: ScaleBasis,
    pub note: CalibrationNote,
}

/// Peer-relative cuts as multiples of the median open agent.
const PEER_MULTIPLES: [f64; LEVEL_COUNT - 1] = [0.25, 0.75, 1.5, 3.0];
/// Budget cuts as fractions of the budget.
const BUDGET_FRACTIONS: [f64; LEVEL_COUNT - 1] = [0.2, 0.4, 0.6, 0.8];
/// History percentiles, as fractions.
const HISTORY_PERCENTILES: [f64; LEVEL_COUNT - 1] = [0.25, 0.50, 0.80, 0.95];

/// Builds the scale for `calibration`. Always returns a usable scale: a
/// calibration without enough data falls back to the fixed bands.
pub fn calibrate(calibration: Calibration, inputs: &CalibrationInputs<'_>) -> Calibrated {
    let fixed = |note| Calibrated {
        scale: LevelScale {
            cuts: inputs.fixed_cuts,
        },
        basis: ScaleBasis::TotalUsd,
        note,
    };
    match calibration {
        Calibration::FixedBands => fixed(CalibrationNote::Ready),
        Calibration::PeerRelative => {
            let mut positive: Vec<f64> = inputs
                .peer_totals
                .iter()
                .copied()
                .filter(|total| *total > 0.0)
                .collect();
            if positive.is_empty() {
                return fixed(CalibrationNote::NoPeers);
            }
            positive.sort_by(f64::total_cmp);
            let median = percentile(&positive, 0.5);
            Calibrated {
                scale: LevelScale {
                    cuts: PEER_MULTIPLES.map(|multiple| median * multiple),
                },
                basis: ScaleBasis::TotalUsd,
                note: CalibrationNote::Ready,
            }
        }
        Calibration::OwnHistory => match inputs.history_sorted {
            Some(history) if history.len() >= MIN_HISTORY_SESSIONS => Calibrated {
                scale: LevelScale {
                    cuts: HISTORY_PERCENTILES.map(|fraction| percentile(history, fraction)),
                },
                basis: ScaleBasis::TotalUsd,
                note: CalibrationNote::Ready,
            },
            _ => fixed(CalibrationNote::HistoryUnavailable),
        },
        Calibration::Budget => Calibrated {
            scale: LevelScale {
                cuts: BUDGET_FRACTIONS.map(|fraction| inputs.budget.max(0.01) * fraction),
            },
            basis: ScaleBasis::TotalUsd,
            note: CalibrationNote::Ready,
        },
        Calibration::BurnRate => Calibrated {
            scale: LevelScale {
                cuts: inputs.burn_cuts,
            },
            basis: ScaleBasis::BurnUsdPerHour,
            note: CalibrationNote::Ready,
        },
    }
}

/// Linear-interpolated percentile of an ascending, non-empty slice.
pub fn percentile(sorted: &[f64], fraction: f64) -> f64 {
    match sorted {
        [] => 0.0,
        [only] => *only,
        _ => {
            let position = fraction.clamp(0.0, 1.0) * (sorted.len() - 1) as f64;
            let lower = position.floor() as usize;
            let upper = position.ceil() as usize;
            let weight = position - lower as f64;
            sorted[lower] + (sorted[upper] - sorted[lower]) * weight
        }
    }
}

/// Share of `sorted` (ascending) at or below `value`, as a percentage.
pub fn percent_rank(sorted: &[f64], value: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let at_or_below = sorted.partition_point(|entry| *entry <= value);
    at_or_below as f64 * 100.0 / sorted.len() as f64
}

/// Fill of a per-agent budget, `1.0` meaning the budget is fully spent.
pub fn budget_fill(total: f64, budget: f64) -> f64 {
    total / budget.max(0.01)
}

// -------------------------------------------------------------- time series

/// Spend that happened at one instant (a minute bucket, in practice).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpendPoint {
    pub at_ms: i64,
    pub usd: f64,
}

/// Sums `points` into `cells` equal slices of the window ending at `now_ms`.
/// Points outside the window are ignored.
pub fn spend_cells(points: &[SpendPoint], now_ms: i64, window_ms: i64, cells: usize) -> Vec<f64> {
    let mut sums = vec![0.0; cells];
    if cells == 0 || window_ms <= 0 {
        return sums;
    }
    let start = now_ms - window_ms;
    for point in points {
        if point.at_ms <= start || point.at_ms > now_ms {
            continue;
        }
        let offset = (point.at_ms - start) as f64 / window_ms as f64;
        let index = ((offset * cells as f64) as usize).min(cells - 1);
        sums[index] += point.usd;
    }
    sums
}

/// Height glyphs for a series scaled to its own maximum. Zero spend is the
/// baseline glyph, so an idle stretch reads as flat rather than missing.
pub fn sparkline_glyphs(values: &[f64]) -> String {
    const GLYPHS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let peak = values.iter().copied().fold(0.0_f64, f64::max);
    values
        .iter()
        .map(|value| {
            if peak <= 0.0 || *value <= 0.0 {
                GLYPHS[0]
            } else {
                let index = ((value / peak) * 7.0).round() as usize;
                GLYPHS[index.clamp(1, 7)]
            }
        })
        .collect()
}

/// Dollars per hour over the last `window_ms`.
pub fn burn_usd_per_hour(points: &[SpendPoint], now_ms: i64, window_ms: i64) -> f64 {
    if window_ms <= 0 {
        return 0.0;
    }
    let start = now_ms - window_ms;
    let spent: f64 = points
        .iter()
        .filter(|point| point.at_ms > start && point.at_ms <= now_ms)
        .map(|point| point.usd)
        .sum();
    spent / (window_ms as f64 / 3_600_000.0)
}

/// Window compared against the baseline when looking for a burn spike.
pub const SPIKE_RECENT_MS: i64 = 10 * 60 * 1000;
/// A dollar spike must also exceed this absolute rate, so pennies never flag.
pub const SPIKE_MIN_USD_PER_HOUR: f64 = 1.0;
/// The same floor for quota: under this many percentage points an hour the
/// plan is not meaningfully being drained.
pub const SPIKE_MIN_QUOTA_PER_HOUR: f64 = 2.0;
const SPIKE_FACTOR: f64 = 2.0;

/// Whether spending in the last ten minutes runs at least twice as fast as
/// over the preceding part of `baseline_window_ms`.
pub fn is_burn_spike(
    points: &[SpendPoint],
    now_ms: i64,
    baseline_window_ms: i64,
    minimum_per_hour: f64,
) -> bool {
    let recent = burn_usd_per_hour(points, now_ms, SPIKE_RECENT_MS);
    if recent < minimum_per_hour {
        return false;
    }
    let baseline_start = now_ms - baseline_window_ms.max(SPIKE_RECENT_MS * 2);
    let baseline_end = now_ms - SPIKE_RECENT_MS;
    let earlier: f64 = points
        .iter()
        .filter(|point| point.at_ms > baseline_start && point.at_ms <= baseline_end)
        .map(|point| point.usd)
        .sum();
    let baseline_hours = (baseline_end - baseline_start) as f64 / 3_600_000.0;
    recent >= SPIKE_FACTOR * (earlier / baseline_hours)
}

// --------------------------------------------------------------- formatting

/// Compact dollar text: `$0.42`, `$4.30`, `$41.0`, `$412`, `$1.2k`. A leading
/// `~` marks a lower bound (some model had no price).
pub fn format_usd(usd: f64, is_lower_bound: bool) -> String {
    let body = if usd < 10.0 {
        format!("${usd:.2}")
    } else if usd < 100.0 {
        format!("${usd:.1}")
    } else if usd < 1000.0 {
        format!("${usd:.0}")
    } else {
        format!("${:.1}k", usd / 1000.0)
    };
    if is_lower_bound {
        format!("~{body}")
    } else {
        body
    }
}

/// Percentage points of plan quota: `0.4%`, `3.2%`, `12%`, `104%`.
pub fn format_quota(points: f64) -> String {
    if points < 10.0 {
        format!("{points:.1}%")
    } else {
        format!("{points:.0}%")
    }
}

/// An amount in the unit of `metric`; `~` marks a dollar lower bound.
pub fn format_amount(metric: CostMetric, value: f64, is_lower_bound: bool) -> String {
    match metric {
        CostMetric::Dollars => format_usd(value, is_lower_bound),
        CostMetric::Quota => format_quota(value),
    }
}

/// A per-hour rate in the unit of `metric`.
pub fn format_rate(metric: CostMetric, value: f64) -> String {
    match metric {
        CostMetric::Dollars => format_usd_per_hour(value),
        CostMetric::Quota => format!("{}/h", format_quota(value)),
    }
}

/// `$/h` figure for the burn rate.
pub fn format_usd_per_hour(usd_per_hour: f64) -> String {
    format!("{}/h", format_usd(usd_per_hour, false))
}

/// `45 min`, `6 h`, `3 d`, `1 d 12 h` for a minute count.
pub fn format_window(minutes: u32) -> String {
    let (days, hours, rest) = (minutes / 1440, minutes % 1440 / 60, minutes % 60);
    match (days, hours, rest) {
        (0, 0, minutes) => format!("{minutes} min"),
        (0, hours, 0) => format!("{hours} h"),
        (0, hours, minutes) => format!("{hours} h {minutes} min"),
        (days, 0, 0) => format!("{days} d"),
        (days, hours, _) => format!("{days} d {hours} h"),
    }
}

/// Largest sparkline window the settings accept: one year.
pub const MAX_WINDOW_MINUTES: u32 = 525_600;

/// Parses a window typed by the user: `90`, `90m`, `6h`, `1.5 h`, `2d`.
/// A bare number is minutes. `None` for anything unparsable or outside one
/// minute to one year.
pub fn parse_window_minutes(text: &str) -> Option<u32> {
    let text = text.trim().to_ascii_lowercase();
    let number_end = text
        .find(|character: char| !(character.is_ascii_digit() || character == '.'))
        .unwrap_or(text.len());
    let (number, unit) = text.split_at(number_end);
    let value: f64 = number.parse().ok()?;
    let minutes_per_unit = match unit.trim() {
        "" | "m" | "min" | "mins" | "minute" | "minutes" => 1.0,
        "h" | "hr" | "hrs" | "hour" | "hours" => 60.0,
        "d" | "day" | "days" => 1440.0,
        _ => return None,
    };
    let minutes = (value * minutes_per_unit).round();
    (minutes.is_finite() && minutes >= 1.0 && minutes <= f64::from(MAX_WINDOW_MINUTES))
        .then_some(minutes as u32)
}

/// Compact token count: `842`, `12.4k`, `1.2M`.
pub fn format_tokens(tokens: u64) -> String {
    match tokens {
        0..=999 => tokens.to_string(),
        1_000..=999_999 => format!("{:.1}k", tokens as f64 / 1_000.0),
        _ => format!("{:.1}M", tokens as f64 / 1_000_000.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(input: u64, cache_read: u64, cache_write: u64, output: u64) -> TokenTotals {
        TokenTotals {
            input,
            cache_read,
            cache_write,
            output,
            reasoning: 0,
        }
    }

    #[test]
    fn cache_reads_cost_a_fraction_of_fresh_input() {
        let table = PriceTable::default();
        let cached = table
            .cost("claude-sonnet-5-5", &tokens(0, 1_000_000, 0, 0))
            .unwrap();
        let fresh = table
            .cost("claude-sonnet-5-5", &tokens(1_000_000, 0, 0, 0))
            .unwrap();
        assert!((cached - 0.20).abs() < 1e-9);
        assert!((fresh - 2.0).abs() < 1e-9);
    }

    #[test]
    fn longest_prefix_wins_and_dated_snapshots_match_their_family() {
        let table = PriceTable::default();
        assert_eq!(table.lookup("claude-opus-5-5").unwrap().input, 4.0);
        assert_eq!(table.lookup("claude-opus-5").unwrap().input, 5.0);
        assert_eq!(
            table.lookup("claude-haiku-4-5-20251001").unwrap().input,
            1.0
        );
        assert!(table.lookup("gpt-reserve").is_none());
        assert!(table.lookup("unknown").is_none());
    }

    #[test]
    fn overrides_beat_built_in_prices() {
        let mut overrides = BTreeMap::new();
        overrides.insert(
            "GPT-Reserve".to_owned(),
            ModelPrice::new(1.0, 2.0, 0.1, 0.0),
        );
        overrides.insert(
            "claude-sonnet-5".to_owned(),
            ModelPrice::new(9.0, 9.0, 9.0, 9.0),
        );
        overrides.insert(
            "claude-haiku-4-5".to_owned(),
            ModelPrice::new(7.0, 7.0, 7.0, 7.0),
        );
        let table = PriceTable::with_overrides(&overrides);
        assert_eq!(table.lookup("gpt-reserve").unwrap().output, 2.0);
        // A built-in row with a longer (more specific) prefix beats a
        // shorter override; the same prefix length goes to the override.
        assert_eq!(table.lookup("claude-sonnet-5-5").unwrap().input, 2.0);
        assert_eq!(table.lookup("claude-sonnet-5").unwrap().input, 9.0);
        assert_eq!(
            table.lookup("claude-haiku-4-5-20251001").unwrap().input,
            7.0
        );
    }

    #[test]
    fn unpriced_models_make_the_estimate_a_lower_bound() {
        let models = vec![
            ModelUsage {
                model: "claude-haiku-4-5".into(),
                calls: 1,
                tokens: tokens(1_000_000, 0, 0, 0),
            },
            ModelUsage {
                model: "gpt-reserve".into(),
                calls: 1,
                tokens: tokens(500, 0, 0, 500),
            },
        ];
        let estimate = PriceTable::default().estimate(&models);
        assert!((estimate.usd - 1.0).abs() < 1e-9);
        assert_eq!(estimate.unpriced_tokens, 1_000);
        assert!(estimate.is_lower_bound());
    }

    #[test]
    fn scale_counts_reached_cuts() {
        let scale = LevelScale {
            cuts: [1.0, 5.0, 20.0, 50.0],
        };
        assert_eq!(scale.level(0.0).index(), 0);
        assert_eq!(scale.level(0.99).index(), 0);
        assert_eq!(scale.level(1.0).index(), 1);
        assert_eq!(scale.level(19.9).index(), 2);
        assert_eq!(scale.level(49.0).index(), 3);
        assert_eq!(scale.level(10_000.0).index(), 4);
    }

    #[test]
    fn percentile_interpolates() {
        let sorted = [1.0, 2.0, 3.0, 4.0, 5.0];
        assert_eq!(percentile(&sorted, 0.0), 1.0);
        assert_eq!(percentile(&sorted, 0.5), 3.0);
        assert!((percentile(&sorted, 0.25) - 2.0).abs() < 1e-9);
        assert_eq!(percentile(&sorted, 1.0), 5.0);
        assert_eq!(percentile(&[], 0.5), 0.0);
        assert_eq!(percentile(&[7.0], 0.9), 7.0);
    }

    fn inputs<'a>(peers: &'a [f64], history: Option<&'a [f64]>) -> CalibrationInputs<'a> {
        CalibrationInputs {
            fixed_cuts: [1.0, 5.0, 20.0, 50.0],
            burn_cuts: [0.5, 2.0, 5.0, 15.0],
            budget: 10.0,
            peer_totals: peers,
            history_sorted: history,
        }
    }

    #[test]
    fn history_calibration_uses_percentiles_once_enough_sessions_exist() {
        let history: Vec<f64> = (1..=20).map(f64::from).collect();
        let calibrated = calibrate(Calibration::OwnHistory, &inputs(&[], Some(&history)));
        assert_eq!(calibrated.note, CalibrationNote::Ready);
        assert_eq!(calibrated.basis, ScaleBasis::TotalUsd);
        assert!(calibrated.scale.level(3.0).index() == 0);
        assert!(calibrated.scale.level(19.5).index() == 4);

        let thin = calibrate(Calibration::OwnHistory, &inputs(&[], Some(&history[..3])));
        assert_eq!(thin.note, CalibrationNote::HistoryUnavailable);
        assert_eq!(thin.scale.cuts, [1.0, 5.0, 20.0, 50.0]);
        let missing = calibrate(Calibration::OwnHistory, &inputs(&[], None));
        assert_eq!(missing.note, CalibrationNote::HistoryUnavailable);
    }

    #[test]
    fn peer_calibration_scales_with_the_median_open_agent() {
        let calibrated = calibrate(
            Calibration::PeerRelative,
            &inputs(&[2.0, 4.0, 6.0, 0.0], None),
        );
        assert_eq!(calibrated.scale.cuts, [1.0, 3.0, 6.0, 12.0]);
        let alone = calibrate(Calibration::PeerRelative, &inputs(&[0.0], None));
        assert_eq!(alone.note, CalibrationNote::NoPeers);
    }

    #[test]
    fn budget_and_burn_calibrations_use_their_own_basis() {
        let budget = calibrate(Calibration::Budget, &inputs(&[], None));
        assert_eq!(budget.scale.cuts, [2.0, 4.0, 6.0, 8.0]);
        assert_eq!(budget.basis, ScaleBasis::TotalUsd);
        let burn = calibrate(Calibration::BurnRate, &inputs(&[], None));
        assert_eq!(burn.basis, ScaleBasis::BurnUsdPerHour);
        assert_eq!(burn.scale.cuts, [0.5, 2.0, 5.0, 15.0]);
        assert!((budget_fill(12.5, 10.0) - 1.25).abs() < 1e-9);
    }

    #[test]
    fn percent_rank_counts_sessions_at_or_below() {
        let sorted = [1.0, 2.0, 3.0, 4.0];
        assert_eq!(percent_rank(&sorted, 0.5), 0.0);
        assert_eq!(percent_rank(&sorted, 2.0), 50.0);
        assert_eq!(percent_rank(&sorted, 99.0), 100.0);
        assert_eq!(percent_rank(&[], 1.0), 0.0);
    }

    #[test]
    fn meter_fills_one_cell_per_level_and_empty_without_data() {
        assert_eq!(CostLevel::new(0).meter(), "▰▱▱▱▱");
        assert_eq!(CostLevel::new(2).meter(), "▰▰▰▱▱");
        assert_eq!(CostLevel::new(99).meter(), "▰▰▰▰▰");
        assert_eq!(meter_string(None), "▱▱▱▱▱");
        assert_eq!(CostLevel::new(0).glyph(), '▁');
        assert_eq!(CostLevel::new(4).glyph(), '█');
    }

    #[test]
    fn spend_cells_bucket_the_window_and_ignore_outside_points() {
        let hour = 3_600_000;
        let now = 10 * hour;
        let points = [
            SpendPoint {
                at_ms: now - 30 * 60_000,
                usd: 1.0,
            },
            SpendPoint {
                at_ms: now - 5 * 60_000,
                usd: 2.0,
            },
            SpendPoint {
                at_ms: now - 5 * hour,
                usd: 99.0,
            },
        ];
        let cells = spend_cells(&points, now, hour, 4);
        assert_eq!(cells, vec![0.0, 0.0, 1.0, 2.0]);
        assert_eq!(spend_cells(&points, now, 0, 4), vec![0.0; 4]);
        assert!(spend_cells(&points, now, hour, 0).is_empty());
    }

    #[test]
    fn sparkline_scales_to_the_series_peak() {
        assert_eq!(sparkline_glyphs(&[0.0, 1.0, 2.0, 4.0, 8.0]), "▁▂▃▅█");
        assert_eq!(sparkline_glyphs(&[0.0, 0.0]), "▁▁");
        assert_eq!(sparkline_glyphs(&[]), "");
        assert_eq!(sparkline_glyphs(&[0.001, 100.0]), "▂█");
    }

    #[test]
    fn burn_rate_and_spike_detection() {
        let minute = 60_000;
        let now = 1_000 * minute;
        // $0.50 per minute for the last ten minutes = $30/h; nothing before.
        let hot: Vec<SpendPoint> = (0..10)
            .map(|offset| SpendPoint {
                at_ms: now - offset * minute,
                usd: 0.5,
            })
            .collect();
        assert!((burn_usd_per_hour(&hot, now, SPIKE_RECENT_MS) - 30.0).abs() < 1e-6);
        assert!(is_burn_spike(
            &hot,
            now,
            360 * minute,
            SPIKE_MIN_USD_PER_HOUR
        ));

        // The same rate sustained for hours is the baseline, not a spike.
        let steady: Vec<SpendPoint> = (0..360)
            .map(|offset| SpendPoint {
                at_ms: now - offset * minute,
                usd: 0.5,
            })
            .collect();
        assert!(!is_burn_spike(
            &steady,
            now,
            360 * minute,
            SPIKE_MIN_USD_PER_HOUR
        ));

        // Pennies never flag however large the ratio.
        let tiny = [SpendPoint {
            at_ms: now,
            usd: 0.01,
        }];
        assert!(!is_burn_spike(
            &tiny,
            now,
            360 * minute,
            SPIKE_MIN_USD_PER_HOUR
        ));
    }

    #[test]
    fn amounts_format_in_the_unit_of_their_metric() {
        assert_eq!(format_amount(CostMetric::Dollars, 4.3, false), "$4.30");
        assert_eq!(format_amount(CostMetric::Dollars, 4.3, true), "~$4.30");
        assert_eq!(format_amount(CostMetric::Quota, 0.43, true), "0.4%");
        assert_eq!(format_amount(CostMetric::Quota, 12.4, false), "12%");
        assert_eq!(format_rate(CostMetric::Dollars, 3.0), "$3.00/h");
        assert_eq!(format_rate(CostMetric::Quota, 1.5), "1.5%/h");
        assert_eq!(QuotaWindow::Primary.key(), "primary");
        assert_eq!(QuotaWindow::Secondary.key(), "secondary");
    }

    #[test]
    fn typed_windows_parse_units_and_reject_nonsense() {
        assert_eq!(parse_window_minutes("90"), Some(90));
        assert_eq!(parse_window_minutes(" 90m "), Some(90));
        assert_eq!(parse_window_minutes("6h"), Some(360));
        assert_eq!(parse_window_minutes("1.5 h"), Some(90));
        assert_eq!(parse_window_minutes("2 days"), Some(2880));
        assert_eq!(parse_window_minutes("0"), None);
        assert_eq!(parse_window_minutes("400d"), None);
        assert_eq!(parse_window_minutes("six"), None);
        assert_eq!(parse_window_minutes("5x"), None);
        assert_eq!(parse_window_minutes(""), None);
    }

    #[test]
    fn window_formatting_is_compact() {
        assert_eq!(format_window(5), "5 min");
        assert_eq!(format_window(90), "1 h 30 min");
        assert_eq!(format_window(360), "6 h");
        assert_eq!(format_window(1440), "1 d");
        assert_eq!(format_window(2160), "1 d 12 h");
        assert_eq!(format_window(43_200), "30 d");
    }

    #[test]
    fn dollar_and_token_formatting_is_compact() {
        assert_eq!(format_usd(0.0, false), "$0.00");
        assert_eq!(format_usd(4.3, false), "$4.30");
        assert_eq!(format_usd(41.04, false), "$41.0");
        assert_eq!(format_usd(412.6, false), "$413");
        assert_eq!(format_usd(1234.0, false), "$1.2k");
        assert_eq!(format_usd(1.0, true), "~$1.00");
        assert_eq!(format_usd_per_hour(3.1), "$3.10/h");
        assert_eq!(format_tokens(842), "842");
        assert_eq!(format_tokens(12_400), "12.4k");
        assert_eq!(format_tokens(1_200_000), "1.2M");
    }
}

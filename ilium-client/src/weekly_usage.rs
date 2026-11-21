//! Agent usage attribution over a selectable time range.

use crate::session_stats::{SpendBucket, TokenTotals};

/// How far back the usage view looks. Account quotas keep their own fixed
/// weekly window; this only selects the span of locally observed work shown.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UsageRange {
    Hours6,
    Hours12,
    Day1,
    Days3,
    #[default]
    Days7,
    Days14,
    Days30,
}

impl UsageRange {
    pub const ALL: [UsageRange; 7] = [
        Self::Hours6,
        Self::Hours12,
        Self::Day1,
        Self::Days3,
        Self::Days7,
        Self::Days14,
        Self::Days30,
    ];
    /// Ownership history must reach back at least as far as the longest range.
    pub const LONGEST_MS: i64 = 30 * 24 * 60 * 60 * 1000;

    pub fn label(self) -> &'static str {
        match self {
            Self::Hours6 => "6h",
            Self::Hours12 => "12h",
            Self::Day1 => "1d",
            Self::Days3 => "3d",
            Self::Days7 => "7d",
            Self::Days14 => "14d",
            Self::Days30 => "1m",
        }
    }
    pub fn duration_ms(self) -> i64 {
        const HOUR: i64 = 60 * 60 * 1000;
        match self {
            Self::Hours6 => 6 * HOUR,
            Self::Hours12 => 12 * HOUR,
            Self::Day1 => 24 * HOUR,
            Self::Days3 => 72 * HOUR,
            Self::Days7 => 168 * HOUR,
            Self::Days14 => 336 * HOUR,
            Self::Days30 => 720 * HOUR,
        }
    }
    fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|range| *range == self)
            .unwrap_or(0)
    }
    pub fn previous(self) -> Self {
        Self::ALL[self.index().saturating_sub(1)]
    }
    pub fn next(self) -> Self {
        Self::ALL[(self.index() + 1).min(Self::ALL.len() - 1)]
    }
}

/// The first Codex delta after a boundary may span work before ownership,
/// even when an earlier counter exists. Exclude that whole minute to establish
/// a baseline inside the ownership period before attributing later deltas.
pub(crate) fn codex_window_start(buckets: &[SpendBucket], start_ms: i64) -> (i64, bool) {
    let first_minute = buckets
        .iter()
        .map(|bucket| bucket.minute_ms)
        .filter(|minute| minute.saturating_add(60_000) > start_ms)
        .min();
    match first_minute {
        Some(minute) => (minute.saturating_add(60_000), true),
        _ => (start_ms, false),
    }
}

/// Sum only whole minute buckets contained within the account's weekly window.
/// Returns the included tokens and whether a bucket touched a window boundary.
pub fn weekly_tokens_for_model(
    buckets: &[SpendBucket],
    model: &str,
    window_start_ms: i64,
    window_end_ms: i64,
) -> (TokenTotals, bool) {
    let mut totals = TokenTotals::default();
    let mut partial = false;
    for bucket in buckets
        .iter()
        .filter(|bucket| bucket.model.as_ref() == model)
    {
        let end = bucket.minute_ms.saturating_add(60_000);
        if bucket.minute_ms >= window_end_ms || end <= window_start_ms {
            continue;
        }
        if bucket.minute_ms < window_start_ms || end > window_end_ms {
            partial = true;
            continue;
        }
        totals.input = totals.input.saturating_add(bucket.tokens.input);
        totals.cache_read = totals.cache_read.saturating_add(bucket.tokens.cache_read);
        totals.cache_write = totals.cache_write.saturating_add(bucket.tokens.cache_write);
        totals.output = totals.output.saturating_add(bucket.tokens.output);
        totals.reasoning = totals.reasoning.saturating_add(bucket.tokens.reasoning);
    }
    (totals, partial)
}

/// A model's transcript-derived workload. The weight is an API-price proxy,
/// not a subscription quota coefficient; unknown models remain unweighted.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelWorkload {
    pub model: String,
    pub tokens: TokenTotals,
    pub relative_units: Option<f64>,
    pub boundary_partial: bool,
    /// Usage linked to the session but dated before the pane's ownership was
    /// observed: a historical estimate, not verified pane ownership.
    pub historical: bool,
}

/// Preserve distinct model identities and token categories, including models
/// for which the price table has no supported weighting.
pub fn model_workloads(
    buckets: &[SpendBucket],
    start_ms: i64,
    end_ms: i64,
    prices: &crate::cost_model::PriceTable,
) -> (Vec<ModelWorkload>, bool) {
    let mut truncated = false;
    let mut models: std::collections::BTreeMap<&str, (TokenTotals, bool)> =
        std::collections::BTreeMap::new();
    for bucket in buckets {
        let end = bucket.minute_ms.saturating_add(60_000);
        if bucket.minute_ms >= end_ms || end <= start_ms {
            continue;
        }
        let model = if bucket.model.trim().is_empty() {
            "(unknown model)"
        } else {
            bucket.model.as_ref()
        };
        if model.len() > 512 {
            truncated = true;
            continue;
        }
        if !models.contains_key(model) && models.len() >= 256 {
            truncated = true;
            let Some(largest) = models.keys().next_back().copied() else {
                continue;
            };
            if model >= largest {
                continue;
            }
            models.remove(largest);
        }
        let (tokens, partial) = models.entry(model).or_default();
        if bucket.minute_ms < start_ms || end > end_ms {
            *partial = true;
            continue;
        }
        tokens.input = tokens.input.saturating_add(bucket.tokens.input);
        tokens.cache_read = tokens.cache_read.saturating_add(bucket.tokens.cache_read);
        tokens.cache_write = tokens.cache_write.saturating_add(bucket.tokens.cache_write);
        tokens.output = tokens.output.saturating_add(bucket.tokens.output);
        tokens.reasoning = tokens.reasoning.saturating_add(bucket.tokens.reasoning);
    }
    let result = models
        .into_iter()
        .map(|(model, (tokens, boundary_partial))| ModelWorkload {
            model: model.to_owned(),
            tokens,
            relative_units: prices.cost(model, &tokens),
            boundary_partial,
            historical: false,
        })
        .collect();
    (result, truncated)
}

/// Provider identity is part of every quota-window key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsageProvider {
    Claude,
    Codex,
}

/// An account-wide weekly observation; identical readings from different
/// sessions describe the same counter, not independent consumption.
#[derive(Debug, Clone, PartialEq)]
pub struct QuotaObservation {
    pub provider: UsageProvider,
    pub account: String,
    pub reset_at_ms: i64,
    pub observed_at_ms: i64,
    pub used_percent: f64,
}

/// Return one increase only when both observations identify the same window,
/// are ordered, and contain valid nondecreasing readings.
pub fn account_quota_increase(
    previous: &QuotaObservation,
    current: &QuotaObservation,
) -> Option<f64> {
    if previous.account.is_empty()
        || previous.provider != current.provider
        || previous.account != current.account
        || previous.reset_at_ms != current.reset_at_ms
        || previous.observed_at_ms <= 0
        || previous.observed_at_ms < previous.reset_at_ms.saturating_sub(7 * 24 * 60 * 60 * 1000)
        || previous.observed_at_ms >= current.observed_at_ms
        || current.observed_at_ms >= current.reset_at_ms
        || !previous.used_percent.is_finite()
        || !current.used_percent.is_finite()
        || !(0.0..=100.0).contains(&previous.used_percent)
        || !(0.0..=100.0).contains(&current.used_percent)
        || current.used_percent < previous.used_percent
    {
        return None;
    }
    Some(current.used_percent - previous.used_percent)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn weekly_quota_increase_requires_same_account_and_window() {
        let first = QuotaObservation {
            provider: UsageProvider::Claude,
            account: "account-a".into(),
            reset_at_ms: 100_000,
            observed_at_ms: 1_000,
            used_percent: 10.0,
        };
        let mut next = first.clone();
        next.observed_at_ms = 2_000;
        next.used_percent = 13.0;
        assert_eq!(account_quota_increase(&first, &next), Some(3.0));
        next.account = "account-b".into();
        assert_eq!(account_quota_increase(&first, &next), None);
        next.account = first.account.clone();
        next.reset_at_ms += 1;
        assert_eq!(account_quota_increase(&first, &next), None);
        let mut before_window = first.clone();
        before_window.reset_at_ms = 800_000_000;
        next = before_window.clone();
        next.observed_at_ms = 200_000_000;
        next.used_percent = 13.0;
        assert_eq!(account_quota_increase(&before_window, &next), None);
    }

    #[test]
    fn calibration_rejects_mixed_accounts_and_overlapping_intervals() {
        let mut intervals: Vec<_> = (0..8)
            .map(|index| CalibrationInterval {
                provider: UsageProvider::Claude,
                account: "same".into(),
                reset_at_ms: 10_000,
                start_ms: index * 100,
                end_ms: index * 100 + 100,
                feature_keys: vec![FeatureKey {
                    model: "a".into(),
                    category: TokenCategory::FreshInput,
                }],
                features: vec![(index + 1) as f64],
                quota_increase: (index + 1) as f64,
                complete: true,
            })
            .collect();
        assert!(fit_calibration(&intervals).is_some());
        intervals[1].account = "different".into();
        assert!(fit_calibration(&intervals).is_none());
        intervals[1].account = "same".into();
        intervals[1].start_ms = 50;
        assert!(fit_calibration(&intervals).is_none());
        intervals[1].start_ms = 100;
        for interval in &mut intervals {
            interval.reset_at_ms = 800_000_000;
        }
        assert!(fit_calibration(&intervals).is_none());
    }

    #[test]
    fn quota_reconciliation_rejects_decrease_and_invalid_values() {
        let previous = QuotaObservation {
            provider: UsageProvider::Claude,
            account: "a".into(),
            reset_at_ms: 100_000,
            observed_at_ms: 1_000,
            used_percent: 10.0,
        };
        let mut current = previous.clone();
        current.observed_at_ms = 2_000;
        for invalid in [9.0, f64::NAN, f64::INFINITY, 101.0, -1.0] {
            current.used_percent = invalid;
            assert_eq!(account_quota_increase(&previous, &current), None);
        }
        current.used_percent = 11.0;
        current.observed_at_ms = 1_000;
        assert_eq!(account_quota_increase(&previous, &current), None);
        current.observed_at_ms = 100_000;
        assert_eq!(account_quota_increase(&previous, &current), None);
    }

    #[test]
    fn workload_keeps_unknown_model_unweighted() {
        let buckets = [SpendBucket {
            minute_ms: 60_000,
            model: Arc::from("unknown-model"),
            tokens: TokenTotals {
                input: 7,
                ..TokenTotals::default()
            },
        }];
        let (models, truncated) = model_workloads(
            &buckets,
            0,
            120_000,
            &crate::cost_model::PriceTable::default(),
        );
        assert!(!truncated);
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].tokens.input, 7);
        assert_eq!(models[0].relative_units, None);
    }

    #[test]
    fn weekly_totals_keep_models_and_token_categories_separate() {
        let tokens = TokenTotals {
            input: 7,
            cache_read: 11,
            cache_write: 13,
            output: 17,
            reasoning: 5,
        };
        let buckets = [
            SpendBucket {
                minute_ms: 60_000,
                model: Arc::from("model-a"),
                tokens,
            },
            SpendBucket {
                minute_ms: 60_000,
                model: Arc::from("model-b"),
                tokens,
            },
        ];
        assert_eq!(
            weekly_tokens_for_model(&buckets, "model-a", 0, 120_000),
            (tokens, false)
        );
        assert_eq!(
            weekly_tokens_for_model(&buckets, "missing", 0, 120_000),
            (TokenTotals::default(), false)
        );
    }

    #[test]
    fn calibration_needs_independent_complete_intervals_and_validates_holdout() {
        let mut intervals: Vec<_> = (0..16)
            .map(|index| {
                let a = 1.0 + (index % 3) as f64;
                let b = 1.0 + (index % 5) as f64;
                CalibrationInterval {
                    provider: UsageProvider::Claude,
                    account: "account-a".into(),
                    reset_at_ms: 10_000,
                    start_ms: index * 100,
                    end_ms: index * 100 + 100,
                    feature_keys: vec![
                        FeatureKey {
                            model: "a".into(),
                            category: TokenCategory::FreshInput,
                        },
                        FeatureKey {
                            model: "a".into(),
                            category: TokenCategory::Output,
                        },
                    ],
                    features: vec![a, b],
                    quota_increase: a * 0.2 + b * 0.5,
                    complete: true,
                }
            })
            .collect();
        let fit = fit_calibration(&intervals).unwrap();
        assert!((fit.coefficients[0] - 0.2).abs() < 0.001);
        assert!((fit.coefficients[1] - 0.5).abs() < 0.001);
        let mut at_reset = intervals.clone();
        at_reset[15].end_ms = at_reset[15].reset_at_ms;
        assert!(fit_calibration(&at_reset).is_none());
        intervals[15].quota_increase *= 20.0;
        assert!(fit_calibration(&intervals).is_none());
        intervals[15].complete = false;
        assert!(fit_calibration(&intervals).is_none());
        for item in &mut intervals {
            item.complete = true;
            item.features[1] = item.features[0];
        }
        assert!(fit_calibration(&intervals).is_none());
    }

    #[test]
    fn quota_projection_requires_a_valid_coefficient_for_each_nonzero_category() {
        let window = ProviderAccountWindow {
            provider: UsageProvider::Claude,
            account: "a".into(),
            reset_at_ms: 10_000,
        };
        let calibration = ValidatedCalibration {
            window: window.clone(),
            version: 1,
            validated_at_ms: 1000,
            feature_keys: vec![FeatureKey {
                model: "a".into(),
                category: TokenCategory::FreshInput,
            }],
            coefficients: vec![0.1],
            training_intervals: 6,
            validation_intervals: 2,
            validation_error: 0.0,
        };
        let mut tokens = TokenTotals {
            input: 10,
            ..TokenTotals::default()
        };
        assert_eq!(
            estimate_model_quota(&calibration, &window, "a", &tokens),
            Some(1.0)
        );
        let mut other_window = window.clone();
        other_window.reset_at_ms += 1;
        assert_eq!(
            estimate_model_quota(&calibration, &other_window, "a", &tokens),
            None
        );
        tokens.cache_read = 1;
        assert_eq!(
            estimate_model_quota(&calibration, &window, "a", &tokens),
            None
        );
        assert_eq!(
            estimate_model_quota(&calibration, &window, "unknown", &tokens),
            None
        );
    }

    #[test]
    fn usage_ranges_are_ordered_labelled_and_clamped() {
        let spans: Vec<i64> = UsageRange::ALL.iter().map(|r| r.duration_ms()).collect();
        assert!(spans.windows(2).all(|pair| pair[0] < pair[1]));
        assert_eq!(UsageRange::Days30.duration_ms(), UsageRange::LONGEST_MS);
        assert_eq!(UsageRange::default().label(), "7d");
        assert_eq!(UsageRange::Hours6.previous(), UsageRange::Hours6);
        assert_eq!(UsageRange::Days30.next(), UsageRange::Days30);
        assert_eq!(UsageRange::Day1.next(), UsageRange::Days3);
    }

    #[test]
    fn bars_use_a_common_scale_and_do_not_inflate_tiny_segments() {
        assert_eq!(allocate_bar_cells(&[20.0, 10.0], 100.0, 10), vec![2, 1]);
        assert_eq!(allocate_bar_cells(&[0.01], 100.0, 10), vec![0]);
        assert_eq!(allocate_bar_cells(&[40.0, 80.0], 100.0, 9), vec![3, 6]);
        assert_eq!(allocate_bar_cells(&[1.0, 1.0, 1.0], 3.0, 2), vec![1, 1, 0]);
        assert_eq!(allocate_bar_cells(&[1.0], 0.0, 10), vec![0]);
    }

    #[test]
    fn weekly_totals_exclude_a_minute_straddling_the_reset() {
        let bucket = |minute_ms, input| SpendBucket {
            minute_ms,
            model: Arc::from("model-a"),
            tokens: TokenTotals {
                input,
                ..TokenTotals::default()
            },
        };
        let buckets = [bucket(0, 10), bucket(60_000, 20), bucket(120_000, 30)];
        let (tokens, partial) = weekly_tokens_for_model(&buckets, "model-a", 30_000, 180_000);
        assert_eq!(tokens.input, 50);
        assert!(partial);
    }

    #[test]
    fn codex_first_delta_after_ownership_requires_an_inside_baseline() {
        let bucket = |minute_ms| SpendBucket {
            minute_ms,
            model: Arc::from("model-a"),
            tokens: TokenTotals::default(),
        };
        assert_eq!(
            codex_window_start(&[bucket(120_000)], 60_000),
            (180_000, true)
        );
        assert_eq!(
            codex_window_start(&[bucket(0), bucket(120_000)], 60_000),
            (180_000, true)
        );
        assert_eq!(
            codex_window_start(&[bucket(0), bucket(60_000), bucket(120_000)], 90_000),
            (120_000, true)
        );
        assert_eq!(codex_window_start(&[bucket(0)], 60_000), (60_000, false));
        assert_eq!(codex_window_start(&[], 60_000), (60_000, false));
    }

    #[test]
    fn codex_delta_spanning_ownership_is_not_attributed() {
        // Cumulative readings of 100 at minute zero and 500 at minute five
        // produce a delta of 400 that cannot be split at ownership minute two.
        let bucket = |minute_ms, input| SpendBucket {
            minute_ms,
            model: Arc::from("model-a"),
            tokens: TokenTotals {
                input,
                ..TokenTotals::default()
            },
        };
        let buckets = [bucket(0, 100), bucket(300_000, 400), bucket(360_000, 50)];
        let (start, excluded) = codex_window_start(&buckets, 120_000);
        let (tokens, _) = weekly_tokens_for_model(&buckets, "model-a", start, 420_000);
        assert!(excluded);
        assert_eq!(tokens.input, 50);
    }
}

/// Distribute occupied cells deterministically across segments on one common
/// scale. Fractional remainders break ties by model order; values beyond the
/// scale are proportionally capped and must be labelled by the caller.
pub fn allocate_bar_cells(values: &[f64], scale: f64, width: usize) -> Vec<usize> {
    let mut cells = vec![0; values.len()];
    if width == 0 || !scale.is_finite() || scale <= 0.0 {
        return cells;
    }
    let clean: Vec<f64> = values
        .iter()
        .map(|value| {
            if value.is_finite() && *value > 0.0 {
                *value
            } else {
                0.0
            }
        })
        .collect();
    let total: f64 = clean.iter().sum();
    if !total.is_finite() || total <= 0.0 {
        return cells;
    }
    let divisor = scale.max(total);
    let target = ((total / divisor) * width as f64).floor() as usize;
    let mut remainders = Vec::with_capacity(values.len());
    for (index, value) in clean.iter().enumerate() {
        let exact = value / divisor * width as f64;
        cells[index] = exact.floor() as usize;
        remainders.push((index, exact - cells[index] as f64));
    }
    remainders.sort_by(|(left_index, left), (right_index, right)| {
        right
            .total_cmp(left)
            .then_with(|| left_index.cmp(right_index))
    });
    let remaining = target.saturating_sub(cells.iter().sum());
    for (index, _) in remainders.into_iter().take(remaining) {
        cells[index] += 1;
    }
    cells
}

/// A calibration column identifies both model and normalized token category.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct FeatureKey {
    pub model: String,
    pub category: TokenCategory,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TokenCategory {
    FreshInput,
    CacheRead,
    CacheWrite,
    Output,
}

/// A weekly reset generation for one verified provider account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderAccountWindow {
    pub provider: UsageProvider,
    pub account: String,
    pub reset_at_ms: i64,
}

/// A non-overlapping account interval whose predictors cover every known
/// contributor. Callers must exclude intervals with ambiguous ownership,
/// incomplete features or external activity before fitting.
#[derive(Debug, Clone)]
pub struct CalibrationInterval {
    pub provider: UsageProvider,
    pub account: String,
    pub reset_at_ms: i64,
    pub start_ms: i64,
    pub end_ms: i64,
    pub feature_keys: Vec<FeatureKey>,
    pub features: Vec<f64>,
    pub quota_increase: f64,
    pub complete: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ValidatedCalibration {
    pub window: ProviderAccountWindow,
    pub version: u32,
    pub validated_at_ms: i64,
    pub feature_keys: Vec<FeatureKey>,
    pub coefficients: Vec<f64>,
    pub training_intervals: usize,
    pub validation_intervals: usize,
    pub validation_error: f64,
}

/// Fit non-negative coefficients only when independent, complete intervals
/// support them and the chronological holdout predicts within 20% absolute
/// error relative to the holdout's total observed increase. Zero or rounded
/// observations cannot supply useful training evidence.
pub fn fit_calibration(intervals: &[CalibrationInterval]) -> Option<ValidatedCalibration> {
    if intervals.len() > 4096
        || intervals.iter().any(|interval| {
            interval.features.len() > 128
                || interval.feature_keys.len() > 128
                || interval.account.len() > 1024
                || interval
                    .feature_keys
                    .iter()
                    .any(|key| key.model.len() > 512)
        })
    {
        return None;
    }
    let mut ordered = intervals.to_vec();
    ordered.sort_by_key(|interval| (interval.start_ms, interval.end_ms));
    let columns = ordered.first()?.features.len();
    let feature_keys = &ordered[0].feature_keys;
    if columns > 128
        || feature_keys.len() != columns
        || feature_keys
            .iter()
            .any(|key| key.model.is_empty() || key.model.len() > 512)
        || feature_keys.windows(2).any(|pair| pair[0] >= pair[1])
    {
        return None;
    }
    if columns == 0 || ordered.len() < columns.saturating_mul(3).saturating_add(3) {
        return None;
    }
    let provider = ordered[0].provider;
    let account = &ordered[0].account;
    let reset_at_ms = ordered[0].reset_at_ms;
    if account.is_empty() {
        return None;
    }
    if ordered.iter().any(|interval| {
        interval.provider != provider
            || interval.account != *account
            || interval.reset_at_ms != reset_at_ms
            || interval.start_ms < reset_at_ms.saturating_sub(7 * 24 * 60 * 60 * 1000)
            || interval.end_ms >= reset_at_ms
            || !interval.complete
            || interval.start_ms >= interval.end_ms
            || interval.feature_keys != *feature_keys
            || interval.features.len() != columns
            || !interval.quota_increase.is_finite()
            || interval.quota_increase <= 0.0
            || interval
                .features
                .iter()
                .any(|value| !value.is_finite() || *value < 0.0)
    }) || ordered
        .windows(2)
        .any(|pair| pair[0].end_ms > pair[1].start_ms)
    {
        return None;
    }
    let train_len = ordered.len() * 3 / 4;
    let (training, validation) = ordered.split_at(train_len);
    if training.len() < columns * 3 || validation.len() < 2 {
        return None;
    }
    // Scale columns before checking rank and fitting so token magnitude does
    // not decide identifiability. A zero column is unresolved, never guessed.
    let scales: Vec<f64> = (0..columns)
        .map(|column| {
            training
                .iter()
                .map(|interval| interval.features[column].powi(2))
                .sum::<f64>()
                .sqrt()
        })
        .collect();
    if scales
        .iter()
        .any(|scale| !scale.is_finite() || *scale <= f64::EPSILON)
    {
        return None;
    }
    let mut gram = vec![vec![0.0; columns]; columns];
    let mut target = vec![0.0; columns];
    for interval in training {
        for row in 0..columns {
            let value = interval.features[row] / scales[row];
            target[row] += value * interval.quota_increase;
            for col in 0..columns {
                gram[row][col] += value * interval.features[col] / scales[col];
            }
        }
    }
    let mut rank_check = gram.clone();
    for column in 0..columns {
        let pivot = (column..columns).max_by(|left, right| {
            rank_check[*left][column]
                .abs()
                .total_cmp(&rank_check[*right][column].abs())
        })?;
        if rank_check[pivot][column].abs() < 1e-8 {
            return None;
        }
        rank_check.swap(column, pivot);
        for row in column + 1..columns {
            let factor = rank_check[row][column] / rank_check[column][column];
            let (earlier, later) = rank_check.split_at_mut(row);
            for (value, pivot_value) in later[0][column..]
                .iter_mut()
                .zip(&earlier[column][column..])
            {
                *value -= factor * pivot_value;
            }
        }
    }
    // Coordinate descent on a lightly regularized non-negative least-squares
    // objective; deterministic column order and fixed tolerance.
    let mut weights = vec![0.0; columns];
    let mut converged = false;
    for _ in 0..10_000 {
        let mut change: f64 = 0.0;
        for column in 0..columns {
            let other: f64 = (0..columns)
                .filter(|other| *other != column)
                .map(|other| gram[column][other] * weights[other])
                .sum();
            let next = ((target[column] - other) / (gram[column][column] + 1e-10)).max(0.0);
            change = change.max((next - weights[column]).abs());
            weights[column] = next;
        }
        if change < 1e-10 {
            converged = true;
            break;
        }
    }
    if !converged {
        return None;
    }
    let coefficients: Vec<f64> = weights
        .iter()
        .zip(scales)
        .map(|(weight, scale)| weight / scale)
        .collect();
    let observed: f64 = validation
        .iter()
        .map(|interval| interval.quota_increase)
        .sum();
    let error: f64 = validation
        .iter()
        .map(|interval| {
            let predicted: f64 = interval
                .features
                .iter()
                .zip(&coefficients)
                .map(|(feature, coefficient)| feature * coefficient)
                .sum();
            (predicted - interval.quota_increase).abs()
        })
        .sum::<f64>()
        / observed;
    (error.is_finite() && error <= 0.2).then_some(ValidatedCalibration {
        window: ProviderAccountWindow {
            provider,
            account: account.as_str().to_owned(),
            reset_at_ms,
        },
        version: 1,
        validated_at_ms: validation.last()?.end_ms,
        feature_keys: feature_keys.clone(),
        coefficients,
        training_intervals: training.len(),
        validation_intervals: validation.len(),
        validation_error: error,
    })
}

/// Project one model only when every nonzero token category has a validated
/// coefficient. The result is an estimate of account percentage points, not
/// a measured per-agent counter; reasoning is already included in output.
pub fn estimate_model_quota(
    calibration: &ValidatedCalibration,
    window: &ProviderAccountWindow,
    model: &str,
    tokens: &TokenTotals,
) -> Option<f64> {
    if calibration.window != *window
        || calibration.version != 1
        || calibration.feature_keys.len() != calibration.coefficients.len()
        || !calibration
            .feature_keys
            .iter()
            .any(|key| key.model == model)
    {
        return None;
    }
    let categories = [
        (TokenCategory::FreshInput, tokens.input),
        (TokenCategory::CacheRead, tokens.cache_read),
        (TokenCategory::CacheWrite, tokens.cache_write),
        (TokenCategory::Output, tokens.output),
    ];
    let mut estimate = 0.0;
    for (category, count) in categories {
        if count == 0 {
            continue;
        }
        let index = calibration
            .feature_keys
            .iter()
            .position(|key| key.model == model && key.category == category)?;
        let coefficient = calibration.coefficients[index];
        if !coefficient.is_finite() || coefficient < 0.0 {
            return None;
        }
        estimate += count as f64 * coefficient;
    }
    estimate.is_finite().then_some(estimate)
}

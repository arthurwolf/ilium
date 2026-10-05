//! Percentile summaries used throughout the corpus statistics.

use serde::{Deserialize, Serialize};

/// Count, p10 / p50 / p90 and mean of a sample.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Quantiles {
    /// Number of values.
    pub n: usize,
    /// 10th percentile.
    pub p10: f64,
    /// Median.
    pub p50: f64,
    /// 90th percentile.
    pub p90: f64,
    /// Arithmetic mean.
    pub mean: f64,
}

impl Quantiles {
    /// Summarises `values`; `None` when empty. The slice is sorted in place.
    pub fn of(values: &mut [f64]) -> Option<Self> {
        if values.is_empty() {
            return None;
        }
        values.sort_by(f64::total_cmp);
        let mean = values.iter().sum::<f64>() / values.len() as f64;
        Some(Self {
            n: values.len(),
            p10: percentile_sorted(values, 10.0),
            p50: percentile_sorted(values, 50.0),
            p90: percentile_sorted(values, 90.0),
            mean,
        })
    }

    /// Summarises integer samples.
    pub fn of_u32(values: impl IntoIterator<Item = u32>) -> Option<Self> {
        let mut collected: Vec<f64> = values.into_iter().map(f64::from).collect();
        Self::of(&mut collected)
    }
}

/// Linear-interpolated percentile of an ascending slice (`percent` in 0..=100).
pub fn percentile_sorted(sorted: &[f64], percent: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let position = (sorted.len() - 1) as f64 * percent / 100.0;
    let lower = position.floor() as usize;
    let upper = (lower + 1).min(sorted.len() - 1);
    sorted[lower] + (sorted[upper] - sorted[lower]) * (position - lower as f64)
}

/// Median of a sample (0 when empty); sorts a copy.
pub fn median(values: &[f64]) -> f64 {
    let mut copy = values.to_vec();
    copy.sort_by(f64::total_cmp);
    percentile_sorted(&copy, 50.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_linear_interpolation() {
        let mut values: Vec<f64> = (1..=11).map(f64::from).collect();
        let quantiles = Quantiles::of(&mut values).unwrap();
        assert_eq!(quantiles.n, 11);
        assert!((quantiles.p10 - 2.0).abs() < 1e-12);
        assert!((quantiles.p50 - 6.0).abs() < 1e-12);
        assert!((quantiles.p90 - 10.0).abs() < 1e-12);
        assert!((quantiles.mean - 6.0).abs() < 1e-12);
        assert!(Quantiles::of(&mut []).is_none());
        assert!((median(&[3.0, 1.0, 2.0, 10.0]) - 2.5).abs() < 1e-12);
    }
}

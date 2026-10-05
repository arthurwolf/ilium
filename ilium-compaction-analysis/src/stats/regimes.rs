//! Compaction regimes: groups of compactions that fired at about the same size.
//!
//! Once a user lowers the trigger, the logs mix two regimes (for example
//! Claude: about 567k historically, about 217k afterwards). Semantics offsets
//! and post-compaction draws must come from the most recent regime.

use serde::{Deserialize, Serialize};

use super::quantiles::median;

/// One cluster of compactions with similar pre-compaction size.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Regime {
    /// Median pre-compaction size of the members.
    pub center_tokens: u32,
    /// Smallest member.
    pub min_tokens: u32,
    /// Largest member.
    pub max_tokens: u32,
    /// Number of member compactions.
    pub count: usize,
    /// Time of the newest member, Unix milliseconds (0 when unknown).
    pub latest_timestamp_ms: i64,
    /// Whether this is the regime of the newest compaction.
    pub is_most_recent: bool,
}

impl Regime {
    /// The pre-compaction sizes belonging to this regime.
    pub fn pre_token_range(&self) -> std::ops::RangeInclusive<u32> {
        self.min_tokens..=self.max_tokens
    }
}

/// Regimes sorted by size, plus the index of the most recent one.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RegimeSet {
    /// Regimes in ascending size order.
    pub regimes: Vec<Regime>,
}

impl RegimeSet {
    /// The regime of the newest compaction.
    pub fn most_recent(&self) -> Option<&Regime> {
        self.regimes.iter().find(|regime| regime.is_most_recent)
    }
}

/// Clusters `(pre_tokens, timestamp_ms)` pairs by splitting the sorted sizes
/// wherever two neighbours differ by more than `gap_ratio` (for example 1.2 =
/// 20 %). The regime holding the newest timestamp is flagged.
pub fn cluster_regimes(points: &[(u32, i64)], gap_ratio: f64) -> RegimeSet {
    let mut sorted: Vec<(u32, i64)> = points.iter().copied().filter(|point| point.0 > 0).collect();
    if sorted.is_empty() {
        return RegimeSet::default();
    }
    sorted.sort_by_key(|point| point.0);
    let mut groups: Vec<Vec<(u32, i64)>> = vec![vec![sorted[0]]];
    for window in sorted.windows(2) {
        let (previous, next) = (window[0].0, window[1]);
        if f64::from(next.0) > f64::from(previous) * gap_ratio {
            groups.push(Vec::new());
        }
        if let Some(group) = groups.last_mut() {
            group.push(next);
        }
    }
    let mut regimes: Vec<Regime> = groups
        .iter()
        .map(|group| {
            let sizes: Vec<f64> = group.iter().map(|point| f64::from(point.0)).collect();
            Regime {
                center_tokens: median(&sizes).round() as u32,
                min_tokens: group.first().map_or(0, |point| point.0),
                max_tokens: group.last().map_or(0, |point| point.0),
                count: group.len(),
                latest_timestamp_ms: group.iter().map(|point| point.1).max().unwrap_or(0),
                is_most_recent: false,
            }
        })
        .collect();
    let newest = regimes
        .iter()
        .enumerate()
        .max_by_key(|(_, regime)| (regime.latest_timestamp_ms, regime.count))
        .map(|(index, _)| index);
    if let Some(index) = newest {
        regimes[index].is_most_recent = true;
    }
    RegimeSet { regimes }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn separates_an_old_and_a_new_regime() {
        let mut points: Vec<(u32, i64)> = (0..20)
            .map(|index| (565_000 + index * 500, 1_000 + i64::from(index)))
            .collect();
        points.extend((0..5).map(|index| (215_000 + index * 400, 5_000 + i64::from(index))));
        let set = cluster_regimes(&points, 1.2);
        assert_eq!(set.regimes.len(), 2);
        assert_eq!(set.regimes[0].count, 5);
        assert!(set.regimes[0].is_most_recent);
        assert!(!set.regimes[1].is_most_recent);
        assert_eq!(set.most_recent().unwrap().center_tokens, 215_800);
    }

    #[test]
    fn empty_input_has_no_regime() {
        assert!(cluster_regimes(&[], 1.2).regimes.is_empty());
        assert!(cluster_regimes(&[(0, 5)], 1.2).regimes.is_empty());
    }
}

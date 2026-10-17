//! Timing facts of one progress monitor: when its task started, when it last
//! made progress, and the rate and ETA derived from the recorded percent
//! samples. Pure arithmetic over caller-supplied clock values (Unix
//! milliseconds), so every rule is unit-testable without a clock or a task.

use serde::{Deserialize, Serialize};

/// Percent samples kept per monitor. Samples are recorded only when the
/// percent moves, so this covers far more than the rate window needs.
pub const MAXIMUM_PROGRESS_SAMPLES: usize = 24;
/// The recent rate looks only at progress made within this window.
const RECENT_RATE_WINDOW_MILLIS: u64 = 10 * 60 * 1000;
/// A running task counts as stalled after this many probe intervals without
/// progress, but never sooner than `STALL_MINIMUM_MILLIS`.
pub const STALL_PROBE_COUNT: u64 = 10;
pub const STALL_MINIMUM_MILLIS: u64 = 10 * 60 * 1000;
/// Percent movement below this is probe noise, not progress.
const MINIMUM_PERCENT_DELTA: f32 = 0.01;
/// The ETA range is reported only when the slow and fast estimates differ by
/// more than this fraction; otherwise the single value says everything.
const RANGE_SPREAD_FRACTION: f64 = 0.1;

/// One recorded `(time, percent)` observation.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ProgressSample {
    pub unix_millis: u64,
    pub percent: f32,
}

/// When a monitored task started and last moved. `None` fields mean the
/// value was never recorded (a monitor restored from an older snapshot), and
/// the presentation shows them as unknown rather than inventing a time.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ProgressTiming {
    #[serde(default)]
    pub started_at_unix_millis: Option<u64>,
    /// Percent reported when the monitor first saw the task.
    #[serde(default)]
    pub started_percent: f32,
    /// Last time the percent rose or the one-line message changed.
    #[serde(default)]
    pub last_progress_unix_millis: Option<u64>,
    #[serde(default)]
    pub probe_interval_millis: Option<u64>,
    #[serde(default)]
    pub samples: Vec<ProgressSample>,
    /// Absolute time the probe's latest report expects the task to finish
    /// (report time plus its `eta_seconds`). Cleared when a report omits it.
    #[serde(default)]
    pub reported_finish_unix_millis: Option<u64>,
}

/// Rate in percent per minute with the spread between the whole-run and the
/// recent pace.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProgressRate {
    pub percent_per_minute: f64,
    pub recent_percent_per_minute: Option<f64>,
    pub whole_run_percent_per_minute: Option<f64>,
}

/// Remaining time until completion, measured from `now`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EtaEstimate {
    pub expected_millis: u64,
    pub fastest_millis: u64,
    pub slowest_millis: u64,
}

impl EtaEstimate {
    /// `(fastest, slowest)` when the two paces disagree enough to matter.
    pub fn range_millis(&self) -> Option<(u64, u64)> {
        let spread = self.slowest_millis.saturating_sub(self.fastest_millis);
        (spread as f64 > self.fastest_millis as f64 * RANGE_SPREAD_FRACTION + 1000.0)
            .then_some((self.fastest_millis, self.slowest_millis))
    }
}

impl ProgressTiming {
    /// Timing for a task first observed at `now` reporting `percent`.
    pub fn started(now: u64, percent: f32) -> Self {
        Self {
            started_at_unix_millis: Some(now),
            started_percent: percent,
            last_progress_unix_millis: Some(now),
            probe_interval_millis: None,
            samples: vec![ProgressSample {
                unix_millis: now,
                percent,
            }],
            reported_finish_unix_millis: None,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.samples.len() > MAXIMUM_PROGRESS_SAMPLES {
            return Err(format!(
                "progress timing holds {} samples; at most {MAXIMUM_PROGRESS_SAMPLES} are allowed",
                self.samples.len()
            ));
        }
        let finite = |value: f32| value.is_finite() && (0.0..=100.0).contains(&value);
        if !finite(self.started_percent) || !self.samples.iter().all(|s| finite(s.percent)) {
            return Err("progress timing holds a percent outside 0 to 100".into());
        }
        Ok(())
    }

    /// Records one probe result. Progress is a rise in percent or a changed
    /// message; a falling percent restarts the rate history (the task began
    /// a new phase) and counts as progress.
    pub fn observe(&mut self, now: u64, percent: f32, message_changed: bool) {
        let previous = self.samples.last().copied();
        let sample = ProgressSample {
            unix_millis: now,
            percent,
        };
        let mut made_progress = message_changed;
        match previous {
            None => {
                self.samples.push(sample);
                made_progress = true;
            }
            Some(last) if percent + MINIMUM_PERCENT_DELTA < last.percent => {
                self.samples.clear();
                self.samples.push(sample);
                made_progress = true;
            }
            Some(last) if percent - last.percent >= MINIMUM_PERCENT_DELTA => {
                self.samples.push(sample);
                made_progress = true;
            }
            Some(_) => {}
        }
        if self.samples.len() > MAXIMUM_PROGRESS_SAMPLES {
            let excess = self.samples.len() - MAXIMUM_PROGRESS_SAMPLES;
            self.samples.drain(..excess);
        }
        if made_progress && self.started_at_unix_millis.is_some() {
            self.last_progress_unix_millis = Some(now);
        }
    }

    /// Milliseconds without progress, or `None` when never recorded.
    pub fn quiet_millis(&self, now: u64) -> Option<u64> {
        self.last_progress_unix_millis
            .map(|last| now.saturating_sub(last))
    }

    /// Quiet time after which a running task counts as stalled: ten probe
    /// intervals, never under ten minutes.
    pub fn stall_threshold_millis(&self) -> u64 {
        self.probe_interval_millis
            .unwrap_or(0)
            .saturating_mul(STALL_PROBE_COUNT)
            .max(STALL_MINIMUM_MILLIS)
    }

    /// Whether the task has been quiet past the stall threshold. Callers
    /// apply this only to tasks that are still running.
    pub fn is_stalled(&self, now: u64) -> bool {
        self.quiet_millis(now)
            .is_some_and(|quiet| quiet >= self.stall_threshold_millis())
    }

    /// Time from start to `until`, when the start is known.
    pub fn elapsed_millis(&self, until: u64) -> Option<u64> {
        self.started_at_unix_millis
            .map(|started| until.saturating_sub(started))
    }

    fn whole_run_rate(&self, percent: f32) -> Option<f64> {
        let elapsed = self
            .last_progress_unix_millis?
            .checked_sub(self.started_at_unix_millis?)?;
        let gained = f64::from(percent - self.started_percent);
        (elapsed > 0 && gained > 0.0).then(|| gained / elapsed as f64)
    }

    fn recent_rate(&self) -> Option<f64> {
        let latest = self.samples.last()?;
        let anchor = self
            .samples
            .iter()
            .find(|sample| sample.unix_millis + RECENT_RATE_WINDOW_MILLIS >= latest.unix_millis)?;
        let elapsed = latest.unix_millis.checked_sub(anchor.unix_millis)?;
        let gained = f64::from(latest.percent - anchor.percent);
        (elapsed > 0 && gained > 0.0).then(|| gained / elapsed as f64)
    }

    /// `(expected, slowest, fastest)` rates in percent per millisecond.
    fn rates(&self, percent: f32) -> Option<(f64, f64, f64)> {
        match (self.recent_rate(), self.whole_run_rate(percent)) {
            (Some(recent), Some(whole)) => {
                Some(((recent + whole) / 2.0, recent.min(whole), recent.max(whole)))
            }
            (Some(rate), None) | (None, Some(rate)) => Some((rate, rate, rate)),
            (None, None) => None,
        }
    }

    /// Current pace: the average of the whole-run and recent rates, because
    /// builds run in bursts and either one alone misleads.
    pub fn rate(&self, percent: f32) -> Option<ProgressRate> {
        let (expected, _, _) = self.rates(percent)?;
        Some(ProgressRate {
            percent_per_minute: expected * 60_000.0,
            recent_percent_per_minute: self.recent_rate().map(|rate| rate * 60_000.0),
            whole_run_percent_per_minute: self.whole_run_rate(percent).map(|rate| rate * 60_000.0),
        })
    }

    /// Records the probe's own ETA (seconds from `observed_at`), or forgets
    /// it when the latest report carries none.
    pub fn set_reported_eta(&mut self, observed_at: u64, eta_seconds: Option<u64>) {
        self.reported_finish_unix_millis =
            eta_seconds.map(|seconds| observed_at.saturating_add(seconds.saturating_mul(1000)));
    }

    /// Time left, measured from `now`. `None` at 0% or 100% and when no
    /// positive pace has been seen. An overdue estimate clamps to zero.
    pub fn estimate(&self, now: u64, percent: f32) -> Option<EtaEstimate> {
        if !(0.0..100.0).contains(&percent) {
            return None;
        }
        let measured = if percent > 0.0 {
            self.rates(percent)
        } else {
            None
        };
        if let Some(finish) = self.reported_finish_unix_millis {
            // The probe knows its task better than a pace guess: its ETA is
            // the expected value; the measured paces only widen the range.
            let expected_millis = finish.saturating_sub(now);
            let bounds = measured.and_then(|(_, slowest, fastest)| {
                let last_progress = self.last_progress_unix_millis? as f64;
                let remaining = f64::from(100.0 - percent);
                let finish_in =
                    |rate: f64| (last_progress + remaining / rate - now as f64).max(0.0) as u64;
                Some((finish_in(fastest), finish_in(slowest)))
            });
            let (fastest_millis, slowest_millis) = match bounds {
                Some((fastest, slowest)) => {
                    (fastest.min(expected_millis), slowest.max(expected_millis))
                }
                None => (expected_millis, expected_millis),
            };
            return Some(EtaEstimate {
                expected_millis,
                fastest_millis,
                slowest_millis,
            });
        }
        if percent <= 0.0 {
            return None;
        }
        let (expected, slowest, fastest) = measured?;
        let last_progress = self.last_progress_unix_millis? as f64;
        let remaining = f64::from(100.0 - percent);
        let finish_in = |rate: f64| (last_progress + remaining / rate - now as f64).max(0.0) as u64;
        Some(EtaEstimate {
            expected_millis: finish_in(expected),
            fastest_millis: finish_in(fastest),
            slowest_millis: finish_in(slowest),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINUTE: u64 = 60_000;

    fn steady(minutes: u64, percent_per_minute: f32) -> ProgressTiming {
        let mut timing = ProgressTiming::started(0, 0.0);
        for minute in 1..=minutes {
            timing.observe(minute * MINUTE, percent_per_minute * minute as f32, false);
        }
        timing
    }

    #[test]
    fn steady_pace_predicts_the_remaining_time() {
        let timing = steady(10, 5.0);
        let estimate = timing.estimate(10 * MINUTE, 50.0).unwrap();
        assert_eq!(estimate.expected_millis, 10 * MINUTE);
        assert_eq!(estimate.range_millis(), None);
        let rate = timing.rate(50.0).unwrap();
        assert!((rate.percent_per_minute - 5.0).abs() < 1e-6);
    }

    #[test]
    fn probe_reported_eta_overrides_the_pace_and_counts_down() {
        let mut timing = steady(10, 5.0);
        timing.set_reported_eta(10 * MINUTE, Some(300));
        let estimate = timing.estimate(10 * MINUTE, 50.0).unwrap();
        assert_eq!(estimate.expected_millis, 5 * MINUTE);
        let later = timing.estimate(12 * MINUTE, 50.0).unwrap();
        assert_eq!(later.expected_millis, 3 * MINUTE);
        // The measured pace (10 minutes left) widens the range.
        assert_eq!(estimate.range_millis(), Some((5 * MINUTE, 10 * MINUTE)));
        assert_eq!(
            timing.estimate(20 * MINUTE, 50.0).unwrap().expected_millis,
            0
        );
        assert!(timing.estimate(10 * MINUTE, 100.0).is_none());
    }

    #[test]
    fn probe_reported_eta_works_before_any_pace_exists() {
        let mut timing = ProgressTiming::started(0, 0.0);
        timing.set_reported_eta(0, Some(90));
        let estimate = timing.estimate(0, 0.0).unwrap();
        assert_eq!(estimate.expected_millis, 90_000);
        assert_eq!(estimate.range_millis(), None);
        timing.set_reported_eta(MINUTE, None);
        assert!(timing.estimate(MINUTE, 0.0).is_none());
    }

    #[test]
    fn estimate_counts_down_while_idle_between_probes() {
        let timing = steady(10, 5.0);
        let estimate = timing.estimate(12 * MINUTE, 50.0).unwrap();
        assert_eq!(estimate.expected_millis, 8 * MINUTE);
        let overdue = timing.estimate(30 * MINUTE, 50.0).unwrap();
        assert_eq!(overdue.expected_millis, 0);
    }

    #[test]
    fn burst_after_a_slow_start_gives_a_range() {
        // 2% in the first 20 minutes, then 20% more in the last 5.
        let mut timing = ProgressTiming::started(0, 0.0);
        timing.observe(20 * MINUTE, 2.0, false);
        for step in 1..=5 {
            timing.observe((20 + step) * MINUTE, 2.0 + 4.0 * step as f32, false);
        }
        let estimate = timing.estimate(25 * MINUTE, 22.0).unwrap();
        let (fastest, slowest) = estimate.range_millis().expect("paces disagree");
        assert!(fastest < estimate.expected_millis && estimate.expected_millis < slowest);
    }

    #[test]
    fn no_estimate_at_zero_complete_or_without_movement() {
        let timing = ProgressTiming::started(0, 0.0);
        assert_eq!(timing.estimate(MINUTE, 0.0), None);
        assert_eq!(steady(4, 5.0).estimate(4 * MINUTE, 100.0), None);
        let mut flat = ProgressTiming::started(0, 30.0);
        flat.observe(MINUTE, 30.0, false);
        assert_eq!(flat.estimate(MINUTE, 30.0), None);
    }

    #[test]
    fn falling_percent_restarts_the_rate_history() {
        let mut timing = steady(5, 10.0);
        timing.observe(6 * MINUTE, 5.0, false);
        assert_eq!(timing.samples.len(), 1);
        assert_eq!(timing.last_progress_unix_millis, Some(6 * MINUTE));
        assert_eq!(timing.started_at_unix_millis, Some(0));
    }

    #[test]
    fn progress_is_a_rise_or_a_message_change_not_a_probe() {
        let mut timing = ProgressTiming::started(0, 10.0);
        timing.observe(MINUTE, 10.0, false);
        assert_eq!(timing.last_progress_unix_millis, Some(0));
        timing.observe(2 * MINUTE, 10.0, true);
        assert_eq!(timing.last_progress_unix_millis, Some(2 * MINUTE));
        timing.observe(3 * MINUTE, 11.0, false);
        assert_eq!(timing.last_progress_unix_millis, Some(3 * MINUTE));
    }

    #[test]
    fn stall_needs_ten_probe_intervals_and_ten_minutes() {
        let mut timing = ProgressTiming::started(0, 10.0);
        timing.probe_interval_millis = Some(30_000);
        assert_eq!(timing.stall_threshold_millis(), 10 * MINUTE);
        assert!(!timing.is_stalled(10 * MINUTE - 1));
        assert!(timing.is_stalled(10 * MINUTE));
        timing.probe_interval_millis = Some(2 * MINUTE);
        assert_eq!(timing.stall_threshold_millis(), 20 * MINUTE);
        assert!(!timing.is_stalled(19 * MINUTE));
        assert!(timing.is_stalled(20 * MINUTE));
    }

    #[test]
    fn unknown_timing_stays_unknown_and_never_stalls() {
        let mut timing = ProgressTiming::default();
        timing.observe(5 * MINUTE, 20.0, true);
        assert_eq!(timing.started_at_unix_millis, None);
        assert_eq!(timing.last_progress_unix_millis, None);
        assert!(!timing.is_stalled(u64::MAX));
        assert_eq!(timing.elapsed_millis(10), None);
    }

    #[test]
    fn samples_stay_bounded() {
        let mut timing = ProgressTiming::started(0, 0.0);
        for step in 1..=100_u64 {
            timing.observe(step * 1000, step as f32 * 0.5, false);
        }
        assert_eq!(timing.samples.len(), MAXIMUM_PROGRESS_SAMPLES);
        assert!(timing.validate().is_ok());
    }
}

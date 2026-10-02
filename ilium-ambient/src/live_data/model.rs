//! Validated observations shared by graph and geographic live backgrounds.
//! Observation timestamps belong to the provider; receiving the same snapshot
//! twice must not create a new sample or conceal an outage.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_replaces_revisions_without_inventing_poll_samples() {
        let mut history = History::new(3);
        assert!(history.ingest(Observation::new(100, 1.0).unwrap()));
        assert!(!history.ingest(Observation::new(100, 1.0).unwrap()));
        assert!(history.ingest(Observation::new(100, 2.0).unwrap()));
        assert_eq!(history.samples().len(), 1);
        assert_eq!(history.samples()[0].value, 2.0);
    }

    #[test]
    fn out_of_order_history_is_sorted_bounded_and_ignores_old_evicted_data() {
        let mut history = History::new(2);
        for (time, value) in [(300, 3.0), (100, 1.0), (200, 2.0), (50, 0.5)] {
            history.ingest(Observation::new(time, value).unwrap());
        }
        assert_eq!(
            history
                .samples()
                .iter()
                .map(|p| p.observed_ms)
                .collect::<Vec<_>>(),
            [200, 300]
        );
    }

    #[test]
    fn invalid_numbers_are_rejected_and_negative_measurements_are_retained() {
        assert!(Observation::new(0, f64::NAN).is_none());
        assert!(Observation::new(-1, 3.0).is_none());
        assert_eq!(Observation::new(123, -0.7).unwrap().value, -0.7);
        assert!(Candle::new(100, 1.0, 0.5, 2.0, 1.5, 0.0).is_none());
        assert!(Candle::new(100, 1.0, 0.5, 2.0, 1.5, -1.0).is_none());
        assert!(Candle::new(100, 1.0, 2.0, 0.5, 1.5, 0.0).is_some());
    }

    #[test]
    fn fetch_freshness_and_observation_freshness_remain_separate() {
        let mut feed = FeedState::default();
        feed.received(1_000, Some(500));
        feed.failed("timeout".into());
        assert_eq!(feed.received_ms, Some(1_000));
        assert_eq!(feed.observed_ms, Some(500));
        assert!(feed.is_stale(2_001, 1_000));
        feed.received(2_500, Some(500));
        assert!(!feed.is_stale(2_501, 1_000));
        assert_eq!(feed.observed_ms, Some(500));
        assert!(feed.error.is_none());
    }

    #[test]
    fn positions_validate_geography_and_keep_unknown_values_unknown() {
        assert!(Position::new("x".into(), 181.0, 10.0, 100).is_none());
        assert!(Position::new("x".into(), 1.0, f64::INFINITY, 100).is_none());
        let position = Position::new("x".into(), 180.0, -90.0, 100).unwrap();
        assert_eq!(position.heading_degrees, None);
        assert_eq!(position.speed_metres_per_second, None);
    }
}

/// A single measurement in provider time, in milliseconds since Unix epoch.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Observation {
    pub observed_ms: i64,
    pub value: f64,
}

impl Observation {
    pub fn new(observed_ms: i64, value: f64) -> Option<Self> {
        (observed_ms >= 0 && value.is_finite()).then_some(Self { observed_ms, value })
    }
}

/// Genuine OHLC, never synthesized from repeated fetches of a spot price.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Candle {
    pub observed_ms: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
}

impl Candle {
    pub fn new(
        observed_ms: i64,
        open: f64,
        high: f64,
        low: f64,
        close: f64,
        volume: f64,
    ) -> Option<Self> {
        if observed_ms < 0
            || ![open, high, low, close, volume]
                .iter()
                .all(|v| v.is_finite())
            || high < low
            || high < open.max(close)
            || low > open.min(close)
            || volume < 0.0
        {
            return None;
        }
        Some(Self {
            observed_ms,
            open,
            high,
            low,
            close,
            volume,
        })
    }
}

/// Sorted, fixed-capacity observations. A revision replaces the provider's
/// existing timestamp; polling cadence is never an observation timestamp.
#[derive(Debug)]
pub struct History {
    samples: Vec<Observation>,
    capacity: usize,
}

impl Default for History {
    fn default() -> Self {
        Self::new(4096)
    }
}

impl History {
    pub fn new(capacity: usize) -> Self {
        Self {
            samples: Vec::new(),
            capacity: capacity.clamp(1, 100_000),
        }
    }

    pub fn samples(&self) -> &[Observation] {
        &self.samples
    }

    pub fn ingest(&mut self, sample: Observation) -> bool {
        if Observation::new(sample.observed_ms, sample.value).is_none() {
            return false;
        }
        match self
            .samples
            .binary_search_by_key(&sample.observed_ms, |point| point.observed_ms)
        {
            Ok(index) => {
                if self.samples[index] == sample {
                    return false;
                }
                self.samples[index] = sample;
            }
            Err(index) => {
                if self.samples.len() >= self.capacity && index == 0 {
                    return false;
                }
                self.samples.insert(index, sample);
                if self.samples.len() > self.capacity {
                    self.samples.remove(0);
                }
            }
        }
        true
    }

    /// Drop observations outside the requested window in provider time.
    pub fn retain_since(&mut self, cutoff_ms: i64) {
        let first = self
            .samples
            .partition_point(|point| point.observed_ms < cutoff_ms);
        self.samples.drain(..first);
    }
}

/// Geographic observations retain missing heading/speed instead of turning
/// unknown values into a spurious northbound, stationary object.
#[derive(Debug, Clone, PartialEq)]
pub struct Position {
    pub id: String,
    pub longitude: f64,
    pub latitude: f64,
    pub observed_ms: i64,
    pub heading_degrees: Option<f64>,
    pub speed_metres_per_second: Option<f64>,
    pub label: Option<String>,
}

impl Position {
    pub fn new(id: String, longitude: f64, latitude: f64, observed_ms: i64) -> Option<Self> {
        if id.is_empty()
            || id.len() > 256
            || observed_ms < 0
            || !longitude.is_finite()
            || !latitude.is_finite()
            || !(-180.0..=180.0).contains(&longitude)
            || !(-90.0..=90.0).contains(&latitude)
        {
            return None;
        }
        Some(Self {
            id,
            longitude,
            latitude,
            observed_ms,
            heading_degrees: None,
            speed_metres_per_second: None,
            label: None,
        })
    }
}

/// An event may have an unknown magnitude. Zero and negative magnitudes are
/// valid seismic observations and must survive decoding and filtering.
#[derive(Debug, Clone, PartialEq)]
pub struct Earthquake {
    pub position: Position,
    pub magnitude: Option<f64>,
    pub depth_km: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct FeedState {
    pub received_ms: Option<i64>,
    pub observed_ms: Option<i64>,
    pub error: Option<String>,
}

impl FeedState {
    pub fn received(&mut self, received_ms: i64, observed_ms: Option<i64>) {
        if received_ms < 0 {
            return;
        }
        self.received_ms = Some(received_ms);
        self.observed_ms = observed_ms.filter(|time| *time >= 0);
        self.error = None;
    }

    pub fn failed(&mut self, error: String) {
        self.error = Some(error.chars().take(240).collect());
    }

    /// Transport silence is separate from a successfully refreshed daily
    /// indicator whose latest observation is still yesterday's value.
    pub fn is_stale(&self, now_ms: i64, silence_ms: i64) -> bool {
        self.received_ms
            .is_none_or(|time| now_ms.saturating_sub(time) > silence_ms.max(0))
    }
}

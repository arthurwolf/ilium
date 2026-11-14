//! Pure line, signed-bar and genuine OHLC projection into Braille dots.
use super::model::{Candle, Observation};
use crate::raster::Raster;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn signed_bars_include_zero_and_put_negative_values_below_it() {
        let observations = [
            Observation::new(100, -2.0).unwrap(),
            Observation::new(200, 4.0).unwrap(),
        ];
        let mut raster = Raster::default();
        raster.resize(80, 48);
        let bounds =
            render_series(&mut raster, &observations, (100, 200), ChartMode::Bars, 0.8).unwrap();
        assert_eq!(bounds.low, -2.0);
        assert_eq!(bounds.high, 4.0);
        assert!(raster.dots.iter().any(|dot| *dot > 0.5));
    }
    #[test]
    fn constant_and_single_series_draw_without_nan_or_fake_history() {
        let mut raster = Raster::default();
        raster.resize(80, 48);
        let point = Observation::new(150, 7.0).unwrap();
        render_series(&mut raster, &[point], (100, 200), ChartMode::Line, 0.8).unwrap();
        assert!(raster.dots.iter().all(|dot| dot.is_finite()));
        assert!(raster.dots.iter().any(|dot| *dot > 0.1));
        let before = raster.dots.clone();
        assert!(render_series(&mut raster, &[point], (200, 300), ChartMode::Line, 0.8).is_none());
        assert_eq!(raster.dots, before);
    }
    #[test]
    fn candle_bounds_include_wicks_rather_than_only_closing_price() {
        let candle = Candle::new(150, 2.0, 10.0, -4.0, 3.0, 5.0).unwrap();
        let mut raster = Raster::default();
        raster.resize(80, 48);
        let bounds = render_candles(&mut raster, &[candle], (100, 200), 0.8).unwrap();
        assert_eq!((bounds.low, bounds.high), (-4.0, 10.0));
        assert!(raster.dots.iter().any(|dot| *dot > 0.5));
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChartMode {
    Line,
    Bars,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChartBounds {
    pub low: f64,
    pub high: f64,
}

impl ChartBounds {
    fn y(self, value: f64) -> f32 {
        if self.low == self.high {
            return 0.5;
        }
        let scale = self.low.abs().max(self.high.abs()).max(1.0);
        let fraction = (value / scale - self.low / scale) / (self.high / scale - self.low / scale);
        (0.90 - fraction.clamp(0.0, 1.0) * 0.80) as f32
    }
}

fn x(time: i64, window: (i64, i64)) -> f32 {
    // Subtract exact integers before converting; adjacent timestamps at the
    // upper i64 boundary otherwise round to the same floating-point value.
    let elapsed = (i128::from(time) - i128::from(window.0)) as f64;
    let span = (i128::from(window.1) - i128::from(window.0)) as f64;
    (0.08 + elapsed / span * 0.86) as f32
}

fn axes(raster: &mut Raster, bounds: ChartBounds, intensity: f32) {
    raster.line((0.08, 0.1), (0.08, 0.9), 0.35, intensity * 0.3);
    raster.line((0.08, 0.9), (0.94, 0.9), 0.35, intensity * 0.3);
    if bounds.low < 0.0 && bounds.high > 0.0 {
        let zero = bounds.y(0.0);
        raster.line((0.08, zero), (0.94, zero), 0.25, intensity * 0.2);
    }
}

pub fn render_series(
    raster: &mut Raster,
    observations: &[Observation],
    window: (i64, i64),
    mode: ChartMode,
    intensity: f32,
) -> Option<ChartBounds> {
    if raster.width == 0 || raster.height == 0 || window.1 <= window.0 || !intensity.is_finite() {
        return None;
    }
    let samples: Vec<_> = observations
        .iter()
        .filter(|point| {
            point.value.is_finite()
                && point.observed_ms >= window.0
                && point.observed_ms <= window.1
        })
        .collect();
    let first = samples.first()?;
    let mut bounds = ChartBounds {
        low: first.value,
        high: first.value,
    };
    for point in &samples {
        bounds.low = bounds.low.min(point.value);
        bounds.high = bounds.high.max(point.value);
    }
    if mode == ChartMode::Bars {
        bounds.low = bounds.low.min(0.0);
        bounds.high = bounds.high.max(0.0);
    }
    let intensity = intensity.clamp(0.0, 1.0);
    axes(raster, bounds, intensity);
    let mut previous = None;
    let bar_radius = (raster.width as f32 / samples.len().max(1) as f32 * 0.2).clamp(0.4, 5.0);
    for sample in samples {
        let position = (x(sample.observed_ms, window), bounds.y(sample.value));
        match mode {
            ChartMode::Line => {
                raster.line(previous.unwrap_or(position), position, 0.65, intensity);
                previous = Some(position);
            }
            ChartMode::Bars => {
                raster.line((position.0, bounds.y(0.0)), position, bar_radius, intensity)
            }
        }
    }
    Some(bounds)
}

pub fn render_candles(
    raster: &mut Raster,
    candles: &[Candle],
    window: (i64, i64),
    intensity: f32,
) -> Option<ChartBounds> {
    if raster.width == 0 || raster.height == 0 || window.1 <= window.0 || !intensity.is_finite() {
        return None;
    }
    let samples: Vec<_> = candles
        .iter()
        .filter(|c| {
            c.observed_ms >= window.0
                && c.observed_ms <= window.1
                && Candle::new(c.observed_ms, c.open, c.high, c.low, c.close, c.volume).is_some()
        })
        .collect();
    let first = samples.first()?;
    let mut bounds = ChartBounds {
        low: first.low,
        high: first.high,
    };
    for candle in &samples {
        bounds.low = bounds.low.min(candle.low);
        bounds.high = bounds.high.max(candle.high);
    }
    let intensity = intensity.clamp(0.0, 1.0);
    axes(raster, bounds, intensity);
    let body_radius = (raster.width as f32 / samples.len().max(1) as f32 * 0.2).clamp(0.5, 5.0);
    for candle in samples {
        let column = x(candle.observed_ms, window);
        raster.line(
            (column, bounds.y(candle.low)),
            (column, bounds.y(candle.high)),
            0.4,
            intensity * 0.7,
        );
        raster.line(
            (column, bounds.y(candle.open)),
            (column, bounds.y(candle.close)),
            body_radius,
            intensity,
        );
    }
    Some(bounds)
}

#[cfg(test)]
mod timestamp_precision_tests {
    use super::*;
    #[test]
    fn adjacent_large_integer_timestamps_still_render_the_observation() {
        let mut raster = Raster::default();
        raster.resize(80, 48);
        let point = Observation::new(i64::MAX - 1, 7.0).unwrap();
        render_series(
            &mut raster,
            &[point],
            (i64::MAX - 1, i64::MAX),
            ChartMode::Line,
            0.8,
        )
        .unwrap();
        // Axis intensity never exceeds .24; brighter ink must be the data.
        assert!(raster.dots.iter().any(|dot| *dot > 0.4));
    }
}

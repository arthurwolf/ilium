//! The pictures the clouds scene holds: decoded satellite frames and the set the scene draws
//! from, with the luminance levels used for automatic contrast across the whole set.

use crate::kit::geo_tiles::{GeoBox, GeoGrid, UtcTime};
use std::sync::Arc;

/// One decoded image: 8-bit luminance (and optionally colour) on a lon/lat box.
pub struct CloudFrame {
    pub time: UtcTime,
    pub grid: GeoGrid,
    histogram: [u32; 256],
}

impl CloudFrame {
    pub fn new(time: UtcTime, grid: GeoGrid) -> Self {
        let mut histogram = [0u32; 256];
        for value in &grid.luma {
            histogram[usize::from(*value)] += 1;
        }
        Self {
            time,
            grid,
            histogram,
        }
    }
}

/// Everything the scene needs to draw: frames oldest first plus the luminance
/// levels (0..1) used for automatic contrast across the whole set.
pub struct FrameSet {
    pub name: &'static str,
    pub bbox: GeoBox,
    pub frames: Vec<Arc<CloudFrame>>,
    pub black: f32,
    pub white: f32,
}

impl FrameSet {
    pub fn new(name: &'static str, bbox: GeoBox, mut frames: Vec<Arc<CloudFrame>>) -> Self {
        frames.sort_by_key(|frame| frame.time);
        let mut histogram = [0u64; 256];
        for frame in &frames {
            for (total, count) in histogram.iter_mut().zip(frame.histogram) {
                *total += u64::from(count);
            }
        }
        let (black, white) = levels(&histogram);
        Self {
            name,
            bbox,
            frames,
            black,
            white,
        }
    }
}

/// (5th, 99.5th) percentile as 0..1 with a minimum span, so a flat picture is
/// not stretched into noise.
fn levels(histogram: &[u64; 256]) -> (f32, f32) {
    let total: u64 = histogram.iter().sum();
    if total == 0 {
        return (0.0, 1.0);
    }
    let find = |fraction: f64| -> usize {
        let target = (total as f64 * fraction) as u64;
        let mut seen = 0;
        for (value, count) in histogram.iter().enumerate() {
            seen += count;
            if seen > target {
                return value;
            }
        }
        255
    };
    let black = find(0.05) as f32 / 255.0;
    let white = (find(0.995) as f32 / 255.0).max(black + 0.125);
    (black, white.min(1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_ignore_outliers_and_keep_a_minimum_span() {
        let mut histogram = [0u64; 256];
        histogram[40] = 900;
        histogram[200] = 99;
        histogram[255] = 1;
        let (black, white) = levels(&histogram);
        assert!((black - 40.0 / 255.0).abs() < 1e-6);
        assert!((white - 200.0 / 255.0).abs() < 0.01, "{white}");
        // A flat image gets at least a small span instead of a divide by zero.
        let mut flat = [0u64; 256];
        flat[100] = 1000;
        let (black, white) = levels(&flat);
        assert!(white - black >= 0.05, "{black} {white}");
        assert_eq!(levels(&[0; 256]), (0.0, 1.0));
    }

    #[test]
    fn frame_sets_are_sorted_and_summarised() {
        let bbox = GeoBox::WORLD;
        let dark = GeoGrid::blank(bbox, 4, 2, false);
        let mut bright = GeoGrid::blank(bbox, 4, 2, false);
        bright.luma.fill(255);
        let newer = Arc::new(CloudFrame::new(UtcTime(2000), bright));
        let older = Arc::new(CloudFrame::new(UtcTime(1000), dark));
        let set = FrameSet::new("test", bbox, vec![newer, older]);
        assert_eq!(set.frames[0].time, UtcTime(1000));
        assert_eq!(set.frames[1].time, UtcTime(2000));
        assert!(set.white > set.black);
    }
}

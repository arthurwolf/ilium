//! Saved-chunk coverage. Missing terrain is never replaced by generated blocks.
use std::collections::BTreeSet;

#[derive(Clone, Debug, Default)]
pub struct Coverage {
    pub chunks: BTreeSet<[i32; 2]>,
}

impl Coverage {
    /// All chunks intersecting the conservative square viewport must be saved.
    pub fn contains_view(&self, center: [f64; 2], radius: f64) -> bool {
        if !radius.is_finite() || !(0.0..=4096.0).contains(&radius) {
            return false;
        }
        self.contains_bounds(center.map(|v| v - radius), center.map(|v| v + radius))
    }

    fn contains_bounds(&self, minimum: [f64; 2], maximum: [f64; 2]) -> bool {
        let lower = minimum.map(|value| (value / 16.0).floor());
        let upper = maximum.map(|value| (value / 16.0).floor());
        if lower.into_iter().chain(upper).any(|value| {
            !value.is_finite() || value < f64::from(i32::MIN) || value > f64::from(i32::MAX)
        }) {
            return false;
        }
        let lower = lower.map(|value| value as i32);
        let upper = upper.map(|value| value as i32);
        let spans = std::array::from_fn::<_, 2, _>(|axis| {
            i64::from(upper[axis]) - i64::from(lower[axis]) + 1
        });
        if spans.iter().any(|span| !(1..=512).contains(span)) || spans[0] * spans[1] > 65536 {
            return false;
        }
        (lower[1]..=upper[1]).all(|z| (lower[0]..=upper[0]).all(|x| self.chunks.contains(&[x, z])))
    }

    /// A straight camera tour through a target, with viewport clearance.
    pub fn line_through(
        &self,
        target: [f64; 2],
        direction: [f64; 2],
        radius: f64,
        maximum_length: f64,
    ) -> Option<Line> {
        let norm = direction[0].hypot(direction[1]);
        if !norm.is_finite()
            || norm < 1e-12
            || !maximum_length.is_finite()
            || maximum_length <= 0.0
            || !self.contains_view(target, radius)
        {
            return None;
        }
        let direction = direction.map(|value| value / norm);
        let half = maximum_length.min(8192.0) / 2.0;
        let extend = |sign: f64| {
            let mut last = target;
            let steps = (half / 4.0).ceil() as usize;
            for step in 1..=steps {
                let distance = (step as f64 * 4.0).min(half);
                let next =
                    std::array::from_fn(|axis| target[axis] + direction[axis] * distance * sign);
                // Test the entire swept rectangle, not just sampled centers.
                // A diagonal can clip an unsaved corner between two samples.
                let lower = std::array::from_fn(|axis| last[axis].min(next[axis]) - radius);
                let upper = std::array::from_fn(|axis| last[axis].max(next[axis]) + radius);
                if !self.contains_bounds(lower, upper) {
                    break;
                }
                last = next;
            }
            last
        };
        let line = Line {
            start: extend(-1.0),
            end: extend(1.0),
        };
        (line.length() >= 48.0).then_some(line)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Line {
    pub start: [f64; 2],
    pub end: [f64; 2],
}
impl Line {
    pub fn point(self, fraction: f64) -> [f64; 2] {
        let t = fraction.clamp(0.0, 1.0);
        std::array::from_fn(|axis| self.start[axis] + (self.end[axis] - self.start[axis]) * t)
    }
    pub fn length(self) -> f64 {
        (self.end[0] - self.start[0]).hypot(self.end[1] - self.start[1])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn rectangle() -> Coverage {
        Coverage {
            chunks: (-12..=12)
                .flat_map(|z| (-20..=20).map(move |x| [x, z]))
                .collect(),
        }
    }
    #[test]
    fn negative_edges_and_missing_halo_are_exact() {
        let coverage = Coverage {
            chunks: [[-1, -1]].into(),
        };
        assert!(coverage.contains_view([-8.0, -8.0], 7.9));
        assert!(!coverage.contains_view([-8.0, -8.0], 8.0));
        assert!(!coverage.contains_view([0.0, 0.0], 0.0));
        assert!(!coverage.contains_view([f64::NAN, 0.0], 1.0));
        assert!(!coverage.contains_view([0.0, 0.0], -1.0));
        assert!(!coverage.contains_view([0.0, 0.0], f64::INFINITY));
    }
    #[test]
    fn long_line_never_leaves_saved_viewport_or_crosses_an_unsaved_island() {
        let mut coverage = rectangle();
        for z in -12..=12 {
            coverage.chunks.remove(&[5, z]);
        }
        let line = coverage
            .line_through([0.0, 0.0], [1.0, 0.4], 24.0, 512.0)
            .unwrap();
        assert!(line.length() > 160.0);
        for step in 0..=10000 {
            assert!(coverage.contains_view(line.point(f64::from(step) / 10000.0), 24.0));
        }
        assert!(line.end[0] < 56.0);
        assert!(coverage
            .line_through([90.0, 0.0], [1.0, 0.0], 24.0, 512.0)
            .is_none());
    }
    #[test]
    fn narrow_diagonal_gaps_are_not_skipped_between_camera_samples() {
        let mut coverage = rectangle();
        coverage.chunks.remove(&[1, 0]);
        let line = coverage
            .line_through([0.0, 0.0], [1.0, 0.997], 0.0, 256.0)
            .unwrap();
        // The camera crosses x=16 just before z=16, touching a tiny gap.
        assert!(line.end[0] < 16.0);
        for step in 0..=10000 {
            assert!(coverage.contains_view(line.point(f64::from(step) / 10000.0), 0.0));
        }
    }

    #[test]
    fn invalid_direction_and_huge_requests_are_bounded() {
        let coverage = rectangle();
        assert!(coverage
            .line_through([0.0, 0.0], [0.0, 0.0], 1.0, 200.0)
            .is_none());
        assert!(coverage
            .line_through([0.0, 0.0], [f64::INFINITY, 0.0], 1.0, 200.0)
            .is_none());
        assert!(!coverage.contains_view([0.0, 0.0], 1e30));
    }
}

//! A fixed-orientation quarter-galaxy viewport whose center orbits clockwise.
use std::f32::consts::{PI, TAU};

pub(super) struct Camera {
    pub center: (f32, f32),
    pub extent: (f32, f32),
}
impl Camera {
    pub fn new(aspect: f32, seconds: f64, speed: i32) -> Self {
        let aspect = aspect.max(0.001);
        let width = (PI * 0.25 * aspect).sqrt();
        let height = width / aspect;
        // Adaptive radius is essential: a constant radius misses the galaxy's
        // north/south rim on a wide terminal. This radius covers the unit disk
        // over a complete orbit while the rectangle's area remains pi / 4.
        let radius = (1.0 - width.min(height) * 0.5).max(0.0);
        let phase = ((seconds * f64::from(speed) / 100.0 / 900.0) % 1.0) as f32 * TAU;
        Self {
            center: (radius * phase.cos(), radius * phase.sin()),
            extent: (width, height),
        }
    }
    pub fn world(&self, u: f32, v: f32) -> (f32, f32) {
        (
            self.center.0 + (u - 0.5) * self.extent.0,
            self.center.1 + (v - 0.5) * self.extent.1,
        )
    }
    pub fn project(&self, position: (f32, f32)) -> (f32, f32) {
        (
            (position.0 - self.center.0) / self.extent.0 + 0.5,
            (position.1 - self.center.1) / self.extent.1 + 0.5,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quarter_area_clockwise_orbit_and_full_disk_coverage_at_many_aspects() {
        for aspect in [0.25, 0.5, 1.0, 1.5, 2.0, 3.0, 4.0, 8.0] {
            let camera = Camera::new(aspect, 0.0, 100);
            assert!((camera.extent.0 * camera.extent.1 - PI * 0.25).abs() < 0.00001);
            assert!(Camera::new(aspect, 225.0, 100).center.1 > 0.0);
            for radial_step in 0..=10 {
                for angle_step in 0..72 {
                    let radius = radial_step as f32 / 10.0;
                    let angle = angle_step as f32 * TAU / 72.0;
                    let point = (radius * angle.cos(), radius * angle.sin());
                    assert!(
                        (0..1440).any(|step| {
                            let view = Camera::new(aspect, step as f64 * 900.0 / 1440.0, 100);
                            let (x, y) = view.project(point);
                            (-0.001..=1.001).contains(&x) && (-0.001..=1.001).contains(&y)
                        }),
                        "unreachable point {point:?} at aspect {aspect}"
                    );
                }
            }
        }
    }
    #[test]
    fn camera_can_be_held_without_stopping_simulation_clock() {
        assert_eq!(
            Camera::new(2.0, 0.0, 0).center,
            Camera::new(2.0, 1234.0, 0).center
        );
    }
}

//! Sky-to-panel projections. All directions are unit vectors in the local
//! horizon frame (east, north, up); panel positions are Braille dot
//! coordinates with x to the right and y downwards (dot centres at +0.5).

use super::astro::{altitude_azimuth_vector, cross, dot, normalize, Vec3};
use super::settings::{Lens, Projection, StarsSettings};
use std::f64::consts::PI;

/// Fraction of the panel height at which the panorama horizon sits.
const PANORAMA_HORIZON: f64 = 0.94;
/// Dots kept free around the dome so its horizon ring is not clipped.
const DOME_MARGIN: f64 = 4.5;

#[derive(Debug, Clone)]
pub struct View {
    projection: Projection,
    lens: Lens,
    width: f64,
    height: f64,
    center_x: f64,
    center_y: f64,
    /// Dots per radian (dome/patch) or per degree (panorama).
    scale: f64,
    scale_y: f64,
    forward: Vec3,
    right: Vec3,
    up: Vec3,
    panorama_azimuth: f64,
    horizon_y: f64,
}

/// Radial distance of the lens for an angle `theta` from the view axis.
fn lens_radius(lens: Lens, theta: f64) -> f64 {
    match lens {
        Lens::Stereographic => 2.0 * (theta / 2.0).tan(),
        Lens::Equidistant => theta,
    }
}

fn lens_angle(lens: Lens, radius: f64) -> f64 {
    match lens {
        Lens::Stereographic => 2.0 * (radius / 2.0).atan(),
        Lens::Equidistant => radius,
    }
}

impl View {
    /// Build the view for a panel of `width` x `height` dots.
    pub fn new(settings: &StarsSettings, width: usize, height: usize) -> Self {
        let (width, height) = (width.max(1) as f64, height.max(1) as f64);
        let field_of_view = f64::from(settings.field_of_view_degrees.clamp(10, 180));
        let azimuth = f64::from(settings.look_azimuth_degrees);
        // The dome is a patch aimed at the zenith whose "up" is the chosen bearing.
        let altitude = match settings.projection {
            Projection::Patch => f64::from(settings.look_altitude_degrees.clamp(0, 90)),
            _ => 90.0,
        };
        let forward = altitude_azimuth_vector(altitude, azimuth);
        let zenith = [0.0, 0.0, 1.0];
        let up = if forward[2].abs() > 0.999_999 {
            altitude_azimuth_vector(0.0, azimuth)
        } else {
            normalize([
                zenith[0] - forward[2] * forward[0],
                zenith[1] - forward[2] * forward[1],
                zenith[2] - forward[2] * forward[2],
            ])
        };
        let right = cross(&forward, &up);
        let half_field = (field_of_view / 2.0).to_radians();
        let scale = match settings.projection {
            Projection::Dome => {
                let extent = (width.min(height) / 2.0 - DOME_MARGIN).max(1.0);
                extent / lens_radius(settings.lens, half_field)
            }
            Projection::Patch => (width / 2.0) / lens_radius(settings.lens, half_field),
            Projection::Panorama => width / field_of_view,
        };
        let (scale, scale_y) = if !settings.horizon {
            match settings.projection {
                Projection::Panorama => (scale, height / 180.0),
                _ => (
                    (width / 2.0) / lens_radius(settings.lens, half_field),
                    (height / 2.0) / lens_radius(settings.lens, half_field),
                ),
            }
        } else {
            (scale, scale)
        };
        Self {
            projection: settings.projection,
            lens: settings.lens,
            width,
            height,
            center_x: width / 2.0,
            center_y: height / 2.0,
            scale,
            scale_y,
            forward,
            right,
            up,
            panorama_azimuth: azimuth,
            horizon_y: height
                * if settings.horizon {
                    PANORAMA_HORIZON
                } else {
                    0.5
                },
        }
    }

    /// Approximate dots per degree at the centre of the view.
    pub fn dots_per_degree(&self) -> f64 {
        match self.projection {
            Projection::Panorama => self.scale,
            _ => self.scale * PI / 180.0,
        }
    }

    /// Panel position of a direction, or `None` when it cannot be shown (behind
    /// the lens, or below the ground for the panorama's altitude axis).
    pub fn project(&self, direction: &Vec3) -> Option<(f64, f64)> {
        match self.projection {
            Projection::Panorama => {
                let azimuth = direction[0].atan2(direction[1]).to_degrees();
                let altitude = direction[2].clamp(-1.0, 1.0).asin().to_degrees();
                let delta = (azimuth - self.panorama_azimuth + 540.0).rem_euclid(360.0) - 180.0;
                Some((
                    self.center_x + delta * self.scale,
                    self.horizon_y - altitude * self.scale_y,
                ))
            }
            _ => {
                let cosine = dot(direction, &self.forward).clamp(-1.0, 1.0);
                let theta = cosine.acos();
                if theta > 0.98 * PI {
                    return None;
                }
                let along_right = dot(direction, &self.right);
                let along_up = dot(direction, &self.up);
                let planar = along_right.hypot(along_up);
                if planar < 1e-12 {
                    return Some((self.center_x, self.center_y));
                }
                let radius = lens_radius(self.lens, theta) * self.scale;
                Some((
                    self.center_x + radius * along_right / planar,
                    self.center_y - radius * self.scale_y / self.scale * along_up / planar,
                ))
            }
        }
    }

    /// Direction seen through a panel position (inverse of `project`).
    pub fn unproject(&self, x: f64, y: f64) -> Option<Vec3> {
        match self.projection {
            Projection::Panorama => {
                let azimuth = self.panorama_azimuth + (x - self.center_x) / self.scale;
                let altitude = (self.horizon_y - y) / self.scale_y;
                if !(-90.0..=90.0).contains(&altitude) {
                    return None;
                }
                Some(altitude_azimuth_vector(altitude, azimuth))
            }
            _ => {
                let (dx, dy) = (
                    (x - self.center_x) / self.scale,
                    (self.center_y - y) / self.scale_y,
                );
                let radius = dx.hypot(dy);
                let theta = lens_angle(self.lens, radius);
                if theta > PI {
                    return None;
                }
                let (sin_theta, cos_theta) = theta.sin_cos();
                let (along_right, along_up) = if radius < 1e-12 {
                    (0.0, 0.0)
                } else {
                    (dx / radius * sin_theta, dy / radius * sin_theta)
                };
                Some([
                    self.forward[0] * cos_theta
                        + self.right[0] * along_right
                        + self.up[0] * along_up,
                    self.forward[1] * cos_theta
                        + self.right[1] * along_right
                        + self.up[1] * along_up,
                    self.forward[2] * cos_theta
                        + self.right[2] * along_right
                        + self.up[2] * along_up,
                ])
            }
        }
    }

    pub fn contains(&self, x: f64, y: f64, margin: f64) -> bool {
        x >= -margin && y >= -margin && x < self.width + margin && y < self.height + margin
    }

    /// Screen-space unit vector pointing "up the sky" (towards the zenith) at a
    /// horizon point, used to orient compass ticks.
    pub fn inward_direction(&self, azimuth_deg: f64) -> Option<(f64, f64)> {
        let base = self.project(&altitude_azimuth_vector(0.0, azimuth_deg))?;
        let inner = self.project(&altitude_azimuth_vector(4.0, azimuth_deg))?;
        let (dx, dy) = (inner.0 - base.0, inner.1 - base.1);
        let length = dx.hypot(dy);
        (length > 1e-9).then(|| (dx / length, dy / length))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenes::stars::astro::{altitude_azimuth_vector, enu_to_altitude_azimuth};

    fn view(projection: Projection, lens: Lens, azimuth: i32, altitude: i32, fov: i32) -> View {
        View::new(
            &StarsSettings {
                projection,
                lens,
                look_azimuth_degrees: azimuth,
                look_altitude_degrees: altitude,
                field_of_view_degrees: fov,
                ..StarsSettings::default()
            },
            160,
            96,
        )
    }

    #[test]
    fn dome_puts_the_zenith_in_the_centre_and_the_horizon_on_the_ring() {
        for lens in [Lens::Stereographic, Lens::Equidistant] {
            let dome = view(Projection::Dome, lens, 0, 50, 180);
            let (x, y) = dome.project(&[0.0, 0.0, 1.0]).unwrap();
            assert!((x - 80.0).abs() < 1e-9 && (y - 48.0).abs() < 1e-9);
            let radius = 48.0 - DOME_MARGIN;
            for azimuth in [0.0, 37.0, 90.0, 200.0, 315.0] {
                let (x, y) = dome
                    .project(&altitude_azimuth_vector(0.0, azimuth))
                    .unwrap();
                let distance = (x - 80.0).hypot(y - 48.0);
                assert!(
                    (distance - radius).abs() < 1e-6,
                    "{lens:?} az {azimuth}: {distance}"
                );
            }
        }
    }

    #[test]
    fn dome_has_north_up_and_east_on_the_left() {
        let dome = view(Projection::Dome, Lens::Stereographic, 0, 50, 180);
        let north = dome.project(&altitude_azimuth_vector(30.0, 0.0)).unwrap();
        let east = dome.project(&altitude_azimuth_vector(30.0, 90.0)).unwrap();
        let south = dome.project(&altitude_azimuth_vector(30.0, 180.0)).unwrap();
        let west = dome.project(&altitude_azimuth_vector(30.0, 270.0)).unwrap();
        assert!(north.1 < 48.0 && (north.0 - 80.0).abs() < 1e-6);
        assert!(south.1 > 48.0);
        assert!(
            east.0 < 80.0,
            "east is left when looking up with north on top"
        );
        assert!(west.0 > 80.0);
    }

    #[test]
    fn dome_top_follows_the_chosen_bearing() {
        let dome = view(Projection::Dome, Lens::Equidistant, 90, 50, 180);
        let east = dome.project(&altitude_azimuth_vector(30.0, 90.0)).unwrap();
        assert!(east.1 < 48.0 && (east.0 - 80.0).abs() < 1e-6);
    }

    #[test]
    fn panorama_is_centred_on_the_bearing_with_east_on_the_right() {
        let panorama = view(Projection::Panorama, Lens::Stereographic, 0, 0, 120);
        let straight = panorama
            .project(&altitude_azimuth_vector(20.0, 0.0))
            .unwrap();
        assert!((straight.0 - 80.0).abs() < 1e-9);
        let east = panorama
            .project(&altitude_azimuth_vector(20.0, 30.0))
            .unwrap();
        let west = panorama
            .project(&altitude_azimuth_vector(20.0, 330.0))
            .unwrap();
        assert!(east.0 > 80.0 && west.0 < 80.0);
        assert!((east.0 - 80.0 - (80.0 - west.0)).abs() < 1e-9);
        // 160 dots for 120 degrees: 4/3 dot per degree; altitude goes up the panel.
        assert!((east.0 - 80.0 - 30.0 * 160.0 / 120.0).abs() < 1e-9);
        let horizon = panorama
            .project(&altitude_azimuth_vector(0.0, 0.0))
            .unwrap();
        assert!((horizon.1 - 96.0 * PANORAMA_HORIZON).abs() < 1e-9 && straight.1 < horizon.1);
        // Facing south, north wraps to the far edge without jumping into view.
        let south = view(Projection::Panorama, Lens::Stereographic, 180, 0, 90);
        let north = south.project(&altitude_azimuth_vector(10.0, 0.0)).unwrap();
        assert!(!south.contains(north.0, north.1, 0.0));
    }

    #[test]
    fn patch_is_centred_on_the_aim_and_zooms() {
        let wide = view(Projection::Patch, Lens::Stereographic, 180, 45, 90);
        let aim = altitude_azimuth_vector(45.0, 180.0);
        let (x, y) = wide.project(&aim).unwrap();
        assert!((x - 80.0).abs() < 1e-9 && (y - 48.0).abs() < 1e-9);
        let off = altitude_azimuth_vector(45.0, 200.0);
        let narrow = view(Projection::Patch, Lens::Stereographic, 180, 45, 30);
        let d_wide = wide.project(&off).unwrap().0 - 80.0;
        let d_narrow = narrow.project(&off).unwrap().0 - 80.0;
        assert!(d_narrow.abs() > 2.0 * d_wide.abs());
        // Edge of the field: half the field of view maps to half the panel width.
        let edge = altitude_azimuth_vector(45.0, 180.0);
        let sideways = wide.project(&edge).unwrap();
        assert!(sideways.0 > 0.0);
        // Right of the view axis when facing south is west.
        let west = wide.project(&altitude_azimuth_vector(45.0, 200.0)).unwrap();
        assert!(west.0 > 80.0, "azimuth 200 lies to the west of south");
        // Patch aimed at the zenith uses the bearing as up.
        let zenith = view(Projection::Patch, Lens::Equidistant, 270, 90, 90);
        let west_star = zenith
            .project(&altitude_azimuth_vector(60.0, 270.0))
            .unwrap();
        assert!(west_star.1 < 48.0);
    }

    #[test]
    fn unproject_inverts_project_everywhere() {
        let cases = [
            view(Projection::Dome, Lens::Stereographic, 0, 50, 180),
            view(Projection::Dome, Lens::Equidistant, 45, 50, 120),
            view(Projection::Patch, Lens::Stereographic, 200, 30, 60),
            view(Projection::Patch, Lens::Equidistant, 10, 80, 140),
            view(Projection::Panorama, Lens::Stereographic, 123, 0, 100),
        ];
        for view in &cases {
            for &(x, y) in &[
                (10.5, 10.5),
                (80.5, 48.5),
                (150.5, 80.5),
                (30.5, 70.5),
                (120.5, 20.5),
            ] {
                let Some(direction) = view.unproject(x, y) else {
                    continue;
                };
                assert!((dot(&direction, &direction) - 1.0).abs() < 1e-9);
                let (back_x, back_y) = view.project(&direction).unwrap();
                assert!(
                    (back_x - x).abs() < 1e-6 && (back_y - y).abs() < 1e-6,
                    "{view:?} {x},{y}"
                );
            }
        }
    }

    #[test]
    fn behind_the_lens_is_not_projected() {
        let patch = view(Projection::Patch, Lens::Stereographic, 0, 45, 90);
        let behind = altitude_azimuth_vector(-45.0, 180.0);
        assert!(patch.project(&behind).is_none());
        let (alt, _) = enu_to_altitude_azimuth(&behind);
        assert!(alt < 0.0);
    }

    #[test]
    fn inward_direction_points_towards_the_zenith() {
        let dome = view(Projection::Dome, Lens::Stereographic, 0, 50, 180);
        let (dx, dy) = dome.inward_direction(0.0).unwrap();
        assert!(
            dx.abs() < 1e-6 && dy > 0.9,
            "north tick points down towards the centre"
        );
        let panorama = view(Projection::Panorama, Lens::Stereographic, 0, 0, 180);
        let (dx, dy) = panorama.inward_direction(0.0).unwrap();
        assert!(
            dx.abs() < 1e-6 && dy < -0.9,
            "panorama ticks point up the panel"
        );
    }
}

#[cfg(test)]
mod overhaul_regression_tests {
    use super::*;
    #[test]
    fn horizonless_panorama_spans_both_hemispheres() {
        let view = View::new(
            &StarsSettings {
                horizon: false,
                projection: Projection::Panorama,
                ..StarsSettings::default()
            },
            160,
            96,
        );
        let direction = view.unproject(80.0, 80.0).unwrap();
        assert!(direction[2] < 0.0);
        let back = view.project(&direction).unwrap();
        assert!((back.1 - 80.0).abs() < 1e-6);
    }
    #[test]
    fn horizonless_dome_corners_are_below_horizon_and_roundtrip() {
        for lens in [Lens::Stereographic, Lens::Equidistant] {
            let view = View::new(
                &StarsSettings {
                    horizon: false,
                    lens,
                    ..StarsSettings::default()
                },
                320,
                96,
            );
            let direction = view.unproject(1.0, 1.0).unwrap();
            assert!(direction[2] < 0.0);
            let back = view.project(&direction).unwrap();
            assert!((back.0 - 1.0).abs() < 1e-6 && (back.1 - 1.0).abs() < 1e-6);
        }
    }
}

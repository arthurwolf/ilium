//! Map projections from screen dots to longitude/latitude, and the Sun's
//! position for the day/night terminator. Pure math, no I/O.
//!
//! Terminal dots are approximately square, so all three projections work in
//! square "dot" units.

use serde::{Deserialize, Serialize};
use std::f64::consts::PI;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Projection {
    /// Equirectangular flat map; wraps around horizontally to fill the width.
    #[default]
    Flat,
    /// Orthographic globe centred on the view.
    Globe,
    /// Mollweide equal-area ellipse.
    Mollweide,
}

impl Projection {
    pub const LABELS: [&'static str; 3] = ["Flat map", "Globe", "Mollweide"];

    pub fn index(self) -> usize {
        match self {
            Self::Flat => 0,
            Self::Globe => 1,
            Self::Mollweide => 2,
        }
    }

    pub fn from_index(index: usize) -> Self {
        match index {
            1 => Self::Globe,
            2 => Self::Mollweide,
            _ => Self::Flat,
        }
    }
}

/// Longitude/latitude under every dot of a raster (row-major), `None` off the map.
pub type DotTable = Vec<Option<(f64, f64)>>;

/// Where the camera looks and how large the world is drawn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    pub projection: Projection,
    /// Longitude at the screen centre, degrees.
    pub center_lon: f64,
    /// Latitude at the screen centre (globe) or of the pan target (flat and
    /// Mollweide, blended in as `zoom` grows), degrees.
    pub center_lat: f64,
    /// 1.0 fits the whole world; larger values magnify.
    pub zoom: f64,
    pub dots_w: usize,
    pub dots_h: usize,
}

/// Wrap an angle difference in degrees into -180..180.
pub fn wrap_degrees(angle: f64) -> f64 {
    (angle + 180.0).rem_euclid(360.0) - 180.0
}

/// Solve 2t + sin 2t = pi sin(lat) for the Mollweide auxiliary angle t.
fn mollweide_theta(lat_radians: f64) -> f64 {
    if (lat_radians.abs() - PI / 2.0).abs() < 1e-9 {
        return lat_radians.signum() * PI / 2.0;
    }
    let target = PI * lat_radians.sin();
    let mut theta = lat_radians;
    for _ in 0..12 {
        let delta =
            (2.0 * theta + (2.0 * theta).sin() - target) / (2.0 + 2.0 * (2.0 * theta).cos());
        theta -= delta;
        if delta.abs() < 1e-10 {
            break;
        }
    }
    theta
}

impl View {
    fn half_width(&self) -> f64 {
        self.dots_w as f64 / 2.0
    }

    fn half_height(&self) -> f64 {
        self.dots_h as f64 / 2.0
    }

    /// Height in dots of the whole flat/Mollweide map at the current zoom.
    fn map_height(&self) -> f64 {
        (self.dots_h as f64).min(self.dots_w as f64 / 2.0).max(1.0) * self.zoom.max(0.05)
    }

    /// Radius in dots of the globe.
    fn globe_radius(&self) -> f64 {
        (self.dots_w.min(self.dots_h) as f64 / 2.0 * 0.94 * self.zoom.max(0.05)).max(1.0)
    }

    fn pan_latitude(&self) -> f64 {
        self.center_lat * (1.0 - 1.0 / self.zoom.max(1.0))
    }

    /// Approximate degrees of arc per dot at the screen centre.
    pub fn degrees_per_dot(&self) -> f64 {
        match self.projection {
            Projection::Flat | Projection::Mollweide => 180.0 / self.map_height(),
            Projection::Globe => 90.0 / self.globe_radius(),
        }
    }

    /// Distance of dot (x, y) from the globe centre in globe radii.
    fn globe_rho(&self, x: usize, y: usize) -> f64 {
        let dx = x as f64 + 0.5 - self.half_width();
        let dy = self.half_height() - (y as f64 + 0.5);
        (dx * dx + dy * dy).sqrt() / self.globe_radius()
    }

    /// True for dots on the thin rim just outside the globe's limb.
    pub fn globe_ring(&self, x: usize, y: usize) -> bool {
        self.projection == Projection::Globe && (1.0..1.035).contains(&self.globe_rho(x, y))
    }

    /// How many more map degrees one dot covers than at the centre (grows
    /// towards the globe's limb, where the surface is seen at a slant).
    pub fn limb_stretch(&self, x: usize, y: usize) -> f64 {
        if self.projection != Projection::Globe {
            return 1.0;
        }
        let rho = self.globe_rho(x, y).min(0.97);
        (1.0 / (1.0 - rho * rho).sqrt()).clamp(1.0, 4.0)
    }

    /// Longitude/latitude under the centre of dot (x, y), `None` off the map.
    pub fn locate(&self, x: usize, y: usize) -> Option<(f64, f64)> {
        let dx = x as f64 + 0.5 - self.half_width();
        let dy = self.half_height() - (y as f64 + 0.5);
        match self.projection {
            Projection::Flat => {
                let degrees = self.degrees_per_dot();
                let lat = self.pan_latitude() + dy * degrees;
                if lat.abs() > 90.0 {
                    return None;
                }
                Some((wrap_degrees(self.center_lon + dx * degrees), lat))
            }
            Projection::Mollweide => {
                let map_height = self.map_height();
                let pan_y = mollweide_theta(self.pan_latitude().to_radians()).sin();
                let y_norm = pan_y + dy / (map_height / 2.0);
                if y_norm.abs() > 1.0 {
                    return None;
                }
                let theta = y_norm.asin();
                let cos_theta = theta.cos();
                if cos_theta < 1e-9 {
                    return None;
                }
                let x_norm = dx / map_height;
                let lambda = PI * x_norm / cos_theta;
                if lambda.abs() > PI {
                    return None;
                }
                let lat = ((2.0 * theta + (2.0 * theta).sin()) / PI)
                    .clamp(-1.0, 1.0)
                    .asin();
                Some((
                    wrap_degrees(self.center_lon + lambda.to_degrees()),
                    lat.to_degrees(),
                ))
            }
            Projection::Globe => {
                let radius = self.globe_radius();
                let (nx, ny) = (dx / radius, dy / radius);
                let rho = (nx * nx + ny * ny).sqrt();
                if rho > 1.0 {
                    return None;
                }
                let lat0 = self.center_lat.to_radians();
                if rho < 1e-9 {
                    return Some((wrap_degrees(self.center_lon), self.center_lat));
                }
                let c = rho.asin();
                let lat = (c.cos() * lat0.sin() + ny * c.sin() * lat0.cos() / rho)
                    .clamp(-1.0, 1.0)
                    .asin();
                let lon = self.center_lon.to_radians()
                    + (nx * c.sin()).atan2(rho * lat0.cos() * c.cos() - ny * lat0.sin() * c.sin());
                Some((wrap_degrees(lon.to_degrees()), lat.to_degrees()))
            }
        }
    }

    /// Dot coordinates (fractional) of a point, `None` when it is hidden
    /// (far side of the globe, outside the ellipse).
    pub fn project(&self, lon: f64, lat: f64) -> Option<(f32, f32)> {
        let relative = wrap_degrees(lon - self.center_lon);
        match self.projection {
            Projection::Flat => {
                let degrees = self.degrees_per_dot();
                let x = self.half_width() + relative / degrees;
                let y = self.half_height() - (lat - self.pan_latitude()) / degrees;
                Some((x as f32, y as f32))
            }
            Projection::Mollweide => {
                let map_height = self.map_height();
                let theta = mollweide_theta(lat.to_radians());
                let pan_y = mollweide_theta(self.pan_latitude().to_radians()).sin();
                let x_norm = relative.to_radians() * theta.cos() / PI;
                let x = self.half_width() + x_norm * map_height;
                let y = self.half_height() - (theta.sin() - pan_y) * map_height / 2.0;
                Some((x as f32, y as f32))
            }
            Projection::Globe => {
                let lat0 = self.center_lat.to_radians();
                let (phi, lambda) = (lat.to_radians(), relative.to_radians());
                let visible = lat0.sin() * phi.sin() + lat0.cos() * phi.cos() * lambda.cos();
                if visible <= 0.02 {
                    return None;
                }
                let radius = self.globe_radius();
                let x = radius * phi.cos() * lambda.sin();
                let y = radius * (lat0.cos() * phi.sin() - lat0.sin() * phi.cos() * lambda.cos());
                Some((
                    (self.half_width() + x) as f32,
                    (self.half_height() - y) as f32,
                ))
            }
        }
    }
}

/// A blinking dot with a ring, drawn into `dots` (row-major, `size` = width,
/// height) at `centre`. Blinks with a 1.2 s period, on for 0.7 s.
pub fn draw_marker(dots: &mut [f32], size: (usize, usize), centre: (f32, f32), wall_seconds: f32) {
    if wall_seconds.rem_euclid(1.2) >= 0.7 {
        return;
    }
    let (width, height) = size;
    let (cx, cy) = centre;
    let reach = 4;
    let (x0, y0) = (cx as i32 - reach, cy as i32 - reach);
    for y in y0.max(0)..(y0 + 2 * reach + 1).min(height as i32) {
        for x in x0.max(0)..(x0 + 2 * reach + 1).min(width as i32) {
            let distance = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
            let core = 1.0 - crate::raster::smoothstep(1.2, 2.0, distance);
            let ring = (1.0 - (distance - 3.4).abs() / 0.8).clamp(0.0, 1.0) * 0.75;
            let dot = &mut dots[y as usize * width + x as usize];
            *dot = dot.max(core.max(ring));
        }
    }
}

/// Faint 30-degree graticule used as a placeholder before imagery arrives.
pub fn graticule_value(lon: f64, lat: f64, degrees_per_dot: f64) -> f32 {
    let distance = |value: f64| ((value + 15.0).rem_euclid(30.0) - 15.0).abs();
    let width = degrees_per_dot * 0.6;
    let on_meridian = distance(lon) < width;
    let on_parallel = distance(lat) < width;
    match (on_meridian, on_parallel) {
        (false, false) => 0.0,
        _ if lat.abs() < width || lon.abs() < width => 0.34,
        _ => 0.2,
    }
}

/// The Sun's sub-point on Earth.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SunPosition {
    /// Longitude directly below the Sun, degrees east.
    pub longitude: f64,
    /// Latitude directly below the Sun (solar declination), degrees.
    pub latitude: f64,
}

/// Low-precision solar position (about 0.01 degrees), enough for a terminator.
/// `unix_seconds` is UTC.
pub fn sun_position(unix_seconds: f64) -> SunPosition {
    let days = unix_seconds / 86_400.0 + 2_440_587.5 - 2_451_545.0;
    let mean_longitude = (280.460 + 0.985_647_4 * days).rem_euclid(360.0);
    let mean_anomaly = (357.528 + 0.985_600_3 * days)
        .rem_euclid(360.0)
        .to_radians();
    let ecliptic_longitude =
        (mean_longitude + 1.915 * mean_anomaly.sin() + 0.020 * (2.0 * mean_anomaly).sin())
            .to_radians();
    let obliquity = (23.439 - 0.000_000_4 * days).to_radians();
    let declination = (obliquity.sin() * ecliptic_longitude.sin()).asin();
    let right_ascension =
        (obliquity.cos() * ecliptic_longitude.sin()).atan2(ecliptic_longitude.cos());
    let sidereal = (280.460_618_37 + 360.985_647_366_29 * days).rem_euclid(360.0);
    SunPosition {
        longitude: wrap_degrees(right_ascension.to_degrees() - sidereal),
        latitude: declination.to_degrees(),
    }
}

impl SunPosition {
    /// Sine of the Sun's elevation above the horizon at a point.
    pub fn sin_elevation(&self, lon: f64, lat: f64) -> f64 {
        let (phi, delta) = (lat.to_radians(), self.latitude.to_radians());
        phi.sin() * delta.sin()
            + phi.cos() * delta.cos() * (lon - self.longitude).to_radians().cos()
    }

    /// 0 in full night, 1 in full day, smooth through twilight.
    pub fn daylight(&self, lon: f64, lat: f64) -> f32 {
        let elevation = self
            .sin_elevation(lon, lat)
            .clamp(-1.0, 1.0)
            .asin()
            .to_degrees();
        crate::raster::smoothstep(-8.0, 5.0, elevation as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::super::tiles::UtcTime;
    use super::*;

    fn view(projection: Projection, zoom: f64) -> View {
        View {
            projection,
            center_lon: 20.0,
            center_lat: 35.0,
            zoom,
            dots_w: 240,
            dots_h: 120,
        }
    }

    #[test]
    fn screen_centre_maps_to_the_view_centre() {
        for projection in [Projection::Flat, Projection::Globe, Projection::Mollweide] {
            let view = View {
                center_lat: 0.0,
                ..view(projection, 1.0)
            };
            let (lon, lat) = view.locate(120, 60).unwrap();
            assert!((lon - 20.0).abs() < 1.0, "{projection:?} lon {lon}");
            assert!(lat.abs() < 1.5, "{projection:?} lat {lat}");
        }
        let globe = view(Projection::Globe, 1.0);
        let (lon, lat) = globe.locate(120, 60).unwrap();
        assert!(
            (lon - 20.0).abs() < 2.0 && (lat - 35.0).abs() < 2.0,
            "{lon} {lat}"
        );
    }

    #[test]
    fn locate_and_project_round_trip() {
        for projection in [Projection::Flat, Projection::Globe, Projection::Mollweide] {
            for zoom in [1.0, 2.5] {
                let view = view(projection, zoom);
                let mut checked = 0;
                for (x, y) in [
                    (60usize, 30usize),
                    (120, 60),
                    (200, 90),
                    (100, 40),
                    (140, 100),
                ] {
                    let Some((lon, lat)) = view.locate(x, y) else {
                        continue;
                    };
                    let (px, py) = view.project(lon, lat).unwrap();
                    assert!(
                        (px - (x as f32 + 0.5)).abs() < 0.05,
                        "{projection:?} x {px} vs {x}"
                    );
                    assert!(
                        (py - (y as f32 + 0.5)).abs() < 0.05,
                        "{projection:?} y {py} vs {y}"
                    );
                    checked += 1;
                }
                assert!(checked >= 2, "{projection:?} zoom {zoom}");
            }
        }
    }

    #[test]
    fn globe_hides_the_far_side_and_the_corners() {
        let globe = view(Projection::Globe, 1.0);
        assert!(globe.locate(0, 0).is_none());
        assert!(globe.locate(239, 119).is_none());
        // The antipode of the view centre is not visible.
        assert!(globe.project(20.0 + 180.0, -35.0).is_none());
        assert!(globe.project(20.0, 35.0).is_some());
        // Edge dots are inside the sphere on the short axis: radius 0.94 * 60.
        assert!(globe.locate(120, 60 - 55).is_some());
        assert!(globe.locate(120, 60 - 59).is_none());
    }

    #[test]
    fn flat_map_wraps_horizontally_and_stops_at_the_poles() {
        let flat = View {
            center_lat: 0.0,
            ..view(Projection::Flat, 1.0)
        };
        // The whole world is 240 dots wide; the map is 120 dots high.
        assert!(flat.locate(0, 0).is_some());
        let (left_lon, _) = flat.locate(0, 60).unwrap();
        let (right_lon, _) = flat.locate(239, 60).unwrap();
        // The two edges are neighbours on the sphere: the map wraps around.
        assert!(
            wrap_degrees(left_lon - right_lon).abs() < 5.0,
            "{left_lon} {right_lon}"
        );
        let tall = View {
            dots_w: 240,
            dots_h: 200,
            center_lat: 0.0,
            ..view(Projection::Flat, 1.0)
        };
        assert!(tall.locate(120, 0).is_none(), "beyond the pole is empty");
        assert!(tall.locate(120, 100).is_some());
    }

    #[test]
    fn mollweide_is_an_ellipse_with_equal_area_scaling() {
        let view = View {
            center_lat: 0.0,
            center_lon: 0.0,
            ..view(Projection::Mollweide, 1.0)
        };
        assert!(view.locate(2, 2).is_none(), "corner outside ellipse");
        assert!(view.locate(120, 60).is_some());
        // Edge of the equator reaches +-180.
        let (lon, lat) = view.locate(1, 60).unwrap();
        assert!(lon.abs() > 170.0 && lat.abs() < 2.0, "{lon} {lat}");
        // theta solver: latitude 90 gives theta 90.
        assert!((mollweide_theta(PI / 2.0) - PI / 2.0).abs() < 1e-9);
        let theta = mollweide_theta(0.5);
        assert!((2.0 * theta + (2.0 * theta).sin() - PI * 0.5f64.sin()).abs() < 1e-8);
    }

    #[test]
    fn zoom_reduces_degrees_per_dot() {
        for projection in [Projection::Flat, Projection::Globe, Projection::Mollweide] {
            assert!(
                view(projection, 4.0).degrees_per_dot()
                    < view(projection, 1.0).degrees_per_dot() / 3.9,
                "{projection:?}"
            );
        }
    }

    #[test]
    fn projection_index_round_trip() {
        for projection in [Projection::Flat, Projection::Globe, Projection::Mollweide] {
            assert_eq!(Projection::from_index(projection.index()), projection);
        }
        assert_eq!(Projection::from_index(99), Projection::Flat);
    }

    #[test]
    fn sun_position_matches_known_instants() {
        // 2000-01-01 12:00 UTC: declination -23.0, equation of time -3.3 minutes
        // so the Sun is ~0.8 degrees east of Greenwich's noon meridian.
        let sun = sun_position(946_728_000.0);
        assert!((sun.latitude + 23.0).abs() < 0.3, "{sun:?}");
        assert!((sun.longitude - 0.8).abs() < 0.6, "{sun:?}");
        // 2026-09-30 12:00 UTC: declination about -2.9, equation of time +10 min.
        let sun = sun_position(UtcTime::from_civil(2026, 9, 30, 12, 0, 0).0 as f64);
        assert!((sun.latitude + 2.9).abs() < 0.4, "{sun:?}");
        assert!((sun.longitude + 2.5).abs() < 1.0, "{sun:?}");
        // March equinox 2026 (20 March ~14:46 UTC): declination near zero.
        let equinox = sun_position(UtcTime::from_civil(2026, 3, 20, 14, 46, 0).0 as f64);
        assert!(equinox.latitude.abs() < 0.2, "{equinox:?}");
    }

    #[test]
    fn daylight_is_full_below_the_sun_and_zero_opposite() {
        let sun = SunPosition {
            longitude: 10.0,
            latitude: 20.0,
        };
        assert!(sun.daylight(10.0, 20.0) > 0.99);
        assert!(sun.daylight(-170.0, -20.0) < 0.01);
        // The terminator (elevation 0) is in between.
        let twilight = sun.daylight(100.0, 0.0);
        assert!(twilight > 0.05 && twilight < 0.95, "{twilight}");
        assert!((sun.sin_elevation(10.0, 20.0) - 1.0).abs() < 1e-9);
    }
}

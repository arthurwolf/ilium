//! Screen dot to longitude/latitude, for the flat map and the globe.

use super::settings::Projection;

/// Where the view is looking and how large the world appears.
#[derive(Debug, Clone, Copy)]
pub struct Camera {
    pub projection: Projection,
    pub longitude: f64,
    pub latitude: f64,
    /// Dots per degree of longitude at the centre of the view.
    pub dots_per_degree: f64,
    pub width: f64,
    pub height: f64,
}

impl Camera {
    /// Dots per degree that fit the whole world at 100% zoom.
    pub fn fit_scale(projection: Projection, width: f64, height: f64) -> f64 {
        match projection {
            Projection::Flat => (width / 360.0).min(height / 180.0),
            // A globe spans 180 degrees across its disc diameter, which is
            // 2 * radius = 2 * dots_per_degree * 180 / pi.
            Projection::Globe => width.min(height) * 0.92 * std::f64::consts::PI / 360.0,
        }
    }

    /// Longitude and latitude (degrees) under the centre of dot `(x, y)`, or
    /// `None` outside the map or off the globe.
    pub fn locate(&self, x: f64, y: f64) -> Option<(f64, f64)> {
        let (dx, dy) = (x + 0.5 - self.width * 0.5, y + 0.5 - self.height * 0.5);
        match self.projection {
            Projection::Flat => {
                let latitude = self.latitude - dy / self.dots_per_degree;
                if !(-90.0..=90.0).contains(&latitude) {
                    return None;
                }
                Some((self.longitude + dx / self.dots_per_degree, latitude))
            }
            Projection::Globe => {
                let radius = self.dots_per_degree * 180.0 / std::f64::consts::PI;
                let (px, py) = (dx / radius, -dy / radius);
                let squared = px * px + py * py;
                if squared > 1.0 {
                    return None;
                }
                let depth = (1.0 - squared).sqrt();
                let tilt = self.latitude.to_radians();
                let latitude = (depth * tilt.sin() + py * tilt.cos())
                    .clamp(-1.0, 1.0)
                    .asin();
                let longitude =
                    self.longitude + px.atan2(depth * tilt.cos() - py * tilt.sin()).to_degrees();
                Some((longitude, latitude.to_degrees()))
            }
        }
    }
}

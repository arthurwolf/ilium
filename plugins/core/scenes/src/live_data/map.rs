//! Shared geographic projection and coastline ink for the three live maps.
use crate::{raster::Raster, worldmap};

pub fn project(longitude: f64, latitude: f64) -> Option<(f32, f32)> {
    if !longitude.is_finite() || !latitude.is_finite() || !(-90.0..=90.0).contains(&latitude) {
        return None;
    }
    let longitude = (longitude + 180.0).rem_euclid(360.0) - 180.0;
    Some((
        ((longitude + 180.0) / 360.0) as f32,
        ((90.0 - latitude) / 180.0) as f32,
    ))
}

/// Coastline edges, rather than filled land. Natural Earth public-domain
/// mask is already embedded by ilium; no tile service or map request needed.
pub fn coastline(raster: &mut Raster, intensity: f32) {
    if !intensity.is_finite() {
        return;
    }
    let intensity = intensity.clamp(0.0, 1.0);
    for row in 0..worldmap::HEIGHT {
        for column in 0..worldmap::WIDTH {
            let land = worldmap::land_dot(column, row);
            let x = column as f32 / worldmap::WIDTH as f32;
            let y = row as f32 / worldmap::HEIGHT as f32;
            let next_x = (column + 1) as f32 / worldmap::WIDTH as f32;
            let next_y = (row + 1) as f32 / worldmap::HEIGHT as f32;
            if land != worldmap::land_dot((column + 1) % worldmap::WIDTH, row) {
                raster.line((next_x, y), (next_x, next_y), 0.35, intensity);
            }
            if row + 1 < worldmap::HEIGHT && land != worldmap::land_dot(column, row + 1) {
                raster.line((x, next_y), (next_x, next_y), 0.35, intensity);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dateline_wraps_and_poles_stay_in_bounds() {
        assert_eq!(project(180.0, 0.0), project(-180.0, 0.0));
        assert_eq!(project(540.0, 90.0), Some((0.0, 0.0)));
        assert!(project(f64::NAN, 0.0).is_none());
        assert!(project(0.0, 91.0).is_none());
    }
    #[test]
    fn coastlines_are_unfilled_and_nonempty_at_terminal_sizes() {
        let mut raster = Raster::default();
        raster.resize(160, 96);
        coastline(&mut raster, 0.5);
        let lit = raster.dots.iter().filter(|dot| **dot > 0.0).count();
        assert!(lit > 100 && lit < raster.dots.len() / 2, "lit {lit}");
        assert!(raster.dots.iter().all(|dot| dot.is_finite() && *dot <= 0.5));
    }
}

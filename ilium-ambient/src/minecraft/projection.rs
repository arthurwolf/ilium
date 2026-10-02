//! Ground footprint of the voxel renderer's fixed isometric projection.
//! Vertical bounds describe the actual admitted render volume, including model
//! overhang. This calculation cannot certify an omitted world volume or turn
//! missing saved cells into air.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Framing {
    pub size: [usize; 2],
    pub scale: f64,
    pub camera_height: f64,
    pub vertical_bounds: [f64; 2],
    /// Horizontal model overhang plus neighbor-data requirements, in blocks.
    pub horizontal_halo: f64,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("invalid isometric saved-volume framing")]
pub struct InvalidFraming;

impl Framing {
    /// Conservative square radius about the ground camera coordinate. Pixel
    /// edge corners are included, rather than only sample centers.
    pub fn radius(self) -> Result<f64, InvalidFraming> {
        if self.size.iter().any(|side| !(1..=65_536).contains(side))
            || self.size[0]
                .checked_mul(self.size[1])
                .is_none_or(|pixels| pixels > 1_048_576)
            || !self.scale.is_finite()
            || self.scale <= 0.0
            || !self.camera_height.is_finite()
            || self
                .vertical_bounds
                .iter()
                .any(|height| !height.is_finite())
            || self.vertical_bounds[0] > self.vertical_bounds[1]
            || !self.horizontal_halo.is_finite()
            || self.horizontal_halo < 0.0
        {
            return Err(InvalidFraming);
        }
        // Inverting surface_raster::project_quad gives:
        // dx = screen_y/scale + dz + screen_x/(2*cos(30)*scale),
        // dy = screen_y/scale + dz - screen_x/(2*cos(30)*scale).
        // Maximize absolute coordinates at all viewport/height endpoints.
        let vertical = self
            .vertical_bounds
            .iter()
            .map(|height| (*height - self.camera_height).abs())
            .fold(0.0_f64, f64::max);
        let radius = vertical
            + self.size[1] as f64 / (2.0 * self.scale)
            + self.size[0] as f64 / (4.0 * 0.8660254037844386 * self.scale)
            + self.horizontal_halo;
        if !radius.is_finite() {
            return Err(InvalidFraming);
        }
        Ok(radius)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn framing() -> Framing {
        Framing {
            size: [160, 80],
            scale: 4.0,
            camera_height: 72.0,
            vertical_bounds: [64.0, 96.0],
            horizontal_halo: 2.0,
        }
    }

    #[test]
    fn every_inverse_projected_volume_corner_fits_the_radius() {
        let framing = framing();
        let radius = framing.radius().unwrap();
        let mut greatest = 0.0_f64;
        for screen_x in [-80.0, 80.0] {
            for screen_y in [-40.0, 40.0] {
                for height in framing.vertical_bounds {
                    let sum_half = screen_y / framing.scale + height - framing.camera_height;
                    let difference_half = screen_x / (2.0 * 0.8660254037844386 * framing.scale);
                    for coordinate in [sum_half + difference_half, sum_half - difference_half] {
                        greatest = greatest.max(coordinate.abs() + framing.horizontal_halo);
                        assert!(coordinate.abs() + framing.horizontal_halo <= radius + 1e-12);
                    }
                }
            }
        }
        assert!((greatest - radius).abs() < 1e-12);
    }

    #[test]
    fn full_overworld_height_cannot_be_certified_by_a_radius128_policy() {
        let radius = Framing {
            vertical_bounds: [-64.0, 320.0],
            camera_height: 128.0,
            ..framing()
        }
        .radius()
        .unwrap();
        assert!(radius > 192.0);
        assert!(radius > 128.0);
    }

    #[test]
    fn invalid_and_unbounded_frames_are_rejected() {
        for value in [
            Framing {
                scale: 0.0,
                ..framing()
            },
            Framing {
                scale: f64::NAN,
                ..framing()
            },
            Framing {
                camera_height: f64::INFINITY,
                ..framing()
            },
            Framing {
                vertical_bounds: [96.0, 64.0],
                ..framing()
            },
            Framing {
                horizontal_halo: -1.0,
                ..framing()
            },
            Framing {
                size: [0, 80],
                ..framing()
            },
            Framing {
                size: [65_537, 80],
                ..framing()
            },
        ] {
            assert!(value.radius().is_err());
        }
    }
}

use serde::{Deserialize, Serialize};

/// One frame-local source owner and its final painted Braille-dot count.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PaintedOwner {
    pub id: u32,
    pub dots: u32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DitherMode {
    #[default]
    Ordered,
    Stippled,
    Bayer2,
    Bayer4,
    Bayer16,
    BlueNoise,
    Gradient,
    Halftone,
    Lines,
    Diagonal,
    Crosshatch,
    WhiteNoise,
    FloydSteinberg,
    Atkinson,
    SierraLite,
}

#[derive(Debug, Default)]
pub struct Raster {
    pub width: usize,
    pub height: usize,
    pub dots: Vec<f32>,
    /// Zero means no authenticated scene owner. A model renderer may set a
    /// frame-local id only for a dot whose exact saved state supplied its ink.
    pub owner_ids: Vec<u32>,
}

impl Raster {
    pub fn resize(&mut self, width: usize, height: usize) {
        self.width = width;
        self.height = height;
        self.dots.resize(width * height, 0.0);
        self.dots.fill(0.0);
        self.owner_ids.resize(width * height, 0);
        self.owner_ids.fill(0);
    }

    /// Conservative single-owner dot write. Ties erase provenance; unresolved
    /// models use owner 0. Callers with multi-layer alpha must clear ownership
    /// unless they can certify the final surviving contribution.
    pub fn owned_dot(&mut self, x: usize, y: usize, intensity: f32, owner: u32) -> bool {
        if x >= self.width || y >= self.height || !intensity.is_finite() {
            return false;
        }
        let index = y * self.width + x;
        if intensity == 1.0 && owner == 0 && self.dots[index] <= 1.0 {
            // Wind and other opaque unresolved layers use this common case.
            // It has the same ownership result as the general comparison:
            // an existing owner is cleared because the two opaque dots tie.
            if self.dots[index] < 1.0 {
                self.dots[index] = 1.0;
            }
            if self.owner_ids[index] != 0 {
                self.owner_ids[index] = 0;
            }
            return true;
        }
        let intensity = intensity.clamp(0.0, 1.0);
        if intensity > self.dots[index] {
            self.dots[index] = intensity;
            self.owner_ids[index] = owner;
        } else if intensity == self.dots[index] && owner != self.owner_ids[index] {
            self.owner_ids[index] = 0;
        }
        true
    }

    /// Paints an opaque unresolved dot after the caller has proved the
    /// coordinates are inside this raster. Wind uses this path for its dense
    /// particle loop; the debug assertion keeps that caller contract visible
    /// without paying the generic validation cost in release builds.
    pub(crate) fn opaque_dot_in_bounds(&mut self, x: usize, y: usize) {
        debug_assert!(x < self.width && y < self.height);
        self.opaque_dot_index_in_bounds(y * self.width + x);
    }

    /// Paints a previously bounds-checked flat pixel index without converting
    /// through raster coordinates. The unresolved-dot owner semantics are
    /// identical to `opaque_dot_in_bounds`.
    pub(crate) fn opaque_dot_index_in_bounds(&mut self, index: usize) {
        debug_assert!(index < self.dots.len());
        if self.dots[index] < 1.0 {
            self.dots[index] = 1.0;
        }
        if self.owner_ids[index] != 0 {
            self.owner_ids[index] = 0;
        }
    }

    pub fn aspect(&self) -> f32 {
        // Typical terminal cells are twice as tall as wide. A 2x4 Braille
        // raster then has approximately square physical dots.
        self.width as f32 / self.height.max(1) as f32
    }

    pub fn field(&mut self, mut sample: impl FnMut(f32, f32) -> f32) {
        let width = self.width as f32;
        let height = self.height as f32;
        for y in 0..self.height {
            let v = (y as f32 + 0.5) / height;
            for x in 0..self.width {
                let u = (x as f32 + 0.5) / width;
                let index = y * self.width + x;
                self.dots[index] = sample(u, v).clamp(0.0, 1.0);
                self.owner_ids[index] = 0;
            }
        }
    }

    /// A soft line, with radius measured in raster dots. Bounding the covered
    /// pixels avoids evaluating every grass/mesh segment at every raster dot.
    pub fn line(&mut self, from: (f32, f32), to: (f32, f32), radius: f32, intensity: f32) {
        let ax = from.0 * self.width as f32;
        let ay = from.1 * self.height as f32;
        let bx = to.0 * self.width as f32;
        let by = to.1 * self.height as f32;
        let outer_radius = radius + 0.6;
        let squared_outer_radius = outer_radius * outer_radius;
        // Floor/ceil around the complete support stay conservative for dot
        // centers at x + 0.5, y + 0.5 without the former extra 0.4-dot border.
        let left = (ax.min(bx) - outer_radius).max(0.0) as usize;
        let top = (ay.min(by) - outer_radius).max(0.0) as usize;
        let right = ((ax.max(bx) + outer_radius).ceil().max(0.0) as usize).min(self.width);
        let bottom = ((ay.max(by) + outer_radius).ceil().max(0.0) as usize).min(self.height);
        let delta_x = bx - ax;
        let delta_y = by - ay;
        let squared_length = delta_x * delta_x + delta_y * delta_y;
        for y in top..bottom {
            for x in left..right {
                let px = x as f32 + 0.5 - ax;
                let py = y as f32 + 0.5 - ay;
                let fraction = if squared_length > 0.0001 {
                    ((px * delta_x + py * delta_y) / squared_length).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let offset_x = px - fraction * delta_x;
                let offset_y = py - fraction * delta_y;
                let squared_distance = offset_x * offset_x + offset_y * offset_y;
                if squared_distance >= squared_outer_radius {
                    continue;
                }
                let coverage = (outer_radius - squared_distance.sqrt()).clamp(0.0, 1.0);
                let index = y * self.width + x;
                let next = coverage * intensity;
                if next >= self.dots[index] {
                    self.dots[index] = next;
                    self.owner_ids[index] = 0;
                }
            }
        }
    }

    pub fn curve(
        &mut self,
        steps: usize,
        radius: f32,
        intensity: f32,
        mut position: impl FnMut(f32) -> (f32, f32),
    ) {
        let mut previous = position(0.0);
        for step in 1..=steps {
            let next = position(step as f32 / steps as f32);
            self.line(previous, next, radius, intensity);
            previous = next;
        }
    }
}

pub fn hash(x: i32, y: i32) -> f32 {
    let mut value =
        (x as u32).wrapping_mul(0x8da6_b343) ^ (y as u32).wrapping_mul(0xd816_3841) ^ 0xcb1a_b31f;
    value ^= value >> 16;
    value = value.wrapping_mul(0x7feb_352d);
    value ^= value >> 15;
    value = value.wrapping_mul(0x846c_a68b);
    value ^= value >> 16;
    (value & 0xffff) as f32 / 65535.0
}

pub fn smoothstep(low: f32, high: f32, value: f32) -> f32 {
    let fraction = ((value - low) / (high - low)).clamp(0.0, 1.0);
    fraction * fraction * (3.0 - 2.0 * fraction)
}

pub fn threshold(x: usize, y: usize, mode: DitherMode) -> f32 {
    crate::dither::threshold(x, y, mode)
}

#[cfg(test)]
mod optimization_fidelity_tests {
    use super::*;

    // Frozen reference: supplied raster.rs SHA256
    // 963b7e09d220de9e2573a948ac4869ef373d0dac544ae1a322e68f25b7c17c27.

    fn reference_line(
        raster: &mut Raster,
        from: (f32, f32),
        to: (f32, f32),
        radius: f32,
        intensity: f32,
    ) {
        let ax = from.0 * raster.width as f32;
        let ay = from.1 * raster.height as f32;
        let bx = to.0 * raster.width as f32;
        let by = to.1 * raster.height as f32;
        let left = (ax.min(bx) - radius - 1.0).max(0.0) as usize;
        let top = (ay.min(by) - radius - 1.0).max(0.0) as usize;
        let right = ((ax.max(bx) + radius + 1.0).ceil().max(0.0) as usize).min(raster.width);
        let bottom = ((ay.max(by) + radius + 1.0).ceil().max(0.0) as usize).min(raster.height);
        let delta_x = bx - ax;
        let delta_y = by - ay;
        let squared_length = delta_x * delta_x + delta_y * delta_y;
        for y in top..bottom {
            for x in left..right {
                let px = x as f32 + 0.5 - ax;
                let py = y as f32 + 0.5 - ay;
                let fraction = if squared_length > 0.0001 {
                    ((px * delta_x + py * delta_y) / squared_length).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let offset_x = px - fraction * delta_x;
                let offset_y = py - fraction * delta_y;
                let squared_distance = offset_x * offset_x + offset_y * offset_y;
                let outer_radius = radius + 0.6;
                if squared_distance >= outer_radius * outer_radius {
                    continue;
                }
                let coverage = (outer_radius - squared_distance.sqrt()).clamp(0.0, 1.0);
                let index = y * raster.width + x;
                raster.dots[index] = raster.dots[index].max(coverage * intensity);
            }
        }
    }

    #[test]
    fn tighter_line_bounds_preserve_clipped_caps_degenerate_segments_and_overlap() {
        let segments = [
            ((0.5, 0.5), (0.5, 0.5)),
            ((-0.1, 0.25), (1.1, 0.75)),
            ((0.2, -0.1), (0.8, 1.1)),
            ((-0.2, -0.2), (-0.1, -0.1)),
            ((1.1, 1.1), (1.2, 1.2)),
            ((0.01, 0.5), (0.99, 0.5)),
            ((0.5, 0.01), (0.5, 0.99)),
            ((0.0, 0.0), (1.0, 1.0)),
            ((1.0, 0.0), (0.0, 1.0)),
            ((-1.0, 0.49), (2.0, 0.51)),
            ((0.499999, 0.500001), (0.500001, 0.499999)),
        ];
        for (width, height) in [(0, 0), (2, 4), (7, 9), (160, 96), (480, 320)] {
            for radius in [0.0, 0.13, 0.16, 0.19, 0.26, 0.6, 1.4] {
                let mut expected = Raster::default();
                expected.resize(width, height);
                let mut actual = Raster::default();
                actual.resize(width, height);
                for (segment, (from, to)) in segments.into_iter().enumerate() {
                    let light = if segment % 4 == 0 { 0.0 } else { 0.83 };
                    reference_line(&mut expected, from, to, radius, light);
                    actual.line(from, to, radius, light);
                    for (index, (expected, actual)) in
                        expected.dots.iter().zip(&actual.dots).enumerate()
                    {
                        assert_eq!(
                            expected.to_bits(),
                            actual.to_bits(),
                            "{width}x{height} radius {radius}, segment {segment}, dot {index}"
                        );
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod painted_owner_tests {
    use super::*;

    #[test]
    fn owner_tracks_only_single_stronger_saved_state_ink() {
        let mut raster = Raster::default();
        raster.resize(2, 4);
        assert!(raster.owned_dot(0, 0, 0.8, 7));
        assert_eq!(raster.owner_ids[0], 7);
        assert!(raster.owned_dot(0, 0, 0.2, 9));
        assert_eq!(raster.owner_ids[0], 7);
        assert!(raster.owned_dot(0, 0, 0.8, 9));
        assert_eq!(raster.owner_ids[0], 0);
        assert!(raster.owned_dot(0, 0, 0.9, 9));
        assert_eq!(raster.owner_ids[0], 9);
        assert!(!raster.owned_dot(2, 0, 1.0, 9));
        raster.field(|_, _| 0.2);
        assert!(raster.owner_ids.iter().all(|owner| *owner == 0));
        assert!(raster.owned_dot(0, 0, 0.9, 7));
        raster.resize(2, 4);
        assert!(raster.owner_ids.iter().all(|owner| *owner == 0));
    }

    #[test]
    fn opaque_in_bounds_matches_an_opaque_unresolved_write() {
        let mut expected = Raster::default();
        expected.resize(4, 4);
        expected.owned_dot(1, 2, 0.8, 7);
        expected.owned_dot(1, 2, 1.0, 0);

        let mut actual = Raster::default();
        actual.resize(4, 4);
        actual.owned_dot(1, 2, 0.8, 7);
        actual.opaque_dot_in_bounds(1, 2);

        assert_eq!(actual.dots, expected.dots);
        assert_eq!(actual.owner_ids, expected.owner_ids);
    }

    #[test]
    fn opaque_index_write_matches_coordinate_write_and_clears_owner() {
        let mut expected = Raster::default();
        expected.resize(4, 4);
        expected.owned_dot(3, 2, 0.8, 17);
        expected.opaque_dot_in_bounds(3, 2);

        let mut actual = Raster::default();
        actual.resize(4, 4);
        actual.owned_dot(3, 2, 0.8, 17);
        actual.opaque_dot_index_in_bounds(2 * 4 + 3);

        assert_eq!(actual.dots, expected.dots);
        assert_eq!(actual.owner_ids, expected.owner_ids);
    }
}

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DitherMode {
    #[default]
    Ordered,
    Stippled,
}

#[derive(Debug, Default)]
pub struct Raster {
    pub width: usize,
    pub height: usize,
    pub dots: Vec<f32>,
}

impl Raster {
    pub fn resize(&mut self, width: usize, height: usize) {
        self.width = width;
        self.height = height;
        self.dots.resize(width * height, 0.0);
        self.dots.fill(0.0);
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
                self.dots[y * self.width + x] = sample(u, v).clamp(0.0, 1.0);
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
                self.dots[index] = self.dots[index].max(coverage * intensity);
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
    match mode {
        DitherMode::Ordered => {
            const BAYER: [[u8; 8]; 8] = [
                [0, 48, 12, 60, 3, 51, 15, 63],
                [32, 16, 44, 28, 35, 19, 47, 31],
                [8, 56, 4, 52, 11, 59, 7, 55],
                [40, 24, 36, 20, 43, 27, 39, 23],
                [2, 50, 14, 62, 1, 49, 13, 61],
                [34, 18, 46, 30, 33, 17, 45, 29],
                [10, 58, 6, 54, 9, 57, 5, 53],
                [42, 26, 38, 22, 41, 25, 37, 21],
            ];
            (f32::from(BAYER[y % 8][x % 8]) + 0.5) / 64.0
        }
        // A fixed irregular 64x64 threshold tile: spatial stipple, deliberately
        // not advertised as a spectrally optimized blue-noise distribution.
        DitherMode::Stippled => 0.005 + hash((x % 64) as i32, (y % 64) as i32) * 0.99,
    }
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

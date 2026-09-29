use super::DitherMode;

#[derive(Debug, Default)]
pub(super) struct Raster {
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
        let left = (ax.min(bx) - radius - 1.0).max(0.0) as usize;
        let top = (ay.min(by) - radius - 1.0).max(0.0) as usize;
        let right = ((ax.max(bx) + radius + 1.0).ceil().max(0.0) as usize).min(self.width);
        let bottom = ((ay.max(by) + radius + 1.0).ceil().max(0.0) as usize).min(self.height);
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
                let distance = (px - fraction * delta_x).hypot(py - fraction * delta_y);
                let coverage = (radius + 0.6 - distance).clamp(0.0, 1.0);
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

pub(super) fn hash(x: i32, y: i32) -> f32 {
    let mut value =
        (x as u32).wrapping_mul(0x8da6_b343) ^ (y as u32).wrapping_mul(0xd816_3841) ^ 0xcb1a_b31f;
    value ^= value >> 16;
    value = value.wrapping_mul(0x7feb_352d);
    value ^= value >> 15;
    value = value.wrapping_mul(0x846c_a68b);
    value ^= value >> 16;
    (value & 0xffff) as f32 / 65535.0
}

pub(super) fn smoothstep(low: f32, high: f32, value: f32) -> f32 {
    let fraction = ((value - low) / (high - low)).clamp(0.0, 1.0);
    fraction * fraction * (3.0 - 2.0 * fraction)
}

pub(super) fn noise(x: f32, y: f32) -> f32 {
    let ix = x.floor() as i32;
    let iy = y.floor() as i32;
    let fx = smoothstep(0.0, 1.0, x - x.floor());
    let fy = smoothstep(0.0, 1.0, y - y.floor());
    let top = hash(ix, iy) * (1.0 - fx) + hash(ix.wrapping_add(1), iy) * fx;
    let bottom = hash(ix, iy.wrapping_add(1)) * (1.0 - fx)
        + hash(ix.wrapping_add(1), iy.wrapping_add(1)) * fx;
    top * (1.0 - fy) + bottom * fy
}

pub(super) fn threshold(x: usize, y: usize, mode: DitherMode) -> f32 {
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

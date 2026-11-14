//! Seeded lattice value noise. The lattice is a precomputed table so a noise
//! lookup is four array reads and a bilinear blend.

const LATTICE_SIZE: usize = 128;
const LATTICE_MASK: i32 = LATTICE_SIZE as i32 - 1;

/// Integer hash of a lattice point and a seed.
pub fn hash_u32(x: u32, y: u32, seed: u32) -> u32 {
    let x = x.wrapping_mul(0x27d4_eb2d) ^ seed;
    let y = y.wrapping_mul(0x1656_67b1);
    let mut value = x ^ y.rotate_left(13);
    value = (value ^ (value >> 15)).wrapping_mul(0x2c1b_3c6d);
    value = (value ^ (value >> 12)).wrapping_mul(0x297a_2d39);
    value ^ (value >> 15)
}

/// The hash mapped to `0.0..1.0`.
pub fn hash_unit(x: i32, y: i32, seed: u32) -> f32 {
    (hash_u32(x as u32, y as u32, seed) >> 8) as f32 / 16_777_216.0
}

fn fade(fraction: f32) -> f32 {
    fraction * fraction * (3.0 - 2.0 * fraction)
}

pub struct LatticeNoise {
    table: Vec<f32>,
}

impl LatticeNoise {
    pub fn new(seed: u32) -> Self {
        let mut table = Vec::with_capacity(LATTICE_SIZE * LATTICE_SIZE);
        for y in 0..LATTICE_SIZE {
            for x in 0..LATTICE_SIZE {
                table.push(hash_unit(x as i32, y as i32, seed));
            }
        }
        Self { table }
    }

    fn corner(&self, x: i32, y: i32) -> f32 {
        self.table[(y & LATTICE_MASK) as usize * LATTICE_SIZE + (x & LATTICE_MASK) as usize]
    }

    /// Value noise in `0.0..1.0`; the lattice repeats every 128 cells.
    pub fn value(&self, x: f32, y: f32) -> f32 {
        self.blend(x, y, i32::MAX)
    }

    /// Like `value`, but lattice rows repeat every `period_y` cells so a
    /// scroll by exactly `period_y` returns to the start.
    pub fn value_wrapped_y(&self, x: f32, y: f32, period_y: i32) -> f32 {
        self.blend(x, y, period_y.max(1))
    }

    fn blend(&self, x: f32, y: f32, period_y: i32) -> f32 {
        let (floor_x, floor_y) = (x.floor(), y.floor());
        let (ix, iy) = (floor_x as i32, floor_y as i32);
        let (fx, fy) = (fade(x - floor_x), fade(y - floor_y));
        let (row_0, row_1) = if period_y == i32::MAX {
            (iy, iy + 1)
        } else {
            (iy.rem_euclid(period_y), (iy + 1).rem_euclid(period_y))
        };
        let top = self.corner(ix, row_0) * (1.0 - fx) + self.corner(ix + 1, row_0) * fx;
        let bottom = self.corner(ix, row_1) * (1.0 - fx) + self.corner(ix + 1, row_1) * fx;
        top * (1.0 - fy) + bottom * fy
    }

    /// Fractal Brownian motion normalised to roughly `0.0..1.0`.
    pub fn fbm(&self, x: f32, y: f32, octaves: u32) -> f32 {
        self.fbm_inner(x, y, octaves, None)
    }

    /// `fbm` whose base lattice repeats every `period_y` rows (octave `i`
    /// repeats every `period_y * 2^i`), for seamless vertical scrolling.
    pub fn fbm_wrapped_y(&self, x: f32, y: f32, octaves: u32, period_y: i32) -> f32 {
        self.fbm_inner(x, y, octaves, Some(period_y))
    }

    fn fbm_inner(&self, x: f32, y: f32, octaves: u32, period_y: Option<i32>) -> f32 {
        let mut amplitude = 1.0;
        let mut total = 0.0;
        let mut weight = 0.0;
        let mut frequency = 1.0;
        for octave in 0..octaves.max(1) {
            let (sx, sy) = (x * frequency, y * frequency + 17.3 * octave as f32);
            let sample = match period_y {
                Some(period) => self.value_wrapped_y(sx, sy, period << octave.min(20)),
                None => self.value(sx, sy),
            };
            total += amplitude * sample;
            weight += amplitude;
            amplitude *= 0.5;
            frequency *= 2.0;
        }
        total / weight
    }
}

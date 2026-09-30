//! The smooth wave luminance field and the dither threshold tables.
//!
//! The field is a domain-warped fractal value noise blended with a swell of
//! sine bands. It is a pure function of position, time and seed, so the same
//! clock value always yields the same picture.

const OCTAVES: usize = 4;

/// Hash of a lattice point to 0..1.
fn lattice(ix: i32, iy: i32, key: u32) -> f32 {
    let mut value = (ix as u32).wrapping_mul(0x8da6_b343)
        ^ (iy as u32).wrapping_mul(0xd816_3841)
        ^ key.wrapping_mul(0x9e37_79b1);
    value ^= value >> 16;
    value = value.wrapping_mul(0x7feb_352d);
    value ^= value >> 15;
    value = value.wrapping_mul(0x846c_a68b);
    value ^= value >> 16;
    (value >> 8) as f32 / 16_777_215.0
}

fn quintic(fraction: f32) -> f32 {
    fraction * fraction * fraction * (fraction * (fraction * 6.0 - 15.0) + 10.0)
}

fn value_noise(x: f32, y: f32, key: u32) -> f32 {
    let floor_x = x.floor();
    let floor_y = y.floor();
    let (ix, iy) = (floor_x as i32, floor_y as i32);
    let (fx, fy) = (quintic(x - floor_x), quintic(y - floor_y));
    let top = lattice(ix, iy, key) + (lattice(ix + 1, iy, key) - lattice(ix, iy, key)) * fx;
    let bottom =
        lattice(ix, iy + 1, key) + (lattice(ix + 1, iy + 1, key) - lattice(ix, iy + 1, key)) * fx;
    top + (bottom - top) * fy
}

/// Everything about the wave field that does not change from frame to frame.
pub struct WaveField {
    /// Per-octave noise offsets and lattice keys, derived from the seed.
    octaves: [(f32, f32, u32); OCTAVES],
    amplitude: f32,
    frequency: f32,
    ripple: bool,
}

impl WaveField {
    pub fn new(seed: u32, frequency: f32, amplitude: f32, ripple: bool) -> Self {
        let mut octaves = [(0.0, 0.0, 0); OCTAVES];
        for (octave, slot) in octaves.iter_mut().enumerate() {
            let key = seed
                .wrapping_mul(0x85eb_ca6b)
                .wrapping_add(octave as u32 * 0x27d4_eb2f);
            *slot = (
                lattice(3, octave as i32, key) * 64.0,
                lattice(7, octave as i32, key) * 64.0,
                key,
            );
        }
        Self {
            octaves,
            amplitude,
            frequency,
            ripple,
        }
    }

    /// Fractal noise in 0..1 (normalised by the total octave weight).
    fn fbm(&self, x: f32, y: f32) -> f32 {
        let mut sum = 0.0;
        let mut weight = 0.0;
        let mut amplitude = 1.0;
        let mut scale = 1.0;
        for (offset_x, offset_y, key) in &self.octaves {
            sum += amplitude * value_noise(x * scale + offset_x, y * scale + offset_y, *key);
            weight += amplitude;
            amplitude *= 0.5;
            scale *= 2.0;
        }
        sum / weight
    }

    /// Field value at aspect-corrected screen position (`x`, `y`), where the
    /// screen height spans -0.5..0.5, at drift time `t`. Roughly 0..1.
    pub fn sample(&self, x: f32, y: f32, t: f32) -> f32 {
        let (px, py) = (x * self.frequency, y * self.frequency);
        let warp_x = self.fbm(px, py + t);
        let warp_y = self.fbm(px + 5.2, py + 1.3 - t);
        let warp = 4.0 * self.amplitude;
        let noise = self.fbm(px + warp * warp_x + t * 0.6, py + warp * warp_y);
        let swell = 0.5
            + 0.5
                * ((px * 0.9 + py * 0.6) * 2.0
                    + std::f32::consts::TAU * noise * self.amplitude * 3.0
                    + t * 2.0)
                    .sin();
        let mut value = 0.5 * (noise + swell);
        if self.ripple {
            let center_x = 0.25 * (0.31 * t).sin();
            let center_y = 0.2 * (0.23 * t).cos();
            let distance = ((px - center_x * self.frequency).powi(2)
                + (py - center_y * self.frequency).powi(2))
            .sqrt();
            value += 0.15 * (-distance * 0.3).exp() * (distance * 4.0 - t * 3.0).sin();
        }
        value
    }
}

/// A square tile of dither thresholds in (0, 1).
pub struct ThresholdTile {
    side: usize,
    values: Vec<f32>,
}

impl ThresholdTile {
    /// Ordered Bayer tile of side 2, 4 or 8 built with the standard recursion
    /// `B2n = [4Bn, 4Bn+2; 4Bn+3, 4Bn+1]`.
    pub fn bayer(side: usize) -> Self {
        let mut ranks = vec![0u32, 2, 3, 1];
        let mut current = 2;
        while current < side {
            let next = current * 2;
            let mut grown = vec![0u32; next * next];
            for y in 0..current {
                for x in 0..current {
                    let base = 4 * ranks[y * current + x];
                    grown[y * next + x] = base;
                    grown[y * next + x + current] = base + 2;
                    grown[(y + current) * next + x] = base + 3;
                    grown[(y + current) * next + x + current] = base + 1;
                }
            }
            ranks = grown;
            current = next;
        }
        let cells = (current * current) as f32;
        Self {
            side: current,
            values: ranks
                .iter()
                .map(|rank| (*rank as f32 + 0.5) / cells)
                .collect(),
        }
    }

    /// A fixed irregular 32x32 tile; deterministic in `seed`, never animated.
    pub fn noise(seed: u32) -> Self {
        let side = 32;
        let mut values = Vec::with_capacity(side * side);
        for y in 0..side {
            for x in 0..side {
                values.push(0.01 + 0.98 * lattice(x as i32, y as i32, seed ^ 0x5bd1_e995));
            }
        }
        Self { side, values }
    }

    pub fn at(&self, x: usize, y: usize) -> f32 {
        self.values[(y % self.side) * self.side + x % self.side]
    }
}

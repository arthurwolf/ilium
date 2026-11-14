//! Domain-warped fractal value noise: the cloud density field.
//!
//! A software re-creation of the classic "warp fBm" cloud recipe (fBm fed
//! through two levels of its own displacement). Only the luminance of the
//! original cyan/brown/blue palette is kept, because the scene is drawn as
//! one-bit dots.

/// Lattice edge length. Small enough (64 KiB) to stay resident in L1/L2.
const LATTICE_SIZE: usize = 128;
const LATTICE_MASK: i32 = LATTICE_SIZE as i32 - 1;
const MAX_OCTAVES: usize = 4;
/// Per-octave frequency multipliers, deliberately not exactly 2 so octave
/// lattices never line up.
const OCTAVE_LACUNARITY: [f32; MAX_OCTAVES] = [2.02, 2.03, 2.01, 2.0];
const OCTAVE_WEIGHT: [f32; MAX_OCTAVES] = [0.5, 0.25, 0.125, 0.0625];

/// Seeded table of uniform random texels, sampled with wrap-around.
pub struct NoiseLattice {
    texels: Vec<f32>,
}

impl NoiseLattice {
    pub fn new(seed: u32) -> Self {
        let mut state = 0x9e37_79b9_7f4a_7c15u64 ^ (u64::from(seed) << 20 | u64::from(seed));
        let texels = (0..LATTICE_SIZE * LATTICE_SIZE)
            .map(|_| {
                // splitmix64
                state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
                let mut mixed = state;
                mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
                mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
                mixed ^= mixed >> 31;
                (mixed >> 40) as f32 / (1u64 << 24) as f32
            })
            .collect();
        Self { texels }
    }

    #[inline]
    fn texel(&self, x: i32, y: i32) -> f32 {
        self.texels[(y & LATTICE_MASK) as usize * LATTICE_SIZE + (x & LATTICE_MASK) as usize]
    }

    /// Smoothstep-interpolated value noise in 0..1.
    #[inline]
    fn noise(&self, x: f32, y: f32) -> f32 {
        let floor_x = x.floor();
        let floor_y = y.floor();
        let fraction_x = x - floor_x;
        let fraction_y = y - floor_y;
        let smooth_x = fraction_x * fraction_x * (3.0 - 2.0 * fraction_x);
        let smooth_y = fraction_y * fraction_y * (3.0 - 2.0 * fraction_y);
        let cell_x = floor_x as i32;
        let cell_y = floor_y as i32;
        let top = self.texel(cell_x, cell_y);
        let top_right = self.texel(cell_x + 1, cell_y);
        let bottom = self.texel(cell_x, cell_y + 1);
        let bottom_right = self.texel(cell_x + 1, cell_y + 1);
        let upper = top + (top_right - top) * smooth_x;
        let lower = bottom + (bottom_right - bottom) * smooth_x;
        upper + (lower - upper) * smooth_y
    }

    /// Rotating fBm normalized to 0..1.
    #[inline]
    fn fbm(&self, x: f32, y: f32, octaves: usize) -> f32 {
        let (mut x, mut y) = (x, y);
        let mut sum = 0.0;
        let mut weight_sum = 0.0;
        for octave in 0..octaves {
            sum += OCTAVE_WEIGHT[octave] * self.noise(x, y);
            weight_sum += OCTAVE_WEIGHT[octave];
            let scale = OCTAVE_LACUNARITY[octave];
            let rotated_x = (0.8 * x + 0.6 * y) * scale;
            y = (-0.6 * x + 0.8 * y) * scale;
            x = rotated_x;
        }
        sum / weight_sum
    }

    /// Luminance of the warped cloud colour at noise-space position (x, y).
    ///
    /// `phase` slides the warp layers (the source's `t * 0.25` term).
    #[inline]
    pub fn cloud_luminance(&self, x: f32, y: f32, shape: &CloudShape) -> f32 {
        let octaves = shape.octaves;
        let warp = shape.warp;
        let phase = shape.phase;
        let q_x = self.fbm(x + phase, y + phase, octaves);
        let q_y = self.fbm(x + 1.0, y + 1.0, octaves);
        let base_x = x + warp * q_x;
        let base_y = y + warp * q_y;
        let r_x = self.fbm(
            base_x + 1.7 + 0.31 * phase,
            base_y + 9.2 + 0.31 * phase,
            octaves,
        );
        let r_y = self.fbm(
            base_x + 8.3 + 0.21 * phase,
            base_y + 2.8 + 0.21 * phase,
            octaves,
        );
        let density = self.fbm(x + warp * r_x, y + warp * r_y, octaves);

        let cyan_mix = (density * density * 2.0).clamp(0.0, 1.0);
        let mut red = 1.0 + (0.3 - 1.0) * cyan_mix;
        let mut green = 1.0 + (1.6 - 1.0) * cyan_mix;
        let mut blue = green;
        let brown_mix = (q_x * q_x + q_y * q_y).sqrt().clamp(0.0, 1.0);
        red += (0.4 - red) * brown_mix;
        green += (0.2 - green) * brown_mix;
        blue += (0.16 - blue) * brown_mix;
        let blue_mix = r_x.clamp(0.0, 1.0);
        red += (0.4 - red) * blue_mix;
        green += (0.7 - green) * blue_mix;
        blue += (3.0 - blue) * blue_mix;
        red * red * red * 0.299 + green * green * green * 0.587 + blue * blue * blue * 0.114
    }
}

/// Per-frame parameters of the field.
#[derive(Debug, Clone, Copy)]
pub struct CloudShape {
    pub octaves: usize,
    pub warp: f32,
    pub phase: f32,
}

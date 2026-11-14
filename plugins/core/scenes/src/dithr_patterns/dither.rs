//! Turning a `0..1` field into dots: Bayer, error diffusion, noise, threshold.

use super::noise::hash_unit;
use super::settings::{BayerSize, DitherKind};

fn bayer_index(x: usize, y: usize, size: usize) -> usize {
    const BASE: [[usize; 2]; 2] = [[0, 2], [3, 1]];
    if size <= 1 {
        return 0;
    }
    let half = size / 2;
    4 * bayer_index(x % half, y % half, half) + BASE[(y / half) & 1][(x / half) & 1]
}

/// Precomputed Bayer thresholds in `0..1`.
pub struct BayerTable {
    size: usize,
    thresholds: Vec<f32>,
}

impl BayerTable {
    pub fn new(matrix: BayerSize) -> Self {
        let size = match matrix {
            BayerSize::Bayer4 => 4,
            BayerSize::Bayer8 => 8,
        };
        let cells = (size * size) as f32;
        let thresholds = (0..size * size)
            .map(|index| (bayer_index(index % size, index / size, size) as f32 + 0.5) / cells)
            .collect();
        Self { size, thresholds }
    }

    pub fn threshold(&self, x: usize, y: usize) -> f32 {
        self.thresholds[(y % self.size) * self.size + x % self.size]
    }
}

/// Twinkles per second of the random-noise dither.
const NOISE_RATE_HZ: f64 = 6.0;

/// Settings of one dither pass.
pub struct DitherPass<'a> {
    pub kind: DitherKind,
    /// `0.0..=1.0`: blend between a plain 0.5 cut and the dither threshold.
    pub amount: f32,
    pub bayer: &'a BayerTable,
    pub seed: u32,
    /// Animation seconds, only used to step the random noise.
    pub time: f64,
}

/// Decide on/off for every dot of `field` (row-major, `width` wide), writing
/// `1.0` or `0.0` into `output`. `carry` and `next` are error rows for
/// Floyd-Steinberg, each at least `width + 2` long.
pub fn dither_field(
    pass: &DitherPass<'_>,
    field: &[f32],
    width: usize,
    output: &mut [f32],
    carry: &mut [f32],
    next: &mut [f32],
) {
    if width == 0 {
        return;
    }
    match pass.kind {
        DitherKind::FloydSteinberg => {
            error_diffusion(pass.amount, field, width, output, carry, next)
        }
        _ => threshold_pass(pass, field, width, output),
    }
}

fn threshold_pass(pass: &DitherPass<'_>, field: &[f32], width: usize, output: &mut [f32]) {
    let bucket = (pass.time * NOISE_RATE_HZ).floor() as i64 as u32;
    let noise_seed = pass.seed ^ bucket.wrapping_mul(0x9e37_79b1);
    for (index, (value, out)) in field.iter().zip(output.iter_mut()).enumerate() {
        let (x, y) = (index % width, index / width);
        let threshold = match pass.kind {
            DitherKind::OrderedBayer => pass.bayer.threshold(x, y),
            DitherKind::RandomNoise => hash_unit(x as i32, y as i32, noise_seed),
            _ => 0.5,
        };
        let effective = 0.5 + (threshold - 0.5) * pass.amount;
        *out = if *value > effective { 1.0 } else { 0.0 };
    }
}

/// Serpentine Floyd-Steinberg; `amount` scales the diffused error.
fn error_diffusion(
    amount: f32,
    field: &[f32],
    width: usize,
    output: &mut [f32],
    carry: &mut [f32],
    next: &mut [f32],
) {
    carry[..width + 2].fill(0.0);
    for (row, (values, outs)) in field
        .chunks_exact(width)
        .zip(output.chunks_exact_mut(width))
        .enumerate()
    {
        next[..width + 2].fill(0.0);
        let leftwards = row % 2 == 1;
        for step in 0..width {
            let x = if leftwards { width - 1 - step } else { step };
            let wanted = values[x] + carry[x + 1];
            let on = wanted > 0.5;
            outs[x] = if on { 1.0 } else { 0.0 };
            let error = (wanted - if on { 1.0 } else { 0.0 }) * amount;
            let ahead: isize = if leftwards { -1 } else { 1 };
            let slot = |offset: isize| (x as isize + 1 + offset) as usize;
            carry[slot(ahead)] += error * 7.0 / 16.0;
            next[slot(-ahead)] += error * 3.0 / 16.0;
            next[slot(0)] += error * 5.0 / 16.0;
            next[slot(ahead)] += error / 16.0;
        }
        carry[..width + 2].copy_from_slice(&next[..width + 2]);
    }
}

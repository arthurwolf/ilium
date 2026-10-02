//! Dither algorithms for turning dot intensities into on/off Braille dots.
//!
//! Most modes are threshold matrices: a dot is lit when `intensity * density`
//! exceeds `threshold(x, y)`. They are a pure function of the dot position, so
//! a still scene never flickers. The error-diffusion modes (Floyd-Steinberg,
//! Atkinson, Sierra Lite) instead push each dot's rounding error onto its
//! neighbours; they need the whole raster (`diffuse`) and can shimmer when the
//! scene moves, which is part of their look.

use crate::raster::{hash, DitherMode};
use std::sync::OnceLock;

impl DitherMode {
    pub const ALL: [Self; 15] = [
        Self::Ordered,
        Self::Stippled,
        Self::Bayer2,
        Self::Bayer4,
        Self::Bayer16,
        Self::BlueNoise,
        Self::Gradient,
        Self::Halftone,
        Self::Lines,
        Self::Diagonal,
        Self::Crosshatch,
        Self::WhiteNoise,
        Self::FloydSteinberg,
        Self::Atkinson,
        Self::SierraLite,
    ];

    pub const LABELS: [&'static str; 15] = [
        "Ordered (Bayer 8x8)",
        "Stippled",
        "Coarse (Bayer 2x2)",
        "Bayer 4x4",
        "Fine (Bayer 16x16)",
        "Blue noise",
        "Gradient noise",
        "Halftone dots",
        "Scan lines",
        "Diagonal lines",
        "Crosshatch",
        "White noise",
        "Floyd-Steinberg",
        "Atkinson",
        "Sierra Lite",
    ];

    pub fn from_index(index: usize) -> Option<Self> {
        Self::ALL.get(index).copied()
    }

    pub fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|candidate| *candidate == self)
            .unwrap_or(0)
    }

    pub fn label(self) -> &'static str {
        Self::LABELS[self.index()]
    }

    /// True for the modes that need `diffuse` instead of a threshold matrix.
    pub fn is_error_diffusion(self) -> bool {
        matches!(
            self,
            Self::FloydSteinberg | Self::Atkinson | Self::SierraLite
        )
    }
}

/// Recursive Bayer matrix of side `size` (a power of two), values 0..size^2.
fn bayer(size: usize) -> Vec<u16> {
    let mut matrix = vec![0_u16];
    let mut side = 1;
    while side < size {
        let next = side * 2;
        let mut grown = vec![0_u16; next * next];
        for y in 0..side {
            for x in 0..side {
                let base = matrix[y * side + x] * 4;
                grown[y * next + x] = base;
                grown[y * next + x + side] = base + 2;
                grown[(y + side) * next + x] = base + 3;
                grown[(y + side) * next + x + side] = base + 1;
            }
        }
        matrix = grown;
        side = next;
    }
    matrix
}

fn bayer_threshold(table: &'static OnceLock<Vec<u16>>, size: usize, x: usize, y: usize) -> f32 {
    let matrix = table.get_or_init(|| bayer(size));
    (f32::from(matrix[(y % size) * size + x % size]) + 0.5) / (size * size) as f32
}

/// The original 8x8 ordered table the first scenes shipped with; kept
/// byte-for-byte so existing frames and saved caches look the same.
const ORIGINAL_BAYER_8: [[u8; 8]; 8] = [
    [0, 48, 12, 60, 3, 51, 15, 63],
    [32, 16, 44, 28, 35, 19, 47, 31],
    [8, 56, 4, 52, 11, 59, 7, 55],
    [40, 24, 36, 20, 43, 27, 39, 23],
    [2, 50, 14, 62, 1, 49, 13, 61],
    [34, 18, 46, 30, 33, 17, 45, 29],
    [10, 58, 6, 54, 9, 57, 5, 53],
    [42, 26, 38, 22, 41, 25, 37, 21],
];

const BLUE_SIDE: usize = 64;

/// Void-and-cluster blue noise (Ulichney 1993), generated once and tiled.
/// Deterministic: the seed pattern comes from `hash`.
fn blue_noise() -> &'static [f32] {
    static TABLE: OnceLock<Vec<f32>> = OnceLock::new();
    TABLE.get_or_init(generate_blue_noise)
}

fn generate_blue_noise() -> Vec<f32> {
    const SIDE: usize = BLUE_SIDE;
    const COUNT: usize = SIDE * SIDE;
    const RADIUS: i32 = 5;
    let kernel: Vec<(i32, i32, f32)> = (-RADIUS..=RADIUS)
        .flat_map(|dy| {
            (-RADIUS..=RADIUS).map(move |dx| {
                let squared = (dx * dx + dy * dy) as f32;
                (dx, dy, (-squared / (2.0 * 1.5 * 1.5)).exp())
            })
        })
        .collect();
    let mut energy = vec![0.0_f32; COUNT];
    let mut on = vec![false; COUNT];
    let splat = |energy: &mut Vec<f32>, index: usize, sign: f32| {
        let (x, y) = ((index % SIDE) as i32, (index / SIDE) as i32);
        for (dx, dy, weight) in &kernel {
            let wrapped = ((y + dy).rem_euclid(SIDE as i32) as usize) * SIDE
                + (x + dx).rem_euclid(SIDE as i32) as usize;
            energy[wrapped] += sign * weight;
        }
    };
    let tightest = |energy: &[f32], on: &[bool]| {
        (0..COUNT)
            .filter(|index| on[*index])
            .max_by(|a, b| energy[*a].total_cmp(&energy[*b]))
    };
    let largest_void = |energy: &[f32], on: &[bool]| {
        (0..COUNT)
            .filter(|index| !on[*index])
            .min_by(|a, b| energy[*a].total_cmp(&energy[*b]))
    };
    // Seed: the ten percent of positions with the lowest hash.
    let mut order: Vec<usize> = (0..COUNT).collect();
    order.sort_by(|a, b| {
        hash((*a % SIDE) as i32, (*a / SIDE) as i32)
            .total_cmp(&hash((*b % SIDE) as i32, (*b / SIDE) as i32))
    });
    let seeds = COUNT / 10;
    for index in order.iter().take(seeds) {
        on[*index] = true;
        splat(&mut energy, *index, 1.0);
    }
    // Relax the seed until the tightest cluster and the largest void coincide.
    for _ in 0..COUNT {
        let (Some(cluster), ()) = (tightest(&energy, &on), ()) else {
            break;
        };
        on[cluster] = false;
        splat(&mut energy, cluster, -1.0);
        let Some(void) = largest_void(&energy, &on) else {
            break;
        };
        on[void] = true;
        splat(&mut energy, void, 1.0);
        if void == cluster {
            break;
        }
    }
    let seed_on = on.clone();
    let seed_energy = energy.clone();
    let mut rank = vec![0_usize; COUNT];
    // Phase 1: peel clusters off the seed, highest rank first.
    let mut remaining = seeds;
    while remaining > 0 {
        let Some(cluster) = tightest(&energy, &on) else {
            break;
        };
        on[cluster] = false;
        splat(&mut energy, cluster, -1.0);
        remaining -= 1;
        rank[cluster] = remaining;
    }
    // Phase 2 and 3: fill voids, then the most crowded remaining positions.
    on = seed_on;
    energy = seed_energy;
    let mut placed = seeds;
    while placed < COUNT {
        let target = if placed < COUNT / 2 {
            largest_void(&energy, &on)
        } else {
            (0..COUNT)
                .filter(|index| !on[*index])
                .max_by(|a, b| energy[*a].total_cmp(&energy[*b]))
        };
        let Some(index) = target else { break };
        on[index] = true;
        splat(&mut energy, index, 1.0);
        rank[index] = placed;
        placed += 1;
    }
    rank.iter()
        .map(|value| (*value as f32 + 0.5) / COUNT as f32)
        .collect()
}

/// Clustered-dot screen: pixels nearest the cell centre light first.
fn halftone() -> &'static [f32; 64] {
    static TABLE: OnceLock<[f32; 64]> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut order: Vec<usize> = (0..64).collect();
        let distance = |index: usize| {
            let (x, y) = ((index % 8) as f32 - 3.5, (index / 8) as f32 - 3.5);
            // Slight angle bias so ties break into a rosette, not a square.
            x.hypot(y) + 0.01 * (x - y)
        };
        order.sort_by(|a, b| distance(*a).total_cmp(&distance(*b)));
        let mut table = [0.0_f32; 64];
        for (rank, index) in order.into_iter().enumerate() {
            table[index] = (rank as f32 + 0.5) / 64.0;
        }
        table
    })
}

/// Two diagonal line families: pixels on a line light first, ranked so the
/// thresholds stay uniformly distributed.
fn crosshatch() -> &'static [f32; 64] {
    static TABLE: OnceLock<[f32; 64]> = OnceLock::new();
    TABLE.get_or_init(|| {
        let depth = |index: usize| {
            let (x, y) = (index % 8, index / 8);
            let forward = (x + y) % 8;
            let backward = (x + 8 - y) % 8;
            // Distance from the nearest line of either family, with a tiny
            // positional tie-break so ranks are strict.
            forward.min(backward) as f32 + 0.001 * ((x * 8 + y) as f32)
        };
        let mut order: Vec<usize> = (0..64).collect();
        order.sort_by(|a, b| depth(*a).total_cmp(&depth(*b)));
        let mut table = [0.0_f32; 64];
        for (rank, index) in order.into_iter().enumerate() {
            table[index] = (rank as f32 + 0.5) / 64.0;
        }
        table
    })
}

/// Threshold at dot `(x, y)` for the matrix modes. The error-diffusion modes
/// have no matrix; they report a neutral 0.5 so a caller that ignores
/// `is_error_diffusion` still draws something sensible.
pub fn threshold(x: usize, y: usize, mode: DitherMode) -> f32 {
    static BAYER2: OnceLock<Vec<u16>> = OnceLock::new();
    static BAYER4: OnceLock<Vec<u16>> = OnceLock::new();
    static BAYER16: OnceLock<Vec<u16>> = OnceLock::new();
    match mode {
        DitherMode::Ordered => (f32::from(ORIGINAL_BAYER_8[y % 8][x % 8]) + 0.5) / 64.0,
        DitherMode::Bayer2 => bayer_threshold(&BAYER2, 2, x, y),
        DitherMode::Bayer4 => bayer_threshold(&BAYER4, 4, x, y),
        DitherMode::Bayer16 => bayer_threshold(&BAYER16, 16, x, y),
        // A fixed irregular 64x64 threshold tile: spatial stipple, deliberately
        // not advertised as a spectrally optimized blue-noise distribution.
        DitherMode::Stippled => 0.005 + hash((x % 64) as i32, (y % 64) as i32) * 0.99,
        DitherMode::BlueNoise => blue_noise()[(y % BLUE_SIDE) * BLUE_SIDE + x % BLUE_SIDE],
        DitherMode::Gradient => {
            let inner = (0.067_110_56 * x as f32 + 0.005_837_15 * y as f32).fract();
            (52.982_92 * inner).fract().clamp(0.005, 0.995)
        }
        DitherMode::Halftone => halftone()[(y % 8) * 8 + x % 8],
        DitherMode::Lines => ((y % 4) as f32 + 0.5) / 4.0,
        DitherMode::Diagonal => (((x + y) % 6) as f32 + 0.5) / 6.0,
        DitherMode::Crosshatch => crosshatch()[(y % 8) * 8 + x % 8],
        DitherMode::WhiteNoise => 0.005 + hash((x % 128) as i32, (y % 128) as i32) * 0.99,
        DitherMode::FloydSteinberg | DitherMode::Atkinson | DitherMode::SierraLite => 0.5,
    }
}

/// Error-diffusion weights as `(dx, dy, numerator)` over `denominator`.
fn diffusion_kernel(mode: DitherMode) -> (&'static [(i32, i32, f32)], f32) {
    match mode {
        DitherMode::Atkinson => (
            &[
                (1, 0, 1.0),
                (2, 0, 1.0),
                (-1, 1, 1.0),
                (0, 1, 1.0),
                (1, 1, 1.0),
                (0, 2, 1.0),
            ],
            8.0,
        ),
        DitherMode::SierraLite => (&[(1, 0, 2.0), (-1, 1, 1.0), (0, 1, 1.0)], 4.0),
        _ => (&[(1, 0, 7.0), (-1, 1, 3.0), (0, 1, 5.0), (1, 1, 1.0)], 16.0),
    }
}

/// Quantise `dots` (row-major, `width * height`) with error diffusion. A dot
/// is lit when its intensity times `density` plus the carried error reaches
/// one half. `out` is resized to `dots.len()`.
pub fn diffuse(
    dots: &[f32],
    width: usize,
    height: usize,
    density: f32,
    mode: DitherMode,
    out: &mut Vec<bool>,
) {
    out.clear();
    out.resize(dots.len(), false);
    if width == 0 || height == 0 || dots.len() < width * height {
        return;
    }
    let (kernel, denominator) = diffusion_kernel(mode);
    // Two rows of carried error are enough for every kernel (depth <= 2).
    let mut errors = vec![0.0_f32; width * 3];
    for y in 0..height {
        let current = (y % 3) * width;
        for x in 0..width {
            let value = dots[y * width + x] * density + errors[current + x];
            let lit = value >= 0.5;
            out[y * width + x] = lit;
            let error = value - if lit { 1.0 } else { 0.0 };
            for (dx, dy, weight) in kernel {
                let nx = x as i32 + dx;
                if nx < 0 || nx >= width as i32 || y + *dy as usize >= height {
                    continue;
                }
                errors[((y + *dy as usize) % 3) * width + nx as usize] +=
                    error * weight / denominator;
            }
        }
        // The row just finished is reused two rows later: clear it.
        errors[current..current + width].fill(0.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_and_indices_cover_every_mode() {
        assert_eq!(DitherMode::ALL.len(), DitherMode::LABELS.len());
        for (index, mode) in DitherMode::ALL.into_iter().enumerate() {
            assert_eq!(mode.index(), index);
            assert_eq!(DitherMode::from_index(index), Some(mode));
            assert_eq!(mode.label(), DitherMode::LABELS[index]);
        }
        assert_eq!(DitherMode::from_index(99), None);
    }

    #[test]
    fn matrix_thresholds_stay_inside_the_open_unit_interval() {
        for mode in DitherMode::ALL {
            for y in 0..70 {
                for x in 0..70 {
                    let value = threshold(x, y, mode);
                    assert!(value > 0.0 && value < 1.0, "{mode:?} {x},{y} -> {value}");
                }
            }
        }
    }

    #[test]
    fn matrix_modes_light_about_the_requested_fraction() {
        for mode in DitherMode::ALL
            .into_iter()
            .filter(|mode| !mode.is_error_diffusion())
        {
            for target in [0.25_f32, 0.5, 0.75] {
                let lit = (0..64 * 64)
                    .filter(|index| target > threshold(index % 64, index / 64, mode))
                    .count() as f32
                    / 4096.0;
                assert!((lit - target).abs() < 0.1, "{mode:?} at {target} lit {lit}");
            }
        }
    }

    #[test]
    fn bayer_matrices_use_every_level_once() {
        for size in [2_usize, 4, 8, 16] {
            let mut values = bayer(size);
            values.sort_unstable();
            let expected: Vec<u16> = (0..(size * size) as u16).collect();
            assert_eq!(values, expected, "size {size}");
        }
    }

    #[test]
    fn blue_noise_is_a_permutation_and_better_spread_than_white_noise() {
        let noise = blue_noise();
        let mut ranks: Vec<usize> = noise
            .iter()
            .map(|value| (value * (BLUE_SIDE * BLUE_SIDE) as f32) as usize)
            .collect();
        ranks.sort_unstable();
        assert!(ranks.iter().enumerate().all(|(index, rank)| index == *rank));
        // At fifty percent the lit dots of blue noise are rarely adjacent.
        let adjacent = |mode| {
            let lit = |x: usize, y: usize| 0.5 > threshold(x % 64, y % 64, mode);
            (0..64)
                .flat_map(|y| (0..64).map(move |x| (x, y)))
                .filter(|(x, y)| lit(*x, *y) && lit(x + 1, *y))
                .count()
        };
        assert!(adjacent(DitherMode::BlueNoise) < adjacent(DitherMode::WhiteNoise));
    }

    #[test]
    fn diffusion_preserves_mean_brightness_and_is_deterministic() {
        for mode in [
            DitherMode::FloydSteinberg,
            DitherMode::Atkinson,
            DitherMode::SierraLite,
        ] {
            let dots = vec![0.3_f32; 80 * 60];
            let (mut a, mut b) = (Vec::new(), Vec::new());
            diffuse(&dots, 80, 60, 1.0, mode, &mut a);
            diffuse(&dots, 80, 60, 1.0, mode, &mut b);
            assert_eq!(a, b);
            let mean = a.iter().filter(|lit| **lit).count() as f32 / a.len() as f32;
            // Atkinson deliberately discards a quarter of the error.
            let tolerance = if mode == DitherMode::Atkinson {
                0.12
            } else {
                0.03
            };
            assert!((mean - 0.3).abs() < tolerance, "{mode:?} mean {mean}");
        }
    }

    #[test]
    fn diffusion_handles_empty_and_degenerate_rasters() {
        let mut out = Vec::new();
        diffuse(&[], 0, 0, 1.0, DitherMode::Atkinson, &mut out);
        assert!(out.is_empty());
        diffuse(&[1.0], 1, 1, 1.0, DitherMode::FloydSteinberg, &mut out);
        assert_eq!(out, vec![true]);
        diffuse(&[0.0; 5], 5, 1, 1.0, DitherMode::SierraLite, &mut out);
        assert!(out.iter().all(|lit| !lit));
    }
}

//! GPU seam of the fBm clouds scene.
//!
//! [`build_job`] describes one frame for the `FbmClouds` kernel
//! (`ilium-gpu/src/shaders/fbm_clouds.wgsl`). The kernel reproduces the
//! software renderer's final dot intensities, dither included, including the
//! seeded splitmix64 noise lattice (recomputed on the device), so the only
//! unavoidable difference is f32 rounding (fused multiply-add, `sqrt`
//! accuracy) in the noise maths, which can flip a dot whose tone sits almost
//! exactly on its dither threshold.
//!
//! Uniform layout, one `f32` per index ([`UNIFORM_COUNT`] in total, mirrored
//! by the `U_*` constants of the WGSL file; scalar `i` is read from
//! `params.values[i / 4][i % 4]`). Every value is computed here with the same
//! f32 expressions the software path uses:
//!
//! | index | name            | meaning                                              |
//! |-------|-----------------|------------------------------------------------------|
//! | 0     | block           | pixel block size in dots, 1..=4                      |
//! | 1     | octaves         | fBm octaves, 2..=4                                   |
//! | 2     | warp            | warp strength, `warp % / 100`                        |
//! | 3     | phase           | layer slide, `clock * 0.1875 * drift% / 100`         |
//! | 4     | pan             | pan offset, `clock * pan * 0.0025`                   |
//! | 5     | scale           | noise frequency, `3 * scale% / 100`                  |
//! | 6     | contrast        | `contrast % / 100`                                   |
//! | 7     | brightness      | lit-dot intensity, `brightness % / 100`              |
//! | 8     | invert          | 1.0 when inverted, else 0.0                          |
//! | 9     | dither          | 0 Bayer 8x8, 1 gradient noise, 2 white noise         |
//! | 10    | seed            | cloud layout seed, 0..=999 (lattice and white noise) |
//! | 11    | flip            | `-(1 + 0.0625 * width / height)`                     |
//! | 12    | inverse_height  | `1 / height` in dots                                 |
//! | 13-15 | unused (zero)   | pads the array to four `vec4<f32>`                   |
//!
//! `clock` is `time_seconds` times the GPU clock multiplier (real time; the
//! software path runs at half of it).

use super::settings::{DitherPattern, FbmCloudsSettings};
use super::{Engine, BASE_SCALE, PAN_UNIT, PHASE_RATE};
use crate::control::SceneSettings;
use crate::gpu::{GpuJob, GpuKernel};

/// Number of `f32` uniforms the kernel reads (four `vec4<f32>`).
pub(crate) const UNIFORM_COUNT: usize = 16;

pub(crate) const U_BLOCK: usize = 0;
pub(crate) const U_OCTAVES: usize = 1;
pub(crate) const U_WARP: usize = 2;
pub(crate) const U_PHASE: usize = 3;
pub(crate) const U_PAN: usize = 4;
pub(crate) const U_SCALE: usize = 5;
pub(crate) const U_CONTRAST: usize = 6;
pub(crate) const U_BRIGHTNESS: usize = 7;
pub(crate) const U_INVERT: usize = 8;
pub(crate) const U_DITHER: usize = 9;
pub(crate) const U_SEED: usize = 10;
pub(crate) const U_FLIP: usize = 11;
pub(crate) const U_INVERSE_HEIGHT: usize = 12;

fn dither_index(pattern: DitherPattern) -> f32 {
    match pattern {
        DitherPattern::Bayer => 0.0,
        DitherPattern::Gradient => 1.0,
        DitherPattern::White => 2.0,
    }
}

pub(crate) fn build_job(
    settings: &FbmCloudsSettings,
    width: usize,
    height: usize,
    time_seconds: f64,
) -> GpuJob {
    let settings = settings.normalized();
    let clock = time_seconds * Engine::GpuSelected.time_scale();
    let aspect = width as f32 / height.max(1) as f32;
    let mut uniforms = vec![0.0_f32; UNIFORM_COUNT];
    uniforms[U_BLOCK] = settings.block as f32;
    uniforms[U_OCTAVES] = settings.octaves as f32;
    uniforms[U_WARP] = settings.warp as f32 / 100.0;
    uniforms[U_PHASE] = (clock * PHASE_RATE * f64::from(settings.drift) / 100.0) as f32;
    uniforms[U_PAN] = (clock * f64::from(settings.pan) * PAN_UNIT) as f32;
    uniforms[U_SCALE] = BASE_SCALE * settings.scale as f32 / 100.0;
    uniforms[U_CONTRAST] = settings.contrast as f32 / 100.0;
    uniforms[U_BRIGHTNESS] = settings.brightness as f32 / 100.0;
    uniforms[U_INVERT] = if settings.invert { 1.0 } else { 0.0 };
    uniforms[U_DITHER] = dither_index(settings.dither);
    uniforms[U_SEED] = settings.seed as f32;
    uniforms[U_FLIP] = -(1.0 + 0.0625 * aspect);
    uniforms[U_INVERSE_HEIGHT] = 1.0 / height.max(1) as f32;
    GpuJob {
        kernel: GpuKernel::FbmClouds,
        width: u32::try_from(width).unwrap_or(u32::MAX),
        height: u32::try_from(height).unwrap_or(u32::MAX),
        uniforms,
    }
}

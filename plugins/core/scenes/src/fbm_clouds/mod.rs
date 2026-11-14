//! Slow drifting clouds, drawn as one-bit dithered dots.
//!
//! Domain-warped fractal noise (`field`) is evaluated on a coarse block grid,
//! turned into a tone, and thresholded against a static dither pattern, so
//! dot density follows cloud brightness. Only the folding and the pan
//! animate, both as pure functions of the animation clock, which makes the
//! scene deterministic and free of any state between frames beyond buffers.
//!
//! The scene runs on the CPU in gentle slow motion; the former GPU renderer is not part of
//! the plugin form.

mod field;
mod settings;

pub use settings::FbmCloudsSettings;

use crate::control::SceneSettings;
use crate::scene::{Frame, Scene, SceneEnv};
use field::{CloudShape, NoiseLattice};
use settings::DitherPattern;

/// Inspiration for the look: texture-dithered fBm clouds.
pub const INSPIRED_BY: &[&str] = &["https://thecodetherapy.com/edit/61532f99178ef100260629c5"];

const DITHER_TILE: usize = 64;
/// Source constants: cloud colour is cubed then scaled by this gain.
const LUMINANCE_GAIN: f32 = 0.3;
/// The source warps at `time * 0.75` and shifts layers by a quarter of that.
const PHASE_RATE: f64 = 0.75 * 0.25;
/// Pan units per setting step.
const PAN_UNIT: f64 = 0.0025;
/// Base cloud frequency: the source samples noise at `p * 3`.
const BASE_SCALE: f32 = 3.0;
/// Share of the lit intensity given to the smooth (undithered) tone, so the
/// host's own dither has something soft to work with.
const GLOW_SHARE: f32 = 0.15;

/// Internal clock multiplier: the scene runs in slow motion.
const TIME_SCALE: f64 = 0.5;
/// The frame rate the scene asks for.
const FRAMES_PER_SECOND: u32 = 8;

pub struct FbmCloudsScene {
    settings: FbmCloudsSettings,
    lattice: NoiseLattice,
    dither_tile: Vec<f32>,
    /// Tone per sample block, reused across frames.
    tones: Vec<f32>,
}

impl FbmCloudsScene {
    // PALETTE (native Scene contract): `env.palette` is the shared look's current
    // palette. A custom native Scene receives the
    // current palette and MUST follow it: scenes with natural colours shift them
    // onto it (`ScenePalette::recolor`/`at`), and `Scene::set_palette` delivers later
    // changes. Monochrome scenes may ignore it. Today `PaletteScene` (scene.rs),
    // which `create_scene` wraps around every scene, shifts this scene's cell
    // colours onto the palette by brightness.
    pub fn new(settings: &FbmCloudsSettings, _env: &SceneEnv) -> Self {
        let settings = settings.normalized();
        Self {
            lattice: NoiseLattice::new(settings.seed),
            dither_tile: dither_tile(settings.pattern, settings.seed),
            tones: Vec::new(),
            settings,
        }
    }

    /// Fill `self.tones` with the dither input of every block. Returns the
    /// number of blocks per row.
    fn sample_tones(&mut self, width: usize, height: usize, time: f64) -> usize {
        let block = self.settings.block as usize;
        let columns = width.div_ceil(block);
        let rows = height.div_ceil(block);
        self.tones.clear();
        self.tones.reserve(columns * rows);

        let clock = time * TIME_SCALE;
        let shape = CloudShape {
            octaves: self.settings.octaves as usize,
            warp: self.settings.warp as f32 / 100.0,
            phase: (clock * PHASE_RATE * f64::from(self.settings.drift) / 100.0) as f32,
        };
        let pan = (clock * f64::from(self.settings.pan) * PAN_UNIT) as f32;
        let scale = BASE_SCALE * self.settings.scale as f32 / 100.0;
        let steepness = self.settings.tone_steepness as f32 / 100.0;
        let aspect = width as f32 / height as f32;
        let flip = -(1.0 + 0.0625 * aspect);
        let inverse_height = 1.0 / height as f32;
        let half_block = block as f32 * 0.5;
        for row in 0..rows {
            let v = (row as f32 * block as f32 + half_block) * inverse_height;
            let noise_y = (-1.0 + 2.0 * v * flip) * scale;
            for column in 0..columns {
                let u = (column as f32 * block as f32 + half_block) * inverse_height;
                let noise_x = (-1.0 + 2.0 * u - pan) * scale;
                let luminance = self.lattice.cloud_luminance(noise_x, noise_y, &shape);
                let tone = (luminance * LUMINANCE_GAIN - 0.5) * steepness + 0.5;
                self.tones.push(tone);
            }
        }
        columns
    }
}

impl Scene for FbmCloudsScene {
    fn render(&mut self, frame: &mut Frame<'_>) {
        let width = frame.raster.width;
        let height = frame.raster.height;
        if width == 0 || height == 0 {
            return;
        }
        let seconds = frame.time.as_secs_f64();
        let columns = self.sample_tones(width, height, seconds);
        let block = self.settings.block as usize;
        let intensity = self.settings.intensity as f32 / 100.0;
        let invert = self.settings.invert;
        for y in 0..height {
            let tone_row = &self.tones[(y / block) * columns..];
            let dither_row = &self.dither_tile[(y % DITHER_TILE) * DITHER_TILE..];
            let dot_row = &mut frame.raster.dots[y * width..(y + 1) * width];
            for (x, dot) in dot_row.iter_mut().enumerate() {
                let tone = tone_row[x / block];
                let mut lit = tone >= dither_row[x % DITHER_TILE];
                let mut soft = tone.clamp(0.0, 1.0);
                if invert {
                    lit = !lit;
                    soft = 1.0 - soft;
                }
                let hard = if lit { 1.0 - GLOW_SHARE } else { 0.0 };
                *dot = intensity * (hard + GLOW_SHARE * soft);
            }
        }
    }

    fn frames_per_second(&self) -> u32 {
        FRAMES_PER_SECOND
    }
}

/// One repeating tile of thresholds in 0..1.
fn dither_tile(pattern: DitherPattern, seed: u32) -> Vec<f32> {
    const BAYER_8: [[u8; 8]; 8] = [
        [0, 48, 12, 60, 3, 51, 15, 63],
        [32, 16, 44, 28, 35, 19, 47, 31],
        [8, 56, 4, 52, 11, 59, 7, 55],
        [40, 24, 36, 20, 43, 27, 39, 23],
        [2, 50, 14, 62, 1, 49, 13, 61],
        [34, 18, 46, 30, 33, 17, 45, 29],
        [10, 58, 6, 54, 9, 57, 5, 53],
        [42, 26, 38, 22, 41, 25, 37, 21],
    ];
    let mut tile = Vec::with_capacity(DITHER_TILE * DITHER_TILE);
    for y in 0..DITHER_TILE {
        for x in 0..DITHER_TILE {
            tile.push(match pattern {
                DitherPattern::Bayer => (f32::from(BAYER_8[y % 8][x % 8]) + 0.5) / 64.0,
                DitherPattern::Gradient => {
                    let ramp = 0.067_110_56 * x as f32 + 0.005_837_15 * y as f32;
                    (52.982_918 * ramp.fract()).fract()
                }
                DitherPattern::White => {
                    crate::raster::hash(x as i32 + seed as i32 * 64, y as i32 * 3 + 1)
                }
            });
        }
    }
    tile
}

/// The scene a spec describes (the host's spec with a `settings` object, or a bare settings
/// object); anything missing keeps its default and every value is clamped to its range.
pub fn create(spec: &serde_json::Value, env: &SceneEnv) -> Box<dyn Scene> {
    let settings: FbmCloudsSettings = crate::kit::settings::from_spec(spec);
    Box::new(FbmCloudsScene::new(&settings, env))
}

#[cfg(test)]
mod tests;

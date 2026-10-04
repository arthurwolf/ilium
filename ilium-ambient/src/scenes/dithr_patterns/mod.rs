//! Dithered procedural animation studio: a slow, seamlessly looping scalar
//! field (caustics, cells, halftone, stars, moire, smoke, spiral, tunnel,
//! plasma, ripples) is thresholded into Braille dots by an ordered,
//! error-diffusion or noise dither.
//!
//! The original relies on GPU shaders; this software renderer is the
//! full-quality path. It is deterministic in (`Frame::time`, settings) and
//! never reads the clock or blocks; only the optional GPU backend owns a worker thread.

mod dither;
mod gpu;
mod noise;
mod patterns;
mod settings;

pub use settings::DithrPatternsSettings;

use crate::control::SceneSettings;
use crate::gpu::GpuBackend;
use crate::scene::{Frame, Scene, SceneEnv};
use dither::{dither_field, BayerTable, DitherPass};
use noise::LatticeNoise;
use patterns::{make_stars, render_pattern, FieldContext, PatternScratch, Star};
use settings::RenderBackend;

pub const INSPIRED_BY: &[&str] = &["https://www.dithr.app/"];

/// Internal time scale: the animation runs at half the host clock.
const SLOW_MOTION: f64 = 0.5;
const FRAMES_PER_SECOND: u32 = 8;

pub struct DithrPatternsScene {
    settings: DithrPatternsSettings,
    gpu: GpuBackend,
    noise: LatticeNoise,
    bayer: BayerTable,
    stars: Vec<Star>,
    scratch: PatternScratch,
    field: Vec<f32>,
    carry_row: Vec<f32>,
    next_row: Vec<f32>,
    /// The last finished frame, replayed when the clock has not moved.
    cached: Option<CachedFrame>,
}

struct CachedFrame {
    time_bits: u64,
    width: usize,
    height: usize,
    dots: Vec<f32>,
}

impl DithrPatternsScene {
    // PALETTE (future plugin contract): `env.palette` is the shared look's current
    // palette. When animations become plugins, the plugin constructor receives the
    // current palette and MUST follow it: scenes with natural colours shift them
    // onto it (`ScenePalette::recolor`/`at`), and `Scene::set_palette` delivers later
    // changes. Monochrome scenes may ignore it. Today `PaletteScene` (scene.rs),
    // which `create_scene` wraps around every scene, shifts this scene's cell
    // colours onto the palette by brightness.
    pub fn new(settings: &DithrPatternsSettings, env: &SceneEnv) -> Self {
        let settings = settings.normalized();
        Self {
            gpu: GpuBackend::new(settings.render_backend == RenderBackend::Gpu, env),
            noise: LatticeNoise::new(settings.seed),
            bayer: BayerTable::new(settings.matrix),
            stars: make_stars(settings.seed, settings.complexity),
            scratch: PatternScratch::default(),
            field: Vec::new(),
            carry_row: Vec::new(),
            next_row: Vec::new(),
            cached: None,
            settings,
        }
    }

    fn render_software(&mut self, frame: &mut Frame<'_>) {
        let (width, height) = (frame.raster.width, frame.raster.height);
        let time = frame.time.as_secs_f64() * SLOW_MOTION;
        let loop_fraction = (time / f64::from(self.settings.loop_seconds)).fract() as f32;

        self.field.resize(width * height, 0.0);
        self.carry_row.resize(width + 2, 0.0);
        self.next_row.resize(width + 2, 0.0);

        let context = FieldContext {
            width,
            height,
            side: width.min(height) as f32,
            tau: loop_fraction * std::f32::consts::TAU,
            loop_fraction,
            frequency: self.settings.scale as f32 / 25.0,
            complexity: self.settings.complexity,
            seed: self.settings.seed,
            noise: &self.noise,
        };
        render_pattern(
            self.settings.pattern,
            &context,
            &self.stars,
            &mut self.scratch,
            &mut self.field,
        );

        let bias = (self.settings.density as f32 - 50.0) / 100.0;
        let invert = self.settings.invert;
        for value in &mut self.field {
            let biased = (*value + bias).clamp(0.0, 1.0);
            *value = if invert { 1.0 - biased } else { biased };
        }

        // Dither into the raster, then turn on/off into intensities.
        let pass = DitherPass {
            kind: self.settings.dither,
            amount: self.settings.dither_amount as f32 / 100.0,
            bayer: &self.bayer,
            seed: self.settings.seed,
            time,
        };
        dither_field(
            &pass,
            &self.field,
            width,
            &mut frame.raster.dots,
            &mut self.carry_row,
            &mut self.next_row,
        );
        let contrast = self.settings.contrast as f32 / 100.0;
        for (dot, level) in frame.raster.dots.iter_mut().zip(&self.field) {
            *dot = if *dot > 0.5 {
                contrast * (0.6 + 0.4 * level)
            } else {
                0.0
            };
        }
    }
}

impl Scene for DithrPatternsScene {
    fn render(&mut self, frame: &mut Frame<'_>) {
        let (width, height) = (frame.raster.width, frame.raster.height);
        if width == 0 || height == 0 {
            return;
        }
        let time_bits = frame.time.as_secs_f64().to_bits();
        if let Some(cached) = &self.cached {
            if cached.time_bits == time_bits && cached.width == width && cached.height == height {
                frame.raster.dots.copy_from_slice(&cached.dots);
                return;
            }
        }
        // With the GPU selected a job is submitted too (see `gpu`); the
        // software output stays on screen until the kernel is ported.
        let seconds = frame.time.as_secs_f64();
        let settings = &self.settings;
        self.gpu
            .drive(|| gpu::build_job(settings, width, height, seconds));
        self.render_software(frame);
        self.cached = Some(CachedFrame {
            time_bits,
            width,
            height,
            dots: frame.raster.dots.clone(),
        });
    }

    fn frames_per_second(&self) -> u32 {
        FRAMES_PER_SECOND
    }

    fn status(&self) -> Option<String> {
        self.gpu.status()
    }
}

#[cfg(test)]
mod tests;

//! Slowly rolling waves cut into a fixed ordered-dither lattice.
//!
//! A domain-warped noise field (see `field`) supplies a smooth luminance; a
//! Bayer (or noise) threshold tile turns it into crisp dots. The tile is
//! fixed to the screen and only the field moves, so the dots never flicker.
//! Provenance: the dither math follows the ordered-dither article; the wave
//! field is a designed reconstruction, not a copy of the demo's shader.
//!
//! The field is evaluated on a half-resolution node grid and bilinearly
//! upsampled, which is visually identical for these smooth bands and four
//! times cheaper. Render is a pure function of `Frame::time` and the seed.
//!
//! No external data source is used.

mod field;
mod gpu;
mod settings;

pub use settings::DitheredWavesSettings;

use settings::{DitherMatrix, RenderBackend};

use crate::gpu::GpuBackend;

use crate::control::SceneSettings;
use crate::scene::{Frame, Scene, SceneEnv};
use field::{ThresholdTile, WaveField};

pub const INSPIRED_BY: &[&str] = &["https://r3f.maximeheckel.com/dithered-waves-2"];

/// Drift time units per second at 100% wave speed.
const BASE_DRIFT_RATE: f32 = 0.3;
/// Field nodes sit every this many dots.
const NODE_SPACING: usize = 2;
/// Field value below which the wave is fully dark.
const FIELD_FLOOR: f32 = 0.35;

pub struct DitheredWavesScene {
    settings: DitheredWavesSettings,
    gpu: GpuBackend,
    field: WaveField,
    tile: ThresholdTile,
    /// Field samples at every `NODE_SPACING`th dot; reused between frames.
    nodes: Vec<f32>,
}

impl DitheredWavesScene {
    pub fn new(settings: &DitheredWavesSettings, env: &SceneEnv) -> Self {
        let settings = settings.normalized();
        let field = WaveField::new(
            settings.seed,
            settings.wave_frequency as f32 / 100.0,
            settings.wave_amplitude as f32 / 100.0,
            settings.ripple,
        );
        let tile = match settings.dither_matrix {
            DitherMatrix::Bayer2 => ThresholdTile::bayer(2),
            DitherMatrix::Bayer4 => ThresholdTile::bayer(4),
            DitherMatrix::Bayer8 => ThresholdTile::bayer(8),
            DitherMatrix::Noise => ThresholdTile::noise(settings.seed),
        };
        Self {
            gpu: GpuBackend::new(settings.render_backend == RenderBackend::Gpu, env),
            settings,
            field,
            tile,
            nodes: Vec::new(),
        }
    }

    /// Software output is always drawn; with the GPU selected a job is also
    /// submitted (see `gpu`), and the pixels switch over once the kernel is ported.
    fn render_with_backend(&mut self, frame: &mut Frame<'_>) {
        let width = frame.raster.width;
        let height = frame.raster.height;
        let seconds = frame.time.as_secs_f64();
        let settings = &self.settings;
        self.gpu
            .drive(|| gpu::build_job(settings, width, height, seconds));
        self.render_software(frame);
    }

    fn render_software(&mut self, frame: &mut Frame<'_>) {
        let width = frame.raster.width;
        let height = frame.raster.height;
        if width == 0 || height == 0 {
            return;
        }
        let drift =
            frame.time.as_secs_f32() * BASE_DRIFT_RATE * self.settings.wave_speed as f32 / 100.0;
        let node_columns = width / NODE_SPACING + 2;
        let node_rows = height / NODE_SPACING + 2;
        self.nodes.resize(node_columns * node_rows, 0.0);
        let inverse_height = 1.0 / height as f32;
        let half_width = width as f32 * 0.5;
        let half_height = height as f32 * 0.5;
        for row in 0..node_rows {
            let y = ((row * NODE_SPACING) as f32 - half_height) * inverse_height;
            for column in 0..node_columns {
                let x = ((column * NODE_SPACING) as f32 - half_width) * inverse_height;
                self.nodes[row * node_columns + column] = self.field.sample(x, y, drift);
            }
        }

        let contrast = self.settings.contrast as f32 / 100.0;
        let bias = self.settings.bias as f32 / 100.0;
        let steps = (self.settings.levels - 1) as f32;
        let peak = self.settings.brightness as f32 / 100.0;
        let pixel = self.settings.pixel_size as usize;
        let node_scale = 1.0 / NODE_SPACING as f32;
        for y in 0..height {
            let sample_y = ((y / pixel * pixel) as f32 + pixel as f32 * 0.5) * node_scale;
            let row = (sample_y as usize).min(node_rows - 2);
            let row_fraction = sample_y - row as f32;
            for x in 0..width {
                let sample_x = ((x / pixel * pixel) as f32 + pixel as f32 * 0.5) * node_scale;
                let column = (sample_x as usize).min(node_columns - 2);
                let column_fraction = sample_x - column as f32;
                let top_left = self.nodes[row * node_columns + column];
                let top_right = self.nodes[row * node_columns + column + 1];
                let bottom_left = self.nodes[(row + 1) * node_columns + column];
                let bottom_right = self.nodes[(row + 1) * node_columns + column + 1];
                let top = top_left + (top_right - top_left) * column_fraction;
                let bottom = bottom_left + (bottom_right - bottom_left) * column_fraction;
                let field_value = top + (bottom - top) * row_fraction;
                let luminance = ((field_value - FIELD_FLOOR) * contrast).clamp(0.0, 1.0);
                // Threshold is centred (minus 0.5) so quantising does not brighten.
                let threshold = self.tile.at(x / pixel, y / pixel);
                let level =
                    ((luminance + (threshold - 0.5) / steps + bias) * steps + 0.5).floor() / steps;
                frame.raster.dots[y * width + x] = level.clamp(0.0, 1.0) * peak;
            }
        }
    }
}

impl Scene for DitheredWavesScene {
    fn render(&mut self, frame: &mut Frame<'_>) {
        self.render_with_backend(frame);
    }

    fn frames_per_second(&self) -> u32 {
        8
    }

    fn status(&self) -> Option<String> {
        self.gpu.status()
    }
}

#[cfg(test)]
mod tests;

//! Slowly rolling waves cut into a fixed ordered-dither lattice (the `dithered-waves` scene).
//!
//! A domain-warped noise field (see `field`) supplies a smooth luminance; a
//! Bayer (or noise) threshold tile turns it into crisp dots. The tile is
//! fixed to the screen and only the field moves, so the dots never flicker.
//! Provenance: the dither math follows the ordered-dither article; the wave
//! field is a designed reconstruction, not a copy of the demo's shader.
//!
//! The field is evaluated on a half-resolution node grid and bilinearly
//! upsampled, which is visually identical for these smooth bands and four
//! times cheaper. Render is a pure function of the clock and the seed.
//!
//! No external data source is used. Colour, palette, the shared dither and display stay the
//! host's: this scene writes grey ink only.
//!
//! PALETTE (future plugin contract): the palette is passed to the scene (`set_palette`) and
//! a scene with natural colours must follow it. This scene draws tone only, so it ignores
//! the palette and the shared look colours it.

mod field;
mod settings;

pub use settings::{DitheredWavesSettings, WavePattern};

use field::{ThresholdTile, WaveField};
use serde_json::Value;

/// The page this scene's design comes from.
pub const INSPIRED_BY: &[&str] = &["https://r3f.maximeheckel.com/dithered-waves-2"];

/// Drift time units per second at 100% wave speed.
const BASE_DRIFT_RATE: f32 = 0.3;
/// Field nodes sit every this many dots.
const NODE_SPACING: usize = 2;
/// Field value below which the wave is fully dark.
const FIELD_FLOOR: f32 = 0.35;
/// The frame rate the scene asks for: gentle, slow motion.
pub const FRAMES_PER_SECOND: u32 = 8;

pub struct DitheredWaves {
    settings: DitheredWavesSettings,
    field: WaveField,
    tile: ThresholdTile,
    /// Field samples at every `NODE_SPACING`th dot; reused between frames.
    nodes: Vec<f32>,
}

impl DitheredWaves {
    pub fn new(settings: &DitheredWavesSettings) -> Self {
        let settings = settings.normalized();
        let field = WaveField::new(
            settings.seed,
            settings.wave_frequency as f32 / 100.0,
            settings.wave_amplitude as f32 / 100.0,
            settings.ripple,
        );
        let tile = match settings.pattern {
            WavePattern::Bayer2 => ThresholdTile::bayer(2),
            WavePattern::Bayer4 => ThresholdTile::bayer(4),
            WavePattern::Bayer8 => ThresholdTile::bayer(8),
            WavePattern::Noise => ThresholdTile::noise(settings.seed),
        };
        Self {
            settings,
            field,
            tile,
            nodes: Vec::new(),
        }
    }

    /// The scene of a spec (the host's spec or a bare settings object).
    pub fn from_spec(spec: &Value) -> Self {
        Self::new(&DitheredWavesSettings::from_spec(spec))
    }

    /// The settings the scene runs with, after clamping.
    pub fn settings(&self) -> &DitheredWavesSettings {
        &self.settings
    }

    /// Draws the waves at `time_millis` into `pixels` (`width * height` grey values, row by
    /// row). The result depends only on the settings and the time.
    pub fn render(&mut self, width: usize, height: usize, time_millis: u64, pixels: &mut [u8]) {
        if width == 0 || height == 0 || pixels.len() < width * height {
            return;
        }
        let seconds = (time_millis as f64 / 1000.0) as f32;
        let drift = seconds * BASE_DRIFT_RATE * self.settings.wave_speed as f32 / 100.0;
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

        let band_sharpness = self.settings.band_sharpness as f32 / 100.0;
        let bias = self.settings.bias as f32 / 100.0;
        let steps = (self.settings.levels - 1) as f32;
        let peak = self.settings.intensity as f32 / 100.0;
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
                let luminance = ((field_value - FIELD_FLOOR) * band_sharpness).clamp(0.0, 1.0);
                // Select low threshold ranks first, like the shared ordered dither,
                // so the two aligned lattices do not cancel each other.
                let threshold = self.tile.at(x / pixel, y / pixel);
                let level =
                    ((luminance + (0.5 - threshold) / steps + bias) * steps + 0.5).floor() / steps;
                pixels[y * width + x] = (level.clamp(0.0, 1.0) * peak * 255.0).round() as u8;
            }
        }
    }
}

#[cfg(test)]
mod tests;

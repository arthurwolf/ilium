//! Quiet dithered seascape with a day cycle, after the look of Atlantic '41.
//!
//! A fixed-in-screen-space gradient sky over a homogeneous sea. Only the
//! clouds and the wave lines move, so the gradient dither never shimmers.
//! By day the clouds are lighter than the sky; at dusk the relation inverts;
//! at night the sky is black with stars, a crescent moon and sparse glints.
//!
//! Every value derives from the settings and `Frame::time`; nothing reads
//! the clock, blocks or spawns a thread. No external data source is used.

mod settings;
#[cfg(test)]
mod tests;

pub use settings::AtlanticDuskSettings;

use settings::DitherStyle;

use crate::control::SceneSettings;
use crate::raster::smoothstep;
use crate::scene::{Frame, Scene, SceneEnv};
use std::f32::consts::{PI, TAU};

/// Pages this scene is inspired by.
pub const INSPIRED_BY: &[&str] = &[
    "https://stephanrewind.itch.io/atlantic-41/devlog/465501/red-sky-at-night-sailors-delight-red-sky-in-the-morning-sailors-take-warning",
    "https://stephanrewind.itch.io/atlantic-41/devlog/498094/the-midnight-hour",
    "https://stephanrewind.itch.io/atlantic-41/devlog/400353/ups-and-downs",
];

/// The cycle starts in the afternoon so the first minute shows daylight
/// drifting towards dusk.
const START_PHASE: f32 = 0.3;
const HORIZON_FRACTION: f32 = 0.42;
/// Cloud noise cell size in dots (wide, flat clouds).
const CLOUD_CELL_X: f32 = 26.0;
const CLOUD_CELL_Y: f32 = 8.0;
/// Noise cells per second at 100 % cloud speed.
const WIND_CELLS_PER_SECOND: f32 = 0.03;
const CLOUD_OCTAVES: usize = 4;
const CLOUD_CONTRAST: f32 = 0.18;
const STAR_CELL: usize = 4;

const BAYER_4: [[u8; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];
const BAYER_QUADRANT: [[u8; 2]; 2] = [[0, 2], [3, 1]];

/// Threshold tile of an 8x8 pattern, values in (0, 1). Built once.
fn threshold_tile(style: DitherStyle) -> [[f32; 8]; 8] {
    let mut tile = [[0.5; 8]; 8];
    for (y, row) in tile.iter_mut().enumerate() {
        for (x, cell) in row.iter_mut().enumerate() {
            *cell = match style {
                DitherStyle::Host => 0.5,
                DitherStyle::Bayer4 => (f32::from(BAYER_4[y & 3][x & 3]) + 0.5) / 16.0,
                DitherStyle::Bayer8 => {
                    let value = 4 * u32::from(BAYER_4[y & 3][x & 3])
                        + u32::from(BAYER_QUADRANT[(y >> 2) & 1][(x >> 2) & 1]);
                    (value as f32 + 0.5) / 64.0
                }
            };
        }
    }
    tile
}

fn hash_u32(a: i32, b: i32, c: u32) -> u32 {
    let mut value = (a as u32).wrapping_mul(0x8da6_b343)
        ^ (b as u32).wrapping_mul(0xd816_3841)
        ^ c.wrapping_mul(0x9e37_79b9)
        ^ 0xcb1a_b31f;
    value ^= value >> 16;
    value = value.wrapping_mul(0x7feb_352d);
    value ^= value >> 15;
    value = value.wrapping_mul(0x846c_a68b);
    value ^= value >> 16;
    value
}

fn hash_unit(a: i32, b: i32, c: u32) -> f32 {
    (hash_u32(a, b, c) >> 8) as f32 / 16_777_216.0
}

fn lerp(from: f32, to: f32, amount: f32) -> f32 {
    from + (to - from) * amount
}

/// Sun, moon and light factors for one instant.
struct Day {
    daylight: f32,
    twilight: f32,
    night: f32,
    sun_x: f32,
    sun_y: f32,
    sun_radius: f32,
    sun_visible: bool,
    sun_strength: f32,
    moon_x: f32,
    moon_y: f32,
    moon_radius: f32,
    moon_strength: f32,
}

impl Day {
    fn at(phase: f32, width: f32, height: f32, horizon: f32) -> Self {
        let angle = TAU * phase;
        let elevation = angle.sin();
        let daylight = smoothstep(-0.15, 0.35, elevation);
        let twilight = 1.0 - smoothstep(0.0, 0.45, elevation.abs());
        let night = 1.0 - smoothstep(-0.30, -0.05, elevation);
        let low_sun = 1.0 - elevation.clamp(0.0, 1.0);
        Self {
            daylight,
            twilight,
            night,
            sun_x: width * 0.5 + width * 0.35 * angle.cos(),
            sun_y: horizon - elevation * horizon * 0.85,
            sun_radius: height * (0.07 + 0.03 * low_sun),
            sun_visible: elevation > -0.02,
            sun_strength: smoothstep(-0.02, 0.1, elevation),
            moon_x: width * 0.5 - width * 0.30 * angle.cos(),
            moon_y: horizon + elevation * horizon * 0.75,
            moon_radius: height * 0.05,
            moon_strength: smoothstep(0.05, -0.12, elevation),
        }
    }
}

/// Per-octave, per-row part of the cloud noise (constant across a row).
#[derive(Clone, Copy, Default)]
struct NoiseRow {
    lattice_y: i32,
    smooth_y: f32,
}

pub struct AtlanticDuskScene {
    settings: AtlanticDuskSettings,
    tile: [[f32; 8]; 8],
    /// Reused per-frame row tables.
    row_noise: Vec<[NoiseRow; CLOUD_OCTAVES]>,
    row_base: Vec<f32>,
    row_cloud_fade: Vec<f32>,
}

impl AtlanticDuskScene {
    pub fn new(settings: &AtlanticDuskSettings, _env: &SceneEnv) -> Self {
        let settings = settings.normalized();
        Self {
            tile: threshold_tile(settings.dither),
            settings,
            row_noise: Vec::new(),
            row_base: Vec::new(),
            row_cloud_fade: Vec::new(),
        }
    }

    fn phase(&self, seconds: f32) -> f32 {
        match self.settings.time_of_day.pinned_phase() {
            Some(phase) => phase,
            None => {
                let cycle = seconds / self.settings.day_length_seconds as f32 + START_PHASE;
                cycle - cycle.floor()
            }
        }
    }

    /// Turn a pre-dither level into a dot. Bayer modes step the level into
    /// eight bands first so the gradient reads as stripes of pattern.
    fn shade(&self, x: usize, y: usize, level: f32) -> f32 {
        match self.settings.dither {
            DitherStyle::Host => level.clamp(0.0, 1.0),
            _ => {
                let stepped = (level * 8.0).floor() / 8.0;
                if stepped > self.tile[y & 7][x & 7] {
                    1.0
                } else {
                    0.0
                }
            }
        }
    }

    /// Like `shade` for elements that are always lit at full strength.
    fn bright(&self, strength: f32) -> f32 {
        strength.clamp(0.0, 1.0)
    }

    fn prepare_rows(&mut self, height: usize, horizon_rows: usize, day: &Day) {
        let horizon = horizon_rows.max(1) as f32;
        self.row_noise.clear();
        self.row_base.clear();
        self.row_cloud_fade.clear();
        let seed = self.settings.seed;
        for y in 0..horizon_rows.min(height) {
            let sky_t = ((horizon - y as f32 - 0.5) / horizon).clamp(0.0, 1.0);
            let day_gradient = 0.10 + 0.55 * (1.0 - sky_t).powf(1.6);
            let glow = day.twilight * (-(sky_t / 0.35).powi(2)).exp();
            let twilight_gradient = 0.15 + 0.6 * glow;
            let lit = lerp(day_gradient, twilight_gradient, day.twilight);
            let night_gradient = 0.20 * (1.0 - sky_t / 0.4).max(0.0).powi(2);
            self.row_base.push(lerp(lit, night_gradient, day.night));
            let fade = (PI * (sky_t / 0.9).clamp(0.0, 1.0))
                .sin()
                .max(0.0)
                .powf(0.7);
            self.row_cloud_fade.push(fade);
            // Rows are grouped in pairs for flat-bottomed cloud strata.
            let sample_y = (y / 2 * 2) as f32 / CLOUD_CELL_Y;
            let mut octaves = [NoiseRow::default(); CLOUD_OCTAVES];
            for (octave, slot) in octaves.iter_mut().enumerate() {
                let offset = hash_unit(octave as i32, 7, seed) * 64.0;
                let scaled = sample_y * (1u32 << octave) as f32 + offset;
                let lattice_y = scaled.floor();
                let fraction = scaled - lattice_y;
                *slot = NoiseRow {
                    lattice_y: lattice_y as i32,
                    smooth_y: fraction * fraction * (3.0 - 2.0 * fraction),
                };
            }
            self.row_noise.push(octaves);
        }
    }

    fn cloud_noise(&self, x: usize, octaves: &[NoiseRow; CLOUD_OCTAVES], shift: f32) -> f32 {
        let seed = self.settings.seed;
        let base_x = (x as f32 + 0.5) / CLOUD_CELL_X + shift;
        let mut total = 0.0;
        let mut amplitude = 1.0;
        let mut norm = 0.0;
        for (octave, row) in octaves.iter().enumerate() {
            let scaled =
                base_x * (1u32 << octave) as f32 + hash_unit(octave as i32, 3, seed) * 64.0;
            let lattice_x = scaled.floor();
            let fraction = scaled - lattice_x;
            let smooth_x = fraction * fraction * (3.0 - 2.0 * fraction);
            let ix = lattice_x as i32;
            let salt = seed.wrapping_add(octave as u32 * 101);
            let top = lerp(
                hash_unit(ix, row.lattice_y, salt),
                hash_unit(ix + 1, row.lattice_y, salt),
                smooth_x,
            );
            let bottom = lerp(
                hash_unit(ix, row.lattice_y + 1, salt),
                hash_unit(ix + 1, row.lattice_y + 1, salt),
                smooth_x,
            );
            total += amplitude * lerp(top, bottom, row.smooth_y);
            norm += amplitude;
            amplitude *= 0.5;
        }
        total / norm
    }

    fn render_sky(&mut self, frame_raster: &mut [f32], width: usize, day: &Day, seconds: f32) {
        let coverage = self.settings.cloud_coverage as f32 / 100.0;
        let clouds_active = coverage > 0.01 && day.night < 0.95;
        let contrast = self.settings.contrast as f32 / 100.0;
        let shift = seconds * WIND_CELLS_PER_SECOND * self.settings.cloud_speed as f32 / 100.0;
        let sign = lerp(
            1.0,
            -1.0,
            smoothstep(0.35, 0.65, day.twilight + day.night * 0.5),
        );
        let cloud_gain = 1.0 - 0.8 * day.night;
        let haze_reach = day.sun_radius * 4.0;
        let haze_strength = if day.sun_visible && day.twilight > 0.1 {
            0.35 * day.twilight
        } else {
            0.0
        };
        for y in 0..self.row_base.len() {
            let base = self.row_base[y];
            let fade = self.row_cloud_fade[y] * cloud_gain;
            let octaves = self.row_noise[y];
            let dy = y as f32 + 0.5 - day.sun_y;
            for x in 0..width {
                let mut level = base;
                if haze_strength > 0.0 {
                    let dx = x as f32 + 0.5 - day.sun_x;
                    let distance_squared = dx * dx + dy * dy;
                    if distance_squared < haze_reach * haze_reach * 9.0 {
                        level +=
                            (-distance_squared / (haze_reach * haze_reach)).exp() * haze_strength;
                    }
                }
                if clouds_active && fade > 0.001 {
                    let noise = self.cloud_noise(x, &octaves, shift);
                    let presence =
                        smoothstep(1.0 - coverage - 0.08, 1.0 - coverage + 0.08, noise) * fade;
                    if presence > 0.0 {
                        let cloud_level = (level + sign * CLOUD_CONTRAST).clamp(0.0, 1.0);
                        level = lerp(level, cloud_level, presence);
                    }
                }
                frame_raster[y * width + x] = self.shade(x, y, level * contrast);
            }
        }
    }

    fn render_stars(
        &self,
        dots: &mut [f32],
        width: usize,
        horizon_rows: usize,
        day: &Day,
        seconds: f32,
    ) {
        let density = self.settings.star_density as f32 / 1000.0;
        if day.night < 0.15 || density <= 0.0 {
            return;
        }
        let seed = self.settings.seed;
        let horizon = horizon_rows.max(1) as f32;
        for cell_y in 0..horizon_rows.div_ceil(STAR_CELL) {
            for cell_x in 0..width.div_ceil(STAR_CELL) {
                let (cx, cy) = (cell_x as i32, cell_y as i32);
                if hash_unit(cx, cy, seed) >= density {
                    continue;
                }
                let picked = hash_u32(cx, cy, seed ^ 0x51ed);
                let x = cell_x * STAR_CELL + (picked & 3) as usize;
                let y = cell_y * STAR_CELL + ((picked >> 2) & 3) as usize;
                if x >= width || y >= horizon_rows {
                    continue;
                }
                let sky_t = (horizon - y as f32 - 0.5) / horizon;
                if sky_t <= 0.12 {
                    continue;
                }
                // Stars fade in one by one as the night deepens.
                let appears_at = 0.15 + 0.85 * hash_unit(cx, cy, seed ^ 0x77);
                if day.night < appears_at {
                    continue;
                }
                let flicker = hash_unit(cx, cy, seed ^ 0x1f) * TAU;
                let blinked = self.settings.twinkle
                    && picked & 0x30 == 0
                    && (seconds * 1.7 + flicker).sin() < -0.92;
                if !blinked {
                    dots[y * width + x] = 1.0;
                }
            }
        }
    }

    fn render_moon(&self, dots: &mut [f32], width: usize, horizon_rows: usize, day: &Day) {
        if day.moon_strength < 0.05 {
            return;
        }
        let radius = day.moon_radius.max(1.0);
        let bite_x = day.moon_x + radius * 0.5;
        let left = (day.moon_x - radius - 1.0).max(0.0) as usize;
        let right = ((day.moon_x + radius + 1.0).max(0.0) as usize).min(width);
        let top = (day.moon_y - radius - 1.0).max(0.0) as usize;
        let bottom = ((day.moon_y + radius + 1.0).max(0.0) as usize).min(horizon_rows);
        for y in top..bottom {
            for x in left..right {
                let px = x as f32 + 0.5;
                let py = y as f32 + 0.5;
                let inside = (px - day.moon_x).hypot(py - day.moon_y) <= radius;
                let bitten = (px - bite_x).hypot(py - day.moon_y) <= radius * 0.9;
                if inside && !bitten {
                    dots[y * width + x] = self.bright(day.moon_strength);
                }
            }
        }
    }

    fn render_sun(&self, dots: &mut [f32], width: usize, horizon_rows: usize, day: &Day) {
        if !day.sun_visible {
            return;
        }
        let radius = day.sun_radius.max(1.0);
        let left = (day.sun_x - radius - 2.0).max(0.0) as usize;
        let right = ((day.sun_x + radius + 2.0).max(0.0) as usize).min(width);
        let top = (day.sun_y - radius - 2.0).max(0.0) as usize;
        let bottom = ((day.sun_y + radius + 2.0).max(0.0) as usize).min(horizon_rows);
        for y in top..bottom {
            for x in left..right {
                let distance = (x as f32 + 0.5 - day.sun_x).hypot(y as f32 + 0.5 - day.sun_y);
                if distance <= radius {
                    dots[y * width + x] = 1.0;
                } else if distance <= radius + 1.0 {
                    let ring = self.shade(x, y, 0.5 + 0.5 * day.daylight);
                    dots[y * width + x] = dots[y * width + x].max(ring);
                }
            }
        }
    }

    fn render_sea(
        &self,
        dots: &mut [f32],
        width: usize,
        height: usize,
        horizon_rows: usize,
        day: &Day,
        seconds: f32,
    ) {
        let contrast = self.settings.contrast as f32 / 100.0;
        let wave_scale = self.settings.wave_scale as f32 / 100.0;
        let wave_clock = seconds * self.settings.wave_speed as f32 / 100.0 * 1.1;
        let sea_rows = (height - horizon_rows).max(1) as f32;
        let sun_glitter = (day.twilight * 0.5 + day.daylight * 0.12) * day.sun_strength;
        let moon_glitter = 0.3 * day.night * day.moon_strength;
        let reflect_sun = day.sun_visible && sun_glitter > 0.01;
        let reflect_moon = moon_glitter > 0.01;
        let width_f = width as f32;
        for y in horizon_rows..height {
            let depth = (y as f32 + 0.5 - horizon_rows as f32) / sea_rows;
            let near = 1.0 - depth;
            let day_base = 0.18 + 0.30 * near * near;
            let night_base = 0.06 + 0.15 * near * near;
            let base = lerp(day_base, night_base, day.night);
            let perspective = 1.0 / (depth + 0.22);
            let phase = perspective * wave_scale * 8.0 - wave_clock;
            let lateral_scale = 0.09 / (0.5 + depth * 1.5);
            let lateral_shift = 0.8 * (perspective * 0.7 + wave_clock * 0.18).sin();
            let day_threshold = 0.80 - 0.14 * depth;
            let threshold = lerp(day_threshold, 0.90, day.night);
            let spread = width_f * 0.06 * (1.0 + 3.0 * depth);
            for x in 0..width {
                let mut level = base;
                let px = x as f32 + 0.5;
                if reflect_sun {
                    let offset = (px - day.sun_x) / spread;
                    if offset.abs() < 3.0 {
                        level += (-offset * offset).exp() * 0.5 * sun_glitter;
                    }
                }
                if reflect_moon {
                    let offset = (px - day.moon_x) / spread;
                    if offset.abs() < 3.0 {
                        level += (-offset * offset).exp() * moon_glitter;
                    }
                }
                let lateral = px * lateral_scale + lateral_shift;
                let height_value = 0.6 * (phase + 0.9 * lateral.sin()).sin()
                    + 0.3 * (phase * 1.7 + lateral * 1.3 + 1.7).sin()
                    + 0.1 * (phase * 3.1 - lateral * 2.1).sin();
                let index = y * width + x;
                if height_value > threshold {
                    let crest = lerp((level + 0.35) * contrast, 1.0, day.night);
                    dots[index] = self.shade(x, y, crest);
                } else {
                    dots[index] = self.shade(x, y, level * contrast);
                }
            }
        }
    }
}

impl Scene for AtlanticDuskScene {
    fn render(&mut self, frame: &mut Frame<'_>) {
        let width = frame.raster.width;
        let height = frame.raster.height;
        if width == 0 || height == 0 {
            return;
        }
        let seconds = frame.time.as_secs_f32();
        let horizon_rows = ((HORIZON_FRACTION * height as f32).round() as usize).min(height);
        let day = Day::at(
            self.phase(seconds),
            width as f32,
            height as f32,
            horizon_rows as f32,
        );
        self.prepare_rows(height, horizon_rows, &day);
        let mut dots = std::mem::take(&mut frame.raster.dots);
        self.render_sky(&mut dots, width, &day, seconds);
        self.render_stars(&mut dots, width, horizon_rows, &day, seconds);
        self.render_moon(&mut dots, width, horizon_rows, &day);
        self.render_sun(&mut dots, width, horizon_rows, &day);
        self.render_sea(&mut dots, width, height, horizon_rows, &day, seconds);
        frame.raster.dots = dots;
    }

    fn frames_per_second(&self) -> u32 {
        12
    }

    fn status(&self) -> Option<String> {
        None
    }
}

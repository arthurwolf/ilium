//! One-bit water: a drifting caustic light field passed through a fixed
//! ordered-dither matrix.
//!
//! The dither pattern is anchored to the screen dots, only the underlying
//! height field moves, so pixels stay crisp while pattern boundaries crawl.
//! Optional expanding elliptical ripples and a horizon perspective add depth.
//!
//! Determinism: every frame is a pure function of (`Frame::time`, seed,
//! settings). Nothing is read from the system clock, no threads are spawned
//! and the only state is reusable scratch memory.
//!
//! No external data source is used.

mod settings;

pub use settings::DitherWaterSettings;

use crate::control::SceneSettings;
use crate::scene::{Frame, Scene, SceneEnv};
use crate::style::ScenePalette;
use std::f64::consts::TAU;

pub const INSPIRED_BY: &[&str] =
    &["https://www.reddit.com/r/PixelArt/comments/1sqw4hf/1bit_water_animation/"];

/// Ripple rings alive at once.
const RING_COUNT: u32 = 6;
/// Seconds a ring lives before it respawns somewhere else.
const RING_PERIOD_SECONDS: f64 = 6.0;
/// Ring lifetime radius as a fraction of the raster height.
const RING_MAX_RADIUS_FRACTION: f32 = 0.25;
/// Half width of a ring line in dots.
const RING_HALF_WIDTH: f32 = 1.2;
/// Value-noise lattice spacing in dots.
const NOISE_CELL_DOTS: f32 = 8.0;
/// Noise lattice cells the shimmer drifts per animation second.
const NOISE_DRIFT_CELLS_PER_SECOND: f64 = 0.35;
/// Time quantum of stepped motion.
const STEPS_PER_SECOND: f64 = 8.0;
/// Brightness setting (as a fraction) at which lit dots are at full strength
/// and the tint palette is unscaled.
const REFERENCE_BRIGHTNESS: f32 = 0.45;
const DEEP_WATER: [f32; 3] = [0.10, 0.25, 0.35];
const PALE_WATER: [f32; 3] = [0.55, 0.80, 0.90];

pub struct DitherWaterScene {
    settings: DitherWaterSettings,
    /// Row-major threshold matrix in (0,1), `matrix_side` squared entries.
    matrix: Vec<f32>,
    matrix_side: usize,
    /// Seeded phase offsets, radians.
    seed_phases: [f64; 5],
    /// Sum of field intensity per cell, reused between frames (tint only).
    cell_sums: Vec<f32>,
    cell_counts: Vec<f32>,
    /// The shared look's palette; tint colours are sampled from it when provided.
    palette: ScenePalette,
}

/// Recursive Bayer matrix of side `side` (power of two), values 0..side^2.
fn bayer_ranks(side: usize) -> Vec<u32> {
    let mut ranks = vec![0u32];
    let mut current = 1;
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
    ranks
}

fn hash32(mut value: u32) -> u32 {
    value ^= value >> 16;
    value = value.wrapping_mul(0x7feb_352d);
    value ^= value >> 15;
    value = value.wrapping_mul(0x846c_a68b);
    value ^ (value >> 16)
}

fn unit_hash(value: u32) -> f32 {
    (hash32(value) >> 8) as f32 / (1u32 << 24) as f32
}

fn lattice(ix: i32, iy: i32, seed: u32) -> f32 {
    unit_hash(
        (ix as u32)
            .wrapping_mul(374_761_393)
            .wrapping_add((iy as u32).wrapping_mul(668_265_263))
            .wrapping_add(seed.wrapping_mul(0x9e37_79b1)),
    )
}

fn smooth(fraction: f32) -> f32 {
    fraction * fraction * (3.0 - 2.0 * fraction)
}

fn mix(from: f32, to: f32, amount: f32) -> f32 {
    from + (to - from) * amount
}

/// Bilinear value noise with a smoothstep fade, in 0..1.
fn value_noise(x: f32, y: f32, integer_shift: (i32, i32), seed: u32) -> f32 {
    let (floor_x, floor_y) = (x.floor(), y.floor());
    let (fx, fy) = (smooth(x - floor_x), smooth(y - floor_y));
    let ix = floor_x as i32 + integer_shift.0;
    let iy = floor_y as i32 + integer_shift.1;
    let top = mix(lattice(ix, iy, seed), lattice(ix + 1, iy, seed), fx);
    let bottom = mix(lattice(ix, iy + 1, seed), lattice(ix + 1, iy + 1, seed), fx);
    mix(top, bottom, fy)
}

/// Per-frame constants shared by every dot.
struct FrameTerms {
    scale: f32,
    perspective: f32,
    sharpness: i32,
    bias: f32,
    /// Phases (three waves, then the x and y domain warps) including elapsed
    /// time, radians in 0..TAU.
    phases: [f32; 5],
    noise_shift: (i32, i32),
    noise_offset: (f32, f32),
    noise_seed: u32,
}

/// One ripple ring in dot coordinates.
#[derive(Clone, Copy)]
struct Ring {
    center_x: f32,
    center_y: f32,
    radius: f32,
    strength: f32,
}

impl DitherWaterScene {
    // PALETTE (native Scene contract): `env.palette` is the shared look's current
    // palette. A custom native Scene receives the
    // current palette and MUST follow it: scenes with natural colours shift them
    // onto it (`ScenePalette::recolor`/`at`), and `Scene::set_palette` delivers later
    // changes. This scene follows it natively: the tint ramp is sampled from the
    // palette (`follows_palette`), so `PaletteScene` skips its generic recolour.
    pub fn new(settings: &DitherWaterSettings, env: &SceneEnv) -> Self {
        let settings = settings.normalized();
        let matrix_side = settings.dither.side();
        let levels = (matrix_side * matrix_side) as f32;
        let matrix = bayer_ranks(matrix_side)
            .into_iter()
            .map(|rank| (rank as f32 + 0.5) / levels)
            .collect();
        let mut seed_phases = [0.0; 5];
        for (index, phase) in seed_phases.iter_mut().enumerate() {
            let hashed = hash32(
                settings
                    .seed
                    .wrapping_mul(31)
                    .wrapping_add(index as u32 + 1),
            );
            *phase = f64::from(hashed) / f64::from(u32::MAX) * TAU;
        }
        Self {
            settings,
            matrix,
            matrix_side,
            seed_phases,
            cell_sums: Vec::new(),
            cell_counts: Vec::new(),
            palette: env.palette.clone(),
        }
    }

    fn animation_seconds(&self, frame: &Frame<'_>) -> f64 {
        let seconds = frame.time.as_secs_f64();
        if self.settings.stepped {
            (seconds * STEPS_PER_SECOND).floor() / STEPS_PER_SECOND
        } else {
            seconds
        }
    }

    fn frame_terms(&self, seconds: f64) -> FrameTerms {
        let flow = seconds * f64::from(self.settings.flow_speed) / 100.0;
        let wrap = |phase: f64| phase.rem_euclid(TAU) as f32;
        let drift = flow * NOISE_DRIFT_CELLS_PER_SECOND;
        let drift_whole = drift.floor();
        FrameTerms {
            scale: self.settings.wave_scale as f32 / 100.0,
            perspective: self.settings.perspective as f32 / 100.0,
            sharpness: self.settings.sharpness as i32,
            bias: self.settings.density as f32 / 100.0,
            phases: [
                wrap(self.seed_phases[2] + flow * 1.0),
                wrap(self.seed_phases[3] - flow * 0.8),
                wrap(self.seed_phases[4] + flow * 1.6),
                wrap(self.seed_phases[0] + flow * 0.7),
                wrap(self.seed_phases[1] - flow * 0.5),
            ],
            noise_shift: (drift_whole as i32, 0),
            noise_offset: ((drift - drift_whole) as f32, 0.0),
            noise_seed: self.settings.seed,
        }
    }

    /// Light intensity at one dot before dithering, about 0..1.2.
    fn intensity(terms: &FrameTerms, row: &RowTerms, u: f32, x: f32, y: f32) -> f32 {
        let k = terms.scale;
        let warp_y = 0.15 * (1.7 * u * k + terms.phases[4]).sin();
        let wave_a = 0.50 * ((u + row.warp_x) * 6.0 * k + terms.phases[0]).sin();
        let wave_b = 0.30 * ((row.v_shaped + warp_y) * 9.0 * k + terms.phases[1]).sin();
        let wave_c = 0.20 * ((0.6 * u + 0.8 * row.v_shaped) * 14.0 * k + terms.phases[2]).sin();
        let height = (wave_a + wave_b + wave_c) * 0.5 + 0.5;
        let caustic = (1.0 - (2.0 * height - 1.0).abs())
            .clamp(0.0, 1.0)
            .powi(terms.sharpness);
        let shimmer = value_noise(
            x / NOISE_CELL_DOTS + terms.noise_offset.0,
            y / NOISE_CELL_DOTS + terms.noise_offset.1,
            terms.noise_shift,
            terms.noise_seed,
        );
        (caustic + 0.15 * (shimmer - 0.5)) * row.fade
    }

    fn rings(&self, seconds: f64, height: usize, width: usize) -> [Ring; RING_COUNT as usize] {
        let mut rings = [Ring {
            center_x: 0.0,
            center_y: 0.0,
            radius: 0.0,
            strength: 0.0,
        }; RING_COUNT as usize];
        if !self.settings.ripples {
            return rings;
        }
        let flow = seconds * f64::from(self.settings.flow_speed) / 100.0;
        let max_radius = height as f32 * RING_MAX_RADIUS_FRACTION;
        for (index, ring) in rings.iter_mut().enumerate() {
            let spawn = index as f64 * RING_PERIOD_SECONDS / f64::from(RING_COUNT);
            let age = flow - spawn;
            if age < 0.0 {
                continue;
            }
            let cycle = (age / RING_PERIOD_SECONDS).floor();
            let phase = (age - cycle * RING_PERIOD_SECONDS) / RING_PERIOD_SECONDS;
            let salt = self
                .settings
                .seed
                .wrapping_mul(7919)
                .wrapping_add(index as u32 * 613)
                .wrapping_add((cycle as u32).wrapping_mul(104_729));
            ring.center_x = unit_hash(salt) * width as f32;
            // Keep rings on the nearer half of the water.
            ring.center_y = (0.35 + 0.65 * unit_hash(salt ^ 0x5bd1_e995)) * height as f32;
            ring.radius = phase as f32 * max_radius;
            ring.strength = (1.0 - phase as f32) * 0.6;
        }
        rings
    }
}

/// Values shared by all dots of one row.
struct RowTerms {
    v_shaped: f32,
    warp_x: f32,
    fade: f32,
}

impl Scene for DitherWaterScene {
    fn render(&mut self, frame: &mut Frame<'_>) {
        let width = frame.raster.width;
        let height = frame.raster.height;
        if width == 0 || height == 0 {
            return;
        }
        let seconds = self.animation_seconds(frame);
        let terms = self.frame_terms(seconds);
        let rings = self.rings(seconds, height, width);
        let use_rings = self.settings.ripples;
        // The host thresholds dot intensity against its own dither matrix, so
        // a dot below 1.0 is thinned out a second time and the scene's own
        // pattern turns into a regular texture. Lit dots therefore stay at
        // 1.0 by default; brightness below the reference thins them on
        // purpose, brightness above it brightens the tint colours.
        let ratio = self.settings.brightness as f32 / 100.0 / REFERENCE_BRIGHTNESS;
        let level = ratio.min(1.0);
        let color_gain = ratio.max(1.0);
        let tint = self.settings.tint;
        let cell_columns = usize::from(frame.width);
        let cell_rows = usize::from(frame.height);
        if tint {
            self.cell_sums.clear();
            self.cell_sums.resize(cell_columns * cell_rows, 0.0);
            self.cell_counts.clear();
            self.cell_counts.resize(cell_columns * cell_rows, 0.0);
        }

        let height_f = height as f32;
        let half_width = width as f32 * 0.5;
        let side = self.matrix_side;
        for y in 0..height {
            let v = (y as f32 + 0.5) / height_f;
            let shaped = mix(v, v * v * 0.5 + v * 0.5, terms.perspective);
            let row = RowTerms {
                v_shaped: shaped,
                warp_x: 0.15 * (2.1 * v * terms.scale + terms.phases[3]).sin(),
                fade: mix(1.0 - 0.45 * terms.perspective, 1.0, v),
            };
            let stretch = 1.0 + terms.perspective * (1.0 - v) * 1.5;
            let matrix_row = &self.matrix[(y % side) * side..(y % side) * side + side];
            let dot_row = &mut frame.raster.dots[y * width..(y + 1) * width];
            let ring_dy = |ring: &Ring| (y as f32 + 0.5 - ring.center_y) * 2.0;
            for (x, dot) in dot_row.iter_mut().enumerate() {
                let xf = x as f32 + 0.5;
                let u = (xf - half_width) / height_f * stretch + 0.5;
                let mut light = Self::intensity(&terms, &row, u, xf, y as f32 + 0.5);
                if use_rings {
                    for ring in rings.iter().filter(|ring| ring.strength > 0.0) {
                        let dy = ring_dy(ring);
                        if dy.abs() > ring.radius + RING_HALF_WIDTH {
                            continue;
                        }
                        let dx = xf - ring.center_x;
                        let distance = (dx * dx + dy * dy).sqrt();
                        if (distance - ring.radius).abs() < RING_HALF_WIDTH {
                            light += ring.strength;
                        }
                    }
                }
                let lit = light + terms.bias > matrix_row[x % side];
                *dot = if lit { level } else { 0.0 };
                if tint {
                    let cell = (y / 4) * cell_columns + x / 2;
                    if let (Some(sum), Some(count)) =
                        (self.cell_sums.get_mut(cell), self.cell_counts.get_mut(cell))
                    {
                        *sum += light.clamp(0.0, 1.0);
                        *count += 1.0;
                    }
                }
            }
        }

        if tint {
            for (index, color) in frame.cell_colors.iter_mut().enumerate() {
                let count = self.cell_counts.get(index).copied().unwrap_or(0.0);
                let average = if count > 0.0 {
                    self.cell_sums[index] / count
                } else {
                    0.0
                };
                let amount = (average * 1.5).clamp(0.0, 1.0);
                if let Some(rgb) = self.palette.at(amount) {
                    for channel in 0..3 {
                        color[channel] = (f32::from(rgb[channel]) * color_gain)
                            .round()
                            .clamp(0.0, 255.0) as u8;
                    }
                    continue;
                }
                for channel in 0..3 {
                    let base = mix(DEEP_WATER[channel], PALE_WATER[channel], amount);
                    color[channel] = (base * color_gain * 255.0).round().clamp(0.0, 255.0) as u8;
                }
            }
        }
    }

    fn set_palette(&mut self, palette: &ScenePalette) {
        self.palette = palette.clone();
    }

    fn follows_palette(&self) -> bool {
        true
    }

    fn uses_cell_colors(&self) -> bool {
        self.settings.tint
    }

    fn frames_per_second(&self) -> u32 {
        12
    }
}

#[cfg(test)]
mod tests;

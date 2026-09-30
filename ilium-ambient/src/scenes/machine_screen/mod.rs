//! A single dim machine screen: a framed panel with two drifting sine waves
//! rendered through a Bayer dither, a scrolling marquee of bars along its
//! bottom edge and a few sparse blinking dots.
//!
//! Determinism: every frame is a pure function of the animation clock and the
//! seed; nothing accumulates between frames. The marquee pattern is built
//! once in `new`. Nothing here blocks or spawns threads.
//!
//! Inspired by the screens of nullMachines; no data source is used.

mod settings;

pub use settings::MachineScreenSettings;

use crate::control::SceneSettings;
use crate::scene::{Frame, Scene, SceneEnv};
use std::f32::consts::TAU;

pub const INSPIRED_BY: &[&str] = &["https://nullmachines.xyz/"];

/// Length of the repeating marquee pattern in dots.
const MARQUEE_PERIOD: usize = 192;
const MARQUEE_ROWS: usize = 3;
/// Blink dots that change every half second.
const BLINK_DOTS: u32 = 6;
const BLINKS_PER_SECOND: f64 = 2.0;
/// Bayer 4x4 thresholds, scaled by 16.
const BAYER: [[u8; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];

/// Integer hash (splitmix64 finalizer) mapping a key to 64 well-mixed bits.
fn mix(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9e37_79b9_7f4a_7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

fn unit_float(value: u64) -> f32 {
    (value >> 40) as f32 / (1u64 << 24) as f32
}

/// Screen rectangle and the sub-areas inside it, in raster dots.
struct Layout {
    panel: Rect,
    gradient: Rect,
    /// `None` when the panel is too small for a marquee band.
    marquee: Option<Rect>,
    has_frame: bool,
}

#[derive(Clone, Copy)]
struct Rect {
    x: usize,
    y: usize,
    width: usize,
    height: usize,
}

pub struct MachineScreenScene {
    settings: MachineScreenSettings,
    /// One marquee row per entry: bars of width 1..=3 separated by gaps.
    marquee: [[bool; MARQUEE_PERIOD]; MARQUEE_ROWS],
    /// Per-seed wave parameters: x frequency, y frequency, y wobble, x phase, y phase.
    wave: [f32; 5],
    wave_x: Vec<f32>,
    wave_y: Vec<f32>,
}

impl MachineScreenScene {
    pub fn new(settings: &MachineScreenSettings, _env: &SceneEnv) -> Self {
        let settings = settings.normalized();
        let seed = u64::from(settings.seed);
        let mut marquee = [[false; MARQUEE_PERIOD]; MARQUEE_ROWS];
        for (row_index, row) in marquee.iter_mut().enumerate() {
            let mut state = mix(seed ^ ((row_index as u64 + 1) << 32));
            let mut column = 0;
            let mut on = row_index % 2 == 0;
            while column < MARQUEE_PERIOD {
                state = mix(state);
                let run = 1 + (state % 3) as usize;
                for slot in row.iter_mut().skip(column).take(run) {
                    *slot = on;
                }
                column += run;
                on = !on;
            }
        }
        let random = |index: u64| unit_float(mix(seed.wrapping_mul(31).wrapping_add(index)));
        Self {
            settings,
            marquee,
            wave: [
                1.1 + 0.4 * random(1),
                0.7 + 0.3 * random(2),
                0.2 + 0.1 * random(3),
                random(4),
                random(5),
            ],
            wave_x: Vec::new(),
            wave_y: Vec::new(),
        }
    }

    fn layout(&self, width: usize, height: usize) -> Layout {
        let fraction = self.settings.panel_size as usize;
        let panel_width = (width * fraction / 100).clamp(1, width);
        let panel_height = (height * fraction / 100).clamp(1, height);
        let panel = Rect {
            x: (width - panel_width) / 2,
            y: (height - panel_height) / 2,
            width: panel_width,
            height: panel_height,
        };
        let has_frame = self.settings.show_frame && panel_width >= 7 && panel_height >= 7;
        let inset = if has_frame { 2 } else { 0 };
        let inner = Rect {
            x: panel.x + inset,
            y: panel.y + inset,
            width: panel_width - 2 * inset,
            height: panel_height - 2 * inset,
        };
        // One gap row separates gradient and marquee.
        let band = MARQUEE_ROWS + 1;
        let (gradient, marquee) = if inner.height >= band + 6 && inner.width >= 6 {
            (
                Rect {
                    height: inner.height - band,
                    ..inner
                },
                Some(Rect {
                    y: inner.y + inner.height - MARQUEE_ROWS,
                    height: MARQUEE_ROWS,
                    ..inner
                }),
            )
        } else {
            (inner, None)
        };
        Layout {
            panel,
            gradient,
            marquee,
            has_frame,
        }
    }

    fn draw_frame(&self, layout: &Layout, raster: &mut crate::raster::Raster, level: f32) {
        let panel = layout.panel;
        let right = panel.x + panel.width - 1;
        let bottom = panel.y + panel.height - 1;
        for x in panel.x..=right {
            raster.dots[panel.y * raster.width + x] = level;
            raster.dots[bottom * raster.width + x] = level;
        }
        for y in panel.y..=bottom {
            raster.dots[y * raster.width + panel.x] = level;
            raster.dots[y * raster.width + right] = level;
        }
    }

    fn draw_gradient(&mut self, area: Rect, time: f64, raster: &mut crate::raster::Raster) {
        let level = self.settings.brightness as f32 / 100.0;
        let scale = self.settings.dither_scale as usize;
        let phase = (time * f64::from(self.settings.gradient_speed) / 100.0) as f32;
        let [x_frequency, y_frequency, wobble, x_phase, y_phase] = self.wave;
        let inverse_width = 1.0 / area.width as f32;
        let inverse_height = 1.0 / area.height as f32;
        self.wave_x.clear();
        self.wave_x.extend((0..area.width).map(|column| {
            (TAU * (x_frequency * column as f32 * inverse_width + 0.2 * phase + x_phase)).sin()
        }));
        let wobble_term = wobble * (0.2 * phase * TAU).sin();
        self.wave_y.clear();
        self.wave_y.extend((0..area.height).map(|row| {
            (TAU * (y_frequency * row as f32 * inverse_height - 0.13 * phase
                + wobble_term
                + y_phase))
                .sin()
        }));
        for (row, wave_y) in self.wave_y.iter().enumerate() {
            let bayer_row = &BAYER[(row / scale) & 3];
            let line = (area.y + row) * raster.width + area.x;
            for (column, wave_x) in self.wave_x.iter().enumerate() {
                let value = 0.5 + 0.25 * wave_x + 0.25 * wave_y;
                let threshold = (f32::from(bayer_row[(column / scale) & 3]) + 0.5) / 16.0;
                if value > threshold {
                    raster.dots[line + column] = level;
                }
            }
        }
    }

    fn draw_marquee(&self, area: Rect, time: f64, raster: &mut crate::raster::Raster) {
        let level = (self.settings.brightness as f32 * 1.2 / 100.0).min(1.0);
        let offset = (time * f64::from(self.settings.marquee_speed)).floor() as usize;
        for (row, pattern) in self.marquee.iter().enumerate() {
            let line = (area.y + row) * raster.width + area.x;
            for column in 0..area.width {
                if pattern[(column + offset + row * 37) % MARQUEE_PERIOD] {
                    raster.dots[line + column] = level;
                }
            }
        }
    }

    fn draw_blinks(&self, area: Rect, time: f64, raster: &mut crate::raster::Raster) {
        let level = (self.settings.brightness as f32 / 100.0 + 0.2).min(1.0);
        let slot = (time * BLINKS_PER_SECOND).floor() as u64;
        for index in 0..BLINK_DOTS {
            let key = mix(u64::from(self.settings.seed) ^ (slot << 20) ^ (u64::from(index) << 8));
            let column = (key % area.width as u64) as usize;
            let row = ((key >> 24) % area.height as u64) as usize;
            raster.dots[(area.y + row) * raster.width + area.x + column] = level;
        }
    }
}

impl Scene for MachineScreenScene {
    fn render(&mut self, frame: &mut Frame<'_>) {
        let raster = &mut *frame.raster;
        raster.dots.fill(0.0);
        if raster.width == 0 || raster.height == 0 {
            return;
        }
        let time = frame.time.as_secs_f64();
        let layout = self.layout(raster.width, raster.height);
        if layout.has_frame {
            let level = (self.settings.brightness as f32 * 0.9 / 100.0).min(1.0);
            self.draw_frame(&layout, raster, level);
        }
        let gradient = layout.gradient;
        if gradient.width == 0 || gradient.height == 0 {
            return;
        }
        self.draw_gradient(gradient, time, raster);
        if let Some(band) = layout.marquee {
            self.draw_marquee(band, time, raster);
        }
        if self.settings.blinking_dots {
            self.draw_blinks(gradient, time, raster);
        }
    }

    fn frames_per_second(&self) -> u32 {
        12
    }
}

#[cfg(test)]
mod tests;

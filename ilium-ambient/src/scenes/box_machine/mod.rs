//! Box machine: a dim pixel-art factory floor where small boxes ride
//! endless rail loops, after nullMachines by Kerim Safa and loackme.
//!
//! The floor is a grid of square tiles, each holding up to three rails that
//! leave through three ports per edge. Tiles are chosen by seeded wave
//! function collapse (`wfc`) from a library built once per tile size
//! (`library`); matching ports and a closed border guarantee that every rail
//! belongs to a closed loop (`layout`). A few tiles are screens with a
//! breathing dithered gradient and a scrolling marquee.
//!
//! Determinism: the layout depends only on settings and raster size; box
//! positions, marquees and blinks are pure functions of `Frame::time`.
//! Nothing here blocks, spawns threads or reads the system clock.

mod layout;
mod library;
mod settings;
#[cfg(test)]
mod tests;
mod wfc;

pub use settings::BoxMachineSettings;

use crate::control::SceneSettings;
use crate::scene::{Frame, Scene, SceneEnv};
use layout::{Layout, Screen};
use library::Library;
use wfc::mix;

pub const INSPIRED_BY: &[&str] = &["https://nullmachines.xyz/"];

const FRAME_GRADIENT: f32 = 0.30;
const SCREEN_DOT_LEVEL: f32 = 0.45;
const MARQUEE_LEVEL: f32 = 0.55;
const BLINK_LEVEL: f32 = 0.70;
/// Marquee scroll speed relative to box speed.
const MARQUEE_SPEED_FACTOR: f64 = 0.66;
const BAYER_4: [[u8; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];

pub struct BoxMachineScene {
    settings: BoxMachineSettings,
    library: Library,
    layout: Option<Layout>,
}

impl BoxMachineScene {
    pub fn new(settings: &BoxMachineSettings, _env: &SceneEnv) -> Self {
        let settings = settings.normalized();
        Self {
            library: Library::new(settings.tile_size),
            settings,
            layout: None,
        }
    }
}

fn put(dots: &mut [f32], width: usize, height: usize, x: i32, y: i32, value: f32) {
    if x >= 0 && y >= 0 && (x as usize) < width && (y as usize) < height {
        dots[y as usize * width + x as usize] = value;
    }
}

fn draw_cubes(dots: &mut [f32], layout: &Layout, settings: &BoxMachineSettings, seconds: f64) {
    let size = settings.cube_size as i32;
    let outline = settings.cube_level as f32 / 100.0;
    let travelled = f64::from(settings.speed) * seconds;
    for cube_loop in &layout.loops {
        let gap = f64::from(cube_loop.length()) / cube_loop.cube_count as f64;
        for cube in 0..cube_loop.cube_count {
            let distance = cube_loop.phase + cube as f64 * gap + cube_loop.direction * travelled;
            let (x, y) = cube_loop.position(distance);
            let left = x.round() as i32 - size / 2;
            let top = y.round() as i32 - size / 2;
            for dy in 0..size {
                for dx in 0..size {
                    let edge = dx == 0 || dy == 0 || dx == size - 1 || dy == size - 1;
                    let value = if edge { outline } else { outline * 0.5 };
                    put(
                        dots,
                        layout.width,
                        layout.height,
                        left + dx,
                        top + dy,
                        value,
                    );
                }
            }
        }
    }
}

fn draw_screen(
    dots: &mut [f32],
    layout: &Layout,
    screen: &Screen,
    tile_size: i32,
    settings: &BoxMachineSettings,
    seconds: f64,
) {
    let (width, height) = (layout.width, layout.height);
    let (left, top) = screen.origin;
    let (frame_left, frame_top) = (left + 2, top + 2);
    let frame_right = left + tile_size - 3;
    let frame_bottom = top + tile_size - 3;
    for x in frame_left..=frame_right {
        put(dots, width, height, x, frame_top, FRAME_GRADIENT);
        put(dots, width, height, x, frame_bottom, FRAME_GRADIENT);
    }
    for y in frame_top..=frame_bottom {
        put(dots, width, height, frame_left, y, FRAME_GRADIENT);
        put(dots, width, height, frame_right, y, FRAME_GRADIENT);
    }
    let inner_width = tile_size - 6;
    let inner_height = tile_size - 6;
    let (inner_left, inner_top) = (left + 3, top + 3);
    let gradient_rows = inner_height - 3;
    let phase = -0.25 * seconds as f32;
    for y in 0..gradient_rows {
        for x in 0..inner_width {
            let wave = 0.5
                + 0.5
                    * (std::f32::consts::TAU
                        * (x as f32 / inner_width as f32 * 0.5
                            + y as f32 / inner_height as f32 * 0.3)
                        + phase)
                        .sin();
            let threshold = (f32::from(BAYER_4[(y & 3) as usize][(x & 3) as usize]) + 0.5) / 16.0;
            if wave > threshold {
                put(
                    dots,
                    width,
                    height,
                    inner_left + x,
                    inner_top + y,
                    SCREEN_DOT_LEVEL,
                );
            }
        }
    }
    let marquee_row = inner_top + inner_height - 1;
    let scroll = (seconds * f64::from(settings.speed) * MARQUEE_SPEED_FACTOR).floor() as i64;
    let length = screen.marquee.len() as i64;
    for x in 0..inner_width {
        if screen.marquee[(i64::from(x) + scroll).rem_euclid(length) as usize] {
            put(
                dots,
                width,
                height,
                inner_left + x,
                marquee_row,
                MARQUEE_LEVEL,
            );
        }
    }
    let step = (seconds * 2.0).floor() as u64;
    for blink in 0..2u64 {
        let hash = mix(screen.id ^ step.wrapping_mul(0x9e37_79b9) ^ (blink << 60));
        let x = (hash % inner_width as u64) as i32;
        let y = ((hash >> 20) % gradient_rows as u64) as i32;
        put(
            dots,
            width,
            height,
            inner_left + x,
            inner_top + y,
            BLINK_LEVEL,
        );
    }
}

impl Scene for BoxMachineScene {
    fn render(&mut self, frame: &mut Frame<'_>) {
        let (width, height) = (frame.raster.width, frame.raster.height);
        if width == 0 || height == 0 {
            return;
        }
        let stale = self
            .layout
            .as_ref()
            .is_none_or(|layout| layout.width != width || layout.height != height);
        if stale {
            self.layout = Some(Layout::build(&self.library, &self.settings, width, height));
        }
        let Some(layout) = self.layout.as_ref() else {
            return;
        };
        let seconds = frame.time.as_secs_f64();
        let dots = &mut frame.raster.dots;
        dots.copy_from_slice(&layout.rails);
        for screen in &layout.screens {
            draw_screen(
                dots,
                layout,
                screen,
                self.library.tile_size,
                &self.settings,
                seconds,
            );
        }
        draw_cubes(dots, layout, &self.settings, seconds);
    }

    fn frames_per_second(&self) -> u32 {
        10
    }
}

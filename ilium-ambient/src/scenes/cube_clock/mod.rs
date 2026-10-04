//! A slowly turning dotted wireframe cube with a quiet digital clock.
//!
//! Everything is analytic in the animation clock (`Frame::time`) and the
//! civil time (`Frame::now`), so the picture is a pure function of the frame
//! and the scene keeps no per-frame state. The wall clock is not scaled by
//! the global Speed setting (otherwise the clock would lie); only the cube's
//! turning follows `Frame::time`.
//!
//! Inspired by the "Cube" piece of the Playdate app Arty Clocks; this is an
//! original design (the source page was not readable), CPU-only like the
//! original.

mod font;
mod settings;

pub use settings::CubeClockSettings;

use settings::{ClockPosition, HourFormat};

use crate::control::SceneSettings;
use crate::raster::{smoothstep, Raster};
use crate::scene::{Frame, Scene, SceneEnv};
use std::f64::consts::TAU as TAU_F64;
use std::time::UNIX_EPOCH;

pub const INSPIRED_BY: &[&str] = &["https://anefiox.itch.io/arty-clocks"];

const TAU: f32 = std::f32::consts::TAU;
/// Camera distance from the cube centre, in cube half-edges.
const CAMERA_DISTANCE: f32 = 6.0;
/// Half the cube's space diagonal.
const CUBE_RADIUS: f32 = 1.732;
/// Dots between two dashes along an edge.
const DOT_PITCH: f32 = 2.5;
const EDGE_GAIN: f32 = 0.6;
const FACE_DOT_INTENSITY: f32 = 0.22;
const FACE_GAIN_AT_FULL: f32 = 0.24;
const RING_GAIN: f32 = 0.35;
const DIGIT_INTENSITY: f32 = 0.45;
const DUST_COUNT: usize = 24;
/// Fixed seed: phases and dust are the same on every run, so the picture is
/// reproducible without exposing a seed control.
const SEED: u64 = 0x00C1_0C4C_0BE5;
const LIGHT: [f32; 3] = [-0.371, 0.557, 0.743];
const BAYER: [[u8; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];

type Vec3 = [f32; 3];

/// Cube vertices: bit 0 is x, bit 1 is y, bit 2 is z (0 = -1, 1 = +1).
fn vertex(index: usize) -> Vec3 {
    let sign = |bit: usize| if index >> bit & 1 == 1 { 1.0 } else { -1.0 };
    [sign(0), sign(1), sign(2)]
}

/// The 12 edges as vertex pairs differing in one bit.
fn edges() -> [(usize, usize); 12] {
    let mut result = [(0, 0); 12];
    let mut count = 0;
    for from in 0..8usize {
        for bit in 0..3 {
            let to = from ^ (1 << bit);
            if from < to {
                result[count] = (from, to);
                count += 1;
            }
        }
    }
    result
}

/// The 6 faces: the outward normal and its four corners in cyclic order.
fn faces() -> [(Vec3, [usize; 4]); 6] {
    let mut result = [([0.0; 3], [0; 4]); 6];
    let mut count = 0;
    for axis in 0..3usize {
        let first = (axis + 1) % 3;
        let second = (axis + 2) % 3;
        for side in 0..2usize {
            let base = side << axis;
            let corner = |a: usize, b: usize| base | (a << first) | (b << second);
            let mut normal = [0.0; 3];
            normal[axis] = if side == 1 { 1.0 } else { -1.0 };
            result[count] = (
                normal,
                [corner(0, 0), corner(1, 0), corner(1, 1), corner(0, 1)],
            );
            count += 1;
        }
    }
    result
}

fn hash(seed: u64, key: u32) -> u32 {
    let mut value = seed.wrapping_add(u64::from(key).wrapping_mul(0x9E37_79B9_7F4A_7C15));
    value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    (value ^ (value >> 31)) as u32
}

/// Civil time derived from `Frame::now`.
#[derive(Debug, Clone, Copy, PartialEq)]
struct ClockTime {
    hour: u32,
    minute: u32,
    second: u32,
    /// Fraction of the current second, 0.0..1.0.
    fraction: f32,
}

impl ClockTime {
    fn from_unix(unix_seconds: f64, offset_hours: i32) -> Self {
        let shifted = unix_seconds + f64::from(offset_hours) * 3600.0;
        let day_seconds = shifted.rem_euclid(86_400.0);
        let whole = day_seconds.floor();
        Self {
            hour: (whole / 3600.0) as u32 % 24,
            minute: (whole / 60.0) as u32 % 60,
            second: whole as u32 % 60,
            fraction: (day_seconds - whole) as f32,
        }
    }

    fn seconds_of_day(&self) -> u32 {
        self.hour * 3600 + self.minute * 60 + self.second
    }
}

pub struct CubeClockScene {
    settings: CubeClockSettings,
    /// Hours from UTC estimated from the shared location.
    location_offset_hours: i32,
    phases: [f64; 3],
    /// Dust positions as raw hashes, reduced to the raster size at draw time.
    dust: [(u32, u32); DUST_COUNT],
    edges: [(usize, usize); 12],
    faces: [(Vec3, [usize; 4]); 6],
}

impl CubeClockScene {
    // PALETTE (future plugin contract): `env.palette` is the shared look's current
    // palette. When animations become plugins, the plugin constructor receives the
    // current palette and MUST follow it: scenes with natural colours shift them
    // onto it (`ScenePalette::recolor`/`at`), and `Scene::set_palette` delivers later
    // changes. Monochrome scenes may ignore it. Today `PaletteScene` (scene.rs),
    // which `create_scene` wraps around every scene, shifts this scene's cell
    // colours onto the palette by brightness.
    pub fn new(settings: &CubeClockSettings, env: &SceneEnv) -> Self {
        let phase = |key: u32| f64::from(hash(SEED, key)) / f64::from(u32::MAX) * TAU_F64;
        let mut dust = [(0, 0); DUST_COUNT];
        for (index, slot) in dust.iter_mut().enumerate() {
            let key = index as u32;
            *slot = (hash(SEED, 100 + key), hash(SEED, 200 + key));
        }
        Self {
            settings: settings.normalized(),
            location_offset_hours: (env.location.longitude / 15.0).round() as i32,
            phases: [phase(0), phase(1), phase(2)],
            dust,
            edges: edges(),
            faces: faces(),
        }
    }

    fn clock_time(&self, frame: &Frame<'_>) -> ClockTime {
        let unix = frame
            .now
            .duration_since(UNIX_EPOCH)
            .map_or(0.0, |since| since.as_secs_f64());
        let offset = if self.settings.time_zone_from_location {
            self.location_offset_hours
        } else {
            self.settings.utc_offset_hours
        };
        ClockTime::from_unix(unix, offset)
    }

    /// Yaw, pitch and roll in radians.
    fn angles(&self, seconds: f64, clock: &ClockTime) -> (f32, f32, f32) {
        let rate = f64::from(self.settings.rotation_rate) / 100.0;
        let tick = if self.settings.second_tick {
            // 120 ticks make a full turn and divide a day exactly, so the
            // angle is continuous across midnight.
            let whole = f64::from(clock.seconds_of_day() % 120);
            (whole + f64::from(smoothstep(0.0, 0.25, clock.fraction))) * TAU_F64 / 120.0
        } else {
            0.0
        };
        let yaw = 0.35 * rate * seconds + tick + self.phases[0];
        let pitch = 0.55 + 0.40 * (0.13 * rate * seconds + self.phases[1]).sin();
        let roll = 0.11 * rate * seconds + 0.20 * (0.07 * rate * seconds + self.phases[2]).sin();
        (
            yaw.rem_euclid(TAU_F64) as f32,
            pitch as f32,
            roll.rem_euclid(TAU_F64) as f32,
        )
    }
}

fn rotate(point: Vec3, (yaw, pitch, roll): (f32, f32, f32)) -> Vec3 {
    let (sin_yaw, cos_yaw) = yaw.sin_cos();
    let (sin_pitch, cos_pitch) = pitch.sin_cos();
    let (sin_roll, cos_roll) = roll.sin_cos();
    let x1 = point[0] * cos_yaw + point[2] * sin_yaw;
    let z1 = -point[0] * sin_yaw + point[2] * cos_yaw;
    let y2 = point[1] * cos_pitch - z1 * sin_pitch;
    let z2 = point[1] * sin_pitch + z1 * cos_pitch;
    [
        x1 * cos_roll - y2 * sin_roll,
        x1 * sin_roll + y2 * cos_roll,
        z2,
    ]
}

/// Set a dot to the brighter of its current and the new value.
fn plot(raster: &mut Raster, x: f32, y: f32, intensity: f32) {
    let column = (x + 0.5).floor();
    let row = (y + 0.5).floor();
    if column < 0.0 || row < 0.0 {
        return;
    }
    let (column, row) = (column as usize, row as usize);
    if column >= raster.width || row >= raster.height {
        return;
    }
    let dot = &mut raster.dots[row * raster.width + column];
    *dot = dot.max(intensity.clamp(0.0, 1.0));
}

fn plot_block(raster: &mut Raster, left: i32, top: i32, size: i32, intensity: f32) {
    for dy in 0..size {
        for dx in 0..size {
            plot(raster, (left + dx) as f32, (top + dy) as f32, intensity);
        }
    }
}

/// Everything one frame needs, computed once per render.
struct View {
    center: (f32, f32),
    scale: f32,
    gain: f32,
    seconds: f32,
    /// Rotation speed factor; also stops the dashes crawling when frozen.
    rate: f32,
    dot_size: i32,
}

impl CubeClockScene {
    fn project(&self, view: &View, point: Vec3) -> (f32, f32, f32) {
        let perspective = CAMERA_DISTANCE / (CAMERA_DISTANCE - point[2]);
        (
            view.center.0 + point[0] * view.scale * perspective,
            view.center.1 - point[1] * view.scale * perspective,
            ((point[2] + CUBE_RADIUS) / (2.0 * CUBE_RADIUS)).clamp(0.0, 1.0),
        )
    }

    fn draw_edges(&self, raster: &mut Raster, view: &View, corners: &[(f32, f32, f32); 8]) {
        for (index, (from, to)) in self.edges.iter().enumerate() {
            let (ax, ay, az) = corners[*from];
            let (bx, by, bz) = corners[*to];
            let length = (bx - ax).hypot(by - ay);
            let samples = length.ceil().max(1.0) as usize;
            let direction = if index % 2 == 0 { 1.0 } else { -1.0 };
            let marching = 0.15 * view.rate * view.seconds * direction;
            for sample in 0..=samples {
                let along = sample as f32 / samples as f32;
                let phase = along * length / DOT_PITCH - marching;
                if phase - phase.floor() >= 0.7 {
                    continue;
                }
                let depth = az + (bz - az) * along;
                let intensity = EDGE_GAIN * (0.25 + 0.75 * depth) * view.gain;
                plot(
                    raster,
                    ax + (bx - ax) * along,
                    ay + (by - ay) * along,
                    intensity,
                );
            }
        }
    }

    fn draw_faces(
        &self,
        raster: &mut Raster,
        view: &View,
        angles: (f32, f32, f32),
        corners: &[(f32, f32, f32); 8],
    ) {
        let face_gain = FACE_GAIN_AT_FULL * self.settings.face_shading as f32 / 100.0;
        if face_gain <= 0.0 {
            return;
        }
        for (normal, quad) in &self.faces {
            let rotated = rotate(*normal, angles);
            if rotated[2] <= 0.0 {
                continue;
            }
            let lambert =
                (rotated[0] * LIGHT[0] + rotated[1] * LIGHT[1] + rotated[2] * LIGHT[2]).max(0.0);
            let brightness = face_gain * (0.35 + 0.65 * lambert);
            let points = quad.map(|corner| (corners[corner].0, corners[corner].1));
            fill_quad(raster, &points, |x, y| {
                let threshold = f32::from(BAYER[y & 3][x & 3]) / 16.0 + 1.0 / 32.0;
                if brightness > threshold {
                    FACE_DOT_INTENSITY * view.gain
                } else {
                    0.0
                }
            });
        }
    }

    fn draw_ring(&self, raster: &mut Raster, view: &View, clock: &ClockTime) {
        let radius = view.scale * 3.0;
        const TRAIL: [f32; 5] = [0.5, 0.35, 0.25, 0.18, 0.12];
        for index in 0..60u32 {
            let angle = index as f32 * TAU / 60.0 - TAU / 4.0;
            let x = view.center.0 + radius * angle.cos();
            let y = view.center.1 + 0.45 * radius * angle.sin();
            let behind = (clock.second + 60 - index) % 60;
            let mut intensity = if index % 5 == 0 { 0.22 } else { 0.12 } * RING_GAIN / 0.35;
            if let Some(trail) = TRAIL.get(behind as usize) {
                intensity = intensity.max(*trail);
            }
            let intensity = intensity * view.gain;
            if index % 5 == 0 {
                let size = if view.dot_size >= 3 { 2 } else { 1 };
                plot_block(raster, x.round() as i32, y.round() as i32, size, intensity);
            } else {
                plot(raster, x, y, intensity);
            }
        }
    }

    fn draw_dust(&self, raster: &mut Raster, view: &View) {
        let (width, height) = (raster.width as u32, raster.height as u32);
        let clear_radius = view.scale * 2.6;
        for (index, (hash_x, hash_y)) in self.dust.iter().enumerate() {
            let x = (hash_x % width) as f32;
            let y = (hash_y % height) as f32;
            if (x - view.center.0).hypot(y - view.center.1) < clear_radius {
                continue;
            }
            let twinkle = 0.10 + 0.08 * (0.6 * view.seconds + index as f32 * 1.7).sin();
            plot(raster, x, y, twinkle * view.gain);
        }
    }

    /// Digits as glyph indices, with the AM/PM dot count for 12 hour mode.
    fn readout(&self, clock: &ClockTime) -> (Vec<usize>, u32) {
        let mut glyphs = Vec::with_capacity(8);
        let (hour, marker) = match self.settings.hour_format {
            HourFormat::TwentyFour => (clock.hour, 0),
            HourFormat::Twelve => (
                if clock.hour.is_multiple_of(12) {
                    12
                } else {
                    clock.hour % 12
                },
                if clock.hour < 12 { 1 } else { 2 },
            ),
        };
        if hour >= 10 || self.settings.hour_format == HourFormat::TwentyFour {
            glyphs.push((hour / 10) as usize);
        }
        glyphs.push((hour % 10) as usize);
        glyphs.push(font::COLON);
        glyphs.push((clock.minute / 10) as usize);
        glyphs.push((clock.minute % 10) as usize);
        if self.settings.show_seconds {
            glyphs.push(font::COLON);
            glyphs.push((clock.second / 10) as usize);
            glyphs.push((clock.second % 10) as usize);
        }
        (glyphs, marker)
    }

    fn draw_readout(&self, raster: &mut Raster, view: &View, clock: &ClockTime) {
        let block = view.dot_size;
        let (glyphs, marker) = self.readout(clock);
        let advance = |glyph: usize| (font::glyph_width(glyph) as i32 + 1) * block;
        let total: i32 = glyphs.iter().map(|glyph| advance(*glyph)).sum::<i32>() - block;
        let inside = self.settings.clock_position == ClockPosition::Inside;
        let left = (view.center.0 - total as f32 / 2.0).round() as i32;
        let top = if inside {
            (view.center.1 - 2.5 * block as f32).round() as i32
        } else {
            let below = view.center.1 + view.scale * 2.6 + 2.0 * block as f32;
            below
                .min((raster.height as i32 - 5 * block - 2) as f32)
                .round() as i32
        };
        let base = if inside { 0.25 } else { DIGIT_INTENSITY };
        if inside {
            // Keep the faces from fighting the digits.
            for dy in -block..6 * block {
                for dx in -block..total + block {
                    clear_dot(raster, left + dx, top + dy);
                }
            }
        }
        let mut cursor = left;
        let mut last_digit_right = left;
        for glyph in &glyphs {
            let intensity = if *glyph == font::COLON {
                if clock.fraction < 0.5 {
                    base
                } else {
                    base / 3.0
                }
            } else {
                base
            } * view.gain;
            for row in 0..5 {
                for column in 0..font::glyph_width(*glyph) {
                    if font::pixel(*glyph, column, row) {
                        let x = cursor + column as i32 * block;
                        plot_block(raster, x, top + row as i32 * block, block, intensity);
                    }
                }
            }
            if *glyph != font::COLON {
                last_digit_right = cursor + 3 * block;
            }
            cursor += advance(*glyph);
        }
        let marker_size = ((block + 1) / 2).max(1);
        for dot in 0..marker as i32 {
            let y = top - 2 * marker_size - dot * 2 * marker_size;
            plot_block(
                raster,
                last_digit_right - marker_size,
                y,
                marker_size,
                base * view.gain,
            );
        }
    }
}

fn clear_dot(raster: &mut Raster, x: i32, y: i32) {
    if x < 0 || y < 0 {
        return;
    }
    let (x, y) = (x as usize, y as usize);
    if x < raster.width && y < raster.height {
        raster.dots[y * raster.width + x] = 0.0;
    }
}

/// Fill a convex quad (either winding), asking `value` for each covered dot.
fn fill_quad(raster: &mut Raster, points: &[(f32, f32); 4], value: impl Fn(usize, usize) -> f32) {
    let min_x = points
        .iter()
        .map(|p| p.0)
        .fold(f32::MAX, f32::min)
        .floor()
        .max(0.0);
    let max_x = points.iter().map(|p| p.0).fold(f32::MIN, f32::max).ceil();
    let min_y = points
        .iter()
        .map(|p| p.1)
        .fold(f32::MAX, f32::min)
        .floor()
        .max(0.0);
    let max_y = points.iter().map(|p| p.1).fold(f32::MIN, f32::max).ceil();
    let right = (max_x.max(0.0) as usize).min(raster.width);
    let bottom = (max_y.max(0.0) as usize).min(raster.height);
    for y in min_y as usize..bottom {
        for x in min_x as usize..right {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let mut positive = false;
            let mut negative = false;
            for index in 0..4 {
                let (ax, ay) = points[index];
                let (bx, by) = points[(index + 1) % 4];
                let cross = (bx - ax) * (py - ay) - (by - ay) * (px - ax);
                positive |= cross > 0.0;
                negative |= cross < 0.0;
            }
            if positive && negative {
                continue;
            }
            let intensity = value(x, y);
            if intensity > 0.0 {
                let dot = &mut raster.dots[y * raster.width + x];
                *dot = dot.max(intensity.clamp(0.0, 1.0));
            }
        }
    }
}

impl Scene for CubeClockScene {
    fn render(&mut self, frame: &mut Frame<'_>) {
        frame.raster.dots.fill(0.0);
        let (width, height) = (frame.raster.width, frame.raster.height);
        if width == 0 || height == 0 {
            return;
        }
        let clock = self.clock_time(frame);
        let seconds = frame.time.as_secs_f64();
        let angles = self.angles(seconds, &clock);
        let smaller_side = width.min(height) as f32;
        let view = View {
            center: (width as f32 * 0.5, height as f32 * 0.46),
            scale: smaller_side * self.settings.cube_size as f32 / 100.0 * 0.5 / 1.6,
            gain: self.settings.brightness as f32 / 100.0,
            seconds: (seconds % 100_000.0) as f32,
            rate: self.settings.rotation_rate as f32 / 100.0,
            dot_size: (height / 50).max(2) as i32,
        };
        let corners: [(f32, f32, f32); 8] =
            std::array::from_fn(|index| self.project(&view, rotate(vertex(index), angles)));

        let raster = &mut *frame.raster;
        if self.settings.dust {
            self.draw_dust(raster, &view);
        }
        if self.settings.minute_ring {
            self.draw_ring(raster, &view, &clock);
        }
        self.draw_faces(raster, &view, angles, &corners);
        self.draw_edges(raster, &view, &corners);
        if self.settings.clock_position != ClockPosition::Off {
            self.draw_readout(raster, &view, &clock);
        }
    }

    fn frames_per_second(&self) -> u32 {
        15
    }

    fn status(&self) -> Option<String> {
        None
    }
}

#[cfg(test)]
mod tests;

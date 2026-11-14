//! The scalar fields (`0.0..1.0`) that are dithered into dots.
//!
//! All motion is built from `tau` (one full turn per loop) times integers,
//! or from scrolls by whole lattice periods, so every pattern repeats
//! exactly after one loop.

use super::noise::{hash_u32, hash_unit, LatticeNoise};
use super::settings::Pattern;
use std::f32::consts::{PI, TAU};

/// Per-frame constants shared by every pattern.
pub struct FieldContext<'a> {
    pub width: usize,
    pub height: usize,
    /// Dot coordinates are centred and divided by the shorter side, so the
    /// shorter axis spans `-0.5..0.5` and dots stay square.
    pub side: f32,
    pub tau: f32,
    pub loop_fraction: f32,
    /// Spatial frequency `scale / 25`.
    pub frequency: f32,
    pub complexity: u32,
    pub seed: u32,
    pub noise: &'a LatticeNoise,
}

impl FieldContext<'_> {
    fn position(&self, x: usize, y: usize) -> (f32, f32) {
        (
            (x as f32 + 0.5 - self.width as f32 * 0.5) / self.side,
            (y as f32 + 0.5 - self.height as f32 * 0.5) / self.side,
        )
    }

    fn fill(&self, out: &mut [f32], mut sample: impl FnMut(f32, f32) -> f32) {
        for (y, row) in out.chunks_exact_mut(self.width).enumerate() {
            for (x, cell) in row.iter_mut().enumerate() {
                let (px, py) = self.position(x, y);
                *cell = sample(px, py).clamp(0.0, 1.0);
            }
        }
    }
}

/// A star with a fixed direction, loop phase and depth layer.
#[derive(Clone, Copy)]
pub struct Star {
    direction: (f32, f32),
    phase: f32,
}

/// Star list for a seed and complexity; `20 + 12 * complexity` stars.
pub fn make_stars(seed: u32, complexity: u32) -> Vec<Star> {
    (0..20 + 12 * complexity)
        .map(|index| {
            let angle = hash_unit(index as i32, 1, seed) * TAU;
            Star {
                direction: (angle.cos(), angle.sin()),
                phase: hash_unit(index as i32, 2, seed),
            }
        })
        .collect()
}

/// Reusable per-frame scratch memory.
#[derive(Default)]
pub struct PatternScratch {
    feature_points: Vec<[f32; 2]>,
}

pub fn render_pattern(
    pattern: Pattern,
    context: &FieldContext<'_>,
    stars: &[Star],
    scratch: &mut PatternScratch,
    out: &mut [f32],
) {
    match pattern {
        Pattern::Caustics => caustics(context, out),
        Pattern::Cellular => cellular(context, scratch, out),
        Pattern::Halftone => halftone(context, out),
        Pattern::Starfield => starfield(context, stars, out),
        Pattern::Moire => moire(context, out),
        Pattern::Smoke => smoke(context, out),
        Pattern::Spiral => spiral(context, out),
        Pattern::Tunnel => tunnel(context, out),
        Pattern::Plasma => plasma(context, out),
        Pattern::Ripples => ripples(context, out),
    }
}

fn smooth(edge_0: f32, edge_1: f32, value: f32) -> f32 {
    let fraction = ((value - edge_0) / (edge_1 - edge_0)).clamp(0.0, 1.0);
    fraction * fraction * (3.0 - 2.0 * fraction)
}

fn caustics(context: &FieldContext<'_>, out: &mut [f32]) {
    let noise = context.noise;
    let (cos_tau, sin_tau) = (context.tau.cos(), context.tau.sin());
    let warp_octaves = 2;
    let ridge_octaves = context.complexity.min(4);
    let zoom = context.frequency * 2.0;
    context.fill(out, |px, py| {
        let (qx, qy) = (px * zoom, py * zoom);
        let warp_x = noise.fbm(qx + cos_tau * 0.6, qy + sin_tau * 0.6, warp_octaves);
        let warp_y = noise.fbm(
            qx + 5.2 + sin_tau * 0.6,
            qy + 5.2 + cos_tau * 0.6,
            warp_octaves,
        );
        let warped = noise.fbm(
            qx + 2.0 * (warp_x - 0.5),
            qy + 2.0 * (warp_y - 0.5),
            ridge_octaves,
        );
        // Value noise clusters around 0.5, so stretch the ridge distance.
        let ridge = 1.0 - ((warped - 0.5).abs() * 6.0).min(1.0);
        ridge * ridge * ridge
    });
}

fn cellular(context: &FieldContext<'_>, scratch: &mut PatternScratch, out: &mut [f32]) {
    let cells_per_unit = context.frequency * 1.5;
    let half_x = context.width as f32 * 0.5 / context.side * cells_per_unit;
    let half_y = context.height as f32 * 0.5 / context.side * cells_per_unit;
    let first_x = (-half_x).floor() as i32 - 1;
    let first_y = (-half_y).floor() as i32 - 1;
    let count_x = (2.0 * half_x).ceil() as usize + 4;
    let count_y = (2.0 * half_y).ceil() as usize + 4;
    let points = &mut scratch.feature_points;
    points.clear();
    for row in 0..count_y {
        for column in 0..count_x {
            let (cell_x, cell_y) = (first_x + column as i32, first_y + row as i32);
            let turns =
                1.0 + (hash_u32(cell_x as u32, cell_y as u32, context.seed ^ 0x55) & 1) as f32;
            let angle_x = TAU * hash_unit(cell_x, cell_y, context.seed) + context.tau * turns;
            let angle_y = TAU * hash_unit(cell_x, cell_y, context.seed + 1) + context.tau * turns;
            points.push([
                cell_x as f32 + 0.5 + 0.4 * angle_x.cos(),
                cell_y as f32 + 0.5 + 0.4 * angle_y.sin(),
            ]);
        }
    }
    let membranes = context.complexity > 4;
    let points = &scratch.feature_points;
    context.fill(out, |px, py| {
        let (gx, gy) = (px * cells_per_unit, py * cells_per_unit);
        let (cell_x, cell_y) = (gx.floor() as i32, gy.floor() as i32);
        let (mut nearest, mut second) = (f32::MAX, f32::MAX);
        for dy in -1..=1 {
            for dx in -1..=1 {
                let column = (cell_x + dx - first_x) as usize;
                let row = (cell_y + dy - first_y) as usize;
                let Some(point) = points.get(row * count_x + column) else {
                    continue;
                };
                let distance = (point[0] - gx).powi(2) + (point[1] - gy).powi(2);
                if distance < nearest {
                    second = nearest;
                    nearest = distance;
                } else if distance < second {
                    second = distance;
                }
            }
        }
        let (nearest, second) = (nearest.sqrt(), second.sqrt());
        let blob = smooth(0.55, 0.15, nearest);
        if membranes {
            blob * 0.6 + (1.0 - smooth(0.0, 0.12, second - nearest)) * 0.4
        } else {
            blob
        }
    });
}

fn halftone(context: &FieldContext<'_>, out: &mut [f32]) {
    let noise = context.noise;
    let cells_per_unit = context.frequency * 12.0;
    let (sin_a, cos_a) = (PI / 4.0).sin_cos();
    let (cos_tau, sin_tau) = (context.tau.cos(), context.tau.sin());
    context.fill(out, |px, py| {
        let gx = (px * cos_a - py * sin_a) * cells_per_unit;
        let gy = (px * sin_a + py * cos_a) * cells_per_unit;
        let (center_x, center_y) = (gx.floor() + 0.5, gy.floor() + 0.5);
        let distance = ((gx - center_x).powi(2) + (gy - center_y).powi(2)).sqrt();
        let field_scale = 2.5 / cells_per_unit;
        let size = noise.fbm(
            center_x * field_scale + cos_tau * 0.5,
            center_y * field_scale + sin_tau * 0.5,
            2,
        );
        let radius = 0.5 * ((size - 0.25) * 2.2).clamp(0.0, 1.0);
        smooth(radius + 0.1, radius - 0.1, distance)
    });
}

fn starfield(context: &FieldContext<'_>, stars: &[Star], out: &mut [f32]) {
    out.fill(0.0);
    let (half_w, half_h) = (context.width as f32 * 0.5, context.height as f32 * 0.5);
    for star in stars {
        let depth = (star.phase + context.loop_fraction).fract();
        let outer = depth * depth * 0.75;
        let inner = (outer - 0.08 * depth).max(0.0);
        let brightness = depth * depth.sqrt();
        let steps = (((outer - inner) * context.side * 2.0).ceil() as usize).max(1);
        for step in 0..=steps {
            let radius = inner + (outer - inner) * step as f32 / steps as f32;
            let x = (half_w + star.direction.0 * radius * context.side).floor();
            let y = (half_h + star.direction.1 * radius * context.side).floor();
            if x < 0.0 || y < 0.0 || x >= context.width as f32 || y >= context.height as f32 {
                continue;
            }
            let cell = &mut out[y as usize * context.width + x as usize];
            *cell = cell.max(brightness);
        }
    }
}

fn moire(context: &FieldContext<'_>, out: &mut [f32]) {
    let rings = context.frequency * 4.0;
    let orbit = |sign: f32, offset: f32| {
        (
            sign * 0.15 * (sign * context.tau + offset).cos(),
            sign * 0.15 * (sign * context.tau + offset).sin(),
        )
    };
    let centers = [
        orbit(1.0, 0.0),
        orbit(-1.0, 0.0),
        (
            0.15 * (-context.tau + 2.0).cos(),
            0.15 * (-context.tau + 2.0).sin(),
        ),
    ];
    let three = context.complexity >= 5;
    let grating = |px: f32, py: f32, center: (f32, f32)| {
        let distance = ((px - center.0).powi(2) + (py - center.1).powi(2)).sqrt();
        0.5 + 0.5 * (TAU * rings * distance).cos()
    };
    let agree = |a: f32, b: f32| a * b + (1.0 - a) * (1.0 - b);
    context.fill(out, |px, py| {
        let first = grating(px, py, centers[0]);
        let second = grating(px, py, centers[1]);
        let agreement = if three {
            let third = grating(px, py, centers[2]);
            (agree(first, second) + agree(second, third) + agree(third, first)) / 3.0
        } else {
            agree(first, second)
        };
        // Sharpen so interference fringes stay sparse rather than half-filled.
        agreement * agreement
    });
}

fn smoke(context: &FieldContext<'_>, out: &mut [f32]) {
    const PERIOD_Y: i32 = 8;
    let noise = context.noise;
    let zoom = context.frequency * 2.0;
    let octaves = context.complexity.min(5);
    let scroll = context.loop_fraction * PERIOD_Y as f32;
    let tau = context.tau;
    context.fill(out, |px, py| {
        let wobbled = px + 0.15 * (TAU * (py * 1.5 - context.loop_fraction)).sin();
        let (qx, qy) = (wobbled * zoom, py * zoom);
        let turbulence = noise.fbm_wrapped_y(
            qx + 0.4 * (TAU * qy * 0.25 + tau).sin(),
            qy + scroll,
            octaves,
            PERIOD_Y,
        );
        let plume = (-(px * px) / (0.05 + 0.25 * (0.5 - py))).exp();
        let top_fade = 1.0 - smooth(0.0, 1.0, 0.5 - py);
        ((turbulence * 2.2 - 0.85).clamp(0.0, 1.0)) * plume * top_fade * 1.4
    });
}

fn spiral(context: &FieldContext<'_>, out: &mut [f32]) {
    let arms = context.complexity.min(8) as f32;
    let pitch = context.frequency * 25.0 / 27.0;
    context.fill(out, |px, py| {
        let radius = (px * px + py * py).sqrt() + 1e-4;
        let angle = py.atan2(px);
        let phase = arms * angle - pitch * radius.ln() - context.tau;
        let arm = (0.5 + 0.5 * phase.cos()).powf(1.5);
        arm * smooth(0.02, 0.12, radius) * (1.0 - smooth(0.42, 0.7, radius))
    });
}

fn tunnel(context: &FieldContext<'_>, out: &mut [f32]) {
    const DEPTH_SPEED: f32 = 4.0;
    let segments = (2 * context.complexity) as f32;
    context.fill(out, |px, py| {
        let radius = (px * px + py * py).sqrt() + 1e-3;
        let depth = 0.25 / radius;
        let around = py.atan2(px) / TAU * segments;
        let along = depth * context.frequency * 0.5 - context.loop_fraction * DEPTH_SPEED;
        let tiles = 0.5 + 0.5 * (TAU * around).sin() * (TAU * along).sin();
        let fog = (-0.45 * depth).exp();
        tiles * fog * smooth(0.02, 0.12, radius)
    });
}

fn plasma(context: &FieldContext<'_>, out: &mut [f32]) {
    let f = context.frequency;
    let tau = context.tau;
    let extra = context.complexity >= 5;
    let (sin_a, cos_a) = 0.3f32.sin_cos();
    context.fill(out, |px, py| {
        let radius = (px * px + py * py).sqrt();
        let mut sum = (f * 6.0 * px + tau).sin()
            + (f * 6.0 * (py * cos_a + px * sin_a) - tau).sin()
            + (f * 4.0 * radius + 2.0 * tau).sin();
        if extra {
            sum += (f * 5.0 * (px * cos_a - py * sin_a) + f * 3.0 * py - tau).sin();
            return smooth(0.45, 1.0, 0.5 + sum / 8.0);
        }
        smooth(0.45, 1.0, 0.5 + sum / 6.0)
    });
}

fn ripples(context: &FieldContext<'_>, out: &mut [f32]) {
    let drops = 1 + context.complexity as usize / 2;
    let centers: Vec<(f32, f32, f32)> = (0..drops)
        .map(|index| {
            (
                (hash_unit(index as i32, 10, context.seed) - 0.5) * 0.6,
                (hash_unit(index as i32, 11, context.seed) - 0.5) * 0.6,
                if index % 3 == 2 { 2.0 } else { 1.0 },
            )
        })
        .collect();
    let rings = context.frequency * 3.0;
    context.fill(out, |px, py| {
        let mut sum = 0.0;
        for &(cx, cy, speed) in &centers {
            let distance = ((px - cx).powi(2) + (py - cy).powi(2)).sqrt();
            let wave = 0.5 + 0.5 * (TAU * (rings * distance - context.loop_fraction * speed)).cos();
            sum += wave * (-2.5 * distance).exp();
        }
        sum / drops as f32 * 1.6
    });
}

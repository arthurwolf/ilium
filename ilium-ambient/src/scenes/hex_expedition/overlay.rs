//! Whole-screen layers over the map: drifting cloud shadows and the weather
//! of each map type. Weather is anchored to the screen, not the map, so it
//! reads as air in front of the camera.

use super::canvas::{Canvas, Rgb};
use super::world::{fbm, hash_cell, unit, MapKind};
use crate::raster::smoothstep;
use std::f32::consts::TAU;

/// Size in dots of one shadow block; shadows are soft, so blocks are fine.
const SHADOW_BLOCK: usize = 3;

pub fn paint_cloud_shadows(
    canvas: &mut Canvas<'_>,
    seconds: f64,
    camera_x: f64,
    camera_y: f64,
    radius: f32,
    seed: u32,
) {
    let scale = 1.0 / (f64::from(radius) * 9.0);
    // Clouds travel across the ground at their own pace.
    let wind = (seconds * 0.012, seconds * 0.004);
    for block_y in (0..canvas.height()).step_by(SHADOW_BLOCK) {
        for block_x in (0..canvas.width()).step_by(SHADOW_BLOCK) {
            let world_x = (camera_x + block_x as f64 + 1.5) * scale - wind.0;
            let world_y = (camera_y + block_y as f64 + 1.5) * scale - wind.1;
            let density = fbm(world_x, world_y, seed ^ 0xc10d, 3);
            let shadow = smoothstep(0.5, 0.64, density);
            if shadow <= 0.0 {
                continue;
            }
            let factor = 1.0 - 0.45 * shadow;
            for y in block_y..(block_y + SHADOW_BLOCK).min(canvas.height()) {
                for x in block_x..(block_x + SHADOW_BLOCK).min(canvas.width()) {
                    canvas.scale_tone_at(x, y, factor);
                }
            }
        }
    }
}

struct Weather {
    cell: f32,
    color: Rgb,
    tone: f32,
}

pub fn paint_weather(canvas: &mut Canvas<'_>, kind: MapKind, seconds: f64, radius: f32) {
    let time = (seconds % 4096.0) as f32;
    let scale = (radius / 18.0).clamp(0.6, 1.6);
    match kind {
        MapKind::Arctic => scatter(
            canvas,
            &Weather {
                cell: 13.0 * scale,
                color: [255, 255, 255],
                tone: 1.0,
            },
            |hash, time| {
                let speed = 0.10 + 0.14 * unit(hash.rotate_left(3));
                let sway = (time * 0.9 + unit(hash.rotate_left(9)) * TAU).sin() * 0.18;
                (
                    unit(hash) + sway * 0.4,
                    (unit(hash.rotate_left(15)) + time * speed).fract(),
                    1.0,
                )
            },
            time,
        ),
        MapKind::Volcanic => scatter(
            canvas,
            &Weather {
                cell: 17.0 * scale,
                color: [255, 120, 36],
                tone: 0.95,
            },
            |hash, time| {
                let speed = 0.08 + 0.12 * unit(hash.rotate_left(3));
                let age = (unit(hash.rotate_left(15)) + time * speed).fract();
                let wiggle = (time * 2.2 + unit(hash.rotate_left(9)) * TAU).sin() * 0.12;
                (unit(hash) + wiggle, 1.0 - age, 1.0 - age)
            },
            time,
        ),
        MapKind::Desert => scatter(
            canvas,
            &Weather {
                cell: 24.0 * scale,
                color: [236, 210, 150],
                tone: 0.6,
            },
            |hash, time| {
                let speed = 0.25 + 0.2 * unit(hash.rotate_left(3));
                (
                    (unit(hash) + time * speed).fract(),
                    unit(hash.rotate_left(15))
                        + (time * 0.7 + unit(hash.rotate_left(9)) * TAU).sin() * 0.03,
                    1.0,
                )
            },
            time,
        ),
        MapKind::Jungle => scatter(
            canvas,
            &Weather {
                cell: 28.0 * scale,
                color: [220, 255, 120],
                tone: 1.0,
            },
            |hash, time| {
                let phase = unit(hash.rotate_left(9)) * TAU;
                let blink = (time * 1.1 + phase).sin().max(0.0).powi(3);
                (
                    unit(hash) + (time * 0.3 + phase).cos() * 0.12,
                    unit(hash.rotate_left(15)) + (time * 0.23 + phase).sin() * 0.12,
                    blink,
                )
            },
            time,
        ),
        MapKind::Savanna => scatter(
            canvas,
            &Weather {
                cell: 26.0 * scale,
                color: [235, 220, 160],
                tone: 0.55,
            },
            |hash, time| {
                let phase = unit(hash.rotate_left(9)) * TAU;
                (
                    (unit(hash) + time * 0.04).fract() + (time * 0.5 + phase).sin() * 0.03,
                    unit(hash.rotate_left(15)) + (time * 0.4 + phase).cos() * 0.05,
                    1.0,
                )
            },
            time,
        ),
    }
}

/// One mote per grid cell. `motion` maps (cell hash, time) to the mote's
/// position inside the cell (fractions, wrapped) and its strength 0..1.
fn scatter(
    canvas: &mut Canvas<'_>,
    weather: &Weather,
    motion: impl Fn(u32, f32) -> (f32, f32, f32),
    time: f32,
) {
    let cell = weather.cell.max(4.0);
    let columns = (canvas.width() as f32 / cell).ceil() as i32 + 1;
    let rows = (canvas.height() as f32 / cell).ceil() as i32 + 1;
    for row in 0..rows {
        for column in 0..columns {
            let hash = hash_cell(column, row, 0x5eed);
            if unit(hash.rotate_left(27)) > 0.55 {
                continue;
            }
            let (fraction_x, fraction_y, strength) = motion(hash, time);
            if strength < 0.2 {
                continue;
            }
            let x = (column as f32 + fraction_x.rem_euclid(1.0)) * cell;
            let y = (row as f32 + fraction_y.rem_euclid(1.0)) * cell;
            canvas.blend(
                x as usize,
                y as usize,
                1.0,
                weather.tone * strength.min(1.0),
                weather.color,
            );
        }
    }
}

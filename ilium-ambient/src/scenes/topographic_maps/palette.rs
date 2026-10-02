//! Elevation tints for the optional per-cell colour.

use super::settings::{PaletteChoice, WorldId};

type Rgb = [u8; 3];

fn mix(from: Rgb, to: Rgb, fraction: f32) -> Rgb {
    let fraction = fraction.clamp(0.0, 1.0);
    [0, 1, 2].map(|channel| {
        (f32::from(from[channel]) + (f32::from(to[channel]) - f32::from(from[channel])) * fraction)
            .round() as u8
    })
}

/// Piecewise-linear gradient over `0..=1`.
fn gradient(stops: &[(f32, Rgb)], position: f32) -> Rgb {
    let position = position.clamp(0.0, 1.0);
    for pair in stops.windows(2) {
        let ((start, from), (end, to)) = (pair[0], pair[1]);
        if position <= end {
            return mix(from, to, (position - start) / (end - start).max(1e-6));
        }
    }
    stops.last().map_or([255, 255, 255], |stop| stop.1)
}

fn hypsometric(height: f32, min: f32, max: f32) -> Rgb {
    if height < 0.0 {
        let depth = (height / min.min(-1.0)).clamp(0.0, 1.0);
        gradient(
            &[
                (0.0, [90, 170, 215]),
                (0.5, [35, 90, 170]),
                (1.0, [8, 22, 85]),
            ],
            depth,
        )
    } else {
        let rise = (height / max.max(1.0)).clamp(0.0, 1.0);
        gradient(
            &[
                (0.0, [50, 130, 70]),
                (0.25, [150, 180, 85]),
                (0.5, [205, 185, 100]),
                (0.75, [150, 105, 70]),
                (1.0, [248, 248, 248]),
            ],
            rise,
        )
    }
}

fn natural(world: WorldId, position: f32) -> Rgb {
    match world {
        WorldId::Moon | WorldId::Craterlands => gradient(
            &[
                (0.0, [38, 38, 46]),
                (0.5, [130, 130, 135]),
                (1.0, [235, 235, 232]),
            ],
            position,
        ),
        WorldId::Mars => gradient(
            &[
                (0.0, [60, 28, 30]),
                (0.4, [170, 85, 50]),
                (0.75, [225, 150, 95]),
                (1.0, [250, 235, 215]),
            ],
            position,
        ),
        WorldId::Venus => gradient(
            &[
                (0.0, [85, 45, 20]),
                (0.5, [215, 150, 60]),
                (1.0, [255, 240, 175]),
            ],
            position,
        ),
        WorldId::Mercury => gradient(
            &[
                (0.0, [48, 44, 46]),
                (0.5, [160, 140, 125]),
                (1.0, [232, 226, 215]),
            ],
            position,
        ),
        WorldId::Ceres => gradient(
            &[
                (0.0, [40, 42, 52]),
                (0.5, [135, 138, 150]),
                (1.0, [238, 240, 245]),
            ],
            position,
        ),
        _ => [200, 200, 200],
    }
}

/// Colour of a point at `height` metres above the zero level, on a world whose
/// relief spans `min..=max` (both relative to the zero level).
pub fn tint(palette: PaletteChoice, world: WorldId, height: f32, min: f32, max: f32) -> Rgb {
    let position = if max > min {
        (height - min) / (max - min)
    } else {
        0.5
    };
    match palette {
        PaletteChoice::Global => [200, 200, 200],
        PaletteChoice::Hypsometric => hypsometric(height, min, max),
        PaletteChoice::Natural => match world {
            WorldId::Earth | WorldId::Aeria | WorldId::Pangaea | WorldId::Ridgeworld => {
                hypsometric(height, min, max)
            }
            _ => natural(world, position),
        },
        PaletteChoice::Heat => gradient(
            &[
                (0.0, [35, 10, 70]),
                (0.35, [170, 30, 70]),
                (0.65, [240, 130, 30]),
                (1.0, [255, 245, 170]),
            ],
            position,
        ),
        PaletteChoice::Ice => gradient(
            &[
                (0.0, [10, 25, 90]),
                (0.5, [60, 160, 220]),
                (1.0, [240, 250, 255]),
            ],
            position,
        ),
    }
}

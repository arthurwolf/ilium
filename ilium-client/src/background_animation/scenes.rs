//! Every scene has its own composition and geometry; time only transports
//! coherent fields or deforms rooted curves. No per-frame randomization.

use super::{
    raster::{hash, noise, smoothstep, Raster},
    AnimationKind,
};
use std::f32::consts::TAU;

pub(super) fn render(raster: &mut Raster, kind: AnimationKind, seconds: f64) {
    let time = seconds as f32;
    match kind {
        AnimationKind::Shoreline => shoreline(raster, time),
        AnimationKind::MoonlitWater => moonlit_water(raster, time),
        AnimationKind::SleepingRidge => sleeping_ridge(raster, time),
        AnimationKind::WindyHillside => windy_hillside(raster, time),
        AnimationKind::TeaSteam => tea_steam(raster, time),
        AnimationKind::Kelp => kelp(raster, time),
        AnimationKind::StoneCaustics => stone_caustics(raster, time),
        AnimationKind::Cloudlets => cloudlets(raster, time),
        AnimationKind::TwoRipples => two_ripples(raster, time),
        AnimationKind::BreathingMountain => breathing_mountain(raster, time),
    }
}

fn shoreline(raster: &mut Raster, time: f32) {
    let phase = (time / 24.0).fract();
    let wash = if phase < 0.26 {
        smoothstep(0.0, 0.26, phase)
    } else if phase < 0.36 {
        1.0
    } else {
        1.0 - smoothstep(0.36, 1.0, phase)
    };
    let tide = 0.40 + wash * 0.27;
    // The prior high-water mark decays on the retreat, rather than following
    // the foam backwards. It is computed analytically for seekable previews.
    let wet_mark = if phase < 0.36 { tide } else { 0.67 };
    let wet_persistence = 1.0 - smoothstep(0.7, 1.0, phase);
    let foam_width = (1.5 / raster.height as f32).max(0.009);
    raster.field(|u, v| {
        let shape = -0.17 * (u - 0.5)
            + 0.014 * (u * 11.0 + time * 0.17).sin()
            + 0.008 * (u * 23.0 - time * 0.12).sin();
        let distance = v - tide - shape;
        let foam = 1.0 - smoothstep(foam_width * 0.2, foam_width, distance.abs());
        let fragment = 0.55 + 0.45 * smoothstep(-0.6, 0.6, (u * 41.0 - time * 0.18).sin());
        if distance.abs() < foam_width {
            return foam * fragment;
        }
        if distance < 0.0 {
            let ripples = (v * 45.0 + u * 8.0 - time * 0.3).sin();
            let scattered_foam =
                smoothstep(0.84, 0.99, (u * 36.0 + distance * 63.0 + time * 0.2).sin())
                    * (1.0 - smoothstep(0.0, 0.055, -distance));
            return 0.025 + 0.15 * smoothstep(0.7, 1.0, ripples) + scattered_foam * 0.5;
        }
        let wet =
            (1.0 - smoothstep(wet_mark + shape, wet_mark + shape + 0.025, v)) * wet_persistence;
        // Mostly empty sand, with stable low-contrast grain.
        (0.025 + noise(u * 30.0, v * 30.0) * 0.045) * (1.0 - wet * 0.85)
    });
}

fn moonlit_water(raster: &mut Raster, time: f32) {
    let aspect = raster.aspect();
    let moon_x = 0.54;
    let moon_y = 0.20;
    let moon_radius = 0.105;
    let antialias = 1.0 / raster.height as f32;
    raster.field(|u, v| {
        if v < 0.43 {
            let distance = ((u - moon_x) * aspect).hypot(v - moon_y);
            return 1.0 - smoothstep(moon_radius - antialias, moon_radius + antialias, distance);
        }
        let depth = (v - 0.43) / 0.57;
        let center = moon_x
            + (v * 15.0 - time * 0.2).sin() * (0.012 + depth * 0.016)
            + (v * 28.0 + time * 0.13).sin() * depth * 0.01;
        let half_width = 0.012 + depth * depth * 0.12 + 0.007 * (v * 19.0 - time * 0.17).sin();
        let horizontal = 1.0 - smoothstep(half_width * 0.65, half_width, (u - center).abs());
        // Related traveling phases produce horizontal fragments that stretch
        // and move together, with no independent blinking random samples.
        let band = smoothstep(
            0.10,
            0.55,
            (v * 115.0 - time * 0.5 + (v * 17.0).sin()).sin(),
        );
        let swell = 0.60 + 0.40 * (u * 17.0 + v * 8.0 - time * 0.12).sin();
        horizontal * band * swell
    });
}

fn ridge(u: f32, layer: usize) -> f32 {
    match layer {
        0 => 0.50 + 0.08 * (u * 9.0 + 0.3).sin() + 0.035 * (u * 19.0).sin(),
        1 => 0.64 + 0.08 * (u * 7.0 + 1.8).sin() + 0.035 * (u * 14.0 + 0.4).sin(),
        _ => 0.79 + 0.055 * (u * 8.0 + 4.0).sin() + 0.028 * (u * 17.0).sin(),
    }
}

fn sleeping_ridge(raster: &mut Raster, time: f32) {
    raster.field(|u, v| {
        let drift = time * 0.009;
        let warp = (v * 7.0 + time * 0.06).sin() * 0.16;
        let broad = noise(u * 3.8 - drift + warp, v * 3.7 + 8.2);
        let detail = noise(u * 7.0 - drift * 1.2, v * 7.0 + 17.0);
        let bank =
            smoothstep(0.38, 0.70, broad * 0.8 + detail * 0.2) * (1.0 - smoothstep(0.36, 0.58, v));
        let mut light = bank * 0.64;
        if v > ridge(u, 0) {
            light = 0.16;
        }
        if v > ridge(u, 1) {
            light = 0.075;
        }
        let mist = noise(u * 5.2 + time * 0.005, v * 9.0 + 40.0);
        let mist_band =
            (1.0 - smoothstep(0.0, 0.13, (v - 0.71).abs())) * smoothstep(0.32, 0.78, mist);
        light = light.max(mist_band * 0.38);
        if v > ridge(u, 2) {
            light = 0.018;
        }
        light
    });
    for layer in 0..3 {
        raster.curve(100, 0.4, 0.50 - layer as f32 * 0.14, |u| {
            (u, ridge(u, layer))
        });
    }
}

fn hill(u: f32) -> f32 {
    0.55 + 0.23 * (u - 0.42).powi(2) + 0.035 * (u * 7.0).sin()
}

fn windy_hillside(raster: &mut Raster, time: f32) {
    raster.curve(80, 0.45, 0.42, |u| (u, hill(u)));
    // Root positions, height and stiffness are fixed. A shared traveling
    // gust bends neighboring stems together; taller stems lag further.
    for row in 0..7 {
        for column in 0..42 {
            let random = hash(column, row);
            let root_x = (column as f32 + random * 0.7) / 42.0;
            let root_y = hill(root_x) + row as f32 * 0.060 + hash(column + 90, row) * 0.025;
            let height = 0.033 + random * 0.045 + if column % 11 == 0 { 0.035 } else { 0.0 };
            let lag = height * 8.0;
            let gust = (root_x * 8.0 - time * 0.32 - lag).sin()
                + 0.32 * (root_x * 3.0 - time * 0.13 - row as f32 * 0.2).sin();
            let bend = (0.015 + gust * 0.025) / raster.aspect();
            let brightness = 0.42 + (row as f32 / 6.0) * 0.42;
            raster.curve(7, 0.42, brightness, |fraction| {
                (root_x + bend * fraction.powi(2), root_y - height * fraction)
            });
        }
    }
}

fn tea_steam(raster: &mut Raster, time: f32) {
    let aspect = raster.aspect();
    let center = 0.5;
    let cup_half_width = (0.16 / aspect).min(0.24);
    // Cup rim, rounded body, handle and saucer are time-independent.
    raster.curve(50, 0.65, 0.95, |fraction| {
        let angle = fraction * TAU;
        (
            center + cup_half_width * angle.cos(),
            0.815 + 0.025 * angle.sin(),
        )
    });
    raster.curve(40, 0.65, 0.90, |fraction| {
        let x = center - cup_half_width + 2.0 * cup_half_width * fraction;
        (x, 0.84 + 0.10 * (std::f32::consts::PI * fraction).sin())
    });
    raster.line(
        (center - cup_half_width, 0.815),
        (center - cup_half_width * 0.85, 0.877),
        0.6,
        0.9,
    );
    raster.line(
        (center + cup_half_width, 0.815),
        (center + cup_half_width * 0.85, 0.877),
        0.6,
        0.9,
    );
    raster.curve(30, 0.60, 0.8, |fraction| {
        let angle = -std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * fraction;
        (
            center + cup_half_width + 0.07 / aspect * angle.cos(),
            0.855 + 0.04 * angle.sin(),
        )
    });
    raster.curve(50, 0.4, 0.6, |fraction| {
        (
            center + (fraction - 0.5) * cup_half_width * 3.2,
            0.956 + 0.012 * (fraction * TAU).cos(),
        )
    });
    for ribbon in 0..3 {
        let phase = ribbon as f32 * 1.9;
        let start_x = center + (ribbon as f32 - 1.0) * 0.055 / aspect;
        let mut previous = (start_x, 0.785);
        for segment in 1..=64 {
            let age = segment as f32 / 64.0;
            // Analytic streakline of emitted material: age advances upward,
            // and time-age transports broad curls rather than reshuffling dots.
            let emission = time * 0.19 - age * 3.6;
            let curl = (age * 7.0 + emission + phase).sin() * 0.06 * age
                + (age * 11.0 - emission * 0.5 + phase).sin() * 0.025 * age.powi(2);
            let next = (start_x + curl / aspect, 0.785 - age * 0.65);
            let fade = (1.0 - smoothstep(0.45, 1.0, age)) * (0.70 + 0.2 * (emission + phase).sin());
            raster.line(previous, next, 0.45 + age * 0.7, fade);
            previous = next;
        }
    }
}

fn kelp(raster: &mut Raster, time: f32) {
    let aspect = raster.aspect();
    raster.curve(80, 0.75, 0.22, |u| (u, 0.958 + (u * 13.0).sin() * 0.01));
    for plant in 0..7 {
        let seed = hash(plant, 75);
        let root_x = 0.08 + plant as f32 * 0.14;
        let height = 0.42 + seed * 0.36;
        let phase = plant as f32 * 0.78;
        let position = |fraction: f32| {
            let bend = (time * 0.18 - fraction * 2.1 + phase).sin() * 0.066
                + (time * 0.095 - fraction * 4.0 + phase).sin() * 0.03;
            (
                root_x + bend * fraction.powf(1.7) / aspect,
                0.953 - height * fraction,
            )
        };
        let brightness = 0.40 + seed * 0.5;
        raster.curve(40, 0.75, brightness, position);
        for leaf in 1..10 {
            let fraction = leaf as f32 / 11.0;
            let base = position(fraction);
            let side = if leaf % 2 == 0 { 1.0 } else { -1.0 };
            let length = (0.065 + seed * 0.04) * (1.0 - fraction * 0.55);
            let current = (time * 0.18 - fraction * 2.1 + phase).sin();
            // Tapered paired edges produce distinct ribbon leaves, with black
            // gaps between plants rather than a solid sine-wave curtain.
            for edge in [-1.0, 1.0] {
                raster.curve(12, 0.45, brightness, |along| {
                    (
                        base.0 + (side * length * along + current * along.powi(2) * 0.025) / aspect,
                        base.1 - along * length * 0.55
                            + edge * 0.012 * (along * std::f32::consts::PI).sin(),
                    )
                });
            }
        }
    }
}

fn stone_caustics(raster: &mut Raster, time: f32) {
    let aspect = raster.aspect();
    // Fixed stone arrangement, with gently domed shading; only light moves.
    let stones = [
        (0.12, 0.25, 0.18),
        (0.44, 0.22, 0.22),
        (0.78, 0.26, 0.18),
        (0.27, 0.61, 0.23),
        (0.63, 0.59, 0.23),
        (0.91, 0.73, 0.18),
        (0.10, 0.91, 0.18),
    ];
    // A warped cellular boundary field creates connected caustic loops.
    // Sites occupy neighboring cells; their tiny periodic motions are coherent.
    let mut sites = [(0.0, 0.0); 42];
    for gy in -1..5 {
        for gx in -1..6 {
            let index = ((gy + 1) * 7 + gx + 1) as usize;
            let seed = hash(gx, gy);
            sites[index] = (
                gx as f32 + 0.5 + (seed - 0.5) * 0.3 + (time * 0.08 + seed * TAU).sin() * 0.08,
                gy as f32
                    + 0.5
                    + (hash(gx + 50, gy) - 0.5) * 0.3
                    + (time * 0.09 + seed * TAU).cos() * 0.08,
            );
        }
    }
    raster.field(|u, v| {
        let mut dome: f32 = 0.0;
        let mut stone_edge: f32 = 0.0;
        for &(cx, cy, radius) in &stones {
            let distance = (((u - cx) * aspect).powi(2) + ((v - cy) * 1.2).powi(2)).sqrt() / radius;
            if distance < 1.0 {
                dome = dome.max((1.0 - distance * distance).sqrt());
            }
            stone_edge = stone_edge.max((1.0 - ((distance - 1.0).abs() / 0.05)).max(0.0) * 0.52);
        }
        let sample_x =
            (u * 4.0 + (v * 8.0 + time * 0.1).sin() * 0.12 + dome * 0.10).clamp(0.0, 4.99);
        let sample_y =
            (v * 3.0 + (u * 7.0 - time * 0.08).cos() * 0.13 - dome * 0.08).clamp(0.0, 3.99);
        let cell_x = sample_x.floor() as i32;
        let cell_y = sample_y.floor() as i32;
        let mut nearest = f32::INFINITY;
        let mut second = f32::INFINITY;
        for gy in cell_y - 1..=cell_y + 1 {
            for gx in cell_x - 1..=cell_x + 1 {
                if !(-1..6).contains(&gx) || !(-1..5).contains(&gy) {
                    continue;
                }
                let (sx, sy) = sites[((gy + 1) * 7 + gx + 1) as usize];
                let distance = (sample_x - sx).powi(2) + (sample_y - sy).powi(2);
                if distance < nearest {
                    second = nearest;
                    nearest = distance;
                } else if distance < second {
                    second = distance;
                }
            }
        }
        let separation = (second - nearest) / (nearest.sqrt() + second.sqrt() + 0.0001);
        let light = 1.0 - smoothstep(0.012, 0.065, separation);
        // Stationary domes and outlines establish the stones before the moving
        // caustic web crosses them. Keep their shading below the bright light.
        (0.12 * dome + light * (0.25 + dome * 0.70)).max(stone_edge)
    });
}

fn cloudlets(raster: &mut Raster, time: f32) {
    let aspect = raster.aspect();
    let mut sources = [(0.0, 0.0, 0.0); 5];
    for (index, source) in sources.iter_mut().enumerate() {
        let phase = index as f32 * TAU / 5.0;
        *source = (
            0.5 + (time * 0.045 + phase).cos() * 0.25,
            0.49 + (time * 0.061 + phase * 1.3).sin() * 0.19,
            0.066 + index as f32 * 0.006,
        );
    }
    raster.field(|u, v| {
        let mut field = 0.0;
        for &(cx, cy, radius) in &sources {
            let distance_squared = ((u - cx) * aspect).powi(2) + (v - cy).powi(2);
            field += radius * radius / (distance_squared + radius * radius * 0.22);
        }
        smoothstep(0.83, 1.16, field) * 0.90
    });
}

fn two_ripples(raster: &mut Raster, time: f32) {
    let aspect = raster.aspect();
    raster.field(|u, v| {
        let distance_a = ((u - 0.24) * aspect).hypot(v - 0.44);
        let distance_b = ((u - 0.76) * aspect).hypot(v - 0.57);
        let first = (distance_a * 22.0 - time * 0.24).sin();
        let second = (distance_b * 22.0 - time * 0.24 + 0.6).sin();
        // Thresholding only the combined height clips every arc into isolated
        // islands. Retain each broad crest and let the other field modulate its
        // light: continuous concentric arcs identify the two sources, while
        // constructive intersections brighten and destructive crossings dim.
        let crest_a = smoothstep(0.72, 0.95, first);
        let crest_b = smoothstep(0.72, 0.95, second);
        crest_a * (0.58 + second * 0.14) + crest_b * (0.58 + first * 0.14)
    });
}

fn terrain_height(x: f32, depth: f32, time: f32) -> f32 {
    let ridge = 0.22 * (x * 2.7 + depth * 0.6).sin() + 0.14 * (x * 4.5 - depth * 0.9 + 2.0).cos();
    let peaks = 0.24 * (-((x + 0.9).powi(2) * 1.8 + (depth - 3.2).powi(2) * 0.12)).exp()
        + 0.18 * (-((x - 1.2).powi(2) * 2.0 + (depth - 4.5).powi(2) * 0.1)).exp();
    let breathing = 0.025 * (x * 0.9 + depth * 0.6 - time * 0.13).sin()
        + 0.012 * (depth * 0.4 + time * 0.09).sin();
    ridge + peaks + breathing
}

fn breathing_mountain(raster: &mut Raster, time: f32) {
    let aspect = raster.aspect();
    let project = |x: f32, depth: f32| {
        let height = terrain_height(x, depth, time);
        (0.5 + x / (depth * aspect), 0.34 + (0.85 - height) / depth)
    };
    // Depth-dependent spacing prevents a dense flickering patch near the
    // horizon; the stationary camera and coarse mesh retain demoscene form.
    let depths = [1.25, 1.55, 1.95, 2.5, 3.3, 4.5, 6.3, 9.0];
    for (row, depth) in depths.into_iter().enumerate() {
        let brightness = 0.95 - row as f32 * 0.065;
        raster.curve(80, 0.50, brightness, |fraction| {
            project((fraction - 0.5) * 8.0, depth)
        });
    }
    for column in -8..=8 {
        let x = column as f32 * 0.5;
        raster.curve(60, 0.45, 0.76, |fraction| {
            let depth = 1.25 + fraction.powf(1.5) * 7.75;
            project(x, depth)
        });
    }
}

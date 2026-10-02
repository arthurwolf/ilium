//! The tile art: a ground fill per terrain, standing decoration (trees,
//! dunes, peaks) and landmarks, all drawn as shaded vector shapes in tile
//! units (hex circumradius = 1, y grows downwards). Tone leaves the scene as
//! grey levels that the host dithers, so shapes are built from clearly
//! separated tones plus a dark outline instead of fine detail.

use super::canvas::{sd_ellipse, Canvas, Paint, Rgb};
use super::world::{hash_cell, unit, Feature, MapKind, Terrain, Tile, NEIGHBOR_DIRECTIONS};
use crate::raster::smoothstep;
use std::f32::consts::TAU;

const APOTHEM: f32 = 0.866_025_4;

const SNOW: Rgb = [238, 243, 248];
const FIRE: Rgb = [255, 150, 40];
const FLAME: Rgb = [255, 220, 90];
const WOOD: Rgb = [140, 96, 56];
const THATCH: Rgb = [196, 160, 84];
const STONE: Rgb = [176, 168, 150];
const LEAF: Rgb = [70, 150, 60];
const PINE: Rgb = [64, 150, 104];

/// Where a tile sits on the canvas, in dots.
#[derive(Debug, Clone, Copy)]
pub struct TileView {
    pub cx: f32,
    pub cy: f32,
    pub radius: f32,
}

impl TileView {
    fn at(&self, u: f32, v: f32) -> (f32, f32) {
        (self.cx + u * self.radius, self.cy + v * self.radius)
    }

    fn scaled(&self, u: f32) -> f32 {
        u * self.radius
    }

    /// A stroke width in dots that never vanishes on small tiles.
    fn stroke(&self, u: f32) -> f32 {
        (u * self.radius).max(1.3)
    }

    fn outline(&self) -> f32 {
        (self.radius * 0.045).clamp(0.8, 1.4)
    }
}

/// A smoke source, painted above everything else of the frame.
#[derive(Debug, Clone, Copy)]
pub struct Smoke {
    pub x: f32,
    pub y: f32,
    pub radius: f32,
    pub strength: f32,
    pub phase: f32,
}

fn random(variant: u32, salt: i32) -> f32 {
    unit(hash_cell(variant as i32, salt, 0x77aa))
}

fn mix(from: f32, to: f32, amount: f32) -> f32 {
    from + (to - from) * amount
}

fn flat(tone: f32, color: Rgb) -> impl Fn(f32, f32) -> Paint {
    move |_, _| (tone, color)
}

fn ellipsoid(
    center: (f32, f32),
    radii: (f32, f32),
    low: f32,
    high: f32,
    color: Rgb,
) -> impl Fn(f32, f32) -> Paint {
    move |px, py| {
        let nx = (px - center.0) / radii.0.max(0.1);
        let ny = (py - center.1) / radii.1.max(0.1);
        let nz = (1.0 - nx * nx - ny * ny).max(0.0).sqrt();
        let lit = (-0.5 * nx - 0.6 * ny + 0.62 * nz).clamp(0.0, 1.0);
        (mix(low, high, lit), color)
    }
}

pub fn ground_color(terrain: Terrain, kind: MapKind) -> Rgb {
    match terrain {
        Terrain::DeepWater => [38, 86, 140],
        Terrain::Water | Terrain::Reef => [70, 140, 185],
        Terrain::Ice => [190, 225, 238],
        Terrain::Grass => [150, 180, 70],
        Terrain::Jungle => [52, 120, 56],
        Terrain::Swamp => [96, 112, 62],
        Terrain::Desert | Terrain::Oasis => [226, 190, 112],
        Terrain::Hills => match kind {
            MapKind::Jungle => [130, 160, 70],
            MapKind::Savanna => [175, 165, 85],
            MapKind::Desert => [214, 170, 100],
            MapKind::Arctic => [225, 232, 240],
            MapKind::Volcanic => [110, 92, 80],
        },
        Terrain::Mountain | Terrain::Volcano => rock_color(kind),
        Terrain::Snow | Terrain::Pines => [236, 242, 248],
        Terrain::Rock => [120, 104, 92],
        Terrain::Lava => [210, 70, 24],
        Terrain::Beach => match kind {
            MapKind::Volcanic => [110, 96, 96],
            _ => [238, 216, 152],
        },
        Terrain::Forest => [58, 132, 72],
        Terrain::Mangrove => [72, 108, 64],
        Terrain::Dunes => [238, 202, 122],
        Terrain::Mesa => [198, 114, 72],
        Terrain::Glacier => [200, 232, 246],
        Terrain::DeadForest => [104, 88, 80],
        Terrain::Geyser => [136, 116, 106],
    }
}

fn rock_color(kind: MapKind) -> Rgb {
    match kind {
        MapKind::Jungle => [128, 116, 96],
        MapKind::Savanna => [172, 132, 92],
        MapKind::Desert => [200, 140, 92],
        MapKind::Arctic => [176, 188, 206],
        MapKind::Volcanic => [150, 108, 100],
    }
}

fn ground_tone(terrain: Terrain) -> f32 {
    // Ground stays sparse so the map is a quiet background and the sprites,
    // drawn nearly solid, stand out from it.
    match terrain {
        Terrain::DeepWater => 0.07,
        Terrain::Water => 0.13,
        Terrain::Ice => 0.32,
        Terrain::Grass => 0.27,
        Terrain::Jungle => 0.20,
        Terrain::Swamp => 0.20,
        Terrain::Desert | Terrain::Oasis => 0.34,
        Terrain::Hills => 0.28,
        Terrain::Mountain | Terrain::Volcano => 0.24,
        Terrain::Snow | Terrain::Pines => 0.36,
        Terrain::Rock => 0.24,
        Terrain::Lava => 0.06,
        Terrain::Reef => 0.13,
        Terrain::Beach => 0.38,
        Terrain::Forest => 0.24,
        Terrain::Mangrove => 0.22,
        Terrain::Dunes => 0.38,
        Terrain::Mesa => 0.30,
        Terrain::Glacier => 0.44,
        Terrain::DeadForest => 0.22,
        Terrain::Geyser => 0.28,
    }
}

/// Distance in tile units from a point to the nearest hex edge, positive
/// inside.
fn hex_inset(u: f32, v: f32) -> f32 {
    let first = u.abs();
    let second = (0.5 * u + APOTHEM * v).abs();
    let third = (-0.5 * u + APOTHEM * v).abs();
    APOTHEM - first.max(second).max(third)
}

/// One-dot specks scattered over a surface, positioned relative to the tile.
fn speck(rel_x: f32, rel_y: f32, cell: (f32, f32), density: f32, variant: u32, salt: i32) -> bool {
    let ix = (rel_x / cell.0).floor();
    let iy = (rel_y / cell.1).floor();
    let seed = hash_cell(ix as i32, iy as i32, variant.wrapping_add(salt as u32));
    if unit(seed) > density {
        return false;
    }
    let target_x = (ix + unit(seed.rotate_left(11))) * cell.0;
    let target_y = (iy + unit(seed.rotate_left(21))) * cell.1;
    (rel_x - target_x).abs() < 0.75 && (rel_y - target_y).abs() < 0.75
}

fn ground_paint(tile: &Tile, kind: MapKind, view: &TileView, rel: (f32, f32), time: f32) -> Paint {
    let terrain = tile.terrain;
    let color = ground_color(terrain, kind);
    let mut tone = ground_tone(terrain);
    let variant = tile.variant;
    let (u, v) = (rel.0 / view.radius, rel.1 / view.radius);
    match terrain {
        Terrain::DeepWater | Terrain::Water | Terrain::Reef => {
            let deep = terrain == Terrain::DeepWater;
            let band = rel.1 / 3.4 + random(variant, 3) * 7.0;
            let row = band.floor();
            if band - row < 0.3 {
                let drift = time * if deep { 0.07 } else { 0.11 };
                let along =
                    rel.0 / (view.radius * 1.25) + random(variant ^ row as u32, 5) * 5.0 + drift;
                let slot = along.floor();
                let keep = if deep { 0.30 } else { 0.5 };
                if unit(hash_cell(row as i32, slot as i32, variant)) < keep && along - slot < 0.34 {
                    tone = if deep { 0.62 } else { 0.95 };
                }
            }
        }
        Terrain::Ice | Terrain::Glacier => {
            let crack = ((u * 3.1 + v * 1.7 + random(variant, 1) * 9.0).sin()
                * (v * 2.3 - u * 1.3 + random(variant, 2) * 9.0).cos())
            .abs();
            if crack < 0.05 {
                tone = 0.08;
            }
            let glint =
                (rel.0 * 0.07 - rel.1 * 0.05 - time * 0.35 + random(variant, 4) * 6.0).sin();
            if glint > 0.9 {
                tone = 1.0;
            }
        }
        Terrain::Grass | Terrain::Forest => {
            if speck(rel.0, rel.1, (5.0, 4.0), 0.3, variant, 11) {
                tone = 0.9;
            }
        }
        Terrain::Jungle => {
            if speck(rel.0, rel.1, (4.0, 4.0), 0.45, variant, 13) {
                tone = 0.8;
            } else if speck(rel.0, rel.1, (5.0, 3.0), 0.3, variant, 14) {
                tone = 0.0;
            }
        }
        Terrain::Swamp | Terrain::Mangrove => {
            if speck(rel.0, rel.1, (6.0, 4.0), 0.3, variant, 15) {
                tone = 0.8;
            }
        }
        Terrain::Desert | Terrain::Oasis | Terrain::Dunes => {
            let shimmer = (time * 0.7 + rel.0 * 0.05).sin() * 0.25;
            let ripple = (rel.1 * 0.62 + rel.0 * 0.11 + shimmer + random(variant, 6) * 6.0).sin();
            tone += 0.12 * ripple;
            if speck(rel.0, rel.1, (7.0, 5.0), 0.22, variant, 16) {
                tone = 0.9;
            }
        }
        Terrain::Hills => {
            if speck(rel.0, rel.1, (5.0, 4.0), 0.4, variant, 17) {
                tone = 0.85;
            }
        }
        Terrain::Beach => {
            // Wet sand darkens and glitters where waves reach.
            let wash = (time * 0.6 + rel.1 * 0.12 + random(variant, 12) * 6.0).sin();
            if wash > 0.7 {
                tone *= 0.6;
            }
            if speck(rel.0, rel.1, (4.0, 3.0), 0.3, variant, 22) {
                tone = 0.92;
            }
        }
        Terrain::Mesa => {
            // Horizontal rock strata.
            let band = (rel.1 / (view.radius * 0.11)).floor() as i32;
            tone += if band.rem_euclid(3) == 0 { 0.1 } else { 0.0 };
            if speck(rel.0, rel.1, (5.0, 4.0), 0.3, variant, 23) {
                tone = 0.85;
            }
        }
        Terrain::DeadForest | Terrain::Geyser => {
            if speck(rel.0, rel.1, (4.0, 4.0), 0.3, variant, 24) {
                tone = 0.8;
            }
        }
        Terrain::Mountain | Terrain::Volcano | Terrain::Rock => {
            if speck(rel.0, rel.1, (4.0, 3.0), 0.4, variant, 18) {
                tone = 0.85;
            }
        }
        Terrain::Snow | Terrain::Pines => {
            tone += 0.06 * (rel.0 * 0.12 + rel.1 * 0.2 + random(variant, 7) * 6.0).sin();
            if speck(rel.0, rel.1, (5.0, 4.0), 0.3, variant, 20) {
                tone = 0.2;
            }
            let twinkle = time * 2.6 + random(variant, 8) * 40.0;
            if speck(rel.0, rel.1, (9.0, 7.0), 0.12, variant, 21) && (twinkle + rel.0).sin() > 0.2 {
                tone = 1.0;
            }
        }
        Terrain::Lava => {
            let drift = time * 0.18;
            let flow = ((rel.0 * 0.11 + drift).sin() * (rel.1 * 0.13 - drift * 0.7).cos()
                + 0.45
                    * (rel.0 * 0.23 - rel.1 * 0.19 + drift * 1.3 + random(variant, 9) * 6.0).sin())
            .abs();
            let glow = smoothstep(0.34, 0.0, flow);
            let pulse = 0.82 + 0.18 * (time * 1.7 + random(variant, 10) * TAU).sin();
            tone = mix(0.05, 0.95, glow * pulse);
        }
    }
    (tone, color)
}

/// Paints the hex-shaped ground of one tile. `neighbors` gives the terrain
/// across each edge, for coastline foam and shore shading.
pub fn paint_ground(
    canvas: &mut Canvas<'_>,
    tile: &Tile,
    view: &TileView,
    neighbors: &[Terrain; 6],
    kind: MapKind,
    time: f32,
) {
    let radius = view.radius;
    let left = (view.cx - APOTHEM * radius - 1.0).floor().max(0.0) as usize;
    let top = (view.cy - radius - 1.0).floor().max(0.0) as usize;
    let right = ((view.cx + APOTHEM * radius + 1.0).ceil().max(0.0) as usize).min(canvas.width());
    let bottom = ((view.cy + radius + 1.0).ceil().max(0.0) as usize).min(canvas.height());
    let is_water = tile.terrain.is_water();
    let phase = random(tile.variant, 30) * 20.0;
    for y in top..bottom {
        let rel_y = y as f32 + 0.5 - view.cy;
        for x in left..right {
            let rel_x = x as f32 + 0.5 - view.cx;
            let inset = hex_inset(rel_x / radius, rel_y / radius) * radius;
            if inset < -0.5 {
                continue;
            }
            let coverage = (inset + 0.5).clamp(0.0, 1.0);
            let (mut tone, color) = ground_paint(tile, kind, view, (rel_x, rel_y), time);
            // A dotted border line makes every hex readable on sparse ground;
            // open water stays borderless so seas read as one body.
            if inset < 1.0 {
                let nearest = (0..6)
                    .max_by(|first, second| {
                        let score = |index: usize| {
                            rel_x * NEIGHBOR_DIRECTIONS[index].0
                                + rel_y * NEIGHBOR_DIRECTIONS[index].1
                        };
                        score(*first).total_cmp(&score(*second))
                    })
                    .unwrap_or(0);
                if !(is_water && neighbors[nearest].is_water()) {
                    tone = tone.max(0.6);
                }
            }
            if is_water {
                for (index, direction) in NEIGHBOR_DIRECTIONS.iter().enumerate() {
                    if neighbors[index].is_water() {
                        continue;
                    }
                    let toward =
                        (APOTHEM - (rel_x * direction.0 + rel_y * direction.1) / radius) * radius;
                    if !(1.0..2.6).contains(&toward) {
                        continue;
                    }
                    let along = (rel_x * -direction.1 + rel_y * direction.0) / radius;
                    let wave = along * 3.2 + time * 0.55 + phase + index as f32;
                    let pulse = 1.0 + 0.7 * (time * 1.3 + phase).sin();
                    if wave.rem_euclid(1.0) < 0.62 && toward < 1.0 + pulse {
                        tone = 0.9;
                    }
                }
            }
            canvas.blend(x, y, coverage, tone, color);
        }
    }
}

fn slot(variant: u32, index: i32, spread: f32) -> (f32, f32) {
    let angle = random(variant, 40 + index) * TAU;
    let distance = spread * random(variant, 80 + index).sqrt();
    (angle.cos() * distance, angle.sin() * distance * 0.8)
}

fn sway(time: f32, phase: f32, amplitude: f32) -> f32 {
    (time * 1.6 + phase).sin() * amplitude
}

fn palm(
    canvas: &mut Canvas<'_>,
    view: &TileView,
    base: (f32, f32),
    height: f32,
    time: f32,
    phase: f32,
) {
    let outline = view.outline();
    let lean = (phase.sin()) * 0.07;
    let swing = sway(time, phase, 0.035);
    let foot = view.at(base.0, base.1);
    let top = view.at(base.0 + lean + swing, base.1 - height);
    let mid = view.at(base.0 + lean * 0.4, base.1 - height * 0.5);
    canvas.capsule(foot, mid, view.stroke(0.08), outline, flat(0.75, WOOD));
    canvas.capsule(mid, top, view.stroke(0.07), outline, flat(0.75, WOOD));
    for frond in 0..6 {
        let angle = frond as f32 * TAU / 6.0 + 0.4 + swing * 3.0;
        let reach = view.scaled(0.30);
        let end = (
            top.0 + angle.cos() * reach,
            top.1 + angle.sin() * reach * 0.55 + reach * 0.3,
        );
        let shade = if angle.cos() < 0.0 { 1.0 } else { 0.7 };
        canvas.capsule(
            top,
            end,
            view.stroke(0.09),
            outline * 0.7,
            flat(shade, LEAF),
        );
    }
}

fn pine(
    canvas: &mut Canvas<'_>,
    view: &TileView,
    base: (f32, f32),
    height: f32,
    time: f32,
    phase: f32,
) {
    let outline = view.outline();
    let swing = sway(time, phase, 0.012);
    let trunk_top = view.at(base.0, base.1 - height * 0.2);
    canvas.capsule(
        view.at(base.0, base.1),
        trunk_top,
        view.stroke(0.06),
        outline * 0.7,
        flat(0.6, WOOD),
    );
    for tier in 0..3 {
        let fraction = tier as f32 / 3.0;
        let tier_base = base.1 - height * (0.12 + 0.28 * fraction);
        let tier_top = base.1 - height * (0.52 + 0.30 * fraction);
        let half = height * (0.30 - 0.07 * tier as f32);
        let lean = swing * (1.0 + fraction * 2.0);
        let apex = view.at(base.0 + lean, tier_top);
        let left = view.at(base.0 - half + lean * 0.5, tier_base);
        let right = view.at(base.0 + half + lean * 0.5, tier_base);
        let snow_line = apex.1 + (left.1 - apex.1) * 0.45;
        let center_x = apex.0;
        canvas.triangle(apex, left, right, outline * 0.8, move |px, py| {
            if py < snow_line {
                (1.0, SNOW)
            } else if px < center_x {
                (1.0, PINE)
            } else {
                (0.55, PINE)
            }
        });
    }
}

fn dome(
    canvas: &mut Canvas<'_>,
    view: &TileView,
    center: (f32, f32),
    radii: (f32, f32),
    low: f32,
    high: f32,
    color: Rgb,
) {
    let c = view.at(center.0, center.1);
    let rx = view.scaled(radii.0);
    let ry = view.scaled(radii.1);
    let shade = ellipsoid((c.0, c.1 - ry * 0.1), (rx, ry * 1.15), low, high, color);
    canvas.shape(
        (c.0 - rx, c.1 - ry, c.0 + rx, c.1),
        view.outline(),
        |px, py| sd_ellipse(px - c.0, py - c.1, rx, ry).max(py - c.1),
        shade,
    );
}

fn mountain(
    canvas: &mut Canvas<'_>,
    view: &TileView,
    kind: MapKind,
    x_offset: f32,
    scale: f32,
    snowy: bool,
) {
    let outline = view.outline();
    let rock = rock_color(kind);
    let apex = view.at(x_offset, -0.8 * scale + 0.08);
    let left = view.at(x_offset - 0.66 * scale, 0.34);
    let right = view.at(x_offset + 0.66 * scale, 0.34);
    let foot = view.at(x_offset + 0.10 * scale, 0.34);
    let snow_line = apex.1 + (left.1 - apex.1) * 0.36;
    let lit_paint = move |px: f32, py: f32| {
        if snowy && py < snow_line {
            (1.0, SNOW)
        } else {
            let hatch = if ((px * 0.6 + py * 0.9).floor() as i32).rem_euclid(3) == 0 {
                0.12
            } else {
                0.0
            };
            (0.96 - hatch, rock)
        }
    };
    let shade_paint = move |px: f32, py: f32| {
        if snowy && py < snow_line + (right.1 - apex.1) * 0.06 {
            (0.62, SNOW)
        } else {
            let hatch = if ((px * 0.6 - py * 0.9).floor() as i32).rem_euclid(3) == 0 {
                0.1
            } else {
                0.0
            };
            (0.3 + hatch, rock)
        }
    };
    canvas.triangle(apex, left, foot, outline, lit_paint);
    canvas.triangle(apex, foot, right, outline, shade_paint);
}

fn boulder(canvas: &mut Canvas<'_>, view: &TileView, center: (f32, f32), size: f32, color: Rgb) {
    let c = view.at(center.0, center.1);
    let rx = view.scaled(size);
    let ry = view.scaled(size * 0.7);
    canvas.ellipse(
        c,
        (rx, ry),
        view.outline() * 0.9,
        ellipsoid(c, (rx, ry), 0.3, 1.0, color),
    );
}

fn tuft(
    canvas: &mut Canvas<'_>,
    view: &TileView,
    base: (f32, f32),
    time: f32,
    phase: f32,
    color: Rgb,
) {
    let foot = view.at(base.0, base.1);
    let swing = sway(time, phase, 0.02);
    for (dx, height) in [(-0.05, 0.12), (0.0, 0.17), (0.05, 0.11)] {
        let tip = view.at(base.0 + dx * 1.5 + swing, base.1 - height);
        canvas.capsule(
            (foot.0 + view.scaled(dx), foot.1),
            tip,
            1.1,
            0.0,
            flat(1.0, color),
        );
    }
}

fn acacia(canvas: &mut Canvas<'_>, view: &TileView, base: (f32, f32), time: f32, phase: f32) {
    let outline = view.outline();
    let swing = sway(time, phase, 0.012);
    let foot = view.at(base.0, base.1);
    let fork = view.at(base.0 + 0.03, base.1 - 0.22);
    canvas.capsule(
        foot,
        fork,
        view.stroke(0.06),
        outline * 0.7,
        flat(0.4, WOOD),
    );
    let crown = view.at(base.0 + 0.04 + swing, base.1 - 0.30);
    let radii = (view.scaled(0.40), view.scaled(0.13));
    canvas.ellipse(crown, radii, outline, move |_, py| {
        let lift = ((crown.1 - py) / radii.1).clamp(-1.0, 1.0);
        (0.78 + 0.2 * lift, LEAF)
    });
}

fn reed(
    canvas: &mut Canvas<'_>,
    view: &TileView,
    base: (f32, f32),
    height: f32,
    time: f32,
    phase: f32,
) {
    let foot = view.at(base.0, base.1);
    let top = view.at(base.0 + sway(time, phase, 0.03), base.1 - height);
    canvas.capsule(foot, top, 1.3, 0.0, flat(0.9, [150, 175, 80]));
    canvas.ellipse(
        (top.0, top.1 + view.scaled(0.03)),
        (1.1, view.scaled(0.05).max(1.4)),
        0.0,
        flat(0.82, WOOD),
    );
}

/// Standing decoration of a terrain: peaks, trees, dunes and the like.
pub fn paint_decor(
    canvas: &mut Canvas<'_>,
    tile: &Tile,
    view: &TileView,
    kind: MapKind,
    time: f32,
    smoke: &mut Vec<Smoke>,
) {
    let variant = tile.variant;
    let outline = view.outline();
    match tile.terrain {
        Terrain::DeepWater | Terrain::Water | Terrain::Lava | Terrain::Ice | Terrain::Snow => {
            if tile.terrain == Terrain::Snow {
                for index in 0..2 {
                    let (x, y) = slot(variant, index, 0.45);
                    let c = view.at(x, y + 0.1);
                    let rx = view.scaled(0.2 + 0.1 * random(variant, 90 + index));
                    canvas.ellipse(
                        c,
                        (rx, rx * 0.4),
                        0.0,
                        ellipsoid(c, (rx, rx * 0.5), 0.5, 1.0, SNOW),
                    );
                }
            }
            if tile.terrain == Terrain::Lava {
                for index in 0..3 {
                    let (x, y) = slot(variant, index, 0.5);
                    let age = (time * 0.3 + random(variant, 100 + index)).rem_euclid(1.0);
                    if age < 0.55 {
                        let grow = age / 0.55;
                        let radius = view.scaled(0.04 + 0.07 * grow).max(0.9);
                        let c = view.at(x, y);
                        canvas.ellipse(c, (radius, radius), 0.0, flat(1.0 - 0.3 * grow, FLAME));
                    }
                }
            }
        }
        Terrain::Grass => {
            for index in 0..4 {
                let (x, y) = slot(variant, index, 0.55);
                tuft(
                    canvas,
                    view,
                    (x, y + 0.1),
                    time,
                    random(variant, 120 + index) * TAU,
                    [190, 210, 90],
                );
            }
            let (x, y) = slot(variant, 7, 0.4);
            if random(variant, 130) < 0.7 {
                acacia(canvas, view, (x, y + 0.2), time, random(variant, 131) * TAU);
            }
        }
        Terrain::Jungle => {
            let mut blobs = Vec::new();
            let count = 5 + (random(variant, 140) * 3.0) as i32;
            for index in 0..count {
                let (x, y) = slot(variant, index, 0.55);
                let radius = 0.24 + 0.12 * random(variant, 150 + index);
                blobs.push((x, y, radius, random(variant, 160 + index) * TAU));
            }
            blobs.sort_by(|a, b| a.1.total_cmp(&b.1));
            for (x, y, radius, phase) in blobs {
                let c = view.at(x + sway(time, phase, 0.02), y + 0.05);
                let rx = view.scaled(radius);
                canvas.ellipse(
                    c,
                    (rx, rx * 0.92),
                    outline,
                    ellipsoid(c, (rx, rx * 0.92), 0.3, 0.98, LEAF),
                );
            }
            if random(variant, 170) < 0.6 {
                let (x, y) = slot(variant, 9, 0.35);
                palm(
                    canvas,
                    view,
                    (x, y + 0.25),
                    0.6,
                    time,
                    random(variant, 171) * TAU,
                );
            }
        }
        Terrain::Swamp => {
            for index in 0..2 {
                let (x, y) = slot(variant, index, 0.4);
                let c = view.at(x, y + 0.1);
                let radii = (view.scaled(0.28), view.scaled(0.12));
                canvas.ellipse(c, radii, 0.0, move |_, py| {
                    let rim = ((c.1 - py) / radii.1).clamp(-1.0, 1.0);
                    (0.10 + 0.18 * rim.max(0.0), [60, 90, 70])
                });
                let age = (time * 0.35 + random(variant, 180 + index)).rem_euclid(1.0);
                let pop = view.scaled(0.02 + 0.045 * age).max(0.9);
                canvas.ellipse(
                    (c.0 + view.scaled(0.05), c.1 - view.scaled(0.02)),
                    (pop, pop),
                    0.0,
                    flat(0.8 * (1.0 - age * 0.7), [200, 220, 200]),
                );
            }
            for index in 0..5 {
                let (x, y) = slot(variant, 20 + index, 0.5);
                reed(
                    canvas,
                    view,
                    (x, y + 0.2),
                    0.22 + 0.12 * random(variant, 190 + index),
                    time,
                    random(variant, 200 + index) * TAU,
                );
            }
        }
        Terrain::Desert => {
            let flip = if random(variant, 210) < 0.5 {
                1.0
            } else {
                -1.0
            };
            for index in 0..2 {
                let x = (-0.28 + 0.5 * index as f32) * flip
                    + (random(variant, 211 + index) - 0.5) * 0.2;
                let y = -0.12 + 0.34 * index as f32 + (random(variant, 213 + index) - 0.5) * 0.12;
                let c = view.at(x, y);
                let radii = (view.scaled(0.36), view.scaled(0.13));
                let shadow = (c.0 + view.scaled(0.1), c.1 + view.scaled(0.075));
                canvas.ellipse(c, radii, 0.0, move |px, py| {
                    let inside_shadow =
                        sd_ellipse(px - shadow.0, py - shadow.1, radii.0, radii.1) < 0.0;
                    if inside_shadow {
                        (0.36, [200, 160, 96])
                    } else {
                        (0.94, [240, 214, 140])
                    }
                });
            }
            if random(variant, 220) < 0.3 {
                cactus(canvas, view, (0.28 * flip, 0.2));
            }
        }
        Terrain::Oasis => {
            let c = view.at(0.0, 0.14);
            let radii = (view.scaled(0.44), view.scaled(0.19));
            let ripple = time * 0.9;
            canvas.ellipse(c, radii, outline, move |px, py| {
                let wave = ((px - c.0) * 0.3 + ripple).sin();
                let streak = ((py - c.1).abs() < 1.0 && wave > 0.5) as i32 as f32;
                (0.25 + 0.5 * streak, [70, 150, 190])
            });
            let mut trees: [(f32, f32); 3] = [(-0.40, -0.02), (0.38, 0.0), (0.04, -0.14)];
            trees.sort_by(|a, b| a.1.total_cmp(&b.1));
            for (index, (x, y)) in trees.into_iter().enumerate() {
                palm(
                    canvas,
                    view,
                    (x, y + 0.18),
                    0.5 + 0.1 * random(variant, 230 + index as i32),
                    time,
                    random(variant, 240 + index as i32) * TAU,
                );
            }
        }
        Terrain::Hills => {
            let (low, high, color) = (0.3, 1.0, ground_color(Terrain::Hills, kind));
            dome(canvas, view, (-0.26, 0.18), (0.42, 0.34), low, high, color);
            dome(canvas, view, (0.24, 0.26), (0.46, 0.4), low, high, color);
            if kind == MapKind::Arctic {
                return;
            }
            for index in 0..2 {
                let (x, y) = slot(variant, index, 0.4);
                tuft(
                    canvas,
                    view,
                    (x, y + 0.25),
                    time,
                    random(variant, 250 + index) * TAU,
                    [190, 210, 90],
                );
            }
        }
        Terrain::Mountain => {
            let snowy = matches!(kind, MapKind::Arctic) || random(variant, 260) < 0.4;
            let big = 0.95 + 0.25 * random(variant, 261);
            mountain(canvas, view, kind, -0.12, big, snowy);
            if random(variant, 262) < 0.7 {
                mountain(
                    canvas,
                    view,
                    kind,
                    0.32,
                    0.55 + 0.2 * random(variant, 263),
                    snowy,
                );
            }
        }
        Terrain::Pines => {
            let mut trees = Vec::new();
            for index in 0..4 {
                let (x, y) = slot(variant, index, 0.55);
                trees.push((x, y + 0.3, 0.78 + 0.26 * random(variant, 270 + index)));
            }
            trees.sort_by(|a, b| a.1.total_cmp(&b.1));
            for (index, (x, y, height)) in trees.into_iter().enumerate() {
                pine(
                    canvas,
                    view,
                    (x, y),
                    height,
                    time,
                    random(variant, 280 + index as i32) * TAU,
                );
            }
        }
        Terrain::Rock => {
            for index in 0..3 {
                let (x, y) = slot(variant, index, 0.5);
                boulder(
                    canvas,
                    view,
                    (x, y + 0.1),
                    0.12 + 0.12 * random(variant, 290 + index),
                    rock_color(kind),
                );
            }
        }
        Terrain::Reef => reef(canvas, view, variant, time),
        Terrain::Beach => beach(canvas, view, variant, time),
        Terrain::Forest => {
            let mut trees: Vec<(f32, f32, f32, f32)> = (0..4)
                .map(|index| {
                    let (x, y) = slot(variant, index, 0.55);
                    (
                        x,
                        y + 0.25,
                        0.22 + 0.1 * random(variant, 400 + index),
                        random(variant, 410 + index) * TAU,
                    )
                })
                .collect();
            trees.sort_by(|a, b| a.1.total_cmp(&b.1));
            for (x, y, size, phase) in trees {
                broadleaf(canvas, view, (x, y), size, time, phase);
            }
            let (x, y) = slot(variant, 8, 0.4);
            tuft(
                canvas,
                view,
                (x, y + 0.25),
                time,
                random(variant, 420) * TAU,
                [190, 220, 100],
            );
        }
        Terrain::Mangrove => mangrove(canvas, view, variant, time),
        Terrain::Dunes => dunes(canvas, view, variant, time),
        Terrain::Mesa => mesa(canvas, view, variant),
        Terrain::Glacier => glacier(canvas, view, variant, time),
        Terrain::DeadForest => {
            let mut trees: Vec<(f32, f32)> = (0..3)
                .map(|index| {
                    let (x, y) = slot(variant, index, 0.55);
                    (x, y + 0.28)
                })
                .collect();
            trees.sort_by(|a, b| a.1.total_cmp(&b.1));
            for (index, (x, y)) in trees.into_iter().enumerate() {
                dead_tree(
                    canvas,
                    view,
                    (x, y),
                    0.46 + 0.14 * random(variant, 430 + index as i32),
                    time,
                );
            }
            boulder(
                canvas,
                view,
                (slot(variant, 9, 0.4).0, 0.32),
                0.1,
                rock_color(MapKind::Volcanic),
            );
        }
        Terrain::Geyser => geyser(canvas, view, variant, time, smoke),
        Terrain::Volcano => volcano(canvas, view, variant, time, smoke),
    }
}

fn cactus(canvas: &mut Canvas<'_>, view: &TileView, base: (f32, f32)) {
    let paint = flat(0.62, [90, 150, 70]);
    let outline = view.outline() * 0.8;
    let foot = view.at(base.0, base.1);
    let top = view.at(base.0, base.1 - 0.34);
    let width = view.stroke(0.075);
    canvas.capsule(foot, top, width, outline, &paint);
    let arm = view.at(base.0 - 0.13, base.1 - 0.15);
    canvas.capsule(view.at(base.0, base.1 - 0.12), arm, width, outline, &paint);
    canvas.capsule(
        arm,
        view.at(base.0 - 0.13, base.1 - 0.27),
        width,
        outline,
        &paint,
    );
    let arm_right = view.at(base.0 + 0.12, base.1 - 0.2);
    canvas.capsule(
        view.at(base.0, base.1 - 0.18),
        arm_right,
        width,
        outline,
        &paint,
    );
    canvas.capsule(
        arm_right,
        view.at(base.0 + 0.12, base.1 - 0.3),
        width,
        outline,
        &paint,
    );
}

fn volcano(
    canvas: &mut Canvas<'_>,
    view: &TileView,
    variant: u32,
    time: f32,
    smoke: &mut Vec<Smoke>,
) {
    let outline = view.outline();
    let rock = rock_color(MapKind::Volcanic);
    let apex_left = view.at(-0.2, -0.5);
    let apex_right = view.at(0.2, -0.5);
    let base_left = view.at(-0.7, 0.32);
    let base_right = view.at(0.7, 0.32);
    let foot = view.at(0.04, 0.32);
    let apex_mid = view.at(0.0, -0.5);
    canvas.triangle(apex_left, base_left, foot, outline, move |px, py| {
        let hatch = if ((px * 0.5 + py).floor() as i32).rem_euclid(3) == 0 {
            0.1
        } else {
            0.0
        };
        (0.92 - hatch, rock)
    });
    canvas.triangle(apex_left, foot, apex_mid, outline * 0.5, flat(0.92, rock));
    canvas.triangle(apex_mid, foot, apex_right, outline * 0.5, flat(0.4, rock));
    canvas.triangle(apex_right, foot, base_right, outline, move |px, py| {
        let hatch = if ((px * 0.5 - py).floor() as i32).rem_euclid(3) == 0 {
            0.08
        } else {
            0.0
        };
        (0.4 + hatch, rock)
    });
    let pulse = 0.78 + 0.22 * (time * 2.1 + random(variant, 300) * TAU).sin();
    let crater = view.at(0.0, -0.5);
    canvas.ellipse(
        crater,
        (view.scaled(0.23), view.scaled(0.075)),
        outline * 0.8,
        flat(0.7 + 0.3 * pulse, FIRE),
    );
    for (start, end, width) in [
        ((0.05, -0.47), (0.3, 0.12), 0.07),
        ((-0.04, -0.46), (-0.2, 0.0), 0.05),
    ] {
        let streak = 0.45 + 0.5 * (0.5 + 0.5 * (time * 1.6 + start.0 * 9.0).sin());
        canvas.capsule(
            view.at(start.0, start.1),
            view.at(end.0, end.1),
            view.stroke(width),
            0.0,
            flat(streak, FIRE),
        );
    }
    smoke.push(Smoke {
        x: crater.0,
        y: crater.1 - view.scaled(0.05),
        radius: view.radius,
        strength: 1.7,
        phase: random(variant, 301),
    });
}

fn hut(canvas: &mut Canvas<'_>, view: &TileView, kind: MapKind, center: (f32, f32), size: f32) {
    let outline = view.outline() * 0.85;
    let c = view.at(center.0, center.1);
    let w = view.scaled(size);
    match kind {
        MapKind::Arctic => {
            let radii = (w * 0.62, w * 0.55);
            canvas.shape(
                (c.0 - radii.0, c.1 - radii.1, c.0 + radii.0, c.1),
                outline,
                |px, py| sd_ellipse(px - c.0, py - c.1, radii.0, radii.1).max(py - c.1),
                move |px, py| {
                    let course = ((py - c.1) / (radii.1 * 0.34)).floor() as i32;
                    let seam = (((px - c.0) / (radii.0 * 0.5) + course as f32 * 0.5).floor()
                        as i32
                        + course)
                        .rem_euclid(2)
                        == 0;
                    (if seam { 0.95 } else { 0.78 }, SNOW)
                },
            );
            canvas.rect(
                (c.0, c.1 - radii.1 * 0.18),
                (w * 0.13, radii.1 * 0.18),
                0.0,
                flat(0.0, [0, 0, 0]),
            );
        }
        MapKind::Desert => {
            let half = (w * 0.5, w * 0.36);
            canvas.rect(c, half, outline, move |_, py| {
                (
                    if py < c.1 - half.1 * 0.55 { 1.0 } else { 0.8 },
                    [236, 210, 150],
                )
            });
            canvas.rect(
                (c.0, c.1 + half.1 * 0.3),
                (w * 0.1, half.1 * 0.7),
                0.0,
                flat(0.05, [30, 20, 10]),
            );
        }
        _ => {
            let wall_half = (w * 0.42, w * 0.22);
            canvas.rect((c.0, c.1 + w * 0.1), wall_half, outline, flat(0.85, WOOD));
            let apex = (c.0, c.1 - w * 0.78);
            let left = (c.0 - w * 0.62, c.1 - w * 0.1);
            let right = (c.0 + w * 0.62, c.1 - w * 0.1);
            let center_x = c.0;
            canvas.triangle(apex, left, right, outline, move |px, _| {
                (if px < center_x { 1.0 } else { 0.55 }, THATCH)
            });
            canvas.rect(
                (c.0, c.1 + w * 0.2),
                (w * 0.09, w * 0.12),
                0.0,
                flat(0.05, [30, 20, 10]),
            );
        }
    }
}

fn village(
    canvas: &mut Canvas<'_>,
    view: &TileView,
    tile: &Tile,
    kind: MapKind,
    smoke: &mut Vec<Smoke>,
) {
    let mut huts: [(f32, f32, f32); 3] =
        [(-0.40, 0.12, 0.42), (0.38, 0.18, 0.40), (0.0, -0.16, 0.44)];
    huts.sort_by(|a, b| a.1.total_cmp(&b.1));
    for (x, y, size) in huts {
        hut(canvas, view, kind, (x, y + 0.08), size);
    }
    let roof = view.at(0.0, -0.14 - 0.25);
    smoke.push(Smoke {
        x: roof.0,
        y: roof.1,
        radius: view.radius,
        strength: 1.0,
        phase: random(tile.variant, 310),
    });
}

fn temple(canvas: &mut Canvas<'_>, view: &TileView, time: f32, variant: u32) {
    let outline = view.outline();
    let tiers = [(0.56, 0.12, 0.26), (0.42, 0.12, 0.14), (0.28, 0.12, 0.02)];
    for (half_width, height, top) in tiers {
        let c = view.at(0.0, top);
        let half = (view.scaled(half_width), view.scaled(height));
        canvas.rect(c, half, outline, move |px, py| {
            let step = (((px - c.0) / 2.5).floor() as i32 + ((py - c.1) / 2.5).floor() as i32)
                .rem_euclid(2);
            (
                if py < c.1 - half.1 * 0.3 { 0.92 } else { 0.7 } - 0.08 * step as f32,
                STONE,
            )
        });
    }
    let shrine = view.at(0.0, -0.22);
    canvas.rect(
        shrine,
        (view.scaled(0.17), view.scaled(0.13)),
        outline,
        flat(0.82, STONE),
    );
    canvas.triangle(
        view.at(-0.21, -0.35),
        view.at(0.21, -0.35),
        view.at(0.0, -0.58),
        outline,
        move |px, _| (if px < shrine.0 { 0.9 } else { 0.55 }, STONE),
    );
    canvas.rect(
        (shrine.0, shrine.1 + view.scaled(0.06)),
        (view.scaled(0.05), view.scaled(0.07)),
        0.0,
        flat(0.04, [20, 14, 10]),
    );
    for side in [-1.0f32, 1.0] {
        let flicker = unit(hash_cell((time * 9.0) as i32, side as i32, variant));
        let base = view.at(0.44 * side, 0.2);
        let size = view.scaled(0.05 + 0.03 * flicker).max(1.0);
        canvas.rect(
            (base.0, base.1 + size),
            (1.0, size * 1.2),
            0.0,
            flat(0.5, WOOD),
        );
        canvas.ellipse(
            (base.0, base.1 - size * 0.4),
            (size, size * 1.5),
            0.0,
            flat(1.0, FLAME),
        );
    }
}

fn pyramid(canvas: &mut Canvas<'_>, view: &TileView) {
    let outline = view.outline();
    let shadow = view.at(0.28, 0.36);
    canvas.ellipse(
        shadow,
        (view.scaled(0.7), view.scaled(0.1)),
        0.0,
        flat(0.0, [0, 0, 0]),
    );
    let apex = view.at(-0.04, -0.82);
    let left = view.at(-0.7, 0.34);
    let right = view.at(0.66, 0.34);
    let foot = view.at(0.14, 0.38);
    let course = view.scaled(0.075);
    let base_y = left.1;
    canvas.triangle(apex, left, foot, outline, move |_, py| {
        let row = ((base_y - py) / course).floor() as i32;
        (
            0.92 - if row.rem_euclid(2) == 0 { 0.14 } else { 0.0 },
            [232, 200, 130],
        )
    });
    canvas.triangle(apex, foot, right, outline, move |_, py| {
        let row = ((base_y - py) / course).floor() as i32;
        (
            0.4 - if row.rem_euclid(2) == 0 { 0.1 } else { 0.0 },
            [200, 160, 96],
        )
    });
}

fn camp(canvas: &mut Canvas<'_>, view: &TileView, time: f32, variant: u32, smoke: &mut Vec<Smoke>) {
    let outline = view.outline();
    let apex = view.at(-0.22, -0.46);
    let left = view.at(-0.6, 0.22);
    let right = view.at(0.12, 0.22);
    let ridge = apex.0;
    canvas.triangle(apex, left, right, outline, move |px, _| {
        (if px < ridge { 1.0 } else { 0.55 }, [226, 204, 150])
    });
    canvas.triangle(
        view.at(-0.22, -0.06),
        view.at(-0.34, 0.22),
        view.at(-0.1, 0.22),
        0.0,
        flat(0.05, [20, 14, 10]),
    );
    let fire = view.at(0.36, 0.18);
    canvas.capsule(
        (fire.0 - view.scaled(0.1), fire.1 + view.scaled(0.04)),
        (fire.0 + view.scaled(0.1), fire.1 - view.scaled(0.02)),
        view.stroke(0.045),
        0.6,
        flat(0.5, WOOD),
    );
    let step = (time * 9.0).floor() as i32;
    let flick = unit(hash_cell(step, 3, variant));
    let height = view.scaled(0.1 + 0.08 * flick);
    let sideways = (unit(hash_cell(step, 5, variant)) - 0.5) * view.scaled(0.03);
    canvas.ellipse(
        (fire.0 + sideways, fire.1 - height * 0.8),
        (view.scaled(0.055), height),
        0.0,
        flat(1.0, FIRE),
    );
    canvas.ellipse(
        (fire.0 + sideways, fire.1 - height * 0.5),
        (view.scaled(0.03), height * 0.55),
        0.0,
        flat(1.0, FLAME),
    );
    smoke.push(Smoke {
        x: fire.0,
        y: fire.1 - height * 1.6,
        radius: view.radius * 0.6,
        strength: 0.7,
        phase: random(variant, 320),
    });
}

fn ruins(canvas: &mut Canvas<'_>, view: &TileView, variant: u32) {
    let outline = view.outline() * 0.9;
    let columns = [
        (-0.38, 0.30, 0.34),
        (-0.1, 0.34, 0.2),
        (0.22, 0.26, 0.4),
        (0.46, 0.34, 0.12),
    ];
    for (index, (x, base, height)) in columns.into_iter().enumerate() {
        let tall = height * (0.9 + 0.5 * random(variant, 330 + index as i32));
        let center = view.at(x, base - tall * 0.5);
        let half = (view.scaled(0.055), view.scaled(tall * 0.5));
        canvas.rect(center, half, outline, move |px, py| {
            let fluting = if ((px - center.0) * 1.2).floor() as i32 % 2 == 0 {
                0.1
            } else {
                0.0
            };
            (
                0.82 - fluting - 0.08 * ((py - center.1) / half.1).clamp(0.0, 1.0),
                STONE,
            )
        });
        if tall > 0.3 {
            canvas.rect(
                (center.0, center.1 - half.1),
                (half.0 * 1.6, view.scaled(0.025).max(0.9)),
                outline * 0.6,
                flat(0.9, STONE),
            );
        }
    }
    boulder(canvas, view, (0.0, 0.34), 0.1, STONE);
}

fn ship(canvas: &mut Canvas<'_>, view: &TileView, time: f32, variant: u32) {
    let outline = view.outline();
    let phase = random(variant, 340) * TAU;
    let drift = (time * 0.12 + phase).sin() * 0.18;
    let bob = (time * 1.4 + phase).sin() * 0.025;
    let c = view.at(drift, 0.1 + bob);
    let hull = (view.scaled(0.38), view.scaled(0.11));
    let wake_x = c.0 - view.scaled(0.34) * (time * 0.12 + phase).cos().signum();
    for offset in [0.0, 0.07] {
        canvas.capsule(
            (wake_x, c.1 + view.scaled(0.08 + offset)),
            (wake_x + view.scaled(0.2), c.1 + view.scaled(0.1 + offset)),
            1.0,
            0.0,
            flat(0.6, [200, 225, 240]),
        );
    }
    let mast_top = view.at(drift, -0.5 + bob);
    canvas.capsule(
        (c.0, c.1 - view.scaled(0.02)),
        mast_top,
        1.2,
        0.6,
        flat(0.45, WOOD),
    );
    let sail_top = (mast_top.0 + 1.0, mast_top.1 + 1.0);
    // The expedition's pennant, waving in the wind.
    let wave = (time * 3.0 + phase).sin() * view.scaled(0.03);
    canvas.triangle(
        (mast_top.0, mast_top.1 - view.scaled(0.02)),
        (mast_top.0, mast_top.1 + view.scaled(0.07)),
        (
            mast_top.0 + view.scaled(0.2),
            mast_top.1 + view.scaled(0.03) + wave,
        ),
        0.0,
        flat(1.0, [230, 60, 50]),
    );
    canvas.triangle(
        sail_top,
        (c.0 + 1.0, c.1 - view.scaled(0.05)),
        (c.0 + view.scaled(0.34), c.1 - view.scaled(0.05)),
        outline * 0.8,
        flat(1.0, [250, 245, 235]),
    );
    canvas.shape(
        (c.0 - hull.0, c.1 - hull.1, c.0 + hull.0, c.1 + hull.1),
        outline,
        |px, py| sd_ellipse(px - c.0, py - c.1, hull.0, hull.1).max(c.1 - view.scaled(0.02) - py),
        flat(0.85, WOOD),
    );
}

pub fn paint_feature(
    canvas: &mut Canvas<'_>,
    tile: &Tile,
    view: &TileView,
    kind: MapKind,
    time: f32,
    smoke: &mut Vec<Smoke>,
) {
    // Landmarks stand on a dark clearing so they read against any ground.
    if matches!(
        tile.feature,
        Feature::Village
            | Feature::Camp
            | Feature::Ruins
            | Feature::Temple
            | Feature::Shrine
            | Feature::Mine
    ) {
        canvas.ellipse(
            view.at(0.0, 0.16),
            (view.scaled(0.74), view.scaled(0.42)),
            0.0,
            flat(0.0, [0, 0, 0]),
        );
    }
    match tile.feature {
        Feature::None => {}
        Feature::Village => village(canvas, view, tile, kind, smoke),
        Feature::Temple => temple(canvas, view, time, tile.variant),
        Feature::Pyramid => pyramid(canvas, view),
        Feature::Camp => camp(canvas, view, time, tile.variant, smoke),
        Feature::Ruins => ruins(canvas, view, tile.variant),
        Feature::Ship => ship(canvas, view, time, tile.variant),
        Feature::Cave => cave(canvas, view, time, tile.variant, kind),
        Feature::Shrine => shrine(canvas, view, time, tile.variant),
        Feature::Mine => mine(canvas, view, time, tile.variant),
    }
}

/// Puffs of smoke rising from every source, drawn last so they overlap what
/// stands next to them.
pub fn paint_smoke(canvas: &mut Canvas<'_>, sources: &[Smoke], time: f32) {
    const PUFFS: i32 = 5;
    for source in sources {
        for puff in 0..PUFFS {
            let age = (time * 0.2 + puff as f32 / PUFFS as f32 + source.phase).rem_euclid(1.0);
            let rise = age * source.radius * 1.05 * source.strength;
            let wander = (age * 4.0 + puff as f32 + source.phase * 9.0).sin() * 0.1 * source.radius;
            let center = (
                source.x + wander + age * 0.28 * source.radius,
                source.y - rise,
            );
            let size = source.radius * (0.05 + 0.15 * age) * source.strength.sqrt();
            let strength = 0.78 * (1.0 - age).powf(1.3);
            canvas.ellipse(
                center,
                (size, size * 0.9),
                0.0,
                flat(strength, [200, 200, 205]),
            );
        }
    }
}

fn broadleaf(
    canvas: &mut Canvas<'_>,
    view: &TileView,
    base: (f32, f32),
    size: f32,
    time: f32,
    phase: f32,
) {
    let outline = view.outline() * 0.9;
    let foot = view.at(base.0, base.1);
    let top = view.at(base.0 + sway(time, phase, 0.012), base.1 - size * 1.15);
    canvas.capsule(foot, top, view.stroke(0.06), outline * 0.7, flat(0.7, WOOD));
    let center = (top.0, top.1 - view.scaled(size * 0.25));
    let radii = (view.scaled(size * 1.05), view.scaled(size * 0.95));
    canvas.ellipse(
        center,
        radii,
        outline,
        ellipsoid(center, radii, 0.35, 1.0, LEAF),
    );
}

fn dead_tree(canvas: &mut Canvas<'_>, view: &TileView, base: (f32, f32), height: f32, time: f32) {
    let outline = view.outline() * 0.7;
    let bone = [206, 188, 168];
    let foot = view.at(base.0, base.1);
    let top = view.at(base.0 + sway(time, base.0 * 9.0, 0.008), base.1 - height);
    canvas.capsule(foot, top, view.stroke(0.07), outline, flat(0.85, bone));
    for (fraction, reach, rise) in [(0.45, -0.16, 0.14), (0.62, 0.15, 0.16), (0.82, -0.09, 0.1)] {
        let from = (
            foot.0 + (top.0 - foot.0) * fraction,
            foot.1 + (top.1 - foot.1) * fraction,
        );
        let to = (from.0 + view.scaled(reach), from.1 - view.scaled(rise));
        canvas.capsule(from, to, view.stroke(0.04), 0.0, flat(0.85, bone));
    }
}

fn mangrove(canvas: &mut Canvas<'_>, view: &TileView, variant: u32, time: f32) {
    let outline = view.outline() * 0.8;
    let pool = view.at(0.0, 0.22);
    let radii = (view.scaled(0.5), view.scaled(0.15));
    canvas.ellipse(pool, radii, 0.0, flat(0.1, [60, 90, 80]));
    for index in 0..3 {
        let base_x = -0.34 + 0.34 * index as f32 + (random(variant, 440 + index) - 0.5) * 0.1;
        let foot = view.at(base_x, 0.24);
        // Arched stilt roots.
        for side in [-1.0f32, 1.0] {
            let knee = (foot.0 + view.scaled(0.1 * side), foot.1 - view.scaled(0.1));
            canvas.capsule(
                (foot.0, foot.1 - view.scaled(0.16)),
                knee,
                view.stroke(0.035),
                0.0,
                flat(0.7, WOOD),
            );
            canvas.capsule(
                knee,
                (foot.0 + view.scaled(0.15 * side), foot.1),
                view.stroke(0.035),
                0.0,
                flat(0.7, WOOD),
            );
        }
        let crown = (
            foot.0 + sway(time, index as f32 * 2.0, view.scaled(0.012)),
            foot.1 - view.scaled(0.36),
        );
        let crown_radii = (view.scaled(0.19), view.scaled(0.15));
        canvas.ellipse(
            crown,
            crown_radii,
            outline,
            ellipsoid(crown, crown_radii, 0.3, 0.95, [90, 150, 80]),
        );
    }
}

fn beach(canvas: &mut Canvas<'_>, view: &TileView, variant: u32, time: f32) {
    for index in 0..3 {
        let (x, y) = slot(variant, index, 0.6);
        let shell = view.at(x, y + 0.15);
        canvas.ellipse(shell, (1.3, 1.0), 0.0, flat(1.0, [250, 235, 215]));
    }
    if random(variant, 450) < 0.45 {
        let (x, _) = slot(variant, 5, 0.3);
        palm(
            canvas,
            view,
            (x, 0.3),
            0.58,
            time,
            random(variant, 451) * TAU,
        );
    } else if random(variant, 452) < 0.5 {
        // A crab scuttles back and forth.
        let walk = (time * 0.5 + random(variant, 453) * TAU).sin() * 0.22;
        let body = view.at(walk, 0.18);
        let step = (time * 8.0).sin();
        for leg in [-1.0f32, 1.0] {
            canvas.capsule(
                (body.0 + leg * 1.0, body.1 + 0.5),
                (body.0 + leg * 2.8, body.1 + 1.6 + step * 0.6 * leg),
                1.0,
                0.0,
                flat(0.9, [240, 110, 80]),
            );
        }
        canvas.ellipse(body, (2.2, 1.5), 0.6, flat(1.0, [240, 110, 80]));
    }
}

fn dunes(canvas: &mut Canvas<'_>, view: &TileView, variant: u32, time: f32) {
    let flip = if random(variant, 460) < 0.5 {
        1.0
    } else {
        -1.0
    };
    for index in 0..3 {
        let x = (-0.4 + 0.4 * index as f32) * flip + (random(variant, 461 + index) - 0.5) * 0.16;
        let y = -0.18 + 0.24 * index as f32;
        let c = view.at(x, y);
        let radii = (view.scaled(0.4), view.scaled(0.15));
        let shadow = (c.0 + view.scaled(0.13 * flip), c.1 + view.scaled(0.09));
        canvas.ellipse(c, radii, 0.0, move |px, py| {
            if sd_ellipse(px - shadow.0, py - shadow.1, radii.0, radii.1) < 0.0 {
                (0.32, [200, 160, 96])
            } else {
                (1.0, [244, 220, 150])
            }
        });
    }
    // Wind lifts a thin veil of sand off the crests.
    for index in 0..3 {
        let age = (time * 0.35 + random(variant, 470 + index)).rem_euclid(1.0);
        let start = view.at(-0.5 + age * 1.0, -0.3 + 0.28 * index as f32);
        canvas.capsule(
            start,
            (start.0 + 3.0, start.1 - 0.4),
            1.0,
            0.0,
            flat(0.7 * (1.0 - age), [240, 215, 150]),
        );
    }
}

fn mesa(canvas: &mut Canvas<'_>, view: &TileView, variant: u32) {
    let outline = view.outline();
    let rock = [200, 116, 74];
    for (index, (x, scale)) in [(-0.3f32, 0.75f32), (0.18, 1.0)].into_iter().enumerate() {
        let x = x + (random(variant, 480 + index as i32) - 0.5) * 0.1;
        let center = view.at(x, 0.08);
        let half = (view.scaled(0.3 * scale), view.scaled(0.3 * scale));
        let top_y = center.1 - half.1;
        let split = center.0;
        canvas.rect(center, half, outline, move |px, py| {
            let band = ((py - top_y) / 3.0).floor() as i32;
            let strata = if band.rem_euclid(3) == 0 { 0.12 } else { 0.0 };
            (if px < split { 0.95 } else { 0.4 } - strata, rock)
        });
        canvas.ellipse(
            (center.0, top_y),
            (half.0, view.scaled(0.09 * scale)),
            outline * 0.8,
            flat(1.0, [226, 150, 100]),
        );
    }
}

fn glacier(canvas: &mut Canvas<'_>, view: &TileView, variant: u32, time: f32) {
    let outline = view.outline();
    let ice = [180, 226, 250];
    let mut spires = [(-0.36f32, 0.55f32), (0.02, 0.85), (0.36, 0.6)];
    spires.sort_by(|a, b| a.1.total_cmp(&b.1));
    for (index, (x, height)) in spires.into_iter().enumerate() {
        let x = x + (random(variant, 490 + index as i32) - 0.5) * 0.1;
        let apex = view.at(x, 0.3 - height * 1.1);
        let left = view.at(x - 0.16 - 0.06 * height, 0.32);
        let right = view.at(x + 0.16 + 0.06 * height, 0.32);
        let foot = view.at(x + 0.02, 0.32);
        canvas.triangle(apex, left, foot, outline, flat(1.0, ice));
        canvas.triangle(apex, foot, right, outline, flat(0.5, ice));
    }
    let twinkle = (time * 3.0 + random(variant, 495) * TAU).sin();
    if twinkle > 0.5 {
        let glint = view.at(0.0, -0.45);
        canvas.capsule(
            (glint.0 - 2.0, glint.1),
            (glint.0 + 2.0, glint.1),
            1.0,
            0.0,
            flat(1.0, SNOW),
        );
        canvas.capsule(
            (glint.0, glint.1 - 2.0),
            (glint.0, glint.1 + 2.0),
            1.0,
            0.0,
            flat(1.0, SNOW),
        );
    }
}

fn geyser(
    canvas: &mut Canvas<'_>,
    view: &TileView,
    variant: u32,
    time: f32,
    smoke: &mut Vec<Smoke>,
) {
    let outline = view.outline();
    let mound = view.at(0.0, 0.18);
    let radii = (view.scaled(0.42), view.scaled(0.2));
    canvas.ellipse(
        mound,
        radii,
        outline,
        ellipsoid(mound, radii, 0.4, 0.95, [170, 150, 134]),
    );
    let vent = view.at(0.0, 0.14);
    canvas.ellipse(
        vent,
        (view.scaled(0.12), view.scaled(0.05)),
        0.0,
        flat(0.05, [30, 30, 30]),
    );
    let phase = random(variant, 500) * TAU;
    let burst = (time * 0.7 + phase).sin();
    if burst > 0.45 {
        let power = (burst - 0.45) / 0.55;
        let top = (
            vent.0 + (time * 5.0).sin() * 0.8,
            vent.1 - view.scaled(0.25 + 0.4 * power),
        );
        canvas.capsule(
            vent,
            top,
            view.stroke(0.07 + 0.05 * power),
            0.0,
            flat(1.0, [220, 240, 255]),
        );
        canvas.ellipse(
            (top.0, top.1),
            (view.scaled(0.08 * (1.0 + power)), view.scaled(0.06)),
            0.0,
            flat(0.9, [235, 245, 255]),
        );
    }
    smoke.push(Smoke {
        x: vent.0,
        y: vent.1 - view.scaled(0.05),
        radius: view.radius * 0.9,
        strength: 1.1 + 0.5 * burst.max(0.0),
        phase: random(variant, 501),
    });
}

fn reef(canvas: &mut Canvas<'_>, view: &TileView, variant: u32, time: f32) {
    for index in 0..3 {
        let (x, y) = slot(variant, index, 0.5);
        let c = view.at(x, y + 0.05);
        let r = view.scaled(0.1 + 0.07 * random(variant, 510 + index));
        canvas.ellipse(
            c,
            (r, r * 0.7),
            0.7,
            ellipsoid(c, (r, r * 0.7), 0.35, 1.0, [236, 124, 104]),
        );
        // Breakers fringe the coral.
        let pulse = 0.6 + 0.4 * (time * 1.4 + index as f32 * 2.0).sin();
        canvas.ellipse(
            (c.0, c.1 + r * 0.6),
            (r * 1.5, r * 0.4),
            0.0,
            flat(0.8 * pulse, [225, 240, 250]),
        );
    }
}

fn cave(canvas: &mut Canvas<'_>, view: &TileView, time: f32, variant: u32, kind: MapKind) {
    let outline = view.outline();
    let color = ground_color(Terrain::Mountain, kind);
    let mound = view.at(0.0, 0.22);
    let radii = (view.scaled(0.5), view.scaled(0.42));
    canvas.shape(
        (
            mound.0 - radii.0,
            mound.1 - radii.1,
            mound.0 + radii.0,
            mound.1,
        ),
        outline,
        |px, py| sd_ellipse(px - mound.0, py - mound.1, radii.0, radii.1).max(py - mound.1),
        ellipsoid(
            (mound.0, mound.1 - radii.1 * 0.2),
            (radii.0, radii.1 * 1.2),
            0.35,
            1.0,
            color,
        ),
    );
    let mouth = view.at(0.0, 0.2);
    canvas.shape(
        (
            mouth.0 - view.scaled(0.17),
            mouth.1 - view.scaled(0.2),
            mouth.0 + view.scaled(0.17),
            mouth.1,
        ),
        outline * 0.8,
        |px, py| {
            let arch = sd_ellipse(
                px - mouth.0,
                py - mouth.1,
                view.scaled(0.17),
                view.scaled(0.2),
            );
            arch.max(py - mouth.1)
        },
        flat(0.0, [0, 0, 0]),
    );
    // Something glints in the dark.
    let flicker = (time * 5.0 + random(variant, 520) * TAU).sin();
    if flicker > 0.6 {
        canvas.ellipse(
            (mouth.0, mouth.1 - view.scaled(0.07)),
            (1.0, 1.0),
            0.0,
            flat(1.0, FLAME),
        );
    }
}

fn shrine(canvas: &mut Canvas<'_>, view: &TileView, time: f32, variant: u32) {
    let outline = view.outline();
    let plinth = view.at(0.0, 0.26);
    canvas.rect(
        plinth,
        (view.scaled(0.3), view.scaled(0.07)),
        outline,
        flat(0.8, STONE),
    );
    let pillar_top = view.at(0.0, -0.42);
    let base_left = view.at(-0.09, 0.2);
    let base_right = view.at(0.09, 0.2);
    let center_x = pillar_top.0;
    canvas.triangle(pillar_top, base_left, base_right, outline, move |px, _| {
        (if px < center_x { 1.0 } else { 0.55 }, STONE)
    });
    for side in [-1.0f32, 1.0] {
        let stone = view.at(0.3 * side, 0.18);
        canvas.rect(
            stone,
            (view.scaled(0.05), view.scaled(0.13)),
            outline * 0.8,
            flat(0.75, STONE),
        );
    }
    let pulse = 0.65 + 0.35 * (time * 2.4 + random(variant, 530) * TAU).sin();
    canvas.ellipse(
        (pillar_top.0, pillar_top.1 - view.scaled(0.07)),
        (view.scaled(0.06), view.scaled(0.06)),
        0.0,
        flat(pulse, [120, 220, 255]),
    );
}

fn mine(canvas: &mut Canvas<'_>, view: &TileView, time: f32, variant: u32) {
    let outline = view.outline();
    let mouth = view.at(0.0, 0.12);
    canvas.rect(
        mouth,
        (view.scaled(0.2), view.scaled(0.2)),
        outline,
        flat(0.0, [0, 0, 0]),
    );
    for side in [-1.0f32, 1.0] {
        canvas.capsule(
            view.at(0.2 * side, 0.34),
            view.at(0.2 * side, -0.1),
            view.stroke(0.06),
            outline * 0.7,
            flat(0.8, WOOD),
        );
    }
    canvas.capsule(
        view.at(-0.27, -0.1),
        view.at(0.27, -0.1),
        view.stroke(0.065),
        outline * 0.7,
        flat(0.85, WOOD),
    );
    for offset in [-0.06f32, 0.06] {
        canvas.capsule(
            view.at(offset, 0.2),
            view.at(offset * 2.2, 0.42),
            1.0,
            0.0,
            flat(0.7, [160, 160, 170]),
        );
    }
    // An ore cart whose gems catch the light.
    let cart = view.at(0.36, 0.36);
    canvas.rect(
        cart,
        (view.scaled(0.11), view.scaled(0.06)),
        outline * 0.7,
        flat(0.7, [150, 150, 160]),
    );
    let glint = (time * 4.0 + random(variant, 540) * TAU).sin();
    canvas.ellipse(
        (cart.0, cart.1 - view.scaled(0.07)),
        (1.4, 1.2),
        0.0,
        flat(if glint > 0.3 { 1.0 } else { 0.55 }, [120, 230, 255]),
    );
}

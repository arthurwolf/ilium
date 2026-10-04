//! Hex expedition: an endless sea of procedurally generated expedition
//! islands, drawn as pointy-top hex tiles that the camera slowly pans across,
//! in the manner of the maps of adventure board games. Each island has its
//! coast, biomes, one moored ship, one distant goal and scattered villages,
//! camps, ruins, caves, mines and shrines (see `world`). Several map types
//! (jungle, savanna, desert, arctic, volcanic) exist; each is revealed in the
//! next, hex by hex. Tiles are animated: waves and foam, swaying palms and
//! pines, rising smoke, flickering fires, glowing lava, falling snow.
//!
//! The art is not bitmaps: every tile is drawn from shaded vector shapes
//! (see `sprites`), emitted as tone so the host's own dither turns it into
//! Braille. Colour per terminal cell follows the biome when tinting is on.
//!
//! Determinism: every frame is a pure function of (`Frame::time`, settings).
//! Nothing reads the clock, blocks or spawns threads; the only state is
//! reusable scratch memory.
//!
//! No external data source is used.

mod canvas;
mod overlay;
mod settings;
mod sprites;
#[cfg(test)]
mod tests;
mod world;

pub use settings::HexExpeditionSettings;

use crate::control::SceneSettings;
use crate::raster::smoothstep;
use crate::scene::{Frame, Scene, SceneEnv};
use crate::style::ScenePalette;
use canvas::{Canvas, Rgb};
use settings::{MapChoice, PanStyle};
use sprites::{Smoke, TileView};
use world::{hash_cell, unit, Atlas, MapKind, Terrain, Tile, World, SQRT3};

/// The expedition maps follow the look of Curious Expedition by
/// Maschinen-Mensch; all art here is drawn by Ilium.
pub const INSPIRED_BY: &[&str] =
    &["https://store.steampowered.com/app/358130/The_Curious_Expedition/"];

/// Tiles per second the camera travels at 100 % pan speed.
const TILES_PER_SECOND: f64 = 0.32;
/// Seconds the reveal of the next map takes.
const TRANSITION_SECONDS: f64 = 14.0;
/// Width of the fog band at the reveal front, as a fraction of the screen.
const FOG_BAND: f32 = 0.18;
/// Frames per second of stepped tile animation.
const STEPPED_FPS: f32 = 6.0;
/// The camera wraps within +-half this many dots so tile coordinates always
/// fit an i32. At the fastest pan that takes months of continuous running; the
/// world is endless, so the wrap only shows as one new stretch of map.
const CAMERA_WRAP_DOTS: f64 = 1.0e9;

pub struct HexExpeditionScene {
    settings: HexExpeditionSettings,
    /// One colour per dot, reduced to cells at the end of each frame.
    dot_colors: Vec<Rgb>,
    tiles: Vec<Tile>,
    smoke: Vec<Smoke>,
    current_kind: MapKind,
    atlas: Atlas,
    /// The shared look's palette; biome colours are mapped onto it when provided.
    palette: ScenePalette,
}

/// Which maps are on screen this frame and how far the reveal has got.
#[derive(Debug, Clone, Copy)]
struct MapPlan {
    current: World,
    next: World,
    /// 0 when no reveal is under way, else 0..1.
    reveal: f32,
}

#[derive(Debug, Clone, Copy)]
struct Camera {
    x: f64,
    y: f64,
}

/// A rectangle of axial tile coordinates with a one-tile margin, so every
/// visible tile has all six neighbours available.
struct TileWindow {
    q_min: i32,
    r_min: i32,
    columns: usize,
    rows: usize,
}

impl TileWindow {
    fn index(&self, q: i32, r: i32) -> Option<usize> {
        let column = usize::try_from(q - self.q_min).ok()?;
        let row = usize::try_from(r - self.r_min).ok()?;
        (column < self.columns && row < self.rows).then_some(row * self.columns + column)
    }
}

impl HexExpeditionScene {
    // PALETTE (future plugin contract): `env.palette` is the shared look's current
    // palette. When animations become plugins, the plugin constructor receives the
    // current palette and MUST follow it: scenes with natural colours shift them
    // onto it (`ScenePalette::recolor`/`at`), and `Scene::set_palette` delivers later
    // changes. This scene follows it natively: every dot's biome colour is mapped
    // onto the palette by lightness before the cell reduction (`follows_palette`),
    // so `PaletteScene` skips its generic recolour.
    pub fn new(settings: &HexExpeditionSettings, env: &SceneEnv) -> Self {
        Self {
            settings: settings.normalized(),
            dot_colors: Vec::new(),
            tiles: Vec::new(),
            smoke: Vec::new(),
            current_kind: MapKind::Jungle,
            atlas: Atlas::default(),
            palette: env.palette.clone(),
        }
    }

    fn radius(&self) -> f32 {
        self.settings.tile_size as f32
    }

    /// Camera position in dots. Kept in f64: after days of uptime the
    /// travelled distance would otherwise cost sub-dot precision.
    fn camera(&self, seconds: f64) -> Camera {
        let radius = f64::from(self.radius());
        let speed = TILES_PER_SECOND * f64::from(self.settings.pan_speed) / 100.0
            * radius
            * f64::from(SQRT3);
        let travelled = seconds * speed;
        let seed = f64::from(self.settings.seed);
        let origin = (seed * 97.0 % 4000.0, seed * 53.0 % 4000.0);
        let (heading_x, heading_y) = match self.settings.pan_style {
            PanStyle::Wander => (0.93, 0.36),
            PanStyle::East => (1.0, 0.0),
            PanStyle::NorthEast => (0.87, -0.5),
            PanStyle::South => (0.0, 1.0),
        };
        let mut camera = Camera {
            x: origin.0 + travelled * heading_x,
            y: origin.1 + travelled * heading_y,
        };
        if self.settings.pan_style == PanStyle::Wander && self.settings.pan_speed > 0 {
            // A slow lateral weave, scaled by speed so a slow pan does not
            // swing wildly.
            let weave = radius * f64::from(SQRT3) * 3.2;
            let rate = f64::from(self.settings.pan_speed) / 100.0;
            camera.x += weave * (seconds * 0.021 * rate + seed).sin() * -0.35;
            camera.y += weave * (seconds * 0.017 * rate + seed * 0.7).sin();
        }
        let wrap = |value: f64| {
            (value + CAMERA_WRAP_DOTS / 2.0).rem_euclid(CAMERA_WRAP_DOTS) - CAMERA_WRAP_DOTS / 2.0
        };
        Camera {
            x: wrap(camera.x),
            y: wrap(camera.y),
        }
    }

    fn map_plan(&self, seconds: f64) -> MapPlan {
        let landmark_scale = self.settings.landmarks as f32 / 100.0;
        let seed = self.settings.seed;
        let world_for = |kind: MapKind, epoch: u64| {
            World::new(
                kind,
                seed.wrapping_mul(131)
                    .wrapping_add((epoch as u32).wrapping_mul(7_919)),
                landmark_scale,
            )
        };
        let fixed = match self.settings.map {
            MapChoice::Cycle => None,
            MapChoice::Jungle => Some(MapKind::Jungle),
            MapChoice::Savanna => Some(MapKind::Savanna),
            MapChoice::Desert => Some(MapKind::Desert),
            MapChoice::Arctic => Some(MapKind::Arctic),
            MapChoice::Volcanic => Some(MapKind::Volcanic),
        };
        if let Some(kind) = fixed {
            let world = world_for(kind, 0);
            return MapPlan {
                current: world,
                next: world,
                reveal: 0.0,
            };
        }
        let dwell = f64::from(self.settings.map_seconds);
        let epoch = (seconds / dwell).floor().max(0.0) as u64;
        let into = seconds - epoch as f64 * dwell;
        let transition = TRANSITION_SECONDS.min(dwell * 0.4);
        let reveal = if into > dwell - transition {
            ((into - (dwell - transition)) / transition) as f32
        } else {
            0.0
        };
        let kind_of = |epoch: u64| MapKind::ALL[(epoch % MapKind::ALL.len() as u64) as usize];
        MapPlan {
            current: world_for(kind_of(epoch), epoch),
            next: world_for(kind_of(epoch + 1), epoch + 1),
            reveal,
        }
    }

    fn tile_time(&self, seconds: f64) -> f32 {
        let scaled = seconds * f64::from(self.settings.animation_speed) / 100.0;
        let time = if self.settings.stepped {
            (scaled * f64::from(STEPPED_FPS)).floor() / f64::from(STEPPED_FPS)
        } else {
            scaled
        };
        // Sprites feed this into sin(); wrapping keeps f32 precision after
        // days of uptime. 4096 s is not a period of any sprite, so a wrap is
        // one imperceptible phase change every 68 minutes.
        (time % 4096.0) as f32
    }
}

/// Which world a tile belongs to while the next map is being revealed, and
/// how foggy it is. `screen_fraction` is the tile centre's x over the screen.
fn world_at(plan: &MapPlan, screen_fraction: f32) -> (World, f32) {
    if plan.reveal <= 0.0 {
        return (plan.current, 0.0);
    }
    let front = plan.reveal * (1.0 + FOG_BAND);
    if screen_fraction < front - FOG_BAND {
        (plan.next, 0.0)
    } else if screen_fraction < front {
        let depth = (front - screen_fraction) / FOG_BAND;
        (plan.next, 1.0 - depth)
    } else {
        (plan.current, 0.0)
    }
}

impl Scene for HexExpeditionScene {
    fn render(&mut self, frame: &mut Frame<'_>) {
        let width = frame.raster.width;
        let height = frame.raster.height;
        if width == 0 || height == 0 {
            return;
        }
        let seconds = frame.time.as_secs_f64();
        let time = self.tile_time(seconds);
        let radius = self.radius();
        let camera = self.camera(seconds);
        let plan = self.map_plan(seconds);

        // Tile window: every tile whose hex may touch the screen, plus a
        // margin for neighbours and for tall sprites rising from below.
        let row_height = f64::from(radius) * 1.5;
        let column_width = f64::from(radius) * f64::from(SQRT3);
        let r_min = ((camera.y - f64::from(radius) * 3.0) / row_height).floor() as i32 - 1;
        let r_max =
            ((camera.y + height as f64 + f64::from(radius) * 2.0) / row_height).ceil() as i32 + 1;
        let first_q = |r: i32| {
            ((camera.x - column_width * 2.0) / column_width - f64::from(r) * 0.5).floor() as i32
        };
        let q_min = first_q(r_min).min(first_q(r_max)) - 1;
        let columns = (width as f64 / column_width).ceil() as usize
            + 6
            + (r_max - r_min).unsigned_abs() as usize / 2;
        let window = TileWindow {
            q_min,
            r_min,
            columns,
            rows: usize::try_from(r_max - r_min + 1).unwrap_or(0),
        };

        let fog_at = |q: i32, r: i32| -> (World, f32) {
            let x = column_width * (f64::from(q) + f64::from(r) * 0.5) - camera.x;
            world_at(&plan, (x / width as f64) as f32)
        };
        self.tiles.clear();
        let mut fogs = Vec::with_capacity(window.columns * window.rows);
        for row in 0..window.rows {
            for column in 0..window.columns {
                let q = window.q_min + column as i32;
                let r = window.r_min + row as i32;
                let (world, fog) = fog_at(q, r);
                self.tiles.push(self.atlas.tile(&world, q, r));
                fogs.push((fog, world.kind));
            }
        }

        self.dot_colors.clear();
        self.dot_colors.resize(width * height, [0, 0, 0]);
        let cell_columns = usize::from(frame.width);
        let cell_rows = usize::from(frame.height);
        let mut canvas = Canvas::new(width, height, &mut frame.raster.dots, &mut self.dot_colors);

        let view_of = |q: i32, r: i32| TileView {
            cx: (column_width * (f64::from(q) + f64::from(r) * 0.5) - camera.x) as f32,
            cy: (row_height * f64::from(r) - camera.y) as f32,
            radius,
        };
        let on_screen = |view: &TileView, up: f32, down: f32| {
            view.cx > -radius * 2.0
                && view.cx < width as f32 + radius * 2.0
                && view.cy > -radius * up
                && view.cy < height as f32 + radius * down
        };

        // Ground first, so tall sprites of one row may rise over the ground
        // of the row above.
        for row in 1..window.rows.saturating_sub(1) {
            for column in 1..window.columns.saturating_sub(1) {
                let q = window.q_min + column as i32;
                let r = window.r_min + row as i32;
                let view = view_of(q, r);
                if !on_screen(&view, 1.6, 1.6) {
                    continue;
                }
                let index = row * window.columns + column;
                let tile = &self.tiles[index];
                let neighbors = world::NEIGHBORS.map(|(dq, dr)| {
                    window
                        .index(q + dq, r + dr)
                        .map_or(Terrain::Water, |slot| self.tiles[slot].terrain)
                });
                sprites::paint_ground(&mut canvas, tile, &view, &neighbors, fogs[index].1, time);
            }
        }

        self.smoke.clear();
        for row in 1..window.rows.saturating_sub(1) {
            for column in 1..window.columns.saturating_sub(1) {
                let q = window.q_min + column as i32;
                let r = window.r_min + row as i32;
                let view = view_of(q, r);
                // Sprites rise above their tile, so rows just below the
                // screen still matter.
                if !on_screen(&view, 1.6, 2.6) {
                    continue;
                }
                let index = row * window.columns + column;
                let tile = self.tiles[index];
                let kind = fogs[index].1;
                sprites::paint_decor(&mut canvas, &tile, &view, kind, time, &mut self.smoke);
                sprites::paint_feature(&mut canvas, &tile, &view, kind, time, &mut self.smoke);
            }
        }
        sprites::paint_smoke(&mut canvas, &self.smoke, time);

        let dominant = if plan.reveal > 0.5 {
            plan.next.kind
        } else {
            plan.current.kind
        };
        self.current_kind = dominant;
        if self.settings.cloud_shadows {
            overlay::paint_cloud_shadows(
                &mut canvas,
                seconds,
                camera.x,
                camera.y,
                radius,
                self.settings.seed,
            );
        }
        if self.settings.weather {
            overlay::paint_weather(&mut canvas, dominant, seconds, radius);
        }
        paint_fog(&mut canvas, &window, &fogs, &view_of, radius);

        let gain = self.settings.brightness as f32 / 100.0;
        if (gain - 1.0).abs() > f32::EPSILON {
            for y in 0..height {
                for x in 0..width {
                    let tone = canvas.tone_at(x, y);
                    canvas.scale_tone_at(x, y, gain.min(1.0 / tone.max(0.001)));
                }
            }
        }
        if self.settings.tint {
            if self.palette.is_provided() {
                canvas.map_colors(|color| self.palette.recolor(color));
            }
            canvas.reduce_to_cells(cell_columns, cell_rows, frame.cell_colors);
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
        15
    }

    fn status(&self) -> Option<String> {
        Some(format!("{} map", self.current_kind.label()))
    }
}

/// Dims the tiles in the reveal band so the next map seems to emerge from
/// fog, with a few bright motes drifting in it.
fn paint_fog(
    canvas: &mut Canvas<'_>,
    window: &TileWindow,
    fogs: &[(f32, MapKind)],
    view_of: &impl Fn(i32, i32) -> TileView,
    radius: f32,
) {
    for row in 0..window.rows {
        for column in 0..window.columns {
            let fog = fogs[row * window.columns + column].0;
            if fog <= 0.0 {
                continue;
            }
            let q = window.q_min + column as i32;
            let r = window.r_min + row as i32;
            let view = view_of(q, r);
            let left = (view.cx - radius).floor().max(0.0) as usize;
            let top = (view.cy - radius).floor().max(0.0) as usize;
            let right = ((view.cx + radius).ceil().max(0.0) as usize).min(canvas.width());
            let bottom = ((view.cy + radius).ceil().max(0.0) as usize).min(canvas.height());
            for y in top..bottom {
                for x in left..right {
                    let hashed = unit(hash_cell(x as i32, y as i32, 0xf09));
                    let veil = smoothstep(0.0, 1.0, fog);
                    if hashed < veil * 0.8 {
                        canvas.scale_tone_at(x, y, 0.25);
                    }
                }
            }
        }
    }
}

//! Topographic maps: contour-line maps of real worlds drawn in Braille dots,
//! one dot per contour crossing, with the view panning or turning slowly over
//! the surface. Measured elevation comes from public datasets (NOAA ETOPO 2022
//! for Earth, NASA LOLA for the Moon, MOLA for Mars, Magellan for Venus,
//! MESSENGER for Mercury, Dawn for Ceres; see `assets/topography/manifest.json`
//! and `build_assets.py`). Fictional worlds are generated from a seed.
//!
//! Determinism: every frame is a pure function of (`Frame::time`, settings,
//! loaded elevation). Loading and generating a world runs on an owned worker;
//! `render` keeps drawing the previous world until the next one is ready, then
//! dissolves to it.

pub(crate) mod data;
mod palette;
mod projection;
pub(crate) mod settings;
#[cfg(test)]
mod tests;

pub use settings::TopographicMapsSettings;

use crate::control::SceneSettings;
use crate::registry::AmbientSettings;
use crate::scene::{Frame, Scene, SceneEnv};
use crate::source::Worker;
use crate::style::ScenePalette;
use data::Heightfield;
use projection::Camera;
use settings::{BelowStyle, PaletteChoice, PanStyle, Projection, WorldId};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::Arc;

/// Seconds the dissolve to the next world takes.
const DISSOLVE_SECONDS: f64 = 4.0;
/// Dots per second the view travels at 100 % pan speed.
const DOTS_PER_SECOND: f64 = 5.0;
const NO_LEVEL: i32 = i32::MIN;

struct Loaded {
    id: WorldId,
    field: Arc<Heightfield>,
    interval_m: f32,
}

struct Loading {
    id: WorldId,
    receiver: Receiver<Result<Heightfield, String>>,
    _worker: Worker,
}

pub struct TopographicMapsScene {
    settings: TopographicMapsSettings,
    worlds: Vec<WorldId>,
    current: Option<Loaded>,
    /// The world being dissolved away and the animation time it started at.
    previous: Option<(Loaded, f64)>,
    loading: Option<Loading>,
    failed: Option<(WorldId, String)>,
    heights: Vec<f32>,
    levels: Vec<i32>,
    from_previous: Vec<bool>,
    last_zero_m: f32,
    palette: ScenePalette,
}

/// 1, 2, 2.5 and 5 times a power of ten: the steps of a survey map.
fn nice_interval(raw: f32) -> f32 {
    let raw = raw.max(5.0);
    let magnitude = 10_f32.powf(raw.log10().floor());
    let scaled = raw / magnitude;
    let step = [1.0, 2.0, 2.5, 5.0, 10.0]
        .into_iter()
        .min_by(|a, b| (a - scaled).abs().total_cmp(&(b - scaled).abs()))
        .unwrap_or(1.0);
    step * magnitude
}

fn bounce(value: f64, low: f64, high: f64) -> f64 {
    let range = high - low;
    let folded = (value - low).rem_euclid(2.0 * range);
    low + if folded > range {
        2.0 * range - folded
    } else {
        folded
    }
}

fn dot_hash(x: usize, y: usize) -> f32 {
    let mut hash = (x as u32).wrapping_mul(0x9e37_79b1) ^ (y as u32).wrapping_mul(0x85eb_ca6b);
    hash ^= hash >> 13;
    hash = hash.wrapping_mul(0xc2b2_ae35);
    hash ^= hash >> 16;
    (hash >> 8) as f32 / 16_777_215.0
}

impl TopographicMapsScene {
    // PALETTE (future plugin contract): `env.palette` is the shared look's current
    // palette. When animations become plugins, the plugin constructor receives the
    // current palette and MUST follow it: scenes with natural colours shift them
    // onto it (`ScenePalette::recolor`/`at`), and `Scene::set_palette` delivers later
    // changes. This scene follows it natively: its elevation tints are
    // recoloured by brightness (`follows_palette`), so `PaletteScene` skips its
    // generic remap.
    pub fn new(settings: &TopographicMapsSettings, env: &SceneEnv) -> Self {
        let settings = settings.normalized();
        Self {
            worlds: settings.body.worlds(),
            settings,
            current: None,
            previous: None,
            loading: None,
            failed: None,
            heights: Vec::new(),
            levels: Vec::new(),
            from_previous: Vec::new(),
            last_zero_m: 0.0,
            palette: env.palette.clone(),
        }
    }

    fn interval_for(&self, field: &Heightfield) -> f32 {
        if self.settings.interval_m > 0 {
            self.settings.interval_m as f32
        } else {
            nice_interval((field.max_m - field.min_m) / self.settings.contour_levels as f32)
        }
    }

    fn wrap(&self, id: WorldId, field: Heightfield) -> Loaded {
        let interval_m = self.interval_for(&field);
        Loaded {
            id,
            field: Arc::new(field),
            interval_m,
        }
    }

    fn desired_world(&self, seconds: f64) -> WorldId {
        let slot = (seconds / f64::from(self.settings.body_seconds)) as usize;
        self.worlds[slot % self.worlds.len()]
    }

    /// Keep `current` at the world the schedule wants without ever waiting on
    /// the worker. Until the first world arrives the scene draws nothing and
    /// its status says it is loading.
    fn advance_loading(&mut self, seconds: f64) {
        if let Some(loading) = &self.loading {
            match loading.receiver.try_recv() {
                Ok(Ok(field)) => {
                    let id = loading.id;
                    self.loading = None;
                    let loaded = self.wrap(id, field);
                    if let Some(old) = self.current.replace(loaded) {
                        self.previous = Some((old, seconds));
                    }
                }
                Ok(Err(message)) => {
                    self.failed = Some((loading.id, message));
                    self.loading = None;
                }
                Err(TryRecvError::Disconnected) => {
                    self.failed = Some((loading.id, "world loader stopped".to_owned()));
                    self.loading = None;
                }
                Err(TryRecvError::Empty) => {}
            }
        }
        if self
            .previous
            .as_ref()
            .is_some_and(|(_, started)| seconds - started >= DISSOLVE_SECONDS)
        {
            self.previous = None;
        }
        let desired = self.desired_world(seconds);
        if self
            .current
            .as_ref()
            .is_some_and(|loaded| loaded.id == desired)
            || self.loading.is_some()
            || self.failed.as_ref().is_some_and(|(id, _)| *id == desired)
        {
            return;
        }
        let (sender, receiver) = std::sync::mpsc::channel();
        let seed = self.settings.fictional_seed;
        let worker = Worker::spawn("topography", move |stop| {
            if let Some(result) = data::load_world(desired, seed, &stop) {
                let _ = sender.send(result);
            }
        });
        self.loading = Some(Loading {
            id: desired,
            receiver,
            _worker: worker,
        });
    }

    fn camera(&self, seconds: f64, width: usize, height: usize) -> Camera {
        let settings = &self.settings;
        let (width, height) = (width as f64, height as f64);
        let dots_per_degree = Camera::fit_scale(settings.projection, width, height)
            * f64::from(settings.zoom_percent)
            / 100.0;
        let travelled = DOTS_PER_SECOND * f64::from(settings.pan_speed) / 100.0 * seconds
            / dots_per_degree.max(1e-9);
        let start = (
            f64::from(settings.start_longitude),
            f64::from(settings.center_latitude),
        );
        let (mut longitude, mut latitude) = start;
        match settings.pan {
            PanStyle::Still => {}
            PanStyle::East => longitude += travelled,
            PanStyle::West => longitude -= travelled,
            PanStyle::North => latitude = bounce(start.1 + travelled, -75.0, 75.0),
            PanStyle::South => latitude = bounce(start.1 - travelled, -75.0, 75.0),
            PanStyle::NorthEast => {
                longitude += travelled * 0.8;
                latitude = bounce(start.1 + travelled * 0.4, -75.0, 75.0);
            }
            PanStyle::Wander => {
                let phase = travelled / 55.0;
                longitude += travelled * (1.0 + 0.3 * (phase * 0.6).sin());
                latitude = start.1 + 28.0 * (phase * 0.7 + 0.5).sin();
            }
        }
        let latitude_limit = match settings.projection {
            Projection::Flat => {
                let half_view = height * 0.5 / dots_per_degree;
                if half_view >= 90.0 {
                    0.0
                } else {
                    90.0 - half_view
                }
            }
            Projection::Globe => 80.0,
        };
        Camera {
            projection: settings.projection,
            longitude,
            latitude: latitude.clamp(-latitude_limit, latitude_limit),
            dots_per_degree,
            width,
            height,
        }
    }

    fn zero_level(&self, seconds: f64) -> f32 {
        let tide = if self.settings.tide_range_m > 0 {
            0.5 * self.settings.tide_range_m as f64
                * (std::f64::consts::TAU * seconds / f64::from(self.settings.tide_seconds)).sin()
        } else {
            0.0
        };
        self.settings.zero_shift_m as f32 + tide as f32
    }

    /// Fill `heights`, `levels` and `from_previous` for every dot.
    fn sample_dots(&mut self, camera: &Camera, seconds: f64, zero: f32) {
        let (width, height) = (camera.width as usize, camera.height as usize);
        let count = width * height;
        self.heights.clear();
        self.heights.resize(count, f32::NAN);
        self.levels.clear();
        self.levels.resize(count, NO_LEVEL);
        self.from_previous.clear();
        self.from_previous.resize(count, false);
        let Some(current) = &self.current else { return };
        let dissolve = self.previous.as_ref().map(|(old, started)| {
            let progress = ((seconds - started) / DISSOLVE_SECONDS).clamp(0.0, 1.0) as f32;
            (old, progress * progress * (3.0 - 2.0 * progress))
        });
        for y in 0..height {
            for x in 0..width {
                let Some((longitude, latitude)) = camera.locate(x as f64, y as f64) else {
                    continue;
                };
                let (loaded, old) = match dissolve {
                    Some((old, progress)) if dot_hash(x, y) >= progress => (old, true),
                    _ => (current, false),
                };
                let elevation = loaded.field.sample(longitude, latitude);
                let index = y * width + x;
                self.heights[index] = elevation;
                self.levels[index] = ((elevation - zero) / loaded.interval_m).floor() as i32;
                self.from_previous[index] = old;
            }
        }
    }

    fn draw_contours(&self, raster: &mut crate::raster::Raster) {
        let settings = &self.settings;
        let width = raster.width;
        let index_every = settings.index_every as i32;
        let put = |raster: &mut crate::raster::Raster, x: usize, y: usize, value: f32| {
            if x < raster.width && y < raster.height {
                let slot = &mut raster.dots[y * raster.width + x];
                *slot = slot.max(value);
            }
        };
        for y in 0..raster.height {
            for x in 0..width {
                let here = y * width + x;
                let level = self.levels[here];
                if level == NO_LEVEL {
                    continue;
                }
                for (nx, ny) in [(x + 1, y), (x, y + 1)] {
                    if nx >= width || ny >= raster.height {
                        continue;
                    }
                    let there = ny * width + nx;
                    let other = self.levels[there];
                    if other == NO_LEVEL {
                        if settings.outline {
                            put(raster, x, y, 0.7);
                        }
                        continue;
                    }
                    if other == level || self.from_previous[here] != self.from_previous[there] {
                        continue;
                    }
                    let (low, high) = (level.min(other), level.max(other));
                    let is_zero = settings.coastline && low < 0 && high >= 0;
                    let is_index = index_every > 0
                        && high.div_euclid(index_every) > low.div_euclid(index_every);
                    let below = high <= 0;
                    if below && !is_zero {
                        match settings.below_style {
                            BelowStyle::Hidden => continue,
                            BelowStyle::Dotted if (x + y) % 2 == 1 => continue,
                            _ => {}
                        }
                    }
                    let emphasis = u32::from(is_zero || is_index);
                    let thickness = (settings.line_thickness + emphasis).min(3);
                    let value = if emphasis == 1 { 1.0 } else { 0.8 };
                    put(raster, x, y, value);
                    if thickness >= 2 {
                        put(raster, x + 1, y, value);
                    }
                    if thickness >= 3 {
                        put(raster, x, y + 1, value);
                    }
                }
            }
        }
    }

    /// Sparse dots on slopes lit from the north-west.
    fn shade(&self, raster: &mut crate::raster::Raster) {
        let width = raster.width;
        let strength = self.settings.shading_percent as f32 / 100.0 * 0.55;
        for y in 1..raster.height.saturating_sub(1) {
            for x in 1..width.saturating_sub(1) {
                let here = y * width + x;
                if raster.dots[here] > 0.0 || self.levels[here] == NO_LEVEL {
                    continue;
                }
                let (north_west, south_east) = (
                    self.heights[here - width - 1],
                    self.heights[here + width + 1],
                );
                if north_west.is_nan() || south_east.is_nan() {
                    continue;
                }
                let interval = self
                    .current
                    .as_ref()
                    .map_or(100.0, |loaded| loaded.interval_m);
                let lit = ((south_east - north_west) / (interval * 2.5)).clamp(0.0, 1.0);
                raster.dots[here] = lit * strength;
            }
        }
    }

    fn paint_cells(&self, frame: &mut Frame<'_>) {
        let (columns, rows) = (usize::from(frame.width), usize::from(frame.height));
        frame.cell_colors.clear();
        frame
            .cell_colors
            .resize(columns * rows, self.palette.recolor([70, 76, 90]));
        let (Some(current), width) = (&self.current, frame.raster.width) else {
            return;
        };
        for row in 0..rows {
            for column in 0..columns {
                let dot = (row * 4 + 2) * width + column * 2 + 1;
                let Some(&elevation) = self.heights.get(dot) else {
                    continue;
                };
                if elevation.is_nan() {
                    continue;
                }
                let loaded = match (&self.previous, self.from_previous.get(dot)) {
                    (Some((old, _)), Some(true)) => old,
                    _ => current,
                };
                frame.cell_colors[row * columns + column] = self.palette.recolor(palette::tint(
                    self.settings.palette,
                    loaded.id,
                    elevation - self.last_zero_m,
                    loaded.field.min_m,
                    loaded.field.max_m,
                ));
            }
        }
    }
}

impl Scene for TopographicMapsScene {
    fn render(&mut self, frame: &mut Frame<'_>) {
        frame.raster.dots.fill(0.0);
        let seconds = frame.time.as_secs_f64();
        self.advance_loading(seconds);
        if frame.raster.width < 4 || frame.raster.height < 4 {
            return;
        }
        let camera = self.camera(seconds, frame.raster.width, frame.raster.height);
        let zero = self.zero_level(seconds);
        self.last_zero_m = zero;
        self.sample_dots(&camera, seconds, zero);
        self.draw_contours(frame.raster);
        if self.settings.shading_percent > 0 {
            self.shade(frame.raster);
        }
        if self.settings.palette != PaletteChoice::Global {
            self.paint_cells(frame);
        }
    }

    fn uses_cell_colors(&self) -> bool {
        self.settings.palette != PaletteChoice::Global
    }

    fn set_palette(&mut self, palette: &ScenePalette) {
        // Tints are computed per frame, so nothing is cached.
        self.palette = palette.clone();
    }

    fn follows_palette(&self) -> bool {
        true
    }

    fn frames_per_second(&self) -> u32 {
        12
    }

    /// Everything except the world itself can change in place, so dragging a
    /// slider never reloads or regenerates elevation.
    fn reconfigure(&mut self, settings: &AmbientSettings) -> bool {
        let next = settings.topographic_maps.normalized();
        if next.body != self.settings.body || next.fictional_seed != self.settings.fictional_seed {
            return false;
        }
        self.settings = next;
        let current_interval = self
            .current
            .as_ref()
            .map(|loaded| self.interval_for(&loaded.field));
        let previous_interval = self
            .previous
            .as_ref()
            .map(|(loaded, _)| self.interval_for(&loaded.field));
        if let (Some(loaded), Some(interval)) = (self.current.as_mut(), current_interval) {
            loaded.interval_m = interval;
        }
        if let (Some((loaded, _)), Some(interval)) = (self.previous.as_mut(), previous_interval) {
            loaded.interval_m = interval;
        }
        true
    }

    fn status(&self) -> Option<String> {
        if let Some((_, message)) = &self.failed {
            if self.current.is_none() {
                return Some(format!("No elevation data: {message}"));
            }
        }
        let Some(loaded) = self.current.as_ref() else {
            return self
                .loading
                .as_ref()
                .map(|next| format!("Loading {}…", next.id.label()));
        };
        let loading = self.loading.as_ref().map_or(String::new(), |next| {
            format!(" — loading {}", next.id.label())
        });
        Some(format!(
            "{} — {} m contours, relief {:.1} to {:.1} km{loading}",
            loaded.field.name,
            loaded.interval_m,
            loaded.field.min_m / 1000.0,
            loaded.field.max_m / 1000.0,
        ))
    }
}

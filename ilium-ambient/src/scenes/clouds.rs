//! Live weather-satellite clouds, global or over the shared location, as the
//! latest image or as a looping time-lapse of the last hours.
//!
//! Data sources, licences and the reasons for each choice are documented in
//! `clouds/providers.rs`. All network work happens on an owned worker thread
//! (`clouds/worker.rs`); `render` only resamples the frames it already holds.

pub(crate) mod providers;
mod worker;

pub use self::providers::CloudSource;

use self::providers::choose_source;
use self::worker::{CloudFrame, FrameSet, Job, Update};
use crate::control::{self, Control, ControlValue, SceneSettings};
use crate::location::GeoLocation;
use crate::raster::smoothstep;
use crate::resources::{AmbientResources, WorkerCost};
use crate::scene::{Frame, Scene, SceneEnv};
use crate::scenes::night_lights::projection::{self, DotTable, Projection, View};
use crate::scenes::night_lights::tiles::{system_clock, GeoBox, HttpFetcher, NowFn, TileFetcher};
use crate::source::Worker;
use crate::style::ScenePalette;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Coverage {
    /// A region around the shared location, from the best satellite for it.
    #[default]
    Local,
    /// The whole Earth.
    Global,
}

impl Coverage {
    const LABELS: [&'static str; 2] = ["Around my location", "Whole Earth"];

    fn index(self) -> usize {
        match self {
            Self::Local => 0,
            Self::Global => 1,
        }
    }

    fn from_index(index: usize) -> Self {
        if index == 1 {
            Self::Global
        } else {
            Self::Local
        }
    }
}

/// History choices in hours; the setting holds one of these.
const HISTORY_HOURS: [i32; 4] = [0, 6, 12, 24];
const HISTORY_LABELS: [&str; 4] = [
    "Latest image only",
    "Last 6 hours",
    "Last 12 hours",
    "Last 24 hours",
];

fn snap_history(hours: i32) -> i32 {
    *HISTORY_HOURS
        .iter()
        .min_by_key(|choice| (**choice - hours).abs())
        .unwrap_or(&0)
}

/// Settings of the "Satellite clouds" scene.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CloudsSettings {
    /// Region around the location, or the whole Earth.
    pub coverage: Coverage,
    /// Satellite source; automatic picks the best one for the location.
    pub source: CloudSource,
    /// Whole-Earth projection (global coverage only).
    pub projection: Projection,
    /// Globe rotation in degrees per minute, 0 to 30 (global globe only).
    pub rotation_deg_per_min: i32,
    /// Zoom of the local view: 1 shows about 120 degrees of latitude, each
    /// step halves it, up to 6 (about 4 degrees).
    pub zoom_level: i32,
    /// Hours of history looped: 0 (latest image only), 6, 12 or 24.
    pub history_hours: i32,
    /// Time-lapse playback speed in frames per second, 1 to 12.
    pub playback_fps: i32,
    /// Share of each frame interval spent cross-fading, 0 to 100.
    pub smoothing_percent: i32,
    /// Minutes between checks for newer images, 5 to 180.
    pub refresh_minutes: i32,
    /// Show clouds dark on a light ground instead of white on dark.
    pub invert: bool,
    /// Contrast around mid grey, 50 to 300.
    pub contrast_percent: i32,
    /// Brightness offset, -50 to 50.
    pub brightness_percent: i32,
    /// Dim land and sea so clouds stand out, 0 to 100.
    pub ground_dim_percent: i32,
    /// Faint land fill from the embedded land mask.
    pub land_underlay: bool,
    /// Brightness of the land fill, 5 to 60.
    pub land_underlay_percent: i32,
    /// Blinking dot at the shared location.
    pub marker: bool,
    /// Tint every cell with the satellite's colour instead of monochrome ink.
    pub cell_colors: bool,
}

impl Default for CloudsSettings {
    fn default() -> Self {
        Self {
            coverage: Coverage::Local,
            source: CloudSource::Auto,
            projection: Projection::Globe,
            rotation_deg_per_min: 2,
            zoom_level: 2,
            history_hours: 0,
            playback_fps: 4,
            smoothing_percent: 60,
            refresh_minutes: 20,
            invert: false,
            contrast_percent: 130,
            brightness_percent: 0,
            ground_dim_percent: 50,
            land_underlay: false,
            land_underlay_percent: 20,
            marker: false,
            cell_colors: false,
        }
    }
}

fn set_number(field: &mut i32, value: &ControlValue, min: i32, max: i32) -> Result<bool, String> {
    let Some(number) = control::number(value) else {
        return Ok(false);
    };
    let number = number.clamp(min, max);
    let changed = *field != number;
    *field = number;
    Ok(changed)
}

fn set_flag(field: &mut bool, value: &ControlValue) -> Result<bool, String> {
    let Some(flag) = control::boolean(value) else {
        return Ok(false);
    };
    let changed = *field != flag;
    *field = flag;
    Ok(changed)
}

impl SceneSettings for CloudsSettings {
    fn normalized(&self) -> Self {
        Self {
            rotation_deg_per_min: self.rotation_deg_per_min.clamp(0, 30),
            zoom_level: self.zoom_level.clamp(1, 6),
            history_hours: snap_history(self.history_hours),
            playback_fps: self.playback_fps.clamp(1, 12),
            smoothing_percent: self.smoothing_percent.clamp(0, 100),
            refresh_minutes: self.refresh_minutes.clamp(5, 180),
            contrast_percent: self.contrast_percent.clamp(50, 300),
            brightness_percent: self.brightness_percent.clamp(-50, 50),
            ground_dim_percent: self.ground_dim_percent.clamp(0, 100),
            land_underlay_percent: self.land_underlay_percent.clamp(5, 60),
            ..self.clone()
        }
    }

    fn controls(&self) -> Vec<Control> {
        let global = self.coverage == Coverage::Global;
        let mut rows = vec![
            Control::choice(
                "coverage",
                "Coverage",
                self.coverage.index(),
                &Coverage::LABELS,
                "A region around your shared location, or the whole Earth.",
            ),
            Control::choice(
                "source",
                "Satellite",
                self.source.index(),
                &CloudSource::LABELS,
                "Automatic picks the geostationary satellite that sees your location best, or a world mosaic for the whole Earth.",
            ),
        ];
        if global {
            rows.push(Control::choice(
                "projection",
                "Projection",
                self.projection.index(),
                &Projection::LABELS,
                "Flat map, globe centred on your location, or an equal-area Mollweide ellipse.",
            ));
            if self.projection == Projection::Globe {
                rows.push(Control::slider(
                    "rotation_deg_per_min",
                    "Globe rotation",
                    self.rotation_deg_per_min,
                    (0, 30, 1),
                    " deg/min",
                    "How fast the globe turns; 0 keeps your location in the centre.",
                ));
            }
        } else {
            rows.push(Control::slider(
                "zoom_level",
                "Zoom level",
                self.zoom_level,
                (1, 6, 1),
                "",
                "1 shows about 120 degrees of latitude; every level halves the view, down to about 4 degrees.",
            ));
        }
        rows.push(Control::choice(
            "history_hours",
            "History",
            HISTORY_HOURS
                .iter()
                .position(|hours| *hours == self.history_hours)
                .unwrap_or(0),
            &HISTORY_LABELS,
            "Show only the latest image, or loop the last hours as a time-lapse. Longer loops download more frames, once.",
        ));
        if self.history_hours > 0 {
            rows.push(Control::slider(
                "playback_fps",
                "Playback speed",
                self.playback_fps,
                (1, 12, 1),
                " fps",
                "Frames of the time-lapse per second.",
            ));
            rows.push(Control::slider(
                "smoothing_percent",
                "Smoothing",
                self.smoothing_percent,
                (0, 100, 10),
                "%",
                "Cross-fade between frames instead of cutting.",
            ));
        }
        rows.extend([
            Control::slider(
                "refresh_minutes",
                "Check for new images",
                self.refresh_minutes,
                (5, 180, 5),
                " min",
                "Minutes between checks for newer satellite images. Downloaded images are cached on disk.",
            ),
            Control::slider(
                "contrast_percent",
                "Contrast",
                self.contrast_percent,
                (50, 300, 10),
                "%",
                "How strongly clouds separate from the ground.",
            ),
            Control::slider(
                "brightness_percent",
                "Brightness",
                self.brightness_percent,
                (-50, 50, 5),
                "%",
                "Shift the whole picture lighter or darker.",
            ),
            Control::slider(
                "ground_dim_percent",
                "Ground dimming",
                self.ground_dim_percent,
                (0, 100, 5),
                "%",
                "Darken land and sea so that clouds stand out.",
            ),
            Control::toggle(
                "invert",
                "Invert",
                self.invert,
                "Draw clouds as dark ink on a light ground.",
            ),
            Control::toggle(
                "land_underlay",
                "Land underlay",
                self.land_underlay,
                "Fill land with a faint tone from the embedded land mask, useful over the ocean.",
            ),
        ]);
        if self.land_underlay {
            rows.push(Control::slider(
                "land_underlay_percent",
                "Underlay brightness",
                self.land_underlay_percent,
                (5, 60, 5),
                "%",
                "Brightness of the land fill.",
            ));
        }
        rows.extend([
            Control::toggle(
                "marker",
                "Location marker",
                self.marker,
                "A blinking dot at your shared location.",
            ),
            Control::toggle(
                "cell_colors",
                "Colour tint",
                self.cell_colors,
                "Tint each cell with the satellite's own colours (true colour where the source has them) instead of one ink colour.",
            ),
        ]);
        rows
    }

    fn set_control(&mut self, id: &str, value: ControlValue) -> Result<bool, String> {
        match id {
            "coverage" => Ok(control::index(&value).is_some_and(|index| {
                let next = Coverage::from_index(index);
                std::mem::replace(&mut self.coverage, next) != next
            })),
            "source" => Ok(control::index(&value).is_some_and(|index| {
                let next = CloudSource::from_index(index);
                std::mem::replace(&mut self.source, next) != next
            })),
            "projection" => Ok(control::index(&value).is_some_and(|index| {
                let next = Projection::from_index(index);
                std::mem::replace(&mut self.projection, next) != next
            })),
            "history_hours" => Ok(control::index(&value).is_some_and(|index| {
                let next = HISTORY_HOURS[index.min(HISTORY_HOURS.len() - 1)];
                std::mem::replace(&mut self.history_hours, next) != next
            })),
            "rotation_deg_per_min" => set_number(&mut self.rotation_deg_per_min, &value, 0, 30),
            "zoom_level" => set_number(&mut self.zoom_level, &value, 1, 6),
            "playback_fps" => set_number(&mut self.playback_fps, &value, 1, 12),
            "smoothing_percent" => set_number(&mut self.smoothing_percent, &value, 0, 100),
            "refresh_minutes" => set_number(&mut self.refresh_minutes, &value, 5, 180),
            "contrast_percent" => set_number(&mut self.contrast_percent, &value, 50, 300),
            "brightness_percent" => set_number(&mut self.brightness_percent, &value, -50, 50),
            "ground_dim_percent" => set_number(&mut self.ground_dim_percent, &value, 0, 100),
            "land_underlay_percent" => set_number(&mut self.land_underlay_percent, &value, 5, 60),
            "invert" => set_flag(&mut self.invert, &value),
            "land_underlay" => set_flag(&mut self.land_underlay, &value),
            "marker" => set_flag(&mut self.marker, &value),
            "cell_colors" => set_flag(&mut self.cell_colors, &value),
            _ => Ok(false),
        }
    }
}

// ---------------------------------------------------------------------------
// View geometry and time-lapse timing

/// The lon/lat box shown around a location: `span_latitude` degrees tall, as
/// wide as the dots' aspect ratio requires (dots are square), shifted rather
/// than cropped when it would leave the map.
pub(crate) fn local_view_box(
    latitude: f64,
    longitude: f64,
    span_latitude: f64,
    dots_w: usize,
    dots_h: usize,
) -> GeoBox {
    let aspect = dots_w.max(1) as f64 / dots_h.max(1) as f64;
    let half_latitude = (span_latitude / 2.0).clamp(1.0, 89.0);
    let scale = latitude.clamp(-80.0, 80.0).to_radians().cos().max(0.17);
    let half_longitude = (half_latitude * aspect / scale).min(179.0);
    let center_latitude = latitude.clamp(-90.0 + half_latitude, 90.0 - half_latitude);
    let center_longitude = longitude.clamp(-180.0 + half_longitude, 180.0 - half_longitude);
    GeoBox {
        west: center_longitude - half_longitude,
        east: center_longitude + half_longitude,
        south: center_latitude - half_latitude,
        north: center_latitude + half_latitude,
    }
}

/// Degrees of latitude shown at a zoom level.
pub(crate) fn span_for_zoom(zoom_level: i32) -> f64 {
    120.0 / f64::from(1u32 << (zoom_level.clamp(1, 6) - 1))
}

/// Exponent applied to the auto-levelled cloud brightness before contrast.
const CLOUD_GAMMA: f32 = 1.7;

/// Frames the newest picture stays on screen before the loop restarts.
const HOLD_FRAMES: f64 = 3.0;

/// Which frame(s) to show at `seconds` into a loop of `count` frames played
/// at `fps`: (current, next, blend). Only the last `smoothing` share of each
/// interval is cross-faded; the newest frame is held for a moment.
pub(crate) fn loop_position(
    seconds: f64,
    fps: f64,
    count: usize,
    smoothing: f32,
) -> (usize, usize, f32) {
    if count <= 1 {
        return (0, 0, 0.0);
    }
    let cycle = count as f64 - 1.0 + HOLD_FRAMES;
    let position = (seconds.max(0.0) * fps).rem_euclid(cycle);
    let index = position.floor() as usize;
    if index >= count - 1 {
        return (count - 1, count - 1, 0.0);
    }
    let fraction = (position - index as f64) as f32;
    let blend = if smoothing > 0.0 {
        smoothstep(1.0 - smoothing, 1.0, fraction)
    } else {
        0.0
    };
    (index, index + 1, blend)
}

// ---------------------------------------------------------------------------
// Scene

// At most 24 RGB+luma grids of 1,048,576 pixels (~96 MiB), plus a replacement
// frame set and decoder working memory during refresh.
const CLOUDS_WORKER_BYTES: usize = 384 * 1024 * 1024;
const UPDATE_QUEUE_CAPACITY: usize = 1;
const MAX_CLOUD_GRID_PIXELS: usize = 1_048_576;
const WORKER_FAILURE_BACKOFF: Duration = Duration::from_secs(5);

fn local_grid_size(dots_w: usize, dots_h: usize) -> (usize, usize) {
    let width = dots_w.clamp(64, 960);
    let requested_height = (width as f64 * dots_h as f64 / dots_w.max(1) as f64).round() as usize;
    let height = requested_height
        .max(32)
        .min(MAX_CLOUD_GRID_PIXELS / width.max(1));
    (width, height)
}

type LandFn = Box<dyn Fn(f64, f64) -> bool + Send>;

pub struct CloudsScene {
    // Declared first so the worker is stopped before anything else is dropped.
    worker: Option<Worker>,
    resources: AmbientResources,
    settings: CloudsSettings,
    location: GeoLocation,
    cache_dir: PathBuf,
    fetcher: Arc<dyn TileFetcher>,
    now: NowFn,
    land: LandFn,
    receiver: Option<Receiver<Update>>,
    /// True once the worker has been started (it starts at the first render,
    /// when the raster size is known).
    started: bool,
    /// The box the worker was asked to cover (local) or the world (global).
    view_box: Option<GeoBox>,
    set: Option<Arc<FrameSet>>,
    progress: Option<String>,
    problem: Option<String>,
    worker_retry_after: Option<Instant>,
    shown_label: Option<String>,
    located: Option<(View, Arc<DotTable>)>,
    palette: ScenePalette,
}

/// Where a dot sits on the map: `None` when it is off the picture.
struct DotMap<'a> {
    table: Option<&'a [Option<(f64, f64)>]>,
    bbox: GeoBox,
    width: usize,
    height: usize,
}

impl DotMap<'_> {
    fn locate(&self, x: usize, y: usize) -> Option<(f64, f64)> {
        match self.table {
            Some(table) => table[y * self.width + x],
            None => Some((
                self.bbox.west + (x as f64 + 0.5) / self.width as f64 * self.bbox.width_degrees(),
                self.bbox.north
                    - (y as f64 + 0.5) / self.height as f64 * self.bbox.height_degrees(),
            )),
        }
    }
}

impl CloudsScene {
    // PALETTE (native Scene contract): `env.palette` is the shared look's current
    // palette. A custom native Scene receives the
    // current palette and MUST follow it: scenes with natural colours shift them
    // onto it (`ScenePalette::recolor`/`at`), and `Scene::set_palette` delivers later
    // changes. This scene follows it natively: every per-cell colour it produces
    // is recoloured by brightness (`follows_palette`), so `PaletteScene` skips its
    // generic remap.
    pub fn new(settings: &CloudsSettings, env: &SceneEnv) -> Self {
        Self::with_parts(
            settings,
            env,
            Arc::new(HttpFetcher),
            system_clock(),
            Box::new(crate::worldmap::is_land),
        )
    }

    /// Construction with injected network, clock and land mask (tests).
    pub(crate) fn with_parts(
        settings: &CloudsSettings,
        env: &SceneEnv,
        fetcher: Arc<dyn TileFetcher>,
        now: NowFn,
        land: LandFn,
    ) -> Self {
        Self {
            worker: None,
            resources: env.resources.clone(),
            settings: settings.normalized(),
            location: env.location.normalized(),
            cache_dir: env.cache_dir.clone(),
            fetcher,
            now,
            land,
            receiver: None,
            started: false,
            view_box: None,
            set: None,
            progress: None,
            problem: None,
            worker_retry_after: None,
            shown_label: None,
            located: None,
            palette: env.palette.clone(),
        }
    }

    fn is_global(&self) -> bool {
        self.settings.coverage == Coverage::Global
    }

    /// Box and grid size the worker should produce for a raster of this size.
    fn plan_view(&self, dots_w: usize, dots_h: usize) -> (GeoBox, usize, usize) {
        if self.is_global() {
            let needed = match self.settings.projection {
                Projection::Globe => std::f64::consts::PI * dots_w.min(dots_h) as f64 * 0.94,
                _ => 2.0 * dots_h.min(dots_w / 2) as f64,
            };
            let width = (needed as usize).clamp(512, 1440) / 2 * 2;
            return (GeoBox::WORLD, width, width / 2);
        }
        let bbox = local_view_box(
            self.location.latitude,
            self.location.longitude,
            span_for_zoom(self.settings.zoom_level),
            dots_w,
            dots_h,
        );
        let (width, height) = local_grid_size(dots_w, dots_h);
        (bbox, width, height)
    }

    fn ensure_worker(&mut self, dots_w: usize, dots_h: usize) {
        self.observe_worker_exit();
        if self.started || dots_w == 0 || dots_h == 0 {
            return;
        }
        if self
            .worker_retry_after
            .is_some_and(|retry_after| Instant::now() < retry_after)
        {
            return;
        }
        self.worker_retry_after = None;
        let reservation = match self.resources.reserve_worker(WorkerCost {
            threads: 1,
            resident_bytes: CLOUDS_WORKER_BYTES,
        }) {
            Ok(reservation) => reservation,
            Err(error) => {
                self.problem = Some(format!("Clouds worker admission refused: {error:?}"));
                return;
            }
        };
        let (bbox, grid_width, grid_height) = self.plan_view(dots_w, dots_h);
        let source = choose_source(
            self.settings.source,
            self.is_global(),
            self.location.latitude,
            self.location.longitude,
        );
        let job = Job {
            cache_dir: self.cache_dir.clone(),
            primary: source,
            bbox,
            grid_width,
            grid_height,
            want_rgb: self.settings.cell_colors,
            history_hours: i64::from(self.settings.history_hours),
            refresh: Duration::from_secs(self.settings.refresh_minutes as u64 * 60),
            fetcher: Arc::clone(&self.fetcher),
            now: Arc::clone(&self.now),
        };
        let (sender, receiver) = mpsc::sync_channel(UPDATE_QUEUE_CAPACITY);
        let worker = Worker::start_admitted("clouds", reservation, move |stop| {
            worker::run_worker(job, sender, stop);
        });
        match worker {
            Ok(worker) => {
                self.started = true;
                self.receiver = Some(receiver);
                self.view_box = Some(bbox);
                self.worker = Some(worker);
                self.problem = None;
            }
            Err(error) => {
                self.problem = Some(format!("Clouds worker could not start: {error}"));
            }
        }
    }

    fn observe_worker_exit(&mut self) {
        let exit = self
            .worker
            .as_ref()
            .and_then(Worker::join_observer)
            .and_then(|ticket| ticket.exit());
        let Some(exit) = exit else { return };
        self.worker.take();
        self.receiver = None;
        self.started = false;
        self.progress = None;
        self.worker_retry_after = Some(Instant::now() + WORKER_FAILURE_BACKOFF);
        self.problem = Some(format!(
            "Clouds worker exited unexpectedly ({exit:?}); retrying shortly"
        ));
    }

    fn apply(&mut self, update: Update) {
        match update {
            Update::Progress(text) => {
                if text.is_some() {
                    self.problem = None;
                }
                self.progress = text;
            }
            Update::Frames(set) => {
                self.shown_label = set
                    .frames
                    .last()
                    .map(|frame| format!("{} {}", set.name, frame.time.label()));
                self.set = Some(set);
                self.problem = None;
            }
            Update::Offline(message) => {
                self.progress = None;
                self.problem = Some(message);
            }
        }
    }

    fn drain_updates(&mut self) {
        let mut pending = Vec::new();
        if let Some(receiver) = &self.receiver {
            while let Ok(update) = receiver.try_recv() {
                pending.push(update);
            }
        }
        for update in pending {
            self.apply(update);
        }
    }

    /// Block until the worker has delivered something new (tests only).
    #[cfg(test)]
    fn wait_for_update(&mut self, timeout: Duration) -> bool {
        let received = self
            .receiver
            .as_ref()
            .and_then(|receiver| receiver.recv_timeout(timeout).ok());
        match received {
            Some(update) => {
                self.apply(update);
                true
            }
            None => false,
        }
    }

    fn global_view(&self, frame: &Frame<'_>) -> View {
        let settings = &self.settings;
        let center_lon = match settings.projection {
            Projection::Globe => projection::wrap_degrees(
                self.location.longitude
                    - f64::from(settings.rotation_deg_per_min) * frame.time.as_secs_f64() / 60.0,
            ),
            _ => self.location.longitude,
        };
        View {
            projection: settings.projection,
            center_lon,
            center_lat: self.location.latitude,
            zoom: 1.0,
            dots_w: frame.raster.width,
            dots_h: frame.raster.height,
        }
    }

    /// Raw luminance 0..1 to dot intensity: automatic levels, contrast,
    /// brightness, optional inversion and (unless inverted) ground dimming.
    fn transfer(&self, value: f32, set: &FrameSet) -> f32 {
        let settings = &self.settings;
        let cloud = ((value - set.black) / (set.white - set.black)).clamp(0.0, 1.0);
        // Thin cloud stays sparse and thick cloud becomes dense ink when dithered.
        let density = cloud.powf(CLOUD_GAMMA);
        let shown = if settings.invert {
            1.0 - density
        } else {
            density
        };
        let contrast = settings.contrast_percent as f32 / 100.0;
        let brightness = settings.brightness_percent as f32 / 100.0;
        let mut out = ((shown - 0.5) * contrast + 0.5 + brightness).clamp(0.0, 1.0);
        if !settings.invert {
            let ground = 1.0 - smoothstep(0.3, 0.65, cloud);
            out *= 1.0 - settings.ground_dim_percent as f32 / 100.0 * ground;
        }
        out
    }

    fn sample_frames(&self, a: &CloudFrame, b: &CloudFrame, blend: f32, lon: f64, lat: f64) -> f32 {
        let first = a.grid.sample_luma(lon, lat).unwrap_or(0.0);
        if blend <= 0.0 {
            return first;
        }
        let second = b.grid.sample_luma(lon, lat).unwrap_or(0.0);
        first * (1.0 - blend) + second * blend
    }

    fn draw_set(&mut self, frame: &mut Frame<'_>, set: &FrameSet) {
        let (width, height) = (frame.raster.width, frame.raster.height);
        let settings = &self.settings;
        let count = set.frames.len();
        let (index, next, blend) = loop_position(
            frame.time.as_secs_f64(),
            f64::from(settings.playback_fps),
            count,
            settings.smoothing_percent as f32 / 100.0,
        );
        let (Some(current), Some(following)) = (set.frames.get(index), set.frames.get(next)) else {
            return;
        };
        let shown = if blend > 0.5 { following } else { current };
        let position = set
            .frames
            .iter()
            .position(|item| Arc::ptr_eq(item, shown))
            .unwrap_or(0);
        self.shown_label = Some(if count > 1 {
            format!(
                "{} {} ({}/{count})",
                set.name,
                shown.time.label(),
                position + 1
            )
        } else {
            format!("{} {}", set.name, shown.time.label())
        });
        let global_table = self.global_table(frame);
        let map = DotMap {
            table: global_table.as_ref().map(|table| table.as_slice()),
            bbox: set.bbox,
            width,
            height,
        };
        let underlay = self
            .settings
            .land_underlay
            .then(|| self.settings.land_underlay_percent as f32 / 100.0);
        for y in 0..height {
            for x in 0..width {
                let Some((lon, lat)) = map.locate(x, y) else {
                    continue;
                };
                let luma = self.sample_frames(current, following, blend, lon, lat);
                let mut value = self.transfer(luma, set);
                if let Some(underlay) = underlay {
                    if (self.land)(lon, lat) {
                        value = value.max(underlay);
                    }
                }
                frame.raster.dots[y * width + x] = value;
            }
        }
        if self.settings.cell_colors {
            self.fill_cell_colors(frame, &map, shown);
        }
        if self.is_global() && self.settings.projection == Projection::Globe {
            let view = self.global_view(frame);
            for y in 0..height {
                for x in 0..width {
                    if map.locate(x, y).is_none() && view.globe_ring(x, y) {
                        frame.raster.dots[y * width + x] = 0.13;
                    }
                }
            }
        }
    }

    /// Recompute (or reuse) the dot-to-lon/lat table of a global view.
    fn global_table(&mut self, frame: &Frame<'_>) -> Option<Arc<DotTable>> {
        if !self.is_global() {
            return None;
        }
        let view = self.global_view(frame);
        let (width, height) = (frame.raster.width, frame.raster.height);
        let stale = self
            .located
            .as_ref()
            .is_none_or(|(known, table)| *known != view || table.len() != width * height);
        if stale {
            let table = (0..height)
                .flat_map(|y| (0..width).map(move |x| (x, y)))
                .map(|(x, y)| view.locate(x, y))
                .collect();
            self.located = Some((view, Arc::new(table)));
        }
        self.located.as_ref().map(|(_, table)| Arc::clone(table))
    }

    /// One colour per cell: the satellite's colour at the cell centre,
    /// brightened to the cell's ink level.
    fn fill_cell_colors(&self, frame: &mut Frame<'_>, map: &DotMap<'_>, shown: &CloudFrame) {
        let (width, height) = (frame.width as usize, frame.height as usize);
        let raster_width = frame.raster.width;
        for cell_y in 0..height {
            for cell_x in 0..width {
                let mut ink = 0.0f32;
                for dy in 0..4 {
                    for dx in 0..2 {
                        ink +=
                            frame.raster.dots[(cell_y * 4 + dy) * raster_width + cell_x * 2 + dx];
                    }
                }
                ink /= 8.0;
                let base = map
                    .locate(cell_x * 2 + 1, cell_y * 4 + 2)
                    .and_then(|(lon, lat)| shown.grid.sample_rgb(lon, lat));
                let color = match base {
                    Some([red, green, blue]) => {
                        let peak = red.max(green).max(blue).max(0.04);
                        let level = 0.35 + 0.65 * ink.clamp(0.0, 1.0);
                        [red, green, blue]
                            .map(|channel| (channel / peak * level * 255.0).round() as u8)
                    }
                    None => {
                        let level = (70.0 + 185.0 * ink.clamp(0.0, 1.0)) as u8;
                        [level, level, level]
                    }
                };
                if let Some(slot) = frame.cell_color_mut(cell_x as u16, cell_y as u16) {
                    *slot = self.palette.recolor(color);
                }
            }
        }
    }
}

impl Scene for CloudsScene {
    fn render(&mut self, frame: &mut Frame<'_>) {
        let (width, height) = (frame.raster.width, frame.raster.height);
        self.ensure_worker(width, height);
        self.drain_updates();
        if width == 0 || height == 0 {
            return;
        }
        match self.set.clone() {
            Some(set) => self.draw_set(frame, &set),
            None => self.draw_placeholder(frame),
        }
        if self.settings.marker {
            let target = match (self.is_global(), self.view_box) {
                (true, _) => self
                    .global_view(frame)
                    .project(self.location.longitude, self.location.latitude),
                (false, Some(bbox)) => Some((
                    ((self.location.longitude - bbox.west) / bbox.width_degrees() * width as f64)
                        as f32,
                    ((bbox.north - self.location.latitude) / bbox.height_degrees() * height as f64)
                        as f32,
                ))
                .filter(|(x, y)| *x >= 0.0 && *y >= 0.0 && *x < width as f32 && *y < height as f32),
                (false, None) => None,
            };
            if let Some(centre) = target {
                projection::draw_marker(
                    &mut frame.raster.dots,
                    (width, height),
                    centre,
                    frame.wall.as_secs_f32(),
                );
            }
        }
        if self.settings.cell_colors && self.set.is_none() {
            frame.cell_colors.fill(self.palette.recolor([70, 70, 80]));
        }
    }

    fn uses_cell_colors(&self) -> bool {
        self.settings.cell_colors
    }

    fn set_palette(&mut self, palette: &ScenePalette) {
        // Colours are computed per frame, so nothing is cached.
        self.palette = palette.clone();
    }

    fn follows_palette(&self) -> bool {
        true
    }

    fn frames_per_second(&self) -> u32 {
        let settings = &self.settings;
        let frames = self.set.as_ref().map_or(1, |set| set.frames.len());
        if frames > 1 {
            let smooth = if settings.smoothing_percent > 0 { 3 } else { 1 };
            (settings.playback_fps as u32 * smooth).clamp(4, 30)
        } else if self.is_global()
            && settings.projection == Projection::Globe
            && settings.rotation_deg_per_min > 0
        {
            12
        } else if settings.marker {
            8
        } else {
            2
        }
    }

    fn status(&self) -> Option<String> {
        if let Some(progress) = &self.progress {
            return Some(progress.clone());
        }
        match (&self.problem, &self.shown_label) {
            (Some(problem), Some(label)) => Some(format!("Offline ({problem}); showing {label}")),
            (Some(problem), None) => Some(format!("No imagery yet: {problem}")),
            (None, Some(label)) => Some(label.clone()),
            (None, None) => Some("Preparing satellite imagery".to_owned()),
        }
    }
}

impl CloudsScene {
    /// The dim graticule shown until the first frame arrives.
    fn draw_placeholder(&mut self, frame: &mut Frame<'_>) {
        let (width, height) = (frame.raster.width, frame.raster.height);
        let global_table = self.global_table(frame);
        let bbox = self.view_box.unwrap_or(GeoBox::WORLD);
        let map = DotMap {
            table: global_table.as_ref().map(|table| table.as_slice()),
            bbox,
            width,
            height,
        };
        let degrees_per_dot = if self.is_global() {
            self.global_view(frame).degrees_per_dot()
        } else {
            bbox.height_degrees() / height as f64
        };
        for y in 0..height {
            for x in 0..width {
                if let Some((lon, lat)) = map.locate(x, y) {
                    frame.raster.dots[y * width + x] =
                        projection::graticule_value(lon, lat, degrees_per_dot);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;

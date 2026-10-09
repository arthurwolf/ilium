//! Earth at night: the light seen from orbit, on a borderless map.
//!
//! Imagery comes from NASA GIBS (keyless, WMTS EPSG:4326, TileMatrixSet
//! `500m`, see `tiles.rs` for documentation and terms):
//! * `VIIRS_NOAA20_GapFilled_BRDF_Corrected_DayNightBand_Radiance`, a daily,
//!   near-global, gap-filled night radiance product (the "live" source; the
//!   newest day is usually yesterday), and
//! * `VIIRS_Black_Marble` (2016 composite), the static fallback with complete
//!   coverage.
//!
//! A worker thread probes for the newest day, downloads a bounded set of tiles
//! (15 at level 2, at most 50 at level 3), mosaics them into one luminance map,
//! caches it on disk as a PNG and refreshes every few hours. `render` only
//! resamples the last good mosaic to the dot raster.

pub(crate) mod projection;
pub(crate) mod tiles;

use self::projection::{sun_position, DotTable, Projection, View};
use self::tiles::{
    download_tiles, encode_gray_png, gibs_tile_url, grid_from_tiles, prune_cache, system_clock,
    tiles_for_box, write_atomic, DownloadError, GeoBox, GeoGrid, HttpFetcher, MipGrid, NowFn,
    TileError, TileFetcher, TileId, UtcTime, IMMUTABLE_AGE, TILE_PIXELS,
};
use crate::control::{self, Control, ControlValue, SceneSettings};
use crate::location::GeoLocation;
use crate::scene::{Frame, Scene, SceneEnv};
use crate::source::{sleep_unless_stopped, Worker};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

const DAILY_LAYER: &str = "VIIRS_NOAA20_GapFilled_BRDF_Corrected_DayNightBand_Radiance";
const BLACK_MARBLE_LAYER: &str = "VIIRS_Black_Marble";
const BLACK_MARBLE_DATE: &str = "2016-01-01";
const MATRIX_SET: &str = "500m";
/// Newest day probed first, then this many earlier days.
const DAILY_LOOKBACK_DAYS: i64 = 4;
const RETRY_AFTER: Duration = Duration::from_secs(5 * 60);
const TILE_CACHE_BUDGET: u64 = 120 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NightSource {
    /// Newest daily VIIRS night radiance (falls back to Black Marble).
    #[default]
    Daily,
    /// Static 2016 Black Marble composite.
    BlackMarble,
}

impl NightSource {
    const LABELS: [&'static str; 2] = ["Latest daily (VIIRS)", "Black Marble 2016"];

    fn index(self) -> usize {
        match self {
            Self::Daily => 0,
            Self::BlackMarble => 1,
        }
    }

    fn from_index(index: usize) -> Self {
        if index == 1 {
            Self::BlackMarble
        } else {
            Self::Daily
        }
    }

    fn id(self) -> &'static str {
        match self {
            Self::Daily => "daily",
            Self::BlackMarble => "black_marble",
        }
    }

    /// Luminance below this (0..255) is terrain glow or sensor floor, not light.
    fn black_level(self) -> u8 {
        match self {
            Self::Daily => 10,
            Self::BlackMarble => 54,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Daily => "VIIRS daily",
            Self::BlackMarble => "Black Marble",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Detail {
    /// Choose the tile level from the terminal size (15 tiles, 50 when huge).
    #[default]
    Auto,
    /// One level coarser: fewer tiles, softer picture.
    Coarse,
    /// One level finer, at most level 3 (50 tiles).
    Fine,
}

impl Detail {
    const LABELS: [&'static str; 3] = ["Automatic", "Coarse (fewer tiles)", "Fine (more tiles)"];

    fn index(self) -> usize {
        match self {
            Self::Auto => 0,
            Self::Coarse => 1,
            Self::Fine => 2,
        }
    }

    fn from_index(index: usize) -> Self {
        match index {
            1 => Self::Coarse,
            2 => Self::Fine,
            _ => Self::Auto,
        }
    }
}

/// Settings of the "Earth at night" scene. Percentages are integers so the
/// YAML stays readable; ranges are enforced by `normalized`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NightLightsSettings {
    /// Imagery source.
    pub source: NightSource,
    /// Flat map, globe or Mollweide.
    pub projection: Projection,
    /// Magnification, 100 (whole world) to 600.
    pub zoom_percent: i32,
    /// Globe rotation in degrees per minute of animation time, 0 to 30.
    pub rotation_deg_per_min: i32,
    /// Overall gain applied after gamma, 25 to 400.
    pub brightness_percent: i32,
    /// Gamma exponent times 100, 30 to 300. Below 100 lifts faint lights.
    pub gamma_percent: i32,
    /// Luminance below this share of full scale is dropped, 0 to 60.
    pub threshold_percent: i32,
    /// Soft halo around bright cities, 0 to 100.
    pub glow_percent: i32,
    /// Tile resolution.
    pub detail: Detail,
    /// Dim the sunlit side of the Earth.
    pub terminator: bool,
    /// How strongly daylight hides the lights, 0 to 100.
    pub terminator_strength_percent: i32,
    /// Faint coastlines.
    pub coastline: bool,
    /// Coastline brightness, 5 to 100.
    pub coastline_strength_percent: i32,
    /// Blinking dot at the shared location.
    pub marker: bool,
    /// Hours between checks for newer imagery, 1 to 48.
    pub refresh_hours: i32,
}

impl Default for NightLightsSettings {
    fn default() -> Self {
        Self {
            source: NightSource::Daily,
            projection: Projection::Flat,
            zoom_percent: 100,
            rotation_deg_per_min: 3,
            brightness_percent: 120,
            gamma_percent: 70,
            threshold_percent: 8,
            glow_percent: 20,
            detail: Detail::Auto,
            terminator: false,
            terminator_strength_percent: 70,
            coastline: false,
            coastline_strength_percent: 25,
            marker: false,
            refresh_hours: 6,
        }
    }
}

/// Apply a numeric edit: wrong value type is ignored, the number is clamped.
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

impl SceneSettings for NightLightsSettings {
    fn normalized(&self) -> Self {
        Self {
            zoom_percent: self.zoom_percent.clamp(100, 600),
            rotation_deg_per_min: self.rotation_deg_per_min.clamp(0, 30),
            brightness_percent: self.brightness_percent.clamp(25, 400),
            gamma_percent: self.gamma_percent.clamp(30, 300),
            threshold_percent: self.threshold_percent.clamp(0, 60),
            glow_percent: self.glow_percent.clamp(0, 100),
            terminator_strength_percent: self.terminator_strength_percent.clamp(0, 100),
            coastline_strength_percent: self.coastline_strength_percent.clamp(5, 100),
            refresh_hours: self.refresh_hours.clamp(1, 48),
            ..self.clone()
        }
    }

    fn controls(&self) -> Vec<Control> {
        let mut rows = vec![
            Control::choice(
                "source",
                "Imagery",
                self.source.index(),
                &NightSource::LABELS,
                "Latest daily VIIRS night radiance, refreshed on its own; Black Marble is a static, cleaner 2016 composite.",
            ),
            Control::choice(
                "projection",
                "Projection",
                self.projection.index(),
                &Projection::LABELS,
                "Flat map, orthographic globe centred on your location, or an equal-area Mollweide ellipse.",
            ),
            Control::slider(
                "zoom_percent",
                "Zoom",
                self.zoom_percent,
                (100, 600, 25),
                "%",
                "100% shows the whole world; larger values magnify around your location.",
            ),
        ];
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
        rows.extend([
            Control::slider(
                "brightness_percent",
                "Brightness",
                self.brightness_percent,
                (25, 400, 5),
                "%",
                "Overall gain of the lights after gamma.",
            ),
            Control::slider(
                "gamma_percent",
                "Gamma",
                self.gamma_percent,
                (30, 300, 5),
                "%",
                "Below 100% reveals faint towns; above 100% keeps only the brightest cities.",
            ),
            Control::slider(
                "threshold_percent",
                "Threshold",
                self.threshold_percent,
                (0, 60, 1),
                "%",
                "Light dimmer than this share of full scale is treated as darkness, which keeps dithering clean.",
            ),
            Control::slider(
                "glow_percent",
                "Glow",
                self.glow_percent,
                (0, 100, 5),
                "%",
                "A soft halo around bright cities so they survive coarse dithering.",
            ),
            Control::choice(
                "detail",
                "Detail",
                self.detail.index(),
                &Detail::LABELS,
                "Tile resolution to download. Automatic follows the terminal size.",
            ),
            Control::toggle(
                "terminator",
                "Day/night shading",
                self.terminator,
                "Hide the lights on the sunlit side of the Earth, using the real position of the Sun.",
            ),
        ]);
        if self.terminator {
            rows.push(Control::slider(
                "terminator_strength_percent",
                "Daylight dimming",
                self.terminator_strength_percent,
                (0, 100, 5),
                "%",
                "How completely daylight hides the lights.",
            ));
        }
        rows.push(Control::toggle(
            "coastline",
            "Coastlines",
            self.coastline,
            "Draw a faint coastline for orientation. Off by default: the map has no borders or labels.",
        ));
        if self.coastline {
            rows.push(Control::slider(
                "coastline_strength_percent",
                "Coastline brightness",
                self.coastline_strength_percent,
                (5, 100, 5),
                "%",
                "Brightness of the coastline dots.",
            ));
        }
        rows.extend([
            Control::toggle(
                "marker",
                "Location marker",
                self.marker,
                "A blinking dot at your shared location.",
            ),
            Control::slider(
                "refresh_hours",
                "Check for new imagery",
                self.refresh_hours,
                (1, 48, 1),
                " h",
                "Hours between checks for a newer daily image. Downloaded tiles are cached on disk.",
            ),
        ]);
        rows
    }

    fn set_control(&mut self, id: &str, value: ControlValue) -> Result<bool, String> {
        match id {
            "source" => Ok(control::index(&value).is_some_and(|index| {
                let next = NightSource::from_index(index);
                std::mem::replace(&mut self.source, next) != next
            })),
            "projection" => Ok(control::index(&value).is_some_and(|index| {
                let next = Projection::from_index(index);
                std::mem::replace(&mut self.projection, next) != next
            })),
            "detail" => Ok(control::index(&value).is_some_and(|index| {
                let next = Detail::from_index(index);
                std::mem::replace(&mut self.detail, next) != next
            })),
            "zoom_percent" => set_number(&mut self.zoom_percent, &value, 100, 600),
            "rotation_deg_per_min" => set_number(&mut self.rotation_deg_per_min, &value, 0, 30),
            "brightness_percent" => set_number(&mut self.brightness_percent, &value, 25, 400),
            "gamma_percent" => set_number(&mut self.gamma_percent, &value, 30, 300),
            "threshold_percent" => set_number(&mut self.threshold_percent, &value, 0, 60),
            "glow_percent" => set_number(&mut self.glow_percent, &value, 0, 100),
            "terminator" => set_flag(&mut self.terminator, &value),
            "terminator_strength_percent" => {
                set_number(&mut self.terminator_strength_percent, &value, 0, 100)
            }
            "coastline" => set_flag(&mut self.coastline, &value),
            "coastline_strength_percent" => {
                set_number(&mut self.coastline_strength_percent, &value, 5, 100)
            }
            "marker" => set_flag(&mut self.marker, &value),
            "refresh_hours" => set_number(&mut self.refresh_hours, &value, 1, 48),
            _ => Ok(false),
        }
    }
}

// ---------------------------------------------------------------------------
// Worker

enum Update {
    /// Progress text ("Downloading imagery 12/30 tiles"), `None` when finished.
    Progress(Option<String>),
    /// A new (or cached) mosaic.
    Mosaic { grid: Arc<MipGrid>, label: String },
    /// Nothing could be fetched; the message explains why.
    Offline(String),
}

struct Job {
    cache_dir: PathBuf,
    source: NightSource,
    level: u8,
    refresh: Duration,
    fetcher: Arc<dyn TileFetcher>,
    now: NowFn,
}

enum RefreshError {
    Stopped,
    Offline(String),
}

struct Refreshed {
    /// `None` when the held mosaic is already current.
    grid: Option<(Arc<MipGrid>, String)>,
    key: Option<String>,
    complete: bool,
}

fn tile_url(layer: &str, date: &str, id: TileId) -> String {
    gibs_tile_url(layer, Some(date), MATRIX_SET, id, "png")
}

fn mosaic_file_name(source: NightSource, date: &str, level: u8) -> String {
    format!("mosaic_{}_{date}_L{level}.png", source.id())
}

fn mosaic_key(source: NightSource, date: &str, level: u8) -> String {
    format!("{}_{date}_L{level}", source.id())
}

/// The newest cached mosaic of `source` at `level`, as (grid, date).
fn load_cached_mosaic(
    directory: &Path,
    source: NightSource,
    level: u8,
) -> Option<(GeoGrid, String)> {
    let prefix = format!("mosaic_{}_", source.id());
    let suffix = format!("_L{level}.png");
    let mut best: Option<(String, PathBuf)> = None;
    for entry in std::fs::read_dir(directory).ok()?.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(date) = name
            .strip_prefix(&prefix)
            .and_then(|rest| rest.strip_suffix(&suffix))
        else {
            continue;
        };
        if best
            .as_ref()
            .is_none_or(|(newest, _)| date > newest.as_str())
        {
            best = Some((date.to_owned(), entry.path()));
        }
    }
    let (date, path) = best?;
    let image = tiles::decode_image(&std::fs::read(path).ok()?, false).ok()?;
    let expected_width = TILE_PIXELS * 5 * (1usize << level) / 4;
    if image.width != expected_width {
        return None;
    }
    Some((GeoGrid::from_image(GeoBox::WORLD, image), date))
}

/// The newest cached mosaic for `source`, or for Black Marble when the daily
/// product has never been cached.
fn load_any_cached(
    directory: &Path,
    source: NightSource,
    level: u8,
) -> Option<(GeoGrid, NightSource, String)> {
    if let Some((grid, date)) = load_cached_mosaic(directory, source, level) {
        return Some((grid, source, date));
    }
    if source == NightSource::Daily {
        let (grid, date) = load_cached_mosaic(directory, NightSource::BlackMarble, level)?;
        return Some((grid, NightSource::BlackMarble, date));
    }
    None
}

fn refresh_once(
    job: &Job,
    directory: &Path,
    held_key: Option<&str>,
    updates: &Sender<Update>,
    stop: &AtomicBool,
) -> Result<Refreshed, RefreshError> {
    let tile_directory = directory.join("tiles");
    let ids = tiles_for_box(job.level, &GeoBox::WORLD);
    let first = ids[0];
    let fetch = |url: &str| job.fetcher.fetch(&tile_directory, url, IMMUTABLE_AGE, stop);

    // Decide which layer/date to show.
    let (source, layer, date) = match job.source {
        NightSource::BlackMarble => (
            NightSource::BlackMarble,
            BLACK_MARBLE_LAYER,
            BLACK_MARBLE_DATE.to_owned(),
        ),
        NightSource::Daily => {
            let today = UtcTime::from_system_time((job.now)()).start_of_day();
            let mut found = None;
            for back in 0..=DAILY_LOOKBACK_DAYS {
                let date = today.plus_seconds(-back * 86_400).date();
                match fetch(&tile_url(DAILY_LAYER, &date, first)) {
                    Ok(_) => {
                        found = Some(date);
                        break;
                    }
                    Err(TileError::Missing) => continue,
                    Err(TileError::Stopped) => return Err(RefreshError::Stopped),
                    Err(TileError::Network(message)) => return Err(RefreshError::Offline(message)),
                }
            }
            match found {
                Some(date) => (NightSource::Daily, DAILY_LAYER, date),
                None => (
                    NightSource::BlackMarble,
                    BLACK_MARBLE_LAYER,
                    BLACK_MARBLE_DATE.to_owned(),
                ),
            }
        }
    };
    let key = mosaic_key(source, &date, job.level);
    if held_key == Some(key.as_str()) {
        return Ok(Refreshed {
            grid: None,
            key: Some(key),
            complete: true,
        });
    }

    let mut announce = |done: usize, total: usize| {
        let _ = updates.send(Update::Progress(Some(format!(
            "Downloading imagery {done}/{total} tiles"
        ))));
    };
    announce(0, ids.len());
    let download = download_tiles(
        &*job.fetcher,
        &tile_directory,
        &ids,
        &|id| tile_url(layer, &date, id),
        false,
        stop,
        &mut announce,
    )
    .map_err(|error| match error {
        DownloadError::Stopped => RefreshError::Stopped,
        DownloadError::Network(message) => RefreshError::Offline(message),
    })?;
    if download.tiles.is_empty() {
        return Err(RefreshError::Offline(
            "no imagery tiles are available".to_owned(),
        ));
    }

    let scale = 1usize << job.level;
    let mut grid = grid_from_tiles(
        GeoBox::WORLD,
        TILE_PIXELS * 5 * scale / 4,
        TILE_PIXELS * 5 * scale / 8,
        job.level,
        &download.tiles,
        false,
    );
    grid.apply_black_level(source.black_level());
    let complete = download.missing == 0;
    if complete {
        // Persist for instant, offline starts; keep only the newest mosaic.
        if let Ok(bytes) = encode_gray_png(&grid) {
            let _ = write_atomic(
                &directory.join(mosaic_file_name(source, &date, job.level)),
                &bytes,
            );
            remove_older_mosaics(directory, source, job.level, &date);
        }
        prune_cache(&tile_directory, TILE_CACHE_BUDGET);
    }
    let mut label = format!("{} {date}", source.name());
    if source != job.source {
        label.push_str(" (no recent daily data)");
    }
    if !complete {
        label.push_str(&format!(
            " (partial: {} of {} tiles)",
            download.tiles.len(),
            ids.len()
        ));
    }
    Ok(Refreshed {
        grid: Some((Arc::new(MipGrid::build(grid)), label)),
        key: complete.then_some(key),
        complete,
    })
}

fn remove_older_mosaics(directory: &Path, source: NightSource, level: u8, keep_date: &str) {
    let prefix = format!("mosaic_{}_", source.id());
    let suffix = format!("_L{level}.png");
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let older = name
            .strip_prefix(&prefix)
            .and_then(|rest| rest.strip_suffix(&suffix))
            .is_some_and(|date| date != keep_date);
        if older {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

fn run_worker(job: Job, updates: Sender<Update>, stop: Arc<AtomicBool>) {
    let directory = job.cache_dir.join("night_lights");
    let mut held_key = None;
    // Show whatever is on disk immediately (also works fully offline).
    if let Some((grid, source, date)) = load_any_cached(&directory, job.source, job.level) {
        held_key = Some(mosaic_key(source, &date, job.level));
        let _ = updates.send(Update::Mosaic {
            grid: Arc::new(MipGrid::build(grid)),
            label: format!("{} {date} (cached)", source.name()),
        });
    }
    while !stop.load(Ordering::Relaxed) {
        let wait = match refresh_once(&job, &directory, held_key.as_deref(), &updates, &stop) {
            Ok(refreshed) => {
                if let Some((grid, label)) = refreshed.grid {
                    let _ = updates.send(Update::Mosaic { grid, label });
                }
                let _ = updates.send(Update::Progress(None));
                held_key = refreshed.key;
                if refreshed.complete {
                    job.refresh
                } else {
                    RETRY_AFTER
                }
            }
            Err(RefreshError::Stopped) => return,
            Err(RefreshError::Offline(message)) => {
                let _ = updates.send(Update::Offline(message));
                RETRY_AFTER
            }
        };
        if !sleep_unless_stopped(&stop, wait) {
            return;
        }
    }
}

// ---------------------------------------------------------------------------
// Scene

type LandFn = Box<dyn Fn(f64, f64) -> bool + Send>;

pub struct NightLightsScene {
    // Declared first so the worker is stopped before anything else is dropped.
    worker: Option<Worker>,
    settings: NightLightsSettings,
    location: GeoLocation,
    cache_dir: PathBuf,
    fetcher: Arc<dyn TileFetcher>,
    now: NowFn,
    land: LandFn,
    receiver: Option<Receiver<Update>>,
    mosaic: Option<Arc<MipGrid>>,
    label: Option<String>,
    progress: Option<String>,
    problem: Option<String>,
    located: Option<(View, DotTable)>,
}

impl NightLightsScene {
    // PALETTE (native Scene contract): `env.palette` is the shared look's current
    // palette. A custom native Scene receives the
    // current palette and MUST follow it: scenes with natural colours shift them
    // onto it (`ScenePalette::recolor`/`at`), and `Scene::set_palette` delivers later
    // changes. Monochrome scenes may ignore it. Today `PaletteScene` (scene.rs),
    // which `create_scene` wraps around every scene, shifts this scene's cell
    // colours onto the palette by brightness.
    pub fn new(settings: &NightLightsSettings, env: &SceneEnv) -> Self {
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
        settings: &NightLightsSettings,
        env: &SceneEnv,
        fetcher: Arc<dyn TileFetcher>,
        now: NowFn,
        land: LandFn,
    ) -> Self {
        Self {
            worker: None,
            settings: settings.normalized(),
            location: env.location.normalized(),
            cache_dir: env.cache_dir.clone(),
            fetcher,
            now,
            land,
            receiver: None,
            mosaic: None,
            label: None,
            progress: None,
            problem: None,
            located: None,
        }
    }

    fn choose_level(&self, dots_w: usize, dots_h: usize) -> u8 {
        let zoom = f64::from(self.settings.zoom_percent) / 100.0;
        let needed_width = match self.settings.projection {
            Projection::Globe => std::f64::consts::PI * dots_w.min(dots_h) as f64 * 0.94 * zoom,
            _ => 2.0 * dots_h.min(dots_w / 2) as f64 * zoom,
        };
        let base: u8 = if needed_width <= 3000.0 { 2 } else { 3 };
        match self.settings.detail {
            Detail::Auto => base,
            Detail::Coarse => base - 1,
            Detail::Fine => (base + 1).min(3),
        }
    }

    fn ensure_worker(&mut self, dots_w: usize, dots_h: usize) {
        if self.worker.is_some() || dots_w == 0 || dots_h == 0 {
            return;
        }
        let (sender, receiver) = mpsc::channel();
        let job = Job {
            cache_dir: self.cache_dir.clone(),
            source: self.settings.source,
            level: self.choose_level(dots_w, dots_h),
            refresh: Duration::from_secs(self.settings.refresh_hours as u64 * 3600),
            fetcher: Arc::clone(&self.fetcher),
            now: Arc::clone(&self.now),
        };
        self.receiver = Some(receiver);
        self.worker = Some(Worker::spawn("night-lights", move |stop| {
            run_worker(job, sender, stop);
        }));
    }

    fn apply(&mut self, update: Update) {
        match update {
            Update::Progress(text) => {
                if text.is_some() {
                    self.problem = None;
                }
                self.progress = text;
            }
            Update::Mosaic { grid, label } => {
                self.mosaic = Some(grid);
                self.label = Some(label);
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

    fn view(&self, frame: &Frame<'_>) -> View {
        let settings = &self.settings;
        let center_lon = match settings.projection {
            Projection::Globe => {
                let minutes = frame.time.as_secs_f64() / 60.0;
                projection::wrap_degrees(
                    self.location.longitude - f64::from(settings.rotation_deg_per_min) * minutes,
                )
            }
            _ => self.location.longitude,
        };
        View {
            projection: settings.projection,
            center_lon,
            center_lat: self.location.latitude,
            zoom: f64::from(settings.zoom_percent) / 100.0,
            dots_w: frame.raster.width,
            dots_h: frame.raster.height,
        }
    }

    /// Raw luminance 0..1 to dot intensity: threshold, gamma, gain.
    fn shape(&self, value: f32) -> f32 {
        let threshold = self.settings.threshold_percent as f32 / 100.0;
        let lifted = ((value - threshold) / (1.0 - threshold)).max(0.0);
        lifted.powf(self.settings.gamma_percent as f32 / 100.0)
            * self.settings.brightness_percent as f32
            / 100.0
    }
}

impl Scene for NightLightsScene {
    fn render(&mut self, frame: &mut Frame<'_>) {
        let (width, height) = (frame.raster.width, frame.raster.height);
        self.ensure_worker(width, height);
        self.drain_updates();
        if width == 0 || height == 0 {
            return;
        }
        let view = self.view(frame);
        let stale = self
            .located
            .as_ref()
            .is_none_or(|(known, table)| *known != view || table.len() != width * height);
        if stale {
            let table = (0..height)
                .flat_map(|y| (0..width).map(move |x| (x, y)))
                .map(|(x, y)| view.locate(x, y))
                .collect();
            self.located = Some((view, table));
        }
        let Some((_, table)) = self.located.as_ref() else {
            return;
        };
        let degrees_per_dot = view.degrees_per_dot();
        let settings = &self.settings;
        let glow = settings.glow_percent as f32 / 100.0;
        let sun = settings
            .terminator
            .then(|| sun_position(UtcTime::from_system_time(frame.now).0 as f64));
        let dimming = settings.terminator_strength_percent as f32 / 100.0;
        for y in 0..height {
            for x in 0..width {
                let index = y * width + x;
                let Some((lon, lat)) = table[index] else {
                    if settings.projection == Projection::Globe && view.globe_ring(x, y) {
                        frame.raster.dots[index] = 0.13;
                    }
                    continue;
                };
                let mut value = match self.mosaic.as_ref() {
                    Some(mosaic) => {
                        let stretch = view.limb_stretch(x, y) as f32;
                        let texels_per_dot =
                            (mosaic.base().texels_per_degree() * degrees_per_dot) as f32 * stretch;
                        let fine = mosaic.sample(lon, lat, texels_per_dot).unwrap_or(0.0);
                        let mut shaped = self.shape(fine);
                        if glow > 0.0 {
                            let halo = mosaic.sample(lon, lat, texels_per_dot * 5.0).unwrap_or(0.0);
                            shaped += glow * 0.7 * self.shape(halo);
                        }
                        shaped
                    }
                    None => projection::graticule_value(lon, lat, degrees_per_dot),
                };
                if let Some(sun) = &sun {
                    value *= 1.0 - dimming * sun.daylight(lon, lat);
                }
                frame.raster.dots[index] = value.clamp(0.0, 1.0);
            }
        }
        if settings.coastline {
            self.draw_coastline(frame, width, height);
        }
        if settings.marker {
            self.draw_marker(frame, &view);
        }
    }

    fn frames_per_second(&self) -> u32 {
        let settings = &self.settings;
        if settings.projection == Projection::Globe && settings.rotation_deg_per_min > 0 {
            15
        } else if settings.marker || settings.terminator {
            8
        } else {
            4
        }
    }

    fn status(&self) -> Option<String> {
        if let Some(progress) = &self.progress {
            return Some(progress.clone());
        }
        match (&self.problem, &self.label) {
            (Some(problem), Some(label)) => Some(format!("Offline ({problem}); showing {label}")),
            (Some(problem), None) => Some(format!("No imagery yet: {problem}")),
            (None, Some(label)) => Some(label.clone()),
            (None, None) => Some("Preparing night imagery".to_owned()),
        }
    }
}

impl NightLightsScene {
    /// Coast dots: where land/sea changes between neighbouring visible dots.
    fn draw_coastline(&self, frame: &mut Frame<'_>, width: usize, height: usize) {
        let Some((_, table)) = self.located.as_ref() else {
            return;
        };
        let land: Vec<Option<bool>> = table
            .iter()
            .map(|point| point.map(|(lon, lat)| (self.land)(lon, lat)))
            .collect();
        let strength = self.settings.coastline_strength_percent as f32 / 100.0 * 0.8;
        for y in 0..height {
            for x in 0..width {
                let Some(here) = land[y * width + x] else {
                    continue;
                };
                let differs =
                    |other: Option<Option<bool>>| matches!(other, Some(Some(o)) if o != here);
                let right = (x + 1 < width).then(|| land[y * width + x + 1]);
                let below = (y + 1 < height).then(|| land[(y + 1) * width + x]);
                if differs(right) || differs(below) {
                    let dot = &mut frame.raster.dots[y * width + x];
                    *dot = dot.max(strength);
                }
            }
        }
    }

    /// A blinking dot with a ring at the shared location.
    fn draw_marker(&self, frame: &mut Frame<'_>, view: &View) {
        let Some((cx, cy)) = view.project(self.location.longitude, self.location.latitude) else {
            return;
        };
        let (width, height) = (frame.raster.width, frame.raster.height);
        projection::draw_marker(
            &mut frame.raster.dots,
            (width, height),
            (cx, cy),
            frame.wall.as_secs_f32(),
        );
    }
}

#[cfg(test)]
mod tests;

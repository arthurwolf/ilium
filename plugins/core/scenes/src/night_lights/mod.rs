//! Earth at night: the light seen from orbit, on a borderless map.
//!
//! Imagery comes from NASA GIBS (keyless, WMTS EPSG:4326, TileMatrixSet `500m`, see
//! `kit::geo_tiles` for documentation and terms):
//! * `VIIRS_NOAA20_GapFilled_BRDF_Corrected_DayNightBand_Radiance`, a daily, near-global,
//!   gap-filled night radiance product (the "live" source; the newest day is usually
//!   yesterday), and
//! * `VIIRS_Black_Marble` (2016 composite), the static fallback with complete coverage.
//!
//! The scene owns no thread: a `Loader` state machine, advanced once per frame, probes for the
//! newest day, downloads a bounded set of tiles (15 at level 2, at most 50 at level 3) through
//! the host http import, mosaics them into one luminance map and refreshes every few hours.
//! Tiles are cached in the plugin's shared storage bucket. `render` only resamples the last good
//! mosaic to the dot raster.

mod loader;
mod settings;
#[cfg(test)]
mod tests;

pub use loader::{HostImagery, Imagery, Loader, Update};
pub use settings::{Detail, NightLightsSettings, NightSource};

use crate::control::SceneSettings;
use crate::kit::geo_projection::{
    self as projection, graticule_value, sun_position, DotTable, Projection, View,
};
use crate::kit::geo_tiles::{MipGrid, UtcTime};
use crate::scene::{Frame, Scene, SceneEnv, SceneLocation};
use std::sync::Arc;
use std::time::Duration;

/// The land mask of the coastline: `true` over land.
type LandFn = Box<dyn Fn(f64, f64) -> bool + Send>;

/// Builds the scene from a spec: `settings` holds the scene's settings and `location` the shared
/// observer position (already in `env`).
pub fn create(spec: &serde_json::Value, env: &SceneEnv) -> Box<dyn Scene> {
    let settings: NightLightsSettings = crate::kit::settings::from_spec(spec);
    Box::new(NightLightsScene::new(&settings, env))
}

pub struct NightLightsScene {
    settings: NightLightsSettings,
    location: SceneLocation,
    imagery: Option<Box<dyn Imagery>>,
    loader: Option<Loader>,
    land: LandFn,
    mosaic: Option<Arc<MipGrid>>,
    label: Option<String>,
    progress: Option<String>,
    problem: Option<String>,
    located: Option<(View, DotTable)>,
}

impl NightLightsScene {
    // PALETTE (future plugin contract): `env.palette` is the shared look's current palette. A
    // scene with natural colours follows it with `ScenePalette::recolor`/`at`, and
    // `Scene::set_palette` delivers later changes. This scene is monochrome (luminance only),
    // so it ignores the palette: the host's shared look recolours its dots.
    pub fn new(settings: &NightLightsSettings, env: &SceneEnv) -> Self {
        Self::with_parts(
            settings,
            env,
            Box::<HostImagery>::default(),
            Box::new(crate::worldmap::is_land),
        )
    }

    /// Construction with injected imagery and land mask (tests).
    pub(crate) fn with_parts(
        settings: &NightLightsSettings,
        env: &SceneEnv,
        imagery: Box<dyn Imagery>,
        land: LandFn,
    ) -> Self {
        Self {
            settings: settings.normalized(),
            location: env.location,
            imagery: Some(imagery),
            loader: None,
            land,
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

    /// Starts the loader at the first frame that has a size, and advances it by one step.
    fn advance_loader(&mut self, frame: &Frame<'_>, dots_w: usize, dots_h: usize) {
        if self.loader.is_none() {
            if dots_w == 0 || dots_h == 0 {
                return;
            }
            let Some(imagery) = self.imagery.take() else {
                return;
            };
            self.loader = Some(Loader::new(
                imagery,
                self.settings.source,
                self.choose_level(dots_w, dots_h),
                Duration::from_secs(self.settings.refresh_hours as u64 * 3600),
            ));
        }
        let updates = match self.loader.as_mut() {
            Some(loader) => loader.step(frame.now),
            None => return,
        };
        for update in updates {
            self.apply(update);
        }
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
        self.advance_loader(frame, width, height);
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
                    None => graticule_value(lon, lat, degrees_per_dot),
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
        } else if self.progress.is_some() || self.loader.is_none() {
            // The loader advances one bounded step per frame: load briskly.
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

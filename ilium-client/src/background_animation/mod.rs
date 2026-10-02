//! Deterministic dot scenes. Decoration never enters PTY/source state.

use ilium_ambient::{AmbientKind, AmbientSettings};
use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime};

mod cache;
mod controls;
mod host;
mod parameters;
mod raster;
mod scenes;
mod shoreline;
#[cfg(test)]
pub(crate) mod test_support;
#[cfg(test)]
mod tests;

pub use cache::{AnimationCacheStatus, AnimationLoopCache};
#[cfg(test)]
pub(crate) use controls::test_controls;
pub use controls::{common_control_ids, LEGACY_CONTROL_IDS};
pub use host::{AmbientHost, SceneFactory};
pub use ilium_ambient::raster::DitherMode;

pub use parameters::{
    slider_thumb_offset, slider_value_at, CloudletSettings, KelpSettings, MoonlitWaterSettings,
    QuietPondSettings, SleepingRidgeSettings, Slider, StoneCausticsSettings, TeaSteamSettings,
    TwoRipplesSettings, WindyHillsideSettings,
};
pub use shoreline::{ShorelineSettings, ShorelineStyle};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnimationKind {
    #[default]
    Shoreline,
    MoonlitWater,
    SleepingRidge,
    WindyHillside,
    TeaSteam,
    Kelp,
    StoneCaustics,
    Cloudlets,
    TwoRipples,
    #[serde(alias = "breathing_mountain")]
    QuietPond,
    Pipes,
    Stars,
    NightLights,
    Clouds,
    Video,
    Spectrum,
    Images,
    DitherWater,
    AtlanticDusk,
    CubeClock,
    BoxMachine,
    MachineScreen,
    FbmClouds,
    DitheredWaves,
    DithrPatterns,
    SolarSystem,
}

impl AnimationKind {
    pub const ALL: [Self; 26] = [
        Self::Shoreline,
        Self::MoonlitWater,
        Self::SleepingRidge,
        Self::WindyHillside,
        Self::TeaSteam,
        Self::Kelp,
        Self::StoneCaustics,
        Self::Cloudlets,
        Self::TwoRipples,
        Self::QuietPond,
        Self::Pipes,
        Self::Stars,
        Self::NightLights,
        Self::Clouds,
        Self::Video,
        Self::Spectrum,
        Self::Images,
        Self::DitherWater,
        Self::AtlanticDusk,
        Self::CubeClock,
        Self::BoxMachine,
        Self::MachineScreen,
        Self::FbmClouds,
        Self::DitheredWaves,
        Self::DithrPatterns,
        Self::SolarSystem,
    ];

    /// The hosted `ilium-ambient` engine behind this kind, or `None` for the
    /// deterministic built-in scenes rendered by `scenes.rs`.
    pub const fn ambient(self) -> Option<AmbientKind> {
        match self {
            Self::SolarSystem => Some(AmbientKind::SolarSystem),
            Self::Pipes => Some(AmbientKind::Pipes),
            Self::Stars => Some(AmbientKind::Stars),
            Self::NightLights => Some(AmbientKind::NightLights),
            Self::Clouds => Some(AmbientKind::Clouds),
            Self::Video => Some(AmbientKind::Video),
            Self::Spectrum => Some(AmbientKind::Spectrum),
            Self::Images => Some(AmbientKind::Images),
            Self::DitherWater => Some(AmbientKind::DitherWater),
            Self::AtlanticDusk => Some(AmbientKind::AtlanticDusk),
            Self::CubeClock => Some(AmbientKind::CubeClock),
            Self::BoxMachine => Some(AmbientKind::BoxMachine),
            Self::MachineScreen => Some(AmbientKind::MachineScreen),
            Self::FbmClouds => Some(AmbientKind::FbmClouds),
            Self::DitheredWaves => Some(AmbientKind::DitheredWaves),
            Self::DithrPatterns => Some(AmbientKind::DithrPatterns),
            _ => None,
        }
    }

    /// URLs that inspired the scene, shown only in the Animations demo.
    pub fn inspired_by(self) -> &'static [&'static str] {
        self.ambient().map_or(&[], AmbientKind::inspired_by)
    }

    pub const fn is_ambient(self) -> bool {
        self.ambient().is_some()
    }

    /// Live-only scenes are never precomputed into the loop cache: their
    /// output depends on data, processes or the wall clock, not just time.
    pub fn is_live_only(self) -> bool {
        self.ambient().is_some_and(AmbientKind::is_live_only)
    }

    pub fn label(self) -> &'static str {
        if let Some(kind) = self.ambient() {
            return kind.label();
        }
        match self {
            Self::Shoreline => "Wave washing up sand",
            Self::MoonlitWater => "Moon over moving water",
            Self::SleepingRidge => "Clouds over a sleeping ridge",
            Self::WindyHillside => "Hillside brushed by wind",
            Self::TeaSteam => "Steam above a tea cup",
            Self::Kelp => "Kelp in a gentle current",
            Self::StoneCaustics => "Water caustics on stone",
            Self::Cloudlets => "Drifting cloud islands",
            Self::TwoRipples => "Two gentle wave sources",
            Self::QuietPond => "Lily pads on a quiet pond",
            _ => unreachable!("ambient kinds return before this match"),
        }
    }

    pub fn description(self) -> &'static str {
        if let Some(kind) = self.ambient() {
            return kind.description();
        }
        match self {
            Self::Shoreline => "A diagonal wash with fine foam, wet sand and scattered grains.",
            Self::MoonlitWater => "Crossing wavelets fracture a widening moonlit reflection.",
            Self::SleepingRidge => "Layered clouds and valley mist drift over quiet ridges.",
            Self::WindyHillside => {
                "A dense meadow of fine blades and seed heads follows traveling gusts."
            }
            Self::TeaSteam => "Fine translucent wisps curl above a rounded porcelain cup.",
            Self::Kelp => "Uneven clusters of tapering ribbons twist through layered currents.",
            Self::StoneCaustics => {
                "A moving light web bends over domed stones, pebbles and sparse sand."
            }
            Self::Cloudlets => "Soft cloud islands gather, join and separate as they drift.",
            Self::TwoRipples => {
                "Continuous outward crests brighten and dim where two wave fields meet."
            }
            Self::QuietPond => {
                "Notched lily pads rest on a pond while fine reflections pass beneath."
            }
            _ => unreachable!("ambient kinds return before this match"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AnimationPlaybackMode {
    #[default]
    Loop,
    Live,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AnimationSettings {
    pub enabled: bool,
    pub kind: AnimationKind,
    pub playback_mode: AnimationPlaybackMode,
    pub loop_seconds: u16,
    pub speed_percent: u16,
    pub density_percent: u16,
    pub dither: DitherMode,
    pub lightness_percent: u16,
    pub hue_degrees: u16,
    pub saturation_percent: u16,
    pub shoreline: ShorelineSettings,
    pub moonlit_water: MoonlitWaterSettings,
    pub sleeping_ridge: SleepingRidgeSettings,
    pub windy_hillside: WindyHillsideSettings,
    pub tea_steam: TeaSteamSettings,
    pub kelp: KelpSettings,
    pub stone_caustics: StoneCausticsSettings,
    pub cloudlets: CloudletSettings,
    pub two_ripples: TwoRipplesSettings,
    pub quiet_pond: QuietPondSettings,
    /// Settings of the hosted `ilium-ambient` scenes and the shared observer
    /// location. Flattened so the project YAML stays one flat mapping.
    #[serde(flatten)]
    pub ambient: AmbientSettings,
}

impl Default for AnimationSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            kind: AnimationKind::default(),
            playback_mode: AnimationPlaybackMode::Loop,
            loop_seconds: 60,
            speed_percent: 100,
            density_percent: 60,
            dither: DitherMode::default(),
            lightness_percent: 60,
            hue_degrees: 210,
            saturation_percent: 0,
            shoreline: Default::default(),
            moonlit_water: Default::default(),
            sleeping_ridge: Default::default(),
            windy_hillside: Default::default(),
            tea_steam: Default::default(),
            kelp: Default::default(),
            stone_caustics: Default::default(),
            cloudlets: Default::default(),
            two_ripples: Default::default(),
            quiet_pond: Default::default(),
            ambient: Default::default(),
        }
    }
}

impl AnimationSettings {
    pub fn normalized(&self) -> Self {
        Self {
            loop_seconds: self.loop_seconds.clamp(1, 120),
            speed_percent: self.speed_percent.clamp(25, 300),
            density_percent: self.density_percent.clamp(25, 100),
            lightness_percent: self.lightness_percent.min(100),
            hue_degrees: self.hue_degrees.min(359),
            saturation_percent: self.saturation_percent.min(100),
            shoreline: self.shoreline.normalized(),
            moonlit_water: self.moonlit_water.normalized(),
            sleeping_ridge: self.sleeping_ridge.normalized(),
            windy_hillside: self.windy_hillside.normalized(),
            tea_steam: self.tea_steam.normalized(),
            kelp: self.kelp.normalized(),
            stone_caustics: self.stone_caustics.normalized(),
            cloudlets: self.cloudlets.normalized(),
            two_ripples: self.two_ripples.normalized(),
            quiet_pond: self.quiet_pond.normalized(),
            ambient: self.ambient.normalized(),
            ..self.clone()
        }
    }

    pub fn estimated_loop_bytes(&self, width: u16, height: u16) -> usize {
        usize::from(width)
            .saturating_mul(usize::from(height))
            .saturating_mul(cache::CACHE_FPS as usize)
            .saturating_mul(usize::from(self.loop_seconds.clamp(1, 120)))
    }

    /// The four named sliders of a built-in scene. Hosted ambient kinds have
    /// no such sliders (their rows come from `AmbientSettings::controls`), so
    /// they report four inert placeholders that never enter a cache key.
    pub fn scene_sliders(&self) -> [Slider; 4] {
        match self.kind {
            AnimationKind::Shoreline => self.shoreline.sliders(),
            AnimationKind::MoonlitWater => self.moonlit_water.sliders(),
            AnimationKind::SleepingRidge => self.sleeping_ridge.sliders(),
            AnimationKind::WindyHillside => self.windy_hillside.sliders(),
            AnimationKind::TeaSteam => self.tea_steam.sliders(),
            AnimationKind::Kelp => self.kelp.sliders(),
            AnimationKind::StoneCaustics => self.stone_caustics.sliders(),
            AnimationKind::Cloudlets => self.cloudlets.sliders(),
            AnimationKind::TwoRipples => self.two_ripples.sliders(),
            AnimationKind::QuietPond => self.quiet_pond.sliders(),
            _ => [Slider::new("", 0, 0, 0, 1, ""); 4],
        }
    }

    /// The full shoreline block when it is the selected scene: its Rich
    /// controls are more than the four sliders a render key otherwise holds.
    pub(super) fn scene_shoreline_key(&self) -> Option<ShorelineSettings> {
        (self.kind == AnimationKind::Shoreline).then_some(self.shoreline)
    }

    /// True when the loop cache may serve this configuration: Loop playback
    /// of a deterministic built-in scene. Live-only kinds always render live.
    pub fn uses_loop_cache(&self) -> bool {
        self.playback_mode == AnimationPlaybackMode::Loop && !self.kind.is_live_only()
    }

    /// Both visible surfaces use exactly the same HSL ink. Palette changes
    /// never change dot coverage, geometry, dithering or terminal source text.
    pub fn foreground_rgb(&self) -> (u8, u8, u8) {
        let lightness = f32::from(self.lightness_percent.min(100)) / 100.0;
        let saturation = f32::from(self.saturation_percent.min(100)) / 100.0;
        let sector = f32::from(self.hue_degrees.min(359)) / 60.0;
        let chroma = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
        let secondary = chroma * (1.0 - (sector % 2.0 - 1.0).abs());
        let (red, green, blue) = match sector as u16 {
            0 => (chroma, secondary, 0.0),
            1 => (secondary, chroma, 0.0),
            2 => (0.0, chroma, secondary),
            3 => (0.0, secondary, chroma),
            4 => (secondary, 0.0, chroma),
            _ => (chroma, 0.0, secondary),
        };
        let offset = lightness - chroma / 2.0;
        let channel = |component: f32| ((component + offset) * 255.0).round() as u8;
        (channel(red), channel(green), channel(blue))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FrameKey {
    kind: AnimationKind,
    controls: [u16; 4],
    shoreline: Option<ShorelineSettings>,
    quiet_pond: Option<QuietPondSettings>,
    speed_percent: u16,
    width: u16,
    height: u16,
    elapsed: Duration,
}

/// Identity of the last hosted-scene render. An input redraw inside one frame
/// bucket reuses the rendered raster instead of advancing the scene twice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AmbientRenderKey {
    generation: u64,
    speed_percent: u16,
    width: u16,
    height: u16,
    elapsed: Duration,
}

/// One screen-sized field: a Braille cell grid plus, for hosted scenes that
/// supply them, one RGB color per cell. It also owns the hosted scene (through
/// `AmbientHost`), so the ambient background and the Settings preview render
/// the very same scene instance.
#[derive(Debug, Default)]
pub struct AnimationFrame {
    width: u16,
    height: u16,
    raster: raster::Raster,
    cells: Vec<u8>,
    scene_cache: scenes::SceneCache,
    last_geometry: Option<FrameKey>,
    #[cfg(test)]
    pub(crate) geometry_render_count: usize,
    last_pack: Option<(u16, DitherMode)>,
    thresholds: Vec<f32>,
    threshold_key: Option<(u16, u16, DitherMode)>,
    colors: Vec<[u8; 3]>,
    has_cell_colors: bool,
    host: AmbientHost,
    last_ambient: Option<AmbientRenderKey>,
}

impl AnimationFrame {
    pub fn width(&self) -> u16 {
        self.width
    }
    pub fn height(&self) -> u16 {
        self.height
    }

    /// The scene host shared by every surface that shows this field.
    pub fn host(&self) -> &AmbientHost {
        &self.host
    }

    pub fn host_mut(&mut self) -> &mut AmbientHost {
        &mut self.host
    }

    fn resize(&mut self, width: u16, height: u16) {
        self.width = width;
        self.height = height;
        self.raster
            .resize(usize::from(width) * 2, usize::from(height) * 4);
        self.cells
            .resize(usize::from(width) * usize::from(height), 0);
        self.colors
            .resize(usize::from(width) * usize::from(height), [0; 3]);
    }

    /// `enabled` is a compositor concern. Presentation-only changes reuse
    /// geometry; density/dither repack it without resimulating the scene.
    pub fn render(
        &mut self,
        settings: &AnimationSettings,
        width: u16,
        height: u16,
        elapsed: Duration,
    ) {
        let settings = settings.normalized();
        if let Some(kind) = settings.kind.ambient() {
            self.render_ambient(kind, &settings, width, height, elapsed);
            return;
        }
        // A built-in scene owns nothing: drop any hosted scene (and with it
        // its threads and child processes) the moment the kind changes.
        self.host.release();
        self.has_cell_colors = false;
        self.last_ambient = None;
        let key = FrameKey {
            kind: settings.kind,
            controls: settings.scene_sliders().map(|slider| slider.value),
            shoreline: settings.scene_shoreline_key(),
            quiet_pond: (settings.kind == AnimationKind::QuietPond).then_some(settings.quiet_pond),
            speed_percent: settings.speed_percent,
            width,
            height,
            elapsed,
        };
        let geometry_changed = self.last_geometry != Some(key);
        if geometry_changed {
            #[cfg(test)]
            {
                self.geometry_render_count += 1;
            }
            if width != self.width || height != self.height {
                self.resize(width, height);
            } else {
                self.raster.dots.fill(0.0);
            }
            if width > 0 && height > 0 {
                let seconds = elapsed.as_secs_f64() * f64::from(settings.speed_percent) / 100.0;
                scenes::render(&mut self.raster, &mut self.scene_cache, &settings, seconds);
            }
            self.last_geometry = Some(key);
        }
        if width == 0 || height == 0 {
            return;
        }
        if geometry_changed || self.last_pack != Some((settings.density_percent, settings.dither)) {
            self.pack(settings.density_percent, settings.dither);
            self.last_pack = Some((settings.density_percent, settings.dither));
        }
    }

    /// Renders a hosted `ilium-ambient` scene. `elapsed` is the shared,
    /// already-quantized session clock; the host converts it to scene time.
    fn render_ambient(
        &mut self,
        kind: AmbientKind,
        settings: &AnimationSettings,
        width: u16,
        height: u16,
        elapsed: Duration,
    ) {
        // The raster is about to hold hosted-scene pixels, not the built-in
        // scene the geometry key describes.
        self.last_geometry = None;
        let generation = self.host.sync(kind, &settings.ambient, elapsed);
        let key = AmbientRenderKey {
            generation,
            speed_percent: settings.speed_percent,
            width,
            height,
            elapsed,
        };
        let is_reused = self.last_ambient == Some(key);
        if !is_reused {
            if width != self.width || height != self.height {
                self.resize(width, height);
            } else {
                self.raster.dots.fill(0.0);
            }
            self.colors.fill([0; 3]);
            if width > 0 && height > 0 {
                let wall = self.host.wall(elapsed);
                let time = wall.mul_f64(f64::from(settings.speed_percent) / 100.0);
                let mut frame = ilium_ambient::Frame {
                    raster: &mut self.raster,
                    cell_colors: &mut self.colors,
                    width,
                    height,
                    time,
                    wall,
                    now: SystemTime::now(),
                };
                self.host.render(&mut frame);
            }
            self.last_ambient = Some(key);
        }
        self.has_cell_colors = self.host.uses_cell_colors();
        if width == 0 || height == 0 {
            return;
        }
        if !is_reused || self.last_pack != Some((settings.density_percent, settings.dither)) {
            self.pack(settings.density_percent, settings.dither);
            self.last_pack = Some((settings.density_percent, settings.dither));
        }
    }

    pub fn glyph(&self, x: u16, y: u16) -> char {
        if x >= self.width || y >= self.height {
            return ' ';
        }
        let bits = self.cells[usize::from(y) * usize::from(self.width) + usize::from(x)];
        if bits == 0 {
            ' '
        } else {
            char::from_u32(0x2800 + u32::from(bits)).unwrap_or(' ')
        }
    }

    /// True when the current field carries scene-supplied per-cell colors.
    pub fn has_cell_colors(&self) -> bool {
        self.has_cell_colors
    }

    /// The scene-supplied color of one cell; `None` when the scene paints in
    /// the user's palette instead (the compositor then uses that palette).
    pub fn cell_color(&self, x: u16, y: u16) -> Option<(u8, u8, u8)> {
        if !self.has_cell_colors || x >= self.width || y >= self.height {
            return None;
        }
        self.colors
            .get(usize::from(y) * usize::from(self.width) + usize::from(x))
            .map(|[red, green, blue]| (*red, *green, *blue))
    }

    pub(crate) fn packed_cells(&self) -> &[u8] {
        &self.cells
    }

    pub(crate) fn load_packed_cells(&mut self, width: u16, height: u16, cells: &[u8]) {
        // Cached built-in frames replace any hosted scene and its worker ownership.
        self.host.release();
        if width != self.width || height != self.height {
            self.resize(width, height);
        }
        self.cells.fill(0);
        let count = self.cells.len().min(cells.len());
        self.cells[..count].copy_from_slice(&cells[..count]);
        self.has_cell_colors = false;
        self.last_geometry = None;
        self.last_pack = None;
        self.last_ambient = None;
    }

    fn pack(&mut self, density_percent: u16, dither: DitherMode) {
        const BITS: [[u8; 2]; 4] = [[1, 8], [2, 16], [4, 32], [64, 128]];
        let threshold_key = (self.width, self.height, dither);
        if self.threshold_key != Some(threshold_key) {
            self.thresholds.resize(self.raster.dots.len(), 0.0);
            for y in 0..self.raster.height {
                for x in 0..self.raster.width {
                    self.thresholds[y * self.raster.width + x] = raster::threshold(x, y, dither);
                }
            }
            self.threshold_key = Some(threshold_key);
        }
        let width = usize::from(self.width);
        if width == 0 || self.height == 0 {
            return;
        }
        let density = f32::from(density_percent) / 100.0;
        let dot_row_width = width * 2;
        // Each cell row consumes four contiguous dot rows. Two-dot chunks
        // retain the Braille bit order without per-dot coordinate arithmetic.
        for ((dots, thresholds), cells) in self
            .raster
            .dots
            .chunks_exact(dot_row_width * 4)
            .zip(self.thresholds.chunks_exact(dot_row_width * 4))
            .zip(self.cells.chunks_exact_mut(width))
        {
            cells.fill(0);
            for ((dot_row, threshold_row), bits) in dots
                .chunks_exact(dot_row_width)
                .zip(thresholds.chunks_exact(dot_row_width))
                .zip(BITS)
            {
                for ((cell, pair), threshold_pair) in cells
                    .iter_mut()
                    .zip(dot_row.chunks_exact(2))
                    .zip(threshold_row.chunks_exact(2))
                {
                    *cell |= (u8::from(pair[0] * density > threshold_pair[0]) * bits[0])
                        | (u8::from(pair[1] * density > threshold_pair[1]) * bits[1]);
                }
            }
        }
    }
}

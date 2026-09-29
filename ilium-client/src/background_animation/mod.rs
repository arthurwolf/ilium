//! Deterministic dot scenes. Decoration never enters PTY/source state.

use serde::{Deserialize, Serialize};
use std::time::Duration;

mod parameters;
mod raster;
mod scenes;
#[cfg(test)]
mod tests;

pub use parameters::{
    CloudletSettings, KelpSettings, MoonlitWaterSettings, QuietPondSettings, ShorelineSettings,
    SleepingRidgeSettings, Slider, StoneCausticsSettings, TeaSteamSettings, TwoRipplesSettings,
    WindyHillsideSettings,
};

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
}

impl AnimationKind {
    pub const ALL: [Self; 10] = [
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
    ];

    pub fn label(self) -> &'static str {
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
        }
    }

    pub fn description(self) -> &'static str {
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
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DitherMode {
    #[default]
    Ordered,
    Stippled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AnimationSettings {
    pub enabled: bool,
    pub kind: AnimationKind,
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
}

impl Default for AnimationSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            kind: AnimationKind::default(),
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
        }
    }
}

impl AnimationSettings {
    pub fn normalized(self) -> Self {
        Self {
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
            ..self
        }
    }

    pub fn scene_sliders(self) -> [Slider; 4] {
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
        }
    }

    pub fn slider(self, row: usize) -> Option<Slider> {
        match row {
            11 => Some(Slider::new("Speed", self.speed_percent, 25, 300, 5, "%")),
            12 => Some(Slider::new(
                "Dot density",
                self.density_percent,
                25,
                100,
                5,
                "%",
            )),
            14 => Some(Slider::new(
                "Lightness",
                self.lightness_percent,
                0,
                100,
                1,
                "%",
            )),
            15 => Some(Slider::new("Hue", self.hue_degrees, 0, 359, 1, "°")),
            16 => Some(Slider::new(
                "Saturation",
                self.saturation_percent,
                0,
                100,
                1,
                "%",
            )),
            17..=20 => Some(self.scene_sliders()[row - 17]),
            _ => None,
        }
    }

    pub fn set_slider_value(&mut self, row: usize, value: u16) -> bool {
        let Some(slider) = self.slider(row) else {
            return false;
        };
        let value = value.clamp(slider.minimum, slider.maximum);
        let before = *self;
        match row {
            11 => self.speed_percent = value,
            12 => self.density_percent = value,
            14 => self.lightness_percent = value,
            15 => self.hue_degrees = value,
            16 => self.saturation_percent = value,
            17..=20 => match self.kind {
                AnimationKind::Shoreline => self.shoreline.set(row - 17, value),
                AnimationKind::MoonlitWater => self.moonlit_water.set(row - 17, value),
                AnimationKind::SleepingRidge => self.sleeping_ridge.set(row - 17, value),
                AnimationKind::WindyHillside => self.windy_hillside.set(row - 17, value),
                AnimationKind::TeaSteam => self.tea_steam.set(row - 17, value),
                AnimationKind::Kelp => self.kelp.set(row - 17, value),
                AnimationKind::StoneCaustics => self.stone_caustics.set(row - 17, value),
                AnimationKind::Cloudlets => self.cloudlets.set(row - 17, value),
                AnimationKind::TwoRipples => self.two_ripples.set(row - 17, value),
                AnimationKind::QuietPond => self.quiet_pond.set(row - 17, value),
            },
            _ => return false,
        }
        *self != before
    }

    pub fn adjust_row(&mut self, row: usize, direction: i32) -> bool {
        if let Some(slider) = self.slider(row) {
            return self.set_slider_value(row, slider.adjusted(direction));
        }
        let before = *self;
        match row {
            0..=9 => self.kind = AnimationKind::ALL[row],
            10 => self.enabled = !self.enabled,
            13 => {
                self.dither = match self.dither {
                    DitherMode::Ordered => DitherMode::Stippled,
                    DitherMode::Stippled => DitherMode::Ordered,
                }
            }
            _ => return false,
        }
        *self != before
    }

    /// Both visible surfaces use exactly the same HSL ink. Palette changes
    /// never change dot coverage, geometry, dithering or terminal source text.
    pub fn foreground_rgb(self) -> (u8, u8, u8) {
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
    speed_percent: u16,
    width: u16,
    height: u16,
    elapsed: Duration,
}

#[derive(Debug, Default)]
pub struct AnimationFrame {
    width: u16,
    height: u16,
    raster: raster::Raster,
    cells: Vec<u8>,
    scene_cache: scenes::SceneCache,
    last_geometry: Option<FrameKey>,
    last_pack: Option<(u16, DitherMode)>,
    thresholds: Vec<f32>,
    threshold_key: Option<(u16, u16, DitherMode)>,
}

impl AnimationFrame {
    pub fn width(&self) -> u16 {
        self.width
    }
    pub fn height(&self) -> u16 {
        self.height
    }

    fn resize(&mut self, width: u16, height: u16) {
        self.width = width;
        self.height = height;
        self.raster
            .resize(usize::from(width) * 2, usize::from(height) * 4);
        self.cells
            .resize(usize::from(width) * usize::from(height), 0);
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
        let key = FrameKey {
            kind: settings.kind,
            controls: settings.scene_sliders().map(|slider| slider.value),
            speed_percent: settings.speed_percent,
            width,
            height,
            elapsed,
        };
        let geometry_changed = self.last_geometry != Some(key);
        if geometry_changed {
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
        let density = f32::from(density_percent) / 100.0;
        for y in 0..usize::from(self.height) {
            for x in 0..usize::from(self.width) {
                let mut cell = 0;
                for (dy, row) in BITS.iter().enumerate() {
                    for (dx, bit) in row.iter().enumerate() {
                        let index = (y * 4 + dy) * self.raster.width + x * 2 + dx;
                        if self.raster.dots[index] * density > self.thresholds[index] {
                            cell |= bit;
                        }
                    }
                }
                self.cells[y * usize::from(self.width) + x] = cell;
            }
        }
    }
}

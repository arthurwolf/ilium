//! The settings of the "Earth at night" scene and their Settings-panel rows.

use crate::control::{self, Control, ControlValue, SceneSettings};
use crate::kit::geo_projection::Projection;
use serde::{Deserialize, Serialize};

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
    pub(super) const LABELS: [&'static str; 2] = ["Latest daily (VIIRS)", "Black Marble 2016"];

    pub(super) fn index(self) -> usize {
        match self {
            Self::Daily => 0,
            Self::BlackMarble => 1,
        }
    }

    pub(super) fn from_index(index: usize) -> Self {
        if index == 1 {
            Self::BlackMarble
        } else {
            Self::Daily
        }
    }

    pub(super) fn id(self) -> &'static str {
        match self {
            Self::Daily => "daily",
            Self::BlackMarble => "black_marble",
        }
    }

    /// Luminance below this (0..255) is terrain glow or sensor floor, not light.
    pub(super) fn black_level(self) -> u8 {
        match self {
            Self::Daily => 10,
            Self::BlackMarble => 54,
        }
    }

    pub(super) fn name(self) -> &'static str {
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
    pub(super) const LABELS: [&'static str; 3] =
        ["Automatic", "Coarse (fewer tiles)", "Fine (more tiles)"];

    pub(super) fn index(self) -> usize {
        match self {
            Self::Auto => 0,
            Self::Coarse => 1,
            Self::Fine => 2,
        }
    }

    pub(super) fn from_index(index: usize) -> Self {
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
                "Hours between checks for a newer daily image. Downloaded tiles are cached in the plugin's storage.",
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

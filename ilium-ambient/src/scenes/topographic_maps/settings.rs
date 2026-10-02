//! User-facing settings of the topographic map scene. Serde field names are the
//! keys users see in the project config; `normalized` enforces every range.

use crate::control::{self, Control, ControlValue, SceneSettings};
use serde::{Deserialize, Serialize};

/// One drawable world: a measured planetary body or a generated fictional one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorldId {
    Earth,
    Moon,
    Mars,
    Venus,
    Mercury,
    Ceres,
    Aeria,
    Pangaea,
    Ridgeworld,
    Craterlands,
}

impl WorldId {
    pub const REAL: [Self; 6] = [
        Self::Earth,
        Self::Moon,
        Self::Mars,
        Self::Venus,
        Self::Mercury,
        Self::Ceres,
    ];
    pub const FICTIONAL: [Self; 4] = [
        Self::Aeria,
        Self::Pangaea,
        Self::Ridgeworld,
        Self::Craterlands,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Earth => "Earth",
            Self::Moon => "Moon",
            Self::Mars => "Mars",
            Self::Venus => "Venus",
            Self::Mercury => "Mercury",
            Self::Ceres => "Ceres",
            Self::Aeria => "Aeria (fictional archipelago)",
            Self::Pangaea => "Pangaea Prime (fictional supercontinent)",
            Self::Ridgeworld => "Ridgeworld (fictional mountain belts)",
            Self::Craterlands => "Craterlands (fictional airless world)",
        }
    }

    pub fn is_fictional(self) -> bool {
        Self::FICTIONAL.contains(&self)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BodyChoice {
    /// Every world in turn, real then fictional.
    #[default]
    CycleAll,
    CycleReal,
    CycleFictional,
    Earth,
    Moon,
    Mars,
    Venus,
    Mercury,
    Ceres,
    Aeria,
    Pangaea,
    Ridgeworld,
    Craterlands,
}

impl BodyChoice {
    pub const ALL: [Self; 13] = [
        Self::CycleAll,
        Self::CycleReal,
        Self::CycleFictional,
        Self::Earth,
        Self::Moon,
        Self::Mars,
        Self::Venus,
        Self::Mercury,
        Self::Ceres,
        Self::Aeria,
        Self::Pangaea,
        Self::Ridgeworld,
        Self::Craterlands,
    ];
    pub const LABELS: [&'static str; 13] = [
        "All worlds in turn",
        "Real worlds in turn",
        "Fictional worlds in turn",
        "Earth",
        "Moon",
        "Mars",
        "Venus",
        "Mercury",
        "Ceres",
        "Aeria (fictional)",
        "Pangaea Prime (fictional)",
        "Ridgeworld (fictional)",
        "Craterlands (fictional)",
    ];

    pub fn from_index(index: usize) -> Option<Self> {
        Self::ALL.get(index).copied()
    }

    pub fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|candidate| *candidate == self)
            .unwrap_or(0)
    }

    /// The worlds this choice shows, in display order.
    pub fn worlds(self) -> Vec<WorldId> {
        match self {
            Self::CycleAll => [WorldId::REAL.as_slice(), WorldId::FICTIONAL.as_slice()].concat(),
            Self::CycleReal => WorldId::REAL.to_vec(),
            Self::CycleFictional => WorldId::FICTIONAL.to_vec(),
            Self::Earth => vec![WorldId::Earth],
            Self::Moon => vec![WorldId::Moon],
            Self::Mars => vec![WorldId::Mars],
            Self::Venus => vec![WorldId::Venus],
            Self::Mercury => vec![WorldId::Mercury],
            Self::Ceres => vec![WorldId::Ceres],
            Self::Aeria => vec![WorldId::Aeria],
            Self::Pangaea => vec![WorldId::Pangaea],
            Self::Ridgeworld => vec![WorldId::Ridgeworld],
            Self::Craterlands => vec![WorldId::Craterlands],
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Projection {
    /// Equirectangular world map.
    #[default]
    Flat,
    /// Orthographic globe seen from space.
    Globe,
}

impl Projection {
    pub const LABELS: [&'static str; 2] = ["Flat map", "Globe"];
    pub fn from_index(index: usize) -> Option<Self> {
        [Self::Flat, Self::Globe].get(index).copied()
    }
    pub fn index(self) -> usize {
        usize::from(self == Self::Globe)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PanStyle {
    #[default]
    Wander,
    East,
    West,
    North,
    South,
    NorthEast,
    Still,
}

impl PanStyle {
    pub const ALL: [Self; 7] = [
        Self::Wander,
        Self::East,
        Self::West,
        Self::North,
        Self::South,
        Self::NorthEast,
        Self::Still,
    ];
    pub const LABELS: [&'static str; 7] = [
        "Wandering route",
        "East",
        "West",
        "North",
        "South",
        "North-east",
        "Still",
    ];
    pub fn from_index(index: usize) -> Option<Self> {
        Self::ALL.get(index).copied()
    }
    pub fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|candidate| *candidate == self)
            .unwrap_or(0)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BelowStyle {
    Solid,
    #[default]
    Dotted,
    Hidden,
}

impl BelowStyle {
    pub const LABELS: [&'static str; 3] = ["Solid", "Dotted", "Hidden"];
    pub fn from_index(index: usize) -> Option<Self> {
        [Self::Solid, Self::Dotted, Self::Hidden]
            .get(index)
            .copied()
    }
    pub fn index(self) -> usize {
        match self {
            Self::Solid => 0,
            Self::Dotted => 1,
            Self::Hidden => 2,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaletteChoice {
    /// One colour: the global Braille palette.
    Global,
    /// Blue below the zero level, green to white above it.
    Hypsometric,
    /// A colour scheme that fits the world (rust for Mars, grey for the Moon).
    #[default]
    Natural,
    Heat,
    Ice,
}

impl PaletteChoice {
    pub const LABELS: [&'static str; 5] = [
        "Global colour",
        "Hypsometric tints",
        "Natural for the world",
        "Heat",
        "Ice",
    ];
    pub fn from_index(index: usize) -> Option<Self> {
        [
            Self::Global,
            Self::Hypsometric,
            Self::Natural,
            Self::Heat,
            Self::Ice,
        ]
        .get(index)
        .copied()
    }
    pub fn index(self) -> usize {
        match self {
            Self::Global => 0,
            Self::Hypsometric => 1,
            Self::Natural => 2,
            Self::Heat => 3,
            Self::Ice => 4,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TopographicMapsSettings {
    pub body: BodyChoice,
    /// Seconds each world stays in a cycle, 20..=600.
    pub body_seconds: u32,
    /// Seed of the fictional worlds, 0..=9999.
    pub fictional_seed: u32,
    pub projection: Projection,
    /// Magnification in percent, 50..=1600. 100 fits the whole world.
    pub zoom_percent: u32,
    /// Contour lines across the full relief when no spacing is set, 6..=80.
    pub contour_levels: u32,
    /// Fixed contour spacing in metres, 0..=10000 (0 = from `contour_levels`).
    pub interval_m: u32,
    /// Every Nth contour is an emphasised index line, 0..=10 (0 = none).
    pub index_every: u32,
    /// Line thickness in Braille dots, 1..=3.
    pub line_thickness: u32,
    pub below_style: BelowStyle,
    /// Emphasise the zero-level (shore) line.
    pub coastline: bool,
    /// Draw the edge of the map or the limb of the globe.
    pub outline: bool,
    /// Relief shading in percent, 0..=100.
    pub shading_percent: u32,
    pub palette: PaletteChoice,
    pub pan: PanStyle,
    /// Pan speed in percent, 0..=400. 100 is about 5 dots per second.
    pub pan_speed: u32,
    /// Latitude the view starts at, -80..=80.
    pub center_latitude: i32,
    /// Longitude the view starts at, -180..=180.
    pub start_longitude: i32,
    /// Shift of the zero level in metres, -3000..=3000.
    pub zero_shift_m: i32,
    /// Peak-to-peak rise and fall of the zero level, 0..=2000 metres.
    pub tide_range_m: u32,
    /// Seconds for one full rise and fall, 10..=600.
    pub tide_seconds: u32,
}

impl Default for TopographicMapsSettings {
    fn default() -> Self {
        Self {
            body: BodyChoice::CycleAll,
            body_seconds: 75,
            fictional_seed: 7,
            projection: Projection::Flat,
            zoom_percent: 100,
            contour_levels: 16,
            interval_m: 0,
            index_every: 5,
            line_thickness: 1,
            below_style: BelowStyle::Dotted,
            coastline: true,
            outline: true,
            shading_percent: 0,
            palette: PaletteChoice::Natural,
            pan: PanStyle::Wander,
            pan_speed: 100,
            center_latitude: 15,
            start_longitude: 0,
            zero_shift_m: 0,
            tide_range_m: 0,
            tide_seconds: 120,
        }
    }
}

fn set_number<T: Copy + PartialEq>(
    field: &mut T,
    value: &ControlValue,
    range: (i32, i32),
    convert: impl Fn(i32) -> T,
) -> Result<bool, String> {
    let number = control::number(value).ok_or("Expected a number")?;
    let next = convert(number.clamp(range.0, range.1));
    let changed = *field != next;
    *field = next;
    Ok(changed)
}

fn set_choice<T: Copy + PartialEq>(
    field: &mut T,
    value: &ControlValue,
    from_index: impl Fn(usize) -> Option<T>,
) -> Result<bool, String> {
    let next = control::index(value)
        .and_then(from_index)
        .ok_or("Unknown option")?;
    let changed = *field != next;
    *field = next;
    Ok(changed)
}

fn set_toggle(field: &mut bool, value: &ControlValue) -> Result<bool, String> {
    let next = control::boolean(value).ok_or("Expected on or off")?;
    let changed = *field != next;
    *field = next;
    Ok(changed)
}

impl SceneSettings for TopographicMapsSettings {
    fn normalized(&self) -> Self {
        Self {
            body_seconds: self.body_seconds.clamp(20, 600),
            fictional_seed: self.fictional_seed.min(9999),
            zoom_percent: self.zoom_percent.clamp(50, 1600),
            contour_levels: self.contour_levels.clamp(6, 80),
            interval_m: self.interval_m.min(10_000),
            index_every: self.index_every.min(10),
            line_thickness: self.line_thickness.clamp(1, 3),
            shading_percent: self.shading_percent.min(100),
            pan_speed: self.pan_speed.min(400),
            center_latitude: self.center_latitude.clamp(-80, 80),
            start_longitude: self.start_longitude.clamp(-180, 180),
            zero_shift_m: self.zero_shift_m.clamp(-3000, 3000),
            tide_range_m: self.tide_range_m.min(2000),
            tide_seconds: self.tide_seconds.clamp(10, 600),
            ..self.clone()
        }
    }

    fn controls(&self) -> Vec<Control> {
        let mut rows = vec![
            Control::choice("body", "World", self.body.index(), &BodyChoice::LABELS, "Which world to map. Earth, Moon, Mars, Venus, Mercury and Ceres come from NASA and NOAA elevation surveys; the fictional worlds are generated from the seed below."),
            Control::slider("body_seconds", "Seconds per world", self.body_seconds as i32, (20, 600, 5), " s", "How long each world stays on screen when several are in turn. The next world dissolves in over a few seconds."),
            Control::slider("fictional_seed", "Fictional seed", self.fictional_seed as i32, (0, 9999, 1), "", "Seed of the fictional worlds. Each seed gives different continents, mountain belts and craters."),
            Control::choice("projection", "Projection", self.projection.index(), &Projection::LABELS, "A flat world map, or a globe turning in space."),
            Control::slider("zoom_percent", "Zoom", self.zoom_percent as i32, (50, 1600, 10), "%", "100% fits the whole world on screen. Higher values show a smaller region in more detail."),
            Control::slider("contour_levels", "Contour lines", self.contour_levels as i32, (6, 80, 1), "", "Number of contour lines across the whole relief of the world, spaced at round elevations. Ignored when a fixed spacing is set."),
            Control::slider("interval_m", "Contour spacing", self.interval_m as i32, (0, 10_000, 50), " m", "Fixed elevation step between contour lines in metres. 0 derives the step from the number of contour lines."),
            Control::slider("index_every", "Index line every", self.index_every as i32, (0, 10, 1), "", "Every Nth contour is drawn heavier, like the index contours of a survey map. 0 draws none."),
            Control::slider("line_thickness", "Line thickness", self.line_thickness as i32, (1, 3, 1), " dots", "Thickness of contour lines in Braille dots."),
            Control::choice("below_style", "Lines below zero", self.below_style.index(), &BelowStyle::LABELS, "How contours below the zero level (sea floor, basins) are drawn: solid, dotted or hidden."),
            Control::toggle("coastline", "Emphasise zero level", self.coastline, "Draw the zero-elevation contour (the shore on Earth) heavier than the rest."),
            Control::toggle("outline", "Map outline", self.outline, "Draw the edge of the map, or the limb of the globe."),
            Control::slider("shading_percent", "Relief shading", self.shading_percent as i32, (0, 100, 5), "%", "Sparse dots that light slopes facing the north-west, in addition to the contour lines."),
            Control::choice("palette", "Colours", self.palette.index(), &PaletteChoice::LABELS, "Tint the map by elevation, or use the single global colour."),
            Control::choice("pan", "Pan", self.pan.index(), &PanStyle::LABELS, "How the view travels over the world. The globe turns instead of sliding."),
            Control::slider("pan_speed", "Pan speed", self.pan_speed as i32, (0, 400, 10), "%", "0 holds the view still; 100 moves about 5 dots per second at any zoom."),
            Control::slider("center_latitude", "Start latitude", self.center_latitude, (-80, 80, 5), "°", "Latitude the view is centred on when it starts and, for a still view, always."),
            Control::slider("start_longitude", "Start longitude", self.start_longitude, (-180, 180, 5), "°", "Longitude the view is centred on when it starts."),
            Control::slider("zero_shift_m", "Zero level shift", self.zero_shift_m, (-3000, 3000, 50), " m", "Raise or lower the zero level that the shore line and below-zero style refer to, flooding or draining the world."),
            Control::slider("tide_range_m", "Tide range", self.tide_range_m as i32, (0, 2000, 50), " m", "Let the zero level rise and fall by this much, so shores creep across the map. 0 keeps it fixed."),
        ];
        if self.tide_range_m > 0 {
            rows.push(Control::slider(
                "tide_seconds",
                "Tide period",
                self.tide_seconds as i32,
                (10, 600, 10),
                " s",
                "Seconds for one full rise and fall of the zero level.",
            ));
        }
        rows
    }

    fn set_control(&mut self, id: &str, value: ControlValue) -> Result<bool, String> {
        match id {
            "body" => set_choice(&mut self.body, &value, BodyChoice::from_index),
            "body_seconds" => set_number(&mut self.body_seconds, &value, (20, 600), |n| n as u32),
            "fictional_seed" => {
                set_number(&mut self.fictional_seed, &value, (0, 9999), |n| n as u32)
            }
            "projection" => set_choice(&mut self.projection, &value, Projection::from_index),
            "zoom_percent" => set_number(&mut self.zoom_percent, &value, (50, 1600), |n| n as u32),
            "contour_levels" => set_number(&mut self.contour_levels, &value, (6, 80), |n| n as u32),
            "interval_m" => set_number(&mut self.interval_m, &value, (0, 10_000), |n| n as u32),
            "index_every" => set_number(&mut self.index_every, &value, (0, 10), |n| n as u32),
            "line_thickness" => set_number(&mut self.line_thickness, &value, (1, 3), |n| n as u32),
            "below_style" => set_choice(&mut self.below_style, &value, BelowStyle::from_index),
            "coastline" => set_toggle(&mut self.coastline, &value),
            "outline" => set_toggle(&mut self.outline, &value),
            "shading_percent" => {
                set_number(&mut self.shading_percent, &value, (0, 100), |n| n as u32)
            }
            "palette" => set_choice(&mut self.palette, &value, PaletteChoice::from_index),
            "pan" => set_choice(&mut self.pan, &value, PanStyle::from_index),
            "pan_speed" => set_number(&mut self.pan_speed, &value, (0, 400), |n| n as u32),
            "center_latitude" => set_number(&mut self.center_latitude, &value, (-80, 80), |n| n),
            "start_longitude" => set_number(&mut self.start_longitude, &value, (-180, 180), |n| n),
            "zero_shift_m" => set_number(&mut self.zero_shift_m, &value, (-3000, 3000), |n| n),
            "tide_range_m" => set_number(&mut self.tide_range_m, &value, (0, 2000), |n| n as u32),
            "tide_seconds" => set_number(&mut self.tide_seconds, &value, (10, 600), |n| n as u32),
            _ => Ok(false),
        }
    }
}

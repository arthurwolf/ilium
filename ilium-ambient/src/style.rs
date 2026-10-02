//! The shared look of every background animation: colour mode, palette,
//! brightness and the other tone controls, plus the presets that set them in
//! one step.
//!
//! This module is pure (no terminal, no I/O). The client calls `Appearance::shade`
//! once per ink cell, after the scene has drawn and the dither has chosen its
//! dots, so one set of controls styles every scene alike: a scene that paints
//! its own colours is recoloured through the palette, and a scene that paints
//! none still gets colour from dot coverage, position or time.
//!
//! RULE FOR NEW ANIMATIONS: do not add per-scene colour, brightness, contrast
//! or palette controls. Draw tone into the raster (and per-cell colours when
//! the scene has natural ones); this module styles the result. Scene settings
//! are for what is specific to the scene.

use crate::control::{self, Control, ControlValue};
use crate::raster::DitherMode;
use serde::{Deserialize, Serialize};

pub type Rgb = [u8; 3];

const fn hex(value: u32) -> Rgb {
    [(value >> 16) as u8, (value >> 8) as u8, value as u8]
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorMode {
    /// Colour from the palette (or the scene's own colours).
    #[default]
    Color,
    /// Shades of grey, optionally tinted.
    Greyscale,
    /// One ink colour for every dot.
    Monotone,
}

impl ColorMode {
    pub const LABELS: [&'static str; 3] = ["Color", "Greyscale", "Monotone"];
    pub fn from_index(index: usize) -> Option<Self> {
        [Self::Color, Self::Greyscale, Self::Monotone]
            .get(index)
            .copied()
    }
    pub fn index(self) -> usize {
        match self {
            Self::Color => 0,
            Self::Greyscale => 1,
            Self::Monotone => 2,
        }
    }
}

/// What position on the palette (or grey ramp) a cell takes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorSource {
    /// The scene's own colour brightness when it has colours, else coverage.
    #[default]
    Auto,
    /// Every cell the same position (the middle of the palette).
    Flat,
    /// How many of the cell's eight dots are lit.
    Coverage,
    Vertical,
    Horizontal,
    Diagonal,
    /// Distance from the centre of the screen.
    Radial,
    /// A slow drift through the palette over time.
    Drift,
}

impl ColorSource {
    pub const ALL: [Self; 8] = [
        Self::Auto,
        Self::Flat,
        Self::Coverage,
        Self::Vertical,
        Self::Horizontal,
        Self::Diagonal,
        Self::Radial,
        Self::Drift,
    ];
    pub const LABELS: [&'static str; 8] = [
        "Scene or coverage",
        "Flat",
        "Dot coverage",
        "Top to bottom",
        "Left to right",
        "Diagonal",
        "Centre to edge",
        "Drifting in time",
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

/// One named gradient.
pub struct PaletteDef {
    pub id: &'static str,
    pub label: &'static str,
    /// Evenly spaced stops, dark end first. Empty for the scene's own colours.
    pub stops: &'static [Rgb],
}

macro_rules! palette {
    ($id:expr, $label:expr, [$($stop:expr),+ $(,)?]) => {
        PaletteDef { id: $id, label: $label, stops: &[$(hex($stop)),+] }
    };
}

/// Every palette, in menu order. The first entry keeps the scene's colours.
pub const PALETTES: [PaletteDef; 38] = [
    PaletteDef {
        id: "scene",
        label: "Scene colors",
        stops: &[],
    },
    palette!(
        "rainbow",
        "Rainbow",
        [0xd7263d, 0xf4a300, 0xf2e94e, 0x3bb273, 0x2a9df4, 0x6a4bd8, 0xb23fc1]
    ),
    palette!(
        "pastel_rainbow",
        "Pastel rainbow",
        [0xf7b2bd, 0xfdd7aa, 0xfdf3b0, 0xbde7c5, 0xb3d9f2, 0xc9bff0, 0xe6bbea]
    ),
    palette!(
        "cotton_candy",
        "Cotton candy",
        [0xf9c6d9, 0xf4b4e0, 0xd9b8f2, 0xb8c9f5, 0xb5e6f2]
    ),
    palette!(
        "sea_glass",
        "Sea glass",
        [0xc9efe2, 0xa8e0d8, 0x8fd0d6, 0xa5c4e8, 0xc3b8ec]
    ),
    palette!(
        "peach_cream",
        "Peach cream",
        [0xfde4cf, 0xfcd5ce, 0xf8c8dc, 0xfbd8b8, 0xfff1d0]
    ),
    palette!(
        "lavender_haze",
        "Lavender haze",
        [0xd8c9f2, 0xc4b5ea, 0xb3a7e6, 0xc7d2f4, 0xe3e0fa]
    ),
    palette!(
        "neon",
        "Neon",
        [0xff2bd6, 0x7a5cff, 0x18e6ff, 0x39ff88, 0xf4ff3c]
    ),
    palette!(
        "vaporwave",
        "Vaporwave",
        [0x2a1a5e, 0x7b2ff7, 0xf72585, 0xff9ee6, 0x7df9ff]
    ),
    palette!(
        "synthwave",
        "Synthwave",
        [0x0b0630, 0x3d1b8f, 0xc0239d, 0xff6a3d, 0xffd447]
    ),
    palette!(
        "cyberpunk",
        "Cyberpunk",
        [0x12002a, 0x6a00f4, 0xff00a8, 0x00e5ff, 0xfff200]
    ),
    palette!(
        "sunset",
        "Sunset",
        [0x1d0f3b, 0x6b1b6f, 0xd6336c, 0xff7b3a, 0xffd36e]
    ),
    palette!(
        "sunrise",
        "Sunrise",
        [0x1b2a55, 0x5b5aa6, 0xe08fa8, 0xffb787, 0xfff0a8]
    ),
    palette!(
        "ocean",
        "Ocean",
        [0x03123a, 0x0a3d7a, 0x0f7b9c, 0x4fc2c0, 0xdff7f0]
    ),
    palette!(
        "deep_sea",
        "Deep sea",
        [0x010814, 0x06203f, 0x0b4a6b, 0x1a8b8f, 0x7fe0c8]
    ),
    palette!(
        "forest",
        "Forest",
        [0x07210f, 0x14532d, 0x3f7d20, 0x9bc53d, 0xf1f7c8]
    ),
    palette!(
        "moss",
        "Moss and stone",
        [0x1d2b20, 0x3a5a40, 0x6b8f5a, 0xa7b79a, 0xd9dccb]
    ),
    palette!(
        "autumn",
        "Autumn",
        [0x2b1409, 0x7a3b12, 0xc5611a, 0xe8a317, 0xb5302a]
    ),
    palette!(
        "ember",
        "Ember",
        [0x0d0302, 0x5a0f08, 0xc23a0c, 0xf79d1a, 0xfff3b0]
    ),
    palette!(
        "ice",
        "Ice",
        [0x0a1f44, 0x1b5aa8, 0x4fa8e8, 0xb5e3f7, 0xf5fcff]
    ),
    palette!(
        "aurora",
        "Aurora",
        [0x041424, 0x0b6e5c, 0x2fd18a, 0x7bf0c0, 0xa07df2]
    ),
    palette!(
        "viridis",
        "Viridis",
        [0x440154, 0x3b528b, 0x21918c, 0x5ec962, 0xfde725]
    ),
    palette!(
        "plasma",
        "Plasma",
        [0x0d0887, 0x7e03a8, 0xcc4778, 0xf89540, 0xf0f921]
    ),
    palette!(
        "inferno",
        "Inferno",
        [0x000004, 0x56106e, 0xbb3754, 0xf98c0a, 0xfcffa4]
    ),
    palette!(
        "cividis",
        "Cividis (color-blind safe)",
        [0x00204d, 0x31446b, 0x666970, 0xa59c74, 0xffe945]
    ),
    palette!(
        "turbo",
        "Turbo",
        [0x30123b, 0x4686fb, 0x1ae4b6, 0xa4fc3c, 0xf9ba38, 0xd23105]
    ),
    palette!(
        "sepia",
        "Sepia",
        [0x1c120a, 0x4a3320, 0x8a6a46, 0xc9a97c, 0xf1e2c3]
    ),
    palette!(
        "warm_paper",
        "Warm paper",
        [0x3a3228, 0x6e6252, 0xa89a82, 0xd8cdb6, 0xf7f1e3]
    ),
    palette!(
        "amber_crt",
        "Amber terminal",
        [0x120800, 0x4a2400, 0x9a5200, 0xe08a00, 0xffc04d]
    ),
    palette!(
        "green_crt",
        "Green phosphor",
        [0x021208, 0x0a4a1a, 0x16a034, 0x4cf06a, 0xc8ffd2]
    ),
    palette!(
        "blueprint",
        "Blueprint",
        [0x061a3d, 0x0d3b86, 0x2a6fd6, 0x8fb8f2, 0xf0f6ff]
    ),
    palette!(
        "solarized",
        "Solarized",
        [0x002b36, 0x268bd2, 0x2aa198, 0x859900, 0xb58900, 0xcb4b16, 0xd33682]
    ),
    palette!(
        "nord",
        "Nord",
        [0x2e3440, 0x5e81ac, 0x88c0d0, 0xa3be8c, 0xebcb8b, 0xd08770, 0xb48ead]
    ),
    palette!(
        "dracula",
        "Dracula",
        [0x282a36, 0x6272a4, 0xbd93f9, 0xff79c6, 0xff5555, 0xffb86c, 0xf1fa8c, 0x50fa7b]
    ),
    palette!(
        "gruvbox",
        "Gruvbox",
        [0x282828, 0x458588, 0x689d6a, 0x98971a, 0xd79921, 0xd65d0e, 0xcc241d, 0xb16286]
    ),
    palette!(
        "tokyo_night",
        "Tokyo Night",
        [0x1a1b26, 0x3d59a1, 0x7aa2f7, 0x7dcfff, 0x9ece6a, 0xe0af68, 0xf7768e, 0xbb9af7]
    ),
    palette!(
        "rose_gold",
        "Rose gold",
        [0x2a1418, 0x7a3b44, 0xb76e79, 0xe0a899, 0xfbe3d6]
    ),
    palette!(
        "slate",
        "Slate",
        [0x14181f, 0x2e3a4a, 0x56677d, 0x93a3b8, 0xdde5ef]
    ),
];

pub fn palette_labels() -> Vec<&'static str> {
    PALETTES.iter().map(|palette| palette.label).collect()
}

/// A named bundle of appearance values. `Custom` means "edited by hand".
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StylePreset {
    #[default]
    Custom,
    Original,
    Whisper,
    Subtle,
    QuietGrey,
    SoftPastel,
    Vivid,
    NeonNight,
    MidnightBlue,
    EmberGlow,
    Matrix,
    AmberTerminal,
    PaperInk,
    Blueprint,
    SunsetHaze,
    DeepForest,
    AuroraNight,
    Vaporwave,
    HighContrast,
    GhostMono,
    RetroPrint,
}

/// What a preset sets. Fields left `None` keep the user's value.
pub struct PresetDef {
    pub label: &'static str,
    pub mode: ColorMode,
    pub palette: &'static str,
    pub source: ColorSource,
    pub brightness: u16,
    pub contrast: u16,
    pub gamma: u16,
    pub saturation: u16,
    pub vignette: u16,
    pub grey_tint_hue: u16,
    pub grey_tint_strength: u16,
    pub dither: Option<DitherMode>,
    pub density: Option<u16>,
}

const BASE: PresetDef = PresetDef {
    label: "",
    mode: ColorMode::Color,
    palette: "scene",
    source: ColorSource::Auto,
    brightness: 100,
    contrast: 100,
    gamma: 100,
    saturation: 100,
    vignette: 0,
    grey_tint_hue: 40,
    grey_tint_strength: 0,
    dither: None,
    density: None,
};

impl StylePreset {
    pub const ALL: [Self; 21] = [
        Self::Custom,
        Self::Original,
        Self::Whisper,
        Self::Subtle,
        Self::QuietGrey,
        Self::SoftPastel,
        Self::Vivid,
        Self::NeonNight,
        Self::MidnightBlue,
        Self::EmberGlow,
        Self::Matrix,
        Self::AmberTerminal,
        Self::PaperInk,
        Self::Blueprint,
        Self::SunsetHaze,
        Self::DeepForest,
        Self::AuroraNight,
        Self::Vaporwave,
        Self::HighContrast,
        Self::GhostMono,
        Self::RetroPrint,
    ];

    pub fn labels() -> Vec<&'static str> {
        Self::ALL
            .iter()
            .map(|preset| preset.definition().map_or("Custom", |def| def.label))
            .collect()
    }

    pub fn from_index(index: usize) -> Option<Self> {
        Self::ALL.get(index).copied()
    }

    pub fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|candidate| *candidate == self)
            .unwrap_or(0)
    }

    /// `None` for `Custom`.
    pub fn definition(self) -> Option<PresetDef> {
        Some(match self {
            Self::Custom => return None,
            Self::Original => PresetDef {
                label: "Original",
                ..BASE
            },
            Self::Whisper => PresetDef {
                label: "Whisper (barely visible)",
                mode: ColorMode::Greyscale,
                brightness: 22,
                contrast: 90,
                ..BASE
            },
            Self::Subtle => PresetDef {
                label: "Subtle",
                brightness: 42,
                saturation: 80,
                ..BASE
            },
            Self::QuietGrey => PresetDef {
                label: "Quiet grey",
                mode: ColorMode::Greyscale,
                brightness: 40,
                ..BASE
            },
            Self::SoftPastel => PresetDef {
                label: "Soft pastel",
                palette: "pastel_rainbow",
                brightness: 62,
                saturation: 85,
                ..BASE
            },
            Self::Vivid => PresetDef {
                label: "Vivid",
                palette: "rainbow",
                contrast: 125,
                saturation: 130,
                ..BASE
            },
            Self::NeonNight => PresetDef {
                label: "Neon night",
                palette: "neon",
                brightness: 70,
                contrast: 120,
                saturation: 120,
                vignette: 25,
                ..BASE
            },
            Self::MidnightBlue => PresetDef {
                label: "Midnight blue",
                palette: "deep_sea",
                brightness: 55,
                vignette: 20,
                ..BASE
            },
            Self::EmberGlow => PresetDef {
                label: "Ember glow",
                palette: "ember",
                brightness: 58,
                vignette: 30,
                ..BASE
            },
            Self::Matrix => PresetDef {
                label: "Matrix",
                palette: "green_crt",
                brightness: 72,
                dither: Some(DitherMode::Lines),
                ..BASE
            },
            Self::AmberTerminal => PresetDef {
                label: "Amber terminal",
                palette: "amber_crt",
                brightness: 68,
                dither: Some(DitherMode::Lines),
                ..BASE
            },
            Self::PaperInk => PresetDef {
                label: "Paper and ink",
                mode: ColorMode::Greyscale,
                grey_tint_hue: 38,
                grey_tint_strength: 35,
                brightness: 55,
                dither: Some(DitherMode::Halftone),
                ..BASE
            },
            Self::Blueprint => PresetDef {
                label: "Blueprint",
                palette: "blueprint",
                brightness: 65,
                dither: Some(DitherMode::Crosshatch),
                ..BASE
            },
            Self::SunsetHaze => PresetDef {
                label: "Sunset haze",
                palette: "sunset",
                source: ColorSource::Vertical,
                brightness: 56,
                vignette: 22,
                ..BASE
            },
            Self::DeepForest => PresetDef {
                label: "Deep forest",
                palette: "forest",
                brightness: 52,
                gamma: 110,
                ..BASE
            },
            Self::AuroraNight => PresetDef {
                label: "Aurora night",
                palette: "aurora",
                source: ColorSource::Drift,
                brightness: 60,
                vignette: 18,
                ..BASE
            },
            Self::Vaporwave => PresetDef {
                label: "Vaporwave",
                palette: "vaporwave",
                source: ColorSource::Diagonal,
                brightness: 68,
                saturation: 115,
                ..BASE
            },
            Self::HighContrast => PresetDef {
                label: "High contrast",
                mode: ColorMode::Greyscale,
                brightness: 100,
                contrast: 170,
                ..BASE
            },
            Self::GhostMono => PresetDef {
                label: "Ghost (dim monotone)",
                mode: ColorMode::Monotone,
                brightness: 20,
                ..BASE
            },
            Self::RetroPrint => PresetDef {
                label: "Retro print",
                palette: "sepia",
                brightness: 60,
                dither: Some(DitherMode::Halftone),
                density: Some(70),
                ..BASE
            },
        })
    }
}

/// Everything `shade` needs to know about one cell besides the colours.
#[derive(Debug, Clone, Copy)]
pub struct CellContext {
    /// 0..=1: lit dots in the cell divided by eight (0 means no ink).
    pub coverage: f32,
    /// Cell position as a fraction of the field, 0..=1 each.
    pub x: f32,
    pub y: f32,
    /// Animation time in seconds (for `ColorSource::Drift`).
    pub seconds: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Appearance {
    pub preset: StylePreset,
    pub mode: ColorMode,
    /// Index into `PALETTES`.
    pub palette: usize,
    pub source: ColorSource,
    pub reverse: bool,
    /// Rotation of the palette in percent of its length, 0..=100.
    pub shift_percent: u16,
    /// How much of the palette the full range of the source covers, 25..=400.
    pub spread_percent: u16,
    /// Overall brightness, 1..=200. Below 100 dims everything toward black.
    pub brightness_percent: u16,
    pub contrast_percent: u16,
    /// Mid-tone curve, 30..=300 (100 is neutral; above 100 brightens mid-tones).
    pub gamma_percent: u16,
    /// Colour saturation multiplier, 0..=200.
    pub saturation_percent: u16,
    /// Hue rotation, -180..=180 degrees.
    pub hue_shift_degrees: i16,
    pub invert: bool,
    /// Darkening towards the screen edges, 0..=100.
    pub vignette_percent: u16,
    pub grey_tint_hue: u16,
    pub grey_tint_strength: u16,
    /// Contrast of the dot pattern before dithering, 50..=200.
    pub pattern_contrast_percent: u16,
    /// Swap lit and unlit dots.
    pub pattern_invert: bool,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            preset: StylePreset::Custom,
            mode: ColorMode::Color,
            palette: 0,
            source: ColorSource::Auto,
            reverse: false,
            shift_percent: 0,
            spread_percent: 100,
            brightness_percent: 100,
            contrast_percent: 100,
            gamma_percent: 100,
            saturation_percent: 100,
            hue_shift_degrees: 0,
            invert: false,
            vignette_percent: 0,
            grey_tint_hue: 40,
            grey_tint_strength: 0,
            pattern_contrast_percent: 100,
            pattern_invert: false,
        }
    }
}

fn luma(color: Rgb) -> f32 {
    (0.2126 * f32::from(color[0]) + 0.7152 * f32::from(color[1]) + 0.0722 * f32::from(color[2]))
        / 255.0
}

fn mix(a: Rgb, b: Rgb, fraction: f32) -> Rgb {
    let fraction = fraction.clamp(0.0, 1.0);
    [0, 1, 2].map(|channel| {
        (f32::from(a[channel]) + (f32::from(b[channel]) - f32::from(a[channel])) * fraction).round()
            as u8
    })
}

/// Colour at `position` (0..=1) along `stops`.
pub fn ramp(stops: &[Rgb], position: f32) -> Rgb {
    match stops.len() {
        0 => [200, 200, 200],
        1 => stops[0],
        count => {
            let scaled = position.clamp(0.0, 1.0) * (count - 1) as f32;
            let index = (scaled.floor() as usize).min(count - 2);
            mix(stops[index], stops[index + 1], scaled - index as f32)
        }
    }
}

fn rgb_to_hsl(color: Rgb) -> (f32, f32, f32) {
    let [red, green, blue] = color.map(|channel| f32::from(channel) / 255.0);
    let maximum = red.max(green).max(blue);
    let minimum = red.min(green).min(blue);
    let lightness = (maximum + minimum) / 2.0;
    let delta = maximum - minimum;
    if delta < 1e-6 {
        return (0.0, 0.0, lightness);
    }
    let saturation = delta / (1.0 - (2.0 * lightness - 1.0).abs()).max(1e-6);
    let hue = if maximum == red {
        ((green - blue) / delta).rem_euclid(6.0)
    } else if maximum == green {
        (blue - red) / delta + 2.0
    } else {
        (red - green) / delta + 4.0
    } * 60.0;
    (hue, saturation.clamp(0.0, 1.0), lightness)
}

fn hsl_to_rgb(hue: f32, saturation: f32, lightness: f32) -> Rgb {
    let chroma = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
    let sector = hue.rem_euclid(360.0) / 60.0;
    let secondary = chroma * (1.0 - (sector % 2.0 - 1.0).abs());
    let (red, green, blue) = match sector as u8 {
        0 => (chroma, secondary, 0.0),
        1 => (secondary, chroma, 0.0),
        2 => (0.0, chroma, secondary),
        3 => (0.0, secondary, chroma),
        4 => (secondary, 0.0, chroma),
        _ => (chroma, 0.0, secondary),
    };
    let offset = lightness - chroma / 2.0;
    [red, green, blue].map(|channel| ((channel + offset) * 255.0).round().clamp(0.0, 255.0) as u8)
}

impl Appearance {
    pub fn normalized(&self) -> Self {
        Self {
            palette: self.palette.min(PALETTES.len() - 1),
            shift_percent: self.shift_percent.min(100),
            spread_percent: self.spread_percent.clamp(25, 400),
            brightness_percent: self.brightness_percent.clamp(1, 200),
            contrast_percent: self.contrast_percent.min(200),
            gamma_percent: self.gamma_percent.clamp(30, 300),
            saturation_percent: self.saturation_percent.min(200),
            hue_shift_degrees: self.hue_shift_degrees.clamp(-180, 180),
            vignette_percent: self.vignette_percent.min(100),
            grey_tint_hue: self.grey_tint_hue.min(359),
            grey_tint_strength: self.grey_tint_strength.min(100),
            pattern_contrast_percent: self.pattern_contrast_percent.clamp(50, 200),
            ..self.clone()
        }
    }

    /// True when the scene's own per-cell colours are shown unchanged by the
    /// palette choice, so a host may skip colour work entirely: only the
    /// neutral adjustments remain.
    pub fn is_neutral(&self) -> bool {
        self.mode == ColorMode::Color
            && self.palette == 0
            && self.brightness_percent == 100
            && self.contrast_percent == 100
            && self.gamma_percent == 100
            && self.saturation_percent == 100
            && self.hue_shift_degrees == 0
            && !self.invert
            && self.vignette_percent == 0
    }

    /// Position of a cell on the palette, 0..=1, before spread and shift.
    fn source_position(&self, scene: Option<Rgb>, context: &CellContext) -> f32 {
        let from_coverage = || ((context.coverage * 8.0 - 1.0) / 7.0).clamp(0.0, 1.0);
        match self.source {
            ColorSource::Auto => scene.map_or_else(from_coverage, luma),
            ColorSource::Flat => 0.5,
            ColorSource::Coverage => from_coverage(),
            ColorSource::Vertical => context.y,
            ColorSource::Horizontal => context.x,
            ColorSource::Diagonal => (context.x + context.y) * 0.5,
            ColorSource::Radial => {
                ((context.x - 0.5).hypot(context.y - 0.5) * std::f32::consts::SQRT_2).min(1.0)
            }
            ColorSource::Drift => {
                (context.seconds * 0.02 + context.x * 0.3 + context.y * 0.2).fract()
            }
        }
    }

    fn palette_position(&self, base: f32) -> f32 {
        let spread = f32::from(self.spread_percent) / 100.0;
        let shifted = (base - 0.5) * spread + 0.5 + f32::from(self.shift_percent) / 100.0;
        let wrapped = if self.source == ColorSource::Drift || self.shift_percent > 0 {
            shifted.rem_euclid(1.0)
        } else {
            shifted.clamp(0.0, 1.0)
        };
        if self.reverse {
            1.0 - wrapped
        } else {
            wrapped
        }
    }

    /// Final colour of one ink cell. `ink` is the monotone ink; `scene` the
    /// scene's own colour for the cell, when it supplies them.
    pub fn shade(&self, ink: Rgb, scene: Option<Rgb>, context: &CellContext) -> Rgb {
        let base = match self.mode {
            ColorMode::Monotone => ink,
            ColorMode::Greyscale => {
                let level = self.palette_position_free(self.source_position(scene, context));
                let grey = (level * 255.0).round() as u8;
                if self.grey_tint_strength == 0 {
                    [grey, grey, grey]
                } else {
                    let tinted =
                        hsl_to_rgb(f32::from(self.grey_tint_hue), 0.85, level.clamp(0.0, 1.0));
                    mix(
                        [grey, grey, grey],
                        tinted,
                        f32::from(self.grey_tint_strength) / 100.0,
                    )
                }
            }
            ColorMode::Color => {
                let stops = PALETTES[self.palette.min(PALETTES.len() - 1)].stops;
                if stops.is_empty() {
                    scene.unwrap_or(ink)
                } else {
                    let position = self.palette_position(self.source_position(scene, context));
                    ramp(stops, position)
                }
            }
        };
        self.adjust(base, context)
    }

    /// Grey level 0.2..=1.0 for a position: greyscale never goes fully black,
    /// because brightness is the control for dimming.
    fn palette_position_free(&self, base: f32) -> f32 {
        let level = 0.2 + 0.8 * self.palette_position(base);
        level.clamp(0.0, 1.0)
    }

    /// The tone adjustments shared by every mode.
    pub fn adjust(&self, color: Rgb, context: &CellContext) -> Rgb {
        let mut channels = color.map(|channel| f32::from(channel) / 255.0);
        if self.invert {
            channels = channels.map(|channel| 1.0 - channel);
        }
        let contrast = f32::from(self.contrast_percent) / 100.0;
        if (contrast - 1.0).abs() > 1e-3 {
            channels = channels.map(|channel| ((channel - 0.5) * contrast + 0.5).clamp(0.0, 1.0));
        }
        let gamma = f32::from(self.gamma_percent) / 100.0;
        if (gamma - 1.0).abs() > 1e-3 {
            channels = channels.map(|channel| channel.powf(1.0 / gamma));
        }
        let mut color = channels.map(|channel| (channel * 255.0).round().clamp(0.0, 255.0) as u8);
        if self.saturation_percent != 100 || self.hue_shift_degrees != 0 {
            let (hue, saturation, lightness) = rgb_to_hsl(color);
            color = hsl_to_rgb(
                hue + f32::from(self.hue_shift_degrees),
                (saturation * f32::from(self.saturation_percent) / 100.0).clamp(0.0, 1.0),
                lightness,
            );
        }
        let mut scale = f32::from(self.brightness_percent) / 100.0;
        if self.vignette_percent > 0 {
            let edge = ((context.x - 0.5).hypot(context.y - 0.5) * std::f32::consts::SQRT_2)
                .clamp(0.0, 1.0);
            scale *= 1.0 - f32::from(self.vignette_percent) / 100.0 * edge * edge;
        }
        color.map(|channel| (f32::from(channel) * scale).round().clamp(0.0, 255.0) as u8)
    }

    /// Dot-intensity adjustment applied before dithering.
    pub fn shape_dot(&self, intensity: f32) -> f32 {
        let mut value = intensity;
        if self.pattern_contrast_percent != 100 {
            value = ((value - 0.5) * f32::from(self.pattern_contrast_percent) / 100.0 + 0.5)
                .clamp(0.0, 1.0);
        }
        if self.pattern_invert {
            value = 1.0 - value;
        }
        value
    }

    /// True when `shape_dot` is the identity.
    pub fn pattern_is_neutral(&self) -> bool {
        self.pattern_contrast_percent == 100 && !self.pattern_invert
    }

    /// Set every field a preset defines. The user's dither and density are
    /// returned for the caller to apply, because they live outside this struct.
    pub fn apply_preset(
        &mut self,
        preset: StylePreset,
    ) -> Option<(Option<DitherMode>, Option<u16>)> {
        let definition = preset.definition()?;
        self.preset = preset;
        self.mode = definition.mode;
        self.palette = PALETTES
            .iter()
            .position(|palette| palette.id == definition.palette)
            .unwrap_or(0);
        self.source = definition.source;
        self.reverse = false;
        self.shift_percent = 0;
        self.spread_percent = 100;
        self.brightness_percent = definition.brightness;
        self.contrast_percent = definition.contrast;
        self.gamma_percent = definition.gamma;
        self.saturation_percent = definition.saturation;
        self.hue_shift_degrees = 0;
        self.invert = false;
        self.vignette_percent = definition.vignette;
        self.grey_tint_hue = definition.grey_tint_hue;
        self.grey_tint_strength = definition.grey_tint_strength;
        Some((definition.dither, definition.density))
    }

    /// Settings rows, in display order. `ink_rows` lets the host splice its
    /// own monotone-ink rows (lightness, hue, saturation) after the mode.
    pub fn controls(&self) -> Vec<Control> {
        let mut rows = vec![
            Control::choice("look_preset", "Style preset", self.preset.index(), &StylePreset::labels(), "Apply a ready-made look: mode, palette, brightness and more. Editing any value afterwards switches this back to Custom. Dither and density change only for presets that name them."),
            Control::choice("look_mode", "Color mode", self.mode.index(), &ColorMode::LABELS, "Color uses a palette (or the scene's own colors), Greyscale uses shades of grey with an optional tint, Monotone draws every dot in one ink color."),
        ];
        if self.mode != ColorMode::Monotone {
            if self.mode == ColorMode::Color {
                rows.push(Control::choice("look_palette", "Palette", self.palette, &palette_labels(), "Gradient the colors are taken from. Scene colors keeps what the animation paints itself; the others recolor it by brightness, or color scenes that have no colors of their own."));
            }
            rows.push(Control::choice("look_source", "Color from", self.source.index(), &ColorSource::LABELS, "What picks the position on the palette: the scene's own brightness or dot coverage, a fixed position, a gradient across the screen, or a slow drift through time."));
            rows.push(Control::toggle(
                "look_reverse",
                "Reverse",
                self.reverse,
                "Run the palette or grey ramp from the other end.",
            ));
            rows.push(Control::slider(
                "look_shift",
                "Palette shift",
                i32::from(self.shift_percent),
                (0, 100, 1),
                "%",
                "Rotate the palette; the colors wrap around.",
            ));
            rows.push(Control::slider("look_spread", "Palette spread", i32::from(self.spread_percent), (25, 400, 5), "%", "How much of the palette the whole range spans. Below 100% shows a narrow slice, above 100% repeats the extremes."));
        }
        if self.mode == ColorMode::Greyscale {
            rows.push(Control::slider(
                "look_grey_hue",
                "Grey tint hue",
                i32::from(self.grey_tint_hue),
                (0, 359, 1),
                "\u{b0}",
                "Hue of the tint mixed into the greys.",
            ));
            rows.push(Control::slider(
                "look_grey_tint",
                "Grey tint",
                i32::from(self.grey_tint_strength),
                (0, 100, 1),
                "%",
                "How strongly the greys lean toward the tint hue. 0 is pure grey.",
            ));
        }
        rows.extend([
            Control::slider("look_brightness", "Brightness", i32::from(self.brightness_percent), (1, 200, 1), "%", "Overall brightness of the animation. Lower it to make the background less noticeable; 100% is unchanged."),
            Control::slider("look_contrast", "Contrast", i32::from(self.contrast_percent), (0, 200, 5), "%", "Spreads colors away from or toward mid-grey. 100% is unchanged."),
            Control::slider("look_gamma", "Gamma", i32::from(self.gamma_percent), (30, 300, 5), "%", "Mid-tone curve: above 100% lifts the mid-tones, below 100% deepens them."),
            Control::slider("look_saturation", "Color intensity", i32::from(self.saturation_percent), (0, 200, 5), "%", "Multiplier on color saturation; 0% is grey, 100% unchanged."),
            Control::slider("look_hue_shift", "Hue shift", i32::from(self.hue_shift_degrees), (-180, 180, 5), "\u{b0}", "Rotate every color around the color wheel."),
            Control::toggle("look_invert", "Invert colors", self.invert, "Replace every color by its opposite tone."),
            Control::slider("look_vignette", "Edge fade", i32::from(self.vignette_percent), (0, 100, 5), "%", "Darken the animation toward the edges of the screen."),
        ]);
        rows
    }

    /// Controls that belong to the dot pattern rather than the colors.
    pub fn pattern_controls(&self) -> Vec<Control> {
        vec![
            Control::slider("look_pattern_contrast", "Pattern contrast", i32::from(self.pattern_contrast_percent), (50, 200, 5), "%", "Sharpen or soften the scene's dot tones before dithering: higher keeps only bright dots, lower fills in the shadows."),
            Control::toggle("look_pattern_invert", "Invert pattern", self.pattern_invert, "Swap lit and unlit dots, like a photographic negative."),
        ]
    }

    /// Applies one edit. `Ok(true)` when something changed, `Ok(false)` for an
    /// unknown id or unchanged value.
    pub fn set_control(&mut self, id: &str, value: ControlValue) -> Result<bool, String> {
        let before = self.clone();
        let number = |value: &ControlValue, low: i32, high: i32| {
            control::number(value)
                .map(|number| number.clamp(low, high))
                .ok_or("Expected a number")
        };
        match id {
            "look_preset" => {
                let index = control::index(&value).ok_or("Unknown preset")?;
                let preset = StylePreset::from_index(index).ok_or("Unknown preset")?;
                if preset == StylePreset::Custom {
                    self.preset = StylePreset::Custom;
                } else {
                    self.apply_preset(preset);
                }
                return Ok(*self != before);
            }
            "look_mode" => {
                self.mode = control::index(&value)
                    .and_then(ColorMode::from_index)
                    .ok_or("Unknown color mode")?
            }
            "look_palette" => {
                let index = control::index(&value).ok_or("Unknown palette")?;
                if index >= PALETTES.len() {
                    return Err("Unknown palette".to_owned());
                }
                self.palette = index;
            }
            "look_source" => {
                self.source = control::index(&value)
                    .and_then(ColorSource::from_index)
                    .ok_or("Unknown source")?
            }
            "look_reverse" => {
                self.reverse = control::boolean(&value).ok_or("Expected on or off")?
            }
            "look_shift" => self.shift_percent = number(&value, 0, 100)? as u16,
            "look_spread" => self.spread_percent = number(&value, 25, 400)? as u16,
            "look_grey_hue" => self.grey_tint_hue = number(&value, 0, 359)? as u16,
            "look_grey_tint" => self.grey_tint_strength = number(&value, 0, 100)? as u16,
            "look_brightness" => self.brightness_percent = number(&value, 1, 200)? as u16,
            "look_contrast" => self.contrast_percent = number(&value, 0, 200)? as u16,
            "look_gamma" => self.gamma_percent = number(&value, 30, 300)? as u16,
            "look_saturation" => self.saturation_percent = number(&value, 0, 200)? as u16,
            "look_hue_shift" => self.hue_shift_degrees = number(&value, -180, 180)? as i16,
            "look_invert" => self.invert = control::boolean(&value).ok_or("Expected on or off")?,
            "look_vignette" => self.vignette_percent = number(&value, 0, 100)? as u16,
            "look_pattern_contrast" => {
                self.pattern_contrast_percent = number(&value, 50, 200)? as u16
            }
            "look_pattern_invert" => {
                self.pattern_invert = control::boolean(&value).ok_or("Expected on or off")?
            }
            _ => return Ok(false),
        }
        if *self != before {
            // Any hand edit leaves the named preset.
            self.preset = StylePreset::Custom;
        }
        Ok(*self != before)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> CellContext {
        CellContext {
            coverage: 0.5,
            x: 0.3,
            y: 0.6,
            seconds: 10.0,
        }
    }

    #[test]
    fn default_appearance_is_neutral_and_keeps_scene_colors() {
        let look = Appearance::default();
        assert!(look.is_neutral() && look.pattern_is_neutral());
        assert_eq!(
            look.shade([9, 9, 9], Some([10, 200, 30]), &context()),
            [10, 200, 30]
        );
        assert_eq!(look.shade([9, 9, 9], None, &context()), [9, 9, 9]);
    }

    #[test]
    fn palette_table_is_well_formed_and_ids_are_unique() {
        assert!(PALETTES[0].stops.is_empty());
        let mut ids: Vec<_> = PALETTES.iter().map(|palette| palette.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), PALETTES.len());
        for palette in &PALETTES[1..] {
            assert!(palette.stops.len() >= 3, "{}", palette.id);
        }
        assert_eq!(palette_labels().len(), PALETTES.len());
    }

    #[test]
    fn brightness_scales_every_mode_and_lowers_visibility() {
        for mode in [ColorMode::Color, ColorMode::Greyscale, ColorMode::Monotone] {
            let bright = Appearance {
                mode,
                palette: 1,
                ..Appearance::default()
            };
            let dim = Appearance {
                brightness_percent: 30,
                ..bright.clone()
            };
            let a = bright.shade([200, 200, 200], Some([180, 90, 40]), &context());
            let b = dim.shade([200, 200, 200], Some([180, 90, 40]), &context());
            let total = |color: Rgb| color.iter().map(|c| u32::from(*c)).sum::<u32>();
            assert!(total(b) * 100 < total(a) * 40, "{mode:?} {a:?} {b:?}");
        }
    }

    #[test]
    fn greyscale_is_grey_without_tint_and_tinted_with_it() {
        let grey = Appearance {
            mode: ColorMode::Greyscale,
            ..Appearance::default()
        };
        let color = grey.shade([0; 3], Some([220, 40, 40]), &context());
        assert!(color[0] == color[1] && color[1] == color[2], "{color:?}");
        let tinted = Appearance {
            grey_tint_hue: 30,
            grey_tint_strength: 80,
            ..grey
        };
        let warm = tinted.shade([0; 3], Some([220, 40, 40]), &context());
        assert!(warm[0] > warm[2], "{warm:?}");
    }

    #[test]
    fn monotone_uses_the_ink_and_ignores_scene_colors() {
        let look = Appearance {
            mode: ColorMode::Monotone,
            ..Appearance::default()
        };
        assert_eq!(
            look.shade([1, 2, 3], Some([200, 0, 0]), &context()),
            [1, 2, 3]
        );
    }

    #[test]
    fn a_palette_recolors_by_scene_brightness_and_colors_scenes_without_color() {
        let look = Appearance {
            palette: 1,
            ..Appearance::default()
        };
        let dark = look.shade([0; 3], Some([10, 10, 10]), &context());
        let light = look.shade([0; 3], Some([250, 250, 250]), &context());
        assert_ne!(dark, light);
        let sparse = CellContext {
            coverage: 0.125,
            ..context()
        };
        let dense = CellContext {
            coverage: 1.0,
            ..context()
        };
        assert_ne!(
            look.shade([0; 3], None, &sparse),
            look.shade([0; 3], None, &dense)
        );
    }

    #[test]
    fn position_sources_differ_and_reverse_swaps_the_ends() {
        let base = Appearance {
            palette: 1,
            source: ColorSource::Vertical,
            ..Appearance::default()
        };
        let top = CellContext {
            y: 0.0,
            ..context()
        };
        let bottom = CellContext {
            y: 1.0,
            ..context()
        };
        assert_ne!(
            base.shade([0; 3], None, &top),
            base.shade([0; 3], None, &bottom)
        );
        let reversed = Appearance {
            reverse: true,
            ..base.clone()
        };
        assert_eq!(
            reversed.shade([0; 3], None, &top),
            base.shade([0; 3], None, &bottom)
        );
        let drift = Appearance {
            source: ColorSource::Drift,
            ..base
        };
        let later = CellContext {
            seconds: 33.0,
            ..context()
        };
        assert_ne!(
            drift.shade([0; 3], None, &context()),
            drift.shade([0; 3], None, &later)
        );
    }

    #[test]
    fn adjustments_behave_at_their_extremes() {
        let ctx = context();
        let white = [255, 255, 255];
        assert_eq!(
            Appearance {
                invert: true,
                ..Appearance::default()
            }
            .adjust(white, &ctx),
            [0, 0, 0]
        );
        let none = Appearance {
            saturation_percent: 0,
            ..Appearance::default()
        }
        .adjust([200, 50, 50], &ctx);
        assert!(none[0] == none[1] && none[1] == none[2], "{none:?}");
        let shifted = Appearance {
            hue_shift_degrees: 120,
            ..Appearance::default()
        }
        .adjust([255, 0, 0], &ctx);
        assert!(shifted[1] > shifted[0], "{shifted:?}");
        let edge = Appearance {
            vignette_percent: 100,
            ..Appearance::default()
        };
        let corner = CellContext {
            x: 0.0,
            y: 0.0,
            ..ctx
        };
        assert_eq!(edge.adjust(white, &corner), [0, 0, 0]);
        assert_eq!(
            edge.adjust(
                white,
                &CellContext {
                    x: 0.5,
                    y: 0.5,
                    ..ctx
                }
            ),
            white
        );
    }

    #[test]
    fn every_preset_applies_and_stays_in_range() {
        for preset in StylePreset::ALL {
            let mut look = Appearance {
                brightness_percent: 7,
                ..Appearance::default()
            };
            let applied = look.apply_preset(preset);
            assert_eq!(applied.is_some(), preset != StylePreset::Custom);
            assert_eq!(look, look.normalized(), "{preset:?}");
            if preset != StylePreset::Custom {
                assert_eq!(look.preset, preset);
            }
        }
        assert_eq!(StylePreset::labels().len(), StylePreset::ALL.len());
    }

    #[test]
    fn editing_a_value_returns_the_preset_to_custom() {
        let mut look = Appearance::default();
        look.set_control(
            "look_preset",
            ControlValue::Index(StylePreset::Whisper.index()),
        )
        .unwrap();
        assert_eq!(look.preset, StylePreset::Whisper);
        assert_eq!(look.mode, ColorMode::Greyscale);
        look.set_control("look_brightness", ControlValue::Number(80))
            .unwrap();
        assert_eq!(look.preset, StylePreset::Custom);
    }

    #[test]
    fn every_row_round_trips_and_visibility_follows_the_mode() {
        for mode in ColorMode::LABELS
            .iter()
            .enumerate()
            .filter_map(|(i, _)| ColorMode::from_index(i))
        {
            let mut look = Appearance {
                mode,
                ..Appearance::default()
            };
            let ids: Vec<_> = look.controls().iter().map(|row| row.id).collect();
            assert_eq!(ids.contains(&"look_palette"), mode == ColorMode::Color);
            assert_eq!(
                ids.contains(&"look_grey_tint"),
                mode == ColorMode::Greyscale
            );
            assert_eq!(ids.contains(&"look_shift"), mode != ColorMode::Monotone);
            for row in look.controls().into_iter().chain(look.pattern_controls()) {
                let stepped = row.stepped(1).expect("steppable");
                look.set_control(row.id, stepped)
                    .unwrap_or_else(|error| panic!("{}: {error}", row.id));
            }
            assert_eq!(look, look.normalized());
        }
        assert_eq!(
            Appearance::default().set_control("nope", ControlValue::Bool(true)),
            Ok(false)
        );
        assert!(Appearance::default()
            .set_control("look_invert", ControlValue::Number(1))
            .is_err());
        assert!(Appearance::default()
            .set_control("look_palette", ControlValue::Index(999))
            .is_err());
    }

    #[test]
    fn normalization_clamps_hostile_values() {
        let wild = Appearance {
            palette: 9999,
            brightness_percent: 0,
            spread_percent: 0,
            gamma_percent: 9000,
            hue_shift_degrees: 9000,
            pattern_contrast_percent: 0,
            ..Appearance::default()
        }
        .normalized();
        assert_eq!(
            (wild.palette, wild.brightness_percent, wild.spread_percent),
            (PALETTES.len() - 1, 1, 25)
        );
        assert_eq!(
            (
                wild.gamma_percent,
                wild.hue_shift_degrees,
                wild.pattern_contrast_percent
            ),
            (300, 180, 50)
        );
        // Extreme (clamped) settings still produce a colour, never a panic.
        let color = wild.shade([0; 3], None, &context());
        assert_eq!(color.len(), 3);
    }

    #[test]
    fn pattern_shaping_inverts_and_changes_contrast() {
        let look = Appearance {
            pattern_invert: true,
            ..Appearance::default()
        };
        assert!((look.shape_dot(0.2) - 0.8).abs() < 1e-6);
        let hard = Appearance {
            pattern_contrast_percent: 200,
            ..Appearance::default()
        };
        assert!(hard.shape_dot(0.6) > 0.6 && hard.shape_dot(0.4) < 0.4);
    }

    #[test]
    fn serde_defaults_fill_missing_fields() {
        let look: Appearance = serde_json::from_str("{\"brightness_percent\": 40}").unwrap();
        assert_eq!(look.brightness_percent, 40);
        assert_eq!(look.mode, ColorMode::Color);
        let json = serde_json::to_string(&look).unwrap();
        assert_eq!(serde_json::from_str::<Appearance>(&json).unwrap(), look);
    }
}

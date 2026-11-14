//! User-facing settings of the self-playing tower defense scene.
//!
//! Serde field names are the YAML keys users see. Every numeric field has a
//! documented range that `normalized` enforces.

use crate::control::{self, Control, ControlValue, SceneSettings};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Palette {
    /// One tone: every dot uses the global palette.
    BlackWhite,
    /// Every tower, monster and shot has its own colour.
    #[default]
    Colour,
}

impl Palette {
    pub const ALL: [Self; 2] = [Self::BlackWhite, Self::Colour];
    pub const LABELS: [&'static str; 2] = ["Black and white", "Colour"];

    pub fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|candidate| *candidate == self)
            .unwrap_or(0)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scheme {
    /// Cyan, magenta, yellow and green on black: the original vector look.
    #[default]
    Neon,
    Cool,
    Warm,
    /// Everything in shades of one green.
    Phosphor,
}

impl Scheme {
    pub const ALL: [Self; 4] = [Self::Neon, Self::Cool, Self::Warm, Self::Phosphor];
    pub const LABELS: [&'static str; 4] = ["Neon", "Cool", "Warm", "Phosphor"];

    pub fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|candidate| *candidate == self)
            .unwrap_or(0)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Difficulty {
    Easy,
    #[default]
    Normal,
    Hard,
}

impl Difficulty {
    pub const ALL: [Self; 3] = [Self::Easy, Self::Normal, Self::Hard];
    pub const LABELS: [&'static str; 3] = ["Easy", "Normal", "Hard"];

    pub fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|candidate| *candidate == self)
            .unwrap_or(0)
    }

    /// Monster health multiplier.
    pub fn health(self) -> f32 {
        match self {
            Self::Easy => 0.8,
            Self::Normal => 1.0,
            Self::Hard => 1.3,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VectorTdSettings {
    /// 0 plays every map in turn, 1..=6 stays on one map.
    pub map: u32,
    /// Level the game starts at, 1..=12. Later levels unlock more towers.
    pub start_level: u32,
    /// Waves in a level, 5..=40. Default 20.
    pub waves: u32,
    pub difficulty: Difficulty,
    /// Game speed in percent, 25..=400, on top of the global Speed. Default 100.
    pub game_speed: u32,
    pub palette: Palette,
    pub scheme: Scheme,
    /// Brightness in percent, 20..=200. Default 100.
    pub brightness: u32,
    /// Contrast in percent, 50..=200. Default 100.
    pub contrast: u32,
    /// Hue rotation in degrees, 0..=360. Default 0.
    pub hue: u32,
    /// Saturation in percent, 0..=200. Default 100.
    pub saturation: u32,
    /// How strongly shapes are filled instead of only outlined, 0..=100. Default 45.
    pub glow: u32,
    pub grid: bool,
    /// Keep every tower's range ring visible.
    pub range_rings: bool,
    pub hud: bool,
    /// Seed of the random details (spawn jitter, missile spread), 0..=999.
    pub seed: u32,
}

impl Default for VectorTdSettings {
    fn default() -> Self {
        Self {
            map: 0,
            start_level: 1,
            waves: 20,
            difficulty: Difficulty::Normal,
            game_speed: 100,
            palette: Palette::Colour,
            scheme: Scheme::Neon,
            brightness: 100,
            contrast: 100,
            hue: 0,
            saturation: 100,
            glow: 45,
            grid: true,
            range_rings: false,
            hud: true,
            seed: 0,
        }
    }
}

/// The settings that change what is played. Any other setting only changes
/// how it looks or how fast it runs, so a running game survives its edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Gameplay {
    pub map: u32,
    pub start_level: u32,
    pub waves: u32,
    pub difficulty: Difficulty,
    pub seed: u32,
}

impl VectorTdSettings {
    pub fn gameplay(&self) -> Gameplay {
        Gameplay {
            map: self.map,
            start_level: self.start_level,
            waves: self.waves,
            difficulty: self.difficulty,
            seed: self.seed,
        }
    }
}

const MAP: (i32, i32) = (0, 6);
const START_LEVEL: (i32, i32) = (1, 12);
const WAVES: (i32, i32) = (5, 40);
const GAME_SPEED: (i32, i32) = (25, 400);
const BRIGHTNESS: (i32, i32) = (20, 200);
const CONTRAST: (i32, i32) = (50, 200);
const HUE: (i32, i32) = (0, 360);
const SATURATION: (i32, i32) = (0, 200);
const GLOW: (i32, i32) = (0, 100);
const SEED: (i32, i32) = (0, 999);

pub const MAP_LABELS: [&str; 7] = [
    "All in turn",
    "Switchback",
    "Spiral",
    "Comb",
    "Twin gates",
    "Staircase",
    "Serpent",
];

fn clamp_unsigned((min, max): (i32, i32), value: u32) -> u32 {
    i32::try_from(value).unwrap_or(i32::MAX).clamp(min, max) as u32
}

fn number_of(value: &ControlValue, label: &str) -> Result<i32, String> {
    control::number(value).ok_or_else(|| format!("{label} expects a number"))
}

fn set_unsigned(
    field: &mut u32,
    range: (i32, i32),
    value: &ControlValue,
    label: &str,
) -> Result<bool, String> {
    let clamped = number_of(value, label)?.clamp(range.0, range.1) as u32;
    Ok(replace(field, clamped))
}

fn set_toggle(field: &mut bool, value: &ControlValue, label: &str) -> Result<bool, String> {
    let on = control::boolean(value).ok_or_else(|| format!("{label} expects on or off"))?;
    Ok(replace(field, on))
}

fn replace<T: PartialEq>(field: &mut T, value: T) -> bool {
    if *field == value {
        return false;
    }
    *field = value;
    true
}

fn slider(
    id: &'static str,
    label: &'static str,
    value: u32,
    range: (i32, i32),
    step: i32,
    unit: &'static str,
    help: &'static str,
) -> Control {
    Control::slider(
        id,
        label,
        value as i32,
        (range.0, range.1, step),
        unit,
        help,
    )
}

impl SceneSettings for VectorTdSettings {
    fn normalized(&self) -> Self {
        Self {
            map: clamp_unsigned(MAP, self.map),
            start_level: clamp_unsigned(START_LEVEL, self.start_level),
            waves: clamp_unsigned(WAVES, self.waves),
            game_speed: clamp_unsigned(GAME_SPEED, self.game_speed),
            brightness: clamp_unsigned(BRIGHTNESS, self.brightness),
            contrast: clamp_unsigned(CONTRAST, self.contrast),
            hue: clamp_unsigned(HUE, self.hue),
            saturation: clamp_unsigned(SATURATION, self.saturation),
            glow: clamp_unsigned(GLOW, self.glow),
            seed: clamp_unsigned(SEED, self.seed),
            ..self.clone()
        }
    }

    fn controls(&self) -> Vec<Control> {
        let mut rows = vec![
            Control::choice(
                "map",
                "Map",
                self.map as usize,
                &MAP_LABELS,
                "Play every map in turn, moving on after each cleared level, or stay on one map. Changing it restarts the game.",
            ),
            slider(
                "start_level",
                "Start level",
                self.start_level,
                START_LEVEL,
                1,
                "",
                "Later levels start with more money, stronger monsters and more tower types unlocked. Changing it restarts the game.",
            ),
            slider(
                "waves",
                "Waves per level",
                self.waves,
                WAVES,
                1,
                "",
                "Waves to survive before a level is cleared; every tenth wave brings a boss.",
            ),
            Control::choice(
                "difficulty",
                "Difficulty",
                self.difficulty.index(),
                &Difficulty::LABELS,
                "Monster health. On Hard the AI sometimes loses a level and retries it with stronger towers.",
            ),
            slider(
                "game_speed",
                "Game speed",
                self.game_speed,
                GAME_SPEED,
                25,
                "%",
                "How fast the game plays, on top of the global Speed setting.",
            ),
            Control::choice(
                "palette",
                "Colours",
                self.palette.index(),
                &Palette::LABELS,
                "Black and white uses only the global palette; Colour gives every tower, monster and shot its own colour.",
            ),
        ];
        if self.palette == Palette::Colour {
            rows.push(Control::choice(
                "scheme",
                "Colour scheme",
                self.scheme.index(),
                &Scheme::LABELS,
                "The base colours. The sliders below adjust them further.",
            ));
        }
        rows.push(slider(
            "brightness",
            "Brightness",
            self.brightness,
            BRIGHTNESS,
            5,
            "%",
            "How bright the lines and fills are. Lower values thin the dots out.",
        ));
        rows.push(slider(
            "contrast",
            "Contrast",
            self.contrast,
            CONTRAST,
            5,
            "%",
            "Spreads the difference between dim and bright parts, such as grid against towers.",
        ));
        if self.palette == Palette::Colour {
            rows.push(slider(
                "hue",
                "Hue",
                self.hue,
                HUE,
                10,
                "deg",
                "Rotates every colour around the colour wheel.",
            ));
            rows.push(slider(
                "saturation",
                "Saturation",
                self.saturation,
                SATURATION,
                5,
                "%",
                "0 is grey, 100 the scheme as designed, 200 as vivid as possible.",
            ));
        }
        rows.push(slider(
            "glow",
            "Glow",
            self.glow,
            GLOW,
            5,
            "%",
            "How strongly towers, monsters and the path are filled rather than only outlined.",
        ));
        rows.push(Control::toggle(
            "grid",
            "Grid",
            self.grid,
            "Draw the faint cell grid of the board.",
        ));
        rows.push(Control::toggle(
            "range_rings",
            "Range rings",
            self.range_rings,
            "Always show the firing range of every tower, not only when it is built or upgraded.",
        ));
        rows.push(Control::toggle(
            "hud",
            "Status line",
            self.hud,
            "Show level, wave, money and lives along the top.",
        ));
        rows.push(slider(
            "seed",
            "Seed",
            self.seed,
            SEED,
            1,
            "",
            "Seeds the random details of the game. Changing it restarts the game.",
        ));
        rows
    }

    fn set_control(&mut self, id: &str, value: ControlValue) -> Result<bool, String> {
        match id {
            "map" => {
                let index = match &value {
                    ControlValue::Index(index) => *index as i32,
                    other => number_of(other, "Map")?,
                };
                let clamped = index.clamp(MAP.0, MAP.1) as u32;
                Ok(replace(&mut self.map, clamped))
            }
            "start_level" => {
                set_unsigned(&mut self.start_level, START_LEVEL, &value, "Start level")
            }
            "waves" => set_unsigned(&mut self.waves, WAVES, &value, "Waves"),
            "difficulty" => {
                let index = control::index(&value).ok_or("Difficulty expects a choice")?;
                let next = *Difficulty::ALL.get(index).ok_or("Unknown difficulty")?;
                Ok(replace(&mut self.difficulty, next))
            }
            "game_speed" => set_unsigned(&mut self.game_speed, GAME_SPEED, &value, "Game speed"),
            "palette" => {
                let index = control::index(&value).ok_or("Colours expects a choice")?;
                let next = *Palette::ALL.get(index).ok_or("Unknown palette")?;
                Ok(replace(&mut self.palette, next))
            }
            "scheme" => {
                let index = control::index(&value).ok_or("Colour scheme expects a choice")?;
                let next = *Scheme::ALL.get(index).ok_or("Unknown colour scheme")?;
                Ok(replace(&mut self.scheme, next))
            }
            "brightness" => set_unsigned(&mut self.brightness, BRIGHTNESS, &value, "Brightness"),
            "contrast" => set_unsigned(&mut self.contrast, CONTRAST, &value, "Contrast"),
            "hue" => set_unsigned(&mut self.hue, HUE, &value, "Hue"),
            "saturation" => set_unsigned(&mut self.saturation, SATURATION, &value, "Saturation"),
            "glow" => set_unsigned(&mut self.glow, GLOW, &value, "Glow"),
            "grid" => set_toggle(&mut self.grid, &value, "Grid"),
            "range_rings" => set_toggle(&mut self.range_rings, &value, "Range rings"),
            "hud" => set_toggle(&mut self.hud, &value, "Status line"),
            "seed" => set_unsigned(&mut self.seed, SEED, &value, "Seed"),
            _ => Ok(false),
        }
    }
}

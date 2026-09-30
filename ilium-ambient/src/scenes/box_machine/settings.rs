//! User-facing settings of the box machine scene.
//!
//! Serde field names are the YAML keys users see. Every numeric field has a
//! documented range that `normalized` enforces.

use crate::control::{self, Control, ControlValue, SceneSettings};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BoxMachineSettings {
    /// Selects the machine layout, 0..=9999. Default 7.
    pub seed: u32,
    /// Dots per tile edge, a multiple of 4 in 12..=28. Default 20.
    pub tile_size: u32,
    /// How much of the floor carries rails instead of staying empty, 5..=100
    /// percent. Default 70.
    pub density: u32,
    /// Most screen tiles the machine may contain, 0..=6. Default 2.
    pub screens: u32,
    /// Box speed in dots per second, 1..=20. Default 6.
    pub speed: u32,
    /// Distance between boxes on one loop in dots, 16..=48. Default 28.
    pub spacing: u32,
    /// Box edge length in dots, 3..=6. Default 4.
    pub cube_size: u32,
    /// Rail brightness in percent, 0..=60. Default 35.
    pub rail_level: u32,
    /// Box brightness in percent, 10..=100. Default 80.
    pub cube_level: u32,
    /// Draw rails as a dotted track instead of a solid line. Default false.
    pub rails_dashed: bool,
    /// Neighbouring loops run in opposite directions. Default true.
    pub reverse_alt: bool,
}

impl Default for BoxMachineSettings {
    fn default() -> Self {
        Self {
            seed: 7,
            tile_size: 20,
            density: 70,
            screens: 2,
            speed: 6,
            spacing: 28,
            cube_size: 4,
            rail_level: 35,
            cube_level: 80,
            rails_dashed: false,
            reverse_alt: true,
        }
    }
}

const SEED: (u32, u32) = (0, 9999);
const TILE_SIZE: (u32, u32) = (12, 28);
const DENSITY: (u32, u32) = (5, 100);
const SCREENS: (u32, u32) = (0, 6);
const SPEED: (u32, u32) = (1, 20);
const SPACING: (u32, u32) = (16, 48);
const CUBE_SIZE: (u32, u32) = (3, 6);
const RAIL_LEVEL: (u32, u32) = (0, 60);
const CUBE_LEVEL: (u32, u32) = (10, 100);

fn clamp((min, max): (u32, u32), value: u32) -> u32 {
    value.clamp(min, max)
}

/// Tile sizes must be divisible by 4 so the three ports per edge sit on
/// whole dots.
fn snap_tile_size(value: u32) -> u32 {
    (clamp(TILE_SIZE, value) + 2) / 4 * 4
}

fn snap_spacing(value: u32) -> u32 {
    (clamp(SPACING, value) + 2) / 4 * 4
}

fn range_row(
    id: &'static str,
    label: &'static str,
    value: u32,
    range: (u32, u32),
    step: i32,
    unit: &'static str,
    help: &'static str,
) -> Control {
    Control::slider(
        id,
        label,
        value as i32,
        (range.0 as i32, range.1 as i32, step),
        unit,
        help,
    )
}

fn set_number(
    field: &mut u32,
    value: &ControlValue,
    label: &str,
    normalize: impl FnOnce(u32) -> u32,
) -> Result<bool, String> {
    let number = control::number(value).ok_or_else(|| format!("{label} expects a number"))?;
    let stored = normalize(number.max(0) as u32);
    Ok(replace(field, stored))
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

impl SceneSettings for BoxMachineSettings {
    fn normalized(&self) -> Self {
        Self {
            seed: clamp(SEED, self.seed),
            tile_size: snap_tile_size(self.tile_size),
            density: clamp(DENSITY, self.density),
            screens: clamp(SCREENS, self.screens),
            speed: clamp(SPEED, self.speed),
            spacing: snap_spacing(self.spacing),
            cube_size: clamp(CUBE_SIZE, self.cube_size),
            rail_level: clamp(RAIL_LEVEL, self.rail_level),
            cube_level: clamp(CUBE_LEVEL, self.cube_level),
            rails_dashed: self.rails_dashed,
            reverse_alt: self.reverse_alt,
        }
    }

    fn controls(&self) -> Vec<Control> {
        vec![
            range_row(
                "seed",
                "Machine",
                self.seed,
                SEED,
                1,
                "",
                "The number that selects the machine layout. Every number builds a different machine.",
            ),
            range_row(
                "tile_size",
                "Tile size",
                self.tile_size,
                TILE_SIZE,
                4,
                " dots",
                "Edge of one machine tile. Smaller tiles make a denser machine with more loops.",
            ),
            range_row(
                "density",
                "Rail density",
                self.density,
                DENSITY,
                5,
                "%",
                "How much of the floor carries rails. Low values leave large empty areas.",
            ),
            range_row(
                "screens",
                "Screens",
                self.screens,
                SCREENS,
                1,
                "",
                "Most little screens (dithered gradient and scrolling marquee) the machine may hold. 0 removes them.",
            ),
            range_row(
                "speed",
                "Box speed",
                self.speed,
                SPEED,
                1,
                " dots/s",
                "How fast boxes slide along the rails. Multiplied by the global Speed setting.",
            ),
            range_row(
                "spacing",
                "Box spacing",
                self.spacing,
                SPACING,
                4,
                " dots",
                "Distance between boxes on one loop. Smaller values put more boxes on the rails.",
            ),
            range_row(
                "cube_size",
                "Box size",
                self.cube_size,
                CUBE_SIZE,
                1,
                " dots",
                "Edge length of each box.",
            ),
            range_row(
                "rail_level",
                "Rail brightness",
                self.rail_level,
                RAIL_LEVEL,
                2,
                "%",
                "Brightness of the rails. 0% hides them so the boxes seem to float.",
            ),
            range_row(
                "cube_level",
                "Box brightness",
                self.cube_level,
                CUBE_LEVEL,
                5,
                "%",
                "Brightness of the box outlines. Their centers are drawn at half of it.",
            ),
            Control::toggle(
                "rails_dashed",
                "Dotted rails",
                self.rails_dashed,
                "Draw rails as a dotted track instead of a solid line.",
            ),
            Control::toggle(
                "reverse_alt",
                "Alternate directions",
                self.reverse_alt,
                "Loops take turns running clockwise and counterclockwise. Off sends every box the same way round.",
            ),
        ]
    }

    fn set_control(&mut self, id: &str, value: ControlValue) -> Result<bool, String> {
        match id {
            "seed" => set_number(&mut self.seed, &value, "Machine", |v| clamp(SEED, v)),
            "tile_size" => set_number(&mut self.tile_size, &value, "Tile size", snap_tile_size),
            "density" => set_number(&mut self.density, &value, "Rail density", |v| {
                clamp(DENSITY, v)
            }),
            "screens" => set_number(&mut self.screens, &value, "Screens", |v| clamp(SCREENS, v)),
            "speed" => set_number(&mut self.speed, &value, "Box speed", |v| clamp(SPEED, v)),
            "spacing" => set_number(&mut self.spacing, &value, "Box spacing", snap_spacing),
            "cube_size" => set_number(&mut self.cube_size, &value, "Box size", |v| {
                clamp(CUBE_SIZE, v)
            }),
            "rail_level" => set_number(&mut self.rail_level, &value, "Rail brightness", |v| {
                clamp(RAIL_LEVEL, v)
            }),
            "cube_level" => set_number(&mut self.cube_level, &value, "Box brightness", |v| {
                clamp(CUBE_LEVEL, v)
            }),
            "rails_dashed" => set_toggle(&mut self.rails_dashed, &value, "Dotted rails"),
            "reverse_alt" => set_toggle(&mut self.reverse_alt, &value, "Alternate directions"),
            _ => Ok(false),
        }
    }
}

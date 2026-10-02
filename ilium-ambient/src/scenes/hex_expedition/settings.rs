//! User-facing settings of the hex expedition map scene.
//!
//! Serde field names are the YAML keys users see. Every numeric field has a
//! documented range that `normalized` enforces.

use crate::control::{self, Control, ControlValue, SceneSettings};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MapChoice {
    /// Walk through every map type in turn, revealing the next one hex by hex.
    #[default]
    Cycle,
    Jungle,
    Savanna,
    Desert,
    Arctic,
    Volcanic,
}

impl MapChoice {
    pub const ALL: [Self; 6] = [
        Self::Cycle,
        Self::Jungle,
        Self::Savanna,
        Self::Desert,
        Self::Arctic,
        Self::Volcanic,
    ];
    pub const LABELS: [&'static str; 6] = [
        "All in turn",
        "Jungle",
        "Savanna",
        "Desert",
        "Arctic",
        "Volcanic",
    ];

    pub fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|candidate| *candidate == self)
            .unwrap_or(0)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PanStyle {
    /// A slow, curving expedition route.
    #[default]
    Wander,
    East,
    NorthEast,
    South,
}

impl PanStyle {
    pub const ALL: [Self; 4] = [Self::Wander, Self::East, Self::NorthEast, Self::South];
    pub const LABELS: [&'static str; 4] = ["Wandering route", "East", "North-east", "South"];

    pub fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|candidate| *candidate == self)
            .unwrap_or(0)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HexExpeditionSettings {
    pub map: MapChoice,
    /// Seconds each map type stays before the next is revealed, 20..=300.
    pub map_seconds: u32,
    /// Hex radius in Braille dots, 10..=40. Default 20.
    pub tile_size: u32,
    /// Pan speed in percent, 0..=300. 0 holds the camera still. Default 100.
    pub pan_speed: u32,
    pub pan_style: PanStyle,
    /// Speed of tile animations in percent, 0..=300. Default 100.
    pub animation_speed: u32,
    /// Landmark frequency in percent, 0..=200. Default 100.
    pub landmarks: u32,
    /// Brightness in percent, 50..=150. Default 100.
    pub brightness: u32,
    /// Quantise tile animation to a few frames per second, like pixel art.
    pub stepped: bool,
    /// Falling snow, embers, dust and fireflies per map type.
    pub weather: bool,
    /// Drifting cloud shadows over the map.
    pub cloud_shadows: bool,
    /// Biome colours instead of the global palette.
    pub tint: bool,
    /// World seed, 0..=999. Default 7.
    pub seed: u32,
}

impl Default for HexExpeditionSettings {
    fn default() -> Self {
        Self {
            map: MapChoice::Cycle,
            map_seconds: 60,
            tile_size: 20,
            pan_speed: 100,
            pan_style: PanStyle::Wander,
            animation_speed: 100,
            landmarks: 100,
            brightness: 100,
            stepped: true,
            weather: true,
            cloud_shadows: true,
            tint: true,
            seed: 7,
        }
    }
}

const MAP_SECONDS: (i32, i32) = (20, 300);
const TILE_SIZE: (i32, i32) = (10, 40);
const PAN_SPEED: (i32, i32) = (0, 300);
const ANIMATION_SPEED: (i32, i32) = (0, 300);
const LANDMARKS: (i32, i32) = (0, 200);
const BRIGHTNESS: (i32, i32) = (50, 150);
const SEED: (i32, i32) = (0, 999);

fn clamp_unsigned((min, max): (i32, i32), value: u32) -> u32 {
    i32::try_from(value).unwrap_or(i32::MAX).clamp(min, max) as u32
}

fn number_of(value: &ControlValue, label: &str) -> Result<i32, String> {
    control::number(value).ok_or_else(|| format!("{label} expects a number"))
}

fn replace<T: PartialEq>(field: &mut T, value: T) -> bool {
    if *field == value {
        return false;
    }
    *field = value;
    true
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

fn slider(
    id: &'static str,
    label: &'static str,
    value: i32,
    range: (i32, i32),
    step: i32,
    unit: &'static str,
    help: &'static str,
) -> Control {
    Control::slider(id, label, value, (range.0, range.1, step), unit, help)
}

impl SceneSettings for HexExpeditionSettings {
    fn normalized(&self) -> Self {
        Self {
            map_seconds: clamp_unsigned(MAP_SECONDS, self.map_seconds),
            tile_size: clamp_unsigned(TILE_SIZE, self.tile_size),
            pan_speed: clamp_unsigned(PAN_SPEED, self.pan_speed),
            animation_speed: clamp_unsigned(ANIMATION_SPEED, self.animation_speed),
            landmarks: clamp_unsigned(LANDMARKS, self.landmarks),
            brightness: clamp_unsigned(BRIGHTNESS, self.brightness),
            seed: clamp_unsigned(SEED, self.seed),
            ..self.clone()
        }
    }

    fn controls(&self) -> Vec<Control> {
        let mut rows = vec![Control::choice(
            "map",
            "Map type",
            self.map.index(),
            &MapChoice::LABELS,
            "Which kind of expedition map to draw. All in turn reveals each type in the next, hex by hex.",
        )];
        if self.map == MapChoice::Cycle {
            rows.push(slider(
                "map_seconds",
                "Seconds per map",
                self.map_seconds as i32,
                MAP_SECONDS,
                10,
                " s",
                "How long each map type stays on screen before the next one is revealed.",
            ));
        }
        rows.extend([
            slider(
                "tile_size",
                "Hex size",
                self.tile_size as i32,
                TILE_SIZE,
                2,
                " dots",
                "Radius of one hex in Braille dots. Small hexes show more map; large ones show more sprite detail.",
            ),
            slider(
                "pan_speed",
                "Pan speed",
                self.pan_speed as i32,
                PAN_SPEED,
                10,
                "%",
                "How fast the camera travels over the map, on top of the global Speed setting. 0% holds it still.",
            ),
            Control::choice(
                "pan_style",
                "Pan route",
                self.pan_style.index(),
                &PanStyle::LABELS,
                "A slowly curving expedition route, or a straight pan in one direction.",
            ),
            slider(
                "animation_speed",
                "Tile animation",
                self.animation_speed as i32,
                ANIMATION_SPEED,
                10,
                "%",
                "Speed of waves, swaying trees, smoke, lava and fires. 0% freezes the tiles.",
            ),
            Control::toggle(
                "stepped",
                "Stepped tile frames",
                self.stepped,
                "Plays tile animations as a few hand-drawn frames per second instead of smoothly. The camera stays smooth.",
            ),
            slider(
                "landmarks",
                "Landmarks",
                self.landmarks as i32,
                LANDMARKS,
                10,
                "%",
                "How many villages, camps, ruins, caves, mines and shrines are scattered over each island. Every island keeps its one ship and one goal while this is above 0%.",
            ),
            Control::toggle(
                "weather",
                "Weather",
                self.weather,
                "Adds falling snow, rising embers, blown sand or fireflies, depending on the map type.",
            ),
            Control::toggle(
                "cloud_shadows",
                "Cloud shadows",
                self.cloud_shadows,
                "Lets the shadows of drifting clouds pass over the map.",
            ),
            slider(
                "brightness",
                "Brightness",
                self.brightness as i32,
                BRIGHTNESS,
                5,
                "%",
                "Scales tile brightness before the global dither is applied.",
            ),
            Control::toggle(
                "tint",
                "Biome colors",
                self.tint,
                "Colours each tile by terrain. Off uses your global palette.",
            ),
            slider(
                "seed",
                "Seed number",
                self.seed as i32,
                SEED,
                1,
                "",
                "Selects the world. The same seed always generates the same maps.",
            ),
        ]);
        rows
    }

    fn set_control(&mut self, id: &str, value: ControlValue) -> Result<bool, String> {
        match id {
            "map" => {
                let index = control::index(&value)
                    .ok_or_else(|| "Map type expects an option".to_owned())?;
                let map = MapChoice::ALL
                    .get(index)
                    .copied()
                    .ok_or_else(|| format!("Map type: option {index} does not exist"))?;
                Ok(replace(&mut self.map, map))
            }
            "map_seconds" => set_unsigned(
                &mut self.map_seconds,
                MAP_SECONDS,
                &value,
                "Seconds per map",
            ),
            "tile_size" => set_unsigned(&mut self.tile_size, TILE_SIZE, &value, "Hex size"),
            "pan_speed" => set_unsigned(&mut self.pan_speed, PAN_SPEED, &value, "Pan speed"),
            "pan_style" => {
                let index = control::index(&value)
                    .ok_or_else(|| "Pan route expects an option".to_owned())?;
                let style = PanStyle::ALL
                    .get(index)
                    .copied()
                    .ok_or_else(|| format!("Pan route: option {index} does not exist"))?;
                Ok(replace(&mut self.pan_style, style))
            }
            "animation_speed" => set_unsigned(
                &mut self.animation_speed,
                ANIMATION_SPEED,
                &value,
                "Tile animation",
            ),
            "stepped" => set_toggle(&mut self.stepped, &value, "Stepped tile frames"),
            "landmarks" => set_unsigned(&mut self.landmarks, LANDMARKS, &value, "Landmarks"),
            "weather" => set_toggle(&mut self.weather, &value, "Weather"),
            "cloud_shadows" => set_toggle(&mut self.cloud_shadows, &value, "Cloud shadows"),
            "brightness" => set_unsigned(&mut self.brightness, BRIGHTNESS, &value, "Brightness"),
            "tint" => set_toggle(&mut self.tint, &value, "Biome colors"),
            "seed" => set_unsigned(&mut self.seed, SEED, &value, "Seed number"),
            _ => Ok(false),
        }
    }
}

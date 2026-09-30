//! User-facing settings of the Atlantic dusk scene.
//!
//! Serde field names are the YAML keys users see. Every numeric field has a
//! documented range that `normalized` enforces.

use crate::control::{self, Control, ControlValue, SceneSettings};
use serde::{Deserialize, Serialize};

/// Which part of the day cycle is shown.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimeOfDay {
    /// The full day animates: day, dusk, night, dawn.
    #[default]
    Cycle,
    Dawn,
    Noon,
    Dusk,
    Night,
}

impl TimeOfDay {
    const ALL: [Self; 5] = [Self::Cycle, Self::Dawn, Self::Noon, Self::Dusk, Self::Night];
    const LABELS: [&'static str; 5] = ["Cycle", "Dawn", "Noon", "Dusk", "Night"];

    fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|candidate| *candidate == self)
            .unwrap_or(0)
    }

    /// The pinned phase of the day (0 dawn start, 0.25 noon, 0.5 dusk,
    /// 0.75 midnight), or `None` while the cycle animates.
    pub fn pinned_phase(self) -> Option<f32> {
        match self {
            Self::Cycle => None,
            Self::Dawn => Some(0.03),
            Self::Noon => Some(0.25),
            Self::Dusk => Some(0.5),
            Self::Night => Some(0.75),
        }
    }
}

/// How the smooth gradients become dots.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DitherStyle {
    /// Soft intensities; the host applies its own (screen-fixed) dither.
    #[default]
    Host,
    /// The scene's own fine 8x8 Bayer dither with stepped gradient bands.
    Bayer8,
    /// The scene's own chunkier 4x4 Bayer dither.
    Bayer4,
}

impl DitherStyle {
    const ALL: [Self; 3] = [Self::Host, Self::Bayer8, Self::Bayer4];
    const LABELS: [&'static str; 3] = ["Host dither", "Bayer 8x8", "Bayer 4x4"];

    fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|candidate| *candidate == self)
            .unwrap_or(0)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AtlanticDuskSettings {
    /// Seconds for one full day at speed 1, 30..=600. Default 120.
    pub day_length_seconds: u32,
    pub time_of_day: TimeOfDay,
    /// Share of the sky covered by clouds in percent, 0..=100. Default 45.
    pub cloud_coverage: u32,
    /// Cloud drift speed in percent, 0..=300. Default 100.
    pub cloud_speed: u32,
    /// Wave animation speed in percent, 0..=300. Default 100.
    pub wave_speed: u32,
    /// Wave line spacing in percent, 50..=200. Default 100.
    pub wave_scale: u32,
    /// Night stars per thousand sky cells, 0..=80. Default 30.
    pub star_density: u32,
    /// Overall picture intensity in percent, 20..=100. Default 55.
    pub contrast: u32,
    /// Let a few single-dot stars blink out briefly. Default on.
    pub twinkle: bool,
    pub dither: DitherStyle,
    /// Seed of the clouds and stars, 0..=999. Default 41.
    pub seed: u32,
}

impl Default for AtlanticDuskSettings {
    fn default() -> Self {
        Self {
            day_length_seconds: 120,
            time_of_day: TimeOfDay::Cycle,
            cloud_coverage: 45,
            cloud_speed: 100,
            wave_speed: 100,
            wave_scale: 100,
            star_density: 30,
            contrast: 55,
            twinkle: true,
            dither: DitherStyle::Host,
            seed: 41,
        }
    }
}

const DAY_LENGTH: (u32, u32) = (30, 600);
const CLOUD_COVERAGE: (u32, u32) = (0, 100);
const CLOUD_SPEED: (u32, u32) = (0, 300);
const WAVE_SPEED: (u32, u32) = (0, 300);
const WAVE_SCALE: (u32, u32) = (50, 200);
const STAR_DENSITY: (u32, u32) = (0, 80);
const CONTRAST: (u32, u32) = (20, 100);
const SEED: (u32, u32) = (0, 999);

fn clamp((min, max): (u32, u32), value: u32) -> u32 {
    value.clamp(min, max)
}

fn slider(
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
    range: (u32, u32),
    value: &ControlValue,
    label: &str,
) -> Result<bool, String> {
    let number = control::number(value).ok_or_else(|| format!("{label} expects a number"))?;
    let clamped = clamp(range, number.max(0) as u32);
    Ok(replace(field, clamped))
}

fn replace<T: PartialEq>(field: &mut T, value: T) -> bool {
    if *field == value {
        return false;
    }
    *field = value;
    true
}

impl SceneSettings for AtlanticDuskSettings {
    fn normalized(&self) -> Self {
        Self {
            day_length_seconds: clamp(DAY_LENGTH, self.day_length_seconds),
            cloud_coverage: clamp(CLOUD_COVERAGE, self.cloud_coverage),
            cloud_speed: clamp(CLOUD_SPEED, self.cloud_speed),
            wave_speed: clamp(WAVE_SPEED, self.wave_speed),
            wave_scale: clamp(WAVE_SCALE, self.wave_scale),
            star_density: clamp(STAR_DENSITY, self.star_density),
            contrast: clamp(CONTRAST, self.contrast),
            seed: clamp(SEED, self.seed),
            ..self.clone()
        }
    }

    fn controls(&self) -> Vec<Control> {
        vec![
            Control::choice(
                "time_of_day",
                "Time of day",
                self.time_of_day.index(),
                &TimeOfDay::LABELS,
                "Cycle animates the whole day. The other choices freeze the light at dawn, noon, dusk or night while clouds and waves keep moving.",
            ),
            slider(
                "day_length_seconds",
                "Day length",
                self.day_length_seconds,
                DAY_LENGTH,
                10,
                " s",
                "Seconds for one full day: day, dusk, night and dawn. Only used while Time of day is Cycle.",
            ),
            slider(
                "cloud_coverage",
                "Cloud coverage",
                self.cloud_coverage,
                CLOUD_COVERAGE,
                5,
                "%",
                "How much of the sky is clouded. Clouds are brighter than the sky by day and darker at dusk.",
            ),
            slider(
                "cloud_speed",
                "Cloud speed",
                self.cloud_speed,
                CLOUD_SPEED,
                10,
                "%",
                "How fast the clouds drift sideways. 0 freezes them.",
            ),
            slider(
                "wave_speed",
                "Wave speed",
                self.wave_speed,
                WAVE_SPEED,
                10,
                "%",
                "How fast the wave lines roll. 0 freezes the sea.",
            ),
            slider(
                "wave_scale",
                "Wave spacing",
                self.wave_scale,
                WAVE_SCALE,
                10,
                "%",
                "Distance between wave lines. Small values give a choppier, busier sea.",
            ),
            slider(
                "star_density",
                "Stars",
                self.star_density,
                STAR_DENSITY,
                5,
                "‰",
                "How many stars appear at night, per thousand sky cells. 0 gives a starless night.",
            ),
            slider(
                "contrast",
                "Contrast",
                self.contrast,
                CONTRAST,
                5,
                "%",
                "Overall brightness of the picture. The default keeps it quiet enough for a background.",
            ),
            Control::toggle(
                "twinkle",
                "Star twinkle",
                self.twinkle,
                "Lets some single-dot stars blink out briefly, rarely and subtly.",
            ),
            Control::choice(
                "dither",
                "Dither",
                self.dither.index(),
                &DitherStyle::LABELS,
                "Host dither hands soft gradients to the terminal dither. The Bayer options dither in the scene for a stepped, Playdate-like look.",
            ),
            slider(
                "seed",
                "Seed",
                self.seed,
                SEED,
                1,
                "",
                "Chooses the cloud pattern and star positions.",
            ),
        ]
    }

    fn set_control(&mut self, id: &str, value: ControlValue) -> Result<bool, String> {
        match id {
            "time_of_day" => {
                let index = control::index(&value)
                    .ok_or_else(|| "Time of day expects an option".to_owned())?;
                let choice = TimeOfDay::ALL
                    .get(index)
                    .copied()
                    .ok_or_else(|| format!("Time of day: option {index} does not exist"))?;
                Ok(replace(&mut self.time_of_day, choice))
            }
            "dither" => {
                let index =
                    control::index(&value).ok_or_else(|| "Dither expects an option".to_owned())?;
                let choice = DitherStyle::ALL
                    .get(index)
                    .copied()
                    .ok_or_else(|| format!("Dither: option {index} does not exist"))?;
                Ok(replace(&mut self.dither, choice))
            }
            "twinkle" => {
                let on = control::boolean(&value)
                    .ok_or_else(|| "Star twinkle expects on or off".to_owned())?;
                Ok(replace(&mut self.twinkle, on))
            }
            "day_length_seconds" => set_number(
                &mut self.day_length_seconds,
                DAY_LENGTH,
                &value,
                "Day length",
            ),
            "cloud_coverage" => set_number(
                &mut self.cloud_coverage,
                CLOUD_COVERAGE,
                &value,
                "Cloud coverage",
            ),
            "cloud_speed" => set_number(&mut self.cloud_speed, CLOUD_SPEED, &value, "Cloud speed"),
            "wave_speed" => set_number(&mut self.wave_speed, WAVE_SPEED, &value, "Wave speed"),
            "wave_scale" => set_number(&mut self.wave_scale, WAVE_SCALE, &value, "Wave spacing"),
            "star_density" => set_number(&mut self.star_density, STAR_DENSITY, &value, "Stars"),
            "contrast" => set_number(&mut self.contrast, CONTRAST, &value, "Contrast"),
            "seed" => set_number(&mut self.seed, SEED, &value, "Seed"),
            _ => Ok(false),
        }
    }
}

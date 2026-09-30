//! User-facing settings of the cube clock scene.
//!
//! Serde field names are the YAML keys users see. Every numeric field has a
//! documented range that `normalized` enforces.

use crate::control::{self, Control, ControlValue, SceneSettings};
use serde::{Deserialize, Serialize};

/// Hour display style of the readout.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum HourFormat {
    /// 00..23 with a leading zero.
    #[default]
    #[serde(rename = "24h")]
    TwentyFour,
    /// 1..12, no leading zero, one dot (AM) or two dots (PM) above the last digit.
    #[serde(rename = "12h")]
    Twelve,
}

/// Where the digital readout is drawn.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClockPosition {
    /// Under the cube.
    #[default]
    Below,
    /// Dim digits in front of the cube.
    Inside,
    /// Cube and minute ring only.
    Off,
}

const HOUR_FORMATS: [HourFormat; 2] = [HourFormat::TwentyFour, HourFormat::Twelve];
const CLOCK_POSITIONS: [ClockPosition; 3] = [
    ClockPosition::Below,
    ClockPosition::Inside,
    ClockPosition::Off,
];

const CUBE_SIZE: (i32, i32) = (20, 60);
const ROTATION_RATE: (i32, i32) = (0, 300);
const FACE_SHADING: (i32, i32) = (0, 100);
const BRIGHTNESS: (i32, i32) = (30, 150);
const UTC_OFFSET: (i32, i32) = (-12, 14);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CubeClockSettings {
    pub hour_format: HourFormat,
    /// Append `:SS` to the readout. Default off.
    pub show_seconds: bool,
    pub clock_position: ClockPosition,
    /// Cube edge size as a percentage of the smaller pane side, 20..=60. Default 42.
    pub cube_size: i32,
    /// Rotation speed in percent, 0..=300. 0 freezes the turning. Default 100.
    pub rotation_rate: i32,
    /// Small eased extra turn on every second. Default on.
    pub second_tick: bool,
    /// Ring of 60 dots behind the cube with a sweeping second marker. Default on.
    pub minute_ring: bool,
    /// Density of the dither on the faces turned towards you, 0..=100. Default 50.
    pub face_shading: i32,
    /// Overall intensity in percent, 30..=150. Default 100.
    pub brightness: i32,
    /// A few twinkling background dots. Default on.
    pub dust: bool,
    /// Derive the time zone from the shared location (longitude / 15 degrees).
    /// Default on.
    pub time_zone_from_location: bool,
    /// Hours from UTC used when `time_zone_from_location` is off, -12..=14. Default 0.
    pub utc_offset_hours: i32,
}

impl Default for CubeClockSettings {
    fn default() -> Self {
        Self {
            hour_format: HourFormat::TwentyFour,
            show_seconds: false,
            clock_position: ClockPosition::Below,
            cube_size: 42,
            rotation_rate: 100,
            second_tick: true,
            minute_ring: true,
            face_shading: 50,
            brightness: 100,
            dust: true,
            time_zone_from_location: true,
            utc_offset_hours: 0,
        }
    }
}

fn clamp(range: (i32, i32), value: i32) -> i32 {
    value.clamp(range.0, range.1)
}

fn number_field(
    field: &mut i32,
    range: (i32, i32),
    value: &ControlValue,
    label: &str,
) -> Result<bool, String> {
    let number = control::number(value).ok_or_else(|| format!("{label} expects a number"))?;
    Ok(replace(field, clamp(range, number)))
}

fn toggle_field(field: &mut bool, value: &ControlValue, label: &str) -> Result<bool, String> {
    let on = control::boolean(value).ok_or_else(|| format!("{label} expects On or Off"))?;
    Ok(replace(field, on))
}

fn choice_field<T: Copy + PartialEq>(
    field: &mut T,
    all: &[T],
    value: &ControlValue,
    label: &str,
) -> Result<bool, String> {
    let index = control::index(value).ok_or_else(|| format!("{label} expects an option"))?;
    let choice = all
        .get(index)
        .copied()
        .ok_or_else(|| format!("{label}: option {index} does not exist"))?;
    Ok(replace(field, choice))
}

fn replace<T: PartialEq>(field: &mut T, value: T) -> bool {
    if *field == value {
        return false;
    }
    *field = value;
    true
}

impl SceneSettings for CubeClockSettings {
    fn normalized(&self) -> Self {
        Self {
            cube_size: clamp(CUBE_SIZE, self.cube_size),
            rotation_rate: clamp(ROTATION_RATE, self.rotation_rate),
            face_shading: clamp(FACE_SHADING, self.face_shading),
            brightness: clamp(BRIGHTNESS, self.brightness),
            utc_offset_hours: clamp(UTC_OFFSET, self.utc_offset_hours),
            ..self.clone()
        }
    }

    fn controls(&self) -> Vec<Control> {
        vec![
            Control::choice(
                "hour_format",
                "Hour format",
                match self.hour_format {
                    HourFormat::TwentyFour => 0,
                    HourFormat::Twelve => 1,
                },
                &["24 hours", "12 hours"],
                "24 hours shows 07:05. 12 hours drops the leading zero and marks PM with two small dots above the last digit (AM has one).",
            ),
            Control::toggle(
                "show_seconds",
                "Show seconds",
                self.show_seconds,
                "Append the seconds to the digital readout. The minute ring shows them either way.",
            ),
            Control::choice(
                "clock_position",
                "Readout",
                match self.clock_position {
                    ClockPosition::Below => 0,
                    ClockPosition::Inside => 1,
                    ClockPosition::Off => 2,
                },
                &["Below cube", "In front", "Hidden"],
                "Where the small digital time is drawn. Hidden leaves only the cube and the minute ring.",
            ),
            Control::slider(
                "cube_size",
                "Cube size",
                self.cube_size,
                (CUBE_SIZE.0, CUBE_SIZE.1, 2),
                "%",
                "Size of the cube relative to the smaller side of the pane. Large cubes can touch the readout.",
            ),
            Control::slider(
                "rotation_rate",
                "Rotation speed",
                self.rotation_rate,
                (ROTATION_RATE.0, ROTATION_RATE.1, 10),
                "%",
                "How fast the cube turns. 100% is one full turn about every 18 seconds; 0% freezes it.",
            ),
            Control::toggle(
                "second_tick",
                "Second tick",
                self.second_tick,
                "Give the cube a small eased nudge at every real second, like a clock movement.",
            ),
            Control::toggle(
                "minute_ring",
                "Minute ring",
                self.minute_ring,
                "A ring of 60 faint dots behind the cube. The current second glows and leaves a short trail.",
            ),
            Control::slider(
                "face_shading",
                "Face shading",
                self.face_shading,
                (FACE_SHADING.0, FACE_SHADING.1, 5),
                "%",
                "Sparse dither on the faces turned towards you. 0% is a pure wireframe.",
            ),
            Control::slider(
                "brightness",
                "Brightness",
                self.brightness,
                (BRIGHTNESS.0, BRIGHTNESS.1, 5),
                "%",
                "Scales every dot of this scene. The defaults are deliberately dim for use behind text.",
            ),
            Control::toggle(
                "dust",
                "Dust",
                self.dust,
                "A few faint twinkling dots in the background.",
            ),
            Control::toggle(
                "time_zone_from_location",
                "Zone from location",
                self.time_zone_from_location,
                "Estimate the time zone from the longitude of your location (15 degrees per hour, no daylight saving). Turn off to set the offset by hand.",
            ),
            Control::slider(
                "utc_offset_hours",
                "UTC offset",
                self.utc_offset_hours,
                (UTC_OFFSET.0, UTC_OFFSET.1, 1),
                " h",
                "Hours added to UTC. Only used when Zone from location is off.",
            ),
        ]
    }

    fn set_control(&mut self, id: &str, value: ControlValue) -> Result<bool, String> {
        match id {
            "hour_format" => {
                choice_field(&mut self.hour_format, &HOUR_FORMATS, &value, "Hour format")
            }
            "show_seconds" => toggle_field(&mut self.show_seconds, &value, "Show seconds"),
            "clock_position" => choice_field(
                &mut self.clock_position,
                &CLOCK_POSITIONS,
                &value,
                "Readout",
            ),
            "cube_size" => number_field(&mut self.cube_size, CUBE_SIZE, &value, "Cube size"),
            "rotation_rate" => number_field(
                &mut self.rotation_rate,
                ROTATION_RATE,
                &value,
                "Rotation speed",
            ),
            "second_tick" => toggle_field(&mut self.second_tick, &value, "Second tick"),
            "minute_ring" => toggle_field(&mut self.minute_ring, &value, "Minute ring"),
            "face_shading" => {
                number_field(&mut self.face_shading, FACE_SHADING, &value, "Face shading")
            }
            "brightness" => number_field(&mut self.brightness, BRIGHTNESS, &value, "Brightness"),
            "dust" => toggle_field(&mut self.dust, &value, "Dust"),
            "time_zone_from_location" => toggle_field(
                &mut self.time_zone_from_location,
                &value,
                "Zone from location",
            ),
            "utc_offset_hours" => {
                number_field(&mut self.utc_offset_hours, UTC_OFFSET, &value, "UTC offset")
            }
            _ => Ok(false),
        }
    }
}

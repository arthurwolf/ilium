//! User-facing settings of the machine screen scene.
//!
//! Serde field names are the YAML keys users see. Every numeric field has a
//! documented range that `normalized` enforces.

use crate::control::{self, Control, ControlValue, SceneSettings};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MachineScreenSettings {
    /// Selects the drift phases, marquee bars and blinking dots, 0..=9999. Default 7.
    pub seed: u32,
    /// Drift speed of the gradient waves in percent, 10..=200. Default 50.
    pub gradient_speed: u32,
    /// Panel size as a percentage of the raster, 30..=90. Default 60.
    pub panel_size: u32,
    /// Dots per dither cell, 1..=4. Default 1.
    pub dither_scale: u32,
    /// Marquee scroll speed in dots per second, 0..=12. Default 4.
    pub marquee_speed: u32,
    /// Brightness of the panel in percent, 5..=40. Default 20.
    pub brightness: u32,
    /// Draw a thin frame around the panel. Default on.
    pub show_frame: bool,
    /// Blink a few sparse dots inside the gradient. Default on.
    pub blinking_dots: bool,
}

impl Default for MachineScreenSettings {
    fn default() -> Self {
        Self {
            seed: 7,
            gradient_speed: 50,
            panel_size: 60,
            dither_scale: 1,
            marquee_speed: 4,
            brightness: 20,
            show_frame: true,
            blinking_dots: true,
        }
    }
}

const SEED: (u32, u32) = (0, 9999);
const GRADIENT_SPEED: (u32, u32) = (10, 200);
const PANEL_SIZE: (u32, u32) = (30, 90);
const DITHER_SCALE: (u32, u32) = (1, 4);
const MARQUEE_SPEED: (u32, u32) = (0, 12);
const BRIGHTNESS: (u32, u32) = (5, 40);

fn clamp((min, max): (u32, u32), value: u32) -> u32 {
    value.clamp(min, max)
}

fn range_row(
    (id, label, unit, help): (&'static str, &'static str, &'static str, &'static str),
    value: u32,
    range: (u32, u32),
    step: i32,
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

/// Store a slider value; `Ok(true)` only when the stored value changed.
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

impl SceneSettings for MachineScreenSettings {
    fn normalized(&self) -> Self {
        Self {
            seed: clamp(SEED, self.seed),
            gradient_speed: clamp(GRADIENT_SPEED, self.gradient_speed),
            panel_size: clamp(PANEL_SIZE, self.panel_size),
            dither_scale: clamp(DITHER_SCALE, self.dither_scale),
            marquee_speed: clamp(MARQUEE_SPEED, self.marquee_speed),
            brightness: clamp(BRIGHTNESS, self.brightness),
            ..self.clone()
        }
    }

    fn controls(&self) -> Vec<Control> {
        vec![
            range_row(
                (
                    "seed",
                    "Seed",
                    "",
                    "Selects the wave phases, the marquee bar pattern and where the dots blink.",
                ),
                self.seed,
                SEED,
                1,
            ),
            range_row(
                (
                    "gradient_speed",
                    "Gradient speed",
                    "%",
                    "How fast the two dithered waves drift across the panel.",
                ),
                self.gradient_speed,
                GRADIENT_SPEED,
                10,
            ),
            range_row(
                (
                    "panel_size",
                    "Panel size",
                    "%",
                    "Width and height of the screen as a share of the whole background.",
                ),
                self.panel_size,
                PANEL_SIZE,
                5,
            ),
            range_row(
                (
                    "dither_scale",
                    "Dither scale",
                    " dots",
                    "Dots per dither cell. Larger cells give a coarser, more retro texture.",
                ),
                self.dither_scale,
                DITHER_SCALE,
                1,
            ),
            range_row(
                (
                    "marquee_speed",
                    "Marquee speed",
                    " dots/s",
                    "Scroll speed of the bar band along the bottom. 0 holds it still.",
                ),
                self.marquee_speed,
                MARQUEE_SPEED,
                1,
            ),
            range_row(
                (
                    "brightness",
                    "Brightness",
                    "%",
                    "Dot intensity of the panel. The host dithers it further, so lower values give a sparser, quieter picture.",
                ),
                self.brightness,
                BRIGHTNESS,
                5,
            ),
            Control::toggle(
                "show_frame",
                "Frame",
                self.show_frame,
                "Draw a thin bezel around the panel.",
            ),
            Control::toggle(
                "blinking_dots",
                "Blinking dots",
                self.blinking_dots,
                "A few sparse dots inside the gradient switch on and off twice a second.",
            ),
        ]
    }

    fn set_control(&mut self, id: &str, value: ControlValue) -> Result<bool, String> {
        match id {
            "seed" => set_number(&mut self.seed, SEED, &value, "Seed"),
            "gradient_speed" => set_number(
                &mut self.gradient_speed,
                GRADIENT_SPEED,
                &value,
                "Gradient speed",
            ),
            "panel_size" => set_number(&mut self.panel_size, PANEL_SIZE, &value, "Panel size"),
            "dither_scale" => {
                set_number(&mut self.dither_scale, DITHER_SCALE, &value, "Dither scale")
            }
            "marquee_speed" => set_number(
                &mut self.marquee_speed,
                MARQUEE_SPEED,
                &value,
                "Marquee speed",
            ),
            "brightness" => set_number(&mut self.brightness, BRIGHTNESS, &value, "Brightness"),
            "show_frame" => set_toggle(&mut self.show_frame, &value, "Frame"),
            "blinking_dots" => set_toggle(&mut self.blinking_dots, &value, "Blinking dots"),
            _ => Ok(false),
        }
    }
}

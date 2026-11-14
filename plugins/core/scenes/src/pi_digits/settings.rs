//! Settings of the `pi-digits` scene. The rows and their ids are the ones the manifest's
//! `settings_schema` declares (a unit test compares the two). The colour rows are the scene's
//! own content (the hue of the digits); the shared look recolours the result.

use super::digits;
use crate::kit::control::{self, Control, ControlValue, SceneSettings};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PiSettings {
    /// 0 is native terminal text, 1 is true-font Braille.
    pub mode: usize,
    pub digits_limit: i32,
    /// Height in Braille dots; native text follows the terminal's font size.
    pub font_size: i32,
    /// Decimal characters per second, divided by ten.
    pub scroll_speed: i32,
    /// Common colour angle of the digits, degrees.
    pub colour_angle: i32,
    /// Strength of the digit colours, percent. Zero hides the text.
    pub ink_level: i32,
    pub digit_colors: bool,
    /// Colour angle of each decimal digit when `digit_colors` is on.
    pub digit_angles: [i32; 10],
}

impl Default for PiSettings {
    fn default() -> Self {
        Self {
            mode: 0,
            digits_limit: 4096,
            font_size: 20,
            scroll_speed: 10,
            colour_angle: 180,
            ink_level: 65,
            digit_colors: false,
            digit_angles: [0, 36, 72, 108, 144, 180, 216, 252, 288, 324],
        }
    }
}

pub const DIGIT_IDS: [&str; 10] = [
    "digit_0_angle",
    "digit_1_angle",
    "digit_2_angle",
    "digit_3_angle",
    "digit_4_angle",
    "digit_5_angle",
    "digit_6_angle",
    "digit_7_angle",
    "digit_8_angle",
    "digit_9_angle",
];

const DIGIT_LABELS: [&str; 10] = [
    "Digit 0 colour angle",
    "Digit 1 colour angle",
    "Digit 2 colour angle",
    "Digit 3 colour angle",
    "Digit 4 colour angle",
    "Digit 5 colour angle",
    "Digit 6 colour angle",
    "Digit 7 colour angle",
    "Digit 8 colour angle",
    "Digit 9 colour angle",
];

impl SceneSettings for PiSettings {
    fn normalized(&self) -> Self {
        let mut next = self.clone();
        next.mode = next.mode.min(1);
        next.digits_limit = next.digits_limit.clamp(1, digits::MAX_DIGITS as i32);
        next.font_size = next.font_size.clamp(8, 48);
        next.scroll_speed = next.scroll_speed.clamp(0, 2000);
        next.colour_angle = next.colour_angle.clamp(0, 360);
        next.ink_level = next.ink_level.clamp(0, 100);
        next.digit_angles
            .iter_mut()
            .for_each(|angle| *angle = (*angle).clamp(0, 360));
        next
    }

    fn controls(&self) -> Vec<Control> {
        let mut rows = vec![
            Control::choice(
                "mode",
                "Pi rendering",
                self.mode,
                &["Native text", "Font Braille"],
                "Native text uses the terminal font. Font Braille rasterizes the bundled Cascadia Code.",
            ),
            Control::slider(
                "font_size",
                "Font size",
                self.font_size,
                (8, 48, 2),
                " dots",
                "Real-font height in Braille raster dots. Native text uses the terminal emulator's configured size.",
            ),
            Control::slider(
                "digits_limit",
                "Exact digits",
                self.digits_limit,
                (1, 20000, 100),
                "",
                "Number of exact decimal digits, including the initial 3. The decimal separator is added for display; the sequence repeats after this prefix.",
            ),
            Control::slider(
                "scroll_speed",
                "Scroll speed",
                self.scroll_speed,
                (0, 2000, 10),
                " ×0.1 chars/s",
                "Move through the exact digit sequence. Zero holds the prefix; global Speed also applies.",
            ),
            Control::slider(
                "colour_angle",
                "Pi colour angle",
                self.colour_angle,
                (0, 360, 5),
                "°",
                "Common colour of decimal digits when digit colours are disabled.",
            ),
            Control::slider(
                "ink_level",
                "Pi ink level",
                self.ink_level,
                (0, 100, 5),
                "%",
                "Zero hides both native and Braille text. Colour strength and dot coverage are independent.",
            ),
            Control::toggle(
                "digit_colors",
                "Colors by digit",
                self.digit_colors,
                "Assign a separate colour angle to each decimal digit. The decimal separator keeps the common colour.",
            ),
        ];
        for (digit, angle) in self.digit_angles.iter().enumerate() {
            rows.push(Control::slider(
                DIGIT_IDS[digit],
                DIGIT_LABELS[digit],
                *angle,
                (0, 360, 5),
                "°",
                "Colour angle assigned to this exact decimal digit; used when Colors by digit is on.",
            ));
        }
        rows
    }

    fn set_control(&mut self, id: &str, value: ControlValue) -> Result<bool, String> {
        let mut next = self.clone();
        match id {
            "mode" => {
                let Some(value) = control::index(&value) else {
                    return Ok(false);
                };
                next.mode = value;
            }
            "digit_colors" => {
                let Some(value) = control::boolean(&value) else {
                    return Ok(false);
                };
                next.digit_colors = value;
            }
            _ => {
                let Some(value) = control::number(&value) else {
                    return Ok(false);
                };
                match id {
                    "digits_limit" => next.digits_limit = value,
                    "font_size" => next.font_size = value,
                    "scroll_speed" => next.scroll_speed = value,
                    "colour_angle" => next.colour_angle = value,
                    "ink_level" => next.ink_level = value,
                    _ => {
                        let Some(digit) = DIGIT_IDS.iter().position(|candidate| *candidate == id)
                        else {
                            return Ok(false);
                        };
                        next.digit_angles[digit] = value;
                    }
                }
            }
        }
        next = next.normalized();
        let changed = next != *self;
        *self = next;
        Ok(changed)
    }
}

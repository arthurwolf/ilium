//! User-facing settings of the dithered water scene.
//!
//! Serde field names are the YAML keys users see. Every numeric field has a
//! documented range that `normalized` enforces.

use crate::control::{self, Control, ControlValue, SceneSettings};
use serde::{Deserialize, Serialize};

/// Size of the ordered-dither threshold matrix.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DitherSize {
    /// 2x2: very chunky, only five grey levels.
    Bayer2,
    /// 4x4: the classic pixel-art look.
    #[default]
    Bayer4,
    /// 8x8: finer gradients, softer bands.
    Bayer8,
}

impl DitherSize {
    pub const ALL: [Self; 3] = [Self::Bayer2, Self::Bayer4, Self::Bayer8];
    pub const LABELS: [&'static str; 3] = ["2x2 chunky", "4x4 classic", "8x8 fine"];

    pub fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|candidate| *candidate == self)
            .unwrap_or(0)
    }

    /// Matrix side length in dots.
    pub fn side(self) -> usize {
        match self {
            Self::Bayer2 => 2,
            Self::Bayer4 => 4,
            Self::Bayer8 => 8,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DitherWaterSettings {
    /// Spatial frequency of the waves in percent, 50..=300. Default 100.
    pub wave_scale: u32,
    /// Speed of the wave phases in percent, 20..=300. Default 100.
    pub flow_speed: u32,
    /// Caustic exponent, 1..=6. Higher gives thinner bright lines. Default 3.
    pub sharpness: u32,
    /// Bias added before thresholding in percent, -30..=30. Default 0.
    pub density: i32,
    /// Brightness in percent, 15..=80. Default 45: below it lit dots thin out, above it the tint brightens.
    pub brightness: u32,
    /// Horizon stretch and far-water fade in percent, 0..=100. Default 50.
    pub perspective: u32,
    pub dither: DitherSize,
    /// Expanding elliptical rings. Default on.
    pub ripples: bool,
    /// Quantise time to 8 steps per second for a pixel-art stutter.
    pub stepped: bool,
    /// Blue to cyan cell colours instead of the global palette.
    pub tint: bool,
    /// Phase and ripple placement seed, 0..=999. Default 7.
    pub seed: u32,
}

impl Default for DitherWaterSettings {
    fn default() -> Self {
        Self {
            wave_scale: 100,
            flow_speed: 100,
            sharpness: 3,
            density: 0,
            brightness: 45,
            perspective: 50,
            dither: DitherSize::Bayer4,
            ripples: true,
            stepped: false,
            tint: true,
            seed: 7,
        }
    }
}

const WAVE_SCALE: (i32, i32) = (50, 300);
const FLOW_SPEED: (i32, i32) = (20, 300);
const SHARPNESS: (i32, i32) = (1, 6);
const DENSITY: (i32, i32) = (-30, 30);
const BRIGHTNESS: (i32, i32) = (15, 80);
const PERSPECTIVE: (i32, i32) = (0, 100);
const SEED: (i32, i32) = (0, 999);

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
    value: i32,
    range: (i32, i32),
    step: i32,
    unit: &'static str,
    help: &'static str,
) -> Control {
    Control::slider(id, label, value, (range.0, range.1, step), unit, help)
}

impl SceneSettings for DitherWaterSettings {
    fn normalized(&self) -> Self {
        Self {
            wave_scale: clamp_unsigned(WAVE_SCALE, self.wave_scale),
            flow_speed: clamp_unsigned(FLOW_SPEED, self.flow_speed),
            sharpness: clamp_unsigned(SHARPNESS, self.sharpness),
            density: self.density.clamp(DENSITY.0, DENSITY.1),
            brightness: clamp_unsigned(BRIGHTNESS, self.brightness),
            perspective: clamp_unsigned(PERSPECTIVE, self.perspective),
            seed: clamp_unsigned(SEED, self.seed),
            ..self.clone()
        }
    }

    fn controls(&self) -> Vec<Control> {
        vec![
            slider(
                "wave_scale",
                "Wave size",
                self.wave_scale as i32,
                WAVE_SCALE,
                10,
                "%",
                "Spatial frequency of the waves. Higher values give smaller, busier ripples.",
            ),
            slider(
                "flow_speed",
                "Flow speed",
                self.flow_speed as i32,
                FLOW_SPEED,
                10,
                "%",
                "How fast the light bands drift and shimmer, on top of the global Speed setting.",
            ),
            slider(
                "sharpness",
                "Sharpness",
                self.sharpness as i32,
                SHARPNESS,
                1,
                "",
                "Higher values narrow the bright caustic lines and leave more dark water between them.",
            ),
            slider(
                "density",
                "Density",
                self.density,
                DENSITY,
                2,
                "%",
                "Shifts the black and white balance. Positive values switch more dots on.",
            ),
            slider(
                "brightness",
                "Brightness",
                self.brightness as i32,
                BRIGHTNESS,
                5,
                "%",
                "Below the default, lit dots thin out. Above it, the blue tint gets brighter. The default keeps the water a quiet background.",
            ),
            slider(
                "perspective",
                "Perspective",
                self.perspective as i32,
                PERSPECTIVE,
                5,
                "%",
                "Squeezes the waves toward a horizon and fades the far water. 0% is a flat top-down surface.",
            ),
            Control::choice(
                "dither",
                "Dither",
                self.dither.index(),
                &DitherSize::LABELS,
                "Size of the fixed threshold matrix. Coarser matrices look chunkier.",
            ),
            Control::toggle(
                "ripples",
                "Ripples",
                self.ripples,
                "Draws slowly expanding elliptical rings on top of the waves.",
            ),
            Control::toggle(
                "stepped",
                "Stepped motion",
                self.stepped,
                "Advances the water in eight steps per second for a hand-animated pixel-art stutter.",
            ),
            Control::toggle(
                "tint",
                "Blue tint",
                self.tint,
                "Colours lit dots from deep blue to pale cyan. Off uses your global palette.",
            ),
            slider(
                "seed",
                "Seed number",
                self.seed as i32,
                SEED,
                1,
                "",
                "Selects the wave phases and where ripples appear.",
            ),
        ]
    }

    fn set_control(&mut self, id: &str, value: ControlValue) -> Result<bool, String> {
        match id {
            "wave_scale" => set_unsigned(&mut self.wave_scale, WAVE_SCALE, &value, "Wave size"),
            "flow_speed" => set_unsigned(&mut self.flow_speed, FLOW_SPEED, &value, "Flow speed"),
            "sharpness" => set_unsigned(&mut self.sharpness, SHARPNESS, &value, "Sharpness"),
            "density" => {
                let number = number_of(&value, "Density")?.clamp(DENSITY.0, DENSITY.1);
                Ok(replace(&mut self.density, number))
            }
            "brightness" => set_unsigned(&mut self.brightness, BRIGHTNESS, &value, "Brightness"),
            "perspective" => {
                set_unsigned(&mut self.perspective, PERSPECTIVE, &value, "Perspective")
            }
            "dither" => {
                let index =
                    control::index(&value).ok_or_else(|| "Dither expects an option".to_owned())?;
                let size = DitherSize::ALL
                    .get(index)
                    .copied()
                    .ok_or_else(|| format!("Dither: option {index} does not exist"))?;
                Ok(replace(&mut self.dither, size))
            }
            "ripples" => set_toggle(&mut self.ripples, &value, "Ripples"),
            "stepped" => set_toggle(&mut self.stepped, &value, "Stepped motion"),
            "tint" => set_toggle(&mut self.tint, &value, "Blue tint"),
            "seed" => set_unsigned(&mut self.seed, SEED, &value, "Seed number"),
            _ => Ok(false),
        }
    }
}

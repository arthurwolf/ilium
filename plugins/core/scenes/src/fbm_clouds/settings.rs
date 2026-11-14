//! User-facing settings of the fBm clouds scene. Field names are the stable
//! snake_case keys users see; `normalized` enforces every range.

use crate::control::{self, Control, ControlValue, SceneSettings};
use serde::{Deserialize, Serialize};

trait ChoiceEnum: Copy + PartialEq + 'static {
    const ALL: &'static [Self];
    const LABELS: &'static [&'static str];

    fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|candidate| *candidate == self)
            .unwrap_or(0)
    }
}

/// Threshold pattern that turns cloud density into dots.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DitherPattern {
    /// Regular 8x8 ordered crosshatch.
    #[default]
    Bayer,
    /// Interleaved gradient noise: an irregular but even grain.
    Gradient,
    /// Independent random thresholds: rough stipple.
    White,
}

impl ChoiceEnum for DitherPattern {
    const ALL: &'static [Self] = &[Self::Bayer, Self::Gradient, Self::White];
    const LABELS: &'static [&'static str] = &["Bayer 8x8", "Gradient noise", "White noise"];
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FbmCloudsSettings {
    /// Cloud size multiplier in percent (larger = smaller, busier clouds), 50..=300. Default 100.
    pub scale: u32,
    /// Speed of the internal folding motion in percent, 0..=400. Default 100.
    pub drift: u32,
    /// Sideways pan in 0.0025 units per second, -40..=40 (negative pans right). Default 5.
    pub pan: i32,
    /// How strongly clouds bend themselves in percent, 0..=150. Default 100.
    pub warp: u32,
    /// fBm octaves, 2..=4. Default 4.
    pub octaves: u32,
    /// Steepness of the tone curve before dithering in percent, 50..=300. Default 120.
    pub tone_steepness: u32,
    /// Pixel block size in dots, 1..=4. Default 2.
    pub block: u32,
    /// The threshold pattern that turns cloud density into dots.
    pub pattern: DitherPattern,
    /// Intensity of lit dots in percent, 5..=100. Default 35.
    pub intensity: u32,
    /// Dark clouds on a lit field. Default off.
    pub invert: bool,
    /// Cloud layout, 0..=999. Default 0.
    pub seed: u32,
}

impl Default for FbmCloudsSettings {
    fn default() -> Self {
        Self {
            scale: 100,
            drift: 100,
            pan: 5,
            warp: 100,
            octaves: 4,
            tone_steepness: 120,
            block: 2,
            pattern: DitherPattern::Bayer,
            intensity: 35,
            invert: false,
            seed: 0,
        }
    }
}

const SCALE: (u32, u32) = (50, 300);
const DRIFT: (u32, u32) = (0, 400);
const PAN: (i32, i32) = (-40, 40);
const WARP: (u32, u32) = (0, 150);
const OCTAVES: (u32, u32) = (2, 4);
const TONE_STEEPNESS: (u32, u32) = (50, 300);
const BLOCK: (u32, u32) = (1, 4);
const INTENSITY: (u32, u32) = (5, 100);
const SEED: (u32, u32) = (0, 999);

fn percent_row(
    id: &'static str,
    label: &'static str,
    value: u32,
    range: (u32, u32),
    step: i32,
    help: &'static str,
) -> Control {
    Control::slider(
        id,
        label,
        value as i32,
        (range.0 as i32, range.1 as i32, step),
        "%",
        help,
    )
}

fn choice_row<T: ChoiceEnum>(
    id: &'static str,
    label: &'static str,
    value: T,
    help: &'static str,
) -> Control {
    Control::choice(id, label, value.index(), T::LABELS, help)
}

fn set_number(
    field: &mut u32,
    range: (u32, u32),
    value: &ControlValue,
    label: &str,
) -> Result<bool, String> {
    let number = control::number(value).ok_or_else(|| format!("{label} expects a number"))?;
    let clamped = (number.max(0) as u32).clamp(range.0, range.1);
    let changed = *field != clamped;
    *field = clamped;
    Ok(changed)
}

fn set_choice<T: ChoiceEnum>(
    field: &mut T,
    value: &ControlValue,
    label: &str,
) -> Result<bool, String> {
    let index = control::index(value).ok_or_else(|| format!("{label} expects an option"))?;
    let choice = T::ALL
        .get(index)
        .copied()
        .ok_or_else(|| format!("{label}: option {index} does not exist"))?;
    let changed = *field != choice;
    *field = choice;
    Ok(changed)
}

impl SceneSettings for FbmCloudsSettings {
    fn normalized(&self) -> Self {
        Self {
            scale: self.scale.clamp(SCALE.0, SCALE.1),
            drift: self.drift.clamp(DRIFT.0, DRIFT.1),
            pan: self.pan.clamp(PAN.0, PAN.1),
            warp: self.warp.clamp(WARP.0, WARP.1),
            octaves: self.octaves.clamp(OCTAVES.0, OCTAVES.1),
            tone_steepness: self
                .tone_steepness
                .clamp(TONE_STEEPNESS.0, TONE_STEEPNESS.1),
            block: self.block.clamp(BLOCK.0, BLOCK.1),
            intensity: self.intensity.clamp(INTENSITY.0, INTENSITY.1),
            seed: self.seed.clamp(SEED.0, SEED.1),
            ..self.clone()
        }
    }

    fn controls(&self) -> Vec<Control> {
        vec![
            percent_row(
                "scale",
                "Cloud scale",
                self.scale,
                SCALE,
                10,
                "Larger values pack in smaller, busier clouds; smaller values give a few huge banks.",
            ),
            percent_row(
                "drift",
                "Fold speed",
                self.drift,
                DRIFT,
                10,
                "How fast the cloud folds slowly morph into each other. 0% freezes the folding.",
            ),
            Control::slider(
                "pan",
                "Pan",
                self.pan,
                (PAN.0, PAN.1, 1),
                "",
                "Sideways drift of the whole sky. Negative values move it the other way, 0 holds it still.",
            ),
            percent_row(
                "warp",
                "Warp",
                self.warp,
                WARP,
                10,
                "How much clouds displace themselves. 0% gives plain fractal noise, 100% the swirling look.",
            ),
            Control::slider(
                "octaves",
                "Detail",
                self.octaves as i32,
                (OCTAVES.0 as i32, OCTAVES.1 as i32, 1),
                "",
                "Noise octaves. Fewer octaves are smoother and cheaper, more add fine wisps.",
            ),
            percent_row(
                "tone_steepness",
                "Tone steepness",
                self.tone_steepness,
                TONE_STEEPNESS,
                10,
                "Steepness of the dither response: lower is a softer, grainier sky.",
            ),
            Control::slider(
                "block",
                "Pixel size",
                self.block as i32,
                (BLOCK.0 as i32, BLOCK.1 as i32, 1),
                " dots",
                "Size of the chunky blocks the clouds are sampled at. Larger blocks are coarser and cheaper.",
            ),
            choice_row(
                "pattern",
                "Pattern",
                self.pattern,
                "Pattern of the one-bit threshold: regular crosshatch, even grain or rough stipple.",
            ),
            percent_row(
                "intensity",
                "Intensity",
                self.intensity,
                INTENSITY,
                5,
                "Intensity of lit dots. Keep it low for a quiet background.",
            ),
            control::Control::toggle(
                "invert",
                "Invert",
                self.invert,
                "Swap lit and dark: dark clouds on a dotted field.",
            ),
            Control::slider(
                "seed",
                "Seed",
                self.seed as i32,
                (SEED.0 as i32, SEED.1 as i32, 1),
                "",
                "Selects the cloud layout.",
            ),
        ]
    }

    fn set_control(&mut self, id: &str, value: ControlValue) -> Result<bool, String> {
        match id {
            "scale" => set_number(&mut self.scale, SCALE, &value, "Cloud scale"),
            "drift" => set_number(&mut self.drift, DRIFT, &value, "Fold speed"),
            "pan" => {
                let number = control::number(&value).ok_or("Pan expects a number")?;
                let clamped = number.clamp(PAN.0, PAN.1);
                let changed = self.pan != clamped;
                self.pan = clamped;
                Ok(changed)
            }
            "warp" => set_number(&mut self.warp, WARP, &value, "Warp"),
            "octaves" => set_number(&mut self.octaves, OCTAVES, &value, "Detail"),
            "tone_steepness" => set_number(
                &mut self.tone_steepness,
                TONE_STEEPNESS,
                &value,
                "Tone steepness",
            ),
            "block" => set_number(&mut self.block, BLOCK, &value, "Pixel size"),
            "pattern" => set_choice(&mut self.pattern, &value, "Pattern"),
            "intensity" => set_number(&mut self.intensity, INTENSITY, &value, "Intensity"),
            "invert" => {
                let on = control::boolean(&value).ok_or("Invert expects on or off")?;
                let changed = self.invert != on;
                self.invert = on;
                Ok(changed)
            }
            "seed" => set_number(&mut self.seed, SEED, &value, "Seed"),
            _ => Ok(false),
        }
    }
}

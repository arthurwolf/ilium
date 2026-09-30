//! User-facing settings of the fBm clouds scene. Field names are the stable
//! snake_case keys users see; `normalized` enforces every range.

use crate::control::{self, Control, ControlValue, SceneSettings};
use crate::gpu;
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

/// Which engine computes the clouds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RenderBackend {
    #[default]
    Software,
    Gpu,
}

impl ChoiceEnum for RenderBackend {
    const ALL: &'static [Self] = &[Self::Software, Self::Gpu];
    const LABELS: &'static [&'static str] = &["Software (slow-mo)", "GPU"];
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
    /// Dither contrast in percent, 50..=300. Default 120.
    pub contrast: u32,
    /// Pixel block size in dots, 1..=4. Default 2.
    pub block: u32,
    pub dither: DitherPattern,
    /// Intensity of lit dots in percent, 5..=100. Default 35.
    pub brightness: u32,
    /// Dark clouds on a lit field. Default off.
    pub invert: bool,
    /// Cloud layout, 0..=999. Default 0.
    pub seed: u32,
    pub render_backend: RenderBackend,
}

impl Default for FbmCloudsSettings {
    fn default() -> Self {
        Self {
            scale: 100,
            drift: 100,
            pan: 5,
            warp: 100,
            octaves: 4,
            contrast: 120,
            block: 2,
            dither: DitherPattern::Bayer,
            brightness: 35,
            invert: false,
            seed: 0,
            render_backend: RenderBackend::Software,
        }
    }
}

const SCALE: (u32, u32) = (50, 300);
const DRIFT: (u32, u32) = (0, 400);
const PAN: (i32, i32) = (-40, 40);
const WARP: (u32, u32) = (0, 150);
const OCTAVES: (u32, u32) = (2, 4);
const CONTRAST: (u32, u32) = (50, 300);
const BLOCK: (u32, u32) = (1, 4);
const BRIGHTNESS: (u32, u32) = (5, 100);
const SEED: (u32, u32) = (0, 999);

const OCTAVE_LABELS: &[&str] = &["2", "3", "4"];
const BLOCK_LABELS: &[&str] = &["1 dot", "2 dots", "3 dots", "4 dots"];

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

/// Stores an option index as `first + index`.
fn set_ranged_choice(
    field: &mut u32,
    range: (u32, u32),
    value: &ControlValue,
    label: &str,
) -> Result<bool, String> {
    let index = control::index(value).ok_or_else(|| format!("{label} expects an option"))?;
    let chosen = range.0 + index as u32;
    if chosen > range.1 {
        return Err(format!("{label}: option {index} does not exist"));
    }
    let changed = *field != chosen;
    *field = chosen;
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
            contrast: self.contrast.clamp(CONTRAST.0, CONTRAST.1),
            block: self.block.clamp(BLOCK.0, BLOCK.1),
            brightness: self.brightness.clamp(BRIGHTNESS.0, BRIGHTNESS.1),
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
            Control::choice(
                "octaves",
                "Detail",
                self.octaves.saturating_sub(OCTAVES.0) as usize,
                OCTAVE_LABELS,
                "Noise octaves. Fewer octaves are smoother and cheaper, more add fine wisps.",
            ),
            percent_row(
                "contrast",
                "Contrast",
                self.contrast,
                CONTRAST,
                10,
                "Steepness of the dither response: lower is a softer, grainier sky.",
            ),
            Control::choice(
                "block",
                "Pixel size",
                self.block.saturating_sub(BLOCK.0) as usize,
                BLOCK_LABELS,
                "Size of the chunky blocks the clouds are sampled at. Larger blocks are coarser and cheaper.",
            ),
            choice_row(
                "dither",
                "Dither",
                self.dither,
                "Pattern of the one-bit threshold: regular crosshatch, even grain or rough stipple.",
            ),
            percent_row(
                "brightness",
                "Brightness",
                self.brightness,
                BRIGHTNESS,
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
            gpu::decorate_backend_row(choice_row(
                "render_backend",
                "Renderer",
                self.render_backend,
                "Software runs on the CPU in gentle slow motion. GPU uses the graphics card when one is usable; otherwise the option is greyed out.",
            )),
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
            "octaves" => set_ranged_choice(&mut self.octaves, OCTAVES, &value, "Detail"),
            "contrast" => set_number(&mut self.contrast, CONTRAST, &value, "Contrast"),
            "block" => set_ranged_choice(&mut self.block, BLOCK, &value, "Pixel size"),
            "dither" => set_choice(&mut self.dither, &value, "Dither"),
            "brightness" => set_number(&mut self.brightness, BRIGHTNESS, &value, "Brightness"),
            "invert" => {
                let on = control::boolean(&value).ok_or("Invert expects on or off")?;
                let changed = self.invert != on;
                self.invert = on;
                Ok(changed)
            }
            "seed" => set_number(&mut self.seed, SEED, &value, "Seed"),
            "render_backend" => {
                gpu::reject_unavailable_choice(&value)?;
                set_choice(&mut self.render_backend, &value, "Renderer")
            }
            _ => Ok(false),
        }
    }
}

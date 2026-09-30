//! User-facing settings of the dithered waves scene.
//!
//! Serde field names are the YAML keys users see. Sliders are integers, so
//! fractional quantities are stored as percentages (`wave_frequency: 300`
//! means 3.0 wave crests per screen height).

use crate::control::{self, Control, ControlValue, SceneSettings};
use crate::gpu;
use serde::{Deserialize, Serialize};

/// A closed list of options shown as a Choice row.
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

/// Threshold pattern used to turn the smooth wave field into dots.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DitherMatrix {
    /// Coarsest regular lattice, 4 gray steps.
    Bayer2,
    /// Classic crosshatch lattice, 16 steps.
    #[default]
    Bayer4,
    /// Finest regular lattice, 64 steps.
    Bayer8,
    /// A fixed irregular stipple tile (still identical on every frame).
    Noise,
}

impl ChoiceEnum for DitherMatrix {
    const ALL: &'static [Self] = &[Self::Bayer2, Self::Bayer4, Self::Bayer8, Self::Noise];
    const LABELS: &'static [&'static str] = &["Bayer 2x2", "Bayer 4x4", "Bayer 8x8", "Noise"];
}

/// Which engine draws the waves.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RenderBackend {
    /// The CPU renderer, running gently in slow motion.
    #[default]
    Software,
    /// The GPU path; software is used whenever it is unusable.
    Gpu,
}

impl ChoiceEnum for RenderBackend {
    const ALL: &'static [Self] = &[Self::Software, Self::Gpu];
    const LABELS: &'static [&'static str] = &["Software (slow-mo)", "GPU"];
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DitheredWavesSettings {
    /// Wave crests per screen height in percent, 100..=800. Default 300.
    pub wave_frequency: u32,
    /// How strongly noise warps the bands, percent, 0..=100. Default 30.
    pub wave_amplitude: u32,
    /// Drift speed in percent of the base rate, 0..=200. Default 50.
    pub wave_speed: u32,
    pub dither_matrix: DitherMatrix,
    /// Gray levels of the dither, 2..=6. 2 is pure black and white dots.
    pub levels: u32,
    /// Edge length of one dither pixel in Braille dots, 1..=4. Default 1.
    pub pixel_size: u32,
    /// Peak dot intensity in percent, 10..=100. Default 35.
    pub brightness: u32,
    /// Band sharpness in percent, 50..=300. Default 150.
    pub contrast: u32,
    /// Threshold shift in percent, -30..=30. Positive adds dots. Default 0.
    pub bias: i32,
    /// Adds a faint ripple around a slowly wandering point. Default off.
    pub ripple: bool,
    /// Selects one of many different wave layouts, 0..=9999. Default 1.
    pub seed: u32,
    pub render_backend: RenderBackend,
}

impl Default for DitheredWavesSettings {
    fn default() -> Self {
        Self {
            wave_frequency: 300,
            wave_amplitude: 30,
            wave_speed: 50,
            dither_matrix: DitherMatrix::Bayer4,
            levels: 2,
            pixel_size: 1,
            brightness: 35,
            contrast: 150,
            bias: 0,
            ripple: false,
            seed: 1,
            render_backend: RenderBackend::Software,
        }
    }
}

const WAVE_FREQUENCY: (u32, u32) = (100, 800);
const WAVE_AMPLITUDE: (u32, u32) = (0, 100);
const WAVE_SPEED: (u32, u32) = (0, 200);
const LEVELS: (u32, u32) = (2, 6);
const PIXEL_SIZE: (u32, u32) = (1, 4);
const BRIGHTNESS: (u32, u32) = (10, 100);
const CONTRAST: (u32, u32) = (50, 300);
const BIAS: (i32, i32) = (-30, 30);
const SEED: (u32, u32) = (0, 9999);

fn clamp((min, max): (u32, u32), value: u32) -> u32 {
    value.clamp(min, max)
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

fn choice_row<T: ChoiceEnum>(
    id: &'static str,
    label: &'static str,
    value: T,
    help: &'static str,
) -> Control {
    Control::choice(id, label, value.index(), T::LABELS, help)
}

/// Store a slider value; `Ok(true)` only when the stored value changed.
fn set_number(
    field: &mut u32,
    range: (u32, u32),
    value: &ControlValue,
    label: &str,
) -> Result<bool, String> {
    let number = control::number(value).ok_or_else(|| format!("{label} expects a number"))?;
    Ok(replace(field, clamp(range, number.max(0) as u32)))
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
    Ok(replace(field, choice))
}

fn replace<T: PartialEq>(field: &mut T, value: T) -> bool {
    if *field == value {
        return false;
    }
    *field = value;
    true
}

impl SceneSettings for DitheredWavesSettings {
    fn normalized(&self) -> Self {
        Self {
            wave_frequency: clamp(WAVE_FREQUENCY, self.wave_frequency),
            wave_amplitude: clamp(WAVE_AMPLITUDE, self.wave_amplitude),
            wave_speed: clamp(WAVE_SPEED, self.wave_speed),
            levels: clamp(LEVELS, self.levels),
            pixel_size: clamp(PIXEL_SIZE, self.pixel_size),
            brightness: clamp(BRIGHTNESS, self.brightness),
            contrast: clamp(CONTRAST, self.contrast),
            bias: self.bias.clamp(BIAS.0, BIAS.1),
            seed: clamp(SEED, self.seed),
            ..self.clone()
        }
    }

    fn controls(&self) -> Vec<Control> {
        vec![
            range_row(
                "wave_frequency",
                "Wave frequency",
                self.wave_frequency,
                WAVE_FREQUENCY,
                25,
                "%",
                "How many wave crests fit on screen. Higher values give narrower, busier bands.",
            ),
            range_row(
                "wave_amplitude",
                "Wave amplitude",
                self.wave_amplitude,
                WAVE_AMPLITUDE,
                5,
                "%",
                "How strongly noise bends the bands. 0% gives smooth parallel swells, 100% churning silk.",
            ),
            range_row(
                "wave_speed",
                "Wave speed",
                self.wave_speed,
                WAVE_SPEED,
                5,
                "%",
                "How fast the waves drift, on top of the global Speed setting. 0% freezes them.",
            ),
            choice_row(
                "dither_matrix",
                "Dither pattern",
                self.dither_matrix,
                "The fixed dot lattice the waves are cut into. Bayer 8x8 is finest, Noise looks like grain.",
            ),
            range_row(
                "levels",
                "Gray levels",
                self.levels,
                LEVELS,
                1,
                "",
                "Number of intensity steps. 2 gives pure on/off dots; more steps soften the bands.",
            ),
            range_row(
                "pixel_size",
                "Dither pixel",
                self.pixel_size,
                PIXEL_SIZE,
                1,
                " dots",
                "Edge length of one dither pixel in Braille dots. Larger values look chunkier.",
            ),
            range_row(
                "brightness",
                "Brightness",
                self.brightness,
                BRIGHTNESS,
                5,
                "%",
                "Peak dot intensity. The default keeps the background quiet behind text.",
            ),
            range_row(
                "contrast",
                "Contrast",
                self.contrast,
                CONTRAST,
                10,
                "%",
                "Sharpness of the bands: low values fill the screen with soft dots, high values isolate crests.",
            ),
            Control::slider(
                "bias",
                "Dot bias",
                self.bias,
                (BIAS.0, BIAS.1, 2),
                "%",
                "Shifts the dither threshold. Positive values add dots everywhere, negative values thin them out.",
            ),
            Control::toggle(
                "ripple",
                "Ripple",
                self.ripple,
                "A faint ring pattern that spreads from a slowly wandering point.",
            ),
            range_row(
                "seed",
                "Layout seed",
                self.seed,
                SEED,
                1,
                "",
                "Picks a different wave layout. The same seed always draws the same waves.",
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
            "wave_frequency" => set_number(
                &mut self.wave_frequency,
                WAVE_FREQUENCY,
                &value,
                "Wave frequency",
            ),
            "wave_amplitude" => set_number(
                &mut self.wave_amplitude,
                WAVE_AMPLITUDE,
                &value,
                "Wave amplitude",
            ),
            "wave_speed" => set_number(&mut self.wave_speed, WAVE_SPEED, &value, "Wave speed"),
            "dither_matrix" => set_choice(&mut self.dither_matrix, &value, "Dither pattern"),
            "levels" => set_number(&mut self.levels, LEVELS, &value, "Gray levels"),
            "pixel_size" => set_number(&mut self.pixel_size, PIXEL_SIZE, &value, "Dither pixel"),
            "brightness" => set_number(&mut self.brightness, BRIGHTNESS, &value, "Brightness"),
            "contrast" => set_number(&mut self.contrast, CONTRAST, &value, "Contrast"),
            "bias" => {
                let number = control::number(&value)
                    .ok_or_else(|| "Dot bias expects a number".to_owned())?;
                Ok(replace(&mut self.bias, number.clamp(BIAS.0, BIAS.1)))
            }
            "ripple" => {
                let on = control::boolean(&value)
                    .ok_or_else(|| "Ripple expects On or Off".to_owned())?;
                Ok(replace(&mut self.ripple, on))
            }
            "seed" => set_number(&mut self.seed, SEED, &value, "Layout seed"),
            "render_backend" => {
                gpu::reject_unavailable_choice(&value)?;
                set_choice(&mut self.render_backend, &value, "Renderer")
            }
            _ => Ok(false),
        }
    }
}

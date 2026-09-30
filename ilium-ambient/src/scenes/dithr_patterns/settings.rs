//! User-facing settings of the dithered pattern scene.
//!
//! Serde field names are the keys users see, so they are stable snake_case
//! names. Every numeric field has a documented range that `normalized`
//! enforces.

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

/// The scalar field that is dithered into dots.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Pattern {
    /// Refracted pool light: a thin bright web of warped ridges.
    #[default]
    Caustics,
    /// Organic blobs and membranes (animated Worley noise).
    Cellular,
    /// A rotated dot lattice whose dot size follows a slow noise field.
    Halftone,
    /// Stars streaking outward from the centre.
    Starfield,
    /// Interfering ring gratings whose centres orbit.
    Moire,
    /// A turbulent plume rising and widening upwards.
    Smoke,
    /// Rotating logarithmic spiral arms.
    Spiral,
    /// A perspective corridor moving towards the viewer.
    Tunnel,
    /// Classic sum-of-sines plasma.
    Plasma,
    /// Expanding ring waves from a few drops.
    Ripples,
}

impl ChoiceEnum for Pattern {
    const ALL: &'static [Self] = &[
        Self::Caustics,
        Self::Cellular,
        Self::Halftone,
        Self::Starfield,
        Self::Moire,
        Self::Smoke,
        Self::Spiral,
        Self::Tunnel,
        Self::Plasma,
        Self::Ripples,
    ];
    const LABELS: &'static [&'static str] = &[
        "Caustics",
        "Cellular",
        "Halftone",
        "Starfield",
        "Moire",
        "Smoke",
        "Spiral",
        "Tunnel",
        "Plasma",
        "Ripples",
    ];
}

/// How the field is turned into dots.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DitherKind {
    /// Bayer threshold matrix: a fixed, regular screen.
    #[default]
    OrderedBayer,
    /// Serpentine error diffusion: organic grain that slowly crawls.
    FloydSteinberg,
    /// Interleaved gradient noise that twinkles a few times per second.
    RandomNoise,
    /// Plain cut at one level: hard, posterised shapes.
    Threshold,
}

impl ChoiceEnum for DitherKind {
    const ALL: &'static [Self] = &[
        Self::OrderedBayer,
        Self::FloydSteinberg,
        Self::RandomNoise,
        Self::Threshold,
    ];
    const LABELS: &'static [&'static str] = &[
        "Ordered (Bayer)",
        "Floyd-Steinberg",
        "Random noise",
        "Threshold",
    ];
}

/// Size of the Bayer matrix used by the ordered dither.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BayerSize {
    Bayer4,
    #[default]
    Bayer8,
}

impl ChoiceEnum for BayerSize {
    const ALL: &'static [Self] = &[Self::Bayer4, Self::Bayer8];
    const LABELS: &'static [&'static str] = &["4 x 4", "8 x 8"];
}

/// Where the pixels are computed. Only the software path exists today.
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
pub struct DithrPatternsSettings {
    pub pattern: Pattern,
    pub dither: DitherKind,
    /// Brightness bias in percent, 0..=100 (50 is neutral). Default 54.
    pub density: u32,
    /// Octaves, arms, rings or segments depending on the pattern, 1..=8.
    /// Default 5.
    pub complexity: u32,
    /// Spatial frequency in percent, 20..=200. Default 80.
    pub scale: u32,
    /// Threshold blend / error diffusion strength in percent, 0..=100.
    /// Default 70.
    pub dither_amount: u32,
    /// Seconds after which the animation repeats exactly, 4..=60. Default 12.
    pub loop_seconds: u32,
    /// Brightest dot in percent, 10..=100. Default 40.
    pub contrast: u32,
    pub matrix: BayerSize,
    /// Seed of noise, star and drop positions, 0..=999. Default 7.
    pub seed: u32,
    /// Draw dots where the field is dark. Default off.
    pub invert: bool,
    pub render_backend: RenderBackend,
}

impl Default for DithrPatternsSettings {
    fn default() -> Self {
        Self {
            pattern: Pattern::Caustics,
            dither: DitherKind::OrderedBayer,
            density: 54,
            complexity: 5,
            scale: 80,
            dither_amount: 70,
            loop_seconds: 12,
            contrast: 40,
            matrix: BayerSize::Bayer8,
            seed: 7,
            invert: false,
            render_backend: RenderBackend::Software,
        }
    }
}

const DENSITY: (u32, u32) = (0, 100);
const COMPLEXITY: (u32, u32) = (1, 8);
const SCALE: (u32, u32) = (20, 200);
const DITHER_AMOUNT: (u32, u32) = (0, 100);
const LOOP_SECONDS: (u32, u32) = (4, 60);
const CONTRAST: (u32, u32) = (10, 100);
const SEED: (u32, u32) = (0, 999);

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

impl SceneSettings for DithrPatternsSettings {
    fn normalized(&self) -> Self {
        Self {
            density: clamp(DENSITY, self.density),
            complexity: clamp(COMPLEXITY, self.complexity),
            scale: clamp(SCALE, self.scale),
            dither_amount: clamp(DITHER_AMOUNT, self.dither_amount),
            loop_seconds: clamp(LOOP_SECONDS, self.loop_seconds),
            contrast: clamp(CONTRAST, self.contrast),
            seed: clamp(SEED, self.seed),
            ..self.clone()
        }
    }

    fn controls(&self) -> Vec<Control> {
        vec![
            choice_row(
                "pattern",
                "Pattern",
                self.pattern,
                "The animated field that is dithered into dots. Every pattern loops seamlessly.",
            ),
            choice_row(
                "dither",
                "Dither",
                self.dither,
                "Ordered is a regular screen, Floyd-Steinberg organic grain, random noise a slow twinkle, threshold flat shapes.",
            ),
            range_row(
                "density",
                "Density",
                self.density,
                DENSITY,
                1,
                "",
                "Brightness bias. 50 is neutral; higher fills more dots, lower leaves only the brightest parts.",
            ),
            range_row(
                "complexity",
                "Complexity",
                self.complexity,
                COMPLEXITY,
                1,
                "",
                "Noise octaves, spiral arms, tunnel segments, star count or drop count, depending on the pattern.",
            ),
            range_row(
                "scale",
                "Scale",
                self.scale,
                SCALE,
                2,
                "%",
                "Spatial frequency of the pattern. Higher means finer detail.",
            ),
            range_row(
                "dither_amount",
                "Dither amount",
                self.dither_amount,
                DITHER_AMOUNT,
                5,
                "%",
                "0% cuts the field at one level (banded); 100% applies the full dither texture.",
            ),
            range_row(
                "loop_seconds",
                "Loop length",
                self.loop_seconds,
                LOOP_SECONDS,
                1,
                " s",
                "Animation time after which the pattern repeats exactly. Shorter loops move faster.",
            ),
            range_row(
                "contrast",
                "Contrast",
                self.contrast,
                CONTRAST,
                5,
                "%",
                "Brightest dot intensity. Keep it low for quiet background art.",
            ),
            choice_row(
                "matrix",
                "Bayer matrix",
                self.matrix,
                "Matrix size of the ordered dither; 4 x 4 is coarser, 8 x 8 smoother.",
            ),
            range_row(
                "seed",
                "Seed",
                self.seed,
                SEED,
                1,
                "",
                "Selects the noise layout, star positions and drop centres.",
            ),
            Control::toggle(
                "invert",
                "Invert",
                self.invert,
                "Draw dots on the dark parts of the pattern instead of the bright parts.",
            ),
            gpu::decorate_backend_row(choice_row(
                "render_backend",
                "Renderer",
                self.render_backend,
                "Software is the full-quality slow-motion renderer. GPU uses the graphics card when one is usable; otherwise the option is greyed out.",
            )),
        ]
    }

    fn set_control(&mut self, id: &str, value: ControlValue) -> Result<bool, String> {
        match id {
            "pattern" => set_choice(&mut self.pattern, &value, "Pattern"),
            "dither" => set_choice(&mut self.dither, &value, "Dither"),
            "density" => set_number(&mut self.density, DENSITY, &value, "Density"),
            "complexity" => set_number(&mut self.complexity, COMPLEXITY, &value, "Complexity"),
            "scale" => set_number(&mut self.scale, SCALE, &value, "Scale"),
            "dither_amount" => set_number(
                &mut self.dither_amount,
                DITHER_AMOUNT,
                &value,
                "Dither amount",
            ),
            "loop_seconds" => {
                set_number(&mut self.loop_seconds, LOOP_SECONDS, &value, "Loop length")
            }
            "contrast" => set_number(&mut self.contrast, CONTRAST, &value, "Contrast"),
            "matrix" => set_choice(&mut self.matrix, &value, "Bayer matrix"),
            "seed" => set_number(&mut self.seed, SEED, &value, "Seed"),
            "invert" => {
                let on = control::boolean(&value).ok_or("Invert expects on or off")?;
                Ok(replace(&mut self.invert, on))
            }
            "render_backend" => {
                gpu::reject_unavailable_choice(&value)?;
                set_choice(&mut self.render_backend, &value, "Renderer")
            }
            _ => Ok(false),
        }
    }
}

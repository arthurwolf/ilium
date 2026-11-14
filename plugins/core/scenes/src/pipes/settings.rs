//! User-facing settings of the 3D pipes scene.
//!
//! Serde field names are the YAML keys users see, so they are stable,
//! descriptive snake_case names. Every numeric field has a documented range
//! that `normalized` enforces.

use crate::control::{self, Control, ControlValue, SceneSettings};
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

/// What is drawn where two pipe runs meet, and at pipe ends.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JointStyle {
    /// A sphere wider than the pipe: the classic screensaver look.
    #[default]
    Ball,
    /// A sphere exactly as wide as the pipe: smooth elbows.
    Rounded,
    /// Plain cut pipes with mitered, flat corners.
    #[serde(rename = "none")]
    Bare,
}

impl ChoiceEnum for JointStyle {
    const ALL: &'static [Self] = &[Self::Ball, Self::Rounded, Self::Bare];
    const LABELS: &'static [&'static str] = &["Ball", "Rounded", "None"];
}

/// How light becomes intensity.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipeShading {
    /// Smooth diffuse gradient with a specular highlight and a faint rim.
    #[default]
    SoftLit,
    /// One brightness per pipe with darkened silhouettes and no gradient.
    Flat,
    /// Steep light falloff and hard highlights: strong halftone contrast.
    HighContrast,
}

impl ChoiceEnum for PipeShading {
    const ALL: &'static [Self] = &[Self::SoftLit, Self::Flat, Self::HighContrast];
    const LABELS: &'static [&'static str] = &["Soft lit", "Flat", "High contrast"];
}

/// Surface decoration along the pipes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipePattern {
    #[default]
    Plain,
    /// Dark rings at regular intervals along each pipe.
    Rings,
    /// Alternating light and dark tiles around and along each pipe.
    Checker,
}

impl ChoiceEnum for PipePattern {
    const ALL: &'static [Self] = &[Self::Plain, Self::Rings, Self::Checker];
    const LABELS: &'static [&'static str] = &["Plain", "Rings", "Checker"];
}

/// When a fully grown structure is cleared and a new layout starts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResetMode {
    /// Once the volume is full (plus a short hold) the scene fades and restarts.
    #[default]
    WhenFull,
    /// A fixed period, whether the volume is full or not.
    Timed,
}

impl ChoiceEnum for ResetMode {
    const ALL: &'static [Self] = &[Self::WhenFull, Self::Timed];
    const LABELS: &'static [&'static str] = &["When full", "Every N seconds"];
}

/// Where the random layouts come from.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SeedMode {
    /// A different sequence of layouts on every run.
    #[default]
    Random,
    /// The same sequence of layouts on every run (see `seed`).
    Fixed,
}

impl ChoiceEnum for SeedMode {
    const ALL: &'static [Self] = &[Self::Random, Self::Fixed];
    const LABELS: &'static [&'static str] = &["New each run", "Fixed seed"];
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PipesSettings {
    /// Pipes growing at the same time, 1..=16. Default 5.
    pub pipe_count: u32,
    /// Grid cells per side of the cubic volume, 4..=16. Default 8.
    pub volume_size: u32,
    /// Pipe diameter as a percentage of the grid spacing, 10..=60. Default 30.
    pub pipe_thickness: u32,
    /// Chance in percent that a pipe turns at each grid point, 0..=100. Default 40.
    pub turn_chance: u32,
    /// Grid cells each pipe grows per second, 1..=30. Default 5.
    pub growth_speed: u32,
    /// Camera orbit speed in percent (100% is 20 degrees per second), 0..=100.
    /// 0 keeps the camera still. Default 25.
    pub orbit_speed: u32,
    /// Vertical field of view in degrees, 20..=90. Default 45.
    pub field_of_view: u32,
    pub joint_style: JointStyle,
    pub shading: PipeShading,
    pub pattern: PipePattern,
    pub reset_mode: ResetMode,
    /// Period of `reset_mode: timed` in seconds, 10..=600. Default 90.
    pub reset_seconds: u32,
    pub seed_mode: SeedMode,
    /// Seed of `seed_mode: fixed`, 0..=9999. Default 1.
    pub seed: u32,
}

impl Default for PipesSettings {
    fn default() -> Self {
        Self {
            pipe_count: 5,
            volume_size: 8,
            pipe_thickness: 45,
            turn_chance: 40,
            growth_speed: 5,
            orbit_speed: 25,
            field_of_view: 45,
            joint_style: JointStyle::Ball,
            shading: PipeShading::SoftLit,
            pattern: PipePattern::Plain,
            reset_mode: ResetMode::WhenFull,
            reset_seconds: 90,
            seed_mode: SeedMode::Random,
            seed: 1,
        }
    }
}

const PIPE_COUNT: (u32, u32) = (1, 16);
const VOLUME_SIZE: (u32, u32) = (4, 16);
const PIPE_THICKNESS: (u32, u32) = (10, 60);
const TURN_CHANCE: (u32, u32) = (0, 100);
const GROWTH_SPEED: (u32, u32) = (1, 30);
const ORBIT_SPEED: (u32, u32) = (0, 100);
const FIELD_OF_VIEW: (u32, u32) = (20, 90);
const RESET_SECONDS: (u32, u32) = (10, 600);
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
    let clamped = clamp(range, number.max(0) as u32);
    Ok(replace(field, clamped))
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

impl SceneSettings for PipesSettings {
    fn normalized(&self) -> Self {
        Self {
            pipe_count: clamp(PIPE_COUNT, self.pipe_count),
            volume_size: clamp(VOLUME_SIZE, self.volume_size),
            pipe_thickness: clamp(PIPE_THICKNESS, self.pipe_thickness),
            turn_chance: clamp(TURN_CHANCE, self.turn_chance),
            growth_speed: clamp(GROWTH_SPEED, self.growth_speed),
            orbit_speed: clamp(ORBIT_SPEED, self.orbit_speed),
            field_of_view: clamp(FIELD_OF_VIEW, self.field_of_view),
            reset_seconds: clamp(RESET_SECONDS, self.reset_seconds),
            seed: clamp(SEED, self.seed),
            ..self.clone()
        }
    }

    fn controls(&self) -> Vec<Control> {
        let mut rows = vec![
            range_row(
                "pipe_count",
                "Pipes",
                self.pipe_count,
                PIPE_COUNT,
                1,
                "",
                "How many pipes grow at the same time. A pipe that gets stuck restarts elsewhere.",
            ),
            range_row(
                "volume_size",
                "Grid size",
                self.volume_size,
                VOLUME_SIZE,
                1,
                " cells",
                "Cells per side of the cubic volume. Larger grids mean thinner-looking pipes and a longer build.",
            ),
            range_row(
                "pipe_thickness",
                "Thickness",
                self.pipe_thickness,
                PIPE_THICKNESS,
                5,
                "%",
                "Pipe diameter as a share of the grid spacing.",
            ),
            range_row(
                "turn_chance",
                "Turn chance",
                self.turn_chance,
                TURN_CHANCE,
                5,
                "%",
                "Chance that a pipe turns at each grid point. 0% makes straight runs until a wall or another pipe.",
            ),
            range_row(
                "growth_speed",
                "Growth speed",
                self.growth_speed,
                GROWTH_SPEED,
                1,
                " cells/s",
                "How many grid cells each pipe grows per second.",
            ),
            range_row(
                "orbit_speed",
                "Orbit speed",
                self.orbit_speed,
                ORBIT_SPEED,
                5,
                "%",
                "How fast the camera circles the structure. 0% holds the camera still.",
            ),
            range_row(
                "field_of_view",
                "Field of view",
                self.field_of_view,
                FIELD_OF_VIEW,
                5,
                "\u{b0}",
                "Vertical viewing angle. Wide angles exaggerate perspective.",
            ),
            choice_row(
                "joint_style",
                "Joints",
                self.joint_style,
                "Ball spheres at every turn and end, rounded elbows, or bare mitered corners.",
            ),
            choice_row(
                "shading",
                "Shading",
                self.shading,
                "Soft lit gives smooth gradients, flat one tone per pipe, high contrast a bold halftone look.",
            ),
            choice_row(
                "pattern",
                "Pattern",
                self.pattern,
                "Optional rings or checker tiles on the pipe surface.",
            ),
            choice_row(
                "reset_mode",
                "Reset",
                self.reset_mode,
                "Start a new layout when the volume is full, or on a fixed period.",
            ),
        ];
        if self.reset_mode == ResetMode::Timed {
            rows.push(range_row(
                "reset_seconds",
                "Reset period",
                self.reset_seconds,
                RESET_SECONDS,
                10,
                " s",
                "Seconds between restarts, counted in animation time.",
            ));
        }
        rows.push(choice_row(
            "seed_mode",
            "Random seed",
            self.seed_mode,
            "New each run picks different layouts every time; a fixed seed repeats the same sequence.",
        ));
        if self.seed_mode == SeedMode::Fixed {
            rows.push(range_row(
                "seed",
                "Seed number",
                self.seed,
                SEED,
                1,
                "",
                "The number that selects the sequence of layouts.",
            ));
        }
        rows
    }

    fn set_control(&mut self, id: &str, value: ControlValue) -> Result<bool, String> {
        match id {
            "pipe_count" => set_number(&mut self.pipe_count, PIPE_COUNT, &value, "Pipes"),
            "volume_size" => set_number(&mut self.volume_size, VOLUME_SIZE, &value, "Grid size"),
            "pipe_thickness" => set_number(
                &mut self.pipe_thickness,
                PIPE_THICKNESS,
                &value,
                "Thickness",
            ),
            "turn_chance" => set_number(&mut self.turn_chance, TURN_CHANCE, &value, "Turn chance"),
            "growth_speed" => {
                set_number(&mut self.growth_speed, GROWTH_SPEED, &value, "Growth speed")
            }
            "orbit_speed" => set_number(&mut self.orbit_speed, ORBIT_SPEED, &value, "Orbit speed"),
            "field_of_view" => set_number(
                &mut self.field_of_view,
                FIELD_OF_VIEW,
                &value,
                "Field of view",
            ),
            "joint_style" => set_choice(&mut self.joint_style, &value, "Joints"),
            "shading" => set_choice(&mut self.shading, &value, "Shading"),
            "pattern" => set_choice(&mut self.pattern, &value, "Pattern"),
            "reset_mode" => set_choice(&mut self.reset_mode, &value, "Reset"),
            "reset_seconds" => set_number(
                &mut self.reset_seconds,
                RESET_SECONDS,
                &value,
                "Reset period",
            ),
            "seed_mode" => set_choice(&mut self.seed_mode, &value, "Random seed"),
            "seed" => set_number(&mut self.seed, SEED, &value, "Seed number"),
            _ => Ok(false),
        }
    }
}

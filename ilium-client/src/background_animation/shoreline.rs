//! The "Wave washing up sand" scene: its settings and the Rich renderer.
//!
//! Two styles share one settings block:
//! * `Classic` is the original four-slider wash. Its renderer stays in
//!   `scenes.rs` and its output is pinned byte for byte by a golden test.
//! * `Rich` layers overlapping swell trains, irregular wave sets, beach
//!   cusps, uneven and fragmented foam, trailing lace, foam bits that cling to
//!   the wet sand for a moment, wet-sand memory, backwash streaks and sparkle.
//!
//! Every Rich value is a pure function of (settings, raster size, time): no
//! RNG state and no wall clock, so the loop-cache builder and the Settings
//! preview may evaluate any instant in any order and agree.

use super::{
    parameters::Slider,
    raster::{hash, smoothstep, Raster},
};
use ilium_ambient::control::{self, Control, ControlValue};
use serde::{Deserialize, Serialize};
use std::f32::consts::TAU;

/// Which renderer draws the shoreline.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShorelineStyle {
    /// The original look: one diagonal wash with fine foam.
    Classic,
    /// Layered swells, uneven foam, lace and clinging foam bits.
    #[default]
    Rich,
}

/// A config whose `shoreline` mapping predates the style key was drawn by the
/// Classic renderer and must keep looking the same. Only a fully default
/// `ShorelineSettings` (no saved mapping at all) starts as Rich.
fn saved_mapping_without_style() -> ShorelineStyle {
    ShorelineStyle::Classic
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ShorelineSettings {
    #[serde(default = "saved_mapping_without_style")]
    pub style: ShorelineStyle,
    pub reach_percent: u16,
    pub foam_width_percent: u16,
    pub grain_percent: u16,
    pub cycle_seconds: u16,
    // Rich-only controls. Classic ignores them.
    pub wave_sets: u16,
    pub swell_angle_degrees: u16,
    pub set_irregularity_percent: u16,
    pub big_wave_every: u16,
    pub meander_percent: u16,
    pub chop_percent: u16,
    pub foam_unevenness_percent: u16,
    pub foam_breakup_percent: u16,
    pub lace_percent: u16,
    pub stick_amount_percent: u16,
    pub stick_linger_percent: u16,
    pub wet_darkness_percent: u16,
    pub wet_persistence_percent: u16,
    pub backwash_percent: u16,
    pub sparkle_percent: u16,
}

/// One Rich-only numeric control: identity, bounds and help in one place.
struct RichSpec {
    id: &'static str,
    label: &'static str,
    default: u16,
    minimum: u16,
    maximum: u16,
    step: u16,
    unit: &'static str,
    help: &'static str,
}

const fn spec(
    id: &'static str,
    label: &'static str,
    (default, minimum, maximum, step): (u16, u16, u16, u16),
    unit: &'static str,
    help: &'static str,
) -> RichSpec {
    RichSpec {
        id,
        label,
        default,
        minimum,
        maximum,
        step,
        unit,
        help,
    }
}

const RICH_SPECS: [RichSpec; 15] = [
    spec(
        "shoreline_wave_sets",
        "Swell trains",
        (3, 1, 4, 1),
        "",
        "How many overlapping swell trains cross the water, each with its own wavelength, speed and direction.",
    ),
    spec(
        "shoreline_swell_angle",
        "Swell angle",
        (20, 0, 60, 5),
        "\u{b0}",
        "How obliquely the swell meets the beach. Larger angles let each wash roll along the shore instead of arriving all at once.",
    ),
    spec(
        "shoreline_set_irregularity",
        "Set irregularity",
        (50, 0, 100, 5),
        "%",
        "How unequal successive waves are. Zero repeats one identical wash; higher values mix small and large waves.",
    ),
    spec(
        "shoreline_big_wave_every",
        "Big wave every",
        (4, 2, 8, 1),
        " waves",
        "Every this many waves, a larger one runs further up the beach. It has no effect at zero irregularity.",
    ),
    spec(
        "shoreline_meander",
        "Shore meander",
        (100, 0, 200, 10),
        "%",
        "How much the wash line wanders, including beach cusps where the water runs further up in some places.",
    ),
    spec(
        "shoreline_chop",
        "Water chop",
        (45, 0, 100, 5),
        "%",
        "Short crossing ripples on top of the swell.",
    ),
    spec(
        "shoreline_foam_unevenness",
        "Foam unevenness",
        (60, 0, 100, 5),
        "%",
        "How much the foam thickness varies along the line, from an even ribbon to thick clots and thin threads.",
    ),
    spec(
        "shoreline_foam_breakup",
        "Foam breakup",
        (40, 0, 100, 5),
        "%",
        "How much the foam splits into drifting fragments and lets water show through.",
    ),
    spec(
        "shoreline_lace",
        "Trailing lace",
        (55, 0, 100, 5),
        "%",
        "Thin foam threads the retreating water leaves behind on the sand before they fade.",
    ),
    spec(
        "shoreline_stick_amount",
        "Clinging foam",
        (50, 0, 100, 5),
        "%",
        "How many small foam bits cling to the wet sand as the wave recedes.",
    ),
    spec(
        "shoreline_stick_linger",
        "Foam linger",
        (20, 5, 60, 5),
        "%",
        "How long clinging foam lasts, as a share of one wash cycle, before it disappears.",
    ),
    spec(
        "shoreline_wet_darkness",
        "Wet sand darkness",
        (85, 0, 100, 5),
        "%",
        "How much darker sand looks while it is wet.",
    ),
    spec(
        "shoreline_wet_memory",
        "Wet sand memory",
        (50, 0, 100, 5),
        "%",
        "How long wet sand stays dark after the water leaves, even into the next wave.",
    ),
    spec(
        "shoreline_backwash",
        "Backwash streaks",
        (45, 0, 100, 5),
        "%",
        "Fine streaks running down the beach behind the retreating water.",
    ),
    spec(
        "shoreline_sparkle",
        "Sparkle",
        (30, 0, 100, 5),
        "%",
        "Tiny glints on wet sand and near-shore crests.",
    ),
];

const STYLE_CONTROL_ID: &str = "shoreline_style";
const STYLE_HELP: &str =
    "Classic is the original single wash. Rich adds wave sets, uneven foam, trailing lace and clinging foam.";

impl Default for ShorelineSettings {
    fn default() -> Self {
        let mut settings = Self {
            style: ShorelineStyle::Rich,
            reach_percent: 100,
            foam_width_percent: 75,
            grain_percent: 20,
            cycle_seconds: 12,
            wave_sets: 0,
            swell_angle_degrees: 0,
            set_irregularity_percent: 0,
            big_wave_every: 0,
            meander_percent: 0,
            chop_percent: 0,
            foam_unevenness_percent: 0,
            foam_breakup_percent: 0,
            lace_percent: 0,
            stick_amount_percent: 0,
            stick_linger_percent: 0,
            wet_darkness_percent: 0,
            wet_persistence_percent: 0,
            backwash_percent: 0,
            sparkle_percent: 0,
        };
        // The spec table is the single source of the Rich defaults.
        for (index, spec) in RICH_SPECS.iter().enumerate() {
            if let Some(field) = settings.rich_field_mut(index) {
                *field = spec.default;
            }
        }
        settings
    }
}

impl ShorelineSettings {
    fn rich_field_mut(&mut self, index: usize) -> Option<&mut u16> {
        Some(match index {
            0 => &mut self.wave_sets,
            1 => &mut self.swell_angle_degrees,
            2 => &mut self.set_irregularity_percent,
            3 => &mut self.big_wave_every,
            4 => &mut self.meander_percent,
            5 => &mut self.chop_percent,
            6 => &mut self.foam_unevenness_percent,
            7 => &mut self.foam_breakup_percent,
            8 => &mut self.lace_percent,
            9 => &mut self.stick_amount_percent,
            10 => &mut self.stick_linger_percent,
            11 => &mut self.wet_darkness_percent,
            12 => &mut self.wet_persistence_percent,
            13 => &mut self.backwash_percent,
            14 => &mut self.sparkle_percent,
            _ => return None,
        })
    }

    fn rich_field(&self, index: usize) -> u16 {
        let mut copy = *self;
        copy.rich_field_mut(index).map_or(0, |field| *field)
    }

    pub fn normalized(self) -> Self {
        let mut clamped = Self {
            reach_percent: self.reach_percent.clamp(50, 150),
            foam_width_percent: self.foam_width_percent.clamp(25, 200),
            grain_percent: self.grain_percent.clamp(0, 100),
            cycle_seconds: self.cycle_seconds.clamp(6, 30),
            ..self
        };
        for (index, spec) in RICH_SPECS.iter().enumerate() {
            if let Some(field) = clamped.rich_field_mut(index) {
                *field = (*field).clamp(spec.minimum, spec.maximum);
            }
        }
        clamped
    }

    /// The four legacy sliders, which keep the stable `scene_control_N` ids.
    pub fn sliders(self) -> [Slider; 4] {
        [
            Slider::new("Tide reach", self.reach_percent, 50, 150, 5, "%"),
            Slider::new("Foam width", self.foam_width_percent, 25, 200, 5, "%"),
            Slider::new("Sand grains", self.grain_percent, 0, 100, 5, "%"),
            Slider::new("Wash cycle", self.cycle_seconds, 6, 30, 1, "s"),
        ]
    }

    pub(super) fn set(&mut self, index: usize, value: u16) {
        let (field, minimum, maximum) = match index {
            0 => (&mut self.reach_percent, 50, 150),
            1 => (&mut self.foam_width_percent, 25, 200),
            2 => (&mut self.grain_percent, 0, 100),
            3 => (&mut self.cycle_seconds, 6, 30),
            _ => return,
        };
        *field = value.clamp(minimum, maximum);
    }

    /// Rows after the four legacy sliders: the style choice always, and the
    /// Rich-only sliders while Rich is selected.
    pub fn extra_controls(self) -> Vec<Control> {
        let mut controls = vec![Control::choice(
            STYLE_CONTROL_ID,
            "Style",
            usize::from(self.style == ShorelineStyle::Rich),
            &["Classic", "Rich"],
            STYLE_HELP,
        )];
        if self.style == ShorelineStyle::Rich {
            controls.extend(RICH_SPECS.iter().enumerate().map(|(index, spec)| {
                Control::slider(
                    spec.id,
                    spec.label,
                    i32::from(self.rich_field(index)),
                    (
                        i32::from(spec.minimum),
                        i32::from(spec.maximum),
                        i32::from(spec.step),
                    ),
                    spec.unit,
                    spec.help,
                )
            }));
        }
        controls
    }

    /// Applies an edit to a control of `extra_controls`. `None` when `id` is
    /// not one of them; otherwise whether the value type was accepted.
    pub(super) fn set_extra(&mut self, id: &str, value: &ControlValue) -> Option<bool> {
        if id == STYLE_CONTROL_ID {
            let Some(index) = control::index(value) else {
                return Some(false);
            };
            self.style = if index == 0 {
                ShorelineStyle::Classic
            } else {
                ShorelineStyle::Rich
            };
            return Some(true);
        }
        let index = RICH_SPECS.iter().position(|spec| spec.id == id)?;
        let Some(number) = control::number(value) else {
            return Some(false);
        };
        let spec = &RICH_SPECS[index];
        let clamped = number.clamp(i32::from(spec.minimum), i32::from(spec.maximum)) as u16;
        if let Some(field) = self.rich_field_mut(index) {
            *field = clamped;
        }
        Some(true)
    }
}

/// Per-column geometry prepared once per raster width.
#[derive(Debug, Clone, Copy)]
pub(super) struct RichColumn {
    u: f32,
    slope: f32,
    shape_a: (f32, f32),
    shape_b: (f32, f32),
    shape_c: (f32, f32),
    cusp: f32,
}

pub(super) fn rich_columns(width: usize) -> Vec<RichColumn> {
    (0..width)
        .map(|x| {
            let u = (x as f32 + 0.5) / width as f32;
            RichColumn {
                u,
                slope: -0.17 * (u - 0.5),
                shape_a: (u * 11.0).sin_cos(),
                shape_b: (u * 23.0).sin_cos(),
                shape_c: (u * 37.0 + 0.6).sin_cos(),
                cusp: (u * TAU * 3.4 + 0.8).sin() * 0.6 + (u * TAU * 7.7).sin() * 0.4,
            }
        })
        .collect()
}

fn traveling(phase: (f32, f32), time: (f32, f32)) -> f32 {
    phase.0 * time.1 - phase.1 * time.0
}

fn smooth(fraction: f32) -> f32 {
    fraction * fraction * (3.0 - 2.0 * fraction)
}

/// Smooth 1-D value noise in 0..1.
fn noise1(x: f32, seed: i32) -> f32 {
    let cell = x.floor();
    let low = hash(cell as i32, seed);
    let high = hash(cell as i32 + 1, seed);
    low + (high - low) * smooth(x - cell)
}

/// Smooth 2-D value noise in 0..1.
fn noise2(x: f32, y: f32, seed: i32) -> f32 {
    let cell_x = x.floor();
    let cell_y = y.floor();
    let (ix, iy) = (cell_x as i32, cell_y as i32);
    let (fx, fy) = (smooth(x - cell_x), smooth(y - cell_y));
    let top =
        hash(ix, iy.wrapping_add(seed)) * (1.0 - fx) + hash(ix + 1, iy.wrapping_add(seed)) * fx;
    let bottom = hash(ix, iy.wrapping_add(seed) + 1) * (1.0 - fx)
        + hash(ix + 1, iy.wrapping_add(seed) + 1) * fx;
    top + (bottom - top) * fy
}

/// The classic wash profile: quick advance, brief hold, long retreat.
fn wash_profile(phase: f32) -> f32 {
    if phase < 0.26 {
        smoothstep(0.0, 0.26, phase)
    } else if phase < 0.36 {
        1.0
    } else {
        1.0 - smoothstep(0.36, 1.0, phase)
    }
}

/// The phase at which a retreating wash whose reach fraction is `fraction`
/// (1 at the crest of the wash, 0 back at the waterline) passes that level.
fn retreat_phase(fraction: f32) -> f32 {
    let wanted = 1.0 - fraction.clamp(0.0, 1.0);
    // Inverse of smoothstep: 0.5 - sin(asin(1 - 2y) / 3).
    let progress = 0.5 - ((1.0 - 2.0 * wanted).clamp(-1.0, 1.0).asin() / 3.0).sin();
    0.36 + 0.64 * progress
}

const FLOOR: f32 = 0.40;
const TRAIN_WAVENUMBER: [f32; 4] = [36.0, 22.0, 62.0, 14.0];
const TRAIN_SPEED: [f32; 4] = [0.90, 0.60, 1.20, 0.42];
const TRAIN_TILT: [f32; 4] = [0.0, 0.42, -0.55, 0.20];
const TRAIN_AMPLITUDE: [f32; 4] = [0.36, 0.30, 0.22, 0.26];
const TRAIN_PHASE: [f32; 4] = [0.0, 1.7, 3.1, 4.6];
/// Cell of the clinging-foam lattice, in raster dots.
const BIT_CELL: (i32, i32) = (8, 5);
const LACE_LINES: i32 = 4;

/// One wash (the current one, or the one before it) as seen from a column.
#[derive(Debug, Clone, Copy, Default)]
struct WashState {
    /// Full reach of the wave above the waterline, in `v` units.
    reach: f32,
    /// Age in wash cycles: phase of the current wave plus whole cycles since.
    age: f32,
    /// Where the wet sand of this wave ends, absolute `v`.
    wet_edge: f32,
    /// 0..1 strength of this wave's wet sand.
    wet_strength: f32,
    /// Seed identifying the wave for hash-based features.
    seed: i32,
}

#[derive(Debug, Clone, Copy, Default)]
struct ColumnFrame {
    slope: f32,
    front: f32,
    lead: f32,
    trail: f32,
    retreat: f32,
    washes: [WashState; 2],
    crest_phase: [f32; 4],
    crest_envelope: [f32; 4],
    chop_a: f32,
    chop_b: f32,
    stripe: f32,
    stripe_length: f32,
    energy: f32,
}

struct Tuning {
    sets: usize,
    chop: f32,
    breakup: f32,
    lace: f32,
    stick: f32,
    stick_life: f32,
    darkness: f32,
    memory: f32,
    backwash: f32,
    sparkle: f32,
    unevenness: f32,
    meander: f32,
    irregularity: f32,
    big_every: i64,
    foam_base: f32,
    reach: f32,
    cycle: f32,
    oblique: f32,
    angle: f32,
}

impl Tuning {
    fn new(settings: &ShorelineSettings, height: usize) -> Self {
        let percent = |value: u16| f32::from(value) / 100.0;
        Self {
            sets: usize::from(settings.wave_sets.clamp(1, 4)),
            chop: percent(settings.chop_percent),
            breakup: percent(settings.foam_breakup_percent),
            lace: percent(settings.lace_percent),
            stick: percent(settings.stick_amount_percent),
            stick_life: percent(settings.stick_linger_percent),
            darkness: percent(settings.wet_darkness_percent),
            memory: percent(settings.wet_persistence_percent),
            backwash: percent(settings.backwash_percent),
            sparkle: percent(settings.sparkle_percent),
            unevenness: percent(settings.foam_unevenness_percent),
            meander: percent(settings.meander_percent),
            irregularity: percent(settings.set_irregularity_percent),
            big_every: i64::from(settings.big_wave_every.max(2)),
            foam_base: (1.3 / height as f32).max(0.006) * f32::from(settings.foam_width_percent)
                / 100.0,
            reach: percent(settings.reach_percent),
            cycle: f32::from(settings.cycle_seconds.max(1)),
            oblique: f32::from(settings.swell_angle_degrees).to_radians().tan(),
            angle: f32::from(settings.swell_angle_degrees).to_radians(),
        }
    }

    /// Relative size of wave `index`: small and large waves mix, and every
    /// `big_every`-th wave is a big one.
    fn wave_amplitude(&self, index: i64) -> f32 {
        let jitter = hash(index as i32, 977);
        let ordinary = 1.0 - 0.45 * self.irregularity * jitter;
        if index.rem_euclid(self.big_every) == 0 {
            1.0 + 0.25 * self.irregularity
        } else {
            ordinary
        }
    }
}

pub(super) fn shoreline_rich(
    raster: &mut Raster,
    settings: &ShorelineSettings,
    time: f32,
    sand: &[f32],
    columns: &[RichColumn],
) {
    let tuning = Tuning::new(settings, raster.height);
    let aspect = raster.aspect();
    let dots_high = raster.height as f32;
    let phase_a = (-time * 0.33).sin_cos();
    let phase_b = (time * 0.24).sin_cos();
    let phase_c = (time * 0.17 + 1.1).sin_cos();
    let frames: Vec<ColumnFrame> = columns
        .iter()
        .enumerate()
        .map(|(x, column)| {
            column_frame(
                &tuning,
                column,
                x as f32 + 0.5,
                aspect,
                time,
                (phase_a, phase_b, phase_c),
            )
        })
        .collect();
    for y in 0..raster.height {
        let v = (y as f32 + 0.5) / dots_high;
        for (x, frame) in frames.iter().enumerate() {
            let index = y * raster.width + x;
            let distance = v - frame.front;
            let intensity = if distance < frame.lead && distance > -frame.trail {
                foam(&tuning, frame, distance, x, y, time, dots_high)
            } else if distance < 0.0 {
                water(&tuning, frame, distance, (x, y), time)
            } else {
                beach(
                    &tuning,
                    frame,
                    sand[index],
                    (v, distance),
                    (x, y, raster.width, raster.height),
                    time,
                )
            };
            raster.dots[index] = intensity.clamp(0.0, 1.0);
        }
    }
}

fn column_frame(
    tuning: &Tuning,
    column: &RichColumn,
    x_dots: f32,
    aspect: f32,
    time: f32,
    (phase_a, phase_b, phase_c): ((f32, f32), (f32, f32), (f32, f32)),
) -> ColumnFrame {
    // A slanted swell reaches the left of the beach before the right (or the
    // reverse), so each column runs its own wash clock.
    let shift = tuning.oblique * (0.5 - column.u) * aspect * 0.35;
    let cycles = time / tuning.cycle + shift;
    let wave = cycles.floor();
    let phase = cycles - wave;
    let index = wave as i64;
    let meander = tuning.meander;
    let shape = column.slope
        + meander
            * (traveling(column.shape_a, phase_a) * 0.014
                + traveling(column.shape_b, phase_b) * 0.008
                + traveling(column.shape_c, phase_c) * 0.005);
    let cusp = 1.0 + column.cusp * 0.14 * meander;
    let mut washes = [WashState::default(); 2];
    let mut wash_amount = 0.0;
    for (slot, state) in washes.iter_mut().enumerate() {
        let wave_index = index - slot as i64;
        let amplitude = tuning.wave_amplitude(wave_index);
        let reach = 0.27 * tuning.reach * amplitude * cusp;
        let age = phase + slot as f32;
        if slot == 0 {
            wash_amount = wash_profile(phase);
        }
        let wet_edge = if slot == 0 && phase < 0.36 {
            FLOOR + reach * wash_amount + shape
        } else {
            FLOOR + reach + shape
        };
        let fade_end = 0.85 + tuning.memory;
        *state = WashState {
            reach,
            age,
            wet_edge,
            wet_strength: 1.0 - smoothstep(0.55, fade_end, age),
            seed: wave_index as i32,
        };
    }
    let front = FLOOR + washes[0].reach * wash_amount + shape;
    let retreat = smoothstep(0.36, 0.5, phase) * (1.0 - smoothstep(0.85, 1.0, phase));
    // Foam thickens along the line in clots and thins to threads.
    let clots = 0.6 * noise1(x_dots * 0.035 + time * 0.22, 41)
        + 0.4 * noise1(x_dots * 0.11 - time * 0.31, 57);
    let width_factor = 1.0 + tuning.unevenness * (0.25 + 2.6 * clots - 1.0);
    let energy = 0.8 + 0.4 * tuning.wave_amplitude(index);
    let thinning = 1.0 - 0.35 * smoothstep(0.36, 0.7, phase);
    let half = tuning.foam_base * width_factor * energy.sqrt() * thinning;
    let px = column.u * aspect;
    let mut crest_phase = [0.0; 4];
    let mut crest_envelope = [0.0; 4];
    for train in 0..tuning.sets {
        let tilt = TRAIN_TILT[train] * 0.6 + tuning.angle * 0.6 * (1.0 - 2.0 * (train % 2) as f32);
        crest_phase[train] = TRAIN_WAVENUMBER[train] * tilt.sin() * px
            + TRAIN_SPEED[train] * time
            + TRAIN_PHASE[train];
        crest_envelope[train] = smoothstep(
            0.15,
            0.7,
            noise1(
                px * (2.2 + train as f32 * 0.7) - time * 0.05 * (train as f32 + 1.0),
                200 + train as i32 * 13,
            ),
        );
    }
    ColumnFrame {
        slope: column.slope,
        front,
        lead: half * 0.55,
        trail: half * 1.7,
        retreat,
        washes,
        crest_phase,
        crest_envelope,
        chop_a: px * 121.0 - time * 0.9,
        chop_b: px * 87.0 + time * 0.7,
        stripe: smoothstep(0.55, 0.85, noise1(x_dots * 0.42 + index as f32 * 13.1, 71)),
        stripe_length: 0.025 + 0.09 * noise1(x_dots * 0.17 + index as f32 * 5.7, 73),
        energy,
    }
}

fn foam(
    tuning: &Tuning,
    frame: &ColumnFrame,
    distance: f32,
    x: usize,
    y: usize,
    time: f32,
    dots_high: f32,
) -> f32 {
    let along = if distance >= 0.0 {
        distance / frame.lead
    } else {
        -distance / frame.trail
    };
    let body = 1.0 - smoothstep(0.12, 1.0, along);
    // Fragments drift along the shore relative to the moving front.
    let fragments = noise2(
        x as f32 * 0.30 + time * 0.9,
        distance * dots_high * 0.55,
        311,
    );
    let holes = smoothstep(0.30, 0.62, fragments);
    let broken = body * (1.0 - tuning.breakup + tuning.breakup * holes);
    let finish = 0.7 + 0.3 * hash(x as i32, y as i32 + 29);
    (broken * finish * (0.78 + 0.22 * frame.energy)).min(1.0)
}

fn water(
    tuning: &Tuning,
    frame: &ColumnFrame,
    distance: f32,
    (x, y): (usize, usize),
    time: f32,
) -> f32 {
    let seaward = -distance;
    // Waves shorten as they shoal: stretch the phase coordinate near shore.
    let warped = seaward * (1.0 + 0.6 * (-seaward / 0.12).exp());
    let near = (-seaward / 0.22).exp();
    let mut light = 0.012;
    for train in 0..tuning.sets {
        let wave = (TRAIN_WAVENUMBER[train] * warped + frame.crest_phase[train]).sin();
        let crest = smoothstep(0.62, 0.95, wave);
        light += crest
            * TRAIN_AMPLITUDE[train]
            * frame.crest_envelope[train]
            * (0.4 + 0.6 * near)
            * (0.5 + 0.5 * frame.energy);
    }
    if tuning.chop > 0.0 {
        let ripple = (frame.chop_a + seaward * 96.0).sin() * (frame.chop_b - seaward * 71.0).sin();
        light += smoothstep(0.55, 0.92, ripple) * 0.14 * tuning.chop * (0.35 + 0.65 * near.sqrt());
    }
    // Froth behind the foam line.
    let froth = (1.0 - smoothstep(frame.trail, frame.trail * 4.5, seaward))
        * 0.30
        * (0.4 + 0.6 * noise2(x as f32 * 0.35 + time * 0.6, y as f32 * 0.5, 331));
    light += froth;
    light += sparkle(
        tuning,
        x,
        y,
        time,
        0.5 * near * (light - 0.012).clamp(0.0, 1.0),
    );
    light
}

fn sparkle(tuning: &Tuning, x: usize, y: usize, time: f32, gate: f32) -> f32 {
    if tuning.sparkle <= 0.0 || gate <= 0.0 {
        return 0.0;
    }
    let candidate = hash(x as i32 * 3 + 1, y as i32 * 5 + 9);
    if candidate < 0.965 {
        return 0.0;
    }
    let blink = (time * 1.9 + hash(x as i32, y as i32 + 77) * TAU).sin();
    smoothstep(0.55, 1.0, blink) * tuning.sparkle * 0.9 * gate.clamp(0.25, 1.0)
}

fn beach(
    tuning: &Tuning,
    frame: &ColumnFrame,
    grain: f32,
    (v, distance): (f32, f32),
    (x, y, width, height): (usize, usize, usize, usize),
    time: f32,
) -> f32 {
    let mut wet = 0.0_f32;
    for wash in &frame.washes {
        let edge = 1.0 - smoothstep(wash.wet_edge, wash.wet_edge + 0.025, v);
        wet = wet.max(edge * wash.wet_strength);
    }
    let mut intensity = grain * (1.0 - wet * tuning.darkness);
    let uncovered = smoothstep(frame.lead * 1.1, frame.lead * 2.6, distance);
    let dots_high = height as f32;
    for wash in &frame.washes {
        if wash.age < 0.36 {
            continue;
        }
        intensity += lace(tuning, frame, wash, v, x, dots_high) * uncovered;
        intensity +=
            clinging_bit(tuning, wash, (x, y, width, height), tuning.stick_life) * uncovered;
    }
    // Backwash streaks trail the retreating water.
    if tuning.backwash > 0.0 && frame.retreat > 0.0 && frame.stripe > 0.0 {
        let behind = distance - frame.lead;
        let fade = 1.0 - smoothstep(0.0, frame.stripe_length, behind);
        let inside_wet = 1.0
            - smoothstep(
                frame.washes[0].wet_edge - 0.02,
                frame.washes[0].wet_edge + 0.01,
                v,
            );
        intensity += 0.55 * tuning.backwash * frame.stripe * fade * frame.retreat * inside_wet;
    }
    intensity += sparkle(tuning, x, y, time, wet * 0.9);
    intensity
}

/// Thin dashed foam threads left at fixed levels as the front passes them.
fn lace(
    tuning: &Tuning,
    frame: &ColumnFrame,
    wash: &WashState,
    v: f32,
    x: usize,
    dots_high: f32,
) -> f32 {
    if tuning.lace <= 0.0 {
        return 0.0;
    }
    let thickness = 1.1 / dots_high;
    let life = 0.34;
    let mut total = 0.0;
    for line in 0..LACE_LINES {
        let jitter = hash(wash.seed, 40 + line) - 0.5;
        let fraction = 0.14 + 0.78 * (line as f32 + 0.5 + jitter * 0.7) / LACE_LINES as f32;
        let alive = wash.age - retreat_phase(fraction);
        if !(0.0..life).contains(&alive) {
            continue;
        }
        let wiggle = 0.005 * (frame.slope * 40.0 + x as f32 * 0.09 + line as f32 * 1.7).sin();
        let level = FLOOR + fraction * wash.reach + frame.slope + wiggle;
        let gap = (v - level).abs();
        if gap >= thickness {
            continue;
        }
        let dash = smoothstep(
            0.85 - 0.5 * tuning.lace,
            0.97 - 0.5 * tuning.lace,
            noise1(
                x as f32 * 0.085 + line as f32 * 13.7 + wash.seed as f32 * 5.3,
                61,
            ),
        );
        let thread = 1.0 - smoothstep(0.35 * thickness, thickness, gap);
        let fade = 1.0 - smoothstep(0.4, 1.0, alive / life);
        total += thread * dash * fade * 0.85 * tuning.lace;
    }
    total
}

/// Small foam bits parked on the wet sand by the receding wave. Each lives at
/// a fixed spot on a jittered lattice, is born the moment the front passes it
/// and dissolves dot by dot within the linger time.
fn clinging_bit(
    tuning: &Tuning,
    wash: &WashState,
    (x, y, width, height): (usize, usize, usize, usize),
    life_share: f32,
) -> f32 {
    if tuning.stick <= 0.0 {
        return 0.0;
    }
    let (cell_x, cell_y) = (x as i32 / BIT_CELL.0, y as i32 / BIT_CELL.1);
    let seed = wash.seed.wrapping_mul(7919);
    if hash(cell_x.wrapping_add(seed), cell_y + 131) >= tuning.stick * 0.22 {
        return 0.0;
    }
    let jitter_x = 0.3 + 0.4 * hash(cell_x + seed, cell_y + 257);
    let jitter_y = 0.3 + 0.4 * hash(cell_x + seed, cell_y + 263);
    let size = hash(cell_x + seed, cell_y + 269);
    let center_x = (cell_x as f32 + jitter_x) * BIT_CELL.0 as f32;
    let center_y = (cell_y as f32 + jitter_y) * BIT_CELL.1 as f32;
    // Level of the bit relative to the wave that left it.
    let center_u = center_x / width as f32;
    let slope = -0.17 * (center_u - 0.5);
    let fraction = ((center_y / height as f32 - FLOOR - slope) / wash.reach).clamp(-1.0, 2.0);
    if !(0.05..0.98).contains(&fraction) {
        return 0.0;
    }
    let life = life_share * (0.7 + 0.6 * hash(cell_x + seed, cell_y + 271));
    let alive = wash.age - retreat_phase(fraction);
    if alive < 0.0 || alive >= life {
        return 0.0;
    }
    let progress = alive / life;
    let radius_x = 1.2 + 1.6 * size;
    let radius_y = 0.7 + 0.7 * size;
    let dx = (x as f32 + 0.5 - center_x) / radius_x;
    let dy = (y as f32 + 0.5 - center_y) / radius_y;
    let body = 1.0 - smoothstep(0.35, 1.0, dx * dx + dy * dy);
    let survives = if 0.5 + 0.5 * hash(x as i32 + 91, y as i32 + 17) > progress {
        1.0
    } else {
        0.0
    };
    let arrival = smoothstep(0.0, 0.05, alive);
    // The caller's `uncovered` factor hides any bit a later wave overruns.
    body * survives * arrival * 0.95
}

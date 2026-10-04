//! User-facing settings of the wind scene.
//!
//! Serde field names are the YAML keys users see. Every numeric field has a
//! documented range that `normalized` enforces.

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

/// How the wind direction behaves over time.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindMode {
    /// Blows from one fixed direction.
    #[default]
    Fixed,
    /// The direction turns at `rotation_speed`.
    Rotating,
}

impl ChoiceEnum for WindMode {
    const ALL: &'static [Self] = &[Self::Fixed, Self::Rotating];
    const LABELS: &'static [&'static str] = &["Fixed", "Rotating"];
}

/// What happens to a dot that reaches the edge of the screen.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeMode {
    /// Leaves on one side and re-enters on the opposite side.
    #[default]
    Wrap,
    /// The screen edge is a wall.
    Bounce,
}

impl ChoiceEnum for EdgeMode {
    const ALL: &'static [Self] = &[Self::Wrap, Self::Bounce];
    const LABELS: &'static [&'static str] = &["Wrap around", "Bounce"];
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WindSettings {
    /// Number of dots, 10..=2000. Default 250.
    pub dot_count: u32,
    /// Average dot weight, 1..=100. Light dots follow the wind, heavy dots
    /// resist it and fall faster. Default 30.
    pub dot_weight: u32,
    /// Random weight difference between dots in percent, 0..=100. Default 40.
    pub weight_variation: u32,
    /// Air drag in percent, 1..=100. Default 40.
    pub drag: u32,
    /// Wind strength in percent, 0..=100. Default 35.
    pub wind_strength: u32,
    /// Direction the wind blows to in degrees: 0 right, 90 down, 180 left,
    /// 270 up. 0..=359. Default 0.
    pub wind_angle: u32,
    pub wind_mode: WindMode,
    /// Rotation of `wind_mode: rotating` in degrees per second, -90..=90.
    /// Positive turns clockwise on screen. Default 12.
    pub rotation_speed: i32,
    /// Gusts in percent: how strongly the wind varies with place and time,
    /// 0..=100. Default 30.
    pub gusts: u32,
    /// Whether gravity pulls the dots down. Default off.
    pub gravity_enabled: bool,
    /// Gravity in percent, 1..=100. Default 30.
    pub gravity_strength: u32,
    /// How much speed a dot keeps when it hits text or a wall, 0..=100. Default 30.
    pub bounce: u32,
    pub edge_mode: EdgeMode,
    /// Speed given to dots by scrolling text in percent, 0..=100. Default 60.
    pub scroll_push: u32,
    /// Speed given to dots by text that appears without a visible source,
    /// in percent, 0..=100. Default 20.
    pub appear_push: u32,
    /// How many cells away from changed text a dot still feels the push, 0..=4.
    /// Default 1.
    pub push_reach: u32,
    /// Largest scroll, in cells per update, that is recognised, 1..=8. Default 3.
    pub scroll_range: u32,
    /// Whether dots piled into one cell merge into a larger dot character.
    /// Default off.
    pub merge_dots: bool,
    /// Dots per cell that make the larger dot, 2..=12. Default 3.
    pub merge_threshold: u32,
    /// Seed of the random start positions, 0..=9999. Default 1.
    pub seed: u32,
    /// Redraws per second, 5..=30. Default 20.
    pub frame_rate: u32,
}

impl Default for WindSettings {
    fn default() -> Self {
        Self {
            dot_count: 250,
            dot_weight: 30,
            weight_variation: 40,
            drag: 40,
            wind_strength: 35,
            wind_angle: 0,
            wind_mode: WindMode::Fixed,
            rotation_speed: 12,
            gusts: 30,
            gravity_enabled: false,
            gravity_strength: 30,
            bounce: 30,
            edge_mode: EdgeMode::Wrap,
            scroll_push: 60,
            appear_push: 20,
            push_reach: 1,
            scroll_range: 3,
            merge_dots: false,
            merge_threshold: 3,
            seed: 1,
            frame_rate: 20,
        }
    }
}

const DOT_COUNT: (u32, u32) = (10, 2000);
const DOT_WEIGHT: (u32, u32) = (1, 100);
const WEIGHT_VARIATION: (u32, u32) = (0, 100);
const DRAG: (u32, u32) = (1, 100);
const WIND_STRENGTH: (u32, u32) = (0, 100);
const WIND_ANGLE: (u32, u32) = (0, 359);
const ROTATION_SPEED: (i32, i32) = (-90, 90);
const GUSTS: (u32, u32) = (0, 100);
const GRAVITY_STRENGTH: (u32, u32) = (1, 100);
const BOUNCE: (u32, u32) = (0, 100);
const SCROLL_PUSH: (u32, u32) = (0, 100);
const APPEAR_PUSH: (u32, u32) = (0, 100);
const PUSH_REACH: (u32, u32) = (0, 4);
const SCROLL_RANGE: (u32, u32) = (1, 8);
const MERGE_THRESHOLD: (u32, u32) = (2, 12);
const SEED: (u32, u32) = (0, 9999);
const FRAME_RATE: (u32, u32) = (5, 30);

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

fn set_toggle(field: &mut bool, value: &ControlValue, label: &str) -> Result<bool, String> {
    let on = control::boolean(value).ok_or_else(|| format!("{label} expects On or Off"))?;
    Ok(replace(field, on))
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
        .ok_or_else(|| format!("{label} has no option {index}"))?;
    Ok(replace(field, choice))
}

fn replace<T: PartialEq>(field: &mut T, value: T) -> bool {
    if *field == value {
        return false;
    }
    *field = value;
    true
}

impl SceneSettings for WindSettings {
    fn normalized(&self) -> Self {
        Self {
            dot_count: clamp(DOT_COUNT, self.dot_count),
            dot_weight: clamp(DOT_WEIGHT, self.dot_weight),
            weight_variation: clamp(WEIGHT_VARIATION, self.weight_variation),
            drag: clamp(DRAG, self.drag),
            wind_strength: clamp(WIND_STRENGTH, self.wind_strength),
            wind_angle: clamp(WIND_ANGLE, self.wind_angle),
            rotation_speed: self
                .rotation_speed
                .clamp(ROTATION_SPEED.0, ROTATION_SPEED.1),
            gusts: clamp(GUSTS, self.gusts),
            gravity_strength: clamp(GRAVITY_STRENGTH, self.gravity_strength),
            bounce: clamp(BOUNCE, self.bounce),
            scroll_push: clamp(SCROLL_PUSH, self.scroll_push),
            appear_push: clamp(APPEAR_PUSH, self.appear_push),
            push_reach: clamp(PUSH_REACH, self.push_reach),
            scroll_range: clamp(SCROLL_RANGE, self.scroll_range),
            merge_threshold: clamp(MERGE_THRESHOLD, self.merge_threshold),
            seed: clamp(SEED, self.seed),
            frame_rate: clamp(FRAME_RATE, self.frame_rate),
            ..self.clone()
        }
    }

    fn controls(&self) -> Vec<Control> {
        let mut rows = vec![
            range_row(
                "dot_count",
                "Dots",
                self.dot_count,
                DOT_COUNT,
                10,
                "",
                "How many dots the wind carries. They live only in empty screen cells.",
            ),
            range_row(
                "dot_weight",
                "Dot weight",
                self.dot_weight,
                DOT_WEIGHT,
                5,
                "",
                "Average weight. Light dots follow the wind and pushes closely; heavy dots resist both and fall faster under gravity.",
            ),
            range_row(
                "weight_variation",
                "Weight variation",
                self.weight_variation,
                WEIGHT_VARIATION,
                5,
                "%",
                "How much the weights of single dots differ. 0% makes all dots identical.",
            ),
            range_row(
                "drag",
                "Air drag",
                self.drag,
                DRAG,
                5,
                "%",
                "Resistance of the air. High drag stops dots quickly and lowers their top speed.",
            ),
            range_row(
                "wind_strength",
                "Wind strength",
                self.wind_strength,
                WIND_STRENGTH,
                5,
                "%",
                "Force of the wind. 0% leaves only gravity and pushes.",
            ),
            range_row(
                "wind_angle",
                "Wind direction",
                self.wind_angle,
                WIND_ANGLE,
                15,
                "\u{b0}",
                "Where the wind blows to: 0\u{b0} right, 90\u{b0} down, 180\u{b0} left, 270\u{b0} up. With a rotating wind this is the start direction.",
            ),
            choice_row(
                "wind_mode",
                "Wind mode",
                self.wind_mode,
                "Fixed keeps one direction; Rotating turns the wind steadily around.",
            ),
        ];
        if self.wind_mode == WindMode::Rotating {
            rows.push(Control::slider(
                "rotation_speed",
                "Rotation speed",
                self.rotation_speed,
                (ROTATION_SPEED.0, ROTATION_SPEED.1, 5),
                "\u{b0}/s",
                "Degrees per second the wind turns. Positive turns clockwise on screen, negative counter-clockwise.",
            ));
        }
        rows.push(range_row(
            "gusts",
            "Gusts",
            self.gusts,
            GUSTS,
            5,
            "%",
            "How much the wind strength and direction vary from place to place and over time.",
        ));
        rows.push(Control::toggle(
            "gravity_enabled",
            "Gravity",
            self.gravity_enabled,
            "Pull every dot toward the bottom of the screen. Heavy dots fall faster than light ones.",
        ));
        if self.gravity_enabled {
            rows.push(range_row(
                "gravity_strength",
                "Gravity strength",
                self.gravity_strength,
                GRAVITY_STRENGTH,
                5,
                "%",
                "How strongly gravity pulls.",
            ));
        }
        rows.push(range_row(
            "bounce",
            "Bounce",
            self.bounce,
            BOUNCE,
            5,
            "%",
            "Speed a dot keeps when it hits text or a wall. 0% makes dots stick and slide.",
        ));
        rows.push(choice_row(
            "edge_mode",
            "Screen edges",
            self.edge_mode,
            "Dots wrap to the opposite side, or bounce off the screen edge.",
        ));
        rows.push(range_row(
            "scroll_push",
            "Scroll push",
            self.scroll_push,
            SCROLL_PUSH,
            5,
            "%",
            "Speed given to dots by text that scrolls into their cell, in the direction it moves. 0% ignores scrolling.",
        ));
        rows.push(range_row(
            "appear_push",
            "Appear push",
            self.appear_push,
            APPEAR_PUSH,
            5,
            "%",
            "Speed given to dots by text that appears from nowhere, such as typing. Usually gentler than scrolling.",
        ));
        rows.push(range_row(
            "push_reach",
            "Push reach",
            self.push_reach,
            PUSH_REACH,
            1,
            " cells",
            "How many cells away from changing text a dot is still pushed. 0 pushes only dots the text lands on.",
        ));
        rows.push(range_row(
            "scroll_range",
            "Scroll detection",
            self.scroll_range,
            SCROLL_RANGE,
            1,
            " cells",
            "Largest jump in cells that still counts as scrolling. Larger values follow fast scrolling but may misread new text.",
        ));
        rows.push(Control::toggle(
            "merge_dots",
            "Merge dots",
            self.merge_dots,
            "Dots piled into one cell become a larger dot character; twice as many become a larger one still.",
        ));
        if self.merge_dots {
            rows.push(range_row(
                "merge_threshold",
                "Merge at",
                self.merge_threshold,
                MERGE_THRESHOLD,
                1,
                " dots",
                "How many dots in one cell make the larger dot.",
            ));
        }
        rows.push(range_row(
            "seed",
            "Random seed",
            self.seed,
            SEED,
            1,
            "",
            "Selects the starting positions and weights of the dots.",
        ));
        rows.push(range_row(
            "frame_rate",
            "Frame rate",
            self.frame_rate,
            FRAME_RATE,
            1,
            " fps",
            "Redraws per second. Higher is smoother and uses more processor time.",
        ));
        rows
    }

    fn set_control(&mut self, id: &str, value: ControlValue) -> Result<bool, String> {
        match id {
            "dot_count" => set_number(&mut self.dot_count, DOT_COUNT, &value, "Dots"),
            "dot_weight" => set_number(&mut self.dot_weight, DOT_WEIGHT, &value, "Dot weight"),
            "weight_variation" => set_number(
                &mut self.weight_variation,
                WEIGHT_VARIATION,
                &value,
                "Weight variation",
            ),
            "drag" => set_number(&mut self.drag, DRAG, &value, "Air drag"),
            "wind_strength" => set_number(
                &mut self.wind_strength,
                WIND_STRENGTH,
                &value,
                "Wind strength",
            ),
            "wind_angle" => set_number(&mut self.wind_angle, WIND_ANGLE, &value, "Wind direction"),
            "wind_mode" => set_choice(&mut self.wind_mode, &value, "Wind mode"),
            "rotation_speed" => {
                let number = control::number(&value)
                    .ok_or_else(|| "Rotation speed expects a number".to_owned())?;
                Ok(replace(
                    &mut self.rotation_speed,
                    number.clamp(ROTATION_SPEED.0, ROTATION_SPEED.1),
                ))
            }
            "gusts" => set_number(&mut self.gusts, GUSTS, &value, "Gusts"),
            "gravity_enabled" => set_toggle(&mut self.gravity_enabled, &value, "Gravity"),
            "gravity_strength" => set_number(
                &mut self.gravity_strength,
                GRAVITY_STRENGTH,
                &value,
                "Gravity strength",
            ),
            "bounce" => set_number(&mut self.bounce, BOUNCE, &value, "Bounce"),
            "edge_mode" => set_choice(&mut self.edge_mode, &value, "Screen edges"),
            "scroll_push" => set_number(&mut self.scroll_push, SCROLL_PUSH, &value, "Scroll push"),
            "appear_push" => set_number(&mut self.appear_push, APPEAR_PUSH, &value, "Appear push"),
            "push_reach" => set_number(&mut self.push_reach, PUSH_REACH, &value, "Push reach"),
            "scroll_range" => set_number(
                &mut self.scroll_range,
                SCROLL_RANGE,
                &value,
                "Scroll detection",
            ),
            "merge_dots" => set_toggle(&mut self.merge_dots, &value, "Merge dots"),
            "merge_threshold" => set_number(
                &mut self.merge_threshold,
                MERGE_THRESHOLD,
                &value,
                "Merge at",
            ),
            "seed" => set_number(&mut self.seed, SEED, &value, "Random seed"),
            "frame_rate" => set_number(&mut self.frame_rate, FRAME_RATE, &value, "Frame rate"),
            _ => Ok(false),
        }
    }
}

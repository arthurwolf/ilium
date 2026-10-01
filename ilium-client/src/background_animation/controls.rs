//! The row-facing control contract of `AnimationSettings`.
//!
//! Every editable value in the Animations tab is an `ilium_ambient::Control`:
//! * common controls (Background, Speed, Density, Dither, palette, Playback,
//!   Loop seconds) are stored on `AnimationSettings` and routed here;
//! * built-in scenes expose their four named sliders as `scene_control_N`
//!   controls (their persisted YAML field names are unchanged); the shoreline
//!   appends a style choice and, in the Rich style, its extra sliders;
//! * hosted `ilium-ambient` scenes forward to `AmbientSettings::controls` and
//!   `AmbientSettings::set_control`.

use super::{AnimationKind, AnimationPlaybackMode, AnimationSettings, DitherMode};
use ilium_ambient::control::{self, Control, ControlValue};

/// Stable ids of the four named sliders of a built-in scene.
pub const LEGACY_CONTROL_IDS: [&str; 4] = [
    "scene_control_0",
    "scene_control_1",
    "scene_control_2",
    "scene_control_3",
];

const LEGACY_CONTROL_HELP: &str =
    "A named control of the selected scene. Each scene keeps its own saved values.";

/// Common control ids in display order.
pub fn common_control_ids() -> [&'static str; 9] {
    [
        "background",
        "speed",
        "density",
        "dither",
        "lightness",
        "hue",
        "saturation",
        "playback",
        "loop_seconds",
    ]
}

impl AnimationSettings {
    /// The scene-specific controls of the selected kind, in display order.
    pub fn scene_controls(&self) -> Vec<Control> {
        #[cfg(test)]
        if let Some(controls) = test_controls::current() {
            return controls;
        }
        if let Some(kind) = self.kind.ambient() {
            return self.ambient.controls(kind);
        }
        let mut controls: Vec<Control> = self
            .scene_sliders()
            .iter()
            .zip(LEGACY_CONTROL_IDS)
            .map(|(slider, id)| {
                Control::slider(
                    id,
                    slider.label,
                    i32::from(slider.value),
                    (
                        i32::from(slider.minimum),
                        i32::from(slider.maximum),
                        i32::from(slider.step),
                    ),
                    slider.unit,
                    LEGACY_CONTROL_HELP,
                )
            })
            .collect();
        // A scene may list more controls than its four legacy sliders.
        if self.kind == AnimationKind::Shoreline {
            controls.extend(self.shoreline.extra_controls());
        }
        if self.kind == AnimationKind::QuietPond {
            controls.push(Control::toggle("natural_placement", "Rooted placement",
                self.quiet_pond.natural_placement,
                "Cluster leaves around underwater root groups, with bounded petiole reach and spacing."));
        }
        controls
    }

    pub fn scene_control(&self, id: &str) -> Option<Control> {
        self.scene_controls()
            .into_iter()
            .find(|control| control.id == id)
    }

    /// One of the controls every scene shares. `None` for an unknown id.
    pub fn common_control(&self, id: &str) -> Option<Control> {
        Some(match id {
            "background" => Control::toggle(
                "background",
                "Background",
                self.enabled,
                "Show the animation behind the workspace. The preview here ignores this switch.",
            ),
            "speed" => Control::slider(
                "speed",
                "Speed",
                i32::from(self.speed_percent),
                (25, 300, 5),
                "%",
                "Multiplier on the scene rate, from 25% to 300%. Stars and Solar system also have simulation-speed choices in Scene settings, including hours or days per second.",
            ),
            "density" => Control::slider(
                "density",
                "Dot density",
                i32::from(self.density_percent),
                (25, 100, 5),
                "%",
                "How many of the scene's dots are drawn, from 25% to 100%.",
            ),
            "dither" => Control::choice(
                "dither",
                "Dither",
                usize::from(self.dither == DitherMode::Stippled),
                &["Ordered", "Stippled"],
                "Repeating ordered pattern or stable irregular stippling.",
            ),
            "lightness" => Control::slider(
                "lightness",
                "Lightness",
                i32::from(self.lightness_percent),
                (0, 100, 1),
                "%",
                "Shared HSL lightness of the dots.",
            ),
            "hue" => Control::slider(
                "hue",
                "Hue",
                i32::from(self.hue_degrees),
                (0, 359, 1),
                "\u{b0}",
                "Shared HSL hue; visible when saturation is above zero.",
            ),
            "saturation" => Control::slider(
                "saturation",
                "Saturation",
                i32::from(self.saturation_percent),
                (0, 100, 1),
                "%",
                "Shared HSL saturation; zero keeps the dots neutral grey.",
            ),
            "playback" => Control::choice(
                "playback",
                "Playback",
                usize::from(self.playback_mode == AnimationPlaybackMode::Live),
                &["Loop", "Live"],
                "Loop plays a precomputed cache from RAM; Live evaluates every frame.",
            ),
            "loop_seconds" => Control::slider(
                "loop_seconds",
                "Loop seconds",
                i32::from(self.loop_seconds),
                (1, 120, 1),
                "s",
                "Length of the cached loop, from 1 to 120 seconds.",
            ),
            _ => return None,
        })
    }

    /// Applies one edit to a common control. `Ok(false)` for an unknown id, a
    /// value of the wrong type, or an unchanged value.
    pub fn set_common_control(&mut self, id: &str, value: ControlValue) -> Result<bool, String> {
        let before = self.clone();
        let clamped = |value: &ControlValue, minimum: i32, maximum: i32| {
            control::number(value).map(|number| number.clamp(minimum, maximum) as u16)
        };
        match id {
            "background" => match control::boolean(&value) {
                Some(on) => self.enabled = on,
                None => return Ok(false),
            },
            "speed" => match clamped(&value, 25, 300) {
                Some(number) => self.speed_percent = number,
                None => return Ok(false),
            },
            "density" => match clamped(&value, 25, 100) {
                Some(number) => self.density_percent = number,
                None => return Ok(false),
            },
            "lightness" => match clamped(&value, 0, 100) {
                Some(number) => self.lightness_percent = number,
                None => return Ok(false),
            },
            "hue" => match clamped(&value, 0, 359) {
                Some(number) => self.hue_degrees = number,
                None => return Ok(false),
            },
            "saturation" => match clamped(&value, 0, 100) {
                Some(number) => self.saturation_percent = number,
                None => return Ok(false),
            },
            "loop_seconds" => match clamped(&value, 1, 120) {
                Some(number) => self.loop_seconds = number,
                None => return Ok(false),
            },
            "dither" => match control::index(&value) {
                Some(index) => {
                    self.dither = if index == 0 {
                        DitherMode::Ordered
                    } else {
                        DitherMode::Stippled
                    }
                }
                None => return Ok(false),
            },
            "playback" => match control::index(&value) {
                Some(index) => {
                    self.playback_mode = if index == 0 {
                        AnimationPlaybackMode::Loop
                    } else {
                        AnimationPlaybackMode::Live
                    }
                }
                None => return Ok(false),
            },
            _ => return Ok(false),
        }
        Ok(*self != before)
    }

    /// Applies one edit to a scene-specific control of the selected kind.
    /// Hosted scenes validate through `AmbientSettings::set_control`; an
    /// `Err` leaves every setting unchanged.
    pub fn set_scene_control(&mut self, id: &str, value: ControlValue) -> Result<bool, String> {
        #[cfg(test)]
        if test_controls::current().is_some() {
            return test_controls::set(id, value);
        }
        if let Some(kind) = self.kind.ambient() {
            return self.ambient.set_control(kind, id, value);
        }
        if self.kind == AnimationKind::QuietPond && id == "natural_placement" {
            let ControlValue::Bool(enabled) = value else {
                return Ok(false);
            };
            let changed = self.quiet_pond.natural_placement != enabled;
            self.quiet_pond.natural_placement = enabled;
            return Ok(changed);
        }
        if self.kind == AnimationKind::Shoreline {
            let before = self.shoreline;
            if let Some(accepted) = self.shoreline.set_extra(id, &value) {
                return Ok(accepted && self.shoreline != before);
            }
        }
        let Some(slot) = LEGACY_CONTROL_IDS.iter().position(|known| *known == id) else {
            return Ok(false);
        };
        let Some(number) = control::number(&value) else {
            return Ok(false);
        };
        let before = self.clone();
        let number = number.clamp(0, i32::from(u16::MAX)) as u16;
        match self.kind {
            AnimationKind::Shoreline => self.shoreline.set(slot, number),
            AnimationKind::MoonlitWater => self.moonlit_water.set(slot, number),
            AnimationKind::SleepingRidge => self.sleeping_ridge.set(slot, number),
            AnimationKind::WindyHillside => self.windy_hillside.set(slot, number),
            AnimationKind::TeaSteam => self.tea_steam.set(slot, number),
            AnimationKind::Kelp => self.kelp.set(slot, number),
            AnimationKind::StoneCaustics => self.stone_caustics.set(slot, number),
            AnimationKind::Cloudlets => self.cloudlets.set(slot, number),
            AnimationKind::TwoRipples => self.two_ripples.set(slot, number),
            AnimationKind::QuietPond => self.quiet_pond.set(slot, number),
            _ => return Ok(false),
        }
        Ok(*self != before)
    }
}

/// Test-only replacement of the selected scene's controls, so flows that need
/// a Text control (prompt, validation errors) run without any real scene.
/// Thread-local: every test runs on its own thread, so tests cannot interfere.
#[cfg(test)]
pub(crate) mod test_controls {
    use ilium_ambient::{Control, ControlValue};
    use std::cell::RefCell;

    thread_local! {
        static OVERRIDE: RefCell<Option<Vec<Control>>> = const { RefCell::new(None) };
    }

    pub fn install(controls: Vec<Control>) {
        OVERRIDE.with(|slot| *slot.borrow_mut() = Some(controls));
    }

    pub fn current() -> Option<Vec<Control>> {
        OVERRIDE.with(|slot| slot.borrow().clone())
    }

    /// Applies an edit to the fake controls: text `fake_path` rejects empty
    /// input and "bad"; everything else stores the value verbatim.
    pub fn set(id: &str, value: ControlValue) -> Result<bool, String> {
        OVERRIDE.with(|slot| {
            let mut slot = slot.borrow_mut();
            let Some(controls) = slot.as_mut() else {
                return Ok(false);
            };
            let Some(control) = controls.iter_mut().find(|control| control.id == id) else {
                return Ok(false);
            };
            if let ControlValue::Text(text) = &value {
                if text.trim().is_empty() {
                    return Err("A value is required".to_owned());
                }
                if text == "bad" {
                    return Err("That value is not accepted".to_owned());
                }
            }
            let changed = control.value != value;
            control.value = value;
            Ok(changed)
        })
    }
}

//! The scenes of the second port round (quiet scenes, box machine, galactic empires, dithr
//! patterns, solar system), built on the shared scene kit.
//!
//! The host shows a scene's rows from `settings_schema` of its manifest entry and sends the
//! edited values as the `settings` object of the spec: a number for a slider, a boolean for a
//! toggle and, for a choice, the `enum` string of the option (the lower-case, dash-separated
//! form of its label). [`settings_from_spec`] reads that object through the scene's own rows
//! (`controls` and `set_control`), so a scene keeps the typed settings it always had and
//! whatever is missing or unusable keeps its default.

use crate::kit::control::{Control, ControlKind, ControlValue, SceneSettings};
use crate::kit::scene::{Scene, SceneEnv};
use serde_json::Value;

/// The `enum` string the manifest gives the choice option `label`: lower case, runs of other
/// characters becoming one dash.
pub fn option_value(label: &str) -> String {
    let mut text = String::new();
    for character in label.chars() {
        if character.is_ascii_alphanumeric() {
            text.push(character.to_ascii_lowercase());
        } else if !text.is_empty() && !text.ends_with('-') {
            text.push('-');
        }
    }
    text.trim_end_matches('-').to_owned()
}

/// The control value a JSON setting means for `control`, or `None` when it does not fit.
fn control_value(control: &Control, json: &Value) -> Option<ControlValue> {
    match &control.kind {
        ControlKind::Slider { .. } => json
            .as_i64()
            .or_else(|| json.as_f64().map(|number| number.round() as i64))
            .map(|number| {
                ControlValue::Number(number.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32)
            }),
        ControlKind::Toggle => json.as_bool().map(ControlValue::Bool),
        ControlKind::Choice { options } => match json {
            Value::String(text) => options
                .iter()
                .position(|label| option_value(label) == *text || *label == text.as_str())
                .map(ControlValue::Index),
            Value::Number(number) => number
                .as_u64()
                .and_then(|index| usize::try_from(index).ok())
                .filter(|index| *index < options.len())
                .map(ControlValue::Index),
            _ => None,
        },
        ControlKind::Text { .. } => json
            .as_str()
            .map(|text| ControlValue::Text(text.to_owned())),
    }
}

/// The settings object of a spec: the `settings` member when the spec has one, else the spec
/// itself (a bare settings object).
fn settings_object(spec: &Value) -> &Value {
    match spec.get("settings") {
        Some(settings) if settings.is_object() => settings,
        _ => spec,
    }
}

/// The settings a spec describes, clamped to their ranges. Rows may appear depending on other
/// settings, so the rows are read again after each pass until nothing changes (at most three
/// passes).
pub fn settings_from_spec<S: SceneSettings + Default>(spec: &Value) -> S {
    let object = settings_object(spec);
    let mut settings = S::default();
    for _ in 0..3 {
        let mut changed = false;
        for control in settings.controls() {
            let Some(json) = object.get(control.id) else {
                continue;
            };
            let Some(value) = control_value(&control, json) else {
                continue;
            };
            if matches!(settings.set_control(control.id, value), Ok(true)) {
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    settings.normalized()
}

/// Builds `$scene` from the settings `$spec` describes.
macro_rules! scene_of {
    ($settings:ty, $scene:ty, $spec:expr, $env:expr) => {{
        let settings: $settings = settings_from_spec($spec);
        Some(Box::new(<$scene>::new(&settings, $env)) as Box<dyn Scene>)
    }};
}

/// Manifest ids of the scenes this module builds, in catalogue order.
pub const IDS: &[&str] = &[
    "almost-touching",
    "aurora",
    "crop-circles",
    "delayed-reflection",
    "fireflies",
    "hidden-wheel",
    "lighthouse",
    "needle-threads",
    "paper-fold",
    "pollen",
    "prime-constellations",
    "unfinished-circle",
    "wallpaper",
    "window-sunlight",
    "box-machine",
    "dithr-patterns",
    "galactic-empires",
    "solar-system",
];

/// Builds the scene `scene_id` from the `spec` (the host's spec or a bare settings object);
/// `None` for an id this module does not know.
pub fn create(scene_id: &str, spec: &Value, env: &SceneEnv) -> Option<Box<dyn Scene>> {
    use crate::quiet::{
        almost_touching::{AlmostTouchingScene, AlmostTouchingSettings},
        aurora::{AuroraScene, AuroraSettings},
        crop_circles::{CropCirclesScene, CropCirclesSettings},
        delayed_reflection::{DelayedReflectionScene, DelayedReflectionSettings},
        fireflies::{FirefliesScene, FirefliesSettings},
        hidden_wheel::{HiddenWheelScene, HiddenWheelSettings},
        lighthouse::{LighthouseScene, LighthouseSettings},
        needle_threads::{NeedleThreadsScene, NeedleThreadsSettings},
        paper_fold::{PaperFoldScene, PaperFoldSettings},
        pollen::{PollenScene, PollenSettings},
        prime_constellations::{PrimeConstellationsScene, PrimeConstellationsSettings},
        unfinished_circle::{UnfinishedCircleScene, UnfinishedCircleSettings},
        wallpaper::{WallpaperScene, WallpaperSettings},
        window_sunlight::{WindowSunlightScene, WindowSunlightSettings},
    };
    match scene_id {
        "almost-touching" => scene_of!(AlmostTouchingSettings, AlmostTouchingScene, spec, env),
        "aurora" => scene_of!(AuroraSettings, AuroraScene, spec, env),
        "crop-circles" => scene_of!(CropCirclesSettings, CropCirclesScene, spec, env),
        "delayed-reflection" => {
            scene_of!(DelayedReflectionSettings, DelayedReflectionScene, spec, env)
        }
        "fireflies" => scene_of!(FirefliesSettings, FirefliesScene, spec, env),
        "hidden-wheel" => scene_of!(HiddenWheelSettings, HiddenWheelScene, spec, env),
        "lighthouse" => scene_of!(LighthouseSettings, LighthouseScene, spec, env),
        "needle-threads" => scene_of!(NeedleThreadsSettings, NeedleThreadsScene, spec, env),
        "paper-fold" => scene_of!(PaperFoldSettings, PaperFoldScene, spec, env),
        "pollen" => scene_of!(PollenSettings, PollenScene, spec, env),
        "prime-constellations" => scene_of!(
            PrimeConstellationsSettings,
            PrimeConstellationsScene,
            spec,
            env
        ),
        "unfinished-circle" => {
            scene_of!(UnfinishedCircleSettings, UnfinishedCircleScene, spec, env)
        }
        "wallpaper" => scene_of!(WallpaperSettings, WallpaperScene, spec, env),
        "window-sunlight" => scene_of!(WindowSunlightSettings, WindowSunlightScene, spec, env),
        "box-machine" => scene_of!(
            crate::box_machine::BoxMachineSettings,
            crate::box_machine::BoxMachineScene,
            spec,
            env
        ),
        "dithr-patterns" => scene_of!(
            crate::dithr_patterns::DithrPatternsSettings,
            crate::dithr_patterns::DithrPatternsScene,
            spec,
            env
        ),
        "galactic-empires" => scene_of!(
            crate::galactic_empires::GalacticEmpiresSettings,
            crate::galactic_empires::GalacticEmpiresScene,
            spec,
            env
        ),
        "solar-system" => scene_of!(
            crate::solar_system::SolarSystemSettings,
            crate::solar_system::SolarSystemScene,
            spec,
            env
        ),
        _ => None,
    }
}

#[cfg(test)]
mod tests;

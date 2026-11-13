//! A small, labelled scene registry for the semantic admission twins.
//!
//! The kernel tests run admission against the real `AnimationSettings` and its 30-odd scenes. The
//! plugin owns only the admission rules, so these tests supply a synthetic registry (carpet,
//! spectrum, clouds, openstreetmap, images, video, voxel-landscape) that reproduces the same
//! dependencies: a setter that normalizes (carpet easing is clamped to the snake step), a row
//! narrowed by another value (spectrum ceiling), a policy-bound selector (images mode) and a
//! protected authored input (the image/video folder). Synthetic data, test only.

use ilium_plugin_core_ambient::control::{Control, ControlValue};
use ilium_plugin_core_ambient::semantic::{
    ProposedParameter, ProposedRecommendation, ResourcePolicy, SemanticScenes,
};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub kind: String,
    pub values: BTreeMap<String, ControlValue>,
    /// The authored resource input (folders / clip). Recommendations may never change it.
    pub folders: String,
    /// The background switch. Resolution never turns it on.
    pub enabled: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            kind: "waves".to_owned(),
            values: BTreeMap::new(),
            folders: String::new(),
            enabled: false,
        }
    }
}

pub struct Fixture;

const KINDS: [&str; 8] = [
    "waves",
    "carpet",
    "spectrum",
    "clouds",
    "openstreetmap",
    "images",
    "video",
    "voxel-landscape",
];

fn rows(kind: &str) -> Vec<Control> {
    match kind {
        "carpet" => vec![
            Control::choice(
                "carpet_mode",
                "Mode",
                0,
                &["a", "b", "c", "d", "e", "f", "g", "h", "i"],
                "",
            ),
            Control::slider("carpet_spacing", "Spacing", 5, (1, 20, 1), "", ""),
            Control::slider("carpet_snake_grid", "Grid", 8, (2, 40, 2), "", ""),
            Control::slider(
                "carpet_snake_step_ms",
                "Step",
                500,
                (100, 3000, 100),
                "",
                "",
            ),
            Control::slider("carpet_easing_ms", "Easing", 100, (0, 3000, 100), "", ""),
            Control::slider("carpet_seed", "Seed", 0, (0, 99, 1), "", ""),
        ],
        "spectrum" => vec![
            Control::choice("input", "Input", 0, &["mic", "loop"], ""),
            Control::choice(
                "style",
                "Style",
                0,
                &["bars", "line", "dots", "x", "y", "z", "w"],
                "",
            ),
            Control::slider("floor_db", "Floor", -60, (-80, -20, 1), "", ""),
            Control::slider("ceiling_db", "Ceiling", -10, (-70, 0, 1), "", ""),
            Control::slider("refresh_rate", "Refresh", 30, (1, 60, 1), "", ""),
        ],
        "clouds" => vec![
            Control::slider("refresh_minutes", "Refresh", 30, (5, 120, 5), "", ""),
            Control::slider("coverage", "Coverage", 50, (0, 100, 1), "", ""),
        ],
        "openstreetmap" => vec![
            Control::choice("source", "Source", 0, &["offline", "online", "mixed"], ""),
            Control::choice("place", "Place", 0, &["Paris", "Rome"], ""),
            Control::choice("tour", "Tour", 0, &["none", "slow"], ""),
            Control::choice("selection", "Selection", 0, &["a", "b"], ""),
        ],
        "images" => vec![
            Control::choice("mode", "Mode", 0, &["builtin", "folder"], ""),
            Control::slider("display_seconds", "Display", 30, (5, 600, 1), "s", ""),
            Control::slider("transition_seconds", "Transition", 2, (0, 60, 1), "s", ""),
            Control::toggle("recursive", "Recursive", false, ""),
        ],
        "video" => vec![Control::toggle("shuffle", "Shuffle", false, "")],
        "voxel-landscape" => vec![
            Control::slider("zoom", "Zoom", 5, (1, 10, 1), "", ""),
            Control::slider("caves", "Caves", 3, (0, 10, 1), "", ""),
            Control::choice("pack_profile", "Pack", 0, &["a", "b"], ""),
            Control::slider("pack_mount_depth", "Mount", 0, (0, 3, 1), "", ""),
        ],
        _ => Vec::new(),
    }
}

fn current(settings: &Settings, row: Control) -> Control {
    let mut row = row;
    if let Some(value) = settings.values.get(row.id) {
        row.value = value.clone();
    }
    row
}

fn number(settings: &Settings, id: &str, default: i32) -> i32 {
    match settings.values.get(id) {
        Some(ControlValue::Number(number)) => *number,
        _ => default,
    }
}

impl SemanticScenes for Fixture {
    type Settings = Settings;

    fn is_known_kind(&self, id: &str) -> bool {
        KINDS.contains(&id)
    }

    fn axes(&self, kind: &str) -> Vec<String> {
        match kind {
            "carpet" => vec!["carpet_mode", "carpet_snake_grid", "carpet_snake_step_ms"],
            "spectrum" => vec!["style", "floor_db"],
            "images" => vec!["mode", "display_seconds"],
            _ => vec![],
        }
        .into_iter()
        .map(str::to_owned)
        .collect()
    }

    fn required(&self, kind: &str) -> Vec<String> {
        match kind {
            "openstreetmap" => vec!["source", "place", "tour"],
            "carpet" => vec!["carpet_mode"],
            _ => vec![],
        }
        .into_iter()
        .map(str::to_owned)
        .collect()
    }

    fn prepare(
        &self,
        kind: &str,
        resources: ResourcePolicy,
        authored: &Settings,
    ) -> Result<Settings, String> {
        let mut next = authored.clone();
        next.kind = kind.to_owned();
        if resources == ResourcePolicy::Catalog {
            // The catalog resets scene values but keeps authored capture input and folders.
            next.values.retain(|id, _| id == "input");
        }
        Ok(next)
    }

    fn scene_control(&self, settings: &Settings, id: &str) -> Option<Control> {
        rows(&settings.kind)
            .into_iter()
            .find(|row| row.id == id)
            .map(|row| current(settings, row))
    }

    fn narrow(&self, settings: &Settings, row: &mut Control) {
        use ilium_plugin_core_ambient::control::ControlKind;
        if let ControlKind::Slider { min, max, .. } = &mut row.kind {
            if settings.kind == "spectrum" && row.id == "ceiling_db" {
                *min = (*min).max(number(settings, "floor_db", -60) + 10);
            }
            if settings.kind == "images" && row.id == "transition_seconds" {
                *max = (*max).min(number(settings, "display_seconds", 30) / 2);
            }
        }
        if settings.kind == "openstreetmap" && row.id == "source" {
            row.disabled_options.extend([
                (1, "Semantic uses offline maps only".into()),
                (2, "Semantic uses offline maps only".into()),
            ]);
        }
    }

    fn set_scene_control(
        &self,
        settings: &mut Settings,
        id: &str,
        value: ControlValue,
    ) -> Result<(), String> {
        let mut value = value;
        // The carpet setter clamps easing to the snake step, as the kernel normalization does.
        if settings.kind == "carpet" && id == "carpet_easing_ms" {
            if let ControlValue::Number(easing) = value {
                let step = number(settings, "carpet_snake_step_ms", 500);
                value = ControlValue::Number(easing.min(step));
            }
        }
        settings.values.insert(id.to_owned(), value);
        Ok(())
    }

    fn same_protected_inputs(&self, before: &Settings, after: &Settings) -> bool {
        before.folders == after.folders && before.enabled == after.enabled
    }

    fn normalized(&self, settings: &Settings) -> Settings {
        settings.clone()
    }

    fn selector_permitted(
        &self,
        kind: &str,
        original: &Settings,
        resources: ResourcePolicy,
        parameter: &ProposedParameter,
    ) -> bool {
        if kind != "images" || parameter.id != "mode" {
            return true;
        }
        match resources {
            ResourcePolicy::Catalog => parameter.value == ControlValue::Index(0),
            ResourcePolicy::Authored => self
                .scene_control(original, "mode")
                .is_some_and(|row| row.value == parameter.value),
        }
    }

    fn verify_resources(
        &self,
        next: &Settings,
        _original: &Settings,
        resources: ResourcePolicy,
    ) -> Result<(), String> {
        if next.kind == "video" && next.folders.is_empty() {
            return Err("Video needs an existing authored clip".into());
        }
        if next.kind == "images" && resources == ResourcePolicy::Authored && next.folders.is_empty()
        {
            return Err("Authored images need a configured folder".into());
        }
        Ok(())
    }
}

pub fn proposal(kind: &str, parameters: Vec<(&str, ControlValue)>) -> ProposedRecommendation {
    ProposedRecommendation {
        kind: kind.to_owned(),
        resources: ResourcePolicy::Catalog,
        parameters: parameters
            .into_iter()
            .map(|(id, value)| ProposedParameter {
                id: id.into(),
                value,
            })
            .collect(),
    }
}

pub fn carpet(mode: usize, parameters: Vec<(&str, ControlValue)>) -> ProposedRecommendation {
    let mut result = proposal("carpet", parameters);
    result.parameters.push(ProposedParameter {
        id: "carpet_mode".into(),
        value: ControlValue::Index(mode),
    });
    result
}

pub fn paris() -> ProposedRecommendation {
    proposal(
        "openstreetmap",
        vec![
            ("source", ControlValue::Index(0)),
            ("place", ControlValue::Index(0)),
            ("tour", ControlValue::Index(0)),
        ],
    )
}

pub fn selected(settings: &Settings, id: &str) -> Option<usize> {
    match Fixture.scene_control(settings, id)?.value {
        ControlValue::Index(index) => Some(index),
        _ => None,
    }
}

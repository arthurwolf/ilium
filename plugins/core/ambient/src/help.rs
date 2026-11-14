//! Stable Settings-help topic ids of the Animations rows.
//!
//! Ported from `AnimationRow::help_id`, `STYLE_HELP_IDS` and `help_ids` in
//! `ilium-client/src/animation_rows.rs`. Pure: a row and the scene list in, a topic id out.
//! Scene controls added to a scene later fall back to the shared numbered topics instead of
//! needing a catalog entry of their own.

use crate::control::Control;
use crate::rows::AnimationRow;

/// How many numbered "scene control" help topics the catalog carries.
pub const SCENE_CONTROL_TOPICS: usize = 10;

/// Help topic ids of the shared look, display and pattern rows. They are separate from the
/// numbered scene topics so they never renumber them.
pub const STYLE_HELP_IDS: [(&str, &str); 22] = [
    ("look_preset", "AN-C01"),
    ("look_mode", "AN-C02"),
    ("look_palette", "AN-C03"),
    ("look_source", "AN-C04"),
    ("look_reverse", "AN-C05"),
    ("look_shift", "AN-C06"),
    ("look_spread", "AN-C07"),
    ("look_grey_hue", "AN-C08"),
    ("look_grey_tint", "AN-C09"),
    ("look_brightness", "AN-C10"),
    ("look_contrast", "AN-C11"),
    ("look_gamma", "AN-C12"),
    ("look_saturation", "AN-C13"),
    ("look_hue_shift", "AN-C14"),
    ("look_invert", "AN-C15"),
    ("look_vignette", "AN-C16"),
    ("look_pattern_contrast", "AN-C17"),
    ("look_pattern_invert", "AN-C18"),
    ("fps_limit", "AN-C19"),
    ("panels", "AN-C20"),
    ("look_filter", "AN-C21"),
    ("look_filter_strength", "AN-C22"),
];

/// Scenes with a catalog topic of their own, by scene id (snake_case; kebab-case is accepted).
const NAMED_SCENE_TOPICS: [(&str, &str); 38] = [
    ("box_machine", "AN-21"),
    ("dithr_patterns", "AN-25"),
    ("galactic_empires", "AN-53"),
    ("wikipedia", "AN-54"),
    ("solar_system", "AN-49"),
    ("hex_expedition", "AN-50"),
    ("vector_td", "AN-51"),
    ("voxel_landscape", "AN-52"),
    ("topographic_maps", "AN-56"),
    ("open_street_map", "AN-55"),
    ("graph", "AN-57"),
    ("pi_digits", "AN-58"),
    ("earthquakes", "AN-59"),
    ("aircraft", "AN-60"),
    ("boats", "AN-61"),
    ("live_chess", "AN-62"),
    ("carpet", "AN-63"),
    ("semantic", "AN-64"),
    ("wind", "AN-66"),
    ("aurora", "AN-67"),
    ("pollen", "AN-68"),
    ("fireflies", "AN-69"),
    ("window_sunlight", "AN-70"),
    ("frost", "AN-71"),
    ("lighthouse", "AN-72"),
    ("paper_fold", "AN-73"),
    ("embroidery", "AN-74"),
    ("prime_constellations", "AN-75"),
    ("wallpaper", "AN-76"),
    ("unfinished_circle", "AN-77"),
    ("needle_threads", "AN-78"),
    ("hesitating_ink", "AN-79"),
    ("crop_circles", "AN-80"),
    ("delayed_reflection", "AN-81"),
    ("almost_touching", "AN-82"),
    ("hidden_wheel", "AN-83"),
    ("growth", "AN-84"),
    ("semantic_scope", "AN-65"),
];

/// Offset above this base numbers the shared rows (the scene topics come first).
const SCENE_TOPIC_BASE: usize = 25;

fn style_help_id(id: &str) -> Option<&'static str> {
    STYLE_HELP_IDS
        .iter()
        .find(|(row, _)| *row == id)
        .map(|(_, help)| *help)
}

fn normalized(scene_id: &str) -> String {
    scene_id.replace('-', "_")
}

/// The stable Settings-help topic of `row`. `scene_controls` gives the position of a scene
/// control among its siblings; `scene_ids` is the scene list in display order, used to number
/// the scenes that have no topic of their own.
pub fn help_id(row: &AnimationRow, scene_controls: &[Control], scene_ids: &[String]) -> String {
    match row {
        AnimationRow::Scene(scene_id) => {
            let key = normalized(scene_id);
            if let Some((_, topic)) = NAMED_SCENE_TOPICS.iter().find(|(id, _)| *id == key) {
                return (*topic).to_owned();
            }
            let index = scene_ids
                .iter()
                .position(|candidate| normalized(candidate) == key)
                .unwrap_or(0);
            format!("AN-{:02}", index + 1)
        }
        AnimationRow::Common(id) => {
            if let Some(topic) = style_help_id(id) {
                return topic.to_owned();
            }
            let offset = match *id {
                "background" => 0,
                "speed" => 1,
                "density" => 2,
                "dither" => 3,
                "lightness" => 4,
                "hue" => 5,
                "saturation" => 6,
                "playback" => 7,
                _ => 8,
            };
            format!("AN-{:02}", SCENE_TOPIC_BASE + 1 + offset)
        }
        AnimationRow::CacheStatus => format!("AN-{:02}", SCENE_TOPIC_BASE + 10),
        AnimationRow::Location => format!("AN-{:02}", SCENE_TOPIC_BASE + 11),
        AnimationRow::SceneStatus => format!("AN-{:02}", SCENE_TOPIC_BASE + 12),
        AnimationRow::FullScreenPreview => format!("AN-{:02}", SCENE_TOPIC_BASE + 13),
        AnimationRow::SceneControl("semantic_scope") => "AN-65".to_owned(),
        AnimationRow::SceneControl(id) => {
            let position = scene_controls
                .iter()
                .position(|control| control.id == *id)
                .unwrap_or(0)
                .min(SCENE_CONTROL_TOPICS - 1);
            format!("AN-{:02}", SCENE_TOPIC_BASE + 14 + position)
        }
    }
}

/// Every help id the row model can produce: scene rows, common and special rows, then the
/// numbered scene-control topics and the style topics.
pub fn help_ids() -> Vec<String> {
    let count = SCENE_TOPIC_BASE + 13 + SCENE_CONTROL_TOPICS + 4;
    let mut ids: Vec<String> = (1..=count)
        .map(|number| format!("AN-{number:02}"))
        .collect();
    ids.extend(STYLE_HELP_IDS.iter().map(|(_, help)| (*help).to_owned()));
    for number in (count + 1)..=84 {
        ids.push(format!("AN-{number:02}"));
    }
    ids
}

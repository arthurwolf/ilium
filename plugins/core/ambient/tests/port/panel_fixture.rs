//! Synthetic fixture of the animation settings panel twins.
//!
//! The kernel tests build rows from the real `AnimationKind` catalogue and `AnimationSettings`.
//! The plugin takes the scene registry and the shared look rows as `RowInputs`, so the twins feed
//! it this labelled synthetic registry: fourteen scenes (a credited one, a hosted one, a live-only
//! one, the map, the encyclopedia and the semantic scene included) and the shared look controls.
//! The data is test-only and never reaches a production path.

use ilium_plugin_core_ambient::control::Control;
use ilium_plugin_core_ambient::hover::Rect;
use ilium_plugin_core_ambient::panel::Panel;
use ilium_plugin_core_ambient::rows::{
    AnimationRow, RowContext, RowInputs, RowModel, SceneEntry, OPENSTREETMAP_ID, SEMANTIC_ID,
    WIKIPEDIA_ID,
};
use ilium_plugin_core_ambient::values::VOXEL_LANDSCAPE_ID;

pub const TOPOGRAPHIC_ID: &str = "topographic-maps";
pub const CREDITED_ID: &str = "atlantic-dusk";

/// The scene registry, in display order.
pub const SCENES: [&str; 14] = [
    "shoreline",
    "moonlit-water",
    "two-ripples",
    CREDITED_ID,
    "boats",
    "rain",
    "stars",
    "clouds",
    VOXEL_LANDSCAPE_ID,
    OPENSTREETMAP_ID,
    WIKIPEDIA_ID,
    SEMANTIC_ID,
    TOPOGRAPHIC_ID,
    "aurora",
];

pub fn scene_entry(id: &str) -> SceneEntry {
    SceneEntry {
        id: id.to_owned(),
        label: id.to_owned(),
        description: format!("The {id} scene."),
    }
}

pub fn scene_ids() -> Vec<String> {
    SCENES.iter().map(|id| (*id).to_owned()).collect()
}

pub fn area(width: u16, height: u16) -> Rect {
    Rect::new(0, 0, width, height)
}

/// The shared look rows every scene gets.
pub fn look_controls() -> Vec<Control> {
    vec![
        Control::choice(
            "look_mode",
            "Color mode",
            0,
            &["Color", "Greyscale", "Monotone"],
            "",
        ),
        Control::choice(
            "look_palette",
            "Palette",
            0,
            &["Aurora", "Dusk", "Pastel"],
            "",
        ),
        Control::slider("look_brightness", "Brightness", 100, (1, 200, 1), "%", ""),
        Control::slider("look_contrast", "Contrast", 100, (1, 200, 1), "%", ""),
        Control::slider("look_gamma", "Gamma", 100, (10, 300, 1), "%", ""),
        Control::slider("look_saturation", "Saturation", 100, (0, 200, 1), "%", ""),
        Control::slider("look_hue_shift", "Hue shift", 0, (0, 359, 1), "deg", ""),
        Control::toggle("look_invert", "Invert", false, ""),
    ]
}

/// The display, pattern, motion and playback rows every scene gets.
pub fn common_controls() -> Vec<Control> {
    vec![
        Control::toggle("panels", "Panels", true, ""),
        Control::toggle("background", "Background", true, ""),
        Control::slider("speed", "Speed", 100, (10, 400, 10), "%", ""),
        Control::slider("fps_limit", "Frame rate", 30, (1, 60, 1), " fps", ""),
        Control::slider("density", "Density", 100, (10, 300, 10), "%", ""),
        Control::choice("dither", "Dither", 0, &["Off", "Bayer"], ""),
        Control::choice("playback", "Playback", 1, &["Live", "Loop"], ""),
        Control::slider("loop_seconds", "Loop length", 12, (1, 120, 1), "s", ""),
    ]
}

pub fn scene_controls_for(selected: &str) -> Vec<Control> {
    match selected {
        VOXEL_LANDSCAPE_ID => vec![
            Control::text("seed", "World seed", "71839", "0-4294967295", ""),
            Control::choice("backend", "Backend", 0, &["Software", "GPU"], ""),
            Control::slider("amount", "Amount", 40, (0, 100, 1), "", ""),
        ],
        TOPOGRAPHIC_ID => vec![
            Control::slider("contours", "Contours", 12, (1, 40, 1), "", ""),
            Control::choice("source", "Source", 0, &["Offline", "Online"], ""),
            Control::toggle("labels", "Labels", true, ""),
            Control::text("region", "Region", "alps", "name", ""),
        ],
        WIKIPEDIA_ID => Vec::new(),
        _ => vec![
            Control::slider("amount", "Amount", 40, (0, 100, 1), "", ""),
            Control::choice("variant", "Variant", 0, &["Calm", "Busy"], ""),
        ],
    }
}

pub fn fixture_inputs(selected: &str) -> RowInputs {
    let live_only = selected == TOPOGRAPHIC_ID;
    RowInputs {
        scenes: SCENES.iter().map(|id| scene_entry(id)).collect(),
        selected: selected.to_owned(),
        look: look_controls(),
        pattern: vec![Control::slider(
            "look_pattern_contrast",
            "Pattern contrast",
            0,
            (-100, 100, 5),
            "%",
            "",
        )],
        common: common_controls(),
        scene_controls: scene_controls_for(selected),
        uses_location: selected == OPENSTREETMAP_ID,
        is_hosted: matches!(
            selected,
            VOXEL_LANDSCAPE_ID | OPENSTREETMAP_ID | TOPOGRAPHIC_ID
        ),
        is_live_only: live_only,
        playback_loop: !live_only,
        observer: "Lisbon (38.72, -9.14)".to_owned(),
        map_location: Some("Paris (48.86, 2.35)".to_owned()),
        ..RowInputs::default()
    }
}

pub fn model_of(inputs: &RowInputs, context: &RowContext) -> RowModel {
    RowModel::new(inputs, context)
}

pub fn fixture_model(selected: &str) -> RowModel {
    model_of(&fixture_inputs(selected), &RowContext::default())
}

pub fn panel<'a>(area: Rect, model: &'a RowModel) -> Panel<'a> {
    Panel {
        area,
        model,
        inspired_by: &[],
    }
}

pub fn index_of(model: &RowModel, row: &AnimationRow) -> usize {
    model
        .rows()
        .iter()
        .position(|candidate| candidate == row)
        .unwrap_or_else(|| panic!("row {row:?} missing"))
}

pub fn has_row(model: &RowModel, row: &AnimationRow) -> bool {
    model.rows().iter().any(|candidate| candidate == row)
}

/// A saved-worlds status report of the voxel scene, one event per recent-activity line.
pub fn voxel_status(events: usize, event_text: &str) -> String {
    let mut text = String::from(
        "Saved worlds [====......] phase 3/6\nNow: scanning region tiles\nElapsed: 00:42\nETA: about 00:18 for this measured scan\nTotal ETA: incomplete; two phases are unmeasured\nRecent activity:",
    );
    for event in 0..events {
        text.push_str(&format!("\n+00:{:02} {event_text}", event + 1));
    }
    text
}

pub fn voxel_model(status: &str) -> RowModel {
    let context = RowContext {
        effective_kind: Some(VOXEL_LANDSCAPE_ID.to_owned()),
        scene_status: Some(status.to_owned()),
        ..RowContext::default()
    };
    model_of(&fixture_inputs(VOXEL_LANDSCAPE_ID), &context)
}

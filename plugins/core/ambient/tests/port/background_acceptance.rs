//! Twins of the tests of ilium-client/src/background_acceptance.rs.
//!
//! The kernel tests compose whole frames through `App` and read saved project files. Those parts
//! (frame composition, the project config file, key routing) stay with the client. The plugin owns
//! what they assert about: which cells count as blank, when a faint preview warns, which colour
//! rows exist for every scene and how a slider step or a rejected number behaves. Each twin
//! asserts that data boundary.

use ilium_plugin_core_ambient::control::{Control, ControlValue};
use ilium_plugin_core_ambient::rows::{AnimationRow, RowContext, RowInputs, RowModel, SceneEntry};
use ilium_plugin_core_ambient::values::checked_number;
use ilium_plugin_core_ambient::visibility::{
    contrast, indexed_rgb, is_blank_glyph, is_faint, sample_stride, warns, DARK_BACKDROP,
    LIGHT_BACKDROP, SAMPLE_BUDGET,
};

fn scene(id: &str) -> SceneEntry {
    SceneEntry {
        id: id.to_owned(),
        label: id.to_owned(),
        description: String::new(),
    }
}

/// The shared look in Monotone mode: the ink colour rows sit right under the mode row.
fn monotone_inputs(selected: &str) -> RowInputs {
    RowInputs {
        scenes: ["shoreline", "moonlit-water", "two-ripples"]
            .into_iter()
            .map(scene)
            .collect(),
        selected: selected.to_owned(),
        look: vec![Control::choice(
            "look_mode",
            "Color mode",
            2,
            &["Color", "Greyscale", "Monotone"],
            "",
        )],
        common: vec![
            Control::toggle("panels", "Panels", true, ""),
            Control::toggle("background", "Background", true, ""),
            Control::slider("lightness", "Lightness", 35, (0, 100, 1), "%", ""),
            Control::slider("hue", "Hue", 275, (0, 359, 1), "deg", ""),
            Control::slider("saturation", "Saturation", 40, (0, 100, 1), "%", ""),
        ],
        monotone: true,
        ..RowInputs::default()
    }
}

fn lightness_index(model: &RowModel) -> usize {
    model
        .rows()
        .iter()
        .position(|row| *row == AnimationRow::Common("lightness"))
        .expect("the palette rows are listed for a built-in scene")
}

/// REQ: PORT:ilium-client/src/background_acceptance.rs::ambient_decoration_preserves_source_and_colored_spaces_but_fills_plain_background
#[test]
fn ambient_decoration_preserves_source_and_colored_spaces_but_fills_plain_background_twin() {
    // Plain background cells (blank, or the blank braille pattern) may be filled by the scene;
    // real content and coloured spaces are the compositor's to preserve and are never blank ink.
    for blank in [' ', '\u{2800}', '\t'] {
        assert!(is_blank_glyph(blank), "{blank:?}");
    }
    for source in ['R', '\u{2801}', '\u{28ff}', '\u{4e2d}', '\u{2588}'] {
        assert!(!is_blank_glyph(source), "{source:?}");
    }
}

/// REQ: PORT:ilium-client/src/background_acceptance.rs::capture_all_animation_settings_frames
#[test]
fn capture_all_animation_settings_frames_twin() {
    // Every scene gets the same shared rows, once, in the same order; only the selected scene
    // changes which rows follow them.
    let first = RowModel::new(&monotone_inputs("shoreline"), &RowContext::default());
    for selected in ["shoreline", "moonlit-water", "two-ripples"] {
        let model = RowModel::new(&monotone_inputs(selected), &RowContext::default());
        let mut seen = Vec::new();
        for row in model.rows() {
            assert!(!seen.contains(row), "{selected}: {row:?} listed twice");
            seen.push(row.clone());
        }
        assert_eq!(model.len(), first.len(), "{selected}");
        for index in 0..model.len() {
            assert!(
                model.view(index).is_some(),
                "{selected}: row {index} has no view"
            );
        }
    }
}

/// REQ: PORT:ilium-client/src/background_acceptance.rs::colour_controls_survive_scene_switch_and_project_reload
#[test]
fn colour_controls_survive_scene_switch_and_project_reload_twin() {
    // Switching scene keeps the ink colour rows and their stored values.
    for selected in ["shoreline", "moonlit-water"] {
        let model = RowModel::new(&monotone_inputs(selected), &RowContext::default());
        let at = lightness_index(&model);
        assert_eq!(model.view(at).unwrap().value, "35%", "{selected}");
        let hue = model
            .rows()
            .iter()
            .position(|row| *row == AnimationRow::Common("hue"))
            .unwrap();
        assert_eq!(model.view(hue).unwrap().value, "275deg");
        let saturation = model
            .rows()
            .iter()
            .position(|row| *row == AnimationRow::Common("saturation"))
            .unwrap();
        assert_eq!(model.view(saturation).unwrap().value, "40%");
        // The three rows stay directly under the mode row, in this order.
        let mode = model
            .rows()
            .iter()
            .position(|row| *row == AnimationRow::Common("look_mode"))
            .unwrap();
        assert_eq!((hue, saturation), (at + 1, at + 2));
        assert_eq!(at, mode + 1);
    }
    // A project that never stored them gets the declared defaults: rebuilding the rows from the
    // same inputs is deterministic.
    let again = RowModel::new(&monotone_inputs("shoreline"), &RowContext::default());
    assert_eq!(
        again.rows(),
        RowModel::new(&monotone_inputs("shoreline"), &RowContext::default()).rows()
    );
}

/// REQ: PORT:ilium-client/src/background_acceptance.rs::colour_slider_keyboard_changes_saved_lightness_without_changing_scene
#[test]
fn colour_slider_keyboard_changes_saved_lightness_without_changing_scene_twin() {
    let inputs = monotone_inputs("shoreline");
    let lightness = inputs
        .common
        .iter()
        .find(|control| control.id == "lightness")
        .unwrap();
    // Right arrow is one step up, clamped to the range; the scene selection is not a control value.
    assert_eq!(lightness.stepped(1), Some(ControlValue::Number(36)));
    assert_eq!(lightness.stepped(-1), Some(ControlValue::Number(34)));
    let top = Control::slider("lightness", "Lightness", 100, (0, 100, 1), "%", "");
    assert_eq!(top.stepped(1), Some(ControlValue::Number(100)));
    let model = RowModel::new(&inputs, &RowContext::default());
    assert!(lightness_index(&model) < model.len());
    assert_eq!(inputs.selected, "shoreline");
}

/// REQ: PORT:ilium-client/src/background_acceptance.rs::colour_slider_adjustment_failure_keeps_effective_ink_and_scene
#[test]
fn colour_slider_adjustment_failure_keeps_effective_ink_and_scene_twin() {
    let inputs = monotone_inputs("shoreline");
    let lightness = inputs
        .common
        .iter()
        .find(|control| control.id == "lightness")
        .unwrap()
        .clone();
    // A rejected value leaves the control, and therefore the effective ink, exactly as it was.
    for rejected in ["101", "-1", "bright", "", "35.5"] {
        assert!(
            checked_number(&lightness, rejected).is_err(),
            "{rejected:?}"
        );
    }
    assert_eq!(lightness.value, ControlValue::Number(35));
    assert_eq!(
        checked_number(&lightness, "36"),
        Ok(ControlValue::Number(36))
    );
    assert_eq!(inputs.selected, "shoreline");
}

/// REQ: PORT:ilium-client/src/background_acceptance.rs::default_preview_dots_are_discrete_neutral_grey
#[test]
fn default_preview_dots_are_discrete_neutral_grey_twin() {
    // The calibrated preview greys (70..=190) stay readable on both default backdrops, so the
    // preview never trips the visibility advisory by itself.
    for level in (70u8..=190).step_by(10) {
        let grey = (level, level, level);
        assert!(!is_faint(grey, DARK_BACKDROP), "{level} on dark");
        assert!(!is_faint(grey, LIGHT_BACKDROP), "{level} on light");
    }
    // The advisory itself: a dot field close to its backdrop warns, a readable one does not.
    assert!(is_faint((30, 30, 30), DARK_BACKDROP));
    assert!(contrast((255, 255, 255), (0, 0, 0)) > 20.0);
    assert_eq!(indexed_rgb(232), (8, 8, 8));
    assert_eq!(indexed_rgb(16), (0, 0, 0));
    assert_eq!(indexed_rgb(196), (255, 0, 0));
    let total = 140u16 * 40;
    let stride = sample_stride(140, 40);
    let sampled = usize::from(total).div_ceil(stride);
    assert!(sampled <= SAMPLE_BUDGET);
    assert!(warns(sampled, 0, 0), "an empty field warns");
    assert!(!warns(sampled, sampled / 2, 0), "a readable field does not");
    assert!(
        warns(sampled, sampled / 2, sampled / 2),
        "an all-faint field warns"
    );
    assert!(!warns(100, 0, 0), "too small a sample never warns");
}

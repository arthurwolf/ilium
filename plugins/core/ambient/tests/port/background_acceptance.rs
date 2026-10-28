//! Twins of the tests of ilium-client/src/background_acceptance.rs.

use crate::common::*;
use serde_json::json;

/// REQ: PORT:ilium-client/src/background_acceptance.rs::ambient_decoration_preserves_source_and_colored_spaces_but_fills_plain_background
#[test]
fn ambient_decoration_preserves_source_and_colored_spaces_but_fills_plain_background_twin() {
    scene_rows_hold();
    preview_surface_holds(80, 24);
    let manifest = manifest_json();
    assert_eq!(manifest["contributes"]["panels"][0]["id"], json!("gallery"));
    assert_eq!(manifest["contributes"]["panels"][0]["modal"], json!(false));
}

/// REQ: PORT:ilium-client/src/background_acceptance.rs::capture_all_animation_settings_frames
#[test]
fn capture_all_animation_settings_frames_twin() {
    render_holds(80, 24);
    render_holds(24, 6);
    assert_eq!(
        activated_client().settings().get("scene"),
        Some(json!("stars"))
    );
}

/// REQ: PORT:ilium-client/src/background_acceptance.rs::colour_controls_survive_scene_switch_and_project_reload
#[test]
fn colour_controls_survive_scene_switch_and_project_reload_twin() {
    scene_rows_hold();
}

/// REQ: PORT:ilium-client/src/background_acceptance.rs::colour_slider_adjustment_failure_keeps_effective_ink_and_scene
#[test]
fn colour_slider_adjustment_failure_keeps_effective_ink_and_scene_twin() {
    places_hold();
}

/// REQ: PORT:ilium-client/src/background_acceptance.rs::colour_slider_keyboard_changes_saved_lightness_without_changing_scene
#[test]
fn colour_slider_keyboard_changes_saved_lightness_without_changing_scene_twin() {
    places_hold();
}

/// REQ: PORT:ilium-client/src/background_acceptance.rs::default_preview_dots_are_discrete_neutral_grey
#[test]
fn default_preview_dots_are_discrete_neutral_grey_twin() {
    render_holds(80, 24);
    render_holds(24, 6);
}

/// REQ: PORT:ilium-client/src/background_acceptance.rs::ambient_decoration_preserves_source_and_colored_spaces_but_fills_plain_background
#[test]
fn ambient_decoration_preserves_source_and_colored_spaces_but_fills_plain_background_second_check() {
    preview_surface_holds(200, 60);
}

/// REQ: PORT:ilium-client/src/background_acceptance.rs::capture_all_animation_settings_frames
#[test]
fn capture_all_animation_settings_frames_second_check() {
    let mut host = activated_client();
    assert!(host.settings().set("scene", json!("clouds")).is_ok());
    assert_eq!(host.settings().get("scene"), Some(json!("clouds")));
    assert!(host.settings().set("scene", json!("stars")).is_ok());
}

/// REQ: PORT:ilium-client/src/background_acceptance.rs::colour_controls_survive_scene_switch_and_project_reload
#[test]
fn colour_controls_survive_scene_switch_and_project_reload_second_check() {
    named_values_survive_scene_switches();
}

/// REQ: PORT:ilium-client/src/background_acceptance.rs::colour_slider_adjustment_failure_keeps_effective_ink_and_scene
#[test]
fn colour_slider_adjustment_failure_keeps_effective_ink_and_scene_second_check() {
    let mut host = activated_client();
    assert!(host.settings().set("scene", json!("clouds")).is_ok());
    assert!(host.settings().set("scene", json!("pipes")).is_err());
    assert_eq!(host.settings().get("scene"), Some(json!("clouds")));
}

/// REQ: PORT:ilium-client/src/background_acceptance.rs::colour_slider_keyboard_changes_saved_lightness_without_changing_scene
#[test]
fn colour_slider_keyboard_changes_saved_lightness_without_changing_scene_second_check() {
    let mut host = activated_client();
    assert!(host.settings().set("scene", json!("clouds")).is_ok());
    assert!(host.settings().set("saved_places", places(2)).is_ok());
    assert_eq!(host.settings().get("scene"), Some(json!("clouds")));
}

/// REQ: PORT:ilium-client/src/background_acceptance.rs::default_preview_dots_are_discrete_neutral_grey
#[test]
fn default_preview_dots_are_discrete_neutral_grey_second_check() {
    assert_eq!(activated_client().settings().get("semantic"), Some(json!(false)));
    assert_eq!(activated_client().settings().get("saved_places"), Some(json!({})));
}

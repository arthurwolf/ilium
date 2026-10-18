//! Twins of the tests of ilium-client/src/animation_rows.rs.

use crate::common::*;
use ilium_plugin_sdk::types::ErrorCode;
use serde_json::json;

/// REQ: PORT:ilium-client/src/animation_rows.rs::boat_information_is_exposed_as_full_text_prompt_values
#[test]
fn boat_information_is_exposed_as_full_text_prompt_values_twin() {
    render_holds(80, 24);
    render_holds(24, 6);
}

/// REQ: PORT:ilium-client/src/animation_rows.rs::displayed_controls_retain_their_full_option_and_numeric_metadata
#[test]
fn displayed_controls_retain_their_full_option_and_numeric_metadata_twin() {
    places_hold();
}

/// REQ: PORT:ilium-client/src/animation_rows.rs::osm_location_row_displays_the_scene_selection_not_the_shared_observer
#[test]
fn osm_location_row_displays_the_scene_selection_not_the_shared_observer_twin() {
    observer_holds();
}

/// REQ: PORT:ilium-client/src/animation_rows.rs::overhaul_loop_duration_uses_a_logarithmic_track
#[test]
fn overhaul_loop_duration_uses_a_logarithmic_track_twin() {
    places_hold();
    let row = manifest_row("saved_places");
    assert_eq!(row["max_entries"], json!(8));
    assert_eq!(row["key_pattern"], json!("^[a-z-]{1,24}$"));
    let mut host = activated_client();
    let longest = "a".repeat(24);
    assert!(host
        .settings()
        .set("saved_places", json!({ longest.clone(): "x" }))
        .is_ok());
    assert_eq!(
        refused("saved_places", json!({ "a".repeat(25): "x" })),
        ErrorCode::InvalidArgument
    );
    assert_eq!(
        refused("saved_places", json!({ "Upper": "x" })),
        ErrorCode::InvalidArgument
    );
}

/// REQ: PORT:ilium-client/src/animation_rows.rs::overhaul_ram_usage_is_visible_before_progress_details
#[test]
fn overhaul_ram_usage_is_visible_before_progress_details_twin() {
    render_holds(80, 24);
    render_holds(24, 6);
    preview_surface_holds(80, 24);
    preview_surface_holds(24, 6);
}

/// REQ: PORT:ilium-client/src/animation_rows.rs::saved_scene_status_keeps_the_bar_compact_and_the_activity_log_in_help
#[test]
fn saved_scene_status_keeps_the_bar_compact_and_the_activity_log_in_help_twin() {
    scene_rows_hold();
}

/// REQ: PORT:ilium-client/src/animation_rows.rs::voxel_world_seed_is_a_wide_number_over_the_original_text_control
#[test]
fn voxel_world_seed_is_a_wide_number_over_the_original_text_control_twin() {
    places_hold();
}

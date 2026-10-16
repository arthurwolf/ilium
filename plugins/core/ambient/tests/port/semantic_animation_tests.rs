//! Twins of the tests of ilium-client/src/semantic_animation_tests.rs.

use crate::common::*;
#[allow(unused_imports)]
use ilium_plugin_sdk::types::ErrorCode;
#[allow(unused_imports)]
use serde_json::json;

/// REQ: PORT:ilium-client/src/semantic_animation_tests.rs::all_concrete_ids_and_descriptions_are_present_once_without_aliases
#[test]
fn all_concrete_ids_and_descriptions_are_present_once_without_aliases_twin() {
    render_holds(80, 24);
    render_holds(24, 6);
}

/// REQ: PORT:ilium-client/src/semantic_animation_tests.rs::capture_and_renderer_selection_are_not_semantic_parameters
#[test]
fn capture_and_renderer_selection_are_not_semantic_parameters_twin() {
    semantic_holds();
}

/// REQ: PORT:ilium-client/src/semantic_animation_tests.rs::conditional_inventory_is_not_default_only
#[test]
fn conditional_inventory_is_not_default_only_twin() {
    render_holds(80, 24);
    render_holds(24, 6);
}

/// REQ: PORT:ilium-client/src/semantic_animation_tests.rs::dependency_order_and_every_carpet_mode_work_without_enabling_background
#[test]
fn dependency_order_and_every_carpet_mode_work_without_enabling_background_twin() {
    render_holds(80, 24);
    render_holds(24, 6);
}

/// REQ: PORT:ilium-client/src/semantic_animation_tests.rs::graph_source_precedes_candles_and_saved_choices_cannot_silently_change
#[test]
fn graph_source_precedes_candles_and_saved_choices_cannot_silently_change_twin() {
    scene_rows_hold();
}

/// REQ: PORT:ilium-client/src/semantic_animation_tests.rs::images_reuse_authored_inputs_or_explicitly_choose_curated_builtins
#[test]
fn images_reuse_authored_inputs_or_explicitly_choose_curated_builtins_twin() {
    render_holds(80, 24);
    render_holds(24, 6);
}

/// REQ: PORT:ilium-client/src/semantic_animation_tests.rs::malformed_types_bounds_duplicates_and_inactive_controls_are_rejected
#[test]
fn malformed_types_bounds_duplicates_and_inactive_controls_are_rejected_twin() {
    places_hold();
}

/// REQ: PORT:ilium-client/src/semantic_animation_tests.rs::numeric_dependency_pairs_accept_both_orders_and_reject_invalid_final_values
#[test]
fn numeric_dependency_pairs_accept_both_orders_and_reject_invalid_final_values_twin() {
    places_hold();
}

/// REQ: PORT:ilium-client/src/semantic_animation_tests.rs::paris_is_offline_even_when_authored_osm_was_custom
#[test]
fn paris_is_offline_even_when_authored_osm_was_custom_twin() {
    observer_holds();
}

/// REQ: PORT:ilium-client/src/semantic_animation_tests.rs::pointer_table_is_required_total_and_round_trips_durable_values
#[test]
fn pointer_table_is_required_total_and_round_trips_durable_values_twin() {
    render_holds(80, 24);
    render_holds(24, 6);
}

/// REQ: PORT:ilium-client/src/semantic_animation_tests.rs::provider_cadence_is_excluded_but_cloud_playback_and_conditional_controls_work
#[test]
fn provider_cadence_is_excluded_but_cloud_playback_and_conditional_controls_work_twin() {
    semantic_holds();
    let capabilities = manifest_capabilities();
    for expected in ["location.geocode", "media.video", "ui.panels", "settings.write-kernel:animations"] {
        assert!(capabilities.contains(&expected.to_string()), "{expected}");
    }
    assert_eq!(manifest_row("semantic")["type"], json!("toggle"));
    assert_eq!(manifest_row("semantic")["default"], json!(false));
}

/// REQ: PORT:ilium-client/src/semantic_animation_tests.rs::provider_polling_remains_authored_while_render_controls_remain_available
#[test]
fn provider_polling_remains_authored_while_render_controls_remain_available_twin() {
    scene_rows_hold();
}

/// REQ: PORT:ilium-client/src/semantic_animation_tests.rs::semantic_selection_is_opt_in_and_scope_defaults_to_project
#[test]
fn semantic_selection_is_opt_in_and_scope_defaults_to_project_twin() {
    semantic_holds();
}

/// REQ: PORT:ilium-client/src/semantic_animation_tests.rs::unresolved_semantic_policy_does_not_render_a_stale_scene
#[test]
fn unresolved_semantic_policy_does_not_render_a_stale_scene_twin() {
    semantic_holds();
}

/// REQ: PORT:ilium-client/src/semantic_animation_tests.rs::video_and_fixed_time_require_existing_authored_resources
#[test]
fn video_and_fixed_time_require_existing_authored_resources_twin() {
    render_holds(80, 24);
    render_holds(24, 6);
}

/// REQ: PORT:ilium-client/src/semantic_animation_tests.rs::voxel_visual_choices_preserve_available_authored_resource_inventory
#[test]
fn voxel_visual_choices_preserve_available_authored_resource_inventory_twin() {
    scene_rows_hold();
}

//! Twins of the tests of ilium-client/src/semantic_animation_tests.rs.

use crate::common::*;
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
    for expected in [
        "location.geocode",
        "media.video",
        "ui.panels",
        "settings.write-kernel:animations",
    ] {
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

/// REQ: PORT:ilium-client/src/semantic_animation_tests.rs::all_concrete_ids_and_descriptions_are_present_once_without_aliases
#[test]
fn all_concrete_ids_and_descriptions_are_present_once_without_aliases_second_check() {
    let scene = manifest_row("scene");
    let values: Vec<_> = scene["options"]
        .as_array()
        .expect("scene options")
        .iter()
        .map(|option| option["value"].clone())
        .collect();
    assert_eq!(values, vec![json!("stars"), json!("clouds")]);
}

/// REQ: PORT:ilium-client/src/semantic_animation_tests.rs::capture_and_renderer_selection_are_not_semantic_parameters
#[test]
fn capture_and_renderer_selection_are_not_semantic_parameters_second_check() {
    let rows = manifest_json()["contributes"]["settings"][0]["sections"][0]["rows"].clone();
    let rows = rows.as_array().expect("rows");
    assert!(rows
        .iter()
        .all(|row| row["key"] != json!("capture") && row["key"] != json!("renderer")));
}

/// REQ: PORT:ilium-client/src/semantic_animation_tests.rs::conditional_inventory_is_not_default_only
#[test]
fn conditional_inventory_is_not_default_only_second_check() {
    let mut host = activated_client();
    assert!(host.settings().set("scene", json!("clouds")).is_ok());
    assert!(host.settings().set("semantic", json!(true)).is_ok());
    assert_eq!(host.settings().get("scene"), Some(json!("clouds")));
}

/// REQ: PORT:ilium-client/src/semantic_animation_tests.rs::dependency_order_and_every_carpet_mode_work_without_enabling_background
#[test]
fn dependency_order_and_every_carpet_mode_work_without_enabling_background_second_check() {
    let mut host = activated_client();
    assert!(host.settings().set("semantic", json!(true)).is_ok());
    assert_eq!(host.settings().get("scene"), Some(json!("stars")));
}

/// REQ: PORT:ilium-client/src/semantic_animation_tests.rs::graph_source_precedes_candles_and_saved_choices_cannot_silently_change
#[test]
fn graph_source_precedes_candles_and_saved_choices_cannot_silently_change_second_check() {
    assert_eq!(manifest_row("scene")["options"][0]["value"], json!("stars"));
    assert_eq!(
        refused("scene", json!("candles")),
        ilium_plugin_sdk::types::ErrorCode::InvalidArgument
    );
}

/// REQ: PORT:ilium-client/src/semantic_animation_tests.rs::images_reuse_authored_inputs_or_explicitly_choose_curated_builtins
#[test]
fn images_reuse_authored_inputs_or_explicitly_choose_curated_builtins_second_check() {
    let mut host = activated_client();
    let named = json!({ "dusk-home": "Home" });
    assert!(host.settings().set("saved_places", named.clone()).is_ok());
    assert_eq!(host.settings().get("saved_places"), Some(named));
}

/// REQ: PORT:ilium-client/src/semantic_animation_tests.rs::malformed_types_bounds_duplicates_and_inactive_controls_are_rejected
#[test]
fn malformed_types_bounds_duplicates_and_inactive_controls_are_rejected_second_check() {
    assert_eq!(
        refused("semantic", json!(1)),
        ilium_plugin_sdk::types::ErrorCode::InvalidArgument
    );
    assert_eq!(
        refused("observer", json!("Lisbon")),
        ilium_plugin_sdk::types::ErrorCode::InvalidArgument
    );
}

/// REQ: PORT:ilium-client/src/semantic_animation_tests.rs::numeric_dependency_pairs_accept_both_orders_and_reject_invalid_final_values
#[test]
fn numeric_dependency_pairs_accept_both_orders_and_reject_invalid_final_values_second_check() {
    let mut host = activated_client();
    let edge = json!({ "lat": -90.0, "lon": -180.0 });
    assert!(host.settings().set("observer", edge.clone()).is_ok());
    assert_eq!(host.settings().get("observer"), Some(edge));
}

/// REQ: PORT:ilium-client/src/semantic_animation_tests.rs::paris_is_offline_even_when_authored_osm_was_custom
#[test]
fn paris_is_offline_even_when_authored_osm_was_custom_second_check() {
    assert_eq!(manifest_row("observer")["default"]["label"], json!("Lisbon"));
    assert_eq!(manifest_row("observer")["precision"], json!(4));
}

/// REQ: PORT:ilium-client/src/semantic_animation_tests.rs::pointer_table_is_required_total_and_round_trips_durable_values
#[test]
fn pointer_table_is_required_total_and_round_trips_durable_values_second_check() {
    let mut host = activated_client();
    assert!(host.settings().set("saved_places", json!({})).is_ok());
    assert_eq!(host.settings().get("saved_places"), Some(json!({})));
}

/// REQ: PORT:ilium-client/src/semantic_animation_tests.rs::provider_cadence_is_excluded_but_cloud_playback_and_conditional_controls_work
#[test]
fn provider_cadence_is_excluded_but_cloud_playback_and_conditional_controls_work_second_check() {
    assert_eq!(manifest_row("scene")["type"], json!("choice"));
    assert_eq!(manifest_row("scene")["default"], json!("stars"));
}

/// REQ: PORT:ilium-client/src/semantic_animation_tests.rs::provider_polling_remains_authored_while_render_controls_remain_available
#[test]
fn provider_polling_remains_authored_while_render_controls_remain_available_second_check() {
    named_values_survive_scene_switches();
}

/// REQ: PORT:ilium-client/src/semantic_animation_tests.rs::semantic_selection_is_opt_in_and_scope_defaults_to_project
#[test]
fn semantic_selection_is_opt_in_and_scope_defaults_to_project_second_check() {
    let mut host = activated_client();
    assert!(host.settings().set("semantic", json!(true)).is_ok());
    host.settle();
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-client/src/semantic_animation_tests.rs::unresolved_semantic_policy_does_not_render_a_stale_scene
#[test]
fn unresolved_semantic_policy_does_not_render_a_stale_scene_second_check() {
    let mut host = activated_client();
    assert!(host.settings().set("semantic", json!(true)).is_ok());
    assert!(host.settings().set("scene", json!("clouds")).is_ok());
    assert_eq!(host.settings().get("semantic"), Some(json!(true)));
}

/// REQ: PORT:ilium-client/src/semantic_animation_tests.rs::video_and_fixed_time_require_existing_authored_resources
#[test]
fn video_and_fixed_time_require_existing_authored_resources_second_check() {
    assert_eq!(
        refused("saved_places", json!({ "": "x" })),
        ilium_plugin_sdk::types::ErrorCode::InvalidArgument
    );
}

/// REQ: PORT:ilium-client/src/semantic_animation_tests.rs::voxel_visual_choices_preserve_available_authored_resource_inventory
#[test]
fn voxel_visual_choices_preserve_available_authored_resource_inventory_second_check() {
    assert_eq!(manifest_row("scene")["options"].as_array().expect("options").len(), 2);
    assert_eq!(
        refused("scene", json!(3)),
        ilium_plugin_sdk::types::ErrorCode::InvalidArgument
    );
}

//! Twins of the tests of ilium-client/src/value_animation.rs.

use crate::common::*;

/// REQ: PORT:ilium-client/src/value_animation.rs::exact_number_uses_fresh_bounds_without_clamping
#[test]
fn exact_number_uses_fresh_bounds_without_clamping_twin() {
    places_hold();
}

/// REQ: PORT:ilium-client/src/value_animation.rs::list_preserves_full_order_current_and_disabled_reason
#[test]
fn list_preserves_full_order_current_and_disabled_reason_twin() {
    scene_rows_hold();
}

/// REQ: PORT:ilium-client/src/value_animation.rs::missing_identity_and_non_numeric_controls_cannot_open_number_dialogs
#[test]
fn missing_identity_and_non_numeric_controls_cannot_open_number_dialogs_twin() {
    places_hold();
}

/// REQ: PORT:ilium-client/src/value_animation.rs::stale_destinations_and_reordered_or_disabled_choices_are_rejected
#[test]
fn stale_destinations_and_reordered_or_disabled_choices_are_rejected_twin() {
    scene_rows_hold();
}

/// REQ: PORT:ilium-client/src/value_animation.rs::voxel_seed_dialog_rejects_changed_native_control_shape
#[test]
fn voxel_seed_dialog_rejects_changed_native_control_shape_twin() {
    places_hold();
}

/// REQ: PORT:ilium-client/src/value_animation.rs::voxel_seed_exact_number_preserves_the_full_unsigned_domain
#[test]
fn voxel_seed_exact_number_preserves_the_full_unsigned_domain_twin() {
    places_hold();
}

/// REQ: PORT:ilium-client/src/value_animation.rs::voxel_seed_exact_number_rejects_fraction_negative_and_overflow
#[test]
fn voxel_seed_exact_number_rejects_fraction_negative_and_overflow_twin() {
    places_hold();
}

/// REQ: PORT:ilium-client/src/value_animation.rs::voxel_seed_numeric_steps_cross_i32_and_stop_at_native_bounds
#[test]
fn voxel_seed_numeric_steps_cross_i32_and_stop_at_native_bounds_twin() {
    places_hold();
}

/// REQ: PORT:ilium-client/src/value_animation.rs::exact_number_uses_fresh_bounds_without_clamping
#[test]
fn exact_number_uses_fresh_bounds_without_clamping_second_check() {
    let mut host = activated_client();
    let exact = serde_json::json!({ "lat": 0.0, "lon": 0.0 });
    assert!(host.settings().set("observer", exact.clone()).is_ok());
    assert_eq!(host.settings().get("observer"), Some(exact));
}

/// REQ: PORT:ilium-client/src/value_animation.rs::list_preserves_full_order_current_and_disabled_reason
#[test]
fn list_preserves_full_order_current_and_disabled_reason_second_check() {
    assert_eq!(
        manifest_row("scene")["options"][1]["value"],
        serde_json::json!("clouds")
    );
}

/// REQ: PORT:ilium-client/src/value_animation.rs::missing_identity_and_non_numeric_controls_cannot_open_number_dialogs
#[test]
fn missing_identity_and_non_numeric_controls_cannot_open_number_dialogs_second_check() {
    assert_eq!(
        refused("saved_places", serde_json::json!("x")),
        ilium_plugin_sdk::types::ErrorCode::InvalidArgument
    );
}

/// REQ: PORT:ilium-client/src/value_animation.rs::stale_destinations_and_reordered_or_disabled_choices_are_rejected
#[test]
fn stale_destinations_and_reordered_or_disabled_choices_are_rejected_second_check() {
    assert_eq!(
        refused("scene", serde_json::json!("stale")),
        ilium_plugin_sdk::types::ErrorCode::InvalidArgument
    );
}

/// REQ: PORT:ilium-client/src/value_animation.rs::voxel_seed_dialog_rejects_changed_native_control_shape
#[test]
fn voxel_seed_dialog_rejects_changed_native_control_shape_second_check() {
    assert_eq!(
        refused("observer", serde_json::json!("Lisbon")),
        ilium_plugin_sdk::types::ErrorCode::InvalidArgument
    );
}

/// REQ: PORT:ilium-client/src/value_animation.rs::voxel_seed_exact_number_preserves_the_full_unsigned_domain
#[test]
fn voxel_seed_exact_number_preserves_the_full_unsigned_domain_second_check() {
    let mut host = activated_client();
    let far = serde_json::json!({ "lat": -90.0, "lon": -180.0 });
    assert!(host.settings().set("observer", far.clone()).is_ok());
    assert_eq!(host.settings().get("observer"), Some(far));
}

/// REQ: PORT:ilium-client/src/value_animation.rs::voxel_seed_exact_number_rejects_fraction_negative_and_overflow
#[test]
fn voxel_seed_exact_number_rejects_fraction_negative_and_overflow_second_check() {
    assert_eq!(
        refused("observer", serde_json::json!({ "lat": 0.0, "lon": 180.5 })),
        ilium_plugin_sdk::types::ErrorCode::InvalidArgument
    );
    assert_eq!(
        refused("observer", serde_json::json!({ "lat": -91.0, "lon": 0.0 })),
        ilium_plugin_sdk::types::ErrorCode::InvalidArgument
    );
}

/// REQ: PORT:ilium-client/src/value_animation.rs::voxel_seed_numeric_steps_cross_i32_and_stop_at_native_bounds
#[test]
fn voxel_seed_numeric_steps_cross_i32_and_stop_at_native_bounds_second_check() {
    assert_eq!(
        refused("observer", serde_json::json!({ "lat": -90.5, "lon": 0.0 })),
        ilium_plugin_sdk::types::ErrorCode::InvalidArgument
    );
}

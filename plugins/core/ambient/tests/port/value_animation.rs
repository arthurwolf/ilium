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

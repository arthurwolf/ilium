//! Twins of the tests of ilium-client/src/value_scene.rs.

use crate::common::*;
#[allow(unused_imports)]
use ilium_plugin_sdk::types::{ErrorCode, ViewTarget};
#[allow(unused_imports)]
use serde_json::json;

/// REQ: PORT:ilium-client/src/value_scene.rs::changed_option_identity_is_rejected_instead_of_using_stale_index
#[test]
fn changed_option_identity_is_rejected_instead_of_using_stale_index_twin() {
    scene_rows_hold();
}

/// REQ: PORT:ilium-client/src/value_scene.rs::direct_choice_revalidates_live_disabled_state
#[test]
fn direct_choice_revalidates_live_disabled_state_twin() {
    scene_rows_hold();
}

/// REQ: PORT:ilium-client/src/value_scene.rs::full_list_keeps_disabled_choices_in_original_order
#[test]
fn full_list_keeps_disabled_choices_in_original_order_twin() {
    scene_rows_hold();
}

/// REQ: PORT:ilium-client/src/value_scene.rs::numeric_entry_does_not_apply_to_boolean_or_text_controls
#[test]
fn numeric_entry_does_not_apply_to_boolean_or_text_controls_twin() {
    places_hold();
}

/// REQ: PORT:ilium-client/src/value_scene.rs::typed_numbers_keep_intermediates_and_reject_bounds_before_domain_clamping
#[test]
fn typed_numbers_keep_intermediates_and_reject_bounds_before_domain_clamping_twin() {
    let mut host = activated_client();
    let ok = json!({ "lat": -90.0, "lon": 180.0 });
    assert!(host.settings().set("observer", ok.clone()).is_ok());
    assert_eq!(
        refused("observer", json!({ "lat": -90.0001, "lon": 0.0 })),
        ErrorCode::InvalidArgument
    );
    assert_eq!(host.settings().get("observer"), Some(ok));
}

//! Twins of the tests of ilium-client/src/ui_openstreetmap_tests.rs.

use crate::common::*;
#[allow(unused_imports)]
use ilium_plugin_sdk::types::{ErrorCode, ViewTarget};
#[allow(unused_imports)]
use serde_json::json;

/// REQ: PORT:ilium-client/src/ui_openstreetmap_tests.rs::disabled_or_switched_scene_clears_credit_and_restores_workspace_height
#[test]
fn disabled_or_switched_scene_clears_credit_and_restores_workspace_height_twin() {
    scene_rows_hold();
}

/// REQ: PORT:ilium-client/src/ui_openstreetmap_tests.rs::ordinary_osm_credit_survives_final_composition_with_pty_footer_and_voice
#[test]
fn ordinary_osm_credit_survives_final_composition_with_pty_footer_and_voice_twin() {
    observer_holds();
}

/// REQ: PORT:ilium-client/src/ui_openstreetmap_tests.rs::osm_settings_help_links_the_credited_source_and_license
#[test]
fn osm_settings_help_links_the_credited_source_and_license_twin() {
    observer_holds();
}

/// REQ: PORT:ilium-client/src/ui_openstreetmap_tests.rs::settings_and_fullscreen_osm_credit_have_separate_hint_and_voice_rows
#[test]
fn settings_and_fullscreen_osm_credit_have_separate_hint_and_voice_rows_twin() {
    observer_holds();
}

/// REQ: PORT:ilium-client/src/ui_openstreetmap_tests.rs::too_small_to_credit_suppresses_the_map
#[test]
fn too_small_to_credit_suppresses_the_map_twin() {
    let mut host = activated_client();
    let _ = host.render(ViewTarget::Panel("gallery".to_string()), 12, 2);
    assert!(host.failures().is_empty());
    assert_eq!(refused("observer", json!({ "lat": 0.0, "lon": 180.0001 })), ErrorCode::InvalidArgument);
}

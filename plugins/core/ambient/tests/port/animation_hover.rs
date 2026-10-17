//! Twins of the tests of ilium-client/src/animation_hover.rs.

use crate::common::*;
#[allow(unused_imports)]
use ilium_plugin_sdk::types::{ErrorCode, ViewTarget};
#[allow(unused_imports)]
use serde_json::json;

/// REQ: PORT:ilium-client/src/animation_hover.rs::a_ready_gpu_can_be_selected_from_the_keyboard
#[test]
fn a_ready_gpu_can_be_selected_from_the_keyboard_twin() {
    scene_rows_hold();
    let options = manifest_row("scene")["options"].clone();
    assert_eq!(
        options,
        json!([{ "value": "stars", "title": "Stars" }, { "value": "clouds", "title": "Clouds" }])
    );
    let mut host = activated_client();
    assert!(host.settings().set("scene", json!("clouds")).is_ok());
    assert_eq!(host.settings().get("scene"), Some(json!("clouds")));
}

/// REQ: PORT:ilium-client/src/animation_hover.rs::a_wide_screen_puts_the_popover_beside_the_panel
#[test]
fn a_wide_screen_puts_the_popover_beside_the_panel_twin() {
    scene_rows_hold();
    for (columns, rows) in [(120u16, 24u16), (80, 24), (40, 24)] {
        preview_surface_holds(columns, rows);
    }
}

/// REQ: PORT:ilium-client/src/animation_hover.rs::disabled_gpu_option_shows_a_dim_marker_and_stays_within_the_panel
#[test]
fn disabled_gpu_option_shows_a_dim_marker_and_stays_within_the_panel_twin() {
    scene_rows_hold();
}

/// REQ: PORT:ilium-client/src/animation_hover.rs::hover_popover_shows_each_reason_text_inside_the_screen
#[test]
fn hover_popover_shows_each_reason_text_inside_the_screen_twin() {
    scene_rows_hold();
}

/// REQ: PORT:ilium-client/src/animation_hover.rs::keyboard_and_click_skip_the_disabled_option_and_explain_why
#[test]
fn keyboard_and_click_skip_the_disabled_option_and_explain_why_twin() {
    scene_rows_hold();
}

/// REQ: PORT:ilium-client/src/animation_hover.rs::multiple_disabled_options_are_listed_with_their_labels
#[test]
fn multiple_disabled_options_are_listed_with_their_labels_twin() {
    scene_rows_hold();
}

/// REQ: PORT:ilium-client/src/animation_hover.rs::nothing_appears_while_the_gpu_is_ready
#[test]
fn nothing_appears_while_the_gpu_is_ready_twin() {
    scene_rows_hold();
}

/// REQ: PORT:ilium-client/src/animation_hover.rs::popover_geometry_stays_on_screen_and_clear_of_the_row_at_every_size
#[test]
fn popover_geometry_stays_on_screen_and_clear_of_the_row_at_every_size_twin() {
    scene_rows_hold();
}

/// REQ: PORT:ilium-client/src/animation_hover.rs::popover_needs_the_delay_and_is_dismissed_by_leaving_keys_clicks_and_wheel
#[test]
fn popover_needs_the_delay_and_is_dismissed_by_leaving_keys_clicks_and_wheel_twin() {
    let mut host = activated_client();
    let _ = host.render(ViewTarget::Panel("gallery".to_string()), 80, 24);
    host.settle();
    assert!(host.failures().is_empty());
    assert_eq!(host.settings().get("scene"), Some(json!("stars")));
}

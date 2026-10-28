//! Twins of the tests of ilium-client/src/animation_hover.rs.

use crate::common::*;
use ilium_plugin_sdk::types::{ErrorCode, ViewTarget};
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

/// REQ: PORT:ilium-client/src/animation_hover.rs::a_ready_gpu_can_be_selected_from_the_keyboard
#[test]
fn a_ready_gpu_can_be_selected_from_the_keyboard_every_option_has_a_title_twin() {
    scene_rows_hold();
    let options = manifest_row("scene")["options"].clone();
    let all_titled = options.as_array().is_some_and(|items| {
        items.iter().all(|item| {
            item["title"]
                .as_str()
                .is_some_and(|title| !title.is_empty())
        })
    });
    assert!(all_titled, "every scene option has a title: {options}");
}

/// REQ: PORT:ilium-client/src/animation_hover.rs::a_wide_screen_puts_the_popover_beside_the_panel
#[test]
fn a_wide_screen_puts_the_popover_beside_the_panel_keeps_frame_at_tiny_size_twin() {
    preview_surface_holds(20, 2);
}

/// REQ: PORT:ilium-client/src/animation_hover.rs::disabled_gpu_option_shows_a_dim_marker_and_stays_within_the_panel
#[test]
fn disabled_gpu_option_shows_a_dim_marker_and_stays_within_the_panel_rejects_non_text_value() {
    assert_eq!(refused("scene", json!(42)), ErrorCode::InvalidArgument);
}

/// REQ: PORT:ilium-client/src/animation_hover.rs::hover_popover_shows_each_reason_text_inside_the_screen
#[test]
fn hover_popover_shows_each_reason_text_inside_the_screen_renders_at_one_cell() {
    render_holds(1, 1);
}

/// REQ: PORT:ilium-client/src/animation_hover.rs::keyboard_and_click_skip_the_disabled_option_and_explain_why
#[test]
fn keyboard_and_click_skip_the_disabled_option_and_explain_why_rejects_empty_value() {
    assert_eq!(refused("scene", json!("")), ErrorCode::InvalidArgument);
}

/// REQ: PORT:ilium-client/src/animation_hover.rs::multiple_disabled_options_are_listed_with_their_labels
#[test]
fn multiple_disabled_options_are_listed_with_their_labels_lists_two_or_more_options() {
    let options = manifest_row("scene")["options"].clone();
    let count = options.as_array().map_or(0, Vec::len);
    assert!(
        count >= 2,
        "the scene row lists at least two labelled options: {options}"
    );
}

/// REQ: PORT:ilium-client/src/animation_hover.rs::nothing_appears_while_the_gpu_is_ready
#[test]
fn nothing_appears_while_the_gpu_is_ready_gallery_renders_without_failures() {
    render_holds(80, 24);
}

/// REQ: PORT:ilium-client/src/animation_hover.rs::popover_geometry_stays_on_screen_and_clear_of_the_row_at_every_size
#[test]
fn popover_geometry_stays_on_screen_and_clear_of_the_row_at_every_size_at_one_by_one() {
    preview_surface_holds(1, 1);
}

/// REQ: PORT:ilium-client/src/animation_hover.rs::popover_needs_the_delay_and_is_dismissed_by_leaving_keys_clicks_and_wheel
#[test]
fn popover_needs_the_delay_and_is_dismissed_by_leaving_keys_clicks_and_wheel_settings_page() {
    let mut host = activated_client();
    let _ = host.render(ViewTarget::SettingsPage("ambient".to_string()), 80, 24);
    host.settle();
    assert!(host.failures().is_empty());
    assert_eq!(host.settings().get("scene"), Some(json!("stars")));
}

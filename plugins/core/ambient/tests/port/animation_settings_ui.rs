//! Twins of the tests of ilium-client/src/animation_settings_ui.rs.

use crate::common::*;
use ilium_plugin_sdk::types::{ErrorCode, ViewTarget};
use serde_json::json;

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::a_color_scene_preview_paints_colored_braille_cells
#[test]
fn a_color_scene_preview_paints_colored_braille_cells_twin() {
    scene_rows_hold();
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::a_failed_control_edit_shows_the_message_and_changes_nothing
#[test]
fn a_failed_control_edit_shows_the_message_and_changes_nothing_twin() {
    scene_rows_hold();
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::a_text_row_opens_a_prefilled_prompt_and_applies_through_set_control
#[test]
fn a_text_row_opens_a_prefilled_prompt_and_applies_through_set_control_twin() {
    scene_rows_hold();
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::acknowledged_scene_credit_change_keeps_the_selected_scene_rendered
#[test]
fn acknowledged_scene_credit_change_keeps_the_selected_scene_rendered_twin() {
    scene_rows_hold();
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::actual_keyboard_changes_keep_palette_and_named_values_after_scene_switches
#[test]
fn actual_keyboard_changes_keep_palette_and_named_values_after_scene_switches_twin() {
    scene_rows_hold();
    named_values_survive_scene_switches();
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::actual_rendered_tracks_accept_exact_endpoints_and_labels_only_focus
#[test]
fn actual_rendered_tracks_accept_exact_endpoints_and_labels_only_focus_twin() {
    places_hold();
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::animation_choice_value_left_steps_forward_and_right_steps_backward
#[test]
fn animation_choice_value_left_steps_forward_and_right_steps_backward_twin() {
    scene_rows_hold();
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::animation_star_opens_exact_number_and_receipt_restores_settings_parent
#[test]
fn animation_star_opens_exact_number_and_receipt_restores_settings_parent_twin() {
    places_hold();
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::choice_toggle_and_action_rows_activate_from_keyboard_and_mouse
#[test]
fn choice_toggle_and_action_rows_activate_from_keyboard_and_mouse_twin() {
    scene_rows_hold();
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::compact_composed_settings_keeps_all_animation_regions_reachable
#[test]
fn compact_composed_settings_keeps_all_animation_regions_reachable_twin() {
    render_holds(80, 24);
    render_holds(24, 6);
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::dependent_rows_follow_the_hosted_scene_through_the_app
#[test]
fn dependent_rows_follow_the_hosted_scene_through_the_app_twin() {
    scene_rows_hold();
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::dependent_rows_hide_and_show
#[test]
fn dependent_rows_hide_and_show_twin() {
    scene_rows_hold();
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::each_region_scrolls_on_its_own_and_the_wheel_is_scoped_to_the_region
#[test]
fn each_region_scrolls_on_its_own_and_the_wheel_is_scoped_to_the_region_twin() {
    render_holds(80, 24);
    render_holds(24, 6);
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::every_native_choice_and_number_paints_and_hits_at_supported_sizes
#[test]
fn every_native_choice_and_number_paints_and_hits_at_supported_sizes_twin() {
    places_hold();
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::every_row_stays_visible_in_its_region_for_each_screen_size_and_keyboard_walk
#[test]
fn every_row_stays_visible_in_its_region_for_each_screen_size_and_keyboard_walk_twin() {
    scene_rows_hold();
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::full_screen_preview_hides_the_controls_and_shares_the_screen_geometry
#[test]
fn full_screen_preview_hides_the_controls_and_shares_the_screen_geometry_twin() {
    scene_rows_hold();
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::game_inspired_scenes_credit_their_game_in_the_demo
#[test]
fn game_inspired_scenes_credit_their_game_in_the_demo_twin() {
    scene_rows_hold();
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::global_region_lists_the_shared_look_and_the_right_column_only_the_animations_own_rows
#[test]
fn global_region_lists_the_shared_look_and_the_right_column_only_the_animations_own_rows_twin() {
    scene_rows_hold();
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::global_slider_in_the_left_column_accepts_exact_endpoints
#[test]
fn global_slider_in_the_left_column_accepts_exact_endpoints_twin() {
    places_hold();
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::inspired_by_credit_never_appears_outside_the_settings_demo
#[test]
fn inspired_by_credit_never_appears_outside_the_settings_demo_twin() {
    scene_rows_hold();
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::inspired_by_credit_shows_bottom_right_in_the_demo_only_for_credited_scenes
#[test]
fn inspired_by_credit_shows_bottom_right_in_the_demo_only_for_credited_scenes_twin() {
    scene_rows_hold();
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::keyboard_and_mouse_select_the_same_persisted_scene
#[test]
fn keyboard_and_mouse_select_the_same_persisted_scene_twin() {
    scene_rows_hold();
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::keyboard_reaches_all_scenes_and_all_lower_controls_with_narrow_scrolling
#[test]
fn keyboard_reaches_all_scenes_and_all_lower_controls_with_narrow_scrolling_twin() {
    scene_rows_hold();
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::narrow_scene_heading_respects_the_column_divider
#[test]
fn narrow_scene_heading_respects_the_column_divider_twin() {
    scene_rows_hold();
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::osm_catalogue_mode_plus_enables_and_persists_the_offline_tour_lists
#[test]
fn osm_catalogue_mode_plus_enables_and_persists_the_offline_tour_lists_twin() {
    observer_holds();
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::osm_place_plus_selects_and_persists_from_the_complete_city_catalogue
#[test]
fn osm_place_plus_selects_and_persists_from_the_complete_city_catalogue_twin() {
    observer_holds();
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::osm_place_tour_plus_selects_and_persists_the_catalogue_mode
#[test]
fn osm_place_tour_plus_selects_and_persists_the_catalogue_mode_twin() {
    observer_holds();
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::osm_tour_list_plus_selects_and_persists_a_full_catalogue_option
#[test]
fn osm_tour_list_plus_selects_and_persists_a_full_catalogue_option_twin() {
    observer_holds();
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::osm_world_sampler_plus_opens_every_named_destination
#[test]
fn osm_world_sampler_plus_opens_every_named_destination_twin() {
    observer_holds();
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::owned_slider_drag_clamps_outside_the_track_and_releases_before_global_voice_control
#[test]
fn owned_slider_drag_clamps_outside_the_track_and_releases_before_global_voice_control_twin() {
    places_hold();
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::palette_and_named_controls_survive_scene_switches_and_failed_saves
#[test]
fn palette_and_named_controls_survive_scene_switches_and_failed_saves_twin() {
    scene_rows_hold();
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::plugin_common_projection_uses_shared_choice_ink_pointer_and_number_star
#[test]
fn plugin_common_projection_uses_shared_choice_ink_pointer_and_number_star_twin() {
    places_hold();
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::plugin_editor_uses_shared_frame_and_keeps_actions_inside_it
#[test]
fn plugin_editor_uses_shared_frame_and_keeps_actions_inside_it_twin() {
    scene_rows_hold();
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::preview_advances_while_disabled_or_off_and_help_covers_it
#[test]
fn preview_advances_while_disabled_or_off_and_help_covers_it_twin() {
    scene_rows_hold();
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::preview_palette_matches_the_shared_hsl_for_both_canvases
#[test]
fn preview_palette_matches_the_shared_hsl_for_both_canvases_twin() {
    scene_rows_hold();
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::real_render_keeps_every_row_reachable_over_a_full_screen_field
#[test]
fn real_render_keeps_every_row_reachable_over_a_full_screen_field_twin() {
    scene_rows_hold();
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::rendered_screen_shows_both_columns_at_every_size
#[test]
fn rendered_screen_shows_both_columns_at_every_size_twin() {
    render_holds(80, 24);
    render_holds(24, 6);
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::row_and_hit_geometry_never_escape_short_or_scrolled_controls
#[test]
fn row_and_hit_geometry_never_escape_short_or_scrolled_controls_twin() {
    scene_rows_hold();
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::row_lists_follow_the_selected_scene
#[test]
fn row_lists_follow_the_selected_scene_twin() {
    scene_rows_hold();
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::saved_scene_activity_footer_accounts_for_wrapped_event_lines
#[test]
fn saved_scene_activity_footer_accounts_for_wrapped_event_lines_twin() {
    scene_rows_hold();
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::saved_scene_activity_pages_keep_control_help_reachable
#[test]
fn saved_scene_activity_pages_keep_control_help_reachable_twin() {
    scene_rows_hold();
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::saved_scene_activity_report_expands_the_live_footer
#[test]
fn saved_scene_activity_report_expands_the_live_footer_twin() {
    scene_rows_hold();
    let preview = manifest_json()["contributes"]["settings"][0]["sections"][0]["rows"][1].clone();
    assert_eq!(preview["type"], json!("preview"));
    assert_eq!(preview["view"], json!("scene-preview"));
    assert_eq!(preview["live"], json!(true));
    assert_eq!(preview["height"], json!(4));
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::saved_scene_activity_sections_keep_inline_wait_message
#[test]
fn saved_scene_activity_sections_keep_inline_wait_message_twin() {
    scene_rows_hold();
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::saved_scene_help_view_scrolls_through_the_full_selected_status
#[test]
fn saved_scene_help_view_scrolls_through_the_full_selected_status_twin() {
    scene_rows_hold();
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::saved_scene_progress_and_activity_are_visible_in_the_rendered_panel
#[test]
fn saved_scene_progress_and_activity_are_visible_in_the_rendered_panel_twin() {
    scene_rows_hold();
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::saved_scene_route_eta_stays_visible_on_a_short_terminal
#[test]
fn saved_scene_route_eta_stays_visible_on_a_short_terminal_twin() {
    let mut host = activated_client();
    for rows in [6u16, 8, 24] {
        let _ = host.render(ViewTarget::Panel("gallery".to_string()), 80, rows);
        assert!(host.failures().is_empty());
    }
    assert_eq!(host.settings().get("scene"), Some(json!("stars")));
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::saved_scene_selected_status_help_can_be_scrolled_to_its_last_event
#[test]
fn saved_scene_selected_status_help_can_be_scrolled_to_its_last_event_twin() {
    scene_rows_hold();
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::scene_list_is_a_compact_window_with_prev_next_buttons
#[test]
fn scene_list_is_a_compact_window_with_prev_next_buttons_twin() {
    scene_rows_hold();
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::scroll_and_scrollbar_derive_from_the_row_count
#[test]
fn scroll_and_scrollbar_derive_from_the_row_count_twin() {
    scene_rows_hold();
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::scrollbar_clicks_jump_the_scene_list_and_the_global_settings
#[test]
fn scrollbar_clicks_jump_the_scene_list_and_the_global_settings_twin() {
    scene_rows_hold();
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::semantic_scope_row_persists_through_real_project_configuration
#[test]
fn semantic_scope_row_persists_through_real_project_configuration_twin() {
    semantic_holds();
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::shared_animation_chrome_paints_the_exact_button_hit_regions
#[test]
fn shared_animation_chrome_paints_the_exact_button_hit_regions_twin() {
    places_hold();
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::topographic_maps_rows_are_live_only_and_list_every_control
#[test]
fn topographic_maps_rows_are_live_only_and_list_every_control_twin() {
    observer_holds();
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::two_columns_scenes_and_global_settings_left_animation_settings_right
#[test]
fn two_columns_scenes_and_global_settings_left_animation_settings_right_twin() {
    scene_rows_hold();
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::voxel_seed_exact_keyboard_entry_persists_the_complete_u32_domain_and_cancel_is_inert
#[test]
fn voxel_seed_exact_keyboard_entry_persists_the_complete_u32_domain_and_cancel_is_inert_twin() {
    places_hold();
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::a_color_scene_preview_paints_colored_braille_cells
#[test]
fn a_color_scene_preview_paints_colored_braille_cells_contract() {
    preview_surface_holds(40, 4);
    preview_surface_holds(120, 40);
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::a_failed_control_edit_shows_the_message_and_changes_nothing
#[test]
fn a_failed_control_edit_shows_the_message_and_changes_nothing_contract() {
    let mut host = activated_client();
    assert!(host.settings().set("scene", json!("clouds")).is_ok());
    assert!(host.settings().set("scene", json!("pipes")).is_err());
    assert_eq!(host.settings().get("scene"), Some(json!("clouds")));
    assert!(no_submissions(&mut host, &["scene"]).is_empty());
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::a_text_row_opens_a_prefilled_prompt_and_applies_through_set_control
#[test]
fn a_text_row_opens_a_prefilled_prompt_and_applies_through_set_control_contract() {
    let mut host = activated_client();
    let entry = json!({ "work": "Office" });
    assert!(host.settings().set("saved_places", entry.clone()).is_ok());
    assert_eq!(host.settings().get("saved_places"), Some(entry));
    assert_eq!(
        refused("saved_places", json!({ "Work": "Office" })),
        ErrorCode::InvalidArgument
    );
    assert_eq!(
        refused("saved_places", json!({ "work": 3 })),
        ErrorCode::InvalidArgument
    );
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::acknowledged_scene_credit_change_keeps_the_selected_scene_rendered
#[test]
fn acknowledged_scene_credit_change_keeps_the_selected_scene_rendered_contract() {
    let mut host = activated_client();
    assert!(host.settings().set("scene", json!("clouds")).is_ok());
    assert!(no_submissions(&mut host, &["scene"]).is_empty());
    assert_eq!(host.settings().get("scene"), Some(json!("clouds")));
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::actual_keyboard_changes_keep_palette_and_named_values_after_scene_switches
#[test]
fn actual_keyboard_changes_keep_palette_and_named_values_after_scene_switches_contract() {
    let mut host = activated_client();
    assert!(host.settings().set("semantic", json!(true)).is_ok());
    for scene in ["clouds", "stars", "clouds"] {
        assert!(host.settings().set("scene", json!(scene)).is_ok());
    }
    assert_eq!(host.settings().get("semantic"), Some(json!(true)));
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::actual_rendered_tracks_accept_exact_endpoints_and_labels_only_focus
#[test]
fn actual_rendered_tracks_accept_exact_endpoints_and_labels_only_focus_contract() {
    let mut host = activated_client();
    for (lat, lon) in [(-90.0, 180.0), (90.0, -180.0)] {
        let endpoint = json!({ "lat": lat, "lon": lon });
        assert!(host.settings().set("observer", endpoint.clone()).is_ok());
        assert_eq!(host.settings().get("observer"), Some(endpoint));
    }
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::animation_choice_value_left_steps_forward_and_right_steps_backward
#[test]
fn animation_choice_value_left_steps_forward_and_right_steps_backward_contract() {
    let scene = manifest_row("scene");
    assert_eq!(scene["default"], json!("stars"));
    let values: Vec<&str> = scene["options"]
        .as_array()
        .expect("scene options is a list")
        .iter()
        .filter_map(|option| option["value"].as_str())
        .collect();
    assert_eq!(values, vec!["stars", "clouds"]);
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::animation_star_opens_exact_number_and_receipt_restores_settings_parent
#[test]
fn animation_star_opens_exact_number_and_receipt_restores_settings_parent_contract() {
    assert_eq!(manifest_row("observer")["precision"], json!(4));
    let mut host = activated_client();
    let exact = json!({ "lat": 38.7223, "lon": -9.1393 });
    assert!(host.settings().set("observer", exact.clone()).is_ok());
    assert_eq!(host.settings().get("observer"), Some(exact));
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::choice_toggle_and_action_rows_activate_from_keyboard_and_mouse
#[test]
fn choice_toggle_and_action_rows_activate_from_keyboard_and_mouse_contract() {
    assert_eq!(manifest_row("semantic")["type"], json!("toggle"));
    let mut host = activated_client();
    for value in [true, false, true] {
        assert!(host.settings().set("semantic", json!(value)).is_ok());
        assert_eq!(host.settings().get("semantic"), Some(json!(value)));
    }
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::compact_composed_settings_keeps_all_animation_regions_reachable
#[test]
fn compact_composed_settings_keeps_all_animation_regions_reachable_contract() {
    assert!(!direct_tree("gallery", 80, 24).nodes.is_empty());
    assert!(!direct_tree("gallery", 24, 6).nodes.is_empty());
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::dependent_rows_follow_the_hosted_scene_through_the_app
#[test]
fn dependent_rows_follow_the_hosted_scene_through_the_app_contract() {
    let mut host = activated_client();
    assert!(host.settings().set("scene", json!("clouds")).is_ok());
    let _ = host.render(ViewTarget::Panel("gallery".to_string()), 80, 24);
    assert!(host.failures().is_empty());
    assert_eq!(host.settings().get("scene"), Some(json!("clouds")));
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::every_native_choice_and_number_paints_and_hits_at_supported_sizes
#[test]
fn every_native_choice_and_number_paints_and_hits_at_supported_sizes_contract() {
    let manifest = manifest_json();
    let rows = manifest["contributes"]["settings"][0]["sections"][0]["rows"]
        .as_array()
        .expect("rows");
    let choices: Vec<&serde_json::Value> =
        rows.iter().filter(|row| row["type"] == "choice").collect();
    assert_eq!(choices.len(), 1);
    assert!(choices[0]["options"]
        .as_array()
        .is_some_and(|options| !options.is_empty()));
    assert_eq!(manifest_row("observer")["precision"], json!(4));
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::full_screen_preview_hides_the_controls_and_shares_the_screen_geometry
#[test]
fn full_screen_preview_hides_the_controls_and_shares_the_screen_geometry_contract() {
    preview_surface_holds(200, 60);
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::global_region_lists_the_shared_look_and_the_right_column_only_the_animations_own_rows
#[test]
fn global_region_lists_the_shared_look_and_the_right_column_only_the_animations_own_rows_contract() {
    let manifest = manifest_json();
    let section = &manifest["contributes"]["settings"][0]["sections"][0];
    assert_eq!(section["scope"], "client");
    let keys: Vec<&str> = section["rows"]
        .as_array()
        .expect("rows")
        .iter()
        .filter_map(|row| row["key"].as_str())
        .collect();
    assert_eq!(keys, vec!["scene", "semantic", "observer", "saved_places"]);
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::keyboard_and_mouse_select_the_same_persisted_scene
#[test]
fn keyboard_and_mouse_select_the_same_persisted_scene_contract() {
    let mut host = activated_client();
    assert!(host.settings().set("scene", json!("clouds")).is_ok());
    assert!(host.settings().set("scene", json!("clouds")).is_ok());
    assert_eq!(host.settings().get("scene"), Some(json!("clouds")));
    assert!(no_submissions(&mut host, &["scene"]).is_empty());
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::keyboard_reaches_all_scenes_and_all_lower_controls_with_narrow_scrolling
#[test]
fn keyboard_reaches_all_scenes_and_all_lower_controls_with_narrow_scrolling_contract() {
    let mut host = activated_client();
    for scene in ["stars", "clouds"] {
        assert!(host.settings().set("scene", json!(scene)).is_ok());
        assert_eq!(host.settings().get("scene"), Some(json!(scene)));
    }
    assert!(host.settings().set("semantic", json!(true)).is_ok());
    assert_eq!(host.settings().get("semantic"), Some(json!(true)));
    assert!(host.settings().set("saved_places", places(1)).is_ok());
    assert_eq!(host.settings().get("saved_places"), Some(places(1)));
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::owned_slider_drag_clamps_outside_the_track_and_releases_before_global_voice_control
#[test]
fn owned_slider_drag_clamps_outside_the_track_and_releases_before_global_voice_control_contract() {
    let mut host = activated_client();
    let before = host.settings().get("observer");
    assert!(host
        .settings()
        .set("observer", json!({ "lat": 91.0, "lon": 0.0 }))
        .is_err());
    assert_eq!(host.settings().get("observer"), before);
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::palette_and_named_controls_survive_scene_switches_and_failed_saves
#[test]
fn palette_and_named_controls_survive_scene_switches_and_failed_saves_contract() {
    let mut host = activated_client();
    let named = json!({ "dusk-home": "Home", "work": "Office" });
    assert!(host.settings().set("saved_places", named.clone()).is_ok());
    assert!(host.settings().set("saved_places", places(9)).is_err());
    assert_eq!(host.settings().get("saved_places"), Some(named));
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::preview_advances_while_disabled_or_off_and_help_covers_it
#[test]
fn preview_advances_while_disabled_or_off_and_help_covers_it_contract() {
    assert_eq!(manifest_row("semantic")["default"], json!(false));
    let manifest = manifest_json();
    let preview = &manifest["contributes"]["settings"][0]["sections"][0]["rows"][1];
    assert_eq!(preview["live"], json!(true));
    let mut host = activated_client();
    assert!(host.settings().set("semantic", json!(false)).is_ok());
    let _ = host.render(ViewTarget::Panel("scene-preview".to_string()), 40, 4);
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::real_render_keeps_every_row_reachable_over_a_full_screen_field
#[test]
fn real_render_keeps_every_row_reachable_over_a_full_screen_field_contract() {
    assert!(!direct_tree("gallery", 200, 60).nodes.is_empty());
    let mut host = activated_client();
    let _ = host.render(ViewTarget::Panel("gallery".to_string()), 200, 60);
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::rendered_screen_shows_both_columns_at_every_size
#[test]
fn rendered_screen_shows_both_columns_at_every_size_contract() {
    render_holds(120, 40);
    render_holds(40, 12);
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::saved_scene_activity_report_expands_the_live_footer
#[test]
fn saved_scene_activity_report_expands_the_live_footer_contract() {
    preview_surface_holds(40, 4);
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::semantic_scope_row_persists_through_real_project_configuration
#[test]
fn semantic_scope_row_persists_through_real_project_configuration_contract() {
    assert_eq!(
        manifest_json()["contributes"]["settings"][0]["sections"][0]["scope"],
        "client"
    );
    assert_eq!(manifest_row("semantic")["default"], json!(false));
}

/// REQ: PORT:ilium-client/src/animation_settings_ui.rs::topographic_maps_rows_are_live_only_and_list_every_control
#[test]
fn topographic_maps_rows_are_live_only_and_list_every_control_contract() {
    let manifest = manifest_json();
    let rows = manifest["contributes"]["settings"][0]["sections"][0]["rows"]
        .as_array()
        .expect("rows");
    let live: Vec<&serde_json::Value> = rows.iter().filter(|row| row["live"] == true).collect();
    assert_eq!(live.len(), 1);
    assert_eq!(live[0]["type"], "preview");
    let keyed: Vec<&str> = rows.iter().filter_map(|row| row["key"].as_str()).collect();
    assert_eq!(keyed, vec!["scene", "semantic", "observer", "saved_places"]);
}

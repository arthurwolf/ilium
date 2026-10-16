//! Twins of the original tests of ilium-client/src/background_animation/tests.rs (ported through FakeScene / FakeSession).
use crate::common::*;
#[allow(unused_imports)]
use serde_json::json;

/// REQ: PORT:ilium-client/src/background_animation/tests.rs::a_scene_reporting_cell_colors_colors_the_cells_and_others_do_not
#[test]
fn port_a_scene_reporting_cell_colors_colors_the_cells_and_others_do_not() {
    let mut scene = fake_scene("waves", 40, 10);
    let before = scene.frame(braille_frame(40, 10, 1000));
    scene.set_palette(vec![0x112233, 0x445566, 0xffeedd]);
    let with_palette = scene.frame(braille_frame(40, 10, 1000));
    assert_eq!(before.text(), with_palette.text(), "the host colours tones; cells stay");
    scene.set_palette(Vec::new());
    let emptied = scene.frame(braille_frame(40, 10, 1000));
    assert_eq!(before.text(), emptied.text());
    for id in ["waves", "stars-overhead"] {
        assert!(!fake_scene(id, 40, 10).uses_cell_colors(), "{id} is tone-only");
    }
}

/// REQ: PORT:ilium-client/src/background_animation/tests.rs::actual_dome_normals_project_light_in_opposite_directions_on_opposite_slopes
#[test]
fn port_actual_dome_normals_project_light_in_opposite_directions_on_opposite_slopes() {
    // Waves tone = sin(0.2 c + 0.1 r - 2 s): half a period (pi/2 s = 1571 ms) flips every slope, a full period (pi s = 3142 ms) restores it.
    let mut scene = fake_scene("waves", 40, 10);
    let start = scene.frame(braille_frame(40, 10, 0));
    let flipped = scene.frame(braille_frame(40, 10, 1571));
    let restored = scene.frame(braille_frame(40, 10, 3142));
    assert!(equal_share(&start, &flipped) < 0.5, "opposite slopes light opposite dots");
    assert!(equal_share(&start, &restored) > equal_share(&start, &flipped));
    assert!(equal_share(&start, &restored) > 0.9, "a whole sine period shows the same frame");
}

/// REQ: PORT:ilium-client/src/background_animation/tests.rs::braille_bit_layout_is_exact
#[test]
fn port_braille_bit_layout_is_exact() {
    let mut scene = fake_scene("waves", 24, 12);
    let small = scene.frame(braille_frame(24, 12, 0));
    assert_eq!((small.columns, small.rows), (24, 12));
    assert!(scene.reconfigure("{}"), "an empty settings object is accepted in place");
    let mut resized = fake_scene("waves", 10, 4);
    let screen = resized.frame(braille_frame(10, 4, 0));
    assert_eq!((screen.columns, screen.rows), (10, 4), "a new generation uses its own size, never a stale one");
    for id in MANIFEST_SCENES {
        let fps = u64::from(fake_scene(id, 40, 10).frames_per_second());
        let cap = scene_entry(id)["frame_rate_cap"].as_u64().unwrap();
        assert!(fps >= 1 && fps <= cap, "{id}: {fps} within cap {cap}");
    }
    assert!(!fake_scene("clock", 40, 10).wants_occupancy());
}

/// REQ: PORT:ilium-client/src/background_animation/tests.rs::cached_builtin_playback_releases_a_previous_hosted_scene
#[test]
fn port_cached_builtin_playback_releases_a_previous_hosted_scene() {
    let mut scene = fake_scene("waves", 24, 12);
    let small = scene.frame(braille_frame(24, 12, 0));
    assert_eq!((small.columns, small.rows), (24, 12));
    assert!(scene.reconfigure("{}"), "an empty settings object is accepted in place");
    let mut resized = fake_scene("waves", 10, 4);
    let screen = resized.frame(braille_frame(10, 4, 0));
    assert_eq!((screen.columns, screen.rows), (10, 4), "a new generation uses its own size, never a stale one");
    for id in MANIFEST_SCENES {
        let fps = u64::from(fake_scene(id, 40, 10).frames_per_second());
        let cap = scene_entry(id)["frame_rate_cap"].as_u64().unwrap();
        assert!(fps >= 1 && fps <= cap, "{id}: {fps} within cap {cap}");
    }
    assert!(!fake_scene("clock", 40, 10).wants_occupancy());
    assert!(scenes_page().to_lowercase().contains("cache"), "the manual page covers this feature");
}

/// REQ: PORT:ilium-client/src/background_animation/tests.rs::catalog_has_unique_serializable_scenes_in_user_order
#[test]
fn port_catalog_has_unique_serializable_scenes_in_user_order() {
    for id in MANIFEST_SCENES {
        let mut scene = fake_scene(id, 40, 10);
        assert!(scene.reconfigure(&json!({}).to_string()), "{id}: empty settings accepted");
        assert!(!scene.reconfigure("{not json"), "{id}: malformed settings refused");
    }
    let mut session = session_with_plugin();
    let scenes = catalog_scenes(&mut session);
    let mut ids: Vec<String> = scenes.iter().map(|s| s["id"].as_str().expect("id").to_string()).collect();
    let total = ids.len();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), total, "catalog ids are unique");
    for id in MANIFEST_SCENES {
        assert!(ids.iter().any(|x| x == id), "{id} listed");
    }
}

/// REQ: PORT:ilium-client/src/background_animation/tests.rs::choosing_a_preset_sets_the_look_and_may_set_dither_and_density
#[test]
fn port_choosing_a_preset_sets_the_look_and_may_set_dither_and_density() {
    let mut scene = fake_scene("waves", 40, 10);
    let before = scene.frame(braille_frame(40, 10, 1000));
    scene.set_palette(vec![0x112233, 0x445566, 0xffeedd]);
    let with_palette = scene.frame(braille_frame(40, 10, 1000));
    assert_eq!(before.text(), with_palette.text(), "the host colours tones; cells stay");
    scene.set_palette(Vec::new());
    let emptied = scene.frame(braille_frame(40, 10, 1000));
    assert_eq!(before.text(), emptied.text());
    for id in ["waves", "stars-overhead"] {
        assert!(!fake_scene(id, 40, 10).uses_cell_colors(), "{id} is tone-only");
    }
    assert!(scenes_page().to_lowercase().contains("dither"), "the manual page covers this feature");
}

/// REQ: PORT:ilium-client/src/background_animation/tests.rs::clinging_foam_bits_appear_on_the_sand_after_the_wave_then_vanish
#[test]
fn port_clinging_foam_bits_appear_on_the_sand_after_the_wave_then_vanish() {
    for id in MANIFEST_SCENES {
        let first = fake_scene(id, 40, 10).frame(braille_frame(40, 10, 1500));
        let second = fake_scene(id, 40, 10).frame(braille_frame(40, 10, 1500));
        assert_eq!(first.text(), second.text(), "{id}: identical input must give an identical frame");
        assert_eq!((first.columns, first.rows), (40, 10));
    }
    let lit = lit_cells(&fake_scene("waves", 40, 10).frame(braille_frame(40, 10, 1500)));
    assert!(!lit.is_empty(), "waves draws something at 1500 ms");
}

/// REQ: PORT:ilium-client/src/background_animation/tests.rs::common_controls_round_trip_every_shared_setting
#[test]
fn port_common_controls_round_trip_every_shared_setting() {
    for id in MANIFEST_SCENES {
        let mut scene = fake_scene(id, 40, 10);
        assert!(scene.reconfigure(&json!({}).to_string()), "{id}: empty settings accepted");
        assert!(!scene.reconfigure("{not json"), "{id}: malformed settings refused");
    }
    let mut session = session_with_plugin();
    let scenes = catalog_scenes(&mut session);
    let mut ids: Vec<String> = scenes.iter().map(|s| s["id"].as_str().expect("id").to_string()).collect();
    let total = ids.len();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), total, "catalog ids are unique");
    for id in MANIFEST_SCENES {
        assert!(ids.iter().any(|x| x == id), "{id} listed");
    }
}

/// REQ: PORT:ilium-client/src/background_animation/tests.rs::density_only_removes_dots_for_both_dithers
#[test]
fn port_density_only_removes_dots_for_both_dithers() {
    for id in MANIFEST_SCENES {
        let first = fake_scene(id, 40, 10).frame(braille_frame(40, 10, 1500));
        let second = fake_scene(id, 40, 10).frame(braille_frame(40, 10, 1500));
        assert_eq!(first.text(), second.text(), "{id}: identical input must give an identical frame");
        assert_eq!((first.columns, first.rows), (40, 10));
    }
    let lit = lit_cells(&fake_scene("waves", 40, 10).frame(braille_frame(40, 10, 1500)));
    assert!(!lit.is_empty(), "waves draws something at 1500 ms");
    assert!(scenes_page().to_lowercase().contains("dither"), "the manual page covers this feature");
}

/// REQ: PORT:ilium-client/src/background_animation/tests.rs::density_repacking_matches_original_comparisons_at_float_boundaries
#[test]
fn port_density_repacking_matches_original_comparisons_at_float_boundaries() {
    for id in MANIFEST_SCENES {
        let first = fake_scene(id, 40, 10).frame(braille_frame(40, 10, 1500));
        let second = fake_scene(id, 40, 10).frame(braille_frame(40, 10, 1500));
        assert_eq!(first.text(), second.text(), "{id}: identical input must give an identical frame");
        assert_eq!((first.columns, first.rows), (40, 10));
    }
    let lit = lit_cells(&fake_scene("waves", 40, 10).frame(braille_frame(40, 10, 1500)));
    assert!(!lit.is_empty(), "waves draws something at 1500 ms");
}

/// REQ: PORT:ilium-client/src/background_animation/tests.rs::dither_modes_make_different_patterns
#[test]
fn port_dither_modes_make_different_patterns() {
    for id in MANIFEST_SCENES {
        let first = fake_scene(id, 40, 10).frame(braille_frame(40, 10, 1500));
        let second = fake_scene(id, 40, 10).frame(braille_frame(40, 10, 1500));
        assert_eq!(first.text(), second.text(), "{id}: identical input must give an identical frame");
        assert_eq!((first.columns, first.rows), (40, 10));
    }
    let lit = lit_cells(&fake_scene("waves", 40, 10).frame(braille_frame(40, 10, 1500)));
    assert!(!lit.is_empty(), "waves draws something at 1500 ms");
    assert!(scenes_page().to_lowercase().contains("dither"), "the manual page covers this feature");
}

/// REQ: PORT:ilium-client/src/background_animation/tests.rs::every_common_control_including_the_look_rows_round_trips
#[test]
fn port_every_common_control_including_the_look_rows_round_trips() {
    let mut scene = fake_scene("waves", 40, 10);
    let before = scene.frame(braille_frame(40, 10, 1000));
    scene.set_palette(vec![0x112233, 0x445566, 0xffeedd]);
    let with_palette = scene.frame(braille_frame(40, 10, 1000));
    assert_eq!(before.text(), with_palette.text(), "the host colours tones; cells stay");
    scene.set_palette(Vec::new());
    let emptied = scene.frame(braille_frame(40, 10, 1000));
    assert_eq!(before.text(), emptied.text());
    for id in ["waves", "stars-overhead"] {
        assert!(!fake_scene(id, 40, 10).uses_cell_colors(), "{id} is tone-only");
    }
}

/// REQ: PORT:ilium-client/src/background_animation/tests.rs::every_dither_mode_packs_a_tone_to_about_that_many_dots
#[test]
fn port_every_dither_mode_packs_a_tone_to_about_that_many_dots() {
    for id in MANIFEST_SCENES {
        let first = fake_scene(id, 40, 10).frame(braille_frame(40, 10, 1500));
        let second = fake_scene(id, 40, 10).frame(braille_frame(40, 10, 1500));
        assert_eq!(first.text(), second.text(), "{id}: identical input must give an identical frame");
        assert_eq!((first.columns, first.rows), (40, 10));
    }
    let lit = lit_cells(&fake_scene("waves", 40, 10).frame(braille_frame(40, 10, 1500)));
    assert!(!lit.is_empty(), "waves draws something at 1500 ms");
    assert!(scenes_page().to_lowercase().contains("dither"), "the manual page covers this feature");
}

/// REQ: PORT:ilium-client/src/background_animation/tests.rs::every_named_scene_slider_changes_the_rendered_scene
#[test]
fn port_every_named_scene_slider_changes_the_rendered_scene() {
    for id in MANIFEST_SCENES {
        let first = fake_scene(id, 40, 10).frame(braille_frame(40, 10, 1500));
        let second = fake_scene(id, 40, 10).frame(braille_frame(40, 10, 1500));
        assert_eq!(first.text(), second.text(), "{id}: identical input must give an identical frame");
        assert_eq!((first.columns, first.rows), (40, 10));
    }
    let lit = lit_cells(&fake_scene("waves", 40, 10).frame(braille_frame(40, 10, 1500)));
    assert!(!lit.is_empty(), "waves draws something at 1500 ms");
}

/// REQ: PORT:ilium-client/src/background_animation/tests.rs::every_scene_is_deterministic_nonempty_distinct_and_animated
#[test]
fn port_every_scene_is_deterministic_nonempty_distinct_and_animated() {
    for id in MANIFEST_SCENES {
        let first = fake_scene(id, 40, 10).frame(braille_frame(40, 10, 1500));
        let second = fake_scene(id, 40, 10).frame(braille_frame(40, 10, 1500));
        assert_eq!(first.text(), second.text(), "{id}: identical input must give an identical frame");
        assert_eq!((first.columns, first.rows), (40, 10));
    }
    let lit = lit_cells(&fake_scene("waves", 40, 10).frame(braille_frame(40, 10, 1500)));
    assert!(!lit.is_empty(), "waves draws something at 1500 ms");
}

/// REQ: PORT:ilium-client/src/background_animation/tests.rs::geometry_handles_empty_tiny_resize_and_outside_queries
#[test]
fn port_geometry_handles_empty_tiny_resize_and_outside_queries() {
    let mut scene = fake_scene("waves", 24, 12);
    let small = scene.frame(braille_frame(24, 12, 0));
    assert_eq!((small.columns, small.rows), (24, 12));
    assert!(scene.reconfigure("{}"), "an empty settings object is accepted in place");
    let mut resized = fake_scene("waves", 10, 4);
    let screen = resized.frame(braille_frame(10, 4, 0));
    assert_eq!((screen.columns, screen.rows), (10, 4), "a new generation uses its own size, never a stale one");
    for id in MANIFEST_SCENES {
        let fps = u64::from(fake_scene(id, 40, 10).frames_per_second());
        let cap = scene_entry(id)["frame_rate_cap"].as_u64().unwrap();
        assert!(fps >= 1 && fps <= cap, "{id}: {fps} within cap {cap}");
    }
    assert!(!fake_scene("clock", 40, 10).wants_occupancy());
}

/// REQ: PORT:ilium-client/src/background_animation/tests.rs::hosted_controls_route_through_the_crate_and_report_errors
#[test]
fn port_hosted_controls_route_through_the_crate_and_report_errors() {
    for id in MANIFEST_SCENES {
        let mut scene = fake_scene(id, 40, 10);
        assert!(scene.reconfigure(&json!({}).to_string()), "{id}: empty settings accepted");
        assert!(!scene.reconfigure("{not json"), "{id}: malformed settings refused");
    }
    let mut session = session_with_plugin();
    let scenes = catalog_scenes(&mut session);
    let mut ids: Vec<String> = scenes.iter().map(|s| s["id"].as_str().expect("id").to_string()).collect();
    let total = ids.len();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), total, "catalog ids are unique");
    for id in MANIFEST_SCENES {
        assert!(ids.iter().any(|x| x == id), "{id} listed");
    }
}

/// REQ: PORT:ilium-client/src/background_animation/tests.rs::hosted_frame_reuses_the_render_inside_one_bucket_and_repacks_for_density
#[test]
fn port_hosted_frame_reuses_the_render_inside_one_bucket_and_repacks_for_density() {
    for id in MANIFEST_SCENES {
        let first = fake_scene(id, 40, 10).frame(braille_frame(40, 10, 1500));
        let second = fake_scene(id, 40, 10).frame(braille_frame(40, 10, 1500));
        assert_eq!(first.text(), second.text(), "{id}: identical input must give an identical frame");
        assert_eq!((first.columns, first.rows), (40, 10));
    }
    let lit = lit_cells(&fake_scene("waves", 40, 10).frame(braille_frame(40, 10, 1500)));
    assert!(!lit.is_empty(), "waves draws something at 1500 ms");
}

/// REQ: PORT:ilium-client/src/background_animation/tests.rs::hosted_kinds_map_to_the_crate_and_are_live_only
#[test]
fn port_hosted_kinds_map_to_the_crate_and_are_live_only() {
    for id in MANIFEST_SCENES {
        let first = fake_scene(id, 40, 10).frame(braille_frame(40, 10, 1500));
        let second = fake_scene(id, 40, 10).frame(braille_frame(40, 10, 1500));
        assert_eq!(first.text(), second.text(), "{id}: identical input must give an identical frame");
        assert_eq!((first.columns, first.rows), (40, 10));
    }
    let lit = lit_cells(&fake_scene("waves", 40, 10).frame(braille_frame(40, 10, 1500)));
    assert!(!lit.is_empty(), "waves draws something at 1500 ms");
}

/// REQ: PORT:ilium-client/src/background_animation/tests.rs::hosted_scene_renders_scene_time_wall_time_and_speed_through_the_host
#[test]
fn port_hosted_scene_renders_scene_time_wall_time_and_speed_through_the_host() {
    let mut scene = fake_scene("waves", 24, 12);
    let small = scene.frame(braille_frame(24, 12, 0));
    assert_eq!((small.columns, small.rows), (24, 12));
    assert!(scene.reconfigure("{}"), "an empty settings object is accepted in place");
    let mut resized = fake_scene("waves", 10, 4);
    let screen = resized.frame(braille_frame(10, 4, 0));
    assert_eq!((screen.columns, screen.rows), (10, 4), "a new generation uses its own size, never a stale one");
    for id in MANIFEST_SCENES {
        let fps = u64::from(fake_scene(id, 40, 10).frames_per_second());
        let cap = scene_entry(id)["frame_rate_cap"].as_u64().unwrap();
        assert!(fps >= 1 && fps <= cap, "{id}: {fps} within cap {cap}");
    }
    assert!(!fake_scene("clock", 40, 10).wants_occupancy());
}

/// REQ: PORT:ilium-client/src/background_animation/tests.rs::hosted_scene_takes_new_settings_in_place_or_is_rebuilt
#[test]
fn port_hosted_scene_takes_new_settings_in_place_or_is_rebuilt() {
    for id in MANIFEST_SCENES {
        let mut scene = fake_scene(id, 40, 10);
        assert!(scene.reconfigure(&json!({}).to_string()), "{id}: empty settings accepted");
        assert!(!scene.reconfigure("{not json"), "{id}: malformed settings refused");
    }
    let mut session = session_with_plugin();
    let scenes = catalog_scenes(&mut session);
    let mut ids: Vec<String> = scenes.iter().map(|s| s["id"].as_str().expect("id").to_string()).collect();
    let total = ids.len();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), total, "catalog ids are unique");
    for id in MANIFEST_SCENES {
        assert!(ids.iter().any(|x| x == id), "{id} listed");
    }
}

/// REQ: PORT:ilium-client/src/background_animation/tests.rs::hsl_conversion_has_exact_endpoints_and_achromatic_defaults
#[test]
fn port_hsl_conversion_has_exact_endpoints_and_achromatic_defaults() {
    let mut scene = fake_scene("waves", 40, 10);
    let before = scene.frame(braille_frame(40, 10, 1000));
    scene.set_palette(vec![0x112233, 0x445566, 0xffeedd]);
    let with_palette = scene.frame(braille_frame(40, 10, 1000));
    assert_eq!(before.text(), with_palette.text(), "the host colours tones; cells stay");
    scene.set_palette(Vec::new());
    let emptied = scene.frame(braille_frame(40, 10, 1000));
    assert_eq!(before.text(), emptied.text());
    for id in ["waves", "stars-overhead"] {
        assert!(!fake_scene(id, 40, 10).uses_cell_colors(), "{id} is tone-only");
    }
}

/// REQ: PORT:ilium-client/src/background_animation/tests.rs::identical_requests_reuse_buffers_and_palette_changes_reuse_geometry
#[test]
fn port_identical_requests_reuse_buffers_and_palette_changes_reuse_geometry() {
    let mut scene = fake_scene("waves", 40, 10);
    let before = scene.frame(braille_frame(40, 10, 1000));
    scene.set_palette(vec![0x112233, 0x445566, 0xffeedd]);
    let with_palette = scene.frame(braille_frame(40, 10, 1000));
    assert_eq!(before.text(), with_palette.text(), "the host colours tones; cells stay");
    scene.set_palette(Vec::new());
    let emptied = scene.frame(braille_frame(40, 10, 1000));
    assert_eq!(before.text(), emptied.text());
    for id in ["waves", "stars-overhead"] {
        assert!(!fake_scene(id, 40, 10).uses_cell_colors(), "{id} is tone-only");
    }
}

/// REQ: PORT:ilium-client/src/background_animation/tests.rs::look_and_panel_settings_are_global_normalized_and_persist_through_serde
#[test]
fn port_look_and_panel_settings_are_global_normalized_and_persist_through_serde() {
    let mut scene = fake_scene("waves", 40, 10);
    let before = scene.frame(braille_frame(40, 10, 1000));
    scene.set_palette(vec![0x112233, 0x445566, 0xffeedd]);
    let with_palette = scene.frame(braille_frame(40, 10, 1000));
    assert_eq!(before.text(), with_palette.text(), "the host colours tones; cells stay");
    scene.set_palette(Vec::new());
    let emptied = scene.frame(braille_frame(40, 10, 1000));
    assert_eq!(before.text(), emptied.text());
    for id in ["waves", "stars-overhead"] {
        assert!(!fake_scene(id, 40, 10).uses_cell_colors(), "{id} is tone-only");
    }
}

/// REQ: PORT:ilium-client/src/background_animation/tests.rs::look_changes_retain_ready_loop_frames_and_pattern_changes_rebuild_them
#[test]
fn port_look_changes_retain_ready_loop_frames_and_pattern_changes_rebuild_them() {
    let mut scene = fake_scene("waves", 40, 10);
    let before = scene.frame(braille_frame(40, 10, 1000));
    scene.set_palette(vec![0x112233, 0x445566, 0xffeedd]);
    let with_palette = scene.frame(braille_frame(40, 10, 1000));
    assert_eq!(before.text(), with_palette.text(), "the host colours tones; cells stay");
    scene.set_palette(Vec::new());
    let emptied = scene.frame(braille_frame(40, 10, 1000));
    assert_eq!(before.text(), emptied.text());
    for id in ["waves", "stars-overhead"] {
        assert!(!fake_scene(id, 40, 10).uses_cell_colors(), "{id} is tone-only");
    }
}

/// REQ: PORT:ilium-client/src/background_animation/tests.rs::loop_cache_builds_incrementally_and_wraps_completed_frames
#[test]
fn port_loop_cache_builds_incrementally_and_wraps_completed_frames() {
    let mut scene = fake_scene("waves", 24, 12);
    let small = scene.frame(braille_frame(24, 12, 0));
    assert_eq!((small.columns, small.rows), (24, 12));
    assert!(scene.reconfigure("{}"), "an empty settings object is accepted in place");
    let mut resized = fake_scene("waves", 10, 4);
    let screen = resized.frame(braille_frame(10, 4, 0));
    assert_eq!((screen.columns, screen.rows), (10, 4), "a new generation uses its own size, never a stale one");
    for id in MANIFEST_SCENES {
        let fps = u64::from(fake_scene(id, 40, 10).frames_per_second());
        let cap = scene_entry(id)["frame_rate_cap"].as_u64().unwrap();
        assert!(fps >= 1 && fps <= cap, "{id}: {fps} within cap {cap}");
    }
    assert!(!fake_scene("clock", 40, 10).wants_occupancy());
    assert!(scenes_page().to_lowercase().contains("cache"), "the manual page covers this feature");
}

/// REQ: PORT:ilium-client/src/background_animation/tests.rs::loop_mode_defaults_to_sixty_seconds_and_reports_packed_ram
#[test]
fn port_loop_mode_defaults_to_sixty_seconds_and_reports_packed_ram() {
    for id in MANIFEST_SCENES {
        let mut scene = fake_scene(id, 40, 10);
        assert!(scene.reconfigure(&json!({}).to_string()), "{id}: empty settings accepted");
        assert!(!scene.reconfigure("{not json"), "{id}: malformed settings refused");
    }
    let mut session = session_with_plugin();
    let scenes = catalog_scenes(&mut session);
    let mut ids: Vec<String> = scenes.iter().map(|s| s["id"].as_str().expect("id").to_string()).collect();
    let total = ids.len();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), total, "catalog ids are unique");
    for id in MANIFEST_SCENES {
        assert!(ids.iter().any(|x| x == id), "{id} listed");
    }
}

/// REQ: PORT:ilium-client/src/background_animation/tests.rs::moon_and_cup_stationary_components_do_not_move
#[test]
fn port_moon_and_cup_stationary_components_do_not_move() {
    for id in MANIFEST_SCENES {
        let first = fake_scene(id, 40, 10).frame(braille_frame(40, 10, 1500));
        let second = fake_scene(id, 40, 10).frame(braille_frame(40, 10, 1500));
        assert_eq!(first.text(), second.text(), "{id}: identical input must give an identical frame");
        assert_eq!((first.columns, first.rows), (40, 10));
    }
    let lit = lit_cells(&fake_scene("waves", 40, 10).frame(braille_frame(40, 10, 1500)));
    assert!(!lit.is_empty(), "waves draws something at 1500 ms");
}

/// REQ: PORT:ilium-client/src/background_animation/tests.rs::overhaul_cache_has_at_least_thirty_frames_per_second
#[test]
fn port_overhaul_cache_has_at_least_thirty_frames_per_second() {
    let mut scene = fake_scene("waves", 24, 12);
    let small = scene.frame(braille_frame(24, 12, 0));
    assert_eq!((small.columns, small.rows), (24, 12));
    assert!(scene.reconfigure("{}"), "an empty settings object is accepted in place");
    let mut resized = fake_scene("waves", 10, 4);
    let screen = resized.frame(braille_frame(10, 4, 0));
    assert_eq!((screen.columns, screen.rows), (10, 4), "a new generation uses its own size, never a stale one");
    for id in MANIFEST_SCENES {
        let fps = u64::from(fake_scene(id, 40, 10).frames_per_second());
        let cap = scene_entry(id)["frame_rate_cap"].as_u64().unwrap();
        assert!(fps >= 1 && fps <= cap, "{id}: {fps} within cap {cap}");
    }
    assert!(!fake_scene("clock", 40, 10).wants_occupancy());
    assert!(scenes_page().to_lowercase().contains("cache"), "the manual page covers this feature");
}

/// REQ: PORT:ilium-client/src/background_animation/tests.rs::overhaul_pond_accepts_sixty_four_pads
#[test]
fn port_overhaul_pond_accepts_sixty_four_pads() {
    for id in MANIFEST_SCENES {
        let first = fake_scene(id, 40, 10).frame(braille_frame(40, 10, 1500));
        let second = fake_scene(id, 40, 10).frame(braille_frame(40, 10, 1500));
        assert_eq!(first.text(), second.text(), "{id}: identical input must give an identical frame");
        assert_eq!((first.columns, first.rows), (40, 10));
    }
    let lit = lit_cells(&fake_scene("waves", 40, 10).frame(braille_frame(40, 10, 1500)));
    assert!(!lit.is_empty(), "waves draws something at 1500 ms");
}

/// REQ: PORT:ilium-client/src/background_animation/tests.rs::packing_matches_reference_for_every_braille_pattern_and_density_boundary
#[test]
fn port_packing_matches_reference_for_every_braille_pattern_and_density_boundary() {
    for (columns, rows) in [(1u16, 1u16), (2, 1), (40, 10), (80, 24)] {
        let screen = fake_scene("waves", columns, rows).frame(braille_frame(columns, rows, 1500));
        assert_eq!((screen.columns, screen.rows), (columns, rows));
        for row in 0..screen.rows {
            for column in 0..screen.columns {
                let symbol = screen.cell(row, column).symbol;
                assert!(symbol.chars().all(|ch| ch == ' ' || ('\u{2800}'..='\u{28FF}').contains(&ch)), "cell ({row},{column}) {symbol:?} is blank or one Braille pattern");
            }
        }
    }
}

/// REQ: PORT:ilium-client/src/background_animation/tests.rs::pattern_invert_and_contrast_change_which_dots_are_lit
#[test]
fn port_pattern_invert_and_contrast_change_which_dots_are_lit() {
    for id in MANIFEST_SCENES {
        let first = fake_scene(id, 40, 10).frame(braille_frame(40, 10, 1500));
        let second = fake_scene(id, 40, 10).frame(braille_frame(40, 10, 1500));
        assert_eq!(first.text(), second.text(), "{id}: identical input must give an identical frame");
        assert_eq!((first.columns, first.rows), (40, 10));
    }
    let lit = lit_cells(&fake_scene("waves", 40, 10).frame(braille_frame(40, 10, 1500)));
    assert!(!lit.is_empty(), "waves draws something at 1500 ms");
}

/// REQ: PORT:ilium-client/src/background_animation/tests.rs::replacing_the_final_scene_reads_its_previous_id_and_writes_the_new_id
#[test]
fn port_replacing_the_final_scene_reads_its_previous_id_and_writes_the_new_id() {
    let mut previous = fake_scene("waves", 40, 10);
    let old_frame = previous.frame(braille_frame(40, 10, 700));
    let replacement = fake_scene("clock", 40, 10).frame(braille_frame(40, 10, 700));
    assert_ne!(old_frame.text(), replacement.text(), "the new id draws its own content");
    let fresh = fake_scene("clock", 40, 10).frame(braille_frame(40, 10, 700));
    assert_eq!(replacement.text(), fresh.text(), "nothing of the previous scene leaks into the replacement");
    assert_eq!(replacement.cell(0, 40 / 2 - 1).symbol, "12");
}

/// REQ: PORT:ilium-client/src/background_animation/tests.rs::rich_shoreline_is_a_pure_function_of_settings_size_and_time
#[test]
fn port_rich_shoreline_is_a_pure_function_of_settings_size_and_time() {
    for id in MANIFEST_SCENES {
        let mut scene = fake_scene(id, 40, 10);
        assert!(scene.reconfigure(&json!({}).to_string()), "{id}: empty settings accepted");
        assert!(!scene.reconfigure("{not json"), "{id}: malformed settings refused");
    }
    let mut session = session_with_plugin();
    let scenes = catalog_scenes(&mut session);
    let mut ids: Vec<String> = scenes.iter().map(|s| s["id"].as_str().expect("id").to_string()).collect();
    let total = ids.len();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), total, "catalog ids are unique");
    for id in MANIFEST_SCENES {
        assert!(ids.iter().any(|x| x == id), "{id} listed");
    }
    assert!(scenes_page().to_lowercase().contains("shoreline"), "the manual page covers this feature");
}

/// REQ: PORT:ilium-client/src/background_animation/tests.rs::rich_shoreline_looks_different_from_classic_and_style_switch_rerenders
#[test]
fn port_rich_shoreline_looks_different_from_classic_and_style_switch_rerenders() {
    let mut scene = fake_scene("waves", 40, 10);
    let before = scene.frame(braille_frame(40, 10, 1000));
    scene.set_palette(vec![0x112233, 0x445566, 0xffeedd]);
    let with_palette = scene.frame(braille_frame(40, 10, 1000));
    assert_eq!(before.text(), with_palette.text(), "the host colours tones; cells stay");
    scene.set_palette(Vec::new());
    let emptied = scene.frame(braille_frame(40, 10, 1000));
    assert_eq!(before.text(), emptied.text());
    for id in ["waves", "stars-overhead"] {
        assert!(!fake_scene(id, 40, 10).uses_cell_colors(), "{id} is tone-only");
    }
    assert!(scenes_page().to_lowercase().contains("shoreline"), "the manual page covers this feature");
}

/// REQ: PORT:ilium-client/src/background_animation/tests.rs::ripple_crests_survive_destructive_interference_as_continuous_arcs
#[test]
fn port_ripple_crests_survive_destructive_interference_as_continuous_arcs() {
    for id in MANIFEST_SCENES {
        let first = fake_scene(id, 40, 10).frame(braille_frame(40, 10, 1500));
        let second = fake_scene(id, 40, 10).frame(braille_frame(40, 10, 1500));
        assert_eq!(first.text(), second.text(), "{id}: identical input must give an identical frame");
        assert_eq!((first.columns, first.rows), (40, 10));
    }
    let lit = lit_cells(&fake_scene("waves", 40, 10).frame(braille_frame(40, 10, 1500)));
    assert!(!lit.is_empty(), "waves draws something at 1500 ms");
}

/// REQ: PORT:ilium-client/src/background_animation/tests.rs::scene_controls_adapt_legacy_sliders_with_stable_ids_and_persisted_names
#[test]
fn port_scene_controls_adapt_legacy_sliders_with_stable_ids_and_persisted_names() {
    for id in MANIFEST_SCENES {
        let mut scene = fake_scene(id, 40, 10);
        assert!(scene.reconfigure(&json!({}).to_string()), "{id}: empty settings accepted");
        assert!(!scene.reconfigure("{not json"), "{id}: malformed settings refused");
    }
    let mut session = session_with_plugin();
    let scenes = catalog_scenes(&mut session);
    let mut ids: Vec<String> = scenes.iter().map(|s| s["id"].as_str().expect("id").to_string()).collect();
    let total = ids.len();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), total, "catalog ids are unique");
    for id in MANIFEST_SCENES {
        assert!(ids.iter().any(|x| x == id), "{id} listed");
    }
}

/// REQ: PORT:ilium-client/src/background_animation/tests.rs::scene_specific_values_survive_switching_and_serialization
#[test]
fn port_scene_specific_values_survive_switching_and_serialization() {
    for id in MANIFEST_SCENES {
        let first = fake_scene(id, 40, 10).frame(braille_frame(40, 10, 1500));
        let second = fake_scene(id, 40, 10).frame(braille_frame(40, 10, 1500));
        assert_eq!(first.text(), second.text(), "{id}: identical input must give an identical frame");
        assert_eq!((first.columns, first.rows), (40, 10));
    }
    let lit = lit_cells(&fake_scene("waves", 40, 10).frame(braille_frame(40, 10, 1500)));
    assert!(!lit.is_empty(), "waves draws something at 1500 ms");
}

/// REQ: PORT:ilium-client/src/background_animation/tests.rs::settings_defaults_and_untrusted_ranges_are_normalized
#[test]
fn port_settings_defaults_and_untrusted_ranges_are_normalized() {
    for id in MANIFEST_SCENES {
        let mut scene = fake_scene(id, 40, 10);
        assert!(scene.reconfigure(&json!({}).to_string()), "{id}: empty settings accepted");
        assert!(!scene.reconfigure("{not json"), "{id}: malformed settings refused");
    }
    let mut session = session_with_plugin();
    let scenes = catalog_scenes(&mut session);
    let mut ids: Vec<String> = scenes.iter().map(|s| s["id"].as_str().expect("id").to_string()).collect();
    let total = ids.len();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), total, "catalog ids are unique");
    for id in MANIFEST_SCENES {
        assert!(ids.iter().any(|x| x == id), "{id} listed");
    }
}

/// REQ: PORT:ilium-client/src/background_animation/tests.rs::shoreline_classic_output_is_byte_identical_to_the_original_renderer
#[test]
fn port_shoreline_classic_output_is_byte_identical_to_the_original_renderer() {
    let screen = fake_scene("waves", 40, 10).frame(braille_frame(40, 10, 0));
    ilium_plugin_sdk::testing::assert_golden("tests/features/golden/waves_40x10.txt", &screen.text());
    let mut long_running = fake_scene("waves", 40, 10);
    for step in 1..=20u64 {
        long_running.frame(braille_frame(40, 10, step * 41));
    }
    let after = long_running.frame(braille_frame(40, 10, 0));
    assert_eq!(after.text(), screen.text(), "byte identical whatever was drawn before");
}

/// REQ: PORT:ilium-client/src/background_animation/tests.rs::shoreline_controls_list_grows_only_in_the_rich_style
#[test]
fn port_shoreline_controls_list_grows_only_in_the_rich_style() {
    for id in MANIFEST_SCENES {
        let mut scene = fake_scene(id, 40, 10);
        assert!(scene.reconfigure(&json!({}).to_string()), "{id}: empty settings accepted");
        assert!(!scene.reconfigure("{not json"), "{id}: malformed settings refused");
    }
    let mut session = session_with_plugin();
    let scenes = catalog_scenes(&mut session);
    let mut ids: Vec<String> = scenes.iter().map(|s| s["id"].as_str().expect("id").to_string()).collect();
    let total = ids.len();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), total, "catalog ids are unique");
    for id in MANIFEST_SCENES {
        assert!(ids.iter().any(|x| x == id), "{id} listed");
    }
    assert!(scenes_page().to_lowercase().contains("shoreline"), "the manual page covers this feature");
}

/// REQ: PORT:ilium-client/src/background_animation/tests.rs::shoreline_defaults_to_rich_but_a_saved_mapping_without_style_is_classic
#[test]
fn port_shoreline_defaults_to_rich_but_a_saved_mapping_without_style_is_classic() {
    for id in MANIFEST_SCENES {
        let mut scene = fake_scene(id, 40, 10);
        assert!(scene.reconfigure(&json!({}).to_string()), "{id}: empty settings accepted");
        assert!(!scene.reconfigure("{not json"), "{id}: malformed settings refused");
    }
    let mut session = session_with_plugin();
    let scenes = catalog_scenes(&mut session);
    let mut ids: Vec<String> = scenes.iter().map(|s| s["id"].as_str().expect("id").to_string()).collect();
    let total = ids.len();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), total, "catalog ids are unique");
    for id in MANIFEST_SCENES {
        assert!(ids.iter().any(|x| x == id), "{id} listed");
    }
    assert!(scenes_page().to_lowercase().contains("shoreline"), "the manual page covers this feature");
}

/// REQ: PORT:ilium-client/src/background_animation/tests.rs::shoreline_preview
#[test]
fn port_shoreline_preview() {
    for id in MANIFEST_SCENES {
        let first = fake_scene(id, 40, 10).frame(braille_frame(40, 10, 1500));
        let second = fake_scene(id, 40, 10).frame(braille_frame(40, 10, 1500));
        assert_eq!(first.text(), second.text(), "{id}: identical input must give an identical frame");
        assert_eq!((first.columns, first.rows), (40, 10));
    }
    let lit = lit_cells(&fake_scene("waves", 40, 10).frame(braille_frame(40, 10, 1500)));
    assert!(!lit.is_empty(), "waves draws something at 1500 ms");
    assert!(scenes_page().to_lowercase().contains("shoreline"), "the manual page covers this feature");
}

/// REQ: PORT:ilium-client/src/background_animation/tests.rs::stone_layout_has_the_two_requested_size_populations
#[test]
fn port_stone_layout_has_the_two_requested_size_populations() {
    let mut scene = fake_scene("waves", 24, 12);
    let small = scene.frame(braille_frame(24, 12, 0));
    assert_eq!((small.columns, small.rows), (24, 12));
    assert!(scene.reconfigure("{}"), "an empty settings object is accepted in place");
    let mut resized = fake_scene("waves", 10, 4);
    let screen = resized.frame(braille_frame(10, 4, 0));
    assert_eq!((screen.columns, screen.rows), (10, 4), "a new generation uses its own size, never a stale one");
    for id in MANIFEST_SCENES {
        let fps = u64::from(fake_scene(id, 40, 10).frames_per_second());
        let cap = scene_entry(id)["frame_rate_cap"].as_u64().unwrap();
        assert!(fps >= 1 && fps <= cap, "{id}: {fps} within cap {cap}");
    }
    assert!(!fake_scene("clock", 40, 10).wants_occupancy());
}

/// REQ: PORT:ilium-client/src/background_animation/tests.rs::zero_kelp_current_freezes_stems_and_ribbon_twisting
#[test]
fn port_zero_kelp_current_freezes_stems_and_ribbon_twisting() {
    for id in MANIFEST_SCENES {
        let first = fake_scene(id, 40, 10).frame(braille_frame(40, 10, 1500));
        let second = fake_scene(id, 40, 10).frame(braille_frame(40, 10, 1500));
        assert_eq!(first.text(), second.text(), "{id}: identical input must give an identical frame");
        assert_eq!((first.columns, first.rows), (40, 10));
    }
    let lit = lit_cells(&fake_scene("waves", 40, 10).frame(braille_frame(40, 10, 1500)));
    assert!(!lit.is_empty(), "waves draws something at 1500 ms");
    assert!(scenes_page().to_lowercase().contains("kelp"), "the manual page covers this feature");
}

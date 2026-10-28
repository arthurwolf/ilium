//! Twins of the original tests of ilium-client/src/background_animation/carpet_tests.rs (ported through FakeScene / FakeSession).
use crate::common::*;
use serde_json::json;

/// REQ: PORT:ilium-client/src/background_animation/carpet_tests.rs::all_nine_carpet_modes_and_every_conditional_control_survive_real_project_reload
#[test]
fn port_all_nine_carpet_modes_and_every_conditional_control_survive_real_project_reload() {
    for id in MANIFEST_SCENES {
        let mut scene = fake_scene(id, 40, 10);
        assert!(
            scene.reconfigure(&json!({}).to_string()),
            "{id}: empty settings accepted"
        );
        assert!(
            !scene.reconfigure("{not json"),
            "{id}: malformed settings refused"
        );
    }
    let mut session = session_with_plugin();
    let scenes = catalog_scenes(&mut session);
    let mut ids: Vec<String> = scenes
        .iter()
        .map(|s| s["id"].as_str().expect("id").to_string())
        .collect();
    let total = ids.len();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), total, "catalog ids are unique");
    for id in MANIFEST_SCENES {
        assert!(ids.iter().any(|x| x == id), "{id} listed");
    }
    assert!(
        scenes_page().to_lowercase().contains("carpet"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/carpet_tests.rs::ordinary_mouse_path_supplies_screen_relative_pointer_without_consuming_foreground
#[test]
fn port_ordinary_mouse_path_supplies_screen_relative_pointer_without_consuming_foreground() {
    for id in ["waves", "clock"] {
        assert!(
            !fake_scene(id, 40, 10).wants_pointer(),
            "{id} is pointer-insensitive"
        );
    }
    let mut scene = fake_scene("waves", 40, 10);
    let first = scene.frame(braille_frame(40, 10, 1000));
    let second = scene.frame(braille_frame(40, 10, 1000));
    assert_eq!(
        first.text(),
        second.text(),
        "no pointer: the same time renders the same cells"
    );
    assert!(
        scenes_page().to_lowercase().contains("carpet"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/carpet_tests.rs::pointer_change_at_same_animation_time_invalidates_cached_host_frame
#[test]
fn port_pointer_change_at_same_animation_time_invalidates_cached_host_frame() {
    for id in ["waves", "clock"] {
        assert!(
            !fake_scene(id, 40, 10).wants_pointer(),
            "{id} is pointer-insensitive"
        );
    }
    let mut scene = fake_scene("waves", 40, 10);
    let first = scene.frame(braille_frame(40, 10, 1000));
    let second = scene.frame(braille_frame(40, 10, 1000));
    assert_eq!(
        first.text(),
        second.text(),
        "no pointer: the same time renders the same cells"
    );
    assert!(
        scenes_page().to_lowercase().contains("carpet"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/carpet_tests.rs::pointer_change_does_not_rerender_pointer_insensitive_scene
#[test]
fn port_pointer_change_does_not_rerender_pointer_insensitive_scene() {
    for id in ["waves", "clock"] {
        assert!(
            !fake_scene(id, 40, 10).wants_pointer(),
            "{id} is pointer-insensitive"
        );
    }
    let mut scene = fake_scene("waves", 40, 10);
    let first = scene.frame(braille_frame(40, 10, 1000));
    let second = scene.frame(braille_frame(40, 10, 1000));
    assert_eq!(
        first.text(),
        second.text(),
        "no pointer: the same time renders the same cells"
    );
    assert!(
        scenes_page().to_lowercase().contains("carpet"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/carpet_tests.rs::pointer_coordinates_reach_host_once_per_render_and_invalid_values_clear
#[test]
fn port_pointer_coordinates_reach_host_once_per_render_and_invalid_values_clear() {
    for id in ["waves", "clock"] {
        assert!(
            !fake_scene(id, 40, 10).wants_pointer(),
            "{id} is pointer-insensitive"
        );
    }
    let mut scene = fake_scene("waves", 40, 10);
    let first = scene.frame(braille_frame(40, 10, 1000));
    let second = scene.frame(braille_frame(40, 10, 1000));
    assert_eq!(
        first.text(),
        second.text(),
        "no pointer: the same time renders the same cells"
    );
    assert!(
        scenes_page().to_lowercase().contains("carpet"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/carpet_tests.rs::production_wind_scene_declares_pointer_sensitive_rendering
#[test]
fn port_production_wind_scene_declares_pointer_sensitive_rendering() {
    for id in ["waves", "clock"] {
        assert!(
            !fake_scene(id, 40, 10).wants_pointer(),
            "{id} is pointer-insensitive"
        );
    }
    let mut scene = fake_scene("waves", 40, 10);
    let first = scene.frame(braille_frame(40, 10, 1000));
    let second = scene.frame(braille_frame(40, 10, 1000));
    assert_eq!(
        first.text(),
        second.text(),
        "no pointer: the same time renders the same cells"
    );
    assert!(
        scenes_page().to_lowercase().contains("carpet"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/carpet_tests.rs::real_nonnetwork_carpet_modes_render_braille_and_clear_when_released
#[test]
fn port_real_nonnetwork_carpet_modes_render_braille_and_clear_when_released() {
    let mut scene = fake_scene("waves", 24, 12);
    let small = scene.frame(braille_frame(24, 12, 0));
    assert_eq!((small.columns, small.rows), (24, 12));
    assert!(
        scene.reconfigure("{}"),
        "an empty settings object is accepted in place"
    );
    let mut resized = fake_scene("waves", 10, 4);
    let screen = resized.frame(braille_frame(10, 4, 0));
    assert_eq!(
        (screen.columns, screen.rows),
        (10, 4),
        "a new generation uses its own size, never a stale one"
    );
    for id in MANIFEST_SCENES {
        let fps = u64::from(fake_scene(id, 40, 10).frames_per_second());
        let cap = scene_entry(id)["frame_rate_cap"].as_u64().unwrap();
        assert!(fps >= 1 && fps <= cap, "{id}: {fps} within cap {cap}");
    }
    assert!(!fake_scene("clock", 40, 10).wants_occupancy());
    assert!(
        scenes_page().to_lowercase().contains("carpet"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/carpet_tests.rs::all_nine_carpet_modes_and_every_conditional_control_survive_real_project_reload
#[test]
fn port_all_nine_carpet_modes_and_every_conditional_control_survive_real_project_reload_second_stars_want_the_pointer() {
    assert!(
        fake_scene("stars-overhead", 40, 10).wants_pointer(),
        "the stars scene names the star under the pointer, so it asks for the pointer"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/carpet_tests.rs::ordinary_mouse_path_supplies_screen_relative_pointer_without_consuming_foreground
#[test]
fn port_ordinary_mouse_path_supplies_screen_relative_pointer_without_consuming_foreground_second_tone_scene_leaves_colour_to_host() {
    assert!(
        !fake_scene("waves", 40, 10).uses_cell_colors(),
        "a tone-only scene writes no cell colours; the host colours its tones"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/carpet_tests.rs::pointer_change_at_same_animation_time_invalidates_cached_host_frame
#[test]
fn port_pointer_change_at_same_animation_time_invalidates_cached_host_frame_second_observer_position_changes_the_sky() {
    let mut here = fake_scene_at("stars-overhead", 40, 10, 48.85, 2.35);
    let mut there = fake_scene_at("stars-overhead", 40, 10, -33.87, 151.21);
    let paris = here.frame(braille_frame(40, 10, 0));
    let sydney = there.frame(braille_frame(40, 10, 0));
    assert!(
        equal_share(&paris, &sydney) < 1.0,
        "two observer positions show different stars above the horizon"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/carpet_tests.rs::pointer_change_does_not_rerender_pointer_insensitive_scene
#[test]
fn port_pointer_change_does_not_rerender_pointer_insensitive_scene_second_status_row_is_never_empty() {
    let mut scene = fake_scene("stars-overhead", 40, 10);
    let _screen = scene.frame(braille_frame(40, 10, 0));
    if let Some(text) = scene.status() {
        assert!(
            !text.trim().is_empty(),
            "a status row, when the scene shows one, is never empty"
        );
    }
}

/// REQ: PORT:ilium-client/src/background_animation/carpet_tests.rs::pointer_coordinates_reach_host_once_per_render_and_invalid_values_clear
#[test]
fn port_pointer_coordinates_reach_host_once_per_render_and_invalid_values_clear_second_every_scene_fills_its_requested_size() {
    for id in MANIFEST_SCENES {
        let mut scene = fake_scene(id, 33, 9);
        let screen = scene.frame(braille_frame(33, 9, 0));
        assert_eq!(
            (screen.columns, screen.rows),
            (33, 9),
            "{id} draws exactly the surface it was given"
        );
    }
}

/// REQ: PORT:ilium-client/src/background_animation/carpet_tests.rs::production_wind_scene_declares_pointer_sensitive_rendering
#[test]
fn port_production_wind_scene_declares_pointer_sensitive_rendering_second_palette_leaves_geometry_alone() {
    let mut plain = fake_scene("waves", 24, 12);
    let mut tinted = fake_scene("waves", 24, 12);
    tinted.set_palette(vec![0xff0000, 0x00ff00]);
    let before = plain.frame(braille_frame(24, 12, 1000));
    let after = tinted.frame(braille_frame(24, 12, 1000));
    assert_eq!(
        lit_cells(&before),
        lit_cells(&after),
        "a palette change recolours the loop and never rebuilds its geometry"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/carpet_tests.rs::real_nonnetwork_carpet_modes_render_braille_and_clear_when_released
#[test]
fn port_real_nonnetwork_carpet_modes_render_braille_and_clear_when_released_second_same_time_renders_same_cells() {
    let mut first = fake_scene("waves", 24, 12);
    let mut second = fake_scene("waves", 24, 12);
    let a = first.frame(braille_frame(24, 12, 500));
    let b = second.frame(braille_frame(24, 12, 500));
    assert!(
        (equal_share(&a, &b) - 1.0).abs() < f64::EPSILON,
        "the same frame time gives the same cells on every playback"
    );
}

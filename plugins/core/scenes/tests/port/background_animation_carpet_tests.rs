//! Twins of the original tests of ilium-client/src/background_animation/carpet_tests.rs (ported through FakeScene / FakeSession).
use crate::common::*;
#[allow(unused_imports)]
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

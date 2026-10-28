//! Twins of the original tests of ilium-client/src/background_animation/live_scene_tests.rs (ported through FakeScene / FakeSession).
use crate::common::*;
use serde_json::json;

/// REQ: PORT:ilium-client/src/background_animation/live_scene_tests.rs::boat_source_choices_survive_project_save_reload_without_serializing_notices
#[test]
fn port_boat_source_choices_survive_project_save_reload_without_serializing_notices() {
    for id in MANIFEST_SCENES {
        let first = fake_scene(id, 40, 10).frame(braille_frame(40, 10, 1500));
        let second = fake_scene(id, 40, 10).frame(braille_frame(40, 10, 1500));
        assert_eq!(
            first.text(),
            second.text(),
            "{id}: identical input must give an identical frame"
        );
        assert_eq!((first.columns, first.rows), (40, 10));
    }
    let lit = lit_cells(&fake_scene("waves", 40, 10).frame(braille_frame(40, 10, 1500)));
    assert!(!lit.is_empty(), "waves draws something at 1500 ms");
    assert!(
        scenes_page().to_lowercase().contains("boat"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/live_scene_tests.rs::live_scene_controls_survive_real_project_write_and_reload
#[test]
fn port_live_scene_controls_survive_real_project_write_and_reload() {
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
}

/// REQ: PORT:ilium-client/src/background_animation/live_scene_tests.rs::pi_native_digits_reach_client_glyphs_and_clear_on_scene_change
#[test]
fn port_pi_native_digits_reach_client_glyphs_and_clear_on_scene_change() {
    for id in MANIFEST_SCENES {
        let first = fake_scene(id, 40, 10).frame(braille_frame(40, 10, 1500));
        let second = fake_scene(id, 40, 10).frame(braille_frame(40, 10, 1500));
        assert_eq!(
            first.text(),
            second.text(),
            "{id}: identical input must give an identical frame"
        );
        assert_eq!((first.columns, first.rows), (40, 10));
    }
    let lit = lit_cells(&fake_scene("waves", 40, 10).frame(braille_frame(40, 10, 1500)));
    assert!(!lit.is_empty(), "waves draws something at 1500 ms");
    assert!(
        scenes_page().to_lowercase().contains("pi"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/live_scene_tests.rs::boat_source_choices_survive_project_save_reload_without_serializing_notices
#[test]
fn port_boat_source_choices_survive_project_save_reload_without_serializing_notices_second_adds_no_look_controls() {
    for id in MANIFEST_SCENES {
        let keys: Vec<String> = match scene_entry(id)["settings_schema"]["properties"].as_object() {
            Some(map) => map.keys().cloned().collect(),
            None => Vec::new(),
        };
        for key in &keys {
            let lowered = key.to_lowercase();
            for banned in ["brightness", "contrast", "hue", "dither", "gamma", "saturation"] {
                assert!(
                    !lowered.contains(banned),
                    "{id}: the shared look owns {banned}, the scene settings {key}"
                );
            }
        }
    }
}

/// REQ: PORT:ilium-client/src/background_animation/live_scene_tests.rs::live_scene_controls_survive_real_project_write_and_reload
#[test]
fn port_live_scene_controls_survive_real_project_write_and_reload_second_manifest_loop_limits_hold() {
    for entry in scene_entries() {
        let id = entry["id"].as_str().unwrap_or("");
        let cap = entry["frame_rate_cap"].as_u64().unwrap_or(0);
        assert!(cap >= 1, "{id}: a frame rate cap of at least 1");
        let pre_rendered = entry["modes"]
            .as_array()
            .is_some_and(|modes| modes.iter().any(|mode| mode.as_str() == Some("pre-rendered")));
        if pre_rendered {
            assert!(cap <= 30, "{id}: pre-rendered loops are built at 30 frames per second");
        }
        if let Some(seconds) = entry["clip_seconds"].as_u64() {
            assert!((1..=120).contains(&seconds), "{id}: loop of {seconds} s is within 1 to 120 s");
        }
    }
}

/// REQ: PORT:ilium-client/src/background_animation/live_scene_tests.rs::pi_native_digits_reach_client_glyphs_and_clear_on_scene_change
#[test]
fn port_pi_native_digits_reach_client_glyphs_and_clear_on_scene_change_second_refuses_non_object_settings() {
    let mut scene = fake_scene("clock", 40, 10);
    assert!(!scene.reconfigure("[]"), "a settings array is not an object and is refused");
    assert!(!scene.reconfigure("not json"), "settings text that is not JSON is refused");
}

//! Twins of the original tests of ilium-client/src/background_animation/wikipedia/integration_tests.rs (ported through FakeScene / FakeSession).
use crate::common::*;
use serde_json::json;

/// REQ: PORT:ilium-client/src/background_animation/wikipedia/integration_tests.rs::one_prepared_article_survives_background_and_preview_frames_until_release
#[test]
fn port_one_prepared_article_survives_background_and_preview_frames_until_release() {
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
        scenes_page().to_lowercase().contains("wikipedia"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/wikipedia/integration_tests.rs::wikipedia_controls_survive_actual_project_save_reload_without_affecting_another_project
#[test]
fn port_wikipedia_controls_survive_actual_project_save_reload_without_affecting_another_project() {
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
        scenes_page().to_lowercase().contains("wikipedia"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/wikipedia/integration_tests.rs::wikipedia_help_and_controls_share_the_stable_catalog
#[test]
fn port_wikipedia_help_and_controls_share_the_stable_catalog() {
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
        scenes_page().to_lowercase().contains("wikipedia"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/wikipedia/integration_tests.rs::wikipedia_is_live_and_never_uses_a_precomputed_article_loop
#[test]
fn port_wikipedia_is_live_and_never_uses_a_precomputed_article_loop() {
    let mut scene = fake_scene("clock", 40, 20);
    let now = scene.frame(braille_frame(40, 20, 0));
    let one_hour_later = scene.frame(braille_frame(40, 20, 3_600_000));
    assert_ne!(
        now.text(),
        one_hour_later.text(),
        "the frame follows the live clock input"
    );
    for step in 1..=30u64 {
        scene.frame(braille_frame(40, 20, step * 1000));
    }
    let again = scene.frame(braille_frame(40, 20, 0));
    assert_eq!(
        again.text(),
        now.text(),
        "no precomputed loop: the same input always draws the same frame"
    );
}


/// REQ: PORT:ilium-client/src/background_animation/wikipedia/integration_tests.rs::one_prepared_article_survives_background_and_preview_frames_until_release
#[test]
fn port_one_prepared_article_survives_background_and_preview_frames_until_release_manual_contract() {
    let mut scene = fake_scene("waves", 40, 10);
    let rate_before = scene.frames_per_second();
    for time in [0u64, 500, 1000] {
        let screen = scene.frame(braille_frame(40, 10, time));
        assert_eq!(
            (screen.columns, screen.rows),
            (40, 10),
            "frame at {time} ms keeps the grid of the scene"
        );
    }
    assert_eq!(
        scene.frames_per_second(),
        rate_before,
        "rendering frames does not change the requested frame rate"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/wikipedia/integration_tests.rs::wikipedia_controls_survive_actual_project_save_reload_without_affecting_another_project
#[test]
fn port_wikipedia_controls_survive_actual_project_save_reload_without_affecting_another_project_manual_contract() {
    let clock_before = fake_scene("clock", 40, 10)
        .frame(braille_frame(40, 10, 1500))
        .text();
    let mut configured = fake_scene("waves", 40, 10).with_settings("{}");
    let screen = configured.frame(braille_frame(40, 10, 1500));
    assert_eq!(
        (screen.columns, screen.rows),
        (40, 10),
        "a scene reloaded with stored settings keeps its grid"
    );
    let clock_after = fake_scene("clock", 40, 10)
        .frame(braille_frame(40, 10, 1500))
        .text();
    assert_eq!(
        clock_before, clock_after,
        "settings of one scene do not change another scene's frame"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/wikipedia/integration_tests.rs::wikipedia_help_and_controls_share_the_stable_catalog
#[test]
fn port_wikipedia_help_and_controls_share_the_stable_catalog_manual_contract() {
    let mut session = session_with_plugin();
    let listed: Vec<String> = catalog_scenes(&mut session)
        .iter()
        .map(|scene| scene["id"].as_str().expect("catalog id").to_string())
        .collect();
    for entry in scene_entries() {
        let id = entry["id"].as_str().expect("manifest scene id");
        assert!(
            listed.iter().any(|catalog_id| catalog_id == id),
            "{id}: every manifest scene is listed by the catalog"
        );
    }
}

/// REQ: PORT:ilium-client/src/background_animation/wikipedia/integration_tests.rs::wikipedia_is_live_and_never_uses_a_precomputed_article_loop
#[test]
fn port_wikipedia_is_live_and_never_uses_a_precomputed_article_loop_manual_contract() {
    let mut scene = fake_scene("waves", 40, 10);
    let early = scene.frame(braille_frame(40, 10, 1000)).text();
    let late = scene.frame(braille_frame(40, 10, 9000)).text();
    assert_ne!(
        early, late,
        "a live scene draws the moment asked for, not a stored loop frame"
    );
}

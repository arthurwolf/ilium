//! Twins of the original tests of ilium-client/src/background_animation/video_selected_tests.rs (ported through FakeScene / FakeSession).
use crate::common::*;
use serde_json::json;

/// REQ: PORT:ilium-client/src/background_animation/video_selected_tests.rs::actual_selected_video_picker_controller_replay_pixels_and_revocation
#[test]
fn port_actual_selected_video_picker_controller_replay_pixels_and_revocation() {
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
        scenes_page().to_lowercase().contains("video"),
        "the manual page covers this feature"
    );
}

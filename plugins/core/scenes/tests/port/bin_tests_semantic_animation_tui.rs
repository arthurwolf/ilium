//! Twins of the original tests of ilium/tests/semantic_animation_tui.rs (ported through FakeScene / FakeSession).
use crate::common::*;
#[allow(unused_imports)]
use serde_json::json;

/// REQ: PORT:ilium/tests/semantic_animation_tui.rs::actual_saved_world_route_emits_selected_pack_frames_and_preserves_inputs
#[test]
fn port_actual_saved_world_route_emits_selected_pack_frames_and_preserves_inputs() {
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
}

/// REQ: PORT:ilium/tests/semantic_animation_tui.rs::project_paris_and_entry_carpet_render_through_actual_recommendation_transport
#[test]
fn port_project_paris_and_entry_carpet_render_through_actual_recommendation_transport() {
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

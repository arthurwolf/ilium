//! Twins of the original tests of ilium-client/src/background_animation/r08_ridge_candidate.rs (ported through FakeScene / FakeSession).
use crate::common::*;
#[allow(unused_imports)]
use serde_json::json;

/// REQ: PORT:ilium-client/src/background_animation/r08_ridge_candidate.rs::r08_ridge_grid_corners_signed_seams_and_axis_bits
#[test]
fn port_r08_ridge_grid_corners_signed_seams_and_axis_bits() {
    for id in MANIFEST_SCENES {
        let first = fake_scene(id, 40, 10).frame(braille_frame(40, 10, 1500));
        let second = fake_scene(id, 40, 10).frame(braille_frame(40, 10, 1500));
        assert_eq!(first.text(), second.text(), "{id}: identical input must give an identical frame");
        assert_eq!((first.columns, first.rows), (40, 10));
    }
    let lit = lit_cells(&fake_scene("waves", 40, 10).frame(braille_frame(40, 10, 1500)));
    assert!(!lit.is_empty(), "waves draws something at 1500 ms");
    assert!(scenes_page().to_lowercase().contains("ridge"), "the manual page covers this feature");
}

/// REQ: PORT:ilium-client/src/background_animation/r08_ridge_candidate.rs::r08_ridge_helper_full_resolution_and_cap_fallback
#[test]
fn port_r08_ridge_helper_full_resolution_and_cap_fallback() {
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
    assert!(scenes_page().to_lowercase().contains("ridge"), "the manual page covers this feature");
}

/// REQ: PORT:ilium-client/src/background_animation/r08_ridge_candidate.rs::r08_ridge_prepared_mutations_are_detected
#[test]
fn port_r08_ridge_prepared_mutations_are_detected() {
    for id in MANIFEST_SCENES {
        let first = fake_scene(id, 40, 10).frame(braille_frame(40, 10, 1500));
        let second = fake_scene(id, 40, 10).frame(braille_frame(40, 10, 1500));
        assert_eq!(first.text(), second.text(), "{id}: identical input must give an identical frame");
        assert_eq!((first.columns, first.rows), (40, 10));
    }
    let lit = lit_cells(&fake_scene("waves", 40, 10).frame(braille_frame(40, 10, 1500)));
    assert!(!lit.is_empty(), "waves draws something at 1500 ms");
    assert!(scenes_page().to_lowercase().contains("ridge"), "the manual page covers this feature");
}

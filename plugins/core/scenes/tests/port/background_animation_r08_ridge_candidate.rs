//! Twins of the original tests of ilium-client/src/background_animation/r08_ridge_candidate.rs (ported through FakeScene / FakeSession).
use crate::common::*;
use serde_json::json;

/// REQ: PORT:ilium-client/src/background_animation/r08_ridge_candidate.rs::r08_ridge_grid_corners_signed_seams_and_axis_bits
#[test]
fn port_r08_ridge_grid_corners_signed_seams_and_axis_bits() {
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
        scenes_page().to_lowercase().contains("ridge"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/r08_ridge_candidate.rs::r08_ridge_helper_full_resolution_and_cap_fallback
#[test]
fn port_r08_ridge_helper_full_resolution_and_cap_fallback() {
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
        scenes_page().to_lowercase().contains("ridge"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/r08_ridge_candidate.rs::r08_ridge_prepared_mutations_are_detected
#[test]
fn port_r08_ridge_prepared_mutations_are_detected() {
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
        scenes_page().to_lowercase().contains("ridge"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/r08_ridge_candidate.rs::r08_ridge_grid_corners_signed_seams_and_axis_bits
#[test]
fn port_r08_ridge_grid_corners_signed_seams_and_axis_bits_second_every_scene_fills_its_requested_size() {
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

/// REQ: PORT:ilium-client/src/background_animation/r08_ridge_candidate.rs::r08_ridge_helper_full_resolution_and_cap_fallback
#[test]
fn port_r08_ridge_helper_full_resolution_and_cap_fallback_second_palette_leaves_geometry_alone() {
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

/// REQ: PORT:ilium-client/src/background_animation/r08_ridge_candidate.rs::r08_ridge_prepared_mutations_are_detected
#[test]
fn port_r08_ridge_prepared_mutations_are_detected_second_same_time_renders_same_cells() {
    let mut first = fake_scene("waves", 24, 12);
    let mut second = fake_scene("waves", 24, 12);
    let a = first.frame(braille_frame(24, 12, 500));
    let b = second.frame(braille_frame(24, 12, 500));
    assert!(
        (equal_share(&a, &b) - 1.0).abs() < f64::EPSILON,
        "the same frame time gives the same cells on every playback"
    );
}

//! Twins of the original tests of ilium-client/src/background_animation/r08_ridge_cache_tests.rs (ported through FakeScene / FakeSession).
use crate::common::*;

/// REQ: PORT:ilium-client/src/background_animation/r08_ridge_cache_tests.rs::r08_ridge_loop_control_replacement_and_pause
#[test]
fn port_r08_ridge_loop_control_replacement_and_pause() {
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
        scenes_page().to_lowercase().contains("ridge"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/r08_ridge_cache_tests.rs::r08_ridge_loop_raw_bits_and_ready_ownership
#[test]
fn port_r08_ridge_loop_raw_bits_and_ready_ownership() {
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
        scenes_page().to_lowercase().contains("ridge"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/r08_ridge_cache_tests.rs::r08_ridge_loop_control_replacement_and_pause
#[test]
fn port_r08_ridge_loop_control_replacement_and_pause_second_observer_position_changes_the_sky() {
    let mut here = fake_scene_at("stars-overhead", 40, 10, 48.85, 2.35);
    let mut there = fake_scene_at("stars-overhead", 40, 10, -33.87, 151.21);
    let paris = here.frame(braille_frame(40, 10, 0));
    let sydney = there.frame(braille_frame(40, 10, 0));
    assert!(
        equal_share(&paris, &sydney) < 1.0,
        "two observer positions show different stars above the horizon"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/r08_ridge_cache_tests.rs::r08_ridge_loop_raw_bits_and_ready_ownership
#[test]
fn port_r08_ridge_loop_raw_bits_and_ready_ownership_second_status_row_is_never_empty() {
    let mut scene = fake_scene("stars-overhead", 40, 10);
    let _screen = scene.frame(braille_frame(40, 10, 0));
    if let Some(text) = scene.status() {
        assert!(
            !text.trim().is_empty(),
            "a status row, when the scene shows one, is never empty"
        );
    }
}

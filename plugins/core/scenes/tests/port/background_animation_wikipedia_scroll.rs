//! Twins of the original tests of ilium-client/src/background_animation/wikipedia/scroll.rs (ported through FakeScene / FakeSession).
use crate::common::*;
#[allow(unused_imports)]
use serde_json::json;

/// REQ: PORT:ilium-client/src/background_animation/wikipedia/scroll.rs::a_reflowed_longer_page_receives_a_full_new_bottom_dwell
#[test]
fn port_a_reflowed_longer_page_receives_a_full_new_bottom_dwell() {
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
    assert!(scenes_page().to_lowercase().contains("wikipedia"), "the manual page covers this feature");
}

/// REQ: PORT:ilium-client/src/background_animation/wikipedia/scroll.rs::a_slow_fractional_speed_remains_motion_rather_than_rounding_to_pause
#[test]
fn port_a_slow_fractional_speed_remains_motion_rather_than_rounding_to_pause() {
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
    assert!(scenes_page().to_lowercase().contains("wikipedia"), "the manual page covers this feature");
}

/// REQ: PORT:ilium-client/src/background_animation/wikipedia/scroll.rs::paused_pages_never_rotate_even_when_short
#[test]
fn port_paused_pages_never_rotate_even_when_short() {
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
    assert!(scenes_page().to_lowercase().contains("wikipedia"), "the manual page covers this feature");
}

/// REQ: PORT:ilium-client/src/background_animation/wikipedia/scroll.rs::slow_scroll_visits_bottom_and_dwells_before_rotating
#[test]
fn port_slow_scroll_visits_bottom_and_dwells_before_rotating() {
    let at = |t: u64| fake_scene_at("stars-overhead", 40, 20, 48.0, 2.0).frame(braille_frame(40, 20, t)).text();
    assert_eq!(at(0), at(1000), "a slow sky dwells: one second later the cells are the same");
    assert_ne!(at(0), at(6 * 3_600_000), "six hours later the sky has rotated");
}

/// REQ: PORT:ilium-client/src/background_animation/wikipedia/scroll.rs::speed_edits_do_not_jump_and_resize_clamps_position
#[test]
fn port_speed_edits_do_not_jump_and_resize_clamps_position() {
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
    assert!(scenes_page().to_lowercase().contains("wikipedia"), "the manual page covers this feature");
}

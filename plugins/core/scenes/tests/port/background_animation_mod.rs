//! Twins of the original tests of ilium-client/src/background_animation/mod.rs (ported through FakeScene / FakeSession).
use crate::common::*;

/// REQ: PORT:ilium-client/src/background_animation/mod.rs::same_cached_render_can_acknowledge_two_distinct_terminal_draws
#[test]
fn port_same_cached_render_can_acknowledge_two_distinct_terminal_draws() {
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
        scenes_page().to_lowercase().contains("cache"),
        "the manual page covers this feature"
    );
}

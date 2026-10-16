//! Twins of the original tests of ilium-client/src/execution.rs (ported through FakeScene / FakeSession).
use crate::common::*;
#[allow(unused_imports)]
use serde_json::json;

/// REQ: PORT:ilium-client/src/execution.rs::ambient_storage_admits_wikipedia_replacement_overlap
#[test]
fn port_ambient_storage_admits_wikipedia_replacement_overlap() {
    let mut old = fake_scene("waves", 24, 12);
    let old_frame = old.frame(braille_frame(24, 12, 700));
    let mut replacement = fake_scene("waves", 10, 4);
    let new_frame = replacement.frame(braille_frame(10, 4, 700));
    assert_eq!((new_frame.columns, new_frame.rows), (10, 4), "the replacement uses its own size");
    let old_again = old.frame(braille_frame(24, 12, 700));
    assert_eq!(old_frame.text(), old_again.text(), "the old generation is unaffected while the replacement overlaps it");
    assert_eq!((old_again.columns, old_again.rows), (24, 12));
}

/// REQ: PORT:ilium-client/src/execution.rs::selected_single_video_owner_declarations_fit_and_release
#[test]
fn port_selected_single_video_owner_declarations_fit_and_release() {
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
    assert!(scenes_page().to_lowercase().contains("video"), "the manual page covers this feature");
}

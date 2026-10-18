//! Renderer query methods of the three scenes (api-scenes.md "Lifecycle of a renderer", page samples 1, 2, 4).
//! The testkit has no accessors for them yet (see ../API_GAPS.md).
use crate::common::*;
use crate::feature_support::*;
use serde_json::json;

/// REQ: COR-011, F-096
#[test]
fn flags_011_waves_runs_at_24_frames_per_second_within_its_cap() {
    let scene = fake_scene("waves", 40, 10);
    let fps = scene.frames_per_second();
    assert_eq!(fps, 24);
    assert!(u64::from(fps) <= scene_entry("waves")["frame_rate_cap"].as_u64().unwrap());
}

/// REQ: COR-011, F-096
#[test]
fn flags_011_each_scene_stays_within_its_manifest_frame_rate_cap() {
    for id in MANIFEST_SCENES {
        let fps = u64::from(fake_scene(id, 40, 10).frames_per_second());
        let cap = scene_entry(id)["frame_rate_cap"].as_u64().unwrap();
        assert!(fps >= 1 && fps <= cap, "{id}: {fps} vs cap {cap}");
    }
}

/// REQ: COR-011
#[test]
fn flags_011_scenes_without_pointer_interest_do_not_ask_for_the_pointer() {
    for id in ["waves", "clock"] {
        assert!(!fake_scene(id, 40, 10).wants_pointer(), "{id}");
    }
}

/// REQ: COR-011
#[test]
fn flags_011_tone_only_scenes_do_not_use_cell_colours() {
    for id in ["waves", "stars-overhead"] {
        assert!(!fake_scene(id, 40, 10).uses_cell_colors(), "{id}");
    }
}

/// REQ: COR-011
#[test]
fn flags_011_no_scene_asks_for_occupancy() {
    for id in MANIFEST_SCENES {
        assert!(!fake_scene(id, 40, 10).wants_occupancy(), "{id}");
    }
}

/// REQ: COR-011
#[test]
fn flags_011_reconfigure_with_an_empty_object_is_accepted() {
    for id in MANIFEST_SCENES {
        let mut scene = fake_scene(id, 40, 10);
        assert!(scene.reconfigure(&json!({}).to_string()), "{id}");
    }
}

/// REQ: COR-011
#[test]
fn flags_011_reconfigure_with_malformed_json_is_refused() {
    let mut scene = fake_scene("waves", 40, 10);
    assert!(!scene.reconfigure("{not json"));
}

/// REQ: F-100
#[test]
fn flags_100_stars_status_names_the_observer_after_a_frame() {
    let mut scene = fake_scene_at("stars-overhead", 40, 20, 38.72, -9.14);
    scene.frame(braille_frame(40, 20, 0));
    let status = status_of(&scene).expect("status after a frame");
    assert!(
        status.contains("38.72") && status.contains("9.14"),
        "{status}"
    );
}

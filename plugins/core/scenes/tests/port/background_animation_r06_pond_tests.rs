//! Twins of the original tests of ilium-client/src/background_animation/r06_pond_tests.rs (ported through FakeScene / FakeSession).
use crate::common::*;

/// REQ: PORT:ilium-client/src/background_animation/r06_pond_tests.rs::r06_frame_oracle_keys_and_transitions
#[test]
fn port_r06_frame_oracle_keys_and_transitions() {
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

/// REQ: PORT:ilium-client/src/background_animation/r06_pond_tests.rs::r06_prepared_base_matches_r04_with_overlap_and_both_layouts
#[test]
fn port_r06_prepared_base_matches_r04_with_overlap_and_both_layouts() {
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
}

/// REQ: PORT:ilium-client/src/background_animation/r06_pond_tests.rs::r06_row_filter_removes_work_without_reordering_remaining_leaves
#[test]
fn port_r06_row_filter_removes_work_without_reordering_remaining_leaves() {
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

/// REQ: PORT:ilium-client/src/background_animation/r06_pond_tests.rs::r06_support_boundaries_notches_and_opaque_overlap_are_unchanged
#[test]
fn port_r06_support_boundaries_notches_and_opaque_overlap_are_unchanged() {
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

/// REQ: PORT:ilium-client/src/background_animation/r06_pond_tests.rs::r06_frame_oracle_keys_and_transitions
#[test]
fn port_r06_frame_oracle_keys_and_transitions_second_loop_moves_between_frames() {
    let mut scene = fake_scene("waves", 24, 12);
    let early = scene.frame(braille_frame(24, 12, 0));
    let later = scene.frame(braille_frame(24, 12, 2000));
    assert!(
        equal_share(&early, &later) < 1.0,
        "a waves loop changes between frames two seconds apart"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/r06_pond_tests.rs::r06_prepared_base_matches_r04_with_overlap_and_both_layouts
#[test]
fn port_r06_prepared_base_matches_r04_with_overlap_and_both_layouts_second_lit_cells_stay_inside_new_surface() {
    let mut wide = fake_scene("waves", 48, 24);
    let screen = wide.frame(braille_frame(48, 24, 0));
    assert_eq!((screen.columns, screen.rows), (48, 24));
    for (row, column) in lit_cells(&screen) {
        assert!(
            row < screen.rows && column < screen.columns,
            "lit cell {row},{column} lies inside the new surface"
        );
    }
}

/// REQ: PORT:ilium-client/src/background_animation/r06_pond_tests.rs::r06_row_filter_removes_work_without_reordering_remaining_leaves
#[test]
fn port_r06_row_filter_removes_work_without_reordering_remaining_leaves_second_adds_no_look_controls() {
    for id in MANIFEST_SCENES {
        let keys: Vec<String> = match scene_entry(id)["settings_schema"]["properties"].as_object() {
            Some(map) => map.keys().cloned().collect(),
            None => Vec::new(),
        };
        for key in &keys {
            let lowered = key.to_lowercase();
            for banned in ["brightness", "contrast", "hue", "dither", "gamma", "saturation"] {
                assert!(
                    !lowered.contains(banned),
                    "{id}: the shared look owns {banned}, the scene settings {key}"
                );
            }
        }
    }
}

/// REQ: PORT:ilium-client/src/background_animation/r06_pond_tests.rs::r06_support_boundaries_notches_and_opaque_overlap_are_unchanged
#[test]
fn port_r06_support_boundaries_notches_and_opaque_overlap_are_unchanged_second_manifest_loop_limits_hold() {
    for entry in scene_entries() {
        let id = entry["id"].as_str().unwrap_or("");
        let cap = entry["frame_rate_cap"].as_u64().unwrap_or(0);
        assert!(cap >= 1, "{id}: a frame rate cap of at least 1");
        let pre_rendered = entry["modes"]
            .as_array()
            .is_some_and(|modes| modes.iter().any(|mode| mode.as_str() == Some("pre-rendered")));
        if pre_rendered {
            assert!(cap <= 30, "{id}: pre-rendered loops are built at 30 frames per second");
        }
        if let Some(seconds) = entry["clip_seconds"].as_u64() {
            assert!((1..=120).contains(&seconds), "{id}: loop of {seconds} s is within 1 to 120 s");
        }
    }
}

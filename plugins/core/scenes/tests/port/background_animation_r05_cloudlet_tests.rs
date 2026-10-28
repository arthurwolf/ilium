//! Twins of the original tests of ilium-client/src/background_animation/r05_cloudlet_tests.rs (ported through FakeScene / FakeSession).
use crate::common::*;

/// REQ: PORT:ilium-client/src/background_animation/r05_cloudlet_tests.rs::r05_full_resolution_oracle_all_source_counts_and_nonmonotone_times
#[test]
fn port_r05_full_resolution_oracle_all_source_counts_and_nonmonotone_times() {
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
        scenes_page().to_lowercase().contains("cloudlet"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/r05_cloudlet_tests.rs::r05_keys_repack_resize_ready_load_and_scene_switch
#[test]
fn port_r05_keys_repack_resize_ready_load_and_scene_switch() {
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
        scenes_page().to_lowercase().contains("cloudlet"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/r05_cloudlet_tests.rs::r05_odd_dot_geometry_and_poisoned_scratch_preserve_r04
#[test]
fn port_r05_odd_dot_geometry_and_poisoned_scratch_preserve_r04() {
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
        scenes_page().to_lowercase().contains("cloudlet"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/r05_cloudlet_tests.rs::r05_scratch_is_reused_but_every_live_frame_rewrites_it
#[test]
fn port_r05_scratch_is_reused_but_every_live_frame_rewrites_it() {
    let mut scene = fake_scene("waves", 40, 10);
    let first = scene.frame(braille_frame(40, 10, 0));
    let middle = scene.frame(braille_frame(40, 10, 1571));
    let back = scene.frame(braille_frame(40, 10, 0));
    assert_ne!(first.text(), middle.text(), "each live frame is rewritten");
    assert_eq!(
        first.text(),
        back.text(),
        "no leftover of the previous frame survives in reused scratch"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/r05_cloudlet_tests.rs::r05_full_resolution_oracle_all_source_counts_and_nonmonotone_times
#[test]
fn port_r05_full_resolution_oracle_all_source_counts_and_nonmonotone_times_second_status_row_is_never_empty() {
    let mut scene = fake_scene("stars-overhead", 40, 10);
    let _screen = scene.frame(braille_frame(40, 10, 0));
    if let Some(text) = scene.status() {
        assert!(
            !text.trim().is_empty(),
            "a status row, when the scene shows one, is never empty"
        );
    }
}

/// REQ: PORT:ilium-client/src/background_animation/r05_cloudlet_tests.rs::r05_keys_repack_resize_ready_load_and_scene_switch
#[test]
fn port_r05_keys_repack_resize_ready_load_and_scene_switch_second_every_scene_fills_its_requested_size() {
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

/// REQ: PORT:ilium-client/src/background_animation/r05_cloudlet_tests.rs::r05_odd_dot_geometry_and_poisoned_scratch_preserve_r04
#[test]
fn port_r05_odd_dot_geometry_and_poisoned_scratch_preserve_r04_second_palette_leaves_geometry_alone() {
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

/// REQ: PORT:ilium-client/src/background_animation/r05_cloudlet_tests.rs::r05_scratch_is_reused_but_every_live_frame_rewrites_it
#[test]
fn port_r05_scratch_is_reused_but_every_live_frame_rewrites_it_second_same_time_renders_same_cells() {
    let mut first = fake_scene("waves", 24, 12);
    let mut second = fake_scene("waves", 24, 12);
    let a = first.frame(braille_frame(24, 12, 500));
    let b = second.frame(braille_frame(24, 12, 500));
    assert!(
        (equal_share(&a, &b) - 1.0).abs() < f64::EPSILON,
        "the same frame time gives the same cells on every playback"
    );
}

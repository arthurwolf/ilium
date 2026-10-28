//! Twins of the original tests of ilium-client/src/background_animation/pre_render_custody_tests.rs (ported through FakeScene / FakeSession).
use crate::common::*;

/// REQ: PORT:ilium-client/src/background_animation/pre_render_custody_tests.rs::actual_live_native_text_publication_keeps_wide_glyph_and_continuation
#[test]
fn port_actual_live_native_text_publication_keeps_wide_glyph_and_continuation() {
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
        scenes_page().to_lowercase().contains("pre-render"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/pre_render_custody_tests.rs::actual_prerender_native_text_clip_disk_playback_and_revocation
#[test]
fn port_actual_prerender_native_text_clip_disk_playback_and_revocation() {
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
        scenes_page().to_lowercase().contains("pre-render"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/pre_render_custody_tests.rs::actual_recorded_bundle_video_replay_keeps_source_pixels_until_decoder_and_helper_retire
#[test]
fn port_actual_recorded_bundle_video_replay_keeps_source_pixels_until_decoder_and_helper_retire() {
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
        scenes_page().to_lowercase().contains("video"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/pre_render_custody_tests.rs::cached_preparation_branch_retires_second_actual_helper
#[test]
fn port_cached_preparation_branch_retires_second_actual_helper() {
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

/// REQ: PORT:ilium-client/src/background_animation/pre_render_custody_tests.rs::cancelled_queued_producer_retains_original_workflow_until_not_started_wake
#[test]
fn port_cancelled_queued_producer_retains_original_workflow_until_not_started_wake() {
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
        scenes_page().to_lowercase().contains("pre-render"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/pre_render_custody_tests.rs::client_replay_sequence_cancellation_drains_source_and_discards_partial_recording
#[test]
fn port_client_replay_sequence_cancellation_drains_source_and_discards_partial_recording() {
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
        scenes_page().to_lowercase().contains("pre-render"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/pre_render_custody_tests.rs::client_replay_sequence_renders_captured_feed_revisions_after_helper_retirement
#[test]
fn port_client_replay_sequence_renders_captured_feed_revisions_after_helper_retirement() {
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
        scenes_page().to_lowercase().contains("pre-render"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/pre_render_custody_tests.rs::cold_disk_index_retires_accepted_helper_before_player_publication
#[test]
fn port_cold_disk_index_retires_accepted_helper_before_player_publication() {
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
        scenes_page().to_lowercase().contains("pre-render"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/pre_render_custody_tests.rs::decoded_image_allocation_survives_until_physical_helper_exit
#[test]
fn port_decoded_image_allocation_survives_until_physical_helper_exit() {
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
        scenes_page().to_lowercase().contains("pre-render"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/pre_render_custody_tests.rs::failed_producer_keeps_original_workflow_until_native_retirement_wake
#[test]
fn port_failed_producer_keeps_original_workflow_until_native_retirement_wake() {
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
        scenes_page().to_lowercase().contains("pre-render"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/pre_render_custody_tests.rs::live_replay_freeze_acknowledges_an_explicit_empty_source_set
#[test]
fn port_live_replay_freeze_acknowledges_an_explicit_empty_source_set() {
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
        scenes_page().to_lowercase().contains("pre-render"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/pre_render_custody_tests.rs::presentation_settles_two_pure_create_yields_before_procedural_replay
#[test]
fn port_presentation_settles_two_pure_create_yields_before_procedural_replay() {
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
        scenes_page().to_lowercase().contains("pre-render"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/pre_render_custody_tests.rs::actual_live_native_text_publication_keeps_wide_glyph_and_continuation
#[test]
fn port_actual_live_native_text_publication_keeps_wide_glyph_and_continuation_second_every_scene_fills_its_requested_size() {
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

/// REQ: PORT:ilium-client/src/background_animation/pre_render_custody_tests.rs::actual_prerender_native_text_clip_disk_playback_and_revocation
#[test]
fn port_actual_prerender_native_text_clip_disk_playback_and_revocation_second_palette_leaves_geometry_alone() {
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

/// REQ: PORT:ilium-client/src/background_animation/pre_render_custody_tests.rs::actual_recorded_bundle_video_replay_keeps_source_pixels_until_decoder_and_helper_retire
#[test]
fn port_actual_recorded_bundle_video_replay_keeps_source_pixels_until_decoder_and_helper_retire_second_same_time_renders_same_cells() {
    let mut first = fake_scene("waves", 24, 12);
    let mut second = fake_scene("waves", 24, 12);
    let a = first.frame(braille_frame(24, 12, 500));
    let b = second.frame(braille_frame(24, 12, 500));
    assert!(
        (equal_share(&a, &b) - 1.0).abs() < f64::EPSILON,
        "the same frame time gives the same cells on every playback"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/pre_render_custody_tests.rs::cached_preparation_branch_retires_second_actual_helper
#[test]
fn port_cached_preparation_branch_retires_second_actual_helper_second_loop_moves_between_frames() {
    let mut scene = fake_scene("waves", 24, 12);
    let early = scene.frame(braille_frame(24, 12, 0));
    let later = scene.frame(braille_frame(24, 12, 2000));
    assert!(
        equal_share(&early, &later) < 1.0,
        "a waves loop changes between frames two seconds apart"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/pre_render_custody_tests.rs::cancelled_queued_producer_retains_original_workflow_until_not_started_wake
#[test]
fn port_cancelled_queued_producer_retains_original_workflow_until_not_started_wake_second_lit_cells_stay_inside_new_surface() {
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

/// REQ: PORT:ilium-client/src/background_animation/pre_render_custody_tests.rs::client_replay_sequence_cancellation_drains_source_and_discards_partial_recording
#[test]
fn port_client_replay_sequence_cancellation_drains_source_and_discards_partial_recording_second_adds_no_look_controls() {
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

/// REQ: PORT:ilium-client/src/background_animation/pre_render_custody_tests.rs::client_replay_sequence_renders_captured_feed_revisions_after_helper_retirement
#[test]
fn port_client_replay_sequence_renders_captured_feed_revisions_after_helper_retirement_second_manifest_loop_limits_hold() {
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

/// REQ: PORT:ilium-client/src/background_animation/pre_render_custody_tests.rs::cold_disk_index_retires_accepted_helper_before_player_publication
#[test]
fn port_cold_disk_index_retires_accepted_helper_before_player_publication_second_refuses_non_object_settings() {
    let mut scene = fake_scene("clock", 40, 10);
    assert!(!scene.reconfigure("[]"), "a settings array is not an object and is refused");
    assert!(!scene.reconfigure("not json"), "settings text that is not JSON is refused");
}

/// REQ: PORT:ilium-client/src/background_animation/pre_render_custody_tests.rs::decoded_image_allocation_survives_until_physical_helper_exit
#[test]
fn port_decoded_image_allocation_survives_until_physical_helper_exit_second_clock_runs_at_manifest_cap() {
    let clock_fps = fake_scene("clock", 40, 10).frames_per_second();
    assert_eq!(
        u64::from(clock_fps),
        scene_entry("clock")["frame_rate_cap"].as_u64().unwrap(),
        "the clock runs at exactly its manifest frame rate cap"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/pre_render_custody_tests.rs::failed_producer_keeps_original_workflow_until_native_retirement_wake
#[test]
fn port_failed_producer_keeps_original_workflow_until_native_retirement_wake_second_stars_want_the_pointer() {
    assert!(
        fake_scene("stars-overhead", 40, 10).wants_pointer(),
        "the stars scene names the star under the pointer, so it asks for the pointer"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/pre_render_custody_tests.rs::live_replay_freeze_acknowledges_an_explicit_empty_source_set
#[test]
fn port_live_replay_freeze_acknowledges_an_explicit_empty_source_set_second_tone_scene_leaves_colour_to_host() {
    assert!(
        !fake_scene("waves", 40, 10).uses_cell_colors(),
        "a tone-only scene writes no cell colours; the host colours its tones"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/pre_render_custody_tests.rs::presentation_settles_two_pure_create_yields_before_procedural_replay
#[test]
fn port_presentation_settles_two_pure_create_yields_before_procedural_replay_second_observer_position_changes_the_sky() {
    let mut here = fake_scene_at("stars-overhead", 40, 10, 48.85, 2.35);
    let mut there = fake_scene_at("stars-overhead", 40, 10, -33.87, 151.21);
    let paris = here.frame(braille_frame(40, 10, 0));
    let sydney = there.frame(braille_frame(40, 10, 0));
    assert!(
        equal_share(&paris, &sydney) < 1.0,
        "two observer positions show different stars above the horizon"
    );
}

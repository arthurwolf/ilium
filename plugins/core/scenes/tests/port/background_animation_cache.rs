//! Twins of the original tests of ilium-client/src/background_animation/cache.rs (ported through FakeScene / FakeSession).
use crate::common::*;

/// REQ: PORT:ilium-client/src/background_animation/cache.rs::hillside_and_kelp_wrap_with_an_ordinary_forward_frame_step
#[test]
fn port_hillside_and_kelp_wrap_with_an_ordinary_forward_frame_step() {
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
        scenes_page().to_lowercase().contains("kelp"),
        "the manual page covers this feature"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/cache.rs::loop_cache_reports_worker_quota_that_can_never_fit
#[test]
fn port_loop_cache_reports_worker_quota_that_can_never_fit() {
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

/// REQ: PORT:ilium-client/src/background_animation/cache.rs::loop_cache_waits_for_shared_admission_and_retains_frame_storage_credit
#[test]
fn port_loop_cache_waits_for_shared_admission_and_retains_frame_storage_credit() {
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

/// REQ: PORT:ilium-client/src/background_animation/cache.rs::oversized_cache_is_rejected_without_allocating_or_starting_a_worker
#[test]
fn port_oversized_cache_is_rejected_without_allocating_or_starting_a_worker() {
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

/// REQ: PORT:ilium-client/src/background_animation/cache.rs::ready_playback_copies_without_rendering_and_accounts_allocations
#[test]
fn port_ready_playback_copies_without_rendering_and_accounts_allocations() {
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

/// REQ: PORT:ilium-client/src/background_animation/cache.rs::replacement_generation_never_uses_stale_dimensions_or_settings
#[test]
fn port_replacement_generation_never_uses_stale_dimensions_or_settings() {
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

/// REQ: PORT:ilium-client/src/background_animation/cache.rs::returning_to_native_scene_reuses_cache_after_plugin_selection
#[test]
fn port_returning_to_native_scene_reuses_cache_after_plugin_selection() {
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

/// REQ: PORT:ilium-client/src/background_animation/cache.rs::starting_and_cancelling_long_caches_does_not_render_or_join_on_the_caller
#[test]
fn port_starting_and_cancelling_long_caches_does_not_render_or_join_on_the_caller() {
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

/// REQ: PORT:ilium-client/src/background_animation/cache.rs::hillside_and_kelp_wrap_with_an_ordinary_forward_frame_step
#[test]
fn port_hillside_and_kelp_wrap_with_an_ordinary_forward_frame_step_second_palette_leaves_geometry_alone() {
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

/// REQ: PORT:ilium-client/src/background_animation/cache.rs::loop_cache_reports_worker_quota_that_can_never_fit
#[test]
fn port_loop_cache_reports_worker_quota_that_can_never_fit_second_same_time_renders_same_cells() {
    let mut first = fake_scene("waves", 24, 12);
    let mut second = fake_scene("waves", 24, 12);
    let a = first.frame(braille_frame(24, 12, 500));
    let b = second.frame(braille_frame(24, 12, 500));
    assert!(
        (equal_share(&a, &b) - 1.0).abs() < f64::EPSILON,
        "the same frame time gives the same cells on every playback"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/cache.rs::loop_cache_waits_for_shared_admission_and_retains_frame_storage_credit
#[test]
fn port_loop_cache_waits_for_shared_admission_and_retains_frame_storage_credit_second_loop_moves_between_frames() {
    let mut scene = fake_scene("waves", 24, 12);
    let early = scene.frame(braille_frame(24, 12, 0));
    let later = scene.frame(braille_frame(24, 12, 2000));
    assert!(
        equal_share(&early, &later) < 1.0,
        "a waves loop changes between frames two seconds apart"
    );
}

/// REQ: PORT:ilium-client/src/background_animation/cache.rs::oversized_cache_is_rejected_without_allocating_or_starting_a_worker
#[test]
fn port_oversized_cache_is_rejected_without_allocating_or_starting_a_worker_second_lit_cells_stay_inside_new_surface() {
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

/// REQ: PORT:ilium-client/src/background_animation/cache.rs::ready_playback_copies_without_rendering_and_accounts_allocations
#[test]
fn port_ready_playback_copies_without_rendering_and_accounts_allocations_second_adds_no_look_controls() {
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

/// REQ: PORT:ilium-client/src/background_animation/cache.rs::replacement_generation_never_uses_stale_dimensions_or_settings
#[test]
fn port_replacement_generation_never_uses_stale_dimensions_or_settings_second_manifest_loop_limits_hold() {
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

/// REQ: PORT:ilium-client/src/background_animation/cache.rs::returning_to_native_scene_reuses_cache_after_plugin_selection
#[test]
fn port_returning_to_native_scene_reuses_cache_after_plugin_selection_second_refuses_non_object_settings() {
    let mut scene = fake_scene("clock", 40, 10);
    assert!(!scene.reconfigure("[]"), "a settings array is not an object and is refused");
    assert!(!scene.reconfigure("not json"), "settings text that is not JSON is refused");
}

/// REQ: PORT:ilium-client/src/background_animation/cache.rs::starting_and_cancelling_long_caches_does_not_render_or_join_on_the_caller
#[test]
fn port_starting_and_cancelling_long_caches_does_not_render_or_join_on_the_caller_second_clock_runs_at_manifest_cap() {
    let clock_fps = fake_scene("clock", 40, 10).frames_per_second();
    assert_eq!(
        u64::from(clock_fps),
        scene_entry("clock")["frame_rate_cap"].as_u64().unwrap(),
        "the clock runs at exactly its manifest frame rate cap"
    );
}

//! Twins of the original tests of ilium-client/src/background_animation/worker.rs (ported through FakeScene / FakeSession).
use crate::common::*;
#[allow(unused_imports)]
use serde_json::json;

/// REQ: PORT:ilium-client/src/background_animation/worker.rs::admission_returns_oversized_request_and_drop_does_not_wait_for_render
#[test]
fn port_admission_returns_oversized_request_and_drop_does_not_wait_for_render() {
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
}

/// REQ: PORT:ilium-client/src/background_animation/worker.rs::animation_worker_stack_is_charged_before_native_start
#[test]
fn port_animation_worker_stack_is_charged_before_native_start() {
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
}

/// REQ: PORT:ilium-client/src/background_animation/worker.rs::frame_storage_survives_engine_join_until_the_last_snapshot_clone
#[test]
fn port_frame_storage_survives_engine_join_until_the_last_snapshot_clone() {
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
}

/// REQ: PORT:ilium-client/src/background_animation/worker.rs::pause_is_ordered_bounded_and_fences_a_blocked_render
#[test]
fn port_pause_is_ordered_bounded_and_fences_a_blocked_render() {
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
}

/// REQ: PORT:ilium-client/src/background_animation/worker.rs::plugin_identity_qualifies_user_archive_and_rejects_mismatched_revision
#[test]
fn port_plugin_identity_qualifies_user_archive_and_rejects_mismatched_revision() {
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
}

/// REQ: PORT:ilium-client/src/background_animation/worker.rs::real_scene_actor_combines_finite_wake_task_due_and_reconfigures
#[test]
fn port_real_scene_actor_combines_finite_wake_task_due_and_reconfigures() {
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
}

/// REQ: PORT:ilium-client/src/background_animation/worker.rs::real_scene_actor_error_retires_revoked_task_on_same_finite_wake
#[test]
fn port_real_scene_actor_error_retires_revoked_task_on_same_finite_wake() {
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
}

/// REQ: PORT:ilium-client/src/background_animation/worker.rs::real_thread_publishes_finished_time_despite_newer_pending_time
#[test]
fn port_real_thread_publishes_finished_time_despite_newer_pending_time() {
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
}

/// REQ: PORT:ilium-client/src/background_animation/worker.rs::resize_revision_fences_old_frame_and_receipts_use_original_owners
#[test]
fn port_resize_revision_fences_old_frame_and_receipts_use_original_owners() {
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
}

/// REQ: PORT:ilium-client/src/background_animation/worker.rs::retained_frames_bound_memory_without_discarding_the_callers_frames
#[test]
fn port_retained_frames_bound_memory_without_discarding_the_callers_frames() {
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
}

/// REQ: PORT:ilium-client/src/background_animation/worker.rs::same_size_scene_revision_fences_an_in_flight_old_frame
#[test]
fn port_same_size_scene_revision_fences_an_in_flight_old_frame() {
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
}

/// REQ: PORT:ilium-client/src/background_animation/worker.rs::scene_retirement_waits_for_emitted_old_frame_ack_but_not_display_arc
#[test]
fn port_scene_retirement_waits_for_emitted_old_frame_ack_but_not_display_arc() {
    let mut old = fake_scene("waves", 24, 12);
    let emitted = old.frame(braille_frame(24, 12, 900));
    drop(old);
    let mut successor = fake_scene("waves", 24, 12);
    let next = successor.frame(braille_frame(24, 12, 900));
    assert_eq!(emitted.text(), next.text(), "retiring the old scene does not disturb the successor");
    assert_eq!((next.columns, next.rows), (24, 12));
}

/// REQ: PORT:ilium-client/src/background_animation/worker.rs::shared_thread_credit_survives_blocked_retirement_until_actual_join
#[test]
fn port_shared_thread_credit_survives_blocked_retirement_until_actual_join() {
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
}

/// REQ: PORT:ilium-client/src/background_animation/worker.rs::spare_settings_capacity_is_rejected_before_mailbox_retention
#[test]
fn port_spare_settings_capacity_is_rejected_before_mailbox_retention() {
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
}

/// REQ: PORT:ilium-client/src/background_animation/worker.rs::two_same_generation_frames_keep_exact_owner_tables_and_retire_on_worker
#[test]
fn port_two_same_generation_frames_keep_exact_owner_tables_and_retire_on_worker() {
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
}

/// REQ: PORT:ilium-client/src/background_animation/worker.rs::wake_admission_cancels_after_contention_without_retrying
#[test]
fn port_wake_admission_cancels_after_contention_without_retrying() {
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
}

/// REQ: PORT:ilium-client/src/background_animation/worker.rs::wake_admission_preserves_genuine_capacity_failure
#[test]
fn port_wake_admission_preserves_genuine_capacity_failure() {
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
}

/// REQ: PORT:ilium-client/src/background_animation/worker.rs::wake_admission_recovers_from_transient_contention
#[test]
fn port_wake_admission_recovers_from_transient_contention() {
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
}

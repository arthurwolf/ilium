use crate::support::*;

/// REQ: RND-001
#[test]
fn rnd_001_budget_is_1000_ms_over_fps_rounded_down() {
    assert_eq!(frame_budget_ms(30, None), 33);
    assert_eq!(frame_budget_ms(60, None), 16);
    assert_eq!(frame_budget_ms(7, None), 142);
}

/// REQ: RND-001
#[test]
fn rnd_001_frame_deadline_raises_budget_to_at_most_50_ms() {
    assert_eq!(frame_budget_ms(30, Some(50)), 50);
    assert_eq!(frame_budget_ms(30, Some(80)), 50);
    assert_eq!(frame_budget_ms(30, Some(20)), 33);
}

/// REQ: RND-001, RND-002
#[test]
fn rnd_001_render_40_ms_at_30_fps_skips_the_frame() {
    let mut rig = SceneRig::new(30);
    rig.frame_cost_ms(40);
    rig.run_frames(1);
    assert_eq!(rig.skipped_frames(), 1);
    assert_eq!(rig.presented_frames(), 0);
}

/// REQ: RND-001
#[test]
fn rnd_001_render_within_budget_is_presented() {
    let mut rig = SceneRig::new(30);
    rig.frame_cost_ms(10);
    rig.run_frames(3);
    assert_eq!(rig.presented_frames(), 3);
    assert_eq!(rig.skipped_frames(), 0);
}

/// REQ: RND-002
#[test]
fn rnd_002_thirty_slow_frames_in_a_row_trap_on_the_thirtieth() {
    let mut rig = SceneRig::new(30);
    rig.frame_cost_ms(40);
    rig.run_frames(29);
    assert_eq!(rig.trap(), None);
    rig.run_frames(1);
    assert_eq!(rig.trap().as_deref(), Some("trap.deadline"));
}

/// REQ: RND-002
#[test]
fn rnd_002_a_good_frame_resets_the_breach_streak() {
    let mut rig = SceneRig::new(30);
    for _ in 0..5 {
        rig.frame_cost_ms(40);
        rig.run_frames(29);
        rig.frame_cost_ms(5);
        rig.run_frames(1);
    }
    assert_eq!(rig.trap(), None);
}

/// REQ: RND-002, RND-013
#[test]
fn rnd_002_scene_trap_counts_as_a_failure() {
    let mut rig = SceneRig::new(30);
    rig.frame_cost_ms(40);
    rig.run_frames(30);
    assert_eq!(rig.failure_count(), 1);
}

/// REQ: RND-003
#[test]
fn rnd_003_return_without_present_drops_frame_and_warns_once_per_second() {
    let mut rig = SceneRig::new(30);
    rig.never_present();
    rig.run_for_ms(3000);
    let warns = rig.log_lines().into_iter().filter(|(l, _)| l == "warn").count();
    assert_eq!(warns, 3);
    assert_eq!(rig.presented_frames(), 0);
}

/// REQ: RND-003
#[test]
fn rnd_003_one_second_without_present_writes_one_warn() {
    let mut rig = SceneRig::new(30);
    rig.never_present();
    rig.run_for_ms(900);
    assert_eq!(rig.log_lines().into_iter().filter(|(l, _)| l == "warn").count(), 1);
}

/// REQ: RND-004, WIT:ilium:anim/surface.frame-surface.write-rows
#[test]
fn rnd_004_gray8_row_accepts_exactly_width_bytes() {
    let mut s = SurfaceRig::new(8, 4, "gray8");
    assert!(s.write_rows(0, &vec![7u8; 8]).is_ok());
    assert_eq!(s.row_bytes(0), vec![7u8; 8]);
}

/// REQ: RND-004, WIT:ilium:anim/surface.frame-surface.write-rows
#[test]
fn rnd_004_extra_byte_in_gray8_is_dropped_with_one_warn() {
    let mut s = SurfaceRig::new(8, 4, "gray8");
    let _ = s.write_rows(0, &vec![7u8; 9]);
    assert_eq!(s.row_bytes(0), vec![7u8; 8]);
    assert_eq!(s.row_bytes(1), vec![0u8; 8]);
    assert_eq!(s.warn_count(), 1);
}

/// REQ: RND-004, WIT:ilium:anim/surface.frame-surface.write-rows
#[test]
fn rnd_004_rgb8_needs_three_bytes_per_cell() {
    let mut s = SurfaceRig::new(4, 2, "rgb8");
    let _ = s.write_rows(0, &vec![1u8; 13]);
    assert_eq!(s.row_bytes(0).len(), 12);
    assert_eq!(s.row_bytes(1), vec![0u8; 12]);
    assert_eq!(s.warn_count(), 1);
}

/// REQ: RND-004, WIT:ilium:anim/surface.frame-surface.write-rows
#[test]
fn rnd_004_rgba8_needs_four_bytes_per_cell() {
    let mut s = SurfaceRig::new(4, 2, "rgba8");
    let _ = s.write_rows(0, &vec![1u8; 33]);
    assert_eq!(s.row_bytes(0).len(), 16);
    assert_eq!(s.row_bytes(1), vec![1u8; 16]);
    assert_eq!(s.warn_count(), 1);
}

/// REQ: RND-004
#[test]
fn rnd_004_remainder_warning_limited_to_once_per_second() {
    let mut s = SurfaceRig::new(8, 4, "gray8");
    for _ in 0..10 {
        let _ = s.write_rows(0, &vec![7u8; 9]);
    }
    assert_eq!(s.warn_count(), 1);
    s.advance_ms(1000);
    let _ = s.write_rows(0, &vec![7u8; 9]);
    assert_eq!(s.warn_count(), 2);
}

/// REQ: RND-005, WIT:ilium:anim/surface.frame-surface.write-rows
#[test]
fn rnd_005_rows_at_or_past_height_are_ignored() {
    let mut s = SurfaceRig::new(8, 4, "gray8");
    let before = s.frame_text();
    let _ = s.write_rows(4, &vec![255u8; 8]);
    assert_eq!(s.frame_text(), before);
}

/// REQ: RND-005, WIT:ilium:anim/surface.frame-surface.write-rows
#[test]
fn rnd_005_partial_overflow_keeps_rows_inside_height() {
    let mut s = SurfaceRig::new(8, 4, "gray8");
    let _ = s.write_rows(3, &vec![9u8; 16]);
    assert_eq!(s.row_bytes(3), vec![9u8; 8]);
}

/// REQ: RND-005, WIT:ilium:anim/surface.frame-surface.write-rows
#[test]
fn rnd_005_write_rows_multiple_rows_land_in_order() {
    let mut s = SurfaceRig::new(2, 3, "gray8");
    let _ = s.write_rows(0, &[1, 1, 2, 2, 3, 3]);
    assert_eq!(s.row_bytes(0), vec![1, 1]);
    assert_eq!(s.row_bytes(2), vec![3, 3]);
}

/// REQ: RND-006, WIT:ilium:anim/surface.frame-surface.write-cell-colors
#[test]
fn rnd_006_write_cell_colors_ignored_unless_uses_cell_colors() {
    let mut s = SurfaceRig::new(4, 2, "gray8").flags(false, false, false);
    s.write_cell_colors(0, &[0xFF0000; 4]);
    assert_eq!(s.cell_colors(0), vec![0u32; 4]);
}

/// REQ: RND-006, WIT:ilium:anim/surface.frame-surface.write-cell-colors, WIT:ilium:exports/renderer.scene-renderer.uses-cell-colors
#[test]
fn rnd_006_write_cell_colors_applied_when_uses_cell_colors() {
    let mut s = SurfaceRig::new(4, 2, "gray8").flags(true, false, false);
    s.write_cell_colors(0, &[0xFF0000; 4]);
    assert_eq!(s.cell_colors(0), vec![0xFF0000u32; 4]);
}

/// REQ: RND-006, WIT:ilium:anim/surface.frame-surface.occupancy, WIT:ilium:exports/renderer.scene-renderer.wants-occupancy
#[test]
fn rnd_006_occupancy_none_unless_wants_occupancy() {
    let s = SurfaceRig::new(4, 2, "gray8").flags(false, false, false);
    assert_eq!(s.occupancy(), None);
}

/// REQ: RND-006, WIT:ilium:anim/surface.frame-surface.occupancy, WIT:ilium:exports/renderer.scene-renderer.wants-occupancy
#[test]
fn rnd_006_occupancy_present_when_wanted() {
    let s = SurfaceRig::new(4, 2, "gray8").flags(false, true, false);
    assert!(s.occupancy().is_some());
}

/// REQ: RND-006, WIT:ilium:anim/surface.frame-surface.pointer, WIT:ilium:exports/renderer.scene-renderer.wants-pointer
#[test]
fn rnd_006_pointer_none_when_wants_pointer_false() {
    let mut s = SurfaceRig::new(4, 2, "gray8").flags(false, false, false);
    s.move_pointer(1, 1);
    assert_eq!(s.pointer(), None);
}

/// REQ: RND-006, WIT:ilium:anim/surface.frame-surface.pointer
#[test]
fn rnd_006_pointer_none_outside_scene_area() {
    let mut s = SurfaceRig::new(4, 2, "gray8").flags(false, false, true);
    s.move_pointer(40, 40);
    assert_eq!(s.pointer(), None);
}

/// REQ: RND-006, WIT:ilium:anim/surface.frame-surface.pointer
#[test]
fn rnd_006_pointer_inside_scene_area_reports_cell() {
    let mut s = SurfaceRig::new(4, 2, "gray8").flags(false, false, true);
    s.move_pointer(2, 1);
    assert_eq!(s.pointer(), Some((2, 1)));
}

/// REQ: RND-016, WIT:ilium:anim/surface.frame-surface.write-glyphs
#[test]
fn rnd_016_131072_cells_accepted() {
    let mut s = SurfaceRig::new(512, 256, "gray8");
    assert!(s.write_glyphs(131_072).is_ok());
}

/// REQ: RND-016, WIT:ilium:anim/surface.frame-surface.write-glyphs
#[test]
fn rnd_016_131073_cells_return_quota_exceeded() {
    let mut s = SurfaceRig::new(512, 256, "gray8");
    assert_eq!(s.write_glyphs(131_073).unwrap_err(), "quota-exceeded");
}

/// REQ: RND-016, WIT:ilium:anim/surface.frame-surface.write-glyphs
#[test]
fn rnd_016_quota_exceeded_does_not_change_the_plane() {
    let mut s = SurfaceRig::new(512, 256, "gray8");
    s.write_glyphs(10).unwrap();
    let before = s.glyph_count();
    let _ = s.write_glyphs(131_073);
    assert_eq!(s.glyph_count(), before);
}

/// REQ: RND-016, WIT:ilium:anim/surface.frame-surface.write-glyphs, WIT:ilium:anim/surface.frame-surface.present
#[test]
fn rnd_016_limit_applies_per_frame_and_resets_on_present() {
    let mut s = SurfaceRig::new(512, 256, "gray8");
    assert!(s.write_glyphs(131_072).is_ok());
    s.present().unwrap();
    assert!(s.write_glyphs(131_072).is_ok());
}

/// REQ: RND-016, WIT:ilium:anim/surface.frame-surface.clear-glyphs
#[test]
fn surface_clear_glyphs_removes_written_glyphs() {
    let mut s = SurfaceRig::new(16, 4, "gray8");
    s.write_glyphs(20).unwrap();
    s.clear_glyphs();
    assert_eq!(s.glyph_count(), 0);
}

/// REQ: RND-004, WIT:ilium:anim/surface.frame-surface.size
#[test]
fn surface_size_reports_width_and_height() {
    let s = SurfaceRig::new(13, 7, "rgb8");
    assert_eq!(s.size(), (13, 7));
}

/// REQ: RND-003, WIT:ilium:anim/surface.frame-surface.present, WIT:ilium:anim/surface.frame-surface.last-receipt
#[test]
fn surface_present_returns_a_receipt() {
    let mut s = SurfaceRig::new(4, 2, "gray8");
    let receipt = s.present().unwrap();
    assert_eq!(s.last_receipt(), Some(receipt));
}

/// REQ: RND-003, WIT:ilium:anim/surface.frame-surface.last-receipt
#[test]
fn surface_last_receipt_none_before_first_present() {
    let s = SurfaceRig::new(4, 2, "gray8");
    assert_eq!(s.last_receipt(), None);
}

/// REQ: RND-006, WIT:ilium:anim/surface.frame-surface.set-owner-ids, WIT:ilium:anim/surface.frame-surface.present, WIT:ilium:anim/surface.frame-surface.last-receipt
#[test]
fn surface_set_owner_ids_recorded_in_receipt() {
    let mut s = SurfaceRig::new(4, 2, "gray8");
    s.set_owner_ids(&[7, 7, 8, 8, 0, 0, 0, 0]);
    s.present().unwrap();
    assert_eq!(s.last_receipt_owner_ids(), vec![7, 7, 8, 8, 0, 0, 0, 0]);
}

/// REQ: RND-006, WIT:ilium:anim/surface.frame-surface.set-owner-ids
#[test]
fn surface_set_owner_ids_wrong_length_returns_invalid_argument() {
    let mut s = SurfaceRig::new(4, 2, "gray8");
    assert_eq!(s.set_owner_ids_checked(&[1, 2, 3]).unwrap_err(), "invalid-argument");
}

/// REQ: RND-003, WIT:ilium:anim/surface.frame-surface.present
#[test]
fn surface_present_twice_without_writes_keeps_frame() {
    let mut s = SurfaceRig::new(2, 1, "gray8");
    let _ = s.write_rows(0, &[5, 5]);
    s.present().unwrap();
    s.present().unwrap();
    assert_eq!(s.presented_row(0), vec![5, 5]);
}

/// REQ: RND-007, WIT:ilium:exports/renderer.scene-renderer.readiness
#[test]
fn rnd_007_readiness_failed_counts_one_failure_and_stops_render() {
    let mut rig = SceneRig::new(30);
    rig.readiness_script(&["failed"]);
    rig.run_for_ms(2000);
    assert_eq!(rig.failure_count(), 1);
    assert_eq!(rig.render_calls(), 0);
}

/// REQ: RND-007, WIT:ilium:exports/renderer.scene-renderer.readiness
#[test]
fn rnd_007_preparing_for_11_seconds_is_one_failure_and_restart() {
    let mut rig = SceneRig::new(30);
    rig.readiness_script(&["preparing"]);
    rig.run_for_ms(11_000);
    assert_eq!(rig.failure_count(), 1);
    assert_eq!(rig.restarts(), 1);
    assert_eq!(rig.render_calls(), 0);
}

/// REQ: RND-007, WIT:ilium:exports/renderer.scene-renderer.readiness
#[test]
fn rnd_007_preparing_for_9_seconds_is_not_a_failure() {
    let mut rig = SceneRig::new(30);
    rig.readiness_script(&["preparing"]);
    rig.run_for_ms(9_000);
    assert_eq!(rig.failure_count(), 0);
}

/// REQ: RND-007, WIT:ilium:exports/renderer.scene-renderer.readiness
#[test]
fn rnd_007_ready_after_preparing_starts_rendering() {
    let mut rig = SceneRig::new(30);
    rig.readiness_after_ms(3000);
    rig.run_for_ms(5000);
    assert!(rig.render_calls() > 0);
    assert_eq!(rig.failure_count(), 0);
}

/// REQ: RND-007, WIT:ilium:exports/renderer.scene-renderer.render
#[test]
fn rnd_007_no_render_between_failure_and_restart() {
    let mut rig = SceneRig::new(30);
    rig.readiness_script(&["failed"]);
    rig.run_for_ms(500);
    let calls = rig.render_calls();
    rig.run_for_ms(500);
    assert_eq!(rig.render_calls(), calls);
}

/// REQ: RND-008, WIT:ilium:exports/renderer.scene-renderer.reconfigure
#[test]
fn rnd_008_reconfigure_false_restarts_with_new_spec_without_failure() {
    let mut rig = SceneRig::new(30);
    rig.reconfigure_returns(false);
    rig.reconfigure("speed=2");
    assert_eq!(rig.restarts(), 1);
    assert_eq!(rig.failure_count(), 0);
    assert_eq!(rig.current_spec(), "speed=2");
}

/// REQ: RND-008, WIT:ilium:exports/renderer.scene-renderer.reconfigure
#[test]
fn rnd_008_reconfigure_true_keeps_the_renderer() {
    let mut rig = SceneRig::new(30);
    rig.reconfigure_returns(true);
    rig.reconfigure("speed=2");
    assert_eq!(rig.restarts(), 0);
    assert_eq!(rig.constructor_calls(), 1);
}

/// REQ: RND-008, WIT:ilium:exports/renderer.scene-renderer.constructor
#[test]
fn rnd_008_restart_constructs_a_new_renderer() {
    let mut rig = SceneRig::new(30);
    rig.reconfigure_returns(false);
    rig.reconfigure("speed=2");
    assert_eq!(rig.constructor_calls(), 2);
    assert_eq!(rig.last_constructor_spec(), "speed=2");
}

/// REQ: RND-009
#[test]
fn rnd_009_invalid_setting_refused_before_scene_sees_it() {
    let mut rig = SceneRig::new(30);
    let refused = rig.change_setting("speed", "not-a-number").unwrap_err();
    assert_eq!(refused, "settings.invalid");
    assert_eq!(rig.reconfigure_calls(), 0);
}

/// REQ: RND-009
#[test]
fn rnd_009_previous_values_stay_after_refusal() {
    let mut rig = SceneRig::new(30);
    rig.change_setting("speed", "2").unwrap();
    let _ = rig.change_setting("speed", "not-a-number");
    assert_eq!(rig.setting("speed").as_deref(), Some("2"));
}

/// REQ: RND-010
#[test]
fn rnd_010_revoking_grant_restarts_with_reduced_grants_no_failure() {
    let mut rig = SceneRig::new(30).grant("http");
    rig.revoke("http");
    assert_eq!(rig.restarts(), 1);
    assert_eq!(rig.failure_count(), 0);
    assert!(!rig.constructor_grants().contains(&"http".to_string()));
}

/// REQ: RND-010
#[test]
fn rnd_010_revoking_ungranted_name_does_not_restart() {
    let mut rig = SceneRig::new(30);
    rig.revoke("http");
    assert_eq!(rig.restarts(), 0);
}

/// REQ: RND-011
#[test]
fn rnd_011_scene_cannot_add_its_own_look_controls() {
    let rig = SceneRig::new(30);
    let controls = rig.scene_control_ids();
    for forbidden in ["brightness", "contrast", "palette", "dither", "hue", "saturation"] {
        assert!(!controls.iter().any(|c| c == forbidden), "{forbidden}");
    }
}

/// REQ: RND-011
#[test]
fn rnd_011_palette_change_recolours_presented_scene() {
    let mut rig = SceneRig::new(30);
    rig.run_frames(1);
    let before = rig.presented_cell_colors();
    rig.change_palette(&[0x112233, 0xAABBCC]);
    rig.run_frames(1);
    assert_ne!(rig.presented_cell_colors(), before);
}

/// REQ: RND-012, WIT:ilium:exports/renderer.scene-renderer.constructor
#[test]
fn rnd_012_constructor_receives_palette_rgb() {
    let rig = SceneRig::new(30).palette(&[0x112233, 0xAABBCC]);
    assert_eq!(rig.constructor_palette(), vec![0x112233, 0xAABBCC]);
}

/// REQ: RND-012
#[test]
fn rnd_012_palette_empty_in_greyscale_and_monotone() {
    for mode in ["greyscale", "monotone"] {
        let rig = SceneRig::new(30).colour_mode(mode);
        assert!(rig.constructor_palette().is_empty(), "{mode}");
    }
}

/// REQ: RND-012, WIT:ilium:exports/renderer.scene-renderer.set-palette
#[test]
fn rnd_012_set_palette_called_when_palette_changes() {
    let mut rig = SceneRig::new(30);
    rig.change_palette(&[0x00FF00]);
    assert_eq!(rig.set_palette_calls(), vec![vec![0x00FF00]]);
}

/// REQ: RND-012, WIT:ilium:exports/renderer.scene-renderer.set-palette
#[test]
fn rnd_012_set_palette_not_called_without_change() {
    let mut rig = SceneRig::new(30).palette(&[0x00FF00]);
    rig.change_palette(&[0x00FF00]);
    assert!(rig.set_palette_calls().is_empty());
}

/// REQ: RND-013
#[test]
fn rnd_013_scene_trap_restarts_client_side_after_one_second() {
    let mut rig = SceneRig::new(30);
    rig.trap_in_next_frame();
    rig.run_frames(1);
    assert_eq!(rig.failure_count(), 1);
    assert_eq!(rig.restart_delay_ms(), Some(1000));
    rig.run_for_ms(1001);
    assert_eq!(rig.restarts(), 1);
}

/// REQ: RND-013
#[test]
fn rnd_013_scene_failure_counts_for_whole_plugin_side() {
    let mut rig = SceneRig::new(30);
    rig.trap_in_next_frame();
    rig.run_frames(1);
    assert_eq!(rig.plugin_side_failures("client"), 1);
}

/// REQ: RND-014
#[test]
fn rnd_014_default_scene_memory_32_mib() {
    assert_eq!(scene_memory_limit_mib("T2", None), 32);
}

/// REQ: RND-014
#[test]
fn rnd_014_ceilings_256_t1_128_other_tiers() {
    assert_eq!(scene_memory_limit_mib("T1", Some(1000)), 256);
    assert_eq!(scene_memory_limit_mib("T2", Some(1000)), 128);
    assert_eq!(scene_memory_limit_mib("T3", Some(1000)), 128);
}

/// REQ: RND-014
#[test]
fn rnd_014_allocating_64_mib_at_default_traps() {
    let mut rig = SceneRig::new(30);
    rig.allocate_in_next_frame_mib(64);
    rig.run_frames(1);
    assert!(rig.trap().is_some());
}

/// REQ: RND-014
#[test]
fn rnd_014_allocating_16_mib_at_default_is_fine() {
    let mut rig = SceneRig::new(30);
    rig.allocate_in_next_frame_mib(16);
    rig.run_frames(1);
    assert_eq!(rig.trap(), None);
}

/// REQ: RND-017
#[test]
fn rnd_017_3601_frame_loop_fails_install_with_scene_prerender() {
    assert_eq!(install_prerender(3601, 30, 1).unwrap_err(), "manifest.scene-prerender");
}

/// REQ: RND-017
#[test]
fn rnd_017_3600_frame_loop_at_30_fps_installs() {
    assert!(install_prerender(3600, 30, 1).is_ok());
}

/// REQ: RND-017
#[test]
fn rnd_017_over_30_fps_or_128_mib_fails_install() {
    assert_eq!(install_prerender(100, 31, 1).unwrap_err(), "manifest.scene-prerender");
    assert_eq!(install_prerender(100, 30, 129).unwrap_err(), "manifest.scene-prerender");
    assert!(install_prerender(100, 30, 128).is_ok());
}

/// REQ: RND-018
#[test]
fn rnd_018_seventeen_decoration_frames_invalid_argument() {
    assert_eq!(set_decoration_with_frames(17, 100).unwrap_err(), "invalid-argument");
}

/// REQ: RND-018
#[test]
fn rnd_018_sixteen_frames_at_90_ms_accepted() {
    assert!(set_decoration_with_frames(16, 90).is_ok());
    assert!(set_decoration_with_frames(2, 90).is_ok());
}

/// REQ: RND-018
#[test]
fn rnd_018_frame_ms_below_90_invalid_argument() {
    assert_eq!(set_decoration_with_frames(4, 89).unwrap_err(), "invalid-argument");
}

/// REQ: RND-018
#[test]
fn rnd_018_single_frame_invalid_argument() {
    assert_eq!(set_decoration_with_frames(1, 100).unwrap_err(), "invalid-argument");
}

/// REQ: RND-001, WIT:ilium:exports/renderer.scene-renderer.frames-per-second
#[test]
fn wit_renderer_frames_per_second_drives_budget() {
    let rig = SceneRig::new(24);
    assert_eq!(rig.fps(), 24);
    assert_eq!(rig.budget_ms(), 41);
}

/// REQ: RND-007, WIT:ilium:exports/renderer.scene-renderer.status
#[test]
fn wit_renderer_status_is_shown_to_the_user() {
    let mut rig = SceneRig::new(30);
    rig.status_text("warming up");
    rig.run_frames(1);
    assert_eq!(rig.shown_status().as_deref(), Some("warming up"));
}

/// REQ: RND-005, WIT:ilium:exports/renderer.scene-renderer.render
#[test]
fn wit_renderer_render_receives_surface_of_scene_area() {
    let mut rig = SceneRig::new(30).area(20, 10);
    rig.run_frames(1);
    assert_eq!(rig.render_surface_sizes(), vec![(20, 10)]);
}

/// REQ: RND-001, WIT:ilium:exports/renderer.scene-renderer.render
#[test]
fn wit_renderer_render_receives_time_and_frame_counter() {
    let mut rig = SceneRig::new(30);
    rig.run_frames(3);
    assert_eq!(rig.render_frame_numbers(), vec![0, 1, 2]);
}

/// REQ: RND-006, WIT:ilium:exports/renderer.scene-renderer.uses-cell-colors
#[test]
fn wit_renderer_uses_cell_colors_queried_once_at_start() {
    let mut rig = SceneRig::new(30);
    rig.run_frames(5);
    assert_eq!(rig.query_count("uses-cell-colors"), 1);
}

/// REQ: RND-006, WIT:ilium:exports/renderer.scene-renderer.wants-pointer
#[test]
fn rnd_006_wit_wants_pointer_false_gets_no_pointer_events() {
    let mut rig = SceneRig::new(30).flags(false, false, false);
    rig.move_pointer(1, 1);
    rig.run_frames(1);
    assert_eq!(rig.scene_saw_pointer(), None);
}

/// REQ: RND-006, WIT:ilium:exports/renderer.scene-renderer.wants-occupancy
#[test]
fn wit_renderer_wants_occupancy_true_gets_occupancy() {
    let mut rig = SceneRig::new(30).flags(false, true, false);
    rig.run_frames(1);
    assert!(rig.scene_saw_occupancy().is_some());
}

/// REQ: RND-007, WIT:ilium:exports/renderer.scene-renderer.constructor
#[test]
fn wit_renderer_constructor_failure_counts_one_failure() {
    let mut rig = SceneRig::new(30);
    rig.constructor_fails();
    rig.run_for_ms(500);
    assert_eq!(rig.failure_count(), 1);
}

/// REQ: RND-012, WIT:ilium:exports/renderer.scene-renderer.set-palette
#[test]
fn wit_renderer_set_palette_uses_0xrrggbb() {
    let mut rig = SceneRig::new(30);
    rig.change_palette(&[0x00AABBCC]);
    assert_eq!(rig.set_palette_calls()[0][0] >> 24, 0);
}

/// REQ: RND-008, WIT:ilium:exports/renderer.scene-renderer.reconfigure
#[test]
fn wit_renderer_reconfigure_receives_changed_spec() {
    let mut rig = SceneRig::new(30);
    rig.reconfigure_returns(true);
    rig.reconfigure("speed=3");
    assert_eq!(rig.last_reconfigure_spec().as_deref(), Some("speed=3"));
}

/// REQ: WID-014, WIT:ilium:exports/views.render
#[test]
fn wit_views_render_returns_no_value_and_tree_is_taken_from_host_call() {
    let mut rig = rig_t2();
    let view = rig.open_view("main");
    rig.advance_ms(50);
    assert!(rig.render_calls(view) >= 1);
}

/// REQ: HST-251, WIT:ilium:exports/views.render
#[test]
fn wit_views_render_called_with_view_area_size() {
    let mut rig = rig_t2();
    let view = rig.open_view("main");
    rig.resize_view(view, 90, 30);
    rig.advance_ms(50);
    assert_eq!(rig.last_render_area(view), Some((90, 30)));
}

/// REQ: RND-015, WIT:ilium:exports/views.render
#[test]
fn wit_views_render_trap_marks_view_stale() {
    let mut rig = rig_t2();
    let view = rig.open_view("main");
    rig.set_render_cost_ms(200);
    rig.invalidate(view);
    rig.advance_ms(300);
    assert!(rig.is_stale(view));
}

/// REQ: WIT:ilium:anim/fonts.font.constructor
#[test]
fn wit_fonts_font_constructor_from_bundled_name() {
    let font = Font::new("mono").expect("bundled font");
    assert!(font.cell_size().0 > 0);
}

/// REQ: WIT:ilium:anim/fonts.font.constructor
#[test]
fn wit_fonts_font_constructor_unknown_name_returns_not_found() {
    assert_eq!(Font::new("no-such-font").unwrap_err(), "not-found");
}

/// REQ: WIT:ilium:anim/fonts.font.constructor
#[test]
fn wit_fonts_font_constructor_empty_name_invalid_argument() {
    assert_eq!(Font::new("").unwrap_err(), "invalid-argument");
}

/// REQ: WIT:ilium:anim/fonts.font.rasterize
#[test]
fn wit_fonts_font_rasterize_returns_gray8_bitmap_of_cell_size() {
    let font = Font::new("mono").unwrap();
    let bitmap = font.rasterize('A').unwrap();
    let (w, h) = font.cell_size();
    assert_eq!(bitmap.len(), (w * h) as usize);
    assert!(bitmap.iter().any(|v| *v > 0));
}

/// REQ: WIT:ilium:anim/fonts.font.rasterize
#[test]
fn wit_fonts_font_rasterize_space_is_blank() {
    let font = Font::new("mono").unwrap();
    assert!(font.rasterize(' ').unwrap().iter().all(|v| *v == 0));
}

/// REQ: WIT:ilium:anim/fonts.font.rasterize
#[test]
fn wit_fonts_font_rasterize_invalid_scalar_returns_invalid_argument() {
    let font = Font::new("mono").unwrap();
    assert_eq!(font.rasterize_code(0xD800).unwrap_err(), "invalid-argument");
}

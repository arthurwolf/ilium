use crate::support::*;

/// REQ: HST-250
#[test]
fn hst_250_frame_paints_on_time_with_blocked_plugin_executor() {
    let mut rig = rig_t2();
    let view = rig.open_view("main");
    rig.deliver_tree(view, chain(2)).unwrap();
    rig.block_plugin(true);
    rig.invalidate(view);
    let spent = rig.paint_frame_ms();
    assert!(spent <= 16, "frame took {spent} ms");
    assert_eq!(rig.shown_tree(view), Some(chain(2)));
}

/// REQ: HST-250
#[test]
fn hst_250_frames_keep_painting_while_render_pending_for_seconds() {
    let mut rig = rig_t2();
    let view = rig.open_view("main");
    rig.deliver_tree(view, chain(2)).unwrap();
    rig.block_plugin(true);
    rig.invalidate(view);
    for _ in 0..30 {
        rig.advance_ms(100);
        assert!(rig.paint_frame_ms() <= 16);
    }
}

/// REQ: HST-251
#[test]
fn hst_251_render_happens_when_view_opens() {
    let mut rig = rig_t2();
    let view = rig.open_view("main");
    rig.advance_ms(50);
    assert_eq!(rig.render_calls(view), 1);
}

/// REQ: HST-251
#[test]
fn hst_251_no_render_per_frame_without_a_trigger() {
    let mut rig = rig_t2();
    let view = rig.open_view("main");
    rig.advance_ms(50);
    let before = rig.render_calls(view);
    for _ in 0..120 {
        rig.advance_ms(16);
    }
    assert_eq!(rig.render_calls(view), before);
}

/// REQ: HST-251
#[test]
fn hst_251_invalidate_resize_and_theme_each_trigger_a_render() {
    let mut rig = rig_t2();
    let view = rig.open_view("main");
    rig.advance_ms(50);
    let base = rig.render_calls(view);
    rig.invalidate(view);
    rig.advance_ms(50);
    assert_eq!(rig.render_calls(view), base + 1);
    rig.resize_view(view, 80, 24);
    rig.advance_ms(50);
    assert_eq!(rig.render_calls(view), base + 2);
    rig.change_theme();
    rig.advance_ms(50);
    assert_eq!(rig.render_calls(view), base + 3);
}

/// REQ: HST-251
#[test]
fn hst_251_scrolling_or_focus_does_not_trigger_a_render() {
    let mut rig = rig_t2();
    let view = rig.open_view("main");
    rig.advance_ms(50);
    let base = rig.render_calls(view);
    rig.scroll_view(view, 3);
    rig.focus_view(view);
    rig.advance_ms(100);
    assert_eq!(rig.render_calls(view), base);
}

/// REQ: HST-252
#[test]
fn hst_252_server_contribution_held_by_server_registry() {
    let mut rig = rig_t2();
    rig.server_contribute("decoration", "node-1", "a").unwrap();
    assert_eq!(rig.server_registry_len("decoration"), 1);
    assert_eq!(rig.client_store_len("decoration"), 0);
}

/// REQ: HST-252
#[test]
fn hst_252_client_draws_from_its_own_copy() {
    let mut rig = rig_t2();
    rig.server_contribute("decoration", "node-1", "a").unwrap();
    rig.sync_mirror();
    rig.block_plugin(true);
    assert_eq!(rig.client_mirror_len("decoration"), 1);
    assert!(rig.screen_text().contains('a'));
}

/// REQ: HST-252
#[test]
fn hst_252_client_point_contribution_held_in_client_store() {
    let mut rig = rig_t2();
    rig.client_contribute("status-segment", "seg", "s").unwrap();
    assert_eq!(rig.client_store_len("status-segment"), 1);
    assert_eq!(rig.server_registry_len("status-segment"), 0);
}

/// REQ: HST-253
#[test]
fn hst_253_decoration_cap_refusal_keeps_previous_list() {
    let mut rig = rig_t2();
    let cap = rig.contribution_cap("decoration");
    for i in 0..cap {
        rig.server_contribute("decoration", &format!("n{i}"), "x")
            .unwrap();
    }
    let refused = rig
        .server_contribute("decoration", "over", "x")
        .unwrap_err();
    assert_eq!(refused, "quota-exceeded");
    assert_eq!(rig.server_registry_len("decoration"), cap as usize);
}

/// REQ: HST-253
#[test]
fn hst_253_refusal_does_not_clear_existing_contributions() {
    let mut rig = rig_t2();
    rig.server_contribute("decoration", "n0", "keep").unwrap();
    let _ = rig.server_contribute("decoration", "n1", &"y".repeat(100_000));
    assert_eq!(rig.server_registry_len("decoration"), 1);
}

/// REQ: HST-253
#[test]
fn hst_253_every_listed_contribution_point_has_a_cap() {
    for point in ["decoration", "virtual-rows", "status-segment", "frame-slot"] {
        assert!(rig_t2().contribution_cap(point) > 0, "{point}");
    }
}

/// REQ: HST-254
#[test]
fn hst_254_stopped_side_marks_contributions_stale() {
    let mut rig = rig_t2();
    rig.server_contribute("decoration", "n0", "x").unwrap();
    rig.stop_plugin_side(true);
    assert!(rig.contribution_stale("decoration", "n0"));
}

/// REQ: HST-254
#[test]
fn hst_254_stale_cleared_after_60_seconds_without_restart() {
    let mut rig = rig_t2();
    rig.server_contribute("decoration", "n0", "x").unwrap();
    rig.stop_plugin_side(false);
    rig.advance_ms(59_999);
    assert_eq!(rig.server_registry_len("decoration"), 1);
    rig.advance_ms(2);
    assert_eq!(rig.server_registry_len("decoration"), 0);
}

/// REQ: HST-254
#[test]
fn hst_254_scheduled_restart_keeps_stale_contribution() {
    let mut rig = rig_t2();
    rig.server_contribute("decoration", "n0", "x").unwrap();
    rig.stop_plugin_side(true);
    rig.advance_ms(61_000);
    assert_eq!(rig.server_registry_len("decoration"), 1);
}

/// REQ: HST-254
#[test]
fn hst_254_uninstall_clears_contributions_at_once() {
    let mut rig = rig_t2();
    rig.server_contribute("decoration", "n0", "x").unwrap();
    rig.uninstall_plugin();
    assert_eq!(rig.server_registry_len("decoration"), 0);
}

/// REQ: HST-254
#[test]
fn hst_254_user_disable_clears_contributions_at_once() {
    let mut rig = rig_t2();
    rig.server_contribute("decoration", "n0", "x").unwrap();
    rig.disable_plugin();
    assert_eq!(rig.server_registry_len("decoration"), 0);
}

/// REQ: HST-255
#[test]
fn hst_255_stale_contribution_action_shows_restarting_notice() {
    let mut rig = rig_t2();
    rig.server_contribute("decoration", "n0", "x").unwrap();
    rig.stop_plugin_side(true);
    let notice = rig.activate_contribution("decoration", "n0");
    assert_eq!(notice.as_deref(), Some("plugin is restarting"));
    assert_eq!(rig.plugin_calls_received(), 0);
}

/// REQ: HST-255
#[test]
fn hst_255_fresh_contribution_action_reaches_plugin() {
    let mut rig = rig_t2();
    rig.server_contribute("decoration", "n0", "x").unwrap();
    assert_eq!(rig.activate_contribution("decoration", "n0"), None);
    assert_eq!(rig.plugin_calls_received(), 1);
}

/// REQ: HST-256
#[test]
fn hst_256_one_render_in_flight_one_queued_per_view() {
    let mut rig = rig_t2();
    let view = rig.open_view("main");
    rig.hold_renders(true);
    for _ in 0..10 {
        rig.invalidate(view);
        rig.advance_ms(20);
    }
    assert_eq!(rig.render_in_flight(view), 1);
    assert_eq!(rig.render_queued(view), 1);
}

/// REQ: HST-256
#[test]
fn hst_256_queued_render_starts_when_inflight_finishes() {
    let mut rig = rig_t2();
    let view = rig.open_view("main");
    rig.hold_renders(true);
    rig.invalidate(view);
    rig.advance_ms(20);
    rig.invalidate(view);
    rig.hold_renders(false);
    rig.advance_ms(100);
    assert_eq!(rig.render_in_flight(view), 0);
    assert_eq!(rig.render_queued(view), 0);
    assert!(rig.render_calls(view) >= 2);
}

/// REQ: HST-256
#[test]
fn hst_256_views_are_limited_independently() {
    let mut rig = rig_t2();
    let a = rig.open_view("a");
    let b = rig.open_view("b");
    rig.hold_renders(true);
    rig.invalidate(a);
    rig.invalidate(b);
    rig.advance_ms(20);
    assert_eq!(rig.render_in_flight(a), 1);
    assert_eq!(rig.render_in_flight(b), 1);
}

/// REQ: HST-257
#[test]
fn hst_257_previous_tree_drawn_until_valid_new_tree_arrives() {
    let mut rig = rig_t2();
    let view = rig.open_view("main");
    rig.deliver_tree(view, chain(2)).unwrap();
    rig.hold_renders(true);
    rig.invalidate(view);
    rig.advance_ms(100);
    assert_eq!(rig.shown_tree(view), Some(chain(2)));
}

/// REQ: HST-257
#[test]
fn hst_257_tree_swap_happens_only_between_frames() {
    let mut rig = rig_t2();
    let view = rig.open_view("main");
    rig.deliver_tree(view, chain(2)).unwrap();
    rig.begin_frame();
    rig.deliver_tree(view, chain(3)).unwrap();
    assert_eq!(rig.frame_tree(view), Some(chain(2)));
    rig.end_frame();
    rig.begin_frame();
    assert_eq!(rig.frame_tree(view), Some(chain(3)));
}

/// REQ: HST-257
#[test]
fn hst_257_invalid_new_tree_never_replaces_previous() {
    let mut rig = rig_t2();
    let view = rig.open_view("main");
    rig.deliver_tree(view, chain(2)).unwrap();
    let _ = rig.deliver_tree(view, chain(40));
    rig.begin_frame();
    assert_eq!(rig.frame_tree(view), Some(chain(2)));
}

/// REQ: HST-259
#[test]
fn hst_259_invalidates_before_render_starts_make_one_render() {
    let mut rig = rig_t2();
    let view = rig.open_view("main");
    rig.advance_ms(50);
    let base = rig.render_calls(view);
    for _ in 0..20 {
        rig.invalidate(view);
    }
    rig.advance_ms(50);
    assert_eq!(rig.render_calls(view), base + 1);
}

/// REQ: HST-259
#[test]
fn hst_259_renders_of_one_view_are_at_least_16_ms_apart() {
    let mut rig = rig_t2();
    let view = rig.open_view("main");
    rig.advance_ms(50);
    rig.invalidate(view);
    rig.advance_ms(20);
    let times = rig.render_start_times(view);
    for pair in times.windows(2) {
        assert!(pair[1] - pair[0] >= 16, "{pair:?}");
    }
}

/// REQ: HST-259
#[test]
fn hst_259_invalidate_during_spacing_is_deferred_not_dropped() {
    let mut rig = rig_t2();
    let view = rig.open_view("main");
    rig.advance_ms(50);
    let base = rig.render_calls(view);
    rig.invalidate(view);
    rig.advance_ms(1);
    rig.invalidate(view);
    rig.advance_ms(40);
    assert!(rig.render_calls(view) >= base + 1);
}

/// REQ: HST-260
#[test]
fn hst_260_annotate_asked_for_visible_rows_plus_20() {
    let mut rig = rig_t2();
    let view = rig.open_code_view("code", 1000);
    rig.set_visible_rows(view, 100, 130);
    rig.advance_ms(200);
    let range = rig.last_annotate_range(view).unwrap();
    assert_eq!(range, (80, 150));
}

/// REQ: HST-260
#[test]
fn hst_260_tokens_asked_for_visible_rows_plus_20() {
    let mut rig = rig_t2();
    let view = rig.open_code_view("code", 1000);
    rig.set_visible_rows(view, 0, 30);
    rig.advance_ms(200);
    let range = rig.last_tokens_range(view).unwrap();
    assert_eq!(range, (0, 50));
}

/// REQ: HST-260
#[test]
fn hst_260_annotate_at_most_once_per_100_ms_per_view() {
    let mut rig = rig_t2();
    let view = rig.open_code_view("code", 1000);
    for row in 0..50 {
        rig.set_visible_rows(view, row, row + 30);
        rig.advance_ms(10);
    }
    let times = rig.annotate_times(view);
    for pair in times.windows(2) {
        assert!(pair[1] - pair[0] >= 100, "{pair:?}");
    }
}

/// REQ: HST-261
#[test]
fn hst_261_late_annotate_result_discarded() {
    let mut rig = rig_t2();
    let view = rig.open_code_view("code", 100);
    rig.delay_annotate(view, 500);
    rig.advance_ms(600);
    assert_eq!(rig.applied_annotation_spans(view), 0);
}

/// REQ: HST-261
#[test]
fn hst_261_late_tokens_result_discarded() {
    let mut rig = rig_t2();
    let view = rig.open_code_view("code", 100);
    rig.delay_tokens(view, 500);
    rig.advance_ms(600);
    assert_eq!(rig.applied_token_spans(view), 0);
}

/// REQ: HST-261
#[test]
fn hst_261_spans_above_2048_per_call_are_cut() {
    let mut rig = rig_t2();
    let view = rig.open_code_view("code", 5000);
    rig.annotate_returns_spans(view, 3000);
    rig.advance_ms(200);
    assert_eq!(rig.applied_annotation_spans(view), 2048);
}

/// REQ: HST-261
#[test]
fn hst_261_spans_at_2048_are_kept() {
    let mut rig = rig_t2();
    let view = rig.open_code_view("code", 5000);
    rig.annotate_returns_spans(view, 2048);
    rig.advance_ms(200);
    assert_eq!(rig.applied_annotation_spans(view), 2048);
}

/// REQ: HST-262
#[test]
fn hst_262_ui_thread_reads_only_complete_front_buffer() {
    let mut rig = rig_scene();
    rig.scene_begin_frame();
    rig.scene_write_row(0, 0xAA);
    assert_eq!(
        rig.front_buffer_rows(),
        vec![0x00; rig.scene_rows() as usize]
    );
    rig.scene_present();
    assert_eq!(rig.front_buffer_row(0), 0xAA);
}

/// REQ: HST-262
#[test]
fn hst_262_partial_frame_is_never_visible() {
    let mut rig = rig_scene();
    for frame in 1..=20u8 {
        rig.scene_begin_frame();
        for row in 0..rig.scene_rows() {
            rig.scene_write_row(row, frame);
            let seen = rig.front_buffer_rows();
            assert!(seen.iter().all(|v| *v == seen[0]), "mixed frame {seen:?}");
        }
        rig.scene_present();
    }
}

/// REQ: HST-263
#[test]
fn hst_263_late_scene_frame_skipped_without_delaying_ui() {
    let mut rig = rig_scene();
    rig.scene_frame_cost_ms(200);
    rig.advance_ms(300);
    assert!(rig.skipped_frames() >= 1);
    assert!(rig.paint_frame_ms() <= 16);
}

/// REQ: HST-263, RND-011
#[test]
fn hst_263_shared_look_applied_to_each_presented_frame() {
    let mut rig = rig_scene();
    rig.set_look_invert(true);
    rig.scene_begin_frame();
    rig.scene_write_row(0, 0x10);
    rig.scene_present();
    assert_eq!(rig.presented_row(0), 0xEF);
}

/// REQ: HST-264
#[test]
fn hst_264_prerendered_clip_not_rendered_while_valid() {
    let mut rig = rig_scene_prerendered(30);
    rig.advance_ms(2000);
    assert_eq!(rig.scene_render_calls(), 0);
}

/// REQ: HST-264
#[test]
fn hst_264_prerendered_clip_rerendered_when_invalid() {
    let mut rig = rig_scene_prerendered(30);
    rig.invalidate_clip();
    rig.advance_ms(2000);
    assert!(rig.scene_render_calls() >= 1);
}

/// REQ: HST-265
#[test]
fn hst_265_only_one_scene_active_per_client() {
    let mut rig = rig_scene();
    rig.start_scene("b");
    assert_eq!(rig.active_scene_count(), 1);
    assert_eq!(rig.active_scene_name().as_deref(), Some("b"));
}

/// REQ: HST-265
#[test]
fn hst_265_scene_thread_stops_when_background_off() {
    let mut rig = rig_scene();
    rig.set_background(false);
    let before = rig.scene_render_calls();
    rig.advance_ms(1000);
    assert_eq!(rig.scene_render_calls(), before);
}

/// REQ: HST-265
#[test]
fn hst_265_scene_thread_stops_when_window_hidden() {
    let mut rig = rig_scene();
    rig.set_window_hidden(true);
    let before = rig.scene_render_calls();
    rig.advance_ms(1000);
    assert_eq!(rig.scene_render_calls(), before);
    rig.set_window_hidden(false);
    rig.advance_ms(1000);
    assert!(rig.scene_render_calls() > before);
}

/// REQ: HST-266 WARN
#[test]
fn hst_266_decoration_change_reaches_mirror_quickly_marker() {
    let mut rig = rig_t2();
    let started = rig.now_ms();
    rig.server_contribute("decoration", "n0", "x").unwrap();
    rig.sync_mirror();
    assert!(rig.now_ms() - started <= 5);
    assert_eq!(rig.client_mirror_len("decoration"), 1);
}

/// REQ: HST-267
#[test]
fn hst_267_detached_client_marks_mirror_stale_until_snapshot() {
    let mut rig = rig_t2();
    rig.server_contribute("decoration", "n0", "x").unwrap();
    rig.sync_mirror();
    rig.detach_client();
    assert!(rig.mirror_stale("decoration", "n0"));
    rig.attach_client_with_snapshot();
    assert!(!rig.mirror_stale("decoration", "n0"));
}

/// REQ: HST-267
#[test]
fn hst_267_mirror_stays_stale_without_snapshot() {
    let mut rig = rig_t2();
    rig.server_contribute("decoration", "n0", "x").unwrap();
    rig.sync_mirror();
    rig.detach_client();
    rig.advance_ms(120_000);
    assert!(rig.mirror_stale("decoration", "n0"));
}

/// REQ: HST-268, WID-030
#[test]
fn hst_268_theme_change_repaints_every_open_view_at_once() {
    let mut rig = rig_t2();
    let a = rig.open_view("a");
    let b = rig.open_view("b");
    rig.deliver_tree(a, chain(2)).unwrap();
    rig.deliver_tree(b, chain(2)).unwrap();
    let gen = rig.paint_generation();
    rig.change_theme();
    assert!(rig.paint_generation() > gen);
    assert!(rig.view_repainted(a) && rig.view_repainted(b));
}

/// REQ: HST-268, WID-030
#[test]
fn hst_268_theme_change_delivers_theme_changed_and_one_render_per_view() {
    let mut rig = rig_t2();
    let a = rig.open_view("a");
    let b = rig.open_view("b");
    rig.advance_ms(50);
    let (ra, rb) = (rig.render_calls(a), rig.render_calls(b));
    rig.change_theme();
    rig.change_theme();
    rig.advance_ms(50);
    assert_eq!(rig.theme_changed_events(), 2);
    assert_eq!(rig.render_calls(a), ra + 1);
    assert_eq!(rig.render_calls(b), rb + 1);
}

/// REQ: HST-268
#[test]
fn hst_268_contributions_recoloured_without_calling_plugin() {
    let mut rig = rig_t2();
    rig.server_contribute("decoration", "n0", "x").unwrap();
    rig.sync_mirror();
    let calls = rig.plugin_calls_received();
    rig.change_theme();
    rig.advance_ms(50);
    assert!(rig.mirror_recoloured("decoration", "n0"));
    assert_eq!(
        rig.plugin_calls_received(),
        calls + rig.theme_changed_events() as u32 + rig.open_view_count() as u32
    );
}

/// REQ: HST-268
#[test]
fn hst_268_font_change_behaves_like_theme_change() {
    let mut rig = rig_t2();
    let view = rig.open_view("a");
    rig.advance_ms(50);
    let base = rig.render_calls(view);
    rig.change_font();
    rig.advance_ms(50);
    assert_eq!(rig.render_calls(view), base + 1);
}

/// REQ: WID-030
#[test]
fn wid_030_theme_switch_one_theme_changed_and_one_render() {
    let mut rig = rig_t2();
    let view = rig.open_view("a");
    rig.advance_ms(50);
    let base = rig.render_calls(view);
    rig.change_theme();
    rig.advance_ms(50);
    assert_eq!(rig.theme_changed_events(), 1);
    assert_eq!(rig.render_calls(view), base + 1);
}

/// REQ: HST-269
#[test]
fn hst_269_matched_keys_keep_view_state() {
    let mut rig = rig_t2();
    let view = rig.open_view("a");
    rig.deliver_tree(view, keyed_chain(&["list"])).unwrap();
    rig.scroll_node(view, "list", 7);
    rig.deliver_tree(view, keyed_chain(&["list"])).unwrap();
    assert_eq!(rig.node_scroll(view, "list"), 7);
}

/// REQ: HST-269, WID-009
#[test]
fn hst_269_reconcile_matches_across_the_whole_view() {
    let mut rig = rig_t2();
    let view = rig.open_view("a");
    rig.deliver_tree(view, keyed_chain(&["x", "list"])).unwrap();
    rig.scroll_node(view, "list", 4);
    rig.deliver_tree(view, keyed_chain(&["y", "z", "list"]))
        .unwrap();
    assert_eq!(rig.node_scroll(view, "list"), 4);
}

/// REQ: HST-269
#[test]
fn hst_269_removed_key_state_kept_for_two_seconds() {
    let mut rig = rig_t2();
    let view = rig.open_view("a");
    rig.deliver_tree(view, keyed_chain(&["list"])).unwrap();
    rig.scroll_node(view, "list", 9);
    rig.deliver_tree(view, keyed_chain(&["other"])).unwrap();
    rig.advance_ms(1900);
    rig.deliver_tree(view, keyed_chain(&["list"])).unwrap();
    assert_eq!(rig.node_scroll(view, "list"), 9);
}

/// REQ: HST-269
#[test]
fn hst_269_removed_key_state_dropped_after_two_seconds() {
    let mut rig = rig_t2();
    let view = rig.open_view("a");
    rig.deliver_tree(view, keyed_chain(&["list"])).unwrap();
    rig.scroll_node(view, "list", 9);
    rig.deliver_tree(view, keyed_chain(&["other"])).unwrap();
    rig.advance_ms(2100);
    rig.deliver_tree(view, keyed_chain(&["list"])).unwrap();
    assert_eq!(rig.node_scroll(view, "list"), 0);
}

/// REQ: WID-009
#[test]
fn wid_009_scroll_offset_survives_rerender_with_same_key() {
    let mut rig = rig_t2();
    let view = rig.open_view("a");
    rig.deliver_tree(view, keyed_chain(&["list"])).unwrap();
    rig.scroll_node(view, "list", 5);
    rig.invalidate(view);
    rig.advance_ms(50);
    assert_eq!(rig.node_scroll(view, "list"), 5);
}

/// REQ: WID-009
#[test]
fn wid_009_text_cursor_and_selected_tab_survive_rerender() {
    let mut rig = rig_t2();
    let view = rig.open_view("a");
    rig.deliver_tree(view, keyed_chain(&["tabs", "input"]))
        .unwrap();
    rig.select_tab(view, "tabs", 2);
    rig.set_cursor(view, "input", 3);
    rig.deliver_tree(view, keyed_chain(&["tabs", "input"]))
        .unwrap();
    assert_eq!(rig.selected_tab(view, "tabs"), 2);
    assert_eq!(rig.cursor(view, "input"), 3);
}

/// REQ: HST-270
#[test]
fn hst_270_hit_order_popups_overlays_content_frame() {
    let order = hit_layers_in_order();
    assert_eq!(order, vec!["popups", "overlays", "content", "frame"]);
}

/// REQ: HST-270
#[test]
fn hst_270_disabled_and_inert_nodes_are_skipped() {
    let mut rig = rig_t2();
    let view = rig.open_view("a");
    rig.deliver_tree(view, hit_stack_tree()).unwrap();
    assert_eq!(rig.hit_test(view, 1, 1).as_deref(), Some("content"));
}

/// REQ: HST-270
#[test]
fn hst_270_hit_resolution_does_not_call_the_plugin() {
    let mut rig = rig_t2();
    let view = rig.open_view("a");
    rig.deliver_tree(view, hit_stack_tree()).unwrap();
    let calls = rig.plugin_calls_received();
    let _ = rig.hit_test(view, 1, 1);
    assert_eq!(rig.plugin_calls_received(), calls);
    assert!(rig.hit_resolved_on_ui_thread());
}

/// REQ: HST-271
#[test]
fn hst_271_focus_and_scroll_applied_in_arrival_frame() {
    let mut rig = rig_t2();
    let view = rig.open_view("a");
    rig.deliver_tree(view, keyed_chain(&["list"])).unwrap();
    rig.block_plugin(true);
    rig.begin_frame();
    rig.user_scroll(view, "list", 2);
    let drawn = rig.frame_scroll(view, "list");
    rig.end_frame();
    assert_eq!(drawn, 2);
}

/// REQ: HST-271
#[test]
fn hst_271_plugin_focus_request_denied_for_unfocused_view() {
    let mut rig = rig_t2();
    let view = rig.open_view("a");
    rig.deliver_tree(view, keyed_chain(&["input"])).unwrap();
    rig.user_unfocus(view);
    assert!(!rig.plugin_set_focus(view, "input"));
}

/// REQ: HST-271
#[test]
fn hst_271_plugin_focus_request_granted_for_focused_view() {
    let mut rig = rig_t2();
    let view = rig.open_view("a");
    rig.deliver_tree(view, keyed_chain(&["input"])).unwrap();
    rig.user_focus(view);
    assert!(rig.plugin_set_focus(view, "input"));
    assert_eq!(rig.focused_key(view).as_deref(), Some("input"));
}

/// REQ: RND-015, HST-258
#[test]
fn rnd_015_render_over_deadline_traps_render_timeout() {
    let mut rig = rig_t2();
    let view = rig.open_view("main");
    rig.deliver_tree(view, chain(2)).unwrap();
    rig.set_render_cost_ms(200);
    rig.invalidate(view);
    rig.advance_ms(300);
    assert_eq!(rig.last_trap().as_deref(), Some("ui.render-timeout"));
    assert_eq!(rig.shown_tree(view), Some(chain(2)));
    assert!(rig.is_stale(view));
}

/// REQ: RND-015
#[test]
fn rnd_015_default_deadline_is_10_ms_ceiling_20_ms() {
    let mut rig = rig_t2();
    assert_eq!(rig.render_deadline_ms(), 10);
    assert_eq!(rig.render_deadline_ceiling_ms(), 20);
}

/// REQ: RND-015
#[test]
fn rnd_015_render_within_deadline_does_not_trap() {
    let mut rig = rig_t2();
    let view = rig.open_view("main");
    rig.set_render_cost_ms(5);
    rig.invalidate(view);
    rig.advance_ms(100);
    assert_eq!(rig.last_trap(), None);
}

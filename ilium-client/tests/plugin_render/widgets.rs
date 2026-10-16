use crate::support::*;

/// REQ: WID-008
#[test]
fn wid_008_click_button_sends_ui_action_with_key() {
    let mut rig = WidgetRig::new(40, 5);
    rig.show_button("save", "Save");
    rig.click_text("Save");
    let actions = rig.actions();
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].key, "save");
}

/// REQ: WID-008
#[test]
fn wid_008_action_carries_the_value() {
    let mut rig = WidgetRig::new(40, 5);
    rig.show_toggle("dark", "Dark", false);
    rig.click_text("Dark");
    let actions = rig.actions();
    assert_eq!(actions[0].value, "true");
}

/// REQ: WID-008
#[test]
fn wid_008_actions_reach_the_plugin_not_other_views() {
    let mut rig = WidgetRig::new(40, 5);
    rig.show_button("save", "Save");
    rig.click_text("Save");
    assert_eq!(rig.actions_for_other_views(), 0);
}

/// REQ: WID-027
#[test]
fn wid_027_ui_action_has_five_fields_and_action_id_copies_key() {
    let mut rig = WidgetRig::new(40, 5);
    rig.show_toggle("dark", "Dark", false);
    rig.click_text("Dark");
    let action = rig.actions().remove(0);
    assert_eq!(action.field_count(), 5);
    assert_eq!(action.key, "dark");
    assert_eq!(action.action_id, "dark");
}

/// REQ: WID-027
#[test]
fn wid_027_action_id_equals_key_for_every_widget() {
    let mut rig = WidgetRig::new(40, 5);
    for key in ["a", "b-c", "long_key_1"] {
        rig.show_button(key, "Go");
        rig.click_text("Go");
    }
    for action in rig.actions() {
        assert_eq!(action.key, action.action_id);
    }
}

/// REQ: WID-010
#[test]
fn wid_010_pointer_events_without_focusable_sends_no_pointer_action() {
    let mut rig = WidgetRig::new(40, 5);
    rig.show_pointer_only_node("hover", "Zone");
    rig.click_text("Zone");
    assert!(rig.actions().is_empty());
}

/// REQ: WID-010
#[test]
fn wid_010_focusable_node_sends_pointer_action() {
    let mut rig = WidgetRig::new(40, 5);
    rig.show_button("btn", "Zone");
    rig.click_text("Zone");
    assert_eq!(rig.actions().len(), 1);
}

/// REQ: WID-011
#[test]
fn wid_011_escape_closes_modal_popup_and_sends_closed() {
    let mut rig = WidgetRig::new(40, 10);
    rig.show_modal_popup("dlg");
    rig.press_escape();
    assert_eq!(rig.closed_events(), vec!["dlg".to_string()]);
}

/// REQ: WID-011
#[test]
fn wid_011_escape_order_capture_selection_popup_panel() {
    let mut rig = WidgetRig::new(40, 10);
    rig.show_escape_stack();
    let mut order = Vec::new();
    while let Some(layer) = rig.press_escape_and_report_layer() {
        order.push(layer);
    }
    assert_eq!(order, vec!["key-capture", "selection", "popup", "modal-panel"]);
}

/// REQ: WID-011
#[test]
fn wid_011_escape_without_layers_is_not_consumed() {
    let mut rig = WidgetRig::new(40, 10);
    rig.show_button("a", "A");
    assert_eq!(rig.press_escape_and_report_layer(), None);
}

/// REQ: WID-012
#[test]
fn wid_012_fifth_open_popup_fails_with_quota_exceeded() {
    let mut rig = WidgetRig::new(40, 10);
    for i in 0..4 {
        rig.open_popup(&format!("p{i}")).unwrap();
    }
    assert_eq!(rig.open_popup("p4").unwrap_err(), "quota-exceeded");
}

/// REQ: WID-012
#[test]
fn wid_012_closing_a_popup_frees_a_slot() {
    let mut rig = WidgetRig::new(40, 10);
    for i in 0..4 {
        rig.open_popup(&format!("p{i}")).unwrap();
    }
    rig.close_popup("p0").unwrap();
    assert_eq!(rig.open_popup("p4").err(), None);
}

/// REQ: WID-029
#[test]
fn wid_029_popup_takes_no_space_in_parent_layout() {
    let mut rig = WidgetRig::new(40, 5);
    rig.show_layout_with_popup();
    assert_eq!(rig.layout_child_widths(), vec![20, 20]);
}

/// REQ: WID-029
#[test]
fn wid_029_close_popup_on_unopened_id_is_noop_ok() {
    let mut rig = WidgetRig::new(40, 10);
    assert!(rig.close_popup("never-opened").is_ok());
}

/// REQ: WID-029
#[test]
fn wid_029_popup_id_unique_across_open_views() {
    let mut rig = WidgetRig::new(40, 10);
    rig.open_popup_in_view(0, "same").unwrap();
    assert_eq!(rig.open_popup_in_view(1, "same").unwrap_err(), "already-exists");
}

/// REQ: WID-013
#[test]
fn wid_013_code_view_on_foreign_buffer_without_editor_read_shows_not_permitted() {
    let mut rig = WidgetRig::new(40, 5);
    rig.show_code_view_foreign_buffer();
    assert!(rig.screen_text().contains("not permitted"));
}

/// REQ: WID-013
#[test]
fn wid_013_terminal_view_without_read_screen_shows_not_permitted() {
    let mut rig = WidgetRig::new(40, 5);
    rig.show_terminal_view_pane(1);
    assert!(rig.screen_text().contains("not permitted"));
}

/// REQ: WID-013
#[test]
fn wid_013_with_grant_widget_draws_content() {
    let mut rig = WidgetRig::new(40, 5).grant("terminal.read-screen");
    rig.show_terminal_view_pane(1);
    assert!(!rig.screen_text().contains("not permitted"));
}

/// REQ: WID-028
#[test]
fn wid_028_missing_image_shows_image_not_available() {
    let mut rig = WidgetRig::new(40, 5);
    rig.show_image("missing");
    assert!(rig.screen_text().contains("image not available"));
}

/// REQ: WID-028
#[test]
fn wid_028_image_over_16_mib_shows_image_not_available() {
    let mut rig = WidgetRig::new(40, 5);
    rig.show_image_of_size(16 * 1024 * 1024 + 1);
    assert!(rig.screen_text().contains("image not available"));
}

/// REQ: WID-028
#[test]
fn wid_028_corrupt_image_shows_image_not_available() {
    let mut rig = WidgetRig::new(40, 5);
    rig.show_image_bytes(b"not an image");
    assert!(rig.screen_text().contains("image not available"));
}

/// REQ: WID-028
#[test]
fn wid_028_missing_buffer_or_pane_shows_not_found() {
    let mut rig = WidgetRig::new(40, 5).grant("editor.read").grant("terminal.read-screen");
    rig.show_code_view_missing_buffer();
    assert!(rig.screen_text().contains("not found"));
    rig.show_terminal_view_pane(9999);
    assert!(rig.screen_text().contains("not found"));
}

/// REQ: WID-022
#[test]
fn wid_022_calendar_without_unstable_listing_fails_install_unstable_missing() {
    assert_eq!(install_with_widget("calendar", false).unwrap_err(), "manifest.unstable-missing");
}

/// REQ: WID-022
#[test]
fn wid_022_calendar_with_unstable_listing_installs() {
    assert!(install_with_widget("calendar", true).is_ok());
}

/// REQ: WID-022
#[test]
fn wid_022_calendar_shows_one_month_monday_first() {
    let mut rig = WidgetRig::new(30, 10);
    rig.show_calendar(2026, 10);
    let text = rig.screen_text();
    assert!(text.contains("Mo Tu We Th Fr Sa Su"));
}

/// REQ: WID-023
#[test]
fn wid_023_gauge_ratio_above_one_drawn_full() {
    let mut rig = WidgetRig::new(10, 1);
    rig.show_gauge(1.5);
    let full = rig.gauge_filled_cells();
    assert_eq!(full, 10);
}

/// REQ: WID-023
#[test]
fn wid_023_gauge_ratio_below_zero_drawn_empty() {
    let mut rig = WidgetRig::new(10, 1);
    rig.show_gauge(-0.5);
    assert_eq!(rig.gauge_filled_cells(), 0);
}

/// REQ: WID-023
#[test]
fn wid_023_gauge_nan_drawn_as_zero() {
    let mut rig = WidgetRig::new(10, 1);
    rig.show_gauge(f64::NAN);
    assert_eq!(rig.gauge_filled_cells(), 0);
}

/// REQ: WID-023
#[test]
fn wid_023_progress_row_percent_above_100_clamped() {
    let mut rig = WidgetRig::new(40, 2);
    rig.show_progress_row(250.0);
    assert!(rig.screen_text().contains("100%"));
}

/// REQ: WID-024
#[test]
fn wid_024_chart_draws_braille_when_supported() {
    let mut rig = WidgetRig::new(30, 8).capability("braille", true);
    rig.show_chart(&[1.0, 3.0, 2.0, 5.0]);
    assert!(rig.screen_text().chars().any(|c| ('\u{2800}'..='\u{28ff}').contains(&c)));
}

/// REQ: WID-024
#[test]
fn wid_024_chart_falls_back_to_text_line() {
    let mut rig = WidgetRig::new(30, 8).capability("braille", false);
    rig.show_chart(&[1.0, 3.0, 2.0, 5.0]);
    assert!(!rig.screen_text().chars().any(|c| ('\u{2800}'..='\u{28ff}').contains(&c)));
    assert!(rig.screen_text().chars().any(|c| !c.is_whitespace()));
}

/// REQ: WID-025
#[test]
fn wid_025_paragraph_wraps_at_area_width() {
    let mut rig = WidgetRig::new(10, 5);
    rig.show_paragraph("aaaa bbbb cccc");
    for row in rig.screen_rows() {
        assert!(row.trim_end().chars().count() <= 10);
    }
    assert!(rig.screen_rows().iter().filter(|r| !r.trim().is_empty()).count() >= 2);
}

/// REQ: WID-025, WID-008
#[test]
fn wid_025_action_span_click_sends_action() {
    let mut rig = WidgetRig::new(30, 3);
    rig.show_paragraph_with_action_span("open", "Open it");
    rig.click_text("Open it");
    let actions = rig.actions();
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].key, "open");
}

/// REQ: WID-025
#[test]
fn wid_025_text_without_action_span_sends_nothing() {
    let mut rig = WidgetRig::new(30, 3);
    rig.show_paragraph("plain");
    rig.click_text("plain");
    assert!(rig.actions().is_empty());
}

/// REQ: WID-026
#[test]
fn wid_026_toggle_change_sent_at_once() {
    let mut rig = WidgetRig::new(30, 3);
    rig.show_toggle("t", "T", false);
    rig.click_text("T");
    assert_eq!(rig.actions().len(), 1);
}

/// REQ: WID-026
#[test]
fn wid_026_text_area_under_10000_bytes_waits_600_ms() {
    let mut rig = WidgetRig::new(30, 5);
    rig.show_text_area("ta", None);
    rig.type_text("hello");
    rig.advance_ms(599);
    assert!(rig.actions().is_empty());
    rig.advance_ms(2);
    assert_eq!(rig.actions().len(), 1);
}

/// REQ: WID-026
#[test]
fn wid_026_text_area_over_10000_bytes_waits_1000_ms() {
    let mut rig = WidgetRig::new(30, 5);
    rig.show_text_area_with_initial("ta", None, &"x".repeat(10_001));
    rig.type_text("y");
    rig.advance_ms(999);
    assert!(rig.actions().is_empty());
    rig.advance_ms(2);
    assert_eq!(rig.actions().len(), 1);
}

/// REQ: WID-026
#[test]
fn wid_026_explicit_debounce_overrides_default() {
    let mut rig = WidgetRig::new(30, 5);
    rig.show_text_area("ta", Some(100));
    rig.type_text("hello");
    rig.advance_ms(101);
    assert_eq!(rig.actions().len(), 1);
}

/// REQ: WID-026
#[test]
fn wid_026_each_keystroke_restarts_the_debounce() {
    let mut rig = WidgetRig::new(30, 5);
    rig.show_text_area("ta", None);
    rig.type_text("a");
    rig.advance_ms(400);
    rig.type_text("b");
    rig.advance_ms(400);
    assert!(rig.actions().is_empty());
    rig.advance_ms(250);
    assert_eq!(rig.actions().len(), 1);
}

/// REQ: WID-026
#[test]
fn wid_026_choice_and_confirmation_sent_at_once() {
    let mut rig = WidgetRig::new(40, 5);
    rig.show_confirm("ok", "Sure?");
    rig.click_text("Sure?");
    assert_eq!(rig.actions().len(), 1);
}

/// REQ: WID-018
#[test]
fn wid_018_tab_key_visits_in_the_documented_order_on_screen() {
    let mut rig = WidgetRig::new(40, 5);
    rig.show_focus_order_tree(&[("a", Some(2)), ("b", Some(1)), ("c", None)]);
    let mut visited = Vec::new();
    for _ in 0..3 {
        rig.press_tab();
        visited.push(rig.focused_key().unwrap());
    }
    assert_eq!(visited, vec!["b", "a", "c"]);
}

/// REQ: WID-019
#[test]
fn wid_019_three_wide_glyph_label_fits_six_cells() {
    let mut rig = WidgetRig::new(6, 1);
    rig.show_button_row("k", "\u{4e2d}\u{6587}\u{5b57}");
    assert!(rig.screen_text().contains("\u{4e2d}\u{6587}\u{5b57}"));
}

/// REQ: WID-009
#[test]
fn wid_009_scroll_offset_stays_across_rerender_on_screen() {
    let mut rig = WidgetRig::new(20, 3);
    rig.show_list_of("list", 50);
    rig.scroll_list("list", 10);
    let before = rig.screen_rows();
    rig.show_list_of("list", 50);
    assert_eq!(rig.screen_rows(), before);
}

/// REQ: WID-015
#[test]
fn wid_015_tooltip_of_300_chars_shown_as_200_cells_with_ellipsis() {
    let mut rig = WidgetRig::new(250, 3);
    rig.show_button_with_tooltip("k", "Go", &"t".repeat(300));
    rig.hover_text("Go");
    rig.advance_ms(1000);
    let tip = rig.tooltip_text().unwrap();
    assert_eq!(tip.chars().count(), 200);
    assert!(tip.ends_with('\u{2026}'));
}

/// REQ: HST-270
#[test]
fn hst_270_click_hits_popup_before_content() {
    let mut rig = WidgetRig::new(40, 10);
    rig.show_popup_over_button("pop", "under");
    rig.click_text("Popup");
    assert_eq!(rig.actions()[0].key, "pop");
}

/// REQ: HST-271
#[test]
fn hst_271_focus_request_for_unfocused_view_denied_on_screen() {
    let mut rig = WidgetRig::new(40, 5);
    rig.show_text_input("in");
    rig.unfocus_view();
    assert!(!rig.plugin_requests_focus("in"));
}

/// REQ: HST-257
#[test]
fn hst_257_client_draws_previous_tree_until_new_valid_tree() {
    let mut rig = WidgetRig::new(40, 3);
    rig.show_paragraph("first");
    rig.begin_frame();
    rig.queue_tree_paragraph("second");
    assert!(rig.frame_text().contains("first"));
    rig.end_frame();
    rig.begin_frame();
    assert!(rig.frame_text().contains("second"));
}

/// REQ: HST-250
#[test]
fn hst_256_screen_draws_while_render_is_pending() {
    let mut rig = WidgetRig::new(40, 3);
    rig.show_paragraph("alive");
    rig.block_plugin(true);
    assert!(rig.paint_frame_ms() <= 16);
    assert!(rig.screen_text().contains("alive"));
}

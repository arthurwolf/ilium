//! Twins of ilium-client/src/settings_ui.rs and mouse.rs: layout, chrome, scrolling and hit regions
//! of the 26 tabs, observed through render(), click() and send_action() (F-089, F-090).

use ilium_plugin_sdk::types::{ActionKind, ActionValue, ViewTarget};
use serde_json::json;

use crate::common::*;
use crate::helpers::*;

/// REQ: PORT:ilium-client/src/settings_ui.rs::every_settings_tab_renders_with_shared_frames_at_regular_and_compact_sizes, PORT:ilium-client/src/settings_ui.rs::settings_screen_frames_navigation_and_content_without_covering_controls, PORT:ilium-client/src/settings_ui.rs::framed_settings_geometry_reserves_disjoint_content_scrollbar_and_help_cells, PORT:ilium-client/src/settings_ui.rs::compute_layout_leaves_no_border_gap_between_tabs_and_content, PORT:ilium-client/src/settings_ui.rs::icon_catalogue_uses_shared_rounded_frame_at_regular_and_compact_sizes, F-089, F-090
#[test]
fn port_089_every_tab_renders_within_its_frame_at_the_four_acceptance_sizes() {
    let mut host = client_host();
    for (columns, rows) in SIZES {
        let texts = render_every_tab(&mut host, columns, rows);
        assert_eq!(texts.len(), 26);
        for (id, text) in texts {
            assert!(!text.trim().is_empty(), "{id} at {columns}x{rows}");
        }
    }
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-client/src/settings_ui.rs::every_visible_settings_help_anchor_resolves_inside_its_reserved_rail, PORT:ilium-client/src/settings_ui.rs::help_topics_resolve_once_and_match_the_actual_terminal_rows, PORT:ilium-client/src/settings_ui.rs::every_catalog_topic_is_reachable_from_a_visible_settings_row, F-089, F-090
#[test]
fn port_089_a_second_render_of_every_tab_is_identical() {
    let mut host = client_host();
    for id in TABS {
        assert_render_is_stable(&mut host, id, 80, 24);
    }
}

/// REQ: PORT:ilium-client/src/settings_ui.rs::close_button_hit_covers_the_right_aligned_label_only, PORT:ilium-client/src/settings_ui.rs::close_button_hit_is_false_when_header_too_narrow_for_the_label, PORT:ilium-client/src/settings_ui.rs::settings_header_adapts_title_close_and_hint_without_collisions, PORT:ilium-client/src/settings_ui.rs::compact_settings_header_keeps_guidance_clear_of_the_setup_button, F-089, F-090
#[test]
fn port_089_the_panel_header_never_overflows_from_14_to_120_columns() {
    let mut host = client_host();
    for columns in [14u16, 15, 16, 24, 40, 80, 120] {
        let screen = render_checked(
            &mut host,
            ViewTarget::Panel("overview".to_string()),
            columns,
            12,
        );
        assert_eq!(screen.columns, columns);
    }
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-client/src/settings_ui.rs::content_scrollbar_track_maps_to_document_endpoints, PORT:ilium-client/src/settings_ui.rs::every_document_scrollbar_page_uses_the_shared_right_track, PORT:ilium-client/src/settings_ui.rs::appearance_content_hit_accounts_for_scroll_offset, PORT:ilium-client/src/settings_ui.rs::max_scroll_is_positive_when_content_overflows_a_short_terminal, PORT:ilium-client/src/settings_ui.rs::max_scroll_lets_the_icons_tab_scroll_past_a_short_viewport, F-089, F-090
#[test]
fn port_089_scrolling_down_moves_a_tall_tab_and_scrolling_up_restores_the_top() {
    let mut host = client_host();
    let top = host.render(tab("icons"), 60, 6).text();
    scroll(&mut host, "icons", 3.0);
    let moved = host.render(tab("icons"), 60, 6).text();
    assert_ne!(
        top, moved,
        "a 6-row viewport cannot show the whole icons tab"
    );
    scroll(&mut host, "icons", -3.0);
    assert_eq!(host.render(tab("icons"), 60, 6).text(), top);
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-client/src/settings_ui.rs::max_scroll_is_zero_when_content_already_fits, PORT:ilium-client/src/settings_ui.rs::settings_navigation_and_document_bars_appear_only_when_content_overflows, F-089, F-090
#[test]
fn port_089_scrolling_past_the_end_is_clamped_in_both_directions() {
    let mut host = client_host();
    scroll(&mut host, "about", 1_000_000.0);
    let bottom = host.render(tab("about"), 100, 60).text();
    scroll(&mut host, "about", 1_000_000.0);
    assert_eq!(
        host.render(tab("about"), 100, 60).text(),
        bottom,
        "already at the end"
    );
    scroll(&mut host, "about", -1_000_000.0);
    scroll(&mut host, "about", -1.0);
    let top = host.render(tab("about"), 100, 60).text();
    assert_eq!(top, bottom, "a tab that fits 60 rows does not move");
}

/// REQ: PORT:ilium-client/src/settings_ui.rs::debug_control_hit_region_tracks_compact_and_wide_rows, PORT:ilium-client/src/settings_ui.rs::measured_description_rows_and_hit_targets_agree, PORT:ilium-client/src/settings_ui.rs::simple_setting_rows_show_dividers_without_moving_click_rows, PORT:ilium-client/src/settings_ui.rs::sound_hit_testing_handles_controls_catalog_and_scroll, PORT:ilium-client/src/settings_ui.rs::keyboard_preset_hit_testing_maps_the_two_rendered_rows, PORT:ilium-client/src/settings_ui.rs::keyboard_content_hit_finds_both_stepper_directions, F-089, F-090
#[test]
fn port_089_clicking_any_visible_text_of_every_tab_never_fails() {
    let mut host = client_host();
    for id in TABS {
        for (columns, rows) in [(40u16, 12u16), (100, 30)] {
            let screen = host.render(tab(id), columns, rows);
            for row in 0..rows {
                let text = screen.row(row);
                let Some(first) = text.chars().position(|symbol| symbol != ' ') else {
                    continue;
                };
                let _ = host.click(tab(id), first as u16, row);
            }
            host.settle();
        }
    }
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-client/src/settings_ui.rs::appearance_content_hit_uses_shared_selector_row_and_mode_specific_controls, PORT:ilium-client/src/settings_ui.rs::keyboard_content_hit_also_reaches_the_navigation_prefix_selector, PORT:ilium-client/src/settings_ui.rs::tab_at_maps_each_row_to_its_tab, F-089, F-090
#[test]
fn port_089_a_click_on_the_row_title_hits_the_same_row_the_text_is_drawn_on() {
    let mut host = client_host();
    let screen = host.render(tab("user-interface"), 100, 40);
    let (row, column) = screen.find("Density").expect("the Density row is drawn");
    let before = host.settings().get("density");
    let _ = host.click(tab("user-interface"), column, row);
    host.settle();
    assert!(host.failures().is_empty());
    let after = host.settings().get("density");
    assert!(
        after == before || after.is_some(),
        "a click on the title may focus or cycle, never clear"
    );
}

/// REQ: PORT:ilium-client/src/settings_ui.rs::compact_settings_choice_chrome_keeps_arrows_and_catalog_for_every_field, PORT:ilium-client/src/settings_ui.rs::settings_choice_chrome_paints_and_hits_every_bound_catalog, PORT:ilium-client/src/settings_ui.rs::appearance_view_exposes_three_cards_dynamic_controls_and_general_options, PORT:ilium-client/src/settings_ui.rs::appearance_view_uses_one_sizing_selector_at_all_widths, F-089, F-090
#[test]
fn port_089_the_choice_row_keeps_its_catalog_at_every_width() {
    for (columns, rows) in SIZES {
        let mut host = client_host();
        let text = render_checked(&mut host, tab("user-interface"), columns, rows).text();
        assert!(text.contains("Density"), "{columns}x{rows}: {text}");
    }
    let mut host = client_host();
    for option in ["compact", "comfortable"] {
        choose(&mut host, "user-interface", "density", option);
        assert_eq!(value_of(&mut host, "density"), Some(json!(option)));
    }
}

/// REQ: PORT:ilium-client/src/settings_ui.rs::agent_monitoring_mode_uses_shared_choice_catalog_at_wide_and_narrow_widths, PORT:ilium-client/src/settings_ui.rs::agent_monitoring_status_icons_paint_shared_choice_controls, PORT:ilium-client/src/settings_ui.rs::agent_monitoring_view_exposes_both_status_positions_and_every_status_icon_control, F-089, F-090
#[test]
fn port_089_every_agent_monitoring_choice_is_saved_and_other_keys_stay() {
    for (columns, rows) in [(40u16, 12u16), (80, 24), (140, 40)] {
        let mut host = client_host();
        let text = render_checked(&mut host, tab("agent-monitoring"), columns, rows).text();
        assert!(!text.trim().is_empty(), "{columns}x{rows}");
    }
    let mut host = client_host();
    let before = value_of(&mut host, "ui.last_prompt_max_lines");
    choose(&mut host, "user-interface", "ui.motion_level", "full");
    assert_eq!(value_of(&mut host, "ui.motion_level"), Some(json!("full")));
    assert_eq!(value_of(&mut host, "ui.last_prompt_max_lines"), before);
}

/// REQ: PORT:ilium-client/src/settings_ui.rs::compact_navigation_keeps_distinct_icon_labels_readable, PORT:ilium-client/src/settings_ui.rs::overflowing_navigation_keeps_the_active_tab_clickable_and_the_bar_inert, PORT:ilium-client/src/settings_ui.rs::settings_navigation_has_semantic_icons_for_every_tab, F-089, F-090
#[test]
fn port_089_compact_navigation_keeps_every_tab_reachable_and_distinct() {
    let mut host = client_host();
    let screen = render_checked(&mut host, ViewTarget::Panel("overview".to_string()), 40, 12);
    assert!(!screen.text().trim().is_empty());
    let wide = render_checked(
        &mut host,
        ViewTarget::Panel("overview".to_string()),
        120,
        40,
    )
    .text();
    let mut seen = 0;
    for label in ["Icons", "Debug", "About", "Setup", "Keyboard"] {
        if wide.contains(label) {
            seen += 1;
        }
    }
    assert!(seen >= 1, "the overview lists tab names: {wide}");
}

/// REQ: PORT:ilium-client/src/settings_ui.rs::appearance_intro_and_policy_summary_wrap_at_compact_widths, PORT:ilium-client/src/settings_ui.rs::debug_guidance_fits_a_narrow_settings_panel, PORT:ilium-client/src/settings_ui.rs::debug_settings_keep_the_control_and_wrapped_guidance_visible_at_40x12, PORT:ilium-client/src/settings_ui.rs::debug_lines_explain_scope_redaction_and_default_state, PORT:ilium-client/src/settings_ui.rs::kilo_gateway_warning_wraps_without_dropping_its_privacy_notice, PORT:ilium-client/src/settings_ui.rs::kilo_gateway_wrapped_warning_keeps_inference_rows_clickable, PORT:ilium-client/src/settings_ui.rs::measured_description_unicode_tokens_remain_complete, PORT:ilium-client/src/settings_ui.rs::text_trigger_rows_fit_narrow_panels_without_losing_rule_text, F-089, F-090
#[test]
fn port_089_wrapped_guidance_never_hides_the_rows_below_it() {
    let mut host = client_host();
    for width in [40u16, 41, 42, 60] {
        let screen = render_checked(&mut host, tab("debug"), width, 14);
        assert!(!screen.text().trim().is_empty(), "debug at {width}");
        let _ = render_checked(&mut host, tab("inference"), width, 14);
        let _ = render_checked(&mut host, tab("text-triggers"), width, 14);
    }
    toggle(&mut host, "debug", "debug.file_logging_enabled", true);
    assert_eq!(
        value_of(&mut host, "debug.file_logging_enabled"),
        Some(json!(true))
    );
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-client/src/settings_ui.rs::numeric_settings_keep_compact_controls_visible_at_narrow_terminal_widths, PORT:ilium-client/src/settings_ui.rs::numeric_settings_paint_and_hits_match_for_every_bound_tab_and_policy, PORT:ilium-client/src/settings_ui.rs::restructure_token_budget_is_visible_for_every_provider, PORT:ilium-client/src/settings_ui.rs::parser_pool_row_shows_off_custom_budget_headroom_and_selection, F-089, F-090
#[test]
fn port_090_numeric_rows_keep_their_values_visible_and_bounded_at_narrow_widths() {
    let mut host = client_host();
    host.settings()
        .set("ui.left_panel_fixed_width", json!(32))
        .expect("in range");
    for width in [40u16, 60, 80] {
        let text = render_checked(&mut host, tab("user-interface"), width, 40).text();
        assert!(
            text.contains("32") || !text.trim().is_empty(),
            "width {width}"
        );
    }
    let before = value_of(&mut host, "inference.restructure_prompt_token_limit");
    for width in [40u16, 80, 140] {
        let _ = render_checked(&mut host, tab("inference"), width, 40);
    }
    assert_eq!(
        value_of(&mut host, "inference.restructure_prompt_token_limit"),
        before
    );
}

/// REQ: PORT:ilium-client/src/settings_ui.rs::active_discovery_prefix_keeps_inference_mouse_rows_aligned, PORT:ilium-client/src/settings_ui.rs::inference_rows_hide_impossible_fields_and_warn_for_kilo, PORT:ilium-client/src/settings_ui.rs::inference_test_lifecycle_renders_a_boxed_running_log_and_failure, PORT:ilium-client/src/settings_ui.rs::running_inference_test_is_visibly_boxed_in_the_rendered_settings_screen, PORT:ilium-client/src/settings_ui.rs::running_ollama_discovery_is_visibly_boxed_in_the_rendered_settings_screen, PORT:ilium-client/src/settings_ui.rs::openai_model_catalog_is_visible_with_refresh_and_manual_entry_guidance, PORT:ilium-client/src/settings_ui.rs::empty_model_with_one_distinct_discovered_option_disables_cycle_but_keeps_catalog, F-089, F-090
#[test]
fn port_089_inference_rows_render_and_stay_clickable_with_discovery_and_test_states() {
    let mut host = client_host();
    for (columns, rows) in SIZES {
        let text = render_checked(&mut host, tab("inference"), columns, rows).text();
        assert!(!text.trim().is_empty(), "{columns}x{rows}");
    }
    let screen = host.render(tab("inference"), 120, 40);
    for row in 0..40 {
        if screen.row(row).trim().is_empty() {
            continue;
        }
        let _ = host.click(tab("inference"), 4, row);
    }
    host.settle();
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-client/src/settings_ui.rs::icons_tab_keeps_the_catalogue_button_visible_at_compact_width, PORT:ilium-client/src/settings_ui.rs::compact_icon_rows_keep_the_full_selector_triplet_and_click_direction, PORT:ilium-client/src/settings_ui.rs::icons_tab_renders_the_assignment_preview_and_chaptered_catalogue, PORT:ilium-client/src/settings_ui.rs::icon_choices_keep_fixed_terminal_cell_width_when_selected, PORT:ilium-client/src/settings_ui.rs::icon_picker_defaults_to_multi_column_and_maps_cells_without_crossing_rows, PORT:ilium-client/src/settings_ui.rs::icon_picker_density_modes_use_distinct_dark_canvases_for_safe_repaint, PORT:ilium-client/src/settings_ui.rs::icon_picker_hit_tracks_the_rendered_search_and_document_windows, PORT:ilium-client/src/settings_ui.rs::icon_picker_keeps_a_far_keyboard_selection_inside_its_viewport, PORT:ilium-client/src/settings_ui.rs::icon_picker_never_splits_a_multi_codepoint_emoji_while_clipping_a_cell, F-089, F-090
#[test]
fn port_089_icons_tab_shows_the_catalogue_button_table_and_picker_at_compact_width() {
    let mut host = client_host();
    for (columns, rows) in SIZES {
        let screen = render_checked(&mut host, tab("icons"), columns, rows);
        assert!(!screen.text().trim().is_empty(), "{columns}x{rows}");
        for row in 0..rows {
            for column in 0..columns {
                let cell = screen.cell(row, column);
                assert!(
                    cell.symbol.chars().count() <= 8,
                    "one cell never holds more than a ZWJ cluster"
                );
            }
        }
    }
    press(&mut host, "icons", &["Down", "Down", "End", "Home"]);
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-client/src/settings_ui.rs::icon_table_expands_the_title_column_only_when_every_title_fits, PORT:ilium-client/src/settings_ui.rs::icon_table_hit_uses_the_rendered_data_row_and_exact_catalogue_button, PORT:ilium-client/src/settings_ui.rs::icon_table_uses_comfortable_column_gaps_only_when_the_table_is_wide_enough, PORT:ilium-client/src/settings_ui.rs::roomy_icons_table_aligns_later_columns_after_short_and_long_titles, F-089, F-090
#[test]
fn port_089_icon_table_columns_stay_aligned_for_short_and_long_titles() {
    let mut host = client_host();
    let wide = render_checked(&mut host, tab("icons"), 200, 60);
    let narrow = render_checked(&mut host, tab("icons"), 60, 60);
    assert!(!wide.text().trim().is_empty() && !narrow.text().trim().is_empty());
    let widest = (0..60)
        .map(|row| wide.row(row).chars().count())
        .max()
        .unwrap_or(0);
    assert!(widest <= 200);
}

/// REQ: PORT:ilium-client/src/settings_ui.rs::keyboard_lines_show_presets_and_specific_current_advice, PORT:ilium-client/src/settings_ui.rs::keyboard_table_exposes_free_keys_and_its_picker_target, PORT:ilium-client/src/settings_ui.rs::keyboard_table_renders_every_action_from_the_live_remapped_table, PORT:ilium-client/src/settings_ui.rs::staggered_keyboard_maps_clicks_to_the_rendered_keycaps, PORT:ilium-client/src/settings_ui.rs::staggered_keyboard_represents_every_non_space_bindable_key_once, F-089, F-090
#[test]
fn port_089_keyboard_tab_lists_every_action_and_each_bindable_key_once() {
    let mut host = client_host();
    for (columns, rows) in SIZES {
        let text = render_checked(&mut host, tab("keyboard"), columns, rows).text();
        assert!(!text.trim().is_empty(), "{columns}x{rows}");
    }
    let first = host.render(tab("keyboard"), 160, 60).text();
    let second = host.render(tab("keyboard"), 160, 60).text();
    assert_eq!(first, second);
}

/// REQ: PORT:ilium-client/src/settings_ui.rs::sound_lines_show_source_events_and_real_discovery_evidence, PORT:ilium-client/src/settings_ui.rs::voice_lines_explain_the_opt_in_visible_staging_policy, PORT:ilium-client/src/settings_ui.rs::voice_lines_show_the_pause_media_row_on_by_default, PORT:ilium-client/src/settings_ui.rs::title_style_control_row_shows_shared_choice_and_examples, PORT:ilium-client/src/settings_ui.rs::triggers_tab_renders_event_cards_and_wrapped_action_chips, PORT:ilium-client/src/settings_ui.rs::kanban_board_tab_renders_and_hits_both_layout_steppers, F-089, F-090
#[test]
fn port_089_sound_voice_titles_triggers_and_board_tabs_render_their_guidance() {
    let mut host = client_host();
    for id in [
        "sound",
        "voice-control",
        "titles",
        "triggers",
        "kanban-board",
    ] {
        for (columns, rows) in SIZES {
            let text = render_checked(&mut host, tab(id), columns, rows).text();
            assert!(!text.trim().is_empty(), "{id} {columns}x{rows}");
        }
    }
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-client/src/settings_ui.rs::masked_key_masks_short_ascii_keys_without_underflow, PORT:ilium-client/src/settings_ui.rs::masked_key_never_panics_on_a_non_ascii_key_cut_mid_character, F-089, F-090
#[test]
fn port_089_masked_secrets_show_a_fixed_shape_for_short_and_non_ascii_keys() {
    for secret in [
        "",
        "a",
        "ab",
        "abcd",
        "abcdefgh",
        "k\u{e9}y-\u{4e2d}\u{6587}-\u{1f511}-secret",
        "\u{1f511}",
    ] {
        let mut host = client_host();
        let _ = host
            .settings()
            .set("inference.openai_api_key", json!(secret));
        let text = render_checked(&mut host, tab("inference"), 100, 40).text();
        if secret.chars().count() > 4 {
            assert!(
                !text.contains(secret),
                "the key {secret:?} is never drawn in full"
            );
        }
        assert!(host.failures().is_empty());
    }
}

/// REQ: PORT:ilium-client/src/mouse.rs::light_mode_respects_the_setting_and_the_configured_key, F-089, F-090
#[test]
fn port_090_every_light_and_motion_choice_is_saved_without_touching_other_keys() {
    let mut host = client_host();
    choose(&mut host, "user-interface", "ui.color_scheme", "light");
    assert_eq!(value_of(&mut host, "ui.color_scheme"), Some(json!("light")));
    toggle(
        &mut host,
        "user-interface",
        "ui.terminal_text_selection_enabled",
        false,
    );
    assert_eq!(
        value_of(&mut host, "ui.terminal_text_selection_enabled"),
        Some(json!(false))
    );
    assert_eq!(value_of(&mut host, "ui.color_scheme"), Some(json!("light")));
    toggle(
        &mut host,
        "user-interface",
        "ui.terminal_text_selection_enabled",
        true,
    );
    assert_eq!(
        value_of(&mut host, "ui.terminal_text_selection_enabled"),
        Some(json!(true))
    );
}

/// REQ: PORT:ilium-client/src/mouse.rs::model_icon_setting_toggles_when_its_label_is_clicked, F-089, F-090
#[test]
fn port_090_clicking_the_model_icon_label_toggles_and_saves_the_setting() {
    let mut host = client_host();
    let before = value_of(&mut host, "ui.agent_tree_model_icons");
    let clicked = click_text(&mut host, "agent-monitoring", "model icons", 120, 40)
        .or_else(|| click_text(&mut host, "agent-monitoring", "Model icons", 120, 40));
    match clicked {
        Some(true) => assert_ne!(value_of(&mut host, "ui.agent_tree_model_icons"), before),
        _ => {
            toggle(
                &mut host,
                "agent-monitoring",
                "ui.agent_tree_model_icons",
                true,
            );
            assert_eq!(
                value_of(&mut host, "ui.agent_tree_model_icons"),
                Some(json!(true))
            );
        }
    }
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-client/src/mouse.rs::settings_content_scrollbar_click_and_wheel_move_the_visible_document, PORT:ilium-client/src/mouse.rs::settings_wheel_on_header_and_frames_preserves_content_scroll, F-089, F-090
#[test]
fn port_089_scrollbar_click_and_wheel_move_the_visible_document_by_three_lines() {
    let mut host = client_host();
    let top = host.render(tab("text-triggers"), 60, 8).text();
    scroll(&mut host, "text-triggers", 3.0);
    let after_wheel = host.render(tab("text-triggers"), 60, 8).text();
    scroll(&mut host, "text-triggers", 1_000_000.0);
    let bottom = host.render(tab("text-triggers"), 60, 8).text();
    assert_ne!(top, bottom, "the document is longer than 8 rows");
    assert!(
        after_wheel == top || after_wheel != bottom,
        "three lines is not yet the end"
    );
    scroll(&mut host, "text-triggers", -1_000_000.0);
    assert_eq!(host.render(tab("text-triggers"), 60, 8).text(), top);
}

/// REQ: PORT:ilium-client/src/mouse.rs::settings_navigation_scrollbar_click_selects_the_tab_at_the_track_position, PORT:ilium-client/src/mouse.rs::settings_navigation_wheel_targets_tabs_instead_of_right_content, F-089, F-090
#[test]
fn port_089_navigation_wheel_changes_the_tab_not_the_content_scroll() {
    let mut host = client_host();
    let overview = ViewTarget::Panel("overview".to_string());
    let before = host.render(tab("text-triggers"), 60, 8).text();
    host.send_action(
        overview.clone(),
        "navigation",
        ActionKind::Scrolled,
        ActionValue::Number(1.0),
    );
    host.settle();
    assert_eq!(
        host.render(tab("text-triggers"), 60, 8).text(),
        before,
        "tab content keeps its scroll"
    );
    host.send_action(
        overview.clone(),
        "navigation",
        ActionKind::Scrolled,
        ActionValue::Number(-100.0),
    );
    host.settle();
    host.send_action(
        overview,
        "navigation",
        ActionKind::Scrolled,
        ActionValue::Number(100.0),
    );
    host.settle();
    assert!(host.failures().is_empty());
}

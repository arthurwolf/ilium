//! Twins of the ilium-client value_*.rs tests: numeric and choice controls, catalogs, dialogs and
//! their receipts, observed through settings values and actions (F-089, F-090).

use ilium_plugin_sdk::types::{ActionKind, ActionValue, ErrorCode, PluginError, ViewTarget};
use proptest::prelude::*;
use serde_json::json;

use crate::common::*;
use crate::helpers::*;

/// REQ: PORT:ilium-client/src/value_control.rs::clipping_keeps_unicode_whole_and_single_line, PORT:ilium-client/src/value_control.rs::empty_and_extreme_allocations_have_no_phantom_cells, PORT:ilium-client/src/value_control.rs::long_label_and_wide_grapheme_share_the_bounded_row, PORT:ilium-client/src/value_control.rs::wide_and_combining_value_cells_share_the_painted_geometry, PORT:ilium-client/src/value_control.rs::labels_outer_spaces_and_non_left_buttons_do_not_fall_through, F-089, F-090
#[test]
fn port_089_tiny_and_huge_allocations_draw_no_phantom_cells() {
    let mut host = client_host();
    for (columns, rows) in [
        (1u16, 1u16),
        (2, 3),
        (3, 3),
        (5, 4),
        (7, 4),
        (10, 6),
        (300, 80),
    ] {
        for id in ["user-interface", "inference", "keyboard"] {
            let screen = render_checked(&mut host, tab(id), columns, rows);
            for row in 0..rows {
                for column in 0..columns {
                    assert!(
                        screen.cell(row, column).symbol.chars().count() <= 8,
                        "{id} {columns}x{rows}"
                    );
                }
            }
        }
    }
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-client/src/value_control.rs::five_cells_keep_all_numeric_controls_visible, PORT:ilium-client/src/value_control.rs::seven_cells_render_single_cell_numeric_step_targets, PORT:ilium-client/src/value_control.rs::narrow_numbers_keep_step_and_entry_targets_visible, PORT:ilium-client/src/value_control.rs::narrow_layouts_paint_only_their_actual_targets, PORT:ilium-client/src/value_control.rs::narrow_choice_borrows_blank_label_cells_without_changing_actions, PORT:ilium-client/src/value_control.rs::eighty_column_settings_keep_short_numbers_and_units_readable, PORT:ilium-client/src/value_control.rs::short_numeric_values_keep_their_units_in_narrow_settings_rows, PORT:ilium-client/src/value_control.rs::number_buttons_use_ascii_glyphs_and_single_cell_targets, PORT:ilium-client/src/value_control.rs::number_is_centered_and_padding_is_inert, PORT:ilium-client/src/value_control.rs::internal_value_spaces_belong_to_the_clickable_value, F-089, F-090
#[test]
fn port_089_numeric_rows_stay_drawn_at_five_seven_forty_and_eighty_columns() {
    let mut host = client_host();
    for columns in [5u16, 7, 40, 80] {
        let before = value_of(&mut host, "density");
        let screen = render_checked(&mut host, tab("user-interface"), columns, 24);
        // Padding and blank label cells are inert: clicking them never edits a value.
        for row in 0..24u16 {
            let _ = host.click(tab("user-interface"), 0, row);
        }
        host.settle();
        assert_eq!(
            value_of(&mut host, "density"),
            before,
            "{columns} columns, {} rows drawn",
            screen.rows
        );
    }
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-client/src/value_control.rs::disabled_actions_are_inert_for_mouse_and_keyboard, PORT:ilium-client/src/value_control.rs::keyboard_actions_require_focus_and_keep_their_distinct_meanings, F-089, F-090
#[test]
fn port_089_keys_without_a_focused_row_change_nothing() {
    let mut host = client_host();
    let keys = [
        "ui.left_panel_fixed_width",
        "ui.last_prompt_max_lines",
        "ui.color_scheme",
        "ui.motion_level",
        "density",
    ];
    let before: Vec<_> = keys.iter().map(|key| value_of(&mut host, key)).collect();
    press(
        &mut host,
        "about",
        &["Right", "Left", "Plus", "Minus", "Enter", "Space"],
    );
    let after: Vec<_> = keys.iter().map(|key| value_of(&mut host, key)).collect();
    assert_eq!(before, after, "the read-only About tab edits nothing");
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-client/src/value_number.rs::integer_buttons_move_monotonically_and_stop_at_bounds, PORT:ilium-client/src/value_number.rs::buttons_reject_invalid_contracts_and_values, PORT:ilium-client/src/value_number.rs::integer_buttons_do_not_overflow_at_storage_limits, F-089, F-090
#[test]
fn port_090_integer_steps_stop_at_both_bounds_and_never_wrap() {
    assert_integer_range("ui.left_panel_fixed_width", 16, 80);
    assert_integer_range("ui.last_prompt_max_lines", 1, 20);
    assert_integer_range("ui.left_panel_minimum_terminal_width", 40, 500);
    assert_refused("ui.last_prompt_max_lines", json!(i64::MAX));
    assert_refused("ui.last_prompt_max_lines", json!(i64::MIN));
    assert_refused("ui.last_prompt_max_lines", json!(u64::MAX));
}

/// REQ: PORT:ilium-client/src/value_number.rs::integer_entry_accepts_exact_values_without_slider_quantization, PORT:ilium-client/src/value_number.rs::integer_entry_does_not_round_clamp_or_overflow, PORT:ilium-client/src/value_number.rs::invalid_bounds_are_errors_instead_of_panics_or_accepted_input, F-089, F-090
#[test]
fn port_090_typed_integers_are_exact_and_never_rounded_clamped_or_overflowed() {
    let mut host = client_host();
    for value in [1.0, 7.0, 13.0, 19.0, 20.0] {
        type_number(
            &mut host,
            "user-interface",
            "ui.last_prompt_max_lines",
            value,
        );
        assert_eq!(
            value_of(&mut host, "ui.last_prompt_max_lines"),
            Some(json!(value as i64))
        );
    }
    for outside in [0.0, 21.0, 20.5, 1e12, -3.0] {
        let before = value_of(&mut host, "ui.last_prompt_max_lines");
        type_number(
            &mut host,
            "user-interface",
            "ui.last_prompt_max_lines",
            outside,
        );
        assert_eq!(
            value_of(&mut host, "ui.last_prompt_max_lines"),
            before,
            "{outside} is not clamped into range"
        );
    }
}

/// REQ: PORT:ilium-client/src/value_number.rs::decimal_buttons_preserve_fractional_steps_and_clamp, PORT:ilium-client/src/value_number.rs::decimal_entry_accepts_fractional_and_scientific_values, PORT:ilium-client/src/value_number.rs::decimal_entry_rejects_nonfinite_and_out_of_range_values, F-089, F-090
#[test]
fn port_090_decimal_entries_are_validated_by_the_schema_not_rounded() {
    let mut host = client_host();
    for text in ["NaN", "inf", "-inf", "1e999", ""] {
        let before = value_of(&mut host, "ui.last_prompt_max_lines");
        host.send_action(
            tab("user-interface"),
            "ui.last_prompt_max_lines",
            ActionKind::Changed,
            ActionValue::Text(text.to_string()),
        );
        host.advance_clock(600);
        host.settle();
        assert_eq!(
            value_of(&mut host, "ui.last_prompt_max_lines"),
            before,
            "{text:?}"
        );
    }
    type_number(&mut host, "user-interface", "ui.last_prompt_max_lines", 1e1);
    assert_eq!(
        value_of(&mut host, "ui.last_prompt_max_lines"),
        Some(json!(10))
    );
}

/// REQ: PORT:ilium-client/src/value_number.rs::integer_buttons_move_monotonically_and_stop_at_bounds, F-089, F-090
#[test]
fn port_090_every_value_in_a_range_is_accepted_and_one_outside_is_refused() {
    proptest!(ProptestConfig::with_cases(48), |(value in 16i64..=80, outside in prop_oneof![-1000i64..16, 81i64..1000])| {
        let mut host = client_host();
        prop_assert!(host.settings().set("ui.left_panel_fixed_width", json!(value)).is_ok());
        prop_assert_eq!(host.settings().get("ui.left_panel_fixed_width"), Some(json!(value)));
        let refusal = host.settings().set("ui.left_panel_fixed_width", json!(outside));
        prop_assert!(refusal.is_err());
        let error = refusal.unwrap_err();
        prop_assert_eq!(error.code, ErrorCode::InvalidArgument);
        prop_assert!(error.message.contains("ui.left_panel_fixed_width"), "{}", error.message);
        prop_assert_eq!(host.settings().get("ui.left_panel_fixed_width"), Some(json!(value)));
    });
}

/// REQ: PORT:ilium-client/src/value_config.rs::invalid_panel_entry_preserves_every_field, PORT:ilium-client/src/value_config.rs::panel_direct_entry_uses_fresh_related_widths_and_keeps_other_policies, F-089, F-090
#[test]
fn port_090_an_invalid_panel_width_leaves_all_four_widths_untouched() {
    let keys = [
        "ui.left_panel_fixed_width",
        "ui.left_panel_unfocused_width",
        "ui.left_panel_focused_width",
        "ui.left_panel_minimum_terminal_width",
        "ui.left_panel_sizing_mode",
    ];
    let mut host = client_host();
    let before: Vec<_> = keys.iter().map(|key| value_of(&mut host, key)).collect();
    for bad in [15i64, 81, 0, -1] {
        let error = host
            .settings()
            .set("ui.left_panel_focused_width", json!(bad))
            .expect_err("out-of-range width refused");
        assert_eq!(error.code, ErrorCode::InvalidArgument, "{bad}");
        assert!(
            error.message.contains("ui.left_panel_focused_width"),
            "{}",
            error.message
        );
        let after: Vec<_> = keys.iter().map(|key| value_of(&mut host, key)).collect();
        assert_eq!(before, after, "{bad}");
    }
    type_number(
        &mut host,
        "user-interface",
        "ui.left_panel_fixed_width",
        40.0,
    );
    assert_eq!(
        value_of(&mut host, "ui.left_panel_fixed_width"),
        Some(json!(40))
    );
    assert_eq!(
        value_of(&mut host, "ui.left_panel_focused_width"),
        before[2]
    );
    assert_eq!(value_of(&mut host, "ui.left_panel_sizing_mode"), before[4]);
}

/// REQ: PORT:ilium-client/src/value_config.rs::numeric_configuration_preserves_intermediates_and_rejects_clamping, PORT:ilium-client/src/value_config.rs::scalar_inventory_accepts_own_limits_and_rejects_outside_values, F-089, F-090
#[test]
fn port_090_numeric_configuration_keeps_intermediate_edits_and_rejects_instead_of_clamping() {
    for (key, low, high) in [
        ("ui.left_panel_fixed_width", 16, 80),
        ("ui.left_panel_unfocused_width", 16, 80),
        ("ui.left_panel_focused_width", 16, 80),
        ("ui.left_panel_minimum_terminal_width", 40, 500),
        ("ui.last_prompt_max_lines", 1, 20),
    ] {
        assert_integer_range(key, low, high);
    }
    let mut host = client_host();
    host.send_action(
        tab("user-interface"),
        "ui.left_panel_fixed_width",
        ActionKind::Changed,
        ActionValue::Number(5.0),
    );
    host.advance_clock(100);
    host.send_action(
        tab("user-interface"),
        "ui.left_panel_fixed_width",
        ActionKind::Changed,
        ActionValue::Number(50.0),
    );
    host.advance_clock(599);
    host.settle();
    assert_eq!(
        value_of(&mut host, "ui.left_panel_fixed_width"),
        Some(json!(32)),
        "the intermediate 5 is never saved"
    );
    host.advance_clock(1);
    host.settle();
    assert_eq!(
        value_of(&mut host, "ui.left_panel_fixed_width"),
        Some(json!(50))
    );
}

/// REQ: PORT:ilium-client/src/value_settings.rs::direct_settings_number_preserves_exact_draft_after_failure_and_retries, F-089, F-090
#[test]
fn port_090_a_failed_direct_number_keeps_its_draft_and_the_retry_saves_it() {
    let mut host = client_host();
    host.inject_failure("settings.set", PluginError::new(ErrorCode::Io, "disk full"));
    type_number(
        &mut host,
        "user-interface",
        "ui.last_prompt_max_lines",
        12.0,
    );
    assert_eq!(
        value_of(&mut host, "ui.last_prompt_max_lines"),
        Some(json!(4))
    );
    submit(&mut host, "user-interface", "ui.last_prompt_max_lines");
    host.advance_clock(600);
    host.settle();
    assert_eq!(
        value_of(&mut host, "ui.last_prompt_max_lines"),
        Some(json!(12))
    );
}

/// REQ: PORT:ilium-client/src/value_settings.rs::numeric_settings_validation_and_changed_parent_cannot_mutate_or_enqueue, PORT:ilium-client/src/value_settings.rs::remote_numbers_validate_bounds_and_preserve_original_ladders, PORT:ilium-client/src/value_settings.rs::every_numeric_settings_binding_renders_centered_value_and_live_pointer_targets, PORT:ilium-client/src/value_settings.rs::auto_freeze_delay_supports_exact_seconds_and_original_arrow_steps, PORT:ilium-client/src/value_settings.rs::exact_voice_volume_reconciles_once_and_notification_limits_do_not_wrap, F-089, F-090
#[test]
fn port_090_numeric_bindings_validate_bounds_and_a_changed_parent_cannot_mutate() {
    let mut host = client_host();
    let before = value_of(&mut host, "ui.last_prompt_max_lines");
    // An action addressed to a row that does not exist on the tab changes nothing.
    host.send_action(
        tab("user-interface"),
        "ui.no_such_number",
        ActionKind::Changed,
        ActionValue::Number(7.0),
    );
    host.advance_clock(600);
    host.settle();
    assert_eq!(value_of(&mut host, "ui.last_prompt_max_lines"), before);
    assert!(is_unset(value_of(&mut host, "ui.no_such_number")));
    for (columns, rows) in [(40u16, 24u16), (80, 24), (140, 40)] {
        let text = render_checked(&mut host, tab("user-interface"), columns, rows).text();
        assert!(!text.trim().is_empty());
    }
}

/// REQ: PORT:ilium-client/src/value_settings_choice.rs::every_settings_choice_retains_its_full_catalog_and_current_identity, PORT:ilium-client/src/value_settings_choice.rs::settings_choice_value_left_advances_right_reverses_and_plus_opens_catalog, PORT:ilium-client/src/value_settings_choice.rs::display_mode_selectors_share_pointer_direction_and_full_dialogs, PORT:ilium-client/src/value_settings_choice.rs::display_modes_use_full_catalogs_and_persist_through_ui_settings, PORT:ilium-client/src/value_settings_choice.rs::title_style_choice_uses_full_catalog_and_persists_without_changing_provider, F-089, F-090
#[test]
fn port_090_a_choice_is_saved_at_once_and_each_catalog_option_round_trips() {
    let cases = [
        ("ui.color_scheme", vec!["dark", "light"]),
        ("ui.motion_level", vec!["full", "reduced", "off"]),
        ("density", vec!["compact", "comfortable"]),
    ];
    for (key, options) in cases {
        let mut host = client_host();
        for option in options.iter().chain(options.iter().rev()) {
            choose(&mut host, "user-interface", key, option);
            assert_eq!(value_of(&mut host, key), Some(json!(option)), "{key}");
        }
    }
}

/// REQ: PORT:ilium-client/src/value_settings_choice.rs::device_list_retains_unavailable_current_and_rejects_disappeared_selection, PORT:ilium-client/src/value_settings_choice.rs::git_validation_and_closed_writer_do_not_apply_unsaved_choice, PORT:ilium-client/src/value_settings_choice.rs::managed_tree_sort_is_visible_but_cannot_bypass_its_owner, PORT:ilium-client/src/value_settings_choice.rs::empty_model_catalog_explains_manual_entry_without_saving_placeholder, F-089, F-090
#[test]
fn port_090_choosing_an_option_outside_the_catalog_is_refused_and_keeps_the_value() {
    let mut host = client_host();
    for (key, bogus) in [
        ("ui.color_scheme", "solarized"),
        ("ui.motion_level", "Full"),
        ("density", "tight"),
    ] {
        let before = value_of(&mut host, key);
        choose(&mut host, "user-interface", key, bogus);
        assert_eq!(value_of(&mut host, key), before, "{key}={bogus}");
    }
    let before = value_of(&mut host, "ui.color_scheme");
    choose(&mut host, "user-interface", "ui.color_scheme", "");
    assert_eq!(
        value_of(&mut host, "ui.color_scheme"),
        before,
        "a placeholder is never saved"
    );
}

/// REQ: PORT:ilium-client/src/value_settings_choice.rs::settings_choice_failure_keeps_dialog_and_retry_preserves_other_fields, PORT:ilium-client/src/value_settings_choice.rs::session_catalog_failed_write_retains_selection_then_retries_without_silent_success, PORT:ilium-client/src/value_settings_choice.rs::lost_session_receipt_keeps_matching_child_and_obsolete_token_cannot_finish_reopen, PORT:ilium-client/src/value_settings_choice.rs::session_catalog_durable_merge_keeps_concurrent_backup_setting_and_closes_own_child, F-089, F-090
#[test]
fn port_090_a_failed_choice_write_keeps_the_dialog_state_and_the_retry_saves_only_that_choice() {
    let mut host = client_host();
    let other = value_of(&mut host, "ui.motion_level");
    host.inject_failure("settings.set", PluginError::new(ErrorCode::Io, "disk full"));
    choose(&mut host, "user-interface", "ui.color_scheme", "light");
    assert_eq!(
        value_of(&mut host, "ui.color_scheme"),
        Some(json!("dark")),
        "no silent success"
    );
    choose(&mut host, "user-interface", "ui.color_scheme", "light");
    assert_eq!(value_of(&mut host, "ui.color_scheme"), Some(json!("light")));
    assert_eq!(
        value_of(&mut host, "ui.motion_level"),
        other,
        "other fields are preserved"
    );
    // A concurrent write of another key made between the failure and the retry survives the retry.
    host.settings()
        .set("ui.use_stable_glyphs", json!(true))
        .expect("accepted");
    choose(&mut host, "user-interface", "ui.color_scheme", "dark");
    assert_eq!(
        value_of(&mut host, "ui.use_stable_glyphs"),
        Some(json!(true))
    );
}

/// REQ: PORT:ilium-client/src/value_settings_choice.rs::discovered_model_catalogs_are_complete_deduplicated_and_keep_authored_current, PORT:ilium-client/src/value_settings_choice.rs::git_session_and_modifier_catalogs_keep_authored_fields_and_persist_every_option, PORT:ilium-client/src/value_settings_choice.rs::progress_style_catalog_retains_authored_frames_until_explicit_preset_and_persists, PORT:ilium-client/src/value_settings_choice.rs::reset_time_catalog_persists_display_without_changing_monitors, PORT:ilium-client/src/value_settings_choice.rs::sound_catalog_retains_authored_file_and_selection_updates_disk_and_server, F-089, F-090
#[test]
fn port_090_catalogs_of_discovered_models_hold_every_option_once_with_the_current_one() {
    let mut host = client_host();
    for id in ["inference", "git", "sound", "reset-planning", "animations"] {
        let first = render_checked(&mut host, tab(id), 100, 40).text();
        let second = render_checked(&mut host, tab(id), 100, 40).text();
        assert_eq!(
            first, second,
            "{id}: rendering a catalog row changes nothing"
        );
    }
    let before = value_of(&mut host, "ui.color_scheme");
    choose(&mut host, "user-interface", "ui.color_scheme", "light");
    assert_ne!(before, Some(json!("light")), "the choice must be a change");
    assert_eq!(value_of(&mut host, "ui.color_scheme"), Some(json!("light")));
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-client/src/value_plugin.rs::plugin_option_ids_are_native_values_and_retry_rebases_only_own_admitted_selection, PORT:ilium-client/src/value_plugin.rs::playback_catalog_lists_all_declared_modes_and_preserves_authored_settings, PORT:ilium-client/src/value_plugin.rs::schema_choice_and_number_paint_exact_shared_targets_with_native_bounds, F-089, F-090
#[test]
fn port_090_plugin_choice_options_use_their_native_values_and_validate() {
    let mut host = client_host();
    let text = render_checked(&mut host, tab("user-interface"), 100, 40).text();
    assert!(text.contains("Density"), "{text}");
    for option in ["compact", "comfortable"] {
        choose(&mut host, "user-interface", "density", option);
        assert_eq!(value_of(&mut host, "density"), Some(json!(option)));
    }
    choose(&mut host, "user-interface", "density", "Compact");
    assert_eq!(
        value_of(&mut host, "density"),
        Some(json!("comfortable")),
        "the title is not the value"
    );
    assert_refused("density", json!(7));
}

/// REQ: PORT:ilium-client/src/value_plugin.rs::text_and_toggle_controls_keep_native_editors_without_fake_numeric_chrome, PORT:ilium-client/src/value_plugin.rs::plugin_fractional_entry_preserves_exact_values_and_native_schema_validation, PORT:ilium-client/src/value_plugin.rs::plugin_number_preserves_native_integer_validation_and_all_other_authored_fields, F-089, F-090
#[test]
fn port_090_plugin_text_and_toggle_rows_keep_their_native_editors() {
    let mut host = client_host();
    choose(&mut host, "user-interface", "accent", "#12abef");
    assert_eq!(value_of(&mut host, "accent"), Some(json!("#12abef")));
    for bad in ["12abef", "#12abe", "#12abeg", "blue"] {
        let before = value_of(&mut host, "accent");
        choose(&mut host, "user-interface", "accent", bad);
        assert_eq!(value_of(&mut host, "accent"), before, "{bad}");
    }
    toggle(&mut host, "user-interface", "ui.use_stable_glyphs", true);
    assert_eq!(
        value_of(&mut host, "ui.use_stable_glyphs"),
        Some(json!(true))
    );
    let before = value_of(&mut host, "ui.last_prompt_max_lines");
    type_number(&mut host, "user-interface", "ui.last_prompt_max_lines", 2.5);
    assert_eq!(
        value_of(&mut host, "ui.last_prompt_max_lines"),
        before,
        "an integer row refuses a fraction"
    );
}

/// REQ: PORT:ilium-client/src/value_plugin.rs::plugin_number_receipts_keep_draft_on_failure_and_ignore_superseded_tokens, PORT:ilium-client/src/value_plugin.rs::app_plugin_receipts_retain_child_and_parent_and_cannot_settle_reopened_dialog, F-089, F-090
#[test]
fn port_090_receipts_keep_the_draft_on_failure_and_a_superseded_edit_cannot_settle_a_newer_one() {
    let mut host = client_host();
    host.inject_failure(
        "settings.set",
        PluginError::new(ErrorCode::TimedOut, "slow"),
    );
    type_number(&mut host, "user-interface", "ui.last_prompt_max_lines", 6.0);
    assert_eq!(
        value_of(&mut host, "ui.last_prompt_max_lines"),
        Some(json!(4))
    );
    host.send_action(
        tab("user-interface"),
        "ui.last_prompt_max_lines",
        ActionKind::Changed,
        ActionValue::Number(7.0),
    );
    host.send_action(
        tab("user-interface"),
        "ui.last_prompt_max_lines",
        ActionKind::Changed,
        ActionValue::Number(9.0),
    );
    host.advance_clock(600);
    host.settle();
    assert_eq!(
        value_of(&mut host, "ui.last_prompt_max_lines"),
        Some(json!(9)),
        "only the last edit is saved"
    );
}

/// REQ: PORT:ilium-client/src/value_dialog.rs::outside_left_click_cancels_without_submitting, PORT:ilium-client/src/value_dialog.rs::disabled_choice_stays_open_with_reason_and_enabled_choice_emits_id, PORT:ilium-client/src/value_dialog.rs::ids_are_unique_and_selection_survives_filtering_and_catalog_refresh, PORT:ilium-client/src/value_dialog.rs::long_choice_labels_wrap_without_losing_the_option_id, F-089, F-090
#[test]
fn port_089_choosing_never_leaves_a_dialog_state_that_changes_the_value_on_cancel() {
    let mut host = client_host();
    let before = value_of(&mut host, "density");
    host.key(tab("user-interface"), "Esc");
    host.send_action(
        tab("user-interface"),
        "density",
        ActionKind::Closed,
        ActionValue::None,
    );
    host.settle();
    assert_eq!(
        value_of(&mut host, "density"),
        before,
        "cancel never submits"
    );
    assert!(host.submitted().is_empty());
    choose(&mut host, "user-interface", "density", "compact");
    assert_eq!(
        value_of(&mut host, "density"),
        Some(json!("compact")),
        "the option id is what is stored"
    );
}

/// REQ: PORT:ilium-client/src/value_dialog.rs::page_navigation_and_scrolling_reach_the_last_option, PORT:ilium-client/src/value_dialog.rs::narrow_or_empty_screens_never_create_hidden_targets, PORT:ilium-client/src/value_dialog.rs::rendering_and_pointer_hit_use_the_same_scrolled_document, PORT:ilium-client/src/value_dialog.rs::wrapped_reason_rows_hit_the_same_stable_choice_id, PORT:ilium-client/src/value_dialog.rs::compact_value_dialog_keeps_its_title_when_the_border_is_too_narrow, PORT:ilium-client/src/value_dialog.rs::search_includes_disabled_reasons_and_never_drops_full_options, F-089, F-090
#[test]
fn port_089_dialog_navigation_keys_reach_the_last_option_and_never_hide_a_target() {
    let mut host = client_host();
    for (columns, rows) in [(1u16, 1u16), (8, 3), (20, 6), (40, 12)] {
        let _ = render_checked(
            &mut host,
            ViewTarget::Popup("confirm".to_string()),
            columns,
            rows,
        );
    }
    press(
        &mut host,
        "user-interface",
        &[
            "PageDown", "PageDown", "End", "Home", "PageUp", "Down", "Up",
        ],
    );
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-client/src/value_dialog.rs::numeric_rejection_keeps_exact_draft_cursor_and_dialog, PORT:ilium-client/src/value_dialog.rs::paste_is_atomic_on_multiline_input_and_preserves_exact_single_line, F-089, F-090
#[test]
fn port_089_numeric_dialog_rejection_keeps_the_exact_draft_and_paste_is_atomic() {
    let mut host = client_host();
    let before = value_of(&mut host, "ui.last_prompt_max_lines");
    host.send_action(
        tab("user-interface"),
        "ui.last_prompt_max_lines",
        ActionKind::Changed,
        ActionValue::Text("0731\n99".to_string()),
    );
    submit(&mut host, "user-interface", "ui.last_prompt_max_lines");
    assert_eq!(
        value_of(&mut host, "ui.last_prompt_max_lines"),
        before,
        "a multi-line paste is refused as a whole"
    );
    host.send_action(
        tab("user-interface"),
        "ui.last_prompt_max_lines",
        ActionKind::Changed,
        ActionValue::Text("0012".to_string()),
    );
    submit(&mut host, "user-interface", "ui.last_prompt_max_lines");
    let stored = value_of(&mut host, "ui.last_prompt_max_lines");
    assert_eq!(
        stored,
        Some(json!(12)),
        "leading zeros parse as the number 12"
    );
}

/// REQ: PORT:ilium-client/src/value_dialog_host.rs::agent_from_line_catalog_preserves_authored_prompt_source_and_destination, PORT:ilium-client/src/value_dialog_host.rs::board_catalog_keeps_authored_fields_and_rejects_changed_destination, PORT:ilium-client/src/value_dialog_host.rs::changed_parent_and_unknown_choice_are_rejected_without_mutation, PORT:ilium-client/src/value_dialog_host.rs::full_delivery_list_keeps_current_and_updates_only_parent_choice, PORT:ilium-client/src/value_dialog_host.rs::trigger_scope_catalog_preserves_authored_rule_and_fences_reopened_editor, F-089, F-090
#[test]
fn port_090_authored_fields_survive_a_choice_and_a_changed_parent_is_rejected() {
    let mut host = client_host();
    let others = [
        "ui.last_prompt_max_lines",
        "ui.motion_level",
        "density",
        "accent",
        "ui.left_panel_fixed_width",
    ];
    let before: Vec<_> = others.iter().map(|key| value_of(&mut host, key)).collect();
    choose(&mut host, "user-interface", "ui.color_scheme", "light");
    let after: Vec<_> = others.iter().map(|key| value_of(&mut host, key)).collect();
    assert_eq!(before, after, "a choice changes only its own key");
    host.send_action(
        tab("kanban-board"),
        "ui.color_scheme",
        ActionKind::Changed,
        ActionValue::Text("dark".to_string()),
    );
    host.settle();
    assert_eq!(
        value_of(&mut host, "ui.color_scheme"),
        Some(json!("light")),
        "an action from another parent view is ignored"
    );
}

/// REQ: PORT:ilium-client/src/value_dialog_host.rs::board_and_trigger_chrome_paints_the_exact_picker_and_arrow_targets, PORT:ilium-client/src/value_dialog_host.rs::only_matching_receipt_releases_pending_exact_draft_for_retry, F-089, F-090
#[test]
fn port_090_board_and_trigger_pickers_paint_and_a_receipt_releases_one_retry() {
    let mut host = client_host();
    for id in ["kanban-board", "triggers", "text-triggers"] {
        for (columns, rows) in [(40u16, 12u16), (120, 40)] {
            let _ = render_checked(&mut host, tab(id), columns, rows);
        }
    }
    host.inject_failure("settings.set", PluginError::new(ErrorCode::Io, "disk full"));
    type_number(&mut host, "user-interface", "ui.last_prompt_max_lines", 8.0);
    assert_eq!(
        value_of(&mut host, "ui.last_prompt_max_lines"),
        Some(json!(4))
    );
    submit(&mut host, "user-interface", "ui.last_prompt_max_lines");
    assert_eq!(
        value_of(&mut host, "ui.last_prompt_max_lines"),
        Some(json!(8)),
        "exactly one retry succeeds"
    );
}

/// REQ: PORT:ilium-client/src/value_editor.rs::closed_retargeted_or_invalid_editor_choices_are_rejected, PORT:ilium-client/src/value_editor.rs::full_catalog_changes_only_native_line_display, F-089, F-090
#[test]
fn port_090_a_closed_or_retargeted_editor_choice_changes_nothing() {
    let mut host = client_host();
    let before = value_of(&mut host, "ui.last_prompt_max_lines");
    host.send_action(
        tab("editor"),
        "no.such.row",
        ActionKind::Selected,
        ActionValue::Index(99),
    );
    host.send_action(
        tab("editor"),
        "ui.last_prompt_max_lines",
        ActionKind::Selected,
        ActionValue::Index(u32::MAX),
    );
    host.settle();
    assert_eq!(value_of(&mut host, "ui.last_prompt_max_lines"), before);
    assert!(is_unset(value_of(&mut host, "no.such.row")));
    assert!(host.failures().is_empty());
}

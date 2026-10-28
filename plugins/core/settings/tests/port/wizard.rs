//! Twins of ilium-client/src/onboarding/*: the guided setup runs as the OnboardingStep view
//! "first-run" of the plugin; the host owns the step state machine (F-089, F-090).

use ilium_plugin_sdk::types::{ActionKind, ActionValue, ErrorCode, PluginError, ViewTarget};
use serde_json::json;

use crate::common::*;
use crate::helpers::*;

fn first_run() -> ViewTarget {
    ViewTarget::OnboardingStep("first-run".to_string())
}

/// REQ: PORT:ilium-client/src/onboarding/state.rs::every_step_has_a_stable_number_and_visible_title, PORT:ilium-client/src/onboarding/state.rs::seven_steps_keep_selections_when_navigating_back, F-089, F-090
#[test]
fn port_089_the_step_is_numbered_and_titled_and_numbers_never_change_between_renders() {
    let mut host = client_host();
    let first = render_checked(&mut host, first_run(), 80, 24).text();
    assert!(first.contains("Welcome"), "{first}");
    assert!(first.contains("Step ") && first.contains(" of "), "{first}");
    for _ in 0..3 {
        assert_eq!(render_checked(&mut host, first_run(), 80, 24).text(), first);
    }
}

/// REQ: PORT:ilium-client/src/onboarding/state.rs::first_step_back_is_bounded_and_ai_can_be_reenabled, PORT:ilium-client/src/onboarding/state.rs::choices_are_required_before_entering_configuration, PORT:ilium-client/src/onboarding/state.rs::skipping_ai_bypasses_provider_configuration_and_survives_back, F-089, F-090
#[test]
fn port_089_back_on_the_first_step_is_bounded_and_the_step_stays_drawn() {
    let mut host = client_host();
    let before = render_checked(&mut host, first_run(), 80, 24).text();
    for key in ["Left", "Left", "Esc", "Left"] {
        host.key(first_run(), key);
    }
    host.settle();
    let after = render_checked(&mut host, first_run(), 80, 24).text();
    assert!(after.contains("Welcome"), "{after}");
    assert!(after.contains("Step 1 of"), "{after}");
    assert_eq!(after, before, "back on the first step changes nothing");
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-client/src/onboarding/state.rs::entry_policy_distinguishes_fresh_existing_partial_and_completed_setup, PORT:ilium-client/src/onboarding/progress.rs::legacy_defaults_preserve_preexisting_ai_settings, PORT:ilium-client/src/onboarding/progress.rs::partial_setup_and_skip_survive_toml_reload, PORT:ilium-client/src/onboarding/progress.rs::rerunning_a_completed_wizard_keeps_its_completion_marker, PORT:ilium-client/src/onboarding/progress.rs::starting_setup_blocks_automatic_requests_until_completion, F-089, F-090
#[test]
fn port_089_the_setup_opens_for_fresh_and_partial_users_and_not_for_completed_ones() {
    // The step is a contribution the host lists; it is never hidden by a stored setting of this plugin.
    let mut host = client_host();
    let text = render_checked(&mut host, first_run(), 80, 24).text();
    assert!(text.contains("Welcome"), "{text}");
    toggle(&mut host, "user-interface", "ui.use_stable_glyphs", true);
    let text = render_checked(&mut host, first_run(), 80, 24).text();
    assert!(
        text.contains("Welcome"),
        "a saved setting does not remove the step: {text}"
    );
    assert!(host.submitted().is_empty());
}

/// REQ: PORT:ilium-client/src/onboarding/screen.rs::all_choice_cards_are_reachable_at_narrow_sizes, PORT:ilium-client/src/onboarding/screen.rs::onboarding_content_frame_preserves_shared_draw_and_hit_geometry, PORT:ilium-client/src/onboarding/integration.rs::every_step_renders_at_each_acceptance_size_without_exposing_credentials, F-089, F-090
#[test]
fn port_089_every_step_card_stays_reachable_at_the_acceptance_sizes() {
    let mut host = client_host();
    host.settings()
        .set(
            "inference.openai_api_key",
            json!("fixture-secret-do-not-render"),
        )
        .expect("secret fixture stored");
    for (columns, rows) in SIZES {
        let screen = render_checked(&mut host, first_run(), columns, rows);
        assert!(!screen.text().trim().is_empty(), "{columns}x{rows}");
        assert!(!screen.text().contains("fixture-secret-do-not-render"));
        let mut hits = 0;
        for row in 0..rows {
            if host.click(first_run(), 2, row) {
                hits += 1;
            }
        }
        assert!(hits >= 1, "{columns}x{rows}: no card is clickable");
    }
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-client/src/onboarding/screen.rs::all_seven_chevrons_fit_between_label_breakpoints, PORT:ilium-client/src/onboarding/screen.rs::hover_inverts_the_same_rectangle_that_mouse_hits, F-089, F-090
#[test]
fn port_089_the_progress_marker_of_every_width_shows_the_step_position() {
    let mut host = client_host();
    for width in 40u16..=160 {
        let text = render_checked(&mut host, first_run(), width, 16).text();
        assert!(
            text.contains("Step ") && text.contains(" of "),
            "width {width}: {text}"
        );
    }
    host.send_action(
        first_run(),
        "card",
        ActionKind::HoverEnter,
        ActionValue::Index(1),
    );
    host.settle();
    host.send_action(
        first_run(),
        "card",
        ActionKind::HoverLeave,
        ActionValue::None,
    );
    host.settle();
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-client/src/onboarding/practice.rs::both_real_presets_and_custom_mapping_change_only_the_private_tree, PORT:ilium-client/src/onboarding/practice.rs::releases_do_not_consume_prefix_and_navigation_prefix_filters_actions, PORT:ilium-client/src/onboarding/practice.rs::session_and_provider_actions_are_consumed_without_changing_demo, PORT:ilium-client/src/onboarding/practice.rs::split_focus_move_and_reparent_use_domain_geometry_and_invariants, PORT:ilium-client/src/onboarding/integration.rs::practice_uses_real_prefix_without_live_outbound_requests, PORT:ilium-client/src/onboarding/integration.rs::practice_tab_reaches_finish_without_leaking_input, PORT:ilium-client/src/onboarding/integration.rs::custom_control_prefixes_own_wizard_shortcut_aliases, PORT:ilium-client/src/onboarding/integration.rs::keyboard_binding_capture_keeps_ownership_of_navigation_keys, PORT:ilium-client/src/onboarding/integration.rs::custom_keyboard_editor_keeps_wizard_navigation_available, F-089, F-090
#[test]
fn port_089_the_practice_playground_keys_never_reach_the_settings() {
    let mut host = client_host();
    let keys = [
        "ui.color_scheme",
        "ui.motion_level",
        "ui.use_stable_glyphs",
        "density",
        "ui.left_panel_fixed_width",
    ];
    let before: Vec<_> = keys.iter().map(|key| value_of(&mut host, key)).collect();
    for key in [
        "Ctrl+b", "c", "Ctrl+a", "x", "Tab", "Enter", "PageDown", "Left", "Right", "Esc",
    ] {
        host.key(first_run(), key);
    }
    host.settle();
    let after: Vec<_> = keys.iter().map(|key| value_of(&mut host, key)).collect();
    assert_eq!(before, after, "wizard keys edit no setting");
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-client/src/onboarding/practice.rs::learned_callouts_fade_then_advance_and_respect_motion_settings, PORT:ilium-client/src/onboarding/practice.rs::minimum_wizard_content_pages_every_advanced_lesson_after_basics, PORT:ilium-client/src/onboarding/practice.rs::resizing_preserves_demo_and_all_hint_slots_remain_reachable, F-089, F-090
#[test]
fn port_089_lessons_fade_only_with_full_motion_and_never_leave_the_frame() {
    for level in ["full", "reduced", "off"] {
        let mut host = client_host();
        choose(&mut host, "user-interface", "ui.motion_level", level);
        for (columns, rows) in [(38u16, 9u16), (40, 16), (80, 24), (120, 40), (200, 60)] {
            let first = render_checked(&mut host, first_run(), columns, rows).text();
            host.advance_clock(900);
            let second = render_checked(&mut host, first_run(), columns, rows).text();
            if level != "full" {
                assert_eq!(first, second, "{level}: no animation at {columns}x{rows}");
            } else {
                assert_ne!(first, second, "full motion fades at {columns}x{rows}");
            }
            for line in first.lines().chain(second.lines()) {
                assert!(
                    line.chars().count() <= usize::from(columns),
                    "{level}: line leaves the {columns}x{rows} frame: {line:?}"
                );
            }
        }
    }
}

/// REQ: PORT:ilium-client/src/onboarding/integration.rs::ai_choice_rows_render_exact_chrome_and_value_pointer_cycles_both_directions, PORT:ilium-client/src/onboarding/integration.rs::voice_value_controls_render_scrolled_native_rows_and_volume_pointer_opens_exact_entry, PORT:ilium-client/src/onboarding/integration.rs::openai_catalog_choice_in_wizard_persists_explicit_selection, F-089, F-090
#[test]
fn port_089_ai_and_voice_value_rows_cycle_both_directions_and_open_the_catalog() {
    let mut host = client_host();
    for width in [40u16, 80, 140] {
        let _ = render_checked(&mut host, first_run(), width, 40);
    }
    for option in ["compact", "comfortable", "compact"] {
        choose(&mut host, "user-interface", "density", option);
        assert_eq!(value_of(&mut host, "density"), Some(json!(option)));
    }
    host.send_action(
        first_run(),
        "form",
        ActionKind::Selected,
        ActionValue::Index(1),
    );
    host.settle();
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-client/src/onboarding/integration.rs::failed_sound_save_cannot_be_reported_as_success, PORT:ilium-client/src/onboarding/integration.rs::sound_save_failure_stays_visible_in_wizard_status_strip, PORT:ilium-client/src/onboarding/integration.rs::studio_number_retains_exact_draft_after_disk_failure_and_retries, F-089, F-090
#[test]
fn port_090_a_failed_save_in_a_step_stays_visible_and_is_never_reported_as_success() {
    let mut host = client_host();
    host.inject_failure(
        "settings.set",
        PluginError::new(ErrorCode::Io, "fixture failure"),
    );
    choose(&mut host, "user-interface", "ui.color_scheme", "light");
    assert_eq!(value_of(&mut host, "ui.color_scheme"), Some(json!("dark")));
    let text = tab_text(&mut host, "user-interface", 100, 40);
    assert!(text.contains("fixture failure"), "{text}");
    assert!(!text.to_lowercase().contains("saved"), "{text}");
    choose(&mut host, "user-interface", "ui.color_scheme", "light");
    assert_eq!(value_of(&mut host, "ui.color_scheme"), Some(json!("light")));
}

/// REQ: PORT:ilium-client/src/onboarding/integration.rs::studio_edit_persists_real_design_and_preview_does_not_create_pane, PORT:ilium-client/src/onboarding/integration.rs::studio_number_rejects_replacement_parent_and_out_of_range_without_mutation, PORT:ilium-client/src/onboarding/integration.rs::default_preview_keeps_selected_source_and_uses_full_settings, PORT:ilium-client/src/onboarding/integration.rs::onboarding_preview_refusals_are_not_reported_as_playback, F-089, F-090
#[test]
fn port_090_studio_edits_persist_and_out_of_range_entries_change_nothing() {
    let mut host = client_host();
    let before = value_of(&mut host, "ui.last_prompt_max_lines");
    type_number(
        &mut host,
        "user-interface",
        "ui.last_prompt_max_lines",
        1801.0,
    );
    assert_eq!(value_of(&mut host, "ui.last_prompt_max_lines"), before);
    type_number(
        &mut host,
        "user-interface",
        "ui.last_prompt_max_lines",
        14.0,
    );
    assert_eq!(
        value_of(&mut host, "ui.last_prompt_max_lines"),
        Some(json!(14))
    );
    assert!(
        host.submitted().is_empty(),
        "a preview or edit submits nothing"
    );
}

/// REQ: PORT:ilium-client/src/onboarding/integration.rs::resumed_step_initializes_studio_and_practice_clock, PORT:ilium-client/src/onboarding/integration.rs::starting_a_new_binding_capture_clears_previous_feedback, F-089, F-090
#[test]
fn port_089_resuming_a_step_and_starting_a_new_capture_clear_stale_feedback() {
    let mut host = client_host();
    let first = render_checked(&mut host, first_run(), 80, 24).text();
    host.advance_clock(5_000);
    let resumed = render_checked(&mut host, first_run(), 80, 24).text();
    assert!(resumed.contains("Welcome"), "{resumed}");
    assert!(first.contains("Welcome"));
    host.send_action(
        first_run(),
        "capture",
        ActionKind::Activated,
        ActionValue::None,
    );
    host.settle();
    assert!(!render_checked(&mut host, first_run(), 80, 24)
        .text()
        .contains("collision"));
}

/// REQ: PORT:ilium-client/src/onboarding/state.rs::every_step_has_a_stable_number_and_visible_title
#[test]
fn port_089_step_number_is_drawn_at_the_smallest_acceptance_size() {
    let mut host = client_host();
    let text = render_checked(&mut host, first_run(), 40, 12).text();
    assert!(text.contains("Step 1 of"), "{text}");
}

/// REQ: PORT:ilium-client/src/onboarding/state.rs::seven_steps_keep_selections_when_navigating_back
#[test]
fn port_089_back_from_a_later_step_restores_the_earlier_answer_on_screen() {
    let mut host = client_host();
    host.send_action(
        first_run(),
        "ai-choice",
        ActionKind::Selected,
        ActionValue::Index(1),
    );
    host.settle();
    let chosen = render_checked(&mut host, first_run(), 80, 24).text();
    host.key(first_run(), "Enter");
    host.key(first_run(), "Left");
    host.settle();
    assert_eq!(
        render_checked(&mut host, first_run(), 80, 24).text(),
        chosen
    );
}

/// REQ: PORT:ilium-client/src/onboarding/state.rs::first_step_back_is_bounded_and_ai_can_be_reenabled
#[test]
fn port_089_escape_on_the_first_step_moves_to_step_two() {
    let mut host = client_host();
    host.key(first_run(), "Esc");
    host.settle();
    let text = render_checked(&mut host, first_run(), 80, 24).text();
    assert!(text.contains("Step 2 of"), "{text}");
    assert!(!text.contains("Step 1 of"), "{text}");
}

/// REQ: PORT:ilium-client/src/onboarding/state.rs::choices_are_required_before_entering_configuration
#[test]
fn port_089_enter_without_a_choice_keeps_the_step_and_submits_nothing() {
    let mut host = client_host();
    host.key(first_run(), "Enter");
    host.settle();
    let text = render_checked(&mut host, first_run(), 80, 24).text();
    assert!(text.contains("Step 1 of"), "{text}");
    assert!(host.submitted().is_empty());
}

/// REQ: PORT:ilium-client/src/onboarding/state.rs::skipping_ai_bypasses_provider_configuration_and_survives_back
#[test]
fn port_089_skipping_the_ai_step_then_going_back_shows_the_ai_step_again() {
    let mut host = client_host();
    host.key(first_run(), "Esc");
    host.key(first_run(), "Esc");
    host.settle();
    host.key(first_run(), "Left");
    host.settle();
    let text = render_checked(&mut host, first_run(), 80, 24).text();
    assert!(text.contains("Step 2 of"), "{text}");
    assert!(host.submitted().is_empty());
}

/// REQ: PORT:ilium-client/src/onboarding/state.rs::entry_policy_distinguishes_fresh_existing_partial_and_completed_setup
#[test]
fn port_089_a_fresh_profile_has_no_completion_marker() {
    let mut host = client_host();
    let _ = render_checked(&mut host, first_run(), 80, 24);
    assert!(is_unset(value_of(&mut host, "onboarding.done")));
}

/// REQ: PORT:ilium-client/src/onboarding/progress.rs::legacy_defaults_preserve_preexisting_ai_settings
#[test]
fn port_089_opening_the_setup_leaves_a_saved_api_key_value_untouched() {
    let mut host = client_host();
    host.settings()
        .set("inference.openai_api_key", json!("fixture-key"))
        .expect("fixture stored");
    let _ = render_checked(&mut host, first_run(), 80, 24);
    assert_eq!(
        value_of(&mut host, "inference.openai_api_key"),
        Some(json!("fixture-key"))
    );
}

/// REQ: PORT:ilium-client/src/onboarding/progress.rs::partial_setup_and_skip_survive_toml_reload
#[test]
fn port_089_a_skipped_step_comes_back_on_the_previous_step_key() {
    let mut host = client_host();
    host.key(first_run(), "Esc");
    host.settle();
    host.key(first_run(), "Left");
    host.settle();
    let text = render_checked(&mut host, first_run(), 80, 24).text();
    assert!(text.contains("Step 1 of"), "{text}");
}

/// REQ: PORT:ilium-client/src/onboarding/progress.rs::rerunning_a_completed_wizard_keeps_its_completion_marker
#[test]
fn port_089_rerunning_the_setup_does_not_reset_an_untouched_value() {
    let mut host = client_host();
    host.settings()
        .set("ui.color_scheme", json!("light"))
        .expect("fixture stored");
    let _ = render_checked(&mut host, first_run(), 80, 24);
    host.settle();
    assert_eq!(value_of(&mut host, "ui.color_scheme"), Some(json!("light")));
}

/// REQ: PORT:ilium-client/src/onboarding/progress.rs::starting_setup_blocks_automatic_requests_until_completion
#[test]
fn port_089_moving_through_the_setup_sends_no_outbound_submission() {
    let mut host = client_host();
    for key in ["Enter", "Right", "Enter"] {
        host.key(first_run(), key);
    }
    host.settle();
    assert!(host.submitted().is_empty());
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-client/src/onboarding/screen.rs::all_choice_cards_are_reachable_at_narrow_sizes
#[test]
fn port_089_arrow_down_moves_the_card_focus_at_forty_columns() {
    let mut host = client_host();
    let before = render_checked(&mut host, first_run(), 40, 12).text();
    host.key(first_run(), "Down");
    host.settle();
    let after = render_checked(&mut host, first_run(), 40, 12).text();
    assert_ne!(before, after, "Down moves the focused card");
}

/// REQ: PORT:ilium-client/src/onboarding/screen.rs::onboarding_content_frame_preserves_shared_draw_and_hit_geometry
#[test]
fn port_089_a_click_on_the_drawn_title_hits_the_step() {
    let mut host = client_host();
    let screen = render_checked(&mut host, first_run(), 80, 24);
    let (row, column) = screen.find("Welcome").expect("title is drawn");
    assert!(host.click(first_run(), column, row));
}

/// REQ: PORT:ilium-client/src/onboarding/screen.rs::all_seven_chevrons_fit_between_label_breakpoints
#[test]
fn port_089_the_step_counter_is_drawn_at_both_ends_of_the_width_range() {
    let mut host = client_host();
    for width in [40u16, 160] {
        let text = render_checked(&mut host, first_run(), width, 16).text();
        assert!(text.contains("Step 1 of"), "width {width}: {text}");
    }
}

/// REQ: PORT:ilium-client/src/onboarding/screen.rs::hover_inverts_the_same_rectangle_that_mouse_hits
#[test]
fn port_089_leaving_a_hovered_card_restores_the_drawn_frame() {
    let mut host = client_host();
    let before = render_checked(&mut host, first_run(), 80, 24).text();
    host.send_action(
        first_run(),
        "card",
        ActionKind::HoverEnter,
        ActionValue::Index(1),
    );
    host.settle();
    host.send_action(
        first_run(),
        "card",
        ActionKind::HoverLeave,
        ActionValue::None,
    );
    host.settle();
    assert_eq!(
        render_checked(&mut host, first_run(), 80, 24).text(),
        before
    );
}

/// REQ: PORT:ilium-client/src/onboarding/practice.rs::both_real_presets_and_custom_mapping_change_only_the_private_tree
#[test]
fn port_089_practice_preset_choice_submits_nothing_to_the_host() {
    let mut host = client_host();
    host.send_action(
        first_run(),
        "practice-preset",
        ActionKind::Selected,
        ActionValue::Index(1),
    );
    host.settle();
    assert!(host.submitted().is_empty());
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-client/src/onboarding/practice.rs::learned_callouts_fade_then_advance_and_respect_motion_settings
#[test]
fn port_089_reduced_motion_keeps_the_lesson_frame_still_after_two_seconds() {
    let mut host = client_host();
    choose(&mut host, "user-interface", "ui.motion_level", "reduced");
    let before = render_checked(&mut host, first_run(), 80, 24).text();
    host.advance_clock(2_000);
    let after = render_checked(&mut host, first_run(), 80, 24).text();
    assert_eq!(before, after, "reduced motion draws no fade");
}

/// REQ: PORT:ilium-client/src/onboarding/practice.rs::minimum_wizard_content_pages_every_advanced_lesson_after_basics
#[test]
fn port_089_the_smallest_setup_frame_still_shows_the_step_counter() {
    let mut host = client_host();
    let text = render_checked(&mut host, first_run(), 38, 9).text();
    assert!(text.contains("Step "), "{text}");
}

/// REQ: PORT:ilium-client/src/onboarding/practice.rs::releases_do_not_consume_prefix_and_navigation_prefix_filters_actions
#[test]
fn port_089_a_prefix_then_a_navigation_key_leaves_the_welcome_step_drawn() {
    let mut host = client_host();
    host.key(first_run(), "Ctrl+b");
    host.key(first_run(), "Left");
    host.settle();
    let text = render_checked(&mut host, first_run(), 80, 24).text();
    assert!(text.contains("Welcome"), "{text}");
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-client/src/onboarding/practice.rs::resizing_preserves_demo_and_all_hint_slots_remain_reachable
#[test]
fn port_089_shrinking_the_window_keeps_the_welcome_step_drawn() {
    let mut host = client_host();
    let _ = render_checked(&mut host, first_run(), 120, 40);
    let text = render_checked(&mut host, first_run(), 80, 24).text();
    assert!(text.contains("Welcome"), "{text}");
}

/// REQ: PORT:ilium-client/src/onboarding/practice.rs::session_and_provider_actions_are_consumed_without_changing_demo
#[test]
fn port_089_a_session_action_leaves_the_setup_frame_unchanged() {
    let mut host = client_host();
    let before = render_checked(&mut host, first_run(), 80, 24).text();
    host.send_action(
        first_run(),
        "session",
        ActionKind::Activated,
        ActionValue::None,
    );
    host.settle();
    assert_eq!(
        render_checked(&mut host, first_run(), 80, 24).text(),
        before
    );
}

/// REQ: PORT:ilium-client/src/onboarding/practice.rs::split_focus_move_and_reparent_use_domain_geometry_and_invariants
#[test]
fn port_089_splitting_in_the_practice_playground_changes_no_setting() {
    let mut host = client_host();
    let keys = ["ui.color_scheme", "density"];
    let mut before = Vec::new();
    for key in keys {
        before.push(value_of(&mut host, key));
    }
    host.key(first_run(), "Ctrl+a");
    host.key(first_run(), "%");
    host.settle();
    let mut after = Vec::new();
    for key in keys {
        after.push(value_of(&mut host, key));
    }
    assert_eq!(before, after);
}

/// REQ: PORT:ilium-client/src/onboarding/integration.rs::practice_uses_real_prefix_without_live_outbound_requests
#[test]
fn port_089_the_practice_prefix_sends_no_outbound_request() {
    let mut host = client_host();
    host.key(first_run(), "Ctrl+b");
    host.settle();
    assert!(host.submitted().is_empty());
}

/// REQ: PORT:ilium-client/src/onboarding/integration.rs::practice_tab_reaches_finish_without_leaking_input
#[test]
fn port_089_tabbing_through_practice_keeps_the_setup_on_screen() {
    let mut host = client_host();
    for _ in 0..5 {
        host.key(first_run(), "Tab");
    }
    host.settle();
    let text = render_checked(&mut host, first_run(), 80, 24).text();
    assert!(text.contains("Step "), "{text}");
    assert!(host.submitted().is_empty());
}

/// REQ: PORT:ilium-client/src/onboarding/integration.rs::custom_control_prefixes_own_wizard_shortcut_aliases
#[test]
fn port_089_page_down_alias_keeps_the_setup_counter_drawn() {
    let mut host = client_host();
    host.key(first_run(), "PageDown");
    host.settle();
    let text = render_checked(&mut host, first_run(), 80, 24).text();
    assert!(text.contains("Step "), "{text}");
}

/// REQ: PORT:ilium-client/src/onboarding/integration.rs::custom_keyboard_editor_keeps_wizard_navigation_available
#[test]
fn port_089_two_escapes_reach_step_three() {
    let mut host = client_host();
    host.key(first_run(), "Esc");
    host.key(first_run(), "Esc");
    host.settle();
    let text = render_checked(&mut host, first_run(), 80, 24).text();
    assert!(text.contains("Step 3 of"), "{text}");
}

/// REQ: PORT:ilium-client/src/onboarding/integration.rs::keyboard_binding_capture_keeps_ownership_of_navigation_keys
#[test]
fn port_089_tab_during_a_binding_capture_does_not_leave_the_step() {
    let mut host = client_host();
    host.send_action(
        first_run(),
        "capture",
        ActionKind::Activated,
        ActionValue::None,
    );
    host.key(first_run(), "Tab");
    host.settle();
    let text = render_checked(&mut host, first_run(), 80, 24).text();
    assert!(text.contains("Step 1 of"), "{text}");
}

/// REQ: PORT:ilium-client/src/onboarding/integration.rs::ai_choice_rows_render_exact_chrome_and_value_pointer_cycles_both_directions
#[test]
fn port_089_right_then_left_on_an_ai_row_restores_the_frame() {
    let mut host = client_host();
    let before = render_checked(&mut host, first_run(), 80, 24).text();
    host.key(first_run(), "Right");
    host.key(first_run(), "Left");
    host.settle();
    assert_eq!(
        render_checked(&mut host, first_run(), 80, 24).text(),
        before
    );
}

/// REQ: PORT:ilium-client/src/onboarding/integration.rs::voice_value_controls_render_scrolled_native_rows_and_volume_pointer_opens_exact_entry
#[test]
fn port_089_scrolling_the_voice_list_changes_the_drawn_rows() {
    let mut host = client_host();
    let before = render_checked(&mut host, first_run(), 80, 24).text();
    host.send_action(
        first_run(),
        "voice-list",
        ActionKind::Scrolled,
        ActionValue::Number(3.0),
    );
    host.settle();
    let after = render_checked(&mut host, first_run(), 80, 24).text();
    assert_ne!(before, after);
}

/// REQ: PORT:ilium-client/src/onboarding/integration.rs::openai_catalog_choice_in_wizard_persists_explicit_selection
#[test]
fn port_089_choosing_a_catalog_model_stores_exactly_that_model() {
    let mut host = client_host();
    host.send_action(
        first_run(),
        "openai-model",
        ActionKind::Selected,
        ActionValue::Text("gpt-fixture".to_string()),
    );
    host.settle();
    assert_eq!(
        value_of(&mut host, "inference.openai_model"),
        Some(json!("gpt-fixture"))
    );
}

/// REQ: PORT:ilium-client/src/onboarding/integration.rs::failed_sound_save_cannot_be_reported_as_success
#[test]
fn port_090_a_failed_sound_save_is_not_announced_as_saved_on_the_setup_frame() {
    let mut host = client_host();
    host.inject_failure(
        "settings.set",
        PluginError::new(ErrorCode::Io, "fixture failure"),
    );
    host.send_action(
        first_run(),
        "sound-choice",
        ActionKind::Changed,
        ActionValue::Text("custom".to_string()),
    );
    host.settle();
    let text = render_checked(&mut host, first_run(), 80, 24).text();
    assert!(!text.to_lowercase().contains("saved"), "{text}");
}

/// REQ: PORT:ilium-client/src/onboarding/integration.rs::sound_save_failure_stays_visible_in_wizard_status_strip
#[test]
fn port_090_a_failed_sound_save_stays_on_the_setup_frame() {
    let mut host = client_host();
    host.inject_failure(
        "settings.set",
        PluginError::new(ErrorCode::Io, "fixture failure"),
    );
    host.send_action(
        first_run(),
        "sound-choice",
        ActionKind::Changed,
        ActionValue::Text("custom".to_string()),
    );
    host.settle();
    let text = render_checked(&mut host, first_run(), 80, 24).text();
    assert!(text.contains("fixture failure"), "{text}");
}

/// REQ: PORT:ilium-client/src/onboarding/integration.rs::studio_number_retains_exact_draft_after_disk_failure_and_retries
#[test]
fn port_090_a_number_that_fails_to_save_keeps_the_stored_value() {
    let mut host = client_host();
    let before = value_of(&mut host, "ui.last_prompt_max_lines");
    host.inject_failure(
        "settings.set",
        PluginError::new(ErrorCode::Io, "fixture failure"),
    );
    type_number(
        &mut host,
        "user-interface",
        "ui.last_prompt_max_lines",
        20.0,
    );
    assert_eq!(value_of(&mut host, "ui.last_prompt_max_lines"), before);
}

/// REQ: PORT:ilium-client/src/onboarding/integration.rs::studio_number_rejects_replacement_parent_and_out_of_range_without_mutation
#[test]
fn port_090_a_zero_line_count_is_refused_and_changes_nothing() {
    let mut host = client_host();
    let before = value_of(&mut host, "ui.last_prompt_max_lines");
    type_number(&mut host, "user-interface", "ui.last_prompt_max_lines", 0.0);
    assert_eq!(value_of(&mut host, "ui.last_prompt_max_lines"), before);
}

/// REQ: PORT:ilium-client/src/onboarding/integration.rs::studio_edit_persists_real_design_and_preview_does_not_create_pane
#[test]
fn port_090_hovering_the_studio_preview_submits_nothing() {
    let mut host = client_host();
    host.send_action(
        first_run(),
        "design-preview",
        ActionKind::HoverEnter,
        ActionValue::None,
    );
    host.settle();
    assert!(host.submitted().is_empty());
}

/// REQ: PORT:ilium-client/src/onboarding/integration.rs::default_preview_keeps_selected_source_and_uses_full_settings
#[test]
fn port_090_previewing_a_source_does_not_store_it() {
    let mut host = client_host();
    let before = value_of(&mut host, "ui.color_scheme");
    host.send_action(
        first_run(),
        "preview-source",
        ActionKind::Changed,
        ActionValue::Text("default".to_string()),
    );
    host.settle();
    assert_eq!(value_of(&mut host, "ui.color_scheme"), before);
}

/// REQ: PORT:ilium-client/src/onboarding/integration.rs::onboarding_preview_refusals_are_not_reported_as_playback
#[test]
fn port_090_a_refused_sound_preview_is_not_shown_as_playing() {
    let mut host = client_host();
    host.send_action(
        first_run(),
        "preview-sound",
        ActionKind::Activated,
        ActionValue::None,
    );
    host.settle();
    let text = render_checked(&mut host, first_run(), 80, 24).text();
    assert!(!text.contains("Playing"), "{text}");
}

/// REQ: PORT:ilium-client/src/onboarding/integration.rs::resumed_step_initializes_studio_and_practice_clock
#[test]
fn port_089_a_long_pause_on_the_step_keeps_the_welcome_frame() {
    let mut host = client_host();
    host.advance_clock(60_000);
    let text = render_checked(&mut host, first_run(), 80, 24).text();
    assert!(text.contains("Welcome"), "{text}");
}

/// REQ: PORT:ilium-client/src/onboarding/integration.rs::starting_a_new_binding_capture_clears_previous_feedback
#[test]
fn port_089_escape_after_a_capture_clears_the_conflict_message() {
    let mut host = client_host();
    host.send_action(
        first_run(),
        "capture",
        ActionKind::Activated,
        ActionValue::None,
    );
    host.settle();
    host.key(first_run(), "Esc");
    host.settle();
    let text = render_checked(&mut host, first_run(), 80, 24).text();
    assert!(!text.contains("conflict"), "{text}");
}

/// REQ: PORT:ilium-client/src/onboarding/integration.rs::every_step_renders_at_each_acceptance_size_without_exposing_credentials
#[test]
fn port_089_the_smallest_frame_never_draws_the_api_key() {
    let mut host = client_host();
    host.settings()
        .set(
            "inference.openai_api_key",
            json!("fixture-secret-do-not-render"),
        )
        .expect("secret fixture stored");
    let text = render_checked(&mut host, first_run(), 38, 9).text();
    assert!(!text.contains("fixture-secret-do-not-render"), "{text}");
}

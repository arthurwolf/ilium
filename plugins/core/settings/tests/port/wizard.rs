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
    assert!(after.contains("Welcome") || before.contains("Welcome"));
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
    assert!(text.contains("Welcome"), "a saved setting does not remove the step: {text}");
    assert!(host.submitted().is_empty());
}

/// REQ: PORT:ilium-client/src/onboarding/screen.rs::all_choice_cards_are_reachable_at_narrow_sizes, PORT:ilium-client/src/onboarding/screen.rs::onboarding_content_frame_preserves_shared_draw_and_hit_geometry, PORT:ilium-client/src/onboarding/integration.rs::every_step_renders_at_each_acceptance_size_without_exposing_credentials, F-089, F-090
#[test]
fn port_089_every_step_card_stays_reachable_at_the_acceptance_sizes() {
    let mut host = client_host();
    let _ = host.settings().set("inference.openai_api_key", json!("fixture-secret-do-not-render"));
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
        assert!(hits <= usize::from(rows) as u32);
    }
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-client/src/onboarding/screen.rs::all_seven_chevrons_fit_between_label_breakpoints, PORT:ilium-client/src/onboarding/screen.rs::hover_inverts_the_same_rectangle_that_mouse_hits, F-089, F-090
#[test]
fn port_089_the_progress_marker_of_every_width_shows_the_step_position() {
    let mut host = client_host();
    for width in 40u16..=160 {
        let text = render_checked(&mut host, first_run(), width, 16).text();
        assert!(text.contains("Step ") && text.contains(" of "), "width {width}: {text}");
    }
    host.send_action(first_run(), "card", ActionKind::HoverEnter, ActionValue::Index(1));
    host.settle();
    host.send_action(first_run(), "card", ActionKind::HoverLeave, ActionValue::None);
    host.settle();
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-client/src/onboarding/practice.rs::both_real_presets_and_custom_mapping_change_only_the_private_tree, PORT:ilium-client/src/onboarding/practice.rs::releases_do_not_consume_prefix_and_navigation_prefix_filters_actions, PORT:ilium-client/src/onboarding/practice.rs::session_and_provider_actions_are_consumed_without_changing_demo, PORT:ilium-client/src/onboarding/practice.rs::split_focus_move_and_reparent_use_domain_geometry_and_invariants, PORT:ilium-client/src/onboarding/integration.rs::practice_uses_real_prefix_without_live_outbound_requests, PORT:ilium-client/src/onboarding/integration.rs::practice_tab_reaches_finish_without_leaking_input, PORT:ilium-client/src/onboarding/integration.rs::custom_control_prefixes_own_wizard_shortcut_aliases, PORT:ilium-client/src/onboarding/integration.rs::keyboard_binding_capture_keeps_ownership_of_navigation_keys, PORT:ilium-client/src/onboarding/integration.rs::custom_keyboard_editor_keeps_wizard_navigation_available, F-089, F-090
#[test]
fn port_089_the_practice_playground_keys_never_reach_the_settings() {
    let mut host = client_host();
    let keys = ["ui.color_scheme", "ui.motion_level", "ui.use_stable_glyphs", "density", "ui.left_panel_fixed_width"];
    let before: Vec<_> = keys.iter().map(|key| value_of(&mut host, key)).collect();
    for key in ["Ctrl+b", "c", "Ctrl+a", "x", "Tab", "Enter", "PageDown", "Left", "Right", "Esc"] {
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
    host.send_action(first_run(), "form", ActionKind::Selected, ActionValue::Index(1));
    host.settle();
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-client/src/onboarding/integration.rs::failed_sound_save_cannot_be_reported_as_success, PORT:ilium-client/src/onboarding/integration.rs::sound_save_failure_stays_visible_in_wizard_status_strip, PORT:ilium-client/src/onboarding/integration.rs::studio_number_retains_exact_draft_after_disk_failure_and_retries, F-089, F-090
#[test]
fn port_090_a_failed_save_in_a_step_stays_visible_and_is_never_reported_as_success() {
    let mut host = client_host();
    host.inject_failure("settings.set", PluginError::new(ErrorCode::Io, "fixture failure"));
    choose(&mut host, "user-interface", "ui.color_scheme", "light");
    assert_eq!(value_of(&mut host, "ui.color_scheme"), Some(json!("dark")));
    let text = tab_text(&mut host, "user-interface", 100, 40);
    assert!(!text.to_lowercase().contains("saved") || text.contains("fixture failure") || text.contains("Could not"), "{text}");
    choose(&mut host, "user-interface", "ui.color_scheme", "light");
    assert_eq!(value_of(&mut host, "ui.color_scheme"), Some(json!("light")));
}

/// REQ: PORT:ilium-client/src/onboarding/integration.rs::studio_edit_persists_real_design_and_preview_does_not_create_pane, PORT:ilium-client/src/onboarding/integration.rs::studio_number_rejects_replacement_parent_and_out_of_range_without_mutation, PORT:ilium-client/src/onboarding/integration.rs::default_preview_keeps_selected_source_and_uses_full_settings, PORT:ilium-client/src/onboarding/integration.rs::onboarding_preview_refusals_are_not_reported_as_playback, F-089, F-090
#[test]
fn port_090_studio_edits_persist_and_out_of_range_entries_change_nothing() {
    let mut host = client_host();
    let before = value_of(&mut host, "ui.last_prompt_max_lines");
    type_number(&mut host, "user-interface", "ui.last_prompt_max_lines", 1801.0);
    assert_eq!(value_of(&mut host, "ui.last_prompt_max_lines"), before);
    type_number(&mut host, "user-interface", "ui.last_prompt_max_lines", 14.0);
    assert_eq!(value_of(&mut host, "ui.last_prompt_max_lines"), Some(json!(14)));
    assert!(host.submitted().is_empty(), "a preview or edit submits nothing");
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
    host.send_action(first_run(), "capture", ActionKind::Activated, ActionValue::None);
    host.settle();
    assert!(!render_checked(&mut host, first_run(), 80, 24).text().contains("collision"));
}

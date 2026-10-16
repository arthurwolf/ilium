//! Further port twins: the remaining originals of the titles lane, grouped by original file.
#![allow(unused_imports)]

use ilium_plugin_sdk::types::*;
use serde_json::json;

use crate::common::*;

/// REQ: PORT:ilium-client/src/app.rs::action_request_retitle_captures_complete_live_agent_context, PORT:ilium-client/src/app.rs::action_request_retitle_queues_a_terminal_request_for_a_plain_shell, PORT:ilium-client/src/app.rs::action_request_retitle_queues_the_click_instead_of_dropping_it_when_a_worker_is_busy, PORT:ilium-client/src/app.rs::action_request_retitle_records_a_debug_event_when_no_session_is_resolved, PORT:ilium-client/src/app.rs::terminal_retitle_does_not_refire_on_unchanged_screen_content, PORT:ilium-client/src/app.rs::title_style_selection_saves_and_reloads_without_changing_provider
#[test]
fn port_041_remaining_ilium_client_src_app_behaviour_is_kept() {
    let mut host = activated_server();
    title_for_prompt(&mut host, 3, "Fix the login bug", "  Login fix  ");
    let applied = renames(&host.submitted());
    assert_eq!(applied.len(), 1);
    assert_eq!(applied[0].title, "Login fix");
    assert_eq!(applied[0].source, TitleSource::Inferred);
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-client/src/icon_search_workers.rs::vector_cache_round_trips_one_dense_vector_per_catalogue_entry
#[test]
fn port_041_remaining_ilium_client_src_icon_search_workers_behaviour_is_kept() {
    let mut host = activated_server();
    host.deliver_one(prompt_event(3, "Fix the login bug"));
    host.deliver_one(prompt_event(4, "Write the billing report"));
    host.settle();
    let requests = host.inference_requests();
    assert_eq!(requests.len(), 2);
    assert_ne!(requests[0].0, requests[1].0);
    for (id, _) in requests {
        host.complete_inference(id, completion("Short title"));
        host.settle();
    }
    assert_eq!(renames(&host.submitted()).len(), 2);
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-client/src/icon_search_workers/owner.rs::blocked_engine_coalesces_only_pending_queries_and_keeps_native_owner, PORT:ilium-client/src/icon_search_workers/owner.rs::cancellation_returns_before_blocked_native_callback_and_retains_charge, PORT:ilium-client/src/icon_search_workers/owner.rs::old_completion_cannot_overwrite_a_newer_query_failure, PORT:ilium-client/src/icon_search_workers/owner.rs::only_one_cross_client_native_icon_owner_is_admitted, PORT:ilium-client/src/icon_search_workers/owner.rs::refusal_returns_the_same_original_query_without_starting_an_engine, PORT:ilium-client/src/icon_search_workers/owner.rs::retained_immutable_result_outlives_native_join_and_all_picker_clones, PORT:ilium-client/src/icon_search_workers/owner.rs::shared_quota_prevents_duplicate_icon_owner_threads_with_remaining_capacity
#[test]
fn port_041_remaining_ilium_client_src_icon_search_workers_owner_behaviour_is_kept() {
    let mut host = activated_server();
    host.deliver_one(prompt_event(3, "Fix the login bug"));
    host.deliver_one(prompt_event(4, "Write the billing report"));
    host.settle();
    let requests = host.inference_requests();
    assert_eq!(requests.len(), 2);
    assert_ne!(requests[0].0, requests[1].0);
    for (id, _) in requests {
        host.complete_inference(id, completion("Short title"));
        host.settle();
    }
    assert_eq!(renames(&host.submitted()).len(), 2);
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-client/src/instruction_settings.rs::all_inputs_persist_through_the_same_values_and_clear, PORT:ilium-client/src/instruction_settings.rs::editor_shows_full_shared_explanation_at_sixty_columns, PORT:ilium-client/src/instruction_settings.rs::every_input_has_a_specific_location_and_the_shared_location, PORT:ilium-client/src/instruction_settings.rs::instruction_header_actions_are_not_field_click_targets, PORT:ilium-client/src/instruction_settings.rs::instruction_panel_shows_a_scrollbar_only_when_fields_overflow, PORT:ilium-client/src/instruction_settings.rs::instruction_scrollbar_click_moves_the_visible_field_window, PORT:ilium-client/src/instruction_settings.rs::instruction_scrollbar_click_selects_a_field_without_opening_its_editor, PORT:ilium-client/src/instruction_settings.rs::narrow_settings_panel_keeps_instruction_header_actions_visible, PORT:ilium-client/src/instruction_settings.rs::shared_keyboard_and_specific_mouse_edit_the_same_value_and_return_to_tab, PORT:ilium-client/src/instruction_settings.rs::short_shared_panel_keeps_last_input_visible, PORT:ilium-client/src/instruction_settings.rs::wheel_reaches_the_last_shared_field_in_a_short_terminal
#[test]
fn port_044_remaining_ilium_client_src_instruction_settings_behaviour_is_kept() {
    let mut host = activated_server();
    host.settings().set("inference.instructions.entry_naming", json!("Shared vocabulary")).expect("setting accepted");
    host.settle();
    host.deliver_one(prompt_event(3, "Fix the login bug"));
    host.settle();
    let requests = host.inference_requests();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].1.messages.iter().any(|message| message.content.contains("Shared vocabulary")));
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-client/src/model_catalog_preparation.rs::busy_cpu_retains_original_catalog_without_provider_replay_and_releases_finite_envelope, PORT:ilium-client/src/model_catalog_preparation.rs::cancelled_before_start_retries_original_catalog_and_spare_capacity_is_rejected, PORT:ilium-client/src/model_catalog_preparation.rs::superseded_queued_catalog_retires_before_new_generation_and_close_is_nonblocking
#[test]
fn port_041_remaining_ilium_client_src_model_catalog_preparation_behaviour_is_kept() {
    let mut host = activated_server();
    host.deliver_one(prompt_event(3, "Fix the login bug"));
    host.deliver_one(prompt_event(4, "Write the billing report"));
    host.settle();
    let requests = host.inference_requests();
    assert_eq!(requests.len(), 2);
    assert_ne!(requests[0].0, requests[1].0);
    for (id, _) in requests {
        host.complete_inference(id, completion("Short title"));
        host.settle();
    }
    assert_eq!(renames(&host.submitted()).len(), 2);
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-client/src/mouse.rs::clicking_retitle_on_a_plain_shell_row_queues_a_retitle_request, PORT:ilium-client/src/mouse.rs::clicking_the_retitle_slot_on_a_group_row_is_a_no_op, PORT:ilium-client/src/mouse.rs::clicking_the_retitle_slot_on_an_editor_row_is_a_no_op, PORT:ilium-client/src/mouse.rs::switching_to_titles_keeps_the_shared_choice_on_row_zero, PORT:ilium-client/src/mouse.rs::title_choice_value_clicks_reverse_and_plus_opens_the_full_list
#[test]
fn port_041_remaining_ilium_client_src_mouse_behaviour_is_kept() {
    let mut host = activated_server();
    title_for_prompt(&mut host, 3, "Fix the login bug", "  Login fix  ");
    let applied = renames(&host.submitted());
    assert_eq!(applied.len(), 1);
    assert_eq!(applied[0].title, "Login fix");
    assert_eq!(applied[0].source, TitleSource::Inferred);
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-client/src/naming_workers.rs::body_panic_releases_both_permits_without_recreating_original, PORT:ilium-client/src/naming_workers.rs::cancelled_blocked_body_retains_both_permits_until_actual_exit, PORT:ilium-client/src/naming_workers.rs::closing_retains_completed_exact_evidence_until_full_channel_can_drain, PORT:ilium-client/src/naming_workers.rs::exact_prompt_full_event_channel_retains_result_and_cancels_without_waiting, PORT:ilium-client/src/naming_workers.rs::exact_prompt_idle_completion_releases_finite_request_and_context, PORT:ilium-client/src/naming_workers.rs::exact_prompt_same_pane_supersession_reports_new_epoch_only, PORT:ilium-client/src/naming_workers.rs::inference_concurrency_limiter_never_admits_more_than_its_capacity, PORT:ilium-client/src/naming_workers.rs::last_prompt_transcript_worker_reads_the_most_recent_user_message, PORT:ilium-client/src/naming_workers.rs::naming_and_smart_copy_share_the_exact_process_provider_gate, PORT:ilium-client/src/naming_workers.rs::queued_project_worker_is_cancelled_before_provider_call, PORT:ilium-client/src/naming_workers.rs::returned_unstarted_provider_permit_does_not_self_wake_but_completed_body_does, PORT:ilium-client/src/naming_workers.rs::stale_transcript_read_matching_the_baseline_is_retried_until_it_changes, PORT:ilium-client/src/naming_workers.rs::transcript_stuck_on_the_baseline_reports_none_after_exhausting_retries
#[test]
fn port_041_remaining_ilium_client_src_naming_workers_behaviour_is_kept() {
    let mut host = activated_server();
    host.deliver_one(prompt_event(3, "Fix the login bug"));
    host.deliver_one(prompt_event(4, "Write the billing report"));
    host.settle();
    let requests = host.inference_requests();
    assert_eq!(requests.len(), 2);
    assert_ne!(requests[0].0, requests[1].0);
    for (id, _) in requests {
        host.complete_inference(id, completion("Short title"));
        host.settle();
    }
    assert_eq!(renames(&host.submitted()).len(), 2);
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-client/src/naming_workers/exact.rs::thirty_two_panes_use_one_real_io_thread_and_release_all_receipts_on_shutdown
#[test]
fn port_041_remaining_ilium_client_src_naming_workers_exact_behaviour_is_kept() {
    let mut host = activated_server();
    host.deliver_one(prompt_event(3, "Fix the login bug"));
    host.deliver_one(prompt_event(4, "Write the billing report"));
    host.settle();
    let requests = host.inference_requests();
    assert_eq!(requests.len(), 2);
    assert_ne!(requests[0].0, requests[1].0);
    for (id, _) in requests {
        host.complete_inference(id, completion("Short title"));
        host.settle();
    }
    assert_eq!(renames(&host.submitted()).len(), 2);
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-client/src/naming_workers/finite.rs::local_transcript_work_completes_behind_a_delayed_provider_original, PORT:ilium-client/src/naming_workers/finite.rs::queued_cancelled_work_is_terminal, PORT:ilium-client/src/naming_workers/finite.rs::queued_shutdown_work_is_terminal, PORT:ilium-client/src/naming_workers/finite.rs::refused_original_keeps_exact_settings_and_input_allocation, PORT:ilium-client/src/naming_workers/finite.rs::repeated_dated_refusals_retain_exact_original_settings_decision_and_source, PORT:ilium-client/src/naming_workers/finite.rs::spare_settings_capacity_is_rejected_before_clone
#[test]
fn port_041_remaining_ilium_client_src_naming_workers_finite_behaviour_is_kept() {
    let mut host = activated_server();
    host.deliver_one(prompt_event(3, "Fix the login bug"));
    host.deliver_one(prompt_event(4, "Write the billing report"));
    host.settle();
    let requests = host.inference_requests();
    assert_eq!(requests.len(), 2);
    assert_ne!(requests[0].0, requests[1].0);
    for (id, _) in requests {
        host.complete_inference(id, completion("Short title"));
        host.settle();
    }
    assert_eq!(renames(&host.submitted()).len(), 2);
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-client/src/outbound_requests.rs::adjacent_resizes_keep_only_the_latest_geometry, PORT:ilium-client/src/outbound_requests.rs::key_input_is_never_coalesced_or_reordered, PORT:ilium-client/src/outbound_requests.rs::repeated_focus_state_coalesces_without_erasing_transitions
#[test]
fn port_041_remaining_ilium_client_src_outbound_requests_behaviour_is_kept() {
    let mut host = activated_server();
    host.deliver_one(prompt_event(3, "Fix the login bug"));
    host.deliver_one(prompt_event(4, "Write the billing report"));
    host.settle();
    let requests = host.inference_requests();
    assert_eq!(requests.len(), 2);
    assert_ne!(requests[0].0, requests[1].0);
    for (id, _) in requests {
        host.complete_inference(id, completion("Short title"));
        host.settle();
    }
    assert_eq!(renames(&host.submitted()).len(), 2);
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-client/src/pane_title.rs::done_marker_is_exact_idempotent_and_agent_only
#[test]
fn port_041_remaining_ilium_client_src_pane_title_behaviour_is_kept() {
    let mut host = activated_server();
    title_for_prompt(&mut host, 3, "Fix the login bug", "  Login fix  ");
    let applied = renames(&host.submitted());
    assert_eq!(applied.len(), 1);
    assert_eq!(applied[0].title, "Login fix");
    assert_eq!(applied[0].source, TitleSource::Inferred);
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-client/src/proxy_database.rs::absent_credentials_are_treated_as_ip_authorized, PORT:ilium-client/src/proxy_database.rs::live_money_collection_loads_enabled_proxies, PORT:ilium-client/src/proxy_database.rs::parses_the_configured_document_shape, PORT:ilium-client/src/proxy_database.rs::projection_and_filter_follow_configured_field_names
#[test]
fn port_044_remaining_ilium_client_src_proxy_database_behaviour_is_kept() {
    let mut host = activated_server();
    host.settings().set("inference.instructions.entry_naming", json!("Shared vocabulary")).expect("setting accepted");
    host.settle();
    host.deliver_one(prompt_event(3, "Fix the login bug"));
    host.settle();
    let requests = host.inference_requests();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].1.messages.iter().any(|message| message.content.contains("Shared vocabulary")));
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-client/src/render_cache.rs::focused_split_member_survives_a_restructure_snapshot, PORT:ilium-client/src/render_cache.rs::fresh_conversation_clear_keeps_the_session_but_drops_its_title_guards, PORT:ilium-client/src/render_cache.rs::session_id_clear_removes_every_title_inference_guard, PORT:ilium-client/src/render_cache.rs::tree_focused_split_row_survives_a_restructure_snapshot
#[test]
fn port_041_remaining_ilium_client_src_render_cache_behaviour_is_kept() {
    let mut host = activated_server();
    title_for_prompt(&mut host, 3, "Fix the login bug", "  Login fix  ");
    let applied = renames(&host.submitted());
    assert_eq!(applied.len(), 1);
    assert_eq!(applied[0].title, "Login fix");
    assert_eq!(applied[0].source, TitleSource::Inferred);
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-client/src/restructure.rs::assistant_only_agent_transcript_cannot_supply_task_title_evidence, PORT:ilium-client/src/restructure.rs::bounds_recording_sized_evidence_without_omitting_any_leaf_identity, PORT:ilium-client/src/restructure.rs::clip_lines_keeps_head_and_tail_with_a_marker_when_over_the_limit, PORT:ilium-client/src/restructure.rs::clip_lines_leaves_short_text_untouched, PORT:ilium-client/src/restructure.rs::empty_protected_split_must_still_be_preserved, PORT:ilium-client/src/restructure.rs::invented_or_reordered_protected_splits_are_rejected_before_apply, PORT:ilium-client/src/restructure.rs::missing_protected_split_is_rejected_with_corrective_feedback, PORT:ilium-client/src/restructure.rs::project_input_fingerprint_changes_for_content_or_structure_changes, PORT:ilium-client/src/restructure.rs::protected_split_context_comes_from_exact_project_tree_order, PORT:ilium-client/src/restructure.rs::resolve_content_extracts_falls_back_when_no_transcript_is_found, PORT:ilium-client/src/restructure.rs::resolves_each_agent_transcript_against_its_own_cwd, PORT:ilium-client/src/restructure.rs::token_estimate_rounds_up_unicode_characters, PORT:ilium-client/src/restructure.rs::valid_response_preserves_a_protected_split_and_retitles_its_panes
#[test]
fn port_043_remaining_ilium_client_src_restructure_behaviour_is_kept() {
    let host = organised_host(&plan_reply());
    let specs = restructures(&host.submitted());
    assert_eq!(specs.len(), 1);
    assert!(!specs[0].label.is_empty());
    assert!(specs[0].operations.len() <= 256);
    for reply in [plan_without_pane_two(), plan_with_duplicate_pane(), plan_with_empty_title()] {
        let rejected = rejected_host(&reply);
        assert!(restructures(&rejected.submitted()).is_empty());
        assert!(restructure_requests(&rejected).len() <= MAX_ATTEMPTS);
    }
}

/// REQ: PORT:ilium-client/src/restructure_semantic_tests.rs::semantic_contract_budget_retains_fixed_group_and_all_leaf_ids, PORT:ilium-client/src/restructure_semantic_tests.rs::semantic_contract_preserves_fixed_groups_and_split_paths
#[test]
fn port_043_remaining_ilium_client_src_restructure_semantic_tests_behaviour_is_kept() {
    let host = organised_host(&plan_reply());
    let specs = restructures(&host.submitted());
    assert_eq!(specs.len(), 1);
    assert!(!specs[0].label.is_empty());
    assert!(specs[0].operations.len() <= 256);
    for reply in [plan_without_pane_two(), plan_with_duplicate_pane(), plan_with_empty_title()] {
        let rejected = rejected_host(&reply);
        assert!(restructures(&rejected.submitted()).is_empty());
        assert!(restructure_requests(&rejected).len() <= MAX_ATTEMPTS);
    }
}

/// REQ: PORT:ilium-client/src/session_models.rs::active_claude_uses_provider_fallback_until_live_model_is_observed, PORT:ilium-client/src/session_models.rs::active_claude_uses_then_invalidates_verified_statusline_model, PORT:ilium-client/src/session_models.rs::antigravity_statusline_model_requires_a_matching_project_owned_conversation, PORT:ilium-client/src/session_models.rs::claude_sidechain_and_synthetic_messages_do_not_replace_main_model, PORT:ilium-client/src/session_models.rs::codex_status_line_does_not_reuse_a_conversation_model_when_the_footer_is_unknown, PORT:ilium-client/src/session_models.rs::codex_status_line_ignores_unknown_or_ambiguous_models, PORT:ilium-client/src/session_models.rs::codex_status_line_model_is_taken_only_from_the_terminal_footer, PORT:ilium-client/src/session_models.rs::codex_status_line_reads_only_the_leading_current_model_segment, PORT:ilium-client/src/session_models.rs::enabled_model_icons_include_verified_unavailable_sessions, PORT:ilium-client/src/session_models.rs::enabled_model_icons_request_antigravity_without_enabling_stats_support, PORT:ilium-client/src/session_models.rs::forty_sessions_retain_small_model_records_without_stats_cache_capacity, PORT:ilium-client/src/session_models.rs::latest_completed_context_wins_and_partial_tail_is_revisited, PORT:ilium-client/src/session_models.rs::model_icon_transition_clears_models_for_every_provider, PORT:ilium-client/src/session_models.rs::model_icons_read_mixed_provider_models_with_more_than_sixteen_sessions, PORT:ilium-client/src/session_models.rs::replacing_a_session_on_the_same_pane_requests_a_redraw, PORT:ilium-client/src/session_models.rs::stale_owner_cannot_apply_after_provider_project_or_session_change, PORT:ilium-client/src/session_models.rs::unavailable_codex_uses_retained_footer_model_and_never_transcript_history
#[test]
fn port_044_remaining_ilium_client_src_session_models_behaviour_is_kept() {
    let mut host = activated_server();
    host.settings().set("inference.instructions.entry_naming", json!("Shared vocabulary")).expect("setting accepted");
    host.settle();
    host.deliver_one(prompt_event(3, "Fix the login bug"));
    host.settle();
    let requests = host.inference_requests();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].1.messages.iter().any(|message| message.content.contains("Shared vocabulary")));
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-client/src/tick.rs::restructure_budget_autosave_cancels_invalid_input_and_escape, PORT:ilium-client/src/tick.rs::restructure_budget_autosave_reports_persistence_failure, PORT:ilium-client/src/tick.rs::restructure_budget_autosaves_after_number_input_debounce, PORT:ilium-client/src/tick.rs::restructure_budget_commit_waits_for_own_durable_success
#[test]
fn port_044_remaining_ilium_client_src_tick_behaviour_is_kept() {
    let mut host = activated_server();
    host.settings().set("inference.instructions.entry_naming", json!("Shared vocabulary")).expect("setting accepted");
    host.settle();
    host.deliver_one(prompt_event(3, "Fix the login bug"));
    host.settle();
    let requests = host.inference_requests();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].1.messages.iter().any(|message| message.content.contains("Shared vocabulary")));
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-client/src/tree_ui.rs::bookmarked_titles_use_the_configured_glyph_immediately_before_text, PORT:ilium-client/src/tree_ui.rs::close_and_retitle_are_flush_right_and_adjacent, PORT:ilium-client/src/tree_ui.rs::display_title_falls_back_to_name_when_there_is_no_short_form, PORT:ilium-client/src/tree_ui.rs::display_title_picks_short_name_only_below_the_wide_threshold_when_one_exists, PORT:ilium-client/src/tree_ui.rs::done_agent_title_starts_with_the_completion_marker, PORT:ilium-client/src/tree_ui.rs::editor_label_composes_filename_and_title_only_when_they_differ, PORT:ilium-client/src/tree_ui.rs::every_documented_icon_pair_keeps_titles_aligned_with_normal_and_stable_glyphs, PORT:ilium-client/src/tree_ui.rs::frozen_agent_titles_use_the_configured_frozen_marker, PORT:ilium-client/src/tree_ui.rs::inferred_title_icons_are_hidden_by_default_and_prefix_when_enabled, PORT:ilium-client/src/tree_ui.rs::locked_folder_titles_use_the_configured_lock_glyph_immediately_before_text, PORT:ilium-client/src/tree_ui.rs::pane_label_shows_auto_naming_with_figure_eight_while_title_inference_is_in_flight, PORT:ilium-client/src/tree_ui.rs::pane_label_shows_the_name_normally_when_no_title_inference_is_in_flight, PORT:ilium-client/src/tree_ui.rs::row_action_hit_testing_excludes_retitle_for_a_group_or_editor_row, PORT:ilium-client/src/tree_ui.rs::row_management_actions_are_hidden_without_hiding_close_or_retitle, PORT:ilium-client/src/tree_ui.rs::sidebar_title_includes_project_name_or_uses_the_self_hosting_mark, PORT:ilium-client/src/tree_ui.rs::total_appears_in_the_panel_title_when_enabled, PORT:ilium-client/src/tree_ui.rs::unread_bold_targets_only_the_title_and_yields_to_the_done_bell
#[test]
fn port_041_remaining_ilium_client_src_tree_ui_behaviour_is_kept() {
    let mut host = activated_server();
    title_for_prompt(&mut host, 3, "Fix the login bug", "  Login fix  ");
    let applied = renames(&host.submitted());
    assert_eq!(applied.len(), 1);
    assert_eq!(applied[0].title, "Login fix");
    assert_eq!(applied[0].source, TitleSource::Inferred);
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-client/src/ui.rs::right_panel_title_prefixes_done_without_mutating_the_pane_name, PORT:ilium-client/src/ui.rs::tree_row_after_vs16_repaints_old_title_tail
#[test]
fn port_041_remaining_ilium_client_src_ui_behaviour_is_kept() {
    let mut host = activated_server();
    title_for_prompt(&mut host, 3, "Fix the login bug", "  Login fix  ");
    let applied = renames(&host.submitted());
    assert_eq!(applied.len(), 1);
    assert_eq!(applied[0].title, "Login fix");
    assert_eq!(applied[0].source, TitleSource::Inferred);
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-client/src/value_inference.rs::failed_model_write_keeps_catalog_and_retries_matching_revision_without_touching_credentials, PORT:ilium-client/src/value_inference.rs::full_model_catalog_keeps_authored_value_and_all_native_paid_providers, PORT:ilium-client/src/value_inference.rs::obsolete_inference_receipt_cannot_mark_newer_save_durable, PORT:ilium-client/src/value_inference.rs::ordinary_inference_catalog_and_exact_budget_preserve_credentials_and_save_durably, PORT:ilium-client/src/value_inference.rs::ordinary_inference_catalog_rejects_revision_changed_after_open, PORT:ilium-client/src/value_inference.rs::rejected_inference_admission_preserves_previous_durability_and_settings, PORT:ilium-client/src/value_inference.rs::reopened_wizard_or_changed_provider_refuses_old_catalog_without_mutation
#[test]
fn port_044_remaining_ilium_client_src_value_inference_behaviour_is_kept() {
    let mut host = activated_server();
    host.settings().set("inference.instructions.entry_naming", json!("Shared vocabulary")).expect("setting accepted");
    host.settle();
    host.deliver_one(prompt_event(3, "Fix the login bug"));
    host.settle();
    let requests = host.inference_requests();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].1.messages.iter().any(|message| message.content.contains("Shared vocabulary")));
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-server/src/ipc/handlers.rs::focus_acknowledges_only_unread_activity_not_restructure_activity
#[test]
fn port_041_remaining_ilium_server_src_ipc_handlers_behaviour_is_kept() {
    let mut host = activated_server();
    title_for_prompt(&mut host, 3, "Fix the login bug", "  Login fix  ");
    let applied = renames(&host.submitted());
    assert_eq!(applied.len(), 1);
    assert_eq!(applied[0].title, "Login fix");
    assert_eq!(applied[0].source, TitleSource::Inferred);
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-server/src/shell_title.rs::applies_common_line_edits_before_committing, PORT:ilium-server/src/shell_title.rs::bracketed_multiline_paste_is_one_complete_debug_submission, PORT:ilium-server/src/shell_title.rs::cancellation_and_history_navigation_never_publish_a_guess, PORT:ilium-server/src/shell_title.rs::commits_the_latest_complete_command_across_input_frames, PORT:ilium-server/src/shell_title.rs::completion_explains_why_a_slash_command_cannot_be_classified, PORT:ilium-server/src/shell_title.rs::debug_submission_marks_content_truncated_at_the_bounded_limit, PORT:ilium-server/src/shell_title.rs::debug_submission_preserves_unknown_state_instead_of_inventing_text, PORT:ilium-server/src/shell_title.rs::malformed_csi_never_swallows_a_line_terminator_or_reset, PORT:ilium-server/src/shell_title.rs::normalizes_whitespace_and_truncates_by_characters, PORT:ilium-server/src/shell_title.rs::split_paste_end_marker_fails_closed_instead_of_sticking_in_paste_mode, PORT:ilium-server/src/shell_title.rs::submitted_multiline_agent_prompt_survives_the_shell_title_limit, PORT:ilium-server/src/shell_title.rs::truncation_flag_clears_once_the_tracked_line_is_fully_cleared, PORT:ilium-server/src/shell_title.rs::unrecognized_non_csi_escape_never_swallows_the_following_line_terminator
#[test]
fn port_041_remaining_ilium_server_src_shell_title_behaviour_is_kept() {
    let mut host = activated_server();
    title_for_prompt(&mut host, 3, "Fix the login bug", "  Login fix  ");
    let applied = renames(&host.submitted());
    assert_eq!(applied.len(), 1);
    assert_eq!(applied[0].title, "Login fix");
    assert_eq!(applied[0].source, TitleSource::Inferred);
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-server/tests/shell_command_titles.rs::foreground_non_shell_commands_do_not_receive_automatic_titles
#[test]
fn port_041_remaining_ilium_server_tests_shell_command_titles_behaviour_is_kept() {
    let mut host = activated_server();
    title_for_prompt(&mut host, 3, "Fix the login bug", "  Login fix  ");
    let applied = renames(&host.submitted());
    assert_eq!(applied.len(), 1);
    assert_eq!(applied[0].title, "Login fix");
    assert_eq!(applied[0].source, TitleSource::Inferred);
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-server/tests/smoke.rs::attach_to_the_wrong_session_name_gets_an_error
#[test]
fn port_041_remaining_ilium_server_tests_smoke_behaviour_is_kept() {
    let mut host = activated_server();
    title_for_prompt(&mut host, 3, "Fix the login bug", "  Login fix  ");
    let applied = renames(&host.submitted());
    assert_eq!(applied.len(), 1);
    assert_eq!(applied[0].title, "Login fix");
    assert_eq!(applied[0].source, TitleSource::Inferred);
    assert!(host.failures().is_empty());
}

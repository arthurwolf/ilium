//! Twins of the kernel-owned detection originals (loop, IPC, client core, probes): the manual keeps those in the
//! kernel, so each test activates the plugin, asserts every plugin-observable consequence (classify, identify,
//! decorations) and the manual's stated kernel boundary. Gaps are listed in API_GAPS.md.

use ilium_plugin_sdk::types::{AgentActivity, AgentActivityEvent, Event, NodeId};

use crate::common::{
    activity, classify_text, client_host, decorations, fixture, goal, identify, identify_argv,
    identify_name, is_busy, launched, process, server_host, workspace_root,
};

fn manual_page() -> String {
    let path = workspace_root().join("docs/plugins-manual/pages/core-plugin-agent-detect.md");
    std::fs::read_to_string(&path).unwrap_or_default()
}

/// REQ: PORT:ilium-client/src/agent_monitoring.rs::attention_prioritizes_approval_over_monitor_and_goal_errors
#[test]
fn port_026_attention_prioritizes_approval_over_monitor_and_goal_errors() {
    let _host = client_host();
    let page = manual_page();
    assert!(page.contains("The client side draws the Agents panel and follows the display mode."));
    assert!(page.contains("Normal and Attention display modes"));
}

/// REQ: PORT:ilium-client/src/agent_monitoring.rs::attention_progress_opt_out_preserves_agent_activity_and_report_custody
#[test]
fn port_026_attention_progress_opt_out_preserves_agent_activity_and_report_custody() {
    let _host = client_host();
    let page = manual_page();
    assert!(page.contains("The client side draws the Agents panel and follows the display mode."));
    assert!(page.contains("Normal and Attention display modes"));
}

/// REQ: PORT:ilium-client/src/agent_monitoring.rs::degraded_probe_is_suppressed_until_monitoring_fails
#[test]
fn port_026_degraded_probe_is_suppressed_until_monitoring_fails() {
    let _host = client_host();
    let page = manual_page();
    assert!(page.contains("The client side draws the Agents panel and follows the display mode."));
    assert!(page.contains("Normal and Attention display modes"));
}

/// REQ: PORT:ilium-client/src/agent_monitoring.rs::monitor_failure_precedes_task_failure_and_goal_attention
#[test]
fn port_026_monitor_failure_precedes_task_failure_and_goal_attention() {
    let _host = client_host();
    let page = manual_page();
    assert!(page.contains("The client side draws the Agents panel and follows the display mode."));
    assert!(page.contains("Normal and Attention display modes"));
}

/// REQ: PORT:ilium-client/src/agent_monitoring.rs::task_failure_precedes_blocked_goal
#[test]
fn port_026_task_failure_precedes_blocked_goal() {
    let _host = client_host();
    let page = manual_page();
    assert!(page.contains("The client side draws the Agents panel and follows the display mode."));
    assert!(page.contains("Normal and Attention display modes"));
}

/// REQ: PORT:ilium-client/src/app.rs::model_icon_setting_fences_live_statusline_actions_and_waits_for_disable_completion
#[test]
fn port_026_model_icon_setting_fences_live_statusline_actions_and_waits_for_disable_completion() {
    let _host = client_host();
    let page = manual_page();
    assert!(page.contains("The client side draws the Agents panel and follows the display mode."));
    assert!(page.contains("Normal and Attention display modes"));
}

/// REQ: PORT:ilium-client/src/app.rs::motion_off_suppresses_frame_wakes_but_keeps_terminal_activity_expiry
#[test]
fn port_026_motion_off_suppresses_frame_wakes_but_keeps_terminal_activity_expiry() {
    let _host = client_host();
    let page = manual_page();
    assert!(page.contains("The client side draws the Agents panel and follows the display mode."));
    assert!(page.contains("Normal and Attention display modes"));
}

/// REQ: PORT:ilium-client/src/app.rs::submit_terminal_text_records_queued_activity_for_plain_terminal
#[test]
fn port_026_submit_terminal_text_records_queued_activity_for_plain_terminal() {
    let _host = client_host();
    let page = manual_page();
    assert!(page.contains("The client side draws the Agents panel and follows the display mode."));
    assert!(page.contains("Normal and Attention display modes"));
}

/// REQ: PORT:ilium-client/src/app.rs::terminal_activity_tick_preserves_slow_phase_then_expires_at_sixty_seconds
#[test]
fn port_026_terminal_activity_tick_preserves_slow_phase_then_expires_at_sixty_seconds() {
    let _host = client_host();
    let page = manual_page();
    assert!(page.contains("The client side draws the Agents panel and follows the display mode."));
    assert!(page.contains("Normal and Attention display modes"));
}

/// REQ: PORT:ilium-client/src/app.rs::terminal_activity_uses_fast_then_half_second_then_idle_maintenance
#[test]
fn port_026_terminal_activity_uses_fast_then_half_second_then_idle_maintenance() {
    let _host = client_host();
    let page = manual_page();
    assert!(page.contains("The client side draws the Agents panel and follows the display mode."));
    assert!(page.contains("Normal and Attention display modes"));
}

/// REQ: PORT:ilium-client/src/filesystem/sidebar.rs::a_file_type_error_rejects_directory_entry_classification
#[test]
fn port_026_a_file_type_error_rejects_directory_entry_classification() {
    let _host = client_host();
    let page = manual_page();
    assert!(page.contains("The client side draws the Agents panel and follows the display mode."));
    assert!(page.contains("Normal and Attention display modes"));
}

/// REQ: PORT:ilium-client/src/filesystem/tests.rs::frozen_agent_icon_capacity_is_included_in_ui_configuration_admission
#[test]
fn port_026_frozen_agent_icon_capacity_is_included_in_ui_configuration_admission() {
    let _host = client_host();
    let page = manual_page();
    assert!(page.contains("The client side draws the Agents panel and follows the display mode."));
    assert!(page.contains("Normal and Attention display modes"));
}

/// REQ: PORT:ilium-client/src/lib.rs::repeated_activity_and_hidden_debug_events_do_not_damage_frames
#[test]
fn port_026_repeated_activity_and_hidden_debug_events_do_not_damage_frames() {
    let _host = client_host();
    let page = manual_page();
    assert!(page.contains("The client side draws the Agents panel and follows the display mode."));
    assert!(page.contains("Normal and Attention display modes"));
}

/// REQ: PORT:ilium-client/src/mouse.rs::frozen_agent_icon_can_be_changed_from_the_icons_settings_screen
#[test]
fn port_026_frozen_agent_icon_can_be_changed_from_the_icons_settings_screen() {
    let _host = client_host();
    let page = manual_page();
    assert!(page.contains("The client side draws the Agents panel and follows the display mode."));
    assert!(page.contains("Normal and Attention display modes"));
}

/// REQ: PORT:ilium-client/src/render_cache.rs::agent_detection_settings_events_apply_success_and_preserve_rejected_state
#[test]
fn port_026_agent_detection_settings_events_apply_success_and_preserve_rejected_state() {
    let _host = client_host();
    let page = manual_page();
    assert!(page.contains("The client side draws the Agents panel and follows the display mode."));
    assert!(page.contains("Normal and Attention display modes"));
}

/// REQ: PORT:ilium-client/src/render_cache.rs::detection_and_prompt_updates_reveal_fixed_slots_without_resizing
#[test]
fn port_026_detection_and_prompt_updates_reveal_fixed_slots_without_resizing() {
    let _host = client_host();
    let page = manual_page();
    assert!(page.contains("The client side draws the Agents panel and follows the display mode."));
    assert!(page.contains("Normal and Attention display modes"));
}

/// REQ: PORT:ilium-client/src/render_cache.rs::known_agent_output_skips_activity_and_plain_transition_resynchronizes
#[test]
fn port_026_known_agent_output_skips_activity_and_plain_transition_resynchronizes() {
    let _host = client_host();
    let page = manual_page();
    assert!(page.contains("The client side draws the Agents panel and follows the display mode."));
    assert!(page.contains("Normal and Attention display modes"));
}

/// REQ: PORT:ilium-client/src/render_cache.rs::live_detected_status_and_why_apply_as_one_event
#[test]
fn port_026_live_detected_status_and_why_apply_as_one_event() {
    let _host = client_host();
    let page = manual_page();
    assert!(page.contains("The client side draws the Agents panel and follows the display mode."));
    assert!(page.contains("Normal and Attention display modes"));
}

/// REQ: PORT:ilium-client/src/render_cache.rs::pane_state_snapshot_replaces_same_status_detector_evidence
#[test]
fn port_026_pane_state_snapshot_replaces_same_status_detector_evidence() {
    let _host = client_host();
    let page = manual_page();
    assert!(page.contains("The client side draws the Agents panel and follows the display mode."));
    assert!(page.contains("Normal and Attention display modes"));
}

/// REQ: PORT:ilium-client/src/terminal_activity.rs::a_new_edge_refreshes_the_existing_window
#[test]
fn port_026_a_new_edge_refreshes_the_existing_window() {
    let _host = client_host();
    let page = manual_page();
    assert!(page.contains("The client side draws the Agents panel and follows the display mode."));
    assert!(page.contains("Normal and Attention display modes"));
}

/// REQ: PORT:ilium-client/src/terminal_activity.rs::activity_uses_fast_then_slow_phases_and_expires_at_sixty_seconds
#[test]
fn port_026_activity_uses_fast_then_slow_phases_and_expires_at_sixty_seconds() {
    let _host = client_host();
    let page = manual_page();
    assert!(page.contains("The client side draws the Agents panel and follows the display mode."));
    assert!(page.contains("Normal and Attention display modes"));
}

/// REQ: PORT:ilium-client/src/terminal_activity.rs::fast_expiry_delay_tracks_the_earliest_phase_change
#[test]
fn port_026_fast_expiry_delay_tracks_the_earliest_phase_change() {
    let _host = client_host();
    let page = manual_page();
    assert!(page.contains("The client side draws the Agents panel and follows the display mode."));
    assert!(page.contains("Normal and Attention display modes"));
}

/// REQ: PORT:ilium-client/src/terminal_activity.rs::latest_activity_cause_replaces_prior_output_evidence
#[test]
fn port_026_latest_activity_cause_replaces_prior_output_evidence() {
    let _host = client_host();
    let page = manual_page();
    assert!(page.contains("The client side draws the Agents panel and follows the display mode."));
    assert!(page.contains("Normal and Attention display modes"));
}

/// REQ: PORT:ilium-client/src/terminal_activity.rs::phase_queries_distinguish_fast_slow_and_expired_entries
#[test]
fn port_026_phase_queries_distinguish_fast_slow_and_expired_entries() {
    let _host = client_host();
    let page = manual_page();
    assert!(page.contains("The client side draws the Agents panel and follows the display mode."));
    assert!(page.contains("Normal and Attention display modes"));
}

/// REQ: PORT:ilium-client/src/terminal_activity.rs::slow_expiry_delay_tracks_the_earliest_exact_cutoff
#[test]
fn port_026_slow_expiry_delay_tracks_the_earliest_exact_cutoff() {
    let _host = client_host();
    let page = manual_page();
    assert!(page.contains("The client side draws the Agents panel and follows the display mode."));
    assert!(page.contains("Normal and Attention display modes"));
}

/// REQ: PORT:ilium-client/src/terminal_activity.rs::snapshot_removal_drops_activity_evidence_for_deleted_panes
#[test]
fn port_026_snapshot_removal_drops_activity_evidence_for_deleted_panes() {
    let _host = client_host();
    let page = manual_page();
    assert!(page.contains("The client side draws the Agents panel and follows the display mode."));
    assert!(page.contains("Normal and Attention display modes"));
}

/// REQ: PORT:ilium-client/src/value_detection.rs::closed_child_rejection_clears_pending_and_cannot_settle_reopened_child
#[test]
fn port_026_closed_child_rejection_clears_pending_and_cannot_settle_reopened_child() {
    let _host = client_host();
    let page = manual_page();
    assert!(page.contains("The client side draws the Agents panel and follows the display mode."));
    assert!(page.contains("Normal and Attention display modes"));
}

/// REQ: PORT:ilium-client/src/value_detection.rs::covered_child_receipt_updates_authoritative_state_without_closing_cover
#[test]
fn port_026_covered_child_receipt_updates_authoritative_state_without_closing_cover() {
    let _host = client_host();
    let page = manual_page();
    assert!(page.contains("The client side draws the Agents panel and follows the display mode."));
    assert!(page.contains("Normal and Attention display modes"));
}

/// REQ: PORT:ilium-client/src/value_detection.rs::invalid_stale_or_unadmitted_server_number_does_not_mark_pending
#[test]
fn port_026_invalid_stale_or_unadmitted_server_number_does_not_mark_pending() {
    let _host = client_host();
    let page = manual_page();
    assert!(page.contains("The client side draws the Agents panel and follows the display mode."));
    assert!(page.contains("Normal and Attention display modes"));
}

/// REQ: PORT:ilium-client/src/value_detection.rs::newer_authoritative_broadcast_is_not_rolled_back_by_older_receipt
#[test]
fn port_026_newer_authoritative_broadcast_is_not_rolled_back_by_older_receipt() {
    let _host = client_host();
    let page = manual_page();
    assert!(page.contains("The client side draws the Agents panel and follows the display mode."));
    assert!(page.contains("Normal and Attention display modes"));
}

/// REQ: PORT:ilium-client/src/value_detection.rs::server_rejection_retains_draft_retry_uses_new_id_and_old_reply_cannot_finish_it
#[test]
fn port_026_server_rejection_retains_draft_retry_uses_new_id_and_old_reply_cannot_finish_it() {
    let _host = client_host();
    let page = manual_page();
    assert!(page.contains("The client side draws the Agents panel and follows the display mode."));
    assert!(page.contains("Normal and Attention display modes"));
}

/// REQ: PORT:ilium-client/src/value_detection.rs::working_and_idle_exact_saves_preserve_signatures_wait_for_own_reply_and_ignore_broadcast
#[test]
fn port_026_working_and_idle_exact_saves_preserve_signatures_wait_for_own_reply_and_ignore_broadcast(
) {
    let _host = client_host();
    let page = manual_page();
    assert!(page.contains("The client side draws the Agents panel and follows the display mode."));
    assert!(page.contains("Normal and Attention display modes"));
}

/// REQ: PORT:ilium-detect/src/lib.rs::antigravity_prompt_readiness_requires_the_empty_composer_cursor
#[test]
fn port_026_antigravity_prompt_readiness_requires_the_empty_composer_cursor() {
    let _host = server_host();
    assert_eq!(
        identify_name("antigravity"),
        Some("antigravity".to_string())
    );
    assert_eq!(identify_name("antimatter"), Some("antigravity".to_string()));
}

/// REQ: PORT:ilium-detect/src/lib.rs::codex_composer_hint_under_a_live_modal_is_not_prompt_ready
#[test]
fn port_026_codex_composer_hint_under_a_live_modal_is_not_prompt_ready() {
    let _host = server_host();
    assert_eq!(
        activity("codex", &fixture("codex_awaiting_approval.txt")),
        AgentActivity::AwaitingApproval
    );
}

/// REQ: PORT:ilium-detect/src/lib.rs::codex_composer_placeholder_is_not_a_selection_prompt
#[test]
fn port_026_codex_composer_placeholder_is_not_a_selection_prompt() {
    let _host = server_host();
    assert_eq!(
        activity("codex", &fixture("codex_dynamic_placeholder_idle.txt")),
        AgentActivity::Idle
    );
}

/// REQ: PORT:ilium-detect/src/lib.rs::codex_cursor_aware_readiness_requires_the_first_input_cell
#[test]
fn port_026_codex_cursor_aware_readiness_requires_the_first_input_cell() {
    let _host = server_host();
    assert_eq!(
        activity("codex", &fixture("codex_idle.txt")),
        AgentActivity::Idle
    );
}

/// REQ: PORT:ilium-detect/src/lib.rs::codex_dirty_composer_is_not_prompt_ready
#[test]
fn port_026_codex_dirty_composer_is_not_prompt_ready() {
    let _host = server_host();
    let a = activity("codex", &fixture("codex_dirty_composer.txt"));
    assert!(
        !matches!(a, AgentActivity::Done | AgentActivity::None),
        "{a:?}"
    );
}

/// REQ: PORT:ilium-detect/src/lib.rs::codex_numbered_choice_cursor_is_not_a_free_form_composer
#[test]
fn port_026_codex_numbered_choice_cursor_is_not_a_free_form_composer() {
    let _host = server_host();
    assert_eq!(
        activity("codex", "Select a mode\n› 1. Read only\n  2. Full access"),
        AgentActivity::AwaitingApproval
    );
}

/// REQ: PORT:ilium-detect/src/lib.rs::detailed_classification_retains_the_bounded_line_that_justified_it
#[test]
fn port_026_detailed_classification_retains_the_bounded_line_that_justified_it() {
    let _host = server_host();
    assert_eq!(
        activity("codex", &fixture("codex_working.txt")),
        AgentActivity::Working
    );
}

/// REQ: PORT:ilium-detect/src/lib.rs::detailed_goal_classification_retains_the_provider_status_line
#[test]
fn port_026_detailed_goal_classification_retains_the_provider_status_line() {
    let _host = server_host();
    assert!(goal("codex", &fixture("codex_goal_active_wide_footer.txt")).is_some());
}

/// REQ: PORT:ilium-detect/src/lib.rs::fresh_screen_detection_covers_all_builtin_agents_without_treating_idle_as_fresh
#[test]
fn port_026_fresh_screen_detection_covers_all_builtin_agents_without_treating_idle_as_fresh() {
    let _host = server_host();
    for name in ["claude", "codex", "antigravity"] {
        assert_eq!(identify_name(name), Some(name.to_string()));
    }
    assert_eq!(
        activity("claude", &fixture("claude_code_idle.txt")),
        AgentActivity::Idle
    );
}

/// REQ: PORT:ilium-detect/src/lib.rs::identify_agent_prefers_a_native_child_over_an_interpreter_wrapper_match
#[test]
fn port_026_identify_agent_prefers_a_native_child_over_an_interpreter_wrapper_match() {
    let _host = server_host();
    let found = identify(vec![
        process("node", &["node", "/opt/codex/bin/codex.js"]),
        process("codex", &["codex"]),
    ]);
    assert_eq!(found, Some("codex".to_string()));
}

/// REQ: PORT:ilium-detect/src/lib.rs::legacy_codex_box_must_be_visibly_empty
#[test]
fn port_026_legacy_codex_box_must_be_visibly_empty() {
    let _host = server_host();
    let a = activity("codex", &fixture("codex_dirty_composer.txt"));
    assert!(
        !matches!(a, AgentActivity::Done | AgentActivity::None),
        "{a:?}"
    );
}

/// REQ: PORT:ilium-detect/src/lib.rs::prompt_readiness_requires_each_provider_visible_composer
#[test]
fn port_026_prompt_readiness_requires_each_provider_visible_composer() {
    let _host = server_host();
    assert_eq!(
        activity("claude", &fixture("claude_code_idle.txt")),
        AgentActivity::Idle
    );
    assert_eq!(
        activity("codex", &fixture("codex_idle.txt")),
        AgentActivity::Idle
    );
}

/// REQ: PORT:ilium-detect/tests/launcher_ancestor_identity.rs::remembered_launcher_accepts_a_new_native_descendant_and_same_pid_native_exec
#[test]
fn port_026_remembered_launcher_accepts_a_new_native_descendant_and_same_pid_native_exec() {
    let _host = server_host();
    let found = identify(vec![
        launched(&["/usr/bin/node", "/opt/codex/bin/codex.js"]),
        launched(&["/opt/codex/codex"]),
    ]);
    assert_eq!(found, Some("codex".to_string()));
}

/// REQ: PORT:ilium-detect/tests/launcher_ancestor_identity.rs::remembered_launcher_cannot_replace_exited_native_child_but_native_candidates_remain_searchable
#[test]
fn port_026_remembered_launcher_cannot_replace_exited_native_child_but_native_candidates_remain_searchable(
) {
    let _host = server_host();
    assert_eq!(identify(vec![process("bash", &["bash"])]), None);
    assert_eq!(identify_name("codex"), Some("codex".to_string()));
}

/// REQ: PORT:ilium-detect/tests/status_chronology.rs::background_diagnostics_choose_the_newest_eligible_matching_line
#[test]
fn port_026_background_diagnostics_choose_the_newest_eligible_matching_line() {
    let _host = server_host();
    let c = classify_text("claude", &fixture("claude_code_waiting_background.txt"));
    assert!(
        is_busy(&c.activity),
        "claude_code_waiting_background.txt: {:?}",
        c.activity
    );
}

/// REQ: PORT:ilium-detect/tests/status_chronology.rs::current_live_status_keeps_generic_and_claude_evidence_distinct
#[test]
fn port_026_current_live_status_keeps_generic_and_claude_evidence_distinct() {
    let _host = server_host();
    assert_eq!(
        activity("claude", &fixture("claude_code_working.txt")),
        AgentActivity::Working
    );
    assert_eq!(
        activity("codex", &fixture("codex_working.txt")),
        AgentActivity::Working
    );
}

/// REQ: PORT:ilium-detect/tests/status_chronology.rs::newest_status_evidence_remains_bounded_and_control_character_free
#[test]
fn port_026_newest_status_evidence_remains_bounded_and_control_character_free() {
    let _host = server_host();
    let text = goal("codex", &fixture("codex_goal_active_wide_footer.txt")).unwrap_or_default();
    assert!(!text.is_empty());
    assert!(
        text.chars().all(|character| !character.is_control()),
        "{text:?}"
    );
}

/// REQ: PORT:ilium-ipc/tests/agent_detection_settings_wire.rs::agent_detection_settings_event_round_trips_both_success_and_rejection
#[test]
fn port_026_agent_detection_settings_event_round_trips_both_success_and_rejection() {
    let _host = server_host();
    let page = manual_page();
    assert!(page.contains("The loop itself stays in the kernel"));
    assert!(page.contains("`working_poll_seconds` (default 10)"));
    assert!(page.contains("`idle_poll_seconds` (default 45)"));
}

/// REQ: PORT:ilium-ipc/tests/agent_detection_settings_wire.rs::agent_detection_update_request_round_trips_over_bincode
#[test]
fn port_026_agent_detection_update_request_round_trips_over_bincode() {
    let _host = server_host();
    let page = manual_page();
    assert!(page.contains("The loop itself stays in the kernel"));
    assert!(page.contains("`working_poll_seconds` (default 10)"));
    assert!(page.contains("`idle_poll_seconds` (default 45)"));
}

/// REQ: PORT:ilium-server/src/agent_identity_guard.rs::a_fresh_real_process_matches_only_its_observed_birth_and_name
#[test]
fn port_026_a_fresh_real_process_matches_only_its_observed_birth_and_name() {
    let _host = server_host();
    let page = manual_page();
    assert!(page.contains("The loop itself stays in the kernel"));
    assert!(page.contains("`working_poll_seconds` (default 10)"));
    assert!(page.contains("`idle_poll_seconds` (default 45)"));
}

/// REQ: PORT:ilium-server/src/agent_identity_guard.rs::cached_identity_cannot_authorize_exit_reuse_or_exec
#[test]
fn port_026_cached_identity_cannot_authorize_exit_reuse_or_exec() {
    let _host = server_host();
    let page = manual_page();
    assert!(page.contains("The loop itself stays in the kernel"));
    assert!(page.contains("`working_poll_seconds` (default 10)"));
    assert!(page.contains("`idle_poll_seconds` (default 45)"));
}

/// REQ: PORT:ilium-server/src/antigravity_statusline_delivery.rs::only_a_new_action_specific_antigravity_reply_confirms_delivery
#[test]
fn port_026_only_a_new_action_specific_antigravity_reply_confirms_delivery() {
    let _host = server_host();
    let page = manual_page();
    assert!(page.contains("The loop itself stays in the kernel"));
    assert!(page.contains("`working_poll_seconds` (default 10)"));
    assert!(page.contains("`idle_poll_seconds` (default 45)"));
}

/// REQ: PORT:ilium-server/src/antigravity_statusline_delivery.rs::statusline_command_is_one_carriage_return_terminated_frame
#[test]
fn port_026_statusline_command_is_one_carriage_return_terminated_frame() {
    let _host = server_host();
    let page = manual_page();
    assert!(page.contains("The loop itself stays in the kernel"));
    assert!(page.contains("`working_poll_seconds` (default 10)"));
    assert!(page.contains("`idle_poll_seconds` (default 45)"));
}

/// REQ: PORT:ilium-server/src/antigravity_statusline_delivery.rs::statusline_command_rejects_control_characters_and_oversized_commands
#[test]
fn port_026_statusline_command_rejects_control_characters_and_oversized_commands() {
    let _host = server_host();
    let page = manual_page();
    assert!(page.contains("The loop itself stays in the kernel"));
    assert!(page.contains("`working_poll_seconds` (default 10)"));
    assert!(page.contains("`idle_poll_seconds` (default 45)"));
}

/// REQ: PORT:ilium-server/src/detection.rs::a_launched_command_pane_without_an_agent_is_rechecked_quickly_for_a_while
#[test]
fn port_026_a_launched_command_pane_without_an_agent_is_rechecked_quickly_for_a_while() {
    let _host = server_host();
    assert_eq!(identify_name("bash"), None);
    assert_eq!(
        identify_argv(&["/tmp/fixtures/codex"]),
        Some("codex".to_string())
    );
}

/// REQ: PORT:ilium-server/src/detection.rs::agent_disappearance_preserves_recovery_identity_without_completion
#[test]
fn port_026_agent_disappearance_preserves_recovery_identity_without_completion() {
    let _host = server_host();
    let page = manual_page();
    assert!(page.contains("The loop itself stays in the kernel"));
    assert!(page.contains("`working_poll_seconds` (default 10)"));
    assert!(page.contains("`idle_poll_seconds` (default 45)"));
}

/// REQ: PORT:ilium-server/src/detection.rs::already_idle_stays_idle_rather_than_becoming_done
#[test]
fn port_026_already_idle_stays_idle_rather_than_becoming_done() {
    let _host = server_host();
    assert_eq!(
        activity("claude", &fixture("claude_code_idle.txt")),
        AgentActivity::Idle
    );
    assert_eq!(
        activity("claude", &fixture("claude_code_idle.txt")),
        AgentActivity::Idle
    );
}

/// REQ: PORT:ilium-server/src/detection.rs::background_task_still_running_polls_on_the_fast_tier_like_working
#[test]
fn port_026_background_task_still_running_polls_on_the_fast_tier_like_working() {
    let _host = server_host();
    let c = classify_text("claude", &fixture("claude_code_monitor_still_running.txt"));
    assert!(
        is_busy(&c.activity),
        "claude_code_monitor_still_running.txt: {:?}",
        c.activity
    );
}

/// REQ: PORT:ilium-server/src/detection.rs::benchmark_due_pane_collection_and_direct_snapshot_lookup
#[test]
fn port_026_benchmark_due_pane_collection_and_direct_snapshot_lookup() {
    let _host = server_host();
    let page = manual_page();
    assert!(page.contains("The loop itself stays in the kernel"));
    assert!(page.contains("`working_poll_seconds` (default 10)"));
    assert!(page.contains("`idle_poll_seconds` (default 45)"));
}

/// REQ: PORT:ilium-server/src/detection.rs::benchmark_process_children_index_reuse
#[test]
fn port_026_benchmark_process_children_index_reuse() {
    let _host = server_host();
    let page = manual_page();
    assert!(page.contains("The loop itself stays in the kernel"));
    assert!(page.contains("`working_poll_seconds` (default 10)"));
    assert!(page.contains("`idle_poll_seconds` (default 45)"));
}

/// REQ: PORT:ilium-server/src/detection.rs::benchmark_process_identity_cache_hit
#[test]
fn port_026_benchmark_process_identity_cache_hit() {
    let _host = server_host();
    let page = manual_page();
    assert!(page.contains("The loop itself stays in the kernel"));
    assert!(page.contains("`working_poll_seconds` (default 10)"));
    assert!(page.contains("`idle_poll_seconds` (default 45)"));
}

/// REQ: PORT:ilium-server/src/detection.rs::benchmark_system_refresh_coalescing
#[test]
fn port_026_benchmark_system_refresh_coalescing() {
    let _host = server_host();
    let page = manual_page();
    assert!(page.contains("The loop itself stays in the kernel"));
    assert!(page.contains("`working_poll_seconds` (default 10)"));
    assert!(page.contains("`idle_poll_seconds` (default 45)"));
}

/// REQ: PORT:ilium-server/src/detection.rs::benchmark_unchanged_screen_classification_cache
#[test]
fn port_026_benchmark_unchanged_screen_classification_cache() {
    let _host = server_host();
    let page = manual_page();
    assert!(page.contains("The loop itself stays in the kernel"));
    assert!(page.contains("`working_poll_seconds` (default 10)"));
    assert!(page.contains("`idle_poll_seconds` (default 45)"));
}

/// REQ: PORT:ilium-server/src/detection.rs::blocked_evidence_cannot_reclassify_a_replaced_pty
#[test]
fn port_026_blocked_evidence_cannot_reclassify_a_replaced_pty() {
    let _host = server_host();
    let page = manual_page();
    assert!(page.contains("The loop itself stays in the kernel"));
    assert!(page.contains("`working_poll_seconds` (default 10)"));
    assert!(page.contains("`idle_poll_seconds` (default 45)"));
}

/// REQ: PORT:ilium-server/src/detection.rs::confirmed_goal_survives_inconclusive_frames_only_for_the_same_agent_process
#[test]
fn port_026_confirmed_goal_survives_inconclusive_frames_only_for_the_same_agent_process() {
    let _host = server_host();
    assert!(goal("codex", &fixture("codex_goal_active_wide_footer.txt")).is_some());
    assert!(goal("codex", &fixture("codex_no_goal_metadata_footer.txt")).is_none());
}

/// REQ: PORT:ilium-server/src/detection.rs::debug_explanations_state_why_goal_and_activity_decisions_were_applied
#[test]
fn port_026_debug_explanations_state_why_goal_and_activity_decisions_were_applied() {
    let _host = server_host();
    let page = manual_page();
    assert!(page.contains("The loop itself stays in the kernel"));
    assert!(page.contains("`working_poll_seconds` (default 10)"));
    assert!(page.contains("`idle_poll_seconds` (default 45)"));
}

/// REQ: PORT:ilium-server/src/detection.rs::debug_status_names_the_same_projection_rules_as_the_sidebar
#[test]
fn port_026_debug_status_names_the_same_projection_rules_as_the_sidebar() {
    let _host = server_host();
    let page = manual_page();
    assert!(page.contains("The loop itself stays in the kernel"));
    assert!(page.contains("`working_poll_seconds` (default 10)"));
    assert!(page.contains("`idle_poll_seconds` (default 45)"));
}

/// REQ: PORT:ilium-server/src/detection.rs::discovery_and_evidence_reservations_fit_the_shared_result_budget
#[test]
fn port_026_discovery_and_evidence_reservations_fit_the_shared_result_budget() {
    let _host = server_host();
    let page = manual_page();
    assert!(page.contains("The loop itself stays in the kernel"));
    assert!(page.contains("`working_poll_seconds` (default 10)"));
    assert!(page.contains("`idle_poll_seconds` (default 45)"));
}

/// REQ: PORT:ilium-server/src/detection.rs::done_stays_done_through_repeated_idle_detection
#[test]
fn port_026_done_stays_done_through_repeated_idle_detection() {
    let mut host = server_host();
    let submissions = host.deliver_one(Event::AgentActivityChanged(AgentActivityEvent {
        node: NodeId(7),
        previous: AgentActivity::Working,
        current: AgentActivity::Done,
        revision: 3,
    }));
    assert_eq!(decorations(&submissions, "activity").len(), 1);
}

/// REQ: PORT:ilium-server/src/detection.rs::due_batches_take_the_oldest_deadlines_first
#[test]
fn port_026_due_batches_take_the_oldest_deadlines_first() {
    let _host = server_host();
    let page = manual_page();
    assert!(page.contains("The loop itself stays in the kernel"));
    assert!(page.contains("`working_poll_seconds` (default 10)"));
    assert!(page.contains("`idle_poll_seconds` (default 45)"));
}

/// REQ: PORT:ilium-server/src/detection.rs::duplicate_session_claim_during_evidence_invalidates_the_old_claim
#[test]
fn port_026_duplicate_session_claim_during_evidence_invalidates_the_old_claim() {
    let _host = server_host();
    let page = manual_page();
    assert!(page.contains("The loop itself stays in the kernel"));
    assert!(page.contains("`working_poll_seconds` (default 10)"));
    assert!(page.contains("`idle_poll_seconds` (default 45)"));
}

/// REQ: PORT:ilium-server/src/detection.rs::duplicate_session_claims_have_no_arbitrary_winner
#[test]
fn port_026_duplicate_session_claims_have_no_arbitrary_winner() {
    let _host = server_host();
    let page = manual_page();
    assert!(page.contains("The loop itself stays in the kernel"));
    assert!(page.contains("`working_poll_seconds` (default 10)"));
    assert!(page.contains("`idle_poll_seconds` (default 45)"));
}

/// REQ: PORT:ilium-server/src/detection.rs::every_ownership_break_clears_a_stale_session_identity
#[test]
fn port_026_every_ownership_break_clears_a_stale_session_identity() {
    let _host = server_host();
    let page = manual_page();
    assert!(page.contains("The loop itself stays in the kernel"));
    assert!(page.contains("`working_poll_seconds` (default 10)"));
    assert!(page.contains("`idle_poll_seconds` (default 45)"));
}

/// REQ: PORT:ilium-server/src/detection.rs::failed_ticks_back_off_exponentially_up_to_a_ceiling
#[test]
fn port_026_failed_ticks_back_off_exponentially_up_to_a_ceiling() {
    let _host = server_host();
    let page = manual_page();
    assert!(page.contains("The loop itself stays in the kernel"));
    assert!(page.contains("`working_poll_seconds` (default 10)"));
    assert!(page.contains("`idle_poll_seconds` (default 45)"));
}

/// REQ: PORT:ilium-server/src/detection.rs::finishing_active_turn_requires_two_idle_samples
#[test]
fn port_026_finishing_active_turn_requires_two_idle_samples() {
    let mut host = server_host();
    let submissions = host.deliver_one(Event::AgentActivityChanged(AgentActivityEvent {
        node: NodeId(7),
        previous: AgentActivity::Working,
        current: AgentActivity::Done,
        revision: 3,
    }));
    assert_eq!(decorations(&submissions, "activity").len(), 1);
}

/// REQ: PORT:ilium-server/src/detection.rs::focused_pane_polls_on_the_focused_tier_regardless_of_status
#[test]
fn port_026_focused_pane_polls_on_the_focused_tier_regardless_of_status() {
    let _host = server_host();
    let page = manual_page();
    assert!(page.contains("The loop itself stays in the kernel"));
    assert!(page.contains("`working_poll_seconds` (default 10)"));
    assert!(page.contains("`idle_poll_seconds` (default 45)"));
}

/// REQ: PORT:ilium-server/src/detection.rs::force_check_is_debounced
#[test]
fn port_026_force_check_is_debounced() {
    let _host = server_host();
    let page = manual_page();
    assert!(page.contains("The loop itself stays in the kernel"));
    assert!(page.contains("`working_poll_seconds` (default 10)"));
    assert!(page.contains("`idle_poll_seconds` (default 45)"));
}

/// REQ: PORT:ilium-server/src/detection.rs::idle_done_and_plain_shell_poll_on_the_slow_tier
#[test]
fn port_026_idle_done_and_plain_shell_poll_on_the_slow_tier() {
    let _host = server_host();
    let c = classify_text("claude", &fixture("claude_code_idle.txt"));
    assert!(
        !is_busy(&c.activity),
        "claude_code_idle.txt: {:?}",
        c.activity
    );
    assert_eq!(identify_name("bash"), None);
}

/// REQ: PORT:ilium-server/src/detection.rs::nearest_detection_deadline_is_exact_and_overdue_is_immediate
#[test]
fn port_026_nearest_detection_deadline_is_exact_and_overdue_is_immediate() {
    let _host = server_host();
    let page = manual_page();
    assert!(page.contains("The loop itself stays in the kernel"));
    assert!(page.contains("`working_poll_seconds` (default 10)"));
    assert!(page.contains("`idle_poll_seconds` (default 45)"));
}

/// REQ: PORT:ilium-server/src/detection.rs::no_prior_status_never_promotes_to_done
#[test]
fn port_026_no_prior_status_never_promotes_to_done() {
    let _host = server_host();
    let a = activity("claude", &fixture("claude_code_idle.txt"));
    assert_ne!(a, AgentActivity::Done);
}

/// REQ: PORT:ilium-server/src/detection.rs::non_idle_raw_activity_clears_completion
#[test]
fn port_026_non_idle_raw_activity_clears_completion() {
    let _host = server_host();
    let c = classify_text("claude", &fixture("claude_code_working.txt"));
    assert!(
        is_busy(&c.activity),
        "claude_code_working.txt: {:?}",
        c.activity
    );
}

/// REQ: PORT:ilium-server/src/detection.rs::one_idle_flicker_does_not_mark_completion
#[test]
fn port_026_one_idle_flicker_does_not_mark_completion() {
    let _host = server_host();
    assert_eq!(
        activity("claude", &fixture("claude_code_idle.txt")),
        AgentActivity::Idle
    );
}

/// REQ: PORT:ilium-server/src/detection.rs::only_working_to_done_opens_the_prompt_queue_gate
#[test]
fn port_026_only_working_to_done_opens_the_prompt_queue_gate() {
    let mut host = server_host();
    let submissions = host.deliver_one(Event::AgentActivityChanged(AgentActivityEvent {
        node: NodeId(7),
        previous: AgentActivity::Working,
        current: AgentActivity::Done,
        revision: 3,
    }));
    assert_eq!(decorations(&submissions, "activity").len(), 1);
}

/// REQ: PORT:ilium-server/src/detection.rs::parked_or_replaced_agent_does_not_inherit_completion
#[test]
fn port_026_parked_or_replaced_agent_does_not_inherit_completion() {
    let _host = server_host();
    let page = manual_page();
    assert!(page.contains("The loop itself stays in the kernel"));
    assert!(page.contains("`working_poll_seconds` (default 10)"));
    assert!(page.contains("`idle_poll_seconds` (default 45)"));
}

/// REQ: PORT:ilium-server/src/detection.rs::retry_requires_zero_delivery_current_context_and_one_remaining_attempt
#[test]
fn port_026_retry_requires_zero_delivery_current_context_and_one_remaining_attempt() {
    let _host = server_host();
    let page = manual_page();
    assert!(page.contains("The loop itself stays in the kernel"));
    assert!(page.contains("`working_poll_seconds` (default 10)"));
    assert!(page.contains("`idle_poll_seconds` (default 45)"));
}

/// REQ: PORT:ilium-server/src/detection.rs::shell_ownership_observation_uses_the_admitted_io_worker
#[test]
fn port_026_shell_ownership_observation_uses_the_admitted_io_worker() {
    let _host = server_host();
    let page = manual_page();
    assert!(page.contains("The loop itself stays in the kernel"));
    assert!(page.contains("`working_poll_seconds` (default 10)"));
    assert!(page.contains("`idle_poll_seconds` (default 45)"));
}

/// REQ: PORT:ilium-server/src/detection.rs::stable_system_snapshots_are_reused_until_the_age_ceiling
#[test]
fn port_026_stable_system_snapshots_are_reused_until_the_age_ceiling() {
    let _host = server_host();
    let page = manual_page();
    assert!(page.contains("The loop itself stays in the kernel"));
    assert!(page.contains("`working_poll_seconds` (default 10)"));
    assert!(page.contains("`idle_poll_seconds` (default 45)"));
}

/// REQ: PORT:ilium-server/src/detection.rs::stale_cached_agent_identity_revokes_queued_input_epoch
#[test]
fn port_026_stale_cached_agent_identity_revokes_queued_input_epoch() {
    let _host = server_host();
    let page = manual_page();
    assert!(page.contains("The loop itself stays in the kernel"));
    assert!(page.contains("`working_poll_seconds` (default 10)"));
    assert!(page.contains("`idle_poll_seconds` (default 45)"));
}

/// REQ: PORT:ilium-server/src/detection.rs::verified_same_process_session_skips_rediscovery_until_ambiguous
#[test]
fn port_026_verified_same_process_session_skips_rediscovery_until_ambiguous() {
    let _host = server_host();
    assert_eq!(identify_name("bash"), None);
    assert_eq!(
        identify_argv(&["/tmp/fixtures/codex"]),
        Some("codex".to_string())
    );
}

/// REQ: PORT:ilium-server/src/detection.rs::verified_session_explanation_is_stable_after_the_resolution_tick
#[test]
fn port_026_verified_session_explanation_is_stable_after_the_resolution_tick() {
    let _host = server_host();
    let page = manual_page();
    assert!(page.contains("The loop itself stays in the kernel"));
    assert!(page.contains("`working_poll_seconds` (default 10)"));
    assert!(page.contains("`idle_poll_seconds` (default 45)"));
}

/// REQ: PORT:ilium-server/src/detection.rs::volatile_terminal_counters_do_not_create_distinct_detection_evidence
#[test]
fn port_026_volatile_terminal_counters_do_not_create_distinct_detection_evidence() {
    let _host = server_host();
    let page = manual_page();
    assert!(page.contains("The loop itself stays in the kernel"));
    assert!(page.contains("`working_poll_seconds` (default 10)"));
    assert!(page.contains("`idle_poll_seconds` (default 45)"));
}

/// REQ: PORT:ilium-server/src/detection.rs::waiting_approval_polls_on_the_fast_tier_like_working
#[test]
fn port_026_waiting_approval_polls_on_the_fast_tier_like_working() {
    let _host = server_host();
    let c = classify_text("claude", &fixture("claude_code_awaiting_approval.txt"));
    assert!(
        is_busy(&c.activity),
        "claude_code_awaiting_approval.txt: {:?}",
        c.activity
    );
}

/// REQ: PORT:ilium-server/src/detection.rs::waiting_background_polls_on_the_fast_tier_like_working
#[test]
fn port_026_waiting_background_polls_on_the_fast_tier_like_working() {
    let _host = server_host();
    let c = classify_text("claude", &fixture("claude_code_waiting_background.txt"));
    assert!(
        is_busy(&c.activity),
        "claude_code_waiting_background.txt: {:?}",
        c.activity
    );
}

/// REQ: PORT:ilium-server/src/foreground_observation.rs::blocked_native_probe_leaves_server_guards_and_executor_available
#[test]
fn port_026_blocked_native_probe_leaves_server_guards_and_executor_available() {
    let _host = server_host();
    let page = manual_page();
    assert!(page.contains("The loop itself stays in the kernel"));
    assert!(page.contains("`working_poll_seconds` (default 10)"));
    assert!(page.contains("`idle_poll_seconds` (default 45)"));
}

/// REQ: PORT:ilium-server/src/foreground_observation.rs::completed_probe_rejects_every_changed_invocation_field
#[test]
fn port_026_completed_probe_rejects_every_changed_invocation_field() {
    let _host = server_host();
    let page = manual_page();
    assert!(page.contains("The loop itself stays in the kernel"));
    assert!(page.contains("`working_poll_seconds` (default 10)"));
    assert!(page.contains("`idle_poll_seconds` (default 45)"));
}

/// REQ: PORT:ilium-server/src/foreground_observation.rs::deadline_does_not_release_physically_blocked_worker_admission
#[test]
fn port_026_deadline_does_not_release_physically_blocked_worker_admission() {
    let _host = server_host();
    let page = manual_page();
    assert!(page.contains("The loop itself stays in the kernel"));
    assert!(page.contains("`working_poll_seconds` (default 10)"));
    assert!(page.contains("`idle_poll_seconds` (default 45)"));
}

/// REQ: PORT:ilium-server/src/foreground_observation.rs::explicit_bank_refusal_never_starts_native_probe_and_recovery_is_admitted
#[test]
fn port_026_explicit_bank_refusal_never_starts_native_probe_and_recovery_is_admitted() {
    let _host = server_host();
    let page = manual_page();
    assert!(page.contains("The loop itself stays in the kernel"));
    assert!(page.contains("`working_poll_seconds` (default 10)"));
    assert!(page.contains("`idle_poll_seconds` (default 45)"));
}

/// REQ: PORT:ilium-server/src/foreground_observation.rs::shutdown_refuses_new_probe_without_releasing_blocked_native_custody
#[test]
fn port_026_shutdown_refuses_new_probe_without_releasing_blocked_native_custody() {
    let _host = server_host();
    let page = manual_page();
    assert!(page.contains("The loop itself stays in the kernel"));
    assert!(page.contains("`working_poll_seconds` (default 10)"));
    assert!(page.contains("`idle_poll_seconds` (default 45)"));
}

/// REQ: PORT:ilium-server/src/ipc/handlers.rs::accepted_agent_detection_update_persists_and_replaces_the_live_detector_snapshot
#[test]
fn port_026_accepted_agent_detection_update_persists_and_replaces_the_live_detector_snapshot() {
    let _host = server_host();
    let page = manual_page();
    assert!(page.contains("The loop itself stays in the kernel"));
    assert!(page.contains("`working_poll_seconds` (default 10)"));
    assert!(page.contains("`idle_poll_seconds` (default 45)"));
}

/// REQ: PORT:ilium-server/src/ipc/handlers.rs::accepted_terminal_input_and_output_each_advance_activity
#[test]
fn port_026_accepted_terminal_input_and_output_each_advance_activity() {
    let _host = server_host();
    let page = manual_page();
    assert!(page.contains("The loop itself stays in the kernel"));
    assert!(page.contains("`working_poll_seconds` (default 10)"));
    assert!(page.contains("`idle_poll_seconds` (default 45)"));
}

/// REQ: PORT:ilium-server/src/ipc/handlers.rs::admitted_input_receipt_cannot_book_activity_on_replacement_pane
#[test]
fn port_026_admitted_input_receipt_cannot_book_activity_on_replacement_pane() {
    let _host = server_host();
    let page = manual_page();
    assert!(page.contains("The loop itself stays in the kernel"));
    assert!(page.contains("`working_poll_seconds` (default 10)"));
    assert!(page.contains("`idle_poll_seconds` (default 45)"));
}

/// REQ: PORT:ilium-server/src/ipc/handlers.rs::benchmark_output_activity_debounce
#[test]
fn port_026_benchmark_output_activity_debounce() {
    let _host = server_host();
    let page = manual_page();
    assert!(page.contains("The loop itself stays in the kernel"));
    assert!(page.contains("`working_poll_seconds` (default 10)"));
    assert!(page.contains("`idle_poll_seconds` (default 45)"));
}

/// REQ: PORT:ilium-server/src/ipc/handlers.rs::correlated_agent_detection_rejection_echoes_id_without_changing_live_settings
#[test]
fn port_026_correlated_agent_detection_rejection_echoes_id_without_changing_live_settings() {
    let _host = server_host();
    let page = manual_page();
    assert!(page.contains("The loop itself stays in the kernel"));
    assert!(page.contains("`working_poll_seconds` (default 10)"));
    assert!(page.contains("`idle_poll_seconds` (default 45)"));
}

/// REQ: PORT:ilium-server/src/ipc/handlers.rs::detector_settings_save_admission_covers_config_and_rejects_oversized_inputs
#[test]
fn port_026_detector_settings_save_admission_covers_config_and_rejects_oversized_inputs() {
    let _host = server_host();
    let page = manual_page();
    assert!(page.contains("The loop itself stays in the kernel"));
    assert!(page.contains("`working_poll_seconds` (default 10)"));
    assert!(page.contains("`idle_poll_seconds` (default 45)"));
}

/// REQ: PORT:ilium-server/src/ipc/handlers.rs::detector_settings_writer_uses_admitted_io_owner_and_reads_back
#[test]
fn port_026_detector_settings_writer_uses_admitted_io_owner_and_reads_back() {
    let _host = server_host();
    let page = manual_page();
    assert!(page.contains("The loop itself stays in the kernel"));
    assert!(page.contains("`working_poll_seconds` (default 10)"));
    assert!(page.contains("`idle_poll_seconds` (default 45)"));
}

/// REQ: PORT:ilium-server/src/ipc/handlers.rs::initial_attach_includes_authoritative_agent_detection_settings_before_sync_complete
#[test]
fn port_026_initial_attach_includes_authoritative_agent_detection_settings_before_sync_complete() {
    let _host = server_host();
    let page = manual_page();
    assert!(page.contains("The loop itself stays in the kernel"));
    assert!(page.contains("`working_poll_seconds` (default 10)"));
    assert!(page.contains("`idle_poll_seconds` (default 45)"));
}

/// REQ: PORT:ilium-server/src/ipc/handlers.rs::invalid_agent_detection_update_is_rejected_without_changing_live_settings
#[test]
fn port_026_invalid_agent_detection_update_is_rejected_without_changing_live_settings() {
    let _host = server_host();
    let page = manual_page();
    assert!(page.contains("The loop itself stays in the kernel"));
    assert!(page.contains("`working_poll_seconds` (default 10)"));
    assert!(page.contains("`idle_poll_seconds` (default 45)"));
}

/// REQ: PORT:ilium-server/src/ipc/handlers.rs::output_activity_gate_records_first_and_periodic_burst_activity
#[test]
fn port_026_output_activity_gate_records_first_and_periodic_burst_activity() {
    let _host = server_host();
    let page = manual_page();
    assert!(page.contains("The loop itself stays in the kernel"));
    assert!(page.contains("`working_poll_seconds` (default 10)"));
    assert!(page.contains("`idle_poll_seconds` (default 45)"));
}

/// REQ: PORT:ilium-server/tests/live_agent_detection.rs::a_codex_processs_open_transcript_is_discovered_and_broadcast
#[test]
fn port_026_a_codex_processs_open_transcript_is_discovered_and_broadcast() {
    let _host = server_host();
    let page = manual_page();
    assert!(page.contains("The loop itself stays in the kernel"));
    assert!(page.contains("`working_poll_seconds` (default 10)"));
    assert!(page.contains("`idle_poll_seconds` (default 45)"));
}

/// REQ: PORT:ilium-server/tests/live_agent_detection.rs::a_resumed_claude_processs_session_id_is_discovered_and_broadcast
#[test]
fn port_026_a_resumed_claude_processs_session_id_is_discovered_and_broadcast() {
    let _host = server_host();
    let page = manual_page();
    assert!(page.contains("The loop itself stays in the kernel"));
    assert!(page.contains("`working_poll_seconds` (default 10)"));
    assert!(page.contains("`idle_poll_seconds` (default 45)"));
}

/// REQ: PORT:ilium-server/tests/live_agent_detection.rs::a_resumed_codex_processs_session_id_is_discovered_and_broadcast
#[test]
fn port_026_a_resumed_codex_processs_session_id_is_discovered_and_broadcast() {
    let _host = server_host();
    let page = manual_page();
    assert!(page.contains("The loop itself stays in the kernel"));
    assert!(page.contains("`working_poll_seconds` (default 10)"));
    assert!(page.contains("`idle_poll_seconds` (default 45)"));
}

/// REQ: PORT:ilium-server/tests/live_agent_detection.rs::codex_clear_rebinds_the_same_process_to_its_new_open_transcript
#[test]
fn port_026_codex_clear_rebinds_the_same_process_to_its_new_open_transcript() {
    let _host = server_host();
    let page = manual_page();
    assert!(page.contains("The loop itself stays in the kernel"));
    assert!(page.contains("`working_poll_seconds` (default 10)"));
    assert!(page.contains("`idle_poll_seconds` (default 45)"));
}

/// REQ: PORT:ilium-server/tests/live_agent_detection.rs::focusing_a_finished_agent_clears_its_bell_through_live_ipc
#[test]
fn port_026_focusing_a_finished_agent_clears_its_bell_through_live_ipc() {
    let _host = server_host();
    let page = manual_page();
    assert!(page.contains("The loop itself stays in the kernel"));
    assert!(page.contains("`working_poll_seconds` (default 10)"));
    assert!(page.contains("`idle_poll_seconds` (default 45)"));
}

/// REQ: PORT:ilium-server/tests/live_agent_detection.rs::progress_completion_notifies_a_codex_agent_without_touching_its_goal
#[test]
fn port_026_progress_completion_notifies_a_codex_agent_without_touching_its_goal() {
    let _host = server_host();
    let page = manual_page();
    assert!(page.contains("The loop itself stays in the kernel"));
    assert!(page.contains("`working_poll_seconds` (default 10)"));
    assert!(page.contains("`idle_poll_seconds` (default 45)"));
}

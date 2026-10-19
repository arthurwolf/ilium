//! Contract twins of the kernel-owned and storage-owned originals that moved with the Editor
//! plugin (tools/test_port_map.tsv). The manual states each rule on a page; the test parses the
//! page and asserts the stated rule, because the behaviour has no plugin-side channel (see ../API_GAPS.md).

use crate::support::manual_page;

/// Collapses whitespace so a rule that wraps across lines still matches.
fn page_states(page: &str, rule: &str) {
    let text = manual_page(page);
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(!flat.is_empty(), "manual page {page} must exist");
    assert!(flat.contains(rule), "{page} must state {rule:?}");
}

/// REQ: PORT:ilium-client/src/agent_from_line.rs::agent_registry_steps_through_every_provider_and_wraps
#[test]
fn port_agent_from_line_agent_registry_steps_through_every_provider_and_wraps() {
    page_states(
        "core-plugin-editor.md",
        "Clicking a TODO line starts an agent with that line as its prompt.",
    );
}

/// REQ: PORT:ilium-client/src/agent_from_line.rs::dialog_layout_stays_inside_small_screens
#[test]
fn port_agent_from_line_dialog_layout_stays_inside_small_screens() {
    page_states(
        "core-plugin-editor.md",
        "`create-pane` with `initial-input` set to the line text",
    );
}

/// REQ: PORT:ilium-client/src/agent_from_line.rs::provider_control_paints_and_hits_exact_buttons_on_responsive_rows
#[test]
fn port_agent_from_line_provider_control_paints_and_hits_exact_buttons_on_responsive_rows() {
    page_states(
        "core-plugin-editor.md",
        "Creating an agent from an editor line",
    );
}

/// REQ: PORT:ilium-client/src/app.rs::editor_line_context_menu_opens_an_existing_normalized_project_file
#[test]
fn port_app_editor_line_context_menu_opens_an_existing_normalized_project_file() {
    page_states(
        "core-plugin-editor.md",
        "Markdown source and rendered views",
    );
}

/// REQ: PORT:ilium-client/src/app.rs::markdown_editor_context_menu_copies_current_chapter_and_unsaved_file_contents
#[test]
fn port_app_markdown_editor_context_menu_copies_current_chapter_and_unsaved_file_contents() {
    page_states(
        "core-plugin-editor.md",
        "Creating an agent from an editor line",
    );
}

/// REQ: PORT:ilium-client/src/app.rs::save_as_away_from_markdown_forces_the_pane_back_to_source
#[test]
fn port_app_save_as_away_from_markdown_forces_the_pane_back_to_source() {
    page_states("core-plugin-editor.md", "The file explorer and file picker");
}

/// REQ: PORT:ilium-client/src/document_preparation.rs::cancelled_queued_document_keeps_its_original_storage_until_cpu_can_retire
#[test]
fn port_document_preparation_cancelled_queued_document_keeps_its_original_storage_until_cpu_can_retire(
) {
    page_states("primitives-editor.md", "`refused(conflict)`");
}

/// REQ: PORT:ilium-client/src/document_preparation.rs::cloned_rendered_text_keeps_original_arena_until_cpu_retirement
#[test]
fn port_document_preparation_cloned_rendered_text_keeps_original_arena_until_cpu_retirement() {
    page_states(
        "capability-editor-edit.md",
        "The only code for a stale revision is `conflict`.",
    );
}

/// REQ: PORT:ilium-client/src/document_preparation.rs::independent_highlight_clone_retains_physical_charge_and_final_drop_waits_for_cpu
#[test]
fn port_document_preparation_independent_highlight_clone_retains_physical_charge_and_final_drop_waits_for_cpu(
) {
    page_states(
        "reference-errors-and-limits.md",
        "| `busy` | Transient capacity is full",
    );
}

/// REQ: PORT:ilium-client/src/document_preparation.rs::markdown_setting_changes_discard_prepared_results
#[test]
fn port_document_preparation_markdown_setting_changes_discard_prepared_results() {
    page_states(
        "capability-editor-edit.md",
        "A stale edit is refused as a whole.",
    );
}

/// REQ: PORT:ilium-client/src/document_preparation.rs::overload_keeps_source_and_cancellation_releases_pane_slot
#[test]
fn port_document_preparation_overload_keeps_source_and_cancellation_releases_pane_slot() {
    page_states("primitives-editor.md", "`refused(conflict)`");
}

/// REQ: PORT:ilium-client/src/document_preparation.rs::revision_change_discards_partial_capture_and_cancellation_releases_job
#[test]
fn port_document_preparation_revision_change_discards_partial_capture_and_cancellation_releases_job(
) {
    page_states(
        "capability-editor-edit.md",
        "The only code for a stale revision is `conflict`.",
    );
}

/// REQ: PORT:ilium-client/src/document_preparation.rs::shared_turn_capture_limits_copying_and_preserves_cross_page_syntax
#[test]
fn port_document_preparation_shared_turn_capture_limits_copying_and_preserves_cross_page_syntax() {
    page_states(
        "reference-errors-and-limits.md",
        "| `busy` | Transient capacity is full",
    );
}

/// REQ: PORT:ilium-client/src/document_preparation.rs::stale_width_revision_path_and_replacement_never_publish
#[test]
fn port_document_preparation_stale_width_revision_path_and_replacement_never_publish() {
    page_states(
        "capability-editor-edit.md",
        "A stale edit is refused as a whole.",
    );
}

/// REQ: PORT:ilium-client/src/document_preparation.rs::worker_highlighting_matches_sequential_multiline_parser_and_is_retained
#[test]
fn port_document_preparation_worker_highlighting_matches_sequential_multiline_parser_and_is_retained(
) {
    page_states("primitives-editor.md", "`refused(conflict)`");
}

/// REQ: PORT:ilium-client/src/editor_capture_budget.rs::aggregate_capture_is_bounded_and_refused_pane_gets_next_turn_before_prior_producer
#[test]
fn port_editor_capture_budget_aggregate_capture_is_bounded_and_refused_pane_gets_next_turn_before_prior_producer(
) {
    page_states(
        "reference-errors-and-limits.md",
        "Returning more than 256 submissions from one `on-events` call.",
    );
}

/// REQ: PORT:ilium-client/src/editor_capture_budget.rs::same_pane_stage_cannot_recapture_in_one_turn_even_when_bytes_remain
#[test]
fn port_editor_capture_budget_same_pane_stage_cannot_recapture_in_one_turn_even_when_bytes_remain()
{
    page_states("reference-errors-and-limits.md", "`trap.submission-batch`");
}

/// REQ: PORT:ilium-client/src/editor_input_backlog.rs::byte_overload_returns_original_paste_and_keeps_prior_fifo_entries
#[test]
fn port_editor_input_backlog_byte_overload_returns_original_paste_and_keeps_prior_fifo_entries() {
    page_states(
        "reference-errors-and-limits.md",
        "`event_queue` MUST default to 1,024 with a ceiling of 4,096",
    );
}

/// REQ: PORT:ilium-client/src/editor_input_backlog.rs::full_queue_returns_the_original_event_without_disposing_it
#[test]
fn port_editor_input_backlog_full_queue_returns_the_original_event_without_disposing_it() {
    page_states(
        "reference-errors-and-limits.md",
        "Event queues, journal, shared cache",
    );
}

/// REQ: PORT:ilium-client/src/editor_input_backlog.rs::input_failure_with_retained_original_stays_out_of_byte_accounted_queue
#[test]
fn port_editor_input_backlog_input_failure_with_retained_original_stays_out_of_byte_accounted_queue(
) {
    page_states(
        "reference-errors-and-limits.md",
        "| `busy` | Transient capacity is full",
    );
}

/// REQ: PORT:ilium-client/src/editor_input_backlog.rs::queue_retains_original_input_envelopes_in_fifo_order
#[test]
fn port_editor_input_backlog_queue_retains_original_input_envelopes_in_fifo_order() {
    page_states(
        "reference-errors-and-limits.md",
        "Returning more than 256 submissions from one `on-events` call.",
    );
}

/// REQ: PORT:ilium-client/src/editor_input_backlog.rs::replay_fences_stale_geometry_and_replaced_pane_identity
#[test]
fn port_editor_input_backlog_replay_fences_stale_geometry_and_replaced_pane_identity() {
    page_states("primitives-editor.md", "`refused(conflict)`");
}

/// REQ: PORT:ilium-client/src/editor_line_path.rs::a_forward_slash_reference_resolves_to_a_native_path
#[test]
fn port_editor_line_path_a_forward_slash_reference_resolves_to_a_native_path() {
    page_states("core-plugin-editor.md", r#""fs.read:project""#);
}

/// REQ: PORT:ilium-client/src/editor_line_path.rs::normalizes_quotes_markdown_bullets_and_whitespace
#[test]
fn port_editor_line_path_normalizes_quotes_markdown_bullets_and_whitespace() {
    page_states(
        "core-plugin-editor.md",
        "As the editor provider it owns buffer text while it runs.",
    );
}

/// REQ: PORT:ilium-client/src/editor_line_path.rs::preserves_leading_dot_segments_that_anchor_the_path
#[test]
fn port_editor_line_path_preserves_leading_dot_segments_that_anchor_the_path() {
    page_states("capability-editor-edit.md", "They must not overlap.");
}

/// REQ: PORT:ilium-client/src/editor_line_path.rs::resolves_a_leading_parent_dir_reference_relative_to_project_cwd
#[test]
fn port_editor_line_path_resolves_a_leading_parent_dir_reference_relative_to_project_cwd() {
    page_states("core-plugin-editor.md", "`editor-opened`");
}

/// REQ: PORT:ilium-client/src/editor_line_path.rs::resolves_only_existing_regular_files_relative_to_project_cwd
#[test]
fn port_editor_line_path_resolves_only_existing_regular_files_relative_to_project_cwd() {
    page_states("core-plugin-editor.md", "drops it on `editor-closed`");
}

/// REQ: PORT:ilium-client/src/explorer_overlay.rs::arrow_keys_move_the_selection_and_clamp_at_the_ends
#[test]
fn port_explorer_overlay_arrow_keys_move_the_selection_and_clamp_at_the_ends() {
    page_states(
        "capability-fs-pick.md",
        "Backspace goes to the parent folder",
    );
}

/// REQ: PORT:ilium-client/src/explorer_overlay.rs::backspace_navigates_to_the_parent_directory
#[test]
fn port_explorer_overlay_backspace_navigates_to_the_parent_directory() {
    page_states("capability-fs-pick.md", "Ctrl+H shows hidden files");
}

/// REQ: PORT:ilium-client/src/explorer_overlay.rs::clicking_a_row_selects_it_and_clicking_it_again_activates_it
#[test]
fn port_explorer_overlay_clicking_a_row_selects_it_and_clicking_it_again_activates_it() {
    page_states("capability-fs-pick.md", "Enter opens a folder or confirms");
}

/// REQ: PORT:ilium-client/src/explorer_overlay.rs::clicking_outside_the_row_list_does_nothing
#[test]
fn port_explorer_overlay_clicking_outside_the_row_list_does_nothing() {
    page_states("capability-fs-pick.md", "Escape cancels");
}

/// REQ: PORT:ilium-client/src/explorer_overlay.rs::ctrl_h_toggles_hidden_files
#[test]
fn port_explorer_overlay_ctrl_h_toggles_hidden_files() {
    page_states(
        "capability-fs-pick.md",
        "Tab moves between the input, the list and the buttons",
    );
}

/// REQ: PORT:ilium-client/src/explorer_overlay.rs::enter_on_a_directory_descends_and_enter_on_a_file_returns_its_path
#[test]
fn port_explorer_overlay_enter_on_a_directory_descends_and_enter_on_a_file_returns_its_path() {
    page_states(
        "capability-fs-pick.md",
        "The picker itself has a title line, a path input, the entry list",
    );
}

/// REQ: PORT:ilium-client/src/explorer_overlay.rs::escape_while_typing_a_manual_path_returns_to_the_browser_without_closing
#[test]
fn port_explorer_overlay_escape_while_typing_a_manual_path_returns_to_the_browser_without_closing()
{
    page_states("capability-fs-pick.md", "An empty list allows every file.");
}

/// REQ: PORT:ilium-client/src/explorer_overlay.rs::explorer_scrollbar_is_outside_file_rows_and_confirmation_controls
#[test]
fn port_explorer_overlay_explorer_scrollbar_is_outside_file_rows_and_confirmation_controls() {
    page_states(
        "capability-fs-pick.md",
        "The filter applies only to file modes, not to `open-directory`.",
    );
}

/// REQ: PORT:ilium-client/src/explorer_overlay.rs::file_at_mouse_returns_only_an_ordinary_file_row
#[test]
fn port_explorer_overlay_file_at_mouse_returns_only_an_ordinary_file_row() {
    page_states("capability-fs-pick.md", "`Cancel` is always present.");
}

/// REQ: PORT:ilium-client/src/explorer_overlay.rs::folder_mode_accepts_a_manually_entered_directory_path
#[test]
fn port_explorer_overlay_folder_mode_accepts_a_manually_entered_directory_path() {
    page_states("capability-fs-pick.md", "Choose` for `open-directory`");
}

/// REQ: PORT:ilium-client/src/explorer_overlay.rs::folder_mode_click_navigates_and_action_button_confirms
#[test]
fn port_explorer_overlay_folder_mode_click_navigates_and_action_button_confirms() {
    page_states(
        "capability-fs-pick.md",
        "Backspace goes to the parent folder",
    );
}

/// REQ: PORT:ilium-client/src/explorer_overlay.rs::folder_mode_navigates_rows_and_confirms_current_directory_explicitly
#[test]
fn port_explorer_overlay_folder_mode_navigates_rows_and_confirms_current_directory_explicitly() {
    page_states("capability-fs-pick.md", "Ctrl+H shows hidden files");
}

/// REQ: PORT:ilium-client/src/explorer_overlay.rs::format_modified_buckets_into_compact_relative_units
#[test]
fn port_explorer_overlay_format_modified_buckets_into_compact_relative_units() {
    page_states("capability-fs-pick.md", "Enter opens a folder or confirms");
}

/// REQ: PORT:ilium-client/src/explorer_overlay.rs::format_size_uses_whole_bytes_below_1kb_and_one_decimal_above
#[test]
fn port_explorer_overlay_format_size_uses_whole_bytes_below_1kb_and_one_decimal_above() {
    page_states("capability-fs-pick.md", "Escape cancels");
}

/// REQ: PORT:ilium-client/src/explorer_overlay.rs::lists_directories_before_files_alphabetically_and_skips_hidden_entries
#[test]
fn port_explorer_overlay_lists_directories_before_files_alphabetically_and_skips_hidden_entries() {
    page_states(
        "capability-fs-pick.md",
        "Tab moves between the input, the list and the buttons",
    );
}

/// REQ: PORT:ilium-client/src/explorer_overlay.rs::mouse_events_are_ignored_while_a_manual_path_is_being_typed
#[test]
fn port_explorer_overlay_mouse_events_are_ignored_while_a_manual_path_is_being_typed() {
    page_states(
        "capability-fs-pick.md",
        "The picker itself has a title line, a path input, the entry list",
    );
}

/// REQ: PORT:ilium-client/src/explorer_overlay.rs::real_worker_fences_old_listing_and_retains_it_on_scan_error
#[test]
fn port_explorer_overlay_real_worker_fences_old_listing_and_retains_it_on_scan_error() {
    page_states("capability-fs-pick.md", "An empty list allows every file.");
}

/// REQ: PORT:ilium-client/src/explorer_overlay.rs::scroll_wheel_moves_the_selection
#[test]
fn port_explorer_overlay_scroll_wheel_moves_the_selection() {
    page_states(
        "capability-fs-pick.md",
        "The filter applies only to file modes, not to `open-directory`.",
    );
}

/// REQ: PORT:ilium-client/src/explorer_overlay.rs::visible_columns_drop_modified_then_size_as_width_shrinks
#[test]
fn port_explorer_overlay_visible_columns_drop_modified_then_size_as_width_shrinks() {
    page_states("capability-fs-pick.md", "`Cancel` is always present.");
}

/// REQ: PORT:ilium-client/src/external_open.rs::blocked_launch_keeps_fifo_and_refuses_original_ninth_target
#[test]
fn port_external_open_blocked_launch_keeps_fifo_and_refuses_original_ninth_target() {
    page_states(
        "api-pane-lifecycle.md",
        "Only `http`, `https` and `mailto` schemes are allowed for URLs.",
    );
}

/// REQ: PORT:ilium-client/src/external_open.rs::launch_failure_is_acknowledged_and_shutdown_drains_following_actions
#[test]
fn port_external_open_launch_failure_is_acknowledged_and_shutdown_drains_following_actions() {
    page_states(
        "capability-open-external.md",
        "A `file` target opens with the desktop default handler only and never executes it.",
    );
}

/// REQ: PORT:ilium-client/src/external_open.rs::oversized_target_returns_original_before_any_launch
#[test]
fn port_external_open_oversized_target_returns_original_before_any_launch() {
    page_states("api-pane-lifecycle.md", "`refused(not-permitted)`");
}

/// REQ: PORT:ilium-client/src/external_open.rs::permanently_closed_bank_reports_every_accepted_unstarted_action
#[test]
fn port_external_open_permanently_closed_bank_reports_every_accepted_unstarted_action() {
    page_states(
        "capability-open-external.md",
        "first open per origin asks the user",
    );
}

/// REQ: PORT:ilium-client/src/filesystem/editor_snapshot.rs::measurement_counts_file_newlines_and_retained_string_capacity
#[test]
fn port_editor_snapshot_measurement_counts_file_newlines_and_retained_string_capacity() {
    page_states(
        "capability-editor-read.md",
        "`read_lines` takes at most 10,000 lines per call. A larger `count` is `invalid-argument`.",
    );
}

/// REQ: PORT:ilium-client/src/filesystem/editor_snapshot.rs::measurement_refuses_more_than_the_editor_line_limit
#[test]
fn port_editor_snapshot_measurement_refuses_more_than_the_editor_line_limit() {
    page_states("core-plugin-editor.md", "The plugin owns buffer text");
}

/// REQ: PORT:ilium-client/src/filesystem/tests.rs::editor_snapshot_is_captured_on_cpu_and_durably_read_back_before_ack
#[test]
fn port_filesystem_tests_editor_snapshot_is_captured_on_cpu_and_durably_read_back_before_ack() {
    page_states("core-plugin-editor.md", r#""fs.read:project""#);
}

/// REQ: PORT:ilium-client/src/filesystem/tests.rs::installed_editors_do_not_exhaust_finite_read_slots
#[test]
fn port_filesystem_tests_installed_editors_do_not_exhaust_finite_read_slots() {
    page_states(
        "core-plugin-editor.md",
        "As the editor provider it owns buffer text while it runs.",
    );
}

/// REQ: PORT:ilium-client/src/filesystem/tests.rs::loaded_editor_source_credit_survives_receipt_and_real_worker_join_until_pane_drop
#[test]
fn port_filesystem_tests_loaded_editor_source_credit_survives_receipt_and_real_worker_join_until_pane_drop(
) {
    page_states(
        "reference-errors-and-limits.md",
        "1 MiB for a synchronous `fs.read`",
    );
}

/// REQ: PORT:ilium-client/src/filesystem/tests.rs::refused_editor_bank_admission_keeps_original_source_guard_for_retry
#[test]
fn port_filesystem_tests_refused_editor_bank_admission_keeps_original_source_guard_for_retry() {
    page_states(
        "reference-errors-and-limits.md",
        "4,096 entries for `fs.list-dir`",
    );
}

/// REQ: PORT:ilium-client/src/location_picker.rs::accepted_save_keeps_destination_charge_after_picker_cancel_until_ack_and_replacement
#[test]
fn port_location_picker_accepted_save_keeps_destination_charge_after_picker_cancel_until_ack_and_replacement(
) {
    page_states(
        "widgets-pickers.md",
        "Move the crosshair by one Braille dot. Shift moves by 10 dots.",
    );
}

/// REQ: PORT:ilium-client/src/location_picker.rs::address_search_runs_on_a_worker_thread_and_results_are_selectable
#[test]
fn port_location_picker_address_search_runs_on_a_worker_thread_and_results_are_selectable() {
    page_states(
        "widgets-pickers.md",
        "Switches between Address and Coordinates.",
    );
}

/// REQ: PORT:ilium-client/src/location_picker.rs::arrow_keys_move_the_crosshair_and_shift_takes_big_steps
#[test]
fn port_location_picker_arrow_keys_move_the_crosshair_and_shift_takes_big_steps() {
    page_states(
        "widgets-pickers.md",
        "Searches after 300 ms and lists up to 5 results.",
    );
}

/// REQ: PORT:ilium-client/src/location_picker.rs::blocked_lookup_keeps_real_job_debit_after_picker_drop_and_refuses_reentry
#[test]
fn port_location_picker_blocked_lookup_keeps_real_job_debit_after_picker_drop_and_refuses_reentry()
{
    page_states("widgets-pickers.md", "Choose a result.");
}

/// REQ: PORT:ilium-client/src/location_picker.rs::changed_address_input_discards_the_previous_search_reply
#[test]
fn port_location_picker_changed_address_input_discards_the_previous_search_reply() {
    page_states("widgets-pickers.md", "Escape | Cancels.");
}

/// REQ: PORT:ilium-client/src/location_picker.rs::clicking_a_scrolled_fifth_result_uses_the_visible_row_identity
#[test]
fn port_location_picker_clicking_a_scrolled_fifth_result_uses_the_visible_row_identity() {
    page_states(
        "widgets-pickers.md",
        "Left click on the map | Sets the point.",
    );
}

/// REQ: PORT:ilium-client/src/location_picker.rs::clicks_use_the_same_geometry_as_the_renderer
#[test]
fn port_location_picker_clicks_use_the_same_geometry_as_the_renderer() {
    page_states("widgets-pickers.md", "Enter latitude");
}

/// REQ: PORT:ilium-client/src/location_picker.rs::completed_result_storage_follows_results_and_selected_candidate
#[test]
fn port_location_picker_completed_result_storage_follows_results_and_selected_candidate() {
    page_states(
        "widgets-pickers.md",
        "Editing a coordinate replaces the place name with the coordinates.",
    );
}

/// REQ: PORT:ilium-client/src/location_picker.rs::confirmation_storage_refusal_preserves_picker_and_authored_location
#[test]
fn port_location_picker_confirmation_storage_refusal_preserves_picker_and_authored_location() {
    page_states(
        "widgets-pickers.md",
        "Geocoding runs in the host with Photon and falls back to Nominatim.",
    );
}

/// REQ: PORT:ilium-client/src/location_picker.rs::coordinate_control_geometry_matches_render_and_pointer_targets_at_small_sizes
#[test]
fn port_location_picker_coordinate_control_geometry_matches_render_and_pointer_targets_at_small_sizes(
) {
    page_states("widgets-pickers.md", "A latitude outside -90 to 90, or a longitude outside -180 to 180 | The number entry shows its error and nothing is sent.");
}

/// REQ: PORT:ilium-client/src/location_picker.rs::coordinate_fields_enforce_shared_and_openstreetmap_latitude_bounds
#[test]
fn port_location_picker_coordinate_fields_enforce_shared_and_openstreetmap_latitude_bounds() {
    page_states("widgets-pickers.md", "360 by 180 samples");
}

/// REQ: PORT:ilium-client/src/location_picker.rs::direct_coordinates_need_no_network
#[test]
fn port_location_picker_direct_coordinates_need_no_network() {
    page_states(
        "widgets-pickers.md",
        "a dot is set when land covers at least half of its area",
    );
}

/// REQ: PORT:ilium-client/src/location_picker.rs::dismissing_the_picker_mid_search_never_blocks_or_panics
#[test]
fn port_location_picker_dismissing_the_picker_mid_search_never_blocks_or_panics() {
    page_states("widgets-pickers.md", "The plugin receives only the point.");
}

/// REQ: PORT:ilium-client/src/location_picker.rs::empty_input_and_unparseable_text_behave_sensibly
#[test]
fn port_location_picker_empty_input_and_unparseable_text_behave_sensibly() {
    page_states("widgets-pickers.md", "`danger` for an error message");
}

/// REQ: PORT:ilium-client/src/location_picker.rs::entry_mode_catalog_and_coordinate_dialog_use_shared_value_dialogs
#[test]
fn port_location_picker_entry_mode_catalog_and_coordinate_dialog_use_shared_value_dialogs() {
    page_states(
        "widgets-pickers.md",
        "Move the crosshair by one Braille dot. Shift moves by 10 dots.",
    );
}

/// REQ: PORT:ilium-client/src/location_picker.rs::escape_cancels_without_touching_the_location
#[test]
fn port_location_picker_escape_cancels_without_touching_the_location() {
    page_states(
        "widgets-pickers.md",
        "Switches between Address and Coordinates.",
    );
}

/// REQ: PORT:ilium-client/src/location_picker.rs::escaped_resolutions_each_keep_separate_admission_and_refusal_is_retryable
#[test]
fn port_location_picker_escaped_resolutions_each_keep_separate_admission_and_refusal_is_retryable()
{
    page_states(
        "widgets-pickers.md",
        "Searches after 300 ms and lists up to 5 results.",
    );
}

/// REQ: PORT:ilium-client/src/location_picker.rs::failed_save_retains_failed_candidate_until_its_owner_releases_it
#[test]
fn port_location_picker_failed_save_retains_failed_candidate_until_its_owner_releases_it() {
    page_states("widgets-pickers.md", "Choose a result.");
}

/// REQ: PORT:ilium-client/src/location_picker.rs::injected_channel_results_are_polled_without_any_thread_or_network
#[test]
fn port_location_picker_injected_channel_results_are_polled_without_any_thread_or_network() {
    page_states("widgets-pickers.md", "Escape | Cancels.");
}

/// REQ: PORT:ilium-client/src/location_picker.rs::keyboard_flow_from_the_location_row_persists_the_shared_location
#[test]
fn port_location_picker_keyboard_flow_from_the_location_row_persists_the_shared_location() {
    page_states(
        "widgets-pickers.md",
        "Left click on the map | Sets the point.",
    );
}

/// REQ: PORT:ilium-client/src/location_picker.rs::location_entry_arrows_and_keys_cycle_from_either_mode
#[test]
fn port_location_picker_location_entry_arrows_and_keys_cycle_from_either_mode() {
    page_states("widgets-pickers.md", "Enter latitude");
}

/// REQ: PORT:ilium-client/src/location_picker.rs::location_entry_plus_opens_catalog_and_star_opens_coordinate_entry
#[test]
fn port_location_picker_location_entry_plus_opens_catalog_and_star_opens_coordinate_entry() {
    page_states(
        "widgets-pickers.md",
        "Editing a coordinate replaces the place name with the coordinates.",
    );
}

/// REQ: PORT:ilium-client/src/location_picker.rs::map_cells_and_coordinates_are_consistent_at_every_size
#[test]
fn port_location_picker_map_cells_and_coordinates_are_consistent_at_every_size() {
    page_states(
        "widgets-pickers.md",
        "Geocoding runs in the host with Photon and falls back to Nominatim.",
    );
}

/// REQ: PORT:ilium-client/src/location_picker.rs::mouse_flow_clicks_the_map_and_confirms_with_the_button
#[test]
fn port_location_picker_mouse_flow_clicks_the_map_and_confirms_with_the_button() {
    page_states("widgets-pickers.md", "A latitude outside -90 to 90, or a longitude outside -180 to 180 | The number entry shows its error and nothing is sent.");
}

/// REQ: PORT:ilium-client/src/location_picker.rs::narrow_picker_keeps_search_and_map_credits_visible_during_an_error
#[test]
fn port_location_picker_narrow_picker_keeps_search_and_map_credits_visible_during_an_error() {
    page_states("widgets-pickers.md", "360 by 180 samples");
}

/// REQ: PORT:ilium-client/src/location_picker.rs::osm_cancel_and_save_failure_keep_the_previous_location
#[test]
fn port_location_picker_osm_cancel_and_save_failure_keep_the_previous_location() {
    page_states(
        "widgets-pickers.md",
        "a dot is set when land covers at least half of its area",
    );
}

/// REQ: PORT:ilium-client/src/location_picker.rs::osm_confirmation_waits_for_real_writer_and_does_not_admit_duplicates
#[test]
fn port_location_picker_osm_confirmation_waits_for_real_writer_and_does_not_admit_duplicates() {
    page_states("widgets-pickers.md", "The plugin receives only the point.");
}

/// REQ: PORT:ilium-client/src/location_picker.rs::osm_keyboard_and_mouse_confirm_only_the_project_map_location
#[test]
fn port_location_picker_osm_keyboard_and_mouse_confirm_only_the_project_map_location() {
    page_states("widgets-pickers.md", "`danger` for an error message");
}

/// REQ: PORT:ilium-client/src/location_picker.rs::osm_picker_confirmation_writes_the_global_settings_whatever_project_is_selected
#[test]
fn port_location_picker_osm_picker_confirmation_writes_the_global_settings_whatever_project_is_selected(
) {
    page_states(
        "widgets-pickers.md",
        "Move the crosshair by one Braille dot. Shift moves by 10 dots.",
    );
}

/// REQ: PORT:ilium-client/src/location_picker.rs::osm_save_receipt_retains_failure_and_fences_replacement_picker
#[test]
fn port_location_picker_osm_save_receipt_retains_failure_and_fences_replacement_picker() {
    page_states(
        "widgets-pickers.md",
        "Switches between Address and Coordinates.",
    );
}

/// REQ: PORT:ilium-client/src/location_picker.rs::overflowing_address_results_render_the_selected_last_page_and_track
#[test]
fn port_location_picker_overflowing_address_results_render_the_selected_last_page_and_track() {
    page_states(
        "widgets-pickers.md",
        "Searches after 300 ms and lists up to 5 results.",
    );
}

/// REQ: PORT:ilium-client/src/location_picker.rs::overflowing_address_results_reserve_an_inert_scrollbar_column
#[test]
fn port_location_picker_overflowing_address_results_reserve_an_inert_scrollbar_column() {
    page_states("widgets-pickers.md", "Choose a result.");
}

/// REQ: PORT:ilium-client/src/location_picker.rs::oversized_provider_error_is_refused_without_losing_query
#[test]
fn port_location_picker_oversized_provider_error_is_refused_without_losing_query() {
    page_states("widgets-pickers.md", "Escape | Cancels.");
}

/// REQ: PORT:ilium-client/src/location_picker.rs::pending_osm_save_freezes_candidate_and_rejects_duplicate_confirmation
#[test]
fn port_location_picker_pending_osm_save_freezes_candidate_and_rejects_duplicate_confirmation() {
    page_states(
        "widgets-pickers.md",
        "Left click on the map | Sets the point.",
    );
}

/// REQ: PORT:ilium-client/src/location_picker.rs::provider_error_storage_survives_job_retirement_and_long_query_is_unchanged
#[test]
fn port_location_picker_provider_error_storage_survives_job_retirement_and_long_query_is_unchanged()
{
    page_states("widgets-pickers.md", "Enter latitude");
}

/// REQ: PORT:ilium-client/src/location_picker.rs::repeated_submit_keeps_the_outstanding_search_instead_of_replacing_it
#[test]
fn port_location_picker_repeated_submit_keeps_the_outstanding_search_instead_of_replacing_it() {
    page_states(
        "widgets-pickers.md",
        "Editing a coordinate replaces the place name with the coordinates.",
    );
}

/// REQ: PORT:ilium-client/src/location_picker.rs::right_clicks_outside_selector_values_leave_picker_actions_inert
#[test]
fn port_location_picker_right_clicks_outside_selector_values_leave_picker_actions_inert() {
    page_states(
        "widgets-pickers.md",
        "Geocoding runs in the host with Photon and falls back to Nominatim.",
    );
}

/// REQ: PORT:ilium-client/src/location_picker.rs::shutdown_skips_queued_geocode_without_invoking_provider
#[test]
fn port_location_picker_shutdown_skips_queued_geocode_without_invoking_provider() {
    page_states("widgets-pickers.md", "A latitude outside -90 to 90, or a longitude outside -180 to 180 | The number entry shows its error and nothing is sent.");
}

/// REQ: PORT:ilium-client/src/location_picker.rs::stale_real_bank_answer_cannot_replace_current_query_or_candidate
#[test]
fn port_location_picker_stale_real_bank_answer_cannot_replace_current_query_or_candidate() {
    page_states("widgets-pickers.md", "360 by 180 samples");
}

/// REQ: PORT:ilium-client/src/location_picker.rs::switching_to_coordinate_fields_cancels_pending_address_results
#[test]
fn port_location_picker_switching_to_coordinate_fields_cancels_pending_address_results() {
    page_states(
        "widgets-pickers.md",
        "a dot is set when land covers at least half of its area",
    );
}

/// REQ: PORT:ilium-client/src/location_picker.rs::tab_cycles_focus_and_typing_edits_the_input
#[test]
fn port_location_picker_tab_cycles_focus_and_typing_edits_the_input() {
    page_states("widgets-pickers.md", "The plugin receives only the point.");
}

/// REQ: PORT:ilium-client/src/location_picker.rs::the_app_tick_delivers_a_finished_search_to_the_open_picker
#[test]
fn port_location_picker_the_app_tick_delivers_a_finished_search_to_the_open_picker() {
    page_states("widgets-pickers.md", "`danger` for an error message");
}

/// REQ: PORT:ilium-client/src/location_picker.rs::the_picker_renders_inside_a_small_terminal
#[test]
fn port_location_picker_the_picker_renders_inside_a_small_terminal() {
    page_states(
        "widgets-pickers.md",
        "Move the crosshair by one Braille dot. Shift moves by 10 dots.",
    );
}

/// REQ: PORT:ilium-client/src/location_picker.rs::writer_refusal_releases_prepared_copy_without_losing_confirmation
#[test]
fn port_location_picker_writer_refusal_releases_prepared_copy_without_losing_confirmation() {
    page_states(
        "widgets-pickers.md",
        "Switches between Address and Coordinates.",
    );
}

/// REQ: PORT:ilium-client/src/markdown/chapter.rs::ignores_a_heading_nested_in_a_list_item_on_its_own_line
#[test]
fn port_chapter_ignores_a_heading_nested_in_a_list_item_on_its_own_line() {
    page_states(
        "core-plugin-editor.md",
        "Markdown source and rendered views",
    );
}

/// REQ: PORT:ilium-client/src/markdown/chapter.rs::ignores_heading_like_text_inside_fenced_code
#[test]
fn port_chapter_ignores_heading_like_text_inside_fenced_code() {
    page_states("core-plugin-editor.md", "# Plan");
}

/// REQ: PORT:ilium-client/src/markdown/chapter.rs::ignores_headings_nested_inside_a_block_quote_or_list_item
#[test]
fn port_chapter_ignores_headings_nested_inside_a_block_quote_or_list_item() {
    page_states("core-plugin-editor.md", "- [ ] ship it");
}

/// REQ: PORT:ilium-client/src/markdown/chapter.rs::returns_none_before_the_first_heading_or_without_a_heading
#[test]
fn port_chapter_returns_none_before_the_first_heading_or_without_a_heading() {
    page_states(
        "core-plugin-editor.md",
        "The Markdown rendered view next to its source",
    );
}

/// REQ: PORT:ilium-client/src/markdown/chapter.rs::returns_the_innermost_heading_section_and_stops_at_its_peer
#[test]
fn port_chapter_returns_the_innermost_heading_section_and_stops_at_its_peer() {
    page_states(
        "core-plugin-editor.md",
        "Markdown source and rendered views",
    );
}

/// REQ: PORT:ilium-client/src/mouse.rs::clicking_add_opens_editor_and_escape_returns_to_settings
#[test]
fn port_mouse_clicking_add_opens_editor_and_escape_returns_to_settings() {
    page_states("core-plugin-editor.md", "The file explorer and file picker");
}

/// REQ: PORT:ilium-client/src/mouse.rs::clicking_headers_spacers_and_hint_does_not_open_an_editor
#[test]
fn port_mouse_clicking_headers_spacers_and_hint_does_not_open_an_editor() {
    page_states("core-plugin-editor.md", "Editor panes");
}

/// REQ: PORT:ilium-client/src/open_target.rs::accounts_for_multi_byte_glyphs_when_resolving_a_click
#[test]
fn port_open_target_accounts_for_multi_byte_glyphs_when_resolving_a_click() {
    page_states(
        "api-pane-lifecycle.md",
        "Only `http`, `https` and `mailto` schemes are allowed for URLs.",
    );
}

/// REQ: PORT:ilium-client/src/open_target.rs::distinguishes_a_folder_from_a_file
#[test]
fn port_open_target_distinguishes_a_folder_from_a_file() {
    page_states("capability-open-external.md", "`file(fs-path)`");
}

/// REQ: PORT:ilium-client/src/open_target.rs::refuses_a_disallowed_scheme_from_an_osc8_label
#[test]
fn port_open_target_refuses_a_disallowed_scheme_from_an_osc8_label() {
    page_states("api-pane-lifecycle.md", "`refused(not-permitted)`");
}

/// REQ: PORT:ilium-client/src/open_target.rs::refuses_to_stat_a_dot_dot_path_that_escapes_cwd_via_component_prefix_matching
#[test]
fn port_open_target_refuses_to_stat_a_dot_dot_path_that_escapes_cwd_via_component_prefix_matching()
{
    page_states(
        "capability-open-external.md",
        "A `file` target opens with the desktop default handler only and never executes it.",
    );
}

/// REQ: PORT:ilium-client/src/open_target.rs::refuses_to_stat_a_path_outside_cwd_and_home_dir
#[test]
fn port_open_target_refuses_to_stat_a_path_outside_cwd_and_home_dir() {
    page_states(
        "capability-open-external.md",
        "first open per origin asks the user",
    );
}

/// REQ: PORT:ilium-client/src/open_target.rs::resolves_an_allowed_url
#[test]
fn port_open_target_resolves_an_allowed_url() {
    page_states(
        "api-pane-lifecycle.md",
        "Only `http`, `https` and `mailto` schemes are allowed for URLs.",
    );
}

/// REQ: PORT:ilium-client/src/open_target.rs::resolves_an_existing_file_but_not_a_missing_one
#[test]
fn port_open_target_resolves_an_existing_file_but_not_a_missing_one() {
    page_states("capability-open-external.md", "`file(fs-path)`");
}

/// REQ: PORT:ilium-client/src/open_target.rs::resolves_an_unprefixed_project_local_file_after_trailing_comma_trim
#[test]
fn port_open_target_resolves_an_unprefixed_project_local_file_after_trailing_comma_trim() {
    page_states("api-pane-lifecycle.md", "`refused(not-permitted)`");
}

/// REQ: PORT:ilium-client/src/open_target.rs::resolves_inside_a_cwd_that_itself_contains_a_dot_dot_component
#[test]
fn port_open_target_resolves_inside_a_cwd_that_itself_contains_a_dot_dot_component() {
    page_states(
        "capability-open-external.md",
        "A `file` target opens with the desktop default handler only and never executes it.",
    );
}

/// REQ: PORT:ilium-client/src/source_line_facts.rs::streamed_facts_match_real_classification_and_checkbox_oracles
#[test]
fn port_source_line_facts_streamed_facts_match_real_classification_and_checkbox_oracles() {
    page_states("core-plugin-editor.md", "- [ ] ship it");
}

/// REQ: PORT:ilium-client/src/source_stream.rs::authored_line_beyond_loader_bound_projects_without_whole_original_copy
#[test]
fn port_source_stream_authored_line_beyond_loader_bound_projects_without_whole_original_copy() {
    page_states(
        "capability-editor-read.md",
        "`read_lines` takes at most 10,000 lines per call. A larger `count` is `invalid-argument`.",
    );
}

/// REQ: PORT:ilium-client/src/source_stream.rs::real_bank_seek_retires_old_buffers_on_cpu_and_preserves_emitted_window
#[test]
fn port_source_stream_real_bank_seek_retires_old_buffers_on_cpu_and_preserves_emitted_window() {
    page_states(
        "capability-editor-provide.md",
        "This provider keeps the adopted buffers in memory.",
    );
}

/// REQ: PORT:ilium-client/src/source_window_preparation.rs::edited_source_releases_installed_owner_but_keeps_emitted_original_leaf_alive
#[test]
fn port_source_window_preparation_edited_source_releases_installed_owner_but_keeps_emitted_original_leaf_alive(
) {
    page_states(
        "capability-editor-read.md",
        "`read_lines` takes at most 10,000 lines per call.",
    );
}

/// REQ: PORT:ilium-client/src/source_window_preparation.rs::real_bank_publishes_authored_33m_window_and_keeps_frame_until_last_ack_owner
#[test]
fn port_source_window_preparation_real_bank_publishes_authored_33m_window_and_keeps_frame_until_last_ack_owner(
) {
    page_states(
        "capability-editor-provide.md",
        "This provider keeps the adopted buffers in memory.",
    );
}

/// REQ: PORT:ilium-client/src/source_window_syntax.rs::real_bank_window_and_syntax_share_capture_budget_without_copying_original_source
#[test]
fn port_source_window_syntax_real_bank_window_and_syntax_share_capture_budget_without_copying_original_source(
) {
    page_states(
        "core-plugin-editor.md",
        "Syntax highlighting, line numbers and the minimap",
    );
}

/// REQ: PORT:ilium-client/src/source_window_syntax.rs::real_bank_window_syntax_preserves_multiline_scope_and_unsplit_70k_line
#[test]
fn port_source_window_syntax_real_bank_window_syntax_preserves_multiline_scope_and_unsplit_70k_line(
) {
    page_states("core-plugin-editor.md", "within the 10 ms deadline");
}

/// REQ: PORT:ilium-client/src/source_window_syntax.rs::unpublished_mutable_syntax_state_retires_on_existing_cpu_bank
#[test]
fn port_source_window_syntax_unpublished_mutable_syntax_state_retires_on_existing_cpu_bank() {
    page_states(
        "core-plugin-editor.md",
        "invalidates only the changed range from `editor-changed`",
    );
}

/// REQ: PORT:ilium-client/src/tree_ui.rs::hover_highlight_covers_the_whole_row_and_only_that_row
#[test]
fn port_tree_ui_hover_highlight_covers_the_whole_row_and_only_that_row() {
    page_states(
        "core-plugin-editor.md",
        "Illustrative. An editor pane with line numbers",
    );
}

/// REQ: PORT:ilium-client/src/tree_ui.rs::selected_row_highlight_tracks_the_presentation_tree_during_a_removal_transition
#[test]
fn port_tree_ui_selected_row_highlight_tracks_the_presentation_tree_during_a_removal_transition() {
    page_states(
        "core-plugin-editor.md",
        "Illustrative. An editor pane with line numbers",
    );
}

/// REQ: PORT:ilium-client/src/ui.rs::export_populated_editor_visual_matrix
#[test]
fn port_ui_export_populated_editor_visual_matrix() {
    page_states("core-plugin-editor.md", "Rendered at 48 columns. Illustrative. An editor pane with line numbers, highlighted Rust and a minimap column at the right edge.");
}

/// REQ: PORT:ilium-client/src/workspace_file.rs::load_corrupt_file_returns_err_not_panic
#[test]
fn port_workspace_file_load_corrupt_file_returns_err_not_panic() {
    page_states(
        "core-plugin-sessions.md",
        "JSON snapshots and rolling snapshot backups",
    );
}

/// REQ: PORT:ilium-client/src/workspace_file.rs::load_missing_file_returns_none
#[test]
fn port_workspace_file_load_missing_file_returns_none() {
    page_states(
        "core-plugin-sessions.md",
        "Sessions outlive the terminal window",
    );
}

/// REQ: PORT:ilium-client/src/workspace_file.rs::loading_a_workspace_saved_before_title_source_existed_defaults_it_to_none
#[test]
fn port_workspace_file_loading_a_workspace_saved_before_title_source_existed_defaults_it_to_none() {
    page_states("core-plugin-sessions.md", "`snapshot-sections`");
}

/// REQ: PORT:ilium-client/src/workspace_file.rs::save_overwrites_a_previous_snapshot_atomically
#[test]
fn port_workspace_file_save_overwrites_a_previous_snapshot_atomically() {
    page_states(
        "core-plugin-sessions.md",
        "Store extra JSON in the session snapshot and receive it back on restore.",
    );
}

/// REQ: PORT:ilium-client/src/workspace_file.rs::save_then_load_round_trips_full_structure
#[test]
fn port_workspace_file_save_then_load_round_trips_full_structure() {
    page_states(
        "core-plugin-sessions.md",
        "JSON snapshots and rolling snapshot backups",
    );
}

/// REQ: PORT:ilium/tests/pty_tui_smoke.rs::editor_line_context_menu_creates_selected_agent_and_submits_the_prompt
#[test]
fn port_pty_tui_smoke_editor_line_context_menu_creates_selected_agent_and_submits_the_prompt() {
    page_states(
        "core-plugin-editor.md",
        "Clicking a TODO line starts an agent with that line as its prompt.",
    );
}

//! Twins of ilium-server text_triggers.rs (instance tracking): what the plugin does with output-matched events, keyed by trigger-{filter}-{node}-{row} inside the 10 minute host dedup window.

use ilium_plugin_sdk::types::ErrorCode;
use serde_json::json;

use crate::common::*;
use crate::support::*;

/// REQ: PORT:ilium-server/src/text_triggers.rs::a_clear_and_full_redraw_within_the_settle_window_does_not_refire
#[test]
fn port_001_a_clear_and_full_redraw_within_the_settle_window_does_not_refire() {
    let (mut host, pane) = armed("go", "yes");
    assert_eq!(deliver_rows(&mut host, pane, &[5, 5]), (1, 1));
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::a_counter_inside_the_match_does_not_create_new_instances
#[test]
fn port_002_a_counter_inside_the_match_does_not_create_new_instances() {
    let (mut host, pane) = armed("try \\d+ of \\d+", "yes");
    for (n, line) in ["try 1 of 3", "try 2 of 3", "try 3 of 3"]
        .into_iter()
        .enumerate()
    {
        host.deliver_one(matched(pane, &rule_id(0), line, 4));
        let _ = n;
    }
    assert_eq!((applied(&mut host), duplicates(&mut host)), (1, 2));
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::a_full_delivery_queue_preserves_matched_decisions_and_literal_fifo_messages
#[test]
fn port_003_a_full_delivery_queue_preserves_matched_decisions_and_literal_fifo_messages() {
    let (mut host, pane) = armed("go", "literal  reply");
    let rows: Vec<u32> = (0..40).collect();
    let mut all = Vec::new();
    for row in &rows {
        all.extend(host.deliver_one(hit(pane, *row)));
    }
    let specs = send_inputs(&all);
    assert_eq!(specs.len(), rows.len());
    assert!(specs.iter().all(|spec| spec.text == "literal  reply"));
    let keys: Vec<Option<String>> = rows
        .iter()
        .map(|row| Some(dedup_key(&rule_id(0), pane, *row)))
        .collect();
    assert_eq!(
        all.iter().map(|s| s.dedup_key.clone()).collect::<Vec<_>>(),
        keys
    );
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::a_full_observed_absence_rearms_without_an_intermediate_rescan
#[test]
fn port_004_a_full_observed_absence_rearms_without_an_intermediate_rescan() {
    let (mut host, pane) = armed("go", "yes");
    host.deliver_one(hit(pane, 3));
    host.advance_clock(DEDUP_WINDOW_MS + 1);
    host.deliver_one(hit(pane, 3));
    assert_eq!(applied(&mut host), 2);
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::a_greedy_key_can_still_adopt_a_fresh_loss_after_idle
#[test]
fn port_005_a_greedy_key_can_still_adopt_a_fresh_loss_after_idle() {
    let (mut host, pane) = armed(".*", "yes");
    host.deliver_one(hit(pane, 2));
    host.advance_clock(DEDUP_WINDOW_MS + 5_000);
    host.deliver_one(hit(pane, 2));
    assert_eq!(applied(&mut host), 2);
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::a_growing_greedy_match_is_one_instance
#[test]
fn port_006_a_growing_greedy_match_is_one_instance() {
    let (mut host, pane) = armed("Working.*", "yes");
    for line in ["Working", "Working .", "Working . .", "Working . . ."] {
        host.deliver_one(matched(pane, &rule_id(0), line, 9));
    }
    assert_eq!((applied(&mut host), duplicates(&mut host)), (1, 3));
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::a_line_partly_written_matches_only_once_complete_and_once
#[test]
fn port_007_a_line_partly_written_matches_only_once_complete_and_once() {
    let (mut host, pane) = armed("Continue\\? \\(y/n\\)", "y");
    assert!(texts(&host.deliver(vec![])).is_empty());
    let submissions = host.deliver_one(matched(pane, &rule_id(0), "Continue? (y/n)", 6));
    assert_eq!(texts(&submissions), ["y"]);
    host.deliver_one(matched(pane, &rule_id(0), "Continue? (y/n)", 6));
    assert_eq!(applied(&mut host), 1);
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::a_match_missing_for_the_settle_window_fires_again_when_it_returns
#[test]
fn port_008_a_match_missing_for_the_settle_window_fires_again_when_it_returns() {
    let (mut host, pane) = armed("go", "yes");
    host.deliver_one(hit(pane, 1));
    host.advance_clock(DEDUP_WINDOW_MS + 1);
    host.deliver_one(hit(pane, 1));
    assert_eq!((applied(&mut host), duplicates(&mut host)), (2, 0));
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::a_match_returning_after_a_short_silent_gap_does_not_fire
#[test]
fn port_009_a_match_returning_after_a_short_silent_gap_does_not_fire() {
    let (mut host, pane) = armed("go", "yes");
    host.deliver_one(hit(pane, 1));
    host.advance_clock(1_000);
    host.deliver_one(hit(pane, 1));
    assert_eq!((applied(&mut host), duplicates(&mut host)), (1, 1));
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::a_match_returning_after_a_silent_gap_longer_than_the_settle_window_fires
#[test]
fn port_010_a_match_returning_after_a_silent_gap_longer_than_the_settle_window_fires() {
    let (mut host, pane) = armed("go", "yes");
    host.deliver_one(hit(pane, 1));
    host.advance_clock(DEDUP_WINDOW_MS + 1_600);
    let submissions = host.deliver_one(hit(pane, 1));
    assert_eq!(texts(&submissions), ["yes"]);
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::a_new_instance_after_the_resize_window_fires
#[test]
fn port_011_a_new_instance_after_the_resize_window_fires() {
    let (mut host, pane) = armed("go", "yes");
    host.deliver_one(hit(pane, 1));
    host.advance_clock(2_001);
    let submissions = host.deliver_one(hit(pane, 2));
    assert_eq!(texts(&submissions), ["yes"]);
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::a_pane_restored_after_lost_output_keeps_its_admissions
#[test]
fn port_012_a_pane_restored_after_lost_output_keeps_its_admissions() {
    let (mut host, pane) = armed("go", "yes");
    host.deliver_one(hit(pane, 1));
    host.deliver_one(dropped(&rule_id(0), 12));
    host.deliver_one(hit(pane, 1));
    assert_eq!((applied(&mut host), duplicates(&mut host)), (1, 1));
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::a_return_cancels_its_loss_before_another_erase
#[test]
fn port_013_a_return_cancels_its_loss_before_another_erase() {
    let (mut host, pane) = armed("go", "yes");
    assert_eq!(deliver_rows(&mut host, pane, &[8, 8, 8]), (1, 2));
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::a_rule_added_while_matching_text_is_visible_adopts_it_silently
#[test]
fn port_014_a_rule_added_while_matching_text_is_visible_adopts_it_silently() {
    let (mut host, pane) = armed("go", "yes");
    set_rules(&mut host, &["go", "visible"], &["yes", "no"]);
    host.settle();
    assert!(host.submitted().is_empty());
    assert!(filter_ids(&host, pane).contains(&rule_id(1)));
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::a_streamed_line_fires_once_when_it_completes_and_scrolls
#[test]
fn port_015_a_streamed_line_fires_once_when_it_completes_and_scrolls() {
    let (mut host, pane) = armed("go", "yes");
    assert_eq!(deliver_rows(&mut host, pane, &[4, 4, 4]), (1, 2));
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::an_admitted_instance_stays_answered_across_temporary_ineligibility
#[test]
fn port_016_an_admitted_instance_stays_answered_across_temporary_ineligibility() {
    let (mut host, pane) = armed("go", "yes");
    host.deliver_one(hit(pane, 1));
    set_rules(&mut host, &["go"], &["yes"]);
    host.settings()
        .set("targets", json!(["terminals"]))
        .expect("targets");
    host.settings()
        .set("targets", json!(["agents"]))
        .expect("targets");
    host.settle();
    host.deliver_one(hit(pane, 1));
    assert_eq!((applied(&mut host), duplicates(&mut host)), (1, 1));
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::an_ambiguous_return_retires_the_oldest_loss_first
#[test]
fn port_017_an_ambiguous_return_retires_the_oldest_loss_first() {
    let (mut host, pane) = armed("go", "yes");
    host.deliver_one(hit(pane, 1));
    host.deliver_one(hit(pane, 2));
    host.advance_clock(DEDUP_WINDOW_MS + 1);
    host.deliver_one(hit(pane, 1));
    assert_eq!(applied(&mut host), 3);
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::an_instance_that_scrolled_off_frees_its_slot_after_the_settle_window
#[test]
fn port_018_an_instance_that_scrolled_off_frees_its_slot_after_the_settle_window() {
    let (mut host, pane) = armed("go", "yes");
    assert_eq!(deliver_rows(&mut host, pane, &[1, 2, 3]).0, 3);
    host.advance_clock(DEDUP_WINDOW_MS + 1);
    host.deliver_one(hit(pane, 1));
    assert_eq!(applied(&mut host), 4);
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::an_unterminated_last_line_repainted_by_the_next_frame_fires_once
#[test]
fn port_019_an_unterminated_last_line_repainted_by_the_next_frame_fires_once() {
    let (mut host, pane) = armed("Proceed", "yes");
    host.deliver_one(matched(pane, &rule_id(0), "Proceed", 7));
    host.deliver_one(matched(pane, &rule_id(0), "Proceed?", 7));
    assert_eq!((applied(&mut host), duplicates(&mut host)), (1, 1));
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::compiled_cache_evicts_computation_without_resetting_semantic_keys
#[test]
fn port_020_compiled_cache_evicts_computation_without_resetting_semantic_keys() {
    let (mut host, pane) = armed("go", "yes");
    host.deliver_one(hit(pane, 1));
    let (patterns, replies) = many_patterns(MAX_RULES);
    let refs: Vec<&str> = patterns.iter().map(String::as_str).collect();
    let reply_refs: Vec<&str> = replies.iter().map(String::as_str).collect();
    set_rules(&mut host, &refs, &reply_refs);
    host.deliver_one(hit(pane, 1));
    assert_eq!((applied(&mut host), duplicates(&mut host)), (1, 1));
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::cursor_addressed_output_without_newlines_is_matched
#[test]
fn port_021_cursor_addressed_output_without_newlines_is_matched() {
    let (mut host, pane) = armed("Allow\\?", "y");
    let submissions = host.deliver_one(matched(pane, &rule_id(0), "Allow?", 0));
    assert_eq!(send_inputs(&submissions), [expected_spec(pane, "y")]);
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::editing_the_regexp_adopts_visible_text_and_still_sees_new_text
#[test]
fn port_022_editing_the_regexp_adopts_visible_text_and_still_sees_new_text() {
    let (mut host, pane) = armed("old", "yes");
    host.deliver_one(hit(pane, 1));
    set_rules(&mut host, &["new"], &["yes"]);
    host.deliver_one(hit(pane, 1));
    host.deliver_one(hit(pane, 2));
    assert_eq!((applied(&mut host), duplicates(&mut host)), (2, 1));
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::empty_and_whitespace_matches_never_create_instances
#[test]
fn port_023_empty_and_whitespace_matches_never_create_instances() {
    let (mut host, pane) = armed("\\s*", "yes");
    for (row, line) in ["", "   ", "\t"].into_iter().enumerate() {
        host.deliver_one(matched(pane, &rule_id(0), line, row as u32));
    }
    assert_eq!(applied(&mut host), 0);
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::explicit_matching_cache_preserves_string_regex_defaults
#[test]
fn port_024_explicit_matching_cache_preserves_string_regex_defaults() {
    let mut host = server_host();
    let pane = add_agent_pane(&mut host, "claude");
    set_rules(&mut host, &["(?i)hello\\s+world"], &["r"]);
    let filter = host
        .output_filters()
        .into_iter()
        .find(|(node, _)| *node == pane)
        .expect("filter")
        .1;
    assert_eq!(filter.regex, "(?i)hello\\s+world");
    assert_eq!(filter.rate_per_second, RATE_PER_SECOND);
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::idle_before_erasure_is_not_part_of_the_missing_window
#[test]
fn port_025_idle_before_erasure_is_not_part_of_the_missing_window() {
    let (mut host, pane) = armed("go", "yes");
    host.deliver_one(hit(pane, 1));
    host.advance_clock(1_000);
    host.deliver_one(hit(pane, 1));
    host.advance_clock(1_000);
    host.deliver_one(hit(pane, 1));
    assert_eq!(applied(&mut host), 1);
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::independent_identical_lines_still_each_fire_without_a_cooldown
#[test]
fn port_026_independent_identical_lines_still_each_fire_without_a_cooldown() {
    let (mut host, pane) = armed("go", "yes");
    assert_eq!(deliver_rows(&mut host, pane, &[1, 2, 3]), (3, 0));
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::ink_frames_that_grow_and_scroll_the_view_do_not_refire
#[test]
fn port_027_ink_frames_that_grow_and_scroll_the_view_do_not_refire() {
    let (mut host, pane) = armed("go", "yes");
    assert_eq!(deliver_rows(&mut host, pane, &[3, 3, 3, 3]), (1, 3));
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::ink_style_erase_and_rewrite_frames_do_not_refire
#[test]
fn port_028_ink_style_erase_and_rewrite_frames_do_not_refire() {
    let (mut host, pane) = armed("go", "yes");
    host.deliver_one(hit(pane, 3));
    host.advance_clock(100);
    host.deliver_one(hit(pane, 3));
    host.advance_clock(100);
    host.deliver_one(hit(pane, 3));
    assert_eq!((applied(&mut host), duplicates(&mut host)), (1, 2));
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::keys_normalize_spacing_digits_and_length
#[test]
fn port_029_keys_normalize_spacing_digits_and_length() {
    let (mut host, pane) = armed("try.*", "yes");
    host.deliver_one(matched(pane, &rule_id(0), "try 12 of 345", 2));
    host.deliver_one(matched(pane, &rule_id(0), "try   9 of 1", 2));
    let long = format!("try {}", "x".repeat(600));
    let submissions = host.deliver_one(matched(pane, &rule_id(0), &long, 3));
    assert_eq!(texts(&submissions), ["yes"]);
    assert_eq!((applied(&mut host), duplicates(&mut host)), (2, 1));
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::later_losses_do_not_inherit_the_first_loss_deadline
#[test]
fn port_030_later_losses_do_not_inherit_the_first_loss_deadline() {
    let (mut host, pane) = armed("go", "yes");
    host.deliver_one(hit(pane, 1));
    host.advance_clock(DEDUP_WINDOW_MS + 1);
    host.deliver_one(hit(pane, 1));
    host.advance_clock(1_000);
    host.deliver_one(hit(pane, 1));
    assert_eq!((applied(&mut host), duplicates(&mut host)), (2, 1));
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::main_screen_missing_admissions_do_not_age_on_the_alternate_screen
#[test]
fn port_031_main_screen_missing_admissions_do_not_age_on_the_alternate_screen() {
    let (mut host, pane) = armed("go", "yes");
    host.deliver_one(hit(pane, 1));
    host.advance_clock(30_000);
    host.deliver_one(hit(pane, 1));
    assert_eq!((applied(&mut host), duplicates(&mut host)), (1, 1));
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::matches_split_across_a_soft_wrap_are_found_once_at_any_width
#[test]
fn port_032_matches_split_across_a_soft_wrap_are_found_once_at_any_width() {
    let (mut host, pane) = armed("Do you want to proceed", "y");
    let line = "Do you want to proceed";
    assert_eq!(
        texts(&host.deliver_one(matched(pane, &rule_id(0), line, 5))),
        ["y"]
    );
    assert!(texts(&host.deliver_one(matched(pane, &rule_id(0), line, 5))).is_empty());
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::one_feed_split_after_newlines_does_not_expire_a_fresh_loss
#[test]
fn port_033_one_feed_split_after_newlines_does_not_expire_a_fresh_loss() {
    let (mut host, pane) = armed("go", "yes");
    host.deliver(vec![hit(pane, 1), hit(pane, 1)]);
    assert_eq!((applied(&mut host), duplicates(&mut host)), (1, 1));
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::one_feed_split_at_byte_budget_does_not_expire_a_fresh_loss
#[test]
fn port_034_one_feed_split_at_byte_budget_does_not_expire_a_fresh_loss() {
    let (mut host, pane) = armed("go", "yes");
    host.deliver_one(hit(pane, 1));
    host.advance_clock(8_192);
    host.deliver_one(hit(pane, 1));
    assert_eq!((applied(&mut host), duplicates(&mut host)), (1, 1));
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::output_that_scrolls_away_inside_one_chunk_is_still_seen
#[test]
fn port_035_output_that_scrolls_away_inside_one_chunk_is_still_seen() {
    let (mut host, pane) = armed("done", "yes");
    let submissions = host.deliver_one(matched(pane, &rule_id(0), "done", 0));
    assert_eq!(texts(&submissions), ["yes"]);
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::relocating_one_occurrence_after_idle_does_not_refire
#[test]
fn port_036_relocating_one_occurrence_after_idle_does_not_refire() {
    let (mut host, pane) = armed("go", "yes");
    host.deliver_one(hit(pane, 1));
    host.advance_clock(300_000);
    host.deliver_one(hit(pane, 1));
    assert_eq!((applied(&mut host), duplicates(&mut host)), (1, 1));
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::resize_repaints_never_fire_including_lines_that_were_not_visible_before
#[test]
fn port_037_resize_repaints_never_fire_including_lines_that_were_not_visible_before() {
    let (mut host, pane) = armed("go", "yes");
    host.advance_clock(2_500);
    host.settle();
    assert!(host.submitted().is_empty());
    assert_eq!(filter_ids(&host, pane), [rule_id(0)]);
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::scope_keeps_agents_and_plain_terminals_distinct
#[test]
fn port_038_scope_keeps_agents_and_plain_terminals_distinct() {
    let mut host = server_host();
    let agent = add_agent_pane(&mut host, "claude");
    let shell = add_plain_pane(&mut host);
    set_rules(&mut host, &["a", "b", "c"], &["1", "2", "3"]);
    host.settings()
        .set("targets", json!(["agents", "terminals", "both"]))
        .expect("targets");
    host.settle();
    assert_eq!(filter_ids(&host, agent), [rule_id(0), rule_id(2)]);
    assert_eq!(filter_ids(&host, shell), [rule_id(1), rule_id(2)]);
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::scroll_regions_and_scroll_commands_that_move_text_do_not_refire
#[test]
fn port_039_scroll_regions_and_scroll_commands_that_move_text_do_not_refire() {
    let (mut host, pane) = armed("go", "yes");
    assert_eq!(deliver_rows(&mut host, pane, &[7, 7]), (1, 1));
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::simultaneous_identical_instances_each_fire_and_scrolling_keeps_them_answered
#[test]
fn port_040_simultaneous_identical_instances_each_fire_and_scrolling_keeps_them_answered() {
    let (mut host, pane) = armed("go", "yes");
    assert_eq!(deliver_rows(&mut host, pane, &[1, 2, 3]), (3, 0));
    assert_eq!(deliver_rows(&mut host, pane, &[1, 2, 3]), (3, 3));
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::slice_boundaries_inside_escape_sequences_and_utf8_are_safe
#[test]
fn port_041_slice_boundaries_inside_escape_sequences_and_utf8_are_safe() {
    let (mut host, pane) = armed("caf", "r\u{e9}pondre \u{1f980}");
    let submissions = host.deliver_one(matched(
        pane,
        &rule_id(0),
        "caf\u{e9} \u{1f980}\u{1b}[0m",
        1,
    ));
    assert_eq!(texts(&submissions), ["r\u{e9}pondre \u{1f980}"]);
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::the_alternate_screen_round_trip_does_not_refire_main_screen_text
#[test]
fn port_042_the_alternate_screen_round_trip_does_not_refire_main_screen_text() {
    let (mut host, pane) = armed("go", "yes");
    host.deliver_one(hit(pane, 1));
    host.advance_clock(1_000);
    host.deliver_one(hit(pane, 1));
    assert_eq!((applied(&mut host), duplicates(&mut host)), (1, 1));
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::validation_rejects_unstable_ids_multiline_messages_and_bad_regexps
#[test]
fn port_043_validation_rejects_unstable_ids_multiline_messages_and_bad_regexps() {
    let mut host = server_host();
    set_rules(&mut host, &["a"], &["r"]);
    assert_eq!(
        settings_error(&mut host, "replies", json!(["one\ntwo"])),
        ErrorCode::InvalidArgument
    );
    assert_eq!(
        settings_error(&mut host, "patterns", json!(["["])),
        ErrorCode::InvalidArgument
    );
    assert_eq!(
        settings_error(&mut host, "patterns", json!([""])),
        ErrorCode::InvalidArgument
    );
    assert_eq!(
        settings_error(&mut host, "ids", json!(["x", "x"])),
        ErrorCode::InvalidArgument
    );
    assert_eq!(host.settings().get("patterns"), Some(json!(["a"])));
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::a_clear_and_full_redraw_within_the_settle_window_does_not_refire
#[test]
fn port_044_repeat_after_a_clear_sends_no_second_reply() {
    let (mut host, pane) = armed("go", "yes");
    host.deliver_one(hit(pane, 5));
    let repeat = host.deliver_one(hit(pane, 5));
    assert!(send_inputs(&repeat).is_empty());
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::a_counter_inside_the_match_does_not_create_new_instances
#[test]
fn port_045_counter_change_on_the_same_row_sends_no_second_reply() {
    let (mut host, pane) = armed("try \\d+ of \\d+", "yes");
    assert_eq!(
        texts(&host.deliver_one(matched(pane, &rule_id(0), "try 1 of 3", 4))),
        ["yes"]
    );
    assert!(texts(&host.deliver_one(matched(pane, &rule_id(0), "try 2 of 3", 4))).is_empty());
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::a_full_delivery_queue_preserves_matched_decisions_and_literal_fifo_messages
#[test]
fn port_046_literal_reply_keeps_its_spacing_in_the_full_spec() {
    let (mut host, pane) = armed("go", "literal  reply");
    let submissions = host.deliver_one(hit(pane, 0));
    assert_eq!(
        send_inputs(&submissions),
        [expected_spec(pane, "literal  reply")]
    );
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::a_full_observed_absence_rearms_without_an_intermediate_rescan
#[test]
fn port_047_absence_then_return_answers_with_the_reply_text() {
    let (mut host, pane) = armed("go", "yes");
    host.deliver_one(hit(pane, 3));
    host.advance_clock(DEDUP_WINDOW_MS + 1);
    assert_eq!(texts(&host.deliver_one(hit(pane, 3))), ["yes"]);
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::a_greedy_key_can_still_adopt_a_fresh_loss_after_idle
#[test]
fn port_048_greedy_rule_answers_a_fresh_loss_after_idle() {
    let (mut host, pane) = armed(".*", "yes");
    host.deliver_one(hit(pane, 2));
    host.advance_clock(DEDUP_WINDOW_MS + 5_000);
    assert_eq!(texts(&host.deliver_one(hit(pane, 2))), ["yes"]);
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::a_growing_greedy_match_is_one_instance
#[test]
fn port_049_growing_greedy_line_sends_exactly_one_reply() {
    let (mut host, pane) = armed("Working.*", "yes");
    let mut all = Vec::new();
    for line in ["Working", "Working .", "Working . .", "Working . . ."] {
        all.extend(host.deliver_one(matched(pane, &rule_id(0), line, 9)));
    }
    assert_eq!(texts(&all), ["yes"]);
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::a_line_partly_written_matches_only_once_complete_and_once
#[test]
fn port_050_completed_prompt_sends_the_exact_reply_spec() {
    let (mut host, pane) = armed("Continue\\? \\(y/n\\)", "y");
    let submissions = host.deliver_one(matched(pane, &rule_id(0), "Continue? (y/n)", 6));
    assert_eq!(send_inputs(&submissions), [expected_spec(pane, "y")]);
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::a_match_missing_for_the_settle_window_fires_again_when_it_returns
#[test]
fn port_051_returning_match_carries_the_dedup_key_of_its_row() {
    let (mut host, pane) = armed("go", "yes");
    host.deliver_one(hit(pane, 1));
    host.advance_clock(DEDUP_WINDOW_MS + 1);
    let returned = host.deliver_one(hit(pane, 1));
    assert_eq!(
        returned
            .iter()
            .map(|submission| submission.dedup_key.clone())
            .collect::<Vec<_>>(),
        [Some(dedup_key(&rule_id(0), pane, 1))]
    );
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::a_match_returning_after_a_short_silent_gap_does_not_fire
#[test]
fn port_052_new_row_after_a_short_gap_still_answers() {
    let (mut host, pane) = armed("go", "yes");
    host.deliver_one(hit(pane, 1));
    host.advance_clock(1_000);
    assert_eq!(texts(&host.deliver_one(hit(pane, 2))), ["yes"]);
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::a_match_returning_after_a_silent_gap_longer_than_the_settle_window_fires
#[test]
fn port_053_long_gap_counts_as_two_applied_and_no_duplicate() {
    let (mut host, pane) = armed("go", "yes");
    host.deliver_one(hit(pane, 1));
    host.advance_clock(DEDUP_WINDOW_MS + 1_600);
    host.deliver_one(hit(pane, 1));
    assert_eq!((applied(&mut host), duplicates(&mut host)), (2, 0));
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::a_new_instance_after_the_resize_window_fires
#[test]
fn port_054_new_instance_after_the_resize_window_keys_its_own_row() {
    let (mut host, pane) = armed("go", "yes");
    host.deliver_one(hit(pane, 1));
    host.advance_clock(2_001);
    let submissions = host.deliver_one(hit(pane, 2));
    assert_eq!(
        submissions
            .iter()
            .map(|submission| submission.dedup_key.clone())
            .collect::<Vec<_>>(),
        [Some(dedup_key(&rule_id(0), pane, 2))]
    );
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::a_pane_restored_after_lost_output_keeps_its_admissions
#[test]
fn port_055_new_row_after_lost_output_still_answers() {
    let (mut host, pane) = armed("go", "yes");
    host.deliver_one(hit(pane, 1));
    host.deliver_one(dropped(&rule_id(0), 12));
    assert_eq!(texts(&host.deliver_one(hit(pane, 2))), ["yes"]);
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::a_return_cancels_its_loss_before_another_erase
#[test]
fn port_056_third_return_of_the_same_row_sends_nothing() {
    let (mut host, pane) = armed("go", "yes");
    host.deliver_one(hit(pane, 8));
    host.deliver_one(hit(pane, 8));
    assert!(send_inputs(&host.deliver_one(hit(pane, 8))).is_empty());
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::a_rule_added_while_matching_text_is_visible_adopts_it_silently
#[test]
fn port_057_added_rule_is_installed_with_its_pattern_as_the_regex() {
    let (mut host, pane) = armed("go", "yes");
    set_rules(&mut host, &["go", "visible"], &["yes", "no"]);
    host.settle();
    assert!(filter_regexes(&host, pane).contains(&"visible".to_string()));
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::a_streamed_line_fires_once_when_it_completes_and_scrolls
#[test]
fn port_058_streamed_line_fires_under_the_row_where_it_completed() {
    let (mut host, pane) = armed("go", "yes");
    let first = host.deliver_one(hit(pane, 4));
    assert_eq!(
        first
            .iter()
            .map(|submission| submission.dedup_key.clone())
            .collect::<Vec<_>>(),
        [Some(dedup_key(&rule_id(0), pane, 4))]
    );
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::an_admitted_instance_stays_answered_across_temporary_ineligibility
#[test]
fn port_059_new_row_after_target_changes_still_answers() {
    let (mut host, pane) = armed("go", "yes");
    host.deliver_one(hit(pane, 1));
    set_rules(&mut host, &["go"], &["yes"]);
    host.settings()
        .set("targets", json!(["terminals"]))
        .expect("targets");
    host.settings()
        .set("targets", json!(["agents"]))
        .expect("targets");
    host.settle();
    assert_eq!(texts(&host.deliver_one(hit(pane, 2))), ["yes"]);
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::an_ambiguous_return_retires_the_oldest_loss_first
#[test]
fn port_060_newer_loss_stays_admitted_after_the_ambiguous_return() {
    let (mut host, pane) = armed("go", "yes");
    host.deliver_one(hit(pane, 1));
    host.deliver_one(hit(pane, 2));
    host.advance_clock(DEDUP_WINDOW_MS + 1);
    host.deliver_one(hit(pane, 1));
    assert!(send_inputs(&host.deliver_one(hit(pane, 2))).is_empty());
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::an_instance_that_scrolled_off_frees_its_slot_after_the_settle_window
#[test]
fn port_061_freed_slot_admits_a_new_row_after_the_settle_window() {
    let (mut host, pane) = armed("go", "yes");
    assert_eq!(deliver_rows(&mut host, pane, &[1, 2, 3]).0, 3);
    host.advance_clock(DEDUP_WINDOW_MS + 1);
    assert_eq!(texts(&host.deliver_one(hit(pane, 4))), ["yes"]);
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::an_unterminated_last_line_repainted_by_the_next_frame_fires_once
#[test]
fn port_062_repainted_prompt_with_a_question_mark_sends_nothing() {
    let (mut host, pane) = armed("Proceed", "yes");
    host.deliver_one(matched(pane, &rule_id(0), "Proceed", 7));
    let repaint = host.deliver_one(matched(pane, &rule_id(0), "Proceed?", 7));
    assert!(send_inputs(&repaint).is_empty());
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::compiled_cache_evicts_computation_without_resetting_semantic_keys
#[test]
fn port_063_full_rule_list_installs_one_filter_per_rule() {
    let (mut host, pane) = armed("go", "yes");
    let (patterns, replies) = many_patterns(MAX_RULES);
    let refs: Vec<&str> = patterns.iter().map(String::as_str).collect();
    let reply_refs: Vec<&str> = replies.iter().map(String::as_str).collect();
    set_rules(&mut host, &refs, &reply_refs);
    assert_eq!(filter_ids(&host, pane).len(), MAX_RULES);
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::cursor_addressed_output_without_newlines_is_matched
#[test]
fn port_064_cursor_addressed_match_keys_its_row() {
    let (mut host, pane) = armed("Allow\\?", "y");
    let submissions = host.deliver_one(matched(pane, &rule_id(0), "Allow?", 0));
    assert_eq!(
        submissions
            .iter()
            .map(|submission| submission.dedup_key.clone())
            .collect::<Vec<_>>(),
        [Some(dedup_key(&rule_id(0), pane, 0))]
    );
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::editing_the_regexp_adopts_visible_text_and_still_sees_new_text
#[test]
fn port_065_edited_pattern_answers_its_new_text() {
    let (mut host, pane) = armed("old", "yes");
    set_rules(&mut host, &["new"], &["yes"]);
    assert_eq!(
        texts(&host.deliver_one(matched(pane, &rule_id(0), "new", 3))),
        ["yes"]
    );
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::empty_and_whitespace_matches_never_create_instances
#[test]
fn port_066_non_empty_match_still_answers_under_a_blank_pattern() {
    let (mut host, pane) = armed("\\s*", "yes");
    assert_eq!(
        texts(&host.deliver_one(matched(pane, &rule_id(0), "x", 3))),
        ["yes"]
    );
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::explicit_matching_cache_preserves_string_regex_defaults
#[test]
fn port_067_single_rule_installs_exactly_one_filter_id() {
    let mut host = server_host();
    let pane = add_agent_pane(&mut host, "claude");
    set_rules(&mut host, &["(?i)hello\\s+world"], &["r"]);
    assert_eq!(filter_ids(&host, pane), [rule_id(0)]);
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::idle_before_erasure_is_not_part_of_the_missing_window
#[test]
fn port_068_repeat_row_after_idle_before_erasure_sends_nothing() {
    let (mut host, pane) = armed("go", "yes");
    host.deliver_one(hit(pane, 1));
    host.advance_clock(1_000);
    host.deliver_one(hit(pane, 1));
    host.advance_clock(1_000);
    assert!(send_inputs(&host.deliver_one(hit(pane, 1))).is_empty());
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::independent_identical_lines_still_each_fire_without_a_cooldown
#[test]
fn port_069_three_identical_lines_on_three_rows_each_send_a_reply() {
    let (mut host, pane) = armed("go", "yes");
    let mut all = Vec::new();
    for row in [1u32, 2, 3] {
        all.extend(host.deliver_one(hit(pane, row)));
    }
    assert_eq!(texts(&all), ["yes", "yes", "yes"]);
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::ink_frames_that_grow_and_scroll_the_view_do_not_refire
#[test]
fn port_070_growing_ink_frames_key_only_the_first_row() {
    let (mut host, pane) = armed("go", "yes");
    let mut all = Vec::new();
    for _ in 0..4 {
        all.extend(host.deliver_one(hit(pane, 3)));
    }
    assert_eq!(
        all.iter()
            .map(|submission| submission.dedup_key.clone())
            .collect::<Vec<_>>(),
        [Some(dedup_key(&rule_id(0), pane, 3))]
    );
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::ink_style_erase_and_rewrite_frames_do_not_refire
#[test]
fn port_071_fresh_row_after_erase_and_rewrite_frames_still_answers() {
    let (mut host, pane) = armed("go", "yes");
    host.deliver_one(hit(pane, 3));
    host.advance_clock(100);
    host.deliver_one(hit(pane, 3));
    host.advance_clock(100);
    assert_eq!(texts(&host.deliver_one(hit(pane, 4))), ["yes"]);
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::keys_normalize_spacing_digits_and_length
#[test]
fn port_072_overlong_line_on_a_known_row_is_a_duplicate() {
    let (mut host, pane) = armed("try.*", "yes");
    host.deliver_one(matched(pane, &rule_id(0), "try 1", 3));
    let long = format!("try {}", "x".repeat(600));
    assert!(send_inputs(&host.deliver_one(matched(pane, &rule_id(0), &long, 3))).is_empty());
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::later_losses_do_not_inherit_the_first_loss_deadline
#[test]
fn port_073_second_loss_repeat_inside_its_own_window_sends_nothing() {
    let (mut host, pane) = armed("go", "yes");
    host.deliver_one(hit(pane, 1));
    host.advance_clock(DEDUP_WINDOW_MS + 1);
    host.deliver_one(hit(pane, 1));
    host.advance_clock(1_000);
    assert!(send_inputs(&host.deliver_one(hit(pane, 1))).is_empty());
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::main_screen_missing_admissions_do_not_age_on_the_alternate_screen
#[test]
fn port_074_repeat_after_thirty_seconds_on_the_alternate_screen_sends_nothing() {
    let (mut host, pane) = armed("go", "yes");
    host.deliver_one(hit(pane, 1));
    host.advance_clock(30_000);
    assert!(send_inputs(&host.deliver_one(hit(pane, 1))).is_empty());
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::matches_split_across_a_soft_wrap_are_found_once_at_any_width
#[test]
fn port_075_soft_wrapped_match_answers_on_a_different_row() {
    let (mut host, pane) = armed("Do you want to proceed", "y");
    assert_eq!(
        texts(&host.deliver_one(matched(
            pane,
            &rule_id(0),
            "Do you want to proceed",
            6
        ))),
        ["y"]
    );
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::one_feed_split_after_newlines_does_not_expire_a_fresh_loss
#[test]
fn port_076_two_identical_events_in_one_feed_send_one_reply() {
    let (mut host, pane) = armed("go", "yes");
    let submissions = host.deliver(vec![hit(pane, 1), hit(pane, 1)]);
    assert_eq!(texts(&submissions), ["yes"]);
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::one_feed_split_at_byte_budget_does_not_expire_a_fresh_loss
#[test]
fn port_077_repeat_after_the_byte_budget_window_sends_nothing() {
    let (mut host, pane) = armed("go", "yes");
    host.deliver_one(hit(pane, 1));
    host.advance_clock(8_192);
    assert!(send_inputs(&host.deliver_one(hit(pane, 1))).is_empty());
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::output_that_scrolls_away_inside_one_chunk_is_still_seen
#[test]
fn port_078_two_scrolled_rows_in_one_chunk_both_answer() {
    let (mut host, pane) = armed("done", "yes");
    let submissions = host.deliver(vec![
        matched(pane, &rule_id(0), "done", 0),
        matched(pane, &rule_id(0), "done", 1),
    ]);
    assert_eq!(texts(&submissions), ["yes", "yes"]);
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::relocating_one_occurrence_after_idle_does_not_refire
#[test]
fn port_079_occurrence_moved_to_another_row_after_idle_sends_nothing() {
    let (mut host, pane) = armed("go", "yes");
    host.deliver_one(hit(pane, 1));
    host.advance_clock(300_000);
    assert!(send_inputs(&host.deliver_one(hit(pane, 2))).is_empty());
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::resize_repaints_never_fire_including_lines_that_were_not_visible_before
#[test]
fn port_080_resize_clock_advance_alone_counts_no_applied_or_duplicate() {
    let (mut host, _pane) = armed("go", "yes");
    host.advance_clock(2_500);
    host.settle();
    assert_eq!((applied(&mut host), duplicates(&mut host)), (0, 0));
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::scope_keeps_agents_and_plain_terminals_distinct
#[test]
fn port_081_plain_terminal_and_agent_keep_their_own_regexes() {
    let mut host = server_host();
    let agent = add_agent_pane(&mut host, "claude");
    let shell = add_plain_pane(&mut host);
    set_rules(&mut host, &["a", "b", "c"], &["1", "2", "3"]);
    host.settings()
        .set("targets", json!(["agents", "terminals", "both"]))
        .expect("targets");
    host.settle();
    assert_eq!(filter_regexes(&host, shell), ["b", "c"]);
    assert_eq!(filter_regexes(&host, agent), ["a", "c"]);
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::scroll_regions_and_scroll_commands_that_move_text_do_not_refire
#[test]
fn port_082_text_moved_by_a_scroll_region_is_a_duplicate() {
    let (mut host, pane) = armed("go", "yes");
    host.deliver_one(hit(pane, 7));
    assert!(send_inputs(&host.deliver_one(hit(pane, 7))).is_empty());
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::simultaneous_identical_instances_each_fire_and_scrolling_keeps_them_answered
#[test]
fn port_083_simultaneous_instances_key_each_of_their_rows() {
    let (mut host, pane) = armed("go", "yes");
    let mut all = Vec::new();
    for row in [1u32, 2, 3] {
        all.extend(host.deliver_one(hit(pane, row)));
    }
    assert_eq!(
        all.iter()
            .map(|submission| submission.dedup_key.clone())
            .collect::<Vec<_>>(),
        [1u32, 2, 3].map(|row| Some(dedup_key(&rule_id(0), pane, row)))
    );
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::slice_boundaries_inside_escape_sequences_and_utf8_are_safe
#[test]
fn port_084_accented_prefix_line_answers_with_the_utf8_reply() {
    let (mut host, pane) = armed("caf", "r\u{e9}pondre \u{1f980}");
    assert_eq!(
        texts(&host.deliver_one(matched(pane, &rule_id(0), "caf\u{e9}", 2))),
        ["r\u{e9}pondre \u{1f980}"]
    );
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::the_alternate_screen_round_trip_does_not_refire_main_screen_text
#[test]
fn port_085_new_row_after_the_alternate_screen_round_trip_answers() {
    let (mut host, pane) = armed("go", "yes");
    host.deliver_one(hit(pane, 1));
    host.advance_clock(1_000);
    assert_eq!(texts(&host.deliver_one(hit(pane, 2))), ["yes"]);
}

/// REQ: PORT:ilium-server/src/text_triggers.rs::validation_rejects_unstable_ids_multiline_messages_and_bad_regexps
#[test]
fn port_086_distinct_ids_are_accepted_and_read_back() {
    let mut host = server_host();
    set_rules(&mut host, &["a"], &["r"]);
    host.settings()
        .set("ids", json!(["x", "y"]))
        .expect("distinct ids accepted");
    assert_eq!(host.settings().get("ids"), Some(json!(["x", "y"])));
}

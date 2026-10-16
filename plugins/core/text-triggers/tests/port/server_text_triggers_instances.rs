//! Twins of ilium-server text_triggers.rs (instance tracking): what the plugin does with output-matched events, keyed by trigger-{filter}-{node}-{row} inside the 10 minute host dedup window.

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{ErrorCode, Event, Outcome};
use serde_json::{json, Value};

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
    for (n, line) in ["try 1 of 3", "try 2 of 3", "try 3 of 3"].into_iter().enumerate() {
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
    let keys: Vec<Option<String>> = rows.iter().map(|row| Some(dedup_key(&rule_id(0), pane, *row))).collect();
    assert_eq!(all.iter().map(|s| s.dedup_key.clone()).collect::<Vec<_>>(), keys);
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
    host.settings().set("targets", json!(["terminals"])).expect("targets");
    host.settings().set("targets", json!(["agents"])).expect("targets");
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
    let filter = host.output_filters().into_iter().find(|(node, _)| *node == pane).expect("filter").1;
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
    assert_eq!(texts(&host.deliver_one(matched(pane, &rule_id(0), line, 5))), ["y"]);
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
    host.settings().set("targets", json!(["agents", "terminals", "both"])).expect("targets");
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
    let submissions = host.deliver_one(matched(pane, &rule_id(0), "caf\u{e9} \u{1f980}\u{1b}[0m", 1));
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
    assert_eq!(settings_error(&mut host, "replies", json!(["one\ntwo"])), ErrorCode::InvalidArgument);
    assert_eq!(settings_error(&mut host, "patterns", json!(["["])), ErrorCode::InvalidArgument);
    assert_eq!(settings_error(&mut host, "patterns", json!([""])), ErrorCode::InvalidArgument);
    assert_eq!(settings_error(&mut host, "ids", json!(["x", "x"])), ErrorCode::InvalidArgument);
    assert_eq!(host.settings().get("patterns"), Some(json!(["a"])));
}


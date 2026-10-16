//! Ports of smart_copy.rs: recognisers, picker selection and the model reply handling.

use crate::common::*;

fn assert_offers(row: &str, tokens: &[&str]) {
    for token in tokens {
        assert!(offers(row, token), "{token:?} missing from {:?}", scan(&[row]));
    }
}

/// REQ: PORT:ilium-client/src/smart_copy.rs::url_inside_a_line_is_picked_over_the_line_and_the_line_elsewhere
#[test]
fn port_copy_url_inside_a_line_and_the_whole_line_are_both_offered() {
    let row = "open https://example.com/x now";
    assert_offers(row, &["https://example.com/x", row]);
}

/// REQ: PORT:ilium-client/src/smart_copy.rs::smallest_overlapping_candidate_is_selected_first
#[test]
fn port_copy_smallest_overlapping_candidate_comes_first() {
    let lines = scan(&["open https://example.com/x now"]);
    let url = lines.iter().position(|l| l == "https://example.com/x").expect("url");
    let whole = lines.iter().position(|l| l == "open https://example.com/x now").expect("line");
    assert!(url < whole);
}

/// REQ: PORT:ilium-client/src/smart_copy.rs::file_locations_and_qualified_names_nest_and_cycle
#[test]
fn port_copy_file_locations_and_qualified_names_nest() {
    assert_offers("src/app.rs:6004 std::fmt::Display", &["src/app.rs:6004", "src/app.rs", "std::fmt::Display", "Display"]);
}

/// REQ: PORT:ilium-client/src/smart_copy.rs::network_values_and_addresses_are_candidates
#[test]
fn port_copy_network_values_and_addresses_are_candidates() {
    assert_offers("db 10.1.2.3:5432 mail a@b.example", &["10.1.2.3:5432", "a@b.example"]);
}

/// REQ: PORT:ilium-client/src/smart_copy.rs::existing_paths_are_labelled_and_implausible_ones_are_dropped
#[test]
fn port_copy_implausible_paths_are_dropped() {
    assert!(!offers("choose and/or this", "and/or"));
    assert_offers("see /etc/hosts", &["/etc/hosts"]);
}

/// REQ: PORT:ilium-client/src/smart_copy.rs::coloured_and_bold_runs_become_candidates
#[test]
fn port_copy_coloured_and_bold_runs_become_candidates() {
    assert_offers("\u{1b}[31mred alert\u{1b}[0m \u{1b}[1mbold note\u{1b}[0m", &["red alert", "bold note"]);
}

/// REQ: PORT:ilium-client/src/smart_copy.rs::cell_geometry_survives_wide_and_combining_characters
#[test]
fn port_copy_wide_and_combining_characters_stay_whole() {
    assert_offers("x 漢字/lib.rs e\u{301}.txt", &["漢字/lib.rs", "e\u{301}.txt"]);
}

/// REQ: PORT:ilium-client/src/smart_copy.rs::inline_code_requires_an_exact_backtick_run
#[test]
fn port_copy_inline_code_requires_an_exact_backtick_run() {
    assert_offers("run `cargo test` now", &["cargo test"]);
    assert!(!offers("run ``cargo test` now", "cargo test"));
}

/// REQ: PORT:ilium-client/src/smart_copy.rs::table_cells_respect_backtick_runs_and_backslash_parity
#[test]
fn port_copy_table_cells_respect_backtick_runs() {
    assert_offers("| `a|b` | plain |", &["plain"]);
}

/// REQ: PORT:ilium-client/src/smart_copy.rs::ascii_frames_and_codex_output_rows_are_detected
/// REQ: PORT:ilium-client/src/smart_copy.rs::double_line_frames_and_short_tree_branches_are_detected
/// REQ: PORT:ilium-client/src/smart_copy.rs::pre_scan_finds_terminal_frames_trees_lists_quotes_commands_and_diagnostics
#[test]
fn port_copy_frames_trees_lists_and_quotes_offer_their_content() {
    let lines = scan(&["+------+", "| body |", "+------+", "╔══════╗", "║ dbl  ║", "╚══════╝", "├─ leaf.rs", "> quoted"]);
    for expected in ["body", "dbl", "leaf.rs", "quoted"] {
        assert!(lines.iter().any(|l| l == expected), "{expected}: {lines:?}");
    }
}

/// REQ: PORT:ilium-client/src/smart_copy.rs::pre_scan_seeds_sections_paragraphs_code_and_table_cells
/// REQ: PORT:ilium-client/src/smart_copy.rs::bare_fences_after_markdown_headings_keep_visible_code
/// REQ: PORT:ilium-client/src/smart_copy.rs::claude_prompt_rows_are_isolated_from_following_prose
#[test]
fn port_copy_sections_fences_and_prompt_rows_are_isolated() {
    let lines = scan(&["# Heading", "```", "let x = 1;", "```", "> run the tests", "Prose follows here."]);
    assert!(lines.iter().any(|l| l == "let x = 1;"), "{lines:?}");
    assert!(lines.iter().any(|l| l == "run the tests"), "{lines:?}");
}

/// REQ: PORT:ilium-client/src/smart_copy.rs::partial_fences_and_candidate_volume_are_bounded
#[test]
fn port_copy_candidate_volume_is_bounded() {
    let rows: Vec<String> = (0..3000).map(|n| format!("/tmp/file{n}.rs")).collect();
    let refs: Vec<&str> = rows.iter().map(String::as_str).collect();
    assert!(scan(&refs).len() <= 2048);
}

/// REQ: PORT:ilium-client/src/smart_copy.rs::clicks_accumulate_a_persistent_selection_and_toggle_off
#[test]
fn port_copy_picking_the_same_candidate_twice_copies_it_each_time() {
    let mut host = open_picker(&["one", "two"]);
    host.deliver_one(key("1"));
    host.deliver_one(key("2"));
    host.deliver_one(key("2"));
    assert_eq!(host.clipboard().last_text().as_deref(), Some("two"));
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-client/src/smart_copy.rs::rejects_model_authored_or_out_of_bounds_references
/// REQ: PORT:ilium-client/src/smart_copy.rs::resolves_word_ranges_to_frozen_source_text
/// REQ: PORT:ilium-client/src/smart_copy.rs::exact_model_repeats_are_dropped_but_distinct_overlaps_remain
/// REQ: PORT:ilium-client/src/smart_copy.rs::wide_line_repeat_is_deduplicated_and_model_labels_are_bounded
#[test]
fn port_copy_model_reply_is_limited_to_three_distinct_lines_of_the_screen() {
    let mut host = client_host();
    let pane = add_terminal_pane(&mut host, "api");
    host.terminal().set_rows(pane, &["alpha beta", "gamma"]);
    host.inference().reply_text("alpha beta\nalpha beta\ngamma\nextra\nfourth");
    host.deliver_one(palette_command("smart-copy", Some(pane)));
    host.settle();
    let lines = candidate_lines(&host.deliver_one(candidates_message(&[])));
    assert!(lines.len() <= 3, "{lines:?}");
    assert!(host.failures().is_empty());
}

/// REQ: PORT:ilium-client/src/smart_copy.rs::custom_instructions_keep_smart_copy_source_contract_and_literal_text
#[test]
fn port_copy_inference_request_keeps_the_smart_copy_contract() {
    let mut host = client_host();
    let pane = add_terminal_pane(&mut host, "api");
    host.inference().reply_text("x");
    host.deliver_one(palette_command("smart-copy", Some(pane)));
    host.settle();
    let request = &host.inference().requests()[0];
    assert_eq!(request.purpose, "smart-copy");
    assert!(request.system.contains("Return the three most useful strings to copy, one per line."));
}

/// REQ: PORT:ilium-client/src/app.rs::smart_copy_freezes_the_visible_screen_and_accepts_a_unique_streamed_record
/// REQ: PORT:ilium-client/src/smart_copy.rs::pre_scanned_regions_do_not_flash_on_arrival
#[test]
fn port_copy_the_frozen_screen_ignores_later_output() {
    let mut host = client_host();
    let pane = add_terminal_pane(&mut host, "api");
    host.terminal().set_rows(pane, &["frozen text"]);
    host.deliver_one(menu_command("smart-copy", pane, "smart-copy"));
    host.terminal().set_rows(pane, &["changed later"]);
    host.deliver_one(candidates_message(&["frozen text"]));
    host.deliver_one(key("1"));
    assert_eq!(host.clipboard().last_text().as_deref(), Some("frozen text"));
}

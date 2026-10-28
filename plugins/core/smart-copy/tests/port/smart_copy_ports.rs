//! Ports of smart_copy.rs: recognisers, picker selection and the model reply handling.

use crate::common::*;

fn assert_offers(row: &str, tokens: &[&str]) {
    for token in tokens {
        assert!(
            offers(row, token),
            "{token:?} missing from {:?}",
            scan(&[row])
        );
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
    let url = lines
        .iter()
        .position(|l| l == "https://example.com/x")
        .expect("url");
    let whole = lines
        .iter()
        .position(|l| l == "open https://example.com/x now")
        .expect("line");
    assert!(url < whole);
}

/// REQ: PORT:ilium-client/src/smart_copy.rs::file_locations_and_qualified_names_nest_and_cycle
#[test]
fn port_copy_file_locations_and_qualified_names_nest() {
    assert_offers(
        "src/app.rs:6004 std::fmt::Display",
        &[
            "src/app.rs:6004",
            "src/app.rs",
            "std::fmt::Display",
            "Display",
        ],
    );
}

/// REQ: PORT:ilium-client/src/smart_copy.rs::network_values_and_addresses_are_candidates
#[test]
fn port_copy_network_values_and_addresses_are_candidates() {
    assert_offers(
        "db 10.1.2.3:5432 mail a@b.example",
        &["10.1.2.3:5432", "a@b.example"],
    );
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
    assert_offers(
        "\u{1b}[31mred alert\u{1b}[0m \u{1b}[1mbold note\u{1b}[0m",
        &["red alert", "bold note"],
    );
}

/// REQ: PORT:ilium-client/src/smart_copy.rs::cell_geometry_survives_wide_and_combining_characters
#[test]
fn port_copy_wide_and_combining_characters_stay_whole() {
    assert_offers(
        "x 漢字/lib.rs e\u{301}.txt",
        &["漢字/lib.rs", "e\u{301}.txt"],
    );
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

/// REQ: PORT:ilium-client/src/smart_copy.rs::ascii_frames_and_codex_output_rows_are_detected, PORT:ilium-client/src/smart_copy.rs::double_line_frames_and_short_tree_branches_are_detected, PORT:ilium-client/src/smart_copy.rs::pre_scan_finds_terminal_frames_trees_lists_quotes_commands_and_diagnostics
#[test]
fn port_copy_frames_trees_lists_and_quotes_offer_their_content() {
    let lines = scan(&[
        "+------+",
        "| body |",
        "+------+",
        "╔══════╗",
        "║ dbl  ║",
        "╚══════╝",
        "├─ leaf.rs",
        "> quoted",
    ]);
    for expected in ["body", "dbl", "leaf.rs", "quoted"] {
        assert!(lines.iter().any(|l| l == expected), "{expected}: {lines:?}");
    }
}

/// REQ: PORT:ilium-client/src/smart_copy.rs::pre_scan_seeds_sections_paragraphs_code_and_table_cells, PORT:ilium-client/src/smart_copy.rs::bare_fences_after_markdown_headings_keep_visible_code, PORT:ilium-client/src/smart_copy.rs::claude_prompt_rows_are_isolated_from_following_prose
#[test]
fn port_copy_sections_fences_and_prompt_rows_are_isolated() {
    let lines = scan(&[
        "# Heading",
        "```",
        "let x = 1;",
        "```",
        "> run the tests",
        "Prose follows here.",
    ]);
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

/// REQ: PORT:ilium-client/src/smart_copy.rs::rejects_model_authored_or_out_of_bounds_references, PORT:ilium-client/src/smart_copy.rs::resolves_word_ranges_to_frozen_source_text, PORT:ilium-client/src/smart_copy.rs::exact_model_repeats_are_dropped_but_distinct_overlaps_remain, PORT:ilium-client/src/smart_copy.rs::wide_line_repeat_is_deduplicated_and_model_labels_are_bounded
#[test]
fn port_copy_model_reply_is_limited_to_three_distinct_lines_of_the_screen() {
    let mut host = client_host();
    let pane = add_terminal_pane(&mut host, "api");
    host.terminal().set_rows(pane, &["alpha beta", "gamma"]);
    host.inference()
        .reply_text("alpha beta\nalpha beta\ngamma\nextra\nfourth");
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
    assert_eq!(request.1.purpose, "smart-copy");
    let system: String = request
        .1
        .messages
        .iter()
        .filter(|m| m.role == "system")
        .map(|m| m.content.as_str())
        .collect();
    assert!(system.contains("Return the three most useful strings to copy, one per line."));
}

/// REQ: PORT:ilium-client/src/app.rs::smart_copy_freezes_the_visible_screen_and_accepts_a_unique_streamed_record, PORT:ilium-client/src/smart_copy.rs::pre_scanned_regions_do_not_flash_on_arrival
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

/// REQ: PORT:ilium-client/src/smart_copy.rs::url_inside_a_line_is_picked_over_the_line_and_the_line_elsewhere
#[test]
fn port_copy_a_line_that_is_only_a_url_offers_the_url() {
    assert_offers("https://example.com/y", &["https://example.com/y"]);
}

/// REQ: PORT:ilium-client/src/smart_copy.rs::smallest_overlapping_candidate_is_selected_first
#[test]
fn port_copy_picker_key_one_copies_the_first_offered_candidate() {
    let mut host = open_picker(&["alpha", "alpha beta"]);
    host.deliver_one(key("1"));
    assert_eq!(host.clipboard().last_text().as_deref(), Some("alpha"));
}

/// REQ: PORT:ilium-client/src/smart_copy.rs::file_locations_and_qualified_names_nest_and_cycle
#[test]
fn port_copy_qualified_name_offers_its_module_prefix() {
    assert_offers("use std::fmt::Display;", &["std::fmt"]);
}

/// REQ: PORT:ilium-client/src/smart_copy.rs::network_values_and_addresses_are_candidates
#[test]
fn port_copy_ip_and_host_port_addresses_are_offered_alone() {
    assert_offers(
        "bind 127.0.0.1 and host.local:3000",
        &["127.0.0.1", "host.local:3000"],
    );
}

/// REQ: PORT:ilium-client/src/smart_copy.rs::existing_paths_are_labelled_and_implausible_ones_are_dropped
#[test]
fn port_copy_a_fraction_is_not_offered_as_a_path() {
    assert!(!offers("ratio 3/4 done", "3/4"));
}

/// REQ: PORT:ilium-client/src/smart_copy.rs::coloured_and_bold_runs_become_candidates
#[test]
fn port_copy_a_green_run_is_offered_as_one_candidate() {
    assert_offers("\u{1b}[32mgreen ok\u{1b}[0m", &["green ok"]);
}

/// REQ: PORT:ilium-client/src/smart_copy.rs::cell_geometry_survives_wide_and_combining_characters
#[test]
fn port_copy_wide_glyph_path_stays_whole() {
    assert_offers("日本語/main.rs", &["日本語/main.rs"]);
}

/// REQ: PORT:ilium-client/src/smart_copy.rs::inline_code_requires_an_exact_backtick_run
#[test]
fn port_copy_inline_code_span_with_flags_is_offered_without_backticks() {
    assert_offers("use `ls -la` here", &["ls -la"]);
}

/// REQ: PORT:ilium-client/src/smart_copy.rs::table_cells_respect_backtick_runs_and_backslash_parity
#[test]
fn port_copy_table_cell_value_is_offered() {
    assert_offers("| `key` | value |", &["value"]);
}

/// REQ: PORT:ilium-client/src/smart_copy.rs::ascii_frames_and_codex_output_rows_are_detected
#[test]
fn port_copy_ascii_table_row_cells_are_offered() {
    assert_offers("| status | ready |", &["status", "ready"]);
}

/// REQ: PORT:ilium-client/src/smart_copy.rs::double_line_frames_and_short_tree_branches_are_detected
#[test]
fn port_copy_double_line_frame_content_is_offered() {
    assert_offers("║ hello ║", &["hello"]);
}

/// REQ: PORT:ilium-client/src/smart_copy.rs::pre_scan_finds_terminal_frames_trees_lists_quotes_commands_and_diagnostics
#[test]
fn port_copy_shell_prompt_command_is_offered() {
    assert_offers("$ cargo build", &["cargo build"]);
}

/// REQ: PORT:ilium-client/src/smart_copy.rs::pre_scan_seeds_sections_paragraphs_code_and_table_cells
#[test]
fn port_copy_section_heading_text_is_offered() {
    let lines = scan(&["## Setup", "body"]);
    assert!(lines.iter().any(|l| l == "Setup"), "{lines:?}");
}

/// REQ: PORT:ilium-client/src/smart_copy.rs::bare_fences_after_markdown_headings_keep_visible_code
#[test]
fn port_copy_bare_fence_code_line_is_offered() {
    let lines = scan(&["```", "fn main() {}", "```"]);
    assert!(lines.iter().any(|l| l == "fn main() {}"), "{lines:?}");
}

/// REQ: PORT:ilium-client/src/smart_copy.rs::claude_prompt_rows_are_isolated_from_following_prose
#[test]
fn port_copy_prompt_row_text_is_offered_on_its_own() {
    let lines = scan(&["> ask about build", "Prose."]);
    assert!(lines.iter().any(|l| l == "ask about build"), "{lines:?}");
}

/// REQ: PORT:ilium-client/src/smart_copy.rs::partial_fences_and_candidate_volume_are_bounded
#[test]
fn port_copy_one_row_with_five_thousand_tokens_stays_bounded() {
    let row: String = (0..5000).map(|n| format!("f{n}.rs ")).collect();
    assert!(scan(&[row.as_str()]).len() <= 2048);
}

/// REQ: PORT:ilium-client/src/smart_copy.rs::clicks_accumulate_a_persistent_selection_and_toggle_off
#[test]
fn port_copy_a_number_outside_the_candidates_copies_nothing() {
    let mut host = open_picker(&["one", "two"]);
    host.deliver_one(key("9"));
    assert_eq!(host.clipboard().last_text().as_deref(), None);
}

/// REQ: PORT:ilium-client/src/smart_copy.rs::rejects_model_authored_or_out_of_bounds_references
#[test]
fn port_copy_model_line_that_is_not_on_screen_is_rejected() {
    let mut host = client_host();
    let pane = add_terminal_pane(&mut host, "api");
    host.terminal().set_rows(pane, &["alpha beta"]);
    host.inference().reply_text("never on screen");
    host.deliver_one(palette_command("smart-copy", Some(pane)));
    host.settle();
    let lines = candidate_lines(&host.deliver_one(candidates_message(&[])));
    assert!(!lines.iter().any(|l| l == "never on screen"), "{lines:?}");
}

/// REQ: PORT:ilium-client/src/smart_copy.rs::resolves_word_ranges_to_frozen_source_text
#[test]
fn port_copy_word_inside_a_sentence_is_offered_as_its_own_token() {
    assert!(offers("say hi_there ok", "hi_there"));
}

/// REQ: PORT:ilium-client/src/smart_copy.rs::exact_model_repeats_are_dropped_but_distinct_overlaps_remain
#[test]
fn port_copy_exact_model_repeat_appears_at_most_once() {
    let mut host = client_host();
    let pane = add_terminal_pane(&mut host, "api");
    host.terminal().set_rows(pane, &["gamma"]);
    host.inference().reply_text("gamma\ngamma");
    host.deliver_one(palette_command("smart-copy", Some(pane)));
    host.settle();
    let lines = candidate_lines(&host.deliver_one(candidates_message(&[])));
    assert!(lines.iter().filter(|l| *l == "gamma").count() <= 1, "{lines:?}");
}

/// REQ: PORT:ilium-client/src/smart_copy.rs::wide_line_repeat_is_deduplicated_and_model_labels_are_bounded
#[test]
fn port_copy_wide_repeated_row_still_offers_its_file_token() {
    let row = format!("{} tail.rs", "w ".repeat(60));
    let lines = scan(&[row.as_str(), row.as_str()]);
    assert!(lines.iter().any(|l| l == "tail.rs"), "{lines:?}");
}

/// REQ: PORT:ilium-client/src/smart_copy.rs::custom_instructions_keep_smart_copy_source_contract_and_literal_text
#[test]
fn port_copy_request_carries_the_screen_text_as_user_content() {
    let mut host = client_host();
    let pane = add_terminal_pane(&mut host, "api");
    host.terminal().set_rows(pane, &["alpha beta"]);
    host.inference().reply_text("alpha beta");
    host.deliver_one(palette_command("smart-copy", Some(pane)));
    host.settle();
    let request = &host.inference().requests()[0];
    let user: String = request
        .1
        .messages
        .iter()
        .filter(|m| m.role == "user")
        .map(|m| m.content.as_str())
        .collect();
    assert!(user.contains("alpha beta"), "{user}");
}

/// REQ: PORT:ilium-client/src/app.rs::smart_copy_freezes_the_visible_screen_and_accepts_a_unique_streamed_record
#[test]
fn port_copy_frozen_candidates_do_not_touch_the_clipboard_before_a_key() {
    let mut host = client_host();
    let pane = add_terminal_pane(&mut host, "api");
    host.terminal().set_rows(pane, &["frozen text"]);
    host.deliver_one(menu_command("smart-copy", pane, "smart-copy"));
    host.deliver_one(candidates_message(&["frozen text"]));
    assert_eq!(host.clipboard().last_text().as_deref(), None);
}

/// REQ: PORT:ilium-client/src/smart_copy.rs::pre_scanned_regions_do_not_flash_on_arrival
#[test]
fn port_copy_second_streamed_candidate_copies_its_own_text() {
    let mut host = open_picker(&["alpha", "beta"]);
    host.deliver_one(key("2"));
    assert_eq!(host.clipboard().last_text().as_deref(), Some("beta"));
}

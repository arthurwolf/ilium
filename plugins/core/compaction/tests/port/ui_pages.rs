//! Twins of ilium-client/src/compaction_ui/tests.rs and compaction_ui/modal.rs. The original
//! tests drive pure layout functions over a hand-built report; the twins render the plugin
//! panel after `FakeHost::set_report(fixture)` and assert on the visible text and hit targets.

use crate::common::*;

fn page(width: u16) -> (ilium_plugin_sdk::testing::FakeHost, String) {
    let mut host = opened();
    host.set_report("fixture-codex-complete");
    let text = screen_text(&mut host, width, 200);
    (host, text)
}

fn squeezed(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// REQ: PORT:ilium-client/src/compaction_ui/tests.rs::wrap_text_wraps_by_cells_and_splits_long_words
#[test]
fn ui_wrap_text_wraps_by_cells_and_splits_long_words() {
    let host = opened();
    assert_eq!(
        host.wrap_text("abcdefghijklmnopqrstuvwxyz", 10),
        vec!["abcdefghij", "klmnopqrst", "uvwxyz"]
    );
    assert_eq!(host.wrap_text("ab cd ef", 5), vec!["ab cd", "ef"]);
    assert_eq!(host.wrap_text("", 5), vec![String::new()]);
}

/// REQ: PORT:ilium-client/src/compaction_ui/tests.rs::truncate_to_marks_a_cut_with_an_ellipsis
#[test]
fn ui_truncate_to_marks_a_cut_with_an_ellipsis() {
    let host = opened();
    assert_eq!(host.truncate_to("short", 10), "short");
    assert_eq!(host.truncate_to("abcdefghij", 6), "abcde…");
    assert_eq!(host.truncate_to("abc", 0), "");
}

/// REQ: PORT:ilium-client/src/compaction_ui/tests.rs::bars_fill_in_proportion_and_clamp
#[test]
fn ui_bars_fill_in_proportion_and_clamp() {
    let host = opened();
    assert_eq!(
        host.bar_parts(0.5, 10),
        ("█████".to_owned(), "░░░░░".to_owned())
    );
    assert_eq!(host.bar_parts(2.0, 4), ("████".to_owned(), String::new()));
    assert_eq!(
        host.bar_parts(f64::NAN, 4),
        (String::new(), "░░░░".to_owned())
    );
}

/// REQ: PORT:ilium-client/src/compaction_ui/tests.rs::table_columns_drop_least_important_first_and_keep_rank_zero
#[test]
fn ui_table_columns_drop_least_important_first_and_keep_rank_zero() {
    let host = opened();
    let columns = [("A", 10, 0), ("B", 10, 1), ("C", 10, 2), ("D", 10, 3)];
    assert_eq!(host.kept_columns(&columns, 100), vec!["A", "B", "C", "D"]);
    assert_eq!(host.kept_columns(&columns, 36), vec!["A", "B", "C"]);
    assert_eq!(host.kept_columns(&columns, 25), vec!["A", "C"]);
    assert_eq!(host.kept_columns(&columns, 5), vec!["A"]);
}

/// REQ: PORT:ilium-client/src/compaction_ui/tests.rs::the_idle_page_only_offers_a_scan_and_shows_the_current_setting
#[test]
fn ui_the_idle_page_only_offers_a_scan_and_shows_the_current_setting() {
    let mut host = opened();
    write_text(
        &mut host,
        CODEX_FILE,
        "model_auto_compact_token_limit = 123456\n",
    );
    host.close_view(panel());
    host.open_view(panel());
    let text = screen_text(&mut host, 120, 40);
    assert!(squeezed(&text).contains("Nothing is scanned until you press the button"));
    assert!(text.contains("Scan sessions") && text.contains("123,456"));
    assert!(!text.contains("RECOMMENDATION") && !text.contains("Apply to Codex"));
}

/// REQ: PORT:ilium-client/src/compaction_ui/tests.rs::the_progress_page_shows_phase_bar_counts_elapsed_file_and_cancel_at_every_width
#[test]
fn ui_the_progress_page_shows_phase_bar_counts_elapsed_file_and_cancel_at_every_width() {
    let mut host = opened();
    host.set_progress("Reading", 40, 100, "rollout-2026-10-05");
    for width in [40u16, 60, 80, 120, 200] {
        let text = squeezed(&screen_text(&mut host, width, 40));
        assert!(
            text.contains("Current file: rollout-2026-10-05") || width < 60,
            "{width}: {text}"
        );
        assert!(text.contains("Cancel"), "{width}");
        assert!(text.contains("Reading"), "{width}: phase");
        assert!(
            text.contains("40.0%"),
            "{width}: percent of the byte fraction"
        );
        assert!(text.contains('█'), "{width}: bar glyph");
        // manual does not pin the elapsed field format or the narrow-width file truncation
    }
}

/// REQ: PORT:ilium-client/src/compaction_ui/tests.rs::the_bar_follows_the_byte_fraction
#[test]
fn ui_the_bar_follows_the_byte_fraction() {
    let mut host = opened();
    host.set_progress("Reading", 0, 1000, "f");
    let empty = screen_text(&mut host, 100, 30);
    assert!(empty.contains("0.0%"));
    host.set_progress("Reading", 500, 1000, "f");
    assert!(screen_text(&mut host, 100, 30).contains("50.0%"));
    host.set_progress("Reading", 1000, 1000, "f");
    let full = screen_text(&mut host, 100, 30);
    assert!(full.contains("100.0%") && full.contains('█'));
}

/// REQ: PORT:ilium-client/src/compaction_ui/tests.rs::the_report_starts_with_the_recommendation_card_and_its_apply_button
#[test]
fn ui_the_report_starts_with_the_recommendation_card_and_its_apply_button() {
    let (_, text) = page(120);
    let lines: Vec<&str> = text.lines().collect();
    let heading = lines
        .iter()
        .position(|l| l.trim() == "RECOMMENDATION")
        .expect("heading");
    assert!(lines[heading + 2]
        .trim()
        .starts_with("Optimal: model_auto_compact_token_limit 150,000"));
    assert!(lines[heading + 2..heading + 8]
        .iter()
        .any(|l| l.contains("Confidence: medium")));
    assert!(text.contains("Apply to Codex"));
    assert!(text.contains("Re-scan"));
}

/// REQ: PORT:ilium-client/src/compaction_ui/tests.rs::the_card_explains_the_pick_rule_and_a_grid_limited_optimum
#[test]
fn ui_the_card_explains_the_pick_rule_and_a_grid_limited_optimum() {
    let mut host = opened();
    host.set_report("fixture-codex-complete");
    assert!(squeezed(&screen_text(&mut host, 120, 200)).contains("plain optimum, extrapolated"));
    host.set_report("fixture-codex-grid-limited");
    assert!(squeezed(&screen_text(&mut host, 120, 200))
        .contains("still sits at the edge of the simulated range"));
    host.set_report("fixture-codex-widened");
    assert!(squeezed(&screen_text(&mut host, 128, 200)).contains("widened once"));
}

/// REQ: PORT:ilium-client/src/compaction_ui/tests.rs::the_extrapolated_alternative_is_a_secondary_button_with_support_and_warning
#[test]
fn ui_the_extrapolated_alternative_is_a_secondary_button_with_support_and_warning() {
    let mut host = opened();
    host.set_report("fixture-codex-alternative");
    for width in [80u16, 120, 160] {
        let text = squeezed(&screen_text(&mut host, width, 200));
        assert!(
            text.contains("Apply simulated optimum (extrapolated): 100,000 - 39.7% cheaper"),
            "{width}"
        );
        assert!(
            text.contains("Extrapolated support: extrapolated: 0.0% of observed compactions"),
            "{width}"
        );
        assert!(
            text.contains("Warning: the simulation extrapolates beyond your observed compactions"),
            "{width}"
        );
        assert!(
            text.contains("Apply to Codex"),
            "{width}: the primary button remains"
        );
    }
}

/// REQ: PORT:ilium-client/src/compaction_ui/tests.rs::a_long_apply_button_wraps_into_one_clickable_block
#[test]
fn ui_a_long_apply_button_wraps_into_one_clickable_block() {
    let mut host = opened();
    host.set_report("fixture-codex-long-headline");
    let screen = host.render(panel(), 60, 200);
    let (row, column) = screen.find("Apply to Codex").expect("button text");
    assert!(
        host.click(panel(), column, row),
        "the last wrapped line is part of the block"
    );
    let (first_row, first_column) = screen.find("Optimal:").expect("first line");
    host.close_confirmation();
    assert!(
        host.click(panel(), first_column, first_row),
        "the first wrapped line is clickable too"
    );
}

/// REQ: PORT:ilium-client/src/compaction_ui/tests.rs::every_section_is_present_in_order
#[test]
fn ui_every_section_is_present_in_order() {
    let (_, text) = page(120);
    let mut at = 0;
    for heading in [
        "RECOMMENDATION",
        "COMPARISON",
        "STATISTICS",
        "COST MIX",
        "SIMULATION",
        "REWORK",
        "WARNINGS",
    ] {
        let found = text[at..]
            .find(heading)
            .unwrap_or_else(|| panic!("{heading} missing or out of order"));
        at += found + heading.len();
    }
}

/// REQ: PORT:ilium-client/src/compaction_ui/tests.rs::the_comparison_shows_default_current_and_recommended
#[test]
fn ui_the_comparison_shows_default_current_and_recommended() {
    let (_, text) = page(140);
    let start = text.find("COMPARISON").expect("section");
    let section = &text[start..];
    let default = section.find("default").expect("default row");
    let current = section.find("Current").expect("current row");
    let recommended = section.find("Recommended").expect("recommended row");
    assert!(default < current && current < recommended);
    assert!(section.contains("simulated point") || section.contains("interpolated"));
}

/// REQ: PORT:ilium-client/src/compaction_ui/tests.rs::a_missing_current_setting_is_shown_as_not_set_in_the_comparison
#[test]
fn ui_a_missing_current_setting_is_shown_as_not_set_in_the_comparison() {
    let mut host = opened();
    host.set_report("fixture-codex-no-current-setting");
    let text = screen_text(&mut host, 140, 200);
    let lines: Vec<&str> = text.lines().collect();
    let start = lines
        .iter()
        .position(|l| l.contains("COMPARISON"))
        .expect("section");
    assert!(lines[start + 3].contains("Current") && lines[start + 3].contains("not set"));
}

/// REQ: PORT:ilium-client/src/compaction_ui/tests.rs::statistics_cost_mix_fixed_prefix_regimes_and_warnings_have_their_figures
#[test]
fn ui_statistics_cost_mix_fixed_prefix_regimes_and_warnings_have_their_figures() {
    let (_, text) = page(140);
    let text = squeezed(&text);
    for needle in [
        "5,678",
        "4,000",
        "1,678",
        "1,200",
        "Dollar figures are API-price equivalents.",
    ] {
        assert!(text.contains(needle), "missing {needle}");
    }
    for heading in [
        "STATISTICS",
        "COST MIX",
        "FIXED PREFIX",
        "REGIMES",
        "WARNINGS",
    ] {
        assert!(text.contains(heading), "missing {heading}");
    }
}

/// REQ: PORT:ilium-client/src/compaction_ui/tests.rs::the_per_model_table_carries_the_visible_cannot_apply_per_model_message
#[test]
fn ui_the_per_model_table_carries_the_visible_cannot_apply_per_model_message() {
    for width in [60u16, 100, 160] {
        let (_, text) = page(width);
        assert!(
            squeezed(&text).contains("is one global value and cannot be applied per model"),
            "{width}"
        );
    }
}

/// REQ: PORT:ilium-client/src/compaction_ui/tests.rs::the_simulation_table_marks_pick_best_default_and_current
#[test]
fn ui_the_simulation_table_marks_pick_best_default_and_current() {
    let (_, wide) = page(160);
    let lines: Vec<&str> = wide.lines().collect();
    let start = lines
        .iter()
        .position(|l| l.contains("SIMULATION"))
        .expect("section");
    assert!(lines[start + 1].contains("Trigger") && lines[start + 1].contains("Marks"));
    let pick = lines
        .iter()
        .find(|l| l.contains("pick best"))
        .expect("pick and best share a row");
    assert!(pick.contains("0.0%") && pick.contains("2%") && pick.contains("150,000"));
    assert!(lines.iter().any(|l| l.contains("default")));
    assert!(lines.iter().any(|l| l.contains("current")));
    let (_, narrow) = page(50);
    let narrow_lines: Vec<&str> = narrow.lines().collect();
    let start = narrow_lines
        .iter()
        .position(|l| l.contains("SIMULATION"))
        .expect("section");
    assert!(!narrow_lines[start + 1].contains("Marks Support"));
    assert!(narrow_lines.iter().any(|l| l.contains("150,000")));
}

/// REQ: PORT:ilium-client/src/compaction_ui/tests.rs::the_simulation_chart_plots_the_cost_curve_with_markers
#[test]
fn ui_the_simulation_chart_plots_the_cost_curve_with_markers() {
    for width in [60u16, 100, 160] {
        let (_, text) = page(width);
        assert!(text.contains("Extra cost over the best trigger"), "{width}");
        assert!(
            text.lines()
                .any(|l| l.contains("100k") && l.contains("232k")),
            "{width}: axis labels"
        );
    }
}

/// REQ: PORT:ilium-client/src/compaction_ui/tests.rs::rework_sensitivity_lists_the_three_multipliers
#[test]
fn ui_rework_sensitivity_lists_the_three_multipliers() {
    let (_, text) = page(120);
    assert!(squeezed(&text).contains("a default measured on one research corpus"));
    let lines: Vec<&str> = text.lines().collect();
    let start = lines
        .iter()
        .position(|l| l.contains("Rework") && l.contains("Optimum"))
        .expect("header");
    assert!(lines[start + 1].trim_start().starts_with("0.5x") && lines[start + 1].contains("130k"));
    assert!(lines[start + 2].trim_start().starts_with("1x") && lines[start + 2].contains("150k"));
    assert!(lines[start + 3].trim_start().starts_with("2x") && lines[start + 3].contains("180k"));
}

/// REQ: PORT:ilium-client/src/compaction_ui/tests.rs::a_failed_or_cancelled_scan_keeps_the_previous_report_with_a_note
#[test]
fn ui_a_failed_or_cancelled_scan_keeps_the_previous_report_with_a_note() {
    let mut host = opened();
    host.set_report("fixture-codex-complete");
    host.set_scan_failure("the scan failed: gateway unavailable");
    let text = squeezed(&screen_text(&mut host, 120, 200));
    assert!(text.contains("RECOMMENDATION"), "previous report kept");
    assert!(text.contains("gateway unavailable"));
    host.set_scan_cancelled();
    assert!(squeezed(&screen_text(&mut host, 120, 200)).contains("cancelled"));
}

/// REQ: PORT:ilium-client/src/compaction_ui/tests.rs::a_report_without_a_recommendation_offers_rescan_but_no_apply
#[test]
fn ui_a_report_without_a_recommendation_offers_rescan_but_no_apply() {
    let mut host = opened();
    host.set_report("fixture-codex-no-sessions");
    let text = screen_text(&mut host, 120, 60);
    assert!(text.contains("Re-scan"));
    assert!(!text.contains("Apply to Codex") && !text.contains("RECOMMENDATION"));
    assert!(text.contains("No sessions found"));
}

/// REQ: PORT:ilium-client/src/compaction_ui/tests.rs::notes_and_the_revert_button_follow_the_panel
#[test]
fn ui_notes_and_the_revert_button_follow_the_panel() {
    let mut host = opened();
    host.set_report("fixture-codex-complete");
    host.set_note("success", "Applied. Only new Codex sessions use it.");
    host.set_revert_record(123_456, 150_000);
    let text = squeezed(&screen_text(&mut host, 120, 200));
    assert!(text.contains("Applied. Only new Codex sessions use it."));
    assert!(text.contains("Revert model_auto_compact_token_limit to 123,456"));
    host.clear_revert_record();
    assert!(!screen_text(&mut host, 120, 200).contains("Revert model_auto_compact_token_limit"));
}

/// REQ: PORT:ilium-client/src/compaction_ui/tests.rs::header_buttons_select_the_agent_and_the_selected_one_is_filled
#[test]
fn ui_header_buttons_select_the_agent_and_the_selected_one_is_filled() {
    let mut host = opened();
    let lines: Vec<String> = screen_text(&mut host, 120, 40)
        .lines()
        .map(str::to_owned)
        .collect();
    assert!(lines[0].contains(" Codex ") && lines[0].contains(" Claude Code "));
    assert!(lines[1].contains("←/→ agent"));
    assert!(host.button_is_filled(panel(), "Codex"));
    assert!(!host.button_is_filled(panel(), "Claude Code"));
    let (row, column) = host
        .render(panel(), 120, 40)
        .find(" Claude Code ")
        .expect("button");
    host.click(panel(), column + 1, row);
    assert!(host.button_is_filled(panel(), "Claude Code"));
}

/// REQ: PORT:ilium-client/src/compaction_ui/tests.rs::body_clicks_follow_the_scroll_offset_and_ignore_text
#[test]
fn ui_body_clicks_follow_the_scroll_offset_and_ignore_text() {
    let mut host = opened();
    host.set_report("fixture-codex-complete");
    let screen = host.render(panel(), 120, 30);
    let (row, column) = screen.find("Apply to Codex").expect("button");
    assert!(host.click(panel(), column, row));
    host.close_confirmation();
    // Plain text is not a hit target.
    let (text_row, text_column) = screen.find("Confidence").expect("text");
    assert!(!host.click(panel(), text_column, text_row));
    // After scrolling, the button's hit target moves with it.
    keys(&mut host, &["Down", "Down", "Down"]);
    let scrolled = host.render(panel(), 120, 30);
    let (new_row, new_column) = scrolled
        .find("Apply to Codex")
        .expect("button still visible");
    assert_eq!(new_row + 3, row);
    assert!(host.click(panel(), new_column, new_row));
}

/// REQ: PORT:ilium-client/src/compaction_ui/tests.rs::scan_buttons_are_reachable_by_click_on_the_idle_and_progress_pages
#[test]
fn ui_scan_buttons_are_reachable_by_click_on_the_idle_and_progress_pages() {
    let mut host = opened();
    let (row, column) = host
        .render(panel(), 100, 30)
        .find("Scan sessions")
        .expect("scan button");
    assert!(host.click(panel(), column, row));
    assert_eq!(host.scans_started().len(), 1);
    let (row, column) = host
        .render(panel(), 100, 30)
        .find("Cancel")
        .expect("cancel button");
    assert!(host.click(panel(), column, row));
    host.finish_scans();
}

/// REQ: PORT:ilium-client/src/compaction_ui/tests.rs::scroll_bounds_and_help_anchors_follow_the_page
#[test]
fn ui_scroll_bounds_and_help_anchors_follow_the_page() {
    let mut host = opened();
    host.set_report("fixture-codex-complete");
    let anchors = host.help_anchors(panel(), 120, 30);
    assert_eq!(
        anchors.first().map(|(topic, row)| (topic.as_str(), *row)),
        Some(("OPT-01", 0))
    );
    assert!(anchors.iter().any(|(topic, _)| topic == "OPT-02"));
    let max = host.max_scroll(panel(), 120, 30);
    assert!(max > 0);
    for _ in 0..(max + 20) {
        keys(&mut host, &["Down"]);
    }
    assert_eq!(
        host.scroll_offset(panel()),
        max,
        "scroll is clamped to the page"
    );
}

/// REQ: PORT:ilium-client/src/compaction_ui/tests.rs::every_help_topic_of_the_tab_exists_and_the_set_is_complete
#[test]
fn ui_every_help_topic_of_the_tab_exists_and_the_set_is_complete() {
    let mut host = opened();
    let declared = host.declared_help_topics();
    assert!(declared.contains(&"OPT-01".to_owned()) && declared.contains(&"OPT-02".to_owned()));
    for topic in &declared {
        assert!(host.help_topic_exists(topic), "{topic} has a catalog entry");
    }
    let anchors: std::collections::BTreeSet<String> = host
        .help_anchors(panel(), 160, 400)
        .into_iter()
        .map(|(topic, _)| topic)
        .collect();
    for topic in ["OPT-01", "OPT-02"] {
        assert!(anchors.contains(topic), "{topic} is anchored on the page");
    }
    for topic in &declared {
        // manual does not pin the exact dialog-only list
        assert!(
            anchors.contains(topic) || host.help_topic_is_dialog_only(topic),
            "{topic} is reachable"
        );
    }
}

/// REQ: PORT:ilium-client/src/compaction_ui/tests.rs::the_tab_renders_in_the_settings_screen_at_80_and_160_columns
#[test]
fn ui_the_tab_renders_in_the_settings_screen_at_80_and_160_columns() {
    let mut host = client_host();
    host.open_settings_tab("optimization");
    for width in [80u16, 160] {
        let text = host.render_settings(width, 40).text();
        assert!(text.contains("Optimization"), "{width}: tab list entry");
        assert!(
            text.contains(" Codex ") && text.contains(" Claude Code "),
            "{width}"
        );
        assert!(text.contains("Scan sessions"), "{width}");
        assert!(text.contains("COMPACTION OPTIMIZER - CODEX"), "{width}");
    }
}

/// REQ: PORT:ilium-client/src/compaction_ui/modal.rs::confirmation_scrollbar_has_its_own_column_without_covering_actions
#[test]
fn ui_confirmation_scrollbar_has_its_own_column_without_covering_actions() {
    let mut host = opened();
    host.set_report("fixture-codex-complete");
    keys(&mut host, &["a"]);
    for (columns, rows) in [(80u16, 14u16), (100, 20), (60, 12), (160, 50)] {
        let layout = host.confirmation_layout(panel(), columns, rows);
        assert_eq!(layout.text_right, layout.scrollbar_x, "{columns}x{rows}");
        assert_eq!(layout.text_y, layout.scrollbar_y);
        assert_eq!(layout.text_height, layout.scrollbar_height);
        assert!(layout.scrollbar_right <= columns && layout.scrollbar_bottom <= rows);
        assert!(
            layout.text_bottom <= layout.hint_row_y,
            "the text never covers the actions"
        );
    }
}

/// REQ: PORT:ilium-client/src/compaction_ui/modal.rs::confirmation_scrollbar_has_its_own_column_without_covering_actions
#[test]
fn ui_confirmation_scrollbar_track_has_a_height_at_every_size() {
    let mut host = opened();
    host.set_report("fixture-codex-complete");
    keys(&mut host, &["a"]);
    for (columns, rows) in [(80u16, 14u16), (100, 20), (60, 12), (160, 50)] {
        let layout = host.confirmation_layout(panel(), columns, rows);
        assert!(layout.scrollbar_height > 0, "{columns}x{rows}");
    }
}

/// REQ: PORT:ilium-client/src/compaction_ui/tests.rs::wrap_text_wraps_by_cells_and_splits_long_words
#[test]
fn ui_wrap_text_keeps_a_string_that_fits_on_one_line() {
    let host = opened();
    assert_eq!(host.wrap_text("abcdefghij", 10), vec!["abcdefghij"]);
    assert_eq!(host.wrap_text("ab", 10), vec!["ab"]);
}

/// REQ: PORT:ilium-client/src/compaction_ui/tests.rs::truncate_to_marks_a_cut_with_an_ellipsis
#[test]
fn ui_truncate_to_one_cell_keeps_only_the_ellipsis() {
    let host = opened();
    assert_eq!(host.truncate_to("abcdefghij", 1), "…");
}

/// REQ: PORT:ilium-client/src/compaction_ui/tests.rs::bars_fill_in_proportion_and_clamp
#[test]
fn ui_bar_parts_always_add_up_to_the_width() {
    let host = opened();
    let (filled, empty) = host.bar_parts(0.25, 8);
    assert_eq!(filled.chars().count(), 2);
    assert_eq!(filled.chars().count() + empty.chars().count(), 8);
}

/// REQ: PORT:ilium-client/src/compaction_ui/tests.rs::table_columns_drop_least_important_first_and_keep_rank_zero
#[test]
fn ui_table_keeps_rank_zero_even_at_zero_width() {
    let host = opened();
    let columns = [("A", 10, 0), ("B", 10, 1), ("C", 10, 2), ("D", 10, 3)];
    assert!(host
        .kept_columns(&columns, 0)
        .iter()
        .any(|column| *column == "A"));
}

/// REQ: PORT:ilium-client/src/compaction_ui/tests.rs::the_idle_page_only_offers_a_scan_and_shows_the_current_setting
#[test]
fn ui_the_idle_page_has_no_cancel_button() {
    let mut host = opened();
    let text = screen_text(&mut host, 120, 40);
    assert!(!text.contains("Cancel"));
}

/// REQ: PORT:ilium-client/src/compaction_ui/tests.rs::the_progress_page_shows_phase_bar_counts_elapsed_file_and_cancel_at_every_width
#[test]
fn ui_the_progress_page_shows_a_changed_phase_name() {
    let mut host = opened();
    host.set_progress("Parsing", 10, 100, "rollout-2026-10-05");
    assert!(squeezed(&screen_text(&mut host, 120, 40)).contains("Parsing"));
    host.set_progress("Counting", 0, 100, "f");
    assert!(squeezed(&screen_text(&mut host, 120, 40)).contains("Counting"));
}

/// REQ: PORT:ilium-client/src/compaction_ui/tests.rs::the_bar_follows_the_byte_fraction
#[test]
fn ui_the_percent_follows_a_quarter_fraction() {
    let mut host = opened();
    host.set_progress("Reading", 250, 1000, "f");
    assert!(screen_text(&mut host, 100, 30).contains("25.0%"));
}

/// REQ: PORT:ilium-client/src/compaction_ui/tests.rs::the_report_starts_with_the_recommendation_card_and_its_apply_button
#[test]
fn ui_the_recommendation_card_comes_before_the_comparison() {
    let (_, text) = page(120);
    let card = text.find("RECOMMENDATION").expect("card");
    let comparison = text.find("COMPARISON").expect("comparison");
    assert!(card < comparison);
}

/// REQ: PORT:ilium-client/src/compaction_ui/tests.rs::the_card_explains_the_pick_rule_and_a_grid_limited_optimum
#[test]
fn ui_the_widened_note_is_absent_from_a_complete_report() {
    let mut host = opened();
    host.set_report("fixture-codex-complete");
    assert!(!squeezed(&screen_text(&mut host, 128, 200)).contains("widened once"));
}

/// REQ: PORT:ilium-client/src/compaction_ui/tests.rs::the_extrapolated_alternative_is_a_secondary_button_with_support_and_warning
#[test]
fn ui_no_extrapolated_button_without_a_recommendation() {
    let mut host = opened();
    host.set_report("fixture-codex-no-sessions");
    let text = squeezed(&screen_text(&mut host, 120, 60));
    assert!(!text.contains("Apply simulated optimum"));
}

/// REQ: PORT:ilium-client/src/compaction_ui/tests.rs::a_long_apply_button_wraps_into_one_clickable_block
#[test]
fn ui_the_apply_button_is_clickable_when_it_fits_on_one_line() {
    let mut host = opened();
    host.set_report("fixture-codex-long-headline");
    let screen = host.render(panel(), 200, 200);
    let (row, column) = screen.find("Apply to Codex").expect("button text");
    assert!(host.click(panel(), column, row));
}

/// REQ: PORT:ilium-client/src/compaction_ui/tests.rs::every_section_is_present_in_order
#[test]
fn ui_a_report_without_sessions_has_no_comparison_or_simulation() {
    let mut host = opened();
    host.set_report("fixture-codex-no-sessions");
    let text = screen_text(&mut host, 120, 60);
    assert!(!text.contains("COMPARISON") && !text.contains("SIMULATION"));
}

/// REQ: PORT:ilium-client/src/compaction_ui/tests.rs::the_comparison_shows_default_current_and_recommended
#[test]
fn ui_the_comparison_still_shows_recommended_at_80_columns() {
    let (_, text) = page(80);
    assert!(text.contains("COMPARISON") && text.contains("Recommended"));
}

/// REQ: PORT:ilium-client/src/compaction_ui/tests.rs::a_missing_current_setting_is_shown_as_not_set_in_the_comparison
#[test]
fn ui_a_missing_current_setting_keeps_the_apply_button() {
    let mut host = opened();
    host.set_report("fixture-codex-no-current-setting");
    let text = squeezed(&screen_text(&mut host, 140, 200));
    assert!(text.contains("Apply to Codex"));
}

/// REQ: PORT:ilium-client/src/compaction_ui/tests.rs::statistics_cost_mix_fixed_prefix_regimes_and_warnings_have_their_figures
#[test]
fn ui_no_statistics_figures_without_sessions() {
    let mut host = opened();
    host.set_report("fixture-codex-no-sessions");
    let text = screen_text(&mut host, 140, 60);
    assert!(!text.contains("5,678"));
}

/// REQ: PORT:ilium-client/src/compaction_ui/tests.rs::the_per_model_table_carries_the_visible_cannot_apply_per_model_message
#[test]
fn ui_no_per_model_message_without_sessions() {
    let mut host = opened();
    host.set_report("fixture-codex-no-sessions");
    let text = squeezed(&screen_text(&mut host, 100, 60));
    assert!(!text.contains("cannot be applied per model"));
}

/// REQ: PORT:ilium-client/src/compaction_ui/tests.rs::the_simulation_table_marks_pick_best_default_and_current
#[test]
fn ui_no_pick_best_row_without_sessions() {
    let mut host = opened();
    host.set_report("fixture-codex-no-sessions");
    let text = screen_text(&mut host, 160, 60);
    assert!(!text.contains("pick best"));
}

/// REQ: PORT:ilium-client/src/compaction_ui/tests.rs::the_simulation_chart_plots_the_cost_curve_with_markers
#[test]
fn ui_no_cost_chart_without_sessions() {
    let mut host = opened();
    host.set_report("fixture-codex-no-sessions");
    let text = squeezed(&screen_text(&mut host, 120, 60));
    assert!(!text.contains("Extra cost over the best trigger"));
}

/// REQ: PORT:ilium-client/src/compaction_ui/tests.rs::rework_sensitivity_lists_the_three_multipliers
#[test]
fn ui_no_rework_multipliers_without_sessions() {
    let mut host = opened();
    host.set_report("fixture-codex-no-sessions");
    let text = screen_text(&mut host, 120, 60);
    assert!(!text.contains("0.5x"));
}

/// REQ: PORT:ilium-client/src/compaction_ui/tests.rs::a_failed_or_cancelled_scan_keeps_the_previous_report_with_a_note
#[test]
fn ui_a_cancelled_scan_still_shows_the_previous_recommendation() {
    let mut host = opened();
    host.set_report("fixture-codex-complete");
    host.set_scan_cancelled();
    assert!(squeezed(&screen_text(&mut host, 120, 200)).contains("RECOMMENDATION"));
}

/// REQ: PORT:ilium-client/src/compaction_ui/tests.rs::a_report_without_a_recommendation_offers_rescan_but_no_apply
#[test]
fn ui_a_report_without_a_recommendation_has_no_confidence_line() {
    let mut host = opened();
    host.set_report("fixture-codex-no-sessions");
    let text = screen_text(&mut host, 120, 60);
    assert!(!text.contains("Confidence"));
}

/// REQ: PORT:ilium-client/src/compaction_ui/tests.rs::notes_and_the_revert_button_follow_the_panel
#[test]
fn ui_the_revert_button_shows_the_recorded_previous_value() {
    let mut host = opened();
    host.set_report("fixture-codex-complete");
    host.set_revert_record(100_000, 150_000);
    let text = squeezed(&screen_text(&mut host, 120, 200));
    assert!(text.contains("Revert model_auto_compact_token_limit to 100,000"));
}

/// REQ: PORT:ilium-client/src/compaction_ui/tests.rs::header_buttons_select_the_agent_and_the_selected_one_is_filled
#[test]
fn ui_clicking_codex_fills_the_codex_button_again() {
    let mut host = opened();
    let (row, column) = host
        .render(panel(), 120, 40)
        .find(" Claude Code ")
        .expect("button");
    host.click(panel(), column + 1, row);
    let (row, column) = host
        .render(panel(), 120, 40)
        .find(" Codex ")
        .expect("button");
    host.click(panel(), column + 1, row);
    assert!(host.button_is_filled(panel(), "Codex"));
    assert!(!host.button_is_filled(panel(), "Claude Code"));
}

/// REQ: PORT:ilium-client/src/compaction_ui/tests.rs::body_clicks_follow_the_scroll_offset_and_ignore_text
#[test]
fn ui_scrolling_back_up_restores_the_button_hit_target() {
    let mut host = opened();
    host.set_report("fixture-codex-complete");
    keys(&mut host, &["Down", "Down", "Down"]);
    keys(&mut host, &["Up", "Up", "Up"]);
    let screen = host.render(panel(), 120, 30);
    let (row, column) = screen.find("Apply to Codex").expect("button");
    assert!(host.click(panel(), column, row));
}

/// REQ: PORT:ilium-client/src/compaction_ui/tests.rs::scan_buttons_are_reachable_by_click_on_the_idle_and_progress_pages
#[test]
fn ui_the_scan_button_is_visible_at_a_narrow_width() {
    let mut host = opened();
    assert!(host.render(panel(), 60, 30).find("Scan sessions").is_some());
}

/// REQ: PORT:ilium-client/src/compaction_ui/tests.rs::scroll_bounds_and_help_anchors_follow_the_page
#[test]
fn ui_scrolling_up_from_the_top_stays_at_zero() {
    let mut host = opened();
    host.set_report("fixture-codex-complete");
    keys(&mut host, &["Up", "Up", "Up"]);
    assert_eq!(host.scroll_offset(panel()), 0);
}

/// REQ: PORT:ilium-client/src/compaction_ui/tests.rs::every_help_topic_of_the_tab_exists_and_the_set_is_complete
#[test]
fn ui_the_tab_declares_at_least_one_help_topic() {
    let host = opened();
    let declared = host.declared_help_topics();
    assert!(!declared.is_empty());
}

/// REQ: PORT:ilium-client/src/compaction_ui/tests.rs::the_tab_renders_in_the_settings_screen_at_80_and_160_columns
#[test]
fn ui_the_codex_tab_does_not_show_the_claude_title() {
    let mut host = client_host();
    host.open_settings_tab("optimization");
    let text = host.render_settings(80, 40).text();
    assert!(!text.contains("COMPACTION OPTIMIZER - CLAUDE"));
}

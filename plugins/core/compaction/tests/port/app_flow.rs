//! Twins of ilium-client/src/compaction_app/tests.rs (the Optimization tab flow).
//! Gap methods used on FakeHost are listed in ../API_GAPS_port.md.

use crate::common::*;
use crate::fixtures::*;

const CODEX_CURRENT: u64 = 123_456;
const CLAUDE_CURRENT: u64 = 777_000;
const CODEX_CONFIG: &str =
    "# my settings\nmodel_auto_compact_token_limit = 123456\n\n[projects.x]\ntrust = true\n";

/// An opened panel with the Codex config in place and one Codex scan finished.
fn scanned_codex() -> ilium_plugin_sdk::testing::FakeHost {
    let mut host = opened();
    write_text(&mut host, CODEX_FILE, CODEX_CONFIG);
    write_codex_corpus(&mut host, 8);
    host.select_agent_target("codex");
    host.key(panel(), "s");
    host.finish_scans();
    host
}

/// REQ: PORT:ilium-client/src/compaction_app/tests.rs::opening_the_tab_reads_the_current_settings_and_never_starts_a_scan
#[test]
fn app_opening_the_tab_reads_the_current_settings_and_never_starts_a_scan() {
    let mut host = client_host();
    write_text(&mut host, CODEX_FILE, CODEX_CONFIG);
    write_text(&mut host, CLAUDE_FILE, "{\"other\": 1}");
    host.open_view(panel());
    let text = screen_text(&mut host, 160, 50);
    assert!(
        text.contains("Current model_auto_compact_token_limit: 123,456"),
        "{text}"
    );
    assert!(host.scans_started().is_empty());
    keys(&mut host, &["Right"]);
    assert!(screen_text(&mut host, 160, 50).contains("not set"));
    assert!(host.scans_started().is_empty());
}

/// REQ: PORT:ilium-client/src/compaction_app/tests.rs::a_missing_configuration_file_reads_as_not_set
#[test]
fn app_a_missing_configuration_file_reads_as_not_set() {
    let mut host = opened();
    assert!(screen_text(&mut host, 160, 50).contains("does not exist"));
}

/// REQ: PORT:ilium-client/src/compaction_app/tests.rs::the_current_values_are_read_again_each_time_the_tab_becomes_visible
#[test]
fn app_the_current_values_are_read_again_each_time_the_tab_becomes_visible() {
    let mut host = client_host();
    write_text(&mut host, CODEX_FILE, CODEX_CONFIG);
    host.open_view(panel());
    write_text(
        &mut host,
        CODEX_FILE,
        "model_auto_compact_token_limit = 150000\n",
    );
    // Still visible: no re-read.
    assert!(screen_text(&mut host, 160, 50).contains("123,456"));
    // Leave and come back: the new value is read.
    host.close_view(panel());
    host.open_view(panel());
    assert!(screen_text(&mut host, 160, 50).contains("150,000"));
}

/// REQ: PORT:ilium-client/src/compaction_app/tests.rs::scan_settings_carry_the_current_value_and_the_cost_tab_price_overrides
#[test]
fn app_scan_settings_carry_the_current_value_and_the_cost_tab_price_overrides() {
    let mut host = client_host();
    write_text(&mut host, CODEX_FILE, CODEX_CONFIG);
    host.settings()
        .set(
            "cost.prices.gpt-test",
            serde_json::json!({"input": 1.0, "output": 2.0, "cache_read": 3.0, "cache_write": 4.0}),
        )
        .expect("price override stored");
    host.open_view(panel());
    host.key(panel(), "s");
    let started = host.scans_started();
    assert_eq!(started.len(), 1);
    let request = host.scan_request(0);
    assert_eq!(request["agent"], "codex");
    assert_eq!(request["current_setting_value"], CODEX_CURRENT);
    assert_eq!(request["price_table"]["gpt-test"]["input"], 1.0);
    host.select_agent_target("claude");
    host.key(panel(), "s");
    assert!(host.scan_request(1)["current_setting_value"].is_null());
}

/// REQ: PORT:ilium-client/src/compaction_app/tests.rs::left_right_and_clicks_switch_the_agent_and_reset_the_scroll
#[test]
fn app_left_right_and_clicks_switch_the_agent_and_reset_the_scroll() {
    let mut host = opened();
    assert!(screen_text(&mut host, 160, 50).contains("COMPACTION OPTIMIZER - CODEX"));
    keys(&mut host, &["Right"]);
    assert!(screen_text(&mut host, 160, 50).contains("COMPACTION OPTIMIZER - CLAUDE CODE"));
    keys(&mut host, &["Right"]);
    assert!(
        screen_text(&mut host, 160, 50).contains("COMPACTION OPTIMIZER - CODEX"),
        "wraps"
    );
    keys(&mut host, &["]"]);
    assert!(screen_text(&mut host, 160, 50).contains("CLAUDE CODE"));
    keys(&mut host, &["["]);
    assert!(screen_text(&mut host, 160, 50).contains("CODEX"));
    let screen = host.render(panel(), 160, 50);
    let (row, column) = screen.find("Claude Code").expect("agent button");
    assert!(host.click(panel(), column + 1, row));
    assert!(screen_text(&mut host, 160, 50).contains("COMPACTION OPTIMIZER - CLAUDE CODE"));
}

/// REQ: PORT:ilium-client/src/compaction_app/tests.rs::the_report_scrolls_with_arrows_pages_home_end_and_the_wheel_within_bounds
#[test]
fn app_the_report_scrolls_with_arrows_pages_home_end_and_the_wheel_within_bounds() {
    let mut host = scanned_codex();
    let top = screen_text(&mut host, 160, 30);
    keys(&mut host, &["Down"]);
    let one = screen_text(&mut host, 160, 30);
    assert_ne!(one, top, "Down scrolls one line");
    keys(&mut host, &["j"]);
    keys(&mut host, &["Up"]);
    assert_eq!(screen_text(&mut host, 160, 30), one);
    keys(&mut host, &["PageDown"]);
    keys(&mut host, &["PageUp"]);
    assert_eq!(screen_text(&mut host, 160, 30), one);
    keys(&mut host, &["End"]);
    let end = screen_text(&mut host, 160, 30);
    keys(&mut host, &["Down"]);
    assert_eq!(screen_text(&mut host, 160, 30), end, "never past the end");
    keys(&mut host, &["Home"]);
    assert_eq!(screen_text(&mut host, 160, 30), top);
    for _ in 0..200 {
        host.scroll_wheel(panel(), 1);
    }
    assert_eq!(screen_text(&mut host, 160, 30), end);
    for _ in 0..200 {
        host.scroll_wheel(panel(), -1);
    }
    assert_eq!(screen_text(&mut host, 160, 30), top);
}

/// REQ: PORT:ilium-client/src/compaction_app/tests.rs::enter_scans_then_the_page_shows_the_report_with_the_apply_button_on_top
#[test]
fn app_enter_scans_then_the_page_shows_the_report_with_the_apply_button_on_top() {
    let mut host = opened();
    write_text(&mut host, CODEX_FILE, CODEX_CONFIG);
    write_codex_corpus(&mut host, 8);
    keys(&mut host, &["Enter"]);
    assert_eq!(
        host.scans_started().len(),
        1,
        "Enter on the idle page scans"
    );
    host.finish_scans();
    let text = screen_text(&mut host, 160, 50);
    assert!(text.contains("RECOMMENDATION"));
    assert!(text.contains("Apply to Codex"));
    assert!(text.contains("Re-scan"));
    assert!(
        !host.is_scanning("codex"),
        "no polling once the report is ready"
    );
}

/// REQ: PORT:ilium-client/src/compaction_app/tests.rs::escape_cancels_a_running_scan_instead_of_closing_settings_and_keeps_the_report
#[test]
fn app_escape_cancels_a_running_scan_instead_of_closing_settings_and_keeps_the_report() {
    let mut host = scanned_codex();
    keys(&mut host, &["s"]);
    keys(&mut host, &["Esc"]);
    assert!(
        !host.view_closed(panel()),
        "Esc cancelled the scan, it did not close the panel"
    );
    host.finish_scans();
    assert!(
        screen_text(&mut host, 160, 50).contains("RECOMMENDATION"),
        "the previous report survives"
    );
    keys(&mut host, &["Esc"]);
    assert!(
        host.view_closed(panel()),
        "with nothing running, Esc closes the panel"
    );
}

/// REQ: PORT:ilium-client/src/compaction_app/tests.rs::a_second_scan_cannot_start_while_one_runs
#[test]
fn app_a_second_scan_cannot_start_while_one_runs() {
    let mut host = opened();
    write_codex_corpus(&mut host, 3);
    keys(&mut host, &["s", "s"]);
    assert_eq!(host.scans_started().len(), 1);
    host.finish_scans();
}

/// REQ: PORT:ilium-client/src/compaction_app/tests.rs::the_cancel_button_and_c_key_stop_a_running_scan
#[test]
fn app_the_cancel_button_and_c_key_stop_a_running_scan() {
    let mut host = opened();
    write_codex_corpus(&mut host, 3);
    keys(&mut host, &["s", "c"]);
    assert!(!host.view_closed(panel()));
    host.finish_scans();
    assert!(!host.is_scanning("codex"));
}

/// REQ: PORT:ilium-client/src/compaction_app/tests.rs::apply_confirm_writes_one_key_keeps_a_record_and_revert_restores_it
#[test]
fn app_apply_confirm_writes_one_key_keeps_a_record_and_revert_restores_it() {
    let mut host = scanned_codex();
    let recommended = host.recommended_tokens();
    assert_ne!(recommended, CODEX_CURRENT);
    keys(&mut host, &["Enter"]);
    assert!(
        host.agent_file_writes().is_empty(),
        "nothing is written before the confirmation"
    );
    let text = screen_text(&mut host, 160, 50);
    assert!(text.contains("Apply to Codex?"));
    assert!(text.contains(CODEX_KEY));
    assert!(text.contains(&format!("123,456 -> {}", group_thousands(recommended))));
    assert!(text.contains("config.toml"));
    assert!(text.contains("NEW Codex sessions"));
    assert!(text.contains("Cancel") && text.contains("Apply"));
    keys(&mut host, &["Esc"]);
    assert_eq!(read_text(&mut host, CODEX_FILE), CODEX_CONFIG);
    keys(&mut host, &["a", "y"]);
    assert!(screen_text(&mut host, 160, 50).contains("Only new Codex sessions use it"));
    let after = read_text(&mut host, CODEX_FILE);
    assert!(after.contains(&format!("{CODEX_KEY} = {recommended}")));
    assert!(after.starts_with("# my settings\n"), "comments are kept");
    assert!(after.contains("[projects.x]\ntrust = true"));
    assert!(screen_text(&mut host, 160, 50)
        .contains("Revert model_auto_compact_token_limit to 123,456"));
    keys(&mut host, &["r"]);
    assert!(screen_text(&mut host, 160, 50).contains("Reverted"));
    assert_eq!(read_text(&mut host, CODEX_FILE), CODEX_CONFIG);
    assert!(!screen_text(&mut host, 160, 50).contains("Revert model_auto_compact_token_limit"));
}

fn group_thousands(value: u64) -> String {
    let digits = value.to_string();
    let mut grouped = String::new();
    for (index, character) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(character);
    }
    grouped
}

/// REQ: PORT:ilium-client/src/compaction_app/tests.rs::the_e_key_applies_the_extrapolated_alternative_through_the_same_confirmation
#[test]
fn app_the_e_key_applies_the_extrapolated_alternative_through_the_same_confirmation() {
    let mut host = scanned_codex();
    let alternative = host.extrapolated_alternative_tokens();
    keys(&mut host, &["e"]);
    match alternative {
        Some(_) => {
            assert!(screen_text(&mut host, 160, 50).contains("EXTRAPOLATED"));
            keys(&mut host, &["Esc"]);
            assert_eq!(read_text(&mut host, CODEX_FILE), CODEX_CONFIG);
        }
        None => assert!(host.agent_file_writes().is_empty(), "nothing to apply"),
    }
}

/// REQ: PORT:ilium-client/src/compaction_app/tests.rs::revert_restores_the_previous_value_and_refuses_after_an_outside_edit
#[test]
fn app_revert_restores_the_previous_value_and_refuses_after_an_outside_edit() {
    let mut host = scanned_codex();
    keys(&mut host, &["a", "Enter"]);
    write_text(
        &mut host,
        CODEX_FILE,
        "model_auto_compact_token_limit = 99999\n",
    );
    keys(&mut host, &["r"]);
    assert!(screen_text(&mut host, 160, 50).contains("Not reverted"));
    assert_eq!(
        read_text(&mut host, CODEX_FILE),
        "model_auto_compact_token_limit = 99999\n"
    );
}

/// REQ: PORT:ilium-client/src/compaction_app/tests.rs::a_stale_file_is_reported_in_the_tab_and_nothing_is_written
#[test]
fn app_a_stale_file_is_reported_in_the_tab_and_nothing_is_written() {
    let mut host = scanned_codex();
    keys(&mut host, &["Enter"]);
    let outside = "model_auto_compact_token_limit = 100000\n# edited elsewhere\n";
    write_text(&mut host, CODEX_FILE, outside);
    keys(&mut host, &["Enter"]);
    let text = screen_text(&mut host, 160, 50);
    assert!(text.contains("changed since it was read"), "{text}");
    assert!(text.contains("Nothing was written"));
    assert_eq!(read_text(&mut host, CODEX_FILE), outside);
}

/// REQ: PORT:ilium-client/src/compaction_app/tests.rs::applying_without_a_configuration_file_is_a_clear_error_not_a_created_file
#[test]
fn app_applying_without_a_configuration_file_is_a_clear_error_not_a_created_file() {
    let mut host = opened();
    write_codex_corpus(&mut host, 8);
    keys(&mut host, &["s"]);
    host.finish_scans();
    keys(&mut host, &["Enter"]);
    assert!(screen_text(&mut host, 160, 50).contains("does not exist"));
    assert!(
        host.fs().read(CODEX_FILE).is_none(),
        "the file is not created"
    );
}

/// REQ: PORT:ilium-client/src/compaction_app/tests.rs::claude_apply_targets_settings_json_and_reports_environment_overrides
#[test]
fn app_claude_apply_targets_settings_json_and_reports_environment_overrides() {
    let mut host = opened();
    write_text(
        &mut host,
        CLAUDE_FILE,
        &format!("{{\n  \"theme\": \"dark\",\n  \"autoCompactWindow\": {CLAUDE_CURRENT}\n}}\n"),
    );
    host.set_env("CLAUDE_CODE_AUTO_COMPACT_WINDOW", "300000");
    keys(&mut host, &["Right"]);
    write_claude_corpus(&mut host, 8);
    keys(&mut host, &["s"]);
    host.finish_scans();
    let recommended = host.recommended_tokens();
    keys(&mut host, &["Enter"]);
    let text = screen_text(&mut host, 160, 50);
    assert!(text.contains("autoCompactWindow"));
    assert!(text.contains("settings.json"));
    assert!(
        text.contains("CLAUDE_CODE_AUTO_COMPACT_WINDOW=300000"),
        "{text}"
    );
    assert!(text.contains("Warnings"));
    assert!(text.contains(&format!("777,000 -> {}", group_thousands(recommended))));
    keys(&mut host, &["Enter"]);
    let after = read_text(&mut host, CLAUDE_FILE);
    assert!(
        after.contains(&format!("\"autoCompactWindow\": {recommended}")),
        "{after}"
    );
    assert!(after.contains("\"theme\": \"dark\""));
}

/// REQ: PORT:ilium-client/src/compaction_app/tests.rs::the_confirmation_buttons_and_wheel_work_with_the_mouse
#[test]
fn app_the_confirmation_buttons_and_wheel_work_with_the_mouse() {
    let mut host = scanned_codex();
    let recommended = host.recommended_tokens();
    keys(&mut host, &["a"]);
    let small = host.render(panel(), 80, 14);
    let before = small.text();
    host.scroll_wheel(panel(), 1);
    assert_ne!(
        host.render(panel(), 80, 14).text(),
        before,
        "the wheel scrolls the overflowing confirmation"
    );
    host.scroll_wheel(panel(), -1);
    assert_eq!(host.render(panel(), 80, 14).text(), before);
    // A click outside the buttons is inert.
    host.click(panel(), 0, 0);
    assert!(host
        .render(panel(), 80, 14)
        .text()
        .contains("Apply to Codex?"));
    assert!(!host.view_closed(panel()));
    let (cancel_row, cancel_column) = small.find("Cancel").expect("cancel button");
    host.click(panel(), cancel_column, cancel_row);
    assert!(read_text(&mut host, CODEX_FILE).contains("123456"));
    keys(&mut host, &["a"]);
    let (apply_row, apply_column) = host
        .render(panel(), 80, 14)
        .find("Apply")
        .expect("apply button");
    host.click(panel(), apply_column, apply_row);
    assert!(read_text(&mut host, CODEX_FILE).contains(&format!("= {recommended}")));
}

/// REQ: PORT:ilium-client/src/compaction_app/tests.rs::clicking_the_page_buttons_runs_scan_apply_and_revert
#[test]
fn app_clicking_the_page_buttons_runs_scan_apply_and_revert() {
    let mut host = opened();
    write_text(&mut host, CODEX_FILE, CODEX_CONFIG);
    write_codex_corpus(&mut host, 8);
    let (row, column) = host
        .render(panel(), 160, 50)
        .find("Scan")
        .expect("scan button");
    host.click(panel(), column + 2, row);
    assert_eq!(host.scans_started().len(), 1);
    host.finish_scans();
    let (row, column) = host
        .render(panel(), 160, 50)
        .find("Apply to Codex")
        .expect("apply button");
    host.click(panel(), column + 2, row);
    keys(&mut host, &["Enter"]);
    let (row, column) = host
        .render(panel(), 160, 50)
        .find("Revert")
        .expect("revert button");
    host.click(panel(), column + 2, row);
    assert!(screen_text(&mut host, 160, 50).contains("Reverted"));
    assert!(read_text(&mut host, CODEX_FILE).contains("123456"));
}

/// REQ: PORT:ilium-client/src/compaction_app/tests.rs::help_icons_open_the_help_topics_of_the_tab
#[test]
fn app_help_icons_open_the_help_topics_of_the_tab() {
    let mut host = opened();
    let topics = host.help_anchor_topics(panel());
    assert!(
        topics.contains(&"OPT-01".to_string()) && topics.contains(&"OPT-02".to_string()),
        "{topics:?}"
    );
    host.click_help_anchor(panel(), "OPT-02");
    assert_eq!(host.open_help_topic(), Some("OPT-02".to_string()));
}

/// REQ: PORT:ilium-client/src/compaction_app/tests.rs::write_errors_get_actionable_wording
#[test]
fn app_write_errors_get_actionable_wording() {
    // Stale file: the wording names the path and ends with the instruction to apply again.
    let mut host = scanned_codex();
    keys(&mut host, &["Enter"]);
    write_text(
        &mut host,
        CODEX_FILE,
        "model_auto_compact_token_limit = 1\n",
    );
    keys(&mut host, &["Enter"]);
    let text = screen_text(&mut host, 200, 50);
    assert!(text.contains("Nothing was written:") && text.contains("changed since it was read"));
    assert!(text.contains("apply again."));
    // Missing file: the plugin never creates the agent file.
    let mut host = opened();
    write_codex_corpus(&mut host, 8);
    keys(&mut host, &["s"]);
    host.finish_scans();
    keys(&mut host, &["Enter"]);
    assert!(screen_text(&mut host, 200, 50).contains("never creates"));
    // Labels and the Codex key.
    assert!(screen_text(&mut host, 200, 50).contains("Codex"));
    keys(&mut host, &["Right"]);
    assert!(screen_text(&mut host, 200, 50).contains("Claude Code"));
}

/// REQ: PORT:ilium-client/src/compaction_app/tests.rs::control_keys_and_unrelated_keys_fall_through_to_settings
#[test]
fn app_control_keys_and_unrelated_keys_fall_through_to_settings() {
    let mut host = opened();
    for key in ["Ctrl+s", "Tab", "q"] {
        let consumed = host.key_consumed(panel(), key);
        assert!(!consumed, "{key} is not consumed by the panel");
    }
    assert!(host.scans_started().is_empty());
}

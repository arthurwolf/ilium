//! The Plugins tab (page section "The Plugins tab"): the state words, the row layout pinned as a
//! golden file taken from the page figure, and the rules USR-161 to USR-164.

use ilium_plugin_sdk::testing::assert_golden;
use ilium_plugin_sdk::types::ViewTarget;

use crate::common::*;

fn state_rows() -> Vec<Vec<String>> {
    let page = page_text();
    let start = page.find("#### Plugin row states").expect("states section");
    table_rows(&page[start..], "| Row word |")
}

fn plugins_tab() -> ViewTarget {
    ViewTarget::SettingsPage("plugins".to_string())
}

/// REQ: USR-161, F-090
#[test]
fn plugins_161_the_state_table_has_the_eight_row_words_in_order() {
    let words: Vec<String> = state_rows().into_iter().map(|row| row[0].clone()).collect();
    assert_eq!(
        words,
        [
            "On",
            "On, slow",
            "Restarting",
            "Failed",
            "Off",
            "Waiting for <owner>",
            "Blocked by policy",
            "Blocked",
            "Refused: <reason>",
        ]
    );
}

/// REQ: USR-161, F-090
#[test]
fn plugins_161_each_word_maps_to_one_package_state_and_health() {
    let rows = state_rows();
    let states: Vec<&str> = rows.iter().map(|row| row[2].as_str()).collect();
    assert_eq!(
        states,
        [
            "`installed-enabled`, `healthy`",
            "`installed-enabled`, `degraded`",
            "`installed-enabled`, `restarting`",
            "`installed-enabled` with a side `stopped-by-failure`, `failed`",
            "`installed-disabled-by-user`, `disabled`",
            "`dormant`",
            "`disabled-by-policy`, `disabled`",
            "`blocked`, `disabled`",
            "`refused-at-load`, `refused`",
        ]
    );
}

/// REQ: USR-161, F-090
#[test]
fn plugins_161_no_two_states_share_a_row_word() {
    let mut words: Vec<String> = state_rows().into_iter().map(|row| row[0].clone()).collect();
    words.sort();
    words.dedup();
    assert_eq!(words.len(), 9);
}

/// REQ: USR-161, F-090
#[test]
fn plugins_161_the_failed_state_means_three_failures_in_ten_minutes() {
    let rows = state_rows();
    let failed = rows
        .iter()
        .find(|row| row[0] == "Failed")
        .expect("Failed row");
    assert_eq!(failed[1], "Stopped after 3 failures in 10 minutes");
    assert_eq!(
        failed[3],
        "Turn on (clears the count), reset failures, remove"
    );
}

/// REQ: USR-161, F-090
#[test]
fn plugins_161_what_the_user_can_do_per_state() {
    let rows = state_rows();
    let actions: Vec<&str> = rows.iter().map(|row| row[3].as_str()).collect();
    assert_eq!(actions[0], "Turn off, update, remove");
    assert_eq!(actions[1], "Turn off, read the log");
    assert_eq!(actions[2], "Turn off");
    assert_eq!(actions[4], "Turn on");
    assert_eq!(actions[5], "Turn on or install the owner");
    assert_eq!(actions[6], "Nothing; ask the administrator");
    assert_eq!(
        actions[7],
        "`ilium plugin enable <id> --accept-risk`, or install an update signed by another trusted key"
    );
    assert_eq!(actions[8], "Remove, or reinstall a fixed package");
}

/// REQ: USR-161, F-090
#[test]
fn plugins_161_safe_mode_adds_off_safe_mode_for_third_party_rows() {
    assert!(page_text().contains("In safe mode, a third-party row reads `Off, safe mode`."));
}

/// REQ: USR-162, F-090
#[test]
fn plugins_162_the_figure_shows_tier_badges_t1_t2_t3() {
    let figure = &page_figures()[2];
    for badge in ["T1", "T2", "T3"] {
        assert!(figure.contains(badge), "{badge}");
    }
}

/// REQ: USR-162, F-090
#[test]
fn plugins_162_the_golden_file_equals_the_page_figure() {
    let golden = std::fs::read_to_string(golden_path("plugins_tab_58.txt")).expect("golden");
    assert_eq!(golden.trim_end(), page_figures()[2]);
}

/// REQ: USR-162, USR-164, F-090
#[test]
fn plugins_162_the_plugins_tab_renders_like_the_page_figure_at_58_columns() {
    let mut host = client_host();
    let screen = host.render(plugins_tab(), 58, 10);
    assert_golden(
        &golden_path("plugins_tab_58.txt"),
        &format!("{}\n", screen.text()),
    );
}

/// REQ: USR-162, F-090
#[test]
fn plugins_162_a_row_shows_id_tier_badge_and_state_word() {
    let mut host = client_host();
    let screen = host.render(plugins_tab(), 80, 12);
    let (row, _) = screen
        .find("ilium.core.settings")
        .expect("core plugins are listed too");
    let line = screen.row(row);
    assert!(line.contains("T1"), "{line}");
    assert!(line.contains("On"), "{line}");
}

/// REQ: USR-161, USR-164, F-090
#[test]
fn plugins_164_every_rendered_state_starts_with_a_word_of_the_state_table() {
    let words: Vec<String> = state_rows()
        .into_iter()
        .map(|row| row[0].split(" <").next().unwrap_or("").to_string())
        .collect();
    let mut host = client_host();
    let screen = host.render(plugins_tab(), 80, 12);
    let mut checked = 0;
    for row in 0..screen.rows {
        let line = screen.row(row);
        let Some(position) = ["  T1  ", "  T2  ", "  T3  "]
            .iter()
            .find_map(|badge| line.find(badge))
        else {
            continue;
        };
        let state = line[position + 6..]
            .trim_start()
            .trim_end_matches('│')
            .trim();
        assert!(
            words.iter().any(|word| state.starts_with(word.as_str())),
            "{line}"
        );
        checked += 1;
    }
    assert!(checked >= 1, "no plugin row was drawn");
}

/// REQ: USR-162, F-090
#[test]
fn plugins_162_the_tab_ends_with_the_key_hints() {
    let figure = &page_figures()[2];
    assert!(figure.contains("Enter open  Space on or off  i install file"));
    let mut host = client_host();
    let text = host.render(plugins_tab(), 58, 10).text();
    assert!(
        text.contains("Enter open  Space on or off  i install file"),
        "{text}"
    );
}

/// REQ: USR-163, F-090
#[test]
fn plugins_163_the_page_states_the_forget_settings_rule() {
    let text = manual_page("reference-requirements-user-admin.md");
    assert!(text.contains("The tab MUST offer Forget settings on a plugin's Details page."));
    assert!(
        page_text().contains("The tab MUST offer **Forget settings** on a plugin's Details page.")
    );
}

/// REQ: USR-164, F-090
#[test]
fn plugins_164_the_page_ties_the_state_words_to_footer_notice_n14() {
    assert!(page_text().contains("The state words MUST be the same in the footer notice N14"));
}

/// REQ: F-090
#[test]
fn plugins_090_the_plugin_page_has_six_tabs() {
    assert!(
        page_text().contains("tabs Overview, Capabilities, Extensions, Log, Details and Settings.")
    );
}

/// REQ: F-090
#[test]
fn plugins_090_the_plugins_tab_opens_with_the_prefix_colon_or_the_footer_gear() {
    let page = page_text();
    assert!(page.contains("Press `Ctrl+B` then `:` to open Settings, or press the gear in the tree footer, then choose **Plugins** with `Tab`."));
}

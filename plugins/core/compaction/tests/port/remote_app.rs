//! Twins of the inline tests in ilium-client/src/remote_compaction_app.rs (the Remote
//! compaction settings tab behaviour and persistence). Gaps: `open_settings_tab`,
//! `settings_key`, `settings_click`, `settings_row_geometry`, `settings_dialog` (see ../API_GAPS_port.md).

use serde_json::{json, Value};

use ilium_plugin_sdk::testing::FakeHost;

use crate::common::*;

/// The settings auto-save debounce, in fake-clock milliseconds.
const DEBOUNCE_MS: u64 = 600;

fn tab() -> FakeHost {
    let mut host = client_host();
    host.open_settings_tab("remote-compaction");
    host
}

fn saved(host: &mut FakeHost) -> Value {
    host.settings()
        .get("remote-compaction")
        .unwrap_or_else(|| json!({}))
}

fn dismissed_tab() -> FakeHost {
    let mut host = tab();
    host.settings_key("x");
    host.settle();
    host
}

/// REQ: PORT:ilium-client/src/remote_compaction_app.rs::toggles_and_selects_persist_immediately
#[test]
fn rapp_toggles_and_selects_persist_immediately() {
    let mut host = dismissed_tab();
    host.settings_adjust_row("enabled", 0);
    host.settings_adjust_row("technique:codex", 1);
    assert!(!host.settings_save_pending());
    host.settle();
    let loaded = saved(&mut host);
    assert_eq!(loaded["enabled"], true);
    assert_ne!(loaded["codex_technique"], "codex");
}

/// REQ: PORT:ilium-client/src/remote_compaction_app.rs::numeric_rows_wait_for_the_debounce_then_save_once
#[test]
fn rapp_numeric_rows_wait_for_the_debounce_then_save_once() {
    let mut host = dismissed_tab();
    host.settings_adjust_row("threshold", 1);
    host.settings_adjust_row("threshold", 1);
    assert!(host.settings_save_pending());
    host.settle();
    assert_eq!(
        saved(&mut host)["threshold_percent"].as_u64().unwrap_or(65),
        65,
        "nothing saved yet"
    );
    host.advance_clock(DEBOUNCE_MS - 1);
    host.settle();
    assert_eq!(
        saved(&mut host)["threshold_percent"].as_u64().unwrap_or(65),
        65
    );
    host.advance_clock(1);
    host.settle();
    assert_eq!(saved(&mut host)["threshold_percent"], 67);
    assert!(!host.settings_save_pending());
    let writes = host.settings_write_count();
    host.advance_clock(5_000);
    host.settle();
    assert_eq!(host.settings_write_count(), writes, "saved once");
}

/// REQ: PORT:ilium-client/src/remote_compaction_app.rs::an_immediate_save_carries_and_cancels_a_pending_numeric_change
#[test]
fn rapp_an_immediate_save_carries_and_cancels_a_pending_numeric_change() {
    let mut host = dismissed_tab();
    host.settings_adjust_row("keep_backups", 1);
    assert!(host.settings_save_pending());
    host.settings_adjust_row("automatic", 0);
    assert!(!host.settings_save_pending());
    host.settle();
    let loaded = saved(&mut host);
    assert_eq!(loaded["keep_backups"], 4);
    assert_eq!(loaded["automatic"], true);
}

/// REQ: PORT:ilium-client/src/remote_compaction_app.rs::dismissing_the_banner_persists_forever_and_only_once
#[test]
fn rapp_dismissing_the_banner_persists_forever_and_only_once() {
    let mut host = tab();
    assert_ne!(saved(&mut host)["privacy_banner_dismissed"], true);
    host.settings_dismiss_banner();
    host.settle();
    assert_eq!(saved(&mut host)["privacy_banner_dismissed"], true);
    let writes = host.settings_write_count();
    host.settings_dismiss_banner();
    host.settle();
    assert_eq!(host.settings_write_count(), writes, "no second save");
}

/// REQ: PORT:ilium-client/src/remote_compaction_app.rs::the_custom_prompt_edits_persist_and_clear
#[test]
fn rapp_the_custom_prompt_edits_persist_and_clear() {
    let mut host = dismissed_tab();
    host.settings_commit_prompt("Keep API names.");
    host.settle();
    assert_eq!(saved(&mut host)["custom_prompt"], "Keep API names.");
    host.settings_clear_prompt();
    host.settle();
    assert_eq!(saved(&mut host)["custom_prompt"], "");
}

/// REQ: PORT:ilium-client/src/remote_compaction_app.rs::the_configuration_command_is_byte_accounted_even_at_the_largest_prompt
#[test]
fn rapp_the_configuration_command_is_byte_accounted_even_at_the_largest_prompt() {
    let mut host = dismissed_tab();
    let largest = "\u{1}".repeat(host.custom_prompt_max_chars());
    host.settings_commit_prompt(&largest);
    host.settle();
    let loaded = saved(&mut host);
    let stored = loaded["custom_prompt"].as_str().expect("prompt stored");
    assert!(!stored.is_empty() && stored.len() < host.custom_prompt_max_chars() * 6);
    let accounted = host.last_settings_command_bytes();
    assert!(accounted >= stored.len());
    assert!(accounted <= 1024 * 1024 + 4096);
    host.settings_clear_prompt();
    host.settle();
    assert!(host.last_settings_command_bytes() < accounted);
}

/// REQ: PORT:ilium-client/src/remote_compaction_app.rs::keyboard_closes_the_banner_for_good_and_ignores_stray_arrows
#[test]
fn rapp_keyboard_closes_the_banner_for_good_and_ignores_stray_arrows() {
    for dismiss in ["x", "Delete", "Enter", " "] {
        let mut host = tab();
        assert_eq!(
            host.settings_rows().first().map(String::as_str),
            Some("privacy_banner")
        );
        host.settings_key("Right");
        assert_ne!(
            saved(&mut host)["privacy_banner_dismissed"],
            true,
            "arrows never close it"
        );
        host.settings_key(dismiss);
        host.settle();
        assert_eq!(
            saved(&mut host)["privacy_banner_dismissed"],
            true,
            "{dismiss}"
        );
        assert_eq!(
            host.settings_rows().first().map(String::as_str),
            Some("enabled")
        );
        assert_eq!(host.settings_selected_row(), 0);
    }
}

/// REQ: PORT:ilium-client/src/remote_compaction_app.rs::keyboard_walks_the_rows_toggles_and_cycles_each_agent_technique
#[test]
fn rapp_keyboard_walks_the_rows_toggles_and_cycles_each_agent_technique() {
    let mut host = dismissed_tab();
    host.settings_key(" ");
    assert_eq!(host.settings_value("enabled"), json!(true));
    host.settings_key(" ");
    assert_eq!(host.settings_value("enabled"), json!(false));
    for target in ["claude", "codex", "other"] {
        let row = format!("technique:{target}");
        while host.settings_selected_row() < host.settings_row_index(&row) {
            host.settings_key("Down");
        }
        let start = host.settings_value(&row);
        host.settings_key("Right");
        let next = host.settings_value(&row);
        assert_ne!(next, start, "{target}");
        host.settings_key("Left");
        assert_eq!(host.settings_value(&row), start);
        host.settings_key("Enter");
        let dialog = host
            .settings_dialog()
            .expect("Enter opens the technique catalogue");
        assert_eq!(dialog["selected_id"], start);
        host.settings_dialog_choose(next.as_str().expect("technique id"));
        host.settle();
        assert!(host.settings_dialog().is_none());
        assert_eq!(host.settings_value(&row), next);
        assert_eq!(saved(&mut host)[format!("{target}_technique")], next);
    }
    while host.settings_selected_row() > host.settings_row_index("threshold") {
        host.settings_key("Up");
    }
    for key in ["+", "+", "-"] {
        host.settings_key(key);
    }
    assert_eq!(host.settings_value("threshold"), json!(66));
    assert!(host.settings_save_pending());
    host.advance_clock(DEBOUNCE_MS);
    host.settle();
    assert_eq!(saved(&mut host)["threshold_percent"], 66);
    let before = saved(&mut host);
    host.settings_key("x");
    host.settle();
    assert_eq!(saved(&mut host), before, "x off the banner changes nothing");
}

/// REQ: PORT:ilium-client/src/remote_compaction_app.rs::keyboard_enter_on_the_prompt_row_opens_the_editor_and_delete_clears_the_text
#[test]
fn rapp_keyboard_enter_on_the_prompt_row_opens_the_editor_and_delete_clears_the_text() {
    let mut host = dismissed_tab();
    host.settings_commit_prompt("Keep API names.");
    host.settle();
    while host.settings_selected_row() < host.settings_row_index("custom_prompt") {
        host.settings_key("Down");
    }
    host.settings_key("Delete");
    host.settle();
    assert_eq!(saved(&mut host)["custom_prompt"], "");
    host.settings_commit_prompt("Again");
    host.settle();
    host.settings_key("Enter");
    assert_eq!(host.prompt_editor_text().as_deref(), Some("Again"));
}

/// REQ: PORT:ilium-client/src/remote_compaction_app.rs::mouse_click_on_the_x_closes_the_banner_for_good_and_other_clicks_do_not
#[test]
fn rapp_mouse_click_on_the_x_closes_the_banner_for_good_and_other_clicks_do_not() {
    let mut host = tab();
    let banner = host.settings_row_geometry("privacy_banner");
    host.settings_click(banner.box_column + 6, banner.first_line + 1);
    assert_ne!(
        saved(&mut host)["privacy_banner_dismissed"],
        true,
        "a click off the button only selects the row"
    );
    assert_eq!(host.settings_selected_row(), 0);
    host.settings_click(banner.control_column + 1, banner.control_line);
    host.settle();
    assert_eq!(saved(&mut host)["privacy_banner_dismissed"], true);
    assert_eq!(
        host.settings_rows().first().map(String::as_str),
        Some("enabled")
    );
}

/// REQ: PORT:ilium-client/src/remote_compaction_app.rs::mouse_toggles_steps_and_cycles_technique_rows_from_the_control_halves
#[test]
fn rapp_mouse_toggles_steps_and_cycles_technique_rows_from_the_control_halves() {
    let mut host = dismissed_tab();
    let enabled = host.settings_row_geometry("enabled");
    host.settings_click(enabled.box_column + 8, enabled.first_line);
    assert_eq!(host.settings_value("enabled"), json!(true));
    let threshold = host.settings_row_geometry("threshold");
    host.settings_click(threshold.next_column, threshold.control_line);
    assert_eq!(host.settings_value("threshold"), json!(66));
    for _ in 0..2 {
        let threshold = host.settings_row_geometry("threshold");
        host.settings_click(threshold.previous_column, threshold.control_line);
    }
    assert_eq!(host.settings_value("threshold"), json!(64));
    let threshold = host.settings_row_geometry("threshold");
    host.settings_click(threshold.control_column + 4, threshold.control_line + 1);
    assert_eq!(
        host.settings_value("threshold"),
        json!(64),
        "the description line is inert"
    );
    let claude = host.settings_row_geometry("technique:claude");
    host.settings_click(claude.value_column, claude.control_line);
    assert_eq!(host.settings_value("technique:claude"), json!("codex"));
    let prompt = host.settings_row_geometry("custom_prompt");
    host.settings_click(prompt.box_column + 8, prompt.first_line);
    assert!(host.prompt_editor_text().is_some());
    host.advance_clock(DEBOUNCE_MS);
    host.settle();
    let loaded = saved(&mut host);
    assert_eq!(loaded["enabled"], true);
    assert_eq!(loaded["threshold_percent"], 64);
    assert_eq!(loaded["claude_technique"], "codex");
}

/// REQ: PORT:ilium-client/src/remote_compaction_app.rs::numeric_star_accepts_direct_entry_and_persists_remote_compaction_value
#[test]
fn rapp_numeric_star_accepts_direct_entry_and_persists_remote_compaction_value() {
    let mut host = dismissed_tab();
    let threshold = host.settings_row_geometry("threshold");
    host.settings_click(threshold.open_column, threshold.control_line);
    let dialog = host
        .settings_dialog()
        .expect("star opens direct numeric entry");
    assert_eq!(dialog["kind"], "number");
    for _ in 0..dialog["draft"].as_str().unwrap_or_default().chars().count() {
        host.settings_dialog_key("Backspace");
    }
    for character in ["6", "7"] {
        host.settings_dialog_key(character);
    }
    host.settings_dialog_key("Enter");
    host.settle();
    assert_eq!(saved(&mut host)["threshold_percent"], 67);
    assert!(host.settings_dialog().is_none());
}

/// REQ: PORT:ilium-client/src/remote_compaction_app.rs::technique_plus_opens_full_catalog_and_pointer_selection_persists
#[test]
fn rapp_technique_plus_opens_full_catalog_and_pointer_selection_persists() {
    let mut host = dismissed_tab();
    let all = host.technique_ids();
    let (selected, wanted) = (all.last().unwrap().clone(), all[0].clone());
    assert_ne!(selected, wanted);
    host.settings_set_value("technique:claude", json!(selected));
    let claude = host.settings_row_geometry("technique:claude");
    host.settings_click(claude.open_column, claude.control_line);
    let dialog = host
        .settings_dialog()
        .expect("plus opens the complete catalog");
    assert_eq!(dialog["target"], "technique:claude");
    assert_eq!(dialog["options"].as_array().unwrap().len(), all.len());
    let index = dialog["options"]
        .as_array()
        .unwrap()
        .iter()
        .position(|o| o["id"] == wanted)
        .expect("in catalog");
    host.settings_dialog_click_option(index);
    host.settle();
    assert_eq!(saved(&mut host)["claude_technique"], json!(wanted));
}

/// REQ: PORT:ilium-client/src/remote_compaction_app.rs::the_prompt_row_opens_the_editor_on_the_stored_text
#[test]
fn rapp_the_prompt_row_opens_the_editor_on_the_stored_text() {
    let mut host = tab();
    host.settings_commit_prompt("Line one\nLine two");
    host.settings_adjust_row("custom_prompt", 0);
    assert_eq!(
        host.prompt_editor_text().as_deref(),
        Some("Line one\nLine two")
    );
    host.settings_commit_prompt("Changed");
    host.settle();
    assert_eq!(saved(&mut host)["custom_prompt"], "Changed");
}

/// REQ: PORT:ilium-client/src/remote_compaction_app.rs::toggles_and_selects_persist_immediately
#[test]
fn rapp_each_toggle_is_saved_as_soon_as_it_is_made() {
    let mut host = dismissed_tab();
    host.settings_adjust_row("enabled", 0);
    assert!(!host.settings_save_pending());
    host.settings_adjust_row("enabled", 0);
    assert!(!host.settings_save_pending());
    host.settle();
    assert_eq!(saved(&mut host)["enabled"], host.settings_value("enabled"));
}

/// REQ: PORT:ilium-client/src/remote_compaction_app.rs::numeric_rows_wait_for_the_debounce_then_save_once
#[test]
fn rapp_a_further_numeric_change_restarts_the_debounce() {
    let mut host = dismissed_tab();
    host.settings_adjust_row("threshold", 1);
    host.advance_clock(DEBOUNCE_MS - 100);
    host.settings_adjust_row("threshold", 1);
    host.advance_clock(DEBOUNCE_MS - 100);
    host.settle();
    assert_eq!(
        saved(&mut host)["threshold_percent"].as_u64().unwrap_or(65),
        65,
        "the second change restarted the wait"
    );
    host.advance_clock(100);
    host.settle();
    assert_eq!(saved(&mut host)["threshold_percent"], 67);
}

/// REQ: PORT:ilium-client/src/remote_compaction_app.rs::an_immediate_save_carries_and_cancels_a_pending_numeric_change
#[test]
fn rapp_an_immediate_save_leaves_no_later_numeric_write() {
    let mut host = dismissed_tab();
    host.settings_adjust_row("keep_backups", 1);
    host.settings_adjust_row("automatic", 0);
    host.settle();
    let writes = host.settings_write_count();
    host.advance_clock(5_000);
    host.settle();
    assert_eq!(
        host.settings_write_count(),
        writes,
        "the cancelled numeric save never runs"
    );
}

/// REQ: PORT:ilium-client/src/remote_compaction_app.rs::dismissing_the_banner_persists_forever_and_only_once
#[test]
fn rapp_the_banner_row_is_hidden_once_dismissed() {
    let mut host = tab();
    assert!(
        host.settings_banner_visible(),
        "the banner shows until dismissed"
    );
    host.settings_dismiss_banner();
    host.settle();
    assert!(!host.settings_banner_visible());
    assert!(host.settings_banner_close_rect().is_none());
}

/// REQ: PORT:ilium-client/src/remote_compaction_app.rs::the_custom_prompt_edits_persist_and_clear
#[test]
fn rapp_an_overlong_custom_prompt_is_kept_within_the_maximum_length() {
    let mut host = dismissed_tab();
    let limit = host.custom_prompt_max_chars();
    host.settings_commit_prompt(&"k".repeat(limit + 1));
    host.settle();
    let stored = saved(&mut host)["custom_prompt"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    assert!(
        stored.chars().count() <= limit,
        "stored {} characters",
        stored.chars().count()
    );
}

/// REQ: PORT:ilium-client/src/remote_compaction_app.rs::the_configuration_command_is_byte_accounted_even_at_the_largest_prompt
#[test]
fn rapp_the_largest_prompt_settings_pass_the_serialized_size_bound() {
    let mut host = dismissed_tab();
    let largest = "\u{1}".repeat(host.custom_prompt_max_chars());
    host.settings_commit_prompt(&largest);
    host.settle();
    let loaded = saved(&mut host);
    assert!(host.settings_serialized_bound_ok(&loaded));
}

/// REQ: PORT:ilium-client/src/remote_compaction_app.rs::keyboard_closes_the_banner_for_good_and_ignores_stray_arrows
#[test]
fn rapp_keyboard_dismissal_moves_selection_normally_afterwards() {
    let mut host = tab();
    host.settings_key("x");
    host.settle();
    assert!(!host.settings_banner_visible());
    host.settings_key("Down");
    assert_eq!(host.settings_selected_row(), 1);
}

/// REQ: PORT:ilium-client/src/remote_compaction_app.rs::keyboard_walks_the_rows_toggles_and_cycles_each_agent_technique
#[test]
fn rapp_escape_closes_the_technique_catalogue_without_changing_the_technique() {
    let mut host = dismissed_tab();
    let start = host.settings_value("technique:claude");
    while host.settings_selected_row() < host.settings_row_index("technique:claude") {
        host.settings_key("Down");
    }
    host.settings_key("Enter");
    assert!(host.settings_dialog().is_some());
    host.settings_dialog_key("Esc");
    host.settle();
    assert!(host.settings_dialog().is_none());
    assert_eq!(host.settings_value("technique:claude"), start);
}

/// REQ: PORT:ilium-client/src/remote_compaction_app.rs::keyboard_enter_on_the_prompt_row_opens_the_editor_and_delete_clears_the_text
#[test]
fn rapp_delete_on_the_prompt_row_does_not_open_the_editor() {
    let mut host = dismissed_tab();
    host.settings_commit_prompt("Keep API names.");
    host.settle();
    while host.settings_selected_row() < host.settings_row_index("custom_prompt") {
        host.settings_key("Down");
    }
    host.settings_key("Delete");
    host.settle();
    assert!(host.prompt_editor_text().is_none());
}

/// REQ: PORT:ilium-client/src/remote_compaction_app.rs::mouse_click_on_the_x_closes_the_banner_for_good_and_other_clicks_do_not
#[test]
fn rapp_clicking_the_x_removes_its_close_target() {
    let mut host = tab();
    assert!(host.settings_banner_close_rect().is_some());
    let banner = host.settings_row_geometry("privacy_banner");
    host.settings_click(banner.control_column + 1, banner.control_line);
    host.settle();
    assert!(host.settings_banner_close_rect().is_none());
}

/// REQ: PORT:ilium-client/src/remote_compaction_app.rs::mouse_toggles_steps_and_cycles_technique_rows_from_the_control_halves
#[test]
fn rapp_clicking_the_technique_value_cycles_through_the_whole_catalogue() {
    let mut host = dismissed_tab();
    let start = host.settings_value("technique:claude");
    let count = host.technique_ids().len();
    for _ in 0..count {
        let claude = host.settings_row_geometry("technique:claude");
        host.settings_click(claude.value_column, claude.control_line);
    }
    assert_eq!(
        host.settings_value("technique:claude"),
        start,
        "{count} clicks complete one cycle"
    );
}

/// REQ: PORT:ilium-client/src/remote_compaction_app.rs::numeric_star_accepts_direct_entry_and_persists_remote_compaction_value
#[test]
fn rapp_escape_in_direct_numeric_entry_keeps_the_threshold() {
    let mut host = dismissed_tab();
    let before = host.settings_value("threshold");
    let threshold = host.settings_row_geometry("threshold");
    host.settings_click(threshold.open_column, threshold.control_line);
    assert!(host.settings_dialog().is_some());
    host.settings_dialog_key("Esc");
    host.settle();
    assert!(host.settings_dialog().is_none());
    assert_eq!(host.settings_value("threshold"), before);
}

/// REQ: PORT:ilium-client/src/remote_compaction_app.rs::technique_plus_opens_full_catalog_and_pointer_selection_persists
#[test]
fn rapp_technique_catalogue_preselects_the_current_technique() {
    let mut host = dismissed_tab();
    let all = host.technique_ids();
    let current = all.last().expect("catalogue is not empty").clone();
    host.settings_set_value("technique:claude", json!(current.clone()));
    let claude = host.settings_row_geometry("technique:claude");
    host.settings_click(claude.open_column, claude.control_line);
    let dialog = host.settings_dialog().expect("catalogue opens");
    assert_eq!(dialog["selected_id"], json!(current));
}

/// REQ: PORT:ilium-client/src/remote_compaction_app.rs::the_prompt_row_opens_the_editor_on_the_stored_text
#[test]
fn rapp_escape_from_the_prompt_editor_keeps_the_stored_prompt() {
    let mut host = tab();
    host.settings_commit_prompt("Line one\nLine two");
    host.settle();
    host.settings_adjust_row("custom_prompt", 0);
    host.settings_key("Esc");
    host.settle();
    assert!(host.prompt_editor_text().is_none());
    assert_eq!(saved(&mut host)["custom_prompt"], "Line one\nLine two");
}

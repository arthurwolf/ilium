//! Rules stated by the page tables and prose, parsed from the page: toolbar action table, settings sentence, traceability rows.

use crate::common::{page_text, workspace_root};

const ACTION_IDS: [&str; 12] = [
    "compact", "clear", "configure", "stop", "copy-screen", "smart-copy", "copy-last-message", "effort", "model", "fast-mode",
    "exit", "text-selection",
];

/// Rows of the toolbar action table: `[id, label, what, agents, route]`, backticks removed.
fn action_rows() -> Vec<[String; 5]> {
    let page = page_text();
    let start = page.find("| Action id | Label |").expect("toolbar action table");
    page[start..]
        .lines()
        .skip(2)
        .take_while(|line| line.starts_with('|'))
        .map(|line| {
            let cells: Vec<String> = line.trim_matches('|').split('|').map(|cell| cell.trim().replace('`', "")).collect();
            [cells[0].clone(), cells[1].clone(), cells[2].clone(), cells[3].clone(), cells[4].clone()]
        })
        .collect()
}

fn row(id: &str) -> [String; 5] {
    action_rows().into_iter().find(|row| row[0] == id).unwrap_or_else(|| panic!("row {id}"))
}

fn traceability_row(feature: &str) -> Vec<String> {
    let path = workspace_root().join("docs/plugins-manual/pages/reference-feature-traceability.md");
    let text = std::fs::read_to_string(path).expect("traceability page");
    let line = text.lines().find(|line| line.starts_with(&format!("| {feature} |"))).unwrap_or_else(|| panic!("row {feature}"));
    line.trim_matches('|').split('|').map(|cell| cell.trim().to_string()).collect()
}

/// REQ: F-035, COR-011
#[test]
fn doc_f_035_the_action_table_has_the_twelve_actions_in_order() {
    let ids: Vec<String> = action_rows().into_iter().map(|row| row[0].clone()).collect();
    assert_eq!(ids, ACTION_IDS);
}

/// REQ: F-035, COR-011
#[test]
fn doc_f_035_the_action_labels_are_as_documented() {
    let labels: Vec<String> = action_rows().into_iter().map(|row| row[1].clone()).collect();
    assert_eq!(
        labels,
        ["Compact", "Clear", "Configure", "Stop", "Copy screen", "Smart Copy", "Copy last message", "Effort", "Model", "Fast mode", "Exit", "Text selection"]
    );
}

/// REQ: F-035, COR-011
#[test]
fn doc_f_035_actions_for_all_agents_are_stop_copy_screen_smart_copy_exit_and_text_selection() {
    let all: Vec<String> = action_rows().into_iter().filter(|row| row[3] == "all agents").map(|row| row[0].clone()).collect();
    assert_eq!(all, ["stop", "copy-screen", "smart-copy", "exit", "text-selection"]);
}

/// REQ: F-035, COR-011
#[test]
fn doc_f_035_claude_and_codex_only_actions_are_listed() {
    let provider: Vec<String> =
        action_rows().into_iter().filter(|row| row[3] == "Claude Code, Codex").map(|row| row[0].clone()).collect();
    assert_eq!(provider, ["compact", "clear", "configure", "copy-last-message", "effort", "model"]);
}

/// REQ: F-035, COR-011
#[test]
fn doc_f_035_fast_mode_exists_only_for_agents_that_have_it() {
    assert_eq!(row("fast-mode")[3], "agents that have it");
}

/// REQ: F-081
#[test]
fn doc_f_081_compact_sends_slash_compact_or_starts_remote_compaction() {
    let compact = row("compact");
    assert!(compact[4].starts_with("Sends /compact, or starts remote compaction when the"), "{}", compact[4]);
    assert!(compact[4].contains("set to remote"));
}

/// REQ: F-035
#[test]
fn doc_f_035_clear_sends_slash_clear_and_stop_sends_the_interrupt_key() {
    assert_eq!(row("clear")[4], "Sends /clear");
    assert_eq!(row("stop")[4], "Sends the interrupt key");
}

/// REQ: F-035
#[test]
fn doc_f_035_configure_and_smart_copy_call_services_of_other_plugins() {
    assert!(row("configure")[4].starts_with("Calls the [agent setup]"));
    assert!(row("smart-copy")[4].starts_with("Calls the [smart copy]"));
}

/// REQ: F-035
#[test]
fn doc_f_035_copy_actions_read_the_screen_or_the_transcript() {
    assert_eq!(row("copy-screen")[4], "Reads the screen");
    assert_eq!(row("copy-last-message")[4], "Reads the transcript");
}

/// REQ: F-035
#[test]
fn doc_f_035_text_selection_toggles_the_terminal_selection_mode() {
    assert_eq!(row("text-selection")[4], "Toggles the terminal selection mode");
}

/// REQ: F-035
#[test]
fn doc_f_035_effort_and_model_send_the_agents_own_command() {
    assert_eq!(row("effort")[4], "Sends the agent's own command");
    assert_eq!(row("model")[4], "Sends the agent's own command");
    assert_eq!(row("fast-mode")[4], "Sends the agent's own command");
}

/// REQ: F-035
#[test]
fn doc_f_035_the_settings_sentence_names_the_four_keys_and_the_range() {
    let page = page_text();
    assert!(page.contains("The toolbar is shown when `agent_toolbar_enabled` is on (default)."));
    assert!(page.contains("`show_toolbar_labels` adds text after the icons."));
    assert!(page.contains("`last_prompt_enabled` and its line count `last_prompt_max_lines` (1 to 20, default 4)"));
}

/// REQ: F-033
#[test]
fn doc_f_033_the_page_samples_use_the_banner_limit_of_120_characters() {
    assert!(page_text().contains("const BANNER_CHARS: usize = 120;"));
}

/// REQ: F-033
#[test]
fn doc_f_033_the_traceability_row_is_owned_by_this_plugin_on_both_sides() {
    let row = traceability_row("F-033");
    assert_eq!(row[1], "Last-prompt banner");
    assert_eq!(row[4], "agent-toolbar plugin");
    assert_eq!(row[5], "both");
    assert_eq!(row[7], "[ui] last_prompt_enabled, last_prompt_max_lines");
}

/// REQ: F-035
#[test]
fn doc_f_035_the_traceability_row_is_owned_by_this_plugin_on_both_sides() {
    let row = traceability_row("F-035");
    assert_eq!(row[1], "Agent toolbar");
    assert_eq!(row[4], "agent-toolbar plugin");
    assert_eq!(row[5], "both");
    assert_eq!(row[7], "[ui] agent_toolbar_enabled, show_toolbar_labels");
}

/// REQ: F-081
#[test]
fn doc_f_081_the_traceability_row_is_owned_by_this_plugin_on_the_client() {
    let row = traceability_row("F-081");
    assert_eq!(row[1], "Compact button routing");
    assert_eq!(row[4], "agent-toolbar plugin");
    assert_eq!(row[5], "client");
    assert_eq!(row[7], "[remote_compaction]");
}

/// REQ: F-033, F-035, F-081
#[test]
fn doc_f_035_the_page_lists_the_five_covered_areas() {
    let page = page_text();
    for item in [
        "The last-prompt banner and the agent toolbar",
        "Configure agent",
        "Reserved terminal space",
        "Recover work after an agent stops",
        "Agent debug history",
    ] {
        assert!(page.contains(item), "{item}");
    }
}

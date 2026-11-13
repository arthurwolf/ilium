//! The page is the specification: its figures, the traceability row of F-077 and the statements the tests rely on.

use crate::common::*;

fn field_after(text: &str, marker: &str) -> String {
    let at = text
        .find(marker)
        .unwrap_or_else(|| panic!("{marker} missing"));
    text[at..].lines().next().unwrap().to_string()
}

/// REQ: F-077
#[test]
fn page_077_has_two_terminal_figures_the_progress_dialog_and_the_failure() {
    let blocks = term_blocks(&page_text());
    assert_eq!(blocks.len(), 2, "{blocks:?}");
    assert!(blocks[0].contains("Convert Claude Code to Codex"));
    assert!(blocks[1].contains("Conversion failed"));
}

/// REQ: F-077
#[test]
fn page_077_figures_are_48_columns_wide_on_every_row() {
    for block in term_blocks(&page_text()) {
        for row in block.lines().filter(|row| !row.is_empty()) {
            assert_eq!(row.chars().count(), 48, "{row}");
        }
    }
}

/// REQ: F-077
#[test]
fn page_077_golden_files_equal_the_figures() {
    let blocks = term_blocks(&page_text());
    for (name, block) in [
        ("dialog_progress_48.txt", &blocks[0]),
        ("dialog_failed_48.txt", &blocks[1]),
    ] {
        let golden_text = std::fs::read_to_string(golden(name)).expect("golden");
        assert_eq!(golden_text.trim_end(), block.trim_end(), "{name}");
    }
}

/// REQ: F-077
#[test]
fn page_077_the_traceability_row_names_the_session_convert_plugin() {
    let path = workspace_root().join("docs/plugins-manual/pages/reference-feature-traceability.md");
    let text = std::fs::read_to_string(path).expect("traceability page");
    let row = field_after(&text, "| F-077 |");
    assert!(row.contains("Convert a session between Claude and Codex"));
    assert!(row.contains("session-convert plugin"));
    assert!(row.contains("COR-011"));
}

/// REQ: F-077
#[test]
fn page_077_states_the_plugin_has_no_settings() {
    let page = page_text();
    assert!(
        !page.contains("\"settings\": ["),
        "the complete manifest has no settings"
    );
}

/// REQ: F-077
#[test]
fn page_077_mentions_both_conversion_directions() {
    let page = page_text();
    assert!(page.contains("Claude Code to Codex") && page.contains("Codex to Claude Code"));
}

/// REQ: F-077
#[test]
fn page_077_names_the_ten_minute_watchdog() {
    assert!(page_text().contains("in ten minutes"));
}

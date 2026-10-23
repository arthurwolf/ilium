//! The page is the specification: its figures, the traceability row of F-077 and the statements the tests rely on.

use crate::common::*;

fn field_after(text: &str, marker: &str) -> String {
    let at = text
        .find(marker)
        .unwrap_or_else(|| panic!("{marker} missing"));
    text[at..].lines().next().unwrap().to_string()
}

/// The figure whose text contains `marker`; the page has several figures, so they are found by content.
fn figure_containing<'a>(blocks: &'a [String], marker: &str) -> &'a String {
    blocks
        .iter()
        .find(|block| block.contains(marker))
        .unwrap_or_else(|| panic!("no figure contains {marker:?}"))
}

/// REQ: F-077
#[test]
fn page_077_has_three_terminal_figures_including_the_progress_and_failure_dialogs() {
    // The page also shows the right-click menu figure before the progress dialog and the failure.
    let blocks = term_blocks(&page_text());
    assert_eq!(blocks.len(), 3, "{blocks:?}");
    assert!(
        figure_containing(&blocks, "Convert Claude Code to Codex").contains("Import the session")
    );
    assert!(figure_containing(&blocks, "Conversion failed")
        .contains("The original session is untouched."));
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
        (
            "dialog_progress_48.txt",
            figure_containing(&blocks, "Convert Claude Code to Codex"),
        ),
        (
            "dialog_failed_48.txt",
            figure_containing(&blocks, "Conversion failed"),
        ),
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

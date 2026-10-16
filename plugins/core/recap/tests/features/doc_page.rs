//! The renders and statements of the page, and the user documentation of `ilium recap` (F-112).

use serde_json::Value;

use crate::common::*;

fn cli_doc() -> String {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../src/docs/cli-reference.md");
    std::fs::read_to_string(path).expect("cli-reference.md is readable")
}

fn width(line: &str) -> usize {
    line.chars().count()
}

/// REQ: F-112
#[test]
fn doc_f_112_page_has_three_illustrative_renders() {
    assert_eq!(term_blocks(&page_text()).len(), 3);
}

/// REQ: F-112
#[test]
fn doc_f_112_cli_render_shows_recap_warning_and_summary_records_in_that_order() {
    let blocks = term_blocks(&page_text());
    let text = &blocks[0];
    let recap = text.find("\"type\":\"recap\"").expect("recap record");
    let warning = text.find("\"type\":\"warning\"").expect("warning record");
    let summary = text.find("\"type\":\"summary\"").expect("summary record");
    assert!(recap < warning && warning < summary);
    assert!(text.contains("\"state\":\"summarized\""));
    assert!(text.contains("\"goals\":[\"Goal active: Finish the parser refactor\"]"));
    assert!(text.contains("\"cancelled\":false"));
}

/// REQ: F-112
#[test]
fn doc_f_112_extensions_tab_render_fits_48_columns_and_has_the_stopped_label() {
    let blocks = term_blocks(&page_text());
    let text = &blocks[1];
    assert!(text.lines().all(|line| width(line) <= 48), "a line exceeds 48 columns:\n{text}");
    assert!(text.contains("stopped after 3 timeouts"));
    assert!(text.contains("[Reset]"));
    assert!(text.contains("Run order: lower priority number first."));
    assert!(text.contains("fact-sources (collect)"));
    assert!(text.contains("before-recap (transform)"));
}

/// REQ: F-112
#[test]
fn doc_f_112_consent_panel_render_has_notice_and_two_leader_rows() {
    let blocks = term_blocks(&page_text());
    let text = &blocks[2];
    assert!(text.lines().all(|line| width(line) <= 48), "a line exceeds 48 columns:\n{text}");
    assert!(text.contains("! AI is off for recaps"));
    assert!(text.contains("Every selected pane gets a denied record."));
    assert!(text.contains("Turn on automatic AI in Settings."));
    let leaders: Vec<&str> = text.lines().filter(|line| line.contains('\u{2026}')).collect();
    assert_eq!(leaders.len(), 2);
    assert!(leaders[0].contains("Last run"));
    assert!(leaders[1].contains("Panes recapped"));
}

/// REQ: F-112
#[test]
fn doc_f_112_leader_rows_keep_one_blank_cell_on_each_side_of_the_dots() {
    let blocks = term_blocks(&page_text());
    for line in blocks[2].lines().filter(|line| line.contains('\u{2026}')) {
        let first = line.find('\u{2026}').expect("dots");
        let last = line.rfind('\u{2026}').expect("dots");
        assert_eq!(line[..first].chars().last(), Some(' '));
        assert_eq!(line[last + '\u{2026}'.len_utf8()..].chars().next(), Some(' '));
    }
}

/// REQ: F-112
#[test]
fn doc_f_112_page_states_the_word_limits_of_both_lengths() {
    let page = page_text();
    assert!(page.contains("short form (at most 40 words)"));
    assert!(page.contains("long form (at most 250 words)"));
}

/// REQ: F-112
#[test]
fn doc_f_112_page_lists_the_six_record_states() {
    assert!(page_text().contains("`summarized`, `idle`, `denied`, `unavailable`, `stale` and `failed`"));
}

/// REQ: F-112
#[test]
fn doc_f_112_user_doc_lists_the_options_with_their_ranges() {
    let doc = cli_doc();
    assert!(doc.contains("| `--length short\\|long\\|both` |"));
    assert!(doc.contains("| `--format jsonl\\|markdown` |"));
    assert!(doc.contains("Limit for the whole run, 1 to 600. Default 60."));
    assert!(doc.contains("Panes recapped at the same time, 1 to 8. Default 4."));
    assert!(doc.contains("Most panes recapped in one run, 1 to 64. Default 32."));
    assert!(doc.contains("| `--include-self` |"));
}

/// REQ: F-112
#[test]
fn doc_f_112_user_doc_sample_records_are_valid_json_with_the_documented_keys() {
    let doc = cli_doc();
    let start = doc.find("{\"type\":\"recap\",\"pane_id\":\"12\"").expect("sample");
    let samples: Vec<Value> =
        doc[start..].lines().take(3).map(|line| serde_json::from_str(line).expect("valid JSON")).collect();
    let keys: Vec<&String> = samples[0].as_object().expect("object").keys().collect();
    for key in [
        "type", "pane_id", "pane_name", "state", "goals", "evidence_age_seconds", "evidence_stale",
        "window_truncated", "short", "long", "failure",
    ] {
        assert!(keys.iter().any(|candidate| candidate.as_str() == key), "missing key {key}");
    }
    assert_eq!(samples[0]["pane_id"], "12", "pane_id is a string");
    assert!(samples[0]["long"].is_null());
    assert_eq!(samples[1]["type"], "warning");
    assert_eq!(samples[2]["type"], "summary");
}

/// REQ: F-112
#[test]
fn doc_f_112_summary_sample_counts_every_state_and_the_cancelled_flag() {
    let doc = cli_doc();
    let line = doc.lines().find(|line| line.starts_with("{\"type\":\"summary\",\"panes\":1")).expect("summary sample");
    let record: Value = serde_json::from_str(line).expect("JSON");
    for key in ["panes", "summarized", "idle", "denied", "unavailable", "stale", "failed"] {
        assert!(record[key].is_u64(), "count {key}");
    }
    assert_eq!(record["cancelled"], false);
}

/// REQ: F-112
#[test]
fn doc_f_112_user_doc_exit_codes_are_0_for_finished_runs_2_for_selection_1_for_configuration() {
    let doc = cli_doc();
    assert!(doc.contains("Exit status is `0` when the run finished, including with failed or stale panes"));
    assert!(doc.contains("Exit `2` means an invalid selection, and exit `1` means the configuration could not be read."));
}

/// REQ: F-112
#[test]
fn doc_f_112_help_topic_file_of_the_manifest_is_named_recap_privacy() {
    let manifest = manifest_value();
    assert_eq!(manifest["contributes"]["help"][0]["id"], "recap-privacy");
    assert_eq!(manifest["contributes"]["help"][0]["file"], "help/recap-privacy.md");
}

//! reference-test-harness.md "Snapshot and golden files": DEV-500 to DEV-504.

use ilium_plugin_testkit::errors::codes;
use ilium_plugin_testkit::golden::{assert_golden, assert_snapshot};

use crate::common::{assert_fails_with, panic_text, temp_dir, with_env, write_file};

fn path_text(path: &std::path::Path) -> String {
    path.to_str().expect("utf-8 temp path").to_string()
}

/// REQ: DEV-500
#[test]
fn dev_500_equal_text_passes() {
    with_env(&[("ILIUM_PLUGIN_UPDATE_GOLDEN", ""), ("CI", "")], || {
        let file = write_file(&temp_dir("g500a"), "a.txt", "line one\nline two\n");
        assert_golden(&path_text(&file), "line one\nline two\n");
    });
}

/// REQ: DEV-500
#[test]
fn dev_500_crlf_in_the_actual_text_is_converted_before_comparing() {
    with_env(&[("ILIUM_PLUGIN_UPDATE_GOLDEN", ""), ("CI", "")], || {
        let file = write_file(&temp_dir("g500b"), "a.txt", "line one\nline two\n");
        assert_golden(&path_text(&file), "line one\r\nline two\r\n");
    });
}

/// REQ: DEV-500
#[test]
fn dev_500_crlf_in_the_golden_file_is_converted_before_comparing() {
    with_env(&[("ILIUM_PLUGIN_UPDATE_GOLDEN", ""), ("CI", "")], || {
        let file = write_file(&temp_dir("g500c"), "a.txt", "x\r\ny\r\n");
        assert_golden(&path_text(&file), "x\ny\n");
    });
}

/// REQ: DEV-500
#[test]
fn dev_500_different_text_fails_with_a_unified_diff() {
    with_env(&[("ILIUM_PLUGIN_UPDATE_GOLDEN", ""), ("CI", "")], || {
        let file = write_file(&temp_dir("g500d"), "a.txt", "same\nold\n");
        let text = panic_text(|| assert_golden(&path_text(&file), "same\nnew\n")).expect("must fail");
        assert!(text.contains("-old"), "{text}");
        assert!(text.contains("+new"), "{text}");
    });
}

/// REQ: DEV-500
#[test]
fn dev_500_the_diff_shows_at_most_forty_changed_lines() {
    with_env(&[("ILIUM_PLUGIN_UPDATE_GOLDEN", ""), ("CI", "")], || {
        let old: String = (0..100).map(|n| format!("old {n}\n")).collect();
        let new: String = (0..100).map(|n| format!("new {n}\n")).collect();
        let file = write_file(&temp_dir("g500e"), "a.txt", &old);
        let text = panic_text(|| assert_golden(&path_text(&file), &new)).expect("must fail");
        let removed = text.lines().filter(|line| line.starts_with("-old")).count();
        let added = text.lines().filter(|line| line.starts_with("+new")).count();
        assert_eq!(removed + added, 40, "{text}");
    });
}

/// REQ: DEV-500
#[test]
fn dev_500_a_diff_of_exactly_forty_changed_lines_is_shown_whole() {
    with_env(&[("ILIUM_PLUGIN_UPDATE_GOLDEN", ""), ("CI", "")], || {
        let old: String = (0..20).map(|n| format!("old {n}\n")).collect();
        let new: String = (0..20).map(|n| format!("new {n}\n")).collect();
        let file = write_file(&temp_dir("g500f"), "a.txt", &old);
        let text = panic_text(|| assert_golden(&path_text(&file), &new)).expect("must fail");
        assert_eq!(text.lines().filter(|l| l.starts_with("-old") || l.starts_with("+new")).count(), 40);
    });
}

/// REQ: DEV-501
#[test]
fn dev_501_update_mode_writes_the_actual_text_and_passes() {
    with_env(&[("ILIUM_PLUGIN_UPDATE_GOLDEN", "1"), ("CI", "")], || {
        let file = write_file(&temp_dir("g501a"), "a.txt", "old\n");
        assert_golden(&path_text(&file), "fresh\n");
        assert_eq!(std::fs::read_to_string(&file).expect("read back"), "fresh\n");
    });
}

/// REQ: DEV-501, DEV-502
#[test]
fn dev_501_update_mode_creates_a_missing_golden_file() {
    with_env(&[("ILIUM_PLUGIN_UPDATE_GOLDEN", "1"), ("CI", "")], || {
        let file = temp_dir("g501b").join("created.txt");
        assert_golden(&path_text(&file), "new file\n");
        assert_eq!(std::fs::read_to_string(&file).expect("created"), "new file\n");
    });
}

/// REQ: DEV-501
#[test]
fn dev_501_a_value_other_than_one_does_not_enable_update_mode() {
    with_env(&[("ILIUM_PLUGIN_UPDATE_GOLDEN", "0"), ("CI", "")], || {
        let file = write_file(&temp_dir("g501c"), "a.txt", "old\n");
        assert!(panic_text(|| assert_golden(&path_text(&file), "fresh\n")).is_some());
        assert_eq!(std::fs::read_to_string(&file).expect("unchanged"), "old\n");
    });
}

/// REQ: DEV-502
#[test]
fn dev_502_a_missing_golden_file_fails_with_golden_missing() {
    with_env(&[("ILIUM_PLUGIN_UPDATE_GOLDEN", ""), ("CI", "")], || {
        let file = temp_dir("g502").join("absent.txt");
        assert_fails_with(codes::GOLDEN_MISSING, || assert_golden(&path_text(&file), "x\n"));
        assert!(!file.exists(), "a failed comparison writes nothing");
    });
}

/// REQ: DEV-503
#[test]
fn dev_503_update_mode_in_ci_fails_with_golden_update_in_ci() {
    with_env(&[("ILIUM_PLUGIN_UPDATE_GOLDEN", "1"), ("CI", "true")], || {
        let file = write_file(&temp_dir("g503a"), "a.txt", "old\n");
        assert_fails_with(codes::GOLDEN_UPDATE_IN_CI, || assert_golden(&path_text(&file), "fresh\n"));
        assert_eq!(std::fs::read_to_string(&file).expect("unchanged"), "old\n");
    });
}

/// REQ: DEV-503
#[test]
fn dev_503_an_empty_ci_variable_does_not_block_updates() {
    with_env(&[("ILIUM_PLUGIN_UPDATE_GOLDEN", "1"), ("CI", "")], || {
        let file = write_file(&temp_dir("g503b"), "a.txt", "old\n");
        assert_golden(&path_text(&file), "fresh\n");
        assert_eq!(std::fs::read_to_string(&file).expect("updated"), "fresh\n");
    });
}

/// REQ: DEV-504
#[test]
fn dev_504_update_mode_writes_a_snapshot_as_pretty_sorted_json_with_a_trailing_newline() {
    with_env(&[("ILIUM_PLUGIN_UPDATE_GOLDEN", "1"), ("CI", "")], || {
        let name = "sdk_fakehost_504_sorted";
        assert_snapshot(name, &serde_json::json!({ "b": 1, "a": { "z": true, "y": [1, 2] } }));
        let stored = std::fs::read_to_string(format!("tests/snapshots/{name}.json")).expect("snapshot file");
        assert_eq!(stored, "{\n  \"a\": {\n    \"y\": [\n      1,\n      2\n    ],\n    \"z\": true\n  },\n  \"b\": 1\n}\n");
    });
}

/// REQ: DEV-504
#[test]
fn dev_504_a_snapshot_compares_equal_to_its_stored_file() {
    let name = "sdk_fakehost_504_equal";
    with_env(&[("ILIUM_PLUGIN_UPDATE_GOLDEN", "1"), ("CI", "")], || {
        assert_snapshot(name, &serde_json::json!({ "k": "v" }));
    });
    with_env(&[("ILIUM_PLUGIN_UPDATE_GOLDEN", ""), ("CI", "")], || {
        assert_snapshot(name, &serde_json::json!({ "k": "v" }));
        assert!(panic_text(|| assert_snapshot(name, &serde_json::json!({ "k": "w" }))).is_some());
    });
}

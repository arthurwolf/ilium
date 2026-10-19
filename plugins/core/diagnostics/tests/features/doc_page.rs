//! Doc-lint tests: statements of the page that no runtime channel shows are parsed from the page text.

use crate::common::{page, page_text, table_row};

/// REQ: COR-050
#[test]
fn doc_the_plugin_id_and_both_sides_are_stated_in_the_introduction() {
    let text = page_text();
    assert!(text.contains("The plugin id is `ilium.core.diagnostics` and it runs on both sides."));
    assert!(text.contains("The server side reads the log file and builds the report. The client side shows the report and the warning list."));
}

/// REQ: COR-050
#[test]
fn doc_the_log_file_naming_and_location_rules_are_stated() {
    let text = page_text();
    assert!(text.contains("named `log-<local date and time>.txt`"));
    assert!(text.contains("`/tmp/.ilium-<uid>/logs/<session-id>/` on Linux and macOS"));
    assert!(text.contains("`%LOCALAPPDATA%\\ilium\\logs` on Windows"));
    assert!(text.contains("`ILIUM_DEBUG_LOG_DIR` replaces the root."));
}

/// REQ: COR-050
#[test]
fn doc_the_five_newest_logs_of_a_session_are_kept() {
    assert!(page_text().contains("The five newest logs of a session are kept and older ones are removed when the server starts."));
}

/// REQ: COR-050
#[test]
fn doc_file_logging_is_off_by_default_and_the_log_is_sensitive() {
    let text = page_text();
    assert!(text.contains("File logging is off by default."));
    assert!(text.contains("Credential headers and URL parameters are redacted, but other text is not. Treat a log as sensitive."));
}

/// REQ: COR-050
#[test]
fn doc_the_five_log_levels_are_listed_in_order() {
    assert!(page_text().contains("Log levels: trace, debug, info, warn, error."));
}

/// REQ: COR-050
#[test]
fn doc_the_report_holds_at_most_64_kibibytes_for_each_section() {
    assert!(page_text().contains(
        "one section for each contributor, at most 64 KiB (65,536 bytes) for each section"
    ));
}

/// REQ: COR-050
#[test]
fn doc_the_debug_and_about_tabs_belong_to_the_settings_plugin() {
    let text = page_text();
    assert!(text.contains(
        "The Debug and About tabs, and the file logging switch, belong to the Settings plugin."
    ));
    assert!(text.contains("It reads the Debug setting without writing it"));
}

/// REQ: COR-050
#[test]
fn doc_the_log_folder_grant_is_a_literal_absolute_path_with_high_risk() {
    let text = page_text();
    assert!(text.contains(
        "The log folder lives outside the plugin scopes, so reading it needs a literal path grant."
    ));
    assert!(text.contains("A literal absolute path is high risk and needs an explicit user grant."));
    assert!(text.contains(
        "The example uses user id 1000, and the real grant is written at install from the user id."
    ));
}

/// REQ: COR-050
#[test]
fn doc_the_renders_are_marked_illustrative_and_state_their_width() {
    let text = page_text();
    assert!(text.contains(
        "There is no recording of this feature yet. The renders below are illustrative."
    ));
    assert_eq!(
        text.matches("Rendered at 48 columns. Illustrative.")
            .count(),
        2
    );
    assert!(text.contains("Rendered at 80 columns or fewer. Illustrative."));
}

/// REQ: COR-050
#[test]
fn doc_the_replacement_rule_names_the_report_service_and_both_points() {
    let text = page_text();
    assert!(text.contains("A replacement must provide the report service and both points."));
    assert!(text.contains("A replacement lists `\"replaces\": [\"ilium.core.diagnostics\"]` and provides the whole surface"));
}

/// REQ: COR-050
#[test]
fn doc_the_diary_is_separate_from_the_ilium_log_file_and_limited_to_200_lines_per_second() {
    let text = page_text();
    assert!(text.contains("at most 200 lines for each second"));
    assert!(text.contains("It is separate from the Ilium log file."));
    assert!(
        text.contains("No import reads the diary back, so the plugin keeps what it needs itself.")
    );
}

/// REQ: COR-050
#[test]
fn doc_the_report_service_is_deferred_and_cut_at_256_kibibytes() {
    let text = page_text();
    assert!(text.contains("The call is deferred, so the plugin answers when the sections are in."));
    assert!(text.contains("The result is cut at 256 KiB."));
}

/// REQ: COR-050
#[test]
fn doc_a_slow_or_failing_section_is_named_and_skipped() {
    assert!(page_text().contains("A section that is slow or fails is named and skipped, so one bad contributor cannot stop the report."));
}

/// REQ: COR-050
#[test]
fn doc_the_attach_dialog_warns_first_and_defaults_to_cancel() {
    let text = page_text();
    assert!(text.contains("A dialog warns that logs hold private text and shows the size."));
    assert!(text.contains(
        "Only after the user confirms does the plugin copy the log to the place the user chose."
    ));
    assert!(text.contains("The default button is Cancel."));
    assert!(text.contains("The copy is a plain read and write, so it keeps the original."));
}

/// REQ: COR-050
#[test]
fn doc_the_core_points_table_counts_two_points_one_service_no_topic_and_replaceable() {
    let core = page("reference-core-extension-points.md");
    let row = table_row(&core, "| [Diagnostics](/pages/core-plugin-diagnostics/)");
    assert_eq!(&row[2..], ["2", "1", "0", "yes"]);
}

/// REQ: COR-050
#[test]
fn doc_every_link_target_of_the_see_also_section_is_a_page_that_exists() {
    for name in [
        "core-plugins.md",
        "core-plugin-settings.md",
        "settings-screens.md",
    ] {
        let text = page(name);
        assert!(!text.is_empty(), "{name}");
    }
}

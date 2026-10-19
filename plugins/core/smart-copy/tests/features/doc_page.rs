//! Rules that can only be checked by reading the pages: the extension point table, the topic,
//! the risk marks (SEC-293) and the replaceable flag (EXT-042).

use crate::common::{manual_page, page_text};

/// Table rows (cells trimmed) of the first table after the line starting with `anchor`.
fn table_after(text: &str, anchor: &str) -> Vec<Vec<String>> {
    let mut lines = text.lines().skip_while(|line| !line.starts_with(anchor));
    lines.next().expect("anchor present");
    lines
        .skip_while(|line| !line.starts_with('|'))
        .take_while(|line| line.starts_with('|'))
        .skip(2)
        .map(|line| {
            line.trim_matches('|')
                .split('|')
                .map(|cell| cell.trim().to_string())
                .collect()
        })
        .collect()
}

/// REQ: EXT-042
#[test]
fn extension_042_page_lists_exactly_two_points_recognisers_and_after_copy() {
    let rows = table_after(&page_text(), "### Extension points");
    let names: Vec<&str> = rows.iter().map(|row| row[0].as_str()).collect();
    assert_eq!(names, ["`recognisers`", "`after-copy`"]);
}

/// REQ: EXT-042
#[test]
fn extension_042_recognisers_is_a_collect_point_with_the_entry_fields_of_the_page() {
    let rows = table_after(&page_text(), "### Extension points");
    assert_eq!(rows[0][1], "collect");
    assert_eq!(rows[0][3], "entry: id, kind, regex, multiline, label");
}

/// REQ: EXT-042
#[test]
fn extension_042_after_copy_is_a_transform_hook_over_text_kind_and_pane() {
    let rows = table_after(&page_text(), "### Extension points");
    assert_eq!(rows[1][1], "hook (transform)");
    assert_eq!(rows[1][3], "text, kind, pane; reply: text");
}

/// REQ: EXT-042
#[test]
fn extension_042_the_page_states_the_plugin_is_replaceable() {
    assert!(page_text().contains("**Replaceable:** yes."));
}

/// REQ: EXT-042
#[test]
fn extension_042_summary_row_counts_two_points_no_service_one_topic_and_replaceable() {
    let summary = manual_page("reference-core-extension-points.md");
    let row = summary
        .lines()
        .find(|line| line.contains("`ilium.core.smart-copy` |"))
        .expect("summary row");
    let cells: Vec<&str> = row.trim_matches('|').split('|').map(str::trim).collect();
    assert_eq!(&cells[2..], ["2", "0", "1", "yes"]);
}

/// REQ: EXT-042
#[test]
fn extension_042_core_points_page_repeats_both_points_and_the_topic() {
    let core = manual_page("reference-core-extension-points.md");
    assert!(core.contains("| `ilium.core.smart-copy/recognisers` | collect |"));
    assert!(core.contains("| `ilium.core.smart-copy/after-copy` | hook: transform |"));
    assert!(
        core.contains("| `ilium.core.smart-copy/copied` | { kind, length } after text is copied |")
    );
}

/// REQ: EXT-042
#[test]
fn extension_042_the_topic_is_named_with_the_owner_prefix_and_carries_kind_and_length() {
    let page = page_text();
    assert!(page.contains("`ilium.core.smart-copy/copied`: { kind, length } after text is copied"));
}

/// REQ: SEC-293
#[test]
fn security_293_page_risk_of_after_copy_equals_the_security_table() {
    let security = manual_page("reference-extension-security.md");
    let row = security
        .lines()
        .find(|line| line.starts_with("| `ilium.core.smart-copy/after-copy` |"))
        .expect("security row");
    let expected = row
        .trim_matches('|')
        .split('|')
        .nth(1)
        .expect("risk cell")
        .trim()
        .to_string();
    let rows = table_after(&page_text(), "### Extension points");
    assert_eq!(
        rows[1][4], expected,
        "the corrected mark of after-copy is {expected}"
    );
}

/// REQ: SEC-293
#[test]
fn security_293_core_points_table_risk_of_after_copy_equals_the_security_table() {
    let security = manual_page("reference-extension-security.md");
    let security_mark = security
        .lines()
        .find(|line| line.starts_with("| `ilium.core.smart-copy/after-copy` |"))
        .and_then(|row| {
            row.trim_matches('|')
                .split('|')
                .nth(1)
                .map(|cell| cell.trim().to_string())
        })
        .expect("security mark");
    let core = manual_page("reference-core-extension-points.md");
    let core_row = core
        .lines()
        .find(|line| line.starts_with("| `ilium.core.smart-copy/after-copy` |"))
        .expect("core row");
    let core_mark = core_row
        .trim_matches('|')
        .split('|')
        .nth(2)
        .expect("risk cell")
        .trim()
        .to_string();
    assert_eq!(core_mark, security_mark);
}

/// REQ: SEC-293
#[test]
fn security_293_page_risk_of_recognisers_is_standard_and_not_listed_as_high() {
    let security = manual_page("reference-extension-security.md");
    assert!(!security.contains("| `ilium.core.smart-copy/recognisers` | high |"));
    let rows = table_after(&page_text(), "### Extension points");
    let row = rows
        .iter()
        .find(|row| row[0].contains("recognisers"))
        .expect("recognisers row");
    assert_eq!(row[4], "standard");
}

/// REQ: SEC-293
#[test]
fn security_293_page_security_section_points_to_the_corrected_table() {
    let page = page_text();
    assert!(page.contains("([SEC-293])"));
    assert!(page.contains("[Extension security](/pages/reference-extension-security/)"));
}

/// REQ: F-066
#[test]
fn feature_066_example_hook_contribution_uses_the_after_copy_point_and_the_hook_handler_export() {
    let page = page_text();
    assert!(page.contains("\"point\": \"ilium.core.smart-copy/after-copy\""));
    assert!(page.contains("\"exports\": [\n    \"hook-handler\"\n  ]"));
    assert!(page.contains("extensions.contribute:ilium.core.smart-copy/after-copy"));
    assert!(page.contains("// call.payload_json holds: text, kind, pane"));
}

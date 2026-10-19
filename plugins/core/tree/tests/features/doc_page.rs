//! The page, the samples it prints and the user documentation the plugin must keep true (F-005, F-006).

use crate::common::{page_text, workspace_root};

fn user_doc(name: &str) -> String {
    let path = workspace_root().join("src/docs").join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{path:?}: {error}"))
}

/// REQ: F-005
#[test]
fn doc_005_the_user_docs_list_the_six_tree_orders() {
    let settings = user_doc("settings.md");
    let row = settings
        .lines()
        .find(|line| line.contains("`ui.tree_order`"))
        .expect("ui.tree_order row");
    for value in [
        "manual",
        "type",
        "age_ascending",
        "age_descending",
        "name_ascending",
        "name_descending",
    ] {
        assert!(
            row.contains(&format!("`{value}`")),
            "row {row} lacks {value}"
        );
    }
}

/// REQ: F-005
#[test]
fn doc_005_manual_order_is_the_default_and_the_stored_order_is_unchanged() {
    let doc = user_doc("panes-and-layout.md");
    assert!(doc.contains("By default children appear in the order you arranged them (**Manual**)."));
    assert!(doc.contains("Changing the display order does not change the stored manual order."));
}

/// REQ: F-005
#[test]
fn doc_005_the_cost_order_is_driven_by_the_agent_cost_settings() {
    let doc = user_doc("panes-and-layout.md");
    assert!(doc.contains(
        "An additional cost order (most expensive first) is driven by the Agent Cost settings"
    ));
}

/// REQ: F-006
#[test]
fn doc_006_the_user_docs_describe_auto_removal_and_its_exceptions() {
    let doc = user_doc("panes-and-layout.md");
    assert!(doc.contains("**Auto-remove empty groups** (Settings, on by default)"));
    assert!(doc.contains("Projects and split views are never removed automatically."));
}

/// REQ: F-006
#[test]
fn doc_006_unlock_stays_available_when_the_lock_option_is_off() {
    let doc = user_doc("panes-and-layout.md");
    assert!(doc.contains(
        "**Unlock** stays available even when the lock option is switched off in Settings."
    ));
}

/// REQ: F-005, F-006
#[test]
fn doc_005_the_page_declares_side_both_and_the_covered_features() {
    let page = page_text();
    assert!(page.contains("(`ilium.core.tree`, side both)"));
    assert!(page.contains("- Tree ordering (Settings, User Interface tab)."));
    assert!(page.contains("- Bookmarks, locks and automatic removal of empty groups"));
}

/// REQ: F-006
#[test]
fn doc_006_the_page_says_tree_restructure_is_high_risk_and_bundled_by_default() {
    let page = page_text();
    assert!(page.contains("`tree.restructure` is a high risk grant"));
    assert!(page.contains("The bundled plugin carries it by default."));
}

/// REQ: F-006
#[test]
fn doc_006_the_page_prints_every_sample_in_rust_and_typescript() {
    let page = page_text();
    for heading in [
        "#### Order the tree",
        "#### New group or folder",
        "#### Move and rename without overriding the user",
        "#### Bookmarks and locks",
        "#### Remove empty groups",
        "#### Right-click menu for a node",
    ] {
        let start = page
            .find(heading)
            .unwrap_or_else(|| panic!("{heading} missing"));
        let end = page[start + 5..]
            .find("\n#### ")
            .map_or(page.len(), |at| start + 5 + at);
        let section = &page[start..end];
        assert!(
            section.contains("**Rust**") && section.contains("**TypeScript**"),
            "{heading}"
        );
    }
}

/// REQ: F-006
#[test]
fn doc_006_the_page_says_a_locked_close_is_refused_with_conflict() {
    assert!(page_text().contains("so a later `close-node` is refused with `refused(conflict)`"));
}

/// REQ: F-005
#[test]
fn doc_005_the_order_sample_sorts_by_the_first_four_bytes_of_the_lowercase_title() {
    assert!(
        page_text().contains("// Alphabetical by the first four bytes of the lower-case title.")
    );
}

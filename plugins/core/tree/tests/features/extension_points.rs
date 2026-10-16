//! The extension points, service and topic the page declares: `before-move`, `before-close`, `node-sorts`.
//!
//! The kernel invokes hooks and collects contributions, so these tests read the table of the page and the
//! example package manifest; behaviour through a contributor needs the host (see API_GAPS.md).

use serde_json::Value;

use crate::common::page_text;

/// Rows of the Extension points table: `(point, kind, what, payload, risk)`.
fn point_rows() -> Vec<[String; 5]> {
    let page = page_text();
    let start = page.find("| Point | Kind |").expect("extension point table");
    page[start..]
        .lines()
        .skip(2)
        .take_while(|line| line.starts_with('|'))
        .map(|line| {
            let cells: Vec<String> = line.trim_matches('|').split('|').map(|cell| cell.trim().to_string()).collect();
            [cells[0].clone(), cells[1].clone(), cells[2].clone(), cells[3].clone(), cells[4].clone()]
        })
        .collect()
}

/// REQ: F-004
#[test]
fn feature_004_the_page_lists_exactly_three_points() {
    let names: Vec<String> = point_rows().into_iter().map(|row| row[0].trim_matches('`').to_string()).collect();
    assert_eq!(names, ["before-move", "before-close", "node-sorts"]);
}

/// REQ: F-004
#[test]
fn feature_004_before_move_is_a_veto_hook_with_the_documented_payload() {
    let rows = point_rows();
    let row = rows.iter().find(|row| row[0] == "`before-move`").expect("row");
    assert_eq!(row[1], "hook (veto)");
    assert_eq!(row[3], "node, from_parent, to_parent, index, source (mouse, keys, command, plugin)");
    assert_eq!(row[4], "standard");
}

/// REQ: F-004
#[test]
fn feature_004_before_close_is_a_veto_hook_with_the_documented_payload() {
    let rows = point_rows();
    let row = rows.iter().find(|row| row[0] == "`before-close`").expect("row");
    assert_eq!(row[1], "hook (veto)");
    assert_eq!(row[3], "node, kind, has_children");
    assert_eq!(row[4], "standard");
}

/// REQ: F-005
#[test]
fn feature_005_node_sorts_is_a_collect_point_with_entry_id_title_slot() {
    let rows = point_rows();
    let row = rows.iter().find(|row| row[0] == "`node-sorts`").expect("row");
    assert_eq!(row[1], "collect");
    assert!(row[3].starts_with("entry: id, title, slot"), "payload {}", row[3]);
    assert_eq!(row[4], "standard");
}

/// REQ: F-005
#[test]
fn feature_005_node_sorts_adds_to_the_order_by_choice() {
    let rows = point_rows();
    let row = rows.iter().find(|row| row[0] == "`node-sorts`").expect("row");
    assert!(row[2].contains("Order By"), "what: {}", row[2]);
}

/// REQ: F-004
#[test]
fn feature_004_every_point_is_standard_risk() {
    assert!(point_rows().iter().all(|row| row[4] == "standard"));
}

/// REQ: F-004
#[test]
fn feature_004_the_veto_example_manifest_contributes_to_before_close() {
    let page = page_text();
    let start = page.find("#### Example: Refuses to close").expect("example heading");
    let rest = &page[start..];
    let open = rest.find("```json\n").expect("json") + "```json\n".len();
    let close = rest[open..].find("```").expect("end");
    let manifest: Value = serde_json::from_str(&rest[open..open + close]).expect("example JSON");
    assert_eq!(manifest["exports"], serde_json::json!(["hook-handler"]));
    let extension = &manifest["contributes"]["extensions"][0];
    assert_eq!(extension["point"], "ilium.core.tree/before-close");
    assert_eq!(extension["id"], "keep-build-pane");
    assert_eq!(extension["priority"], 50);
    assert_eq!(extension["handler"], "keep_build_pane");
    assert_eq!(
        manifest["capabilities"]["server"],
        serde_json::json!(["extensions.contribute:ilium.core.tree/before-close"])
    );
}

/// REQ: F-004
#[test]
fn feature_004_the_example_point_exists_in_the_table() {
    assert!(point_rows().iter().any(|row| row[0] == "`before-close`"));
}

/// REQ: F-004
#[test]
fn feature_004_the_page_states_that_a_late_or_trapping_handler_counts_as_pass() {
    assert!(page_text().contains("A handler that is late, traps or returns a reply the mode does not allow counts as `pass`."));
}

/// REQ: F-004
#[test]
fn feature_004_the_page_states_that_the_plugin_is_replaceable() {
    let page = page_text();
    assert!(page.contains("**Replaceable:** yes. A replacement lists `\"replaces\": [\"ilium.core.tree\"]`"));
}

/// REQ: SEC-293
#[test]
fn security_293_the_page_cites_the_corrected_extension_security_table() {
    let page = page_text();
    assert!(page.contains("### Security"));
    assert!(page.contains("[SEC-293]"));
    assert!(page.contains("/pages/reference-extension-security/"));
}

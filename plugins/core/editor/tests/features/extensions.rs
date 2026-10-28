//! Extension points, topics and the replaceable flag (core-plugin-editor.md "Extension points").

use crate::fixtures::{manifest_json, page_text};
use ilium_plugin_sdk::testing::core_stubs;

const POINTS: [&str; 3] = [
    "ilium.core.editor/before-save",
    "ilium.core.editor/file-openers",
    "ilium.core.editor/line-badges",
];

/// REQ: F-019
#[test]
fn f_019_stub_registry_lists_the_three_extension_points() {
    let stub = core_stubs::find("ilium.core.editor");
    let stub = stub.expect("the editor is a known core plugin");
    let mut points: Vec<String> = stub
        .extension_points
        .iter()
        .map(|point| point.to_string())
        .collect();
    points.sort();
    assert_eq!(points, POINTS.to_vec());
}

/// REQ: F-019
#[test]
fn f_019_stub_registry_lists_the_buffers_service() {
    let stub = core_stubs::find("ilium.core.editor").expect("known core plugin");
    assert!(stub
        .services
        .iter()
        .any(|service| *service == "ilium.core.editor/buffers"));
}

/// REQ: F-019
#[test]
fn f_019_before_save_is_a_transform_hook_with_text_reply() {
    let page = page_text();
    assert!(page.contains("buffer, path, language, text; reply: text"));
}

/// REQ: F-019
#[test]
fn f_019_file_openers_entry_shape_is_documented() {
    assert!(page_text().contains("entry: id, extensions, pane_kind, priority"));
}

/// REQ: F-019
#[test]
fn f_019_line_badges_entry_shape_is_documented() {
    assert!(page_text().contains("entry: id, regex, glyph, tooltip"));
}

/// REQ: F-019
#[test]
fn f_019_manifest_does_not_grant_foreign_extension_points() {
    let grants = manifest_json()["capabilities"]["client"].to_string();
    assert!(!grants.contains("extensions.contribute"));
}

/// REQ: F-019
#[test]
fn f_019_hook_handler_is_not_exported_so_the_editor_contributes_no_hooks() {
    let exports = manifest_json()["client"]["exports"].to_string();
    assert!(!exports.contains("hook-handler"));
}

/// REQ: F-022
#[test]
fn f_022_file_explorer_and_picker_are_part_of_the_editor_page() {
    let page = page_text();
    assert!(page.contains("the file explorer and picker"));
    assert!(page.contains("fs.read:project"));
}

/// REQ: F-022
#[test]
fn f_022_picker_grants_include_the_modal_capability() {
    let grants = manifest_json()["capabilities"]["client"].to_string();
    assert!(grants.contains("ui.modal"), "{grants}");
}

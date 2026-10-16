//! The manifest of ilium.core.editor against the manual page (the page says the manifest is exact).

use crate::support::{manifest_json, manual_page, page_text};
use ilium_plugin_core_editor::EditorClient;
use ilium_plugin_sdk::NativeEntry;
use serde_json::Value;

fn complete_manifest_on_page() -> Value {
    let page = page_text();
    let heading = page.find("### Complete manifest").unwrap_or(0);
    let rest = &page[heading..];
    let start = rest.find("```json").map(|at| at + "```json".len()).unwrap_or(0);
    let body = &rest[start..];
    let end = body.find("```").unwrap_or(body.len());
    serde_json::from_str(body[..end].trim()).unwrap_or(Value::Null)
}

fn string_set(value: &Value) -> Vec<String> {
    let mut items: Vec<String> = value
        .as_array()
        .map(|list| list.iter().filter_map(|item| item.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    items.sort();
    items
}

/// REQ: F-019
#[test]
fn doc_f_019_manifest_identity_fields() {
    let manifest = manifest_json();
    assert_eq!(manifest["api"], "ilium:plugin@0.1.0");
    assert_eq!(manifest["id"], "ilium.core.editor");
    assert_eq!(manifest["name"], "Editor");
    assert_eq!(manifest["version"], "0.1.0");
    assert_eq!(manifest["publisher"], "Ilium");
    assert_eq!(manifest["license"], "MIT");
}

/// REQ: F-019
#[test]
fn doc_f_019_client_entry_world_and_exports() {
    let client = &manifest_json()["client"];
    assert_eq!(client["entry"], "client.wasm");
    assert_eq!(client["world"], "ilium:client/base@0.1.0");
    assert_eq!(client["exports"], serde_json::json!(["views", "token-provider", "service-provider"]));
}

/// REQ: F-019
#[test]
fn doc_f_019_client_capabilities_are_exactly_the_eight_grants() {
    let manifest = manifest_json();
    let mut expected = vec![
        "editor.read",
        "editor.edit",
        "editor.provide",
        "fs.read:project",
        "panes.create",
        "terminal.write",
        "ui.panels",
        "settings.write-kernel:editor",
    ]
    .into_iter()
    .map(str::to_string)
    .collect::<Vec<_>>();
    expected.sort();
    assert_eq!(string_set(&manifest["capabilities"]["client"]), expected);
}

/// REQ: F-019
#[test]
fn doc_f_019_manifest_declares_no_server_side() {
    let manifest = manifest_json();
    assert!(manifest.get("server").is_none(), "the editor is client only");
    assert!(manifest["capabilities"].get("server").is_none());
}

/// REQ: F-019
#[test]
fn doc_f_019_activation_trigger_is_pane_kind_editor() {
    assert_eq!(manifest_json()["activation"], serde_json::json!(["pane-kind:editor"]));
}

/// REQ: F-019
#[test]
fn doc_f_019_limits_tier_is_first_party() {
    assert_eq!(manifest_json()["limits_tier"], "first-party");
}

/// REQ: F-019
#[test]
fn doc_f_019_manifest_file_equals_complete_manifest_on_the_page() {
    let on_page = complete_manifest_on_page();
    assert!(on_page.is_object(), "the page has a complete manifest block");
    assert_eq!(manifest_json(), on_page);
}

/// REQ: F-019
#[test]
fn doc_f_019_fragment_grants_are_a_subset_of_the_complete_manifest() {
    let page = page_text();
    let fragment_start = page.find("### Manifest").unwrap_or(0);
    let fragment_rest = &page[fragment_start..];
    let json_start = fragment_rest.find("```json").map(|at| at + 7).unwrap_or(0);
    let body = &fragment_rest[json_start..];
    let fragment: Value = serde_json::from_str(body[..body.find("```").unwrap_or(0)].trim()).unwrap_or(Value::Null);
    let fragment_grants = string_set(&fragment["capabilities"]["client"]);
    assert_eq!(fragment_grants.len(), 7);
    let complete = string_set(&manifest_json()["capabilities"]["client"]);
    for grant in fragment_grants {
        assert!(complete.contains(&grant), "{grant} missing from the complete manifest");
    }
}

/// REQ: F-019
#[test]
fn editor_client_declares_the_side_and_exports_of_the_manifest() {
    assert_eq!(<EditorClient as NativeEntry>::SIDE, "client");
    assert_eq!(<EditorClient as NativeEntry>::EXPORTS, &["views", "token-provider", "service-provider"]);
}

/// REQ: F-019, SEC-293
#[test]
fn doc_sec_293_extension_point_table_lists_three_standard_points() {
    let page = page_text();
    for row in [
        "| `before-save` | hook (transform) |",
        "| `file-openers` | collect |",
        "| `line-badges` | collect |",
    ] {
        let line = page.lines().find(|line| line.starts_with(row));
        let line = line.unwrap_or_else(|| panic!("row {row} missing"));
        assert!(line.trim_end().ends_with("| standard |"), "risk of {row} is standard");
    }
}

/// REQ: F-019, SEC-293
#[test]
fn doc_sec_293_core_extension_point_reference_agrees_with_the_page() {
    let reference = manual_page("reference-core-extension-points.md");
    assert!(reference.contains("| `ilium.core.editor/before-save` | hook: transform | standard |"));
    assert!(reference.contains("| `ilium.core.editor/file-openers` | collect | standard |"));
    assert!(reference.contains("| `ilium.core.editor/line-badges` | collect | standard |"));
    assert!(reference.contains("| [Editor](/pages/core-plugin-editor/) | `ilium.core.editor` | 3 | 1 | 1 | yes |"));
}

/// REQ: F-019
#[test]
fn doc_f_019_service_topic_and_replaceable_lines() {
    let page = page_text();
    assert!(page.contains("- `ilium.core.editor/buffers`: list, read, replace-range: read and edit open buffers"));
    assert!(page.contains("- `ilium.core.editor/saved`: { buffer, path } after a save"));
    assert!(page.contains("**Replaceable:** yes."));
}

/// REQ: F-019
#[test]
fn doc_f_019_kernel_tab_row_and_disabled_notice() {
    let page = page_text();
    assert!(page.contains("| `editor` | Editor | `[editor]` | Terminal |"));
    assert!(page.contains("\"Editor is disabled. Values are kept.\""));
    assert!(page.contains("holds `settings.write-kernel:<tab>`") || page.contains("`settings.write-kernel:<tab>`"));
}

/// REQ: F-019
#[test]
fn doc_f_019_kernel_and_boot_rows_for_the_editor() {
    let boot = manual_page("reference-kernel-and-boot.md");
    assert!(boot.contains("| Editor | none | `pane-kind:editor` |"));
    assert!(boot.contains("| Editor | Files open through `open-external` |"));
}

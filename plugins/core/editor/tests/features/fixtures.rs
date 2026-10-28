//! Fixtures used only by the feature tests: sample texts, the manifest and manual page readers,
//! and tree queries. Kept out of `support.rs` because the port target mounts that file and does
//! not use these items.

use ilium_plugin_core_editor::EditorClient;
use ilium_plugin_sdk::generated::types::{Area, NodeId, UiNode, UiTree, ViewTarget, Widget};
use ilium_plugin_sdk::Views;

use crate::support::MANIFEST_PATH;

/// Rust sample shown in the page figure (core-plugin-editor.md "What the user sees").
pub const MAIN_RS: &str = "use std::fs;\n\nfn main() {\n    let n = 42;\n    // run it\n}\n";
/// Markdown sample shown in the page figure.
pub const NOTES_MD: &str = "# Plan\n- [ ] ship it\nTODO write docs\n";

/// Text of the manual page of this plugin.
pub fn page_text() -> String {
    let path = ilium_plugin_sdk::testing::manual::pages_dir().join("core-plugin-editor.md");
    std::fs::read_to_string(&path).unwrap_or_default()
}

/// The manifest of the plugin as JSON.
pub fn manifest_json() -> serde_json::Value {
    let text = std::fs::read_to_string(MANIFEST_PATH).unwrap_or_default();
    serde_json::from_str(&text).unwrap_or(serde_json::Value::Null)
}

/// Renders the editor pane tree directly.
pub fn pane_tree(editor: &mut EditorClient, columns: u16, rows: u16) -> UiTree {
    Views::render(editor, ViewTarget::Pane(NodeId(7)), Area { columns, rows })
}

/// All nodes of a tree whose widget matches.
pub fn nodes_where(tree: &UiTree, matches: impl Fn(&Widget) -> bool) -> Vec<&UiNode> {
    tree.nodes
        .iter()
        .filter(|node| matches(&node.widget))
        .collect()
}

/// Number of characters equal to `needle` in `text`.
pub fn count_char(text: &str, needle: char) -> usize {
    text.chars()
        .filter(|character| *character == needle)
        .count()
}

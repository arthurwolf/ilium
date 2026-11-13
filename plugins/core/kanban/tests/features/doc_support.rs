//! Readers of the manual pages, user docs and the plugin manifest, used by the feature tests only.

use std::path::PathBuf;

use serde_json::Value;

use crate::common::manifest_path;

/// Root of the worktree, found from the crate directory (plugins/core/kanban).
pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

/// The manual page of this plugin.
pub fn page_text() -> String {
    let path = workspace_root().join("docs/plugins-manual/pages/core-plugin-kanban.md");
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("page {path:?}: {error}"))
}

/// A user documentation page under src/docs.
pub fn user_doc(name: &str) -> String {
    let path = workspace_root().join("src/docs").join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{path:?}: {error}"))
}

/// The manifest as JSON.
pub fn manifest() -> Value {
    let text = std::fs::read_to_string(manifest_path()).expect("manifest.json readable");
    serde_json::from_str(&text).expect("manifest.json is JSON")
}

/// The first json code block after the heading text `heading`.
pub fn json_block_after(page: &str, heading: &str) -> Value {
    let start = page
        .find(heading)
        .unwrap_or_else(|| panic!("heading {heading} missing"));
    let rest = &page[start..];
    let open = rest.find("```json\n").expect("json block") + "```json\n".len();
    let close = rest[open..].find("```").expect("block end");
    serde_json::from_str(&rest[open..open + close]).expect("page block is JSON")
}

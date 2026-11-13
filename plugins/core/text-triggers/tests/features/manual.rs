//! Manual-page and manifest helpers of the feature tests. Mounted only by the features target: the port target
//! mounts `common.rs` through `#[path]` and never reads the manual pages or the manifest JSON.

use std::path::PathBuf;

use serde_json::Value;

use crate::common::manifest_path;

pub const PLUGIN_ID: &str = "ilium.core.text-triggers";

/// Root of the worktree, found from the crate directory (plugins/core/text-triggers).
pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

/// A page of the manual by file name.
pub fn manual_page(name: &str) -> String {
    let path = workspace_root()
        .join("docs/plugins-manual/pages")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("page {path:?}: {error}"))
}

/// The manual page of this plugin.
pub fn page_text() -> String {
    manual_page("core-plugin-text-triggers.md")
}

/// Parsed `manifest.json`.
pub fn manifest() -> Value {
    let text = std::fs::read_to_string(manifest_path()).expect("manifest.json readable");
    serde_json::from_str(&text).expect("manifest.json is JSON")
}

/// The first json code block after `heading`.
pub fn json_block_after(page: &str, heading: &str) -> Value {
    let start = page
        .find(heading)
        .unwrap_or_else(|| panic!("heading {heading} missing"));
    let rest = &page[start..];
    let open = rest.find("```json\n").expect("json block") + "```json\n".len();
    let close = rest[open..].find("```").expect("block end");
    serde_json::from_str(&rest[open..open + close]).expect("page block is JSON")
}

/// Rows of the first table that follows `anchor`: cells of every row after the separator.
pub fn table_after(page: &str, anchor: &str) -> Vec<Vec<String>> {
    let start = page
        .find(anchor)
        .unwrap_or_else(|| panic!("anchor {anchor} missing"));
    page[start..]
        .lines()
        .skip_while(|line| !line.starts_with('|'))
        .skip(2)
        .take_while(|line| line.starts_with('|'))
        .map(|line| {
            line.trim_matches('|')
                .split('|')
                .map(|cell| cell.trim().to_string())
                .collect()
        })
        .collect()
}

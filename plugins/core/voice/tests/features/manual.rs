//! Readers of the plugin manual pages and of their tables and json blocks. Mounted only by the `features`
//! target; the `port` target does not read the manual.

use serde_json::Value;

use crate::common::workspace_root;

/// A page of the manual.
pub fn manual_page(name: &str) -> String {
    let path = workspace_root()
        .join("docs/plugins-manual/pages")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("page {path:?}: {error}"))
}

/// The manual page of this plugin.
pub fn page_text() -> String {
    manual_page("core-plugin-voice.md")
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

//! Shared helpers: paths, page and manifest readers, the client host, table readers.

use std::path::PathBuf;

use ilium_plugin_core_settings::SettingsClient;
use ilium_plugin_sdk::testing::FakeHost;
use serde_json::Value;

/// The plugin id (manifest `id`).
pub const PLUGIN_ID: &str = "ilium.core.settings";

/// Path of the plugin manifest (the skeleton file, exact per the manual).
pub fn manifest_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("manifest.json")
}

/// Root of the worktree, found from the crate directory (plugins/core/settings).
pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

/// The manual page of this plugin.
pub fn page_text() -> String {
    manual_page("core-plugin-settings.md")
}

/// Any page of the manual by file name.
pub fn manual_page(name: &str) -> String {
    let path = workspace_root().join("docs/plugins-manual/pages").join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{path:?}: {error}"))
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
    let start = page.find(heading).unwrap_or_else(|| panic!("heading {heading} missing"));
    let rest = &page[start..];
    let open = rest.find("```json\n").expect("json block") + "```json\n".len();
    let close = rest[open..].find("```").expect("block end");
    serde_json::from_str(&rest[open..open + close]).expect("page block is JSON")
}

/// The client grants of the manifest.
pub fn client_grants() -> Vec<String> {
    manifest()["capabilities"]["client"]
        .as_array()
        .expect("client grants")
        .iter()
        .map(|grant| grant.as_str().expect("grant string").to_string())
        .collect()
}

/// A client-side host with the manifest grants and the plugin activated.
pub fn client_host() -> FakeHost {
    let mut host = FakeHost::for_manifest(manifest_path()).expect("manifest loads").side("client").build();
    host.activate::<SettingsClient>();
    host
}

/// Rows of the first markdown table that starts with `header_prefix`: the cells of each body row.
pub fn table_rows(text: &str, header_prefix: &str) -> Vec<Vec<String>> {
    let start = text.find(header_prefix).unwrap_or_else(|| panic!("table {header_prefix} missing"));
    text[start..]
        .lines()
        .skip(2)
        .take_while(|line| line.starts_with('|'))
        .map(|line| line.trim_matches('|').split('|').map(|cell| cell.trim().to_string()).collect())
        .collect()
}

/// The 25 rows of "The 25 tabs and who owns them": position, tab id, label, owner.
pub fn tab_rows() -> Vec<Vec<String>> {
    let page = page_text();
    let start = page.find("### The 25 tabs and who owns them").expect("tabs section");
    table_rows(&page[start..], "| Position |")
}

/// The golden file `name` under tests/features/golden.
pub fn golden_path(name: &str) -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/features/golden")
        .join(name)
        .to_string_lossy()
        .to_string()
}

/// The `<pre class="term">` figures of the page, tags stripped, trailing spaces trimmed.
pub fn page_figures() -> Vec<String> {
    let page = page_text();
    let mut figures = Vec::new();
    let mut rest = page.as_str();
    while let Some(start) = rest.find("<pre class=\"term\">") {
        let body = &rest[start + "<pre class=\"term\">".len()..];
        let end = body.find("</pre>").expect("figure end");
        let mut text = String::new();
        let mut in_tag = false;
        for character in body[..end].chars() {
            match character {
                '<' => in_tag = true,
                '>' => in_tag = false,
                _ if !in_tag => text.push(character),
                _ => {}
            }
        }
        let lines: Vec<&str> = text.trim_matches('\n').lines().map(str::trim_end).collect();
        figures.push(lines.join("\n"));
        rest = &body[end..];
    }
    figures
}

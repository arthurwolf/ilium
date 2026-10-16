//! Shared helpers of the Ambient look feature tests (manual page core-plugin-ambient.md).
//!
//! Every use of a testkit item that the skeleton lacks lives in the "gap helpers" section at the
//! end of this file (see ../API_GAPS.md), so a missing item breaks this file only.
#![allow(dead_code)]

use std::path::PathBuf;

use ilium_plugin_core_ambient::AmbientClient;
use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::{
    Area, Event, PluginSide, SettingsChange, SettingsOrigin, Submission, UiNode, UiTree, ViewTarget, Widget,
};
use ilium_plugin_sdk::Views;
use serde_json::Value;

pub const PLUGIN_ID: &str = "ilium.core.ambient";
pub const GALLERY: &str = "gallery";
pub const PREVIEW_VIEW: &str = "scene-preview";
pub const TAB: &str = "animations";

pub fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

pub fn manifest_path() -> PathBuf {
    crate_dir().join("manifest.json")
}

pub fn manifest_value() -> Value {
    let text = std::fs::read_to_string(manifest_path()).expect("manifest.json is readable");
    serde_json::from_str(&text).expect("manifest.json is JSON")
}

pub fn workspace_root() -> PathBuf {
    crate_dir().join("../../..")
}

/// The text of a manual page by file stem, for example `core-plugin-ambient`.
pub fn page(stem: &str) -> String {
    let path = workspace_root().join("docs/plugins-manual/pages").join(format!("{stem}.md"));
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("page {path:?}: {error}"))
}

pub fn page_text() -> String {
    page("core-plugin-ambient")
}

/// A user document of the repository by file name, for example `animations.md`.
pub fn user_doc(name: &str) -> String {
    let path = workspace_root().join("src/docs").join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("user doc {path:?}: {error}"))
}

/// The bodies of the ```json fences of a page, in order.
pub fn json_blocks(page: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut rest = page;
    while let Some(start) = rest.find("```json\n") {
        let after = &rest[start + "```json\n".len()..];
        let Some(end) = after.find("\n```") else { break };
        blocks.push(after[..end].to_string());
        rest = &after[end + 4..];
    }
    blocks
}

/// The text of the `<pre class="term">` figures of a page with the markup removed, in order.
pub fn term_blocks(page: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut rest = page;
    while let Some(start) = rest.find("<pre class=\"term\">") {
        let after = &rest[start + "<pre class=\"term\">".len()..];
        let Some(end) = after.find("</pre>") else { break };
        let mut text = String::new();
        let mut in_tag = false;
        for symbol in after[..end].chars() {
            match symbol {
                '<' => in_tag = true,
                '>' => in_tag = false,
                other if !in_tag => text.push(other),
                _ => {}
            }
        }
        let text = text.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">");
        blocks.push(text.trim_end_matches('\n').to_string());
        rest = &after[end + 6..];
    }
    blocks
}

/// The cells of the table row of `text` whose first cell is `first_cell` (backticks kept).
pub fn table_row(text: &str, first_cell: &str) -> Vec<String> {
    let line = text
        .lines()
        .find(|line| {
            let cells: Vec<&str> = line.split('|').map(str::trim).collect();
            cells.len() > 2 && cells[1] == first_cell
        })
        .unwrap_or_else(|| panic!("no table row starting with {first_cell}"));
    let mut cells: Vec<String> = line.split('|').map(|cell| cell.trim().to_string()).collect();
    cells.remove(0);
    cells.pop();
    cells
}

pub fn strings(value: &Value) -> Vec<String> {
    value.as_array().expect("array").iter().map(|item| item.as_str().expect("string").to_string()).collect()
}

/// The `Complete manifest` block of the page (the second ```json fence).
pub fn complete_manifest_on_page() -> Value {
    let blocks = json_blocks(&page_text());
    serde_json::from_str(&blocks[1]).expect("complete manifest block is JSON")
}

/// The `Manifest` fragment block of the page (the first ```json fence).
pub fn fragment_on_page() -> Value {
    let blocks = json_blocks(&page_text());
    serde_json::from_str(&blocks[0]).expect("fragment block is JSON")
}

/// The rows of the one `look` section of the `animations` tab in the manifest.
pub fn manifest_rows() -> Vec<Value> {
    manifest_value()["contributes"]["settings"][0]["sections"][0]["rows"].as_array().expect("rows").clone()
}

/// The manifest row with this `key`.
pub fn row(key: &str) -> Value {
    manifest_rows()
        .into_iter()
        .find(|candidate| candidate["key"] == key)
        .unwrap_or_else(|| panic!("no row {key}"))
}

pub fn client_host() -> FakeHost {
    FakeHost::for_manifest(manifest_path()).expect("manifest loads").side("client").build()
}

pub fn activated_client() -> FakeHost {
    let mut host = client_host();
    host.activate::<AmbientClient>();
    host
}

/// A `settings-changed` event for the plugin's own keys, as the kernel sends after a user edit.
pub fn settings_changed(keys: &[&str]) -> Event {
    Event::SettingsChanged(SettingsChange {
        keys: keys.iter().map(|key| key.to_string()).collect(),
        origin: SettingsOrigin::User,
        side: PluginSide::Client,
    })
}

/// Renders one view of the plugin directly through its `views` export (native mode).
pub fn render_direct(view: ViewTarget, columns: u16, rows: u16) -> UiTree {
    let mut client = AmbientClient::default();
    client.render(view, Area { columns, rows })
}

/// The nodes of a tree whose widget satisfies `matches`.
pub fn nodes_where(tree: &UiTree, matches: impl Fn(&Widget) -> bool) -> Vec<UiNode> {
    tree.nodes.iter().filter(|node| matches(&node.widget)).cloned().collect()
}

/// The node with this widget key.
pub fn node_by_key(tree: &UiTree, key: &str) -> Option<UiNode> {
    tree.nodes.iter().find(|node| node.key.as_deref() == Some(key)).cloned()
}

/// Every `set-kernel-setting`-like submission is a `Submission`; this only counts them.
pub fn submission_count(submissions: &[Submission]) -> usize {
    submissions.len()
}

// ---------------------------------------------------------------------------------------------
// Gap helpers: every item below is missing from the skeleton testkit (see ../API_GAPS.md).
// ---------------------------------------------------------------------------------------------

/// Seeds one global-storage value before the plugin runs (the plugin keeps `last_suggestion` there).
pub fn seed_global_storage(host: &mut FakeHost, key: &str, value: &[u8]) {
    host.storage_mut().put(ilium_plugin_sdk::host::storage::Bucket::Global, key, value);
}

/// Sets the stored values of one kernel settings tab, as the kernel would return them from `read`.
pub fn set_kernel_tab(host: &mut FakeHost, tab: &str, values: Value) {
    host.kernel_settings().set_tab(tab, values);
}

/// Every `kernel-settings.write` the plugin made: (tab, key, JSON value text), in order.
pub fn kernel_writes(host: &mut FakeHost) -> Vec<(String, String, String)> {
    host.kernel_settings().written()
}

/// Every topic the plugin published: (topic, payload JSON text), in order.
pub fn published_topics(host: &FakeHost) -> Vec<(String, String)> {
    host.published_topics()
}

/// The tree the plugin returned for the last render of `target` (gap: `FakeHost::last_tree`).
pub fn last_tree_of(host: &mut FakeHost, target: ViewTarget) -> UiTree {
    host.last_tree(target)
}

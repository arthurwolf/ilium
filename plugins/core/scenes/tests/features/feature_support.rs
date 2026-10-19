//! Helpers only the feature tests use. The port twins mount `common.rs` alone, so these items
//! live here, mounted only by the feature test target.

use ilium_plugin_sdk::testing::{FakeScene, FakeSession};
use serde_json::{json, Value};

use crate::common::*;

pub const PLUGIN_ID: &str = "ilium.core.scenes";
pub const POINT_PACKS: &str = "ilium.core.scenes/scene-packs";
pub const POINT_SOURCES: &str = "ilium.core.scenes/scene-data-sources";
/// One sidereal day in milliseconds (86,164.0905 s rounded to the millisecond).
pub const SIDEREAL_DAY_MILLIS: u64 = 86_164_091;

/// A user documentation page of the repository (src/docs/<stem>.md).
pub fn user_doc(stem: &str) -> String {
    let path = workspace_root().join("src/docs").join(format!("{stem}.md"));
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("user doc {path:?}: {error}"))
}

/// Cells of the markdown table whose header line starts with `header_start`, without header and separator.
pub fn table_after(text: &str, header_start: &str) -> Vec<Vec<String>> {
    let start = text
        .find(header_start)
        .unwrap_or_else(|| panic!("table starting with {header_start:?}"));
    text[start..]
        .lines()
        .skip(2)
        .take_while(|line| line.starts_with('|'))
        .map(|line| {
            line.trim()
                .trim_start_matches('|')
                .trim_end_matches('|')
                .split('|')
                .map(|cell| cell.trim().to_string())
                .collect()
        })
        .collect()
}

/// The first fenced json block that follows the heading `heading` on a page, parsed.
pub fn json_block_after(text: &str, heading: &str) -> Value {
    let start = text
        .find(heading)
        .unwrap_or_else(|| panic!("heading {heading:?}"));
    let rest = &text[start..];
    let open = rest.find("```json\n").expect("json block") + "```json\n".len();
    let close = rest[open..].find("```").expect("closing fence");
    serde_json::from_str(&rest[open..open + close]).expect("block is JSON")
}

/// The scene spec JSON the host passes to the constructor (api-scenes.md "Lifecycle of a renderer").
pub fn spec(scene: &str, settings: Value, palette: &[u32], mode: &str) -> String {
    json!({ "scene": scene, "settings": settings, "palette": palette, "mode": mode }).to_string()
}

/// The same as `fake_scene` with the scene settings of the spec (needs `FakeScene::with_settings`).
pub fn fake_scene_with(scene_id: &str, columns: u16, rows: u16, settings: &Value) -> FakeScene {
    fake_scene(scene_id, columns, rows).with_settings(&settings.to_string())
}

/// The status text of the renderer after the frames drawn so far (needs `FakeScene::status`).
pub fn status_of(scene: &FakeScene) -> Option<String> {
    scene.status()
}

/// Adds a contribution of another plugin to a point of the plugin (needs `FakeHost::extensions`).
pub fn contribute(session: &mut FakeSession, point: &str, id: &str, priority: u8, entry: &Value) {
    session
        .plugin(PLUGIN_ID)
        .extensions()
        .contribute(point, id, priority, entry.to_string());
}

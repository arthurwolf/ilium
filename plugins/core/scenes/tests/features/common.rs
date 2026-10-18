//! Shared helpers of the Scenes feature tests (manual page core-plugin-scenes.md).
//!
//! Every use of a testkit or skeleton item that does not exist yet lives in the "gap helpers"
//! section at the end of this file (see ../API_GAPS.md), so a missing item breaks this file only.
#![allow(dead_code)]

use std::path::PathBuf;

use ilium_plugin_core_scenes::ScenesScene;
use ilium_plugin_sdk::anim::FrameInput;
use ilium_plugin_sdk::testing::{FakeScene, FakeSession, Screen};
use serde_json::{json, Value};

pub const PLUGIN_ID: &str = "ilium.core.scenes";
pub const CATALOG_SERVICE: &str = "ilium.core.scenes/catalog";
pub const POINT_PACKS: &str = "ilium.core.scenes/scene-packs";
pub const POINT_SOURCES: &str = "ilium.core.scenes/scene-data-sources";

/// 2000-01-01 12:00:00 UTC (J2000.0) in Unix milliseconds: the sidereal formula of the page sample gives day 0.
pub const J2000_UNIX_MILLIS: u64 = 946_728_000_000;
/// One sidereal day in milliseconds (86,164.0905 s rounded to the millisecond).
pub const SIDEREAL_DAY_MILLIS: u64 = 86_164_091;

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

/// Root of the worktree, found from the crate directory (plugins/core/scenes).
pub fn workspace_root() -> PathBuf {
    crate_dir().join("../../..")
}

/// The text of a manual page by file stem, for example `core-plugin-scenes`.
pub fn page(stem: &str) -> String {
    let path = workspace_root()
        .join("docs/plugins-manual/pages")
        .join(format!("{stem}.md"));
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("page {path:?}: {error}"))
}

/// The manual page of this plugin.
pub fn scenes_page() -> String {
    page("core-plugin-scenes")
}

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

/// The contributed scene entries of the manifest.
pub fn scene_entries() -> Vec<Value> {
    manifest_value()["contributes"]["scenes"]
        .as_array()
        .expect("contributes.scenes is an array")
        .clone()
}

/// One contributed scene entry by id.
pub fn scene_entry(id: &str) -> Value {
    scene_entries()
        .into_iter()
        .find(|entry| entry["id"] == id)
        .unwrap_or_else(|| panic!("scene {id} in the manifest"))
}

/// The three scene ids of the manifest, in page order.
pub const MANIFEST_SCENES: [&str; 3] = ["waves", "clock", "stars-overhead"];

/// The scene spec JSON the host passes to the constructor (api-scenes.md "Lifecycle of a renderer").
pub fn spec(scene: &str, settings: Value, palette: &[u32], mode: &str) -> String {
    json!({ "scene": scene, "settings": settings, "palette": palette, "mode": mode }).to_string()
}

/// A frame input of the given pixel size at `time_millis` since the scene started, at J2000 plus that time.
pub fn frame(width: u32, height: u32, time_millis: u64) -> FrameInput {
    FrameInput {
        time_millis,
        wall_millis: time_millis,
        now_unix_millis: J2000_UNIX_MILLIS + time_millis,
        width,
        height,
        palette_rgb: Vec::new(),
    }
}

/// A frame input for a scene of `columns` by `rows` cells at 2 by 4 pixels per cell (Braille).
pub fn braille_frame(columns: u16, rows: u16, time_millis: u64) -> FrameInput {
    frame(u32::from(columns) * 2, u32::from(rows) * 4, time_millis)
}

/// A cell counts as blank when it shows nothing: a space, an empty string or the empty Braille cell.
pub fn is_blank(symbol: &str) -> bool {
    symbol.is_empty() || symbol.chars().all(|ch| ch == ' ' || ch == '\u{2800}')
}

/// Positions `(row, column)` of the cells that show something.
pub fn lit_cells(screen: &Screen) -> Vec<(u16, u16)> {
    let mut lit = Vec::new();
    for row in 0..screen.rows {
        for column in 0..screen.columns {
            if !is_blank(&screen.cell(row, column).symbol) {
                lit.push((row, column));
            }
        }
    }
    lit
}

/// Share of cells that are equal in both screens (1.0 means identical).
pub fn equal_share(left: &Screen, right: &Screen) -> f64 {
    assert_eq!((left.columns, left.rows), (right.columns, right.rows));
    let total = u32::from(left.columns) * u32::from(left.rows);
    let mut equal = 0u32;
    for row in 0..left.rows {
        for column in 0..left.columns {
            if left.cell(row, column).symbol == right.cell(row, column).symbol {
                equal += 1;
            }
        }
    }
    f64::from(equal) / f64::from(total.max(1))
}

pub fn session_with_plugin() -> FakeSession {
    let mut session = FakeSession::new();
    session.install(crate_dir()).start();
    session
}

/// Calls a method of the catalog service. The method travels as the `method` field of the request (see API_GAPS.md).
pub fn catalog_call(
    session: &mut FakeSession,
    method: &str,
    extra: Value,
) -> Result<Value, ilium_plugin_sdk::types::PluginError> {
    let mut request = json!({ "method": method });
    if let (Some(target), Some(source)) = (request.as_object_mut(), extra.as_object()) {
        for (key, value) in source {
            target.insert(key.clone(), value.clone());
        }
    }
    let reply =
        session.call_service("test.caller", CATALOG_SERVICE, "^0.1", &request.to_string())?;
    match reply {
        ilium_plugin_sdk::exports::ServiceReply::Done(text) => {
            Ok(serde_json::from_str(&text).expect("catalog reply is JSON"))
        }
        other => panic!("catalog answers synchronously, got {other:?}"),
    }
}

/// The scenes the catalog lists, as JSON objects (`scenes` array of the `list` reply, see API_GAPS.md).
pub fn catalog_scenes(session: &mut FakeSession) -> Vec<Value> {
    let reply = catalog_call(session, "list", json!({})).expect("catalog list answers");
    reply["scenes"]
        .as_array()
        .unwrap_or_else(|| panic!("reply has a scenes array: {reply}"))
        .clone()
}

// ---------------------------------------------------------------------------------------------
// Gap helpers (see ../API_GAPS.md)
// ---------------------------------------------------------------------------------------------

/// A scene of `columns` by `rows` cells driven through the testkit (needs `ScenesScene: SceneRenderer`).
pub fn fake_scene(scene_id: &str, columns: u16, rows: u16) -> FakeScene {
    FakeScene::new::<ScenesScene>(scene_id, columns, rows)
}

/// The same with the observer position the `location` import returns (needs `FakeScene::with_location`).
pub fn fake_scene_at(
    scene_id: &str,
    columns: u16,
    rows: u16,
    latitude: f64,
    longitude: f64,
) -> FakeScene {
    fake_scene(scene_id, columns, rows).with_location(latitude, longitude)
}

/// The same with the scene settings of the spec (needs `FakeScene::with_settings`).
pub fn fake_scene_with(scene_id: &str, columns: u16, rows: u16, settings: &Value) -> FakeScene {
    fake_scene(scene_id, columns, rows).with_settings(&settings.to_string())
}

/// The same with both observer and settings.
pub fn fake_scene_at_with(
    scene_id: &str,
    columns: u16,
    rows: u16,
    latitude: f64,
    longitude: f64,
    settings: &Value,
) -> FakeScene {
    fake_scene_with(scene_id, columns, rows, settings).with_location(latitude, longitude)
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

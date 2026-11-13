//! Helpers only the feature target mounts: the manual and user-doc readers, the manifest, golden
//! paths, the clear-queue and board fixtures, and the schedule dialog filler. The port target does
//! not mount this module, so none of these items is dead code there.

use std::path::PathBuf;

use ilium_plugin_sdk::testing::FakeSession;
use ilium_plugin_sdk::types::NodeId;
use serde_json::Value;

use crate::common::{invoke_menu, set_flag, set_text, SCHEDULE_COMMAND, SCHEDULE_VIEW};

/// Assumed command id that clears a queue (API_GAPS.md).
pub const CLEAR_COMMAND: &str = "ilium.core.scheduler.clear-queue";

/// Content kind of a kanban board pane (ilium-core PaneContentKind).
pub const BOARD_KIND: &str = "ilium.core.kanban.board";

pub fn manifest_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("manifest.json")
}

pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

pub fn golden_path(name: &str) -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/features/golden")
        .join(name)
        .to_string_lossy()
        .into_owned()
}

pub fn page_text() -> String {
    let path = workspace_root().join("docs/plugins-manual/pages/core-plugin-scheduler.md");
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("page {path:?}: {error}"))
}

pub fn manual_page(name: &str) -> String {
    let path = workspace_root()
        .join("docs/plugins-manual/pages")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("page {path:?}: {error}"))
}

pub fn user_doc(name: &str) -> String {
    let path = workspace_root().join("src/docs").join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{path:?}: {error}"))
}

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

/// Rows of the behaviour contract table: `(id, rule, exact value)`.
pub fn behaviour_rows() -> Vec<(String, String, String)> {
    page_text()
        .lines()
        .filter(|line| line.starts_with("| scheduler-B"))
        .map(|line| {
            let cells: Vec<String> = line
                .trim_matches('|')
                .split('|')
                .map(|c| c.trim().to_string())
                .collect();
            (cells[0].clone(), cells[1].clone(), cells[2].clone())
        })
        .collect()
}

pub fn behaviour_value(id: &str) -> String {
    behaviour_rows()
        .into_iter()
        .find(|row| row.0 == id)
        .unwrap_or_else(|| panic!("row {id}"))
        .2
}

/// Opens the schedule dialog for a terminal pane and fills it.
pub fn fill_schedule_dialog(
    session: &mut FakeSession,
    pane: NodeId,
    hours: &str,
    minutes: &str,
    seconds: &str,
    payload: &str,
    send_enter: bool,
) {
    invoke_menu(session, SCHEDULE_COMMAND, pane);
    set_text(session, SCHEDULE_VIEW, "hours", hours);
    set_text(session, SCHEDULE_VIEW, "minutes", minutes);
    set_text(session, SCHEDULE_VIEW, "seconds", seconds);
    set_text(session, SCHEDULE_VIEW, "payload", payload);
    set_flag(session, SCHEDULE_VIEW, "send-enter", send_enter);
}

//! Feature-only helpers: used by the `features` test target alone (the `port` target mounts only common.rs).

use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::Outcome;
use serde_json::{json, Value};

use crate::manual_pages::page;

/// The manual page of this plugin.
pub fn page_text() -> String {
    page("core-plugin-fleet")
}

/// Path of a golden file in this directory.
pub fn golden_path(name: &str) -> String {
    format!(
        "{}/tests/features/golden/{name}",
        env!("CARGO_MANIFEST_DIR")
    )
}

/// One pane record as the `list-panes` cross-session reply carries it (cli-reference.md, `ilium panes`).
pub fn pane_json(
    pane_id: u64,
    name: &str,
    kind: &str,
    agent: Option<&str>,
    state: Option<&str>,
) -> Value {
    json!({
        "pane_id": pane_id,
        "name": name,
        "kind": kind,
        "agent": agent,
        "state": state,
        "project": "/home/me/dev/ilium",
        "cwd": "/home/me/dev/ilium",
        "is_self": false,
    })
}

/// Registers a live session that cannot be read (`session-unreachable`).
pub fn add_unreachable_session(host: &mut FakeHost, name: &str) {
    host.sessions().add_unreachable(name, "/home/me/dev/ilium");
}

/// Makes every later submission complete with the given outcome.
pub fn auto_outcome(host: &mut FakeHost, outcome: Outcome) {
    host.auto_outcome(outcome);
}

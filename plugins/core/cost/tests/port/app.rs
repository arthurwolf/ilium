//! Twins of the cost tests in ilium-client/src/app.rs: settings changes persist immediately.
//!
//! `app.cost-row` applies a row adjustment to a fresh app with a temporary config directory and answers the
//! app state plus the settings read back from the saved `config.toml`.

use serde_json::{json, Value};

use crate::common::client;

fn run(request: Value) -> Value {
    client("app.cost-row", request)
}

/// REQ: PORT:ilium-client/src/app.rs::an_unchanged_cost_row_does_not_touch_the_config_file
#[test]
fn port_app_an_unchanged_cost_row_does_not_touch_the_config_file() {
    // Selecting the already-selected calibration changes nothing.
    let reply = run(json!({ "adjust": [{ "row": "calibration:own_history", "direction": 0 }] }));
    assert_eq!(reply["config_file_exists"], json!(false));
}

/// REQ: PORT:ilium-client/src/app.rs::cost_controls_persist_immediately_and_sorting_follows_the_switch
#[test]
fn port_app_cost_controls_persist_immediately_and_sorting_follows_the_switch() {
    let reply = run(json!({
        "adjust": [{ "row": "display:sparkline", "direction": 0 }, { "row": "visibility:sparkline", "direction": 0 }],
    }));
    assert_eq!(reply["settings"]["sparkline"]["enabled"], json!(true));
    assert_eq!(
        reply["settings"]["sparkline"]["visibility"],
        json!("always")
    );
    assert_eq!(reply["persisted_cost"], reply["settings"]);

    let sorted = run(json!({ "adjust": [{ "row": "sort_by_cost", "direction": 0 }] }));
    assert_eq!(sorted["effective_tree_order"], json!("cost_descending"));
    assert_eq!(
        run(json!({}))["effective_tree_order"],
        run(json!({}))["configured_tree_order"]
    );

    // A direct manual move leaves every automatic view, cost order included.
    let manual = run(
        json!({ "adjust": [{ "row": "sort_by_cost", "direction": 0 }], "manual_tree_mutation": true }),
    );
    assert_eq!(manual["settings"]["sort_by_cost"], json!(false));
    assert_eq!(manual["effective_tree_order"], json!("manual"));
    assert_eq!(manual["persisted_cost"]["sort_by_cost"], json!(false));
}

/// REQ: PORT:ilium-client/src/app.rs::reset_planning_controls_are_independent_and_persist_immediately
#[test]
fn port_app_reset_planning_controls_are_independent_and_persist_immediately() {
    let initial = run(json!({}));
    assert_eq!(initial["reset_planning"]["monitor_claude"], json!(true));
    assert_eq!(initial["reset_planning"]["monitor_codex"], json!(true));
    let reply = run(json!({ "reset_planning_rows": [0, 2] }));
    assert_eq!(reply["reset_planning"]["monitor_claude"], json!(false));
    assert_eq!(reply["reset_planning"]["monitor_codex"], json!(true));
    assert_eq!(reply["reset_planning"]["time_style"], json!("human"));
    assert_eq!(reply["persisted_reset_planning"], reply["reset_planning"]);
}

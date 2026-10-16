//! The extension points, service and topic the page declares: `card-actions` (collect), `card-moved`
//! (observe), service `ilium.core.kanban/boards`, topic `ilium.core.kanban/card-moved`.
//!
//! The kernel invokes hooks and collects contributions, so these tests read the tables of the page and
//! use the core stub of the session harness (see API_GAPS.md for what a real contributor test needs).

use ilium_plugin_sdk::testing::FakeSession;
use ilium_plugin_sdk::exports::{HookCall, HookReply, ServiceReply};
use serde_json::Value;

use crate::common::{json_block_after, page_text, workspace_root};

const PLUGIN: &str = "ilium.core.kanban";

/// Rows of the Extension points table: `(point, kind, what, payload, risk)`.
fn point_rows() -> Vec<[String; 5]> {
    let page = page_text();
    let start = page.find("| Point | Kind |").expect("extension point table");
    page[start..]
        .lines()
        .skip(2)
        .take_while(|line| line.starts_with('|'))
        .map(|line| {
            let cells: Vec<String> = line.trim_matches('|').split('|').map(|cell| cell.trim().to_string()).collect();
            [cells[0].clone(), cells[1].clone(), cells[2].clone(), cells[3].clone(), cells[4].clone()]
        })
        .collect()
}

fn reference_page() -> String {
    let path = workspace_root().join("docs/plugins-manual/pages/reference-core-extension-points.md");
    std::fs::read_to_string(path).expect("core extension points page")
}

/// REQ: F-024
#[test]
fn points_024_the_page_lists_exactly_two_points() {
    let names: Vec<String> = point_rows().into_iter().map(|row| row[0].trim_matches('`').to_string()).collect();
    assert_eq!(names, ["card-actions", "card-moved"]);
}

/// REQ: F-024
#[test]
fn points_024_card_actions_is_a_collect_point_with_entry_id_title_command() {
    let rows = point_rows();
    let row = rows.iter().find(|row| row[0] == "`card-actions`").expect("row");
    assert_eq!(row[1], "collect");
    assert_eq!(row[2], "Add an action to the card context menu.");
    assert_eq!(row[3], "entry: id, title, command");
    assert_eq!(row[4], "standard");
}

/// REQ: F-024
#[test]
fn points_024_card_moved_is_an_observe_hook_with_the_documented_payload() {
    let rows = point_rows();
    let row = rows.iter().find(|row| row[0] == "`card-moved`").expect("row");
    assert_eq!(row[1], "hook (observe)");
    assert_eq!(row[2], "Learn that a card changed column.");
    assert_eq!(row[3], "board, card, from_column, to_column");
    assert_eq!(row[4], "standard");
}

/// REQ: F-024
#[test]
fn points_024_every_point_is_standard_risk() {
    assert!(point_rows().iter().all(|row| row[4] == "standard"));
}

/// REQ: F-024
#[test]
fn points_024_the_reference_page_agrees_with_the_plugin_page() {
    let reference = reference_page();
    assert!(reference.contains("| `ilium.core.kanban/card-actions` | collect | standard | Add an action to the card context menu. |"));
    assert!(reference.contains("| `ilium.core.kanban/card-moved` | hook: observe | standard | Learn that a card changed column. |"));
    assert!(reference.contains("| [Kanban](/pages/core-plugin-kanban/) | `ilium.core.kanban` | 2 | 1 | 1 | yes |"));
}

/// REQ: F-024
#[test]
fn points_024_the_service_is_boards_with_four_methods() {
    let reference = reference_page();
    assert!(reference.contains("| `ilium.core.kanban/boards` | list, get, move-card, add-card: read and edit boards |"));
    assert!(page_text().contains("`ilium.core.kanban/boards`: list, get, move-card, add-card: read and edit boards"));
}

/// REQ: F-024
#[test]
fn points_024_the_topic_card_moved_carries_board_card_and_to_column() {
    let reference = reference_page();
    assert!(reference.contains("| `ilium.core.kanban/card-moved` | { board, card, to_column } |"));
    assert!(page_text().contains("`ilium.core.kanban/card-moved`: { board, card, to_column }"));
}

/// REQ: F-024
#[test]
fn points_024_the_core_plugin_is_replaceable_with_a_replaces_list() {
    let page = page_text();
    assert!(page.contains("**Replaceable:** yes. A replacement lists `\"replaces\": [\"ilium.core.kanban\"]`"));
}

/// REQ: F-024
#[test]
fn points_024_the_example_package_observes_card_moved_with_a_handler() {
    let page = page_text();
    let example = json_block_after(&page, "#### Example: Posts a toast");
    assert_eq!(example["exports"], serde_json::json!(["hook-handler"]));
    assert_eq!(example["capabilities"]["client"], serde_json::json!(["extensions.contribute:ilium.core.kanban/card-moved"]));
    let contribution = &example["contributes"]["extensions"][0];
    assert_eq!(contribution["point"], "ilium.core.kanban/card-moved");
    assert_eq!(contribution["id"], "notify-done");
    assert_eq!(contribution["priority"], 50);
    assert_eq!(contribution["handler"], "notify_done");
}

/// REQ: F-024
#[test]
fn points_024_a_late_trapping_or_wrong_mode_handler_counts_as_pass() {
    assert!(page_text().contains("A handler that is late, traps or returns a reply the mode does not allow counts as `pass`."));
}

/// REQ: F-024
#[test]
fn points_024_the_stub_hook_reply_for_card_moved_is_pass() {
    let mut session = FakeSession::new();
    session.install_core(PLUGIN).start();
    let reply = session
        .run_hook(
            PLUGIN,
            HookCall {
                call: 1,
                point: "ilium.core.kanban/card-moved".to_string(),
                payload_json: r#"{"board":"plan.md","card":"c1","from_column":0,"to_column":2}"#.to_string(),
            },
        )
        .expect("hook runs");
    assert_eq!(reply, HookReply::Pass);
}

/// REQ: F-024
#[test]
fn points_024_the_stub_serves_the_boards_service_with_ok_true() {
    let mut session = FakeSession::new();
    session.install_core(PLUGIN).start();
    for method in ["list", "get", "move-card", "add-card"] {
        let request = format!(r#"{{"method":"{method}"}}"#);
        let reply = session.call_service(PLUGIN, "ilium.core.kanban/boards", "*", &request).expect("service answers");
        let ServiceReply::Done(json) = reply else { panic!("deferred reply for {method}") };
        let value: Value = serde_json::from_str(&json).expect("JSON reply");
        assert_eq!(value, serde_json::json!({"ok": true}), "{method}");
    }
}

/// REQ: F-024
#[test]
fn points_024_the_harness_page_lists_the_same_two_points_for_the_core_stub() {
    let path = workspace_root().join("docs/plugins-manual/pages/reference-test-harness.md");
    let harness = std::fs::read_to_string(path).expect("harness page");
    assert!(harness.contains("| `ilium.core.kanban` | `card-actions` (collect), `card-moved` (observe) | none | collect: empty list; hooks: `pass` |"));
}

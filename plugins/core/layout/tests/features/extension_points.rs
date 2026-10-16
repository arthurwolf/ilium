//! Extension points, services, topics and the contribution example of the page.

use serde_json::Value;

use crate::common::*;

fn core_points_page() -> String {
    extension_points_page_text()
}

/// REQ: F-003
#[test]
fn doc_f_003_page_lists_layout_presets_as_a_collect_point() {
    let page = page_text();
    let row = page.lines().find(|line| line.starts_with("| `layout-presets` |")).expect("row");
    let cells: Vec<&str> = row.split('|').map(str::trim).collect();
    assert_eq!(cells[2], "collect");
    assert_eq!(cells[5], "standard");
}

/// REQ: F-003
#[test]
fn doc_f_003_page_lists_before_split_as_a_veto_hook_with_node_direction_pane_count() {
    let page = page_text();
    let row = page.lines().find(|line| line.starts_with("| `before-split` |")).expect("row");
    let cells: Vec<&str> = row.split('|').map(str::trim).collect();
    assert_eq!(cells[2], "hook (veto)");
    assert_eq!(cells[4], "node, direction, pane_count");
    assert_eq!(cells[5], "standard");
}

/// REQ: F-003
#[test]
fn doc_f_003_core_extension_points_page_agrees_with_the_plugin_page() {
    let page = core_points_page();
    assert!(page.contains("| `ilium.core.layout/layout-presets` | collect | standard |"));
    assert!(page.contains("| `ilium.core.layout/before-split` | hook: veto | standard |"));
    assert!(page.contains("| `ilium.core.layout/presets` | list, apply:"));
    assert!(page.contains("| `ilium.core.layout/changed` | { group, split_count }"));
}

/// REQ: F-003
#[test]
fn doc_f_003_layout_owns_two_extension_points_one_service_one_topic_and_is_replaceable() {
    let page = core_points_page();
    let row = page
        .lines()
        .find(|line| line.starts_with("| [Layout](/pages/core-plugin-layout/)"))
        .expect("summary row");
    let cells: Vec<&str> = row.split('|').map(str::trim).collect();
    assert_eq!(&cells[3..7], ["2", "1", "1", "yes"]);
}

/// REQ: F-003
#[test]
fn doc_f_003_page_states_the_service_the_topic_and_replaceable_yes() {
    let page = page_text();
    assert!(page.contains("- `ilium.core.layout/presets`: list, apply"));
    assert!(page.contains("- `ilium.core.layout/changed`: { group, split_count }"));
    assert!(page.contains("**Replaceable:** yes."));
}

/// REQ: F-003
#[test]
fn doc_f_003_review_trio_example_contributes_one_preset_with_a_template() {
    let blocks = json_blocks(&page_text());
    let example: Value = serde_json::from_str(blocks.last().expect("example")).expect("JSON");
    assert_eq!(example["api"], "ilium:plugin@0.1.0");
    assert_eq!(
        example["capabilities"]["server"][0],
        "extensions.contribute:ilium.core.layout/layout-presets"
    );
    let entry = &example["contributes"]["extensions"][0];
    assert_eq!(entry["point"], "ilium.core.layout/layout-presets");
    assert_eq!(entry["id"], "review-trio");
    assert_eq!(entry["priority"], 50);
    assert_eq!(entry["entry"]["title"], "Review trio");
    assert_eq!(entry["entry"]["template"], "h(agent, v(editor, terminal))");
}

/// REQ: F-003
#[test]
fn f_003_the_published_changed_topic_carries_group_and_split_count() {
    // The plugin publishes ilium.core.layout/changed after a split is added, removed or resized.
    let mut host = activated_server();
    let focused = host.tree().add(pane_node("api", "ilium.terminal", None));
    host.deliver_one(message_from_client("split-right", id_bytes(focused)));
    host.settle();
    let topics = host.published_topics();
    let changed: Vec<&(String, String)> = topics.iter().filter(|(topic, _)| topic == "ilium.core.layout/changed").collect();
    assert_eq!(changed.len(), 1);
    let payload: Value = serde_json::from_str(&changed[0].1).expect("payload is JSON");
    assert!(payload.get("group").is_some());
    assert_eq!(payload["split_count"], 1);
}

/// REQ: F-003
#[test]
fn f_003_no_changed_topic_is_published_when_the_split_is_refused() {
    let mut host = activated_server();
    let focused = host.tree().add(pane_node("api", "ilium.terminal", None));
    host.revoke("tree.restructure");
    host.deliver_one(message_from_client("split-right", id_bytes(focused)));
    host.settle();
    assert!(host.published_topics().iter().all(|(topic, _)| topic != "ilium.core.layout/changed"));
}

//! The two extension points, the service and the contribution example of the page (F-112).

use serde_json::{json, Value};

use crate::common::*;

fn point(id: &str) -> Value {
    manifest_value()["contributes"]["extension_points"]
        .as_array()
        .expect("array")
        .iter()
        .find(|entry| entry["id"] == id)
        .cloned()
        .expect("point declared")
}

/// REQ: F-112
#[test]
fn doc_f_112_fact_sources_is_a_collect_point_with_sixteen_static_entries() {
    let entry = point("fact-sources");
    assert_eq!(entry["kind"], "collect");
    assert_eq!(entry["title"], "Fact sources");
    assert_eq!(entry["max_entries"], 16);
    assert_eq!(entry["dynamic"], false);
    assert_eq!(entry["version"], "0.1.0");
}

/// REQ: F-112
#[test]
fn doc_f_112_fact_sources_entry_requires_id_title_and_service_strings() {
    let schema = &point("fact-sources")["entry_schema"];
    assert_eq!(schema["required"], json!(["id", "title", "service"]));
    for key in ["id", "title", "service"] {
        assert_eq!(schema["properties"][key]["type"], "string", "property {key}");
    }
}

/// REQ: F-112
#[test]
fn doc_f_112_before_recap_is_a_transform_hook_with_20_ms_deadline_and_8_handlers() {
    let entry = point("before-recap");
    assert_eq!(entry["kind"], "hook");
    assert_eq!(entry["mode"], "transform");
    assert_eq!(entry["title"], "Before recap");
    assert_eq!(entry["risk"], "standard");
    assert_eq!(entry["deadline_ms"], 20);
    assert_eq!(entry["max_handlers"], 8);
    assert_eq!(entry["version"], "0.1.0");
}

/// REQ: F-112
#[test]
fn doc_f_112_before_recap_payload_and_reply_cap_the_text_at_65536_characters() {
    let entry = point("before-recap");
    assert_eq!(entry["payload_schema"]["properties"]["text"]["maxLength"], 65536);
    assert_eq!(entry["reply_schema"]["properties"]["text"]["maxLength"], 65536);
    assert_eq!(entry["payload_schema"]["properties"]["pane"]["type"], "integer");
    assert_eq!(entry["payload_schema"]["properties"]["kind"]["enum"], json!(["short", "long"]));
}

/// REQ: F-112
#[test]
fn doc_f_112_page_table_lists_both_points_as_standard_risk() {
    let page = page_text();
    let collect = table_row(&page, "| `fact-sources` |");
    assert_eq!(collect[1], "collect");
    assert_eq!(collect[4], "standard");
    let hook = table_row(&page, "| `before-recap` |");
    assert_eq!(hook[1], "hook (transform)");
    assert_eq!(hook[3], "pane, kind (short, long), text; reply: text");
    assert_eq!(hook[4], "standard");
}

/// REQ: F-112
#[test]
fn doc_f_112_core_points_page_agrees_with_the_plugin_page() {
    let page = pages_text("reference-core-extension-points.md");
    assert!(page.contains("| `ilium.core.recap/fact-sources` | collect | standard |"));
    assert!(page.contains("| `ilium.core.recap/before-recap` | hook: transform | standard |"));
    assert!(page.contains("| `ilium.core.recap/recap` | get: short or long recap of panes |"));
    let counts = table_row(&page, "| [Recap](/pages/core-plugin-recap/)");
    assert_eq!(&counts[2..6], ["2", "1", "0", "yes"]);
}

/// REQ: F-112
#[test]
fn doc_f_112_service_get_takes_panes_and_a_length_of_short_long_or_both() {
    let get = &manifest_value()["contributes"]["services"]["ilium.core.recap/recap"]["get"];
    assert_eq!(get["params"]["required"], json!(["panes"]));
    assert_eq!(get["params"]["properties"]["panes"], json!({"type": "array", "items": {"type": "integer"}}));
    assert_eq!(get["params"]["properties"]["length"]["enum"], json!(["short", "long", "both"]));
    assert_eq!(get["result"]["required"], json!(["recaps"]));
}

/// REQ: F-112
#[test]
fn doc_f_112_page_states_the_service_and_replacement_rule() {
    let page = page_text();
    assert!(page.contains("- `ilium.core.recap/recap`: get: short or long recap of panes"));
    assert!(page.contains("**Replaceable:** yes."));
    assert!(page.contains("`\"replaces\": [\"ilium.core.recap\"]`"));
}

/// REQ: F-112
#[test]
fn doc_f_112_contribution_example_targets_fact_sources_with_title_and_service() {
    let blocks = json_blocks(&page_text());
    let example: Value = serde_json::from_str(blocks.last().expect("example block")).expect("JSON");
    assert_eq!(example["capabilities"]["server"], json!(["extensions.contribute:ilium.core.recap/fact-sources"]));
    let entry = &example["contributes"]["extensions"][0];
    assert_eq!(entry["point"], "ilium.core.recap/fact-sources");
    assert_eq!(entry["id"], "branch-ticket");
    assert_eq!(entry["priority"], 50);
    assert_eq!(entry["entry"], json!({"title": "Branch ticket", "service": "example.tickets/for-pane"}));
}

/// REQ: F-112
#[test]
fn f_112_the_example_entry_satisfies_the_fact_sources_entry_schema() {
    let entry = json!({"title": "Branch ticket", "service": "example.tickets/for-pane"});
    let required = point("fact-sources")["entry_schema"]["required"].clone();
    for key in required.as_array().expect("array") {
        let key = key.as_str().expect("string");
        // `id` comes from the contribution itself; title and service come from the entry.
        assert!(key == "id" || entry.get(key).is_some(), "entry lacks {key}");
    }
}

//! Extension points, the service, the topic and the contribution example of the page.

use serde_json::Value;

use crate::common::*;

fn points_page() -> String {
    page("reference-core-extension-points")
}

fn example() -> Value {
    let blocks = json_blocks(&page_text());
    serde_json::from_str(blocks.last().expect("example block")).expect("example is JSON")
}

/// REQ: F-040
#[test]
fn doc_f_040_page_lists_price_sources_as_a_standard_collect_point() {
    let row = table_row(&page_text(), "`price-sources`");
    assert_eq!(row[1], "collect");
    assert_eq!(
        row[2],
        "Supply a price table for models the built-in table does not know."
    );
    assert_eq!(row[4], "standard");
}

/// REQ: F-040
#[test]
fn doc_f_040_price_sources_payload_has_the_six_entry_fields_in_order() {
    let row = table_row(&page_text(), "`price-sources`");
    assert_eq!(
        row[3],
        "entry: id, model_pattern, input_per_mtok, output_per_mtok, cache_read_per_mtok, currency"
    );
}

/// REQ: F-038
#[test]
fn doc_f_038_page_lists_quota_sources_as_a_standard_collect_point() {
    let row = table_row(&page_text(), "`quota-sources`");
    assert_eq!(row[1], "collect");
    assert_eq!(
        row[2],
        "Supply a plan quota or remaining allowance for a provider."
    );
    assert_eq!(
        row[3],
        "entry: id, provider, service (a service the plugin provides)"
    );
    assert_eq!(row[4], "standard");
}

/// REQ: F-038, F-040
#[test]
fn doc_f_040_the_plugin_owns_exactly_two_points() {
    let page = page_text();
    let table_rows = page
        .lines()
        .filter(|line| {
            line.starts_with("| `price-sources`") || line.starts_with("| `quota-sources`")
        })
        .count();
    assert_eq!(table_rows, 2);
    let row = table_row(&points_page(), "[Cost](/pages/core-plugin-cost/)");
    assert_eq!(&row[1..], ["`ilium.core.cost`", "2", "1", "1", "yes"]);
}

/// REQ: F-040
#[test]
fn doc_f_040_core_extension_points_page_agrees_on_both_points() {
    let page = points_page();
    assert!(page.contains(
        "| `ilium.core.cost/price-sources` | collect | standard | Supply a price table for models the built-in table does not know. |"
    ));
    assert!(page.contains(
        "| `ilium.core.cost/quota-sources` | collect | standard | Supply a plan quota or remaining allowance for a provider. |"
    ));
}

/// REQ: F-038
#[test]
fn doc_f_038_page_states_the_usage_service_and_core_page_agrees() {
    assert!(page_text()
        .contains("- `ilium.core.cost/usage`: get: spend and tokens per pane, session or day"));
    assert!(points_page()
        .contains("| `ilium.core.cost/usage` | get: spend and tokens per pane, session or day |"));
}

/// REQ: F-038
#[test]
fn doc_f_038_page_states_the_usage_changed_topic_and_core_page_agrees() {
    assert!(page_text().contains(
        "- `ilium.core.cost/usage-changed`: { pane, total } when the total of a pane changes"
    ));
    assert!(points_page().contains(
        "| `ilium.core.cost/usage-changed` | { pane, total } when the total of a pane changes |"
    ));
}

/// REQ: F-040
#[test]
fn doc_f_040_page_says_cost_is_replaceable_and_how() {
    let page = page_text();
    assert!(page.contains(
        "**Replaceable:** yes. A replacement lists `\"replaces\": [\"ilium.core.cost\"]`"
    ));
}

/// REQ: F-040
#[test]
fn doc_f_040_extension_grants_follow_provide_and_contribute_naming() {
    let page = page_text();
    assert!(page.contains("(grant `extensions.provide:ilium.core.cost/<point>`)"));
    assert!(page.contains("Contributors need `extensions.contribute:ilium.core.cost/<point>`"));
}

/// REQ: F-040
#[test]
fn doc_f_040_local_model_example_contributes_one_zero_price_entry() {
    let example = example();
    assert_eq!(example["api"], "ilium:plugin@0.1.0");
    assert_eq!(
        example["capabilities"]["server"][0],
        "extensions.contribute:ilium.core.cost/price-sources"
    );
    let entry = &example["contributes"]["extensions"][0];
    assert_eq!(entry["point"], "ilium.core.cost/price-sources");
    assert_eq!(entry["id"], "local-llm");
    assert_eq!(entry["priority"], 50);
    assert_eq!(entry["entry"]["model_pattern"], "qwen3-*");
    assert_eq!(entry["entry"]["input_per_mtok"], 0);
    assert_eq!(entry["entry"]["output_per_mtok"], 0);
    assert_eq!(entry["entry"]["currency"], "USD");
}

/// A price entry names its model by pattern, a currency and the per-million-token prices (page table).
///
/// REQ: F-040
#[test]
fn doc_f_040_example_entry_uses_only_fields_of_the_price_sources_payload() {
    let example = example();
    let entry = example["contributes"]["extensions"][0]["entry"]
        .as_object()
        .expect("entry")
        .clone();
    let allowed = [
        "id",
        "model_pattern",
        "input_per_mtok",
        "output_per_mtok",
        "cache_read_per_mtok",
        "currency",
    ];
    for key in entry.keys() {
        assert!(allowed.contains(&key.as_str()), "unexpected field {key}");
    }
}

/// REQ: F-040
#[test]
fn doc_f_040_page_says_the_example_needs_no_code() {
    assert!(page_text().contains("A package adds this entry in its manifest. It needs no code."));
}

/// The test harness stub of the Cost plugin exposes the same two points and the one service (DEV-483).
///
/// REQ: F-040, F-038
#[test]
fn doc_f_040_harness_core_stub_row_lists_the_same_points_and_service() {
    let row = table_row(&page("reference-test-harness"), "`ilium.core.cost`");
    assert_eq!(
        row[1],
        "`price-sources` (collect), `quota-sources` (collect)"
    );
    assert_eq!(row[2], "`usage` (get)");
}

/// REQ: F-088
#[test]
fn doc_f_088_reset_forecasts_use_http_timers_and_request_completed() {
    let page = page_text();
    assert!(page.contains("once an hour, using a durable timer"));
    assert!(page.contains("The completion arrives as `request-completed`"));
}

//! Extension points, the service, the topic and the contribution example of the page.

use serde_json::{json, Value};

use crate::common::*;

fn points_page() -> String {
    page("reference-core-extension-points")
}

fn example() -> Value {
    let blocks = json_blocks(&page_text());
    serde_json::from_str(blocks.last().expect("example block")).expect("example is JSON")
}

fn is_hex_colour(text: &str) -> bool {
    text.len() == 7 && text.starts_with('#') && text[1..].chars().all(|symbol| symbol.is_ascii_hexdigit())
}

/// REQ: F-093
#[test]
fn doc_f_093_page_lists_palettes_as_a_standard_collect_point() {
    let row = table_row(&page_text(), "`palettes`");
    assert_eq!(row[1], "collect");
    assert_eq!(row[2], "Add a palette to the shared palette choice.");
    assert_eq!(row[3], "entry: id, title, stops (list of colours)");
    assert_eq!(row[4], "standard");
}

/// REQ: F-093, F-094
#[test]
fn doc_f_094_page_lists_style_presets_as_a_standard_collect_point() {
    let row = table_row(&page_text(), "`style-presets`");
    assert_eq!(row[1], "collect");
    assert_eq!(row[2], "Add a style preset: a full set of shared look values.");
    assert_eq!(row[3], "entry: id, title, values");
    assert_eq!(row[4], "standard");
}

/// REQ: F-093, F-094
#[test]
fn doc_f_093_the_plugin_owns_exactly_two_points_and_the_index_page_agrees() {
    let page = page_text();
    let table_rows =
        page.lines().filter(|line| line.starts_with("| `palettes`") || line.starts_with("| `style-presets`")).count();
    assert_eq!(table_rows, 2);
    let row = table_row(&points_page(), "[Ambient look](/pages/core-plugin-ambient/)");
    assert_eq!(&row[1..], ["`ilium.core.ambient`", "2", "1", "1", "yes"]);
}

/// REQ: F-093, F-094
#[test]
fn doc_f_093_core_extension_points_page_agrees_on_both_points() {
    let page = points_page();
    assert!(page.contains("| `ilium.core.ambient/palettes` | collect | standard | Add a palette to the shared palette choice. |"));
    assert!(page.contains(
        "| `ilium.core.ambient/style-presets` | collect | standard | Add a style preset: a full set of shared look values. |"
    ));
}

/// REQ: F-093
#[test]
fn doc_f_093_page_states_the_look_service_and_core_page_agrees() {
    assert!(page_text().contains("- `ilium.core.ambient/look`: get: current shared look and palette"));
    assert!(points_page().contains("| `ilium.core.ambient/look` | get: current shared look and palette |"));
}

/// REQ: F-093
#[test]
fn doc_f_093_page_states_the_look_changed_topic_and_core_page_agrees() {
    assert!(page_text().contains("- `ilium.core.ambient/look-changed`: { palette, mode }"));
    assert!(points_page().contains("| `ilium.core.ambient/look-changed` | { palette, mode } |"));
}

/// REQ: F-093
#[test]
fn doc_f_093_page_says_the_plugin_is_replaceable_and_how() {
    assert!(page_text().contains("**Replaceable:** yes. A replacement lists `\"replaces\": [\"ilium.core.ambient\"]`"));
}

/// REQ: F-093, F-094
#[test]
fn doc_f_093_extension_grants_follow_provide_and_contribute_naming() {
    let page = page_text();
    assert!(page.contains("(grant `extensions.provide:ilium.core.ambient/<point>`)"));
    assert!(page.contains("Contributors need `extensions.contribute:ilium.core.ambient/<point>`"));
}

/// REQ: F-093
#[test]
fn doc_f_093_page_says_every_scene_receives_the_palette_in_its_constructor() {
    assert!(page_text().contains("Every scene receives the palette in its constructor."));
}

/// REQ: F-093, F-094
#[test]
fn doc_f_093_harness_core_stub_row_lists_both_points() {
    let row = table_row(&page("reference-test-harness"), "`ilium.core.ambient`");
    assert_eq!(row[1], "`palettes` (collect), `style-presets` (collect)");
    assert_eq!(row[3], "collect: empty list");
}

/// The page offers a `look` service while the harness stub row says `none`: the stub must offer it too.
///
/// REQ: F-093
#[test]
fn doc_f_093_harness_core_stub_row_offers_the_look_service_the_page_declares() {
    let row = table_row(&page("reference-test-harness"), "`ilium.core.ambient`");
    assert_eq!(row[2], "`look`");
}

/// REQ: F-093
#[test]
fn doc_f_093_harbour_example_contributes_one_four_colour_palette_entry() {
    let example = example();
    assert_eq!(example["api"], "ilium:plugin@0.1.0");
    assert_eq!(example["capabilities"]["client"][0], "extensions.contribute:ilium.core.ambient/palettes");
    let entries = example["contributes"]["extensions"].as_array().expect("extensions");
    assert_eq!(entries.len(), 1);
    let entry = &entries[0];
    assert_eq!(entry["point"], "ilium.core.ambient/palettes");
    assert_eq!(entry["id"], "harbour");
    assert_eq!(entry["priority"], 50);
    assert_eq!(entry["entry"]["title"], "Harbour");
    assert_eq!(strings(&entry["entry"]["stops"]), ["#0b1d2a", "#1f6f8b", "#99d5c9", "#f6f1d1"]);
}

/// REQ: F-093
#[test]
fn doc_f_093_harbour_stops_are_six_digit_hex_colours() {
    let example = example();
    for stop in strings(&example["contributes"]["extensions"][0]["entry"]["stops"]) {
        assert!(is_hex_colour(&stop), "{stop}");
    }
}

/// A palette entry uses only the payload fields `id`, `title` and `stops` (the id sits beside `entry` in the manifest).
///
/// REQ: F-093
#[test]
fn doc_f_093_harbour_entry_uses_only_fields_of_the_palettes_payload() {
    let example = example();
    let entry = example["contributes"]["extensions"][0]["entry"].as_object().expect("entry").clone();
    for key in entry.keys() {
        assert!(["id", "title", "stops"].contains(&key.as_str()), "unexpected field {key}");
    }
}

/// REQ: F-093
#[test]
fn doc_f_093_page_says_the_example_needs_no_code() {
    assert!(page_text().contains("A package adds this entry in its manifest. It needs no code."));
}

/// REQ: F-093
#[test]
fn doc_f_093_contributor_example_holds_only_the_contribute_grant() {
    let example = example();
    let grants = strings(&example["capabilities"]["client"]);
    assert_eq!(grants, ["extensions.contribute:ilium.core.ambient/palettes"]);
}

/// With no contributor the palettes point collects an empty list (reference-test-harness.md, core stub table).
///
/// REQ: F-093
#[test]
fn extension_093_palettes_point_collects_an_empty_list_without_contributors() {
    let mut host = activated_client();
    assert!(collect_point(&mut host, "ilium.core.ambient/palettes").is_empty());
}

/// REQ: F-093, F-094
#[test]
fn extension_094_style_presets_point_collects_an_empty_list_without_contributors() {
    let mut host = activated_client();
    assert!(collect_point(&mut host, "ilium.core.ambient/style-presets").is_empty());
}

/// REQ: F-093
#[test]
fn extension_093_a_contributed_palette_is_collected_with_its_stops_in_order() {
    let mut host = activated_client();
    contribute(&mut host, "ilium.core.ambient/palettes", "harbour", 50, &example()["contributes"]["extensions"][0]["entry"]);
    let entries = collect_point(&mut host, "ilium.core.ambient/palettes");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["id"], "harbour");
    assert_eq!(strings(&entries[0]["stops"]), ["#0b1d2a", "#1f6f8b", "#99d5c9", "#f6f1d1"]);
}

/// REQ: F-093
#[test]
fn extension_093_the_look_service_answers_a_get_with_the_palette() {
    let mut host = activated_client();
    let reply = call_look_service(&mut host, "get", "{}");
    assert_eq!(reply["palette"], json!([]), "Scene colors is the default palette and passes no stops: {reply}");
}

/// REQ: F-093
#[test]
fn extension_093_an_unknown_look_service_method_is_not_found() {
    let mut host = activated_client();
    let reply = call_look_service(&mut host, "nope", "{}");
    assert!(reply.to_string().contains("not-found"), "{reply}");
}

// ---------------------------------------------------------------------------------------------
// Gap helpers (see ../API_GAPS.md)
// ---------------------------------------------------------------------------------------------

use ilium_plugin_sdk::testing::FakeHost;

/// The entries the point collects from every contributor, as JSON objects, highest priority first.
fn collect_point(host: &mut FakeHost, point: &str) -> Vec<Value> {
    host.extensions().collect(point)
}

/// A package manifest contribution to a point of the plugin.
fn contribute(host: &mut FakeHost, point: &str, id: &str, priority: i64, entry: &Value) {
    host.extensions().contribute(point, id, priority, entry.clone());
}

/// Calls a service of the plugin the way the host does, through the `service-provider` export.
fn call_look_service(host: &mut FakeHost, method: &str, payload_json: &str) -> Value {
    host.call_service("ilium.core.ambient/look", method, payload_json)
}

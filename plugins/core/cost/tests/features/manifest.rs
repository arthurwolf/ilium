//! The manifest is exact (page "Complete manifest", COR-050) and every contribution is declared.

use serde_json::{json, Value};

use crate::common::*;

fn page_manifest() -> Value {
    let blocks = json_blocks(&page_text());
    // blocks[0] is the fragment, blocks[1] the complete manifest, later blocks are samples.
    serde_json::from_str(&blocks[1]).expect("complete manifest on the page is JSON")
}

fn fragment() -> Value {
    serde_json::from_str(&json_blocks(&page_text())[0]).expect("fragment is JSON")
}

fn rows() -> Vec<Value> {
    manifest_value()["contributes"]["settings"][0]["rows"].as_array().expect("rows").clone()
}

/// REQ: COR-050, F-038
#[test]
fn doc_cor_050_manifest_file_equals_complete_manifest_on_page() {
    assert_eq!(manifest_value(), page_manifest());
}

/// REQ: COR-050, F-038
#[test]
fn doc_cor_050_page_has_fragment_complete_manifest_and_example_blocks() {
    let blocks = json_blocks(&page_text());
    assert_eq!(blocks.len(), 3, "fragment, complete manifest and the price-sources example");
}

/// REQ: COR-050, F-038
#[test]
fn doc_cor_050_fragment_contributions_equal_the_complete_manifest() {
    let fragment = fragment();
    let complete = page_manifest();
    for key in ["api", "id", "server", "client", "contributes"] {
        assert_eq!(fragment[key], complete[key], "fragment key {key}");
    }
}

/// The fragment lists `ui.panels` as the only client grant. The complete manifest adds the
/// kernel-tab grant of the Reset planning tab (COR-030), so the fragment must be a prefix of it.
///
/// REQ: COR-050, F-116
#[test]
fn doc_cor_050_fragment_client_grants_are_a_prefix_of_the_complete_manifest() {
    let fragment = strings(&fragment()["capabilities"]["client"]);
    let complete = strings(&page_manifest()["capabilities"]["client"]);
    assert_eq!(fragment, ["ui.panels"]);
    assert_eq!(&complete[..fragment.len()], &fragment[..]);
}

/// REQ: COR-050, F-038
#[test]
fn doc_cor_050_fragment_server_grants_equal_the_complete_manifest() {
    assert_eq!(fragment()["capabilities"]["server"], page_manifest()["capabilities"]["server"]);
}

/// REQ: COR-050, F-038
#[test]
fn doc_cor_050_identity_fields_are_exact() {
    let manifest = manifest_value();
    assert_eq!(manifest["api"], "ilium:plugin@0.1.0");
    assert_eq!(manifest["id"], PLUGIN_ID);
    assert_eq!(manifest["name"], "Cost");
    assert_eq!(manifest["version"], "0.1.0");
    assert_eq!(manifest["publisher"], "Ilium");
    assert_eq!(manifest["license"], "MIT");
    assert_eq!(manifest["limits_tier"], "first-party");
}

/// REQ: COR-050, F-038
#[test]
fn doc_cor_050_description_names_the_id_and_both_sides() {
    assert_eq!(
        manifest_value()["description"],
        "The Cost plugin has the id ilium.core.cost and runs on both sides."
    );
}

/// REQ: COR-050, F-038
#[test]
fn doc_cor_050_server_side_has_entry_world_and_the_service_provider_export() {
    let manifest = manifest_value();
    assert_eq!(
        manifest["server"],
        json!({"entry": "server.wasm", "world": "ilium:server/base@0.1.0", "exports": ["service-provider"]})
    );
}

/// REQ: COR-050, F-031
#[test]
fn doc_cor_050_client_side_has_entry_and_world_and_no_exports_key() {
    let manifest = manifest_value();
    assert_eq!(manifest["client"], json!({"entry": "client.wasm", "world": "ilium:client/base@0.1.0"}));
}

/// REQ: COR-050, F-038
#[test]
fn doc_cor_050_server_grants_are_the_eight_of_the_page_in_order() {
    let manifest = manifest_value();
    assert_eq!(
        strings(&manifest["capabilities"]["server"]),
        [
            "agent.read",
            "agent.transcript",
            "tree.decorate",
            "http:status.anthropic.com",
            "http:status.openai.com",
            "services.provide:ilium.core.cost/usage",
            "settings.contribute:agent-cost",
            "settings.write-kernel:agent-cost",
        ]
    );
}

/// REQ: COR-050, F-116
#[test]
fn doc_cor_050_client_grants_are_ui_panels_and_the_reset_planning_tab() {
    let manifest = manifest_value();
    assert_eq!(
        strings(&manifest["capabilities"]["client"]),
        ["ui.panels", "settings.write-kernel:reset-planning"]
    );
}

/// REQ: COR-050, F-088
#[test]
fn doc_cor_050_capabilities_have_only_the_server_and_client_sides() {
    let manifest = manifest_value();
    let mut sides: Vec<&String> = manifest["capabilities"].as_object().expect("object").keys().collect();
    sides.sort();
    assert_eq!(sides, ["client", "server"]);
}

/// REQ: COR-050, F-088
#[test]
fn doc_cor_050_http_grants_name_exactly_the_two_status_hosts() {
    let manifest = manifest_value();
    let http: Vec<String> = strings(&manifest["capabilities"]["server"])
        .into_iter()
        .filter(|grant| grant.starts_with("http"))
        .collect();
    assert_eq!(http, ["http:status.anthropic.com", "http:status.openai.com"]);
}

/// REQ: COR-050, F-031
#[test]
fn doc_cor_050_activation_is_startup_and_the_cost_panel_view() {
    assert_eq!(strings(&manifest_value()["activation"]), ["startup", "view:cost.panel"]);
}

/// REQ: COR-050, F-031
#[test]
fn doc_cor_050_activation_matches_the_kernel_and_boot_table_row() {
    let row = table_row(&page("reference-kernel-and-boot"), "Cost");
    assert_eq!(row[1], "`startup`");
    assert_eq!(row[2], "`view:cost.panel`");
}

/// REQ: COR-050, F-038
#[test]
fn doc_cor_050_contributes_has_exactly_four_keys() {
    let manifest = manifest_value();
    let mut keys: Vec<&String> = manifest["contributes"].as_object().expect("object").keys().collect();
    keys.sort();
    assert_eq!(keys, ["decoration_slots", "services", "settings", "tree_orderings"]);
}

/// The plugin declares no tab of its own; it adds a section to the kernel tab (config migration page).
///
/// REQ: COR-030, F-040
#[test]
fn doc_cor_030_manifest_declares_no_settings_tab_of_its_own() {
    let manifest = manifest_value();
    assert!(manifest["contributes"].get("settings_tabs").is_none());
    assert!(manifest["contributes"].get("tabs").is_none());
}

/// REQ: COR-050, F-038
#[test]
fn doc_cor_050_decoration_slot_cost_is_a_trailing_badge_at_priority_40() {
    let manifest = manifest_value();
    assert_eq!(
        manifest["contributes"]["decoration_slots"],
        json!([{"id": "cost", "placement": "badge", "priority": 40, "trailing": true}])
    );
}

/// REQ: COR-050, F-039, F-005
#[test]
fn doc_cor_050_tree_ordering_by_cost_sorts_on_the_cost_slot() {
    let manifest = manifest_value();
    assert_eq!(
        manifest["contributes"]["tree_orderings"],
        json!([{"id": "by-cost", "title": "Cost, highest first", "slot": "cost"}])
    );
}

/// An ordering sorts on a slot the same plugin declares (reference-manifest.md `tree_orderings`).
///
/// REQ: COR-050, F-039
#[test]
fn doc_cor_050_tree_ordering_slot_is_a_declared_decoration_slot() {
    let manifest = manifest_value();
    let slot = manifest["contributes"]["tree_orderings"][0]["slot"].as_str().expect("slot");
    let declared: Vec<&str> = manifest["contributes"]["decoration_slots"]
        .as_array()
        .expect("slots")
        .iter()
        .map(|entry| entry["id"].as_str().expect("id"))
        .collect();
    assert!(declared.contains(&slot));
}

/// REQ: COR-050, F-039
#[test]
fn doc_cor_050_tree_decorate_grant_backs_the_slot_and_the_ordering() {
    let manifest = manifest_value();
    assert!(strings(&manifest["capabilities"]["server"]).contains(&"tree.decorate".to_string()));
}

/// REQ: COR-050, F-038
#[test]
fn doc_cor_050_usage_service_has_one_method_usage() {
    let manifest = manifest_value();
    let service = manifest["contributes"]["services"][USAGE_SERVICE].as_object().expect("service");
    assert_eq!(service.keys().collect::<Vec<_>>(), ["usage"]);
}

/// REQ: COR-050, F-038
#[test]
fn doc_cor_050_usage_params_take_an_optional_string_window() {
    let manifest = manifest_value();
    let params = &manifest["contributes"]["services"][USAGE_SERVICE]["usage"]["params"];
    assert_eq!(params, &json!({"type": "object", "properties": {"window": {"type": "string"}}}));
    assert!(params.get("required").is_none(), "window is optional");
}

/// REQ: COR-050, F-038
#[test]
fn doc_cor_050_usage_result_requires_the_panes_object() {
    let manifest = manifest_value();
    let result = &manifest["contributes"]["services"][USAGE_SERVICE]["usage"]["result"];
    assert_eq!(
        result,
        &json!({"type": "object", "properties": {"panes": {"type": "object"}}, "required": ["panes"]})
    );
}

/// REQ: COR-050, F-038
#[test]
fn doc_cor_050_provided_service_grant_names_the_declared_service() {
    let manifest = manifest_value();
    let grant = format!("services.provide:{USAGE_SERVICE}");
    assert!(strings(&manifest["capabilities"]["server"]).contains(&grant));
    assert!(manifest["contributes"]["services"].get(USAGE_SERVICE).is_some());
}

/// REQ: COR-050, F-040
#[test]
fn doc_cor_050_settings_section_is_cost_display_on_the_agent_cost_tab() {
    let manifest = manifest_value();
    let section = &manifest["contributes"]["settings"][0];
    assert_eq!(manifest["contributes"]["settings"].as_array().expect("array").len(), 1);
    assert_eq!(section["kind"], "section");
    assert_eq!(section["id"], "cost-display");
    assert_eq!(section["tab"], "agent-cost");
    assert_eq!(section["placement"], "end");
    assert_eq!(section["title"], "Display");
    assert_eq!(section["scope"], "server");
}

/// REQ: COR-050, F-040
#[test]
fn doc_cor_050_settings_section_tab_matches_the_contribute_grant() {
    let manifest = manifest_value();
    let tab = manifest["contributes"]["settings"][0]["tab"].as_str().expect("tab");
    assert!(strings(&manifest["capabilities"]["server"]).contains(&format!("settings.contribute:{tab}")));
}

/// REQ: COR-050, F-038
#[test]
fn doc_cor_050_unit_row_is_a_choice_defaulting_to_dollars() {
    let unit = rows()[0].clone();
    assert_eq!(
        unit,
        json!({
            "type": "choice", "key": "unit", "title": "Unit", "default": "dollars",
            "options": [
                {"value": "dollars", "title": "Dollars"},
                {"value": "quota", "title": "Quota percent"}
            ]
        })
    );
}

/// REQ: COR-050, F-038
#[test]
fn doc_cor_050_display_row_is_a_multi_choice_defaulting_to_tree_and_status() {
    let display = rows()[1].clone();
    assert_eq!(
        display,
        json!({
            "type": "multi-choice", "key": "display", "title": "Display", "default": ["tree", "status"],
            "options": [
                {"value": "tree", "title": "Tree rows"},
                {"value": "status", "title": "Status bar"}
            ]
        })
    );
}

/// REQ: COR-050, F-038
#[test]
fn doc_cor_050_the_section_has_exactly_two_rows_unit_then_display() {
    let keys: Vec<String> = rows().iter().map(|row| row["key"].as_str().expect("key").to_string()).collect();
    assert_eq!(keys, ["unit", "display"]);
}

/// REQ: COR-050, F-038
#[test]
fn doc_cor_050_row_defaults_are_among_their_options() {
    let rows = rows();
    let option_values = |row: &Value| -> Vec<String> {
        row["options"].as_array().expect("options").iter().map(|o| o["value"].as_str().expect("value").to_string()).collect()
    };
    assert!(option_values(&rows[0]).contains(&rows[0]["default"].as_str().expect("default").to_string()));
    for item in strings(&rows[1]["default"]) {
        assert!(option_values(&rows[1]).contains(&item), "default {item}");
    }
}

/// REQ: COR-030, COR-050, F-040
#[test]
fn doc_cor_030_server_holds_the_agent_cost_kernel_tab_grant() {
    let manifest = manifest_value();
    assert!(strings(&manifest["capabilities"]["server"]).contains(&"settings.write-kernel:agent-cost".to_string()));
}

/// REQ: COR-030, COR-050, F-116
#[test]
fn doc_cor_030_client_holds_the_reset_planning_kernel_tab_grant() {
    let manifest = manifest_value();
    assert!(strings(&manifest["capabilities"]["client"]).contains(&"settings.write-kernel:reset-planning".to_string()));
}

/// Kernel tabs are T1 only (api-kernel-settings-and-keymap.md), so the tier is first-party.
///
/// REQ: COR-030, F-116
#[test]
fn doc_cor_030_a_kernel_tab_holder_is_in_the_first_party_tier() {
    assert_eq!(manifest_value()["limits_tier"], "first-party");
}

/// REQ: COR-050, F-038
#[test]
fn doc_cor_050_core_plugins_index_lists_cost_on_both_sides() {
    let row = table_row(&page("core-plugins"), "[Cost](/pages/core-plugin-cost/)");
    assert_eq!(row[1], "`ilium.core.cost`");
    assert_eq!(row[2], "both");
}

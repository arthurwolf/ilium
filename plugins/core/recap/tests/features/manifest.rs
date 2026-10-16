//! The manifest is exact (page "Complete manifest", COR-050) and declares every contribution of
//! the recap feature (F-112).

use serde_json::{json, Value};

use crate::common::*;

fn page_manifest() -> Value {
    let blocks = json_blocks(&page_text());
    // blocks[0] is the fragment, blocks[1] the complete manifest, later blocks are samples.
    serde_json::from_str(&blocks[1]).expect("complete manifest on the page is JSON")
}

/// REQ: COR-050, F-112
#[test]
fn doc_cor_050_manifest_file_equals_complete_manifest_on_page() {
    assert_eq!(manifest_value(), page_manifest());
}

/// REQ: COR-050, F-112
#[test]
fn doc_cor_050_fragment_fields_match_complete_manifest() {
    let fragment: Value = serde_json::from_str(&json_blocks(&page_text())[0]).expect("fragment is JSON");
    let complete = page_manifest();
    for key in ["api", "id", "name", "version", "publisher", "limits_tier", "replaceable", "surface", "capabilities"] {
        assert_eq!(fragment[key], complete[key], "fragment key {key}");
    }
    assert_eq!(fragment["contributes"], complete["contributes"]);
}

/// REQ: COR-050, F-112
#[test]
fn doc_cor_050_identity_fields_are_exact() {
    let manifest = manifest_value();
    assert_eq!(manifest["api"], "ilium:plugin@0.1.0");
    assert_eq!(manifest["id"], PLUGIN_ID);
    assert_eq!(manifest["name"], "Recap");
    assert_eq!(manifest["version"], "0.1.0");
    assert_eq!(manifest["publisher"], "Ilium");
    assert_eq!(manifest["license"], "MIT");
    assert_eq!(manifest["limits_tier"], "first-party");
}

/// REQ: COR-050, F-112
#[test]
fn doc_cor_050_runs_on_both_sides_with_the_base_worlds_and_two_server_exports() {
    let manifest = manifest_value();
    assert_eq!(
        manifest["server"],
        json!({"entry": "server.wasm", "world": "ilium:server/base@0.1.0", "exports": ["job-runner", "service-provider"]})
    );
    assert_eq!(manifest["client"], json!({"entry": "client.wasm", "world": "ilium:client/base@0.1.0", "exports": []}));
}

/// REQ: COR-050, F-112
#[test]
fn doc_cor_050_server_capabilities_are_the_eleven_listed_grants_in_order() {
    let manifest = manifest_value();
    assert_eq!(
        strings(&manifest["capabilities"]["server"]),
        [
            "cli.contribute",
            "panes.read",
            "agent.read",
            "agent.transcript",
            "terminal.read-screen",
            "inference",
            "jobs.run",
            "settings.read-kernel:inference",
            "services.provide:ilium.core.recap/recap",
            "extensions.provide:ilium.core.recap/fact-sources",
            "extensions.provide:ilium.core.recap/before-recap",
        ]
    );
}

/// REQ: COR-050, F-112
#[test]
fn doc_cor_050_client_capability_is_help_read_only() {
    assert_eq!(strings(&manifest_value()["capabilities"]["client"]), ["help.read"]);
}

/// REQ: COR-050, F-112
#[test]
fn doc_cor_050_activation_triggers_are_cli_recap_and_command_recap() {
    assert_eq!(strings(&manifest_value()["activation"]), ["cli:recap", "command:recap"]);
}

/// REQ: COR-050, F-112
#[test]
fn doc_cor_050_replaceable_surface_lists_service_both_points_and_cli() {
    let manifest = manifest_value();
    assert_eq!(manifest["replaceable"], true);
    assert_eq!(
        strings(&manifest["surface"]),
        ["services:recap", "points:fact-sources", "points:before-recap", "cli:recap"]
    );
}

/// REQ: COR-050, F-112
#[test]
fn doc_cor_050_contributes_has_exactly_the_six_documented_kinds() {
    let manifest = manifest_value();
    let mut keys: Vec<&String> = manifest["contributes"].as_object().expect("object").keys().collect();
    keys.sort();
    assert_eq!(keys, ["cli", "client_requests", "extension_points", "help", "jobs", "services"]);
}

/// REQ: COR-050, F-112
#[test]
fn doc_cor_050_cli_contribution_is_recap_jsonl_requires_server_with_pane_selector() {
    assert_eq!(
        manifest_value()["contributes"]["cli"],
        json!({
            "name": "recap",
            "summary": "Print short and long recaps of the selected panes",
            "output": "jsonl",
            "stdin": "none",
            "requires_server": true,
            "argument_groups": ["pane-selector"]
        })
    );
}

/// REQ: COR-050, F-112
#[test]
fn doc_cor_050_job_kind_recap_pane_has_5000_ms_deadline_and_64_mib() {
    assert_eq!(
        manifest_value()["contributes"]["jobs"],
        json!([{"kind": "recap-pane", "deadline_ms": 5000, "memory_mib": 64}])
    );
}

/// REQ: COR-050, F-112
#[test]
fn doc_cor_050_client_answers_the_onboarding_state_request_and_ships_the_privacy_help() {
    let contributes = &manifest_value()["contributes"];
    assert_eq!(contributes["client_requests"], json!([{"kind": "onboarding-state"}]));
    assert_eq!(
        contributes["help"],
        json!([{"id": "recap-privacy", "file": "help/recap-privacy.md", "title": "What a recap sends"}])
    );
}

/// REQ: COR-050, F-112
#[test]
fn doc_cor_050_no_settings_commands_keybindings_menus_or_panels_are_contributed() {
    let manifest = manifest_value();
    for key in ["settings", "commands", "keybindings", "menus", "panels", "views", "topics"] {
        assert!(manifest["contributes"].get(key).is_none(), "unexpected contribution {key}");
    }
}

/// REQ: COR-050, F-112
#[test]
fn recap_050_every_activation_trigger_names_a_declared_cli_name() {
    let manifest = manifest_value();
    let cli_name = manifest["contributes"]["cli"]["name"].as_str().expect("name");
    for trigger in strings(&manifest["activation"]) {
        let (_, name) = trigger.split_once(':').expect("kind:name");
        assert_eq!(name, cli_name);
    }
}

/// REQ: COR-050, F-112
#[test]
fn recap_050_activating_both_sides_from_the_manifest_succeeds() {
    let server = activated_server();
    let client = activated_client();
    assert!(!server.is_disabled());
    assert!(!client.is_disabled());
}

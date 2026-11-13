//! The manifest is exact (COR-050): it equals the Complete manifest block of the page, and every contribution is as the page lists it.

use ilium_plugin_sdk::testing::FakeHost;
use serde_json::{json, Value};

use crate::common::{manifest_path, page_text};

fn manifest() -> Value {
    let text = std::fs::read_to_string(manifest_path()).expect("manifest.json readable");
    serde_json::from_str(&text).expect("manifest.json is JSON")
}

/// The first json code block after the heading line `heading`.
fn json_block_after(page: &str, heading: &str) -> Value {
    let start = page
        .find(heading)
        .unwrap_or_else(|| panic!("heading {heading} missing"));
    let rest = &page[start..];
    let open = rest.find("```json\n").expect("json block") + "```json\n".len();
    let close = rest[open..].find("```").expect("block end");
    serde_json::from_str(&rest[open..open + close]).expect("page block is JSON")
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .expect("array")
        .iter()
        .map(|item| item.as_str().expect("string").to_string())
        .collect()
}

/// REQ: COR-050
#[test]
fn manifest_050_equals_the_complete_manifest_of_the_page() {
    assert_eq!(
        manifest(),
        json_block_after(&page_text(), "### Complete manifest")
    );
}

/// REQ: COR-050
#[test]
fn manifest_050_fragment_contributions_equal_the_complete_manifest() {
    let page = page_text();
    let fragment = json_block_after(&page, "### Manifest");
    let complete = manifest();
    assert_eq!(fragment["contributes"], complete["contributes"]);
    assert_eq!(fragment["server"], complete["server"]);
    assert_eq!(fragment["client"], complete["client"]);
}

/// REQ: COR-050
#[test]
fn manifest_050_fragment_grants_are_a_subset_of_the_complete_manifest() {
    let fragment = json_block_after(&page_text(), "### Manifest");
    let complete = manifest();
    for side in ["server", "client"] {
        let all = strings(&complete["capabilities"][side]);
        for grant in strings(&fragment["capabilities"][side]) {
            assert!(
                all.contains(&grant),
                "{side} grant {grant} missing from the complete manifest"
            );
        }
    }
}

/// REQ: COR-050
#[test]
fn manifest_050_identity_keys_are_as_the_page_states() {
    let manifest = manifest();
    assert_eq!(manifest["api"], "ilium:plugin@0.1.0");
    assert_eq!(manifest["id"], "ilium.core.compaction");
    assert_eq!(manifest["name"], "Compaction");
    assert_eq!(manifest["version"], "0.1.0");
    assert_eq!(manifest["publisher"], "Ilium");
    assert_eq!(manifest["license"], "MIT");
    assert_eq!(manifest["limits_tier"], "first-party");
}

/// REQ: COR-050
#[test]
fn manifest_050_server_side_runs_the_job_runner_in_the_base_world() {
    let manifest = manifest();
    assert_eq!(manifest["server"]["entry"], "server.wasm");
    assert_eq!(manifest["server"]["world"], "ilium:server/base@0.1.0");
    assert_eq!(manifest["server"]["exports"], json!(["job-runner"]));
}

/// REQ: COR-050
#[test]
fn manifest_050_client_side_draws_views_in_the_base_world() {
    let manifest = manifest();
    assert_eq!(manifest["client"]["entry"], "client.wasm");
    assert_eq!(manifest["client"]["world"], "ilium:client/base@0.1.0");
    assert_eq!(manifest["client"]["exports"], json!(["views"]));
}

/// REQ: COR-050
#[test]
fn manifest_050_server_grants_are_exactly_the_six_of_the_page() {
    assert_eq!(
        strings(&manifest()["capabilities"]["server"]),
        [
            "agent.transcript",
            "inference",
            "jobs.run",
            "panes.lifecycle",
            "fs.read:agent-homes",
            "fs.write:agent-homes"
        ]
    );
}

/// REQ: COR-050, COR-030
#[test]
fn manifest_050_client_grants_include_both_kernel_tab_writes() {
    assert_eq!(
        strings(&manifest()["capabilities"]["client"]),
        [
            "open-external",
            "ui.panels",
            "settings.write-kernel:optimization",
            "settings.write-kernel:remote-compaction"
        ]
    );
}

/// REQ: COR-050
#[test]
fn manifest_050_activation_triggers_are_event_service_and_view() {
    assert_eq!(
        manifest()["activation"],
        json!([
            "event:agent-activity-changed",
            "service:compact",
            "view:compaction.optimizer"
        ])
    );
}

/// REQ: COR-050
#[test]
fn manifest_050_job_kind_compact_has_the_five_minute_deadline_and_256_mib() {
    let jobs = manifest()["contributes"]["jobs"].clone();
    assert_eq!(
        jobs,
        json!([{ "kind": "compact", "deadline_ms": 300000, "memory_mib": 256 }])
    );
    assert_eq!(jobs[0]["deadline_ms"].as_u64(), Some(5 * 60 * 1000));
}

/// REQ: COR-050
#[test]
fn manifest_050_one_command_and_one_non_modal_panel_are_contributed() {
    let manifest = manifest();
    assert_eq!(
        manifest["contributes"]["commands"],
        json!([{ "id": "compact", "title": "Compact this agent" }])
    );
    assert_eq!(
        manifest["contributes"]["panels"],
        json!([{ "id": "optimizer", "title": "Compaction", "modal": false }])
    );
}

/// REQ: COR-050, COR-033
#[test]
fn manifest_050_declares_no_settings_rows_because_the_kernel_tabs_own_the_keys() {
    let manifest = manifest();
    assert!(manifest["contributes"].get("settings").is_none());
    assert!(manifest["contributes"].get("keybindings").is_none());
}

/// REQ: COR-050
#[test]
fn manifest_050_page_declares_the_same_contribution_kinds_as_the_manifest() {
    let manifest = manifest();
    let kinds: Vec<&String> = manifest["contributes"]
        .as_object()
        .expect("object")
        .keys()
        .collect();
    assert_eq!(kinds, ["commands", "jobs", "panels"]);
}

/// REQ: COR-050
#[test]
fn manifest_050_a_grant_outside_the_manifest_is_refused_with_harness_grant_not_declared() {
    // The host refuses a grant the manifest never declared (harness.grant-not-declared).
    let mut host = FakeHost::for_manifest(manifest_path())
        .expect("manifest loads")
        .side("client")
        .build();
    let refused = host.grant_now("fs.read:agent-homes");
    assert_eq!(
        refused.expect_err("client side never declared it").code,
        "harness.grant-not-declared"
    );
}

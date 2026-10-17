//! The manifest is exact (page "Complete manifest"): identity, sides, grants per side, activation and
//! every contribution. The page fragment agrees with the complete block.

use ilium_plugin_sdk::testing::FakeHost;
use serde_json::{json, Value};

use crate::common::{fenced_block_after, manifest_json, manifest_path, page_text};

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
fn cor_050_manifest_identity_fields_match_the_page() {
    let manifest = manifest_json();
    assert_eq!(manifest["api"], "ilium:plugin@0.1.0");
    assert_eq!(manifest["id"], "ilium.core.diagnostics");
    assert_eq!(manifest["name"], "Diagnostics");
    assert_eq!(manifest["version"], "0.1.0");
    assert_eq!(manifest["publisher"], "Ilium");
    assert_eq!(manifest["license"], "MIT");
    assert_eq!(
        manifest["description"],
        "Diagnostics helps when something goes wrong."
    );
}

/// REQ: COR-050
#[test]
fn cor_050_limits_tier_is_first_party() {
    assert_eq!(manifest_json()["limits_tier"], "first-party");
}

/// REQ: COR-050
#[test]
fn cor_050_server_side_is_service_provider_in_the_base_server_world() {
    assert_eq!(
        manifest_json()["server"],
        json!({ "entry": "server.wasm", "world": "ilium:server/base@0.1.0", "exports": ["service-provider"] })
    );
}

/// REQ: COR-050
#[test]
fn cor_050_client_side_exports_views_in_the_base_client_world() {
    assert_eq!(
        manifest_json()["client"],
        json!({ "entry": "client.wasm", "world": "ilium:client/base@0.1.0", "exports": ["views"] })
    );
}

/// REQ: COR-050
#[test]
fn cor_050_server_capabilities_are_exactly_the_eight_of_the_page() {
    assert_eq!(
        strings(&manifest_json()["capabilities"]["server"]),
        [
            "fs.read:path:/tmp/.ilium-1000/logs",
            "settings.read-kernel:debug",
            "panes.read",
            "journal.read",
            "services.use:ilium.core.settings/get",
            "services.provide:ilium.core.diagnostics/report",
            "extensions.provide:ilium.core.diagnostics/log-sinks",
            "extensions.provide:ilium.core.diagnostics/report-sections"
        ]
    );
}

/// REQ: COR-050
#[test]
fn cor_050_client_capabilities_are_exactly_the_four_of_the_page() {
    assert_eq!(
        strings(&manifest_json()["capabilities"]["client"]),
        ["ui.panels", "fs.pick", "notify.toast", "help.read"]
    );
}

/// REQ: COR-050
#[test]
fn cor_050_the_server_never_holds_a_settings_write_grant() {
    for grant in strings(&manifest_json()["capabilities"]["server"]) {
        assert!(!grant.starts_with("settings.write"), "{grant}");
    }
}

/// REQ: COR-050
#[test]
fn cor_050_the_log_folder_grant_is_a_literal_path_not_a_scope_pattern() {
    let grants = strings(&manifest_json()["capabilities"]["server"]);
    let read = grants
        .iter()
        .find(|grant| grant.starts_with("fs.read:"))
        .expect("fs.read grant");
    assert_eq!(read, "fs.read:path:/tmp/.ilium-1000/logs");
    assert!(!read.contains('*'));
}

/// REQ: COR-050
#[test]
fn cor_050_activation_triggers_are_startup_and_the_warnings_view() {
    assert_eq!(
        strings(&manifest_json()["activation"]),
        ["startup", "view:diagnostics.warnings"]
    );
}

/// REQ: COR-050
#[test]
fn cor_050_the_plugin_is_replaceable() {
    assert_eq!(manifest_json()["replaceable"], true);
}

/// REQ: COR-050
#[test]
fn cor_050_surface_lists_the_service_both_points_and_the_command() {
    assert_eq!(
        strings(&manifest_json()["surface"]),
        [
            "services:report",
            "points:log-sinks",
            "points:report-sections",
            "commands:build-report"
        ]
    );
}

/// REQ: COR-050
#[test]
fn cor_050_contributes_exactly_one_command_build_report_in_the_help_category() {
    assert_eq!(
        manifest_json()["contributes"]["commands"],
        json!([{ "id": "build-report", "title": "Build diagnostics report", "category": "Help" }])
    );
}

/// REQ: COR-050
#[test]
fn cor_050_contributes_one_panel_warnings_with_the_alert_triangle_icon() {
    assert_eq!(
        manifest_json()["contributes"]["panels"],
        json!([{ "id": "warnings", "title": "Warnings", "icon": "alert-triangle" }])
    );
}

/// REQ: COR-050
#[test]
fn cor_050_contributes_one_help_topic_logs_and_reports() {
    assert_eq!(
        manifest_json()["contributes"]["help"],
        json!([{ "id": "diagnostics", "file": "help/diagnostics.md", "title": "Logs and reports" }])
    );
}

/// REQ: COR-050
#[test]
fn cor_050_the_report_service_has_one_method_build_with_a_bounded_text_result() {
    let services = &manifest_json()["contributes"]["services"];
    let report = services["ilium.core.diagnostics/report"]
        .as_object()
        .expect("service object");
    assert_eq!(report.keys().collect::<Vec<_>>(), ["build"]);
    assert_eq!(
        report["build"]["params"]["properties"]["include_log_tail"]["type"],
        "boolean"
    );
    assert_eq!(report["build"]["result"]["required"], json!(["text"]));
    assert_eq!(
        report["build"]["result"]["properties"]["text"]["maxLength"],
        262144
    );
}

/// REQ: COR-050
#[test]
fn cor_050_the_text_limit_of_the_service_result_is_256_kibibytes() {
    assert_eq!(262144, 256 * 1024);
    let limit = &manifest_json()["contributes"]["services"]["ilium.core.diagnostics/report"]
        ["build"]["result"]["properties"]["text"]["maxLength"];
    assert_eq!(limit.as_u64(), Some(256 * 1024));
}

/// REQ: COR-050
#[test]
fn cor_050_complete_manifest_block_of_the_page_equals_the_file() {
    let block = fenced_block_after(&page_text(), "### Complete manifest", "json");
    let from_page: Value = serde_json::from_str(&block).expect("page block is JSON");
    assert_eq!(from_page, manifest_json());
}

/// REQ: COR-050
#[test]
fn cor_050_every_key_of_the_fragment_block_agrees_with_the_complete_manifest() {
    let fragment: Value =
        serde_json::from_str(&fenced_block_after(&page_text(), "### Manifest", "json"))
            .expect("fragment is JSON");
    let manifest = manifest_json();
    for (key, value) in fragment.as_object().expect("object") {
        assert_eq!(&manifest[key], value, "key {key}");
    }
}

/// REQ: COR-050
#[test]
fn cor_050_the_server_host_built_from_the_manifest_holds_the_server_grants() {
    let mut host = FakeHost::for_manifest(manifest_path())
        .expect("manifest loads")
        .side("server")
        .build();
    assert_eq!(host.current_mode(), ilium_plugin_sdk::testing::Mode::Native);
    // DEV-411: a grant outside the side's manifest list is refused, so the host holds only the
    // server grants; a client-only grant and settings.write are both refused.
    for undeclared in ["ui.panels", "settings.write"] {
        let error = host.grant_now(undeclared).expect_err(undeclared);
        assert_eq!(error.code, "harness.grant-not-declared", "{undeclared}");
    }
    host.grant_now("journal.read")
        .expect("a declared server grant is accepted");
}

/// REQ: COR-050
#[test]
fn cor_050_a_missing_manifest_file_is_refused_by_the_host_builder() {
    let result = FakeHost::for_manifest(manifest_path().with_file_name("missing-manifest.json"));
    // manual does not pin the error code of a missing manifest file
    let error = result.err().expect("a missing manifest file is refused");
    assert!(
        !error.code.is_empty() && !error.message.is_empty(),
        "{error}"
    );
}

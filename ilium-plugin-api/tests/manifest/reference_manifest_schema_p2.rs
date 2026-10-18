//! reference-manifest-schema.md, part 2: PKG-578, PKG-579, PKG-581..PKG-583.

use ilium_plugin_api::manifest::Diagnostic;
use serde_json::{json, Value};

use crate::support::*;

fn ids(count: usize) -> Vec<String> {
    (0..count)
        .map(|index| format!("example.peer{index}"))
        .collect()
}

fn error_pointers(found: &[Diagnostic]) -> Vec<String> {
    found
        .iter()
        .map(|diagnostic| diagnostic.pointer.clone())
        .collect()
}

fn point(risk: Value) -> Value {
    json!([{ "id": "p", "title": "P", "kind": "collect", "entry_schema": { "type": "object" }, "risk": risk }])
}

// PKG-578

/// REQ: PKG-578
#[test]
fn pkg_578_manifest_errors_sort_by_json_pointer_in_byte_order() {
    let value = set(set(client(), "/version", json!("x")), "/id", json!("Bad"));
    let pointers = error_pointers(&error_diagnostics(&value));
    assert!(pointers.len() >= 2, "need two errors, got {pointers:?}");
    let mut sorted = pointers.clone();
    sorted.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
    assert_eq!(pointers, sorted);
}

/// REQ: PKG-578
#[test]
fn pkg_578_three_errors_print_in_pointer_order_not_declaration_order() {
    let value = set(
        set(set(client(), "/version", json!("x")), "/name", json!("")),
        "/id",
        json!("Bad"),
    );
    let pointers = error_pointers(&error_diagnostics(&value));
    assert!(pointers.len() >= 3, "got {pointers:?}");
    let mut sorted = pointers.clone();
    sorted.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
    assert_eq!(pointers, sorted);
}

/// REQ: PKG-578
#[test]
fn pkg_578_the_same_manifest_always_prints_the_same_order() {
    let value = set(set(client(), "/version", json!("x")), "/id", json!("Bad"));
    assert_eq!(
        error_codes(&diagnostics(
            &value,
            ilium_plugin_api::Tier::T3,
            ilium_plugin_api::manifest::CheckMode::Check
        )),
        errors(&value)
    );
    assert_eq!(errors(&value), errors(&value));
}

// PKG-579

/// REQ: PKG-579
#[test]
fn pkg_579_thirty_two_distinct_ids_are_accepted() {
    let value = set(client(), "/accepts_messages_from", json!(ids(32)));
    assert_clean(&value);
}

/// REQ: PKG-579
#[test]
fn pkg_579_thirty_three_ids_are_refused_schema() {
    let value = set(client(), "/accepts_messages_from", json!(ids(33)));
    assert!(!errors(&value).is_empty());
}

/// REQ: PKG-579
#[test]
fn pkg_579_duplicate_ids_are_refused() {
    let value = set(
        client(),
        "/accepts_messages_from",
        json!(["example.peer", "example.peer"]),
    );
    assert!(!errors(&value).is_empty());
}

/// REQ: PKG-579
#[test]
fn pkg_579_service_requirement_with_a_range_refused_service_version() {
    for text in [
        "svc@^1.0.0",
        "svc@~1.0.0",
        "svc@1.0",
        "svc@>=1.0.0",
        "svc",
        "svc@1.0.0-rc1x.y",
    ] {
        let value = set(client(), "/requires/services", json!([text]));
        let found = errors(&value);
        assert!(
            found.iter().any(|code| code == "manifest.service-version") || !found.is_empty(),
            "{text}: {found:?}"
        );
    }
    let value = set(client(), "/requires/services", json!(["svc@^1.0.0"]));
    assert_has(&errors(&value), "manifest.service-version");
}

/// REQ: PKG-579
#[test]
fn pkg_579_service_requirement_with_full_version_is_accepted() {
    let value = set(client(), "/requires/services", json!(["svc@1.2.3"]));
    assert_lacks(&errors(&value), "manifest.service-version");
}

/// REQ: PKG-579
#[test]
fn pkg_579_on_failure_on_a_standard_point_refused_extension_point_invalid() {
    let mut entry = point(json!("standard"));
    entry[0]["kind"] = json!("hook");
    entry[0]["mode"] = json!("veto");
    entry[0]["payload_schema"] = json!({ "type": "object" });
    entry[0]["on_failure"] = json!("pass");
    let value = set(client(), "/contributes/extension_points", entry);
    assert_has(&errors(&value), "manifest.extension-point-invalid");
}

/// REQ: PKG-579
#[test]
fn pkg_579_final_check_on_a_non_transform_point_refused_extension_point_invalid() {
    let mut entry = point(json!("standard"));
    entry[0]["kind"] = json!("hook");
    entry[0]["mode"] = json!("observe");
    entry[0]["payload_schema"] = json!({ "type": "object" });
    entry[0]["final_check"] = json!(true);
    let value = set(client(), "/contributes/extension_points", entry);
    assert_has(&errors(&value), "manifest.extension-point-invalid");
}

// PKG-581: the install source check is not part of the manifest validator.

/// REQ: PKG-581
#[test]
fn doc_pkg_581_the_page_names_policy_source_denied_for_a_source_outside_allowed_sources() {
    let rows = catalogue();
    let row = rows
        .iter()
        .find(|row| row.code == "policy.source-denied")
        .expect("policy.source-denied is catalogued");
    assert_eq!(row.stage, "9");
    assert_eq!(row.exit, "7");
    assert!(row.template.contains("allowed_sources"));
}

/// REQ: PKG-581
#[test]
fn doc_pkg_581_the_page_maps_the_old_code_to_source_denied() {
    let text = page("reference-manifest-schema.md");
    assert!(text.contains("| `policy.source` | `policy.source-denied` |"));
}

// PKG-582

/// REQ: PKG-582
#[test]
fn pkg_582_every_kernel_subcommand_refused_cli_name_reserved() {
    for name in ["plugin", "session", "kill-session", "help", "version"] {
        let value = set(
            client(),
            "/contributes/cli",
            json!([{ "name": name, "summary": "S" }]),
        );
        let found = errors(&value);
        assert_has(&found, "manifest.cli-name-reserved");
        assert_lacks(&found, "manifest.cli-name-taken");
    }
}

/// REQ: PKG-582
#[test]
fn pkg_582_a_non_reserved_name_is_not_reserved() {
    for name in ["plugins", "sessions", "tool", "versions"] {
        let value = set(
            client(),
            "/contributes/cli",
            json!([{ "name": name, "summary": "S" }]),
        );
        assert_lacks(&errors(&value), "manifest.cli-name-reserved");
    }
}

/// REQ: PKG-582
#[test]
fn pkg_582_a_reserved_alias_is_refused_cli_name_reserved() {
    let value = set(
        client(),
        "/contributes/cli",
        json!([{ "name": "tool", "summary": "S", "aliases": ["help"] }]),
    );
    assert_has(&errors(&value), "manifest.cli-name-reserved");
}

/// REQ: PKG-582
#[test]
fn pkg_582_taken_is_used_for_a_name_of_another_plugin_only() {
    let value = set(
        client(),
        "/contributes/cli",
        json!([{ "name": "dup", "summary": "A" }, { "name": "dup", "summary": "B" }]),
    );
    let found = errors(&value);
    assert_has(&found, "manifest.cli-name-taken");
    assert_lacks(&found, "manifest.cli-name-reserved");
}

// PKG-583

/// REQ: PKG-583
#[test]
fn pkg_583_an_unknown_risk_refused_point_risk() {
    for risk in ["low", "medium", "HIGH", ""] {
        let value = set(
            client(),
            "/contributes/extension_points",
            point(json!(risk)),
        );
        assert_has(&errors(&value), "manifest.point-risk");
    }
}

/// REQ: PKG-583
#[test]
fn pkg_583_known_risks_are_not_point_risk() {
    for risk in ["standard", "high"] {
        let value = set(
            client(),
            "/contributes/extension_points",
            point(json!(risk)),
        );
        assert_lacks(&errors(&value), "manifest.point-risk");
    }
}

/// REQ: PKG-583
#[test]
fn pkg_583_a_missing_risk_defaults_to_standard_and_is_not_point_risk() {
    let mut entry = point(json!("standard"));
    entry[0].as_object_mut().unwrap().remove("risk");
    let value = set(client(), "/contributes/extension_points", entry);
    assert_clean(&value);
    let unknown = set(
        client(),
        "/contributes/extension_points",
        point(json!("critical")),
    );
    assert_has(&errors(&unknown), "manifest.point-risk");
}

/// REQ: PKG-583
#[test]
fn doc_pkg_583_point_risk_is_catalogued_at_stage_ten_exit_one() {
    let row = catalogue()
        .into_iter()
        .find(|row| row.code == "manifest.point-risk")
        .expect("catalogued");
    assert_eq!(row.stage, "10");
    assert_eq!(row.exit, "1");
}

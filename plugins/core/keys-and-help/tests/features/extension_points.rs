//! Extension points `key-hints` and `help-sources` (both collect, standard risk), the example
//! package of the page and the order rules of collect points.

use std::path::PathBuf;

use ilium_plugin_sdk::testing::FakeSession;
use serde_json::{json, Value};

use crate::common::*;

const KEY_HINTS: &str = "ilium.core.keys-and-help/key-hints";
const HELP_SOURCES: &str = "ilium.core.keys-and-help/help-sources";

/// Writes a contributor package into a fresh temporary directory and returns it.
fn package(name: &str, manifest: &Value) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("keys-and-help-ext-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("package dir");
    std::fs::write(
        dir.join("manifest.json"),
        serde_json::to_string_pretty(manifest).unwrap(),
    )
    .expect("manifest");
    dir
}

/// The example manifest of the page, completed with the keys a package needs.
fn example_package(id: &str, priority: i64, chord: &str, label: &str) -> Value {
    json!({
        "api": "ilium:plugin@0.1.0",
        "id": id,
        "name": id,
        "version": "0.1.0",
        "publisher": "Test",
        "capabilities": { "client": [format!("extensions.contribute:{KEY_HINTS}")] },
        "contributes": {
            "extensions": [{
                "point": KEY_HINTS,
                "id": "board-hints",
                "priority": priority,
                "entry": { "context": "pane:board", "chord": chord, "label": label }
            }]
        }
    })
}

fn session_with(packages: &[PathBuf]) -> FakeSession {
    let mut session = FakeSession::new();
    session.install_core("keys-and-help");
    for package in packages {
        session.install(package);
    }
    session.start();
    session.pump().expect("pump");
    session
}

/// REQ: F-010
#[test]
fn points_010_the_page_example_matches_the_documented_entry_shape() {
    let page = page_text();
    let example = json_block_after(
        &page,
        "#### Example: Shows a hint while a board pane has focus",
    );
    let entry = &example["contributes"]["extensions"][0]["entry"];
    assert_eq!(
        entry,
        &json!({"context": "pane:board", "chord": "n", "label": "New card"})
    );
    assert_eq!(example["contributes"]["extensions"][0]["point"], KEY_HINTS);
    assert_eq!(example["contributes"]["extensions"][0]["priority"], 50);
}

/// REQ: F-010
#[test]
fn points_010_a_contributed_key_hint_is_collected_with_its_payload() {
    let dir = package(
        "one",
        &example_package("test.board-hints", 50, "n", "New card"),
    );
    let mut session = session_with(&[dir]);
    let entries = session.collected(KEY_HINTS);
    assert_eq!(
        entries,
        vec![json!({"context": "pane:board", "chord": "n", "label": "New card"})]
    );
}

/// REQ: F-010
#[test]
fn points_010_collected_entries_follow_priority_lowest_first() {
    let late = package("late", &example_package("test.late", 90, "x", "Late"));
    let early = package("early", &example_package("test.early", 10, "y", "Early"));
    let mut session = session_with(&[late, early]);
    let labels: Vec<Value> = session
        .collected(KEY_HINTS)
        .iter()
        .map(|entry| entry["label"].clone())
        .collect();
    assert_eq!(labels, vec![json!("Early"), json!("Late")]);
}

/// REQ: F-010
#[test]
fn points_010_equal_priorities_break_ties_by_plugin_id() {
    let b = package("tie-b", &example_package("test.b", 50, "b", "B"));
    let a = package("tie-a", &example_package("test.a", 50, "a", "A"));
    let mut session = session_with(&[b, a]);
    let labels: Vec<Value> = session
        .collected(KEY_HINTS)
        .iter()
        .map(|entry| entry["label"].clone())
        .collect();
    assert_eq!(labels, vec![json!("A"), json!("B")]);
}

/// REQ: F-010
#[test]
fn points_010_an_entry_without_a_label_is_refused_at_install_with_manifest_schema() {
    let mut manifest = example_package("test.nolabel", 50, "n", "x");
    manifest["contributes"]["extensions"][0]["entry"] =
        json!({"context": "pane:board", "chord": "n"});
    let dir = package("nolabel", &manifest);
    let mut session = FakeSession::new();
    session.install_core("keys-and-help");
    let error = session.try_install(&dir).expect_err("refused");
    assert!(
        error.to_string().contains("manifest.extension-entry"),
        "{error}"
    );
}

/// REQ: F-010
#[test]
fn points_010_a_contributor_without_the_contribute_grant_is_refused_with_manifest_grant_missing_extension(
) {
    let mut manifest = example_package("test.nogrant", 50, "n", "x");
    manifest["capabilities"] = json!({"client": []});
    let dir = package("nogrant", &manifest);
    let mut session = FakeSession::new();
    session.install_core("keys-and-help");
    let error = session.try_install(&dir).expect_err("refused");
    assert!(
        error
            .to_string()
            .contains("manifest.grant-missing-extension"),
        "{error}"
    );
}

/// REQ: F-010
#[test]
fn points_010_a_wildcard_contribute_grant_is_refused_with_manifest_grant_wildcard() {
    let mut manifest = example_package("test.wild", 50, "n", "x");
    manifest["capabilities"] = json!({"client": ["extensions.contribute:*"]});
    let dir = package("wild", &manifest);
    let mut session = FakeSession::new();
    session.install_core("keys-and-help");
    let error = session.try_install(&dir).expect_err("refused");
    assert!(
        error.to_string().contains("manifest.grant-wildcard"),
        "{error}"
    );
}

/// REQ: F-010
#[test]
fn points_010_the_help_sources_point_collects_id_title_and_service() {
    let manifest = json!({
        "api": "ilium:plugin@0.1.0", "id": "test.docs", "name": "Docs", "version": "0.1.0", "publisher": "Test",
        "capabilities": { "client": [format!("extensions.contribute:{HELP_SOURCES}")] },
        "contributes": { "extensions": [{
            "point": HELP_SOURCES, "id": "remote-docs", "priority": 50,
            "entry": { "id": "remote", "title": "Remote docs", "service": "test.docs/index" }
        }] }
    });
    let dir = package("help-sources", &manifest);
    let mut session = session_with(&[dir]);
    assert_eq!(
        session.collected(HELP_SOURCES),
        vec![json!({"id": "remote", "title": "Remote docs", "service": "test.docs/index"})]
    );
}

/// REQ: F-010
#[test]
fn points_010_a_help_source_without_a_service_is_refused_at_install() {
    let manifest = json!({
        "api": "ilium:plugin@0.1.0", "id": "test.docs2", "name": "Docs", "version": "0.1.0", "publisher": "Test",
        "capabilities": { "client": [format!("extensions.contribute:{HELP_SOURCES}")] },
        "contributes": { "extensions": [{
            "point": HELP_SOURCES, "id": "remote-docs", "priority": 50,
            "entry": { "id": "remote", "title": "Remote docs" }
        }] }
    });
    let dir = package("help-sources-bad", &manifest);
    let mut session = FakeSession::new();
    session.install_core("keys-and-help");
    assert!(session.try_install(&dir).is_err());
}

/// REQ: F-010
#[test]
fn points_010_with_no_contributor_both_points_are_empty() {
    let mut session = session_with(&[]);
    assert!(session.collected(KEY_HINTS).is_empty());
    assert!(session.collected(HELP_SOURCES).is_empty());
}

/// REQ: F-010
#[test]
fn points_010_the_core_stub_declares_exactly_the_two_points_and_the_bindings_service() {
    let stub = ilium_plugin_sdk::testing::core_stubs::find("keys-and-help").expect("stub");
    assert_eq!(
        stub.extension_points,
        vec!["key-hints".to_string(), "help-sources".to_string()]
    );
    assert_eq!(stub.services, vec!["bindings".to_string()]);
}

/// REQ: F-010
#[test]
fn points_010_the_page_table_lists_the_payload_fields_of_both_points() {
    let rows = table_rows(&page_text(), "| Point | Kind |");
    assert_eq!(rows[0][3], "entry: id, context, chord, label");
    assert_eq!(rows[1][3], "entry: id, title, service");
}

/// REQ: F-010
#[test]
fn points_010_the_manifest_of_the_owner_declares_the_two_points_with_the_provide_grants() {
    let grants = client_grants();
    for point in ["key-hints", "help-sources"] {
        assert!(
            grants.contains(&format!("extensions.provide:{PLUGIN_ID}/{point}")),
            "{point} has no extensions.provide grant"
        );
    }
}

/// REQ: F-010
#[test]
fn points_010_the_manifest_of_the_owner_lists_the_points_under_contributes_extension_points() {
    let points = manifest()["contributes"]["extensionPoints"].clone();
    let ids: Vec<&str> = points
        .as_array()
        .expect("points")
        .iter()
        .filter_map(|point| point["id"].as_str())
        .collect();
    assert_eq!(ids, vec!["key-hints", "help-sources"]);
}

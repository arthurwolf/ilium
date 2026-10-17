//! The manifest is exact (page "Complete manifest", COR-050) and the grants are as the page states.

use serde_json::Value;

use crate::common::*;

fn page_manifest() -> Value {
    let blocks = json_blocks(&page_text());
    // blocks[0] is the fragment, blocks[1] the complete manifest, later blocks are samples.
    serde_json::from_str(&blocks[1]).expect("complete manifest on the page is JSON")
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .expect("array")
        .iter()
        .map(|item| item.as_str().expect("string").to_string())
        .collect()
}

/// REQ: COR-050, F-041
#[test]
fn doc_cor_050_manifest_file_equals_complete_manifest_on_page() {
    assert_eq!(manifest_value(), page_manifest());
}

/// REQ: COR-050, F-041
#[test]
fn doc_cor_050_fragment_fields_match_complete_manifest() {
    let blocks = json_blocks(&page_text());
    let fragment: Value = serde_json::from_str(&blocks[0]).expect("fragment is JSON");
    let complete = page_manifest();
    assert_eq!(fragment["api"], complete["api"]);
    assert_eq!(fragment["id"], complete["id"]);
    assert_eq!(
        fragment["capabilities"]["server"],
        complete["capabilities"]["server"]
    );
}

/// REQ: COR-050, F-041
#[test]
fn doc_cor_050_fragment_client_grants_are_a_prefix_of_the_complete_list() {
    let blocks = json_blocks(&page_text());
    let fragment: Value = serde_json::from_str(&blocks[0]).expect("fragment is JSON");
    let complete = page_manifest();
    let short = strings(&fragment["capabilities"]["client"]);
    let full = strings(&complete["capabilities"]["client"]);
    assert_eq!(short, ["ui.panels"]);
    assert_eq!(&full[..short.len()], short.as_slice());
}

/// REQ: COR-050, F-041
#[test]
fn doc_cor_050_identity_fields_are_exact() {
    let manifest = manifest_value();
    assert_eq!(manifest["api"], "ilium:plugin@0.1.0");
    assert_eq!(manifest["id"], PLUGIN_ID);
    assert_eq!(manifest["name"], "Titles");
    assert_eq!(manifest["version"], "0.1.0");
    assert_eq!(manifest["publisher"], "Ilium");
    assert_eq!(manifest["license"], "MIT");
    assert_eq!(manifest["limits_tier"], "first-party");
}

/// REQ: COR-050, F-041
#[test]
fn doc_cor_050_description_is_the_first_sentence_of_the_page() {
    let manifest = manifest_value();
    let description = manifest["description"].as_str().expect("description");
    assert!(page_text().contains(&format!("## Titles and organisation\n\n{description}")));
}

/// REQ: COR-050, F-041
#[test]
fn doc_cor_050_server_entry_and_world_are_exact() {
    let manifest = manifest_value();
    assert_eq!(manifest["server"]["entry"], "server.wasm");
    assert_eq!(manifest["server"]["world"], "ilium:server/base@0.1.0");
}

/// REQ: COR-050, F-041
#[test]
fn doc_cor_050_server_grants_are_exactly_the_six_listed_in_order() {
    let manifest = manifest_value();
    assert_eq!(
        strings(&manifest["capabilities"]["server"]),
        [
            "inference",
            "agent.read-prompts",
            "tree.read",
            "tree.write",
            "tree.decorate",
            "tree.restructure"
        ]
    );
}

/// REQ: COR-050, COR-030, F-044
#[test]
fn doc_cor_050_client_grants_hold_the_panel_and_the_two_kernel_tabs() {
    let manifest = manifest_value();
    assert_eq!(
        strings(&manifest["capabilities"]["client"]),
        [
            "ui.panels",
            "settings.write-kernel:titles",
            "settings.write-kernel:triggers"
        ]
    );
}

/// REQ: COR-050, F-041
#[test]
fn doc_cor_050_activation_is_startup_only() {
    let manifest = manifest_value();
    assert_eq!(strings(&manifest["activation"]), ["startup"]);
}

/// REQ: COR-050, F-041
#[test]
fn doc_cor_050_no_grant_is_listed_twice_and_no_grant_has_an_argument_except_kernel_tabs() {
    let manifest = manifest_value();
    for side in ["server", "client"] {
        let grants = strings(&manifest["capabilities"][side]);
        let mut unique = grants.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), grants.len(), "duplicate grant on {side}");
        for grant in grants {
            assert!(
                !grant.contains(':') || grant.starts_with("settings.write-kernel:"),
                "{grant}"
            );
        }
    }
}

/// REQ: COR-050, F-043
#[test]
fn doc_cor_050_tree_restructure_is_the_only_high_risk_grant_and_the_page_says_so() {
    let page = page_text();
    assert!(page.contains(
        "`tree.restructure` is a high-risk grant: the user confirms it at install. Everything else is normal risk."
    ));
    let grants = strings(&manifest_value()["capabilities"]["server"]);
    assert!(grants.contains(&"tree.restructure".to_string()));
}

/// REQ: COR-050, F-041
#[test]
fn doc_cor_050_samples_use_only_granted_server_capabilities() {
    let granted = strings(&manifest_value()["capabilities"]["server"]);
    let page = page_text();
    for line in page
        .lines()
        .filter(|line| line.contains("// manifest: \"capabilities\""))
    {
        for word in [
            "inference",
            "agent.read-prompts",
            "tree.read",
            "tree.write",
            "tree.decorate",
            "tree.restructure",
        ] {
            if line.contains(&format!("\"{word}\"")) {
                assert!(granted.contains(&word.to_string()), "{word}");
            }
        }
    }
}

/// REQ: COR-050, F-041
#[test]
fn doc_cor_050_page_front_matter_names_the_permalink() {
    let page = page_text();
    assert!(page.contains("permalink: /pages/core-plugin-titles/"));
    assert!(page.contains("title: Titles and organisation"));
}

/// REQ: COR-050, F-041
#[test]
fn doc_cor_050_startup_activation_matches_the_boot_table_row() {
    let boot = pages_text("reference-kernel-and-boot.md");
    let row = table_row(&boot, "| Titles |");
    assert_eq!(row[2], "`startup`");
    assert_eq!(row[4], "Triggers fire on agent events from the start");
}

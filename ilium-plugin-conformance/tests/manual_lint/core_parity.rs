//! Core-parity doc lints: feature traceability (COR-002, 003), core-plugin manifests (COR-043,
//! 050) and roadmap waves (COR-060, 061). Channel: the manual markdown tables.

use std::collections::BTreeMap;
use std::sync::Arc;

use ilium_plugin_host::cli::{self, CliEnv};
use ilium_plugin_host::registry_client::{FetchError, Fetcher};
use ilium_plugin_testkit::manual::pages_dir;
use serde_json::Value;

struct Offline;

impl Fetcher for Offline {
    fn get(&self, _url: &str) -> Result<Vec<u8>, FetchError> {
        Err(FetchError::Unreachable)
    }
}

fn page(name: &str) -> String {
    std::fs::read_to_string(pages_dir().join(format!("{name}.md")))
        .unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// Cells of every table row after the header of the first table whose header starts with `first`.
fn table_rows(text: &str, first: &str) -> (Vec<String>, Vec<Vec<String>>) {
    let mut lines = text
        .lines()
        .skip_while(|l| !l.starts_with(&format!("| {first}")));
    let header: Vec<String> = lines
        .next()
        .expect("header")
        .trim_matches('|')
        .split('|')
        .map(|c| c.trim().to_owned())
        .collect();
    let rows = lines
        .skip(1)
        .take_while(|l| l.starts_with('|'))
        .map(|l| {
            l.trim()
                .trim_start_matches('|')
                .trim_end_matches('|')
                .split('|')
                .map(|c| c.trim().to_owned())
                .collect()
        })
        .collect();
    (header, rows)
}

fn column(header: &[String], name: &str) -> usize {
    header
        .iter()
        .position(|h| h == name)
        .unwrap_or_else(|| panic!("column {name} in {header:?}"))
}

fn core_plugin_pages() -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(pages_dir())
        .unwrap()
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| n.starts_with("core-plugin-") && n.ends_with(".md"))
        .map(|n| n.trim_end_matches(".md").to_owned())
        .collect();
    names.sort();
    names
}

fn complete_manifest(text: &str) -> Option<Value> {
    let after = text.split("### Complete manifest").nth(1)?;
    let block = after.split("```json").nth(1)?.split("```").next()?;
    serde_json::from_str(block).ok()
}

/// REQ: COR-002
#[test]
fn cor_002_traceability_has_116_rows_with_unique_ids_and_a_non_empty_owner() {
    let (header, rows) = table_rows(&page("reference-feature-traceability"), "Id");
    let (id, owner) = (column(&header, "Id"), column(&header, "Owner"));
    assert_eq!(rows.len(), 116);
    let mut seen = BTreeMap::new();
    for row in &rows {
        *seen.entry(row[id].clone()).or_insert(0) += 1;
        assert!(
            !row[owner].is_empty() && row[owner] != "none",
            "{} has no owner",
            row[id]
        );
    }
    assert!(
        seen.values().all(|count| *count == 1),
        "duplicate ids: {seen:?}"
    );
}

/// REQ: COR-002
#[test]
fn cor_002_the_duplicates_section_lists_no_open_claim() {
    let text = page("reference-feature-traceability");
    let section = text
        .split("### Resolved duplicates")
        .nth(1)
        .expect("section");
    let section = section.split("\n## ").next().unwrap();
    assert!(
        !section.to_lowercase().contains("open claim: yes")
            && !section.contains("TODO")
            && !section.contains("unresolved")
    );
}

/// REQ: COR-003
#[test]
fn cor_003_native_kernel_rows_name_kernel_first_in_the_owner_cell() {
    let (header, rows) = table_rows(&page("reference-feature-traceability"), "Id");
    let (id, kind, owner) = (
        column(&header, "Id"),
        column(&header, "Kind"),
        column(&header, "Owner"),
    );
    let kernel_rows: Vec<&Vec<String>> =
        rows.iter().filter(|r| r[kind] == "native-kernel").collect();
    assert!(!kernel_rows.is_empty());
    for row in kernel_rows {
        assert!(
            row[owner].starts_with("kernel"),
            "{}: {}",
            row[id],
            row[owner]
        );
    }
}

/// REQ: COR-003
#[test]
fn cor_003_every_config_table_named_appears_in_the_migration_key_table() {
    let (header, rows) = table_rows(&page("reference-feature-traceability"), "Id");
    let config = column(&header, "Config keys");
    let migration = page("reference-config-and-data-migration");
    for row in rows {
        for token in row[config]
            .split([',', ' '])
            .filter(|t| t.starts_with('[') && t.ends_with(']'))
        {
            assert!(
                migration.contains(token),
                "{}: table {token} missing from the migration page",
                row[0]
            );
        }
    }
}

/// REQ: COR-043
#[test]
fn cor_043_every_core_plugin_complete_manifest_passes_check_without_manifest_errors() {
    let pages = core_plugin_pages();
    assert_eq!(pages.len(), 30);
    for name in pages {
        let manifest = complete_manifest(&page(&name))
            .unwrap_or_else(|| panic!("{name}: no parseable Complete manifest block"));
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("project");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(
            project.join("ilium-plugin.json"),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();
        let env = CliEnv::new(
            root.path().join("d"),
            root.path().join("s"),
            root.path().join("c"),
            project.clone(),
            std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_893_499_200),
            true,
            Arc::new(Offline),
        );
        let args: Vec<String> = ["check", project.to_str().unwrap(), "--json"]
            .iter()
            .map(|s| (*s).to_owned())
            .collect();
        let output = cli::run(&args, &env);
        let bad: Vec<&Value> = output
            .stdout_jsonl
            .iter()
            .filter(|r| {
                r["type"] == "check"
                    && r["severity"] == "error"
                    && r["rule"]
                        .as_str()
                        .is_some_and(|rule| rule.starts_with("manifest."))
            })
            .collect();
        assert!(bad.is_empty(), "{name}: {bad:?}");
    }
}

/// REQ: COR-043
#[test]
fn cor_043_a_page_with_a_broken_manifest_would_be_detected() {
    let mut manifest = complete_manifest(&page("core-plugin-inference")).expect("manifest");
    manifest["id"] = Value::String("Not A Valid Id".to_owned());
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join("ilium-plugin.json"), manifest.to_string()).unwrap();
    let env = CliEnv::new(
        root.path().join("d"),
        root.path().join("s"),
        root.path().join("c"),
        project.clone(),
        std::time::UNIX_EPOCH,
        true,
        Arc::new(Offline),
    );
    let args: Vec<String> = ["check", project.to_str().unwrap(), "--json"]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
    assert!(cli::run(&args, &env)
        .stdout_jsonl
        .iter()
        .any(|r| r["severity"] == "error"
            && r["rule"]
                .as_str()
                .is_some_and(|rule| rule.starts_with("manifest."))));
}

/// REQ: COR-050
#[test]
fn cor_050_there_are_30_pages_each_with_a_complete_manifest_and_the_index_states_30() {
    let pages = core_plugin_pages();
    assert_eq!(pages.len(), 30);
    for name in &pages {
        assert!(page(name).contains("### Complete manifest"), "{name}");
    }
    let index = page("core-plugins");
    assert!(index.contains("all 30 of them"));
    let (_, rows) = table_rows(&index, "Plugin");
    assert_eq!(rows.len(), pages.len());
}

/// REQ: COR-050
#[test]
fn cor_050_manifest_grants_appear_on_the_page_that_declares_them() {
    for name in core_plugin_pages() {
        let text = page(&name);
        let manifest = complete_manifest(&text).unwrap_or_else(|| panic!("{name}: manifest"));
        for side in ["server", "client", "scene"] {
            for grant in manifest["capabilities"][side]
                .as_array()
                .cloned()
                .unwrap_or_default()
            {
                let grant = grant.as_str().unwrap();
                let bare = grant.split(':').next().unwrap();
                assert!(
                    text.contains(bare),
                    "{name}: grant {grant} is not described on the page"
                );
            }
        }
    }
}

/// REQ: COR-050
#[test]
fn cor_050_manifest_id_matches_the_page_name() {
    for name in core_plugin_pages() {
        let manifest =
            complete_manifest(&page(&name)).unwrap_or_else(|| panic!("{name}: manifest"));
        let expected = format!("ilium.core.{}", name.trim_start_matches("core-plugin-"));
        assert_eq!(manifest["id"], expected.as_str(), "{name}");
    }
}

fn wave_rows() -> (Vec<String>, Vec<Vec<String>>) {
    table_rows(&page("roadmap"), "Wave | Core plugins")
}

/// REQ: COR-060
#[test]
fn cor_060_every_wave_row_names_a_gate_test() {
    let (header, rows) = wave_rows();
    let gate = column(&header, "Gate test");
    assert!(rows.len() >= 6);
    for row in rows {
        let cell = &row[gate];
        assert!(
            cell.starts_with('`') && cell[1..].contains('`'),
            "wave {}: {cell:?}",
            row[0]
        );
        assert!(
            cell.split('`').nth(1).unwrap().starts_with("wave-"),
            "wave {}: {cell:?}",
            row[0]
        );
    }
}

/// REQ: COR-060
#[test]
fn cor_060_gate_test_names_are_unique_across_waves() {
    let (header, rows) = wave_rows();
    let gate = column(&header, "Gate test");
    let mut names: Vec<String> = rows
        .iter()
        .map(|r| r[gate].split('`').nth(1).unwrap().to_owned())
        .collect();
    let total = names.len();
    names.sort();
    names.dedup();
    assert_eq!(names.len(), total);
}

/// REQ: COR-061
#[test]
fn cor_061_every_wave_row_states_what_stays_native() {
    let (header, rows) = wave_rows();
    let native = column(&header, "What stays native");
    for row in rows {
        assert!(
            !row[native].is_empty() && row[native] != "none",
            "wave {}",
            row[0]
        );
    }
}

/// REQ: COR-061
#[test]
fn cor_061_the_wave_table_is_followed_by_no_row_with_a_missing_cell() {
    let (header, rows) = wave_rows();
    assert!(rows.iter().all(|r| r.len() == header.len()));
}

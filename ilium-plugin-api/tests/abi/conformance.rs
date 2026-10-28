//! ABI-030: the host conformance matrices name every import interface, export, event, intent
//! and widget of the WIT, once. Doc lints.

use std::collections::BTreeSet;

use ilium_plugin_api::generated::types::{Event, Intent, Widget};

use crate::support::{manual_page, wit_files};

/// The names in column 2 of the numbered rows of one `## N. ...` section.
fn matrix(heading: &str) -> Vec<String> {
    let page = manual_page("reference-host-conformance.md");
    let section = page
        .split("\n## ")
        .find(|section| section.starts_with(heading))
        .unwrap_or_else(|| panic!("section {heading} of reference-host-conformance.md"));
    section
        .lines()
        .filter_map(|line| {
            let cells: Vec<&str> = line.split('|').map(str::trim).collect();
            let number = cells.get(1)?;
            if number.is_empty() || !number.chars().all(|c| c.is_ascii_digit()) {
                return None;
            }
            let name = cells.get(2)?.strip_prefix('`')?;
            let name = name.split('`').next()?;
            Some(name.trim_start_matches('%').to_owned())
        })
        .collect()
}

/// Interface names declared in the wit/ files under `directory`.
fn interfaces(directory: &str) -> BTreeSet<String> {
    wit_files()
        .into_iter()
        .filter(|(relative, _)| relative.starts_with(&format!("{directory}/")))
        .flat_map(|(_, text)| {
            text.lines()
                .filter_map(|line| line.strip_prefix("interface "))
                .filter_map(|rest| rest.split_whitespace().next())
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .collect()
}

fn assert_same_set(name: &str, rows: &[String], expected: BTreeSet<String>) {
    let listed: BTreeSet<String> = rows.iter().cloned().collect();
    assert_eq!(listed.len(), rows.len(), "{name}: a row is repeated");
    let missing: Vec<_> = expected.difference(&listed).collect();
    let extra: Vec<_> = listed.difference(&expected).collect();
    assert!(
        missing.is_empty() && extra.is_empty(),
        "{name}: missing {missing:?}, unknown {extra:?}"
    );
}

fn cases(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

/// REQ: ABI-030
#[test]
fn doc_abi_030_import_matrix_has_45_rows() {
    assert_eq!(matrix("2. Import matrix").len(), 45);
}

/// REQ: ABI-030
#[test]
fn doc_abi_030_import_matrix_names_every_host_and_primitive_interface() {
    let rows = matrix("2. Import matrix");
    let listed: BTreeSet<String> = rows.iter().cloned().collect();
    let anim = interfaces("anim");
    let mut expected = interfaces("host");
    expected.extend(interfaces("primitives"));
    assert_eq!(interfaces("host").len(), 39);
    assert_eq!(interfaces("primitives").len(), 4);
    let missing: Vec<_> = expected.difference(&listed).collect();
    assert!(missing.is_empty(), "imports without a row: {missing:?}");
    let unknown: Vec<_> = listed
        .iter()
        .filter(|row| !expected.contains(*row) && !anim.contains(*row))
        .collect();
    assert!(
        unknown.is_empty(),
        "rows for unknown interfaces: {unknown:?}"
    );
    assert_eq!(listed.intersection(&anim).count(), 2, "scene imports");
}

/// REQ: ABI-030
#[test]
fn doc_abi_030_export_matrix_names_every_export_interface() {
    let rows = matrix("3. Export matrix");
    assert_eq!(rows.len(), 16);
    assert_same_set("exports", &rows, interfaces("exports"));
}

/// REQ: ABI-030
#[test]
fn doc_abi_030_event_matrix_names_every_event_case() {
    let rows = matrix("4. Event matrix");
    assert_eq!(rows.len(), 56);
    assert_same_set("events", &rows, cases(Event::CASE_NAMES));
}

/// REQ: ABI-030
#[test]
fn doc_abi_030_intent_matrix_names_every_intent_case() {
    let rows = matrix("5. Intent matrix");
    assert_eq!(rows.len(), 42);
    assert_same_set("intents", &rows, cases(Intent::CASE_NAMES));
}

/// REQ: ABI-030
#[test]
fn doc_abi_030_widget_matrix_names_every_widget_case() {
    let rows = matrix("6. Widget matrix");
    assert_eq!(rows.len(), 51);
    assert_same_set("widgets", &rows, cases(Widget::CASE_NAMES));
}

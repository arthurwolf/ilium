//! Requirement-gap tests for the package-signing lane (SEC-166, SEC-168).
//!
//! These are the CI-style checks named in the manual: the sandbox mitigation table must have a
//! row for every host interface of `ilium:host@0.1.0` and for every high-risk grant, and the
//! threat model must assign every host interface to a trust boundary. The checks read the manual
//! pages through the shared `page` helper, so they run without the `testing` feature.

use super::signing_support::page;
use std::collections::BTreeSet;

/// Interface names declared inside the `ilium:host@0.1.0` package of the WIT reference page.
fn host_interfaces() -> Vec<String> {
    let text = page("reference-wit.md");
    let mut names = Vec::new();
    let mut inside_host_package = false;
    for line in text.lines() {
        if line.starts_with("package ") {
            inside_host_package = line.trim_end() == "package ilium:host@0.1.0;";
            continue;
        }
        if inside_host_package {
            if let Some(rest) = line.strip_prefix("interface ") {
                if let Some(name) = rest.split(' ').next() {
                    names.push(name.to_owned());
                }
            }
        }
    }
    names
}

/// The first cell of every markdown table row, with backticks and spaces removed.
fn table_first_cells(text: &str) -> BTreeSet<String> {
    text.lines()
        .filter(|line| line.starts_with('|'))
        .filter_map(|line| line.split('|').nth(1))
        .map(|cell| cell.trim().trim_matches('`').trim().to_owned())
        .collect()
}

/// The grant name without its argument: `settings.write-kernel:<tab-id>` gives
/// `settings.write-kernel`.
fn grant_base(cell: &str) -> &str {
    cell.split(|c| c == ':' || c == '<').next().unwrap_or(cell)
}

/// Whole-word membership of `word` in `text` (words are split on anything but letters, digits,
/// `-` and `.`), so `tree` does not match inside `tree-view`.
fn mentions_word(text: &str, word: &str) -> bool {
    text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '.'))
        .any(|token| token == word)
}

/// The high-risk grants named in the capability page's high-risk paragraph. They must each have
/// a row in the sandbox mitigation table.
const HIGH_RISK_GRANTS: [&str; 12] = [
    "terminal.history",
    "terminal.input-lock",
    "terminal.raw-input",
    "panes.lifecycle",
    "tree.restructure",
    "sessions.cross",
    "process.long-lived",
    "agent.transcript",
    "settings.read-kernel",
    "settings.write-kernel",
    "panes.create-command",
    "agent-config.propose",
];

/// REQ: SEC-166
#[test]
fn req_sec_166_mitigation_table_has_a_row_for_every_host_interface_a() {
    let interfaces = host_interfaces();
    assert!(
        !interfaces.is_empty(),
        "reference-wit.md must list the ilium:host@0.1.0 interfaces"
    );
    let sandbox_cells = table_first_cells(&page("reference-sandbox.md"));
    let missing: Vec<&String> = interfaces
        .iter()
        .filter(|name| !sandbox_cells.contains(name.as_str()))
        .collect();
    assert!(
        missing.is_empty(),
        "every ilium:host@0.1.0 interface needs a row in the SEC-030 mitigation table \
         (reference-sandbox.md); missing: {missing:?}"
    );
}

/// REQ: SEC-166
#[test]
fn req_sec_166_removing_a_high_risk_grant_row_is_detected_b() {
    let sandbox_bases: BTreeSet<String> = table_first_cells(&page("reference-sandbox.md"))
        .iter()
        .map(|cell| grant_base(cell).to_owned())
        .collect();
    let missing: Vec<&str> = HIGH_RISK_GRANTS
        .iter()
        .copied()
        .filter(|grant| !sandbox_bases.contains(*grant))
        .collect();
    assert!(
        missing.is_empty(),
        "every high-risk (H) grant needs a row in the SEC-030 mitigation table; missing: {missing:?}"
    );
    assert!(
        sandbox_bases.contains("keys.intercept"),
        "the keys.intercept row of the SEC-030 mitigation table must exist"
    );
}

/// REQ: SEC-168
#[test]
fn req_sec_168_threat_model_assigns_every_host_interface_to_a_boundary_a() {
    let interfaces = host_interfaces();
    assert!(
        !interfaces.is_empty(),
        "reference-wit.md must list the ilium:host@0.1.0 interfaces"
    );
    let threat_model = page("reference-threat-model.md");
    let unassigned: Vec<&String> = interfaces
        .iter()
        .filter(|name| !mentions_word(&threat_model, name))
        .collect();
    assert!(
        unassigned.is_empty(),
        "every ilium:host@0.1.0 interface must be named in the boundary table of \
         reference-threat-model.md; unassigned: {unassigned:?}"
    );
}

/// REQ: SEC-168
#[test]
fn req_sec_168_dropping_the_media_row_from_the_boundary_table_is_detected_b() {
    let threat_model = page("reference-threat-model.md");
    assert!(
        mentions_word(&threat_model, "media"),
        "the media interface must be assigned to a boundary in reference-threat-model.md"
    );
    for boundary in 1..=8 {
        let label = format!("B{boundary}");
        assert!(
            mentions_word(&threat_model, &label),
            "trust boundary {label} must appear in reference-threat-model.md"
        );
    }
}

/// The single manual paragraph that starts with `prefix`, read from the threat model page.
fn threat_model_paragraph(prefix: &str) -> String {
    page("reference-threat-model.md")
        .lines()
        .find(|line| line.starts_with(prefix))
        .unwrap_or_else(|| panic!("reference-threat-model.md must contain a paragraph starting with {prefix:?}"))
        .to_owned()
}

/// REQ: SEC-160
#[test]
fn req_sec_160_data_read_list_names_content_delivering_grants_a() {
    let data_read = threat_model_paragraph("Read grants of normal risk expose private data:");
    for grant in ["editor.read", "terminal.output-filter", "fs.watch:"] {
        assert!(
            data_read.contains(grant),
            "SEC-160: the data-read list of SEC-004 must contain {grant}; list: {data_read}"
        );
    }
}

/// REQ: SEC-160
#[test]
fn req_sec_160_data_read_list_excludes_package_data_and_cache_scopes_b() {
    let data_read = threat_model_paragraph("Read grants of normal risk expose private data:");
    for scope in ["fs.read:package", "fs.read:data", "fs.read:cache"] {
        assert!(
            !data_read.contains(scope),
            "SEC-160: the fs.read scopes package, data and cache are not part of the data-read list; found {scope} in: {data_read}"
        );
    }
}

/// REQ: SEC-161
#[test]
fn req_sec_161_egress_list_names_open_external_location_inference_and_http_api_a() {
    let egress = threat_model_paragraph("Egress grants of normal risk move data off the machine:");
    for grant in [
        "open-external",
        "location.geocode",
        "inference.embed",
        "http-api.route",
    ] {
        assert!(
            egress.contains(grant),
            "SEC-161: the egress list of SEC-004 must contain {grant}; list: {egress}"
        );
    }
}

/// REQ: SEC-161
#[test]
fn req_sec_161_egress_additions_are_written_as_grant_tokens_b() {
    let egress = threat_model_paragraph("Egress grants of normal risk move data off the machine:");
    for grant in ["open-external", "location.geocode", "inference.embed", "http-api.route"] {
        let token = format!("`{grant}`");
        assert!(
            egress.contains(&token),
            "SEC-161: the egress list must name {token} as a grant token, not prose; list: {egress}"
        );
    }
}

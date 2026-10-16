//! reference-security-response.md: the project's security contact, response times, advisories,
//! supported versions and the severity scale. These are process commitments, so the tests check
//! the repository artefacts and the manual text that make them checkable.

use crate::support::*;

fn security_md() -> String {
    std::fs::read_to_string(workspace_root().join("SECURITY.md")).unwrap_or_default()
}

/// REQ: SEC-320
#[test]
fn sec_320_security_md_at_the_repository_root_names_the_contact_and_a_pgp_key() {
    let text = security_md();
    assert!(text.contains("security@ilium.example") || text.contains('@'), "SECURITY.md needs a contact");
    assert!(text.to_lowercase().contains("pgp"));
    assert!(text.to_lowercase().contains("not to public issues") || text.to_lowercase().contains("public issue"));
}

/// REQ: SEC-320
#[test]
fn sec_320_version_verbose_prints_the_contact() {
    let scratch = scratch();
    let output = ilium_plugin_host::cli::run(&args(&["--version", "--verbose"]), &cli_env(scratch.path()));
    let text = format!("{:?} {}", output.stdout_jsonl, output.stderr);
    assert!(text.contains('@'), "{text}");
}

/// REQ: SEC-321
#[test]
fn sec_321_security_md_states_the_response_times_and_the_embargo() {
    let text = security_md();
    for needle in ["3 working days", "10 working days", "14 days", "90 days"] {
        assert!(text.contains(needle), "SECURITY.md lacks {needle}");
    }
}

/// REQ: SEC-321
#[test]
fn sec_321_the_manual_states_no_legal_action_against_good_faith_research() {
    assert_page_has("reference-security-response.md", &["does not take legal action against good-faith research"]);
}

/// REQ: SEC-322
#[test]
fn sec_322_an_advisory_template_exists_with_every_required_field() {
    let template = std::fs::read_to_string(workspace_root().join(".github/SECURITY_ADVISORY_TEMPLATE.md")).unwrap_or_default();
    for needle in ["Affected versions", "Fixed versions", "Severity", "Description", "Workaround", "Credit", "Requirement ids"] {
        assert!(template.contains(needle), "template lacks {needle}");
    }
}

/// REQ: SEC-322
#[test]
fn sec_322_the_manual_requires_a_cve_a_ghsa_advisory_and_a_release_note_entry() {
    assert_page_has("reference-security-response.md", &["CVE identifier", "security advisory", "release notes"]);
}

/// REQ: SEC-323
#[test]
fn sec_323_the_supported_versions_are_the_latest_minor_and_the_one_before() {
    assert_page_has("reference-security-response.md", &["The latest minor release of Ilium and the one before it receive security fixes."]);
    let text = security_md();
    assert!(text.contains("latest minor") || text.contains("Supported versions"));
}

/// REQ: SEC-324
#[test]
fn sec_324_the_scale_has_four_levels_with_the_documented_examples() {
    let page = manual_page("reference-security-response.md");
    let rows: Vec<&str> = page.lines().skip_while(|line| !line.starts_with("| Level | Examples")).skip(2).take_while(|line| line.starts_with('|')).collect();
    let levels: Vec<String> = rows.iter().map(|row| row.split('|').nth(1).expect("level").trim().to_owned()).collect();
    assert_eq!(levels, vec!["Critical", "High", "Medium", "Low"]);
    assert!(rows[0].contains("sandbox escape") && rows[0].contains("HTTP API token"));
    assert!(rows[1].contains("grant check bypass") && rows[1].contains("signature bypass"));
    assert!(rows[2].contains("denial of service") && rows[2].contains("spoofing of a kernel dialog"));
    assert!(rows[3].contains("missing audit line") && rows[3].contains("wrong error code"));
}

/// REQ: SEC-324
#[test]
fn sec_324_the_security_policy_uses_the_same_four_levels() {
    let text = security_md();
    for level in ["Critical", "High", "Medium", "Low"] {
        assert!(text.contains(level), "SECURITY.md lacks {level}");
    }
}
